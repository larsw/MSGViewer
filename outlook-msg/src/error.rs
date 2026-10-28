use std::io;

/// Why a file could not be read as a Message at all.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The file is not an OLE compound file.
    #[error("not an Outlook message file: {0}")]
    NotCompoundFile(#[source] io::Error),
    /// The file is a compound file, but has no Message properties in it.
    #[error("not an Outlook message file: no message properties found")]
    NotAMessage,
    /// A stream inside the compound file could not be read.
    #[error("could not read message data: {0}")]
    Io(#[from] io::Error),
}
