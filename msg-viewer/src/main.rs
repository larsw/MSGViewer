mod application;
mod body_view;
mod config;
mod header_block;
mod message_view;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    let resources =
        gio::Resource::load(config::resources_file()).expect("Could not load resources");
    gio::resources_register(&resources);

    application::Application::new().run()
}
