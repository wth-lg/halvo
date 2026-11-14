use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use halvo_sqlcipher::rusqlite::Connection;
use uuid::Uuid;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::file_root::test_support::{FileRootCustody, TestProtector};
use crate::custody::file_root::{FileRoots, Protector};
use crate::custody::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

use super::db::test_support::pages;
use super::db::{EncryptedStore, KeyFile};
use super::roots::RootGuard;
use super::segment::tests::{BODY, CIPHER_CHUNK_LEN, HEADER, TAG_LEN};
use super::segment::{self, segment_path, CHUNK};
#[cfg(unix)]
const LOCK_FILES: [&str; 0] = [];
#[cfg(windows)]
const LOCK_FILES: [&str; 1] = ["writer.lock"];
#[cfg(windows)]
const NON_UNICODE_DIR_TEXTS: usize = 4;

use super::{
    Met, SegmentExpiry, SegmentStore, StoreError, StoreName, StoreResult, DEFAULT_RETENTION,
};

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
                inner: crate::store::roots::custody(root_id),
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
    fn roots(&self) -> Option<&std::path::Path> {
        None
    }

    fn wrap_store_key(
        &self,
        key: &StoreKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        self.inner
            .wrap_store_key(key, crate::custody::KeyLabel::STORE)
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.inner.create_root()
    }
    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        self.inner
            .unwrap_store_key(wrapped, crate::custody::KeyLabel::STORE)
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
    let root_id = RootId::fresh().unwrap();
    let _root_id_guard = RootGuard::new(root_id);
    drop(
        EncryptedStore::create(
            dir.path(),
            Box::new(crate::store::roots::custody(root_id)),
            DEFAULT_RETENTION,
        )
        .unwrap(),
    );

    let mut child = hold_store_in_child(dir.path());
    wait_for(&dir.path().join("child-ready"), &mut child);
    let _ = std::fs::remove_file(dir.path().join("writer.lock"));

    let second = crate::store::roots::open(dir.path(), DEFAULT_RETENTION);

    std::fs::write(dir.path().join("child-stop"), b"").unwrap();
    assert!(child.wait().unwrap().success(), "the holding child failed");
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
    assert!(
        matches!(second, Err(StoreError::Locked)),
        "a second writer was admitted after the lock file was unlinked: {:?}",
        second.err()
    );
}

#[test]
fn a_held_store_answers_locked_before_its_custody_is_called() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh().unwrap();
    let _root_id_guard = RootGuard::new(root_id);
    drop(
        EncryptedStore::create(
            dir.path(),
            Box::new(crate::store::roots::custody(root_id)),
            DEFAULT_RETENTION,
        )
        .unwrap(),
    );

    let mut child = hold_store_in_child(dir.path());
    wait_for(&dir.path().join("child-ready"), &mut child);
    let _ = std::fs::remove_file(dir.path().join("writer.lock"));
    let mut key_file = KeyFile::read(dir.path()).unwrap();
    key_file.root_id = RootId::fresh().unwrap();
    std::fs::write(
        KeyFile::path(dir.path()),
        serde_json::to_vec(&key_file).unwrap(),
    )
    .unwrap();
    let custody_calls = AtomicUsize::new(0);

    let second = EncryptedStore::open_with(
        dir.path(),
        &crate::store::roots::roots_for_store(dir.path()),
        DEFAULT_RETENTION,
        |locator: &crate::custody::CustodyLocator| {
            custody_calls.fetch_add(1, Ordering::SeqCst);
            crate::custody::custody_for(locator)
        },
    );

    std::fs::write(dir.path().join("child-stop"), b"").unwrap();
    assert!(child.wait().unwrap().success(), "the holding child failed");
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
    assert!(
        matches!(second, Err(StoreError::Locked)),
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
    let store = crate::store::roots::open(&dir, DEFAULT_RETENTION).unwrap();
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
        .env(
            crate::store::roots::ROOTS_OF_THE_PARENT,
            crate::store::roots::roots_for_store(dir),
        )
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
    let root_id = RootId::fresh().unwrap();
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
    let reopened = crate::store::roots::open(dir.path(), DEFAULT_RETENTION);
    assert!(
        matches!(
            reopened,
            Err(StoreError::DatabaseMissing | StoreError::Custody(CustodyError::RootMissing))
        ),
        "a backup of store.key reopened a deleted store: {:?}",
        reopened.err()
    );
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
}

#[test]
fn a_failed_create_leaves_no_root_and_no_half_store() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
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
    std::fs::create_dir_all(dir.path().join("store.db").join("held")).unwrap();
    let root_id = RootId::fresh().unwrap();
    let _root_id_guard = RootGuard::new(root_id);
    let (custody, _) = FlakyCustody::new(root_id, 0);

    let result = EncryptedStore::create(dir.path(), Box::new(custody), DEFAULT_RETENTION);
    assert!(result.is_err());
    assert!(
        !KeyFile::path(dir.path()).exists(),
        "a failed create left store.key"
    );
    assert!(
        !crate::store::roots::custody(root_id).root_exists().unwrap(),
        "a failed create left its root alive"
    );
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
}

#[test]
fn a_create_that_fails_after_wrapping_destroys_the_root_it_made() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh().unwrap();
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
        !crate::store::roots::custody(root_id).root_exists().unwrap(),
        "a failed create left its root alive"
    );
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
}

struct WrapThenFail(FlakyCustody);

impl KeyCustody for WrapThenFail {
    fn kind(&self) -> CustodyKind {
        self.0.kind()
    }
    fn root_id(&self) -> RootId {
        self.0.root_id()
    }
    fn roots(&self) -> Option<&std::path::Path> {
        None
    }

