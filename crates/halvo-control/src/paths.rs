use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::model::APP_BUNDLE_ID;

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
    #[error("the scratch directory does not resolve to a directory that exists")]
    Unresolved,
    #[error("the scratch directory is the data directory or lies inside it")]
    InsideDataDir,
    #[error("the scratch directory holds the data directory")]
    AboveDataDir,
    #[error("the scratch directory is the account's LaunchAgents directory or lies inside it")]
    InsideLaunchAgents,
    #[error(
        "the control socket path is 104 bytes or longer and no fallback directory can hold it"
    )]
    SocketPathTooLong,
}

pub trait AccountLookup {
    /// The home directory of the account on macOS, and its LocalAppData known
    /// folder on Windows; `None` when the account gives none.
    fn base_dir(&self) -> Result<Option<OsString>, PathError>;

    fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DarwinUserTempDir(PathBuf);

impl DarwinUserTempDir {
    pub fn new(path: PathBuf) -> Option<Self> {
        is_darwin_user_temp_dir(&path).then_some(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemAccount;

impl AccountLookup for SystemAccount {
    fn base_dir(&self) -> Result<Option<OsString>, PathError> {
        system::base_dir()
    }

    fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
        system::user_temp_dir()
    }
}

pub const MACOS_DATA_DIR: [&str; 3] = ["Library", "Application Support", "Halvo"];
pub const WINDOWS_DATA_DIR: [&str; 2] = [APP_BUNDLE_ID, "data"];
pub const MACOS_LAUNCH_AGENTS_DIR: [&str; 2] = ["Library", "LaunchAgents"];

#[cfg(windows)]
const DATA_DIR: &[&str] = &WINDOWS_DATA_DIR;
#[cfg(not(windows))]
const DATA_DIR: &[&str] = &MACOS_DATA_DIR;

pub const STATE_FILE: &str = "daemon-state.json";
pub const INTENT_FILE: &str = "delete-all.intent";
#[cfg(windows)]
pub const INSTANCE_LOCK_FILE: &str = "daemon.lock";
pub const CONTROL_SOCKET: &str = "control.sock";
pub const RUN_MARKER: &str = "running";
pub const DAEMON_ERR: &str = "daemon.err";
/// The extension of a root file under `roots/`, named `<root id>.root`.
pub const ROOT_FILE_EXT: &str = "root";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Content,
    Custody,
    Runtime,
    Log,
}

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
    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn kind(&self) -> EntryKind {
        self.kind
    }

    pub const fn key_files(&self) -> &'static [&'static str] {
        self.key_files
    }

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

pub const ENTRIES: [Entry; 4] = [STORE, ROOTS, RUN, LOGS];

pub const RESERVED_ENTRY_NAMES: [&str; 2] = ["derived", "agents"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovedBy {
    DeleteAll,
    RemoveHalvo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeychainItem {
    pub service: &'static str,
    pub account_form: &'static str,
    pub owner: &'static str,
    pub removed_by: RemovedBy,
}

pub const KEYCHAIN_ITEMS: [KeychainItem; 0] = [];

/// The name of the store directory of the billing run's account helper.
pub const ACCOUNT_HELPER_STORE_DIR: &str = "com.halvo.account";

/// A directory outside the data directory that a run of Halvo keeps files in (contracts.md
/// C17 items 1 and 2), with what removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutsidePlace {
    name: &'static str,
    owner: &'static str,
    removed_by: RemovedBy,
}

impl OutsidePlace {
    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn owner(&self) -> &'static str {
        self.owner
    }

    pub const fn removed_by(&self) -> RemovedBy {
        self.removed_by
    }

    /// The directory under the account's folder for an app's local data:
    /// `~/Library/Application Support` on macOS, LocalAppData on Windows.
    pub fn path(&self, account: &dyn AccountLookup) -> Result<PathBuf, PathError> {
        let mut path = base_dir(account)?;
        path.extend(LOCAL_DATA_FOLDER);
        path.push(self.name);
        checked(path)
    }
}

#[cfg(windows)]
const LOCAL_DATA_FOLDER: &[&str] = &[];
#[cfg(not(windows))]
const LOCAL_DATA_FOLDER: &[&str] = &["Library", "Application Support"];

/// A saved sign-in stays through a delete-all and goes with Remove Halvo (C17 item 7).
pub const ACCOUNT_HELPER_STORE: OutsidePlace = OutsidePlace {
    name: ACCOUNT_HELPER_STORE_DIR,
    owner: "the account helper of run 20261109-halvo-r7-billing",
    removed_by: RemovedBy::RemoveHalvo,
};

pub const OUTSIDE_PLACES: [OutsidePlace; 1] = [ACCOUNT_HELPER_STORE];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir(PathBuf);

impl DataDir {
    pub fn for_current_user() -> Result<Self, PathError> {
        Self::for_account(&SystemAccount)
    }

    pub fn for_account(account: &dyn AccountLookup) -> Result<Self, PathError> {
        let mut path = base_dir(account)?;
        path.extend(DATA_DIR);
        Self::from_flag(path)
    }

    pub fn from_flag(path: impl Into<PathBuf>) -> Result<Self, PathError> {
        checked(path.into()).map(Self)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn is_provisioned(&self) -> bool {
        self.state_file().exists()
    }

    pub fn entry(&self, entry: &Entry) -> PathBuf {
        self.0.join(entry.name)
    }

    pub fn store_dir(&self) -> PathBuf {
        self.entry(&STORE)
    }

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

    #[cfg(windows)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScratchDir {
    given: PathBuf,
    real: PathBuf,
}

impl ScratchDir {
    /// Takes the data directory from `account`. Refuses a path that does not exist yet with
    /// [`PathError::Unresolved`], so a caller makes the scratch directory first; a path that
    /// is or lies in the data directory, as given or as resolved, with
    /// [`PathError::InsideDataDir`]; and a directory that holds the data directory, compared
    /// by directory identity, with [`PathError::AboveDataDir`]. [`ScratchDir::path`] is the
    /// resolved path.
    pub fn new(path: impl Into<PathBuf>, account: &dyn AccountLookup) -> Result<Self, PathError> {
        Self::against(path, &DataDir::for_account(account)?)
    }

