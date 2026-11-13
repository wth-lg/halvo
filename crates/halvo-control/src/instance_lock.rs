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

/// Opens the directory with `O_DIRECTORY` and `O_NONBLOCK`, so that anything at `dir` that is
/// no directory (a FIFO, a file, a socket, a link to one of them) is an error at once and the
/// open never waits; std adds close-on-exec.
fn open(dir: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK)
        .open(dir)
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

/// Test helpers for an entry that is no directory at a data directory's path.
#[cfg(test)]
pub(crate) mod planted {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    /// How long a call on a planted entry may take before the test calls it blocked. The call
    /// is expected to return at once; 10 s leaves a hosted runner that holds a thread for
    /// seconds far inside the bound.
    pub(crate) const BOUND: Duration = Duration::from_secs(10);

    pub(crate) fn mkfifo(path: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: c_path is a live NUL-terminated path; mkfifo takes no other pointer.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    }

    /// While a thread may be blocked in an open of the FIFO at `fifo`, this opens the FIFO for
    /// writing, which ends such an open, and waits for the thread, each with a bound, on every
    /// path out of the test.
    struct Unblock {
        fifo: PathBuf,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Drop for Unblock {
        fn drop(&mut self) {
            use std::os::unix::fs::OpenOptionsExt;
            let Some(thread) = self.thread.take() else {
                return;
            };
            let until = Instant::now() + BOUND;
            let mut writer = None;
            while !thread.is_finished() && Instant::now() < until {
                if writer.is_none() {
                    writer = std::fs::OpenOptions::new()
                        .write(true)
                        .custom_flags(libc::O_NONBLOCK)
                        .open(&self.fifo)
                        .ok();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(writer);
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }

    /// A directory of mode 000 that the test made; its mode goes back to 0o700 when the value is
    /// dropped, so that the test's temporary directory can be removed on every path.
    pub(crate) struct Closed(PathBuf);

    impl Closed {
        pub(crate) fn make(path: &Path) -> Self {
            use std::os::unix::fs::PermissionsExt;
            std::fs::create_dir(path).unwrap();
            let closed = Self(path.to_path_buf());
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
            assert!(
                std::fs::read_dir(path).is_err(),
                "a directory of mode 000 can be read: the test runs with rights that pass the mode"
            );
            closed
        }
    }

    impl Drop for Closed {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
        }
    }

    /// The answer of `call` on `path`, from a thread of its own, or `None` when it has not
    /// answered within [`BOUND`]. `fifo` names a FIFO that the guard opens for writing to end
    /// a blocked open; it may be `path` itself or the FIFO that a link at `path` names.
    pub(crate) fn answer_within<T: Send + 'static>(
        path: &Path,
        fifo: &Path,
        call: impl FnOnce(&Path) -> T + Send + 'static,
    ) -> Option<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        let at = path.to_path_buf();
        let thread = std::thread::spawn(move || {
            let _ = tx.send(call(&at));
        });
        let _guard = Unblock {
            fifo: fifo.to_path_buf(),
            thread: Some(thread),
        };
        rx.recv_timeout(BOUND).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("il").tempdir().unwrap()
    }

    /// What sits at the data directory's path, and the FIFO that a blocked open waits on.
    fn plant(
        base: &Path,
        kind: &str,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        Option<std::os::unix::net::UnixListener>,
    ) {
        let path = base.join("d");
        let fifo = base.join("f");
        planted::mkfifo(&fifo);
        let mut listener = None;
        match kind {
            "a FIFO" => {
                std::fs::rename(&fifo, &path).unwrap();
                return (path.clone(), path, None);
            }
            "a regular file" => std::fs::write(&path, b"x").unwrap(),
            "a socket" => listener = Some(std::os::unix::net::UnixListener::bind(&path).unwrap()),
            "a link to a FIFO" => std::os::unix::fs::symlink(&fifo, &path).unwrap(),
            _ => unreachable!(),
        }
        (path, fifo, listener)
    }

