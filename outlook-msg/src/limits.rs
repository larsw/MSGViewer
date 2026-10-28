//! How much of an untrusted file the parser is willing to read.

const MIB: u64 = 1024 * 1024;

/// Caps on what one `.msg` file can make the parser do.
///
/// [`Limits::DEFAULT`] fits every real Message we know of with room to spare;
/// smaller limits are mainly useful in tests and fuzzing. Whatever a limit
/// stops from being read is left out of the Message and reported as a
/// [`Problem`](crate::Problem), so hitting one gives a Partial read, never an
/// error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Largest single stream read, in bytes. Longer streams are skipped.
    ///
    /// Exchange Online caps a whole message at 150 MB of MIME, which is
    /// about 112 MB once the base64 encoding of attachments is undone, so
    /// 128 MiB holds the biggest Attachment a real Message can carry.
    pub max_stream_len: u64,
    /// Most bytes read from all streams of one file together.
    ///
    /// A real file is read about once, so this is twice
    /// [`max_stream_len`](Self::max_stream_len). Streams in a compound file
    /// can share sectors, so without this cap a small hostile file could
    /// make the parser read the same data over and over. Every stream counts
    /// as at least 64 bytes (one mini sector), so a file can't make the
    /// parser open millions of empty streams either.
    pub max_total_read: u64,
    /// Deepest Embedded message read: 1 allows Embedded messages in the
    /// top-level Message, but none inside those.
    ///
    /// Forwarding a Message as an attachment again and again rarely goes
    /// more than a few levels deep. 10 leaves room for that, while keeping a
    /// hostile file from nesting Messages until the stack overflows.
    pub max_embedded_depth: usize,
    /// Most recipients read for one Message; the rest are skipped.
    ///
    /// Exchange Online allows at most 1,000 recipients per message, and
    /// that only when an administrator raises the default of 500.
    pub max_recipients: usize,
    /// Most Attachments read for one Message; the rest are skipped.
    ///
    /// No mail client produces anything close to this, and it keeps a
    /// hostile file from making the viewer build thousands of widgets.
    pub max_attachments: usize,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        max_stream_len: 128 * MIB,
        max_total_read: 256 * MIB,
        max_embedded_depth: 10,
        max_recipients: 2048,
        max_attachments: 1024,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}
