//! Compressed RTF, as `PidTagRtfCompressed` stores it ([MS-OXRTFCP]).
//!
//! A 16-byte header gives the compressed and uncompressed sizes, the
//! compression type and a CRC. `LZFu` data is an LZ77 variant whose 4096-byte
//! dictionary starts out holding common RTF; `MELA` data is not compressed
//! at all.
//!
//! [MS-OXRTFCP]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxrtfcp/

use std::fmt;

const HEADER_LEN: usize = 16;
/// COMPSIZE counts the rest of the header after itself, and the contents.
const COMPSIZE_HEADER_PART: u32 = 12;
/// COMPTYPE of compressed contents, "LZFu".
const COMPRESSED: u32 = 0x7546_5A4C;
/// COMPTYPE of uncompressed contents, "MELA".
const UNCOMPRESSED: u32 = 0x414C_454D;

pub(crate) const DICTIONARY_LEN: usize = 4096;
/// What the dictionary holds before decompression starts
/// ([MS-OXRTFCP] 2.1.2.1).
pub(crate) const PRELOADED: &[u8; 207] = b"{\\rtf1\\ansi\\mac\\deff0\\deftab720{\\fonttbl;}\
{\\f0\\fnil \\froman \\fswiss \\fmodern \\fscript \\fdecor MS Sans SerifSymbolArialTimes New \
RomanCourier{\\colortbl\\red0\\green0\\blue0\r\n\\par \\pard\\plain\\f0\\fs20\\b\\i\\u\\tab\\tx";

/// The CRC lookup table ([MS-OXRTFCP] 2.1.2.2.1): that of the usual CRC-32,
/// which the CRC here uses without its initial and final inversion.
const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 1 {
                (value >> 1) ^ 0xEDB8_8320
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

/// The CRC of `bytes` ([MS-OXRTFCP] 2.1.3.2).
pub(crate) fn crc(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0, |crc, &byte| {
        CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8)
    })
}

/// Why Compressed RTF couldn't be decompressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Damage {
    /// Shorter than its header.
    NoHeader,
    /// A compression type other than `LZFu` or `MELA`.
    UnknownType(u32),
    /// COMPSIZE counts more bytes than there are, or fewer than the header.
    BadCompressedSize { stated: u32, available: usize },
    /// The CRC of the contents isn't the one in the header.
    BadCrc { stated: u32, actual: u32 },
    /// The contents end before the reference that marks their end.
    Truncated,
    /// RAWSIZE, or the uncompressed contents, are over the size limit.
    TooLarge { size: u64, limit: u64 },
    /// The contents decompress to less than RAWSIZE.
    BadSize { stated: u32, actual: usize },
}

impl fmt::Display for Damage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Damage::NoHeader => write!(f, "Compressed RTF is shorter than its header"),
            Damage::UnknownType(ty) => {
                write!(
                    f,
                    "Compressed RTF has an unknown compression type {ty:#010x}"
                )
            }
            Damage::BadCompressedSize { stated, available } => write!(
                f,
                "Compressed RTF says it is {stated} bytes, but {available} follow its size"
            ),
            Damage::BadCrc { stated, actual } => write!(
                f,
                "Compressed RTF has CRC {actual:#010x}, not the {stated:#010x} its header gives"
            ),
            Damage::Truncated => write!(f, "Compressed RTF ends before its end marker"),
            Damage::TooLarge { size, limit } => write!(
                f,
                "RTF is {size} bytes uncompressed, more than the limit of {limit}"
            ),
            Damage::BadSize { stated, actual } => write!(
                f,
                "Compressed RTF decompresses to {actual} bytes, fewer than the {stated} its header gives"
            ),
        }
    }
}

