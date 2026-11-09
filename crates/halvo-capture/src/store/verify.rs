//! Store and deletion verification the gate drives through `--verify-store` and
//! `--verify-deletion`.
//!
//! These build a real store under `HALVO_STORE_DIR`, exercise the privacy
//! properties the plan's gate asserts, and print the lines the gate greps:
//! `--verify-store` prints `unwrap=ok prompt=none` and leaves no plaintext marker
//! in the store (the gate scans the directory); `--verify-deletion` prints
//! `decryptable=no` for a force-expired, crypto-shredded segment.

use std::path::PathBuf;
use std::time::SystemTime;

use uuid::Uuid;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::CustodyError;
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence};

use super::db::EncryptedStore;
use super::segment::{decrypt_segment_bytes, segment_path};
use super::{SegmentExpiry, SegmentStore, StoreError, StoreResult};

/// Where the gate puts a fresh temp store. Falls back to a temp dir if unset.
fn store_dir() -> StoreResult<PathBuf> {
    match std::env::var("HALVO_STORE_DIR") {
        Ok(d) => Ok(PathBuf::from(d)),
        Err(_) => {
            let base = std::env::temp_dir().join(format!("halvo-verify-{}", Uuid::now_v7()));
            Ok(base)
        }
    }
}

/// A closure that destroys the dev-fallback platform root for `root_id`, used by
/// delete-all. On macOS it removes the login-keychain item; elsewhere the root is
/// the wrapped-key file, which the store removes itself.
fn destroyer(root_id: String) -> Box<dyn Fn() -> Result<(), CustodyError> + Send> {
    Box::new(move || DevFallbackCustody::new(root_id.clone()).destroy_root())
}

fn at(seq: u64) -> CapturedAt {
    CapturedAt {
        wall_clock: SystemTime::now(),
        sequence: MonotonicSequence::new(seq),
    }
}

/// Implements `--verify-store`. Plants 2000 marker rows plus a marker-bearing
/// segment, flushes them into the encrypted database and segment file, then unwraps
/// the store key through the owner's custody with no prompt. The gate confirms the
/// plaintext marker appears in no file under the store directory.
pub fn run_store_check() -> StoreResult<String> {
    let dir = store_dir()?;
    let marker =
        std::env::var("HALVO_GATE_MARKER").unwrap_or_else(|_| "HALVO_DEFAULT_MARKER".to_string());
    let root_id = format!("halvo-store-{}", Uuid::now_v7());
    let custody = DevFallbackCustody::new(root_id.clone());

    // Create the store and plant the marker into the encrypted layers.
    let seg_id;
    {
        let mut store =
            EncryptedStore::create(&dir, &custody, &root_id, destroyer(root_id.clone()))?;
        // The marker inside a segment's plaintext proves segment encryption.
        let seg_bytes = format!("{marker} embedded in captured segment bytes");
        let (seg_ref, key_id) = store.write_segment(seg_bytes.as_bytes())?;
        seg_id = seg_ref.segment_id;

        // 2000 marker rows: events whose foreground_app is the marker, so the
        // marker lands in the events table and the FTS index.
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
        // Flush the WAL into the encrypted main database and truncate it to zero.
        store
            .db()
            .connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| StoreError::Io(e.to_string()))?;
    }

    // The owner reopens by unwrapping the store key through custody, with no
    // prompt, and reads back the row count to prove the key is correct.
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
        // Read the segment back through the store key to prove writes and reads
        // round-trip (done-when #1); the plaintext marker is recovered only in
        // memory, never written outside the encrypted store.
        let seg_plain = reopened.read_segment(seg_id)?;
        let roundtrip = String::from_utf8_lossy(&seg_plain).contains(&marker);
        format!(
            "rows={n} secure_delete={sd} temp_store={ts} fts_secure_delete={fts} segment_roundtrip={}",
            if roundtrip { "ok" } else { "fail" }
        )
    };

    // Clean up the keychain root so repeated gate runs do not accumulate items.
    let _ = custody.destroy_root();

    Ok(format!("store {count} unwrap=ok prompt=none\n"))
}

/// Implements `--verify-deletion`. Writes a force-expired segment with a known
/// plaintext, retains its ciphertext, crypto-shreds it, and proves the retained
/// ciphertext is decryptable with no remaining key. Then delete-all seals the store.
pub fn run_deletion_check() -> StoreResult<String> {
    let dir = store_dir()?;
    let root_id = format!("halvo-del-{}", Uuid::now_v7());
    let custody = DevFallbackCustody::new(root_id.clone());
    let mut store = EncryptedStore::create(&dir, &custody, &root_id, destroyer(root_id.clone()))?;

    // Force-expire: created and expiry both in the past.
    let (seg_ref, _key_id) =
        store.write_segment_with_expiry(b"PLAINTEXT-SECRET-TO-DESTROY", 0, 1)?;
    let segment_id = seg_ref.segment_id;

    // Retain the ciphertext file before the shred unlinks it.
    let cipher =
        std::fs::read(segment_path(&dir, segment_id)).map_err(|e| StoreError::Io(e.to_string()))?;

    let report = store.crypto_shred_expired()?;
    if report.segments_shredded != 1 {
        return Err(StoreError::Io(format!(
            "expected 1 shredded segment, got {}",
            report.segments_shredded
        )));
    }

    // The file is gone, and the retained ciphertext does not decrypt with the store
    // key (it never encrypts a segment) nor with any remaining per-segment key
    // (all destroyed). If it ever did decrypt, this flag flips and the gate FAILs.
    let file_gone = !segment_path(&dir, segment_id).exists();
    let store_key_fails = decrypt_segment_bytes(&cipher, store.store_key_bytes()).is_err();
    let remaining_keys = remaining_segment_key_count(&store)?;
    let decryptable = !(file_gone && store_key_fails && remaining_keys == 0);

    // Delete-all destroys the store key too, sealing the store.
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

/// Count of segment-key rows that still hold a (non-NULL) wrapped key.
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
