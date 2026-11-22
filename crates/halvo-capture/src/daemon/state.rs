//! The daemon's state file, daemon-state.json (contracts.md C3.7 and C13.5):
//! whether the person wants capture running or paused, the storage cap, the
//! permissions that were once granted and that were asked for, the five
//! switches and the mark that a store was made.

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use halvo_control::model::StorageCap;
use halvo_control::onboarding::Permission;
use halvo_control::paths::STATE_FILE;
use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::floor::{Switch, Switches};

/// The version of the state file that this build writes, and the highest that
/// it loads.
pub const STATE_VERSION: u64 = 1;

/// The largest state file that a load reads; a larger one does not load.
pub const STATE_FILE_MAX_BYTES: u64 = 65_536;

/// The durable write of the whole file `name` in the folder `dir`: a
/// temporary name beside it, a flush that reaches the drive, a rename over
/// `name` and a flush of the folder, so that `name` holds the bytes from
/// before the write or `bytes`, never a part of them. The store's
/// durable-write helper implements it (contracts.md C3.7).
pub trait StateWrite: Send {
    fn write_whole(&self, dir: &StateDir, name: &OsStr, bytes: &[u8]) -> io::Result<()>;
}

/// The folder that holds the state file. On macOS it is a descriptor of the
/// folder of its own, so that every open, read, rename and removal of the
/// state file goes through that folder wherever it is moved; the folder's
/// present name is read only as the store's writer records it at each write,
/// where a failure of that read fails the write, and for the words of an
/// error. The service run opens it through the instance lock's descriptor. On
/// Windows it is the folder's path.
pub struct StateDir {
    #[cfg(target_os = "macos")]
    fd: std::os::fd::OwnedFd,
    #[cfg(not(target_os = "macos"))]
    path: PathBuf,
}

impl StateDir {
    /// The folder at `path`, opened as it stands now and following no link
    /// at its last part, as the store's directory handle opens a folder.
    #[cfg(target_os = "macos")]
    pub fn open(path: &Path) -> io::Result<StateDir> {
        use std::os::unix::fs::OpenOptionsExt;
        let folder = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        Ok(StateDir { fd: folder.into() })
    }

    /// The folder at `path`.
    #[cfg(not(target_os = "macos"))]
    pub fn open(path: &Path) -> io::Result<StateDir> {
        Ok(StateDir {
            path: path.to_path_buf(),
        })
    }

    /// The folder that the descriptor `held` holds, opened anew through it:
    /// the same folder wherever it stands, on a descriptor of its own that
    /// shares no lock that `held` takes.
    #[cfg(target_os = "macos")]
    pub fn of_folder(held: std::os::fd::BorrowedFd<'_>) -> io::Result<StateDir> {
        use std::os::fd::{AsRawFd, FromRawFd};
        // SAFETY: `held` is a descriptor open for the call, the name is the NUL-terminated
        // static ".", and without O_CREAT openat reads no mode argument.
        let fd = unsafe {
            libc::openat(
                held.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if fd == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is the descriptor that the openat above returned and checked, and
        // nothing else owns it, so the OwnedFd becomes its one owner.
        Ok(StateDir {
            fd: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) },
        })
    }

    /// The folder's descriptor, for the port's write.
    #[cfg(target_os = "macos")]
    pub fn descriptor(&self) -> std::os::fd::BorrowedFd<'_> {
        use std::os::fd::AsFd;
        self.fd.as_fd()
    }

    /// The folder's path, for the port's write.
    #[cfg(not(target_os = "macos"))]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(target_os = "macos")]
    fn file_for_words(&self) -> PathBuf {
        use std::os::fd::AsRawFd;
        use std::os::unix::ffi::OsStrExt;
        let mut buf = vec![0u8; libc::PATH_MAX as usize];
        // SAFETY: F_GETPATH writes at most PATH_MAX bytes, which `buf` holds, and `self.fd`
        // is a descriptor that this StateDir owns and keeps open across the call.
        if unsafe { libc::fcntl(self.fd.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) } == -1 {
            return PathBuf::from(STATE_FILE);
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Path::new(OsStr::from_bytes(&buf[..len])).join(STATE_FILE)
    }

    #[cfg(not(target_os = "macos"))]
    fn file_for_words(&self) -> PathBuf {
        self.path.join(STATE_FILE)
    }
}

/// Whether the person wants capture running or paused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Desired {
    Running,
    #[default]
    Paused,
}

/// The storage cap that the state file holds: a cap in range, or a stored
/// value out of range, which makes the cap unknown, so that nothing is
/// evicted for it until a valid cap is set (RR-daemon-19). A write keeps a
/// value out of range as it was stored. A state file that does not load
/// gives no cap at all: the cap is unknown and nothing is evicted, since a
/// person's records are never evicted on the word of a file that could not
/// be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Known(StorageCap),
    OutOfRange(u64),
    Unread,
}

impl Cap {
    fn of(bytes: u64) -> Cap {
        StorageCap::from_bytes(bytes).map_or(Cap::OutOfRange(bytes), Cap::Known)
    }

    fn bytes(self) -> u64 {
        match self {
            Cap::Known(cap) => cap.bytes(),
            Cap::OutOfRange(bytes) => bytes,
            Cap::Unread => StorageCap::DEFAULT.bytes(),
        }
    }
}

/// The state that the state file keeps, whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonState {
    pub desired: Desired,
    pub storage_cap: Cap,
    pub permissions_once_granted: Vec<Permission>,
    pub permissions_asked: Vec<Permission>,
    pub switches: Switches,
    /// The mark that a store was made or opened under this data directory; it
    /// holds no id (contracts.md C3.7, ruling ST-34).
    pub store_made: bool,
}

impl Default for DaemonState {
    fn default() -> DaemonState {
        DaemonState {
            desired: Desired::default(),
            storage_cap: Cap::Known(StorageCap::DEFAULT),
            permissions_once_granted: Vec::new(),
            permissions_asked: Vec::new(),
            switches: Switches::default(),
            store_made: false,
        }
    }
}

