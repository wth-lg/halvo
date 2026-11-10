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
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, GetDC, GetPixel, ReleaseDC};
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
    EnumWindows, FindWindowW, GetClassNameW, GetForegroundWindow, GetSystemMetrics,
    GetWindowDisplayAffinity, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    PeekMessageW, RegisterClassW, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, UnregisterClassW, ES_PASSWORD, MSG, PM_REMOVE, SC_KEYMENU, SM_CMONITORS,
    SM_CXSCREEN, SM_CYSCREEN, SM_REMOTESESSION, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    SW_MINIMIZE, SW_RESTORE, SW_SHOW, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
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
const CHILD_MODE_ENV: &str = "HALVO_VERIFY_CHILD_MODE";
const LIMITED_TASK_ENV: &str = "HALVO_VERIFY_LIMITED_TASK";
const LIMITED_OUT_ENV: &str = "HALVO_VERIFY_LIMITED_OUT";
const LISTENER_OUT_ENV: &str = "HALVO_VERIFY_LISTENER_OUT";
const LISTENER_WINDOW_CLASS: &str = "HalvoVerifyListenerWindow";
const PROGRAM: &str = r"C:\Program Files\Halvo\halvo-capture.exe";
/// COLORREF is 0x00BBGGRR: full red and blue, no green.
const MAGENTA: u32 = 0x00FF_00FF;
const GREEN: u32 = 0x0000_FF00;
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

    fn cover_screen(&self) {
        let (cx, cy) = screen_size();
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, 0, 0, cx, cy, SWP_NOZORDER);
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
    match std::env::var(CHILD_MODE_ENV).as_deref() {
        Ok("scope") => return child_scope(),
        Ok("hold") => return child_hold(secs),
        _ => {}
    }
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

/// Child mode "scope": retitle the foreground window (30 bytes), a child
/// control inside it (40), a visible window that is not the foreground window
/// (50) and a window that was never shown (60). The sizes tell the parent
/// which of the four the window-title source reported.
fn child_scope() {
    let first = TestWindow::create(&normal_window(
        "HalvoVerifyScopeFirst",
        "halvo-verify-scope-first",
        40,
        40,
    ))
    .expect("foreground window");
    let label_class = HSTRING::from("STATIC");
    let label = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(label_class.as_ptr()),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE,
            10,
            10,
            300,
            24,
            Some(first.hwnd),
            None,
            Some(first.hinstance),
            None,
        )
    }
    .expect("child control");
    let background = TestWindow::create(&normal_window(
        "HalvoVerifyScopeBackground",
        "halvo-verify-scope-background",
        260,
        200,
    ))
    .expect("background window");
    let hidden = TestWindow::create(&WindowSpec {
        class: "HalvoVerifyScopeHidden",
        title: "halvo-verify-scope-hidden",
        style: WS_OVERLAPPEDWINDOW,
        ex_style: WINDOW_EX_STYLE(0),
        fill: 0x0000_80FF,
        x: 300,
        y: 300,
    })
    .expect("hidden window");
    pump_thread(Duration::from_millis(300));
    let (foreground, method) = first.activate();
    pump_thread(Duration::from_millis(500));

    first.set_title(&"f".repeat(30)).expect("foreground title");
    pump_thread(Duration::from_millis(400));
    let label_text = HSTRING::from("c".repeat(40));
    let label_set = unsafe { SetWindowTextW(label, &label_text) }.is_ok();
    pump_thread(Duration::from_millis(400));
    let first_still_foreground = first.is_foreground();
    background
        .set_title(&"b".repeat(50))
        .expect("background title");
    pump_thread(Duration::from_millis(400));
    hidden.set_title(&"h".repeat(60)).expect("hidden title");
    pump_thread(Duration::from_millis(400));
    say(
        "child",
        format!(
            "mode=scope first_window_foreground={foreground} method={method} \
             first_window_foreground_during_changes={first_still_foreground} \
             control_text_set={label_set} background_window_is_foreground={}",
            background.is_foreground()
        ),
    );
}

/// Child mode "hold": keep one window in the foreground for `secs`, then
/// switch to a second window of the same process and stay three seconds.
fn child_hold(secs: u64) {
    let first = TestWindow::create(&normal_window(
        "HalvoVerifyHoldFirst",
        "halvo-verify-hold-first",
        40,
        40,
    ))
    .expect("first window");
    let second = TestWindow::create(&normal_window(
        "HalvoVerifyHoldSecond",
        "halvo-verify-hold-second",
        220,
        180,
    ))
    .expect("second window");
    pump_thread(Duration::from_millis(300));
    let (foreground, method) = first.activate();
    say(
        "child",
        format!("mode=hold first_window_foreground={foreground} method={method}"),
    );
    pump_thread(Duration::from_secs(secs));
    let held = first.is_foreground();
    unsafe {
        let _ = SetForegroundWindow(second.hwnd);
    }
    pump_thread(Duration::from_millis(100));
    say(
        "child",
        format!(
            "mode=hold first_window_held_foreground={held} switched_to_second_window={}",
            second.is_foreground()
        ),
    );
    pump_thread(Duration::from_secs(3));
}

