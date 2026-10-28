//! Truncated, corrupted and incomplete copies of a sample must give a Partial
//! read or an Unreadable file, never a panic.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use cfb::CompoundFile;
use outlook_msg::{Error, Limits, Message, PropValue};

fn sample_bytes() -> Vec<u8> {
    let path = format!("{}/tests/samples/testMSG.msg", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(path).unwrap()
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
    let original = sample_bytes();
    let mut rng = Rng(0x5EED_F00D_0A11_C0DE);
    let small_limits = Limits {
        max_stream_len: 4096,
        max_total_read: 16 * 1024,
        ..Limits::DEFAULT
    };
    let (mut partial, mut unreadable) = (0, 0);
    for round in 0..3000 {
        let mut bytes = original.clone();
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
                let _ = (message.subject(), message.sender());
            }
            Err(_) => unreadable += 1,
        }
    }
    // Make sure the mutations reach both outcomes.
    assert!(partial > 0 && unreadable > 0, "{partial} {unreadable}");
}
