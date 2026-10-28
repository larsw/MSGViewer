//! MAPI property storage inside a compound file ([MS-OXMSG] 2.4).
//!
//! Each storage (the Message itself, a recipient, an attachment) has a
//! `__properties_version1.0` stream of fixed-length entries. Fixed-size values
//! sit in the entry; variable-length values live in sibling
//! `__substg1.0_IIIITTTT` streams named after the property tag.

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::Path;
use std::time::{Duration, SystemTime};

use encoding_rs::Encoding;

use crate::reader::Reader;

pub(crate) const PROPERTIES_STREAM: &str = "__properties_version1.0";

/// Header length of the properties stream, which depends on the storage kind.
#[derive(Clone, Copy, Debug)]
pub(crate) enum StorageKind {
    TopLevelMessage,
    #[allow(dead_code)]
    EmbeddedMessage,
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

// Codepage property IDs ([MS-OXPROPS]), in order of preference for 8-bit
// strings: PidTagMessageCodepage names the codepage of the Message's 8-bit
// string Properties; PidTagInternetCodepage that of its Body.
const PID_MESSAGE_CODEPAGE: u16 = 0x3FFD;
const PID_INTERNET_CODEPAGE: u16 = 0x3FDE;

/// Properties kept as bytes even when stored as 8-bit strings, since their
/// charset may not be the Message codepage: an HTML Body (`PidTagHtml`)
/// declares its own.
const KEEP_AS_BYTES: [u16; 1] = [0x1013];

/// The encoding for 8-bit strings when no codepage is given.
pub(crate) const DEFAULT_ENCODING: &Encoding = encoding_rs::WINDOWS_1252;

/// Seconds from the FILETIME epoch (1601-01-01) to the Unix epoch.
const FILETIME_UNIX_OFFSET: u64 = 11_644_473_600;

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
#[derive(Clone, Debug)]
pub struct Properties {
    values: HashMap<u16, PropValue>,
    /// The encoding 8-bit strings were decoded with.
    encoding: &'static Encoding,
}

impl Default for Properties {
    fn default() -> Self {
        Properties {
            values: HashMap::new(),
            encoding: DEFAULT_ENCODING,
        }
    }
}

/// One entry of a properties stream.
struct Entry {
    tag: u32,
    value: [u8; 8],
}

impl Entry {
    fn id(&self) -> u16 {
        (self.tag >> 16) as u16
    }

    fn ty(&self) -> u16 {
        self.tag as u16
    }
}

impl Properties {
    /// Reads the properties of `storage`.
    ///
    /// 8-bit strings are decoded with the codepage the storage itself names,
    /// or else with `inherited`: a recipient or attachment uses the codepage
    /// of the Message it belongs to.
    ///
    /// A Property whose value can't be read is skipped and recorded as a
    /// Problem. Returns `None`, also with a Problem recorded, when the
    /// storage's property table can't be read at all.
    pub(crate) fn read<F: Read + Seek>(
        reader: &mut Reader<F>,
        storage: &Path,
        kind: StorageKind,
        inherited: &'static Encoding,
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
        let entries: Vec<Entry> = entries
            .chunks_exact(ENTRY_LEN)
            .filter_map(|entry| {
                Some(Entry {
                    tag: u32::from_le_bytes(entry.get(0..4)?.try_into().ok()?),
                    value: entry.get(8..16)?.try_into().ok()?,
                })
            })
            .collect();

        // Fixed-size values come first, since the codepage among them is
        // needed to decode the 8-bit strings.
        let mut values = HashMap::new();
        for entry in &entries {
            if let Some(value) = fixed_value(entry.ty(), entry.value) {
                values.insert(entry.id(), value);
            }
        }
        let encoding = string8_encoding(&values).unwrap_or(inherited);

        for entry in &entries {
            let ty = entry.ty();
            if matches!(ty, PT_STRING8 | PT_UNICODE | PT_BINARY) {
                let stream = storage.join(format!("__substg1.0_{:08X}", entry.tag));
                let ty = if ty == PT_STRING8 && KEEP_AS_BYTES.contains(&entry.id()) {
                    PT_BINARY
                } else {
                    ty
                };
                if let Some(data) = reader.read_stream(&stream) {
                    values.insert(entry.id(), decode_variable(data, ty, encoding));
                }
            }
        }
        Some(Properties { values, encoding })
    }

    /// The encoding 8-bit strings in these properties were decoded with.
    pub(crate) fn encoding(&self) -> &'static Encoding {
        self.encoding
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