fn spawn_child(mode: &str, secs: u64) -> ChildProcess {
    let dir = tempfile::tempdir().expect("temp dir for the child executable");
    let exe = dir.path().join(CHILD_EXE_NAME);
    std::fs::copy(std::env::current_exe().expect("current exe"), &exe)
        .expect("copy the test binary");
    let process = Command::new(&exe)
        .args(["helper_child_window", "--exact", "--ignored", "--nocapture"])
        .env(CHILD_SECS_ENV, secs.to_string())
        .env(CHILD_MODE_ENV, mode)
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

    let mut child = spawn_child("cycle", 4);
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
        assert!(quiet.is_empty(), "events arrived before any input");
        assert_eq!(
            keystrokes, PRESSES as usize,
            "one keystroke event per press"
        );
        assert_eq!(pointers, MOVES as usize, "one pointer event per move");
        assert!(after_stop.is_empty(), "events arrived after stop");
        assert!(
            attributed,
            "every event must carry the app in the shared cell"
        );
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
        let distinct_sizes: std::collections::BTreeSet<u64> = child_titles
            .iter()
            .copied()
            .filter(|len| CHILD_TITLE_LENGTHS.contains(len))
            .collect();
        say(
            "c4",
            format!("distinct_scripted_title_sizes={}", distinct_sizes.len()),
        );
        assert!(
            distinct_sizes.len() >= 2,
            "the events must follow more than one title change"
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

        let mut machine_keys_covered = Vec::new();
        for (path_name, path) in [("native", UNINSTALL), ("wow6432node", UNINSTALL_WOW)] {
            let first_listable = hklm
                .open_subkey_with_flags(path, KEY_READ)
                .ok()
                .and_then(|key| {
                    key.enum_keys().flatten().find_map(|name| {
                        let sub = key.open_subkey_with_flags(&name, KEY_READ).ok()?;
                        let display: String = sub.get_value("DisplayName").ok()?;
                        let system = sub.get_value::<u32, _>("SystemComponent").ok() == Some(1);
                        let display = display.trim().to_string();
                        (!display.is_empty() && !system).then_some(display)
                    })
                });
            let in_scan = first_listable
                .as_ref()
                .map(|name| before.apps.iter().any(|app| &app.name == name));
            say(
                "c5",
                format!("hklm_{path_name}_first_listable_app_is_in_scan={in_scan:?}"),
            );
            machine_keys_covered.push(in_scan);
        }

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
            machine_keys_covered
                .iter()
                .all(|covered| *covered != Some(false)),
            "the scan missed an app that a machine-wide Uninstall key lists"
        );
        assert!(
            found.is_some_and(|app| app.version.as_deref() == Some("9.9.9")
                && app.publisher.as_deref() == Some("halvo-verifier")),
            "the scan must return the probe's version and publisher"
        );
        assert!(system_component_hidden, "a system component must be hidden");
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

/// File kinds and sizes. A segment file's name is its random id, so only the
/// part after the last dot or slash goes to the log.
fn files_line(dir: &Path) -> String {
    list_files(dir)
        .iter()
        .map(|(name, len)| {
            let kind = name.rsplit(['/', '.']).next().unwrap_or(name);
            format!("{kind}:{len}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Unprotect `blob` with DPAPI for the current user and return only the
/// length of the plaintext, which is zeroed before it is freed.
fn dpapi_plain_len(blob: &[u8], entropy: &[u8]) -> Result<usize, String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let salt = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input as *const _,
            None,
            Some(&salt as *const _),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out as *mut _,
        )
    }
    .map_err(|e| e.to_string())?;
    let len = out.cbData as usize;
    unsafe {
        std::ptr::write_bytes(out.pbData, 0, len);
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
    }
    Ok(len)
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
        // The other root has a file of its own, so the failure is the seal and
        // not a missing file.
        let other = DpapiCustody::new(RootId::fresh());
        let other_wrap = other.wrap_store_key(&StoreKey::random().expect("random store key"));
        let other_root_rejected = other_wrap.is_ok() && other.unwrap_store_key(&wrapped).is_err();
        let _ = other.destroy_root();

        let root_bytes = std::fs::read(root_file(root_id)).expect("read the root file");
        let dpapi_len = dpapi_plain_len(&root_bytes, root_id.as_bytes());
        let dpapi_other_entropy = dpapi_plain_len(&root_bytes, &[0u8; 16]);
        let mut damaged = root_bytes.clone();
        let middle = damaged.len() / 2;
        damaged[middle] ^= 0x01;
        std::fs::write(root_file(root_id), &damaged).expect("damage the root file");
        let damaged_root_rejected = custody.unwrap_store_key(&wrapped).is_err();
        std::fs::write(root_file(root_id), &root_bytes).expect("restore the root file");
        let restored_root_unwraps = custody
            .unwrap_store_key(&wrapped)
            .is_ok_and(|k| k.expose() == key.expose());
        say(
            "c6",
            format!(
                "root_file_len={} dpapi_unprotect_of_root_file_for_this_user={} plain_len={} \
                 dpapi_unprotect_with_other_entropy={} damaged_root_file_rejected={damaged_root_rejected} \
                 restored_root_file_unwraps={restored_root_unwraps}",
                root_bytes.len(),
                show(&dpapi_len),
                dpapi_len.as_ref().copied().unwrap_or(0),
                show(&dpapi_other_entropy),
            ),
        );
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
                "destroy_root={} root_file_exists_after={} unwrap_after_destroy_is_error={} destroy_root_again={}",
                show(&destroy),
                root_file(root_id).exists(),
                unwrap_after_destroy.is_err(),
                show(&destroy_again),
            ),
        );
        assert!(round_trip, "DPAPI custody must unwrap what it wrapped");
        assert!(tampered_rejected && other_root_rejected);
        assert_eq!(
            dpapi_len,
            Ok(32),
            "the root file must be a DPAPI blob of a 32-byte root for this user"
        );
        assert!(
            dpapi_other_entropy.is_err(),
            "the root file must not open with another entropy"
        );
        assert!(damaged_root_rejected && restored_root_unwraps);
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

        let segment_files_after_shred = list_files(&store_dir)
            .iter()
            .filter(|(name, _)| name.ends_with(".seg"))
            .count();
        let marker_on_disk_after_shred = dir_holds(&store_dir, &marker);
        say(
            "c6",
            format!(
                "segment_files_after_shred={segment_files_after_shred} \
                 plaintext_marker_in_store_files_after_shred={marker_on_disk_after_shred}"
            ),
        );

        let delete_all = store.crypto_shred_all();
        drop(store);
        let left_after_delete_all = list_files(&store_dir);
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
        assert_eq!(
            segment_files_after_shred, 1,
            "the shredded segment's file must be gone and the live one kept"
        );
        assert!(
            !marker_on_disk_after_shred,
            "the plaintext marker must not be in any store file"
        );
        assert!(
            left_after_delete_all
                .iter()
                .all(|(name, _)| name == "writer.lock"),
            "delete-all must leave no database, key file or segment"
        );
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

/// Every line of a task definition, with the account's name and SID replaced,
/// so the log holds no account identity.
fn task_lines(xml: &str) -> Vec<String> {
    xml.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            if line.contains("<UserId>") {
                "<UserId>(account, masked)</UserId>".to_string()
            } else if line.contains("<Author>") {
                "<Author>(account, masked)</Author>".to_string()
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
        let effective = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                "$s = (Get-ScheduledTask -TaskPath '\\Halvo\\' -TaskName 'HalvoCapture').Settings; \
                 foreach ($n in 'ExecutionTimeLimit','DisallowStartIfOnBatteries','StopIfGoingOnBatteries',\
                 'MultipleInstances','RestartCount','StartWhenAvailable','AllowHardTerminate','Hidden','Priority') \
                 { $n + '=' + $s.$n }",
            ])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default();
        for line in effective.lines().map(str::trim).filter(|l| !l.is_empty()) {
            say("c7", format!("effective_setting {line}"));
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
        assert!(
            task_registers(&first.text, program),
            "the installer must recognize its own task"
        );
        assert!(
            first.text.contains("<LogonTrigger>"),
            "the task must have a logon trigger"
        );
        assert!(
            first.query_ok && second.query_ok && first.text == second.text,
            "the second install must not rewrite the task"
        );
        assert!(
            !query_task_xml(DEFAULT_TASK_NAME).query_ok,
            "the task must be gone after the uninstall"
        );
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
    reference_x: u32,
    reference_y: u32,
}

struct FrameSample {
    width: u32,
    height: u32,
    bgra: [u8; 4],
    reference: [u8; 4],
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
                let raw: &[u8] = buffer.as_raw_buffer();
                let pixel = |x: u32, y: u32| {
                    let offset = y as usize * pitch + x as usize * 4;
                    raw.get(offset..offset + 4)
                        .map(|px| [px[0], px[1], px[2], px[3]])
                };
                pixel(self.flags.x, self.flags.y)
                    .zip(pixel(self.flags.reference_x, self.flags.reference_y))
                    .map(|(bgra, reference)| FrameSample {
                        width,
                        height,
                        bgra,
                        reference,
                    })
                    .ok_or_else(|| "a probe pixel is outside the frame".to_string())
            });
        let _ = self.flags.samples.send(sample);
        Ok(())
    }
}

/// Capture the primary monitor and return the last sample of the pixel at
/// (`x`, `y`) seen within three seconds of the first frame.
fn capture_pixel(x: u32, y: u32) -> Result<FrameSample, String> {
    capture_pixels(x, y, x, y)
}

