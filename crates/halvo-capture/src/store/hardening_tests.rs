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
use super::roots::RootGuard;
use super::segment::tests::{BODY, CIPHER_CHUNK_LEN, HEADER, TAG_LEN};
use super::segment::{self, segment_path, CHUNK};
use super::{SegmentExpiry, SegmentStore, StoreError, StoreResult, DEFAULT_RETENTION};

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
    conn.execute_batch(&super::db::test_support::key_statement(store_key))?;
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
        .execute_batch(&super::db::test_support::key_statement(&store_key))
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
    let _root_id_guard = RootGuard::new(root_id);
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
    let _root_id_guard = RootGuard::new(root_id);
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
fn first_opens_for_parent() {
    let Ok(out) = std::env::var("HALVO_TEST_FIRST_OPENS_OUT") else {
        println!("first_opens_for_parent: HALVO_TEST_FIRST_OPENS_OUT is not set, so no parent asked for opens");
        return;
    };
    const THREADS: usize = 8;
    let barrier = Arc::new(std::sync::Barrier::new(THREADS));
    let dirs: Vec<_> = (0..THREADS).map(|_| tempfile::tempdir().unwrap()).collect();
    let opens: Vec<_> = dirs
        .iter()
        .map(|dir| {
            let (barrier, dir) = (Arc::clone(&barrier), dir.path().to_path_buf());
            std::thread::spawn(move || {
                barrier.wait();
                super::db::Db::open(&super::durable::Dir::open(&dir).unwrap(), &[7u8; 32]).map(drop)
            })
        })
        .collect();
    let failed: Vec<String> = opens
        .into_iter()
        .filter_map(|open| open.join().unwrap().err())
        .map(|e| format!("{e:?}"))
        .collect();
    std::fs::write(
        out,
        format!(
            "{}\n{}",
            failed.len(),
            failed.first().cloned().unwrap_or_default()
        ),
    )
    .unwrap();
}

#[test]
fn first_opens_from_several_threads_at_once_all_succeed() {
    let launches: usize = std::env::var("HALVO_TEST_FIRST_OPENS_LAUNCHES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(20);
    let dir = tempfile::tempdir().unwrap();
    let (mut failed_launches, mut failed_opens) = (0, 0);
    let mut errors = std::collections::BTreeSet::new();
    for launch in 0..launches {
        let out = dir.path().join(format!("launch-{launch}"));
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "store::hardening_tests::first_opens_for_parent",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HALVO_TEST_FIRST_OPENS_OUT", &out)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        let text = std::fs::read_to_string(&out)
            .unwrap_or_else(|_| format!("8\nthe child wrote nothing and exited {status}"));
        let mut lines = text.lines();
        let failed: usize = lines.next().unwrap_or("8").parse().unwrap_or(8);
        if failed > 0 {
            failed_launches += 1;
            failed_opens += failed;
            errors.insert(lines.next().unwrap_or_default().to_string());
        }
    }
    println!(
        "launches={launches} opens_per_launch=8 launches_with_a_failed_open={failed_launches} \
         failed_opens={failed_opens} errors={errors:?}"
    );
    assert_eq!(
        failed_opens, 0,
        "{failed_opens} first opens failed in {failed_launches} of {launches} launches: {errors:?}"
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
    let _root_id_guard = RootGuard::new(root_id);
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
    let _root_id_guard = RootGuard::new(root_id);
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
    let _root_id_guard = RootGuard::new(root_id);
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

#[test]
fn fail_segment_removal_for_parent() {
    let Ok(dir) = std::env::var("HALVO_TEST_FAILED_REMOVAL_DIR") else {
        println!("fail_segment_removal_for_parent: HALVO_TEST_FAILED_REMOVAL_DIR is not set, so no parent asked for a failed removal");
        return;
    };
    let dir = PathBuf::from(dir);
    let (store_dir, roots_dir, root_id, _) = file_root_store(&dir);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let (expired, _) = store
        .write_segment_with_expiry(&mut Cursor::new(b"orphan-bytes"), 0, 1)
        .unwrap();
    let path = segment_path(&store_dir, expired.segment_id);
    let cipher = std::fs::read(&path).unwrap();
    let store_key = *store.store_key().unwrap().expose();
    store.db().unwrap().checkpoint_truncate().unwrap();
    let before =
        wrapped_key_in_copy_of_main_file_without_wal(&store_dir, &store_key, expired.segment_id)
            .unwrap();
    assert!(
        recoverable(&cipher, &store_key, before),
        "the probe does not find the live key in store.db, so it cannot see one left behind"
    );

    let first = store.crypto_shred_expired();

    let from_main_file =
        wrapped_key_in_copy_of_main_file_without_wal(&store_dir, &store_key, expired.segment_id)
            .unwrap();
    println!();
    println!(
        "FAILED-REMOVAL first_pass_failed={} file_left={} wal_bytes={} key_in_main_file={} \
         live_key_rows={} segment={}",
        first.is_err(),
        path.exists(),
        wal_len(&store_dir),
        recoverable(&cipher, &store_key, from_main_file),
        live_key_rows(&store),
        expired.segment_id.simple()
    );
}

#[test]
fn a_shred_whose_file_removal_fails_still_destroys_the_key_and_the_next_pass_removes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::fail_segment_removal_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_TEST_FAILED_REMOVAL_DIR", dir.path())
        .env(
            super::failpoint::FROM_THE_ENVIRONMENT,
            "segment_file_remove",
        )
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the child failed: {text}");
    let line = text
        .lines()
        .find(|line| line.starts_with("FAILED-REMOVAL "))
        .unwrap_or_else(|| panic!("the child printed no FAILED-REMOVAL line: {text}"));
    let segment_id = line
        .rsplit_once(" segment=")
        .and_then(|(_, id)| Uuid::parse_str(id).ok())
        .unwrap_or_else(|| panic!("the child named no segment: {line}"));
    let (head, _) = line.rsplit_once(" segment=").unwrap();
    assert_eq!(
        head,
        "FAILED-REMOVAL first_pass_failed=true file_left=true wal_bytes=0 key_in_main_file=false \
         live_key_rows=1",
        "a pass whose file removal failed: the pass must fail, leave the file, leave no frame in \
         the WAL and no key of the segment in store.db"
    );

    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = KeyFile::read(&store_dir).unwrap().root_id;
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store.crypto_shred_expired().unwrap();
    assert!(
        !segment_path(&store_dir, segment_id).exists(),
        "the next pass did not remove the orphan file"
    );
}

#[test]
fn a_shred_clears_the_key_durably_before_it_removes_the_file_and_syncs_the_shard_after() {
    use super::durable::test_support::{Call, RecordingFs};
    use std::sync::Mutex;

    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let (expired, _) = store
        .write_segment_with_expiry(&mut Cursor::new(b"cleared before removal"), 0, 1)
        .unwrap();
    store.write_segment(&mut Cursor::new(b"live")).unwrap();
    let path = segment_path(store.dir(), expired.segment_id);
    let shard = path.parent().unwrap().to_path_buf();
    let cipher = std::fs::read(&path).unwrap();
    let store_key = *store.store_key().unwrap().expose();
    let at_removal = Arc::new(Mutex::new(Vec::new()));
    let fs = {
        let (dir, path, at_removal) = (
            dir.path().to_path_buf(),
            path.clone(),
            Arc::clone(&at_removal),
        );
        RecordingFs::observed_by(move |call| {
            if *call == Call::RemoveFile(path.clone()) {
                let in_main_file = wrapped_key_in_copy_of_main_file_without_wal(
                    &dir,
                    &store_key,
                    expired.segment_id,
                )
                .unwrap();
                at_removal.lock().unwrap().push((
                    recoverable(&cipher, &store_key, in_main_file),
                    wal_len(&dir),
                ));
            }
        })
    };
    let fs = Arc::new(fs);
    store.use_fs(fs.clone());

    assert_eq!(store.crypto_shred_expired().unwrap().segments_shredded, 1);

    assert_eq!(
        *at_removal.lock().unwrap(),
        vec![(false, 0)],
        "when the file was removed, store.db or its log still held the key of the segment"
    );
    let calls = fs.calls();
    let removal = calls
        .iter()
        .position(|call| *call == Call::RemoveFile(path.clone()))
        .expect("the shred removed the segment file through the seam");
    assert!(
        calls[removal + 1..].contains(&Call::SyncDir(shard.clone())),
        "the shard was not synced after the removal: {calls:?}"
    );
    assert!(!path.exists());
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
    let before = super::db::dir_size(store.handle()).unwrap();

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
    let grown = super::db::dir_size(store.handle()).unwrap() - before;
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

fn keyless_name() -> String {
    format!("{}.seg", Uuid::now_v7().simple())
}

fn another_shard_than(segment_id: Uuid) -> &'static str {
    if segment_id.simple().to_string().starts_with("ff") {
        "fe"
    } else {
        "ff"
    }
}

fn files_in(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) {
    let made = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(made.success(), "mklink /J failed");
}

#[test]
fn the_keyless_sweep_does_not_follow_a_shard_that_links_out_of_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path().join("store").as_path());
    let (live, _) = store.write_segment(&mut Cursor::new(b"live")).unwrap();
    let shard = another_shard_than(live.segment_id);
    let victim_name = format!("{shard}{}.seg", &Uuid::now_v7().simple().to_string()[2..]);
    std::fs::write(outside.path().join(&victim_name), b"victim").unwrap();
    std::fs::write(outside.path().join("notes.txt"), b"victim").unwrap();
    link_dir(
        outside.path(),
        &segment::segments_root(store.dir()).join(shard),
    );

    let pass = store.crypto_shred_expired();

    assert_eq!(
        files_in(outside.path()),
        2,
        "the pass ({pass:?}) removed files outside the store through a linked shard"
    );
}

fn entries_below(dir: &Path) -> Vec<(String, u64)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(below) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&below) else {
            continue;
        };
        for entry in entries.flatten() {
            let meta = std::fs::symlink_metadata(entry.path()).unwrap();
            if meta.is_dir() && !meta.file_type().is_symlink() {
                stack.push(entry.path());
            } else {
                let name = entry.path().strip_prefix(dir).unwrap().to_path_buf();
                found.push((name.to_string_lossy().into_owned(), meta.len()));
            }
        }
    }
    found.sort();
    found
}

type SwapAnswer = Arc<std::sync::Mutex<Option<Result<(), Option<i32>>>>>;

fn swap_shard_for_a_link(shard: &Path, parked: &Path, target: &Path, answer: &SwapAnswer) {
    let mut answer = answer.lock().unwrap();
    if answer.is_some() {
        return;
    }
    let moved = std::fs::rename(shard, parked);
    if moved.is_ok() {
        link_dir(target, shard);
    }
    *answer = Some(moved.map_err(|e| e.raw_os_error()));
}

const SHARING_VIOLATION: i32 = 32;

#[test]
fn the_keyless_sweep_removes_nothing_through_a_shard_that_becomes_a_link_after_the_walk() {
    use super::durable::test_support::{Call, RecordingFs};

    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let shard_name = segment_id.simple().to_string()[..2].to_string();
    let keyless = format!("{shard_name}{}.seg", "0".repeat(30));
    let shard = segment::segments_root(store.dir()).join(&shard_name);
    std::fs::write(shard.join(&keyless), b"a keyless file of the store").unwrap();
    std::fs::write(outside.path().join(&keyless), b"a file outside the store").unwrap();
    let answer = SwapAnswer::default();
    let fs = {
        let (shard, parked) = (shard.clone(), dir.path().join("parked-shard"));
        let (target, answer) = (outside.path().to_path_buf(), Arc::clone(&answer));
        RecordingFs::observed_by(move |call| {
            if matches!(call, Call::RemoveFile(path) if path.parent() == Some(shard.as_path())) {
                swap_shard_for_a_link(&shard, &parked, &target, &answer);
            }
        })
    };
    store.use_fs(Arc::new(fs));

    let swept = store
        .remove_keyless_files()
        .map(|sweep| (sweep.removed, sweep.not_store_files))
        .map_err(|e| format!("{e:?}"));

    let swapped = *answer.lock().unwrap();
    assert!(
        outside.path().join(&keyless).exists(),
        "the sweep ({swept:?}) removed a file outside the store through a shard swapped for a link"
    );
    say_branch(
        "the_keyless_sweep_removes_nothing_through_a_shard_that_becomes_a_link_after_the_walk",
        &format!("the move of the walked shard: {swapped:?}; the sweep: {swept:?}"),
    );
    match swapped {
        Some(Ok(())) => assert_eq!(
            swept,
            Ok((1, 1)),
            "the sweep did not remove the keyless file of the walked shard and count the link"
        ),
        Some(Err(code)) => assert!(
            cfg!(windows) && code == Some(SHARING_VIOLATION) && swept == Ok((1, 0)),
            "the move of the walked shard answered {code:?} and the sweep {swept:?}"
        ),
        None => panic!("the sweep removed no file through the seam: {swept:?}"),
    }
}

