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
use halvo_capture::secure_input::windows::PasswordGate;
use halvo_capture::sink::{channel, SinkHandle, SinkWriter};
use halvo_capture::source::{CaptureError, CaptureSource, SourceContext};
use halvo_capture::windows::apps::scan_installed;
use halvo_capture::windows::foreground::{current_foreground_app, ForegroundAppSource};
use halvo_capture::windows::input::{seam, RawInputSource};
use halvo_capture::windows::screen::ScreenSource;
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
    KEYEVENTF_KEYUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_WHEEL, MOUSEINPUT, VIRTUAL_KEY, VK_F15, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    FindWindowW, GetForegroundWindow, GetSystemMetrics, GetWindowDisplayAffinity,
    GetWindowThreadProcessId, PeekMessageW, RegisterClassW, SetForegroundWindow,
    SetWindowDisplayAffinity, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage,
    UnregisterClassW, MSG, PM_REMOVE, SC_KEYMENU, SM_CMONITORS, SM_CXSCREEN, SM_CYSCREEN,
    SM_REMOTESESSION, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
    SYSTEM_METRICS_INDEX, WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SYSCOMMAND,
    WNDCLASSW, WS_CHILD, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
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
const CHILD_MODE_ENV: &str = "HALVO_VERIFY_CHILD_MODE";
const LISTENER_OUT_ENV: &str = "HALVO_VERIFY_LISTENER_OUT";
const LISTENER_WINDOW_CLASS: &str = "HalvoVerifyListenerWindow";
const FIXED_TITLE: &str = "halvo-verify-second-window-with-a-fixed-title";
const MAGENTA: u32 = 0x00FF_00FF;
const FIRST_FRAME_WAIT: Duration = Duration::from_secs(10);
const FULL_SINK_STOP_LIMIT: Duration = Duration::from_secs(2);
const BOUNDS_FILE: &str = "docs/baseline/r1-gate-bounds.txt";

fn say(test: &str, message: impl AsRef<str>) {
    println!("TF {test} {}", message.as_ref());
}

fn show<T, E: std::fmt::Display>(result: &Result<T, E>) -> String {
    match result {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("error({e})"),
    }
}

