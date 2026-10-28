//! Bounded, fault-tolerant reading of the compound file.
//!
//! All parsing goes through a [`Reader`], which enforces the [`Limits`] and
//! collects a [`Problem`] for everything it had to leave out. Parsing code
//! never fails because one part of a file is damaged: it records a Problem
//! and carries on with the rest, and the Message ends up a Partial read.
//!
//! - Read streams with [`Reader::read_stream`]. It returns `None`, with the
//!   Problem already recorded, when the stream is missing, too large, over
//!   the read budget or damaged; skip whatever needed it.
//! - Read a storage's Properties with
//!   [`Properties::read`](crate::Properties), which skips bad Properties the
//!   same way.
//! - List recipient or attachment storages with [`Reader::child_storages`],
//!   passing the matching limit from [`Reader::limits`], so that the rest
//!   are skipped and reported.
//! - Before reading an Embedded message, get the Limits to read it with from
//!   [`Reader::embedded_limits`], which refuses to go deeper than
//!   [`Limits::max_embedded_depth`].
//! - For anything else that is wrong but can be skipped, call
//!   [`Reader::problem`].

use std::fmt;
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};

use cfb::CompoundFile;

use crate::Limits;

/// Every stream read counts as at least one mini sector against the read
/// budget.
const MIN_STREAM_COST: u64 = 64;

/// Most Problems kept for one file; a badly damaged file could otherwise
/// produce one for every Property it names.
const MAX_PROBLEMS: usize = 100;

/// Something in a Message that couldn't be read, and so was left out of a
/// Partial read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// Where in the compound file it is, e.g. `/__substg1.0_0037001F`.
    pub path: String,
    /// What went wrong, in English, for logs and bug reports.
    pub reason: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.reason)
    }
}

pub(crate) struct Reader<F> {
    file: CompoundFile<F>,
    limits: Limits,
    /// Bytes left of `limits.max_total_read`.
    budget: u64,
    problems: Vec<Problem>,
}

impl<F: Read + Seek> Reader<F> {
    pub(crate) fn new(file: CompoundFile<F>, limits: Limits) -> Self {
        Reader {
            file,
            budget: limits.max_total_read,
            limits,
            problems: Vec::new(),
        }
    }

    pub(crate) fn limits(&self) -> &Limits {
        &self.limits
    }

    /// The compound file, for looking at its directory. Read stream data
    /// through the Reader instead, so that it counts against the limits.
    pub(crate) fn file(&self) -> &CompoundFile<F> {
        &self.file
    }

    pub(crate) fn is_stream(&self, path: &Path) -> bool {
        self.file.is_stream(path)
    }

    /// Records that something at `path` couldn't be read and was skipped.
    pub(crate) fn problem(&mut self, path: &Path, reason: impl fmt::Display) {
        if self.problems.len() < MAX_PROBLEMS {
            self.problems.push(Problem {
                path: path.display().to_string(),
                reason: reason.to_string(),
            });
        }
    }

    pub(crate) fn into_problems(self) -> Vec<Problem> {
        self.problems
    }

