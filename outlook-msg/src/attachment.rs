//! Attachments of a Message ([MS-OXMSG] 2.2.2).
//!
//! Each Attachment is a sub-storage `__attach_version1.0_#XXXXXXXX` of the
//! Message's storage, with its own properties stream. A file's data sits in
//! `PidTagAttachDataBinary`; an Embedded message is a whole Message stored in
//! the `__substg1.0_3701000D` sub-storage instead.

use std::io::{self, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

use cfb::CompoundFile;
use encoding_rs::Encoding;

use crate::props::{PROPERTIES_STREAM, Properties, StorageKind};
use crate::{Error, Message, PropValue};

const ATTACHMENT_STORAGE_PREFIX: &str = "__attach_version1.0_#";
const EMBEDDED_MESSAGE_STORAGE: &str = "__substg1.0_3701000D";
const NAMED_PROPERTIES_STORAGE: &str = "__nameid_version1.0";

// Property IDs ([MS-OXPROPS]).
const PID_DISPLAY_NAME: u16 = 0x3001;
const PID_ATTACH_SIZE: u16 = 0x0E20;
const PID_ATTACH_DATA_BINARY: u16 = 0x3701;
const PID_ATTACH_FILENAME: u16 = 0x3704;
const PID_ATTACH_METHOD: u16 = 0x3705;
const PID_ATTACH_LONG_FILENAME: u16 = 0x3707;
const PID_ATTACH_MIME_TAG: u16 = 0x370E;
const PID_ATTACH_CONTENT_ID: u16 = 0x3712;
const PID_ATTACH_FLAGS: u16 = 0x3714;
const PID_ATTACHMENT_HIDDEN: u16 = 0x7FFE;

/// `PidTagAttachMethod` value of an Embedded message ([MS-OXCMSG] 2.2.2.9).
const AFEMBEDDEDMESSAGE: i64 = 5;

/// The properties header of a top-level Message is 8 bytes longer than that
/// of an Embedded message ([MS-OXMSG] 2.4.1).
const EMBEDDED_HEADER_LEN: usize = 24;
const TOP_LEVEL_HEADER_PADDING: [u8; 8] = [0; 8];

/// File names longer than this many bytes are shortened, leaving room for a
/// " (2)" suffix within the usual 255-byte file system limit.
const MAX_FILE_NAME_LEN: usize = 200;
/// The name used when an Attachment has no usable name of its own.
const FALLBACK_FILE_NAME: &str = "attachment";

/// A file or Embedded message carried by a Message.
#[derive(Clone, Debug)]
pub struct Attachment {
    props: Properties,
    /// An Embedded message, re-encoded as a standalone `.msg` file.
    embedded: Option<Vec<u8>>,
}

impl Attachment {
    /// All properties of the Attachment.
    pub fn properties(&self) -> &Properties {
        &self.props
    }

    /// Whether this Attachment is an Embedded message rather than a file.
    pub fn is_embedded_message(&self) -> bool {
        self.int(PID_ATTACH_METHOD) == Some(AFEMBEDDEDMESSAGE)
    }

    /// The name the sender gave the Attachment: its long file name, else its
    /// short (8.3) file name, else its display name.
    ///
    /// This comes straight from the file and may hold path separators or
    /// control characters; use [`Attachment::safe_file_name`] for anything
    /// that names a file.
    pub fn file_name(&self) -> Option<&str> {
        [
            PID_ATTACH_LONG_FILENAME,
            PID_ATTACH_FILENAME,
            PID_DISPLAY_NAME,
        ]
        .into_iter()
        .filter_map(|id| self.props.string(id))
        .find(|name| !name.trim().is_empty())
    }

    /// A file name that is safe to create in a folder: see
    /// [`sanitize_file_name`]. Embedded messages get a `.msg` extension.
    pub fn safe_file_name(&self) -> String {
        let mut name = self
            .file_name()
            .and_then(sanitize_file_name)
            .unwrap_or_else(|| FALLBACK_FILE_NAME.to_owned());
        if self.is_embedded_message() && !name.to_lowercase().ends_with(".msg") {
            name.push_str(".msg");
        }
        name
    }

    /// The MIME type the sender declared, e.g. `image/png`. It is not checked
    /// against the data.
    pub fn mime_type(&self) -> Option<&str> {
        self.props.string(PID_ATTACH_MIME_TAG)
    }

    /// The Content-ID by which an HTML Body refers to this Attachment through
    /// a `cid:` link, without angle brackets.
    pub fn content_id(&self) -> Option<&str> {
        self.props
            .string(PID_ATTACH_CONTENT_ID)
            .map(|id| id.trim().trim_start_matches('<').trim_end_matches('>'))
            .filter(|id| !id.is_empty())
    }

    /// Whether the sender marked the Attachment as hidden
    /// (`PidTagAttachmentHidden`), as is usual for Inline attachments.
    pub fn is_hidden(&self) -> bool {
        matches!(
            self.props.get(PID_ATTACHMENT_HIDDEN),
            Some(PropValue::Bool(true))
        )
    }

    /// `PidTagAttachFlags`: 0x1 invisible in HTML, 0x2 invisible in RTF,
    /// 0x4 rendered in the Body ([MS-OXCMSG] 2.2.2.18). 0 when absent.
    pub fn flags(&self) -> u32 {
        self.int(PID_ATTACH_FLAGS).map_or(0, |flags| flags as u32)
    }

    /// The Attachment's content, byte for byte: the file's data, or for an
    /// Embedded message a standalone `.msg` file holding it. `None` when the
    /// Attachment carries no data that can be saved, such as a link to a
    /// file elsewhere or an OLE object.
    pub fn data(&self) -> Option<&[u8]> {
        if self.is_embedded_message() {
            return self.embedded.as_deref();
        }
        match self.props.get(PID_ATTACH_DATA_BINARY)? {
            PropValue::Binary(data) => Some(data),
            _ => None,
        }
    }

    /// The size of [`Attachment::data`] in bytes, or the size Outlook
    /// recorded for the Attachment when there is no data.
    pub fn size(&self) -> Option<u64> {
        match self.data() {
            Some(data) => Some(data.len() as u64),
            None => self
                .int(PID_ATTACH_SIZE)
                .and_then(|size| u64::try_from(size).ok()),
        }
    }

    /// Reads the Embedded message, if this Attachment is one.
    pub fn embedded_message(&self) -> Option<Result<Message, Error>> {
        self.embedded.as_deref().map(Message::from_bytes)
    }

    fn int(&self, id: u16) -> Option<i64> {
        self.props.int(id)
    }
}

/// Reads every Attachment of the Message stored in `storage`, in the order
/// Outlook numbered them. 8-bit strings fall back to the Message's `encoding`.
pub(crate) fn read_all<F: Read + Seek>(
    file: &mut CompoundFile<F>,
    storage: &Path,
    encoding: &'static Encoding,
) -> Result<Vec<Attachment>, Error> {
    let mut storages: Vec<(u32, PathBuf)> = file
        .read_storage(storage)?
        .filter(|entry| entry.is_storage())
        .filter_map(|entry| {
            let number = entry.name().strip_prefix(ATTACHMENT_STORAGE_PREFIX)?;
            let number = u32::from_str_radix(number, 16).ok()?;
            Some((number, entry.path().to_owned()))
        })
        .collect();
    storages.sort();

    let mut attachments = Vec::with_capacity(storages.len());
    for (_, path) in storages {
        if !file.is_stream(path.join(PROPERTIES_STREAM)) {
            continue;
        }
        let props = Properties::read(file, &path, StorageKind::RecipientOrAttachment, encoding)?;
        let mut attachment = Attachment {
            props,
            embedded: None,
        };
        let embedded = path.join(EMBEDDED_MESSAGE_STORAGE);
        if attachment.is_embedded_message() && file.is_storage(&embedded) {
            // A damaged Embedded message leaves just that Attachment without data.
            attachment.embedded = export_embedded_message(file, &embedded).ok();
        }
        attachments.push(attachment);
    }
    Ok(attachments)
}

/// Copies the Embedded message in `storage` into a new compound file, laid
/// out as a top-level `.msg` file, and returns its bytes.
fn export_embedded_message<F: Read + Seek>(
    file: &mut CompoundFile<F>,
    storage: &Path,
) -> io::Result<Vec<u8>> {
    let mut out = CompoundFile::create_with_version(cfb::Version::V3, Cursor::new(Vec::new()))?;
    let root = Path::new("/");
    copy_storage(file, storage, &mut out, root)?;

    // Named properties are mapped once, at the root of the outermost file
    // ([MS-OXMSG] 2.2.3), so the Embedded message needs a copy of the map.
    let named = root.join(NAMED_PROPERTIES_STORAGE);
    if file.is_storage(&named) && !out.exists(&named) {
        copy_storage(file, &named, &mut out, &named)?;
    }

    let props = root.join(PROPERTIES_STREAM);
    if out.is_stream(&props) {
        let mut table = Vec::new();
        out.open_stream(&props)?.read_to_end(&mut table)?;
        if table.len() >= EMBEDDED_HEADER_LEN {
            table.splice(
                EMBEDDED_HEADER_LEN..EMBEDDED_HEADER_LEN,
                TOP_LEVEL_HEADER_PADDING,
            );
            let mut stream = out.create_stream(&props)?;
            stream.write_all(&table)?;
            stream.flush()?;
        }
    }

    out.flush()?;
    Ok(out.into_inner().into_inner())
}

/// Copies the storage `from` with everything in it to `to` in `out`.
fn copy_storage<F: Read + Seek>(
    file: &mut CompoundFile<F>,
    from: &Path,
    out: &mut CompoundFile<Cursor<Vec<u8>>>,
    to: &Path,
) -> io::Result<()> {
    let entries: Vec<cfb::Entry> = file.walk_storage(from)?.collect();
    for entry in &entries {
        let relative = entry
            .path()
            .strip_prefix(from)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad storage path"))?;
        let target = to.join(relative);
        if entry.is_stream() {
            let mut source = file.open_stream(entry.path())?;
            let mut copy = out.create_stream(&target)?;
            io::copy(&mut source, &mut copy)?;
            copy.flush()?;
        } else {
            if !out.exists(&target) {
                out.create_storage(&target)?;
            }
            out.set_storage_clsid(&target, *entry.clsid())?;
        }
    }
    Ok(())
}

/// Turns a file name taken from a Message into one that is safe to create in
/// a folder, or `None` if nothing usable is left.
///
/// Only the last path component is kept, so the result never names another
/// folder. Control and bidirectional-override characters are dropped, so the
/// name shows its real extension. Characters other systems reject in file
/// names become `_`. Leading dots (hidden files, `..`) and trailing dots and
/// spaces are trimmed, and long names are shortened, keeping the extension.
pub fn sanitize_file_name(name: &str) -> Option<String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|&c| !c.is_control() && !is_format_control(c))
        .map(|c| match c {
            ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned
        .trim_start_matches(|c: char| c == '.' || c.is_whitespace())
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }
    Some(shorten(trimmed, MAX_FILE_NAME_LEN))
}

