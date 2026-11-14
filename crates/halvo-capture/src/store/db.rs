//! SQLCipher metadata store and the [`EncryptedStore`].

use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use halvo_sqlcipher::rusqlite::config::DbConfig;
use halvo_sqlcipher::rusqlite::{self, Connection, ErrorCode, OpenFlags};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::custody::file_root::{self, RootsLeft};
use crate::custody::{
    custody_for, is_label, CustodyError, CustodyKind, CustodyLocator, KeyCustody, KeyLabel, RootId,
    StoreKey, WrappedKey,
};
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

use super::durable::{self, Dir, DurableFs, EntryKind, OsFs, Refused};
use super::lock::DirLock;
use super::segment::{self, SegmentHeader, SegmentKey};
use super::{DeleteReport, Met, SegmentStore, StoreError, StoreName, StoreResult};

const KEY_FILE_VERSION: u32 = 2;
const DATABASE_VERSION: i64 = 1;
const KEY_FILE: &str = "store.key";
const CREATE_PENDING: &str = "create.pending";
const CREATE_PENDING_TEMP: &str = "create.pending.tmp";
const DATABASE_FILE: &str = "store.db";
const DATABASE_LOG: &str = "store.db-wal";
const DATABASE_SIDE_FILES: [(&str, StoreName); 3] = [
    (DATABASE_LOG, StoreName::Log),
    ("store.db-shm", StoreName::SharedMemory),
    ("store.db-journal", StoreName::Journal),
];

#[cfg(test)]
pub(crate) const SEAM_BEFORE_THE_LOOK: &str = "a database open, before its look";
#[cfg(test)]
pub(crate) const SEAM_AFTER_THE_LAST_LOOK: &str = "a database open, after its last look";
#[cfg(test)]
pub(crate) const SEAM_BEFORE_THE_MODE_STEP: &str = "the mode step of store.db";
#[cfg(test)]
pub(crate) const SEAM_SIZE_AFTER_THE_LISTING: &str = "the size, after its first listing";
#[cfg(test)]
pub(crate) const SEAM_AFTER_THE_FIRST_READ: &str = "a database open, after its first read";
#[cfg(test)]
pub(crate) const SEAM_BEFORE_THE_DATABASE_STEP: &str =
    "a create, between its probe and its database step";
#[cfg(test)]
pub(crate) const SEAM_AFTER_THE_ROOT_STEP: &str = "a delete-all, after its root step";
#[cfg(test)]
pub(crate) const SEAM_BEFORE_THE_FIRST_READ: &str = "a database open, before its first read";
#[cfg(test)]
pub(crate) const SEAM_AFTER_THE_ROOT: &str = "a create, after its root";
#[cfg(test)]
pub(crate) const SEAM_AT_THE_LOG_CHECK: &str = "an open, at the first instant of the log's check";

fn obstructed(name: StoreName, met: Met) -> StoreError {
    StoreError::Obstructed { name, met }
}

#[derive(Deserialize)]
struct KeyFileVersion {
    version: u32,
}

#[derive(Deserialize)]
struct OlderKeyFile {
    root_id: String,
}

fn known_version(found: i64, supported: i64) -> StoreResult<()> {
    match found.cmp(&supported) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Greater => Err(StoreError::NewerStore { found, supported }),
        std::cmp::Ordering::Less => Err(StoreError::OlderStore { found, supported }),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct KeyFile {
    pub version: u32,
    pub custody_kind: CustodyKind,
    pub root_id: RootId,
    pub namespace: String,
    pub wrapped_key: String,
}

pub(crate) struct KeyFilePlace {
    dir: Dir,
    name: OsString,
    temp: OsString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyNames {
    Absent,
    Root(RootId),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Removal {
    pub(crate) gone: bool,
    pub(crate) not_followed: u64,
}

impl KeyFile {
    pub(crate) fn new(custody: &dyn KeyCustody, label: KeyLabel, wrapped: &WrappedKey) -> Self {
        KeyFile {
            version: KEY_FILE_VERSION,
            custody_kind: custody.kind(),
            root_id: custody.root_id(),
            namespace: label.as_str().to_string(),
            wrapped_key: BASE64.encode(&wrapped.0),
        }
    }

    pub(crate) fn at(path: &Path) -> StoreResult<KeyFilePlace> {
        let name = path
            .file_name()
            .ok_or(StoreError::Io(ErrorKind::InvalidInput))?;
        let parent = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let dir = Dir::open(parent).map_err(store_dir_error)?;
        Ok(KeyFilePlace::in_dir(dir, name))
    }

    #[cfg(test)]
    pub(crate) fn path(store_dir: &Path) -> PathBuf {
        store_dir.join(KEY_FILE)
    }

    #[cfg(test)]
    pub(crate) fn read(store_dir: &Path) -> StoreResult<KeyFile> {
        Self::at(&Self::path(store_dir))?.read(KeyLabel::STORE)
    }

    fn of_store(store_dir: &Dir) -> StoreResult<KeyFilePlace> {
        let dir = store_dir.try_clone().map_err(io_error)?;
        Ok(KeyFilePlace::in_dir(dir, OsStr::new(KEY_FILE)))
    }

    pub(crate) fn wrapped_store_key(&self) -> StoreResult<WrappedKey> {
        let wrapped = BASE64
            .decode(self.wrapped_key.as_bytes())
            .map_err(|_| StoreError::KeyFileUnparseable)?;
        match self.custody_kind.wrapped_store_key_len() {
            Some(len) if wrapped.len() != len => Err(StoreError::KeyFileUnparseable),
            _ => Ok(WrappedKey(wrapped)),
        }
    }

    pub(crate) fn locator(&self, roots: &Path) -> CustodyLocator {
        CustodyLocator {
            kind: self.custody_kind,
            root_id: self.root_id,
            roots: roots.to_path_buf(),
        }
    }
}

impl KeyFilePlace {
    fn in_dir(dir: Dir, name: &OsStr) -> Self {
        let mut temp = name.to_os_string();
        temp.push(".tmp");
        KeyFilePlace {
            dir,
            name: name.to_os_string(),
            temp,
        }
    }

    fn bytes(&self) -> StoreResult<Vec<u8>> {
        let mut bytes = Vec::new();
        let read = self
            .dir
            .open_file(&self.name)
            .and_then(|mut file| file.read_to_end(&mut bytes));
        match read {
            Ok(_) => Ok(bytes),
            Err(e) if e.kind() == ErrorKind::NotFound => Err(StoreError::Sealed),
            Err(e) => Err(key_file_error(e)),
        }
    }

    pub(crate) fn read(&self, label: KeyLabel) -> StoreResult<KeyFile> {
        let json = self.bytes()?;
        serde_json::from_slice::<KeyFileVersion>(&json)
            .map_err(|_| StoreError::KeyFileUnparseable)
            .and_then(|head| known_version(i64::from(head.version), i64::from(KEY_FILE_VERSION)))?;
        let key_file: KeyFile =
            serde_json::from_slice(&json).map_err(|_| StoreError::KeyFileUnparseable)?;
        if !is_label(key_file.namespace.as_bytes()) {
            return Err(StoreError::KeyFileUnparseable);
        }
        if key_file.namespace != label.as_str() {
            return Err(StoreError::KeyFileOfAnotherLabel);
        }
        Ok(key_file)
    }

    fn older_root(&self) -> Option<RootId> {
        let json = self.bytes().ok()?;
        let head: KeyFileVersion = serde_json::from_slice(&json).ok()?;
        if head.version != 1 {
            return None;
        }
        let older: OlderKeyFile = serde_json::from_slice(&json).ok()?;
        RootId::parse(&older.root_id)
    }

    pub(crate) fn named_root(&self) -> Option<RootId> {
        let json = self.bytes().ok()?;
        let named: OlderKeyFile = serde_json::from_slice(&json).ok()?;
        RootId::parse(&named.root_id)
    }

    fn names(&self) -> KeyNames {
        match self.dir.entry(&self.name) {
            Err(e) if e.kind() == ErrorKind::NotFound => KeyNames::Absent,
            _ => match self.named_root() {
                Some(root_id) => KeyNames::Root(root_id),
                None => KeyNames::Unknown,
            },
        }
    }

    fn present(&self) -> StoreResult<bool> {
        match self.dir.entry(&self.name) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_error(e)),
        }
    }

    pub(crate) fn publish(&self, key_file: &KeyFile) -> StoreResult<()> {
        let json = serde_json::to_vec_pretty(key_file)
            .map_err(|_| StoreError::Io(ErrorKind::InvalidData))?;
        zero_and_remove(&self.dir, &self.temp).map_err(io_error)?;
        let published = self.write_and_link(&json);
        let discarded = match published {
            Ok(()) => match self.dir.remove_file(&self.temp) {
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                other => other,
            },
            Err(_) => zero_and_remove(&self.dir, &self.temp),
        };
        published?;
        discarded.map_err(io_error)?;
        self.dir.sync().map_err(io_error)
    }

    fn write_and_link(&self, json: &[u8]) -> StoreResult<()> {
        let mut file = self.dir.create_new(&self.temp).map_err(io_error)?;
        file.write_all(json).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        failpoint::hit("key_file_before_publish")?;
        self.dir
            .link(&self.temp, &self.name)
            .map_err(|e| match e.kind() {
                ErrorKind::AlreadyExists => StoreError::AlreadyExists,
                _ => io_error(e),
            })
    }

    pub(crate) fn destroy(&self) -> Removal {
        let _ = zero_and_remove(&self.dir, &self.temp);
        if failpoint::hit("key_file_destroy").is_err() {
            return Removal {
                gone: false,
                not_followed: 0,
            };
        }
        let not_followed = match self.dir.entry(&self.name) {
            Ok(entry) if entry.kind != EntryKind::File => 1,
            _ => 0,
        };
        let _ = zero_and_remove(&self.dir, &self.name);
        let _ = self.dir.sync();
        let gone = matches!(self.dir.entry(&self.name), Err(e) if e.kind() == ErrorKind::NotFound);
        Removal { gone, not_followed }
    }
}

fn io_error(e: std::io::Error) -> StoreError {
    StoreError::Io(e.kind())
}

fn zero_and_remove(dir: &Dir, name: &OsStr) -> std::io::Result<()> {
    if let Ok(entry) = dir.entry(name) {
        if entry.kind == EntryKind::File {
            let _ = zero_fill(dir, name);
        }
    }
    match dir.remove_entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn zero_fill(dir: &Dir, name: &OsStr) -> std::io::Result<()> {
    let mut file = dir.open_file_for_write(name)?;
    let len = file.metadata()?.len() as usize;
    file.write_all(&vec![0u8; len])?;
    file.sync_all()
}

fn refused_at(name: StoreName, e: std::io::Error) -> StoreError {
    match durable::refusal(&e) {
        Some(Refused::Link) => obstructed(name, Met::Link),
        #[cfg(windows)]
        Some(Refused::Moved) => obstructed(StoreName::StoreDirectory, Met::DirectoryNotAtItsName),
        Some(_) => obstructed(name, Met::OtherKind),
        None if e.kind() == ErrorKind::IsADirectory => obstructed(name, Met::OtherKind),
        None => io_error(e),
    }
}

fn key_file_error(e: std::io::Error) -> StoreError {
    refused_at(StoreName::KeyFile, e)
}

fn store_dir_error(e: std::io::Error) -> StoreError {
    refused_at(StoreName::StoreDirectory, e)
}

const DATABASE_OPEN: OpenFlags = OpenFlags::SQLITE_OPEN_READ_WRITE
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX)
    .union(OpenFlags::SQLITE_OPEN_NOFOLLOW);

const PROBE_OPEN: OpenFlags = OpenFlags::SQLITE_OPEN_READ_ONLY
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX)
    .union(OpenFlags::SQLITE_OPEN_NOFOLLOW);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(target_os = "macos"),
    expect(dead_code, reason = "the volume form is compiled on macOS alone")
)]
pub(crate) enum NameForm {
    Volume,
    Kept,
}

