use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cfb::CompoundFile;
use encoding_rs::Encoding;

use crate::Error;
use crate::props::{DEFAULT_ENCODING, PROPERTIES_STREAM, Properties, StorageKind};

// Property IDs ([MS-OXPROPS]).
const PID_MESSAGE_CLASS: u16 = 0x001A;
const PID_SUBJECT: u16 = 0x0037;
const PID_CLIENT_SUBMIT_TIME: u16 = 0x0039;
const PID_SENT_REPRESENTING_NAME: u16 = 0x0042;
const PID_SENT_REPRESENTING_ADDRTYPE: u16 = 0x0064;
const PID_SENT_REPRESENTING_EMAIL: u16 = 0x0065;
const PID_RECIPIENT_TYPE: u16 = 0x0C15;
const PID_SENDER_NAME: u16 = 0x0C1A;
const PID_SENDER_ADDRTYPE: u16 = 0x0C1E;
const PID_SENDER_EMAIL: u16 = 0x0C1F;
const PID_MESSAGE_DELIVERY_TIME: u16 = 0x0E06;
const PID_DISPLAY_NAME: u16 = 0x3001;
const PID_ADDRTYPE: u16 = 0x3002;
const PID_EMAIL_ADDRESS: u16 = 0x3003;
const PID_SMTP_ADDRESS: u16 = 0x39FE;
const PID_SENDER_SMTP_ADDRESS: u16 = 0x5D01;
const PID_SENT_REPRESENTING_SMTP_ADDRESS: u16 = 0x5D02;
const PID_RECIPIENT_DISPLAY_NAME: u16 = 0x5FF6;

/// Flag in PidTagRecipientType for a recipient that already received a
/// resent Message.
const MAPI_SUBMITTED: u32 = 0x8000_0000;

/// Name prefix of recipient storages, followed by an 8-digit hex index
/// ([MS-OXMSG] 2.2.1).
const RECIPIENT_STORAGE_PREFIX: &str = "__recip_version1.0_#";
/// The most recipient storages a `.msg` file may hold ([MS-OXMSG] 2.2.1).
const MAX_RECIPIENTS: usize = 2048;

/// A person or mailbox named in a Message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mailbox {
    pub name: Option<String>,
    /// The SMTP address, when one is known.
    pub address: Option<String>,
}

/// Which recipient line of a Message a recipient is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecipientKind {
    To,
    Cc,
    Bcc,
}

/// Someone a Message was addressed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipient {
    pub kind: RecipientKind,
    pub mailbox: Mailbox,
}

/// One Message read from a `.msg` file.
#[derive(Clone, Debug)]
pub struct Message {
    props: Properties,
    recipients: Vec<Recipient>,
}

impl Message {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let mut file = CompoundFile::open(Cursor::new(bytes)).map_err(Error::NotCompoundFile)?;
        let root = Path::new("/");
        if !file.is_stream(root.join(PROPERTIES_STREAM)) {
            return Err(Error::NotAMessage);
        }
        let props = Properties::read(
            &mut file,
            root,
            StorageKind::TopLevelMessage,
            DEFAULT_ENCODING,
        )?;
        let recipients = read_recipients(&mut file, props.encoding())?;
        Ok(Message { props, recipients })
    }

    /// All properties of the Message.
    pub fn properties(&self) -> &Properties {
        &self.props
    }

    /// The Message class, e.g. `IPM.Note`.
    pub fn message_class(&self) -> Option<&str> {
        self.props.string(PID_MESSAGE_CLASS)
    }

    pub fn subject(&self) -> Option<&str> {
        self.props.string(PID_SUBJECT)
    }

    /// When the Message was sent, falling back to when it was delivered.
    pub fn date(&self) -> Option<SystemTime> {
        self.props
            .time(PID_CLIENT_SUBMIT_TIME)
            .or_else(|| self.props.time(PID_MESSAGE_DELIVERY_TIME))
    }

    /// Who sent the Message, preferring the actual sender over the mailbox
    /// it was sent on behalf of.
    pub fn sender(&self) -> Option<Mailbox> {
        mailbox(
            &self.props,
            &[PID_SENDER_NAME],
            PID_SENDER_SMTP_ADDRESS,
            PID_SENDER_ADDRTYPE,
            PID_SENDER_EMAIL,
        )
        .or_else(|| {
            mailbox(
                &self.props,
                &[PID_SENT_REPRESENTING_NAME],
                PID_SENT_REPRESENTING_SMTP_ADDRESS,
                PID_SENT_REPRESENTING_ADDRTYPE,
                PID_SENT_REPRESENTING_EMAIL,
            )
        })
    }

    /// The To, Cc and Bcc recipients, in the order the Message lists them.
    pub fn recipients(&self) -> &[Recipient] {
        &self.recipients
    }
}