/// The state of a state file that does not load: the defaults, with no cap.
fn unread() -> DaemonState {
    DaemonState {
        storage_cap: Cap::Unread,
        ..DaemonState::default()
    }
}

/// The cap that a reset of the settings stores: a number out of range, which
/// reads as a cap that is not known, so that nothing is evicted until a cap is
/// set.
pub const RESET_CAP_BYTES: u64 = 0;

/// The state that a reset of the settings writes: the defaults, desired
/// paused among them, with the cap not set, so that nothing is evicted, and
/// the mark `store_made` set, so that the next life makes no store where no
/// key file stands, as a life on the file that did not load makes none.
pub fn after_a_reset() -> DaemonState {
    DaemonState {
        storage_cap: Cap::OutOfRange(RESET_CAP_BYTES),
        store_made: true,
        ..DaemonState::default()
    }
}

/// How the state file stands: it loaded or was written, or an entry stands at
/// its name that does not load, or the last write failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateHealth {
    Ok,
    /// The entry at the name does not load: it does not parse, it is no
    /// regular file (a link among them, which is never followed), it cannot
    /// be read, or it is larger than [`STATE_FILE_MAX_BYTES`].
    Corrupt,
    /// The file names a version above [`STATE_VERSION`].
    NewerVersion(u64),
    /// The last write failed with this kind; memory holds what the file read
    /// after it.
    WriteFailed(io::ErrorKind),
}

/// Why a change of the state was not taken. The text names the file.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("the state file {} does not load ({health:?}), so it is left as it is", .file.display())]
    LeftAsItIs { file: PathBuf, health: StateHealth },
    #[error("the state file {} was not written: {kind}", .file.display())]
    NotWritten { file: PathBuf, kind: io::ErrorKind },
    /// The file holds the change, which differs from the state of before, and
    /// memory took it: the write failed after the file came to hold the
    /// change, as when the flush of its folder fails after the rename, so the
    /// change is in effect and may not outlive a power loss.
    #[error("the state file {} holds the change and memory took it, but the flush of its folder failed: {kind}", .file.display())]
    NotFlushed { file: PathBuf, kind: io::ErrorKind },
    /// The write failed with `kind`, and the file could not be read after it
    /// (`read`), so it may hold the change or the state of before; memory
    /// took desired paused.
    #[error("the state file {} could not be read after the failed write ({read}), so it may hold the change or the state of before: {kind}", .file.display())]
    MayHoldTheChange {
        file: PathBuf,
        kind: io::ErrorKind,
        read: io::ErrorKind,
    },
}

