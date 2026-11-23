//! The adapter: the daemon's store port over the encrypted store, the one
//! code through which any caller outside the store module opens a store
//! (ruling PR-30 PR1). It makes a data directory's store a [`StoreHost`], hands the owner a
//! [`CaptureStore`] or a [`MaintainStore`], maps every error of an open to
//! the port's [`OpenFault`] by the store's own variants, and gives a scratch
//! directory the gate's calls.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use super::db::{EncryptedStore, KeyFile};
use super::durable::{self, Dir, OsFs};
use super::KindTotal;
use super::{Layout, OpenFor, RootReading, StoreError, DEFAULT_RETENTION};
use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::{CustodyError, CustodyKind, KeyCustody, KeyLabel, RootId};
use crate::daemon::custody_policy::Custody;
use crate::daemon::port::{
    Access, ByteBound, CaptureStore, DataDirStore, EntryKind, MaintFault, MaintainStore, OpenFault,
    Opened, Ordinal, Pass, Probe, RootGuard, ScratchDir, ScratchStore, SegmentWrite, StoreHost,
    StoreId, Usage, WriteFault, Written,
};
use crate::daemon::{EntriesLeft, TailAnswer, TailReport, TailRoot};

struct Adapter {
    store: EncryptedStore,
    /// The id of the store of this handle, as its open or its create read it.
    #[cfg(feature = "r2")]
    id: StoreId,
}

impl MaintainStore for Adapter {
    fn expire_due(&mut self, now: SystemTime, max_segments: usize) -> Result<Pass, MaintFault> {
        self.store
            .expire_due(now, max_segments)
            .map(pass)
            .map_err(maint_fault)
    }

    fn evict_to(&mut self, bound: ByteBound, max_segments: usize) -> Result<Pass, MaintFault> {
        self.store
            .evict_to(bound.bytes(), max_segments)
            .map(pass)
            .map_err(maint_fault)
    }

    fn usage(&mut self) -> Result<Usage, MaintFault> {
        self.store
            .usage()
            .map(|usage| Usage {
                disk_bytes: usage.disk_bytes,
                logical_bytes: usage.logical_bytes,
            })
            .map_err(maint_fault)
    }

    fn delete_all(&mut self) -> TailAnswer {
        tail_answer(self.store.delete_all())
    }
}

impl CaptureStore for Adapter {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        let kinds: Vec<KindTotal<'static>> = write
            .entries()
            .iter()
            .map(|entry| KindTotal {
                kind: kind_text(entry.kind),
                events: entry.events,
                bytes: entry.bytes,
            })
            .collect();
        let segment_id = write.segment_id().uuid();
        let stored = super::SegmentWrite {
            segment_id: &segment_id,
            day: super::UtcDay::from_days_since_epoch(write.day().days_since_epoch()),
            first_capture: write.first_capture(),
            last_capture: write.last_capture(),
            kinds: &kinds,
            payload: write.payload(),
        };
        match self.store.write_batch(stored) {
            Ok(written) => Ok(Written {
                ordinal: Ordinal::new(written.ordinal.get()),
                logical_bytes: written.logical_bytes,
                disk_bytes: written.disk_bytes,
            }),
            Err(fault) => Err(write_fault(fault)),
        }
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        free_bytes(self.store.dir())
    }

    #[cfg(feature = "r2")]
    fn read_segment(
        &mut self,
        from: Ordinal,
    ) -> Result<crate::daemon::port::SegmentRead, MaintFault> {
        read::first_at(&self.store, self.id, from).map_err(maint_fault)
    }

    fn inventory(&mut self) -> Result<Vec<halvo_control::model::InventoryRow>, MaintFault> {
        let rows = self.store.inventory().map_err(maint_fault)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(halvo_control::model::InventoryRow {
                    day: halvo_control::model::UtcDay::from_days_since_epoch(
                        row.day.days_since_epoch(),
                    )
                    .ok()?,
                    modality: row.kind.parse().ok()?,
                    events: row.events,
                    bytes: row.bytes,
                })
            })
            .collect())
    }
}

