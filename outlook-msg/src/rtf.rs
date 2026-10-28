//! RTF Bodies: Compressed RTF ([MS-OXRTFCP]) and the Encapsulated body that
//! may be inside it ([MS-OXRTFEX]).
//!
//! Outlook keeps the Body of many Messages only as Compressed RTF
//! (`PidTagRtfCompressed`). When the Message was written in HTML or plain
//! text, the RTF encapsulates it, and the original can be recovered exactly.
//! Genuine RTF isn't rendered (ADR-0006); only its text is taken out.
//!
//! [MS-OXRTFCP]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxrtfcp/
//! [MS-OXRTFEX]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxrtfex/

mod compressed;
mod encapsulation;

use std::io::{Read, Seek};
use std::path::Path;

use crate::reader::Reader;
use crate::{PropValue, Properties};

use encapsulation::Origin;

/// `PidTagRtfCompressed` ([MS-OXPROPS]), a binary Property.
const PID_RTF_COMPRESSED: u16 = 0x1009;

/// What a Message's Compressed RTF holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RtfBody {
    /// An Encapsulated HTML body.
    Html(String),
    /// An Encapsulated plain-text body.
    PlainText(String),
    /// Genuine RTF, as plain text: its formatting is lost.
    Genuine(String),
}

/// Reads the Compressed RTF in the Properties of `storage`, if it has any.
///
/// RTF that is damaged, over [`Limits::max_stream_len`](crate::Limits)
/// uncompressed, or not RTF at all, is left out and recorded as a Problem.
/// Empty RTF is left out without one.
pub(crate) fn read<F: Read + Seek>(
    reader: &mut Reader<F>,
    storage: &Path,
    props: &Properties,
) -> Option<RtfBody> {
    let PropValue::Binary(data) = props.get(PID_RTF_COMPRESSED)? else {
        return None;
    };
    let path = storage.join(format!("__substg1.0_{PID_RTF_COMPRESSED:04X}0102"));
    let rtf = match compressed::decompress(data, reader.limits().max_stream_len) {
        Ok(rtf) => rtf,
        Err(damage) => {
            reader.problem(&path, damage);
            return None;
        }
    };
    if rtf.iter().all(|b| b.is_ascii_whitespace() || *b == 0) {
        return None;
    }
    if !encapsulation::is_rtf(&rtf) {
        reader.problem(&path, "Compressed RTF does not hold RTF");
        return None;
    }
    let origin = encapsulation::origin(&rtf);
    let body = encapsulation::extract(&rtf, origin);
    Some(match origin {
        Origin::Html => RtfBody::Html(body),
        Origin::Text => RtfBody::PlainText(body),
        Origin::Rtf => RtfBody::Genuine(body),
    })
}
