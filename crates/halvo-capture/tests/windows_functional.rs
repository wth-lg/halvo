#![cfg(target_os = "windows")]
#![deny(clippy::undocumented_unsafe_blocks)]

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

mod browser;
mod stall;
use stall::FieldWindow;
mod stamp_phases;
use stamp_phases::{period_ms, phase_of, Phase, TICK_PERIOD_FALLBACK_MS};

use halvo_capture::event::{CaptureStreamEvent, Modality};
use halvo_capture::floor::exclude::{FloorHandle, Lineage, NoLineage, Parent, ProcessId};
use halvo_capture::floor::payload::{Button, FoldClass, KeyClass};
use halvo_capture::floor::{Switch, Switches};
use halvo_capture::secure_input::windows::{ContentReading, PasswordGate};
use halvo_capture::sink::{channel, SinkHandle, SinkWriter};
use halvo_capture::source::{CaptureError, CaptureSource, SourceContext};
use halvo_capture::windows::apps::scan_installed;
use halvo_capture::windows::exclusion::ProcessLineage;
use halvo_capture::windows::foreground::{current_foreground_app, ForegroundAppSource};
use halvo_capture::windows::input::{seam, RawInputSource};
use halvo_capture::windows::screen::{ScreenSource, DEFAULT_MIN_FRAME_INTERVAL};
use halvo_capture::windows::sources::ServiceSet;
use halvo_capture::windows::window_title::WindowTitleSource;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, GetDC, GetPixel, ReleaseDC, HBRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_WHEEL,
    MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_A, VK_F15, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    FindWindowW, GetForegroundWindow, GetSystemMetrics, GetWindowDisplayAffinity,
    GetWindowThreadProcessId, PeekMessageW, RegisterClassW, SetForegroundWindow,
    SetWindowDisplayAffinity, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage,
    UnregisterClassW, MSG, PM_REMOVE, SC_KEYMENU, SM_CMONITORS, SM_CXSCREEN, SM_CYSCREEN,
    SM_REMOTESESSION, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
    SYSTEM_METRICS_INDEX, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SYSCOMMAND,
    WNDCLASSW, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP,
    WS_VISIBLE,
};

#[link(name = "kernel32")]
extern "system" {
    fn ProcessIdToSessionId(process_id: u32, session_id: *mut u32) -> i32;
    fn AddVectoredExceptionHandler(first: u32, handler: VectoredHandler) -> *mut core::ffi::c_void;
    fn GetModuleHandleExW(
        flags: u32,
        address: *const u16,
        module: *mut *mut core::ffi::c_void,
    ) -> i32;
    fn GetModuleFileNameW(module: *mut core::ffi::c_void, name: *mut u16, size: u32) -> u32;
    fn GetStdHandle(which: u32) -> *mut core::ffi::c_void;
    fn GetTickCount() -> u32;
    fn GetSystemTimeAdjustment(
        adjustment: *mut u32,
        increment: *mut u32,
        disabled: *mut i32,
    ) -> i32;
    fn WriteFile(
        file: *mut core::ffi::c_void,
        buffer: *const u8,
        len: u32,
        written: *mut u32,
        overlapped: *mut core::ffi::c_void,
    ) -> i32;
}

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut ExceptionRecord,
    address: *mut core::ffi::c_void,
    params: u32,
    info: [usize; 15],
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut core::ffi::c_void,
}

type VectoredHandler = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;

const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
const MODULE_FROM_ADDRESS: u32 = 0x4;
const MODULE_UNCHANGED_REFCOUNT: u32 = 0x2;

static FAULT_LINES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

struct FaultLine {
    buf: [u8; 320],
    len: usize,
}

impl FaultLine {
    fn text(&mut self, s: &[u8]) {
        for &b in s {
            if self.len < self.buf.len() - 1 {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn hex(&mut self, mut v: usize) {
        let mut digits = [0u8; 16];
        let mut n = 0;
        loop {
            let d = (v & 0xF) as u8;
            digits[n] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
            n += 1;
            v >>= 4;
            if v == 0 {
                break;
            }
        }
        self.text(b"0x");
        while n > 0 {
            n -= 1;
            self.text(&digits[n..n + 1]);
        }
    }
}

unsafe extern "system" fn on_fault(pointers: *mut ExceptionPointers) -> i32 {
    // SAFETY: the system passes a valid EXCEPTION_POINTERS whose record lives
    // while the handler runs.
    let record = unsafe { &*(*pointers).record };
    let watched = matches!(record.code, 0xC000_0005 | 0xC000_0374 | 0xC000_0409);
    if !watched || FAULT_LINES.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 3 {
        return 0;
    }
    let mut line = FaultLine {
        buf: [0; 320],
        len: 0,
    };
    line.text(b"fault code=");
    line.hex(record.code as usize);
    let mut module: *mut core::ffi::c_void = core::ptr::null_mut();
    // SAFETY: with FROM_ADDRESS the call reads the address as a lookup key
    // only, and `module` is a live local that it writes.
    let found = unsafe {
        GetModuleHandleExW(
            MODULE_FROM_ADDRESS | MODULE_UNCHANGED_REFCOUNT,
            record.address as *const u16,
            &mut module,
        )
    } != 0;
    if found {
        let mut name = [0u16; 260];
        // SAFETY: `name` holds 260 units, the size passed, and `module` is the
        // handle that the lookup above returned.
        let n = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), 260) } as usize;
        let start = name[..n]
            .iter()
            .rposition(|&c| c == b'\\' as u16)
            .map_or(0, |i| i + 1);
        line.text(b" module=");
        for &c in &name[start..n] {
            line.text(&[if c < 128 { c as u8 } else { b'?' }]);
        }
        line.text(b" offset=");
        line.hex((record.address as usize).wrapping_sub(module as usize));
    } else {
        line.text(b" module=<none> offset=");
        line.hex(record.address as usize & 0xFFFF);
    }
    line.text(b"\n");
    let mut written = 0u32;
    // SAFETY: the buffer and its length describe `line.buf`, and `written` is
    // a live local; the standard output handle is the process's own.
    unsafe {
        WriteFile(
            GetStdHandle(STD_OUTPUT_HANDLE),
            line.buf.as_ptr(),
            line.len as u32,
            &mut written,
            core::ptr::null_mut(),
        );
    }
    FAULT_LINES_WRITTEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    0
}

static FAULT_LINES_WRITTEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

struct CountingAllocator;

