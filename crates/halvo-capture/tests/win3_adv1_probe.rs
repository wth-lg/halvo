#![cfg(target_os = "windows")]
//! Scratch probe of interrogate-win3-adversary-1. Never merged.
//! It prints counts, sizes, booleans and times alone.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_capture::event::{CaptureStreamEvent, Modality};
use halvo_capture::secure_input::windows::adv1_probe as probe;
use halvo_capture::secure_input::windows::PasswordGate;
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
    GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsDialogMessageW, PeekMessageW, RegisterClassW, SetForegroundWindow,
    SetWindowTextW, ShowWindow, TranslateMessage, UnregisterClassW, WindowFromPoint, ES_PASSWORD,
    MSG, PM_REMOVE, SC_KEYMENU, SM_CXSCREEN, SM_CYSCREEN, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW,
    WS_TABSTOP, WS_VISIBLE,
};

fn say(message: impl AsRef<str>) {
    println!("ADV1 {}", message.as_ref());
}

fn with_deadline<F>(test: &'static str, limit: Duration, body: F)
where
    F: FnOnce() + Send + 'static,
{
    let (done, finished) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name(format!("adv1-{test}"))
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
        .name("adv1-sink-forward".to_string())
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

fn pump_dialog(hwnd: HWND) {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a live MSG that PeekMessageW fills, and the same value
    // goes to IsDialogMessageW, TranslateMessage and DispatchMessageW.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if IsDialogMessageW(hwnd, &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

struct ProbeWindow {
    hwnd: HWND,
    class: HSTRING,
    hinstance: HINSTANCE,
}

impl ProbeWindow {
    fn create() -> ProbeWindow {
        let class = HSTRING::from("HalvoAdv1ProbeFields");
        let title = HSTRING::from("halvo-adv1-probe-fields");
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keys {
    TabLetter,
    TabThenLateLetter,
    LetterOnly,
    ClickLetter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sender {
    TestThread,
    FixtureThread,
}

#[derive(Debug, Clone, Copy)]
struct Phase {
    name: &'static str,
    keys: Keys,
    sender: Sender,
    stall_ms: u64,
    self_focus: bool,
    late_letter_ms: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct State {
    foreground: bool,
    focus_plain: bool,
    focus_password: bool,
    plain_len: i32,
    password_len: i32,
}

enum Command {
    Reset,
    State,
    Round(Phase),
}

enum Answer {
    State(State),
    Sent {
        before: State,
        sent_at: u64,
        accepted: u32,
        click_aimed: bool,
    },
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

fn serve(take: &mpsc::Receiver<Command>, give: &mpsc::Sender<Answer>, opened: &mpsc::Sender<bool>) {
    let window = ProbeWindow::create();
    let plain = window.edit(WS_TABSTOP, 10);
    let password = window.edit(WS_TABSTOP | WINDOW_STYLE(ES_PASSWORD as u32), 44);
    pump_thread(Duration::from_millis(300));
    let foreground = window.activate();
    focus_on(plain);
    pump_thread(Duration::from_millis(300));
    let _ = opened.send(foreground);
    let state = |window: &ProbeWindow| {
        // SAFETY: GetFocus takes no argument and returns a handle value.
        let focus = unsafe { GetFocus() };
        State {
            foreground: window.is_foreground(),
            focus_plain: focus == plain,
            focus_password: focus == password,
            plain_len: text_len(plain),
            password_len: text_len(password),
        }
    };
    loop {
        pump_dialog(window.hwnd);
        match take.recv_timeout(Duration::from_millis(5)) {
            Ok(Command::Reset) => {
                if !window.is_foreground() {
                    window.activate();
                }
                clear_text(plain);
                clear_text(password);
                focus_on(plain);
                pump_thread(Duration::from_millis(50));
                let _ = give.send(Answer::State(state(&window)));
            }
            Ok(Command::State) => {
                let _ = give.send(Answer::State(state(&window)));
            }
            Ok(Command::Round(phase)) => {
                let before = state(&window);
                let letter = [key_input(VK_A, false), key_input(VK_A, true)];
                let mut click_aimed = true;
                let mut inputs: Vec<INPUT> = Vec::new();
                match phase.keys {
                    Keys::TabLetter => {
                        inputs.push(key_input(VK_TAB, false));
                        inputs.push(key_input(VK_TAB, true));
                    }
                    Keys::TabThenLateLetter => {
                        inputs.push(key_input(VK_TAB, false));
                        inputs.push(key_input(VK_TAB, true));
                    }
                    Keys::LetterOnly => {}
                    Keys::ClickLetter => {
                        click_aimed = aim_at(password);
                        inputs.push(mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN));
                        inputs.push(mouse_input(0, 0, MOUSEEVENTF_LEFTUP));
                    }
                }
                if phase.keys != Keys::TabThenLateLetter {
                    inputs.extend_from_slice(&letter);
                }
                let sent_at = probe::micros();
                let accepted = if click_aimed { send(&inputs) } else { 0 };
                let _ = give.send(Answer::Sent {
                    before,
                    sent_at,
                    accepted,
                    click_aimed,
                });
                if phase.stall_ms > 0 {
                    std::thread::sleep(Duration::from_millis(phase.stall_ms));
                }
                if phase.self_focus {
                    focus_on(password);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

struct Fields {
    commands: mpsc::Sender<Command>,
    answers: mpsc::Receiver<Answer>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Fields {
    fn open() -> Fields {
        let (commands, take) = mpsc::channel::<Command>();
        let (give, answers) = mpsc::channel::<Answer>();
        let (opened, opening) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("adv1-field-window".to_string())
            .spawn(move || serve(&take, &give, &opened))
            .expect("spawn the field window thread");
        let foreground = opening
            .recv_timeout(Duration::from_secs(20))
            .expect("INCONCLUSIVE: the field window did not open");
        say(format!("field_window_foreground={foreground}"));
        Fields {
            commands,
            answers,
            thread: Some(thread),
        }
    }

    fn ask(&self, command: Command) -> Answer {
        self.commands
            .send(command)
            .expect("the field window thread");
        self.answers
            .recv_timeout(Duration::from_secs(10))
            .expect("INCONCLUSIVE: the field window did not answer")
    }

    fn state(&self, command: Command) -> State {
        match self.ask(command) {
            Answer::State(state) => state,
            Answer::Sent { .. } => State::default(),
        }
    }
}

impl Drop for Fields {
    fn drop(&mut self) {
        let (closed, _) = mpsc::channel();
        self.commands = closed;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Default)]
struct Totals {
    rounds: u32,
    valid: u32,
    letters_recorded: u32,
    letters_recorded_and_in_password: u32,
    letters_in_password: u32,
    letters_in_plain: u32,
    focus_keys_recorded: u32,
    letters_late: u32,
    letters_withheld_active: u32,
    letters_withheld_unknown: u32,
    keystroke_events: usize,
}

fn rel(at: u64, base: u64) -> i64 {
    i64::try_from(at).unwrap_or(i64::MAX) - i64::try_from(base).unwrap_or(i64::MAX)
}

fn run_phase(fields: &Fields, events: &Events, phase: Phase, rounds: u32) {
    let mut totals = Totals::default();
    for round in 0..rounds {
        let reset = fields.state(Command::Reset);
        let _ = events.drain_for(Duration::from_millis(300));
        let _ = probe::take();
        let (before, sent_at, accepted, click_aimed) = match phase.sender {
            Sender::TestThread => {
                let before = fields.state(Command::State);
                let sent_at = probe::micros();
                let accepted = send(&[
                    key_input(VK_TAB, false),
                    key_input(VK_TAB, true),
                    key_input(VK_A, false),
                    key_input(VK_A, true),
                ]);
                (before, sent_at, accepted, true)
            }
            Sender::FixtureThread => match fields.ask(Command::Round(phase)) {
                Answer::Sent {
                    before,
                    sent_at,
                    accepted,
                    click_aimed,
                } => {
                    let mut accepted = accepted;
                    if phase.keys == Keys::TabThenLateLetter {
                        std::thread::sleep(Duration::from_millis(phase.late_letter_ms));
                        accepted += send(&[key_input(VK_A, false), key_input(VK_A, true)]);
                    }
                    (before, sent_at, accepted, click_aimed)
                }
                Answer::State(_) => (State::default(), 0, 0, false),
            },
        };
        let seen = events.drain_for(Duration::from_millis(phase.stall_ms + 450));
        let after = fields.state(Command::State);
        let log = probe::take();
        let keystroke_events = seen
            .iter()
            .filter(|e| e.modality == Modality::Keystroke)
            .count();

        let arrivals: Vec<&(u8, u64, u32, u32)> = log.iter().filter(|e| e.0 == 8).collect();
        let decisions: Vec<&(u8, u64, u32, u32)> = log
            .iter()
            .filter(|e| matches!(e.0, 5 | 6 | 7 | 10 | 11))
            .collect();
        let letter_at = arrivals.iter().position(|e| e.2 & 1 == 0);
        let letter_decision = letter_at.and_then(|i| decisions.get(i)).map(|e| e.0);
        let focus_key_decision = arrivals
            .iter()
            .position(|e| e.2 & 1 == 1)
            .and_then(|i| decisions.get(i))
            .map(|e| e.0);
        let letter_arrival = letter_at.map(|i| arrivals[i].1);
        let latch = log.iter().find(|e| matches!(e.0, 3 | 4)).map(|e| e.1);
        let read = latch.and_then(|latch| log.iter().find(|e| e.0 == 2 && e.1 >= latch));
        let read_began =
            latch.and_then(|latch| log.iter().find(|e| e.0 == 12 && e.1 >= latch).map(|e| e.1));
        let first_field = letter_arrival.and_then(|at| log.iter().find(|e| e.0 == 1 && e.1 >= at));
        let first_password_mark = log
            .iter()
            .find(|e| matches!(e.0, 1 | 2) && e.3 == 1)
            .map(|e| e.1);
        let letter_released = letter_at.and_then(|i| decisions.get(i)).map(|e| e.1);
        let or = |v: Option<i64>| v.map_or("none".to_string(), |v| v.to_string());
        let expected = match phase.keys {
            Keys::TabLetter | Keys::TabThenLateLetter | Keys::ClickLetter => 4,
            Keys::LetterOnly => 2,
        };
        let valid = reset.foreground
            && before.foreground
            && before.focus_plain
            && accepted == expected
            && click_aimed
            && letter_decision.is_some();
        say(format!(
            "phase={} round={round} valid={valid} fg_at_send={} focus_plain_at_send={} accepted={accepted} click_aimed={click_aimed} \
             password_len_before={} password_len_after={} plain_len_before={} plain_len_after={} focus_password_after={} fg_after={} \
             keystroke_events={keystroke_events} key_downs_seen={} letter_decision={} focus_key_decision={} letter_read_as_password_edit={} \
             send_to_first_arrival_us={} latch_to_read_began_us={} latch_to_read_mark_us={} read_state={} \
             letter_to_first_focus_event_us={} first_focus_event_state={} letter_to_first_password_mark_us={} letter_to_release_us={}",
            phase.name,
            before.foreground,
            before.focus_plain,
            before.password_len,
            after.password_len,
            before.plain_len,
            after.plain_len,
            after.focus_password,
            after.foreground,
            arrivals.len(),
            or(letter_decision.map(i64::from)),
            or(focus_key_decision.map(i64::from)),
            or(letter_at.map(|i| i64::from((arrivals[i].2 >> 1) & 1))),
            or(arrivals.first().map(|e| rel(e.1, sent_at))),
            or(latch.zip(read_began).map(|(l, r)| rel(r, l))),
            or(latch.zip(read).map(|(l, r)| rel(r.1, l))),
            or(read.map(|e| i64::from(e.3))),
            or(letter_arrival.zip(first_field).map(|(k, f)| rel(f.1, k))),
            or(first_field.map(|e| i64::from(e.3))),
            or(letter_arrival.zip(first_password_mark).map(|(k, p)| rel(p, k))),
            or(letter_arrival.zip(letter_released).map(|(k, r)| rel(r, k))),
        ));
        let raw: Vec<String> = log
            .iter()
            .map(|e| format!("{}@{}:{}:{}", e.0, rel(e.1, sent_at), e.2, e.3))
            .collect();
        say(format!(
            "phase={} round={round} log_entries={} log={}",
            phase.name,
            raw.len(),
            raw.join(",")
        ));
        totals.rounds += 1;
        totals.keystroke_events += keystroke_events;
        if valid {
            totals.valid += 1;
            let in_password = after.password_len > before.password_len;
            totals.letters_in_password += u32::from(in_password);
            totals.letters_in_plain += u32::from(after.plain_len > before.plain_len);
            totals.focus_keys_recorded += u32::from(focus_key_decision == Some(5));
            match letter_decision {
                Some(5) => {
                    totals.letters_recorded += 1;
                    totals.letters_recorded_and_in_password += u32::from(in_password);
                }
                Some(6) => totals.letters_late += 1,
                Some(7) => totals.letters_withheld_active += 1,
                Some(10) => totals.letters_withheld_unknown += 1,
                _ => {}
            }
        }
    }
    say(format!(
        "summary phase={} keys={:?} sender={:?} stall_ms={} self_focus={} late_letter_ms={} rounds={} valid_rounds={} keystroke_events={} \
         letters_recorded={} letters_in_password={} letters_recorded_and_in_password={} letters_in_plain={} \
         focus_keys_recorded={} letters_late={} letters_withheld_active={} letters_withheld_unknown={}",
        phase.name,
        phase.keys,
        phase.sender,
        phase.stall_ms,
        phase.self_focus,
        phase.late_letter_ms,
        totals.rounds,
        totals.valid,
        totals.keystroke_events,
        totals.letters_recorded,
        totals.letters_in_password,
        totals.letters_recorded_and_in_password,
        totals.letters_in_plain,
        totals.focus_keys_recorded,
        totals.letters_late,
        totals.letters_withheld_active,
        totals.letters_withheld_unknown,
    ));
}

fn run_phases(test: &'static str, phases: Vec<Phase>, rounds: u32) {
    let fields = Fields::open();
    let (sink, writer) = channel(8192);
    let events = forward(writer);
    let mut keyboard = RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
    let start = keyboard.begin(SourceContext::new(sink.clone()));
    say(format!("test={test} keyboard_start_ok={}", start.is_ok()));
    if start.is_ok() {
        let _ = events.drain_for(Duration::from_millis(500));
        for phase in phases {
            run_phase(&fields, &events, phase, rounds);
        }
    }
    let stopped = keyboard.shutdown();
    say(format!("test={test} keyboard_stop_ok={}", stopped.is_ok()));
    drop(fields);
    assert!(
        start.is_ok(),
        "INCONCLUSIVE: the keyboard source did not start"
    );
}

const fn phase(
    name: &'static str,
    keys: Keys,
    sender: Sender,
    stall_ms: u64,
    self_focus: bool,
) -> Phase {
    Phase {
        name,
        keys,
        sender,
        stall_ms,
        self_focus,
        late_letter_ms: 0,
    }
}

const fn late(name: &'static str, stall_ms: u64, late_letter_ms: u64) -> Phase {
    Phase {
        name,
        keys: Keys::TabThenLateLetter,
        sender: Sender::FixtureThread,
        stall_ms,
        self_focus: false,
        late_letter_ms,
    }
}

#[test]
#[ignore = "probe: needs an interactive Windows desktop"]
fn adv1_a_tab_then_a_letter_with_and_without_a_stall() {
    with_deadline("tab", Duration::from_secs(480), || {
        run_phases(
            "tab",
            vec![
                phase("P", Keys::LetterOnly, Sender::FixtureThread, 0, false),
                phase("E", Keys::TabLetter, Sender::TestThread, 0, false),
                phase("E2", Keys::TabLetter, Sender::TestThread, 0, false),
                phase("E3", Keys::TabLetter, Sender::TestThread, 0, false),
                phase("S0", Keys::TabLetter, Sender::FixtureThread, 0, false),
                phase("S60", Keys::TabLetter, Sender::FixtureThread, 60, false),
                phase("S150", Keys::TabLetter, Sender::FixtureThread, 150, false),
                phase("S220", Keys::TabLetter, Sender::FixtureThread, 220, false),
                phase("S300", Keys::TabLetter, Sender::FixtureThread, 300, false),
                late("L300", 300, 150),
                late("L600", 600, 400),
            ],
            20,
        );
    });
}

#[test]
#[ignore = "probe: needs an interactive Windows desktop"]
fn adv1_b_a_letter_and_then_the_fixture_moves_the_focus_itself() {
    with_deadline("self-focus", Duration::from_secs(240), || {
        run_phases(
            "self-focus",
            vec![
                phase("N150", Keys::LetterOnly, Sender::FixtureThread, 150, true),
                phase("N300", Keys::LetterOnly, Sender::FixtureThread, 300, true),
                phase("N600", Keys::LetterOnly, Sender::FixtureThread, 600, true),
            ],
            20,
        );
    });
}

#[test]
#[ignore = "probe: needs an interactive Windows desktop"]
fn adv1_c_a_click_then_a_letter_with_and_without_a_stall() {
    with_deadline("click", Duration::from_secs(240), || {
        run_phases(
            "click",
            vec![
                phase("K0", Keys::ClickLetter, Sender::FixtureThread, 0, false),
                phase("K150", Keys::ClickLetter, Sender::FixtureThread, 150, false),
                phase("K300", Keys::ClickLetter, Sender::FixtureThread, 300, false),
            ],
            20,
        );
    });
}