/// Why a reset of the settings wrote nothing. The text names the file.
#[derive(Debug, thiserror::Error)]
pub enum ResetRefused {
    #[error("the state file {} loads, so the settings were not reset", .file.display())]
    Loads { file: PathBuf },
    #[error("the state file {} is of version {version}, newer than this build, so the settings were not reset", .file.display())]
    Newer { file: PathBuf, version: u64 },
    #[error("the state file {} was not reset: {kind}", .file.display())]
    NotWritten { file: PathBuf, kind: io::ErrorKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Loaded {
    Readable,
    Corrupt,
    NewerVersion(u64),
}

/// The state file of one data directory and the state that it holds: the one
/// holder of the daemon's state. A change is written whole before memory
/// takes it, and after a failed write memory takes what the file reads, so
/// memory equals the file, or differs from it toward not capturing where the
/// file did not load and memory holds the defaults, desired paused among
/// them, and where a failed write left a file that is missing, does not load
/// or could not be read and memory took desired paused. One place differs
/// toward capturing: a write that grows the file past
/// [`STATE_FILE_MAX_BYTES`] succeeds and memory takes it, while the next start
/// reads the file as corrupt and paused.
pub struct StateFile {
    dir: StateDir,
    state: DaemonState,
    loaded: Loaded,
    last_write: Option<io::ErrorKind>,
    writer: Box<dyn StateWrite>,
}

/// The state file of a service run as its supervisor and the connections of
/// a life that is ending share it (ruling PL-17 (d)): [`StateFile::update`]
/// stays its one writer, and the lock lets one write in at a time.
pub type SharedStateFile = std::sync::Arc<std::sync::Mutex<StateFile>>;

impl StateFile {
    /// Loads the state file of `dir`, the folder of a data directory that the
    /// caller holds by its instance lock. A file that loads gives its state, and a missing
    /// field or switch its default. When no entry stands at the name the
    /// defaults are written through `writer`, the one write of a load; an
    /// entry that stands is never written by a load. An entry that does not
    /// load is left as it is, and the state is the defaults, with desired
    /// paused.
    pub fn open(dir: StateDir, writer: Box<dyn StateWrite>) -> StateFile {
        let (state, loaded, missing) = match read(&dir) {
            Found::Missing => (DaemonState::default(), Loaded::Readable, true),
            Found::Unloadable | Found::CannotRead(_) => (unread(), Loaded::Corrupt, false),
            Found::Bytes(bytes) => match parse(&bytes) {
                Ok(state) => (state, Loaded::Readable, false),
                Err(loaded) => (unread(), loaded, false),
            },
        };
        let mut file = StateFile {
            dir,
            state,
            loaded,
            last_write: None,
            writer,
        };
        if missing {
            if let Err(error) = file.save(&file.state) {
                file.last_write = Some(error.kind());
            }
        }
        file
    }

    pub fn state(&self) -> &DaemonState {
        &self.state
    }

    pub fn health(&self) -> StateHealth {
        match (self.loaded, self.last_write) {
            (Loaded::Corrupt, _) => StateHealth::Corrupt,
            (Loaded::NewerVersion(version), _) => StateHealth::NewerVersion(version),
            (Loaded::Readable, Some(kind)) => StateHealth::WriteFailed(kind),
            (Loaded::Readable, None) => StateHealth::Ok,
        }
    }

    /// Writes the state that `change` makes of the current one, whole, and
    /// takes it into memory once the write succeeded. Every call writes, also
    /// when the change leaves the state as it was. A file that did not load is
    /// never written: the call answers [`StateError::LeftAsItIs`]. After a
    /// failed write the file is read again, and memory takes what the next
    /// start would read: the state of before, answered
    /// [`StateError::NotWritten`]; the change, when the file holds it and it
    /// differs from the state of before, answered [`StateError::NotFlushed`];
    /// another state that loads, answered [`StateError::NotWritten`]; desired
    /// paused with the other fields kept when the file is missing or does not
    /// load, answered [`StateError::NotWritten`]; and desired paused with the
    /// other fields kept when the file could not be read, answered
    /// [`StateError::MayHoldTheChange`].
    pub fn update(&mut self, change: impl FnOnce(&mut DaemonState)) -> Result<(), StateError> {
        if self.loaded != Loaded::Readable {
            return Err(StateError::LeftAsItIs {
                file: self.dir.file_for_words(),
                health: self.health(),
            });
        }
        let mut next = self.state.clone();
        change(&mut next);
        match self.save(&next) {
            Ok(()) => {
                self.state = next;
                self.last_write = None;
                Ok(())
            }
            Err(error) => {
                let kind = error.kind();
                self.last_write = Some(kind);
                let file = self.dir.file_for_words();
                match reread(&self.dir) {
                    Reread::Loads(on_disk) if on_disk == self.state => {
                        Err(StateError::NotWritten { file, kind })
                    }
                    Reread::Loads(on_disk) if on_disk == next => {
                        self.state = next;
                        Err(StateError::NotFlushed { file, kind })
                    }
                    Reread::Loads(on_disk) => {
                        self.state = on_disk;
                        Err(StateError::NotWritten { file, kind })
                    }
                    Reread::CannotRead(read) => {
                        self.state.desired = Desired::Paused;
                        Err(StateError::MayHoldTheChange { file, kind, read })
                    }
                    Reread::DoesNotLoad | Reread::Missing => {
                        self.state.desired = Desired::Paused;
                        Err(StateError::NotWritten { file, kind })
                    }
                }
            }
        }
    }

    /// Writes the defaults over the state file, also over one that does not
    /// load, and takes them into memory once the write succeeded: the reset
    /// of a whole delete of everything, the person's own word that the file
    /// goes (contracts.md C17 item 3), so that a store is made after it. It
    /// is the one write over a file that did not load.
    pub fn reset(&mut self) -> io::Result<()> {
        let fresh = DaemonState::default();
        self.save(&fresh)?;
        self.state = fresh;
        self.loaded = Loaded::Readable;
        self.last_write = None;
        Ok(())
    }

    /// Writes [`after_a_reset`] over a state file that does not load, and over
    /// no other, and takes it into memory once the write succeeded: the
    /// person's reset of the settings alone, which keeps the store. A file
    /// that loads, also one whose last write failed, and one of a newer
    /// version are left as they are, and a failed write leaves memory as it
    /// was.
    pub fn reset_settings(&mut self) -> Result<(), ResetRefused> {
        match self.loaded {
            Loaded::Corrupt => {}
            Loaded::Readable => {
                return Err(ResetRefused::Loads {
                    file: self.dir.file_for_words(),
                })
            }
            Loaded::NewerVersion(version) => {
                return Err(ResetRefused::Newer {
                    file: self.dir.file_for_words(),
                    version,
                })
            }
        }
        let fresh = after_a_reset();
        self.save(&fresh)
            .map_err(|error| ResetRefused::NotWritten {
                file: self.dir.file_for_words(),
                kind: error.kind(),
            })?;
        self.state = fresh;
        self.loaded = Loaded::Readable;
        self.last_write = None;
        Ok(())
    }

    /// Adds to `permissions_once_granted` each permission of `granted` that it
    /// does not hold, through [`StateFile::update`]; when it holds them all,
    /// nothing is written.
    pub fn record_granted(&mut self, granted: &[Permission]) -> Result<(), StateError> {
        let new: Vec<Permission> = granted
            .iter()
            .copied()
            .filter(|permission| !self.state.permissions_once_granted.contains(permission))
            .collect();
        if new.is_empty() {
            return Ok(());
        }
        self.update(|state| {
            for permission in new {
                if !state.permissions_once_granted.contains(&permission) {
                    state.permissions_once_granted.push(permission);
                }
            }
        })
    }

    /// Adds `permission` to `permissions_asked` through
    /// [`StateFile::update`]; when a file that loaded holds it already,
    /// nothing is written, and a file that did not load answers
    /// [`StateError::LeftAsItIs`].
    pub fn record_asked(&mut self, permission: Permission) -> Result<(), StateError> {
        if self.loaded == Loaded::Readable && self.state.permissions_asked.contains(&permission) {
            return Ok(());
        }
        self.update(|state| {
            if !state.permissions_asked.contains(&permission) {
                state.permissions_asked.push(permission);
            }
        })
    }

    /// Takes `permission` out of `permissions_once_granted` through
    /// [`StateFile::update`]; when a file that loaded does not hold it, nothing
    /// is written, and a file that did not load answers
    /// [`StateError::LeftAsItIs`].
    pub fn forget_granted(&mut self, permission: Permission) -> Result<(), StateError> {
        if self.loaded == Loaded::Readable
            && !self.state.permissions_once_granted.contains(&permission)
        {
            return Ok(());
        }
        self.update(|state| {
            state
                .permissions_once_granted
                .retain(|held| *held != permission);
        })
    }

    fn save(&self, state: &DaemonState) -> io::Result<()> {
        let bytes = encode(state)?;
        self.writer
            .write_whole(&self.dir, OsStr::new(STATE_FILE), &bytes)
    }
}

enum Found {
    Missing,
    Unloadable,
    CannotRead(io::ErrorKind),
    Bytes(Vec<u8>),
}

enum Reread {
    Loads(DaemonState),
    DoesNotLoad,
    Missing,
    CannotRead(io::ErrorKind),
}

fn reread(dir: &StateDir) -> Reread {
    match read(dir) {
        Found::Bytes(bytes) => parse(&bytes).map_or(Reread::DoesNotLoad, Reread::Loads),
        Found::Unloadable => Reread::DoesNotLoad,
        Found::Missing => Reread::Missing,
        Found::CannotRead(kind) => Reread::CannotRead(kind),
    }
}

fn read(dir: &StateDir) -> Found {
    let file = match open_without_following(dir) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Found::Missing,
        Err(error) if is_a_link(&error) => return Found::Unloadable,
        Err(error) => return Found::CannotRead(error.kind()),
    };
    match file.metadata() {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Found::Unloadable,
        Err(error) => return Found::CannotRead(error.kind()),
    }
    let mut bytes = Vec::new();
    match file.take(STATE_FILE_MAX_BYTES + 1).read_to_end(&mut bytes) {
        Ok(read) if read as u64 <= STATE_FILE_MAX_BYTES => Found::Bytes(bytes),
        Ok(_) => Found::Unloadable,
        Err(error) => Found::CannotRead(error.kind()),
    }
}

#[cfg(target_os = "macos")]
fn is_a_link(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(target_os = "macos"))]
fn is_a_link(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::InvalidInput
}

