//! The instance lock of a data directory (contracts.md C3.2 step 3 and C3.5).
//! On macOS it is halvo-control's flock on the directory's own descriptor,
//! held for the life of the process and unlocked on drop, so no file in the
//! directory can be unlinked to free it. On Windows it is the lock file
//! daemon.lock, opened without delete sharing; win-5's service run takes it.

#[cfg(target_os = "macos")]
pub use macos::{InstanceLock, BACKUP_EXCLUSION_ATTRIBUTE};
#[cfg(windows)]
pub use windows::InstanceLock;

#[cfg(target_os = "macos")]
mod macos {
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;
    use std::path::PathBuf;

    use halvo_control::instance_lock;
    use halvo_control::paths::{check_data_dir, DataDir, DataDirCheck, DataDirFault};

    /// The extended attribute that tells Time Machine to copy nothing of a
    /// directory.
    pub const BACKUP_EXCLUSION_ATTRIBUTE: &str = "com.apple.metadata:com_apple_backup_excludeItem";

    const BACKUP_EXCLUSION_VALUE: [u8; 61] = [
        0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0x5f, 0x10, 0x11, 0x63, 0x6f, 0x6d, 0x2e,
        0x61, 0x70, 0x70, 0x6c, 0x65, 0x2e, 0x62, 0x61, 0x63, 0x6b, 0x75, 0x70, 0x64, 0x08, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x1c,
    ];

    /// The instance lock of one data directory, held until the value drops.
    #[derive(Debug)]
    pub struct InstanceLock(instance_lock::InstanceLock);

    impl InstanceLock {
        /// Takes the lock with halvo-control's take: one exclusive flock on
        /// the directory's own descriptor, tried again every TAKE_STEP_MS for
        /// at most TAKE_FOR_MS, so that a reader's shared flock never makes a
        /// start give up; `None` when another process holds it for that long.
        /// The descriptor is close-on-exec.
        pub fn take(dir: &DataDir) -> io::Result<Option<InstanceLock>> {
            Ok(instance_lock::InstanceLock::take(dir)?.map(InstanceLock))
        }

        /// The check of the data directory on the lock's own descriptor
        /// (halvo-control's check_data_dir).
        pub fn check(&self, dir: &DataDir) -> Result<DataDirCheck, DataDirFault> {
            check_data_dir(dir, Some(&self.0))
        }

        /// The folder that the lock's descriptor holds, as the system names it
        /// now: the folder that a link at the data directory's name leads to,
        /// read on the descriptor and never by the name again.
        pub fn folder(&self) -> io::Result<PathBuf> {
            let mut buf = vec![0u8; libc::PATH_MAX as usize];
            if unsafe { libc::fcntl(self.0.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) } == -1 {
                return Err(io::Error::last_os_error());
            }
            let text = std::ffi::CStr::from_bytes_until_nul(&buf)
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
            Ok(PathBuf::from(std::ffi::OsStr::from_bytes(text.to_bytes())))
        }

