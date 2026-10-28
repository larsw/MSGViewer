//! RTF Bodies: Messages whose Body is Compressed RTF, alone or with a
//! plain-text or HTML Body, built here with `cfb` or taken from the samples.

#[path = "support/lzfu.rs"]
mod lzfu;

use std::io::{Cursor, Write};

use cfb::CompoundFile;
use outlook_msg::{Body, Limits, Message, PropValue};

const PID_SUBJECT: u16 = 0x0037;
const PID_RTF_IN_SYNC: u16 = 0x0E1F;
const PID_BODY: u16 = 0x1000;
const PID_RTF_COMPRESSED: u16 = 0x1009;
const PID_HTML: u16 = 0x1013;

const RTF_STREAM: &str = "/__substg1.0_10090102";

enum Value {
    Bool(bool),
    Unicode(String),
    Binary(Vec<u8>),
}

/// Builds a `.msg` file with the given Message properties, as
/// `tests/generated.rs` does.
fn build(props: &[(u16, Value)]) -> Vec<u8> {
    let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    let mut table = vec![0; 32];
    for (id, value) in props {
        let (ty, fixed, data): (u16, [u8; 8], Option<Vec<u8>>) = match value {
            Value::Bool(b) => (0x000B, [u8::from(*b), 0, 0, 0, 0, 0, 0, 0], None),
            Value::Unicode(s) => {
                let mut data: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                data.extend_from_slice(&[0, 0]);
                (0x001F, len(&data), Some(data))
            }
            Value::Binary(data) => (0x0102, len(data), Some(data.clone())),
        };
        let tag = (u32::from(*id) << 16) | u32::from(ty);
        table.extend_from_slice(&tag.to_le_bytes());
        table.extend_from_slice(&6u32.to_le_bytes());
        table.extend_from_slice(&fixed);
        if let Some(data) = data {
            let path = format!("/__substg1.0_{tag:08X}");
            file.create_stream(&path).unwrap().write_all(&data).unwrap();
        }
    }
    file.create_stream("/__properties_version1.0")
        .unwrap()
        .write_all(&table)
        .unwrap();
    file.flush().unwrap();
    file.into_inner().into_inner()
}

fn len(data: &[u8]) -> [u8; 8] {
    let mut fixed = [0; 8];
    fixed[..4].copy_from_slice(&(data.len() as u32).to_le_bytes());
    fixed
}

fn read(props: &[(u16, Value)]) -> Message {
    Message::from_bytes(&build(props)).unwrap()
}

fn rtf(rtf: &str) -> (u16, Value) {
    (
        PID_RTF_COMPRESSED,
        Value::Binary(lzfu::compress(rtf.as_bytes())),
    )
}

fn plain(text: &str) -> (u16, Value) {
    (PID_BODY, Value::Unicode(text.into()))
}

fn subject() -> (u16, Value) {
    (PID_SUBJECT, Value::Unicode("RTF".into()))
}

fn sample(name: &str) -> Message {
    let path = format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"));
    Message::from_bytes(&std::fs::read(path).unwrap()).unwrap()
}

/// The Message of sample `name` with nothing but its Compressed RTF, as
/// many Messages are stored.
fn rtf_only(name: &str) -> Message {
    let Some(PropValue::Binary(data)) = sample(name).properties().get(PID_RTF_COMPRESSED).cloned()
    else {
        panic!("{name} has no Compressed RTF");
    };
    read(&[subject(), (PID_RTF_COMPRESSED, Value::Binary(data))])
}

const ENCAPSULATED_HTML: &str = "{\\rtf1\\ansi\\ansicpg1252\\fromhtml1 \\deff0{\\fonttbl{\\f0\\fswiss Arial;}}\
    {\\*\\htmltag19 <html>}{\\*\\htmltag50 <body>}\\htmlrtf {\\htmlrtf0 Caf\\'e9 \
    {\\*\\htmltag84 <img src=\"https://example.org/x.png\">}\\htmlrtf }\\htmlrtf0 \
    {\\*\\htmltag58 </body>}{\\*\\htmltag27 </html>}}";
const ENCAPSULATED_HTML_BODY: &str =
    "<html><body>Café <img src=\"https://example.org/x.png\"></body></html>";

