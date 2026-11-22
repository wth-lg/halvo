#![cfg(windows)]
#![deny(clippy::undocumented_unsafe_blocks)]

//! The value Halvo under the Run key, under a standard account. Check 36-app-windows-logon runs
//! the outer test, which starts this test binary's helper as the account halvo-standard through
//! the form --as-standard-account of check 50, so that no case reads or writes the registry of
//! the runner's own account.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use halvo_control::paths::DataDir;
use halvo_control::service::{
    read_start_value, remove_start_value, write_start_value, RemoveOutcome, StartValueSpec,
};
use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_TIMEOUT};

const OUTER: &str = "the_run_value_starts_a_hidden_app_with_one_child_under_a_standard_account";
const HELPER: &str = "helper_run_value";
const ACCOUNT: &str = "halvo-standard";
const REQUEST: &str = "run-value-logon-request.txt";
const STANDARD_ACCOUNT_FORM: &str = "scripts/r1/ci.d/50-windows-functional.sh";
const DAEMON: &str = "halvo-capture.exe";
const CHILD_WITHIN: Duration = Duration::from_secs(60);
const ONE_CHILD_FOR: Duration = Duration::from_secs(3);
const CHILD_END_WITHIN: Duration = Duration::from_secs(12);
const HELPER_WITHIN: Duration = Duration::from_secs(300);
const STEP: Duration = Duration::from_millis(250);

