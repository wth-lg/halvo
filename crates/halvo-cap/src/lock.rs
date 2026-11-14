use std::ffi::OsString;
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
/// The lock is per account: its file lies below the account's own directory, which the
/// system gives whatever the environment of a process holds (see [`model_lock_path`]), so
/// every process of one account meets at one file, and a loader that another account starts
/// takes that account's lock.
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
    /// ended. It is the one cause after which a caller starts the loader again; after every
    /// other cause no caller starts it again as if the lock were held.
    Held(PathBuf),
    /// The system's call gave no directory for the account, or one that is no absolute
    /// path, so there is no lock to take: `call` is `getpwuid_r` on macOS and
    /// `SHGetKnownFolderPath` on Windows, or the call before it that failed, and `code` is
    /// the error that a failed call returned.
    NoAccountDir {
        call: &'static str,
        code: Option<i32>,
    },
    /// This system has no model lock: it is neither macOS nor Windows.
    Unsupported,
    /// A call on the lock file or its directory failed, or the lock path opened and is no
    /// regular file, which gives the call `open`.
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
            LockError::NoAccountDir { call, code: None } => write!(
                f,
                "no model lock: {call} gave no absolute directory for the account"
            ),
            LockError::NoAccountDir {
                call,
                code: Some(code),
            } => write!(f, "no model lock: {call} failed with the code {code}"),
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
const ACCOUNT_CALL: &str = "getpwuid_r";

#[cfg(target_os = "macos")]
fn account_dir() -> Result<Option<OsString>, LockError> {
    account_dir_of(unsafe { libc::geteuid() }, 1024)
}

#[cfg(target_os = "macos")]
fn account_dir_of(uid: libc::uid_t, first_len: usize) -> Result<Option<OsString>, LockError> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStringExt;

    const MAX_BUFFER: usize = 1 << 20;
    let mut buf = vec![0u8; first_len];
    loop {
        let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
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
            return Err(LockError::NoAccountDir {
                call: ACCOUNT_CALL,
                code: Some(rc),
            });
        }
        if found.is_null() || pwd.pw_dir.is_null() {
            return Ok(None);
        }
        let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
        return Ok(Some(OsString::from_vec(dir.to_bytes().to_vec())));
    }
}

#[cfg(windows)]
const ACCOUNT_CALL: &str = "SHGetKnownFolderPath";

#[cfg(windows)]
fn account_dir() -> Result<Option<OsString>, LockError> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{TOKEN_IMPERSONATE, TOKEN_QUERY};
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::{
        FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    };

    struct Token(HANDLE);

    impl Drop for Token {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    struct TaskMem(*mut u16);

    impl Drop for TaskMem {
        fn drop(&mut self) {
            unsafe { CoTaskMemFree(self.0.cast_const().cast()) };
        }
    }

    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_IMPERSONATE,
            &mut token,
        )
    } == 0
    {
        return Err(LockError::NoAccountDir {
            call: "OpenProcessToken",
            code: io::Error::last_os_error().raw_os_error(),
        });
    }
    let token = Token(token);
    let mut path = TaskMem(std::ptr::null_mut());
    let hr = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DEFAULT as u32,
            token.0,
            &mut path.0,
        )
    };
    if hr < 0 {
        return Err(LockError::NoAccountDir {
            call: ACCOUNT_CALL,
            code: Some(hr),
        });
    }
    if path.0.is_null() {
        return Ok(None);
    }
    let len = (0..)
        .take_while(|&i| unsafe { *path.0.add(i) } != 0)
        .count();
    let wide = unsafe { std::slice::from_raw_parts(path.0, len) };
    Ok(Some(OsString::from_wide(wide)))
}

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

#[cfg(any(target_os = "macos", windows))]
fn lock_path_from(found: Result<Option<OsString>, LockError>) -> Result<PathBuf, LockError> {
    match found? {
        Some(base) if Path::new(&base).is_absolute() => Ok(lock_path_in(Path::new(&base))),
        _ => Err(LockError::NoAccountDir {
            call: ACCOUNT_CALL,
            code: None,
        }),
    }
}

/// The path of this account's model lock: `Library/Caches/halvo/model.lock` below the home
/// directory of the account of the process's effective user id, which `getpwuid_r` gives,
/// on macOS, and `halvo\model.lock` below the LocalAppData known folder of the process's
/// own token, which `SHGetKnownFolderPath` gives, on Windows. It reads no variable, so
/// every process of the account gets the same path whatever its environment holds. A
/// lookup that gives no directory, or one that is no absolute path, gives
/// [`LockError::NoAccountDir`], which names the system's call.
#[cfg(any(target_os = "macos", windows))]
pub fn model_lock_path() -> Result<PathBuf, LockError> {
    lock_path_from(account_dir())
}