type Named = (NameForm, VolumeIds);

#[cfg(unix)]
fn open_database(store_dir: &Dir, flags: OpenFlags) -> StoreResult<(Connection, Named)> {
    use std::os::unix::fs::PermissionsExt;
    #[cfg(test)]
    failpoint::seam(SEAM_BEFORE_THE_MODE_STEP);
    match store_dir.open_file(OsStr::new(DATABASE_FILE)) {
        Ok(file) => file
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(io_error)?,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(StoreError::DatabaseMissing),
        Err(e) => return Err(refused_at(StoreName::Database, e)),
    }
    open_connection(store_dir, flags)
}

fn refuse_a_moved_store_dir(store_dir: &Dir) -> StoreResult<()> {
    match store_dir.still_named() {
        Ok(true) => Ok(()),
        Ok(false) => Err(obstructed(
            StoreName::StoreDirectory,
            Met::DirectoryNotAtItsName,
        )),
        Err(e) => Err(io_error(e)),
    }
}

#[cfg(windows)]
fn open_database(store_dir: &Dir, flags: OpenFlags) -> StoreResult<(Connection, Named)> {
    #[cfg(test)]
    failpoint::seam(SEAM_BEFORE_THE_MODE_STEP);
    open_connection(store_dir, flags)
}

fn open_connection(store_dir: &Dir, flags: OpenFlags) -> StoreResult<(Connection, Named)> {
    #[cfg(test)]
    failpoint::seam(SEAM_BEFORE_THE_LOOK);
    look_at_the_database_files(store_dir)?;
    #[cfg(windows)]
    let held = hold_the_database_file(store_dir)?;
    refuse_a_moved_store_dir(store_dir)?;
    #[cfg(test)]
    failpoint::seam(SEAM_AFTER_THE_LAST_LOOK);
    let (name, form, volume) = database_name(store_dir)?;
    let conn = Connection::open_with_flags(name, flags)
        .map(|conn| (conn, (form, volume)))
        .map_err(|e| after_a_code(store_dir, &e));
    #[cfg(test)]
    let conn = match (conn, code_seams::forced()) {
        (Ok(_), Some(code)) => Err(after_a_code_of(store_dir, Some(code))),
        (conn, _) => conn,
    };
    #[cfg(windows)]
    drop(held);
    conn
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(target_os = "macos"),
    expect(dead_code, reason = "the volume form is compiled on macOS alone")
)]
pub(crate) enum VolumeIds {
    Yes,
    No,
    Unknown,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VolumeReadings {
    pub(crate) flag: Option<bool>,
    pub(crate) capability: Option<(bool, bool)>,
    pub(crate) root: Option<RootAnswer>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootAnswer {
    TheRoot,
    NoSuchName,
    Other,
}

#[cfg(target_os = "macos")]
impl VolumeReadings {
    pub(crate) fn ids(self) -> VolumeIds {
        let yes = self.capability == Some((true, true))
            || self.flag == Some(true)
            || self.root == Some(RootAnswer::TheRoot);
        let no = self.capability == Some((true, false))
            && self.flag == Some(false)
            && self.root == Some(RootAnswer::NoSuchName);
        match (yes, no) {
            (true, _) => VolumeIds::Yes,
            (false, true) => VolumeIds::No,
            (false, false) => VolumeIds::Unknown,
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectoryStat {
    TheHandle,
    AnotherIdentity,
    NoSuchName,
    Failed,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameChoice {
    Volume,
    Kept,
    StatAgain,
    Refused,
    Failed,
}

#[cfg(target_os = "macos")]
pub(crate) fn choose_the_name(
    stat: DirectoryStat,
    before: VolumeIds,
    after: VolumeIds,
    handle_away: bool,
    stats_left: u32,
) -> NameChoice {
    match stat {
        DirectoryStat::TheHandle => NameChoice::Volume,
        DirectoryStat::AnotherIdentity => NameChoice::Refused,
        DirectoryStat::Failed => NameChoice::Failed,
        DirectoryStat::NoSuchName if before != after => NameChoice::Refused,
        DirectoryStat::NoSuchName => match before {
            VolumeIds::No => NameChoice::Kept,
            VolumeIds::Unknown => NameChoice::Refused,
            VolumeIds::Yes if handle_away => NameChoice::Refused,
            VolumeIds::Yes if stats_left > 0 => NameChoice::StatAgain,
            VolumeIds::Yes => NameChoice::Refused,
        },
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn volume_readings(store_dir: &Dir) -> VolumeReadings {
    #[cfg(test)]
    if let Some(readings) = name_seams::volume_answer() {
        return readings;
    }
    let Ok((flags, mount, fsid)) = store_dir.volume() else {
        return VolumeReadings {
            flag: None,
            capability: None,
            root: None,
        };
    };
    let flag = Some(flags & libc::MNT_DOVOLFS as u32 != 0);
    let root_dir = Dir::open(&mount).ok();
    let capability = root_dir.as_ref().and_then(|root| {
        let fd = root.raw_descriptor();
        match durable::volume_of(fd) {
            Ok((_, _, id)) if id == fsid => path_from_id(fd),
            _ => None,
        }
    });
    let root = root_dir.and_then(|root| {
        let (device, file_number) = root.identity().ok()?;
        let name = PathBuf::from(format!("/.vol/{device}/{file_number}"));
        Some(match volume_stat_of(&name) {
            Ok((EntryKind::Dir, d, n)) if d == device && n == file_number => RootAnswer::TheRoot,
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(libc::ENOENT | libc::ENOTSUP | libc::EOPNOTSUPP)
                ) =>
            {
                RootAnswer::NoSuchName
            }
            _ => RootAnswer::Other,
        })
    });
    VolumeReadings {
        flag,
        capability,
        root,
    }
}

#[cfg(target_os = "macos")]
fn path_from_id(fd: std::os::fd::RawFd) -> Option<(bool, bool)> {
    #[repr(C)]
    struct Answer {
        length: u32,
        capabilities: libc::vol_capabilities_attr_t,
    }
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: 0,
        volattr: libc::ATTR_VOL_INFO | libc::ATTR_VOL_CAPABILITIES,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    // SAFETY: Answer is plain data, for which all-zero bytes are a valid value.
    let mut answer: Answer = unsafe { std::mem::zeroed() };
    // SAFETY: list and answer outlive the call, the size is answer's own, and
    // fd is a descriptor that the caller keeps open across the call.
    let done = unsafe {
        libc::fgetattrlist(
            fd,
            (&mut list as *mut libc::attrlist).cast(),
            (&mut answer as *mut Answer).cast(),
            size_of::<Answer>(),
            0,
        )
    };
    if done != 0 {
        return None;
    }
    let format = libc::VOL_CAPABILITIES_FORMAT;
    let bit = libc::VOL_CAP_FMT_PATH_FROM_ID;
    Some((
        answer.capabilities.valid[format] & bit != 0,
        answer.capabilities.capabilities[format] & bit != 0,
    ))
}

#[cfg(target_os = "macos")]
fn database_name(store_dir: &Dir) -> StoreResult<(PathBuf, NameForm, VolumeIds)> {
    let (device, file_number) = store_dir.identity().map_err(io_error)?;
    let volume = PathBuf::from(format!("/.vol/{device}/{file_number}"));
    let before = volume_readings(store_dir).ids();
    let mut stats_left = 2;
    loop {
        let (stat, error) = match volume_stat(&volume) {
            Ok((EntryKind::Dir, d, n)) if d == device && n == file_number => {
                (DirectoryStat::TheHandle, None)
            }
            Ok(_) => (DirectoryStat::AnotherIdentity, None),
            Err(e)
                if matches!(
                    e.raw_os_error(),
                    Some(libc::ENOENT | libc::ENOTSUP | libc::EOPNOTSUPP)
                ) =>
            {
                (DirectoryStat::NoSuchName, None)
            }
            Err(e) => (DirectoryStat::Failed, Some(e)),
        };
        let after = match stat {
            DirectoryStat::NoSuchName => volume_readings(store_dir).ids(),
            _ => before,
        };
        let away = stat == DirectoryStat::NoSuchName
            && (matches!(store_dir.link_count(), Ok(0))
                || !matches!(store_dir.still_named(), Ok(true)));
        match choose_the_name(stat, before, after, away, stats_left) {
            NameChoice::Volume => {
                #[cfg(test)]
                if let Some(given) = name_seams::given_name() {
                    return Ok((given, NameForm::Volume, before));
                }
                return Ok((volume.join(DATABASE_FILE), NameForm::Volume, before));
            }
            NameChoice::Kept => {
                return Ok((store_dir.path().join(DATABASE_FILE), NameForm::Kept, before))
            }
            NameChoice::StatAgain => stats_left -= 1,
            NameChoice::Refused => {
                return Err(obstructed(
                    StoreName::StoreDirectory,
                    Met::DirectoryNotAtItsName,
                ))
            }
            NameChoice::Failed => {
                return Err(error.map_or(StoreError::Io(ErrorKind::Other), io_error))
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn volume_stat(volume: &Path) -> std::io::Result<(EntryKind, u64, u64)> {
    #[cfg(test)]
    if let Some(answer) = name_seams::stat_answer() {
        return answer;
    }
    let meta = std::fs::symlink_metadata(volume)?;
    use std::os::unix::fs::MetadataExt;
    let kind = if meta.file_type().is_symlink() {
        EntryKind::Link
    } else if meta.is_dir() {
        EntryKind::Dir
    } else if meta.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    Ok((kind, meta.dev() as u64, meta.ino()))
}

#[cfg(windows)]
fn database_name(store_dir: &Dir) -> StoreResult<(PathBuf, NameForm, VolumeIds)> {
    Ok((
        store_dir.path().join(DATABASE_FILE),
        NameForm::Kept,
        VolumeIds::No,
    ))
}

#[cfg(all(test, target_os = "macos"))]
pub(crate) mod name_seams {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use crate::store::durable::EntryKind;

    type Answer = std::io::Result<(EntryKind, u64, u64)>;

    thread_local! {
        static VOLUME: RefCell<Option<super::VolumeReadings>> = const { RefCell::new(None) };
        static GIVEN: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
        static STAT: RefCell<Option<Box<dyn Fn() -> Answer>>> = const { RefCell::new(None) };
    }

    pub(crate) fn answer_volume(readings: Option<super::VolumeReadings>) {
        VOLUME.with(|volume| *volume.borrow_mut() = readings);
    }

    pub(crate) fn volume_answer() -> Option<super::VolumeReadings> {
        VOLUME.with(|volume| *volume.borrow())
    }

    pub(crate) fn give_name(name: Option<PathBuf>) {
        GIVEN.with(|given| *given.borrow_mut() = name);
    }

    pub(crate) fn given_name() -> Option<PathBuf> {
        GIVEN.with(|given| given.borrow().clone())
    }

    pub(crate) fn answer_stat(answer: Option<Box<dyn Fn() -> Answer>>) {
        STAT.with(|stat| *stat.borrow_mut() = answer);
    }

    pub(crate) fn stat_answer() -> Option<Answer> {
        STAT.with(|stat| stat.borrow().as_ref().map(|answer| answer()))
    }
}

#[cfg(test)]
pub(crate) mod code_seams {
    use std::cell::Cell;

    use halvo_sqlcipher::rusqlite;

    thread_local! {
        static FORCED: Cell<Option<(usize, i32)>> = const { Cell::new(None) };
    }

    pub(crate) fn force(nth: usize, code: Option<i32>) {
        FORCED.with(|forced| forced.set(code.map(|code| (nth, code))));
    }

    pub(crate) const PROBE_TIMEOUT: &str = "the live-holder probe's busy timeout";
    pub(crate) const PROBE_LOCKING_MODE: &str = "the live-holder probe's locking mode";
    pub(crate) const HELD_LOOK_HAS_MOVED: &str = "the held look's question whether the file moved";

    thread_local! {
        static FORCED_AT: Cell<Option<(&'static str, i32)>> = const { Cell::new(None) };
    }

    pub(crate) fn force_at(place: &'static str, code: Option<i32>) {
        FORCED_AT.with(|forced| forced.set(code.map(|code| (place, code))));
    }

    pub(crate) fn or_forced_at<T>(
        place: &'static str,
        result: rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        FORCED_AT.with(|forced| match forced.get() {
            Some((at, code)) if at == place => {
                forced.set(None);
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(code),
                    None,
                ))
            }
            _ => result,
        })
    }

    pub(crate) fn forced() -> Option<i32> {
        FORCED.with(|forced| match forced.get() {
            Some((1, code)) => {
                forced.set(None);
                Some(code)
            }
            Some((nth, code)) => {
                forced.set(Some((nth - 1, code)));
                None
            }
            None => None,
        })
    }
}

fn after_a_code(store_dir: &Dir, e: &rusqlite::Error) -> StoreError {
    let code = match e {
        rusqlite::Error::SqliteFailure(failure, _) => Some(failure.extended_code),
        _ => None,
    };
    after_a_code_of(store_dir, code)
}

fn after_a_code_of(store_dir: &Dir, code: Option<i32>) -> StoreError {
    match look_again(store_dir) {
        Err(found) => found,
        Ok(()) => code_answer(code),
    }
}

fn look_again(store_dir: &Dir) -> StoreResult<()> {
    refuse_a_moved_store_dir(store_dir)?;
    look_at_the_database_files(store_dir)
}

pub(crate) fn code_answer(code: Option<i32>) -> StoreError {
    match code.map(|code| code & 0xff) {
        Some(rusqlite::ffi::SQLITE_CORRUPT | rusqlite::ffi::SQLITE_NOTADB) => {
            StoreError::DatabaseUnreadable
        }
        Some(rusqlite::ffi::SQLITE_FULL) => StoreError::Io(ErrorKind::StorageFull),
        Some(rusqlite::ffi::SQLITE_BUSY | rusqlite::ffi::SQLITE_LOCKED) => StoreError::Locked,
        _ => StoreError::Io(ErrorKind::Other),
    }
}

#[cfg(windows)]
fn hold_the_database_file(store_dir: &Dir) -> StoreResult<Option<std::fs::File>> {
    match store_dir.hold_file(OsStr::new(DATABASE_FILE)) {
        Ok(held) => Ok(Some(held)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(refused_at(StoreName::Database, e)),
    }
}

fn look_at_the_database_files(store_dir: &Dir) -> StoreResult<()> {
    let names = [(DATABASE_FILE, StoreName::Database)]
        .into_iter()
        .chain(DATABASE_SIDE_FILES);
    for (file, name) in names {
        match store_dir.entry(OsStr::new(file)) {
            Ok(entry) if entry.kind == EntryKind::File => {}
            Ok(entry) if entry.kind == EntryKind::Link => return Err(obstructed(name, Met::Link)),
            Ok(_) => return Err(obstructed(name, Met::OtherKind)),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(e)),
        }
    }
    Ok(())
}

pub(crate) use super::failpoint;

pub(crate) fn db_error(e: rusqlite::Error) -> StoreError {
    match e {
        rusqlite::Error::SqliteFailure(failure, _) => StoreError::Db(failure.extended_code),
        _ => StoreError::Corrupt("the database returned an unexpected value".to_string()),
    }
}

pub(crate) fn unix_secs_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn unix_nanos(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

fn remove_in(store_dir: &Dir, name: &str) -> StoreResult<()> {
    match store_dir.remove_file(OsStr::new(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    custody_kind: CustodyKind,
    root_id: RootId,
}

fn write_pending(store_dir: &Dir, custody: &dyn KeyCustody) -> StoreResult<()> {
    let json = serde_json::to_vec(&Pending {
        custody_kind: custody.kind(),
        root_id: custody.root_id(),
    })
    .map_err(|_| StoreError::Io(ErrorKind::InvalidData))?;
    durable::write_durably(
        &OsFs,
        store_dir,
        OsStr::new(CREATE_PENDING_TEMP),
        OsStr::new(CREATE_PENDING),
        io_error,
        |_| Ok(()),
        |dir, temp| match dir.remove_file(temp) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(io_error(e)),
            _ => Ok(()),
        },
        |out| out.write_all(&json).map_err(io_error),
    )
}

fn read_pending(store_dir: &Dir) -> Option<Pending> {
    let mut json = Vec::new();
    store_dir
        .open_file(OsStr::new(CREATE_PENDING))
        .and_then(|mut file| file.read_to_end(&mut json))
        .ok()?;
    serde_json::from_slice(&json).ok()
}

fn remove_pending(store_dir: &Dir) -> StoreResult<()> {
    remove_in(store_dir, CREATE_PENDING_TEMP)?;
    remove_in(store_dir, CREATE_PENDING)?;
    store_dir.sync().map_err(io_error)
}

fn clear_pending(store_dir: &Dir, roots: &Path, keyed: KeyNames) -> StoreResult<()> {
    let present = store_dir.entry(OsStr::new(CREATE_PENDING)).is_ok()
        || store_dir.entry(OsStr::new(CREATE_PENDING_TEMP)).is_ok();
    if !present || keyed == KeyNames::Unknown {
        return Ok(());
    }
    if let Some(pending) = read_pending(store_dir) {
        if keyed != KeyNames::Root(pending.root_id) {
            file_root::destroy_root_at(roots, pending.root_id)?;
        }
    }
    remove_pending(store_dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    pub busy: i64,
    pub wal_bytes: u64,
}

pub(crate) struct Db {
    conn: Connection,
    store_dir: Dir,
    log_name: std::ffi::OsString,
    named: Named,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Database {
    New,
    Existing,
}

const SCHEMA_TABLES: [&str; 3] = ["segment_keys", "events", "events_fts"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DatabaseRemoval {
    pub(crate) left: bool,
    pub(crate) not_followed: u64,
}

impl Db {
    pub(crate) fn destroy_through(store_dir: &Dir) -> DatabaseRemoval {
        if failpoint::hit("database_destroy").is_err() {
            return DatabaseRemoval {
                left: true,
                not_followed: 0,
            };
        }
        let mut removal = DatabaseRemoval {
            left: false,
            not_followed: 0,
        };
        let names = [DATABASE_FILE]
            .into_iter()
            .chain(DATABASE_SIDE_FILES.map(|(file, _)| file));
        for name in names {
            let name = OsStr::new(name);
            match store_dir.entry(name) {
                Ok(entry) if entry.kind != EntryKind::File => removal.not_followed += 1,
                _ => {}
            }
            let _ = store_dir.remove_entry(name);
            if !matches!(store_dir.entry(name), Err(e) if e.kind() == ErrorKind::NotFound) {
                removal.left = true;
            }
        }
        let _ = store_dir.sync();
        removal
    }

    fn destroy_in(store_dir: &Dir) -> StoreResult<()> {
        let removal = Self::destroy_through(store_dir);
        if removal.left {
            return Err(StoreError::Io(ErrorKind::Other));
        }
        Ok(())
    }

    pub(crate) fn open(store_dir: &Dir, key: &[u8; 32]) -> StoreResult<Self> {
        Self::open_file(store_dir, key, Database::New)
    }

    pub(crate) fn open_existing(store_dir: &Dir, key: &[u8; 32]) -> StoreResult<Self> {
        Self::open_file(store_dir, key, Database::Existing)
    }

    fn refuse_a_live_holder(store_dir: &Dir) -> StoreResult<()> {
        halvo_sqlcipher::initialize().map_err(|code| code_answer(Some(code)))?;
        match store_dir.entry(OsStr::new(DATABASE_FILE)) {
            Ok(entry) if entry.kind == EntryKind::File && entry.len > 0 => {}
            _ => return Ok(()),
        }
        let probe = match open_connection(store_dir, PROBE_OPEN) {
            Ok((probe, _)) => probe,
            Err(StoreError::Locked) => return Err(StoreError::Locked),
            Err(e @ StoreError::Obstructed { .. }) => return Err(e),
            Err(_) => return Ok(()),
        };
        let timeout = probe.busy_timeout(Duration::ZERO);
        #[cfg(test)]
        let timeout = code_seams::or_forced_at(code_seams::PROBE_TIMEOUT, timeout);
        timeout.map_err(|e| after_a_code(store_dir, &e))?;
        let locking = probe.execute_batch("PRAGMA locking_mode = EXCLUSIVE;");
        #[cfg(test)]
        let locking = code_seams::or_forced_at(code_seams::PROBE_LOCKING_MODE, locking);
        locking.map_err(|e| after_a_code(store_dir, &e))?;
        let read = probe.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        });
        match read.map_err(|e| e.sqlite_error_code()) {
            Err(Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)) => {
                Err(StoreError::Locked)
            }
            _ => Ok(()),
        }
    }

    fn require_file(store_dir: &Dir) -> StoreResult<()> {
        match store_dir.entry(OsStr::new(DATABASE_FILE)) {
            Ok(entry) if entry.kind == EntryKind::Link => {
                Err(obstructed(StoreName::Database, Met::Link))
            }
            Ok(entry) if entry.kind == EntryKind::File && entry.len == 0 => {
                Err(StoreError::DatabaseMissing)
            }
            Ok(entry) if entry.kind == EntryKind::File => Ok(()),
            Ok(_) => Err(obstructed(StoreName::Database, Met::OtherKind)),
            Err(e) if e.kind() == ErrorKind::NotFound => Err(StoreError::DatabaseMissing),
            Err(e) => Err(io_error(e)),
        }
    }

    fn open_file(store_dir: &Dir, key: &[u8; 32], database: Database) -> StoreResult<Self> {
        if database == Database::Existing {
            Self::require_file(store_dir)?;
        }
        halvo_sqlcipher::initialize().map_err(|code| code_answer(Some(code)))?;
        let log_was_there = match store_dir.entry(OsStr::new(DATABASE_LOG)) {
            Ok(_) => true,
            Err(e) if e.kind() == ErrorKind::NotFound => false,
            Err(e) => return Err(io_error(e)),
        };
        if database == Database::New {
            store_dir
                .create_new(OsStr::new(DATABASE_FILE))
                .map_err(io_error)?;
        }
        let (conn, (name_form, volume_ids)) = open_database(store_dir, DATABASE_OPEN)?;
        let set_up = Self::set_up(&conn, key, database, log_was_there);
        #[cfg(test)]
        failpoint::seam(SEAM_AFTER_THE_FIRST_READ);
        held_look(&conn, store_dir, name_form)?;
        set_up.map_err(|e| Self::answered(store_dir, e))?;
        let db = Self {
            conn,
            store_dir: store_dir.try_clone().map_err(io_error)?,
            log_name: DATABASE_LOG.into(),
            named: (name_form, volume_ids),
        };
        db.finish(database)
            .map_err(|e| Self::answered(store_dir, e))?;
        Ok(db)
    }

    fn answered(store_dir: &Dir, e: StoreError) -> StoreError {
        match e {
            StoreError::Db(code) => after_a_code_of(store_dir, Some(code)),
            other => other,
        }
    }

    fn set_up(
        conn: &Connection,
        key: &[u8; 32],
        database: Database,
        log_was_there: bool,
    ) -> StoreResult<()> {
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, log_was_there)
            .map_err(db_error)?;
        conn.busy_timeout(Duration::ZERO).map_err(db_error)?;
        conn.execute_batch(&key_pragma(key)).map_err(db_error)?;
        conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE;")
            .map_err(db_error)?;
        #[cfg(test)]
        failpoint::seam(SEAM_BEFORE_THE_FIRST_READ);
        verify_key(conn)?;
        if database == Database::Existing {
            known_version(pragma_int(conn, "user_version")?, DATABASE_VERSION)?;
        }
        conn.execute_batch(
            "PRAGMA secure_delete = ON;\n\
             PRAGMA temp_store = MEMORY;\n\
             PRAGMA foreign_keys = ON;\n\
             PRAGMA checkpoint_fullfsync = ON;\n\
             PRAGMA cipher_log_level = NONE;",
        )
        .map_err(db_error)?;
        require_write_ahead_log(conn)?;
        require_secure_delete(conn)?;
        require_exclusive_locking(conn)?;
        require_checkpoint_fullfsync(conn)?;
        require_cipher_log_level_none(conn)?;
        require_pinned_cipher(conn)
    }

    fn finish(&self, database: Database) -> StoreResult<()> {
        self.log_drained()?;
        match database {
            Database::New => {
                self.init_schema()?;
                self.conn
                    .execute_batch(&format!("PRAGMA user_version = {DATABASE_VERSION};"))
                    .map_err(db_error)?;
            }
            Database::Existing => self.require_schema()?,
        }
        self.enable_fts_secure_delete()?;
        self.log_look()?;
        if database == Database::New {
            self.checkpoint_truncate()?;
            self.page_look()?;
        }
        held_look(&self.conn, &self.store_dir, self.named.0)?;
        failpoint::hit("db_open")?;
        self.conn
            .set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)
            .map_err(db_error)?;
        Ok(())
    }

    fn log_drained(&self) -> StoreResult<()> {
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .map_err(db_error)?;
        if busy != 0 {
            return Err(StoreError::Locked);
        }
        #[cfg(test)]
        failpoint::seam(SEAM_AT_THE_LOG_CHECK);
        match self.store_dir.entry(&self.log_name) {
            Ok(entry) if entry.kind == EntryKind::File && entry.len == 0 => Ok(()),
            Ok(entry) if entry.kind == EntryKind::Link => {
                Err(obstructed(StoreName::Log, Met::Link))
            }
            Ok(entry) if entry.kind != EntryKind::File => {
                Err(obstructed(StoreName::Log, Met::OtherKind))
            }
            Ok(_) => Err(obstructed(StoreName::Log, Met::DatabaseOutsideTheDirectory)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error(e)),
        }
    }

    fn log_look(&self) -> StoreResult<()> {
        match self.store_dir.entry(&self.log_name) {
            Ok(entry) if entry.kind == EntryKind::File && entry.len > 0 => Ok(()),
            Ok(entry) if entry.kind == EntryKind::Link => {
                Err(obstructed(StoreName::Log, Met::Link))
            }
            Ok(entry) if entry.kind != EntryKind::File => {
                Err(obstructed(StoreName::Log, Met::OtherKind))
            }
            Ok(_) => Err(obstructed(StoreName::Log, Met::DatabaseOutsideTheDirectory)),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                Err(obstructed(StoreName::Log, Met::DatabaseOutsideTheDirectory))
            }
            Err(e) => Err(io_error(e)),
        }
    }

    fn page_look(&self) -> StoreResult<()> {
        let page: u64 = self
            .conn
            .query_row("PRAGMA page_size", [], |r| r.get::<_, String>(0))
            .map_err(db_error)?
            .parse()
            .map_err(|_| StoreError::SettingNotInEffect("page_size"))?;
        match self.store_dir.entry(OsStr::new(DATABASE_FILE)) {
            Ok(entry) if entry.kind == EntryKind::File && entry.len >= page => Ok(()),
            Ok(_) => Err(obstructed(
                StoreName::Database,
                Met::DatabaseOutsideTheDirectory,
            )),
            Err(e) if e.kind() == ErrorKind::NotFound => Err(obstructed(
                StoreName::Database,
                Met::DatabaseOutsideTheDirectory,
            )),
            Err(e) => Err(io_error(e)),
        }
    }

    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn name_form(&self) -> NameForm {
        self.named.0
    }

    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn volume_ids(&self) -> VolumeIds {
        self.named.1
    }

    fn init_schema(&self) -> StoreResult<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS segment_keys (\n\
                   id INTEGER PRIMARY KEY,\n\
                   segment_id TEXT NOT NULL UNIQUE,\n\
                   wrapped_key BLOB,\n\
                   created_at INTEGER NOT NULL,\n\
                   expires_at INTEGER NOT NULL\n\
                 );\n\
                 CREATE TABLE IF NOT EXISTS events (\n\
                   event_id TEXT PRIMARY KEY,\n\
                   wall_clock_nanos INTEGER NOT NULL,\n\
                   sequence INTEGER NOT NULL,\n\
                   modality TEXT NOT NULL,\n\
                   foreground_app TEXT NOT NULL,\n\
                   segment_id TEXT NOT NULL,\n\
                   byte_offset INTEGER NOT NULL,\n\
                   byte_len INTEGER NOT NULL,\n\
                   segment_key_id INTEGER NOT NULL,\n\
                   is_derived INTEGER NOT NULL,\n\
                   retention_class TEXT NOT NULL\n\
                 );\n\
                 CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(foreground_app);",
            )
            .map_err(db_error)
    }

    fn require_schema(&self) -> StoreResult<()> {
        for table in SCHEMA_TABLES {
            let defined: i64 = self
                .conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .map_err(unreadable_or_db_error)?;
            if defined != 1 {
                return Err(StoreError::DatabaseUnreadable);
            }
            self.conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(unreadable_or_db_error)?;
        }
        Ok(())
    }

    fn enable_fts_secure_delete(&self) -> StoreResult<()> {
        self.conn
            .execute_batch("INSERT INTO events_fts(events_fts, rank) VALUES('secure-delete', 1);")
            .map_err(db_error)
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    fn reserve_segment_key(
        &self,
        segment_id: uuid::Uuid,
        created_at: u64,
        expires_at: u64,
    ) -> StoreResult<SegmentKeyId> {
        self.conn
            .execute(
                "INSERT INTO segment_keys (segment_id, wrapped_key, created_at, expires_at) \
                 VALUES (?1, NULL, ?2, ?3)",
                rusqlite::params![
                    segment_id.simple().to_string(),
                    created_at as i64,
                    expires_at as i64
                ],
            )
            .map_err(db_error)?;
        Ok(SegmentKeyId(self.conn.last_insert_rowid()))
    }

    fn commit_segment_key(&self, id: SegmentKeyId, wrapped: &[u8]) -> StoreResult<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE segment_keys SET wrapped_key = ?1 WHERE id = ?2 AND wrapped_key IS NULL",
                rusqlite::params![wrapped, id.0],
            )
            .map_err(db_error)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StoreError::Corrupt(
                "a segment key row was not reserved for this write".to_string(),
            ))
        }
    }

    pub(crate) fn insert_event(&self, event: &CaptureStreamEvent) -> StoreResult<()> {
        self.conn
            .execute_batch("SAVEPOINT insert_event;")
            .map_err(db_error)?;
        match self.insert_event_rows(event) {
            Ok(()) => self
                .conn
                .execute_batch("RELEASE insert_event;")
                .map_err(db_error),
            Err(cause) => match self
                .conn
                .execute_batch("ROLLBACK TO insert_event; RELEASE insert_event;")
            {
                Ok(()) => Err(cause),
                Err(e) => Err(db_error(e)),
            },
        }
    }

    fn insert_event_rows(&self, event: &CaptureStreamEvent) -> StoreResult<()> {
        self.conn
            .execute(
                "INSERT INTO events (event_id, wall_clock_nanos, sequence, modality, \
                   foreground_app, segment_id, byte_offset, byte_len, segment_key_id, \
                   is_derived, retention_class) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![
                    event.event_id.simple().to_string(),
                    unix_nanos(event.captured_at.wall_clock),
                    event.captured_at.sequence.get() as i64,
                    format!("{:?}", event.modality),
                    event.foreground_app,
                    event.segment_ref.segment_id.simple().to_string(),
                    event.segment_ref.byte_offset as i64,
                    event.segment_ref.byte_len as i64,
                    event.segment_key_id.0,
                    event.is_derived as i64,
                    format!("{:?}", event.retention_class),
                ],
            )
            .map_err(db_error)?;
        let rowid = self.conn.last_insert_rowid();
        self.conn
            .execute(
                "INSERT INTO events_fts (rowid, foreground_app) VALUES (?1, ?2)",
                rusqlite::params![rowid, event.foreground_app],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn pragma_int(&self, name: &str) -> StoreResult<i64> {
        pragma_int(&self.conn, name)
    }

    pub(crate) fn cipher_provider(&self) -> StoreResult<String> {
        self.conn
            .query_row("PRAGMA cipher_provider", [], |r| r.get::<_, String>(0))
            .map_err(db_error)
    }

    pub(crate) fn fts_secure_delete(&self) -> StoreResult<i64> {
        self.conn
            .query_row(
                "SELECT CAST(v AS INTEGER) FROM events_fts_config WHERE k='secure-delete'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map_err(db_error)
    }

    pub(crate) fn keyed_segments(&self) -> Option<u64> {
        self.conn
            .query_row(
                "SELECT count(*) FROM segment_keys WHERE wrapped_key IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0),
            )
            .ok()
            .map(|count| count as u64)
    }

    pub(crate) fn wal_bytes(&self) -> StoreResult<u64> {
        match self.store_dir.entry(&self.log_name) {
            Ok(entry) if entry.kind == EntryKind::File => Ok(entry.len),
            Ok(entry) if entry.kind == EntryKind::Link => {
                Err(obstructed(StoreName::Log, Met::Link))
            }
            Ok(_) => Err(obstructed(StoreName::Log, Met::OtherKind)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(0),
            Err(e) => Err(io_error(e)),
        }
    }

    pub(crate) fn checkpoint_truncate(&self) -> StoreResult<Checkpoint> {
        let busy: i64 = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .map_err(db_error)?;
        let checkpoint = Checkpoint {
            busy,
            wal_bytes: self.wal_bytes()?,
        };
        if checkpoint.busy != 0 || checkpoint.wal_bytes != 0 {
            return Err(StoreError::CheckpointIncomplete {
                busy: checkpoint.busy,
                wal_bytes: checkpoint.wal_bytes,
            });
        }
        Ok(checkpoint)
    }
}

fn verify_key(conn: &Connection) -> StoreResult<()> {
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|_| ())
    .map_err(|e| match e.sqlite_error_code() {
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => {
            StoreError::DatabaseUnreadable
        }
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => StoreError::Locked,
        _ => db_error(e),
    })
}

fn unreadable_or_db_error(e: rusqlite::Error) -> StoreError {
    match e.sqlite_error_code() {
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => {
            StoreError::DatabaseUnreadable
        }
        _ => db_error(e),
    }
}

fn require_secure_delete(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "secure_delete")? {
        1 => Ok(()),
        _ => Err(StoreError::SettingNotInEffect("secure_delete")),
    }
}

fn require_write_ahead_log(conn: &Connection) -> StoreResult<()> {
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL;", [], |r| r.get(0))
        .map_err(db_error)?;
    if mode == "wal" {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect("journal_mode"))
    }
}

const CIPHER_PROVIDER: &str = "libtomcrypt";
const CIPHER_VERSION: &str = "4.19.0";

fn require_pinned_cipher(conn: &Connection) -> StoreResult<()> {
    let text = |pragma: &str| -> StoreResult<String> {
        conn.query_row(pragma, [], |r| r.get(0)).map_err(db_error)
    };
    cipher_in_effect(
        &text("PRAGMA cipher_provider")?,
        &text("PRAGMA cipher_version")?,
        halvo_sqlcipher::os_entropy_hook_installed(),
    )
}

fn cipher_in_effect(provider: &str, version: &str, os_entropy_hook: bool) -> StoreResult<()> {
    if provider != CIPHER_PROVIDER {
        return Err(StoreError::SettingNotInEffect("cipher_provider"));
    }
    if version.split_whitespace().next() != Some(CIPHER_VERSION) {
        return Err(StoreError::SettingNotInEffect("cipher_version"));
    }
    if !os_entropy_hook {
        return Err(StoreError::SettingNotInEffect("os_entropy_hook"));
    }
    Ok(())
}

fn require_exclusive_locking(conn: &Connection) -> StoreResult<()> {
    let mode: String = conn
        .query_row("PRAGMA main.locking_mode", [], |r| r.get(0))
        .map_err(db_error)?;
    if mode == "exclusive" {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect("locking_mode"))
    }
}

fn require_cipher_log_level_none(conn: &Connection) -> StoreResult<()> {
    let level: String = conn
        .query_row("PRAGMA cipher_log_level", [], |r| r.get(0))
        .map_err(db_error)?;
    if level == "NONE" {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect("cipher_log_level"))
    }
}

fn require_checkpoint_fullfsync(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "checkpoint_fullfsync")? {
        1 => Ok(()),
        _ => Err(StoreError::SettingNotInEffect("checkpoint_fullfsync")),
    }
}

fn pragma_int(conn: &Connection, name: &str) -> StoreResult<i64> {
    conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0))
        .map_err(db_error)
}

