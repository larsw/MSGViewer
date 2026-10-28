//! Truncated, corrupted and incomplete copies of a sample must give a Partial
//! read or an Unreadable file, never a panic.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use cfb::CompoundFile;
use outlook_msg::{Attachment, Error, Limits, Message, PropValue};

fn sample_bytes() -> Vec<u8> {
    sample("testMSG.msg")
}

fn sample(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/samples/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(path).unwrap()
}

/// The storages in the root storage whose names start with `prefix`.
fn root_storages(bytes: &[u8], prefix: &str) -> Vec<PathBuf> {
    let file = CompoundFile::open(Cursor::new(bytes)).unwrap();
    let mut storages: Vec<PathBuf> = file
        .read_root_storage()
        .filter(|entry| entry.is_storage() && entry.name().starts_with(prefix))
        .map(|entry| entry.path().to_path_buf())
        .collect();
    storages.sort();
    storages
}

fn file_names(message: &Message) -> Vec<Option<String>> {
    message
        .attachments()
        .iter()
        .map(|attachment| attachment.file_name().map(str::to_owned))
        .collect()
}

/// Every Property of `message`, in a comparable form.
fn all_properties(message: &Message) -> Vec<(u16, PropValue)> {
    (0..=u16::MAX)
        .filter_map(|id| Some((id, message.properties().get(id)?.clone())))
        .collect()
}

/// Copies a compound file, leaving out the stream at `removed`.
fn without_stream(bytes: &[u8], removed: &Path) -> Vec<u8> {
    let mut source = CompoundFile::open(Cursor::new(bytes)).unwrap();
    let mut copy = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    let entries: Vec<(PathBuf, bool)> = source
        .walk()
        .filter(|entry| !entry.is_root())
        .map(|entry| (entry.path().to_path_buf(), entry.is_stream()))
        .collect();
    for (path, is_stream) in entries {
        if !is_stream {
            copy.create_storage(&path).unwrap();
        } else if path != removed {
            let mut data = Vec::new();
            source
                .open_stream(&path)
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            copy.create_stream(&path).unwrap().write_all(&data).unwrap();
        }
    }
    copy.flush().unwrap();
    copy.into_inner().into_inner()
}

fn root_streams(bytes: &[u8]) -> Vec<PathBuf> {
    let file = CompoundFile::open(Cursor::new(bytes)).unwrap();
    file.read_root_storage()
        .filter(|entry| entry.is_stream())
        .map(|entry| entry.path().to_path_buf())
        .collect()
}

#[test]
fn intact_sample_is_not_partial() {
    let message = Message::from_bytes(&sample_bytes()).unwrap();
    assert!(!message.is_partial(), "{:?}", message.problems());
}

#[test]
fn truncated_file_is_partial_or_unreadable() {
    let bytes = sample_bytes();
    let complete = all_properties(&Message::from_bytes(&bytes).unwrap());
    let offsets = (0..bytes.len())
        .step_by(97)
        .chain([7, 8, 511, 512, 513, bytes.len() - 1]);
    for len in offsets {
        match Message::from_bytes(&bytes[..len]) {
            // A read without Problems must have everything.
            Ok(message) if !message.is_partial() => {
                assert_eq!(all_properties(&message), complete, "truncated to {len}")
            }
            Ok(_) | Err(Error::NotCompoundFile(_) | Error::Damaged(_) | Error::NotAMessage) => {}
        }
    }
}

#[test]
fn missing_property_stream_is_partial() {
    let bytes = sample_bytes();
    let subject = Path::new("/__substg1.0_0037001E");
    let message = Message::from_bytes(&without_stream(&bytes, subject)).unwrap();
    assert!(message.is_partial());
    assert_eq!(message.subject(), None);
    assert_eq!(message.message_class(), Some("IPM.Note"));
    assert_eq!(message.problems()[0].path, "/__substg1.0_0037001E");
}

#[test]
fn missing_html_body_falls_back_to_plain_text_body() {
    let bytes = sample("testMSG_chinese.msg");
    let html = Path::new("/__substg1.0_1013001E");
    let message = Message::from_bytes(&without_stream(&bytes, html)).unwrap();
    assert!(message.is_partial());
    assert_eq!(message.problems()[0].path, "/__substg1.0_1013001E");
    assert!(matches!(
        message.body(),
        Some(outlook_msg::Body::PlainText(text)) if text.contains("Alfresco MSG format testing")
    ));
}

