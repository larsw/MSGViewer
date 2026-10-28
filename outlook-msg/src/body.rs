//! The Body of a Message ([MS-OXCMSG] 2.2.1.56).
//!
//! A Message can store its Body as plain text, HTML, Compressed RTF, or
//! several of these. [`Message::body`] picks the one to show.
//!
//! [MS-OXCMSG]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcmsg/

use crate::Message;

// Property IDs ([MS-OXPROPS]).
const PID_BODY: u16 = 0x1000;

/// The Body to show for a Message, in the form it should be rendered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// A plain-text Body. Lines usually end in `\r\n`, as Outlook stores them.
    PlainText(String),
}

impl Message {
    /// The plain-text Body (`PidTagBody`), if the Message has one.
    pub fn plain_text_body(&self) -> Option<&str> {
        self.properties().string(PID_BODY)
    }

    /// The Body to show, or `None` if the Message has no Body that can be
    /// read. Only plain-text Bodies are read so far.
    pub fn body(&self) -> Option<Body> {
        self.plain_text_body()
            .map(|text| Body::PlainText(text.to_owned()))
    }
}
