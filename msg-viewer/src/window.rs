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
                Err(error) => win.show_error(&error),
            }
        });
    }

    fn show_message(&self, message: &Message) {
        let imp = self.imp();
        if let Some(subject) = message.subject() {
            self.set_title(Some(subject));
        }
        imp.message_view.set_message(message);
        imp.stack.set_visible_child_name("message");
    }

    fn show_error(&self, error: &str) {
        let imp = self.imp();
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

async fn load_message(file: &gio::File) -> Result<Message, String> {
    let (bytes, _etag) = file
        .load_contents_future()
        .await
        .map_err(|error| error.message().to_owned())?;
    let bytes = bytes.to_vec();
    gio::spawn_blocking(move || Message::from_bytes(&bytes))
        .await
        .map_err(|_| "The file could not be read".to_owned())?
        .map_err(|error| error.to_string())
}