#[test]
fn each_missing_root_stream_gives_partial_read_or_unreadable_file() {
    let bytes = sample_bytes();
    for stream in root_streams(&bytes) {
        let result = Message::from_bytes(&without_stream(&bytes, &stream));
        if stream.ends_with("__properties_version1.0") {
            assert!(matches!(result, Err(Error::NotAMessage)));
        } else if stream.to_string_lossy().starts_with("/__substg1.0_") {
            let message = result.unwrap();
            assert!(message.is_partial(), "{stream:?} removed");
            assert!(
                message
                    .problems()
                    .iter()
                    .any(|problem| Path::new(&problem.path) == stream)
            );
        }
    }
}

#[test]
fn not_a_message_is_unreadable() {
    let bytes = compound_file_with_stream("/WordDocument", b"text");
    assert!(matches!(
        Message::from_bytes(&bytes),
        Err(Error::NotAMessage)
    ));
}

fn compound_file_with_stream(path: &str, data: &[u8]) -> Vec<u8> {
    let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    file.create_stream(path).unwrap().write_all(data).unwrap();
    file.flush().unwrap();
    file.into_inner().into_inner()
}

#[test]
fn short_property_table_is_partial() {
    let bytes = compound_file_with_stream("/__properties_version1.0", &[0; 10]);
    assert!(Message::from_bytes(&bytes).unwrap().is_partial());
}

#[test]
fn property_table_ending_in_partial_entry_is_partial() {
    let mut table = vec![0; 32];
    // PidTagImportance, a PtypInteger32, then half an entry.
    table.extend([0x03, 0x00, 0x17, 0x00, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]);
    table.extend([0x1F, 0x00, 0x37, 0x00]);
    let bytes = compound_file_with_stream("/__properties_version1.0", &table);
    let message = Message::from_bytes(&bytes).unwrap();
    assert!(message.is_partial());
    assert_eq!(message.properties().get(0x0017), Some(&PropValue::Int(2)));
}

#[test]
fn damaged_compound_file_is_unreadable() {
    let bytes = sample_bytes();
    assert!(matches!(
        Message::from_bytes(&bytes[..600]),
        Err(Error::Damaged(_))
    ));
    assert!(matches!(
        Message::from_bytes(&bytes[..8]),
        Err(Error::Damaged(_))
    ));
}

#[test]
fn stream_over_size_limit_is_skipped() {
    let limits = Limits {
        max_stream_len: 2000,
        ..Limits::DEFAULT
    };
    let message = Message::from_bytes_with_limits(&sample_bytes(), limits).unwrap();
    assert!(message.is_partial());
    assert_eq!(message.subject(), Some("MIME registry use cases"));
    for problem in message.problems() {
        assert!(problem.reason.contains("limit of 2000"), "{problem}");
    }
}

#[test]
fn read_budget_is_enforced() {
    let limits = Limits {
        max_total_read: 2000,
        ..Limits::DEFAULT
    };
    let message = Message::from_bytes_with_limits(&sample_bytes(), limits).unwrap();
    assert!(message.is_partial());
}

/// A small, deterministic random number generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Damages `bytes` in one of several ways, chosen by `rng`.
fn mutate(bytes: &mut Vec<u8>, rng: &mut Rng) {
    let len = bytes.len();
    match rng.below(6) {
        // Flip a few bits.
        0 => {
            for _ in 0..1 + rng.below(8) {
                let i = rng.below(len);
                bytes[i] ^= 1 << rng.below(8);
            }
        }
        // Overwrite a run with random bytes.
        1 => {
            let start = rng.below(len);
            for byte in bytes.iter_mut().skip(start).take(1 + rng.below(64)) {
                *byte = rng.next() as u8;
            }
        }
        // Set a little-endian u32 to an interesting value, as found in
        // sector numbers, sizes and directory links.
        2 => {
            let i = rng.below(len - 4);
            let values = [0, 1, 0x7FFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFE, 0xFFFF_FFFA];
            let value: u32 = values[rng.below(values.len())];
            bytes[i..i + 4].copy_from_slice(&value.to_le_bytes());
        }
        // Copy one 512-byte sector over another.
        3 => {
            let sectors = len / 512;
            let (from, to) = (rng.below(sectors) * 512, rng.below(sectors) * 512);
            let sector = bytes[from..from + 512].to_vec();
            bytes[to..to + 512].copy_from_slice(&sector);
        }
        // Zero a sector.
        4 => {
            let start = rng.below(len / 512) * 512;
            bytes[start..start + 512].fill(0);
        }
        // Truncate.
        _ => bytes.truncate(rng.below(len)),
    }
}

