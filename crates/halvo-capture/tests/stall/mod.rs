use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_capture::event::Modality;
use halvo_capture::secure_input::windows::PasswordGate;
use halvo_capture::sink::SinkHandle;
use halvo_capture::source::{CaptureSource, SourceContext};
use halvo_capture::windows::input::seam::{self, KeyClass, Verdict};
use halvo_capture::windows::input::RawInputSource;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT, MOUSE_EVENT_FLAGS, VK_A, VK_MENU, VK_P,
    VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowRect, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsDialogMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW,
    PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetWindowTextW, ShowWindow,
    TranslateMessage, UnregisterClassW, WindowFromPoint, ES_PASSWORD, MSG, MWMO_INPUTAVAILABLE,
    PEEK_MESSAGE_REMOVE_TYPE, PM_NOREMOVE, PM_QS_SENDMESSAGE, QS_SENDMESSAGE, SC_KEYMENU,
    SM_CXSCREEN, SM_CYSCREEN, SW_MINIMIZE, SW_RESTORE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_CHAR, WM_KEYDOWN, WM_SYSCOMMAND, WM_SYSKEYDOWN, WNDCLASSW, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};

use super::{
    capture, focus_on, foreground_window, key_input, pump_thread, say, send, source_built_by_test,
    start_report, stop_counted, with_deadline, Events, Field,
};

mod kinds;

use kinds::{
    phase, ControlRead, Controls, Kind, Landing, Lead, Outcome, Phase, PhaseRun, Step, Totals,
    Wait, CONTROL_LETTERS, NATIVE_PHASES,
};

const FIXTURE_ENV: &str = "HALVO_VERIFY_STALL_FIXTURE";
const FIXTURE_TEST: &str = "stall::helper_stall_fixture";

const WM_STALL: u32 = WM_APP + 7;
const FIRST_PASSWORD_ENV: &str = "HALVO_VERIFY_STALL_FIRST_PASSWORD";

const CMD_RESET: usize = 1;
const CMD_STATE: usize = 2;
const CMD_ROUND: usize = 3;
const CMD_QUIT: usize = 4;
const CMD_FOCUS: usize = 5;
const CMD_REVEAL: usize = 6;
const CMD_WANDER: usize = 7;
const CMD_AIM: usize = 8;
const CMD_MASK: usize = 9;
const CMD_NOTIFY_FOCUS: usize = 10;
const ARG_SELF_FOCUS: usize = 0x1_0000;
const ARG_AIM: usize = 0x2_0000;
const ARG_SERVE_SENT: usize = 0x4_0000;
const ARG_STALL_AFTER_A_TAB: usize = 0x8_0000;
const ARG_OTHER_FRONT: usize = 0x10_0000;
const ARG_TWO_STEP: usize = 0x20_0000;
const ARG_SECOND: usize = 0x1;
const ARG_START_IN_PASSWORD: usize = 0x2;
const ARG_SILENT_PASSWORD: usize = 0x4;
const TWO_STEP_MS: u64 = 200;
const ARG_LATE_MOVE: usize = 0x40_0000;
const ARG_OWN_CHANGE: usize = 0x80_0000;
const LATE_MOVE_WORK_MS: u64 = 200;

const LETTER_STEP: Duration = Duration::from_millis(60);
const FORM_ENV: &str = "HALVO_WF_FORM";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Form {
    Short,
    Long,
}

impl Form {
    fn word(self) -> &'static str {
        match self {
            Form::Short => "short",
            Form::Long => "long",
        }
    }
}

pub(super) fn form() -> Form {
    match std::env::var(FORM_ENV).as_deref() {
        Ok("short") => Form::Short,
        Ok("long") => Form::Long,
        other => panic!("{FORM_ENV} is short or long, not {other:?}"),
    }
}

pub(super) fn short_form() -> bool {
    form() == Form::Short
}

pub(super) fn rounds_for(short: u32, long: u32) -> u32 {
    if short_form() {
        short
    } else {
        long
    }
}

fn rounds_at(form: Form, stall_ms: u64) -> u32 {
    match (form, stall_ms) {
        (Form::Short, _) => 1,
        (Form::Long, 2_000..) => 5,
        (Form::Long, _) => 20,
    }
}

fn flag(value: bool) -> u8 {
    u8::from(value)
}

unsafe extern "system" fn fixture_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_SYSCOMMAND && (wparam.0 & 0xFFF0) == SC_KEYMENU as usize {
        return LRESULT(0);
    }
    // SAFETY: the arguments are the ones that the system passed to this
    // window procedure.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

struct FixtureWindow {
    hwnd: HWND,
    class: HSTRING,
    hinstance: HINSTANCE,
}

impl FixtureWindow {
    fn create() -> FixtureWindow {
        FixtureWindow::create_named("HalvoVerifyStallFixture", "halvo-verify-stall-fixture")
    }

    fn create_named(class: &str, title: &str) -> FixtureWindow {
        let class = HSTRING::from(class);
        let title = HSTRING::from(title);
        // SAFETY: a null module name asks for the handle of this executable.
        let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .expect("GetModuleHandleW")
            .into();
        let wnd_class = WNDCLASSW {
            lpfnWndProc: Some(fixture_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `wnd_class` points at `class`, which the returned value
        // holds until its Drop unregisters the class.
        let registered = unsafe { RegisterClassW(&wnd_class) };
        assert!(registered != 0, "INCONCLUSIVE: RegisterClassW");
        // SAFETY: the class name and title point at live HSTRINGs for the
        // duration of the call, and the class was registered above.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                120,
                120,
                360,
                240,
                None,
                None,
                Some(hinstance),
                None,
            )
        }
        .expect("INCONCLUSIVE: CreateWindowExW");
        FixtureWindow {
            hwnd,
            class,
            hinstance,
        }
    }

    fn edit(&self, style: WINDOW_STYLE, y: i32) -> HWND {
        let class = HSTRING::from("EDIT");
        // SAFETY: the class name points at a live HSTRING for the call, and
        // the parent is this window, which the calling thread owns.
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
                Some(self.hinstance),
                None,
            )
        }
        .expect("INCONCLUSIVE: the edit control")
    }

    fn is_foreground(&self) -> bool {
        foreground_window() == self.hwnd
    }

    fn activate(&self) -> Option<Front> {
        // SAFETY: the three calls take this thread's window handle by value.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetForegroundWindow(self.hwnd);
        }
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return Some(Front::SetForeground);
        }
        send(&[key_input(VK_MENU, false)]);
        // SAFETY: SetForegroundWindow takes a window handle by value.
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        send(&[key_input(VK_MENU, true)]);
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return Some(Front::AltKey);
        }
        let mut pid = 0u32;
        // SAFETY: `pid` is a live u32 that the call writes into.
        let current_thread =
            unsafe { GetWindowThreadProcessId(foreground_window(), Some(&mut pid)) };
        // SAFETY: GetCurrentThreadId takes no argument.
        let this_thread = unsafe { GetCurrentThreadId() };
        if current_thread != 0 && current_thread != this_thread {
            // SAFETY: the input of the two threads is joined only around the
            // two window calls and detached again before the block ends.
            unsafe {
                let _ = AttachThreadInput(this_thread, current_thread, true);
                let _ = BringWindowToTop(self.hwnd);
                let _ = SetForegroundWindow(self.hwnd);
                let _ = AttachThreadInput(this_thread, current_thread, false);
            }
        }
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return Some(Front::JoinedInput);
        }
        // SAFETY: ShowWindow takes this thread's window handle by value.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
        }
        pump_thread(Duration::from_millis(200));
        self.is_foreground().then_some(Front::MinimizeRestore)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Front {
    SetForeground = 1,
    AltKey = 2,
    JoinedInput = 3,
    MinimizeRestore = 4,
}

impl Front {
    fn code(front: Option<Front>) -> i64 {
        front.map_or(0, |front| front as i64)
    }

    fn of_code(code: i64) -> Option<Front> {
        [
            Front::SetForeground,
            Front::AltKey,
            Front::JoinedInput,
            Front::MinimizeRestore,
        ]
        .into_iter()
        .find(|front| *front as i64 == code)
    }

