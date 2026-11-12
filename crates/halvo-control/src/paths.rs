//! The per-user data directory, the registry of what Halvo keeps under it, and
//! the control endpoint. This is the only file that performs a per-user lookup
//! (contracts.md C3.4).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::model::APP_BUNDLE_ID;

/// Why a directory or an endpoint could not be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("the account lookup failed with code {0}")]
    Lookup(i32),
    #[error("the account gives no directory")]
    Unset,
    #[error("the account gives an empty directory")]
    Empty,
    #[error("the path is not absolute")]
    Relative,
    #[error("the path holds a parent-directory component")]
    NotNormal,
    #[error("the path is not UTF-8")]
    NotUtf8,
    #[error("the scratch directory is the data directory or lies inside it")]
    InsideDataDir,
    #[error(
        "the control socket path is 104 bytes or longer and no fallback directory can hold it"
    )]
    SocketPathTooLong,
}

/// The per-user lookups, injected so that a test can give an unset, an empty or
/// a relative answer.
pub trait AccountLookup {
    /// macOS: the home directory of the effective user from the account database
    /// (getpwuid_r). Windows: the LocalAppData known folder. `None` when the
    /// account gives none.
    fn base_dir(&self) -> Result<Option<OsString>, PathError>;

    /// macOS: the per-user temporary directory, confstr(_CS_DARWIN_USER_TEMP_DIR),
    /// only in the system's shape /var/folders/<a>/<b>/T. `None` where the system
    /// has none or the answer has another shape.
    fn user_temp_dir(&self) -> Result<Option<PathBuf>, PathError>;
}

/// The lookups of the account this process runs as. No environment variable
/// moves them.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemAccount;

impl AccountLookup for SystemAccount {
    fn base_dir(&self) -> Result<Option<OsString>, PathError> {
        system::base_dir()
    }

    fn user_temp_dir(&self) -> Result<Option<PathBuf>, PathError> {
        system::user_temp_dir()
    }
}

/// The components of the macOS data directory under the home directory.
pub const MACOS_DATA_DIR: [&str; 3] = ["Library", "Application Support", "Halvo"];
/// The components of the Windows data directory under LocalAppData (rulings B5).
pub const WINDOWS_DATA_DIR: [&str; 2] = [APP_BUNDLE_ID, "data"];
/// The components of the LaunchAgents directory under the home directory.
pub const MACOS_LAUNCH_AGENTS_DIR: [&str; 2] = ["Library", "LaunchAgents"];

#[cfg(windows)]
const DATA_DIR: &[&str] = &WINDOWS_DATA_DIR;
#[cfg(not(windows))]
const DATA_DIR: &[&str] = &MACOS_DATA_DIR;

/// The state file of contracts.md C3.7.
pub const STATE_FILE: &str = "daemon-state.json";
/// The delete-all intent of contracts.md C17.3.
pub const INTENT_FILE: &str = "delete-all.intent";
/// The instance lock file, Windows only (contracts.md C3.5).
pub const INSTANCE_LOCK_FILE: &str = "daemon.lock";
/// The control socket under run/.
pub const CONTROL_SOCKET: &str = "control.sock";
/// The run marker under run/.
pub const RUN_MARKER: &str = "running";
/// The daemon's error log under logs/.
pub const DAEMON_ERR: &str = "daemon.err";

/// What an entry of the data directory holds; the daemon decides how delete-all
/// handles each kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Captured content, readable only through its key files.
    Content,
    /// The custody root files.
    Custody,
    /// Files of the running daemon: the control socket and the run marker.
    Runtime,
    /// The daemon's log, which holds neither captured nor derived content.
    Log,
}

/// One registered entry of the data directory (contracts.md C17.2). Only the
/// rows of this file exist; no other crate can build one:
///
/// ```compile_fail
/// use halvo_control::paths::{Entry, EntryKind};
/// let outside = Entry { name: "..", kind: EntryKind::Log, key_files: &[], owner_pid_file: None };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    name: &'static str,
    kind: EntryKind,
    key_files: &'static [&'static str],
    owner_pid_file: Option<&'static str>,
}

impl Entry {
    /// One path component under the data directory.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn kind(&self) -> EntryKind {
        self.kind
    }

    /// The key files, relative to the data directory and inside the entry.
    pub const fn key_files(&self) -> &'static [&'static str] {
        self.key_files
    }

    /// The file that names the process that owns the entry, relative to the data
    /// directory and inside the entry.
    pub const fn owner_pid_file(&self) -> Option<&'static str> {
        self.owner_pid_file
    }
}

