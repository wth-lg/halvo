//! The instance lock of a data directory on unix (ruling PL-8 part (2)): a flock on the
//! directory's own descriptor. The daemon and the offline tools take it; the client reads it
//! after a connect that found no daemon, so that it never calls a held directory "not running".

use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;
use std::time::{Duration, Instant};

/// The step between two tries of a take that found the lock held.
pub const TAKE_STEP_MS: u64 = 50;

/// How long a take tries before it answers that the lock is held.
pub const TAKE_FOR_MS: u64 = 1_000;

/// What a read of the lock found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockRead {
    Free,
    Held,
    /// The data directory does not exist.
    NoDirectory,
}

/// The lock, held until the value is dropped. Its descriptor is close-on-exec.
#[derive(Debug)]
pub struct InstanceLock {
    directory: std::fs::File,
}

fn flock(file: &std::fs::File, operation: libc::c_int) -> std::io::Result<bool> {
    // SAFETY: the descriptor is the open file of `file`, which outlives the call; flock takes
    // no pointer.
    if unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(false)
    } else {
        Err(error)
    }
}

/// Opens the directory; std opens every descriptor with close-on-exec.
fn open(dir: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(dir)
}

impl InstanceLock {
    /// One non-blocking try of the exclusive lock: `None` when another open file holds it.
    pub fn try_take(dir: &Path) -> std::io::Result<Option<Self>> {
        let directory = open(dir)?;
        Ok(flock(&directory, libc::LOCK_EX)?.then_some(Self { directory }))
    }

    /// Takes the exclusive lock, trying again every [`TAKE_STEP_MS`] for at most
    /// [`TAKE_FOR_MS`]; `None` when it stays held for that long.
    pub fn take(dir: &Path) -> std::io::Result<Option<Self>> {
        let until = Instant::now() + Duration::from_millis(TAKE_FOR_MS);
        loop {
            if let Some(lock) = Self::try_take(dir)? {
                return Ok(Some(lock));
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            std::thread::sleep(left.min(Duration::from_millis(TAKE_STEP_MS)));
        }
    }
}

impl AsRawFd for InstanceLock {
    fn as_raw_fd(&self) -> RawFd {
        self.directory.as_raw_fd()
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = flock(&self.directory, libc::LOCK_UN);
    }
}

/// Reads the lock by a non-blocking shared flock on a descriptor of the directory that is
/// dropped at once. It never waits.
pub fn read(dir: &Path) -> std::io::Result<LockRead> {
    let directory = match open(dir) {
        Ok(directory) => directory,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LockRead::NoDirectory),
        Err(e) => return Err(e),
    };
    if flock(&directory, libc::LOCK_SH)? {
        drop(directory);
        Ok(LockRead::Free)
    } else {
        Ok(LockRead::Held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("il").tempdir().unwrap()
    }

    #[test]
    fn the_take_steps_are_50_ms_for_at_most_1_s() {
        assert_eq!(TAKE_STEP_MS, 50);
        assert_eq!(TAKE_FOR_MS, 1_000);
    }

    #[test]
    fn a_read_answers_free_held_or_no_directory() {
        let base = dir();
        assert_eq!(read(base.path()).unwrap(), LockRead::Free);
        let held = InstanceLock::take(base.path()).unwrap().unwrap();
        assert_eq!(read(base.path()).unwrap(), LockRead::Held);
        drop(held);
        assert_eq!(read(base.path()).unwrap(), LockRead::Free);
        assert_eq!(
            read(&base.path().join("missing")).unwrap(),
            LockRead::NoDirectory
        );
    }

    #[test]
    fn a_take_against_a_held_lock_answers_held_after_its_steps() {
        let base = dir();
        let _held = InstanceLock::take(base.path()).unwrap().unwrap();
        let started = Instant::now();
        assert!(InstanceLock::take(base.path()).unwrap().is_none());
        assert!(started.elapsed() >= Duration::from_millis(TAKE_FOR_MS));
        assert!(InstanceLock::try_take(base.path()).unwrap().is_none());
    }

    #[test]
    fn after_a_read_a_take_succeeds_at_once() {
        let base = dir();
        for _ in 0..20 {
            assert_eq!(read(base.path()).unwrap(), LockRead::Free);
            assert!(InstanceLock::try_take(base.path()).unwrap().is_some());
        }
    }

    #[test]
    fn a_dropped_lock_is_released() {
        let base = dir();
        drop(InstanceLock::take(base.path()).unwrap().unwrap());
        assert!(InstanceLock::try_take(base.path()).unwrap().is_some());
    }

    #[test]
    fn every_one_of_200_takes_succeeds_against_a_loop_of_reads() {
        let base = dir();
        let path = base.path().to_path_buf();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader = {
            let (path, stop) = (path.clone(), std::sync::Arc::clone(&stop));
            std::thread::spawn(move || {
                let mut reads = 0u64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    read(&path).unwrap();
                    reads += 1;
                }
                reads
            })
        };
        let mut taken = 0;
        for _ in 0..200 {
            if InstanceLock::take(&path).unwrap().is_some() {
                taken += 1;
            }
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let reads = reader.join().unwrap();
        assert_eq!(taken, 200);
        assert!(reads > 0);
    }

    #[test]
    fn the_descriptor_of_a_taken_lock_is_close_on_exec() {
        let base = dir();
        let lock = InstanceLock::take(base.path()).unwrap().unwrap();
        // SAFETY: the descriptor is the open directory of `lock`; F_GETFD takes no pointer.
        let flags = unsafe { libc::fcntl(lock.as_raw_fd(), libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }
}
