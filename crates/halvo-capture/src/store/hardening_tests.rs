use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use halvo_sqlcipher::rusqlite::Connection;
use uuid::Uuid;

use crate::custody::dev_fallback::{dev_root_exists, DevFallbackCustody};
use crate::custody::file_root::test_support::{FileRootCustody, TestProtector};
use crate::custody::file_root::{FileRoots, Protector};
use crate::custody::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

use super::db::test_support::pages;
use super::db::{EncryptedStore, KeyFile};
use super::segment::tests::{BODY, CIPHER_CHUNK_LEN, HEADER, TAG_LEN};
use super::segment::{self, segment_path, CHUNK};
use super::{SegmentExpiry, SegmentStore, StoreError, StoreResult, DEFAULT_RETENTION};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn wal_len(dir: &Path) -> u64 {
    std::fs::metadata(dir.join("store.db-wal"))
        .map(|m| m.len())
        .unwrap_or(0)
}

fn wrapped_key_in_copy_of_main_file_without_wal(
    dir: &Path,
    store_key: &[u8; 32],
    segment_id: Uuid,
) -> Result<Option<Vec<u8>>, halvo_sqlcipher::rusqlite::Error> {
    let copy = tempfile::tempdir().unwrap();
    std::fs::copy(dir.join("store.db"), copy.path().join("store.db")).unwrap();
    let conn = Connection::open(copy.path().join("store.db"))?;
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", hex(store_key)))?;
    match conn.query_row(
        "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
        [segment_id.simple().to_string()],
        |r| r.get::<_, Option<Vec<u8>>>(0),
    ) {
        Err(halvo_sqlcipher::rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        other => other,
    }
}

fn recoverable(cipher: &[u8], store_key: &[u8; 32], wrapped: Option<Vec<u8>>) -> bool {
    wrapped
        .and_then(|w| segment::unwrap_segment_key(store_key, &w).ok())
        .and_then(|k| segment::decrypt_segment_bytes(cipher, &k).ok())
        .is_some()
}

fn live_key_rows(store: &EncryptedStore) -> i64 {
    store
        .db()
        .unwrap()
        .connection()
        .query_row(
            "SELECT count(*) FROM segment_keys WHERE wrapped_key IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

struct FlakyCustody {
    inner: DevFallbackCustody,
    destroy_failures_before_success: AtomicUsize,
    destroy_calls: Arc<AtomicUsize>,
}

impl FlakyCustody {
    fn new(root_id: RootId, destroy_failures_before_success: usize) -> (Self, Arc<AtomicUsize>) {
        let destroy_calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                inner: DevFallbackCustody::new(root_id),
                destroy_failures_before_success: AtomicUsize::new(destroy_failures_before_success),
                destroy_calls: Arc::clone(&destroy_calls),
            },
            destroy_calls,
        )
    }
}

impl KeyCustody for FlakyCustody {
    fn kind(&self) -> CustodyKind {
        self.inner.kind()
    }
    fn root_id(&self) -> RootId {
        self.inner.root_id()
    }
    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        self.inner.wrap_store_key(key)
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.inner.create_root()
    }
    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        self.inner.unwrap_store_key(wrapped)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.destroy_calls.fetch_add(1, Ordering::SeqCst);
        let remaining = self.destroy_failures_before_success.load(Ordering::SeqCst);
        if remaining > 0 {
            self.destroy_failures_before_success
                .store(remaining - 1, Ordering::SeqCst);
            return Err(CustodyError::RootLocked(-25308));
        }
        self.inner.destroy_root()
    }
}

#[test]
fn shred_under_a_concurrent_reader_leaves_nothing_recoverable() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let (expired, _) = store
        .write_segment_with_expiry(&mut Cursor::new(b"SHREDDED-BUT-RECOVERED"), 0, 1)
        .unwrap();
    store.write_segment(&mut Cursor::new(b"live")).unwrap();
    let cipher = std::fs::read(segment_path(dir.path(), expired.segment_id)).unwrap();
    let store_key = *store.store_key().unwrap().expose();

    let reader = Connection::open(dir.path().join("store.db")).unwrap();
    reader.busy_timeout(Duration::ZERO).unwrap();
    reader
        .execute_batch(&format!("PRAGMA key = \"x'{}'\";", hex(&store_key)))
        .unwrap();
    let snapshot = reader.unchecked_transaction().ok().filter(|tx| {
        tx.query_row("SELECT count(*) FROM segment_keys", [], |r| {
            r.get::<_, i64>(0)
        })
        .is_ok()
    });
    assert!(
        snapshot.is_none(),
        "a second connection read the store while it was open"
    );

    let report = store.crypto_shred_expired().unwrap();
    assert_eq!(report.segments_shredded, 1);

    let from_snapshot = snapshot.as_ref().and_then(|tx| {
        tx.query_row(
            "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
            [expired.segment_id.simple().to_string()],
            |r| r.get::<_, Option<Vec<u8>>>(0),
        )
        .unwrap_or(None)
    });
    let from_main_file =
        wrapped_key_in_copy_of_main_file_without_wal(dir.path(), &store_key, expired.segment_id)
            .unwrap();
    assert_eq!(
        wal_len(dir.path()),
        0,
        "the shred reported success with frames still in the WAL"
    );
    assert!(
        !recoverable(&cipher, &store_key, from_snapshot),
        "a snapshot reader recovered the shredded segment"
    );
    assert!(
        !recoverable(&cipher, &store_key, from_main_file),
        "a copy of store.db recovered the shredded segment"
    );
}

#[test]
fn a_second_process_is_refused_even_after_the_lock_file_is_unlinked() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    drop(
        EncryptedStore::create(
            dir.path(),
            Box::new(DevFallbackCustody::new(root_id)),
            DEFAULT_RETENTION,
        )
        .unwrap(),
    );

    let mut child = hold_store_in_child(dir.path());
    wait_for(&dir.path().join("child-ready"), &mut child);
    let _ = std::fs::remove_file(dir.path().join("writer.lock"));

    let second = EncryptedStore::open(dir.path(), DEFAULT_RETENTION);

    std::fs::write(dir.path().join("child-stop"), b"").unwrap();
    assert!(child.wait().unwrap().success(), "the holding child failed");
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
    assert!(
        matches!(second, Err(StoreError::Locked(_))),
        "a second writer was admitted after the lock file was unlinked: {:?}",
        second.err()
    );
}

#[test]
fn a_held_store_answers_locked_before_its_custody_is_called() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    drop(
        EncryptedStore::create(
            dir.path(),
            Box::new(DevFallbackCustody::new(root_id)),
            DEFAULT_RETENTION,
        )
        .unwrap(),
    );

    let mut child = hold_store_in_child(dir.path());
    wait_for(&dir.path().join("child-ready"), &mut child);
    let _ = std::fs::remove_file(dir.path().join("writer.lock"));
    let mut key_file = KeyFile::read(dir.path()).unwrap();
    key_file.root_id = RootId::fresh();
    std::fs::write(
        KeyFile::path(dir.path()),
        serde_json::to_vec(&key_file).unwrap(),
    )
    .unwrap();
    let custody_calls = AtomicUsize::new(0);

    let second = EncryptedStore::open_with(dir.path(), DEFAULT_RETENTION, |kind, root_id| {
        custody_calls.fetch_add(1, Ordering::SeqCst);
        crate::custody::custody_for(kind, root_id)
    });

    std::fs::write(dir.path().join("child-stop"), b"").unwrap();
    assert!(child.wait().unwrap().success(), "the holding child failed");
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
    assert!(
        matches!(second, Err(StoreError::Locked(_))),
        "a held store with a root id that names no root answered {:?}",
        second.err()
    );
    assert_eq!(
        custody_calls.load(Ordering::SeqCst),
        0,
        "open called the custody of a held store"
    );
}

#[test]
fn hold_store_for_parent() {
    let Ok(dir) = std::env::var("HALVO_TEST_HOLD_STORE") else {
        println!("hold_store_for_parent: HALVO_TEST_HOLD_STORE is not set, so no parent asked for a holder");
        return;
    };
    let dir = PathBuf::from(dir);
    let store = EncryptedStore::open(&dir, DEFAULT_RETENTION).unwrap();
    std::fs::write(dir.join("child-ready"), b"").unwrap();
    let started = Instant::now();
    while !dir.join("child-stop").exists() {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "parent never stopped us"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(store);
}

fn hold_store_in_child(dir: &Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::hold_store_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_TEST_HOLD_STORE", dir)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}

fn wait_for(marker: &Path, child: &mut Child) {
    let started = Instant::now();
    while !marker.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited early with {status}");
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "child never signalled ready"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn delete_all_survives_a_transient_root_destroy_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    let (custody, destroy_calls) = FlakyCustody::new(root_id, 1);
    let mut store =
        EncryptedStore::create(dir.path(), Box::new(custody), DEFAULT_RETENTION).unwrap();
    store.write_segment(&mut Cursor::new(b"a")).unwrap();
    let key_file_backup = std::fs::read(KeyFile::path(dir.path())).unwrap();

    let first = store.crypto_shred_all();
    assert!(
        first.is_err(),
        "delete-all reported success with the root alive"
    );
    assert_eq!(destroy_calls.load(Ordering::SeqCst), 1);

    let retry = store.crypto_shred_all();
    assert!(
        retry.is_ok(),
        "delete-all could not be retried: {:?}",
        retry.err()
    );
    assert_eq!(destroy_calls.load(Ordering::SeqCst), 2);
    drop(store);

    std::fs::write(KeyFile::path(dir.path()), key_file_backup).unwrap();
    let reopened = EncryptedStore::open(dir.path(), DEFAULT_RETENTION);
    assert!(
        matches!(
            reopened,
            Err(StoreError::Custody(CustodyError::RootMissing))
        ),
        "a backup of store.key reopened a deleted store: {:?}",
        reopened.err()
    );
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
}

#[test]
fn a_failed_create_leaves_no_root_and_no_half_store() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    super::db::failpoint::arm("db_open");
    let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
    assert!(
        created.is_err(),
        "the create with a failing Db::open succeeded"
    );
    for name in ["store.key", "store.db", "store.db-wal", "store.db-shm"] {
        assert!(
            !store_dir.join(name).exists(),
            "a create that failed in Db::open left {name}"
        );
    }
    let roots = FileRoots::new(roots_dir, TestProtector::new(0x5a).0);
    assert!(
        !roots.root_exists(root_id).unwrap(),
        "a failed create left a root"
    );
}

#[test]
fn a_create_that_cannot_clear_its_directory_fails_and_makes_no_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("store.db")).unwrap();
    let root_id = RootId::fresh();
    let (custody, _) = FlakyCustody::new(root_id, 0);

    let result = EncryptedStore::create(dir.path(), Box::new(custody), DEFAULT_RETENTION);
    assert!(result.is_err());
    assert!(
        !KeyFile::path(dir.path()).exists(),
        "a failed create left store.key"
    );
    assert!(
        !dev_root_exists(root_id).unwrap(),
        "a failed create left its root alive"
    );
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
}

