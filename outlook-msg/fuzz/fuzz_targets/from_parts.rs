//! Builds a well-formed compound file from arbitrary Message parts and reads
//! it. Mutating raw bytes mostly breaks the compound file itself; this
//! reaches the property tables and values behind it.

#![no_main]

use std::io::{Cursor, Write};

use arbitrary::Arbitrary;
use cfb::CompoundFile;
use libfuzzer_sys::fuzz_target;
use outlook_msg::{Limits, Message};

#[derive(Arbitrary, Debug)]
struct Entry {
    tag: u32,
    flags: u32,
    value: [u8; 8],
    /// The `__substg1.0_` stream for the tag, if there is one.
    stream: Option<Vec<u8>>,
}

#[derive(Arbitrary, Debug)]
struct Storage {
    /// Header bytes before the entries.
    header: Vec<u8>,
    entries: Vec<Entry>,
    /// Bytes after the last whole entry.
    trailer: Vec<u8>,
}

#[derive(Arbitrary, Debug)]
enum Child {
    Recipient(Storage),
    Attachment(Storage),
    /// An Embedded message in an attachment storage.
    Embedded(Storage),
}

#[derive(Arbitrary, Debug)]
struct Parts {
    root: Storage,
    children: Vec<Child>,
    small_limits: bool,
}

fn write_storage(file: &mut CompoundFile<Cursor<Vec<u8>>>, path: &str, storage: &Storage) {
    let mut table = storage.header.clone();
    for entry in &storage.entries {
        table.extend(entry.tag.to_le_bytes());
        table.extend(entry.flags.to_le_bytes());
        table.extend(entry.value);
        if let Some(data) = &entry.stream {
            let name = format!("{path}/__substg1.0_{:08X}", entry.tag);
            if let Ok(mut stream) = file.create_stream(name) {
                let _ = stream.write_all(data);
            }
        }
    }
    table.extend(&storage.trailer);
    if let Ok(mut stream) = file.create_stream(format!("{path}/__properties_version1.0")) {
        let _ = stream.write_all(&table);
    }
}

fn build(parts: &Parts) -> Option<Vec<u8>> {
    let mut file = CompoundFile::create(Cursor::new(Vec::new())).ok()?;
    write_storage(&mut file, "", &parts.root);
    for (i, child) in parts.children.iter().enumerate() {
        let (path, storage) = match child {
            Child::Recipient(s) => (format!("/__recip_version1.0_#{i:08X}"), s),
            Child::Attachment(s) => (format!("/__attach_version1.0_#{i:08X}"), s),
            Child::Embedded(s) => {
                let attachment = format!("/__attach_version1.0_#{i:08X}");
                file.create_storage(&attachment).ok()?;
                (format!("{attachment}/__substg1.0_3701000D"), s)
            }
        };
        file.create_storage(&path).ok()?;
        write_storage(&mut file, &path, storage);
    }
    file.flush().ok()?;
    Some(file.into_inner().into_inner())
}

fuzz_target!(|parts: Parts| {
    let Some(bytes) = build(&parts) else { return };
    let limits = if parts.small_limits {
        Limits {
            max_stream_len: 256,
            max_total_read: 4096,
            max_embedded_depth: 2,
            max_recipients: 2,
            max_attachments: 2,
        }
    } else {
        Limits::DEFAULT
    };
    if let Ok(message) = Message::from_bytes_with_limits(&bytes, limits) {
        let _ = (message.subject(), message.sender(), message.message_class());
    }
});