/// As [`capture_pixel`], with a second pixel read from the same frame.
fn capture_pixels(
    x: u32,
    y: u32,
    reference_x: u32,
    reference_y: u32,
) -> Result<FrameSample, String> {
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
        PixelProbeFlags {
            samples,
            x,
            y,
            reference_x,
            reference_y,
        },
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
    say("probe", "capture stop requested");
    let stopped = control.stop();
    say(
        "probe",
        format!("capture stopped stop_ok={}", stopped.is_ok()),
    );
    last.unwrap_or_else(|| Err("no frame within 10 seconds".to_string()))
}

fn is_green(bgra: [u8; 4]) -> bool {
    bgra[0] < 60 && bgra[1] > 200 && bgra[2] < 60
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

        // A second window that is never excluded. Its color must be in both
        // frames, or the second frame says nothing about the first window.
        let reference = TestWindow::create(&WindowSpec {
            class: "HalvoVerifyReference",
            title: "halvo-verify-reference",
            style: WS_POPUP | WS_VISIBLE,
            ex_style: WS_EX_TOPMOST,
            fill: GREEN,
            x: 520,
            y: 100,
        })
        .expect("reference window");
        pump_thread(Duration::from_millis(500));
        let (reference_x, reference_y) = (
            520 + WINDOW_WIDTH as u32 / 2,
            100 + WINDOW_HEIGHT as u32 / 2,
        );

        let shown = capture_pixels(x, y, reference_x, reference_y);
        let exclude = exclude_window_from_capture(window.hwnd);
        pump_thread(Duration::from_millis(500));
        let excluded = capture_pixels(x, y, reference_x, reference_y);
        for (phase, sample) in [("window-shown", &shown), ("window-excluded", &excluded)] {
            match sample {
                Ok(s) => say(
                    "c8b",
                    format!(
                        "phase={phase} frame={}x{} probe_pixel=({x},{y}) is_window_color={} \
                         reference_pixel=({reference_x},{reference_y}) is_reference_color={}",
                        s.width,
                        s.height,
                        is_magenta(s.bgra),
                        is_green(s.reference)
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
            is_magenta(shown.bgra) && is_green(shown.reference),
            "INCONCLUSIVE: the two windows were not in the frame before the exclusion"
        );
        assert!(exclude.is_ok(), "the display-affinity call must succeed");
        assert!(
            is_green(excluded.reference)
                && (excluded.width, excluded.height) == (shown.width, shown.height),
            "INCONCLUSIVE: the second frame does not show the reference window"
        );
        assert!(
            !is_magenta(excluded.bgra),
            "the excluded window is still in the captured frame"
        );
        drop(reference);
        drop(window);
        say("c8b", "teardown window_destroyed=true");
        std::thread::sleep(Duration::from_secs(2));
        say("c8b", "teardown idle_2s_after=true");
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

        let mut child = spawn_child("cycle", RUN.as_secs());
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
        for kind in [
            "ForegroundApp",
            "Keystroke",
            "Pointer",
            "ScreenFrame",
            "WindowTitle",
        ] {
            assert!(
                counts.get(kind).copied().unwrap_or(0) > 0,
                "INCONCLUSIVE: no {kind} event in the run, so that source did no work"
            );
        }
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

/// Not one of the nine claims: which windows the window-title source reports.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x2_window_title_events_for_other_windows_and_controls() {
    with_deadline("x2", Duration::from_secs(90), || {
        env_report("x2");
        let (sink, writer) = channel(8192);
        let mut title = WindowTitleSource::new();
        let start = title.start(sink.clone());
        let mut child = spawn_child("scope", 0);
        let mut events = Vec::new();
        let exit = loop {
            events.extend(drain_for(&writer, Duration::from_millis(100)));
            if let Some(status) = child.process.try_wait().expect("poll the child") {
                break status;
            }
        };
        events.extend(drain_for(&writer, Duration::from_secs(1)));
        title.stop();
        summarize("x2", "scope", &events);
        let with_size = |len: u64| {
            events
                .iter()
                .filter(|e| {
                    e.modality == Modality::WindowTitle
                        && is_child_app(&e.foreground_app)
                        && e.segment_ref.byte_len == len
                })
                .count()
        };
        say(
            "x2",
            format!(
                "title_start={} child_exit_ok={} events_for_foreground_window_title={} \
                 events_for_child_control_text={} events_for_background_window_title={} \
                 events_for_never_shown_window_title={}",
                show(&start),
                exit.success(),
                with_size(30),
                with_size(40),
                with_size(50),
                with_size(60),
            ),
        );
        assert!(start.is_ok(), "the window-title source must start");
        assert!(exit.success(), "the child process must finish");
        assert!(
            with_size(30) >= 1,
            "INCONCLUSIVE: no event for the foreground window's own title, so the other counts say nothing"
        );
    });
}

/// Not one of the nine claims: which app the keyboard source attributes a
/// keystroke to when the foreground source started after the foreground
/// window was already in place.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x3_attribution_before_the_first_foreground_change() {
    with_deadline("x3", Duration::from_secs(120), || {
        env_report("x3");
        let mut child = spawn_child("hold", 9);
        let child_pid = child.process.id();
        let waited = Instant::now();
        while foreground_pid() != child_pid && waited.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        // Let the activation settle, so no late event of it reaches the hook.
        std::thread::sleep(Duration::from_secs(2));
        let child_foreground_at_start = foreground_pid() == child_pid;
        let reader_sees_child = current_foreground_app().is_some_and(|app| is_child_app(&app));

        let (sink, writer) = channel(4096);
        let mut foreground = ForegroundAppSource::new();
        let last_app = foreground.last_app();
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app));
        let foreground_start = foreground.start(sink.clone());
        let keyboard_start = keyboard.start(sink.clone());
        let settle = drain_for(&writer, Duration::from_secs(1));
        let cell_before_typing = last_app.lock().ok().and_then(|cell| cell.clone());
        press_keys(5);
        let before = drain_for(&writer, Duration::from_millis(700));
        let still_child_foreground = foreground_pid() == child_pid;
        let cell_after_typing = last_app.lock().ok().and_then(|cell| cell.clone());

        let mut between = Vec::new();
        let waited = Instant::now();
        while !between
            .iter()
            .any(|e: &CaptureStreamEvent| e.modality == Modality::ForegroundApp)
            && waited.elapsed() < Duration::from_secs(15)
        {
            between.extend(drain_for(&writer, Duration::from_millis(100)));
        }
        press_keys(5);
        let after = drain_for(&writer, Duration::from_millis(700));
        let exit = child.process.wait().expect("wait for the child");
        foreground.stop();
        keyboard.stop();

        let keys = |events: &[CaptureStreamEvent]| count(events, Modality::Keystroke);
        let empty_app = |events: &[CaptureStreamEvent]| {
            events
                .iter()
                .filter(|e| e.modality == Modality::Keystroke && e.foreground_app.is_empty())
                .count()
        };
        let child_app = |events: &[CaptureStreamEvent]| {
            events
                .iter()
                .filter(|e| e.modality == Modality::Keystroke && is_child_app(&e.foreground_app))
                .count()
        };
        say(
            "x3",
            format!(
                "foreground_start={} keyboard_start={} child_window_foreground_when_sources_started={child_foreground_at_start} \
                 current_foreground_app_reader_names_child={reader_sees_child} \
                 foreground_events_in_first_second={} last_app_cell_is_none_before_typing={} \
                 child_window_foreground_while_typing={still_child_foreground} \
                 foreground_events_while_typing={} last_app_cell_is_none_after_typing={}",
                show(&foreground_start),
                show(&keyboard_start),
                count(&settle, Modality::ForegroundApp),
                cell_before_typing.is_none(),
                count(&before, Modality::ForegroundApp),
                cell_after_typing.is_none(),
            ),
        );
        say(
            "x3",
            format!(
                "keystrokes_before_first_foreground_change={} with_empty_app={} attributed_to_child={} \
                 foreground_events_seen_after={} keystrokes_after_first_change={} with_empty_app_after={} \
                 attributed_to_child_after={} child_exit_ok={}",
                keys(&before),
                empty_app(&before),
                child_app(&before),
                count(&between, Modality::ForegroundApp),
                keys(&after),
                empty_app(&after),
                child_app(&after),
                exit.success(),
            ),
        );
        assert!(foreground_start.is_ok() && keyboard_start.is_ok());
        assert!(
            child_foreground_at_start && still_child_foreground,
            "INCONCLUSIVE: the child window was not the foreground window while the keys were typed"
        );
        assert!(
            keys(&before) >= 1,
            "INCONCLUSIVE: no keystroke event to attribute"
        );
    });
}

