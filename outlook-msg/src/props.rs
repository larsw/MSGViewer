//! MAPI property storage inside a compound file ([MS-OXMSG] 2.4).
//!
//! Each storage (the Message itself, a recipient, an attachment) has a
//! `__properties_version1.0` stream of fixed-length entries. Fixed-size values
//! sit in the entry; variable-length values live in sibling
//! `__substg1.0_IIIITTTT` streams named after the property tag.

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::Path;

use crate::reader::Reader;

pub(crate) const PROPERTIES_STREAM: &str = "__properties_version1.0";

/// Header length of the properties stream, which depends on the storage kind.
#[derive(Clone, Copy, Debug)]
pub(crate) enum StorageKind {
    TopLevelMessage,
    #[allow(dead_code)]
    EmbeddedMessage,
    #[allow(dead_code)]
    RecipientOrAttachment,
}

impl StorageKind {
    fn header_len(self) -> usize {
        match self {
            StorageKind::TopLevelMessage => 32,
            StorageKind::EmbeddedMessage => 24,
            StorageKind::RecipientOrAttachment => 8,
        }
    }
}

const ENTRY_LEN: usize = 16;

// Property types ([MS-OXCDATA] 2.11.1).
const PT_SHORT: u16 = 0x0002;
const PT_LONG: u16 = 0x0003;
const PT_BOOLEAN: u16 = 0x000B;
const PT_LONGLONG: u16 = 0x0014;
const PT_SYSTIME: u16 = 0x0040;
const PT_STRING8: u16 = 0x001E;
const PT_UNICODE: u16 = 0x001F;
const PT_BINARY: u16 = 0x0102;

/// A decoded property value.
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Int(i64),
    Bool(bool),
    /// A FILETIME: 100-nanosecond intervals since 1601-01-01 UTC.
    Time(u64),
    String(String),
    Binary(Vec<u8>),
}

/// The properties of one storage, keyed by property ID.
#[derive(Clone, Debug, Default)]
pub struct Properties {
    values: HashMap<u16, PropValue>,
}

impl Properties {
    /// Reads the Properties of one storage. A Property whose value can't be
    /// read is skipped and recorded as a Problem. Returns `None`, also with a
    /// Problem recorded, when the storage's property table can't be read at
    /// all.
    pub(crate) fn read<F: Read + Seek>(
        reader: &mut Reader<F>,
        storage: &Path,
        kind: StorageKind,
    ) -> Option<Self> {
        let table_path = storage.join(PROPERTIES_STREAM);
        let mut table = Vec::new();
        if let Err(error) = reader.read_stream_into(&table_path, &mut table) {
            reader.problem(&table_path, error);
            // Keep whatever entries were read before the damage.
            if table.len() < kind.header_len() + ENTRY_LEN {
                return None;
            }
        }
        let Some(entries) = table.get(kind.header_len()..) else {
            reader.problem(&table_path, "property table is shorter than its header");
            return Some(Properties::default());
        };
        if entries.len() % ENTRY_LEN != 0 {
            reader.problem(&table_path, "property table ends in a partial entry");
        }

        let mut values = HashMap::new();
        for entry in entries.chunks_exact(ENTRY_LEN) {
            let tag = u32::from_le_bytes(entry[0..4].try_into().unwrap());
            let (id, ty) = ((tag >> 16) as u16, tag as u16);
            let raw: [u8; 8] = entry[8..16].try_into().unwrap();
            let value = match ty {
                PT_SHORT => Some(PropValue::Int(i16::from_le_bytes([raw[0], raw[1]]).into())),
                PT_LONG => Some(PropValue::Int(
                    i32::from_le_bytes(raw[0..4].try_into().unwrap()).into(),
                )),
                PT_BOOLEAN => Some(PropValue::Bool(raw[0] != 0)),
                PT_LONGLONG => Some(PropValue::Int(i64::from_le_bytes(raw))),
                PT_SYSTIME => Some(PropValue::Time(u64::from_le_bytes(raw))),
                PT_STRING8 | PT_UNICODE | PT_BINARY => {
                    let stream = storage.join(format!("__substg1.0_{tag:08X}"));
                    reader
                        .read_stream(&stream)
                        .map(|data| decode_variable(data, ty))
                }
                _ => None,
            };
            if let Some(value) = value {
                values.insert(id, value);
            }
        }
        Some(Properties { values })
    }

    pub fn get(&self, id: u16) -> Option<&PropValue> {
        self.values.get(&id)
    }

    pub fn string(&self, id: u16) -> Option<&str> {
        match self.get(id)? {
            PropValue::String(s) => Some(s),
            _ => None,
        }
    }
}

fn decode_variable(data: Vec<u8>, ty: u16) -> PropValue {
    match ty {
        PT_UNICODE => PropValue::String(decode_unicode(&data)),
        PT_STRING8 => PropValue::String(decode_string8(&data)),
        _ => PropValue::Binary(data),
    }
}

fn decode_unicode(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut s = String::from_utf16_lossy(&units);
    s.truncate(s.trim_end_matches('\0').len());
    s
}

/// Decodes an 8-bit string. Assumes Windows-1252 until codepage handling lands.
fn decode_string8(data: &[u8]) -> String {
    let data = data.strip_suffix(&[0]).unwrap_or(data);
    encoding_rs::WINDOWS_1252.decode(data).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_strips_terminator() {
        assert_eq!(decode_unicode(b"h\0i\0\0\0"), "hi");
    }

    #[test]
    fn string8_decodes_windows_1252() {
        assert_eq!(decode_string8(b"caf\xe9\0"), "café");
    }
}
