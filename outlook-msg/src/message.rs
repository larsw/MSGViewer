use std::io::Cursor;
use std::path::Path;

use cfb::CompoundFile;

use crate::Error;
use crate::props::{PROPERTIES_STREAM, Properties, StorageKind};

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

/// One Message read from a `.msg` file.
#[derive(Clone, Debug)]
pub struct Message {
    props: Properties,
}

impl Message {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let mut file = CompoundFile::open(Cursor::new(bytes)).map_err(Error::NotCompoundFile)?;
        let root = Path::new("/");
        if !file.is_stream(root.join(PROPERTIES_STREAM)) {
            return Err(Error::NotAMessage);
        }
        let props = Properties::read(&mut file, root, StorageKind::TopLevelMessage)?;
        Ok(Message { props })
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
