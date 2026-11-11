use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

/// The model lock (contracts.md C23.12), held while this value lives.
///
/// The lock is an exclusive lock of the operating system on the open lock file, and it
/// belongs to the process that took it. On both systems that is the process that loads the
/// weights: a wrapper that starts a loader takes no lock, and no process hands a loaded
/// model to a child. A process that the holder starts with an exec, as every
/// `std::process::Command` does, does not inherit the lock, so the lock is free when the
/// holder has ended, also when it is killed and whatever it started, and no process but the
/// holder can release it. A child that a fork alone makes, with no exec, shares the open
/// file and can release the lock or keep it after the holder has ended; a loader makes no
/// such child. A holder that becomes the loader by an exec first calls
/// `ModelLock::make_inheritable` (macOS), and the process that it becomes holds the lock
/// until it ends or releases it. On Windows nothing hands the lock on.
///
/// The lock is free again when this value is dropped, so a loader binds it to a named
/// variable and keeps it until it exits; a caller who drops it at once gets a warning.
///
/// ```no_run
/// fn load() -> Result<(), halvo_cap::LockError> {
///     let lock = halvo_cap::take_model_lock()?;
///     println!("loading under {}", lock.path().display());
///     Ok(())
/// }
/// ```
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// fn load() -> Result<(), halvo_cap::LockError> {
///     halvo_cap::take_model_lock()?;
///     Ok(())
/// }
/// ```
#[derive(Debug)]
#[must_use = "the model lock is free again when this value is dropped: bind it to a named variable"]
pub struct ModelLock {
    _file: File,
    path: PathBuf,
}

impl ModelLock {
    /// The path of the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lets the program that this process becomes by an exec keep the lock: it clears the
    /// close-on-exec flag of the lock's descriptor. It serves one case, a holder that becomes
    /// the loader by an exec, which calls it just before the exec. What else it lets happen:
    /// every process that this process, or the program that it becomes, starts after the
    /// call inherits the lock too, the lock is then held until the last of them has ended or
    /// one of them releases it, which frees it for all of them, and a process that one of
    /// them starts inherits it in turn; a loader starts no process. The call exists on macOS,
    /// where an exec exists; on Windows nothing hands the lock on.
    #[cfg(target_os = "macos")]
    pub fn make_inheritable(&self) -> Result<(), LockError> {
        use std::os::fd::AsRawFd;
        let fd = self._file.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0 {
            return Err(LockError::Io {
                call: "fcntl",
                path: self.path.clone(),
                source: io::Error::last_os_error(),
            });
        }
        Ok(())
    }
}

/// Why the model lock was not taken.
#[derive(Debug)]
pub enum LockError {
    /// The model lock is held, by another process or by this one: the loader loads nothing,
    /// exits with a non-zero status, and its caller starts it again after the holder has
    /// ended.
    Held(PathBuf),
    /// The variable that names the user's cache directory is not set to an absolute path:
    /// it is missing, empty or relative. `HOME` on macOS, `LOCALAPPDATA` on Windows.
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
            LockError::Held(path) => write!(f, "the model lock is held: {}", path.display()),
            LockError::NoCacheDir(var) => {
                write!(
                    f,
                    "no model lock: the variable {var} is not set to an absolute path"
                )
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
/// and `%LOCALAPPDATA%\halvo\model.lock` on Windows, with the variable read from the
/// environment of this process as its loader gave it. A value that is missing, empty or no
/// absolute path gives [`LockError::NoCacheDir`], which names the variable.
#[cfg(any(target_os = "macos", windows))]
pub fn model_lock_path() -> Result<PathBuf, LockError> {
    match std::env::var_os(CACHE_VAR) {
        Some(base) if Path::new(&base).is_absolute() => Ok(lock_path_in(Path::new(&base))),
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

/// Takes the model lock at [`model_lock_path`] without waiting.
///
/// Creates the directory and the empty lock file where they are missing, writes nothing
/// into the file and never removes it. Returns [`LockError::Held`] at once when the lock is
/// held, by another process or by this one.
///
/// The lock is an exclusive lock of the operating system on the open lock file, and it
/// belongs to the process that took it. On both systems that is the process that loads the
/// weights: a wrapper that starts a loader takes no lock, and no process hands a loaded
/// model to a child. A process that the holder starts with an exec, as every
/// `std::process::Command` does, does not inherit the lock, so the lock is free when the
/// holder has ended, also when it is killed and whatever it started, and no process but the
/// holder can release it. A child that a fork alone makes, with no exec, shares the open
/// file and can release the lock or keep it after the holder has ended; a loader makes no
/// such child. A holder that becomes the loader by an exec first calls
/// `ModelLock::make_inheritable` (macOS), and the process that it becomes holds the lock
/// until it ends or releases it. On Windows nothing hands the lock on.
///
/// The lock is free again when the returned value is dropped, so a loader binds it to a
/// named variable and keeps it until it exits.
///
/// On Windows the file is opened without delete sharing, so while it is held neither the
/// file nor its directory can be renamed or removed. On macOS they can: a lock file or
/// directory that another program moves away while the lock is held lets the next loader
/// create a new file and take the lock.
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
        Ok(()) => Ok(ModelLock { _file: file, path }),
        Err(TryLockError::WouldBlock) => Err(LockError::Held(path)),
        Err(TryLockError::Error(source)) => Err(LockError::Io {
            call: "try_lock",
            path,
            source,
        }),
    }
}
