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
/// The lock is free again when the value that holds it is dropped, so a loader binds that
/// value to a named variable and keeps it until it exits; a caller who drops it at once gets
/// a warning.
///
/// On Windows the file is opened without delete sharing, so while it is held neither the
/// file nor its directory can be renamed or removed. On macOS they can: a lock file or
/// directory that another program moves away while the lock is held lets the next loader
/// create a new file and take the lock.
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
/// The lock is free again when the value that holds it is dropped, so a loader binds that
/// value to a named variable and keeps it until it exits; a caller who drops it at once gets
/// a warning.
///
/// On Windows the file is opened without delete sharing, so while it is held neither the
/// file nor its directory can be renamed or removed. On macOS they can: a lock file or
/// directory that another program moves away while the lock is held lets the next loader
/// create a new file and take the lock.
pub fn take_model_lock() -> Result<ModelLock, LockError> {
    take_lock_at(model_lock_path()?)
}

fn take_lock_at(path: PathBuf) -> Result<ModelLock, LockError> {
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

#[cfg(all(test, any(target_os = "macos", windows)))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Lines, Read, Write};
    use std::process::{Child, ChildStdout, Command, Stdio};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    const LOADER: &str = "lock::tests::loader";

    fn arg(key: &str) -> Option<String> {
        let prefix = format!("{key}=");
        std::env::args().find_map(|a| a.strip_prefix(&prefix).map(str::to_owned))
    }

    fn loader_args(command: &mut Command, role: &str, dir: &Path) {
        command
            .args(["--ignored", "--exact", LOADER, "--nocapture", "--quiet"])
            .arg(format!("halvo-role={role}"))
            .arg(format!("halvo-dir={}", dir.display()));
    }

    #[test]
    #[ignore = "a loader process that the other tests start"]
    fn loader() {
        let (Some(role), Some(dir)) = (arg("halvo-role"), arg("halvo-dir")) else {
            return;
        };
        let dir = PathBuf::from(dir);
        let take = || take_lock_at(lock_path_in(&dir));
        let mut out = io::stdout();
        let code = match role.as_str() {
            #[cfg(unix)]
            "hold-then-exec" => match take() {
                Ok(lock) => {
                    use std::os::unix::process::CommandExt;
                    writeln!(out, "held {}", lock.path().display()).unwrap();
                    out.flush().unwrap();
                    if let Err(e) = lock.make_inheritable() {
                        writeln!(out, "error {e}").unwrap();
                        std::process::exit(4);
                    }
                    let error = Command::new("/bin/sh")
                        .args(["-c", "echo execd; exec /bin/sleep 60"])
                        .exec();
                    writeln!(out, "error {error}").unwrap();
                    4
                }
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            },
            "start-child-and-exit" => match take() {
                Ok(lock) => match start_grandchild("wait-stdin", &dir) {
                    Ok(_) => {
                        writeln!(out, "held {}", lock.path().display()).unwrap();
                        0
                    }
                    Err(e) => {
                        writeln!(out, "error {e}").unwrap();
                        4
                    }
                },
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            },
            #[cfg(target_os = "macos")]
            "hand-on-start-child-and-exit" => match take() {
                Ok(lock) => match lock
                    .make_inheritable()
                    .map_err(|e| io::Error::other(e.to_string()))
                    .and_then(|()| start_grandchild("wait-stdin", &dir))
                {
                    Ok(_) => {
                        writeln!(out, "held {}", lock.path().display()).unwrap();
                        0
                    }
                    Err(e) => {
                        writeln!(out, "error {e}").unwrap();
                        4
                    }
                },
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            },
            "wait-stdin" => {
                let mut rest = Vec::new();
                io::stdin().read_to_end(&mut rest).unwrap();
                0
            }
            #[cfg(target_os = "macos")]
            "hold-and-start-unlocker" => match take() {
                Ok(lock) => {
                    let ended =
                        start_grandchild("unlock-inherited", &dir).and_then(|mut c| c.wait());
                    writeln!(out, "unlocker {ended:?}").unwrap();
                    writeln!(out, "held {}", lock.path().display()).unwrap();
                    out.flush().unwrap();
                    let mut rest = Vec::new();
                    io::stdin().read_to_end(&mut rest).unwrap();
                    0
                }
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            },
            #[cfg(target_os = "macos")]
            "unlock-inherited" => {
                for fd in 3..256 {
                    unsafe { libc::flock(fd, libc::LOCK_UN) };
                }
                0
            }
            "take-twice" => match take() {
                Ok(_first) => match take() {
                    Ok(_) => {
                        writeln!(out, "second taken").unwrap();
                        0
                    }
                    Err(e) => {
                        writeln!(out, "second {e}").unwrap();
                        3
                    }
                },
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            },
            "hold" => match take() {
                Ok(lock) => {
                    writeln!(out, "held {}", lock.path().display()).unwrap();
                    out.flush().unwrap();
                    let mut rest = Vec::new();
                    io::stdin().read_to_end(&mut rest).unwrap();
                    0
                }
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            },
            "try" => {
                let begun = Instant::now();
                let taken = take();
                let ms = begun.elapsed().as_millis();
                match taken {
                    Ok(lock) => {
                        writeln!(out, "taken {}", lock.path().display()).unwrap();
                        0
                    }
                    Err(LockError::Held(path)) => {
                        writeln!(out, "held-refused ms={ms} {}", path.display()).unwrap();
                        3
                    }
                    Err(e) => {
                        writeln!(out, "error {e}").unwrap();
                        4
                    }
                }
            }
            _ => 6,
        };
        out.flush().unwrap();
        std::process::exit(code);
    }

    fn start_grandchild(role: &str, dir: &Path) -> io::Result<Child> {
        let mut command = Command::new(std::env::current_exe()?);
        loader_args(&mut command, role, dir);
        command.stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    }

    struct Started(Child);

    impl Started {
        fn end(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Drop for Started {
        fn drop(&mut self) {
            self.end();
        }
    }

    struct Holder {
        child: Started,
        #[cfg_attr(not(unix), allow(dead_code))]
        lines: Lines<BufReader<ChildStdout>>,
    }

    fn fresh_dir(name: &str) -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let target = exe.ancestors().nth(3).unwrap();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = target
            .join("tmp")
            .join("halvo-cap-lock")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn loader_command(role: &str, dir: &Path) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        loader_args(&mut command, role, dir);
        command.stdin(Stdio::null()).stderr(Stdio::inherit());
        command
    }

    fn loader_line(role: &str, dir: &Path) -> (i32, String) {
        let output = loader_command(role, dir).output().unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        let line = text
            .lines()
            .find(|l| {
                ["held", "taken", "second", "error"]
                    .iter()
                    .any(|start| l.starts_with(start))
            })
            .unwrap_or("")
            .to_string();
        (output.status.code().unwrap_or(-1), line)
    }

    fn start_holder_as(role: &str, dir: &Path) -> Holder {
        let mut child = Started(
            loader_command(role, dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut lines = BufReader::new(child.0.stdout.take().unwrap()).lines();
        let first = lines
            .find(|l| {
                l.as_ref()
                    .map_or(true, |l| l.starts_with("held") || l.starts_with("error"))
            })
            .unwrap()
            .unwrap();
        assert_eq!(first, format!("held {}", lock_path_in(dir).display()));
        Holder { child, lines }
    }

    #[test]
    fn a_second_loader_is_refused_at_once_and_a_killed_holder_frees_the_lock() {
        let dir = fresh_dir("refused");
        let lock = lock_path_in(&dir);
        assert!(!lock.exists());

        let mut holder = start_holder_as("hold", &dir);

        let (code, line) = loader_line("try", &dir);
        assert_eq!(code, 3, "the second loader got: {line}");
        let ms: u64 = line
            .strip_prefix("held-refused ms=")
            .and_then(|rest| rest.split(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("unexpected line: {line}"));
        assert!(ms < 1000, "the second loader waited {ms} ms");

        holder.child.end();

        let (code, line) = loader_line("try", &dir);
        assert_eq!(code, 0, "after the holder was killed: {line}");
        assert_eq!(line, format!("taken {}", lock.display()));

        let meta = std::fs::metadata(&lock).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.len(), 0);
    }

    #[test]
    fn the_take_leaves_the_content_of_the_lock_file_as_it_was() {
        let dir = fresh_dir("content");
        let lock = lock_path_in(&dir);
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        std::fs::write(&lock, "left by an earlier run\n").unwrap();
        let (code, line) = loader_line("try", &dir);
        assert_eq!(code, 0, "{line}");
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap(),
            "left by an earlier run\n"
        );
    }

    #[test]
    fn a_holder_that_starts_a_child_and_ends_leaves_the_lock_free_while_the_child_lives() {
        let dir = fresh_dir("child");
        let mut holder = start_holder_as("start-child-and-exit", &dir);
        let child_input = holder.child.0.stdin.take().unwrap();
        holder.child.0.wait().unwrap();
        let (code, line) = loader_line("try", &dir);
        drop(child_input);
        assert_eq!(
            code, 0,
            "a loader after the holder ended, while the child that it started lives: {line}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_of_the_holder_cannot_release_the_lock() {
        let dir = fresh_dir("unlock");
        let _holder = start_holder_as("hold-and-start-unlocker", &dir);
        let (code, line) = loader_line("try", &dir);
        assert_eq!(
            code, 3,
            "a loader while the holder lives, after a child of the holder unlocked what it inherited: {line}"
        );
    }

    #[test]
    fn a_held_lock_is_refused_with_words_that_name_no_other_process() {
        let dir = fresh_dir("twice");
        let (code, line) = loader_line("take-twice", &dir);
        assert_eq!(code, 3, "{line}");
        assert_eq!(
            line,
            format!(
                "second the model lock is held: {}",
                lock_path_in(&dir).display()
            )
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_started_after_the_second_call_holds_the_lock_after_the_holder_has_ended() {
        let dir = fresh_dir("hand-on");
        let mut holder = start_holder_as("hand-on-start-child-and-exit", &dir);
        let child_input = holder.child.0.stdin.take().unwrap();
        holder.child.0.wait().unwrap();
        let (code, line) = loader_line("try", &dir);
        drop(child_input);
        assert_eq!(
            code, 3,
            "a loader after the holder ended, while the child that it started after the second call lives: {line}"
        );
        let begun = Instant::now();
        loop {
            let (code, line) = loader_line("try", &dir);
            if code == 0 {
                break;
            }
            assert!(
                begun.elapsed() < std::time::Duration::from_secs(60),
                "the lock stayed held after the child ended: {line}"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[test]
    fn a_loader_that_exits_frees_the_lock() {
        let dir = fresh_dir("exit");
        let (code, line) = loader_line("try", &dir);
        assert_eq!(code, 0, "{line}");
        let (code, line) = loader_line("try", &dir);
        assert_eq!(code, 0, "{line}");
    }

    #[cfg(unix)]
    #[test]
    fn a_holder_that_execs_keeps_the_lock_until_its_process_ends() {
        let dir = fresh_dir("exec");
        let mut holder = start_holder_as("hold-then-exec", &dir);
        let next = holder.lines.next().unwrap().unwrap();
        assert_eq!(next, "execd", "the holder did not exec");
        let (code, line) = loader_line("try", &dir);
        holder.child.end();
        assert_eq!(
            code, 3,
            "a second loader got the lock after the holder's exec: {line}"
        );
        let (code, line) = loader_line("try", &dir);
        assert_eq!(code, 0, "after the holder's process ended: {line}");
    }

    #[cfg(windows)]
    #[test]
    fn the_held_lock_file_and_its_directory_cannot_be_moved() {
        let dir = fresh_dir("move");
        let lock = lock_path_in(&dir);
        let mut holder = start_holder_as("hold", &dir);
        let moved = dir.join("moved.lock");
        let file_moved = std::fs::rename(&lock, &moved).is_ok();
        let dir_moved = std::fs::rename(dir.join("halvo"), dir.join("halvo-moved")).is_ok();
        let (code, line) = loader_line("try", &dir);
        holder.child.end();
        assert!(!file_moved, "the held lock file was renamed");
        assert!(
            !dir_moved,
            "the directory of the held lock file was renamed"
        );
        assert_eq!(code, 3, "a second loader got: {line}");
    }
}
