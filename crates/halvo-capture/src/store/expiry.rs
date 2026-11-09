//! Window-expiry crypto-shred.
//!
//! Deletion is key destruction, never an overwrite: modern flash cannot be
//! securely overwritten, so the data key is what gets sanitized (researcher-2
//! findings 24, 44; NIST SP 800-88r2 cryptographic erase). Destroying a segment's
//! per-segment key makes its ciphertext file unrecoverable with any remaining key.
//! With `secure_delete` ON, SQLite also zero-fills the freed wrapped-key bytes, and
//! FTS5 secure-delete removes the text-index entries, so no residue is left in the
//! database either (findings 36, 37).
//!
//! The one store writer runs this, so there is never a second writer.

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
    /// Segment ids whose key still exists and whose expiry has passed.
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

    /// Destroy one segment: delete its events and FTS rows, unlink its file, and
    /// NULL (zero-fill) its wrapped per-segment key. The plaintext key never
    /// touched disk, so after this no key can decrypt the segment.
    fn shred_segment(&mut self, segment_id_simple: &str) -> StoreResult<()> {
        let conn = self.db().connection();
        // Delete the text-index entries for this segment's events first.
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
        // Destroy the key: secure_delete zero-fills the old blob bytes.
        conn.execute(
            "UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = ?1",
            [segment_id_simple],
        )
        .map_err(|e| StoreError::Io(e.to_string()))?;

        // Unlink the ciphertext file (freeing disk; it is already unrecoverable).
        if let Ok(id) = Uuid::parse_str(segment_id_simple) {
            let path = segment::segment_path(self.dir(), id);
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }

    /// Checkpoint the WAL to zero bytes so no plaintext-logical residue remains in
    /// the WAL after a shred (finding 36).
    fn checkpoint_truncate(&self) -> StoreResult<()> {
        self.db()
            .connection()
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(|e| StoreError::Io(e.to_string()))
    }

    /// Whether the store key has been destroyed (delete-all ran). A sealed store
    /// has no key file.
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
        // Shred every segment regardless of expiry.
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

        // Destroy the store key: delete the wrapped key file and the platform root,
        // so any retained ciphertext is unrecoverable (Apple effaceable-key
        // pattern, NIST purge; researcher-2 findings 24, 44).
        let key_file = KeyFile::path(self.dir());
        let _ = std::fs::remove_file(&key_file);
        self.destroy_root()
            .map_err(|e| StoreError::Io(format!("destroy root: {e}")))?;

        Ok(ShredReport {
            segments_shredded: shredded,
            keys_destroyed: shredded + 1, // + the store key
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custody::dev_fallback::DevFallbackCustody;
    use crate::custody::CustodyError;
    use crate::store::segment::{decrypt_segment_bytes, segment_path};
    use crate::store::SegmentStore;
    use std::path::Path;

    fn make_store(dir: &Path) -> EncryptedStore {
        let root_id = format!("halvo-test-{}", uuid::Uuid::now_v7());
        let custody = DevFallbackCustody::new(root_id.clone());
        #[cfg(target_os = "macos")]
        let destroyer = {
            let c = DevFallbackCustody::new(root_id.clone());
            Box::new(move || c.destroy_root()) as Box<dyn Fn() -> Result<(), CustodyError> + Send>
        };
        #[cfg(not(target_os = "macos"))]
        let destroyer = Box::new(|| Ok(())) as Box<dyn Fn() -> Result<(), CustodyError> + Send>;
        EncryptedStore::create(dir, &custody, &root_id, destroyer).unwrap()
    }

    #[test]
    fn expired_segment_is_not_decryptable_after_shred() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        // Force-expire: created and expires both in the past.
        let (seg_ref, _key_id) = store
            .write_segment_with_expiry(b"PLAINTEXT-SECRET", 0, 1)
            .unwrap();
        // Retain a copy of the ciphertext file before shredding.
        let cipher = std::fs::read(segment_path(dir.path(), seg_ref.segment_id)).unwrap();

        let report = store.crypto_shred_expired().unwrap();
        assert_eq!(report.segments_shredded, 1);

        // The file is gone and the retained ciphertext decrypts with no key: the
        // store key never encrypts a segment, and the per-segment key is destroyed.
        assert!(!segment_path(dir.path(), seg_ref.segment_id).exists());
        assert!(decrypt_segment_bytes(&cipher, store.store_key_bytes()).is_err());
        // The wrapped key row is now NULL.
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
        store.write_segment(b"a").unwrap();
        store.write_segment(b"b").unwrap();
        let report = store.crypto_shred_all().unwrap();
        assert_eq!(report.segments_shredded, 2);
        assert!(store.is_sealed());
    }
}