#[test]
fn the_keyless_sweep_counts_a_shard_that_becomes_a_link_before_its_open_and_removes_nothing_behind_it(
) {
    use super::durable::test_support::{Call, RecordingFs};

    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let shard_name = segment_id.simple().to_string()[..2].to_string();
    let keyless = format!("{shard_name}{}.seg", "0".repeat(30));
    let shard = segment::segments_root(store.dir()).join(&shard_name);
    let parked = dir.path().join("parked-shard");
    std::fs::write(shard.join(&keyless), b"a keyless file of the store").unwrap();
    std::fs::write(outside.path().join(&keyless), b"a file outside the store").unwrap();
    let answer = SwapAnswer::default();
    let fs = {
        let (shard, parked) = (shard.clone(), parked.clone());
        let (target, answer) = (outside.path().to_path_buf(), Arc::clone(&answer));
        RecordingFs::observed_by(move |call| {
            if matches!(call, Call::OpenDir(opened) if *opened == shard) {
                swap_shard_for_a_link(&shard, &parked, &target, &answer);
            }
        })
    };
    store.use_fs(Arc::new(fs));

    let swept = store
        .remove_keyless_files()
        .map(|sweep| (sweep.removed, sweep.not_store_files))
        .map_err(|e| format!("{e:?}"));

    say_branch(
        "the_keyless_sweep_counts_a_shard_that_becomes_a_link_before_its_open_and_removes_nothing_behind_it",
        &format!("the move of the shard: {:?}; the sweep: {swept:?}", *answer.lock().unwrap()),
    );
    assert_eq!(
        *answer.lock().unwrap(),
        Some(Ok(())),
        "the shard was not swapped for a link before its open"
    );
    assert!(
        outside.path().join(&keyless).exists(),
        "the sweep ({swept:?}) removed a file behind a shard that became a link before its open"
    );
    assert!(parked.join(&keyless).exists());
    assert_eq!(
        swept,
        Ok((0, 1)),
        "the sweep did not count the link that it found in place of the shard"
    );
}

fn sweep_race(control: bool) -> String {
    use std::sync::atomic::AtomicBool;
    const SWEEPS: u64 = 3_600;

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let segments = segment::segments_root(store.dir());
    let (outside, parked) = (dir.path().join("outside"), dir.path().join("parked"));
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir(&parked).unwrap();
    std::fs::create_dir(segments.join("ab")).unwrap();
    let victim = outside.join(format!("ab{}.seg", "0123456789abcdef0123456789abcd"));
    std::fs::write(&victim, b"a file outside the store").unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let racer = {
        let (stop, segments, outside, parked) = (
            Arc::clone(&stop),
            segments.clone(),
            outside.clone(),
            parked.clone(),
        );
        std::thread::spawn(move || {
            let (shard, aside) = (segments.join("ab"), parked.join("ab"));
            let mut swaps = 0u64;
            while !stop.load(Ordering::Relaxed) {
                if std::fs::rename(&shard, &aside).is_ok() {
                    if control {
                        let _ = std::fs::create_dir(&shard);
                        std::thread::yield_now();
                        let _ = std::fs::remove_dir(&shard);
                    } else {
                        link_dir(&outside, &shard);
                        std::thread::yield_now();
                        let _ = std::fs::remove_file(&shard);
                        let _ = std::fs::remove_dir(&shard);
                    }
                    let _ = std::fs::rename(&aside, &shard);
                    swaps += 1;
                }
            }
            swaps
        })
    };
    let started = Instant::now();
    let (mut sweeps, mut removed_outside, mut refused) = (0u64, 0u64, 0u64);
    while sweeps < SWEEPS && started.elapsed() < Duration::from_secs(600) {
        if store.remove_keyless_files().is_err() {
            refused += 1;
        }
        sweeps += 1;
        if !victim.exists() {
            removed_outside += 1;
            std::fs::write(&victim, b"a file outside the store").unwrap();
        }
    }
    stop.store(true, Ordering::Relaxed);
    let swaps = racer.join().unwrap();
    let counts = format!(
        "control={control} sweeps={sweeps} racer_swaps={swaps} sweeps_with_an_error={refused} \
         removed_outside={removed_outside} seconds={:.1}",
        started.elapsed().as_secs_f64()
    );
    println!("sweep race: {counts}");
    assert!(sweeps >= SWEEPS, "the race ran {sweeps} sweeps: {counts}");
    assert_eq!(
        removed_outside, 0,
        "the sweep removed a file outside the store: {counts}"
    );
    counts
}

#[test]
#[ignore = "a counted race of 3,600 sweeps, 1 to 3 s on a Mac and 600 s at most; run it by hand when the sweep changes: cargo test -p halvo-capture --lib -- --ignored --exact store::hardening_tests::the_keyless_sweep_removes_nothing_outside_while_a_thread_swaps_a_shard_for_a_link --nocapture"]
fn the_keyless_sweep_removes_nothing_outside_while_a_thread_swaps_a_shard_for_a_link() {
    sweep_race(false);
}

#[test]
#[ignore = "the control of the counted sweep race, with a plain directory swapped in; run it by hand with the race: cargo test -p halvo-capture --lib -- --ignored --exact store::hardening_tests::the_keyless_sweep_race_control_with_a_plain_directory_swapped_in --nocapture"]
fn the_keyless_sweep_race_control_with_a_plain_directory_swapped_in() {
    sweep_race(true);
}

#[test]
fn the_keyless_sweep_does_not_follow_a_directory_link_inside_a_shard() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path().join("store").as_path());
    let (live, _) = store.write_segment(&mut Cursor::new(b"live")).unwrap();
    std::fs::write(outside.path().join(keyless_name()), b"outside").unwrap();
    std::fs::write(outside.path().join(keyless_name()), b"outside").unwrap();
    let shard = segment_path(store.dir(), live.segment_id)
        .parent()
        .unwrap()
        .to_path_buf();
    link_dir(outside.path(), &shard.join(keyless_name()));

    let pass = store.crypto_shred_expired();

    assert_eq!(
        files_in(outside.path()),
        2,
        "the pass ({pass:?}) removed files outside the store through a link in a shard"
    );
    assert!(segment_path(store.dir(), live.segment_id).exists());
}

#[test]
fn the_keyless_sweep_leaves_the_store_files_when_a_shard_links_to_the_store_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh();
    let _root_id_guard = RootGuard::new(root_id);
    let mut store = EncryptedStore::create(
        dir.path(),
        Box::new(DevFallbackCustody::new(root_id)),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (live, _) = store.write_segment(&mut Cursor::new(b"live")).unwrap();
    link_dir(
        dir.path(),
        &segment::segments_root(dir.path()).join(another_shard_than(live.segment_id)),
    );

    let pass = store.crypto_shred_expired();
    drop(store);

    let left: Vec<bool> = ["store.key", "store.db", "writer.lock"]
        .iter()
        .map(|name| dir.path().join(name).exists())
        .collect();
    let reopened = EncryptedStore::open(dir.path(), DEFAULT_RETENTION).map(|store| {
        let read = store.read_segment(live.segment_id, &mut Vec::new());
        read.is_ok()
    });
    DevFallbackCustody::new(root_id).destroy_root().unwrap();
    assert_eq!(
        left,
        [true, true, true],
        "the pass ({pass:?}) removed store.key, store.db or writer.lock through a link"
    );
    assert!(
        matches!(reopened, Ok(true)),
        "the store did not reopen with its live segment: {reopened:?}"
    );
}

#[test]
fn the_keyless_sweep_removes_nothing_through_a_segments_directory_that_is_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store
        .db()
        .unwrap()
        .connection()
        .execute(
            "UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = ?1",
            [segment_id.simple().to_string()],
        )
        .unwrap();
    let segments = segment::segments_root(&store_dir);
    let shard_name = segment_id.simple().to_string()[..2].to_string();
    std::fs::rename(segments.join(&shard_name), target.path().join(&shard_name)).unwrap();
    std::fs::remove_dir(&segments).unwrap();
    std::fs::write(
        target.path().join(&shard_name).join(keyless_name()),
        b"foreign",
    )
    .unwrap();
    std::fs::write(
        target.path().join(&shard_name).join("notes.txt"),
        b"foreign",
    )
    .unwrap();
    link_dir(target.path(), &segments);
    let before = files_under(target.path()).len();

    let pass = store.crypto_shred_expired();
    drop(store);
    let reopened = open_over_file_root(&store_dir, &roots_dir, root_id).map(drop);

    assert_eq!(before, 3);
    assert_eq!(
        files_under(target.path()).len(),
        3,
        "the pass ({pass:?}) removed files through a segments directory that is a link"
    );
    assert!(
        reopened.is_err(),
        "a store whose segments directory is a link opened"
    );
}

#[test]
fn the_keyless_sweep_removes_only_the_names_of_the_store_and_counts_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let (live, _) = store.write_segment(&mut Cursor::new(b"live")).unwrap();
    let shard = segment_path(dir.path(), live.segment_id)
        .parent()
        .unwrap()
        .to_path_buf();
    let keyless = shard.join(format!(
        "{}{}.seg",
        &live.segment_id.simple().to_string()[..2],
        &Uuid::now_v7().simple().to_string()[2..]
    ));
    let temp = keyless.with_extension("seg.tmp");
    std::fs::write(&keyless, b"keyless").unwrap();
    std::fs::write(&temp, b"half-written").unwrap();
    let id = Uuid::now_v7();
    let foreign = [
        shard.join("notes.txt"),
        shard.join(format!("{}.seg", id.hyphenated())),
        shard.join(format!("{}.seg", id.simple().to_string().to_uppercase())),
        shard.join(format!("{}.seg.bak", id.simple())),
        segment::segments_root(dir.path()).join("stray.seg"),
        segment::segments_root(dir.path())
            .join("zz")
            .join(format!("{}.seg", id.simple())),
    ];
    for path in &foreign {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"not a name of the store").unwrap();
    }

    let sweep = store.remove_keyless_files().unwrap();

    assert!(!keyless.exists(), "the sweep left a keyless segment file");
    assert!(!temp.exists(), "the sweep left a temp file");
    let kept: Vec<bool> = foreign.iter().map(|path| path.exists()).collect();
    assert_eq!(
        kept, [true; 6],
        "the sweep removed a file that is no name of the store"
    );
    assert_eq!((sweep.removed, sweep.not_store_files), (2, 6), "{sweep:?}");
    assert!(segment_path(dir.path(), live.segment_id).exists());
}

#[cfg(unix)]
fn modes_under(dir: &Path) -> Vec<(String, u32, bool)> {
    use std::os::unix::fs::PermissionsExt;

    let mut modes = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).unwrap() {
            let entry = entry.unwrap();
            let meta = std::fs::symlink_metadata(entry.path()).unwrap();
            let name = entry
                .path()
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            modes.push((name, meta.permissions().mode() & 0o777, meta.is_dir()));
            if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    modes.sort();
    modes
}

#[cfg(unix)]
#[test]
fn make_store_files_for_parent() {
    let Ok(dir) = std::env::var("HALVO_TEST_MODES_DIR") else {
        println!("make_store_files_for_parent: HALVO_TEST_MODES_DIR is not set, so no parent asked for files");
        return;
    };
    let dir = PathBuf::from(dir);
    let (store_dir, roots_dir, root_id, _) = file_root_store(&dir);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store
        .write_segment_with_expiry(&mut Cursor::new(b"expired"), 0, 1)
        .unwrap();
    store.write_segment(&mut Cursor::new(b"live")).unwrap();
    println!();
    for (name, mode, is_dir) in modes_under(&dir) {
        println!(
            "MODE {mode:o} {} {name}",
            if is_dir { "dir" } else { "file" }
        );
    }
    store.crypto_shred_expired().unwrap();
    drop(store);
}

#[cfg(unix)]
#[test]
fn every_store_file_is_made_0600_and_every_directory_0700_under_umask_022() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new("/bin/sh")
        .args([
            "-c",
            "umask 022 && exec \"$0\" \"$@\"",
            std::env::current_exe().unwrap().to_str().unwrap(),
            "--exact",
            "store::hardening_tests::make_store_files_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_TEST_MODES_DIR", dir.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "the child failed: {text}");
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("MODE ")).collect();
    let wrong: Vec<&&str> = lines
        .iter()
        .filter(|line| !(line.starts_with("MODE 700 dir ") || line.starts_with("MODE 600 file ")))
        .collect();
    for name in [
        "store.db",
        "store.db-wal",
        "store.key",
        "writer.lock",
        "segments",
    ] {
        assert!(
            lines
                .iter()
                .any(|line| line.ends_with(&format!(" store/{name}"))),
            "the child listed no store/{name}: {lines:?}"
        );
    }
    assert!(
        lines.iter().any(|line| line.contains(".seg"))
            && lines.iter().any(|line| line.contains(".root")),
        "the child listed no segment or root file: {lines:?}"
    );
    assert!(
        wrong.is_empty(),
        "entries of a store made under umask 022 with a wider or other mode: {wrong:?} of {lines:?}"
    );
}