fn schtasks(args: &[&str]) -> String {
    match Command::new("schtasks.exe").args(args).output() {
        Ok(out) if out.status.success() => "ok".to_string(),
        Ok(out) => format!(
            "error({})",
            String::from_utf8_lossy(&out.stderr)
                .trim()
                .replace('\n', " ")
        ),
        Err(e) => format!("not-run({e})"),
    }
}

/// Runs under the restricted token x4 makes: installs the logon task through
/// the product installer and writes what happened to the file x4 names.
#[test]
#[ignore = "helper: does nothing unless x4 launches it under a restricted token"]
fn helper_limited_install() {
    let from_env = std::env::var(LIMITED_TASK_ENV)
        .ok()
        .zip(std::env::var(LIMITED_OUT_ENV).ok());
    // A process started as another user gets that user's environment, so the
    // standard-user run passes its two parameters in a file beside the binary.
    let from_file = || {
        let request = std::env::current_exe().ok()?.with_file_name("request.txt");
        let text = std::fs::read_to_string(request).ok()?;
        let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
        Some((lines.next()?.to_string(), lines.next()?.to_string()))
    };
    let Some((task_name, out_path)) = from_env.or_else(from_file) else {
        return;
    };
    let mut lines = Vec::new();
    let integrity = integrity_label();
    let administrators_deny_only = Command::new(r"C:\Windows\System32\whoami.exe")
        .arg("/groups")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
        .lines()
        .any(|line| line.contains("BUILTIN\\Administrators") && line.contains("deny only"));
    lines.push(format!(
        "token integrity={integrity:?} administrators_group_deny_only={administrators_deny_only}"
    ));

    let installer = WindowsTaskInstaller::new(TaskSpec {
        task_name: task_name.clone(),
        program: PROGRAM.to_string(),
        args: vec!["--service-run".to_string()],
    });
    let install = installer.install();
    lines.push(format!(
        "product_install_logon_task_in_folder={} status_after={:?}",
        show(&install),
        installer.status().ok()
    ));
    if install.is_ok() {
        lines.push(format!(
            "product_uninstall={}",
            show(&installer.uninstall())
        ));
    }

    let run = format!("\"{PROGRAM}\" \"--service-run\"");
    let once_in_folder = format!("{task_name}Once");
    let cases: [(&str, &str, &[&str]); 3] = [
        (
            "once_trigger_in_folder",
            &once_in_folder,
            &["/SC", "ONCE", "/ST", "23:59"],
        ),
        (
            "once_trigger_at_root",
            "HalvoVerifyLimitedRootOnce",
            &["/SC", "ONCE", "/ST", "23:59"],
        ),
        (
            "logon_trigger_at_root",
            "HalvoVerifyLimitedRootLogon",
            &["/SC", "ONLOGON"],
        ),
    ];
    for (label, name, trigger) in cases {
        let mut args = vec!["/Create", "/F"];
        args.extend_from_slice(trigger);
        args.extend_from_slice(&["/RL", "LIMITED", "/IT", "/TN", name, "/TR", &run]);
        let created = schtasks(&args);
        lines.push(format!("schtasks_create_{label}={created}"));
        if created == "ok" {
            lines.push(format!(
                "schtasks_delete_{label}={}",
                schtasks(&["/Delete", "/F", "/TN", name])
            ));
        }
    }
    std::fs::write(out_path, lines.join("\n")).expect("write the restricted-token result");
}

/// A process running under a token like the one an ordinary desktop process
/// has on a machine with UAC: the administrators group deny-only, the
/// privileges of a standard user, medium integrity.
struct RestrictedProcess {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl RestrictedProcess {
    /// Wait up to `millis` and return the exit code.
    fn wait(self, millis: u32) -> u32 {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        let mut code = u32::MAX;
        unsafe {
            WaitForSingleObject(self.handle, millis);
            GetExitCodeProcess(self.handle, &mut code);
            CloseHandle(self.handle);
        }
        code
    }
}

/// Start this test binary's `test_name` under the restricted token, with
/// `env` added to its environment. It gets no console and no handles.
fn spawn_restricted(test_name: &str, env: &[(&str, String)]) -> Result<RestrictedProcess, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        CreateRestrictedToken, SetTokenInformation, TokenIntegrityLevel, LUA_TOKEN, SID,
        SID_AND_ATTRIBUTES, SID_IDENTIFIER_AUTHORITY, TOKEN_ALL_ACCESS, TOKEN_MANDATORY_LABEL,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, OpenProcessToken, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
    };
    const SE_GROUP_INTEGRITY: u32 = 0x20;
    const MEDIUM_INTEGRITY_RID: u32 = 0x2000;
    let last_error = || std::io::Error::last_os_error().to_string();

    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
            TOKEN_ALL_ACCESS,
            &mut token,
        )
    } == 0
    {
        return Err(format!("OpenProcessToken: {}", last_error()));
    }
    let mut limited: HANDLE = std::ptr::null_mut();
    let restricted = unsafe {
        CreateRestrictedToken(
            token,
            LUA_TOKEN,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            &mut limited,
        )
    };
    let restricted_error = last_error();
    unsafe {
        CloseHandle(token);
    }
    if restricted == 0 {
        return Err(format!("CreateRestrictedToken: {restricted_error}"));
    }
    let mut medium = SID {
        Revision: 1,
        SubAuthorityCount: 1,
        IdentifierAuthority: SID_IDENTIFIER_AUTHORITY {
            Value: [0, 0, 0, 0, 0, 16],
        },
        SubAuthority: [MEDIUM_INTEGRITY_RID],
    };
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: (&mut medium as *mut SID).cast(),
            Attributes: SE_GROUP_INTEGRITY,
        },
    };
    if unsafe {
        SetTokenInformation(
            limited,
            TokenIntegrityLevel,
            (&label as *const TOKEN_MANDATORY_LABEL).cast(),
            (core::mem::size_of::<TOKEN_MANDATORY_LABEL>() + core::mem::size_of::<SID>()) as u32,
        )
    } == 0
    {
        let error = last_error();
        unsafe {
            CloseHandle(limited);
        }
        return Err(format!("SetTokenInformation: {error}"));
    }

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command_line: Vec<u16> = format!(
        "\"{}\" {test_name} --exact --ignored --nocapture",
        exe.display()
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    for (name, value) in env {
        std::env::set_var(name, value);
    }
    let mut startup: STARTUPINFOW = unsafe { core::mem::zeroed() };
    startup.cb = core::mem::size_of::<STARTUPINFOW>() as u32;
    let mut info: PROCESS_INFORMATION = unsafe { core::mem::zeroed() };
    let created = unsafe {
        CreateProcessAsUserW(
            limited,
            std::ptr::null(),
            command_line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut info,
        )
    };
    let created_error = last_error();
    for (name, _) in env {
        std::env::remove_var(name);
    }
    unsafe {
        CloseHandle(limited);
    }
    if created == 0 {
        return Err(format!("CreateProcessAsUserW: {created_error}"));
    }
    unsafe {
        CloseHandle(info.hThread);
    }
    Ok(RestrictedProcess {
        handle: info.hProcess,
    })
}

