//! Builds a Message whose Body is arbitrary Compressed RTF, and reads its
//! Body. The RTF is compressed here, with a correct header and CRC, so the
//! fuzzer reaches the decompressor's references and the RTF reader behind
//! it rather than stopping at the CRC.

#![no_main]

#[path = "../../tests/support/lzfu.rs"]
mod lzfu;

use std::io::{Cursor, Write};

use arbitrary::Arbitrary;
use cfb::CompoundFile;
use libfuzzer_sys::fuzz_target;
use outlook_msg::{Limits, Message};

#[derive(Arbitrary, Debug)]
enum Packing {
    /// Compressed as `LZFu`.
    Compressed,
    /// Stored as `MELA`.
    Uncompressed,
    /// The property holds the bytes as they are, header and all.
    Raw,
}

#[derive(Arbitrary, Debug)]
enum Heading {
    Html,
    Text,
    Rtf,
    None,
}

#[derive(Arbitrary, Debug)]
struct Parts {
    heading: Heading,
    rtf: Vec<u8>,
    packing: Packing,
    plain: Option<String>,
    html: Option<Vec<u8>>,
    in_sync: Option<bool>,
    small_limits: bool,
}

fn build(parts: &Parts) -> Option<Vec<u8>> {
    let heading: &[u8] = match parts.heading {
        Heading::Html => b"{\\rtf1\\ansi\\ansicpg1252\\fromhtml1 \\deff0",
        Heading::Text => b"{\\rtf1\\ansi\\ansicpg1252\\fromtext \\deff0",
        Heading::Rtf => b"{\\rtf1\\ansi\\ansicpg1252\\deff0",
        Heading::None => b"",
    };
    let rtf = [heading, &parts.rtf].concat();
    let rtf = match parts.packing {
        Packing::Compressed => lzfu::compress(&rtf),
        Packing::Uncompressed => lzfu::uncompressed(&rtf),
        Packing::Raw => rtf,
    };

    let mut file = CompoundFile::create(Cursor::new(Vec::new())).ok()?;
    let mut table = vec![0; 32];
    let mut property = |tag: u32, value: [u8; 8], data: Option<&[u8]>| -> Option<()> {
        table.extend(tag.to_le_bytes());
        table.extend(6u32.to_le_bytes());
        table.extend(value);
        if let Some(data) = data {
            let mut stream = file.create_stream(format!("/__substg1.0_{tag:08X}")).ok()?;
            stream.write_all(data).ok()?;
        }
        Some(())
    };
    let size = |data: &[u8]| (data.len() as u64).to_le_bytes();
    property(0x10090102, size(&rtf), Some(&rtf))?;
    if let Some(plain) = &parts.plain {
        let data: Vec<u8> = plain.encode_utf16().flat_map(u16::to_le_bytes).collect();
        property(0x1000001F, size(&data), Some(&data))?;
    }
    if let Some(html) = &parts.html {
        property(0x10130102, size(html), Some(html))?;
    }
    if let Some(in_sync) = parts.in_sync {
        property(0x0E1F000B, u64::from(in_sync).to_le_bytes(), None)?;
    }
    file.create_stream("/__properties_version1.0")
        .ok()?
        .write_all(&table)
        .ok()?;
    file.flush().ok()?;
    Some(file.into_inner().into_inner())
}

fuzz_target!(|parts: Parts| {
    let Some(bytes) = build(&parts) else { return };
    let limits = if parts.small_limits {
        Limits {
            max_stream_len: 4096,
            ..Limits::DEFAULT
        }
    } else {
        Limits::DEFAULT
    };
    if let Ok(message) = Message::from_bytes_with_limits(&bytes, limits) {
        let _ = message.body();
        for problem in message.problems() {
            let _ = problem.to_string();
        }
    }
});