    fn name(front: Option<Front>) -> &'static str {
        match front {
            Some(Front::SetForeground) => "set-foreground",
            Some(Front::AltKey) => "alt-key-unlock",
            Some(Front::JoinedInput) => "attach-thread-input",
            Some(Front::MinimizeRestore) => "minimize-restore",
            None => "none",
        }
    }
}

impl Drop for FixtureWindow {
    fn drop(&mut self) {
        // SAFETY: the window belongs to this value and is destroyed once, by
        // the thread that created it; then its class is unregistered.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
        pump_thread(Duration::from_millis(50));
        // SAFETY: this value registered the class and unregisters it once.
        unsafe {
            let _ = UnregisterClassW(PCWSTR(self.class.as_ptr()), Some(self.hinstance));
        }
    }
}

fn text_len(hwnd: HWND) -> i32 {
    // SAFETY: GetWindowTextLengthW reads the length of a window's text.
    unsafe { GetWindowTextLengthW(hwnd) }
}

fn clear_text(hwnd: HWND) {
    let empty = HSTRING::new();
    // SAFETY: `empty` is a live HSTRING for the duration of the call.
    unsafe {
        let _ = SetWindowTextW(hwnd, &empty);
    }
}

fn mouse_input(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn aim_at(target: HWND) -> bool {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a live RECT that the call fills.
    if unsafe { GetWindowRect(target, &mut rect) }.is_err() {
        return false;
    }
    let (x, y) = ((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2);
    // SAFETY: GetSystemMetrics reads a system value and takes no pointer.
    let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    if cx < 2 || cy < 2 || x < 0 || y < 0 || x >= cx || y >= cy {
        return false;
    }
    let to = |v: i32, span: i32| (i64::from(v) * 65_535 / i64::from(span - 1)) as i32;
    send(&[mouse_input(
        to(x, cx),
        to(y, cy),
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
    )]);
    std::thread::sleep(Duration::from_millis(30));
    let mut at = POINT::default();
    // SAFETY: `at` is a live POINT that the call fills; WindowFromPoint takes
    // the point by value.
    unsafe { GetCursorPos(&mut at).is_ok() && WindowFromPoint(at) == target }
}

fn post(window: usize, code: usize, arg: usize) {
    // SAFETY: PostMessageW takes a window handle and plain values; a window
    // that is gone makes the call fail and changes nothing.
    unsafe {
        let _ = PostMessageW(
            Some(HWND(window as *mut core::ffi::c_void)),
            WM_STALL,
            WPARAM(code),
            LPARAM(arg as isize),
        );
    }
}

fn stall(stall_ms: u64, wait: Wait) {
    if stall_ms == 0 {
        return;
    }
    if wait == Wait::Sleeps {
        std::thread::sleep(Duration::from_millis(stall_ms));
        return;
    }
    let end = Instant::now() + Duration::from_millis(stall_ms);
    let mut msg = MSG::default();
    loop {
        let now = Instant::now();
        if now >= end {
            break;
        }
        let left = u32::try_from((end - now).as_millis()).unwrap_or(1).max(1);
        // SAFETY: the wait takes no handle; `msg` is a live MSG, and the peek
        // serves sent messages alone and removes nothing from the queue.
        unsafe {
            let _ = MsgWaitForMultipleObjectsEx(None, left, QS_SENDMESSAGE, MWMO_INPUTAVAILABLE);
            let _ = PeekMessageW(
                &mut msg,
                None,
                0,
                0,
                PEEK_MESSAGE_REMOVE_TYPE(PM_NOREMOVE.0 | PM_QS_SENDMESSAGE.0),
            );
        }
    }
}

struct Wander {
    began: Instant,
    span: Duration,
    next: Instant,
    seed: u64,
    intervals: Vec<(u128, u128)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixtureField {
    Plain = 0,
    Password = 1,
    Second = 2,
}

impl FixtureField {
    fn wire(self) -> usize {
        self as usize
    }

    fn of_wire(wire: usize) -> FixtureField {
        match wire {
            1 => FixtureField::Password,
            2 => FixtureField::Second,
            _ => FixtureField::Plain,
        }
    }
}
const WANDER_TIMER: usize = 1;

fn fixture_main() {
    use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SendMessageW, SetTimer, WM_TIMER};
    const EM_SETPASSWORDCHAR: u32 = 0x00CC;
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(900));
        std::process::exit(3);
    });
    let window = FixtureWindow::create();
    let plain = window.edit(WS_TABSTOP, 10);
    let second = window.edit(WS_TABSTOP, 78);
    let password = window.edit(WS_TABSTOP | WINDOW_STYLE(ES_PASSWORD as u32), 44);
    const EM_GETPASSWORDCHAR: u32 = 0x00D2;
    // SAFETY: SendMessageW passes plain values to this thread's edit control.
    let mask = unsafe { SendMessageW(password, EM_GETPASSWORDCHAR, None, None) }.0 as usize;
    let masked = |hwnd: HWND| {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongW, GWL_STYLE};
        // SAFETY: GetWindowLongW reads the style of this thread's edit control.
        let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
        style & ES_PASSWORD as u32 != 0
    };
    let enable_second = |on: bool| {
        // SAFETY: EnableWindow takes a handle of this thread's edit control.
        unsafe {
            let _ = EnableWindow(second, on);
        }
    };
    enable_second(false);
    let field = |wire: usize| match FixtureField::of_wire(wire) {
        FixtureField::Password => password,
        FixtureField::Second => second,
        FixtureField::Plain => plain,
    };
    pump_thread(Duration::from_millis(300));
    let front = window.activate();
    let first = if std::env::var_os(FIRST_PASSWORD_ENV).is_some() {
        password
    } else {
        plain
    };
    focus_on(first);
    pump_thread(Duration::from_millis(300));
    let target = window.hwnd.0 as usize;
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else {
                break;
            };
            let mut parts = line.split_whitespace();
            let mut next = || {
                parts
                    .next()
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0)
            };
            let (code, arg) = (next(), next());
            post(target, code, arg);
            if code == CMD_QUIT {
                return;
            }
        }
        post(target, CMD_QUIT, 0);
    });
    println!(
        "STALLC ready fg={} method={}",
        flag(front.is_some()),
        Front::code(front)
    );
    let mut letters = [0u32; 4];
    let mut tabs = [0u32; 4];
    let mut wander: Option<Wander> = None;
    let mut silent_password = false;
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that GetMessageW fills.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        if msg.message == WM_TIMER && msg.hwnd == window.hwnd && msg.wParam.0 == WANDER_TIMER {
            if let Some(Wander {
                began,
                span,
                next,
                seed,
                intervals,
            }) = wander.as_mut()
            {
                if began.elapsed() >= *span {
                    if let Some(open) = intervals.last_mut().filter(|(_, end)| *end == 0) {
                        open.1 = unix_ms();
                    }
                    focus_on(plain);
                    // SAFETY: the timer belongs to this thread's window.
                    unsafe {
                        let _ = KillTimer(Some(window.hwnd), WANDER_TIMER);
                    }
                    for (start, end) in intervals.iter() {
                        println!("STALLC interval start={start} end={end}");
                    }
                    println!("STALLC wandered count={}", intervals.len());
                    wander = None;
                } else if Instant::now() >= *next {
                    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                    let to_password = !intervals.last().is_some_and(|(_, end)| *end == 0);
                    let now = unix_ms();
                    if to_password {
                        focus_on(password);
                        intervals.push((now, 0));
                    } else {
                        focus_on(plain);
                        if let Some(open) = intervals.last_mut() {
                            open.1 = now;
                        }
                    }
                    *next = Instant::now() + Duration::from_millis(150 + (*seed >> 33) % 450);
                }
            }
            continue;
        }
        if msg.message == WM_STALL && msg.hwnd == window.hwnd {
            // SAFETY: GetFocus takes no argument and returns a handle value.
            let focus = unsafe { GetFocus() };
            let arg = msg.lParam.0 as usize;
            match msg.wParam.0 {
                CMD_RESET => {
                    let reactivated = !window.is_foreground();
                    if reactivated {
                        let _ = window.activate();
                    }
                    // SAFETY: SendMessageW passes a plain value to this
                    // thread's edit controls.
                    unsafe {
                        let _ = SendMessageW(plain, EM_SETPASSWORDCHAR, Some(WPARAM(0)), None);
                        let _ =
                            SendMessageW(password, EM_SETPASSWORDCHAR, Some(WPARAM(mask)), None);
                    }
                    clear_text(plain);
                    clear_text(second);
                    clear_text(password);
                    enable_second(arg & ARG_SECOND != 0);
                    let start = if arg & ARG_START_IN_PASSWORD != 0 {
                        password
                    } else {
                        plain
                    };
                    let focused = focus_on(start);
                    letters = [0; 4];
                    silent_password = arg & ARG_SILENT_PASSWORD != 0;
                    tabs = [0; 4];
                    println!(
                        "STALLC reset fg={} focused={} reactivated={}",
                        flag(window.is_foreground()),
                        flag(focused),
                        flag(reactivated)
                    );
                }
                CMD_STATE => {
                    println!(
                        "STALLC state fg={} focus_plain={} focus_password={} plain_len={} \
                         password_len={} second_len={} letters_plain={} letters_password={} \
                         letters_second={} letters_other={} tabs_plain={} tabs_password={} \
                         tabs_second={} tabs_other={} password_masked={}",
                        flag(window.is_foreground()),
                        flag(focus == plain),
                        flag(focus == password),
                        text_len(plain),
                        text_len(password),
                        text_len(second),
                        letters[0],
                        letters[1],
                        letters[2],
                        letters[3],
                        tabs[0],
                        tabs[1],
                        tabs[2],
                        tabs[3],
                        flag(masked(password))
                    );
                }
                CMD_FOCUS => {
                    let focused = focus_on(field(arg));
                    println!("STALLC focused ok={}", flag(focused));
                }
                CMD_REVEAL => {
                    // SAFETY: SendMessageW passes a plain value to this
                    // thread's edit control.
                    unsafe {
                        let _ = SendMessageW(password, EM_SETPASSWORDCHAR, Some(WPARAM(0)), None);
                    }
                    println!("STALLC revealed ok=1");
                }
                CMD_MASK => {
                    // SAFETY: SendMessageW passes a plain value to this
                    // thread's edit control.
                    unsafe {
                        let _ =
                            SendMessageW(field(arg), EM_SETPASSWORDCHAR, Some(WPARAM(0x2A)), None);
                    }
                    println!("STALLC masked ok=1");
                }
                CMD_NOTIFY_FOCUS => {
                    use windows::Win32::UI::Accessibility::NotifyWinEvent;
                    use windows::Win32::UI::WindowsAndMessaging::{
                        CHILDID_SELF, EVENT_OBJECT_FOCUS, OBJID_CLIENT,
                    };
                    // SAFETY: NotifyWinEvent takes a window handle of this
                    // thread and plain values.
                    unsafe {
                        NotifyWinEvent(
                            EVENT_OBJECT_FOCUS,
                            field(arg),
                            OBJID_CLIENT.0,
                            CHILDID_SELF as i32,
                        );
                    }
                    println!("STALLC notified ok=1");
                }
                CMD_AIM => {
                    let aimed = aim_at(field(arg));
                    println!("STALLC aimed ok={}", flag(aimed));
                }
                CMD_WANDER => {
                    wander = Some(Wander {
                        began: Instant::now(),
                        span: Duration::from_millis(arg as u64),
                        next: Instant::now(),
                        seed: 7,
                        intervals: Vec::new(),
                    });
                    // SAFETY: the timer belongs to this thread's window and
                    // is killed when the wander ends.
                    unsafe {
                        let _ = SetTimer(Some(window.hwnd), WANDER_TIMER, 10, None);
                    }
                }
                CMD_ROUND => {
                    let stall_ms = (arg & 0xFFFF) as u64;
                    let aimed = arg & ARG_AIM == 0 || aim_at(password);
                    println!(
                        "STALLC ack fg={} focus_plain={} focus_password={} plain_len={} password_len={} aimed={}",
                        flag(window.is_foreground()),
                        flag(focus == plain),
                        flag(focus == password),
                        text_len(plain),
                        text_len(password),
                        flag(aimed)
                    );
                    if arg & ARG_STALL_AFTER_A_TAB != 0 {
                        dispatch_until_a_tab(window.hwnd, &mut tabs, [plain, password, second]);
                    }
                    let other =
                        (arg & ARG_OTHER_FRONT != 0).then(|| other_window_in_front(stall_ms));
                    if arg & ARG_TWO_STEP != 0 {
                        enable_second(true);
                        focus_on(second);
                        std::thread::sleep(Duration::from_millis(TWO_STEP_MS));
                        focus_on(password);
                    }
                    let wait = if arg & ARG_SERVE_SENT != 0 {
                        Wait::ServesSent
                    } else {
                        Wait::Sleeps
                    };
                    stall(stall_ms, wait);
                    let (plain_len, password_len) = (text_len(plain), text_len(password));
                    if arg & ARG_SELF_FOCUS != 0 {
                        focus_on(password);
                    }
                    if arg & ARG_LATE_MOVE != 0 {
                        if arg & ARG_OWN_CHANGE != 0 {
                            // SAFETY: SetWindowTextW takes this thread's edit
                            // control and a live HSTRING.
                            unsafe {
                                let _ = SetWindowTextW(plain, &HSTRING::from("x"));
                            }
                        }
                        stall(LATE_MOVE_WORK_MS, Wait::ServesSent);
                        focus_on(password);
                    }
                    if let Some(other) = other {
                        let _ = other.join();
                    }
                    println!("STALLC woke plain_len={plain_len} password_len={password_len}");
                }
                _ => {
                    // SAFETY: PostQuitMessage takes an exit code.
                    unsafe { PostQuitMessage(0) };
                }
            }
            continue;
        }
        if silent_password && msg.message == WM_CHAR && msg.hwnd == password {
            continue;
        }
        if msg.message == WM_SYSKEYDOWN && msg.wParam.0 == usize::from(VK_P.0) {
            focus_on(password);
            continue;
        }
        count_key(&msg, &mut letters, &mut tabs, [plain, password, second]);
        // SAFETY: `msg` is the live MSG that GetMessageW filled.
        unsafe {
            if IsDialogMessageW(window.hwnd, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    drop(window);
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn count_key(msg: &MSG, letters: &mut [u32; 4], tabs: &mut [u32; 4], fields: [HWND; 3]) {
    if msg.message != WM_KEYDOWN {
        return;
    }
    let to = fields.iter().position(|f| *f == msg.hwnd).unwrap_or(3);
    let key = msg.wParam.0 as u16;
    if key == VK_A.0 {
        letters[to] += 1;
    } else if key == VK_TAB.0 {
        tabs[to] += 1;
    }
}

fn dispatch_until_a_tab(dialog: HWND, tabs: &mut [u32; 4], fields: [HWND; 3]) {
    let end = Instant::now() + Duration::from_secs(1);
    let mut letters = [0u32; 4];
    let mut msg = MSG::default();
    while Instant::now() < end {
        // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same
        // value goes to IsDialogMessageW, TranslateMessage and
        // DispatchMessageW.
        let got = unsafe {
            PeekMessageW(
                &mut msg,
                None,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
            )
        };
        if !got.as_bool() {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        let tab_down = msg.message == WM_KEYDOWN && msg.wParam.0 as u16 == VK_TAB.0;
        count_key(&msg, &mut letters, tabs, fields);
        // SAFETY: as above.
        unsafe {
            if !IsDialogMessageW(dialog, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if tab_down {
            return;
        }
    }
}

fn other_window_in_front(stall_ms: u64) -> std::thread::JoinHandle<()> {
    let back = foreground_window().0 as usize;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(stall_ms / 2));
        let other =
            FixtureWindow::create_named("HalvoVerifyStallOther", "halvo-verify-stall-other");
        let _ = other.activate();
        pump_thread(Duration::from_millis(stall_ms / 2 + 300));
        drop(other);
        // SAFETY: SetForegroundWindow takes a handle value.
        unsafe {
            let _ = SetForegroundWindow(HWND(back as *mut core::ffi::c_void));
        }
    })
}

#[test]
#[ignore = "helper: the stall fixture's process; does nothing unless a stall test starts it"]
fn helper_stall_fixture() {
    if std::env::var_os(FIXTURE_ENV).is_none() {
        return;
    }
    fixture_main();
}

type Fields = HashMap<String, i64>;

fn parse(line: &str) -> Option<(String, Fields)> {
    // With --nocapture the first line of a test shares the line of its name.
    let rest = &line[line.find("STALLC ")? + 7..];
    let mut parts = rest.split_whitespace();
    let kind = parts.next()?.to_string();
    let fields = parts
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((key.to_string(), value.parse::<i64>().ok()?))
        })
        .collect();
    Some((kind, fields))
}