#[cfg(unix)]
#[test]
fn open_tightens_a_store_directory_made_at_0755_to_0700() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    std::fs::set_permissions(&store_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id).map(drop);
    let mode = std::fs::metadata(&store_dir).unwrap().permissions().mode() & 0o777;
    assert!(opened.is_ok(), "{opened:?}");
    assert_eq!(mode, 0o700, "open left the store directory at {mode:o}");
}

fn answer_of_open_and_create(store_dir: &Path, roots_dir: &Path) -> (String, String) {
    let opened = match open_by_the_recorded_root(store_dir, roots_dir) {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?}"),
    };
    let created = match EncryptedStore::create(
        store_dir,
        Box::new(file_root_custody(
            roots_dir,
            TestProtector::new(0x5a).0,
            RootId::fresh(),
        )),
        DEFAULT_RETENTION,
    ) {
        Ok(store) => {
            drop(store);
            "Ok".to_string()
        }
        Err(StoreError::AlreadyExists(_)) => "AlreadyExists".to_string(),
        Err(e) => format!("{e:?}"),
    };
    (opened, created)
}

fn a_delete_all_stopped_after_its_root_step(dir: &Path) -> (PathBuf, PathBuf) {
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    super::db::failpoint::arm("key_file_destroy");
    assert!(
        store.crypto_shred_all().is_err(),
        "the failpoint did not stop delete-all"
    );
    drop(store);
    (store_dir, roots_dir)
}

#[test]
fn after_a_stopped_delete_all_open_and_create_answer_by_what_the_stop_left() {
    let whole = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = a_delete_all_stopped_after_its_root_step(whole.path());
    let key_file_whole = answer_of_open_and_create(&store_dir, &roots_dir);

    let zeroed = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = a_delete_all_stopped_after_its_root_step(zeroed.path());
    let len = std::fs::metadata(KeyFile::path(&store_dir)).unwrap().len() as usize;
    std::fs::write(KeyFile::path(&store_dir), vec![0u8; len]).unwrap();
    let key_file_zeroed = answer_of_open_and_create(&store_dir, &roots_dir);

    let gone = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = a_delete_all_stopped_after_its_root_step(gone.path());
    std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
    let segment = segment_path(&store_dir, Uuid::now_v7());
    std::fs::create_dir_all(segment.parent().unwrap()).unwrap();
    std::fs::write(&segment, b"a segment file that the stop left").unwrap();
    assert!(store_dir.join("store.db").exists() && segment.exists());
    let key_file_gone = answer_of_open_and_create(&store_dir, &roots_dir);
    let segment_left_after_create = segment.exists();

    assert_eq!(
        [key_file_whole, key_file_zeroed, key_file_gone],
        [
            (
                "Custody(RootMissing)".to_string(),
                "AlreadyExists".to_string()
            ),
            (
                "KeyFileUnparseable".to_string(),
                "AlreadyExists".to_string()
            ),
            ("Sealed".to_string(), "Ok".to_string()),
        ]
    );
    assert!(
        !segment_left_after_create,
        "the create over a store whose key file was gone left its segment file"
    );
}

#[cfg(unix)]
#[test]
fn open_refuses_a_store_db_that_is_a_link_before_its_custody_and_writes_nothing_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let target = outside.path().join("store.db");
    std::fs::rename(store_dir.join("store.db"), &target).unwrap();
    std::os::unix::fs::symlink(&target, store_dir.join("store.db")).unwrap();
    let names = |d: &Path| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let bytes_before = std::fs::read(&target).unwrap();
    let beside_before = names(outside.path());
    let custody_calls = AtomicUsize::new(0);

    let opened = EncryptedStore::open_with(&store_dir, DEFAULT_RETENTION, |_, _| {
        custody_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )))
    });
    let beside_while_open = names(outside.path());
    let answer = match &opened {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?}"),
    };
    drop(opened);

    assert!(
        answer == "Corrupt(\"the database file of the store is a link\")",
        "an open of a store whose store.db is a link answered {answer}; beside the target \
         while it was open: {beside_while_open:?}"
    );
    assert_eq!(
        custody_calls.load(Ordering::SeqCst),
        0,
        "open called the custody"
    );
    assert!(
        std::fs::read(&target).unwrap() == bytes_before,
        "the bytes of the target changed"
    );
    assert_eq!(
        names(outside.path()),
        beside_before,
        "an entry appeared beside the target"
    );
}

#[cfg(unix)]
fn make_fifo(path: &Path) {
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: name is a NUL-terminated copy that outlives the call.
    assert_eq!(
        unsafe { libc::mkfifo(name.as_ptr(), 0o600) },
        0,
        "mkfifo {}",
        path.display()
    );
}

#[cfg(unix)]
const FIFO_BOUND: Duration = Duration::from_secs(5);

/// The answer of `call` when it comes within `bound`, run on its own thread.
/// When the call is still blocked at the bound, a writer opens `fifo` to let it
/// go, and the answer is `None`.
#[cfg(unix)]
fn answer_within<T: Send + 'static>(
    bound: Duration,
    fifo: &Path,
    call: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    use std::os::unix::fs::OpenOptionsExt;
    let (sender, answers) = std::sync::mpsc::channel();
    let caller = std::thread::spawn(move || {
        let _ = sender.send(call());
    });
    match answers.recv_timeout(bound) {
        Ok(answer) => {
            caller.join().unwrap();
            Some(answer)
        }
        Err(_) => {
            let _ = std::fs::File::options()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(fifo);
            let _ = answers.recv_timeout(Duration::from_secs(10));
            None
        }
    }
}

fn answer_text<T>(result: &StoreResult<T>) -> String {
    match result {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

#[cfg(unix)]
#[test]
fn open_answers_a_store_key_that_is_a_link_as_corrupt_within_a_bound_and_names_no_id() {
    for behind_the_link in ["the key file", "a FIFO"] {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        let target = outside.path().join("store.key");
        match behind_the_link {
            "the key file" => std::fs::rename(KeyFile::path(&store_dir), &target).unwrap(),
            _ => {
                std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
                make_fifo(&target);
            }
        }
        std::os::unix::fs::symlink(&target, KeyFile::path(&store_dir)).unwrap();

        let opened = answer_within(FIFO_BOUND, &target, move || {
            answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id))
        });

        let answer = opened.unwrap_or_else(|| {
            panic!("an open with a link to {behind_the_link} at store.key gave no answer in {FIFO_BOUND:?}")
        });
        assert_eq!(
            answer, "Corrupt(\"the key file of the store is a link\")",
            "an open with a link to {behind_the_link} at store.key"
        );
        assert!(!answer.contains(&root_id.to_string()));
    }
}

#[cfg(unix)]
#[test]
fn open_answers_a_store_key_that_is_a_fifo_as_key_file_unparseable_within_a_bound() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let key_file = KeyFile::path(&store_dir);
    std::fs::remove_file(&key_file).unwrap();
    make_fifo(&key_file);

    let opened = answer_within(FIFO_BOUND, &key_file, move || {
        answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id))
    });

    assert_eq!(
        opened.as_deref(),
        Some("KeyFileUnparseable"),
        "an open with a FIFO at store.key gave no answer in {FIFO_BOUND:?}, or another one"
    );
}

#[cfg(unix)]
#[test]
fn a_segment_read_answers_a_segment_file_that_is_a_link_as_corrupt_within_a_bound_and_names_no_id()
{
    for behind_the_link in ["the segment file", "a FIFO"] {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
        let path = segment_path(&store_dir, segment_id);
        let target = outside.path().join("segment");
        match behind_the_link {
            "the segment file" => std::fs::rename(&path, &target).unwrap(),
            _ => {
                std::fs::remove_file(&path).unwrap();
                make_fifo(&target);
            }
        }
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();

        let read = answer_within(FIFO_BOUND, &target, move || {
            answer_text(&store.read_segment(segment_id, &mut Vec::new()))
        });

        let answer = read.unwrap_or_else(|| {
            panic!("a read through a link to {behind_the_link} gave no answer in {FIFO_BOUND:?}")
        });
        assert_eq!(
            answer, "Corrupt(\"a segment file of the store is a link\")",
            "a read of a segment file that is a link to {behind_the_link}"
        );
        assert!(!answer.contains(&segment_id.simple().to_string()));
    }
}

#[cfg(unix)]
#[test]
fn a_segment_read_answers_a_segment_file_that_is_a_fifo_as_corrupt_within_a_bound() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let path = segment_path(&store_dir, segment_id);
    std::fs::remove_file(&path).unwrap();
    make_fifo(&path);
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();

    let read = answer_within(FIFO_BOUND, &path, move || {
        answer_text(&store.read_segment(segment_id, &mut Vec::new()))
    });

    assert_eq!(
        read.as_deref(),
        Some("Corrupt(\"a segment file of the store is no regular file\")"),
        "a read of a segment file that is a FIFO gave no answer in {FIFO_BOUND:?}, or another one"
    );
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Puts a link at store.db to `target`, or answers why this account may not
/// make a file link.
fn link_store_db_to(store_dir: &Path, target: &Path) -> Result<(), String> {
    let db = store_dir.join("store.db");
    let kept = std::fs::read(&db).unwrap();
    std::fs::remove_file(&db).unwrap();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(target, &db);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(target, &db);
    linked.map_err(|e| {
        std::fs::write(&db, kept).unwrap();
        format!("this account may not make a file link: {e}")
    })
}

#[test]
fn open_refuses_a_link_put_at_store_db_inside_the_custody_call_and_changes_nothing_beside_its_target(
) {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let target = outside.path().join("store.db");
    std::fs::copy(store_dir.join("store.db"), &target).unwrap();
    let (beside_before, bytes_before) = (names_in(outside.path()), std::fs::read(&target).unwrap());
    let linked = std::sync::Mutex::new(None);

    let opened = EncryptedStore::open_with(&store_dir, DEFAULT_RETENTION, |_, _| {
        *linked.lock().unwrap() = Some(link_store_db_to(&store_dir, &target));
        Ok(Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )))
    });
    let answer = answer_text(&opened);
    drop(opened);

    match linked.into_inner().unwrap() {
        Some(Ok(())) => {}
        Some(Err(why)) => {
            println!("the seam put no link at store.db: {why}");
            if cfg!(windows) {
                return;
            }
            panic!("{why}");
        }
        None => panic!("open did not call the custody: {answer}"),
    }
    assert_eq!(
        answer, "Corrupt(\"the database file of the store is a link\")",
        "an open with a link put at store.db inside the custody call"
    );
    assert_eq!(
        names_in(outside.path()),
        beside_before,
        "an entry appeared beside the target of the link ({answer})"
    );
    assert!(
        std::fs::read(&target).unwrap() == bytes_before,
        "the bytes of the target changed ({answer})"
    );
}

#[cfg(unix)]
#[test]
#[ignore = "a counted race of 300 opens; run it by hand when the opens of store.db change: cargo test -p halvo-capture --lib -- --ignored --exact store::hardening_tests::no_open_goes_through_a_link_that_a_thread_swaps_in_at_store_db --nocapture"]
fn no_open_goes_through_a_link_that_a_thread_swaps_in_at_store_db() {
    use std::sync::atomic::AtomicBool;
    const OPENS: usize = 300;

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let target = outside.join("store.db");
    std::fs::rename(store_dir.join("store.db"), &target).unwrap();
    std::fs::hard_link(&target, store_dir.join("store.db")).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let racer = {
        let (stop, store_dir, target) = (Arc::clone(&stop), store_dir.clone(), target.clone());
        std::thread::spawn(move || {
            let (hard, soft, db) = (
                store_dir.join("swap.hard"),
                store_dir.join("swap.soft"),
                store_dir.join("store.db"),
            );
            let mut swaps = 0u64;
            while !stop.load(Ordering::SeqCst) {
                let _ = std::fs::remove_file(&soft);
                std::os::unix::fs::symlink(&target, &soft).unwrap();
                std::fs::rename(&soft, &db).unwrap();
                let _ = std::fs::remove_file(&hard);
                std::fs::hard_link(&target, &hard).unwrap();
                std::fs::rename(&hard, &db).unwrap();
                swaps += 1;
            }
            swaps
        })
    };
    let (mut beside_the_target, mut inside, mut refused) = (0usize, 0usize, 0usize);
    let mut other = Vec::new();
    for _ in 0..OPENS {
        match open_over_file_root(&store_dir, &roots_dir, root_id) {
            Ok(store) => {
                if outside.join("store.db-wal").exists() {
                    beside_the_target += 1;
                } else {
                    inside += 1;
                }
                drop(store);
                let _ = std::fs::remove_file(outside.join("store.db-wal"));
            }
            Err(StoreError::Corrupt(_)) => refused += 1,
            Err(e) => other.push(format!("{e:?}")),
        }
    }
    stop.store(true, Ordering::SeqCst);
    let swaps = racer.join().unwrap();
    other.sort();
    other.dedup();
    let counts = format!(
        "opens={OPENS} swaps={swaps} ok_with_a_log_beside_the_target={beside_the_target} \
         ok_inside={inside} refused_as_corrupt={refused} other_answers={other:?}"
    );
    println!("store.db race: {counts}");
    assert_eq!(
        beside_the_target, 0,
        "an open went through a link swapped in at store.db: {counts}"
    );
}