#[test]
fn a_create_that_fails_after_wrapping_destroys_the_root_it_made() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    let (custody, _) = FlakyCustody::new(root_id, 0);
    let custody = WrapThenFail(custody);

    let result = EncryptedStore::create(dir.path(), Box::new(custody), DEFAULT_RETENTION);
    assert!(result.is_err());
    assert!(
        !KeyFile::path(dir.path()).exists(),
        "a failed create left store.key"
    );
    assert!(
        !dir.path().join("store.db").exists(),
        "a failed create left store.db"
    );
    assert!(
        !dev_root_exists(root_id).unwrap(),
        "a failed create left its root alive"
    );
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
}

struct WrapThenFail(FlakyCustody);

impl KeyCustody for WrapThenFail {
    fn kind(&self) -> CustodyKind {
        self.0.kind()
    }
    fn root_id(&self) -> RootId {
        self.0.root_id()
    }
    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        self.0.wrap_store_key(key)?;
        Err(CustodyError::Crypto(
            "wrap failed after the root was made".to_string(),
        ))
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.0.create_root()
    }
    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        self.0.unwrap_store_key(wrapped)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.0.destroy_root()
    }
}

#[cfg(unix)]
#[test]
fn a_shred_whose_file_removal_fails_still_destroys_the_key_and_the_next_pass_removes_the_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let (expired, _) = store
        .write_segment_with_expiry(&mut Cursor::new(b"orphan-bytes"), 0, 1)
        .unwrap();
    let path = segment_path(dir.path(), expired.segment_id);
    let cipher = std::fs::read(&path).unwrap();
    let store_key = *store.store_key().unwrap().expose();
    let shard = path.parent().unwrap().to_path_buf();
    store.db().unwrap().checkpoint_truncate().unwrap();
    let before =
        wrapped_key_in_copy_of_main_file_without_wal(dir.path(), &store_key, expired.segment_id)
            .unwrap();
    assert!(
        recoverable(&cipher, &store_key, before),
        "the probe does not find the live key in store.db, so it cannot see one left behind"
    );
    std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o500)).unwrap();

    let first = store.crypto_shred_expired();

    let from_main_file =
        wrapped_key_in_copy_of_main_file_without_wal(dir.path(), &store_key, expired.segment_id);
    let wal_after_the_failed_pass = wal_len(dir.path());
    std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o700)).unwrap();
    let from_main_file = from_main_file.unwrap();
    assert!(path.exists(), "the fixture did not block the removal");
    assert!(
        first.is_err(),
        "the pass reported success with the file still present"
    );
    assert_eq!(
        wal_after_the_failed_pass, 0,
        "the failed pass left frames in the WAL"
    );
    assert!(
        !recoverable(&cipher, &store_key, from_main_file),
        "the key of a segment whose file removal failed is still in store.db"
    );
    assert_eq!(live_key_rows(&store), 0);

    store.crypto_shred_expired().unwrap();
    assert!(
        !path.exists(),
        "the next pass did not remove the orphan file"
    );
}

#[test]
fn delete_all_leaves_a_directory_a_fresh_create_can_use() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    store.write_segment(&mut Cursor::new(b"a")).unwrap();
    store.crypto_shred_all().unwrap();
    drop(store);
    assert!(
        !dir.path().join("store.db").exists(),
        "delete-all left store.db"
    );
    assert!(
        !dir.path().join("segments").exists(),
        "delete-all left segments/"
    );

    let fresh = super::db::test_support::make_store(dir.path());
    assert!(fresh.db().is_ok());
}

struct DyingReader {
    remaining: usize,
}

impl Read for DyingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("source died mid-segment"));
        }
        let n = buf.len().min(self.remaining);
        buf[..n].fill(0xAB);
        self.remaining -= n;
        Ok(n)
    }
}

struct CountingSink(u64);

impl Write for CountingSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&p) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                out.push(entry.path());
            }
        }
    }
    out
}

#[test]
fn a_failed_write_commits_no_key_and_leaves_no_readable_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let before = store.size_bytes().unwrap();

    let result = store.write_segment(&mut DyingReader {
        remaining: CHUNK * 2 + 17,
    });
    assert!(result.is_err());

    assert_eq!(
        live_key_rows(&store),
        0,
        "a failed write left a live key row"
    );
    let leftovers = files_under(&dir.path().join("segments"));
    assert!(
        leftovers.is_empty(),
        "a failed write left files: {leftovers:?}"
    );
    let grown = store.size_bytes().unwrap() - before;
    assert!(
        grown < CHUNK as u64,
        "the store grew by {grown} bytes after a failed write of two chunks; only the \
         tombstone row's database page may remain"
    );
}

#[test]
fn a_truncated_segment_yields_no_plaintext_and_reports_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let plaintext = vec![7u8; CHUNK * 3 + 5];
    let (seg_ref, _) = store.write_segment(&mut Cursor::new(&plaintext)).unwrap();
    let path = segment_path(dir.path(), seg_ref.segment_id);
    let full = std::fs::read(&path).unwrap();

    let truncated_at = full.len() - (CHUNK + 5 + 16);
    std::fs::write(&path, &full[..truncated_at]).unwrap();
    let mut sink = CountingSink(0);
    let truncated = store.read_segment(seg_ref.segment_id, &mut sink);
    assert!(
        matches!(truncated, Err(StoreError::Corrupt(_))),
        "truncation reported as {truncated:?}"
    );
    assert_eq!(
        sink.0, 0,
        "a truncated segment emitted {} plaintext bytes",
        sink.0
    );

    let mut tampered = full.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    std::fs::write(&path, &tampered).unwrap();
    let mut sink = CountingSink(0);
    let tail = store.read_segment(seg_ref.segment_id, &mut sink);
    assert!(
        matches!(tail, Err(StoreError::Corrupt(_))),
        "tail tamper reported as {tail:?}"
    );
    assert_eq!(
        sink.0, 0,
        "a tail-tampered segment emitted {} plaintext bytes",
        sink.0
    );

    std::fs::write(&path, &full).unwrap();
    let mut out = Vec::new();
    store.read_segment(seg_ref.segment_id, &mut out).unwrap();
    assert_eq!(out, plaintext);
}

#[test]
fn keyless_segment_files_and_stray_temp_files_are_removed_by_the_next_pass() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let (seg_ref, _) = store
        .write_segment(&mut Cursor::new(b"soon orphaned"))
        .unwrap();
    let (live, _) = store.write_segment(&mut Cursor::new(b"live")).unwrap();
    let orphan = segment_path(dir.path(), seg_ref.segment_id);
    store
        .db()
        .unwrap()
        .connection()
        .execute(
            "UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = ?1",
            [seg_ref.segment_id.simple().to_string()],
        )
        .unwrap();
    let stray = orphan.with_extension("seg.tmp");
    std::fs::write(&stray, b"half-written").unwrap();
    let unknown = segment_path(dir.path(), Uuid::now_v7());
    std::fs::create_dir_all(unknown.parent().unwrap()).unwrap();
    std::fs::write(&unknown, b"no key row at all").unwrap();

    store.crypto_shred_expired().unwrap();

    assert!(
        !orphan.exists(),
        "the shred pass left a keyless segment file"
    );
    assert!(!stray.exists(), "the shred pass left a temp file");
    assert!(
        !unknown.exists(),
        "the shred pass left a file with no key row"
    );
    assert!(segment_path(dir.path(), live.segment_id).exists());
}

#[test]
fn a_wrong_store_key_opens_as_database_unreadable_and_a_shredded_segment_reads_as_sealed() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let (seg_ref, _) = store
        .write_segment_with_expiry(&mut Cursor::new(b"gone"), 0, 1)
        .unwrap();
    store.crypto_shred_expired().unwrap();
    assert!(matches!(
        store.read_segment(seg_ref.segment_id, &mut Vec::new()),
        Err(StoreError::Sealed)
    ));
    drop(store);

    let other = tempfile::tempdir().unwrap();
    let other_root = RootId::fresh();
    drop(
        EncryptedStore::create(
            other.path(),
            Box::new(DevFallbackCustody::new(other_root)),
            DEFAULT_RETENTION,
        )
        .unwrap(),
    );
    std::fs::copy(KeyFile::path(other.path()), KeyFile::path(dir.path())).unwrap();
    let opened = EncryptedStore::open(dir.path(), DEFAULT_RETENTION);
    DevFallbackCustody::new(other_root).destroy_root().unwrap();
    assert!(
        matches!(opened, Err(StoreError::DatabaseUnreadable)),
        "a wrong store key opened as {:?}",
        opened.err()
    );
}

fn files_and_lengths(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut files: Vec<(PathBuf, u64)> = files_under(dir)
        .into_iter()
        .map(|path| {
            let len = std::fs::metadata(&path).unwrap().len();
            (path, len)
        })
        .collect();
    files.sort();
    files
}

fn open_over_file_root(
    store_dir: &Path,
    roots_dir: &Path,
    root_id: RootId,
) -> StoreResult<EncryptedStore> {
    EncryptedStore::open_with(store_dir, DEFAULT_RETENTION, |_, _| {
        Ok(Box::new(file_root_custody(
            roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )))
    })
}

type DamageToTheDatabase = fn(&Path, &Path, RootId);

fn cut_store_db_to(store_dir: &Path, len: u64) {
    std::fs::File::options()
        .write(true)
        .open(store_dir.join("store.db"))
        .unwrap()
        .set_len(len)
        .unwrap();
}