        /// Sets [`BACKUP_EXCLUSION_ATTRIBUTE`] on the directory through the
        /// lock's descriptor, so that it lands on the folder that the lock
        /// holds. One attribute write: it runs no tool and cannot wait on one.
        /// The mark stops Time Machine from copying the data directory; it
        /// covers no sync client, no other backup tool, no Migration Assistant
        /// and no copy by hand, and it does not undo a copy made before it.
        pub fn exclude_from_backup(&self) -> io::Result<()> {
            let name = std::ffi::CString::new(BACKUP_EXCLUSION_ATTRIBUTE)
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
            let set = unsafe {
                libc::fsetxattr(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    BACKUP_EXCLUSION_VALUE.as_ptr().cast(),
                    BACKUP_EXCLUSION_VALUE.len(),
                    0,
                    0,
                )
            };
            if set == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        #[cfg(test)]
        pub(crate) fn raw_fd(&self) -> std::os::unix::io::RawFd {
            self.0.as_raw_fd()
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};

    use halvo_control::paths::DataDir;

    const ERROR_SHARING_VIOLATION: i32 = 32;
    const FILE_SHARE_READ: u32 = 0x1;

    /// The lock file DIR/daemon.lock, open with no write and no delete
    /// sharing while the value lives.
    #[derive(Debug)]
    pub struct InstanceLock {
        _held: std::fs::File,
    }

    impl InstanceLock {
        /// Opens DIR/daemon.lock with read sharing alone, tried again every
        /// 50 ms for at most 1 s; `None` when another open holds it for that
        /// long.
        pub fn take(dir: &DataDir) -> io::Result<Option<InstanceLock>> {
            let until = Instant::now() + Duration::from_millis(1_000);
            loop {
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .share_mode(FILE_SHARE_READ)
                    .open(dir.instance_lock_file())
                {
                    Ok(file) => return Ok(Some(InstanceLock { _held: file })),
                    Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {}
                    Err(error) => return Err(error),
                }
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Ok(None);
                }
                std::thread::sleep(left.min(Duration::from_millis(50)));
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #[cfg(all(target_os = "macos", debug_assertions))]
    pub(crate) use on_macos::{backup_mark, kept_dir};

    #[cfg(target_os = "macos")]
    mod on_macos {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::os::unix::ffi::OsStrExt;
        use std::path::{Path, PathBuf};
        use std::process::{Child, Command, Stdio};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        use halvo_control::instance_lock::{self, LockRead};
        use halvo_control::paths::DataDir;

        use super::super::{InstanceLock, BACKUP_EXCLUSION_ATTRIBUTE};

        const HOLD: &str = "HALVO_DAEMON_INSTANCE_HOLD";
        const HOLDER: &str = "daemon::instance::tests::on_macos::hold_for_parent";

        pub(crate) fn kept_dir(prefix: &str) -> PathBuf {
            let target = std::env::current_exe()
                .unwrap()
                .ancestors()
                .nth(3)
                .unwrap()
                .join("tmp");
            std::fs::create_dir_all(&target).unwrap();
            tempfile::Builder::new()
                .prefix(prefix)
                .tempdir_in(target)
                .unwrap()
                .keep()
        }

        fn data_dir(path: &Path) -> DataDir {
            DataDir::from_flag(path.to_str().unwrap()).unwrap()
        }

        fn flock_file(path: &Path) -> Option<std::fs::File> {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .unwrap();
            let held = unsafe {
                libc::flock(
                    std::os::unix::io::AsRawFd::as_raw_fd(&file),
                    libc::LOCK_EX | libc::LOCK_NB,
                )
            };
            (held == 0).then_some(file)
        }

        #[test]
        fn hold_for_parent() {
            let Some(dir) = std::env::var_os(HOLD) else {
                return;
            };
            let dir = PathBuf::from(dir);
            let lock = InstanceLock::take(&data_dir(&dir)).unwrap().unwrap();
            let file = flock_file(&dir.join("daemon.lock")).unwrap();
            let mut out = std::io::stdout().lock();
            writeln!(out, "instance-holder: held").unwrap();
            out.flush().unwrap();
            let mut rest = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut rest);
            drop((lock, file));
        }

        struct Holder {
            child: Child,
            out: std::io::Lines<BufReader<std::process::ChildStdout>>,
        }

        fn holder(dir: &Path) -> Holder {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", HOLDER, "--nocapture", "--test-threads=1"])
                .env(HOLD, dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
            let started = Instant::now();
            loop {
                let line = out.next().expect("the holder ended early").unwrap();
                if line.contains("instance-holder: held") {
                    break;
                }
                assert!(started.elapsed() < Duration::from_secs(60));
            }
            Holder { child, out }
        }

        fn release(mut holder: Holder) {
            drop(holder.child.stdin.take());
            for line in holder.out.by_ref() {
                line.unwrap();
            }
            assert!(holder.child.wait().unwrap().success(), "the holder failed");
        }

        #[test]
        fn a_second_holder_is_refused_while_files_in_the_directory_are_unlinked() {
            let dir = kept_dir("instance-tests-");
            let child = holder(&dir);
            let lock_file = dir.join("daemon.lock");
            std::fs::remove_file(&lock_file).unwrap();
            let by_a_lock_file = flock_file(&lock_file);
            assert!(
                by_a_lock_file.is_some(),
                "a lock file freed by its unlink was not taken again"
            );
            drop(by_a_lock_file);
            let named = data_dir(&dir);
            for _ in 0..50 {
                std::fs::write(&lock_file, b"").unwrap();
                std::fs::remove_file(&lock_file).unwrap();
                assert!(
                    instance_lock::InstanceLock::try_take(&named)
                        .unwrap()
                        .is_none(),
                    "a second holder took the directory's lock"
                );
            }
            assert!(InstanceLock::take(&named).unwrap().is_none());
            assert_eq!(instance_lock::read(&named).unwrap(), LockRead::Held);
            release(child);
            assert!(InstanceLock::take(&named).unwrap().is_some());
        }

        #[test]
        fn a_loop_of_lock_reads_never_makes_a_take_answer_held() {
            let dir = kept_dir("instance-tests-");
            let named = data_dir(&dir);
            let done = Arc::new(AtomicBool::new(false));
            let reads = {
                let done = done.clone();
                let named = named.clone();
                std::thread::spawn(move || {
                    let mut count = 0u64;
                    while !done.load(Ordering::SeqCst) {
                        let _ = instance_lock::read(&named).unwrap();
                        count += 1;
                    }
                    count
                })
            };
            for round in 0..300 {
                assert!(
                    InstanceLock::take(&named).unwrap().is_some(),
                    "take {round} answered held while only reads ran"
                );
            }
            done.store(true, Ordering::SeqCst);
            assert!(reads.join().unwrap() > 0);
        }

        #[test]
        fn the_lock_descriptor_is_closed_on_exec() {
            let dir = kept_dir("instance-tests-");
            let lock = InstanceLock::take(&data_dir(&dir)).unwrap().unwrap();
            let flags = unsafe { libc::fcntl(lock.raw_fd(), libc::F_GETFD) };
            assert!(flags >= 0);
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
        }

        #[test]
        fn the_check_reads_the_lock_descriptor_and_not_the_name() {
            let root = kept_dir("instance-tests-");
            let dir = root.join("data");
            std::fs::create_dir(&dir).unwrap();
            let named = data_dir(&dir);
            let lock = InstanceLock::take(&named).unwrap().unwrap();
            std::fs::rename(&dir, root.join("moved")).unwrap();
            assert!(
                matches!(
                    lock.check(&named),
                    Ok(halvo_control::paths::DataDirCheck::Own { .. })
                ),
                "{:?}",
                lock.check(&named)
            );
            assert_eq!(
                halvo_control::paths::check_data_dir(&named, None),
                Ok(halvo_control::paths::DataDirCheck::Missing)
            );
            assert_eq!(
                lock.folder().unwrap(),
                root.join("moved").canonicalize().unwrap()
            );
        }

        pub(crate) fn backup_mark(path: &Path, follow: bool) -> Option<Vec<u8>> {
            let name = std::ffi::CString::new(BACKUP_EXCLUSION_ATTRIBUTE).unwrap();
            let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            let options = if follow { 0 } else { libc::XATTR_NOFOLLOW };
            let mut value = vec![0u8; 256];
            let read = unsafe {
                libc::getxattr(
                    path.as_ptr(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                    0,
                    options,
                )
            };
            if read < 0 {
                return None;
            }
            value.truncate(read as usize);
            Some(value)
        }

        #[test]
        fn the_backup_mark_lands_on_the_folder_that_the_lock_holds() {
            let root = kept_dir("instance-tests-");
            let own = root.join("own");
            std::fs::create_dir(&own).unwrap();
            let target = root.join("target");
            std::fs::create_dir(&target).unwrap();
            let link = root.join("link");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            for dir in [&own, &link] {
                let lock = InstanceLock::take(&data_dir(dir)).unwrap().unwrap();
                lock.exclude_from_backup().unwrap();
            }
            let tmutil = b"bplist00_\x10\x11com.apple.backupd";
            let own_mark = backup_mark(&own, false).unwrap();
            assert!(own_mark.starts_with(tmutil));
            assert_eq!(backup_mark(&target, false), Some(own_mark));
            assert_eq!(backup_mark(&link, false), None);
            let held = InstanceLock::take(&data_dir(&link)).unwrap().unwrap();
            assert_eq!(
                held.folder().unwrap(),
                target.canonicalize().unwrap(),
                "the folder was not read on the descriptor"
            );
        }
    }

    #[cfg(windows)]
    mod on_windows {
        use std::time::{Duration, Instant};

        use halvo_control::paths::DataDir;

        use super::super::InstanceLock;

        #[test]
        fn a_second_holder_is_refused_while_the_lock_file_is_held() {
            let dir = tempfile::tempdir().unwrap();
            let named = DataDir::from_flag(dir.path().to_str().unwrap()).unwrap();
            let held = InstanceLock::take(&named).unwrap().unwrap();
            let started = Instant::now();
            assert!(InstanceLock::take(&named).unwrap().is_none());
            assert!(started.elapsed() >= Duration::from_millis(900));
            assert!(std::fs::remove_file(named.instance_lock_file()).is_err());
            assert!(std::fs::OpenOptions::new()
                .write(true)
                .open(named.instance_lock_file())
                .is_err());
            drop(held);
            assert!(InstanceLock::take(&named).unwrap().is_some());
        }
    }
}
