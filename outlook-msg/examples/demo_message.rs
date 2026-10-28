//! Writes the made-up Messages used for the app's screenshots:
//!
//!   cargo run -p outlook-msg --example demo_message -- OUT_DIR
//!
//! writes `OUT_DIR/project-update.msg` (an HTML Body with an Inline
//! attachment, recipients and three Attachments) and `OUT_DIR/newsletter.msg`
//! (an HTML Body asking for Remote content). Every person and address in them
//! is fictitious.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use cfb::CompoundFile;

// Property IDs ([MS-OXPROPS]).
const PID_MESSAGE_CLASS: u16 = 0x001A;
const PID_SUBJECT: u16 = 0x0037;
const PID_CLIENT_SUBMIT_TIME: u16 = 0x0039;
const PID_RECIPIENT_TYPE: u16 = 0x0C15;
const PID_SENDER_NAME: u16 = 0x0C1A;
const PID_SENDER_ADDRTYPE: u16 = 0x0C1E;
const PID_SENDER_EMAIL: u16 = 0x0C1F;
const PID_BODY: u16 = 0x1000;
const PID_HTML: u16 = 0x1013;
const PID_DISPLAY_NAME: u16 = 0x3001;
const PID_ADDRTYPE: u16 = 0x3002;
const PID_EMAIL_ADDRESS: u16 = 0x3003;
const PID_ATTACH_SIZE: u16 = 0x0E20;
const PID_ATTACH_DATA: u16 = 0x3701;
const PID_ATTACH_METHOD: u16 = 0x3705;
const PID_ATTACH_LONG_FILENAME: u16 = 0x3707;
const PID_ATTACH_MIME_TAG: u16 = 0x370E;
const PID_ATTACH_CONTENT_ID: u16 = 0x3712;
const PID_ATTACH_FLAGS: u16 = 0x3714;
const PID_SMTP_ADDRESS: u16 = 0x39FE;
const PID_ATTACHMENT_HIDDEN: u16 = 0x7FFE;

const TO: i32 = 1;
const CC: i32 = 2;
const BY_VALUE: i32 = 1;
const RENDERED_IN_BODY: i32 = 0x4;

/// FILETIME of 2026-10-15 07:30 UTC.
const SENT: u64 = (1_792_049_400 + 11_644_473_600) * 10_000_000;

enum Value {
    Long(i32),
    Bool(bool),
    Time(u64),
    Unicode(String),
    Binary(Vec<u8>),
}

type Props = Vec<(u16, Value)>;

fn text(s: &str) -> Value {
    Value::Unicode(s.to_owned())
}

struct Person {
    name: &'static str,
    address: &'static str,
}

struct Attachment {
    name: &'static str,
    mime: &'static str,
    data: Vec<u8>,
    content_id: Option<&'static str>,
}

struct Demo {
    subject: &'static str,
    sender: Person,
    recipients: Vec<(i32, Person)>,
    plain: &'static str,
    html: String,
    attachments: Vec<Attachment>,
}

fn main() {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: demo_message OUT_DIR"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    write(&dir.join("project-update.msg"), &project_update());
    write(&dir.join("newsletter.msg"), &newsletter());
}