#[cfg(unix)]
#[test]
fn open_narrows_store_db_to_0600_on_its_own_handle_so_its_log_is_made_0600() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let db = store_dir.join("store.db");
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!store_dir.join("store.db-wal").exists());

    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store
        .write_segment(&mut Cursor::new(b"after the open"))
        .unwrap();

    assert_eq!(
        (mode(&db), mode(&store_dir.join("store.db-wal"))),
        (0o600, 0o600),
        "after an open of a store.db at 0644 and one write, store.db and store.db-wal"
    );
}

#[cfg(unix)]
#[test]
fn file_colon_store_for_parent() {
    let Some(roots_dir) = std::env::var_os("HALVO_TEST_FILE_COLON_ROOTS") else {
        println!(
            "file_colon_store_for_parent: HALVO_TEST_FILE_COLON_ROOTS is not set, so no parent asked for a store"
        );
        return;
    };
    let custody = file_root_custody(
        Path::new(&roots_dir),
        TestProtector::new(0x5a).0,
        RootId::fresh(),
    );
    let mut store =
        EncryptedStore::create(Path::new("file:x"), Box::new(custody), DEFAULT_RETENTION).unwrap();
    store.write_segment(&mut Cursor::new(b"in file:x")).unwrap();
}

#[cfg(unix)]
#[test]
fn a_relative_store_directory_named_file_colon_x_holds_its_database_and_x_gets_no_file() {
    let cwd = tempfile::tempdir().unwrap();
    let roots = tempfile::tempdir().unwrap();
    std::fs::create_dir(cwd.path().join("x")).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::file_colon_store_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(cwd.path())
        .env("HALVO_TEST_FILE_COLON_ROOTS", roots.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "the child failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let db = cwd.path().join("file:x").join("store.db");
    let meta = std::fs::metadata(&db).unwrap();
    assert!(meta.len() > 0, "store.db in file:x is empty");
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o777,
        0o600
    );
    assert_eq!(
        names_in(&cwd.path().join("x")),
        Vec::<String>::new(),
        "the store put a file into x"
    );
}

#[cfg(unix)]
#[test]
fn delete_all_unlinks_a_store_key_that_is_a_link_and_writes_nothing_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let precious = outside.path().join("precious");
    std::fs::write(&precious, b"bytes that the store does not own").unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    std::fs::remove_file(KeyFile::path(dir.path())).unwrap();
    std::os::unix::fs::symlink(&precious, KeyFile::path(dir.path())).unwrap();

    let deleted = store.crypto_shred_all();

    assert_eq!(
        std::fs::read(&precious).unwrap(),
        b"bytes that the store does not own",
        "delete-all ({deleted:?}) wrote through a store.key that is a link"
    );
    assert!(
        std::fs::symlink_metadata(KeyFile::path(dir.path())).is_err(),
        "delete-all left the link at store.key"
    );
}

#[derive(Default)]
struct LargestWrite {
    largest: usize,
    total: u64,
}

impl Write for LargestWrite {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.largest = self.largest.max(buf.len());
        self.total += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_segment_read_hands_its_sink_at_most_one_chunk_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let plaintext = vec![0x5au8; CHUNK * 4 + 17];
    let (segment, _) = store.write_segment(&mut Cursor::new(&plaintext)).unwrap();
    let mut sink = LargestWrite::default();
    store.read_segment(segment.segment_id, &mut sink).unwrap();
    assert_eq!(sink.total, plaintext.len() as u64);
    assert!(
        sink.largest <= CHUNK,
        "a read handed its sink {} bytes at once, more than one chunk of {CHUNK}",
        sink.largest
    );
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
    let _other_root_guard = RootGuard::new(other_root);
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
            problems.push(format!(
                "{case}: open answered {answer} in place of {expected}"
            ));
        }
        if after != before {
            problems.push(format!(
                "{case}: open changed the files of the store from {before:?} to {after:?}"
            ));
        }
    }
    problems
}

fn edit_key_file(store_dir: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut fields: serde_json::Value =
        serde_json::from_slice(&std::fs::read(KeyFile::path(store_dir)).unwrap()).unwrap();
    edit(&mut fields);
    std::fs::write(
        KeyFile::path(store_dir),
        serde_json::to_vec(&fields).unwrap(),
    )
    .unwrap();
}

fn set_user_version(store_dir: &Path, roots_dir: &Path, root_id: RootId, version: i64) {
    let store = open_over_file_root(store_dir, roots_dir, root_id).unwrap();
    store
        .db()
        .unwrap()
        .connection()
        .execute_batch(&format!("PRAGMA user_version = {version};"))
        .unwrap();
}

fn open_by_the_recorded_root(store_dir: &Path, roots_dir: &Path) -> StoreResult<EncryptedStore> {
    EncryptedStore::open_with(store_dir, DEFAULT_RETENTION, |_, recorded| {
        Ok(Box::new(file_root_custody(
            roots_dir,
            TestProtector::new(0x5a).0,
            recorded,
        )))
    })
}

fn version_answers(cases: &[(&str, DamageToTheDatabase)]) -> Vec<String> {
    cases
        .iter()
        .map(|(case, change)| {
            let dir = tempfile::tempdir().unwrap();
            let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
            change(&store_dir, &roots_dir, root_id);
            let before = files_and_lengths(&store_dir);
            let answer = match open_by_the_recorded_root(&store_dir, &roots_dir) {
                Ok(_) => "Ok".to_string(),
                Err(e) => format!("{e:?}"),
            };
            let changed = files_and_lengths(&store_dir) != before;
            format!(
                "{case}: {answer}{}",
                if changed { ", files changed" } else { "" }
            )
        })
        .collect()
}

#[test]
fn open_refuses_a_format_version_that_this_build_does_not_know_and_changes_no_file() {
    let answers = version_answers(&[
        ("store.key version 999", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["version"] = 999.into())
        }),
        ("store.key version 2", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["version"] = 2.into())
        }),
        ("store.key version 0", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["version"] = 0.into())
        }),
        (
            "store.key version 999 and no other field",
            |store_dir, _, _| {
                std::fs::write(KeyFile::path(store_dir), b"{\"version\": 999}").unwrap()
            },
        ),
        ("user_version 99", |store_dir, roots_dir, root_id| {
            set_user_version(store_dir, roots_dir, root_id, 99)
        }),
        ("user_version 0", |store_dir, roots_dir, root_id| {
            set_user_version(store_dir, roots_dir, root_id, 0)
        }),
        ("user_version -1", |store_dir, roots_dir, root_id| {
            set_user_version(store_dir, roots_dir, root_id, -1)
        }),
    ]);
    assert_eq!(
        answers,
        [
            "store.key version 999: NewerStore { found: 999, supported: 1 }",
            "store.key version 2: NewerStore { found: 2, supported: 1 }",
            "store.key version 0: OlderStore { found: 0, supported: 1 }",
            "store.key version 999 and no other field: NewerStore { found: 999, supported: 1 }",
            "user_version 99: NewerStore { found: 99, supported: 1 }",
            "user_version 0: OlderStore { found: 0, supported: 1 }",
            "user_version -1: OlderStore { found: -1, supported: 1 }",
        ]
    );

    {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        set_user_version(&store_dir, &roots_dir, root_id, 99);
        let log = store_dir.join("store.db-wal");
        std::fs::write(&log, b"").unwrap();

        let refused = open_by_the_recorded_root(&store_dir, &roots_dir).map(drop);

        assert!(
            matches!(
                refused,
                Err(StoreError::NewerStore {
                    found: 99,
                    supported: 1
                })
            ),
            "{refused:?}"
        );
        assert_eq!(
            std::fs::symlink_metadata(&log)
                .map(|meta| (meta.is_file(), meta.len()))
                .ok(),
            Some((true, 0)),
            "a refused open removed or changed an empty store.db-wal"
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        set_user_version(&store_dir, &roots_dir, root_id, 99);
        std::fs::remove_file(store_dir.join("writer.lock")).unwrap();
        std::fs::set_permissions(&store_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let contents = |store_dir: &Path| -> Vec<(PathBuf, Vec<u8>)> {
            let mut files: Vec<(PathBuf, Vec<u8>)> = files_under(store_dir)
                .into_iter()
                .filter(|path| !path.ends_with("writer.lock"))
                .map(|path| {
                    let bytes = std::fs::read(&path).unwrap();
                    (path, bytes)
                })
                .collect();
            files.sort();
            files
        };
        let before = contents(&store_dir);

        let refused = open_by_the_recorded_root(&store_dir, &roots_dir).map(drop);

        let mode = std::fs::metadata(&store_dir).unwrap().permissions().mode() & 0o777;
        assert!(
            matches!(
                refused,
                Err(StoreError::NewerStore {
                    found: 99,
                    supported: 1
                })
            ),
            "{refused:?}"
        );
        assert_eq!(mode, 0o700, "a refused open left the directory at {mode:o}");
        assert!(
            store_dir.join("writer.lock").exists(),
            "a refused open made no writer.lock"
        );
        assert!(
            contents(&store_dir) == before,
            "a refused open changed a file of the store other than writer.lock"
        );
    }
}

#[test]
fn every_form_of_a_key_file_opens_as_the_result_that_names_it() {
    let mut answers = version_answers(&[
        ("control", |_, _, _| {}),
        (
            "names the root of another store",
            |store_dir, roots_dir, _| {
                let other =
                    file_root_custody(roots_dir, TestProtector::new(0x5a).0, RootId::fresh());
                other.create_root().unwrap();
                edit_key_file(store_dir, |v| {
                    v["root_id"] = serde_json::to_value(other.root_id).unwrap()
                });
            },
        ),
        (
            "holds the wrap of another store",
            |store_dir, roots_dir, root_id| {
                let custody = file_root_custody(roots_dir, TestProtector::new(0x5a).0, root_id);
                let another = custody
                    .wrap_store_key(&StoreKey::random().unwrap())
                    .unwrap();
                edit_key_file(store_dir, |v| {
                    v["wrapped_store_key_b64"] = {
                        use base64::Engine;
                        base64::engine::general_purpose::STANDARD
                            .encode(&another.0)
                            .into()
                    }
                });
            },
        ),
        ("zero length", |store_dir, _, _| {
            std::fs::write(KeyFile::path(store_dir), b"").unwrap()
        }),
        ("cut to 1 byte", |store_dir, _, _| {
            let bytes = std::fs::read(KeyFile::path(store_dir)).unwrap();
            std::fs::write(KeyFile::path(store_dir), &bytes[..1]).unwrap();
        }),
        ("cut to half", |store_dir, _, _| {
            let bytes = std::fs::read(KeyFile::path(store_dir)).unwrap();
            std::fs::write(KeyFile::path(store_dir), &bytes[..bytes.len() / 2]).unwrap();
        }),
        ("cut by 1 byte", |store_dir, _, _| {
            let bytes = std::fs::read(KeyFile::path(store_dir)).unwrap();
            std::fs::write(KeyFile::path(store_dir), &bytes[..bytes.len() - 1]).unwrap();
        }),
        ("a wrapped key that is no base64", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["wrapped_store_key_b64"] = "!!!!".into())
        }),
        ("a wrapped key of 8 bytes", |store_dir, _, _| {
            edit_key_file(store_dir, |v| {
                v["wrapped_store_key_b64"] = "AAAAAAAAAAA=".into()
            })
        }),
        ("custody kind tpm", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["custody_kind"] = "tpm".into())
        }),
        ("version as a string", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["version"] = "1".into())
        }),
        ("version below zero", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["version"] = (-1).into())
        }),
        ("version 999", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["version"] = 999.into())
        }),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let first = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let second = open_over_file_root(&store_dir, &roots_dir, root_id).map(drop);
    drop(first);
    answers.push(format!(
        "opened twice: {}",
        match second {
            Ok(()) => "Ok".to_string(),
            Err(StoreError::Locked(_)) => "Locked".to_string(),
            Err(e) => format!("{e:?}"),
        }
    ));
    assert_eq!(
        answers,
        [
            "control: Ok",
            "names the root of another store: Custody(WrapAuthFailed)",
            "holds the wrap of another store: DatabaseUnreadable",
            "zero length: KeyFileUnparseable",
            "cut to 1 byte: KeyFileUnparseable",
            "cut to half: KeyFileUnparseable",
            "cut by 1 byte: KeyFileUnparseable",
            "a wrapped key that is no base64: KeyFileUnparseable",
            "a wrapped key of 8 bytes: KeyFileUnparseable",
            "custody kind tpm: KeyFileUnparseable",
            "version as a string: KeyFileUnparseable",
            "version below zero: KeyFileUnparseable",
            "version 999: NewerStore { found: 999, supported: 1 }",
            "opened twice: Locked",
        ]
    );
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
                    let another = custody
                        .wrap_store_key(&StoreKey::random().unwrap())
                        .unwrap();
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
    let _root_id_guard = RootGuard::new(root_id);
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
    let _root_id_guard = RootGuard::new(root_id);
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