/// Reads a Mailbox from `props`, taking the first non-empty of the `names`
/// and preferring the SMTP address over the email address, which is only an
/// SMTP address if the address type says so (Exchange uses `EX` addresses).
fn mailbox(
    props: &Properties,
    names: &[u16],
    smtp: u16,
    addrtype: u16,
    email: u16,
) -> Option<Mailbox> {
    let non_empty = |id| props.string(id).filter(|s| !s.is_empty());
    let name = names.iter().find_map(|&id| non_empty(id));
    let address = non_empty(smtp).or_else(|| {
        props
            .string(addrtype)
            .filter(|t| t.eq_ignore_ascii_case("SMTP"))
            .and(non_empty(email))
    });
    let mailbox = Mailbox {
        name: name.map(str::to_owned),
        address: address.map(str::to_owned),
    };
    (mailbox != Mailbox::default()).then_some(mailbox)
}

fn read_recipients<F: Read + Seek>(
    file: &mut CompoundFile<F>,
    encoding: &'static Encoding,
) -> Result<Vec<Recipient>, Error> {
    let mut storages: Vec<(u32, PathBuf)> = file
        .read_root_storage()
        .filter(|entry| entry.is_storage())
        .filter_map(|entry| Some((recipient_index(entry.name())?, entry.path().to_owned())))
        .collect();
    storages.sort_unstable_by_key(|(index, _)| *index);
    storages.truncate(MAX_RECIPIENTS);

    let mut recipients = Vec::new();
    for (_, storage) in storages {
        if !file.is_stream(storage.join(PROPERTIES_STREAM)) {
            continue;
        }
        let props = Properties::read(file, &storage, StorageKind::RecipientOrAttachment, encoding)?;
        recipients.extend(recipient(&props));
    }
    Ok(recipients)
}

/// The index in a recipient storage's name, `__recip_version1.0_#XXXXXXXX`.
fn recipient_index(name: &str) -> Option<u32> {
    let hex = name.strip_prefix(RECIPIENT_STORAGE_PREFIX)?;
    if hex.len() != 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// A Recipient from its properties, or `None` if it is not a To, Cc or Bcc
/// recipient or names nobody.
fn recipient(props: &Properties) -> Option<Recipient> {
    // MAPI_SUBMITTED may be set on To, Cc and Bcc; MAPI_P1 marks a copy of
    // a recipient made for resending, which is not shown. A recipient without
    // a type is most likely a To recipient.
    let kind = match props
        .int(PID_RECIPIENT_TYPE)
        .map(|t| t as u32 & !MAPI_SUBMITTED)
    {
        None | Some(1) => RecipientKind::To,
        Some(2) => RecipientKind::Cc,
        Some(3) => RecipientKind::Bcc,
        Some(_) => return None,
    };
    let mailbox = mailbox(
        props,
        &[PID_DISPLAY_NAME, PID_RECIPIENT_DISPLAY_NAME],
        PID_SMTP_ADDRESS,
        PID_ADDRTYPE,
        PID_EMAIL_ADDRESS,
    )?;
    Some(Recipient { kind, mailbox })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recipient_storage_index() {
        assert_eq!(recipient_index("__recip_version1.0_#0000000A"), Some(10));
        assert_eq!(recipient_index("__recip_version1.0_#0000000"), None);
        assert_eq!(recipient_index("__recip_version1.0_#+0000000"), None);
        assert_eq!(recipient_index("__attach_version1.0_#00000000"), None);
    }
}
