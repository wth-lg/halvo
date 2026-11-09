//! Window-expiry crypto-shred.

use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use super::db::{EncryptedStore, KeyFile};
use super::segment;
use super::{SegmentExpiry, ShredReport, StoreError, StoreResult};

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl EncryptedStore {
    fn expired_segment_ids(&self, now: u64) -> StoreResult<Vec<String>> {
        let conn = self.db().connection();
        let mut stmt = conn
            .prepare(
                "SELECT segment_id FROM segment_keys \
                 WHERE wrapped_key IS NOT NULL AND expires_at <= ?1",
            )
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let rows = stmt
            .query_map([now as i64], |r| r.get::<_, String>(0))
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let mut ids = Vec::new();
        for r in rows {
            ids.push(r.map_err(|e| StoreError::Io(e.to_string()))?);
        }
        Ok(ids)
    }

    fn shred_segment(&mut self, segment_id_simple: &str) -> StoreResult<()> {
        let conn = self.db().connection();
        conn.execute(
            "DELETE FROM events_fts WHERE rowid IN \
               (SELECT rowid FROM events WHERE segment_id = ?1)",
            [segment_id_simple],
        )
        .map_err(|e| StoreError::Io(e.to_string()))?;
        conn.execute(
            "DELETE FROM events WHERE segment_id = ?1",
            [segment_id_simple],
        )
        .map_err(|e| StoreError::Io(e.to_string()))?;
        conn.execute(
            "UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = ?1",
            [segment_id_simple],
        )
        .map_err(|e| StoreError::Io(e.to_string()))?;

        if let Ok(id) = Uuid::parse_str(segment_id_simple) {
            let path = segment::segment_path(self.dir(), id);
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }

    fn checkpoint_truncate(&self) -> StoreResult<()> {
        self.db()
            .connection()
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(|e| StoreError::Io(e.to_string()))
    }

    pub(crate) fn is_sealed(&self) -> bool {
        !KeyFile::path(self.dir()).exists()
    }
}

impl SegmentExpiry for EncryptedStore {
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport> {
        let now = now_secs();
        let ids = self.expired_segment_ids(now)?;
        let mut shredded = 0u64;
        for id in &ids {
            self.shred_segment(id)?;
            shredded += 1;
        }
        self.checkpoint_truncate()?;
        Ok(ShredReport {
            segments_shredded: shredded,
            keys_destroyed: shredded,
        })
    }

    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport> {
        let conn = self.db().connection();
        let mut stmt = conn
            .prepare("SELECT segment_id FROM segment_keys WHERE wrapped_key IS NOT NULL")
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let mut ids = Vec::new();
        for r in rows {
            ids.push(r.map_err(|e| StoreError::Io(e.to_string()))?);
        }
        drop(stmt);

        let mut shredded = 0u64;
        for id in &ids {
            self.shred_segment(id)?;
            shredded += 1;
        }
        self.checkpoint_truncate()?;

        let key_file = KeyFile::path(self.dir());
        let _ = std::fs::remove_file(&key_file);
        self.destroy_root()
            .map_err(|e| StoreError::Io(format!("destroy root: {e}")))?;

        Ok(ShredReport {
            segments_shredded: shredded,
            keys_destroyed: shredded + 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::custody::RootId;
    use crate::store::segment::segment_path;
    use crate::store::SegmentStore;
    use std::path::Path;

    fn make_store(dir: &Path) -> EncryptedStore {
        let custody = DevFallbackCustody::new(RootId::fresh());
        EncryptedStore::create(dir, Box::new(custody)).unwrap()
    }

    #[test]
    fn expired_segment_is_not_decryptable_after_shred() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, _key_id) = store
            .write_segment_with_expiry(&mut std::io::Cursor::new(b"PLAINTEXT-SECRET"), 0, 1)
            .unwrap();

        let report = store.crypto_shred_expired().unwrap();
        assert_eq!(report.segments_shredded, 1);

        assert!(!segment_path(dir.path(), seg_ref.segment_id).exists());
        assert!(matches!(
            store.read_segment(seg_ref.segment_id, &mut Vec::new()),
            Err(StoreError::Sealed)
        ));
        let null: Option<Vec<u8>> = store
            .db()
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE segment_id=?1",
                [seg_ref.segment_id.simple().to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(null.is_none());
    }

    #[test]
    fn delete_all_seals_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        store
            .write_segment(&mut std::io::Cursor::new(b"a"))
            .unwrap();
        store
            .write_segment(&mut std::io::Cursor::new(b"b"))
            .unwrap();
        let report = store.crypto_shred_all().unwrap();
        assert_eq!(report.segments_shredded, 2);
        assert!(store.is_sealed());
    }
}
