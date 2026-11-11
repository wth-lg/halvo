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
use std::io::{self, Cursor, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use halvo_sqlcipher::rusqlite::Connection;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use uuid::Uuid;

use crate::custody::dev_fallback::{dev_root_bytes, replace_dev_root, DevFallbackCustody};
use crate::custody::{custody_for, opens_under_an_all_zero_root, CustodyError, KeyCustody, RootId};

use super::db::test_support::pages;
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

const SCALED_SEGMENTS: usize = 1200;
const SCALED_SEGMENT_LEN: usize = 40;
const OLDEST_FIRST_PER_PASS: usize = 7;
const OLDEST_FIRST_PASSES: usize = 50;
const A_SOURCE_DIES_AFTER_EVERY: usize = 3;
const WRAPPED_KEY_MIDDLE: Range<usize> = 24..56;

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
    ReopenUnderAnotherRoot,
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
        1 => Just(Op::ReopenUnderAnotherRoot),
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
    wrapped_key: Vec<u8>,
    expires_at: u64,
    fate: Fate,
}

struct ASourceThatDies {
    bytes_before_it_dies: usize,
}

impl Read for ASourceThatDies {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.bytes_before_it_dies == 0 {
            return Err(io::Error::other("the source died"));
        }
        let given = buf.len().min(self.bytes_before_it_dies);
        buf[..given].fill(0x5a);
        self.bytes_before_it_dies -= given;
        Ok(given)
    }
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
        let key_file = KeyFile::read(&self.dir).expect("the fresh store's key file");
        let wrapped = key_file
            .wrapped_store_key()
            .expect("the fresh store's wrapped key decodes");
        assert!(
            !opens_under_an_all_zero_root(key_file.root_id, &wrapped),
            "the wrapped store key of a fresh store opens under an all-zero key, so no root seals it"
        );
        self.key_file = Some(key_file);
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
            Op::ReopenUnderAnotherRoot => self.reopen_under_another_root(),
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
        let wrapped_key = wrapped_key_row(store, seg_ref.segment_id)
            .expect("the key row of a written segment holds its wrapped key");
        self.segments.push(ModelSegment {
            id: seg_ref.segment_id,
            plaintext,
            cipher,
            wrapped_key,
            expires_at,
            fate: Fate::Live,
        });
        Ok(())
    }

    fn write_from_a_source_that_dies(&mut self) {
        let store = self.store.as_mut().expect("an open store");
        let now = unix_secs_now();
        let written = store.write_segment_with_expiry(
            &mut ASourceThatDies {
                bytes_before_it_dies: 100,
            },
            now,
            Expiry::Future.expires_at(now),
        );
        assert!(
            written.is_err(),
            "a write whose source died returned a segment"
        );
    }

    fn time_passes_for_the_oldest_live_segments(&mut self, count: usize) {
        let store = self.store.as_ref().expect("an open store");
        let conn = store.db().expect("the open store's db").connection();
        let oldest_live = self
            .segments
            .iter_mut()
            .filter(|seg| seg.fate == Fate::Live)
            .take(count);
        for seg in oldest_live {
            seg.expires_at = 1;
            let rows = conn
                .execute(
                    "UPDATE segment_keys SET expires_at = 1 WHERE segment_id = ?1",
                    [seg.id.simple().to_string()],
                )
                .expect("the expiry of a live segment moves into the past");
            assert_eq!(rows, 1, "a live segment has one key row");
        }
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

    fn reopen_under_another_root(&mut self) -> Result<(), TestCaseError> {
        if self.deleted {
            return Ok(());
        }
        let root_id = *self.roots.last().expect("the root id of the store");
        let root = dev_root_bytes(root_id)
            .expect("the root of the store reads")
            .expect("the root of the store exists");
        let mut another_root = *root;
        another_root[0] ^= 0x01;
        self.store = None;
        replace_dev_root(root_id, &another_root)
            .expect("other bytes are put under the id of the root");
        let under_another_root = EncryptedStore::open(&self.dir, DEFAULT_RETENTION).map(drop);
        replace_dev_root(root_id, &root).expect("the bytes of the root are put back");
        let under_its_root = EncryptedStore::open(&self.dir, DEFAULT_RETENTION);
        prop_assert!(
            matches!(
                under_another_root,
                Err(StoreError::Custody(CustodyError::WrapAuthFailed))
            ),
            "with other bytes under the id of its root, the store reopened as {:?}",
            under_another_root
        );
        match under_its_root {
            Ok(store) => self.store = Some(store),
            Err(e) => prop_assert!(
                false,
                "with the bytes of its root put back, the reopen failed: {e}"
            ),
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
    /// `segment_keys` rows of the live connection, each unwrapped with the
    /// store key.
    fn remaining_keys(&self, store: &EncryptedStore) -> Vec<SegmentKey> {
        let store_key = store.store_key().expect("the open store's key");
        wrapped_rows(store.db().expect("the open store's db").connection())
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
        self.check_remaining_keys(store)?;
        self.check_reads(store)?;
        self.check_store_files(store)
    }

    fn check_at_scale(&self) -> Result<(), TestCaseError> {
        let store = self.open_store().expect("an open store");
        self.check_reads(store)?;
        self.check_store_files(store)
    }

    fn check_remaining_keys(&self, store: &EncryptedStore) -> Result<(), TestCaseError> {
        let keys = self.remaining_keys(store);
        for seg in self.segments.iter().filter(|s| s.fate == Fate::Shredded) {
            for key in &keys {
                prop_assert!(
                    segment::decrypt_segment_bytes(&seg.cipher, key).is_err(),
                    "shredded segment {} decrypts with a key remaining in the store",
                    seg.id.simple()
                );
            }
        }
        Ok(())
    }

    fn check_reads(&self, store: &EncryptedStore) -> Result<(), TestCaseError> {
        for seg in &self.segments {
            match seg.fate {
                Fate::Shredded => {
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

    fn check_store_files(&self, store: &EncryptedStore) -> Result<(), TestCaseError> {
        let store_key = store.store_key().expect("the open store's key");
        let images = pages::store_pages(&self.dir, store_key.expose());
        let (live, shredded): (Vec<&ModelSegment>, Vec<&ModelSegment>) =
            self.segments.iter().partition(|seg| seg.fate == Fate::Live);
        let whole: Vec<&[u8]> = shredded
            .iter()
            .map(|seg| seg.wrapped_key.as_slice())
            .collect();
        let middle: Vec<&[u8]> = shredded
            .iter()
            .map(|seg| &seg.wrapped_key[WRAPPED_KEY_MIDDLE])
            .collect();
        let found = pages::places_of(&images, &whole)
            .into_iter()
            .zip(pages::places_of(&images, &middle));
        for (seg, (whole, middle)) in shredded.iter().zip(found) {
            prop_assert!(
                whole.is_empty() && middle.is_empty(),
                "the wrapped key of shredded segment {} is still in the store files: whole at {:?}, its middle 32 bytes at {:?}",
                seg.id.simple(),
                whole,
                middle
            );
        }
        let live_keys: Vec<&[u8]> = live.iter().map(|seg| seg.wrapped_key.as_slice()).collect();
        for (seg, places) in live.iter().zip(pages::places_of(&images, &live_keys)) {
            prop_assert!(
                !places.is_empty(),
                "the wrapped key of live segment {} is in no page of store.db, no frame of store.db-wal and no record of store.db-journal, so the reader of the store files sees no key",
                seg.id.simple()
            );
        }
        Ok(())
    }

    fn check_deleted(&self) -> Result<(), TestCaseError> {
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
        for name in [
            "store.key",
            "store.db",
            "store.db-wal",
            "store.db-shm",
            "store.db-journal",
            "segments",
        ] {
            prop_assert!(
                !self.dir.join(name).exists(),
                "delete-all left {name} in the store directory"
            );
        }
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

fn wrapped_key_row(store: &EncryptedStore, segment_id: Uuid) -> Option<Vec<u8>> {
    store
        .db()
        .expect("the open store's db")
        .connection()
        .query_row(
            "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
            [segment_id.simple().to_string()],
            |r| r.get(0),
        )
        .expect("the key row of the segment")
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

fn hold<T>(result: Result<T, TestCaseError>) -> T {
    result.unwrap_or_else(|failure| panic!("{failure}"))
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

#[test]
fn no_key_of_a_shredded_segment_is_left_when_three_in_four_expire_in_one_pass() {
    let dir = tempfile::tempdir().expect("a temp dir for the store");
    let mut harness = Harness::create(dir.path());
    for i in 0..SCALED_SEGMENTS {
        let expiry = if i % 4 == 0 {
            Expiry::Future
        } else {
            Expiry::Past
        };
        hold(harness.write(SCALED_SEGMENT_LEN, expiry));
    }
    hold(harness.check_at_scale());
    hold(harness.shred_expired());
    hold(harness.check_at_scale());
}

#[test]
fn no_key_of_a_shredded_segment_is_left_when_segments_expire_oldest_first() {
    let dir = tempfile::tempdir().expect("a temp dir for the store");
    let mut harness = Harness::create(dir.path());
    for i in 0..SCALED_SEGMENTS {
        hold(harness.write(SCALED_SEGMENT_LEN, Expiry::Future));
        if (i + 1) % A_SOURCE_DIES_AFTER_EVERY == 0 {
            harness.write_from_a_source_that_dies();
        }
    }
    hold(harness.check_at_scale());
    for pass in 1..=OLDEST_FIRST_PASSES {
        harness.time_passes_for_the_oldest_live_segments(OLDEST_FIRST_PER_PASS);
        hold(harness.shred_expired());
        let store = harness.open_store().expect("an open store");
        if let Err(failure) = harness.check_store_files(store) {
            panic!("after pass {pass} of {OLDEST_FIRST_PASSES}: {failure}");
        }
    }
    hold(harness.check_at_scale());
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
