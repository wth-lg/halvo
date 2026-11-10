//! Functional checks of the Windows capture backend on a real Windows machine.
//!
//! Every test is ignored by default. The CI step runs each one by name in its
//! own process with `--ignored --nocapture`, so one claim's hooks, windows and
//! memory never reach another's. A test prints `HALVO-VERIFY <claim>` evidence
//! lines and asserts the claim's pass condition. The lines hold only what the
//! test's own scripted activity produced: counts, sizes, booleans and the file
//! names of executables.
#![cfg(target_os = "windows")]

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use halvo_capture::custody::windows::DpapiCustody;
use halvo_capture::custody::{CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};
use halvo_capture::event::{CaptureStreamEvent, CapturedAt, Modality, MonotonicSequence};
use halvo_capture::service::windows::{
    task_registers, TaskSpec, WindowsTaskInstaller, DEFAULT_TASK_NAME,
};
use halvo_capture::service::{ServiceInstaller, ServiceStatus};
use halvo_capture::sink::{channel, SinkWriter};
use halvo_capture::source::CaptureSource;
use halvo_capture::store::db::EncryptedStore;
use halvo_capture::store::{SegmentExpiry, SegmentStore, StoreError, DEFAULT_RETENTION};
use halvo_capture::windows::apps::{scan_installed, wait_for_change};
use halvo_capture::windows::foreground::{current_foreground_app, ForegroundAppSource};
use halvo_capture::windows::input::RawInputSource;
use halvo_capture::windows::screen::{exclude_window_from_capture, ScreenSource};
use halvo_capture::windows::window_title::WindowTitleSource;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOUSEEVENTF_MOVE, MOUSEINPUT, VIRTUAL_KEY, VK_A, VK_F15,
    VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetForegroundWindow, GetSystemMetrics, GetWindowDisplayAffinity, GetWindowTextLengthW,
    GetWindowThreadProcessId, PeekMessageW, RegisterClassW, SetForegroundWindow, SetWindowPos,
    SetWindowTextW, ShowWindow, TranslateMessage, UnregisterClassW, ES_PASSWORD, MSG, PM_REMOVE,
    SC_KEYMENU, SM_CMONITORS, SM_CXSCREEN, SM_CYSCREEN, SM_REMOTESESSION, SWP_NOACTIVATE,
    SWP_NOSIZE, SWP_NOZORDER, SW_MINIMIZE, SW_RESTORE, SW_SHOW, WDA_EXCLUDEFROMCAPTURE,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_EX_TOPMOST,
    WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

#[link(name = "kernel32")]
extern "system" {
    fn ProcessIdToSessionId(process_id: u32, session_id: *mut u32) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn GetProcessWindowStation() -> *mut core::ffi::c_void;
    fn GetThreadDesktop(thread_id: u32) -> *mut core::ffi::c_void;
    fn OpenInputDesktop(flags: u32, inherit: i32, access: u32) -> *mut core::ffi::c_void;
    fn CloseDesktop(desktop: *mut core::ffi::c_void) -> i32;
    fn GetUserObjectInformationW(
        object: *mut core::ffi::c_void,
        index: i32,
        info: *mut core::ffi::c_void,
        len: u32,
        needed: *mut u32,
    ) -> i32;
}

const UOI_NAME: i32 = 2;
const DESKTOP_READOBJECTS: u32 = 0x0001;

/// The app string the input and screen tests put in the shared last-app cell,
/// to see that a source attributes its events to it.
const ATTRIBUTION: &str = "halvo-verify-attributed-app";
const CHILD_SECS_ENV: &str = "HALVO_VERIFY_CHILD_SECS";
const CHILD_EXE_NAME: &str = "halvo-verify-child.exe";
/// COLORREF is 0x00BBGGRR: full red and blue, no green.
const MAGENTA: u32 = 0x00FF_00FF;
const BUDGET_48_MIB: usize = 48 * 1024 * 1024;

fn say(claim: &str, message: impl AsRef<str>) {
    println!("HALVO-VERIFY {claim} {}", message.as_ref());
}

fn show<T, E: std::fmt::Display>(result: &Result<T, E>) -> String {
    match result {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("error({e})"),
    }
}