fn get(fields: &Fields, key: &str) -> i64 {
    fields.get(key).copied().unwrap_or(-1)
}

pub(super) struct Fixture {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<(String, Fields)>,
    foreground: bool,
    front: Option<Front>,
}

impl Fixture {
    fn open(test: &str, first_password: bool) -> Fixture {
        let exe = std::env::current_exe().expect("the test executable");
        let mut command = Command::new(exe);
        command
            .args([
                "--ignored",
                "--exact",
                FIXTURE_TEST,
                "--nocapture",
                "--test-threads=1",
            ])
            .env(FIXTURE_ENV, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if first_password {
            command.env(FIRST_PASSWORD_ENV, "1");
        }
        let mut child = command
            .spawn()
            .expect("INCONCLUSIVE: the fixture process did not start");
        let stdin = child.stdin.take().expect("the fixture's stdin");
        let stdout = child.stdout.take().expect("the fixture's stdout");
        let (tx, lines) = mpsc::channel();
        std::thread::Builder::new()
            .name("tf-stall-fixture-lines".to_string())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    if let Some(parsed) = parse(&line) {
                        if tx.send(parsed).is_err() {
                            break;
                        }
                    }
                }
            })
            .expect("spawn the fixture reader");
        let mut fixture = Fixture {
            child,
            stdin,
            lines,
            foreground: false,
            front: None,
        };
        let ready = fixture.expect("ready", Duration::from_secs(40));
        fixture.foreground = get(&ready, "fg") == 1;
        fixture.front = Front::of_code(get(&ready, "method"));
        say(
            test,
            format!(
                "fixture_process_started=true fixture_window_foreground={} method={}",
                flag(fixture.foreground),
                Front::code(fixture.front)
            ),
        );
        fixture
    }

    fn tell(&mut self, code: usize, arg: usize) {
        let _ = writeln!(self.stdin, "{code} {arg}");
        let _ = self.stdin.flush();
    }

    fn expect(&self, kind: &str, limit: Duration) -> Fields {
        let deadline = Instant::now() + limit;
        loop {
            let now = Instant::now();
            if now >= deadline {
                panic!("INCONCLUSIVE: the fixture process did not answer {kind}");
            }
            match self.lines.recv_timeout(deadline - now) {
                Ok((got, fields)) if got == kind => return fields,
                Ok(_) => {}
                Err(_) => panic!("INCONCLUSIVE: the fixture process did not answer {kind}"),
            }
        }
    }

    fn ask(&mut self, code: usize, arg: usize, kind: &str) -> Fields {
        self.tell(code, arg);
        self.expect(kind, Duration::from_secs(10))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.tell(CMD_QUIT, 0);
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) struct FieldWindow {
    fixture: Mutex<Fixture>,
    pub(super) foreground: bool,
    pub(super) method: &'static str,
}

