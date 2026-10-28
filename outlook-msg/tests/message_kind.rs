//! Message classes other than `IPM.Note`, and signed and encrypted
//! Messages, on small `.msg` files built here with `cfb`, since there are no
//! redistributable samples of them.

use std::io::{Cursor, Write};

use cfb::CompoundFile;
use outlook_msg::{Body, Message, MessageKind};

const PID_MESSAGE_CLASS: u16 = 0x001A;
const PID_SUBJECT: u16 = 0x0037;
const PID_SENDER_NAME: u16 = 0x0C1A;
const PID_BODY: u16 = 0x1000;
const PID_ATTACH_DATA: u16 = 0x3701;
const PID_ATTACH_METHOD: u16 = 0x3705;
const PID_ATTACH_LONG_FILENAME: u16 = 0x3707;
const PID_ATTACH_MIME_TAG: u16 = 0x370E;

const BY_VALUE: i32 = 1;

const TOP_LEVEL_HEADER: usize = 32;
const ATTACHMENT_HEADER: usize = 8;

/// The start of a DER-encoded CMS ContentInfo of type signed-data.
const CMS_SIGNED: &[u8] = &[
    0x30, 0x82, 0x01, 0x00, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02, 0xA0,
];
/// The start of a DER-encoded CMS ContentInfo of type enveloped-data.
const CMS_ENVELOPED: &[u8] = &[
    0x30, 0x80, 0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x03, 0xA0, 0x80,
];