const KEY_PRAGMA_HEAD: &str = "PRAGMA key = \"x'";
const KEY_PRAGMA_TAIL: &str = "'\";";

fn key_pragma(key: &[u8; 32]) -> Zeroizing<String> {
    let mut pragma = Zeroizing::new(String::with_capacity(
        KEY_PRAGMA_HEAD.len() + 64 + KEY_PRAGMA_TAIL.len(),
    ));
    pragma.push_str(KEY_PRAGMA_HEAD);
    pragma.push_str(&hex_lower(key));
    pragma.push_str(KEY_PRAGMA_TAIL);
    pragma
}

fn hex_lower(bytes: &[u8; 32]) -> Zeroizing<String> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = Zeroizing::new(String::with_capacity(64));
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(target_os = "macos")]
fn held_look(conn: &Connection, store_dir: &Dir, form: NameForm) -> StoreResult<()> {
    let outside = || obstructed(StoreName::Database, Met::DatabaseOutsideTheDirectory);
    let (kind, device, file_number) = match store_dir.entry_identity(OsStr::new(DATABASE_FILE)) {
        Ok(found) => found,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(outside()),
        Err(e) => return Err(io_error(e)),
    };
    match kind {
        EntryKind::File => {}
        EntryKind::Link => return Err(obstructed(StoreName::Database, Met::Link)),
        _ => return Err(obstructed(StoreName::Database, Met::OtherKind)),
    }
    let mut moved: std::os::raw::c_int = 0;
    let asked = conn.file_control(
        Some(rusqlite::MAIN_DB),
        rusqlite::FileControl::HasMoved(&mut moved),
    );
    #[cfg(test)]
    let asked = code_seams::or_forced_at(code_seams::HELD_LOOK_HAS_MOVED, asked);
    asked.map_err(|e| after_a_code(store_dir, &e))?;
    if moved != 0 {
        return Err(outside());
    }
    let kept = conn.path().map(PathBuf::from).ok_or_else(outside)?;
    match volume_stat_of(&kept) {
        Ok((EntryKind::File, d, n)) if d == device && n == file_number => {}
        Ok(_) => return Err(outside()),
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(outside()),
        Err(e) => return Err(io_error(e)),
    }
    if form == NameForm::Volume {
        let (d, n) = store_dir.identity().map_err(io_error)?;
        let named = PathBuf::from(format!("/.vol/{d}/{n}")).join(DATABASE_FILE);
        if kept != named {
            return Err(obstructed(
                StoreName::StoreDirectory,
                Met::DirectoryNotAtItsName,
            ));
        }
    }
    refuse_a_moved_store_dir(store_dir)
}