const ENCAPSULATED_TEXT: &str = "{\\rtf1\\ansi\\ansicpg1252\\fromtext \\deff0{\\fonttbl{\\f0\\fswiss Arial;}}\
    \\uc1\\pard\\plain\\f0\\fs20 Caf\\'e9\\par\r\nna\\u239?ve\\tab x\\par\r\n}";

const GENUINE: &str = "{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fswiss Arial;}}\
    {\\colortbl ;\\red255\\green0\\blue0;}\\pard\\f0\\fs20 Plain and {\\b bold}\\par\r\n}";

#[test]
fn encapsulated_html_is_the_body_of_an_rtf_only_message() {
    let msg = read(&[subject(), rtf(ENCAPSULATED_HTML)]);
    assert_eq!(msg.body(), Some(Body::Html(ENCAPSULATED_HTML_BODY.into())));
    assert!(!msg.is_partial());
}

#[test]
fn encapsulated_html_wins_over_plain_text_body() {
    let msg = read(&[subject(), plain("Café"), rtf(ENCAPSULATED_HTML)]);
    assert_eq!(msg.body(), Some(Body::Html(ENCAPSULATED_HTML_BODY.into())));
}

#[test]
fn html_body_wins_over_compressed_rtf() {
    let html = (PID_HTML, Value::Binary(b"<p>Original</p>".to_vec()));
    let msg = read(&[subject(), html, rtf(ENCAPSULATED_HTML)]);
    assert_eq!(msg.body(), Some(Body::Html("<p>Original</p>".into())));

    // The RTF isn't needed then, so damage to it doesn't matter.
    let html = (PID_HTML, Value::Binary(b"<p>Original</p>".to_vec()));
    let damaged = (
        PID_RTF_COMPRESSED,
        Value::Binary(b"not compressed RTF".to_vec()),
    );
    let msg = read(&[subject(), html, damaged]);
    assert_eq!(msg.body(), Some(Body::Html("<p>Original</p>".into())));
    assert!(!msg.is_partial());

    // A blank HTML Body doesn't count.
    let html = (PID_HTML, Value::Binary(b" \r\n".to_vec()));
    let msg = read(&[subject(), html, rtf(ENCAPSULATED_HTML)]);
    assert_eq!(msg.body(), Some(Body::Html(ENCAPSULATED_HTML_BODY.into())));
}

#[test]
fn encapsulated_plain_text_is_recovered() {
    let msg = read(&[subject(), rtf(ENCAPSULATED_TEXT)]);
    assert_eq!(
        msg.body(),
        Some(Body::PlainText("Café\r\nnaïve\tx\r\n".into()))
    );
}

#[test]
fn plain_text_body_wins_over_encapsulated_plain_text_unless_rtf_is_newer() {
    let msg = read(&[subject(), plain("Café"), rtf(ENCAPSULATED_TEXT)]);
    assert_eq!(msg.body(), Some(Body::PlainText("Café".into())));
    let in_sync = (PID_RTF_IN_SYNC, Value::Bool(true));
    let msg = read(&[subject(), plain("Café"), in_sync, rtf(ENCAPSULATED_TEXT)]);
    assert_eq!(msg.body(), Some(Body::PlainText("Café".into())));

    let out_of_sync = (PID_RTF_IN_SYNC, Value::Bool(false));
    let msg = read(&[subject(), plain("Old"), out_of_sync, rtf(ENCAPSULATED_TEXT)]);
    assert_eq!(
        msg.body(),
        Some(Body::PlainText("Café\r\nnaïve\tx\r\n".into()))
    );
}

#[test]
fn genuine_rtf_shows_plain_text_body() {
    let msg = read(&[subject(), plain("Plain and bold"), rtf(GENUINE)]);
    assert_eq!(msg.body(), Some(Body::GenuineRtf("Plain and bold".into())));
}

#[test]
fn genuine_rtf_without_plain_text_body_shows_its_text() {
    let msg = read(&[subject(), rtf(GENUINE)]);
    assert_eq!(
        msg.body(),
        Some(Body::GenuineRtf("Plain and bold\r\n".into()))
    );
    let blank = plain(" ");
    let msg = read(&[subject(), blank, rtf(GENUINE)]);
    assert_eq!(
        msg.body(),
        Some(Body::GenuineRtf("Plain and bold\r\n".into()))
    );
    let out_of_sync = (PID_RTF_IN_SYNC, Value::Bool(false));
    let msg = read(&[subject(), plain("Old"), out_of_sync, rtf(GENUINE)]);
    assert_eq!(
        msg.body(),
        Some(Body::GenuineRtf("Plain and bold\r\n".into()))
    );
}

