//! Build-time configuration. Meson passes these through Cargo's environment;
//! the fallbacks match the Flatpak install layout.

use std::path::{Path, PathBuf};

pub const APP_ID: &str = "io.github.larsw.MSGViewer";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The gettext domain of the app's translations and Blueprint templates.
pub const GETTEXT_PACKAGE: &str = "msg-viewer";

pub fn resources_file() -> PathBuf {
    let pkgdatadir = option_env!("MSG_VIEWER_PKGDATADIR").unwrap_or("/app/share/msg-viewer");
    Path::new(pkgdatadir).join("msg-viewer.gresource")
}

/// Where the compiled translations (`<lang>/LC_MESSAGES/msg-viewer.mo`) are.
pub fn localedir() -> &'static str {
    option_env!("MSG_VIEWER_LOCALEDIR").unwrap_or("/app/share/locale")
}
