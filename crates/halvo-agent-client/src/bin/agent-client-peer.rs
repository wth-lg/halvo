//! The other end of the library in its tests. Started by the library with `--run ... --browser SCRIPT`, it plays a
//! runner from the script file that `--browser` names and logs what it reads to `SCRIPT.log`; started as `browser`
//! it plays a browser that sleeps, as `child` a process that sleeps, and as `stderr-child MARKER` a process that
//! prints MARKER on standard error and exits. On macOS `holder-parent LOG [SECONDS]` starts `holder [SECONDS]`, a
//! process of a group of its own that sleeps with the standard input and output it was given and then exits with
//! the status 0, logs it to LOG and exits, or ends it and exits with 1 when it cannot read its creation time or
//! write the line; on Windows `hold PID IN OUT LOG [SECONDS]` copies the two handles IN and OUT of the process PID
//! into itself, logs how many to LOG, sleeps and exits with the status 0. Each sleeps SECONDS, 300 by default. As
//! `console-report HOST RUNNER LOG` it appends to LOG whether HOST and RUNNER are attached to its console and whether
//! that console has a window.

#![deny(clippy::undocumented_unsafe_blocks)]

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use halvo_agent_client::process;
use halvo_agent_client::wire::{read_frame, write_frame};

#[path = "agent-client-peer/host.rs"]
mod host;

fn sleep_forever() -> ! {
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

struct Log(Mutex<std::fs::File>);

impl Log {
    fn line(&self, text: &str) {
        if let Ok(mut f) = self.0.lock() {
            let _ = f.write_all(format!("{text}\n").as_bytes());
            let _ = f.flush();
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum OnEof {
    End,
    Hang,
    Send(u16),
    Stderr(String),
}

const FLOOD_CHUNK: usize = 65_536;

const HOLD_BOUND: u64 = 300;

fn write_stderr(bytes: &[u8]) -> bool {
    let mut err = io::stderr().lock();
    err.write_all(bytes).is_ok() && err.flush().is_ok()
}

fn flood_stderr(total: usize) -> usize {
    let chunk = [b'x'; FLOOD_CHUNK];
    let mut wrote = 0;
    while wrote < total {
        let n = FLOOD_CHUNK.min(total - wrote);
        if !write_stderr(&chunk[..n]) {
            break;
        }
        wrote += n;
    }
    wrote
}

#[cfg(unix)]
fn stderr_kind() -> &'static str {
    use std::os::fd::AsFd;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let Ok(fd) = io::stderr().as_fd().try_clone_to_owned() else {
        return "other";
    };
    let Ok(meta) = std::fs::File::from(fd).metadata() else {
        return "other";
    };
    let kind = meta.file_type();
    if kind.is_char_device() {
        match std::fs::metadata("/dev/null") {
            Ok(null) if null.rdev() == meta.rdev() => "null",
            _ => "other",
        }
    } else if kind.is_fifo() {
        "pipe"
    } else if kind.is_file() {
        "file"
    } else {
        "other"
    }
}

#[cfg(windows)]
fn stderr_kind() -> &'static str {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileType(file: *mut std::ffi::c_void) -> u32;
        fn GetConsoleMode(console: *mut std::ffi::c_void, mode: *mut u32) -> i32;
    }
    const FILE_TYPE_DISK: u32 = 1;
    const FILE_TYPE_CHAR: u32 = 2;
    const FILE_TYPE_PIPE: u32 = 3;
    let handle = io::stderr().as_raw_handle();
    let mut mode = 0u32;
    // SAFETY: `handle` is this process's standard error handle, which stays open for the call, and GetFileType only reads it.
    match unsafe { GetFileType(handle) } {
        // SAFETY: `handle` stays open for the call, and `mode` is a live place for the answer.
        FILE_TYPE_CHAR if unsafe { GetConsoleMode(handle, &mut mode) } == 0 => "null",
        FILE_TYPE_PIPE => "pipe",
        FILE_TYPE_DISK => "file",
        _ => "other",
    }
}

#[cfg(target_os = "macos")]
fn own_descriptors() -> String {
    extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut std::ffi::c_void,
            size: i32,
        ) -> i32;
    }
    const PROC_PIDLISTFDS: i32 = 1;
    let mut list = vec![[0u32; 2]; 65_536];
    let room = (list.len() * 8) as i32;
    let pid = std::process::id() as i32;
    // SAFETY: `list` is live and writable for `room` bytes, the size passed, and the call writes at most that.
    let got = unsafe { proc_pidinfo(pid, PROC_PIDLISTFDS, 0, list.as_mut_ptr().cast(), room) };
    if got <= 0 || got >= room {
        return "unreadable".to_owned();
    }
    let mut fds: Vec<u32> = list[..got as usize / 8].iter().map(|e| e[0]).collect();
    fds.sort_unstable();
    fds.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