    /// Reads a whole stream, or records a Problem and returns `None`.
    pub(crate) fn read_stream(&mut self, path: &Path) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        match self.read_stream_into(path, &mut data) {
            Ok(()) => Some(data),
            Err(error) => {
                self.problem(path, error);
                None
            }
        }
    }

    /// Reads a whole stream into `data`. On error, `data` holds whatever
    /// was read before it, which callers may salvage.
    pub(crate) fn read_stream_into(&mut self, path: &Path, data: &mut Vec<u8>) -> io::Result<()> {
        if !self.file.is_stream(path) {
            self.charge(MIN_STREAM_COST)?;
            return Err(io::Error::new(io::ErrorKind::NotFound, "stream is missing"));
        }
        let mut stream = self.file.open_stream(path)?;
        let len = stream.len();
        if len > self.limits.max_stream_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "stream is {len} bytes, more than the limit of {}",
                    self.limits.max_stream_len
                ),
            ));
        }
        self.charge(len.max(MIN_STREAM_COST))?;
        stream.read_to_end(data)?;
        Ok(())
    }

    fn charge(&mut self, cost: u64) -> io::Result<()> {
        match self.budget.checked_sub(cost) {
            Some(left) => {
                self.budget = left;
                Ok(())
            }
            None => {
                self.budget = 0;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "file needs more than {} bytes read in all",
                        self.limits.max_total_read
                    ),
                ))
            }
        }
    }

    /// The numbered storages directly inside `parent`, named `prefix`
    /// followed by an 8-digit hex number, such as `__recip_version1.0_#`
    /// ([MS-OXMSG] 2.2.1), in number order. Past `limit` the rest are
    /// skipped and a Problem is recorded.
    pub(crate) fn child_storages(
        &mut self,
        parent: &Path,
        prefix: &str,
        limit: usize,
    ) -> Vec<PathBuf> {
        let mut storages: Vec<(u32, PathBuf)> = match self.file.read_storage(parent) {
            Ok(entries) => entries
                .filter(|entry| entry.is_storage())
                .filter_map(|entry| {
                    Some((
                        storage_number(entry.name(), prefix)?,
                        entry.path().to_owned(),
                    ))
                })
                .collect(),
            Err(error) => {
                self.problem(parent, error);
                return Vec::new();
            }
        };
        storages.sort_unstable();
        if storages.len() > limit {
            let skipped = storages.len() - limit;
            storages.truncate(limit);
            self.problem(
                parent,
                format!("{skipped} {prefix} storages over the limit of {limit} were skipped"),
            );
        }
        storages.into_iter().map(|(_, path)| path).collect()
    }

    /// The Limits to read the Embedded message at `path` with, one level
    /// less deep than this Reader's. Past [`Limits::max_embedded_depth`] it
    /// records a Problem instead, and returns `None`.
    pub(crate) fn embedded_limits(&mut self, path: &Path) -> Option<Limits> {
        match self.limits.max_embedded_depth.checked_sub(1) {
            Some(depth) => Some(Limits {
                max_embedded_depth: depth,
                ..self.limits
            }),
            None => {
                self.problem(path, "Embedded message nested too deep was skipped");
                None
            }
        }
    }
}

