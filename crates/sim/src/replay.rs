//! The `.eonreplay` file format, its writer and reader, and `verify`.
//! Specification: `docs/DETERMINISM.md`, "Replays".
//!
//! # Layout
//!
//! ```text
//! "EONR"                      4 magic bytes            (MAGIC)
//! u16 little-endian           format version, 1       (FORMAT_VERSION)
//! postcard(MatchSetup)        header
//! postcard(Record)*           stream, no framing
//! ```
//!
//! Records are [`Record::Tick`] for every tick that had commands (the
//! commands handed to `Sim::step` for that tick, before the command delay)
//! and [`Record::Hash`] after every `hash_every` ticks (20 by default, 1 under
//! `--hash-every-tick`) plus one final hash at clean exit. Records are
//! written in tick order; a tick's `Tick` record precedes its `Hash` record.
//!
//! The reader tolerates truncation (`kill -9`): it decodes records with
//! `postcard::take_from_bytes` and stops at the first record that does not
//! decode, keeping everything before it. Postcard encodings are
//! self-delimiting and prefix-free per type, so a cut-off tail never decodes
//! as a shorter valid record.
//!
//! # Writer
//!
//! [`ReplayWriter`] wraps a `BufWriter<File>`. `tick` and `hash` append
//! records; `flush` writes the buffer (the game calls it every 20 ticks);
//! `finish` flushes and `sync_all`s once at clean exit. No fsync during play
//! (Apple `F_FULLFSYNC` cost). The dedicated writer thread and channel live
//! in the game crate (M2); this type is the sink.
//!
//! # Verify
//!
//! [`verify`] reads the file, fails fast on `sim_version` mismatch, loads
//! the rules from `data_dir` and compares `rules_hash`, then re-simulates:
//! for each tick from 0, `step` with that tick's recorded commands (none
//! when no `Tick` record exists), and at every `Hash` record compares the
//! whole hash; on mismatch the first differing field of
//! [`SubHashes`] names the subsystem. The result is exactly one
//! [`VerifyOutcome`], whose `Display` is the string the CLI prints.

use crate::ai_hook::AiController;
use crate::command::PlayerCommand;
use crate::state::{MatchSetup, SubHashes};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

/// File magic.
pub const MAGIC: [u8; 4] = *b"EONR";
/// Format version written by this build.
pub const FORMAT_VERSION: u16 = 1;
/// Default interval between hash records, in ticks.
pub const DEFAULT_HASH_EVERY: u32 = 20;

/// The decoded fixed-size prefix plus header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayHeader {
    /// Always [`FORMAT_VERSION`] for files this build writes.
    pub format_version: u16,
    /// Everything needed to reproduce the match from tick 0.
    pub setup: MatchSetup,
}

/// The commands handed to `Sim::step` on one tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickBatch {
    /// Tick the commands were issued on (the `step` call index).
    pub tick: u32,
    /// Commands in the order they were passed to `step`.
    pub cmds: Vec<PlayerCommand>,
}

/// The state hash after `tick` completed steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashRecord {
    /// `Sim::tick()` when the hash was taken.
    pub tick: u32,
    /// `Sim::hash()`.
    pub hash: u64,
    /// `Sim::sub_hashes()`, so `verify` can name the diverging subsystem.
    pub sub: SubHashes,
}

/// One record of the stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Record {
    /// Commands for a tick.
    Tick(TickBatch),
    /// A hash checkpoint.
    Hash(HashRecord),
}

