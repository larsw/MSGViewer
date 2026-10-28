//! Writing Attachments to files the reader picked through the file-chooser
//! portal. Names given here must already be safe file names
//! (`Attachment::safe_file_name`).

use gtk::prelude::*;
use gtk::{gio, glib};

/// How many numbered names Save All tries before giving up on a name.
const MAX_NUMBERED_NAMES: u32 = 1000;

/// Writes `data` to `file`, replacing what is there; the save dialog has
/// already asked the reader about that.
pub async fn save_to_file(file: &gio::File, data: glib::Bytes) -> Result<(), glib::Error> {
    file.replace_contents_future(data, None, false, gio::FileCreateFlags::REPLACE_DESTINATION)
        .await
        .map(|_| ())
        .map_err(|(_, error)| error)
}

/// Saves each `(name, data)` pair as a new file in `folder`. A name that is
/// taken gets a number, as in `report (2).pdf`, so no existing file is ever
/// overwritten. Returns the files written, in order.
pub async fn save_into_folder(
    folder: &gio::File,
    attachments: &[(String, glib::Bytes)],
) -> Result<Vec<gio::File>, glib::Error> {
    let mut saved = Vec::with_capacity(attachments.len());
    for (name, data) in attachments {
        saved.push(save_as_new_file(folder, name, data).await?);
    }
    Ok(saved)
}

async fn save_as_new_file(
    folder: &gio::File,
    name: &str,
    data: &glib::Bytes,
) -> Result<gio::File, glib::Error> {
    for number in 1..=MAX_NUMBERED_NAMES {
        let file = folder.child(numbered_name(name, number));
        if !file.has_parent(Some(folder)) {
            return Err(glib::Error::new(
                gio::IOErrorEnum::InvalidFilename,
                &format!("“{name}” is not a valid file name"),
            ));
        }
        // Creating fails if the file exists, without a window for a race.
        let stream = match file
            .create_future(gio::FileCreateFlags::NONE, glib::Priority::DEFAULT)
            .await
        {
            Ok(stream) => stream,
            Err(error) if error.matches(gio::IOErrorEnum::Exists) => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = write_and_close(&stream, data.clone()).await {
            // Don't leave a truncated copy behind.
            let _ = file.delete_future(glib::Priority::DEFAULT).await;
            return Err(error);
        }
        return Ok(file);
    }
    Err(glib::Error::new(
        gio::IOErrorEnum::Exists,
        &format!("Too many files named like “{name}” already exist"),
    ))
}

async fn write_and_close(
    stream: &gio::FileOutputStream,
    data: glib::Bytes,
) -> Result<(), glib::Error> {
    let (_, _, error) = stream
        .write_all_future(data, glib::Priority::DEFAULT)
        .await
        .map_err(|(_, error)| error)?;
    if let Some(error) = error {
        return Err(error);
    }
    stream.close_future(glib::Priority::DEFAULT).await
}

/// The `number`th name to try for `name`: the name itself, then
/// `name (2).ext`, `name (3).ext` and so on, keeping `.tar.gz` style
/// extensions whole.
pub fn numbered_name(name: &str, number: u32) -> String {
    if number <= 1 {
        return name.to_owned();
    }
    let (stem, extension) = split_extension(name);
    format!("{stem} ({number}){extension}")
}

fn split_extension(name: &str) -> (&str, &str) {
    let Some(dot) = name.rfind('.').filter(|&dot| dot > 0) else {
        return (name, "");
    };
    let stem = &name[..dot];
    let dot = match stem.rfind('.') {
        Some(tar) if tar > 0 && stem[tar..].eq_ignore_ascii_case(".tar") => tar,
        _ => dot,
    };
    name.split_at(dot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_names_before_the_extension() {
        assert_eq!(numbered_name("report.pdf", 1), "report.pdf");
        assert_eq!(numbered_name("report.pdf", 2), "report (2).pdf");
        assert_eq!(numbered_name("my.report.pdf", 3), "my.report (3).pdf");
        assert_eq!(numbered_name("README", 2), "README (2)");
        assert_eq!(numbered_name("backup.tar.gz", 2), "backup (2).tar.gz");
        assert_eq!(numbered_name("tar.gz", 2), "tar (2).gz");
        assert_eq!(numbered_name(".hidden", 2), ".hidden (2)");
    }

    /// Runs `future` to completion on a fresh main context.
    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        glib::MainContext::new().block_on(future)
    }

    fn temp_folder() -> gio::File {
        let path = glib::mkdtemp(glib::tmp_dir().join("msg-viewer-test-XXXXXX")).unwrap();
        gio::File::for_path(path)
    }

    fn contents(file: &gio::File) -> Vec<u8> {
        std::fs::read(file.path().unwrap()).unwrap()
    }

    fn remove(folder: &gio::File) {
        std::fs::remove_dir_all(folder.path().unwrap()).unwrap();
    }

    fn bytes(data: &[u8]) -> glib::Bytes {
        glib::Bytes::from(data)
    }

    #[test]
    fn save_all_never_overwrites() {
        let folder = temp_folder();
        let existing = folder.child("report.pdf");
        std::fs::write(existing.path().unwrap(), b"keep me").unwrap();

        let attachments = [
            ("report.pdf".to_owned(), bytes(b"first")),
            ("report.pdf".to_owned(), bytes(b"second")),
            ("notes.txt".to_owned(), bytes(b"")),
        ];
        let saved = block_on(save_into_folder(&folder, &attachments)).unwrap();

        let names: Vec<_> = saved.iter().map(|f| f.basename().unwrap()).collect();
        assert_eq!(
            names,
            ["report (2).pdf", "report (3).pdf", "notes.txt"].map(std::path::PathBuf::from)
        );
        assert_eq!(contents(&existing), b"keep me");
        assert_eq!(contents(&saved[0]), b"first");
        assert_eq!(contents(&saved[1]), b"second");
        assert_eq!(contents(&saved[2]), b"");
        remove(&folder);
    }

    #[test]
    fn save_all_refuses_names_outside_the_folder() {
        let folder = temp_folder();
        let attachments = [("../escaped".to_owned(), bytes(b"x"))];
        let error = block_on(save_into_folder(&folder, &attachments)).unwrap_err();
        assert!(error.matches(gio::IOErrorEnum::InvalidFilename));
        assert!(
            !folder
                .parent()
                .unwrap()
                .child("escaped")
                .query_exists(None::<&gio::Cancellable>)
        );
        remove(&folder);
    }

    #[test]
    fn save_writes_byte_for_byte_and_replaces() {
        let folder = temp_folder();
        let file = folder.child("data.bin");
        std::fs::write(file.path().unwrap(), b"old contents, longer").unwrap();
        let data: Vec<u8> = (0..=255).cycle().take(100_000).collect();

        block_on(save_to_file(&file, bytes(&data))).unwrap();
        assert_eq!(contents(&file), data);
        remove(&folder);
    }
}
