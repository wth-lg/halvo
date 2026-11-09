//! The store the app binds until phase P6 wires the encrypted segment store in.
//! It holds nothing, so the inventory is empty, and it refuses to pretend a
//! deletion happened: delete-all and the cap report that the store is not wired.

use halvo_capture::consent::status::{Inventory, StoreInventory, UtcDay};
use halvo_capture::store::{SegmentExpiry, ShredReport, StoreError, StoreResult};

pub struct UnwiredStore;

pub const DESCRIPTION: &str = "stand-in store: the encrypted store is wired in at phase P6";

const NOT_WIRED: &str = "the encrypted segment store (wired in at phase P6)";

impl SegmentExpiry for UnwiredStore {
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport> {
        Err(StoreError::NotImplemented(NOT_WIRED))
    }

    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport> {
        Err(StoreError::NotImplemented(NOT_WIRED))
    }
}

impl StoreInventory for UnwiredStore {
    fn inventory(&self) -> StoreResult<Inventory> {
        Ok(Inventory::default())
    }

    fn crypto_shred_before(&mut self, _day: UtcDay) -> StoreResult<ShredReport> {
        Err(StoreError::NotImplemented(NOT_WIRED))
    }
}
