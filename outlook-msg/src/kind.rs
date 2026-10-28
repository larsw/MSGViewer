//! What kind of Message a file holds, from its Message class
//! (`PidTagMessageClass`, [MS-OXCMSG] 2.2.1.3), as far as showing it is
//! concerned.
//!
//! Message classes are matched without regard to case, and a class also
//! covers the classes derived from it: `IPM.Note` covers
//! `IPM.Note.Mobile.SMS`, but not `IPM.Notebook`.
//!
//! Signed and encrypted Messages are recognised as [MS-OXOSMIME] and
//! [MS-OXORMMS] describe them; their content is never parsed or decrypted.
//!
//! [MS-OXCMSG]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcmsg/
//! [MS-OXOSMIME]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxosmime/
//! [MS-OXORMMS]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxormms/

use crate::{Attachment, Message};

/// The MIME type of the single Attachment of a rights-managed (IRM) Message,
/// which holds its encrypted content ([MS-OXORMMS] 3.1.4.1.2).
const RPMSG_MIME_TYPE: &str = "application/x-microsoft-rpmsg-message";

/// CMS content types ([RFC 5652], [RFC 5083]) as DER-encoded object
/// identifiers, tag and length included.
const OID_SIGNED_DATA: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02,
];
const OID_ENVELOPED_DATA: &[u8] = &[
    0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x03,
];
const OID_AUTH_ENVELOPED_DATA: &[u8] = &[
    0x06, 0x0B, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x10, 0x01, 0x17,
];
/// How far into an S/MIME Attachment its content type is looked for. It
/// follows the outer SEQUENCE tag and length, at most 6 bytes.
const CONTENT_TYPE_WINDOW: usize = 24;

/// What kind of Message this is, which decides how it is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageKind {
    /// An ordinary email: `IPM.Note` or a class derived from it, or a
    /// Message without a Message class.
    Note,
    /// Encrypted with S/MIME or rights management (IRM): its Body and
    /// Attachments are in an Attachment that can't be read without the
    /// recipient's key. Any Body stored alongside is at best a placeholder.
    Encrypted,
    /// Digitally signed with S/MIME. A clear-signed Message usually has a
    /// copy of its Body stored alongside the signed content, which is in an
    /// Attachment; an opaque-signed one usually has no Body at all.
    Signed,
    /// Any other Message class, such as `IPM.Appointment`, `IPM.Contact`
    /// or `REPORT.IPM.Note.IPNRN`, as stored in the file.
    Other(String),
}

impl Message {
    /// What kind of Message this is, from its Message class and, for
    /// S/MIME and rights-managed Messages, the Attachment that holds their
    /// content.
    pub fn kind(&self) -> MessageKind {
        let Some(class) = self.message_class().map(str::trim) else {
            return MessageKind::Note;
        };
        if class.is_empty() {
            return MessageKind::Note;
        }
        // Reports about a signed or encrypted Message, such as
        // `REPORT.IPM.Note.SMIME.NDR`, are not themselves protected.
        if !class_is(class, "REPORT") {
            // Outlook goes by the suffix alone ([MS-OXOSMIME] 2.1.3.1,
            // 2.1.3.2.2.2 and Appendix A), which also covers classes such as
            // `IPM.Note.Receipt.SMIME` and InfoPath forms.
            if ends_with(class, ".SMIME.MultipartSigned") {
                return MessageKind::Signed;
            }
            if ends_with(class, ".SMIME") {
                return smime_kind(self.attachments());
            }
            // Signed and encrypted Messages from Exchange's old Key
            // Management Server.
            if class_is(class, "IPM.Note.Secure.Sign") {
                return MessageKind::Signed;
            }
            if class_is(class, "IPM.Note.Secure") {
                return MessageKind::Encrypted;
            }
        }
        if class_is(class, "IPM.Note") {
            if is_rights_managed(self.attachments()) {
                return MessageKind::Encrypted;
            }
            return MessageKind::Note;
        }
        MessageKind::Other(class.to_owned())
    }
}