#[test]
#[ignore = "logon: check 36-app-windows-logon runs it; it makes a standard account and starts the built app"]
fn the_run_value_starts_a_hidden_app_with_one_child_under_a_standard_account() {
    let hosted = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted");
    assert!(
        hosted,
        "{OUTER} makes a standard account and starts the built app: it runs on a GitHub-hosted Windows runner alone"
    );
    let app = PathBuf::from(env!("CARGO_BIN_EXE_halvo"));
    let daemon = app.with_file_name(DAEMON);
    assert!(
        daemon.is_file(),
        "{} is not built: check 36 builds it with cargo build -p halvo-capture",
        daemon.display()
    );
    let exe = std::env::current_exe().expect("the test binary has a path");
    let request = exe.with_file_name(REQUEST);
    std::fs::write(&request, format!("app={}\n", app.display())).expect("the request is written");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the folder above src-tauri");
    let mut child = Command::new("sh")
        .current_dir(repo)
        .arg(STANDARD_ACCOUNT_FORM)
        .args(["--as-standard-account", "--"])
        .arg(&exe)
        .args([HELPER, "--exact", "--ignored", "--nocapture"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("sh starts the form of check 50");
    let out = read_all(child.stdout.take());
    let err = read_all(child.stderr.take());
    let until = Instant::now() + HELPER_WITHIN;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < until => std::thread::sleep(STEP),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    println!("{out}");
    eprintln!("{err}");
    let _ = std::fs::remove_file(&request);
    let status = status.unwrap_or_else(|| {
        panic!(
            "the helper as {ACCOUNT} did not end within {} s",
            HELPER_WITHIN.as_secs()
        )
    });
    assert!(
        status.success(),
        "the helper as {ACCOUNT} ended with {status}"
    );
    assert!(
        out.contains(&format!("{HELPER}: PASS")),
        "the helper as {ACCOUNT} printed no PASS line"
    );
}

#[test]
#[ignore = "helper: check 36-app-windows-logon starts it through the_run_value_starts_a_hidden_app_with_one_child_under_a_standard_account as the standard account; alone it does nothing"]
fn helper_run_value() {
    let Some(app) = requested_app() else {
        return;
    };
    let dir = DataDir::for_current_user().expect("the account has a data folder");
    let folder = dir.path().to_string_lossy().to_lowercase();
    assert!(
        folder.contains(&format!(r"\{ACCOUNT}")),
        "{HELPER} runs as {ACCOUNT} alone; this account's data folder is {folder}"
    );
    std::fs::create_dir_all(dir.path()).expect("the data folder is made");
    std::fs::write(dir.state_file(), b"{}\n").expect("the state file of a provisioned folder");
    say("provisioned=yes");

    let spec = StartValueSpec::new(&app).expect("a start value for the built app");
    let mut guard = Guard {
        spec: spec.clone(),
        app: None,
    };
    let written = write_start_value(&spec).expect("the value is written");
    say(&format!("written={written:?}"));
    let line = read_start_value(&spec)
        .expect("the value reads")
        .expect("the value stands");
    assert_eq!(line, spec.data(), "the value reads back as it was written");
    say(&format!("read_back={line}"));

    let (process, pid) = start_command_line(&line).expect("the value starts as one command line");
    guard.app = Some(process);
    say(&format!("started pid={pid}"));
    let began = Instant::now();
    loop {
        let children = children_named(pid, DAEMON);
        assert!(
            children.len() < 2,
            "{} {DAEMON} run as children of the app",
            children.len()
        );
        if children.len() == 1 {
            break;
        }
        assert!(
            still_runs(process),
            "the app ended before its child started"
        );
        assert!(
            began.elapsed() < CHILD_WITHIN,
            "no {DAEMON} ran as the app's child within {} s",
            CHILD_WITHIN.as_secs()
        );
        std::thread::sleep(STEP);
    }
    say(&format!("child_after_ms={}", began.elapsed().as_millis()));
    std::thread::sleep(ONE_CHILD_FOR);
    let children = children_named(pid, DAEMON);
    assert_eq!(children.len(), 1, "the children {DAEMON} of the app");
    say("children=1");
    let visible = visible_windows(pid);
    for window in &visible {
        if window.is_the_crates_event_window() {
            say(&format!("window of the crate, not shown: {window}"));
        } else {
            say(&format!("window {window}"));
        }
    }
    let shown: Vec<&Window> = visible.iter().filter(|window| window.is_shown()).collect();
    let names: Vec<String> = shown.iter().map(|window| window.to_string()).collect();
    assert!(
        shown.is_empty(),
        "the app started by the value shows {} window(s) with an area: {}",
        shown.len(),
        names.join("; ")
    );
    say(&format!(
        "visible_windows=0 with an area but the crate's event window, of {} visible",
        visible.len()
    ));
    let elevated = token_elevated(process).expect("the app's token reads");
    assert!(!elevated, "the app started by the value runs elevated");
    say("token_elevated=false");

    guard.end_the_app();
    let ended = Instant::now();
    while !children_named(pid, DAEMON).is_empty() {
        assert!(
            ended.elapsed() < CHILD_END_WITHIN,
            "the {DAEMON} of the ended app still runs after {} s",
            CHILD_END_WITHIN.as_secs()
        );
        std::thread::sleep(STEP);
    }
    say(&format!(
        "child_ended_after_ms={}",
        ended.elapsed().as_millis()
    ));
    let removed = remove_start_value(&spec).expect("the value is taken away");
    assert_eq!(
        removed,
        RemoveOutcome::Removed,
        "the value stood until then"
    );
    assert_eq!(
        read_start_value(&spec).expect("the value reads"),
        None,
        "no value stands after its removal"
    );
    say("removed=yes");
    say("PASS");
}

fn say(line: &str) {
    println!("{HELPER}: {line}");
}

fn read_all(stream: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut text);
        }
        String::from_utf8_lossy(&text).into_owned()
    })
}

/// The built app of the outer test's request beside this binary; none where no request stands.
fn requested_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let text = std::fs::read_to_string(exe.with_file_name(REQUEST)).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix("app="))
        .map(PathBuf::from)
}

/// Ends the app that the helper started and takes the value away, also when a case panics.
struct Guard {
    spec: StartValueSpec,
    app: Option<HANDLE>,
}