    fn wrap_store_key(
        &self,
        key: &StoreKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        self.0
            .wrap_store_key(key, crate::custody::KeyLabel::STORE)?;
        Err(CustodyError::Crypto(
            "wrap failed after the root was made".to_string(),
        ))
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.0.create_root()
    }
    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        self.0
            .unwrap_store_key(wrapped, crate::custody::KeyLabel::STORE)
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
    let root_id = RootId::fresh().unwrap();
    let _root_id_guard = RootGuard::new(root_id);
    let mut store = EncryptedStore::create(
        dir.path(),
        Box::new(crate::store::roots::custody(root_id)),
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

    let left: Vec<bool> = ["store.key", "store.db"]
        .iter()
        .map(|name| dir.path().join(name).exists())
        .collect();
    let reopened = crate::store::roots::open(dir.path(), DEFAULT_RETENTION).map(|store| {
        let read = store.read_segment(live.segment_id, &mut Vec::new());
        read.is_ok()
    });
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
    assert_eq!(
        left,
        [true, true],
        "the pass ({pass:?}) removed store.key or store.db through a link"
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
    for name in ["store.db", "store.db-wal", "store.key", "segments"] {
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
            RootId::fresh().unwrap(),
        )),
        DEFAULT_RETENTION,
    ) {
        Ok(store) => {
            drop(store);
            "Ok".to_string()
        }
        Err(StoreError::AlreadyExists) => "AlreadyExists".to_string(),
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
    assert!(segment.exists());
    let key_file_gone = answer_of_open_and_create(&store_dir, &roots_dir);
    let segment_left_after_create = segment.exists();

    assert_eq!(
        [key_file_whole, key_file_zeroed, key_file_gone],
        [
            ("DatabaseMissing".to_string(), "AlreadyExists".to_string()),
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

    let opened = EncryptedStore::open_with(
        &store_dir,
        &crate::store::roots::roots_for_store(&store_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            custody_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    );
    let beside_while_open = names(outside.path());
    let answer = match &opened {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?}"),
    };
    drop(opened);

    assert!(
        answer == "Obstructed { name: Database, met: Link }",
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
            answer, "Obstructed { name: KeyFile, met: Link }",
            "an open with a link to {behind_the_link} at store.key"
        );
        assert!(!answer.contains(&root_id.to_string()));
    }
}

#[cfg(unix)]
#[test]
fn open_answers_a_store_key_that_is_a_fifo_as_obstructed_within_a_bound() {
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
        Some("Obstructed { name: KeyFile, met: OtherKind }"),
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
            answer, "Obstructed { name: SegmentFile, met: Link }",
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
        Some("Obstructed { name: SegmentFile, met: OtherKind }"),
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

    let opened = EncryptedStore::open_with(
        &store_dir,
        &crate::store::roots::roots_for_store(&store_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            *linked.lock().unwrap() = Some(link_store_db_to(&store_dir, &target));
            Ok(Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    );
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
        answer, "Obstructed { name: Database, met: Link }",
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
        RootId::fresh().unwrap(),
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
    let other_root = RootId::fresh().unwrap();
    let _other_root_guard = RootGuard::new(other_root);
    drop(
        EncryptedStore::create(
            other.path(),
            Box::new(crate::store::roots::custody(other_root)),
            DEFAULT_RETENTION,
        )
        .unwrap(),
    );
    std::fs::copy(KeyFile::path(other.path()), KeyFile::path(dir.path())).unwrap();
    let opened = crate::store::roots::open(dir.path(), DEFAULT_RETENTION);
    crate::store::roots::custody(other_root)
        .destroy_root()
        .unwrap();
    assert!(
        matches!(opened, Err(StoreError::DatabaseUnreadable)),
        "a wrong store key opened as {:?}",
        opened.err()
    );
}

fn store_files_and_lengths(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut files: Vec<(PathBuf, u64)> = files_under(dir)
        .into_iter()
        .filter(|path| {
            !LOCK_FILES
                .iter()
                .any(|name| path.file_name() == Some(std::ffi::OsStr::new(name)))
        })
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
    EncryptedStore::open_with(
        store_dir,
        &crate::store::roots::roots_for_store(store_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            Ok(Box::new(file_root_custody(
                roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    )
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
        let before = store_files_and_lengths(&store_dir);
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
        let after = store_files_and_lengths(&store_dir);
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
    EncryptedStore::open_with(
        store_dir,
        &crate::store::roots::roots_for_store(store_dir),
        DEFAULT_RETENTION,
        |locator: &crate::custody::CustodyLocator| {
            let recorded = locator.root_id;
            Ok(Box::new(file_root_custody(
                roots_dir,
                TestProtector::new(0x5a).0,
                recorded,
            )))
        },
    )
}

fn version_answers(cases: &[(&str, DamageToTheDatabase)]) -> Vec<String> {
    cases
        .iter()
        .map(|(case, change)| {
            let dir = tempfile::tempdir().unwrap();
            let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
            change(&store_dir, &roots_dir, root_id);
            let before = store_files_and_lengths(&store_dir);
            let answer = match open_by_the_recorded_root(&store_dir, &roots_dir) {
                Ok(_) => "Ok".to_string(),
                Err(e) => format!("{e:?}"),
            };
            let changed = store_files_and_lengths(&store_dir) != before;
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
        ("store.key version 3", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["version"] = 3.into())
        }),
        ("store.key version 1", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["version"] = 1.into())
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
            "store.key version 999: NewerStore { found: 999, supported: 2 }",
            "store.key version 3: NewerStore { found: 3, supported: 2 }",
            "store.key version 1: OlderStore { found: 1, supported: 2 }",
            "store.key version 0: OlderStore { found: 0, supported: 2 }",
            "store.key version 999 and no other field: NewerStore { found: 999, supported: 2 }",
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
            !store_dir.join("writer.lock").exists(),
            "a refused open made a writer.lock"
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
                let other = file_root_custody(
                    roots_dir,
                    TestProtector::new(0x5a).0,
                    RootId::fresh().unwrap(),
                );
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
                    .wrap_store_key(
                        &StoreKey::random().unwrap(),
                        crate::custody::KeyLabel::STORE,
                    )
                    .unwrap();
                edit_key_file(store_dir, |v| {
                    v["wrapped_key"] = {
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
            edit_key_file(store_dir, |v| v["wrapped_key"] = "!!!!".into())
        }),
        ("a wrapped key of 8 bytes", |store_dir, _, _| {
            edit_key_file(store_dir, |v| v["wrapped_key"] = "AAAAAAAAAAA=".into())
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
            Err(StoreError::Locked) => "Locked".to_string(),
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
            "version below zero: OlderStore { found: -1, supported: 2 }",
            "version 999: NewerStore { found: 999, supported: 2 }",
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
                        .wrap_store_key(
                            &StoreKey::random().unwrap(),
                            crate::custody::KeyLabel::STORE,
                        )
                        .unwrap();
                    let mut key_file = KeyFile::read(store_dir).unwrap();
                    key_file.wrapped_key = STANDARD.encode(&another.0);
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
    let root_id = RootId::fresh().unwrap();
    let _root_id_guard = RootGuard::new(root_id);
    let store = EncryptedStore::create(
        dir.path(),
        Box::new(crate::store::roots::custody(root_id)),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let held_as_a_spawned_child_holds_it =
        super::db::test_support::duplicate_writer_lock_descriptor(&store);

    drop(store);
    let reopened = crate::store::roots::open(dir.path(), DEFAULT_RETENTION);

    drop(held_as_a_spawned_child_holds_it);
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
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
    let root_id = RootId::fresh().unwrap();
    let _root_id_guard = RootGuard::new(root_id);
    drop(
        EncryptedStore::create(
            dir.path(),
            Box::new(crate::store::roots::custody(root_id)),
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
        match crate::store::roots::open(dir.path(), DEFAULT_RETENTION) {
            Ok(store) => drop(store),
            Err(StoreError::Locked) => refused_as_locked += 1,
            Err(e) => other_errors.push(e),
        }
    }

    stop.store(true, Ordering::SeqCst);
    for spawner in spawners {
        spawner.join().unwrap();
    }
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
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
    let root_id = RootId::fresh().unwrap();
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
    fn roots(&self) -> Option<&std::path::Path> {
        None
    }

    fn wrap_store_key(
        &self,
        _key: &StoreKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        Err((self.error)())
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        Err((self.error)())
    }
    fn unwrap_store_key(
        &self,
        _wrapped: &WrappedKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        Err((self.error)())
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        Ok(())
    }
}

fn open_answer(error: fn() -> CustodyError) -> StoreError {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, _, root_id, _) = file_root_store(dir.path());
    EncryptedStore::open_with(
        &store_dir,
        &crate::store::roots::roots_for_store(&store_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| Ok(Box::new(FailingUnwrap { root_id, error })),
    )
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
    crate::store::roots::open(&store_dir, DEFAULT_RETENTION)
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
        let opened = crate::store::roots::open(&store_dir, DEFAULT_RETENTION);
        assert!(
            matches!(opened, Err(StoreError::KeyFileUnparseable)),
            "{case} key file: {:?}",
            opened.err()
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, _, _, _) = file_root_store(dir.path());
    let mut key_file = KeyFile::read(&store_dir).unwrap();
    key_file.wrapped_key = "!!!not base64!!!".to_string();
    std::fs::write(
        KeyFile::path(&store_dir),
        serde_json::to_vec(&key_file).unwrap(),
    )
    .unwrap();
    let opened = crate::store::roots::open(&store_dir, DEFAULT_RETENTION);
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
    let wrap = STANDARD.decode(&written.wrapped_key).unwrap();
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
        key_file.wrapped_key = STANDARD.encode(&field);
        std::fs::write(
            KeyFile::path(&store_dir),
            serde_json::to_vec(&key_file).unwrap(),
        )
        .unwrap();
        let opened = EncryptedStore::open_with(
            &store_dir,
            &crate::store::roots::roots_for_store(&store_dir),
            DEFAULT_RETENTION,
            |_: &crate::custody::CustodyLocator| {
                Ok(Box::new(file_root_custody(
                    &roots_dir,
                    TestProtector::new(0x5a).0,
                    root_id,
                )))
            },
        );
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
        EncryptedStore::open_with(
            &store_dir,
            &crate::store::roots::roots_for_store(&store_dir),
            DEFAULT_RETENTION,
            |_: &crate::custody::CustodyLocator| {
                Ok(Box::new(file_root_custody(
                    &roots_dir,
                    TestProtector::new(0x5a).0,
                    root_id,
                )))
            },
        )
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
    std::fs::create_dir_all(failed_dir.join("store.db-shm").join("held")).unwrap();
    std::fs::write(failed_dir.join("store.db-wal"), log).unwrap();
    let created = EncryptedStore::create(
        &failed_dir,
        Box::new(file_root_custody(
            &dir.path().join("roots-of-the-failed-create"),
            TestProtector::new(0x5a).0,
            RootId::fresh().unwrap(),
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
        let removal = KeyFile::at(&KeyFile::path(&store_dir)).unwrap().destroy();
        assert!(removal.gone);
        if !keep_the_database {
            let database =
                super::db::Db::destroy_through(&super::durable::Dir::open(&store_dir).unwrap());
            assert!(!database.left);
        }
        let created = EncryptedStore::create(
            &store_dir,
            Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                RootId::fresh().unwrap(),
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
    let mut open_lock = || {
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
    };
    let custody = file_root_custody(
        &roots_dir,
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );

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
            .chain(LOCK_FILES.map(|name| (name.to_string(), Vec::new())))
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
                matches!(
                    &created,
                    Err(StoreError::Obstructed {
                        name: StoreName::StoreDirectory,
                        met: Met::DirectoryNotAtItsName
                    })
                ),
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
        RootId::fresh().unwrap(),
    );

    let created =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).map(drop);

    assert!(
        matches!(created, Err(StoreError::AlreadyExists)),
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
    EncryptedStore::open_with(
        store_dir,
        &crate::store::roots::roots_for_store(store_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            Ok(Box::new(file_root_custody(
                roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    )
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
        matches!(
            read,
            Err(StoreError::Obstructed {
                name: StoreName::SegmentFile,
                met: Met::OtherKind
            })
        ),
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
    assert!(matches!(read, Err(StoreError::SegmentMissing)), "{read:?}");
}

fn open_over_protector(
    store_dir: &Path,
    roots_dir: &Path,
    root_id: RootId,
    protector: &Arc<TestProtector>,
) -> StoreResult<EncryptedStore> {
    EncryptedStore::open_with(
        store_dir,
        &crate::store::roots::roots_for_store(store_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            Ok(Box::new(file_root_custody(
                roots_dir,
                Arc::clone(protector),
                root_id,
            )))
        },
    )
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
    fn roots(&self) -> Option<&std::path::Path> {
        None
    }

    fn wrap_store_key(
        &self,
        key: &StoreKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        self.inner
            .wrap_store_key(key, crate::custody::KeyLabel::STORE)
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.inner.create_root()?;
        std::fs::create_dir(&self.key_file).unwrap();
        Ok(())
    }
    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        self.inner
            .unwrap_store_key(wrapped, crate::custody::KeyLabel::STORE)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.inner.destroy_root()
    }
}

#[test]
fn a_create_whose_key_file_can_be_neither_written_nor_removed_still_destroys_its_root() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
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
    let root_id = RootId::fresh().unwrap();
    let _root_id_guard = RootGuard::new(root_id);
    let store = EncryptedStore::create(
        dir.path(),
        Box::new(crate::store::roots::custody(root_id)),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (before, at_free) = key_bytes_before_and_when_freed(store, |store| {
        store.store_key().unwrap().expose().as_ptr()
    });
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
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
    let root_id = RootId::fresh().unwrap();
    let _root_id_guard = RootGuard::new(root_id);
    let mut store = EncryptedStore::create(
        dir.path(),
        Box::new(crate::store::roots::custody(root_id)),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let store_key = zeroize::Zeroizing::new(*store.store_key().unwrap().expose());
    store
        .write_segment(&mut Cursor::new(b"a segment beside the key"))
        .unwrap();
    drop(store);
    let at_rest = super::verify::raw_key_at_rest(dir.path(), &store_key);
    crate::store::roots::custody(root_id)
        .destroy_root()
        .unwrap();
    assert!(
        matches!(at_rest, Ok(false)),
        "the raw store key is at rest in the store directory: {at_rest:?}"
    );
}

#[test]
fn the_store_key_file_holds_its_five_fields_and_no_other() {
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
            "namespace",
            "root_id",
            "version",
            "wrapped_key"
        ]
    );
    assert_eq!(json["version"], 2);
    assert_eq!(json["custody_kind"], "dev_fallback");
    assert_eq!(json["namespace"], "store");
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
            RootId::fresh().unwrap(),
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
    let reopened = EncryptedStore::open_with(
        &first_dir,
        &crate::store::roots::roots_for_store(&first_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| Ok(custody()),
    );
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
        let root_id = RootId::fresh().unwrap();
        let _root_id_guard = RootGuard::new(root_id);
        a_second_create_with_one_root_id_fails_and_leaves_the_first_store_readable(&|| {
            Box::new(crate::store::roots::custody(root_id))
        });
    }
    let dir = tempfile::tempdir().unwrap();
    let roots_dir = dir.path().join("roots");
    let root_id = RootId::fresh().unwrap();
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
    let root_id = RootId::fresh().unwrap();
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
    let reopened = EncryptedStore::open_with(
        &first_dir,
        &crate::store::roots::roots_for_store(&first_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            Ok(Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    );
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
    if second_names != LOCK_FILES {
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
    fn roots(&self) -> Option<&std::path::Path> {
        None
    }

    fn wrap_store_key(
        &self,
        _key: &StoreKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        Err((self.answer)())
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        Err((self.answer)())
    }
    fn unwrap_store_key(
        &self,
        _wrapped: &WrappedKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
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
            root_id: RootId::fresh().unwrap(),
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
        if names != LOCK_FILES {
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
        let root_id = RootId::fresh().unwrap();
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
            let reopened = EncryptedStore::open_with(
                &store_dir,
                &crate::store::roots::roots_for_store(&store_dir),
                DEFAULT_RETENTION,
                |_: &crate::custody::CustodyLocator| {
                    Ok(Box::new(file_root_custody(
                        &roots_dir,
                        TestProtector::new(0x5a).0,
                        root_id,
                    )))
                },
            );
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
        let root_id = RootId::fresh().unwrap();
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        let mut store =
            EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
        store
            .write_segment(&mut Cursor::new(b"crash image"))
            .unwrap();
        let crashed = dir.join("crashed");
        std::fs::create_dir(&crashed).unwrap();
        for name in ["store.key", "store.db", "store.db-wal"] {
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
        store_files_and_lengths(&crashed)
    );
}

#[test]
fn a_refused_open_after_a_crash_changes_no_file_of_the_store() {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    fn crash_image(dir: &Path) -> (PathBuf, PathBuf, RootId) {
        let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
        let root_id = RootId::fresh().unwrap();
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        let mut store =
            EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
        store
            .write_segment(&mut Cursor::new(b"crash image"))
            .unwrap();
        let crashed = dir.join("crashed");
        std::fs::create_dir(&crashed).unwrap();
        for name in ["store.key", "store.db", "store.db-wal"] {
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
        .wrap_store_key(
            &StoreKey::random().unwrap(),
            crate::custody::KeyLabel::STORE,
        )
        .unwrap();
    let mut key_file = KeyFile::read(&crashed).unwrap();
    key_file.wrapped_key = STANDARD.encode(&another.0);
    std::fs::write(
        KeyFile::path(&crashed),
        serde_json::to_vec(&key_file).unwrap(),
    )
    .unwrap();

    let before = store_files_and_lengths(&crashed);
    let db_before = std::fs::read(crashed.join("store.db")).unwrap();
    let wal_before = std::fs::read(crashed.join("store.db-wal")).unwrap();
    let answer = open_over_file_root(&crashed, &roots_dir, root_id)
        .map(drop)
        .map_err(|e| format!("{e:?}"));
    let after = store_files_and_lengths(&crashed);
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
        let root_id = RootId::fresh().unwrap();
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        let mut store =
            EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
        store
            .write_segment(&mut Cursor::new(b"crash image"))
            .unwrap();
        let crashed = dir.join("crashed");
        std::fs::create_dir(&crashed).unwrap();
        for name in ["store.key", "store.db", "store.db-wal"] {
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
    let before = store_files_and_lengths(&crashed);
    let answer = open_over_file_root(&crashed, &roots_dir, root_id)
        .map(drop)
        .map_err(|e| format!("{e:?}"));
    let after = store_files_and_lengths(&crashed);
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
    let root_id = RootId::fresh().unwrap();
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
    let root_id = RootId::fresh().unwrap();
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
                    matches!(
                        &written,
                        Err(StoreError::Obstructed {
                            name: StoreName::Shard,
                            met: Met::DirectoryNotAtItsName
                        })
                    ),
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
    let root_id = RootId::fresh().unwrap();
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
    let custody = file_root_custody(
        &roots_dir,
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
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
    let store = EncryptedStore::open_with(
        &store_dir,
        &crate::store::roots::roots_for_store(&store_dir),
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            std::thread::sleep(SLOW_REBUILD);
            Ok(Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    )
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
        matches!(
            &shred,
            Err(StoreError::Obstructed {
                name: StoreName::Log,
                met: Met::Link
            })
        ),
        "a shred with a link at store.db-wal answered {shred:?}"
    );
    assert_eq!(std::fs::read(&target).unwrap(), vec![7u8; 100]);
}

#[cfg(unix)]
fn a_write_store(dir: &Path) -> (PathBuf, EncryptedStore) {
    let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
    let custody = file_root_custody(
        &roots_dir,
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
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
        matches!(
            &written,
            Err(StoreError::Obstructed {
                name: StoreName::SegmentFile,
                met: Met::Link
            })
        ),
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
        matches!(
            &written,
            Err(StoreError::Obstructed {
                name: StoreName::Shard,
                met: Met::DirectoryNotAtItsName
            })
        ),
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
        nth_call(if let CallSite::Probe = site { 1 } else { 2 }, act),
    );

    let answer = match site {
        CallSite::Probe | CallSite::Existing => {
            answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id))
        }
        CallSite::New => {
            let custody = file_root_custody(
                &dir.path().join("roots-new"),
                TestProtector::new(0x5a).0,
                RootId::fresh().unwrap(),
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
                answer == "Obstructed { name: Database, met: Link }",
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
                            RootId::fresh().unwrap(),
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
    let store_name = |name: &str| match name {
        "store.db" => "Database",
        "store.db-wal" => "Log",
        "store.db-shm" => "SharedMemory",
        _ => "Journal",
    };
    let mut expected = Vec::new();
    for name in [
        "store.db",
        "store.db-wal",
        "store.db-shm",
        "store.db-journal",
    ] {
        for what in ["Link", "Directory", "Fifo"] {
            let met = if what == "Link" { "Link" } else { "OtherKind" };
            let open = format!("Obstructed {{ name: {}, met: {met} }}", store_name(name));
            expected.push(format!("open {what} at {name}: {open}"));
            expected.push(format!("create {what} at {name}: Ok"));
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
        Some("Obstructed { name: StoreDirectory, met: DirectoryNotAtItsName }"),
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
        Some("Obstructed { name: Database, met: OtherKind }"),
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
        Some("Obstructed { name: Journal, met: OtherKind }"),
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
        (Some("Obstructed { name: Database, met: Link }"), 0o644),
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
fn a_store_db_that_becomes_a_fifo_before_the_mode_step_answers_obstructed_within_a_bound() {
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
        Some("Obstructed { name: Database, met: OtherKind }"),
        "within {CALL_BOUND:?}"
    );
}

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
    let mut open_lock = || {
        if !linked {
            linked = true;
            std::fs::rename(&store_dir, &moved).unwrap();
            std::os::unix::fs::symlink(&moved, &store_dir).unwrap();
        }
    };
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
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
        matches!(
            &created,
            Err(StoreError::Obstructed {
                name: StoreName::StoreDirectory,
                met: Met::DirectoryNotAtItsName
            })
        ),
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
        RootId::fresh().unwrap(),
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
            let expected = match name {
                "store.db" => "Obstructed { name: Database, met: Link }",
                "store.db-wal" => "Obstructed { name: Log, met: Link }",
                "store.db-shm" => "Obstructed { name: SharedMemory, met: Link }",
                _ => "Obstructed { name: Journal, met: Link }",
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

fn delete_report(answer: &StoreResult<super::DeleteReport>) -> super::DeleteReport {
    match answer {
        Ok(report) => report.clone(),
        Err(StoreError::DeleteIncomplete(report)) => (**report).clone(),
        Err(e) => panic!("delete-all answered {e:?}, no report"),
    }
}

fn root_files_in(roots_dir: &Path) -> Vec<String> {
    match std::fs::read_dir(roots_dir) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.contains(".root"))
                .collect();
            names.sort();
            names
        }
        Err(_) => Vec::new(),
    }
}

fn store_files_in(dir: &Path) -> Vec<String> {
    files_with_bytes(dir)
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| {
            name == "store.key"
                || name.starts_with("store.db")
                || name.starts_with("segments")
                || name == "create.pending"
        })
        .collect()
}

#[cfg(unix)]
fn a_directory_of_another_party(at: &Path) -> Vec<(String, Vec<u8>)> {
    std::fs::create_dir_all(at.join("segments").join("ab")).unwrap();
    std::fs::write(at.join("store.key"), b"a key file of another party").unwrap();
    std::fs::write(at.join("store.db"), b"a database of another party").unwrap();
    std::fs::write(at.join("store.db-wal"), b"a log of another party").unwrap();
    std::fs::write(
        at.join("segments").join("ab").join("notes.txt"),
        b"a file of another name",
    )
    .unwrap();
    files_with_bytes(at)
}

#[cfg(unix)]
#[test]
fn delete_all_acts_through_its_handle_and_leaves_a_directory_put_at_the_store_name_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store.write_segment(&mut Cursor::new(b"moved")).unwrap();
    let moved = dir.path().join("moved-away");
    std::fs::rename(&store_dir, &moved).unwrap();
    std::fs::create_dir(&store_dir).unwrap();
    let theirs = a_directory_of_another_party(&store_dir);

    let report = delete_report(&store.delete_all());
    drop(store);

    assert_eq!(
        files_with_bytes(&store_dir),
        theirs,
        "delete-all changed the directory put at the store's name"
    );
    assert_eq!(
        store_files_in(&moved),
        Vec::<String>::new(),
        "delete-all left files of the store in the moved directory: {report:?}"
    );
    assert_eq!(report.not_followed, 1, "{report:?}");
    assert!(report.root_destroyed(), "{report:?}");
    assert_eq!(
        root_files_in(&roots_dir),
        Vec::<String>::new(),
        "the roots directory on disk still holds a root file: {report:?}"
    );
}

#[cfg(unix)]
#[test]
fn delete_all_opens_lists_and_changes_nothing_behind_a_link_put_at_the_store_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let moved = dir.path().join("moved-away");
    std::fs::rename(&store_dir, &moved).unwrap();
    let behind = dir.path().join("behind-the-link");
    std::fs::create_dir(&behind).unwrap();
    let theirs = a_directory_of_another_party(&behind);
    std::os::unix::fs::symlink(&behind, &store_dir).unwrap();
    let modes_before = modes_under(&behind);

    let report = delete_report(&store.delete_all());
    drop(store);

    assert_eq!(files_with_bytes(&behind), theirs);
    assert_eq!(modes_under(&behind), modes_before);
    assert!(std::fs::symlink_metadata(&store_dir)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(store_files_in(&moved), Vec::<String>::new(), "{report:?}");
    assert_eq!(report.not_followed, 1, "{report:?}");
    assert_eq!(
        root_files_in(&roots_dir),
        Vec::<String>::new(),
        "the roots directory on disk still holds a root file: {report:?}"
    );
}

#[cfg(unix)]
#[test]
fn the_delete_tail_removes_a_link_at_the_store_name_and_then_sweeps_every_root_file_under_roots() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let roots_dir = data.join("roots");
    let behind = dir.path().join("elsewhere");
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let mut store = EncryptedStore::create(&behind, Box::new(custody), DEFAULT_RETENTION).unwrap();
    store.write_segment(&mut Cursor::new(b"behind")).unwrap();
    drop(store);
    let second = RootId::fresh().unwrap();
    file_root_custody(&roots_dir, TestProtector::new(0x5a).0, second)
        .create_root()
        .unwrap();
    std::os::unix::fs::symlink(&behind, data.join("store")).unwrap();
    let behind_before = files_with_bytes(&behind);

    let answer = EncryptedStore::delete_store_dir(&data.join("store"), &roots_dir);
    let report = delete_report(&answer);

    assert_eq!(
        root_files_in(&roots_dir),
        Vec::<String>::new(),
        "the tail that removed a link at the store name left a root file under roots (ruling ST-45 H1)"
    );
    assert!(std::fs::symlink_metadata(data.join("store")).is_err());
    assert_eq!(files_with_bytes(&behind), behind_before);
    assert_eq!(report.root, super::RootReading::Unnamed, "{report:?}");
    assert!(answer.is_ok(), "{report:?}");
    assert_eq!(report.not_followed, 1, "{report:?}");
}

#[cfg(unix)]
#[test]
fn delete_all_destroys_every_root_file_under_its_roots_directory_and_names_one_that_stays() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let second = RootId::fresh().unwrap();
    file_root_custody(&roots_dir, TestProtector::new(0x5a).0, second)
        .create_root()
        .unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let report = delete_report(&store.delete_all());
    assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
    assert!(report.root_destroyed(), "{report:?}");
    drop(store);

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let kept = RootId::fresh().unwrap();
    file_root_custody(&roots_dir, TestProtector::new(0x5a).0, kept)
        .create_root()
        .unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let answer = store.delete_all();
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let report = delete_report(&answer);
    assert!(answer.is_err(), "a delete that left roots answered Ok");
    assert!(!report.root_destroyed(), "{report:?}");
    let mut expected = vec![root_id, kept];
    expected.sort_by_key(|id| id.to_string());
    assert_eq!(report.roots.left, expected, "{report:?}");
    assert!(
        store.write_segment(&mut Cursor::new(b"after")).is_err(),
        "a write after the root step was taken"
    );
    let again = store.delete_all();
    assert!(again.is_ok(), "the second call did not finish: {again:?}");
    assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn a_socket_a_directory_and_a_fifo_at_store_key_answer_obstructed_without_a_wait() {
    let mut answers = Vec::new();
    for what in ["socket", "directory", "fifo"] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        let key = KeyFile::path(&store_dir);
        std::fs::remove_file(&key).unwrap();
        let _listener = match what {
            "socket" => Some(std::os::unix::net::UnixListener::bind(&key).unwrap()),
            "directory" => {
                std::fs::create_dir(&key).unwrap();
                None
            }
            _ => {
                make_fifo(&key);
                None
            }
        };
        let answer = within(CALL_BOUND, move || {
            answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id))
        });
        answers.push(format!("{what}: {answer:?}"));
    }
    assert_eq!(
        answers,
        [
            "socket: Some(\"Obstructed { name: KeyFile, met: OtherKind }\")",
            "directory: Some(\"Obstructed { name: KeyFile, met: OtherKind }\")",
            "fifo: Some(\"Obstructed { name: KeyFile, met: OtherKind }\")",
        ]
    );
}

#[cfg(unix)]
#[test]
fn delete_all_answers_a_fifo_at_store_key_without_a_wait_and_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let key = KeyFile::path(&store_dir);
    std::fs::remove_file(&key).unwrap();
    make_fifo(&key);
    let answer = within(CALL_BOUND, move || {
        let mut store = store;
        delete_report(&store.delete_all())
    });
    let report = answer.expect("delete-all waited on a FIFO at store.key");
    assert!(report.key_file_gone, "{report:?}");
    assert_eq!(report.not_followed, 1, "{report:?}");
    assert!(std::fs::symlink_metadata(&key).is_err());
}

#[cfg(unix)]
#[test]
fn create_answers_obstructed_when_sqlite_holds_a_database_outside_its_handle_and_clears_the_handle()
{
    use super::db::SEAM_AFTER_THE_LAST_LOOK;
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    let decoy_source = tempfile::tempdir().unwrap();
    let (decoy_store, _, _, _) = file_root_store(decoy_source.path());
    let moved = dir.path().join("moved-away");
    force_kept_name(true);
    let act = {
        let (store_dir, moved, decoy_db) = (
            store_dir.clone(),
            moved.clone(),
            decoy_store.join("store.db"),
        );
        move || {
            std::fs::rename(&store_dir, &moved).unwrap();
            std::fs::create_dir(&store_dir).unwrap();
            std::fs::copy(&decoy_db, store_dir.join("store.db")).unwrap();
        }
    };
    let _seam = super::failpoint::at(SEAM_AFTER_THE_LAST_LOOK, nth_call(1, act));
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
    force_kept_name(false);
    let answer = answer_text(&created);
    drop(created);
    let at_the_name = names_in_store_dir(&store_dir);
    say_branch(
        "create on a split store",
        &format!("the directory at the name holds {at_the_name:?}"),
    );
    assert!(
        answer.starts_with("Obstructed {"),
        "create on a database outside its handle answered {answer}"
    );
    assert_eq!(
        store_files_in(&moved),
        Vec::<String>::new(),
        "create left files of its own in its handle's directory"
    );
}

fn holds_no_marker(text: &str, markers: &[&str]) -> bool {
    markers.iter().all(|marker| !text.contains(marker))
}

#[test]
fn no_text_of_an_answer_holds_the_directory_marker_or_a_segment_id() {
    const MARKER: &str = "HALVO-PATH-MARKER-7f3a";
    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join(format!("parent-{MARKER}"));
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(&parent);
    let id_simple = segment_id.simple().to_string();
    let id_hyphen = segment_id.to_string();
    let markers = [MARKER, id_simple.as_str(), id_hyphen.as_str()];
    let mut texts: Vec<String> = Vec::new();
    let mut take = |label: &str, e: &StoreError| {
        texts.push(format!("{label}: {e}"));
        texts.push(format!("{label}: {e:?}"));
    };

    let held = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    if let Err(e) = open_over_file_root(&store_dir, &roots_dir, root_id) {
        take("open of a held store", &e);
    }
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    if let Err(e) = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION) {
        take("create over a held store", &e);
    }
    drop(held);
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    if let Err(e) = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION) {
        take("create over a store", &e);
    }
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    std::fs::remove_file(segment_path(&store_dir, segment_id)).unwrap();
    if let Err(e) = store.read_segment(segment_id, &mut Vec::new()) {
        take("read of a segment with no file", &e);
    }
    if let Err(e) = store.crypto_shred_expired() {
        take("shred", &e);
    }
    let link_target = dir.path().join(format!("target-{MARKER}"));
    std::fs::write(&link_target, b"x").unwrap();
    #[cfg(unix)]
    {
        let segments = store_dir.join("segments");
        let aside = dir.path().join(format!("segments-aside-{MARKER}"));
        std::fs::rename(&segments, &aside).unwrap();
        std::os::unix::fs::symlink(&aside, &segments).unwrap();
        if let Err(e) = store.write_segment(&mut Cursor::new(b"through a link")) {
            take("write through a linked segments directory", &e);
        }
        std::fs::remove_file(&segments).unwrap();
        std::fs::rename(&aside, &segments).unwrap();
    }
    let ro = std::fs::metadata(&roots_dir).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    }
    if let Err(e) = store.delete_all() {
        take("delete-all that leaves a root", &e);
    }
    std::fs::set_permissions(&roots_dir, ro).unwrap();
    if let Err(e) = store.write_segment(&mut Cursor::new(b"after delete-all")) {
        take("write after delete-all", &e);
    }
    drop(store);
    if let Err(e) = open_over_file_root(&store_dir, &roots_dir, root_id) {
        take("open after delete-all", &e);
    }
    if let Err(e) = open_over_file_root(
        &dir.path().join(format!("absent-{MARKER}")),
        &roots_dir,
        root_id,
    ) {
        take("open of a missing directory", &e);
    }
    #[cfg(unix)]
    {
        let linked = dir.path().join(format!("linked-{MARKER}"));
        std::os::unix::fs::symlink(&store_dir, &linked).unwrap();
        if let Err(e) = open_over_file_root(&linked, &roots_dir, root_id) {
            take("open of a linked directory", &e);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        let mut wide: Vec<u16> = "not-unicode-".encode_utf16().collect();
        wide.push(0xD800);
        let odd = dir.path().join(std::ffi::OsString::from_wide(&wide));
        let custody = file_root_custody(
            &dir.path().join("roots-odd"),
            TestProtector::new(0x5a).0,
            RootId::fresh().unwrap(),
        );
        match EncryptedStore::create(&odd, Box::new(custody), DEFAULT_RETENTION) {
            Ok(store) => {
                if let Err(e) = store.read_segment(segment_id, &mut Vec::new()) {
                    take("read in a directory whose name is not Unicode", &e);
                }
            }
            Err(e) => take("create in a directory whose name is not Unicode", &e),
        }
        if let Err(e) = open_over_file_root(&odd, &roots_dir, root_id) {
            take("open in a directory whose name is not Unicode", &e);
        }
        texts.push(format!("odd name: {}", odd.display()));
    }
    let leaking: Vec<&String> = texts
        .iter()
        .filter(|text| !text.starts_with("odd name: ") && !holds_no_marker(text, &markers))
        .collect();
    println!("TEXTS {texts:#?}");
    #[cfg(windows)]
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.contains("whose name is not Unicode: "))
            .count(),
        NON_UNICODE_DIR_TEXTS,
        "the answers in the directory whose name is not Unicode were not read: {texts:#?}"
    );
    let fewest = if cfg!(windows) { 19 } else { 20 };
    assert!(
        texts.len() >= fewest,
        "too few answers were read: {texts:#?}"
    );
    assert!(
        leaking.is_empty(),
        "texts that hold a path or an id: {leaking:#?}"
    );
}

#[cfg(target_os = "macos")]
fn database_list_name(store: &EncryptedStore) -> String {
    store
        .db()
        .unwrap()
        .connection()
        .query_row("PRAGMA database_list", [], |r| r.get::<_, String>(2))
        .unwrap()
}

#[cfg(target_os = "macos")]
fn volume_name_of(dir: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(dir).unwrap();
    format!("/.vol/{}/{}/store.db", meta.dev(), meta.ino())
}

#[cfg(target_os = "macos")]
#[test]
fn sqlite_keeps_the_volume_name_of_the_handle_for_an_opened_and_a_created_store() {
    use super::db::NameForm;
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let root_id = custody.root_id;
    let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
    let created_name = database_list_name(&created);
    let created_form = created.db().unwrap().name_form();
    drop(created);
    let opened = open_over_file_root(&store_dir, &dir.path().join("roots"), root_id).unwrap();
    let opened_name = database_list_name(&opened);
    let opened_form = opened.db().unwrap().name_form();
    let expected = volume_name_of(&store_dir);
    println!("NAMES created={created_name} opened={opened_name} expected={expected}");
    assert_eq!(created_name, expected);
    assert_eq!(opened_name, expected);
    assert_eq!(created_form, NameForm::Volume);
    assert_eq!(opened_form, NameForm::Volume);
}

#[cfg(target_os = "macos")]
fn a_decoy_at(at: &Path, decoy_source: &Path) -> Vec<(String, Vec<u8>)> {
    std::fs::create_dir(at).unwrap();
    for name in ["store.key", "store.db"] {
        std::fs::copy(decoy_source.join(name), at.join(name)).unwrap();
    }
    std::fs::write(at.join("store.db-wal"), b"").unwrap();
    files_with_bytes(at)
}

#[cfg(target_os = "macos")]
#[test]
fn after_a_move_once_open_a_write_a_checkpoint_and_the_close_act_in_the_moved_directory() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let decoy_source = tempfile::tempdir().unwrap();
    let (decoy_store, _, _, _) = file_root_store(decoy_source.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let moved = dir.path().join("moved-away");
    std::fs::rename(&store_dir, &moved).unwrap();
    let theirs = a_decoy_at(&store_dir, &decoy_store);

    let (written, _) = store
        .write_segment(&mut Cursor::new(b"after the move"))
        .unwrap();
    store.db().unwrap().checkpoint_truncate().unwrap();
    drop(store);

    assert_eq!(files_with_bytes(&store_dir), theirs);
    let reopened = open_over_file_root(&moved, &roots_dir, root_id).unwrap();
    let mut plain = Vec::new();
    reopened
        .read_segment(written.segment_id, &mut plain)
        .unwrap();
    assert_eq!(plain, b"after the move");
}

#[cfg(target_os = "macos")]
#[test]
fn a_move_before_the_first_read_refuses_the_open_and_changes_nothing_at_the_old_name() {
    use super::db::SEAM_BEFORE_THE_FIRST_READ;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let decoy_source = tempfile::tempdir().unwrap();
    let (decoy_store, _, _, _) = file_root_store(decoy_source.path());
    let moved = dir.path().join("moved-away");
    let theirs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let act = {
        let (store_dir, moved, decoy, theirs) = (
            store_dir.clone(),
            moved.clone(),
            decoy_store.clone(),
            Arc::clone(&theirs),
        );
        move || {
            std::fs::rename(&store_dir, &moved).unwrap();
            *theirs.lock().unwrap() = a_decoy_at(&store_dir, &decoy);
        }
    };
    let seam = super::failpoint::at(SEAM_BEFORE_THE_FIRST_READ, nth_call(1, act));
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
    drop(seam);
    let answer = answer_text(&opened);
    drop(opened);
    assert_eq!(
        answer,
        "Obstructed { name: StoreDirectory, met: DirectoryNotAtItsName }"
    );
    assert_eq!(files_with_bytes(&store_dir), *theirs.lock().unwrap());
    let reopened = open_over_file_root(&moved, &roots_dir, root_id).unwrap();
    reopened.read_segment(segment_id, &mut Vec::new()).unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn what_the_first_read_of_a_database_with_no_pages_removes_lies_in_the_moved_directory() {
    use super::db::{Db, SEAM_AFTER_THE_LAST_LOOK};
    use super::durable::Dir;
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    std::fs::write(store_dir.join("store.db-journal"), vec![b'R'; 600]).unwrap();
    std::fs::write(store_dir.join("store.db-wal"), vec![b'R'; 600]).unwrap();
    let moved = dir.path().join("moved-away");
    let act = {
        let (store_dir, moved) = (store_dir.clone(), moved.clone());
        move || {
            std::fs::rename(&store_dir, &moved).unwrap();
            std::fs::create_dir(&store_dir).unwrap();
            std::fs::write(store_dir.join("store.db"), b"").unwrap();
            std::fs::write(store_dir.join("store.db-journal"), vec![b'X'; 1000]).unwrap();
            std::fs::write(store_dir.join("store.db-wal"), vec![b'X'; 1000]).unwrap();
        }
    };
    let handle = Dir::open(&store_dir).unwrap();
    let seam = super::failpoint::at(SEAM_AFTER_THE_LAST_LOOK, nth_call(1, act));
    let opened = Db::open(&handle, &[0x44; 32]).map(drop);
    drop(seam);
    println!(
        "NOPAGES answer={opened:?} old_name={:?} moved={:?}",
        store_files_and_lengths(&store_dir),
        store_files_and_lengths(&moved)
    );
    assert_eq!(
        std::fs::read(store_dir.join("store.db-journal")).unwrap(),
        vec![b'X'; 1000]
    );
    assert_eq!(
        std::fs::read(store_dir.join("store.db-wal")).unwrap(),
        vec![b'X'; 1000]
    );
    assert_eq!(std::fs::read(store_dir.join("store.db")).unwrap(), b"");
    assert!(
        std::fs::read(moved.join("store.db-journal")).map_or(true, |b| b != vec![b'R'; 600]),
        "the journal of the handle's database with no pages stayed as it was"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_volume_name_that_leads_to_another_directory_is_refused_and_a_create_leaves_nothing() {
    use super::db::name_seams;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let other_source = tempfile::tempdir().unwrap();
    let (other_store, _, _, _) = file_root_store(other_source.path());
    let other_before = files_with_bytes(&other_store);
    name_seams::give_name(Some(PathBuf::from(volume_name_of(&other_store))));
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    let fresh = dir.path().join("fresh");
    let custody = file_root_custody(
        &dir.path().join("roots-fresh"),
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let created = EncryptedStore::create(&fresh, Box::new(custody), DEFAULT_RETENTION);
    name_seams::give_name(None);
    let created = answer_text(&created);
    assert_eq!(
        opened,
        "Obstructed { name: Database, met: DatabaseOutsideTheDirectory }"
    );
    assert!(
        created.starts_with("Obstructed {"),
        "create answered {created}"
    );
    assert_eq!(store_files_in(&fresh), Vec::<String>::new());
    assert_eq!(files_with_bytes(&other_store), other_before);
}

#[cfg(target_os = "macos")]
#[test]
fn the_stat_of_the_volume_name_chooses_the_kept_name_or_refuses() {
    use super::db::{name_seams, NameForm};
    use super::durable::EntryKind;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    force_kept_name(true);
    let kept = open_over_file_root(&store_dir, &roots_dir, root_id);
    force_kept_name(false);
    let kept = kept.unwrap();
    assert_eq!(kept.db().unwrap().name_form(), NameForm::Kept);
    assert_eq!(
        database_list_name(&kept),
        std::fs::canonicalize(&store_dir)
            .unwrap()
            .join("store.db")
            .to_string_lossy()
    );
    drop(kept);
    name_seams::answer_stat(Some(Box::new(|| Ok((EntryKind::Dir, 1, 2)))));
    let other = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    name_seams::answer_stat(Some(Box::new(|| {
        Err(io::Error::from_raw_os_error(libc::EIO))
    })));
    let failed = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    force_kept_name(false);
    assert_eq!(
        other,
        "Obstructed { name: StoreDirectory, met: DirectoryNotAtItsName }"
    );
    assert_eq!(failed, "Io(InputOutputError)");
}

#[cfg(target_os = "macos")]
#[test]
fn a_store_directory_removed_while_open_answers_obstructed_at_the_next_read_and_makes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let held = dir.path().join("held");
    std::fs::create_dir(&held).unwrap();
    for entry in std::fs::read_dir(&store_dir).unwrap() {
        let entry = entry.unwrap();
        std::fs::rename(entry.path(), held.join(entry.file_name())).unwrap();
    }
    std::fs::remove_dir(&store_dir).unwrap();
    let held_before = store_files_and_lengths(&held);
    let read = store.read_segment(segment_id, &mut Vec::new());
    let answer = answer_text(&read);
    drop(store);
    assert_eq!(
        answer,
        "Obstructed { name: StoreDirectory, met: DirectoryNotAtItsName }"
    );
    assert!(!store_dir.exists());
    assert_eq!(
        store_files_and_lengths(&held)
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>(),
        held_before
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn hold_the_database_for_parent() {
    let Ok(dir) = std::env::var("HALVO_TEST_HOLD_DATABASE") else {
        println!("hold_the_database_for_parent: HALVO_TEST_HOLD_DATABASE is not set, so no parent asked for a holder");
        return;
    };
    let dir = PathBuf::from(dir);
    let key = std::fs::read(dir.join("raw-key")).unwrap();
    let conn = Connection::open(dir.join("store").join("store.db")).unwrap();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    conn.execute_batch(&format!(
        "PRAGMA key = \"x'{hex}'\"; PRAGMA locking_mode = EXCLUSIVE;"
    ))
    .unwrap();
    conn.query_row("SELECT count(*) FROM segment_keys", [], |r| {
        r.get::<_, i64>(0)
    })
    .unwrap();
    conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;").unwrap();
    std::fs::write(dir.join("child-ready"), b"").unwrap();
    let started = Instant::now();
    while !dir.join("child-stop").exists() && started.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn the_live_holder_probe_answers_locked_while_another_process_holds_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let key = *store.store_key().unwrap().expose();
    drop(store);
    std::fs::write(dir.path().join("raw-key"), key).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::hold_the_database_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_TEST_HOLD_DATABASE", dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    wait_for(&dir.path().join("child-ready"), &mut child);
    let while_held = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    std::fs::write(dir.path().join("child-stop"), b"").unwrap();
    assert!(child.wait().unwrap().success(), "the holding child failed");
    std::fs::remove_file(dir.path().join("raw-key")).unwrap();
    let after = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    assert_eq!(while_held, "Locked");
    assert_eq!(after, "Ok");
}

#[cfg(target_os = "macos")]
fn swap_race(arm: &str, seam: &'static str, rounds: usize) -> String {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let decoy_source = tempfile::tempdir().unwrap();
    let (decoy_store, _, _, _) = file_root_store(decoy_source.path());
    let parent = dir.path().to_path_buf();
    let (mut ok, mut obstructed, mut other, mut decoy_changed, mut kept) = (0, 0, 0, 0, 0);
    for round in 0..rounds {
        let decoy = parent.join(format!("decoy-{round}"));
        let theirs = a_decoy_at(&decoy, &decoy_store);
        let aside = parent.join(format!("aside-{round}"));
        let act = {
            let (store_dir, decoy, aside, arm) = (
                store_dir.clone(),
                decoy.clone(),
                aside.clone(),
                arm.to_string(),
            );
            move || {
                if arm == "swap" {
                    swap_directories(&store_dir, &decoy);
                } else {
                    std::fs::rename(&store_dir, &aside).unwrap();
                    std::fs::rename(&decoy, &store_dir).unwrap();
                }
            }
        };
        let guard = super::failpoint::at(seam, nth_call(1, act));
        if arm == "kept-renames" {
            force_kept_name(true);
        }
        let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
        if let Ok(store) = &opened {
            if store.db().unwrap().name_form() == super::db::NameForm::Kept {
                kept += 1;
            }
        }
        let answer = answer_text(&opened);
        drop(opened);
        force_kept_name(false);
        drop(guard);
        match answer.as_str() {
            "Ok" => ok += 1,
            text if text.starts_with("Obstructed") => obstructed += 1,
            _ => other += 1,
        }
        if files_with_bytes(&store_dir) != theirs {
            decoy_changed += 1;
        }
        if arm == "swap" {
            swap_directories(&store_dir, &decoy);
        } else {
            std::fs::rename(&store_dir, &decoy).unwrap();
            std::fs::rename(&aside, &store_dir).unwrap();
        }
    }
    assert_eq!(kept, 0, "{arm}: an open used the kept name");
    format!(
        "SWAP arm={arm} seam={seam:?} rounds={rounds} ok={ok} obstructed={obstructed} \
         other={other} decoy_changed={decoy_changed} kept_name_opens={kept}"
    )
}

#[cfg(target_os = "macos")]
fn swap_directories(a: &Path, b: &Path) {
    let (a, b) = (
        std::ffi::CString::new(a.as_os_str().as_encoded_bytes()).unwrap(),
        std::ffi::CString::new(b.as_os_str().as_encoded_bytes()).unwrap(),
    );
    // SAFETY: both names are NUL-terminated copies that live across the call.
    let swapped = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
    assert_eq!(swapped, 0, "{}", io::Error::last_os_error());
}

#[cfg(target_os = "macos")]
#[test]
fn the_swap_race_counts_in_both_windows_under_the_volume_name() {
    use super::db::{SEAM_AFTER_THE_LAST_LOOK, SEAM_BEFORE_THE_FIRST_READ};
    let readings = super::db::volume_readings(&super::durable::Dir::open(Path::new("/")).unwrap());
    println!("VOLUME readings={readings:?} ids={:?}", readings.ids());
    for arm in ["kept-renames", "renames", "swap"] {
        for seam in [SEAM_AFTER_THE_LAST_LOOK, SEAM_BEFORE_THE_FIRST_READ] {
            println!("{}", swap_race(arm, seam, 20));
        }
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn root_and_key_file_after_the_first_call(
    dir: &Path,
    roots_dir: &Path,
    root_id: RootId,
) -> (bool, bool) {
    let roots = FileRoots::new(roots_dir.to_path_buf(), TestProtector::new(0x5a).0);
    (
        roots.root_path(root_id).exists(),
        KeyFile::path(dir).exists(),
    )
}

#[test]
fn delete_all_on_a_damaged_page_destroys_the_root_and_the_key_file_in_its_first_call() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let (segment, key_id) = store
        .write_segment(&mut Cursor::new(b"with events"))
        .unwrap();
    for i in 1..=10u64 {
        let event = crate::event::CaptureStreamEvent::new_raw(
            crate::event::CapturedAt {
                wall_clock: std::time::SystemTime::now(),
                sequence: crate::event::MonotonicSequence::new(i),
            },
            crate::event::Modality::ForegroundApp,
            format!("com.example.app-{i}"),
            segment.clone(),
            key_id,
        );
        store.append_event(&event).unwrap();
    }
    let index_page: i64 = store
        .db()
        .unwrap()
        .connection()
        .query_row(
            "SELECT rootpage FROM sqlite_master WHERE type = 'table' AND name = 'events'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    store.db().unwrap().checkpoint_truncate().unwrap();
    drop(store);
    let mut db = std::fs::File::options()
        .write(true)
        .open(store_dir.join("store.db"))
        .unwrap();
    std::io::Seek::seek(
        &mut db,
        std::io::SeekFrom::Start((index_page as u64 - 1) * 4096 + 100),
    )
    .unwrap();
    db.write_all(&[0xA5; 512]).unwrap();
    db.sync_all().unwrap();
    drop(db);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let first = store.delete_all();
    let (root_left, key_left) =
        root_and_key_file_after_the_first_call(&store_dir, &roots_dir, root_id);
    println!("case delete_all={first:?} store_key_file_exists={key_left} root_exists={root_left}");
    assert!(!root_left && !key_left, "{first:?}");
    assert_eq!(
        files_under(&store_dir.join("segments")),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn delete_all_under_query_only_destroys_the_root_and_the_key_file() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store
        .db()
        .unwrap()
        .connection()
        .execute_batch("PRAGMA query_only = ON;")
        .unwrap();
    let first = store.delete_all();
    let (root_left, key_left) =
        root_and_key_file_after_the_first_call(&store_dir, &roots_dir, root_id);
    assert!(!root_left && !key_left, "{first:?}");
}

#[cfg(unix)]
#[test]
fn delete_all_with_a_blocked_shard_destroys_the_root_and_the_key_file_and_the_tail_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let shard = segment_path(&store_dir, segment_id)
        .parent()
        .unwrap()
        .to_path_buf();
    set_mode(&shard, 0o500);
    let first = store.delete_all();
    set_mode(&shard, 0o700);
    let report = delete_report(&first);
    let (root_left, key_left) =
        root_and_key_file_after_the_first_call(&store_dir, &roots_dir, root_id);
    assert!(!root_left && !key_left, "{report:?}");
    assert_eq!(
        report.files_left, 3,
        "the segment file, its shard and segments/ stay, each counted: {report:?}"
    );
    assert!(store.write_segment(&mut Cursor::new(b"x")).is_err());
    drop(store);
    let tail = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    assert!(tail.is_ok(), "{tail:?}");
    assert!(!segment_path(&store_dir, segment_id).exists());
}

#[cfg(unix)]
#[test]
fn delete_all_with_a_linked_segments_directory_beside_a_blocked_shard_destroys_the_root_first() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("theirs"), b"theirs").unwrap();
    let segments = store_dir.join("segments");
    std::fs::rename(&segments, dir.path().join("segments-aside")).unwrap();
    std::os::unix::fs::symlink(outside.path(), &segments).unwrap();
    let report = delete_report(&store.delete_all());
    let (root_left, key_left) =
        root_and_key_file_after_the_first_call(&store_dir, &roots_dir, root_id);
    assert!(!root_left && !key_left, "{report:?}");
    assert_eq!(
        std::fs::read(outside.path().join("theirs")).unwrap(),
        b"theirs"
    );
    assert!(report.not_followed >= 1, "{report:?}");
}

#[cfg(unix)]
#[test]
fn a_roots_directory_at_0500_destroys_the_root_by_its_zeros_and_seals_the_handle() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let roots = FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0);
    let wrapped = KeyFile::read(&store_dir)
        .unwrap()
        .wrapped_store_key()
        .unwrap();
    set_mode(&roots_dir, 0o500);
    let destroyed = roots.destroy_root(root_id);
    let unwrap = roots
        .unwrap(root_id, crate::custody::KeyLabel::STORE, &wrapped)
        .map(drop);
    set_mode(&roots_dir, 0o700);
    assert!(destroyed.is_ok(), "{destroyed:?}");
    assert!(
        matches!(unwrap, Err(CustodyError::RootMissing)),
        "{unwrap:?}"
    );
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    set_mode(&roots_dir, 0o500);
    let first = store.delete_all();
    let write = store.write_segment(&mut Cursor::new(b"after")).map(drop);
    set_mode(&roots_dir, 0o700);
    assert!(first.is_err(), "{first:?}");
    assert!(matches!(write, Err(StoreError::Sealed)), "{write:?}");
    let second = store.delete_all();
    assert!(second.is_ok(), "{second:?}");
}

#[cfg(unix)]
#[test]
fn a_store_directory_at_0500_fails_the_first_call_and_the_retry_on_the_handle_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    set_mode(&store_dir, 0o500);
    let first = store.delete_all();
    let write = store.write_segment(&mut Cursor::new(b"after")).map(drop);
    set_mode(&store_dir, 0o700);
    assert!(first.is_err(), "{first:?}");
    assert!(matches!(write, Err(StoreError::Sealed)), "{write:?}");
    let retry = store.delete_all();
    assert!(retry.is_ok(), "{retry:?}");
    assert!(!KeyFile::path(&store_dir).exists());
    assert!(!store_dir.join("store.db").exists());
}

#[cfg(unix)]
#[test]
fn delete_all_removes_a_store_key_and_a_root_file_of_mode_0400() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let roots = FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0);
    set_mode(&KeyFile::path(&store_dir), 0o400);
    set_mode(&roots.root_path(root_id), 0o400);
    let first = store.delete_all();
    assert!(!KeyFile::path(&store_dir).exists(), "{first:?}");
    assert!(!roots.root_path(root_id).exists(), "{first:?}");
}

#[cfg(unix)]
#[test]
fn a_root_file_of_mode_000_is_root_denied_and_the_tail_removes_the_files_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let roots = FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0);
    set_mode(&roots.root_path(root_id), 0o000);
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    set_mode(&roots_dir, 0o500);
    let tail = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    set_mode(&roots_dir, 0o700);
    let report = delete_report(&tail);
    assert_eq!(opened, "Custody(RootDenied)");
    assert_eq!(
        store_files_in(&store_dir),
        Vec::<String>::new(),
        "{report:?}"
    );
    assert_eq!(report.roots.left, vec![root_id], "{report:?}");
}

#[test]
fn the_tail_cleans_a_directory_with_a_zeroed_key_file_and_a_fresh_create_takes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, segment_id) = file_root_store(dir.path());
    let len = std::fs::metadata(KeyFile::path(&store_dir)).unwrap().len() as usize;
    std::fs::write(KeyFile::path(&store_dir), vec![0u8; len]).unwrap();
    let tail = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    assert!(tail.is_ok(), "{tail:?}");
    assert_eq!(delete_report(&tail).root, super::RootReading::Unnamed);
    assert_eq!(store_files_in(&store_dir), Vec::<String>::new());
    assert!(!segment_path(&store_dir, segment_id).exists());
    assert_eq!(
        root_files_in(&roots_dir),
        Vec::<String>::new(),
        "the tail beside a key file that names nothing left a root file under roots (ruling ST-45 H1)"
    );
    let custody = file_root_custody(
        &roots_dir,
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
    assert!(created.is_ok(), "{:?}", created.err());
}

#[test]
fn the_tail_on_a_key_file_of_version_1_removes_the_files_and_reports_its_root_as_not_handled() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    let older = RootId::fresh().unwrap();
    std::fs::write(
        KeyFile::path(&store_dir),
        format!(
            "{{\"version\":1,\"custody_kind\":\"dev_fallback\",\"root_id\":\"{}\",\"wrapped_store_key_b64\":\"AAAA\"}}",
            uuid::Uuid::from_bytes(*older.as_bytes()).hyphenated()
        ),
    )
    .unwrap();
    let report = delete_report(&EncryptedStore::delete_store_dir(&store_dir, &roots_dir));
    assert_eq!(report.root_not_handled, Some(older), "{report:?}");
    assert_eq!(store_files_in(&store_dir), Vec::<String>::new());
    assert!(!report.root_destroyed());
}

#[test]
fn a_failed_create_keeps_no_key_file_at_either_failpoint() {
    let mut problems = Vec::new();
    for point in ["key_file_before_publish", "create_after_the_key_file"] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
        let root_id = RootId::fresh().unwrap();
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        super::db::failpoint::arm(point);
        let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
        let left = store_files_in(&store_dir);
        let temp = store_dir.join("store.key.tmp").exists();
        let roots = root_files_in(&roots_dir);
        if created.is_ok() || !left.is_empty() || temp || !roots.is_empty() {
            problems.push(format!(
                "{point}: {:?} left {left:?}, a temporary key file {temp}, roots {roots:?}",
                created.as_ref().map(drop)
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[cfg(unix)]
#[test]
fn kill_a_create_after_its_root_for_parent() {
    let Ok(dir) = std::env::var("HALVO_TEST_KILLED_CREATE_DIR") else {
        println!("kill_a_create_after_its_root_for_parent: HALVO_TEST_KILLED_CREATE_DIR is not set, so no parent asked for a killed create");
        return;
    };
    let dir = PathBuf::from(dir);
    let root_id = RootId::parse(&std::fs::read_to_string(dir.join("root-id")).unwrap()).unwrap();
    let custody = file_root_custody(&dir.join("roots"), TestProtector::new(0x5a).0, root_id);
    let _seam = super::failpoint::at(super::db::SEAM_AFTER_THE_ROOT, || {
        // SAFETY: kill with the process's own id ends this test process, which
        // the parent started for this case alone.
        unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
    });
    let _ = EncryptedStore::create(&dir.join("store"), Box::new(custody), DEFAULT_RETENTION);
}

#[cfg(unix)]
#[test]
fn a_create_killed_between_its_root_and_its_key_file_leaves_a_root_that_the_next_create_destroys() {
    let dir = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh().unwrap();
    std::fs::write(dir.path().join("root-id"), root_id.to_string()).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::kill_a_create_after_its_root_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_TEST_KILLED_CREATE_DIR", dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    let roots_dir = dir.path().join("roots");
    let store_dir = dir.path().join("store");
    let root_after_the_kill = root_files_in(&roots_dir);
    let pending = store_dir.join("create.pending").exists();
    println!(
        "child_status={status:?} root_exists={} pending={pending}",
        !root_after_the_kill.is_empty()
    );
    assert!(!status.success(), "the child was not killed: {status:?}");
    assert_eq!(root_after_the_kill, vec![format!("{root_id}.root")]);
    assert!(pending, "the killed create left no create.pending");
    let custody = file_root_custody(
        &roots_dir,
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let fresh_root = custody.root_id;
    let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
    assert!(created.is_ok(), "{:?}", created.err());
    assert_eq!(
        root_files_in(&roots_dir),
        vec![format!("{fresh_root}.root")]
    );
    assert!(!store_dir.join("create.pending").exists());
}

#[cfg(unix)]
#[test]
fn create_for_parent() {
    let Ok(dir) = std::env::var("HALVO_TEST_CREATE_DIR") else {
        println!(
            "create_for_parent: HALVO_TEST_CREATE_DIR is not set, so no parent asked for a create"
        );
        return;
    };
    let dir = PathBuf::from(dir);
    let index = std::env::var("HALVO_TEST_CREATE_INDEX").unwrap();
    let custody = file_root_custody(
        &dir.join(format!("roots-{index}")),
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    while !dir.join("go").exists() {
        std::thread::sleep(Duration::from_micros(200));
    }
    let created = EncryptedStore::create(&dir.join("store"), Box::new(custody), DEFAULT_RETENTION);
    let answer = match &created {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?}"),
    };
    std::fs::write(dir.join(format!("answer-{index}")), &answer).unwrap();
    if created.is_ok() {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
#[test]
fn two_processes_that_create_on_one_directory_never_both_answer_ok() {
    const RUNS: usize = 20;
    let mut both_ok = 0;
    let mut answers = Vec::new();
    for _ in 0..RUNS {
        let dir = tempfile::tempdir().unwrap();
        let children: Vec<Child> = (0..2)
            .map(|index| {
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "store::hardening_tests::create_for_parent",
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env("HALVO_TEST_CREATE_DIR", dir.path())
                    .env("HALVO_TEST_CREATE_INDEX", index.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap()
            })
            .collect();
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(dir.path().join("go"), b"").unwrap();
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        let got: Vec<String> = (0..2)
            .map(|index| {
                std::fs::read_to_string(dir.path().join(format!("answer-{index}"))).unwrap()
            })
            .collect();
        if got.iter().all(|answer| answer == "Ok") {
            both_ok += 1;
        }
        answers.push(got);
    }
    println!("CREATE-RACE runs={RUNS} both_ok={both_ok} answers={answers:?}");
    assert_eq!(both_ok, 0, "{answers:?}");
}

#[test]
fn a_second_create_in_one_process_is_refused_while_a_first_is_under_way() {
    const RUNS: usize = 20;
    let mut both_ok = 0;
    for _ in 0..RUNS {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = dir.path().join("store");
        let second_answer = Arc::new(std::sync::Mutex::new(None));
        let act = {
            let (store_dir, roots, second_answer) = (
                store_dir.clone(),
                dir.path().join("roots-1"),
                Arc::clone(&second_answer),
            );
            move || {
                let _ = std::fs::remove_file(store_dir.join("writer.lock"));
                let (store_dir, roots) = (store_dir.clone(), roots.clone());
                let second = std::thread::spawn(move || {
                    EncryptedStore::create(
                        &store_dir,
                        Box::new(file_root_custody(
                            &roots,
                            TestProtector::new(0x5a).0,
                            RootId::fresh().unwrap(),
                        )),
                        DEFAULT_RETENTION,
                    )
                    .is_ok()
                })
                .join()
                .unwrap();
                *second_answer.lock().unwrap() = Some(second);
            }
        };
        let seam = super::failpoint::at(super::db::SEAM_BEFORE_THE_MODE_STEP, nth_call(1, act));
        let first = EncryptedStore::create(
            &store_dir,
            Box::new(file_root_custody(
                &dir.path().join("roots-0"),
                TestProtector::new(0x5a).0,
                RootId::fresh().unwrap(),
            )),
            DEFAULT_RETENTION,
        );
        drop(seam);
        if *second_answer.lock().unwrap() == Some(true) {
            both_ok += 1;
        }
        drop(first);
    }
    println!("CREATE-RACE-IN-PROCESS runs={RUNS} both_ok={both_ok}");
    assert_eq!(both_ok, 0);
}

#[test]
fn two_root_writes_under_one_id_never_both_answer_ok_and_replace_no_root() {
    const ROUNDS: usize = 500;
    let (mut both, mut neither, mut unopened) = (0, 0, 0);
    for _ in 0..ROUNDS {
        let dir = tempfile::tempdir().unwrap();
        let roots_dir = dir.path().join("roots");
        let root_id = RootId::fresh().unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let writers: Vec<_> = (0..2)
            .map(|_| {
                let (roots_dir, barrier) = (roots_dir.clone(), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    let custody =
                        file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
                    barrier.wait();
                    custody.create_root().map(|()| {
                        custody
                            .wrap_store_key(
                                &StoreKey::from_bytes([0x3c; 32]),
                                crate::custody::KeyLabel::STORE,
                            )
                            .unwrap()
                    })
                })
            })
            .collect();
        let answers: Vec<_> = writers.into_iter().map(|w| w.join().unwrap()).collect();
        let oks: Vec<&WrappedKey> = answers.iter().filter_map(|a| a.as_ref().ok()).collect();
        let refused = answers
            .iter()
            .filter(|a| matches!(a, Err(CustodyError::RootExists)))
            .count();
        match (oks.len(), refused) {
            (1, 1) => {}
            (2, _) => both += 1,
            _ => neither += 1,
        }
        let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
        if let Some(wrapped) = oks.first() {
            if custody
                .unwrap_store_key(wrapped, crate::custody::KeyLabel::STORE)
                .is_err()
            {
                unopened += 1;
            }
        }
    }
    println!("ROOT-RACE rounds={ROUNDS} both_ok={both} neither={neither} unopened={unopened}");
    assert_eq!((both, neither, unopened), (0, 0, 0));
}

#[test]
fn two_creates_under_one_root_id_in_two_directories_never_both_answer_ok() {
    const ROUNDS: usize = 300;
    let (mut both, mut neither, mut unopened) = (0, 0, 0);
    for _ in 0..ROUNDS {
        let dir = tempfile::tempdir().unwrap();
        let roots_dir = dir.path().join("roots");
        let root_id = RootId::fresh().unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let creators: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|name| {
                let (store_dir, roots_dir, barrier) = (
                    dir.path().join(name),
                    roots_dir.clone(),
                    Arc::clone(&barrier),
                );
                std::thread::spawn(move || {
                    let custody =
                        file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
                    barrier.wait();
                    let created =
                        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION)
                            .map(drop);
                    (store_dir, created)
                })
            })
            .collect();
        let answers: Vec<_> = creators.into_iter().map(|c| c.join().unwrap()).collect();
        let made: Vec<&PathBuf> = answers
            .iter()
            .filter(|(_, created)| created.is_ok())
            .map(|(dir, _)| dir)
            .collect();
        let refused = answers
            .iter()
            .filter(|(_, created)| {
                matches!(created, Err(StoreError::Custody(CustodyError::RootExists)))
            })
            .count();
        match (made.len(), refused) {
            (1, 1) => {}
            (2, _) => both += 1,
            _ => neither += 1,
        }
        for store_dir in made {
            if open_over_file_root(store_dir, &roots_dir, root_id).is_err() {
                unopened += 1;
            }
        }
    }
    println!(
        "STORE-ROOT-RACE rounds={ROUNDS} both_ok={both} neither={neither} unopened={unopened}"
    );
    assert_eq!((both, neither, unopened), (0, 0, 0));
}

#[test]
fn a_root_that_appears_between_the_look_and_the_write_is_kept_and_the_create_answers_root_exists() {
    let dir = tempfile::tempdir().unwrap();
    let roots_dir = dir.path().join("roots");
    let root_id = RootId::fresh().unwrap();
    let theirs = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let ours = file_root_custody(&roots_dir, TestProtector::new(0x77).0, root_id);
    let (protector, _) = TestProtector::new(0x5a);
    let first_look = FileRoots::new(roots_dir.clone(), protector);
    assert!(!first_look.root_exists(root_id).unwrap());
    theirs.create_root().unwrap();
    let wrapped = theirs
        .wrap_store_key(
            &StoreKey::from_bytes([0x2b; 32]),
            crate::custody::KeyLabel::STORE,
        )
        .unwrap();
    let made = ours.roots.create_root_without_a_look(root_id);
    assert!(matches!(made, Err(CustodyError::RootExists)), "{made:?}");
    assert_eq!(
        theirs
            .unwrap_store_key(&wrapped, crate::custody::KeyLabel::STORE)
            .unwrap()
            .expose(),
        &[0x2b; 32]
    );
}

#[test]
fn the_root_id_of_a_key_file_is_the_stem_of_its_root_file_in_32_lowercase_hex() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(KeyFile::path(&store_dir)).unwrap()).unwrap();
    let text = json["root_id"].as_str().unwrap().to_string();
    assert_eq!(text.len(), 32);
    assert!(text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
    assert_eq!(root_files_in(&roots_dir), vec![format!("{text}.root")]);
    assert_eq!(text, root_id.to_string());
}

#[test]
fn open_refuses_a_key_file_of_another_label_and_changes_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    edit_key_file(&store_dir, |v| v["namespace"] = "derived".into());
    let before = files_with_bytes(&store_dir);
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    assert_eq!(opened, "KeyFileOfAnotherLabel");
    assert_eq!(files_with_bytes(&store_dir), before);
    edit_key_file(&store_dir, |v| v["namespace"] = "Not A Label".into());
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    assert_eq!(opened, "KeyFileUnparseable");
}

#[test]
fn the_golden_key_file_of_version_2_holds_five_fields_and_reads_with_the_store_label() {
    let golden = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/testdata/store-key-v2.golden"),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&golden).unwrap();
    let mut fields: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort();
    assert_eq!(
        fields,
        [
            "custody_kind",
            "namespace",
            "root_id",
            "version",
            "wrapped_key"
        ]
    );
    assert_eq!(json["version"], 2);
    assert_eq!(json["namespace"], "store");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("store.key"), &golden).unwrap();
    let read = KeyFile::read(dir.path()).unwrap();
    assert_eq!(read.root_id.to_string(), json["root_id"].as_str().unwrap());
    assert_eq!(read.wrapped_store_key().unwrap().0.len(), 72);
}

#[cfg(unix)]
#[test]
fn a_second_key_file_at_another_path_under_one_root_with_another_label_is_its_own() {
    use super::db::KeyFile as Kf;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let other = crate::custody::KeyLabel::new("derived").unwrap();
    let second_key = StoreKey::from_bytes([0x91; 32]);
    let wrapped = custody.wrap_store_key(&second_key, other).unwrap();
    std::fs::create_dir(dir.path().join("derived")).unwrap();
    let second_path = dir.path().join("derived").join("master.key");
    let place = Kf::at(&second_path).unwrap();
    place.publish(&Kf::new(&custody, other, &wrapped)).unwrap();
    let second = Kf::at(&second_path).unwrap().read(other).unwrap();
    let first = Kf::read(&store_dir).unwrap();
    let (w1, w2) = (
        first.wrapped_store_key().unwrap(),
        second.wrapped_store_key().unwrap(),
    );
    assert!(custody
        .unwrap_store_key(&w1, crate::custody::KeyLabel::STORE)
        .is_ok());
    assert_eq!(
        custody.unwrap_store_key(&w2, other).unwrap().expose(),
        &[0x91; 32]
    );
    assert!(matches!(
        custody.unwrap_store_key(&w2, crate::custody::KeyLabel::STORE),
        Err(CustodyError::WrapAuthFailed)
    ));
    assert!(matches!(
        custody.unwrap_store_key(&w1, other),
        Err(CustodyError::WrapAuthFailed)
    ));
    assert!(matches!(
        Kf::at(&second_path)
            .unwrap()
            .read(crate::custody::KeyLabel::STORE),
        Err(StoreError::KeyFileOfAnotherLabel)
    ));
    custody.destroy_root().unwrap();
    assert!(matches!(
        custody.unwrap_store_key(&w1, crate::custody::KeyLabel::STORE),
        Err(CustodyError::RootMissing)
    ));
    assert!(matches!(
        custody.unwrap_store_key(&w2, other),
        Err(CustodyError::RootMissing)
    ));
    let target = dir.path().join("target");
    std::fs::write(&target, b"a file behind a link").unwrap();
    let linked = dir.path().join("linked.key");
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    let removal = Kf::at(&linked).unwrap().destroy();
    assert!(removal.gone && removal.not_followed == 1, "{removal:?}");
    assert_eq!(std::fs::read(&target).unwrap(), b"a file behind a link");
}

#[test]
fn two_stores_with_two_roots_locations_keep_each_root_under_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let (first_store, first_roots, _, _) = file_root_store(&dir.path().join("one"));
    let (second_store, second_roots, _, _) = file_root_store(&dir.path().join("two"));
    assert_eq!(root_files_in(&first_roots).len(), 1);
    assert_eq!(root_files_in(&second_roots).len(), 1);
    assert_ne!(root_files_in(&first_roots), root_files_in(&second_roots));
    let everywhere: usize = files_under(dir.path())
        .iter()
        .filter(|path| path.to_string_lossy().ends_with(".root"))
        .count();
    assert_eq!(everywhere, 2);
    assert!(first_store.exists() && second_store.exists());
}

#[cfg(unix)]
#[test]
fn a_store_made_by_one_build_opens_from_a_copy_of_the_binary_at_a_second_path() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, segment_id) = {
        let root = RootGuard::fresh();
        let roots_dir = root.roots();
        let mut store = EncryptedStore::create(
            &dir.path().join("store"),
            Box::new(root.custody()),
            DEFAULT_RETENTION,
        )
        .unwrap();
        let (segment, _) = store
            .write_segment(&mut Cursor::new(b"two builds"))
            .unwrap();
        drop(store);
        let kept = dir.path().join("roots");
        std::fs::create_dir(&kept).unwrap();
        for entry in std::fs::read_dir(&roots_dir).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), kept.join(entry.file_name())).unwrap();
        }
        (
            dir.path().join("store"),
            kept,
            root.id(),
            segment.segment_id,
        )
    };
    let second = dir.path().join("second-build");
    std::fs::copy(std::env::current_exe().unwrap(), &second).unwrap();
    let output = Command::new(&second)
        .args([
            "--exact",
            "store::hardening_tests::open_a_store_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_TEST_OPEN_STORE", &store_dir)
        .env("HALVO_TEST_OPEN_ROOTS", &roots_dir)
        .env("HALVO_TEST_OPEN_SEGMENT", segment_id.to_string())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find_map(|line| line.find("open=").map(|at| &line[at..]))
        .unwrap_or("none");
    println!("TWO-BUILDS {line} status={:?}", output.status);
    std::fs::remove_file(&second).unwrap();
    assert_eq!(line, "open=Ok read=Ok");
}

#[cfg(unix)]
#[test]
fn open_a_store_for_parent() {
    let Ok(dir) = std::env::var("HALVO_TEST_OPEN_STORE") else {
        println!("open_a_store_for_parent: HALVO_TEST_OPEN_STORE is not set, so no parent asked for an open");
        return;
    };
    let roots = std::env::var("HALVO_TEST_OPEN_ROOTS").unwrap();
    let segment = Uuid::parse_str(&std::env::var("HALVO_TEST_OPEN_SEGMENT").unwrap()).unwrap();
    let opened = EncryptedStore::open(Path::new(&dir), Path::new(&roots), DEFAULT_RETENTION);
    let read = match &opened {
        Ok(store) => store.read_segment(segment, &mut Vec::new()).map(drop),
        Err(_) => Err(StoreError::Sealed),
    };
    println!(
        "open={} read={}",
        if opened.is_ok() {
            "Ok".to_string()
        } else {
            format!("{:?}", opened.as_ref().err())
        },
        if read.is_ok() {
            "Ok".to_string()
        } else {
            format!("{read:?}")
        }
    );
    if let Ok(mut store) = opened {
        let _ = store.delete_all();
    }
}

#[cfg(unix)]
#[test]
fn a_store_directory_made_by_the_tests_holds_no_writer_lock() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let found: Vec<PathBuf> = files_under(&store_dir)
        .into_iter()
        .filter(|path| path.ends_with("writer.lock"))
        .collect();
    drop(store);
    assert_eq!(found, Vec::<PathBuf>::new());
}

#[cfg(target_os = "macos")]
const KILL_OPS: [&str; 6] = [
    "write",
    "write_then_pass",
    "shred",
    "delete_all",
    "create",
    "open",
];

#[cfg(target_os = "macos")]
const CRASHPOINT_DIR_KEY: &str = "HALVO_IT_DIR";
#[cfg(target_os = "macos")]
const CRASHPOINT_CALL_KEY: &str = "HALVO_IT_CRASH_AT";
#[cfg(target_os = "macos")]
const CRASHPOINT_TRACE_KEY: &str = "HALVO_IT_TRACE";
#[cfg(target_os = "macos")]
const CRASHPOINT_EXIT: i32 = 77;

#[cfg(target_os = "macos")]
fn kill_table_store(dir: &Path) -> (PathBuf, PathBuf, RootId, Vec<Uuid>) {
    let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let mut store =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
    let mut expired = Vec::new();
    for i in 0..4u8 {
        let body = vec![i + 1; 40_000];
        if i % 2 == 0 {
            let (seg, _) = store
                .write_segment_with_expiry(&mut Cursor::new(&body), 0, 1)
                .unwrap();
            expired.push(seg.segment_id);
        } else {
            store.write_segment(&mut Cursor::new(&body)).unwrap();
        }
    }
    (store_dir, roots_dir, root_id, expired)
}

#[cfg(target_os = "macos")]
#[test]
fn kill_table_child_for_parent() {
    let Ok(dir) = std::env::var("HALVO_KILL_TABLE_DIR") else {
        println!("kill_table_child_for_parent: HALVO_KILL_TABLE_DIR is not set, so no parent asked for a child");
        return;
    };
    let dir = PathBuf::from(dir);
    let op = std::env::var("HALVO_KILL_TABLE_OP").unwrap();
    let root_id = RootId::parse(&std::fs::read_to_string(dir.join("root-id")).unwrap()).unwrap();
    let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
    std::fs::write(dir.join("ARM"), b"armed").unwrap();
    match op.as_str() {
        "create" => {
            let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
            EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
        }
        "open" => drop(open_over_file_root(&store_dir, &roots_dir, root_id).unwrap()),
        _ => {
            let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
            match op.as_str() {
                "write" | "write_then_pass" => {
                    store
                        .write_segment(&mut Cursor::new(vec![9u8; 40_000]))
                        .unwrap();
                }
                "shred" => {
                    store.crypto_shred_expired().unwrap();
                }
                _ => {
                    store.delete_all().unwrap();
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn kill_table_child(
    dir: &Path,
    op: &str,
    crash_at: usize,
    trace: Option<&Path>,
    dylib: &Path,
) -> i32 {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "store::hardening_tests::kill_table_child_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_KILL_TABLE_DIR", dir)
        .env("HALVO_KILL_TABLE_OP", op)
        .env(CRASHPOINT_DIR_KEY, dir.file_name().unwrap())
        .env(CRASHPOINT_CALL_KEY, crash_at.to_string())
        .env("DYLD_INSERT_LIBRARIES", dylib)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(trace) = trace {
        command.env(CRASHPOINT_TRACE_KEY, trace);
    }
    command.status().unwrap().code().unwrap_or(-1)
}

#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq, Eq)]
struct EndStates {
    no_shredded_decrypts: bool,
    no_unnamed_root: bool,
    no_keyless_file: bool,
    /// A root file of a create stopped between its create_root and the second
    /// write of its mark stands: the mark reads owed and no key file names the
    /// root, the named limit (i) of ruling ST-44 F3, which the next tail's
    /// sweep finishes (ruling ST-45 H1).
    under_the_limit_of_a_stopped_create: bool,
}

#[cfg(target_os = "macos")]
impl EndStates {
    fn all_true(&self) -> bool {
        self.no_shredded_decrypts && self.no_unnamed_root && self.no_keyless_file
    }
}

#[cfg(target_os = "macos")]
fn end_states_after_a_kill(dir: &Path, op: &str, expired: &[Uuid]) -> EndStates {
    let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
    let read_json = |path: PathBuf| {
        std::fs::read(path)
            .ok()
            .and_then(|json| serde_json::from_slice::<serde_json::Value>(&json).ok())
    };
    let key_file = read_json(KeyFile::path(&store_dir));
    let mark = read_json(store_dir.join("create.pending"));
    let mark_root = mark
        .as_ref()
        .and_then(|v| v["root_id"].as_str().and_then(RootId::parse));
    let mark_reads_made = mark.as_ref().is_some_and(|v| v["reading"] == "made");
    let named: Vec<RootId> = [
        key_file
            .as_ref()
            .and_then(|v| v["root_id"].as_str().and_then(RootId::parse)),
        mark_root.filter(|_| mark_reads_made),
    ]
    .into_iter()
    .flatten()
    .collect();
    let id_of = |name: &str| RootId::parse(name.get(..32)?);
    let unnamed: Vec<String> = root_files_in(&roots_dir)
        .into_iter()
        .filter(|name| !id_of(name).is_some_and(|id| named.contains(&id)))
        .collect();
    let under_the_limit_of_a_stopped_create = key_file.is_none()
        && !mark_reads_made
        && !unnamed.is_empty()
        && unnamed.iter().all(|name| id_of(name) == mark_root);
    let no_unnamed_root = unnamed.is_empty() || under_the_limit_of_a_stopped_create;
    let root_id = named.first().copied();
    let root_file = root_id.map(|id| roots_dir.join(format!("{id}.root")).exists());
    let opened = root_id.map(|id| open_over_file_root(&store_dir, &roots_dir, id));
    let (mut no_shredded_decrypts, mut no_keyless_file) = (true, true);
    if op == "delete_all" && root_file != Some(true) {
        no_shredded_decrypts = !matches!(opened, Some(Ok(_)));
    }
    if let Some(Ok(mut store)) = opened {
        if op == "shred" {
            let conn = store.db().unwrap().connection();
            let cleared: Vec<Uuid> = expired
                .iter()
                .copied()
                .filter(|id| {
                    conn.query_row(
                        "SELECT wrapped_key IS NULL FROM segment_keys WHERE segment_id = ?1",
                        [id.simple().to_string()],
                        |r| r.get::<_, bool>(0),
                    )
                    .unwrap_or(true)
                })
                .collect();
            no_shredded_decrypts = cleared
                .iter()
                .all(|id| store.read_segment(*id, &mut io::sink()).is_err());
        }
        if op == "write_then_pass" || op == "shred" || op == "write" {
            let _ = store.crypto_shred_expired();
        }
        let live: Vec<String> = {
            let conn = store.db().unwrap().connection();
            let mut statement = conn
                .prepare("SELECT segment_id FROM segment_keys WHERE wrapped_key IS NOT NULL")
                .unwrap();
            statement
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        if op == "write_then_pass" || op == "shred" {
            no_keyless_file = files_under(&store_dir.join("segments")).iter().all(|path| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                live.iter().any(|id| name == format!("{id}.seg"))
            });
        }
    }
    EndStates {
        no_shredded_decrypts,
        no_unnamed_root,
        no_keyless_file,
        under_the_limit_of_a_stopped_create,
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_kill_before_each_file_system_call_of_each_operation_leaves_no_key_or_root_behind() {
    let work = tempfile::tempdir().unwrap();
    let dylib = work.path().join("crashpoint.dylib");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store/testdata/crashpoint.c");
    let built = Command::new("cc")
        .args(["-dynamiclib", "-o"])
        .arg(&dylib)
        .arg(&source)
        .status()
        .unwrap();
    assert!(built.success(), "cc did not build the interposer");
    let prepare = |op: &str| -> (tempfile::TempDir, Vec<Uuid>) {
        let dir = tempfile::tempdir().unwrap();
        let (_, _, root_id, expired) = kill_table_store(dir.path());
        std::fs::write(dir.path().join("root-id"), root_id.to_string()).unwrap();
        if op == "create" {
            let store_dir = dir.path().join("store");
            let mut store =
                open_over_file_root(&store_dir, &dir.path().join("roots"), root_id).unwrap();
            store.delete_all().unwrap();
            drop(store);
            std::fs::write(
                dir.path().join("root-id"),
                RootId::fresh().unwrap().to_string(),
            )
            .unwrap();
        }
        (dir, expired)
    };
    let mut rows = 0;
    let mut wrong = Vec::new();
    for op in KILL_OPS {
        let (dir, expired) = prepare(op);
        let before_the_run = finished_delete_all_reading(dir.path());
        let trace = work.path().join(format!("trace-{op}.txt"));
        let code = kill_table_child(dir.path(), op, 0, Some(&trace), &dylib);
        assert_eq!(code, 0, "the {op} child failed without a kill");
        let unkilled = end_states_after_a_kill(dir.path(), op, &expired);
        println!("KILL op={op} unkilled states={unkilled:?}");
        assert!(
            unkilled.all_true() && !unkilled.under_the_limit_of_a_stopped_create,
            "the unkilled {op} run left {unkilled:?}"
        );
        if op == "delete_all" {
            let after = finished_delete_all_reading(dir.path());
            println!(
                "KILL op=delete_all unkilled store_key_gone={} named_root_gone={} unwraps={}",
                after.store_key_gone,
                after.named_root_gone(dir.path(), before_the_run.root_id),
                before_the_run.unwraps_now(dir.path())
            );
            assert!(
                after.store_key_gone
                    && after.named_root_gone(dir.path(), before_the_run.root_id)
                    && !before_the_run.unwraps_now(dir.path()),
                "the unkilled delete-all left its key file, its root or a root that unwraps"
            );
        }
        let calls = std::fs::read_to_string(&trace).unwrap().lines().count();
        assert!(calls > 0, "the {op} child made no counted call");
        for at in 1..=calls {
            let (dir, expired) = prepare(op);
            let code = kill_table_child(dir.path(), op, at, None, &dylib);
            let states = end_states_after_a_kill(dir.path(), op, &expired);
            rows += 1;
            println!(
                "KILL op={op} before_call={at} exit={code} states={states:?}{}",
                if states.under_the_limit_of_a_stopped_create {
                    " named-limit=ST-44 F3 (i)"
                } else {
                    ""
                }
            );
            if code != CRASHPOINT_EXIT || !states.all_true() {
                wrong.push(format!("{op} at call {at}: exit {code}, states {states:?}"));
            }
        }
        println!("KILL op={op} calls={calls}");
    }
    println!("KILL rows={rows}");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let roots_dir = dir.path().join("roots");
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        EncryptedStore::create(
            &store_dir,
            Box::new(StopsAfterItsRoot(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                RootId::fresh().unwrap(),
            ))),
            DEFAULT_RETENTION,
        )
        .map(drop)
    }));
    let states = end_states_after_a_kill(dir.path(), "create", &[]);
    println!("KILL op=stopped_create states={states:?} named-limit=ST-44 F3 (i)");
    assert!(
        stopped.is_err(),
        "the stand-in custody did not stop the create"
    );
    assert!(
        states.under_the_limit_of_a_stopped_create && states.all_true(),
        "a create stopped between its create_root and the second write of its mark left {states:?}, not the named limit (i)"
    );
    let tail = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let left = root_files_in(&roots_dir);
    println!(
        "KILL op=stopped_create then the tail: answer_ok={} root_files_left={}",
        tail.is_ok(),
        left.len()
    );
    assert!(
        tail.is_ok() && left.is_empty(),
        "the tail after a create stopped under the named limit (i) left {} root files, answer {tail:?} (ruling ST-45 H1: the sweep destroys the root of a mark that reads owed)",
        left.len()
    );
}

/// The store's state before and after a delete-all of the kill table: the
/// root that its key file names and its wrapped key.
#[cfg(target_os = "macos")]
struct DeleteAllReading {
    root_id: Option<RootId>,
    wrapped: Option<WrappedKey>,
    store_key_gone: bool,
}

#[cfg(target_os = "macos")]
impl DeleteAllReading {
    fn named_root_gone(&self, dir: &Path, root_id: Option<RootId>) -> bool {
        root_id.is_some_and(|id| {
            !root_files_in(&dir.join("roots"))
                .iter()
                .any(|name| name.starts_with(&id.to_string()))
        })
    }

    fn unwraps_now(&self, dir: &Path) -> bool {
        match (self.root_id, &self.wrapped) {
            (Some(root_id), Some(wrapped)) => {
                file_root_custody(&dir.join("roots"), TestProtector::new(0x5a).0, root_id)
                    .unwrap_store_key(wrapped, crate::custody::KeyLabel::STORE)
                    .is_ok()
            }
            _ => false,
        }
    }
}

#[cfg(target_os = "macos")]
fn finished_delete_all_reading(dir: &Path) -> DeleteAllReading {
    let store_dir = dir.join("store");
    let key_file = KeyFile::read(&store_dir).ok();
    DeleteAllReading {
        root_id: key_file.as_ref().map(|k| k.root_id),
        wrapped: key_file.and_then(|k| k.wrapped_store_key().ok()),
        store_key_gone: !store_dir.join("store.key").exists(),
    }
}

/// A custody whose create_root makes the root and then stops the create, as a
/// process killed between its create_root and the second write of its mark.
#[cfg(target_os = "macos")]
struct StopsAfterItsRoot<C>(C);

#[cfg(target_os = "macos")]
impl<C: KeyCustody> KeyCustody for StopsAfterItsRoot<C> {
    fn kind(&self) -> CustodyKind {
        self.0.kind()
    }
    fn root_id(&self) -> RootId {
        self.0.root_id()
    }
    fn roots(&self) -> Option<&std::path::Path> {
        self.0.roots()
    }
    fn wrap_store_key(
        &self,
        key: &StoreKey,
        label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        self.0.wrap_store_key(key, label)
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.0.create_root()?;
        panic!("the create stops after its root, before the second write of its mark");
    }
    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        label: crate::custody::KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        self.0.unwrap_store_key(wrapped, label)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.0.destroy_root()
    }
}

#[cfg(target_os = "macos")]
fn fill_the_volume(volume: &Path) -> u64 {
    let mut written = 0u64;
    for (index, chunk) in [1 << 20, 64 << 10, 4 << 10, 512].into_iter().enumerate() {
        let path = volume.join(format!("filler-{index}"));
        let Ok(mut file) = std::fs::File::create(&path) else {
            continue;
        };
        let block = vec![0x5au8; chunk];
        while file.write_all(&block).is_ok() {
            written += chunk as u64;
        }
        let _ = file.sync_all();
    }
    written
}

#[cfg(target_os = "macos")]
fn free_the_volume(volume: &Path) {
    for index in 0..4 {
        let _ = std::fs::remove_file(volume.join(format!("filler-{index}")));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_store_on_a_full_volume_ends_delete_all_with_no_root_and_no_key_file() {
    let Ok(volume) = std::env::var("HALVO_FULL_VOLUME") else {
        println!("a_store_on_a_full_volume: HALVO_FULL_VOLUME is not set, so no check asked for a full volume");
        return;
    };
    let volume = PathBuf::from(volume);
    let scenario = std::env::var("HALVO_FULL_SCENARIO").unwrap();
    free_the_volume(&volume);
    let base = volume.join(format!("scenario-{scenario}"));
    let (store_dir, roots_dir) = (base.join("store"), base.join("roots"));
    let root_id = RootId::fresh().unwrap();
    let custody = DevFallbackCustody::new(root_id, roots_dir.clone());
    let mut store =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
    store
        .write_segment_with_expiry(&mut Cursor::new(vec![1u8; 100_000]), 0, 1)
        .unwrap();
    for i in 0..20u8 {
        store
            .write_segment(&mut Cursor::new(vec![i; 100_000]))
            .unwrap();
    }
    let answer = match scenario.as_str() {
        "A" => {
            let filled = fill_the_volume(&volume);
            println!("scenario=A filled_bytes={filled}");
            answer_text(&store.delete_all())
        }
        "B" => {
            store.db().unwrap().checkpoint_truncate().unwrap();
            let filled = fill_the_volume(&volume);
            println!("scenario=B filled_bytes={filled}");
            answer_text(&store.delete_all())
        }
        _ => {
            drop(store);
            let filled = fill_the_volume(&volume);
            println!("scenario=C filled_bytes={filled}");
            match EncryptedStore::open(&store_dir, &roots_dir, DEFAULT_RETENTION) {
                Ok(mut reopened) => answer_text(&reopened.delete_all()),
                Err(e) => {
                    println!("scenario=C open={e:?}");
                    answer_text(&EncryptedStore::delete_store_dir(&store_dir, &roots_dir))
                }
            }
        }
    };
    let root_gone = !roots_dir.join(format!("{root_id}.root")).exists();
    let key_file_gone = !KeyFile::path(&store_dir).exists();
    println!(
        "scenario={scenario} delete_all={} root_gone={} key_file_gone={}",
        answer.split(' ').next().unwrap_or(""),
        if root_gone { "yes" } else { "no" },
        if key_file_gone { "yes" } else { "no" }
    );
    free_the_volume(&volume);
    assert!(root_gone && key_file_gone, "{answer}");
}

#[test]
fn a_code_at_the_open_is_answered_by_its_class_after_a_look() {
    use super::db::{code_answer, code_seams};
    let mut answers = Vec::new();
    for code in [267, 26, 5, 6, 13, 14, 1802] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        code_seams::force(2, Some(code));
        let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
        code_seams::force(0, None);
        answers.push(format!("{code}: {opened}"));
    }
    answers.push(format!("initialize: {:?}", code_answer(Some(1))));
    answers.push(format!("no code: {:?}", code_answer(None)));
    assert_eq!(
        answers,
        [
            "267: DatabaseUnreadable",
            "26: DatabaseUnreadable",
            "5: Locked",
            "6: Locked",
            "13: Io(StorageFull)",
            "14: Io(Other)",
            "1802: Io(Other)",
            "initialize: Io(Other)",
            "no code: Io(Other)",
        ]
    );
}

#[cfg(unix)]
#[test]
fn at_the_seam_after_the_last_look_a_moved_directory_a_link_and_a_directory_answer_obstructed() {
    use super::db::SEAM_AFTER_THE_LAST_LOOK;
    let mut answers = Vec::new();
    for case in ["moved", "link at store.db", "directory at store.db-wal"] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        let act = {
            let (store_dir, aside, case) = (
                store_dir.clone(),
                dir.path().join("aside"),
                case.to_string(),
            );
            move || match case.as_str() {
                "moved" => {
                    std::fs::rename(&store_dir, &aside).unwrap();
                    std::fs::create_dir(&store_dir).unwrap();
                }
                "link at store.db" => {
                    std::fs::rename(store_dir.join("store.db"), &aside).unwrap();
                    std::os::unix::fs::symlink(&aside, store_dir.join("store.db")).unwrap();
                }
                _ => {
                    let _ = std::fs::remove_file(store_dir.join("store.db-wal"));
                    std::fs::create_dir(store_dir.join("store.db-wal")).unwrap();
                }
            }
        };
        #[cfg(target_os = "macos")]
        force_kept_name(true);
        let seam = super::failpoint::at(SEAM_AFTER_THE_LAST_LOOK, nth_call(2, act));
        let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
        drop(seam);
        #[cfg(target_os = "macos")]
        force_kept_name(false);
        answers.push(format!("{case}: {opened}"));
    }
    println!("ST35 {answers:?}");
    assert!(
        answers.iter().all(|answer| answer.contains("Obstructed {")),
        "{answers:#?}"
    );
}

#[cfg(unix)]
#[test]
fn an_empty_database_at_the_name_of_a_moved_directory_answers_obstructed_and_not_older_store() {
    use super::db::SEAM_AFTER_THE_LAST_LOOK;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let act = {
        let (store_dir, aside) = (store_dir.clone(), dir.path().join("aside"));
        move || {
            std::fs::rename(&store_dir, &aside).unwrap();
            std::fs::create_dir(&store_dir).unwrap();
            std::fs::write(store_dir.join("store.db"), b"").unwrap();
        }
    };
    #[cfg(target_os = "macos")]
    force_kept_name(true);
    let seam = super::failpoint::at(SEAM_AFTER_THE_LAST_LOOK, nth_call(2, act));
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    drop(seam);
    #[cfg(target_os = "macos")]
    force_kept_name(false);
    println!("W1 {opened}");
    assert!(opened.starts_with("Obstructed {"), "{opened}");
}

#[cfg(unix)]
#[test]
fn a_create_after_its_directory_was_swapped_for_an_empty_one_answers_by_the_look() {
    use super::db::SEAM_AFTER_THE_LAST_LOOK;
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    let act = {
        let (store_dir, aside) = (store_dir.clone(), dir.path().join("aside"));
        move || {
            std::fs::rename(&store_dir, &aside).unwrap();
            std::fs::create_dir(&store_dir).unwrap();
        }
    };
    #[cfg(target_os = "macos")]
    force_kept_name(true);
    let seam = super::failpoint::at(SEAM_AFTER_THE_LAST_LOOK, nth_call(1, act));
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let created = answer_text(&EncryptedStore::create(
        &store_dir,
        Box::new(custody),
        DEFAULT_RETENTION,
    ));
    drop(seam);
    #[cfg(target_os = "macos")]
    force_kept_name(false);
    println!(
        "T1 create after the swap answered {created}; at the name {:?}",
        names_in_store_dir(&store_dir)
    );
    assert!(created.starts_with("Obstructed {"), "{created}");
}

#[cfg(target_os = "macos")]
fn force_kept_name(on: bool) {
    use super::db::{name_seams, RootAnswer, VolumeReadings};
    if on {
        name_seams::answer_stat(Some(Box::new(|| {
            Err(io::Error::from_raw_os_error(libc::ENOENT))
        })));
        name_seams::answer_volume(Some(VolumeReadings {
            flag: Some(false),
            capability: Some((true, false)),
            root: Some(RootAnswer::NoSuchName),
        }));
    } else {
        name_seams::answer_stat(None);
        name_seams::answer_volume(None);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_no_such_name_answer_chooses_the_kept_name_only_on_a_volume_that_reads_no_both_times() {
    use super::db::{choose_the_name, DirectoryStat as S, NameChoice as C, VolumeIds as V};
    let rows: [(S, V, V, bool, u32, C); 16] = [
        (S::TheHandle, V::Yes, V::Yes, false, 2, C::Volume),
        (S::TheHandle, V::Unknown, V::Unknown, false, 2, C::Volume),
        (S::AnotherIdentity, V::Yes, V::Yes, false, 2, C::Refused),
        (S::AnotherIdentity, V::No, V::No, false, 2, C::Refused),
        (S::Failed, V::Yes, V::Yes, false, 2, C::Failed),
        (S::NoSuchName, V::No, V::No, false, 2, C::Kept),
        (S::NoSuchName, V::No, V::No, true, 0, C::Kept),
        (S::NoSuchName, V::No, V::Yes, false, 2, C::Refused),
        (S::NoSuchName, V::Yes, V::No, false, 2, C::Refused),
        (S::NoSuchName, V::No, V::Unknown, false, 2, C::Refused),
        (S::NoSuchName, V::Unknown, V::Unknown, false, 2, C::Refused),
        (S::NoSuchName, V::Yes, V::Yes, false, 2, C::StatAgain),
        (S::NoSuchName, V::Yes, V::Yes, false, 1, C::StatAgain),
        (S::NoSuchName, V::Yes, V::Yes, false, 0, C::Refused),
        (S::NoSuchName, V::Yes, V::Yes, true, 2, C::Refused),
        (S::NoSuchName, V::Unknown, V::Unknown, true, 2, C::Refused),
    ];
    let wrong: Vec<String> = rows
        .iter()
        .filter(|(stat, before, after, away, left, want)| {
            choose_the_name(*stat, *before, *after, *away, *left) != *want
        })
        .map(|row| format!("{row:?}"))
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[cfg(target_os = "macos")]
#[test]
fn the_three_readings_of_a_volume_decide_yes_no_and_unknown() {
    use super::db::{RootAnswer as R, VolumeIds as V, VolumeReadings};
    let read = |flag, capability, root| {
        VolumeReadings {
            flag,
            capability,
            root,
        }
        .ids()
    };
    let rows = [
        (
            read(Some(false), Some((true, true)), Some(R::NoSuchName)),
            V::Yes,
        ),
        (
            read(Some(true), Some((true, false)), Some(R::NoSuchName)),
            V::Yes,
        ),
        (
            read(Some(false), Some((true, false)), Some(R::TheRoot)),
            V::Yes,
        ),
        (
            read(Some(false), Some((true, false)), Some(R::NoSuchName)),
            V::No,
        ),
        (
            read(Some(false), Some((false, false)), Some(R::NoSuchName)),
            V::Unknown,
        ),
        (read(Some(false), None, Some(R::NoSuchName)), V::Unknown),
        (
            read(None, Some((true, false)), Some(R::NoSuchName)),
            V::Unknown,
        ),
        (
            read(Some(false), Some((true, false)), Some(R::Other)),
            V::Unknown,
        ),
        (read(Some(false), Some((true, false)), None), V::Unknown),
    ];
    for (index, (got, want)) in rows.iter().enumerate() {
        assert_eq!(got, want, "row {index}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn the_volume_of_the_tests_reads_yes_and_a_volume_that_reads_no_gets_the_kept_name() {
    use super::db::{name_seams, NameForm, RootAnswer, VolumeIds, VolumeReadings};
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let handle = super::durable::Dir::open(&store_dir).unwrap();
    let readings = super::db::volume_readings(&handle);
    println!("VOLUME readings={readings:?} ids={:?}", readings.ids());
    assert_eq!(readings.ids(), VolumeIds::Yes);
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    assert_eq!(opened.db().unwrap().volume_ids(), VolumeIds::Yes);
    drop(opened);
    force_kept_name(true);
    let kept = open_over_file_root(&store_dir, &roots_dir, root_id);
    force_kept_name(false);
    let kept = kept.unwrap();
    assert_eq!(kept.db().unwrap().name_form(), NameForm::Kept);
    assert_eq!(kept.db().unwrap().volume_ids(), VolumeIds::No);
    drop(kept);
    name_seams::answer_stat(Some(Box::new(|| {
        Err(io::Error::from_raw_os_error(libc::ENOENT))
    })));
    let on_yes = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    name_seams::answer_volume(Some(VolumeReadings {
        flag: None,
        capability: None,
        root: Some(RootAnswer::Other),
    }));
    let on_unknown = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    force_kept_name(false);
    assert_eq!(
        on_yes,
        "Obstructed { name: StoreDirectory, met: DirectoryNotAtItsName }"
    );
    assert_eq!(
        on_unknown,
        "Obstructed { name: StoreDirectory, met: DirectoryNotAtItsName }"
    );
}

#[cfg(unix)]
#[test]
fn an_old_log_with_bytes_in_the_directory_while_sqlite_holds_another_log_is_refused() {
    use super::db::SEAM_AT_THE_LOG_CHECK;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let act = {
        let (store_dir, aside) = (store_dir.clone(), dir.path().join("sqlite-log-aside"));
        move || {
            let log = store_dir.join("store.db-wal");
            if log.exists() {
                std::fs::rename(&log, &aside).unwrap();
            }
            std::fs::write(&log, vec![0x55; 4152]).unwrap();
        }
    };
    let seam = super::failpoint::at(SEAM_AT_THE_LOG_CHECK, nth_call(1, act));
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    drop(seam);
    println!(
        "W6 answer={opened} log_at_the_name_after_the_close={:?}",
        std::fs::metadata(store_dir.join("store.db-wal"))
            .map(|m| m.len())
            .ok()
    );
    assert_eq!(
        opened,
        "Obstructed { name: Log, met: DatabaseOutsideTheDirectory }"
    );
}

#[cfg(unix)]
#[test]
fn delete_all_reports_a_database_name_that_it_cannot_remove() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    std::fs::create_dir_all(store_dir.join("store.db-shm").join("held")).unwrap();
    let report = delete_report(&store.delete_all());
    assert!(report.database_left, "{report:?}");
    assert_eq!(report.not_followed, 1, "{report:?}");
    assert!(report.root_destroyed(), "{report:?}");
}

#[cfg(unix)]
#[test]
fn a_database_file_swapped_at_its_name_after_sqlite_opened_it_is_refused() {
    use super::db::SEAM_AFTER_THE_FIRST_READ;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let decoy_source = tempfile::tempdir().unwrap();
    let (decoy_store, _, _, _) = file_root_store(decoy_source.path());
    let act = {
        let (store_dir, aside, decoy) = (
            store_dir.clone(),
            dir.path().join("store-db-aside"),
            decoy_store.join("store.db"),
        );
        move || {
            std::fs::rename(store_dir.join("store.db"), &aside).unwrap();
            std::fs::copy(&decoy, store_dir.join("store.db")).unwrap();
        }
    };
    let seam = super::failpoint::at(SEAM_AFTER_THE_FIRST_READ, nth_call(1, act));
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    drop(seam);
    assert_eq!(
        opened,
        "Obstructed { name: Database, met: DatabaseOutsideTheDirectory }"
    );
}

#[test]
fn a_key_file_is_published_create_only_and_an_entry_at_its_name_stays() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let wrapped = custody
        .wrap_store_key(
            &StoreKey::from_bytes([0x19; 32]),
            crate::custody::KeyLabel::STORE,
        )
        .unwrap();
    let before = std::fs::read(KeyFile::path(&store_dir)).unwrap();
    let place = KeyFile::at(&KeyFile::path(&store_dir)).unwrap();
    let published = place.publish(&KeyFile::new(
        &custody,
        crate::custody::KeyLabel::STORE,
        &wrapped,
    ));
    assert!(
        matches!(published, Err(StoreError::AlreadyExists)),
        "{published:?}"
    );
    assert_eq!(std::fs::read(KeyFile::path(&store_dir)).unwrap(), before);
    assert!(!store_dir.join("store.key.tmp").exists());
}

#[cfg(unix)]
#[test]
fn delete_all_counts_a_shard_that_it_cannot_list_as_left() {
    use std::os::unix::fs::PermissionsExt;
    println!("CASE-NOTE: a segments shard that cannot be opened keeps its segment file; delete_all must not answer Ok while that file is left");
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment_id) = file_root_store(dir.path());
    let file = segment_path(&store_dir, segment_id);
    let shard = file.parent().unwrap().to_path_buf();
    assert!(file.exists());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o000)).unwrap();
    let answer = store.delete_all();
    std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o700)).unwrap();
    let left = file.exists();
    println!(
        "CASE-NOTE: answer {} segment file left {left}",
        answer_text(&answer)
    );
    assert!(
        !(answer.is_ok() && left),
        "delete_all answered Ok, but the segment file under an unreadable shard is still there"
    );
}

#[cfg(unix)]
#[test]
fn the_tail_counts_a_shard_that_it_cannot_list_as_left() {
    use std::os::unix::fs::PermissionsExt;
    println!("CASE-NOTE: delete_store_dir must not answer Ok while a segment file under an unreadable shard is left");
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _root_id, segment_id) = file_root_store(dir.path());
    let file = segment_path(&store_dir, segment_id);
    let shard = file.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o000)).unwrap();
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o700)).unwrap();
    let left = file.exists();
    println!(
        "CASE-NOTE: answer {} segment file left {left}",
        answer_text(&answer)
    );
    assert!(
        !(answer.is_ok() && left),
        "delete_store_dir answered Ok, but the segment file under an unreadable shard is still there"
    );
}

#[test]
fn a_create_under_the_id_of_a_live_root_is_refused_before_it_records_anything() {
    let dir = tempfile::tempdir().unwrap();
    let (_, roots_dir, root_id, _) = file_root_store(dir.path());
    let before = std::fs::read(roots_dir.join(format!("{root_id}.root"))).unwrap();
    let other = dir.path().join("other");
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let created = answer_text(&EncryptedStore::create(
        &other,
        Box::new(custody),
        DEFAULT_RETENTION,
    ));
    assert_eq!(created, "Custody(RootExists)");
    assert!(!other.join("create.pending").exists());
    assert_eq!(
        std::fs::read(roots_dir.join(format!("{root_id}.root"))).unwrap(),
        before
    );
}

#[cfg(unix)]
#[test]
fn a_pending_create_beside_an_unreadable_key_file_destroys_no_root() {
    use super::db::SEAM_BEFORE_THE_MODE_STEP;
    use super::failpoint;
    use std::os::unix::fs::PermissionsExt;
    println!("CASE-NOTE: a store whose create stopped after publishing store.key and before removing create.pending keeps its root when an open cannot read store.key for a moment; the open must refuse, not destroy the root that store.key names");
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let before = root_files_in(&roots_dir);
    assert_eq!(before.len(), 1);
    let scratch = dir.path().join("scratch");
    let pending = scratch.join("create.pending");
    let seen = pending.clone();
    let seam = failpoint::at(SEAM_BEFORE_THE_MODE_STEP, move || {
        if seen.exists() {
            panic!("review_create_02: the scratch create stops here");
        }
    });
    let custody = file_root_custody(
        &dir.path().join("scratch-roots"),
        TestProtector::new(0x5a).0,
        root_id,
    );
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        EncryptedStore::create(&scratch, Box::new(custody), DEFAULT_RETENTION).is_ok()
    }));
    drop(seam);
    assert!(stopped.is_err());
    std::fs::copy(&pending, store_dir.join("create.pending")).unwrap();
    let key = store_dir.join("store.key");
    let mode = std::fs::metadata(&key).unwrap().permissions().mode();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o000)).unwrap();
    let answer = EncryptedStore::open_with(
        &store_dir,
        &roots_dir,
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            Ok(Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    );
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(mode)).unwrap();
    println!(
        "CASE-NOTE: the open over the unreadable key file answered {}",
        answer_text(&answer)
    );
    drop(answer);
    assert_eq!(
        root_files_in(&roots_dir),
        before,
        "an open that could not read store.key destroyed the root that store.key names"
    );
    assert!(
        open_over_file_root(&store_dir, &roots_dir, root_id).is_ok(),
        "the store no longer opens once its key file can be read again"
    );
}