impl FieldWindow {
    pub(super) fn open(test: &str, first: Field) -> FieldWindow {
        let fixture = Fixture::open(test, first == Field::Password);
        let method = Front::name(fixture.front);
        say(
            test,
            format!(
                "field_window_foreground={} method={method} first_focus={first:?} process_of_its_own=true",
                fixture.foreground
            ),
        );
        FieldWindow {
            foreground: fixture.foreground,
            method,
            fixture: Mutex::new(fixture),
        }
    }

    fn fixture(&self) -> std::sync::MutexGuard<'_, Fixture> {
        self.fixture
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn focus(&self, field: Field) -> bool {
        let index = match field {
            Field::Plain => FixtureField::Plain,
            Field::Password => FixtureField::Password,
        };
        let focused = get(
            &self.fixture().ask(CMD_FOCUS, index.wire(), "focused"),
            "ok",
        ) == 1;
        std::thread::sleep(Duration::from_millis(50));
        focused
    }

    pub(super) fn click(&self, field: Field) -> bool {
        let index = match field {
            Field::Plain => FixtureField::Plain,
            Field::Password => FixtureField::Password,
        };
        let aimed = get(&self.fixture().ask(CMD_AIM, index.wire(), "aimed"), "ok") == 1;
        if aimed {
            send(&[
                mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN),
                mouse_input(0, 0, MOUSEEVENTF_LEFTUP),
            ]);
        }
        std::thread::sleep(Duration::from_millis(50));
        aimed
    }

    pub(super) fn reveal(&self) -> bool {
        get(&self.fixture().ask(CMD_REVEAL, 0, "revealed"), "ok") == 1
    }

    pub(super) fn password_has_focus(&self) -> bool {
        get(&self.fixture().ask(CMD_STATE, 0, "state"), "focus_password") == 1
    }

    pub(super) fn text_len(&self, field: Field) -> i32 {
        let state = self.fixture().ask(CMD_STATE, 0, "state");
        let key = match field {
            Field::Plain => "plain_len",
            Field::Password => "password_len",
        };
        i32::try_from(get(&state, key)).unwrap_or(-1)
    }

    pub(super) fn start_wander(&self, span: Duration) {
        let span = usize::try_from(span.as_millis()).unwrap_or(usize::MAX);
        self.fixture().tell(CMD_WANDER, span);
    }

    pub(super) fn wander_intervals(&self, span: Duration) -> Vec<(u128, u128)> {
        let fixture = self.fixture();
        let deadline = Instant::now() + span + Duration::from_secs(5);
        let mut intervals = Vec::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                panic!("INCONCLUSIVE: the field window did not report its focus moves");
            }
            match fixture.lines.recv_timeout(deadline - now) {
                Ok((kind, fields)) if kind == "interval" => {
                    let at = |key| u128::try_from(get(&fields, key)).unwrap_or(0);
                    intervals.push((at("start"), at("end")));
                }
                Ok((kind, _)) if kind == "wandered" => return intervals,
                Ok(_) => {}
                Err(_) => panic!("INCONCLUSIVE: the field window did not report its focus moves"),
            }
        }
    }
}

const CLICK_ROUNDS_WITHOUT_WAIT: u32 = 5;

#[derive(Debug, Default, Clone, Copy)]
struct Decisions {
    letters_recorded: u32,
    tabs_recorded: u32,
    other_recorded: u32,
    letters_late: u32,
    letters_withheld: u32,
    tabs_withheld: u32,
    letters_withheld_password: u32,
    tabs_withheld_password: u32,
    lost_at_stop: u32,
    first_letter_rule: Option<seam::Rule>,
    first_tab_rule: Option<seam::Rule>,
}

const RULES: [seam::Rule; 11] = [
    seam::Rule::PasswordEdit,
    seam::Rule::NotClearedInTime,
    seam::Rule::AnotherThreadInFront,
    seam::Rule::StateAtTheKey,
    seam::Rule::MarkAfterTheKey,
    seam::Rule::ReadAfterTheHold,
    seam::Rule::NoClickRoute,
    seam::Rule::ElementNotTheSame,
    seam::Rule::MarksLost,
    seam::Rule::ClicksNotForwarded,
    seam::Rule::LandingAfterTheLatch,
];

