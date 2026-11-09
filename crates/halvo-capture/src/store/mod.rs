//! The encrypted segment store: its traits, types and errors.

pub mod db;
pub mod expiry;
pub mod segment;
pub mod verify;

#[cfg(test)]
mod formal_tests;
#[cfg(test)]
mod hardening_tests;

use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use crate::custody::CustodyError;
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

pub const DEFAULT_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// The sole-writer store the capture daemon appends to. One writer only; the
/// daemon holds it behind the single-writer sink.
pub trait SegmentStore {
    /// Append one event's metadata row.
    fn append_event(&mut self, event: &CaptureStreamEvent) -> StoreResult<()>;

    /// Write raw capture bytes as a new encrypted segment and return its ref and
    /// the id of the per-segment key that encrypts it.
    fn write_segment(&mut self, bytes: &mut dyn Read) -> StoreResult<(SegmentRef, SegmentKeyId)>;

    /// Total bytes the store occupies on disk, for the cost bound.
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

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store i/o failed: {0}")]
    Io(String),
    #[error("the store is sealed; its key is not available")]
    Sealed,
    #[error("the store holds corrupt or tampered data: {0}")]
    Corrupt(String),
    #[error("another writer holds the store at {0}")]
    Locked(PathBuf),
    #[error("a store already exists at {0}")]
    AlreadyExists(PathBuf),
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
