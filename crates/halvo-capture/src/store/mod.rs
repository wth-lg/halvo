//! The encrypted segment store: its traits, types and errors.

pub mod db;
pub(crate) mod durable;
pub mod expiry;
#[cfg(test)]
pub(crate) mod failpoint;
pub(crate) mod lock;
#[cfg(not(test))]
pub(crate) mod failpoint {
    pub(crate) fn hit(_name: &'static str) -> super::StoreResult<()> {
        Ok(())
    }
}
pub mod segment;
pub mod verify;

#[cfg(test)]
mod formal_tests;
#[cfg(test)]
mod hardening_tests;
#[cfg(test)]
#[path = "test_support/pages.rs"]
pub(crate) mod pages;
#[cfg(test)]
#[path = "test_support/roots.rs"]
pub(crate) mod roots;

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::custody::{CustodyError, RootId};
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

pub const DEFAULT_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// The store the capture daemon appends to.
pub trait SegmentStore {
    /// Append one event's metadata row and its search-index row, both or
    /// neither.
    fn append_event(&mut self, event: &CaptureStreamEvent) -> StoreResult<()>;

    /// Write raw capture bytes as a new encrypted segment and return its ref and
    /// the id of the per-segment key that encrypts it.
    fn write_segment(&mut self, bytes: &mut dyn Read) -> StoreResult<(SegmentRef, SegmentKeyId)>;

    /// The summed lengths of the files under the store directory, for the cost
    /// bound. A directory that cannot be read is an error; one that
    /// went away is skipped.
    fn size_bytes(&self) -> StoreResult<u64>;
}

/// Window-expiry crypto-shred: destroy the per-segment key for every segment past
/// its retention bound, so its ciphertext becomes unrecoverable.
pub trait SegmentExpiry {
    /// Crypto-shred every segment past its retention bound.
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport>;

    /// Crypto-shred everything and destroy the store key (delete-all), in the
    /// order of [`db::EncryptedStore::delete_all`], which it calls: `Ok` with
    /// the count of keyed segments, and that count plus the store key as the
    /// keys destroyed, when the root is gone and nothing is left (an empty
    /// report when the count could not be read or a second call finished the
    /// work); otherwise [`StoreError::DeleteIncomplete`] with the report, after
    /// every step ran. A second call on the same handle resumes.
    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport>;
}

/// The result of a crypto-shred pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShredReport {
    pub segments_shredded: u64,
    pub keys_destroyed: u64,
}

/// A name of the store, for [`StoreError::Obstructed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreName {
    StoreDirectory,
    Parent,
    KeyFile,
    Database,
    Log,
    SharedMemory,
    Journal,
    WriterLock,
    Segments,
    Shard,
    SegmentFile,
}

/// What stood at a store name in place of the store's own entry, for
/// [`StoreError::Obstructed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Met {
    Link,
    OtherKind,
    DirectoryNotAtItsName,
    DatabaseOutsideTheDirectory,
}

impl fmt::Display for StoreName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StoreName::StoreDirectory => "the store directory",
            StoreName::Parent => "a parent of the store directory",
            StoreName::KeyFile => "the store-key file",
            StoreName::Database => "the database file",
            StoreName::Log => "the write-ahead log",
            StoreName::SharedMemory => "the shared-memory file",
            StoreName::Journal => "the journal",
            StoreName::WriterLock => "the writer lock",
            StoreName::Segments => "the segments directory",
            StoreName::Shard => "a shard directory",
            StoreName::SegmentFile => "a segment file",
        })
    }
}

impl fmt::Display for Met {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Met::Link => "a link",
            Met::OtherKind => "an entry of another kind than the store keeps there",
            Met::DirectoryNotAtItsName => "a directory that is no longer at its name",
            Met::DatabaseOutsideTheDirectory => {
                "a database that SQLite holds outside the store directory"
            }
        })
    }
}

/// What a delete says of the store's root. `Destroyed`: this call, or an
/// earlier call on the same handle, removed that root's file itself, and the
/// last reading of the roots directory shows none. `Left`: a root may stand,
/// the one named when the call knows its id. `Unnamed`: nothing that the call
/// could read names a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootReading {
    Destroyed,
    Left(Option<RootId>),
    Unnamed,
}

impl RootReading {
    /// The reading as the delete's line prints it: yes, no or unknown.
    pub fn as_text(&self) -> &'static str {
        match self {
            RootReading::Destroyed => "yes",
            RootReading::Left(_) => "no",
            RootReading::Unnamed => "unknown",
        }
    }
}

/// What a delete-all, or the delete tail on a directory, did: each step was
/// attempted. `keyed_segments` is the count of keyed segments read before the
/// root step, `None` when it could not be read. `root` reads the store's own
/// root. `roots` lists the root files that stay under the roots directory
/// after the root step; `root_not_handled` names the root of a key file that
/// this build cannot destroy (a key file of another version, or of a custody
/// that keeps no root file under the roots directory). `not_followed` counts
/// the store names, the store directory's own name among them, at which the
/// delete met a link, an entry of another kind or a directory that was not
/// the handle's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    pub keyed_segments: Option<u64>,
    pub root: RootReading,
    pub roots: RootsLeft,
    pub root_not_handled: Option<RootId>,
    pub key_file_gone: bool,
    pub database_left: bool,
    pub files_left: u64,
    pub not_followed: u64,
}

pub use crate::custody::file_root::RootsLeft;