/// Decompresses Compressed RTF ([MS-OXRTFCP] 2.2), refusing to produce
/// more than `max_len` bytes.
///
/// For `LZFu` data, the header's sizes and CRC must all be right, and the
/// output is cut to the size the header gives, which the contents must
/// fill. `MELA` data is taken
/// as it is, all of it whatever the header says, as the specification
/// recommends.
pub(crate) fn decompress(data: &[u8], max_len: u64) -> Result<Vec<u8>, Damage> {
    let field = |i: usize| {
        data.get(i * 4..i * 4 + 4)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or(Damage::NoHeader)
    };
    let (comp_size, raw_size, comp_type, stated_crc) = (field(0)?, field(1)?, field(2)?, field(3)?);
    let rest = &data[HEADER_LEN..];
    match comp_type {
        UNCOMPRESSED => {
            let size = rest.len() as u64;
            if size > max_len {
                return Err(Damage::TooLarge {
                    size,
                    limit: max_len,
                });
            }
            Ok(rest.to_vec())
        }
        COMPRESSED => {
            if u64::from(raw_size) > max_len {
                return Err(Damage::TooLarge {
                    size: raw_size.into(),
                    limit: max_len,
                });
            }
            let contents = comp_size
                .checked_sub(COMPSIZE_HEADER_PART)
                .and_then(|len| rest.get(..len as usize))
                .ok_or(Damage::BadCompressedSize {
                    stated: comp_size,
                    available: data.len() - 4,
                })?;
            let actual_crc = crc(contents);
            if actual_crc != stated_crc {
                return Err(Damage::BadCrc {
                    stated: stated_crc,
                    actual: actual_crc,
                });
            }
            decompress_lzfu(contents, raw_size)
        }
        other => Err(Damage::UnknownType(other)),
    }
}

