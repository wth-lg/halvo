//! The stall phases (W3-5 C3, W3-7 L1 and L3): the native fixture as a
//! process of its own with an ordinary message loop, whose thread waits while
//! the keys are typed ahead of it. Each phase gives its verdict by round and
//! prints numbers alone.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_capture::event::Modality;
use halvo_capture::secure_input::windows::PasswordGate;
use halvo_capture::source::{CaptureSource, SourceContext};
use halvo_capture::windows::input::seam::{self, KeyClass, Verdict};
use halvo_capture::windows::input::RawInputSource;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT, MOUSE_EVENT_FLAGS, VK_A, VK_MENU, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowRect, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsDialogMessageW, MsgWaitForMultipleObjectsEx, PeekMessageW,
    PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow, SetWindowTextW, ShowWindow,
    TranslateMessage, UnregisterClassW, WindowFromPoint, ES_PASSWORD, MSG, MWMO_INPUTAVAILABLE,
    PEEK_MESSAGE_REMOVE_TYPE, PM_NOREMOVE, PM_QS_SENDMESSAGE, QS_SENDMESSAGE, SC_KEYMENU,
    SM_CXSCREEN, SM_CYSCREEN, SW_MINIMIZE, SW_RESTORE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_KEYDOWN, WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
    WS_VISIBLE,
};

use super::{
    capture, focus_on, foreground_window, key_input, pump_thread, say, send, source_built_by_test,
    stop_counted, with_deadline, Events, Field,
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
/// Reset flags: enable the second plain edit; start with the focus in the
/// password edit.
const ARG_SECOND: usize = 0x1;
const ARG_START_IN_PASSWORD: usize = 0x2;
/// The fixture thread's wait between its two focus moves (W3-5 C6).
const TWO_STEP_MS: u64 = 200;

/// The step between the letters that are typed through a wait.
const LETTER_STEP: Duration = Duration::from_millis(60);
/// Letters typed in the plain field for each control of a test.
const CONTROL_LETTERS: u32 = 5;

fn flag(value: bool) -> u8 {
    u8::from(value)
}

// ---------------------------------------------------------------- the fixture

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

    /// Brings the window to the front and answers whether it is there, with
    /// the code of the call that put it there (1 SetForegroundWindow, 2 the
    /// Alt key, 3 joined input, 4 minimize and restore, 0 none).
    fn activate(&self) -> (bool, u8) {
        // SAFETY: the three calls take this thread's window handle by value.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetForegroundWindow(self.hwnd);
        }
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return (true, 1);
        }
        send(&[key_input(VK_MENU, false)]);
        // SAFETY: SetForegroundWindow takes a window handle by value.
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        send(&[key_input(VK_MENU, true)]);
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return (true, 2);
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
            return (true, 3);
        }
        // SAFETY: ShowWindow takes this thread's window handle by value.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
        }
        pump_thread(Duration::from_millis(200));
        let there = self.is_foreground();
        (there, if there { 4 } else { 0 })
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

/// Puts the pointer over the middle of `target` and says whether it is there.
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