#[cfg(unix)]
#[test]
fn a_pending_create_beside_a_key_file_of_a_newer_version_destroys_no_root() {
    use super::db::SEAM_BEFORE_THE_MODE_STEP;
    use super::failpoint;
    use std::os::unix::fs::PermissionsExt;
    println!("CASE-NOTE: named_root answers None for a store.key of another version, so clear_pending destroys the root that both store.key and create.pending name; an open must refuse the version and keep the root");
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let before = root_files_in(&roots_dir);
    assert_eq!(before.len(), 1);
    let scratch = dir.path().join("scratch");
    let pending = scratch.join("create.pending");
    let seen = pending.clone();
    let seam = failpoint::at(SEAM_BEFORE_THE_MODE_STEP, move || {
        if seen.exists() {
            panic!("review_create_03: the scratch create stops here");
        }
    });
    let custody = file_root_custody(
        &dir.path().join("scratch-roots"),
        TestProtector::new(0x5a).0,
        root_id,
    );
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        EncryptedStore::create(&scratch, Box::new(custody), DEFAULT_RETENTION).is_ok()
    }));
    drop(seam);
    assert!(stopped.is_err());
    std::fs::copy(&pending, store_dir.join("create.pending")).unwrap();
    let key = store_dir.join("store.key");
    let original = std::fs::read(&key).unwrap();
    let mode = std::fs::metadata(&key).unwrap().permissions().mode();
    let mut json: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let version = json["version"]
        .as_u64()
        .expect("store.key has a numeric version");
    json["version"] = serde_json::Value::from(version + 1);
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&key, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    let answer = EncryptedStore::open_with(
        &store_dir,
        &roots_dir,
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| {
            Ok(Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    );
    println!(
        "CASE-NOTE: the open over the newer key file answered {}",
        answer_text(&answer)
    );
    drop(answer);
    std::fs::write(&key, &original).unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(mode)).unwrap();
    assert_eq!(
        root_files_in(&roots_dir),
        before,
        "an open that met a store.key of another version destroyed the root that it names"
    );
    assert!(
        open_over_file_root(&store_dir, &roots_dir, root_id).is_ok(),
        "the store no longer opens once its key file is back"
    );
}

