//! Window-expiry crypto-shred.

use std::collections::HashSet;

use halvo_sqlcipher::rusqlite::{self, Connection, Row};
use uuid::Uuid;

use super::db::{db_error, unix_secs_now, EncryptedStore};
use super::segment;
use super::{DeleteReport, SegmentExpiry, ShredReport, StoreError, StoreResult};

const STORE_KEY_COUNT: u64 = 1;

/// What one sweep of the keyless segment files did: the files it removed,
/// and the entries at the two walked levels of segments/ that the walk does
/// not take as files of the store (a link or a directory with a store name
/// among them, nothing below the two levels, and no live segment that the
/// sweep keeps), which it leaves in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sweep {
    pub removed: u64,
    pub not_store_files: u64,
}

fn segment_id_column(row: &Row<'_>) -> rusqlite::Result<String> {
    row.get(0)
}

fn parse_segment_id(text: &str) -> StoreResult<Uuid> {
    Uuid::parse_str(text).map_err(|_| {
        StoreError::Corrupt("segment_keys holds a segment id that does not parse".to_string())
    })
}

fn delete_events_and_index(conn: &Connection, segment_id: &str) -> StoreResult<()> {
    conn.execute(
        "DELETE FROM events_fts WHERE rowid IN \
           (SELECT rowid FROM events WHERE segment_id = ?1)",
        [segment_id],
    )
    .map_err(db_error)?;
    conn.execute("DELETE FROM events WHERE segment_id = ?1", [segment_id])
        .map_err(db_error)?;
    Ok(())
}

impl EncryptedStore {
    fn segment_ids_with_keys(&self, expired_by: Option<u64>) -> StoreResult<Vec<Uuid>> {
        let conn = self.db()?.connection();
        let mut keyed = conn
            .prepare("SELECT segment_id FROM segment_keys WHERE wrapped_key IS NOT NULL")
            .map_err(db_error)?;
        let mut expired = conn
            .prepare(
                "SELECT segment_id FROM segment_keys \
                 WHERE wrapped_key IS NOT NULL AND expires_at <= ?1",
            )
            .map_err(db_error)?;
        let rows = match expired_by {
            Some(now) => expired.query_map([now as i64], segment_id_column),
            None => keyed.query_map([], segment_id_column),
        }
        .map_err(db_error)?;
        rows.map(|row| parse_segment_id(&row.map_err(db_error)?))
            .collect()
    }

