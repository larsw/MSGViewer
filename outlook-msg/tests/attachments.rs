//! Attachments read from small `.msg` files built here with `cfb`, to cover
//! cases the sample files don't: short and missing names, hostile names,
//! Attachments without data and Embedded messages in detail.

use std::io::{Cursor, Write};
use std::path::Path;

use cfb::CompoundFile;
use outlook_msg::Message;

const PID_SUBJECT: u16 = 0x0037;
const PID_ATTACH_SIZE: u16 = 0x0E20;
const PID_DISPLAY_NAME: u16 = 0x3001;
const PID_ATTACH_DATA: u16 = 0x3701;
const PID_ATTACH_FILENAME: u16 = 0x3704;
const PID_ATTACH_METHOD: u16 = 0x3705;
const PID_ATTACH_LONG_FILENAME: u16 = 0x3707;
const PID_ATTACH_MIME_TAG: u16 = 0x370E;
const PID_ATTACH_CONTENT_ID: u16 = 0x3712;
const PID_ATTACH_FLAGS: u16 = 0x3714;
const PID_ATTACHMENT_HIDDEN: u16 = 0x7FFE;

const BY_VALUE: i32 = 1;
const BY_REFERENCE: i32 = 2;
const EMBEDDED_MESSAGE: i32 = 5;

const TOP_LEVEL_HEADER: usize = 32;
const EMBEDDED_HEADER: usize = 24;
const ATTACHMENT_HEADER: usize = 8;

const CFB_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

