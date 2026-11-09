//! Store and deletion verification the gate drives through `--verify-store` and
//! `--verify-deletion`.

use std::fmt;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::SystemTime;

use uuid::Uuid;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{KeyCustody, RootId};
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence};

use super::db::EncryptedStore;
use super::segment::{self, segment_path};
use super::{SegmentExpiry, SegmentStore, StoreError, StoreResult, DEFAULT_RETENTION};

pub const MARKER_ROWS: u64 = 2000;
pub const OWNER_UNWRAP_NO_PROMPT: &str = "unwrap=ok prompt=none";
pub const DECRYPTABLE: &str = "decryptable";

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn db_error(e: rusqlite::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

fn store_dir() -> PathBuf {
    match std::env::var("HALVO_STORE_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => std::env::temp_dir().join(format!("halvo-verify-{}", Uuid::now_v7())),
    }
}

fn at(seq: u64) -> CapturedAt {
    CapturedAt {
        wall_clock: SystemTime::now(),
        sequence: MonotonicSequence::new(seq),
    }
}

/// What `--verify-store` measured on a fresh store holding [`MARKER_ROWS`] marker
/// rows and one marker segment, reopened through the owner's custody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreCheck {
    pub rows: i64,
    pub secure_delete: i64,
    pub temp_store: i64,
    pub fts_secure_delete: i64,
    pub cipher_provider: String,
    pub segment_roundtrip: bool,
}

impl fmt::Display for StoreCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "store rows={} secure_delete={} temp_store={} fts_secure_delete={} \
             cipher_provider={} segment_roundtrip={} {OWNER_UNWRAP_NO_PROMPT}",
            self.rows,
            self.secure_delete,
            self.temp_store,
            self.fts_secure_delete,
            self.cipher_provider,
            if self.segment_roundtrip { "ok" } else { "fail" },
        )
    }
}

/// Implements `--verify-store`.
pub fn run_store_check() -> StoreResult<StoreCheck> {
    let dir = store_dir();
    let marker =
        std::env::var("HALVO_GATE_MARKER").unwrap_or_else(|_| "HALVO_DEFAULT_MARKER".to_string());
    let root_id = RootId::fresh();

    let segment_id = {
        let mut store = EncryptedStore::create(
            &dir,
            Box::new(DevFallbackCustody::new(root_id)),
            DEFAULT_RETENTION,
        )?;
        let seg_bytes = format!("{marker} embedded in captured segment bytes");
        let (seg_ref, key_id) = store.write_segment(&mut Cursor::new(seg_bytes))?;

        let tx = store
            .db()?
            .connection()
            .unchecked_transaction()
            .map_err(db_error)?;
        for i in 0..MARKER_ROWS {
            let event = CaptureStreamEvent::new_raw(
                at(i),
                Modality::ForegroundApp,
                marker.clone(),
                seg_ref.clone(),
                key_id,
            );
            store.db()?.insert_event(&event)?;
        }
        tx.commit().map_err(db_error)?;
        store.db()?.checkpoint_truncate()?;
        seg_ref.segment_id
    };

    let check = {
        let reopened = EncryptedStore::open(&dir, DEFAULT_RETENTION)?;
        let db = reopened.db()?;
        let rows: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .map_err(db_error)?;
        let mut seg_plain = Vec::new();
        reopened.read_segment(segment_id, &mut seg_plain)?;
        StoreCheck {
            rows,
            secure_delete: db.pragma_int("secure_delete")?,
            temp_store: db.pragma_int("temp_store")?,
            fts_secure_delete: db.fts_secure_delete()?,
            cipher_provider: db.cipher_provider()?,
            segment_roundtrip: String::from_utf8_lossy(&seg_plain).contains(&marker),
        }
    };

    DevFallbackCustody::new(root_id).destroy_root()?;
    Ok(check)
}

/// What `--verify-deletion` measured: one force-expired segment crypto-shredded
/// beside a live sibling, then delete-all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionCheck {
    pub segment_id: Uuid,
    pub file_gone: bool,
    pub decryptable_with_remaining_keys: bool,
    pub sibling_readable: bool,
    pub remaining_keys: usize,
    pub delete_all_keys: u64,
    pub store_sealed: bool,
}