/// The integrity level line of `whoami /groups`, for example
/// `Mandatory Label\Medium Mandatory Level`.
fn integrity_label() -> String {
    let groups = Command::new(r"C:\Windows\System32\whoami.exe")
        .arg("/groups")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    groups
        .lines()
        .find(|line| line.contains("Mandatory Label"))
        .map(|line| {
            line.split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Not one of the nine claims: claim 7 ran as an elevated administrator, which
/// is what the runner gives. This runs the same install under a token like the
/// one an ordinary desktop process has.
#[test]
#[ignore = "functional: creates a restricted token and a scheduled task"]
fn x4_logon_task_install_without_elevation() {
    with_deadline("x4", Duration::from_secs(180), || {
        let task_name = r"Halvo\HalvoVerifyLimitedProbe";
        let dir = tempfile::tempdir().expect("temp dir");
        let out_path = dir.path().join("limited.txt");
        say(
            "x4",
            format!("this_process integrity={:?}", integrity_label()),
        );
        let child = spawn_restricted(
            "helper_limited_install",
            &[
                (LIMITED_TASK_ENV, task_name.to_string()),
                (LIMITED_OUT_ENV, out_path.display().to_string()),
            ],
        );
        say(
            "x4",
            format!(
                "restricted_process_started={}",
                show(&child.as_ref().map(|_| ()))
            ),
        );
        let child = child.expect("INCONCLUSIVE: no process under the restricted token");
        let exit_code = child.wait(120_000);
        let report = std::fs::read_to_string(&out_path).unwrap_or_default();
        say(
            "x4",
            format!(
                "child_exit_code={exit_code} child_wrote_report={}",
                !report.is_empty()
            ),
        );
        for line in report.lines() {
            say("x4", format!("restricted_token {line}"));
        }

        let installer = WindowsTaskInstaller::new(TaskSpec {
            task_name: task_name.to_string(),
            program: PROGRAM.to_string(),
            args: vec!["--service-run".to_string()],
        });
        let left_behind = installer.status().ok();
        let elevated_install = installer.install();
        let elevated_uninstall = installer.uninstall();
        say(
            "x4",
            format!(
                "task_status_after_restricted_run={left_behind:?} \
                 elevated_control_install_same_task={} elevated_control_uninstall={}",
                show(&elevated_install),
                show(&elevated_uninstall),
            ),
        );
        assert!(
            !report.is_empty(),
            "INCONCLUSIVE: the restricted process wrote no report"
        );
    });
}

fn unix_millis(time: SystemTime) -> u128 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Runs under the restricted token x8 makes: a keyboard source and a window of
/// medium integrity. Writes the time of every keystroke event to the file x8
/// names.
#[test]
#[ignore = "helper: does nothing unless x8 launches it under a restricted token"]
fn helper_limited_listener() {
    let (Ok(out_path), Ok(secs)) = (
        std::env::var(LISTENER_OUT_ENV),
        std::env::var(CHILD_SECS_ENV),
    ) else {
        return;
    };
    let secs: u64 = secs.parse().expect("listener seconds");
    let (sink, writer) = channel(4096);
    let mut keyboard = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
    let start = keyboard.start(sink.clone());
    let window = TestWindow::create(&normal_window(
        LISTENER_WINDOW_CLASS,
        "halvo-verify-listener",
        320,
        60,
    ));
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut stamps = Vec::new();
    while Instant::now() < deadline {
        pump_thread(Duration::from_millis(20));
        for event in drain_for(&writer, Duration::from_millis(20)) {
            if event.modality == Modality::Keystroke {
                stamps.push(unix_millis(event.captured_at.wall_clock).to_string());
            }
        }
    }
    keyboard.stop();
    let report = format!(
        "keyboard_start={} window_created={} integrity={:?}\n{}",
        show(&start),
        window.is_ok(),
        integrity_label(),
        stamps.join(",")
    );
    std::fs::write(out_path, report).expect("write the listener result");
}

/// Not one of the nine claims: the code relies on the integrity boundary to
/// keep keys typed into an elevated window away from the keyboard source.
/// A keyboard source of medium integrity listens while keys go first to an
/// elevated window and then to a window of its own integrity.
#[test]
#[ignore = "functional: needs an interactive Windows desktop and an elevated process"]
fn x8_raw_input_across_the_integrity_boundary() {
    with_deadline("x8", Duration::from_secs(120), || {
        env_report("x8");
        let dir = tempfile::tempdir().expect("temp dir");
        let out_path = dir.path().join("listener.txt");
        say(
            "x8",
            format!("this_process integrity={:?}", integrity_label()),
        );
        let listener = spawn_restricted(
            "helper_limited_listener",
            &[
                (LISTENER_OUT_ENV, out_path.display().to_string()),
                (CHILD_SECS_ENV, "9".to_string()),
            ],
        );
        say(
            "x8",
            format!("listener_started={}", show(&listener.as_ref().map(|_| ()))),
        );
        let listener = listener.expect("INCONCLUSIVE: no process under the restricted token");

        let own = TestWindow::create(&normal_window(
            "HalvoVerifyElevatedWindow",
            "halvo-verify-elevated",
            60,
            60,
        ))
        .expect("elevated window");
        pump_thread(Duration::from_millis(2500));
        let (own_foreground, method) = own.activate();
        pump_thread(Duration::from_millis(300));
        let elevated_from = unix_millis(SystemTime::now());
        press_keys(5);
        pump_thread(Duration::from_millis(400));
        let elevated_to = unix_millis(SystemTime::now());
        let own_foreground_after = own.is_foreground();

        let class = HSTRING::from(LISTENER_WINDOW_CLASS);
        let listener_window = unsafe { FindWindowW(&class, PCWSTR::null()) };
        let handed_over = match &listener_window {
            Ok(hwnd) => {
                unsafe {
                    let _ = SetForegroundWindow(*hwnd);
                }
                pump_thread(Duration::from_millis(400));
                unsafe { GetForegroundWindow() == *hwnd }
            }
            Err(_) => false,
        };
        let medium_from = unix_millis(SystemTime::now());
        press_keys(5);
        pump_thread(Duration::from_millis(400));
        let medium_to = unix_millis(SystemTime::now());
        let still_handed_over = listener_window
            .as_ref()
            .is_ok_and(|hwnd| unsafe { GetForegroundWindow() == *hwnd });

        let exit_code = listener.wait(60_000);
        let report = std::fs::read_to_string(&out_path).unwrap_or_default();
        let mut lines = report.lines();
        let header = lines.next().unwrap_or("").to_string();
        let stamps: Vec<u128> = lines
            .next()
            .unwrap_or("")
            .split(',')
            .filter_map(|stamp| stamp.parse().ok())
            .collect();
        let within =
            |from: u128, to: u128| stamps.iter().filter(|t| (from..=to).contains(*t)).count();
        say("x8", format!("listener exit_code={exit_code} {header}"));
        say(
            "x8",
            format!(
                "elevated_window_foreground={own_foreground} method={method} \
                 elevated_window_foreground_after_typing={own_foreground_after} \
                 keys_typed_into_elevated_window=5 keystroke_events_in_medium_integrity_listener={} \
                 listener_window_found={} foreground_handed_to_listener_window={handed_over} \
                 still_foreground_after_typing={still_handed_over} \
                 keys_typed_into_medium_integrity_window=5 keystroke_events_in_listener={} \
                 listener_keystroke_events_total={}",
                within(elevated_from, elevated_to),
                listener_window.is_ok(),
                within(medium_from, medium_to),
                stamps.len(),
            ),
        );
        assert!(
            !header.is_empty(),
            "INCONCLUSIVE: the listener wrote no report"
        );
        assert!(
            own_foreground && own_foreground_after && handed_over && still_handed_over,
            "INCONCLUSIVE: the foreground window was not where the test needed it"
        );
        assert!(
            within(medium_from, medium_to) >= 1,
            "INCONCLUSIVE: the listener saw no keys even for a window of its own integrity"
        );
    });
}

/// Not one of the nine claims: does the screen source survive being stopped
/// and started again in one process, which pause and resume would do.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x5_screen_source_stop_and_start_again() {
    with_deadline("x5", Duration::from_secs(120), || {
        env_report("x5");
        let last_app = Arc::new(Mutex::new(None));
        for cycle in 1..=3 {
            let (sink, writer) = channel(256);
            let mut screen = ScreenSource::new(Arc::clone(&last_app));
            let start = screen.start(sink.clone());
            let frames = count(
                &drain_for(&writer, Duration::from_secs(2)),
                Modality::ScreenFrame,
            );
            say(
                "x5",
                format!("cycle={cycle} start={} frames_in_2s={frames}", show(&start)),
            );
            screen.stop();
            say("x5", format!("cycle={cycle} stopped=true"));
            std::thread::sleep(Duration::from_millis(500));
        }
        std::thread::sleep(Duration::from_secs(3));
        say(
            "x5",
            "result three_cycles_completed=true idle_3s_after=true",
        );
    });
}

/// Not one of the nine claims: one capture session of the test's own pixel
/// probe, to tell a crash of the probe from a crash of a second session.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x6_pixel_probe_single_capture() {
    with_deadline("x6", Duration::from_secs(120), || {
        env_report("x6");
        let sample = capture_pixel(10, 10);
        say(
            "x6",
            format!("single_capture={}", show(&sample.as_ref().map(|_| ()))),
        );
        std::thread::sleep(Duration::from_secs(3));
        say("x6", "result idle_3s_after=true");
    });
}

/// How many of the sampled pixels on the left screen edge are yellow, read
/// through GDI and not through the capture session.
fn yellow_edge_pixels() -> usize {
    let (_, cy) = screen_size();
    let mut yellow = 0;
    unsafe {
        let dc = GetDC(None);
        for x in 0..6 {
            for y in [cy / 6, cy / 3, cy / 2, 2 * cy / 3, 5 * cy / 6] {
                let color = GetPixel(dc, x, y).0;
                let (r, g, b) = (color & 0xFF, (color >> 8) & 0xFF, (color >> 16) & 0xFF);
                if r > 200 && g > 150 && b < 100 {
                    yellow += 1;
                }
            }
        }
        ReleaseDC(None, dc);
    }
    yellow
}

/// Not one of the nine claims: does the product's capture session draw the
/// system capture border on the screen while it runs.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x7_capture_border_on_the_screen() {
    with_deadline("x7", Duration::from_secs(60), || {
        env_report("x7");
        let before = yellow_edge_pixels();
        let (sink, writer) = channel(256);
        let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
        let start = screen.start(sink.clone());
        let frames = count(
            &drain_for(&writer, Duration::from_millis(1500)),
            Modality::ScreenFrame,
        );
        let during = yellow_edge_pixels();
        screen.stop();
        std::thread::sleep(Duration::from_secs(1));
        let after = yellow_edge_pixels();
        say(
            "x7",
            format!(
                "screen_start={} frames={frames} sampled_edge_pixels=30 yellow_before_capture={before} \
                 yellow_during_capture={during} yellow_after_stop={after}",
                show(&start)
            ),
        );
        assert!(start.is_ok(), "the screen source must start");
    });
}