/// The wait of the fixture's thread: one that serves nothing, or one that
/// answers sent messages while posted messages and input stay queued.
fn stall(stall_ms: u64, serve_sent: bool) {
    if stall_ms == 0 {
        return;
    }
    if !serve_sent {
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

/// A wander of the focus: its start, its span, the next move, the seed of the
/// waits and the password field's intervals in unix ms.
type Wander = (Instant, Duration, Instant, u64, Vec<(u128, u128)>);

/// The field that a command names: 0 the plain edit, 1 the password edit,
/// 2 the second plain edit, which is disabled until a round of order (8).
const FIELD_PLAIN: usize = 0;
const FIELD_PASSWORD: usize = 1;
const FIELD_SECOND: usize = 2;
const WANDER_TIMER: usize = 1;

/// The fixture process: a window with a plain edit, a second plain edit and a
/// password edit, the dialog's own message loop on GetMessageW, commands as
/// posted messages. It counts the key-downs of the letter and of Tab by the
/// control that each was addressed to, and prints counts, lengths, flags and
/// times alone.
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
    // The password edit's own mask character, set again at each reset, so
    // that a phase after phase D's reveal runs on a masked edit (W3-10 T1).
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
    let field = |index: usize| match index {
        FIELD_PASSWORD => password,
        FIELD_SECOND => second,
        _ => plain,
    };
    pump_thread(Duration::from_millis(300));
    let (foreground, method) = window.activate();
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
    println!("STALLC ready fg={} method={method}", flag(foreground));
    let mut letters = [0u32; 4];
    let mut tabs = [0u32; 4];
    let mut wander: Option<Wander> = None;
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that GetMessageW fills.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        if msg.message == WM_TIMER && msg.hwnd == window.hwnd && msg.wParam.0 == WANDER_TIMER {
            if let Some((began, span, next, seed, intervals)) = wander.as_mut() {
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
                        window.activate();
                    }
                    // SAFETY: SendMessageW passes a plain value to this
                    // thread's edit controls; 0 takes the plain edit's mask
                    // away, and the password edit gets its own mask again.
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
                    // thread's edit control; a mask character sets
                    // ES_PASSWORD on the edit while it keeps the focus.
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
                    wander = Some((
                        Instant::now(),
                        Duration::from_millis(arg as u64),
                        Instant::now(),
                        7,
                        Vec::new(),
                    ));
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
                    stall(stall_ms, arg & ARG_SERVE_SENT != 0);
                    let (plain_len, password_len) = (text_len(plain), text_len(password));
                    if arg & ARG_SELF_FOCUS != 0 {
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

/// Counts a key-down of the letter or of Tab by the control it was sent to.
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

/// Takes and dispatches the window's messages until the first Tab has moved
/// the focus, for at most one second (order (8): the program stalls after the
/// first Tab).
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

/// Order (9): a window of another thread of this process comes to the front
/// halfway through the wait, and gives the front back when it closes.
fn other_window_in_front(stall_ms: u64) -> std::thread::JoinHandle<()> {
    let back = foreground_window().0 as usize;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(stall_ms / 2));
        let other =
            FixtureWindow::create_named("HalvoVerifyStallOther", "halvo-verify-stall-other");
        other.activate();
        pump_thread(Duration::from_millis(stall_ms / 2 + 300));
        drop(other);
        // SAFETY: SetForegroundWindow takes a handle value; this process owns
        // the foreground, so it may give it to its own window.
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
    method: i64,
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
            method: 0,
        };
        let ready = fixture.expect("ready", Duration::from_secs(40));
        fixture.foreground = get(&ready, "fg") == 1;
        fixture.method = get(&ready, "method");
        say(
            test,
            format!(
                "fixture_process_started=true fixture_window_foreground={} method={}",
                flag(fixture.foreground),
                fixture.method
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

/// The two-field window of the functional tests, as a process of its own
/// (W3-7 Q2 (f)): in the test's own process the detector's read of the
/// focused field takes 221 to 829 ms, so the gate would withhold every key.
pub(super) struct FieldWindow {
    fixture: Mutex<Fixture>,
    pub(super) foreground: bool,
    pub(super) method: &'static str,
}

impl FieldWindow {
    pub(super) fn open(test: &str, first: Field) -> FieldWindow {
        let fixture = Fixture::open(test, first == Field::Password);
        let method = match fixture.method {
            1 => "set-foreground",
            2 => "alt-key-unlock",
            3 => "attach-thread-input",
            4 => "minimize-restore",
            _ => "none",
        };
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
            Field::Plain => FIELD_PLAIN,
            Field::Password => FIELD_PASSWORD,
        };
        let focused = get(&self.fixture().ask(CMD_FOCUS, index, "focused"), "ok") == 1;
        std::thread::sleep(Duration::from_millis(50));
        focused
    }

    /// Puts the pointer over the middle of the field and sends one click
    /// of the left button there (W3-7 K12); says whether the pointer was
    /// over the field.
    pub(super) fn click(&self, field: Field) -> bool {
        let index = match field {
            Field::Plain => FIELD_PLAIN,
            Field::Password => FIELD_PASSWORD,
        };
        let aimed = get(&self.fixture().ask(CMD_AIM, index, "aimed"), "ok") == 1;
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

// ----------------------------------------------------------------- the phases

/// What comes before the first letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lead {
    Tab,
    Click,
    /// Tab, Tab and the letter 30 ms apart, across the second plain edit, and
    /// the program waits after the first Tab (order (8)).
    TabTab,
    /// Enter in the plain field, which moves no focus (W3-5 C5).
    Enter,
    /// No focus key and no click: the program moves the focus by itself
    /// (W3-5 C6).
    Nothing,
}

/// The fixture thread's wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// A wait that serves nothing.
    Sleeps,
    /// A wait that answers sent messages while posted messages and input stay
    /// queued.
    ServesSent,
}

/// Where the letters of a round land when the fixture works as it should.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Landing {
    Password,
    Plain,
}

#[derive(Debug, Clone, Copy)]
struct Phase {
    test: &'static str,
    name: &'static str,
    lead: Lead,
    wait: Wait,
    stall_ms: u64,
    rounds: u32,
    /// Further letters typed through the wait at steps of 60 ms.
    letters_through_the_wait: bool,
    /// Letters typed after the lead when no wait follows (W3-5 C5).
    letters_after: u32,
    start_in_password: bool,
    self_focus: bool,
    two_step: bool,
    other_front: bool,
    landing: Landing,
}

const fn phase(
    test: &'static str,
    name: &'static str,
    lead: Lead,
    wait: Wait,
    stall_ms: u64,
) -> Phase {
    Phase {
        test,
        name,
        lead,
        wait,
        stall_ms,
        rounds: if stall_ms >= 2_000 { 5 } else { 20 },
        letters_through_the_wait: true,
        letters_after: 0,
        start_in_password: false,
        self_focus: false,
        two_step: false,
        other_front: false,
        landing: Landing::Password,
    }
}

/// Rounds of a click phase with no wait, before its stalled rounds: the
/// control that the button-down reached the latch (W3-5 C4).
const CLICK_ROUNDS_WITHOUT_WAIT: u32 = 5;

/// What the source did with the keys of one span, from the test seam.
#[derive(Debug, Default, Clone, Copy)]
struct Decisions {
    letters_recorded: u32,
    tabs_recorded: u32,
    other_recorded: u32,
    letters_late: u32,
    letters_withheld: u32,
    tabs_withheld: u32,
    /// Letters and Tabs withheld with a Password reading (W3-10 T1).
    letters_withheld_password: u32,
    tabs_withheld_password: u32,
    lost_at_stop: u32,
    /// The rule that decided the first letter, as its index in `RULES`.
    first_letter_rule: i64,
    /// The rule that decided the first Tab, as its index in `RULES`.
    first_tab_rule: i64,
}

const RULES: [seam::Rule; 7] = [
    seam::Rule::PasswordEdit,
    seam::Rule::NotClearedInTime,
    seam::Rule::AnotherThreadInFront,
    seam::Rule::StateAtTheKey,
    seam::Rule::MarkAfterTheKey,
    seam::Rule::ReadAfterTheHold,
    seam::Rule::NoClickRoute,
];

fn rule_index(rule: Option<seam::Rule>) -> i64 {
    rule.and_then(|rule| RULES.iter().position(|r| *r == rule))
        .map_or(-1, |i| i as i64)
}

impl Decisions {
    fn of(decided: &[seam::Decided]) -> Decisions {
        let mut d = Decisions {
            first_letter_rule: -1,
            first_tab_rule: -1,
            ..Decisions::default()
        };
        for key in decided {
            if key.class == KeyClass::Printable && d.first_letter_rule < 0 {
                d.first_letter_rule = rule_index(key.rule);
            }
            if key.class == KeyClass::Tab && d.first_tab_rule < 0 {
                d.first_tab_rule = rule_index(key.rule);
            }
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

/// The control of W3-7's correction (3): five letters in the plain field after
/// a focus move by the fixture itself, with no focus key and no click since.
/// It answers how many of them the source recorded.
fn control(test: &str, which: &str, fixture: &mut Fixture, events: &Events) -> u32 {
    let reset = fixture.ask(CMD_RESET, 0, "reset");
    // Three moves of the fixture's own there and back, each 250 ms after
    // the one before, so that an own read sees each move apart and closes a
    // latch that the keys of the rounds left open (each move closes at most
    // one). The last move is the fixture's, with no focus key and no click
    // after it (W3-7's correction (3)).
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(250));
        let _ = fixture.ask(CMD_FOCUS, FIELD_PASSWORD, "focused");
        std::thread::sleep(Duration::from_millis(250));
        let _ = fixture.ask(CMD_FOCUS, FIELD_PLAIN, "focused");
    }
    let _ = events.drain_for(Duration::from_millis(400));
    let _ = seam::take();
    let mut accepted = 0;
    for _ in 0..CONTROL_LETTERS {
        accepted += send(&[key_input(VK_A, false), key_input(VK_A, true)]);
        std::thread::sleep(LETTER_STEP);
    }
    let seen = events.drain_for(Duration::from_millis(900));
    let decided = Decisions::of(&seam::take());
    let state = fixture.ask(CMD_STATE, 0, "state");
    // W3-10 T2: the control counts the keystroke events that reached the
    // sink, not the seam's verdicts, so a source that clears keys and pushes
    // none fails it.
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

#[derive(Debug, Default)]
pub(super) struct Totals {
    pub(super) rounds: u32,
    pub(super) counted: u32,
    pub(super) letters_sent: u32,
    pub(super) letters_landed: u32,
    pub(super) letters_recorded: u32,
    pub(super) tabs_recorded: u32,
    pub(super) other_recorded: u32,
    pub(super) late_moved: u64,
    pub(super) withheld_set: u32,
    pub(super) rounds_with_a_recorded_letter: u32,
    /// Counted rounds in which a letter or the Tab was not withheld with a
    /// Password reading (W3-10 T1; asserted by phase E alone).
    pub(super) rounds_not_withheld_by_the_password_field: u32,
}

impl Totals {
    fn add(&mut self, other: Totals) {
        self.rounds += other.rounds;
        self.counted += other.counted;
        self.letters_sent += other.letters_sent;
        self.letters_landed += other.letters_landed;
        self.letters_recorded += other.letters_recorded;
        self.tabs_recorded += other.tabs_recorded;
        self.other_recorded += other.other_recorded;
        self.late_moved += other.late_moved;
        self.withheld_set += other.withheld_set;
        self.rounds_with_a_recorded_letter += other.rounds_with_a_recorded_letter;
        self.rounds_not_withheld_by_the_password_field +=
            other.rounds_not_withheld_by_the_password_field;
    }
}

fn key_pair(key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> [INPUT; 2] {
    [key_input(key, false), key_input(key, true)]
}

/// Runs the rounds of one phase and answers its totals.
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
            // Enter moves no focus, so its latch stays open until a move:
            // the fixture's own move closes the latch of the round before.
            let _ = fixture.ask(CMD_FOCUS, FIELD_PASSWORD, "focused");
            std::thread::sleep(Duration::from_millis(250));
            let _ = fixture.ask(CMD_FOCUS, FIELD_PLAIN, "focused");
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
                Lead::TabTab => {
                    accepted += send(&key_pair(VK_TAB));
                    inputs += 2;
                    std::thread::sleep(Duration::from_millis(30));
                    accepted += send(&key_pair(VK_TAB));
                    inputs += 2;
                    std::thread::sleep(Duration::from_millis(30));
                }
                Lead::Nothing => {}
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
        let counted = get(&reset, "fg") == 1
            && get(&reset, "reactivated") == 0
            && get(&reset, "focused") == 1
            && get(&ack, "fg") == 1
            && get(&ack, start_focus) == 1
            && (phase.start_in_password || get(&ack, "password_len") == 0)
            && get(&ack, "aimed") == 1
            && accepted == inputs
            && password_unchanged_at_the_wake
            && (phase.other_front || get(&state, "fg") == 1)
            && letters_landed == letters_sent
            && letters_elsewhere == 0;
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
                 late_events_moved_by={late_moved} keys_withheld_before={} keys_withheld_after={}",
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
                decided.first_letter_rule,
                decided.first_tab_rule,
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
            ),
        );
        totals.rounds += 1;
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

/// One stall test: the fixture process, the keyboard source, the control
/// before the first round, the rounds, the control after the last. A recorded
/// letter in a counted round fails it, and so does a recorded key of a class
/// other than Tab, a moved late value, or a control that recorded no letter.
fn stall_test(phase: Phase) {
    let test = phase.test;
    with_deadline(test, Duration::from_secs(300), move || {
        let totals = stall_phase(phase);
        assert_eq!(
            totals.letters_recorded, 0,
            "a letter typed ahead of the program was recorded in a counted round"
        );
        assert_eq!(
            totals.other_recorded, 0,
            "a key of another class than Tab was recorded in a counted round"
        );
        assert_eq!(
            totals.late_moved, 0,
            "the source's late value moved during the phase"
        );
        assert_eq!(
            totals.counted, totals.rounds,
            "INCONCLUSIVE: not every round counted"
        );
    });
}

/// The body of a stall phase; it panics when a control records no letter.
fn stall_phase(phase: Phase) -> Totals {
    let test = phase.test;
    source_built_by_test(test, "keyboard");
    let mut fixture = Fixture::open(test, false);
    let (sink, events) = capture(8192);
    let mut keyboard = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
    let start = keyboard.begin(SourceContext::new(sink.clone()));
    say(test, format!("keyboard_start_ok={}", start.is_ok()));
    assert!(
        start.is_ok(),
        "INCONCLUSIVE: the keyboard source did not start"
    );
    let _ = events.drain_for(Duration::from_millis(500));
    let first = control(test, "before", &mut fixture, &events);
    let mut totals = Totals::default();
    if phase.lead == Lead::Click {
        let unstalled = Phase {
            stall_ms: 0,
            rounds: CLICK_ROUNDS_WITHOUT_WAIT,
            name: "click_with_no_wait",
            ..phase
        };
        totals.add(run_rounds(
            test,
            &mut fixture,
            &events,
            &keyboard,
            unstalled,
        ));
    }
    totals.add(run_rounds(test, &mut fixture, &events, &keyboard, phase));
    let last = control(test, "after", &mut fixture, &events);
    let _ = stop_counted(&mut keyboard, "keyboard");
    drop(fixture);
    say(
        test,
        format!(
            "summary phase={} rounds={} counted_rounds={} letters_sent={} letters_landed={} \
             letters_recorded={} rounds_with_a_recorded_letter={} tabs_recorded={} other_recorded={} \
             late_events_moved_by={} rounds_that_set_keys_withheld={} control_before={first} control_after={last}",
            phase.name,
            totals.rounds,
            totals.counted,
            totals.letters_sent,
            totals.letters_landed,
            totals.letters_recorded,
            totals.rounds_with_a_recorded_letter,
            totals.tabs_recorded,
            totals.other_recorded,
            totals.late_moved,
            totals.withheld_set,
        ),
    );
    assert!(
        first > 0 && last > 0,
        "a control recorded no letter, so the source did not show that it records"
    );
    totals
}

impl FieldWindow {
    /// Phase E of the native fixture on its own process (K11, W3-5 C2): a Tab
    /// and a letter in one burst from the plain field, 20 rounds, a verdict by
    /// round, with the two controls of W3-7's correction (3).
    pub(super) fn phase_e(
        &self,
        test: &str,
        events: &Events,
        keyboard: &RawInputSource,
    ) -> (Totals, u32, u32, bool) {
        let mut fixture = self.fixture();
        let first = control(test, "before", &mut fixture, events);
        // W3-10 T1: phase E runs on the masked password edit; the reset of
        // the control has masked it again after phase D's reveal.
        let masked = get(&fixture.ask(CMD_STATE, 0, "state"), "password_masked") == 1;
        say(
            test,
            format!("phase=E password_edit_masked_at_the_start={}", flag(masked)),
        );
        let phase = Phase {
            rounds: 20,
            letters_through_the_wait: false,
            ..phase("fixture-native", "E", Lead::Tab, Wait::Sleeps, 0)
        };
        let totals = run_rounds(test, &mut fixture, events, keyboard, phase);
        let last = control(test, "after", &mut fixture, events);
        (totals, first, last, masked)
    }
}

fn tab(test: &'static str, name: &'static str, wait: Wait, stall_ms: u64) -> Phase {
    phase(test, name, Lead::Tab, wait, stall_ms)
}

fn click(test: &'static str, name: &'static str, wait: Wait, stall_ms: u64) -> Phase {
    phase(test, name, Lead::Click, wait, stall_ms)
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_150_ms() {
    stall_test(tab(
        "stall-tab-sleeps-150",
        "tab_sleeps_150ms",
        Wait::Sleeps,
        150,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_200_ms() {
    stall_test(tab(
        "stall-tab-sleeps-200",
        "tab_sleeps_200ms",
        Wait::Sleeps,
        200,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_300_ms() {
    stall_test(tab(
        "stall-tab-sleeps-300",
        "tab_sleeps_300ms",
        Wait::Sleeps,
        300,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_600_ms() {
    stall_test(tab(
        "stall-tab-sleeps-600",
        "tab_sleeps_600ms",
        Wait::Sleeps,
        600,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_2000_ms() {
    stall_test(tab(
        "stall-tab-sleeps-2000",
        "tab_sleeps_2000ms",
        Wait::Sleeps,
        2000,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_150_ms() {
    stall_test(tab(
        "stall-tab-serves-150",
        "tab_serves_150ms",
        Wait::ServesSent,
        150,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_200_ms() {
    stall_test(tab(
        "stall-tab-serves-200",
        "tab_serves_200ms",
        Wait::ServesSent,
        200,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_300_ms() {
    stall_test(tab(
        "stall-tab-serves-300",
        "tab_serves_300ms",
        Wait::ServesSent,
        300,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_600_ms() {
    stall_test(tab(
        "stall-tab-serves-600",
        "tab_serves_600ms",
        Wait::ServesSent,
        600,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_2000_ms() {
    stall_test(tab(
        "stall-tab-serves-2000",
        "tab_serves_2000ms",
        Wait::ServesSent,
        2000,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_150_ms() {
    stall_test(click(
        "stall-click-sleeps-150",
        "click_sleeps_150ms",
        Wait::Sleeps,
        150,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_200_ms() {
    stall_test(click(
        "stall-click-sleeps-200",
        "click_sleeps_200ms",
        Wait::Sleeps,
        200,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_300_ms() {
    stall_test(click(
        "stall-click-sleeps-300",
        "click_sleeps_300ms",
        Wait::Sleeps,
        300,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_600_ms() {
    stall_test(click(
        "stall-click-sleeps-600",
        "click_sleeps_600ms",
        Wait::Sleeps,
        600,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_2000_ms() {
    stall_test(click(
        "stall-click-sleeps-2000",
        "click_sleeps_2000ms",
        Wait::Sleeps,
        2000,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_150_ms() {
    stall_test(click(
        "stall-click-serves-150",
        "click_serves_150ms",
        Wait::ServesSent,
        150,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_200_ms() {
    stall_test(click(
        "stall-click-serves-200",
        "click_serves_200ms",
        Wait::ServesSent,
        200,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_300_ms() {
    stall_test(click(
        "stall-click-serves-300",
        "click_serves_300ms",
        Wait::ServesSent,
        300,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_600_ms() {
    stall_test(click(
        "stall-click-serves-600",
        "click_serves_600ms",
        Wait::ServesSent,
        600,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_2000_ms() {
    stall_test(click(
        "stall-click-serves-2000",
        "click_serves_2000ms",
        Wait::ServesSent,
        2000,
    ));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_tab_and_a_letter_across_a_second_field_while_the_program_waits_after_the_first_tab() {
    for wait in [Wait::ServesSent, Wait::Sleeps] {
        stall_test(Phase {
            letters_through_the_wait: false,
            ..phase("stall-tab-tab", "tab_tab_300ms", Lead::TabTab, wait, 300)
        });
    }
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_and_letters_while_a_window_of_another_thread_comes_to_the_front() {
    for wait in [Wait::ServesSent, Wait::Sleeps] {
        stall_test(Phase {
            other_front: true,
            letters_through_the_wait: false,
            ..phase(
                "stall-other-front",
                "other_front_300ms",
                Lead::Tab,
                wait,
                300,
            )
        });
    }
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn enter_with_no_focus_move_then_five_letters_records_no_letter() {
    let test = "stall-enter";
    with_deadline(test, Duration::from_secs(300), move || {
        let totals = stall_phase(Phase {
            letters_through_the_wait: false,
            letters_after: 4,
            landing: Landing::Plain,
            ..phase(
                test,
                "enter_then_five_letters",
                Lead::Enter,
                Wait::Sleeps,
                0,
            )
        });
        assert_eq!(
            totals.letters_recorded, 0,
            "a letter after Enter was recorded"
        );
        assert!(
            totals.withheld_set > 0 || totals.counted == 0,
            "keys_withheld did not read yes"
        );
        assert_eq!(totals.late_moved, 0, "the source's late value moved");
        assert_eq!(
            totals.counted, totals.rounds,
            "INCONCLUSIVE: not every round counted"
        );
    });
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn a_second_tab_out_of_the_password_field_then_a_letter_records_nothing() {
    let test = "stall-tab-out";
    with_deadline(test, Duration::from_secs(300), move || {
        let totals = stall_phase(Phase {
            letters_through_the_wait: false,
            start_in_password: true,
            landing: Landing::Plain,
            ..phase(test, "tab_out_then_a_letter", Lead::Tab, Wait::Sleeps, 0)
        });
        assert_eq!(
            totals.letters_recorded, 0,
            "the letter after the Tab out was recorded"
        );
        assert_eq!(
            totals.tabs_recorded, 0,
            "the Tab out of the password field was recorded"
        );
        assert_eq!(
            totals.other_recorded, 0,
            "a key of another class was recorded"
        );
        assert_eq!(totals.late_moved, 0, "the source's late value moved");
        assert_eq!(
            totals.counted, totals.rounds,
            "INCONCLUSIVE: not every round counted"
        );
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
        assert!(
            start.is_ok(),
            "INCONCLUSIVE: the keyboard source did not start"
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

/// Five letters at the step of a person's typing, sent one by one.
fn five_letters() -> u32 {
    let mut accepted = 0;
    for _ in 0..5 {
        accepted += send(&[key_input(VK_A, false), key_input(VK_A, true)]);
        std::thread::sleep(LETTER_STEP);
    }
    accepted
}

/// One round of a phase on the native fixture: the fixture's own focus
/// move to `start`, then `act`, then five letters; it answers whether the
/// round counted (the window in front, the focus in `landing` at the end
/// and the five letters in that field) and the letters recorded.
fn native_round(
    (test, phase): (&str, &str),
    round: u32,
    fixture: &mut Fixture,
    events: &Events,
    (start, landing): (usize, &str),
    act: impl FnOnce(&mut Fixture) -> bool,
) -> (bool, u32) {
    let reset = fixture.ask(CMD_RESET, 0, "reset");
    let other = if start == FIELD_PLAIN {
        FIELD_PASSWORD
    } else {
        FIELD_PLAIN
    };
    let _ = fixture.ask(CMD_FOCUS, other, "focused");
    std::thread::sleep(Duration::from_millis(250));
    let _ = fixture.ask(CMD_FOCUS, start, "focused");
    let _ = events.drain_for(Duration::from_millis(400));
    let _ = seam::take();
    let _ = seam::take_marks();
    let acted = act(fixture);
    let accepted = five_letters();
    let _ = events.drain_for(Duration::from_millis(900));
    let decided = Decisions::of(&seam::take());
    let state = fixture.ask(CMD_STATE, 0, "state");
    let counted = get(&reset, "fg") == 1
        && acted
        && get(&state, &format!("focus_{landing}")) == 1
        && get(&state, &format!("letters_{landing}")) == 5;
    say(
        test,
        format!(
            "phase={phase} round={round} fg={} acted={} accepted={accepted} focus_{landing}={} \
             letters_{landing}={} letters_decided={} letters_recorded={} first_letter_rule={} counted={}",
            get(&reset, "fg"),
            u8::from(acted),
            get(&state, &format!("focus_{landing}")),
            get(&state, &format!("letters_{landing}")),
            decided.letters_recorded + decided.letters_withheld + decided.letters_late,
            decided.letters_recorded,
            decided.first_letter_rule,
            u8::from(counted),
        ),
    );
    (counted, decided.letters_recorded)
}

/// The phase of a native fixture: the controls before and after, `rounds`
/// rounds, and the asserts that no letter of a counted round is recorded.
fn native_phase(
    test: &'static str,
    phase: &'static str,
    start: usize,
    landing: &'static str,
    act: fn(&mut Fixture) -> bool,
) {
    with_deadline(test, Duration::from_secs(150), move || {
        source_built_by_test(test, "keyboard");
        let mut fixture = Fixture::open(test, false);
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let started = keyboard.begin(SourceContext::new(sink.clone()));
        assert!(
            started.is_ok(),
            "INCONCLUSIVE: the keyboard source did not start"
        );
        let _ = events.drain_for(Duration::from_millis(500));
        let before = control(test, "before", &mut fixture, &events);
        let mut counted = 0;
        let mut recorded = 0;
        for round in 0..5 {
            let (c, r) = native_round(
                (test, phase),
                round,
                &mut fixture,
                &events,
                (start, landing),
                act,
            );
            counted += u32::from(c);
            recorded += r;
        }
        let after = control(test, "after", &mut fixture, &events);
        let late = keyboard.health().counters.late_events;
        let _ = stop_counted(&mut keyboard, "keyboard");
        drop(fixture);
        say(
            test,
            format!("phase={phase} counted={counted} letters_recorded={recorded} control_before={before} control_after={after}"),
        );
        assert!(
            before == CONTROL_LETTERS && after == CONTROL_LETTERS,
            "a control recorded fewer than {CONTROL_LETTERS} letters of the plain field: the source did not record"
        );
        assert!(
            counted >= 4,
            "INCONCLUSIVE: {phase} counted {counted} rounds of 5"
        );
        assert_eq!(
            recorded, 0,
            "{phase}: a letter of a password field was recorded"
        );
        assert_eq!(late, 0, "the source's late value moved");
    });
}

/// W3-7 Q2 (h), native: the plain edit is masked while it has the focus,
/// and none of the five letters typed into it after the mask is recorded.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn an_edit_masked_while_it_has_the_focus_records_no_letter_after_the_mask() {
    native_phase("stall-mask", "mask", FIELD_PLAIN, "plain", |fixture| {
        get(&fixture.ask(CMD_MASK, FIELD_PLAIN, "masked"), "ok") == 1
    });
}

/// W3-7 O1 with W3-9 (3): with the password field focused, the program
/// raises a focus notice for the plain edit, which does not take the focus.
/// On the hosted runner the system delivers the gate no event for it (no
/// mark after the notice, round B), so the phase shows that a notice the
/// system does not deliver changes nothing: none of the five letters typed
/// into the password field after it is recorded. The rule that an event's
/// Clear clears no key is held through the focus callback by the library
/// test `a_focus_events_clear_from_the_callback_clears_no_key`.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn a_focus_notice_that_the_system_does_not_deliver_changes_nothing() {
    native_phase(
        "stall-stale-focus",
        "stale-focus",
        FIELD_PASSWORD,
        "password",
        |fixture| {
            let notified = get(
                &fixture.ask(CMD_NOTIFY_FOCUS, FIELD_PLAIN, "notified"),
                "ok",
            ) == 1;
            std::thread::sleep(Duration::from_millis(150));
            // Every mark after the notice is printed, with what its reading
            // saw of its element's password property, and whether a focus
            // event that read Clear reached the gate. A round of the hosted
            // runner of round B saw none (W3-7 O1).
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
        },
    );
}

/// W3-5 C6 with K2: a letter with no focus key and no click before it, and
/// the program moves the focus to the password field by itself after its
/// wait. It answers the letters recorded among those that landed in the
/// password field in counted rounds, and how far the late value moved.
pub(super) fn no_focus_key_and_the_program_moves_the_focus_itself() -> (u32, u64) {
    let test = "stall-self-focus";
    let mut recorded = 0;
    let mut late = 0;
    for (wait, stall_ms) in [
        (Wait::ServesSent, 150),
        (Wait::ServesSent, 300),
        (Wait::ServesSent, 600),
        (Wait::Sleeps, 300),
    ] {
        let totals = stall_phase(Phase {
            self_focus: true,
            letters_through_the_wait: false,
            ..phase(test, "self_focus", Lead::Nothing, wait, stall_ms)
        });
        recorded += totals.letters_recorded;
        late += totals.late_moved;
    }
    (recorded, late)
}

/// W3-5 C6, closed by the form: the program moves the focus to a plain
/// element, waits 200 ms without taking messages, moves it to the password
/// field and then takes the keys. No letter is recorded.
#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn the_program_moves_the_focus_in_two_steps_and_no_letter_is_recorded() {
    stall_test(Phase {
        two_step: true,
        letters_through_the_wait: false,
        ..phase("stall-two-step", "two_step", Lead::Nothing, Wait::Sleeps, 0)
    });
}