#[cfg(target_os = "macos")]
fn volume_stat_of(name: &Path) -> std::io::Result<(EntryKind, u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(name)?;
    let kind = if meta.file_type().is_symlink() {
        EntryKind::Link
    } else if meta.is_file() {
        EntryKind::File
    } else if meta.is_dir() {
        EntryKind::Dir
    } else {
        EntryKind::Other
    };
    Ok((kind, meta.dev() as u64, meta.ino()))
}

#[cfg(windows)]
fn held_look(_conn: &Connection, store_dir: &Dir, _form: NameForm) -> StoreResult<()> {
    match store_dir.entry(OsStr::new(DATABASE_FILE)) {
        Ok(entry) if entry.kind == EntryKind::File => Ok(()),
        Ok(entry) if entry.kind == EntryKind::Link => {
            Err(obstructed(StoreName::Database, Met::Link))
        }
        Ok(_) => Err(obstructed(StoreName::Database, Met::OtherKind)),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(obstructed(
            StoreName::Database,
            Met::DatabaseOutsideTheDirectory,
        )),
        Err(e) => Err(io_error(e)),
    }
}

enum Keyed {
    Open { db: Db, store_key: StoreKey },
    Deleting(Option<Box<DeleteReport>>),
    Deleted,
}

/// The encrypted store: the SQLCipher database, the per-segment files
/// under `dir`, and the one store key in the clear only in memory.
pub struct EncryptedStore {
    dir: Dir,
    retention: Duration,
    custody: Box<dyn KeyCustody + Send>,
    keyed: Keyed,
    fs: Arc<dyn DurableFs>,
    unwrap_time: Duration,
    #[cfg_attr(
        not(all(test, unix)),
        expect(
            dead_code,
            reason = "a test reads whether the open's cleanup of a stopped create failed"
        )
    )]
    pending_left: bool,
    _writer_lock: DirLock,
}

