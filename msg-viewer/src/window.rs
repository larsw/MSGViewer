use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gdk, gio, glib};
use outlook_msg::{Attachment, Message};

use crate::application::Application;
use crate::find_bar::FindBar;
use crate::message_view::MessageView;

/// Actions that only make sense while a Message is shown.
const MESSAGE_ACTIONS: [&str; 6] = [
    "win.find",
    "win.find-next",
    "win.find-previous",
    "win.zoom-in",
    "win.zoom-out",
    "win.zoom-reset",
];

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/window.ui")]
    pub struct Window {
        #[template_child]
        pub find_bar: TemplateChild<FindBar>,
        #[template_child]
        pub partial_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub message_view: TemplateChild<MessageView>,
        #[template_child]
        pub error_page: TemplateChild<adw::StatusPage>,
        /// The file shown in this window, if any.
        pub file: RefCell<Option<gio::File>>,
        /// Whether this window shows an Embedded message, which has no file.
        pub shows_embedded_message: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "MsgViewerWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            MessageView::ensure_type();
            FindBar::ensure_type();
            klass.bind_template();
            klass.install_action_async("win.open", None, |win, _, _| async move {
                win.choose_file().await;
            });
            klass.install_action("win.find", None, |win, _, _| win.imp().find_bar.open());
            klass.install_action("win.find-next", None, |win, _, _| {
                win.imp().find_bar.find_next();
            });
            klass.install_action("win.find-previous", None, |win, _, _| {
                win.imp().find_bar.find_previous();
            });
            klass.install_action("win.zoom-in", None, |win, _, _| {
                win.imp().message_view.body_view().zoom_in();
            });
            klass.install_action("win.zoom-out", None, |win, _, _| {
                win.imp().message_view.body_view().zoom_out();
            });
            klass.install_action("win.zoom-reset", None, |win, _, _| {
                win.imp().message_view.body_view().zoom_reset();
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_conveniences();
        }
    }
    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl Window {
    pub fn new(app: &Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    /// Reads `file` and shows it in this window.
    pub fn open_file(&self, file: gio::File) {
        self.imp().file.replace(Some(file.clone()));
        let win = self.clone();
        glib::spawn_future_local(async move {
            match load_message(&file).await {
                Ok(message) => win.show_message(&message),
                Err(error) => {
                    // Nothing is shown, so the next file can reuse this window.
                    win.imp().file.replace(None);
                    win.show_error(&error);
                }
            }
        });
    }

    /// Reads the Embedded message carried by `attachment` and shows it in
    /// this window. There is no file behind it, so the window is titled
    /// with the Attachment's name until the Message's subject is known.
    pub fn open_embedded(&self, attachment: Attachment) {
        self.imp().shows_embedded_message.set(true);
        self.set_title(Some(&attachment.safe_file_name()));
        let win = self.clone();
        glib::spawn_future_local(async move {
            match read_embedded(attachment).await {
                Ok(message) => win.show_message(&message),
                Err(error) => {
                    win.imp().shows_embedded_message.set(false);
                    win.show_error(&error);
                }
            }
        });
    }

    /// Whether this window shows nothing yet, so the next file can use it.
    fn is_empty(&self) -> bool {
        let imp = self.imp();
        imp.file.borrow().is_none() && !imp.shows_embedded_message.get()
    }

    fn show_message(&self, message: &Message) {
        let imp = self.imp();
        if let Some(subject) = message.subject() {
            self.set_title(Some(subject));
        }
        for problem in message.problems() {
            glib::g_debug!("msg-viewer", "Partial read: {problem}");
        }
        imp.partial_banner.set_revealed(message.is_partial());
        imp.message_view.set_message(message);
        imp.stack.set_visible_child_name("message");
    }

    fn show_error(&self, error: &str) {
        let imp = self.imp();
        imp.partial_banner.set_revealed(false);
        imp.error_page.set_description(Some(error));
        imp.stack.set_visible_child_name("error");
    }

    async fn choose_file(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Outlook Messages")));
        filter.add_mime_type("application/vnd.ms-outlook");
        filter.add_suffix("msg");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);

        let dialog = gtk::FileDialog::builder()
            .title(gettext("Open Message"))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let file = match dialog.open_future(Some(self)).await {
            Ok(file) => file,
            Err(error) if error.matches(gtk::DialogError::Dismissed) => return,
            Err(error) => {
                glib::g_warning!("msg-viewer", "Could not choose a file: {error}");
                return;
            }
        };

        self.open_here_or_in_new_window(file);
    }

    /// Each file gets its own window; this one is reused only while it is
    /// empty.
    fn open_here_or_in_new_window(&self, file: gio::File) {
        if self.is_empty() {
            self.open_file(file);
        } else if let Some(app) = self.application().and_downcast::<Application>() {
            let window = Window::new(&app);
            window.present();
            window.open_file(file);
        }
    }
}

/// Dropping files, finding text and zooming.
impl Window {
    fn setup_conveniences(&self) {
        let imp = self.imp();
        imp.find_bar.set_body_view(&imp.message_view.body_view());
        imp.stack.connect_visible_child_name_notify(glib::clone!(
            #[weak(rename_to = win)]
            self,
            move |_| win.update_message_actions()
        ));
        self.update_message_actions();
        self.setup_drop_target();
    }

    fn update_message_actions(&self) {
        let showing_message = self.imp().stack.visible_child_name().as_deref() == Some("message");
        for action in MESSAGE_ACTIONS {
            self.action_set_enabled(action, showing_message);
        }
        if !showing_message {
            self.imp().find_bar.close();
        }
    }

    /// Opens files dropped anywhere on the window, the Body included.
    fn setup_drop_target(&self) {
        let Some(content) = self.content() else {
            return;
        };
        let target = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
        target.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
        // Ahead of the Body's web view, which would take the drop itself.
        target.set_propagation_phase(gtk::PropagationPhase::Capture);
        target.connect_drop(glib::clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                let files = dropped_files(value);
                for file in &files {
                    win.open_here_or_in_new_window(file.clone());
                }
                !files.is_empty()
            }
        ));
        // Highlighted by style.css while a drag is over it.
        content.add_css_class("drop-target");
        content.add_controller(target);
    }
}