    #[test]
    fn an_entry_that_is_no_directory_at_the_data_directory_path_is_an_error_at_once() {
        for kind in ["a FIFO", "a regular file", "a socket", "a link to a FIFO"] {
            let base = dir();
            let (path, fifo, _listener) = plant(base.path(), kind);
            let read = planted::answer_within(&path, &fifo, |p| read(p).map_err(|e| e.kind()));
            let try_take = planted::answer_within(&path, &fifo, |p| {
                InstanceLock::try_take(p)
                    .map(|l| l.is_some())
                    .map_err(|e| e.kind())
            });
            let take = planted::answer_within(&path, &fifo, |p| {
                InstanceLock::take(p)
                    .map(|l| l.is_some())
                    .map_err(|e| e.kind())
            });
            for (call, answer) in [
                ("read", read.map(|a| a.map(|r| format!("{r:?}")))),
                ("try_take", try_take.map(|a| a.map(|t| format!("{t:?}")))),
                ("take", take.map(|a| a.map(|t| format!("{t:?}")))),
            ] {
                let Some(answer) = answer else {
                    panic!("{call} on {kind} at the data directory's path did not return within {:?}; PL-8 (2)(d) says it never waits", planted::BOUND);
                };
                assert!(
                    matches!(answer, Err(k) if k != std::io::ErrorKind::NotFound),
                    "{call} on {kind}: {answer:?}; anything that is no directory is an error, never NoDirectory, Free or a lock"
                );
            }
        }
    }

    #[test]
    fn a_read_of_a_directory_that_cannot_be_opened_is_an_error_and_never_no_directory() {
        let base = dir();
        let path = base.path().join("d");
        let _closed = planted::Closed::make(&path);
        assert_eq!(
            read(&path).map_err(|e| e.kind()),
            Err(std::io::ErrorKind::PermissionDenied),
            "an open error that is no missing directory is an error of the read (PL-8 (2)(b))"
        );
    }

    #[test]
    fn two_reads_side_by_side_both_answer_free() {
        let base = dir();
        let under_way = open(base.path()).unwrap();
        assert!(flock(&under_way, libc::LOCK_SH).unwrap());
        assert_eq!(
            read(base.path()).unwrap(),
            LockRead::Free,
            "beside the shared lock that another read holds while it is under way, a read \
             answers free: the read's flock is shared"
        );
        drop(under_way);

        const READS: usize = 2_000;
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let readers: Vec<_> = (0..2)
            .map(|_| {
                let path = base.path().to_path_buf();
                let start = std::sync::Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    (0..READS)
                        .filter(|_| read(&path).unwrap() != LockRead::Free)
                        .count()
                })
            })
            .collect();
        let held: usize = readers.into_iter().map(|t| t.join().unwrap()).sum();
        assert_eq!(
            held, 0,
            "two threads that only read, {READS} reads each: {held} answered held"
        );
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
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let reads = std::sync::Arc::new(AtomicU64::new(0));
        let reader = {
            let (path, stop, reads) = (
                path.clone(),
                std::sync::Arc::clone(&stop),
                std::sync::Arc::clone(&reads),
            );
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    read(&path).unwrap();
                    reads.fetch_add(1, Ordering::Relaxed);
                }
            })
        };
        let until = Instant::now() + Duration::from_secs(10);
        while reads.load(Ordering::Relaxed) == 0 {
            assert!(Instant::now() < until, "the reader made no read in 10 s");
            std::thread::yield_now();
        }
        let before = reads.load(Ordering::Relaxed);
        let mut retried = 0;
        for i in 0..200 {
            let mut takes = 1;
            while InstanceLock::take(&path).unwrap().is_none() {
                takes += 1;
                if takes > 10 {
                    stop.store(true, Ordering::Relaxed);
                    panic!(
                        "take {i} found the lock held through 10 takes of up to 1 s each: a read \
                         holds its shared lock only for one flock call, so 10 s leaves a hosted \
                         runner that holds the reading thread for seconds far inside the bound"
                    );
                }
            }
            retried += takes - 1;
        }
        let during = reads.load(Ordering::Relaxed) - before;
        stop.store(true, Ordering::Relaxed);
        reader.join().unwrap();
        println!("reads while the 200 takes ran: {during}; takes tried again: {retried}");
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
