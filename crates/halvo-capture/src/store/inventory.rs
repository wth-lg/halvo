//! The encrypted store's [`StoreInventory`]: the per-day, per-modality counts the
//! consent view reads and the date-cutoff crypto-shred it drives. Wiring the real
//! store to the consent controller replaces the P5 stand-in store, so the view's
//! delete-all and storage-cap eviction run against real segments.

use super::db::EncryptedStore;
use super::{ShredReport, StoreError, StoreResult};
use crate::consent::status::{Inventory, InventoryRow, StoreInventory, UtcDay};
use crate::event::Modality;

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const SECONDS_PER_DAY: i64 = 86_400;

fn db_error(e: rusqlite::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

/// Map the `events.modality` text (written as the modality's Debug form) back to
/// the closed enum. An unknown value is a corrupt row.
fn modality_from_text(text: &str) -> StoreResult<Modality> {
    Ok(match text {
        "ScreenFrame" => Modality::ScreenFrame,
        "Keystroke" => Modality::Keystroke,
        "Pointer" => Modality::Pointer,
        "ForegroundApp" => Modality::ForegroundApp,
        "WindowTitle" => Modality::WindowTitle,
        "SystemAudio" => Modality::SystemAudio,
        "Microphone" => Modality::Microphone,
        "Camera" => Modality::Camera,
        other => {
            return Err(StoreError::Corrupt(format!(
                "events.modality holds an unknown modality {other:?}"
            )))
        }
    })
}

impl StoreInventory for EncryptedStore {
    fn inventory(&self) -> StoreResult<Inventory> {
        // A sealed store (delete-all has run) holds nothing; report an empty
        // inventory rather than an error, as the consent controller refreshes
        // the inventory right after it shreds everything.
        let db = match self.db() {
            Ok(db) => db,
            Err(StoreError::Sealed) => return Ok(Inventory::default()),
            Err(e) => return Err(e),
        };
        let mut stmt = db
            .connection()
            .prepare(
                "SELECT modality, \
                        wall_clock_nanos / ?1 / ?2 AS day, \
                        COUNT(*), \
                        SUM(byte_len) \
                 FROM events \
                 GROUP BY modality, day",
            )
            .map_err(db_error)?;
        let rows = stmt
            .query_map([NANOS_PER_SECOND, SECONDS_PER_DAY], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(db_error)?;
        let mut inventory_rows = Vec::new();
        for row in rows {
            let (modality_text, day, events, bytes) = row.map_err(db_error)?;
            inventory_rows.push(InventoryRow {
                day: UtcDay::from_days_since_epoch(day),
                modality: modality_from_text(&modality_text)?,
                events: events.max(0) as u64,
                bytes: bytes.max(0) as u64,
            });
        }
        Ok(Inventory::from_rows(inventory_rows))
    }

    fn crypto_shred_before(&mut self, day: UtcDay) -> StoreResult<ShredReport> {
        let cutoff_secs = (day.days_since_epoch() * SECONDS_PER_DAY).max(0) as u64;
        self.shred_created_before(cutoff_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent::status::{ConsentController, SettingsFile, StorageCap, StoreInventory};
    use crate::consent::ConsentView;
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::custody::RootId;
    use crate::event::{CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence};
    use crate::store::segment::{segment_files, SegmentFile};
    use crate::store::{SegmentExpiry, SegmentStore, DEFAULT_RETENTION};
    use std::io::Cursor;
    use std::time::SystemTime;

    fn segment_count(dir: &std::path::Path) -> usize {
        segment_files(dir)
            .unwrap()
            .iter()
            .filter(|f| matches!(f, SegmentFile::Segment { .. }))
            .count()
    }

    fn write_event_backed_segment(store: &mut EncryptedStore, modality: Modality, seq: u64) {
        let (seg_ref, key_id) = store
            .write_segment(&mut Cursor::new(format!("payload {seq}").into_bytes()))
            .unwrap();
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: SystemTime::now(),
                sequence: MonotonicSequence::new(seq),
            },
            modality,
            "com.apple.Terminal".to_string(),
            seg_ref,
            key_id,
        );
        store.append_event(&event).unwrap();
    }

    #[test]
    fn inventory_counts_events_by_modality_and_a_sealed_store_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = RootId::fresh();
        let mut store = EncryptedStore::create(
            dir.path(),
            Box::new(DevFallbackCustody::new(root)),
            DEFAULT_RETENTION,
        )
        .unwrap();
        write_event_backed_segment(&mut store, Modality::ScreenFrame, 0);
        write_event_backed_segment(&mut store, Modality::Keystroke, 1);

        let inventory = store.inventory().unwrap();
        assert_eq!(inventory.total_events(), 2);
        assert!(inventory.total_bytes() > 0);

        // Delete-all seals the store; its inventory is then empty, not an error.
        store.crypto_shred_all().unwrap();
        assert!(store.inventory().unwrap().is_empty());
    }

    #[test]
    fn consent_delete_all_removes_real_segments_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let root = RootId::fresh();
        let mut store = EncryptedStore::create(
            dir.path(),
            Box::new(DevFallbackCustody::new(root)),
            DEFAULT_RETENTION,
        )
        .unwrap();
        for seq in 0..3 {
            write_event_backed_segment(&mut store, Modality::ScreenFrame, seq);
        }
        assert_eq!(segment_count(dir.path()), 3);

        let settings = SettingsFile::new(dir.path().join("consent.json"));
        let mut controller = ConsentController::open(Box::new(store), settings).unwrap();
        assert_eq!(controller.inventory().unwrap().total_events(), 3);

        // The real store, through the consent view, crypto-shreds everything.
        ConsentView::delete_all(&mut controller).unwrap();

        assert_eq!(segment_count(dir.path()), 0, "every segment file is gone");
        assert!(controller.inventory().unwrap().is_empty());
        // Drop the controller so the store releases its writer lock, then the
        // directory opens as sealed: the key file and custody root are gone.
        drop(controller);
        assert!(matches!(
            EncryptedStore::open(dir.path(), DEFAULT_RETENTION),
            Err(StoreError::Sealed)
        ));
    }

    #[test]
    fn a_fresh_real_store_opens_in_a_consent_controller_without_shredding() {
        // Default cap is far above a small store, so open does not evict.
        let dir = tempfile::tempdir().unwrap();
        let root = RootId::fresh();
        let mut store = EncryptedStore::create(
            dir.path(),
            Box::new(DevFallbackCustody::new(root)),
            DEFAULT_RETENTION,
        )
        .unwrap();
        write_event_backed_segment(&mut store, Modality::Pointer, 0);
        let settings = SettingsFile::new(dir.path().join("consent.json"));
        let mut controller = ConsentController::open(Box::new(store), settings).unwrap();
        assert_eq!(controller.storage_cap(), StorageCap::DEFAULT);
        assert_eq!(controller.inventory().unwrap().total_events(), 1);
        assert_eq!(segment_count(dir.path()), 1);
        // Clean up the custody root the test created.
        ConsentView::delete_all(&mut controller).unwrap();
    }
}
