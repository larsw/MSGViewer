use std::time::{Duration, SystemTime};

use outlook_msg::{Error, Mailbox, Message, Recipient, RecipientKind};

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
