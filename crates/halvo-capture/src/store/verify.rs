//! Store and deletion verification the gate drives through `--verify-store` and
//! `--verify-deletion`. Phase P2 replaces these bodies with the real checks
//! (marker scan, second-account read, owner unwrap, crypto-shred), keeping these
//! signatures so the bin and the gate are unchanged. Until then they report that
//! the check is not implemented, so the gate FAILs the store and deletion rows.

use super::{StoreError, StoreResult};

/// On success, returns the report text the gate greps (it must contain
/// `unwrap=ok prompt=none` and leave no plaintext marker in the store).
pub fn run_store_check() -> StoreResult<String> {
    Err(StoreError::NotImplemented("verify-store"))
}

/// On success, returns the report text the gate greps (it must contain
/// `decryptable=no` for the force-expired segment).
pub fn run_deletion_check() -> StoreResult<String> {
    Err(StoreError::NotImplemented("verify-deletion"))
}