fn set_resolution(width: i32, height: i32) -> bool {
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            &format!("Set-DisplayResolution -Width {width} -Height {height} -Force"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Not one of the nine claims: what the screen source delivers when the
/// display mode changes while it runs (a dock, a projector, a remote session).
#[test]
#[ignore = "functional: needs an interactive Windows desktop; changes the display mode"]
fn x9_screen_source_across_a_display_mode_change() {
    with_deadline("x9", Duration::from_secs(180), || {
        env_report("x9");
        let (start_w, start_h) = screen_size();
        let (other_w, other_h) = if (start_w, start_h) == (1280, 720) {
            (1024, 768)
        } else {
            (1280, 720)
        };
        let (sink, writer) = channel(1024);
        let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
        let start = screen.start(sink.clone());
        let before = drain_for(&writer, Duration::from_secs(3));
        summarize("x9", "before-mode-change", &before);

        let changed = set_resolution(other_w, other_h);
        let (now_w, now_h) = screen_size();
        let window =
            TestWindow::create(&magenta_popup("HalvoVerifyModeChange", "halvo-verify-mode"))
                .expect("moving window");
        for step in 0..12 {
            window.move_to(100 + step * 20, 100 + step * 10);
            pump_thread(Duration::from_millis(250));
        }
        drop(window);
        let after = drain_for(&writer, Duration::from_secs(2));
        summarize("x9", "after-mode-change", &after);
        screen.stop();
        let restored = set_resolution(start_w, start_h);
        say(
            "x9",
            format!(
                "screen_start={} mode_before={start_w}x{start_h} mode_change_command_ok={changed} \
                 mode_after={now_w}x{now_h} frames_before={} frames_after_change={} \
                 frames_after_with_new_size={} frames_after_with_old_size={} mode_restored={restored}",
                show(&start),
                count(&before, Modality::ScreenFrame),
                count(&after, Modality::ScreenFrame),
                after
                    .iter()
                    .filter(|e| e.segment_ref.byte_len == now_w as u64 * now_h as u64 * 4)
                    .count(),
                after
                    .iter()
                    .filter(|e| e.segment_ref.byte_len == start_w as u64 * start_h as u64 * 4)
                    .count(),
            ),
        );
        assert!(start.is_ok(), "the screen source must start");
        assert!(
            changed && (now_w, now_h) == (other_w, other_h),
            "INCONCLUSIVE: the display mode did not change"
        );
    });
}

/// Not one of the nine claims: how many frames the screen source delivers
/// while nothing on the screen changes. A topmost window of one color covers
/// the whole screen, so no other window's repaint reaches the display.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x13_frames_on_a_static_screen() {
    with_deadline("x13", Duration::from_secs(90), || {
        env_report("x13");
        let cover = TestWindow::create(&magenta_popup("HalvoVerifyStatic", "halvo-verify-static"))
            .expect("covering window");
        cover.cover_screen();
        pump_thread(Duration::from_secs(1));
        let (sink, writer) = channel(1024);
        let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
        let start = screen.start(sink.clone());
        let mut frames = 0;
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(8) {
            pump_thread(Duration::from_millis(100));
            frames += count(
                &drain_for(&writer, Duration::from_millis(100)),
                Modality::ScreenFrame,
            );
        }
        let covered = cover.is_foreground();
        drop(cover);
        let after_uncover = count(
            &drain_for(&writer, Duration::from_secs(2)),
            Modality::ScreenFrame,
        );
        screen.stop();
        say(
            "x13",
            format!(
                "screen_start={} static_screen_secs=8 frames_while_static={frames} \
                 cover_window_was_foreground={covered} frames_in_2s_after_uncover={after_uncover} \
                 min_frame_interval_ms=500",
                show(&start)
            ),
        );
        assert!(start.is_ok(), "the screen source must start");
    });
}

/// Not one of the nine claims: the product's screen source started and stopped
/// fifteen times in one process.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x12_screen_source_fifteen_start_stop_cycles() {
    with_deadline("x12", Duration::from_secs(180), || {
        env_report("x12");
        let last_app = Arc::new(Mutex::new(None));
        let mut frames_total = 0;
        for cycle in 1..=15 {
            let (sink, writer) = channel(256);
            let mut screen = ScreenSource::new(Arc::clone(&last_app));
            let start = screen.start(sink.clone());
            let frames = count(
                &drain_for(&writer, Duration::from_millis(700)),
                Modality::ScreenFrame,
            );
            frames_total += frames;
            screen.stop();
            say(
                "x12",
                format!(
                    "cycle={cycle} start={} frames={frames} stopped=true",
                    show(&start)
                ),
            );
        }
        std::thread::sleep(Duration::from_secs(3));
        say(
            "x12",
            format!("result cycles_completed=15 frames_total={frames_total} idle_3s_after=true"),
        );
    });
}

