//! Store and deletion verification the gate drives through `--verify-store` and
//! `--verify-deletion`. Every field the gate reads is a measurement of the
//! store under test; nothing here prints a constant it did not observe.

use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use uuid::Uuid;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{custody_for, KeyCustody, RootId};
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence};

use super::db::{db_error, EncryptedStore, KeyFile};
use super::segment::{self, segment_path};
use super::{SegmentExpiry, SegmentStore, StoreError, StoreResult, DEFAULT_RETENTION};

pub const MARKER_ROWS: u64 = 2000;
pub const DECRYPTABLE: &str = "decryptable";

/// An unattended keychain unwrap returns in milliseconds; a custody prompt
/// blocks until a person answers it, which no one does in under a second. An
/// unwrap slower than this is reported as `prompt=shown`.
pub const PROMPT_BOUND_MS: u128 = 1000;

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// The store directory for a verify run: `HALVO_STORE_DIR`, or a fresh temp
/// directory that is removed when the run ends.
struct StoreDir {
    path: PathBuf,
    owned: bool,
}

impl StoreDir {
    fn from_env() -> Self {
        match std::env::var("HALVO_STORE_DIR") {
            Ok(d) => Self {
                path: PathBuf::from(d),
                owned: false,
            },
            Err(_) => Self {
                path: std::env::temp_dir().join(format!("halvo-verify-{}", Uuid::now_v7())),
                owned: true,
            },
        }
    }
}

impl Drop for StoreDir {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Destroys a dev-fallback root when dropped, so a verify run that fails
/// halfway leaves no keychain item behind. Succeeds when the root is gone.
struct RootGuard(RootId);

impl Drop for RootGuard {
    fn drop(&mut self) {
        let _ = DevFallbackCustody::new(self.0).destroy_root();
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
    /// Wall time of the owner's store-key unwrap through the recorded custody.
    pub unwrap_ms: u128,
}

impl StoreCheck {
    pub fn prompt_shown(&self) -> bool {
        self.unwrap_ms > PROMPT_BOUND_MS
    }
}

impl fmt::Display for StoreCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "store rows={} secure_delete={} temp_store={} fts_secure_delete={} \
             cipher_provider={} segment_roundtrip={} unwrap=ok unwrap_ms={} prompt={}",
            self.rows,
            self.secure_delete,
            self.temp_store,
            self.fts_secure_delete,
            self.cipher_provider,
            if self.segment_roundtrip { "ok" } else { "fail" },
            self.unwrap_ms,
            if self.prompt_shown() { "shown" } else { "none" },
        )
    }
}

/// Implements `--verify-store`.
pub fn run_store_check() -> StoreResult<StoreCheck> {
    let dir = StoreDir::from_env();
    let marker =
        std::env::var("HALVO_GATE_MARKER").unwrap_or_else(|_| "HALVO_DEFAULT_MARKER".to_string());
    run_store_check_in(&dir.path, &marker)
}

pub(crate) fn run_store_check_in(dir: &Path, marker: &str) -> StoreResult<StoreCheck> {
    let root_id = RootId::fresh();
    let _root = RootGuard(root_id);

    let segment_id = {
        let mut store = EncryptedStore::create(
            dir,
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
                marker.to_string(),
                seg_ref.clone(),
                key_id,
            );
            store.db()?.insert_event(&event)?;
        }
        tx.commit().map_err(db_error)?;
        store.db()?.checkpoint_truncate()?;
        seg_ref.segment_id
    };

    let unwrap_ms = {
        let key_file = KeyFile::read(dir)?;
        let custody = custody_for(key_file.custody_kind, key_file.root_id)?;
        let started = Instant::now();
        custody.unwrap_store_key(&key_file.wrapped_store_key()?)?;
        started.elapsed().as_millis()
    };

    let reopened = EncryptedStore::open(dir, DEFAULT_RETENTION)?;
    let db = reopened.db()?;
    let rows: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .map_err(db_error)?;
    let mut seg_plain = Vec::new();
    reopened.read_segment(segment_id, &mut seg_plain)?;
    Ok(StoreCheck {
        rows,
        secure_delete: db.pragma_int("secure_delete")?,
        temp_store: db.pragma_int("temp_store")?,
        fts_secure_delete: db.fts_secure_delete()?,
        cipher_provider: db.cipher_provider()?,
        segment_roundtrip: String::from_utf8_lossy(&seg_plain).contains(marker),
        unwrap_ms,
    })
}

