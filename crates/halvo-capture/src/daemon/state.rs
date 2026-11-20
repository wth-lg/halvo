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

/// The durable write of the whole file `name` in the directory `dir`: a
/// temporary name beside it, a flush that reaches the drive, a rename over
/// `name` and a flush of the directory, so that `name` holds the bytes from
/// before the write or `bytes`, never a part of them. The store's
/// durable-write helper implements it (contracts.md C3.7).
pub trait StateWrite: Send {
    fn write_whole(&self, dir: &Path, name: &OsStr, bytes: &[u8]) -> io::Result<()>;
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
/// value out of range as it was stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Known(StorageCap),
    OutOfRange(u64),
}

impl Cap {
    fn of(bytes: u64) -> Cap {
        StorageCap::from_bytes(bytes).map_or(Cap::OutOfRange(bytes), Cap::Known)
    }

    fn bytes(self) -> u64 {
        match self {
            Cap::Known(cap) => cap.bytes(),
            Cap::OutOfRange(bytes) => bytes,
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
    /// The last write failed with this kind; memory holds what the file held
    /// before it.
    WriteFailed(io::ErrorKind),
}

/// Why a change of the state was not taken. The text names the file.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("the state file {} does not load ({health:?}), so it is left as it is", .file.display())]
    LeftAsItIs { file: PathBuf, health: StateHealth },
    #[error("the state file {} was not written: {kind}", .file.display())]
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
/// takes it, so memory equals the file, or differs from it only where the
/// file did not load and memory holds the defaults, desired paused among
/// them.
pub struct StateFile {
    dir: PathBuf,
    state: DaemonState,
    loaded: Loaded,
    last_write: Option<io::ErrorKind>,
    writer: Box<dyn StateWrite>,
}

impl StateFile {
    /// Loads the state file of `dir`, a data directory that the caller holds by
    /// its instance lock. A file that loads gives its state, and a missing
    /// field or switch its default. When no entry stands at the name the
    /// defaults are written through `writer`, the one write of a load; an
    /// entry that stands is never written by a load. An entry that does not
    /// load is left as it is, and the state is the defaults, with desired
    /// paused.
    pub fn open(dir: PathBuf, writer: Box<dyn StateWrite>) -> StateFile {
        let (state, loaded, missing) = match read(&dir.join(STATE_FILE)) {
            Found::Missing => (DaemonState::default(), Loaded::Readable, true),
            Found::Unloadable => (DaemonState::default(), Loaded::Corrupt, false),
            Found::Bytes(bytes) => match parse(&bytes) {
                Ok(state) => (state, Loaded::Readable, false),
                Err(loaded) => (DaemonState::default(), loaded, false),
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
    /// takes it into memory only once the write succeeded. Every call writes,
    /// also when the change leaves the state as it was. A file that did not
    /// load is never written: the call answers [`StateError::LeftAsItIs`]. A
    /// failed write answers [`StateError::NotWritten`] and leaves memory as it
    /// was.
    pub fn update(&mut self, change: impl FnOnce(&mut DaemonState)) -> Result<(), StateError> {
        if self.loaded != Loaded::Readable {
            return Err(StateError::LeftAsItIs {
                file: self.dir.join(STATE_FILE),
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
                self.last_write = Some(error.kind());
                Err(StateError::NotWritten {
                    file: self.dir.join(STATE_FILE),
                    kind: error.kind(),
                })
            }
        }
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

    fn save(&self, state: &DaemonState) -> io::Result<()> {
        let bytes = encode(state)?;
        self.writer
            .write_whole(&self.dir, OsStr::new(STATE_FILE), &bytes)
    }
}

enum Found {
    Missing,
    Unloadable,
    Bytes(Vec<u8>),
}

fn read(path: &Path) -> Found {
    let file = match open_without_following(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Found::Missing,
        Err(_) => return Found::Unloadable,
    };
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return Found::Unloadable;
    }
    let mut bytes = Vec::new();
    match file.take(STATE_FILE_MAX_BYTES + 1).read_to_end(&mut bytes) {
        Ok(read) if read as u64 <= STATE_FILE_MAX_BYTES => Found::Bytes(bytes),
        Ok(_) | Err(_) => Found::Unloadable,
    }
}

#[cfg(target_os = "macos")]
fn open_without_following(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(target_os = "macos"))]
fn open_without_following(path: &Path) -> io::Result<std::fs::File> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    std::fs::File::open(path)
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
        fn write_whole(&self, dir: &Path, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.failing.load(Ordering::SeqCst) {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            let mut temp = name.to_os_string();
            temp.push(".test-new");
            std::fs::write(dir.join(&temp), bytes)?;
            std::fs::rename(dir.join(&temp), dir.join(name))
        }
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
        StateFile::open(dir.to_path_buf(), Box::new(writer.clone()))
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
            assert_eq!(file.state(), &DaemonState::default(), "{shown}");
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
}