const SHARING_VIOLATION_AT_CREATE: i32 = 32;

fn files_with_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found: Vec<(String, Vec<u8>)> = files_under(dir)
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            let name = path
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (name, bytes)
        })
        .collect();
    found.sort();
    found
}

#[test]
fn create_clears_through_its_handle_and_leaves_the_files_of_a_directory_put_at_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, moved, roots_dir) = (
        dir.path().join("store"),
        dir.path().join("moved-store"),
        dir.path().join("roots"),
    );
    std::fs::create_dir(&store_dir).unwrap();
    let theirs: [(&str, &[u8]); 6] = [
        ("store.db", b"a database of another party"),
        ("store.db-wal", b"its log"),
        ("store.db-shm", b"its shared memory"),
        ("store.key.tmp", b"a file of that name"),
        (
            "segments/01/01000000000000000000000000000000.seg",
            b"a segment of theirs",
        ),
        ("notes.txt", b"and a file of no store"),
    ];
    let mut moved_away: Option<Result<(), Option<i32>>> = None;
    let mut open_lock = |path: &Path| -> io::Result<std::fs::File> {
        if moved_away.is_none() {
            let renamed = std::fs::rename(&store_dir, &moved);
            if renamed.is_ok() {
                for (name, bytes) in theirs {
                    let file = store_dir.join(name);
                    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                    std::fs::write(file, bytes).unwrap();
                }
            }
            moved_away = Some(renamed.map_err(|e| e.raw_os_error()));
        }
        std::fs::File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
    };
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, RootId::fresh());

    let created = EncryptedStore::create_with(
        &store_dir,
        Box::new(custody),
        DEFAULT_RETENTION,
        &mut open_lock,
    )
    .map(drop);

    let expected: Vec<(String, Vec<u8>)> = {
        let mut expected: Vec<(String, Vec<u8>)> = theirs
            .iter()
            .map(|(name, bytes)| (name.to_string(), bytes.to_vec()))
            .chain([("writer.lock".to_string(), Vec::new())])
            .collect();
        expected.sort();
        expected
    };
    say_branch(
        "create_clears_through_its_handle_and_leaves_the_files_of_a_directory_put_at_its_name",
        &format!("the move of the store directory: {moved_away:?}; create: {created:?}"),
    );
    match moved_away {
        Some(Ok(())) => {
            assert_eq!(
                files_with_bytes(&store_dir),
                expected,
                "create ({created:?}) removed or changed files of the directory put at its name \
                 after it took its handle"
            );
            assert!(
                matches!(&created, Err(StoreError::Corrupt(text))
                    if text == "the store directory no longer stands at its name"),
                "create answered {created:?} for a store directory moved away from its name"
            );
        }
        Some(Err(code)) => assert!(
            cfg!(windows) && code == Some(SHARING_VIOLATION_AT_CREATE) && created.is_ok(),
            "the move of the store directory answered {code:?} and create {created:?}"
        ),
        None => panic!("create did not open writer.lock: {created:?}"),
    }
}

#[cfg(unix)]
#[test]
fn create_takes_a_link_at_store_key_as_a_store_and_follows_it_nowhere() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    std::os::unix::fs::symlink(dir.path().join("no-such-key"), KeyFile::path(&store_dir)).unwrap();
    std::fs::write(store_dir.join("store.db"), b"a database that stays").unwrap();
    let before = entries_below(&store_dir);
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh(),
    );

    let created =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).map(drop);

    assert!(
        matches!(created, Err(StoreError::AlreadyExists(_))),
        "create over a store.key that is a link to a missing file answered {created:?}"
    );
    let mut after = entries_below(&store_dir);
    after.retain(|(name, _)| name != "writer.lock");
    assert_eq!(
        after, before,
        "create changed the directory that it refused"
    );
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
    assert_eq!(
        store.db().unwrap().cipher_provider().unwrap(),
        "libtomcrypt"
    );
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
    let _root_id_guard = RootGuard::new(root_id);
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
    let _root_id_guard = RootGuard::new(root_id);
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
        let _root_id_guard = RootGuard::new(root_id);
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

#[test]
fn an_open_after_a_crash_makes_no_shared_memory_file() {
    fn crash_image(dir: &Path) -> (PathBuf, PathBuf, RootId) {
        let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
        let root_id = RootId::fresh();
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        let mut store =
            EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
        store
            .write_segment(&mut Cursor::new(b"crash image"))
            .unwrap();
        let crashed = dir.join("crashed");
        std::fs::create_dir(&crashed).unwrap();
        for name in ["store.key", "store.db", "store.db-wal", "writer.lock"] {
            std::fs::copy(store_dir.join(name), crashed.join(name)).unwrap();
        }
        drop(store);
        assert!(
            wal_len(&crashed) > 0,
            "the copy holds no write-ahead log, so this case shows nothing"
        );
        (crashed, roots_dir, root_id)
    }

    let dir = tempfile::tempdir().unwrap();
    let (crashed, roots_dir, root_id) = crash_image(dir.path());
    assert!(!crashed.join("store.db-shm").exists());
    let opened = open_over_file_root(&crashed, &roots_dir, root_id);
    let shm_while_open = crashed.join("store.db-shm").exists();
    assert!(
        opened.is_ok(),
        "the crash image of a store opened as {:?}",
        opened.as_ref().err()
    );
    drop(opened);
    let shm_after_drop = crashed.join("store.db-shm").exists();
    assert!(
        !shm_while_open && !shm_after_drop,
        "open made store.db-shm in the store directory (while open: {shm_while_open}, after \
         the drop: {shm_after_drop}); files: {:?}",
        files_and_lengths(&crashed)
    );
}

#[test]
fn a_refused_open_after_a_crash_changes_no_file_of_the_store() {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    fn crash_image(dir: &Path) -> (PathBuf, PathBuf, RootId) {
        let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
        let root_id = RootId::fresh();
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        let mut store =
            EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
        store
            .write_segment(&mut Cursor::new(b"crash image"))
            .unwrap();
        let crashed = dir.join("crashed");
        std::fs::create_dir(&crashed).unwrap();
        for name in ["store.key", "store.db", "store.db-wal", "writer.lock"] {
            std::fs::copy(store_dir.join(name), crashed.join(name)).unwrap();
        }
        drop(store);
        assert!(
            wal_len(&crashed) > 0,
            "the copy holds no write-ahead log, so this case shows nothing"
        );
        (crashed, roots_dir, root_id)
    }

    let dir = tempfile::tempdir().unwrap();
    let (crashed, roots_dir, root_id) = crash_image(dir.path());
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let another = custody
        .wrap_store_key(&StoreKey::random().unwrap())
        .unwrap();
    let mut key_file = KeyFile::read(&crashed).unwrap();
    key_file.wrapped_store_key_b64 = STANDARD.encode(&another.0);
    std::fs::write(
        KeyFile::path(&crashed),
        serde_json::to_vec(&key_file).unwrap(),
    )
    .unwrap();

    let before = files_and_lengths(&crashed);
    let db_before = std::fs::read(crashed.join("store.db")).unwrap();
    let wal_before = std::fs::read(crashed.join("store.db-wal")).unwrap();
    let answer = open_over_file_root(&crashed, &roots_dir, root_id)
        .map(drop)
        .map_err(|e| format!("{e:?}"));
    let after = files_and_lengths(&crashed);
    let db_after = std::fs::read(crashed.join("store.db")).unwrap();
    let wal_after = std::fs::read(crashed.join("store.db-wal")).ok();
    assert_eq!(answer, Err("DatabaseUnreadable".to_string()));
    assert!(
        after == before && db_after == db_before && wal_after.as_deref() == Some(&wal_before[..]),
        "an open that answered DatabaseUnreadable changed the store: files {before:?} became \
         {after:?}; store.db bytes changed: {}; store.db-wal kept: {}",
        db_after != db_before,
        wal_after.is_some()
    );
}

#[test]
fn an_empty_database_beside_a_write_ahead_log_opens_as_database_missing_and_keeps_the_log() {
    fn crash_image(dir: &Path) -> (PathBuf, PathBuf, RootId) {
        let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
        let root_id = RootId::fresh();
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        let mut store =
            EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
        store
            .write_segment(&mut Cursor::new(b"crash image"))
            .unwrap();
        let crashed = dir.join("crashed");
        std::fs::create_dir(&crashed).unwrap();
        for name in ["store.key", "store.db", "store.db-wal", "writer.lock"] {
            std::fs::copy(store_dir.join(name), crashed.join(name)).unwrap();
        }
        drop(store);
        assert!(
            wal_len(&crashed) > 0,
            "the copy holds no write-ahead log, so this case shows nothing"
        );
        (crashed, roots_dir, root_id)
    }

    let dir = tempfile::tempdir().unwrap();
    let (crashed, roots_dir, root_id) = crash_image(dir.path());
    cut_store_db_to(&crashed, 0);
    let before = files_and_lengths(&crashed);
    let answer = open_over_file_root(&crashed, &roots_dir, root_id)
        .map(drop)
        .map_err(|e| format!("{e:?}"));
    let after = files_and_lengths(&crashed);
    assert_eq!(answer, Err("DatabaseMissing".to_string()));
    assert_eq!(
        after, before,
        "an open that answered DatabaseMissing changed the files of a sealed store"
    );
}

#[test]
fn a_refused_newer_store_whose_log_holds_frames_keeps_its_database_and_its_log() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let copy_dir = dir.path().join("copy");
    std::fs::create_dir(&copy_dir).unwrap();
    let held = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    held.db()
        .unwrap()
        .connection()
        .execute_batch("PRAGMA user_version = 99;")
        .unwrap();
    assert!(
        wal_len(&store_dir) > 0,
        "the version write left no frame in the write-ahead log"
    );
    for name in ["store.key", "store.db", "store.db-wal"] {
        std::fs::write(
            copy_dir.join(name),
            std::fs::read(store_dir.join(name)).unwrap(),
        )
        .unwrap();
    }
    drop(held);
    let db_before = std::fs::read(copy_dir.join("store.db")).unwrap();
    let wal_before = std::fs::read(copy_dir.join("store.db-wal")).unwrap();

    let answer = open_by_the_recorded_root(&copy_dir, &roots_dir).map(drop);

    let db_after = std::fs::read(copy_dir.join("store.db")).unwrap();
    let wal_after = std::fs::read(copy_dir.join("store.db-wal")).ok();
    assert!(
        matches!(
            answer,
            Err(StoreError::NewerStore {
                found: 99,
                supported: 1
            })
        ),
        "a store.db of user_version 99 in its log opened as {answer:?}"
    );
    assert!(
        db_after == db_before,
        "the refused open wrote into store.db: {} bytes before, {} after",
        db_before.len(),
        db_after.len()
    );
    assert!(
        wal_after.as_deref() == Some(&wal_before[..]),
        "the refused open changed or removed store.db-wal: {} bytes before, {:?} after",
        wal_before.len(),
        wal_after.as_ref().map(Vec::len)
    );
}

