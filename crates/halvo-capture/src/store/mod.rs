//! The encrypted segment store: its traits, types and errors.

pub mod db;
pub(crate) mod durable;
pub(in crate::store) mod expiry;
#[cfg(test)]
pub(in crate::store) mod failpoint;
#[cfg(not(test))]
pub(in crate::store) mod failpoint {
    pub(crate) fn hit(_name: &'static str) -> super::StoreResult<()> {
        Ok(())
    }
}
pub(in crate::store) mod segment;
pub(in crate::store) mod verify;

#[cfg(test)]
mod formal_tests;
#[cfg(test)]
mod hardening_tests;
#[cfg(test)]
#[path = "test_support/pages.rs"]
pub(in crate::store) mod pages;
#[cfg(test)]
#[path = "test_support/roots.rs"]
pub(crate) mod roots;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use uuid::Uuid;

use crate::custody::CustodyError;
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

pub(in crate::store) const DEFAULT_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

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

    /// Crypto-shred everything and destroy the store key (delete-all). A call
    /// that stopped after its root step is finished by a second call on the
    /// same handle, which destroys the root, the key file, the database files
    /// and the segments directory again where they are left, and returns an
    /// empty report. Once that handle is gone, no call finishes it while the
    /// key file is left: `open` answers [`StoreError::Custody`] with
    /// [`CustodyError::RootMissing`] for a whole key file and
    /// [`StoreError::KeyFileUnparseable`] for one that the stop left zeroed,
    /// and `create` answers [`StoreError::AlreadyExists`]. Once the key file is
    /// gone, `open` answers [`StoreError::Sealed`], and `create` removes what is
    /// left and makes a new store. It opens `store.key` by name to zero it,
    /// without `O_NONBLOCK`, so a FIFO there makes it wait for good.
    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport>;
}

/// The result of a crypto-shred pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShredReport {
    pub segments_shredded: u64,
    pub keys_destroyed: u64,
}

/// Why a store call failed. A custody failure, also one of `open`'s unwrap,
/// arrives as [`StoreError::Custody`] with its own variant.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store i/o failed: {0}")]
    Io(std::io::ErrorKind),
    /// SQLite returned this extended result code; a database that cannot grow
    /// returns `SQLITE_FULL`.
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
    #[error("the store holds corrupt or tampered data: {0}")]
    Corrupt(String),
    #[error("another writer holds the store at {0}")]
    Locked(PathBuf),
    #[error("a store already exists at {0}")]
    AlreadyExists(PathBuf),
    /// store.key is empty, zeroed or does not parse.
    #[error("the store-key file does not parse")]
    KeyFileUnparseable,
    /// store.db is missing or empty: `open` makes no database in its place.
    #[error("the store database is missing or empty")]
    DatabaseMissing,
    /// The store key that store.key unwraps to does not open store.db: a key
    /// of another store, a damaged page or a cut file.
    #[error("the store key does not open the store database")]
    DatabaseUnreadable,
    /// store.key or store.db has a format version above the one this build
    /// reads. The refused open writes to no file of the store but writer.lock,
    /// which it makes when it is missing, and it narrows the directory to its
    /// owner.
    #[error(
        "the store has format version {found}, above version {supported} that this build reads"
    )]
    NewerStore { found: i64, supported: i64 },
    /// store.key or store.db has a format version below the one this build
    /// reads. No migration exists. The refused open writes to no file of the
    /// store but writer.lock, which it makes when it is missing, and it narrows
    /// the directory to its owner.
    #[error(
        "the store has format version {found}, below version {supported} that this build reads"
    )]
    OlderStore { found: i64, supported: i64 },
    /// A segment whose key is live has no file.
    #[error("a segment whose key is live has no file")]
    SegmentMissing(Uuid),
    /// A setting that the store requires does not read back.
    #[error("the store database does not have {0} in effect")]
    SettingNotInEffect(&'static str),
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
