//! Build-time configuration. Meson passes these through Cargo's environment;
//! the fallbacks match the Flatpak install layout.

use std::path::{Path, PathBuf};

pub const APP_ID: &str = "io.github.larsw.MSGViewer";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn resources_file() -> PathBuf {
    let pkgdatadir = option_env!("MSG_VIEWER_PKGDATADIR").unwrap_or("/app/share/msg-viewer");
    Path::new(pkgdatadir).join("msg-viewer.gresource")
}
