use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use outlook_msg::Message;

use crate::application::Application;
use crate::message_view::MessageView;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/larsw/MSGViewer/ui/window.ui")]
    pub struct Window {
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
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "MsgViewerWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            MessageView::ensure_type();
            klass.bind_template();
            klass.install_action_async("win.open", None, |win, _, _| async move {
                win.choose_file().await;
            });
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for Window {}
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
        filter.set_name(Some("Outlook Messages"));
        filter.add_mime_type("application/vnd.ms-outlook");
        filter.add_suffix("msg");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);

        let dialog = gtk::FileDialog::builder()
            .title("Open Message")
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

        // Each file gets its own window; reuse this one only while it is empty.
        if self.imp().file.borrow().is_none() {
            self.open_file(file);
        } else if let Some(app) = self.application().and_downcast::<Application>() {
            let window = Window::new(&app);
            window.present();
            window.open_file(file);
        }
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
        .map_err(|_| "The file couldn’t be read.".to_owned())?
        .map_err(|error| {
            glib::g_debug!("msg-viewer", "Unreadable file: {error}");
            describe_parse_error(&error).to_owned()
        })
}

fn describe_load_error(error: &glib::Error) -> String {
    match error.kind::<gio::IOErrorEnum>() {
        Some(gio::IOErrorEnum::NotFound) => "The file doesn’t exist anymore.".to_owned(),
        Some(gio::IOErrorEnum::PermissionDenied) => {
            "You don’t have permission to read the file.".to_owned()
        }
        Some(gio::IOErrorEnum::IsDirectory) => "This is a folder, not a file.".to_owned(),
        _ => error.message().to_owned(),
    }
}

fn describe_parse_error(error: &outlook_msg::Error) -> &'static str {
    match error {
        outlook_msg::Error::NotCompoundFile(_) => "This file isn’t an Outlook message.",
        outlook_msg::Error::NotAMessage => {
            "This file isn’t an Outlook message. It may be another kind of Office document."
        }
        outlook_msg::Error::Damaged(_) => "The message is too damaged to be read.",
    }
}
