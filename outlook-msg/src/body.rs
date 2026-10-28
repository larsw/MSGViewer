//! The Body of a Message ([MS-OXCMSG] 2.2.1.56).
//!
//! A Message can store its Body as plain text, HTML, Compressed RTF, or
//! several of these. [`Message::body`] picks the one to show.
//!
//! [MS-OXCMSG]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcmsg/

use crate::{Message, PropValue, html};

// Property IDs ([MS-OXPROPS]).
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
        let props = self.properties();
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

    /// The Body to show, or `None` if the Message has no Body that can be
    /// read: the HTML Body if there is one, else the plain-text Body.
    /// Compressed RTF is not read yet.
    pub fn body(&self) -> Option<Body> {
        self.html_body()
            .filter(|html| !html.trim().is_empty())
            .map(Body::Html)
            .or_else(|| {
                self.plain_text_body()
                    .map(|text| Body::PlainText(text.to_owned()))
            })
    }
}