pub const STORE: Entry = Entry {
    name: "store",
    kind: EntryKind::Content,
    key_files: &["store/store.key"],
    owner_pid_file: None,
};
pub const ROOTS: Entry = Entry {
    name: "roots",
    kind: EntryKind::Custody,
    key_files: &[],
    owner_pid_file: None,
};
pub const RUN: Entry = Entry {
    name: "run",
    kind: EntryKind::Runtime,
    key_files: &[],
    owner_pid_file: None,
};
pub const LOGS: Entry = Entry {
    name: "logs",
    kind: EntryKind::Log,
    key_files: &[],
    owner_pid_file: None,
};

/// Every registered entry, in the table order of contracts.md C17.2. Delete-all
/// removes these and no other entry.
pub const ENTRIES: [Entry; 4] = [STORE, ROOTS, RUN, LOGS];

/// Names reserved for later runs (contracts.md C17.2); R1 registers and creates
/// neither.
pub const RESERVED_ENTRY_NAMES: [&str; 2] = ["derived", "agents"];

/// Who removes a keychain item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovedBy {
    DeleteAll,
    RemoveHalvo,
}

/// One keychain item that Halvo creates (contracts.md C17.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeychainItem {
    pub service: &'static str,
    pub account_form: &'static str,
    pub owner: &'static str,
    pub removed_by: RemovedBy,
}

/// Every keychain item. R1 creates none (RD-24, RD-42).
pub const KEYCHAIN_ITEMS: [KeychainItem; 0] = [];

/// The per-user data directory: absolute, UTF-8 and with no parent-directory
/// component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir(PathBuf);

impl DataDir {
    /// The default directory of the account this process runs as (contracts.md
    /// C3.4).
    pub fn for_current_user() -> Result<Self, PathError> {
        Self::for_account(&SystemAccount)
    }

    /// The default directory under `account`'s base directory.
    pub fn for_account(account: &dyn AccountLookup) -> Result<Self, PathError> {
        let mut path = base_dir(account)?;
        path.extend(DATA_DIR);
        Self::from_flag(path)
    }

    /// The directory a `--data-dir` flag names: absolute or an error.
    pub fn from_flag(path: impl Into<PathBuf>) -> Result<Self, PathError> {
        checked(path.into()).map(Self)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Whether a daemon has provisioned this directory: its state file exists
    /// (contracts.md C10.7).
    pub fn is_provisioned(&self) -> bool {
        self.state_file().exists()
    }

    /// The directory of a registered entry.
    pub fn entry(&self, entry: &Entry) -> PathBuf {
        self.0.join(entry.name)
    }

    pub fn store_dir(&self) -> PathBuf {
        self.entry(&STORE)
    }

    /// The root files of the custody (RD-24), on both systems.
    pub fn roots_dir(&self) -> PathBuf {
        self.entry(&ROOTS)
    }

    pub fn run_dir(&self) -> PathBuf {
        self.entry(&RUN)
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.entry(&LOGS)
    }

    pub fn state_file(&self) -> PathBuf {
        self.0.join(STATE_FILE)
    }

    pub fn intent_file(&self) -> PathBuf {
        self.0.join(INTENT_FILE)
    }

    pub fn instance_lock_file(&self) -> PathBuf {
        self.0.join(INSTANCE_LOCK_FILE)
    }

    pub fn control_socket(&self) -> PathBuf {
        self.run_dir().join(CONTROL_SOCKET)
    }

    pub fn run_marker(&self) -> PathBuf {
        self.run_dir().join(RUN_MARKER)
    }

    pub fn daemon_err(&self) -> PathBuf {
        self.logs_dir().join(DAEMON_ERR)
    }

    fn text(&self) -> &str {
        self.0.to_str().unwrap_or_default()
    }
}

/// A scratch directory of a debug-only mode: absolute, and neither the user's
/// data directory nor inside it (contracts.md C4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    pub fn new(path: impl Into<PathBuf>, user_data_dir: &DataDir) -> Result<Self, PathError> {
        let path = checked(path.into())?;
        let data = user_data_dir.path();
        if same_or_inside(&path, data)
            || same_or_inside(&resolved(&path), &resolved(data))
            || inside_by_identity(&path, data)
        {
            return Err(PathError::InsideDataDir);
        }
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// The LaunchAgents directory of an account (macOS).
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchAgentsDir(PathBuf);

#[cfg(target_os = "macos")]
impl LaunchAgentsDir {
    pub fn for_current_user() -> Result<Self, PathError> {
        Self::for_account(&SystemAccount)
    }