    pub(crate) fn against(
        path: impl Into<PathBuf>,
        user_data_dir: &DataDir,
    ) -> Result<Self, PathError> {
        let given = checked(path.into())?;
        let real = checked(given.canonicalize().map_err(|_| PathError::Unresolved)?)?;
        let scratch = Self { given, real };
        if scratch.is_or_lies_in(user_data_dir.path()) {
            return Err(PathError::InsideDataDir);
        }
        if scratch.holds(user_data_dir.path()) {
            return Err(PathError::AboveDataDir);
        }
        Ok(scratch)
    }

    fn holds(&self, dir: &Path) -> bool {
        let Some(me) = identity(&self.real) else {
            return false;
        };
        let real_ancestors = dir
            .ancestors()
            .find_map(|existing| existing.canonicalize().ok())
            .into_iter()
            .flat_map(|real| real.ancestors().map(Path::to_path_buf).collect::<Vec<_>>());
        dir.ancestors()
            .map(Path::to_path_buf)
            .chain(real_ancestors)
            .any(|ancestor| identity(&ancestor) == Some(me))
    }

    pub fn path(&self) -> &Path {
        &self.real
    }

    fn is_or_lies_in(&self, dir: &Path) -> bool {
        lies_within(&self.given, &self.real, dir)
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginItemDir {
    path: PathBuf,
    when_missing: WhenMissing,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhenMissing {
    CreateWithMode0755,
    MustExist,
}

#[cfg(target_os = "macos")]
impl LoginItemDir {
    pub fn for_current_user() -> Result<Self, PathError> {
        Self::for_account(&SystemAccount)
    }

    pub fn for_account(account: &dyn AccountLookup) -> Result<Self, PathError> {
        let mut path = base_dir(account)?;
        path.extend(MACOS_LAUNCH_AGENTS_DIR);
        Ok(Self {
            path: checked(path)?,
            when_missing: WhenMissing::CreateWithMode0755,
        })
    }

    /// The login item's directory for a scratch run: `dir` itself, which an install writes
    /// into and never makes. Refuses `dir` with [`PathError::InsideLaunchAgents`] when, as
    /// given or as resolved, it is or lies in the account's LaunchAgents directory.
    pub fn scratch_form(dir: &ScratchDir, account: &dyn AccountLookup) -> Result<Self, PathError> {
        let agents = Self::for_account(account)?;
        if dir.is_or_lies_in(agents.path()) {
            return Err(PathError::InsideLaunchAgents);
        }
        Ok(Self {
            path: dir.path().to_path_buf(),
            when_missing: WhenMissing::MustExist,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn when_missing(&self) -> WhenMissing {
        self.when_missing
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

fn lies_within(given: &Path, real: &Path, dir: &Path) -> bool {
    let Some(target) = identity(dir) else {
        return false;
    };
    real.ancestors()
        .chain(given.ancestors())
        .any(|ancestor| identity(ancestor) == Some(target))
}

#[cfg(unix)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(path)
        .ok()
        .map(|meta| (meta.dev(), meta.ino()))
}

#[cfg(windows)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` ends in a NUL and outlives the call.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is open and `info` is a live value of the type the call fills.
    let read = unsafe { GetFileInformationByHandle(handle, &mut info) };
    // SAFETY: `handle` came from CreateFileW above and is closed once.
    let _ = unsafe { CloseHandle(handle) };
    read.ok()?;
    Some((
        u64::from(info.dwVolumeSerialNumber),
        u64::from(info.nFileIndexHigh) << 32 | u64::from(info.nFileIndexLow),
    ))
}

#[cfg(windows)]
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

pub const SOCKET_PATH_MAX_BYTES: usize = 104;

pub const FALLBACK_SOCKET_DIR: &str = crate::service::CAPTURE_AGENT_LABEL;

const FNV1A_64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_64_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn endpoint_hash(text: &str) -> String {
    let mut hash = FNV1A_64_OFFSET_BASIS;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV1A_64_PRIME);
    }
    format!("{hash:016x}")
}

pub fn pipe_name(sid: &str, data_dir: &DataDir) -> String {
    let hash = endpoint_hash(&final_path(data_dir).to_lowercase());
    format!(r"\\.\pipe\halvo-control-{sid}-{hash}")
}

#[cfg(windows)]
fn final_path(data_dir: &DataDir) -> String {
    resolved(data_dir.path())
        .to_str()
        .map_or_else(|| data_dir.text().to_string(), str::to_string)
}

#[cfg(not(windows))]
fn final_path(data_dir: &DataDir) -> String {
    data_dir.text().to_string()
}

#[cfg(target_os = "macos")]
pub fn socket_path(data_dir: &DataDir, account: &dyn AccountLookup) -> Result<PathBuf, PathError> {
    let primary = data_dir.control_socket();
    if primary.as_os_str().len() < SOCKET_PATH_MAX_BYTES {
        return Ok(primary);
    }
    let temp = account
        .user_temp_dir()?
        .ok_or(PathError::SocketPathTooLong)?;
    let temp = temp.path();
    let meta = std::fs::metadata(temp).map_err(|_| PathError::SocketPathTooLong)?;
    // SAFETY: geteuid reads the process's effective user id; it takes no pointer and cannot fail.
    let uid = unsafe { libc::geteuid() };
    if !is_a_private_dir_of(&meta, uid) {
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

#[cfg(target_os = "macos")]
fn is_a_private_dir_of(meta: &std::fs::Metadata, uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;

    meta.is_dir() && meta.uid() == uid && meta.mode() & 0o777 == 0o700
}

fn is_darwin_user_temp_dir(path: &Path) -> bool {
    let mut components = path.components();
    if components.next() != Some(Component::RootDir) {
        return false;
    }
    let names: Option<Vec<&str>> = components
        .map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    match names.as_deref() {
        Some(["var", "folders", a, b, "T"]) | Some(["private", "var", "folders", a, b, "T"]) => {
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

    use super::{DarwinUserTempDir, PathError};

    const MAX_BUFFER: usize = 1 << 20;

    pub fn base_dir() -> Result<Option<OsString>, PathError> {
        // SAFETY: geteuid reads the process's effective user id; it takes no pointer and cannot
        // fail.
        let uid = unsafe { libc::geteuid() };
        let mut buf = vec![0u8; 1024];
        loop {
            // SAFETY: passwd holds integers and raw pointers only, so all-zero bytes are a valid
            // value; getpwuid_r fills it before a field is read.
            let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
            let mut found: *mut libc::passwd = std::ptr::null_mut();
            // SAFETY: `pwd`, `found` and `buf` live across the call and `buf.len()` is the length
            // of `buf`, so getpwuid_r writes only inside them, and the strings that it points `pwd`
            // at lie in `buf`.
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
            // SAFETY: getpwuid_r returned 0 with `found` and `pw_dir` not null, so `pw_dir` points
            // to a NUL-terminated string in `buf`, which lives and is not written until `dir` is
            // copied below.
            let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
            return Ok(Some(OsString::from_vec(dir.to_bytes().to_vec())));
        }
    }

    pub fn user_temp_dir() -> Result<Option<DarwinUserTempDir>, PathError> {
        let name = libc::_CS_DARWIN_USER_TEMP_DIR;
        // SAFETY: with a null buffer and a length of 0, confstr writes nothing and returns the
        // length that the value needs.
        let len = unsafe { libc::confstr(name, std::ptr::null_mut(), 0) };
        if len == 0 {
            return Ok(None);
        }
        let mut buf = vec![0u8; len];
        // SAFETY: `buf` is live and writable for `buf.len()` bytes, the length passed, so confstr
        // writes inside it.
        let got = unsafe { libc::confstr(name, buf.as_mut_ptr().cast(), buf.len()) };
        if got == 0 || got > buf.len() {
            return Ok(None);
        }
        let text = CStr::from_bytes_until_nul(&buf).map_err(|_| PathError::NotUtf8)?;
        let path = PathBuf::from(OsString::from_vec(text.to_bytes().to_vec()));
        Ok(DarwinUserTempDir::new(path))
    }
}

#[cfg(windows)]
mod system {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use windows::core::{GUID, HRESULT, PWSTR};
    use windows::Win32::Foundation::{E_POINTER, HANDLE};
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT};

    use super::PathError;

    mod token {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{TOKEN_IMPERSONATE, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        /// The access token of the current process; only `open` makes one.
        pub(super) struct Token(HANDLE);

        impl Token {
            pub(super) fn open() -> windows::core::Result<Token> {
                let mut handle = HANDLE::default();
                // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing, and
                // `handle` is a live HANDLE that the call writes.
                unsafe {
                    OpenProcessToken(
                        GetCurrentProcess(),
                        TOKEN_QUERY | TOKEN_IMPERSONATE,
                        &mut handle,
                    )
                }?;
                Ok(Token(handle))
            }

            pub(super) fn handle(&self) -> HANDLE {
                self.0
            }
        }

        impl Drop for Token {
            fn drop(&mut self) {
                // SAFETY: only `open` makes a Token, from a handle that OpenProcessToken opened,
                // and this drop closes it once.
                let _ = unsafe { CloseHandle(self.0) };
            }
        }
    }

    struct TaskMem(PWSTR);

    impl Drop for TaskMem {
        fn drop(&mut self) {
            #[cfg(test)]
            {
                if !self.0.is_null() {
                    tests::FREED.with(|freed| freed.set(freed.get() + 1));
                }
            }
            // SAFETY: the pointer is null, or a buffer of the COM task allocator that this value
            // alone owns, and this drop frees it once; CoTaskMemFree of a null pointer does
            // nothing.
            unsafe { CoTaskMemFree(Some(self.0 .0 as *const _)) };
        }
    }

    /// Runs `call` with the out pointer of a buffer that is freed on every path, and copies the
    /// string out of it on success. A success that left the pointer null is a lookup error.
    ///
    /// # Safety
    ///
    /// On success `call` has set the out pointer to a NUL-terminated string in a buffer of the
    /// COM task allocator that nothing else frees; on failure it has left it null or set it to
    /// such a buffer.
    unsafe fn known_folder(
        call: impl FnOnce(*mut PWSTR) -> HRESULT,
    ) -> Result<Option<OsString>, PathError> {
        let mut path = TaskMem(PWSTR::null());
        call(&mut path.0)
            .ok()
            .map_err(|e| PathError::Lookup(e.code().0))?;
        if path.0.is_null() {
            return Err(PathError::Lookup(E_POINTER.0));
        }
        // SAFETY: the call succeeded and set the pointer, so by the condition of this function
        // it points to a NUL-terminated string, which `path` frees only after this copy.
        let wide = unsafe { path.0.as_wide() };
        Ok(Some(OsString::from_wide(wide)))
    }

    pub fn base_dir() -> Result<Option<OsString>, PathError> {
        let token = token::Token::open().map_err(|e| PathError::Lookup(e.code().0))?;
        windows::core::link!("shell32.dll" "system" fn SHGetKnownFolderPath(rfid: *const GUID, dwflags: u32, htoken: HANDLE, ppszpath: *mut PWSTR) -> HRESULT);
        let lookup = |path: *mut PWSTR| {
            // SAFETY: the folder id is a static GUID, the token is open with the TOKEN_QUERY and
            // TOKEN_IMPERSONATE rights that the function asks for, and `path` is the live out
            // pointer of a PWSTR that the call writes.
            unsafe {
                SHGetKnownFolderPath(
                    &FOLDERID_LocalAppData,
                    KF_FLAG_DEFAULT.0 as u32,
                    token.handle(),
                    path,
                )
            }
        };
        // SAFETY: SHGetKnownFolderPath sets the out pointer to a string that the COM task
        // allocator holds for its caller to free, on success and on failure alike, or leaves it
        // null; nothing else frees it, so the condition of known_folder holds.
        unsafe { known_folder(lookup) }
    }

    pub fn user_temp_dir() -> Result<Option<super::DarwinUserTempDir>, PathError> {
        Ok(None)
    }

    #[cfg(test)]
    pub(super) mod tests {
        use std::cell::Cell;

        use windows::core::{HRESULT, PWSTR};
        use windows::Win32::Foundation::{E_FAIL, E_POINTER};
        use windows::Win32::System::Com::CoTaskMemAlloc;

        use super::{known_folder, PathError};

        thread_local! {
            pub(crate) static FREED: Cell<usize> = const { Cell::new(0) };
        }

        fn stand_in(path: *mut PWSTR, code: HRESULT) -> HRESULT {
            let text: Vec<u16> = "C:\\stand-in\0".encode_utf16().collect();
            // SAFETY: the size is that of `text`; the buffer is written below before any read.
            let buffer = unsafe { CoTaskMemAlloc(text.len() * 2) }.cast::<u16>();
            assert!(!buffer.is_null());
            // SAFETY: `buffer` holds `text.len()` u16 values, and `path` is the live out
            // pointer that known_folder hands in.
            unsafe {
                std::ptr::copy_nonoverlapping(text.as_ptr(), buffer, text.len());
                *path = PWSTR(buffer);
            }
            code
        }

        #[test]
        fn a_failed_lookup_that_hands_back_a_buffer_frees_it_and_answers_its_code() {
            let before = FREED.with(Cell::get);
            // SAFETY: stand_in sets the out pointer to a NUL-terminated string in a buffer of
            // the COM task allocator that nothing else frees.
            let failed = unsafe { known_folder(|path| stand_in(path, E_FAIL)) };
            assert_eq!(failed, Err(PathError::Lookup(E_FAIL.0)));
            assert_eq!(
                FREED.with(Cell::get),
                before + 1,
                "the buffer was not freed"
            );
            // SAFETY: as above, stand_in sets the out pointer to such a buffer.
            let ok = unsafe { known_folder(|path| stand_in(path, HRESULT(0))) }
                .unwrap()
                .unwrap();
            assert_eq!(ok, "C:\\stand-in");
            assert_eq!(FREED.with(Cell::get), before + 2);
        }

        #[test]
        fn a_success_that_left_the_pointer_null_is_a_lookup_error_and_is_never_read() {
            let before = FREED.with(Cell::get);
            // SAFETY: the call writes nothing, so the out pointer stays null and no buffer is
            // handed out; known_folder reads no string on that path.
            let empty = unsafe { known_folder(|_| HRESULT(0)) };
            assert_eq!(empty, Err(PathError::Lookup(E_POINTER.0)));
            assert_eq!(
                FREED.with(Cell::get),
                before,
                "a null pointer was counted as a buffer"
            );
        }
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

        fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
            Ok(self.temp.clone().and_then(DarwinUserTempDir::new))
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
            assert!(
                ScratchDir::against(&path, &data).is_err(),
                "{}",
                path.display()
            );
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
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text
            .lines()
            .find_map(|line| line.split_once("data_dir=").map(|(_, dir)| dir))
            .unwrap_or_else(|| {
                let errors = String::from_utf8_lossy(&output.stderr);
                panic!("no data_dir line in {text}; stderr: {errors}")
            });
        assert_eq!(Path::new(line), mine.path());
        assert!(!Path::new(line).starts_with(elsewhere.path()));
    }

    struct At(PathBuf);

    impl AccountLookup for At {
        fn base_dir(&self) -> Result<Option<OsString>, PathError> {
            Ok(Some(self.0.clone().into_os_string()))
        }

        fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
            Ok(None)
        }
    }

    #[test]
    fn a_scratch_directory_that_holds_the_data_directory_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().canonicalize().unwrap().join("home");
        std::fs::create_dir(&home).unwrap();
        let account = At(home.clone());
        let data = DataDir::for_account(&account).unwrap();
        let above: Vec<&Path> = data
            .path()
            .ancestors()
            .skip(1)
            .take_while(|a| a.starts_with(&home))
            .collect();
        assert_eq!(above.len(), DATA_DIR.len(), "{above:?}");
        for dir in &above {
            std::fs::create_dir_all(dir).unwrap();
            assert_eq!(
                ScratchDir::new(dir, &account),
                Err(PathError::AboveDataDir),
                "{}",
                dir.display()
            );
        }
        let top = home.ancestors().last().unwrap();
        assert_eq!(ScratchDir::new(top, &account), Err(PathError::AboveDataDir));
        assert_eq!(
            ScratchDir::new(root.path(), &account),
            Err(PathError::AboveDataDir)
        );
        std::fs::create_dir_all(data.path()).unwrap();
        assert_eq!(
            ScratchDir::new(data.path(), &account),
            Err(PathError::InsideDataDir)
        );
        let side = root.path().join("side");
        std::fs::create_dir(&side).unwrap();
        assert!(ScratchDir::new(&side, &account).is_ok());
        let in_home_not_above = home.join("other");
        std::fs::create_dir(&in_home_not_above).unwrap();
        assert!(ScratchDir::new(&in_home_not_above, &account).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_scratch_directory_above_the_data_directory_is_refused_through_a_link_and_by_its_real_parent(
    ) {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().canonicalize().unwrap();
        let homes = real.join("homes");
        std::fs::create_dir_all(homes.join("a")).unwrap();
        let link = real.join("l");
        std::os::unix::fs::symlink(homes.join("a"), &link).unwrap();
        let account = At(link.clone());
        assert_eq!(
            ScratchDir::new(&link, &account),
            Err(PathError::AboveDataDir)
        );
        assert_eq!(
            ScratchDir::new(homes.join("a"), &account),
            Err(PathError::AboveDataDir)
        );
        assert_eq!(
            ScratchDir::new(&homes, &account),
            Err(PathError::AboveDataDir)
        );
        let to_homes = real.join("to-homes");
        std::os::unix::fs::symlink(&homes, &to_homes).unwrap();
        assert_eq!(
            ScratchDir::new(&to_homes, &account),
            Err(PathError::AboveDataDir)
        );
        let other = homes.join("b");
        std::fs::create_dir(&other).unwrap();
        assert!(ScratchDir::new(&other, &account).is_ok());
    }

    #[test]
    fn the_scratch_type_takes_the_data_directory_from_the_account() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().canonicalize().unwrap();
        let data = DataDir::for_account(&At(home.clone())).unwrap();
        std::fs::create_dir_all(data.path()).unwrap();
        let inside = data.path().join("store");
        std::fs::create_dir(&inside).unwrap();
        assert_eq!(
            ScratchDir::new(&inside, &At(home.clone())),
            Err(PathError::InsideDataDir)
        );
        let elsewhere = tempfile::tempdir().unwrap();
        assert!(ScratchDir::new(&inside, &At(elsewhere.path().to_path_buf())).is_ok());
        assert_eq!(
            ScratchDir::new(&inside, &account("relative")),
            Err(PathError::Relative)
        );
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
    fn a_scratch_value_holds_the_path_that_it_resolves_to() {
        #[cfg(unix)]
        use std::os::unix::fs::symlink as symlink_dir;
        #[cfg(windows)]
        use std::os::windows::fs::symlink_dir;

        let guard = tempfile::tempdir().unwrap();
        let real = guard.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = guard.path().join("link");
        symlink_dir(&real, &link).unwrap();
        let data = DataDir::from_flag(guard.path().join("data")).unwrap();
        assert_eq!(
            ScratchDir::against(&link, &data).unwrap().path(),
            real.canonicalize().unwrap()
        );
    }

    #[test]
    fn a_scratch_directory_is_refused_when_its_given_or_its_resolved_path_is_in_the_data_directory()
    {
        #[cfg(unix)]
        use std::os::unix::fs::symlink as symlink_dir;
        #[cfg(windows)]
        use std::os::windows::fs::symlink_dir;

        let guard = tempfile::tempdir().unwrap();
        let root = guard.path();
        let data = DataDir::from_flag(root.join("data")).unwrap();
        std::fs::create_dir_all(data.path().join("store").join("x")).unwrap();
        std::fs::create_dir(root.join("elsewhere")).unwrap();
        let to_store = root.join("to-store");
        symlink_dir(data.path().join("store"), &to_store).unwrap();
        let to_data = root.join("to-data");
        symlink_dir(data.path(), &to_data).unwrap();
        let out = data.path().join("out");
        symlink_dir(root.join("elsewhere"), &out).unwrap();
        let got: Vec<_> = [
            to_store.clone(),
            to_store.join("x"),
            to_store.join("missing"),
            to_data.join("store"),
            out,
            root.join("elsewhere"),
        ]
        .iter()
        .map(|scratch| ScratchDir::against(scratch, &data).map(|_| ()))
        .collect();
        assert_eq!(
            got,
            [
                Err(PathError::InsideDataDir),
                Err(PathError::InsideDataDir),
                Err(PathError::Unresolved),
                Err(PathError::InsideDataDir),
                Err(PathError::InsideDataDir),
                Ok(()),
            ]
        );
    }

    #[test]
    fn a_scratch_directory_is_never_the_data_directory_or_inside_it() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::from_flag(root.path().join("Halvo")).unwrap();
        std::fs::create_dir_all(data.path().join("store")).unwrap();
        std::fs::create_dir_all(data.path().join("a").join("b")).unwrap();
        let upper = PathBuf::from(data.path().to_str().unwrap().to_uppercase());
        for inside in [
            data.path().to_path_buf(),
            data.path().join("store"),
            data.path().join("a").join("b"),
            upper.join("store"),
        ] {
            assert_eq!(
                ScratchDir::against(&inside, &data),
                Err(PathError::InsideDataDir),
                "{}",
                inside.display()
            );
        }
        for outside in [
            root.path().join("Halvo-scratch"),
            root.path().join("scratch"),
        ] {
            std::fs::create_dir(&outside).unwrap();
            assert_eq!(
                ScratchDir::against(&outside, &data).unwrap().path(),
                outside.canonicalize().unwrap()
            );
        }
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(data.path(), &link).unwrap();
            assert_eq!(
                ScratchDir::against(link.join("store"), &data),
                Err(PathError::InsideDataDir)
            );
        }
    }

    #[test]
    fn the_account_helper_s_store_directory_has_this_name_on_both_systems() {
        assert_eq!(ACCOUNT_HELPER_STORE_DIR, "com.halvo.account");
        assert_eq!(ACCOUNT_HELPER_STORE.name(), ACCOUNT_HELPER_STORE_DIR);
        assert_eq!(OUTSIDE_PLACES, [ACCOUNT_HELPER_STORE]);
    }

    #[test]
    fn delete_all_leaves_a_saved_sign_in_and_remove_halvo_takes_it_away() {
        assert_eq!(ACCOUNT_HELPER_STORE.removed_by(), RemovedBy::RemoveHalvo);
        for place in OUTSIDE_PLACES {
            assert!(ENTRIES.iter().all(|entry| entry.name() != place.name()));
            assert!(!RESERVED_ENTRY_NAMES.contains(&place.name()));
        }
    }

    #[test]
    fn the_account_helper_s_store_directory_lies_in_the_local_data_folder_and_outside_the_data_directory(
    ) {
        let home = tempfile::tempdir().unwrap();
        let user = At(home.path().to_path_buf());
        let place = ACCOUNT_HELPER_STORE.path(&user).unwrap();
        let folder: &[&str] = if cfg!(windows) {
            &[]
        } else {
            &["Library", "Application Support"]
        };
        let mut want = home.path().to_path_buf();
        want.extend(folder);
        want.push("com.halvo.account");
        assert_eq!(place, want);
        let data = DataDir::for_account(&user).unwrap();
        assert!(!place.starts_with(data.path()));
        assert!(!data.path().starts_with(&place));
        assert_eq!(
            ACCOUNT_HELPER_STORE.path(&account("relative")),
            Err(PathError::Relative)
        );
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
        #[cfg(windows)]
        let files = [STATE_FILE, INTENT_FILE, INSTANCE_LOCK_FILE];
        #[cfg(not(windows))]
        let files = [STATE_FILE, INTENT_FILE];
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
    fn the_top_level_files_have_these_names() {
        let data = DataDir::from_flag(absolute("data")).unwrap();
        assert_eq!(
            data.intent_file(),
            absolute("data").join("delete-all.intent")
        );
        #[cfg(windows)]
        assert_eq!(
            data.instance_lock_file(),
            PathBuf::from(r"C:\data\daemon.lock")
        );
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_the_data_directory_lies_in_the_local_app_data_folder() {
        use std::os::windows::ffi::OsStringExt;
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{
            FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
        };

        // SAFETY: the folder id is a static GUID, and no token asks for this user's folder.
        let path =
            unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }.unwrap();
        // SAFETY: the call succeeded, so `path` points to a NUL-terminated string.
        let local = PathBuf::from(OsString::from_wide(unsafe { path.as_wide() }));
        // SAFETY: `path` is the buffer that the call allocated, and it is freed once here.
        unsafe { CoTaskMemFree(Some(path.0 as *const _)) };
        let data = DataDir::for_current_user().unwrap();
        println!(
            "local app data {}, data directory {}",
            local.display(),
            data.path().display()
        );
        assert!(data.path().starts_with(&local));
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
        #[cfg(windows)]
        let top_level = [
            data.state_file(),
            data.intent_file(),
            data.instance_lock_file(),
        ];
        #[cfg(not(windows))]
        let top_level = [data.state_file(), data.intent_file()];
        for path in top_level {
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
        std::fs::create_dir_all(data.path().join("store")).unwrap();
        let firm = Path::new("/System/Volumes/Data").join(data.path().strip_prefix("/").unwrap());
        let (a, b) = (
            std::fs::metadata(data.path()).unwrap(),
            std::fs::metadata(&firm).unwrap(),
        );
        assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()));
        for inside in [firm.clone(), firm.join("store")] {
            assert_eq!(
                ScratchDir::against(&inside, &data),
                Err(PathError::InsideDataDir),
                "{}",
                inside.display()
            );
        }
        assert_eq!(
            ScratchDir::against(firm.join("not-yet").join("x"), &data),
            Err(PathError::Unresolved)
        );
        let missing = DataDir::from_flag(real.join("Later")).unwrap();
        let firm_missing =
            Path::new("/System/Volumes/Data").join(missing.path().strip_prefix("/").unwrap());
        assert_eq!(
            ScratchDir::against(firm_missing.join("store"), &missing),
            Err(PathError::Unresolved)
        );
        std::fs::create_dir(real.join("Halvo-scratch")).unwrap();
        assert!(ScratchDir::against(firm.with_file_name("Halvo-scratch"), &data).is_ok());
    }

    #[test]
    fn a_last_component_other_than_t_is_refused() {
        for other in ["/var/folders/7n/abc/C", "/private/var/folders/7n/abc/0"] {
            assert!(!is_darwin_user_temp_dir(Path::new(other)), "{other}");
            assert_eq!(DarwinUserTempDir::new(PathBuf::from(other)), None);
        }
    }

    #[test]
    fn a_suffix_after_t_is_refused() {
        for suffixed in ["/var/folders/7n/abc/T/zz/", "/var/folders/7n/abc/T/r1/T/"] {
            assert!(!is_darwin_user_temp_dir(Path::new(suffixed)), "{suffixed}");
            assert_eq!(DarwinUserTempDir::new(PathBuf::from(suffixed)), None);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_socket_path_bound_is_the_size_of_sun_path() {
        // SAFETY: sockaddr_un holds integers and a byte array only, so all-zero bytes are a valid
        // value.
        let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        assert_eq!(SOCKET_PATH_MAX_BYTES, address.sun_path.len());
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
            "/var/folders/7n/T/",
            "/tmp/",
            "/Users/a/T",
            "var/folders/7n/abc/T",
        ] {
            assert!(!is_darwin_user_temp_dir(Path::new(bad)), "{bad}");
        }
        for path in ["/var/folders/7n/abc/T", "/tmp", "relative/T"] {
            assert_eq!(
                DarwinUserTempDir::new(PathBuf::from(path)).is_some(),
                is_darwin_user_temp_dir(Path::new(path)),
                "{path}"
            );
        }
        let temp = SystemAccount.user_temp_dir().unwrap().unwrap();
        assert!(
            is_darwin_user_temp_dir(temp.path()),
            "{}",
            temp.path().display()
        );
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

    #[cfg(windows)]
    #[test]
    fn on_windows_a_scratch_path_is_refused_in_every_form_of_the_data_directory() {
        let guard = tempfile::tempdir().unwrap();
        let root = guard.path();
        let data = DataDir::from_flag(root.join("data")).unwrap();
        std::fs::create_dir_all(data.path().join("store")).unwrap();
        std::fs::create_dir(root.join("elsewhere")).unwrap();
        let missing = DataDir::from_flag(root.join("missing-data")).unwrap();
        let mut rows = vec![
            (
                "another case",
                &data,
                root.join("DATA").join("store"),
                Err(PathError::InsideDataDir),
            ),
            (
                "a trailing dot",
                &data,
                root.join("data."),
                Err(PathError::InsideDataDir),
            ),
            (
                "below a trailing dot",
                &data,
                root.join("data.").join("store"),
                Err(PathError::InsideDataDir),
            ),
            (
                "a trailing dot of a missing data dir",
                &missing,
                root.join("missing-data."),
                Err(PathError::Unresolved),
            ),
            (
                "control: a directory outside",
                &data,
                root.join("elsewhere"),
                Ok(()),
            ),
        ];
        let real = data.path().canonicalize().unwrap();
        let text = real.to_str().unwrap().trim_start_matches(r"\\?\");
        let (drive, rest) = text.split_once(":\\").unwrap();
        let share = PathBuf::from(format!(r"\\localhost\{drive}$\{rest}")).join("store");
        let reachable = share.is_dir();
        println!("admin share {} reachable={reachable}", share.display());
        if reachable {
            rows.push((
                "the admin share",
                &data,
                share,
                Err(PathError::InsideDataDir),
            ));
        }
        let wrong: Vec<String> = rows
            .iter()
            .filter_map(|(form, data, scratch, want)| {
                let got = ScratchDir::against(scratch, data).map(|_| ());
                println!("{form}: {} -> {got:?}", scratch.display());
                (got != *want).then(|| format!("{form}: got {got:?}, want {want:?}"))
            })
            .collect();
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[cfg(windows)]
    #[test]
    fn one_data_directory_keeps_its_pipe_name_when_it_is_made() {
        let root = tempfile::tempdir().unwrap();
        let data = DataDir::from_flag(root.path().join("Data")).unwrap();
        let sid = "S-1-5-21-1-2-3-1001";
        let before = pipe_name(sid, &data);
        std::fs::create_dir(data.path()).unwrap();
        let after = pipe_name(sid, &data);
        println!("pipe name before {before}, after {after}");
        assert_eq!(before, after);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_scratch_path_is_refused_in_every_form_of_the_data_or_launch_agents_directory() {
        use std::os::unix::fs::symlink;

        enum Target<'a> {
            Data(&'a Path),
            LaunchAgents,
        }
        let guard = tempfile::tempdir().unwrap();
        let root = guard.path().canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(home.join("Library")).unwrap();
        let account = Account {
            base: Some(Box::leak(
                home.to_str().unwrap().to_owned().into_boxed_str(),
            )),
            temp: None,
        };
        let agents = LoginItemDir::for_account(&account).unwrap();
        let far = DataDir::from_flag(root.join("far-data")).unwrap();
        let mut wrong = Vec::new();
        let mut count = 0;
        let mut judge =
            |form: &str, target: Target, scratch: &Path, want: Result<(), PathError>| {
                count += 1;
                let got = match target {
                    Target::Data(data) => {
                        ScratchDir::against(scratch, &DataDir::from_flag(data).unwrap()).map(|_| ())
                    }
                    Target::LaunchAgents => ScratchDir::against(scratch, &far)
                        .and_then(|scratch| LoginItemDir::scratch_form(&scratch, &account))
                        .map(|_| ()),
                };
                if got != want {
                    wrong.push(format!("{form}: got {got:?}, want {want:?}"));
                }
            };
        let dir_of = |case: &str| {
            let dir = root.join(case);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        let unresolved = Err(PathError::Unresolved);

        for (case, data, scratch, form) in [
            (
                "1",
                "Caf\u{e9}",
                "Cafe\u{301}",
                "another spelling (NFD) of a missing data dir",
            ),
            (
                "2",
                "stra\u{df}e",
                "STRASSE",
                "another spelling (STRASSE) of a missing data dir",
            ),
            (
                "3",
                "store",
                "\u{17f}tore",
                "another spelling (long s) of a missing data dir",
            ),
        ] {
            let dir = dir_of(case);
            judge(
                form,
                Target::Data(&dir.join(data)),
                &dir.join(scratch),
                unresolved,
            );
        }

        let dir = dir_of("4");
        let data = dir.join("data");
        symlink(&data, dir.join("link")).unwrap();
        judge(
            "a dangling link to a missing data dir",
            Target::Data(&data),
            &dir.join("link"),
            unresolved,
        );
        judge(
            "a path below a dangling link to a missing data dir",
            Target::Data(&data),
            &dir.join("link").join("store"),
            unresolved,
        );

        judge(
            "another spelling (long s) of a missing LaunchAgents",
            Target::LaunchAgents,
            &home.join("Library").join("LaunchAgent\u{17f}"),
            unresolved,
        );
        symlink(agents.path(), root.join("agents-link")).unwrap();
        judge(
            "a dangling link to a missing LaunchAgents",
            Target::LaunchAgents,
            &root.join("agents-link"),
            unresolved,
        );

        let dir = dir_of("8");
        let live = dir.join("live-data");
        std::fs::create_dir(&live).unwrap();
        symlink(live.join("store"), dir.join("store-link")).unwrap();
        judge(
            "a dangling link to a missing child of an existing data dir",
            Target::Data(&live),
            &dir.join("store-link"),
            unresolved,
        );
        judge(
            "a missing parent below an existing data dir",
            Target::Data(&live),
            &live.join("no-such-parent").join("scratch"),
            unresolved,
        );

        std::fs::create_dir(live.join("store")).unwrap();
        symlink(&live, dir.join("live-link")).unwrap();
        let elsewhere = dir_of("elsewhere");
        symlink(&elsewhere, live.join("out")).unwrap();
        for (form, scratch) in [
            ("a link to an existing data dir", dir.join("live-link")),
            (
                "a path below a link to an existing data dir",
                dir.join("live-link").join("store"),
            ),
            (
                "another case of an existing data dir",
                dir.join("LIVE-DATA").join("store"),
            ),
            (
                "another spelling (long s) of an existing data dir's store",
                live.join("\u{17f}tore"),
            ),
            (
                "a link inside an existing data dir to a directory outside it",
                live.join("out"),
            ),
        ] {
            judge(
                form,
                Target::Data(&live),
                &scratch,
                Err(PathError::InsideDataDir),
            );
        }

        std::fs::create_dir_all(agents.path()).unwrap();
        symlink(agents.path(), root.join("agents-link-2")).unwrap();
        symlink(&elsewhere, agents.path().join("out")).unwrap();
        for (form, scratch) in [
            (
                "a link to an existing LaunchAgents",
                root.join("agents-link-2"),
            ),
            (
                "another spelling (long s) of an existing LaunchAgents",
                home.join("Library").join("LaunchAgent\u{17f}"),
            ),
            (
                "a link inside an existing LaunchAgents to a directory outside it",
                agents.path().join("out"),
            ),
        ] {
            judge(
                form,
                Target::LaunchAgents,
                &scratch,
                Err(PathError::InsideLaunchAgents),
            );
        }

        let outside = dir_of("outside");
        judge(
            "control: an existing directory outside both",
            Target::Data(&live),
            &outside,
            Ok(()),
        );

        assert!(
            wrong.is_empty(),
            "{} of {count} forms wrong:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
        assert!(!agents.path().join("com.halvo.check-l.plist").exists());
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
        #[cfg(windows)]
        let final_path = r"\\?\C:\Data A";
        #[cfg(not(windows))]
        let final_path = a.path().to_str().unwrap();
        let hash = endpoint_hash(&final_path.to_lowercase());
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
    fn a_temp_dir_of_another_account_never_holds_the_fallback_socket() {
        use std::os::unix::fs::MetadataExt;

        // SAFETY: geteuid reads the process's effective user id; it takes no pointer and cannot
        // fail.
        let me = unsafe { libc::geteuid() };
        let long = DataDir::from_flag(format!("/{}", "a".repeat(120))).unwrap();
        let mut seen = 0;
        for a in std::fs::read_dir("/var/folders").unwrap().flatten() {
            let Ok(inner) = std::fs::read_dir(a.path()) else {
                continue;
            };
            for b in inner.flatten() {
                let temp = b.path().join("T");
                let Ok(meta) = std::fs::metadata(&temp) else {
                    continue;
                };
                if !meta.is_dir() || meta.uid() == me || meta.mode() & 0o777 != 0o700 {
                    continue;
                }
                seen += 1;
                let account = Account {
                    base: None,
                    temp: Some(temp),
                };
                assert_eq!(
                    socket_path(&long, &account),
                    Err(PathError::SocketPathTooLong)
                );
            }
        }
        println!("temp dirs of other accounts with mode 0700: {seen}");
        assert!(seen > 0, "no directory of another account here");
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_is_refused_by_both_types() {
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(std::ffi::OsStr::from_bytes(b"/data-\xff"));
        let data = DataDir::from_flag("/data").unwrap();
        assert_eq!(DataDir::from_flag(path), Err(PathError::NotUtf8));
        assert_eq!(ScratchDir::against(path, &data), Err(PathError::NotUtf8));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_fallback_starts_at_a_socket_path_of_exactly_104_bytes() {
        let none = Account {
            base: None,
            temp: None,
        };
        let socket_for = |len: usize| {
            let name = "a".repeat(len - "/run/control.sock".len() - 1);
            let data = DataDir::from_flag(format!("/{name}")).unwrap();
            assert_eq!(data.control_socket().as_os_str().len(), len);
            socket_path(&data, &none)
        };
        assert!(socket_for(SOCKET_PATH_MAX_BYTES - 1).is_ok());
        assert_eq!(
            socket_for(SOCKET_PATH_MAX_BYTES),
            Err(PathError::SocketPathTooLong)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_fallback_socket_path_of_exactly_104_bytes_is_refused() {
        let mine = SystemAccount.user_temp_dir().unwrap().unwrap();
        let private: PathBuf = Path::new("/private")
            .join(mine.path().strip_prefix("/").unwrap())
            .components()
            .collect();
        let fallback =
            private.as_os_str().len() + "/com.halvo.capture/0123456789abcdef/control.sock".len();
        println!("fallback bytes through the /private spelling: {fallback}");
        assert_eq!(
            fallback, SOCKET_PATH_MAX_BYTES,
            "the case needs a path of 104 bytes"
        );
        let long = DataDir::from_flag(format!("/{}", "a".repeat(120))).unwrap();
        let account = Account {
            base: None,
            temp: Some(private),
        };
        assert_eq!(
            socket_path(&long, &account),
            Err(PathError::SocketPathTooLong)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_private_dir_is_a_directory_that_the_user_owns_with_mode_0700() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let root = tempfile::tempdir().unwrap();
        let made = |name: &str, dir: bool, mode: u32| {
            let path = root.path().join(name);
            if dir {
                std::fs::create_dir(&path).unwrap();
            } else {
                std::fs::write(&path, "x").unwrap();
            }
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            std::fs::metadata(&path).unwrap()
        };
        let private = made("private", true, 0o700);
        let me = private.uid();
        assert!(is_a_private_dir_of(&private, me));
        assert!(is_a_private_dir_of(&made("sticky", true, 0o1700), me));
        assert!(!is_a_private_dir_of(&private, me + 1), "another owner");
        assert!(
            !is_a_private_dir_of(&made("file", false, 0o700), me),
            "a file"
        );
        for mode in [0o755, 0o770, 0o701, 0o600, 0o500] {
            let open = made(&format!("mode-{mode:o}"), true, mode);
            assert!(!is_a_private_dir_of(&open, me), "mode {mode:o}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn two_long_data_directories_get_two_fallback_sockets_under_104_bytes() {
        let long = |c: &str| DataDir::from_flag(format!("/{}", c.repeat(120))).unwrap();
        let (a, b) = (long("a"), long("b"));
        let temp = SystemAccount.user_temp_dir().unwrap().unwrap();
        let temp = temp.path();
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
    fn an_implementor_cannot_hand_socket_path_a_temp_dir_of_another_shape() {
        let long = DataDir::from_flag(format!("/{}", "a".repeat(120))).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let private = tempfile::tempdir().unwrap();
        std::fs::set_permissions(private.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let account = Account {
            base: None,
            temp: Some(private.path().to_path_buf()),
        };
        assert_eq!(
            socket_path(&long, &account),
            Err(PathError::SocketPathTooLong)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_long_data_directory_without_a_private_user_temp_dir_is_an_error() {
        let long = DataDir::from_flag(format!("/{}", "a".repeat(120))).unwrap();
        for temp in [None, Some(PathBuf::from("/var/folders/r1/no-such-dir/T"))] {
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
            LoginItemDir::for_account(&account(BASE)).unwrap().path(),
            Path::new("/Users/a b/Library/LaunchAgents")
        );
        assert_eq!(
            LoginItemDir::for_account(&account("")),
            Err(PathError::Empty)
        );
        assert_eq!(
            LoginItemDir::for_account(&account("Users/a")),
            Err(PathError::Relative)
        );
        let unset = Account {
            base: None,
            temp: None,
        };
        assert_eq!(LoginItemDir::for_account(&unset), Err(PathError::Unset));
    }
}
