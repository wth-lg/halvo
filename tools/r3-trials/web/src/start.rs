//! Every start of a child of this process, and the start of the browser on its debugging pipe
//! (A11.1, the coordinator's rulings CP-89 (b) and CP-93 (1)).

use std::ffi::OsString;
use std::fs::File;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

static STARTS: Mutex<()> = Mutex::new(());

/// The one lock that every start of a child in this process takes. A value of this type is the
/// only way to start a child: the crate's `clippy.toml` refuses `std::process::Command::spawn`,
/// `output` and `status` everywhere but in `StartLock::spawn`. While it is held, the ends of a
/// browser's pipe exist in this process with nothing but close-on-exec (macOS) or with the
/// inherit flag clear (Windows), except inside a start of that browser under the same lock.
pub struct StartLock {
    _held: MutexGuard<'static, ()>,
}

/// Takes the start lock, waiting for a start of another thread to end.
pub fn lock() -> StartLock {
    StartLock {
        _held: STARTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    }
}

/// A failed start: the step that failed and the system's error number, never a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartFault {
    pub step: &'static str,
    pub code: i32,
}

impl std::fmt::Display for StartFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} os_error={}", self.step, self.code)
    }
}

impl StartFault {
    fn last(step: &'static str) -> StartFault {
        StartFault {
            step,
            code: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        }
    }
}

impl StartLock {
    /// Starts `cmd` as std does, under this lock.
    #[allow(clippy::disallowed_methods)]
    pub fn spawn(&self, cmd: &mut std::process::Command) -> std::io::Result<std::process::Child> {
        cmd.spawn()
    }
}

/// Starts `cmd` under the start lock with its standard output and error captured, as std's
/// `output` does, and waits for its output with the lock released.
pub fn output(cmd: &mut std::process::Command) -> std::io::Result<std::process::Output> {
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = {
        let held = lock();
        held.spawn(cmd)?
    };
    child.wait_with_output()
}

/// The starts of `std::process::Command` under the start lock, in place of `spawn`, `output` and
/// `status`.
pub trait Start {
    /// `spawn` under the start lock.
    fn start(&mut self) -> std::io::Result<std::process::Child>;
    /// `output` with the start under the start lock.
    fn start_output(&mut self) -> std::io::Result<std::process::Output>;
    /// `status` with the start under the start lock.
    fn start_status(&mut self) -> std::io::Result<std::process::ExitStatus>;
}

impl Start for std::process::Command {
    fn start(&mut self) -> std::io::Result<std::process::Child> {
        let held = lock();
        held.spawn(self)
    }

    fn start_output(&mut self) -> std::io::Result<std::process::Output> {
        output(self)
    }

    fn start_status(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let mut child = self.start()?;
        child.wait()
    }
}

/// The identity of the two pipes of a browser's debugging channel, by which a check finds a
/// process that holds an end of them: on macOS the system's handles of the four ends, as
/// `proc_pidfdinfo` reports them for a pipe; on Windows the values, in this process, of the copies
/// of the four ends that `OnPipe::kept` holds, to which a check compares another process's handle
/// with `CompareObjectHandles` (no query that waits on a pipe with a pending read). The Windows
/// values are valid while `kept` is open; the ends themselves close at the stop, and a value of a
/// closed handle may name another object of this process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipeIds {
    #[cfg(target_os = "macos")]
    pub handles: [u64; 4],
    #[cfg(windows)]
    pub handles: [usize; 4],
}

/// A child that a start of this module made. It is ended only through `kill`, which reaches the
/// child while it is not reaped, because an unreaped child keeps its process number (macOS) and
/// the held process handle keeps it (Windows).
pub struct Started {
    pid: u32,
    reaped: bool,
    #[cfg(windows)]
    process: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: the process handle is owned by `Started` alone and the calls on it are thread safe.
#[cfg(windows)]
unsafe impl Send for Started {}
// SAFETY: as for `Send`; no method mutates through `&self`.
#[cfg(windows)]
unsafe impl Sync for Started {}

/// The browser on its pipe: the child, the end from which the executor reads the browser's
/// messages, the end to which it writes its own, the identity of both pipes, and on Windows the
/// executor's copies of the browser's two ends, not inheritable, which `ids` names; a copy of the
/// browser's write end keeps the executor's read from ending while it is held.
pub struct OnPipe {
    pub child: Started,
    pub from_browser: File,
    pub to_browser: File,
    pub ids: PipeIds,
    pub kept: Vec<File>,
}

/// What a start places into the child besides its standard streams.
pub struct Spec<'a> {
    pub program: &'a Path,
    pub args: Vec<OsString>,
    /// Variables set or replaced in the child's environment; the rest is this process's.
    pub env: Vec<(OsString, OsString)>,
    /// A new file for the child's standard error, or the null device.
    pub stderr: Option<&'a Path>,
}

