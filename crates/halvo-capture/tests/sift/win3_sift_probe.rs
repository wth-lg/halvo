#![cfg(target_os = "windows")]
//! Scratch probe of verifier-interrogate-11. Never merged.
//! The native fixture as a process of its own with an ordinary message loop
//! (the helpers for the window, its activation and the sink are adversary 1's).
//! It prints counts, sizes, booleans and times alone.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_capture::event::{CaptureStreamEvent, Modality};
use halvo_capture::secure_input::windows::adv1_probe as probe;
use halvo_capture::secure_input::windows::{FieldState, PasswordGate};
use halvo_capture::sink::{channel, SinkWriter};
use halvo_capture::source::{CaptureSource, SourceContext};
use halvo_capture::windows::input::RawInputSource;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, SendInput, SetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY, VK_A,
    VK_MENU, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowRect,
    GetWindowTextLengthW, GetWindowThreadProcessId, IsDialogMessageW, MsgWaitForMultipleObjectsEx,
    PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    SetWindowTextW, ShowWindow, TranslateMessage, UnregisterClassW, WindowFromPoint, ES_PASSWORD,
    MSG, MWMO_INPUTAVAILABLE, PEEK_MESSAGE_REMOVE_TYPE, PM_NOREMOVE, PM_QS_SENDMESSAGE, PM_REMOVE,
    QS_SENDMESSAGE, SC_KEYMENU, SM_CXSCREEN, SM_CYSCREEN, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_KEYDOWN, WM_SYSCOMMAND, WNDCLASSW, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};

fn say(message: impl AsRef<str>) {
    println!("SIFT {}", message.as_ref());
}

fn with_deadline<F>(test: &'static str, limit: Duration, body: F)
where
    F: FnOnce() + Send + 'static,
{
    let (done, finished) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name(format!("sift-{test}"))
        .spawn(move || {
            body();
            let _ = done.send(());
        })
        .expect("spawn the test body");
    match finished.recv_timeout(limit) {
        Ok(()) => worker.join().expect("test body finished"),
        Err(RecvTimeoutError::Timeout) => {
            say(format!(
                "test={test} result=TIMEOUT limit_secs={}",
                limit.as_secs()
            ));
            panic!("{test} did not finish within {limit:?}");
        }
        Err(RecvTimeoutError::Disconnected) => {
            if let Err(payload) = worker.join() {
                std::panic::resume_unwind(payload);
            }
        }
    }
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

fn send(inputs: &[INPUT]) -> u32 {
    // SAFETY: `inputs` is a live slice of INPUT values and the size argument
    // is the size of one INPUT, as SendInput requires.
    unsafe { SendInput(inputs, core::mem::size_of::<INPUT>() as i32) }
}

fn foreground_window() -> HWND {
    // SAFETY: GetForegroundWindow takes no argument and returns a handle value.
    unsafe { GetForegroundWindow() }
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
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("INCONCLUSIVE: the sink closed")
                }
            }
        }
        events
    }
}

