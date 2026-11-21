//! The listing of the store: the per-day rollup, read as it stands.

use super::db::{db_error, EncryptedStore};
use super::{InventoryRow, StoreError, StoreResult, Usage, UtcDay};

impl EncryptedStore {
    /// The store's byte totals on disk and in its payloads, as [`Usage`]
    /// defines them. Rows of shredded segments stay in the database, so its
    /// pages count after every live segment is gone.
    pub fn usage(&self) -> StoreResult<Usage> {
        let db = self.db()?;
        let (segment_disk, logical): (i64, i64) = db
            .connection()
            .query_row(
                "SELECT coalesce(sum(disk_bytes), 0), coalesce(sum(logical_bytes), 0) \
                 FROM segment_keys WHERE wrapped_key IS NOT NULL",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_error)?;
        Ok(Usage {
            disk_bytes: (segment_disk.max(0) as u64)
                .saturating_add(db.live_page_bytes()?)
                .saturating_add(db.wal_bytes()?),
            logical_bytes: logical.max(0) as u64,
        })
    }

    /// The rows of the per-day rollup whose count is above 0, by UTC day and
    /// kind text. A batch write adds its totals in its transaction and a
    /// shred takes them out in its own, so the rows are the records and bytes
    /// of the live segments; no query groups segments or records. A rollup row
    /// at 0 stays in the table and is left out here. A sealed store lists
    /// nothing.
    pub fn inventory(&self) -> StoreResult<Vec<InventoryRow>> {
        let db = match self.db() {
            Ok(db) => db,
            Err(StoreError::Sealed) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut rows = db
            .connection()
            .prepare(
                "SELECT day, kind, events, bytes FROM daily_totals WHERE events > 0 \
                 ORDER BY day, kind",
            )
            .map_err(db_error)?;
        let read = rows
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(db_error)?;
        read.map(|row| {
            let (day, kind, events, bytes) = row.map_err(db_error)?;
            Ok(InventoryRow {
                day: UtcDay::from_days_since_epoch(day),
                kind,
                events: events.max(0) as u64,
                bytes: bytes.max(0) as u64,
            })
        })
        .collect()
    }
}