impl Started {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Whether the child has ended; it is reaped then.
    pub fn exited(&mut self) -> bool {
        if self.reaped {
            return true;
        }
        self.reaped = sys::reap(self, false);
        self.reaped
    }

    /// Ends the child with the system's hard kill while it is not reaped, then reaps it.
    pub fn kill(&mut self) {
        if !self.reaped {
            sys::hard_kill(self);
            self.reaped = sys::reap(self, true);
        }
    }

    /// Waits for the child's end and reaps it.
    pub fn wait(&mut self) {
        if !self.reaped {
            self.reaped = sys::reap(self, true);
        }
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        sys::release(self);
    }
}

/// Starts the browser with its debugging channel on a pipe and with nothing else of this process
/// in it but its standard streams: on macOS `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT` and
/// the browser's ends placed at descriptors 3 and 4 (`--remote-debugging-pipe`); on Windows
/// `CreateProcessW` with `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` naming the browser's two ends and its
/// standard handles, the ends made inheritable inside the lock alone and named by
/// `--remote-debugging-io-pipes`. The caller's `args` hold `--remote-debugging-pipe`.
pub fn start_on_pipe(held: &StartLock, spec: &Spec<'_>) -> Result<OnPipe, StartFault> {
    sys::start_on_pipe(held, spec)
}

/// Starts a program with one more descriptor or handle placed in it besides its standard
/// streams: on macOS at descriptor 3, on Windows as an inheritable handle in the handle list,
/// whose value the caller passes in `spec.args`. Nothing else of this process reaches the child.
pub fn start_with(
    held: &StartLock,
    spec: &Spec<'_>,
    placed: RawPlaced,
) -> Result<Started, StartFault> {
    sys::start_with(held, spec, placed)
}

#[cfg(unix)]
pub type RawPlaced = std::os::fd::RawFd;
#[cfg(windows)]
pub type RawPlaced = windows_sys::Win32::Foundation::HANDLE;

/// One argument of a Windows command line, quoted by the rules of the C runtime's parser, which
/// Chrome and Edge use: a backslash is literal unless a run of them ends at a double quote.
pub fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

#[cfg(target_os = "macos")]
mod sys {
    use super::{OnPipe, PipeIds, RawPlaced, Spec, StartFault, StartLock, Started};
    use std::ffi::{CString, OsString};
    use std::fs::File;
    use std::os::fd::{FromRawFd, RawFd};
    use std::os::unix::ffi::OsStrExt;