impl EncryptedStore {
    /// Create a fresh store in `dir` under `custody`, whose roots directory
    /// holds the new root. It takes the writer lock, finishes what a stopped
    /// create left in `create.pending`, and answers
    /// [`StoreError::AlreadyExists`] when any entry stands at `store.key` and
    /// [`StoreError::Locked`] while another process holds the lock or the
    /// database, with nothing removed. Otherwise it removes the keyless
    /// leftovers (a segment that it cannot remove stays for the open-time
    /// sweep), records the custody kind and root id in `create.pending`, opens
    /// a new database under a random store key, makes the root, publishes the
    /// key file create-only and removes `create.pending`. When a step fails it
    /// destroys the root that it made, removes the key file and the database
    /// files, every step attempted, and answers the step's error, or a
    /// removal's error when one failed.
    pub fn create(
        dir: &Path,
        custody: Box<dyn KeyCustody + Send>,
        retention: Duration,
    ) -> StoreResult<Self> {
        Self::create_with(dir, custody, retention, &mut || {})
    }

    pub(crate) fn create_with(
        dir: &Path,
        custody: Box<dyn KeyCustody + Send>,
        retention: Duration,
        after_the_handle: &mut dyn FnMut(),
    ) -> StoreResult<Self> {
        durable::create_private_dir_all(dir).map_err(io_error)?;
        let store = Dir::open(dir).map_err(store_dir_error)?;
        store.make_private().map_err(io_error)?;
        after_the_handle();
        let dir = &store;
        let _writer_lock = DirLock::take(dir)?;
        let key_file = KeyFile::of_store(dir)?;
        match Db::refuse_a_live_holder(dir) {
            Ok(()) | Err(StoreError::Obstructed { .. }) => {}
            Err(e) => return Err(e),
        }
        if let Some(roots) = custody.roots() {
            clear_pending(dir, roots, key_file.names())?;
            if file_root::root_file_present(roots, custody.root_id()) {
                return Err(StoreError::Custody(
                    custody
                        .create_root()
                        .err()
                        .unwrap_or(CustodyError::RootExists),
                ));
            }
        }
        if key_file.present()? {
            return Err(StoreError::AlreadyExists);
        }
        failpoint::hit("create_before_the_database_step")?;
        #[cfg(test)]
        failpoint::seam(SEAM_BEFORE_THE_DATABASE_STEP);
        Db::destroy_in(dir)?;
        let _ = segment::remove_segments_tree(dir);
        write_pending(dir, custody.as_ref())?;
        let store_key = StoreKey::random()?;
        let db = Db::open(dir, store_key.expose()).map_err(|e| Self::without_database(dir, e))?;
        let db = Self::under_a_new_root(dir, custody.as_ref(), db)?;
        #[cfg(test)]
        failpoint::seam(SEAM_AFTER_THE_ROOT);
        match Self::write_key_file(&key_file, custody.as_ref(), &store_key)
            .and_then(|()| failpoint::hit("create_after_the_key_file"))
            .and_then(|()| remove_pending(dir))
        {
            Ok(()) => Ok(Self {
                dir: store,
                retention,
                custody,
                keyed: Keyed::Open { db, store_key },
                fs: Arc::new(OsFs),
                unwrap_time: Duration::ZERO,
                pending_left: false,
                _writer_lock,
            }),
            Err(e) => {
                drop(db);
                Err(Self::abandon_create(dir, &key_file, custody.as_ref(), e))
            }
        }
    }

