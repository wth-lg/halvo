use std::cell::Cell;
use std::fs::OpenOptions;
use std::io::{self, Cursor, ErrorKind, Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use halvo_sqlcipher::rusqlite::types::ValueRef;
use halvo_sqlcipher::rusqlite::Connection;
use proptest::prelude::*;
use proptest::test_runner::{contextualize_config, FileFailurePersistence, TestRunner};
use uuid::Uuid;

use crate::custody::dev_fallback::{dev_root_bytes, replace_dev_root, DevFallbackCustody};
use crate::custody::{
    custody_for, opens_under_an_all_zero_root, CustodyError, KeyCustody, RootId, WrappedKey,
};
use crate::event::{
    CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence, SegmentKeyId, SegmentRef,
};

use super::db::test_support::pages;
use super::db::{failpoint, unix_secs_now, EncryptedStore, KeyFile};
use super::segment::{self, segment_path, SegmentKey, CHUNK};
use super::verify::{raw_key_at_rest, root_state, RootState};
use super::{SegmentExpiry, SegmentStore, ShredReport, StoreError, DEFAULT_RETENTION};

const SHRED_CASES: u32 = 48;
const MAX_OPS: usize = 12;
const FAILING_MOVE_CASES: u32 = 32;
const EVENTS_INDEX: &str = "events_fts";
const EVENTS_INDEX_SET_ASIDE: &str = "events_fts_set_aside";

const SCALED_SEGMENTS: usize = 1200;
const SCALED_SEGMENT_LEN: usize = 40;
const OLDEST_FIRST_PER_PASS: usize = 7;
const OLDEST_FIRST_PASSES: usize = 50;
const A_SOURCE_DIES_AFTER_EVERY: usize = 3;
const WRAPPED_KEY_MIDDLE: Range<usize> = 24..56;
const FILES_OF_A_STORE: [&str; 7] = [
    "segments",
    "store.db",
    "store.db-journal",
    "store.db-shm",
    "store.db-wal",
    "store.key",
    "writer.lock",
];

const RACE_CASES: u32 = 30;
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

fn failing_moves_config() -> ProptestConfig {
    ProptestConfig {
        cases: FAILING_MOVE_CASES,
        ..shred_config()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expiry {
    Past,
    OnShredBoundary,
    Future,
}

impl Expiry {
    fn expires_at(self, now: u64) -> u64 {
        match self {
            Expiry::Past => 1,
            Expiry::OnShredBoundary => now,
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
    CreateThatFailsAfterItsDatabase,
    CreateUnderTheRootOfThisStore,
    ShredAllThatStopsAfterItsRootStep,
    DestroyTheRootAgain,
    Append { index_row_fails: bool },
    CreateWhoseRootStepFails,
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
        Just(Expiry::OnShredBoundary),
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

fn op_with_failures() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (segment_len(), expiry()).prop_map(|(len, expiry)| Op::Write { len, expiry }),
        1 => Just(Op::ShredExpired),
        2 => Just(Op::ShredAll),
        1 => Just(Op::Reopen),
        1 => Just(Op::Recreate),
        2 => Just(Op::CreateThatFailsAfterItsDatabase),
        2 => Just(Op::CreateUnderTheRootOfThisStore),
        2 => Just(Op::ShredAllThatStopsAfterItsRootStep),
        1 => Just(Op::DestroyTheRootAgain),
        3 => any::<bool>().prop_map(|index_row_fails| Op::Append { index_row_fails }),
        2 => Just(Op::CreateWhoseRootStepFails),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Live,
    Shredded,
}

struct ModelSegment {
    id: Uuid,
    seg_ref: SegmentRef,
    key_id: SegmentKeyId,
    plaintext: Vec<u8>,
    cipher: Vec<u8>,
    wrapped_key: Vec<u8>,
    expires_at: u64,
    events: u64,
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

struct Harness {
    dir: PathBuf,
    roots: Vec<RootId>,
    store: Option<EncryptedStore>,
    deleted_since_last_create: bool,
    delete_all_owes_its_files: bool,
    latest_created_key_file: Option<KeyFile>,
    segments: Vec<ModelSegment>,
    moves_that_did_not_apply: usize,
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
            deleted_since_last_create: false,
            delete_all_owes_its_files: false,
            latest_created_key_file: None,
            segments: Vec::new(),
            moves_that_did_not_apply: 0,
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
        let key_file_aside = tempfile::tempdir().expect("a temp dir for a copy of store.key");
        std::fs::copy(
            KeyFile::path(&self.dir),
            key_file_aside.path().join("store.key"),
        )
        .expect("store.key copies");
        let raw_key_at_rest = raw_key_at_rest(
            key_file_aside.path(),
            store.store_key().expect("the fresh store's key").expose(),
        )
        .expect("the copy of store.key reads");
        assert!(
            !raw_key_at_rest,
            "store.key of the fresh store holds the raw store key"
        );
        self.latest_created_key_file = Some(key_file);
        self.store = Some(store);
        self.deleted_since_last_create = false;
        self.delete_all_owes_its_files = false;
    }

    fn does_not_apply(&mut self) -> Result<(), TestCaseError> {
        self.moves_that_did_not_apply += 1;
        Ok(())
    }

    fn open_store(&self) -> Option<&EncryptedStore> {
        if self.deleted_since_last_create {
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
            Op::CreateThatFailsAfterItsDatabase => self.create_that_fails_after_its_database(),
            Op::CreateUnderTheRootOfThisStore => self.create_under_the_root_of_this_store(),
            Op::ShredAllThatStopsAfterItsRootStep => {
                self.shred_all_that_stops_after_its_root_step()
            }
            Op::DestroyTheRootAgain => self.destroy_the_root_again(),
            Op::Append { index_row_fails } => self.append(*index_row_fails),
            Op::CreateWhoseRootStepFails => self.create_whose_root_step_fails(),
        }
    }

    fn write(&mut self, len: usize, expiry: Expiry) -> Result<(), TestCaseError> {
        let Some(store) = self.store.as_mut() else {
            return self.does_not_apply();
        };
        let plaintext: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let now = unix_secs_now();
        let expires_at = expiry.expires_at(now);
        let result = store.write_segment_with_expiry(&mut Cursor::new(&plaintext), now, expires_at);
        if self.deleted_since_last_create {
            prop_assert!(
                matches!(result, Err(StoreError::Sealed)),
                "a write after delete-all did not report Sealed: {:?}",
                result.err()
            );
            return Ok(());
        }
        let (seg_ref, key_id) = result.expect("a write on an open store");
        let cipher = std::fs::read(segment_path(&self.dir, seg_ref.segment_id))
            .expect("the segment file exists after the write");
        let wrapped_key = wrapped_key_row(store, seg_ref.segment_id)
            .expect("the key row of a written segment holds its wrapped key");
        self.segments.push(ModelSegment {
            id: seg_ref.segment_id,
            seg_ref,
            key_id,
            plaintext,
            cipher,
            wrapped_key,
            expires_at,
            events: 0,
            fate: Fate::Live,
        });
        Ok(())
    }

    fn append(&mut self, index_row_fails: bool) -> Result<(), TestCaseError> {
        if self.deleted_since_last_create {
            return self.does_not_apply();
        }
        let newest_live = self
            .segments
            .iter_mut()
            .rev()
            .find(|seg| seg.fate == Fate::Live);
        let (Some(store), Some(seg)) = (self.store.as_mut(), newest_live) else {
            return self.does_not_apply();
        };
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: SystemTime::now(),
                sequence: MonotonicSequence::new(seg.events + 1),
            },
            Modality::ScreenFrame,
            "an app".to_string(),
            seg.seg_ref.clone(),
            seg.key_id,
        );
        if index_row_fails {
            rename_table(store, EVENTS_INDEX, EVENTS_INDEX_SET_ASIDE);
            let appended = store.append_event(&event);
            rename_table(store, EVENTS_INDEX_SET_ASIDE, EVENTS_INDEX);
            prop_assert!(
                appended.is_err(),
                "an append whose index row could not be written returned Ok"
            );
            return Ok(());
        }
        store
            .append_event(&event)
            .expect("an append on an open store");
        seg.events += 1;
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
            return self.does_not_apply();
        };
        let now = unix_secs_now();
        let result = store.crypto_shred_expired();
        if self.deleted_since_last_create {
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
            return self.does_not_apply();
        };
        let result = store.crypto_shred_all();
        if self.deleted_since_last_create {
            self.delete_all_owes_its_files = false;
            prop_assert!(
                matches!(
                    result,
                    Ok(ShredReport {
                        segments_shredded: 0,
                        keys_destroyed: 0
                    })
                ),
                "a second delete-all did not finish with nothing left to shred: {:?}",
                result
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
        self.deleted_since_last_create = true;
        prop_assert_eq!(report.segments_shredded, live);
        prop_assert_eq!(report.keys_destroyed, live + 1);
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), TestCaseError> {
        self.store = None;
        if !self.deleted_since_last_create {
            widen_the_database_file(&self.dir);
        }
        match EncryptedStore::open(&self.dir, DEFAULT_RETENTION) {
            Ok(store) => {
                prop_assert!(!self.deleted_since_last_create, "a deleted store reopened");
                self.store = Some(store);
            }
            Err(StoreError::Sealed) => {
                prop_assert!(
                    self.deleted_since_last_create && !self.delete_all_owes_its_files,
                    "a store whose delete-all did not finish reopened as Sealed"
                );
            }
            Err(StoreError::Custody(CustodyError::RootMissing)) => {
                prop_assert!(
                    self.delete_all_owes_its_files,
                    "a store whose delete-all did not stop after its root step reopened as RootMissing"
                );
            }
            Err(e) => prop_assert!(false, "reopen failed: {e}"),
        }
        Ok(())
    }

    fn reopen_under_another_root(&mut self) -> Result<(), TestCaseError> {
        if self.deleted_since_last_create {
            return self.does_not_apply();
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
        if self.deleted_since_last_create && !self.delete_all_owes_its_files {
            self.store = None;
            self.create_store();
            return Ok(());
        }
        let refused_root = RootId::fresh();
        self.roots.insert(0, refused_root);
        let result = EncryptedStore::create(
            &self.dir,
            Box::new(DevFallbackCustody::new(refused_root)),
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

    fn create_that_fails_after_its_database(&mut self) -> Result<(), TestCaseError> {
        if !self.deleted_since_last_create || self.delete_all_owes_its_files {
            return self.does_not_apply();
        }
        self.store = None;
        let root = RootId::fresh();
        self.roots.push(root);
        failpoint::arm("db_open");
        let result = EncryptedStore::create(
            &self.dir,
            Box::new(DevFallbackCustody::new(root)),
            DEFAULT_RETENTION,
        );
        let reached_its_database = failpoint::hit("db_open").is_ok();
        prop_assert!(
            reached_its_database,
            "a create on a free directory did not open a database"
        );
        prop_assert!(
            matches!(result, Err(StoreError::Io(ErrorKind::Other))),
            "a create whose database failed after it was made returned {:?}",
            result.as_ref().err()
        );
        let root_left = dev_root_bytes(root)
            .expect("the custody says whether the root exists")
            .is_some();
        prop_assert!(
            !root_left,
            "a create that failed left the root that it made"
        );
        for name in names_in(&self.dir) {
            prop_assert!(
                name == "writer.lock",
                "a create that failed left {name} in the store directory"
            );
        }
        Ok(())
    }

    fn create_under_the_root_of_this_store(&mut self) -> Result<(), TestCaseError> {
        if self.deleted_since_last_create || self.store.is_none() {
            return self.does_not_apply();
        }
        let root = *self.roots.last().expect("the root id of the store");
        let another_dir = tempfile::tempdir().expect("a temp dir for a second store");
        let result = EncryptedStore::create(
            another_dir.path(),
            Box::new(DevFallbackCustody::new(root)),
            DEFAULT_RETENTION,
        );
        prop_assert!(
            matches!(result, Err(StoreError::Custody(CustodyError::RootExists))),
            "a create under the root of another store returned {:?} in place of RootExists",
            result.as_ref().err()
        );
        for name in names_in(another_dir.path()) {
            prop_assert!(
                name == "writer.lock",
                "a create that was refused its root left {name} in its directory"
            );
        }
        self.store = None;
        match EncryptedStore::open(&self.dir, DEFAULT_RETENTION) {
            Ok(store) => self.store = Some(store),
            Err(e) => prop_assert!(
                false,
                "after a create under its root was refused, the store did not reopen: {e}"
            ),
        }
        Ok(())
    }

    fn create_whose_root_step_fails(&self) -> Result<(), TestCaseError> {
        let root = *self.roots.last().expect("the root id of the newest store");
        for answer in ANSWERS_OF_A_ROOT_STEP_THAT_MADE_NO_ROOT {
            let another_dir = tempfile::tempdir().expect("a temp dir for a second store");
            let destroy_calls = Arc::new(AtomicUsize::new(0));
            let result = EncryptedStore::create(
                another_dir.path(),
                Box::new(ACustodyWhoseRootStepFails {
                    root_id: root,
                    answer,
                    destroy_calls: Arc::clone(&destroy_calls),
                }),
                DEFAULT_RETENTION,
            );
            let answered = format!("{:?}", StoreError::Custody(answer()));
            let returned = result.as_ref().err().map(|e| format!("{e:?}"));
            prop_assert!(
                returned.as_deref() == Some(answered.as_str()),
                "a create whose root step answered {answered} returned {returned:?}"
            );
            let destroyed = destroy_calls.load(Ordering::SeqCst);
            prop_assert!(
                destroyed == 0,
                "a create whose root step answered {answered} had made no root and called destroy_root {destroyed} times"
            );
            for name in names_in(another_dir.path()) {
                prop_assert!(
                    name == "writer.lock",
                    "a create whose root step answered {answered} left {name} in its directory"
                );
            }
        }
        Ok(())
    }

    fn shred_all_that_stops_after_its_root_step(&mut self) -> Result<(), TestCaseError> {
        if self.deleted_since_last_create {
            return self.does_not_apply();
        }
        let Some(store) = self.store.as_mut() else {
            return self.does_not_apply();
        };
        failpoint::arm("key_file_destroy");
        let result = store.crypto_shred_all();
        let reached_its_key_file = failpoint::hit("key_file_destroy").is_ok();
        for seg in &mut self.segments {
            seg.fate = Fate::Shredded;
        }
        self.deleted_since_last_create = true;
        self.delete_all_owes_its_files = true;
        prop_assert!(
            reached_its_key_file,
            "delete-all did not reach the removal of its key file: {:?}",
            result
        );
        prop_assert!(
            matches!(result, Err(StoreError::Io(ErrorKind::Other))),
            "a delete-all whose key file could not be removed returned {:?}",
            result
        );
        Ok(())
    }

    fn destroy_the_root_again(&mut self) -> Result<(), TestCaseError> {
        if !self.deleted_since_last_create {
            return self.does_not_apply();
        }
        let root = self
            .latest_created_key_file
            .as_ref()
            .expect("the deleted store's key file")
            .root_id;
        require_a_root_that_destroys_again(&DevFallbackCustody::new(root))
    }

    fn remaining_keys(&self, store: &EncryptedStore) -> Vec<SegmentKey> {
        let store_key = store.store_key().expect("the open store's key");
        let conn = store.db().expect("the open store's db").connection();
        for wrapped in wrapped_rows(conn) {
            segment::unwrap_segment_key(store_key.expose(), &wrapped)
                .expect("a remaining key unwraps");
        }
        blobs_of_every_table(conn)
            .iter()
            .filter_map(|blob| segment::unwrap_segment_key(store_key.expose(), blob).ok())
            .collect()
    }

    fn check(&self) -> Result<(), TestCaseError> {
        if self.deleted_since_last_create {
            return self.check_deleted();
        }
        let Some(store) = self.open_store() else {
            return Ok(());
        };
        self.check_directory()?;
        self.check_remaining_keys(store)?;
        self.check_reads(store)?;
        self.check_events(store)?;
        self.check_store_files(store)
    }

    fn check_events(&self, store: &EncryptedStore) -> Result<(), TestCaseError> {
        let conn = store.db().expect("the open store's db").connection();
        let appended: u64 = self
            .segments
            .iter()
            .filter(|seg| seg.fate == Fate::Live)
            .map(|seg| seg.events)
            .sum();
        for table in ["events", EVENTS_INDEX] {
            let rows: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .expect("the rows of the events count");
            prop_assert!(
                rows as u64 == appended,
                "{table} holds {rows} rows, and {appended} appends returned Ok for the segments that are live"
            );
        }
        Ok(())
    }

    fn check_at_scale(&self) -> Result<(), TestCaseError> {
        let store = self.open_store().expect("an open store");
        self.check_directory()?;
        self.check_reads(store)?;
        self.check_store_files(store)
    }

    fn check_directory(&self) -> Result<(), TestCaseError> {
        for name in names_in(&self.dir) {
            prop_assert!(
                FILES_OF_A_STORE.contains(&name.as_str()),
                "the store directory holds {name}, which is no file of a store"
            );
        }
        require_owner_only_modes(&self.dir)
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
            .latest_created_key_file
            .as_ref()
            .expect("the deleted store's key file");
        let custody = custody_for(key_file.custody_kind, key_file.root_id)
            .expect("the dev-fallback custody rebuilds");
        let wrapped = key_file
            .wrapped_store_key()
            .expect("the retained wrapped key decodes");
        require_a_destroyed_root(custody.as_ref(), &wrapped)?;
        for name in names_in(&self.dir) {
            if self.delete_all_owes_its_files {
                prop_assert!(
                    FILES_OF_A_STORE.contains(&name.as_str()),
                    "a delete-all that stopped left {name}, which is no file of a store"
                );
            } else {
                prop_assert!(
                    name == "writer.lock",
                    "delete-all left {name} in the store directory"
                );
            }
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

fn require_a_destroyed_root(
    custody: &dyn KeyCustody,
    retained: &WrappedKey,
) -> Result<(), TestCaseError> {
    match root_state(custody, retained) {
        RootState::Destroyed => Ok(()),
        RootState::Alive => Err(TestCaseError::fail(
            "after delete-all the root is alive: the retained wrapped store key still unwraps",
        )),
        RootState::Unknown(error) => Err(TestCaseError::fail(format!(
            "after delete-all the state of the root is unknown: the unwrap of the retained wrapped store key failed with {error:?}, which does not say that the root is missing"
        ))),
    }
}

fn require_a_root_that_destroys_again(custody: &dyn KeyCustody) -> Result<(), TestCaseError> {
    match custody.destroy_root() {
        Ok(()) => Ok(()),
        Err(error) => Err(TestCaseError::fail(format!(
            "destroy_root on a root that delete-all had destroyed failed with {error:?}, so a delete-all that stopped after its root step could not finish"
        ))),
    }
}

fn rename_table(store: &EncryptedStore, from: &str, to: &str) {
    store
        .db()
        .expect("the open store's db")
        .connection()
        .execute_batch(&format!("ALTER TABLE {from} RENAME TO {to};"))
        .expect("the index table of the events renames");
}

struct AStandInCustody {
    unwrap: fn() -> Result<crate::custody::StoreKey, CustodyError>,
    destroy: fn() -> Result<(), CustodyError>,
}

impl KeyCustody for AStandInCustody {
    fn kind(&self) -> crate::custody::CustodyKind {
        crate::custody::CustodyKind::DevFallback
    }
    fn root_id(&self) -> RootId {
        RootId::fresh()
    }
    fn wrap_store_key(&self, _key: &crate::custody::StoreKey) -> Result<WrappedKey, CustodyError> {
        Ok(WrappedKey(Vec::new()))
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        Ok(())
    }
    fn unwrap_store_key(
        &self,
        _wrapped: &WrappedKey,
    ) -> Result<crate::custody::StoreKey, CustodyError> {
        (self.unwrap)()
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        (self.destroy)()
    }
}

const ANSWERS_OF_A_ROOT_STEP_THAT_MADE_NO_ROOT: [fn() -> CustodyError; 5] = [
    || CustodyError::RootLocked(-25308),
    || CustodyError::RootDenied,
    || CustodyError::RootCorrupt,
    || CustodyError::Crypto("the root of this id does not decode".to_string()),
    || CustodyError::Os(Some(17)),
];

struct ACustodyWhoseRootStepFails {
    root_id: RootId,
    answer: fn() -> CustodyError,
    destroy_calls: Arc<AtomicUsize>,
}

impl KeyCustody for ACustodyWhoseRootStepFails {
    fn kind(&self) -> crate::custody::CustodyKind {
        crate::custody::CustodyKind::DevFallback
    }
    fn root_id(&self) -> RootId {
        self.root_id
    }
    fn wrap_store_key(&self, _key: &crate::custody::StoreKey) -> Result<WrappedKey, CustodyError> {
        Ok(WrappedKey(vec![0u8; 72]))
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        Err((self.answer)())
    }
    fn unwrap_store_key(
        &self,
        _wrapped: &WrappedKey,
    ) -> Result<crate::custody::StoreKey, CustodyError> {
        Err(CustodyError::RootMissing)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.destroy_calls.fetch_add(1, Ordering::SeqCst);
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

#[cfg(unix)]
fn require_owner_only_modes(store_dir: &Path) -> Result<(), TestCaseError> {
    use std::os::unix::fs::PermissionsExt;
    let mut entries = vec![store_dir.to_path_buf()];
    while let Some(path) = entries.pop() {
        let meta = std::fs::symlink_metadata(&path).expect("an entry of the store directory stats");
        let mode = meta.permissions().mode() & 0o777;
        let owner_only = if meta.is_dir() { 0o700 } else { 0o600 };
        prop_assert!(
            mode == owner_only,
            "{} has mode {:o} in the store directory, and a store makes it {:o}",
            path.strip_prefix(store_dir).unwrap_or(&path).display(),
            mode,
            owner_only
        );
        if meta.is_dir() {
            let below = std::fs::read_dir(&path).expect("a directory of the store lists");
            entries.extend(below.map(|entry| entry.expect("an entry of the store").path()));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn require_owner_only_modes(_store_dir: &Path) -> Result<(), TestCaseError> {
    Ok(())
}

#[cfg(unix)]
fn widen_the_database_file(store_dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        store_dir.join("store.db"),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("store.db of a store that no process holds takes a wider mode");
}

#[cfg(not(unix))]
fn widen_the_database_file(_store_dir: &Path) {}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the store directory lists")
        .map(|entry| {
            entry
                .expect("an entry of the store directory")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn blobs_of_every_table(conn: &Connection) -> Vec<Vec<u8>> {
    let mut tables = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .expect("the schema reads");
    let tables: Vec<String> = tables
        .query_map([], |row| row.get(0))
        .expect("the names of the tables read")
        .map(|name| name.expect("the name of a table"))
        .collect();
    let mut blobs = Vec::new();
    for table in tables {
        let mut rows = conn
            .prepare(&format!("SELECT * FROM \"{table}\""))
            .expect("a table of the store reads");
        let columns = rows.column_count();
        let mut rows = rows.query([]).expect("the rows of a table read");
        while let Some(row) = rows.next().expect("a row of a table") {
            for column in 0..columns {
                if let ValueRef::Blob(blob) = row.get_ref(column).expect("a value of a row") {
                    blobs.push(blob.to_vec());
                }
            }
        }
    }
    blobs
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

proptest! {
    #![proptest_config(failing_moves_config())]

    #[test]
    fn no_failed_or_refused_move_leaves_a_half_store(
        ops in prop::collection::vec(op_with_failures(), 1..=MAX_OPS),
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
fn no_failed_or_refused_move_leaves_a_half_store_in_a_fixed_order() {
    let moves = [
        Op::Write {
            len: SCALED_SEGMENT_LEN,
            expiry: Expiry::Past,
        },
        Op::Append {
            index_row_fails: false,
        },
        Op::Append {
            index_row_fails: true,
        },
        Op::Write {
            len: SCALED_SEGMENT_LEN,
            expiry: Expiry::Future,
        },
        Op::Append {
            index_row_fails: false,
        },
        Op::CreateUnderTheRootOfThisStore,
        Op::ShredExpired,
        Op::Append {
            index_row_fails: true,
        },
        Op::ShredAllThatStopsAfterItsRootStep,
        Op::DestroyTheRootAgain,
        Op::Recreate,
        Op::ShredAll,
        Op::ShredAll,
        Op::DestroyTheRootAgain,
        Op::CreateThatFailsAfterItsDatabase,
        Op::CreateThatFailsAfterItsDatabase,
        Op::Recreate,
        Op::Write {
            len: SCALED_SEGMENT_LEN,
            expiry: Expiry::Future,
        },
        Op::Append {
            index_row_fails: false,
        },
        Op::CreateUnderTheRootOfThisStore,
        Op::Reopen,
        Op::ShredAll,
        Op::Reopen,
        Op::CreateWhoseRootStepFails,
    ];
    let dir = tempfile::tempdir().expect("a temp dir for the store");
    let mut harness = Harness::create(dir.path());
    hold(harness.check());
    for (at, op) in moves.iter().enumerate() {
        if let Err(failure) = harness.apply(op).and_then(|()| harness.check()) {
            panic!("at move {} of {}, {op:?}: {failure}", at + 1, moves.len());
        }
        assert_eq!(
            harness.moves_that_did_not_apply,
            0,
            "move {} of {}, {op:?}, does not apply to the state that the moves before it left",
            at + 1,
            moves.len()
        );
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

#[test]
fn the_delete_all_step_takes_only_a_missing_root_as_destroyed() {
    let retained = WrappedKey(vec![0u8; 72]);
    let answer = |unwrap| {
        let custody = AStandInCustody {
            unwrap,
            destroy: || Ok(()),
        };
        match require_a_destroyed_root(&custody, &retained) {
            Ok(()) => "destroyed".to_string(),
            Err(failure) => failure.to_string(),
        }
    };

    assert_eq!(answer(|| Err(CustodyError::RootMissing)), "destroyed");
    let alive = answer(|| Ok(crate::custody::StoreKey::from_bytes([1u8; 32])));
    assert!(alive.contains("the root is alive"), "{alive}");
    let transient = answer(|| Err(CustodyError::RootLocked(-25308)));
    assert!(
        transient.contains("the state of the root is unknown") && transient.contains("RootLocked"),
        "{transient}"
    );
    let another_error = answer(|| Err(CustodyError::WrapAuthFailed));
    assert!(
        another_error.contains("the state of the root is unknown"),
        "{another_error}"
    );
}

#[test]
fn a_destroyed_root_destroys_again_only_when_its_custody_returns_ok() {
    let again = |destroy| {
        let custody = AStandInCustody {
            unwrap: || Err(CustodyError::RootMissing),
            destroy,
        };
        match require_a_root_that_destroys_again(&custody) {
            Ok(()) => "destroyed again".to_string(),
            Err(failure) => failure.to_string(),
        }
    };

    assert_eq!(again(|| Ok(())), "destroyed again");
    let refused = again(|| Err(CustodyError::RootMissing));
    assert!(
        refused.contains("destroy_root on a root that delete-all had destroyed")
            && refused.contains("RootMissing"),
        "{refused}"
    );
}

#[derive(Debug, Clone)]
struct Contender {
    delay_ms: u64,
    acquisitions: usize,
}

#[derive(Debug, Clone)]
struct Race {
    contenders: Vec<Contender>,
    hold_ms: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    Locked,
    AccessDenied,
}

fn writer_refusal(error: &StoreError) -> Option<Refusal> {
    match error {
        StoreError::Locked(_) => Some(Refusal::Locked),
        StoreError::Io(ErrorKind::PermissionDenied) if cfg!(windows) => Some(Refusal::AccessDenied),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Enter,
    Exit,
    OverlapSeen,
    Refused(Refusal),
}

#[derive(Debug, Clone, Copy)]
struct Logged {
    contender: usize,
    event: Event,
    nanos: u128,
    open_started: u128,
    asked_its_custody: bool,
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn log_line(path: &Path, contender: usize, event: &str) {
    log_text(path, &format!("{contender} {event} {}\n", now_nanos()));
}

fn log_refusal(
    path: &Path,
    contender: usize,
    refusal: Refusal,
    open_started: u128,
    asked_its_custody: bool,
) {
    let event = match refusal {
        Refusal::Locked => "refused-locked",
        Refusal::AccessDenied => "refused-access-denied",
    };
    let custody = if asked_its_custody {
        "after-its-custody"
    } else {
        "before-its-custody"
    };
    let now = now_nanos();
    log_text(
        path,
        &format!("{contender} {event} {now} {open_started} {custody}\n"),
    );
}

fn log_text(path: &Path, line: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("the race log opens for append");
    file.write_all(line.as_bytes())
        .expect("the race log line writes");
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
                Some("refused-locked") => Event::Refused(Refusal::Locked),
                Some("refused-access-denied") => Event::Refused(Refusal::AccessDenied),
                other => panic!("unknown race log event {other:?}"),
            };
            let nanos = parts
                .next()
                .and_then(|s| s.parse().ok())
                .expect("timestamp");
            let open_started = parts.next().map_or(nanos, |s| {
                s.parse().expect("the time at which a refused open began")
            });
            let asked_its_custody = match parts.next() {
                None | Some("before-its-custody") => false,
                Some("after-its-custody") => true,
                other => panic!("unknown custody mark of a refusal {other:?}"),
            };
            Logged {
                contender,
                event,
                nanos,
                open_started,
                asked_its_custody,
            }
        })
        .collect()
}

fn hold_intervals_per_contender(log: &[Logged]) -> Vec<(usize, u128, u128)> {
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
            Event::OverlapSeen | Event::Refused(_) => {}
        }
    }
    out
}

#[test]
fn contend_for_store_for_parent() {
    let Ok(dir) = std::env::var(CONTEND_DIR) else {
        println!("contend_for_store_for_parent: {CONTEND_DIR} is not set, so no race asked for a contender");
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
            let asked_its_custody = Cell::new(false);
            let open_started = now_nanos();
            let opened =
                EncryptedStore::open_with(&store_dir, DEFAULT_RETENTION, |kind, root_id| {
                    asked_its_custody.set(true);
                    custody_for(kind, root_id)
                });
            match opened {
                Ok(store) => break store,
                Err(error) => {
                    let Some(refusal) = writer_refusal(&error) else {
                        panic!("contender {index}: open failed with {error:?}, which is no refusal of a second writer");
                    };
                    log_refusal(&log, index, refusal, open_started, asked_its_custody.get());
                    assert!(
                        Instant::now() < deadline,
                        "contender {index} never got the store; last refusal: {error}"
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

#[derive(Default)]
struct RefusalsSeen {
    locked: Cell<u64>,
    access_denied: Cell<u64>,
    while_another_contender_held_the_store: Cell<u64>,
    before_the_custody_through_a_whole_hold: Cell<u64>,
}

impl RefusalsSeen {
    fn count(&self, log: &[Logged], held: &[(usize, u128, u128)]) {
        for entry in log {
            let Event::Refused(refusal) = entry.event else {
                continue;
            };
            let class = match refusal {
                Refusal::Locked => &self.locked,
                Refusal::AccessDenied => &self.access_denied,
            };
            class.set(class.get() + 1);
            let another_held = held.iter().any(|&(holder, entered, left)| {
                holder != entry.contender && entered <= entry.nanos && entry.nanos <= left
            });
            if another_held {
                let seen = &self.while_another_contender_held_the_store;
                seen.set(seen.get() + 1);
            }
            if holder_through_the_whole_open(entry, held).is_some() {
                let seen = &self.before_the_custody_through_a_whole_hold;
                seen.set(seen.get() + 1);
            }
        }
    }
}

fn holder_through_the_whole_open(refused: &Logged, held: &[(usize, u128, u128)]) -> Option<usize> {
    held.iter()
        .find(|&&(holder, entered, left)| {
            holder != refused.contender && entered <= refused.open_started && refused.nanos <= left
        })
        .map(|&(holder, _, _)| holder)
}

fn one_race(race: &Race, refusals: &RefusalsSeen) -> Result<(), TestCaseError> {
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

    let statuses = run_race(dir.path(), race);
    let text = std::fs::read_to_string(dir.path().join("race.log")).unwrap_or_default();
    DevFallbackCustody::new(root)
        .destroy_root()
        .expect("the race's root is destroyed");

    let log = parse_log(&text);
    prop_assert!(
        !log.iter().any(|e| e.event == Event::OverlapSeen),
        "a contender found another contender inside the store"
    );
    let held = hold_intervals_per_contender(&log);
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
    for refused in log.iter().filter(|entry| entry.asked_its_custody) {
        let holder = holder_through_the_whole_open(refused, &held);
        prop_assert!(
            holder.is_none(),
            "contender {} asked its custody in an open that was refused, and contender {:?} held the store from before that open to after it",
            refused.contender,
            holder
        );
    }
    for (i, plan) in race.contenders.iter().enumerate() {
        let count = held.iter().filter(|(c, _, _)| *c == i).count();
        prop_assert_eq!(
            count,
            plan.acquisitions,
            "contender {} held the store {} times",
            i,
            count
        );
    }
    refusals.count(&log, &held);
    Ok(())
}

#[test]
fn at_most_one_process_holds_the_store_at_a_time() {
    let config = contextualize_config(race_config());
    let cases = config.cases;
    let refusals = RefusalsSeen::default();
    let mut runner = TestRunner::new(config);
    if let Err(failure) = runner.run(&race(), |race| one_race(&race, &refusals)) {
        panic!("{failure}\n{runner}");
    }
    let while_held = refusals.while_another_contender_held_the_store.get();
    let through_a_hold = refusals.before_the_custody_through_a_whole_hold.get();
    let _ = writeln!(
        io::stderr(),
        "check 2 refusals in {cases} cases: locked={} access_denied={} while_another_contender_held_the_store={while_held} before_the_custody_through_a_whole_hold={through_a_hold}",
        refusals.locked.get(),
        refusals.access_denied.get(),
    );
    assert!(
        cases < RACE_CASES || while_held > 0,
        "in {cases} cases no contender was refused while another held the store, so this run cannot tell a lock from no lock"
    );
    assert!(
        cases < RACE_CASES || through_a_hold > 0,
        "in {cases} cases no open began and was refused within one hold of another contender, so this run cannot tell a refusal before the custody from one after it"
    );
}