#[cfg(unix)]
#[test]
fn a_create_answers_locked_before_it_clears_a_pending_create() {
    println!("CASE-NOTE: create runs clear_pending, which destroys the root that create.pending names, before Db::refuse_a_live_holder; its doc says Locked comes with nothing removed, so while another process holds store.db the pending root and create.pending should stay, and I expect both gone");
    let pending_root = RootId::fresh().unwrap();
    let killed = make_create_pending_naming(pending_root);
    let pending_file = killed.path().join("store").join("create.pending");
    let pending_root_file = killed
        .path()
        .join("roots")
        .join(format!("{pending_root}.root"));
    assert!(pending_file.exists() && pending_root_file.exists());

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let key = *store.store_key().unwrap().expose();
    drop(store);
    std::fs::write(dir.path().join("raw-key"), key).unwrap();
    std::fs::rename(
        store_dir.join("store.key"),
        dir.path().join("store.key-aside"),
    )
    .unwrap();
    std::fs::copy(&pending_file, store_dir.join("create.pending")).unwrap();
    std::fs::copy(
        &pending_root_file,
        roots_dir.join(format!("{pending_root}.root")),
    )
    .unwrap();

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::hold_the_database_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_TEST_HOLD_DATABASE", dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    wait_for(&dir.path().join("child-ready"), &mut child);
    let custody = file_root_custody(
        &roots_dir,
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
    let answer = answer_text(&created);
    drop(created);
    let roots_while_held = root_files_in(&roots_dir);
    let pending_while_held = store_dir.join("create.pending").exists();
    std::fs::write(dir.path().join("child-stop"), b"").unwrap();
    let _ = child.wait();
    println!("answer={answer} roots={roots_while_held:?} pending={pending_while_held}");
    assert_eq!(answer, "Locked");
    let mut expected = vec![format!("{root_id}.root"), format!("{pending_root}.root")];
    expected.sort();
    assert_eq!(
        roots_while_held, expected,
        "create removed the pending root before it answered Locked"
    );
    assert!(
        pending_while_held,
        "create removed create.pending before it answered Locked"
    );
}

/// A store directory and a roots directory left by a create that a child
/// process ended between its root and its key file: `store/create.pending`
/// names `pending_root`, and `roots/<pending_root>.root` stands.
#[cfg(unix)]
fn make_create_pending_naming(pending_root: RootId) -> tempfile::TempDir {
    let killed = tempfile::tempdir().unwrap();
    std::fs::write(killed.path().join("root-id"), pending_root.to_string()).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::kill_a_create_after_its_root_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HALVO_TEST_KILLED_CREATE_DIR", killed.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success(), "the child was not killed: {status:?}");
    assert!(killed.path().join("store").join("create.pending").exists());
    assert!(killed
        .path()
        .join("roots")
        .join(format!("{pending_root}.root"))
        .exists());
    killed
}

#[cfg(unix)]
struct RefusedOpen {
    answer: String,
    pending_root_kept: bool,
    pending_kept: bool,
    store_entries_kept: bool,
    roots_entries_kept: bool,
}

#[cfg(unix)]
fn default_file_root_custody(
    roots_dir: &Path,
    root_id: RootId,
) -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
    Ok(Box::new(file_root_custody(
        roots_dir,
        TestProtector::new(0x5a).0,
        root_id,
    )))
}

