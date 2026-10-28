//! The Body of a Message ([MS-OXCMSG] 2.2.1.56).
//!
//! A Message can store its Body as plain text, HTML, Compressed RTF, or
//! several of these. [`Message::body`] picks the one to show.
//!
//! [MS-OXCMSG]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcmsg/

use std::io::{Read, Seek};
use std::path::Path;

use crate::reader::Reader;
use crate::rtf::{self, RtfBody};
use crate::{Message, PropValue, Properties, html};

// Property IDs ([MS-OXPROPS]).
const PID_RTF_IN_SYNC: u16 = 0x0E1F;
const PID_BODY: u16 = 0x1000;
const PID_HTML: u16 = 0x1013;
const PID_INTERNET_CODEPAGE: u16 = 0x3FDE;
const PID_MESSAGE_CODEPAGE: u16 = 0x3FFD;

/// The Body to show for a Message, in the form it should be rendered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// A plain-text Body. Lines usually end in `\r\n`, as Outlook stores them.
    PlainText(String),
    /// An HTML Body, exactly as the sender wrote it, so it may reference
    /// Remote content and Inline attachments (`cid:` links). It has been
    /// decoded, so any charset its `<meta>` elements declare no longer
    /// applies.
    Html(String),
    /// The text of a Genuine RTF Body, shown as plain text. RTF isn't
    /// rendered, so its formatting is lost, and the reader should be told.
    GenuineRtf(String),
}

impl Message {
    /// The plain-text Body (`PidTagBody`), if the Message has one.
    pub fn plain_text_body(&self) -> Option<&str> {
        self.properties().string(PID_BODY)
    }

    /// The HTML Body (`PidTagHtml`), if the Message has one, decoded with the
    /// charset its own `<meta>` declares, else the Internet codepage, else
    /// the Message codepage.
    pub fn html_body(&self) -> Option<String> {
        html_body(self.properties())
    }

    /// The Body to show, or `None` if the Message has no Body that can be
    /// read. In order of preference:
    ///
    /// 1. The HTML Body (`PidTagHtml`). When a Message has both HTML and
    ///    Compressed RTF, the HTML is the original, or at worst converted
    ///    from the RTF with its formatting.
    /// 2. An Encapsulated HTML body from the Compressed RTF. Many Messages
    ///    written in HTML keep it only there, and the HTML shows them as
    ///    written, where the plain-text Body has lost their formatting.
    /// 3. The plain-text Body (`PidTagBody`). When the Compressed RTF holds
    ///    an Encapsulated plain-text body, they are the same text; when it
    ///    holds Genuine RTF, the plain-text Body is its text, and comes as
    ///    [`Body::GenuineRtf`].
    /// 4. The text from the Compressed RTF, if the plain-text Body is
    ///    missing or blank, or `PidTagRtfInSync` is false, which means the
    ///    RTF was changed after the plain-text Body was written. (Without
    ///    `PidTagRtfInSync`, the two may differ in either direction, and the
    ///    plain-text Body is still preferred.)
    ///
    /// Compressed RTF that can't be read was recorded as a Problem when the
    /// Message was read, and is skipped here.
    pub fn body(&self) -> Option<Body> {
        if let Some(html) = self.html_body().filter(|html| !is_blank(html)) {
            return Some(Body::Html(html));
        }
        let plain = self.plain_text_body();
        let rtf_is_newer = self.properties().get(PID_RTF_IN_SYNC) == Some(&PropValue::Bool(false));
        let text = |rtf_text: &str| match plain {
            Some(plain) if !rtf_is_newer && !is_blank(plain) => plain.to_owned(),
            _ => rtf_text.to_owned(),
        };
        let from_rtf = match self.rtf_body() {
            Some(RtfBody::Html(html)) => Some(Body::Html(html.clone())),
            Some(RtfBody::PlainText(rtf_text)) => Some(Body::PlainText(text(rtf_text))),
            Some(RtfBody::Genuine(rtf_text)) => Some(Body::GenuineRtf(text(rtf_text))),
            None => None,
        };
        from_rtf
            .filter(|body| match body {
                Body::Html(text) | Body::PlainText(text) | Body::GenuineRtf(text) => {
                    !is_blank(text)
                }
            })
            .or_else(|| plain.map(|text| Body::PlainText(text.to_owned())))
    }
}

/// Reads the Compressed RTF of a Message whose Body could come from it: one
/// without an HTML Body. Damaged RTF is recorded as a Problem.
pub(crate) fn read_rtf<F: Read + Seek>(
    reader: &mut Reader<F>,
    storage: &Path,
    props: &Properties,
) -> Option<RtfBody> {
    if html_body(props).is_some_and(|html| !is_blank(&html)) {
        return None;
    }
    rtf::read(reader, storage, props)
}

fn html_body(props: &Properties) -> Option<String> {
    let html = match props.get(PID_HTML)? {
        PropValue::Binary(bytes) => html::decode(
            bytes,
            &[
                props.int(PID_INTERNET_CODEPAGE),
                props.int(PID_MESSAGE_CODEPAGE),
            ],
        ),
        PropValue::String(html) => html.clone(),
        _ => return None,
    };
    Some(html)
}

fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}