/// The files in a drop, which is either a list of files or one file.
fn dropped_files(value: &glib::Value) -> Vec<gio::File> {
    if let Ok(list) = value.get::<gdk::FileList>() {
        list.files()
    } else if let Ok(file) = value.get::<gio::File>() {
        vec![file]
    } else {
        Vec::new()
    }
}

/// Reads a Message, or says in plain words why the file is unreadable.
async fn load_message(file: &gio::File) -> Result<Message, String> {
    let (bytes, _etag) = file
        .load_contents_future()
        .await
        .map_err(|error| describe_load_error(&error))?;
    let bytes = bytes.to_vec();
    gio::spawn_blocking(move || Message::from_bytes(&bytes))
        .await
        .map_err(|_| gettext("The file couldn’t be read."))?
        .map_err(|error| {
            glib::g_debug!("msg-viewer", "Unreadable file: {error}");
            describe_parse_error(&error)
        })
}

/// Reads the Embedded message carried by `attachment`, off the main thread,
/// or says in plain words why it can't be read.
async fn read_embedded(attachment: Attachment) -> Result<Message, String> {
    gio::spawn_blocking(move || attachment.embedded_message())
        .await
        .map_err(|_| gettext("The message couldn’t be read."))?
        .ok_or_else(|| gettext("The message couldn’t be read."))?
        .map_err(|error| {
            glib::g_debug!("msg-viewer", "Unreadable Embedded message: {error}");
            describe_parse_error(&error)
        })
}

fn describe_load_error(error: &glib::Error) -> String {
    match error.kind::<gio::IOErrorEnum>() {
        Some(gio::IOErrorEnum::NotFound) => gettext("The file doesn’t exist anymore."),
        Some(gio::IOErrorEnum::PermissionDenied) => {
            gettext("You don’t have permission to read the file.")
        }
        Some(gio::IOErrorEnum::IsDirectory) => gettext("This is a folder, not a file."),
        _ => error.message().to_owned(),
    }
}

fn describe_parse_error(error: &outlook_msg::Error) -> String {
    match error {
        outlook_msg::Error::NotCompoundFile(_) => gettext("This file isn’t an Outlook message."),
        outlook_msg::Error::NotAMessage => gettext(
            "This file isn’t an Outlook message. It may be another kind of Office document.",
        ),
        outlook_msg::Error::Damaged(_) => gettext("The message is too damaged to be read."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_carry_one_or_more_files() {
        let paths = |files: Vec<gio::File>| -> Vec<_> {
            files.iter().filter_map(|file| file.path()).collect()
        };
        let one = gio::File::for_path("/tmp/a.msg");
        let two = gio::File::for_path("/tmp/b.msg");

        let list = gdk::FileList::from_array(&[one.clone(), two.clone()]);
        assert_eq!(
            paths(dropped_files(&list.to_value())),
            ["/tmp/a.msg", "/tmp/b.msg"].map(std::path::PathBuf::from)
        );
        assert_eq!(
            paths(dropped_files(&one.to_value())),
            [std::path::PathBuf::from("/tmp/a.msg")]
        );
        assert!(dropped_files(&"text".to_value()).is_empty());
    }

    #[test]
    fn unreadable_file_reason_hides_parser_details() {
        let error = Message::from_bytes(b"plain text").unwrap_err();
        assert_eq!(
            describe_parse_error(&error),
            "This file isn’t an Outlook message."
        );
    }
}
