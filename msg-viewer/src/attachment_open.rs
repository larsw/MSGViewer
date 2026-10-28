//! Opening Attachments in other apps through the OpenURI portal (ADR-0005).
//!
//! The portal opens files, not data, so each Attachment opened is first
//! written to a temporary copy in the app's cache folder. In the Flatpak
//! that folder is the app's own (`~/.var/app/<app id>/cache`), so no
//! filesystem permission is needed, and other apps get the copy only through
//! the portal. Copies are read-only, since changes to them are lost: each
//! window's copies are removed when it closes, and all of them when the app
//! exits, and at startup in case it didn't exit cleanly.

use std::cell::RefCell;
use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::attachment_save;

/// The folder all temporary copies go into.
pub fn copies_folder() -> PathBuf {
    glib::user_cache_dir().join("msg-viewer").join("opened")
}

/// Removes every temporary copy. Only call this while no window is open.
pub fn remove_all_copies() {
    remove_folder(&copies_folder());
}

/// The temporary copies made for one window, in a folder of their own
/// that is removed when this is dropped.
#[derive(Debug)]
pub struct TempCopies {
    parent: PathBuf,
    folder: RefCell<Option<PathBuf>>,
}

impl Default for TempCopies {
    fn default() -> Self {
        TempCopies::new(copies_folder())
    }
}

impl TempCopies {
    /// Copies kept in a new folder inside `parent`, made when the first copy
    /// is.
    pub fn new(parent: PathBuf) -> Self {
        TempCopies {
            parent,
            folder: RefCell::new(None),
        }
    }

    /// Writes `data` to a new read-only file named `name`, a safe file name
    /// (`Attachment::safe_file_name`). Each copy gets a folder of its own,
    /// so Attachments with the same name don't replace each other.
    pub async fn write(&self, name: &str, data: glib::Bytes) -> Result<gio::File, glib::Error> {
        let folder = make_private_folder(&self.folder()?).map_err(io_error)?;
        let folder = gio::File::for_path(folder);
        let file = folder.child(name);
        if !file.has_parent(Some(&folder)) {
            return Err(glib::Error::new(
                gio::IOErrorEnum::InvalidFilename,
                // Translators: {name} is the Attachment's file name.
                &gettext("“{name}” is not a valid file name").replace("{name}", name),
            ));
        }
        attachment_save::save_to_file(&file, data).await?;
        if let Some(path) = file.path() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o400)).map_err(io_error)?;
        }
        Ok(file)
    }

    /// The folder for this window's copies, made on first use.
    fn folder(&self) -> Result<PathBuf, glib::Error> {
        if let Some(folder) = &*self.folder.borrow() {
            return Ok(folder.clone());
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.parent)
            .map_err(io_error)?;
        let folder = make_private_folder(&self.parent).map_err(io_error)?;
        self.folder.replace(Some(folder.clone()));
        Ok(folder)
    }
}

impl Drop for TempCopies {
    fn drop(&mut self) {
        if let Some(folder) = self.folder.take() {
            remove_folder(&folder);
        }
    }
}

/// Hands `file` to the OpenURI portal. With `ask`, the reader picks the
/// app; otherwise it opens in their default app for its type.
pub async fn launch(
    file: &gio::File,
    parent: Option<&gtk::Window>,
    ask: bool,
) -> Result<(), glib::Error> {
    let launcher = gtk::FileLauncher::new(Some(file));
    launcher.set_always_ask(ask);
    launcher.launch_future(parent).await
}

/// Makes a new folder only this user can enter, with a random name inside
/// `parent`.
fn make_private_folder(parent: &Path) -> io::Result<PathBuf> {
    glib::mkdtemp(parent.join("XXXXXX")).ok_or_else(|| {
        // Translators: {folder} is where the copy of an Attachment goes.
        io::Error::other(
            gettext("Could not create a folder in {folder}")
                .replace("{folder}", &parent.display().to_string()),
        )
    })
}

fn remove_folder(folder: &Path) {
    match fs::remove_dir_all(folder) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => glib::g_warning!(
            "msg-viewer",
            "Could not remove temporary copies in {folder:?}: {error}"
        ),
    }
}

fn io_error(error: io::Error) -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::Failed, &error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        glib::MainContext::new().block_on(future)
    }

    fn temp_folder() -> PathBuf {
        glib::mkdtemp(glib::tmp_dir().join("msg-viewer-test-XXXXXX")).unwrap()
    }

    #[test]
    fn copies_are_private_read_only_and_removed_on_drop() {
        let parent = temp_folder().join("opened");
        let copies = TempCopies::new(parent.clone());

        let first = block_on(copies.write("report.pdf", glib::Bytes::from(b"first"))).unwrap();
        let second = block_on(copies.write("report.pdf", glib::Bytes::from(b"second"))).unwrap();
        let (first, second) = (first.path().unwrap(), second.path().unwrap());

        assert_ne!(first, second);
        assert_eq!(first.file_name().unwrap(), "report.pdf");
        assert_eq!(fs::read(&first).unwrap(), b"first");
        assert_eq!(fs::read(&second).unwrap(), b"second");
        assert!(first.starts_with(&parent));
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&first), 0o400);
        assert_eq!(mode(first.parent().unwrap()), 0o700);
        assert_eq!(mode(first.parent().unwrap().parent().unwrap()), 0o700);

        let window_folder = first.parent().unwrap().parent().unwrap().to_owned();
        drop(copies);
        assert!(!window_folder.exists());
        assert!(parent.exists());
        fs::remove_dir_all(parent.parent().unwrap()).unwrap();
    }

    #[test]
    fn copies_of_other_windows_are_kept() {
        let parent = temp_folder();
        let one = TempCopies::new(parent.clone());
        let other = TempCopies::new(parent.clone());
        let kept = block_on(other.write("a.txt", glib::Bytes::from(b"a"))).unwrap();
        block_on(one.write("a.txt", glib::Bytes::from(b"a"))).unwrap();

        drop(one);
        assert!(kept.path().unwrap().exists());
        drop(other);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn refuses_names_outside_the_folder() {
        let parent = temp_folder();
        let copies = TempCopies::new(parent.clone());
        let error = block_on(copies.write("../escaped", glib::Bytes::from(b"x"))).unwrap_err();
        assert!(error.matches(gio::IOErrorEnum::InvalidFilename));
        drop(copies);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn nothing_is_made_until_a_copy_is() {
        let parent = temp_folder().join("opened");
        drop(TempCopies::new(parent.clone()));
        assert!(!parent.exists());
        fs::remove_dir_all(parent.parent().unwrap()).unwrap();
    }
}