    fn under_a_new_root(dir: &Dir, custody: &dyn KeyCustody, db: Db) -> StoreResult<Db> {
        match custody.create_root() {
            Ok(()) => Ok(db),
            Err(e) => {
                drop(db);
                Err(Self::without_database(dir, StoreError::Custody(e)))
            }
        }
    }

    fn write_key_file(
        key_file: &KeyFilePlace,
        custody: &dyn KeyCustody,
        store_key: &StoreKey,
    ) -> StoreResult<()> {
        let wrapped = custody.wrap_store_key(store_key, KeyLabel::STORE)?;
        key_file.publish(&KeyFile::new(custody, KeyLabel::STORE, &wrapped))
    }

    fn without_database(dir: &Dir, cause: StoreError) -> StoreError {
        let database = Db::destroy_in(dir);
        let pending = remove_pending(dir);
        match database.and(pending) {
            Ok(()) => cause,
            Err(left_behind) => left_behind,
        }
    }

    fn abandon_create(
        dir: &Dir,
        key_file: &KeyFilePlace,
        custody: &dyn KeyCustody,
        cause: StoreError,
    ) -> StoreError {
        let root = custody.destroy_root().map_err(StoreError::Custody);
        let key_file = match key_file.destroy() {
            Removal { gone: true, .. } => Ok(()),
            Removal { gone: false, .. } => Err(StoreError::Io(ErrorKind::Other)),
        };
        let database = Db::destroy_in(dir);
        let pending = match root {
            Ok(()) => remove_pending(dir),
            Err(_) => Ok(()),
        };
        match root.and(key_file).and(database).and(pending) {
            Ok(()) => cause,
            Err(left_behind) => left_behind,
        }
    }

    /// Open an existing store: read the store-key file, **unwrap the store key
    /// through the custody that it names, under the roots directory `roots`**,
    /// and open the database. `open` never makes a database. An open that is
    /// refused destroys no root file, writes nothing under the roots
    /// directory and leaves `create.pending` as it found it. Once the database
    /// is open, it finishes what a stopped create left in `create.pending`:
    /// it keeps the root that the key file it read names and destroys another
    /// one; a cleanup that cannot finish leaves the open as it is, and the
    /// next open tries again. Fails
    /// [`StoreError::Locked`] while another writer holds the directory or the
    /// database, [`StoreError::Sealed`] when `store.key` is missing (delete-all
    /// ran, or no store was made here), [`StoreError::DatabaseMissing`] when
    /// `store.db` is missing or empty, before any custody call, and
    /// [`StoreError::DatabaseUnreadable`] when the unwrapped key does not open
    /// the database: a key of another store, a damaged page, a cut file, or a
    /// database without the store's tables. Fails
    /// [`StoreError::KeyFileUnparseable`] for a key file that does not parse or
    /// whose wrapped key has another length than a wrap,
    /// [`StoreError::KeyFileOfAnotherLabel`] for a key file of another label,
    /// [`StoreError::OlderStore`] for a key file of version 1, the obstructed
    /// variant when a link or an entry of another kind stands at a store name
    /// or the directory leaves its name, and [`StoreError::Custody`] with the
    /// custody's own variant when the custody kind is not in this build or the
    /// unwrap fails: [`CustodyError::RootMissing`], [`CustodyError::RootCorrupt`]
    /// and [`CustodyError::WrapAuthFailed`] mean that the key cannot come back,
    /// and [`CustodyError::RootDenied`] that the root file cannot be read.
    /// No call waits on a
    /// FIFO.
    pub fn open(dir: &Path, roots: &Path, retention: Duration) -> StoreResult<Self> {
        Self::open_with(dir, roots, retention, custody_for)
    }

    pub(crate) fn open_with(
        dir: &Path,
        roots: &Path,
        retention: Duration,
        custody_for: impl FnOnce(&CustodyLocator) -> Result<Box<dyn KeyCustody + Send>, CustodyError>,
    ) -> StoreResult<Self> {
        let store = Dir::open(dir).map_err(store_dir_error)?;
        store.make_private().map_err(io_error)?;
        let _writer_lock = DirLock::take(&store)?;
        let key_file = KeyFile::of_store(&store)?.read(KeyLabel::STORE)?;
        let wrapped = key_file.wrapped_store_key()?;
        segment::refuse_a_linked_segments_root(&store)?;
        Db::require_file(&store)?;
        Db::refuse_a_live_holder(&store)?;
        let custody = custody_for(&key_file.locator(roots))?;
        let started = Instant::now();
        let store_key = custody.unwrap_store_key(&wrapped, KeyLabel::STORE)?;
        let unwrap_time = started.elapsed();
        let db = Db::open_existing(&store, store_key.expose())?;
        let pending_left = clear_pending(&store, roots, KeyNames::Root(key_file.root_id)).is_err();
        Ok(Self {
            dir: store,
            retention,
            custody,
            keyed: Keyed::Open { db, store_key },
            fs: Arc::new(OsFs),
            unwrap_time,
            pending_left,
            _writer_lock,
        })
    }

    fn keyed(&self) -> StoreResult<(&Db, &StoreKey)> {
        match &self.keyed {
            Keyed::Open { db, store_key } => Ok((db, store_key)),
            Keyed::Deleting(_) | Keyed::Deleted => Err(StoreError::Sealed),
        }
    }

    pub(crate) fn db(&self) -> StoreResult<&Db> {
        self.keyed().map(|(db, _)| db)
    }

    pub(crate) fn dir(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn handle(&self) -> &Dir {
        &self.dir
    }

    /// How long the custody unwrap of the store key took when `open` made this
    /// handle; zero for a handle from `create`, which unwraps nothing.
    pub fn unwrap_time(&self) -> Duration {
        self.unwrap_time
    }

    pub(crate) fn fs(&self) -> &dyn DurableFs {
        self.fs.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn use_fs(&mut self, fs: Arc<dyn DurableFs>) {
        self.fs = fs;
    }

    pub(crate) fn store_key(&self) -> StoreResult<&StoreKey> {
        self.keyed().map(|(_, key)| key)
    }

    pub(crate) fn answer(&self, e: StoreError) -> StoreError {
        match e {
            StoreError::Db(rusqlite::ffi::SQLITE_IOERR_FSTAT | rusqlite::ffi::SQLITE_CANTOPEN)
            | StoreError::SegmentMissing
                if matches!(self.dir.still_named(), Ok(false)) =>
            {
                obstructed(StoreName::StoreDirectory, Met::DirectoryNotAtItsName)
            }
            other => other,
        }
    }

    /// Read and decrypt a segment's plaintext: unwrap its per-segment key with the
    /// store key, then read the file twice, first to authenticate every chunk and
    /// then to write the plaintext to `out`. Nothing reaches `out` before the first
    /// read has authenticated the whole file; a file that changes between the two
    /// reads ends in an error, after `out` has taken the chunks that still
    /// authenticate. Fails [`StoreError::Sealed`] if the segment has been
    /// crypto-shredded or never finished writing (its key row is NULL or gone),
    /// or if this store never wrote a segment with that id, and
    /// [`StoreError::Corrupt`] if the file does not authenticate.
    pub fn read_segment(&self, segment_id: uuid::Uuid, out: &mut dyn Write) -> StoreResult<u64> {
        let (db, store_key) = self.keyed()?;
        let wrapped: Option<Vec<u8>> = db
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
                [segment_id.simple().to_string()],
                |r| r.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StoreError::Sealed,
                other => self.answer(db_error(other)),
            })?;
        let wrapped = wrapped.ok_or(StoreError::Sealed)?;
        let seg_key = segment::unwrap_segment_key(store_key.expose(), &wrapped)?;
        segment::read_segment_file(&self.dir, &seg_key, segment_id, out).map_err(|e| self.answer(e))
    }

