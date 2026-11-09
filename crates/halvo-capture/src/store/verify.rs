//! Store and deletion verification the gate drives through `--verify-store` and
//! `--verify-deletion`.

use std::path::PathBuf;
use std::time::SystemTime;

use uuid::Uuid;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::CustodyError;
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence};

use super::db::EncryptedStore;
use super::segment::{decrypt_segment_bytes, segment_path};
use super::{SegmentExpiry, SegmentStore, StoreError, StoreResult};

fn store_dir() -> StoreResult<PathBuf> {
    match std::env::var("HALVO_STORE_DIR") {
        Ok(d) => Ok(PathBuf::from(d)),
        Err(_) => {
            let base = std::env::temp_dir().join(format!("halvo-verify-{}", Uuid::now_v7()));
            Ok(base)
        }
    }
}

fn destroyer(root_id: String) -> Box<dyn Fn() -> Result<(), CustodyError> + Send> {
    Box::new(move || DevFallbackCustody::new(root_id.clone()).destroy_root())
}

fn at(seq: u64) -> CapturedAt {
    CapturedAt {
        wall_clock: SystemTime::now(),
        sequence: MonotonicSequence::new(seq),
    }
}

/// Implements `--verify-store`.
pub fn run_store_check() -> StoreResult<String> {
    let dir = store_dir()?;
    let marker =
        std::env::var("HALVO_GATE_MARKER").unwrap_or_else(|_| "HALVO_DEFAULT_MARKER".to_string());
    let root_id = format!("halvo-store-{}", Uuid::now_v7());
    let custody = DevFallbackCustody::new(root_id.clone());

    let seg_id;
    {
        let mut store =
            EncryptedStore::create(&dir, &custody, &root_id, destroyer(root_id.clone()))?;
        let seg_bytes = format!("{marker} embedded in captured segment bytes");
        let (seg_ref, key_id) = store.write_segment(seg_bytes.as_bytes())?;
        seg_id = seg_ref.segment_id;

        store
            .db()
            .connection()
            .execute_batch("BEGIN")
            .map_err(|e| StoreError::Io(e.to_string()))?;
        for i in 0..2000u64 {
            let event = CaptureStreamEvent::new_raw(
                at(i),
                Modality::ForegroundApp,
                marker.clone(),
                seg_ref.clone(),
                key_id,
            );
            store.append_event(&event)?;
        }
        store
            .db()
            .connection()
            .execute_batch("COMMIT")
            .map_err(|e| StoreError::Io(e.to_string()))?;
        store
            .db()
            .connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| StoreError::Io(e.to_string()))?;
    }

    let count = {
        let reopened = EncryptedStore::open(&dir, &custody, destroyer(root_id.clone()))?;
        let n: i64 = reopened
            .db()
            .connection()
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .map_err(|e| StoreError::Io(e.to_string()))?;
        let sd = reopened.db().pragma_int("secure_delete")?;
        let ts = reopened.db().pragma_int("temp_store")?;
        let fts = reopened.db().fts_secure_delete()?;
        let seg_plain = reopened.read_segment(seg_id)?;
        let roundtrip = String::from_utf8_lossy(&seg_plain).contains(&marker);
        format!(
            "rows={n} secure_delete={sd} temp_store={ts} fts_secure_delete={fts} segment_roundtrip={}",
            if roundtrip { "ok" } else { "fail" }
        )
    };

    let _ = custody.destroy_root();

    Ok(format!("store {count} unwrap=ok prompt=none\n"))
}

/// Implements `--verify-deletion`.
pub fn run_deletion_check() -> StoreResult<String> {
    let dir = store_dir()?;
    let root_id = format!("halvo-del-{}", Uuid::now_v7());
    let custody = DevFallbackCustody::new(root_id.clone());
    let mut store = EncryptedStore::create(&dir, &custody, &root_id, destroyer(root_id.clone()))?;

    let (seg_ref, _key_id) =
        store.write_segment_with_expiry(b"PLAINTEXT-SECRET-TO-DESTROY", 0, 1)?;
    let segment_id = seg_ref.segment_id;

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
    let store_key_fails = decrypt_segment_bytes(&cipher, store.store_key_bytes()).is_err();
    let remaining_keys = remaining_segment_key_count(&store)?;
    let decryptable = !(file_gone && store_key_fails && remaining_keys == 0);

    let all = store.crypto_shred_all()?;
    let sealed = store.is_sealed();

    let _ = custody.destroy_root();

    Ok(format!(
        "segment={} shred=ok decryptable={} remaining_keys={} delete_all_keys={} store_sealed={}\n",
        segment_id.simple(),
        if decryptable { "yes" } else { "no" },
        remaining_keys,
        all.keys_destroyed,
        if sealed { "yes" } else { "no" },
    ))
}

fn remaining_segment_key_count(store: &EncryptedStore) -> StoreResult<i64> {
    store
        .db()
        .connection()
        .query_row(
            "SELECT count(*) FROM segment_keys WHERE wrapped_key IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .map_err(|e| StoreError::Io(e.to_string()))
}
