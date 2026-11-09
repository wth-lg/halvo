//! The encrypted segment store: traits and types only. The SQLCipher metadata
//! database, the per-segment files, window-expiry crypto-shred and the verify
//! paths are phase P2, added as the sibling files already declared here (db.rs,
//! segment.rs, expiry.rs, verify.rs), so P2 never edits this file.

pub mod db;
pub mod expiry;
pub mod segment;
pub mod verify;

use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

/// The sole-writer store the capture daemon appends to. One writer only; the
/// daemon holds it behind the single-writer sink.
pub trait SegmentStore {
    /// Append one event's metadata row.
    fn append_event(&mut self, event: &CaptureStreamEvent) -> StoreResult<()>;

    /// Write raw capture bytes as a new encrypted segment and return its ref and
    /// the id of the per-segment key that encrypts it.
    fn write_segment(&mut self, bytes: &[u8]) -> StoreResult<(SegmentRef, SegmentKeyId)>;

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
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// `--verify-store`: no plaintext leaked and the owner unwrapped the store key
/// with no prompt. Delegates to the verify module, which phase P2 fills.
pub fn verify_store() -> StoreResult<String> {
    verify::run_store_check()
}

/// `--verify-deletion`: a crypto-shredded segment is not decryptable with any
/// remaining key. Delegates to the verify module, which phase P2 fills.
pub fn verify_deletion() -> StoreResult<String> {
    verify::run_deletion_check()
}