/// An open over a store whose directory also holds the `create.pending` of a
/// killed create, which names `pending_root` (another root than the store's,
/// whose root file is copied under the store's roots directory, unless the
/// case names the store's own root). `prepare` changes the store before the
/// open and returns what must live until the open has answered. The open
/// is `EncryptedStore::open_with` with the test's own roots directory.
#[cfg(unix)]
fn refused_open_case<G>(
    case: &str,
    pending_names_the_store_root: bool,
    prepare: impl FnOnce(&Path, &Path, &Path, RootId) -> G,
    custody_for: impl FnOnce(&Path, RootId) -> Result<Box<dyn KeyCustody + Send>, CustodyError>,
) -> RefusedOpen {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let pending_root = if pending_names_the_store_root {
        root_id
    } else {
        RootId::fresh().unwrap()
    };
    let killed = make_create_pending_naming(pending_root);
    std::fs::copy(
        killed.path().join("store").join("create.pending"),
        store_dir.join("create.pending"),
    )
    .unwrap();
    if !pending_names_the_store_root {
        std::fs::copy(
            killed
                .path()
                .join("roots")
                .join(format!("{pending_root}.root")),
            roots_dir.join(format!("{pending_root}.root")),
        )
        .unwrap();
    }
    let guard = prepare(dir.path(), &store_dir, &roots_dir, root_id);
    let pending_root_file = roots_dir.join(format!("{pending_root}.root"));
    let root_bytes = std::fs::read(&pending_root_file).ok();
    let pending_bytes = std::fs::read(store_dir.join("create.pending")).ok();
    let store_before = store_files_and_bytes(&store_dir);
    let roots_before = files_with_bytes(&roots_dir);
    let opened = EncryptedStore::open_with(
        &store_dir,
        &roots_dir,
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| custody_for(&roots_dir, root_id),
    );
    let answer = answer_text(&opened);
    drop(opened);
    let store_after = store_files_and_bytes(&store_dir);
    let roots_after = files_with_bytes(&roots_dir);
    drop(guard);
    let seen = RefusedOpen {
        answer,
        pending_root_kept: root_bytes.is_some()
            && std::fs::read(&pending_root_file).ok() == root_bytes,
        pending_kept: pending_bytes.is_some()
            && std::fs::read(store_dir.join("create.pending")).ok() == pending_bytes,
        store_entries_kept: store_after == store_before,
        roots_entries_kept: roots_after == roots_before,
    };
    println!(
        "REFUSED-OPEN case={case} answer={} pending_root_kept={} pending_kept={} \
         store_entries_kept={} roots_entries_kept={}",
        seen.answer,
        seen.pending_root_kept,
        seen.pending_kept,
        seen.store_entries_kept,
        seen.roots_entries_kept
    );
    for (name, bytes) in &store_after {
        println!(
            "REFUSED-OPEN case={case} store entry {name} len={}",
            bytes.len()
        );
    }
    for (name, bytes) in &roots_after {
        println!(
            "REFUSED-OPEN case={case} roots entry {name} len={}",
            bytes.len()
        );
    }
    seen
}