    fn destroy_segment_keys(&self, ids: &[Uuid]) -> StoreResult<()> {
        let db = self.db()?;
        let tx = db.connection().unchecked_transaction().map_err(db_error)?;
        for segment_id in ids {
            let id = segment_id.simple().to_string();
            delete_events_and_index(&tx, &id)?;
            tx.execute(
                "UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = ?1",
                [&id],
            )
            .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        db.checkpoint_truncate()?;
        Ok(())
    }

    pub(crate) fn remove_keyless_files(&self) -> StoreResult<Sweep> {
        let live: HashSet<Uuid> = self.segment_ids_with_keys(None)?.into_iter().collect();
        let swept =
            segment::sweep_segment_files(self.handle(), self.fs(), &|id| live.contains(&id))?;
        match swept.first_failure {
            Some(first) => Err(first),
            None => Ok(Sweep {
                removed: swept.removed,
                not_store_files: swept.not_store_files,
            }),
        }
    }

    fn shred_segments(&mut self, ids: &[Uuid]) -> StoreResult<u64> {
        self.destroy_segment_keys(ids)?;
        self.remove_keyless_files()?;
        Ok(ids.len() as u64)
    }

    /// Delete-all on this handle, in the order of the contract, every step
    /// attempted whatever the others answered, each through the handle and
    /// never by its kept name, following no link: (1) the count of keyed
    /// segments, unknown when it cannot be read; (2) the root step, which opens
    /// the custody's roots location once and, when that directory holds the
    /// store's root or an earlier call on this handle removed it, destroys
    /// every root file there, the store's own first, and otherwise destroys
    /// nothing and reads the root as left; (3) the handle refuses writes from here on,
    /// whatever step 2 answered; (4) the key file, removed even when its
    /// zero-fill failed; (5) the three database files and the journal; (6)
    /// the segments tree. `Ok` with the report when the root reads destroyed
    /// and nothing is left, [`StoreError::DeleteIncomplete`] with it otherwise. A
    /// second call on the same handle runs the steps again where they are
    /// left. It runs no bulk key delete, no checkpoint and no keyless sweep.
    pub fn delete_all(&mut self) -> StoreResult<DeleteReport> {
        let keyed_segments = self.db().ok().and_then(|db| db.keyed_segments());
        self.destroy_store_key()?;
        let mut report = self.deleting_report().unwrap_or_else(|| self.untouched());
        report.keyed_segments = keyed_segments;
        if report.complete() {
            self.mark_deleted();
            Ok(report)
        } else {
            Err(StoreError::DeleteIncomplete(Box::new(report)))
        }
    }

    fn destroy_store_key(&mut self) -> StoreResult<()> {
        let root_step = self.root_step();
        #[cfg(test)]
        super::failpoint::seam(super::db::SEAM_AFTER_THE_ROOT_STEP);
        self.remove_files_after(root_step);
        Ok(())
    }
}

impl SegmentExpiry for EncryptedStore {
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport> {
        let ids = self.segment_ids_with_keys(Some(unix_secs_now()))?;
        let segments_shredded = self.shred_segments(&ids)?;
        Ok(ShredReport {
            segments_shredded,
            keys_destroyed: segments_shredded,
        })
    }

    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport> {
        let report = self.delete_all()?;
        Ok(match report.keyed_segments {
            Some(segments_shredded) => ShredReport {
                segments_shredded,
                keys_destroyed: segments_shredded + STORE_KEY_COUNT,
            },
            None => ShredReport::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::test_support::make_store;
    use crate::store::segment::segment_path;
    use crate::store::{SegmentStore, DEFAULT_RETENTION};
    use std::io::Cursor;

    fn wrapped_key_row(store: &EncryptedStore, segment_id: Uuid) -> Option<Vec<u8>> {
        store
            .db()
            .unwrap()
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE segment_id=?1",
                [segment_id.simple().to_string()],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn expired_segment_is_not_decryptable_after_shred() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, _key_id) = store
            .write_segment_with_expiry(&mut Cursor::new(b"PLAINTEXT-SECRET"), 0, 1)
            .unwrap();
        let (live, _) = store
            .write_segment(&mut Cursor::new(b"still live"))
            .unwrap();

        let report = store.crypto_shred_expired().unwrap();
        assert_eq!(report.segments_shredded, 1);

        assert!(!segment_path(dir.path(), seg_ref.segment_id).exists());
        assert!(matches!(
            store.read_segment(seg_ref.segment_id, &mut Vec::new()),
            Err(StoreError::Sealed)
        ));
        assert!(wrapped_key_row(&store, seg_ref.segment_id).is_none());
        assert!(wrapped_key_row(&store, live.segment_id).is_some());
    }

    #[test]
    fn shredding_a_segment_whose_file_is_already_gone_still_destroys_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, _) = store
            .write_segment_with_expiry(&mut Cursor::new(b"orphaned"), 0, 1)
            .unwrap();
        std::fs::remove_file(segment_path(dir.path(), seg_ref.segment_id)).unwrap();

        let report = store.crypto_shred_expired().unwrap();
        assert_eq!(report.segments_shredded, 1);
        assert!(wrapped_key_row(&store, seg_ref.segment_id).is_none());
        assert_eq!(store.crypto_shred_expired().unwrap().segments_shredded, 0);
    }

    #[test]
    fn delete_all_seals_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        store.write_segment(&mut Cursor::new(b"a")).unwrap();
        store.write_segment(&mut Cursor::new(b"b")).unwrap();
        let report = store.crypto_shred_all().unwrap();
        assert_eq!(report.segments_shredded, 2);
        assert_eq!(report.keys_destroyed, 3);
        assert!(matches!(
            store.write_segment(&mut Cursor::new(b"c")),
            Err(StoreError::Sealed)
        ));
        assert!(matches!(store.db(), Err(StoreError::Sealed)));
        drop(store);
        assert!(matches!(
            crate::store::roots::open(dir.path(), DEFAULT_RETENTION),
            Err(StoreError::Sealed)
        ));
    }
}