    pub(crate) fn write_segment_with_expiry(
        &mut self,
        bytes: &mut dyn Read,
        created_at: u64,
        expires_at: u64,
    ) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        let (db, store_key) = self.keyed()?;
        let segment_id = uuid::Uuid::now_v7();
        let seg_key = SegmentKey::random()?;
        let wrapped = segment::wrap_segment_key(store_key.expose(), &seg_key)?;
        let segment_key_id = db
            .reserve_segment_key(segment_id, created_at, expires_at)
            .map_err(|e| self.answer(e))?;
        let header = SegmentHeader {
            segment_id,
            segment_key_id,
        };
        let byte_len =
            segment::write_segment_file(&self.dir, self.fs.as_ref(), &seg_key, &header, bytes)?;
        if let Err(e) = db.commit_segment_key(segment_key_id, &wrapped) {
            segment::remove_written_segment(&self.dir, segment_id)?;
            return Err(self.answer(e));
        }
        Ok((
            SegmentRef {
                segment_id,
                byte_offset: 0,
                byte_len,
            },
            segment_key_id,
        ))
    }

    pub(crate) fn root_step(&self) -> RootsLeft {
        let destroyed = self.custody.destroy_root();
        let named = self.custody.root_id();
        match self.custody.roots() {
            Some(roots) => file_root::destroy_every_root(roots, Some(named)),
            None => RootsLeft {
                left: destroyed.err().map(|_| named).into_iter().collect(),
                listed: true,
            },
        }
    }

    pub(crate) fn untouched(&self) -> DeleteReport {
        DeleteReport {
            keyed_segments: None,
            roots: RootsLeft {
                left: vec![self.custody.root_id()],
                listed: true,
            },
            root_not_handled: None,
            key_file_gone: false,
            database_left: true,
            files_left: 0,
            not_followed: 0,
        }
    }

    pub(crate) fn remove_files_after(&mut self, roots: RootsLeft) {
        self.keyed = Keyed::Deleting(None);
        let mut report = remove_the_store_files(&self.dir, None, roots);
        if !matches!(self.dir.still_named(), Ok(true)) {
            report.not_followed += 1;
        }
        self.keyed = Keyed::Deleting(Some(Box::new(report)));
    }

    pub(crate) fn deleting_report(&self) -> Option<DeleteReport> {
        match &self.keyed {
            Keyed::Deleting(Some(report)) => Some((**report).clone()),
            _ => None,
        }
    }

    #[cfg(all(test, unix))]
    pub(crate) fn pending_left(&self) -> bool {
        self.pending_left
    }

    #[cfg(all(test, unix))]
    pub(crate) fn writer_lock_file(&self) -> &std::fs::File {
        &self._writer_lock.file
    }

    pub(crate) fn mark_deleted(&mut self) {
        self.keyed = Keyed::Deleted;
    }

    /// The delete tail on a store directory that no handle holds: it takes
    /// the writer lock, destroys the root that `store.key` or `create.pending`
    /// names and then every other root file under `roots`, removes the key
    /// file, the database files and the segments tree through a handle of the
    /// directory, never opens the database and follows no link. A link or an
    /// entry of another kind at the directory's own name is removed as the
    /// entry that it is and counted as not followed. A key file of version 1
    /// is removed with the rest, and its root is reported as not handled by
    /// this build. Answers as [`EncryptedStore::delete_all`] does, and
    /// [`StoreError::Locked`] while another process holds the directory.
    pub fn delete_store_dir(store_dir: &Path, roots: &Path) -> StoreResult<DeleteReport> {
        let store = match Dir::open(store_dir) {
            Ok(store) => store,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return finished(DeleteReport {
                    keyed_segments: None,
                    roots: file_root::destroy_every_root(roots, None),
                    root_not_handled: None,
                    key_file_gone: true,
                    database_left: false,
                    files_left: 0,
                    not_followed: 0,
                })
            }
            Err(e) if durable::refusal(&e).is_some() => {
                return finished(remove_what_stands_at(store_dir, roots))
            }
            Err(e) => return Err(io_error(e)),
        };
        let _writer_lock = DirLock::take(&store)?;
        let place = KeyFile::of_store(&store)?;
        let named = place
            .named_root()
            .or_else(|| read_pending(&store).map(|pending| pending.root_id));
        let root_not_handled = place.older_root();
        let roots = file_root::destroy_every_root(roots, named);
        let mut report = remove_the_store_files(&store, None, roots);
        report.root_not_handled = root_not_handled;
        finished(report)
    }
}

fn finished(report: DeleteReport) -> StoreResult<DeleteReport> {
    if report.complete() {
        Ok(report)
    } else {
        Err(StoreError::DeleteIncomplete(Box::new(report)))
    }
}

fn remove_what_stands_at(store_dir: &Path, roots: &Path) -> DeleteReport {
    let roots = file_root::destroy_every_root(roots, None);
    let gone = match (store_dir.parent(), store_dir.file_name()) {
        (Some(parent), Some(name)) => Dir::open(parent)
            .and_then(|parent| {
                parent.remove_entry(name)?;
                parent.sync()
            })
            .is_ok(),
        _ => false,
    };
    DeleteReport {
        keyed_segments: None,
        roots,
        root_not_handled: None,
        key_file_gone: true,
        database_left: false,
        files_left: u64::from(!gone),
        not_followed: 1,
    }
}

fn remove_the_store_files(
    store: &Dir,
    keyed_segments: Option<u64>,
    roots: RootsLeft,
) -> DeleteReport {
    let key_file = match KeyFile::of_store(store) {
        Ok(place) => place.destroy(),
        Err(_) => Removal {
            gone: false,
            not_followed: 0,
        },
    };
    let database = Db::destroy_through(store);
    let segments = segment::remove_segments_through(store);
    let pending_left = u64::from(remove_pending(store).is_err());
    DeleteReport {
        keyed_segments,
        roots,
        root_not_handled: None,
        key_file_gone: key_file.gone,
        database_left: database.left,
        files_left: segments.left + pending_left,
        not_followed: key_file.not_followed + database.not_followed + segments.not_followed,
    }
}

impl SegmentStore for EncryptedStore {
    fn append_event(&mut self, event: &CaptureStreamEvent) -> StoreResult<()> {
        self.db()?.insert_event(event).map_err(|e| self.answer(e))
    }

    fn write_segment(&mut self, bytes: &mut dyn Read) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        let created = unix_secs_now();
        let expires = created + self.retention.as_secs();
        self.write_segment_with_expiry(bytes, created, expires)
    }

    fn size_bytes(&self) -> StoreResult<u64> {
        dir_size(&self.dir)
    }
}

fn listed(dir: &Dir) -> StoreResult<Vec<durable::Entry>> {
    match dir.entries() {
        Ok(entries) => Ok(entries),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(io_error(e)),
    }
}