    pub(super) fn reap(child: &Started, block: bool) -> bool {
        let mut status = 0;
        let flags = if block { 0 } else { libc::WNOHANG };
        loop {
            // SAFETY: waitpid on our own unreaped child with a valid pointer to a local integer.
            let r = unsafe { libc::waitpid(child.pid as libc::pid_t, &mut status, flags) };
            if r == child.pid as libc::pid_t {
                return true;
            }
            if r == 0 {
                return false;
            }
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return r < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD);
        }
    }

    pub(super) fn hard_kill(child: &Started) {
        // SAFETY: the child is not reaped, so its number names it and no other process. That holds
        // while nothing else in this process reaps this child: `reap` above is the only wait for a
        // child that a start made, and every other wait in the crate names its own child (std's
        // `Child::wait` and `try_wait` call waitpid with that child's number); nothing waits for
        // any child (waitpid with -1, wait), the crate sets no SIGCHLD disposition (SIG_IGN would
        // let the system reap it), and tokio is built without its process feature (Cargo.toml),
        // so no runtime reaps beside it.
        unsafe { libc::kill(child.pid as libc::pid_t, libc::SIGKILL) };
    }

    pub(super) fn release(_child: &mut Started) {}

    /// Moves `fd` to a number of 10 or more with close-on-exec set, and closes `fd`.
    fn lift(fd: RawFd) -> Result<RawFd, StartFault> {
        // SAFETY: fcntl and close on a descriptor that this function's caller owns.
        let lifted = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
        let fault = StartFault::last("pipe");
        // SAFETY: as above; `fd` is not used again.
        unsafe { libc::close(fd) };
        if lifted < 0 {
            return Err(fault);
        }
        Ok(lifted)
    }

    fn close_all(fds: &[RawFd]) {
        for fd in fds {
            // SAFETY: each descriptor is owned here and closed once.
            unsafe { libc::close(*fd) };
        }
    }

    /// The system's handle of the pipe end at `fd` in this process.
    pub(super) fn pipe_handle(fd: RawFd) -> Option<u64> {
        crate::peer::pipe_handle_of(std::process::id(), fd)
    }

    fn cstring(bytes: &[u8]) -> Result<CString, StartFault> {
        CString::new(bytes).map_err(|_| StartFault {
            step: "argument",
            code: libc::EINVAL,
        })
    }

    fn environment(spec: &Spec<'_>) -> Result<Vec<CString>, StartFault> {
        let mut vars: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(k, _)| !spec.env.iter().any(|(s, _)| s == k))
            .collect();
        vars.extend(spec.env.iter().cloned());
        vars.iter()
            .map(|(k, v)| {
                let mut kv = k.as_bytes().to_vec();
                kv.push(b'=');
                kv.extend_from_slice(v.as_bytes());
                cstring(&kv)
            })
            .collect()
    }

    /// Runs `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`: the child gets the null device or the
    /// file of `spec.stderr` at 0 to 2 and `placed[i]` at `3 + i`, and nothing else.
    fn spawn(spec: &Spec<'_>, placed: &[RawFd]) -> Result<u32, StartFault> {
        let program = cstring(spec.program.as_os_str().as_bytes())?;
        let mut argv_owned = vec![program.clone()];
        for a in &spec.args {
            argv_owned.push(cstring(a.as_bytes())?);
        }
        let env_owned = environment(spec)?;
        let mut argv: Vec<*mut libc::c_char> =
            argv_owned.iter().map(|c| c.as_ptr() as *mut _).collect();
        argv.push(std::ptr::null_mut());
        let mut envp: Vec<*mut libc::c_char> =
            env_owned.iter().map(|c| c.as_ptr() as *mut _).collect();
        envp.push(std::ptr::null_mut());
        let null = cstring(b"/dev/null")?;
        let err_path = match spec.stderr {
            Some(p) => cstring(p.as_os_str().as_bytes())?,
            None => null.clone(),
        };
        let err_flags = if spec.stderr.is_some() {
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW
        } else {
            libc::O_WRONLY
        };
        let mut actions: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
        let mut attr: libc::posix_spawnattr_t = std::ptr::null_mut();
        // SAFETY: every pointer passed below points to a value that lives until the end of this
        // block: the action list and the attributes are initialised before use and destroyed
        // once, the strings are owned by `argv_owned`, `env_owned`, `null` and `err_path`, and the
        // two pointer arrays end with a null pointer as posix_spawn requires.
        unsafe {
            if libc::posix_spawn_file_actions_init(&mut actions) != 0 {
                return Err(StartFault::last("spawn_actions"));
            }
            if libc::posix_spawnattr_init(&mut attr) != 0 {
                libc::posix_spawn_file_actions_destroy(&mut actions);
                return Err(StartFault::last("spawn_attributes"));
            }
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut empty);
            let mut defaults: libc::sigset_t = std::mem::zeroed();
            libc::sigfillset(&mut defaults);
            let flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
                | libc::POSIX_SPAWN_SETSIGDEF
                | libc::POSIX_SPAWN_SETSIGMASK;
            let mut rc = libc::posix_spawnattr_setflags(&mut attr, flags as libc::c_short);
            rc |= libc::posix_spawnattr_setsigmask(&mut attr, &empty);
            rc |= libc::posix_spawnattr_setsigdefault(&mut attr, &defaults);
            rc |= libc::posix_spawn_file_actions_addopen(
                &mut actions,
                0,
                null.as_ptr(),
                libc::O_RDONLY,
                0,
            );
            rc |= libc::posix_spawn_file_actions_addopen(
                &mut actions,
                1,
                null.as_ptr(),
                libc::O_WRONLY,
                0,
            );
            rc |= libc::posix_spawn_file_actions_addopen(
                &mut actions,
                2,
                err_path.as_ptr(),
                err_flags,
                0o600,
            );
            for (i, fd) in placed.iter().enumerate() {
                rc |= libc::posix_spawn_file_actions_adddup2(&mut actions, *fd, 3 + i as i32);
            }
            let mut pid: libc::pid_t = 0;
            if rc == 0 {
                rc = libc::posix_spawn(
                    &mut pid,
                    program.as_ptr(),
                    &actions,
                    &attr,
                    argv.as_ptr(),
                    envp.as_ptr(),
                );
            }
            libc::posix_spawn_file_actions_destroy(&mut actions);
            libc::posix_spawnattr_destroy(&mut attr);
            if rc != 0 {
                return Err(StartFault {
                    step: "spawn",
                    code: rc,
                });
            }
            Ok(pid as u32)
        }
    }

    pub(super) fn start_on_pipe(_held: &StartLock, spec: &Spec<'_>) -> Result<OnPipe, StartFault> {
        let mut to = [0; 2];
        let mut from = [0; 2];
        // SAFETY: pipe writes two descriptors into the two-element array.
        if unsafe { libc::pipe(to.as_mut_ptr()) } != 0 {
            return Err(StartFault::last("pipe"));
        }
        // SAFETY: as above.
        if unsafe { libc::pipe(from.as_mut_ptr()) } != 0 {
            let fault = StartFault::last("pipe");
            close_all(&to);
            return Err(fault);
        }
        let mut lifted = Vec::with_capacity(4);
        for fd in [to[0], to[1], from[0], from[1]] {
            match lift(fd) {
                Ok(l) => lifted.push(l),
                Err(f) => {
                    close_all(&lifted);
                    let rest: Vec<RawFd> = [to[0], to[1], from[0], from[1]]
                        .into_iter()
                        .skip(lifted.len() + 1)
                        .collect();
                    close_all(&rest);
                    return Err(f);
                }
            }
        }
        let (browser_reads, we_write, we_read, browser_writes) =
            (lifted[0], lifted[1], lifted[2], lifted[3]);
        let mut handles = [0u64; 4];
        for (slot, fd) in handles
            .iter_mut()
            .zip([browser_reads, we_write, we_read, browser_writes])
        {
            *slot = pipe_handle(fd).unwrap_or(0);
        }
        let spawned = spawn(spec, &[browser_reads, browser_writes]);
        close_all(&[browser_reads, browser_writes]);
        let pid = match spawned {
            Ok(pid) => pid,
            Err(f) => {
                close_all(&[we_write, we_read]);
                return Err(f);
            }
        };
        // SAFETY: both descriptors are open, owned here alone, and moved into one `File` each.
        let (to_browser, from_browser) =
            unsafe { (File::from_raw_fd(we_write), File::from_raw_fd(we_read)) };
        Ok(OnPipe {
            child: Started { pid, reaped: false },
            from_browser,
            to_browser,
            ids: PipeIds { handles },
            kept: Vec::new(),
        })
    }

    /// A copy of `placed` at a number of 10 or more with close-on-exec set, so that the action
    /// that places it at 3 never copies a descriptor onto itself, which would keep its
    /// close-on-exec flag and leave the child without it.
    pub(super) fn lifted_copy(placed: RawFd) -> Result<RawFd, StartFault> {
        // SAFETY: F_DUPFD_CLOEXEC makes a new descriptor for the open `placed`.
        let lifted = unsafe { libc::fcntl(placed, libc::F_DUPFD_CLOEXEC, 10) };
        if lifted < 0 {
            return Err(StartFault::last("placed"));
        }
        Ok(lifted)
    }

    pub(super) fn start_with(
        _held: &StartLock,
        spec: &Spec<'_>,
        placed: RawPlaced,
    ) -> Result<Started, StartFault> {
        let lifted = lifted_copy(placed)?;
        let spawned = spawn(spec, &[lifted]);
        close_all(&[lifted]);
        Ok(Started {
            pid: spawned?,
            reaped: false,
        })
    }
}