    pub fn for_account(account: &dyn AccountLookup) -> Result<Self, PathError> {
        let mut path = base_dir(account)?;
        path.extend(MACOS_LAUNCH_AGENTS_DIR);
        checked(path).map(Self)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

fn base_dir(account: &dyn AccountLookup) -> Result<PathBuf, PathError> {
    let base = account.base_dir()?.ok_or(PathError::Unset)?;
    if base.is_empty() {
        return Err(PathError::Empty);
    }
    checked(PathBuf::from(base))
}

fn checked(path: PathBuf) -> Result<PathBuf, PathError> {
    if path.to_str().is_none() {
        return Err(PathError::NotUtf8);
    }
    if !path.is_absolute() {
        return Err(PathError::Relative);
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(PathError::NotNormal);
    }
    Ok(path.components().collect())
}

fn same_or_inside(path: &Path, dir: &Path) -> bool {
    let mut path = path.components();
    dir.components().all(|d| match path.next() {
        Some(p) => p
            .as_os_str()
            .to_string_lossy()
            .to_lowercase()
            .eq(&d.as_os_str().to_string_lossy().to_lowercase()),
        None => false,
    })
}

#[cfg(unix)]
fn inside_by_identity(path: &Path, dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Some((existing, rest)) = deepest_existing(dir) else {
        return false;
    };
    let Ok(target) = std::fs::metadata(existing) else {
        return false;
    };
    path.ancestors().any(|ancestor| {
        std::fs::metadata(ancestor)
            .is_ok_and(|meta| meta.dev() == target.dev() && meta.ino() == target.ino())
            && path
                .strip_prefix(ancestor)
                .is_ok_and(|tail| same_or_inside(tail, &rest))
    })
}

#[cfg(not(unix))]
fn inside_by_identity(_path: &Path, _dir: &Path) -> bool {
    false
}

#[cfg(unix)]
fn deepest_existing(dir: &Path) -> Option<(&Path, PathBuf)> {
    let mut rest = Vec::new();
    let mut existing = dir;
    while !existing.exists() {
        rest.push(existing.file_name()?);
        existing = existing.parent()?;
    }
    Some((existing, rest.into_iter().rev().collect()))
}

fn resolved(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            return rest
                .into_iter()
                .rev()
                .fold(real, |acc: PathBuf, c| acc.join(c));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// sun_path holds 104 bytes on macOS; a socket path this long or longer moves to
/// the fallback (contracts.md C3.5).
pub const SOCKET_PATH_MAX_BYTES: usize = 104;

/// The directory under the user temp dir that holds the fallback sockets.
pub const FALLBACK_SOCKET_DIR: &str = "com.halvo.capture";

/// The endpoint hash of contracts.md C3.5: FNV-1a, 64 bit, over the UTF-8 bytes
/// of the path, as 16 lowercase hex digits.
pub fn endpoint_hash(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The Windows pipe of the control channel for the user with SID text `sid` and
/// the data directory `data_dir` (contracts.md C15.1). The hash is over the final
/// path in lower case: on Windows the path that the system resolves for the
/// directory when it exists, else the path as given.
pub fn pipe_name(sid: &str, data_dir: &DataDir) -> String {
    let hash = endpoint_hash(&final_path(data_dir).to_lowercase());
    format!(r"\\.\pipe\halvo-control-{sid}-{hash}")
}

#[cfg(windows)]
fn final_path(data_dir: &DataDir) -> String {
    data_dir
        .path()
        .canonicalize()
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .unwrap_or_else(|| data_dir.text().to_string())
}

#[cfg(not(windows))]
fn final_path(data_dir: &DataDir) -> String {
    data_dir.text().to_string()
}

/// The unix socket of the control channel for `data_dir`: run/control.sock, or,
/// when that path is 104 bytes or longer, the fallback in the user temp dir,
/// which must be owned by this user with mode 0700 (contracts.md C3.5).
#[cfg(target_os = "macos")]
pub fn socket_path(data_dir: &DataDir, account: &dyn AccountLookup) -> Result<PathBuf, PathError> {
    use std::os::unix::fs::MetadataExt;

    let primary = data_dir.control_socket();
    if primary.as_os_str().len() < SOCKET_PATH_MAX_BYTES {
        return Ok(primary);
    }
    let temp = account
        .user_temp_dir()?
        .ok_or(PathError::SocketPathTooLong)?;
    let temp = checked(temp).map_err(|_| PathError::SocketPathTooLong)?;
    let meta = std::fs::metadata(&temp).map_err(|_| PathError::SocketPathTooLong)?;
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
        return Err(PathError::SocketPathTooLong);
    }
    let fallback = temp
        .join(FALLBACK_SOCKET_DIR)
        .join(endpoint_hash(data_dir.text()))
        .join(CONTROL_SOCKET);
    if fallback.as_os_str().len() >= SOCKET_PATH_MAX_BYTES {
        return Err(PathError::SocketPathTooLong);
    }
    Ok(fallback)
}

/// Whether `path` has the shape of the per-user temporary directory that the
/// system gives, /var/folders/<a>/<b>/T, so that a suffix that an environment
/// variable adds is refused.
#[cfg(target_os = "macos")]
fn is_darwin_user_temp_dir(path: &Path) -> bool {
    let names: Vec<&std::ffi::OsStr> = path
        .components()
        .map(|component| component.as_os_str())
        .collect();
    let names: Vec<&str> = names.iter().filter_map(|name| name.to_str()).collect();
    match names.as_slice() {
        ["/", "var", "folders", a, b, "T"] | ["/", "private", "var", "folders", a, b, "T"] => {
            !a.is_empty() && !b.is_empty()
        }
        _ => false,
    }
}

#[cfg(target_os = "macos")]
mod system {
    use std::ffi::{CStr, OsString};
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    use super::PathError;

    const MAX_BUFFER: usize = 1 << 20;

    pub fn base_dir() -> Result<Option<OsString>, PathError> {
        let uid = unsafe { libc::geteuid() };
        let mut buf = vec![0u8; 1024];
        loop {
            let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
            let mut found: *mut libc::passwd = std::ptr::null_mut();
            let rc = unsafe {
                libc::getpwuid_r(
                    uid,
                    &mut pwd,
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                    &mut found,
                )
            };
            if rc == libc::ERANGE && buf.len() < MAX_BUFFER {
                buf.resize(buf.len() * 2, 0);
                continue;
            }
            if rc != 0 {
                return Err(PathError::Lookup(rc));
            }
            if found.is_null() || pwd.pw_dir.is_null() {
                return Ok(None);
            }
            let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
            return Ok(Some(OsString::from_vec(dir.to_bytes().to_vec())));
        }
    }

    pub fn user_temp_dir() -> Result<Option<PathBuf>, PathError> {
        let name = libc::_CS_DARWIN_USER_TEMP_DIR;
        let len = unsafe { libc::confstr(name, std::ptr::null_mut(), 0) };
        if len == 0 {
            return Ok(None);
        }
        let mut buf = vec![0u8; len];
        let got = unsafe { libc::confstr(name, buf.as_mut_ptr().cast(), buf.len()) };
        if got == 0 || got > buf.len() {
            return Ok(None);
        }
        let text = CStr::from_bytes_until_nul(&buf).map_err(|_| PathError::NotUtf8)?;
        let path = PathBuf::from(OsString::from_vec(text.to_bytes().to_vec()));
        Ok(super::is_darwin_user_temp_dir(&path).then_some(path))
    }
}

#[cfg(windows)]
mod system {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    use super::PathError;

    pub fn base_dir() -> Result<Option<OsString>, PathError> {
        let path = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }
            .map_err(|e| PathError::Lookup(e.code().0))?;
        let text = OsString::from_wide(unsafe { path.as_wide() });
        unsafe { CoTaskMemFree(Some(path.0 as *const _)) };
        Ok(Some(text))
    }

    pub fn user_temp_dir() -> Result<Option<PathBuf>, PathError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct Account {
        base: Option<&'static str>,
        temp: Option<PathBuf>,
    }

    impl AccountLookup for Account {
        fn base_dir(&self) -> Result<Option<OsString>, PathError> {
            Ok(self.base.map(OsString::from))
        }

        fn user_temp_dir(&self) -> Result<Option<PathBuf>, PathError> {
            Ok(self.temp.clone())
        }
    }

    fn account(base: &'static str) -> Account {
        Account {
            base: Some(base),
            temp: None,
        }
    }

    #[cfg(windows)]
    const BASE: &str = r"C:\Users\a b\AppData\Local";
    #[cfg(not(windows))]
    const BASE: &str = "/Users/a b";

    #[cfg(windows)]
    const RELATIVE: [&str; 5] = ["Users", r".\Users", r"\Users\a", "C:Users", "."];
    #[cfg(not(windows))]
    const RELATIVE: [&str; 4] = ["Users/a", "./Users/a", "a", "."];

    #[cfg(windows)]
    fn absolute(name: &str) -> PathBuf {
        PathBuf::from(format!(r"C:\{name}"))
    }
    #[cfg(not(windows))]
    fn absolute(name: &str) -> PathBuf {
        PathBuf::from(format!("/{name}"))
    }

    #[test]
    fn an_unset_empty_or_relative_account_directory_is_an_error() {
        let unset = Account {
            base: None,
            temp: None,
        };
        assert_eq!(DataDir::for_account(&unset), Err(PathError::Unset));
        assert_eq!(DataDir::for_account(&account("")), Err(PathError::Empty));
        for relative in RELATIVE {
            assert_eq!(
                DataDir::for_account(&account(relative)),
                Err(PathError::Relative),
                "{relative}"
            );
        }
        let parent = format!("{BASE}{}..", std::path::MAIN_SEPARATOR);
        let parent: &'static str = Box::leak(parent.into_boxed_str());
        assert_eq!(
            DataDir::for_account(&account(parent)),
            Err(PathError::NotNormal)
        );
    }

    #[test]
    fn the_default_directory_lies_under_the_account_directory() {
        let dir = DataDir::for_account(&account(BASE)).unwrap();
        #[cfg(windows)]
        assert_eq!(
            dir.path(),
            Path::new(r"C:\Users\a b\AppData\Local\com.halvo.desktop\data")
        );
        #[cfg(not(windows))]
        assert_eq!(
            dir.path(),
            Path::new("/Users/a b/Library/Application Support/Halvo")
        );
        assert_eq!(dir.roots_dir(), dir.path().join("roots"));
    }

    #[test]
    fn the_types_refuse_a_relative_empty_or_unnormal_directory() {
        let data = DataDir::from_flag(absolute("data")).unwrap();
        let mut refused: Vec<PathBuf> = RELATIVE.iter().map(PathBuf::from).collect();
        refused.push(PathBuf::new());
        refused.push(absolute("a").join("..").join("b"));
        for path in refused {
            assert!(DataDir::from_flag(&path).is_err(), "{}", path.display());
            assert!(ScratchDir::new(&path, &data).is_err(), "{}", path.display());
        }
        let with_slash = absolute("a").join("b").join("");
        assert_eq!(
            DataDir::from_flag(with_slash).unwrap().path(),
            absolute("a").join("b")
        );
    }

    fn lower(components: &[&str]) -> Vec<String> {
        components.iter().map(|c| c.to_lowercase()).collect()
    }

    fn assert_disjoint(data: &[String], install: &[String]) {
        assert_ne!(data, install, "the data directory is the install directory");
        assert!(
            !data.starts_with(install),
            "the data directory lies inside the install directory"
        );
        assert!(
            !install.starts_with(data),
            "the data directory is an ancestor of the install directory"
        );
    }

    #[test]
    fn the_windows_data_directory_is_not_the_install_directory_not_inside_it_and_not_above_it() {
        let base = ["C:", "Users", "a b", "AppData", "Local"];
        let data = lower(&[&base[..], &WINDOWS_DATA_DIR[..]].concat());
        let install = lower(&[&base[..], &["Halvo"][..]].concat());
        assert_disjoint(&data, &install);
        assert!(!data.contains(&"roaming".to_string()));
    }

    #[test]
    fn the_macos_data_directory_is_not_an_install_directory_not_inside_one_and_not_above_one() {
        let home = ["", "Users", "a b"];
        let data = lower(&[&home[..], &MACOS_DATA_DIR[..]].concat());
        for install in [
            lower(&["", "Applications", "Halvo.app"]),
            lower(&[&home[..], &["Applications", "Halvo.app"][..]].concat()),
        ] {
            assert_disjoint(&data, &install);
        }
    }

    #[test]
    fn the_resolved_directory_is_disjoint_from_the_install_directories_of_this_system() {
        let data = DataDir::for_account(&account(BASE)).unwrap();
        #[cfg(windows)]
        let installs = [PathBuf::from(BASE).join("Halvo")];
        #[cfg(not(windows))]
        let installs = [
            PathBuf::from("/Applications/Halvo.app"),
            PathBuf::from(BASE).join("Applications").join("Halvo.app"),
        ];
        for install in installs {
            assert_ne!(data.path(), install);
            assert!(!data.path().starts_with(&install));
            assert!(!install.starts_with(data.path()));
        }
    }

    #[test]
    #[ignore = "a child of the_data_dir_comes_from_the_account_not_from_the_environment"]
    fn print_the_data_dir_of_this_process() {
        println!(
            "data_dir={}",
            DataDir::for_current_user().unwrap().path().display()
        );
    }

    #[test]
    fn the_data_dir_comes_from_the_account_not_from_the_environment() {
        let mine = DataDir::for_current_user().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "paths::tests::print_the_data_dir_of_this_process",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HOME", elsewhere.path())
            .env("LOCALAPPDATA", elsewhere.path())
            .env("APPDATA", elsewhere.path())
            .env("USERPROFILE", elsewhere.path())
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        let line = text
            .lines()
            .find_map(|line| line.split_once("data_dir=").map(|(_, dir)| dir))
            .unwrap_or_else(|| panic!("no data_dir line in {text}"));
        assert_eq!(Path::new(line), mine.path());
        assert!(!Path::new(line).starts_with(elsewhere.path()));
    }

    #[test]
    fn is_provisioned_is_true_once_the_state_file_exists() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::from_flag(root.path()).unwrap();
        assert_eq!(data.state_file(), root.path().join("daemon-state.json"));
        assert!(!data.is_provisioned());
        std::fs::write(data.state_file(), "{}").unwrap();
        assert!(data.is_provisioned());
    }

    #[test]
    fn a_scratch_directory_is_never_the_data_directory_or_inside_it() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::from_flag(root.path().join("Halvo")).unwrap();
        std::fs::create_dir_all(data.path()).unwrap();
        let upper = PathBuf::from(data.path().to_str().unwrap().to_uppercase());
        for inside in [
            data.path().to_path_buf(),
            data.path().join("store"),
            data.path().join("a").join("b"),
            upper.join("store"),
        ] {
            assert_eq!(
                ScratchDir::new(&inside, &data),
                Err(PathError::InsideDataDir),
                "{}",
                inside.display()
            );
        }
        for outside in [
            root.path().join("Halvo-scratch"),
            root.path().join("scratch"),
        ] {
            assert_eq!(
                ScratchDir::new(&outside, &data).unwrap().path(),
                outside.as_path()
            );
        }
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(data.path(), &link).unwrap();
            assert_eq!(
                ScratchDir::new(link.join("store"), &data),
                Err(PathError::InsideDataDir)
            );
        }
    }