fn answers_after_damage(cases: &[(&str, DamageToTheDatabase)], expected: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for (case, damage) in cases {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
        damage(&store_dir, &roots_dir, root_id);
        let before = files_and_lengths(&store_dir);
        let answer = match open_over_file_root(&store_dir, &roots_dir, root_id) {
            Ok(mut store) => {
                let read = store.read_segment(segment_id, &mut Vec::new());
                let pass = store.crypto_shred_expired();
                drop(store);
                format!(
                    "Ok, then read_segment {read:?}, the next expiry pass {pass:?} and the live \
                     segment file left after it: {}",
                    segment_path(&store_dir, segment_id).exists()
                )
            }
            Err(e) => format!("{e:?}"),
        };
        let after = files_and_lengths(&store_dir);
        if answer != expected {
            problems.push(format!("{case}: open answered {answer} in place of {expected}"));
        }
        if after != before {
            problems.push(format!(
                "{case}: open changed the files of the store from {before:?} to {after:?}"
            ));
        }
    }
    problems
}

#[test]
fn open_answers_a_missing_or_empty_database_as_database_missing_and_makes_no_file() {
    let problems = answers_after_damage(
        &[
            ("store.db removed", |store_dir, _, _| {
                std::fs::remove_file(store_dir.join("store.db")).unwrap()
            }),
            ("store.db cut to 0 bytes", |store_dir, _, _| {
                cut_store_db_to(store_dir, 0)
            }),
        ],
        "DatabaseMissing",
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn open_answers_a_database_that_its_key_does_not_open_as_database_unreadable() {
    let problems = answers_after_damage(
        &[
            ("store.db cut to 1000 bytes", |store_dir, _, _| {
                cut_store_db_to(store_dir, 1000)
            }),
            ("store.db cut to 4096 bytes", |store_dir, _, _| {
                cut_store_db_to(store_dir, 4096)
            }),
            (
                "store.key holds the wrap of another store key under the same root",
                |store_dir, roots_dir, root_id| {
                    use base64::engine::general_purpose::STANDARD;
                    use base64::Engine;

                    let custody = file_root_custody(roots_dir, TestProtector::new(0x5a).0, root_id);
                    let another = custody.wrap_store_key(&StoreKey::random().unwrap()).unwrap();
                    let mut key_file = KeyFile::read(store_dir).unwrap();
                    key_file.wrapped_store_key_b64 = STANDARD.encode(&another.0);
                    std::fs::write(
                        KeyFile::path(store_dir),
                        serde_json::to_vec(&key_file).unwrap(),
                    )
                    .unwrap();
                },
            ),
            ("byte 100 of store.db flipped", |store_dir, _, _| {
                let path = store_dir.join("store.db");
                let mut bytes = std::fs::read(&path).unwrap();
                bytes[100] ^= 0xff;
                std::fs::write(&path, bytes).unwrap();
            }),
        ],
        "DatabaseUnreadable",
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[cfg(unix)]
struct PatternReader {
    remaining: u64,
}

#[cfg(unix)]
impl Read for PatternReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (buf.len() as u64).min(self.remaining) as usize;
        for (i, b) in buf[..n].iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        self.remaining -= n as u64;
        Ok(n)
    }
}

#[cfg(unix)]
fn rss_kib() -> u64 {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[cfg(unix)]
#[test]
#[ignore = "run with: cargo test --release -p halvo-capture --lib peak_rss -- --ignored --nocapture"]
fn peak_rss_stays_bounded_for_a_512_mib_segment() {
    const SEGMENT_BYTES: u64 = 512 * 1024 * 1024;
    const BOUND_KIB: u64 = 64 * 1024;
    const SAMPLE_EVERY: Duration = Duration::from_millis(20);

    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let before = rss_kib();
    let peak = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicUsize::new(0));
    let sampler = {
        let (peak, stop) = (Arc::clone(&peak), Arc::clone(&stop));
        std::thread::spawn(move || {
            while stop.load(Ordering::SeqCst) == 0 {
                peak.fetch_max(rss_kib() as usize, Ordering::SeqCst);
                std::thread::sleep(SAMPLE_EVERY);
            }
        })
    };

    let (seg_ref, _) = store
        .write_segment(&mut PatternReader {
            remaining: SEGMENT_BYTES,
        })
        .unwrap();
    let after_write = rss_kib();
    let mut sink = CountingSink(0);
    store.read_segment(seg_ref.segment_id, &mut sink).unwrap();
    let after_read = rss_kib();
    stop.store(1, Ordering::SeqCst);
    sampler.join().unwrap();
    let peak = (peak.load(Ordering::SeqCst) as u64)
        .max(after_write)
        .max(after_read);

    println!(
        "RSS segment_bytes={SEGMENT_BYTES} read_back_bytes={} rss_before_kib={before} \
         rss_after_write_kib={after_write} rss_after_read_kib={after_read} \
         rss_peak_kib={peak} peak_delta_kib={} bound_kib={BOUND_KIB}",
        sink.0,
        peak - before
    );
    assert_eq!(seg_ref.byte_len, SEGMENT_BYTES);
    assert_eq!(sink.0, SEGMENT_BYTES);
    assert!(
        peak - before <= BOUND_KIB,
        "peak RSS grew by {} KiB",
        peak - before
    );
}

#[cfg(unix)]
#[test]
fn a_dropped_store_releases_the_writer_lock_while_a_duplicate_of_its_descriptor_stays_open() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    let store = EncryptedStore::create(
        dir.path(),
        Box::new(DevFallbackCustody::new(root_id)),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let held_as_a_spawned_child_holds_it =
        super::db::test_support::duplicate_writer_lock_descriptor(&store);

    drop(store);
    let reopened = EncryptedStore::open(dir.path(), DEFAULT_RETENTION);

    drop(held_as_a_spawned_child_holds_it);
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
    assert!(
        reopened.is_ok(),
        "the store did not reopen after its only writer was dropped: {:?}",
        reopened.err()
    );
}

#[cfg(unix)]
#[test]
fn no_open_is_refused_as_locked_while_other_threads_spawn_children() {
    use std::sync::atomic::AtomicBool;

    const SPAWNING_THREADS: usize = 4;
    const OPEN_THEN_DROP_CYCLES: usize = 200;

    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    drop(
        EncryptedStore::create(
            dir.path(),
            Box::new(DevFallbackCustody::new(root_id)),
            DEFAULT_RETENTION,
        )
        .unwrap(),
    );

    let stop = Arc::new(AtomicBool::new(false));
    let children_spawned = Arc::new(AtomicUsize::new(0));
    let spawners: Vec<_> = (0..SPAWNING_THREADS)
        .map(|_| {
            let (stop, children_spawned) = (Arc::clone(&stop), Arc::clone(&children_spawned));
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let ran = Command::new("/usr/bin/true").status();
                    if ran.is_ok_and(|status| status.success()) {
                        children_spawned.fetch_add(1, Ordering::SeqCst);
                    }
                }
            })
        })
        .collect();

    let mut refused_as_locked = 0;
    let mut other_errors = Vec::new();
    for _ in 0..OPEN_THEN_DROP_CYCLES {
        match EncryptedStore::open(dir.path(), DEFAULT_RETENTION) {
            Ok(store) => drop(store),
            Err(StoreError::Locked(_)) => refused_as_locked += 1,
            Err(e) => other_errors.push(e),
        }
    }

    stop.store(true, Ordering::SeqCst);
    for spawner in spawners {
        spawner.join().unwrap();
    }
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
    let children_spawned = children_spawned.load(Ordering::SeqCst);
    assert!(
        children_spawned > 0,
        "no child was spawned, so no open ran beside a spawn"
    );
    assert!(
        other_errors.is_empty(),
        "opens failed for a reason other than Locked: {other_errors:?}"
    );
    assert_eq!(
        refused_as_locked, 0,
        "{refused_as_locked} of {OPEN_THEN_DROP_CYCLES} opens were refused as Locked with no \
         other writer, while {children_spawned} children were spawned"
    );
}

fn file_root_custody<P: Protector>(
    roots_dir: &Path,
    protector: P,
    root_id: RootId,
) -> FileRootCustody<P> {
    FileRootCustody {
        roots: FileRoots::new(roots_dir.to_path_buf(), protector),
        root_id,
    }
}

fn file_root_store(dir: &Path) -> (PathBuf, PathBuf, RootId, Uuid) {
    let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
    let root_id = RootId::fresh();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let mut store =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
    let (segment, _) = store
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    (store_dir, roots_dir, root_id, segment.segment_id)
}

struct FailingUnwrap {
    root_id: RootId,
    error: fn() -> CustodyError,
}

impl KeyCustody for FailingUnwrap {
    fn kind(&self) -> CustodyKind {
        CustodyKind::Dpapi
    }
    fn root_id(&self) -> RootId {
        self.root_id
    }
    fn wrap_store_key(&self, _key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        Err((self.error)())
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        Err((self.error)())
    }
    fn unwrap_store_key(&self, _wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        Err((self.error)())
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        Ok(())
    }
}

fn open_answer(error: fn() -> CustodyError) -> StoreError {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, _, root_id, _) = file_root_store(dir.path());
    EncryptedStore::open_with(&store_dir, DEFAULT_RETENTION, |_, _| {
        Ok(Box::new(FailingUnwrap { root_id, error }))
    })
    .err()
    .expect("open succeeded over a custody whose unwrap fails")
}

macro_rules! open_passes_each_custody_error_through {
    ($($name:ident: $error:expr => $pattern:pat,)*) => {$(
        #[test]
        fn $name() {
            let answer = open_answer($error);
            assert!(
                matches!(answer, StoreError::Custody($pattern)),
                "open answered {answer:?}"
            );
        }
    )*};
}