/// Invisible characters that change how the rest of a name is displayed, as
/// in `invoice\u{202E}fdp.exe` showing as `invoiceexe.pdf`.
fn is_format_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// Shortens `name` to at most `max` bytes, keeping a short extension.
fn shorten(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_owned();
    }
    let extension = name
        .rfind('.')
        .map(|dot| &name[dot..])
        .filter(|extension| extension.len() <= 16);
    let extension = extension.unwrap_or_default();
    let mut end = max - extension.len();
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{extension}", name[..end].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitize(name: &str) -> Option<String> {
        sanitize_file_name(name)
    }

    #[test]
    fn keeps_ordinary_names() {
        assert_eq!(
            sanitize("Report 2026.pdf").as_deref(),
            Some("Report 2026.pdf")
        );
        assert_eq!(
            sanitize("Ärger über Öl.docx").as_deref(),
            Some("Ärger über Öl.docx")
        );
    }

    #[test]
    fn keeps_only_the_last_path_component() {
        assert_eq!(sanitize("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(
            sanitize("C:\\Users\\ada\\notes.txt").as_deref(),
            Some("notes.txt")
        );
        assert_eq!(sanitize("/etc/shadow").as_deref(), Some("shadow"));
    }

    #[test]
    fn rejects_names_with_nothing_left() {
        assert_eq!(sanitize(""), None);
        assert_eq!(sanitize("   "), None);
        assert_eq!(sanitize("."), None);
        assert_eq!(sanitize(".."), None);
        assert_eq!(sanitize("dir/"), None);
        assert_eq!(sanitize("\0\u{7}\n"), None);
    }

    #[test]
    fn trims_leading_dots_and_trailing_dots_and_spaces() {
        assert_eq!(sanitize(".bashrc").as_deref(), Some("bashrc"));
        assert_eq!(sanitize(" ..hidden. . ").as_deref(), Some("hidden"));
    }

    #[test]
    fn drops_control_and_bidi_characters() {
        assert_eq!(sanitize("a\0b\nc\u{1b}.txt").as_deref(), Some("abc.txt"));
        assert_eq!(
            sanitize("invoice\u{202E}fdp.exe").as_deref(),
            Some("invoicefdp.exe")
        );
    }

    #[test]
    fn replaces_reserved_characters() {
        assert_eq!(
            sanitize("Re: what?*.msg").as_deref(),
            Some("Re_ what__.msg")
        );
        assert_eq!(sanitize("a<b>c|d\"e").as_deref(), Some("a_b_c_d_e"));
    }

    #[test]
    fn shortens_long_names_keeping_the_extension() {
        let long = format!("{}.pdf", "é".repeat(300));
        let short = sanitize(&long).unwrap();
        assert!(short.len() <= MAX_FILE_NAME_LEN);
        assert!(short.ends_with("é.pdf"));

        let no_extension = "x".repeat(300);
        assert_eq!(sanitize(&no_extension).unwrap().len(), MAX_FILE_NAME_LEN);
    }
}