fn rule_index(rule: Option<seam::Rule>) -> i64 {
    rule.and_then(|rule| RULES.iter().position(|r| *r == rule))
        .map_or(-1, |i| i as i64)
}

impl Decisions {
    fn of(decided: &[seam::Decided]) -> Decisions {
        let first_rule = |class: KeyClass| {
            decided
                .iter()
                .find(|key| key.class == class)
                .and_then(|key| key.rule)
        };
        let mut d = Decisions {
            first_letter_rule: first_rule(KeyClass::Printable),
            first_tab_rule: first_rule(KeyClass::Tab),
            ..Decisions::default()
        };
        for key in decided {
            match (key.class, key.verdict) {
                (KeyClass::Printable, Verdict::WithheldPassword) => {
                    d.letters_withheld_password += 1
                }
                (KeyClass::Tab, Verdict::WithheldPassword) => d.tabs_withheld_password += 1,
                _ => {}
            }
            match (key.class, key.verdict) {
                (KeyClass::Printable, Verdict::Recorded) => d.letters_recorded += 1,
                (KeyClass::Tab, Verdict::Recorded) => d.tabs_recorded += 1,
                (_, Verdict::Recorded) => d.other_recorded += 1,
                (_, Verdict::LostAtStop) => d.lost_at_stop += 1,
                (KeyClass::Printable, Verdict::Late) => d.letters_late += 1,
                (KeyClass::Printable, _) => d.letters_withheld += 1,
                (KeyClass::Tab, _) => d.tabs_withheld += 1,
                _ => {}
            }
        }
        d
    }

    fn recorded(&self) -> u32 {
        self.letters_recorded + self.tabs_recorded + self.other_recorded
    }
}

fn control(test: &str, which: &str, fixture: &mut Fixture, events: &Events) -> u32 {
    let _ = seam::take_marks();
    let reset = fixture.ask(CMD_RESET, 0, "reset");
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(250));
        let _ = fixture.ask(CMD_FOCUS, FixtureField::Password.wire(), "focused");
        std::thread::sleep(Duration::from_millis(250));
        let _ = fixture.ask(CMD_FOCUS, FixtureField::Plain.wire(), "focused");
    }
    let _ = events.drain_for(Duration::from_millis(400));
    let _ = seam::take();
    let mut accepted = 0;
    for _ in 0..CONTROL_LETTERS {
        accepted += send(&[key_input(VK_A, false), key_input(VK_A, true)]);
        std::thread::sleep(LETTER_STEP);
    }
    let seen = events.drain_for(Duration::from_millis(900));
    let keys = seam::take();
    super::control_reads(test, which, &seam::take_marks(), &keys);
    let decided = Decisions::of(&keys);
    let state = fixture.ask(CMD_STATE, 0, "state");
    let reached = seen
        .iter()
        .filter(|e| e.modality == Modality::Keystroke)
        .count();
    say(
        test,
        format!(
            "control={which} fg={} focus_plain={} letters_sent={CONTROL_LETTERS} accepted={accepted} \
             letters_plain={} keystroke_events={reached} letters_recorded_by_the_seam={}",
            get(&reset, "fg"),
            get(&reset, "focused"),
            get(&state, "letters_plain"),
            decided.letters_recorded,
        ),
    );
    u32::try_from(reached).unwrap_or(u32::MAX)
}