open_passes_each_custody_error_through! {
    open_answers_a_missing_root_as_root_missing:
        || CustodyError::RootMissing => CustodyError::RootMissing,
    open_answers_a_root_of_the_wrong_length_as_root_corrupt:
        || CustodyError::RootCorrupt => CustodyError::RootCorrupt,
    open_answers_a_refused_root_as_root_denied:
        || CustodyError::RootDenied => CustodyError::RootDenied,
    open_answers_a_root_state_that_can_clear_as_root_locked_with_its_code:
        || CustodyError::RootLocked(-2146893813) => CustodyError::RootLocked(-2146893813),
    open_answers_a_wrap_that_does_not_open_as_wrap_auth_failed:
        || CustodyError::WrapAuthFailed => CustodyError::WrapAuthFailed,
    open_answers_another_platform_code_as_os_with_its_code:
        || CustodyError::Os(Some(5)) => CustodyError::Os(Some(5)),
    open_answers_a_permanent_unwrap_failure_as_crypto:
        || CustodyError::Crypto("permanent".to_string()) => CustodyError::Crypto(_),
    open_answers_a_custody_kind_this_build_lacks_as_custody_kind_unsupported:
        || CustodyError::CustodyKindUnsupported(CustodyKind::SecureEnclave)
            => CustodyError::CustodyKindUnsupported(CustodyKind::SecureEnclave),
    open_answers_an_unbuilt_custody_call_as_not_implemented:
        || CustodyError::NotImplemented("unbuilt") => CustodyError::NotImplemented(_),
}

fn open_with_recorded_kind(kind: CustodyKind) -> StoreResult<EncryptedStore> {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, _, _, _) = file_root_store(dir.path());
    let mut key_file = KeyFile::read(&store_dir).unwrap();
    key_file.custody_kind = kind;
    std::fs::write(
        KeyFile::path(&store_dir),
        serde_json::to_vec(&key_file).unwrap(),
    )
    .unwrap();
    EncryptedStore::open(&store_dir, DEFAULT_RETENTION)
}

#[test]
fn a_key_file_of_a_custody_kind_this_build_lacks_opens_as_custody_kind_unsupported() {
    let enclave = open_with_recorded_kind(CustodyKind::SecureEnclave);
    assert!(
        matches!(
            enclave,
            Err(StoreError::Custody(CustodyError::CustodyKindUnsupported(
                CustodyKind::SecureEnclave
            )))
        ),
        "secure_enclave: {:?}",
        enclave.err()
    );
    #[cfg(not(target_os = "windows"))]
    {
        let dpapi = open_with_recorded_kind(CustodyKind::Dpapi);
        assert!(
            matches!(
                dpapi,
                Err(StoreError::Custody(CustodyError::CustodyKindUnsupported(
                    CustodyKind::Dpapi
                )))
            ),
            "dpapi: {:?}",
            dpapi.err()
        );
    }
}

#[test]
fn an_empty_a_zeroed_and_a_garbled_key_file_open_as_key_file_unparseable() {
    for (case, bytes) in [
        ("empty", Vec::new()),
        ("zeroed", vec![0u8; 232]),
        ("garbled", b"{\"version\": 1, \"custody_kind\"".to_vec()),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, _, _, _) = file_root_store(dir.path());
        std::fs::write(KeyFile::path(&store_dir), &bytes).unwrap();
        let opened = EncryptedStore::open(&store_dir, DEFAULT_RETENTION);
        assert!(
            matches!(opened, Err(StoreError::KeyFileUnparseable)),
            "{case} key file: {:?}",
            opened.err()
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, _, _, _) = file_root_store(dir.path());
    let mut key_file = KeyFile::read(&store_dir).unwrap();
    key_file.wrapped_store_key_b64 = "!!!not base64!!!".to_string();
    std::fs::write(
        KeyFile::path(&store_dir),
        serde_json::to_vec(&key_file).unwrap(),
    )
    .unwrap();
    let opened = EncryptedStore::open(&store_dir, DEFAULT_RETENTION);
    assert!(
        matches!(opened, Err(StoreError::KeyFileUnparseable)),
        "a wrapped key that is not base64: {:?}",
        opened.err()
    );
}