    #[test]
    fn the_registry_holds_the_four_r1_entries_in_table_order() {
        assert_eq!(
            ENTRIES.map(|entry| entry.name),
            ["store", "roots", "run", "logs"]
        );
        assert_eq!(
            ENTRIES.map(|entry| entry.kind),
            [
                EntryKind::Content,
                EntryKind::Custody,
                EntryKind::Runtime,
                EntryKind::Log
            ]
        );
        assert_eq!(STORE.key_files, ["store/store.key"]);
        assert!(ENTRIES[1..].iter().all(|entry| entry.key_files.is_empty()));
        assert!(ENTRIES.iter().all(|entry| entry.owner_pid_file.is_none()));
        assert_eq!(RESERVED_ENTRY_NAMES, ["derived", "agents"]);
        assert!(KEYCHAIN_ITEMS.is_empty());
    }

    #[test]
    fn every_entry_name_is_one_unique_path_component_and_no_reserved_or_file_name() {
        let files = [STATE_FILE, INTENT_FILE, INSTANCE_LOCK_FILE];
        for (at, entry) in ENTRIES.iter().enumerate() {
            let mut components = Path::new(entry.name).components();
            assert!(matches!(components.next(), Some(Component::Normal(_))));
            assert!(components.next().is_none(), "{}", entry.name);
            assert!(!entry.name.contains(['/', '\\']), "{}", entry.name);
            assert!(!RESERVED_ENTRY_NAMES.contains(&entry.name));
            assert!(!files.contains(&entry.name));
            assert!(ENTRIES[at + 1..]
                .iter()
                .all(|other| !other.name.eq_ignore_ascii_case(entry.name)));
        }
    }

