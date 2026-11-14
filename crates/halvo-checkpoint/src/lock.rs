use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::identity::{same_file, FileId};
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
/// each directory that holds a workbook that a call writes (contracts.md A11.3, plan.md U2.3b),
/// and one mark for each prepared copy that a live call holds. A lock file is named for its
/// directory by the SHA-256 of what names that directory on the system (on macOS its device and
/// inode numbers, on Windows its final path), so that every spelling of one directory takes one
/// lock and the name holds no value of the user; the file is empty. The system releases a lock
/// when the holder's file closes, also when the process that held it is killed. Each call
/// checks again that `agents/` and `agents/locks/` are directories and no links, and that
/// `agents/locks/` is still the directory that [`Locks::open`] opened, compared by
/// [`crate::same_file`].
#[derive(Clone)]
pub struct Locks {
    dir: PathBuf,
    wait: Duration,
    opened: FileId,
}

impl fmt::Debug for Locks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Locks")
            .field("wait", &self.wait)
            .finish_non_exhaustive()
    }
}

/// A lock of a workbook's directory, held until it is dropped.
pub struct Held {
    _file: File,
}

impl fmt::Debug for Held {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Held").finish_non_exhaustive()
    }
}

/// The mark of a file that a call made beside a workbook: `agents/locks/<64 hex>.live`, an
/// empty file of mode 0600, named by the SHA-256 of what names the file's directory on the
/// system and the file's name. A live call holds a shared lock of the system on it from before
/// it makes the file until the mark is dropped, and the drop removes it. A killed call's mark
/// stays until a later sweep takes it ([`Marked::Left`]).
pub struct Mark {
    file: Option<File>,
    path: PathBuf,
}

impl fmt::Debug for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mark").finish_non_exhaustive()
    }
}

impl Mark {
    /// Lets go of the mark and leaves its file in `agents/locks/`, as a sweep does for a file
    /// that it leaves.
    pub fn keep(mut self) {
        drop(self.file.take());
        self.path = PathBuf::new();
    }
}

impl Drop for Mark {
    fn drop(&mut self) {
        if self.file.is_some() {
            let _ = fs::remove_file(&self.path);
        }
        drop(self.file.take());
    }
}

/// What [`Locks::marked`] found for a file beside a workbook.
#[derive(Debug)]
pub enum Marked {
    /// A live call holds the file's mark.
    Live,
    /// No mark of the file stands in `agents/locks/`: no call of this data directory made it,
    /// or a call made it before marks were made.
    Unmarked,
    /// A mark that no live call holds, which a killed call left, now held by this call alone;
    /// dropping it removes its file, [`Mark::keep`] leaves it.
    Left(Mark),
}

impl Locks {
    /// Opens `agents/locks/` under the data directory `data`, making `agents/` and
    /// `agents/locks/` where they are missing. A call that takes a lock waits up to `wait` for
    /// another holder to release it, and then answers [`LockError::Busy`].
    ///
    /// Two calls exclude each other, and a sweep sees the marks of a call, only when both
    /// opened the same data directory: the sheet executor and the supervisor library open the
    /// one data directory of Halvo. The [`Locks`] remember the directory `agents/locks/` that
    /// they opened; when `agents/` or `agents/locks/` is deleted or replaced while they are
    /// kept, every later call answers [`LockError::Link`], and a new [`Locks`] must be opened.
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
        let opened = places(&dir)?;
        Ok(Locks { dir, wait, opened })
    }

    /// Takes the lock of the directory `dir`, waiting up to the wait of [`Locks::open`].
    pub fn hold(&self, dir: &Path) -> Result<Held, LockError> {
        let file = self.file(&format!("{}.lock", Sha256::of(&key(dir)?).hex()))?;
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

    /// Marks the file `name` of the directory `dir` as held by a live call until the mark is
    /// dropped: a shared lock of the system on its own file in `agents/locks/`, so that no
    /// file of the person's folder is held open. A caller marks a file under the lock of `dir`.
    pub fn mark(&self, dir: &Path, name: &OsStr) -> Result<Mark, LockError> {
        let file_name = mark_name(dir, name)?;
        let file = self.file(&file_name)?;
        match file.try_lock_shared() {
            Ok(()) => Ok(Mark {
                file: Some(file),
                path: self.dir.join(file_name),
            }),
            Err(TryLockError::WouldBlock) => Err(LockError::Busy),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    /// Whether a live call holds the mark of the file `name` of the directory `dir`, no mark
    /// stands, or a killed call left one, which the answer then holds. It checks first that
    /// `agents/locks/` is still the directory that [`Locks::open`] opened.
    pub fn marked(&self, dir: &Path, name: &OsStr) -> Result<Marked, LockError> {
        self.places()?;
        let path = self.dir.join(mark_name(dir, name)?);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(LockError::Link),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Marked::Unmarked),
            Err(error) => return Err(error.into()),
        }
        let file = open(&path, false)?;
        match file.try_lock() {
            Ok(()) => Ok(Marked::Left(Mark {
                file: Some(file),
                path,
            })),
            Err(TryLockError::WouldBlock) => Ok(Marked::Live),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    fn places(&self) -> Result<(), LockError> {
        if same_file(&places(&self.dir)?, &self.opened) {
            Ok(())
        } else {
            Err(LockError::Link)
        }
    }

    fn file(&self, name: &str) -> Result<File, LockError> {
        self.places()?;
        let path = self.dir.join(name);
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(LockError::Link);
        }
        let file = open(&path, true)?;
        let opened = file.metadata()?;
        let named = fs::symlink_metadata(&path)?;
        if opened.file_type().is_symlink() || named.file_type().is_symlink() || !opened.is_file() {
            return Err(LockError::Link);
        }
        if !same_file(&FileId::of(&file)?, &FileId::at(&path)?) {
            return Err(LockError::Link);
        }
        Ok(file)
    }
}

fn places(dir: &Path) -> Result<FileId, LockError> {
    let agents = dir.parent().unwrap_or(dir);
    for part in [agents, dir] {
        let meta = fs::symlink_metadata(part)?;
        if meta.file_type().is_symlink() {
            return Err(LockError::Link);
        }
        if !meta.is_dir() {
            return Err(LockError::Io(io::ErrorKind::NotADirectory));
        }
    }
    Ok(FileId::at(dir)?)
}

fn mark_name(dir: &Path, name: &OsStr) -> Result<String, LockError> {
    let mut key = key(dir)?;
    key.push(0);
    key.extend_from_slice(name.as_encoded_bytes());
    Ok(format!("{}.live", Sha256::of(&key).hex()))
}

#[cfg(unix)]
fn key(dir: &Path) -> Result<Vec<u8>, LockError> {
    let opened = File::open(dir)?;
    if !opened.metadata()?.is_dir() {
        return Err(LockError::Io(io::ErrorKind::NotADirectory));
    }
    let (device, inode) = FileId::of(&opened)?.numbers();
    Ok(format!("unix {device} {inode}").into_bytes())
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

fn open(path: &Path, create: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(create);
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