#[cfg(windows)]
fn own_descriptors() -> String {
    "none".to_owned()
}

#[cfg(unix)]
fn console_line(_host: u32, _runner: u32) -> String {
    "attached=none".to_owned()
}

#[cfg(windows)]
fn console_line(host: u32, runner: u32) -> String {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        fn GetConsoleWindow() -> *mut c_void;
    }
    #[link(name = "user32")]
    extern "system" {
        fn IsWindowVisible(window: *mut c_void) -> i32;
    }
    let mut list = [0u32; 256];
    // SAFETY: `list` is live and writable for the count of entries passed, and the call writes at most that many.
    let n = unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) };
    let held = &list[..(n as usize).min(list.len())];
    let yes = |pid: u32| if held.contains(&pid) { "yes" } else { "no" };
    // SAFETY: GetConsoleWindow takes no argument and returns a window handle or null.
    let window = unsafe { GetConsoleWindow() };
    // SAFETY: `window` is the handle that GetConsoleWindow returned, which the call only reads.
    let visible = !window.is_null() && unsafe { IsWindowVisible(window) } != 0;
    let window = if window.is_null() {
        "none"
    } else if visible {
        "visible"
    } else {
        "hidden"
    };
    format!(
        "attached={n} runner={} host={} window={window}",
        yes(runner),
        yes(host)
    )
}

#[cfg(unix)]
fn pipes_beyond_three() -> usize {
    use std::os::unix::fs::FileTypeExt;
    let Ok(entries) = std::fs::read_dir("/dev/fd") else {
        return usize::MAX;
    };
    let fds: Vec<i32> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .filter(|fd| *fd > 2)
        .collect();
    fds.iter()
        .filter(|fd| {
            std::fs::metadata(format!("/dev/fd/{fd}")).is_ok_and(|m| {
                let t = m.file_type();
                t.is_fifo() || t.is_socket()
            })
        })
        .count()
}

#[cfg(windows)]
fn pipes_beyond_three() -> usize {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileType(file: *mut c_void) -> u32;
    }
    const FILE_TYPE_PIPE: u32 = 3;
    let own = [
        io::stdin().as_raw_handle(),
        io::stdout().as_raw_handle(),
        io::stderr().as_raw_handle(),
    ];
    (1..=0x1000usize)
        .map(|n| (n * 4) as *mut c_void)
        .filter(|h| !own.contains(h))
        // SAFETY: GetFileType takes any value and answers FILE_TYPE_UNKNOWN for one that is no open handle; this process's own three, on which a read may wait, are left out above.
        .filter(|h| unsafe { GetFileType(*h) } == FILE_TYPE_PIPE)
        .count()
}

