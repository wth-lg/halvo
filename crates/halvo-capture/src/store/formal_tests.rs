use std::cell::{Cell, RefCell};
use std::fs::OpenOptions;
use std::io::{self, Cursor, ErrorKind, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use halvo_sqlcipher::rusqlite::types::ValueRef;
use halvo_sqlcipher::rusqlite::Connection;
use proptest::prelude::*;
use proptest::test_runner::{contextualize_config, FileFailurePersistence, TestRunner};
use uuid::Uuid;

use crate::custody::{
    custody_for, opens_under_an_all_zero_root, CustodyError, KeyCustody, RootId, WrappedKey,
};
use crate::event::{
    CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence, SegmentKeyId, SegmentRef,
};

use super::db::test_support::{key_statement, pages};
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
const DATABASE_FILES: [&str; 4] = [
    "store.db",
    "store.db-journal",
    "store.db-shm",
    "store.db-wal",
];
const DATABASE_PAGE: u64 = 4096;
const KEY_BESIDE: &str = "derived.key";
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

const CREATOR_CASES: u32 = 12;
const LOCK_PATHS_UNLINKED_EVERY: Duration = Duration::from_micros(200);
const CREATE_DIR: &str = "HALVO_FORMAL_CREATE_DIR";
const CREATE_WAITS: &str = "HALVO_FORMAL_CREATE_WAITS";
const AROUND_DIR: &str = "HALVO_FORMAL_AROUND_DIR";
const AROUND_ROLE: &str = "HALVO_FORMAL_AROUND_ROLE";

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
    ArmFault(Fault),
    OpenWithoutItsDatabase { cut: bool },
    LinkBelowSegments,
    DestroyTheRootAgain,
    Append { index_row_fails: bool },
    CreateWhoseRootStepFails,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    DamagedPage,
    QueryOnly,
    KeyAndRootReadOnly,
    RootStep,
    KeyFileStep,
    DatabaseStep,
    BlockedShard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Left {
    root: bool,
    key_file: bool,
    database: bool,
    segment_files: u64,
}

struct Outside {
    files: Vec<(String, Vec<u8>)>,
    dir: tempfile::TempDir,
}

struct InTheWay {
    fault: Fault,
    blocked_shard: Option<(PathBuf, u64)>,
}

fn fault() -> impl Strategy<Value = Fault> {
    prop_oneof![
        Just(Fault::DamagedPage),
        Just(Fault::QueryOnly),
        Just(Fault::KeyAndRootReadOnly),
        Just(Fault::RootStep),
        Just(Fault::KeyFileStep),
        Just(Fault::DatabaseStep),
        Just(Fault::BlockedShard),
    ]
}

fn another_label() -> crate::custody::KeyLabel {
    crate::custody::KeyLabel::new("derived").expect("a label of the listed letters")
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
        3 => fault().prop_map(Op::ArmFault),
        2 => any::<bool>().prop_map(|cut| Op::OpenWithoutItsDatabase { cut }),
        1 => Just(Op::LinkBelowSegments),
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
    stopped: Option<Left>,
    armed: Option<Fault>,
    latest_created_key_file: Option<KeyFile>,
    key_beside: Option<tempfile::TempDir>,
    outside: Option<Outside>,
    segments: Vec<ModelSegment>,
    moves_that_did_not_apply: usize,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.store = None;
        for root in &self.roots {
            crate::store::roots::custody(*root)
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
            stopped: None,
            armed: None,
            latest_created_key_file: None,
            key_beside: None,
            outside: None,
            segments: Vec::new(),
            moves_that_did_not_apply: 0,
        };
        harness.create_store();
        harness
    }

    fn create_store(&mut self) {
        let root = RootId::fresh().unwrap();
        self.roots.push(root);
        let modes_inside: Rc<RefCell<Option<Result<(), String>>>> = Rc::new(RefCell::new(None));
        let look = {
            let (dir, seen) = (self.dir.clone(), Rc::clone(&modes_inside));
            failpoint::at(super::db::SEAM_AFTER_THE_ROOT, move || {
                let read = require_owner_only_modes(&dir).map_err(|failure| failure.to_string());
                seen.borrow_mut().get_or_insert(read);
            })
        };
        let store = EncryptedStore::create(
            &self.dir,
            Box::new(crate::store::roots::custody(root)),
            DEFAULT_RETENTION,
        )
        .expect("create on a free directory");
        drop(look);
        let modes_inside = modes_inside.borrow_mut().take();
        assert!(
            matches!(modes_inside, Some(Ok(()))),
            "while a create stood between its root and its key file: {}",
            match modes_inside {
                Some(Err(failure)) => failure,
                _ => "it never came to that place, so the modes of its files were not read"
                    .to_string(),
            }
        );
        let key_file = KeyFile::read(&self.dir).expect("the fresh store's key file");
        let wrapped = key_file
            .wrapped_store_key()
            .expect("the fresh store's wrapped key decodes");
        assert!(
            !opens_under_an_all_zero_root(key_file.root_id, crate::custody::KeyLabel::STORE, &wrapped),
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
        let beside = tempfile::tempdir().expect("a temp dir for a key file beside the store");
        let custody = crate::store::roots::custody(root);
        let another = custody
            .wrap_store_key(
                &crate::custody::StoreKey::from_bytes([7u8; 32]),
                another_label(),
            )
            .expect("a second key wraps under the root of the store");
        KeyFile::at(&beside.path().join(KEY_BESIDE))
            .and_then(|place| place.publish(&KeyFile::new(&custody, another_label(), &another)))
            .expect("the key file of another label is written beside the store");
        self.key_beside = Some(beside);
        self.latest_created_key_file = Some(key_file);
        self.store = Some(store);
        self.deleted_since_last_create = false;
        self.stopped = None;
        let beside = self.unwrap_of_the_key_beside();
        assert!(
            beside.is_ok(),
            "the key file of another label beside a fresh store does not unwrap under the root of the store: {beside:?}"
        );
    }

    fn open_without_its_database(&mut self, cut: bool) -> Result<(), TestCaseError> {
        if self.deleted_since_last_create || self.store.is_none() {
            return self.does_not_apply();
        }
        self.store = None;
        let aside = tempfile::tempdir().expect("a temp dir for store.db set aside");
        let (database, kept) = (self.dir.join("store.db"), aside.path().join("store.db"));
        let how = if cut { "cut to nothing" } else { "taken away" };
        if cut {
            std::fs::copy(&database, &kept).expect("store.db copies aside");
            OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&database)
                .expect("store.db is cut to nothing");
        } else {
            std::fs::rename(&database, &kept).expect("store.db is taken away");
        }
        let before = listing_of(&self.dir);
        let opened = crate::store::roots::open(&self.dir, DEFAULT_RETENTION).map(drop);
        let after = listing_of(&self.dir);
        if cut {
            std::fs::copy(&kept, &database).expect("store.db is put back");
        } else {
            std::fs::rename(&kept, &database).expect("store.db is put back");
        }
        prop_assert!(
            matches!(opened, Err(StoreError::DatabaseMissing)),
            "an open of a store whose store.db was {} answered {:?} in place of DatabaseMissing",
            how,
            opened
        );
        prop_assert!(
            before == after,
            "an open of a store whose store.db was {} changed the store directory from {:?} to {:?}",
            how,
            before,
            after
        );
        match crate::store::roots::open(&self.dir, DEFAULT_RETENTION) {
            Ok(store) => self.store = Some(store),
            Err(e) => prop_assert!(
                false,
                "with its store.db put back, the store did not reopen: {e}"
            ),
        }
        Ok(())
    }

    fn link_below_segments(&mut self) -> Result<(), TestCaseError> {
        let of_no_segment = segment_path(&self.dir, Uuid::from_bytes([0xff; 16]));
        let shard = of_no_segment.parent().expect("the shard of a segment");
        let applies = cfg!(unix)
            && !self.deleted_since_last_create
            && self.store.is_some()
            && self.outside.is_none()
            && self.dir.join("segments").is_dir()
            && std::fs::symlink_metadata(shard).is_err();
        if !applies {
            return self.does_not_apply();
        }
        let outside = tempfile::tempdir().expect("a temp dir outside the store");
        let name = of_no_segment
            .file_name()
            .expect("the name of a segment file");
        std::fs::write(outside.path().join(name), b"a file of another program")
            .expect("the file outside the store writes");
        link_to_a_directory(outside.path(), shard);
        self.outside = Some(Outside {
            files: files_with_their_bytes(outside.path()),
            dir: outside,
        });
        Ok(())
    }

    fn check_outside(&self) -> Result<(), TestCaseError> {
        let Some(outside) = &self.outside else {
            return Ok(());
        };
        let held = &outside.files;
        let holds = files_with_their_bytes(outside.dir.path());
        prop_assert!(
            holds == *held,
            "the directory outside the store, to which a link below segments leads, held {:?} and now holds {:?}",
            held.iter().map(|(name, bytes)| (name, bytes.len())).collect::<Vec<_>>(),
            holds.iter().map(|(name, bytes)| (name, bytes.len())).collect::<Vec<_>>()
        );
        Ok(())
    }

    fn unwrap_of_the_key_beside(&self) -> Result<(), CustodyError> {
        let beside = self
            .key_beside
            .as_ref()
            .expect("the key file beside the store");
        let key_file = KeyFile::at(&beside.path().join(KEY_BESIDE))
            .and_then(|place| place.read(another_label()))
            .expect("the key file beside the store reads");
        let wrapped = key_file
            .wrapped_store_key()
            .expect("the wrapped key beside the store decodes");
        let custody =
            custody_for(&key_file.locator(&crate::store::roots::roots_of(key_file.root_id)))
                .expect("the dev-fallback custody rebuilds");
        custody
            .unwrap_store_key(&wrapped, another_label())
            .map(drop)
    }

    fn left_on_disk(&self) -> Left {
        let root_id = self
            .latest_created_key_file
            .as_ref()
            .expect("the key file of the store")
            .root_id;
        let stands = |name: &str| std::fs::symlink_metadata(self.dir.join(name)).is_ok();
        Left {
            root: !matches!(crate::store::roots::custody(root_id).root_bytes(), Ok(None)),
            key_file: stands("store.key"),
            database: DATABASE_FILES.iter().any(|name| stands(name)),
            segment_files: files_below(&self.dir.join("segments")),
        }
    }

    fn put_in_the_way(&mut self, fault: Fault) -> Option<InTheWay> {
        let live = !self.deleted_since_last_create;
        let on_disk = self.left_on_disk();
        let mut blocked_shard = None;
        match fault {
            Fault::DamagedPage if live => self.damage_a_page_of_the_events(),
            Fault::QueryOnly if live => self
                .store
                .as_ref()?
                .db()
                .expect("the open store's db")
                .connection()
                .execute_batch("PRAGMA query_only = ON;")
                .expect("the connection of the store takes the read-only setting"),
            Fault::KeyAndRootReadOnly if cfg!(unix) && on_disk.key_file && on_disk.root => {
                set_mode(&self.dir.join("store.key"), 0o400);
                set_mode(&self.root_file(), 0o400);
            }
            Fault::RootStep if live => self.reopen_under_a_custody_whose_root_step_fails_once(),
            Fault::KeyFileStep if on_disk.key_file => failpoint::arm("key_file_destroy"),
            Fault::DatabaseStep if on_disk.database => failpoint::arm("database_destroy"),
            Fault::BlockedShard if cfg!(unix) => {
                let shard = a_shard_with_a_file(&self.dir.join("segments"))?;
                let files = files_below(&shard);
                set_mode(&shard, 0o500);
                blocked_shard = Some((shard, files));
            }
            _ => return None,
        }
        Some(InTheWay {
            fault,
            blocked_shard,
        })
    }

    fn take_out_of_the_way(&self, in_the_way: Option<&InTheWay>) -> Result<Left, TestCaseError> {
        let mut expected = Left::default();
        let Some(put) = in_the_way else {
            return Ok(expected);
        };
        match put.fault {
            Fault::KeyFileStep => {
                prop_assert!(
                    failpoint::hit("key_file_destroy").is_ok(),
                    "delete-all did not reach the removal of its key file"
                );
                expected.key_file = true;
            }
            Fault::DatabaseStep => {
                prop_assert!(
                    failpoint::hit("database_destroy").is_ok(),
                    "delete-all did not reach the removal of its database"
                );
                expected.database = true;
            }
            Fault::RootStep => expected.root = true,
            Fault::BlockedShard => {
                let (shard, files) = put
                    .blocked_shard
                    .as_ref()
                    .expect("the shard that the fault blocked");
                set_mode(shard, 0o700);
                expected.segment_files = *files;
            }
            Fault::DamagedPage | Fault::QueryOnly | Fault::KeyAndRootReadOnly => {}
        }
        Ok(expected)
    }

    fn root_file(&self) -> PathBuf {
        let root_id = self
            .latest_created_key_file
            .as_ref()
            .expect("the key file of the store")
            .root_id;
        crate::store::roots::roots_of(root_id).join(format!("{root_id}.root"))
    }

    fn damage_a_page_of_the_events(&mut self) {
        let store = self.store.take().expect("an open store");
        let page = {
            let db = store.db().expect("the open store's db");
            let page: i64 = db
                .connection()
                .query_row(
                    "SELECT rootpage FROM sqlite_master WHERE type = 'table' AND name = 'events'",
                    [],
                    |r| r.get(0),
                )
                .expect("the root page of the events");
            db.checkpoint_truncate()
                .expect("the log of the store drains into store.db");
            page as u64
        };
        drop(store);
        let mut file = OpenOptions::new()
            .write(true)
            .open(self.dir.join("store.db"))
            .expect("store.db opens for a write");
        file.seek(SeekFrom::Start((page - 1) * DATABASE_PAGE + 100))
            .expect("the place of the damage in store.db");
        file.write_all(&[0xa5; 512])
            .expect("the damage of one page writes");
        file.sync_all().expect("the damaged page syncs");
        drop(file);
        self.store = Some(
            crate::store::roots::open(&self.dir, DEFAULT_RETENTION)
                .expect("a store with one damaged page of its events opens"),
        );
    }

    fn reopen_under_a_custody_whose_root_step_fails_once(&mut self) {
        self.store = None;
        let roots = crate::store::roots::roots_for_store(&self.dir);
        let store = EncryptedStore::open_with(
            &self.dir,
            &roots,
            DEFAULT_RETENTION,
            |locator: &crate::custody::CustodyLocator| {
                let custody: Box<dyn KeyCustody + Send> =
                    Box::new(ACustodyWhoseRootStepFailsOnce {
                        inner: custody_for(locator)?,
                        failed: AtomicBool::new(false),
                    });
                Ok(custody)
            },
        )
        .expect("the store reopens under a custody whose root step fails once");
        self.store = Some(store);
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
            Op::ArmFault(fault) => {
                self.armed = Some(*fault);
                Ok(())
            }
            Op::OpenWithoutItsDatabase { cut } => self.open_without_its_database(*cut),
            Op::LinkBelowSegments => self.link_below_segments(),
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
        if self.store.is_none() {
            return self.does_not_apply();
        }
        let first_call = !self.deleted_since_last_create;
        let live = self
            .segments
            .iter()
            .filter(|s| s.fate == Fate::Live)
            .count() as u64;
        let in_the_way = match self.armed.take() {
            Some(fault) => {
                let put = self.put_in_the_way(fault);
                if put.is_none() {
                    self.moves_that_did_not_apply += 1;
                }
                put
            }
            None => None,
        };
        let way = in_the_way.as_ref().map(|put| put.fault);
        let store = self.store.as_mut().expect("the handle of the store");
        let result = store.crypto_shred_all();
        let expected = self.take_out_of_the_way(in_the_way.as_ref())?;
        for seg in &mut self.segments {
            seg.fate = Fate::Shredded;
        }
        self.deleted_since_last_create = true;
        self.stopped = Some(expected).filter(|left| *left != Left::default());
        if !expected.root {
            self.require_the_root_destroyed()?;
        }
        let store = self.store.as_mut().expect("the handle of the store");
        let write = store
            .write_segment(&mut Cursor::new(b"written after delete-all"))
            .map(drop);
        prop_assert!(
            matches!(write, Err(StoreError::Sealed)),
            "after a delete-all that returned {:?}, a write on its handle answered {:?} in place of Sealed",
            result,
            write
        );
        let on_disk = self.left_on_disk();
        match &result {
            Ok(report) => {
                prop_assert!(
                    expected == Left::default(),
                    "a delete-all with {:?} in its way returned Ok, and the step that failed leaves {:?}",
                    way,
                    expected
                );
                if first_call {
                    prop_assert_eq!(report.segments_shredded, live);
                    prop_assert_eq!(report.keys_destroyed, live + 1);
                } else {
                    prop_assert!(
                        *report == ShredReport::default(),
                        "a second delete-all did not finish with nothing left to shred: {:?}",
                        result
                    );
                }
            }
            Err(StoreError::DeleteIncomplete(report)) if expected != Left::default() => {
                prop_assert!(
                    report.root_destroyed() != on_disk.root
                        && report.key_file_gone != on_disk.key_file
                        && report.database_left == on_disk.database
                        && report.files_left == on_disk.segment_files,
                    "a delete-all that returned an error reported {:?}, and a look at the disk finds {:?}",
                    report,
                    on_disk
                );
                prop_assert!(
                    on_disk == expected,
                    "a delete-all with {:?} in its way left {:?}, and only {:?} could stay, so it skipped a step after the one that failed",
                    way,
                    on_disk,
                    expected
                );
                if expected.root {
                    let root_id = self
                        .latest_created_key_file
                        .as_ref()
                        .expect("the key file of the store")
                        .root_id;
                    prop_assert!(
                        report.root == crate::store::RootReading::Left(Some(root_id)),
                        "a delete-all whose root step failed reported {:?}, which does not name the root of the store as the one that stays",
                        report
                    );
                }
            }
            Err(_) if first_call => prop_assert!(
                false,
                "a delete-all with {:?} in its way returned {:?}",
                way,
                result
            ),
            Err(_) => prop_assert!(
                false,
                "a second delete-all did not finish with nothing left to shred: {:?}",
                result
            ),
        }
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), TestCaseError> {
        self.store = None;
        if !self.deleted_since_last_create {
            widen_the_database_file(&self.dir);
        }
        let reopened = crate::store::roots::open(&self.dir, DEFAULT_RETENTION);
        if !self.deleted_since_last_create {
            match reopened {
                Ok(store) => self.store = Some(store),
                Err(e) => prop_assert!(false, "reopen failed: {e}"),
            }
            return Ok(());
        }
        let key_file_left = self.stopped.is_some_and(|left| left.key_file);
        match reopened.map(drop) {
            Ok(()) => prop_assert!(false, "a deleted store reopened"),
            Err(StoreError::DatabaseMissing) if key_file_left => {}
            Err(StoreError::Sealed) if !key_file_left => {}
            Err(e) => prop_assert!(
                false,
                "after a delete-all that left {:?}, the store reopened as {e:?}; with its key file left it answers DatabaseMissing, and Sealed without it",
                self.stopped.unwrap_or_default()
            ),
        }
        Ok(())
    }

    fn reopen_under_another_root(&mut self) -> Result<(), TestCaseError> {
        if self.deleted_since_last_create {
            return self.does_not_apply();
        }
        let root_id = *self.roots.last().expect("the root id of the store");
        let root = crate::store::roots::custody(root_id)
            .root_bytes()
            .expect("the root of the store reads")
            .expect("the root of the store exists");
        let mut another_root = *root;
        another_root[0] ^= 0x01;
        self.store = None;
        crate::store::roots::custody(root_id)
            .replace_root(&another_root)
            .expect("other bytes are put under the id of the root");
        let under_another_root = crate::store::roots::open(&self.dir, DEFAULT_RETENTION).map(drop);
        crate::store::roots::custody(root_id)
            .replace_root(&root)
            .expect("the bytes of the root are put back");
        let under_its_root = crate::store::roots::open(&self.dir, DEFAULT_RETENTION);
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
        let key_file_left = self.stopped.is_some_and(|left| left.key_file);
        let no_handle_and_no_key_file = self.store.is_none() && !key_file_left;
        if self.deleted_since_last_create && (self.stopped.is_none() || no_handle_and_no_key_file) {
            self.store = None;
            self.create_store();
            return Ok(());
        }
        let refused_root = RootId::fresh().unwrap();
        self.roots.insert(0, refused_root);
        let result = EncryptedStore::create(
            &self.dir,
            Box::new(crate::store::roots::custody(refused_root)),
            DEFAULT_RETENTION,
        );
        prop_assert!(
            matches!(result, Err(StoreError::Locked | StoreError::AlreadyExists)),
            "create on a directory that holds a store did not refuse: {:?}",
            result.err()
        );
        Ok(())
    }

    fn create_that_fails_after_its_database(&mut self) -> Result<(), TestCaseError> {
        if !self.deleted_since_last_create || self.stopped.is_some() {
            return self.does_not_apply();
        }
        self.store = None;
        let root = RootId::fresh().unwrap();
        self.roots.push(root);
        failpoint::arm("db_open");
        let result = EncryptedStore::create(
            &self.dir,
            Box::new(crate::store::roots::custody(root)),
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
        let root_left = crate::store::roots::custody(root)
            .root_bytes()
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
            Box::new(crate::store::roots::custody(root)),
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
        match crate::store::roots::open(&self.dir, DEFAULT_RETENTION) {
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

    fn destroy_the_root_again(&mut self) -> Result<(), TestCaseError> {
        if !self.deleted_since_last_create || self.stopped.is_some_and(|left| left.root) {
            return self.does_not_apply();
        }
        let root = self
            .latest_created_key_file
            .as_ref()
            .expect("the deleted store's key file")
            .root_id;
        require_a_root_that_destroys_again(&crate::store::roots::custody(root))
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
        self.check_outside()?;
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

    fn require_the_root_destroyed(&self) -> Result<(), TestCaseError> {
        let key_file = self
            .latest_created_key_file
            .as_ref()
            .expect("the deleted store's key file");
        let custody =
            custody_for(&key_file.locator(&crate::store::roots::roots_of(key_file.root_id)))
                .expect("the dev-fallback custody rebuilds");
        let wrapped = key_file
            .wrapped_store_key()
            .expect("the retained wrapped key decodes");
        require_a_destroyed_root(custody.as_ref(), &wrapped)?;
        match self.unwrap_of_the_key_beside() {
            Err(CustodyError::RootMissing) => Ok(()),
            Ok(()) => Err(TestCaseError::fail(
                "after delete-all the key file of another label under the root of the store still unwraps",
            )),
            Err(error) => Err(TestCaseError::fail(format!(
                "after delete-all the unwrap of the key file of another label failed with {error:?}, which does not say that the root is missing"
            ))),
        }
    }

    fn check_deleted(&self) -> Result<(), TestCaseError> {
        if !self.stopped.is_some_and(|left| left.root) {
            self.require_the_root_destroyed()?;
        }
        if let Some(left) = self.stopped {
            let on_disk = self.left_on_disk();
            prop_assert!(
                on_disk == left,
                "a delete-all that stopped left {:?}, and the store directory now holds {:?}",
                left,
                on_disk
            );
        }
        for name in names_in(&self.dir) {
            if self.stopped.is_some() {
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
        RootId::fresh().unwrap()
    }
    fn roots(&self) -> Option<&std::path::Path> {
        None
    }

    fn wrap_store_key(
        &self,
        _key: &crate::custody::StoreKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        Ok(WrappedKey(Vec::new()))
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        Ok(())
    }
    fn unwrap_store_key(
        &self,
        _wrapped: &WrappedKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<crate::custody::StoreKey, CustodyError> {
        (self.unwrap)()
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        (self.destroy)()
    }
}

struct ACustodyWhoseRootStepFailsOnce {
    inner: Box<dyn KeyCustody + Send>,
    failed: AtomicBool,
}

impl KeyCustody for ACustodyWhoseRootStepFailsOnce {
    fn kind(&self) -> crate::custody::CustodyKind {
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
        key: &crate::custody::StoreKey,
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
    ) -> Result<crate::custody::StoreKey, CustodyError> {
        self.inner.unwrap_store_key(wrapped, label)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        if self.failed.swap(true, Ordering::SeqCst) {
            self.inner.destroy_root()
        } else {
            Err(CustodyError::RootDenied)
        }
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
    fn roots(&self) -> Option<&std::path::Path> {
        None
    }

    fn wrap_store_key(
        &self,
        _key: &crate::custody::StoreKey,
        _label: crate::custody::KeyLabel,
    ) -> Result<WrappedKey, CustodyError> {
        Ok(WrappedKey(vec![0u8; 72]))
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        Err((self.answer)())
    }
    fn unwrap_store_key(
        &self,
        _wrapped: &WrappedKey,
        _label: crate::custody::KeyLabel,
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
        if meta.file_type().is_symlink() {
            continue;
        }
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

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

fn files_below(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .map(|entry| {
            let entry = entry.expect("an entry below segments");
            let is_dir = entry
                .file_type()
                .expect("the type of an entry below segments")
                .is_dir();
            if is_dir {
                files_below(&entry.path())
            } else {
                1
            }
        })
        .sum()
}

fn listing_of(dir: &Path) -> Vec<(String, u64)> {
    let mut listing = Vec::new();
    let mut entries = vec![dir.to_path_buf()];
    while let Some(path) = entries.pop() {
        let meta = std::fs::symlink_metadata(&path).expect("an entry of the store directory stats");
        if meta.is_dir() {
            let below = std::fs::read_dir(&path).expect("a directory of the store lists");
            entries.extend(below.map(|entry| entry.expect("an entry of the store").path()));
        }
        let len = if meta.is_file() { meta.len() } else { 0 };
        let name = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        listing.push((name, len));
    }
    listing.sort();
    listing
}

fn files_with_their_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .expect("the directory outside the store lists")
        .map(|entry| {
            let entry = entry.expect("an entry outside the store");
            let bytes = std::fs::read(entry.path()).expect("a file outside the store reads");
            (entry.file_name().to_string_lossy().into_owned(), bytes)
        })
        .collect();
    files.sort();
    files
}

#[cfg(unix)]
fn link_to_a_directory(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("a symbolic link below segments");
}

#[cfg(not(unix))]
fn link_to_a_directory(_target: &Path, _link: &Path) {}

fn a_shard_with_a_file(segments: &Path) -> Option<PathBuf> {
    let mut shards: Vec<PathBuf> = std::fs::read_dir(segments)
        .ok()?
        .map(|entry| entry.expect("an entry of segments"))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|shard| files_below(shard) > 0)
        .collect();
    shards.sort();
    shards.into_iter().next()
}

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
        Op::ArmFault(Fault::KeyFileStep),
        Op::ShredAll,
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
    let a_store_with_events = [
        Op::Recreate,
        Op::Write {
            len: SCALED_SEGMENT_LEN,
            expiry: Expiry::Future,
        },
        Op::Append {
            index_row_fails: false,
        },
    ];
    let mut moves = moves.to_vec();
    for (fault, stops) in [
        (Fault::DamagedPage, false),
        (Fault::QueryOnly, false),
        (Fault::RootStep, true),
        (Fault::DatabaseStep, true),
        (Fault::KeyAndRootReadOnly, false),
        (Fault::BlockedShard, true),
    ] {
        let unix_alone = matches!(fault, Fault::KeyAndRootReadOnly | Fault::BlockedShard);
        if unix_alone && !cfg!(unix) {
            continue;
        }
        moves.extend(a_store_with_events.iter().cloned());
        moves.extend([Op::ArmFault(fault), Op::ShredAll]);
        if stops {
            moves.push(Op::ShredAll);
        }
    }
    moves.extend(a_store_with_events.iter().cloned());
    moves.push(Op::Write {
        len: SCALED_SEGMENT_LEN,
        expiry: Expiry::Past,
    });
    if cfg!(unix) {
        moves.push(Op::LinkBelowSegments);
    }
    moves.extend([
        Op::ShredExpired,
        Op::OpenWithoutItsDatabase { cut: false },
        Op::OpenWithoutItsDatabase { cut: true },
        Op::ShredAll,
    ]);
    moves.extend(a_store_with_events.iter().cloned());
    moves.extend([Op::ArmFault(Fault::DatabaseStep), Op::ShredAll, Op::Reopen]);
    moves.extend(a_store_with_events.iter().cloned());
    moves.extend([
        Op::ArmFault(Fault::KeyFileStep),
        Op::ShredAll,
        Op::Reopen,
        Op::Recreate,
    ]);
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

fn creators_config() -> ProptestConfig {
    ProptestConfig {
        cases: CREATOR_CASES,
        ..race_config()
    }
}

#[derive(Debug, Clone)]
struct Creators {
    creators: usize,
    first_waits_before_its_database_step: bool,
    lock_paths_unlinked: bool,
}

fn creators() -> impl Strategy<Value = Creators> {
    (
        2..=MAX_CONTENDERS,
        prop::bool::weighted(0.75),
        prop::bool::weighted(0.75),
    )
        .prop_map(
            |(creators, first_waits_before_its_database_step, lock_paths_unlinked)| Creators {
                creators,
                first_waits_before_its_database_step,
                lock_paths_unlinked,
            },
        )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CreatorAnswer {
    Created { segment: Uuid },
    Closing { files_without_a_name: usize },
    Refused(String),
}

fn creator_refusal(error: &StoreError) -> String {
    match error {
        StoreError::Locked => "locked".to_string(),
        StoreError::AlreadyExists => "already-exists".to_string(),
        StoreError::Io(ErrorKind::PermissionDenied) => "access-denied".to_string(),
        other => format!("{other:?}").replace(' ', ""),
    }
}

fn bytes_of_creator(index: usize) -> Vec<u8> {
    format!("written by creator {index}").into_bytes()
}

fn wait_for(mark: &Path) {
    let started = Instant::now();
    while !mark.exists() && started.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(LOCK_PATHS_UNLINKED_EVERY);
    }
}

fn put_the_mark(mark: &Path) {
    if !mark.exists() {
        std::fs::write(mark, b"").expect("a mark of the race writes");
    }
}

#[cfg(unix)]
fn open_files_without_a_name() -> usize {
    use std::os::unix::fs::MetadataExt;
    let Ok(descriptors) = std::fs::read_dir("/dev/fd") else {
        return 0;
    };
    descriptors
        .filter_map(|descriptor| std::fs::metadata(descriptor.ok()?.path()).ok())
        .filter(|open| open.is_file() && open.nlink() == 0)
        .count()
}

#[cfg(not(unix))]
fn open_files_without_a_name() -> usize {
    0
}

#[cfg(unix)]
#[test]
fn the_look_for_open_files_without_a_name_sees_a_removed_file() {
    let dir = tempfile::tempdir().expect("a temp dir for a file that loses its name");
    let path = dir.path().join("removed-while-open");
    let held = std::fs::File::create(&path).expect("the file is made");
    std::fs::remove_file(&path).expect("the file loses its name");
    let seen = open_files_without_a_name();
    drop(held);
    assert!(
        seen >= 1,
        "a file that was removed while open is not among the open files without a name"
    );
}

#[test]
fn create_a_store_for_parent() {
    let Ok(dir) = std::env::var(CREATE_DIR) else {
        println!(
            "create_a_store_for_parent: {CREATE_DIR} is not set, so no race asked for a creator"
        );
        return;
    };
    let dir = PathBuf::from(dir);
    let index: usize = env_number(CONTEND_INDEX);
    let waits = env_number::<u8>(CREATE_WAITS) == 1;
    let log = dir.join("race.log");
    let custody = crate::store::roots::custody(RootId::fresh().expect("a fresh root id"));
    let _window = waits.then(|| {
        let (held, release) = (dir.join("held"), dir.join("release"));
        failpoint::at(super::db::SEAM_BEFORE_THE_DATABASE_STEP, move || {
            put_the_mark(&held);
            wait_for(&release);
        })
    });
    put_the_mark(&dir.join(format!("ready-{index}")));
    if !waits {
        wait_for(&dir.join("go"));
    }
    match EncryptedStore::create(&dir.join("store"), Box::new(custody), DEFAULT_RETENTION) {
        Ok(mut store) => {
            let (segment, _) = store
                .write_segment(&mut Cursor::new(bytes_of_creator(index)))
                .unwrap_or_else(|e| panic!("creator {index}: write failed with {e}"));
            log_text(
                &log,
                &format!("{index} created {}\n", segment.segment_id.simple()),
            );
            wait_for(&dir.join("close"));
            log_text(
                &log,
                &format!("{index} closing {}\n", open_files_without_a_name()),
            );
            drop(store);
        }
        Err(error) => log_text(
            &log,
            &format!("{index} refused {}\n", creator_refusal(&error)),
        ),
    }
}

fn creator_answers(log: &Path) -> Vec<(usize, CreatorAnswer)> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
        .map(|line| {
            let mut parts = line.split_whitespace();
            let index = parts
                .next()
                .and_then(|s| s.parse().ok())
                .expect("the index of a creator");
            let verb = parts.next();
            let word = parts.next().expect("the word after a creator's answer");
            let answer = match verb {
                Some("created") => CreatorAnswer::Created {
                    segment: Uuid::parse_str(word).expect("the segment of a creator"),
                },
                Some("closing") => CreatorAnswer::Closing {
                    files_without_a_name: word.parse().expect("a count of open files"),
                },
                Some("refused") => CreatorAnswer::Refused(word.to_string()),
                other => panic!("unknown answer of a creator {other:?}"),
            };
            (index, answer)
        })
        .collect()
}

fn spawn_creator(dir: &Path, roots: &Path, index: usize, waits: bool) -> Child {
    Command::new(std::env::current_exe().expect("the test binary path"))
        .args([
            "--exact",
            "store::formal_tests::create_a_store_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CREATE_DIR, dir)
        .env(crate::store::roots::ROOTS_OF_THE_PARENT, roots)
        .env(CONTEND_INDEX, index.to_string())
        .env(CREATE_WAITS, if waits { "1" } else { "0" })
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the creator spawns")
}

fn attend_the_creators(
    dir: &Path,
    case: &Creators,
    children: &mut [Child],
) -> Vec<std::process::ExitStatus> {
    let lock_path = dir.join("store").join("writer.lock");
    let log = dir.join("race.log");
    let started = Instant::now();
    let mut statuses: Vec<Option<std::process::ExitStatus>> = vec![None; children.len()];
    while statuses.iter().any(Option::is_none) {
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "the creators did not finish in time"
        );
        for (child, status) in children.iter_mut().zip(statuses.iter_mut()) {
            if status.is_none() {
                *status = child.try_wait().expect("the creator's status polls");
            }
        }
        let answered: Vec<usize> = creator_answers(&log)
            .into_iter()
            .map(|(index, _)| index)
            .collect();
        let settled = |index: usize| answered.contains(&index) || statuses[index].is_some();
        let ready =
            |index: usize| dir.join(format!("ready-{index}")).exists() || statuses[index].is_some();
        let first_stands_in_its_window =
            !case.first_waits_before_its_database_step || dir.join("held").exists() || settled(0);
        if (0..children.len()).all(ready) && first_stands_in_its_window {
            put_the_mark(&dir.join("go"));
        }
        if (1..children.len()).all(settled) {
            put_the_mark(&dir.join("release"));
        }
        if (0..children.len()).all(settled) {
            put_the_mark(&dir.join("close"));
        }
        if case.lock_paths_unlinked {
            let _ = std::fs::remove_file(&lock_path);
        }
        std::thread::sleep(LOCK_PATHS_UNLINKED_EVERY);
    }
    statuses
        .into_iter()
        .map(|s| s.expect("every status"))
        .collect()
}

#[derive(Default)]
struct CreatorsSeen {
    created: Cell<u64>,
    locked: Cell<u64>,
    already_exists: Cell<u64>,
    access_denied: Cell<u64>,
    other_under_the_unlink: Cell<u64>,
    refused_while_the_first_stood_before_its_database_step: Cell<u64>,
    cases_with_no_creator: Cell<u64>,
    others: std::cell::RefCell<std::collections::BTreeSet<String>>,
}

fn one_more(count: &Cell<u64>) {
    count.set(count.get() + 1);
}

fn one_creators_race(case: &Creators, seen: &CreatorsSeen) -> Result<(), TestCaseError> {
    let dir = tempfile::tempdir().expect("a temp dir for the creators");
    let store_dir = dir.path().join("store");
    let roots = dir.path().join("roots");
    std::fs::create_dir(&store_dir).expect("the empty store directory");
    let mut children: Vec<Child> = (0..case.creators)
        .map(|index| {
            let waits = index == 0 && case.first_waits_before_its_database_step;
            spawn_creator(dir.path(), &roots, index, waits)
        })
        .collect();
    let statuses = attend_the_creators(dir.path(), case, &mut children);
    let answers = creator_answers(&dir.path().join("race.log"));
    let stood_in_its_window = dir.path().join("held").exists();

    for (i, status) in statuses.iter().enumerate() {
        prop_assert!(status.success(), "creator {i} exited with {status}");
    }
    let created: Vec<(usize, Uuid)> = answers
        .iter()
        .filter_map(|(index, answer)| match answer {
            CreatorAnswer::Created { segment } => Some((*index, *segment)),
            _ => None,
        })
        .collect();
    prop_assert!(
        created.len() <= 1,
        "creators {:?} each created a store in the one directory",
        created.iter().map(|(index, _)| *index).collect::<Vec<_>>()
    );
    let under_the_unlink_on_windows = cfg!(windows) && case.lock_paths_unlinked;
    if let Some(&(winner, segment)) = created.first() {
        let without_a_name = answers.iter().find_map(|(index, answer)| match answer {
            CreatorAnswer::Closing {
                files_without_a_name,
            } if *index == winner => Some(*files_without_a_name),
            _ => None,
        });
        prop_assert!(
            without_a_name == Some(0),
            "creator {} got the store and, at its close, held open files that had lost their name: {:?}; another creator removed its database",
            winner,
            without_a_name
        );
        let reopened = EncryptedStore::open(&store_dir, &roots, DEFAULT_RETENTION);
        prop_assert!(
            reopened.is_ok(),
            "after the creators the store of creator {} did not reopen: {:?}",
            winner,
            reopened.as_ref().err()
        );
        let store = reopened.expect("the reopened store");
        let mut read_back = Vec::new();
        let read = store.read_segment(segment, &mut read_back);
        prop_assert!(
            read.is_ok() && read_back == bytes_of_creator(winner),
            "after the creators the segment that creator {} wrote does not read back: {:?}",
            winner,
            read
        );
        drop(store);
        let root_of_the_store = KeyFile::read(&store_dir)
            .expect("the key file of the winner")
            .root_id;
        let mut root_files: Vec<String> = std::fs::read_dir(&roots)
            .expect("the roots directory of the creators lists")
            .map(|entry| {
                entry
                    .expect("an entry of the roots directory")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|name| name.ends_with(".root"))
            .collect();
        root_files.sort();
        prop_assert!(
            root_files == vec![format!("{root_of_the_store}.root")],
            "after the creators the roots directory holds {:?}, and the one store has the root {}",
            root_files,
            root_of_the_store
        );
        for name in names_in(&store_dir) {
            prop_assert!(
                FILES_OF_A_STORE.contains(&name.as_str()),
                "after the creators the store directory holds {name}, which is no file of a store"
            );
        }
    }
    for (index, answer) in &answers {
        let CreatorAnswer::Refused(refusal) = answer else {
            continue;
        };
        let class = match refusal.as_str() {
            "locked" => &seen.locked,
            "already-exists" => &seen.already_exists,
            "access-denied" if cfg!(windows) => &seen.access_denied,
            other => {
                prop_assert!(
                    under_the_unlink_on_windows && other.starts_with("Io("),
                    "creator {} was answered {}, which is no refusal of a second creator",
                    index,
                    other
                );
                seen.others.borrow_mut().insert(other.to_string());
                &seen.other_under_the_unlink
            }
        };
        one_more(class);
        if stood_in_its_window && *index != 0 {
            one_more(&seen.refused_while_the_first_stood_before_its_database_step);
        }
    }
    prop_assert!(
        !case.first_waits_before_its_database_step
            || stood_in_its_window
            || under_the_unlink_on_windows,
        "the first creator did not come to the place between its probe and its database step"
    );
    if created.is_empty() {
        one_more(&seen.cases_with_no_creator);
        prop_assert!(
            under_the_unlink_on_windows,
            "no creator got the empty directory: {:?}",
            answers
        );
    } else {
        one_more(&seen.created);
    }
    Ok(())
}

#[test]
fn at_most_one_creator_gets_an_empty_store_directory() {
    let config = contextualize_config(creators_config());
    let cases = config.cases;
    let seen = CreatorsSeen::default();
    let mut runner = TestRunner::new(config);
    if let Err(failure) = runner.run(&creators(), |case| one_creators_race(&case, &seen)) {
        panic!("{failure}\n{runner}");
    }
    let in_a_window = seen
        .refused_while_the_first_stood_before_its_database_step
        .get();
    let _ = writeln!(
        io::stderr(),
        "check 2 creators in {cases} cases: created={} locked={} already_exists={} access_denied={} other_under_the_unlink={} {:?} refused_while_the_first_stood_before_its_database_step={in_a_window} cases_with_no_creator={}",
        seen.created.get(),
        seen.locked.get(),
        seen.already_exists.get(),
        seen.access_denied.get(),
        seen.other_under_the_unlink.get(),
        seen.others.borrow(),
        seen.cases_with_no_creator.get(),
    );
    assert!(
        cases < CREATOR_CASES || in_a_window > 0,
        "in {cases} cases no creator was refused while the first stood before its database step, so this run cannot tell a lock from no lock"
    );
}

fn direct_answer(result: &halvo_sqlcipher::rusqlite::Result<()>) -> String {
    match result {
        Ok(()) => "got-in".to_string(),
        Err(e) => format!("refused-{:?}", e.sqlite_error_code()),
    }
}

#[test]
fn go_around_the_directory_lock_for_parent() {
    let Ok(dir) = std::env::var(AROUND_DIR) else {
        println!("go_around_the_directory_lock_for_parent: {AROUND_DIR} is not set, so no parent asked for a process around the lock");
        return;
    };
    let dir = PathBuf::from(dir);
    let role = std::env::var(AROUND_ROLE).expect("the role of the process around the lock");
    let store_dir = dir.join("store");
    let key_file = KeyFile::read(&store_dir).expect("the key file of the store reads");
    let wrapped = key_file
        .wrapped_store_key()
        .expect("the wrapped store key decodes");
    let store_key = custody_for(&key_file.locator(&dir.join("roots")))
        .and_then(|custody| custody.unwrap_store_key(&wrapped, crate::custody::KeyLabel::STORE))
        .expect("the store key unwraps for the process around the lock");
    let conn = Connection::open(store_dir.join("store.db")).expect("store.db opens by its name");
    conn.busy_timeout(Duration::ZERO)
        .expect("the connection waits for no lock");
    conn.execute_batch(&key_statement(store_key.expose()))
        .expect("the connection takes the store key");
    match role.as_str() {
        "holds" => {
            conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE;")
                .expect("the connection asks for the database alone");
            conn.query_row("SELECT count(*) FROM segment_keys", [], |r| {
                r.get::<_, i64>(0)
            })
            .expect("the process around the lock reads a database that no one holds");
            conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;")
                .expect("the process around the lock takes the database");
            put_the_mark(&dir.join("holding"));
            wait_for(&dir.join("stop"));
        }
        "reads" => {
            let read = conn
                .query_row("SELECT count(*) FROM segment_keys", [], |r| {
                    r.get::<_, i64>(0)
                })
                .map(drop);
            let write = conn.execute_batch("BEGIN IMMEDIATE; COMMIT;");
            std::fs::write(
                dir.join("answers"),
                format!(
                    "read={} write={}",
                    direct_answer(&read),
                    direct_answer(&write)
                ),
            )
            .expect("the answers of the process around the lock write");
        }
        other => panic!("unknown role of a process around the lock {other:?}"),
    }
}

fn spawn_around_the_lock(dir: &Path, role: &str) -> Child {
    Command::new(std::env::current_exe().expect("the test binary path"))
        .args([
            "--exact",
            "store::formal_tests::go_around_the_directory_lock_for_parent",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(AROUND_DIR, dir)
        .env(AROUND_ROLE, role)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the process around the lock spawns")
}

#[test]
fn a_process_that_goes_around_the_directory_lock_is_refused_at_the_database() {
    let dir = tempfile::tempdir().expect("a temp dir for the store");
    let store_dir = dir.path().join("store");
    let roots = dir.path().join("roots");
    let root = RootId::fresh().expect("a fresh root id");
    let mut store = EncryptedStore::create(
        &store_dir,
        Box::new(crate::custody::dev_fallback::DevFallbackCustody::new(
            root,
            roots.clone(),
        )),
        DEFAULT_RETENTION,
    )
    .expect("the store creates");
    store
        .write_segment(&mut Cursor::new(
            b"written before the process around the lock",
        ))
        .expect("the holder writes");

    let reader = spawn_around_the_lock(dir.path(), "reads")
        .wait()
        .expect("the process that reads the database itself ends");
    let answers = std::fs::read_to_string(dir.path().join("answers")).unwrap_or_default();
    let holder_wrote_after = store
        .write_segment(&mut Cursor::new(
            b"written after the process around the lock",
        ))
        .map(drop);
    drop(store);

    let mut holder = spawn_around_the_lock(dir.path(), "holds");
    let started = Instant::now();
    while !dir.path().join("holding").exists() {
        let ended = holder.try_wait().expect("the holder's status polls");
        assert!(
            ended.is_none() && started.elapsed() < Duration::from_secs(60),
            "the process that holds the database itself did not come to hold it: {ended:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let asked_its_custody = Cell::new(false);
    let while_held = EncryptedStore::open_with(
        &store_dir,
        &roots,
        DEFAULT_RETENTION,
        |locator: &crate::custody::CustodyLocator| {
            asked_its_custody.set(true);
            custody_for(locator)
        },
    )
    .map(drop);
    put_the_mark(&dir.path().join("stop"));
    let holder = holder.wait().expect("the holder ends");
    let after =
        EncryptedStore::open(&store_dir, &roots, DEFAULT_RETENTION).and_then(|mut store| {
            store
                .write_segment(&mut Cursor::new(b"written after the holder left"))
                .map(drop)
        });
    crate::custody::dev_fallback::DevFallbackCustody::new(root, roots)
        .destroy_root()
        .expect("the root of the store is destroyed");

    let _ = writeln!(
        io::stderr(),
        "check 2 around the directory lock: {answers} open_while_another_held_the_database={while_held:?} asked_its_custody={}",
        asked_its_custody.get()
    );
    assert!(
        reader.success() && holder.success(),
        "a process around the lock exited with {reader} and {holder}"
    );
    assert!(
        answers.starts_with("read=refused-") && answers.contains(" write=refused-"),
        "a process that opened store.db itself, without the directory lock, was answered {answers:?} while another process held the store"
    );
    assert!(
        holder_wrote_after.is_ok(),
        "after a process tried the database itself, the holder of the store could not write: {holder_wrote_after:?}"
    );
    assert!(
        matches!(&while_held, Err(error) if writer_refusal(error).is_some()),
        "an open while another process held the database itself answered {while_held:?}, which is no refusal of a second writer"
    );
    assert!(
        !asked_its_custody.get(),
        "an open that was refused while another process held the database itself had asked its custody"
    );
    assert!(
        after.is_ok(),
        "after the process that held the database itself left, the store did not open and write: {after:?}"
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
    SharingViolation,
}

fn writer_refusal(error: &StoreError) -> Option<Refusal> {
    match error {
        StoreError::Locked => Some(Refusal::Locked),
        StoreError::Io(ErrorKind::PermissionDenied) if cfg!(windows) => Some(Refusal::AccessDenied),
        StoreError::Io(kind) if cfg!(windows) && format!("{kind:?}") == "Uncategorized" => {
            Some(Refusal::SharingViolation)
        }
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
        Refusal::SharingViolation => "refused-sharing-violation",
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
                Some("refused-sharing-violation") => Event::Refused(Refusal::SharingViolation),
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
            let opened = EncryptedStore::open_with(
                &store_dir,
                &crate::store::roots::roots_for_store(&store_dir),
                DEFAULT_RETENTION,
                |locator: &crate::custody::CustodyLocator| {
                    asked_its_custody.set(true);
                    custody_for(locator)
                },
            );
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
        .env(
            crate::store::roots::ROOTS_OF_THE_PARENT,
            crate::store::roots::roots_for_store(&dir.join("store")),
        )
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
    sharing_violation: Cell<u64>,
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
                Refusal::SharingViolation => &self.sharing_violation,
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
    let root = RootId::fresh().unwrap();
    drop(
        EncryptedStore::create(
            &store_dir,
            Box::new(crate::store::roots::custody(root)),
            DEFAULT_RETENTION,
        )
        .expect("the race's store creates"),
    );

    let statuses = run_race(dir.path(), race);
    let text = std::fs::read_to_string(dir.path().join("race.log")).unwrap_or_default();
    crate::store::roots::custody(root)
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
        "check 2 refusals in {cases} cases: locked={} access_denied={} sharing_violation={} while_another_contender_held_the_store={while_held} before_the_custody_through_a_whole_hold={through_a_hold}",
        refusals.locked.get(),
        refusals.access_denied.get(),
        refusals.sharing_violation.get(),
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
