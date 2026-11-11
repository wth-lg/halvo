//! Formal checks of the store's two safety properties, run against the real
//! store rather than a model of it. `docs/verify/store-shred.md` records the
//! properties, the covered code, the bounds and the command.
//!
//! 1. Crypto-shred: after a segment is shredded, no key remaining in the store
//!    decrypts its ciphertext, over random sequences of writes, shreds,
//!    delete-alls, reopens and recreates.
//! 2. Single writer: at most one process holds the store at a time, over random
//!    races of contending processes, with the lock file unlinked under them.

use std::fs::OpenOptions;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use halvo_sqlcipher::rusqlite::Connection;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{custody_for, KeyCustody, RootId};

use super::db::{unix_secs_now, EncryptedStore, KeyFile};
use super::segment::{self, segment_path, SegmentKey, CHUNK};
use super::{SegmentExpiry, SegmentStore, StoreError, DEFAULT_RETENTION};

/// Random operation sequences, each ending in a full invariant check. Every
/// case creates a store with a real dev-fallback root, so the count is kept to
/// what runs in well under a minute; `PROPTEST_CASES` raises it.
const SHRED_CASES: u32 = 48;
/// The longest sequence of store operations one case replays. Each write adds
/// one segment, so this also bounds the segment count.
const MAX_OPS: usize = 12;

/// Random multi-process races. Each case spawns up to [`MAX_CONTENDERS`]
/// copies of the test binary, so the count is small and shrinking is short.
const RACE_CASES: u32 = 10;
const MAX_CONTENDERS: usize = 5;
const MAX_ACQUISITIONS: usize = 3;

const CONTEND_DIR: &str = "HALVO_FORMAL_CONTEND_DIR";
const CONTEND_INDEX: &str = "HALVO_FORMAL_CONTEND_INDEX";
const CONTEND_ACQUISITIONS: &str = "HALVO_FORMAL_CONTEND_ACQUISITIONS";
const CONTEND_HOLD_MS: &str = "HALVO_FORMAL_CONTEND_HOLD_MS";
const CONTEND_DELAY_MS: &str = "HALVO_FORMAL_CONTEND_DELAY_MS";

fn shred_config() -> ProptestConfig {
    ProptestConfig {
        cases: SHRED_CASES,
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..ProptestConfig::default()
    }
}

fn race_config() -> ProptestConfig {
    ProptestConfig {
        cases: RACE_CASES,
        max_shrink_iters: 24,
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..ProptestConfig::default()
    }
}

// ---------------------------------------------------------------------------
// Property 1: crypto-shred non-decryptability.
// ---------------------------------------------------------------------------

/// When a written segment expires, relative to the clock at the time of the
/// write. `AtWrite` sits on the `expires_at <= now` boundary of
/// `crypto_shred_expired`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expiry {
    Past,
    AtWrite,
    Future,
}

impl Expiry {
    fn expires_at(self, now: u64) -> u64 {
        match self {
            Expiry::Past => 1,
            Expiry::AtWrite => now,
            Expiry::Future => now + DEFAULT_RETENTION.as_secs(),
        }
    }
}

#[derive(Debug, Clone)]
enum Op {
    Write { len: usize, expiry: Expiry },
    ShredExpired,
    ShredAll,
    Reopen,
    Recreate,
}

fn segment_len() -> impl Strategy<Value = usize> {
    prop_oneof![
        3 => 0usize..=512,
        1 => (CHUNK - 1)..=(CHUNK + 1),
        1 => Just(CHUNK * 2 + 7),
    ]
}

