//! Store and deletion verification the gate drives through `--verify-store` and
//! `--verify-deletion`.

use std::fmt::{self, Write as _};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use base64::engine::GeneralPurpose;
use base64::Engine;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{
    custody_for, opens_under_an_all_zero_root, CustodyError, CustodyKind, KeyCustody, RootId,
    WrappedKey,
};
use crate::event::{CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence};

use super::db::{db_error, EncryptedStore, KeyFile};
use super::segment::{self, segment_path};
use super::{SegmentExpiry, SegmentStore, StoreError, StoreResult, DEFAULT_RETENTION};

pub const MARKER_ROWS: u64 = 2000;
pub const DECRYPTABLE: &str = "decryptable";

/// An unwrap slower than this is reported as `prompt=shown`.
pub const PROMPT_BOUND_MS: u128 = 1000;

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

pub(crate) struct StoreDir {
    path: PathBuf,
    owned: bool,
}

impl StoreDir {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn from_env() -> Self {
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

struct DestroyRootOnDrop(RootId);

impl Drop for DestroyRootOnDrop {
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
    /// Wall time of the store-key unwrap of the `open` that reopened the store.
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

pub(crate) fn marker_from_env() -> String {
    std::env::var("HALVO_GATE_MARKER").unwrap_or_else(|_| "HALVO_DEFAULT_MARKER".to_string())
}

pub(crate) fn run_store_check_in(dir: &Path, marker: &str) -> StoreResult<StoreCheck> {
    let root_id = RootId::fresh();
    let _root = DestroyRootOnDrop(root_id);
    let store = EncryptedStore::create(
        dir,
        Box::new(DevFallbackCustody::new(root_id)),
        DEFAULT_RETENTION,
    )?;
    check_store(store, marker, &custody_for)
}

pub(crate) type RebuildCustody<'a> =
    &'a dyn Fn(CustodyKind, RootId) -> Result<Box<dyn KeyCustody + Send>, CustodyError>;

pub(crate) fn check_store(
    mut store: EncryptedStore,
    marker: &str,
    rebuild: RebuildCustody<'_>,
) -> StoreResult<StoreCheck> {
    let store_dir = store.dir().to_path_buf();
    let dir = store_dir.as_path();

    let segment_id = {
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
    drop(store);

    let reopened = EncryptedStore::open_with(dir, DEFAULT_RETENTION, rebuild)?;
    let unwrap_ms = reopened.unwrap_time().as_millis();
    let db = reopened.db()?;
    let rows: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .map_err(db_error)?;
    let mut seg_plain = Vec::new();
    reopened.read_segment(segment_id, &mut seg_plain)?;
    let check = StoreCheck {
        rows,
        secure_delete: db.pragma_int("secure_delete")?,
        temp_store: db.pragma_int("temp_store")?,
        fts_secure_delete: db.fts_secure_delete()?,
        cipher_provider: db.cipher_provider()?,
        segment_roundtrip: String::from_utf8_lossy(&seg_plain).contains(marker),
        unwrap_ms,
    };
    let store_key = Zeroizing::new(*reopened.store_key()?.expose());
    drop(reopened);
    if raw_key_at_rest(dir, &store_key)? {
        return Err(StoreError::Corrupt(
            "a file of the store directory holds the raw store key".to_string(),
        ));
    }
    Ok(check)
}

fn hex_text(key: &[u8; 32], upper: bool, padded: bool) -> Zeroizing<Vec<u8>> {
    let mut text = Zeroizing::new(String::with_capacity(64));
    for b in key {
        let _ = match (upper, padded) {
            (false, true) => write!(text, "{b:02x}"),
            (true, true) => write!(text, "{b:02X}"),
            (false, false) => write!(text, "{b:x}"),
            (true, false) => write!(text, "{b:X}"),
        };
    }
    Zeroizing::new(text.as_bytes().to_vec())
}

fn base64_text(engine: &GeneralPurpose, key: &[u8; 32], offset: usize) -> Zeroizing<Vec<u8>> {
    let mut shifted = Zeroizing::new(vec![0u8; offset]);
    shifted.extend_from_slice(key);
    let text = Zeroizing::new(engine.encode(shifted.as_slice()));
    let first = (8 * offset).div_ceil(6);
    let end = (8 * offset + 8 * key.len()) / 6;
    Zeroizing::new(text.as_bytes()[first..end].to_vec())
}

fn key_texts(key: &[u8; 32]) -> Vec<Zeroizing<Vec<u8>>> {
    let mut texts = vec![Zeroizing::new(key.to_vec())];
    for upper in [false, true] {
        for padded in [true, false] {
            texts.push(hex_text(key, upper, padded));
        }
    }
    for engine in [&STANDARD_NO_PAD, &URL_SAFE_NO_PAD] {
        for offset in 0..3 {
            texts.push(base64_text(engine, key, offset));
        }
    }
    texts
}

fn holds_as_decimal_numbers(bytes: &[u8], key: &[u8; 32]) -> bool {
    let mut last = Zeroizing::new(Vec::with_capacity(key.len()));
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        let number = std::str::from_utf8(&bytes[start..at])
            .ok()
            .filter(|digits| digits.len() <= 3)
            .and_then(|digits| digits.parse::<u8>().ok());
        let Some(number) = number else {
            last.clear();
            continue;
        };
        if last.len() == key.len() {
            last.remove(0);
        }
        last.push(number);
        if last.as_slice() == key {
            return true;
        }
        let mut next = at;
        while next < bytes.len() && bytes[next].is_ascii_whitespace() {
            next += 1;
        }
        if next < bytes.len() && bytes[next] == b',' {
            next += 1;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if next < bytes.len() && bytes[next].is_ascii_digit() {
                at = next;
                continue;
            }
        }
        last.clear();
    }
    false
}

pub(crate) fn raw_key_at_rest(dir: &Path, key: &[u8; 32]) -> StoreResult<bool> {
    let texts = key_texts(key);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| StoreError::Io(e.kind()))?;
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|e| StoreError::Io(e.kind()))? {
                stack.push(entry.map_err(|e| StoreError::Io(e.kind()))?.path());
            }
        } else if metadata.is_file() {
            let bytes = Zeroizing::new(std::fs::read(&path).map_err(|e| StoreError::Io(e.kind()))?);
            if texts.iter().any(|text| {
                bytes
                    .windows(text.len())
                    .any(|window| window == text.as_slice())
            }) || holds_as_decimal_numbers(&bytes, key)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[derive(Debug)]
pub(crate) enum RootState {
    Destroyed,
    Alive,
    Unknown(CustodyError),
}

pub(crate) fn root_state(custody: &dyn KeyCustody, wrapped: &WrappedKey) -> RootState {
    match custody.unwrap_store_key(wrapped) {
        Ok(_) => RootState::Alive,
        Err(CustodyError::RootMissing) => RootState::Destroyed,
        Err(other) => RootState::Unknown(other),
    }
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
    /// The custody answers RootMissing for the wrap retained from before
    /// delete-all, and that wrap does not open under an all-zero root.
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

pub(crate) fn run_deletion_check_in(dir: &Path) -> StoreResult<DeletionCheck> {
    let root_id = RootId::fresh();
    let _root = DestroyRootOnDrop(root_id);
    let store = EncryptedStore::create(
        dir,
        Box::new(DevFallbackCustody::new(root_id)),
        DEFAULT_RETENTION,
    )?;
    check_deletion(store, &custody_for)
}

pub(crate) fn check_deletion(
    mut store: EncryptedStore,
    rebuild: RebuildCustody<'_>,
) -> StoreResult<DeletionCheck> {
    let store_dir = store.dir().to_path_buf();
    let dir = store_dir.as_path();

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
    let wal_bytes = store.db()?.wal_bytes()?;
    let remaining_keys = remaining_wrapped_keys(&store)?.len();
    let decryptable_with_remaining_keys = decryptable_with_remaining_keys(&store, &cipher)?;
    let sibling_readable = store
        .read_segment(sibling.segment_id, &mut Vec::new())
        .is_ok();

    let key_file = KeyFile::read(dir)?;
    let all = store.crypto_shred_all()?;
    drop(store);
    let retained = key_file.wrapped_store_key()?;
    let custody = rebuild(key_file.custody_kind, key_file.root_id)?;
    let root_destroyed = match root_state(custody.as_ref(), &retained) {
        RootState::Destroyed => !opens_under_an_all_zero_root(key_file.root_id, &retained),
        RootState::Alive => false,
        RootState::Unknown(e) => return Err(StoreError::Custody(e)),
    };
    let store_sealed = matches!(
        EncryptedStore::open_with(dir, DEFAULT_RETENTION, rebuild),
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
    use crate::custody::file_root::test_support::{FileRootCustody, TestProtector};
    use crate::custody::file_root::FileRoots;
    use crate::store::db::test_support::make_store;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE};

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

    struct Answers(fn() -> Result<crate::custody::StoreKey, CustodyError>);

    impl KeyCustody for Answers {
        fn kind(&self) -> crate::custody::CustodyKind {
            crate::custody::CustodyKind::DevFallback
        }
        fn root_id(&self) -> RootId {
            RootId::fresh()
        }
        fn wrap_store_key(
            &self,
            _key: &crate::custody::StoreKey,
        ) -> Result<WrappedKey, CustodyError> {
            Ok(WrappedKey(Vec::new()))
        }
        fn create_root(&self) -> Result<(), CustodyError> {
            Ok(())
        }
        fn unwrap_store_key(
            &self,
            _wrapped: &WrappedKey,
        ) -> Result<crate::custody::StoreKey, CustodyError> {
            (self.0)()
        }
        fn destroy_root(&self) -> Result<(), CustodyError> {
            Ok(())
        }
    }

    #[test]
    fn the_root_state_is_destroyed_only_for_a_missing_root_and_alive_only_for_an_unwrap() {
        let wrapped = WrappedKey(vec![0u8; 72]);
        let transient = root_state(&Answers(|| Err(CustodyError::RootLocked(-25308))), &wrapped);
        assert!(
            matches!(
                transient,
                RootState::Unknown(CustodyError::RootLocked(-25308))
            ),
            "{transient:?}"
        );
        let alive = root_state(
            &Answers(|| Ok(crate::custody::StoreKey::from_bytes([1u8; 32]))),
            &wrapped,
        );
        assert!(matches!(alive, RootState::Alive), "{alive:?}");
        let destroyed = root_state(&Answers(|| Err(CustodyError::RootMissing)), &wrapped);
        assert!(matches!(destroyed, RootState::Destroyed), "{destroyed:?}");
        let every_error: [fn() -> Result<crate::custody::StoreKey, CustodyError>; 11] = [
            || Err(CustodyError::RootMissing),
            || Err(CustodyError::RootCorrupt),
            || Err(CustodyError::RootDenied),
            || Err(CustodyError::NoRootDirectory),
            || Err(CustodyError::RootLocked(-25308)),
            || {
                Err(CustodyError::CustodyKindUnsupported(
                    CustodyKind::SecureEnclave,
                ))
            },
            || Err(CustodyError::RootExists),
            || Err(CustodyError::WrapAuthFailed),
            || Err(CustodyError::Os(Some(5))),
            || Err(CustodyError::Crypto("the wrap does not decode".to_string())),
            || Err(CustodyError::NotImplemented("a custody call")),
        ];
        let mut wrong = Vec::new();
        for answer in every_error {
            let Err(error) = answer() else {
                unreachable!("every entry answers an error")
            };
            let owed = state_owed_to(&error);
            let state = root_state(&Answers(answer), &wrapped);
            let got = match state {
                RootState::Destroyed => "destroyed",
                RootState::Alive => "alive",
                RootState::Unknown(_) => "unknown",
            };
            if got != owed {
                wrong.push(format!("{error:?} gave {got} in place of {owed}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    fn state_owed_to(error: &CustodyError) -> &'static str {
        match error {
            CustodyError::RootMissing => "destroyed",
            CustodyError::RootCorrupt
            | CustodyError::RootDenied
            | CustodyError::NoRootDirectory
            | CustodyError::RootLocked(_)
            | CustodyError::CustodyKindUnsupported(_)
            | CustodyError::RootExists
            | CustodyError::WrapAuthFailed
            | CustodyError::Os(_)
            | CustodyError::Crypto(_)
            | CustodyError::NotImplemented(_) => "unknown",
        }
    }

    fn file_root_custody(roots_dir: &Path, root_id: RootId) -> FileRootCustody<TestProtector> {
        FileRootCustody {
            roots: FileRoots::new(roots_dir.to_path_buf(), TestProtector::new(0x5a).0),
            root_id,
        }
    }

    fn file_root_store(dir: &Path) -> (EncryptedStore, PathBuf, RootId) {
        let roots_dir = dir.join("roots");
        let root_id = RootId::fresh();
        let store = EncryptedStore::create(
            &dir.join("store"),
            Box::new(file_root_custody(&roots_dir, root_id)),
            DEFAULT_RETENTION,
        )
        .unwrap();
        (store, roots_dir, root_id)
    }

    #[test]
    fn the_store_check_fails_when_the_store_key_file_holds_the_raw_store_key() {
        let dir = tempfile::tempdir().unwrap();
        let (store, roots_dir, root_id) = file_root_store(&dir.path().join("healthy"));
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(file_root_custody(&roots_dir, root_id)))
            };
        let healthy = check_store(store, "MARKER-OF-A-HEALTHY-STORE", &rebuild);
        assert!(
            healthy.is_ok(),
            "a healthy store failed the check: {healthy:?}"
        );

        let (store, roots_dir, root_id) = file_root_store(&dir.path().join("leaky"));
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(file_root_custody(&roots_dir, root_id)))
            };
        let key_hex: String = store
            .store_key()
            .unwrap()
            .expose()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let key_file = KeyFile::path(store.dir());
        let mut fields: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&key_file).unwrap()).unwrap();
        fields["debug_store_key"] = serde_json::Value::String(key_hex);
        std::fs::write(&key_file, serde_json::to_vec(&fields).unwrap()).unwrap();
        let leaky = check_store(store, "MARKER-OF-A-LEAKY-STORE", &rebuild);
        assert!(
            matches!(&leaky, Err(StoreError::Corrupt(why)) if why.contains("raw store key")),
            "a store.key that holds the raw store key passed the check: {leaky:?}"
        );
    }

    struct SlowUnwrap(FileRootCustody<TestProtector>);

    const SLOW_UNWRAP_MS: u64 = 1100;

    impl KeyCustody for SlowUnwrap {
        fn kind(&self) -> CustodyKind {
            self.0.kind()
        }
        fn root_id(&self) -> RootId {
            self.0.root_id()
        }
        fn wrap_store_key(
            &self,
            key: &crate::custody::StoreKey,
        ) -> Result<WrappedKey, CustodyError> {
            self.0.wrap_store_key(key)
        }
        fn create_root(&self) -> Result<(), CustodyError> {
            self.0.create_root()
        }
        fn unwrap_store_key(
            &self,
            wrapped: &WrappedKey,
        ) -> Result<crate::custody::StoreKey, CustodyError> {
            std::thread::sleep(std::time::Duration::from_millis(SLOW_UNWRAP_MS));
            self.0.unwrap_store_key(wrapped)
        }
        fn destroy_root(&self) -> Result<(), CustodyError> {
            self.0.destroy_root()
        }
    }

    #[test]
    fn the_store_check_prints_the_time_of_the_unwrap_that_opened_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let (store, roots_dir, root_id) = file_root_store(dir.path());
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(SlowUnwrap(file_root_custody(&roots_dir, root_id))))
            };
        let check = check_store(store, "MARKER-OF-A-SLOW-UNWRAP", &rebuild).unwrap();
        assert!(
            check.unwrap_ms >= u128::from(SLOW_UNWRAP_MS),
            "an unwrap of {SLOW_UNWRAP_MS} ms printed unwrap_ms={}",
            check.unwrap_ms
        );
        let line = check.to_string();
        assert!(
            line.contains(&format!(" unwrap=ok unwrap_ms={} ", check.unwrap_ms)),
            "{line}"
        );
    }

    #[test]
    fn the_deletion_check_fails_when_it_cannot_tell_the_state_of_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _, _) = file_root_store(dir.path());
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(Answers(|| Err(CustodyError::RootLocked(5)))))
            };
        let check = check_deletion(store, &rebuild);
        assert!(
            matches!(check, Err(StoreError::Custody(CustodyError::RootLocked(5)))),
            "a root whose state the custody cannot tell gave {check:?}"
        );
    }

    struct AllZeroRoot(RootId);

    impl KeyCustody for AllZeroRoot {
        fn kind(&self) -> CustodyKind {
            CustodyKind::DevFallback
        }
        fn root_id(&self) -> RootId {
            self.0
        }
        fn wrap_store_key(
            &self,
            key: &crate::custody::StoreKey,
        ) -> Result<WrappedKey, CustodyError> {
            crate::custody::dev_fallback::aead_seal(&[0u8; 32], key.expose(), self.0.as_bytes())
                .map(WrappedKey)
                .map_err(|_| CustodyError::Crypto("the seal failed".to_string()))
        }
        fn create_root(&self) -> Result<(), CustodyError> {
            Ok(())
        }
        fn unwrap_store_key(
            &self,
            _wrapped: &WrappedKey,
        ) -> Result<crate::custody::StoreKey, CustodyError> {
            Err(CustodyError::RootMissing)
        }
        fn destroy_root(&self) -> Result<(), CustodyError> {
            Ok(())
        }
    }

    #[test]
    fn the_deletion_check_takes_a_missing_root_as_destroyed_only_when_no_all_zero_root_opens_the_key(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (store, roots_dir, root_id) = file_root_store(&dir.path().join("healthy"));
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(file_root_custody(&roots_dir, root_id)))
            };
        let healthy = check_deletion(store, &rebuild).unwrap();
        assert!(healthy.root_destroyed, "{healthy}");
        assert!(healthy.store_sealed, "{healthy}");

        let zero_dir = dir.path().join("all-zero");
        let store = EncryptedStore::create(
            &zero_dir,
            Box::new(AllZeroRoot(RootId::fresh())),
            DEFAULT_RETENTION,
        )
        .unwrap();
        let rebuild =
            |_: CustodyKind, root_id: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(AllZeroRoot(root_id)))
            };
        let zero = check_deletion(store, &rebuild).unwrap();
        assert!(
            !zero.root_destroyed,
            "a key that opens under an all-zero root counted as destroyed: {zero}"
        );
        assert!(zero.to_string().contains(" root_destroyed=no "), "{zero}");
    }

    struct DestroysNothing(FileRootCustody<TestProtector>);

    impl KeyCustody for DestroysNothing {
        fn kind(&self) -> CustodyKind {
            self.0.kind()
        }
        fn root_id(&self) -> RootId {
            self.0.root_id()
        }
        fn wrap_store_key(
            &self,
            key: &crate::custody::StoreKey,
        ) -> Result<WrappedKey, CustodyError> {
            self.0.wrap_store_key(key)
        }
        fn create_root(&self) -> Result<(), CustodyError> {
            self.0.create_root()
        }
        fn unwrap_store_key(
            &self,
            wrapped: &WrappedKey,
        ) -> Result<crate::custody::StoreKey, CustodyError> {
            self.0.unwrap_store_key(wrapped)
        }
        fn destroy_root(&self) -> Result<(), CustodyError> {
            Ok(())
        }
    }

    #[test]
    fn the_deletion_check_takes_a_root_that_still_unwraps_the_retained_key_as_alive() {
        let dir = tempfile::tempdir().unwrap();
        let roots_dir = dir.path().join("roots");
        let root_id = RootId::fresh();
        let store = EncryptedStore::create(
            &dir.path().join("store"),
            Box::new(DestroysNothing(file_root_custody(&roots_dir, root_id))),
            DEFAULT_RETENTION,
        )
        .unwrap();
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(file_root_custody(&roots_dir, root_id)))
            };
        let check = check_deletion(store, &rebuild).unwrap();
        assert!(
            !check.root_destroyed,
            "a root that still unwraps the retained key counted as destroyed: {check}"
        );
        assert!(check.to_string().contains(" root_destroyed=no "), "{check}");
    }

    #[test]
    fn the_deletion_check_counts_as_sealed_only_a_directory_with_no_store() {
        let dir = tempfile::tempdir().unwrap();
        let (store, roots_dir, root_id) = file_root_store(dir.path());
        let key_file = KeyFile::path(store.dir());
        let left_by_a_stopped_delete_all = std::fs::read(&key_file).unwrap();
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                if !key_file.exists() {
                    std::fs::write(&key_file, &left_by_a_stopped_delete_all).unwrap();
                }
                Ok(Box::new(file_root_custody(&roots_dir, root_id)))
            };
        let check = check_deletion(store, &rebuild).unwrap();
        let open = EncryptedStore::open_with(
            dir.path().join("store").as_path(),
            DEFAULT_RETENTION,
            rebuild,
        )
        .map(drop);
        assert!(
            matches!(open, Err(StoreError::Custody(CustodyError::RootMissing))),
            "the open after delete-all with a store.key left in place answered {open:?}"
        );
        assert!(
            !check.store_sealed,
            "an open that answered Custody(RootMissing) counted as sealed: {check}"
        );
        assert!(check.to_string().contains(" store_sealed=no"), "{check}");
    }

    #[test]
    fn the_deletion_check_fails_when_its_shred_pass_shreds_another_count_than_one() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, roots_dir, root_id) = file_root_store(dir.path());
        store
            .write_segment_with_expiry(
                &mut Cursor::new(b"a segment that expired before the check"),
                0,
                1,
            )
            .unwrap();
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(file_root_custody(&roots_dir, root_id)))
            };
        let check = check_deletion(store, &rebuild);
        assert!(
            matches!(&check, Err(StoreError::Corrupt(why)) if why == "expected 1 shredded segment, got 2"),
            "a shred pass of two segments did not stop the deletion check: {check:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_store_check_fails_on_a_file_of_the_store_directory_that_it_cannot_read() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let (store, roots_dir, root_id) = file_root_store(dir.path());
        let unreadable = store.dir().join("a-file-that-the-scan-cannot-read");
        std::fs::write(&unreadable, b"bytes that the scan must read").unwrap();
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        let rebuild =
            |_: CustodyKind, _: RootId| -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
                Ok(Box::new(file_root_custody(&roots_dir, root_id)))
            };
        let check = check_store(store, "MARKER-BESIDE-AN-UNREADABLE-FILE", &rebuild);
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            matches!(
                check,
                Err(StoreError::Io(std::io::ErrorKind::PermissionDenied))
            ),
            "a store check with a file that it cannot read answered {check:?}"
        );
    }

    #[test]
    fn the_all_zero_root_probe_opens_only_a_key_wrapped_under_an_all_zero_root() {
        let root_id = RootId::fresh();
        let key = [0x37u8; 32];
        let under_zero =
            crate::custody::dev_fallback::aead_seal(&[0u8; 32], &key, root_id.as_bytes()).unwrap();
        let under_a_root =
            crate::custody::dev_fallback::aead_seal(&[0x91u8; 32], &key, root_id.as_bytes())
                .unwrap();
        assert!(opens_under_an_all_zero_root(
            root_id,
            &WrappedKey(under_zero)
        ));
        assert!(!opens_under_an_all_zero_root(
            root_id,
            &WrappedKey(under_a_root)
        ));
    }

    fn base64_inside_a_longer_text(
        engine: &base64::engine::GeneralPurpose,
        before: usize,
        key: &[u8; 32],
        after: usize,
    ) -> Vec<u8> {
        let mut bytes = vec![0xa5u8; before];
        bytes.extend_from_slice(key);
        bytes.extend(std::iter::repeat_n(0x5au8, after));
        engine.encode(&bytes).into_bytes()
    }

    #[test]
    fn the_raw_key_scan_finds_every_listed_form_of_the_key_and_nothing_else() {
        let key: [u8; 32] = std::array::from_fn(|i| [0x03, 0xf0, 0x0a, 0x9c][i % 4] ^ (i as u8));
        assert!(key.iter().any(|&b| b < 0x10) && key.iter().any(|&b| b > 0x7f));
        let hex = |case: fn(&u8) -> String| -> Vec<u8> {
            key.iter().flat_map(|b| case(b).into_bytes()).collect()
        };
        let mut another_list: Vec<u8> = key.to_vec();
        another_list[17] ^= 0x01;
        let mut longer_list = vec![1u8, 2, 3, 4, 5];
        longer_list.extend_from_slice(&key);
        longer_list.extend_from_slice(&[6, 7]);
        let cases: Vec<(&str, Vec<u8>, bool)> = vec![
            ("raw bytes", key.to_vec(), true),
            ("lower hex", hex(|b| format!("{b:02x}")), true),
            ("upper hex", hex(|b| format!("{b:02X}")), true),
            (
                "lower hex without the padding of a byte",
                hex(|b| format!("{b:x}")),
                true,
            ),
            (
                "upper hex without the padding of a byte",
                hex(|b| format!("{b:X}")),
                true,
            ),
            ("base64", STANDARD.encode(key).into_bytes(), true),
            (
                "url-safe base64 without padding",
                URL_SAFE_NO_PAD.encode(key).into_bytes(),
                true,
            ),
            (
                "base64 of 25 bytes, the key and 7 bytes",
                base64_inside_a_longer_text(&STANDARD, 25, &key, 7),
                true,
            ),
            (
                "base64 of 26 bytes and the key",
                base64_inside_a_longer_text(&STANDARD, 26, &key, 0),
                true,
            ),
            (
                "url-safe base64 of 1 byte, the key and 3 bytes",
                base64_inside_a_longer_text(&URL_SAFE, 1, &key, 3),
                true,
            ),
            (
                "url-safe base64 of 2 bytes and the key, without padding",
                base64_inside_a_longer_text(&URL_SAFE_NO_PAD, 2, &key, 0),
                true,
            ),
            ("a debug print", format!("{key:?}").into_bytes(), true),
            (
                "a pretty debug print",
                format!("{key:#?}").into_bytes(),
                true,
            ),
            (
                "a JSON array",
                serde_json::to_vec(&key.to_vec()).unwrap(),
                true,
            ),
            (
                "a pretty JSON array",
                serde_json::to_vec_pretty(&key.to_vec()).unwrap(),
                true,
            ),
            (
                "a debug print of a longer list",
                format!("{longer_list:?}").into_bytes(),
                true,
            ),
            (
                "a list of numbers of four digits",
                key.map(|b| format!("{b:04}")).join(", ").into_bytes(),
                false,
            ),
            ("another key", key.map(|b| b ^ 0x40).to_vec(), false),
            (
                "a list with one number changed",
                format!("{another_list:?}").into_bytes(),
                false,
            ),
        ];
        let mut wrong = Vec::new();
        for (case, bytes, found) in cases {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("segments")).unwrap();
            let mut file = b"prefix ".to_vec();
            file.extend_from_slice(&bytes);
            file.extend_from_slice(b" suffix");
            std::fs::write(dir.path().join("segments").join("one.seg"), &file).unwrap();
            let answer = raw_key_at_rest(dir.path(), &key).unwrap();
            if answer != found {
                wrong.push(format!("{case}: the scan answered {answer}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
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
    fn measure_for_parent() {
        let Ok(dir) = std::env::var("HALVO_TEST_MEASURE_DIR") else {
            println!("measure_for_parent: HALVO_TEST_MEASURE_DIR is not set, so no parent asked for a measure");
            return;
        };
        let dir = PathBuf::from(dir);
        let store = crate::store::measure_store(&dir.join("store"), "MARKER-OF-THE-MEASURE-ENTRY");
        let deletion = crate::store::measure_deletion(&dir.join("deletion"));
        println!();
        println!("MEASURED {}", store.unwrap().trim_end());
        println!("MEASURED {}", deletion.unwrap().trim_end());
    }

    #[test]
    fn the_measure_entries_take_their_directory_and_read_no_environment() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store::verify::tests::measure_for_parent",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HALVO_TEST_MEASURE_DIR", dir.path())
            .env("HALVO_STORE_DIR", elsewhere.path())
            .env("HALVO_GATE_MARKER", "MARKER-FROM-THE-ENVIRONMENT")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "the child failed: {text}");
        let measured: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("MEASURED "))
            .collect();
        assert_eq!(measured.len(), 2, "{text}");
        assert!(
            measured[0].starts_with("store rows=2000 ") && measured[0].contains(" unwrap=ok "),
            "{}",
            measured[0]
        );
        assert!(
            measured[1].starts_with("segment=") && measured[1].ends_with(" store_sealed=yes"),
            "{}",
            measured[1]
        );
        assert!(KeyFile::path(&dir.path().join("store")).exists());
        assert!(dir.path().join("deletion").is_dir());
        assert_eq!(
            std::fs::read_dir(elsewhere.path()).unwrap().count(),
            0,
            "a measure entry wrote into the directory that HALVO_STORE_DIR names"
        );
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