/// Errors reading or writing a replay. Sim divergence is not an error; it
/// is a [`VerifyOutcome`].
#[derive(Debug)]
pub enum ReplayError {
    /// File system failure.
    Io {
        /// File involved.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The file does not start with [`MAGIC`].
    BadMagic {
        /// File involved.
        path: PathBuf,
    },
    /// The format version is not one this build reads.
    UnsupportedVersion {
        /// File involved.
        path: PathBuf,
        /// Version found in the file.
        found: u16,
    },
    /// The header did not decode (the file is shorter than a header or corrupt).
    Header {
        /// File involved.
        path: PathBuf,
        /// Decoder error.
        source: postcard::Error,
    },
    /// A record could not be encoded (writer side; should not happen).
    Encode(postcard::Error),
    /// The rules under `data_dir` did not load.
    Rules(rules::Error),
}

impl core::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReplayError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            ReplayError::BadMagic { path } => {
                write!(f, "{}: not an .eonreplay file (bad magic)", path.display())
            }
            ReplayError::UnsupportedVersion { path, found } => write!(
                f,
                "{}: unsupported replay format version {found} (this build reads {FORMAT_VERSION})",
                path.display()
            ),
            ReplayError::Header { path, source } => {
                write!(
                    f,
                    "{}: replay header does not decode: {source}",
                    path.display()
                )
            }
            ReplayError::Encode(e) => write!(f, "replay record does not encode: {e}"),
            ReplayError::Rules(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ReplayError {}

impl From<rules::Error> for ReplayError {
    fn from(e: rules::Error) -> ReplayError {
        ReplayError::Rules(e)
    }
}

/// Appends records to an `.eonreplay` file. See the module docs.
#[derive(Debug)]
pub struct ReplayWriter {
    path: PathBuf,
    out: BufWriter<File>,
    records: u64,
}

impl ReplayWriter {
    /// Create (truncate) `path` and write the magic, version and header.
    pub fn create(path: &Path, setup: &MatchSetup) -> Result<ReplayWriter, ReplayError> {
        let _ = (path, setup);
        todo!("M1 replay: ReplayWriter::create (implementer C)")
    }

    /// Append a [`Record::Tick`]. A no-op when `cmds` is empty.
    pub fn tick(&mut self, tick: u32, cmds: &[PlayerCommand]) -> Result<(), ReplayError> {
        let _ = (tick, cmds);
        todo!("M1 replay: ReplayWriter::tick (implementer C)")
    }

    /// Append a [`Record::Hash`].
    pub fn hash(&mut self, tick: u32, hash: u64, sub: &SubHashes) -> Result<(), ReplayError> {
        let _ = (tick, hash, sub);
        todo!("M1 replay: ReplayWriter::hash (implementer C)")
    }

    /// Write buffered bytes to the OS (no fsync).
    pub fn flush(&mut self) -> Result<(), ReplayError> {
        use std::io::Write;
        self.out.flush().map_err(|source| ReplayError::Io {
            path: self.path.clone(),
            source,
        })
    }

    /// Flush and `sync_all`; consumes the writer. Call once at clean exit.
    pub fn finish(self) -> Result<(), ReplayError> {
        todo!("M1 replay: ReplayWriter::finish (implementer C)")
    }

    /// File being written.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Records appended so far.
    pub fn records(&self) -> u64 {
        self.records
    }
}

/// Reads an `.eonreplay` file, tolerating a truncated tail.
#[derive(Debug)]
pub struct ReplayReader;

impl ReplayReader {
    /// Read the whole file: header (magic and version checked) and every
    /// complete record. A truncated final record is dropped silently.
    pub fn open(path: &Path) -> Result<(MatchSetup, Vec<Record>), ReplayError> {
        let _ = path;
        todo!("M1 replay: ReplayReader::open (implementer C)")
    }

    /// Decode from bytes already in memory (the body of `open`; tests use it).
    pub fn decode(path: &Path, bytes: &[u8]) -> Result<(MatchSetup, Vec<Record>), ReplayError> {
        let _ = (path, bytes);
        todo!("M1 replay: ReplayReader::decode (implementer C)")
    }
}

/// Result of [`verify`]. `Display` yields exactly the line the CLI prints;
/// [`VerifyOutcome::exit_code`] is the process exit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Every recorded hash was reproduced.
    Ok {
        /// Hash at the last hash record.
        final_hash: u64,
        /// Ticks re-simulated (the tick of the last hash record).
        ticks: u32,
    },
    /// A recorded hash did not match.
    Diverged {
        /// Tick of the first mismatching hash record.
        tick: u32,
        /// Name of the first differing sub-hash (see [`SubHashes::first_difference`]).
        subsystem: &'static str,
    },
    /// `header.sim_version != SIM_VERSION`; nothing was simulated.
    SimVersionMismatch {
        /// Version in the replay.
        replay: u32,
        /// `crate::SIM_VERSION`.
        binary: u32,
    },
    /// `header.rules_hash != Rules::load(data_dir).rules_hash()`.
    RulesChanged {
        /// Hash in the replay.
        replay: u64,
        /// Hash of the loaded rules.
        loaded: u64,
    },
}

impl VerifyOutcome {
    /// Process exit status: 0 for `Ok`, 1 otherwise.
    pub fn exit_code(&self) -> u8 {
        match self {
            VerifyOutcome::Ok { .. } => 0,
            _ => 1,
        }
    }
}

impl core::fmt::Display for VerifyOutcome {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VerifyOutcome::Ok { final_hash, ticks } => {
                write!(f, "OK final_hash=0x{final_hash:016x} ticks={ticks}")
            }
            VerifyOutcome::Diverged { tick, subsystem } => {
                write!(f, "DIVERGED at tick {tick} (subsystem: {subsystem})")
            }
            VerifyOutcome::SimVersionMismatch { replay, binary } => {
                write!(f, "SIM VERSION MISMATCH (replay {replay}, binary {binary})")
            }
            VerifyOutcome::RulesChanged { replay, loaded } => write!(
                f,
                "RULES CHANGED since recording (replay 0x{replay:016x}, loaded 0x{loaded:016x})"
            ),
        }
    }
}