#[cfg(unix)]
#[test]
fn delete_all_writes_nothing_through_a_root_file_or_a_partial_root_file_that_is_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let root_path =
        FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0).root_path(root_id);
    let partial_path = root_path.with_extension("root.tmp");
    let owned: &[u8] = b"bytes that the store does not own";
    let root_target = outside.path().join("root-target");
    let partial_target = outside.path().join("partial-target");
    std::fs::write(&root_target, owned).unwrap();
    std::fs::write(&partial_target, owned).unwrap();
    std::fs::remove_file(&root_path).unwrap();
    std::os::unix::fs::symlink(&root_target, &root_path).unwrap();
    std::os::unix::fs::symlink(&partial_target, &partial_path).unwrap();

    let deleted = store.crypto_shred_all();

    assert_eq!(
        std::fs::read(&root_target).unwrap(),
        owned,
        "delete-all ({deleted:?}) wrote through a root file that is a link"
    );
    assert_eq!(
        std::fs::read(&partial_target).unwrap(),
        owned,
        "delete-all ({deleted:?}) wrote through a partial root file that is a link"
    );
}

#[cfg(unix)]
#[test]
fn create_makes_no_file_through_a_writer_lock_that_is_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    let target = outside.path().join("made-through-the-link");
    std::os::unix::fs::symlink(&target, store_dir.join("writer.lock")).unwrap();
    let root_id = RootId::fresh();
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        root_id,
    );

    let created =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).map(drop);

    assert!(
        std::fs::symlink_metadata(&target).is_err(),
        "create ({created:?}) made a file outside the store through a writer.lock that is a link"
    );
}

#[cfg(unix)]
#[test]
fn a_segment_write_puts_no_file_through_a_shard_that_links_out_of_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path().join("store").as_path());
    let shard = Uuid::now_v7().simple().to_string()[..2].to_string();
    let segments = segment::segments_root(store.dir());
    super::durable::create_private_dir_all(&segments).unwrap();
    link_dir(outside.path(), &segments.join(&shard));

    let written = store
        .write_segment(&mut Cursor::new(b"through the link"))
        .map(|(seg_ref, _)| seg_ref.segment_id);

    assert_eq!(
        files_in(outside.path()),
        0,
        "the write ({written:?}) put a file outside the store through a linked shard"
    );
}

fn regular_file_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(below) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&below) else {
            continue;
        };
        for entry in entries.flatten() {
            let meta = std::fs::symlink_metadata(entry.path()).unwrap();
            if meta.is_dir() && !meta.file_type().is_symlink() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

enum ShardSwap {
    ForAForeignDirectory,
    ForALinkToItsMovedSelf,
}

fn a_segment_write_whose_shard_is_swapped_at_the_create_of_its_partial(swap: ShardSwap) {
    use super::durable::test_support::{Call, RecordingFs};

    let dir = tempfile::tempdir().unwrap();
    let (foreign, aside) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(foreign.path().join("notes.txt"), b"a file of the person").unwrap();
    let roots_dir = dir.path().join("roots");
    let root_id = RootId::fresh();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let mut store = EncryptedStore::create(
        &dir.path().join("store"),
        Box::new(custody),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let moved = aside.path().join("moved-shard");
    let target = match swap {
        ShardSwap::ForAForeignDirectory => foreign.path().to_path_buf(),
        ShardSwap::ForALinkToItsMovedSelf => moved.clone(),
    };
    let answer = SwapAnswer::default();
    let fs = {
        let (moved, target, answer) = (moved.clone(), target.clone(), Arc::clone(&answer));
        RecordingFs::observed_by(move |call| {
            if let Call::CreateNew(partial) = call {
                swap_shard_for_a_link(partial.parent().unwrap(), &moved, &target, &answer);
            }
        })
    };
    store.use_fs(Arc::new(fs));
    let (foreign_before, aside_before) =
        (entries_below(foreign.path()), entries_below(aside.path()));

    let written = store
        .write_segment(&mut Cursor::new(b"through the swapped shard"))
        .map(drop);

    let swapped = *answer.lock().unwrap();
    let size = store.size_bytes();
    let in_the_store = regular_file_bytes(store.dir());
    let aside_after = entries_below(aside.path());
    let foreign_after = entries_below(foreign.path());
    let deleted = store.crypto_shred_all().map(drop);

    assert_eq!(
        foreign_after, foreign_before,
        "the write ({written:?}) put a file into a foreign directory through a swapped shard"
    );
    say_branch(
        match swap {
            ShardSwap::ForAForeignDirectory => {
                "a_segment_write_puts_no_file_into_a_foreign_directory_put_in_place_of_its_shard_at_the_create"
            }
            ShardSwap::ForALinkToItsMovedSelf => {
                "a_segment_write_whose_shard_is_moved_away_and_linked_back_at_the_create_answers_corrupt"
            }
        },
        &format!("the move of the shard: {swapped:?}; the write: {written:?}"),
    );
    match swapped {
        Some(Ok(())) => {
            assert_eq!(
                aside_after, aside_before,
                "the write ({written:?}) left a file in the shard that was moved out of the store"
            );
            if let ShardSwap::ForALinkToItsMovedSelf = swap {
                assert!(
                    matches!(&written, Err(StoreError::Corrupt(text))
                        if text == "a shard of the segments directory of the store is a link"),
                    "a write whose shard was moved away and linked back answered {written:?}"
                );
            }
        }
        Some(Err(code)) => assert!(
            cfg!(windows) && code == Some(SHARING_VIOLATION) && written.is_ok(),
            "the move of the shard answered {code:?} and the write {written:?}"
        ),
        None => panic!("the write made no partial file through the seam: {written:?}"),
    }
    assert!(
        matches!(size, Ok(bytes) if bytes == in_the_store),
        "size_bytes answered {size:?}, and the regular files of the store hold {in_the_store} bytes"
    );
    assert!(deleted.is_ok(), "delete-all answered {deleted:?}");
    assert_eq!(
        (entries_below(foreign.path()), entries_below(aside.path())),
        (foreign_before, aside_before),
        "delete-all changed what lies outside the store"
    );
}

#[test]
fn a_segment_write_puts_no_file_into_a_foreign_directory_put_in_place_of_its_shard_at_the_create() {
    a_segment_write_whose_shard_is_swapped_at_the_create_of_its_partial(
        ShardSwap::ForAForeignDirectory,
    );
}

#[test]
fn a_segment_write_whose_shard_is_moved_away_and_linked_back_at_the_create_answers_corrupt() {
    a_segment_write_whose_shard_is_swapped_at_the_create_of_its_partial(
        ShardSwap::ForALinkToItsMovedSelf,
    );
}

#[cfg(unix)]
#[test]
fn a_root_write_tightens_a_roots_directory_made_at_0755_to_0700() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let roots_dir = dir.path().join("roots");
    std::fs::create_dir(&roots_dir).unwrap();
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let root_id = RootId::fresh();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);

    let created = EncryptedStore::create(
        &dir.path().join("store"),
        Box::new(custody),
        DEFAULT_RETENTION,
    )
    .map(drop);

    let mode = std::fs::metadata(&roots_dir).unwrap().permissions().mode() & 0o777;
    assert!(created.is_ok(), "{created:?}");
    assert_eq!(
        mode, 0o700,
        "a root write left the roots directory at {mode:o}"
    );
}

#[cfg(unix)]
#[test]
fn the_keyless_sweep_leaves_a_link_to_a_file_that_carries_a_segment_name() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut store = super::db::test_support::make_store(dir.path());
    let (live, _) = store.write_segment(&mut Cursor::new(b"live")).unwrap();
    let shard = segment_path(dir.path(), live.segment_id)
        .parent()
        .unwrap()
        .to_path_buf();
    let target = outside.path().join("target");
    std::fs::write(&target, b"outside").unwrap();
    let link = shard.join(format!(
        "{}{}.seg",
        &live.segment_id.simple().to_string()[..2],
        &Uuid::now_v7().simple().to_string()[2..]
    ));
    let partial_link = link.with_extension("seg.tmp");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    std::os::unix::fs::symlink(&target, &partial_link).unwrap();

    let sweep = store.remove_keyless_files();

    assert!(
        std::fs::symlink_metadata(&link).is_ok()
            && std::fs::symlink_metadata(&partial_link).is_ok(),
        "the sweep ({sweep:?}) removed a link that is no regular file"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"outside");
    assert!(
        matches!(sweep, Ok(ref s) if s.removed == 0 && s.not_store_files == 2),
        "{sweep:?}"
    );
    assert!(segment_path(dir.path(), live.segment_id).exists());
}

#[cfg(unix)]
#[test]
fn create_tightens_an_existing_store_directory_made_at_0755_to_0700() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    std::fs::create_dir(&store_dir).unwrap();
    std::fs::set_permissions(&store_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, RootId::fresh());
    let created =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).map(drop);
    let mode = std::fs::metadata(&store_dir).unwrap().permissions().mode() & 0o777;
    assert!(created.is_ok(), "{created:?}");
    assert_eq!(mode, 0o700, "create left the store directory at {mode:o}");
}

#[test]
fn a_durable_write_and_a_durable_removal_report_a_directory_sync_that_fails() {
    use super::durable::test_support::{Kind, RecordingFs};
    use super::durable::{remove_durably, write_durably, Dir};
    use std::ffi::OsStr;

    let dir = tempfile::tempdir().unwrap();
    let handle = Dir::open(dir.path()).unwrap();
    let (temp, target) = (OsStr::new("file.tmp"), OsStr::new("file"));
    let fs = RecordingFs::new();
    fs.fail(Kind::SyncDir);
    let written: io::Result<()> = write_durably(
        &fs,
        &handle,
        temp,
        target,
        |e| e,
        |_| Ok(()),
        |_, _| Ok(()),
        |out| out.write_all(b"bytes"),
    );
    assert_eq!(
        written.map_err(|e| e.kind()),
        Err(io::ErrorKind::StorageFull),
        "a durable write whose directory sync failed reported success"
    );
    let removed = remove_durably(&fs, &handle, target);
    assert_eq!(
        removed.map_err(|e| e.kind()),
        Err(io::ErrorKind::StorageFull),
        "a durable removal whose directory sync failed reported success"
    );
}

#[test]
fn a_durable_write_whose_discard_fails_answers_with_the_error_of_the_discard() {
    use super::durable::test_support::{Kind, RecordingFs};
    use super::durable::{write_durably, Dir};
    use std::ffi::OsStr;

    let dir = tempfile::tempdir().unwrap();
    let handle = Dir::open(dir.path()).unwrap();
    let fs = RecordingFs::new();
    fs.fail(Kind::Rename);
    let written: io::Result<()> = write_durably(
        &fs,
        &handle,
        OsStr::new("file.tmp"),
        OsStr::new("file"),
        |e| e,
        |_| Ok(()),
        |_, _| Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        |out| out.write_all(b"bytes"),
    );
    assert_eq!(
        written.map_err(|e| e.kind()),
        Err(io::ErrorKind::PermissionDenied),
        "the error of a discard that left the temporary file did not take the place of the first error"
    );
}

#[test]
fn the_unwrap_time_of_an_open_holds_no_time_of_the_custody_rebuild() {
    const SLOW_REBUILD: Duration = Duration::from_millis(1100);
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = EncryptedStore::open_with(&store_dir, DEFAULT_RETENTION, |_, _| {
        std::thread::sleep(SLOW_REBUILD);
        Ok(Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )))
    })
    .unwrap();
    assert!(
        store.unwrap_time() < SLOW_REBUILD,
        "a custody rebuild of {SLOW_REBUILD:?} before a quick unwrap gave unwrap_time {:?}",
        store.unwrap_time()
    );
}

#[cfg(unix)]
#[test]
fn a_shred_reads_no_size_through_a_link_at_the_log_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store
        .write_segment_with_expiry(&mut Cursor::new(b"expired"), 1, 2)
        .unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("big");
    std::fs::write(&target, vec![7u8; 100]).unwrap();
    let wal = store_dir.join("store.db-wal");
    std::fs::rename(&wal, dir.path().join("log-moved-aside")).unwrap();
    std::os::unix::fs::symlink(&target, &wal).unwrap();

    let shred = store.crypto_shred_expired().map(drop);

    assert!(
        matches!(&shred, Err(StoreError::Corrupt(text))
            if text == "a file beside the database of the store is a link"),
        "a shred with a link at store.db-wal answered {shred:?}"
    );
    assert_eq!(std::fs::read(&target).unwrap(), vec![7u8; 100]);
}

#[cfg(unix)]
fn a_write_store(dir: &Path) -> (PathBuf, EncryptedStore) {
    let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, RootId::fresh());
    let store = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
    (store_dir, store)
}