impl fmt::Display for DeletionCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "segment={} shred=ok file_gone={} {DECRYPTABLE}={} sibling_readable={} \
             remaining_keys={} delete_all_keys={} store_sealed={}",
            self.segment_id.simple(),
            yes_no(self.file_gone),
            yes_no(self.decryptable_with_remaining_keys),
            yes_no(self.sibling_readable),
            self.remaining_keys,
            self.delete_all_keys,
            yes_no(self.store_sealed),
        )
    }
}

/// Implements `--verify-deletion`.
pub fn run_deletion_check() -> StoreResult<DeletionCheck> {
    let dir = store_dir();
    let mut store = EncryptedStore::create(
        &dir,
        Box::new(DevFallbackCustody::new(RootId::fresh())),
        DEFAULT_RETENTION,
    )?;

    let (expired, _) =
        store.write_segment_with_expiry(&mut Cursor::new(b"PLAINTEXT-SECRET-TO-DESTROY"), 0, 1)?;
    let (sibling, _) = store.write_segment(&mut Cursor::new(b"live sibling segment"))?;
    let segment_id = expired.segment_id;
    let cipher =
        std::fs::read(segment_path(&dir, segment_id)).map_err(|e| StoreError::Io(e.to_string()))?;

    let report = store.crypto_shred_expired()?;
    if report.segments_shredded != 1 {
        return Err(StoreError::Io(format!(
            "expected 1 shredded segment, got {}",
            report.segments_shredded
        )));
    }

    let file_gone = !segment_path(&dir, segment_id).exists();
    let remaining_keys = remaining_wrapped_keys(&store)?.len();
    let decryptable_with_remaining_keys = decryptable_with_remaining_keys(&store, &cipher)?;
    let sibling_readable = store
        .read_segment(sibling.segment_id, &mut Vec::new())
        .is_ok();

    let all = store.crypto_shred_all()?;
    drop(store);
    let store_sealed = matches!(
        EncryptedStore::open(&dir, DEFAULT_RETENTION),
        Err(StoreError::Sealed)
    );

    Ok(DeletionCheck {
        segment_id,
        file_gone,
        decryptable_with_remaining_keys,
        sibling_readable,
        remaining_keys,
        delete_all_keys: all.keys_destroyed,
        store_sealed,
    })
}

fn remaining_wrapped_keys(store: &EncryptedStore) -> StoreResult<Vec<Vec<u8>>> {
    let conn = store.db()?.connection();
    let mut stmt = conn
        .prepare("SELECT wrapped_key FROM segment_keys WHERE wrapped_key IS NOT NULL")
        .map_err(db_error)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .map_err(db_error)?;
    rows.map(|row| row.map_err(db_error)).collect()
}

fn decryptable_with_remaining_keys(store: &EncryptedStore, cipher: &[u8]) -> StoreResult<bool> {
    let store_key = store.store_key()?;
    for wrapped in remaining_wrapped_keys(store)? {
        let seg_key = segment::unwrap_segment_key(store_key.expose(), &wrapped)?;
        if segment::decrypt_segment_bytes(cipher, &seg_key).is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::test_support::make_store;

    #[test]
    fn a_shredded_segment_is_not_decryptable_with_any_remaining_key_while_its_sibling_is() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (expired, _) = store
            .write_segment_with_expiry(&mut Cursor::new(b"PLAINTEXT-SECRET-TO-DESTROY"), 0, 1)
            .unwrap();
        let (live, _) = store
            .write_segment(&mut Cursor::new(b"live sibling"))
            .unwrap();
        let expired_cipher = std::fs::read(segment_path(dir.path(), expired.segment_id)).unwrap();
        let live_cipher = std::fs::read(segment_path(dir.path(), live.segment_id)).unwrap();

        assert_eq!(store.crypto_shred_expired().unwrap().segments_shredded, 1);

        assert_eq!(remaining_wrapped_keys(&store).unwrap().len(), 1);
        assert!(!decryptable_with_remaining_keys(&store, &expired_cipher).unwrap());
        assert!(decryptable_with_remaining_keys(&store, &live_cipher).unwrap());
    }
}
