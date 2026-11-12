use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::Sha256;

/// Why a call could not take the lock of a workbook's directory. It then touched no file
/// beside the workbook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockError {
    /// Another call held the lock for the whole wait.
    Busy,
    /// A link stands in the place of `agents/`, `agents/locks/` or the lock file.
    Link,
    /// The system answered an error of this kind: the data directory is not a directory,
    /// `agents/locks/` or the lock file cannot be made or opened, or the workbook's directory
    /// cannot be read.
    Io(io::ErrorKind),
}

impl From<io::Error> for LockError {
    fn from(error: io::Error) -> LockError {
        LockError::Io(error.kind())
    }
}

/// The directory `agents/locks/` under Halvo's data directory, which holds one lock file for
/// each directory that holds a workbook that a call writes (contracts.md A11.3, plan.md U2.3b).
/// A lock file is named for its directory by the SHA-256 of what names that directory on the
/// system (on macOS its device and inode numbers, on Windows its final path), so that every
/// spelling of one directory takes one lock and the name holds no value of the user; the file
/// is empty. The system releases a lock when the holder's file closes, also when the process
/// that held it is killed.
#[derive(Clone)]
pub struct Locks {
    dir: PathBuf,
    wait: Duration,
}

impl fmt::Debug for Locks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Locks")
            .field("wait", &self.wait)
            .finish_non_exhaustive()
    }
}

/// A lock of a workbook's directory, held until it is dropped.
#[derive(Debug)]
pub struct Held {
    _file: File,
}

impl Locks {
    /// Opens `agents/locks/` under the data directory `data`, making `agents/` and
    /// `agents/locks/` where they are missing. A call that takes a lock waits up to `wait` for
    /// another holder to release it, and then answers [`LockError::Busy`].
    pub fn open(data: &Path, wait: Duration) -> Result<Locks, LockError> {
        if !fs::metadata(data)?.is_dir() {
            return Err(LockError::Io(io::ErrorKind::NotADirectory));
        }
        let agents = data.join("agents");
        let dir = agents.join("locks");
        for part in [&agents, &dir] {
            match fs::create_dir(part) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
            let meta = fs::symlink_metadata(part)?;
            if meta.file_type().is_symlink() {
                return Err(LockError::Link);
            }
            if !meta.is_dir() {
                return Err(LockError::Io(io::ErrorKind::NotADirectory));
            }
        }
        Ok(Locks { dir, wait })
    }

    /// Takes the lock of the directory `dir`, waiting up to the wait of [`Locks::open`].
    pub fn hold(&self, dir: &Path) -> Result<Held, LockError> {
        let path = self
            .dir
            .join(format!("{}.lock", Sha256::of(&key(dir)?).hex()));
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(LockError::Link);
        }
        let file = open(&path)?;
        let opened = file.metadata()?;
        let named = fs::symlink_metadata(&path)?;
        if opened.file_type().is_symlink() || named.file_type().is_symlink() || !opened.is_file() {
            return Err(LockError::Link);
        }
        if !same_file(&opened, &named) {
            return Err(LockError::Link);
        }
        let deadline = Instant::now() + self.wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Held { _file: file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(TryLockError::WouldBlock) => return Err(LockError::Busy),
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }
}

#[cfg(unix)]
fn key(dir: &Path) -> Result<Vec<u8>, LockError> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(dir)?;
    if !meta.is_dir() {
        return Err(LockError::Io(io::ErrorKind::NotADirectory));
    }
    Ok(format!("unix {} {}", meta.dev(), meta.ino()).into_bytes())
}

#[cfg(windows)]
fn key(dir: &Path) -> Result<Vec<u8>, LockError> {
    use std::os::windows::ffi::OsStrExt;
    let full = fs::canonicalize(dir)?;
    if !fs::metadata(&full)?.is_dir() {
        return Err(LockError::Io(io::ErrorKind::NotADirectory));
    }
    let mut key = b"windows ".to_vec();
    for unit in full.as_os_str().encode_wide() {
        key.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(key)
}

fn open(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        #[cfg(target_os = "macos")]
        options.custom_flags(0x0100);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    options.open(path)
}

#[cfg(unix)]
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    (a.dev(), a.ino()) == (b.dev(), b.ino())
}

#[cfg(not(unix))]
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.is_file() && b.is_file()
}