#[test]
fn a_wrapped_store_key_that_decodes_to_another_length_than_a_wrap_opens_as_key_file_unparseable() {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let written = KeyFile::read(&store_dir).unwrap();
    let wrap = STANDARD.decode(&written.wrapped_store_key_b64).unwrap();
    assert_eq!(wrap.len(), 72, "a wrap of the store key is not 72 bytes");
    let mut one_byte_changed = wrap.clone();
    one_byte_changed[40] ^= 0x01;
    let mut one_byte_more = wrap.clone();
    one_byte_more.push(0x00);
    let cases: [(&str, Vec<u8>, &str); 8] = [
        ("empty", Vec::new(), "KeyFileUnparseable"),
        ("3 bytes", wrap[..3].to_vec(), "KeyFileUnparseable"),
        ("cut to 10 bytes", wrap[..10].to_vec(), "KeyFileUnparseable"),
        ("the nonce alone", wrap[..24].to_vec(), "KeyFileUnparseable"),
        ("one byte short", wrap[..71].to_vec(), "KeyFileUnparseable"),
        ("one byte more", one_byte_more, "KeyFileUnparseable"),
        (
            "a wrap with one byte changed",
            one_byte_changed,
            "Custody(WrapAuthFailed)",
        ),
        ("the wrap itself", wrap.clone(), "opens"),
    ];
    let mut problems = Vec::new();
    for (case, field, expected) in cases {
        let mut key_file = KeyFile::read(&store_dir).unwrap();
        key_file.wrapped_store_key_b64 = STANDARD.encode(&field);
        std::fs::write(
            KeyFile::path(&store_dir),
            serde_json::to_vec(&key_file).unwrap(),
        )
        .unwrap();
        let opened = EncryptedStore::open_with(&store_dir, DEFAULT_RETENTION, |_, _| {
            Ok(Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        });
        let answer = match &opened {
            Ok(_) => "opens".to_string(),
            Err(e) => format!("{e:?}"),
        };
        drop(opened);
        if answer != expected {
            problems.push(format!(
                "a wrapped-key field of {} bytes ({case}) answered {answer} in place of {expected}",
                field.len()
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[cfg(unix)]
fn with_mode_000<T>(path: &Path, action: impl FnOnce() -> T) -> T {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let answer = action();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    answer
}

#[cfg(unix)]
#[test]
fn a_key_file_that_exists_and_cannot_be_read_opens_as_io_permission_denied() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let opened = with_mode_000(&KeyFile::path(&store_dir), || {
        EncryptedStore::open_with(&store_dir, DEFAULT_RETENTION, |_, _| {
            Ok(Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        })
        .map(drop)
    });
    assert!(
        matches!(opened, Err(StoreError::Io(io::ErrorKind::PermissionDenied))),
        "a store.key of mode 000 answered {opened:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_segment_file_that_exists_and_cannot_be_read_reads_as_io_permission_denied() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    let read = with_mode_000(&segment_path(&store_dir, segment_id), || {
        store.read_segment(segment_id, &mut Vec::new())
    });
    assert!(
        matches!(read, Err(StoreError::Io(io::ErrorKind::PermissionDenied))),
        "a segment file of mode 000 answered {read:?}"
    );
}

#[test]
fn delete_all_zeroes_the_store_key_file_through_every_name_before_it_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let second_name = dir.path().join("a-second-name-of-store.key");
    std::fs::hard_link(KeyFile::path(&store_dir), &second_name).unwrap();
    let before = std::fs::read(&second_name).unwrap();
    let mut store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    store.crypto_shred_all().unwrap();
    drop(store);
    let after = std::fs::read(&second_name).unwrap();
    assert!(
        !KeyFile::path(&store_dir).exists(),
        "delete-all left store.key"
    );
    assert_eq!(
        after.len(),
        before.len(),
        "the second name of store.key changed its length"
    );
    let nonzero = after.iter().filter(|&&b| b != 0).count();
    assert_eq!(
        nonzero, 0,
        "store.key was removed with {nonzero} of its bytes not zeroed"
    );
}

#[test]
fn a_write_ahead_log_left_in_the_directory_is_gone_after_delete_all_and_after_a_failed_create() {
    let log = b"a write-ahead log that outlived its database";
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(&dir.path().join("deleted"));
    let mut store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    super::db::failpoint::arm("key_file_destroy");
    let stopped = store.crypto_shred_all();
    assert!(
        stopped.is_err(),
        "the delete-all did not stop at its key file"
    );
    std::fs::write(store_dir.join("store.db-wal"), log).unwrap();
    store.crypto_shred_all().unwrap();
    drop(store);
    let after_delete_all = store_dir.join("store.db-wal").exists();

    let failed_dir = dir.path().join("failed");
    std::fs::create_dir_all(failed_dir.join("store.db-shm")).unwrap();
    std::fs::write(failed_dir.join("store.db-wal"), log).unwrap();
    let created = EncryptedStore::create(
        &failed_dir,
        Box::new(file_root_custody(
            &dir.path().join("roots-of-the-failed-create"),
            TestProtector::new(0x5a).0,
            RootId::fresh(),
        )),
        DEFAULT_RETENTION,
    );
    let create_failed = created.is_err();
    drop(created);
    let after_failed_create = failed_dir.join("store.db-wal").exists();

    assert!(
        !after_delete_all,
        "a write-ahead log that was in the directory outlived delete-all"
    );
    assert!(
        create_failed,
        "a create over a store.db-shm that is a directory succeeded"
    );
    assert!(
        !after_failed_create,
        "a write-ahead log that was in the directory outlived a failed create"
    );
}

#[test]
fn a_segment_write_whose_key_commit_fails_leaves_no_segment_file() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    store
        .db()
        .unwrap()
        .connection()
        .execute_batch(
            "CREATE TEMP TRIGGER refuse_the_key_commit BEFORE UPDATE OF wrapped_key ON segment_keys \
             BEGIN SELECT RAISE(ABORT, 'the key commit is refused'); END;",
        )
        .unwrap();
    let mut before = files_under(&segment::segments_root(&store_dir));
    let written = store.write_segment(&mut Cursor::new(b"a segment whose key never commits"));
    let mut after = files_under(&segment::segments_root(&store_dir));
    before.sort();
    after.sort();
    assert!(
        written.is_err(),
        "a write whose key commit was refused succeeded"
    );
    assert_eq!(
        after, before,
        "a write whose key commit failed left a file under segments/"
    );
}

#[test]
fn a_create_over_a_database_and_segment_files_without_a_key_file_starts_from_nothing() {
    let mut problems = Vec::new();
    for keep_the_database in [true, false] {
        let case = if keep_the_database {
            "store.db and a segment file"
        } else {
            "a segment file"
        };
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, _, segment_id) = file_root_store(dir.path());
        let old_segment = segment_path(&store_dir, segment_id);
        KeyFile::destroy(&store_dir).unwrap();
        if !keep_the_database {
            super::db::Db::destroy(&store_dir).unwrap();
        }
        let created = EncryptedStore::create(
            &store_dir,
            Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                RootId::fresh(),
            )),
            DEFAULT_RETENTION,
        );
        match &created {
            Err(e) => problems.push(format!("a create over {case} answered {e:?}")),
            Ok(store) => {
                let keys: i64 = store
                    .db()
                    .unwrap()
                    .connection()
                    .query_row("SELECT count(*) FROM segment_keys", [], |r| r.get(0))
                    .unwrap();
                if keys != 0 {
                    problems.push(format!("a create over {case} holds {keys} segment keys"));
                }
                let read = store.read_segment(segment_id, &mut Vec::new());
                if !matches!(read, Err(StoreError::Sealed)) {
                    problems.push(format!(
                        "a create over {case} read the old segment: {read:?}"
                    ));
                }
            }
        }
        if old_segment.exists() {
            problems.push(format!("a create over {case} left the old segment file"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn reopened_file_root_store(store_dir: &Path, roots_dir: &Path, root_id: RootId) -> EncryptedStore {
    EncryptedStore::open_with(store_dir, DEFAULT_RETENTION, |_, _| {
        Ok(Box::new(file_root_custody(
            roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )))
    })
    .unwrap()
}

#[test]
fn a_segment_file_replaced_by_a_directory_reads_as_io() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let path = segment_path(&store_dir, segment_id);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    let read = store.read_segment(segment_id, &mut Vec::new());
    #[cfg(unix)]
    assert!(
        matches!(read, Err(StoreError::Io(io::ErrorKind::IsADirectory))),
        "{read:?}"
    );
    #[cfg(not(unix))]
    assert!(matches!(read, Err(StoreError::Io(_))), "{read:?}");
}

struct FullVolume;

impl Write for FullVolume {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::StorageFull.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_full_volume_under_the_reader_reads_as_io_storage_full() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    let read = store.read_segment(segment_id, &mut FullVolume);
    assert!(
        matches!(read, Err(StoreError::Io(io::ErrorKind::StorageFull))),
        "{read:?}"
    );
}

#[test]
fn a_full_database_answers_db_with_sqlite_full() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    {
        let conn = store.db().unwrap().connection();
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
        conn.query_row(&format!("PRAGMA max_page_count = {pages}"), [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap();
    }
    let mut answer = None;
    for i in 0..400u32 {
        if let Err(e) = store.write_segment(&mut Cursor::new(i.to_le_bytes())) {
            answer = Some(e);
            break;
        }
    }
    let full = halvo_sqlcipher::rusqlite::ffi::SQLITE_FULL;
    assert!(
        matches!(answer, Some(StoreError::Db(code)) if code & 0xff == full),
        "{answer:?}"
    );
}

#[test]
fn a_live_key_without_its_file_reads_as_segment_missing() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    std::fs::remove_file(segment_path(&store_dir, segment_id)).unwrap();
    let store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    let read = store.read_segment(segment_id, &mut Vec::new());
    assert!(
        matches!(read, Err(StoreError::SegmentMissing(id)) if id == segment_id),
        "{read:?}"
    );
}

fn open_over_protector(
    store_dir: &Path,
    roots_dir: &Path,
    root_id: RootId,
    protector: &Arc<TestProtector>,
) -> StoreResult<EncryptedStore> {
    EncryptedStore::open_with(store_dir, DEFAULT_RETENTION, |_, _| {
        Ok(Box::new(file_root_custody(
            roots_dir,
            Arc::clone(protector),
            root_id,
        )))
    })
}

#[test]
fn a_root_file_custody_answers_open_by_the_state_of_its_root() {
    const CAN_CLEAR: i32 = -2146893813;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());

    let (protector, _) = TestProtector::failing_once(0x5a, CustodyError::RootLocked(CAN_CLEAR));
    let protector = Arc::new(protector);
    let first = open_over_protector(&store_dir, &roots_dir, root_id, &protector);
    assert!(
        matches!(
            first,
            Err(StoreError::Custody(CustodyError::RootLocked(CAN_CLEAR)))
        ),
        "a protector failure that can clear answered {:?}",
        first.err()
    );
    let second = open_over_protector(&store_dir, &roots_dir, root_id, &protector);
    assert!(
        second.is_ok(),
        "the open after the failure cleared answered {:?}",
        second.err()
    );
    drop(second);

    let root_file =
        FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0).root_path(root_id);
    let blob = std::fs::read(&root_file).unwrap();
    std::fs::write(&root_file, vec![0u8; blob.len()]).unwrap();
    let (protector, unprotects) = TestProtector::new(0x5a);
    let zeroed = open_over_protector(&store_dir, &roots_dir, root_id, &Arc::new(protector));
    assert!(
        matches!(zeroed, Err(StoreError::Custody(CustodyError::RootMissing))),
        "a root file of zeros answered {:?}",
        zeroed.err()
    );
    assert_eq!(
        unprotects.load(Ordering::SeqCst),
        0,
        "the protector was called for a root file of zeros"
    );

    std::fs::write(&root_file, b"").unwrap();
    let (protector, unprotects) = TestProtector::new(0x5a);
    let empty = open_over_protector(&store_dir, &roots_dir, root_id, &Arc::new(protector));
    assert!(
        matches!(empty, Err(StoreError::Custody(CustodyError::RootMissing))),
        "an empty root file answered {:?}",
        empty.err()
    );
    assert_eq!(
        unprotects.load(Ordering::SeqCst),
        0,
        "the protector was called for an empty root file"
    );

    std::fs::write(&root_file, [7u8; 31]).unwrap();
    let short = open_over_protector(
        &store_dir,
        &roots_dir,
        root_id,
        &Arc::new(TestProtector::new(0x5a).0),
    );
    assert!(
        matches!(short, Err(StoreError::Custody(CustodyError::RootCorrupt))),
        "a root of 31 bytes answered {:?}",
        short.err()
    );

    std::fs::write(&root_file, [7u8; 33]).unwrap();
    let long = open_over_protector(
        &store_dir,
        &roots_dir,
        root_id,
        &Arc::new(TestProtector::new(0x5a).0),
    );
    assert!(
        matches!(long, Err(StoreError::Custody(CustodyError::RootCorrupt))),
        "a root of 33 bytes answered {:?}",
        long.err()
    );
}

struct DirectoryAtTheKeyFile {
    inner: FileRootCustody<TestProtector>,
    key_file: PathBuf,
}

impl KeyCustody for DirectoryAtTheKeyFile {
    fn kind(&self) -> CustodyKind {
        self.inner.kind()
    }
    fn root_id(&self) -> RootId {
        self.inner.root_id()
    }
    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        self.inner.wrap_store_key(key)
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.inner.create_root()?;
        std::fs::create_dir(&self.key_file).unwrap();
        Ok(())
    }
    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        self.inner.unwrap_store_key(wrapped)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.inner.destroy_root()
    }
}

#[test]
fn a_create_whose_key_file_can_be_neither_written_nor_removed_still_destroys_its_root() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh();
    let custody = DirectoryAtTheKeyFile {
        inner: file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id),
        key_file: KeyFile::path(&store_dir),
    };
    let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
    assert!(created.is_err(), "the create succeeded");
    let root_file = FileRoots::new(roots_dir, TestProtector::new(0x5a).0).root_path(root_id);
    assert!(!root_file.exists(), "a failed create left its root");
    assert!(
        !store_dir.join("store.db").exists(),
        "a failed create left store.db"
    );
}

#[cfg(unix)]
#[test]
fn a_root_file_this_process_may_not_read_answers_open_as_root_denied() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file =
        FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0).root_path(root_id);
    std::fs::set_permissions(&root_file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let denied = open_over_protector(
        &store_dir,
        &roots_dir,
        root_id,
        &Arc::new(TestProtector::new(0x5a).0),
    );
    std::fs::set_permissions(&root_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        matches!(denied, Err(StoreError::Custody(CustodyError::RootDenied))),
        "a root file of mode 000 answered {:?}",
        denied.err()
    );
}

struct KeyedRow {
    id: i64,
    wrapped: Vec<u8>,
}

fn keyed_rows(store: &EncryptedStore) -> Vec<KeyedRow> {
    let conn = store.db().unwrap().connection();
    let mut statement = conn
        .prepare(
            "SELECT id, wrapped_key FROM segment_keys WHERE wrapped_key IS NOT NULL ORDER BY id",
        )
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok(KeyedRow {
                id: row.get(0)?,
                wrapped: row.get(1)?,
            })
        })
        .unwrap();
    rows.map(|row| row.unwrap()).collect()
}

struct Leftovers {
    whole: usize,
    middle: usize,
    places: Vec<String>,
}

fn leftovers_in_raw_pages(store: &EncryptedStore, key: &[u8; 32], rows: &[&KeyedRow]) -> Leftovers {
    let images = pages::store_pages(store.dir(), key);
    let whole: Vec<&[u8]> = rows.iter().map(|row| row.wrapped.as_slice()).collect();
    let middle: Vec<&[u8]> = rows.iter().map(|row| &row.wrapped[24..56]).collect();
    let whole = pages::places_of(&images, &whole);
    let middle = pages::places_of(&images, &middle);
    let owners = pages::page_owners(store.db().unwrap().connection());
    let places = middle
        .iter()
        .flatten()
        .map(|(file, page)| match (*file, owners.get(page)) {
            ("store.db", Some(owner)) => format!("store.db page {page} of {owner}"),
            ("store.db", None) => format!("store.db page {page}, a free page"),
            (file, _) => format!("{file} slot of page {page}"),
        })
        .collect();
    Leftovers {
        whole: whole.iter().filter(|found| !found.is_empty()).count(),
        middle: middle.iter().filter(|found| !found.is_empty()).count(),
        places,
    }
}