/// Not one of the nine claims: the test's pixel-reading capture handler
/// started and stopped eight times in one process. Run 1 ended in an access
/// violation after two such captures; this counts how often that happens.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x11_pixel_probe_eight_captures() {
    with_deadline("x11", Duration::from_secs(240), || {
        env_report("x11");
        for capture in 1..=8 {
            let sample = capture_pixel(10, 10);
            say(
                "x11",
                format!(
                    "capture={capture} result={}",
                    show(&sample.as_ref().map(|_| ()))
                ),
            );
        }
        std::thread::sleep(Duration::from_secs(3));
        say("x11", "result captures_completed=8 idle_3s_after=true");
    });
}

fn powershell_line(script: &str) -> String {
    Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

/// The rows `tasklist` prints for `image`, as (pid, session number) pairs.
fn processes_named(image: &str) -> Vec<(String, String)> {
    let filter = format!("IMAGENAME eq {image}");
    let out = Command::new("tasklist.exe")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    out.lines()
        .filter(|line| {
            line.to_ascii_lowercase()
                .contains(&image.to_ascii_lowercase())
        })
        .map(|line| {
            let fields: Vec<&str> = line.split(',').map(|f| f.trim_matches('"')).collect();
            (
                fields.get(1).unwrap_or(&"").to_string(),
                fields.get(3).unwrap_or(&"").to_string(),
            )
        })
        .collect()
}

unsafe extern "system" fn collect_visible_class(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    if IsWindowVisible(hwnd).as_bool() {
        let mut buf = [0u16; 128];
        let len = GetClassNameW(hwnd, &mut buf) as usize;
        let classes = &mut *(lparam.0 as *mut Vec<String>);
        classes.push(String::from_utf16_lossy(&buf[..len]));
    }
    windows::core::BOOL(1)
}

/// The window classes of the visible top-level windows, with their counts.
fn visible_window_classes() -> BTreeMap<String, usize> {
    let mut classes: Vec<String> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect_visible_class),
            LPARAM(&mut classes as *mut Vec<String> as isize),
        );
    }
    let mut counts = BTreeMap::new();
    for class in classes {
        *counts.entry(class).or_default() += 1;
    }
    counts
}

/// Not one of the nine claims: what the foreground source calls an app whose
/// window the system hosts in another process (a packaged UI app). The app is
/// the Settings app that ships with Windows.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x14_foreground_app_of_a_hosted_app() {
    with_deadline("x14", Duration::from_secs(120), || {
        env_report("x14");
        let (sink, writer) = channel(4096);
        let mut foreground = ForegroundAppSource::new();
        let start = foreground.start(sink.clone());
        let launched = Command::new("cmd.exe")
            .args(["/c", "start", "ms-settings:"])
            .status()
            .is_ok_and(|status| status.success());
        let mut events = Vec::new();
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(12) {
            events.extend(drain_for(&writer, Duration::from_millis(200)));
        }
        let settings_processes = processes_named("SystemSettings.exe").len();
        let foreground_now = current_foreground_app().map(|app| file_name(&app).to_string());
        foreground.stop();
        let _ = Command::new("taskkill.exe")
            .args(["/IM", "SystemSettings.exe", "/F"])
            .output();
        summarize("x14", "after-opening-settings", &events);
        let named = |file: &str| {
            events
                .iter()
                .filter(|e| file_name(&e.foreground_app).eq_ignore_ascii_case(file))
                .count()
        };
        say(
            "x14",
            format!(
                "foreground_start={} settings_launched={launched} settings_processes_running={settings_processes} \
                 foreground_app_file_now={foreground_now:?} events_naming_applicationframehost={} \
                 events_naming_systemsettings={}",
                show(&start),
                named("ApplicationFrameHost.exe"),
                named("SystemSettings.exe"),
            ),
        );
        assert!(start.is_ok(), "the foreground source must start");
        assert!(
            settings_processes >= 1,
            "INCONCLUSIVE: the Settings app did not start on this machine"
        );
    });
}

