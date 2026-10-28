use std::io::{self, Cursor, Read, Seek};
use std::path::Path;
use std::time::SystemTime;

use cfb::CompoundFile;
use encoding_rs::Encoding;

use crate::attachment::{self, Attachment};
use crate::body;
use crate::props::{DEFAULT_ENCODING, PROPERTIES_STREAM, Properties, StorageKind};
use crate::reader::Reader;
use crate::rtf::RtfBody;
use crate::{Error, Limits, Problem};

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

/// The first bytes of every compound file.
const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// One Message read from a `.msg` file.
#[derive(Clone, Debug)]
pub struct Message {
    props: Properties,
    recipients: Vec<Recipient>,
    attachments: Vec<Attachment>,
    /// What the Compressed RTF holds, if the Body could come from it.
    rtf: Option<RtfBody>,
    problems: Vec<Problem>,
}

impl Message {
    /// Reads a Message within [`Limits::DEFAULT`].
    ///
    /// Fails only for an Unreadable file. When parts of the file are
    /// damaged, the rest is still read: see [`Message::is_partial`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        Self::from_bytes_with_limits(bytes, Limits::DEFAULT)
    }

    /// Reads a Message, reading no more than `limits` allow.
    pub fn from_bytes_with_limits(bytes: &[u8], limits: Limits) -> Result<Self, Error> {
        let file = CompoundFile::open(Cursor::new(bytes)).map_err(|error| {
            if bytes.starts_with(&CFB_SIGNATURE) {
                Error::Damaged(error)
            } else {
                Error::NotCompoundFile(error)
            }
        })?;
        let mut reader = Reader::new(file, limits);
        let root = Path::new("/");
        if !reader.is_stream(&root.join(PROPERTIES_STREAM)) {
            return Err(Error::NotAMessage);
        }
        let Some(props) = Properties::read(
            &mut reader,
            root,
            StorageKind::TopLevelMessage,
            DEFAULT_ENCODING,
        ) else {
            let reason = reader.into_problems().pop().map(|p| p.reason);
            let reason = reason.unwrap_or_else(|| "property table can't be read".into());
            return Err(Error::Damaged(io::Error::new(
                io::ErrorKind::InvalidData,
                reason,
            )));
        };
        let recipients = read_recipients(&mut reader, props.encoding());
        let attachments = attachment::read_all(&mut reader, root, props.encoding());
        let rtf = body::read_rtf(&mut reader, root, &props);
        Ok(Message {
            props,
            recipients,
            attachments,
            rtf,
            problems: reader.into_problems(),
        })
    }

    /// Whether this is a Partial read: some of the Message was damaged or
    /// missing and has been left out.
    pub fn is_partial(&self) -> bool {
        !self.problems.is_empty()
    }

    /// What was left out of a Partial read, at most the first hundred
    /// Problems.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// All properties of the Message.
    pub fn properties(&self) -> &Properties {
        &self.props
    }

    /// What the Compressed RTF holds, if the Body could come from it.
    pub(crate) fn rtf_body(&self) -> Option<&RtfBody> {
        self.rtf.as_ref()
    }

    /// The Attachments, in the order the sender added them.
    pub fn attachments(&self) -> &[Attachment] {
        &self.attachments
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

/// Reads the recipients, in the order the Message numbers them. A damaged
/// recipient is left out and recorded as a Problem.
fn read_recipients<F: Read + Seek>(
    reader: &mut Reader<F>,
    encoding: &'static Encoding,
) -> Vec<Recipient> {
    let limit = reader.limits().max_recipients;
    let storages = reader.child_storages(Path::new("/"), RECIPIENT_STORAGE_PREFIX, limit);
    let mut recipients = Vec::new();
    for storage in storages {
        let kind = StorageKind::RecipientOrAttachment;
        if let Some(props) = Properties::read(reader, &storage, kind, encoding) {
            recipients.extend(recipient(&props));
        }
    }
    recipients
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