/// Re-simulate `path` against the rules under `data_dir` (see the module
/// docs). `ai` drives every slot with `is_ai == true` exactly as during the
/// recording; scenario recordings have no AI slots and pass a no-op bot.
pub fn verify(
    path: &Path,
    data_dir: &Path,
    ai: Box<dyn AiController>,
) -> Result<VerifyOutcome, ReplayError> {
    let _ = (path, data_dir, ai);
    todo!("M1 replay: verify (implementer C)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_outcome_strings_and_exit_codes_are_exact() {
        let ok = VerifyOutcome::Ok {
            final_hash: 0x0123_4567_89ab_cdef,
            ticks: 1200,
        };
        assert_eq!(
            ok.to_string(),
            "OK final_hash=0x0123456789abcdef ticks=1200"
        );
        assert_eq!(ok.exit_code(), 0);
        let d = VerifyOutcome::Diverged {
            tick: 40,
            subsystem: "units",
        };
        assert_eq!(d.to_string(), "DIVERGED at tick 40 (subsystem: units)");
        assert_eq!(d.exit_code(), 1);
        let v = VerifyOutcome::SimVersionMismatch {
            replay: 1,
            binary: 2,
        };
        assert_eq!(v.to_string(), "SIM VERSION MISMATCH (replay 1, binary 2)");
        assert_eq!(v.exit_code(), 1);
        let r = VerifyOutcome::RulesChanged {
            replay: 1,
            loaded: 2,
        };
        assert_eq!(
            r.to_string(),
            "RULES CHANGED since recording (replay 0x0000000000000001, loaded 0x0000000000000002)"
        );
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn records_round_trip_through_postcard_and_take_from_bytes() {
        let a = Record::Tick(TickBatch {
            tick: 3,
            cmds: vec![PlayerCommand::new(
                crate::ids::PlayerId(0),
                1,
                crate::command::Command::Surrender,
            )],
        });
        let b = Record::Hash(HashRecord {
            tick: 20,
            hash: 7,
            sub: SubHashes::default(),
        });
        let mut bytes = postcard::to_allocvec(&a).unwrap();
        bytes.extend(postcard::to_allocvec(&b).unwrap());
        let (ra, rest): (Record, &[u8]) = postcard::take_from_bytes(&bytes).unwrap();
        let (rb, rest): (Record, &[u8]) = postcard::take_from_bytes(rest).unwrap();
        assert_eq!(ra, a);
        assert_eq!(rb, b);
        assert!(rest.is_empty());
        // A truncated tail does not decode.
        let cut = &bytes[..bytes.len() - 3];
        let (_, rest): (Record, &[u8]) = postcard::take_from_bytes(cut).unwrap();
        assert!(postcard::take_from_bytes::<Record>(rest).is_err());
    }
}