/// The read of the cursor on the encrypted store.
#[cfg(feature = "r2")]
mod read {
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};

    use halvo_sqlcipher::rusqlite::OptionalExtension;
    use zeroize::{Zeroize, Zeroizing};

    use crate::daemon::port::{
        DataDirStore, LiveSegment, Ordinal, SegmentRead, StoreId, PAYLOAD_MAX_BYTES,
    };
    use crate::store::db::{db_error, EncryptedStore};
    use crate::store::expiry::expiry_instant;
    use crate::store::{StoreError, StoreResult};

    const NANOS_PER_SECOND: i64 = 1_000_000_000;

    /// How many times a read of this process met a live key row whose
    /// segment could not be read.
    static DAMAGED: AtomicU64 = AtomicU64::new(0);

    impl DataDirStore {
        /// How many times the cursor's reads of this process passed a
        /// damaged segment: one whose key row stands while its file is gone
        /// or does not authenticate. Such a segment answers no line, so this
        /// count is all that tells of it. A segment counts once for each
        /// read that meets it.
        pub fn damaged_reads() -> u64 {
            DAMAGED.load(Ordering::Relaxed)
        }
    }

    /// A writer into a buffer that never makes it grow: it refuses the bytes
    /// that the capacity of the buffer does not hold, so no growth leaves a
    /// copy of plain bytes behind that nothing wipes.
    struct Within<'a>(&'a mut Vec<u8>);

    impl io::Write for Within<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.0.capacity() - self.0.len() {
                return Err(io::Error::from(io::ErrorKind::FileTooLarge));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// What `store`, whose id is `id`, answers to a read at `from`: its id
    /// and the highest ordinal that it gave, and the first row at or after
    /// `from` that holds its key, with the plain lines of its segment and
    /// its expiry instant. A row that expiry or eviction shredded holds no
    /// key and is passed with no byte of its file read; an ask above every
    /// row reads no file. On a read from ordinal 0 that finds such a row, the
    /// answer also names, in ascending order, every row above it that holds
    /// no key: a segment that expiry, eviction or the open-time reconcile
    /// shredded out of the order of its ordinal.
    ///
    /// The call reads one segment at most: two queries on the key rows, a
    /// third on a read from ordinal 0, and one file. It takes no lock and no
    /// transaction and writes no file. The
    /// plain bytes stand in one buffer that is wiped when it drops, made
    /// with the capacity of the largest payload before the read and never
    /// grown.
    ///
    /// A row whose file is gone or does not authenticate is damage: it
    /// answers its ordinal and its expiry with no line, and the read counts
    /// it, so the caller goes on at the next ordinal. A database error, a
    /// handle on which a delete began and every other error of the file
    /// answer the error and no line.
    pub(super) fn first_at(
        store: &EncryptedStore,
        id: StoreId,
        from: Ordinal,
    ) -> StoreResult<SegmentRead> {
        let keys = store.db()?.connection();
        let highest: Option<i64> = keys
            .query_row("SELECT max(id) FROM segment_keys", [], |row| row.get(0))
            .map_err(db_error)?;
        let ordinal = |row: i64| Ordinal::new(crate::store::Ordinal::of_row(row).get());
        let mut read = SegmentRead {
            store: id,
            highest: highest.map(ordinal),
            segment: None,
            gone: Vec::new(),
        };
        let Ok(from) = i64::try_from(from.get()) else {
            return Ok(read);
        };
        let keyed: Option<(i64, String, i64)> = keys
            .query_row(
                "SELECT id, segment_id, first_capture FROM segment_keys \
                 WHERE id >= ?1 AND wrapped_key IS NOT NULL ORDER BY id LIMIT 1",
                [from],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(db_error)?;
        let Some((row, segment_id, first_capture)) = keyed else {
            return Ok(read);
        };
        if from == 0 {
            read.gone = keys
                .prepare(
                    "SELECT id FROM segment_keys WHERE id > ?1 AND wrapped_key IS NULL ORDER BY id",
                )
                .and_then(|mut keyless| {
                    keyless
                        .query_map([row], |row| row.get::<_, i64>(0))?
                        .map(|id| id.map(ordinal))
                        .collect()
                })
                .map_err(db_error)?;
        }
        let mut lines = Zeroizing::new(Vec::with_capacity(PAYLOAD_MAX_BYTES));
        let file = match uuid::Uuid::parse_str(&segment_id) {
            Ok(segment_id) => store.read_segment(segment_id, &mut Within(&mut lines)),
            Err(_) => Err(StoreError::SegmentMissing),
        };
        match file {
            Ok(_) => {}
            Err(StoreError::SegmentMissing | StoreError::Corrupt(_)) => {
                lines.zeroize();
                DAMAGED.fetch_add(1, Ordering::Relaxed);
            }
            Err(fault) => return Err(fault),
        }
        read.segment = Some(LiveSegment {
            ordinal: ordinal(row),
            lines,
            expiry: expiry_instant(first_capture, store.retention())
                .saturating_mul(NANOS_PER_SECOND),
        });
        Ok(read)
    }
}

fn pass(pass: super::Pass) -> Pass {
    Pass {
        shredded: pass
            .shredded
            .into_iter()
            .map(|ordinal| Ordinal::new(ordinal.get()))
            .collect(),
        more_due: pass.more_due,
        owed: pass.owed,
    }
}

fn kind_text(kind: EntryKind) -> &'static str {
    #[cfg(feature = "r2")]
    if kind == EntryKind::RUN_MARK {
        return crate::floor::record::RUN_MARK_KIND;
    }
    kind.as_str()
}