/// Run `body` on its own thread and fail the test if it has not finished
/// within `limit`, so a source that hangs in start or stop cannot hold the job.
fn with_deadline<F>(claim: &'static str, limit: Duration, body: F)
where
    F: FnOnce() + Send + 'static,
{
    let (done, finished) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name(format!("verify-{claim}"))
        .spawn(move || {
            body();
            let _ = done.send(());
        })
        .expect("spawn the test body");
    match finished.recv_timeout(limit) {
        Ok(()) => worker.join().expect("test body finished"),
        Err(RecvTimeoutError::Timeout) => {
            say(
                claim,
                format!("result=TIMEOUT limit_secs={}", limit.as_secs()),
            );
            panic!("{claim} did not finish within {limit:?}");
        }
        Err(RecvTimeoutError::Disconnected) => {
            if let Err(payload) = worker.join() {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

fn user_object_name(handle: *mut core::ffi::c_void) -> String {
    if handle.is_null() {
        return "<none>".to_string();
    }
    let mut buf = [0u16; 256];
    let mut needed = 0u32;
    let ok = unsafe {
        GetUserObjectInformationW(
            handle,
            UOI_NAME,
            buf.as_mut_ptr().cast(),
            (buf.len() * 2) as u32,
            &mut needed,
        )
    };
    if ok == 0 {
        return "<unreadable>".to_string();
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// One line on what kind of session the test runs in: the facts that decide
/// whether input, windows and the screen exist at all on this machine.
fn env_report(claim: &str) {
    let mut session = u32::MAX;
    let session_ok = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } != 0;
    let station = user_object_name(unsafe { GetProcessWindowStation() });
    let desktop = user_object_name(unsafe { GetThreadDesktop(GetCurrentThreadId()) });
    let input_desktop = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    let input_name = user_object_name(input_desktop);
    if !input_desktop.is_null() {
        unsafe {
            CloseDesktop(input_desktop);
        }
    }
    let (cx, cy) = screen_size();
    let (monitors, remote) = unsafe {
        (
            GetSystemMetrics(SM_CMONITORS),
            GetSystemMetrics(SM_REMOTESESSION),
        )
    };
    let foreground = unsafe { GetForegroundWindow() };
    say(
        claim,
        format!(
            "env github_sha={} session_id_ok={session_ok} session_id={session} \
             window_station={station} desktop={desktop} input_desktop={input_name} \
             screen={cx}x{cy} monitors={monitors} remote_session={remote} \
             foreground_window_present={} foreground_app_file={:?}",
            std::env::var("GITHUB_SHA").unwrap_or_else(|_| "unset".to_string()),
            !foreground.0.is_null(),
            current_foreground_app().as_deref().map(file_name),
        ),
    );
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn key_input(key: VIRTUAL_KEY, released: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if released {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_move(dx: i32, dy: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Insert the events into the input stream; returns how many the OS accepted.
fn send(inputs: &[INPUT]) -> u32 {
    unsafe { SendInput(inputs, core::mem::size_of::<INPUT>() as i32) }
}

/// Press and release F15 `presses` times; returns the accepted event count.
/// F15 types nothing into whatever window has the focus.
fn press_keys(presses: u32) -> u32 {
    let mut accepted = 0;
    for _ in 0..presses {
        accepted += send(&[key_input(VK_F15, false)]);
        std::thread::sleep(Duration::from_millis(5));
        accepted += send(&[key_input(VK_F15, true)]);
        std::thread::sleep(Duration::from_millis(5));
    }
    accepted
}

/// Move the pointer `moves` times by three pixels, alternating direction so it
/// ends where it began; returns the accepted event count.
fn move_pointer(moves: u32) -> u32 {
    let mut accepted = 0;
    for i in 0..moves {
        let dx = if i.is_multiple_of(2) { 3 } else { -3 };
        accepted += send(&[mouse_move(dx, 0)]);
        std::thread::sleep(Duration::from_millis(5));
    }
    accepted
}

/// Collect every event the sink delivers within `window`.
fn drain_for(writer: &SinkWriter, window: Duration) -> Vec<CaptureStreamEvent> {
    let deadline = Instant::now() + window;
    let mut events = Vec::new();
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match writer.receiver().recv_timeout(deadline - now) {
            Ok(event) => events.push(event),
            Err(_) => break,
        }
    }
    events
}

fn count(events: &[CaptureStreamEvent], modality: Modality) -> usize {
    events.iter().filter(|e| e.modality == modality).count()
}

/// Print the events of one phase grouped by modality, app file name and
/// payload size. The events carry no content, so neither do the lines.
fn summarize(claim: &str, phase: &str, events: &[CaptureStreamEvent]) {
    let mut groups: BTreeMap<(String, String, u64), usize> = BTreeMap::new();
    for event in events {
        let key = (
            format!("{:?}", event.modality),
            file_name(&event.foreground_app).to_string(),
            event.segment_ref.byte_len,
        );
        *groups.entry(key).or_default() += 1;
    }
    say(
        claim,
        format!(
            "phase={phase} events={} groups={}",
            events.len(),
            groups.len()
        ),
    );
    for ((modality, app, byte_len), n) in groups.iter().take(40) {
        say(
            claim,
            format!(
                "phase={phase} modality={modality} app_file={app:?} byte_len={byte_len} count={n}"
            ),
        );
    }
    if groups.len() > 40 {
        say(
            claim,
            format!("phase={phase} groups_not_printed={}", groups.len() - 40),
        );
    }
}

unsafe extern "system" fn test_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // A tapped Alt unlocks SetForegroundWindow; it must not also put the window
    // into the modal menu loop, which would stall the pump.
    if msg == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) == SC_KEYMENU as usize {
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Dispatch this thread's window messages for `window`.
fn pump_thread(window: Duration) {
    let deadline = Instant::now() + window;
    let mut msg = MSG::default();
    loop {
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct WindowSpec<'a> {
    class: &'a str,
    title: &'a str,
    style: WINDOW_STYLE,
    ex_style: WINDOW_EX_STYLE,
    fill: u32,
    x: i32,
    y: i32,
}

const WINDOW_WIDTH: i32 = 360;
const WINDOW_HEIGHT: i32 = 240;

/// A visible top-level window owned by the calling thread, filled with one
/// color. Destroyed, and its class unregistered, on drop.
struct TestWindow {
    hwnd: HWND,
    class_name: HSTRING,
    hinstance: HINSTANCE,
}

impl TestWindow {
    fn create(spec: &WindowSpec) -> Result<Self, String> {
        let class_name = HSTRING::from(spec.class);
        let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .map_err(|e| format!("GetModuleHandleW: {e}"))?
            .into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(test_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hbrBackground: unsafe { CreateSolidBrush(COLORREF(spec.fill)) },
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(format!(
                "RegisterClassW: {}",
                windows::core::Error::from_thread()
            ));
        }
        let title = HSTRING::from(spec.title);
        let hwnd = unsafe {
            CreateWindowExW(
                spec.ex_style,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                spec.style,
                spec.x,
                spec.y,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                None,
                None,
                Some(hinstance),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW: {e}"))?;
        Ok(Self {
            hwnd,
            class_name,
            hinstance,
        })
    }

    fn set_title(&self, title: &str) -> Result<(), String> {
        let title = HSTRING::from(title);
        unsafe { SetWindowTextW(self.hwnd, &title) }.map_err(|e| format!("SetWindowTextW: {e}"))
    }

    fn move_to(&self, x: i32, y: i32) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn is_foreground(&self) -> bool {
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    /// Make this window the foreground window, trying the plain call first and
    /// then the documented ways around the foreground lock. Returns whether the
    /// OS reports it as the foreground window and which way worked.
    fn activate(&self) -> (bool, &'static str) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetForegroundWindow(self.hwnd);
        }
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return (true, "SetForegroundWindow");
        }

        send(&[key_input(VK_MENU, false)]);
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        send(&[key_input(VK_MENU, true)]);
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return (true, "alt-key-unlock");
        }

        unsafe {
            let current = GetForegroundWindow();
            let current_thread = GetWindowThreadProcessId(current, None);
            let this_thread = GetCurrentThreadId();
            if current_thread != 0 && current_thread != this_thread {
                let _ = AttachThreadInput(this_thread, current_thread, true);
                let _ = BringWindowToTop(self.hwnd);
                let _ = SetForegroundWindow(self.hwnd);
                let _ = AttachThreadInput(this_thread, current_thread, false);
            }
        }
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return (true, "attach-thread-input");
        }

        unsafe {
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
        }
        pump_thread(Duration::from_millis(200));
        (self.is_foreground(), "minimize-restore")
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(PCWSTR(self.class_name.as_ptr()), Some(self.hinstance));
        }
        pump_thread(Duration::from_millis(50));
    }
}

fn normal_window<'a>(class: &'a str, title: &'a str, x: i32, y: i32) -> WindowSpec<'a> {
    WindowSpec {
        class,
        title,
        style: WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        ex_style: WINDOW_EX_STYLE(0),
        fill: 0x0000_80FF,
        x,
        y,
    }
}

fn magenta_popup<'a>(class: &'a str, title: &'a str) -> WindowSpec<'a> {
    WindowSpec {
        class,
        title,
        style: WS_POPUP | WS_VISIBLE,
        ex_style: WS_EX_TOPMOST,
        fill: MAGENTA,
        x: 100,
        y: 100,
    }
}

/// The title the child gives its first window at `step`: 14 to 21 bytes, so a
/// window-title event's payload size shows it came from one of these titles.
fn child_title(step: usize) -> String {
    format!("halvo-verify-{}", "x".repeat(step % 8 + 1))
}

const CHILD_TITLE_LENGTHS: std::ops::RangeInclusive<u64> = 14..=21;

/// The process whose windows the foreground and window-title hooks observe.
/// Those hooks skip the process that installed them, so the windows have to
/// belong to another process: this same binary, copied under another name and
/// launched with [`CHILD_SECS_ENV`] set. Run directly, the test does nothing.
#[test]
#[ignore = "helper: does nothing unless a functional test launches it as a child process"]
fn helper_child_window() {
    let Ok(secs) = std::env::var(CHILD_SECS_ENV) else {
        return;
    };
    let secs: u64 = secs.parse().expect("child seconds");
    let first = TestWindow::create(&normal_window(
        "HalvoVerifyChildFirst",
        &child_title(0),
        40,
        40,
    ))
    .expect("first child window");
    let second = TestWindow::create(&normal_window(
        "HalvoVerifyChildSecond",
        "halvo-verify-second-window-with-a-fixed-title",
        220,
        180,
    ))
    .expect("second child window");
    pump_thread(Duration::from_millis(300));
    let (foreground, method) = first.activate();
    say(
        "child",
        format!(
            "pid={} first_window_foreground={foreground} method={method}",
            std::process::id()
        ),
    );

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut step = 0usize;
    let mut switches = 0u32;
    let mut switches_confirmed = 0u32;
    while Instant::now() < deadline {
        step += 1;
        first
            .set_title(&child_title(step))
            .expect("set the child title");
        pump_thread(Duration::from_millis(250));
        if step.is_multiple_of(2) {
            let target = if (step / 2) % 2 == 1 { &second } else { &first };
            unsafe {
                let _ = SetForegroundWindow(target.hwnd);
            }
            pump_thread(Duration::from_millis(50));
            switches += 1;
            if target.is_foreground() {
                switches_confirmed += 1;
            }
        }
        pump_thread(Duration::from_millis(200));
    }
    say(
        "child",
        format!(
            "title_changes={step} foreground_switches={switches} \
             switches_confirmed={switches_confirmed}"
        ),
    );
}

struct ChildProcess {
    process: std::process::Child,
    _dir: tempfile::TempDir,
}

fn spawn_child(secs: u64) -> ChildProcess {
    let dir = tempfile::tempdir().expect("temp dir for the child executable");
    let exe = dir.path().join(CHILD_EXE_NAME);
    std::fs::copy(std::env::current_exe().expect("current exe"), &exe)
        .expect("copy the test binary");
    let process = Command::new(&exe)
        .args(["helper_child_window", "--exact", "--ignored", "--nocapture"])
        .env(CHILD_SECS_ENV, secs.to_string())
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn the child");
    ChildProcess { process, _dir: dir }
}

fn foreground_pid() -> u32 {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return 0;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid
    }
}

fn is_child_app(app: &str) -> bool {
    file_name(app).eq_ignore_ascii_case(CHILD_EXE_NAME)
}

/// What the foreground and window-title sources reported while a window of
/// this process, and then windows of a child process, were created, activated
/// and retitled.
struct WindowScenario {
    foreground_start: Result<(), String>,
    title_start: Result<(), String>,
    own_window_was_foreground: bool,
    own_events: Vec<CaptureStreamEvent>,
    own_exe_file: String,
    child_was_foreground: bool,
    last_app_was_child: bool,
    child_exit_ok: bool,
    child_events: Vec<CaptureStreamEvent>,
}

fn window_scenario(claim: &'static str) -> WindowScenario {
    env_report(claim);
    let (sink, writer) = channel(8192);
    let mut foreground = ForegroundAppSource::new();
    let last_app = foreground.last_app();
    let mut title = WindowTitleSource::new();
    let foreground_start = foreground.start(sink.clone()).map_err(|e| e.to_string());
    let title_start = title.start(sink.clone()).map_err(|e| e.to_string());
    say(
        claim,
        format!(
            "foreground_start={} title_start={}",
            show(&foreground_start),
            show(&title_start)
        ),
    );

    summarize(
        claim,
        "ambient-1s",
        &drain_for(&writer, Duration::from_secs(1)),
    );

    let own_exe = std::env::current_exe().expect("current exe");
    let own_exe_file = file_name(&own_exe.to_string_lossy()).to_string();
    let own_title_first = "o".repeat(40);
    let own_title_second = "o".repeat(41);
    let own = TestWindow::create(&normal_window(
        "HalvoVerifyOwnWindow",
        &own_title_first,
        60,
        60,
    ))
    .expect("own window");
    pump_thread(Duration::from_millis(300));
    let (own_window_was_foreground, own_method) = own.activate();
    own.set_title(&own_title_second).expect("own title");
    pump_thread(Duration::from_millis(400));
    drop(own);
    let own_events = drain_for(&writer, Duration::from_secs(1));
    say(
        claim,
        format!(
            "phase=own-process-window was_foreground={own_window_was_foreground} \
             method={own_method} own_exe_file={own_exe_file:?}"
        ),
    );
    summarize(claim, "own-process-window", &own_events);

    let mut child = spawn_child(4);
    let child_pid = child.process.id();
    let mut child_events = Vec::new();
    let mut child_was_foreground = false;
    let mut last_app_was_child = false;
    let exit = loop {
        child_events.extend(drain_for(&writer, Duration::from_millis(100)));
        child_was_foreground |= foreground_pid() == child_pid;
        if let Ok(cell) = last_app.lock() {
            last_app_was_child |= cell.as_deref().is_some_and(is_child_app);
        }
        if let Some(status) = child.process.try_wait().expect("poll the child") {
            break status;
        }
    };
    child_events.extend(drain_for(&writer, Duration::from_secs(1)));
    foreground.stop();
    title.stop();
    say(
        claim,
        format!(
            "phase=child-process-window child_exit_ok={} os_reported_child_foreground={child_was_foreground} \
             last_app_cell_was_child={last_app_was_child}",
            exit.success()
        ),
    );
    summarize(claim, "child-process-window", &child_events);

    WindowScenario {
        foreground_start,
        title_start,
        own_window_was_foreground,
        own_events,
        own_exe_file,
        child_was_foreground,
        last_app_was_child,
        child_exit_ok: exit.success(),
        child_events,
    }
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn c1_raw_input_delivers_keystrokes_and_pointer_events() {
    with_deadline("c1", Duration::from_secs(90), || {
        const PRESSES: u32 = 10;
        const MOVES: u32 = 10;
        env_report("c1");
        let (sink, writer) = channel(4096);
        let last_app = Arc::new(Mutex::new(Some(ATTRIBUTION.to_string())));
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app));
        let mut pointer = RawInputSource::pointer(Arc::clone(&last_app));
        let keyboard_start = keyboard.start(sink.clone());
        let pointer_start = pointer.start(sink.clone());
        say(
            "c1",
            format!(
                "keyboard_start={} pointer_start={}",
                show(&keyboard_start),
                show(&pointer_start)
            ),
        );

        let quiet = drain_for(&writer, Duration::from_secs(1));
        summarize("c1", "no-input-1s", &quiet);

        let key_events_accepted = press_keys(PRESSES);
        let after_keys = drain_for(&writer, Duration::from_millis(1500));
        summarize("c1", "after-key-presses", &after_keys);

        let move_events_accepted = move_pointer(MOVES);
        let after_moves = drain_for(&writer, Duration::from_millis(1500));
        summarize("c1", "after-pointer-moves", &after_moves);

        keyboard.stop();
        pointer.stop();
        press_keys(3);
        move_pointer(3);
        let after_stop = drain_for(&writer, Duration::from_millis(700));
        summarize("c1", "after-stop", &after_stop);

        let keystrokes = count(&after_keys, Modality::Keystroke);
        let pointers = count(&after_moves, Modality::Pointer);
        let attributed = after_keys
            .iter()
            .chain(&after_moves)
            .all(|e| e.foreground_app == ATTRIBUTION);
        say(
            "c1",
            format!(
                "key_presses_sent={PRESSES} sendinput_key_events_accepted={key_events_accepted} \
                 keystroke_events={keystrokes} one_event_per_press={} \
                 pointer_moves_sent={MOVES} sendinput_move_events_accepted={move_events_accepted} \
                 pointer_events={pointers} one_event_per_move={} \
                 events_before_input={} events_after_stop={} attributed_to_last_app={attributed}",
                keystrokes == PRESSES as usize,
                pointers == MOVES as usize,
                quiet.len(),
                after_stop.len(),
            ),
        );

        assert!(keyboard_start.is_ok(), "the keyboard source must start");
        assert!(pointer_start.is_ok(), "the pointer source must start");
        assert!(
            key_events_accepted > 0 && move_events_accepted > 0,
            "INCONCLUSIVE: SendInput inserted no events on this machine"
        );
        assert!(keystrokes >= 1, "no keystroke event for {PRESSES} presses");
        assert!(pointers >= 1, "no pointer event for {MOVES} moves");
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn c2_screen_source_delivers_a_frame() {
    with_deadline("c2", Duration::from_secs(120), || {
        env_report("c2");
        let (cx, cy) = screen_size();
        let expected_len = cx as u64 * cy as u64 * 4;
        let (sink, writer) = channel(1024);
        let last_app = Arc::new(Mutex::new(Some(ATTRIBUTION.to_string())));
        let mut screen = ScreenSource::new(last_app);
        let started = Instant::now();
        let start = screen.start(sink.clone());
        say(
            "c2",
            format!(
                "screen_start={} start_ms={}",
                show(&start),
                started.elapsed().as_millis()
            ),
        );
        assert!(start.is_ok(), "the screen source must start");

        let first = writer.receiver().recv_timeout(Duration::from_secs(10));
        say(
            "c2",
            format!(
                "first_frame_within_10s={} first_frame_ms={}",
                first.is_ok(),
                started.elapsed().as_millis()
            ),
        );
        let mut idle: Vec<CaptureStreamEvent> = first.into_iter().collect();
        idle.extend(drain_for(&writer, Duration::from_secs(4)));
        summarize("c2", "unchanged-screen", &idle);

        let window = TestWindow::create(&magenta_popup("HalvoVerifyFrames", "halvo-verify-frames"))
            .expect("moving window");
        for step in 0..20 {
            window.move_to(100 + step * 20, 100 + step * 10);
            pump_thread(Duration::from_millis(250));
        }
        drop(window);
        let moving = drain_for(&writer, Duration::from_millis(700));
        summarize("c2", "moving-window-5s", &moving);

        screen.stop();
        let after_stop = drain_for(&writer, Duration::from_millis(1500));
        let frames: Vec<&CaptureStreamEvent> = idle
            .iter()
            .chain(&moving)
            .filter(|e| e.modality == Modality::ScreenFrame)
            .collect();
        say(
            "c2",
            format!(
                "frames_unchanged_screen={} frames_moving_window={} frames_after_stop={} \
                 expected_byte_len={expected_len} every_frame_is_full_screen={} \
                 attributed_to_last_app={} dropped={}",
                count(&idle, Modality::ScreenFrame),
                count(&moving, Modality::ScreenFrame),
                after_stop.len(),
                frames
                    .iter()
                    .all(|e| e.segment_ref.byte_len == expected_len),
                frames.iter().all(|e| e.foreground_app == ATTRIBUTION),
                sink.dropped_count(),
            ),
        );
        assert!(!frames.is_empty(), "no frame of the desktop in 15 seconds");
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn c3_foreground_source_reports_an_activated_window() {
    with_deadline("c3", Duration::from_secs(90), || {
        let seen = window_scenario("c3");
        let child_foreground = seen
            .child_events
            .iter()
            .filter(|e| e.modality == Modality::ForegroundApp && is_child_app(&e.foreground_app))
            .count();
        let own_foreground = seen
            .own_events
            .iter()
            .filter(|e| {
                e.modality == Modality::ForegroundApp
                    && file_name(&e.foreground_app).eq_ignore_ascii_case(&seen.own_exe_file)
            })
            .count();
        say(
            "c3",
            format!(
                "foreground_events_for_child_window={child_foreground} \
                 os_reported_child_foreground={} last_app_cell_was_child={} \
                 foreground_events_for_own_process_window={own_foreground} \
                 own_window_was_foreground={}",
                seen.child_was_foreground, seen.last_app_was_child, seen.own_window_was_foreground,
            ),
        );
        assert!(
            seen.foreground_start.is_ok(),
            "the foreground source must start"
        );
        assert!(seen.child_exit_ok, "the child process must finish");
        assert!(
            seen.child_was_foreground,
            "INCONCLUSIVE: the OS never made the child window the foreground window"
        );
        assert!(
            child_foreground >= 1,
            "no foreground event for the activated child window"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn c4_window_title_source_reports_a_title_change() {
    with_deadline("c4", Duration::from_secs(90), || {
        let seen = window_scenario("c4");
        let child_titles: Vec<u64> = seen
            .child_events
            .iter()
            .filter(|e| e.modality == Modality::WindowTitle && is_child_app(&e.foreground_app))
            .map(|e| e.segment_ref.byte_len)
            .collect();
        let expected_sizes = child_titles
            .iter()
            .filter(|len| CHILD_TITLE_LENGTHS.contains(len))
            .count();
        let own_titles = seen
            .own_events
            .iter()
            .filter(|e| {
                e.modality == Modality::WindowTitle
                    && file_name(&e.foreground_app).eq_ignore_ascii_case(&seen.own_exe_file)
            })
            .count();
        say(
            "c4",
            format!(
                "title_events_for_child_window={} with_a_scripted_title_size={expected_sizes} \
                 title_events_for_own_process_window={own_titles}",
                child_titles.len(),
            ),
        );
        assert!(
            seen.title_start.is_ok(),
            "the window-title source must start"
        );
        assert!(seen.child_exit_ok, "the child process must finish");
        assert!(
            expected_sizes >= 1,
            "no window-title event for the child's title changes"
        );
    });
}

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_WOW: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
const PROBE_KEY: &str = "HalvoVerifyProbe";

/// Start `wait_for_change` on its own thread; the receiver gets its result.
fn arm_watcher() -> mpsc::Receiver<Result<(), String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(wait_for_change().map_err(|e| e.to_string()));
    });
    std::thread::sleep(Duration::from_millis(500));
    rx
}

#[test]
#[ignore = "functional: reads and writes the Windows registry"]
fn c5_installed_software_returns_registry_entries() {
    with_deadline("c5", Duration::from_secs(90), || {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        for (hive_name, hive) in [("HKLM", &hklm), ("HKCU", &hkcu)] {
            for (path_name, path) in [("native", UNINSTALL), ("wow6432node", UNINSTALL_WOW)] {
                let subkeys = hive
                    .open_subkey_with_flags(path, KEY_READ)
                    .map(|key| key.enum_keys().flatten().count());
                say(
                    "c5",
                    format!(
                        "scanned_key={hive_name}/{path_name} exists={} subkeys={}",
                        subkeys.is_ok(),
                        subkeys.unwrap_or(0)
                    ),
                );
            }
        }

        let before = scan_installed().expect("scan the registry");
        let mut sorted = before.apps.clone();
        sorted.sort();
        sorted.dedup();
        say(
            "c5",
            format!(
                "entries={} with_version={} with_publisher={} sorted_and_deduplicated={}",
                before.len(),
                before.apps.iter().filter(|a| a.version.is_some()).count(),
                before.apps.iter().filter(|a| a.publisher.is_some()).count(),
                sorted == before.apps,
            ),
        );

        let probe_name = format!("Halvo Verify Probe {}", std::process::id());
        let probe_path = format!(r"{UNINSTALL}\{PROBE_KEY}");
        let hkcu_key_existed = hkcu.open_subkey_with_flags(UNINSTALL, KEY_READ).is_ok();
        assert!(
            !before.apps.iter().any(|a| a.name == probe_name),
            "the probe entry must not exist yet"
        );

        let first_watch = arm_watcher();
        let spurious = first_watch.recv_timeout(Duration::from_secs(1)).is_ok();
        let (probe, _) = hkcu
            .create_subkey(&probe_path)
            .expect("create the probe key");
        probe
            .set_value("DisplayName", &format!("  {probe_name}  "))
            .expect("probe name");
        probe
            .set_value("DisplayVersion", &"9.9.9")
            .expect("probe version");
        probe
            .set_value("Publisher", &"halvo-verifier")
            .expect("probe publisher");
        let first_woke = first_watch.recv_timeout(Duration::from_secs(5));
        say(
            "c5",
            format!(
                "watch_armed_while_hkcu_uninstall_key_existed={hkcu_key_existed} \
                 woke_before_any_change={spurious} woke_within_5s_of_hkcu_install={}",
                first_woke.is_ok()
            ),
        );

        let with_probe = scan_installed().expect("scan with the probe");
        let found = with_probe.apps.iter().find(|a| a.name == probe_name);
        say(
            "c5",
            format!(
                "probe_entry_found={} probe_version_ok={} probe_publisher_ok={} entries={}",
                found.is_some(),
                found.is_some_and(|a| a.version.as_deref() == Some("9.9.9")),
                found.is_some_and(|a| a.publisher.as_deref() == Some("halvo-verifier")),
                with_probe.len(),
            ),
        );

        let second_watch = arm_watcher();
        probe
            .set_value("DisplayVersion", &"9.9.10")
            .expect("probe version change");
        let second_woke = second_watch.recv_timeout(Duration::from_secs(5));
        say(
            "c5",
            format!(
                "watch_armed_with_hkcu_uninstall_key_present woke_within_5s_of_change={}",
                second_woke.is_ok()
            ),
        );

        probe
            .set_value("SystemComponent", &1u32)
            .expect("probe system component");
        let hidden = scan_installed().expect("scan with the hidden probe");
        let system_component_hidden = !hidden.apps.iter().any(|a| a.name == probe_name);

        drop(probe);
        hkcu.delete_subkey_all(&probe_path)
            .expect("delete the probe key");
        if !hkcu_key_existed {
            let _ = hkcu.delete_subkey(UNINSTALL);
        }
        let after = scan_installed().expect("scan after cleanup");
        say(
            "c5",
            format!(
                "system_component_probe_hidden={system_component_hidden} \
                 probe_gone_after_delete={} entries_after={} same_as_before={}",
                !after.apps.iter().any(|a| a.name == probe_name),
                after.len(),
                after == before,
            ),
        );

        assert!(!before.is_empty(), "the registry scan returned no entries");
        assert!(
            found.is_some(),
            "the scan did not return the entry the test wrote"
        );
        assert!(after == before, "the scan must return to its first result");
    });
}

fn list_files(dir: &Path) -> Vec<(String, u64)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                out.push((relative.to_string_lossy().replace('\\', "/"), len));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn dir_holds(dir: &Path, needle: &[u8]) -> bool {
    list_files(dir).iter().any(|(relative, _)| {
        std::fs::read(dir.join(relative))
            .map(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
            .unwrap_or(false)
    })
}

/// File names and sizes; a segment file's name is its random id.
fn files_line(dir: &Path) -> String {
    list_files(dir)
        .iter()
        .map(|(name, len)| format!("{name}:{len}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
#[ignore = "functional: uses DPAPI for the current Windows user"]
fn c6_dpapi_custody_and_store_lifecycle() {
    with_deadline("c6", Duration::from_secs(180), || {
        let local = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA");
        let roots_dir = PathBuf::from(local).join("Halvo").join("roots");
        let root_file = |id: RootId| roots_dir.join(format!("{id}.root"));

        let root_id = RootId::fresh();
        let custody = DpapiCustody::new(root_id);
        let key = StoreKey::random().expect("random store key");
        let wrapped = custody.wrap_store_key(&key);
        say(
            "c6",
            format!(
                "custody_kind_is_dpapi={} wrap={} wrapped_len={} root_file_exists={} root_file_len={}",
                custody.kind() == CustodyKind::Dpapi,
                show(&wrapped),
                wrapped.as_ref().map(|w| w.0.len()).unwrap_or(0),
                root_file(root_id).exists(),
                std::fs::metadata(root_file(root_id)).map(|m| m.len()).unwrap_or(0),
            ),
        );
        let wrapped = wrapped.expect("wrap the store key");
        let unwrapped = custody.unwrap_store_key(&wrapped);
        let round_trip = unwrapped.as_ref().is_ok_and(|k| k.expose() == key.expose());
        let fresh_handle = DpapiCustody::new(root_id)
            .unwrap_store_key(&wrapped)
            .is_ok_and(|k| k.expose() == key.expose());
        let mut tampered = wrapped.0.clone();
        if let Some(last) = tampered.last_mut() {
            *last ^= 0x01;
        }
        let tampered_rejected = custody.unwrap_store_key(&WrappedKey(tampered)).is_err();
        let other_root_rejected = DpapiCustody::new(RootId::fresh())
            .unwrap_store_key(&wrapped)
            .is_err();
        let key_in_wrapped = wrapped.0.windows(32).any(|w| w == key.expose());
        let key_in_root_file = std::fs::read(root_file(root_id))
            .map(|bytes| bytes.windows(32).any(|w| w == key.expose()))
            .unwrap_or(false);
        say(
            "c6",
            format!(
                "unwrap={} round_trip_equal={round_trip} second_custody_handle_unwraps={fresh_handle} \
                 tampered_wrapped_key_rejected={tampered_rejected} other_root_rejected={other_root_rejected} \
                 store_key_bytes_in_wrapped_form={key_in_wrapped} store_key_bytes_in_root_file={key_in_root_file}",
                show(&unwrapped),
            ),
        );
        let destroy = custody.destroy_root();
        let unwrap_after_destroy = custody.unwrap_store_key(&wrapped);
        let destroy_again = custody.destroy_root();
        say(
            "c6",
            format!(
                "destroy_root={} root_file_exists_after={} unwrap_after_destroy={} destroy_root_again={}",
                show(&destroy),
                root_file(root_id).exists(),
                show(&unwrap_after_destroy),
                show(&destroy_again),
            ),
        );
        assert!(round_trip, "DPAPI custody must unwrap what it wrapped");
        assert!(tampered_rejected && other_root_rejected);
        assert!(destroy.is_ok() && unwrap_after_destroy.is_err());

        let dir = tempfile::tempdir().expect("temp dir");
        let store_dir = dir.path().join("store");
        let store_root = RootId::fresh();
        let marker = format!("HALVO-VERIFY-PLAINTEXT-MARKER-{}", std::process::id()).into_bytes();

        let created = EncryptedStore::create(
            &store_dir,
            Box::new(DpapiCustody::new(store_root)),
            DEFAULT_RETENTION,
        );
        say(
            "c6",
            format!("store_create={}", show(&created.as_ref().map(|_| ()))),
        );
        let mut store = created.expect("create the store");
        let (live, live_key) = store
            .write_segment(&mut Cursor::new(marker.clone()))
            .expect("write the live segment");
        let event = CaptureStreamEvent::new_raw(
            CapturedAt {
                wall_clock: SystemTime::now(),
                sequence: MonotonicSequence::new(1),
            },
            Modality::Keystroke,
            ATTRIBUTION.to_string(),
            live.clone(),
            live_key,
        );
        let append = store.append_event(&event);
        say(
            "c6",
            format!(
                "segment_written_len={} append_event={} size_bytes={} root_file_exists={} files={}",
                live.byte_len,
                show(&append),
                store.size_bytes().unwrap_or(0),
                root_file(store_root).exists(),
                files_line(&store_dir),
            ),
        );
        drop(store);

        let second_create = EncryptedStore::create(
            &store_dir,
            Box::new(DpapiCustody::new(RootId::fresh())),
            DEFAULT_RETENTION,
        );
        let refuses_second_create = matches!(second_create, Err(StoreError::AlreadyExists(_)));
        drop(second_create);

        let reopened = EncryptedStore::open(&store_dir, Duration::ZERO);
        say(
            "c6",
            format!(
                "second_create_refused={refuses_second_create} store_reopen={}",
                show(&reopened.as_ref().map(|_| ()))
            ),
        );
        let mut store = reopened.expect("reopen the store");
        let mut read_back = Vec::new();
        let read = store.read_segment(live.segment_id, &mut read_back);
        let (expiring, _) = store
            .write_segment(&mut Cursor::new(marker.clone()))
            .expect("write the expiring segment");
        say(
            "c6",
            format!(
                "segment_read_after_reopen={} plaintext_equal={} plaintext_marker_in_store_files={} files={}",
                show(&read),
                read_back == marker,
                dir_holds(&store_dir, &marker),
                files_line(&store_dir),
            ),
        );

        let shred = store.crypto_shred_expired();
        let expired_read = store.read_segment(expiring.segment_id, &mut Vec::new());
        let mut live_again = Vec::new();
        let live_read = store.read_segment(live.segment_id, &mut live_again);
        say(
            "c6",
            format!(
                "shred_expired={} segments_shredded={} expired_segment_read={} \
                 live_segment_read={} live_plaintext_equal={} files={}",
                show(&shred),
                shred.as_ref().map(|r| r.segments_shredded).unwrap_or(0),
                show(&expired_read),
                show(&live_read),
                live_again == marker,
                files_line(&store_dir),
            ),
        );

        let delete_all = store.crypto_shred_all();
        drop(store);
        let open_after = EncryptedStore::open(&store_dir, DEFAULT_RETENTION);
        let sealed_after = matches!(open_after, Err(StoreError::Sealed));
        say(
            "c6",
            format!(
                "delete_all={} keys_destroyed={} root_file_exists_after={} open_after_is_sealed={sealed_after} \
                 open_after={} files_left={}",
                show(&delete_all),
                delete_all.as_ref().map(|r| r.keys_destroyed).unwrap_or(0),
                root_file(store_root).exists(),
                show(&open_after.as_ref().map(|_| ())),
                files_line(&store_dir),
            ),
        );
        drop(open_after);

        let recreate_root = RootId::fresh();
        let recreated = EncryptedStore::create(
            &store_dir,
            Box::new(DpapiCustody::new(recreate_root)),
            DEFAULT_RETENTION,
        );
        say(
            "c6",
            format!(
                "create_in_same_directory_after_delete_all={}",
                show(&recreated.as_ref().map(|_| ()))
            ),
        );
        if let Ok(mut store) = recreated {
            let cleanup = store.crypto_shred_all();
            say(
                "c6",
                format!(
                    "cleanup_delete_all={} root_file_exists_after={}",
                    show(&cleanup),
                    root_file(recreate_root).exists()
                ),
            );
        }

        assert!(append.is_ok(), "the store must append an event");
        assert!(
            read.is_ok() && read_back == marker,
            "the reopened store must read its segment"
        );
        assert!(
            refuses_second_create,
            "a second create over an existing store must be refused"
        );
        assert!(
            shred.as_ref().is_ok_and(|r| r.segments_shredded == 1),
            "exactly the expired segment must be shredded"
        );
        assert!(expired_read.is_err() && live_read.is_ok() && live_again == marker);
        assert!(delete_all.is_ok(), "delete-all must succeed");
        assert!(sealed_after, "the store must be sealed after delete-all");
        assert!(
            !root_file(store_root).exists(),
            "delete-all must remove the root file"
        );
    });
}

struct TaskXml {
    query_ok: bool,
    has_nul_bytes: bool,
    text: String,
}

fn query_task_xml(task_name: &str) -> TaskXml {
    let out = Command::new("schtasks.exe")
        .args(["/Query", "/TN", task_name, "/XML", "ONE"])
        .output()
        .expect("run schtasks");
    TaskXml {
        query_ok: out.status.success(),
        has_nul_bytes: out.stdout.contains(&0),
        text: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

/// The lines of a task definition that say what runs, when and as whom. The
/// account's SID and name are replaced, so the log holds no account identity.
fn task_lines(xml: &str) -> Vec<String> {
    const KEEP: [&str; 9] = [
        "<Command>",
        "<Arguments>",
        "<LogonTrigger>",
        "<UserId>",
        "<LogonType>",
        "<RunLevel>",
        "<Date>",
        "<Enabled>",
        "<ExecutionTimeLimit>",
    ];
    xml.lines()
        .map(str::trim)
        .filter(|line| KEEP.iter().any(|tag| line.contains(tag)))
        .map(|line| {
            if line.contains("<UserId>") {
                "<UserId>(account, masked)</UserId>".to_string()
            } else {
                line.to_string()
            }
        })
        .collect()
}

#[test]
#[ignore = "functional: creates and removes a Windows scheduled task"]
fn c7_logon_task_installs_is_idempotent_and_is_removed() {
    with_deadline("c7", Duration::from_secs(180), || {
        let program = r"C:\Program Files\Halvo\halvo-capture.exe";
        let spec = TaskSpec::for_capture_daemon(program);
        say(
            "c7",
            format!(
                "task_name={DEFAULT_TASK_NAME:?} task_run={:?}",
                spec.task_run()
            ),
        );
        let installer = WindowsTaskInstaller::new(spec);
        let status_before = installer.status().expect("status");
        assert_eq!(
            status_before,
            ServiceStatus::NotInstalled,
            "the task must not exist before the test; a task that does is not the test's to touch"
        );

        let first_install = installer.install();
        let status_after_first = installer.status().expect("status");
        let first = query_task_xml(DEFAULT_TASK_NAME);
        say(
            "c7",
            format!(
                "first_install={} status_after={status_after_first:?} xml_query_ok={} \
                 xml_has_nul_bytes={} task_registers_program={}",
                show(&first_install),
                first.query_ok,
                first.has_nul_bytes,
                task_registers(&first.text, program),
            ),
        );
        for line in task_lines(&first.text) {
            say("c7", format!("task_definition {line}"));
        }

        std::thread::sleep(Duration::from_millis(2100));
        let second_install = installer.install();
        let status_after_second = installer.status().expect("status");
        let second = query_task_xml(DEFAULT_TASK_NAME);
        say(
            "c7",
            format!(
                "second_install={} status_after={status_after_second:?} \
                 definition_unchanged_by_second_install={}",
                show(&second_install),
                first.text == second.text,
            ),
        );

        let first_uninstall = installer.uninstall();
        let status_after_uninstall = installer.status().expect("status");
        let second_uninstall = installer.uninstall();
        say(
            "c7",
            format!(
                "uninstall={} status_after={status_after_uninstall:?} uninstall_again={} \
                 task_folder_left_behind={}",
                show(&first_uninstall),
                show(&second_uninstall),
                Path::new(r"C:\Windows\System32\Tasks\Halvo").exists(),
            ),
        );

        assert!(first_install.is_ok(), "the first install must succeed");
        assert_eq!(status_after_first, ServiceStatus::Installed);
        assert!(second_install.is_ok(), "the second install must succeed");
        assert_eq!(status_after_second, ServiceStatus::Installed);
        assert!(first_uninstall.is_ok(), "the uninstall must succeed");
        assert_eq!(status_after_uninstall, ServiceStatus::NotInstalled);
        assert!(second_uninstall.is_ok(), "a second uninstall must succeed");
    });
}

fn display_affinity(hwnd: HWND) -> Result<u32, String> {
    let mut affinity = 0u32;
    unsafe { GetWindowDisplayAffinity(hwnd, &mut affinity) }
        .map(|()| affinity)
        .map_err(|e| e.to_string())
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn c8_a_window_can_be_excluded_from_capture() {
    with_deadline("c8", Duration::from_secs(60), || {
        env_report("c8");
        let window =
            TestWindow::create(&magenta_popup("HalvoVerifyExclude", "halvo-verify-exclude"))
                .expect("window");
        pump_thread(Duration::from_millis(300));
        let before = display_affinity(window.hwnd);
        let exclude = exclude_window_from_capture(window.hwnd);
        let after = display_affinity(window.hwnd);
        let null_window = exclude_window_from_capture(HWND(std::ptr::null_mut()));
        say(
            "c8",
            format!(
                "affinity_before={before:?} exclude_window_from_capture={} affinity_after={after:?} \
                 wda_excludefromcapture={} exclude_null_window={}",
                show(&exclude),
                WDA_EXCLUDEFROMCAPTURE.0,
                show(&null_window),
            ),
        );
        assert!(exclude.is_ok(), "the display-affinity call must succeed");
        assert_eq!(after, Ok(WDA_EXCLUDEFROMCAPTURE.0));
        assert!(null_window.is_err(), "a null window must be an error");
    });
}

struct PixelProbeFlags {
    samples: mpsc::Sender<Result<FrameSample, String>>,
    x: u32,
    y: u32,
}

struct FrameSample {
    width: u32,
    height: u32,
    bgra: [u8; 4],
}

/// A capture handler for the test only: it reads one pixel of each frame. The
/// product's handler reads no pixels, so the product cannot show what a frame
/// contains.
struct PixelProbe {
    flags: PixelProbeFlags,
}

impl GraphicsCaptureApiHandler for PixelProbe {
    type Flags = PixelProbeFlags;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { flags: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let (width, height) = (frame.width(), frame.height());
        let sample = frame
            .buffer()
            .map_err(|e| format!("frame buffer: {e}"))
            .and_then(|mut buffer| {
                let pitch = buffer.row_pitch() as usize;
                let offset = self.flags.y as usize * pitch + self.flags.x as usize * 4;
                buffer
                    .as_raw_buffer()
                    .get(offset..offset + 4)
                    .map(|px| FrameSample {
                        width,
                        height,
                        bgra: [px[0], px[1], px[2], px[3]],
                    })
                    .ok_or_else(|| "the probe pixel is outside the frame".to_string())
            });
        let _ = self.flags.samples.send(sample);
        Ok(())
    }
}

/// Capture the primary monitor and return the last sample of the pixel at
/// (`x`, `y`) seen within three seconds of the first frame.
fn capture_pixel(x: u32, y: u32) -> Result<FrameSample, String> {
    let (samples, received) = mpsc::channel();
    let monitor = Monitor::primary().map_err(|e| format!("primary monitor: {e}"))?;
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        PixelProbeFlags { samples, x, y },
    );
    let control =
        PixelProbe::start_free_threaded(settings).map_err(|e| format!("capture start: {e}"))?;
    let mut last = None;
    let mut deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(sample) = received.try_recv() {
            if last.is_none() {
                deadline = Instant::now() + Duration::from_secs(3);
            }
            last = Some(sample);
        }
        pump_thread(Duration::from_millis(20));
    }
    let _ = control.stop();
    last.unwrap_or_else(|| Err("no frame within 10 seconds".to_string()))
}

fn is_magenta(bgra: [u8; 4]) -> bool {
    bgra[0] > 200 && bgra[1] < 60 && bgra[2] > 200
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn c8b_an_excluded_window_is_absent_from_a_captured_frame() {
    with_deadline("c8b", Duration::from_secs(120), || {
        env_report("c8b");
        let window = TestWindow::create(&magenta_popup(
            "HalvoVerifyExcludePixels",
            "halvo-verify-exclude-pixels",
        ))
        .expect("window");
        pump_thread(Duration::from_millis(500));
        let (x, y) = (
            100 + WINDOW_WIDTH as u32 / 2,
            100 + WINDOW_HEIGHT as u32 / 2,
        );

        let shown = capture_pixel(x, y);
        let exclude = exclude_window_from_capture(window.hwnd);
        pump_thread(Duration::from_millis(500));
        let excluded = capture_pixel(x, y);
        for (phase, sample) in [("window-shown", &shown), ("window-excluded", &excluded)] {
            match sample {
                Ok(s) => say(
                    "c8b",
                    format!(
                        "phase={phase} frame={}x{} probe_pixel=({x},{y}) is_window_color={}",
                        s.width,
                        s.height,
                        is_magenta(s.bgra)
                    ),
                ),
                Err(e) => say("c8b", format!("phase={phase} capture_error={e}")),
            }
        }
        say(
            "c8b",
            format!("exclude_window_from_capture={}", show(&exclude)),
        );
        let shown = shown.expect("INCONCLUSIVE: no frame with the window shown");
        let excluded = excluded.expect("INCONCLUSIVE: no frame with the window excluded");
        assert!(
            is_magenta(shown.bgra),
            "INCONCLUSIVE: the window was not in the frame before it was excluded"
        );
        assert!(exclude.is_ok(), "the display-affinity call must succeed");
        assert!(
            !is_magenta(excluded.bgra),
            "the excluded window is still in the captured frame"
        );
    });
}

fn memory() -> PROCESS_MEMORY_COUNTERS {
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            core::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    }
    .expect("GetProcessMemoryInfo");
    counters
}

fn memory_line(phase: &str, counters: &PROCESS_MEMORY_COUNTERS) -> String {
    format!(
        "memory phase={phase} working_set_bytes={} peak_working_set_bytes={} \
         private_bytes={} peak_private_bytes={}",
        counters.WorkingSetSize,
        counters.PeakWorkingSetSize,
        counters.PagefileUsage,
        counters.PeakPagefileUsage,
    )
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop; runs for 60 seconds"]
fn c9_peak_working_set_of_sources_1_to_4_for_60_seconds() {
    with_deadline("c9", Duration::from_secs(240), || {
        const RUN: Duration = Duration::from_secs(60);
        env_report("c9");
        say(
            "c9",
            format!(
                "build_profile={}",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
            ),
        );
        say("c9", memory_line("before-sources", &memory()));

        let (sink, writer) = channel(8192);
        let mut foreground = ForegroundAppSource::new();
        let last_app = foreground.last_app();
        let mut title = WindowTitleSource::new();
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app));
        let mut pointer = RawInputSource::pointer(Arc::clone(&last_app));
        let mut screen = ScreenSource::new(Arc::clone(&last_app));
        let starts = [
            ("foreground", foreground.start(sink.clone())),
            ("window_title", title.start(sink.clone())),
            ("keyboard", keyboard.start(sink.clone())),
            ("pointer", pointer.start(sink.clone())),
            ("screen", screen.start(sink.clone())),
        ];
        for (name, result) in &starts {
            say("c9", format!("source={name} start={}", show(result)));
        }
        let all_started = starts.iter().all(|(_, result)| result.is_ok());
        say("c9", memory_line("sources-started", &memory()));

        let mut child = spawn_child(RUN.as_secs());
        let began = Instant::now();
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        let mut next_sample_at = 5u64;
        let mut samples = 0u32;
        let mut highest_working_set = 0usize;
        let (mut keys_accepted, mut moves_accepted) = (0u32, 0u32);
        while began.elapsed() < RUN {
            keys_accepted += press_keys(1);
            moves_accepted += move_pointer(2);
            for event in drain_for(&writer, Duration::from_millis(80)) {
                *counts.entry(format!("{:?}", event.modality)).or_default() += 1;
            }
            let now = memory();
            samples += 1;
            highest_working_set = highest_working_set.max(now.WorkingSetSize);
            if began.elapsed().as_secs() >= next_sample_at {
                say("c9", memory_line(&format!("t+{next_sample_at}s"), &now));
                next_sample_at += 5;
            }
        }
        let end = memory();
        say("c9", memory_line("after-60s-before-stop", &end));

        foreground.stop();
        title.stop();
        keyboard.stop();
        pointer.stop();
        screen.stop();
        let child_exit = child.process.wait().expect("wait for the child");
        for event in drain_for(&writer, Duration::from_millis(500)) {
            *counts.entry(format!("{:?}", event.modality)).or_default() += 1;
        }
        say("c9", memory_line("sources-stopped", &memory()));
        say(
            "c9",
            format!(
                "events_by_modality={counts:?} frames_dropped_under_backpressure={} \
                 sendinput_key_events_accepted={keys_accepted} sendinput_move_events_accepted={moves_accepted} \
                 child_exit_ok={}",
                sink.dropped_count(),
                child_exit.success(),
            ),
        );
        say(
            "c9",
            format!(
                "result all_sources_started={all_started} run_secs={} working_set_samples={samples} \
                 highest_sampled_working_set_bytes={highest_working_set} \
                 peak_working_set_bytes={} peak_working_set_mib={:.2} \
                 peak_private_bytes={} budget_bytes={BUDGET_48_MIB} peak_within_budget={}",
                RUN.as_secs(),
                end.PeakWorkingSetSize,
                end.PeakWorkingSetSize as f64 / (1024.0 * 1024.0),
                end.PeakPagefileUsage,
                end.PeakWorkingSetSize <= BUDGET_48_MIB,
            ),
        );
        assert!(
            all_started,
            "INCONCLUSIVE: not every source started, so the number is not for sources 1 to 4"
        );
    });
}

/// Not one of the nine claims: what the keyboard source does while the focus
/// is in a password field. Windows has no secure-input state to ask for; this
/// records what Raw Input delivers in that case.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x1_keystrokes_typed_into_a_password_field() {
    with_deadline("x1", Duration::from_secs(90), || {
        const LETTERS: u32 = 5;
        env_report("x1");
        let (sink, writer) = channel(4096);
        let last_app = Arc::new(Mutex::new(Some(ATTRIBUTION.to_string())));
        let mut keyboard = RawInputSource::keyboard(last_app);
        let start = keyboard.start(sink.clone());

        let window = TestWindow::create(&normal_window(
            "HalvoVerifyPassword",
            "halvo-verify-password",
            80,
            80,
        ))
        .expect("window");
        let edit_class = HSTRING::from("EDIT");
        let edit = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(edit_class.as_ptr()),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_PASSWORD as u32),
                10,
                10,
                220,
                24,
                Some(window.hwnd),
                None,
                Some(window.hinstance),
                None,
            )
        }
        .expect("password edit control");
        pump_thread(Duration::from_millis(300));
        let (foreground, method) = window.activate();
        unsafe {
            let _ = SetFocus(Some(edit));
        }
        pump_thread(Duration::from_millis(200));
        let focused = unsafe { GetFocus() == edit };
        let before = drain_for(&writer, Duration::from_millis(500));

        let mut accepted = 0;
        for _ in 0..LETTERS {
            accepted += send(&[key_input(VK_A, false)]);
            pump_thread(Duration::from_millis(20));
            accepted += send(&[key_input(VK_A, true)]);
            pump_thread(Duration::from_millis(20));
        }
        pump_thread(Duration::from_millis(300));
        let characters_in_field = unsafe { GetWindowTextLengthW(edit) };
        let during = drain_for(&writer, Duration::from_secs(1));
        keyboard.stop();
        say(
            "x1",
            format!(
                "keyboard_start={} window_foreground={foreground} method={method} \
                 password_field_has_focus={focused} letters_sent={LETTERS} \
                 sendinput_key_events_accepted={accepted} characters_in_password_field={characters_in_field} \
                 keystroke_events_before_typing={} keystroke_events_while_typing_into_password_field={}",
                show(&start),
                count(&before, Modality::Keystroke),
                count(&during, Modality::Keystroke),
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert!(
            focused && characters_in_field == LETTERS as i32,
            "INCONCLUSIVE: the scripted keys did not reach the password field"
        );
    });
}
