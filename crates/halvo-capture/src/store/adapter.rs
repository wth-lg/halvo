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
use super::{Layout, OpenFor, RootReading, SegmentStore, StoreError, DEFAULT_RETENTION};
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
}

impl MaintainStore for Adapter {
    fn expire_due(&mut self, _now: SystemTime, _max_segments: usize) -> Result<Pass, MaintFault> {
        Ok(Pass::default())
    }

    fn evict_to(&mut self, _bound: ByteBound, _max_segments: usize) -> Result<Pass, MaintFault> {
        Ok(Pass::default())
    }

    fn usage(&mut self) -> Result<Usage, MaintFault> {
        self.store
            .size_bytes()
            .map(|disk_bytes| Usage {
                disk_bytes,
                logical_bytes: 0,
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
    let adapter = Adapter { store };
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

fn open_at(store_dir: &Path, roots: &Path, access: Access) -> Opened {
    let id = key_id(store_dir);
    match (
        EncryptedStore::open(store_dir, roots, DEFAULT_RETENTION, OpenFor::Write),
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
        tail_answer(EncryptedStore::delete_data_dir(
            self.path(),
            Layout::DataDirectory,
        ))
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
            store: Box::new(Adapter { store }),
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
