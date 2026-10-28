mod application;
mod attachment_open;
mod attachment_policy;
mod attachment_save;
mod attachment_strip;
mod body_view;
mod config;
mod find_bar;
mod header_block;
mod i18n;
mod message_view;
mod preferences_dialog;
mod protection_notice;
mod settings;
mod window;

use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};
use gtk::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    // GTK sets the locale from the environment when it starts.
    bindtextdomain(config::GETTEXT_PACKAGE, config::localedir())
        .expect("Could not bind the text domain");
    bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8")
        .expect("Could not set the text domain's codeset");
    textdomain(config::GETTEXT_PACKAGE).expect("Could not set the text domain");

    let resources =
        gio::Resource::load(config::resources_file()).expect("Could not load resources");
    gio::resources_register(&resources);

    application::Application::new().run()
}