impl Guard {
    fn end_the_app(&mut self) {
        use windows::Win32::System::Threading::TerminateProcess;
        if let Some(process) = self.app.take() {
            // SAFETY: `process` came from CreateProcessW, and this guard alone holds and closes
            // it, once.
            unsafe {
                let _ = TerminateProcess(process, 1);
                let _ = CloseHandle(process);
            }
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.end_the_app();
        let _ = remove_start_value(&self.spec);
    }
}

/// Starts `line` as one command line, as a logon starts the value: no application name, so
/// that the system reads the program from the line's own quotes. The test build of halvo.exe is
/// a console program (the release build alone sets the windows subsystem), so it starts with
/// no console window.
fn start_command_line(line: &str) -> windows::core::Result<(HANDLE, u32)> {
    use windows::Win32::System::Threading::{
        CreateProcessW, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let mut text: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
    let startup = STARTUPINFOW {
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: `text` is a NUL-terminated wide string that lives across the call and that the
    // call may write, as CreateProcessW asks of its command line; `startup` holds its own size
    // in cb, and `info` is a live local that the call fills.
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(text.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &startup,
            &mut info,
        )
    }?;
    // SAFETY: the thread handle is new and owned here alone.
    let _ = unsafe { CloseHandle(info.hThread) };
    Ok((info.hProcess, info.dwProcessId))
}

fn still_runs(process: HANDLE) -> bool {
    use windows::Win32::System::Threading::WaitForSingleObject;
    // SAFETY: `process` is a live process handle of the guard.
    unsafe { WaitForSingleObject(process, 0) == WAIT_TIMEOUT }
}

/// The ids of the processes named `name` whose parent is `parent`, from one snapshot of the
/// process table.
fn children_named(parent: u32, name: &str) -> Vec<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: the call takes plain values and returns a new snapshot handle, which is closed
    // below, or an error.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .expect("a snapshot of the process table");
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..Default::default()
    };
    let mut found = Vec::new();
    // SAFETY: the snapshot is open, and `entry` is a live PROCESSENTRY32W whose dwSize holds
    // its size, which the call fills.
    let mut read = unsafe { Process32FirstW(snapshot, &mut entry) };
    while read.is_ok() {
        let end = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if entry.th32ParentProcessID == parent && exe.eq_ignore_ascii_case(name) {
            found.push(entry.th32ProcessID);
        }
        // SAFETY: as for Process32FirstW above.
        read = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    // SAFETY: the snapshot is open and owned here alone.
    let _ = unsafe { CloseHandle(snapshot) };
    found
}

/// What the helper reads of a visible top-level window of the app.
struct Window {
    class: String,
    title: String,
    style: u32,
    extended: u32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// The event window of the windowing crate that Cargo.lock pins (tao 0.37.2): its Windows event
/// loop makes it at size 0, layered, click-through and a tool window, and keeps it in the
/// visible state so that it receives paint messages; the system reads it at 16 by 16 at the
/// screen's upper left corner (run 45044830383 printed it so).
const CRATE_EVENT_WINDOW_CLASS: &str = "Tao Thread Event Target";
const CRATE_EVENT_WINDOW_STYLE: u32 = 0x9400_0000;
const CRATE_EVENT_WINDOW_EXSTYLE: u32 = 0x0808_00a0;
const CRATE_EVENT_WINDOW_MOST: i32 = 16;

impl Window {
    /// The case counts as shown every visible window whose rectangle has an area, a tool window
    /// among them, but one: the crate's event window, named exactly by its class, its empty
    /// title, its style and extended style and a rectangle of at most 16 by 16 at 0,0. No run
    /// shows whether Windows draws that window, which is a named limit of the unit and a hand
    /// check of the owner's sitting; any other window that the app shows at a hidden start fails
    /// the case by its name and size.
    fn is_shown(&self) -> bool {
        self.has_an_area() && !self.is_the_crates_event_window()
    }

    fn has_an_area(&self) -> bool {
        self.right > self.left && self.bottom > self.top
    }

