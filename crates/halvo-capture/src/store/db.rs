//! SQLCipher metadata store and the [`EncryptedStore`].

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use halvo_sqlcipher::rusqlite::config::DbConfig;
use halvo_sqlcipher::rusqlite::{self, Connection, ErrorCode, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::custody::file_root::{self, NamedBy, NamedRoot, RootStepForm, RootsLeft};
use crate::custody::{
    custody_for, is_label, CustodyError, CustodyKind, CustodyLocator, FileIdentity, KeyCustody,
    KeyLabel, RootId, RootsHandle, StoreKey, WrappedKey,
};
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

use super::durable::{self, Dir, DurableFs, EntryKind, OsFs, Refused};
use super::lock::DirLock;
use super::segment::{self, SegmentHeader, SegmentKey};
use super::{
    DeleteReport, KindTotal, Layout, Met, OpenFor, Ordinal, Reconciled, Refusal, RootReading,
    SegmentStore, SegmentWrite, StoreError, StoreName, StoreResult, UtcDay, WriteFault, Written,
    KINDS_MAX, KIND_MAX_BYTES, PAYLOAD_MAX_BYTES,
};

const KEY_FILE_VERSION: u32 = 2;
#[cfg(target_os = "macos")]
const STATS_AFTER_THE_FIRST: u32 = 2;
const DATABASE_VERSION: i64 = 2;
const PAGE_SIZE: i64 = 4096;
/// The name of the store directory in a data directory (C3.5).
const STORE_DIR_NAME: &str = "store";
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
pub(in crate::store) const SEAM_BEFORE_THE_LOOK: &str = "a database open, before its look";
#[cfg(test)]
pub(in crate::store) const SEAM_AFTER_THE_LAST_LOOK: &str = "a database open, after its last look";
#[cfg(test)]
pub(in crate::store) const SEAM_BEFORE_THE_MODE_STEP: &str = "the mode step of store.db";
#[cfg(test)]
pub(in crate::store) const SEAM_SIZE_AFTER_THE_LISTING: &str = "the size, after its first listing";
#[cfg(test)]
pub(in crate::store) const SEAM_AFTER_THE_FIRST_READ: &str =
    "a database open, after its first read";
#[cfg(test)]
pub(in crate::store) const SEAM_BEFORE_THE_DATABASE_STEP: &str =
    "a create, between its probe and its database step";
#[cfg(test)]
pub(in crate::store) const SEAM_AFTER_THE_ROOT_STEP: &str = "a delete-all, after its root step";
#[cfg(test)]
pub(in crate::store) const SEAM_BEFORE_THE_FIRST_READ: &str =
    "a database open, before its first read";
#[cfg(test)]
pub(in crate::store) const SEAM_AFTER_THE_ROOT: &str = "a create, after its root";
/// A place where a test acts in a create whose look met an entry at the
/// root's name, before the create picks its answer.
#[cfg(test)]
pub(in crate::store) const SEAM_IN_THE_REFUSAL_ARM: &str =
    "a create, after its look met an entry at the root's name";
/// A place where a test acts after the tail opened the store directory and
/// before its root step.
#[cfg(test)]
pub(in crate::store) const SEAM_BEFORE_THE_TAILS_ROOT_STEP: &str =
    "the tail, after its look at the store and before its root step";
/// A place where a test acts after the tail on a data directory opened that
/// directory and before it opens the store directory from that handle
/// (ruling ST-48 U5 (b)).
#[cfg(test)]
pub(in crate::store) const SEAM_AFTER_THE_DATA_DIRECTORY_OPEN: &str =
    "the tail, after it opened the data directory";
#[cfg(test)]
pub(in crate::store) const SEAM_AT_THE_LOG_CHECK: &str =
    "an open, at the first instant of the log's check";
#[cfg(test)]
pub(in crate::store) const SEAM_BEFORE_THE_PAGE_LOOK: &str = "a create, before its page look";
#[cfg(test)]
pub(in crate::store) const SEAM_BEFORE_THE_DRAIN: &str = "an open, before the drain of its log";

fn obstructed(name: StoreName, met: Met) -> StoreError {
    StoreError::Obstructed { name, met }
}

#[derive(Deserialize)]
struct KeyFileVersion {
    version: i64,
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
pub(in crate::store) struct KeyFile {
    pub version: u32,
    pub custody_kind: CustodyKind,
    pub root_id: RootId,
    pub namespace: String,
    pub wrapped_key: String,
}

pub(in crate::store) struct KeyFilePlace {
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

/// What a key file names for the delete tail: a root whose file this build
/// destroys under the roots directory, a root that it does not handle (a key
/// file of another version, or of a custody that keeps no root file under the
/// roots directory), or nothing.
enum KeyFileNames {
    Destroys(RootId),
    NotHandled(RootId),
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::store) struct Removal {
    pub(crate) gone: bool,
    pub(crate) not_followed: u64,
}

impl KeyFile {
    pub(in crate::store) fn new(
        custody: &dyn KeyCustody,
        label: KeyLabel,
        wrapped: &WrappedKey,
    ) -> Self {
        KeyFile {
            version: KEY_FILE_VERSION,
            custody_kind: custody.kind(),
            root_id: custody.root_id(),
            namespace: label.as_str().to_string(),
            wrapped_key: BASE64.encode(&wrapped.0),
        }
    }

    pub(in crate::store) fn at(path: &Path) -> StoreResult<KeyFilePlace> {
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

    pub(in crate::store) fn locator(&self, roots: &Path) -> CustodyLocator {
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

    pub(in crate::store) fn read(&self, label: KeyLabel) -> StoreResult<KeyFile> {
        let json = self.bytes()?;
        serde_json::from_slice::<KeyFileVersion>(&json)
            .map_err(|_| StoreError::KeyFileUnparseable)
            .and_then(|head| known_version(head.version, i64::from(KEY_FILE_VERSION)))?;
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

    fn what_it_names(&self) -> KeyFileNames {
        let Some(root_id) = self.named_root() else {
            return KeyFileNames::Nothing;
        };
        let fields = self
            .bytes()
            .ok()
            .and_then(|json| serde_json::from_slice::<serde_json::Value>(&json).ok());
        let version = fields.as_ref().and_then(|f| f["version"].as_i64());
        let kind = fields
            .as_ref()
            .and_then(|f| serde_json::from_value::<CustodyKind>(f["custody_kind"].clone()).ok());
        match (version, kind) {
            (Some(version), Some(CustodyKind::DevFallback | CustodyKind::Dpapi))
                if version == i64::from(KEY_FILE_VERSION) =>
            {
                KeyFileNames::Destroys(root_id)
            }
            _ => KeyFileNames::NotHandled(root_id),
        }
    }

    pub(in crate::store) fn named_root(&self) -> Option<RootId> {
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

    #[cfg(test)]
    pub(in crate::store) fn publish(&self, key_file: &KeyFile) -> StoreResult<()> {
        self.publish_keeping(key_file).0
    }

    pub(in crate::store) fn publish_from(
        &self,
        temp: &OsStr,
        key_file: &KeyFile,
    ) -> StoreResult<()> {
        let place = KeyFilePlace {
            dir: self.dir.try_clone().map_err(io_error)?,
            name: self.name.clone(),
            temp: temp.to_os_string(),
        };
        place.publish_keeping(key_file).0
    }

    fn publish_keeping(&self, key_file: &KeyFile) -> (StoreResult<()>, Published) {
        let mut published = Published {
            linked: false,
            temp: None,
        };
        let json = match serde_json::to_vec_pretty(key_file) {
            Ok(json) => json,
            Err(_) => return (Err(StoreError::Io(ErrorKind::InvalidData)), published),
        };
        if let Err(e) = self.clear_a_temp_of_another_party() {
            return (Err(io_error(e)), published);
        }
        let linked = self.write_and_link(&json, &mut published);
        let discarded = match linked {
            Ok(()) => match self.dir.remove_file(&self.temp) {
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                other => other,
            },
            Err(_) => self.remove_own_temp(&published),
        };
        let result = linked
            .and_then(|()| discarded.map_err(io_error))
            .and_then(|()| self.dir.sync().map_err(io_error));
        (result, published)
    }

    fn clear_a_temp_of_another_party(&self) -> std::io::Result<()> {
        let more_than_one_name = match self.dir.entry(&self.temp) {
            Ok(entry) if entry.kind == EntryKind::File => self
                .dir
                .open_file(&self.temp)
                .and_then(|file| durable::file_link_count(&file))
                .map_or(true, |links| links > 1),
            _ => false,
        };
        if !more_than_one_name {
            return zero_and_remove(&self.dir, &self.temp);
        }
        match self.dir.remove_entry(&self.temp) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn write_and_link(&self, json: &[u8], published: &mut Published) -> StoreResult<()> {
        let mut file = self.dir.create_new(&self.temp).map_err(io_error)?;
        file.write_all(json).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        published.temp = Some(file);
        failpoint::hit("key_file_before_publish")?;
        self.dir
            .link(&self.temp, &self.name)
            .map_err(|e| match e.kind() {
                ErrorKind::AlreadyExists => StoreError::AlreadyExists,
                _ => io_error(e),
            })?;
        published.linked = true;
        Ok(())
    }

    fn remove_own_temp(&self, published: &Published) -> std::io::Result<()> {
        let Some(file) = &published.temp else {
            return Ok(());
        };
        if !self.dir.names_file(&self.temp, file)? {
            return Ok(());
        }
        zero_through(file)?;
        match self.dir.remove_file(&self.temp) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn abandon(&self, published: &Published) -> std::io::Result<bool> {
        let Some(file) = &published.temp else {
            return Ok(false);
        };
        let zeroed = zero_through(file);
        let own_name = published.linked && self.dir.names_file(&self.name, file)?;
        if own_name {
            match self.dir.remove_file(&self.name) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        if self.dir.names_file(&self.temp, file)? {
            match self.dir.remove_file(&self.temp) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        self.dir.sync()?;
        zeroed.map(|()| own_name)
    }

    /// Removes the key file for a delete: first an entry at the temporary
    /// name as a create clears it (with more than one name, or a count that
    /// cannot be read, removed without a zero-fill), then `store.key`, whose
    /// zero-fill reaches every name of the store's own key file. An entry at
    /// the temporary name that is no regular file and that stays is counted
    /// as not followed (ruling ST-45 H5).
    pub(in crate::store) fn destroy(&self) -> Removal {
        let temp_kind = self.dir.entry(&self.temp).ok().map(|entry| entry.kind);
        let temp_stays = self.clear_a_temp_of_another_party().is_err();
        let temp_not_followed =
            u64::from(temp_stays && temp_kind.is_some_and(|kind| kind != EntryKind::File));
        if failpoint::hit("key_file_destroy").is_err() {
            return Removal {
                gone: false,
                not_followed: temp_not_followed,
            };
        }
        let not_followed = temp_not_followed
            + match self.dir.entry(&self.name) {
                Ok(entry) if entry.kind != EntryKind::File => 1,
                _ => 0,
            };
        let _ = zero_and_remove(&self.dir, &self.name);
        let _ = self.dir.sync();
        let absent = |name: &OsStr| matches!(self.dir.entry(name), Err(e) if e.kind() == ErrorKind::NotFound);
        let gone = absent(&self.name) && absent(&self.temp);
        Removal { gone, not_followed }
    }
}

fn io_error(e: std::io::Error) -> StoreError {
    StoreError::Io(e.kind())
}

/// What the publish of a key file did, kept for a create that is abandoned:
/// whether its own link answered Ok, and the handle of the temporary file
/// that this create made.
pub(in crate::store) struct Published {
    linked: bool,
    temp: Option<std::fs::File>,
}

fn zero_through(file: &std::fs::File) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom};
    let len = file.metadata()?.len() as usize;
    let mut writer = file;
    writer.seek(SeekFrom::Start(0))?;
    writer.write_all(&vec![0u8; len])?;
    file.sync_all()
}

/// Zero-fills the regular file at `name` and removes the name. Before the
/// zero-fill it sets the file's mode to 0600 by its name through the
/// directory's handle, following no link (on Windows it clears the read-only
/// attribute), so that the zero-fill reaches every name of the file; if the
/// zero-fill still fails the name is removed all the same (ruling ST-46 K5
/// N1).
fn zero_and_remove(dir: &Dir, name: &OsStr) -> std::io::Result<()> {
    if let Ok(entry) = dir.entry(name) {
        if entry.kind == EntryKind::File {
            make_writable_by_name(dir, name);
            let _ = zero_fill(dir, name);
        }
    }
    match dir.remove_entry(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(unix)]
fn make_writable_by_name(dir: &Dir, name: &OsStr) {
    let _ = dir.set_file_mode(name, 0o600);
}

#[cfg(windows)]
fn make_writable_by_name(dir: &Dir, name: &OsStr) {
    let path = dir.path().join(name);
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        let mut permissions = metadata.permissions();
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "a key file that the store destroys has to be writable for its zero-fill"
        )]
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(&path, permissions);
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
pub(in crate::store) enum NameForm {
    Volume,
    Kept,
}

type Named = (NameForm, VolumeIds);

#[cfg(unix)]
fn open_database(store_dir: &Dir, flags: OpenFlags) -> StoreResult<(Connection, Named)> {
    #[cfg(test)]
    failpoint::seam(SEAM_BEFORE_THE_MODE_STEP);
    set_mode_then_close(store_dir)?;
    open_connection(store_dir, flags)
}

#[cfg(unix)]
fn set_mode_then_close(store_dir: &Dir) -> StoreResult<()> {
    use std::os::unix::fs::PermissionsExt;
    match store_dir.open_file(OsStr::new(DATABASE_FILE)) {
        Ok(file) => file
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(io_error)?,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(StoreError::DatabaseMissing),
        Err(e) => return Err(refused_at(StoreName::Database, e)),
    }
    Ok(())
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
pub(in crate::store) enum VolumeIds {
    Yes,
    No,
    Unknown,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::store) struct VolumeReadings {
    pub(crate) flag: Option<bool>,
    pub(crate) capability: Option<(bool, bool)>,
    pub(crate) root: Option<RootAnswer>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::store) enum RootAnswer {
    TheRoot,
    NoSuchName,
    Other,
}

#[cfg(target_os = "macos")]
impl VolumeReadings {
    pub(in crate::store) fn ids(self) -> VolumeIds {
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
pub(in crate::store) enum DirectoryStat {
    TheHandle,
    AnotherIdentity,
    NoSuchName,
    Failed,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::store) enum NameChoice {
    Volume,
    Kept,
    StatAgain,
    Refused,
    Failed,
}

#[cfg(target_os = "macos")]
pub(in crate::store) fn choose_the_name(
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
pub(in crate::store) fn volume_readings(store_dir: &Dir) -> VolumeReadings {
    #[cfg(test)]
    if let Some(readings) = name_seams::volume_answer() {
        return readings;
    }
    let Ok(volume) = store_dir.volume() else {
        return VolumeReadings {
            flag: None,
            capability: None,
            root: None,
        };
    };
    let flag = Some(volume.flags & libc::MNT_DOVOLFS as u32 != 0);
    let root_dir = Dir::open(&volume.mount_point).ok();
    let capability_at_the_volume_root = |root: &Dir| {
        let fd = root.raw_descriptor();
        match durable::volume_of(fd) {
            Ok(root_volume) if root_volume.fsid == volume.fsid => path_from_id(fd),
            _ => None,
        }
    };
    let capability = root_dir.as_ref().and_then(capability_at_the_volume_root);
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
    let mut stats_left = STATS_AFTER_THE_FIRST;
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
pub(in crate::store) mod name_seams {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use crate::store::durable::EntryKind;

    type Answer = std::io::Result<(EntryKind, u64, u64)>;

    thread_local! {
        static VOLUME: RefCell<Option<super::VolumeReadings>> = const { RefCell::new(None) };
        static GIVEN: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
        static STAT: RefCell<Option<Box<dyn Fn() -> Answer>>> = const { RefCell::new(None) };
    }

    pub(in crate::store) fn answer_volume(readings: Option<super::VolumeReadings>) {
        VOLUME.with(|volume| *volume.borrow_mut() = readings);
    }

    pub(in crate::store) fn volume_answer() -> Option<super::VolumeReadings> {
        VOLUME.with(|volume| *volume.borrow())
    }

    pub(in crate::store) fn give_name(name: Option<PathBuf>) {
        GIVEN.with(|given| *given.borrow_mut() = name);
    }

    pub(in crate::store) fn given_name() -> Option<PathBuf> {
        GIVEN.with(|given| given.borrow().clone())
    }

    pub(in crate::store) fn answer_stat(answer: Option<Box<dyn Fn() -> Answer>>) {
        STAT.with(|stat| *stat.borrow_mut() = answer);
    }

    pub(in crate::store) fn stat_answer() -> Option<Answer> {
        STAT.with(|stat| stat.borrow().as_ref().map(|answer| answer()))
    }
}

#[cfg(test)]
pub(in crate::store) mod code_seams {
    use std::cell::Cell;

    use halvo_sqlcipher::rusqlite;

    thread_local! {
        static FORCED: Cell<Option<(usize, i32)>> = const { Cell::new(None) };
    }

    pub(in crate::store) fn force(nth: usize, code: Option<i32>) {
        FORCED.with(|forced| forced.set(code.map(|code| (nth, code))));
    }

    pub(in crate::store) const PROBE_TIMEOUT: &str = "the live-holder probe's busy timeout";
    pub(in crate::store) const PROBE_LOCKING_MODE: &str = "the live-holder probe's locking mode";
    pub(in crate::store) const HELD_LOOK_HAS_MOVED: &str =
        "the held look's question whether the file moved";
    pub(in crate::store) const CHECKPOINT_QUERY: &str =
        "the truncating checkpoint's query of checkpoint_truncate";
    pub(in crate::store) const LOG_ENTRY_READ: &str = "the read of the log's entry in log_look";
    pub(in crate::store) const SHRED_COMMIT: &str = "the commit of an expiry chunk";

    thread_local! {
        static FORCED_IO_AT: Cell<Option<(&'static str, std::io::ErrorKind)>> =
            const { Cell::new(None) };
    }

    /// Makes the next read at `place` of this thread fail with `kind`, once;
    /// `None` takes the forced failure back.
    pub(in crate::store) fn force_io_at(place: &'static str, kind: Option<std::io::ErrorKind>) {
        FORCED_IO_AT.with(|forced| forced.set(kind.map(|kind| (place, kind))));
    }

    pub(in crate::store) fn or_forced_io_at<T>(
        place: &'static str,
        result: std::io::Result<T>,
    ) -> std::io::Result<T> {
        FORCED_IO_AT.with(|forced| match forced.get() {
            Some((at, kind)) if at == place => {
                forced.set(None);
                Err(std::io::Error::from(kind))
            }
            _ => result,
        })
    }

    thread_local! {
        static FORCED_AT: Cell<Option<(&'static str, i32)>> = const { Cell::new(None) };
    }

    pub(in crate::store) fn force_at(place: &'static str, code: Option<i32>) {
        FORCED_AT.with(|forced| forced.set(code.map(|code| (place, code))));
    }

    pub(in crate::store) fn or_forced_at<T>(
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

    pub(in crate::store) fn forced() -> Option<i32> {
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

#[cfg(test)]
pub(in crate::store) mod read_back_seam {
    use std::cell::RefCell;

    use halvo_sqlcipher::rusqlite::Connection;

    type Act = Box<dyn Fn(&Connection)>;

    thread_local! {
        static ACT: RefCell<Option<Act>> = const { RefCell::new(None) };
    }

    /// Runs `act` on the connection of each open and create of this thread,
    /// after the settings are set and before they are read back, until the
    /// guard is dropped.
    pub(in crate::store) fn before_the_read_backs(act: impl Fn(&Connection) + 'static) -> Guard {
        ACT.with(|slot| *slot.borrow_mut() = Some(Box::new(act)));
        Guard
    }

    pub(in crate::store) struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            ACT.with(|slot| *slot.borrow_mut() = None);
        }
    }

    pub(super) fn run(conn: &Connection) {
        ACT.with(|slot| {
            if let Some(act) = slot.borrow().as_ref() {
                act(conn);
            }
        });
    }
}

#[cfg(test)]
pub(in crate::store) mod pinned_cipher_seam {
    use std::cell::RefCell;

    /// What a test hands the open in place of the library's answer. A field
    /// left `None` is read from the library.
    #[derive(Clone, Copy, Default)]
    pub(in crate::store) struct Answers {
        pub(in crate::store) provider: Option<&'static str>,
        pub(in crate::store) version: Option<&'static str>,
        pub(in crate::store) os_entropy_hook: Option<bool>,
    }

    thread_local! {
        static ANSWERS: RefCell<Option<Answers>> = const { RefCell::new(None) };
    }

    /// Hands each open and create of this thread `answers` where it reads the
    /// cipher provider, the cipher version and the entropy hook of the
    /// library, until the guard is dropped.
    pub(in crate::store) fn instead_of_the_library(answers: Answers) -> Guard {
        ANSWERS.with(|slot| *slot.borrow_mut() = Some(answers));
        Guard
    }

    pub(in crate::store) struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            ANSWERS.with(|slot| *slot.borrow_mut() = None);
        }
    }

    pub(super) fn text(pragma: &str) -> Option<String> {
        let answers = ANSWERS.with(|slot| *slot.borrow())?;
        match pragma {
            "PRAGMA cipher_provider" => answers.provider,
            "PRAGMA cipher_version" => answers.version,
            _ => None,
        }
        .map(str::to_string)
    }

    pub(super) fn os_entropy_hook() -> Option<bool> {
        ANSWERS.with(|slot| *slot.borrow())?.os_entropy_hook
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

pub(in crate::store) fn code_answer(code: Option<i32>) -> StoreError {
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
#[must_use = "the hold keeps store.db at its name only while it lives"]
struct DatabaseHold {
    _file: Option<std::fs::File>,
}

#[cfg(windows)]
fn hold_the_database_file(store_dir: &Dir) -> StoreResult<DatabaseHold> {
    match store_dir.hold_file(OsStr::new(DATABASE_FILE)) {
        Ok(held) => Ok(DatabaseHold { _file: Some(held) }),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(DatabaseHold { _file: None }),
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

pub(in crate::store) use super::failpoint;

pub(in crate::store) fn db_error(e: rusqlite::Error) -> StoreError {
    match e {
        rusqlite::Error::SqliteFailure(failure, _) => StoreError::Db(failure.extended_code),
        _ => StoreError::Corrupt("the database returned an unexpected value".to_string()),
    }
}

#[cfg(test)]
pub(in crate::store) fn unix_secs_now() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(in crate::store) fn random_segment_id() -> StoreResult<uuid::Uuid> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| StoreError::Seal)?;
    Ok(uuid::Builder::from_random_bytes(bytes).into_uuid())
}

fn remove_in(store_dir: &Dir, name: &str) -> StoreResult<()> {
    match store_dir.remove_file(OsStr::new(name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

/// Whether the root that a pending mark names was made: `Owed` is written
/// before the root step, `Made` after the custody's create_root answered Ok
/// and the wrap read the root file through the held roots directory, before
/// the key file is published. A mark without the reading, one that does not
/// parse, and one that reads made without the identity of its root file read
/// `Owed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PendingReading {
    #[default]
    Owed,
    Made,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    custody_kind: CustodyKind,
    root_id: RootId,
    #[serde(default)]
    reading: PendingReading,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root_file: Option<FileIdentity>,
}

impl Pending {
    /// The root that this mark names: one that reads made with the identity
    /// of its root file, which a cleanup destroys alone; every other mark
    /// names none.
    fn made_root(&self) -> Option<NamedRoot> {
        match (self.reading, self.root_file) {
            (PendingReading::Made, Some(file)) => Some(NamedRoot {
                id: self.root_id,
                by: NamedBy::AMark,
                file: Some(file),
            }),
            _ => None,
        }
    }
}

fn write_pending(
    store_dir: &Dir,
    custody: &dyn KeyCustody,
    reading: PendingReading,
    root_file: Option<FileIdentity>,
) -> StoreResult<()> {
    let json = serde_json::to_vec(&Pending {
        custody_kind: custody.kind(),
        root_id: custody.root_id(),
        reading,
        root_file,
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

/// Where a cleanup of a stopped create reaches the roots: the directory that
/// the store holds, with the identity of the store's own root file, which no
/// cleanup destroys, or, before a create took its handle, the path.
#[derive(Clone, Copy)]
enum CleanupRoots<'a> {
    Path(&'a Path),
    Held(&'a RootsHandle, FileIdentity),
}

/// The answer of a create whose look met an entry at the root name of its
/// id, read through the custody and never written, so that the refusal
/// publishes no root (the review's F1): `RootExists` for a root that the
/// custody reads, and for a name at which nothing stands any longer; the
/// custody's own error for a root that it cannot read. The read is an unwrap
/// of an empty wrap, which no root authenticates.
fn a_root_that_stands(custody: &dyn KeyCustody) -> CustodyError {
    match custody.unwrap_store_key(&WrappedKey(Vec::new()), KeyLabel::STORE) {
        Ok(_) | Err(CustodyError::WrapAuthFailed | CustodyError::RootMissing) => {
            CustodyError::RootExists
        }
        Err(e) => e,
    }
}

fn clear_pending(store_dir: &Dir, roots: CleanupRoots<'_>, keyed: KeyNames) -> StoreResult<()> {
    let present = store_dir.entry(OsStr::new(CREATE_PENDING)).is_ok()
        || store_dir.entry(OsStr::new(CREATE_PENDING_TEMP)).is_ok();
    if !present || keyed == KeyNames::Unknown {
        return Ok(());
    }
    if let Some(made) = read_pending(store_dir).and_then(|p| p.made_root()) {
        if keyed != KeyNames::Root(made.id) {
            match roots {
                CleanupRoots::Path(roots) => file_root::destroy_named_root_at(roots, made)?,
                CleanupRoots::Held(_, own) if made.file == Some(own) => {}
                CleanupRoots::Held(held, _) => file_root::destroy_root_through(&held.dir, made)?,
            }
        }
    }
    remove_pending(store_dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::store) struct Checkpoint {
    pub busy: i64,
    pub wal_bytes: u64,
}

pub(in crate::store) struct Db {
    conn: Connection,
    store_dir: Dir,
    log_name: std::ffi::OsString,
    named: Named,
    log_written: std::cell::Cell<bool>,
    look_failed: std::cell::Cell<Option<LookFailure>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LookFailure {
    Obstructed(Met),
    Unread(ErrorKind),
    Checkpoint(CheckpointFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckpointFailure {
    Db(i32),
    Io(ErrorKind),
    Incomplete { busy: i64, wal_bytes: u64 },
    Other,
}

impl LookFailure {
    fn of(e: &StoreError) -> Self {
        match e {
            StoreError::Obstructed { met, .. } => LookFailure::Obstructed(*met),
            StoreError::Io(kind) => LookFailure::Unread(*kind),
            _ => LookFailure::Unread(ErrorKind::Other),
        }
    }

    fn of_a_checkpoint(e: &StoreError) -> Self {
        LookFailure::Checkpoint(match e {
            StoreError::Db(code) => CheckpointFailure::Db(*code),
            StoreError::Io(kind) => CheckpointFailure::Io(*kind),
            StoreError::CheckpointIncomplete { busy, wal_bytes } => CheckpointFailure::Incomplete {
                busy: *busy,
                wal_bytes: *wal_bytes,
            },
            _ => CheckpointFailure::Other,
        })
    }

    fn answer(self) -> StoreError {
        match self {
            LookFailure::Obstructed(met) => obstructed(StoreName::Log, met),
            LookFailure::Unread(kind) => StoreError::Io(kind),
            LookFailure::Checkpoint(CheckpointFailure::Db(code)) => StoreError::Db(code),
            LookFailure::Checkpoint(CheckpointFailure::Io(kind)) => StoreError::Io(kind),
            LookFailure::Checkpoint(CheckpointFailure::Incomplete { busy, wal_bytes }) => {
                StoreError::CheckpointIncomplete { busy, wal_bytes }
            }
            LookFailure::Checkpoint(CheckpointFailure::Other) => {
                StoreError::Corrupt("the checkpoint after a shred failed".to_string())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Database {
    New,
    Existing,
}

const SCHEMA_TABLES: [&str; 3] = ["segment_keys", "segment_totals", "daily_totals"];

const SCHEMA: &str = "CREATE TABLE segment_keys (\n\
       id INTEGER PRIMARY KEY AUTOINCREMENT,\n\
       segment_id TEXT NOT NULL UNIQUE,\n\
       wrapped_key BLOB,\n\
       day INTEGER NOT NULL,\n\
       first_capture INTEGER NOT NULL,\n\
       last_capture INTEGER NOT NULL,\n\
       logical_bytes INTEGER NOT NULL,\n\
       disk_bytes INTEGER NOT NULL\n\
     );\n\
     CREATE INDEX segment_keys_live_by_first_capture ON segment_keys (first_capture)\n\
       WHERE wrapped_key IS NOT NULL;\n\
     CREATE TABLE segment_totals (\n\
       segment INTEGER NOT NULL REFERENCES segment_keys (id),\n\
       kind TEXT NOT NULL,\n\
       events INTEGER NOT NULL,\n\
       bytes INTEGER NOT NULL,\n\
       PRIMARY KEY (segment, kind)\n\
     ) WITHOUT ROWID;\n\
     CREATE TABLE daily_totals (\n\
       day INTEGER NOT NULL,\n\
       kind TEXT NOT NULL,\n\
       events INTEGER NOT NULL,\n\
       bytes INTEGER NOT NULL,\n\
       PRIMARY KEY (day, kind)\n\
     ) WITHOUT ROWID;";

/// The UTC day and the first and last capture instants of one segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::store) struct Instants {
    pub(in crate::store) day: UtcDay,
    pub(in crate::store) first: i64,
    pub(in crate::store) last: i64,
}

impl Instants {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "only the old write calls it, which has no caller outside the store's tests until core-3 deletes it"
        )
    )]
    fn at(nanos: i64) -> Instants {
        Instants {
            day: UtcDay::of_instant(nanos),
            first: nanos,
            last: nanos,
        }
    }
}

fn as_column(value: u64) -> StoreResult<i64> {
    i64::try_from(value)
        .map_err(|_| StoreError::Corrupt("a count above the column's range".to_string()))
}

/// Why `write` must be refused, before anything is written.
pub(in crate::store) fn refusal_of(write: &SegmentWrite<'_>) -> Option<Refusal> {
    let is_v4 = write.segment_id.get_version_num() == 4
        && write.segment_id.get_variant() == uuid::Variant::RFC4122;
    let kind_text = |kind: &str| {
        (1..=KIND_MAX_BYTES).contains(&kind.len())
            && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    };
    let mut kinds: Vec<&str> = write.kinds.iter().map(|total| total.kind).collect();
    kinds.sort_unstable();
    let events = write
        .kinds
        .iter()
        .try_fold(0u64, |sum, total| sum.checked_add(total.events));
    if !is_v4 {
        Some(Refusal::NotRandomId)
    } else if write.payload.len() > PAYLOAD_MAX_BYTES {
        Some(Refusal::PayloadOverCap)
    } else if write.kinds.len() > KINDS_MAX {
        Some(Refusal::TooManyKinds)
    } else if !write.kinds.iter().all(|total| kind_text(total.kind)) {
        Some(Refusal::KindNotText)
    } else if kinds.windows(2).any(|pair| pair[0] == pair[1]) {
        Some(Refusal::RepeatedKind)
    } else if events.is_none_or(|events| events == 0 || events > i64::MAX as u64)
        || write
            .kinds
            .iter()
            .any(|total| total.bytes > i64::MAX as u64)
    {
        Some(Refusal::NoEvent)
    } else if write.first_capture > write.last_capture {
        Some(Refusal::InstantsOutOfOrder)
    } else if UtcDay::of_instant(write.first_capture) != UtcDay::of_instant(write.last_capture) {
        Some(Refusal::InstantsOnTwoDays)
    } else if UtcDay::of_instant(write.first_capture) != write.day {
        Some(Refusal::DayNotTheInstants)
    } else {
        None
    }
}

const DATABASE_KEY_LABEL: [u8; 24] = *b"halvo-store-database-key";
const WRAPPING_KEY_LABEL: [u8; 24] = *b"halvo-store-segment-wrap";

/// A key derived from the store key under `label`: the XChaCha20-Poly1305
/// ciphertext of 32 zero bytes under the store key with the label as nonce,
/// without its tag, which is the XChaCha20 stream from its second block (the
/// first block keys Poly1305), so that keys of two labels are independent
/// and the store key itself encrypts nothing.
fn derived_key(store_key: &StoreKey, label: &[u8; 24]) -> StoreResult<Zeroizing<[u8; 32]>> {
    use aead::{Aead, KeyInit};
    let cipher = chacha20poly1305::XChaCha20Poly1305::new(store_key.expose().into());
    let sealed = Zeroizing::new(
        cipher
            .encrypt(
                &chacha20poly1305::XNonce::from(*label),
                [0u8; 32].as_slice(),
            )
            .map_err(|_| StoreError::Seal)?,
    );
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&sealed[..32]);
    Ok(key)
}

pub(in crate::store) fn database_key(store_key: &StoreKey) -> StoreResult<Zeroizing<[u8; 32]>> {
    derived_key(store_key, &DATABASE_KEY_LABEL)
}

pub(in crate::store) fn wrapping_key(store_key: &StoreKey) -> StoreResult<Zeroizing<[u8; 32]>> {
    derived_key(store_key, &WRAPPING_KEY_LABEL)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::store) struct DatabaseRemoval {
    pub(crate) left: bool,
    pub(crate) not_followed: u64,
}

impl Db {
    pub(in crate::store) fn destroy_through(store_dir: &Dir) -> DatabaseRemoval {
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

    /// `OlderStore` or `NewerStore` when no log with frames stands beside
    /// store.db and the database key, or else the store key itself, opens
    /// store.db: a read-write connection under exclusive locking, which does
    /// not checkpoint at its close when a log stood. A version is believed
    /// only from a store.db that is a whole number of its pages long; a
    /// version above this build's answers `NewerStore`, and one below answers
    /// `OlderStore` when store.db holds the table `segment_keys`. It reads
    /// user_version, the page size and sqlite_master, and answers `None` for
    /// every other reading.
    fn another_version_of(store_dir: &Dir, store_key: &StoreKey) -> Option<StoreError> {
        let log_was_there = match store_dir.entry(OsStr::new(DATABASE_LOG)) {
            Ok(entry) if entry.len > 0 => return None,
            Ok(_) => true,
            Err(e) if e.kind() == ErrorKind::NotFound => false,
            Err(_) => return None,
        };
        let length = store_dir.entry(OsStr::new(DATABASE_FILE)).ok()?.len;
        let database = database_key(store_key).ok()?;
        for key in [&*database, store_key.expose()] {
            let (probe, _) = open_connection(store_dir, DATABASE_OPEN).ok()?;
            probe
                .set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, log_was_there)
                .ok()?;
            probe.execute_batch(&key_pragma(key)).ok()?;
            probe
                .execute_batch("PRAGMA locking_mode = EXCLUSIVE;")
                .ok()?;
            if pragma_int(&probe, "user_version").is_err() {
                continue;
            }
            return match another_version(&probe, length) {
                Err(e @ (StoreError::OlderStore { .. } | StoreError::NewerStore { .. })) => Some(e),
                _ => None,
            };
        }
        None
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
        let (log_was_there, log_holds_frames) = match store_dir.entry(OsStr::new(DATABASE_LOG)) {
            Ok(entry) => (true, entry.len > 0),
            Err(e) if e.kind() == ErrorKind::NotFound => (false, false),
            Err(e) => return Err(io_error(e)),
        };
        let whole_length = match database {
            Database::Existing if !log_holds_frames => store_dir
                .entry(OsStr::new(DATABASE_FILE))
                .map(|entry| entry.len)
                .ok(),
            _ => None,
        };
        if database == Database::New {
            store_dir
                .create_new(OsStr::new(DATABASE_FILE))
                .map_err(io_error)?;
        }
        let (conn, (name_form, volume_ids)) = open_database(store_dir, DATABASE_OPEN)?;
        let set_up = Self::set_up(&conn, key, database, log_was_there, whole_length);
        #[cfg(test)]
        failpoint::seam(SEAM_AFTER_THE_FIRST_READ);
        held_look(&conn, store_dir, name_form)?;
        set_up.map_err(|e| Self::answered(store_dir, e))?;
        let db = Self {
            conn,
            store_dir: store_dir.try_clone().map_err(io_error)?,
            log_name: DATABASE_LOG.into(),
            named: (name_form, volume_ids),
            log_written: std::cell::Cell::new(database == Database::New),
            look_failed: std::cell::Cell::new(None),
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
        whole_length: Option<u64>,
    ) -> StoreResult<()> {
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, log_was_there)
            .map_err(db_error)?;
        conn.busy_timeout(Duration::ZERO).map_err(db_error)?;
        conn.execute_batch("PRAGMA cipher_log_level = NONE;")
            .map_err(db_error)?;
        conn.execute_batch(&key_pragma(key)).map_err(db_error)?;
        conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE;")
            .map_err(db_error)?;
        #[cfg(test)]
        failpoint::seam(SEAM_BEFORE_THE_FIRST_READ);
        verify_key(conn)?;
        if database == Database::Existing {
            match whole_length {
                Some(length) => another_version(conn, length)?,
                None => known_version(pragma_int(conn, "user_version")?, DATABASE_VERSION)?,
            }
        }
        conn.execute_batch(
            "PRAGMA secure_delete = ON;\n\
             PRAGMA temp_store = MEMORY;\n\
             PRAGMA foreign_keys = ON;\n\
             PRAGMA synchronous = FULL;\n\
             PRAGMA fullfsync = ON;\n\
             PRAGMA checkpoint_fullfsync = ON;",
        )
        .map_err(db_error)?;
        #[cfg(test)]
        read_back_seam::run(conn);
        require_write_ahead_log(conn)?;
        require_secure_delete(conn)?;
        require_exclusive_locking(conn)?;
        require_synchronous_full(conn)?;
        require_fullfsync(conn)?;
        require_checkpoint_fullfsync(conn)?;
        require_foreign_keys(conn)?;
        require_cipher_log_level_none(conn)?;
        require_pinned_cipher(conn)
    }

    /// The last steps of an open or a create. For an existing database the
    /// look at the tables, the two settings and a held look come first, read
    /// through the log's frames with no write, so that a refusal leaves the
    /// log and `store.db` as they were; then the drain; then, for a new
    /// database, the tables, the version and their read-backs, the log's
    /// look, a checkpoint and the page look; then the last held look, which a
    /// `store.db` moved or exchanged at its name during the drain fails.
    fn finish(&self, database: Database) -> StoreResult<()> {
        #[cfg(test)]
        failpoint::seam(SEAM_BEFORE_THE_DRAIN);
        if database == Database::Existing {
            self.require_schema()?;
            require_setting(&self.conn, "auto_vacuum", 0)?;
            require_setting(&self.conn, "page_size", PAGE_SIZE)?;
            held_look(&self.conn, &self.store_dir, self.named.0)?;
        }
        self.log_drained()?;
        if database == Database::New {
            self.init_schema()?;
            self.conn
                .execute_batch(&format!("PRAGMA user_version = {DATABASE_VERSION};"))
                .map_err(db_error)?;
            require_setting(&self.conn, "auto_vacuum", 0)?;
            require_setting(&self.conn, "page_size", PAGE_SIZE)?;
            self.log_look()?;
            self.checkpoint_truncate()?;
            #[cfg(test)]
            failpoint::seam(SEAM_BEFORE_THE_PAGE_LOOK);
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
        let entry = self.store_dir.entry(&self.log_name);
        #[cfg(test)]
        let entry = code_seams::or_forced_io_at(code_seams::LOG_ENTRY_READ, entry);
        match entry {
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

    /// The second instant of the log's check, at the first commit of a
    /// handle whose open wrote nothing and whose statements changed a row:
    /// the log in the store directory holds the commit's frames, or the
    /// answer is the obstructed variant, or `Io` when the look cannot read
    /// the log's entry (the `Unread` arm), except against a program of the same
    /// user that changes the entries of the store's directory or writes its
    /// files (a rename of another file into the directory is such a change).
    /// When such a program puts another file of more than 0 bytes at the
    /// log's name while SQLite writes its log elsewhere, the look passes: a
    /// batch answered `Ok` is then lost when the process ends without a clean
    /// close (shown at a kill, reasoned for a crash or a loss of power), and an
    /// ordinal can be issued twice. A log unlinked or moved from its name, with
    /// nothing or an empty file left there, fails the look. `changes_before` is the connection's
    /// count of changed rows before that commit's transaction began, so that
    /// rows of a transaction that rolled back do not count. The look comes
    /// after the commit, so an obstructed look leaves a commit whose frames
    /// are in no file at the store's names, and a look that could not read the
    /// log's entry leaves a commit whose frames may lie in the log at its
    /// name, which the next open's drain brings into `store.db`. From then on
    /// the handle answers the look's own answer to every call that reads or
    /// writes the database ([`Db::look_failure`]), and its close does not
    /// checkpoint, so nothing of that log reaches `store.db` through this
    /// handle, which stays as it was before the commit.
    pub(in crate::store) fn look_after_the_first_commit(
        &self,
        changes_before: u64,
    ) -> StoreResult<()> {
        if self.log_written.get() || self.conn.total_changes() == changes_before {
            return Ok(());
        }
        if let Err(e) = self.log_look() {
            self.look_failed.set(Some(LookFailure::of(&e)));
            self.conn
                .set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
                .map_err(db_error)?;
            return Err(e);
        }
        self.log_written.set(true);
        Ok(())
    }

    /// The answer of a failed look after this handle's first commit, or of a
    /// checkpoint that failed after a shred's commit, which the handle gives
    /// to every later call that reads or writes the database; `None` while
    /// neither failed.
    pub(in crate::store) fn look_failure(&self) -> Option<StoreError> {
        self.look_failed.get().map(LookFailure::answer)
    }

    /// The answer of a shred's checkpoint that failed: when the shred's
    /// commit changed a row (the connection's count of changed rows is not
    /// `changes_before`), the shred is committed in the log and not in
    /// `store.db`, so the handle keeps the failure as it keeps a failed look,
    /// its close does not checkpoint, and nothing of it reports the shred;
    /// a later expiry or eviction call on the handle finishes the checkpoint
    /// and lifts the failure, or the next open's drain brings the log into
    /// `store.db` before it answers.
    pub(in crate::store) fn kept_after_a_shred(
        &self,
        e: StoreError,
        changes_before: u64,
    ) -> StoreError {
        if self.conn.total_changes() == changes_before || self.look_failed.get().is_some() {
            return e;
        }
        self.look_failed.set(Some(LookFailure::of_a_checkpoint(&e)));
        match self
            .conn
            .set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
        {
            Ok(_) => e,
            Err(config) => db_error(config),
        }
    }

    pub(in crate::store) fn owes_a_checkpoint(&self) -> bool {
        matches!(self.look_failed.get(), Some(LookFailure::Checkpoint(_)))
    }

    pub(in crate::store) fn finish_owed_checkpoint(&self) -> StoreResult<()> {
        if !self.owes_a_checkpoint() {
            return Ok(());
        }
        self.checkpoint_truncate()?;
        self.look_failed.set(None);
        Ok(())
    }

    /// Whether this handle's look after its first commit failed because it
    /// could not read the log's entry, so that the log may stand at its
    /// name with the commit's frames.
    pub(in crate::store) fn look_unread(&self) -> bool {
        matches!(self.look_failed.get(), Some(LookFailure::Unread(_)))
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
    pub(in crate::store) fn name_form(&self) -> NameForm {
        self.named.0
    }

    #[cfg(all(test, target_os = "macos"))]
    pub(in crate::store) fn volume_ids(&self) -> VolumeIds {
        self.named.1
    }

    fn init_schema(&self) -> StoreResult<()> {
        self.conn.execute_batch(SCHEMA).map_err(db_error)
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

    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The ordinal of the key row of `segment_id`, live or shredded.
    pub(in crate::store) fn ordinal_of(
        &self,
        segment_id: uuid::Uuid,
    ) -> StoreResult<Option<Ordinal>> {
        self.conn
            .query_row(
                "SELECT id FROM segment_keys WHERE segment_id = ?1",
                [segment_id.simple().to_string()],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map(|id| id.map(Ordinal::of_row))
            .map_err(db_error)
    }

    /// One transaction: the complete key row, the totals of each kind and the
    /// rollup of the day. Nothing of it stays when it fails.
    fn insert_segment(
        &self,
        segment_id: uuid::Uuid,
        wrapped: &[u8],
        instants: &Instants,
        sizes: (u64, u64),
        kinds: &[KindTotal<'_>],
    ) -> StoreResult<Ordinal> {
        let changes_before = self.conn.total_changes();
        let tx = self.conn.unchecked_transaction().map_err(db_error)?;
        tx.execute(
            "INSERT INTO segment_keys (segment_id, wrapped_key, day, first_capture, last_capture, \
               logical_bytes, disk_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                segment_id.simple().to_string(),
                wrapped,
                instants.day.days_since_epoch(),
                instants.first,
                instants.last,
                as_column(sizes.0)?,
                as_column(sizes.1)?
            ],
        )
        .map_err(db_error)?;
        let ordinal = tx.last_insert_rowid();
        for total in kinds {
            let (events, bytes) = (as_column(total.events)?, as_column(total.bytes)?);
            tx.execute(
                "INSERT INTO segment_totals (segment, kind, events, bytes) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![ordinal, total.kind, events, bytes],
            )
            .map_err(db_error)?;
            tx.execute(
                "INSERT INTO daily_totals (day, kind, events, bytes) VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (day, kind) DO UPDATE SET events = events + excluded.events, \
                 bytes = bytes + excluded.bytes",
                rusqlite::params![instants.day.days_since_epoch(), total.kind, events, bytes],
            )
            .map_err(db_error)?;
        }
        failpoint::hit("batch_in_the_transaction")?;
        tx.commit().map_err(db_error)?;
        self.look_after_the_first_commit(changes_before)?;
        Ok(Ordinal::of_row(ordinal))
    }

    pub(crate) fn pragma_int(&self, name: &str) -> StoreResult<i64> {
        pragma_int(&self.conn, name)
    }

    pub(in crate::store) fn live_page_bytes(&self) -> StoreResult<u64> {
        let read = |name: &str| -> StoreResult<u64> {
            pragma_number(&self.conn, name)
                .map_err(db_error)?
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(|| StoreError::Corrupt("a page count does not read".to_string()))
        };
        Ok(read("page_count")?
            .saturating_sub(read("freelist_count")?)
            .saturating_mul(read("page_size")?))
    }

    pub(crate) fn cipher_provider(&self) -> StoreResult<String> {
        self.conn
            .query_row("PRAGMA cipher_provider", [], |r| r.get::<_, String>(0))
            .map_err(db_error)
    }

    pub(in crate::store) fn keyed_segments(&self) -> Option<u64> {
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

    /// The truncating checkpoint: the log copied into store.db, the
    /// full-flush call asked for on store.db on a Mac (fullfsync, read back
    /// at the open) and the log emptied; `CheckpointIncomplete` when a reader
    /// held it back or the log still holds bytes. Where that call fails the
    /// library makes a plain fsync in its place and answers Ok, a named limit
    /// (ruling ST-50). After a shred's key clear, with its full flush
    /// answered, it is the step that makes the shred last through a loss of
    /// power, before any file of the shred is removed (ruling PR-30 PR9 (2)).
    pub(crate) fn checkpoint_truncate(&self) -> StoreResult<Checkpoint> {
        let busy = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                r.get::<_, i64>(0)
            });
        #[cfg(test)]
        let busy = code_seams::or_forced_at(code_seams::CHECKPOINT_QUERY, busy);
        let busy = busy.map_err(db_error)?;
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

fn require_mend(linked: bool) -> StoreResult<()> {
    if linked {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect("page_gap_mend_linked"))
    }
}

fn setting_in_effect(name: &'static str, read: i64, wanted: i64) -> StoreResult<()> {
    if read == wanted {
        Ok(())
    } else {
        Err(StoreError::SettingNotInEffect(name))
    }
}

fn pragma_number(conn: &Connection, name: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(&format!("PRAGMA {name}"), [], |r| {
        Ok(match r.get::<_, rusqlite::types::Value>(0)? {
            rusqlite::types::Value::Integer(n) => Some(n),
            rusqlite::types::Value::Text(text) => text.trim().parse().ok(),
            _ => None,
        })
    })
}

fn require_setting(conn: &Connection, name: &'static str, wanted: i64) -> StoreResult<()> {
    match pragma_number(conn, name).map_err(db_error)? {
        Some(read) => setting_in_effect(name, read, wanted),
        None => Err(StoreError::SettingNotInEffect(name)),
    }
}

fn require_whole(conn: &Connection, length: u64) -> StoreResult<()> {
    match pragma_number(conn, "page_size").map_err(unreadable_or_db_error)? {
        Some(size) if size > 0 && length > 0 && length.is_multiple_of(size as u64) => Ok(()),
        _ => Err(StoreError::DatabaseUnreadable),
    }
}

fn holds_the_key_table(conn: &Connection) -> StoreResult<bool> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'segment_keys'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|count| count == 1)
    .map_err(unreadable_or_db_error)
}

fn another_version(conn: &Connection, length: u64) -> StoreResult<()> {
    let found = pragma_int(conn, "user_version").map_err(|_| StoreError::DatabaseUnreadable)?;
    require_whole(conn, length)?;
    if found < DATABASE_VERSION && !holds_the_key_table(conn)? {
        return Err(StoreError::DatabaseUnreadable);
    }
    known_version(found, DATABASE_VERSION)
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
const CIPHER_VERSION: &str = "4.20.0";

fn require_pinned_cipher(conn: &Connection) -> StoreResult<()> {
    let text = |pragma: &str| -> StoreResult<String> {
        #[cfg(test)]
        if let Some(answer) = pinned_cipher_seam::text(pragma) {
            return Ok(answer);
        }
        conn.query_row(pragma, [], |r| r.get(0)).map_err(db_error)
    };
    let os_entropy_hook = || {
        #[cfg(test)]
        if let Some(answer) = pinned_cipher_seam::os_entropy_hook() {
            return answer;
        }
        halvo_sqlcipher::os_entropy_hook_installed()
    };
    cipher_in_effect(
        &text("PRAGMA cipher_provider")?,
        &text("PRAGMA cipher_version")?,
        os_entropy_hook(),
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

fn require_foreign_keys(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "foreign_keys")? {
        1 => Ok(()),
        _ => Err(StoreError::SettingNotInEffect("foreign_keys")),
    }
}

fn require_synchronous_full(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "synchronous")? {
        2 => Ok(()),
        _ => Err(StoreError::SettingNotInEffect("synchronous")),
    }
}

fn require_fullfsync(conn: &Connection) -> StoreResult<()> {
    match pragma_int(conn, "fullfsync")? {
        1 => Ok(()),
        _ => Err(StoreError::SettingNotInEffect("fullfsync")),
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
        Ok(durable::EntryIdentity {
            kind,
            device,
            file_number,
        }) => (kind, device, file_number),
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
    Open { db: Box<Db>, store_key: StoreKey },
    Deleting(Option<Box<DeleteReport>>),
    Deleted(Box<DeleteReport>),
}

pub(in crate::store) struct RootStep {
    reading: RootReading,
    roots: RootsLeft,
}

/// How the store reaches its root: through the roots directory that it holds,
/// with the identity of its root file, or through a custody that keeps no
/// root file.
enum RootKeeping {
    Held {
        roots: RootsHandle,
        root_file: FileIdentity,
    },
    NoRootFile,
}

impl RootKeeping {
    fn named(&self, id: RootId) -> NamedRoot {
        match self {
            RootKeeping::Held { root_file, .. } => NamedRoot {
                id,
                by: NamedBy::TheStore,
                file: Some(*root_file),
            },
            RootKeeping::NoRootFile => NamedRoot::by_id(id),
        }
    }
}

/// The encrypted store: the SQLCipher database, the per-segment files
/// under `dir`, and the one store key in the clear only in memory.
pub(in crate::store) struct EncryptedStore {
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
    /// The roots listing of the call on this handle that read its root
    /// destroyed; a later call reports from it and opens no roots path.
    root_destroyed_with: Option<RootsLeft>,
    roots: RootKeeping,
    reconciled: Reconciled,
    pub(in crate::store) owed_files: BTreeSet<uuid::Uuid>,
    access: OpenFor,
    _writer_lock: DirLock,
}

impl EncryptedStore {
    /// Create a fresh store in `dir` under `custody`, whose roots directory
    /// holds the new root. It takes the writer lock and answers
    /// [`StoreError::Locked`] while another process holds the lock or the
    /// database and [`StoreError::AlreadyExists`] when any entry stands at
    /// `store.key`, with nothing removed; then it finishes what a stopped
    /// create left in `create.pending`. Otherwise it removes the keyless
    /// leftovers (a segment that it cannot remove stays for the open-time
    /// sweep), records the custody kind and root id in `create.pending`, opens
    /// a new database under a random store key, makes the root, publishes the
    /// key file create-only and removes `create.pending`. When a step fails it
    /// destroys the root that it made, removes the key file and the database
    /// files, every step attempted, and answers the step's error, or a
    /// removal's error when one failed. On Windows a store path or a roots
    /// path of which a part ends in a dot or a space answers
    /// `Io(InvalidInput)` before anything is made. Before every other answer,
    /// and before anything is made, it answers
    /// [`StoreError::SettingNotInEffect`] with `page_gap_mend_linked` when the
    /// linked database does not zero the gap of a page that it builds again or
    /// edits in place under secure delete; on the database that it has just
    /// made it reads auto_vacuum and the page size back and answers the same
    /// variant with `auto_vacuum` or `page_size` unless they read 0 and 4,096,
    /// as a step that fails.
    pub fn create(
        dir: &Path,
        custody: Box<dyn KeyCustody + Send>,
        retention: Duration,
    ) -> StoreResult<Self> {
        Self::create_with(dir, custody, retention, &mut || {})
    }

    pub(in crate::store) fn create_with(
        dir: &Path,
        custody: Box<dyn KeyCustody + Send>,
        retention: Duration,
        after_the_handle: &mut dyn FnMut(),
    ) -> StoreResult<Self> {
        require_mend(halvo_sqlcipher::page_gap_mend_linked())?;
        #[cfg(windows)]
        refuse_a_part_that_windows_trims(&[Some(dir), custody.roots()])?;
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
        if key_file.present()? {
            return Err(StoreError::AlreadyExists);
        }
        if let Some(roots) = custody.roots() {
            clear_pending(dir, CleanupRoots::Path(roots), key_file.names())?;
            if file_root::root_file_may_be_present(roots, custody.root_id()) {
                #[cfg(test)]
                failpoint::seam(SEAM_IN_THE_REFUSAL_ARM);
                return Err(StoreError::Custody(a_root_that_stands(custody.as_ref())));
            }
        }
        failpoint::hit("create_before_the_database_step")?;
        #[cfg(test)]
        failpoint::seam(SEAM_BEFORE_THE_DATABASE_STEP);
        Db::destroy_in(dir)?;
        let _ = segment::remove_segments_tree(dir);
        write_pending(dir, custody.as_ref(), PendingReading::Owed, None)?;
        let store_key = StoreKey::random()?;
        let db = database_key(&store_key)
            .and_then(|key| Db::open(dir, &key))
            .map_err(|e| Self::without_database(dir, e))?;
        let db = Self::under_a_new_root(dir, custody.as_ref(), db)?;
        let (wrapped, roots) = match Self::wrap_under_the_new_root(custody.as_ref(), &store_key) {
            Ok(wrapped) => wrapped,
            Err((e, held)) => {
                drop(db);
                let own_root = held.as_ref().map(|roots| (roots, custody.root_id()));
                return Err(Self::abandon_create(
                    dir,
                    &key_file,
                    None,
                    custody.as_ref(),
                    own_root.map(|(roots, id)| (roots, NamedRoot::by_id(id))),
                    e,
                ));
            }
        };
        let own_root = match &roots {
            RootKeeping::Held { roots: held, .. } => Some((held, roots.named(custody.root_id()))),
            RootKeeping::NoRootFile => None,
        };
        if let Err(e) = write_pending(
            dir,
            custody.as_ref(),
            PendingReading::Made,
            own_root.and_then(|(_, root)| root.file),
        ) {
            drop(db);
            return Err(Self::abandon_create(
                dir,
                &key_file,
                None,
                custody.as_ref(),
                own_root,
                e,
            ));
        }
        #[cfg(test)]
        failpoint::seam(SEAM_AFTER_THE_ROOT);
        let (written, published) =
            key_file.publish_keeping(&KeyFile::new(custody.as_ref(), KeyLabel::STORE, &wrapped));
        match written
            .and_then(|()| failpoint::hit("create_after_the_key_file"))
            .and_then(|()| remove_pending(dir))
        {
            Ok(()) => Ok(Self {
                dir: store,
                retention,
                custody,
                keyed: Keyed::Open {
                    db: Box::new(db),
                    store_key,
                },
                fs: Arc::new(OsFs),
                unwrap_time: Duration::ZERO,
                pending_left: false,
                root_destroyed_with: None,
                roots,
                reconciled: Reconciled::default(),
                owed_files: BTreeSet::new(),
                access: OpenFor::Write,
                _writer_lock,
            }),
            Err(e) => {
                drop(db);
                Err(Self::abandon_create(
                    dir,
                    &key_file,
                    Some(&published),
                    custody.as_ref(),
                    own_root,
                    e,
                ))
            }
        }
    }

    /// The wrap of a create, after its create_root answered Ok: through the
    /// roots directory, held from here on, for a custody that keeps a root
    /// file, with the identity of the root file that the wrap read. On a
    /// failure, the error and the held directory when it was taken.
    fn wrap_under_the_new_root(
        custody: &dyn KeyCustody,
        store_key: &StoreKey,
    ) -> Result<(WrappedKey, RootKeeping), (StoreError, Option<RootsHandle>)> {
        let Some(path) = custody.roots() else {
            return custody
                .wrap_store_key(store_key, KeyLabel::STORE)
                .map(|wrapped| (wrapped, RootKeeping::NoRootFile))
                .map_err(|e| (StoreError::Custody(e), None));
        };
        let roots = RootsHandle::open(path).map_err(|e| (StoreError::Custody(e), None))?;
        match custody.wrap_store_key_in(&roots, store_key, KeyLabel::STORE) {
            Ok((wrapped, root_file)) => Ok((wrapped, RootKeeping::Held { roots, root_file })),
            Err(e) => Err((StoreError::Custody(e), Some(roots))),
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
        published: Option<&Published>,
        custody: &dyn KeyCustody,
        own_root: Option<(&RootsHandle, NamedRoot)>,
        cause: StoreError,
    ) -> StoreError {
        let own_name = match published {
            Some(published) => key_file.abandon(published),
            None => Ok(false),
        };
        let kept_for_the_entry_there =
            !matches!(own_name, Ok(true)) && key_file.named_root() == Some(custody.root_id());
        let root = if kept_for_the_entry_there {
            Ok(())
        } else {
            match own_root {
                Some((roots, root)) => file_root::destroy_root_through(&roots.dir, root),
                None => custody.destroy_root(),
            }
            .map_err(StoreError::Custody)
        };
        let key_file = own_name.map(drop).map_err(io_error);
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
    /// database without the store's tables. A version is believed only from a
    /// `store.db` that the store's key, or the database key derived from it,
    /// opens and that is whole (no shorter than its header says, which SQLite
    /// itself refuses at the first read, and a whole number of its pages long,
    /// read while no log with frames stands): a version above this
    /// build's answers [`StoreError::NewerStore`] whatever its schema holds, a
    /// version below answers [`StoreError::OlderStore`] when the database
    /// holds the table `segment_keys` and `DatabaseUnreadable` when it does
    /// not, and a cut file answers `DatabaseUnreadable` whatever its version.
    /// A refused open of another version leaves every entry of the store's
    /// directory with its name, length and bytes. Fails
    /// [`StoreError::KeyFileUnparseable`] for a key file that does not parse or
    /// whose wrapped key has another length than a wrap,
    /// [`StoreError::KeyFileOfAnotherLabel`] for a key file of another label,
    /// [`StoreError::OlderStore`] for a key file of version 1, the obstructed
    /// variant when a link or an entry of another kind stands at a store name
    /// or the directory leaves its name, the obstructed variant for the log
    /// when the reconcile's shred, as the handle's first commit, finds its log
    /// gone from its name (nothing then reports that shred, and `store.db`
    /// and every entry of the directory keep their names, lengths and bytes,
    /// so the next open shreds the row whole), the checkpoint's own error when
    /// it is `Db`, `Io` or `CheckpointIncomplete`, and `Corrupt` for any other
    /// error of the checkpoint, when the reconcile's shred
    /// committed and its checkpoint then failed (nothing then reports that
    /// shred, the close does not checkpoint, and the next open's drain brings
    /// the shred into `store.db` before it answers), and [`StoreError::Custody`] with the
    /// custody's own variant when the custody kind is not in this build or the
    /// unwrap fails: [`CustodyError::RootMissing`], [`CustodyError::RootCorrupt`]
    /// and [`CustodyError::WrapAuthFailed`] mean that the key cannot come back,
    /// and [`CustodyError::RootDenied`] that the root file cannot be read.
    /// No call waits on a
    /// FIFO. On Windows a store path or a roots path of which a part ends in a
    /// dot or a space answers `Io(InvalidInput)` before anything is opened.
    /// Before every other answer, and before it touches a file, it answers
    /// [`StoreError::SettingNotInEffect`] with `page_gap_mend_linked` when the
    /// linked database does not zero the gap of a page that it builds again or
    /// edits in place under secure delete; on a database that the key opened
    /// and whose version is this build's, it answers the same variant with
    /// `auto_vacuum` when `store.db` does not read auto_vacuum 0, and with
    /// `page_size` when it does not read a page size of 4,096. The look at the
    /// tables, the two settings and a look that `store.db` at its name is
    /// still the file that the open holds come before the open drains its log
    /// into `store.db`, so an open that one of them refuses leaves every entry
    /// of the store's directory with its name, length and bytes, and the log
    /// with the frames that it held. The one refusal that can follow the
    /// drain is that of a `store.db` that another program moves or exchanges
    /// at its name during the drain, which is the obstructed variant and
    /// never `Ok`. `access` is the caller's decision, and the open runs the
    /// same steps for both: a handle opened with [`OpenFor::Maintenance`]
    /// expires, evicts, lists and deletes, and its batch write answers
    /// [`WriteFault::MaintenanceOnly`].
    pub fn open(
        dir: &Path,
        roots: &Path,
        retention: Duration,
        access: OpenFor,
    ) -> StoreResult<Self> {
        let mut opened = Self::open_with(dir, roots, retention, custody_for)?;
        opened.access = access;
        Ok(opened)
    }

    pub(in crate::store) fn open_with(
        dir: &Path,
        roots: &Path,
        retention: Duration,
        custody_for: impl FnOnce(&CustodyLocator) -> Result<Box<dyn KeyCustody + Send>, CustodyError>,
    ) -> StoreResult<Self> {
        require_mend(halvo_sqlcipher::page_gap_mend_linked())?;
        #[cfg(windows)]
        refuse_a_part_that_windows_trims(&[Some(dir), Some(roots)])?;
        let store = Dir::open(dir).map_err(store_dir_error)?;
        store.make_private().map_err(io_error)?;
        let _writer_lock = DirLock::take(&store)?;
        let key_file = KeyFile::of_store(&store)?.read(KeyLabel::STORE)?;
        let wrapped = key_file.wrapped_store_key()?;
        segment::refuse_a_linked_segments_root(&store)?;
        Db::require_file(&store)?;
        Db::refuse_a_live_holder(&store)?;
        let custody = custody_for(&key_file.locator(roots))?;
        let held = custody.roots().map(RootsHandle::open).transpose()?;
        let started = Instant::now();
        let (store_key, kept) = match held {
            Some(held) => {
                let (store_key, root_file) =
                    custody.unwrap_store_key_in(&held, &wrapped, KeyLabel::STORE)?;
                (
                    store_key,
                    RootKeeping::Held {
                        roots: held,
                        root_file,
                    },
                )
            }
            None => (
                custody.unwrap_store_key(&wrapped, KeyLabel::STORE)?,
                RootKeeping::NoRootFile,
            ),
        };
        let unwrap_time = started.elapsed();
        let db = match Db::open_existing(&store, &*database_key(&store_key)?) {
            Err(StoreError::DatabaseUnreadable) => {
                return Err(Db::another_version_of(&store, &store_key)
                    .unwrap_or(StoreError::DatabaseUnreadable))
            }
            opened => opened?,
        };
        let cleanup_roots = match &kept {
            RootKeeping::Held {
                roots: held,
                root_file,
            } => CleanupRoots::Held(held, *root_file),
            RootKeeping::NoRootFile => CleanupRoots::Path(roots),
        };
        let pending_left =
            clear_pending(&store, cleanup_roots, KeyNames::Root(key_file.root_id)).is_err();
        let mut opened = Self {
            dir: store,
            retention,
            custody,
            keyed: Keyed::Open {
                db: Box::new(db),
                store_key,
            },
            fs: Arc::new(OsFs),
            unwrap_time,
            pending_left,
            root_destroyed_with: None,
            roots: kept,
            reconciled: Reconciled::default(),
            owed_files: BTreeSet::new(),
            access: OpenFor::Write,
            _writer_lock,
        };
        opened.reconciled = opened.reconcile();
        if let Keyed::Open { db, .. } = &opened.keyed {
            if let Some(failure) = db.look_failure() {
                return Err(failure);
            }
        }
        Ok(opened)
    }

    fn keyed(&self) -> StoreResult<(&Db, &StoreKey)> {
        match &self.keyed {
            Keyed::Open { db, store_key } => match db.look_failure() {
                Some(failure) => Err(failure),
                None => Ok((db, store_key)),
            },
            Keyed::Deleting(_) | Keyed::Deleted(_) => Err(StoreError::Sealed),
        }
    }

    pub(in crate::store) fn db(&self) -> StoreResult<&Db> {
        self.keyed().map(|(db, _)| db)
    }

    fn writable(&self) -> StoreResult<(&Db, &StoreKey)> {
        let keyed = self.keyed()?;
        if self.access == OpenFor::Maintenance {
            return Err(StoreError::MaintenanceOnly);
        }
        Ok(keyed)
    }

    pub(in crate::store) fn maintained_db(&self) -> StoreResult<&Db> {
        match &self.keyed {
            Keyed::Open { db, .. } if db.owes_a_checkpoint() => Ok(db),
            _ => self.db(),
        }
    }

    pub(in crate::store) fn dir(&self) -> &Path {
        self.dir.path()
    }

    pub(in crate::store) fn handle(&self) -> &Dir {
        &self.dir
    }

    /// How long the custody unwrap of the store key took when `open` made this
    /// handle; zero for a handle from `create`, which unwraps nothing.
    pub fn unwrap_time(&self) -> Duration {
        self.unwrap_time
    }

    pub(in crate::store) fn fs(&self) -> &dyn DurableFs {
        self.fs.as_ref()
    }

    #[cfg(test)]
    pub(in crate::store) fn use_fs(&mut self, fs: Arc<dyn DurableFs>) {
        self.fs = fs;
    }

    pub(in crate::store) fn store_key(&self) -> StoreResult<&StoreKey> {
        self.keyed().map(|(_, key)| key)
    }

    pub(in crate::store) fn answer(&self, e: StoreError) -> StoreError {
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
        let seg_key =
            segment::unwrap_segment_key(&*wrapping_key(store_key)?, segment_id, &wrapped)?;
        segment::read_segment_file(&self.dir, &seg_key, segment_id, out).map_err(|e| self.answer(e))
    }

    /// Store one batch, all or nothing. A batch that this method refuses, or
    /// whose segment id the store holds already, writes nothing. Otherwise it
    /// writes the segment file under a fresh segment key and makes it durable,
    /// then one transaction inserts the complete key row, the totals of each
    /// kind and the per-day rollup; when any step fails, the transaction rolls
    /// back and the file is removed, so no row and no file remain from the
    /// call (a file whose removal fails is left for the open-time sweep, as a
    /// file with no key row). One step follows the commit: at a handle's
    /// first commit, the look that the log in the store directory holds the
    /// commit's frames. When it fails the call answers the obstructed variant
    /// for the log (in the `Io` class), the handle answers that variant to
    /// every later call, and its close does not checkpoint, so the row of
    /// that commit reaches no file at the store's names and `store.db` stays
    /// as it was; the file is removed, so no row and no file remain. When the
    /// look could not read the log's entry, the log may stand at its name
    /// with the row, so the file stays: the next open keeps it with its row,
    /// or sweeps it as a file with no key row. The caller keeps the batch on
    /// every answer. A handle opened with [`OpenFor::Maintenance`] answers
    /// [`WriteFault::MaintenanceOnly`] and writes nothing.
    ///
    /// Two named limits of the refusal, which bounds each batch and not the
    /// day: (a) the events or the bytes of one day and kind, summed over its
    /// batches, past the 64-bit range: the rollup then stores that sum as a
    /// real number and [`EncryptedStore::inventory`] answers
    /// [`StoreError::Corrupt`]; no real input reaches it, since no capture
    /// makes 9.2e18 events; (b) a kind with 0 events and some bytes beside a
    /// kind with events: the batch is stored, its rollup row holds those
    /// bytes, and the inventory does not list a kind with 0 events; such a
    /// summary is the caller's fault, a malformed summary.
    pub fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        if let Some(refusal) = refusal_of(&write) {
            self.writable()?;
            return Err(WriteFault::Refused(refusal));
        }
        let (db, _) = self.writable()?;
        if let Some(ordinal) = db
            .ordinal_of(*write.segment_id)
            .map_err(|e| self.answer(e))?
        {
            return Err(WriteFault::AlreadyWritten { ordinal });
        }
        let instants = Instants {
            day: write.day,
            first: write.first_capture,
            last: write.last_capture,
        };
        let mut payload = write.payload;
        self.store_segment(*write.segment_id, &instants, &mut payload, write.kinds)
            .map_err(WriteFault::from)
    }

    fn store_segment(
        &mut self,
        segment_id: uuid::Uuid,
        instants: &Instants,
        bytes: &mut dyn Read,
        kinds: &[KindTotal<'_>],
    ) -> StoreResult<Written> {
        let (db, store_key) = self.writable()?;
        let seg_key = SegmentKey::random()?;
        let wrapped = segment::wrap_segment_key(&*wrapping_key(store_key)?, segment_id, &seg_key)?;
        let header = SegmentHeader { segment_id };
        let logical_bytes =
            segment::write_segment_file(&self.dir, self.fs.as_ref(), &seg_key, &header, bytes)?;
        let committed = failpoint::hit("batch_before_the_transaction")
            .and_then(|()| segment::disk_bytes(&self.dir, segment_id))
            .and_then(|disk_bytes| {
                db.insert_segment(
                    segment_id,
                    &wrapped,
                    instants,
                    (logical_bytes, disk_bytes),
                    kinds,
                )
                .map(|ordinal| Written {
                    ordinal,
                    logical_bytes,
                    disk_bytes,
                })
            });
        match committed {
            Ok(written) => Ok(written),
            Err(e) => {
                if !db.look_unread() {
                    segment::remove_written_segment(&self.dir, segment_id)?;
                }
                Err(self.answer(e))
            }
        }
    }

    /// The legacy write of [`SegmentStore::write_segment`]: a random id, no
    /// totals, both instants the write instant.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no caller outside the store's tests: the adapter writes through write_batch, and core-3 deletes this call"
        )
    )]
    fn write_untallied(&mut self, bytes: &mut dyn Read) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        let segment_id = random_segment_id()?;
        let instants = Instants::at(crate::event::unix_nanos(SystemTime::now()));
        let written = self.store_segment(segment_id, &instants, bytes, &[])?;
        Ok((
            SegmentRef {
                segment_id,
                byte_offset: 0,
                byte_len: written.logical_bytes,
            },
            SegmentKeyId(written.ordinal.get() as i64),
        ))
    }

    /// A segment with no totals that falls due at `expires_at`, in whole Unix
    /// seconds, under this handle's window: its first capture is that second
    /// less the window. `created_at` is not stored.
    #[cfg(test)]
    pub(in crate::store) fn write_segment_with_expiry(
        &mut self,
        bytes: &mut dyn Read,
        created_at: u64,
        expires_at: u64,
    ) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        let _ = created_at;
        let first = (i128::from(expires_at) - i128::from(self.retention.as_secs()))
            .saturating_mul(1_000_000_000)
            .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64;
        let segment_id = random_segment_id()?;
        let written = self.store_segment(segment_id, &Instants::at(first), bytes, &[])?;
        Ok((
            SegmentRef {
                segment_id,
                byte_offset: 0,
                byte_len: written.logical_bytes,
            },
            SegmentKeyId(written.ordinal.get() as i64),
        ))
    }

    /// The window of this handle.
    pub(in crate::store) fn retention(&self) -> std::time::Duration {
        self.retention
    }

    /// What the reconcile of the open that made this handle did; zero for a
    /// handle from `create`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no caller outside the store's tests until daemon-3e shows the open's reconcile counts"
        )
    )]
    pub fn reconciled(&self) -> Reconciled {
        self.reconciled
    }

    pub(in crate::store) fn wrapping_key(&self) -> StoreResult<Zeroizing<[u8; 32]>> {
        wrapping_key(self.store_key()?)
    }

    #[cfg(test)]
    pub(in crate::store) fn database_key(&self) -> StoreResult<Zeroizing<[u8; 32]>> {
        database_key(self.store_key()?)
    }

    /// The root step of the handle. Once a call on this handle read its root
    /// destroyed, a later call reads its root as destroyed from the handle's
    /// state and the other roots from the listing that the earlier call kept,
    /// and opens no roots path again (ruling ST-45 H3).
    pub(in crate::store) fn root_step(&self) -> RootStep {
        if let Some(kept) = &self.root_destroyed_with {
            return RootStep {
                reading: RootReading::Destroyed,
                roots: kept.clone(),
            };
        }
        let named = self.custody.root_id();
        let (reading, roots) = match &self.roots {
            RootKeeping::Held { roots, .. } => file_root::destroy_roots_through(
                &roots.dir,
                Some(self.roots.named(named)),
                RootStepForm::Witness,
            ),
            RootKeeping::NoRootFile => (
                match self.custody.destroy_root() {
                    Ok(()) => RootReading::Destroyed,
                    Err(_) => RootReading::Left(Some(named)),
                },
                RootsLeft::none_counted(true),
            ),
        };
        RootStep { reading, roots }
    }

    pub(in crate::store) fn untouched(&self) -> DeleteReport {
        DeleteReport {
            keyed_segments: None,
            root: RootReading::Left(Some(self.custody.root_id())),
            roots: RootsLeft::none_counted(true),
            root_not_handled: None,
            key_file_gone: false,
            database_left: true,
            files_left: 0,
            not_followed: 0,
        }
    }

    pub(in crate::store) fn remove_files_after(&mut self, root_step: RootStep) {
        self.keyed = Keyed::Deleting(None);
        if root_step.reading == RootReading::Destroyed && self.root_destroyed_with.is_none() {
            self.root_destroyed_with = Some(root_step.roots.clone());
        }
        let mut report =
            remove_the_store_files(&self.dir, None, root_step.reading, root_step.roots);
        if !matches!(self.dir.still_named(), Ok(true)) {
            report.not_followed += 1;
        }
        self.keyed = Keyed::Deleting(Some(Box::new(report)));
    }

    pub(in crate::store) fn deleting_report(&self) -> Option<DeleteReport> {
        match &self.keyed {
            Keyed::Deleting(Some(report)) => Some((**report).clone()),
            _ => None,
        }
    }

    #[cfg(all(test, unix))]
    pub(in crate::store) fn pending_left(&self) -> bool {
        self.pending_left
    }

    #[cfg(all(test, unix))]
    pub(in crate::store) fn writer_lock_file(&self) -> &std::fs::File {
        &self._writer_lock.file
    }

    pub(in crate::store) fn mark_deleted(&mut self, report: DeleteReport) {
        self.keyed = Keyed::Deleted(Box::new(report));
    }

    pub(in crate::store) fn finished_report(&self) -> Option<DeleteReport> {
        match &self.keyed {
            Keyed::Deleted(report) => Some((**report).clone()),
            _ => None,
        }
    }

    /// The delete tail on a data directory `dir` of the closed `layout`, which
    /// no handle holds. It opens `dir` as a handle by the components of its
    /// path, following no link at its last part in any spelling (`dir/`,
    /// `dir/.`), opens the store directory and the roots directory from the
    /// handle of the directory that holds each by their fixed names, never by
    /// a path and following no link, takes the writer lock and runs the root
    /// step; when it holds the writer lock it removes the key file, the
    /// database files and the segments tree through the store's handle, and
    /// without it it removes none of them. It never opens the database. A root
    /// is named by `store.key`, by its name, or by a `create.pending` that
    /// reads made, by the identity of its root file, never by one that reads
    /// owed. With the lock held, with nothing at the store's name, and in the
    /// data layout after it removed a link or an entry of another kind at the
    /// store's name (counted as not followed), the root step sweeps the roots
    /// directory: it destroys every root file and partial root file there,
    /// named or not, and removes every regular file at any other name,
    /// zero-filling it first only when it has one name and a root file's
    /// length, which it reads by the name before any mode change (ruling
    /// ST-48 U1); by a made mark the file of its identity is written through
    /// only where the name at which it stands lets a write (ruling ST-48 U2);
    /// it removes a link or an entry of another kind as the entry that it is,
    /// and enters no directory; in the scratch layout nothing is
    /// swept where no store directory stands, because its roots lie inside it.
    /// A link, or an entry of another kind, at the roots directory's name is
    /// not followed: the roots read as not listed. A data directory that is a
    /// link, in either layout and in any spelling, is not followed and nothing
    /// is removed: the root is left unread, the roots are not listed and
    /// `not_followed` is 1. A store directory that it cannot open leaves its
    /// root unread; a lock that it cannot take for a reason other than a
    /// holder, and on Windows a data directory of which a part ends in a dot
    /// or a space (read through its verbatim form, with no removal and
    /// `not_followed` 1), get the root step for the named root alone, with the
    /// witness; these answer [`StoreError::DeleteIncomplete`] with the files as
    /// left. The root reads destroyed only when this call destroyed a named
    /// root's file, left when a named root was not destroyed, and unnamed when
    /// nothing names one; a key file of another version, or of a custody that
    /// keeps no root file under the roots directory, is removed with the rest,
    /// and its root is reported as not handled and never as destroyed. Answers
    /// as [`EncryptedStore::delete_all`] does, `Ok` only when the roots
    /// directory was listed after the step and nothing stays in it at any name,
    /// or nothing stands at the roots path, and [`StoreError::Locked`] while
    /// another process holds the store directory (ruling ST-45 H1; store-3b;
    /// ruling ST-47 T1 to T3). Two named limits of the root step stand. A
    /// mode change that a program racing the step causes between the read of
    /// a file's count of names and length by its name and the open of its
    /// kept handle: the mode is set by the name, so a file of another party
    /// put at the name in that window gets mode 0600, and the kept handle then
    /// shows another identity and the name is removed with no write, its
    /// bytes kept. And a file of another program that a process of the same
    /// user hard-links at a root file's name is zeroed through every name
    /// when it has a root file's length at a root name (any length above 0 on
    /// Windows) or stands at a partial name (on the walk that a made mark
    /// names, a partial name of the mark's own id; ruling ST-49).
    pub fn delete_data_dir(dir: &Path, layout: Layout) -> StoreResult<DeleteReport> {
        #[cfg(windows)]
        if durable::has_a_part_that_windows_trims(dir) {
            return finished(Self::root_step_alone_at_a_trimmed_name(dir, layout));
        }
        match layout {
            Layout::DataDirectory => Self::delete_in_a_data_directory(dir),
            Layout::ScratchStore => Self::delete_a_scratch_store(dir),
        }
    }

    fn delete_in_a_data_directory(dir: &Path) -> StoreResult<DeleteReport> {
        let parent = match Dir::open(dir) {
            Ok(parent) => parent,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return finished(nothing_of_a_store(
                    RootReading::Unnamed,
                    RootsLeft::none_counted(true),
                ))
            }
            Err(e) if durable::refusal(&e).is_some() => {
                let mut report =
                    not_opened(RootReading::Left(None), RootsLeft::none_counted(false));
                report.not_followed = 1;
                return finished(report);
            }
            Err(_) => {
                return finished(not_opened(
                    RootReading::Left(None),
                    RootsLeft::none_counted(false),
                ))
            }
        };
        #[cfg(test)]
        failpoint::seam(SEAM_AFTER_THE_DATA_DIRECTORY_OPEN);
        match parent.open_dir(OsStr::new(STORE_DIR_NAME)) {
            Ok(store) => Self::delete_through(&store, &parent),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let (root, roots) =
                    file_root::destroy_roots_below(&parent, None, RootStepForm::Sweep);
                finished(nothing_of_a_store(root, roots))
            }
            Err(e) if durable::refusal(&e).is_some() => {
                let gone = parent
                    .remove_entry(OsStr::new(STORE_DIR_NAME))
                    .and_then(|()| parent.sync())
                    .is_ok();
                let (root, roots) = if gone {
                    file_root::destroy_roots_below(&parent, None, RootStepForm::Sweep)
                } else {
                    (RootReading::Unnamed, RootsLeft::none_counted(false))
                };
                let mut report = nothing_of_a_store(root, roots);
                report.files_left = u64::from(!gone);
                report.not_followed = 1;
                finished(report)
            }
            #[cfg(windows)]
            Err(e) if e.raw_os_error() == Some(durable::ERROR_SHARING_VIOLATION) => {
                Err(StoreError::Locked)
            }
            Err(_) => finished(not_opened(
                RootReading::Left(None),
                RootsLeft::none_counted(false),
            )),
        }
    }

    fn delete_a_scratch_store(dir: &Path) -> StoreResult<DeleteReport> {
        match Dir::open(dir) {
            Ok(store) => Self::delete_through(&store, &store),
            Err(e) if e.kind() == ErrorKind::NotFound => finished(nothing_of_a_store(
                RootReading::Unnamed,
                RootsLeft::none_counted(true),
            )),
            Err(e) if durable::refusal(&e).is_some() => {
                let mut report =
                    not_opened(RootReading::Left(None), RootsLeft::none_counted(false));
                report.not_followed = 1;
                finished(report)
            }
            #[cfg(windows)]
            Err(e) if e.raw_os_error() == Some(durable::ERROR_SHARING_VIOLATION) => {
                Err(StoreError::Locked)
            }
            Err(_) => finished(not_opened(
                RootReading::Left(None),
                RootsLeft::none_counted(false),
            )),
        }
    }

    /// The tail through the handle of the store directory, with the roots
    /// directory opened from the handle of `parent`, the directory that holds
    /// it.
    fn delete_through(store: &Dir, parent: &Dir) -> StoreResult<DeleteReport> {
        let place = KeyFile::of_store(store)?;
        let writer_lock = match DirLock::take(store)
            .and_then(|lock| failpoint::hit("tail_lock").map(|()| lock))
        {
            Ok(lock) => Some(lock),
            Err(StoreError::Locked) => return Err(StoreError::Locked),
            Err(_) => None,
        };
        let form = match writer_lock {
            Some(_) => RootStepForm::Sweep,
            None => RootStepForm::NamedAlone,
        };
        #[cfg(test)]
        failpoint::seam(SEAM_BEFORE_THE_TAILS_ROOT_STEP);
        let (reading, roots, root_not_handled) = tail_root_step(store, &place, parent, form);
        let mut report = match writer_lock {
            Some(_) => remove_the_store_files(store, None, reading, roots),
            None => not_opened(reading, roots),
        };
        report.root_not_handled = root_not_handled;
        finished(report)
    }
}

impl EncryptedStore {
    /// The tail on a data directory of which a part ends in a dot or a
    /// space: Win32 trims that end when it opens a name that is not
    /// verbatim, so a removal could reach the sibling without it. The tail
    /// removes nothing. It runs the root step from the key file that it reads
    /// through the verbatim form of the path, takes no lock there, and counts
    /// the path as not followed.
    #[cfg(windows)]
    fn root_step_alone_at_a_trimmed_name(dir: &Path, layout: Layout) -> DeleteReport {
        let unread = || {
            (
                RootReading::Left(None),
                RootsLeft::none_counted(false),
                None,
            )
        };
        let (reading, roots, root_not_handled) = durable::verbatim_form(dir)
            .and_then(|verbatim| Dir::open(&verbatim).ok())
            .and_then(|parent| {
                let store = match layout {
                    Layout::DataDirectory => parent.open_dir(OsStr::new(STORE_DIR_NAME)).ok()?,
                    Layout::ScratchStore => parent.try_clone().ok()?,
                };
                let place = KeyFile::of_store(&store).ok()?;
                let roots_parent = match layout {
                    Layout::DataDirectory => &parent,
                    Layout::ScratchStore => &store,
                };
                Some(tail_root_step(
                    &store,
                    &place,
                    roots_parent,
                    RootStepForm::NamedAlone,
                ))
            })
            .unwrap_or_else(unread);
        let mut report = not_opened(reading, roots);
        report.root_not_handled = root_not_handled;
        report.not_followed = 1;
        report
    }
}

/// The root step of the tail in `form`, on the roots directory that
/// `parent` holds: the root that the key file names, by its name, or that a
/// made pending mark names when no key file stands, by the identity of its
/// root file that the mark records. The sweep runs also beside a key file
/// that this build does not handle, whose root then reads left on this call
/// (ruling ST-45 H1).
fn tail_root_step(
    store: &Dir,
    place: &KeyFilePlace,
    parent: &Dir,
    form: RootStepForm,
) -> (RootReading, RootsLeft, Option<RootId>) {
    let (named, root_not_handled) = match place.what_it_names() {
        KeyFileNames::Destroys(root_id) => (Some(NamedRoot::by_id(root_id)), None),
        KeyFileNames::NotHandled(root_id) => (None, Some(root_id)),
        KeyFileNames::Nothing => (
            read_pending(store).and_then(|pending| pending.made_root()),
            None,
        ),
    };
    let (reading, roots) = match root_not_handled {
        Some(root_id) if form == RootStepForm::Sweep => (
            RootReading::Left(Some(root_id)),
            file_root::destroy_roots_below(parent, None, RootStepForm::Sweep).1,
        ),
        Some(root_id) => (
            RootReading::Left(Some(root_id)),
            RootsLeft::none_counted(false),
        ),
        None => file_root::destroy_roots_below(parent, named, form),
    };
    (reading, roots, root_not_handled)
}

/// The report of a tail at whose store name no store directory stands, or no
/// longer stands after this call removed what stood there.
fn nothing_of_a_store(root: RootReading, roots: RootsLeft) -> DeleteReport {
    DeleteReport {
        keyed_segments: None,
        root,
        roots,
        root_not_handled: None,
        key_file_gone: true,
        database_left: false,
        files_left: 0,
        not_followed: 0,
    }
}

/// On Windows a create or an open given a store path or a roots path of which
/// a part ends in a dot or a space answers `Io(InvalidInput)` before it makes
/// or opens anything, verbatim or not: Win32 trims that end on a path that is
/// not verbatim, and a create there made its directory at the trimmed sibling
/// (hosted run 43578145799), so no store of Halvo's making lies under such a
/// name (ruling ST-45 H6 (b)).
#[cfg(windows)]
fn refuse_a_part_that_windows_trims(paths: &[Option<&Path>]) -> StoreResult<()> {
    if paths
        .iter()
        .flatten()
        .any(|path| durable::has_a_part_that_windows_trims(path))
    {
        return Err(StoreError::Io(ErrorKind::InvalidInput));
    }
    Ok(())
}

fn finished(report: DeleteReport) -> StoreResult<DeleteReport> {
    if report.complete() {
        Ok(report)
    } else {
        Err(StoreError::DeleteIncomplete(Box::new(report)))
    }
}

/// The report of a tail that could not open the store directory, or could not
/// take its lock for a reason other than a holder: the root step as it went,
/// and the store's files as left.
fn not_opened(root: RootReading, roots: RootsLeft) -> DeleteReport {
    DeleteReport {
        keyed_segments: None,
        root,
        roots,
        root_not_handled: None,
        key_file_gone: false,
        database_left: true,
        files_left: 1,
        not_followed: 0,
    }
}

fn remove_the_store_files(
    store: &Dir,
    keyed_segments: Option<u64>,
    root: RootReading,
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
        root,
        roots,
        root_not_handled: None,
        key_file_gone: key_file.gone,
        database_left: database.left,
        files_left: segments.entries_left + pending_left,
        not_followed: key_file.not_followed + database.not_followed + segments.links_or_others_met,
    }
}

impl SegmentStore for EncryptedStore {
    fn append_event(&mut self, event: &CaptureStreamEvent) -> StoreResult<()> {
        let _ = event;
        Err(StoreError::NotImplemented("append_event"))
    }

    fn write_segment(&mut self, bytes: &mut dyn Read) -> StoreResult<(SegmentRef, SegmentKeyId)> {
        self.write_untallied(bytes)
    }

    fn size_bytes(&self) -> StoreResult<u64> {
        if !matches!(self.keyed, Keyed::Open { .. }) {
            return Err(StoreError::Sealed);
        }
        dir_size(&self.dir)
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "only size_bytes calls it, which has no caller outside the store's tests until core-3 deletes it"
    )
)]
fn listed(dir: &Dir) -> StoreResult<Vec<durable::Entry>> {
    match dir.entries() {
        Ok(entries) => Ok(entries),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(io_error(e)),
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "only size_bytes calls it, which has no caller outside the store's tests until core-3 deletes it"
    )
)]
pub(in crate::store) fn dir_size(dir: &Dir) -> StoreResult<u64> {
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

/// What the steps of a database open answer in the directory `dir`, for the
/// trace of a create in a directory whose name is not Unicode (inbox entry
/// of 2026-11-13T20:16Z (d)): booleans and codes alone, no path.
#[cfg(test)]
#[cfg_attr(
    not(windows),
    expect(dead_code, reason = "the trace runs on Windows alone")
)]
pub(in crate::store) fn database_open_steps(dir: &Path) -> Vec<String> {
    let mut steps = Vec::new();
    let store = match Dir::open(dir) {
        Ok(store) => store,
        Err(e) => {
            steps.push(format!(
                "open_dir={:?} code={:?}",
                e.kind(),
                e.raw_os_error()
            ));
            return steps;
        }
    };
    steps.push(format!(
        "handle_path_is_unicode={}",
        store.path().to_str().is_some()
    ));
    match database_name(&store) {
        Ok((name, _, _)) => {
            steps.push(format!(
                "database_name_is_unicode={}",
                name.to_str().is_some()
            ));
            steps.push(format!(
                "sqlite_open={}",
                match Connection::open_with_flags(&name, PROBE_OPEN) {
                    Ok(_) => "Ok".to_string(),
                    Err(rusqlite::Error::InvalidPath(_)) => "InvalidPath".to_string(),
                    Err(e) => format!("code={:?}", e.sqlite_error_code()),
                }
            ));
        }
        Err(e) => steps.push(format!("database_name={e:?}")),
    }
    steps
}

#[cfg(test)]
pub(in crate::store) mod test_support {
    pub(in crate::store) use crate::store::pages;

    use std::ops::{Deref, DerefMut};
    use std::path::Path;

    use super::EncryptedStore;
    use crate::store::roots::RootGuard;
    use crate::store::DEFAULT_RETENTION;

    pub(in crate::store) struct TestStore {
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

    pub(in crate::store) fn make_store(dir: &Path) -> TestStore {
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

    #[test]
    fn the_open_and_the_create_refuse_a_library_without_the_mend_and_other_page_settings_by_name() {
        assert!(matches!(
            require_mend(false),
            Err(StoreError::SettingNotInEffect("page_gap_mend_linked"))
        ));
        assert!(require_mend(true).is_ok());
        assert!(halvo_sqlcipher::page_gap_mend_linked());
        assert!(matches!(
            setting_in_effect("auto_vacuum", 1, 0),
            Err(StoreError::SettingNotInEffect("auto_vacuum"))
        ));
        assert!(matches!(
            setting_in_effect("page_size", 8192, PAGE_SIZE),
            Err(StoreError::SettingNotInEffect("page_size"))
        ));
        assert!(setting_in_effect("auto_vacuum", 0, 0).is_ok());
        assert!(setting_in_effect("page_size", 4096, PAGE_SIZE).is_ok());
        let source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("store")
                .join("db.rs"),
        )
        .unwrap()
        .replace("\r\n", "\n");
        let code = &source[..source.find("\n#[cfg(test)]\nmod tests {").unwrap()];
        let call = "        require_mend(halvo_sqlcipher::page_gap_mend_linked())?;\n";
        let signature_end = ") -> StoreResult<Self> {\n";
        for function in ["fn open_with(", "fn create_with("] {
            let start = code.find(function).unwrap();
            let body = start + code[start..].find(signature_end).unwrap() + signature_end.len();
            assert!(
                code[body..].starts_with(call),
                "{function} does not begin with the gate of the mend"
            );
        }
        assert_eq!(code.matches(call).count(), 2);
        for line in [
            "require_setting(&self.conn, \"auto_vacuum\", 0)?;",
            "require_setting(&self.conn, \"page_size\", PAGE_SIZE)?;",
        ] {
            assert_eq!(
                code.matches(line).count(),
                2,
                "{line} stands once for a create and once for an open"
            );
        }
    }

    #[test]
    fn the_code_table_reads_each_primary_code_and_no_code() {
        let rows: [(Option<i32>, &str); 10] = [
            (Some(5), "Locked"),
            (Some(5 | (1 << 8)), "Locked"),
            (Some(6), "Locked"),
            (Some(11), "DatabaseUnreadable"),
            (Some(26), "DatabaseUnreadable"),
            (Some(13), "Io(StorageFull)"),
            (Some(10 | (7 << 8)), "Io(Other)"),
            (Some(14), "Io(Other)"),
            (Some(1), "Io(Other)"),
            (None, "Io(Other)"),
        ];
        let wrong: Vec<String> = rows
            .iter()
            .filter_map(|&(code, expected)| {
                let read = format!("{:?}", code_answer(code));
                (read != expected).then(|| format!("{code:?} read {read}, not {expected}"))
            })
            .collect();
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    #[test]
    fn the_text_of_a_database_error_holds_no_text_of_the_sqlite_message() {
        let message = "unable to open database file at /var/folders/secret-name/store.db";
        let e = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(14),
            Some(message.to_string()),
        );
        let answer = db_error(e);
        let shown = format!("{answer} {answer:?}");
        assert!(!shown.contains("secret-name"), "{shown}");
        assert!(!shown.contains("/var/folders"), "{shown}");
        assert!(!shown.contains("unable to open"), "{shown}");
    }
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
    fn append_event_is_refused_and_changes_no_byte_of_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, key_id) = store
            .write_segment(&mut std::io::Cursor::new(b"segment"))
            .unwrap();
        store.db().unwrap().checkpoint_truncate().unwrap();
        let files = || {
            ["store.db", "store.db-wal"]
                .map(|name| std::fs::read(dir.path().join(name)).unwrap_or_default())
        };
        let before = files();
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
        assert!(
            matches!(appended, Err(StoreError::NotImplemented("append_event"))),
            "{appended:?}"
        );
        assert!(files() == before, "append_event changed a database file");
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
        assert_eq!(db.pragma_int("foreign_keys").unwrap(), 1);
        assert_eq!(db.pragma_int("user_version").unwrap(), DATABASE_VERSION);
        assert_eq!(db.pragma_int("checkpoint_fullfsync").unwrap(), 1);
        assert_eq!(db.pragma_int("synchronous").unwrap(), 2);
        assert_eq!(db.pragma_int("fullfsync").unwrap(), 1);
        let text = |pragma: &str| -> String {
            db.connection().query_row(pragma, [], |r| r.get(0)).unwrap()
        };
        assert_eq!(text("PRAGMA journal_mode"), "wal");
        assert_eq!(text("PRAGMA main.locking_mode"), "exclusive");
        assert_eq!(text("PRAGMA cipher_log_level"), "NONE");
        assert_eq!(db.cipher_provider().unwrap(), "libtomcrypt");
        assert!(
            text("PRAGMA cipher_version").starts_with("4.20.0"),
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
            ("libtomcrypt", "4.20.0 community", true, None),
            ("openssl", "4.20.0 community", true, Some("cipher_provider")),
            (
                "libtomcrypt",
                "4.19.0 community",
                true,
                Some("cipher_version"),
            ),
            (
                "libtomcrypt",
                "4.6.1 community",
                true,
                Some("cipher_version"),
            ),
            (
                "libtomcrypt",
                "4.20.01 community",
                true,
                Some("cipher_version"),
            ),
            (
                "libtomcrypt",
                "4.20.0 community",
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
    fn writes_and_reads_a_segment() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_store(dir.path());
        let (seg_ref, key_id) = store
            .write_segment(&mut std::io::Cursor::new(b"screen frame bytes"))
            .unwrap();
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
        let seg_key = segment::unwrap_segment_key(
            &store.wrapping_key().unwrap(),
            seg_ref.segment_id,
            &wrapped,
        )
        .unwrap();
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