/// The number in a storage name, `prefix` then 8 hex digits.
fn storage_number(name: &str, prefix: &str) -> Option<u32> {
    let hex = name.strip_prefix(prefix)?;
    if hex.len() != 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    fn compound_file(build: impl FnOnce(&mut CompoundFile<Cursor<Vec<u8>>>)) -> Vec<u8> {
        let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        build(&mut file);
        file.flush().unwrap();
        file.into_inner().into_inner()
    }

    fn reader(bytes: &[u8], limits: Limits) -> Reader<Cursor<&[u8]>> {
        Reader::new(CompoundFile::open(Cursor::new(bytes)).unwrap(), limits)
    }

    fn write_stream(file: &mut CompoundFile<Cursor<Vec<u8>>>, path: &str, data: &[u8]) {
        file.create_stream(path).unwrap().write_all(data).unwrap();
    }

    #[test]
    fn reads_stream() {
        let bytes = compound_file(|file| write_stream(file, "/a", b"hello"));
        let mut reader = reader(&bytes, Limits::DEFAULT);
        assert_eq!(
            reader.read_stream(Path::new("/a")).as_deref(),
            Some(&b"hello"[..])
        );
        assert!(reader.into_problems().is_empty());
    }

    #[test]
    fn missing_stream_is_a_problem() {
        let bytes = compound_file(|_| {});
        let mut reader = reader(&bytes, Limits::DEFAULT);
        assert_eq!(reader.read_stream(Path::new("/a")), None);
        assert_eq!(reader.into_problems()[0].path, "/a");
    }

    #[test]
    fn skips_stream_over_size_limit() {
        let bytes = compound_file(|file| {
            write_stream(file, "/small", &[1; 100]);
            write_stream(file, "/big", &[1; 101]);
        });
        let limits = Limits {
            max_stream_len: 100,
            ..Limits::DEFAULT
        };
        let mut reader = reader(&bytes, limits);
        assert!(reader.read_stream(Path::new("/small")).is_some());
        assert_eq!(reader.read_stream(Path::new("/big")), None);
        assert_eq!(reader.into_problems().len(), 1);
    }

    #[test]
    fn stops_reading_when_budget_runs_out() {
        let bytes = compound_file(|file| write_stream(file, "/a", &[1; 1000]));
        let limits = Limits {
            max_total_read: 2500,
            ..Limits::DEFAULT
        };
        let mut reader = reader(&bytes, limits);
        assert!(reader.read_stream(Path::new("/a")).is_some());
        assert!(reader.read_stream(Path::new("/a")).is_some());
        assert_eq!(reader.read_stream(Path::new("/a")), None);
        assert_eq!(reader.into_problems().len(), 1);
    }

    #[test]
    fn empty_streams_cost_budget_too() {
        let bytes = compound_file(|file| write_stream(file, "/empty", b""));
        let limits = Limits {
            max_total_read: 10 * MIN_STREAM_COST,
            ..Limits::DEFAULT
        };
        let mut reader = reader(&bytes, limits);
        let read = (0..100)
            .filter(|_| reader.read_stream(Path::new("/empty")).is_some())
            .count();
        assert_eq!(read, 10);
    }

    #[test]
    fn keeps_a_bounded_number_of_problems() {
        let bytes = compound_file(|_| {});
        let mut reader = reader(&bytes, Limits::DEFAULT);
        for _ in 0..MAX_PROBLEMS * 2 {
            reader.problem(Path::new("/"), "bad");
        }
        assert_eq!(reader.into_problems().len(), MAX_PROBLEMS);
    }

    #[test]
    fn lists_child_storages_in_number_order_up_to_limit() {
        let bytes = compound_file(|file| {
            for i in [0x10, 2, 0, 0xA, 1] {
                file.create_storage(format!("/__recip_version1.0_#{i:08x}"))
                    .unwrap();
            }
            file.create_storage("/__recip_version1.0_#0000000").unwrap();
            file.create_storage("/__attach_version1.0_#00000000")
                .unwrap();
            write_stream(file, "/__recip_version1.0_#00000003", b"");
        });
        let mut reader = reader(&bytes, Limits::DEFAULT);
        let recipients = reader.child_storages(Path::new("/"), "__recip_version1.0_#", 4);
        assert_eq!(
            recipients,
            [0, 1, 2, 0xA].map(|i| PathBuf::from(format!("/__recip_version1.0_#{i:08x}")))
        );
        assert_eq!(reader.into_problems().len(), 1);
    }

    #[test]
    fn parses_storage_number() {
        let prefix = "__recip_version1.0_#";
        assert_eq!(
            storage_number("__recip_version1.0_#0000000A", prefix),
            Some(10)
        );
        assert_eq!(storage_number("__recip_version1.0_#0000000", prefix), None);
        assert_eq!(storage_number("__recip_version1.0_#+0000000", prefix), None);
        assert_eq!(
            storage_number("__attach_version1.0_#00000000", prefix),
            None
        );
    }

    #[test]
    fn embedded_limits_count_down_depth() {
        let bytes = compound_file(|_| {});
        let limits = Limits {
            max_embedded_depth: 1,
            ..Limits::DEFAULT
        };
        let mut outer = reader(&bytes, limits);
        let inner_limits = outer.embedded_limits(Path::new("/a")).unwrap();
        assert_eq!(inner_limits.max_embedded_depth, 0);
        assert!(outer.into_problems().is_empty());

        let mut inner = reader(&bytes, inner_limits);
        assert_eq!(inner.embedded_limits(Path::new("/a")), None);
        assert_eq!(inner.into_problems().len(), 1);
    }
}
