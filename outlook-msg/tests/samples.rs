use outlook_msg::{Error, Mailbox, Message};

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