enum Value<'a> {
    Long(i32),
    Unicode(&'a str),
    Binary(&'a [u8]),
}

type Props<'a> = &'a [(u16, Value<'a>)];

/// Builds a `.msg` file with the given Message properties and Attachments.
fn build(props: Props, attachments: &[Props]) -> Vec<u8> {
    let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    write_props(&mut file, "", TOP_LEVEL_HEADER, props);
    for (i, attachment) in attachments.iter().enumerate() {
        let storage = format!("/__attach_version1.0_#{i:08X}");
        file.create_storage(&storage).unwrap();
        write_props(&mut file, &storage, ATTACHMENT_HEADER, attachment);
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
}

fn write_props(
    file: &mut CompoundFile<Cursor<Vec<u8>>>,
    storage: &str,
    header_len: usize,
    props: Props,
) {
    let mut table = vec![0; header_len];
    for (id, value) in props {
        let (ty, fixed, variable): (u16, [u8; 8], Option<Vec<u8>>) = match value {
            Value::Long(v) => (0x0003, widen(&v.to_le_bytes()), None),
            Value::Unicode(s) => {
                let mut bytes: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                bytes.extend([0, 0]);
                (0x001F, [0; 8], Some(bytes))
            }
            Value::Binary(b) => (0x0102, [0; 8], Some(b.to_vec())),
        };
        let tag = (u32::from(*id) << 16) | u32::from(ty);
        table.extend(tag.to_le_bytes());
        table.extend(6u32.to_le_bytes());
        table.extend(fixed);
        if let Some(bytes) = variable {
            let path = format!("{storage}/__substg1.0_{tag:08X}");
            file.create_stream(&path)
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        }
    }
    let path = format!("{storage}/__properties_version1.0");
    file.create_stream(&path)
        .unwrap()
        .write_all(&table)
        .unwrap();
}

fn widen(bytes: &[u8]) -> [u8; 8] {
    let mut out = [0; 8];
    out[..bytes.len()].copy_from_slice(bytes);
    out
}

fn read(props: Props, attachments: &[Props]) -> Message {
    Message::from_bytes(&build(props, attachments)).unwrap()
}

/// A Message of `class` with a subject and the given Attachments.
fn with_class(class: &str, attachments: &[Props]) -> Message {
    read(
        &[
            (PID_MESSAGE_CLASS, Value::Unicode(class)),
            (PID_SUBJECT, Value::Unicode("Quarterly figures")),
        ],
        attachments,
    )
}

fn kind(class: &str) -> MessageKind {
    with_class(class, &[]).kind()
}

/// The single Attachment of an S/MIME Message: `smime.p7m` with the given
/// MIME type and data.
fn smime_attachment<'a>(mime_type: &'a str, data: &'a [u8]) -> Vec<(u16, Value<'a>)> {
    vec![
        (PID_ATTACH_METHOD, Value::Long(BY_VALUE)),
        (PID_ATTACH_LONG_FILENAME, Value::Unicode("smime.p7m")),
        (PID_ATTACH_MIME_TAG, Value::Unicode(mime_type)),
        (PID_ATTACH_DATA, Value::Binary(data)),
    ]
}

#[test]
fn ordinary_email_is_a_note() {
    assert_eq!(kind("IPM.Note"), MessageKind::Note);
    assert_eq!(kind("ipm.note"), MessageKind::Note);
    assert_eq!(kind("IPM.Note.Mobile.SMS"), MessageKind::Note);
}

#[test]
fn message_without_class_is_a_note() {
    let msg = read(&[(PID_SUBJECT, Value::Unicode("Hello"))], &[]);
    assert_eq!(msg.kind(), MessageKind::Note);
    assert_eq!(kind("  "), MessageKind::Note);
}

#[test]
fn other_classes_keep_their_class() {
    for class in [
        "IPM.Appointment",
        "IPM.Schedule.Meeting.Request",
        "IPM.Schedule.Meeting.Canceled",
        "IPM.Contact",
        "IPM.DistList",
        "IPM.Task",
        "IPM.StickyNote",
        "IPM.Post",
        "IPM.Notebook",
        "REPORT.IPM.Note.IPNRN",
        "REPORT.IPM.Note.SMIME.NDR",
    ] {
        assert_eq!(kind(class), MessageKind::Other(class.to_owned()));
    }
}

#[test]
fn meeting_request_has_subject_body_and_attachments() {
    let agenda: Props = &[
        (PID_ATTACH_METHOD, Value::Long(BY_VALUE)),
        (PID_ATTACH_LONG_FILENAME, Value::Unicode("agenda.txt")),
        (PID_ATTACH_DATA, Value::Binary(b"1. Figures\r\n2. AOB\r\n")),
    ];
    let msg = read(
        &[
            (
                PID_MESSAGE_CLASS,
                Value::Unicode("IPM.Schedule.Meeting.Request"),
            ),
            (PID_SUBJECT, Value::Unicode("Quarterly review")),
            (PID_SENDER_NAME, Value::Unicode("Ada Lovelace")),
            (PID_BODY, Value::Unicode("Let's go over the figures.")),
        ],
        &[agenda],
    );
    assert_eq!(
        msg.kind(),
        MessageKind::Other("IPM.Schedule.Meeting.Request".into())
    );
    assert_eq!(msg.subject(), Some("Quarterly review"));
    assert_eq!(
        msg.body(),
        Some(Body::PlainText("Let's go over the figures.".into()))
    );
    assert_eq!(msg.attachments()[0].file_name(), Some("agenda.txt"));
}

#[test]
fn clear_signed_message_is_signed() {
    let mime = b"Content-Type: multipart/signed; boundary=x\r\n\r\n--x--\r\n";
    let attachment = smime_attachment("multipart/signed", mime);
    let msg = read(
        &[
            (
                PID_MESSAGE_CLASS,
                Value::Unicode("IPM.Note.SMIME.MultipartSigned"),
            ),
            (PID_SUBJECT, Value::Unicode("Signed")),
            (PID_BODY, Value::Unicode("Readable copy of the Body")),
        ],
        &[&attachment],
    );
    assert_eq!(msg.kind(), MessageKind::Signed);
    assert!(msg.body().is_some());
    assert_eq!(kind("ipm.note.smime.multipartsigned"), MessageKind::Signed);
    // Outlook goes by the suffix alone.
    assert_eq!(
        kind("IPM.InfoPathForm.Expenses.SMIME.MultipartSigned"),
        MessageKind::Signed
    );
}

#[test]
fn clear_signed_message_labelled_smime_is_signed() {
    let attachment = smime_attachment("multipart/signed; protocol=x", b"--x--\r\n");
    assert_eq!(
        with_class("IPM.Note.SMIME", &[&attachment]).kind(),
        MessageKind::Signed
    );
}

#[test]
fn smime_message_is_told_apart_by_its_mime_type() {
    let encrypted = smime_attachment("application/pkcs7-mime; smime-type=enveloped-data", b"");
    assert_eq!(
        with_class("IPM.Note.SMIME", &[&encrypted]).kind(),
        MessageKind::Encrypted
    );
    let signed = smime_attachment("application/pkcs7-mime; smime-type=\"signed-data\"", b"");
    assert_eq!(
        with_class("IPM.Note.SMIME", &[&signed]).kind(),
        MessageKind::Signed
    );
}

#[test]
fn smime_message_is_told_apart_by_its_content_type() {
    let encrypted = smime_attachment("application/pkcs7-mime", CMS_ENVELOPED);
    assert_eq!(
        with_class("IPM.Note.SMIME", &[&encrypted]).kind(),
        MessageKind::Encrypted
    );
    let opaque_signed = smime_attachment("application/x-pkcs7-mime", CMS_SIGNED);
    assert_eq!(
        with_class("IPM.Note.SMIME", &[&opaque_signed]).kind(),
        MessageKind::Signed
    );
}

#[test]
fn unrecognised_smime_message_is_taken_as_encrypted() {
    assert_eq!(kind("IPM.Note.SMIME"), MessageKind::Encrypted);
    let garbage = smime_attachment("application/octet-stream", b"\x30");
    assert_eq!(
        with_class("ipm.note.smime", &[&garbage]).kind(),
        MessageKind::Encrypted
    );
    // Outlook goes by the suffix alone.
    assert_eq!(kind("IPM.Note.Receipt.SMIME"), MessageKind::Encrypted);
}

#[test]
fn legacy_secure_messages_are_signed_or_encrypted() {
    assert_eq!(kind("IPM.Note.Secure"), MessageKind::Encrypted);
    assert_eq!(kind("IPM.Note.Secure.Sign"), MessageKind::Signed);
    assert_eq!(kind("ipm.note.secure.sign"), MessageKind::Signed);
}

#[test]
fn rights_managed_message_is_encrypted() {
    let rpmsg: Props = &[
        (PID_ATTACH_METHOD, Value::Long(BY_VALUE)),
        (PID_ATTACH_LONG_FILENAME, Value::Unicode("message.rpmsg")),
        (
            PID_ATTACH_MIME_TAG,
            Value::Unicode("application/x-microsoft-rpmsg-message"),
        ),
        (PID_ATTACH_DATA, Value::Binary(b"\x76\xe8\x04\x60")),
    ];
    assert_eq!(
        with_class("IPM.Note", &[rpmsg]).kind(),
        MessageKind::Encrypted
    );
    // Only the first Attachment holds protected content.
    let other: Props = &[(PID_ATTACH_LONG_FILENAME, Value::Unicode("notes.txt"))];
    assert_eq!(
        with_class("IPM.Note", &[other, rpmsg]).kind(),
        MessageKind::Note
    );
}
