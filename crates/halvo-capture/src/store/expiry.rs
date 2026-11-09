//! Window-expiry crypto-shred.

use std::io::ErrorKind;
use std::path::Path;

use rusqlite::{Connection, Row};
use uuid::Uuid;

use super::db::{unix_secs_now, EncryptedStore, KeyFile};
use super::segment;
use super::{SegmentExpiry, ShredReport, StoreError, StoreResult};

const STORE_KEY_COUNT: u64 = 1;

fn db_error(e: rusqlite::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

fn segment_id_column(row: &Row<'_>) -> rusqlite::Result<String> {
    row.get(0)
}

fn parse_segment_id(text: &str) -> StoreResult<Uuid> {
    Uuid::parse_str(text).map_err(|e| {
        StoreError::Io(format!(
            "segment_keys holds a corrupt segment id {text}: {e}"
        ))
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

fn remove_segment_file(store_dir: &Path, segment_id: Uuid) -> StoreResult<()> {
    match std::fs::remove_file(segment::segment_path(store_dir, segment_id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(StoreError::Io(format!(
            "removing segment file {}: {e}",
            segment_id.simple()
        ))),
    }
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

    fn shred_segment(&mut self, segment_id: Uuid) -> StoreResult<()> {
        let conn = self.db()?.connection();
        let id = segment_id.simple().to_string();
        let tx = conn.unchecked_transaction().map_err(db_error)?;
        delete_events_and_index(&tx, &id)?;
        tx.execute(
            "UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = ?1",
            [&id],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        remove_segment_file(self.dir(), segment_id)
    }

    fn shred_segments(&mut self, ids: &[Uuid]) -> StoreResult<u64> {
        for id in ids {
            self.shred_segment(*id)?;
        }
        self.db()?.checkpoint_truncate()?;
        Ok(ids.len() as u64)
    }

    fn destroy_store_key(&mut self) -> StoreResult<()> {
        std::fs::remove_file(KeyFile::path(self.dir()))
            .map_err(|e| StoreError::Io(format!("removing the store-key file: {e}")))?;
        self.seal();
        self.destroy_root()?;
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
        let ids = self.segment_ids_with_keys(None)?;
        let segments_shredded = self.shred_segments(&ids)?;
        self.destroy_store_key()?;
        Ok(ShredReport {
            segments_shredded,
            keys_destroyed: segments_shredded + STORE_KEY_COUNT,
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
            EncryptedStore::open(dir.path(), DEFAULT_RETENTION),
            Err(StoreError::Sealed)
        ));
    }
}