    pub fn int(&self, id: u16) -> Option<i64> {
        match self.get(id)? {
            PropValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// A time property, or `None` if it is missing, zero or out of range.
    pub fn time(&self, id: u16) -> Option<SystemTime> {
        match self.get(id)? {
            PropValue::Time(filetime) => filetime_to_system_time(*filetime),
            _ => None,
        }
    }
}

fn fixed_value(ty: u16, raw: [u8; 8]) -> Option<PropValue> {
    let [b0, b1, b2, b3, ..] = raw;
    match ty {
        PT_SHORT => Some(PropValue::Int(i16::from_le_bytes([b0, b1]).into())),
        PT_LONG => Some(PropValue::Int(i32::from_le_bytes([b0, b1, b2, b3]).into())),
        PT_BOOLEAN => Some(PropValue::Bool(b0 != 0)),
        PT_LONGLONG => Some(PropValue::Int(i64::from_le_bytes(raw))),
        PT_SYSTIME => Some(PropValue::Time(u64::from_le_bytes(raw))),
        _ => None,
    }
}

/// The encoding named by the Message codepage, falling back to the Internet
/// codepage; `None` if neither names an encoding usable for 8-bit strings.
fn string8_encoding(values: &HashMap<u16, PropValue>) -> Option<&'static Encoding> {
    [PID_MESSAGE_CODEPAGE, PID_INTERNET_CODEPAGE]
        .into_iter()
        .filter_map(|id| match values.get(&id)? {
            PropValue::Int(codepage) => u16::try_from(*codepage).ok(),
            _ => None,
        })
        .find_map(codepage_encoding)
}

/// Maps a Windows codepage number to an encoding for 8-bit strings. UTF-16
/// codepages are rejected, since 8-bit strings cannot be in UTF-16.
pub(crate) fn codepage_encoding(codepage: u16) -> Option<&'static Encoding> {
    codepage::to_encoding_no_replacement(codepage)
        .filter(|&encoding| encoding != encoding_rs::UTF_16LE && encoding != encoding_rs::UTF_16BE)
}

fn filetime_to_system_time(filetime: u64) -> Option<SystemTime> {
    if filetime == 0 {
        return None;
    }
    let since_1601 = Duration::new(filetime / 10_000_000, (filetime % 10_000_000) as u32 * 100);
    SystemTime::UNIX_EPOCH
        .checked_sub(Duration::from_secs(FILETIME_UNIX_OFFSET))?
        .checked_add(since_1601)
}

fn decode_variable(data: Vec<u8>, ty: u16, encoding: &'static Encoding) -> PropValue {
    match ty {
        PT_UNICODE => PropValue::String(decode_unicode(&data)),
        PT_STRING8 => PropValue::String(decode_string8(&data, encoding)),
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

fn decode_string8(data: &[u8], encoding: &'static Encoding) -> String {
    let mut s = encoding.decode_without_bom_handling(data).0.into_owned();
    s.truncate(s.trim_end_matches('\0').len());
    s
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
        assert_eq!(decode_string8(b"caf\xe9\0", DEFAULT_ENCODING), "café");
    }

    #[test]
    fn string8_decodes_multibyte_codepage() {
        let big5 = codepage_encoding(950).unwrap();
        assert_eq!(decode_string8(b"\xae\xe6\xa6\xa1\0", big5), "格式");
    }

    #[test]
    fn maps_windows_codepages() {
        assert_eq!(codepage_encoding(1252), Some(encoding_rs::WINDOWS_1252));
        assert_eq!(codepage_encoding(1251), Some(encoding_rs::WINDOWS_1251));
        assert_eq!(codepage_encoding(932), Some(encoding_rs::SHIFT_JIS));
        assert_eq!(codepage_encoding(65001), Some(encoding_rs::UTF_8));
        // ISO-8859-1 is unified with Windows-1252, as on the web.
        assert_eq!(codepage_encoding(28591), Some(encoding_rs::WINDOWS_1252));
    }

    #[test]
    fn rejects_codepages_unusable_for_8_bit_strings() {
        assert_eq!(codepage_encoding(1200), None);
        assert_eq!(codepage_encoding(1201), None);
        assert_eq!(codepage_encoding(0), None);
        assert_eq!(codepage_encoding(20127), None);
    }

    #[test]
    fn message_codepage_wins_over_internet_codepage() {
        let values = HashMap::from([
            (PID_MESSAGE_CODEPAGE, PropValue::Int(1251)),
            (PID_INTERNET_CODEPAGE, PropValue::Int(1252)),
        ]);
        assert_eq!(string8_encoding(&values), Some(encoding_rs::WINDOWS_1251));
    }

    #[test]
    fn unusable_message_codepage_falls_back_to_internet_codepage() {
        let values = HashMap::from([
            (PID_MESSAGE_CODEPAGE, PropValue::Int(1200)),
            (PID_INTERNET_CODEPAGE, PropValue::Int(950)),
        ]);
        assert_eq!(string8_encoding(&values), Some(encoding_rs::BIG5));
    }

    #[test]
    fn converts_filetime() {
        let unix = Duration::from_secs(1_700_000_000);
        let filetime = (unix.as_secs() + FILETIME_UNIX_OFFSET) * 10_000_000 + 5;
        assert_eq!(
            filetime_to_system_time(filetime),
            Some(SystemTime::UNIX_EPOCH + unix + Duration::from_nanos(500))
        );
        assert_eq!(filetime_to_system_time(0), None);
        assert!(filetime_to_system_time(u64::MAX).is_some());
    }
}
