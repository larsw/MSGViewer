use outlook_msg::{Body, Error, Mailbox, Message};

fn sample(name: &str) -> Message {
    let path = format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"));
    Message::from_bytes(&std::fs::read(path).unwrap()).unwrap()
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
fn rejects_non_compound_file() {
    assert!(matches!(
        Message::from_bytes(b"From: someone\r\n\r\nhello"),
        Err(Error::NotCompoundFile(_))
    ));
}

#[test]
fn reads_string8_plain_text_body() {
    let msg = sample("testMSG.msg");
    let body = msg.plain_text_body().unwrap();
    assert!(body.starts_with("Hi,\r\n\r\nAs discussed, I've been working"));
    assert!(body.ends_with("Jukka Zitting\r\n"));
    assert_eq!(msg.body(), Some(Body::PlainText(body.to_owned())));
}

#[test]
fn reads_unicode_plain_text_body() {
    let msg = sample("test-outlook2003.msg");
    let body = msg.plain_text_body().unwrap();
    assert!(body.contains("Thank you for using Microsoft® Office Outlook® 2003!"));
    assert!(body.contains("<http://r.office.microsoft.com/r/rlidwelcomeFAQ?clid=1033>"));
}