    fn is_the_crates_event_window(&self) -> bool {
        self.class == CRATE_EVENT_WINDOW_CLASS
            && self.title.is_empty()
            && self.style == CRATE_EVENT_WINDOW_STYLE
            && self.extended == CRATE_EVENT_WINDOW_EXSTYLE
            && self.left == 0
            && self.top == 0
            && (0..=CRATE_EVENT_WINDOW_MOST).contains(&self.right)
            && (0..=CRATE_EVENT_WINDOW_MOST).contains(&self.bottom)
    }
}

impl std::fmt::Display for Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "class={:?} title={:?} style=0x{:08x} exstyle=0x{:08x} rect={},{},{},{} size={}x{}",
            self.class,
            self.title,
            self.style,
            self.extended,
            self.left,
            self.top,
            self.right,
            self.bottom,
            self.right - self.left,
            self.bottom - self.top
        )
    }
}

/// The visible top-level windows of the process `pid`, each with its class name, title, style,
/// extended style and rectangle.
fn visible_windows(pid: u32) -> Vec<Window> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE,
    };
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the address of the vector that the caller holds across the
        // enumeration on this thread.
        unsafe { (*(found.0 as *mut Vec<HWND>)).push(hwnd) };
        BOOL::from(true)
    }
    let mut all: Vec<HWND> = Vec::new();
    // SAFETY: the callback has the WNDENUMPROC signature and writes `all` through a pointer made
    // from an exclusive borrow, only while this enumeration runs on this thread.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut Vec<HWND> as isize));
    }
    all.into_iter()
        .filter(|hwnd| {
            let mut owner = 0u32;
            // SAFETY: the call reads a window handle and writes the live local `owner`.
            unsafe { GetWindowThreadProcessId(*hwnd, Some(&mut owner)) };
            // SAFETY: IsWindowVisible reads a handle value.
            owner == pid && unsafe { IsWindowVisible(*hwnd) }.as_bool()
        })
        .map(|hwnd| {
            let mut class = [0u16; 256];
            let mut title = [0u16; 256];
            // SAFETY: each call reads a window handle and writes at most the length of the live
            // local buffer that it is given.
            let (class_len, title_len) = unsafe {
                (
                    GetClassNameW(hwnd, &mut class),
                    GetWindowTextW(hwnd, &mut title),
                )
            };
            let text = |units: &[u16], len: i32| {
                let len = usize::try_from(len).unwrap_or(0).min(units.len());
                String::from_utf16_lossy(&units[..len])
            };
            // SAFETY: each call reads a window handle and an index of the window's longs.
            let (style, extended) = unsafe {
                (
                    GetWindowLongW(hwnd, GWL_STYLE),
                    GetWindowLongW(hwnd, GWL_EXSTYLE),
                )
            };
            let mut rect = RECT::default();
            // SAFETY: the call reads a window handle and writes the live local `rect`.
            let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
            Window {
                class: text(&class, class_len),
                title: text(&title, title_len),
                style: u32::from_ne_bytes(style.to_ne_bytes()),
                extended: u32::from_ne_bytes(extended.to_ne_bytes()),
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            }
        })
        .collect()
}

/// Whether the token of `process` is elevated.
fn token_elevated(process: HANDLE) -> windows::core::Result<bool> {
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::OpenProcessToken;
    let mut token = HANDLE::default();
    // SAFETY: `process` is a live process handle of the guard, and `token` is a live local that
    // receives a new handle, closed below.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut needed = 0u32;
    let size = u32::try_from(std::mem::size_of::<TOKEN_ELEVATION>()).unwrap_or(u32::MAX);
    // SAFETY: `elevation` holds `size` bytes for the call to write, and `needed` is a live local.
    let read = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            size,
            &mut needed,
        )
    };
    // SAFETY: `token` is the new handle of OpenProcessToken, owned here alone.
    let _ = unsafe { CloseHandle(token) };
    read.map(|()| elevation.TokenIsElevated != 0)
}
