use std::io::{self, Cursor};
use std::path::Path;

use cfb::CompoundFile;

use crate::props::{PROPERTIES_STREAM, Properties, StorageKind};
use crate::reader::Reader;
use crate::{Error, Limits, Problem};

// Property IDs ([MS-OXPROPS]).
const PID_MESSAGE_CLASS: u16 = 0x001A;
const PID_SUBJECT: u16 = 0x0037;
const PID_SENT_REPRESENTING_NAME: u16 = 0x0042;
const PID_SENT_REPRESENTING_ADDRTYPE: u16 = 0x0064;
const PID_SENT_REPRESENTING_EMAIL: u16 = 0x0065;
const PID_SENDER_NAME: u16 = 0x0C1A;
const PID_SENDER_ADDRTYPE: u16 = 0x0C1E;
const PID_SENDER_EMAIL: u16 = 0x0C1F;
const PID_SENDER_SMTP_ADDRESS: u16 = 0x5D01;
const PID_SENT_REPRESENTING_SMTP_ADDRESS: u16 = 0x5D02;

/// A person or mailbox named in a Message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mailbox {
    pub name: Option<String>,
    /// The SMTP address, when one is known.
    pub address: Option<String>,
}

/// The first bytes of every compound file.
const CFB_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// One Message read from a `.msg` file.
#[derive(Clone, Debug)]
pub struct Message {
    props: Properties,
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
        let Some(props) = Properties::read(&mut reader, root, StorageKind::TopLevelMessage) else {
            let reason = reader.into_problems().pop().map(|p| p.reason);
            let reason = reason.unwrap_or_else(|| "property table can't be read".into());
            return Err(Error::Damaged(io::Error::new(
                io::ErrorKind::InvalidData,
                reason,
            )));
        };
        Ok(Message {
            props,
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

    /// The Message class, e.g. `IPM.Note`.
    pub fn message_class(&self) -> Option<&str> {
        self.props.string(PID_MESSAGE_CLASS)
    }

    pub fn subject(&self) -> Option<&str> {
        self.props.string(PID_SUBJECT)
    }

    /// Who sent the Message, preferring the actual sender over the mailbox
    /// it was sent on behalf of.
    pub fn sender(&self) -> Option<Mailbox> {
        let sender = self.mailbox(
            PID_SENDER_NAME,
            PID_SENDER_SMTP_ADDRESS,
            PID_SENDER_ADDRTYPE,
            PID_SENDER_EMAIL,
        );
        let mailbox = match sender {
            Mailbox {
                name: None,
                address: None,
            } => self.mailbox(
                PID_SENT_REPRESENTING_NAME,
                PID_SENT_REPRESENTING_SMTP_ADDRESS,
                PID_SENT_REPRESENTING_ADDRTYPE,
                PID_SENT_REPRESENTING_EMAIL,
            ),
            sender => sender,
        };
        (mailbox != Mailbox::default()).then_some(mailbox)
    }

    fn mailbox(&self, name: u16, smtp: u16, addrtype: u16, email: u16) -> Mailbox {
        let address = self.props.string(smtp).or_else(|| {
            self.props
                .string(addrtype)
                .filter(|t| t.eq_ignore_ascii_case("SMTP"))
                .and(self.props.string(email))
        });
        Mailbox {
            name: self.props.string(name).map(str::to_owned),
            address: address.map(str::to_owned),
        }
    }
}
