//! Reader for Microsoft Outlook `.msg` files.
//!
//! A `.msg` file is an OLE compound file holding MAPI properties, laid out as
//! described in [MS-OXMSG]. [`Message::from_bytes`] reads one into an owned
//! [`Message`]; nothing borrows from the input afterwards.
//!
//! [MS-OXMSG]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxmsg/

mod body;
mod error;
mod message;
mod props;

pub use body::Body;
pub use error::Error;
pub use message::{Mailbox, Message};
pub use props::{PropValue, Properties};