#[cfg(target_os = "macos")]
fn open_without_following(dir: &StateDir) -> io::Result<std::fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = std::ffi::CString::new(STATE_FILE)?;
    // SAFETY: `dir.fd` is a descriptor that `dir` owns and keeps open for the call, `name` is
    // NUL-terminated and outlives it, and without O_CREAT openat reads no mode argument.
    let fd = unsafe {
        libc::openat(
            dir.fd.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` is the descriptor that the openat above returned and checked, and nothing
    // else owns it, so the File becomes its one owner.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(not(target_os = "macos"))]
fn open_without_following(dir: &StateDir) -> io::Result<std::fs::File> {
    let path = dir.path.join(STATE_FILE);
    let meta = std::fs::symlink_metadata(&path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    std::fs::File::open(&path)
}

fn version_default() -> u64 {
    STATE_VERSION
}

fn cap_default() -> u64 {
    StorageCap::DEFAULT.bytes()
}

fn is_unset(mark: &bool) -> bool {
    !*mark
}

fn parse(bytes: &[u8]) -> Result<DaemonState, Loaded> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Loaded::Corrupt)?;
    let fields = value.as_object().ok_or(Loaded::Corrupt)?;
    let version = match fields.get("version") {
        None => version_default(),
        Some(version) => version.as_u64().ok_or(Loaded::Corrupt)?,
    };
    if version == 0 {
        return Err(Loaded::Corrupt);
    }
    if version > STATE_VERSION {
        return Err(Loaded::NewerVersion(version));
    }
    let on_disk: OnDisk = serde_json::from_slice(bytes).map_err(|_| Loaded::Corrupt)?;
    Ok(DaemonState {
        desired: on_disk.desired,
        storage_cap: Cap::of(on_disk.storage_cap_bytes),
        permissions_once_granted: on_disk.permissions_once_granted,
        permissions_asked: on_disk.permissions_asked,
        switches: on_disk.switches.0,
        store_made: on_disk.store_made,
    })
}

fn encode(state: &DaemonState) -> io::Result<Vec<u8>> {
    let on_disk = OnDisk {
        version: STATE_VERSION,
        desired: state.desired,
        storage_cap_bytes: state.storage_cap.bytes(),
        permissions_once_granted: state.permissions_once_granted.clone(),
        permissions_asked: state.permissions_asked.clone(),
        switches: SwitchMap(state.switches),
        store_made: state.store_made,
    };
    let mut bytes = serde_json::to_vec(&on_disk).map_err(io::Error::other)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OnDisk {
    #[serde(default = "version_default")]
    version: u64,
    #[serde(default)]
    desired: Desired,
    #[serde(default = "cap_default")]
    storage_cap_bytes: u64,
    #[serde(default)]
    permissions_once_granted: Vec<Permission>,
    #[serde(default)]
    permissions_asked: Vec<Permission>,
    #[serde(default)]
    switches: SwitchMap,
    #[serde(default, skip_serializing_if = "is_unset")]
    store_made: bool,
}

#[derive(Default)]
struct SwitchMap(Switches);

impl Serialize for SwitchMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(Switch::ALL.len()))?;
        for switch in Switch::ALL {
            map.serialize_entry(switch.as_str(), &self.0.is_on(switch))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for SwitchMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Names;

        impl<'de> Visitor<'de> for Names {
            type Value = SwitchMap;

            fn expecting(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                out.write_str("a map of switch names to true or false")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<SwitchMap, A::Error> {
                let mut switches = Switches::default();
                let mut seen = Vec::new();
                while let Some((name, on)) = entries.next_entry::<String, bool>()? {
                    let switch = name
                        .parse::<Switch>()
                        .map_err(|_| serde::de::Error::custom("no switch has that name"))?;
                    if seen.contains(&switch) {
                        return Err(serde::de::Error::custom("a switch is named twice"));
                    }
                    seen.push(switch);
                    switches.set(switch, on);
                }
                Ok(SwitchMap(switches))
            }
        }

        deserializer.deserialize_map(Names)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    /// A write of the whole file by a temporary name and a rename, which fails
    /// with StorageFull while its switch is on and counts its calls.
    #[derive(Clone, Default)]
    pub(crate) struct TestWrite {
        pub(crate) failing: Arc<AtomicBool>,
        pub(crate) calls: Arc<AtomicUsize>,
    }

    impl StateWrite for TestWrite {
        fn write_whole(&self, dir: &StateDir, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.failing.load(Ordering::SeqCst) {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            let mut temp = name.to_os_string();
            temp.push(".test-new");
            put(dir, &temp, name, bytes)
        }
    }

    #[cfg(target_os = "macos")]
    fn put(dir: &StateDir, temp: &OsStr, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        let at = dir.descriptor().as_raw_fd();
        let temp = std::ffi::CString::new(temp.as_bytes())?;
        let name = std::ffi::CString::new(name.as_bytes())?;
        // SAFETY: `at` is the descriptor that `dir` owns, open across the call; `temp` is
        // NUL-terminated and outlives it, and O_CREAT reads the mode passed as an unsigned int.
        let fd = unsafe {
            libc::openat(
                at,
                temp.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
                0o600 as libc::c_uint,
            )
        };
        if fd == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is the descriptor that the openat above returned and checked; the File
        // becomes its one owner and closes it at the end of this statement.
        unsafe { std::fs::File::from_raw_fd(fd) }.write_all(bytes)?;
        // SAFETY: `at` is open across the call, and `temp` and `name` are NUL-terminated
        // strings that outlive it.
        if unsafe { libc::renameat(at, temp.as_ptr(), at, name.as_ptr()) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn put(dir: &StateDir, temp: &OsStr, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(dir.path().join(temp), bytes)?;
        std::fs::rename(dir.path().join(temp), dir.path().join(name))
    }

    pub(crate) fn test_dir() -> tempfile::TempDir {
        let target = std::env::current_exe()
            .unwrap()
            .ancestors()
            .nth(3)
            .unwrap()
            .join("tmp");
        std::fs::create_dir_all(&target).unwrap();
        tempfile::Builder::new()
            .prefix("state-tests-")
            .tempdir_in(target)
            .unwrap()
    }

    fn open(dir: &Path, writer: &TestWrite) -> StateFile {
        StateFile::open(StateDir::open(dir).unwrap(), Box::new(writer.clone()))
    }

    fn file_of(dir: &Path) -> PathBuf {
        dir.join(STATE_FILE)
    }

    fn stamp(path: &Path) -> (Vec<u8>, std::time::SystemTime) {
        (
            std::fs::read(path).unwrap(),
            std::fs::symlink_metadata(path).unwrap().modified().unwrap(),
        )
    }

    const DEFAULTS: &str = "{\"version\":1,\"desired\":\"paused\",\"storage_cap_bytes\":26843545600,\"permissions_once_granted\":[],\"permissions_asked\":[],\"switches\":{\"title_text\":true,\"key_class\":true,\"pointer_class\":true,\"screen_activity\":false,\"site_read\":false}}\n";

    #[test]
    fn a_directory_without_a_state_file_gets_the_defaults_written_once() {
        let dir = test_dir();
        let writer = TestWrite::default();
        let file = open(dir.path(), &writer);
        assert_eq!(file.health(), StateHealth::Ok);
        assert_eq!(file.state(), &DaemonState::default());
        assert_eq!(writer.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read_to_string(file_of(dir.path())).unwrap(),
            DEFAULTS
        );
        let again = open(dir.path(), &writer);
        assert_eq!(again.state(), &DaemonState::default());
        assert_eq!(writer.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_read_never_writes() {
        let dir = test_dir();
        let path = file_of(dir.path());
        std::fs::write(
            &path,
            "{\"version\":1,\"desired\":\"running\",\"permissions_asked\":[\"input_monitoring\"]}",
        )
        .unwrap();
        let before = stamp(&path);
        let writer = TestWrite::default();
        for _ in 0..100 {
            let file = open(dir.path(), &writer);
            assert_eq!(file.state().desired, Desired::Running);
            assert_eq!(file.health(), StateHealth::Ok);
        }
        assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
        assert_eq!(stamp(&path), before);
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(STATE_FILE)]);
    }

    #[test]
    fn a_newer_version_starts_paused_and_leaves_the_file_byte_identical() {
        let dir = test_dir();
        let path = file_of(dir.path());
        std::fs::write(
            &path,
            "{\"version\":99,\"desired\":\"running\",\"key_of_version_99\":7}",
        )
        .unwrap();
        let before = stamp(&path);
        let writer = TestWrite::default();
        let mut file = open(dir.path(), &writer);
        assert_eq!(file.health(), StateHealth::NewerVersion(99));
        assert_eq!(file.state().desired, Desired::Paused);
        let refused = file.update(|state| state.desired = Desired::Paused);
        let Err(error @ StateError::LeftAsItIs { .. }) = refused else {
            panic!("a newer file was not refused: {refused:?}");
        };
        assert!(error.to_string().contains(&path.display().to_string()));
        assert!(file.record_granted(&[Permission::InputMonitoring]).is_err());
        assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
        assert_eq!(stamp(&path), before);
        assert_eq!(file.state().desired, Desired::Paused);
    }

    #[test]
    fn a_file_that_does_not_load_starts_paused_and_is_left_untouched() {
        let cases: [&[u8]; 10] = [
            b"",
            b"{\"switches\":{\"title_text\":true,\"title_text\":false}}",
            b"{\"desired\":",
            b"[1]",
            b"{\"version\":0,\"desired\":\"running\"}",
            b"{\"version\":\"1\",\"desired\":\"running\"}",
            b"{\"version\":1,\"desired\":\"running\",\"unknown\":1}",
            b"{\"desired\":\"running\",\"desired\":\"running\"}",
            b"{\"desired\":\"running\",\"switches\":{\"screen_content\":true}}",
            b"{\"desired\":\"running\",\"storage_cap_bytes\":-5}",
        ];
        for bytes in cases {
            let dir = test_dir();
            let path = file_of(dir.path());
            std::fs::write(&path, bytes).unwrap();
            let before = stamp(&path);
            let writer = TestWrite::default();
            let mut file = open(dir.path(), &writer);
            let shown = String::from_utf8_lossy(bytes);
            assert_eq!(file.health(), StateHealth::Corrupt, "{shown}");
            assert_eq!(file.state(), &unread(), "{shown}");
            assert!(file.update(|_| {}).is_err(), "{shown}");
            assert_eq!(writer.calls.load(Ordering::SeqCst), 0, "{shown}");
            assert_eq!(stamp(&path), before, "{shown}");
        }
    }

    #[test]
    fn a_file_larger_than_the_bound_does_not_load() {
        let dir = test_dir();
        let path = file_of(dir.path());
        let mut bytes = b"{\"desired\":\"running\"}".to_vec();
        bytes.resize(STATE_FILE_MAX_BYTES as usize + 1, b' ');
        std::fs::write(&path, &bytes).unwrap();
        let writer = TestWrite::default();
        let file = open(dir.path(), &writer);
        assert_eq!(file.health(), StateHealth::Corrupt);
        assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
        bytes.truncate(STATE_FILE_MAX_BYTES as usize);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(open(dir.path(), &writer).state().desired, Desired::Running);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_link_at_the_name_is_not_followed_and_is_no_first_run() {
        let dir = test_dir();
        let target = dir.path().join("elsewhere.json");
        std::fs::write(&target, "{\"desired\":\"running\"}").unwrap();
        std::os::unix::fs::symlink(&target, file_of(dir.path())).unwrap();
        let dangling = test_dir();
        std::os::unix::fs::symlink(dangling.path().join("gone"), file_of(dangling.path())).unwrap();
        for dir in [dir.path(), dangling.path()] {
            let writer = TestWrite::default();
            let file = open(dir, &writer);
            assert_eq!(file.health(), StateHealth::Corrupt);
            assert_eq!(file.state().desired, Desired::Paused);
            assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
            assert!(std::fs::symlink_metadata(file_of(dir))
                .unwrap()
                .file_type()
                .is_symlink());
        }
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "{\"desired\":\"running\"}"
        );
    }

    #[test]
    fn a_file_without_desired_reads_paused() {
        let dir = test_dir();
        std::fs::write(file_of(dir.path()), "{\"version\":1}").unwrap();
        let writer = TestWrite::default();
        let file = open(dir.path(), &writer);
        assert_eq!(file.health(), StateHealth::Ok);
        assert_eq!(file.state().desired, Desired::Paused);
        assert_eq!(file.state(), &DaemonState::default());
        assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_file_without_a_version_loads_as_version_1() {
        let dir = test_dir();
        let path = file_of(dir.path());
        std::fs::write(&path, "{\"desired\":\"running\"}").unwrap();
        let writer = TestWrite::default();
        let mut file = open(dir.path(), &writer);
        assert_eq!(file.health(), StateHealth::Ok);
        assert_eq!(file.state().desired, Desired::Running);
        file.update(|_| {}).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["version"], 1);
        assert_eq!(written["desired"], "running");
    }

    #[test]
    fn a_missing_switch_keeps_its_default_and_screen_activity_is_off() {
        let dir = test_dir();
        let path = file_of(dir.path());
        std::fs::write(&path, "{\"switches\":{\"title_text\":false}}").unwrap();
        let writer = TestWrite::default();
        let switches = open(dir.path(), &writer).state().switches;
        assert!(!switches.is_on(Switch::TitleText));
        assert!(switches.is_on(Switch::KeyClass));
        assert!(switches.is_on(Switch::PointerClass));
        assert!(!switches.is_on(Switch::ScreenActivity));
        assert!(!switches.is_on(Switch::SiteRead));
        std::fs::write(&path, "{}").unwrap();
        assert_eq!(
            open(dir.path(), &writer).state().switches,
            Switches::default()
        );
        std::fs::write(&path, "{\"switches\":{\"screen_activity\":true}}").unwrap();
        assert!(open(dir.path(), &writer)
            .state()
            .switches
            .is_on(Switch::ScreenActivity));
        assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn after_a_failed_write_memory_equals_the_file_and_a_retry_writes_again() {
        let dir = test_dir();
        let path = file_of(dir.path());
        let writer = TestWrite::default();
        let mut file = open(dir.path(), &writer);
        writer.failing.store(true, Ordering::SeqCst);
        let failed = file.update(|state| state.desired = Desired::Running);
        let Err(error @ StateError::NotWritten { .. }) = failed else {
            panic!("a failed write was taken: {failed:?}");
        };
        assert!(error.to_string().contains(&path.display().to_string()));
        assert_eq!(
            file.health(),
            StateHealth::WriteFailed(io::ErrorKind::StorageFull)
        );
        assert_eq!(file.state().desired, Desired::Paused);
        assert_eq!(
            parse(&std::fs::read(&path).unwrap()).unwrap(),
            file.state().clone()
        );
        writer.failing.store(false, Ordering::SeqCst);
        file.update(|state| state.desired = Desired::Running)
            .unwrap();
        assert_eq!(file.health(), StateHealth::Ok);
        assert_eq!(file.state().desired, Desired::Running);
        assert_eq!(
            parse(&std::fs::read(&path).unwrap()).unwrap(),
            file.state().clone()
        );
        file.update(|state| state.desired = Desired::Running)
            .unwrap();
        assert_eq!(writer.calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn a_failed_first_write_keeps_the_defaults_and_names_the_kind() {
        let dir = test_dir();
        let writer = TestWrite::default();
        writer.failing.store(true, Ordering::SeqCst);
        let file = open(dir.path(), &writer);
        assert_eq!(
            file.health(),
            StateHealth::WriteFailed(io::ErrorKind::StorageFull)
        );
        assert_eq!(file.state(), &DaemonState::default());
        assert!(std::fs::symlink_metadata(file_of(dir.path())).is_err());
    }

    #[test]
    fn a_stored_cap_out_of_range_is_unknown_and_kept_as_it_was() {
        let dir = test_dir();
        let path = file_of(dir.path());
        let writer = TestWrite::default();
        for (stored, cap) in [
            (5, Cap::OutOfRange(5)),
            (
                StorageCap::MAX.bytes() + 1,
                Cap::OutOfRange(StorageCap::MAX.bytes() + 1),
            ),
            (StorageCap::MIN.bytes(), Cap::Known(StorageCap::MIN)),
        ] {
            std::fs::write(&path, format!("{{\"storage_cap_bytes\":{stored}}}")).unwrap();
            let mut file = open(dir.path(), &writer);
            assert_eq!(file.health(), StateHealth::Ok);
            assert_eq!(file.state().storage_cap, cap);
            file.update(|state| state.desired = Desired::Running)
                .unwrap();
            let written: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(written["storage_cap_bytes"], stored);
        }
    }

    #[test]
    fn a_granted_permission_is_recorded_once_and_the_mark_round_trips() {
        let dir = test_dir();
        let path = file_of(dir.path());
        std::fs::write(&path, "{\"store_made\":true}").unwrap();
        let writer = TestWrite::default();
        let mut file = open(dir.path(), &writer);
        assert!(file.state().store_made);
        file.record_granted(&[Permission::InputMonitoring]).unwrap();
        file.record_granted(&[Permission::InputMonitoring]).unwrap();
        file.record_granted(&[]).unwrap();
        assert_eq!(writer.calls.load(Ordering::SeqCst), 1);
        let reread = open(dir.path(), &writer);
        assert_eq!(
            reread.state().permissions_once_granted,
            vec![Permission::InputMonitoring]
        );
        assert!(reread.state().store_made);
        file.update(|state| state.store_made = false).unwrap();
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("store_made"));
    }

    #[cfg(target_os = "macos")]
    fn fifo_at(path: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_fifo_at_the_name_with_no_writer_loads_at_once_as_corrupt_and_is_never_written() {
        use std::os::unix::fs::OpenOptionsExt;

        let dir = test_dir();
        let path = file_of(dir.path());
        fifo_at(&path);
        let writer = TestWrite::default();
        let (sent, got) = std::sync::mpsc::channel();
        let (at, port) = (dir.path().to_path_buf(), writer.clone());
        let load = std::thread::spawn(move || {
            let file = open(&at, &port);
            let _ = sent.send((file.health(), file.state().desired));
        });
        let bound = std::time::Duration::from_secs(20);
        let Ok(answer) = got.recv_timeout(bound) else {
            let writer_end = std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path);
            let late = got.recv_timeout(bound);
            drop(writer_end);
            let _ = load.join();
            panic!("the load of a FIFO at the name did not return within 20 s; once a writer opened the FIFO it answered {late:?}");
        };
        load.join().unwrap();
        assert_eq!(answer, (StateHealth::Corrupt, Desired::Paused));
        assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
        assert!(!std::fs::symlink_metadata(&path).unwrap().is_file());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_fifo_that_holds_a_whole_state_and_has_no_writer_loads_as_corrupt() {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;

        let dir = test_dir();
        let path = file_of(dir.path());
        fifo_at(&path);
        let mut both = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        both.write_all(b"{\"desired\":\"running\"}").unwrap();
        let reader = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .unwrap();
        drop(both);
        let writer = TestWrite::default();
        let file = open(dir.path(), &writer);
        drop(reader);
        assert_eq!(
            (file.health(), file.state().desired),
            (StateHealth::Corrupt, Desired::Paused)
        );
        assert_eq!(writer.calls.load(Ordering::SeqCst), 0);
    }

    struct WritesThenFails(Vec<u8>);

    impl StateWrite for WritesThenFails {
        fn write_whole(&self, dir: &StateDir, name: &OsStr, _bytes: &[u8]) -> io::Result<()> {
            let mut temp = name.to_os_string();
            temp.push(".test-new");
            put(dir, &temp, name, &self.0)?;
            Err(io::Error::from(io::ErrorKind::StorageFull))
        }
    }

    fn failing_after(dir: &Path, left: &[u8]) -> StateFile {
        StateFile::open(
            StateDir::open(dir).unwrap(),
            Box::new(WritesThenFails(left.to_vec())),
        )
    }

    #[test]
    fn a_failed_write_that_leaves_another_state_in_the_file_leaves_memory_holding_that_state() {
        let dir = test_dir();
        drop(open(dir.path(), &TestWrite::default()));
        let mut file = failing_after(
            dir.path(),
            b"{\"version\":1,\"desired\":\"paused\",\"store_made\":true}",
        );
        let answer = file.update(|state| state.desired = Desired::Running);
        assert!(
            matches!(
                answer,
                Err(StateError::NotWritten {
                    kind: io::ErrorKind::StorageFull,
                    ..
                })
            ),
            "{answer:?}"
        );
        let next = open(dir.path(), &TestWrite::default());
        assert_eq!(file.state(), next.state());
        assert!(file.state().store_made);
        assert_eq!(
            file.health(),
            StateHealth::WriteFailed(io::ErrorKind::StorageFull)
        );
    }

    #[test]
    fn a_failed_write_that_leaves_a_file_that_does_not_load_leaves_memory_paused() {
        let dir = test_dir();
        let writer = TestWrite::default();
        let mut first = open(dir.path(), &writer);
        first
            .update(|state| state.desired = Desired::Running)
            .unwrap();
        drop(first);
        let mut file = failing_after(dir.path(), b"not a state");
        assert_eq!(file.state().desired, Desired::Running);
        let answer = file.update(|state| state.store_made = true);
        assert!(
            matches!(answer, Err(StateError::NotWritten { .. })),
            "{answer:?}"
        );
        assert_eq!(file.state().desired, Desired::Paused);
        assert!(!file.state().store_made);
        let next = open(dir.path(), &writer);
        assert_eq!(
            (next.health(), next.state().desired),
            (StateHealth::Corrupt, Desired::Paused)
        );
    }

    #[cfg(target_os = "macos")]
    pub(crate) struct ModeBack(pub(crate) PathBuf);

    #[cfg(target_os = "macos")]
    impl Drop for ModeBack {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o600));
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) struct WrittenThenUnreadable;

    #[cfg(target_os = "macos")]
    impl StateWrite for WrittenThenUnreadable {
        fn write_whole(&self, dir: &StateDir, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
            use std::os::unix::fs::PermissionsExt;
            let mut temp = name.to_os_string();
            temp.push(".test-new");
            put(dir, &temp, name, bytes)?;
            std::fs::set_permissions(dir.file_for_words(), std::fs::Permissions::from_mode(0o000))?;
            Err(io::Error::from(io::ErrorKind::StorageFull))
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn a_plain_open_is_refused_by_mode_000(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let _back = ModeBack(path.to_path_buf());
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let refused = std::fs::File::open(path).map(drop);
        assert_eq!(
            refused.as_ref().map_err(io::Error::kind),
            Err(io::ErrorKind::PermissionDenied),
            "the mode 000 does not keep this user out of the state file, so the case of a read that cannot be made is not made here: {refused:?}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_failed_write_whose_file_cannot_be_read_again_is_answered_as_one_that_may_hold_the_change()
    {
        let dir = test_dir();
        drop(open(dir.path(), &TestWrite::default()));
        a_plain_open_is_refused_by_mode_000(&file_of(dir.path()));
        let back = ModeBack(file_of(dir.path()));
        let mut file = StateFile::open(
            StateDir::open(dir.path()).unwrap(),
            Box::new(WrittenThenUnreadable),
        );
        let answer = file.update(|state| state.desired = Desired::Running);
        let words = answer
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert!(words.contains("may hold the change"), "{answer:?}");
        assert_eq!(file.state().desired, Desired::Paused);
        assert_eq!(
            file.health(),
            StateHealth::WriteFailed(io::ErrorKind::StorageFull)
        );
        drop(back);
        let next = open(dir.path(), &TestWrite::default());
        assert_eq!(
            (next.health(), next.state().desired),
            (StateHealth::Ok, Desired::Running)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_failed_write_whose_file_cannot_be_read_again_leaves_memory_paused_also_when_it_was_running(
    ) {
        let dir = test_dir();
        let writer = TestWrite::default();
        let mut first = open(dir.path(), &writer);
        first
            .update(|state| state.desired = Desired::Running)
            .unwrap();
        drop(first);
        a_plain_open_is_refused_by_mode_000(&file_of(dir.path()));
        let back = ModeBack(file_of(dir.path()));
        let mut file = StateFile::open(
            StateDir::open(dir.path()).unwrap(),
            Box::new(WrittenThenUnreadable),
        );
        assert_eq!(file.state().desired, Desired::Running);
        let answer = file.update(|state| state.store_made = true);
        assert!(
            matches!(answer, Err(StateError::MayHoldTheChange { .. })),
            "{answer:?}"
        );
        assert_eq!(file.state().desired, Desired::Paused);
        assert!(!file.state().store_made);
        drop(back);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_file_that_cannot_be_read_at_the_start_is_corrupt_and_left_as_it_is() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir();
        let writer = TestWrite::default();
        let mut first = open(dir.path(), &writer);
        first
            .update(|state| state.desired = Desired::Running)
            .unwrap();
        drop(first);
        let before = std::fs::read(file_of(dir.path())).unwrap();
        a_plain_open_is_refused_by_mode_000(&file_of(dir.path()));
        let back = ModeBack(file_of(dir.path()));
        std::fs::set_permissions(file_of(dir.path()), std::fs::Permissions::from_mode(0o000))
            .unwrap();
        let later = TestWrite::default();
        let file = open(dir.path(), &later);
        drop(back);
        assert_eq!(
            (file.health(), file.state().desired),
            (StateHealth::Corrupt, Desired::Paused)
        );
        assert_eq!(later.calls.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read(file_of(dir.path())).unwrap(), before);
    }

    #[cfg(target_os = "macos")]
    struct LinkLeft;

    #[cfg(target_os = "macos")]
    impl StateWrite for LinkLeft {
        fn write_whole(&self, dir: &StateDir, _name: &OsStr, _bytes: &[u8]) -> io::Result<()> {
            let file = dir.file_for_words();
            std::fs::remove_file(&file)?;
            std::os::unix::fs::symlink(file.with_file_name("elsewhere"), &file)?;
            Err(io::Error::from(io::ErrorKind::StorageFull))
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_failed_write_that_leaves_a_link_at_the_name_is_answered_not_written_and_memory_takes_paused(
    ) {
        let dir = test_dir();
        let writer = TestWrite::default();
        let mut first = open(dir.path(), &writer);
        first
            .update(|state| state.desired = Desired::Running)
            .unwrap();
        drop(first);
        let mut file = StateFile::open(StateDir::open(dir.path()).unwrap(), Box::new(LinkLeft));
        assert_eq!(file.state().desired, Desired::Running);
        let answer = file.update(|state| state.store_made = true);
        assert!(
            matches!(
                answer,
                Err(StateError::NotWritten {
                    kind: io::ErrorKind::StorageFull,
                    ..
                })
            ),
            "{answer:?}"
        );
        assert_eq!(file.state().desired, Desired::Paused);
        assert!(std::fs::symlink_metadata(file_of(dir.path()))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(windows)]
    struct WrittenThenHeld(Arc<std::sync::Mutex<Option<std::fs::File>>>);

    #[cfg(windows)]
    impl StateWrite for WrittenThenHeld {
        fn write_whole(&self, dir: &StateDir, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
            use std::os::windows::fs::OpenOptionsExt;
            let mut temp = name.to_os_string();
            temp.push(".test-new");
            put(dir, &temp, name, bytes)?;
            let held = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(dir.path().join(name))?;
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(held);
            Err(io::Error::from(io::ErrorKind::StorageFull))
        }
    }

    #[cfg(windows)]
    struct HandleBack(Arc<std::sync::Mutex<Option<std::fs::File>>>);

    #[cfg(windows)]
    impl Drop for HandleBack {
        fn drop(&mut self) {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_failed_write_whose_file_cannot_be_read_again_is_answered_as_one_that_may_hold_the_change()
    {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = test_dir();
        drop(open(dir.path(), &TestWrite::default()));
        let path = file_of(dir.path());
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let refused = std::fs::File::open(&path).map(drop);
        drop(held);
        assert!(
            refused.is_err(),
            "a handle with no sharing does not keep a plain open of the state file out here, so the case of a read that cannot be made is not made"
        );
        let handle = Arc::new(std::sync::Mutex::new(None));
        let back = HandleBack(handle.clone());
        let mut file = StateFile::open(
            StateDir::open(dir.path()).unwrap(),
            Box::new(WrittenThenHeld(handle)),
        );
        let answer = file.update(|state| state.desired = Desired::Running);
        let words = answer
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert!(words.contains("may hold the change"), "{answer:?}");
        assert_eq!(file.state().desired, Desired::Paused);
        assert_eq!(
            file.health(),
            StateHealth::WriteFailed(io::ErrorKind::StorageFull)
        );
        drop(back);
        let next = open(dir.path(), &TestWrite::default());
        assert_eq!(
            (next.health(), next.state().desired),
            (StateHealth::Ok, Desired::Running)
        );
    }
}