fn key_pair(key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> [INPUT; 2] {
    [key_input(key, false), key_input(key, true)]
}

fn run_rounds(
    test: &str,
    fixture: &mut Fixture,
    events: &Events,
    keyboard: &RawInputSource,
    phase: Phase,
) -> Totals {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
    let mut totals = Totals::default();
    for round in 0..phase.rounds {
        let mut reset_arg = 0;
        if phase.lead == Lead::TabTab {
            reset_arg |= ARG_SECOND;
        }
        if phase.start_in_password {
            reset_arg |= ARG_START_IN_PASSWORD;
        }
        let reset = fixture.ask(CMD_RESET, reset_arg, "reset");
        if phase.lead == Lead::Enter {
            let _ = fixture.ask(CMD_FOCUS, FixtureField::Password.wire(), "focused");
            std::thread::sleep(Duration::from_millis(250));
            let _ = fixture.ask(CMD_FOCUS, FixtureField::Plain.wire(), "focused");
        }
        let _ = events.drain_for(Duration::from_millis(400));
        let _ = seam::take();
        let before = keyboard.health().counters;
        let mut arg = usize::try_from(phase.stall_ms).unwrap_or(0) & 0xFFFF;
        if phase.lead == Lead::Click {
            arg |= ARG_AIM;
        }
        if phase.lead == Lead::TabTab {
            arg |= ARG_STALL_AFTER_A_TAB;
        }
        if phase.wait == Wait::ServesSent {
            arg |= ARG_SERVE_SENT;
        }
        if phase.self_focus {
            arg |= ARG_SELF_FOCUS;
        }
        if phase.two_step {
            arg |= ARG_TWO_STEP;
        }
        if phase.other_front {
            arg |= ARG_OTHER_FRONT;
        }
        match phase.lead {
            Lead::LateMove => arg |= ARG_LATE_MOVE,
            Lead::OwnChangeThenLateMove => arg |= ARG_LATE_MOVE | ARG_OWN_CHANGE,
            _ => {}
        }
        let ack = fixture.ask(CMD_ROUND, arg, "ack");
        let sent_at = Instant::now();
        let mut letters_sent = 0u32;
        let mut accepted = 0u32;
        let mut inputs = 0u32;
        if get(&ack, "aimed") == 1 {
            let mut burst: Vec<INPUT> = Vec::new();
            match phase.lead {
                Lead::Tab => burst.extend(key_pair(VK_TAB)),
                Lead::Click => {
                    burst.push(mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN));
                    burst.push(mouse_input(0, 0, MOUSEEVENTF_LEFTUP));
                }
                Lead::Enter => burst.extend(key_pair(VK_RETURN)),
                Lead::AccessKey => burst.extend([
                    key_input(VK_MENU, false),
                    key_input(VK_P, false),
                    key_input(VK_P, true),
                    key_input(VK_MENU, true),
                ]),
                Lead::TabTab => {
                    accepted += send(&key_pair(VK_TAB));
                    inputs += 2;
                    std::thread::sleep(Duration::from_millis(30));
                    accepted += send(&key_pair(VK_TAB));
                    inputs += 2;
                    std::thread::sleep(Duration::from_millis(30));
                }
                Lead::Nothing | Lead::LateMove | Lead::OwnChangeThenLateMove => {}
            }
            burst.extend(key_pair(VK_A));
            accepted += send(&burst);
            inputs += burst.len() as u32;
            letters_sent += 1;
            for _ in 0..phase.letters_after {
                std::thread::sleep(LETTER_STEP);
                accepted += send(&key_pair(VK_A));
                inputs += 2;
                letters_sent += 1;
            }
            let wait = Duration::from_millis(phase.stall_ms);
            while phase.letters_through_the_wait && sent_at.elapsed() + LETTER_STEP < wait {
                std::thread::sleep(LETTER_STEP);
                accepted += send(&key_pair(VK_A));
                inputs += 2;
                letters_sent += 1;
            }
        }
        let woke = fixture.expect("woke", Duration::from_secs(10));
        let seen = events.drain_for(Duration::from_millis(900));
        let state = fixture.ask(CMD_STATE, 0, "state");
        let after = keyboard.health().counters;
        let decided = Decisions::of(&seam::take());
        let keystroke_events = seen
            .iter()
            .filter(|e| e.modality == Modality::Keystroke)
            .count() as u32;
        let records_beyond_time_app_and_class = seen
            .iter()
            .filter(|e| e.modality == Modality::Keystroke)
            .filter(|e| {
                e.foreground_app.is_empty()
                    || !e.view.as_ref().is_some_and(|view| {
                        view.key_class.is_some()
                            && view.click.is_none()
                            && view.fold.is_none()
                            && view.title.is_none()
                    })
            })
            .count() as u32;
        let landed_key = match phase.landing {
            Landing::Password => "letters_password",
            Landing::Plain => "letters_plain",
        };
        let letters_landed = u32::try_from(get(&state, landed_key)).unwrap_or(0);
        let letters_elsewhere = get(&state, "letters_plain")
            + get(&state, "letters_password")
            + get(&state, "letters_second")
            + get(&state, "letters_other")
            - i64::from(letters_landed);
        let start_focus = if phase.start_in_password {
            "focus_password"
        } else {
            "focus_plain"
        };
        let password_unchanged_at_the_wake = match phase.landing {
            Landing::Password => get(&woke, "password_len") == 0,
            Landing::Plain => get(&woke, "password_len") == get(&ack, "password_len"),
        };
        let conditions = [
            (
                "own_window_not_in_front_at_the_reset",
                get(&reset, "fg") == 1,
            ),
            ("reactivated", get(&reset, "reactivated") == 0),
            ("not_focused_at_the_reset", get(&reset, "focused") == 1),
            ("own_window_not_in_front_at_the_send", get(&ack, "fg") == 1),
            ("start_field_not_focused", get(&ack, start_focus) == 1),
            (
                "password_not_empty_at_the_send",
                phase.start_in_password || get(&ack, "password_len") == 0,
            ),
            ("not_aimed", get(&ack, "aimed") == 1),
            ("inputs_not_accepted", accepted == inputs),
            (
                "password_changed_before_the_wake",
                password_unchanged_at_the_wake,
            ),
            (
                "own_window_not_in_front_after",
                phase.other_front || get(&state, "fg") == 1,
            ),
            ("letters_not_all_landed", letters_landed == letters_sent),
            ("letters_elsewhere", letters_elsewhere == 0),
        ];
        let why: Vec<&str> = conditions
            .iter()
            .filter(|(_, ok)| !ok)
            .map(|(name, _)| *name)
            .collect();
        let counted = why.is_empty();
        let sink_differs = keystroke_events != decided.recorded();
        let late_moved = after.late_events.saturating_sub(before.late_events);
        let withheld_set = !before.keys_withheld && after.keys_withheld;
        say(
            test,
            format!(
                "phase={} round={round} counted={} letters_sent={letters_sent} letters_landed={letters_landed} \
                 letters_recorded={} tabs_recorded={} other_recorded={} recorded_by_the_seam={} \
                 keystroke_events={keystroke_events} letters_withheld={} letters_late={} tabs_withheld={} \
                 lost_at_stop={} rule_of_the_first_letter={} rule_of_the_first_tab={} \
                 fg_at_send={} focus_at_send={} plain_len_at_send={} password_len_at_send={} \
                 plain_len_at_wake={} password_len_at_wake={} plain_len_after={} password_len_after={} \
                 letters_plain={} letters_password={} letters_second={} letters_other={} tabs_plain={} \
                 tabs_password={} tabs_second={} fg_after={} reactivated={} aimed={} \
                 late_events_moved_by={late_moved} keys_withheld_before={} keys_withheld_after={} \
                 sink_differs_from_the_seam={} uncounted_because={}",
                phase.name,
                flag(counted),
                decided.letters_recorded,
                decided.tabs_recorded,
                decided.other_recorded,
                decided.recorded(),
                decided.letters_withheld,
                decided.letters_late,
                decided.tabs_withheld,
                decided.lost_at_stop,
                rule_index(decided.first_letter_rule),
                rule_index(decided.first_tab_rule),
                get(&ack, "fg"),
                get(&ack, start_focus),
                get(&ack, "plain_len"),
                get(&ack, "password_len"),
                get(&woke, "plain_len"),
                get(&woke, "password_len"),
                get(&state, "plain_len"),
                get(&state, "password_len"),
                get(&state, "letters_plain"),
                get(&state, "letters_password"),
                get(&state, "letters_second"),
                get(&state, "letters_other"),
                get(&state, "tabs_plain"),
                get(&state, "tabs_password"),
                get(&state, "tabs_second"),
                get(&state, "fg"),
                get(&reset, "reactivated"),
                get(&ack, "aimed"),
                flag(before.keys_withheld),
                flag(after.keys_withheld),
                flag(sink_differs),
                if why.is_empty() {
                    "none".to_string()
                } else {
                    why.join(",")
                },
            ),
        );
        totals.rounds += 1;
        totals.sink_differs += u32::from(sink_differs);
        totals.keystroke_events += keystroke_events;
        totals.records_beyond_time_app_and_class += records_beyond_time_app_and_class;
        totals.late_moved += late_moved;
        totals.withheld_set += u32::from(withheld_set);
        if counted {
            totals.counted += 1;
            totals.letters_sent += letters_sent;
            totals.letters_landed += letters_landed;
            totals.letters_recorded += decided.letters_recorded;
            totals.tabs_recorded += decided.tabs_recorded;
            totals.other_recorded += decided.other_recorded;
            totals.rounds_with_a_recorded_letter += u32::from(decided.letters_recorded > 0);
            let letters_decided =
                decided.letters_recorded + decided.letters_withheld + decided.letters_late;
            totals.rounds_not_withheld_by_the_password_field += u32::from(
                decided.letters_withheld_password != letters_decided
                    || letters_decided == 0
                    || decided.tabs_withheld_password != 1,
            );
        }
    }
    totals
}

static SESSIONS: AtomicU32 = AtomicU32::new(0);

struct Session {
    test: &'static str,
    id: u32,
    at: u32,
    fixture: Fixture,
    keyboard: RawInputSource,
    events: Events,
    sink: SinkHandle,
    before: ControlRead,
}

impl Session {
    fn open(test: &'static str) -> Session {
        source_built_by_test(test, "keyboard");
        let mut fixture = Fixture::open(test, false);
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        say(test, format!("keyboard_start_ok={}", start.is_ok()));
        start_report(test, "keyboard", &start);
        assert!(
            start.is_ok(),
            "INCONCLUSIVE: the keyboard source did not start: {start:?}"
        );
        let _ = events.drain_for(Duration::from_millis(500));
        let id = SESSIONS.fetch_add(1, Ordering::SeqCst) + 1;
        let recorded = control(test, "before", &mut fixture, &events);
        say(
            test,
            format!("session={id} step=0 control=before recorded={recorded}"),
        );
        Session {
            test,
            id,
            at: 0,
            fixture,
            keyboard,
            events,
            sink,
            before: ControlRead {
                step: Step { session: id, at: 0 },
                recorded,
            },
        }
    }