fn write_fault(fault: super::WriteFault) -> WriteFault {
    match fault {
        super::WriteFault::AlreadyWritten { ordinal } => WriteFault::AlreadyWritten {
            ordinal: Ordinal::new(ordinal.get()),
        },
        super::WriteFault::Refused(_) => WriteFault::Refused,
        super::WriteFault::Io(kind) => WriteFault::Io(kind),
        super::WriteFault::Db(code) => WriteFault::Db(code),
        super::WriteFault::Sealed => WriteFault::Sealed,
        super::WriteFault::MaintenanceOnly => WriteFault::Sealed,
    }
}

fn maint_fault(e: StoreError) -> MaintFault {
    match e {
        StoreError::Io(kind) => MaintFault::Io(kind),
        StoreError::Db(code) => MaintFault::Db(code),
        StoreError::CheckpointIncomplete { .. } => MaintFault::CheckpointIncomplete,
        StoreError::SegmentMissing => MaintFault::SegmentMissing,
        _ => MaintFault::Io(io::ErrorKind::Other),
    }
}

fn open_fault(e: &StoreError) -> OpenFault {
    match e {
        StoreError::Io(io::ErrorKind::StorageFull) => OpenFault::NoSpace,
        StoreError::Io(io::ErrorKind::Unsupported | io::ErrorKind::InvalidInput) => {
            OpenFault::IoNotRetried
        }
        StoreError::Io(_)
        | StoreError::Db(_)
        | StoreError::Seal
        | StoreError::CheckpointIncomplete { .. }
        | StoreError::SegmentMissing
        | StoreError::AlreadyExists
        | StoreError::DeleteIncomplete(_) => OpenFault::Io,
        StoreError::Locked => OpenFault::Locked,
        StoreError::Obstructed { .. } | StoreError::Sealed => OpenFault::Obstructed,
        StoreError::MaintenanceOnly => OpenFault::Obstructed,
        StoreError::Corrupt(_)
        | StoreError::DatabaseMissing
        | StoreError::DatabaseUnreadable
        | StoreError::SettingNotInEffect(_) => OpenFault::Database,
        StoreError::KeyFileUnparseable | StoreError::KeyFileOfAnotherLabel => OpenFault::KeyFile,
        StoreError::NewerStore { .. } => OpenFault::NewerStore,
        StoreError::OlderStore { .. } => OpenFault::OlderStore,
        StoreError::NotImplemented(_) => OpenFault::CustodyKind,
        StoreError::Custody(custody) => custody_fault(custody),
    }
}

fn custody_fault(e: &CustodyError) -> OpenFault {
    match e {
        CustodyError::RootMissing => OpenFault::RootMissing,
        CustodyError::RootCorrupt | CustodyError::Crypto(_) => OpenFault::RootCorrupt,
        CustodyError::RootDenied | CustodyError::NoRootDirectory => OpenFault::RootDenied,
        CustodyError::RootLocked(_) => OpenFault::CustodyLocked,
        CustodyError::CustodyKindUnsupported(_) | CustodyError::NotImplemented(_) => {
            OpenFault::CustodyKind
        }
        CustodyError::WrapAuthFailed => OpenFault::WrapAuth,
        CustodyError::RootExists | CustodyError::Os(_) => OpenFault::Io,
    }
}

fn custody_of(kind: CustodyKind) -> Custody {
    match kind {
        CustodyKind::DevFallback => Custody::RootFile,
        CustodyKind::Dpapi => Custody::Dpapi,
        CustodyKind::SecureEnclave => Custody::SecureEnclave,
    }
}

fn store_id(root: RootId) -> StoreId {
    StoreId::from_bytes(*root.as_bytes())
}