/// What `--verify-deletion` measured: one force-expired segment crypto-shredded
/// beside a live sibling, then delete-all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionCheck {
    pub segment_id: Uuid,
    pub file_gone: bool,
    /// The saved ciphertext decrypted under some key still in the store.
    pub decryptable_with_remaining_keys: bool,
    pub sibling_readable: bool,
    pub remaining_keys: usize,
    /// Bytes left in the WAL after the shred pass.
    pub wal_bytes: u64,
    pub delete_all_keys: u64,
    /// The store key retained from before delete-all no longer unwraps through
    /// the custody that wrapped it.
    pub root_destroyed: bool,
    /// `open` on the directory after delete-all returns `Sealed`.
    pub store_sealed: bool,
}

impl fmt::Display for DeletionCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "segment={} shred=ok file_gone={} {DECRYPTABLE}={} sibling_readable={} \
             remaining_keys={} wal_bytes={} delete_all_keys={} root_destroyed={} store_sealed={}",
            self.segment_id.simple(),
            yes_no(self.file_gone),
            yes_no(self.decryptable_with_remaining_keys),
            yes_no(self.sibling_readable),
            self.remaining_keys,
            self.wal_bytes,
            self.delete_all_keys,
            yes_no(self.root_destroyed),
            yes_no(self.store_sealed),
        )
    }
}

/// Implements `--verify-deletion`.
pub fn run_deletion_check() -> StoreResult<DeletionCheck> {
    let dir = StoreDir::from_env();
    run_deletion_check_in(&dir.path)
}

pub(crate) fn run_deletion_check_in(dir: &Path) -> StoreResult<DeletionCheck> {
    let root_id = RootId::fresh();
    let _root = RootGuard(root_id);
    let mut store = EncryptedStore::create(
        dir,
        Box::new(DevFallbackCustody::new(root_id)),
        DEFAULT_RETENTION,
    )?;

    let (expired, _) =
        store.write_segment_with_expiry(&mut Cursor::new(b"PLAINTEXT-SECRET-TO-DESTROY"), 0, 1)?;
    let (sibling, _) = store.write_segment(&mut Cursor::new(b"live sibling segment"))?;
    let segment_id = expired.segment_id;
    let cipher =
        std::fs::read(segment_path(dir, segment_id)).map_err(|e| StoreError::Io(e.kind()))?;

    let report = store.crypto_shred_expired()?;
    if report.segments_shredded != 1 {
        return Err(StoreError::Corrupt(format!(
            "expected 1 shredded segment, got {}",
            report.segments_shredded
        )));
    }

    let file_gone = !segment_path(dir, segment_id).exists();
    let wal_bytes = store.db()?.wal_bytes();
    let remaining_keys = remaining_wrapped_keys(&store)?.len();
    let decryptable_with_remaining_keys = decryptable_with_remaining_keys(&store, &cipher)?;
    let sibling_readable = store
        .read_segment(sibling.segment_id, &mut Vec::new())
        .is_ok();

    let key_file = KeyFile::read(dir)?;
    let all = store.crypto_shred_all()?;
    drop(store);
    let root_destroyed = custody_for(key_file.custody_kind, key_file.root_id)?
        .unwrap_store_key(&key_file.wrapped_store_key()?)
        .is_err();
    let store_sealed = matches!(
        EncryptedStore::open(dir, DEFAULT_RETENTION),
        Err(StoreError::Sealed)
    );

    Ok(DeletionCheck {
        segment_id,
        file_gone,
        decryptable_with_remaining_keys,
        sibling_readable,
        remaining_keys,
        wal_bytes,
        delete_all_keys: all.keys_destroyed,
        root_destroyed,
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

    #[test]
    fn the_store_check_measures_the_roundtrip_and_the_unwrap() {
        let dir = tempfile::tempdir().unwrap();
        let check = run_store_check_in(dir.path(), "MARKER-FOR-THE-STORE-CHECK").unwrap();
        assert_eq!(check.rows, MARKER_ROWS as i64);
        assert!(check.segment_roundtrip);
        assert_eq!(check.cipher_provider, "libtomcrypt");
        assert!(!check.prompt_shown(), "unwrap took {} ms", check.unwrap_ms);
        let line = check.to_string();
        assert!(line.contains(" segment_roundtrip=ok "), "{line}");
        assert!(line.contains(" prompt=none"), "{line}");
    }

    #[test]
    fn the_deletion_check_measures_the_root_and_the_seal() {
        let dir = tempfile::tempdir().unwrap();
        let check = run_deletion_check_in(dir.path()).unwrap();
        assert!(check.file_gone);
        assert!(!check.decryptable_with_remaining_keys);
        assert!(check.sibling_readable);
        assert_eq!(check.remaining_keys, 1);
        assert_eq!(check.wal_bytes, 0);
        assert_eq!(check.delete_all_keys, 2);
        assert!(check.root_destroyed);
        assert!(check.store_sealed);
        let line = check.to_string();
        assert!(line.contains(" root_destroyed=yes "), "{line}");
        assert!(line.contains(" store_sealed=yes"), "{line}");
    }
}