#[cfg(unix)]
#[test]
fn a_segment_write_whose_partial_file_became_a_link_at_its_sync_publishes_nothing() {
    use super::durable::test_support::{Call, RecordingFs};

    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("a-file-of-another-party");
    std::fs::write(&target, b"bytes that stay").unwrap();
    let (store_dir, mut store) = a_write_store(dir.path());
    let swapped = Arc::new(std::sync::Mutex::new(false));
    let fs = {
        let (target, aside, swapped) = (
            target.clone(),
            dir.path().join("partial-moved-aside"),
            Arc::clone(&swapped),
        );
        RecordingFs::observed_by(move |call| {
            if let Call::SyncFile(partial) = call {
                let mut swapped = swapped.lock().unwrap();
                if !*swapped {
                    std::fs::rename(partial, &aside).unwrap();
                    std::os::unix::fs::symlink(&target, partial).unwrap();
                    *swapped = true;
                }
            }
        })
    };
    store.use_fs(Arc::new(fs));

    let written = store
        .write_segment(&mut Cursor::new(
            b"a segment whose partial file became a link",
        ))
        .map(drop);

    assert!(*swapped.lock().unwrap(), "the seam did not run");
    assert!(
        matches!(&written, Err(StoreError::Corrupt(text))
            if text == "a segment file of the store is a link"),
        "a write whose partial file became a link answered {written:?}"
    );
    let published: Vec<String> = entries_below(&segment::segments_root(&store_dir))
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| name.ends_with(".seg"))
        .collect();
    assert_eq!(
        published,
        Vec::<String>::new(),
        "the write published a segment"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"bytes that stay");
}

#[cfg(unix)]
#[test]
fn a_segment_write_whose_shard_name_takes_a_plain_directory_at_the_create_answers_corrupt() {
    use super::durable::test_support::{Call, RecordingFs};

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, mut store) = a_write_store(dir.path());
    let moved = dir.path().join("moved-shard");
    let swapped = Arc::new(std::sync::Mutex::new(false));
    let fs = {
        let (moved, swapped) = (moved.clone(), Arc::clone(&swapped));
        RecordingFs::observed_by(move |call| {
            if let Call::CreateNew(partial) = call {
                let mut swapped = swapped.lock().unwrap();
                if !*swapped {
                    let shard = partial.parent().unwrap();
                    std::fs::rename(shard, &moved).unwrap();
                    std::fs::create_dir(shard).unwrap();
                    *swapped = true;
                }
            }
        })
    };
    store.use_fs(Arc::new(fs));

    let written = store
        .write_segment(&mut Cursor::new(b"a segment whose shard was replaced"))
        .map(drop);

    assert!(*swapped.lock().unwrap(), "the seam did not run");
    assert!(
        matches!(&written, Err(StoreError::Corrupt(text))
            if text == "a shard of the segments directory of the store is a link"),
        "a write whose shard name took a plain directory answered {written:?}"
    );
    assert_eq!(
        (
            entries_below(&segment::segments_root(&store_dir)),
            entries_below(&moved)
        ),
        (Vec::new(), Vec::new()),
        "the write left or published a file"
    );
}

#[cfg(unix)]
const CALL_BOUND: Duration = Duration::from_secs(5);

/// The answer of `call`, made on a thread of its own, when it comes within
/// `bound`; `None` when it does not, and then that thread is left behind.
#[cfg(unix)]
fn within<T: Send + 'static>(
    bound: Duration,
    call: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (sender, answers) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(call());
    });
    answers.recv_timeout(bound).ok()
}

fn say_branch(test: &str, branch: &str) {
    let _ = writeln!(std::io::stderr(), "BRANCH {test}: {branch}");
}

/// Moves `name` away and puts a link to `target` there; the answer of the
/// move, which on Windows a held handle refuses with error 32.
fn put_a_link_in_place_of(name: &Path, aside: &Path, target: &Path) -> Result<(), Option<i32>> {
    std::fs::rename(name, aside).map_err(|e| e.raw_os_error())?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, name).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, name).unwrap();
    Ok(())
}

fn nth_call(n: usize, mut act: impl FnMut() + 'static) -> impl FnMut() + 'static {
    let mut calls = 0;
    move || {
        calls += 1;
        if calls == n {
            act();
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CallSite {
    Probe,
    Existing,
    New,
}

fn a_link_put_at_store_db_after_the_last_look(site: CallSite) {
    use super::db::SEAM_AFTER_THE_LAST_LOOK;
    use super::failpoint;

    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let (other_dir, other_roots, other_root, _) = file_root_store(outside.path());
    let held = open_over_file_root(&other_dir, &other_roots, other_root).unwrap();
    let target = match site {
        CallSite::Probe => other_dir.join("store.db"),
        CallSite::Existing => {
            let copy = outside.path().join("a-copy.db");
            std::fs::copy(store_dir.join("store.db"), &copy).unwrap();
            copy
        }
        CallSite::New => {
            let empty = outside.path().join("an-empty.db");
            std::fs::write(&empty, b"").unwrap();
            empty
        }
    };
    if let CallSite::New = site {
        std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
    }
    let target_dir = target.parent().unwrap().to_path_buf();
    let (bytes_before, beside_before) = (std::fs::read(&target).unwrap(), names_in(&target_dir));
    let moved = Arc::new(std::sync::Mutex::new(None));
    let act = {
        let (name, aside, target, moved) = (
            store_dir.join("store.db"),
            dir.path().join("store-db-aside"),
            target.clone(),
            Arc::clone(&moved),
        );
        move || *moved.lock().unwrap() = Some(put_a_link_in_place_of(&name, &aside, &target))
    };
    let _seam = failpoint::at(
        SEAM_AFTER_THE_LAST_LOOK,
        nth_call(if let CallSite::Existing = site { 2 } else { 1 }, act),
    );

    let answer = match site {
        CallSite::Probe | CallSite::Existing => {
            answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id))
        }
        CallSite::New => {
            let custody = file_root_custody(
                &dir.path().join("roots-new"),
                TestProtector::new(0x5a).0,
                RootId::fresh(),
            );
            answer_text(&EncryptedStore::create(
                &store_dir,
                Box::new(custody),
                DEFAULT_RETENTION,
            ))
        }
    };

    let moved = *moved.lock().unwrap();
    let test = format!("a link at store.db after the last look, {site:?}");
    match moved {
        Some(Ok(())) => {
            say_branch(&test, "the name was moved and a link put there");
            assert!(
                answer == "Corrupt(\"the database file of the store is a link\")",
                "{test}: the call answered {answer}"
            );
        }
        Some(Err(code)) => {
            say_branch(&test, &format!("the move was refused with {code:?}"));
            assert!(
                cfg!(windows) && code == Some(32),
                "{test}: the move answered {code:?}"
            );
        }
        None => panic!("{test}: the seam did not run; the call answered {answer}"),
    }
    assert!(
        std::fs::read(&target).unwrap() == bytes_before,
        "{test}: the target's bytes changed ({answer})"
    );
    assert_eq!(
        names_in(&target_dir),
        beside_before,
        "{test}: an entry appeared beside the target ({answer})"
    );
    drop(held);
}

#[test]
fn the_holder_probe_opens_no_link_put_at_store_db_after_its_last_look() {
    a_link_put_at_store_db_after_the_last_look(CallSite::Probe);
}

#[test]
fn the_open_of_an_existing_database_opens_no_link_put_at_store_db_after_its_last_look() {
    a_link_put_at_store_db_after_the_last_look(CallSite::Existing);
}

#[test]
fn the_open_of_a_new_database_opens_no_link_put_at_store_db_after_its_last_look() {
    a_link_put_at_store_db_after_the_last_look(CallSite::New);
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
enum Planted {
    Link,
    Directory,
    Fifo,
}

#[cfg(unix)]
fn plant(at: &Path, what: Planted, outside: &Path) {
    if at.exists() || std::fs::symlink_metadata(at).is_ok() {
        std::fs::rename(at, outside.join("moved-aside")).unwrap();
    }
    match what {
        Planted::Link => {
            let target = outside.join("target");
            std::fs::write(&target, b"bytes of another party").unwrap();
            std::os::unix::fs::symlink(&target, at).unwrap();
        }
        Planted::Directory => std::fs::create_dir(at).unwrap(),
        Planted::Fifo => make_fifo(at),
    }
}

#[cfg(unix)]
#[test]
fn open_and_create_answer_a_link_a_directory_or_a_fifo_at_each_database_name_within_a_bound() {
    let mut answers = Vec::new();
    for name in [
        "store.db",
        "store.db-wal",
        "store.db-shm",
        "store.db-journal",
    ] {
        for what in [Planted::Link, Planted::Directory, Planted::Fifo] {
            for call in ["open", "create"] {
                let dir = tempfile::tempdir().unwrap();
                let outside = tempfile::tempdir().unwrap();
                let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
                if call == "create" {
                    std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
                }
                plant(&store_dir.join(name), what, outside.path());
                let made_roots = dir.path().join("roots-of-create");
                let answer = within(CALL_BOUND, move || {
                    if call == "open" {
                        answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id))
                    } else {
                        let custody = file_root_custody(
                            &made_roots,
                            TestProtector::new(0x5a).0,
                            RootId::fresh(),
                        );
                        answer_text(&EncryptedStore::create(
                            &store_dir,
                            Box::new(custody),
                            DEFAULT_RETENTION,
                        ))
                    }
                });
                let target = outside.path().join("target");
                if target.exists() {
                    assert_eq!(std::fs::read(&target).unwrap(), b"bytes of another party");
                }
                let answer = answer.unwrap_or_else(|| {
                    panic!("{call} with a {what:?} at {name} gave no answer in {CALL_BOUND:?}")
                });
                answers.push(format!("{call} {what:?} at {name}: {answer}"));
            }
        }
    }
    let _ = writeln!(std::io::stderr(), "ANSWERS {answers:#?}");
    let linked_db = "Corrupt(\"the database file of the store is a link\")";
    let linked_side = "Corrupt(\"a file beside the database of the store is a link\")";
    let mut expected = Vec::new();
    for name in [
        "store.db",
        "store.db-wal",
        "store.db-shm",
        "store.db-journal",
    ] {
        for what in ["Link", "Directory", "Fifo"] {
            let open = match (name, what) {
                ("store.db", "Link") => linked_db,
                (_, "Link") => linked_side,
                _ => "DatabaseUnreadable",
            };
            let create = match (name, what) {
                ("store.db-journal", "Link") => linked_side,
                ("store.db-journal", _) => "DatabaseUnreadable",
                (_, "Directory") => "Io(PermissionDenied)",
                _ => "Ok",
            };
            expected.push(format!("open {what} at {name}: {open}"));
            expected.push(format!("create {what} at {name}: {create}"));
        }
    }
    assert_eq!(answers, expected);
}

#[cfg(unix)]
fn open_with_a_seam(
    store_dir: &Path,
    roots_dir: &Path,
    root_id: RootId,
    seam: &'static str,
    nth: usize,
    act: impl FnMut() + Send + 'static,
) -> Option<String> {
    let (store, roots) = (store_dir.to_path_buf(), roots_dir.to_path_buf());
    within(CALL_BOUND, move || {
        let _seam = super::failpoint::at(seam, nth_call(nth, act));
        answer_text(&open_over_file_root(&store, &roots, root_id))
    })
}

#[cfg(unix)]
#[test]
fn open_answers_not_locked_for_a_held_store_put_at_its_name_before_the_probes_look() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let (other_dir, other_roots, other_root, _) = file_root_store(outside.path());
    let held = open_over_file_root(&other_dir, &other_roots, other_root).unwrap();
    let theirs = files_with_bytes(&other_dir);
    let act = {
        let (store, moved, other) = (
            store_dir.clone(),
            dir.path().join("moved-store"),
            other_dir.clone(),
        );
        move || {
            std::fs::rename(&store, &moved).unwrap();
            std::fs::rename(&other, &store).unwrap();
        }
    };

    let answer = open_with_a_seam(
        &store_dir,
        &roots_dir,
        root_id,
        super::db::SEAM_BEFORE_THE_LOOK,
        1,
        act,
    );

    let theirs_after = files_with_bytes(&store_dir);
    drop(held);
    assert_eq!(
        answer.as_deref(),
        Some("Corrupt(\"the store directory no longer stands at its name\")"),
        "open with a held store put at its name before the probe's look"
    );
    assert!(
        theirs_after == theirs,
        "the probe changed the database of the store put at the name"
    );
}

#[cfg(unix)]
#[test]
fn open_answers_a_fifo_swapped_in_at_store_db_before_the_probes_look_within_a_bound() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let act = {
        let (db, aside) = (
            store_dir.join("store.db"),
            dir.path().join("store-db-aside"),
        );
        move || {
            std::fs::rename(&db, &aside).unwrap();
            make_fifo(&db);
        }
    };

    let answer = open_with_a_seam(
        &store_dir,
        &roots_dir,
        root_id,
        super::db::SEAM_BEFORE_THE_LOOK,
        1,
        act,
    );

    assert_eq!(
        answer.as_deref(),
        Some("DatabaseUnreadable"),
        "within {CALL_BOUND:?}"
    );
}