fn forward(writer: SinkWriter) -> Events {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("sift-sink-forward".to_string())
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

unsafe extern "system" fn probe_wnd_proc(
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

struct ProbeWindow {
    hwnd: HWND,
    class: HSTRING,
    hinstance: HINSTANCE,
}

impl ProbeWindow {
    fn create() -> ProbeWindow {
        let class = HSTRING::from("HalvoSiftProbeFields");
        let title = HSTRING::from("halvo-sift-probe-fields");
        // SAFETY: a null module name asks for the handle of this executable.
        let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .expect("GetModuleHandleW")
            .into();
        let wnd_class = WNDCLASSW {
            lpfnWndProc: Some(probe_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `wnd_class` points at `class`, which the returned value
        // holds until its Drop unregisters the class.
        assert!(
            unsafe { RegisterClassW(&wnd_class) } != 0,
            "INCONCLUSIVE: RegisterClassW"
        );
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
        ProbeWindow {
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

    fn activate(&self) -> bool {
        // SAFETY: the three calls take this thread's window handle by value.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = BringWindowToTop(self.hwnd);
            let _ = SetForegroundWindow(self.hwnd);
        }
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return true;
        }
        send(&[key_input(VK_MENU, false)]);
        // SAFETY: SetForegroundWindow takes a window handle by value.
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
        }
        send(&[key_input(VK_MENU, true)]);
        pump_thread(Duration::from_millis(200));
        if self.is_foreground() {
            return true;
        }
        let current_thread = {
            let mut pid = 0u32;
            // SAFETY: `pid` is a live u32 that the call writes into.
            unsafe { GetWindowThreadProcessId(foreground_window(), Some(&mut pid)) }
        };
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
            return true;
        }
        // SAFETY: ShowWindow takes this thread's window handle by value.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
        }
        pump_thread(Duration::from_millis(200));
        self.is_foreground()
    }
}

impl Drop for ProbeWindow {
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

fn focus_on(hwnd: HWND) -> bool {
    // SAFETY: SetFocus and GetFocus take and return handle values of this
    // thread's windows.
    unsafe {
        let _ = SetFocus(Some(hwnd));
        GetFocus() == hwnd
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

const WM_SIFT: u32 = WM_APP + 7;
const CMD_RESET: usize = 1;
const CMD_STATE: usize = 2;
const CMD_ROUND: usize = 3;
const CMD_QUIT: usize = 4;
const ARG_SELF_FOCUS: usize = 0x1_0000;
const ARG_AIM: usize = 0x2_0000;
const ARG_SERVE_SENT: usize = 0x4_0000;

fn post(window: usize, code: usize, arg: usize) {
    // SAFETY: PostMessageW takes a window handle and plain values; a window
    // that is gone makes the call fail and changes nothing.
    unsafe {
        let _ = PostMessageW(
            Some(HWND(window as *mut core::ffi::c_void)),
            WM_SIFT,
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

fn flag(value: bool) -> u8 {
    u8::from(value)
}

/// The fixture process: a window with a plain edit and a password edit, the
/// dialog's own message loop on GetMessageW, commands as posted messages.
fn child_main() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(900));
        std::process::exit(3);
    });
    let window = ProbeWindow::create();
    let plain = window.edit(WS_TABSTOP, 10);
    let password = window.edit(WS_TABSTOP | WINDOW_STYLE(ES_PASSWORD as u32), 44);
    pump_thread(Duration::from_millis(300));
    let foreground = window.activate();
    focus_on(plain);
    pump_thread(Duration::from_millis(300));
    let target = window.hwnd.0 as usize;
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else {
                break;
            };
            let mut parts = line.split_whitespace();
            let code = parts
                .next()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            let arg = parts
                .next()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            post(target, code, arg);
            if code == CMD_QUIT {
                return;
            }
        }
        post(target, CMD_QUIT, 0);
    });
    println!("SIFTC ready fg={}", flag(foreground));
    let mut letters = [0u32; 3];
    let mut tabs = [0u32; 3];
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that GetMessageW fills.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        if msg.message == WM_SIFT && msg.hwnd == window.hwnd {
            // SAFETY: GetFocus takes no argument and returns a handle value.
            let focus = unsafe { GetFocus() };
            match msg.wParam.0 {
                CMD_RESET => {
                    let reactivated = !window.is_foreground();
                    if reactivated {
                        window.activate();
                    }
                    clear_text(plain);
                    clear_text(password);
                    let focused = focus_on(plain);
                    letters = [0; 3];
                    tabs = [0; 3];
                    println!(
                        "SIFTC reset fg={} focus_plain={} reactivated={}",
                        flag(window.is_foreground()),
                        flag(focused),
                        flag(reactivated)
                    );
                }
                CMD_STATE => {
                    println!(
                        "SIFTC state fg={} focus_plain={} focus_password={} plain_len={} password_len={} \
                         letters_plain={} letters_password={} letters_other={} tabs_plain={} tabs_password={} tabs_other={}",
                        flag(window.is_foreground()),
                        flag(focus == plain),
                        flag(focus == password),
                        text_len(plain),
                        text_len(password),
                        letters[0],
                        letters[1],
                        letters[2],
                        tabs[0],
                        tabs[1],
                        tabs[2]
                    );
                }
                CMD_ROUND => {
                    let arg = msg.lParam.0 as usize;
                    let stall_ms = (arg & 0xFFFF) as u64;
                    let aimed = arg & ARG_AIM == 0 || aim_at(password);
                    println!(
                        "SIFTC ack fg={} focus_plain={} plain_len={} password_len={} aimed={}",
                        flag(window.is_foreground()),
                        flag(focus == plain),
                        text_len(plain),
                        text_len(password),
                        flag(aimed)
                    );
                    stall(stall_ms, arg & ARG_SERVE_SENT != 0);
                    let (plain_len, password_len) = (text_len(plain), text_len(password));
                    if arg & ARG_SELF_FOCUS != 0 {
                        focus_on(password);
                    }
                    println!("SIFTC woke plain_len={plain_len} password_len={password_len}");
                }
                _ => {
                    // SAFETY: PostQuitMessage takes an exit code.
                    unsafe { PostQuitMessage(0) };
                }
            }
            continue;
        }
        if msg.message == WM_KEYDOWN {
            let to = if msg.hwnd == plain {
                0
            } else if msg.hwnd == password {
                1
            } else {
                2
            };
            let key = msg.wParam.0 as u16;
            if key == VK_A.0 {
                letters[to] += 1;
            } else if key == VK_TAB.0 {
                tabs[to] += 1;
            }
        }
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

#[test]
#[ignore = "probe: the fixture process of the sift probe; does nothing unless SIFT_CHILD is set"]
fn sift_child_fixture() {
    if std::env::var_os("SIFT_CHILD").is_none() {
        return;
    }
    child_main();
}

type Fields = HashMap<String, i64>;

fn parse(line: &str) -> Option<(String, Fields)> {
    // With --nocapture the first line of a test shares the line of its name.
    let rest = &line[line.find("SIFTC ")? + 6..];
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

struct Fixture {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<(String, Fields)>,
}

impl Fixture {
    fn open() -> Fixture {
        let exe = std::env::current_exe().expect("the test executable");
        let mut child = Command::new(exe)
            .args([
                "--ignored",
                "--exact",
                "sift_child_fixture",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("SIFT_CHILD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("INCONCLUSIVE: the fixture process did not start");
        let stdin = child.stdin.take().expect("the fixture's stdin");
        let stdout = child.stdout.take().expect("the fixture's stdout");
        let (tx, lines) = mpsc::channel();
        std::thread::Builder::new()
            .name("sift-fixture-lines".to_string())
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
        let fixture = Fixture {
            child,
            stdin,
            lines,
        };
        let ready = fixture.expect("ready", Duration::from_secs(40));
        say(format!(
            "fixture_process_started=true fixture_window_foreground={}",
            get(&ready, "fg")
        ));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keys {
    TabLetter,
    LetterOnly,
    ClickLetter,
}

#[derive(Debug, Clone, Copy)]
struct Phase {
    name: &'static str,
    keys: Keys,
    stall_ms: u64,
    self_focus: bool,
    serve_sent: bool,
}

const fn phase(name: &'static str, keys: Keys, stall_ms: u64) -> Phase {
    Phase {
        name,
        keys,
        stall_ms,
        self_focus: false,
        serve_sent: false,
    }
}

#[derive(Debug, Default)]
struct Totals {
    rounds: u32,
    counted: u32,
    letters_recorded: u32,
    letters_in_password: u32,
    leaks: u32,
    letters_late: u32,
    letters_active: u32,
    letters_unknown: u32,
    focus_keys_recorded: u32,
    late_events: u64,
    keys_withheld_set: u32,
}

fn rel(at: u64, base: u64) -> i64 {
    i64::try_from(at).unwrap_or(i64::MAX) - i64::try_from(base).unwrap_or(i64::MAX)
}

fn state_code(state: Option<FieldState>) -> i64 {
    match state {
        Some(FieldState::Clear) => 0,
        Some(FieldState::Password) => 1,
        Some(FieldState::Unknown) => 2,
        None => -1,
    }
}

type Entry = (u8, u64, u32, u32);

/// Runs one phase and answers the rounds that counted with a recorded letter
/// that landed in the password field.
fn run_phase(
    fixture: &mut Fixture,
    events: &Events,
    keyboard: &RawInputSource,
    phase: Phase,
    rounds: u32,
) -> u32 {
    let mut totals = Totals::default();
    let limit = Duration::from_secs(10);
    for round in 0..rounds {
        let _ = probe::take();
        fixture.tell(CMD_RESET, 0);
        let reset = fixture.expect("reset", limit);
        let _ = events.drain_for(Duration::from_millis(400));
        let before_health = keyboard.health().counters;
        let gate_before = state_code(keyboard.field_state());
        let mut arg = usize::try_from(phase.stall_ms).unwrap_or(0) & 0xFFFF;
        if phase.self_focus {
            arg |= ARG_SELF_FOCUS;
        }
        if phase.keys == Keys::ClickLetter {
            arg |= ARG_AIM;
        }
        if phase.serve_sent {
            arg |= ARG_SERVE_SENT;
        }
        let mut inputs: Vec<INPUT> = Vec::new();
        match phase.keys {
            Keys::TabLetter => {
                inputs.push(key_input(VK_TAB, false));
                inputs.push(key_input(VK_TAB, true));
            }
            Keys::LetterOnly => {}
            Keys::ClickLetter => {
                inputs.push(mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN));
                inputs.push(mouse_input(0, 0, MOUSEEVENTF_LEFTUP));
            }
        }
        inputs.push(key_input(VK_A, false));
        inputs.push(key_input(VK_A, true));
        fixture.tell(CMD_ROUND, arg);
        let ack = fixture.expect("ack", limit);
        let sent_at = probe::micros();
        let accepted = if get(&ack, "aimed") == 1 {
            send(&inputs)
        } else {
            0
        };
        let seen = events.drain_for(Duration::from_millis(phase.stall_ms + 700));
        let woke = fixture.expect("woke", limit);
        fixture.tell(CMD_STATE, 0);
        let after = fixture.expect("state", limit);
        let after_health = keyboard.health().counters;
        let log: Vec<Entry> = probe::take();
        let keystroke_events = seen
            .iter()
            .filter(|e| e.modality == Modality::Keystroke)
            .count();

        let arrivals: Vec<&Entry> = log.iter().filter(|e| e.0 == 8).collect();
        let decisions: Vec<&Entry> = log
            .iter()
            .filter(|e| matches!(e.0, 5 | 6 | 7 | 10 | 11))
            .collect();
        let letter_at = arrivals.iter().position(|e| e.3 & 1 == 0);
        let focus_key_at = arrivals.iter().position(|e| e.3 & 1 == 1);
        let letter_decision = letter_at.and_then(|i| decisions.get(i)).copied();
        let focus_key_decision = focus_key_at.and_then(|i| decisions.get(i)).copied();
        let latch = log
            .iter()
            .find(|e| matches!(e.0, 3 | 4) && e.1 >= sent_at)
            .map(|e| e.1);
        let read = latch.and_then(|latch| log.iter().find(|e| e.0 == 2 && e.1 >= latch));
        let read_began =
            latch.and_then(|latch| log.iter().find(|e| e.0 == 12 && e.1 >= latch).map(|e| e.1));
        let marks_before: Vec<&Entry> = log
            .iter()
            .filter(|e| matches!(e.0, 1 | 2) && e.1 < sent_at)
            .collect();
        let first_field_after = log.iter().find(|e| e.0 == 1 && e.1 >= sent_at);
        let first_password_mark = log
            .iter()
            .find(|e| matches!(e.0, 1 | 2) && e.3 == 1 && e.1 >= sent_at)
            .map(|e| e.1);
        let or = |v: Option<i64>| v.map_or("none".to_string(), |v| v.to_string());
        let (expected_inputs, expected_downs) = match phase.keys {
            Keys::TabLetter => (4, 2),
            Keys::ClickLetter => (4, 1),
            Keys::LetterOnly => (2, 1),
        };
        let counted = get(&reset, "fg") == 1
            && get(&reset, "reactivated") == 0
            && get(&ack, "fg") == 1
            && get(&ack, "focus_plain") == 1
            && get(&ack, "password_len") == 0
            && get(&ack, "aimed") == 1
            && accepted == expected_inputs
            && arrivals.len() == expected_downs
            && decisions.len() == arrivals.len()
            && get(&woke, "password_len") == 0
            && letter_decision.is_some();
        let in_password = get(&after, "letters_password") == 1;
        let recorded = letter_decision.is_some_and(|e| e.0 == 5);
        let leak = counted && recorded && in_password;
        let late_delta = after_health.late_events - before_health.late_events;
        let withheld_set = !before_health.keys_withheld && after_health.keys_withheld;
        say(format!(
            "phase={} round={round} counted={} leak={} letter_decision={} letter_age_at_decision_ms={} focus_key_decision={} \
             letters_to_plain={} letters_to_password={} letters_elsewhere={} tabs_to_plain={} password_len_at_ack={} \
             password_len_at_wake={} password_len_after={} plain_len_after={} focus_password_after={} fg_reset={} reactivated={} \
             fg_ack={} focus_plain_ack={} fg_after={} aimed={} accepted={accepted} keystroke_events={keystroke_events} \
             key_downs_seen={} decisions_seen={} letter_read_as_password_edit={} gate_before_send={gate_before} \
             marks_before_send={} last_mark_before_send={} latch_to_read_began_us={} latch_to_read_mark_us={} read_state={} \
             send_to_first_focus_event_us={} first_focus_event_state={} send_to_first_password_mark_us={} \
             late_events_delta={late_delta} keys_withheld_before={} keys_withheld_after={}",
            phase.name,
            flag(counted),
            flag(leak),
            or(letter_decision.map(|e| i64::from(e.0))),
            or(letter_decision.map(|e| i64::from(e.3.wrapping_sub(e.2)))),
            or(focus_key_decision.map(|e| i64::from(e.0))),
            get(&after, "letters_plain"),
            get(&after, "letters_password"),
            get(&after, "letters_other"),
            get(&after, "tabs_plain"),
            get(&ack, "password_len"),
            get(&woke, "password_len"),
            get(&after, "password_len"),
            get(&after, "plain_len"),
            get(&after, "focus_password"),
            get(&reset, "fg"),
            get(&reset, "reactivated"),
            get(&ack, "fg"),
            get(&ack, "focus_plain"),
            get(&after, "fg"),
            get(&ack, "aimed"),
            arrivals.len(),
            decisions.len(),
            or(letter_at.map(|i| i64::from((arrivals[i].3 >> 1) & 1))),
            marks_before.len(),
            or(marks_before.last().map(|e| i64::from(e.3))),
            or(latch.zip(read_began).map(|(l, r)| rel(r, l))),
            or(latch.zip(read).map(|(l, r)| rel(r.1, l))),
            or(read.map(|e| i64::from(e.3))),
            or(first_field_after.map(|e| rel(e.1, sent_at))),
            or(first_field_after.map(|e| i64::from(e.3))),
            or(first_password_mark.map(|p| rel(p, sent_at))),
            flag(before_health.keys_withheld),
            flag(after_health.keys_withheld),
        ));
        let raw: Vec<String> = log
            .iter()
            .take(120)
            .map(|e| format!("{}@{}:{}:{}", e.0, rel(e.1, sent_at), e.2, e.3))
            .collect();
        say(format!(
            "phase={} round={round} log_entries={} log={}",
            phase.name,
            log.len(),
            raw.join(",")
        ));
        totals.rounds += 1;
        totals.late_events += late_delta;
        totals.keys_withheld_set += u32::from(withheld_set);
        if counted {
            totals.counted += 1;
            totals.letters_in_password += u32::from(in_password);
            totals.leaks += u32::from(leak);
            totals.focus_keys_recorded += u32::from(focus_key_decision.is_some_and(|e| e.0 == 5));
            match letter_decision.map(|e| e.0) {
                Some(5) => totals.letters_recorded += 1,
                Some(6) => totals.letters_late += 1,
                Some(7) => totals.letters_active += 1,
                Some(10) => totals.letters_unknown += 1,
                _ => {}
            }
        }
    }
    say(format!(
        "summary phase={} keys={:?} stall_ms={} self_focus={} serve_sent={} rounds={} counted_rounds={} \
         letters_recorded={} letters_in_password={} recorded_and_in_password={} letters_late={} letters_withheld_active={} \
         letters_withheld_unknown={} focus_keys_recorded={} late_events_delta={} rounds_that_set_keys_withheld={}",
        phase.name,
        phase.keys,
        phase.stall_ms,
        flag(phase.self_focus),
        flag(phase.serve_sent),
        totals.rounds,
        totals.counted,
        totals.letters_recorded,
        totals.letters_in_password,
        totals.leaks,
        totals.letters_late,
        totals.letters_active,
        totals.letters_unknown,
        totals.focus_keys_recorded,
        totals.late_events,
        totals.keys_withheld_set,
    ));
    totals.leaks
}

fn run_phases(test: &'static str, phases: Vec<Phase>, rounds: u32) {
    let mut fixture = Fixture::open();
    let (sink, writer) = channel(8192);
    let events = forward(writer);
    let mut keyboard = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
    let start = keyboard.begin(SourceContext::new(sink.clone()));
    say(format!("test={test} keyboard_start_ok={}", start.is_ok()));
    let mut leaks = 0;
    if start.is_ok() {
        let _ = events.drain_for(Duration::from_millis(500));
        for phase in phases {
            leaks += run_phase(&mut fixture, &events, &keyboard, phase, rounds);
        }
    }
    let stopped = keyboard.shutdown();
    say(format!(
        "test={test} keyboard_stop_ok={} recorded_letters_that_landed_in_the_password_field={leaks}",
        stopped.is_ok()
    ));
    drop(fixture);
    assert!(
        start.is_ok(),
        "INCONCLUSIVE: the keyboard source did not start"
    );
    assert_eq!(
        leaks, 0,
        "a letter was recorded in a counted round and landed in the password field"
    );
}

#[test]
#[ignore = "probe: needs an interactive Windows desktop"]
fn sift_a_tab_then_a_letter_against_a_stalled_program() {
    with_deadline("tab", Duration::from_secs(420), || {
        run_phases(
            "tab",
            vec![
                phase("P", Keys::LetterOnly, 0),
                phase("T0", Keys::TabLetter, 0),
                phase("T150", Keys::TabLetter, 150),
                phase("T200", Keys::TabLetter, 200),
                phase("T300", Keys::TabLetter, 300),
                phase("T600", Keys::TabLetter, 600),
            ],
            20,
        );
    });
}

#[test]
#[ignore = "probe: needs an interactive Windows desktop"]
fn sift_b_a_tab_then_a_letter_against_a_program_that_serves_sent_messages() {
    with_deadline("tab-serve", Duration::from_secs(300), || {
        let serve = |name, stall_ms| Phase {
            serve_sent: true,
            ..phase(name, Keys::TabLetter, stall_ms)
        };
        run_phases(
            "tab-serve",
            vec![
                serve("TS150", 150),
                serve("TS300", 300),
                serve("TS600", 600),
            ],
            20,
        );
    });
}

#[test]
#[ignore = "probe: needs an interactive Windows desktop"]
fn sift_c_a_click_then_a_letter_against_a_stalled_program() {
    with_deadline("click", Duration::from_secs(360), || {
        run_phases(
            "click",
            vec![
                phase("K0", Keys::ClickLetter, 0),
                phase("K150", Keys::ClickLetter, 150),
                phase("K300", Keys::ClickLetter, 300),
                phase("K600", Keys::ClickLetter, 600),
            ],
            20,
        );
    });
}

#[test]
#[ignore = "probe: needs an interactive Windows desktop"]
fn sift_d_a_letter_and_then_the_program_moves_the_focus_itself() {
    with_deadline("self-focus", Duration::from_secs(300), || {
        let own = |name, stall_ms| Phase {
            self_focus: true,
            ..phase(name, Keys::LetterOnly, stall_ms)
        };
        run_phases(
            "self-focus",
            vec![own("N150", 150), own("N300", 300), own("N600", 600)],
            20,
        );
    });
}