#[test]
fn many_mutations_never_panic() {
    let originals = SAMPLES.map(sample);
    let mut rng = Rng(0x5EED_F00D_0A11_C0DE);
    let small_limits = Limits {
        max_stream_len: 4096,
        max_total_read: 16 * 1024,
        ..Limits::DEFAULT
    };
    let (mut partial, mut unreadable) = (0, 0);
    for round in 0..3000 {
        let mut bytes = originals[round % originals.len()].clone();
        for _ in 0..1 + rng.below(3) {
            if bytes.len() < 1024 {
                break;
            }
            mutate(&mut bytes, &mut rng);
        }
        let limits = if round % 4 == 0 {
            small_limits
        } else {
            Limits::DEFAULT
        };
        match Message::from_bytes_with_limits(&bytes, limits) {
            Ok(message) => {
                partial += usize::from(message.is_partial());
                // Reading the Message must not panic either.
                let _ = (message.subject(), message.sender(), message.date());
                for attachment in message.attachments() {
                    let _ = (attachment.safe_file_name(), attachment.size());
                    if let Some(Ok(embedded)) = attachment.embedded_message() {
                        let _ = (embedded.subject(), embedded.attachments().len());
                    }
                }
            }
            Err(_) => unreadable += 1,
        }
    }
    // Make sure the mutations reach both outcomes.
    assert!(partial > 0 && unreadable > 0, "{partial} {unreadable}");
}

const SAMPLES: [&str; 7] = [
    "testMSG.msg",
    "test-outlook.msg",
    "testMSG_att_doc.msg",
    "testMSG_att_msg.msg",
    "testMSG_chinese.msg",
    "testMSG_dateHeaderOnly.msg",
    "testMSG_forwarded.msg",
];

#[test]
fn intact_samples_are_not_partial() {
    for name in SAMPLES {
        let message = Message::from_bytes(&sample(name)).unwrap();
        assert!(!message.is_partial(), "{name}: {:?}", message.problems());
        for attachment in message.attachments() {
            if let Some(embedded) = attachment.embedded_message() {
                let embedded = embedded.unwrap();
                assert!(!embedded.is_partial(), "{name}: {:?}", embedded.problems());
            }
        }
    }
}

#[test]
fn damaged_recipient_is_left_out() {
    let mut checked = 0;
    for name in SAMPLES {
        let bytes = sample(name);
        let original = Message::from_bytes(&bytes).unwrap();
        for storage in root_storages(&bytes, "__recip_version1.0_#") {
            let table = storage.join("__properties_version1.0");
            let message = Message::from_bytes(&without_stream(&bytes, &table)).unwrap();
            assert!(message.is_partial(), "{name}: {table:?} removed");
            assert_eq!(message.subject(), original.subject());
            assert_eq!(file_names(&message), file_names(&original));
            // The other recipients are still read, in order.
            let left = message.recipients();
            assert!(left.len() + 1 >= original.recipients().len(), "{name}");
            let mut rest = original.recipients().iter();
            assert!(left.iter().all(|r| rest.any(|o| o == r)), "{name}");
            checked += 1;
        }
    }
    assert!(checked > 1);
}

#[test]
fn damaged_attachment_is_left_out() {
    let mut checked = 0;
    for name in SAMPLES {
        let bytes = sample(name);
        let original = Message::from_bytes(&bytes).unwrap();
        let storages = root_storages(&bytes, "__attach_version1.0_#");
        for (i, storage) in storages.iter().enumerate() {
            let table = storage.join("__properties_version1.0");
            let message = Message::from_bytes(&without_stream(&bytes, &table)).unwrap();
            assert!(message.is_partial(), "{name}: {table:?} removed");
            assert_eq!(message.subject(), original.subject());
            assert_eq!(message.recipients(), original.recipients());
            let mut expected = file_names(&original);
            expected.remove(i);
            assert_eq!(file_names(&message), expected, "{name}");
            checked += 1;
        }
    }
    assert!(checked > 1);
}

/// The first Embedded message storage in `bytes`.
fn embedded_storage(bytes: &[u8]) -> PathBuf {
    let file = CompoundFile::open(Cursor::new(bytes)).unwrap();
    file.walk()
        .find(|entry| entry.is_storage() && entry.name() == "__substg1.0_3701000D")
        .unwrap()
        .path()
        .to_path_buf()
}

fn streams_in(bytes: &[u8], storage: &Path) -> Vec<PathBuf> {
    let file = CompoundFile::open(Cursor::new(bytes)).unwrap();
    file.read_storage(storage)
        .unwrap()
        .filter(|entry| entry.is_stream())
        .map(|entry| entry.path().to_path_buf())
        .collect()
}

fn first_embedded(message: &Message) -> &Attachment {
    message
        .attachments()
        .iter()
        .find(|attachment| attachment.is_embedded_message())
        .unwrap()
}

