//! Edge cases on small `.msg` files built here with `cfb`, laid out as in
//! [MS-OXMSG] 2.4.

use std::io::{Cursor, Write};
use std::time::{Duration, SystemTime};

use cfb::CompoundFile;
use outlook_msg::{Body, Mailbox, Message, Recipient, RecipientKind};

const PID_SUBJECT: u16 = 0x0037;
const PID_CLIENT_SUBMIT_TIME: u16 = 0x0039;
const PID_RECIPIENT_TYPE: u16 = 0x0C15;
const PID_SENDER_NAME: u16 = 0x0C1A;
const PID_SENDER_ADDRTYPE: u16 = 0x0C1E;
const PID_SENDER_EMAIL: u16 = 0x0C1F;
const PID_MESSAGE_DELIVERY_TIME: u16 = 0x0E06;
const PID_BODY: u16 = 0x1000;
const PID_HTML: u16 = 0x1013;
const PID_DISPLAY_NAME: u16 = 0x3001;
const PID_ADDRTYPE: u16 = 0x3002;
const PID_EMAIL_ADDRESS: u16 = 0x3003;
const PID_SMTP_ADDRESS: u16 = 0x39FE;
const PID_INTERNET_CODEPAGE: u16 = 0x3FDE;
const PID_MESSAGE_CODEPAGE: u16 = 0x3FFD;

const TO: i32 = 1;
const CC: i32 = 2;
const BCC: i32 = 3;

/// FILETIME of 2023-11-14 22:13:20 UTC (Unix time 1 700 000 000).
const FILETIME_2023: u64 = (1_700_000_000 + 11_644_473_600) * 10_000_000;

#[derive(Clone)]
enum Value {
    Long(i32),
    Time(u64),
    String8(Vec<u8>),
    Unicode(String),
    Binary(Vec<u8>),
}

type Props = Vec<(u16, Value)>;

fn string8(bytes: &[u8]) -> Value {
    Value::String8(bytes.to_vec())
}

fn unicode(s: &str) -> Value {
    Value::Unicode(s.to_owned())
}