fn probe_at(store_dir: &Path) -> Probe {
    let read =
        KeyFile::at(&store_dir.join("store.key")).and_then(|place| place.read(KeyLabel::STORE));
    match read {
        Ok(key_file) => Probe::KeyFile {
            custody: custody_of(key_file.custody_kind),
            id: store_id(key_file.root_id),
        },
        Err(StoreError::Sealed | StoreError::Io(io::ErrorKind::NotFound)) => {
            let stands = |name: &str| std::fs::symlink_metadata(store_dir.join(name)).is_ok();
            match (
                stands("create.pending"),
                stands("segments"),
                stands("store.db"),
            ) {
                (true, false, _) => Probe::UnfinishedCreate,
                (_, true, _) | (false, false, true) => Probe::KeylessStore,
                (false, false, false) => Probe::NoKeyFile,
            }
        }
        Err(e) => Probe::Fault {
            fault: open_fault(&e),
            id: None,
        },
    }
}

fn key_id(store_dir: &Path) -> Option<StoreId> {
    match probe_at(store_dir) {
        Probe::KeyFile { id, .. } => Some(id),
        Probe::NoKeyFile | Probe::KeylessStore | Probe::UnfinishedCreate | Probe::Fault { .. } => {
            None
        }
    }
}

fn custody_for_create(
    custody: Custody,
    roots: &Path,
) -> Result<(Box<dyn KeyCustody + Send>, RootId), OpenFault> {
    let root_id = RootId::fresh().map_err(|e| custody_fault(&e))?;
    match custody {
        Custody::RootFile => Ok((
            Box::new(DevFallbackCustody::new(root_id, roots.to_path_buf())),
            root_id,
        )),
        #[cfg(target_os = "windows")]
        Custody::Dpapi => Ok((
            Box::new(crate::custody::windows::DpapiCustody::new(
                root_id,
                roots.to_path_buf(),
            )),
            root_id,
        )),
        #[cfg(not(target_os = "windows"))]
        Custody::Dpapi => Err(OpenFault::CustodyKind),
        Custody::SecureEnclave => Err(OpenFault::CustodyKind),
    }
}

fn opened(store: EncryptedStore, id: StoreId, access: Access) -> Opened {
    let adapter = Adapter {
        store,
        #[cfg(feature = "r2")]
        id,
    };
    match access {
        Access::ReadWrite => Opened::ReadWrite {
            store: Box::new(adapter),
            id,
        },
        Access::MaintenanceOnly => Opened::MaintenanceOnly {
            store: Box::new(adapter),
            id,
        },
    }
}

fn open_for(access: Access) -> OpenFor {
    match access {
        Access::ReadWrite => OpenFor::Write,
        Access::MaintenanceOnly => OpenFor::Maintenance,
    }
}

fn open_at(store_dir: &Path, roots: &Path, access: Access) -> Opened {
    let id = key_id(store_dir);
    match (
        EncryptedStore::open(store_dir, roots, DEFAULT_RETENTION, open_for(access)),
        id,
    ) {
        (Ok(store), Some(id)) => opened(store, id, access),
        (Ok(_), None) => Opened::Fault {
            fault: OpenFault::KeyFile,
            id: None,
        },
        (Err(e), id) => Opened::Fault {
            fault: open_fault(&e),
            id,
        },
    }
}

fn create_at(store_dir: &Path, roots: &Path, custody: Custody) -> Opened {
    let (custody, root_id) = match custody_for_create(custody, roots) {
        Ok(made) => made,
        Err(fault) => return Opened::Fault { fault, id: None },
    };
    match EncryptedStore::create(store_dir, custody, DEFAULT_RETENTION) {
        Ok(store) => opened(store, store_id(root_id), Access::ReadWrite),
        Err(StoreError::AlreadyExists) => open_at(store_dir, roots, Access::ReadWrite),
        Err(e) => Opened::Fault {
            fault: open_fault(&e),
            id: None,
        },
    }
}

impl StoreHost for DataDirStore {
    fn probe(&self) -> Probe {
        probe_at(&self.store_dir())
    }

    fn open(&self, access: Access) -> Opened {
        open_at(&self.store_dir(), &self.roots(), access)
    }

    fn create(&self, custody: Custody) -> Opened {
        create_at(&self.store_dir(), &self.roots(), custody)
    }

    fn delete(&self) -> TailAnswer {
        let dir = resolved_data_dir(self.path()).unwrap_or_else(|_| self.path().to_path_buf());
        tail_answer(EncryptedStore::delete_data_dir(&dir, Layout::DataDirectory))
    }
}