#[test]
fn uncompressed_rtf_is_read_too() {
    let mela = lzfu::uncompressed(ENCAPSULATED_HTML.as_bytes());
    let msg = read(&[subject(), (PID_RTF_COMPRESSED, Value::Binary(mela))]);
    assert_eq!(msg.body(), Some(Body::Html(ENCAPSULATED_HTML_BODY.into())));
}

#[test]
fn damaged_rtf_is_a_problem_and_falls_back_to_plain_text_body() {
    let mut data = lzfu::compress(ENCAPSULATED_HTML.as_bytes());
    let last = data.len() - 1;
    data[last] ^= 0xFF;
    let msg = read(&[
        subject(),
        plain("Café"),
        (PID_RTF_COMPRESSED, Value::Binary(data)),
    ]);
    assert_eq!(msg.body(), Some(Body::PlainText("Café".into())));
    assert!(msg.is_partial());
    assert_eq!(msg.problems().len(), 1);
    assert_eq!(msg.problems()[0].path, RTF_STREAM);
    assert!(
        msg.problems()[0].reason.contains("CRC"),
        "{}",
        msg.problems()[0]
    );
    assert_eq!(msg.subject(), Some("RTF"));
}

#[test]
fn damaged_rtf_without_other_body_leaves_none() {
    for data in [
        vec![],
        b"LZFu".to_vec(),
        lzfu::with_header(&[0xFF, 0, 0], 100),
        lzfu::uncompressed(b"<html>not RTF</html>"),
    ] {
        let msg = read(&[subject(), (PID_RTF_COMPRESSED, Value::Binary(data))]);
        assert_eq!(msg.body(), None);
        assert_eq!(msg.problems().len(), 1);
        assert_eq!(msg.problems()[0].path, RTF_STREAM);
    }
}

#[test]
fn empty_rtf_is_no_body_and_no_problem() {
    // How Outlook compresses an empty RTF ([MS-OXRTFCP] 2.3.3.2).
    let empty = lzfu::with_header(&[0x02, 0x00, 0x0D, 0x00], 0);
    let msg = read(&[subject(), (PID_RTF_COMPRESSED, Value::Binary(empty))]);
    assert_eq!(msg.body(), None);
    assert!(!msg.is_partial());

    let empty = (PID_RTF_COMPRESSED, Value::Binary(lzfu::compress(b"")));
    let msg = read(&[subject(), plain("Text"), empty]);
    assert_eq!(msg.body(), Some(Body::PlainText("Text".into())));
    assert!(!msg.is_partial());
}

#[test]
fn blank_encapsulated_body_falls_back_to_plain_text_body() {
    let blank = "{\\rtf1\\ansi\\fromhtml1 {\\*\\htmltag64}\\htmlrtf Hidden\\htmlrtf0}";
    let msg = read(&[subject(), plain("Text"), rtf(blank)]);
    assert_eq!(msg.body(), Some(Body::PlainText("Text".into())));
}

#[test]
fn rtf_over_the_size_limit_is_a_problem() {
    let mut big = String::from("{\\rtf1\\ansi\\fromtext ");
    big.push_str(&"All work and no play. ".repeat(500));
    big.push('}');
    let bytes = build(&[subject(), plain("Short"), rtf(&big)]);
    let limits = Limits {
        max_stream_len: 4096,
        ..Limits::DEFAULT
    };
    let msg = Message::from_bytes_with_limits(&bytes, limits).unwrap();
    assert_eq!(msg.body(), Some(Body::PlainText("Short".into())));
    assert_eq!(msg.problems().len(), 1);
    assert_eq!(msg.problems()[0].path, RTF_STREAM);
    assert!(msg.problems()[0].reason.contains("limit"));

    // Within the limit, it is read.
    let msg = Message::from_bytes_with_limits(&bytes, Limits::DEFAULT).unwrap();
    assert!(!msg.is_partial());
}

#[test]
fn samples_with_rtf_and_plain_text_show_encapsulated_html() {
    let msg = sample("testMSG_att_doc.msg");
    assert_eq!(msg.html_body(), None);
    let Some(Body::Html(html)) = msg.body() else {
        panic!("expected an HTML Body");
    };
    assert!(html.starts_with("<html xmlns:o=\"urn:schemas-microsoft-com:office:office\""));
    assert!(html.trim_end().ends_with("</html>"));
}