/// The files of a store directory with their bytes, without the lock file
/// that an open on Windows makes and leaves.
#[cfg(unix)]
fn store_files_and_bytes(store_dir: &Path) -> Vec<(String, Vec<u8>)> {
    files_with_bytes(store_dir)
        .into_iter()
        .filter(|(name, _)| !LOCK_FILES.contains(&name.as_str()))
        .collect()
}

#[cfg(unix)]
fn assert_the_refused_open_wrote_nothing(seen: &RefusedOpen, expected: &str) {
    assert_eq!(seen.answer, expected);
    assert!(
        seen.pending_root_kept,
        "the refused open destroyed or changed the root file that create.pending names"
    );
    assert!(
        seen.pending_kept,
        "the refused open removed or changed create.pending"
    );
    assert!(
        seen.store_entries_kept,
        "the refused open changed an entry of the store directory"
    );
    assert!(
        seen.roots_entries_kept,
        "the refused open changed an entry under the roots directory"
    );
}

#[cfg(unix)]
#[test]
fn a_refused_open_of_a_key_file_of_version_3_keeps_the_pending_root_and_create_pending() {
    let seen = refused_open_case(
        "version 3",
        false,
        |_, store_dir, _, _| edit_key_file(store_dir, |v| v["version"] = 3.into()),
        default_file_root_custody,
    );
    assert_the_refused_open_wrote_nothing(&seen, "NewerStore { found: 3, supported: 2 }");
}