impl DataDirStore {
    /// Flushes the folder `folder` of the data directory to the drive
    /// through the store's durable-write helper, whose directory flush makes
    /// no call on Windows. A link at the data directory's own name is
    /// followed, and none at the folder's.
    pub fn sync_folder(&self, folder: &Path) -> io::Result<()> {
        Dir::open(&resolved_data_dir(self.path())?.join(folder))?.sync()
    }

    /// Removes the file `name` of the data directory, when it stands, and
    /// flushes the data directory to the drive. A link at the data
    /// directory's own name is followed.
    pub fn remove_durably(&self, name: &str) -> io::Result<()> {
        let dir = Dir::open(&resolved_data_dir(self.path())?)?;
        durable::remove_durably(&OsFs, &dir, OsStr::new(name))
    }

    /// Destroys the key file at `file`, a path inside the data directory,
    /// through the store's key-file helper: its zero-fill, then its removal.
    /// True when no entry stands at its name after the call.
    pub fn destroy_key_file(&self, file: &Path) -> bool {
        let path = self.path().join(file);
        if let Ok(place) = KeyFile::at(&path) {
            let _ = place.destroy();
        }
        std::fs::symlink_metadata(&path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
    }

    /// Whether the store's lock file stands in the store folder, read by the
    /// reader of that name that the tail asks behind its close too: only no
    /// such entry, of the lock file or of a folder above it,
    /// reads as gone. A link at the data directory's own name is followed.
    #[cfg(windows)]
    pub fn lock_file_stands(&self) -> bool {
        let store = resolved_data_dir(self.path())
            .and_then(|dir| Dir::open(&dir))
            .and_then(|dir| dir.open_dir(OsStr::new("store")));
        match store {
            Ok(store) => super::lock::DirLock::file_stands_in(&store),
            Err(e) => e.kind() != io::ErrorKind::NotFound,
        }
    }
}

#[cfg(all(test, target_os = "macos", debug_assertions))]
impl DataDirStore {
    /// The plaintext of every segment that the store of this data directory
    /// recorded, the oldest first capture first, or `None` for a segment that
    /// no longer decrypts because its key is gone. It opens the store for
    /// maintenance alone, so no other handle may hold it.
    pub(crate) fn read_back(&self) -> Vec<Option<Vec<u8>>> {
        let store = EncryptedStore::open(
            &self.store_dir(),
            &self.roots(),
            DEFAULT_RETENTION,
            OpenFor::Maintenance,
        )
        .unwrap();
        let ids: Vec<String> = {
            let db = store.db().unwrap();
            let mut rows = db
                .connection()
                .prepare("SELECT segment_id FROM segment_keys ORDER BY first_capture, id")
                .unwrap();
            let ids = rows
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            ids
        };
        ids.iter()
            .map(|id| {
                let mut plaintext = Vec::new();
                match store.read_segment(uuid::Uuid::parse_str(id).unwrap(), &mut plaintext) {
                    Ok(_) => Some(plaintext),
                    Err(StoreError::Sealed) => None,
                    Err(e) => panic!("segment {id} did not read: {e}"),
                }
            })
            .collect()
    }
}

impl crate::daemon::DurableState {
    /// A writer of the state file through the store's durable-write helper.
    pub fn writer() -> Box<dyn crate::daemon::state::StateWrite> {
        Box::new(super::state_file::DurableStateWrite::new())
    }
}

impl ScratchDir {
    /// `--verify-store`: the measurements on a fresh store made in this
    /// directory that holds `marker`, as the line that the gate judges, or
    /// the error's text.
    pub fn measure_store(&self, marker: &str) -> Result<String, String> {
        super::measure_store(self.path(), marker).map_err(|e| e.to_string())
    }

    /// `--verify-deletion`: the measurements on a fresh store made in this
    /// directory, as the line that the gate judges, or the error's text.
    pub fn measure_deletion(&self) -> Result<String, String> {
        super::measure_deletion(self.path()).map_err(|e| e.to_string())
    }

    /// The delete tail of `--delete-all --store-dir` (contracts.md C4 item
    /// 9) on this directory, with the path as the command line gave it.
    pub fn delete_tail(&self) -> TailAnswer {
        tail_answer(EncryptedStore::delete_data_dir(
            self.path(),
            Layout::ScratchStore,
        ))
    }