/// Whether `class` is `base` or derived from it, ignoring case.
fn class_is(class: &str, base: &str) -> bool {
    match class.get(..base.len()) {
        Some(start) if start.eq_ignore_ascii_case(base) => {
            matches!(class.as_bytes().get(base.len()), None | Some(b'.'))
        }
        _ => false,
    }
}

/// Whether `class` ends with `suffix`, ignoring case.
fn ends_with(class: &str, suffix: &str) -> bool {
    class
        .len()
        .checked_sub(suffix.len())
        .and_then(|start| class.get(start..))
        .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
}

/// Tells an encrypted `IPM.Note.SMIME` Message from a signed one by its one
/// Attachment, as [MS-OXOSMIME] Appendix A <3> and <12> recommend: by its
/// MIME type, and else by the CMS content type its data starts with. When
/// neither tells, the Message is taken to be encrypted, which is what the
/// class usually means.
fn smime_kind(attachments: &[Attachment]) -> MessageKind {
    let Some(attachment) = attachments.first() else {
        return MessageKind::Encrypted;
    };
    let mime_type = attachment
        .mime_type()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if mime_type.starts_with("multipart/signed") {
        // A clear-signed Message labelled `IPM.Note.SMIME`.
        return MessageKind::Signed;
    }
    // The smime-type parameter of application/pkcs7-mime ([RFC 8551] 3.2).
    if mime_type.contains("enveloped-data") {
        return MessageKind::Encrypted;
    }
    if mime_type.contains("signed-data") {
        return MessageKind::Signed;
    }
    let start = attachment.data().unwrap_or_default();
    let start = start.get(..CONTENT_TYPE_WINDOW).unwrap_or(start);
    if contains(start, OID_ENVELOPED_DATA) || contains(start, OID_AUTH_ENVELOPED_DATA) {
        MessageKind::Encrypted
    } else if contains(start, OID_SIGNED_DATA) {
        MessageKind::Signed
    } else {
        MessageKind::Encrypted
    }
}

/// Whether the Message is rights-managed: the first Attachment holds its
/// encrypted content ([MS-OXORMMS] 3.1.4.1.2). The Message class stays
/// `IPM.Note`; Outlook marks such Messages by a content class, a named
/// Property, but the Attachment is enough to tell.
fn is_rights_managed(attachments: &[Attachment]) -> bool {
    attachments
        .first()
        .and_then(Attachment::mime_type)
        .is_some_and(|mime_type| mime_type.trim().eq_ignore_ascii_case(RPMSG_MIME_TYPE))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_class_and_derived_classes_ignoring_case() {
        assert!(class_is("IPM.Note", "IPM.Note"));
        assert!(class_is("ipm.note", "IPM.Note"));
        assert!(class_is("IPM.Note.Mobile.SMS", "IPM.Note"));
        assert!(!class_is("IPM.Notebook", "IPM.Note"));
        assert!(!class_is("IPM", "IPM.Note"));
        assert!(!class_is("IPM.Not", "IPM.Note"));
        // Slicing inside a multi-byte character must not panic.
        assert!(!class_is("IPM.Noté", "IPM.Note."));
        assert!(!class_is("ÉÉÉÉ", "IPM"));
    }

    #[test]
    fn matches_suffix_ignoring_case() {
        assert!(ends_with("IPM.Note.SMIME", ".SMIME"));
        assert!(ends_with("ipm.note.smime", ".SMIME"));
        assert!(!ends_with("IPM.Note.SMIME.MultipartSigned", ".SMIME"));
        assert!(!ends_with("SMIME", ".SMIME"));
        assert!(!ends_with("éSMIME", ".SMIME"));
    }

    #[test]
    fn finds_content_type() {
        let mut signed = vec![0x30, 0x80];
        signed.extend_from_slice(OID_SIGNED_DATA);
        assert!(contains(&signed, OID_SIGNED_DATA));
        assert!(!contains(&signed[..5], OID_SIGNED_DATA));
        assert!(!contains(&[], OID_SIGNED_DATA));
    }
}