#[cfg(unix)]
#[test]
fn a_refused_open_of_a_key_file_of_version_1_keeps_the_pending_root_and_create_pending() {
    let seen = refused_open_case(
        "version 1",
        false,
        |_, store_dir, _, _| edit_key_file(store_dir, |v| v["version"] = 1.into()),
        default_file_root_custody,
    );
    assert_the_refused_open_wrote_nothing(&seen, "OlderStore { found: 1, supported: 2 }");
}

#[cfg(unix)]
#[test]
fn a_refused_open_of_a_key_file_of_another_label_keeps_the_pending_root_and_create_pending() {
    let seen = refused_open_case(
        "another label",
        false,
        |_, store_dir, _, _| edit_key_file(store_dir, |v| v["namespace"] = "derived".into()),
        default_file_root_custody,
    );
    assert_the_refused_open_wrote_nothing(&seen, "KeyFileOfAnotherLabel");
}

#[cfg(unix)]
#[test]
fn a_refused_open_of_a_key_file_that_does_not_parse_keeps_the_pending_root_and_create_pending() {
    let seen = refused_open_case(
        "unparseable with a root id",
        false,
        |_, store_dir, _, _| edit_key_file(store_dir, |v| v["wrapped_key"] = "AAA".into()),
        default_file_root_custody,
    );
    assert_the_refused_open_wrote_nothing(&seen, "KeyFileUnparseable");
}

#[cfg(unix)]
#[test]
fn a_refused_open_with_no_store_db_keeps_the_pending_root_and_create_pending() {
    let seen = refused_open_case(
        "store.db missing",
        false,
        |dir, store_dir, _, _| {
            std::fs::rename(store_dir.join("store.db"), dir.join("store.db-aside")).unwrap();
        },
        default_file_root_custody,
    );
    assert_the_refused_open_wrote_nothing(&seen, "DatabaseMissing");
}

#[cfg(unix)]
#[test]
fn a_refused_open_at_the_custody_keeps_the_pending_root_and_create_pending() {
    let seen = refused_open_case(
        "custody refusal",
        false,
        |_, _, _, _| (),
        |_, _| Err(CustodyError::RootDenied),
    );
    assert_the_refused_open_wrote_nothing(&seen, "Custody(RootDenied)");
}

#[cfg(unix)]
#[test]
fn a_refused_open_in_the_database_keeps_the_pending_root_and_create_pending() {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    let seen = refused_open_case(
        "database unreadable",
        false,
        |_, store_dir, roots_dir, root_id| {
            let custody = file_root_custody(roots_dir, TestProtector::new(0x5a).0, root_id);
            let another = custody
                .wrap_store_key(
                    &StoreKey::random().unwrap(),
                    crate::custody::KeyLabel::STORE,
                )
                .unwrap();
            edit_key_file(store_dir, |v| {
                v["wrapped_key"] = STANDARD.encode(&another.0).into();
            });
        },
        default_file_root_custody,
    );
    assert_the_refused_open_wrote_nothing(&seen, "DatabaseUnreadable");
}

#[cfg(unix)]
struct DatabaseHolder {
    child: Child,
    stop: PathBuf,
}

#[cfg(unix)]
impl Drop for DatabaseHolder {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.stop, b"");
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
#[test]
fn a_refused_open_while_another_process_holds_the_database_keeps_the_pending_root_and_create_pending(
) {
    let seen = refused_open_case(
        "locked by a holder",
        false,
        |dir, store_dir, roots_dir, root_id| {
            let aside = dir.join("create.pending-aside");
            std::fs::rename(store_dir.join("create.pending"), &aside).unwrap();
            let store = EncryptedStore::open_with(
                store_dir,
                &dir.join("no-roots"),
                DEFAULT_RETENTION,
                |_: &crate::custody::CustodyLocator| default_file_root_custody(roots_dir, root_id),
            )
            .unwrap();
            let key = *store.store_key().unwrap().expose();
            drop(store);
            std::fs::rename(&aside, store_dir.join("create.pending")).unwrap();
            std::fs::write(dir.join("raw-key"), key).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "store::hardening_tests::hold_the_database_for_parent",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("HALVO_TEST_HOLD_DATABASE", dir)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            wait_for(&dir.join("child-ready"), &mut child);
            DatabaseHolder {
                child,
                stop: dir.join("child-stop"),
            }
        },
        default_file_root_custody,
    );
    assert_the_refused_open_wrote_nothing(&seen, "Locked");
}

#[cfg(unix)]
#[test]
fn a_refused_open_of_version_3_beside_a_create_pending_of_its_own_root_keeps_both() {
    let seen = refused_open_case(
        "own root beside version 3",
        true,
        |_, store_dir, _, _| edit_key_file(store_dir, |v| v["version"] = 3.into()),
        default_file_root_custody,
    );
    assert_the_refused_open_wrote_nothing(&seen, "NewerStore { found: 3, supported: 2 }");
}

