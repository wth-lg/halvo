use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// The model lock (contracts.md C23.12), held by this process while the value lives.
///
/// The lock is an exclusive lock of the operating system on the open lock file, so the
/// system frees it when the value is dropped or when the process ends, also when it is
/// killed. A loader takes it before it reads a weights file and keeps the value until it
/// exits.
#[derive(Debug)]
pub struct ModelLock {
    _file: File,
    path: PathBuf,
}

impl ModelLock {
    /// The path of the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Why the model lock was not taken.
#[derive(Debug)]
pub enum LockError {
    /// Another process holds the model lock: the loader loads nothing, exits with a
    /// non-zero status, and its caller starts it again after the holder has ended.
    Held(PathBuf),
    /// The variable that names the user's cache directory is not set or is empty:
    /// `HOME` on macOS, `LOCALAPPDATA` on Windows.
    NoCacheDir(&'static str),
    /// This system has no model lock: it is neither macOS nor Windows.
    Unsupported,
    /// A call on the lock file or its directory failed.
    Io {
        call: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Held(path) => write!(
                f,
                "the model lock is held by another process: {}",
                path.display()
            ),
            LockError::NoCacheDir(var) => {
                write!(f, "no model lock: the variable {var} is not set")
            }
            LockError::Unsupported => write!(f, "no model lock on this system"),
            LockError::Io { call, path, source } => {
                write!(f, "{call} {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LockError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(target_os = "macos")]
const CACHE_VAR: &str = "HOME";
#[cfg(windows)]
const CACHE_VAR: &str = "LOCALAPPDATA";

#[cfg(target_os = "macos")]
fn lock_path_in(base: &Path) -> PathBuf {
    base.join("Library")
        .join("Caches")
        .join("halvo")
        .join("model.lock")
}

#[cfg(windows)]
fn lock_path_in(base: &Path) -> PathBuf {
    base.join("halvo").join("model.lock")
}

/// The path of this user's model lock: `$HOME/Library/Caches/halvo/model.lock` on macOS
/// and `%LOCALAPPDATA%\halvo\model.lock` on Windows, read from the environment of this
/// process.
#[cfg(any(target_os = "macos", windows))]
pub fn model_lock_path() -> Result<PathBuf, LockError> {
    match std::env::var_os(CACHE_VAR) {
        Some(base) if !base.is_empty() => Ok(lock_path_in(Path::new(&base))),
        _ => Err(LockError::NoCacheDir(CACHE_VAR)),
    }
}

/// The path of this user's model lock; there is none on this system.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn model_lock_path() -> Result<PathBuf, LockError> {
    Err(LockError::Unsupported)
}

fn lock_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    options
}

#[cfg(target_os = "macos")]
fn keep_across_exec(file: &File, path: &Path) -> Result<(), LockError> {
    use std::os::fd::AsRawFd;
    let fd = file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
        return Err(LockError::Io {
            call: "fcntl",
            path: path.to_path_buf(),
            source: io::Error::last_os_error(),
        });
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn keep_across_exec(_file: &File, _path: &Path) -> Result<(), LockError> {
    Ok(())
}

/// Takes the model lock at [`model_lock_path`] without waiting.
///
/// Creates the directory and the empty lock file where they are missing, writes nothing
/// into the file and never removes it. Returns [`LockError::Held`] at once when another
/// process holds the lock.
///
/// The lock binds the open file. On Windows the file is opened without delete sharing, so
/// while it is held neither the file nor its directory can be renamed or removed. On macOS
/// they can: a lock file or directory that another program moves away while the lock is
/// held lets the next loader create a new file and take the lock.
///
/// On macOS the lock stays held across an exec of the holder, so a loader that takes it and
/// then execs the program that reads the weights keeps it until that process ends. A
/// process that the holder starts inherits the open file and holds the lock with it until
/// both have ended.
pub fn take_model_lock() -> Result<ModelLock, LockError> {
    let path = model_lock_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| LockError::Io {
            call: "create_dir_all",
            path: dir.to_path_buf(),
            source,
        })?;
    }
    let file = lock_file_options()
        .open(&path)
        .map_err(|source| LockError::Io {
            call: "open",
            path: path.clone(),
            source,
        })?;
    match file.try_lock() {
        Ok(()) => {
            keep_across_exec(&file, &path)?;
            Ok(ModelLock { _file: file, path })
        }
        Err(TryLockError::WouldBlock) => Err(LockError::Held(path)),
        Err(TryLockError::Error(source)) => Err(LockError::Io {
            call: "try_lock",
            path,
            source,
        }),
    }
}