    fn begin(&mut self, test: &str, name: &str) -> (Step, u32) {
        if self.at > 0 {
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(250));
                let _ = self
                    .fixture
                    .ask(CMD_FOCUS, FixtureField::Password.wire(), "focused");
                std::thread::sleep(Duration::from_millis(250));
                let _ = self
                    .fixture
                    .ask(CMD_FOCUS, FixtureField::Plain.wire(), "focused");
            }
            let _ = self.events.drain_for(Duration::from_millis(400));
            let _ = seam::take();
        }
        let open = kinds::latches_of(self.keyboard.open_latches().as_deref());
        let step = self.next(test, name);
        say(
            test,
            format!("phase={name} open_latches_at_the_start={open}"),
        );
        (step, open)
    }

    fn next(&mut self, test: &str, name: &str) -> Step {
        self.at += 1;
        say(
            test,
            format!("session={} step={} phase={name}", self.id, self.at),
        );
        Step {
            session: self.id,
            at: self.at,
        }
    }

    fn rounds(&mut self, phase: Phase) -> PhaseRun {
        let phase = Phase {
            rounds: rounds_for(1, phase.rounds),
            ..phase
        };
        let (step, open) = self.begin(phase.test, phase.name);
        let mut totals = Totals::default();
        if phase.lead == Lead::Click {
            let unstalled = Phase {
                stall_ms: 0,
                rounds: rounds_for(1, CLICK_ROUNDS_WITHOUT_WAIT),
                name: "click_with_no_wait",
                ..phase
            };
            totals.add(run_rounds(
                phase.test,
                &mut self.fixture,
                &self.events,
                &self.keyboard,
                unstalled,
            ));
        }
        totals.add(run_rounds(
            phase.test,
            &mut self.fixture,
            &self.events,
            &self.keyboard,
            phase,
        ));
        say(
            phase.test,
            format!(
                "form={} rounds_of_the_phase={} rounds={} counted_rounds={}",
                form().word(),
                phase.rounds,
                totals.rounds,
                totals.counted
            ),
        );
        PhaseRun {
            test: phase.test,
            name: phase.name,
            wait: phase.wait,
            step,
            outcome: Outcome::Rounds {
                totals,
                checks: phase.checks,
            },
            open_latches_at_the_start: open,
        }
    }

    fn native(
        &mut self,
        (test, name): (&'static str, &'static str),
        (start, landing): (FixtureField, &'static str),
        act: fn(&mut Fixture) -> bool,
    ) -> PhaseRun {
        let (step, open) = self.begin(test, name);
        let rounds = rounds_for(1, 5);
        let mut counted = 0;
        let mut recorded = 0;
        for round in 0..rounds {
            let (c, r) = native_round(
                (test, name),
                round,
                &mut self.fixture,
                &self.events,
                (start, landing),
                act,
            );
            counted += u32::from(c);
            recorded += r;
        }
        PhaseRun {
            test,
            name,
            wait: Wait::Sleeps,
            step,
            outcome: Outcome::Native {
                rounds,
                counted,
                recorded,
            },
            open_latches_at_the_start: open,
        }
    }

    fn close(self) -> Controls {
        let Session {
            test,
            id,
            at,
            mut fixture,
            mut keyboard,
            events,
            sink,
            before,
        } = self;
        let step = Step {
            session: id,
            at: at + 1,
        };
        let recorded = control(test, "after", &mut fixture, &events);
        let late_at_the_end = keyboard.health().counters.late_events;
        let _ = stop_counted(&mut keyboard, "keyboard");
        drop(fixture);
        drop(sink);
        say(
            test,
            format!(
                "session={id} step={} control=after recorded={recorded} late_value={late_at_the_end}",
                step.at
            ),
        );
        Controls {
            before,
            after: ControlRead { step, recorded },
            late_at_the_end,
        }
    }
}

fn run_kind(kind: Kind) -> Vec<PhaseRun> {
    let phases = kinds::refuse_an_empty_kind(kind.test(), kind.phases())
        .unwrap_or_else(|why| panic!("{why}"));
    let mut session = Session::open(kind.test());
    let runs: Vec<PhaseRun> = phases
        .into_iter()
        .map(|phase| session.rounds(phase))
        .collect();
    let controls = session.close();
    let failures = kinds::verdicts(&runs, &controls, say);
    assert!(
        failures.is_empty(),
        "{} of the {} phases failed: {}",
        failures.len(),
        runs.len(),
        failures.join(";")
    );
    runs
}

fn kind_test(kind: Kind) {
    with_deadline(
        kind.test(),
        Duration::from_secs(kind.limit_secs()),
        move || {
            run_kind(kind);
        },
    );
}

impl FieldWindow {
    pub(super) fn phase_e(
        &self,
        test: &str,
        events: &Events,
        keyboard: &RawInputSource,
    ) -> (Totals, u32, u32, bool) {
        let mut fixture = self.fixture();
        let first = control(test, "before", &mut fixture, events);
        let masked = get(&fixture.ask(CMD_STATE, 0, "state"), "password_masked") == 1;
        say(
            test,
            format!("phase=E password_edit_masked_at_the_start={}", flag(masked)),
        );
        let phase = Phase {
            rounds: rounds_for(1, 20),
            letters_through_the_wait: false,
            ..phase("fixture-native", "E", Lead::Tab, Wait::Sleeps, 0)
        };
        let totals = run_rounds(test, &mut fixture, events, keyboard, phase);
        let last = control(test, "after", &mut fixture, events);
        (totals, first, last, masked)
    }
}

