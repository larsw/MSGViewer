use std::io;

/// Why a file is an Unreadable file: it could not be read as a Message at all.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file is not an OLE compound file.
    #[error("not an Outlook message file: {0}")]
    NotCompoundFile(#[source] io::Error),
    /// The file is a compound file, but has no Message properties in it,
    /// such as an older Word or Excel document.
    #[error("not an Outlook message file: no message properties found")]
    NotAMessage,
    /// The file looks like a compound file or a Message, but is too damaged
    /// for anything in it to be read.
    #[error("damaged Outlook message file: {0}")]
    Damaged(#[source] io::Error),
}