/// The path of this account's model lock; there is none on this system.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn model_lock_path() -> Result<PathBuf, LockError> {
    Err(LockError::Unsupported)
}

#[cfg(windows)]
fn lock_file_options() -> OpenOptions {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    let mut options = OpenOptions::new();
    options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    options
}

#[cfg(target_os = "macos")]
fn lock_file_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.custom_flags(libc::O_NONBLOCK);
    options
}

#[cfg(not(any(target_os = "macos", windows)))]
fn lock_file_options() -> OpenOptions {
    OpenOptions::new()
}

/// Takes the model lock at [`model_lock_path`] without waiting.
///
/// Creates the directory and the empty lock file where they are missing, writes nothing
/// into the file and never removes it. An existing lock file is opened for reading alone,
/// so a home that can no longer be written still gives the lock; a directory or a file
/// that cannot be made, and a lock call that the system refuses, give [`LockError::Io`]
/// with the call. Returns [`LockError::Held`] at once when the lock is held, by another
/// process or by this one.
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
/// The lock is per account: its file lies below the account's own directory, which the
/// system gives whatever the environment of a process holds (see [`model_lock_path`]), so
/// every process of one account meets at one file, and a loader that another account starts
/// takes that account's lock.
#[cfg(any(target_os = "macos", windows))]
pub fn take_model_lock() -> Result<ModelLock, LockError> {
    take_lock_from(account_dir())
}

/// Takes the model lock; there is none on this system.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn take_model_lock() -> Result<ModelLock, LockError> {
    Err(LockError::Unsupported)
}

#[cfg(any(target_os = "macos", windows))]
fn take_lock_from(found: Result<Option<OsString>, LockError>) -> Result<ModelLock, LockError> {
    take_lock_at(lock_path_from(found)?)
}

#[cfg(any(target_os = "macos", windows))]
fn create_lock_file(path: &Path) -> Result<(), LockError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| LockError::Io {
            call: "create_dir_all",
            path: dir.to_path_buf(),
            source,
        })?;
    }
    lock_file_options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|source| LockError::Io {
            call: "create",
            path: path.to_path_buf(),
            source,
        })?;
    Ok(())
}

#[cfg(any(target_os = "macos", windows))]
fn take_lock_at(path: PathBuf) -> Result<ModelLock, LockError> {
    let open = || lock_file_options().read(true).open(&path);
    let file = match open() {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            create_lock_file(&path)?;
            open()
        }
        opened => opened,
    }
    .and_then(|file| match file.metadata()?.is_file() {
        true => Ok(file),
        false => Err(io::Error::other("the lock path is no file")),
    })
    .map_err(|source| LockError::Io {
        call: "open",
        path: path.clone(),
        source,
    })?;
    let answer = file.try_lock();
    lock_answer(answer, file, path)
}