    /// Makes a store in this directory under this system's own custody, with
    /// its root file under `roots` inside it, and the guard of that root,
    /// held from right after the create.
    pub fn create(&self) -> Result<ScratchStore, OpenFault> {
        let roots = self.roots();
        let own = match crate::daemon::custody_policy::System::this() {
            crate::daemon::custody_policy::System::Macos => Custody::RootFile,
            crate::daemon::custody_policy::System::Windows => Custody::Dpapi,
        };
        let (custody, root_id) = custody_for_create(own, &roots)?;
        let store = EncryptedStore::create(self.path(), custody, DEFAULT_RETENTION)
            .map_err(|e| open_fault(&e))?;
        let root = RootGuard::new(Box::new(move || destroy_root(own, root_id, &roots)));
        Ok(ScratchStore {
            store: Box::new(Adapter {
                store,
                #[cfg(feature = "r2")]
                id: store_id(root_id),
            }),
            root,
        })
    }
}

fn destroy_root(custody: Custody, root_id: RootId, roots: &Path) -> io::Result<()> {
    let destroyed = match custody {
        Custody::RootFile => DevFallbackCustody::new(root_id, roots.to_path_buf()).destroy_root(),
        #[cfg(target_os = "windows")]
        Custody::Dpapi => {
            crate::custody::windows::DpapiCustody::new(root_id, roots.to_path_buf()).destroy_root()
        }
        #[cfg(not(target_os = "windows"))]
        Custody::Dpapi => Ok(()),
        Custody::SecureEnclave => Ok(()),
    };
    destroyed.map_err(|e| io::Error::other(e.to_string()))
}

fn tail_answer(deleted: super::StoreResult<super::DeleteReport>) -> TailAnswer {
    match deleted {
        Ok(report) => TailAnswer::Finished(tail_report(&report)),
        Err(StoreError::DeleteIncomplete(report)) => TailAnswer::NotFinished(tail_report(&report)),
        Err(StoreError::Locked) => TailAnswer::Held,
        Err(error) => TailAnswer::Failed(error.to_string()),
    }
}

fn tail_report(report: &super::DeleteReport) -> TailReport {
    TailReport {
        keyed_segments: report.keyed_segments,
        root: match report.root {
            RootReading::Destroyed => TailRoot::Destroyed,
            RootReading::Left(id) => TailRoot::Left(id.map(|id| id.to_string())),
            RootReading::Unnamed => TailRoot::Unnamed,
        },
        root_files_left: report.roots.names.clone(),
        others_left: EntriesLeft {
            root_shaped: report.roots.others_left.root_shaped,
            directories: report.roots.others_left.directories,
            other: report.roots.others_left.other,
        },
        roots_listed: report.roots.listed,
        roots_at_their_path: report.roots.at_its_path,
        key_file_gone: report.key_file_gone,
        database_left: report.database_left,
        files_left: report.files_left,
        not_followed: report.not_followed,
        lock_file_kept: report.lock_file_kept,
    }
}

#[cfg(target_os = "macos")]
fn free_bytes(dir: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `stat` points to writable memory of the size of `libc::statvfs`.
    let status = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `statvfs` returned 0, so it filled `stat`.
    let stat = unsafe { stat.assume_init() };
    Ok(u64::from(stat.f_bavail).saturating_mul(stat.f_frsize))
}

#[cfg(not(target_os = "macos"))]
fn free_bytes(_dir: &Path) -> io::Result<u64> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

use crate::custody::{custody_for, RootsHandle, StoreKey};
use crate::daemon::port::DerivedKeyFault;

const DERIVED_LABEL: KeyLabel = KeyLabel::new("derived").expect("a label of the listed letters");

impl DataDirStore {
    /// The key of the derived store. Its wrap lies in derived/master.key,
    /// made under the root that store/store.key names with the label
    /// `derived`, so the delete that destroys the capture store's root ends
    /// this key too. Where no key file stands, the call makes 32 random
    /// bytes, wraps them, makes the folder derived/ and publishes the key
    /// file create-only by a hard link from a temporary name of its own;
    /// where one stands, and where another ask won that link, it unwraps
    /// that file, so every ask answers the same key. It opens no database,
    /// takes no lock, never replaces or removes a key file, and makes
    /// nothing before its wrap answers: without a capture store or its root
    /// it answers a [`DerivedKeyFault`] and leaves the data directory as it
    /// was.
    pub fn derived_store_key(&self) -> Result<StoreKey, DerivedKeyFault> {
        let roots_dir = self.roots();
        let store = KeyFile::at(&self.store_dir().join("store.key"))
            .and_then(|place| place.read(KeyLabel::STORE))
            .map_err(|e| match e {
                StoreError::Sealed | StoreError::Io(io::ErrorKind::NotFound) => {
                    DerivedKeyFault::NoStore
                }
                other => derived_fault(open_fault(&other)),
            })?;
        let custody = custody_for(&store.locator(&roots_dir)).map_err(|e| root_fault(&e))?;
        let roots = RootsHandle::open(&roots_dir).map_err(|e| root_fault(&e))?;
        let file = self.path().join("derived").join("master.key");
        if let Some(key) = standing_derived_key(&file, &store, custody.as_ref(), &roots)? {
            return Ok(key);
        }
        let key = StoreKey::random().map_err(|e| root_fault(&e))?;
        let (wrapped, _) = custody
            .wrap_store_key_in(&roots, &key, DERIVED_LABEL)
            .map_err(|e| root_fault(&e))?;
        let key_file = KeyFile::new(custody.as_ref(), DERIVED_LABEL, &wrapped);
        match publish_derived(self.path(), &file, &key_file) {
            Ok(()) => Ok(key),
            Err(StoreError::AlreadyExists) => {
                standing_derived_key(&file, &store, custody.as_ref(), &roots)?
                    .ok_or(DerivedKeyFault::Io)
            }
            Err(e) => Err(derived_fault(open_fault(&e))),
        }
    }