#[cfg(windows)]
mod sys {
    use super::{
        quote_windows_arg, OnPipe, PipeIds, RawPlaced, Spec, StartFault, StartLock, Started,
    };
    use std::ffi::OsString;
    use std::fs::File;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{
        CloseHandle, DuplicateHandle, SetHandleInformation, DUPLICATE_SAME_ACCESS, GENERIC_READ,
        GENERIC_WRITE, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
        InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
        WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
        PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES,
        STARTUPINFOEXW,
    };

    pub(super) fn reap(child: &Started, block: bool) -> bool {
        // SAFETY: the process handle is open and owned by `child`.
        let r = unsafe { WaitForSingleObject(child.process, if block { INFINITE } else { 0 }) };
        r == WAIT_OBJECT_0
    }

    pub(super) fn hard_kill(child: &Started) {
        // SAFETY: the handle names our child and no other process while it is held.
        unsafe { TerminateProcess(child.process, 1) };
    }

    pub(super) fn release(child: &mut Started) {
        if !child.process.is_null() {
            // SAFETY: the handle is owned by `child` and closed once.
            unsafe { CloseHandle(child.process) };
            child.process = std::ptr::null_mut();
        }
    }

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn close_all(handles: &[HANDLE]) {
        for h in handles {
            if !h.is_null() && *h != INVALID_HANDLE_VALUE {
                // SAFETY: each handle is owned here and closed once.
                unsafe { CloseHandle(*h) };
            }
        }
    }

    fn inheritable(h: HANDLE, on: bool) -> bool {
        // SAFETY: the handle is open and owned by this process.
        unsafe {
            SetHandleInformation(
                h,
                HANDLE_FLAG_INHERIT,
                if on { HANDLE_FLAG_INHERIT } else { 0 },
            ) != 0
        }
    }