/// Builds a `.msg` file with the given Message properties and recipients.
fn build(props: &Props, recipients: &[Props]) -> Vec<u8> {
    let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    // Top-level header: reserved, next recipient ID, next attachment ID,
    // recipient count, attachment count, reserved.
    let mut header = vec![0; 8];
    for n in [recipients.len(), 0, recipients.len(), 0] {
        header.extend_from_slice(&(n as u32).to_le_bytes());
    }
    header.extend_from_slice(&[0; 8]);
    write_storage(&mut file, "", header, props);
    for (i, recipient) in recipients.iter().enumerate() {
        let storage = format!("/__recip_version1.0_#{i:08X}");
        file.create_storage(&storage).unwrap();
        write_storage(&mut file, &storage, vec![0; 8], recipient);
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
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
            Value::Time(t) => (0x0040, t.to_le_bytes(), None),
            Value::String8(s) => {
                let data = [s.as_slice(), &[0]].concat();
                (
                    0x001E,
                    widen(&(data.len() as u32).to_le_bytes()),
                    Some(data),
                )
            }
            Value::Binary(data) => (
                0x0102,
                widen(&(data.len() as u32).to_le_bytes()),
                Some(data.clone()),
            ),
            Value::Unicode(s) => {
                let mut data: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                data.extend_from_slice(&[0, 0]);
                (
                    0x001F,
                    widen(&(data.len() as u32).to_le_bytes()),
                    Some(data),
                )
            }
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

fn read(props: &Props, recipients: &[Props]) -> Message {
    Message::from_bytes(&build(props, recipients)).unwrap()
}

fn recipient(kind: RecipientKind, name: &str, address: Option<&str>) -> Recipient {
    Recipient {
        kind,
        mailbox: Mailbox {
            name: Some(name.into()),
            address: address.map(Into::into),
        },
    }
}

#[test]
fn decodes_8_bit_strings_with_message_codepage_windows_1251() {
    let msg = read(
        &vec![
            (PID_MESSAGE_CODEPAGE, Value::Long(1251)),
            (PID_INTERNET_CODEPAGE, Value::Long(1252)),
            (PID_SUBJECT, string8(b"\xcf\xf0\xe8\xe2\xe5\xf2")),
            (
                PID_SENDER_NAME,
                string8(b"\xc8\xe2\xe0\xed \xcf\xe5\xf2\xf0\xee\xe2"),
            ),
        ],
        &[vec![
            (PID_RECIPIENT_TYPE, Value::Long(TO)),
            (
                PID_DISPLAY_NAME,
                string8(b"\xc8\xe2\xe0\xed \xcf\xe5\xf2\xf0\xee\xe2"),
            ),
        ]],
    );
    assert_eq!(msg.subject(), Some("Привет"));
    assert_eq!(msg.sender().unwrap().name.as_deref(), Some("Иван Петров"));
    // Recipients use the Message's codepage.
    assert_eq!(
        msg.recipients(),
        [recipient(RecipientKind::To, "Иван Петров", None)]
    );
}

#[test]
fn decodes_8_bit_strings_with_message_codepage_shift_jis() {
    let msg = read(
        &vec![
            (PID_MESSAGE_CODEPAGE, Value::Long(932)),
            (
                PID_SUBJECT,
                string8(b"\x89\xef\x8b\x63\x82\xcc\x8b\x63\x8e\x96\x98\x5e"),
            ),
        ],
        &[vec![
            (PID_RECIPIENT_TYPE, Value::Long(CC)),
            (
                PID_DISPLAY_NAME,
                string8(b"\x8e\x52\x93\x63\x91\xbe\x98\x59"),
            ),
        ]],
    );
    assert_eq!(msg.subject(), Some("会議の議事録"));
    assert_eq!(
        msg.recipients(),
        [recipient(RecipientKind::Cc, "山田太郎", None)]
    );
}

#[test]
fn falls_back_to_internet_codepage_windows_1250() {
    let msg = read(
        &vec![
            (PID_INTERNET_CODEPAGE, Value::Long(1250)),
            (
                PID_SUBJECT,
                string8(b"\x8e\x6c\x75\x9d\x6f\x75\xe8\x6b\xfd\x20\x6b\xf9\xf2"),
            ),
        ],
        &[],
    );
    assert_eq!(msg.subject(), Some("Žluťoučký kůň"));
}

#[test]
fn falls_back_to_windows_1252_without_codepage() {
    let msg = read(&vec![(PID_SUBJECT, string8(b"Caf\xe9 \x80 5"))], &[]);
    assert_eq!(msg.subject(), Some("Café € 5"));
}

#[test]
fn ignores_utf_16_codepage_for_8_bit_strings() {
    let msg = read(
        &vec![
            (PID_MESSAGE_CODEPAGE, Value::Long(1200)),
            (PID_SUBJECT, string8(b"Caf\xe9")),
        ],
        &[],
    );
    assert_eq!(msg.subject(), Some("Café"));
}

#[test]
fn unicode_strings_ignore_codepage() {
    let msg = read(
        &vec![
            (PID_MESSAGE_CODEPAGE, Value::Long(1251)),
            (PID_SUBJECT, unicode("Grüße, 世界 🌍")),
        ],
        &[vec![(PID_DISPLAY_NAME, unicode("Zoë"))]],
    );
    assert_eq!(msg.subject(), Some("Grüße, 世界 🌍"));
    assert_eq!(
        msg.recipients(),
        [recipient(RecipientKind::To, "Zoë", None)]
    );
}

#[test]
fn missing_properties_are_none() {
    let msg = read(&vec![], &[]);
    assert_eq!(msg.message_class(), None);
    assert_eq!(msg.subject(), None);
    assert_eq!(msg.sender(), None);
    assert_eq!(msg.date(), None);
    assert!(msg.recipients().is_empty());
}

#[test]
fn date_prefers_submit_time_over_delivery_time() {
    let unix = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let msg = read(
        &vec![
            (
                PID_MESSAGE_DELIVERY_TIME,
                Value::Time(FILETIME_2023 + 600 * 10_000_000),
            ),
            (PID_CLIENT_SUBMIT_TIME, Value::Time(FILETIME_2023)),
        ],
        &[],
    );
    assert_eq!(msg.date(), Some(unix));

    let msg = read(
        &vec![(PID_MESSAGE_DELIVERY_TIME, Value::Time(FILETIME_2023))],
        &[],
    );
    assert_eq!(msg.date(), Some(unix));
}

#[test]
fn reads_many_recipients_in_order() {
    let mut recipients = Vec::new();
    let mut expected = Vec::new();
    for (kind, ty, count) in [
        (RecipientKind::To, TO, 40),
        (RecipientKind::Cc, CC, 12),
        (RecipientKind::Bcc, BCC, 3),
    ] {
        for i in 0..count {
            let name = format!("{kind:?} {i}");
            let address = format!("{}{i}@example.org", format!("{kind:?}").to_lowercase());
            recipients.push(vec![
                (PID_RECIPIENT_TYPE, Value::Long(ty)),
                (PID_DISPLAY_NAME, unicode(&name)),
                (PID_ADDRTYPE, unicode("SMTP")),
                (PID_EMAIL_ADDRESS, unicode(&address)),
            ]);
            expected.push(recipient(kind, &name, Some(&address)));
        }
    }
    let msg = read(&vec![(PID_SUBJECT, unicode("All hands"))], &recipients);
    assert_eq!(msg.recipients(), expected);
}

#[test]
fn recipient_addresses_prefer_smtp() {
    let msg = read(
        &vec![],
        &[
            // Exchange recipient with an SMTP address.
            vec![
                (PID_DISPLAY_NAME, unicode("Ada")),
                (PID_ADDRTYPE, unicode("EX")),
                (
                    PID_EMAIL_ADDRESS,
                    unicode("/O=EXAMPLE/CN=RECIPIENTS/CN=ADA"),
                ),
                (PID_SMTP_ADDRESS, unicode("ada@example.org")),
            ],
            // Exchange recipient without one: the EX address is not shown.
            vec![
                (PID_DISPLAY_NAME, unicode("Grace")),
                (PID_ADDRTYPE, unicode("EX")),
                (
                    PID_EMAIL_ADDRESS,
                    unicode("/O=EXAMPLE/CN=RECIPIENTS/CN=GRACE"),
                ),
            ],
            // Only an address.
            vec![
                (PID_ADDRTYPE, unicode("SMTP")),
                (PID_EMAIL_ADDRESS, unicode("linus@example.org")),
            ],
        ],
    );
    assert_eq!(
        msg.recipients(),
        [
            recipient(RecipientKind::To, "Ada", Some("ada@example.org")),
            recipient(RecipientKind::To, "Grace", None),
            Recipient {
                kind: RecipientKind::To,
                mailbox: Mailbox {
                    name: None,
                    address: Some("linus@example.org".into()),
                },
            },
        ]
    );
}

#[test]
fn recipient_type_flags_and_other_types() {
    const MAPI_SUBMITTED: i32 = 0x8000_0000_u32 as i32;
    const MAPI_P1: i32 = 0x1000_0000;
    let named = |ty: i32, name: &str| {
        vec![
            (PID_RECIPIENT_TYPE, Value::Long(ty)),
            (PID_DISPLAY_NAME, unicode(name)),
        ]
    };
    let msg = read(
        &vec![],
        &[
            named(CC | MAPI_SUBMITTED, "Submitted"),
            named(MAPI_P1, "Resend copy"),
            named(0, "Originator"),
            named(BCC, "Hidden"),
            // Names nobody, so it is left out.
            vec![(PID_RECIPIENT_TYPE, Value::Long(TO))],
        ],
    );
    assert_eq!(
        msg.recipients(),
        [
            recipient(RecipientKind::Cc, "Submitted", None),
            recipient(RecipientKind::Bcc, "Hidden", None),
        ]
    );
}

#[test]
fn sender_ignores_non_smtp_email() {
    let msg = read(
        &vec![
            (PID_SENDER_NAME, unicode("Ada")),
            (PID_SENDER_ADDRTYPE, unicode("EX")),
            (PID_SENDER_EMAIL, unicode("/O=EXAMPLE/CN=ADA")),
        ],
        &[],
    );
    assert_eq!(
        msg.sender(),
        Some(Mailbox {
            name: Some("Ada".into()),
            address: None,
        })
    );
}

fn html_body(props: Props) -> Option<String> {
    match read(&props, &[]).body()? {
        Body::Html(html) => Some(html),
        Body::PlainText(_) => None,
    }
}

#[test]
fn html_body_is_preferred_over_plain_text() {
    let msg = read(
        &vec![
            (PID_BODY, unicode("Plain")),
            (PID_HTML, Value::Binary(b"<p>Rich</p>".to_vec())),
        ],
        &[],
    );
    assert_eq!(msg.body(), Some(Body::Html("<p>Rich</p>".into())));
    assert_eq!(msg.plain_text_body(), Some("Plain"));
}

#[test]
fn empty_html_body_falls_back_to_plain_text() {
    let msg = read(
        &vec![
            (PID_BODY, unicode("Plain")),
            (PID_HTML, Value::Binary(b" \r\n\0".to_vec())),
        ],
        &[],
    );
    assert_eq!(msg.body(), Some(Body::PlainText("Plain".into())));
}

#[test]
fn html_body_charset_comes_from_meta_first() {
    let html = b"<html><head><meta http-equiv=\"Content-Type\" \
                 content=\"text/html; charset=windows-1251\"></head>\
                 <body>\xcf\xf0\xe8\xe2\xe5\xf2</body></html>";
    assert_eq!(
        html_body(vec![
            (PID_INTERNET_CODEPAGE, Value::Long(932)),
            (PID_MESSAGE_CODEPAGE, Value::Long(1252)),
            (PID_HTML, Value::Binary(html.to_vec())),
        ])
        .unwrap(),
        "<html><head><meta http-equiv=\"Content-Type\" \
         content=\"text/html; charset=windows-1251\"></head>\
         <body>Привет</body></html>"
    );
}

#[test]
fn html_body_charset_falls_back_to_internet_then_message_codepage() {
    let html = || Value::Binary(b"<p>\x8e\x52\x93\x63</p>\0".to_vec());
    assert_eq!(
        html_body(vec![
            (PID_INTERNET_CODEPAGE, Value::Long(932)),
            (PID_MESSAGE_CODEPAGE, Value::Long(1252)),
            (PID_HTML, html()),
        ]),
        Some("<p>山田</p>".into())
    );
    assert_eq!(
        html_body(vec![
            (PID_INTERNET_CODEPAGE, Value::Long(1200)),
            (PID_MESSAGE_CODEPAGE, Value::Long(932)),
            (PID_HTML, html()),
        ]),
        Some("<p>山田</p>".into())
    );
    assert_eq!(
        html_body(vec![(PID_HTML, html())]),
        Some("<p>\u{17d}R\u{201c}c</p>".into())
    );
}

#[test]
fn string8_html_body_ignores_message_codepage_when_meta_says_otherwise() {
    // 8-bit string Properties use the Message codepage, but the HTML Body
    // declares its own charset.
    let msg = read(
        &vec![
            (PID_MESSAGE_CODEPAGE, Value::Long(1251)),
            (PID_SUBJECT, string8(b"\xcf\xf0\xe8\xe2\xe5\xf2")),
            (
                PID_HTML,
                string8(b"<meta charset=\"utf-8\"><p>Gr\xc3\xbc\xc3\x9fe</p>"),
            ),
        ],
        &[],
    );
    assert_eq!(msg.subject(), Some("Привет"));
    assert_eq!(
        msg.body(),
        Some(Body::Html("<meta charset=\"utf-8\"><p>Grüße</p>".into()))
    );
}

#[test]
fn unicode_html_body_ignores_its_meta_charset() {
    assert_eq!(
        html_body(vec![(
            PID_HTML,
            unicode("<meta charset=\"big5\"><p>Grüße, 世界</p>")
        )]),
        Some("<meta charset=\"big5\"><p>Grüße, 世界</p>".into())
    );
}

/// Writes generated `.msg` files for looking at the app with, e.g.
/// `MSG_FIXTURE_DIR=/tmp/fixtures cargo test -p outlook-msg -- --ignored`.
#[test]
#[ignore]
fn write_fixtures() {
    let dir = std::env::var("MSG_FIXTURE_DIR").expect("MSG_FIXTURE_DIR is not set");
    let mut recipients = Vec::new();
    for (ty, count) in [(TO, 25), (CC, 8), (BCC, 2)] {
        for i in 0..count {
            recipients.push(vec![
                (PID_RECIPIENT_TYPE, Value::Long(ty)),
                (PID_DISPLAY_NAME, unicode(&format!("Person Number{i}"))),
                (PID_ADDRTYPE, unicode("SMTP")),
                (
                    PID_EMAIL_ADDRESS,
                    unicode(&format!("person.number{i}@example.org")),
                ),
            ]);
        }
    }
    let many = build(
        &vec![
            (
                PID_SUBJECT,
                unicode("Quarterly planning for the whole department"),
            ),
            (PID_SENDER_NAME, unicode("Ada Lovelace")),
            (PID_SENDER_ADDRTYPE, unicode("SMTP")),
            (PID_SENDER_EMAIL, unicode("ada@example.org")),
            (PID_CLIENT_SUBMIT_TIME, Value::Time(FILETIME_2023)),
        ],
        &recipients,
    );
    std::fs::write(format!("{dir}/many-recipients.msg"), many).unwrap();

    // Every way an HTML Body can ask for Remote content, each at its own
    // path on MSG_FIXTURE_REMOTE (a local test server, 127.0.0.1:8765 by
    // default), so its log shows what got through. Host names under
    // .msgv-test.invalid show up only as DNS lookups.
    let remote = std::env::var("MSG_FIXTURE_REMOTE").unwrap_or("127.0.0.1:8765".into());
    let html = REMOTE_CONTENT_HTML.replace("{remote}", &remote);
    let remote_content = build(
        &vec![
            (PID_SUBJECT, unicode("Remote content in every form")),
            (PID_SENDER_NAME, unicode("Ada Lovelace")),
            (PID_SENDER_ADDRTYPE, unicode("SMTP")),
            (PID_SENDER_EMAIL, unicode("ada@example.org")),
            (PID_CLIENT_SUBMIT_TIME, Value::Time(FILETIME_2023)),
            (PID_BODY, unicode("Plain-text Body, not shown.")),
            (PID_HTML, Value::Binary(html.into_bytes())),
        ],
        &[],
    );
    std::fs::write(format!("{dir}/remote-content.msg"), remote_content).unwrap();
}

const REMOTE_CONTENT_HTML: &str = r##"<!DOCTYPE html>
<html><head>
<meta http-equiv="Content-Type" content="text/html; charset=utf-8">
<meta http-equiv="x-dns-prefetch-control" content="on">
<meta http-equiv="refresh" content="2;url=http://{remote}/meta-refresh">
<base href="http://{remote}/base/">
<link rel="stylesheet" href="http://{remote}/link-stylesheet.css">
<link rel="preload" as="image" href="http://{remote}/link-preload.png">
<link rel="prefetch" href="http://{remote}/link-prefetch">
<link rel="preconnect" href="http://{remote}/">
<link rel="preconnect" href="http://preconnect.msgv-test.invalid/">
<link rel="dns-prefetch" href="http://dns-prefetch.msgv-test.invalid/">
<link rel="icon" href="http://{remote}/link-icon.png">
<script type="speculationrules">{"prefetch": [{"source": "list", "urls": ["http://{remote}/speculation-rules"]}]}</script>
<script src="http://{remote}/script.js"></script>
<style>
@import url("http://{remote}/css-import.css");
@font-face { font-family: Remote; src: url(http://{remote}/css-font.woff2) format("woff2"); }
body { font-family: Remote, sans-serif; background-image: url('http://{remote}/css-background.png'); }
.set { background-image: image-set("http://{remote}/css-image-set.png" 1x); }
</style>
</head>
<body background="http://{remote}/body-background.png">
<h1>Quarterly newsletter</h1>
<p>Dear reader, this Message asks for Remote content in every way we know of.
Read <a href="http://link.msgv-test.invalid/article" ping="http://{remote}/a-ping">the full article</a>.</p>
<p>An image embedded in the Body: <img alt="embedded" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAAAFklEQVR42mPQW+BMEmIY1TCqYfhqAAAOjhEQeUYZuQAAAABJRU5ErkJggg==" width="16" height="16"></p>
<p>Remote images:
<img alt="img src" src="http://{remote}/img-src.png" width="40" height="40">
<img alt="srcset" srcset="http://{remote}/img-srcset.png 1x, http://{remote}/img-srcset-2x.png 2x" width="40" height="40">
<picture><source srcset="http://{remote}/picture-source.png"><img alt="picture" src="relative.png" width="40" height="40"></picture>
<img alt="protocol-relative" src="//{remote}/protocol-relative.png" width="40" height="40">
<img alt="lazy" loading="lazy" src="http://{remote}/img-lazy.png" width="40" height="40">
<input type="image" alt="input" src="http://{remote}/input-image.png" width="40" height="40">
<svg width="40" height="40"><image href="http://{remote}/svg-image.png" width="40" height="40"/><use href="http://{remote}/svg-use.svg#a"/></svg>
</p>
<table background="http://{remote}/table-background.png"><tr>
<td style="background: url(http://{remote}/td-style.png)" width="120">Styled cell</td>
<td class="set" width="120">image-set</td></tr></table>
<p>An Inline attachment, not resolved yet: <img alt="cid" src="cid:image001.png@01D0A524.96D40F30" width="40" height="40"></p>
<iframe src="http://{remote}/iframe.html" width="80" height="40"></iframe>
<object data="http://{remote}/object.png" width="40" height="40"></object>
<embed src="http://{remote}/embed.png" width="40" height="40">
<video poster="http://{remote}/video-poster.png" src="http://{remote}/video.mp4" width="40" height="40"></video>
<audio src="http://{remote}/audio.mp3"></audio>
<form action="http://{remote}/form"><input name="q" value="form"></form>
</body></html>
"##;