    /// The data directory itself: where its own name is a link, the
    /// directory that the link leads to, else the name as it is.
    pub fn resolved_path(&self) -> io::Result<std::path::PathBuf> {
        resolved_data_dir(self.path())
    }
}

fn standing_derived_key(
    file: &Path,
    store: &KeyFile,
    custody: &dyn KeyCustody,
    roots: &RootsHandle,
) -> Result<Option<StoreKey>, DerivedKeyFault> {
    let derived = match KeyFile::at(file).and_then(|place| place.read(DERIVED_LABEL)) {
        Ok(derived) => derived,
        Err(StoreError::Sealed | StoreError::Io(io::ErrorKind::NotFound)) => return Ok(None),
        Err(
            StoreError::KeyFileUnparseable
            | StoreError::KeyFileOfAnotherLabel
            | StoreError::NewerStore { .. }
            | StoreError::OlderStore { .. },
        ) => return Err(DerivedKeyFault::DerivedKeyFile),
        Err(e) => return Err(derived_fault(open_fault(&e))),
    };
    if derived.custody_kind != store.custody_kind || derived.root_id != store.root_id {
        return Err(DerivedKeyFault::AnotherRoot);
    }
    let wrapped = derived
        .wrapped_store_key()
        .map_err(|_| DerivedKeyFault::DerivedKeyFile)?;
    custody
        .unwrap_store_key_in(roots, &wrapped, DERIVED_LABEL)
        .map(|(key, _)| Some(key))
        .map_err(|e| root_fault(&e))
}

fn publish_derived(data_dir: &Path, file: &Path, key_file: &KeyFile) -> super::StoreResult<()> {
    let io_fault = |e: io::Error| StoreError::Io(e.kind());
    let dir = Dir::open(&resolved_data_dir(data_dir).map_err(io_fault)?).map_err(io_fault)?;
    dir.create_dir(OsStr::new("derived")).map_err(io_fault)?;
    dir.sync().map_err(io_fault)?;
    let mut tag = [0u8; 16];
    crate::custody::dev_fallback::fill_random(&mut tag)
        .map_err(|_| StoreError::Io(io::ErrorKind::Other))?;
    let tag: String = tag.iter().map(|byte| format!("{byte:02x}")).collect();
    KeyFile::at(file)?.publish_from(OsStr::new(&format!("master.key.{tag}.tmp")), key_file)
}

fn root_fault(e: &CustodyError) -> DerivedKeyFault {
    derived_fault(custody_fault(e))
}

fn derived_fault(fault: OpenFault) -> DerivedKeyFault {
    match fault {
        OpenFault::Locked | OpenFault::Io | OpenFault::Database => DerivedKeyFault::Io,
        OpenFault::NoSpace => DerivedKeyFault::NoSpace,
        OpenFault::Obstructed => DerivedKeyFault::Obstructed,
        OpenFault::CustodyLocked => DerivedKeyFault::CustodyLocked,
        OpenFault::RootDenied => DerivedKeyFault::RootDenied,
        OpenFault::IoNotRetried => DerivedKeyFault::IoNotRetried,
        OpenFault::RootMissing => DerivedKeyFault::RootMissing,
        OpenFault::RootCorrupt => DerivedKeyFault::RootCorrupt,
        OpenFault::WrapAuth => DerivedKeyFault::WrapAuth,
        OpenFault::KeyFile => DerivedKeyFault::StoreKeyFile,
        OpenFault::CustodyKind => DerivedKeyFault::CustodyKind,
        OpenFault::OlderStore => DerivedKeyFault::OlderStore,
        OpenFault::NewerStore => DerivedKeyFault::NewerStore,
    }
}

fn resolved_data_dir(data_dir: &Path) -> io::Result<std::path::PathBuf> {
    match std::fs::symlink_metadata(data_dir) {
        Ok(entry) if entry.file_type().is_symlink() => std::fs::canonicalize(data_dir),
        _ => Ok(data_dir.to_path_buf()),
    }
}

#[cfg(all(test, feature = "r2"))]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::key_id;
    use crate::daemon::port::tests::write_at;
    use crate::daemon::port::{
        ByteBound, CaptureStore, DataDirStore, MaintainStore, Ordinal, ScratchDir,
    };