#[test]
#[ignore = "functional: reads the round word that 50-windows-functional passes"]
fn the_round_count_of_the_stall_tests() {
    const WAITS_MS: [u64; 5] = [150, 200, 300, 600, 2_000];
    let form = form();
    let rounds: Vec<String> = WAITS_MS
        .iter()
        .map(|ms| format!("{ms}:{}", rounds_at(form, *ms)))
        .collect();
    println!(
        "form={} counted_rounds_at_each_wait_ms={}",
        form.word(),
        rounds.join(",")
    );
    if form == Form::Short {
        for ms in WAITS_MS {
            let (short, long) = (rounds_at(Form::Short, ms), rounds_at(Form::Long, ms));
            if short < long {
                println!(
                    "left_out {} of the {long} counted rounds of each native stall test at the wait of {ms} ms, \
                     which the long form runs",
                    long - short
                );
            }
        }
        println!("left_out the loops by exact name, which the long form runs");
        println!(
            "left_out the rounds of every other stall phase but one, of phase E but one, of each browser stall and swap phase but one, of the fixtures' phases E and F but one, of the letter and Tab out of a swapped field but one, and of each held page of the self-move but one; each phase and each wait still runs once"
        );
    }
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_at_each_wait() {
    kind_test(Kind::TabSleeps);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_at_each_wait() {
    kind_test(Kind::TabServes);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_at_each_wait() {
    kind_test(Kind::ClickSleeps);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_at_each_wait() {
    kind_test(Kind::ClickServes);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn a_second_field_and_a_window_of_another_thread_while_the_program_waits_300_ms() {
    kind_test(Kind::SecondFieldAndOtherFront);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn enter_a_tab_out_and_a_focus_move_in_two_steps_record_no_letter() {
    kind_test(Kind::EnterTabOutAndTwoStep);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn an_access_key_that_moves_the_focus_while_the_program_waits_records_no_letter() {
    kind_test(Kind::AccessKey);
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn letters_at_a_password_edit_that_makes_no_event_then_a_tab_and_a_letter_record_none_of_them() {
    let test = "stall-silent-password";
    with_deadline(test, Duration::from_secs(200), move || {
        source_built_by_test(test, "keyboard");
        let mut fixture = Fixture::open(test, false);
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        start_report(test, "keyboard", &start);
        assert!(
            start.is_ok(),
            "INCONCLUSIVE: the keyboard source did not start: {start:?}"
        );
        let _ = events.drain_for(Duration::from_millis(500));
        let before = control(test, "before", &mut fixture, &events);
        let mut rounds = 0;
        let mut counted = 0;
        let mut recorded_at_the_password = 0;
        for (wait, stall_ms) in [
            (Wait::Sleeps, 0),
            (Wait::ServesSent, 300),
            (Wait::Sleeps, 300),
        ] {
            for round in 0..rounds_for(2, 5) {
                let reset = fixture.ask(
                    CMD_RESET,
                    ARG_START_IN_PASSWORD | ARG_SILENT_PASSWORD,
                    "reset",
                );
                let _ = events.drain_for(Duration::from_millis(400));
                let _ = seam::take();
                let mut arg = usize::try_from(stall_ms).unwrap_or(0) & 0xFFFF;
                if wait == Wait::ServesSent {
                    arg |= ARG_SERVE_SENT;
                }
                let ack = fixture.ask(CMD_ROUND, arg, "ack");
                let mut accepted = 0;
                for _ in 0..3 {
                    accepted += send(&key_pair(VK_A));
                    std::thread::sleep(LETTER_STEP);
                }
                accepted += send(&key_pair(VK_TAB));
                std::thread::sleep(LETTER_STEP);
                accepted += send(&key_pair(VK_A));
                let _ = fixture.expect("woke", Duration::from_secs(10));
                let _ = events.drain_for(Duration::from_millis(900));
                let state = fixture.ask(CMD_STATE, 0, "state");
                let letters: Vec<bool> = seam::take()
                    .iter()
                    .filter(|key| key.class == KeyClass::Printable)
                    .map(|key| key.verdict == Verdict::Recorded)
                    .collect();
                let at_the_password = letters.iter().take(3).filter(|r| **r).count();
                let in_the_plain_field = letters.get(3).copied().unwrap_or(false);
                let ok = get(&reset, "fg") == 1
                    && get(&ack, "focus_password") == 1
                    && accepted == 10
                    && get(&state, "letters_password") == 3
                    && get(&state, "letters_plain") == 1
                    && get(&state, "password_len") == 0
                    && letters.len() == 4;
                say(
                    test,
                    format!(
                        "phase=silent_password wait={wait:?} stall_ms={stall_ms} round={round} counted={} \
                         letters_password={} letters_plain={} password_len={} letters_decided={} \
                         recorded_at_the_password={at_the_password} recorded_in_the_plain_field={}",
                        u8::from(ok),
                        get(&state, "letters_password"),
                        get(&state, "letters_plain"),
                        get(&state, "password_len"),
                        letters.len(),
                        u8::from(in_the_plain_field),
                    ),
                );
                rounds += 1;
                counted += u32::from(ok);
                recorded_at_the_password += at_the_password;
            }
        }
        let after = control(test, "after", &mut fixture, &events);
        let late = keyboard.health().counters.late_events;
        let _ = stop_counted(&mut keyboard, "keyboard");
        drop(fixture);
        say(
            test,
            format!(
                "form={} rounds={rounds} counted_rounds={counted} recorded_at_the_password={recorded_at_the_password} \
                 control_before={before} control_after={after}",
                form().word()
            ),
        );
        assert_eq!(
            recorded_at_the_password, 0,
            "a letter typed at the password edit that makes no event was recorded"
        );
        assert_eq!(late, 0, "the source's late value moved");
        assert_eq!(
            (before, after),
            (CONTROL_LETTERS, CONTROL_LETTERS),
            "a control recorded another number of letters in the plain field"
        );
        assert_eq!(counted, rounds, "INCONCLUSIVE: not every round counted");
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn five_letters_of_the_plain_field_after_the_programs_own_focus_move_are_recorded() {
    let test = "stall-plain-control";
    with_deadline(test, Duration::from_secs(120), move || {
        source_built_by_test(test, "keyboard");
        let mut fixture = Fixture::open(test, false);
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        start_report(test, "keyboard", &start);
        assert!(
            start.is_ok(),
            "INCONCLUSIVE: the keyboard source did not start: {start:?}"
        );
        let _ = events.drain_for(Duration::from_millis(500));
        let recorded = control(test, "phase", &mut fixture, &events);
        let late = keyboard.health().counters.late_events;
        let _ = stop_counted(&mut keyboard, "keyboard");
        drop(fixture);
        assert_eq!(
            recorded, CONTROL_LETTERS,
            "the letters of the plain field were not all recorded"
        );
        assert_eq!(late, 0, "the source's late value moved");
    });
}

fn five_letters() -> u32 {
    let mut accepted = 0;
    for _ in 0..5 {
        accepted += send(&[key_input(VK_A, false), key_input(VK_A, true)]);
        std::thread::sleep(LETTER_STEP);
    }
    accepted
}

fn native_round(
    (test, phase): (&str, &str),
    round: u32,
    fixture: &mut Fixture,
    events: &Events,
    (start, landing): (FixtureField, &str),
    act: impl FnOnce(&mut Fixture) -> bool,
) -> (bool, u32) {
    let reset = fixture.ask(CMD_RESET, 0, "reset");
    let other = if start == FixtureField::Plain {
        FixtureField::Password
    } else {
        FixtureField::Plain
    };
    let _ = fixture.ask(CMD_FOCUS, other.wire(), "focused");
    std::thread::sleep(Duration::from_millis(250));
    let _ = fixture.ask(CMD_FOCUS, start.wire(), "focused");
    let _ = events.drain_for(Duration::from_millis(400));
    let _ = seam::take();
    let _ = seam::take_marks();
    let acted = act(fixture);
    let accepted = five_letters();
    let seen = events.drain_for(Duration::from_millis(900));
    let decided = Decisions::of(&seam::take());
    let keystroke_events = seen
        .iter()
        .filter(|e| e.modality == Modality::Keystroke)
        .count() as u32;
    let state = fixture.ask(CMD_STATE, 0, "state");
    let counted = get(&reset, "fg") == 1
        && acted
        && get(&state, &format!("focus_{landing}")) == 1
        && get(&state, &format!("letters_{landing}")) == 5;
    say(
        test,
        format!(
            "phase={phase} round={round} fg={} acted={} accepted={accepted} focus_{landing}={} \
             letters_{landing}={} letters_decided={} letters_recorded={} keystroke_events={keystroke_events} \
             first_letter_rule={} counted={}",
            get(&reset, "fg"),
            u8::from(acted),
            get(&state, &format!("focus_{landing}")),
            get(&state, &format!("letters_{landing}")),
            decided.letters_recorded + decided.letters_withheld + decided.letters_late,
            decided.letters_recorded,
            rule_index(decided.first_letter_rule),
            u8::from(counted),
        ),
    );
    (counted, decided.recorded().max(keystroke_events))
}

fn masked(fixture: &mut Fixture) -> bool {
    get(
        &fixture.ask(CMD_MASK, FixtureField::Plain.wire(), "masked"),
        "ok",
    ) == 1
}

fn notified_and_unseen(fixture: &mut Fixture) -> bool {
    let notified = get(
        &fixture.ask(CMD_NOTIFY_FOCUS, FixtureField::Plain.wire(), "notified"),
        "ok",
    ) == 1;
    std::thread::sleep(Duration::from_millis(150));
    let marks = seam::take_marks();
    let seen = marks.iter().any(|mark| {
        mark.what == "event"
            && mark.state == Some(halvo_capture::secure_input::windows::FieldState::Clear)
    });
    let listed: Vec<String> = marks
        .iter()
        .map(|mark| {
            let element = mark.element.as_deref().unwrap_or("");
            let password = element
                .split(' ')
                .find(|part| part.starts_with("is_password="))
                .unwrap_or("is_password=none");
            format!("{}:{:?}:{password}", mark.what, mark.state)
        })
        .collect();
    say(
        "stall-stale-focus",
        format!(
            "notified={notified} clear_focus_event_seen={seen} marks_after_the_notice={} [{}]",
            marks.len(),
            listed.join(",")
        ),
    );
    notified
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn a_masked_edit_and_a_focus_notice_that_the_system_does_not_deliver_record_no_letter() {
    let test = "stall-mask-stale-focus";
    with_deadline(test, Duration::from_secs(150), move || {
        let mut session = Session::open(test);
        let runs = [
            session.native(NATIVE_PHASES[0], (FixtureField::Plain, "plain"), masked),
            session.native(
                NATIVE_PHASES[1],
                (FixtureField::Password, "password"),
                notified_and_unseen,
            ),
        ];
        let controls = session.close();
        let failures = kinds::verdicts(&runs, &controls, say);
        assert!(
            failures.is_empty(),
            "{} of the {} phases failed: {}",
            failures.len(),
            runs.len(),
            failures.join(";")
        );
    });
}

pub(super) fn the_programs_own_change_of_the_focused_field_before_a_late_move(
) -> (u32, u64, u32, u32) {
    run_kind(Kind::OwnChangeThenLateMove).iter().fold(
        (0, 0, 0, 0),
        |(recorded, late, records, beyond), run| match run.outcome {
            Outcome::Rounds { totals, .. } => (
                recorded + totals.letters_recorded,
                late + totals.late_moved,
                records + totals.keystroke_events,
                beyond + totals.records_beyond_time_app_and_class,
            ),
            Outcome::Native { .. } => (recorded, late, records, beyond),
        },
    )
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn a_key_queued_before_a_late_move_to_the_password_field_records_nothing() {
    kind_test(Kind::LateMove);
}

pub(super) fn no_focus_key_and_the_program_moves_the_focus_itself() -> (u32, u64) {
    run_kind(Kind::SelfFocus)
        .iter()
        .fold((0, 0), |(recorded, late), run| match run.outcome {
            Outcome::Rounds { totals, .. } => {
                (recorded + totals.letters_recorded, late + totals.late_moved)
            }
            Outcome::Native { .. } => (recorded, late),
        })
}