fn project_update() -> Demo {
    Demo {
        subject: "Q3 project update",
        sender: Person {
            name: "Ada Lovelace",
            address: "ada@example.org",
        },
        recipients: vec![
            (
                TO,
                Person {
                    name: "Charles Babbage",
                    address: "charles@example.org",
                },
            ),
            (
                TO,
                Person {
                    name: "Grace Hopper",
                    address: "grace@example.org",
                },
            ),
            (
                CC,
                Person {
                    name: "Alan Turing",
                    address: "alan@example.org",
                },
            ),
        ],
        plain: "Hi all,\r\n\r\nHere's where the project stands at the end of Q3.\r\n",
        html: r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
body { font-family: sans-serif; color: #241f31; line-height: 1.45; }
h2 { color: #1a5fb4; font-size: 1.25em; margin-bottom: 0.3em; }
li { margin: 0.2em 0; }
.caption { color: #5e5c64; font-size: 0.9em; }
</style></head><body>
<p>Hi all,</p>
<p>Here's where the project stands at the end of Q3. The short version: we're ahead of plan, and the new viewer shipped on time.</p>
<h2>Highlights</h2>
<ul>
<li>Milestone 3 closed two weeks early</li>
<li>Customer satisfaction up to 4.6 out of 5</li>
<li>Support tickets down 18% since July</li>
</ul>
<p><img src="cid:progress-chart@example.org" width="520" height="220" alt="Progress by quarter"><br>
<span class="caption">Progress by quarter</span></p>
<p>The full report and the budget are attached. Let's go through them at Thursday's meeting.</p>
<p>Best,<br>Ada</p>
</body></html>"#
            .to_owned(),
        attachments: vec![
            Attachment {
                name: "progress-chart.png",
                mime: "image/png",
                data: include_bytes!("demo/chart.png").to_vec(),
                content_id: Some("progress-chart@example.org"),
            },
            Attachment {
                name: "Q3 report.pdf",
                mime: "application/pdf",
                data: pdf(),
                content_id: None,
            },
            Attachment {
                name: "Budget 2027.xlsx",
                mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                data: padded(b"PK\x03\x04", 48_213),
                content_id: None,
            },
            Attachment {
                name: "Meeting notes.txt",
                mime: "text/plain",
                data: b"Agenda for Thursday\n\n1. Q3 results\n2. Budget 2027\n3. Roadmap\n".to_vec(),
                content_id: None,
            },
        ],
    }
}

fn newsletter() -> Demo {
    Demo {
        subject: "Your October newsletter",
        sender: Person {
            name: "Example Weekly",
            address: "news@example.com",
        },
        recipients: vec![(
            TO,
            Person {
                name: "Grace Hopper",
                address: "grace@example.org",
            },
        )],
        plain: "This month at Example Weekly.\r\n",
        html: r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
body { font-family: sans-serif; color: #241f31; }
.banner { background: #e5a50a; color: #241f31; padding: 16px; border-radius: 8px; }
img { border: 1px solid #deddda; }
</style></head><body>
<div class="banner"><h2>This month at Example Weekly</h2></div>
<p><img src="https://example.com/newsletter/hero.jpg" width="480" height="160" alt="Autumn hiking"></p>
<p>Ten trails to walk before the first snow, a guide to waterproof boots, and our readers' best photos from September.</p>
<p><img src="https://example.com/newsletter/photos.jpg" width="480" height="120" alt="Reader photos"></p>
<p><a href="https://example.com/newsletter/october">Read online</a></p>
<img src="https://example.com/track/open.gif" width="1" height="1" alt="">
</body></html>"#
            .to_owned(),
        attachments: vec![],
    }
}

/// A small, valid one-page PDF.
fn pdf() -> Vec<u8> {
    padded(
        b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
          2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
          3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >> endobj\n\
          trailer << /Root 1 0 R >>\n%%EOF\n",
        184_320,
    )
}

/// `head` followed by zeros up to `len` bytes, for a realistic size.
fn padded(head: &[u8], len: usize) -> Vec<u8> {
    let mut data = head.to_vec();
    data.resize(len.max(head.len()), 0);
    data
}

fn write(path: &Path, demo: &Demo) {
    let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();

    let mut props: Props = vec![
        (PID_MESSAGE_CLASS, text("IPM.Note")),
        (PID_SUBJECT, text(demo.subject)),
        (PID_CLIENT_SUBMIT_TIME, Value::Time(SENT)),
        (PID_SENDER_NAME, text(demo.sender.name)),
        (PID_SENDER_ADDRTYPE, text("SMTP")),
        (PID_SENDER_EMAIL, text(demo.sender.address)),
        (PID_BODY, text(demo.plain)),
        (PID_HTML, Value::Binary(demo.html.clone().into_bytes())),
    ];
    // The Internet codepage of the HTML Body: UTF-8.
    props.push((0x3FDE, Value::Long(65001)));

    // Top-level header: reserved, next recipient ID, next attachment ID,
    // recipient count, attachment count, reserved ([MS-OXMSG] 2.4.1.1).
    let (recipients, attachments) = (demo.recipients.len(), demo.attachments.len());
    let mut header = vec![0; 8];
    for n in [recipients, attachments, recipients, attachments] {
        header.extend_from_slice(&(n as u32).to_le_bytes());
    }
    header.extend_from_slice(&[0; 8]);
    write_storage(&mut file, "", header, &props);

    for (i, (kind, person)) in demo.recipients.iter().enumerate() {
        let storage = format!("/__recip_version1.0_#{i:08X}");
        file.create_storage(&storage).unwrap();
        let props = vec![
            (PID_RECIPIENT_TYPE, Value::Long(*kind)),
            (PID_DISPLAY_NAME, text(person.name)),
            (PID_ADDRTYPE, text("SMTP")),
            (PID_EMAIL_ADDRESS, text(person.address)),
            (PID_SMTP_ADDRESS, text(person.address)),
        ];
        write_storage(&mut file, &storage, vec![0; 8], &props);
    }

    for (i, attachment) in demo.attachments.iter().enumerate() {
        let storage = format!("/__attach_version1.0_#{i:08X}");
        file.create_storage(&storage).unwrap();
        let mut props = vec![
            (PID_ATTACH_METHOD, Value::Long(BY_VALUE)),
            (PID_ATTACH_LONG_FILENAME, text(attachment.name)),
            (PID_DISPLAY_NAME, text(attachment.name)),
            (PID_ATTACH_MIME_TAG, text(attachment.mime)),
            (PID_ATTACH_SIZE, Value::Long(attachment.data.len() as i32)),
            (PID_ATTACH_DATA, Value::Binary(attachment.data.clone())),
        ];
        if let Some(id) = attachment.content_id {
            props.push((PID_ATTACH_CONTENT_ID, text(id)));
            props.push((PID_ATTACH_FLAGS, Value::Long(RENDERED_IN_BODY)));
            props.push((PID_ATTACHMENT_HIDDEN, Value::Bool(true)));
        }
        write_storage(&mut file, &storage, vec![0; 8], &props);
    }

    file.flush().unwrap();
    std::fs::write(path, file.into_inner().into_inner()).unwrap();
    println!("wrote {}", path.display());
}

fn write_storage(
    file: &mut CompoundFile<Cursor<Vec<u8>>>,
    storage: &str,
    mut table: Vec<u8>,
    props: &Props,
) {
    for (id, value) in props {
        let (ty, fixed, data): (u16, [u8; 8], Option<Vec<u8>>) = match value {
            Value::Long(n) => (0x0003, widen(&n.to_le_bytes()), None),
            Value::Bool(b) => (0x000B, widen(&[u8::from(*b)]), None),
            Value::Time(t) => (0x0040, t.to_le_bytes(), None),
            Value::Unicode(s) => {
                let mut data: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                data.extend_from_slice(&[0, 0]);
                (
                    0x001F,
                    widen(&(data.len() as u32).to_le_bytes()),
                    Some(data),
                )
            }
            Value::Binary(data) => (
                0x0102,
                widen(&(data.len() as u32).to_le_bytes()),
                Some(data.clone()),
            ),
        };
        let tag = (u32::from(*id) << 16) | u32::from(ty);
        table.extend_from_slice(&tag.to_le_bytes());
        table.extend_from_slice(&6u32.to_le_bytes()); // readable | writable
        table.extend_from_slice(&fixed);
        if let Some(data) = data {
            let path = format!("{storage}/__substg1.0_{tag:08X}");
            file.create_stream(&path).unwrap().write_all(&data).unwrap();
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
