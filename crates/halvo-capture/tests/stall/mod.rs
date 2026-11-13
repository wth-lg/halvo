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
    stop_counted, with_deadline, Events,
};

const FIXTURE_ENV: &str = "HALVO_VERIFY_STALL_FIXTURE";
const FIXTURE_TEST: &str = "stall::helper_stall_fixture";

const WM_STALL: u32 = WM_APP + 7;
const CMD_RESET: usize = 1;
const CMD_STATE: usize = 2;
const CMD_ROUND: usize = 3;
const CMD_QUIT: usize = 4;
const ARG_SELF_FOCUS: usize = 0x1_0000;
const ARG_AIM: usize = 0x2_0000;
const ARG_SERVE_SENT: usize = 0x4_0000;

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
        let class = HSTRING::from("HalvoVerifyStallFixture");
        let title = HSTRING::from("halvo-verify-stall-fixture");
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

/// The fixture process: a window with a plain edit and a password edit, the
/// dialog's own message loop on GetMessageW, commands as posted messages. It
/// counts the key-downs of the letter and of Tab by the control that each was
/// addressed to, and prints counts, lengths and flags alone.
fn fixture_main() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(900));
        std::process::exit(3);
    });
    let window = FixtureWindow::create();
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
    println!("STALLC ready fg={}", flag(foreground));
    let mut letters = [0u32; 3];
    let mut tabs = [0u32; 3];
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that GetMessageW fills.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            break;
        }
        if msg.message == WM_STALL && msg.hwnd == window.hwnd {
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
                        "STALLC reset fg={} focus_plain={} reactivated={}",
                        flag(window.is_foreground()),
                        flag(focused),
                        flag(reactivated)
                    );
                }
                CMD_STATE => {
                    println!(
                        "STALLC state fg={} focus_plain={} focus_password={} plain_len={} \
                         password_len={} letters_plain={} letters_password={} letters_other={} \
                         tabs_plain={} tabs_password={} tabs_other={}",
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
                        "STALLC ack fg={} focus_plain={} plain_len={} password_len={} aimed={}",
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
                    println!("STALLC woke plain_len={plain_len} password_len={password_len}");
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

struct Fixture {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<(String, Fields)>,
}

impl Fixture {
    fn open(test: &str) -> Fixture {
        let exe = std::env::current_exe().expect("the test executable");
        let mut child = Command::new(exe)
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
            .stderr(Stdio::inherit())
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
        let fixture = Fixture {
            child,
            stdin,
            lines,
        };
        let ready = fixture.expect("ready", Duration::from_secs(40));
        say(
            test,
            format!(
                "fixture_process_started=true fixture_window_foreground={}",
                get(&ready, "fg")
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

// ----------------------------------------------------------------- the phases

/// What comes before the first letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lead {
    Tab,
    Click,
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

#[derive(Debug, Clone, Copy)]
struct Phase {
    test: &'static str,
    lead: Lead,
    wait: Wait,
    stall_ms: u64,
    rounds: u32,
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
    lost_at_stop: u32,
}

impl Decisions {
    fn of(decided: &[seam::Decided]) -> Decisions {
        let mut d = Decisions::default();
        for key in decided {
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
    say(
        test,
        format!(
            "control={which} fg={} focus_plain={} letters_sent={CONTROL_LETTERS} accepted={accepted} \
             letters_plain={} keystroke_events={} letters_recorded={}",
            get(&reset, "fg"),
            get(&reset, "focus_plain"),
            get(&state, "letters_plain"),
            seen.iter()
                .filter(|e| e.modality == Modality::Keystroke)
                .count(),
            decided.letters_recorded,
        ),
    );
    decided.letters_recorded
}

#[derive(Debug, Default)]
struct Totals {
    rounds: u32,
    counted: u32,
    letters_sent: u32,
    letters_in_password: u32,
    letters_recorded: u32,
    tabs_recorded: u32,
    other_recorded: u32,
    late_moved: u64,
    withheld_set: u32,
    rounds_with_a_recorded_letter: u32,
}

impl Totals {
    fn add(&mut self, other: Totals) {
        self.rounds += other.rounds;
        self.counted += other.counted;
        self.letters_sent += other.letters_sent;
        self.letters_in_password += other.letters_in_password;
        self.letters_recorded += other.letters_recorded;
        self.tabs_recorded += other.tabs_recorded;
        self.other_recorded += other.other_recorded;
        self.late_moved += other.late_moved;
        self.withheld_set += other.withheld_set;
        self.rounds_with_a_recorded_letter += other.rounds_with_a_recorded_letter;
    }
}

/// Runs one stall phase and answers its totals.
fn run_rounds(
    test: &str,
    fixture: &mut Fixture,
    events: &Events,
    keyboard: &RawInputSource,
    phase: Phase,
) -> Totals {
    let mut totals = Totals::default();
    for round in 0..phase.rounds {
        let reset = fixture.ask(CMD_RESET, 0, "reset");
        let _ = events.drain_for(Duration::from_millis(400));
        let _ = seam::take();
        let before = keyboard.health().counters;
        let mut arg = usize::try_from(phase.stall_ms).unwrap_or(0) & 0xFFFF;
        if phase.lead == Lead::Click {
            arg |= ARG_AIM;
        }
        if phase.wait == Wait::ServesSent {
            arg |= ARG_SERVE_SENT;
        }
        let mut burst: Vec<INPUT> = Vec::new();
        match phase.lead {
            Lead::Tab => {
                burst.push(key_input(VK_TAB, false));
                burst.push(key_input(VK_TAB, true));
            }
            Lead::Click => {
                burst.push(mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN));
                burst.push(mouse_input(0, 0, MOUSEEVENTF_LEFTUP));
            }
        }
        burst.push(key_input(VK_A, false));
        burst.push(key_input(VK_A, true));
        let ack = fixture.ask(CMD_ROUND, arg, "ack");
        let sent_at = Instant::now();
        let mut letters_sent = 0u32;
        let mut accepted = 0u32;
        let mut inputs = 0u32;
        if get(&ack, "aimed") == 1 {
            accepted += send(&burst);
            inputs += burst.len() as u32;
            letters_sent += 1;
            let wait = Duration::from_millis(phase.stall_ms);
            while sent_at.elapsed() + LETTER_STEP < wait {
                std::thread::sleep(LETTER_STEP);
                accepted += send(&[key_input(VK_A, false), key_input(VK_A, true)]);
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
        let letters_in_password = u32::try_from(get(&state, "letters_password")).unwrap_or(0);
        let counted = get(&reset, "fg") == 1
            && get(&reset, "reactivated") == 0
            && get(&ack, "fg") == 1
            && get(&ack, "focus_plain") == 1
            && get(&ack, "password_len") == 0
            && get(&ack, "aimed") == 1
            && accepted == inputs
            && get(&woke, "password_len") == 0
            && get(&state, "fg") == 1
            && letters_in_password == letters_sent
            && get(&state, "letters_plain") == 0;
        let late_moved = after.late_events.saturating_sub(before.late_events);
        let withheld_set = !before.keys_withheld && after.keys_withheld;
        say(
            test,
            format!(
                "phase={} round={round} counted={} letters_sent={letters_sent} \
                 letters_recorded={} tabs_recorded={} other_recorded={} recorded_by_the_seam={} \
                 keystroke_events={keystroke_events} \
                 letters_withheld={} letters_late={} tabs_withheld={} lost_at_stop={} \
                 fg_at_send={} focus_plain_at_send={} plain_len_at_send={} password_len_at_send={} \
                 plain_len_at_wake={} password_len_at_wake={} plain_len_after={} password_len_after={} \
                 letters_plain={} letters_password={} letters_other={} tabs_plain={} tabs_password={} \
                 fg_after={} reactivated={} aimed={} late_events_moved_by={late_moved} \
                 keys_withheld_before={} keys_withheld_after={}",
                phase_name(phase),
                flag(counted),
                decided.letters_recorded,
                decided.tabs_recorded,
                decided.other_recorded,
                decided.recorded(),
                decided.letters_withheld,
                decided.letters_late,
                decided.tabs_withheld,
                decided.lost_at_stop,
                get(&ack, "fg"),
                get(&ack, "focus_plain"),
                get(&ack, "plain_len"),
                get(&ack, "password_len"),
                get(&woke, "plain_len"),
                get(&woke, "password_len"),
                get(&state, "plain_len"),
                get(&state, "password_len"),
                get(&state, "letters_plain"),
                get(&state, "letters_password"),
                get(&state, "letters_other"),
                get(&state, "tabs_plain"),
                get(&state, "tabs_password"),
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
            totals.letters_in_password += letters_in_password;
            totals.letters_recorded += decided.letters_recorded;
            totals.tabs_recorded += decided.tabs_recorded;
            totals.other_recorded += decided.other_recorded;
            totals.rounds_with_a_recorded_letter += u32::from(decided.letters_recorded > 0);
        }
    }
    totals
}

fn phase_name(phase: Phase) -> String {
    let lead = match phase.lead {
        Lead::Tab => "tab",
        Lead::Click => "click",
    };
    let wait = match phase.wait {
        Wait::Sleeps => "sleeps",
        Wait::ServesSent => "serves_sent",
    };
    format!("{lead}_{wait}_{}ms", phase.stall_ms)
}

/// One stall test: the fixture process, the keyboard source, the control
/// before the first round, the rounds, the control after the last.
fn stall_test(phase: Phase) {
    let test = phase.test;
    with_deadline(test, Duration::from_secs(240), move || {
        source_built_by_test(test, "keyboard");
        let mut fixture = Fixture::open(test);
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
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
                "summary phase={} rounds={} counted_rounds={} letters_sent={} letters_in_password={} \
                 letters_recorded={} rounds_with_a_recorded_letter={} tabs_recorded={} other_recorded={} \
                 late_events_moved_by={} rounds_that_set_keys_withheld={} control_before={first} control_after={last}",
                phase_name(phase),
                totals.rounds,
                totals.counted,
                totals.letters_sent,
                totals.letters_in_password,
                totals.letters_recorded,
                totals.rounds_with_a_recorded_letter,
                totals.tabs_recorded,
                totals.other_recorded,
                totals.late_moved,
                totals.withheld_set,
            ),
        );
        assert_eq!(
            totals.letters_recorded, 0,
            "a letter typed ahead of the stalled program was recorded in a counted round and landed in the password field"
        );
        assert_eq!(
            totals.other_recorded, 0,
            "a key of another class than Tab was recorded in a counted round"
        );
        assert!(
            first > 0 && last > 0,
            "a control recorded no letter, so the source did not show that it records"
        );
        assert_eq!(
            totals.counted, totals.rounds,
            "INCONCLUSIVE: not every round counted"
        );
    });
}

const fn tab(test: &'static str, wait: Wait, stall_ms: u64, rounds: u32) -> Phase {
    Phase {
        test,
        lead: Lead::Tab,
        wait,
        stall_ms,
        rounds,
    }
}

const fn click(test: &'static str, wait: Wait, stall_ms: u64, rounds: u32) -> Phase {
    Phase {
        test,
        lead: Lead::Click,
        wait,
        stall_ms,
        rounds,
    }
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_150_ms() {
    stall_test(tab("stall-tab-sleeps-150", Wait::Sleeps, 150, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_200_ms() {
    stall_test(tab("stall-tab-sleeps-200", Wait::Sleeps, 200, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_300_ms() {
    stall_test(tab("stall-tab-sleeps-300", Wait::Sleeps, 300, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_600_ms() {
    stall_test(tab("stall-tab-sleeps-600", Wait::Sleeps, 600, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_sleeps_2000_ms() {
    stall_test(tab("stall-tab-sleeps-2000", Wait::Sleeps, 2000, 5));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_150_ms() {
    stall_test(tab("stall-tab-serves-150", Wait::ServesSent, 150, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_200_ms() {
    stall_test(tab("stall-tab-serves-200", Wait::ServesSent, 200, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_300_ms() {
    stall_test(tab("stall-tab-serves-300", Wait::ServesSent, 300, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_600_ms() {
    stall_test(tab("stall-tab-serves-600", Wait::ServesSent, 600, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn tab_then_letters_while_the_program_serves_sent_messages_2000_ms() {
    stall_test(tab("stall-tab-serves-2000", Wait::ServesSent, 2000, 5));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_150_ms() {
    stall_test(click("stall-click-sleeps-150", Wait::Sleeps, 150, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_200_ms() {
    stall_test(click("stall-click-sleeps-200", Wait::Sleeps, 200, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_300_ms() {
    stall_test(click("stall-click-sleeps-300", Wait::Sleeps, 300, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_600_ms() {
    stall_test(click("stall-click-sleeps-600", Wait::Sleeps, 600, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_sleeps_2000_ms() {
    stall_test(click("stall-click-sleeps-2000", Wait::Sleeps, 2000, 5));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_150_ms() {
    stall_test(click("stall-click-serves-150", Wait::ServesSent, 150, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_200_ms() {
    stall_test(click("stall-click-serves-200", Wait::ServesSent, 200, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_300_ms() {
    stall_test(click("stall-click-serves-300", Wait::ServesSent, 300, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_600_ms() {
    stall_test(click("stall-click-serves-600", Wait::ServesSent, 600, 20));
}

#[test]
#[ignore = "functional: needs an interactive Windows desktop"]
fn click_then_letters_while_the_program_serves_sent_messages_2000_ms() {
    stall_test(click("stall-click-serves-2000", Wait::ServesSent, 2000, 5));
}