#[cfg(windows)]
fn hold(args: &[String]) -> ! {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetCurrentProcess() -> *mut c_void;
        fn DuplicateHandle(
            source_process: *mut c_void,
            source: *mut c_void,
            target_process: *mut c_void,
            target: *mut *mut c_void,
            access: u32,
            inherit: i32,
            options: u32,
        ) -> i32;
    }
    const PROCESS_DUP_HANDLE: u32 = 0x0040;
    const DUPLICATE_SAME_ACCESS: u32 = 0x0002;
    let num = |i: usize| {
        args.get(i)
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
    };
    // SAFETY: OpenProcess takes plain values and returns a new handle or null.
    let runner = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, num(1) as u32) };
    let mut held = 0;
    if !runner.is_null() {
        for value in [num(2), num(3)] {
            let mut copy: *mut c_void = std::ptr::null_mut();
            // SAFETY: `runner` is the live handle opened above, GetCurrentProcess gives a pseudo handle that needs no close, and `copy` is a live place for the new handle.
            let ok = unsafe {
                DuplicateHandle(
                    runner,
                    value as *mut c_void,
                    GetCurrentProcess(),
                    &mut copy,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if ok != 0 {
                held += 1;
            }
        }
    }
    if let Some(log) = args.get(4) {
        if let Ok(mut f) = OpenOptions::new().append(true).open(log) {
            let _ = f.write_all(format!("holder holds={held}\n").as_bytes());
        }
    }
    let bound = args
        .get(5)
        .and_then(|s| s.parse().ok())
        .unwrap_or(HOLD_BOUND);
    thread::sleep(Duration::from_secs(bound));
    std::process::exit(0)
}

#[cfg(unix)]
fn write_inherited(number: &str, text: &[u8]) -> &'static str {
    extern "C" {
        fn write(fd: i32, buf: *const std::ffi::c_void, len: usize) -> isize;
    }
    match number.parse::<i32>() {
        Ok(fd) if fd > 2 => {
            // SAFETY: `text` is live and readable for its length, the length passed; a number that is no open descriptor only makes the call fail.
            if unsafe { write(fd, text.as_ptr().cast(), text.len()) } > 0 {
                "wrote"
            } else {
                "absent"
            }
        }
        Ok(_) => "own",
        Err(_) => "bad",
    }
}

#[cfg(windows)]
fn write_inherited(number: &str, text: &[u8]) -> &'static str {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetHandleInformation(handle: *mut c_void, flags: *mut u32) -> i32;
        fn WriteFile(
            handle: *mut c_void,
            buf: *const u8,
            len: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }
    let Ok(value) = number.parse::<usize>() else {
        return "bad";
    };
    let handle = value as *mut c_void;
    let own = [
        io::stdin().as_raw_handle(),
        io::stdout().as_raw_handle(),
        io::stderr().as_raw_handle(),
    ];
    if own.contains(&handle) {
        return "own";
    }
    let mut flags = 0u32;
    // SAFETY: GetHandleInformation takes any value and fails for one that is no open handle; `flags` is a live place for the answer.
    if unsafe { GetHandleInformation(handle, &mut flags) } == 0 || flags & 1 == 0 {
        return "absent";
    }
    let mut wrote = 0u32;
    // SAFETY: `handle` is an inheritable handle that this process holds open, as checked above, and the buffer and `wrote` are live for the call.
    let ok = unsafe {
        WriteFile(
            handle,
            text.as_ptr(),
            text.len() as u32,
            &mut wrote,
            std::ptr::null_mut(),
        )
    };
    if ok != 0 && wrote > 0 {
        "wrote"
    } else {
        "absent"
    }
}

#[cfg(unix)]
fn in_system_directory() -> bool {
    std::env::current_dir().is_ok_and(|d| d == Path::new("/"))
}

#[cfg(windows)]
fn in_system_directory() -> bool {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buf = [0u16; 1024];
    // SAFETY: `buf` is live and writable for its length in units, the size passed, and the call writes at most that.
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return false;
    }
    let system = std::ffi::OsString::from_wide(&buf[..n]);
    let trim = |s: &str| s.trim_end_matches('\\').to_ascii_lowercase();
    std::env::current_dir()
        .is_ok_and(|d| trim(&d.to_string_lossy()) == trim(&system.to_string_lossy()))
}

fn environment_lines(marker: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut marked = Vec::new();
    let mut equal = Vec::new();
    let mut temporary = 0;
    for (name, value) in std::env::vars_os() {
        let name = name.to_string_lossy().into_owned();
        let text = value.to_string_lossy();
        if text.contains(marker) {
            marked.push(name.clone());
        }
        if text == marker {
            equal.push(name.clone());
        }
        if ["TMPDIR", "TEMP", "TMP"].contains(&name.as_str()) && Path::new(&value).is_dir() {
            temporary += 1;
        }
        names.push(name);
    }
    names.sort();
    marked.sort();
    equal.sort();
    vec![
        format!("environment names={}", names.join(",")),
        format!("environment marked={}", marked.join(",")),
        format!("environment equal={}", equal.join(",")),
        format!("environment temporary-directories={temporary}"),
    ]
}

