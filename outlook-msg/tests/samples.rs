use std::time::{Duration, SystemTime};

use outlook_msg::{Error, Mailbox, Message, Recipient, RecipientKind};

const PID_ATTACH_FILENAME: u16 = 0x3704;
const CFB_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

fn sample(name: &str) -> Message {
    let path = format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"));
    Message::from_bytes(&std::fs::read(path).unwrap()).unwrap()
}

fn unix(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn to(name: &str, address: Option<&str>) -> Recipient {
    Recipient {
        kind: RecipientKind::To,
        mailbox: Mailbox {
            name: Some(name.into()),
            address: address.map(Into::into),
        },
    }
}

#[test]
fn reads_subject_and_sender() {
    let msg = sample("testMSG.msg");
    assert_eq!(msg.message_class(), Some("IPM.Note"));
    assert_eq!(msg.subject(), Some("MIME registry use cases"));
    assert_eq!(
        msg.sender(),
        Some(Mailbox {
            name: Some("Jukka Zitting".into()),
            address: Some("jukka.zitting@gmail.com".into()),
        })
    );
}

#[test]
fn reads_date_and_recipients() {
    let msg = sample("testMSG.msg");
    assert_eq!(msg.date(), Some(unix(1_233_256_600)));
    assert_eq!(
        msg.recipients(),
        [to(
            "tika-dev@lucene.apache.org",
            Some("tika-dev@lucene.apache.org")
        )]
    );
}

#[test]
fn decodes_8_bit_strings_in_big5() {
    // No Message codepage; the Internet codepage is 950 (Big5).
    let msg = sample("testMSG_chinese.msg");
    assert_eq!(
        msg.subject(),
        Some("Alfresco MSG format testing ( MSG 格式測試 )")
    );
    assert_eq!(
        msg.sender().unwrap().name.as_deref(),
        Some("Tests Chang@FT (張毓倫)")
    );
    assert_eq!(
        msg.recipients(),
        [to(
            "Tests Chang@FT (張毓倫)",
            Some("tests.chang@fengttt.com")
        )]
    );
}

#[test]
fn decodes_8_bit_strings_in_iso_8859_1() {
    // No Message codepage; the Internet codepage is 28591 (ISO-8859-1).
    let msg = sample("test-outlook.msg");
    assert_eq!(msg.subject(), Some("Microsoft Outlook Express 6"));
    assert_eq!(
        msg.sender(),
        Some(Mailbox {
            name: Some("L'Équipe Microsoft Outlook Express".into()),
            address: Some("msoe@microsoft.com".into()),
        })
    );
    assert_eq!(msg.date(), Some(unix(1_175_790_366)));
}

#[test]
fn date_falls_back_to_delivery_time() {
    // Has PidTagMessageDeliveryTime but no PidTagClientSubmitTime.
    let msg = sample("testMSG_dateHeaderOnly.msg");
    assert_eq!(
        msg.date(),
        Some(unix(1_179_898_496) + Duration::from_nanos(734_375_000))
    );
}

#[test]
fn prefers_smtp_address_of_exchange_recipient() {
    let msg = sample("testMSG_forwarded.msg");
    assert_eq!(msg.subject(), Some("FW: Testing"));
    assert_eq!(
        msg.recipients(),
        [to("Paul Allan Hill", Some("paul@metajure.com"))]
    );
    assert_eq!(msg.date(), Some(unix(1_323_304_239)));
}

#[test]
fn rejects_non_compound_file() {
    assert!(matches!(
        Message::from_bytes(b"From: someone\r\n\r\nhello"),
        Err(Error::NotCompoundFile(_))
    ));
}

#[test]
fn message_without_attachments_has_none() {
    assert!(sample("testMSG.msg").attachments().is_empty());
}

#[test]
fn reads_file_attachments_with_long_and_short_names() {
    let msg = sample("testMSG_att_doc.msg");
    let attachments = msg.attachments();
    assert_eq!(attachments.len(), 2);

    let doc = &attachments[0];
    assert_eq!(doc.file_name(), Some("test-unicode.doc"));
    assert_eq!(
        doc.properties().string(PID_ATTACH_FILENAME),
        Some("TEST-U~1.DOC")
    );
    assert!(!doc.is_embedded_message());
    assert_eq!(doc.size(), Some(24064));
    assert!(doc.data().unwrap().starts_with(CFB_MAGIC));

    let txt = &attachments[1];
    assert_eq!(txt.file_name(), Some("pj1.txt"));
    assert_eq!(txt.size(), Some(89));
    assert!(txt.data().unwrap().starts_with("école".as_bytes()));
}

#[test]
fn reads_embedded_message_and_file_attachments() {
    let msg = sample("testMSG_att_msg.msg");
    let attachments = msg.attachments();
    assert_eq!(attachments.len(), 2);

    let embedded = &attachments[0];
    assert!(embedded.is_embedded_message());
    assert_eq!(embedded.file_name(), Some("Test Attachment"));
    assert_eq!(embedded.safe_file_name(), "Test Attachment.msg");
    let saved = Message::from_bytes(embedded.data().unwrap()).unwrap();
    assert_eq!(saved.subject(), Some("Test Attachment"));
    assert_eq!(
        embedded.embedded_message().unwrap().unwrap().subject(),
        Some("Test Attachment")
    );

    let pdf = &attachments[1];
    assert_eq!(pdf.file_name(), Some("smbprn.00009008.KdcPjl.pdf"));
    assert_eq!(
        pdf.properties().string(PID_ATTACH_FILENAME),
        Some("smbprn~1.pdf")
    );
    assert_eq!(pdf.size(), Some(13539));
    assert!(pdf.data().unwrap().starts_with(b"%PDF-1.4"));
}