/// Not one of the nine claims: a key held down. The keyboard repeats a held
/// key as more key-down packets with no key-up between them; SendInput sends
/// that same sequence here.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x15_a_held_key() {
    with_deadline("x15", Duration::from_secs(60), || {
        const REPEATS: u32 = 6;
        env_report("x15");
        let (sink, writer) = channel(4096);
        let mut keyboard = RawInputSource::keyboard(Arc::new(Mutex::new(None)));
        let start = keyboard.start(sink.clone());
        let mut accepted = 0;
        for _ in 0..REPEATS {
            accepted += send(&[key_input(VK_F15, false)]);
            std::thread::sleep(Duration::from_millis(30));
        }
        accepted += send(&[key_input(VK_F15, true)]);
        let events = drain_for(&writer, Duration::from_secs(1));
        keyboard.stop();
        let keystrokes = count(&events, Modality::Keystroke);
        say(
            "x15",
            format!(
                "keyboard_start={} physical_presses=1 key_down_packets={REPEATS} key_up_packets=1 \
                 sendinput_events_accepted={accepted} keystroke_events={keystrokes} \
                 one_event_for_the_press={}",
                show(&start),
                keystrokes == 1,
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert!(keystrokes >= 1, "INCONCLUSIVE: no keystroke event at all");
    });
}

/// Not one of the nine claims: does the inventory hold packaged apps. The
/// runner's own terminal is one, so the log needs no app names.
#[test]
#[ignore = "functional: reads the Windows registry"]
fn x16_packaged_apps_and_the_inventory() {
    with_deadline("x16", Duration::from_secs(120), || {
        let scan = scan_installed().expect("scan the registry");
        let packages = powershell_line("(Get-AppxPackage | Measure-Object).Count");
        let terminal_package = powershell_line(
            "(Get-AppxPackage -Name Microsoft.WindowsTerminal | Measure-Object).Count",
        );
        let terminal_in_inventory = scan
            .apps
            .iter()
            .any(|app| app.name.to_ascii_lowercase().contains("terminal"));
        say(
            "x16",
            format!(
                "inventory_entries={} packaged_apps_registered_for_this_user={packages} \
                 windows_terminal_packages={terminal_package} foreground_app_file={:?} \
                 inventory_has_an_entry_naming_terminal={terminal_in_inventory}",
                scan.len(),
                current_foreground_app().as_deref().map(file_name),
            ),
        );
        assert!(
            !packages.is_empty(),
            "INCONCLUSIVE: could not count the packaged apps"
        );
    });
}

/// Not one of the nine claims: start the task the installer made, then
/// uninstall it, and see what happens to the process it started and whether
/// starting it put a window on the desktop.
#[test]
#[ignore = "functional: creates, runs and removes a Windows scheduled task"]
fn x17_a_running_task_and_uninstall() {
    with_deadline("x17", Duration::from_secs(180), || {
        const IMAGE: &str = "waitfor.exe";
        env_report("x17");
        let task_name = r"Halvo\HalvoVerifyRunning";
        let installer = WindowsTaskInstaller::new(TaskSpec {
            task_name: task_name.to_string(),
            program: r"C:\Windows\System32\waitfor.exe".to_string(),
            args: vec![
                "HalvoVerifySignal".to_string(),
                "/T".to_string(),
                "120".to_string(),
            ],
        });
        assert_eq!(
            installer.status().expect("status"),
            ServiceStatus::NotInstalled,
            "the probe task must not exist before the test"
        );
        assert!(
            processes_named(IMAGE).is_empty(),
            "INCONCLUSIVE: the probe program is already running"
        );
        let windows_before = visible_window_classes();
        let install = installer.install();
        let run = schtasks(&["/Run", "/TN", task_name]);
        std::thread::sleep(Duration::from_secs(4));
        let started = processes_named(IMAGE);
        let windows_after = visible_window_classes();
        let new_windows: Vec<String> = windows_after
            .iter()
            .filter(|(class, n)| **n > windows_before.get(*class).copied().unwrap_or(0))
            .map(|(class, n)| {
                format!(
                    "{class}+{}",
                    n - windows_before.get(class).copied().unwrap_or(0)
                )
            })
            .collect();

        let uninstall = installer.uninstall();
        std::thread::sleep(Duration::from_secs(2));
        let status_after = installer.status().ok();
        let still_running = processes_named(IMAGE);
        let _ = Command::new("taskkill.exe")
            .args(["/IM", IMAGE, "/F"])
            .output();
        say(
            "x17",
            format!(
                "install={} schtasks_run={run} task_processes_after_run={} task_process_sessions={:?} \
                 new_visible_top_level_windows={new_windows:?}",
                show(&install),
                started.len(),
                started
                    .iter()
                    .map(|(_, session)| session.as_str())
                    .collect::<Vec<_>>(),
            ),
        );
        say(
            "x17",
            format!(
                "uninstall={} status_after={status_after:?} task_processes_still_running_after_uninstall={}",
                show(&uninstall),
                still_running.len(),
            ),
        );
        assert!(install.is_ok(), "the install must succeed");
        assert!(
            !started.is_empty(),
            "INCONCLUSIVE: running the task started no process"
        );
    });
}

/// Not one of the nine claims: the second install when the program path holds
/// a letter outside ASCII, as a profile folder often does.
#[test]
#[ignore = "functional: creates and removes a Windows scheduled task"]
fn x18_second_install_with_a_non_ascii_path() {
    with_deadline("x18", Duration::from_secs(120), || {
        let task_name = r"Halvo\HalvoVerifyNonAscii";
        let program = "C:\\Program Files\\H\u{e4}lvo\\halvo-capture.exe";
        let installer = WindowsTaskInstaller::new(TaskSpec {
            task_name: task_name.to_string(),
            program: program.to_string(),
            args: vec!["--service-run".to_string()],
        });
        assert_eq!(
            installer.status().expect("status"),
            ServiceStatus::NotInstalled,
            "the probe task must not exist before the test"
        );
        let first_install = installer.install();
        let first = query_task_xml(task_name);
        std::thread::sleep(Duration::from_millis(2100));
        let second_install = installer.install();
        let second = query_task_xml(task_name);
        let uninstall = installer.uninstall();
        say(
            "x18",
            format!(
                "first_install={} installer_recognizes_its_own_task={} second_install={} \
                 definition_unchanged_by_second_install={} uninstall={}",
                show(&first_install),
                task_registers(&first.text, program),
                show(&second_install),
                first.text == second.text,
                show(&uninstall),
            ),
        );
        assert!(first_install.is_ok() && second_install.is_ok() && uninstall.is_ok());
    });
}

const VARIANT_ENV: &str = "HALVO_VERIFY_VARIANT";

/// Not one of the nine claims: what c8b does, with one part changed at a
/// time, to find which part its access violation needs. Variants: `baseline`
/// (c8b itself), `no-exclude` (the window is never excluded),
/// `product-source` (the product's screen source captures, so no pixel is
/// read), `delayed-destroy` (three seconds between the last capture and the
/// window's destruction).
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x19_exclusion_teardown_variants() {
    let variant = std::env::var(VARIANT_ENV).unwrap_or_else(|_| "baseline".to_string());
    with_deadline("x19", Duration::from_secs(120), move || {
        say("x19", format!("variant={variant} begin"));
        let window =
            TestWindow::create(&magenta_popup("HalvoVerifyVariant", "halvo-verify-variant"))
                .expect("window");
        pump_thread(Duration::from_millis(500));
        let (x, y) = (
            100 + WINDOW_WIDTH as u32 / 2,
            100 + WINDOW_HEIGHT as u32 / 2,
        );
        let capture = |label: &str| {
            if variant == "product-source" {
                let (sink, writer) = channel(256);
                let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
                let start = screen.start(sink.clone());
                let mut frames = 0;
                let began = Instant::now();
                while began.elapsed() < Duration::from_secs(3) {
                    pump_thread(Duration::from_millis(50));
                    frames += count(
                        &drain_for(&writer, Duration::from_millis(50)),
                        Modality::ScreenFrame,
                    );
                }
                screen.stop();
                say(
                    "x19",
                    format!(
                        "variant={variant} capture={label} product_source start={} frames={frames}",
                        show(&start)
                    ),
                );
            } else {
                let sample = capture_pixel(x, y);
                say(
                    "x19",
                    format!(
                        "variant={variant} capture={label} pixel_probe result={}",
                        show(&sample.as_ref().map(|_| ()))
                    ),
                );
            }
        };
        capture("first");
        if variant != "no-exclude" {
            let exclude = exclude_window_from_capture(window.hwnd);
            say(
                "x19",
                format!(
                    "variant={variant} exclude_window_from_capture={}",
                    show(&exclude)
                ),
            );
        }
        pump_thread(Duration::from_millis(500));
        capture("second");
        if variant == "delayed-destroy" {
            pump_thread(Duration::from_secs(3));
        }
        say("x19", format!("variant={variant} destroying_window=true"));
        drop(window);
        say("x19", format!("variant={variant} window_destroyed=true"));
        std::thread::sleep(Duration::from_secs(2));
        say("x19", format!("variant={variant} result completed=true"));
    });
}