thread_local! {
    static COUNTED_ALLOCATIONS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

fn count_allocation() {
    let _ = COUNTED_ALLOCATIONS.try_with(|count| {
        if let Some(n) = count.get() {
            count.set(Some(n + 1));
        }
    });
}

// SAFETY: every call goes to the system allocator with the same arguments;
// the count is a thread-local cell with a constant initializer, which takes
// no allocation.
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: the caller's contract is the system allocator's.
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: as for alloc.
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        count_allocation();
        // SAFETY: as for alloc.
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: as for alloc.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations_of(body: impl FnOnce()) -> u64 {
    COUNTED_ALLOCATIONS.with(|count| count.set(Some(0)));
    body();
    COUNTED_ALLOCATIONS
        .with(|count| count.replace(None))
        .unwrap_or(0)
}

fn one_fault(address: *mut core::ffi::c_void) -> (u64, u32, i32) {
    use std::sync::atomic::Ordering::SeqCst;
    FAULT_LINES.store(0, SeqCst);
    let mut record = ExceptionRecord {
        code: 0xC000_0005,
        flags: 0,
        record: core::ptr::null_mut(),
        address,
        params: 0,
        info: [0; 15],
    };
    let mut pointers = ExceptionPointers {
        record: &mut record,
        context: core::ptr::null_mut(),
    };
    let before = FAULT_LINES_WRITTEN.load(SeqCst);
    let mut answer = -1;
    let allocations = allocations_of(|| {
        // SAFETY: `pointers` and `record` are live locals shaped as the system
        // passes them, and the handler reads them only during the call.
        answer = unsafe { on_fault(&mut pointers) };
    });
    (
        allocations,
        FAULT_LINES_WRITTEN.load(SeqCst) - before,
        answer,
    )
}

#[test]
#[ignore = "functional: calls the fault handler of the functional check"]
fn the_fault_line_allocates_nothing() {
    let control = allocations_of(|| {
        std::hint::black_box(Box::new(7u64));
    });
    let (in_module, lines, answer) = one_fault(on_fault as *mut core::ffi::c_void);
    let (at_zero, zero_lines, zero_answer) = one_fault(core::ptr::null_mut());
    say(
        "fault-line",
        format!(
            "allocations_of_one_box={control} allocations_of_the_handler={in_module} \
             fault_lines_written={lines} allocations_at_address_zero={at_zero} \
             fault_lines_written_at_address_zero={zero_lines}"
        ),
    );
    assert_eq!(
        control, 1,
        "INCONCLUSIVE: the counting allocator did not count a box"
    );
    assert_eq!(
        (lines, zero_lines),
        (1, 1),
        "INCONCLUSIVE: the handler wrote no fault line"
    );
    assert_eq!(
        (answer, zero_answer),
        (0, 0),
        "the handler must let the search go on"
    );
    assert_eq!((in_module, at_zero), (0, 0), "the fault line allocated");
}

fn install_fault_line() {
    // SAFETY: `on_fault` has the PVECTORED_EXCEPTION_HANDLER signature and
    // lives for the whole process.
    unsafe {
        AddVectoredExceptionHandler(1, on_fault);
    }
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

const ATTRIBUTION: &str = "halvo-verify-attributed-app";
const CHILD_SECS_ENV: &str = "HALVO_VERIFY_CHILD_SECS";
const CHILD_EXE_NAME: &str = "halvo-verify-child.exe";
/// The file name of a child that the floor's static list names.
const LISTED_CHILD_EXE: &str = "KeePass.exe";
const GHOST_CLASS: &str = "HalvoVerifyGhostChild";
const GHOST_CAPTION: &str = "halvo-verify-ghost-caption";
/// What the system adds to the caption of a hung window's ghost.
const GHOST_CAPTION_ENDING: &str = " (Not Responding)";
const BACKGROUND_CLASS: &str = "HalvoVerifyBackgroundChild";
const CHILD_MODE_ENV: &str = "HALVO_VERIFY_CHILD_MODE";
const LISTENER_OUT_ENV: &str = "HALVO_VERIFY_LISTENER_OUT";
const LISTENER_WINDOW_CLASS: &str = "HalvoVerifyListenerWindow";
const FIXED_TITLE: &str = "halvo-verify-second-window-with-a-fixed-title";
const MAGENTA: u32 = 0x00FF_00FF;
const FIRST_FRAME_WAIT: Duration = Duration::from_secs(10);
const FULL_SINK_STOP_LIMIT: Duration = Duration::from_secs(2);
const BOUNDS_FILE: &str = "docs/baseline/r1-gate-bounds.txt";

macro_rules! flipped {
    ($id:literal, $wanted:expr, $evidence:expr) => {{
        let evidence: String = $evidence;
        println!("{evidence}");
        let _ = $id;
        assert!($wanted, "{evidence}");
    }};
}

fn say(test: &str, message: impl AsRef<str>) {
    println!("TF {test} {}", message.as_ref());
}

/// How long a test waits for the first Clear of a password gate's content
/// reading.
const GATE_CLEAR_WAIT: Duration = Duration::from_secs(10);

/// Waits until the gate's content reading is Clear, up to `limit`, while this
/// thread answers its messages: whether it read Clear, and the wait.
fn wait_for_clear(gate: &PasswordGate, limit: Duration) -> (bool, Duration) {
    let began = Instant::now();
    let clear = |gate: &PasswordGate| matches!(gate.content_reading(), ContentReading::Clear(_));
    while !clear(gate) && began.elapsed() < limit {
        pump_thread(Duration::from_millis(20));
    }
    (clear(gate), began.elapsed())
}

/// [`wait_for_clear`] with its line.
fn content_reads_clear(test: &str, gate: &PasswordGate, limit: Duration) -> bool {
    let (gate_first_clear, waited) = wait_for_clear(gate, limit);
    say(
        test,
        format!(
            "gate_first_clear={gate_first_clear} waited_ms={} reading={:?}",
            waited.as_millis(),
            gate.content_reading()
        ),
    );
    gate_first_clear
}

/// The count of the content readings that the sources asked of the gate:
/// in all, Clear by the kind that answered, no answer, where a title is kept
/// and a frame is not, and Unknown by its cause.
fn gate_readings(test: &str, gate: &PasswordGate) {
    let readings = gate.readings();
    say(
        test,
        format!(
            "gate_readings clear={} no_answer={} password_field={} unknown={}",
            readings.clear(),
            readings.no_answer,
            readings.password_field,
            readings.unknown()
        ),
    );
    say(
        test,
        format!(
            "gate_clear_by native_edit={} chrome={} edge={} other_kind={}",
            readings.native_edit, readings.chrome, readings.edge, readings.other_kind
        ),
    );
    say(
        test,
        format!(
            "gate_unknown_by before_first_read={} firefox={} unreadable={} desktop_switch={}",
            readings.before_first_read,
            readings.firefox,
            readings.unreadable,
            readings.desktop_switch
        ),
    );
}

fn show<T, E: std::fmt::Display>(result: &Result<T, E>) -> String {
    match result {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("error({e})"),
    }
}

fn start_report<T>(test: &str, source: &str, result: &Result<T, CaptureError>) {
    if let Err(error) = result {
        say(
            test,
            format!(
                "start_failed source={source} error={error:?} text={}",
                error.to_string().replace(' ', "_")
            ),
        );
        if source == "keyboard" {
            detector_line(test, "start-failed");
        }
    }
}

/// Prints the per-user Uninstall key of both registry views when a test
/// starts and again when it ends, a panic or a time-out among the ends, so
/// that one hosted run shows after which test the key stands (ruling W3-27
/// (3) (b)).
struct UninstallKeyPrints {
    test: &'static str,
}

impl UninstallKeyPrints {
    fn before(test: &'static str) -> Self {
        uninstall_key_lines(test, "before");
        Self { test }
    }
}

impl Drop for UninstallKeyPrints {
    fn drop(&mut self) {
        uninstall_key_lines(self.test, "after");
    }
}

fn uninstall_key_lines(test: &str, when: &str) {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    for view in per_user_uninstall_views(&hkcu) {
        say(test, format!("uninstall_key_{when} {}", view.line()));
    }
}

fn with_deadline<F>(test: &'static str, limit: Duration, body: F)
where
    F: FnOnce() + Send + 'static,
{
    let _prints = UninstallKeyPrints::before(test);
    let (done, finished) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name(format!("tf-{test}"))
        .spawn(move || {
            body();
            let _ = done.send(());
        })
        .expect("spawn the test body");
    match finished.recv_timeout(limit) {
        Ok(()) => worker.join().expect("test body finished"),
        Err(RecvTimeoutError::Timeout) => {
            say(
                test,
                format!("result=TIMEOUT limit_secs={}", limit.as_secs()),
            );
            panic!("{test} did not finish within {limit:?}");
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
        return String::new();
    }
    let mut buf = [0u16; 256];
    let mut needed = 0u32;
    // SAFETY: `handle` is a non-null window-station or desktop handle, and the
    // buffer pointer and its byte length describe `buf`, which outlives the call.
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
        return String::new();
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn metric(index: SYSTEM_METRICS_INDEX) -> i32 {
    // SAFETY: GetSystemMetrics reads a system value and takes no pointer.
    unsafe { GetSystemMetrics(index) }
}

fn screen_size() -> (i32, i32) {
    (metric(SM_CXSCREEN), metric(SM_CYSCREEN))
}

fn foreground_window() -> HWND {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    unsafe { GetForegroundWindow() }
}

fn window_pid(hwnd: HWND) -> u32 {
    if hwnd.0.is_null() {
        return 0;
    }
    let mut pid = 0u32;
    // SAFETY: `pid` is a live u32 that the call writes the process id into.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    pid
}

fn foreground_pid() -> u32 {
    window_pid(foreground_window())
}

fn set_foreground(hwnd: HWND) {
    // SAFETY: SetForegroundWindow takes a window handle by value; a stale
    // handle makes the call fail and changes nothing.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
}

fn env_report(test: &str) {
    let mut session = u32::MAX;
    // SAFETY: `session` is a live u32 that the call writes the session id into.
    let session_ok = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } != 0;
    // SAFETY: both calls return handles that this process does not own and
    // must not close; they are only read by `user_object_name`.
    let (station, desktop) = unsafe {
        (
            user_object_name(GetProcessWindowStation()),
            user_object_name(GetThreadDesktop(GetCurrentThreadId())),
        )
    };
    // SAFETY: OpenInputDesktop returns a desktop handle or null; a non-null
    // handle is closed once, right after its name is read.
    let input = unsafe {
        let handle = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        let name = user_object_name(handle);
        if !handle.is_null() {
            CloseDesktop(handle);
        }
        name
    };
    let (cx, cy) = screen_size();
    say(
        test,
        format!(
            "env session_id_ok={session_ok} session_id={session} \
             window_station_is_winsta0={} desktop_is_default={} input_desktop_is_default={} \
             screen={cx}x{cy} monitors={} remote_session={} \
             foreground_window_present={} foreground_app_known={}",
            station.eq_ignore_ascii_case("WinSta0"),
            desktop.eq_ignore_ascii_case("Default"),
            input.eq_ignore_ascii_case("Default"),
            metric(SM_CMONITORS),
            metric(SM_REMOTESESSION),
            !foreground_window().0.is_null(),
            current_foreground_app().is_some(),
        ),
    );
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn own_exe_file() -> String {
    std::env::current_exe()
        .map(|exe| file_name(&exe.to_string_lossy()).to_string())
        .unwrap_or_default()
}

fn is_child_app(app: &str) -> bool {
    file_name(app).eq_ignore_ascii_case(CHILD_EXE_NAME)
}

fn app_class(app: &str, own_exe: &str) -> &'static str {
    if app.is_empty() {
        "empty"
    } else if app == ATTRIBUTION {
        "attribution"
    } else if is_child_app(app) {
        "child"
    } else if file_name(app).eq_ignore_ascii_case(own_exe) {
        "test"
    } else {
        "other"
    }
}

const SHUTDOWN_LINE: &str = "TF shutdown source=";

fn stop_counted<S: CaptureSource + ?Sized>(source: &mut S, name: &str) -> Result<(), CaptureError> {
    let stopped = source.shutdown();
    println!("{SHUTDOWN_LINE}{name} ok={}", stopped.is_ok());
    stopped
}

fn source_built_by_test(test: &str, source: &str) {
    say(test, format!("source_built_by_test={source}"));
    if source == "screen" {
        say(test, "screen_activity=on");
    }
}

/// The context of a screen source that a test builds: the switch
/// `screen_activity` is off by default, and the source starts only with it on.
fn screen_on(sink: SinkHandle) -> SourceContext {
    let mut switches = Switches::default();
    switches.set(Switch::ScreenActivity, true);
    SourceContext::with_floor(sink, FloorHandle::new(None, Arc::new(NoLineage), switches))
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

fn wheel_notch() -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 120,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> u32 {
    // SAFETY: `inputs` is a live slice of INPUT values and the size argument
    // is the size of one INPUT, as SendInput requires.
    unsafe { SendInput(inputs, core::mem::size_of::<INPUT>() as i32) }
}

fn type_letters(presses: u32) -> u32 {
    let mut accepted = 0;
    for _ in 0..presses {
        accepted += send(&[key_input(VK_A, false)]);
        std::thread::sleep(Duration::from_millis(5));
        accepted += send(&[key_input(VK_A, true)]);
        std::thread::sleep(Duration::from_millis(5));
    }
    accepted
}

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

/// Puts the pointer over the middle of the window in front, since a pointer
/// sample is decided on the window under the pointer; answers whether that
/// window is the one under the pointer then.
fn point_at_front() -> bool {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetWindowRect, SetCursorPos, WindowFromPoint, GA_ROOT,
    };
    let front = foreground_window();
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live RECT that the call writes into.
    if front.0.is_null() || unsafe { GetWindowRect(front, &mut rect) }.is_err() {
        return false;
    }
    let middle = POINT {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    // SAFETY: SetCursorPos, WindowFromPoint and GetAncestor take plain values.
    unsafe {
        let _ = SetCursorPos(middle.x, middle.y);
        GetAncestor(WindowFromPoint(middle), GA_ROOT) == front
    }
}

fn move_pointer(moves: u32) -> u32 {
    let mut accepted = 0;
    for i in 0..moves {
        let dx = if i.is_multiple_of(2) { 3 } else { -3 };
        accepted += send(&[mouse_move(dx, 0)]);
        std::thread::sleep(Duration::from_millis(5));
    }
    accepted
}

/// One event as the test reads it: the sink's event, with the app and the
/// title's length of a floor value filled from the seam's view of it, and the
/// view. The view names an app or a title only by the names that this test
/// gives it.
#[derive(Clone)]
struct Seen {
    event: CaptureStreamEvent,
    view: Option<seam::FloorView>,
    /// The OS time of a keystroke, which the seam keeps in the order of the
    /// keys that the source handed on; its record holds the second alone.
    key_time: Option<SystemTime>,
}

impl std::ops::Deref for Seen {
    type Target = CaptureStreamEvent;

    fn deref(&self) -> &CaptureStreamEvent {
        &self.event
    }
}

/// The app file names that the tests compare an event's app with.
fn app_names() -> Vec<String> {
    vec![
        CHILD_EXE_NAME.to_string(),
        own_exe_file(),
        "ApplicationFrameHost.exe".to_string(),
        "SystemSettings.exe".to_string(),
    ]
}

/// The titles that the tests compare a title event with by its length.
fn known_titles() -> Vec<String> {
    let mut titles: Vec<String> = (0..8).map(child_title).collect();
    titles.push(FIXED_TITLE.to_string());
    titles.extend(["f".repeat(30), "b".repeat(50), "h".repeat(60)]);
    for prefix in [
        PASSWORD_EDIT_TITLE,
        PLAIN_TITLE,
        PROTECTED_NO_ANSWER_TITLE,
        NO_ANSWER_TITLE,
    ] {
        titles.extend((0..3).map(|step| format!("{prefix}-{step}")));
    }
    titles.extend([OWN_WINDOW_PASSWORD_TITLE, OWN_WINDOW_PLAIN_TITLE].map(str::to_string));
    titles.push(GHOST_CAPTION.to_string());
    titles.push(GHOST_CAPTION.to_string() + GHOST_CAPTION_ENDING);
    titles
}

fn seen(received: halvo_capture::sink::Received, apps: &[String], titles: &[String]) -> Seen {
    let mut event = received.event().clone();
    let apps: Vec<&str> = apps.iter().map(String::as_str).collect();
    let titles_ref: Vec<&str> = titles.iter().map(String::as_str).collect();
    let view = received
        .into_floor()
        .ok()
        .map(|floor| seam::view(floor.event(), &apps, &titles_ref));
    if let Some(view) = &view {
        event.foreground_app = match view.app {
            seam::AppSeen::Unknown => String::new(),
            seam::AppSeen::Named(index) => apps[index].to_string(),
            seam::AppSeen::Other => "other.exe".to_string(),
        };
        if let Some(seam::TitleSeen {
            among: Some(index), ..
        }) = view.title
        {
            event.segment_ref.byte_len = titles[index].len() as u64;
        }
    }
    let key_time = (event.modality == Modality::Keystroke)
        .then(seam::next_recorded_tick)
        .flatten()
        .map(|tick| {
            // SAFETY: GetTickCount takes no argument and reads a counter.
            let age = unsafe { GetTickCount() }.wrapping_sub(tick);
            SystemTime::now() - Duration::from_millis(u64::from(age.min(i32::MAX as u32)))
        });
    Seen {
        event,
        view,
        key_time,
    }
}

struct Events(mpsc::Receiver<Seen>);

impl Events {
    fn drain_for(&self, window: Duration) -> Vec<Seen> {
        let deadline = Instant::now() + window;
        let mut events = Vec::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match self.0.recv_timeout(deadline - now) {
                Ok(event) => events.push(event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => sink_closed(),
            }
        }
        events
    }

    fn first_within(&self, limit: Duration) -> Option<Seen> {
        match self.0.recv_timeout(limit) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => sink_closed(),
        }
    }
}

fn sink_closed() -> ! {
    panic!("INCONCLUSIVE: the sink closed while the test still held a handle to it")
}

fn forward(writer: SinkWriter) -> Events {
    let (tx, rx) = mpsc::channel();
    let apps = app_names();
    let titles = known_titles();
    std::thread::Builder::new()
        .name("tf-sink-forward".to_string())
        .spawn(move || loop {
            match writer.recv_timeout(Duration::from_secs(60)) {
                Ok(received) => {
                    if tx.send(seen(received, &apps, &titles)).is_err() {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        })
        .expect("spawn the sink forwarder");
    Events(rx)
}

fn capture(capacity: usize) -> (SinkHandle, Events) {
    let (sink, writer) = channel(capacity);
    (sink, forward(writer))
}

fn count(events: &[Seen], modality: Modality) -> usize {
    events.iter().filter(|e| e.modality == modality).count()
}

fn summarize(test: &str, phase: &str, events: &[Seen]) {
    let own_exe = own_exe_file();
    let mut groups: BTreeMap<(String, &'static str, u64), usize> = BTreeMap::new();
    for event in events {
        let key = (
            format!("{:?}", event.modality),
            app_class(&event.foreground_app, &own_exe),
            event.segment_ref.byte_len,
        );
        *groups.entry(key).or_default() += 1;
    }
    say(
        test,
        format!(
            "phase={phase} events={} groups={}",
            events.len(),
            groups.len()
        ),
    );
    for ((modality, app, byte_len), n) in groups.iter().take(40) {
        say(
            test,
            format!("phase={phase} modality={modality} app={app} byte_len={byte_len} count={n}"),
        );
    }
    if groups.len() > 40 {
        say(
            test,
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
    if msg == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) == SC_KEYMENU as usize {
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn pump_thread(window: Duration) {
    let deadline = Instant::now() + window;
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same
        // value is handed to TranslateMessage and DispatchMessageW.
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

fn pump_answering(window: Duration) {
    use windows::Win32::UI::WindowsAndMessaging::{
        MsgWaitForMultipleObjectsEx, MWMO_INPUTAVAILABLE, QS_ALLINPUT,
    };
    let deadline = Instant::now() + window;
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same
        // value is handed to TranslateMessage and DispatchMessageW.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let left = u32::try_from(left.as_millis()).unwrap_or(u32::MAX).max(1);
        // SAFETY: no handle is passed; the call waits for this thread's
        // queue or the time.
        unsafe {
            let _ = MsgWaitForMultipleObjectsEx(None, left, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
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

struct Brush(HBRUSH);

impl Brush {
    fn solid(color: u32) -> Self {
        // SAFETY: CreateSolidBrush takes a color by value and returns a brush
        // that this value owns until it is deleted or given to a class.
        Self(unsafe { CreateSolidBrush(COLORREF(color)) })
    }

    fn give_to_class(self) {
        std::mem::forget(self);
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        // SAFETY: no registered class holds this brush, so this value is its
        // one owner and deletes it once, here.
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

struct ClassRegistration {
    name: HSTRING,
    hinstance: HINSTANCE,
}

impl ClassRegistration {
    fn register(name: &str, fill: u32) -> Result<Self, String> {
        let name = HSTRING::from(name);
        // SAFETY: a null module name asks for the handle of this executable,
        // which stays loaded for the life of the process.
        let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .map_err(|e| format!("GetModuleHandleW: {e}"))?
            .into();
        let brush = Brush::solid(fill);
        let class = WNDCLASSW {
            lpfnWndProc: Some(test_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(name.as_ptr()),
            hbrBackground: brush.0,
            ..Default::default()
        };
        // SAFETY: `class` points at `name`, which the returned value holds
        // until its Drop unregisters the class; on failure `brush` deletes
        // the brush.
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(format!(
                "RegisterClassW: {}",
                windows::core::Error::from_thread()
            ));
        }
        brush.give_to_class();
        Ok(Self { name, hinstance })
    }
}

impl Drop for ClassRegistration {
    fn drop(&mut self) {
        // SAFETY: this value registered the class and unregisters it once,
        // here; the system then deletes the class's background brush.
        unsafe {
            let _ = UnregisterClassW(PCWSTR(self.name.as_ptr()), Some(self.hinstance));
        }
    }
}

struct TestWindow {
    hwnd: HWND,
    class: ClassRegistration,
}

impl TestWindow {
    fn create(spec: &WindowSpec) -> Result<Self, String> {
        let class = ClassRegistration::register(spec.class, spec.fill)?;
        let title = HSTRING::from(spec.title);
        // SAFETY: the class name and title point at live HSTRINGs for the
        // duration of the call, and the class was registered above.
        let hwnd = unsafe {
            CreateWindowExW(
                spec.ex_style,
                PCWSTR(class.name.as_ptr()),
                PCWSTR(title.as_ptr()),
                spec.style,
                spec.x,
                spec.y,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                None,
                None,
                Some(class.hinstance),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW: {e}"))?;
        Ok(Self { hwnd, class })
    }

    fn child_control(&self, class: &str, style: WINDOW_STYLE, y: i32) -> Result<HWND, String> {
        let class = HSTRING::from(class);
        // SAFETY: the class name points at a live HSTRING for the call, and the
        // parent is this window, which the calling thread owns.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | style,
                10,
                y,
                300,
                24,
                Some(self.hwnd),
                None,
                Some(self.class.hinstance),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW: {e}"))
    }

    fn set_title(&self, title: &str) -> Result<(), String> {
        set_text(self.hwnd, title)
    }

    fn move_to(&self, x: i32, y: i32) {
        // SAFETY: SetWindowPos takes this thread's window handle by value.
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
        // SAFETY: SetWindowPos takes this thread's window handle by value.
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, 0, 0, cx, cy, SWP_NOZORDER);
        }
    }

    fn is_foreground(&self) -> bool {
        foreground_window() == self.hwnd
    }

    fn activate(&self) -> (bool, &'static str) {
        self.activate_with(true)
    }

    fn activate_with(&self, keys: bool) -> (bool, &'static str) {
        // SAFETY: the three calls take this thread's window handle by value.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetForegroundWindow(self.hwnd);
        }
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return (true, "SetForegroundWindow");
        }

        if keys {
            send(&[key_input(VK_MENU, false)]);
            set_foreground(self.hwnd);
            send(&[key_input(VK_MENU, true)]);
            pump_thread(Duration::from_millis(200));
            if self.is_foreground() {
                return (true, "alt-key-unlock");
            }
        }

        let current_thread = {
            let mut pid = 0u32;
            // SAFETY: `pid` is a live u32 that the call writes into.
            unsafe { GetWindowThreadProcessId(foreground_window(), Some(&mut pid)) }
        };
        // SAFETY: GetCurrentThreadId takes no argument.
        let this_thread = unsafe { GetCurrentThreadId() };
        if current_thread != 0 && current_thread != this_thread {
            // SAFETY: the input of the two threads is joined only around the two
            // window calls and detached again before the block ends.
            unsafe {
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

        // SAFETY: ShowWindow takes this thread's window handle by value.
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
        // SAFETY: the window belongs to this value and is destroyed once,
        // here, by the thread that created it; its class is unregistered
        // after this, when the field `class` drops.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
        pump_thread(Duration::from_millis(50));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Plain,
    Password,
}

fn focus_on(hwnd: HWND) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
    // SAFETY: SetFocus and GetFocus take and return handle values of this
    // thread's windows.
    unsafe {
        let _ = SetFocus(Some(hwnd));
        GetFocus() == hwnd
    }
}

fn type_letters_slowly(letters: u32) -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_A;
    let mut accepted = 0;
    for _ in 0..letters {
        accepted += send(&[key_input(VK_A, false)]);
        std::thread::sleep(Duration::from_millis(20));
        accepted += send(&[key_input(VK_A, true)]);
        std::thread::sleep(Duration::from_millis(20));
    }
    accepted
}

fn set_text(hwnd: HWND, text: &str) -> Result<(), String> {
    let text = HSTRING::from(text);
    // SAFETY: `text` is a live HSTRING for the duration of the call.
    unsafe { SetWindowTextW(hwnd, &text) }.map_err(|e| format!("SetWindowTextW: {e}"))
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

fn child_title(step: usize) -> String {
    format!("halvo-verify-{}", "x".repeat(step % 8 + 1))
}

const CHILD_TITLE_LENGTHS: std::ops::RangeInclusive<u64> = 14..=21;
const CHILD_TITLE_SIZES_NO_SWITCH_SHOWS: [u64; 4] = [15, 17, 19, 21];

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
        Ok("root") => return child_root(secs),
        Ok("password-title") => return child_password_title(),
        Ok("no-answer-title") => return child_no_answer_title(),
        Ok("own-window") => return child_own_window(),
        Ok("hang") => return child_hang(),
        Ok("background") => return child_background(secs),
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
        FIXED_TITLE,
        220,
        180,
    ))
    .expect("second child window");
    let first_edit = first
        .child_control("EDIT", WINDOW_STYLE(0), 10)
        .expect("the first window's edit");
    let second_edit = second
        .child_control("EDIT", WINDOW_STYLE(0), 10)
        .expect("the second window's edit");
    pump_thread(Duration::from_millis(300));
    let (foreground, method) = first.activate();
    let edit_focused = focus_on(first_edit);
    say(
        "child",
        format!("first_window_foreground={foreground} method={method} edit_focused={edit_focused}"),
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
            let (target, edit) = if (step / 2) % 2 == 1 {
                (&second, second_edit)
            } else {
                (&first, first_edit)
            };
            set_foreground(target.hwnd);
            focus_on(edit);
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

fn child_scope() {
    let first = TestWindow::create(&normal_window(
        "HalvoVerifyScopeFirst",
        "halvo-verify-scope-first",
        40,
        40,
    ))
    .expect("foreground window");
    let label = first
        .child_control("STATIC", WINDOW_STYLE(0), 10)
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
    let edit = first
        .child_control("EDIT", WINDOW_STYLE(0), 50)
        .expect("the first window's edit");
    pump_thread(Duration::from_millis(300));
    let (foreground, method) = first.activate();
    focus_on(edit);
    pump_thread(Duration::from_millis(500));

    first.set_title(&"f".repeat(30)).expect("foreground title");
    pump_thread(Duration::from_millis(400));
    let label_set = set_text(label, &"c".repeat(40)).is_ok();
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
    let edit = first
        .child_control("EDIT", WINDOW_STYLE(0), 10)
        .expect("the first window's edit");
    let second_edit = second
        .child_control("EDIT", WINDOW_STYLE(0), 10)
        .expect("the second window's edit");
    pump_answering(Duration::from_millis(300));
    let (foreground, method) = first.activate();
    let edit_focused = focus_on(edit);
    say(
        "child",
        format!(
            "mode=hold first_window_foreground={foreground} method={method} \
             plain_edit_has_focus={edit_focused}"
        ),
    );
    pump_answering(Duration::from_secs(secs));
    let held = first.is_foreground();
    set_foreground(second.hwnd);
    focus_on(second_edit);
    pump_answering(Duration::from_millis(100));
    say(
        "child",
        format!(
            "mode=hold first_window_held_foreground={held} switched_to_second_window={}",
            second.is_foreground()
        ),
    );
    pump_answering(Duration::from_secs(3));
}

/// The exclusion root of the tree test: it starts a grandchild of the test
/// that holds a window in front with a plain edit focused, and waits for it.
fn child_root(secs: u64) {
    let mut grandchild = spawn_child("hold", secs);
    let exit = grandchild.wait();
    grandchild.print_output();
    say(
        "child",
        format!("mode=root grandchild_exit_ok={}", exit.success()),
    );
}

/// A window that stays where it was made and answers its messages for `secs`.
fn child_background(secs: u64) {
    let _window = TestWindow::create(&normal_window(
        BACKGROUND_CLASS,
        "halvo-verify-background",
        420,
        320,
    ))
    .expect("the window");
    pump_answering(Duration::from_secs(secs));
    say("child", "mode=background");
}

/// A window that comes to the front, then a thread that takes no message for
/// 8 s, so that the system reads the window as hung and may show its ghost.
fn child_hang() {
    let window =
        TestWindow::create(&normal_window(GHOST_CLASS, GHOST_CAPTION, 60, 60)).expect("the window");
    pump_answering(Duration::from_millis(300));
    let (foreground, method) = window.activate();
    pump_answering(Duration::from_millis(1_500));
    std::thread::sleep(Duration::from_secs(8));
    pump_answering(Duration::from_millis(2_000));
    say(
        "child",
        format!("mode=hang foreground={foreground} method={method}"),
    );
}

const PASSWORD_EDIT_TITLE: &str = "halvo-verify-title-in-a-password-edit";
const PLAIN_TITLE: &str = "halvo-verify-plain-title";

/// A window with a password edit and a plain edit: three title changes while
/// the password edit has the focus, then three while the plain edit has it.
fn child_password_title() {
    let window = TestWindow::create(&normal_window(
        "HalvoVerifyPasswordTitle",
        "halvo-verify-password-title",
        40,
        40,
    ))
    .expect("the window");
    let password = window
        .child_control("EDIT", WINDOW_STYLE(ES_PASSWORD_STYLE), 10)
        .expect("the password edit");
    let plain = window
        .child_control("EDIT", WINDOW_STYLE(0), 50)
        .expect("the plain edit");
    pump_answering(Duration::from_millis(300));
    let (foreground, method) = window.activate();
    let mut phases = Vec::new();
    for (edit, prefix) in [(password, PASSWORD_EDIT_TITLE), (plain, PLAIN_TITLE)] {
        let focused = focus_on(edit);
        pump_answering(Duration::from_millis(800));
        for step in 0..3 {
            window
                .set_title(&format!("{prefix}-{step}"))
                .expect("set the title");
            pump_answering(Duration::from_millis(400));
        }
        phases.push(focused);
    }
    say(
        "child",
        format!(
            "mode=password-title window_foreground={foreground} method={method} \
             password_edit_focused={} plain_edit_focused={}",
            phases[0], phases[1]
        ),
    );
    pump_answering(Duration::from_millis(500));
}

const ES_PASSWORD_STYLE: u32 = 0x20;

const PROTECTED_NO_ANSWER_TITLE: &str = "halvo-verify-protected-no-answer-title";
const NO_ANSWER_TITLE: &str = "halvo-verify-no-answer-title";

/// How long each wait of the row with no answer waits for its state: the
/// child for each go of the test, the test for each reading of the gate and
/// each title.
const NO_ANSWER_STATE_WAIT: Duration = Duration::from_secs(20);

/// Waits, answering this thread's messages, until the test writes the file
/// `name` in the directory of the child's own program, up to `limit`: whether
/// it came.
fn wait_for_go(name: &str, limit: Duration) -> bool {
    let program = std::env::current_exe().expect("the child's program");
    let go = program.with_file_name(name);
    let began = Instant::now();
    while !go.exists() && began.elapsed() < limit {
        pump_answering(Duration::from_millis(20));
    }
    go.exists()
}

/// A window with two boxes whose UI Automation provider gives no answer on
/// IsPassword, the second of them protected. Each step waits for the test's
/// go, which the test writes when it has read the state that the step needs:
/// the box with no answer takes the focus, then three title changes, each
/// behind the go of the one before; then the protected box, and three more.
/// The child ends at the test's last go.
fn child_no_answer_title() {
    let window = TestWindow::create(&normal_window(
        "HalvoVerifyNoAnswerTitle",
        "halvo-verify-no-answer-window",
        40,
        40,
    ))
    .expect("the window");
    no_answer_box::register().expect("the class of the boxes");
    let no_answer = no_answer_box::create(window.hwnd, false, 10).expect("the box");
    let protected = no_answer_box::create(window.hwnd, true, 50).expect("the protected box");
    let (foreground, method) = window.activate();
    let mut phases = Vec::new();
    let mut goes = Vec::new();
    for (the_box, prefix, phase) in [
        (no_answer, NO_ANSWER_TITLE, "no-answer"),
        (protected, PROTECTED_NO_ANSWER_TITLE, "protected"),
    ] {
        phases.push(no_answer_box::focus(the_box));
        goes.push(wait_for_go(phase, NO_ANSWER_STATE_WAIT));
        for step in 0..3 {
            window
                .set_title(&format!("{prefix}-{step}"))
                .expect("set the title");
            goes.push(wait_for_go(
                &format!("{phase}-{step}"),
                NO_ANSWER_STATE_WAIT,
            ));
        }
    }
    goes.push(wait_for_go("end", NO_ANSWER_STATE_WAIT));
    let (providers_handed, is_password_asked, state_asked) = no_answer_box::asked();
    say(
        "child",
        format!(
            "mode=no-answer-title window_foreground={foreground} method={method} \
             no_answer_box_focused={} protected_box_focused={} goes_seen={}/{} \
             providers_handed={providers_handed} is_password_asked={is_password_asked} \
             state_asked={state_asked}",
            phases[0],
            phases[1],
            goes.iter().filter(|seen| **seen).count(),
            goes.len()
        ),
    );
}

const OWN_WINDOW_PASSWORD_TITLE: &str = "halvo-verify-own-window-password";
const OWN_WINDOW_PLAIN_TITLE: &str = "halvo-verify-own-window-plain";

/// How long each wait of the row of a switch waits for its state: the child
/// for each go of the test, the test for each reading of the gate and each
/// title or drop.
const OWN_WINDOW_STATE_WAIT: Duration = Duration::from_secs(20);

/// The procedure of a window of the row of a switch: whenever the window is
/// activated it puts the cursor into its first child window, its one edit,
/// as a dialog does, so that the focus never rests on the window itself.
unsafe extern "system" fn own_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindow, GW_CHILD, WA_INACTIVE, WM_ACTIVATE};
    if msg == WM_ACTIVATE && (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE {
        // SAFETY: GetWindow and SetFocus take handle values of this thread's
        // windows.
        if let Ok(edit) = unsafe { GetWindow(hwnd, GW_CHILD) } {
            // SAFETY: as above.
            unsafe {
                let _ = SetFocus(Some(edit));
            }
            return LRESULT(0);
        }
    }
    // SAFETY: the arguments are the ones that the system passed.
    unsafe { test_wnd_proc(hwnd, msg, wparam, lparam) }
}

/// A hidden window of the row of a switch with one edit, a password edit
/// where `password` holds, into which its procedure puts the cursor when the
/// window is activated.
fn own_window(class: &str, title: &str, password: bool, x: i32) -> TestWindow {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_WNDPROC};
    let mut spec = normal_window(class, title, x, 40);
    spec.style = WS_OVERLAPPEDWINDOW;
    let window = TestWindow::create(&spec).expect("the window");
    let style = if password { ES_PASSWORD_STYLE } else { 0 };
    window
        .child_control("EDIT", WINDOW_STYLE(style), 10)
        .expect("the edit");
    let procedure: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT =
        own_window_proc;
    // SAFETY: the window belongs to this thread, and its new procedure has
    // the WNDPROC signature and hands every other message to the class's
    // procedure.
    unsafe {
        SetWindowLongPtrW(window.hwnd, GWLP_WNDPROC, procedure as usize as isize);
    }
    window
}

/// Three windows of the row of a switch: a lead window with no edit, one
/// whose cursor starts in a password edit, one whose cursor starts in a plain
/// edit. The lead window comes in front first, so that the child's process
/// holds the front and each later window comes in front at once, with its
/// cursor put into its edit as it is activated; then the password window,
/// the plain one and the password window again, each behind the test's go
/// for the step before.
fn child_own_window() {
    let lead = TestWindow::create(&normal_window(
        "HalvoVerifyOwnWindowLead",
        "halvo-verify-own-window-lead",
        40,
        320,
    ))
    .expect("the lead window");
    let password = own_window(
        "HalvoVerifyOwnWindowPassword",
        OWN_WINDOW_PASSWORD_TITLE,
        true,
        40,
    );
    let plain = own_window(
        "HalvoVerifyOwnWindowPlain",
        OWN_WINDOW_PLAIN_TITLE,
        false,
        420,
    );
    let mut fronts = Vec::new();
    let mut goes = Vec::new();
    for (window, go) in [
        (&lead, "lead"),
        (&password, "password-first"),
        (&plain, "plain"),
        (&password, "password-second"),
    ] {
        fronts.push(window.activate());
        goes.push(wait_for_go(go, OWN_WINDOW_STATE_WAIT));
    }
    say(
        "child",
        format!(
            "mode=own-window window_foreground={}/{} method={} goes_seen={}/{}",
            fronts.iter().filter(|(foreground, _)| *foreground).count(),
            fronts.len(),
            fronts
                .iter()
                .map(|(_, method)| *method)
                .collect::<Vec<_>>()
                .join(","),
            goes.iter().filter(|seen| **seen).count(),
            goes.len()
        ),
    );
}

/// A child window of the test's own class whose UI Automation provider gives
/// no answer on IsPassword, the property that the gate reads; a protected box
/// also serves a LegacyIAccessible pattern whose State holds
/// STATE_SYSTEM_PROTECTED. The providers are built by hand from the windows
/// crate's vtables, as tests/windows_prints.rs builds its event sinks; they
/// live as long as the child process, so they keep no count of references.
mod no_answer_box {
    use core::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, PoisonError};

    use windows::core::{Interface, GUID, HRESULT, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{
        E_NOINTERFACE, E_NOTIMPL, E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, S_OK, WPARAM,
    };
    use windows::Win32::System::Com::SAFEARRAY;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{
        ILegacyIAccessibleProvider, ILegacyIAccessibleProvider_Vtbl, IRawElementProviderSimple,
        IRawElementProviderSimple_Vtbl, ProviderOptions, ProviderOptions_ServerSideProvider,
        UIA_AutomationFocusChangedEventId, UIA_IsPasswordPropertyId,
        UIA_LegacyIAccessiblePatternId, UiaHostProviderFromHwnd, UiaRaiseAutomationEvent,
        UiaReturnRawElementProvider, UiaRootObjectId, ROLE_SYSTEM_TEXT, UIA_PATTERN_ID,
        UIA_PROPERTY_ID,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, STATE_SYSTEM_PROTECTED, WINDOW_EX_STYLE,
        WM_GETOBJECT, WNDCLASSW, WS_BORDER, WS_CHILD, WS_VISIBLE,
    };

    const CLASS: &str = "HalvoVerifyNoAnswerBox";

    /// How often this process handed UI Automation the provider of a box.
    static PROVIDERS_HANDED: AtomicU32 = AtomicU32::new(0);
    /// How often UI Automation asked a box for IsPassword.
    static IS_PASSWORD_ASKED: AtomicU32 = AtomicU32::new(0);
    /// How often UI Automation asked a protected box for its State.
    static STATE_ASKED: AtomicU32 = AtomicU32::new(0);

    /// The three counts: providers handed, IsPassword asked, State asked.
    pub(super) fn asked() -> (u32, u32, u32) {
        (
            PROVIDERS_HANDED.load(Ordering::Relaxed),
            IS_PASSWORD_ASKED.load(Ordering::Relaxed),
            STATE_ASKED.load(Ordering::Relaxed),
        )
    }

    /// The boxes of this process: each window handle with its provider.
    static BOXES: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

    #[repr(C)]
    struct Simple {
        vtable: *const IRawElementProviderSimple_Vtbl,
        hwnd: HWND,
        legacy: *mut c_void,
    }

    #[repr(C)]
    struct Legacy {
        vtable: *const ILegacyIAccessibleProvider_Vtbl,
    }

    static SIMPLE_VTABLE: IRawElementProviderSimple_Vtbl = IRawElementProviderSimple_Vtbl {
        base__: windows::core::IUnknown_Vtbl {
            QueryInterface: simple_query,
            AddRef: kept,
            Release: kept,
        },
        ProviderOptions: options,
        GetPatternProvider: pattern,
        GetPropertyValue: property,
        HostRawElementProvider: host,
    };

    static LEGACY_VTABLE: ILegacyIAccessibleProvider_Vtbl = ILegacyIAccessibleProvider_Vtbl {
        base__: windows::core::IUnknown_Vtbl {
            QueryInterface: legacy_query,
            AddRef: kept,
            Release: kept,
        },
        Select: select,
        DoDefaultAction: default_action,
        SetValue: set_value,
        GetIAccessible: no_object,
        ChildId: child_id,
        Name: no_object,
        Value: no_object,
        Description: no_object,
        Role: role,
        State: state,
        Help: no_object,
        KeyboardShortcut: no_object,
        GetSelection: selection,
        DefaultAction: no_object,
    };

    /// Registers the class of the boxes, once in a process.
    pub(super) fn register() -> Result<(), String> {
        let name = HSTRING::from(CLASS);
        let class = WNDCLASSW {
            lpfnWndProc: Some(box_proc),
            hInstance: module()?,
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `class` points at `name`, which lives across the call; the
        // system copies the name, and the class lives as long as the process.
        if unsafe { RegisterClassW(&class) } == 0 {
            return Err(format!(
                "RegisterClassW: {}",
                windows::core::Error::from_thread()
            ));
        }
        Ok(())
    }

    fn module() -> Result<HINSTANCE, String> {
        // SAFETY: a null module name asks for the handle of this executable,
        // which stays loaded for the life of the process.
        unsafe { GetModuleHandleW(None) }
            .map(Into::into)
            .map_err(|e| format!("GetModuleHandleW: {e}"))
    }

    /// A box at `y` inside `parent`, a window of this thread; a protected box
    /// serves the LegacyIAccessible pattern with the protected State.
    pub(super) fn create(parent: HWND, protected: bool, y: i32) -> Result<HWND, String> {
        let class = HSTRING::from(CLASS);
        // SAFETY: the class name points at a live HSTRING for the call, the
        // class is registered, and the parent is a window of this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_BORDER,
                10,
                y,
                300,
                24,
                Some(parent),
                None,
                Some(module()?),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW: {e}"))?;
        let legacy = if protected {
            Box::into_raw(Box::new(Legacy {
                vtable: &LEGACY_VTABLE,
            }))
            .cast::<c_void>()
        } else {
            core::ptr::null_mut()
        };
        let simple = Box::into_raw(Box::new(Simple {
            vtable: &SIMPLE_VTABLE,
            hwnd,
            legacy,
        }));
        BOXES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((hwnd.0 as usize, simple as usize));
        Ok(hwnd)
    }

    /// Moves the focus to the box `hwnd` of this thread and raises UI
    /// Automation's focus event for its provider: whether the box has the
    /// focus.
    pub(super) fn focus(hwnd: HWND) -> bool {
        // SAFETY: SetFocus and GetFocus take and return handle values of this
        // thread's windows.
        let focused = unsafe {
            let _ = SetFocus(Some(hwnd));
            GetFocus() == hwnd
        };
        if let Some(provider) = provider_of(hwnd) {
            // SAFETY: the provider lives as long as the process.
            let _ =
                unsafe { UiaRaiseAutomationEvent(&provider, UIA_AutomationFocusChangedEventId) };
        }
        focused
    }

    fn provider_of(hwnd: HWND) -> Option<IRawElementProviderSimple> {
        let raw = BOXES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(window, _)| *window == hwnd.0 as usize)
            .map(|(_, raw)| *raw)?;
        // SAFETY: `raw` points at a Simple of this module, which lives as long
        // as the process and whose first field is the vtable of the interface;
        // it keeps no count of references, so the value made here owns none.
        Some(unsafe { IRawElementProviderSimple::from_raw(raw as *mut c_void) })
    }

    unsafe extern "system" fn box_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_GETOBJECT && lparam.0 as i32 == UiaRootObjectId {
            if let Some(provider) = provider_of(hwnd) {
                PROVIDERS_HANDED.fetch_add(1, Ordering::Relaxed);
                // SAFETY: the arguments are the ones that the system passed,
                // and the provider lives as long as the process.
                return unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, &provider) };
            }
        }
        // SAFETY: the arguments are the ones that the system passed.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn answer(this: *mut c_void, iid: *const GUID, out: *mut *mut c_void, own: GUID) -> HRESULT {
        if iid.is_null() || out.is_null() {
            return E_POINTER;
        }
        // SAFETY: the system passes a live IID and a writable out pointer;
        // the objects of this module keep no count of references.
        unsafe {
            if *iid == windows::core::IUnknown::IID || *iid == own {
                *out = this;
                S_OK
            } else {
                *out = core::ptr::null_mut();
                E_NOINTERFACE
            }
        }
    }

    unsafe extern "system" fn simple_query(
        this: *mut c_void,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        answer(this, iid, out, IRawElementProviderSimple::IID)
    }

    unsafe extern "system" fn legacy_query(
        this: *mut c_void,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        answer(this, iid, out, ILegacyIAccessibleProvider::IID)
    }

    unsafe extern "system" fn kept(_this: *mut c_void) -> u32 {
        1
    }

    unsafe extern "system" fn options(_this: *mut c_void, out: *mut ProviderOptions) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: the system passes a writable out pointer.
        unsafe { out.write(ProviderOptions_ServerSideProvider) };
        S_OK
    }

    unsafe extern "system" fn pattern(
        this: *mut c_void,
        id: UIA_PATTERN_ID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `this` is a live Simple of this module, and the system
        // passes a writable out pointer.
        unsafe {
            let simple = &*(this as *const Simple);
            out.write(if id == UIA_LegacyIAccessiblePatternId {
                simple.legacy
            } else {
                core::ptr::null_mut()
            });
        }
        S_OK
    }

    /// No property has an answer of the box's own: the system's host provider
    /// of its window gives the others, and nothing gives IsPassword.
    unsafe extern "system" fn property(
        _this: *mut c_void,
        id: UIA_PROPERTY_ID,
        out: *mut VARIANT,
    ) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        if id == UIA_IsPasswordPropertyId {
            IS_PASSWORD_ASKED.fetch_add(1, Ordering::Relaxed);
        }
        // SAFETY: the system passes a writable out pointer; an empty VARIANT
        // says that the provider gives no value.
        unsafe { out.write(VARIANT::default()) };
        S_OK
    }

    unsafe extern "system" fn host(this: *mut c_void, out: *mut *mut c_void) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: `this` is a live Simple of this module whose handle is a
        // window of this process, and the system passes a writable out
        // pointer, which takes the one reference of the host provider.
        unsafe {
            let simple = &*(this as *const Simple);
            match UiaHostProviderFromHwnd(simple.hwnd) {
                Ok(host) => {
                    out.write(host.into_raw());
                    S_OK
                }
                Err(error) => {
                    out.write(core::ptr::null_mut());
                    error.code()
                }
            }
        }
    }

    unsafe extern "system" fn select(_this: *mut c_void, _flags: i32) -> HRESULT {
        E_NOTIMPL
    }

    unsafe extern "system" fn default_action(_this: *mut c_void) -> HRESULT {
        E_NOTIMPL
    }

    unsafe extern "system" fn set_value(_this: *mut c_void, _value: PCWSTR) -> HRESULT {
        E_NOTIMPL
    }

    unsafe extern "system" fn no_object(_this: *mut c_void, out: *mut *mut c_void) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: the system passes a writable out pointer; a null string or
        // object is the empty answer.
        unsafe { out.write(core::ptr::null_mut()) };
        S_OK
    }

    unsafe extern "system" fn child_id(_this: *mut c_void, out: *mut i32) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: the system passes a writable out pointer; 0 is CHILDID_SELF.
        unsafe { out.write(0) };
        S_OK
    }

    unsafe extern "system" fn role(_this: *mut c_void, out: *mut u32) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: the system passes a writable out pointer.
        unsafe { out.write(ROLE_SYSTEM_TEXT) };
        S_OK
    }

    unsafe extern "system" fn state(_this: *mut c_void, out: *mut u32) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        STATE_ASKED.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the system passes a writable out pointer.
        unsafe { out.write(STATE_SYSTEM_PROTECTED) };
        S_OK
    }

    unsafe extern "system" fn selection(_this: *mut c_void, out: *mut *mut SAFEARRAY) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        // SAFETY: the system passes a writable out pointer.
        unsafe { out.write(core::ptr::null_mut()) };
        E_NOTIMPL
    }
}

struct ChildProcess {
    process: std::process::Child,
    output: Option<std::thread::JoinHandle<String>>,
    _dir: tempfile::TempDir,
}

impl ChildProcess {
    fn id(&self) -> u32 {
        self.process.id()
    }

    fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
        self.process.try_wait().expect("poll the child")
    }

    fn wait(&mut self) -> std::process::ExitStatus {
        self.process.wait().expect("wait for the child")
    }

    /// Writes the go `name` beside the child's program, where a child that
    /// waits by `wait_for_go` reads it.
    fn go(&self, name: &str) {
        std::fs::write(self._dir.path().join(name), b"").expect("write a go of the child");
    }

    fn print_output(&mut self) {
        let Some(reader) = self.output.take() else {
            return;
        };
        let text = reader.join().unwrap_or_default();
        for line in text.lines() {
            println!("child: {line}");
        }
    }
}

fn scratch_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("tf-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("temp dir under CARGO_TARGET_TMPDIR")
}

fn spawn_child(mode: &str, secs: u64) -> ChildProcess {
    spawn_child_as(mode, secs, CHILD_EXE_NAME)
}

fn spawn_child_as(mode: &str, secs: u64, exe_name: &str) -> ChildProcess {
    let dir = scratch_dir();
    let exe = dir.path().join(exe_name);
    std::fs::copy(std::env::current_exe().expect("current exe"), &exe)
        .expect("copy the test binary");
    let mut process = Command::new(&exe)
        .args(["helper_child_window", "--exact", "--ignored", "--nocapture"])
        .env(CHILD_SECS_ENV, secs.to_string())
        .env(CHILD_MODE_ENV, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the child");
    let mut stdout = process.stdout.take().expect("the child's stdout");
    let output = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    ChildProcess {
        process,
        output: Some(output),
        _dir: dir,
    }
}

struct WindowScenario {
    gate_read_clear: bool,
    foreground_start: Result<(), String>,
    title_start: Result<(), String>,
    own_window_was_foreground: bool,
    own_events: Vec<Seen>,
    own_exe_file: String,
    child_was_foreground: bool,
    last_app_was_child: bool,
    child_exit_ok: bool,
    child_events: Vec<Seen>,
}

fn window_scenario(test: &'static str) -> WindowScenario {
    env_report(test);
    let (sink, events) = capture(8192);
    let mut foreground = ForegroundAppSource::new();
    let last_app = foreground.last_app();
    let set = ServiceSet::new();
    let gate = set.password_gate();
    let mut title = set.gated_title(WindowTitleSource::new());
    let foreground_start = foreground
        .begin(SourceContext::new(sink.clone()))
        .map(|_| ())
        .map_err(|e| e.to_string());
    let title_start = title
        .begin(SourceContext::new(sink.clone()))
        .map(|_| ())
        .map_err(|e| e.to_string());
    say(
        test,
        format!(
            "foreground_start={} title_start={}",
            show(&foreground_start),
            show(&title_start)
        ),
    );

    summarize(
        test,
        "ambient-1s",
        &events.drain_for(Duration::from_secs(1)),
    );

    let own_exe_file = own_exe_file();
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
    let own_events = events.drain_for(Duration::from_secs(1));
    say(
        test,
        format!(
            "phase=own-process-window was_foreground={own_window_was_foreground} \
             method={own_method}"
        ),
    );
    summarize(test, "own-process-window", &own_events);

    let mut child = spawn_child("cycle", 4);
    let child_pid = child.id();
    let gate_read_clear = content_reads_clear(test, &gate, GATE_CLEAR_WAIT);
    let mut child_events = Vec::new();
    let mut child_was_foreground = false;
    let mut last_app_was_child = false;
    let exit = loop {
        child_events.extend(events.drain_for(Duration::from_millis(100)));
        child_was_foreground |= foreground_pid() == child_pid;
        if let Ok(cell) = last_app.lock() {
            last_app_was_child |= cell.as_deref().is_some_and(is_child_app);
        }
        if let Some(status) = child.try_wait() {
            break status;
        }
    };
    child_events.extend(events.drain_for(Duration::from_secs(1)));
    let _ = stop_counted(&mut foreground, "foreground");
    let _ = stop_counted(&mut title, "window_title");
    gate_readings(test, &gate);
    child.print_output();
    say(
        test,
        format!(
            "phase=child-process-window child_exit_ok={} os_reported_child_foreground={child_was_foreground} \
             last_app_cell_was_child={last_app_was_child}",
            exit.success()
        ),
    );
    summarize(test, "child-process-window", &child_events);

    WindowScenario {
        gate_read_clear,
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
        source_built_by_test("c1", "keyboard");
        let (sink, events) = capture(4096);
        let last_app = Arc::new(Mutex::new(Some(ATTRIBUTION.to_string())));
        let _fields = FieldWindow::open(
            "c1_raw_input_delivers_keystrokes_and_pointer_events",
            Field::Plain,
        );
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app), PasswordGate::new());
        let mut pointer = RawInputSource::pointer(Arc::clone(&last_app));
        let keyboard_start = keyboard.begin(SourceContext::new(sink.clone()));
        let pointer_start = pointer.begin(SourceContext::new(sink.clone()));
        say(
            "c1",
            format!(
                "keyboard_start={} pointer_start={}",
                show(&keyboard_start),
                show(&pointer_start)
            ),
        );

        let quiet = events.drain_for(Duration::from_secs(1));
        summarize("c1", "no-input-1s", &quiet);

        let key_events_accepted = type_letters(PRESSES);
        let after_keys = events.drain_for(Duration::from_millis(1500));
        summarize("c1", "after-key-presses", &after_keys);

        let _ = point_at_front();
        let move_events_accepted = move_pointer(MOVES);
        let after_moves = events.drain_for(Duration::from_millis(1500));
        summarize("c1", "after-pointer-moves", &after_moves);

        let _ = stop_counted(&mut keyboard, "keyboard");
        let _ = stop_counted(&mut pointer, "pointer");
        type_letters(3);
        move_pointer(3);
        let after_stop = events.drain_for(Duration::from_millis(700));
        summarize("c1", "after-stop", &after_stop);

        let keystrokes = count(&after_keys, Modality::Keystroke);
        let pointers = count(&after_moves, Modality::Pointer);
        let front_app = after_keys
            .first()
            .map(|e| e.foreground_app.clone())
            .unwrap_or_default();
        let attributed = !front_app.is_empty()
            && after_keys
                .iter()
                .chain(&after_moves)
                .all(|e| e.foreground_app == front_app);
        say(
            "c1",
            format!(
                "key_presses_sent={PRESSES} sendinput_key_events_accepted={key_events_accepted} \
                 keystroke_events={keystrokes} pointer_moves_sent={MOVES} \
                 sendinput_move_events_accepted={move_events_accepted} pointer_events={pointers} \
                 events_before_input={} events_after_stop={} attributed_to_one_app_in_front={attributed}",
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
        assert!(pointers >= 1, "no pointer event for the moves");
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
        source_built_by_test("c2", "screen");
        let _fields = FieldWindow::open("c2_screen_source_delivers_a_frame", Field::Plain);
        let (sink, events) = capture(1024);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut screen = set.gated_screen(ScreenSource::new(Arc::new(Mutex::new(None))));
        let started = Instant::now();
        let start = screen.begin(screen_on(sink.clone()));
        say(
            "c2",
            format!(
                "screen_start={} start_ms={}",
                show(&start),
                started.elapsed().as_millis()
            ),
        );
        assert!(start.is_ok(), "the screen source must start");
        let clear = content_reads_clear("c2", &gate, GATE_CLEAR_WAIT);

        let first = events.first_within(FIRST_FRAME_WAIT);
        say(
            "c2",
            format!(
                "first_frame_within_{}s={} first_frame_ms={}",
                FIRST_FRAME_WAIT.as_secs(),
                first.is_some(),
                started.elapsed().as_millis()
            ),
        );
        let mut idle: Vec<Seen> = first.into_iter().collect();
        idle.extend(events.drain_for(Duration::from_secs(4)));
        summarize("c2", "unchanged-screen", &idle);

        let window = TestWindow::create(&WindowSpec {
            ex_style: WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            ..magenta_popup("HalvoVerifyFrames", "halvo-verify-frames")
        })
        .expect("moving window");
        for step in 0..20 {
            window.move_to(100 + step * 20, 100 + step * 10);
            pump_thread(Duration::from_millis(250));
        }
        drop(window);
        let moving = events.drain_for(Duration::from_millis(700));
        summarize("c2", "moving-window-5s", &moving);

        let _ = stop_counted(&mut screen, "screen");
        let after_stop = events.drain_for(Duration::from_millis(1500));
        let frames: Vec<&Seen> = idle
            .iter()
            .chain(&moving)
            .filter(|e| e.modality == Modality::ScreenFrame)
            .collect();
        say(
            "c2",
            format!(
                "frames_unchanged_screen={} frames_moving_window={} frames_after_stop={} \
                 frames_with_an_app={} dropped={}",
                count(&idle, Modality::ScreenFrame),
                count(&moving, Modality::ScreenFrame),
                after_stop.len(),
                frames
                    .iter()
                    .filter(|e| !e.foreground_app.is_empty())
                    .count(),
                sink.dropped_count(),
            ),
        );
        gate_readings("c2", &gate);
        assert!(clear, "the gate never read Clear");
        assert!(!frames.is_empty(), "no frame of the desktop in 15 seconds");
        assert!(
            frames.iter().all(|e| e.view.is_some()),
            "a frame event is no record of the floor"
        );
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
        let distinct_sizes: std::collections::BTreeSet<u64> = child_titles
            .iter()
            .copied()
            .filter(|len| CHILD_TITLE_LENGTHS.contains(len))
            .collect();
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
                 distinct_scripted_title_sizes={} title_events_for_own_process_window={own_titles}",
                child_titles.len(),
                distinct_sizes.len(),
            ),
        );
        assert!(
            seen.title_start.is_ok(),
            "the window-title source must start"
        );
        assert!(seen.child_exit_ok, "the child process must finish");
        assert!(seen.gate_read_clear, "the gate never read Clear");
        assert!(
            expected_sizes >= 1,
            "no window-title event for the child's title changes"
        );
        assert!(
            distinct_sizes.len() >= 2,
            "the events must follow more than one title change"
        );
        assert!(
            distinct_sizes
                .iter()
                .any(|len| CHILD_TITLE_SIZES_NO_SWITCH_SHOWS.contains(len)),
            "no event for a title that only a title change shows"
        );
    });
}

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_WOW: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
const PROBE_KEY: &str = "HalvoVerifyProbe";
const FIRST_SUBKEYS_SHOWN: usize = 20;

struct PerUserView {
    view: &'static str,
    exists: bool,
    subkeys: Option<u32>,
    values: Option<u32>,
    first_subkeys: Vec<String>,
}

impl PerUserView {
    fn holds_nothing(&self) -> bool {
        !self.exists || (self.subkeys == Some(0) && self.values == Some(0))
    }

    fn line(&self) -> String {
        let count = |n: Option<u32>| n.map_or_else(|| "unread".to_string(), |n| n.to_string());
        format!(
            "per_user_uninstall_key={} exists={} subkeys={} values={} first_subkeys={:?}",
            self.view,
            self.exists,
            count(self.subkeys),
            count(self.values),
            self.first_subkeys
        )
    }
}

fn per_user_uninstall_views(hkcu: &winreg::RegKey) -> [PerUserView; 2] {
    use winreg::enums::KEY_READ;

    [("native", UNINSTALL), ("wow6432node", UNINSTALL_WOW)].map(|(view, path)| {
        match hkcu.open_subkey_with_flags(path, KEY_READ) {
            Ok(key) => {
                let info = key.query_info().ok();
                PerUserView {
                    view,
                    exists: true,
                    subkeys: info.as_ref().map(|info| info.sub_keys),
                    values: info.as_ref().map(|info| info.values),
                    first_subkeys: key
                        .enum_keys()
                        .take(FIRST_SUBKEYS_SHOWN)
                        .flatten()
                        .collect(),
                }
            }
            Err(_) => PerUserView {
                view,
                exists: false,
                subkeys: Some(0),
                values: Some(0),
                first_subkeys: Vec::new(),
            },
        }
    })
}