fn copy_of_store_key(store: &EncryptedStore) -> zeroize::Zeroizing<[u8; 32]> {
    zeroize::Zeroizing::new(*store.store_key().unwrap().expose())
}

fn expire_rows_up_to(store: &EncryptedStore, last_id: i64) {
    store
        .db()
        .unwrap()
        .connection()
        .execute(
            "UPDATE segment_keys SET expires_at = 1 WHERE wrapped_key IS NOT NULL AND id <= ?1",
            [last_id],
        )
        .unwrap();
}

#[test]
fn after_a_shred_no_database_side_file_holds_the_wrapped_key_of_the_shredded_segment() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let journal_mode: String = store
        .db()
        .unwrap()
        .connection()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal_mode, "wal");
    let body = b"segment to shred ".repeat(100);
    store
        .write_segment_with_expiry(&mut Cursor::new(&body), 0, 1)
        .unwrap();
    store.write_segment(&mut Cursor::new(b"live")).unwrap();
    let key = copy_of_store_key(&store);
    let rows = keyed_rows(&store);
    assert_eq!(rows.len(), 2);

    assert_eq!(store.crypto_shred_expired().unwrap().segments_shredded, 1);

    let left = leftovers_in_raw_pages(&store, &key, &[&rows[0]]);
    assert_eq!(
        (left.whole, left.middle),
        (0, 0),
        "a database file holds the wrapped key of the shredded segment at {:?}",
        left.places
    );
    assert_eq!(
        leftovers_in_raw_pages(&store, &key, &[&rows[1]]).whole,
        1,
        "the page reader does not see the live wrapped key"
    );
    assert!(!dir.path().join("store.db-journal").exists());
}

#[test]
fn an_open_store_draws_its_random_bytes_from_the_os_entropy_hook_of_the_pinned_cipher() {
    let dir = tempfile::tempdir().unwrap();
    let store = super::db::test_support::make_store(dir.path());
    assert!(
        halvo_sqlcipher::os_entropy_hook_installed(),
        "LibTomCrypt's random source is not the OS entropy hook"
    );
    assert_eq!(store.db().unwrap().cipher_provider().unwrap(), "libtomcrypt");
}

#[test]
fn a_shredded_wrapped_key_is_gone_from_the_raw_database_pages() {
    const SEGMENTS: usize = 1200;
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    for i in 0..SEGMENTS {
        let expires_at = if i % 4 != 0 { 1 } else { i64::MAX as u64 };
        store
            .write_segment_with_expiry(&mut Cursor::new(vec![i as u8; 40]), 0, expires_at)
            .unwrap();
    }
    let key = copy_of_store_key(&store);
    let rows = keyed_rows(&store);
    assert_eq!(rows.len(), SEGMENTS);
    store.db().unwrap().checkpoint_truncate().unwrap();
    let all: Vec<&KeyedRow> = rows.iter().collect();
    let control = leftovers_in_raw_pages(&store, &key, &all);
    assert_eq!(
        control.whole, SEGMENTS,
        "the page reader does not see every live wrapped key before any shred"
    );

    let first_wave: Vec<&KeyedRow> = rows
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 4 != 0)
        .map(|(_, row)| row)
        .collect();
    let shredded = store.crypto_shred_expired().unwrap().segments_shredded;
    assert_eq!(shredded as usize, first_wave.len());
    let after_first = leftovers_in_raw_pages(&store, &key, &first_wave);
    assert_eq!(
        (after_first.whole, after_first.middle),
        (0, 0),
        "after the first pass, wrapped keys of shredded segments are in the raw pages: whole={} middle_32_bytes={} of={} at {:?}",
        after_first.whole,
        after_first.middle,
        first_wave.len(),
        after_first.places
    );

    let second_wave: Vec<&KeyedRow> = rows
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 16 != 0)
        .map(|(_, row)| row)
        .collect();
    let kept: Vec<&KeyedRow> = rows
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 16 == 0)
        .map(|(_, row)| row)
        .collect();
    store
        .db()
        .unwrap()
        .connection()
        .execute(
            "UPDATE segment_keys SET expires_at = 1 WHERE wrapped_key IS NOT NULL AND (id - 1) % 16 != 0",
            [],
        )
        .unwrap();
    store.crypto_shred_expired().unwrap();
    let after_second = leftovers_in_raw_pages(&store, &key, &second_wave);
    assert_eq!(
        (after_second.whole, after_second.middle),
        (0, 0),
        "after the second pass, wrapped keys of shredded segments are in the raw pages: whole={} middle_32_bytes={} of={} at {:?}",
        after_second.whole,
        after_second.middle,
        second_wave.len(),
        after_second.places
    );
    assert_eq!(
        leftovers_in_raw_pages(&store, &key, &kept).whole,
        kept.len(),
        "the page reader lost the live wrapped keys"
    );
}

#[test]
fn a_shredded_wrapped_key_is_gone_from_the_raw_database_pages_when_rows_expire_oldest_first() {
    const SEGMENTS: usize = 1200;
    const PER_PASS: usize = 50;
    const PASSES: usize = 20;
    const FAILED_WRITE_EVERY: usize = 9;
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    for i in 0..SEGMENTS {
        store
            .write_segment_with_expiry(&mut Cursor::new(vec![i as u8; 40]), 0, i64::MAX as u64)
            .unwrap();
        if (i + 1) % FAILED_WRITE_EVERY == 0 {
            let failed = store.write_segment_with_expiry(
                &mut DyingReader { remaining: 100 },
                0,
                i64::MAX as u64,
            );
            assert!(failed.is_err());
        }
    }
    let key = copy_of_store_key(&store);
    let rows = keyed_rows(&store);
    assert_eq!(rows.len(), SEGMENTS);
    store.db().unwrap().checkpoint_truncate().unwrap();
    let all: Vec<&KeyedRow> = rows.iter().collect();
    assert_eq!(
        leftovers_in_raw_pages(&store, &key, &all).whole,
        SEGMENTS,
        "the page reader does not see every live wrapped key before any shred"
    );

    let mut passes_with_leftovers = Vec::new();
    for pass in 1..=PASSES {
        let done = pass * PER_PASS;
        expire_rows_up_to(&store, rows[done - 1].id);
        store.crypto_shred_expired().unwrap();
        let shredded: Vec<&KeyedRow> = rows[..done].iter().collect();
        let found = leftovers_in_raw_pages(&store, &key, &shredded);
        if found.middle > 0 {
            passes_with_leftovers.push((pass, found.middle, found.places));
        }
    }
    assert!(
        passes_with_leftovers.is_empty(),
        "wrapped keys of shredded segments were in the raw pages after an expiry pass returned (pass, keys, places): {passes_with_leftovers:?}"
    );
}

fn tamper_pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| ((i as u64 * 31 + seed as u64 * 7) % 251) as u8)
        .collect()
}

fn chunk_spans(file_len: usize) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut at = BODY;
    while at < file_len {
        let end = (at + CIPHER_CHUNK_LEN).min(file_len);
        spans.push((at, end));
        at = end;
    }
    spans
}

fn rebuild(original: &[u8], spans: &[(usize, usize)], order: &[usize]) -> Vec<u8> {
    let mut rebuilt = original[..BODY].to_vec();
    for &i in order {
        rebuilt.extend_from_slice(&original[spans[i].0..spans[i].1]);
    }
    rebuilt
}

#[derive(Default)]
struct TamperTally {
    cases: usize,
    anomalies: Vec<String>,
}

impl TamperTally {
    fn case(
        &mut self,
        store: &EncryptedStore,
        id: Uuid,
        path: &Path,
        original: &[u8],
        changed: &[u8],
        label: &str,
    ) {
        if changed == original {
            return;
        }
        std::fs::write(path, changed).unwrap();
        let mut sink = CountingSink(0);
        let result = store.read_segment(id, &mut sink);
        self.cases += 1;
        if !(matches!(result, Err(StoreError::Corrupt(_))) && sink.0 == 0) {
            self.anomalies.push(format!(
                "{label}: answered {:?} after {} plaintext bytes",
                result.map(|_| ()),
                sink.0
            ));
        }
    }
}