fn expiry() -> impl Strategy<Value = Expiry> {
    prop_oneof![
        Just(Expiry::Past),
        Just(Expiry::AtWrite),
        Just(Expiry::Future)
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        5 => (segment_len(), expiry()).prop_map(|(len, expiry)| Op::Write { len, expiry }),
        2 => Just(Op::ShredExpired),
        1 => Just(Op::ShredAll),
        1 => Just(Op::Reopen),
        1 => Just(Op::Recreate),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Live,
    Shredded,
}

struct ModelSegment {
    id: Uuid,
    plaintext: Vec<u8>,
    cipher: Vec<u8>,
    expires_at: u64,
    fate: Fate,
}

/// The real store under test next to the model the harness keeps of it.
struct Harness {
    dir: PathBuf,
    roots: Vec<RootId>,
    store: Option<EncryptedStore>,
    /// Delete-all ran and no store has been created in the directory since.
    deleted: bool,
    /// The store-key file as written by the latest create, kept to show that
    /// delete-all destroys the root that unwraps it.
    key_file: Option<KeyFile>,
    segments: Vec<ModelSegment>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.store = None;
        for root in &self.roots {
            DevFallbackCustody::new(*root)
                .destroy_root()
                .expect("the harness's dev-fallback root is destroyed when the case ends");
        }
    }
}

impl Harness {
    fn create(dir: &Path) -> Self {
        let mut harness = Self {
            dir: dir.to_path_buf(),
            roots: Vec::new(),
            store: None,
            deleted: false,
            key_file: None,
            segments: Vec::new(),
        };
        harness.create_store();
        harness
    }

    fn create_store(&mut self) {
        let root = RootId::fresh();
        self.roots.push(root);
        let store = EncryptedStore::create(
            &self.dir,
            Box::new(DevFallbackCustody::new(root)),
            DEFAULT_RETENTION,
        )
        .expect("create on a free directory");
        self.key_file = Some(KeyFile::read(&self.dir).expect("the fresh store's key file"));
        self.store = Some(store);
        self.deleted = false;
    }

    fn open_store(&self) -> Option<&EncryptedStore> {
        if self.deleted {
            None
        } else {
            self.store.as_ref()
        }
    }

    fn apply(&mut self, op: &Op) -> Result<(), TestCaseError> {
        match op {
            Op::Write { len, expiry } => self.write(*len, *expiry),
            Op::ShredExpired => self.shred_expired(),
            Op::ShredAll => self.shred_all(),
            Op::Reopen => self.reopen(),
            Op::Recreate => self.recreate(),
        }
    }

    fn write(&mut self, len: usize, expiry: Expiry) -> Result<(), TestCaseError> {
        let Some(store) = self.store.as_mut() else {
            return Ok(());
        };
        let plaintext: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let now = unix_secs_now();
        let expires_at = expiry.expires_at(now);
        let result = store.write_segment_with_expiry(&mut Cursor::new(&plaintext), now, expires_at);
        if self.deleted {
            prop_assert!(
                matches!(result, Err(StoreError::Sealed)),
                "a write after delete-all did not report Sealed: {:?}",
                result.err()
            );
            return Ok(());
        }
        let (seg_ref, _) = result.expect("a write on an open store");
        let cipher = std::fs::read(segment_path(&self.dir, seg_ref.segment_id))
            .expect("the segment file exists after the write");
        self.segments.push(ModelSegment {
            id: seg_ref.segment_id,
            plaintext,
            cipher,
            expires_at,
            fate: Fate::Live,
        });
        Ok(())
    }

    fn shred_expired(&mut self) -> Result<(), TestCaseError> {
        let Some(store) = self.store.as_mut() else {
            return Ok(());
        };
        let now = unix_secs_now();
        let result = store.crypto_shred_expired();
        if self.deleted {
            prop_assert!(
                matches!(result, Err(StoreError::Sealed)),
                "a shred after delete-all did not report Sealed: {:?}",
                result.err()
            );
            return Ok(());
        }
        let report = result.expect("a shred pass on an open store");
        let mut expected = 0u64;
        for seg in &mut self.segments {
            if seg.fate == Fate::Live && seg.expires_at <= now {
                seg.fate = Fate::Shredded;
                expected += 1;
            }
        }
        prop_assert_eq!(
            report.segments_shredded,
            expected,
            "crypto_shred_expired reported a different count than the segments past their bound"
        );
        prop_assert_eq!(report.keys_destroyed, expected);
        Ok(())
    }