enum Value<'a> {
    Long(i32),
    Bool(bool),
    Unicode(&'a str),
    String8(&'a [u8]),
    Binary(&'a [u8]),
}

type Props<'a> = &'a [(u16, Value<'a>)];

/// What goes into one attachment storage.
enum Content<'a> {
    Props(Props<'a>),
    /// Properties of the Attachment, then of the Embedded message, then the
    /// Embedded message's own Attachments.
    Embedded(Props<'a>, Props<'a>, Props<'a>),
}

struct Fixture(CompoundFile<Cursor<Vec<u8>>>);

impl Fixture {
    fn new(subject: &str) -> Self {
        let file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        let mut fixture = Fixture(file);
        fixture.props(
            "/",
            TOP_LEVEL_HEADER,
            &[(PID_SUBJECT, Value::Unicode(subject))],
        );
        fixture
    }

    fn props(&mut self, storage: &str, header_len: usize, props: Props) {
        let storage = Path::new(storage);
        let mut table = vec![0; header_len];
        for (id, value) in props {
            let (ty, fixed, variable): (u16, [u8; 8], Option<Vec<u8>>) = match value {
                Value::Long(v) => (0x0003, widen(&v.to_le_bytes()), None),
                Value::Bool(v) => (0x000B, widen(&[u8::from(*v)]), None),
                Value::Unicode(s) => {
                    let mut bytes: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                    bytes.extend([0, 0]);
                    (0x001F, [0; 8], Some(bytes))
                }
                Value::String8(s) => (0x001E, [0; 8], Some([*s, &[0]].concat())),
                Value::Binary(b) => (0x0102, [0; 8], Some(b.to_vec())),
            };
            let tag = u32::from(*id) << 16 | u32::from(ty);
            table.extend(tag.to_le_bytes());
            table.extend(6u32.to_le_bytes());
            table.extend(fixed);
            if let Some(bytes) = variable {
                self.stream(&storage.join(format!("__substg1.0_{tag:08X}")), &bytes);
            }
        }
        self.stream(&storage.join("__properties_version1.0"), &table);
    }

    fn stream(&mut self, path: &Path, bytes: &[u8]) {
        let mut stream = self.0.create_stream(path).unwrap();
        stream.write_all(bytes).unwrap();
        stream.flush().unwrap();
    }

    fn attachment(mut self, number: u32, content: Content) -> Self {
        let storage = format!("/__attach_version1.0_#{number:08X}");
        self.0.create_storage(&storage).unwrap();
        match content {
            Content::Props(props) => self.props(&storage, ATTACHMENT_HEADER, props),
            Content::Embedded(props, message, inner) => {
                self.props(&storage, ATTACHMENT_HEADER, props);
                let embedded = format!("{storage}/__substg1.0_3701000D");
                self.0.create_storage(&embedded).unwrap();
                self.props(&embedded, EMBEDDED_HEADER, message);
                let inner_storage = format!("{embedded}/__attach_version1.0_#00000000");
                self.0.create_storage(&inner_storage).unwrap();
                self.props(&inner_storage, ATTACHMENT_HEADER, inner);
            }
        }
        self
    }

    fn read(mut self) -> Message {
        self.0.flush().unwrap();
        Message::from_bytes(self.0.into_inner().get_ref()).unwrap()
    }
}

fn widen(bytes: &[u8]) -> [u8; 8] {
    let mut out = [0; 8];
    out[..bytes.len()].copy_from_slice(bytes);
    out
}

/// A Message with one by-value Attachment holding `props` and some data.
fn one_file(props: Props) -> Message {
    Fixture::new("Names")
        .attachment(0, Content::Props(props))
        .read()
}

#[test]
fn prefers_the_long_file_name() {
    let msg = one_file(&[
        (PID_ATTACH_METHOD, Value::Long(BY_VALUE)),
        (PID_ATTACH_FILENAME, Value::Unicode("QUARTE~1.XLS")),
        (
            PID_ATTACH_LONG_FILENAME,
            Value::Unicode("Quarterly report 2026.xlsx"),
        ),
        (PID_DISPLAY_NAME, Value::Unicode("Quarterly report")),
        (PID_ATTACH_DATA, Value::Binary(b"data")),
    ]);
    let attachment = &msg.attachments()[0];
    assert_eq!(attachment.file_name(), Some("Quarterly report 2026.xlsx"));
    assert_eq!(attachment.safe_file_name(), "Quarterly report 2026.xlsx");
}

#[test]
fn falls_back_to_the_short_file_name() {
    let msg = one_file(&[
        (PID_ATTACH_METHOD, Value::Long(BY_VALUE)),
        (PID_ATTACH_LONG_FILENAME, Value::Unicode("")),
        (PID_ATTACH_FILENAME, Value::String8(b"REPORT~1.PDF")),
        (PID_ATTACH_DATA, Value::Binary(b"%PDF-")),
    ]);
    assert_eq!(msg.attachments()[0].file_name(), Some("REPORT~1.PDF"));
}

#[test]
fn falls_back_to_the_display_name_then_a_generic_name() {
    let msg = one_file(&[(PID_DISPLAY_NAME, Value::String8(b"caf\xe9.txt"))]);
    assert_eq!(msg.attachments()[0].file_name(), Some("café.txt"));

    let msg = one_file(&[(PID_ATTACH_DATA, Value::Binary(b"?"))]);
    assert_eq!(msg.attachments()[0].file_name(), None);
    assert_eq!(msg.attachments()[0].safe_file_name(), "attachment");
}

#[test]
fn hostile_file_names_are_made_safe() {
    let cases = [
        ("../../.bashrc", "bashrc"),
        ("..\\..\\Windows\\win.ini", "win.ini"),
        ("/etc/passwd", "passwd"),
        ("invoice\u{202E}fdp.exe", "invoicefdp.exe"),
        ("new\nline\0.txt", "newline.txt"),
        ("..", "attachment"),
        ("dir/", "attachment"),
    ];
    for (hostile, safe) in cases {
        let msg = one_file(&[(PID_ATTACH_LONG_FILENAME, Value::Unicode(hostile))]);
        let attachment = &msg.attachments()[0];
        assert_eq!(attachment.file_name(), Some(hostile));
        assert_eq!(attachment.safe_file_name(), safe, "for {hostile:?}");
    }
}

#[test]
fn reads_data_and_details() {
    let data: Vec<u8> = (0..=255).cycle().take(5000).collect();
    let msg = one_file(&[
        (PID_ATTACH_METHOD, Value::Long(BY_VALUE)),
        (PID_ATTACH_LONG_FILENAME, Value::Unicode("logo.png")),
        (PID_ATTACH_DATA, Value::Binary(&data)),
        (PID_ATTACH_MIME_TAG, Value::Unicode("image/png")),
        (PID_ATTACH_CONTENT_ID, Value::Unicode("<logo@example.org>")),
        (PID_ATTACHMENT_HIDDEN, Value::Bool(true)),
        (PID_ATTACH_FLAGS, Value::Long(4)),
    ]);
    let attachment = &msg.attachments()[0];
    assert_eq!(attachment.data(), Some(&data[..]));
    assert_eq!(attachment.size(), Some(5000));
    assert_eq!(attachment.mime_type(), Some("image/png"));
    assert_eq!(attachment.content_id(), Some("logo@example.org"));
    assert!(attachment.is_hidden());
    assert_eq!(attachment.flags(), 4);
    assert!(!attachment.is_embedded_message());
    assert!(attachment.embedded_message().is_none());
}

#[test]
fn attachment_without_data_has_no_data_but_a_size() {
    let msg = one_file(&[
        (PID_ATTACH_METHOD, Value::Long(BY_REFERENCE)),
        (
            PID_ATTACH_LONG_FILENAME,
            Value::Unicode("on-the-share.docx"),
        ),
        (PID_ATTACH_SIZE, Value::Long(1234)),
    ]);
    let attachment = &msg.attachments()[0];
    assert_eq!(attachment.data(), None);
    assert_eq!(attachment.size(), Some(1234));
    assert!(!attachment.is_hidden());
    assert_eq!(attachment.content_id(), None);
}

#[test]
fn attachments_keep_their_numbered_order() {
    let named = |name| [(PID_ATTACH_LONG_FILENAME, Value::Unicode(name))];
    let msg = Fixture::new("Order")
        .attachment(0x0A, Content::Props(&named("third")))
        .attachment(0x02, Content::Props(&named("second")))
        .attachment(0x00, Content::Props(&named("first")))
        .read();
    let names: Vec<_> = msg.attachments().iter().map(|a| a.file_name()).collect();
    assert_eq!(names, [Some("first"), Some("second"), Some("third")]);
}

#[test]
fn embedded_message_reads_as_a_standalone_message() {
    let msg = Fixture::new("Outer")
        .attachment(
            0,
            Content::Embedded(
                &[
                    (PID_ATTACH_METHOD, Value::Long(EMBEDDED_MESSAGE)),
                    (PID_DISPLAY_NAME, Value::Unicode("Re: Plans")),
                ],
                &[(PID_SUBJECT, Value::Unicode("Re: Plans"))],
                &[
                    (PID_ATTACH_METHOD, Value::Long(BY_VALUE)),
                    (PID_ATTACH_LONG_FILENAME, Value::Unicode("plan.txt")),
                    (PID_ATTACH_DATA, Value::Binary(b"step 1")),
                ],
            ),
        )
        .read();
    let attachment = &msg.attachments()[0];
    assert!(attachment.is_embedded_message());
    assert_eq!(attachment.safe_file_name(), "Re_ Plans.msg");

    // Saving writes a `.msg` file that reads back as the Embedded message.
    let data = attachment.data().unwrap();
    assert!(data.starts_with(CFB_MAGIC));
    assert_eq!(attachment.size(), Some(data.len() as u64));
    let saved = Message::from_bytes(data).unwrap();
    assert_eq!(saved.subject(), Some("Re: Plans"));

    let embedded = attachment.embedded_message().unwrap().unwrap();
    assert_eq!(embedded.subject(), Some("Re: Plans"));
    let inner = &embedded.attachments()[0];
    assert_eq!(inner.file_name(), Some("plan.txt"));
    assert_eq!(inner.data(), Some(&b"step 1"[..]));
}

#[test]
fn embedded_message_without_storage_has_no_data() {
    let msg = one_file(&[
        (PID_ATTACH_METHOD, Value::Long(EMBEDDED_MESSAGE)),
        (PID_DISPLAY_NAME, Value::Unicode("Lost.msg")),
    ]);
    let attachment = &msg.attachments()[0];
    assert!(attachment.is_embedded_message());
    assert_eq!(attachment.safe_file_name(), "Lost.msg");
    assert_eq!(attachment.data(), None);
    assert!(attachment.embedded_message().is_none());
}