fn wait_for_file(path: &Path, within: Duration) -> bool {
    let deadline = std::time::Instant::now() + within;
    while !path.exists() {
        if std::time::Instant::now() > deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

struct Peer {
    run: String,
    agent: String,
    workflow: String,
    log: Arc<Log>,
    log_path: PathBuf,
    out: io::Stdout,
    replies: Receiver<Vec<u8>>,
    on_eof: Arc<Mutex<OnEof>>,
    first_descriptors: String,
    browsers: Arc<Mutex<Vec<Child>>>,
}

fn arg_after(args: &[String], key: &str) -> String {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default()
}

fn spawn_self(role: &str, own_group: bool) -> io::Result<Child> {
    spawn_self_with(role, own_group, None)
}

fn spawn_self_with(role: &str, own_group: bool, log: Option<&Path>) -> io::Result<Child> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg(role);
    if let Some(log) = log {
        cmd.arg(log);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    if own_group {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = own_group;
    cmd.spawn()
}

fn browser_in_group_of_last_child(
    made: &Mutex<Vec<Child>>,
    role: &str,
    log: &Path,
) -> io::Result<Child> {
    let leader = made
        .lock()
        .ok()
        .and_then(|list| list.last().map(Child::id))
        .ok_or_else(|| io::Error::other("no child"))?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg(role)
        .arg(log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(i32::try_from(leader).map_err(io::Error::other)?);
    }
    #[cfg(not(unix))]
    let _ = leader;
    cmd.spawn()
}

#[cfg(unix)]
fn close_stdout() {
    use std::os::fd::FromRawFd;
    // SAFETY: the step closes standard output on purpose: descriptor 1 is open, and nothing of this process writes to it after the step.
    drop(unsafe { std::os::fd::OwnedFd::from_raw_fd(1) });
}

#[cfg(windows)]
fn close_stdout() {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    // SAFETY: the step closes standard output on purpose: its handle is open, and nothing of this process writes to it after the step.
    drop(unsafe {
        std::os::windows::io::OwnedHandle::from_raw_handle(io::stdout().as_raw_handle())
    });
}

fn end_browsers(browsers: &Mutex<Vec<Child>>) {
    if let Ok(mut list) = browsers.lock() {
        for b in list.iter_mut() {
            #[cfg(unix)]
            if process::process_group(b.id()) == std::num::NonZeroU32::new(b.id()) {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", &format!("-{}", b.id())])
                    .status();
            }
            let _ = b.kill();
            let _ = b.wait();
        }
    }
}

impl Peer {
    fn fill(&self, text: &str) -> String {
        text.replace("{run}", &self.run)
            .replace("{agent}", &self.agent)
            .replace("{workflow}", &self.workflow)
    }

    fn send(&mut self, json: &str) {
        let mut out = self.out.lock();
        if write_frame(&mut out, json).is_err() {
            self.log.line("write-failed");
        }
    }

    fn reply(&self) -> Option<String> {
        let bytes = self.replies.recv().ok()?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        self.log.line(&format!("reply {text}"));
        Some(text)
    }

    fn step(&mut self, line: &str) {
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "hello" => {
                let json = self.fill(r#"{"t":"hello","run":"{run}","agent":"{agent}","v":1}"#);
                self.send(&json);
            }
            "hello-other" => {
                let json = self.fill(
                    r#"{"t":"hello","run":"ffffffffffffffffffffffffffffffff","agent":"{agent}","v":1}"#,
                );
                self.send(&json);
            }
            "hello-other-agent" => {
                let json = self.fill(
                    r#"{"t":"hello","run":"{run}","agent":"ffffffffffffffffffffffffffffffff","v":1}"#,
                );
                self.send(&json);
            }
            "garbage" => self.send("{\"t\":"),
            "oversize" => {
                let mut out = self.out.lock();
                let _ = out.write_all(&[0, 1, 0, 1]);
                let _ = out.flush();
            }
            "close-stdout" => {
                close_stdout();
                self.log.line("stdout-closed");
            }
            "child-on-stdout" => {
                let made = std::env::current_exe().and_then(|exe| {
                    Command::new(exe)
                        .arg("child")
                        .stdin(Stdio::null())
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::null())
                        .spawn()
                });
                match made {
                    Ok(c) => {
                        let created = process::creation_time(c.id())
                            .map(|t| t.to_string())
                            .unwrap_or_default();
                        self.log
                            .line(&format!("child pid={} created={created}", c.id()));
                        if let Ok(mut list) = self.browsers.lock() {
                            list.push(c);
                        }
                    }
                    Err(_) => self.log.line("child-failed"),
                }
            }
            "send" => {
                let json = self.fill(rest);
                self.send(&json);
            }
            "ask" => {
                let json = self.fill(rest);
                self.send(&json);
                if self.reply().is_none() {
                    self.log.line("no-reply");
                }
            }
            "browser" => self.browser(rest),
            "child" => match spawn_self("child", false) {
                Ok(c) => {
                    let created = process::creation_time(c.id())
                        .map(|t| t.to_string())
                        .unwrap_or_default();
                    self.log
                        .line(&format!("child pid={} created={created}", c.id()));
                    if let Ok(mut list) = self.browsers.lock() {
                        list.push(c);
                    }
                }
                Err(_) => self.log.line("child-failed"),
            },
            "breakaway" => self.breakaway(),
            "writer-child" => {
                let mut parts = rest.splitn(3, ' ');
                let delay = parts.next().unwrap_or("0").to_owned();
                let count = parts.next().unwrap_or("0").to_owned();
                let frame = self.fill(parts.next().unwrap_or(""));
                let made = std::env::current_exe().and_then(|exe| {
                    let mut cmd = Command::new(exe);
                    cmd.arg("writer")
                        .arg(&delay)
                        .arg(&count)
                        .arg(&frame)
                        .arg(&self.log_path)
                        .stdin(Stdio::null())
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::null());
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::CommandExt;
                        cmd.process_group(0);
                    }
                    cmd.spawn()
                });
                match made {
                    Ok(c) => {
                        let created = process::creation_time(c.id())
                            .map(|t| t.to_string())
                            .unwrap_or_default();
                        self.log
                            .line(&format!("child pid={} created={created}", c.id()));
                        if let Ok(mut list) = self.browsers.lock() {
                            list.push(c);
                        }
                    }
                    Err(_) => self.log.line("child-failed"),
                }
            }
            "child-own-group" => match spawn_self("child", true) {
                Ok(c) => {
                    let created = process::creation_time(c.id())
                        .map(|t| t.to_string())
                        .unwrap_or_default();
                    self.log
                        .line(&format!("child pid={} created={created}", c.id()));
                    if let Ok(mut list) = self.browsers.lock() {
                        list.push(c);
                    }
                }
                Err(_) => self.log.line("child-failed"),
            },
            "browser-foreign" => {
                let pid: u32 = rest.parse().unwrap_or(0);
                let created = process::creation_time(pid)
                    .map(|c| c.to_string())
                    .unwrap_or_default();
                let group = process::process_group(pid)
                    .map_or_else(|| "null".to_owned(), |g| g.to_string());
                let json = format!(
                    r#"{{"t":"browser","pid":{pid},"created":"{created}","group":{group}}}"#
                );
                self.send(&json);
                if self.reply().is_none() {
                    self.log.line("no-reply");
                }
            }
            "stderr-marker" => self.stderr_marker(rest),
            "stderr-kind" => self.log.line(&format!("stderr-kind {}", stderr_kind())),
            "write-inherited" => {
                let (number, text) = rest.split_once(' ').unwrap_or((rest, ""));
                let result = write_inherited(number, format!("{text}\n").as_bytes());
                self.log.line(&format!("inherited {number} {result}"));
            }
            "environment" => {
                for line in environment_lines(rest) {
                    self.log.line(&line);
                }
            }
            "environment-is" => {
                let (name, expected) = rest.split_once(' ').unwrap_or((rest, ""));
                let same = |v: &str| {
                    if cfg!(windows) {
                        v.trim_end_matches('\\')
                            .eq_ignore_ascii_case(expected.trim_end_matches('\\'))
                    } else {
                        v == expected
                    }
                };
                let word = match std::env::var(name) {
                    Ok(v) if same(&v) => "same",
                    Ok(_) => "other",
                    Err(_) => "absent",
                };
                self.log.line(&format!("environment-is {name} {word}"));
            }
            "directory" => {
                let yes = if in_system_directory() { "yes" } else { "no" };
                self.log.line(&format!("directory system={yes}"));
            }
            "write-relative" => {
                let made = std::fs::File::create(rest).is_ok();
                self.log.line(&format!(
                    "relative made={}",
                    if made { "yes" } else { "no" }
                ));
                if made && in_system_directory() && std::fs::remove_file(rest).is_ok() {
                    self.log.line("relative removed=yes");
                }
            }
            "console" => {
                let host = rest.parse().unwrap_or(0);
                let me = std::process::id();
                self.log
                    .line(&format!("console {}", console_line(host, me)));
                let reported = std::env::current_exe().and_then(|exe| {
                    Command::new(exe)
                        .arg("console-report")
                        .arg(host.to_string())
                        .arg(me.to_string())
                        .arg(&self.log_path)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                });
                if !reported.is_ok_and(|s| s.success()) {
                    self.log.line("console-child failed");
                }
            }
            "pipes" => self
                .log
                .line(&format!("pipes beyond={}", pipes_beyond_three())),
            "holder" => self.holder(),
            "descriptors" => self
                .log
                .line(&format!("descriptors first={}", self.first_descriptors)),
            "wait-for" => {
                let there = wait_for_file(Path::new(rest), Duration::from_secs(30));
                self.log.line(&format!(
                    "waited found={}",
                    if there { "yes" } else { "no" }
                ));
            }
            "socket" => {
                let ok = std::net::TcpListener::bind("127.0.0.1:0").is_ok();
                self.log
                    .line(&format!("socket ok={}", if ok { "yes" } else { "no" }));
            }
            "stderr-flood" => {
                let (total, after) = rest.split_once(' ').unwrap_or((rest, ""));
                if !after.is_empty() && !wait_for_file(Path::new(after), Duration::from_secs(30)) {
                    self.log.line("stderr-flood no-start");
                    return;
                }
                let ready = after.is_empty() || Path::new(after).exists();
                self.log.line(&format!(
                    "stderr-flood start after={}",
                    if ready { "yes" } else { "no" }
                ));
                let wrote = flood_stderr(total.parse().unwrap_or(0));
                self.log.line(&format!("stderr-flood wrote={wrote}"));
            }
            "on-eof" => {
                let v = match rest.split_once(' ') {
                    Some(("send", port)) => OnEof::Send(port.parse().unwrap_or(0)),
                    Some(("stderr", marker)) => OnEof::Stderr(marker.to_owned()),
                    _ if rest == "hang" => OnEof::Hang,
                    _ => OnEof::End,
                };
                if let Ok(mut o) = self.on_eof.lock() {
                    *o = v;
                }
            }
            "log" => self.log.line(rest),
            "sleep" => thread::sleep(Duration::from_millis(rest.parse().unwrap_or(0))),
            "idle" => sleep_forever(),
            "exit" => {
                self.log.line("exit");
                std::process::exit(rest.parse().unwrap_or(0));
            }
            _ => self.log.line(&format!("unknown-step {word}")),
        }
    }

    fn stderr_marker(&mut self, marker: &str) {
        let line = write_stderr(format!("{marker}\n").as_bytes());
        let mut mixed = vec![0xff, 0xfe, 0x80];
        mixed.extend_from_slice(marker.as_bytes());
        mixed.extend_from_slice(&[0xc3, 0x28, b'\n']);
        let mixed = write_stderr(&mixed);
        let made = std::env::current_exe().and_then(|exe| {
            Command::new(exe)
                .arg("stderr-child")
                .arg(marker)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
        });
        let child = match made {
            Ok(mut c) => {
                let created = process::creation_time(c.id())
                    .map(|t| t.to_string())
                    .unwrap_or_default();
                self.log
                    .line(&format!("child pid={} created={created}", c.id()));
                c.wait().is_ok_and(|s| s.success())
            }
            Err(_) => false,
        };
        let last = write_stderr(marker.as_bytes());
        let yes = |b: bool| if b { "yes" } else { "no" };
        self.log.line(&format!(
            "stderr-marker line={} mixed={} child={} last={}",
            yes(line),
            yes(mixed),
            yes(child),
            yes(last)
        ));
    }

    fn browser(&mut self, rest: &str) {
        let words: Vec<&str> = rest.split_whitespace().collect();
        let has = |w: &str| words.contains(&w);
        let own = !has("in-runner-group");
        let role = if has("child") {
            "browser-with-child"
        } else {
            "browser"
        };
        let spawned = if has("in-group-of-child") {
            self.step("child-own-group");
            browser_in_group_of_last_child(&self.browsers, role, &self.log_path)
        } else {
            spawn_self_with(role, own, Some(&self.log_path))
        };
        let child = match spawned {
            Ok(c) => c,
            Err(_) => {
                self.log.line("browser-failed");
                return;
            }
        };
        let pid = child.id();
        if let Ok(mut list) = self.browsers.lock() {
            list.push(child);
        }
        let mut created = None;
        for _ in 0..250 {
            created = process::creation_time(pid);
            if created.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let real = created.map(|c| c.to_string()).unwrap_or_default();
        self.log
            .line(&format!("browser-real pid={pid} created={real}"));
        let created = if has("created=wrong") {
            "1".to_owned()
        } else {
            real
        };
        let group = if has("group=null") {
            "null".to_owned()
        } else if has("group=other") {
            "2147483647".to_owned()
        } else if has("group=runner-claim") {
            std::process::id().to_string()
        } else {
            process::process_group(pid).map_or_else(|| "null".to_owned(), |g| g.to_string())
        };
        self.log.line(&format!(
            "browser pid={pid} created={created} group={group}"
        ));
        let json =
            format!(r#"{{"t":"browser","pid":{pid},"created":"{created}","group":{group}}}"#);
        self.send(&json);
        if self.reply().is_none() {
            self.log.line("no-reply");
        }
    }

    fn holder(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let made = std::env::current_exe().and_then(|exe| {
                Command::new(exe)
                    .arg("holder-parent")
                    .arg(&self.log_path)
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .status()
            });
            let ok = made.is_ok_and(|s| s.success());
            self.log
                .line(&format!("holder made={}", if ok { "yes" } else { "no" }));
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            self.log.line(&format!(
                "own-handles pid={} in={} out={}",
                std::process::id(),
                io::stdin().as_raw_handle() as usize,
                io::stdout().as_raw_handle() as usize
            ));
        }
    }

    fn breakaway(&mut self) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
            let made = std::env::current_exe().and_then(|exe| {
                Command::new(exe)
                    .arg("child")
                    .creation_flags(CREATE_BREAKAWAY_FROM_JOB)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            });
            match made {
                Ok(mut c) => {
                    self.log.line(&format!("breakaway made pid={}", c.id()));
                    let _ = c.kill();
                    let _ = c.wait();
                }
                Err(_) => self.log.line("breakaway refused"),
            }
        }
        #[cfg(not(windows))]
        self.log.line("breakaway none");
    }
}

