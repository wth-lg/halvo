use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

use halvo_control::paths::DataDir;

/// What a launch found at the lock of its data directory.
#[derive(Debug)]
pub enum Taken {
    /// This launch holds the lock in the file: it is the one Halvo of the directory while the
    /// file stays open, and the lock ends with its process, however that ends.
    First(File),
    /// Another Halvo holds the lock: this launch makes no builder, no tray, no child and no
    /// poller.
    Second,
}

/// Takes the one lock of `dir` (contracts.md C14 item 8), before the builder runs: makes the
/// data directory where it is missing (mode 0700), run/ (mode 0700) and run/app.lock (mode
/// 0600), and nothing else, then takes the lock of that file without waiting.
pub fn take(dir: &DataDir) -> io::Result<Taken> {
    if std::fs::symlink_metadata(dir.path())
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        private_dirs(dir.path())?;
    }
    match private_dir(&dir.run_dir()) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let file = lock_file(&dir.app_lock())?;
    match file.try_lock() {
        Ok(()) => Ok(Taken::First(file)),
        Err(TryLockError::WouldBlock) => Ok(Taken::Second),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

fn lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Makes the directory `path` with mode 0700, its parent being there.
#[cfg(unix)]
pub fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

/// Makes the directory `path`, its parent being there.
#[cfg(not(unix))]
pub fn private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

#[cfg(unix)]
fn private_dirs(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn private_dirs(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}