pub(crate) fn dir_size(dir: &Dir) -> StoreResult<u64> {
    type Pending = Vec<(Option<Rc<Dir>>, std::ffi::OsString)>;
    fn take(
        parent: Option<&Rc<Dir>>,
        entries: Vec<durable::Entry>,
        total: &mut u64,
        pending: &mut Pending,
    ) {
        for entry in entries {
            match entry.kind {
                EntryKind::File => *total += entry.len,
                EntryKind::Dir => pending.push((parent.cloned(), entry.name)),
                EntryKind::Link | EntryKind::Other => {}
            }
        }
    }
    let mut total = 0u64;
    let mut pending = Pending::new();
    take(None, listed(dir)?, &mut total, &mut pending);
    #[cfg(test)]
    failpoint::seam(SEAM_SIZE_AFTER_THE_LISTING);
    while let Some((parent, name)) = pending.pop() {
        let opened = match &parent {
            Some(parent) => parent.open_dir(&name),
            None => dir.open_dir(&name),
        };
        let below = match opened {
            Ok(below) => Rc::new(below),
            Err(e) if e.kind() == ErrorKind::NotFound || durable::refusal(&e).is_some() => continue,
            Err(e) => return Err(io_error(e)),
        };
        take(Some(&below), listed(&below)?, &mut total, &mut pending);
    }
    Ok(total)
}

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) use crate::store::pages;

    use std::ops::{Deref, DerefMut};
    use std::path::Path;

    use super::EncryptedStore;
    use crate::store::roots::RootGuard;
    use crate::store::DEFAULT_RETENTION;

    pub(crate) struct TestStore {
        store: EncryptedStore,
        _root: RootGuard,
    }

    impl Deref for TestStore {
        type Target = EncryptedStore;
        fn deref(&self) -> &EncryptedStore {
            &self.store
        }
    }

    impl DerefMut for TestStore {
        fn deref_mut(&mut self) -> &mut EncryptedStore {
            &mut self.store
        }
    }

    pub(crate) fn make_store(dir: &Path) -> TestStore {
        let root = RootGuard::fresh();
        let custody = crate::store::roots::custody(root.id());
        TestStore {
            store: EncryptedStore::create(dir, Box::new(custody), DEFAULT_RETENTION).unwrap(),
            _root: root,
        }
    }

    #[cfg(unix)]
    pub(crate) fn duplicate_writer_lock_descriptor(store: &EncryptedStore) -> std::fs::File {
        store.writer_lock_file().try_clone().unwrap()
    }

    pub(crate) fn key_statement(key: &[u8; 32]) -> zeroize::Zeroizing<String> {
        super::key_pragma(key)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::make_store;
    use super::*;
    use crate::event::{CapturedAt, Modality, MonotonicSequence};
    use crate::store::roots::RootGuard;
    use crate::store::{SegmentStore, DEFAULT_RETENTION};

    #[test]
    fn the_key_pragma_is_built_in_one_allocation_that_never_grows() {
        let pragma = key_pragma(&[0xab; 32]);
        assert_eq!(pragma.capacity(), pragma.len());
        assert!(pragma.starts_with(KEY_PRAGMA_HEAD) && pragma.ends_with(KEY_PRAGMA_TAIL));
        let hex = hex_lower(&[0xab; 32]);
        assert_eq!(hex.capacity(), hex.len());
    }

    #[cfg(unix)]
    #[test]
    fn a_write_ahead_log_that_cannot_be_read_fails_the_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open(&Dir::open(dir.path()).unwrap(), &[7u8; 32]).unwrap();
        db.log_name = "no-such-wal".into();
        assert!(
            db.checkpoint_truncate().is_ok(),
            "a missing write-ahead log is not an empty one"
        );
        let looping = dir.path().join("looping-wal");
        std::os::unix::fs::symlink(&looping, &looping).unwrap();
        db.log_name = "looping-wal".into();
        let checkpoint = db.checkpoint_truncate();
        assert!(
            checkpoint.is_err(),
            "a write-ahead log that cannot be read let the checkpoint succeed: {checkpoint:?}"
        );
    }

    #[test]
    fn an_event_whose_index_row_fails_leaves_no_event_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, key_id) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        store
            .db()
            .unwrap()
            .connection()
            .execute_batch("DROP TABLE events_fts;")
            .unwrap();
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: SystemTime::now(),
                sequence: MonotonicSequence::new(1),
            },
            Modality::ScreenFrame,
            "app".to_string(),
            seg_ref,
            key_id,
        );
        let appended = store.append_event(&event);
        let rows: i64 = store
            .db()
            .unwrap()
            .connection()
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert!(
            appended.is_err(),
            "the append with no index table succeeded"
        );
        assert_eq!(rows, 0, "a failed append left its event row");
    }

    #[cfg(unix)]
    #[test]
    fn a_store_directory_that_cannot_be_read_fails_the_size_and_does_not_shrink_it() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, _) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        let shard = segment::segment_path(dir.path(), seg_ref.segment_id)
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o000)).unwrap();
        let size = store.size_bytes();
        std::fs::set_permissions(&shard, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            matches!(size, Err(StoreError::Io(ErrorKind::PermissionDenied))),
            "{size:?}"
        );
    }

    #[test]
    fn a_checkpoint_that_leaves_bytes_in_the_wal_is_incomplete_and_no_sqlite_code() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open(&Dir::open(dir.path()).unwrap(), &[7u8; 32]).unwrap();
        let wal = dir.path().join("a-wal-that-keeps-bytes");
        std::fs::write(&wal, [1u8; 100]).unwrap();
        db.log_name = "a-wal-that-keeps-bytes".into();
        let checkpoint = db.checkpoint_truncate();
        assert!(
            matches!(
                checkpoint,
                Err(StoreError::CheckpointIncomplete {
                    busy: 0,
                    wal_bytes: 100
                })
            ),
            "{checkpoint:?}"
        );
    }

    #[test]
    fn a_key_row_that_holds_a_key_is_not_committed_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (_, key_id) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        let again = store.db().unwrap().commit_segment_key(key_id, &[1u8; 72]);
        assert!(matches!(again, Err(StoreError::Corrupt(_))), "{again:?}");
    }

    #[test]
    fn a_wrapped_segment_key_that_does_not_open_under_the_store_key_reads_as_corrupt_not_sealed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (segment, _) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        store
            .db()
            .unwrap()
            .connection()
            .execute(
                "UPDATE segment_keys SET wrapped_key = ?1 WHERE segment_id = ?2",
                rusqlite::params![vec![7u8; 72], segment.segment_id.simple().to_string()],
            )
            .unwrap();
        let read = store.read_segment(segment.segment_id, &mut Vec::new());
        assert!(matches!(read, Err(StoreError::Corrupt(_))), "{read:?}");
    }

    #[test]
    fn pragmas_are_set() {
        let dir = tempfile::tempdir().unwrap();
        let store = make_store(dir.path());
        let db = store.db().unwrap();
        assert_eq!(db.pragma_int("secure_delete").unwrap(), 1);
        assert_eq!(db.pragma_int("temp_store").unwrap(), 2);
        assert_eq!(db.fts_secure_delete().unwrap(), 1);
        assert_eq!(db.pragma_int("checkpoint_fullfsync").unwrap(), 1);
        let text = |pragma: &str| -> String {
            db.connection().query_row(pragma, [], |r| r.get(0)).unwrap()
        };
        assert_eq!(text("PRAGMA journal_mode"), "wal");
        assert_eq!(text("PRAGMA main.locking_mode"), "exclusive");
        assert_eq!(text("PRAGMA cipher_log_level"), "NONE");
        assert_eq!(db.cipher_provider().unwrap(), "libtomcrypt");
        assert!(
            text("PRAGMA cipher_version").starts_with("4.19.0"),
            "{}",
            text("PRAGMA cipher_version")
        );
        assert!(
            halvo_sqlcipher::os_entropy_hook_installed(),
            "LibTomCrypt's random source is not the OS entropy hook"
        );
    }

    #[test]
    fn a_cipher_other_than_the_pinned_one_is_refused() {
        let cases = [
            ("libtomcrypt", "4.19.0 community", true, None),
            ("openssl", "4.19.0 community", true, Some("cipher_provider")),
            (
                "libtomcrypt",
                "4.6.1 community",
                true,
                Some("cipher_version"),
            ),
            (
                "libtomcrypt",
                "4.19.01 community",
                true,
                Some("cipher_version"),
            ),
            (
                "libtomcrypt",
                "4.19.0 community",
                false,
                Some("os_entropy_hook"),
            ),
        ];
        for (provider, version, hook, refused) in cases {
            let answer = cipher_in_effect(provider, version, hook);
            match refused {
                None => assert!(answer.is_ok(), "{provider} {version} {hook}: {answer:?}"),
                Some(name) => assert!(
                    matches!(answer, Err(StoreError::SettingNotInEffect(n)) if n == name),
                    "{provider} {version} {hook}: {answer:?}"
                ),
            }
        }
    }

    fn persistent_store(dir: &Path) -> (EncryptedStore, RootId) {
        let root_id = RootId::fresh().unwrap();
        let custody = Box::new(crate::store::roots::custody(root_id));
        let store = EncryptedStore::create(dir, custody, DEFAULT_RETENTION).unwrap();
        (store, root_id)
    }

    #[test]
    fn a_second_writer_on_the_same_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (first, root_id) = persistent_store(dir.path());
        let _root = RootGuard::new(root_id);
        assert!(matches!(
            crate::store::roots::open(dir.path(), DEFAULT_RETENTION),
            Err(StoreError::Locked)
        ));
        drop(first);
        crate::store::roots::open(dir.path(), DEFAULT_RETENTION)
            .expect("a dropped writer leaves the store free to reopen");
        crate::store::roots::custody(root_id)
            .destroy_root()
            .unwrap();
    }

    #[test]
    fn a_second_take_of_the_directory_lock_is_refused_until_the_first_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let handle = Dir::open(dir.path()).unwrap();
        let first = DirLock::take(&handle).unwrap();
        let other_handle = Dir::open(dir.path()).unwrap();
        let second_beside_the_first = DirLock::take(&other_handle).map(drop);
        assert!(
            matches!(second_beside_the_first, Err(StoreError::Locked)),
            "a second take beside a live directory lock answered {second_beside_the_first:?}"
        );
        drop(first);
        DirLock::take(&other_handle)
            .expect("a dropped directory lock leaves the directory free to lock");
    }

    #[cfg(unix)]
    #[test]
    fn a_dropped_directory_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open() {
        let dir = tempfile::tempdir().unwrap();
        let handle = Dir::open(dir.path()).unwrap();
        let never_put_in_a_store = DirLock::take(&handle).unwrap();
        let held_as_a_spawned_child_holds_it = never_put_in_a_store.file.try_clone().unwrap();

        drop(never_put_in_a_store);
        let taken_again = DirLock::take(&Dir::open(dir.path()).unwrap()).map(drop);

        drop(held_as_a_spawned_child_holds_it);
        assert!(
            taken_again.is_ok(),
            "a directory lock dropped outside a store was still held: {:?}",
            taken_again.err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_directory_lock_makes_no_file_and_holds_after_every_name_in_the_directory_is_unlinked() {
        let dir = tempfile::tempdir().unwrap();
        let root = RootGuard::fresh();
        let store = EncryptedStore::create(dir.path(), Box::new(root.custody()), DEFAULT_RETENTION)
            .unwrap();
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !names.iter().any(|name| name == "writer.lock"),
            "the store made a lock file: {names:?}"
        );
        for name in &names {
            let _ = std::fs::remove_file(dir.path().join(name));
        }
        let second = DirLock::take(&Dir::open(dir.path()).unwrap()).map(drop);
        assert!(
            matches!(second, Err(StoreError::Locked)),
            "a second take after every name was unlinked answered {second:?}"
        );
        drop(store);
    }

    #[test]
    fn create_refuses_a_directory_that_already_holds_a_store() {
        let dir = tempfile::tempdir().unwrap();
        drop(make_store(dir.path()));
        let second_root = RootGuard::fresh();
        let again = EncryptedStore::create(
            dir.path(),
            Box::new(crate::store::roots::custody(second_root.id())),
            DEFAULT_RETENTION,
        );
        assert!(matches!(again, Err(StoreError::AlreadyExists)));
    }

    #[test]
    fn open_reports_a_missing_root_as_root_missing() {
        let dir = tempfile::tempdir().unwrap();
        let (store, root_id) = persistent_store(dir.path());
        let _root = RootGuard::new(root_id);
        drop(store);
        crate::store::roots::custody(root_id)
            .destroy_root()
            .unwrap();
        assert!(matches!(
            crate::store::roots::open(dir.path(), DEFAULT_RETENTION),
            Err(StoreError::Custody(CustodyError::RootMissing))
        ));
    }

    #[test]
    fn open_rebuilds_the_custody_from_the_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, root_id) = persistent_store(dir.path());
        let _root = RootGuard::new(root_id);
        let (seg_ref, _) = store
            .write_segment(&mut std::io::Cursor::new(b"persisted"))
            .unwrap();
        drop(store);
        let reopened = crate::store::roots::open(dir.path(), DEFAULT_RETENTION).unwrap();
        let mut plain = Vec::new();
        reopened
            .read_segment(seg_ref.segment_id, &mut plain)
            .unwrap();
        assert_eq!(plain, b"persisted");
    }

    #[test]
    fn writes_and_reads_a_segment_and_event() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, key_id) = store
            .write_segment(&mut std::io::Cursor::new(b"screen frame bytes"))
            .unwrap();
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: SystemTime::now(),
                sequence: MonotonicSequence::new(1),
            },
            Modality::ScreenFrame,
            "com.apple.Terminal".to_string(),
            seg_ref.clone(),
            key_id,
        );
        store.append_event(&event).unwrap();
        let wrapped: Vec<u8> = store
            .db()
            .unwrap()
            .connection()
            .query_row(
                "SELECT wrapped_key FROM segment_keys WHERE id=?1",
                [key_id.0],
                |r| r.get(0),
            )
            .unwrap();
        let seg_key =
            segment::unwrap_segment_key(store.store_key().unwrap().expose(), &wrapped).unwrap();
        let mut plain = Vec::new();
        segment::read_segment_file(store.handle(), &seg_key, seg_ref.segment_id, &mut plain)
            .unwrap();
        assert_eq!(plain, b"screen frame bytes");
        let mut again = Vec::new();
        assert_eq!(
            store.read_segment(seg_ref.segment_id, &mut again).unwrap(),
            18
        );
        assert_eq!(again, b"screen frame bytes");
    }

    #[test]
    fn the_open_flag_constants_carry_no_follow_and_neither_uri_nor_create() {
        for flags in [DATABASE_OPEN, PROBE_OPEN] {
            assert!(flags.contains(OpenFlags::SQLITE_OPEN_NOFOLLOW), "{flags:?}");
            assert!(!flags.contains(OpenFlags::SQLITE_OPEN_URI), "{flags:?}");
            assert!(!flags.contains(OpenFlags::SQLITE_OPEN_CREATE), "{flags:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn sqlite_under_the_constant_flags_refuses_a_link_and_the_store_maps_it_to_corrupt() {
        halvo_sqlcipher::initialize().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let resolved = Dir::open(dir.path()).unwrap();
        let target = resolved.path().join("target.db");
        Connection::open(&target)
            .unwrap()
            .execute_batch("CREATE TABLE t (x INTEGER);")
            .unwrap();
        let bytes = std::fs::read(&target).unwrap();
        let link = resolved.path().join("store.db");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        for flags in [DATABASE_OPEN, PROBE_OPEN] {
            let answer = Connection::open_with_flags(&link, flags)
                .and_then(|conn| {
                    conn.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))
                })
                .map_err(|e| after_a_code(&resolved, &e));
            assert!(
                matches!(
                    &answer,
                    Err(StoreError::Obstructed {
                        name: StoreName::Database,
                        met: Met::Link
                    })
                ),
                "{flags:?}: {answer:?}"
            );
        }
        let mut names: Vec<String> = std::fs::read_dir(resolved.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["store.db", "target.db"]);
        assert!(std::fs::read(&target).unwrap() == bytes);
    }
}
