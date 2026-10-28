//! Reader for Microsoft Outlook `.msg` files.
//!
//! A `.msg` file is an OLE compound file holding MAPI properties, laid out as
//! described in [MS-OXMSG]. [`Message::from_bytes`] reads one into an owned
//! [`Message`]; nothing borrows from the input afterwards.
//!
//! # Damaged files
//!
//! `.msg` files come from strangers, so the parser never panics, and reads
//! no more than its [`Limits`] allow. Reading has two failure outcomes:
//!
//! - An Unreadable file, which isn't a Message at all, or is too damaged for
//!   anything in it to be read: [`Message::from_bytes`] returns an [`Error`].
//! - A Partial read, where some Properties, recipients, Attachments or
//!   Embedded messages were damaged, missing or over a limit: the rest is
//!   still read, [`Message::is_partial`] is true, and
//!   [`Message::problems`] says what was left out.
//!
//! Inside the crate, all parsing goes through a `Reader`, which enforces the
//! limits and collects the Problems; see `src/reader.rs` for how to use it.
//!
//! [MS-OXMSG]: https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxmsg/

mod attachment;
mod body;
mod error;
mod limits;
mod message;
mod props;
mod reader;

pub use attachment::{Attachment, sanitize_file_name};
pub use body::Body;
pub use error::Error;
pub use limits::Limits;
pub use message::{Mailbox, Message, Recipient, RecipientKind};
pub use props::{PropValue, Properties};
pub use reader::Problem;