#[cfg(any(target_os = "macos", windows))]
fn lock_answer(
    answer: Result<(), TryLockError>,
    file: File,
    path: PathBuf,
) -> Result<ModelLock, LockError> {
    match answer {
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
        let take = || take_lock_from(Ok(Some(dir.clone().into_os_string())));
        let mut out = io::stdout();
        let code = match role.as_str() {
            #[cfg(target_os = "macos")]
            "lookup-uid" => {
                let uid: libc::uid_t = arg("halvo-uid").and_then(|u| u.parse().ok()).unwrap();
                match account_dir_of(uid, 1024) {
                    Ok(None) => {
                        writeln!(out, "none").unwrap();
                        0
                    }
                    Ok(Some(_)) => {
                        writeln!(out, "some").unwrap();
                        7
                    }
                    Err(e) => {
                        writeln!(out, "error {e}").unwrap();
                        4
                    }
                }
            }
            "take-relative" | "take-empty" => {
                let base = if role == "take-relative" {
                    "relative-home"
                } else {
                    ""
                };
                match take_lock_from(Ok(Some(base.into()))) {
                    Ok(_lock) => {
                        writeln!(out, "taken").unwrap();
                        0
                    }
                    Err(LockError::NoAccountDir { call, code: None }) => {
                        writeln!(out, "no-account-dir {call}").unwrap();
                        5
                    }
                    Err(e) => {
                        writeln!(out, "error {e}").unwrap();
                        4
                    }
                }
            }
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
                ["held", "taken", "second", "error", "no-account-dir"]
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
    fn a_child_that_the_holder_starts_inherits_no_descriptor_of_the_lock_and_cannot_unlock_it() {
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

    struct Unwritable(Vec<PathBuf>);

    impl Unwritable {
        fn make(paths: &[PathBuf]) -> Self {
            let made = Unwritable(paths.to_vec());
            for path in paths {
                set_writable(path, false).unwrap();
            }
            made
        }
    }

    impl Drop for Unwritable {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = set_writable(path, true);
            }
        }
    }

    #[cfg(unix)]
    fn set_writable(path: &Path, writable: bool) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let mode = match (path.is_dir(), writable) {
            (true, true) => 0o755,
            (true, false) => 0o555,
            (false, true) => 0o644,
            (false, false) => 0o444,
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    #[cfg(windows)]
    fn set_writable(path: &Path, writable: bool) -> io::Result<()> {
        if path.is_dir() {
            let mut icacls = Command::new("icacls");
            icacls.arg(path);
            if writable {
                icacls.args(["/remove:d", "*S-1-1-0"]);
            } else {
                icacls.args(["/deny", "*S-1-1-0:(WD,AD)"]);
            }
            let status = icacls
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!("icacls exited {status}")))
            }
        } else {
            let mut permissions = std::fs::metadata(path)?.permissions();
            permissions.set_readonly(!writable);
            std::fs::set_permissions(path, permissions)
        }
    }

    #[cfg(windows)]
    struct OwnDeny(PathBuf, String);

    #[cfg(windows)]
    impl OwnDeny {
        fn make(dir: &Path) -> Self {
            let who = Command::new("whoami").output().unwrap();
            let account = String::from_utf8(who.stdout).unwrap().trim().to_string();
            assert!(
                who.status.success() && !account.is_empty(),
                "whoami gave no account"
            );
            let made = OwnDeny(dir.to_path_buf(), account);
            let status = Command::new("icacls")
                .arg(dir)
                .args(["/deny", &format!("{}:(AD,GW)", made.1)])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "icacls exited {status}");
            made
        }
    }

    #[cfg(windows)]
    impl Drop for OwnDeny {
        fn drop(&mut self) {
            let _ = Command::new("icacls")
                .arg(&self.0)
                .args(["/remove:d", &self.1])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    #[test]
    fn an_existing_lock_file_that_cannot_be_written_is_taken_and_held() {
        let dir = fresh_dir("read-only");
        let lock = lock_path_in(&dir);
        let parent = lock.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&parent).unwrap();
        std::fs::write(&lock, "").unwrap();
        let _unwritable = Unwritable::make(&[lock.clone(), parent.clone()]);
        assert!(
            File::create(parent.join("probe")).is_err(),
            "the test could not make the directory unwritable"
        );
        let _holder = start_holder_as("hold", &dir);
        let (code, line) = loader_line("try", &dir);
        assert_eq!(code, 3, "a second loader got: {line}");
    }

    #[test]
    fn a_directory_that_cannot_be_written_gives_io_and_no_lock() {
        let base = fresh_dir("unwritable-base");
        let _base_unwritable = Unwritable::make(std::slice::from_ref(&base));
        assert!(
            File::create(base.join("probe")).is_err(),
            "the test could not make the directory unwritable"
        );
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut made = std::fs::create_dir(base.join("probe-dir")).is_ok();
        #[cfg(windows)]
        let _own_deny = if made {
            let own = OwnDeny::make(&base);
            made = std::fs::create_dir(base.join("probe-dir-own")).is_ok();
            Some(own)
        } else {
            None
        };
        if made {
            let _ = writeln!(
                io::stderr(),
                "model-lock unwritable base: not tried here, because this system let the test make a directory in a directory whose ACL denies it, under the deny entry for Everyone (WD,AD) and under the deny entry for the process's own account (AD,GW) on the directory itself; the lock directory below is tried"
            );
        } else {
            match take_lock_from(Ok(Some(base.clone().into_os_string()))) {
                Err(LockError::Io {
                    call: "create_dir_all",
                    ..
                }) => {}
                other => panic!("a base that cannot be written: {other:?}"),
            }
            let first = lock_path_in(&base)
                .strip_prefix(&base)
                .unwrap()
                .components()
                .next()
                .unwrap()
                .as_os_str()
                .to_os_string();
            assert!(!base.join(first).exists(), "the take made a directory");
            let _ = writeln!(io::stderr(), "model-lock unwritable base: tried");
        }

        let dir = fresh_dir("unwritable-dir");
        let lock = lock_path_in(&dir);
        let parent = lock.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&parent).unwrap();
        let _dir_unwritable = Unwritable::make(std::slice::from_ref(&parent));
        match take_lock_from(Ok(Some(dir.into_os_string()))) {
            Err(LockError::Io { call: "create", .. }) => {}
            other => panic!("a lock directory that cannot be written: {other:?}"),
        }
        assert!(!lock.exists());
    }

    #[test]
    fn a_directory_at_the_lock_path_gives_io_and_no_lock() {
        let dir = fresh_dir("lock-is-a-directory");
        let lock = lock_path_in(&dir);
        std::fs::create_dir_all(&lock).unwrap();
        match take_lock_from(Ok(Some(dir.into_os_string()))) {
            Err(LockError::Io { call: "open", .. }) => {}
            other => panic!("a directory at the lock path: {other:?}"),
        }
        assert!(lock.is_dir());
        assert_eq!(std::fs::read_dir(&lock).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_lock_file_that_cannot_be_read_gives_io_and_is_left_as_it_was() {
        use std::os::unix::fs::PermissionsExt;
        struct Unreadable(PathBuf);
        impl Drop for Unreadable {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o644));
            }
        }
        let dir = fresh_dir("unreadable");
        let lock = lock_path_in(&dir);
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        std::fs::write(&lock, "left by an earlier run\n").unwrap();
        let _unreadable = Unreadable(lock.clone());
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(
            File::open(&lock).is_err(),
            "the test could not make the lock file unreadable"
        );
        match take_lock_from(Ok(Some(dir.into_os_string()))) {
            Err(LockError::Io { call: "open", .. }) => {}
            other => panic!("an unreadable lock file: {other:?}"),
        }
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap(),
            "left by an earlier run\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_at_the_lock_path_gives_io_at_once() {
        let dir = fresh_dir("fifo");
        let lock = lock_path_in(&dir);
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        let made = Command::new("/usr/bin/mkfifo").arg(&lock).status().unwrap();
        assert!(made.success(), "mkfifo exited {made}");
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(take_lock_from(Ok(Some(dir.into_os_string()))).map(|_| ()));
        });
        match receive.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(Err(LockError::Io { call: "open", .. })) => {}
            other => panic!("a fifo at the lock path: {other:?}"),
        }
    }

    #[test]
    fn a_lock_file_that_another_process_made_after_the_open_is_kept_as_it_is() {
        let dir = fresh_dir("made-meanwhile");
        let lock = lock_path_in(&dir);
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        std::fs::write(&lock, "made by another loader\n").unwrap();
        create_lock_file(&lock).unwrap();
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap(),
            "made by another loader\n"
        );
    }

    #[test]
    fn each_answer_of_the_lock_call_gives_its_own_result_and_an_error_is_never_held() {
        let dir = fresh_dir("answers");
        let path = dir.join("model.lock");
        std::fs::write(&path, "").unwrap();
        let open = || File::open(&path).unwrap();
        match lock_answer(
            Err(TryLockError::Error(io::Error::other("refused"))),
            open(),
            path.clone(),
        ) {
            Err(LockError::Io {
                call: "try_lock",
                path: at,
                ..
            }) => assert_eq!(at, path),
            other => panic!("an error of the lock call: {other:?}"),
        }
        match lock_answer(Err(TryLockError::WouldBlock), open(), path.clone()) {
            Err(LockError::Held(at)) => assert_eq!(at, path),
            other => panic!("a lock call that would block: {other:?}"),
        }
        match lock_answer(Ok(()), open(), path.clone()) {
            Ok(lock) => assert_eq!(lock.path(), path),
            other => panic!("a lock call that took the lock: {other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_lookup_of_a_user_id_with_no_account_gives_none_while_home_names_a_directory() {
        let uid = [
            4_000_000_001u32,
            3_999_999_999,
            2_147_483_000,
            987_654_321,
            77_777,
        ]
        .into_iter()
        .find(|uid| {
            !Command::new("/usr/bin/id")
                .arg(uid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success()
        })
        .expect("every user id of the list has an account");
        let dir = fresh_dir("no-account");
        let output = loader_command("lookup-uid", &dir)
            .arg(format!("halvo-uid={uid}"))
            .env("HOME", &dir)
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(dir.is_dir());
        assert_eq!(output.status.code(), Some(0), "user id {uid}: {text}");
        assert!(text.lines().any(|l| l == "none"), "user id {uid}: {text}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_lookup_with_a_first_buffer_of_8_bytes_grows_it_and_gives_the_same_directory() {
        let uid = unsafe { libc::geteuid() };
        let plain = account_dir_of(uid, 1024).unwrap();
        assert!(plain.is_some(), "the lookup gave no directory");
        let small = account_dir_of(uid, 8).unwrap();
        assert!(
            small == plain,
            "the lookup with a first buffer of 8 bytes gave another answer"
        );
    }

    #[test]
    fn a_file_in_the_way_of_the_lock_directory_gives_io_and_no_lock() {
        let base = fresh_dir("file-in-the-way");
        let lock = lock_path_in(&base);
        let in_the_way = lock.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(in_the_way.parent().unwrap()).unwrap();
        std::fs::write(&in_the_way, "in the way\n").unwrap();
        match create_lock_file(&lock) {
            Err(LockError::Io {
                call: "create_dir_all",
                ..
            }) => {}
            other => panic!("a file where the lock directory would be made: {other:?}"),
        }
        match take_lock_from(Ok(Some(base.clone().into_os_string()))) {
            Err(LockError::Io { call, .. }) if call == "open" || call == "create_dir_all" => {
                let _ = writeln!(
                    io::stderr(),
                    "model-lock file in the way: the take answered Io with the call {call}"
                );
            }
            other => panic!("a take with a file in the way: {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(&in_the_way).unwrap(),
            "in the way\n"
        );
        assert_eq!(
            std::fs::read_dir(in_the_way.parent().unwrap())
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn the_lookup_is_the_systems_call() {
        let want = if cfg!(windows) {
            "SHGetKnownFolderPath"
        } else {
            "getpwuid_r"
        };
        assert_eq!(ACCOUNT_CALL, want);
    }

    #[test]
    fn a_lookup_that_gives_an_absolute_directory_gives_the_lock_below_it() {
        let dir = fresh_dir("absolute");
        let path = lock_path_from(Ok(Some(dir.clone().into_os_string()))).unwrap();
        assert_eq!(path, lock_path_in(&dir));
    }

    #[test]
    fn a_lookup_that_gives_no_directory_gives_the_cause_and_no_lock() {
        match take_lock_from(Ok(None)) {
            Err(LockError::NoAccountDir { call, code: None }) => assert_eq!(call, ACCOUNT_CALL),
            other => panic!("a lookup that gave no directory: {other:?}"),
        }
    }

    #[test]
    fn a_lookup_that_fails_gives_the_cause_with_its_call_and_code() {
        let failed = LockError::NoAccountDir {
            call: "a-call",
            code: Some(5),
        };
        match take_lock_from(Err(failed)) {
            Err(LockError::NoAccountDir {
                call: "a-call",
                code: Some(5),
            }) => {}
            other => panic!("a lookup that failed: {other:?}"),
        }
    }

    #[test]
    fn a_lookup_that_gives_no_absolute_path_gives_the_cause_and_no_lock() {
        for role in ["take-relative", "take-empty"] {
            let dir = fresh_dir(role);
            let output = loader_command(role, &dir)
                .current_dir(&dir)
                .output()
                .unwrap();
            let text = String::from_utf8(output.stdout).unwrap();
            assert_eq!(output.status.code(), Some(5), "{role}: {text}");
            let want = format!("no-account-dir {ACCOUNT_CALL}");
            assert!(text.lines().any(|l| l == want), "{role}: {text}");
            let left = std::fs::read_dir(&dir).unwrap().count();
            assert_eq!(left, 0, "{role}: the take made {left} entries");
        }
    }

    #[test]
    fn the_cause_names_the_systems_call_and_no_variable() {
        for found in [Ok(None), Ok(Some("relative-home".into()))] {
            let text = lock_path_from(found).unwrap_err().to_string();
            assert!(text.contains(ACCOUNT_CALL), "{text}");
            for name in ["HOME", "LOCALAPPDATA", "APPDATA", "USERPROFILE", "variable"] {
                assert!(!text.contains(name), "{text}");
            }
        }
        let text = LockError::NoAccountDir {
            call: ACCOUNT_CALL,
            code: Some(5),
        }
        .to_string();
        assert!(text.contains(ACCOUNT_CALL) && text.contains('5'), "{text}");
    }
}