    const SECOND: i64 = 1_000_000_000;

    /// Every entry under `dir`, in every folder under it, with its size.
    fn listing(dir: &Path) -> BTreeMap<PathBuf, u64> {
        let mut entries = BTreeMap::new();
        let mut folders = vec![dir.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                let meta = std::fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    folders.push(path.clone());
                }
                entries.insert(path, meta.len());
            }
        }
        entries
    }

    #[test]
    fn a_read_answers_the_first_live_segment_of_a_real_store() {
        let dir = tempfile::Builder::new()
            .prefix("adapter-read-")
            .tempdir()
            .unwrap();
        let made = ScratchDir::at(dir.path()).create().unwrap();
        let mut store = made.store;
        let noon = (20_000 * 86_400 + 43_200) * SECOND;
        let captured = [
            noon + 250,
            noon + 61 * SECOND + 500,
            noon + 122 * SECOND + 750,
        ];
        let batches: [&[u8]; 3] = [b"{\"n\":1}\n", b"{\"n\":2}\n{\"n\":22}\n", b"{\"n\":3}\n"];
        let first = write_at(&mut store, captured[0], batches[0]);
        let before_the_second = listing(dir.path());
        let second = write_at(&mut store, captured[1], batches[1]);
        let segments = dir.path().join("segments");
        let files_of_the_second: Vec<PathBuf> = listing(dir.path())
            .into_iter()
            .filter(|(path, _)| path.starts_with(&segments) && path.is_file())
            .filter(|(path, _)| !before_the_second.contains_key(path))
            .map(|(path, _)| path)
            .collect();
        assert_eq!(files_of_the_second.len(), 1);
        let third = write_at(&mut store, captured[2], batches[2]);
        let evicted = store.evict_to(ByteBound::raw(0), 1).unwrap();
        assert_eq!(evicted.shredded, [first.ordinal]);

        let id = key_id(dir.path()).unwrap();
        let expiry = (captured[1].div_euclid(SECOND) + 1_209_600) * SECOND;
        let unread = listing(dir.path());
        let read = store.read_segment(Ordinal::new(0)).unwrap();
        assert_eq!((read.store, read.highest), (id, Some(third.ordinal)));
        let live = read.segment.unwrap();
        assert_eq!(live.ordinal, second.ordinal);
        assert_eq!(live.lines.as_slice(), batches[1]);
        assert_eq!(live.expiry, expiry);
        let past = store.read_segment(Ordinal::new(u64::MAX)).unwrap();
        assert_eq!((past.store, past.highest), (id, Some(third.ordinal)));
        assert!(
            past.segment.is_none(),
            "a read above every segment read one"
        );
        assert_eq!(listing(dir.path()), unread, "a read made or changed a file");

        let counted = DataDirStore::damaged_reads();
        std::fs::remove_file(&files_of_the_second[0]).unwrap();
        let damaged = listing(dir.path());
        let read = store.read_segment(Ordinal::new(0)).unwrap();
        let passed = read.segment.unwrap();
        assert_eq!((passed.ordinal, passed.expiry), (second.ordinal, expiry));
        assert!(passed.lines.is_empty(), "a damaged segment answered a line");
        assert_eq!(DataDirStore::damaged_reads() - counted, 1);
        let next = Ordinal::new(second.ordinal.get() + 1);
        let live = store.read_segment(next).unwrap().segment.unwrap();
        assert_eq!(live.ordinal, third.ordinal);
        assert_eq!(live.lines.as_slice(), batches[2]);
        assert_eq!(DataDirStore::damaged_reads() - counted, 1);
        assert_eq!(
            listing(dir.path()),
            damaged,
            "a read made or changed a file"
        );

        drop(store);
        made.root.finish().unwrap();
        dir.close().unwrap();
    }
}
