//! The encrypted segment store: its traits, types and errors.

pub mod db;
pub mod expiry;
pub mod segment;
pub mod verify;

#[cfg(test)]
mod formal_tests;
#[cfg(test)]
mod hardening_tests;
#[cfg(test)]
#[path = "test_support/pages.rs"]
pub(crate) mod pages;

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use uuid::Uuid;

use crate::custody::CustodyError;
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
    /// bound. A file or directory that cannot be read is an error; one that
    /// went away is skipped.
    fn size_bytes(&self) -> StoreResult<u64>;
}

/// Window-expiry crypto-shred: destroy the per-segment key for every segment past
/// its retention bound, so its ciphertext becomes unrecoverable.
pub trait SegmentExpiry {
    /// Crypto-shred every segment past its retention bound.
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport>;

    /// Crypto-shred everything and destroy the store key (delete-all).
    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport>;
}

/// The result of a crypto-shred pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShredReport {
    pub segments_shredded: u64,
    pub keys_destroyed: u64,
}

/// Why a store call failed, classified where the failure happens so that the
/// owner can match every state. A custody failure, also one of `open`'s unwrap,
/// arrives as [`StoreError::Custody`] with its own variant.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store i/o failed: {0}")]
    Io(std::io::ErrorKind),
    /// SQLite returned this extended result code; a database that cannot grow
    /// returns `SQLITE_FULL`.
    #[error("the store database failed with SQLite code {0}")]
    Db(i32),
    /// The write-ahead log checkpoint was busy or left bytes in the log, so the
    /// shred that asked for it is not complete.
    #[error("the checkpoint left the write-ahead log busy ({busy}) or holding {wal_bytes} bytes")]
    CheckpointIncomplete { busy: i64, wal_bytes: u64 },
    /// No store-key file is in the directory, the store key does not open the
    /// database, the handle was sealed by delete-all, or the segment was shredded.
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
    /// A segment whose key is live has no file.
    #[error("a segment whose key is live has no file")]
    SegmentMissing(Uuid),
    /// A setting that the store requires does not read back from the database.
    #[error("the store database does not have {0} in effect")]
    SettingNotInEffect(&'static str),
    #[error(transparent)]
    Custody(#[from] CustodyError),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// `--verify-store`: no plaintext leaked and the owner unwrapped the store key
/// with no prompt.
pub fn verify_store() -> StoreResult<String> {
    verify::run_store_check().map(|check| check.to_string())
}

/// `--verify-deletion`: a crypto-shredded segment is not decryptable with any
/// remaining key.
pub fn verify_deletion() -> StoreResult<String> {
    verify::run_deletion_check().map(|check| check.to_string())
}