#[test]
fn rtf_only_sample_shows_encapsulated_html() {
    let msg = rtf_only("test-outlook2003.msg");
    let Some(Body::Html(html)) = msg.body() else {
        panic!("expected an HTML Body");
    };
    assert!(html.starts_with("<HTML>\r\n<HEAD>\r\n"));
    assert!(html.contains("Thank you for using Microsoft&reg; Office Outlook&reg; 2003!"));
    assert!(html.contains("<A HREF=\"http://r.office.microsoft.com/r/rlidwelcomeFAQ?clid=1033\">frequently asked questions</a>"));
    assert!(!msg.is_partial());
}

#[test]
fn rtf_only_sample_in_utf_8_shows_encapsulated_html() {
    // \ansicpg65001, with \htmlrtf groups around RTF-only line breaks.
    let msg = rtf_only("message_1979.msg");
    let Some(Body::Html(html)) = msg.body() else {
        panic!("expected an HTML Body");
    };
    assert!(
        html.contains(
            "<DIV class=EVShortcutMsgBody>Hi George,\r\n\r\nI have spoken to David Taffs"
        )
    );
    assert!(html.contains("Regards\r\n\r\n&nbsp;&nbsp;&nbsp;&nbsp;Alec\r\n</DIV>"));
    assert!(html.ends_with("</BODY></HTML>"));
}

#[test]
fn encapsulated_plain_text_samples_match_their_plain_text_bodies() {
    for name in ["testMSG.msg", "cyrillic_message.msg"] {
        let msg = sample(name);
        let Some(Body::PlainText(text)) = rtf_only(name).body() else {
            panic!("{name}: expected a plain-text Body");
        };
        assert_eq!(
            text.trim_end(),
            msg.plain_text_body().unwrap().trim_end(),
            "{name}"
        );
        assert_eq!(
            msg.body(),
            Some(Body::PlainText(msg.plain_text_body().unwrap().into()))
        );
    }
    // \ansicpg1251, with the font in \fcharset204.
    let msg = sample("cyrillic_message.msg");
    assert!(
        msg.plain_text_body()
            .unwrap()
            .starts_with("Ваше сообщение с темой \"FW: RTM\" было успешно получено")
    );
}

#[test]
fn genuine_rtf_samples_show_plain_text_body() {
    for (name, text) in [
        ("quick.msg", "The quick brown fox jumps over the lazy dog"),
        (
            "testMSG_StickyNote.msg",
            "This is a note about the quick brown fox",
        ),
        ("testMSG_Contact.msg", "This was one quick brown fox."),
    ] {
        let msg = sample(name);
        let Some(Body::GenuineRtf(body)) = msg.body() else {
            panic!("{name}: expected a Genuine RTF Body");
        };
        assert_eq!(body, msg.plain_text_body().unwrap(), "{name}");
        assert!(body.starts_with(text), "{name}");
        // And without the plain-text Body, the text from the RTF.
        let Some(Body::GenuineRtf(rtf_text)) = rtf_only(name).body() else {
            panic!("{name}: expected a Genuine RTF Body");
        };
        assert!(rtf_text.starts_with(text), "{name}: {rtf_text:?}");
    }
}

#[test]
fn genuine_rtf_text_in_big5() {
    // \ansicpg950, with fonts in \fcharset136.
    let Some(Body::GenuineRtf(text)) = rtf_only("testMSG_chinese.msg").body() else {
        panic!("expected a Genuine RTF Body");
    };
    assert!(text.contains("Alfresco MSG format testing"), "{text}");
    assert!(text.contains("格式"), "{text}");
}

#[test]
fn every_sample_rtf_reads_without_problems() {
    let dir = format!("{}/tests/samples", env!("CARGO_MANIFEST_DIR"));
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "msg") {
            let name = path.file_name().unwrap().to_str().unwrap();
            if sample(name).properties().get(PID_RTF_COMPRESSED).is_some() {
                let msg = rtf_only(name);
                assert!(!msg.is_partial(), "{name}: {:?}", msg.problems());
                assert!(msg.body().is_some(), "{name}");
            }
        }
    }
}