fn on_link_end(log: &Log, on_eof: &Mutex<OnEof>, browsers: &Mutex<Vec<Child>>) {
    log.line("eof");
    let what = on_eof.lock().map(|o| o.clone()).unwrap_or(OnEof::End);
    match what {
        OnEof::End => {
            end_browsers(browsers);
            std::process::exit(1);
        }
        OnEof::Stderr(marker) => {
            let wrote = write_stderr(format!("{marker}\n").as_bytes());
            log.line(&format!(
                "stderr-after-eof wrote={}",
                if wrote { "yes" } else { "no" }
            ));
            end_browsers(browsers);
            std::process::exit(1);
        }
        OnEof::Hang => {}
        OnEof::Send(port) => {
            if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                let _ = s.write_all(b"POST /effect HTTP/1.1\r\n\r\n");
                log.line("sent");
            }
        }
    }
}

fn runner(args: &[String]) {
    let first_descriptors = own_descriptors();
    let script = PathBuf::from(arg_after(args, "--browser"));
    let log_path = Path::new(&format!("{}.log", script.display())).to_path_buf();
    let file = match OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(f) => f,
        Err(_) => std::process::exit(70),
    };
    let log = Arc::new(Log(Mutex::new(file)));
    log.line(&format!("args {}", args.len()));
    let me = std::process::id();
    let created = process::creation_time(me)
        .map(|t| t.to_string())
        .unwrap_or_default();
    log.line(&format!("runner pid={me} created={created}"));
    let (tx, rx) = mpsc::channel();
    let on_eof = Arc::new(Mutex::new(OnEof::End));
    let browsers: Arc<Mutex<Vec<Child>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let log = Arc::clone(&log);
        let on_eof = Arc::clone(&on_eof);
        let browsers = Arc::clone(&browsers);
        thread::spawn(move || {
            let mut input = io::stdin().lock();
            loop {
                match read_frame(&mut input) {
                    Ok(Some(bytes)) => {
                        if bytes == br#"{"t":"cancel"}"# {
                            log.line("cancel");
                        } else if tx.send(bytes).is_err() {
                            return;
                        }
                    }
                    _ => {
                        on_link_end(&log, &on_eof, &browsers);
                        return;
                    }
                }
            }
        });
    }
    let text = std::fs::read_to_string(&script).unwrap_or_default();
    let mut peer = Peer {
        run: arg_after(args, "--run"),
        agent: arg_after(args, "--agent"),
        workflow: arg_after(args, "--workflow"),
        log,
        log_path: log_path.clone(),
        out: io::stdout(),
        replies: rx,
        on_eof,
        first_descriptors,
        browsers,
    };
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        peer.step(line.trim());
    }
    peer.log.line("script-end");
    sleep_forever();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("browser") => sleep_forever(),
        Some("writer") => {
            let delay = args.get(1).and_then(|d| d.parse().ok()).unwrap_or(0);
            let count: u32 = args.get(2).and_then(|c| c.parse().ok()).unwrap_or(0);
            let frame = args.get(3).cloned().unwrap_or_default();
            thread::sleep(Duration::from_millis(delay));
            let mut out = io::stdout().lock();
            let mut wrote = 0;
            for _ in 0..count {
                if write_frame(&mut out, &frame).is_ok() {
                    wrote += 1;
                }
                thread::sleep(Duration::from_millis(50));
            }
            if let Some(log) = args.get(4) {
                if let Ok(mut f) = OpenOptions::new().append(true).open(log) {
                    let _ = f.write_all(format!("writer wrote={wrote}\n").as_bytes());
                }
            }
            sleep_forever();
        }
        Some("browser-with-child") => {
            if let Ok(c) = spawn_self("child", false) {
                let created = process::creation_time(c.id())
                    .map(|t| t.to_string())
                    .unwrap_or_default();
                if let Some(log) = args.get(1) {
                    if let Ok(mut f) = OpenOptions::new().append(true).open(log) {
                        let line = format!("browser-child pid={} created={created}\n", c.id());
                        let _ = f.write_all(line.as_bytes());
                    }
                }
            }
            sleep_forever();
        }
        Some("child") => sleep_forever(),
        Some("console-report") => {
            let pid = |i: usize| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(0);
            let line = format!("console-child {}\n", console_line(pid(1), pid(2)));
            let wrote = args.get(3).is_some_and(|log| {
                OpenOptions::new()
                    .append(true)
                    .open(log)
                    .is_ok_and(|mut f| f.write_all(line.as_bytes()).is_ok())
            });
            std::process::exit(if wrote { 0 } else { 1 });
        }
        #[cfg(unix)]
        Some("holder-parent") => {
            use std::os::unix::process::CommandExt;
            let bound = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| HOLD_BOUND.to_string());
            let made = std::env::current_exe().and_then(|exe| {
                Command::new(exe)
                    .arg("holder")
                    .arg(&bound)
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .spawn()
            });
            let Ok(mut c) = made else {
                std::process::exit(1);
            };
            let written = process::creation_time(c.id()).is_some_and(|created| {
                args.get(1).is_some_and(|log| {
                    OpenOptions::new()
                        .append(true)
                        .open(log)
                        .is_ok_and(|mut f| {
                            let line = format!("child pid={} created={created}\n", c.id());
                            f.write_all(line.as_bytes()).is_ok()
                        })
                })
            });
            if !written {
                let _ = c.kill();
                let _ = c.wait();
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        #[cfg(unix)]
        Some("holder") => {
            let bound = args
                .get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(HOLD_BOUND);
            thread::sleep(Duration::from_secs(bound));
            std::process::exit(0);
        }
        #[cfg(windows)]
        Some("hold") => hold(&args),
        Some("stderr-child") => {
            let marker = args.get(1).cloned().unwrap_or_default();
            let wrote = write_stderr(format!("{marker}\n").as_bytes());
            std::process::exit(if wrote { 0 } else { 1 });
        }
        Some("host") => host::run(&args[1..]),
        _ if args.iter().any(|a| a == "--run") => runner(&args),
        _ => std::process::exit(64),
    }
}