impl DeleteReport {
    /// The store's root reads destroyed.
    pub fn root_destroyed(&self) -> bool {
        self.root == RootReading::Destroyed
    }

    /// The root reads destroyed or unnamed, no root file that the step
    /// counted stays, and the delete left nothing of the store: the report of
    /// an `Ok`. A root that reads left never gives `Ok`.
    pub fn complete(&self) -> bool {
        matches!(self.root, RootReading::Destroyed | RootReading::Unnamed)
            && self.roots.none_left()
            && self.root_not_handled.is_none()
            && self.key_file_gone
            && !self.database_left
            && self.files_left == 0
    }
}

/// Why a store call failed. A custody failure, also one of `open`'s unwrap,
/// arrives as [`StoreError::Custody`] with its own variant. No Display or
/// Debug form holds a path, a file name of the account, a segment id or a
/// text that another library built.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store i/o failed: {0}")]
    Io(std::io::ErrorKind),
    /// SQLite returned this extended result code.
    #[error("the store database failed with SQLite code {0}")]
    Db(i32),
    /// Sealing a segment or a segment key failed: the random source or the
    /// cipher refused.
    #[error("sealing a segment or a segment key failed")]
    Seal,
    /// The write-ahead log checkpoint was busy or left bytes in the log.
    #[error("the checkpoint left the write-ahead log busy ({busy}) or holding {wal_bytes} bytes")]
    CheckpointIncomplete { busy: i64, wal_bytes: u64 },
    /// No store-key file is in the directory,
    /// the handle was sealed by delete-all, or the segment was shredded.
    #[error("the store is sealed; its key is not available")]
    Sealed,
    /// Damage: data that does not decode or authenticate.
    #[error("the store holds corrupt or tampered data: {0}")]
    Corrupt(String),
    /// At a store name stands something that is not the store's own entry, or
    /// the store directory does not stand at its name. The store looked
    /// behind nothing.
    #[error("the store is obstructed: at {name} stands {met}")]
    Obstructed { name: StoreName, met: Met },
    #[error("another writer holds the store")]
    Locked,
    #[error("a store already exists")]
    AlreadyExists,
    /// store.key is empty, zeroed or does not parse.
    #[error("the store-key file does not parse")]
    KeyFileUnparseable,
    /// The key file's namespace is a valid label other than the one asked for.
    #[error("the key file holds a key of another label")]
    KeyFileOfAnotherLabel,
    /// store.db is missing or empty: `open` makes no database in its place.
    #[error("the store database is missing or empty")]
    DatabaseMissing,
    /// The store key that store.key unwraps to does not open store.db: a key
    /// of another store, a damaged page or a cut file.
    #[error("the store key does not open the store database")]
    DatabaseUnreadable,
    /// store.key or store.db has a format version above the one this build
    /// reads.
    #[error(
        "the store has format version {found}, above version {supported} that this build reads"
    )]
    NewerStore { found: i64, supported: i64 },
    /// store.key or store.db has a format version below the one this build
    /// reads, among them a key file of version 1, which an older build wrote.
    /// No migration exists; the delete tail of
    /// [`db::EncryptedStore::delete_store_dir`] removes such a store.
    #[error(
        "the store has format version {found}, below version {supported} that this build reads"
    )]
    OlderStore { found: i64, supported: i64 },
    /// A segment whose key is live has no file.
    #[error("a segment whose key is live has no file")]
    SegmentMissing,
    /// A setting that the store requires does not read back.
    #[error("the store database does not have {0} in effect")]
    SettingNotInEffect(&'static str),
    /// A delete-all ran every step and left part of the store; the report
    /// says what.
    #[error("delete-all left part of the store")]
    DeleteIncomplete(Box<DeleteReport>),
    #[error(transparent)]
    Custody(#[from] CustodyError),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// Run the `--verify-store` measurements on a fresh store in the directory
/// that HALVO_STORE_DIR names, or in a temporary one, with the marker of
/// HALVO_GATE_MARKER, and return the line that the gate judges.
pub fn verify_store() -> StoreResult<String> {
    let dir = verify::StoreDir::from_env();
    measure_store(dir.path(), &verify::marker_from_env())
}

/// Run the `--verify-deletion` measurements in the directory that
/// HALVO_STORE_DIR names, or in a temporary one, and return the line that the
/// gate judges.
pub fn verify_deletion() -> StoreResult<String> {
    let dir = verify::StoreDir::from_env();
    measure_deletion(dir.path())
}

/// Run the `--verify-store` measurements on a fresh store made in `dir` that
/// holds `marker`, and return the line that the gate judges. It reads neither
/// HALVO_STORE_DIR nor HALVO_GATE_MARKER and uses no default marker.
/// Fails when a file of the store holds the raw store key.
pub fn measure_store(dir: &Path, marker: &str) -> StoreResult<String> {
    verify::run_store_check_in(dir, marker).map(|check| check.to_string())
}

/// Run the `--verify-deletion` measurements on a fresh store made in `dir`:
/// one expired segment shredded beside a live one, then delete-all. Returns
/// the line that the gate judges. It reads neither HALVO_STORE_DIR nor
/// HALVO_GATE_MARKER. Fails when the state of the root after delete-all is
/// unknown.
pub fn measure_deletion(dir: &Path) -> StoreResult<String> {
    verify::run_deletion_check_in(dir).map(|check| check.to_string())
}
