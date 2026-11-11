//! Regression tests for the adversarial review of the encrypted store: each test
//! reproduces one confirmed finding and fails on the store before its fix.

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

/// The wrapped key for `segment_id` as seen in a copy of `store.db` alone (no
/// WAL), read with the store key: what a snapshot of the main file leaks.
fn wrapped_key_in_main_file(dir: &Path, store_key: &[u8; 32], segment_id: Uuid) -> Option<Vec<u8>> {
    let copy = tempfile::tempdir().unwrap();
    std::fs::copy(dir.join("store.db"), copy.path().join("store.db")).unwrap();
    let conn = Connection::open(copy.path().join("store.db")).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", hex(store_key)))
        .unwrap();
    conn.query_row(
        "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
        [segment_id.simple().to_string()],
        |r| r.get::<_, Option<Vec<u8>>>(0),
    )
    .unwrap_or(None)
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

/// Custody over the real dev-fallback root whose `destroy_root` fails the first
/// `failures` times and counts every call.
struct FlakyCustody {
    inner: DevFallbackCustody,
    failures: AtomicUsize,
    destroy_calls: Arc<AtomicUsize>,
}

impl FlakyCustody {
    fn new(root_id: RootId, failures: usize) -> (Self, Arc<AtomicUsize>) {
        let destroy_calls = Arc::new(AtomicUsize::new(0));
        (
            Self {
                inner: DevFallbackCustody::new(root_id),
                failures: AtomicUsize::new(failures),
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
    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        self.inner.unwrap_store_key(wrapped)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.destroy_calls.fetch_add(1, Ordering::SeqCst);
        let remaining = self.failures.load(Ordering::SeqCst);
        if remaining > 0 {
            self.failures.store(remaining - 1, Ordering::SeqCst);
            return Err(CustodyError::RootLocked(-25308));
        }
        self.inner.destroy_root()
    }
}

/// Finding HIGH-1 (adversary-1 #3, adversary-2 #2): a reader holding a snapshot
/// must not be able to keep a shredded key alive, and the shred must leave
/// nothing recoverable in store.db or its WAL.
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
    let from_main_file = wrapped_key_in_main_file(dir.path(), &store_key, expired.segment_id);
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

/// Finding MEDIUM-4 (pr-review #6): the single-writer invariant must hold even
/// after the writer lock file is unlinked out from under the first writer.
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

/// The child half of the two-process lock test: opens the store named by
/// `HALVO_TEST_HOLD_STORE` and holds it until told to stop. A no-op otherwise.
#[test]
fn hold_store_for_parent() {
    let Ok(dir) = std::env::var("HALVO_TEST_HOLD_STORE") else {
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

/// Finding MEDIUM-5 (adversary-3 F1): a transient destroy_root failure must leave
/// delete-all retryable, and once it succeeds a backup of store.key must not
/// reopen the store.
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

/// Finding MEDIUM-5 (adversary-2 #5, pr-review #10): a create that fails must
/// destroy the root it made and leave no half store behind.
#[test]
fn a_failed_create_leaves_no_root_and_no_half_store() {
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

/// Same finding, failing after the root exists: the wrap succeeded (so the root
/// was created) but the key file could not be written.
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

/// A custody whose wrap creates the real root and then fails.
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
    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        self.0.unwrap_store_key(wrapped)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.0.destroy_root()
    }
}

/// Finding MEDIUM-5 (adversary-2 #1, pr-review #7): when a segment file cannot be
/// removed, its key must still be destroyed everywhere, and the next pass must
/// remove the orphan.
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
    std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o500)).unwrap();

    let first = store.crypto_shred_expired();

    let from_main_file = wrapped_key_in_main_file(dir.path(), &store_key, expired.segment_id);
    std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(path.exists(), "the fixture did not block the removal");
    assert!(
        first.is_err(),
        "the pass reported success with the file still present"
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

/// Finding MEDIUM-5 (pr-review #1): delete-all must leave a directory a fresh
/// create can use.
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

/// Finding MEDIUM-6 (adversary-1 #4): a write that fails mid-stream must commit
/// no key and leave no readable file.
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

/// Finding LOW-7 (adversary-1 #5, adversary-3 F4): a truncated or tail-tampered
/// segment yields no plaintext at all, and the error names corruption, not a
/// sealed store.
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

/// Finding LOW-8 (adversary-1 #6, adversary-3 F5): a ciphertext file with no live
/// key row, and a stray temp file, are removed by the next shred pass.
#[test]
fn orphan_segment_files_are_reconciled() {
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

/// Finding LOW-9 (pr-review #2 and #4): a wrong store key is a sealed store; a
/// shredded segment is sealed; tampered data is corrupt.
#[test]
fn a_wrong_store_key_opens_as_sealed_and_a_shredded_segment_reads_as_sealed() {
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
        matches!(opened, Err(StoreError::Sealed)),
        "a wrong store key opened as {:?}",
        opened.err()
    );
}

/// Yields `remaining` bytes of a fixed pattern without ever holding them.
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

/// The memory bound: the peak resident set during a 512 MiB segment write and
/// read must stay within 64 MiB of the resident set before the write. Sampled
/// every 20 ms with `ps -o rss=`; run with
/// `cargo test --release -p halvo-capture --lib peak_rss -- --ignored --nocapture`.
#[cfg(unix)]
#[test]
#[ignore]
fn peak_rss_stays_bounded_for_a_512_mib_segment() {
    const SEGMENT_BYTES: u64 = 512 * 1024 * 1024;
    const BOUND_KIB: u64 = 64 * 1024;

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
                std::thread::sleep(Duration::from_millis(20));
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
    assert!(matches!(read, Err(StoreError::Io(_))), "{read:?}");
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