fn per_user_key_absent_or_removed(test: &str, hkcu: &winreg::RegKey) -> bool {
    use winreg::enums::KEY_READ;

    let views = per_user_uninstall_views(hkcu);
    for view in &views {
        say(test, view.line());
    }
    let key_existed = hkcu.open_subkey_with_flags(UNINSTALL, KEY_READ).is_ok();
    say(test, format!("hkcu_uninstall_key_existed={key_existed}"));
    if !key_existed {
        return true;
    }
    if !views.iter().all(PerUserView::holds_nothing) {
        say(
            test,
            "hkcu_uninstall_key_existed=true empty=false removed=false",
        );
        return false;
    }
    let deleted = hkcu.delete_subkey(UNINSTALL);
    let removed = deleted.is_ok() && hkcu.open_subkey_with_flags(UNINSTALL, KEY_READ).is_err();
    say(
        test,
        format!(
            "hkcu_uninstall_key_existed=true empty=true removed={removed} delete={}",
            show(&deleted)
        ),
    );
    removed
}

/// Puts the per-user Uninstall key of each registry view back as its test
/// found it, in every exit of the test, a panic among them: a key that stood
/// and is gone is made again, empty, and a key that did not stand and holds
/// nothing is removed. A key that holds something is never touched.
struct PerUserKeyRestore {
    test: &'static str,
    hkcu: winreg::RegKey,
    stood: [bool; 2],
}

impl Drop for PerUserKeyRestore {
    fn drop(&mut self) {
        let paths = [UNINSTALL, UNINSTALL_WOW];
        let now = per_user_uninstall_views(&self.hkcu);
        for ((path, stood), view) in paths.into_iter().zip(self.stood).zip(&now) {
            let done = match (stood, view.exists) {
                (true, false) => Some(("made_again", self.hkcu.create_subkey(path).map(|_| ()))),
                (false, true) if view.holds_nothing() => {
                    Some(("removed", self.hkcu.delete_subkey(path)))
                }
                _ => None,
            };
            if let Some((what, result)) = done {
                say(
                    self.test,
                    format!(
                        "restore per_user_uninstall_key={} {what}={}",
                        view.view,
                        show(&result)
                    ),
                );
            }
        }
    }
}

/// JB-28 (1), ruling W3-27 (3) (a): c5-watch's precondition removes an empty
/// per-user Uninstall key and goes on, and leaves a key that holds something.
/// The test makes the empty key when it is absent, and its guard puts both
/// views back as it found them.
#[test]
#[ignore = "functional: writes the per-user Uninstall key of the Windows registry; 50-windows-functional runs every test of this binary under --ignored, so a test that is not ignored here runs zero tests there, and under 12-test's cargo test it would write the registry of every machine that runs the workspace's tests"]
fn c5_watch_precondition_removes_an_empty_per_user_key_and_goes_on() {
    const TEST: &str = "c5-precondition";
    with_deadline(TEST, Duration::from_secs(60), || {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let found = per_user_uninstall_views(&hkcu);
        for view in &found {
            say(TEST, format!("found {}", view.line()));
        }
        let restore = PerUserKeyRestore {
            test: TEST,
            hkcu: RegKey::predef(HKEY_CURRENT_USER),
            stood: found.each_ref().map(|view| view.exists),
        };
        let counts = |views: &[PerUserView; 2]| {
            views
                .each_ref()
                .map(|view| (view.exists, view.subkeys, view.values))
        };
        if !found.iter().all(PerUserView::holds_nothing) {
            let went_on = per_user_key_absent_or_removed(TEST, &hkcu);
            let left = per_user_uninstall_views(&hkcu);
            say(
                TEST,
                format!("held_something=true precondition_went_on={went_on}"),
            );
            assert!(
                !went_on,
                "the precondition went on past a per-user Uninstall key that holds something"
            );
            assert_eq!(
                counts(&left),
                counts(&found),
                "the precondition changed a per-user Uninstall key that holds something"
            );
            drop(restore);
            return;
        }
        if !found[0].exists {
            hkcu.create_subkey(UNINSTALL)
                .expect("make the empty per-user Uninstall key");
        }
        let made = per_user_uninstall_views(&hkcu);
        for view in &made {
            say(TEST, format!("made {}", view.line()));
        }
        assert!(
            made[0].exists && made.iter().all(PerUserView::holds_nothing),
            "the per-user Uninstall key is not there and empty before the precondition"
        );
        let went_on = per_user_key_absent_or_removed(TEST, &hkcu);
        let gone = hkcu.open_subkey_with_flags(UNINSTALL, KEY_READ).is_err();
        say(
            TEST,
            format!("held_something=false precondition_went_on={went_on} key_gone={gone}"),
        );
        assert!(
            went_on,
            "the precondition did not go on past an empty per-user Uninstall key"
        );
        assert!(
            gone,
            "the precondition went on and left the empty per-user Uninstall key"
        );
        drop(restore);
    });
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
        for view in per_user_uninstall_views(&hkcu) {
            say("c5", view.line());
        }

        let before = scan_installed().expect("scan the registry");
        let mut sorted = before.apps().to_vec();
        sorted.sort();
        sorted.dedup();
        say(
            "c5",
            format!(
                "entries={} with_version={} with_publisher={} sorted_and_deduplicated={}",
                before.len(),
                before.apps().iter().filter(|a| a.version.is_some()).count(),
                before
                    .apps()
                    .iter()
                    .filter(|a| a.publisher.is_some())
                    .count(),
                sorted.as_slice() == before.apps(),
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
                .map(|name| before.apps().iter().any(|app| &app.name == name));
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
            !before.apps().iter().any(|a| a.name == probe_name),
            "the probe entry must not exist yet"
        );

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

        let with_probe = scan_installed().expect("scan with the probe");
        let found = with_probe.apps().iter().find(|a| a.name == probe_name);
        let found_ok = found.is_some_and(|app| {
            app.version.as_deref() == Some("9.9.9")
                && app.publisher.as_deref() == Some("halvo-verifier")
        });
        say(
            "c5",
            format!(
                "hkcu_uninstall_key_existed={hkcu_key_existed} probe_entry_found={} \
                 probe_version_and_publisher_ok={found_ok} entries={}",
                found.is_some(),
                with_probe.len(),
            ),
        );

        probe
            .set_value("SystemComponent", &1u32)
            .expect("probe system component");
        let hidden = scan_installed().expect("scan with the hidden probe");
        let system_component_hidden = !hidden.apps().iter().any(|a| a.name == probe_name);

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
                !after.apps().iter().any(|a| a.name == probe_name),
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
            found_ok,
            "the scan must return the probe with its version and publisher"
        );
        assert!(system_component_hidden, "a system component must be hidden");
        assert!(after == before, "the scan must return to its first result");
    });
}

fn display_affinity(hwnd: HWND) -> Result<u32, String> {
    let mut affinity = 0u32;
    // SAFETY: `affinity` is a live u32 that the call writes into.
    unsafe { GetWindowDisplayAffinity(hwnd, &mut affinity) }
        .map(|()| affinity)
        .map_err(|e| e.to_string())
}

fn exclude_from_capture(hwnd: HWND) -> Result<(), String> {
    // SAFETY: SetWindowDisplayAffinity takes a window handle and a flag by
    // value; a null or foreign handle makes the call fail.
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }.map_err(|e| e.to_string())
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn c8_display_affinity_exclude_from_capture_is_set_and_read_back() {
    with_deadline("c8", Duration::from_secs(60), || {
        env_report("c8");
        let window =
            TestWindow::create(&magenta_popup("HalvoVerifyExclude", "halvo-verify-exclude"))
                .expect("window");
        pump_thread(Duration::from_millis(300));
        let before = display_affinity(window.hwnd);
        let exclude = exclude_from_capture(window.hwnd);
        let after = display_affinity(window.hwnd);
        let null_window = exclude_from_capture(HWND(std::ptr::null_mut()));
        say(
            "c8",
            format!(
                "affinity_before={before:?} set_display_affinity={} affinity_after={after:?} \
                 wda_excludefromcapture={} set_display_affinity_null_window={}",
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

fn memory() -> PROCESS_MEMORY_COUNTERS {
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    // SAFETY: `counters` is a live PROCESS_MEMORY_COUNTERS and the size
    // argument is its size; the pseudo handle of this process needs no close.
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

fn bound(key: &str) -> Result<u64, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(BOUNDS_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read {BOUNDS_FILE}: {e}"))?;
    let prefix = format!("{key}=");
    let values: Vec<&str> = text
        .lines()
        .filter_map(|line| line.trim_end_matches('\r').strip_prefix(prefix.as_str()))
        .collect();
    match values.as_slice() {
        [value] => value
            .trim()
            .parse()
            .map_err(|e| format!("{key} in {BOUNDS_FILE} is not a count: {e}")),
        [] => Err(format!("{BOUNDS_FILE} holds no line {key}=")),
        _ => Err(format!("{BOUNDS_FILE} holds {} lines {key}=", values.len())),
    }
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop; runs for 60 seconds"]
fn c9_peak_working_set_of_sources_1_to_4_for_60_seconds() {
    with_deadline("c9", Duration::from_secs(240), || {
        const RUN: Duration = Duration::from_secs(60);
        env_report("c9");
        source_built_by_test("c9", "keyboard");
        source_built_by_test("c9", "screen");
        let rss_max_bytes = bound("rss_max_bytes");
        say(
            "c9",
            format!(
                "build_profile={} rss_max_bytes={}",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                },
                show(&rss_max_bytes),
            ),
        );
        let rss_max_bytes = rss_max_bytes.expect("the bound rss_max_bytes");
        say("c9", memory_line("before-sources", &memory()));

        let (sink, events) = capture(8192);
        let mut foreground = ForegroundAppSource::new();
        let last_app = foreground.last_app();
        let _fields = FieldWindow::open(
            "c9_peak_working_set_of_sources_1_to_4_for_60_seconds",
            Field::Plain,
        );
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app), PasswordGate::new());
        let gate = keyboard
            .password_gate()
            .expect("the keyboard source's gate");
        let mut title = WindowTitleSource::new().with_password_gate(Arc::clone(&gate));
        let mut pointer = RawInputSource::pointer(Arc::clone(&last_app));
        let mut screen =
            ScreenSource::new(Arc::clone(&last_app)).with_password_gate(Arc::clone(&gate));
        let starts = [
            (
                "foreground",
                foreground.begin(SourceContext::new(sink.clone())),
            ),
            (
                "window_title",
                title.begin(SourceContext::new(sink.clone())),
            ),
            ("keyboard", {
                let _ = seam::take();
                keyboard.begin(SourceContext::new(sink.clone()))
            }),
            ("pointer", pointer.begin(SourceContext::new(sink.clone()))),
            ("screen", screen.begin(screen_on(sink.clone()))),
        ];
        for (name, result) in &starts {
            say("c9", format!("source={name} start={}", show(result)));
        }
        let all_started = starts.iter().all(|(_, result)| result.is_ok());
        say("c9", memory_line("sources-started", &memory()));
        let clear = content_reads_clear("c9", &gate, GATE_CLEAR_WAIT);

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
            for event in events.drain_for(Duration::from_millis(80)) {
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

        let _ = stop_counted(&mut foreground, "foreground");
        let _ = stop_counted(&mut title, "window_title");
        let _ = stop_counted(&mut keyboard, "keyboard");
        let _ = stop_counted(&mut pointer, "pointer");
        let _ = stop_counted(&mut screen, "screen");
        let child_exit = child.wait();
        child.print_output();
        for event in events.drain_for(Duration::from_millis(500)) {
            *counts.entry(format!("{:?}", event.modality)).or_default() += 1;
        }
        say("c9", memory_line("sources-stopped", &memory()));
        gate_readings("c9", &gate);
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
        let peak = end.PeakWorkingSetSize as u64;
        say(
            "c9",
            format!(
                "result all_sources_started={all_started} run_secs={} working_set_samples={samples} \
                 highest_sampled_working_set_bytes={highest_working_set} \
                 peak_working_set_bytes={peak} peak_private_bytes={} \
                 rss_max_bytes={rss_max_bytes} peak_within_bound={}",
                RUN.as_secs(),
                end.PeakPagefileUsage,
                peak <= rss_max_bytes,
            ),
        );
        assert!(
            all_started,
            "INCONCLUSIVE: not every source started, so the number is not for five sources"
        );
        assert!(clear, "the gate never read Clear");
        let keys_decided = seam::take().len();
        say("c9", format!("keys_that_the_gate_decided={keys_decided}"));
        assert!(
            keys_decided > 0,
            "INCONCLUSIVE: the keyboard source decided no key in the run, so it did no work"
        );
        for kind in ["ForegroundApp", "Pointer", "ScreenFrame", "WindowTitle"] {
            assert!(
                counts.get(kind).copied().unwrap_or(0) > 0,
                "INCONCLUSIVE: no {kind} event in the run, so that source did no work"
            );
        }
        assert!(
            peak <= rss_max_bytes,
            "the peak working set is above rss_max_bytes"
        );
    });
}

fn unix_millis(time: SystemTime) -> u128 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn tick_period() -> (u128, &'static str) {
    let (mut adjustment, mut increment, mut disabled) = (0u32, 0u32, 0i32);
    // SAFETY: the three pointers are live locals of the types that the call
    // writes.
    let read = unsafe { GetSystemTimeAdjustment(&mut adjustment, &mut increment, &mut disabled) };
    match period_ms(increment) {
        Some(period) if read != 0 => (period, "system"),
        _ => (TICK_PERIOD_FALLBACK_MS, "fallback"),
    }
}

fn integrity_is_medium() -> bool {
    Command::new(r"C:\Windows\System32\whoami.exe")
        .arg("/groups")
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|line| line.contains(r"Mandatory Label\Medium Mandatory Level"))
        })
        .unwrap_or(false)
}

struct RestrictedProcess {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

impl RestrictedProcess {
    fn wait(&self, millis: u32) -> Option<u32> {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
        let mut code = u32::MAX;
        // SAFETY: `handle` is the open process handle that CreateProcessAsUserW
        // gave this value; Drop closes it, after this call.
        unsafe {
            if WaitForSingleObject(self.handle, millis) != WAIT_OBJECT_0 {
                return None;
            }
            GetExitCodeProcess(self.handle, &mut code);
        }
        Some(code)
    }
}

impl Drop for RestrictedProcess {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::TerminateProcess;
        // SAFETY: `handle` is the open process handle of this value; ending a
        // process that has already exited fails and changes nothing, and the
        // handle is closed once, here.
        unsafe {
            TerminateProcess(self.handle, 1);
            CloseHandle(self.handle);
        }
    }
}

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
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo handle of this process needs no close, and `token`
    // is a live HANDLE that the call writes into.
    let opened = unsafe {
        OpenProcessToken(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
            TOKEN_ALL_ACCESS,
            &mut token,
        )
    };
    if opened == 0 {
        return Err(format!("OpenProcessToken: {}", last_error()));
    }
    let mut limited: HANDLE = std::ptr::null_mut();
    // SAFETY: `token` is the open token from above; the null pointers with
    // zero counts pass no SID or privilege lists, and `limited` is written.
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
    // SAFETY: `token` is open and closed once, here.
    unsafe {
        CloseHandle(token);
    }
    if restricted == 0 {
        return Err(format!("CreateRestrictedToken: {restricted_error}"));
    }
    #[repr(C)]
    struct MediumLabel {
        label: TOKEN_MANDATORY_LABEL,
        sid: SID,
    }
    let mut medium = MediumLabel {
        label: TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: std::ptr::null_mut(),
                Attributes: SE_GROUP_INTEGRITY,
            },
        },
        sid: SID {
            Revision: 1,
            SubAuthorityCount: 1,
            IdentifierAuthority: SID_IDENTIFIER_AUTHORITY {
                Value: [0, 0, 0, 0, 0, 16],
            },
            SubAuthority: [MEDIUM_INTEGRITY_RID],
        },
    };
    let value: *mut MediumLabel = &mut medium;
    // SAFETY: `value` points at `medium`, a live MediumLabel; the label's SID
    // pointer is set to the SID inside that same value, through `value`.
    unsafe {
        (*value).label.Label.Sid = std::ptr::addr_of_mut!((*value).sid).cast();
    }
    // SAFETY: the pointer is `value`, the start of `medium`, which holds the
    // label and then the SID that `Label.Sid` points at; the length is the
    // size of `medium`, so it covers both parts and no byte past them, and
    // `medium` lives for the call.
    let labelled = unsafe {
        SetTokenInformation(
            limited,
            TokenIntegrityLevel,
            value.cast_const().cast(),
            core::mem::size_of::<MediumLabel>() as u32,
        )
    };
    if labelled == 0 {
        let error = last_error();
        // SAFETY: `limited` is open and closed once, here.
        unsafe {
            CloseHandle(limited);
        }
        return Err(format!("SetTokenInformation: {error}"));
    }

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
    // SAFETY: STARTUPINFOW and PROCESS_INFORMATION are plain C structs for
    // which all-zero bytes are a valid value.
    let (mut startup, mut info): (STARTUPINFOW, PROCESS_INFORMATION) =
        unsafe { (core::mem::zeroed(), core::mem::zeroed()) };
    startup.cb = core::mem::size_of::<STARTUPINFOW>() as u32;
    // SAFETY: `limited` is an open primary token, `command_line` is a
    // writable NUL-terminated buffer, and `startup` and `info` are live.
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
    // SAFETY: `limited` is open and closed once, here.
    unsafe {
        CloseHandle(limited);
    }
    if created == 0 {
        return Err(format!("CreateProcessAsUserW: {created_error}"));
    }
    // SAFETY: the thread handle of the new process is not used and is closed
    // once, here; the process handle goes to the returned value.
    unsafe {
        CloseHandle(info.hThread);
    }
    Ok(RestrictedProcess {
        handle: info.hProcess,
    })
}

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
    let (sink, events) = capture(4096);
    let mut keyboard = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
    let start = keyboard.begin(SourceContext::new(sink.clone()));
    let started_ms = unix_millis(SystemTime::now());
    let window = TestWindow::create(&normal_window(
        LISTENER_WINDOW_CLASS,
        "halvo-verify-listener",
        320,
        60,
    ));
    let edit = window
        .as_ref()
        .ok()
        .and_then(|window| window.child_control("EDIT", WINDOW_STYLE(0), 10).ok());
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut stamps = Vec::new();
    let mut recorded = 0usize;
    let mut values = Vec::new();
    let mut last_values = None;
    let _ = seam::take();
    while Instant::now() < deadline {
        pump_thread(Duration::from_millis(20));
        if let (Ok(window), Some(edit)) = (&window, edit) {
            if window.is_foreground() {
                focus_on(edit);
            }
        }
        recorded += count(
            &events.drain_for(Duration::from_millis(20)),
            Modality::Keystroke,
        );
        // SAFETY: GetTickCount takes no argument and reads a counter.
        let tick = unsafe { GetTickCount() };
        let now = unix_millis(SystemTime::now());
        for key in seam::take() {
            let age = u128::from(tick.wrapping_sub(key.os_tick));
            stamps.push(now.saturating_sub(age).to_string());
        }
        let counters = keyboard.health().counters;
        let read = (u8::from(counters.keys_withheld), counters.late_events);
        if last_values != Some(read) {
            values.push(format!("{now}:{}:{}", read.0, read.1));
            last_values = Some(read);
        }
    }
    let _ = stop_counted(&mut keyboard, "keyboard");
    let report = format!(
        "keyboard_start={} window_created={} integrity_is_medium={} keys_decided={} keys_recorded={recorded}\n{started_ms}\n{}\n{}",
        show(&start),
        window.is_ok(),
        integrity_is_medium(),
        stamps.len(),
        stamps.join(","),
        values.join(",")
    );
    std::fs::write(out_path, report).expect("write the listener result");
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and an elevated process"]
fn x8_raw_input_across_the_integrity_boundary() {
    with_deadline("x8", Duration::from_secs(120), || {
        env_report("x8");
        source_built_by_test("x8", "keyboard");
        let dir = scratch_dir();
        let out_path = dir.path().join("listener.txt");
        say(
            "x8",
            format!("this_process_integrity_is_medium={}", integrity_is_medium()),
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
        let (own_foreground, method) = own.activate_with(false);
        pump_thread(Duration::from_millis(300));
        let elevated_from = unix_millis(SystemTime::now());
        press_keys(5);
        pump_thread(Duration::from_millis(400));
        let elevated_to = unix_millis(SystemTime::now());
        let own_foreground_after = own.is_foreground();

        let class = HSTRING::from(LISTENER_WINDOW_CLASS);
        // SAFETY: the class name is a live HSTRING for the call, and a null
        // title matches any window of that class.
        let listener_window = unsafe { FindWindowW(&class, PCWSTR::null()) };
        let handed_over = match &listener_window {
            Ok(hwnd) => {
                set_foreground(*hwnd);
                pump_thread(Duration::from_millis(400));
                foreground_window() == *hwnd
            }
            Err(_) => false,
        };
        let medium_from = unix_millis(SystemTime::now());
        press_keys(5);
        pump_thread(Duration::from_millis(400));
        let medium_to = unix_millis(SystemTime::now());
        let still_handed_over = listener_window
            .as_ref()
            .is_ok_and(|hwnd| foreground_window() == *hwnd);

        let exit_code = listener.wait(60_000);
        let report = std::fs::read_to_string(&out_path).unwrap_or_default();
        let mut lines = report.lines();
        let header = lines.next().unwrap_or("").to_string();
        let listener_started: Option<u128> = lines.next().and_then(|line| line.parse().ok());
        let stamps: Vec<u128> = lines
            .next()
            .unwrap_or("")
            .split(',')
            .filter_map(|stamp| stamp.parse().ok())
            .collect();
        let values: Vec<(u128, u8, u64)> = lines
            .next()
            .unwrap_or("")
            .split(',')
            .filter_map(|value| {
                let mut parts = value.split(':');
                Some((
                    parts.next()?.parse().ok()?,
                    parts.next()?.parse().ok()?,
                    parts.next()?.parse().ok()?,
                ))
            })
            .collect();
        let value_at = |time: u128| {
            values
                .iter()
                .rev()
                .find(|(at, _, _)| *at < time)
                .map(|(_, withheld, late)| (*withheld, *late))
        };
        let at_elevated_from = value_at(elevated_from);
        let at_medium_from = value_at(medium_from);
        let at_end = values.last().map(|(_, withheld, late)| (*withheld, *late));
        let set_by_the_elevated_phase =
            at_medium_from.is_some_and(|(withheld, late)| withheld != 0 || late != 0);
        let (period, period_from) = tick_period();
        let phases: Vec<Phase> = stamps
            .iter()
            .map(|stamp| {
                phase_of(
                    *stamp,
                    elevated_from,
                    elevated_to,
                    medium_from,
                    medium_to,
                    period,
                )
            })
            .collect();
        let count = |phase: Phase| phases.iter().filter(|p| **p == phase).count();
        let indistinct = phase_of(
            medium_from,
            elevated_from,
            elevated_to,
            medium_from,
            medium_to,
            period,
        ) == Phase::Indistinct;
        let from_medium = |t: u128| t as i128 - medium_from as i128;
        let medium_distances: Vec<i128> = stamps
            .iter()
            .zip(&phases)
            .filter(|(_, phase)| **phase == Phase::Medium)
            .map(|(stamp, _)| from_medium(*stamp))
            .collect();
        let source_started_before_typing = listener_started.is_some_and(|t| t < elevated_from);
        let between_the_phases = phases.iter().filter(|phase| phase.fails()).count();
        let distances: Vec<String> = stamps
            .iter()
            .map(|stamp| from_medium(*stamp).to_string())
            .collect();
        say(
            "x8",
            format!(
                "tick_period_ms={period} tick_period_from={period_from} \
                 elevated_to_from_elevated_from={} medium_from_from_elevated_from={} \
                 medium_to_from_elevated_from={} stamps_from_medium_from=[{}] \
                 phases_indistinct={indistinct} \
                 least_medium_stamp_from_medium_from={} \
                 medium_stamps_before_medium_from={}",
                elevated_to.saturating_sub(elevated_from),
                medium_from.saturating_sub(elevated_from),
                medium_to.saturating_sub(elevated_from),
                distances.join(","),
                medium_distances
                    .iter()
                    .min()
                    .map_or("none".to_string(), i128::to_string),
                medium_distances.iter().filter(|d| **d < 0).count(),
            ),
        );
        say(
            "x8",
            format!(
                "listener_values (keys_withheld, late) at_elevated_from={at_elevated_from:?} \
                 at_medium_from={at_medium_from:?} at_end={at_end:?} \
                 set_by_the_elevated_phase={set_by_the_elevated_phase}"
            ),
        );
        say(
            "x8",
            format!(
                "listener exit_code={exit_code:?} {header} \
                 listener_source_started_before_typing={source_started_before_typing} \
                 keystroke_events_from_the_elevated_phase_to_the_medium_phase={between_the_phases}"
            ),
        );
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
                count(Phase::Elevated),
                listener_window.is_ok(),
                count(Phase::Medium),
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
            source_started_before_typing,
            "INCONCLUSIVE: the listener's keyboard source did not start before the keys were typed"
        );
        assert!(
            !indistinct,
            "INCONCLUSIVE: the phases lay two tick periods apart or closer, so no stamp tells them apart"
        );
        assert!(
            count(Phase::Medium) >= 1,
            "INCONCLUSIVE: the listener saw no keys even for a window of its own integrity"
        );
        assert_eq!(
            between_the_phases, 0,
            "keys typed into an elevated window reached a medium-integrity keyboard source"
        );
        assert!(
            at_medium_from.is_some(),
            "INCONCLUSIVE: the listener wrote no withheld or late value before the medium phase"
        );
        assert_eq!(
            at_elevated_from,
            Some((0, 0)),
            "INCONCLUSIVE: the listener's withheld or late value was set before the elevated phase"
        );
        assert!(
            !set_by_the_elevated_phase,
            "the listener's keys_withheld or late value was set by the end of the elevated phase"
        );
    });
}