/// Decompresses `LZFu` contents ([MS-OXRTFCP] 2.2.3.2), which must come to
/// at least `raw_size` bytes. Bytes past that are dropped: Outlook writes an
/// empty RTF as a single NUL with a RAWSIZE of 0 (2.3.3.2, step 8).
fn decompress_lzfu(contents: &[u8], raw_size: u32) -> Result<Vec<u8>, Damage> {
    let raw_len = raw_size as usize;
    let mut dictionary = [0; DICTIONARY_LEN];
    dictionary[..PRELOADED.len()].copy_from_slice(PRELOADED);
    let mut write = PRELOADED.len();
    // A reference of two bytes expands to at most 17, so a lying RAWSIZE
    // can't make this allocate much more than the contents justify.
    let mut output = Vec::with_capacity(raw_len.min(contents.len().saturating_mul(9)));
    let mut input = contents.iter().copied();
    loop {
        let control = input.next().ok_or(Damage::Truncated)?;
        for bit in 0..8 {
            if control & (1 << bit) == 0 {
                let literal = input.next().ok_or(Damage::Truncated)?;
                if output.len() < raw_len {
                    output.push(literal);
                }
                dictionary[write] = literal;
                write = (write + 1) % DICTIONARY_LEN;
                continue;
            }
            let (Some(high), Some(low)) = (input.next(), input.next()) else {
                return Err(Damage::Truncated);
            };
            let reference = u16::from_be_bytes([high, low]);
            let mut read = usize::from(reference >> 4);
            if read == write {
                return if output.len() == raw_len {
                    Ok(output)
                } else {
                    Err(Damage::BadSize {
                        stated: raw_size,
                        actual: output.len(),
                    })
                };
            }
            let len = usize::from(reference & 0xF) + 2;
            for _ in 0..len {
                let byte = dictionary[read];
                if output.len() < raw_len {
                    output.push(byte);
                }
                dictionary[write] = byte;
                read = (read + 1) % DICTIONARY_LEN;
                write = (write + 1) % DICTIONARY_LEN;
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/support/lzfu.rs"]
mod lzfu;

#[cfg(test)]
mod tests {
    use super::*;

    /// [MS-OXRTFCP] 3.1.1: Simple Compressed RTF.
    const EXAMPLE_1: &[u8] = &[
        0x2d, 0x00, 0x00, 0x00, 0x2b, 0x00, 0x00, 0x00, 0x4c, 0x5a, 0x46, 0x75, 0xf1, 0xc5, 0xc7,
        0xa7, 0x03, 0x00, 0x0a, 0x00, 0x72, 0x63, 0x70, 0x67, 0x31, 0x32, 0x35, 0x42, 0x32, 0x0a,
        0xf3, 0x20, 0x68, 0x65, 0x6c, 0x09, 0x00, 0x20, 0x62, 0x77, 0x05, 0xb0, 0x6c, 0x64, 0x7d,
        0x0a, 0x80, 0x0f, 0xa0,
    ];
    const EXAMPLE_1_RTF: &[u8] = b"{\\rtf1\\ansi\\ansicpg1252\\pard hello world}\r\n";

    /// [MS-OXRTFCP] 3.1.2: Reading a Token from the Dictionary that Crosses
    /// WritePosition.
    const EXAMPLE_2: &[u8] = &[
        0x1a, 0x00, 0x00, 0x00, 0x1c, 0x00, 0x00, 0x00, 0x4c, 0x5a, 0x46, 0x75, 0xe2, 0xd4, 0x4b,
        0x51, 0x41, 0x00, 0x04, 0x20, 0x57, 0x58, 0x59, 0x5a, 0x0d, 0x6e, 0x7d, 0x01, 0x0e, 0xb0,
    ];
    const EXAMPLE_2_RTF: &[u8] = b"{\\rtf1 WXYZWXYZWXYZWXYZWXYZ}";

    const LIMIT: u64 = 1 << 20;

    #[test]
    fn preloaded_dictionary_is_207_bytes() {
        assert_eq!(PRELOADED.len(), 207);
        assert!(PRELOADED.ends_with(b"\\fs20\\b\\i\\u\\tab\\tx"));
        assert_eq!(&PRELOADED[168..170], b"\r\n");
    }

    #[test]
    fn crc_table_matches_the_specification() {
        // [MS-OXRTFCP] 2.1.2.2.1, a few entries from each part.
        assert_eq!(CRC_TABLE[0], 0x0000_0000);
        assert_eq!(CRC_TABLE[1], 0x7707_3096);
        assert_eq!(CRC_TABLE[3], 0x9909_51ba);
        assert_eq!(CRC_TABLE[128], 0xedb8_8320);
        assert_eq!(CRC_TABLE[255], 0x2d02_ef8d);
    }

    #[test]
    fn computes_example_crc() {
        // [MS-OXRTFCP] 3.3.1: the first byte, then all of example 1.
        assert_eq!(crc(&[0x03]), 0x9909_51ba);
        assert_eq!(crc(&EXAMPLE_1[HEADER_LEN..]), 0xa7c7_c5f1);
        assert_eq!(crc(&EXAMPLE_2[HEADER_LEN..]), 0x514b_d4e2);
    }

    #[test]
    fn decompresses_simple_example() {
        assert_eq!(decompress(EXAMPLE_1, LIMIT).unwrap(), EXAMPLE_1_RTF);
    }

    #[test]
    fn decompresses_reference_crossing_write_position() {
        assert_eq!(decompress(EXAMPLE_2, LIMIT).unwrap(), EXAMPLE_2_RTF);
    }

    #[test]
    fn compressor_reproduces_the_examples() {
        // [MS-OXRTFCP] 3.2 compresses the same RTF as 3.1 decompresses.
        assert_eq!(lzfu::compress(EXAMPLE_1_RTF), EXAMPLE_1);
        assert_eq!(lzfu::compress(EXAMPLE_2_RTF), EXAMPLE_2);
    }

    #[test]
    fn round_trips_through_compressor() {
        let mut rtf =
            b"{\\rtf1\\ansi\\ansicpg1252\\fromhtml1 \\deff0{\\fonttbl{\\f0 Arial;}}".to_vec();
        for i in 0..2000u32 {
            // Enough to wrap the dictionary several times, with repeats
            // both near and far.
            rtf.extend(format!("{{\\*\\htmltag84 <p id={}>}}x\\par ", i % 37).bytes());
            rtf.extend((i * 7919 % 251) as u8..((i * 7919 % 251) as u8).saturating_add(3));
        }
        rtf.push(b'}');
        assert_eq!(decompress(&lzfu::compress(&rtf), LIMIT).unwrap(), rtf);
        assert_eq!(decompress(&lzfu::compress(b""), LIMIT).unwrap(), b"");
    }

    #[test]
    fn reads_uncompressed_contents_as_they_are() {
        let mela = lzfu::uncompressed(b"{\\rtf1 plain}");
        assert_eq!(decompress(&mela, LIMIT).unwrap(), b"{\\rtf1 plain}");
        // The CRC isn't checked, and RAWSIZE doesn't matter.
        let mut odd = mela.clone();
        odd[4] = 3;
        odd[12] = 0xFF;
        assert_eq!(decompress(&odd, LIMIT).unwrap(), b"{\\rtf1 plain}");
    }

    #[test]
    fn rejects_short_header_and_unknown_type() {
        for len in 0..HEADER_LEN {
            assert_eq!(decompress(&EXAMPLE_1[..len], LIMIT), Err(Damage::NoHeader));
        }
        let mut data = EXAMPLE_1.to_vec();
        data[8..12].copy_from_slice(b"LZFV");
        assert!(matches!(
            decompress(&data, LIMIT),
            Err(Damage::UnknownType(_))
        ));
    }

    #[test]
    fn rejects_bad_crc() {
        let mut data = EXAMPLE_1.to_vec();
        data[20] ^= 1;
        assert!(matches!(
            decompress(&data, LIMIT),
            Err(Damage::BadCrc {
                stated: 0xa7c7_c5f1,
                ..
            })
        ));
    }

    #[test]
    fn rejects_compressed_size_past_the_end() {
        let mut data = EXAMPLE_1.to_vec();
        data[0] += 1;
        assert!(matches!(
            decompress(&data, LIMIT),
            Err(Damage::BadCompressedSize { stated: 0x2e, .. })
        ));
        data[0..4].copy_from_slice(&11u32.to_le_bytes());
        assert!(matches!(
            decompress(&data, LIMIT),
            Err(Damage::BadCompressedSize { .. })
        ));
        // Bytes past COMPSIZE are ignored.
        let mut padded = EXAMPLE_1.to_vec();
        padded.extend_from_slice(&[0; 7]);
        assert_eq!(decompress(&padded, LIMIT).unwrap(), EXAMPLE_1_RTF);
    }

    /// Example 1 with its contents cut to `len` bytes, and its header
    /// fixed to match.
    fn example_1_cut(len: usize) -> Vec<u8> {
        let contents = &EXAMPLE_1[HEADER_LEN..HEADER_LEN + len];
        lzfu::with_header(contents, 0x2b)
    }

    #[test]
    fn rejects_contents_without_end_marker() {
        let full = EXAMPLE_1.len() - HEADER_LEN;
        for len in 0..full {
            assert!(
                matches!(
                    decompress(&example_1_cut(len), LIMIT),
                    Err(Damage::Truncated | Damage::BadSize { .. })
                ),
                "cut to {len}"
            );
        }
        assert!(decompress(&example_1_cut(full), LIMIT).is_ok());
    }

    #[test]
    fn rejects_output_shorter_than_raw_size() {
        let contents = &EXAMPLE_1[HEADER_LEN..];
        for raw_size in [0x2c, 1000] {
            assert_eq!(
                decompress(&lzfu::with_header(contents, raw_size), LIMIT),
                Err(Damage::BadSize {
                    stated: raw_size,
                    actual: 0x2b
                })
            );
        }
    }

    #[test]
    fn cuts_output_to_raw_size() {
        let contents = &EXAMPLE_1[HEADER_LEN..];
        for raw_size in [0, 1, 0x2a] {
            assert_eq!(
                decompress(&lzfu::with_header(contents, raw_size), LIMIT).unwrap(),
                &EXAMPLE_1_RTF[..raw_size as usize]
            );
        }
    }

    #[test]
    fn decompresses_empty_rtf_both_ways() {
        // 2.3.3.2, step 8: Outlook writes a NUL before the end marker.
        let outlook = lzfu::with_header(&[0x02, 0x00, 0x0D, 0x00], 0);
        assert_eq!(decompress(&outlook, LIMIT).unwrap(), b"");
        let plain = lzfu::with_header(&[0x01, 0x0C, 0xF0], 0);
        assert_eq!(decompress(&plain, LIMIT).unwrap(), b"");
    }

    #[test]
    fn refuses_output_over_the_limit() {
        assert_eq!(decompress(EXAMPLE_1, 0x2b).unwrap(), EXAMPLE_1_RTF);
        assert_eq!(
            decompress(EXAMPLE_1, 0x2a),
            Err(Damage::TooLarge {
                size: 0x2b,
                limit: 0x2a
            })
        );
        let mela = lzfu::uncompressed(&[b'x'; 100]);
        assert!(decompress(&mela, 100).is_ok());
        assert!(matches!(
            decompress(&mela, 99),
            Err(Damage::TooLarge { size: 100, .. })
        ));
    }

    #[test]
    fn hostile_raw_size_does_not_allocate_it() {
        // Claims 4 GiB, holds almost nothing: refused by the limit, and
        // without the limit, by the contents running out.
        let data = lzfu::with_header(&[0x00, b'a'], u32::MAX);
        assert!(matches!(
            decompress(&data, LIMIT),
            Err(Damage::TooLarge { .. })
        ));
        assert_eq!(decompress(&data, u64::MAX), Err(Damage::Truncated));
    }

    #[test]
    fn never_panics_on_arbitrary_contents() {
        // A cheap stand-in for fuzzing: every short contents with a correct
        // header and CRC.
        let mut state = 0x2545_f491_u32;
        for len in 0..64 {
            for _ in 0..200 {
                let contents: Vec<u8> = (0..len)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        state as u8
                    })
                    .collect();
                let _ = decompress(&lzfu::with_header(&contents, state % 300), LIMIT);
            }
        }
    }
}
