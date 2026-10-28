//! A Compressed RTF writer for tests and fuzzing ([MS-OXRTFCP] 2.3), kept
//! apart from the crate, which only ever reads Compressed RTF.
//!
//! The crate's unit tests, the integration tests and the fuzz targets all
//! include this file with `#[path]`, so it depends on nothing but `std`.

#![allow(dead_code)]

const DICTIONARY_LEN: usize = 4096;
const MAX_MATCH_LEN: usize = 17;
const PRELOADED: &[u8; 207] = b"{\\rtf1\\ansi\\mac\\deff0\\deftab720{\\fonttbl;}\
{\\f0\\fnil \\froman \\fswiss \\fmodern \\fscript \\fdecor MS Sans SerifSymbolArialTimes New \
RomanCourier{\\colortbl\\red0\\green0\\blue0\r\n\\par \\pard\\plain\\f0\\fs20\\b\\i\\u\\tab\\tx";

/// Compresses `raw` as `LZFu`, following the specification's pseudocode
/// (2.3.3.2), so the output matches its examples byte for byte.
pub fn compress(raw: &[u8]) -> Vec<u8> {
    let mut dictionary = Dictionary::new();
    let mut contents = Vec::new();
    let mut cursor = 0;
    loop {
        let mut control = 0u8;
        let mut tokens = Vec::new();
        let mut finished = false;
        for bit in 0..8 {
            if cursor == raw.len() {
                let end = (dictionary.write as u16) << 4;
                tokens.extend_from_slice(&end.to_be_bytes());
                control |= 1 << bit;
                finished = true;
                break;
            }
            let (offset, len) = dictionary.longest_match(&raw[cursor..]);
            if len < 2 {
                tokens.push(raw[cursor]);
                cursor += 1;
            } else {
                let reference = ((offset as u16) << 4) | (len as u16 - 2);
                tokens.extend_from_slice(&reference.to_be_bytes());
                control |= 1 << bit;
                cursor += len;
            }
        }
        contents.push(control);
        contents.extend_from_slice(&tokens);
        if finished {
            return with_header(&contents, raw.len() as u32);
        }
    }
}

/// Puts a correct `LZFu` header on compressed `contents`, giving `raw_size`
/// as their uncompressed size.
pub fn with_header(contents: &[u8], raw_size: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(16 + contents.len());
    data.extend_from_slice(&(contents.len() as u32 + 12).to_le_bytes());
    data.extend_from_slice(&raw_size.to_le_bytes());
    data.extend_from_slice(b"LZFu");
    data.extend_from_slice(&crc(contents).to_le_bytes());
    data.extend_from_slice(contents);
    data
}

/// Stores `raw` without compressing it, as `MELA` (2.3.3.1).
pub fn uncompressed(raw: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(16 + raw.len());
    data.extend_from_slice(&(raw.len() as u32 + 12).to_le_bytes());
    data.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    data.extend_from_slice(b"MELA");
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(raw);
    data
}

/// The CRC of 2.1.3.2, computed bit by bit rather than from a table.
pub fn crc(bytes: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

struct Dictionary {
    bytes: [u8; DICTIONARY_LEN],
    write: usize,
    end: usize,
}

impl Dictionary {
    fn new() -> Self {
        let mut bytes = [0; DICTIONARY_LEN];
        bytes[..PRELOADED.len()].copy_from_slice(PRELOADED);
        Dictionary {
            bytes,
            write: PRELOADED.len(),
            end: PRELOADED.len(),
        }
    }

    fn add(&mut self, byte: u8) {
        self.bytes[self.write] = byte;
        if self.end < DICTIONARY_LEN {
            self.end += 1;
        }
        self.write = (self.write + 1) % DICTIONARY_LEN;
    }

    /// FindLongestMatch of 2.3.3.2.1: the offset and length of the longest
    /// match for the start of `input`, adding the matched bytes (or the
    /// first byte, if nothing matched) to the dictionary.
    fn longest_match(&mut self, input: &[u8]) -> (usize, usize) {
        let final_offset = self.write % DICTIONARY_LEN;
        let mut offset = if self.end == DICTIONARY_LEN {
            (self.write + 1) % DICTIONARY_LEN
        } else {
            0
        };
        let (mut best_offset, mut best_len) = (0, 0);
        loop {
            // TryMatch
            let max_len = MAX_MATCH_LEN.min(input.len());
            let mut len = 0;
            let mut at = offset;
            while len < max_len && self.bytes[at] == input[len] {
                len += 1;
                if len > best_len {
                    self.add(input[len - 1]);
                }
                at = (at + 1) % DICTIONARY_LEN;
            }
            if len > best_len {
                best_offset = offset;
                best_len = len;
            }
            offset = (offset + 1) % DICTIONARY_LEN;
            if offset == final_offset || best_len == MAX_MATCH_LEN {
                break;
            }
        }
        if best_len == 0 {
            self.add(input[0]);
        }
        (best_offset, best_len)
    }
}