fn yellow_edge_pixels() -> usize {
    const CLR_INVALID: u32 = 0xFFFF_FFFF;
    let (_, cy) = screen_size();
    let mut yellow = 0;
    let mut unread = 0;
    // SAFETY: the screen DC is taken and released once in this block, and
    // GetPixel reads it only between the two calls and only when it is not null.
    let dc_taken = unsafe {
        let dc = GetDC(None);
        let taken = !dc.is_invalid();
        if taken {
            for x in 0..6 {
                for y in [cy / 6, cy / 3, cy / 2, 2 * cy / 3, 5 * cy / 6] {
                    let color = GetPixel(dc, x, y).0;
                    if color == CLR_INVALID {
                        unread += 1;
                        continue;
                    }
                    let (r, g, b) = (color & 0xFF, (color >> 8) & 0xFF, (color >> 16) & 0xFF);
                    if r > 200 && g > 150 && b < 100 {
                        yellow += 1;
                    }
                }
            }
            ReleaseDC(None, dc);
        }
        taken
    };
    assert!(
        dc_taken && unread == 0,
        "INCONCLUSIVE: the screen could not be read ({unread} of 30 edge pixels unread, screen DC taken: {dc_taken})"
    );
    yellow
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x7_capture_border_on_the_screen() {
    with_deadline("x7", Duration::from_secs(60), || {
        env_report("x7");
        source_built_by_test("x7", "screen");
        let _fields = FieldWindow::open("x7_capture_border_on_the_screen", Field::Plain);
        let before = yellow_edge_pixels();
        let (sink, events) = capture(256);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut screen = set.gated_screen(ScreenSource::new(Arc::new(Mutex::new(None))));
        let start = screen.begin(screen_on(sink.clone()));
        let clear = content_reads_clear("x7", &gate, GATE_CLEAR_WAIT);
        let first = events.first_within(FIRST_FRAME_WAIT);
        let frame_before_sampling = first
            .as_ref()
            .is_some_and(|e| e.modality == Modality::ScreenFrame);
        let frames = usize::from(frame_before_sampling)
            + count(
                &events.drain_for(Duration::from_millis(500)),
                Modality::ScreenFrame,
            );
        let during = yellow_edge_pixels();
        let _ = stop_counted(&mut screen, "screen");
        std::thread::sleep(Duration::from_secs(1));
        let after = yellow_edge_pixels();
        say(
            "x7",
            format!(
                "screen_start={} frame_before_sampling={frame_before_sampling} frames={frames} \
                 sampled_edge_pixels=30 yellow_before_capture={before} \
                 yellow_during_capture={during} yellow_after_stop={after}",
                show(&start)
            ),
        );
        gate_readings("x7", &gate);
        assert!(start.is_ok(), "the screen source must start");
        assert!(clear, "the gate never read Clear");
        assert!(
            frame_before_sampling,
            "INCONCLUSIVE: no frame before the edge was sampled"
        );
        assert_eq!(before, 0, "yellow edge pixels before the capture started");
        assert!(during > 0, "no capture border on the screen during capture");
        assert_eq!(after, 0, "the capture border stayed after stop");
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

#[test]
#[ignore = "functional: needs an interactive Windows desktop; changes the display mode"]
fn x9_screen_source_across_a_display_mode_change() {
    with_deadline("x9", Duration::from_secs(180), || {
        env_report("x9");
        source_built_by_test("x9", "screen");
        let (start_w, start_h) = screen_size();
        let (other_w, other_h) = if (start_w, start_h) == (1280, 720) {
            (1024, 768)
        } else {
            (1280, 720)
        };
        let _fields = FieldWindow::open(
            "x9_screen_source_across_a_display_mode_change",
            Field::Plain,
        );
        let (sink, events) = capture(1024);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut screen = set.gated_screen(ScreenSource::new(Arc::new(Mutex::new(None))));
        let start = screen.begin(screen_on(sink.clone()));
        let clear = content_reads_clear("x9", &gate, GATE_CLEAR_WAIT);
        let before = events.drain_for(Duration::from_secs(3));
        summarize("x9", "before-mode-change", &before);

        let changed = set_resolution(other_w, other_h);
        let (now_w, now_h) = screen_size();
        let window = TestWindow::create(&WindowSpec {
            ex_style: WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            ..magenta_popup("HalvoVerifyModeChange", "halvo-verify-mode")
        })
        .expect("moving window");
        for step in 0..12 {
            window.move_to(100 + step * 20, 100 + step * 10);
            pump_thread(Duration::from_millis(250));
        }
        drop(window);
        let after = events.drain_for(Duration::from_secs(2));
        summarize("x9", "after-mode-change", &after);
        let _ = stop_counted(&mut screen, "screen");
        let restored = set_resolution(start_w, start_h);
        let frames_after = count(&after, Modality::ScreenFrame);
        say(
            "x9",
            format!(
                "screen_start={} mode_before={start_w}x{start_h} mode_change_command_ok={changed} \
                 mode_after={now_w}x{now_h} frames_before={} frames_after_change={} \
                 mode_restored={restored}",
                show(&start),
                count(&before, Modality::ScreenFrame),
                frames_after,
            ),
        );
        gate_readings("x9", &gate);
        assert!(start.is_ok(), "the screen source must start");
        assert!(clear, "the gate never read Clear");
        assert!(
            changed && (now_w, now_h) == (other_w, other_h),
            "INCONCLUSIVE: the display mode did not change"
        );
        assert!(frames_after >= 1, "no frame after the mode change");
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x13_frames_on_a_static_screen() {
    with_deadline("x13", Duration::from_secs(90), || {
        env_report("x13");
        source_built_by_test("x13", "screen");
        let (sink, events) = capture(1024);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut screen = set.gated_screen(ScreenSource::new(Arc::new(Mutex::new(None))));
        let start = screen.begin(screen_on(sink.clone()));
        let cover = TestWindow::create(&magenta_popup("HalvoVerifyStatic", "halvo-verify-static"))
            .expect("covering window");
        let edit_above_the_client_area = cover
            .child_control("EDIT", WINDOW_STYLE(0), -100)
            .expect("the cover's edit");
        cover.cover_screen();
        let (in_front, method) = cover.activate();
        let cover_edit_focused = focus_on(edit_above_the_client_area);
        let clear = content_reads_clear("x13", &gate, GATE_CLEAR_WAIT);
        pump_thread(Duration::from_secs(1));
        cover.move_to(40, 0);
        pump_thread(Duration::from_millis(150));
        cover.move_to(0, 0);
        let mut first_frame = false;
        let began = Instant::now();
        while !first_frame && began.elapsed() < FIRST_FRAME_WAIT {
            pump_thread(Duration::from_millis(100));
            first_frame = count(
                &events.drain_for(Duration::from_millis(100)),
                Modality::ScreenFrame,
            ) > 0;
        }
        let mut records_while_static = 0;
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(8) {
            pump_thread(Duration::from_millis(100));
            records_while_static += count(
                &events.drain_for(Duration::from_millis(100)),
                Modality::ScreenFrame,
            );
        }
        let covered = cover.is_foreground();
        let windows_above_cover = visible_windows_above(cover.hwnd);
        let mut records_while_moving = 0;
        for step in 0..12 {
            cover.move_to(if step % 2 == 0 { 40 } else { 0 }, 0);
            pump_thread(Duration::from_millis(150));
            records_while_moving += count(
                &events.drain_for(Duration::from_millis(100)),
                Modality::ScreenFrame,
            );
        }
        drop(cover);
        let frames = screen.source().frame_counts();
        let _ = stop_counted(&mut screen, "screen");
        let every_frame_changed =
            frames.delivered > 0 && frames.with_changed_region == frames.delivered;
        let by_the_time_filter = 8_000 / DEFAULT_MIN_FRAME_INTERVAL.as_millis() as usize + 1;
        say("x13", format!("cover_edit_focused={cover_edit_focused}"));
        gate_readings("x13", &gate);
        say(
            "x13",
            format!(
                "screen_start={} screen_activity=on cover_activated={in_front} method={method} \
                 regions_reported={} first_frame={first_frame} \
                 static_screen_secs=8 records_while_static={records_while_static} \
                 cover_window_was_foreground={covered} windows_above_cover={windows_above_cover} \
                 records_while_moving={records_while_moving} \
                 frames_delivered={} frames_with_changed_region={} \
                 every_delivered_frame_changed={every_frame_changed} \
                 records_by_the_time_filter={by_the_time_filter}",
                show(&start),
                frames.regions_reported,
                frames.delivered,
                frames.with_changed_region,
            ),
        );
        assert!(start.is_ok(), "the screen source must start");
        assert!(clear, "the gate never read Clear");
        assert!(first_frame, "INCONCLUSIVE: no first frame");
        assert!(
            covered,
            "INCONCLUSIVE: the covering window was not in front, so the screen was not static"
        );
        assert!(
            records_while_moving >= 1,
            "no screen record while a window moved"
        );
        if every_frame_changed && records_while_static > 1 {
            assert!(
                records_while_static <= by_the_time_filter,
                "more screen records on a static screen than one for each interval of the time filter"
            );
        } else {
            flipped!(
                "VWF-10",
                records_while_static <= 1,
                format!("TF x13 records_while_static={records_while_static}")
            );
        }
    });
}

/// The visible top-level windows above `hwnd` in the order of the desktop.
fn visible_windows_above(hwnd: HWND) -> usize {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindow, IsWindowVisible, GW_HWNDPREV};
    let mut count = 0;
    let mut current = hwnd;
    for _ in 0..4_096 {
        // SAFETY: GetWindow takes a window handle by value; a handle with no
        // window before it gives an error.
        let Ok(above) = (unsafe { GetWindow(current, GW_HWNDPREV) }) else {
            break;
        };
        // SAFETY: IsWindowVisible takes a window handle by value.
        if unsafe { IsWindowVisible(above) }.as_bool() {
            count += 1;
        }
        current = above;
    }
    count
}

fn detector_line(test: &str, at: &str) {
    let seen = seam::detector();
    // SAFETY: GetTickCount takes no argument and reads a counter.
    let now = unsafe { GetTickCount() };
    let attach = match &seen.attach {
        None => "none".to_string(),
        Some(Ok(())) => "ok".to_string(),
        Some(Err(error)) => format!("error:{}", error.replace(' ', "_")),
    };
    say(
        test,
        format!(
            "detector at={at} attach={attach} own_reads_since_the_start={} read_in_flight={} \
             read_in_flight_since_ms={} focus_handler_entered={} focus_handler_ended={}",
            seen.own_reads,
            seen.read_since.is_some(),
            seen.read_since
                .map_or(-1, |since| i64::from(now.wrapping_sub(since))),
            seen.handler_entered,
            seen.handler_ended,
        ),
    );
}

fn gate_reads_clear(test: &str, fields: Option<&FieldWindow>, keyboard: &RawInputSource) -> bool {
    use halvo_capture::secure_input::windows::FieldState;
    let started = Instant::now();
    let at_start = keyboard.field_state();
    if let Some(fields) = fields {
        fields.focus(Field::Password);
        std::thread::sleep(Duration::from_millis(150));
        fields.focus(Field::Plain);
    }
    let began = Instant::now();
    let mut clear = false;
    while began.elapsed() < Duration::from_secs(3) {
        if keyboard.field_state() == Some(FieldState::Clear) {
            clear = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(150));
    if let Some(left) = Duration::from_secs(1).checked_sub(started.elapsed()) {
        std::thread::sleep(left);
    }
    let _ = seam::take();
    say(
        test,
        format!(
            "gate_at_start={at_start:?} gate_reads_clear_before_typing={clear} waited_ms={}",
            started.elapsed().as_millis()
        ),
    );
    if !clear {
        detector_line(test, "gate-reads-clear");
    }
    clear
}

fn control_reads(test: &str, which: &str, marks: &[seam::MarkSeen], decided: &[seam::Decided]) {
    let letters: Vec<u32> = decided
        .iter()
        .filter(|key| key.class == seam::KeyClass::Printable)
        .map(|key| key.os_tick)
        .collect();
    let Some(first) = letters.first().copied() else {
        say(
            test,
            format!("control_reads={which} letters=0 marks={}", marks.len()),
        );
        return;
    };
    let rel = |tick: u32| i64::from(tick.wrapping_sub(first) as i32);
    let last = letters.iter().map(|tick| rel(*tick)).max().unwrap_or(0);
    let reads: Vec<(i64, i64, Option<seam::Moved>)> = marks
        .iter()
        .filter(|mark| mark.what == "read")
        .map(|mark| {
            (
                rel(mark.at.unwrap_or(mark.began)),
                rel(mark.began),
                mark.moved,
            )
        })
        .filter(|(at, _, _)| (-1500..=last + 500).contains(at))
        .collect();
    let event = marks
        .iter()
        .filter(|mark| mark.what == "event" || mark.what.starts_with("latch"))
        .map(|mark| rel(mark.at.unwrap_or(mark.began)))
        .filter(|at| *at < 0)
        .max();
    let armed = event.and_then(|event| reads.iter().find(|(_, began, _)| *began >= event));
    let not_same_from_the_first = reads
        .iter()
        .filter(|(at, _, moved)| *at >= 0 && *at <= last + 250 && *moved != Some(seam::Moved::Same))
        .count();
    let listed: Vec<String> = reads
        .iter()
        .map(|(at, _, moved)| format!("{at}:{moved:?}"))
        .collect();
    say(
        test,
        format!(
            "control_reads={which} letters={} last_letter_at={last} reads=[{}] move_at={event:?} \
             armed_read_at={:?} armed_moved={:?} armed_read_before_the_first_letter={} \
             reads_not_same_from_the_first_letter={not_same_from_the_first}",
            letters.len(),
            listed.join(","),
            armed.map(|(at, _, _)| *at),
            armed.and_then(|(_, _, moved)| *moved),
            armed.is_some_and(|(at, _, _)| *at < 0),
        ),
    );
}

fn mark_trail(test: &str, which: &str, marks: &[seam::MarkSeen], decided: &[seam::Decided]) {
    let Some(first) = decided.first().map(|key| key.os_tick) else {
        say(test, format!("trail {which} keys=0 marks={}", marks.len()));
        return;
    };
    let rel = |tick: u32| i64::from(tick.wrapping_sub(first) as i32);
    let keys: Vec<String> = decided
        .iter()
        .map(|key| {
            format!(
                "{:?}@{}:{:?}:{:?}@{}",
                key.class,
                rel(key.os_tick),
                key.verdict,
                key.rule,
                rel(key.decided_tick)
            )
        })
        .collect();
    let last = decided
        .iter()
        .map(|key| rel(key.decided_tick))
        .max()
        .unwrap_or(0);
    let listed: Vec<String> = marks
        .iter()
        .filter_map(|mark| {
            let at = rel(mark.at.unwrap_or(mark.began));
            (-800..=last + 300).contains(&at).then(|| {
                let file = mark
                    .element
                    .as_deref()
                    .and_then(|e| e.split(' ').find(|part| part.starts_with("kind=")))
                    .unwrap_or("kind=none");
                format!(
                    "{}@{at}:{:?}:{:?}:{file}",
                    mark.what, mark.state, mark.moved
                )
            })
        })
        .collect();
    say(
        test,
        format!(
            "trail {which} keys=[{}] marks=[{}]",
            keys.join(","),
            listed.join(",")
        ),
    );
}

fn gate_trail(test: &str) {
    let keys = seam::take();
    let marks = seam::take_marks();
    let Some(base) = keys.first().map(|key| key.os_tick) else {
        say(test, format!("trail keys=0 marks={}", marks.len()));
        return;
    };
    let rel = |tick: u32| i64::from(tick.wrapping_sub(base) as i32);
    let last = keys.iter().map(|key| rel(key.os_tick)).max().unwrap_or(0);
    for key in &keys {
        say(
            test,
            format!(
                "trail key class={:?} verdict={:?} rule={:?} at={} decided={}",
                key.class,
                key.verdict,
                key.rule,
                rel(key.os_tick),
                rel(key.decided_tick)
            ),
        );
    }
    for mark in marks.iter().filter(|mark| {
        let at = rel(mark.at.unwrap_or(mark.began));
        (-1500..=last + 500).contains(&at)
    }) {
        say(
            test,
            format!(
                "trail mark what={} state={:?} moved={:?} at={} began={} {}",
                mark.what,
                mark.state,
                mark.moved,
                mark.at.map_or(i64::MIN, rel),
                rel(mark.began),
                mark.element.as_deref().unwrap_or("element=none")
            ),
        );
    }
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn the_first_own_read_after_a_start_reads_the_focused_password_field() {
    with_deadline("first-read", Duration::from_secs(90), || {
        use halvo_capture::secure_input::windows::FieldState;
        source_built_by_test("first-read", "keyboard");
        let fields = FieldWindow::open("first-read", Field::Password);
        let mut rounds = 0;
        let mut passed = 0;
        for round in 0..5 {
            let focused = fields.focus(Field::Password);
            std::thread::sleep(Duration::from_millis(300));
            let _ = seam::take();
            let _ = seam::take_marks();
            let (sink, _events) = capture(256);
            let mut keyboard =
                RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
            let start = keyboard.begin(SourceContext::new(sink.clone()));
            let marks = seam::take_marks();
            let stopped = stop_counted(&mut keyboard, "keyboard");
            let started = marks.iter().position(|mark| mark.what == "start");
            let first_read =
                started.and_then(|at| marks[at..].iter().find(|mark| mark.what == "read"));
            let start_state = started.map(|at| marks[at].state);
            let read_state = first_read.and_then(|mark| mark.state);
            let element = first_read
                .and_then(|mark| mark.element.as_deref())
                .unwrap_or("element=none");
            say(
                "first-read",
                format!(
                    "round={round} foreground={} focused={focused} start={} stop={} \
                     start_mark={start_state:?} first_read={read_state:?} {element}",
                    fields.foreground,
                    show(&start),
                    show(&stopped)
                ),
            );
            assert!(fields.foreground, "the fixture window is not in front");
            assert!(
                focused,
                "the fixture's password field did not take the focus"
            );
            assert!(
                start.is_ok() && stopped.is_ok(),
                "the source did not start and stop"
            );
            assert_eq!(
                start_state,
                Some(Some(FieldState::Unknown)),
                "the start's mark"
            );
            rounds += 1;
            if read_state == Some(FieldState::Password) && element.contains("is_password=1") {
                passed += 1;
            }
        }
        say(
            "first-read",
            format!("rounds={rounds} first_read_password={passed}"),
        );
        assert_eq!(
            passed, rounds,
            "a first own read after a start did not read the password field"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn capture_times_follow_the_input_order() {
    with_deadline("order", Duration::from_secs(60), || {
        const ROUNDS: usize = 3;
        env_report("order");
        source_built_by_test("order", "keyboard");
        let (sink, writer) = channel(1);
        let last_app = Arc::new(Mutex::new(None));
        let fields = FieldWindow::open("capture_times_follow_the_input_order", Field::Plain);
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app), PasswordGate::new());
        let mut pointer = RawInputSource::pointer(last_app);
        let keyboard_start = keyboard.begin(SourceContext::new(sink.clone()));
        let pointer_start = pointer.begin(SourceContext::new(sink.clone()));
        gate_reads_clear("order", Some(&fields), &keyboard);
        let mut keys_sent = Vec::new();
        let mut turns_sent = Vec::new();
        let mut accepted = 0;
        for _ in 0..ROUNDS {
            accepted += send(&[key_input(VK_A, false)]);
            keys_sent.push(SystemTime::now());
            accepted += send(&[key_input(VK_A, true)]);
            std::thread::sleep(Duration::from_millis(100));
            accepted += send(&[wheel_notch()]);
            turns_sent.push(SystemTime::now());
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_millis(1500));
        let events = forward(writer).drain_for(Duration::from_secs(3));
        let _ = stop_counted(&mut keyboard, "keyboard");
        let _ = stop_counted(&mut pointer, "pointer");
        let mut key_times: Vec<SystemTime> = events
            .iter()
            .filter(|e| e.modality == Modality::Keystroke)
            .map(|e| e.captured_at.wall_clock)
            .collect();
        key_times.sort();
        let scrolls: Vec<(u64, SystemTime)> = events
            .iter()
            .filter_map(|e| match e.view.and_then(|view| view.fold) {
                Some((FoldClass::Scroll, count)) => Some((count, e.captured_at.wall_clock)),
                _ => None,
            })
            .collect();
        let distance_ms = |at: SystemTime, sent: SystemTime| {
            at.duration_since(sent)
                .or_else(|e| Ok::<Duration, ()>(e.duration()))
                .unwrap_or_default()
                .as_millis()
        };
        let keys_in_their_second = key_times.len() == ROUNDS
            && key_times.iter().zip(&keys_sent).all(|(at, sent)| {
                sent.duration_since(*at)
                    .is_ok_and(|before| before < Duration::from_millis(1_250))
            });
        let scroll_distance_ms = scrolls
            .first()
            .map_or(u128::MAX, |(_, at)| distance_ms(*at, turns_sent[0]));
        say(
            "order",
            format!(
                "keyboard_start={} pointer_start={} sink_capacity=1 draining_stopped_ms=1500 \
                 sendinput_events_accepted={accepted} events={} keystroke_events={} \
                 keys_in_their_second={keys_in_their_second} scroll_records={} scroll_count={} \
                 scroll_distance_from_the_first_turn_ms={scroll_distance_ms}",
                show(&keyboard_start),
                show(&pointer_start),
                events.len(),
                key_times.len(),
                scrolls.len(),
                scrolls.first().map_or(0, |(count, _)| *count),
            ),
        );
        assert!(keyboard_start.is_ok(), "the keyboard source must start");
        assert!(pointer_start.is_ok(), "the pointer source must start");
        assert_eq!(
            accepted,
            3 * ROUNDS as u32,
            "INCONCLUSIVE: SendInput did not insert every key and wheel event"
        );
        assert!(
            keys_in_their_second,
            "a keystroke record is not stamped at the second of its input"
        );
        assert_eq!(
            scrolls.len(),
            1,
            "the wheel turns are not one scroll record"
        );
        assert_eq!(
            scrolls[0].0, ROUNDS as u64,
            "the scroll record does not count every turn"
        );
        assert!(
            scroll_distance_ms <= 250,
            "the scroll record is {scroll_distance_ms} ms away from its first turn"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn stop_with_a_full_sink() {
    with_deadline("full-sink", Duration::from_secs(60), || {
        env_report("full-sink");
        source_built_by_test("full-sink", "keyboard");
        let (sink, writer) = channel(1);
        let fields = FieldWindow::open("stop_with_a_full_sink", Field::Plain);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        gate_reads_clear("full-sink", Some(&fields), &keyboard);
        let accepted = type_letters(3);
        std::thread::sleep(Duration::from_millis(500));
        let (done, returned) = mpsc::channel();
        let began = Instant::now();
        std::thread::Builder::new()
            .name("tf-full-sink-stop".to_string())
            .spawn(move || {
                let _ = done.send(stop_counted(&mut keyboard, "keyboard"));
            })
            .expect("spawn the stop thread");
        let stopped = returned.recv_timeout(FULL_SINK_STOP_LIMIT);
        let waited_ms = began.elapsed().as_millis();
        let events = forward(writer);
        let delivered = count(
            &events.drain_for(Duration::from_secs(1)),
            Modality::Keystroke,
        );
        say(
            "full-sink",
            format!(
                "keyboard_start={} sink_capacity=1 sendinput_key_events_accepted={accepted} \
                 stop={stopped:?} stop_ms={waited_ms} \
                 keystroke_events_delivered_after_the_stop={delivered}",
                show(&start),
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert_eq!(
            accepted, 6,
            "INCONCLUSIVE: SendInput did not insert the three key presses"
        );
        assert!(
            delivered >= 2,
            "INCONCLUSIVE: fewer than two keystrokes reached the sink, so it was not full when stop was called"
        );
        assert!(
            matches!(stopped, Ok(Ok(()))),
            "the stop did not return Ok within {FULL_SINK_STOP_LIMIT:?}: {stopped:?}"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn a_dropped_keyboard_source_stops_capturing() {
    with_deadline("dropped", Duration::from_secs(60), || {
        env_report("dropped");
        source_built_by_test("dropped", "keyboard");
        let (sink, events) = capture(4096);
        let fields = FieldWindow::open("a_dropped_keyboard_source_stops_capturing", Field::Plain);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        gate_reads_clear("dropped", Some(&fields), &keyboard);
        let accepted_before = type_letters(1);
        let before = count(
            &events.drain_for(Duration::from_millis(700)),
            Modality::Keystroke,
        );
        drop(keyboard);
        std::thread::sleep(Duration::from_millis(300));
        let accepted_after = type_letters(3);
        let after = count(
            &events.drain_for(Duration::from_secs(1)),
            Modality::Keystroke,
        );
        let mut next = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let next_start = next.begin(SourceContext::new(sink.clone()));
        gate_reads_clear("dropped", Some(&fields), &next);
        let accepted_next = type_letters(1);
        let next_keys = count(
            &events.drain_for(Duration::from_millis(700)),
            Modality::Keystroke,
        );
        let next_stop = stop_counted(&mut next, "keyboard");
        say(
            "dropped",
            format!(
                "keyboard_start={} sendinput_key_events_accepted_before_the_drop={accepted_before} \
                 keystroke_events_before_the_drop={before} \
                 sendinput_key_events_accepted_after_the_drop={accepted_after} \
                 keystroke_events_after_the_drop={after} next_keyboard_start={} \
                 keystroke_events_of_the_next_source={next_keys} next_keyboard_stop={}",
                show(&start),
                show(&next_start),
                show(&next_stop),
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert!(
            accepted_before == 2 && accepted_after == 6 && accepted_next == 2,
            "INCONCLUSIVE: SendInput did not insert every key event"
        );
        assert!(
            before >= 1,
            "INCONCLUSIVE: the keyboard source recorded no key before the drop"
        );
        assert_eq!(after, 0, "the dropped source recorded keys");
        assert!(
            next_start.is_ok(),
            "a new keyboard source must start in the same process"
        );
        assert!(next_keys >= 1, "the new keyboard source recorded no key");
        assert!(next_stop.is_ok(), "the new keyboard source must stop");
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn keys_queued_before_a_stop_are_delivered_or_counted() {
    with_deadline("queued-at-stop", Duration::from_secs(60), || {
        env_report("queued-at-stop");
        source_built_by_test("queued-at-stop", "keyboard");
        let (sink, writer) = channel(64);
        let last_app = Arc::new(Mutex::new(None));
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        std::thread::sleep(Duration::from_millis(200));
        let _ = seam::take();
        let held = seam::hold_the_pumps();
        let accepted = press_keys(5);
        std::thread::sleep(Duration::from_millis(300));
        let (done, returned) = mpsc::channel();
        std::thread::Builder::new()
            .name("tf-queued-at-stop-stop".to_string())
            .spawn(move || {
                let stopped = stop_counted(&mut keyboard, "keyboard");
                let lost = keyboard.handoff_counts().lost;
                let late = keyboard.health().counters.late_events;
                let _ = done.send((stopped, lost, late));
            })
            .expect("spawn the stop thread");
        std::thread::sleep(Duration::from_millis(200));
        let mut before_release = 0u64;
        while let Ok(received) = writer.try_recv() {
            if received.event().modality == Modality::Keystroke {
                before_release += 1;
            }
        }
        drop(held);
        let outcome = returned.recv_timeout(Duration::from_secs(5));
        let events = forward(writer);
        let delivered = before_release
            + count(
                &events.drain_for(Duration::from_secs(1)),
                Modality::Keystroke,
            ) as u64;
        let (stopped, lost, late) = match &outcome {
            Ok((stopped, lost, late)) => (format!("{stopped:?}"), *lost, *late),
            Err(e) => (format!("no answer ({e:?})"), 0, 0),
        };
        let decided = seam::take();
        let not_recorded = decided
            .iter()
            .filter(|key| key.verdict != seam::Verdict::Recorded)
            .count() as u64;
        say(
            "queued-at-stop",
            format!(
                "keyboard_start={} presses=5 sendinput_key_events_accepted={accepted} \
                 keystrokes_before_the_release={before_release} \
                 stop={stopped} keystrokes_delivered={delivered} lost={lost} late_value={late} \
                 keys_decided_by_the_gate={} keys_withheld_or_undecided_at_the_stop={not_recorded} \
                 neither_delivered_nor_decided={}",
                show(&start),
                decided.len(),
                5i64 - delivered as i64 - lost as i64 - not_recorded as i64,
            ),
        );
        drop(sink);
        assert!(start.is_ok(), "the keyboard source must start");
        assert_eq!(
            accepted, 10,
            "INCONCLUSIVE: SendInput did not insert every key event"
        );
        assert_eq!(
            before_release, 0,
            "the hold did not take: keystrokes reached the sink before the hold was released, so this run shows nothing about input queued at a stop"
        );
        assert!(
            matches!(outcome, Ok((Ok(()), _, _))),
            "the stop did not answer Ok: {outcome:?}"
        );
        assert_eq!(
            delivered + lost + not_recorded,
            5,
            "key presses queued before the stop were neither delivered nor decided: \
             delivered {delivered}, lost {lost}, withheld or undecided {not_recorded}"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn a_timed_out_stop_reads_no_input_posted_after_its_answer() {
    with_deadline("after-timeout", Duration::from_secs(60), || {
        env_report("after-timeout");
        source_built_by_test("after-timeout", "keyboard");
        let (sink, writer) = channel(64);
        let last_app = Arc::new(Mutex::new(None));
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        std::thread::sleep(Duration::from_millis(200));
        let _ = seam::take();
        let held = seam::hold_the_pumps();
        let accepted_before = press_keys(1);
        std::thread::sleep(Duration::from_millis(300));
        let (done, returned) = mpsc::channel();
        let stopper = std::thread::Builder::new()
            .name("tf-after-timeout-stop".to_string())
            .spawn(move || {
                let stopped = stop_counted(&mut keyboard, "keyboard");
                let _ = done.send(stopped);
                keyboard
            })
            .expect("spawn the stop thread");
        let stopped = returned.recv_timeout(Duration::from_secs(5));
        let accepted_after = press_keys(5);
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
        let keyboard = stopper.join().expect("the stop thread");
        std::thread::sleep(Duration::from_secs(1));
        let lost = keyboard.handoff_counts().lost;
        let late = keyboard.health().counters.late_events;
        let events = forward(writer);
        let delivered = count(
            &events.drain_for(Duration::from_secs(1)),
            Modality::Keystroke,
        ) as u64;
        let decided = seam::take().len() as u64;
        say(
            "after-timeout",
            format!(
                "keyboard_start={} presses_before_the_stop=1 presses_after_the_answer=5 \
                 sendinput_key_events_accepted_before={accepted_before} \
                 sendinput_key_events_accepted_after={accepted_after} stop={stopped:?} \
                 keystrokes_delivered={delivered} lost={lost} late_value={late} \
                 keys_that_the_pump_read={decided}",
                show(&start),
            ),
        );
        drop(keyboard);
        drop(sink);
        assert!(start.is_ok(), "the keyboard source must start");
        assert!(
            accepted_before == 2 && accepted_after == 10,
            "INCONCLUSIVE: SendInput did not insert every key event"
        );
        assert!(
            matches!(stopped, Ok(Err(CaptureError::Timeout { .. }))),
            "INCONCLUSIVE: the held pump's stop did not answer Timeout: {stopped:?}"
        );
        assert_eq!(
            decided, 1,
            "the pump read input posted after the Timeout answer: it decided {decided} keys, delivered {delivered}, lost {lost}"
        );
    });
}

fn screen_frames_within(events: &Events, window: Duration) -> usize {
    count(&events.drain_for(window), Modality::ScreenFrame)
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x5_screen_source_stop_and_start_again() {
    install_fault_line();
    with_deadline("x5", Duration::from_secs(90), || {
        env_report("x5");
        source_built_by_test("x5", "screen");
        let _fields = FieldWindow::open("x5_screen_source_stop_and_start_again", Field::Plain);
        let (sink, events) = capture(256);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut screen = set.gated_screen(ScreenSource::new(Arc::new(Mutex::new(None))));
        let first = screen.begin(screen_on(sink.clone()));
        let first_clear = content_reads_clear("x5", &gate, GATE_CLEAR_WAIT);
        let first_frame = events.first_within(FIRST_FRAME_WAIT).is_some();
        let first_stop = stop_counted(&mut screen, "screen");
        let _ = events.drain_for(Duration::from_millis(500));
        let second = screen.begin(screen_on(sink.clone()));
        let second_clear = content_reads_clear("x5", &gate, GATE_CLEAR_WAIT);
        let second_frame = events.first_within(FIRST_FRAME_WAIT).is_some();
        let second_stop = stop_counted(&mut screen, "screen");
        gate_readings("x5", &gate);
        say(
            "x5",
            format!(
                "first_start={} first_frame={first_frame} first_stop={} second_start={} \
                 second_frame={second_frame} second_stop={}",
                show(&first),
                show(&first_stop),
                show(&second),
                show(&second_stop),
            ),
        );
        assert!(first.is_ok() && second.is_ok(), "both starts must succeed");
        assert!(first_clear && second_clear, "the gate never read Clear");
        assert!(
            first_frame && second_frame,
            "each session must deliver a frame"
        );
        assert!(
            first_stop.is_ok() && second_stop.is_ok(),
            "both stops must succeed"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x12_screen_source_fifteen_start_stop_cycles() {
    const CYCLES: usize = 15;
    install_fault_line();
    with_deadline("x12", Duration::from_secs(180), || {
        env_report("x12");
        source_built_by_test("x12", "screen");
        let _fields =
            FieldWindow::open("x12_screen_source_fifteen_start_stop_cycles", Field::Plain);
        let last_app = Arc::new(Mutex::new(None));
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut started = 0;
        let mut stopped = 0;
        let mut frames = 0;
        let mut cycles_read_clear = 0;
        for _ in 0..CYCLES {
            let (sink, events) = capture(256);
            let mut screen = set.gated_screen(ScreenSource::new(Arc::clone(&last_app)));
            if screen.begin(screen_on(sink.clone())).is_ok() {
                started += 1;
            }
            if wait_for_clear(&gate, GATE_CLEAR_WAIT).0 {
                cycles_read_clear += 1;
            }
            frames += screen_frames_within(&events, Duration::from_millis(700));
            if stop_counted(&mut screen, "screen").is_ok() {
                stopped += 1;
            }
        }
        std::thread::sleep(Duration::from_secs(3));
        say(
            "x12",
            format!(
                "cycles={CYCLES} started={started} stopped={stopped} frames_total={frames} \
                 cycles_read_clear={cycles_read_clear} idle_3s_after=true"
            ),
        );
        gate_readings("x12", &gate);
        assert_eq!(started, CYCLES, "every cycle must start");
        assert_eq!(
            cycles_read_clear, CYCLES,
            "the gate never read Clear in a cycle"
        );
        assert_eq!(stopped, CYCLES, "every cycle must stop");
        assert!(frames >= 1, "no frame in fifteen cycles");
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn c8b_window_destroyed_after_the_last_capture_session() {
    install_fault_line();
    with_deadline("c8b", Duration::from_secs(120), || {
        env_report("c8b");
        source_built_by_test("c8b", "screen");
        let popup = TestWindow::create(&magenta_popup("HalvoVerifyC8b", "halvo-verify-c8b"))
            .expect("the window");
        pump_thread(Duration::from_millis(500));
        let _fields = FieldWindow::open(
            "c8b_window_destroyed_after_the_last_capture_session",
            Field::Plain,
        );
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let session = |label: &str| {
            let (sink, events) = capture(256);
            let before = gate.readings();
            let mut screen = set.gated_screen(ScreenSource::new(Arc::new(Mutex::new(None))));
            let start = screen.begin(screen_on(sink.clone()));
            let mut frames = 0;
            let began = Instant::now();
            while began.elapsed() < Duration::from_secs(3) {
                pump_thread(Duration::from_millis(50));
                frames += screen_frames_within(&events, Duration::from_millis(50));
            }
            let stop = stop_counted(&mut screen, "screen");
            frames += screen_frames_within(&events, Duration::from_millis(300));
            let lost = screen.source().handoff_counts().lost;
            let after = gate.readings();
            let gate_clear = after.clear() - before.clear();
            let gate_unknown = after.unknown() - before.unknown();
            let gate_password_field = after.password_field - before.password_field;
            let kept_frames = frames as u64 + lost;
            say(
                "c8b",
                format!(
                    "capture={label} start={} gate_clear={gate_clear} gate_unknown={gate_unknown} \
                     gate_password_field={gate_password_field} frames={frames} lost={lost} stop={}",
                    show(&start),
                    show(&stop)
                ),
            );
            (start.is_ok() && stop.is_ok(), kept_frames, gate_clear)
        };
        let first = session("first");
        let excluded = exclude_from_capture(popup.hwnd);
        pump_thread(Duration::from_millis(500));
        let second = session("second");
        gate_readings("c8b", &gate);
        drop(popup);
        say(
            "c8b",
            "teardown window_destroyed_after_the_last_session=true",
        );
        std::thread::sleep(Duration::from_secs(2));
        say("c8b", "teardown idle_2s_after=true");
        assert!(
            excluded.is_ok(),
            "the window must take the display affinity"
        );
        assert!(first.0 && second.0, "each session must start and stop");
        assert!(
            first.2 + second.2 > 0,
            "INCONCLUSIVE: no session read Clear, so no session showed that a running detector keeps frames"
        );
        assert_eq!(
            (first.1, second.1),
            (first.2, second.2),
            "the frames that each session kept (first, second) are not its Clear readings: \
             a frame left with no Clear reading, or a Clear reading kept no frame"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x3_attribution_before_the_first_foreground_change() {
    with_deadline("x3", Duration::from_secs(120), || {
        env_report("x3");
        source_built_by_test("x3", "keyboard");
        let mut child = spawn_child("hold", 9);
        let child_pid = child.id();
        let waited = Instant::now();
        while foreground_pid() != child_pid && waited.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_secs(2));
        let child_foreground_at_start = foreground_pid() == child_pid;
        let reader_sees_child = current_foreground_app().is_some_and(|app| is_child_app(&app));

        let (sink, events) = capture(4096);
        let mut foreground = ForegroundAppSource::new();
        let last_app = foreground.last_app();
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app), PasswordGate::new());
        let foreground_start = foreground.begin(SourceContext::new(sink.clone()));
        let keyboard_start = keyboard.begin(SourceContext::new(sink.clone()));
        gate_reads_clear("x3", None, &keyboard);
        let settle = events.drain_for(Duration::from_secs(1));
        let cell_before_typing = last_app.lock().ok().and_then(|cell| cell.clone());
        type_letters(5);
        let before = events.drain_for(Duration::from_millis(700));
        let still_child_foreground = foreground_pid() == child_pid;

        let mut between = Vec::new();
        let waited = Instant::now();
        while !between
            .iter()
            .any(|e: &Seen| e.modality == Modality::ForegroundApp)
            && waited.elapsed() < Duration::from_secs(15)
        {
            between.extend(events.drain_for(Duration::from_millis(100)));
        }
        type_letters(5);
        let after = events.drain_for(Duration::from_millis(700));
        let exit = child.wait();
        let _ = stop_counted(&mut foreground, "foreground");
        let _ = stop_counted(&mut keyboard, "keyboard");
        child.print_output();

        let keys = |events: &[Seen]| count(events, Modality::Keystroke);
        let empty_app = |events: &[Seen]| {
            events
                .iter()
                .filter(|e| e.modality == Modality::Keystroke && e.foreground_app.is_empty())
                .count()
        };
        gate_trail("x3");
        let child_app = |events: &[Seen]| {
            events
                .iter()
                .filter(|e| e.modality == Modality::Keystroke && is_child_app(&e.foreground_app))
                .count()
        };
        say(
            "x3",
            format!(
                "foreground_start={} keyboard_start={} \
                 child_window_foreground_when_sources_started={child_foreground_at_start} \
                 current_foreground_app_reader_names_child={reader_sees_child} \
                 foreground_events_in_first_second={} last_app_cell_is_none_before_typing={} \
                 child_window_foreground_while_typing={still_child_foreground}",
                show(&foreground_start),
                show(&keyboard_start),
                count(&settle, Modality::ForegroundApp),
                cell_before_typing.is_none(),
            ),
        );
        say(
            "x3",
            format!(
                "keystrokes_before_first_foreground_change={} attributed_to_child={} \
                 foreground_events_seen_after={} keystrokes_after_first_change={} \
                 with_empty_app_after={} attributed_to_child_after={} child_exit_ok={}",
                keys(&before),
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
        let evidence: String = format!(
            "TF x3 keystrokes_before_first_foreground_change_with_empty_app={} \
            attributed_to_child={}",
            empty_app(&before),
            child_app(&before)
        );
        println!("{evidence}");
        assert!(
            keys(&before) == 5 && child_app(&before) == 5 && empty_app(&before) == 0,
            "{evidence}"
        );
    });
}

fn image_name(pid: u32) -> String {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: OpenProcess takes plain values; the handle is closed once below.
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return "unreadable".to_string();
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` and `len` are live, `len` holds the length of `buf`, and
    // `handle` is open for the call.
    let read = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: `handle` is the handle that OpenProcess returned above.
    unsafe {
        let _ = CloseHandle(handle);
    }
    if read.is_err() {
        return "unreadable".to_string();
    }
    file_name(&String::from_utf16_lossy(&buf[..len as usize])).to_string()
}

fn class_of_window(hwnd: HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buf = [0u16; 128];
    // SAFETY: `buf` is a live buffer whose length the call takes.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)])
}

fn window_sample(hwnd: HWND) -> String {
    use windows::core::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;
    unsafe extern "system" fn each(child: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the address of the vector that the caller holds
        // across the enumeration on this thread.
        unsafe { (*(found.0 as *mut Vec<HWND>)).push(child) };
        BOOL::from(true)
    }
    let mut children: Vec<HWND> = Vec::new();
    // SAFETY: the callback has the WNDENUMPROC signature and writes `children`
    // through a pointer made from an exclusive borrow, only while this
    // enumeration runs on this thread; nothing else touches `children` meanwhile.
    unsafe {
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(each),
            LPARAM(&mut children as *mut Vec<HWND> as isize),
        );
    }
    let own = window_pid(hwnd);
    let mut others: Vec<String> = children
        .iter()
        .filter(|child| window_pid(**child) != own)
        .map(|child| {
            format!(
                "{}:{}",
                image_name(window_pid(*child)),
                class_of_window(*child)
            )
        })
        .collect();
    others.sort();
    others.dedup();
    format!(
        "sample foreground_class={} foreground_image={} child_windows={} children_of_other_processes={}",
        class_of_window(hwnd),
        image_name(own),
        children.len(),
        if others.is_empty() { "none".to_string() } else { others.join(",") },
    )
}

fn processes_named(image: &str) -> usize {
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
        .count()
}

fn foreground_without_input(hwnd: HWND, own: bool) -> (bool, &'static str) {
    // SAFETY: the calls take a window handle by value.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
    pump_thread(Duration::from_millis(200));
    if foreground_window() == hwnd {
        return (true, "SetForegroundWindow");
    }
    let front_thread = {
        let mut pid = 0u32;
        // SAFETY: `pid` is a live u32 that the call writes into.
        unsafe { GetWindowThreadProcessId(foreground_window(), Some(&mut pid)) }
    };
    // SAFETY: GetCurrentThreadId takes no argument.
    let this_thread = unsafe { GetCurrentThreadId() };
    if front_thread != 0 && front_thread != this_thread {
        // SAFETY: the input of the two threads is joined only around the two
        // window calls and detached again before the block ends.
        unsafe {
            let _ = AttachThreadInput(this_thread, front_thread, true);
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(this_thread, front_thread, false);
        }
    }
    pump_thread(Duration::from_millis(200));
    if foreground_window() == hwnd {
        return (true, "attach-thread-input");
    }
    if own {
        // SAFETY: ShowWindow takes this thread's window handle by value.
        unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        pump_thread(Duration::from_millis(200));
    }
    (foreground_window() == hwnd, "minimize-restore")
}

fn top_windows() -> Vec<HWND> {
    use windows::core::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the address of the vector that the caller holds
        // across the enumeration on this thread.
        unsafe { (*(found.0 as *mut Vec<HWND>)).push(hwnd) };
        BOOL::from(true)
    }
    let mut all: Vec<HWND> = Vec::new();
    // SAFETY: the callback has the WNDENUMPROC signature and writes `all`
    // through a pointer made from an exclusive borrow, only while this
    // enumeration runs on this thread; nothing else touches `all` meanwhile.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut Vec<HWND> as isize));
    }
    all
}

fn settings_window() -> Option<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
    top_windows().into_iter().find(|hwnd| {
        // SAFETY: IsWindowVisible reads a handle value.
        let visible = unsafe { IsWindowVisible(*hwnd) }.as_bool();
        visible
            && (image_name(window_pid(*hwnd)).eq_ignore_ascii_case("SystemSettings.exe")
                || class_of_window(*hwnd) == "ApplicationFrameWindow"
                    && window_sample(*hwnd).contains("SystemSettings.exe"))
    })
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x14_foreground_app_of_a_hosted_app() {
    use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
    with_deadline("x14", Duration::from_secs(150), || {
        env_report("x14");
        let anchor = TestWindow::create(&normal_window(
            "HalvoVerifyAnchor",
            "halvo-verify-anchor",
            60,
            60,
        ))
        .expect("the anchor window");
        pump_thread(Duration::from_millis(300));
        let before = window_sample(foreground_window());
        let (anchor_front, anchor_method) = foreground_without_input(anchor.hwnd, true);
        // SAFETY: AllowSetForegroundWindow takes a plain value.
        let allowed = unsafe { AllowSetForegroundWindow(ASFW_ANY) }.is_ok();
        say(
            "x14",
            format!(
                "before_anchor {before} anchor_foreground={anchor_front} \
                 anchor_method={anchor_method} allow_set_foreground_any={allowed}"
            ),
        );
        let (sink, events) = capture(4096);
        let mut foreground = ForegroundAppSource::new();
        let start = foreground.begin(SourceContext::new(sink.clone()));
        let launched = Command::new("cmd.exe")
            .args(["/c", "start", "ms-settings:"])
            .status()
            .is_ok_and(|status| status.success());
        let mut seen = Vec::new();
        let began = Instant::now();
        let mut sampled = Instant::now();
        let mut settings_front = false;
        let mut brought = None;
        while began.elapsed() < Duration::from_secs(15) {
            pump_thread(Duration::from_millis(100));
            seen.extend(events.drain_for(Duration::from_millis(100)));
            if let Some(frame) = settings_window() {
                if foreground_window() == frame {
                    settings_front = true;
                } else if brought.is_none() && began.elapsed() >= Duration::from_secs(3) {
                    brought = Some(foreground_without_input(frame, false));
                }
            }
            if sampled.elapsed() >= Duration::from_secs(1) {
                sampled = Instant::now();
                say("x14", window_sample(foreground_window()));
            }
        }
        seen.extend(events.drain_for(Duration::from_millis(500)));
        let settings_processes = processes_named("SystemSettings.exe");
        let foreground_now = current_foreground_app().unwrap_or_default();
        let _ = stop_counted(&mut foreground, "foreground");
        let _ = Command::new("taskkill.exe")
            .args(["/IM", "SystemSettings.exe", "/F"])
            .output();
        drop(anchor);
        summarize("x14", "after-opening-settings", &seen);
        let named = |file: &str| {
            seen.iter()
                .filter(|e| file_name(&e.foreground_app).eq_ignore_ascii_case(file))
                .count()
        };
        let frame_host = named("ApplicationFrameHost.exe");
        let settings = named("SystemSettings.exe");
        say(
            "x14",
            format!(
                "foreground_start={} settings_launched={launched} \
                 settings_processes_running={settings_processes} \
                 settings_window_was_foreground={settings_front} \
                 brought_to_front_by_the_fixture={brought:?} \
                 foreground_now_is_frame_host={} foreground_now_is_settings={} \
                 events_naming_applicationframehost={frame_host}",
                show(&start),
                file_name(&foreground_now).eq_ignore_ascii_case("ApplicationFrameHost.exe"),
                file_name(&foreground_now).eq_ignore_ascii_case("SystemSettings.exe"),
            ),
        );
        assert!(start.is_ok(), "the foreground source must start");
        assert!(
            settings_processes >= 1,
            "INCONCLUSIVE: the Settings app did not start on this machine"
        );
        assert!(
            settings_front,
            "INCONCLUSIVE: the Settings window never came to the front by an allowed call"
        );
        let evidence: String = format!("TF x14 events_naming_systemsettings={settings}");
        println!("{evidence}");
        assert!(settings >= 1, "{evidence}");
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x2_window_title_events_for_other_windows_and_controls() {
    with_deadline("x2", Duration::from_secs(90), || {
        env_report("x2");
        let (sink, events) = capture(8192);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut title = set.gated_title(WindowTitleSource::new());
        let start = title.begin(SourceContext::new(sink.clone()));
        let mut child = spawn_child("scope", 0);
        let clear = content_reads_clear("x2", &gate, GATE_CLEAR_WAIT);
        let mut seen = Vec::new();
        let exit = loop {
            seen.extend(events.drain_for(Duration::from_millis(100)));
            if let Some(status) = child.try_wait() {
                break status;
            }
        };
        seen.extend(events.drain_for(Duration::from_secs(1)));
        let _ = stop_counted(&mut title, "window_title");
        gate_readings("x2", &gate);
        child.print_output();
        summarize("x2", "scope", &seen);
        let with_size = |len: u64| {
            seen.iter()
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
                 events_for_child_control_text={}",
                show(&start),
                exit.success(),
                with_size(30),
                with_size(40),
            ),
        );
        assert!(start.is_ok(), "the window-title source must start");
        assert!(exit.success(), "the child process must finish");
        assert!(clear, "the gate never read Clear");
        assert!(
            with_size(30) >= 1,
            "INCONCLUSIVE: no event for the foreground window's own title, so the other counts say nothing"
        );
        let evidence: String = format!(
            "TF x2 events_for_child_control_text={} events_for_background_window_title={} \
            events_for_never_shown_window_title={}",
            with_size(40),
            with_size(50),
            with_size(60),
        );
        println!("{evidence}");
        assert!(
            with_size(40) == 0 && with_size(50) == 0 && with_size(60) == 0,
            "{evidence}"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn title_event_on_a_switch_to_a_window_with_a_fixed_title() {
    with_deadline("fixed-title", Duration::from_secs(90), || {
        env_report("fixed-title");
        let (sink, events) = capture(8192);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut title = set.gated_title(WindowTitleSource::new());
        let start = title.begin(SourceContext::new(sink.clone()));
        let mut child = spawn_child("cycle", 4);
        let child_pid = child.id();
        let clear = content_reads_clear("fixed-title", &gate, GATE_CLEAR_WAIT);
        let fixed_len = FIXED_TITLE.len() as i32;
        let mut seen = Vec::new();
        let mut fixed_window_was_foreground = false;
        let exit = loop {
            seen.extend(events.drain_for(Duration::from_millis(100)));
            let hwnd = foreground_window();
            fixed_window_was_foreground |=
                window_pid(hwnd) == child_pid && title_len(hwnd) == fixed_len;
            if let Some(status) = child.try_wait() {
                break status;
            }
        };
        seen.extend(events.drain_for(Duration::from_secs(1)));
        let _ = stop_counted(&mut title, "window_title");
        gate_readings("fixed-title", &gate);
        child.print_output();
        summarize("fixed-title", "cycle", &seen);
        let titles_of = |wanted: &dyn Fn(u64) -> bool| {
            seen.iter()
                .filter(|e| {
                    e.modality == Modality::WindowTitle
                        && is_child_app(&e.foreground_app)
                        && wanted(e.segment_ref.byte_len)
                })
                .count()
        };
        let scripted = titles_of(&|len| CHILD_TITLE_LENGTHS.contains(&len));
        let fixed = titles_of(&|len| len == FIXED_TITLE.len() as u64);
        say(
            "fixed-title",
            format!(
                "title_start={} child_exit_ok={} fixed_title_bytes={fixed_len} \
                 fixed_title_window_was_foreground={fixed_window_was_foreground} \
                 title_events_with_a_scripted_title_size={scripted}",
                show(&start),
                exit.success(),
            ),
        );
        assert!(start.is_ok(), "the window-title source must start");
        assert!(exit.success(), "the child process must finish");
        assert!(clear, "the gate never read Clear");
        assert!(
            fixed_window_was_foreground,
            "INCONCLUSIVE: the window with the fixed title never came to the front"
        );
        assert!(
            scripted >= 1,
            "INCONCLUSIVE: no title event for the child's title changes"
        );
        let evidence: String = format!("TF fixed-title title_events_for_the_fixed_title={fixed}");
        println!("{evidence}");
        assert!(fixed >= 1, "{evidence}");
    });
}

fn title_len(hwnd: HWND) -> i32 {
    // SAFETY: GetWindowTextLengthW reads the length of a window's text; a
    // handle that is no longer a window gives 0.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(hwnd) }
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x15_a_held_key() {
    with_deadline("x15", Duration::from_secs(60), || {
        const REPEATS: u32 = 6;
        env_report("x15");
        source_built_by_test("x15", "keyboard");
        let (sink, events) = capture(4096);
        let fields = FieldWindow::open("x15_a_held_key", Field::Plain);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        gate_reads_clear("x15", Some(&fields), &keyboard);
        let mut accepted = 0;
        for _ in 0..REPEATS {
            accepted += send(&[key_input(VK_A, false)]);
            std::thread::sleep(Duration::from_millis(30));
        }
        accepted += send(&[key_input(VK_A, true)]);
        let seen = events.drain_for(Duration::from_secs(1));
        let _ = stop_counted(&mut keyboard, "keyboard");
        let keystrokes = count(&seen, Modality::Keystroke);
        say(
            "x15",
            format!(
                "keyboard_start={} physical_presses=1 key_down_packets={REPEATS} \
                 key_up_packets=1 sendinput_events_accepted={accepted}",
                show(&start),
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert_eq!(
            accepted,
            REPEATS + 1,
            "INCONCLUSIVE: SendInput did not insert every packet"
        );
        assert!(keystrokes >= 1, "INCONCLUSIVE: no keystroke event at all");
        let evidence: String = format!("TF x15 keystroke_events={keystrokes}");
        println!("{evidence}");
        assert!(keystrokes == 1, "{evidence}");
    });
}

fn unix_millis_of(event: &Seen) -> u128 {
    unix_millis(event.key_time.unwrap_or(event.captured_at.wall_clock))
}

fn keystrokes_inside(events: &[Seen], intervals: &[(u128, u128)], margin: u128) -> usize {
    events
        .iter()
        .filter(|e| e.modality == Modality::Keystroke)
        .filter(|e| {
            let at = unix_millis_of(e);
            intervals
                .iter()
                .any(|(start, end)| at > start + margin && at + margin < *end)
        })
        .count()
}

fn tab_then_letter() -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_TAB};
    send(&[
        key_input(VK_TAB, false),
        key_input(VK_TAB, true),
        key_input(VK_A, false),
        key_input(VK_A, true),
    ])
}

fn letters_while(span: Duration, every: Duration) -> (u32, Vec<u128>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_A;
    let began = Instant::now();
    let mut accepted = 0;
    let mut sent = Vec::new();
    while began.elapsed() < span {
        sent.push(unix_millis(SystemTime::now()));
        accepted += send(&[key_input(VK_A, false), key_input(VK_A, true)]);
        std::thread::sleep(every);
    }
    (accepted, sent)
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn password_field_fixture_native_dialog_phases_a_to_f() {
    with_deadline("fixture-native", Duration::from_secs(200), || {
        const LETTERS: u32 = 5;
        env_report("fixture-native");
        source_built_by_test("fixture-native", "keyboard");
        let fields = FieldWindow::open("fixture-native", Field::Plain);
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        let keys = |events: &[Seen]| count(events, Modality::Keystroke);

        let _ = seam::take();
        let _ = seam::take_marks();
        let focused_a = fields.focus(Field::Plain);
        let _ = events.drain_for(Duration::from_millis(500));
        let _ = seam::take();
        type_letters_slowly(LETTERS);
        let a = keys(&events.drain_for(Duration::from_secs(1)));
        control_reads(
            "fixture-native",
            "phase-A",
            &seam::take_marks(),
            &seam::take(),
        );
        let gate_a = keyboard.field_state();

        let focused_b = fields.focus(Field::Password);
        let _ = events.drain_for(Duration::from_millis(500));
        let password_before_b = fields.text_len(Field::Password);
        type_letters_slowly(LETTERS);
        let b = keys(&events.drain_for(Duration::from_secs(1)));
        let typed_b = fields.text_len(Field::Password) - password_before_b;
        let gate_b = keyboard.field_state();
        let withheld_b = keyboard.health().counters.keys_withheld;

        fields.focus(Field::Plain);
        let _ = events.drain_for(Duration::from_millis(500));
        let span = Duration::from_secs(6);
        fields.start_wander(span);
        let (accepted_c, sent_c) = letters_while(span, Duration::from_millis(60));
        let intervals = fields.wander_intervals(span);
        let seen_c = events.drain_for(Duration::from_secs(1));
        let c = keys(&seen_c);
        let c_inside = keystrokes_inside(&seen_c, &intervals, 30);

        let focused_d = fields.focus(Field::Password);
        let _ = events.drain_for(Duration::from_millis(300));
        let revealed = fields.reveal();
        let _ = events.drain_for(Duration::from_millis(300));
        let clicked_d = fields.click(Field::Password);
        let _ = events.drain_for(Duration::from_millis(300));
        let gate_d = keyboard.field_state();
        let still_focused_d = fields.password_has_focus();
        type_letters_slowly(LETTERS);
        let d = keys(&events.drain_for(Duration::from_secs(1)));

        let (e, e_control_before, e_control_after, e_masked) =
            fields.phase_e("fixture-native", &events, &keyboard);

        drop(fields);
        let mut f = 0;
        for round in 0..10 {
            let opened = FieldWindow::open("fixture-native-f", Field::Password);
            type_letters_slowly(1);
            f += keys(&events.drain_for(Duration::from_millis(500)));
            if round == 9 {
                say(
                    "fixture-native",
                    format!(
                        "phase=F password_characters_last_round={}",
                        opened.text_len(Field::Password)
                    ),
                );
            }
        }
        let _ = stop_counted(&mut keyboard, "keyboard");
        say(
            "fixture-native",
            format!(
                "keyboard_start={} phase=A plain_field_has_focus={focused_a} letters_sent={LETTERS} \
                 keystroke_events={a} gate={gate_a:?}",
                show(&start)
            ),
        );
        say(
            "fixture-native",
            format!(
                "phase=B password_field_has_focus={focused_b} letters_sent={LETTERS} \
                 characters_in_password_field={typed_b} keystroke_events={b} gate={gate_b:?} \
                 keys_withheld={withheld_b}"
            ),
        );
        say(
            "fixture-native",
            format!(
                "phase=C letters_sent={} sendinput_events_accepted={accepted_c} focus_moves_to_password={} \
                 keystroke_events={c} keystroke_events_inside_password_focus={c_inside}",
                sent_c.len(),
                intervals.len(),
            ),
        );
        say(
            "fixture-native",
            format!(
                "phase=D password_field_has_focus={focused_d} revealed={revealed} \
                 clicked_inside={clicked_d} focus_stayed={still_focused_d} gate_after_the_click={gate_d:?} \
                 keystroke_events={d}"
            ),
        );
        say(
            "fixture-native",
            format!(
                "phase=E rounds={} counted_rounds={} letters_recorded={} tabs_recorded={} other_recorded={} \
                 letters_landed_in_the_password_field={} control_before={e_control_before} \
                 control_after={e_control_after} password_edit_masked={e_masked} \
                 rounds_not_withheld_by_the_password_field={}",
                e.rounds, e.counted, e.letters_recorded, e.tabs_recorded, e.other_recorded, e.letters_landed,
                e.rounds_not_withheld_by_the_password_field,
            ),
        );
        say(
            "fixture-native",
            format!("phase=F rounds=10 keystroke_events={f}"),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert!(
            focused_a
                && focused_b
                && focused_d
                && revealed
                && clicked_d
                && still_focused_d
                && typed_b == LETTERS as i32,
            "INCONCLUSIVE: the scripted keys did not reach the fields"
        );
        assert!(
            !intervals.is_empty() && !sent_c.is_empty(),
            "INCONCLUSIVE: phase C moved no focus or typed nothing"
        );
        assert_eq!(a, LETTERS as usize, "phase A: every key of the plain field");
        assert_eq!(b, 0, "phase B: a key of the password field was recorded");
        assert!(withheld_b, "phase B: keys_withheld must read yes");
        assert_eq!(
            c_inside, 0,
            "phase C: a key typed while the password field had the focus was recorded"
        );
        assert!(c >= 1, "phase C: no key of the plain field was recorded");
        assert_eq!(
            d, 0,
            "phase D: a key of the revealed password field was recorded"
        );
        assert_eq!(
            gate_d,
            Some(halvo_capture::secure_input::windows::FieldState::Password),
            "phase D: after a click inside the revealed field the gate did not keep Password"
        );
        assert_eq!(
            e.letters_recorded, 0,
            "phase E: a key typed right after Tab into the password field was recorded"
        );
        assert!(
            e_masked,
            "INCONCLUSIVE: phase E's password edit was not masked at its start"
        );
        assert_eq!(
            e.rounds_not_withheld_by_the_password_field, 0,
            "phase E: a key of a counted round was not withheld by a reading of the password field"
        );
        assert_eq!(
            e.other_recorded, 0,
            "phase E: a key of another class than Tab was recorded"
        );
        assert!(
            e_control_before > 0 && e_control_after > 0,
            "phase E: a control recorded no letter of the plain field"
        );
        assert_eq!(
            e.counted, e.rounds,
            "INCONCLUSIVE: phase E: not every round counted"
        );
        assert_eq!(
            f, 0,
            "phase F: a key typed into a password field that had the focus at open was recorded"
        );
    });
}

const FIXTURE_PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>halvo-fixture</title></head>
<body>
<input id="plain" type="text">
<input id="password" type="password">
<input id="masked" type="text" style="-webkit-text-security: disc">
<script>
const query = new URLSearchParams(location.search);
const phase = query.get('phase') || 'A';
const run = query.get('run') || '0';
const counts = { plain: 0, password: 0, masked: 0 };
let seq = '';
function show() {
  document.title = 'halvo-fixture ' + phase + run + ' plain=' + counts.plain + ' password=' +
    counts.password + ' masked=' + counts.masked + ' seq=' + seq.slice(-150) + ' end';
}
for (const id of ['plain', 'password', 'masked']) {
  document.getElementById(id).addEventListener('keydown', (e) => {
    if (e.repeat) return;
    counts[id]++;
    seq += id === 'password' ? 'P' : (id === 'plain' ? 'T' : 'M');
    show();
  });
}
const focusOn = (id) => document.getElementById(id).focus();
if (phase === 'A' || phase === 'C' || phase === 'E') focusOn('plain');
if (phase === 'B' || phase === 'D' || phase === 'F') focusOn('password');
if (phase === 'M') focusOn('masked');
if (phase === 'C') {
  let x = 7;
  const hop = () => {
    x = (x * 1103515245 + 12345) % 2147483648;
    focusOn(document.activeElement.id === 'plain' ? 'password' : 'plain');
    setTimeout(hop, 150 + (x % 450));
  };
  setTimeout(hop, 1500);
}
if (phase === 'D') setTimeout(() => { document.getElementById('password').type = 'text'; }, 800);
if (phase === 'E') {
  document.getElementById('password').addEventListener('keyup', (e) => {
    if (e.key !== 'Tab') setTimeout(() => focusOn('plain'), 250);
  });
}
show();
</script>
</body></html>
"#;

struct Browser {
    name: &'static str,
    paths: &'static [&'static str],
    chromium: bool,
    renamed_copy_keys_reach_the_page: bool,
}

const CHROME: Browser = Browser {
    name: "chrome",
    paths: &[
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ],
    chromium: true,
    renamed_copy_keys_reach_the_page: true,
};

const EDGE: Browser = Browser {
    name: "edge",
    paths: &[
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ],
    chromium: true,
    renamed_copy_keys_reach_the_page: true,
};

const FIREFOX: Browser = Browser {
    name: "firefox",
    paths: &[
        r"C:\Program Files\Mozilla Firefox\firefox.exe",
        r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe",
    ],
    chromium: false,
    renamed_copy_keys_reach_the_page: false,
};

struct PageCounts {
    plain: u32,
    password: u32,
    masked: u32,
    seq: String,
}

fn page_counts(title: &str) -> Option<PageCounts> {
    let field = |key: &str| -> Option<&str> {
        let at = title.find(key)? + key.len();
        title[at..].split(' ').next()
    };
    Some(PageCounts {
        plain: field(" plain=")?.parse().ok()?,
        password: field(" password=")?.parse().ok()?,
        masked: field(" masked=")?.parse().ok()?,
        seq: field(" seq=")?.to_string(),
    })
}

fn window_titled(prefix: &str) -> Option<(HWND, String)> {
    use windows::core::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW};
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
        let mut buf = [0u16; 2048];
        // SAFETY: `buf` is a live buffer whose length the call takes, and
        // `found` is the address of the vector that the caller holds across
        // the enumeration on this thread.
        unsafe {
            let len = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)]);
            (*(found.0 as *mut Vec<(HWND, String)>)).push((hwnd, title));
        }
        BOOL::from(true)
    }
    let mut all: Vec<(HWND, String)> = Vec::new();
    // SAFETY: the callback has the WNDENUMPROC signature and writes `all`
    // through a pointer made from an exclusive borrow, only while this
    // enumeration runs on this thread; nothing else touches `all` meanwhile.
    unsafe {
        let _ = EnumWindows(
            Some(each),
            LPARAM(&mut all as *mut Vec<(HWND, String)> as isize),
        );
    }
    all.into_iter().find(|(_, title)| title.starts_with(prefix))
}

fn title_of(hwnd: HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let mut buf = [0u16; 2048];
    // SAFETY: `buf` is a live buffer whose length the call takes.
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)])
}

struct BrowserRun<'a> {
    browser: &'a Browser,
    exe: String,
    profile: tempfile::TempDir,
    page: String,
    runs: u32,
}

impl<'a> BrowserRun<'a> {
    fn new(browser: &'a Browser) -> Option<BrowserRun<'a>> {
        let exe = browser
            .paths
            .iter()
            .find(|path| Path::new(path).is_file())?;
        let profile = scratch_dir();
        if !browser.chromium {
            let prefs = "user_pref(\"browser.shell.checkDefaultBrowser\", false);\n\
                         user_pref(\"browser.aboutwelcome.enabled\", false);\n\
                         user_pref(\"datareporting.policy.dataSubmissionEnabled\", false);\n\
                         user_pref(\"browser.startup.homepage_override.mstone\", \"ignore\");\n\
                         user_pref(\"startup.homepage_welcome_url\", \"\");\n\
                         user_pref(\"trailhead.firstrun.didSeeAboutWelcome\", true);\n";
            std::fs::write(profile.path().join("user.js"), prefs).ok()?;
        }
        let page_path = profile.path().join("fixture.html");
        std::fs::write(&page_path, FIXTURE_PAGE).ok()?;
        let page = format!("file:///{}", page_path.to_string_lossy().replace('\\', "/"));
        Some(BrowserRun {
            browser,
            exe: exe.to_string(),
            profile,
            page,
            runs: 0,
        })
    }

    fn open(&mut self, phase: char) -> Option<HWND> {
        self.runs += 1;
        let url = format!("{}?phase={phase}&run={}", self.page, self.runs);
        let profile = self.profile.path().to_string_lossy().into_owned();
        let args: Vec<String> = if self.browser.chromium {
            vec![
                format!("--user-data-dir={profile}"),
                "--no-first-run".to_string(),
                "--no-default-browser-check".to_string(),
                "--disable-features=Translate".to_string(),
                "--new-window".to_string(),
                url,
            ]
        } else {
            vec![
                "-profile".to_string(),
                profile,
                "-new-window".to_string(),
                url,
            ]
        };
        let _ = Command::new(&self.exe)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let prefix = format!("halvo-fixture {phase}{} ", self.runs);
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(30) {
            if let Some((hwnd, _)) = window_titled(&prefix) {
                for _ in 0..10 {
                    if foreground_window() == hwnd {
                        break;
                    }
                    send(&[key_input(VK_MENU, false)]);
                    set_foreground(hwnd);
                    send(&[key_input(VK_MENU, true)]);
                    std::thread::sleep(Duration::from_millis(200));
                }
                std::thread::sleep(Duration::from_millis(1200));
                return Some(hwnd);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        None
    }
}

impl Drop for BrowserRun<'_> {
    fn drop(&mut self) {
        let image = Path::new(&self.exe)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = Command::new("taskkill.exe")
            .args(["/IM", &image, "/F", "/T"])
            .output();
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn browser_phase(
    test: &str,
    run: &mut BrowserRun,
    keyboard: &RawInputSource,
    events: &Events,
    phase: char,
    typing: impl FnOnce() -> Vec<u128>,
) -> (usize, Vec<Seen>, Option<PageCounts>, Vec<u128>) {
    let hwnd = run.open(phase);
    let foreground = hwnd.is_some_and(|hwnd| foreground_window() == hwnd);
    let _ = events.drain_for(Duration::from_millis(300));
    let sent = typing();
    let seen = events.drain_for(Duration::from_millis(1500));
    let counts = hwnd.and_then(|hwnd| page_counts(&title_of(hwnd)));
    let keystrokes = count(&seen, Modality::Keystroke);
    say(
        test,
        format!(
            "browser={} phase={phase} page_window_found={} page_window_foreground={foreground} \
             page_plain={} page_password={} page_masked={} keystroke_events={keystrokes} gate={:?}",
            run.browser.name,
            hwnd.is_some(),
            counts.as_ref().map_or(-1, |c| i64::from(c.plain)),
            counts.as_ref().map_or(-1, |c| i64::from(c.password)),
            counts.as_ref().map_or(-1, |c| i64::from(c.masked)),
            keyboard.field_state(),
        ),
    );
    (keystrokes, seen, counts, sent)
}

fn letters(n: u32) -> Vec<u128> {
    let mut sent = Vec::new();
    for _ in 0..n {
        sent.push(unix_millis(SystemTime::now()));
        type_letters_slowly(1);
    }
    sent
}

fn password_key_events(seen: &[Seen], sent: &[u128], seq: &str) -> usize {
    let fields: Vec<char> = seq.chars().collect();
    let offset = sent.len().saturating_sub(fields.len());
    seen.iter()
        .filter(|e| e.modality == Modality::Keystroke)
        .filter(|e| {
            let at = unix_millis_of(e);
            let nearest = sent
                .iter()
                .enumerate()
                .min_by_key(|(_, t)| t.abs_diff(at))
                .map(|(i, _)| i);
            nearest
                .and_then(|i| i.checked_sub(offset))
                .and_then(|i| fields.get(i))
                .is_some_and(|field| *field != 'T')
        })
        .count()
}

fn browser_fixture(test: &'static str, browser: &'static Browser) {
    with_deadline(test, Duration::from_secs(300), || {
        env_report(test);
        source_built_by_test(test, "keyboard");
        let mut run = BrowserRun::new(browser);
        say(
            test,
            format!("browser={} installed={}", browser.name, run.is_some()),
        );
        let run = run
            .as_mut()
            .expect("the browser is not installed on this runner: FAIL");
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        let unknown_class = !browser.chromium;
        let control = |which: &str| -> usize {
            let _ = seam::take_marks();
            let fields = FieldWindow::open(test, Field::Plain);
            let focused = fields.focus(Field::Plain);
            let _ = events.drain_for(Duration::from_millis(600));
            let _ = seam::take();
            type_letters_slowly(5);
            let recorded = count(
                &events.drain_for(Duration::from_secs(1)),
                Modality::Keystroke,
            );
            control_reads(test, which, &seam::take_marks(), &seam::take());
            say(
                test,
                format!(
                    "control={which} window=native-plain-field foreground={} focused={focused} \
                     letters_sent=5 keystroke_events={recorded}",
                    fields.foreground
                ),
            );
            recorded
        };
        let control_before = if unknown_class { control("before") } else { 5 };

        let _ = browser_phase(test, run, &keyboard, &events, 'W', Vec::new);
        let _ = seam::take();
        let _ = seam::take_marks();
        let (a, _, a_page, _) = browser_phase(test, run, &keyboard, &events, 'A', || {
            let _ = seam::take();
            letters(5)
        });
        control_reads(test, "phase-A", &seam::take_marks(), &seam::take());
        let gate_a = keyboard.field_state();
        let (b, _, b_page, _) = browser_phase(test, run, &keyboard, &events, 'B', || letters(5));
        let gate_b = keyboard.field_state();
        let withheld_b = keyboard.health().counters.keys_withheld;
        let (c, c_seen, c_page, c_sent) = browser_phase(test, run, &keyboard, &events, 'C', || {
            std::thread::sleep(Duration::from_millis(1000));
            letters_while(Duration::from_secs(6), Duration::from_millis(60)).1
        });
        let c_seq = c_page.as_ref().map(|p| p.seq.clone()).unwrap_or_default();
        let c_password = password_key_events(&c_seen, &c_sent, &c_seq);
        let (d, _, d_page, _) = browser_phase(test, run, &keyboard, &events, 'D', || {
            std::thread::sleep(Duration::from_millis(1200));
            letters(5)
        });
        let _ = seam::take();
        let (e, _, e_page, _) = browser_phase(test, run, &keyboard, &events, 'E', || {
            for _ in 0..stall::rounds_for(1, 20) {
                tab_then_letter();
                std::thread::sleep(Duration::from_millis(500));
                send(&[key_input(
                    windows::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT,
                    false,
                )]);
                send(&[key_input(
                    windows::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT,
                    true,
                )]);
            }
            Vec::new()
        });
        let e_decided = seam::take();
        let mut sorted: Vec<&seam::Decided> = e_decided.iter().collect();
        sorted.sort_by_key(|key| key.os_tick);
        let mut round = 0;
        let mut last: Option<u32> = None;
        for key in sorted {
            if last.is_some_and(|last| key.os_tick.wrapping_sub(last) > 300) {
                round += 1;
            }
            last = Some(key.os_tick);
            say(
                test,
                format!(
                    "phase=E round={round} class={:?} verdict={:?} rule={:?}",
                    key.class, key.verdict, key.rule
                ),
            );
        }
        let recorded = |class: seam::KeyClass| {
            e_decided
                .iter()
                .filter(|key| key.class == class && key.verdict == seam::Verdict::Recorded)
                .count()
        };
        let (e_letters, e_tabs) = (
            recorded(seam::KeyClass::Printable),
            recorded(seam::KeyClass::Tab),
        );
        let e_other = e.saturating_sub(e_letters + e_tabs);
        say(
            test,
            format!(
                "phase=E letters_recorded={e_letters} tabs_recorded={e_tabs} other_classes_recorded={e_other} \
                 keys_decided_by_the_gate={}",
                e_decided.len()
            ),
        );
        let mut f = 0;
        let mut f_password = 0;
        for _ in 0..stall::rounds_for(1, 10) {
            let (keys, _, page, _) =
                browser_phase(test, run, &keyboard, &events, 'F', || letters(1));
            f += keys;
            f_password += page.map_or(0, |p| p.password);
        }
        let control_after = if unknown_class { control("after") } else { 5 };
        let _ = stop_counted(&mut keyboard, "keyboard");
        let page = |p: &Option<PageCounts>| {
            p.as_ref()
                .map_or((0, 0, 0), |p| (p.plain, p.password, p.masked))
        };
        say(
            test,
            format!(
                "keyboard_start={} browser={} detector_class={} phase_A_plain_keys={} phase_A_events={a} \
                 phase_B_password_keys={} phase_B_events={b} \
                 keys_withheld={withheld_b} phase_C_keys={} phase_C_events={c} phase_C_password_key_events={c_password} \
                 phase_D_keys={} phase_D_events={d} phase_E_password_keys={} phase_E_events={e} \
                 phase_F_password_keys={f_password} phase_F_events={f}",
                show(&start),
                browser.name,
                if unknown_class { "unknown" } else { "read" },
                page(&a_page).0,
                page(&b_page).1,
                c_seq.len(),
                page(&d_page).0 + page(&d_page).1,
                page(&e_page).1,
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert!(
            page(&a_page).0 == 5 && page(&b_page).1 == 5,
            "INCONCLUSIVE: the page did not take the scripted keys"
        );
        assert_eq!(b, 0, "phase B: a key of the password input was recorded");
        assert!(withheld_b, "phase B: keys_withheld must read yes");
        assert_eq!(
            c_password, 0,
            "phase C: a key typed into the password input was recorded"
        );
        assert_eq!(
            d, 0,
            "phase D: a key of the revealed password input was recorded"
        );
        assert!(
            page(&e_page).1 >= 1,
            "INCONCLUSIVE: phase E typed nothing into the password input"
        );
        assert_eq!(
            f, 0,
            "phase F: a key of the autofocus password input was recorded"
        );
        if unknown_class {
            say(
                test,
                format!(
                    "gate_while_the_plain_input_had_the_focus={gate_a:?} \
                     gate_while_the_password_input_had_the_focus={gate_b:?} \
                     control_before={control_before} control_after={control_after}"
                ),
            );
            assert!(
                control_before == 5 && control_after == 5,
                "the control in a plain field of a kind that reads Clear did not record its letters: \
                 the source recorded nothing"
            );
            {
                use halvo_capture::secure_input::windows::FieldState;
                assert_eq!(
                    gate_a,
                    Some(FieldState::Unknown),
                    "the gate did not read Unknown while the page's plain input had the focus"
                );
                assert!(
                    matches!(gate_b, Some(FieldState::Unknown | FieldState::Password)),
                    "the gate read neither Unknown nor Password while the page's password input had the focus"
                );
            }
            assert_eq!(
                a + c + e,
                0,
                "a Firefox focus must read Unknown and give no keystroke event"
            );
        } else {
            assert_eq!(a, 5, "phase A: every key of the text input");
            assert!(c >= 1, "phase C: no key of the text input was recorded");
            assert_eq!(
                e_letters, 0,
                "phase E: a key after Tab into the password input was recorded"
            );
        }
    });
}

fn style_mask_phase(test: &'static str, browser: &'static Browser) -> (usize, String) {
    let out = Arc::new(Mutex::new((0usize, String::new())));
    let kept = Arc::clone(&out);
    with_deadline(test, Duration::from_secs(240), move || {
        env_report(test);
        source_built_by_test(test, "keyboard");
        let mut run = BrowserRun::new(browser);
        say(
            test,
            format!("browser={} installed={}", browser.name, run.is_some()),
        );
        let run = run
            .as_mut()
            .expect("the browser is not installed on this runner: FAIL");
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        let _ = browser_phase(test, run, &keyboard, &events, 'W', Vec::new);
        let (keys, _, page, _) = browser_phase(test, run, &keyboard, &events, 'M', || letters(5));
        let uia = seam::focused_element_properties();
        let _ = stop_counted(&mut keyboard, "keyboard");
        say(
            test,
            format!(
                "browser={} phase=M uia_of_the_focused_element {uia}",
                browser.name
            ),
        );
        say(
            test,
            format!(
                "browser={} phase=M keyboard_start={} masked_letters_landed={} keystroke_events={keys}",
                browser.name,
                show(&start),
                page.as_ref().map_or(-1, |p| i64::from(p.masked))
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert_eq!(
            page.map_or(0, |p| p.masked),
            5,
            "INCONCLUSIVE: the style-masked input did not take the 5 letters"
        );
        *kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = (keys, uia);
    });
    let taken = out
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    taken
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Chrome"]
fn password_field_fixture_chrome() {
    browser_fixture("fixture-chrome", &CHROME);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Edge"]
fn password_field_fixture_edge() {
    browser_fixture("fixture-edge", &EDGE);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop and Firefox"]
fn password_field_fixture_firefox() {
    browser_fixture("fixture-firefox", &FIREFOX);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x1_keystrokes_typed_into_a_password_field() {
    with_deadline("x1", Duration::from_secs(90), || {
        const LETTERS: u32 = 5;
        env_report("x1");
        source_built_by_test("x1", "keyboard");
        let fields = FieldWindow::open("x1", Field::Plain);
        let (sink, events) = capture(4096);
        let last_app = Arc::new(Mutex::new(Some(ATTRIBUTION.to_string())));
        let mut keyboard = RawInputSource::keyboard(last_app, PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));

        let plain_focused = fields.focus(Field::Plain);
        let plain_before = events.drain_for(Duration::from_millis(500));
        let plain_accepted = type_letters_slowly(LETTERS);
        let plain_typed = count(
            &events.drain_for(Duration::from_secs(1)),
            Modality::Keystroke,
        );
        let plain_characters = fields.text_len(Field::Plain);
        let plain_gate = keyboard.field_state();
        say(
            "x1",
            format!(
                "keyboard_start={} window_foreground={} method={} \
                 plain_field_has_focus={plain_focused} letters_sent={LETTERS} \
                 sendinput_key_events_accepted={plain_accepted} \
                 characters_in_plain_field={plain_characters} gate_in_plain_field={plain_gate:?} \
                 keystroke_events_before_typing={} plain_field_keystroke_events={plain_typed}",
                show(&start),
                fields.foreground,
                fields.method,
                count(&plain_before, Modality::Keystroke),
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert!(
            plain_focused && plain_characters == LETTERS as i32,
            "INCONCLUSIVE: the scripted keys did not reach the plain field"
        );
        assert_eq!(
            plain_typed, LETTERS as usize,
            "the keyboard source did not record the keys typed into the plain field"
        );

        let password_focused = fields.focus(Field::Password);
        let before = events.drain_for(Duration::from_millis(500));
        let accepted = type_letters_slowly(LETTERS);
        let during = events.drain_for(Duration::from_secs(1));
        let characters = fields.text_len(Field::Password);
        let password_gate = keyboard.field_state();
        let withheld = keyboard.health().counters.keys_withheld;
        let _ = stop_counted(&mut keyboard, "keyboard");
        let typed = count(&during, Modality::Keystroke);
        say(
            "x1",
            format!(
                "password_field_has_focus={password_focused} letters_sent={LETTERS} \
                 sendinput_key_events_accepted={accepted} \
                 characters_in_password_field={characters} gate_in_password_field={password_gate:?} \
                 keys_withheld={withheld} \
                 keystroke_events_before_typing_into_password_field={}",
                count(&before, Modality::Keystroke),
            ),
        );
        assert!(
            password_focused && characters == LETTERS as i32,
            "INCONCLUSIVE: the scripted keys did not reach the password field"
        );
        let evidence: String = format!(
            "TF x1 keystroke_events_while_typing_into_password_field={typed} \
            plain_field_keystroke_events={plain_typed}"
        );
        println!("{evidence}");
        assert!(typed == 0 && withheld, "{evidence}");
    });
}

macro_rules! defect {
    ($id:literal, $wanted:expr, $evidence:expr) => {{
        let evidence: String = $evidence;
        println!("{evidence}");
        if !($wanted) {
            panic!("KNOWN-DEFECT {}", $id);
        }
    }};
}

#[test]
#[ignore = "functional: was a known defect: W3-5 A2, a focus move that the page makes itself after holding its main thread"]
fn browser_self_move_after_a_held_main_thread_in_chrome() {
    let recorded = browser::self_move_phases("browser-self-chrome", "chrome");
    flipped!(
        "W3-5-A2",
        recorded == 0,
        format!(
            "TF browser-self-chrome password_letters_recorded_while_the_page_was_held={recorded}"
        )
    );
}

#[test]
#[ignore = "functional: was a known defect: W3-5 A2, a focus move that the page makes itself after holding its main thread"]
fn browser_self_move_after_a_held_main_thread_in_edge() {
    let recorded = browser::self_move_phases("browser-self-edge", "edge");
    flipped!(
        "W3-5-A2",
        recorded == 0,
        format!(
            "TF browser-self-edge password_letters_recorded_while_the_page_was_held={recorded}"
        )
    );
}

#[test]
#[ignore = "functional: was a known defect: W3-7 Q2 (i), a type swap on a page that holds its main thread"]
fn browser_swaps_on_a_held_main_thread_in_chrome() {
    let recorded = browser::held_swap_phases("browser-held-swap-chrome", "chrome");
    flipped!(
        "W3-7-Q2i",
        recorded == 0,
        format!("TF browser-held-swap-chrome password_letters_recorded_while_the_page_was_held={recorded}")
    );
}

#[test]
#[ignore = "functional: was a known defect: W3-7 Q2 (i), a type swap on a page that holds its main thread"]
fn browser_swaps_on_a_held_main_thread_in_edge() {
    let recorded = browser::held_swap_phases("browser-held-swap-edge", "edge");
    flipped!(
        "W3-7-Q2i",
        recorded == 0,
        format!("TF browser-held-swap-edge password_letters_recorded_while_the_page_was_held={recorded}")
    );
}

#[test]
#[ignore = "functional: was a known defect: W3-5 A2, a focus move that no latch sees"]
fn stall_no_focus_key_and_the_program_moves_the_focus_itself() {
    with_deadline("stall-self-focus", Duration::from_secs(330), || {
        let (recorded, late) = stall::no_focus_key_and_the_program_moves_the_focus_itself();
        assert_eq!(late, 0, "the source's late value moved");
        flipped!(
            "W3-5-A2",
            recorded == 0,
            format!(
                "TF stall-self-focus letters_recorded_that_landed_in_the_password_field={recorded}"
            )
        );
    });
}

mod known_defect {
    use super::*;

    #[test]
    #[ignore = "known defect: win-6, an earlier key's landing before a page's late move"]
    fn browser_an_earlier_keys_landing_before_the_pages_late_move_in_chrome() {
        let recorded = browser::earlier_key_phases("browser-take-one-chrome", "chrome", true);
        defect!(
            "W6-earlier-key",
            recorded == 0,
            format!("TF browser-take-one-chrome password_letters_recorded={recorded}")
        );
    }

    #[test]
    #[ignore = "known defect: win-6, an earlier key's landing before a page's late move"]
    fn browser_an_earlier_keys_landing_before_the_pages_late_move_in_edge() {
        let recorded = browser::earlier_key_phases("browser-take-one-edge", "edge", true);
        defect!(
            "W6-earlier-key",
            recorded == 0,
            format!("TF browser-take-one-edge password_letters_recorded={recorded}")
        );
    }

    #[test]
    #[ignore = "known defect: win-6, a change by the pointer behind its own button-down before a late move"]
    fn stall_a_change_by_the_pointer_behind_its_own_button_down_before_a_late_move() {
        let read = stall::change_by_the_pointer(true);
        assert!(
            read.late == 0,
            "the source's late value moved: {}",
            read.late
        );
        assert!(
            read.beyond == 0,
            "{} keystroke records held more than a time, an app and a class",
            read.beyond
        );
        defect!(
            "W6-pointer-change",
            read.recorded == 0,
            format!(
                "TF stall-pointer-change letters_recorded_that_landed_in_the_password_field={} \
                 keystroke_records={} records_beyond_time_app_and_class={}",
                read.recorded, read.records, read.beyond
            )
        );
    }

    #[test]
    #[ignore = "known defect: win-6, a program frozen longer than the bound while a person types ahead"]
    fn stall_a_program_frozen_longer_than_the_bound_while_a_person_types_ahead() {
        let read = stall::frozen_program(true);
        assert!(
            read.late == 0,
            "the source's late value moved: {}",
            read.late
        );
        assert!(
            read.beyond == 0,
            "{} keystroke records held more than a time, an app and a class",
            read.beyond
        );
        defect!(
            "W6-frozen",
            read.recorded == 0,
            format!(
                "TF stall-frozen letters_recorded_that_landed_in_the_password_field={} \
                 keystroke_records={} records_beyond_time_app_and_class={}",
                read.recorded, read.records, read.beyond
            )
        );
    }

    #[test]
    #[ignore = "known defect: win-6, a program frozen longer than the bound behind a Tab while a person types ahead"]
    fn stall_a_program_frozen_longer_than_the_bound_behind_a_tab_while_a_person_types_ahead() {
        let read = stall::frozen_program_behind_a_tab(true);
        assert!(
            read.late == 0,
            "the source's late value moved: {}",
            read.late
        );
        assert!(
            read.beyond == 0,
            "{} keystroke records held more than a time, an app and a class",
            read.beyond
        );
        defect!(
            "W6-frozen-tab",
            read.recorded == 0,
            format!(
                "TF stall-frozen-tab letters_recorded_that_landed_in_the_password_field={} \
                 keystroke_records={} records_beyond_time_app_and_class={}",
                read.recorded, read.records, read.beyond
            )
        );
    }

    #[test]
    #[ignore = "known defect: win-6, the program's own change behind a Tab before the Tab's late move"]
    fn stall_the_programs_own_change_behind_a_tab_before_the_tabs_late_move() {
        let read = stall::own_change_behind_a_tab(true);
        assert!(
            read.late == 0,
            "the source's late value moved: {}",
            read.late
        );
        assert!(
            read.beyond == 0,
            "{} keystroke records held more than a time, an app and a class",
            read.beyond
        );
        defect!(
            "W6-own-change-tab",
            read.recorded == 0,
            format!(
                "TF stall-own-change-tab letters_recorded_that_landed_in_the_password_field={} \
                 keystroke_records={} records_beyond_time_app_and_class={}",
                read.recorded, read.records, read.beyond
            )
        );
    }

    #[test]
    #[ignore = "known defect: win-6, the program's own change of the focused field before a late move"]
    fn stall_the_programs_own_change_of_the_focused_field_before_a_late_move() {
        with_deadline("stall-own-change", Duration::from_secs(300), || {
            let (recorded, late, records, beyond) =
                stall::the_programs_own_change_of_the_focused_field_before_a_late_move();
            assert_eq!(late, 0, "the source's late value moved");
            assert_eq!(
                beyond, 0,
                "a keystroke record held more than its time, its app and its class"
            );
            defect!(
                "W6-own-change",
                recorded == 0,
                format!(
                    "TF stall-own-change letters_recorded_that_landed_in_the_password_field={recorded} \
                     keystroke_records={records} records_beyond_time_app_and_class={beyond}"
                )
            );
        });
    }

    #[test]
    #[ignore = "known defect: W3-20, a text input that a page masks by style alone"]
    fn style_masked_input_in_chrome_and_edge() {
        let mut recorded = 0;
        let mut lines = Vec::new();
        for (test, browser) in [("style-mask-chrome", &CHROME), ("style-mask-edge", &EDGE)] {
            let (keys, uia) = style_mask_phase(test, browser);
            recorded += keys;
            lines.push(format!("{}:{uia}", browser.name));
        }
        for line in &lines {
            assert!(
                line.contains(":is_password=false ")
                    && line.contains(" protected=false ")
                    && line.contains(" control_type=50004 "),
                "the UI Automation line of the style-masked input differs, so the gate could read it \
                 and the case is not this one: {line}"
            );
        }
        defect!(
            "W3-20-style-mask",
            recorded == 0,
            format!("TF style-mask keystroke_events_in_chrome_and_edge={recorded}")
        );
    }
}

use halvo_capture::windows::apps::{InstalledSoftwareWatcher, WatchOutcome};

fn arm_watcher() -> mpsc::Receiver<Result<(), String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let woke = InstalledSoftwareWatcher::new()
            .map_err(|e| e.to_string())
            .and_then(
                |mut watcher| match watcher.wait(Some(Duration::from_secs(20))) {
                    Ok(WatchOutcome::Changed) => Ok(()),
                    Ok(other) => Err(format!("the watch answered {other:?}")),
                    Err(e) => Err(e.to_string()),
                },
            );
        let _ = tx.send(woke);
    });
    std::thread::sleep(Duration::from_millis(500));
    rx
}

fn answer<T>(rx: &mpsc::Receiver<T>, limit: Duration, what: &str) -> Option<T> {
    match rx.recv_timeout(limit) {
        Ok(value) => Some(value),
        Err(RecvTimeoutError::Timeout) => None,
        Err(RecvTimeoutError::Disconnected) => {
            panic!("INCONCLUSIVE: {what} ended without an answer")
        }
    }
}

fn watch_woke(rx: &mpsc::Receiver<Result<(), String>>, limit: Duration, what: &str) -> bool {
    match answer(rx, limit, what) {
        Some(Ok(())) => true,
        Some(Err(e)) => panic!("INCONCLUSIVE: {what} returned an error: {e}"),
        None => false,
    }
}

fn powershell_line(script: &str) -> String {
    Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_default()
}

#[test]
#[ignore = "functional: reads and writes the Windows registry"]
fn c5_watch_armed_while_the_per_user_key_is_absent() {
    with_deadline("c5-watch", Duration::from_secs(90), || {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key_absent = per_user_key_absent_or_removed("c5-watch", &hkcu);
        assert!(
            key_absent,
            "INCONCLUSIVE: the per-user Uninstall key exists, so no watch can be armed while it is absent"
        );

        let first = arm_watcher();
        let spurious = watch_woke(&first, Duration::from_secs(1), "the first watch");
        let probe_path = format!(r"{UNINSTALL}\{PROBE_KEY}");
        let (probe, _) = hkcu
            .create_subkey(&probe_path)
            .expect("create the probe key");
        probe
            .set_value(
                "DisplayName",
                &format!("Halvo Verify Probe {}", std::process::id()),
            )
            .expect("probe name");
        let first_woke = watch_woke(&first, Duration::from_secs(5), "the first watch");

        let second = arm_watcher();
        probe
            .set_value("DisplayVersion", &"9.9.10")
            .expect("probe version change");
        let second_woke = watch_woke(&second, Duration::from_secs(5), "the second watch");

        drop(probe);
        hkcu.delete_subkey_all(&probe_path)
            .expect("delete the probe key");
        let _ = hkcu.delete_subkey(UNINSTALL);
        say(
            "c5-watch",
            format!(
                "woke_before_any_change={spurious} \
                 watch_armed_with_the_key_present_woke_within_5s_of_change={second_woke}"
            ),
        );
        assert!(!spurious, "the watch woke before any change");
        assert!(
            second_woke,
            "INCONCLUSIVE: a watch armed with the key present did not wake on a change"
        );
        flipped!(
            "RR-windows-12",
            first_woke,
            format!("TF c5-watch woke_within_5s_of_hkcu_install={first_woke}")
        );
    });
}

#[test]
#[ignore = "functional: reads the Windows registry"]
fn x16_packaged_apps_and_the_inventory() {
    with_deadline("x16", Duration::from_secs(120), || {
        let scan = scan_installed().expect("scan the registry");
        let packages: u32 = powershell_line("(Get-AppxPackage | Measure-Object).Count")
            .parse()
            .unwrap_or(0);
        let terminal_packages: u32 = powershell_line(
            "(Get-AppxPackage -Name Microsoft.WindowsTerminal | Measure-Object).Count",
        )
        .parse()
        .unwrap_or(0);
        let terminal_in_inventory = scan
            .apps()
            .iter()
            .any(|app| app.name.to_ascii_lowercase().contains("terminal"));
        say(
            "x16",
            format!(
                "inventory_entries={} packaged_apps_registered_for_this_user={packages} \
                 windows_terminal_packages={terminal_packages}",
                scan.len(),
            ),
        );
        assert!(
            !scan.is_empty(),
            "INCONCLUSIVE: the registry scan returned no entries"
        );
        assert!(
            packages >= 1 && terminal_packages >= 1,
            "INCONCLUSIVE: no packaged Windows Terminal is registered for this user"
        );
        flipped!(
            "RR-windows-12",
            terminal_in_inventory,
            format!("TF x16 inventory_has_an_entry_naming_terminal={terminal_in_inventory}")
        );
    });
}

#[test]
#[ignore = "functional: reads and writes the Windows registry"]
fn c5_three_probe_entries_against_what_windows_lists() {
    with_deadline("c5-three", Duration::from_secs(90), || {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key_existed = hkcu.open_subkey_with_flags(UNINSTALL, KEY_READ).is_ok();
        let id = std::process::id();
        let probes = [
            (
                "HalvoVerifyProbeListed",
                format!("Halvo Verify Listed {id}"),
                None,
                false,
            ),
            (
                "HalvoVerifyProbeSystem",
                format!("Halvo Verify System {id}"),
                None,
                true,
            ),
            (
                "HalvoVerifyProbeChild",
                format!("Halvo Verify Child {id}"),
                Some("HalvoVerifyProbeListed"),
                false,
            ),
        ];
        for (key, name, parent, system) in &probes {
            let (probe, _) = hkcu
                .create_subkey(format!(r"{UNINSTALL}\{key}"))
                .expect("create a probe key");
            probe.set_value("DisplayName", name).expect("probe name");
            if let Some(parent) = parent {
                probe
                    .set_value("ParentKeyName", &parent.to_string())
                    .expect("probe parent");
            }
            if *system {
                probe
                    .set_value("SystemComponent", &1u32)
                    .expect("probe system component");
            }
        }
        let scan = scan_installed();
        for (key, ..) in &probes {
            let _ = hkcu.delete_subkey_all(format!(r"{UNINSTALL}\{key}"));
        }
        if !key_existed {
            let _ = hkcu.delete_subkey(UNINSTALL);
        }
        let scan = scan.expect("scan with the three probes");
        let listed = probes
            .each_ref()
            .map(|(_, name, ..)| scan.apps().iter().any(|app| &app.name == name));
        let windows_lists = [true, false, false];
        say(
            "c5-three",
            format!(
                "probe_listed={} probe_system_component_listed={} probe_with_parent_listed={}",
                listed[0], listed[1], listed[2]
            ),
        );
        assert!(
            listed[0],
            "INCONCLUSIVE: the plain probe entry is not in the scan"
        );
        flipped!(
            "RR-windows-25",
            listed == windows_lists,
            format!(
                "TF c5-three scan_lists_what_windows_lists={}",
                listed == windows_lists
            )
        );
    });
}

#[test]
#[ignore = "functional: was a known defect: E4, a keystroke record holds no class"]
fn one_key_of_each_class_gives_a_record_only_for_the_keys_that_change_text() {
    with_deadline("classes", Duration::from_secs(90), || {
        env_report("classes");
        source_built_by_test("classes", "keyboard");
        let (sink, events) = capture(4096);
        let mut child = spawn_child("hold", 14);
        let began = Instant::now();
        let mut held_in_front = false;
        while !held_in_front && began.elapsed() < Duration::from_secs(15) {
            pump_thread(Duration::from_millis(100));
            held_in_front = class_of_window(foreground_window()) == "HalvoVerifyHoldFirst";
        }
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        let _ = events.drain_for(Duration::from_secs(1));
        let presses: [&[VIRTUAL_KEY]; 6] = [
            &[VIRTUAL_KEY(0x41)],
            &[VIRTUAL_KEY(0x08)],
            &[VIRTUAL_KEY(0x25)],
            &[VK_F15],
            &[VIRTUAL_KEY(0x11), VIRTUAL_KEY(0x41)],
            &[VIRTUAL_KEY(0x0D)],
        ];
        let mut accepted = 0;
        for keys in presses {
            accepted += key_press(keys);
        }
        let mut seen = events.drain_for(Duration::from_secs(2));
        let _ = stop_counted(&mut keyboard, "keyboard");
        let mut second = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let second_start = second.begin(SourceContext::new(sink.clone()));
        let _ = events.drain_for(Duration::from_millis(500));
        accepted += key_press(&[VIRTUAL_KEY(0x09)]);
        seen.extend(events.drain_for(Duration::from_secs(2)));
        let still_in_front = class_of_window(foreground_window()) == "HalvoVerifyHoldFirst";
        let _ = stop_counted(&mut second, "keyboard");
        let _ = child.wait();
        child.print_output();
        let classes: Vec<KeyClass> = seen
            .iter()
            .filter_map(|e| e.view.and_then(|view| view.key_class))
            .collect();
        let each: Vec<String> = KeyClass::ALL
            .iter()
            .map(|class| {
                format!(
                    "{}={}",
                    class.as_str(),
                    classes.iter().filter(|seen| *seen == class).count()
                )
            })
            .collect();
        say(
            "classes",
            format!(
                "keyboard_start={} held_in_front={held_in_front} still_in_front={still_in_front} \
                 sendinput_key_events_accepted={accepted} keystroke_events={} {}",
                show(&start),
                count(&seen, Modality::Keystroke),
                each.join(" ")
            ),
        );
        assert!(
            start.is_ok() && second_start.is_ok(),
            "the keyboard sources must start"
        );
        assert!(
            held_in_front && still_in_front,
            "INCONCLUSIVE: the child's window with its plain edit was not in front"
        );
        assert_eq!(
            accepted, 16,
            "INCONCLUSIVE: SendInput did not insert every key"
        );
        let changed_text = [KeyClass::Printable, KeyClass::Editing];
        flipped!(
            "E4",
            KeyClass::ALL.iter().all(|class| {
                classes.iter().filter(|seen| *seen == class).count()
                    == usize::from(changed_text.contains(class))
            }) && classes.len() == changed_text.len(),
            format!(
                "TF classes keystroke_records_with_a_class={}",
                classes.len()
            )
        );
    });
}

#[test]
#[ignore = "functional: was a known defect: E4 and B14-20, pointer events are not folded by class and interval"]
fn pointer_events_fold_into_one_record_for_each_class_and_interval() {
    with_deadline("folds", Duration::from_secs(90), || {
        const MOVES: usize = 100;
        env_report("folds");
        source_built_by_test("folds", "pointer");
        let (sink, events) = capture(8192);
        let cover = TestWindow::create(&magenta_popup("HalvoVerifyFolds", "halvo-verify-folds"))
            .expect("covering window");
        cover.cover_screen();
        pump_thread(Duration::from_millis(500));
        let mut pointer = RawInputSource::pointer(Arc::new(Mutex::new(None)));
        let start = pointer.begin(SourceContext::new(sink.clone()));
        let _ = events.drain_for(Duration::from_secs(1));
        let handled_before_the_first = pointer_samples_handled();
        let moves: Vec<INPUT> = (0..MOVES)
            .map(|i| mouse(MOUSEEVENTF_MOVE, if i % 2 == 0 { 3 } else { -3 }, 0))
            .collect();
        let moved = send(&moves);
        let first = events.drain_for(Duration::from_millis(2_500));
        let handled_before_the_second = pointer_samples_handled();
        let mut second_sent = send(&[
            mouse(MOUSEEVENTF_LEFTDOWN, 0, 0),
            mouse(MOUSEEVENTF_LEFTUP, 0, 0),
        ]);
        second_sent += send(&[mouse(MOUSEEVENTF_WHEEL, 0, 120)]);
        // The press of the drag goes alone, and its moves go only once the
        // source has taken it: a move in the press's own sample is a plain
        // move by the source's rule, which the third phase wants.
        second_sent += send(&[mouse(MOUSEEVENTF_LEFTDOWN, 0, 0)]);
        let mut second = Vec::new();
        let (press_taken, press_waited) =
            wait_for_clicks(&events, &mut second, 2, PRESS_TAKEN_WAIT);
        second_sent += if press_taken {
            send(&[
                mouse(MOUSEEVENTF_MOVE, 3, 0),
                mouse(MOUSEEVENTF_MOVE, -3, 0),
                mouse(MOUSEEVENTF_MOVE, 3, 0),
                mouse(MOUSEEVENTF_LEFTUP, 0, 0),
            ])
        } else {
            send(&[mouse(MOUSEEVENTF_LEFTUP, 0, 0)])
        };
        second.extend(events.drain_for(Duration::from_millis(2_500)));
        let handled_before_the_third = pointer_samples_handled();
        let third_sent = send(&[
            mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTDOWN, -3, 0),
            mouse(MOUSEEVENTF_LEFTUP, 0, 0),
        ]);
        let third = events.drain_for(Duration::from_millis(2_500));
        let handled_after_the_third = pointer_samples_handled();
        let samples_handled_of_the_first = handled_before_the_second - handled_before_the_first;
        let samples_handled_of_the_second = handled_before_the_third - handled_before_the_second;
        let samples_handled_of_the_third = handled_after_the_third - handled_before_the_third;
        let press_waited_ms = press_waited.as_millis();
        let covered = cover.is_foreground();
        pump_thread(Duration::from_millis(100));
        drop(cover);
        let _ = stop_counted(&mut pointer, "pointer");
        let moves_first = folds(&first, FoldClass::Move);
        let drags = folds(&second, FoldClass::Drag);
        let scrolls = folds(&second, FoldClass::Scroll);
        let moves_second = folds(&second, FoldClass::Move);
        let clicks_second = clicks(&second);
        let moves_third = folds(&third, FoldClass::Move);
        let clicks_third = clicks(&third);
        say(
            "folds",
            format!(
                "pointer_start={} cover_window_was_foreground={covered} sendinput_move_events_accepted={moved} \
                 move_records={moves_first:?} pointer_events={} \
                 samples_handled_of_the_first={samples_handled_of_the_first} \
                 second_accepted={second_sent} press_taken={press_taken} press_waited={press_waited_ms}ms \
                 clicks={} drag_records={drags:?} scroll_records={scrolls:?} \
                 move_records_of_the_second={moves_second:?} \
                 samples_handled_of_the_second={samples_handled_of_the_second} \
                 third_accepted={third_sent} \
                 clicks_of_the_third={} move_records_of_the_third={moves_third:?} \
                 samples_handled_of_the_third={samples_handled_of_the_third}",
                show(&start),
                count(&first, Modality::Pointer),
                clicks_second.len(),
                clicks_third.len(),
            ),
        );
        assert!(start.is_ok(), "the pointer source must start");
        assert!(
            press_taken,
            "the pointer source gave no click record of the press of the second phase within {}s, \
             so its moves were not sent",
            PRESS_TAKEN_WAIT.as_secs()
        );
        assert!(
            moved as usize == MOVES && second_sent == 8 && third_sent == 2,
            "INCONCLUSIVE: SendInput did not insert every pointer event"
        );
        flipped!(
            "E4",
            moves_first == [MOVES as u64]
                && clicks_second == [Button::Primary, Button::Primary]
                && drags == [3]
                && scrolls == [1]
                && moves_second.is_empty()
                && clicks_third == [Button::Primary]
                && moves_third == [1],
            format!("TF folds move_records={moves_first:?}")
        );
    });
}

#[test]
#[ignore = "functional: was a known defect: P8, an event of a process under the exclusion root leaves the sources"]
fn a_window_under_the_exclusion_root_gives_no_event_and_another_window_does() {
    with_deadline("tree", Duration::from_secs(120), || {
        env_report("tree");
        for source in ["foreground", "window_title", "keyboard", "pointer"] {
            source_built_by_test("tree", source);
        }
        let (sink, events) = capture(8192);
        let mut root = spawn_child("root", 14);
        let floor = FloorHandle::new(
            Some(ProcessId::new(root.id())),
            Arc::new(ProcessLineage::new()),
            Switches::default(),
        );
        let ctx = SourceContext::with_floor(sink.clone(), floor);
        let mut foreground = ForegroundAppSource::new();
        let mut title = WindowTitleSource::new();
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let mut pointer = RawInputSource::pointer(Arc::new(Mutex::new(None)));
        let starts = [
            foreground.begin(ctx.clone()),
            title.begin(ctx.clone()),
            keyboard.begin(ctx.clone()),
            pointer.begin(ctx.clone()),
        ];
        let began = Instant::now();
        let mut held_in_front = false;
        while !held_in_front && began.elapsed() < Duration::from_secs(15) {
            pump_thread(Duration::from_millis(100));
            held_in_front = class_of_window(foreground_window()) == "HalvoVerifyHoldFirst";
        }
        let _ = events.drain_for(Duration::from_millis(500));
        let (gate_read_clear_in_the_tree, gate_waited_in_the_tree) =
            gate_reads_clear_in_front_of(&keyboard, foreground_pid());
        let keyboard_tree_drops_before_the_letter = keyboard.drops().exclusion_tree;
        let in_tree_keys = type_letters(1);
        let (keyboard_tree_drops, keyboard_drop_waited) =
            keyboard_tree_drops_above(&keyboard, keyboard_tree_drops_before_the_letter);
        let _ = point_at_front();
        let in_tree_moves = move_pointer(10);
        let in_tree = events.drain_for(Duration::from_millis(1_500));
        let still_in_front = class_of_window(foreground_window()) == "HalvoVerifyHoldFirst";
        let fields = FieldWindow::open(
            "a_window_under_the_exclusion_root_gives_no_event_and_another_window_does",
            Field::Plain,
        );
        let _ = events.drain_for(Duration::from_millis(500));
        let outside_pid = foreground_pid();
        let (gate_read_clear_outside, gate_waited_outside) =
            gate_reads_clear_in_front_of(&keyboard, outside_pid);
        let outside_keys = type_letters(1);
        let (mut outside, outside_record_waited) = events_until_a_keystroke(&events);
        let _ = point_at_front();
        let outside_moves = move_pointer(10);
        outside.extend(events.drain_for(Duration::from_millis(2_000)));
        let lineage = ProcessLineage::new();
        let mut walked = ProcessId::new(outside_pid);
        let mut walk_steps = 0;
        let mut walk_end = "steps";
        let mut last_opened = false;
        for _ in 0..64 {
            if walked.get() == root.id() {
                walk_end = "root";
                break;
            }
            match lineage.parent_of(walked) {
                Parent::Of(parent) => {
                    walked = parent;
                    walk_steps += 1;
                }
                Parent::None => {
                    walk_end = "none";
                    break;
                }
                Parent::Unknown => {
                    walk_end = "unknown";
                    break;
                }
            }
        }
        // SAFETY: OpenProcess takes plain values; a handle that it gives is
        // closed at once.
        unsafe {
            use windows::Win32::System::Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            };
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, walked.get())
            {
                last_opened = true;
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
        }
        drop(fields);
        for (source, name) in [
            (&mut foreground as &mut dyn CaptureSource, "foreground"),
            (&mut title, "window_title"),
            (&mut keyboard, "keyboard"),
            (&mut pointer, "pointer"),
        ] {
            let _ = stop_counted(source, name);
        }
        let _ = root.wait();
        root.print_output();
        let drops = [
            foreground.drops(),
            title.drops(),
            keyboard.drops(),
            pointer.drops(),
        ];
        let tree_drops: u64 = drops.iter().map(|drops| drops.exclusion_tree).sum();
        let unknown_lineage_drops: u64 = drops.iter().map(|drops| drops.unknown_lineage).sum();
        let other_drops: u64 = drops
            .iter()
            .map(|drops| {
                drops.switch_off + drops.listed_app + drops.private_window + drops.secure_input
            })
            .sum();
        let outside_keystroke_classes: Vec<Option<KeyClass>> = outside
            .iter()
            .filter(|event| event.modality == Modality::Keystroke)
            .map(|event| event.view.and_then(|view| view.key_class))
            .collect();
        say(
            "tree",
            format!(
                "starts_ok={} grandchild_window_in_front={held_in_front} still_in_front={still_in_front} \
                 gate_read_clear_in_the_tree={gate_read_clear_in_the_tree} gate_waited_in_the_tree_ms={} \
                 sendinput_key_events_accepted={in_tree_keys} \
                 keyboard_tree_drops_before_the_letter={keyboard_tree_drops_before_the_letter} \
                 keyboard_tree_drops={keyboard_tree_drops} keyboard_drop_waited_ms={} \
                 sendinput_move_events_accepted={in_tree_moves} \
                 events_in_the_tree={} exclusion_tree_drops={tree_drops} \
                 unknown_lineage_drops={unknown_lineage_drops} other_drops={other_drops} \
                 outside_walk_steps={walk_steps} outside_walk_end={walk_end} \
                 outside_walk_last_process_opened={last_opened} \
                 gate_read_clear_outside={gate_read_clear_outside} gate_waited_outside_ms={} \
                 outside_keys={outside_keys} outside_record_waited_ms={} \
                 outside_moves={outside_moves} events_outside={} keystroke_events_outside={} \
                 outside_keystroke_classes={outside_keystroke_classes:?} pointer_events_outside={}",
                starts.iter().all(Result::is_ok),
                gate_waited_in_the_tree.as_millis(),
                keyboard_drop_waited.as_millis(),
                in_tree.len(),
                gate_waited_outside.as_millis(),
                outside_record_waited.as_millis(),
                outside.len(),
                count(&outside, Modality::Keystroke),
                count(&outside, Modality::Pointer),
            ),
        );
        assert!(starts.iter().all(Result::is_ok), "the sources must start");
        assert!(
            held_in_front && still_in_front,
            "INCONCLUSIVE: the window of the grandchild was not in front"
        );
        assert!(
            gate_read_clear_in_the_tree && gate_read_clear_outside,
            "INCONCLUSIVE: the gate did not read the plain edit in front as Clear before a letter"
        );
        assert!(
            in_tree_keys == 2 && outside_keys == 2,
            "INCONCLUSIVE: SendInput did not insert every key"
        );
        assert!(
            count(&outside, Modality::Pointer) >= 1,
            "no pointer event for a window outside the tree"
        );
        assert!(
            outside_keystroke_classes == [Some(KeyClass::Printable)],
            "the letter in the plain edit of a window outside the tree gave not one keystroke record"
        );
        flipped!(
            "P8",
            in_tree.is_empty() && keyboard_tree_drops > keyboard_tree_drops_before_the_letter,
            format!(
                "TF tree events_in_the_tree={} keyboard_tree_drops={keyboard_tree_drops}",
                in_tree.len()
            )
        );
    });
}

/// How long the tree test waits for each state around its letters: the
/// gate's Clear of the plain edit in front, the keyboard's drop by the tree,
/// and the one keystroke record of the window outside the tree.
const TREE_STATE_WAIT: Duration = Duration::from_secs(3);

/// Waits until the keyboard's gate reads Clear and the seam holds a Clear
/// mark noted while a window of `front_pid` was in front, so that a Clear of
/// the window before it does not pass: whether both held, and the wait.
fn gate_reads_clear_in_front_of(keyboard: &RawInputSource, front_pid: u32) -> (bool, Duration) {
    use halvo_capture::secure_input::windows::FieldState;
    let began = Instant::now();
    let mut clear_mark_in_front = false;
    loop {
        clear_mark_in_front |= seam::take_marks()
            .iter()
            .any(|mark| mark.front_pid == front_pid && mark.state == Some(FieldState::Clear));
        let clear = clear_mark_in_front && keyboard.field_state() == Some(FieldState::Clear);
        if clear || began.elapsed() >= TREE_STATE_WAIT {
            return (clear, began.elapsed());
        }
        pump_thread(Duration::from_millis(20));
    }
}

/// Waits until the keyboard's own count of drops by the tree is above
/// `before`. The count rises only at a key that the gate cleared and the
/// floor's tree then left out, so it cannot rise before such a key was
/// decided. Answers the count and the wait.
fn keyboard_tree_drops_above(keyboard: &RawInputSource, before: u64) -> (u64, Duration) {
    let began = Instant::now();
    loop {
        let keyboard_tree_drops = keyboard.drops().exclusion_tree;
        if keyboard_tree_drops > before || began.elapsed() >= TREE_STATE_WAIT {
            return (keyboard_tree_drops, began.elapsed());
        }
        pump_thread(Duration::from_millis(20));
    }
}

/// Takes events until the first keystroke record, at most for the tree
/// test's wait: the events taken, that record the last of them, and the wait.
fn events_until_a_keystroke(events: &Events) -> (Vec<Seen>, Duration) {
    let began = Instant::now();
    let mut taken = Vec::new();
    while let Some(left) = TREE_STATE_WAIT.checked_sub(began.elapsed()) {
        let Some(event) = events.first_within(left) else {
            break;
        };
        let keystroke = event.modality == Modality::Keystroke;
        taken.push(event);
        if keystroke {
            break;
        }
    }
    (taken, began.elapsed())
}

fn key_press(keys: &[VIRTUAL_KEY]) -> u32 {
    let mut accepted = 0;
    for key in keys {
        accepted += send(&[key_input(*key, false)]);
        std::thread::sleep(Duration::from_millis(30));
    }
    for key in keys.iter().rev() {
        accepted += send(&[key_input(*key, true)]);
        std::thread::sleep(Duration::from_millis(30));
    }
    std::thread::sleep(Duration::from_millis(300));
    accepted
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy: 0,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn folds(events: &[Seen], wanted: FoldClass) -> Vec<u64> {
    events
        .iter()
        .filter_map(|e| match e.view.and_then(|view| view.fold) {
            Some((class, count)) if class == wanted => Some(count),
            _ => None,
        })
        .collect()
}

fn clicks(events: &[Seen]) -> Vec<Button> {
    events
        .iter()
        .filter_map(|e| e.view.and_then(|view| view.click))
        .collect()
}

/// How long the pointer test waits for the click record of the press of its
/// drag.
const PRESS_TAKEN_WAIT: Duration = Duration::from_secs(10);

/// Reads records into `phase` until it holds `wanted` click records, up to
/// `limit`: whether it holds them, and the wait. The source builds a press's
/// click record after it has noted the button as held.
fn wait_for_clicks(
    events: &Events,
    phase: &mut Vec<Seen>,
    wanted: usize,
    limit: Duration,
) -> (bool, Duration) {
    let began = Instant::now();
    while clicks(phase).len() < wanted {
        let left = limit.saturating_sub(began.elapsed());
        if left.is_zero() {
            break;
        }
        match events.first_within(left) {
            Some(event) => phase.push(event),
            None => break,
        }
    }
    (clicks(phase).len() >= wanted, began.elapsed())
}

/// The count of raw pointer samples that the pointer sources of this process
/// have handled.
fn pointer_samples_handled() -> u64 {
    seam::pointer_queue_largest().1
}

fn titles_with(events: &[Seen], prefix: &str) -> usize {
    let titles = known_titles();
    events
        .iter()
        .filter(|e| {
            e.view
                .and_then(|view| view.title)
                .and_then(|title| title.among)
                .is_some_and(|index| titles[index].starts_with(prefix))
        })
        .count()
}

#[test]
#[ignore = "functional: a wheel turn over a background window of a listed app is decided on that app"]
fn a_wheel_turn_over_a_background_window_of_a_listed_app_gives_no_scroll_record() {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetWindowRect, SetCursorPos, SetWindowPos, WindowFromPoint, GA_ROOT,
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    with_deadline("wheel-under", Duration::from_secs(90), || {
        env_report("wheel-under");
        source_built_by_test("wheel-under", "pointer");
        let (sink, events) = capture(4096);
        let front = TestWindow::create(&normal_window(
            "HalvoVerifyWheelFront",
            "halvo-verify-wheel-front",
            20,
            20,
        ))
        .expect("the window in front");
        let mut child = spawn_child_as("background", 12, LISTED_CHILD_EXE);
        let class: Vec<u16> = BACKGROUND_CLASS.encode_utf16().chain([0]).collect();
        let mut background = HWND::default();
        let began = Instant::now();
        while background.0.is_null() && began.elapsed() < Duration::from_secs(10) {
            pump_thread(Duration::from_millis(100));
            // SAFETY: `class` is a NUL-terminated wide string that outlives the call.
            background =
                unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.unwrap_or_default();
        }
        // SAFETY: SetWindowPos takes the child's window handle by value and
        // plain flags; it moves the window above the others without
        // activating it.
        let _ = unsafe {
            SetWindowPos(
                background,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
        };
        pump_thread(Duration::from_millis(500));
        let (front_activated, method) = front.activate();
        let mut rect = RECT::default();
        // SAFETY: `rect` is a live RECT that the call writes into.
        let _ = unsafe { GetWindowRect(background, &mut rect) };
        let middle = POINT {
            x: (rect.left + rect.right) / 2,
            y: (rect.top + rect.bottom) / 2,
        };
        // SAFETY: SetCursorPos, WindowFromPoint and GetAncestor take plain values.
        let pointer_over_background = unsafe {
            let _ = SetCursorPos(middle.x, middle.y);
            !background.0.is_null() && GetAncestor(WindowFromPoint(middle), GA_ROOT) == background
        };
        let mut pointer = RawInputSource::pointer(Arc::new(Mutex::new(None)));
        let start = pointer.begin(SourceContext::new(sink.clone()));
        let _ = events.drain_for(Duration::from_secs(1));
        let sent = send(&[mouse(MOUSEEVENTF_WHEEL, 0, 120)]);
        let seen = events.drain_for(Duration::from_millis(2_500));
        let front_still_in_front = front.is_foreground();
        let _ = stop_counted(&mut pointer, "pointer");
        while child.try_wait().is_none() {
            pump_thread(Duration::from_millis(200));
        }
        child.print_output();
        let scroll_records = folds(&seen, FoldClass::Scroll).len();
        let scroll_record_app_is_front = seen.iter().any(|e| {
            e.view.is_some_and(|view| {
                matches!(view.fold, Some((FoldClass::Scroll, _)))
                    && view.app == seam::AppSeen::Named(1)
            })
        });
        let listed_app_drops = pointer.drops().listed_app;
        say(
            "wheel-under",
            format!(
                "pointer_start={} front_activated={front_activated} method={method} \
                 pointer_over_background={pointer_over_background} \
                 front_still_in_front={front_still_in_front} wheel_accepted={sent} \
                 scroll_records={scroll_records} scroll_record_app_is_front={scroll_record_app_is_front} \
                 listed_app_drops={listed_app_drops}",
                show(&start)
            ),
        );
        assert!(start.is_ok(), "the pointer source must start");
        assert!(
            sent == 1 && pointer_over_background && front_still_in_front,
            "INCONCLUSIVE: the wheel turn was not sent over a background window"
        );
        assert!(
            scroll_records == 0 && listed_app_drops >= 1,
            "a wheel turn over a listed app's background window was recorded"
        );
    });
}

#[test]
#[ignore = "functional: the ghost of a hung window of a listed app must not carry its caption out"]
fn the_ghost_of_a_hung_listed_window_gives_no_title() {
    use windows::Win32::UI::WindowsAndMessaging::IsHungAppWindow;
    with_deadline("ghost", Duration::from_secs(90), || {
        env_report("ghost");
        source_built_by_test("ghost", "foreground");
        source_built_by_test("ghost", "window_title");
        let (sink, events) = capture(4096);
        let mut foreground = ForegroundAppSource::new();
        let mut title = WindowTitleSource::new();
        let foreground_start = foreground.begin(SourceContext::new(sink.clone()));
        let title_start = title.begin(SourceContext::new(sink.clone()));
        let mut child = spawn_child_as("hang", 0, LISTED_CHILD_EXE);
        let class: Vec<u16> = GHOST_CLASS.encode_utf16().chain([0]).collect();
        let mut hung = HWND::default();
        let mut seen = Vec::new();
        let began = Instant::now();
        while hung.0.is_null() && began.elapsed() < Duration::from_secs(10) {
            seen.extend(events.drain_for(Duration::from_millis(100)));
            // SAFETY: `class` is a NUL-terminated wide string that outlives the call.
            hung =
                unsafe { FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) }.unwrap_or_default();
        }
        let mut hung_read = false;
        while !hung.0.is_null() && !hung_read && began.elapsed() < Duration::from_secs(20) {
            seen.extend(events.drain_for(Duration::from_millis(200)));
            // SAFETY: IsHungAppWindow takes a window handle by value.
            hung_read = unsafe { IsHungAppWindow(hung) }.as_bool();
        }
        if hung_read {
            send(&[key_input(VK_MENU, false)]);
            set_foreground(hung);
            send(&[key_input(VK_MENU, true)]);
        }
        seen.extend(events.drain_for(Duration::from_millis(1_000)));
        let front = foreground_window();
        let mut front_pid = 0u32;
        // SAFETY: `front_pid` is a live u32 that the call writes into.
        unsafe { GetWindowThreadProcessId(front, Some(&mut front_pid)) };
        let ghost_seen = !front.0.is_null() && front != hung && class_of_window(front) == "Ghost";
        let front_file = image_name(front_pid);
        let foreground_process_is_dwm = front_file.eq_ignore_ascii_case("dwm.exe");
        let foreground_process_is_the_child = front_file.eq_ignore_ascii_case(LISTED_CHILD_EXE);
        while child.try_wait().is_none() {
            seen.extend(events.drain_for(Duration::from_millis(200)));
        }
        seen.extend(events.drain_for(Duration::from_millis(800)));
        let _ = stop_counted(&mut title, "window_title");
        let _ = stop_counted(&mut foreground, "foreground");
        child.print_output();
        let title_records_of_ghost = titles_with(&seen, GHOST_CAPTION);
        let listed_app_drops = title.drops().listed_app;
        say(
            "ghost",
            format!(
                "foreground_start={} title_start={} window_found={} hung_read={hung_read} \
                 ghost_seen={ghost_seen} foreground_process_is_dwm={foreground_process_is_dwm} \
                 foreground_process_is_the_child={foreground_process_is_the_child} \
                 title_records_of_ghost={title_records_of_ghost} listed_app_drops={listed_app_drops}",
                show(&foreground_start),
                show(&title_start),
                !hung.0.is_null()
            ),
        );
        assert!(
            foreground_start.is_ok() && title_start.is_ok(),
            "the sources must start"
        );
        assert_eq!(
            title_records_of_ghost, 0,
            "a title that names the hung window's caption left the source"
        );
    });
}

#[test]
#[ignore = "functional: was a known defect: E4, a title leaves while the keyboard source's gate reads Password"]
fn a_title_change_while_a_password_edit_has_the_focus_gives_no_title_event() {
    with_deadline("password-title", Duration::from_secs(90), || {
        env_report("password-title");
        source_built_by_test("password-title", "keyboard");
        source_built_by_test("password-title", "window_title");
        let (sink, events) = capture(4096);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let gate = keyboard
            .password_gate()
            .expect("the keyboard source's gate");
        let mut title = WindowTitleSource::new().with_password_gate(gate);
        let keyboard_start = keyboard.begin(SourceContext::new(sink.clone()));
        let title_start = title.begin(SourceContext::new(sink.clone()));
        let mut child = spawn_child("password-title", 0);
        let mut seen = Vec::new();
        while child.try_wait().is_none() {
            seen.extend(events.drain_for(Duration::from_millis(200)));
        }
        seen.extend(events.drain_for(Duration::from_millis(800)));
        let _ = stop_counted(&mut title, "window_title");
        let _ = stop_counted(&mut keyboard, "keyboard");
        child.print_output();
        let in_password_edit = titles_with(&seen, PASSWORD_EDIT_TITLE);
        let plain = titles_with(&seen, PLAIN_TITLE);
        let drops = title.drops();
        say(
            "password-title",
            format!(
                "keyboard_start={} title_start={} title_events_with_the_password_edit_focused={in_password_edit} \
                 title_events_with_the_plain_edit_focused={plain} secure_input_drops={}",
                show(&keyboard_start),
                show(&title_start),
                drops.secure_input
            ),
        );
        assert!(
            keyboard_start.is_ok() && title_start.is_ok(),
            "the sources must start"
        );
        flipped!(
            "E4",
            in_password_edit == 0 && plain >= 1,
            format!("TF password-title in_password_edit={in_password_edit} plain={plain}")
        );
    });
}

/// Waits until the gate's content reading is `wanted`, while the child runs,
/// up to `limit`: whether it read so.
fn wait_for_reading(
    gate: &PasswordGate,
    wanted: ContentReading,
    child: &mut ChildProcess,
    limit: Duration,
) -> bool {
    let began = Instant::now();
    while gate.content_reading() != wanted && child.try_wait().is_none() && began.elapsed() < limit
    {
        pump_thread(Duration::from_millis(20));
    }
    gate.content_reading() == wanted
}

/// The window title and screen sources of one service set share its gate.
/// Every step waits for a state, never for a time: the screen source begins
/// once the gate reads the child's box with no answer, and the child's first
/// title waits for that read; each title waits until the test has seen the one
/// before it kept, or, at the protected box, dropped; the protected box's
/// titles wait for the gate's read of Password; the screen source stops before
/// the child's focus leaves the two boxes. So every frame that the screen source
/// is offered comes while one of the two boxes has the focus, and every title is
/// decided at the reading of its own box.
#[test]
#[ignore = "functional: a title is kept and a frame dropped while an element with no answer has the focus"]
fn a_title_change_while_an_element_with_no_answer_has_the_focus_gives_a_title_event_and_no_frame() {
    with_deadline("no-answer-title", Duration::from_secs(90), || {
        env_report("no-answer-title");
        source_built_by_test("no-answer-title", "window_title");
        source_built_by_test("no-answer-title", "screen");
        let (sink, events) = capture(4096);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut title = set.gated_title(WindowTitleSource::new());
        let mut screen = set.gated_screen(ScreenSource::new(Arc::new(Mutex::new(None))));
        let title_start = title.begin(SourceContext::new(sink.clone()));
        let mut child = spawn_child("no-answer-title", 0);
        let no_answer_read = wait_for_reading(
            &gate,
            ContentReading::NoAnswer,
            &mut child,
            NO_ANSWER_STATE_WAIT,
        );
        let screen_start = screen.begin(screen_on(sink.clone()));
        let mut seen = Vec::new();
        child.go("no-answer");
        for step in 0..3 {
            let began = Instant::now();
            while titles_with(&seen, NO_ANSWER_TITLE) <= step
                && child.try_wait().is_none()
                && began.elapsed() < NO_ANSWER_STATE_WAIT
            {
                seen.extend(events.drain_for(Duration::from_millis(50)));
            }
            child.go(&format!("no-answer-{step}"));
        }
        let protected_read = wait_for_reading(
            &gate,
            ContentReading::Password,
            &mut child,
            NO_ANSWER_STATE_WAIT,
        );
        let drops_before = title.source().drops().secure_input;
        child.go("protected");
        for step in 0..3 {
            let began = Instant::now();
            while title.source().drops().secure_input <= drops_before + step
                && child.try_wait().is_none()
                && began.elapsed() < NO_ANSWER_STATE_WAIT
            {
                seen.extend(events.drain_for(Duration::from_millis(50)));
            }
            child.go(&format!("protected-{step}"));
        }
        let protected_title_drops = title.source().drops().secure_input - drops_before;
        let reading_at_the_screen_stop = gate.content_reading();
        let screen_stop = stop_counted(&mut screen, "screen");
        let frame_drops = screen.source().drops();
        child.go("end");
        while child.try_wait().is_none() {
            seen.extend(events.drain_for(Duration::from_millis(200)));
        }
        seen.extend(events.drain_for(Duration::from_millis(800)));
        let _ = stop_counted(&mut title, "window_title");
        child.print_output();
        gate_readings("no-answer-title", &gate);
        let at_no_answer = titles_with(&seen, NO_ANSWER_TITLE);
        let at_protected = titles_with(&seen, PROTECTED_NO_ANSWER_TITLE);
        let frames = count(&seen, Modality::ScreenFrame);
        say(
            "no-answer-title",
            format!(
                "title_start={} screen_start={} screen_stop={} no_answer_read={no_answer_read} \
                 protected_read={protected_read} \
                 reading_at_the_screen_stop={reading_at_the_screen_stop:?} \
                 title_events_at_no_answer={at_no_answer} \
                 title_events_at_protected_no_answer={at_protected} \
                 protected_title_drops={protected_title_drops} screen_frames={frames} \
                 screen_frame_drops_at_secure_input={}",
                show(&title_start),
                show(&screen_start),
                show(&screen_stop),
                frame_drops.secure_input
            ),
        );
        assert!(
            title_start.is_ok() && screen_start.is_ok() && screen_stop.is_ok(),
            "the sources must start and stop"
        );
        assert!(
            no_answer_read,
            "the gate did not read the box with no answer as NoAnswer"
        );
        assert_eq!(
            at_no_answer, 3,
            "a title set while the box with no answer has the focus was not kept"
        );
        assert!(
            protected_read,
            "the gate did not read the protected box as a password field"
        );
        assert_eq!(
            (at_protected, protected_title_drops),
            (0, 3),
            "the titles set while the protected box has the focus: kept, dropped"
        );
        assert_eq!(
            reading_at_the_screen_stop,
            ContentReading::Password,
            "the reading when the screen source stops"
        );
        assert_eq!(
            frames, 0,
            "a screen frame was kept at a box with no answer or at the protected box"
        );
        assert!(
            frame_drops.secure_input >= 1,
            "the screen source was offered no frame, so no frame shows nothing"
        );
    });
}

/// Waits, draining the events into `seen`, until `done` holds, the child
/// ends or `limit` passes: whether `done` held.
fn wait_for_state(
    events: &Events,
    seen: &mut Vec<Seen>,
    child: &mut ChildProcess,
    limit: Duration,
    mut done: impl FnMut(&[Seen]) -> bool,
) -> bool {
    let began = Instant::now();
    while !done(seen) && child.try_wait().is_none() && began.elapsed() < limit {
        seen.extend(events.drain_for(Duration::from_millis(20)));
    }
    done(seen)
}

/// The title source of a service set and its gate, with a child whose
/// windows come in front in turn. Every step waits for a state, never for a
/// time: the child's lead window in front and its ask settled; the password
/// window's ask dropped once the gate reads its password edit; the plain
/// window's title recorded once the gate reads its plain edit; the password
/// window's ask dropped again behind it.
#[test]
#[ignore = "functional: the title at a switch is decided by the gate's read of the window in front"]
fn a_switch_records_a_title_only_by_the_gates_read_of_the_window_in_front() {
    with_deadline("own-window", Duration::from_secs(90), || {
        env_report("own-window");
        source_built_by_test("own-window", "window_title");
        let (sink, events) = capture(4096);
        let set = ServiceSet::new();
        let gate = set.password_gate();
        let mut title = set.gated_title(WindowTitleSource::new());
        let title_start = title.begin(SourceContext::new(sink.clone()));
        let drops = |title: &halvo_capture::windows::sources::Gated<WindowTitleSource>| {
            title.source().drops().secure_input
        };
        let settled = |title: &halvo_capture::windows::sources::Gated<WindowTitleSource>| {
            let waits = title.source().waits();
            waits.at_once + waits.after_a_wait + waits.behind_the_bound + waits.ended_with_no_read
        };
        let settled_at_the_start = settled(&title);
        let mut child = spawn_child("own-window", 0);
        let child_pid = child.id();
        let mut seen = Vec::new();
        let lead_in_front = wait_for_state(
            &events,
            &mut seen,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
            |_| window_pid(foreground_window()) == child_pid,
        );
        let lead_settled = wait_for_state(
            &events,
            &mut seen,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
            |_| settled(&title) > settled_at_the_start,
        );
        let at_the_start = drops(&title);
        child.go("lead");
        let first_password_read = wait_for_reading(
            &gate,
            ContentReading::Password,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
        );
        let first_dropped = wait_for_state(
            &events,
            &mut seen,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
            |_| drops(&title) > at_the_start,
        );
        child.go("password-first");
        let plain_recorded = wait_for_state(
            &events,
            &mut seen,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
            |seen| titles_with(seen, OWN_WINDOW_PLAIN_TITLE) >= 1,
        );
        let reading_at_the_plain_title = gate.content_reading();
        let before_the_second = drops(&title);
        child.go("plain");
        let second_password_read = wait_for_reading(
            &gate,
            ContentReading::Password,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
        );
        let second_dropped = wait_for_state(
            &events,
            &mut seen,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
            |_| drops(&title) > before_the_second,
        );
        child.go("password-second");
        while child.try_wait().is_none() {
            seen.extend(events.drain_for(Duration::from_millis(200)));
        }
        seen.extend(events.drain_for(Duration::from_millis(800)));
        let title_stop = stop_counted(&mut title, "window_title");
        child.print_output();
        gate_readings("own-window", &gate);
        let waits = title.source().waits();
        say(
            "own-window",
            format!(
                "title_waits at_once={} after_a_wait={} behind_the_bound={} replaced={} \
                 left={} ended_with_no_read={} longest_wait_ms={}",
                waits.at_once,
                waits.after_a_wait,
                waits.behind_the_bound,
                waits.replaced,
                waits.left,
                waits.ended_with_no_read,
                waits.longest_wait_ms
            ),
        );
        let at_password = titles_with(&seen, OWN_WINDOW_PASSWORD_TITLE);
        let at_plain = titles_with(&seen, OWN_WINDOW_PLAIN_TITLE);
        say(
            "own-window",
            format!(
                "title_start={} title_stop={} lead_in_front={lead_in_front} \
                 lead_settled={lead_settled} first_password_read={first_password_read} \
                 first_dropped={first_dropped} plain_recorded={plain_recorded} \
                 reading_at_the_plain_title={reading_at_the_plain_title:?} \
                 second_password_read={second_password_read} second_dropped={second_dropped} \
                 title_events_of_the_password_window={at_password} \
                 title_events_of_the_plain_window={at_plain} title_drops_at_secure_input={}",
                show(&title_start),
                show(&title_stop),
                drops(&title)
            ),
        );
        assert!(
            title_start.is_ok() && title_stop.is_ok(),
            "the title source must start and stop"
        );
        assert!(
            first_password_read && first_dropped,
            "the password window's first ask: read {first_password_read}, dropped {first_dropped}"
        );
        assert!(
            plain_recorded,
            "the plain window's title was not recorded once the gate read its edit"
        );
        assert!(
            second_password_read && second_dropped,
            "the password window's second ask: read {second_password_read}, dropped {second_dropped}"
        );
        assert_eq!(
            at_password, 0,
            "a title of the window whose cursor starts in a password edit was recorded"
        );
    });
}

/// The letters that the row of keys behind a switch sends in each part.
const SWITCH_KEYS_LETTERS: usize = 3;

/// The test seam's gate marks and decided keys that the row of keys behind a
/// switch took, in their order.
#[derive(Default)]
struct SwitchSeen {
    marks: Vec<seam::MarkSeen>,
    decided: Vec<seam::Decided>,
}

impl SwitchSeen {
    fn take(&mut self) {
        self.marks.extend(seam::take_marks());
        self.decided.extend(seam::take());
    }

    /// Whether the marks from `from` hold a switch and an own read that began
    /// at least one settle time (100 ms) behind it: the read that the switch
    /// asked, or one that serves it.
    fn read_behind_a_switch(&self, from: usize) -> bool {
        self.switch_and_its_read(from).1.is_some()
    }

    /// The last switch of the marks from `from`, and the first own read that
    /// began at least 100 ms behind it.
    fn switch_and_its_read(&self, from: usize) -> (Option<u32>, Option<&seam::MarkSeen>) {
        let marks = &self.marks[from.min(self.marks.len())..];
        let switch = marks
            .iter()
            .rev()
            .find(|mark| mark.what == "switch")
            .and_then(|mark| mark.at);
        let read = switch.and_then(|at| {
            marks
                .iter()
                .find(|mark| mark.what == "read" && (mark.began.wrapping_sub(at) as i32) >= 100)
        });
        (switch, read)
    }

    /// One part's line: the switch and the read behind it, the newest read or
    /// event before the part's first key, and the part's keys.
    fn part_line(&self, part: &str, marks_from: usize, decided_from: usize) -> (String, usize) {
        let (switch, read) = self.switch_and_its_read(marks_from);
        let keys = &self.decided[decided_from.min(self.decided.len())..];
        let first_key = keys.first().map(|key| key.os_tick);
        let newest = first_key.and_then(|key| {
            self.marks[marks_from.min(self.marks.len())..]
                .iter()
                .rev()
                .find(|mark| {
                    (mark.what == "read" || mark.what == "event")
                        && mark.at.is_some_and(|at| (key.wrapping_sub(at) as i32) >= 0)
                })
        });
        let recorded = keys
            .iter()
            .filter(|key| key.verdict == seam::Verdict::Recorded)
            .count();
        let line = format!(
            "part={part} switch_seen={} read_behind_the_switch={} read_behind_the_switch_state={:?} \
             read_behind_the_switch_ms={} newest_before_the_first_key={}:{:?} letters_decided={} \
             letters_recorded={recorded} first_rule={:?}",
            switch.is_some(),
            read.is_some(),
            read.and_then(|mark| mark.state),
            match (switch, read) {
                (Some(at), Some(mark)) => i64::from(mark.began.wrapping_sub(at) as i32),
                _ => -1,
            },
            newest.map_or("none", |mark| mark.what),
            newest.and_then(|mark| mark.state),
            keys.len(),
            keys.first().and_then(|key| key.rule),
        );
        (line, recorded)
    }
}

/// Sends `count` letters, each a press and a release: how many events the
/// system took.
fn send_letters(count: usize) -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_A;
    let mut accepted = 0;
    for _ in 0..count {
        accepted += send(&[key_input(VK_A, false)]);
        accepted += send(&[key_input(VK_A, true)]);
    }
    accepted
}

/// The keyboard source and the title source of one service set, made by the
/// set as the daemon makes them, on its one gate, with a child whose windows
/// come in front in turn, each with its cursor put into its one edit and no
/// click and no focus key. Every step waits for a state: (1) the window with
/// the password edit in front, the gate reading Password, its title ask
/// dropped and a read behind its switch, then letters: none is recorded and
/// no title; (2) the window with the plain edit in front, its title recorded
/// and a read behind its switch, then letters: they are recorded.
#[test]
#[ignore = "functional: keys behind a switch alone are decided beside the read that the switch asks"]
fn keys_behind_a_switch_alone_are_decided_beside_the_read_that_the_switch_asks() {
    use halvo_capture::daemon::sources::SourceFactory;
    use halvo_control::proto::SourceName;
    with_deadline("switch-keys", Duration::from_secs(90), || {
        env_report("switch-keys");
        source_built_by_test("switch-keys", "keyboard");
        source_built_by_test("switch-keys", "window_title");
        let (sink, events) = capture(4096);
        let mut set = ServiceSet::new();
        let gate = set.password_gate();
        let mut keyboard = set
            .make(SourceName::Keyboard)
            .expect("the set makes a keyboard source");
        let mut title = set.gated_title(WindowTitleSource::new());
        let keyboard_start = keyboard.begin(SourceContext::new(sink.clone()));
        let title_start = title.begin(SourceContext::new(sink.clone()));
        let drops = |title: &halvo_capture::windows::sources::Gated<WindowTitleSource>| {
            title.source().drops().secure_input
        };
        let settled = |title: &halvo_capture::windows::sources::Gated<WindowTitleSource>| {
            let waits = title.source().waits();
            waits.at_once + waits.after_a_wait + waits.behind_the_bound + waits.ended_with_no_read
        };
        let settled_at_the_start = settled(&title);
        let mut seam_seen = SwitchSeen::default();
        seam_seen.take();
        let mut child = spawn_child("own-window", 0);
        let child_pid = child.id();
        let mut seen = Vec::new();
        let lead_in_front = wait_for_state(
            &events,
            &mut seen,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
            |_| window_pid(foreground_window()) == child_pid,
        );
        let lead_settled = wait_for_state(
            &events,
            &mut seen,
            &mut child,
            OWN_WINDOW_STATE_WAIT,
            |_| settled(&title) > settled_at_the_start,
        );
        let at_the_lead = drops(&title);
        seam_seen.take();
        let marks_of_part_1 = seam_seen.marks.len();
        child.go("lead");
        let mut ready_1 = false;
        let began = Instant::now();
        while child.try_wait().is_none() && began.elapsed() < OWN_WINDOW_STATE_WAIT {
            seen.extend(events.drain_for(Duration::from_millis(20)));
            seam_seen.take();
            if gate.content_reading() == ContentReading::Password
                && drops(&title) > at_the_lead
                && seam_seen.read_behind_a_switch(marks_of_part_1)
            {
                ready_1 = true;
                break;
            }
        }
        let decided_of_part_1 = seam_seen.decided.len();
        let sent_1 = send_letters(SWITCH_KEYS_LETTERS);
        let began = Instant::now();
        while seam_seen.decided.len() < decided_of_part_1 + SWITCH_KEYS_LETTERS
            && child.try_wait().is_none()
            && began.elapsed() < OWN_WINDOW_STATE_WAIT
        {
            seen.extend(events.drain_for(Duration::from_millis(20)));
            seam_seen.take();
        }
        let keystrokes_after_part_1 = count(&seen, Modality::Keystroke);
        let (line_1, recorded_1) =
            seam_seen.part_line("password", marks_of_part_1, decided_of_part_1);
        let marks_of_part_2 = seam_seen.marks.len();
        child.go("password-first");
        let mut ready_2 = false;
        let began = Instant::now();
        while child.try_wait().is_none() && began.elapsed() < OWN_WINDOW_STATE_WAIT {
            seen.extend(events.drain_for(Duration::from_millis(20)));
            seam_seen.take();
            if titles_with(&seen, OWN_WINDOW_PLAIN_TITLE) >= 1
                && seam_seen.read_behind_a_switch(marks_of_part_2)
            {
                ready_2 = true;
                break;
            }
        }
        let decided_of_part_2 = seam_seen.decided.len();
        let sent_2 = send_letters(SWITCH_KEYS_LETTERS);
        let began = Instant::now();
        while seam_seen.decided.len() < decided_of_part_2 + SWITCH_KEYS_LETTERS
            && child.try_wait().is_none()
            && began.elapsed() < OWN_WINDOW_STATE_WAIT
        {
            seen.extend(events.drain_for(Duration::from_millis(20)));
            seam_seen.take();
        }
        let keystrokes_after_part_2 = count(&seen, Modality::Keystroke);
        let (line_2, recorded_2) = seam_seen.part_line("plain", marks_of_part_2, decided_of_part_2);
        child.go("plain");
        child.go("password-second");
        while child.try_wait().is_none() {
            seen.extend(events.drain_for(Duration::from_millis(200)));
        }
        seen.extend(events.drain_for(Duration::from_millis(800)));
        let title_stop = stop_counted(&mut title, "window_title");
        let keyboard_stop = stop_counted(keyboard.as_mut(), "keyboard");
        child.print_output();
        gate_readings("switch-keys", &gate);
        say("switch-keys", &line_1);
        say("switch-keys", &line_2);
        let at_password = titles_with(&seen, OWN_WINDOW_PASSWORD_TITLE);
        let waits = title.source().waits();
        say(
            "switch-keys",
            format!(
                "keyboard_start={} title_start={} keyboard_stop={} title_stop={} \
                 lead_in_front={lead_in_front} lead_settled={lead_settled} ready_1={ready_1} \
                 ready_2={ready_2} sent_1={sent_1} sent_2={sent_2} \
                 keystroke_records_after_part_1={keystrokes_after_part_1} \
                 keystroke_records_after_part_2={keystrokes_after_part_2} \
                 title_events_of_the_password_window={at_password} \
                 title_events_of_the_plain_window={} title_drops_at_secure_input={} \
                 title_waits_ended_with_no_read={}",
                show(&keyboard_start),
                show(&title_start),
                show(&keyboard_stop),
                show(&title_stop),
                titles_with(&seen, OWN_WINDOW_PLAIN_TITLE),
                drops(&title),
                waits.ended_with_no_read
            ),
        );
        assert!(
            keyboard_start.is_ok() && title_start.is_ok(),
            "the sources must start"
        );
        assert!(
            ready_1 && ready_2,
            "a part's state did not come: password {ready_1}, plain {ready_2}"
        );
        assert!(
            recorded_1 == 0 && keystrokes_after_part_1 == 0 && at_password == 0,
            "a key or a title at the password edit behind a switch alone: {line_1}"
        );
        assert!(
            recorded_2 >= 1 && keystrokes_after_part_2 > keystrokes_after_part_1,
            "no key recorded at the plain edit behind a switch alone: {line_2}"
        );
    });
}

const SECOND_ACCOUNT_TEST: &str = "daemon_refuses_a_second_account";
const SECOND_ACCOUNT_TAG: &str = "second_account";
const SECOND_ACCOUNT_REQUEST: &str = "second-account-request.txt";
const SECOND_ACCOUNT_PROBE_EXE: &str = "halvo-wf-second-account.exe";
const SECOND_ACCOUNT_CTL_EXE: &str = "halvo-capture.exe";
const SECOND_ACCOUNT_ROOT_COPY: &str = "root-copy.bin";
const STANDARD_ACCOUNT_FORM: &str = "scripts/r1/ci.d/50-windows-functional.sh";
const SERVICE_RUN_BIN: &str = env!("CARGO_BIN_EXE_halvo-capture");
const SERVICE_RUN_START_WAIT: Duration = Duration::from_secs(60);
const SERVICE_RUN_STOP_WAIT: Duration = Duration::from_secs(20);
const HELD_RUN_WAIT: Duration = Duration::from_secs(30);
const HELD_NAME_WAIT: Duration = Duration::from_secs(90);
const STANDARD_ACCOUNT_FORM_WAIT: Duration = Duration::from_secs(150);
const HOLDER_END_WAIT: Duration = Duration::from_secs(60);
const HOLDER_CONNECT_WAIT: Duration = Duration::from_secs(120);
const HOLDER_READ_WAIT: Duration = Duration::from_secs(15);
const NOT_RUNNING_AT: &str = "halvo-capture: capture service not running at ";
const BOUND_IN_USE: &str = "the control channel could not be bound: address in use";
const SERVER_OF_ANOTHER_USER: &str = "the server runs as another user";
const OS_ERROR_ACCESS_DENIED: &str = "(os error 5)";

fn os_code(error: &std::io::Error) -> i32 {
    error.raw_os_error().unwrap_or(-1)
}

fn must<T>(result: std::io::Result<T>, what: &str) -> T {
    result.unwrap_or_else(|error| panic!("{what} failed with code {}", os_code(&error)))
}

fn win32_code(error: &windows::core::Error) -> u32 {
    let hresult = error.code().0 as u32;
    if hresult & 0xFFFF_0000 == 0x8007_0000 {
        hresult & 0xFFFF
    } else {
        hresult
    }
}

fn wide_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn yes_no(holds: bool) -> &'static str {
    if holds {
        "yes"
    } else {
        "no"
    }
}

fn code_text(code: Option<i32>) -> String {
    code.map_or_else(|| "none".to_string(), |code| code.to_string())
}

fn read_all(mut from: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = from.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn unprotected_len(blob: &[u8], entropy: &[u8]) -> Result<usize, u32> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let extra = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the two input blobs point into `blob` and `entropy`, which live
    // across the call and which the call only reads; `out` is a live local
    // that receives memory that LocalFree releases below.
    let done = unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&extra),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        )
    };
    if let Err(error) = done {
        return Err(error.code().0 as u32);
    }
    let len = out.cbData as usize;
    if !out.pbData.is_null() {
        // SAFETY: the call wrote `len` bytes at `out.pbData`, which this
        // block zeroes and then releases once with LocalFree.
        unsafe {
            std::ptr::write_bytes(out.pbData, 0, len);
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        }
    }
    Ok(len)
}

struct ServiceRun {
    child: std::process::Child,
    stderr: Option<std::thread::JoinHandle<String>>,
}

impl ServiceRun {
    fn start(dir: &Path) -> Self {
        let mut child = must(
            Command::new(SERVICE_RUN_BIN)
                .args(["--service-run", "--data-dir"])
                .arg(dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn(),
            "the start of a service run",
        );
        let stderr = read_all(child.stderr.take().expect("the stderr of the service run"));
        Self {
            child,
            stderr: Some(stderr),
        }
    }

    fn ended_within(&mut self, within: Duration) -> Option<i32> {
        let end = Instant::now() + within;
        loop {
            if let Some(status) = must(self.child.try_wait(), "the poll of the service run") {
                return Some(status.code().unwrap_or(-1));
            }
            if Instant::now() >= end {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn stderr_after_its_end(&mut self) -> String {
        self.stderr
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default()
    }
}

impl Drop for ServiceRun {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn service_ctl(dir: &Path, verb: &str) -> std::process::Output {
    must(
        Command::new(SERVICE_RUN_BIN)
            .arg("ctl")
            .arg("--data-dir")
            .arg(dir)
            .arg(verb)
            .stdin(Stdio::null())
            .output(),
        "a ctl of the runner",
    )
}

fn status_with_store_open(dir: &Path) -> Option<String> {
    let end = Instant::now() + SERVICE_RUN_START_WAIT;
    loop {
        let output = service_ctl(dir, "status");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.code() == Some(halvo_control::exit::CLEAN)
            && stdout.lines().any(|line| line == "store=open")
        {
            return Some(stdout);
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn runner_pipe_name(dir: &Path) -> String {
    let Ok(data_dir) = halvo_control::paths::DataDir::from_flag(dir) else {
        panic!("the scratch data directory is no data directory");
    };
    match halvo_control::client_windows::PipeConnector::for_data_dir(&data_dir) {
        Ok(connector) => connector.name().to_string(),
        Err(_) => panic!("the runner names no pipe for its scratch data directory"),
    }
}

fn write_request(shared: &Path, fields: &[(&str, &str)]) {
    let text: String = fields
        .iter()
        .map(|(key, value)| format!("{key}={value}\n"))
        .collect();
    must(
        std::fs::write(shared.join(SECOND_ACCOUNT_REQUEST), text),
        "the write of the request of the second account",
    );
}

fn grant_to_users(shared: &Path) -> bool {
    Command::new(r"C:\Windows\System32\icacls.exe")
        .arg(shared)
        .args(["/grant", "*S-1-5-32-545:(OI)(CI)RX", "/T", "/Q"])
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

struct FormRun {
    child: std::process::Child,
    stdout: Option<std::thread::JoinHandle<String>>,
    stderr: Option<std::thread::JoinHandle<String>>,
}

struct FormOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl FormRun {
    fn helper(probe_exe: &Path) -> Self {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the folder above the crates");
        let mut child = must(
            Command::new("sh")
                .current_dir(repo)
                .arg(STANDARD_ACCOUNT_FORM)
                .args(["--as-standard-account", "--"])
                .arg(probe_exe)
                .args([
                    "helper_second_account",
                    "--exact",
                    "--ignored",
                    "--nocapture",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn(),
            "the start of the form of check 50 by sh",
        );
        let stdout = read_all(child.stdout.take().expect("the stdout of the form"));
        let stderr = read_all(child.stderr.take().expect("the stderr of the form"));
        Self {
            child,
            stdout: Some(stdout),
            stderr: Some(stderr),
        }
    }

    fn ended(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    fn finish(mut self, within: Duration) -> Option<FormOutput> {
        let end = Instant::now() + within;
        let code = loop {
            if let Some(status) = must(self.child.try_wait(), "the poll of the form") {
                break status.code();
            }
            if Instant::now() >= end {
                let _ = self.child.kill();
                let _ = self.child.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let joined = |reader: Option<std::thread::JoinHandle<String>>| {
            reader
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default()
        };
        Some(FormOutput {
            code,
            stdout: joined(self.stdout.take()),
            stderr: joined(self.stderr.take()),
        })
    }
}

fn form_lines(test: &str, run: &str, output: &FormOutput) -> BTreeMap<String, String> {
    for line in output.stderr.lines().map(str::trim_end) {
        if !line.starts_with("standard_account") {
            continue;
        }
        if line
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_=- ".contains(&b))
        {
            say(test, format!("form run={run} {line}"));
        } else {
            say(
                test,
                format!("form run={run} standard_account_line=withheld"),
            );
        }
    }
    if output.stdout.lines().any(|line| line.starts_with("FAIL ")) {
        say(test, format!("form run={run} refused=yes"));
    }
    say(
        test,
        format!("form run={run} exit={}", code_text(output.code)),
    );
    let prefix = format!("TF {SECOND_ACCOUNT_TAG} ");
    output
        .stdout
        .lines()
        .filter_map(|line| line.trim_end().strip_prefix(prefix.as_str()))
        .flat_map(str::split_whitespace)
        .filter_map(|token| token.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn got(fields: &BTreeMap<String, String>, key: &str) -> String {
    fields
        .get(key)
        .cloned()
        .unwrap_or_else(|| "none".to_string())
}

fn held_name_stands(name: &str, holder: &mut FormRun) -> bool {
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    let name = wide_nul(name);
    let end = Instant::now() + HELD_NAME_WAIT;
    loop {
        // SAFETY: `name` ends in a NUL and outlives the call; no other pointer
        // is passed.
        let waited = unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), 1) };
        if waited.as_bool() || std::io::Error::last_os_error().raw_os_error() != Some(2) {
            return true;
        }
        if holder.ended() || Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn check(faults: &mut Vec<&'static str>, holds: bool, part: &'static str) {
    if !holds {
        faults.push(part);
    }
}

#[test]
#[ignore = "functional: starts the built daemon and probes it from the standard account that the form of check 50 makes on a hosted runner alone"]
fn daemon_refuses_a_second_account() {
    let test = SECOND_ACCOUNT_TEST;
    let mut faults = Vec::new();
    let base = std::path::PathBuf::from(
        std::env::var_os("LOCALAPPDATA").expect("the variable of the local app data of the runner"),
    );
    let under_profile =
        std::env::var_os("USERPROFILE").is_some_and(|profile| base.starts_with(profile));
    say(
        test,
        format!(
            "data_dir_base=local_app_data under_own_profile={}",
            yes_no(under_profile)
        ),
    );
    check(&mut faults, under_profile, "place");
    let stamp = format!("{}-{}", std::process::id(), unix_millis(SystemTime::now()));
    let dir = base.join(["halvo-wf-second-account-", &stamp].concat());
    let held_dir = base.join(["halvo-wf-second-account-held-", &stamp].concat());
    let dir_text = dir
        .to_str()
        .expect("a data directory that is valid text")
        .to_string();

    let mut daemon = ServiceRun::start(&dir);
    let Some(status) = status_with_store_open(&dir) else {
        panic!(
            "the service run gave no status with store=open within {}s",
            SERVICE_RUN_START_WAIT.as_secs()
        );
    };
    let platform_windows = status.lines().any(|line| line == "platform=windows");
    say(
        test,
        format!(
            "control ctl=status exit=0 platform_windows={} store=open",
            yes_no(platform_windows)
        ),
    );
    check(&mut faults, platform_windows, "control");

    let root_files: Vec<(std::path::PathBuf, String)> = must(
        std::fs::read_dir(dir.join("roots")),
        "the listing of the roots by the runner",
    )
    .filter_map(Result::ok)
    .filter_map(|entry| {
        let name = entry.file_name().into_string().ok()?;
        let id = name.strip_suffix(".root")?.to_string();
        Some((entry.path(), id))
    })
    .collect();
    assert!(
        root_files.len() == 1,
        "the roots of the data directory hold {} root files and not one",
        root_files.len()
    );
    let (root_file, root_id_text) = root_files.into_iter().next().expect("the one root file");
    let Some(root_id) = halvo_capture::custody::RootId::parse(&root_id_text) else {
        panic!("the name of the root file holds no root id");
    };

    let shared = scratch_dir();
    let probe_exe = shared.path().join(SECOND_ACCOUNT_PROBE_EXE);
    let ctl_exe = shared.path().join(SECOND_ACCOUNT_CTL_EXE);
    let root_copy = shared.path().join(SECOND_ACCOUNT_ROOT_COPY);
    must(
        std::fs::copy(
            must(std::env::current_exe(), "the read of the test binary"),
            &probe_exe,
        ),
        "the copy of the test binary",
    );
    must(
        std::fs::copy(SERVICE_RUN_BIN, &ctl_exe),
        "the copy of the daemon",
    );
    must(
        std::fs::copy(&root_file, &root_copy),
        "the copy of the root file",
    );
    let copy_bytes = must(
        std::fs::read(&root_copy),
        "the read of the root copy by the runner",
    );
    let runner_unprotects = unprotected_len(&copy_bytes, root_id.as_bytes());
    say(
        test,
        format!(
            "probe=3 control runner_unprotects={} len={}",
            yes_no(runner_unprotects.is_ok()),
            runner_unprotects.unwrap_or(0)
        ),
    );

    let runner_user = must(
        halvo_control::client_windows::current_user(),
        "the read of the runner's own user",
    );
    let runner_pipe = runner_pipe_name(&dir);
    let default_pipe = [r"\\.\pipe\halvo-wf-default-descriptor-", &stamp].concat();
    let default_held = default_descriptor_pipe(&default_pipe);
    say(
        test,
        format!(
            "probe=4 control default_descriptor_pipe made={}",
            yes_no(default_held.is_ok())
        ),
    );
    write_request(
        shared.path(),
        &[
            ("mode", "probes"),
            ("data_dir", &dir_text),
            ("runner_pipe", &runner_pipe),
            ("default_pipe", &default_pipe),
            ("runner_user", &runner_user),
            (
                "ctl",
                ctl_exe.to_str().expect("a copy path that is valid text"),
            ),
            (
                "root_copy",
                root_copy.to_str().expect("a copy path that is valid text"),
            ),
            ("root_id", &root_id_text),
        ],
    );
    let granted = grant_to_users(shared.path());
    say(
        test,
        format!(
            "shared_folder copies=3 read_and_execute_granted={}",
            yes_no(granted)
        ),
    );
    check(&mut faults, granted, "grant");

    let Some(probes) = FormRun::helper(&probe_exe).finish(STANDARD_ACCOUNT_FORM_WAIT) else {
        panic!(
            "the form of the probes did not end within {}s",
            STANDARD_ACCOUNT_FORM_WAIT.as_secs()
        );
    };
    let a = form_lines(test, "probes", &probes);
    check(&mut faults, probes.code == Some(0), "form_probes");
    say(
        test,
        format!(
            "second_account own_user={} administrators_member={}",
            got(&a, "own_user"),
            got(&a, "administrators_member")
        ),
    );
    check(
        &mut faults,
        got(&a, "own_user") == "other" && got(&a, "administrators_member") == "no",
        "second_account",
    );
    say(
        test,
        format!(
            "probe=1 ctl=status exit={} status_lines={} endpoint={} runner_pipe_reached={}",
            got(&a, "ctl_exit"),
            got(&a, "ctl_stdout_lines"),
            got(&a, "ctl_endpoint"),
            yes_no(got(&a, "ctl_endpoint") == "runner_pipe")
        ),
    );
    check(
        &mut faults,
        got(&a, "ctl_exit") == halvo_control::exit::UNREACHABLE.to_string()
            && got(&a, "ctl_stdout_lines") == "0"
            && got(&a, "ctl_endpoint") == "own_pipe",
        "probe_1",
    );
    say(
        test,
        format!(
            "probe=2 listing={} code={}",
            got(&a, "listing"),
            got(&a, "listing_code")
        ),
    );
    check(
        &mut faults,
        got(&a, "listing") == "failed" && got(&a, "listing_code") == "5",
        "probe_2",
    );
    say(
        test,
        format!(
            "probe=3 root_copy_read={} copy_len={} second_account_unprotects={} code={}",
            got(&a, "root_copy_read"),
            copy_bytes.len(),
            got(&a, "root_copy_unprotects"),
            got(&a, "root_copy_code")
        ),
    );
    check(
        &mut faults,
        runner_unprotects == Ok(32)
            && got(&a, "root_copy_read") == copy_bytes.len().to_string()
            && got(&a, "root_copy_unprotects") == "no",
        "probe_3",
    );
    say(
        test,
        format!(
            "probe=4 runner_pipe_open={} code={}",
            got(&a, "runner_pipe_open"),
            got(&a, "runner_pipe_code")
        ),
    );
    say(
        test,
        format!(
            "probe=4 runner_pipe_read_open={} code={}",
            got(&a, "runner_pipe_read_open"),
            got(&a, "runner_pipe_read_code")
        ),
    );
    say(
        test,
        format!(
            "probe=4 control default_descriptor_pipe write_open={} write_code={} read_open={} read_code={}",
            got(&a, "default_pipe_write_open"),
            got(&a, "default_pipe_write_code"),
            got(&a, "default_pipe_read_open"),
            got(&a, "default_pipe_read_code")
        ),
    );
    drop(default_held);
    check(
        &mut faults,
        got(&a, "runner_pipe_open") == "failed"
            && got(&a, "runner_pipe_code") == "5"
            && got(&a, "runner_pipe_read_open") == "failed"
            && got(&a, "runner_pipe_read_code") == "5",
        "probe_4",
    );
    check(
        &mut faults,
        got(&a, "default_pipe_read_open") == "opened"
            || got(&a, "default_pipe_write_open") == "opened",
        "probe_4_control",
    );

    must(
        std::fs::create_dir_all(&held_dir),
        "the make of the second data directory",
    );
    let held_pipe = runner_pipe_name(&held_dir);
    write_request(
        shared.path(),
        &[("mode", "hold"), ("runner_pipe", &held_pipe)],
    );
    let mut holder = FormRun::helper(&probe_exe);
    let stands = held_name_stands(&held_pipe, &mut holder);
    say(test, format!("probe=5 held_name_stands={}", yes_no(stands)));
    let (held_exit, bound_in_use) = if stands {
        let mut held_run = ServiceRun::start(&held_dir);
        let held_exit = held_run.ended_within(HELD_RUN_WAIT);
        let bound_in_use =
            held_exit.is_some() && held_run.stderr_after_its_end().contains(BOUND_IN_USE);
        (held_exit, bound_in_use)
    } else {
        (None, false)
    };
    say(
        test,
        format!(
            "probe=5 service_run=held_name exit={} bound_in_use={}",
            code_text(held_exit),
            yes_no(bound_in_use)
        ),
    );
    let refused = service_ctl(&held_dir, "status");
    let refused_err = String::from_utf8_lossy(&refused.stderr).into_owned();
    let refusal = if refused_err.contains(OS_ERROR_ACCESS_DENIED) {
        "io_access_denied"
    } else if refused_err.contains(SERVER_OF_ANOTHER_USER) {
        "server_of_another_user"
    } else {
        "other"
    };
    let refused_lines = String::from_utf8_lossy(&refused.stdout).lines().count();
    say(
        test,
        format!(
            "probe=5 ctl=status exit={} status_lines={refused_lines} refusal={refusal}",
            code_text(refused.status.code())
        ),
    );
    let held = holder.finish(HOLDER_END_WAIT);
    let b = match &held {
        Some(output) => form_lines(test, "hold", output),
        None => {
            say(test, "form run=hold ended=no");
            BTreeMap::new()
        }
    };
    say(
        test,
        format!(
            "probe=5 holder_made={} holder_connected={} holder_bytes={} holder_read={}",
            got(&b, "holder_made"),
            got(&b, "holder_connected"),
            got(&b, "holder_bytes"),
            got(&b, "holder_read")
        ),
    );
    check(
        &mut faults,
        stands
            && held_exit == Some(halvo_control::exit::ERROR)
            && bound_in_use
            && refused.status.code() == Some(halvo_control::exit::ERROR)
            && refused_lines == 0
            && refusal != "other"
            && held.as_ref().is_some_and(|output| output.code == Some(0))
            && got(&b, "holder_made") == "yes"
            && got(&b, "holder_connected") == "yes"
            && got(&b, "holder_bytes") == "0"
            && got(&b, "holder_read") == "end",
        "probe_5",
    );

    let stopped = service_ctl(&dir, "stop");
    let flushed = String::from_utf8_lossy(&stopped.stdout).starts_with("stopped flushed=yes");
    let ended = daemon.ended_within(SERVICE_RUN_STOP_WAIT);
    say(
        test,
        format!(
            "stop ctl_exit={} flushed={} service_run_exit={}",
            code_text(stopped.status.code()),
            yes_no(flushed),
            code_text(ended)
        ),
    );
    check(
        &mut faults,
        stopped.status.code() == Some(halvo_control::exit::CLEAN)
            && flushed
            && ended == Some(halvo_control::exit::CLEAN),
        "stop",
    );
    assert!(
        faults.is_empty(),
        "these parts failed: {}",
        faults.join(" ")
    );
}

#[test]
#[ignore = "helper: does nothing unless daemon_refuses_a_second_account starts it as the second account"]
fn helper_second_account() {
    let Some(request) = second_account_request() else {
        return;
    };
    match request.get("mode").map(String::as_str) {
        Some("probes") => second_account_probes(&request),
        Some("hold") => second_account_hold(&request),
        _ => panic!("the request of the second account names no mode"),
    }
}

fn second_account_request() -> Option<BTreeMap<String, String>> {
    let exe = std::env::current_exe().ok()?;
    let text = std::fs::read_to_string(exe.parent()?.join(SECOND_ACCOUNT_REQUEST)).ok()?;
    Some(
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    )
}

fn requested<'a>(request: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    request
        .get(key)
        .map(String::as_str)
        .unwrap_or_else(|| panic!("the request of the second account has no {key}"))
}

fn administrators_member() -> &'static str {
    use windows::Win32::Security::{
        CheckTokenMembership, CreateWellKnownSid, WinBuiltinAdministratorsSid, PSID,
    };
    let mut sid = [0u32; 17];
    let mut size = std::mem::size_of_val(&sid) as u32;
    let sid_ptr = PSID(sid.as_mut_ptr().cast());
    // SAFETY: `sid` holds `size` bytes, aligned for a SID, and lives across
    // the call, which writes the SID of the Administrators group into it.
    if unsafe { CreateWellKnownSid(WinBuiltinAdministratorsSid, None, Some(sid_ptr), &mut size) }
        .is_err()
    {
        return "unknown";
    }
    let mut member = windows::core::BOOL::default();
    // SAFETY: `sid_ptr` points to the SID that the call above wrote into
    // `sid`, which lives across this call, and `member` is a live local.
    match unsafe { CheckTokenMembership(None, sid_ptr, &mut member) } {
        Ok(()) if member.as_bool() => "yes",
        Ok(()) => "no",
        Err(_) => "unknown",
    }
}

fn open_pipe(name: &str, access: u32) -> Result<(), u32> {
    use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_BUSY};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_NONE, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    };
    let name = wide_nul(name);
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        // SAFETY: `name` ends in a NUL and outlives the call; no other pointer
        // is passed.
        let opened = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                access,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
        };
        match opened {
            Ok(handle) => {
                // SAFETY: `handle` came from CreateFileW above and is closed
                // once.
                let _ = unsafe { CloseHandle(handle) };
                return Ok(());
            }
            Err(error) if win32_code(&error) == ERROR_PIPE_BUSY.0 && Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(error) => return Err(win32_code(&error)),
        }
    }
}

fn second_account_probes(request: &BTreeMap<String, String>) {
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    let tag = SECOND_ACCOUNT_TAG;
    let own_user = match halvo_control::client_windows::current_user() {
        Ok(sid) if sid == requested(request, "runner_user") => "same",
        Ok(_) => "other",
        Err(_) => "unread",
    };
    say(
        tag,
        format!(
            "own_user={own_user} administrators_member={}",
            administrators_member()
        ),
    );

    let data_dir = requested(request, "data_dir");
    let runner_pipe = requested(request, "runner_pipe");
    let own_pipe = halvo_control::paths::DataDir::from_flag(data_dir)
        .ok()
        .and_then(|dir| halvo_control::client_windows::PipeConnector::for_data_dir(&dir).ok())
        .map(|connector| connector.name().to_string());
    match Command::new(requested(request, "ctl"))
        .args(["ctl", "--data-dir", data_dir, "status"])
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let endpoint = stderr
                .lines()
                .find_map(|line| line.trim_end().strip_prefix(NOT_RUNNING_AT));
            let place = match endpoint {
                Some(name) if name == runner_pipe => "runner_pipe",
                Some(name) if Some(name) == own_pipe.as_deref() => "own_pipe",
                Some(_) => "other",
                None => "none",
            };
            say(
                tag,
                format!(
                    "ctl_exit={} ctl_stdout_lines={} ctl_endpoint={place}",
                    code_text(output.status.code()),
                    String::from_utf8_lossy(&output.stdout).lines().count()
                ),
            );
        }
        Err(error) => say(
            tag,
            format!("ctl_exit=not_started ctl_start_code={}", os_code(&error)),
        ),
    }

    match std::fs::read_dir(data_dir) {
        Ok(_) => say(tag, "listing=read listing_code=0"),
        Err(error) => say(
            tag,
            format!("listing=failed listing_code={}", os_code(&error)),
        ),
    }

    let Some(root_id) = halvo_capture::custody::RootId::parse(requested(request, "root_id")) else {
        panic!("the request of the second account names no root id");
    };
    match std::fs::read(requested(request, "root_copy")) {
        Ok(blob) => {
            let unprotected = match unprotected_len(&blob, root_id.as_bytes()) {
                Ok(len) => format!("root_copy_unprotects=yes root_copy_len={len}"),
                Err(code) => format!("root_copy_unprotects=no root_copy_code={code:#010x}"),
            };
            say(tag, format!("root_copy_read={} {unprotected}", blob.len()));
        }
        Err(error) => say(
            tag,
            format!(
                "root_copy_read=failed root_copy_read_code={}",
                os_code(&error)
            ),
        ),
    }

    let read_write = (GENERIC_READ | GENERIC_WRITE).0;
    let read_alone = GENERIC_READ.0;
    let default_pipe = requested(request, "default_pipe");
    for (key, name, access) in [
        ("runner_pipe", runner_pipe, read_write),
        ("runner_pipe_read", runner_pipe, read_alone),
        ("default_pipe_write", default_pipe, read_write),
        ("default_pipe_read", default_pipe, read_alone),
    ] {
        match open_pipe(name, access) {
            Ok(()) => say(tag, format!("{key}_open=opened {key}_code=0")),
            Err(code) => say(tag, format!("{key}_open=failed {key}_code={code}")),
        }
    }
}

fn default_descriptor_pipe(name: &str) -> Result<std::os::windows::io::OwnedHandle, u32> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    let name = wide_nul(name);
    // SAFETY: `name` ends in a NUL and outlives the call; no security
    // attributes are passed, so the pipe takes the system's default
    // descriptor.
    let pipe = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4_096,
            4_096,
            0,
            None,
        )
    };
    let failed = std::io::Error::last_os_error();
    if pipe.is_invalid() {
        return Err(failed.raw_os_error().map_or(u32::MAX, |code| code as u32));
    }
    // SAFETY: `pipe` is a new handle that nothing else owns or closes.
    Ok(unsafe { OwnedHandle::from_raw_handle(pipe.0) })
}

fn hold_pipe(name: &str) -> Result<std::os::windows::io::OwnedHandle, u32> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    let sddl = wide_nul("D:(A;;GA;;;WD)");
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sddl` ends in a NUL and outlives the call, and `descriptor` is a
    // live local that receives memory that LocalFree releases below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|error| win32_code(&error))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let name = wide_nul(name);
    // SAFETY: `name` ends in a NUL, and `attributes` and the descriptor that it
    // points to live across the call.
    let pipe = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4_096,
            4_096,
            0,
            Some(&attributes),
        )
    };
    let failed = std::io::Error::last_os_error();
    // SAFETY: the descriptor came from the conversion above, whose memory
    // LocalFree releases, once.
    let _ = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    if pipe.is_invalid() {
        return Err(failed.raw_os_error().map_or(u32::MAX, |code| code as u32));
    }
    // SAFETY: `pipe` is a new handle that nothing else owns or closes.
    Ok(unsafe { OwnedHandle::from_raw_handle(pipe.0) })
}

enum Held {
    Connected(bool),
    Read(Result<usize, i32>),
}

fn second_account_hold(request: &BTreeMap<String, String>) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{ERROR_PIPE_CONNECTED, HANDLE};
    use windows::Win32::System::Pipes::ConnectNamedPipe;
    let tag = SECOND_ACCOUNT_TAG;
    let pipe = match hold_pipe(requested(request, "runner_pipe")) {
        Ok(pipe) => pipe,
        Err(code) => {
            say(tag, format!("holder_made=no holder_made_code={code}"));
            return;
        }
    };
    say(tag, "holder_made=yes");
    let (tell, told) = mpsc::channel();
    std::thread::spawn(move || {
        // SAFETY: `pipe` is the open handle of a pipe instance that this
        // thread owns for the whole call.
        let connected = match unsafe { ConnectNamedPipe(HANDLE(pipe.as_raw_handle()), None) } {
            Ok(()) => true,
            Err(error) => win32_code(&error) == ERROR_PIPE_CONNECTED.0,
        };
        let _ = tell.send(Held::Connected(connected));
        if connected {
            let mut file = std::fs::File::from(pipe);
            let mut bytes = [0u8; 4_096];
            let read = file.read(&mut bytes).map_err(|error| os_code(&error));
            let _ = tell.send(Held::Read(read));
        }
    });
    let connected = match told.recv_timeout(HOLDER_CONNECT_WAIT) {
        Ok(Held::Connected(true)) => "yes",
        Ok(_) => "failed",
        Err(_) => "no",
    };
    let read = if connected == "yes" {
        match told.recv_timeout(HOLDER_READ_WAIT) {
            Ok(Held::Read(Ok(0))) => "holder_bytes=0 holder_read=end".to_string(),
            Ok(Held::Read(Ok(count))) => format!("holder_bytes={count} holder_read=request"),
            Ok(Held::Read(Err(code))) => {
                format!("holder_bytes=0 holder_read=failed holder_read_code={code}")
            }
            _ => "holder_bytes=0 holder_read=timed_out".to_string(),
        }
    } else {
        "holder_bytes=0 holder_read=none".to_string()
    };
    say(tag, format!("holder_connected={connected} {read}"));
}