#[cfg(unix)]
#[test]
fn a_refused_open_of_a_stopped_create_answers_sealed_and_the_next_create_clears_it() {
    let pending_root = RootId::fresh().unwrap();
    let killed = make_create_pending_naming(pending_root);
    let (store_dir, roots_dir) = (killed.path().join("store"), killed.path().join("roots"));
    let pending_root_file = roots_dir.join(format!("{pending_root}.root"));
    let root_bytes = std::fs::read(&pending_root_file).unwrap();
    let pending_bytes = std::fs::read(store_dir.join("create.pending")).unwrap();
    let store_before = store_files_and_bytes(&store_dir);
    let roots_before = files_with_bytes(&roots_dir);
    let opened = EncryptedStore::open_with(
        &store_dir,
        &roots_dir,
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| default_file_root_custody(&roots_dir, pending_root),
    );
    let answer = answer_text(&opened);
    drop(opened);
    let seen = RefusedOpen {
        answer,
        pending_root_kept: std::fs::read(&pending_root_file).ok() == Some(root_bytes),
        pending_kept: std::fs::read(store_dir.join("create.pending")).ok() == Some(pending_bytes),
        store_entries_kept: store_files_and_bytes(&store_dir) == store_before,
        roots_entries_kept: files_with_bytes(&roots_dir) == roots_before,
    };
    println!(
        "REFUSED-OPEN case=no key file answer={} pending_root_kept={} pending_kept={} \
         store_entries_kept={} roots_entries_kept={}",
        seen.answer,
        seen.pending_root_kept,
        seen.pending_kept,
        seen.store_entries_kept,
        seen.roots_entries_kept
    );
    assert_the_refused_open_wrote_nothing(&seen, "Sealed");
    let custody = file_root_custody(
        &roots_dir,
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let fresh_root = custody.root_id;
    let created = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
    assert!(created.is_ok(), "{:?}", created.err());
    assert_eq!(
        root_files_in(&roots_dir),
        vec![format!("{fresh_root}.root")]
    );
    assert!(!store_dir.join("create.pending").exists());
}

/// An open that succeeds over a store whose directory holds the
/// `create.pending` of a killed create: what stands after it.
#[cfg(unix)]
struct OpenAfterAStoppedCreate {
    store_root_bytes_kept: bool,
    pending_root_file_left: bool,
    pending_left: bool,
}

#[cfg(unix)]
fn open_after_a_stopped_create(pending_names_the_store_root: bool) -> OpenAfterAStoppedCreate {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let pending_root = if pending_names_the_store_root {
        root_id
    } else {
        RootId::fresh().unwrap()
    };
    let killed = make_create_pending_naming(pending_root);
    std::fs::copy(
        killed.path().join("store").join("create.pending"),
        store_dir.join("create.pending"),
    )
    .unwrap();
    if !pending_names_the_store_root {
        std::fs::copy(
            killed
                .path()
                .join("roots")
                .join(format!("{pending_root}.root")),
            roots_dir.join(format!("{pending_root}.root")),
        )
        .unwrap();
    }
    let store_root_file = roots_dir.join(format!("{root_id}.root"));
    let store_root_bytes = std::fs::read(&store_root_file).unwrap();
    let opened = EncryptedStore::open_with(
        &store_dir,
        &roots_dir,
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| default_file_root_custody(&roots_dir, root_id),
    );
    assert!(opened.is_ok(), "{:?}", opened.err());
    drop(opened);
    OpenAfterAStoppedCreate {
        store_root_bytes_kept: std::fs::read(&store_root_file).ok() == Some(store_root_bytes),
        pending_root_file_left: !pending_names_the_store_root
            && roots_dir.join(format!("{pending_root}.root")).exists(),
        pending_left: store_dir.join("create.pending").exists(),
    }
}

#[cfg(unix)]
#[test]
fn an_open_beside_a_create_pending_of_its_own_root_keeps_the_root_and_removes_create_pending() {
    let seen = open_after_a_stopped_create(true);
    assert!(seen.store_root_bytes_kept);
    assert!(!seen.pending_left, "create.pending stayed after the open");
}

#[cfg(unix)]
#[test]
fn an_open_beside_a_create_pending_of_another_root_destroys_that_root_and_keeps_its_own() {
    let seen = open_after_a_stopped_create(false);
    assert!(seen.store_root_bytes_kept);
    assert!(
        !seen.pending_root_file_left,
        "the root that create.pending names stayed"
    );
    assert!(!seen.pending_left, "create.pending stayed after the open");
}

#[cfg(unix)]
#[test]
fn an_open_whose_cleanup_of_a_stopped_create_fails_answers_ok_and_delete_all_clears_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let pending_root = RootId::fresh().unwrap();
    let killed = make_create_pending_naming(pending_root);
    std::fs::copy(
        killed.path().join("store").join("create.pending"),
        store_dir.join("create.pending"),
    )
    .unwrap();
    let pending_root_file = roots_dir.join(format!("{pending_root}.root"));
    std::fs::copy(
        killed
            .path()
            .join("roots")
            .join(format!("{pending_root}.root")),
        &pending_root_file,
    )
    .unwrap();
    std::fs::set_permissions(&pending_root_file, std::fs::Permissions::from_mode(0o000)).unwrap();
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let opened = EncryptedStore::open_with(
        &store_dir,
        &roots_dir,
        DEFAULT_RETENTION,
        |_: &crate::custody::CustodyLocator| default_file_root_custody(&roots_dir, root_id),
    );
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(&pending_root_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut store = opened.unwrap();
    let flag = store.pending_left();
    let root_stands = pending_root_file.exists();
    let pending_stands = store_dir.join("create.pending").exists();
    println!(
        "CLEANUP-FAILED pending_left={flag} root_stands={root_stands} pending_stands={pending_stands}"
    );
    assert!(flag, "the handle does not say that the cleanup failed");
    assert!(
        root_stands && pending_stands,
        "the leftover of the stopped create is gone"
    );
    let deleted = store.delete_all();
    assert!(deleted.is_ok(), "{deleted:?}");
    drop(store);
    assert!(
        !store_dir.join("create.pending").exists(),
        "delete_all left create.pending"
    );
    assert!(!store_dir.join("create.pending.tmp").exists());
    assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn an_open_answers_a_fifo_or_a_link_at_create_pending_and_a_fifo_at_its_root_within_a_bound() {
    for case in [
        "fifo at create.pending",
        "link at create.pending",
        "fifo at the pending root",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        let outside = dir.path().join("outside");
        let pending_root = RootId::fresh().unwrap();
        let killed = make_create_pending_naming(pending_root);
        let fifo = match case {
            "fifo at create.pending" => {
                let fifo = store_dir.join("create.pending");
                make_fifo(&fifo);
                fifo
            }
            "link at create.pending" => {
                std::fs::copy(killed.path().join("store").join("create.pending"), &outside)
                    .unwrap();
                std::os::unix::fs::symlink(&outside, store_dir.join("create.pending")).unwrap();
                outside.clone()
            }
            _ => {
                std::fs::copy(
                    killed.path().join("store").join("create.pending"),
                    store_dir.join("create.pending"),
                )
                .unwrap();
                let fifo = roots_dir.join(format!("{pending_root}.root"));
                make_fifo(&fifo);
                fifo
            }
        };
        let outside_before = std::fs::read(&outside).ok();
        let (store_path, roots_path) = (store_dir.clone(), roots_dir.clone());
        let opened = answer_within(FIFO_BOUND, &fifo, move || {
            answer_text(&EncryptedStore::open_with(
                &store_path,
                &roots_path,
                DEFAULT_RETENTION,
                |_: &crate::custody::CustodyLocator| {
                    default_file_root_custody(&roots_path, root_id)
                },
            ))
        });
        println!("PENDING-ENTRY case={case} answer={opened:?}");
        assert_eq!(
            opened.as_deref(),
            Some("Ok"),
            "{case}: no answer within {FIFO_BOUND:?}, or another one"
        );
        assert_eq!(
            std::fs::read(&outside).ok(),
            outside_before,
            "{case}: the link's target changed"
        );
    }
}

#[test]
fn the_live_holder_probe_and_the_held_look_answer_a_code_by_its_class_and_never_a_bare_code() {
    use super::db::code_seams;
    let mut places = vec![
        ("open", code_seams::PROBE_TIMEOUT),
        ("open", code_seams::PROBE_LOCKING_MODE),
    ];
    if cfg!(target_os = "macos") {
        places.push(("open", code_seams::HELD_LOOK_HAS_MOVED));
        places.push(("create", code_seams::HELD_LOOK_HAS_MOVED));
    }
    let codes = [
        (11, "DatabaseUnreadable"),
        (26, "DatabaseUnreadable"),
        (13, "Io(StorageFull)"),
        (5, "Locked"),
        (6, "Locked"),
        (1, "Io(Other)"),
        (10 | (3 << 8), "Io(Other)"),
    ];
    for (call, place) in places {
        for (code, expected) in codes {
            let dir = tempfile::tempdir().unwrap();
            let roots_dir = dir.path().join("roots");
            let answer = if call == "open" {
                let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
                code_seams::force_at(place, Some(code));
                let opened = EncryptedStore::open_with(
                    &store_dir,
                    &roots_dir,
                    DEFAULT_RETENTION,
                    |_: &crate::custody::CustodyLocator| {
                        Ok(Box::new(file_root_custody(
                            &roots_dir,
                            TestProtector::new(0x5a).0,
                            root_id,
                        )))
                    },
                );
                answer_text(&opened)
            } else {
                let custody = file_root_custody(
                    &roots_dir,
                    TestProtector::new(0x5a).0,
                    RootId::fresh().unwrap(),
                );
                code_seams::force_at(place, Some(code));
                let created = EncryptedStore::create(
                    &dir.path().join("store"),
                    Box::new(custody),
                    DEFAULT_RETENTION,
                );
                answer_text(&created)
            };
            code_seams::force_at(place, None);
            println!("CODE-AT call={call} place={place} code={code} answer={answer}");
            assert_eq!(
                answer, expected,
                "{call} answered code {code} at {place} as {answer}"
            );
        }
    }
}

#[test]
fn a_delete_report_and_its_roots_left_print_each_root_id_in_its_one_text_form() {
    let (left, not_handled) = (RootId::fresh().unwrap(), RootId::fresh().unwrap());
    let report = super::DeleteReport {
        keyed_segments: None,
        root: super::RootReading::Left(Some(left)),
        roots: super::RootsLeft {
            left: vec![left],
            names: vec![format!("{left}.root")],
            listed: true,
        },
        root_not_handled: Some(not_handled),
        key_file_gone: true,
        database_left: false,
        files_left: 0,
        not_followed: 0,
    };
    let printed = format!("{report:?} {:?} {:?}", report.roots, left);
    println!("REPORT-DEBUG {printed}");
    for id in [left, not_handled] {
        let text = id.to_string();
        assert_eq!(text.len(), 32);
        assert!(printed.contains(&text), "{printed} does not hold {text}");
        let hyphenated = uuid::Uuid::parse_str(&text)
            .unwrap()
            .hyphenated()
            .to_string();
        assert!(
            !printed.contains(&hyphenated),
            "{printed} holds the hyphenated {hyphenated}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_key_file_place_refuses_a_link_at_its_parents_name_and_reads_nothing_behind_it() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("store.key"), b"{\"version\":2}").unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join("link")).unwrap();
    let place = KeyFile::at(&dir.path().join("link").join("store.key"));
    let answer = place.map(drop).map_err(|e| format!("{e:?}"));
    println!("KEY-FILE-AT link answer={answer:?}");
    assert_eq!(
        answer,
        Err("Obstructed { name: StoreDirectory, met: Link }".to_string())
    );
    assert_eq!(
        std::fs::read(real.join("store.key")).unwrap(),
        b"{\"version\":2}"
    );
}

#[cfg(unix)]
#[test]
fn delete_all_unlinks_a_link_at_one_database_side_file_and_still_removes_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    store
        .write_segment(&mut Cursor::new(b"before the link"))
        .unwrap();
    let outside = dir.path().join("outside");
    std::fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, store_dir.join("store.db-journal")).unwrap();
    let report = match store.delete_all() {
        Ok(report) => report,
        Err(StoreError::DeleteIncomplete(report)) => *report,
        Err(e) => panic!("delete_all answered {e:?}"),
    };
    println!("SIDE-FILE-LINK report={report:?}");
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
    for name in ["store.db", "store.db-wal", "store.db-journal"] {
        assert!(
            std::fs::symlink_metadata(store_dir.join(name)).is_err(),
            "{name} stands after delete_all"
        );
    }
    assert!(!report.database_left);
    assert_eq!(report.not_followed, 1);
}

#[cfg(unix)]
#[test]
fn an_open_whose_log_is_unlinked_before_its_first_write_answers_obstructed_at_the_second_instant() {
    use super::db::SEAM_AT_THE_LOG_CHECK;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let act = {
        let store_dir = store_dir.clone();
        move || {
            let log = store_dir.join("store.db-wal");
            if log.exists() {
                std::fs::remove_file(&log).unwrap();
            }
        }
    };
    let seam = super::failpoint::at(SEAM_AT_THE_LOG_CHECK, nth_call(1, act));
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    drop(seam);
    println!("LOG-UNLINKED answer={opened}");
    assert_eq!(
        opened,
        "Obstructed { name: Log, met: DatabaseOutsideTheDirectory }"
    );
}

#[cfg(unix)]
#[test]
fn a_create_whose_database_is_cut_to_nothing_before_the_page_look_answers_obstructed() {
    use super::db::SEAM_BEFORE_THE_PAGE_LOOK;
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let act = {
        let store_db = store_dir.join("store.db");
        move || {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&store_db)
                .unwrap()
                .set_len(0)
                .unwrap();
        }
    };
    let seam = super::failpoint::at(SEAM_BEFORE_THE_PAGE_LOOK, nth_call(1, act));
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let created = answer_text(&EncryptedStore::create(
        &store_dir,
        Box::new(custody),
        DEFAULT_RETENTION,
    ));
    drop(seam);
    println!("PAGE-LOOK answer={created}");
    assert_eq!(
        created,
        "Obstructed { name: Database, met: DatabaseOutsideTheDirectory }"
    );
    assert_eq!(
        root_files_in(&dir.path().join("roots")),
        Vec::<String>::new()
    );
}

#[cfg(unix)]
#[test]
fn the_answer_of_a_handle_takes_a_gone_directory_code_as_obstructed_only_when_the_name_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let inputs = || {
        [
            StoreError::Db(halvo_sqlcipher::rusqlite::ffi::SQLITE_IOERR_FSTAT),
            StoreError::Db(halvo_sqlcipher::rusqlite::ffi::SQLITE_CANTOPEN),
            StoreError::SegmentMissing,
            StoreError::Db(11),
        ]
    };
    let named: Vec<String> = inputs()
        .into_iter()
        .map(|e| format!("{:?}", store.answer(e)))
        .collect();
    std::fs::rename(&store_dir, dir.path().join("moved")).unwrap();
    let gone: Vec<String> = inputs()
        .into_iter()
        .map(|e| format!("{:?}", store.answer(e)))
        .collect();
    println!("ANSWER named={named:?} gone={gone:?}");
    assert_eq!(named, ["Db(1802)", "Db(14)", "SegmentMissing", "Db(11)"]);
    let moved = "Obstructed { name: StoreDirectory, met: DirectoryNotAtItsName }";
    assert_eq!(gone, [moved, moved, moved, "Db(11)"]);
}

#[cfg(unix)]
struct RootsWithAFailingDestroy {
    inner: FileRootCustody<TestProtector>,
    roots: PathBuf,
}

#[cfg(unix)]
impl KeyCustody for RootsWithAFailingDestroy {
    fn kind(&self) -> CustodyKind {
        self.inner.kind()
    }
    fn root_id(&self) -> RootId {
        self.inner.root_id()
    }
    fn roots(&self) -> Option<&std::path::Path> {
        Some(&self.roots)
    }
    fn wrap_store_key(
        &self,
        key: &StoreKey,
        label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        self.inner.wrap_store_key(key, label)
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.inner.create_root()
    }
    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        label: crate::custody::KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        self.inner.unwrap_store_key(wrapped, label)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        Err(CustodyError::RootDenied)
    }
}

#[cfg(unix)]
#[test]
fn a_root_step_whose_custody_destroy_fails_still_destroys_every_root_file_under_its_roots() {
    let dir = tempfile::tempdir().unwrap();
    let roots_dir = dir.path().join("roots");
    let custody = RootsWithAFailingDestroy {
        inner: file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            RootId::fresh().unwrap(),
        ),
        roots: roots_dir.clone(),
    };
    let mut store = EncryptedStore::create(
        &dir.path().join("store"),
        Box::new(custody),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let other = RootId::fresh().unwrap();
    std::fs::write(roots_dir.join(format!("{other}.root")), [9u8; 40]).unwrap();
    let answer = answer_text(&store.delete_all());
    println!(
        "ROOT-STEP answer={answer} left={:?}",
        root_files_in(&roots_dir)
    );
    assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn the_size_of_a_store_counts_nothing_behind_a_link_to_a_large_file() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let before = store.size_bytes().unwrap();
    let big = dir.path().join("big");
    std::fs::write(&big, vec![1u8; 1 << 20]).unwrap();
    std::os::unix::fs::symlink(&big, store_dir.join("a-link-to-a-large-file")).unwrap();
    let after = store.size_bytes().unwrap();
    println!("SIZE before={before} after={after}");
    assert_eq!(after, before);
}

#[cfg(unix)]
#[test]
fn delete_all_unlinks_a_link_to_a_directory_in_segments_and_counts_it_as_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("kept"), b"kept").unwrap();
    std::os::unix::fs::symlink(&outside, store_dir.join("segments").join("zz")).unwrap();
    let report = match store.delete_all() {
        Ok(report) => report,
        Err(StoreError::DeleteIncomplete(report)) => *report,
        Err(e) => panic!("delete_all answered {e:?}"),
    };
    println!("SEGMENTS-LINK report={report:?}");
    assert_eq!(std::fs::read(outside.join("kept")).unwrap(), b"kept");
    assert!(std::fs::symlink_metadata(store_dir.join("segments")).is_err());
    assert_eq!(report.not_followed, 1);
}

#[cfg(unix)]
#[test]
fn a_sweep_whose_first_removal_fails_still_removes_the_other_keyless_file() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let paths: Vec<PathBuf> = (0..2)
        .map(|_| {
            let (segment, _) = store
                .write_segment_with_expiry(&mut Cursor::new(b"expired"), 1, 2)
                .unwrap();
            segment_path(&store_dir, segment.segment_id)
        })
        .collect();
    super::failpoint::arm("segment_file_remove");
    let shred = answer_text(&store.crypto_shred_expired());
    let left = paths.iter().filter(|path| path.exists()).count();
    println!("SWEEP shred={shred} left={left}");
    assert_eq!(left, 1, "the sweep left {left} of the two keyless files");
}

#[cfg(unix)]
#[test]
fn removing_a_written_segment_through_a_shard_that_is_a_link_removes_nothing_and_answers_ok() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let segment_id = Uuid::from_bytes([0xab; 16]);
    let file = segment_path(&store_dir, segment_id);
    let shard = file.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(shard.parent().unwrap()).unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let behind = outside.join(file.file_name().unwrap());
    std::fs::write(&behind, b"behind the link").unwrap();
    std::os::unix::fs::symlink(&outside, &shard).unwrap();
    let handle = super::durable::Dir::open(&store_dir).unwrap();
    let removed =
        segment::remove_written_segment(&handle, segment_id).map_err(|e| format!("{e:?}"));
    println!("WRITTEN-SEGMENT answer={removed:?}");
    assert_eq!(removed, Ok(()));
    assert_eq!(std::fs::read(&behind).unwrap(), b"behind the link");
}

#[cfg(unix)]
#[test]
fn a_custody_made_before_its_root_guard_shares_the_guards_roots_directory() {
    let root_id = RootId::fresh().unwrap();
    let custody = crate::store::roots::custody(root_id);
    let guard = RootGuard::new(root_id);
    assert_eq!(custody.roots(), Some(guard.roots().as_path()));
}

#[cfg(unix)]
mod closed_list;

#[cfg(unix)]
mod pass_two;

#[cfg(windows)]
mod closed_list_windows;