#[test]
fn damaged_embedded_message_is_partial_too() {
    let bytes = sample("testMSG_att_msg.msg");
    let subject = streams_in(&bytes, &embedded_storage(&bytes))
        .into_iter()
        .find(|path| path.to_string_lossy().contains("__substg1.0_0037"))
        .unwrap();
    let message = Message::from_bytes(&without_stream(&bytes, &subject)).unwrap();
    // What is missing is found when the Embedded message itself is read.
    assert!(!message.is_partial());
    let inner = first_embedded(&message)
        .embedded_message()
        .unwrap()
        .unwrap();
    assert!(inner.is_partial());
    assert_eq!(inner.subject(), None);
}

#[test]
fn embedded_message_past_depth_limit_is_left_out() {
    let bytes = sample("testMSG_att_msg.msg");
    let read = |depth| {
        let limits = Limits {
            max_embedded_depth: depth,
            ..Limits::DEFAULT
        };
        Message::from_bytes_with_limits(&bytes, limits).unwrap()
    };

    let message = read(0);
    assert!(message.is_partial());
    assert!(first_embedded(&message).data().is_none());
    assert!(first_embedded(&message).embedded_message().is_none());

    let message = read(1);
    assert!(!message.is_partial());
    let inner = first_embedded(&message)
        .embedded_message()
        .unwrap()
        .unwrap();
    assert!(inner.subject().is_some());
}

/// Writes a Message into `storage` that holds the next one down as an
/// Embedded message, `level` times; the innermost has the subject "bottom".
fn write_nested(
    file: &mut CompoundFile<Cursor<Vec<u8>>>,
    storage: &str,
    header_len: usize,
    level: usize,
) {
    let subject = match level {
        0 => "bottom".to_owned(),
        _ => format!("level {level}"),
    };
    let mut table = vec![0; header_len];
    table.extend(0x0037_001Fu32.to_le_bytes());
    table.extend([6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let text: Vec<u8> = subject.encode_utf16().flat_map(u16::to_le_bytes).collect();
    write(file, &format!("{storage}/__properties_version1.0"), &table);
    write(file, &format!("{storage}/__substg1.0_0037001F"), &text);
    if level == 0 {
        return;
    }
    let attachment = format!("{storage}/__attach_version1.0_#00000000");
    file.create_storage(&attachment).unwrap();
    // PidTagAttachMethod: an Embedded message.
    let mut table = vec![0; 8];
    table.extend(0x3705_0003u32.to_le_bytes());
    table.extend([6, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0]);
    write(
        file,
        &format!("{attachment}/__properties_version1.0"),
        &table,
    );
    let embedded = format!("{attachment}/__substg1.0_3701000D");
    file.create_storage(&embedded).unwrap();
    write_nested(file, &embedded, 24, level - 1);
}

fn write(file: &mut CompoundFile<Cursor<Vec<u8>>>, path: &str, data: &[u8]) {
    file.create_stream(path).unwrap().write_all(data).unwrap();
}

/// Opens the first Embedded message of each Message in turn, starting from
/// `bytes`, until there is none. Returns how many were opened, and the
/// subject of the last and whether it was a Partial read.
fn descend(bytes: &[u8], limits: Limits) -> (usize, Option<String>, bool) {
    let mut message = Message::from_bytes_with_limits(bytes, limits).unwrap();
    let mut opened = 0;
    while let Some(next) = message
        .attachments()
        .first()
        .and_then(Attachment::embedded_message)
    {
        message = next.unwrap();
        opened += 1;
    }
    let subject = message.subject().map(str::to_owned);
    (opened, subject, message.is_partial())
}

#[test]
fn nesting_stops_at_depth_limit() {
    let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    write_nested(&mut file, "", 32, 5);
    file.flush().unwrap();
    let bytes = file.into_inner().into_inner();
    let limits = |depth| Limits {
        max_embedded_depth: depth,
        ..Limits::DEFAULT
    };
    assert_eq!(
        descend(&bytes, limits(5)),
        (5, Some("bottom".into()), false)
    );
    assert_eq!(
        descend(&bytes, limits(3)),
        (3, Some("level 2".into()), true)
    );
    assert_eq!(
        descend(&bytes, limits(0)),
        (0, Some("level 5".into()), true)
    );
}

#[test]
fn attachments_and_recipients_past_limits_are_left_out() {
    let bytes = sample("testMSG_att_doc.msg");
    let original = Message::from_bytes(&bytes).unwrap();
    let limits = Limits {
        max_attachments: 1,
        max_recipients: 1,
        ..Limits::DEFAULT
    };
    let message = Message::from_bytes_with_limits(&bytes, limits).unwrap();
    assert!(original.attachments().len() > 1 || original.recipients().len() > 1);
    assert!(message.is_partial());
    assert_eq!(file_names(&message), file_names(&original)[..1]);
    assert_eq!(message.recipients(), &original.recipients()[..1]);
}