fn with_deadline<F>(test: &'static str, limit: Duration, body: F)
where
    F: FnOnce() + Send + 'static,
{
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

fn stop_counted<S: CaptureSource>(source: &mut S, name: &str) -> Result<(), CaptureError> {
    let stopped = source.shutdown();
    println!("{SHUTDOWN_LINE}{name} ok={}", stopped.is_ok());
    stopped
}

fn source_built_by_test(test: &str, source: &str) {
    say(test, format!("source_built_by_test={source}"));
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

fn move_pointer(moves: u32) -> u32 {
    let mut accepted = 0;
    for i in 0..moves {
        let dx = if i.is_multiple_of(2) { 3 } else { -3 };
        accepted += send(&[mouse_move(dx, 0)]);
        std::thread::sleep(Duration::from_millis(5));
    }
    accepted
}

struct Events(mpsc::Receiver<CaptureStreamEvent>);

impl Events {
    fn drain_for(&self, window: Duration) -> Vec<CaptureStreamEvent> {
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

    fn first_within(&self, limit: Duration) -> Option<CaptureStreamEvent> {
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
    std::thread::Builder::new()
        .name("tf-sink-forward".to_string())
        .spawn(move || {
            while let Some(event) = writer.recv() {
                if tx.send(event).is_err() {
                    break;
                }
            }
        })
        .expect("spawn the sink forwarder");
    Events(rx)
}

fn capture(capacity: usize) -> (SinkHandle, Events) {
    let (sink, writer) = channel(capacity);
    (sink, forward(writer))
}

fn count(events: &[CaptureStreamEvent], modality: Modality) -> usize {
    events.iter().filter(|e| e.modality == modality).count()
}

fn summarize(test: &str, phase: &str, events: &[CaptureStreamEvent]) {
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
    pump_thread(Duration::from_millis(300));
    let (foreground, method) = first.activate();
    say(
        "child",
        format!("first_window_foreground={foreground} method={method}"),
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
            set_foreground(target.hwnd);
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
    pump_thread(Duration::from_millis(300));
    let (foreground, method) = first.activate();
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
    let dir = scratch_dir();
    let exe = dir.path().join(CHILD_EXE_NAME);
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

fn window_scenario(test: &'static str) -> WindowScenario {
    env_report(test);
    let (sink, events) = capture(8192);
    let mut foreground = ForegroundAppSource::new();
    let last_app = foreground.last_app();
    let mut title = WindowTitleSource::new();
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

        let key_events_accepted = press_keys(PRESSES);
        let after_keys = events.drain_for(Duration::from_millis(1500));
        summarize("c1", "after-key-presses", &after_keys);

        let move_events_accepted = move_pointer(MOVES);
        let after_moves = events.drain_for(Duration::from_millis(1500));
        summarize("c1", "after-pointer-moves", &after_moves);

        let _ = stop_counted(&mut keyboard, "keyboard");
        let _ = stop_counted(&mut pointer, "pointer");
        press_keys(3);
        move_pointer(3);
        let after_stop = events.drain_for(Duration::from_millis(700));
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
                 keystroke_events={keystrokes} pointer_moves_sent={MOVES} \
                 sendinput_move_events_accepted={move_events_accepted} pointer_events={pointers} \
                 events_before_input={} events_after_stop={} attributed_to_last_app={attributed}",
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
        let (cx, cy) = screen_size();
        let expected_len = cx as u64 * cy as u64 * 4;
        let (sink, events) = capture(1024);
        let last_app = Arc::new(Mutex::new(Some(ATTRIBUTION.to_string())));
        let mut screen = ScreenSource::new(last_app);
        let started = Instant::now();
        let start = screen.begin(SourceContext::new(sink.clone()));
        say(
            "c2",
            format!(
                "screen_start={} start_ms={}",
                show(&start),
                started.elapsed().as_millis()
            ),
        );
        assert!(start.is_ok(), "the screen source must start");

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
        let mut idle: Vec<CaptureStreamEvent> = first.into_iter().collect();
        idle.extend(events.drain_for(Duration::from_secs(4)));
        summarize("c2", "unchanged-screen", &idle);

        let window = TestWindow::create(&magenta_popup("HalvoVerifyFrames", "halvo-verify-frames"))
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
        assert!(
            frames
                .iter()
                .all(|e| e.segment_ref.byte_len == expected_len),
            "a frame event does not have the size of the full screen"
        );
        assert!(
            frames.iter().all(|e| e.foreground_app == ATTRIBUTION),
            "a frame event does not carry the app in the shared cell"
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
        let found = with_probe.apps.iter().find(|a| a.name == probe_name);
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
        let mut title = WindowTitleSource::new();
        let _fields = FieldWindow::open(
            "c9_peak_working_set_of_sources_1_to_4_for_60_seconds",
            Field::Plain,
        );
        let mut keyboard = RawInputSource::keyboard(Arc::clone(&last_app), PasswordGate::new());
        let mut pointer = RawInputSource::pointer(Arc::clone(&last_app));
        let mut screen = ScreenSource::new(Arc::clone(&last_app));
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
            ("screen", screen.begin(SourceContext::new(sink.clone()))),
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
        let before = yellow_edge_pixels();
        let (sink, events) = capture(256);
        let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
        let start = screen.begin(SourceContext::new(sink.clone()));
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
        assert!(start.is_ok(), "the screen source must start");
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
        let (sink, events) = capture(1024);
        let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
        let start = screen.begin(SourceContext::new(sink.clone()));
        let before = events.drain_for(Duration::from_secs(3));
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
        let after = events.drain_for(Duration::from_secs(2));
        summarize("x9", "after-mode-change", &after);
        let _ = stop_counted(&mut screen, "screen");
        let restored = set_resolution(start_w, start_h);
        let with_new_size = after
            .iter()
            .filter(|e| e.segment_ref.byte_len == now_w as u64 * now_h as u64 * 4)
            .count();
        say(
            "x9",
            format!(
                "screen_start={} mode_before={start_w}x{start_h} mode_change_command_ok={changed} \
                 mode_after={now_w}x{now_h} frames_before={} frames_after_change={} \
                 frames_after_with_new_size={with_new_size} frames_after_with_old_size={} \
                 mode_restored={restored}",
                show(&start),
                count(&before, Modality::ScreenFrame),
                count(&after, Modality::ScreenFrame),
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
        assert!(
            with_new_size >= 1,
            "no frame of the new display size after the mode change"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn x13_frames_on_a_static_screen() {
    with_deadline("x13", Duration::from_secs(90), || {
        env_report("x13");
        source_built_by_test("x13", "screen");
        let cover = TestWindow::create(&magenta_popup("HalvoVerifyStatic", "halvo-verify-static"))
            .expect("covering window");
        cover.cover_screen();
        pump_thread(Duration::from_secs(1));
        let (sink, events) = capture(1024);
        let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
        let start = screen.begin(SourceContext::new(sink.clone()));
        let mut frames = 0;
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(8) {
            pump_thread(Duration::from_millis(100));
            frames += count(
                &events.drain_for(Duration::from_millis(100)),
                Modality::ScreenFrame,
            );
        }
        let covered = cover.is_foreground();
        drop(cover);
        let after_uncover = count(
            &events.drain_for(Duration::from_secs(2)),
            Modality::ScreenFrame,
        );
        let _ = stop_counted(&mut screen, "screen");
        say(
            "x13",
            format!(
                "screen_start={} static_screen_secs=8 frames_while_static={frames} \
                 cover_window_was_foreground={covered} frames_in_2s_after_uncover={after_uncover}",
                show(&start)
            ),
        );
        assert!(start.is_ok(), "the screen source must start");
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn pointer_moves_coalesce() {
    with_deadline("coalesce", Duration::from_secs(60), || {
        const MOVES: u32 = 100;
        env_report("coalesce");
        source_built_by_test("coalesce", "pointer");
        let (sink, events) = capture(8192);
        let mut pointer = RawInputSource::pointer(Arc::new(Mutex::new(None)));
        let start = pointer.begin(SourceContext::new(sink.clone()));
        let first = move_pointer(MOVES / 2);
        let wheel = send(&[wheel_notch()]);
        let second = move_pointer(MOVES / 2);
        let pointers = count(
            &events.drain_for(Duration::from_millis(1500)),
            Modality::Pointer,
        );
        std::thread::sleep(Duration::from_millis(300));
        let coalesced_before = pointer.handoff_counts().coalesced;
        let mut segment_sent = send(&[mouse_move(3, 0)]);
        std::thread::sleep(Duration::from_millis(10));
        segment_sent += send(&[wheel_notch()]);
        std::thread::sleep(Duration::from_millis(10));
        segment_sent += send(&[mouse_move(-3, 0)]);
        let segment = count(
            &events.drain_for(Duration::from_millis(700)),
            Modality::Pointer,
        );
        let segment_coalesced = pointer.handoff_counts().coalesced - coalesced_before;
        let _ = stop_counted(&mut pointer, "pointer");
        let counts = pointer.handoff_counts();
        let accepted = first + second;
        say(
            "coalesce",
            format!(
                "pointer_start={} moves_sent={MOVES} sendinput_move_events_accepted={accepted} \
                 sendinput_wheel_events_accepted={wheel} pointer_events={pointers} \
                 coalesced={} lost={} move_notch_move_accepted={segment_sent} \
                 move_notch_move_events={segment} move_notch_move_coalesced={segment_coalesced}",
                show(&start),
                counts.coalesced,
                counts.lost,
            ),
        );
        assert!(start.is_ok(), "the pointer source must start");
        assert!(
            accepted == MOVES && wheel == 1,
            "INCONCLUSIVE: SendInput did not insert every move and the wheel notch"
        );
        assert!(pointers >= 1, "no pointer event at all");
        assert!(
            pointers < accepted as usize,
            "every move gave its own pointer event"
        );
        assert_eq!(counts.lost, 0, "the hand-off lost packets");
        assert_eq!(
            pointers as u64 + counts.coalesced,
            u64::from(accepted) + 1,
            "the wheel packet inside the burst is not its own pointer event"
        );
        assert_eq!(
            segment_sent, 3,
            "INCONCLUSIVE: SendInput did not insert the move, the notch and the move"
        );
        assert_eq!(
            (segment, segment_coalesced),
            (3, 0),
            "a wheel notch 10 ms after a move and 10 ms before the next was folded into the motion sample"
        );
    });
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
        let mut injected = Vec::new();
        let mut accepted = 0;
        for _ in 0..ROUNDS {
            accepted += send(&[key_input(VK_F15, false)]);
            injected.push((Modality::Keystroke, SystemTime::now()));
            accepted += send(&[key_input(VK_F15, true)]);
            std::thread::sleep(Duration::from_millis(100));
            accepted += send(&[wheel_notch()]);
            injected.push((Modality::Pointer, SystemTime::now()));
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_millis(1500));
        let events = forward(writer).drain_for(Duration::from_secs(2));
        let _ = stop_counted(&mut keyboard, "keyboard");
        let _ = stop_counted(&mut pointer, "pointer");
        let mut stamped: Vec<(Modality, SystemTime)> = events
            .iter()
            .map(|e| (e.modality, e.captured_at.wall_clock))
            .collect();
        stamped.sort_by_key(|(_, at)| *at);
        let kinds: Vec<Modality> = stamped.iter().map(|(kind, _)| *kind).collect();
        let wanted: Vec<Modality> = injected.iter().map(|(kind, _)| *kind).collect();
        let worst_ms = stamped
            .iter()
            .zip(&injected)
            .map(|((_, at), (_, sent))| {
                at.duration_since(*sent)
                    .or_else(|e| Ok::<Duration, ()>(e.duration()))
                    .unwrap_or_default()
                    .as_millis()
            })
            .max()
            .unwrap_or(0);
        say(
            "order",
            format!(
                "keyboard_start={} pointer_start={} sink_capacity=1 draining_stopped_ms=1500 \
                 sendinput_events_accepted={accepted} events={} order_matches={} \
                 worst_distance_from_input_ms={worst_ms}",
                show(&keyboard_start),
                show(&pointer_start),
                events.len(),
                kinds == wanted,
            ),
        );
        assert!(keyboard_start.is_ok(), "the keyboard source must start");
        assert!(pointer_start.is_ok(), "the pointer source must start");
        assert_eq!(
            accepted,
            3 * ROUNDS as u32,
            "INCONCLUSIVE: SendInput did not insert every key and wheel event"
        );
        assert_eq!(
            kinds, wanted,
            "the capture times do not follow the input order"
        );
        assert!(
            worst_ms <= 250,
            "a capture time is {worst_ms} ms away from its input"
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
        let accepted = press_keys(3);
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
        let accepted_before = press_keys(1);
        let before = count(
            &events.drain_for(Duration::from_millis(700)),
            Modality::Keystroke,
        );
        drop(keyboard);
        std::thread::sleep(Duration::from_millis(300));
        let accepted_after = press_keys(3);
        let after = count(
            &events.drain_for(Duration::from_secs(1)),
            Modality::Keystroke,
        );
        let mut next = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let next_start = next.begin(SourceContext::new(sink.clone()));
        gate_reads_clear("dropped", Some(&fields), &next);
        let accepted_next = press_keys(1);
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
        let held = last_app.lock().expect("the last-app cell");
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
        let held = last_app.lock().expect("the last-app cell");
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
        let (sink, events) = capture(256);
        let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
        let first = screen.begin(SourceContext::new(sink.clone()));
        let first_frame = events.first_within(FIRST_FRAME_WAIT).is_some();
        let first_stop = stop_counted(&mut screen, "screen");
        let _ = events.drain_for(Duration::from_millis(500));
        let second = screen.begin(SourceContext::new(sink.clone()));
        let second_frame = events.first_within(FIRST_FRAME_WAIT).is_some();
        let second_stop = stop_counted(&mut screen, "screen");
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
        let last_app = Arc::new(Mutex::new(None));
        let mut started = 0;
        let mut stopped = 0;
        let mut frames = 0;
        for _ in 0..CYCLES {
            let (sink, events) = capture(256);
            let mut screen = ScreenSource::new(Arc::clone(&last_app));
            if screen.begin(SourceContext::new(sink.clone())).is_ok() {
                started += 1;
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
                 idle_3s_after=true"
            ),
        );
        assert_eq!(started, CYCLES, "every cycle must start");
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
        let session = |label: &str| {
            let (sink, events) = capture(256);
            let mut screen = ScreenSource::new(Arc::new(Mutex::new(None)));
            let start = screen.begin(SourceContext::new(sink.clone()));
            let mut frames = 0;
            let began = Instant::now();
            while began.elapsed() < Duration::from_secs(3) {
                pump_thread(Duration::from_millis(50));
                frames += screen_frames_within(&events, Duration::from_millis(50));
            }
            let stop = stop_counted(&mut screen, "screen");
            say(
                "c8b",
                format!(
                    "capture={label} start={} frames={frames} stop={}",
                    show(&start),
                    show(&stop)
                ),
            );
            start.is_ok() && stop.is_ok() && frames > 0
        };
        let first = session("first");
        let excluded = exclude_from_capture(popup.hwnd);
        pump_thread(Duration::from_millis(500));
        let second = session("second");
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
        assert!(
            first && second,
            "each session must start, deliver a frame and stop"
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
        press_keys(5);
        let before = events.drain_for(Duration::from_millis(700));
        let still_child_foreground = foreground_pid() == child_pid;

        let mut between = Vec::new();
        let waited = Instant::now();
        while !between
            .iter()
            .any(|e: &CaptureStreamEvent| e.modality == Modality::ForegroundApp)
            && waited.elapsed() < Duration::from_secs(15)
        {
            between.extend(events.drain_for(Duration::from_millis(100)));
        }
        press_keys(5);
        let after = events.drain_for(Duration::from_millis(700));
        let exit = child.wait();
        let _ = stop_counted(&mut foreground, "foreground");
        let _ = stop_counted(&mut keyboard, "keyboard");
        child.print_output();

        let keys = |events: &[CaptureStreamEvent]| count(events, Modality::Keystroke);
        let empty_app = |events: &[CaptureStreamEvent]| {
            events
                .iter()
                .filter(|e| e.modality == Modality::Keystroke && e.foreground_app.is_empty())
                .count()
        };
        gate_trail("x3");
        let child_app = |events: &[CaptureStreamEvent]| {
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
        let mut title = WindowTitleSource::new();
        let start = title.begin(SourceContext::new(sink.clone()));
        let mut child = spawn_child("scope", 0);
        let mut seen = Vec::new();
        let exit = loop {
            seen.extend(events.drain_for(Duration::from_millis(100)));
            if let Some(status) = child.try_wait() {
                break status;
            }
        };
        seen.extend(events.drain_for(Duration::from_secs(1)));
        let _ = stop_counted(&mut title, "window_title");
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
        let mut title = WindowTitleSource::new();
        let start = title.begin(SourceContext::new(sink.clone()));
        let mut child = spawn_child("cycle", 4);
        let child_pid = child.id();
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
            accepted += send(&[key_input(VK_F15, false)]);
            std::thread::sleep(Duration::from_millis(30));
        }
        accepted += send(&[key_input(VK_F15, true)]);
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

fn unix_millis_of(event: &CaptureStreamEvent) -> u128 {
    unix_millis(event.captured_at.wall_clock)
}

fn keystrokes_inside(
    events: &[CaptureStreamEvent],
    intervals: &[(u128, u128)],
    margin: u128,
) -> usize {
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
        let keys = |events: &[CaptureStreamEvent]| count(events, Modality::Keystroke);

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
}

const CHROME: Browser = Browser {
    name: "chrome",
    paths: &[
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ],
    chromium: true,
};

const EDGE: Browser = Browser {
    name: "edge",
    paths: &[
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ],
    chromium: true,
};

const FIREFOX: Browser = Browser {
    name: "firefox",
    paths: &[
        r"C:\Program Files\Mozilla Firefox\firefox.exe",
        r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe",
    ],
    chromium: false,
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
        let mut buf = [0u16; 512];
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
    let mut buf = [0u16; 512];
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
) -> (
    usize,
    Vec<CaptureStreamEvent>,
    Option<PageCounts>,
    Vec<u128>,
) {
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

fn password_key_events(seen: &[CaptureStreamEvent], sent: &[u128], seq: &str) -> usize {
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
        let (m, _, m_page, _) = browser_phase(test, run, &keyboard, &events, 'M', || letters(5));
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
            for _ in 0..20 {
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
        for _ in 0..10 {
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
                 phase_M_masked_keys={} phase_M_events={m} phase_B_password_keys={} phase_B_events={b} \
                 keys_withheld={withheld_b} phase_C_keys={} phase_C_events={c} phase_C_password_key_events={c_password} \
                 phase_D_keys={} phase_D_events={d} phase_E_password_keys={} phase_E_events={e} \
                 phase_F_password_keys={f_password} phase_F_events={f}",
                show(&start),
                browser.name,
                if unknown_class { "unknown" } else { "read" },
                page(&a_page).0,
                page(&m_page).2,
                page(&b_page).1,
                c_seq.len(),
                page(&d_page).0 + page(&d_page).1,
                page(&e_page).1,
            ),
        );
        assert!(start.is_ok(), "the keyboard source must start");
        assert!(
            page(&a_page).0 == 5 && page(&b_page).1 == 5 && page(&m_page).2 == 5,
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
                a + m + c + e,
                0,
                "a Firefox focus must read Unknown and give no keystroke event"
            );
        } else {
            assert_eq!(a, 5, "phase A: every key of the text input");
            assert_eq!(
                m, 5,
                "phase M: the CSS-masked text input is recorded, as the consent text says"
            );
            assert!(c >= 1, "phase C: no key of the text input was recorded");
            assert_eq!(
                e_letters, 0,
                "phase E: a key after Tab into the password input was recorded"
            );
        }
    });
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

mod known_defect {
    use super::*;

    #[test]
    #[ignore = "known defect: W3-5 A2, a focus move that the page makes itself after holding its main thread"]
    fn browser_self_move_after_a_held_main_thread_in_chrome() {
        let recorded = browser::self_move_phases("browser-self-chrome", "chrome");
        defect!(
            "W3-5-A2",
            recorded == 0,
            format!("TF browser-self-chrome password_letters_recorded={recorded}")
        );
    }

    #[test]
    #[ignore = "known defect: W3-5 A2, a focus move that the page makes itself after holding its main thread"]
    fn browser_self_move_after_a_held_main_thread_in_edge() {
        let recorded = browser::self_move_phases("browser-self-edge", "edge");
        defect!(
            "W3-5-A2",
            recorded == 0,
            format!("TF browser-self-edge password_letters_recorded={recorded}")
        );
    }

    #[test]
    #[ignore = "known defect: W3-7 Q2 (i), a type swap on a page that holds its main thread"]
    fn browser_swaps_on_a_held_main_thread_in_chrome() {
        let recorded = browser::held_swap_phases("browser-held-swap-chrome", "chrome");
        defect!(
            "W3-7-Q2i",
            recorded == 0,
            format!("TF browser-held-swap-chrome password_letters_recorded={recorded}")
        );
    }

    #[test]
    #[ignore = "known defect: W3-7 Q2 (i), a type swap on a page that holds its main thread"]
    fn browser_swaps_on_a_held_main_thread_in_edge() {
        let recorded = browser::held_swap_phases("browser-held-swap-edge", "edge");
        defect!(
            "W3-7-Q2i",
            recorded == 0,
            format!("TF browser-held-swap-edge password_letters_recorded={recorded}")
        );
    }

    #[test]
    #[ignore = "known defect: W3-5 A2, a focus move that no latch sees"]
    fn stall_no_focus_key_and_the_program_moves_the_focus_itself() {
        with_deadline("stall-self-focus", Duration::from_secs(330), || {
            let (recorded, late) = stall::no_focus_key_and_the_program_moves_the_focus_itself();
            assert_eq!(late, 0, "the source's late value moved");
            defect!(
                "W3-5-A2",
                recorded == 0,
                format!("TF stall-self-focus letters_recorded_that_landed_in_the_password_field={recorded}")
            );
        });
    }

    use halvo_capture::windows::apps::wait_for_change;

    fn arm_watcher() -> mpsc::Receiver<Result<(), String>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(wait_for_change().map_err(|e| e.to_string()));
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
    #[ignore = "known defect: reads and writes the Windows registry"]
    fn c5_watch_armed_while_the_per_user_key_is_absent() {
        with_deadline("c5-watch", Duration::from_secs(90), || {
            use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
            use winreg::RegKey;

            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let key_existed = hkcu.open_subkey_with_flags(UNINSTALL, KEY_READ).is_ok();
            say(
                "c5-watch",
                format!("hkcu_uninstall_key_existed={key_existed}"),
            );
            assert!(
                !key_existed,
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
            defect!(
                "RR-windows-12",
                first_woke,
                format!("TF c5-watch woke_within_5s_of_hkcu_install={first_woke}")
            );
        });
    }

    #[test]
    #[ignore = "known defect: reads the Windows registry"]
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
                .apps
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
            defect!(
                "RR-windows-12",
                terminal_in_inventory,
                format!("TF x16 inventory_has_an_entry_naming_terminal={terminal_in_inventory}")
            );
        });
    }
}