    fn shred_all(&mut self) -> Result<(), TestCaseError> {
        let Some(store) = self.store.as_mut() else {
            return Ok(());
        };
        let result = store.crypto_shred_all();
        if self.deleted {
            prop_assert!(
                matches!(result, Err(StoreError::Sealed)),
                "a second delete-all did not report Sealed: {:?}",
                result.err()
            );
            return Ok(());
        }
        let report = result.expect("delete-all on an open store");
        let live = self
            .segments
            .iter()
            .filter(|s| s.fate == Fate::Live)
            .count() as u64;
        for seg in &mut self.segments {
            seg.fate = Fate::Shredded;
        }
        self.deleted = true;
        prop_assert_eq!(report.segments_shredded, live);
        prop_assert_eq!(report.keys_destroyed, live + 1);
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), TestCaseError> {
        self.store = None;
        match EncryptedStore::open(&self.dir, DEFAULT_RETENTION) {
            Ok(store) => {
                prop_assert!(!self.deleted, "a deleted store reopened");
                self.store = Some(store);
            }
            Err(StoreError::Sealed) => {
                prop_assert!(self.deleted, "an existing store reopened as Sealed");
            }
            Err(e) => prop_assert!(false, "reopen failed: {e}"),
        }
        Ok(())
    }

    fn recreate(&mut self) -> Result<(), TestCaseError> {
        if self.deleted {
            self.store = None;
            self.create_store();
            return Ok(());
        }
        let result = EncryptedStore::create(
            &self.dir,
            Box::new(DevFallbackCustody::new(RootId::fresh())),
            DEFAULT_RETENTION,
        );
        prop_assert!(
            matches!(
                result,
                Err(StoreError::Locked(_) | StoreError::AlreadyExists(_))
            ),
            "create on a directory that holds a store did not refuse: {:?}",
            result.err()
        );
        Ok(())
    }

    /// Every segment key still usable through the store: the non-NULL
    /// `segment_keys` rows of the live connection, plus the rows a copy of
    /// `store.db` alone yields, each unwrapped with the store key.
    fn remaining_keys(&self, store: &EncryptedStore) -> Vec<SegmentKey> {
        let store_key = store.store_key().expect("the open store's key");
        let mut wrapped = wrapped_rows(store.db().expect("the open store's db").connection());
        wrapped.extend(wrapped_rows_in_main_file(&self.dir, store_key.expose()));
        wrapped
            .iter()
            .map(|w| {
                segment::unwrap_segment_key(store_key.expose(), w).expect("a remaining key unwraps")
            })
            .collect()
    }

    fn check(&self) -> Result<(), TestCaseError> {
        if self.deleted {
            return self.check_deleted();
        }
        let Some(store) = self.open_store() else {
            return Ok(());
        };
        let keys = self.remaining_keys(store);
        for seg in &self.segments {
            match seg.fate {
                Fate::Shredded => {
                    for key in &keys {
                        prop_assert!(
                            segment::decrypt_segment_bytes(&seg.cipher, key).is_err(),
                            "shredded segment {} decrypts with a key remaining in the store",
                            seg.id.simple()
                        );
                    }
                    let read = store.read_segment(seg.id, &mut Vec::new());
                    prop_assert!(
                        matches!(read, Err(StoreError::Sealed)),
                        "shredded segment {} reads as {:?} instead of Sealed",
                        seg.id.simple(),
                        read.err()
                    );
                    prop_assert!(
                        !segment_path(&self.dir, seg.id).exists(),
                        "shredded segment {} still has its file",
                        seg.id.simple()
                    );
                }
                Fate::Live => {
                    let mut out = Vec::new();
                    let read = store.read_segment(seg.id, &mut out);
                    prop_assert!(
                        read.is_ok(),
                        "live segment {} does not read: {:?}",
                        seg.id.simple(),
                        read.err()
                    );
                    prop_assert!(
                        out == seg.plaintext,
                        "live segment {} read back different bytes",
                        seg.id.simple()
                    );
                }
            }
        }
        Ok(())
    }