fn truncation_points(file: &[u8], spans: &[(usize, usize)]) -> Vec<usize> {
    let mut cuts: Vec<usize> = if file.len() <= 200 {
        (0..file.len()).collect()
    } else {
        let mut cuts = vec![
            0,
            1,
            HEADER - 1,
            HEADER,
            HEADER + 1,
            BODY - 1,
            BODY,
            BODY + 1,
        ];
        for &(start, end) in spans {
            cuts.extend([
                start + 1,
                start + 15,
                start + 16,
                start + 17,
                (start + end) / 2,
                end.saturating_sub(17),
                end - 16,
                end - 1,
                end,
            ]);
        }
        cuts
    };
    cuts.retain(|&cut| cut < file.len());
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

#[test]
fn every_modified_segment_file_is_rejected_with_no_plaintext() {
    const CASES: usize = 1992;
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let mut tally = TamperTally::default();
    let sizes = [
        0usize,
        1,
        CHUNK - 1,
        CHUNK,
        CHUNK + 1,
        2 * CHUNK,
        3 * CHUNK + 5,
    ];
    for &len in &sizes {
        let every_bit = len == 0 || len == CHUNK + 1;
        let (plain_a, plain_b) = (tamper_pattern(len, 1), tamper_pattern(len, 2));
        let (ref_a, _) = store.write_segment(&mut Cursor::new(&plain_a)).unwrap();
        let (ref_b, _) = store.write_segment(&mut Cursor::new(&plain_b)).unwrap();
        let (id_a, id_b) = (ref_a.segment_id, ref_b.segment_id);
        let (path_a, path_b) = (
            segment_path(dir.path(), id_a),
            segment_path(dir.path(), id_b),
        );
        let a = std::fs::read(&path_a).unwrap();
        let b = std::fs::read(&path_b).unwrap();
        assert_eq!(a.len(), b.len());
        let spans = chunk_spans(a.len());
        let n = spans.len();
        let mut case =
            |label: String, changed: &[u8]| tally.case(&store, id_a, &path_a, &a, changed, &label);

        for cut in truncation_points(&a, &spans) {
            case(format!("len={len} truncated at {cut}"), &a[..cut]);
        }
        let (last_start, last_end) = spans[n - 1];
        let extensions: [(&str, Vec<u8>); 5] = [
            ("+1 byte", vec![0u8]),
            ("+16 bytes", vec![0u8; 16]),
            ("+copy of the last chunk", a[last_start..last_end].to_vec()),
            ("+copy of the body", a[BODY..].to_vec()),
            ("+one chunk of zeros", vec![0u8; CIPHER_CHUNK_LEN]),
        ];
        for (label, tail) in extensions {
            let mut extended = a.clone();
            extended.extend_from_slice(&tail);
            case(format!("len={len} {label}"), &extended);
        }
        if n >= 2 {
            let identity: Vec<usize> = (0..n).collect();
            let mut swapped = identity.clone();
            swapped.swap(0, 1);
            case(
                format!("len={len} chunks 0 and 1 swapped"),
                &rebuild(&a, &spans, &swapped),
            );
            let mut swapped = identity.clone();
            swapped.swap(0, n - 1);
            case(
                format!("len={len} first and last chunk swapped"),
                &rebuild(&a, &spans, &swapped),
            );
            let mut reversed = identity.clone();
            reversed.reverse();
            case(
                format!("len={len} chunks reversed"),
                &rebuild(&a, &spans, &reversed),
            );
            for i in 0..n {
                let mut duplicated = identity.clone();
                duplicated.insert(i, i);
                case(
                    format!("len={len} chunk {i} duplicated"),
                    &rebuild(&a, &spans, &duplicated),
                );
                let mut dropped = identity.clone();
                dropped.remove(i);
                case(
                    format!("len={len} chunk {i} dropped"),
                    &rebuild(&a, &spans, &dropped),
                );
            }
        }
        for i in [0usize, n - 1] {
            let mut spliced = a.clone();
            spliced[spans[i].0..spans[i].1].copy_from_slice(&b[spans[i].0..spans[i].1]);
            case(
                format!("len={len} chunk {i} from the other segment"),
                &spliced,
            );
        }
        let mut spliced = a.clone();
        spliced[HEADER..BODY].copy_from_slice(&b[HEADER..BODY]);
        case(format!("len={len} nonce from the other segment"), &spliced);
        let mut spliced = a.clone();
        spliced[..HEADER].copy_from_slice(&b[..HEADER]);
        case(format!("len={len} header from the other segment"), &spliced);
        let mut spliced = b.clone();
        spliced[BODY..].copy_from_slice(&a[BODY..]);
        case(
            format!("len={len} header and nonce of the other segment"),
            &spliced,
        );

        let bits_at = |offset: usize| -> Vec<u8> {
            if every_bit {
                (0..8).collect()
            } else {
                vec![(offset % 8) as u8]
            }
        };
        for offset in 0..BODY {
            for bit in bits_at(offset) {
                let mut flipped = a.clone();
                flipped[offset] ^= 1 << bit;
                case(
                    format!("len={len} header or nonce bit {bit} at {offset}"),
                    &flipped,
                );
            }
        }
        for (chunk, &(start, end)) in spans.iter().enumerate() {
            for offset in (end - TAG_LEN)..end {
                for bit in bits_at(offset) {
                    let mut flipped = a.clone();
                    flipped[offset] ^= 1 << bit;
                    case(
                        format!("len={len} chunk {chunk} tag bit {bit} at {offset}"),
                        &flipped,
                    );
                }
            }
            if end - TAG_LEN > start {
                for offset in [start, (start + end - TAG_LEN) / 2, end - TAG_LEN - 1] {
                    let mut flipped = a.clone();
                    flipped[offset] ^= 0x80;
                    case(
                        format!("len={len} chunk {chunk} ciphertext bit at {offset}"),
                        &flipped,
                    );
                }
            }
        }

        tally.case(
            &store,
            id_a,
            &path_a,
            &a,
            &b,
            &format!("len={len} file of the other segment under this id"),
        );
        tally.case(
            &store,
            id_b,
            &path_b,
            &b,
            &a,
            &format!("len={len} file of the first segment under the other id"),
        );

        std::fs::write(&path_a, &a).unwrap();
        std::fs::write(&path_b, &b).unwrap();
        for (id, plain) in [(id_a, &plain_a), (id_b, &plain_b)] {
            let mut out = Vec::new();
            store.read_segment(id, &mut out).unwrap();
            assert_eq!(&out, plain, "a restored original no longer reads back");
        }
    }
    assert_eq!(
        tally.anomalies,
        Vec::<String>::new(),
        "modified segment files that were not rejected as Corrupt with 0 bytes emitted"
    );
    assert_eq!(tally.cases, CASES, "the matrix ran another number of cases");
}

struct FreedBlockWatch;

static WATCHED_BLOCK: AtomicUsize = AtomicUsize::new(0);
static NONZERO_AT_FREE_PLUS_ONE: AtomicUsize = AtomicUsize::new(0);
static WATCH_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

unsafe impl std::alloc::GlobalAlloc for FreedBlockWatch {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        std::alloc::System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        std::alloc::System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        std::alloc::System.realloc(ptr, layout, new_size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        let address = ptr as usize;
        if WATCHED_BLOCK.load(Ordering::SeqCst) == address
            && WATCHED_BLOCK
                .compare_exchange(address, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            let bytes = std::slice::from_raw_parts(ptr, layout.size());
            let nonzero = bytes.iter().filter(|&&b| b != 0).count();
            NONZERO_AT_FREE_PLUS_ONE.store(nonzero + 1, Ordering::SeqCst);
        }
        std::alloc::System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static FREED_BLOCK_WATCH: FreedBlockWatch = FreedBlockWatch;

fn key_bytes_before_and_when_freed<T>(
    value: T,
    key_block: impl FnOnce(&T) -> *const u8,
) -> (usize, usize) {
    let _serial = WATCH_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let block = key_block(&value);
    let before = unsafe { std::slice::from_raw_parts(block, 32) }
        .iter()
        .filter(|&&b| b != 0)
        .count();
    NONZERO_AT_FREE_PLUS_ONE.store(0, Ordering::SeqCst);
    WATCHED_BLOCK.store(block as usize, Ordering::SeqCst);
    drop(value);
    WATCHED_BLOCK.store(0, Ordering::SeqCst);
    let seen = NONZERO_AT_FREE_PLUS_ONE.load(Ordering::SeqCst);
    assert!(
        seen > 0,
        "the block that held the key was not freed when its owner was dropped"
    );
    (before, seen - 1)
}

#[test]
fn a_dropped_segment_key_leaves_zeros_in_its_heap_block() {
    assert_eq!(
        size_of::<segment::SegmentKey>(),
        size_of::<usize>(),
        "a segment key holds more than a pointer, so a move copies key bytes"
    );
    let (before, at_free) =
        key_bytes_before_and_when_freed(segment::SegmentKey::random().unwrap(), |key| {
            key.expose().as_ptr()
        });
    assert!(before > 0, "the block showed no key bytes before the drop");
    assert_eq!(
        at_free, 0,
        "{at_free} of 32 bytes of a segment key were in its block when it was freed"
    );
}

#[test]
fn a_dropped_store_key_leaves_zeros_in_its_heap_block() {
    assert_eq!(
        size_of::<StoreKey>(),
        size_of::<usize>(),
        "a store key holds more than a pointer, so a move copies key bytes"
    );
    let (before, at_free) =
        key_bytes_before_and_when_freed(StoreKey::from_bytes([0xa5; 32]), |key| {
            key.expose().as_ptr()
        });
    assert_eq!(
        before, 32,
        "the block did not show the store key before the drop"
    );
    assert_eq!(
        at_free, 0,
        "{at_free} of 32 bytes of a store key were in its block when it was freed"
    );

    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    let store = EncryptedStore::create(
        dir.path(),
        Box::new(DevFallbackCustody::new(root_id)),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (before, at_free) = key_bytes_before_and_when_freed(store, |store| {
        store.store_key().unwrap().expose().as_ptr()
    });
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
    assert!(
        before > 0,
        "the block showed no store key bytes before the store was dropped"
    );
    assert_eq!(
        at_free, 0,
        "{at_free} of 32 bytes of the store key were in its block when the store was dropped"
    );
}

#[test]
fn no_file_of_a_dropped_store_holds_its_raw_store_key() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    let mut store = EncryptedStore::create(
        dir.path(),
        Box::new(DevFallbackCustody::new(root_id)),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let store_key = zeroize::Zeroizing::new(*store.store_key().unwrap().expose());
    store
        .write_segment(&mut Cursor::new(b"a segment beside the key"))
        .unwrap();
    drop(store);
    let at_rest = super::verify::raw_key_at_rest(dir.path(), &store_key);
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
    assert!(
        matches!(at_rest, Ok(false)),
        "the raw store key is at rest in the store directory: {at_rest:?}"
    );
}

#[test]
fn the_store_key_file_holds_its_four_fields_and_no_other() {
    let dir = tempfile::tempdir().unwrap();
    drop(super::db::test_support::make_store(dir.path()));
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(KeyFile::path(dir.path())).unwrap()).unwrap();
    let fields: Vec<&str> = json
        .as_object()
        .expect("store.key is one JSON object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        [
            "custody_kind",
            "root_id",
            "version",
            "wrapped_store_key_b64"
        ]
    );
    assert_eq!(json["version"], 1);
    assert_eq!(json["custody_kind"], "dev_fallback");
}

#[test]
fn a_delete_all_that_fails_after_its_root_step_finishes_on_the_next_call() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = reopened_file_root_store(&store_dir, &roots_dir, root_id);
    super::db::failpoint::arm("key_file_destroy");
    let first = store.crypto_shred_all();
    assert!(first.is_err(), "the failpoint did not fail the delete-all");
    assert!(
        KeyFile::path(&store_dir).exists(),
        "the failpoint did not stop the key-file step"
    );
    let retry = store.crypto_shred_all();
    assert!(
        retry.is_ok(),
        "the second delete-all did not finish: {:?}",
        retry.err()
    );
    for name in [
        "store.key",
        "store.db",
        "store.db-wal",
        "store.db-shm",
        "segments",
    ] {
        assert!(
            !store_dir.join(name).exists(),
            "the finished delete-all left {name}"
        );
    }
    drop(store);
    let fresh = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            RootId::fresh(),
        )),
        DEFAULT_RETENTION,
    );
    assert!(
        fresh.is_ok(),
        "a create after the finished delete-all failed: {:?}",
        fresh.err()
    );
}

