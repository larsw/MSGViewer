//! The reader's preferences, kept in GSettings with the schema in
//! `data/io.github.larsw.MSGViewer.gschema.xml`, which Meson installs.

use gtk::gio;

use crate::config;

/// Whether Bodies use the system font rather than `BODY_FONT`.
pub const USE_SYSTEM_FONT: &str = "use-system-font";
/// The font chosen for Bodies, as a Pango font description; empty for the
/// system font.
pub const BODY_FONT: &str = "body-font";
/// Whether plain-text Bodies use the system monospace font.
pub const MONOSPACE_PLAIN_TEXT: &str = "monospace-plain-text";

/// The app's settings. Every instance sees changes made through any other.
pub fn new() -> gio::Settings {
    gio::Settings::new(config::APP_ID)
}
