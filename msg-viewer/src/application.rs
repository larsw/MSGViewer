use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::config;
use crate::window::Window;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct Application;

    #[glib::object_subclass]
    impl ObjectSubclass for Application {
        const NAME: &'static str = "MsgViewerApplication";
        type Type = super::Application;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for Application {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_actions();
        }
    }

    impl ApplicationImpl for Application {
        fn activate(&self) {
            Window::new(&self.obj()).present();
        }

        /// Files opened from Files or the command line each get a window.
        fn open(&self, files: &[gio::File], _hint: &str) {
            for file in files {
                let window = Window::new(&self.obj());
                window.present();
                window.open_file(file.clone());
            }
        }
    }

    impl GtkApplicationImpl for Application {}
    impl AdwApplicationImpl for Application {}
}

glib::wrapper! {
    pub struct Application(ObjectSubclass<imp::Application>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Application {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", config::APP_ID)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .property("resource-base-path", "/io/github/larsw/MSGViewer")
            .build()
    }

    fn setup_actions(&self) {
        let quit = gio::ActionEntry::builder("quit")
            .activate(|app: &Self, _, _| app.quit())
            .build();
        let about = gio::ActionEntry::builder("about")
            .activate(|app: &Self, _, _| app.show_about())
            .build();
        self.add_action_entries([quit, about]);

        self.set_accels_for_action("app.quit", &["<primary>q"]);
        self.set_accels_for_action("win.open", &["<primary>o"]);
        self.set_accels_for_action("window.close", &["<primary>w"]);
        self.set_accels_for_action("win.find", &["<primary>f"]);
        self.set_accels_for_action("win.find-next", &["<primary>g"]);
        self.set_accels_for_action("win.find-previous", &["<primary><shift>g"]);
        self.set_accels_for_action(
            "win.zoom-in",
            &["<primary>plus", "<primary>equal", "<primary>KP_Add"],
        );
        self.set_accels_for_action("win.zoom-out", &["<primary>minus", "<primary>KP_Subtract"]);
        self.set_accels_for_action("win.zoom-reset", &["<primary>0", "<primary>KP_0"]);
        // AdwApplication adds app.shortcuts (Ctrl+?) for shortcuts-dialog.ui.
    }

    fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name("MSG Viewer")
            .application_icon(config::APP_ID)
            .version(config::VERSION)
            .developer_name("Lars Wilhelmsen")
            .license_type(gtk::License::Apache20)
            .website("https://github.com/larsw/gnome-msg-viewer")
            .issue_url("https://github.com/larsw/gnome-msg-viewer/issues")
            .build();
        dialog.present(self.active_window().as_ref());
    }
}