    /// Opens the null device, or creates the new file `path`, as an inheritable handle; the
    /// caller holds the start lock and closes it after the start.
    fn std_handle(path: Option<&std::path::Path>, write: bool) -> Result<HANDLE, StartFault> {
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let (name, disposition) = match path {
            Some(p) => (wide(p.as_os_str()), CREATE_NEW),
            None => (wide(std::ffi::OsStr::new("NUL")), OPEN_EXISTING),
        };
        // SAFETY: `name` is a NUL-terminated wide string and `sa` lives across the call.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                if write { GENERIC_WRITE } else { GENERIC_READ },
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &sa,
                disposition,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(StartFault::last("std_handle"));
        }
        Ok(h)
    }

    fn environment(spec: &Spec<'_>) -> Vec<u16> {
        let mut vars: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(k, _)| {
                !spec.env.iter().any(|(s, _)| {
                    s.to_string_lossy()
                        .eq_ignore_ascii_case(&k.to_string_lossy())
                })
            })
            .collect();
        vars.extend(spec.env.iter().cloned());
        vars.sort_by_key(|(k, _)| k.to_string_lossy().to_uppercase());
        let mut block = Vec::new();
        for (k, v) in vars {
            block.extend(k.encode_wide());
            block.push(b'=' as u16);
            block.extend(v.encode_wide());
            block.push(0);
        }
        block.push(0);
        block
    }

    /// Runs `CreateProcessW` with a handle list of the standard handles and `placed`, each made
    /// inheritable for the call alone and made not inheritable again after it.
    fn spawn(spec: &Spec<'_>, placed: &[HANDLE]) -> Result<Started, StartFault> {
        let stdin = std_handle(None, false)?;
        let stdout = match std_handle(None, true) {
            Ok(h) => h,
            Err(f) => {
                close_all(&[stdin]);
                return Err(f);
            }
        };
        let stderr = match std_handle(spec.stderr, true) {
            Ok(h) => h,
            Err(f) => {
                close_all(&[stdin, stdout]);
                return Err(f);
            }
        };
        let mut list: Vec<HANDLE> = vec![stdin, stdout, stderr];
        list.extend_from_slice(placed);
        let result = spawn_listed(spec, &list, [stdin, stdout, stderr]);
        for h in placed {
            inheritable(*h, false);
        }
        close_all(&[stdin, stdout, stderr]);
        result
    }

    fn spawn_listed(
        spec: &Spec<'_>,
        list: &[HANDLE],
        std: [HANDLE; 3],
    ) -> Result<Started, StartFault> {
        for h in list {
            if !inheritable(*h, true) {
                return Err(StartFault::last("inherit"));
            }
        }
        let mut line = quote_windows_arg(&spec.program.to_string_lossy());
        for a in &spec.args {
            line.push(' ');
            line.push_str(&quote_windows_arg(&a.to_string_lossy()));
        }
        let mut line: Vec<u16> = OsString::from(line)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let program = wide(spec.program.as_os_str());
        let mut env = environment(spec);
        let mut size = 0usize;
        // SAFETY: the first call asks for the size of a list of one attribute.
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size) };
        let mut buf = vec![0u8; size.max(64)];
        let attrs = buf.as_mut_ptr() as *mut core::ffi::c_void;
        // SAFETY: `buf` holds `size` bytes, as the first call asked for.
        if unsafe { InitializeProcThreadAttributeList(attrs, 1, 0, &mut size) } == 0 {
            return Err(StartFault::last("attribute_list"));
        }
        // SAFETY: `list` lives until DeleteProcThreadAttributeList below, as the attribute
        // requires, and holds handles that are open and inheritable.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                attrs,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                list.as_ptr() as *const core::ffi::c_void,
                std::mem::size_of_val(list),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if updated == 0 {
            let fault = StartFault::last("handle_list");
            // SAFETY: the list was initialised above and is deleted once.
            unsafe { DeleteProcThreadAttributeList(attrs) };
            return Err(fault);
        }
        // SAFETY: STARTUPINFOEXW is a C struct of integers, handles and pointers (windows-sys
        // declares it `#[repr(C)]` and `Copy`), for which all-zero bits are a valid value: zero
        // sizes and flags, null handles and pointers. The fields that CreateProcessW reads are set
        // below before the call.
        let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        si.StartupInfo.hStdInput = std[0];
        si.StartupInfo.hStdOutput = std[1];
        si.StartupInfo.hStdError = std[2];
        si.lpAttributeList = attrs;
        // SAFETY: PROCESS_INFORMATION holds two handles and two numbers (windows-sys declares it
        // `#[repr(C)]` and `Copy`); all-zero bits are a valid value, and CreateProcessW writes
        // every field before the code below reads one.
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: every pointer is valid for the call: the program and the mutable command line
        // are NUL-terminated wide strings, the environment block ends with two NULs, `si` holds
        // the attribute list initialised above, and `pi` receives the two new handles.
        let created = unsafe {
            CreateProcessW(
                program.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                env.as_mut_ptr() as *const core::ffi::c_void,
                std::ptr::null(),
                &si.StartupInfo,
                &mut pi,
            )
        };
        let fault = StartFault::last("create_process");
        // SAFETY: the list was initialised above and is deleted once.
        unsafe { DeleteProcThreadAttributeList(attrs) };
        if created == 0 {
            return Err(fault);
        }
        close_all(&[pi.hThread]);
        Ok(Started {
            pid: pi.dwProcessId,
            reaped: false,
            process: pi.hProcess,
        })
    }

    pub(super) fn start_on_pipe(_held: &StartLock, spec: &Spec<'_>) -> Result<OnPipe, StartFault> {
        let mut browser_reads: HANDLE = std::ptr::null_mut();
        let mut we_write: HANDLE = std::ptr::null_mut();
        let mut we_read: HANDLE = std::ptr::null_mut();
        let mut browser_writes: HANDLE = std::ptr::null_mut();
        // SAFETY: CreatePipe writes two new handles, made not inheritable by the null attributes.
        if unsafe { CreatePipe(&mut browser_reads, &mut we_write, std::ptr::null(), 0) } == 0 {
            return Err(StartFault::last("pipe"));
        }
        // SAFETY: as above.
        if unsafe { CreatePipe(&mut we_read, &mut browser_writes, std::ptr::null(), 0) } == 0 {
            let fault = StartFault::last("pipe");
            close_all(&[browser_reads, we_write]);
            return Err(fault);
        }
        let copy = |h: HANDLE| -> Option<HANDLE> {
            let mut dup: HANDLE = std::ptr::null_mut();
            // SAFETY: duplicates an open handle of this process into this process, not inheritable.
            let ok = unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    h,
                    GetCurrentProcess(),
                    &mut dup,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            (ok != 0).then_some(dup)
        };
        let copies = [
            copy(we_write),
            copy(we_read),
            copy(browser_reads),
            copy(browser_writes),
        ];
        let [Some(kept_we_write), Some(kept_we_read), Some(kept_reads), Some(kept_writes)] = copies
        else {
            let fault = StartFault::last("pipe_copy");
            let made: Vec<HANDLE> = copies.iter().flatten().copied().collect();
            close_all(&made);
            close_all(&[browser_reads, we_write, we_read, browser_writes]);
            return Err(fault);
        };
        let handles = [
            kept_we_write as usize,
            kept_we_read as usize,
            kept_reads as usize,
            kept_writes as usize,
        ];
        let mut with_pipes = Spec {
            program: spec.program,
            args: spec.args.clone(),
            env: spec.env.clone(),
            stderr: spec.stderr,
        };
        with_pipes.args.push(OsString::from(format!(
            "--remote-debugging-io-pipes={},{}",
            browser_reads as usize, browser_writes as usize
        )));
        let started = spawn(&with_pipes, &[browser_reads, browser_writes]);
        close_all(&[browser_reads, browser_writes]);
        let child = match started {
            Ok(c) => c,
            Err(f) => {
                close_all(&[
                    we_write,
                    we_read,
                    kept_we_write,
                    kept_we_read,
                    kept_reads,
                    kept_writes,
                ]);
                return Err(f);
            }
        };
        // SAFETY: the six handles are open, owned here alone, and moved into one `File` each.
        let (to_browser, from_browser, kept) = unsafe {
            (
                File::from_raw_handle(we_write as _),
                File::from_raw_handle(we_read as _),
                vec![
                    File::from_raw_handle(kept_we_write as _),
                    File::from_raw_handle(kept_we_read as _),
                    File::from_raw_handle(kept_reads as _),
                    File::from_raw_handle(kept_writes as _),
                ],
            )
        };
        Ok(OnPipe {
            child,
            from_browser,
            to_browser,
            ids: PipeIds { handles },
            kept,
        })
    }

    pub(super) fn start_with(
        _held: &StartLock,
        spec: &Spec<'_>,
        placed: RawPlaced,
    ) -> Result<Started, StartFault> {
        spawn(spec, &[placed])
    }

    #[allow(dead_code)]
    fn unused(_: OsString) -> OsString {
        OsString::from_wide(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CP-126 E5 F1: `Start::start` takes the start lock: while another thread holds it, a start
    /// waits (pr-review-19 F1: the lock taken out of `start` passed every test).
    #[cfg(unix)]
    #[test]
    fn a_start_waits_while_the_start_lock_is_held() {
        let held = lock();
        let (tx, rx) = std::sync::mpsc::channel();
        let starter = std::thread::spawn(move || {
            let child = std::process::Command::new("/bin/sleep").arg("0").start();
            let _ = tx.send(());
            child
        });
        let early = rx.recv_timeout(std::time::Duration::from_millis(500));
        drop(held);
        let late = rx.recv_timeout(std::time::Duration::from_secs(10));
        if let Ok(mut c) = starter.join().unwrap() {
            let _ = c.wait();
        }
        assert!(early.is_err(), "the start ran while the lock was held");
        assert!(
            late.is_ok(),
            "the start did not run after the lock was let go"
        );
    }

    /// CP-126 E5 t12: with a link at the path of the start's error file the start refuses, and
    /// the link's target is left as it was (pr-review-19 t12: `O_TRUNC` in place of
    /// `O_EXCL|O_NOFOLLOW` passed every test).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_link_at_the_error_file_is_refused_and_its_target_kept() {
        let dir = std::env::temp_dir().join(format!("r3-web-t12-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target.txt");
        std::fs::write(&target, "keep").unwrap();
        let link = dir.join("stderr");
        let _ = std::os::unix::fs::symlink(&target, &link);
        let spec = Spec {
            program: Path::new("/bin/sleep"),
            args: vec!["1".into()],
            env: Vec::new(),
            stderr: Some(&link),
        };
        let started = {
            let held = lock();
            start_on_pipe(&held, &spec)
        };
        let refused = started.is_err();
        if let Ok(mut on) = started {
            on.child.kill();
        }
        assert!(refused, "the start took a link as its error file");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
    }

    /// CP-126 E5 M13: a variable of the start replaces the process's own of that name (A11.1's
    /// `TMPDIR` of the browser): the child's environment holds the name once, with the start's
    /// value. PATH stands in, which every test process has. The child is the stand-in in its `env`
    /// mode, which writes every entry of its environment, so a second entry of the name shows.
    #[cfg(all(target_os = "macos", feature = "driver"))]
    #[test]
    fn a_variable_of_the_start_replaces_the_processs_own() {
        let dir = std::env::temp_dir().join(format!("r3-web-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("env.txt");
        let me = std::env::current_exe().unwrap();
        let spec = Spec {
            program: &me,
            args: vec![
                crate::standin::ENTRY.into(),
                "--exact".into(),
                "--quiet".into(),
                "--test-threads=1".into(),
                "--".into(),
                out.clone().into_os_string(),
            ],
            env: vec![
                (crate::standin::VAR.into(), "env".into()),
                ("PATH".into(), "/r3-web-test-path".into()),
            ],
            stderr: None,
        };
        let mut on = {
            let held = lock();
            start_on_pipe(&held, &spec).expect("start")
        };
        on.child.wait();
        let env = std::fs::read_to_string(&out).unwrap();
        let paths: Vec<&str> = env.lines().filter(|l| l.starts_with("PATH=")).collect();
        assert_eq!(paths, vec!["PATH=/r3-web-test-path"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_on_the_pipe_holds_its_standard_streams_and_its_two_ends_alone() {
        use std::os::fd::AsRawFd;
        let (spare_read, spare_write) = {
            let _held = lock();
            let mut fds = [0; 2];
            // SAFETY: pipe writes two new descriptors into the array; they are not close-on-exec.
            assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
            (fds[0], fds[1])
        };
        let spec = Spec {
            program: Path::new("/bin/sleep"),
            args: vec!["5".into()],
            env: Vec::new(),
            stderr: None,
        };
        let mut on = {
            let held = lock();
            start_on_pipe(&held, &spec).expect("start")
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        let pid = on.child.pid();
        let mut fds = crate::peer::fds_of(pid).expect("fds");
        fds.sort_unstable();
        let numbers: Vec<i32> = fds.iter().map(|(fd, _)| *fd).collect();
        assert_eq!(numbers, vec![0, 1, 2, 3, 4]);
        let held: Vec<u64> = crate::peer::pipe_handles_of(pid)
            .expect("pipes")
            .iter()
            .map(|(_, h)| *h)
            .collect();
        assert_eq!(held, vec![on.ids.handles[0], on.ids.handles[3]]);
        let mine = crate::peer::pipe_handles_of(std::process::id()).expect("own pipes");
        let ends_here: Vec<u64> = mine
            .iter()
            .map(|(_, h)| *h)
            .filter(|h| on.ids.handles.contains(h))
            .collect();
        assert_eq!(ends_here.len(), 2);
        assert!(ends_here.contains(&on.ids.handles[1]) && ends_here.contains(&on.ids.handles[2]));
        assert!(crate::checks::holds_an_end(pid, &on.ids));
        let mut other = std::process::Command::new("/bin/sleep");
        other.arg("5");
        let mut other = other.start().expect("other");
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!crate::checks::holds_an_end(other.id(), &on.ids));
        let table = crate::procs::table().expect("table");
        let elsewhere = table
            .iter()
            .find(|p| p.pid == other.id())
            .cloned()
            .expect("row of the other child");
        assert_eq!(crate::checks::pipe_holders(&table, &elsewhere, &on.ids), 1);
        let _ = other.kill();
        let _ = other.wait();
        for fd in [on.to_browser.as_raw_fd(), on.from_browser.as_raw_fd()] {
            // SAFETY: F_GETFD reads the flags of an open descriptor of this process.
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
                libc::FD_CLOEXEC
            );
        }
        on.child.kill();
        // SAFETY: the two spare descriptors are owned here and closed once.
        unsafe {
            libc::close(spare_read);
            libc::close(spare_write);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_placed_descriptor_is_copied_above_the_placed_numbers_with_close_on_exec() {
        let file = std::fs::File::open("/dev/null").expect("open");
        let fd = std::os::fd::AsRawFd::as_raw_fd(&file);
        let copy = sys::lifted_copy(fd).expect("copy");
        assert!(copy >= 10, "{copy}");
        // SAFETY: F_GETFD reads the flags of the open copy, which is closed once after.
        unsafe {
            assert_eq!(
                libc::fcntl(copy, libc::F_GETFD) & libc::FD_CLOEXEC,
                libc::FD_CLOEXEC
            );
            libc::close(copy);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_started_with_a_placed_descriptor_holds_it_at_3_and_nothing_else() {
        let listener = {
            let _held = lock();
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind")
        };
        let spec = Spec {
            program: Path::new("/bin/sleep"),
            args: vec!["5".into()],
            env: Vec::new(),
            stderr: None,
        };
        let mut child = {
            let held = lock();
            start_with(&held, &spec, std::os::fd::AsRawFd::as_raw_fd(&listener)).expect("start")
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mut fds = crate::peer::fds_of(child.pid()).expect("fds");
        fds.sort_unstable();
        assert_eq!(
            fds.iter().map(|(fd, _)| *fd).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        let port = listener.local_addr().expect("addr").port();
        assert!(crate::peer::sockets(&[child.pid()])
            .iter()
            .any(|s| s.listening && s.local.1 == port));
        child.kill();
        assert!(child.exited());
    }

    #[cfg(windows)]
    #[test]
    fn a_child_on_the_pipe_holds_its_two_ends_and_a_child_started_beside_it_holds_none() {
        let ping = Path::new(r"C:\Windows\System32\PING.EXE");
        let spec = Spec {
            program: ping,
            args: vec!["-n".into(), "6".into(), "127.0.0.1".into()],
            env: Vec::new(),
            stderr: None,
        };
        let mut on = {
            let held = lock();
            start_on_pipe(&held, &spec).expect("start")
        };
        let mut beside = std::process::Command::new(ping);
        beside
            .args(["-n", "6", "127.0.0.1"])
            .stdout(std::process::Stdio::null());
        let mut second = beside.start().expect("second");
        let values: Vec<usize> = (1..4096).map(|i| i * 4).collect();
        let held =
            crate::peer::pipe_holders(on.child.pid(), &values, &on.ids.handles).expect("read");
        assert_eq!(held.len(), 2, "{held:?}");
        let other = crate::peer::pipe_holders(second.id(), &values, &on.ids.handles).expect("read");
        assert!(other.is_empty(), "{other:?}");
        assert!(crate::checks::holds_an_end(on.child.pid(), &on.ids));
        assert!(!crate::checks::holds_an_end(second.id(), &on.ids));
        let _ = second.kill();
        let _ = second.wait();
        on.child.kill();
    }

    #[test]
    fn a_windows_argument_is_quoted_by_the_runtime_s_rules() {
        assert_eq!(quote_windows_arg("--no-first-run"), "--no-first-run");
        assert_eq!(
            quote_windows_arg(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            r#""C:\Program Files\Google\Chrome\Application\chrome.exe""#
        );
        assert_eq!(quote_windows_arg(""), r#""""#);
        assert_eq!(quote_windows_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_windows_arg(r"a b\"), r#""a b\\""#);
        assert_eq!(quote_windows_arg(r#"a\"b c"#), r#""a\\\"b c""#);
        assert_eq!(
            quote_windows_arg("--host-resolver-rules=MAP *.test 127.0.0.1,MAP * ~NOTFOUND"),
            r#""--host-resolver-rules=MAP *.test 127.0.0.1,MAP * ~NOTFOUND""#
        );
    }

    #[test]
    fn a_fault_names_its_step_and_number_and_no_path() {
        let f = StartFault {
            step: "spawn",
            code: 2,
        };
        assert_eq!(f.to_string(), "spawn os_error=2");
    }
}