    fn check_deleted(&self) -> Result<(), TestCaseError> {
        for name in ["store.key", "store.db", "store.db-wal", "segments"] {
            prop_assert!(
                !self.dir.join(name).exists(),
                "delete-all left {name} in the store directory"
            );
        }
        let key_file = self
            .key_file
            .as_ref()
            .expect("the deleted store's key file");
        let custody = custody_for(key_file.custody_kind, key_file.root_id)
            .expect("the dev-fallback custody rebuilds");
        let wrapped = key_file
            .wrapped_store_key()
            .expect("the retained wrapped key decodes");
        prop_assert!(
            custody.unwrap_store_key(&wrapped).is_err(),
            "the retained wrapped store key still unwraps after delete-all"
        );
        if let Some(store) = &self.store {
            for seg in &self.segments {
                let read = store.read_segment(seg.id, &mut Vec::new());
                prop_assert!(
                    matches!(read, Err(StoreError::Sealed)),
                    "segment {} reads as {:?} after delete-all",
                    seg.id.simple(),
                    read.err()
                );
            }
        }
        Ok(())
    }
}

fn wrapped_rows(conn: &Connection) -> Vec<Vec<u8>> {
    let mut stmt = conn
        .prepare("SELECT wrapped_key FROM segment_keys WHERE wrapped_key IS NOT NULL")
        .expect("the segment_keys table exists");
    let rows = stmt
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .expect("the segment_keys rows read");
    rows.map(|r| r.expect("a wrapped key row")).collect()
}

/// The non-NULL key rows a copy of `store.db` taken without its WAL yields
/// under the store key. Before the first checkpoint the copy holds no schema,
/// so an unreadable copy yields no rows.
fn wrapped_rows_in_main_file(dir: &Path, store_key: &[u8; 32]) -> Vec<Vec<u8>> {
    let copy = tempfile::tempdir().expect("a temp dir for the store.db copy");
    if std::fs::copy(dir.join("store.db"), copy.path().join("store.db")).is_err() {
        return Vec::new();
    }
    let Ok(conn) = Connection::open(copy.path().join("store.db")) else {
        return Vec::new();
    };
    let key_pragma = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex(store_key).as_str()));
    if conn.execute_batch(&key_pragma).is_err() {
        return Vec::new();
    }
    let Ok(mut stmt) =
        conn.prepare("SELECT wrapped_key FROM segment_keys WHERE wrapped_key IS NOT NULL")
    else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0)) else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok()).collect()
}

fn hex(bytes: &[u8; 32]) -> Zeroizing<String> {
    let mut s = Zeroizing::new(String::with_capacity(64));
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

proptest! {
    #![proptest_config(shred_config())]

    /// Property 1. For any bounded sequence of writes, expiry shreds,
    /// delete-alls, reopens and recreates: once a segment is shredded, no key
    /// remaining in the store decrypts its ciphertext, it reads as Sealed and
    /// its file is gone; every live segment still reads back its bytes; and
    /// after delete-all the directory holds no store and the retained wrapped
    /// store key no longer unwraps.
    #[test]
    fn a_shredded_segment_is_unrecoverable_with_every_remaining_key(
        ops in prop::collection::vec(op(), 1..=MAX_OPS),
    ) {
        let dir = tempfile::tempdir().expect("a temp dir for the store");
        let mut harness = Harness::create(dir.path());
        harness.check()?;
        for op in &ops {
            harness.apply(op)?;
            harness.check()?;
        }
    }
}

// ---------------------------------------------------------------------------
// Property 2: single writer, across processes.
// ---------------------------------------------------------------------------

/// The plan for one contending process.
#[derive(Debug, Clone)]
struct Contender {
    delay_ms: u64,
    acquisitions: usize,
}

#[derive(Debug, Clone)]
struct Race {
    contenders: Vec<Contender>,
    hold_ms: u64,
    /// How often the parent unlinks `writer.lock` while the race runs; `None`
    /// leaves it in place.
    unlink_every_ms: Option<u64>,
}

fn contender() -> impl Strategy<Value = Contender> {
    (0u64..=20, 1..=MAX_ACQUISITIONS).prop_map(|(delay_ms, acquisitions)| Contender {
        delay_ms,
        acquisitions,
    })
}

fn race() -> impl Strategy<Value = Race> {
    (
        prop::collection::vec(contender(), 2..=MAX_CONTENDERS),
        5u64..=30,
        prop_oneof![1 => Just(None), 3 => (1u64..=8).prop_map(Some)],
    )
        .prop_map(|(contenders, hold_ms, unlink_every_ms)| Race {
            contenders,
            hold_ms,
            unlink_every_ms,
        })
}

/// One line of the race log: which contender, what happened, when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Enter,
    Exit,
    OverlapSeen,
}

