//! What to tell the reader about a signed or encrypted Message, whose
//! content MSG Viewer can't show (ADR-0006).

use outlook_msg::{Message, MessageKind};

/// A notice shown with a Message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtectionNotice {
    /// A banner above the Message, which is shown as usual.
    Banner(&'static str),
    /// A page in place of the Attachments and Body, below the Header block.
    Page {
        icon_name: &'static str,
        title: &'static str,
        description: &'static str,
    },
}

const ENCRYPTED: ProtectionNotice = ProtectionNotice::Page {
    icon_name: "channel-secure-symbolic",
    title: "This Message Is Encrypted",
    description: "Only its recipients can read it, in a mail app that can decrypt it",
};

const SIGNED_CONTENT_HIDDEN: ProtectionNotice = ProtectionNotice::Page {
    icon_name: "application-certificate-symbolic",
    title: "This Message Is Digitally Signed",
    description: "Its signed content can only be shown in a mail app that supports signed email",
};

const SIGNED: ProtectionNotice =
    ProtectionNotice::Banner("This message is digitally signed, but its signature isn’t checked");

/// The notice to show with `message`, if it needs one.
///
/// The Header block is always shown: S/MIME and rights management protect
/// only the Body and Attachments, so the subject, sender and recipients
/// are the real ones.
///
/// - An encrypted Message gets a page, since its Body is at best a
///   placeholder, and its one Attachment is the encrypted content.
/// - A signed Message with a Body is shown as usual, with a banner. A
///   clear-signed Message carries a readable copy of its Body next to the
///   signed content.
/// - A signed Message without a Body gets a page: an opaque-signed Message
///   keeps everything in its signed content.
pub fn protection_notice(message: &Message) -> Option<ProtectionNotice> {
    match message.kind() {
        MessageKind::Encrypted => Some(ENCRYPTED),
        MessageKind::Signed if message.body().is_some() => Some(SIGNED),
        MessageKind::Signed => Some(SIGNED_CONTENT_HIDDEN),
        MessageKind::Note | MessageKind::Other(_) => None,
    }
}