    #[test]
    fn key_files_and_pid_files_are_relative_and_stay_inside_their_entry() {
        for entry in ENTRIES {
            for file in entry.key_files.iter().chain(entry.owner_pid_file.iter()) {
                let path = Path::new(file);
                assert!(path.is_relative(), "{file}");
                assert!(path
                    .components()
                    .all(|component| matches!(component, Component::Normal(_))));
                assert!(path.starts_with(entry.name), "{file}");
                assert_ne!(path, Path::new(entry.name));
            }
        }
    }

    #[test]
    fn every_layout_accessor_lies_in_a_registered_entry_or_is_a_top_level_file() {
        let data = DataDir::from_flag(absolute("data")).unwrap();
        for path in [
            data.store_dir(),
            data.roots_dir(),
            data.run_dir(),
            data.logs_dir(),
            data.control_socket(),
            data.run_marker(),
            data.daemon_err(),
        ] {
            let inside = path.strip_prefix(data.path()).unwrap();
            let first = inside.components().next().unwrap();
            assert!(
                ENTRIES
                    .iter()
                    .any(|entry| Path::new(entry.name) == Path::new(first.as_os_str())),
                "{}",
                path.display()
            );
        }
        for path in [
            data.state_file(),
            data.intent_file(),
            data.instance_lock_file(),
        ] {
            assert_eq!(path.parent(), Some(data.path()));
        }
        assert_eq!(data.entry(&STORE), data.path().join("store"));
        assert_eq!(data.run_marker(), data.path().join("run").join("running"));
        assert_eq!(
            data.daemon_err(),
            data.path().join("logs").join("daemon.err")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_scratch_directory_through_the_data_firmlink_is_still_the_data_directory() {
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let real = root.path().canonicalize().unwrap();
        let data = DataDir::from_flag(real.join("Halvo")).unwrap();
        std::fs::create_dir_all(data.path()).unwrap();
        let firm = Path::new("/System/Volumes/Data").join(data.path().strip_prefix("/").unwrap());
        let (a, b) = (
            std::fs::metadata(data.path()).unwrap(),
            std::fs::metadata(&firm).unwrap(),
        );
        assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()));
        for inside in [
            firm.clone(),
            firm.join("store"),
            firm.join("not-yet").join("x"),
        ] {
            assert_eq!(
                ScratchDir::new(&inside, &data),
                Err(PathError::InsideDataDir),
                "{}",
                inside.display()
            );
        }
        let missing = DataDir::from_flag(real.join("Later")).unwrap();
        let firm_missing =
            Path::new("/System/Volumes/Data").join(missing.path().strip_prefix("/").unwrap());
        assert_eq!(
            ScratchDir::new(firm_missing.join("store"), &missing),
            Err(PathError::InsideDataDir)
        );
        assert!(ScratchDir::new(firm.with_file_name("Halvo-scratch"), &data).is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn only_the_system_shape_of_the_user_temp_dir_is_taken() {
        for good in [
            "/var/folders/7n/abc_xyz/T/",
            "/private/var/folders/7n/abc/T",
        ] {
            assert!(is_darwin_user_temp_dir(Path::new(good)), "{good}");
        }
        for bad in [
            "/var/folders/7n/abc/T/zz/",
            "/var/folders/7n/abc/T/r1/T/",
            "/var/folders/7n/T/",
            "/tmp/",
            "/Users/a/T",
            "var/folders/7n/abc/T",
        ] {
            assert!(!is_darwin_user_temp_dir(Path::new(bad)), "{bad}");
        }
        let temp = SystemAccount.user_temp_dir().unwrap().unwrap();
        assert!(is_darwin_user_temp_dir(&temp), "{}", temp.display());
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_the_pipe_hash_is_over_the_final_path() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("Data");
        std::fs::create_dir(&real).unwrap();
        let link = root.path().join("Link");
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();
        let sid = "S-1-5-21-1-2-3-1001";
        let by_real = pipe_name(sid, &DataDir::from_flag(&real).unwrap());
        let by_link = pipe_name(sid, &DataDir::from_flag(&link).unwrap());
        let by_case = pipe_name(
            sid,
            &DataDir::from_flag(real.to_str().unwrap().to_uppercase()).unwrap(),
        );
        assert_eq!(by_real, by_link);
        assert_eq!(by_real, by_case);
        let final_path = real.canonicalize().unwrap();
        let hash = endpoint_hash(&final_path.to_str().unwrap().to_lowercase());
        assert_eq!(by_real, format!(r"\\.\pipe\halvo-control-{sid}-{hash}"));
    }

    #[test]
    fn the_endpoint_hash_is_fnv_1a_64_in_16_lowercase_hex_digits() {
        assert_eq!(endpoint_hash(""), "cbf29ce484222325");
        assert_eq!(endpoint_hash("a"), "af63dc4c8601ec8c");
        assert_eq!(endpoint_hash("foobar"), "85944171f73967e8");
    }

    #[test]
    fn two_data_directories_give_two_pipe_names_for_one_sid() {
        let sid = "S-1-5-21-1004336348-1177238915-682003330-1001";
        let a = DataDir::from_flag(absolute("Data A")).unwrap();
        let b = DataDir::from_flag(absolute("Data B")).unwrap();
        assert_ne!(pipe_name(sid, &a), pipe_name(sid, &b));
        let hash = endpoint_hash(&a.path().to_str().unwrap().to_lowercase());
        assert_eq!(
            pipe_name(sid, &a),
            format!(r"\\.\pipe\halvo-control-{sid}-{hash}")
        );
        let a_upper = DataDir::from_flag(absolute("DATA A")).unwrap();
        assert_eq!(pipe_name(sid, &a), pipe_name(sid, &a_upper));
        assert_ne!(pipe_name(sid, &a), pipe_name("S-1-5-21-1-2-3-1002", &a));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_short_data_directory_keeps_its_socket_under_run() {
        let data = DataDir::from_flag("/Users/a b/Library/Application Support/Halvo").unwrap();
        assert_eq!(
            socket_path(&data, &SystemAccount).unwrap(),
            data.path().join("run").join("control.sock")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn two_long_data_directories_get_two_fallback_sockets_under_104_bytes() {
        let long = |c: &str| DataDir::from_flag(format!("/{}", c.repeat(120))).unwrap();
        let (a, b) = (long("a"), long("b"));
        let temp = SystemAccount.user_temp_dir().unwrap().unwrap();
        let socket_a = socket_path(&a, &SystemAccount).unwrap();
        let socket_b = socket_path(&b, &SystemAccount).unwrap();
        assert_ne!(socket_a, socket_b);
        for (data, socket) in [(&a, &socket_a), (&b, &socket_b)] {
            assert!(socket.as_os_str().len() < SOCKET_PATH_MAX_BYTES);
            assert_eq!(
                *socket,
                temp.join("com.halvo.capture")
                    .join(endpoint_hash(data.path().to_str().unwrap()))
                    .join("control.sock")
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_long_data_directory_without_a_private_user_temp_dir_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let long = DataDir::from_flag(format!("/{}", "a".repeat(120))).unwrap();
        let open = tempfile::tempdir().unwrap();
        std::fs::set_permissions(open.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        for temp in [
            None,
            Some(open.path().to_path_buf()),
            Some(open.path().join("missing")),
            Some(PathBuf::from("/")),
            Some(PathBuf::from("relative")),
        ] {
            let account = Account { base: None, temp };
            assert_eq!(
                socket_path(&long, &account),
                Err(PathError::SocketPathTooLong)
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_launch_agents_directory_comes_from_the_account() {
        assert_eq!(
            LaunchAgentsDir::for_account(&account(BASE)).unwrap().path(),
            Path::new("/Users/a b/Library/LaunchAgents")
        );
        assert_eq!(
            LaunchAgentsDir::for_account(&account("")),
            Err(PathError::Empty)
        );
        assert_eq!(
            LaunchAgentsDir::for_account(&account("Users/a")),
            Err(PathError::Relative)
        );
        let unset = Account {
            base: None,
            temp: None,
        };
        assert_eq!(LaunchAgentsDir::for_account(&unset), Err(PathError::Unset));
    }
}