#[derive(Debug, Clone, Copy)]
struct Logged {
    contender: usize,
    event: Event,
    nanos: u128,
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn log_line(path: &Path, contender: usize, event: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("the race log opens for append");
    writeln!(file, "{contender} {event} {}", now_nanos()).expect("the race log line writes");
}

fn parse_log(text: &str) -> Vec<Logged> {
    text.lines()
        .map(|line| {
            let mut parts = line.split(' ');
            let contender = parts
                .next()
                .and_then(|s| s.parse().ok())
                .expect("contender index");
            let event = match parts.next() {
                Some("enter") => Event::Enter,
                Some("exit") => Event::Exit,
                Some("overlap-seen") => Event::OverlapSeen,
                other => panic!("unknown race log event {other:?}"),
            };
            let nanos = parts
                .next()
                .and_then(|s| s.parse().ok())
                .expect("timestamp");
            Logged {
                contender,
                event,
                nanos,
            }
        })
        .collect()
}

/// The hold intervals per contender, from the enter/exit pairs in the log.
fn intervals(log: &[Logged]) -> Vec<(usize, u128, u128)> {
    let mut open: Vec<Option<u128>> = Vec::new();
    let mut out = Vec::new();
    for entry in log {
        if open.len() <= entry.contender {
            open.resize(entry.contender + 1, None);
        }
        match entry.event {
            Event::Enter => {
                assert!(open[entry.contender].is_none(), "enter without exit");
                open[entry.contender] = Some(entry.nanos);
            }
            Event::Exit => {
                let start = open[entry.contender].take().expect("exit without enter");
                out.push((entry.contender, start, entry.nanos));
            }
            Event::OverlapSeen => {}
        }
    }
    out
}

/// The child half of the race: open the store named by the environment,
/// retrying while it is held elsewhere, and hold it for a while, as many
/// times as planned. A no-op outside a race.
#[test]
fn contend_for_store_for_parent() {
    let Ok(dir) = std::env::var(CONTEND_DIR) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let index: usize = env_number(CONTEND_INDEX);
    let acquisitions: usize = env_number(CONTEND_ACQUISITIONS);
    let hold = Duration::from_millis(env_number(CONTEND_HOLD_MS));
    std::thread::sleep(Duration::from_millis(env_number(CONTEND_DELAY_MS)));
    let store_dir = dir.join("store");
    let log = dir.join("race.log");
    let inside = dir.join("inside");
    let deadline = Instant::now() + Duration::from_secs(60);

    for _ in 0..acquisitions {
        let mut store = loop {
            match EncryptedStore::open(&store_dir, DEFAULT_RETENTION) {
                Ok(store) => break store,
                Err(refusal) => {
                    assert!(
                        Instant::now() < deadline,
                        "contender {index} never got the store; last refusal: {refusal}"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        };
        if inside.exists() {
            log_line(&log, index, "overlap-seen");
        }
        std::fs::write(&inside, index.to_string()).expect("the inside witness writes");
        log_line(&log, index, "enter");
        store
            .write_segment(&mut Cursor::new(b"written while holding the store"))
            .unwrap_or_else(|e| panic!("contender {index}: write failed with {e}"));
        std::thread::sleep(hold);
        log_line(&log, index, "exit");
        let _ = std::fs::remove_file(&inside);
        drop(store);
    }
}

fn env_number<T: std::str::FromStr>(name: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} is set"))
        .parse()
        .unwrap_or_else(|e| panic!("{name} parses: {e:?}"))
}

fn spawn_contender(dir: &Path, index: usize, plan: &Contender, hold_ms: u64) -> Child {
    Command::new(std::env::current_exe().expect("the test binary path"))
        .args([
            "--exact",
            "store::formal_tests::contend_for_store_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CONTEND_DIR, dir)
        .env(CONTEND_INDEX, index.to_string())
        .env(CONTEND_ACQUISITIONS, plan.acquisitions.to_string())
        .env(CONTEND_HOLD_MS, hold_ms.to_string())
        .env(CONTEND_DELAY_MS, plan.delay_ms.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the contender spawns")
}

/// Run one race to completion: spawn the contenders, unlink the lock file on
/// the requested cadence until every child exits, and return the exit
/// statuses in contender order.
fn run_race(dir: &Path, race: &Race) -> Vec<std::process::ExitStatus> {
    let mut children: Vec<Child> = race
        .contenders
        .iter()
        .enumerate()
        .map(|(i, plan)| spawn_contender(dir, i, plan, race.hold_ms))
        .collect();
    let lock_file = dir.join("store").join("writer.lock");
    let started = Instant::now();
    let mut statuses: Vec<Option<std::process::ExitStatus>> = vec![None; children.len()];
    while statuses.iter().any(Option::is_none) {
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "the race did not finish in time"
        );
        for (child, status) in children.iter_mut().zip(statuses.iter_mut()) {
            if status.is_none() {
                *status = child.try_wait().expect("the contender's status polls");
            }
        }
        match race.unlink_every_ms {
            Some(ms) => {
                let _ = std::fs::remove_file(&lock_file);
                std::thread::sleep(Duration::from_millis(ms));
            }
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    statuses
        .into_iter()
        .map(|s| s.expect("every status"))
        .collect()
}

proptest! {
    #![proptest_config(race_config())]

    /// Property 2. For any bounded race of processes opening the same store,
    /// each holding it for a while and writing to it, with `writer.lock`
    /// unlinked under them on a random cadence: no two hold intervals overlap,
    /// no contender finds another inside, every contender holds the store as
    /// many times as planned, and every contender exits clean.
    #[test]
    fn at_most_one_process_holds_the_store_at_a_time(race in race()) {
        let dir = tempfile::tempdir().expect("a temp dir for the race");
        let store_dir = dir.path().join("store");
        let root = RootId::fresh();
        drop(
            EncryptedStore::create(
                &store_dir,
                Box::new(DevFallbackCustody::new(root)),
                DEFAULT_RETENTION,
            )
            .expect("the race's store creates"),
        );

        let statuses = run_race(dir.path(), &race);
        let text = std::fs::read_to_string(dir.path().join("race.log")).unwrap_or_default();
        DevFallbackCustody::new(root)
            .destroy_root()
            .expect("the race's root is destroyed");

        let log = parse_log(&text);
        prop_assert!(
            !log.iter().any(|e| e.event == Event::OverlapSeen),
            "a contender found another contender inside the store"
        );
        let held = intervals(&log);
        for (a, &(ca, sa, ea)) in held.iter().enumerate() {
            for &(cb, sb, eb) in &held[a + 1..] {
                prop_assert!(
                    ea <= sb || eb <= sa,
                    "contenders {ca} and {cb} held the store at the same time"
                );
            }
        }
        for (i, status) in statuses.iter().enumerate() {
            prop_assert!(status.success(), "contender {i} exited with {status}");
        }
        for (i, plan) in race.contenders.iter().enumerate() {
            let count = held.iter().filter(|(c, _, _)| *c == i).count();
            prop_assert_eq!(count, plan.acquisitions, "contender {} held the store {} times", i, count);
        }
    }
}