fn a_second_create_with_one_root_id_fails_and_leaves_the_first_store_readable(
    custody: &dyn Fn() -> Box<dyn KeyCustody + Send>,
) {
    let dir = tempfile::tempdir().unwrap();
    let (first_dir, second_dir) = (dir.path().join("first"), dir.path().join("second"));
    let mut first = EncryptedStore::create(&first_dir, custody(), DEFAULT_RETENTION).unwrap();
    let (segment, _) = first
        .write_segment(&mut Cursor::new(b"the first store"))
        .unwrap();
    drop(first);
    let second = EncryptedStore::create(&second_dir, custody(), DEFAULT_RETENTION);
    let second_answer = second.as_ref().err().map(|e| format!("{e:?}"));
    drop(second);
    let reopened = EncryptedStore::open_with(&first_dir, DEFAULT_RETENTION, |_, _| Ok(custody()));
    let read = reopened
        .as_ref()
        .map(|store| {
            store
                .read_segment(segment.segment_id, &mut Vec::new())
                .is_ok()
        })
        .map_err(|e| format!("{e:?}"));
    drop(reopened);
    custody().destroy_root().unwrap();
    assert!(
        second_answer.is_some(),
        "a create took the root of another store"
    );
    assert!(
        second_answer
            .as_deref()
            .is_some_and(|answer| answer.contains("RootExists")),
        "the refused create answered {second_answer:?}"
    );
    assert!(
        matches!(read, Ok(true)),
        "the first store does not read after the refused create: {read:?}"
    );
}

#[test]
fn a_create_never_takes_the_root_of_another_store() {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let root_id = RootId::fresh();
        a_second_create_with_one_root_id_fails_and_leaves_the_first_store_readable(&|| {
            Box::new(DevFallbackCustody::new(root_id))
        });
    }
    let dir = tempfile::tempdir().unwrap();
    let roots_dir = dir.path().join("roots");
    let root_id = RootId::fresh();
    a_second_create_with_one_root_id_fails_and_leaves_the_first_store_readable(&|| {
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        ))
    });
}

fn names_in_store_dir(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn a_create_meets_a_root_of_another_store_that_it_cannot_read(
    state: &str,
    second_protector: TestProtector,
    make_unreadable: &dyn Fn(&Path, RootId) -> Vec<u8>,
    expected: fn(&StoreError) -> bool,
) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let (first_dir, second_dir, roots_dir) = (
        dir.path().join("first"),
        dir.path().join("second"),
        dir.path().join("roots"),
    );
    let root_id = RootId::fresh();
    let mut first = EncryptedStore::create(
        &first_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (segment, _) = first
        .write_segment(&mut Cursor::new(b"the first store"))
        .unwrap();
    drop(first);
    let root_path =
        FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0).root_path(root_id);
    let healthy = std::fs::read(&root_path).unwrap();
    let held = make_unreadable(&root_path, root_id);

    let second = EncryptedStore::create(
        &second_dir,
        Box::new(file_root_custody(&roots_dir, second_protector, root_id)),
        DEFAULT_RETENTION,
    );
    let answer = second.as_ref().err().map(|e| format!("{e:?}"));
    let answered_its_state = second.as_ref().err().is_some_and(expected);
    drop(second);
    #[cfg(unix)]
    if root_path.exists() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let left = std::fs::read(&root_path).ok();
    let second_names = names_in_store_dir(&second_dir);
    std::fs::write(&root_path, &healthy).unwrap();
    let reopened = EncryptedStore::open_with(&first_dir, DEFAULT_RETENTION, |_, _| {
        Ok(Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )))
    });
    let read = reopened
        .as_ref()
        .map(|store| {
            store
                .read_segment(segment.segment_id, &mut Vec::new())
                .is_ok()
        })
        .map_err(|e| format!("{e:?}"));

    let mut problems = Vec::new();
    if !answered_its_state {
        problems.push(format!(
            "a create under a root that is {state} answered {answer:?}"
        ));
    }
    if left.as_deref() != Some(held.as_slice()) {
        problems.push(format!(
            "a create under a root that is {state} removed or changed that root: it holds {:?} bytes",
            left.map(|bytes| bytes.len())
        ));
    }
    if second_names != ["writer.lock"] {
        problems.push(format!(
            "a create under a root that is {state} left {second_names:?}"
        ));
    }
    if !matches!(read, Ok(true)) {
        problems.push(format!(
            "after a create under its root that was {state}, the first store does not read: {read:?}"
        ));
    }
    problems
}

#[test]
fn a_create_under_a_root_of_another_store_that_it_cannot_read_fails_and_leaves_that_root() {
    let as_it_is = |path: &Path, _: RootId| std::fs::read(path).unwrap();
    let mut problems = a_create_meets_a_root_of_another_store_that_it_cannot_read(
        "locked",
        TestProtector::failing_once(0x5a, CustodyError::RootLocked(5)).0,
        &as_it_is,
        |e| matches!(e, StoreError::Custody(CustodyError::RootLocked(5))),
    );
    problems.extend(a_create_meets_a_root_of_another_store_that_it_cannot_read(
        "corrupt",
        TestProtector::failing_once(
            0x5a,
            CustodyError::Crypto("the root does not decode".to_string()),
        )
        .0,
        &as_it_is,
        |e| matches!(e, StoreError::Custody(CustodyError::Crypto(_))),
    ));
    problems.extend(a_create_meets_a_root_of_another_store_that_it_cannot_read(
        "of the wrong length",
        TestProtector::new(0x5a).0,
        &|path, root_id| {
            let blob = TestProtector::new(0x5a)
                .0
                .protect(&[0x17; 33], root_id.as_bytes())
                .unwrap();
            std::fs::write(path, &blob).unwrap();
            blob
        },
        |e| matches!(e, StoreError::Custody(CustodyError::RootCorrupt)),
    ));
    #[cfg(unix)]
    problems.extend(a_create_meets_a_root_of_another_store_that_it_cannot_read(
        "denied",
        TestProtector::new(0x5a).0,
        &|path, _| {
            use std::os::unix::fs::PermissionsExt;
            let bytes = std::fs::read(path).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o200)).unwrap();
            bytes
        },
        |e| matches!(e, StoreError::Custody(CustodyError::RootDenied)),
    ));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

type AnswerOfTheRootStep = fn() -> CustodyError;

struct ARootThatCannotBeRead {
    root_id: RootId,
    answer: AnswerOfTheRootStep,
    destroy_calls: Arc<AtomicUsize>,
}

impl KeyCustody for ARootThatCannotBeRead {
    fn kind(&self) -> CustodyKind {
        CustodyKind::DevFallback
    }
    fn root_id(&self) -> RootId {
        self.root_id
    }
    fn wrap_store_key(&self, _key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        Err((self.answer)())
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        Err((self.answer)())
    }
    fn unwrap_store_key(&self, _wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        Err((self.answer)())
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.destroy_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn a_create_whose_custody_cannot_read_the_root_of_its_id_destroys_no_root() {
    let states: [(&str, AnswerOfTheRootStep); 4] = [
        ("locked", || CustodyError::RootLocked(-25308)),
        ("denied", || CustodyError::RootDenied),
        ("corrupt", || {
            CustodyError::Crypto("the root does not decode".to_string())
        }),
        ("of the wrong length", || CustodyError::RootCorrupt),
    ];
    let mut problems = Vec::new();
    for (state, answer) in states {
        let dir = tempfile::tempdir().unwrap();
        let destroy_calls = Arc::new(AtomicUsize::new(0));
        let custody = ARootThatCannotBeRead {
            root_id: RootId::fresh(),
            answer,
            destroy_calls: Arc::clone(&destroy_calls),
        };
        let created = EncryptedStore::create(dir.path(), Box::new(custody), DEFAULT_RETENTION);
        let got = created.as_ref().err().map(|e| format!("{e:?}"));
        let expected = format!("{:?}", StoreError::Custody(answer()));
        if got.as_deref() != Some(expected.as_str()) {
            problems.push(format!(
                "a create under a root that is {state} answered {got:?} in place of {expected}"
            ));
        }
        let destroyed = destroy_calls.load(Ordering::SeqCst);
        if destroyed != 0 {
            problems.push(format!(
                "a create under a root that is {state} destroyed that root ({destroyed} calls)"
            ));
        }
        drop(created);
        let names = names_in_store_dir(dir.path());
        if names != ["writer.lock"] {
            problems.push(format!(
                "a create under a root that is {state} left {names:?}"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn two_creates_at_one_instant_under_one_root_id_destroy_no_root_that_a_create_made() {
    const ROUNDS: usize = 64;
    let mut missing = Vec::new();
    let mut replaced = 0;
    for round in 0..ROUNDS {
        let dir = tempfile::tempdir().unwrap();
        let roots_dir = dir.path().join("roots");
        let root_id = RootId::fresh();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let creators: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|name| {
                let store_dir = dir.path().join(name);
                let roots_dir = roots_dir.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let custody =
                        file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
                    barrier.wait();
                    let made =
                        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION)
                            .is_ok();
                    (store_dir, made)
                })
            })
            .collect();
        for creator in creators {
            let (store_dir, made) = creator.join().unwrap();
            if !made {
                continue;
            }
            let reopened = EncryptedStore::open_with(&store_dir, DEFAULT_RETENTION, |_, _| {
                Ok(Box::new(file_root_custody(
                    &roots_dir,
                    TestProtector::new(0x5a).0,
                    root_id,
                )))
            });
            match reopened {
                Err(StoreError::Custody(CustodyError::RootMissing)) => missing.push(round),
                Err(StoreError::Custody(CustodyError::WrapAuthFailed)) => replaced += 1,
                _ => {}
            }
        }
    }
    assert!(
        missing.is_empty(),
        "in {} of {ROUNDS} rounds a store whose create returned Ok answers that its root is \
         missing (rounds {missing:?}). Not asserted here: {replaced} stores whose wrap no longer \
         opens because the other create replaced the root's bytes, the race that store-3 takes",
        missing.len()
    );
}