#[cfg(unix)]
#[test]
fn open_answers_a_fifo_at_store_db_journal_within_a_bound() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    make_fifo(&store_dir.join("store.db-journal"));

    let answer = within(CALL_BOUND, move || {
        answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id))
    });

    assert_eq!(
        answer.as_deref(),
        Some("DatabaseUnreadable"),
        "within {CALL_BOUND:?}"
    );
}

#[cfg(unix)]
#[test]
fn the_mode_step_sets_no_mode_through_a_link_put_at_store_db_after_its_type_read() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let target = outside.path().join("a-file-at-0644");
    std::fs::write(&target, b"bytes of another party").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
    let act = {
        let (db, aside, target) = (
            store_dir.join("store.db"),
            dir.path().join("store-db-aside"),
            target.clone(),
        );
        move || {
            std::fs::rename(&db, &aside).unwrap();
            std::os::unix::fs::symlink(&target, &db).unwrap();
        }
    };

    let answer = open_with_a_seam(
        &store_dir,
        &roots_dir,
        root_id,
        super::db::SEAM_BEFORE_THE_MODE_STEP,
        1,
        act,
    );

    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        (answer.as_deref(), mode),
        (
            Some("Corrupt(\"the database file of the store is a link\")"),
            0o644
        ),
        "the mode step and a link put at store.db after its type read"
    );
}

#[cfg(unix)]
#[test]
fn a_store_db_that_goes_away_before_the_mode_step_answers_database_missing() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let act = {
        let (db, aside) = (
            store_dir.join("store.db"),
            dir.path().join("store-db-aside"),
        );
        move || std::fs::rename(&db, &aside).unwrap()
    };

    let answer = open_with_a_seam(
        &store_dir,
        &roots_dir,
        root_id,
        super::db::SEAM_BEFORE_THE_MODE_STEP,
        1,
        act,
    );

    assert_eq!(answer.as_deref(), Some("DatabaseMissing"));
}

#[cfg(unix)]
#[test]
fn a_store_db_that_becomes_a_fifo_before_the_mode_step_answers_database_unreadable_within_a_bound()
{
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let act = {
        let (db, aside) = (
            store_dir.join("store.db"),
            dir.path().join("store-db-aside"),
        );
        move || {
            std::fs::rename(&db, &aside).unwrap();
            make_fifo(&db);
        }
    };

    let answer = open_with_a_seam(
        &store_dir,
        &roots_dir,
        root_id,
        super::db::SEAM_BEFORE_THE_MODE_STEP,
        1,
        act,
    );

    assert_eq!(
        answer.as_deref(),
        Some("DatabaseUnreadable"),
        "within {CALL_BOUND:?}"
    );
}

/// An open store whose directory is then moved away, with another directory
/// put at its name that holds `theirs`; the store keeps its handle.
#[cfg(unix)]
fn a_store_moved_away_from_its_name(
    dir: &Path,
    theirs: &[(&str, &[u8])],
) -> (PathBuf, PathBuf, EncryptedStore, Uuid) {
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir);
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let moved = dir.join("moved-store");
    std::fs::rename(&store_dir, &moved).unwrap();
    for (name, bytes) in theirs {
        let file = store_dir.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    (store_dir, moved, store, segment_id)
}

#[cfg(unix)]
fn shard_of(segment_id: Uuid) -> String {
    segment_id.simple().to_string()[..2].to_string()
}

#[cfg(unix)]
#[test]
fn a_sweep_after_the_store_directory_moved_acts_in_its_own_directory_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let shard = shard_of(segment_id);
    let ours = format!("{shard}{}.seg", "1".repeat(30));
    let theirs_name = format!("segments/{shard}/{shard}{}.seg", "2".repeat(30));
    std::fs::write(
        store_dir.join("segments").join(&shard).join(&ours),
        b"keyless and ours",
    )
    .unwrap();
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let moved = dir.path().join("moved-store");
    std::fs::rename(&store_dir, &moved).unwrap();
    let file = store_dir.join(&theirs_name);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"theirs").unwrap();
    let theirs = files_with_bytes(&store_dir);

    let swept = store
        .remove_keyless_files()
        .map(|s| (s.removed, s.not_store_files));

    assert_eq!(
        (
            swept.map_err(|e| format!("{e:?}")),
            files_with_bytes(&store_dir)
        ),
        (Ok((1, 0)), theirs),
        "the sweep after a move"
    );
    assert!(
        !moved.join("segments").join(&shard).join(&ours).exists(),
        "the sweep left the store's own keyless file"
    );
}

#[cfg(unix)]
#[test]
fn a_write_and_its_read_after_the_store_directory_moved_act_in_its_own_directory_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, _moved, mut store, segment_id) =
        a_store_moved_away_from_its_name(dir.path(), &[("notes.txt", b"theirs")]);
    let _ = segment_id;
    let theirs = files_with_bytes(&store_dir);

    let written = store
        .write_segment(&mut Cursor::new(b"written after the move"))
        .map(|(seg_ref, _)| seg_ref.segment_id);
    let read = written
        .as_ref()
        .map_err(|e| format!("{e:?}"))
        .and_then(|id| {
            let mut out = Vec::new();
            store
                .read_segment(*id, &mut out)
                .map(|_| out)
                .map_err(|e| format!("{e:?}"))
        });

    assert_eq!(
        read,
        Ok(b"written after the move".to_vec()),
        "the write {written:?}"
    );
    assert_eq!(
        files_with_bytes(&store_dir),
        theirs,
        "the write changed the directory at the name"
    );
}

#[cfg(unix)]
#[test]
fn the_size_after_the_store_directory_moved_counts_its_own_directory_alone() {
    let dir = tempfile::tempdir().unwrap();
    let big = vec![9u8; 1 << 20];
    let (store_dir, moved, store, segment_id) = a_store_moved_away_from_its_name(dir.path(), &[]);
    let file = store_dir
        .join("segments")
        .join(shard_of(segment_id))
        .join("big");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, &big).unwrap();

    let size = store.size_bytes().map_err(|e| format!("{e:?}"));

    assert_eq!(
        size,
        Ok(regular_file_bytes(&moved)),
        "size_bytes after a move"
    );
}

#[cfg(unix)]
#[test]
fn create_answers_a_moved_directory_when_a_link_to_it_stands_at_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    let moved = dir.path().join("moved-store");
    let mut linked = false;
    let mut open_lock = |path: &Path| -> io::Result<std::fs::File> {
        if !linked {
            linked = true;
            std::fs::rename(&store_dir, &moved).unwrap();
            std::os::unix::fs::symlink(&moved, &store_dir).unwrap();
        }
        std::fs::File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
    };
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh(),
    );

    let created = EncryptedStore::create_with(
        &store_dir,
        Box::new(custody),
        DEFAULT_RETENTION,
        &mut open_lock,
    )
    .map(drop);

    assert!(linked, "the seam did not run");
    assert!(
        matches!(&created, Err(StoreError::Corrupt(text))
            if text == "the store directory no longer stands at its name"),
        "create with a link to its own directory at its name answered {created:?}"
    );
}

#[cfg(unix)]
#[test]
fn create_removes_a_keyless_store_key_tmp_and_makes_its_store() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    std::fs::write(
        store_dir.join("store.key.tmp"),
        b"left by a create that stopped",
    )
    .unwrap();
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh(),
    );

    let created =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).map(drop);

    assert!(
        created.is_ok(),
        "create over a keyless store.key.tmp answered {created:?}"
    );
    assert!(KeyFile::path(&store_dir).exists());
    assert!(!store_dir.join("store.key.tmp").exists());
}

#[cfg(unix)]
#[test]
fn the_size_skips_a_directory_that_became_a_link_after_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("big"), vec![9u8; 1 << 20]).unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let act = {
        let (segments, aside, target) = (
            store_dir.join("segments"),
            dir.path().join("segments-aside"),
            outside.path().to_path_buf(),
        );
        move || {
            std::fs::rename(&segments, &aside).unwrap();
            std::os::unix::fs::symlink(&target, &segments).unwrap();
        }
    };
    let _seam = super::failpoint::at(super::db::SEAM_SIZE_AFTER_THE_LISTING, nth_call(1, act));

    let size = store.size_bytes().map_err(|e| format!("{e:?}"));

    assert_eq!(
        size,
        Ok(regular_file_bytes(&store_dir)),
        "size_bytes with segments made a link after the listing"
    );
}

#[cfg(unix)]
#[test]
fn the_sweep_walks_no_directory_whose_name_is_no_shard_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let not_a_shard = store_dir.join("segments").join("0a1");
    std::fs::create_dir(&not_a_shard).unwrap();
    let file = not_a_shard.join(format!("0a1{}.seg", "3".repeat(29)));
    std::fs::write(&file, b"no file of the store").unwrap();
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();

    let swept = store
        .remove_keyless_files()
        .map(|s| (s.removed, s.not_store_files));

    assert!(
        file.exists(),
        "the sweep removed a file below a directory that is no shard ({swept:?})"
    );
    assert_eq!(swept.map_err(|e| format!("{e:?}")), Ok((0, 1)));
}

#[cfg(unix)]
#[test]
fn the_sweep_takes_no_segment_name_that_stands_in_another_shard() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let shard = shard_of(segment_id);
    let other = if shard == "cd" { "ef" } else { "cd" };
    let file = store_dir
        .join("segments")
        .join(&shard)
        .join(format!("{other}{}.seg", "4".repeat(30)));
    std::fs::write(&file, b"a segment name of another shard").unwrap();
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();

    let swept = store
        .remove_keyless_files()
        .map(|s| (s.removed, s.not_store_files));

    assert!(
        file.exists(),
        "the sweep removed a segment name that stands in another shard ({swept:?})"
    );
    assert_eq!(swept.map_err(|e| format!("{e:?}")), Ok((0, 1)));
}

#[cfg(windows)]
#[test]
fn windows_reads_what_sqlite_does_with_a_file_link_at_store_db_wal() {
    let say = |line: String| {
        let _ = writeln!(std::io::stderr(), "E5A {line}");
    };
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let target = outside.path().join("outside.bin");
    let pattern: Vec<u8> = (0..100u32).map(|i| (i * 7 % 251) as u8).collect();
    std::fs::write(&target, &pattern).unwrap();
    let linked = std::os::windows::fs::symlink_file(&target, store_dir.join("store.db-wal"));
    say(format!(
        "link made: {:?}",
        linked.as_ref().map(drop).map_err(|e| e.raw_os_error())
    ));
    if linked.is_err() {
        return;
    }
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
    say(format!("open: {}", answer_text(&opened)));
    if let Ok(mut store) = opened {
        let written = store
            .write_segment(&mut Cursor::new(b"through the log"))
            .map(drop);
        say(format!("write: {written:?}"));
        drop(store);
    }
    let after = std::fs::read(&target).unwrap_or_default();
    say(format!(
        "outside file: length before {} after {}, bytes equal {}",
        pattern.len(),
        after.len(),
        after == pattern
    ));
    say(format!("store directory after: {:?}", names_in(&store_dir)));
    say(format!(
        "outside directory after: {:?}",
        names_in(outside.path())
    ));
}

#[cfg(windows)]
#[test]
fn windows_open_refuses_a_file_link_or_a_junction_at_each_database_name() {
    let say = |line: String| {
        let _ = writeln!(std::io::stderr(), "E5B {line}");
    };
    for name in [
        "store.db",
        "store.db-wal",
        "store.db-shm",
        "store.db-journal",
    ] {
        for kind in ["file link", "junction"] {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
            let at = store_dir.join(name);
            if at.exists() {
                std::fs::rename(&at, dir.path().join("moved-aside")).unwrap();
            }
            let target = outside.path().join("target");
            let made = if kind == "file link" {
                std::fs::write(&target, b"bytes of another party").unwrap();
                std::os::windows::fs::symlink_file(&target, &at).map_err(|e| e.raw_os_error())
            } else {
                std::fs::create_dir(&target).unwrap();
                link_dir(&target, &at);
                Ok(())
            };
            if let Err(code) = made {
                say(format!(
                    "{kind} at {name}: this account may not make one ({code:?})"
                ));
                continue;
            }

            let answer = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));

            say(format!("{kind} at {name}: {answer}"));
            let expected = if name == "store.db" {
                "Corrupt(\"the database file of the store is a link\")"
            } else {
                "Corrupt(\"a file beside the database of the store is a link\")"
            };
            assert_eq!(answer, expected, "open with a {kind} at {name}");
            if kind == "file link" {
                assert_eq!(std::fs::read(&target).unwrap(), b"bytes of another party");
            } else {
                assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
            }
        }
    }
}
