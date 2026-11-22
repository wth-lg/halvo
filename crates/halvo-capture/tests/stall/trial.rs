use std::sync::atomic::AtomicBool;

use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    AccessibleObjectFromEvent, CUIAutomation, IAccessible, IUIAutomation, SetWinEventHook,
    UnhookWinEvent, HWINEVENTHOOK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    PostThreadMessageW, SendMessageTimeoutW, EVENT_OBJECT_FOCUS, EVENT_OBJECT_TEXTSELECTIONCHANGED,
    EVENT_OBJECT_VALUECHANGE, SMTO_ABORTIFHUNG, SMTO_BLOCK, STATE_SYSTEM_PROTECTED,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_NULL, WM_QUIT,
};

use super::*;

pub(crate) fn tick() -> u32 {
    // SAFETY: GetTickCount takes no argument and reads a counter.
    unsafe { super::super::GetTickCount() }
}

fn ms_after(a: u32, b: u32) -> i64 {
    i64::from(b.wrapping_sub(a) as i32)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WinEventSeen {
    pub(crate) event: u32,
    pub(crate) hwnd: isize,
    pub(crate) made: u32,
    pub(crate) handled: u32,
    pub(crate) pid: u32,
    pub(crate) protected: i8,
}

static SEEN_EVENTS: Mutex<Vec<WinEventSeen>> = Mutex::new(Vec::new());
static READ_STATE: AtomicBool = AtomicBool::new(false);

fn protected_of(hwnd: HWND, object: i32, child: i32) -> i8 {
    let mut acc: Option<IAccessible> = None;
    let mut var = VARIANT::default();
    // SAFETY: the out pointers are live locals, and the call takes plain values.
    let found =
        unsafe { AccessibleObjectFromEvent(hwnd, object as u32, child as u32, &mut acc, &mut var) };
    let Some(acc) = acc.filter(|_| found.is_ok()) else {
        return -1;
    };
    // SAFETY: `acc` is live and `var` names the child that the call gave.
    let Ok(state) = (unsafe { acc.get_accState(&var) }) else {
        return -1;
    };
    // SAFETY: the tag names the member of the union that the value holds.
    unsafe {
        let inner = &state.Anonymous.Anonymous;
        if inner.vt == VT_I4 {
            i8::from(inner.Anonymous.lVal as u32 & STATE_SYSTEM_PROTECTED != 0)
        } else {
            -1
        }
    }
}

unsafe extern "system" fn on_win_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    made: u32,
) {
    let handled = tick();
    let mut pid = 0u32;
    // SAFETY: GetWindowThreadProcessId reads a handle value and fills `pid`.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    let protected = if READ_STATE.load(Ordering::Relaxed) {
        protected_of(hwnd, object, child)
    } else {
        -1
    };
    SEEN_EVENTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(WinEventSeen {
            event,
            hwnd: hwnd.0 as isize,
            made,
            handled,
            pid,
            protected,
        });
}

pub(crate) struct Recorder {
    thread: u32,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Recorder {
    pub(crate) fn start(read_state: bool) -> Recorder {
        READ_STATE.store(read_state, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        let join = std::thread::spawn(move || {
            // SAFETY: joins this thread to the process's MTA for the reads of
            // the event's element.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let hooks: Vec<HWINEVENTHOOK> = [
                EVENT_OBJECT_FOCUS,
                EVENT_OBJECT_VALUECHANGE,
                EVENT_OBJECT_TEXTSELECTIONCHANGED,
            ]
            .into_iter()
            .map(|event| {
                // SAFETY: an out-of-context hook with no module and a callback of
                // the WINEVENTPROC signature, served by this thread's pump.
                unsafe {
                    SetWinEventHook(
                        event,
                        event,
                        None,
                        Some(on_win_event),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                    )
                }
            })
            .collect();
            // SAFETY: GetCurrentThreadId takes no argument.
            let _ = tx.send(unsafe { GetCurrentThreadId() });
            let mut msg = MSG::default();
            // SAFETY: `msg` is a live MSG that GetMessageW fills.
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
                // SAFETY: as above.
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            for hook in hooks {
                // SAFETY: each hook is one that this thread set.
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            }
            // SAFETY: this thread's CoInitializeEx was called once above.
            unsafe { CoUninitialize() };
        });
        let thread = rx.recv().expect("the recorder's thread");
        Recorder {
            thread,
            join: Some(join),
        }
    }

    pub(crate) fn take(&self) -> Vec<WinEventSeen> {
        std::mem::take(
            &mut *SEEN_EVENTS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // SAFETY: PostThreadMessageW takes plain values.
        unsafe {
            let _ = PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ThreadRead {
    state: u32,
    reason: u32,
    switches: u32,
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut u8, len: u32, ret: *mut u32) -> i32;
}

fn thread_read(pid: u32, tid: u32, buffer: &mut Vec<u8>) -> Option<ThreadRead> {
    const SYSTEM_PROCESS_INFORMATION: u32 = 5;
    let mut needed = 0u32;
    for _ in 0..4 {
        // SAFETY: `buffer` is live for its length, and `needed` is a live u32.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PROCESS_INFORMATION,
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(u32::MAX),
                &mut needed,
            )
        };
        if status >= 0 {
            break;
        }
        buffer.resize((needed as usize).max(buffer.len() * 2) + 65_536, 0);
    }
    let read_u32 = |at: usize| -> Option<u32> {
        Some(u32::from_ne_bytes(buffer.get(at..at + 4)?.try_into().ok()?))
    };
    let read_u64 = |at: usize| -> Option<u64> {
        Some(u64::from_ne_bytes(buffer.get(at..at + 8)?.try_into().ok()?))
    };
    let mut at = 0usize;
    loop {
        let next = read_u32(at)? as usize;
        let threads = read_u32(at + 4)? as usize;
        if read_u64(at + 0x50)? == u64::from(pid) {
            for index in 0..threads {
                let entry = at + 0x100 + index * 0x50;
                if read_u64(entry + 48)? == u64::from(tid) {
                    return Some(ThreadRead {
                        switches: read_u32(entry + 64)?,
                        state: read_u32(entry + 68)?,
                        reason: read_u32(entry + 72)?,
                    });
                }
            }
            return None;
        }
        if next == 0 {
            return None;
        }
        at += next;
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Sample {
    at: u32,
    thread: Option<ThreadRead>,
    answered: bool,
}

pub(crate) struct Sampler {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<Vec<Sample>>>,
}

impl Sampler {
    pub(crate) fn start(pid: u32, tid: u32, window: isize) -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let join = std::thread::spawn(move || {
            let mut buffer = vec![0u8; 1 << 20];
            let mut samples = Vec::new();
            while !flag.load(Ordering::Relaxed) {
                let at = tick();
                let thread = thread_read(pid, tid, &mut buffer);
                let mut result = 0usize;
                // SAFETY: SendMessageTimeoutW takes plain values and a live out
                // pointer; WM_NULL does nothing in the receiver.
                let answered = unsafe {
                    SendMessageTimeoutW(
                        HWND(window as *mut core::ffi::c_void),
                        WM_NULL,
                        WPARAM(0),
                        LPARAM(0),
                        SMTO_ABORTIFHUNG | SMTO_BLOCK,
                        10,
                        Some(&mut result),
                    )
                }
                .0 != 0;
                samples.push(Sample {
                    at,
                    thread,
                    answered,
                });
                std::thread::sleep(Duration::from_millis(10));
            }
            samples
        });
        Sampler {
            stop,
            join: Some(join),
        }
    }

    pub(crate) fn finish(mut self) -> Vec<Sample> {
        self.stop.store(true, Ordering::Relaxed);
        self.join
            .take()
            .and_then(|join| join.join().ok())
            .unwrap_or_default()
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn user_wait(read: ThreadRead) -> bool {
    read.state == 5 && (read.reason == 6 || read.reason == 13)
}

fn delay_wait(read: ThreadRead) -> bool {
    read.state == 5 && (read.reason == 4 || read.reason == 11)
}

pub(crate) fn sample_counts(samples: &[Sample], from: u32, to: u32) -> String {
    let inside: Vec<&Sample> = samples
        .iter()
        .filter(|s| ms_after(from, s.at) >= 0 && ms_after(s.at, to) > 0)
        .collect();
    let user = inside
        .iter()
        .filter(|s| s.thread.is_some_and(user_wait))
        .count();
    let delay = inside
        .iter()
        .filter(|s| s.thread.is_some_and(delay_wait))
        .count();
    let unread = inside.iter().filter(|s| s.thread.is_none()).count();
    let answered = inside.iter().filter(|s| s.answered).count();
    let reasons: std::collections::BTreeSet<String> = inside
        .iter()
        .filter_map(|s| s.thread.map(|t| format!("{}.{}", t.state, t.reason)))
        .collect();
    let switches = match (
        inside.first().and_then(|s| s.thread),
        inside.last().and_then(|s| s.thread),
    ) {
        (Some(a), Some(b)) => i64::from(b.switches.wrapping_sub(a.switches)),
        _ => -1,
    };
    format!(
        "samples={} user_wait={user} delay_wait={delay} unread={unread} probe_answered={answered} \
         states=[{}] switches={switches}",
        inside.len(),
        reasons.into_iter().collect::<Vec<_>>().join(",")
    )
}

struct Com;

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: the thread's CoInitializeEx succeeded once.
        unsafe { CoUninitialize() };
    }
}

pub(crate) fn pointer_element() -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let began = Instant::now();
        // SAFETY: joins this thread to the process's MTA.
        if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .is_err()
        {
            let _ = tx.send("pointer=no-com".to_string());
            return;
        }
        let _com = Com;
        // SAFETY: CoCreateInstance makes the system's UI Automation object.
        let automation: windows::core::Result<IUIAutomation> =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) };
        let Ok(automation) = automation else {
            let _ = tx.send("pointer=no-uia".to_string());
            return;
        };
        let mut at = POINT::default();
        // SAFETY: `at` is a live POINT that the call fills.
        let _ = unsafe { GetCursorPos(&mut at) };
        // SAFETY: the call takes the live automation object and a point.
        let text = match unsafe { automation.ElementFromPoint(at) } {
            Ok(element) => {
                // SAFETY: the element is live for the call.
                let password =
                    unsafe { element.CurrentIsPassword() }.map_or(-1, |p| i32::from(p.as_bool()));
                format!(
                    "pointer=answered pointer_is_pw={password} pointer_ms={}",
                    began.elapsed().as_millis()
                )
            }
            Err(_) => format!("pointer=error pointer_ms={}", began.elapsed().as_millis()),
        };
        let _ = tx.send(text);
    });
    rx.recv_timeout(Duration::from_millis(1_500))
        .unwrap_or_else(|_| "pointer=no-answer-in-1500ms".to_string())
}

pub(crate) fn event_line(
    seen: &[WinEventSeen],
    pid: u32,
    from: u32,
    held_until: u32,
    class: impl Fn(&WinEventSeen) -> char,
) -> String {
    let ours: Vec<&WinEventSeen> = seen
        .iter()
        .filter(|e| e.pid == pid && ms_after(from, e.made) >= 0)
        .collect();
    let mut out = Vec::new();
    for (event, word) in [
        (EVENT_OBJECT_FOCUS, "focus"),
        (EVENT_OBJECT_VALUECHANGE, "value"),
        (EVENT_OBJECT_TEXTSELECTIONCHANGED, "caret"),
    ] {
        let of: Vec<&&WinEventSeen> = ours.iter().filter(|e| e.event == event).collect();
        let held = of
            .iter()
            .filter(|e| ms_after(e.made, held_until) > 0)
            .count();
        let list: Vec<String> = of
            .iter()
            .map(|e| {
                format!(
                    "{}{:+}/{}",
                    class(e),
                    ms_after(held_until, e.made),
                    ms_after(e.made, e.handled)
                )
            })
            .collect();
        out.push(format!(
            "{word}_made_while_held={held} {word}=[{}]",
            list.join(",")
        ));
    }
    out.join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrialLead {
    Plain,
    Tab,
    Click,
    OwnMove,
}

fn native_trial_round(
    test: &str,
    fixture: &mut Fixture,
    recorder: &Recorder,
    (lead, wait, stall_ms): (TrialLead, Wait, u64),
    round: u32,
) {
    let pid = fixture.child.id();
    let tid = u32::try_from(get(&fixture.ready, "thread")).unwrap_or(0);
    let hwnds = [
        get(&fixture.ready, "plain") as isize,
        get(&fixture.ready, "password") as isize,
    ];
    let window = get(&fixture.ready, "window") as isize;
    let _reset = fixture.ask(CMD_RESET, 0, "reset");
    std::thread::sleep(Duration::from_millis(400));
    let _ = seam::take();
    let _ = recorder.take();
    let mut arg = usize::try_from(stall_ms).unwrap_or(0) & 0xFFFF;
    if lead == TrialLead::Click {
        arg |= ARG_AIM;
    }
    if wait == Wait::ServesSent {
        arg |= ARG_SERVE_SENT;
    }
    if lead == TrialLead::OwnMove {
        arg |= ARG_SELF_FOCUS;
    }
    let sampler = Sampler::start(pid, tid, window);
    let ack = fixture.ask(CMD_ROUND, arg, "ack");
    let sent = tick();
    let mut letters = 0u32;
    let mut pointer = String::from("pointer=not-read");
    match lead {
        TrialLead::Tab => {
            send(&key_pair(VK_TAB));
        }
        TrialLead::Click => {
            send(&[
                mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN),
                mouse_input(0, 0, MOUSEEVENTF_LEFTUP),
            ]);
        }
        TrialLead::Plain | TrialLead::OwnMove => {}
    }
    send(&key_pair(VK_A));
    letters += 1;
    if lead == TrialLead::Click {
        pointer = pointer_element();
    }
    let span = Duration::from_millis(stall_ms.max(300));
    let began = Instant::now();
    while began.elapsed() + LETTER_STEP < span {
        std::thread::sleep(LETTER_STEP);
        send(&key_pair(VK_A));
        letters += 1;
    }
    let woke = fixture.expect("woke", Duration::from_secs(10));
    std::thread::sleep(Duration::from_millis(900));
    let after = tick();
    let samples = sampler.finish();
    let seen = recorder.take();
    let state = fixture.ask(CMD_STATE, 0, "state");
    let decided = Decisions::of(&seam::take());
    let woke_at = get(&woke, "tick") as u32;
    let ack_at = get(&ack, "tick") as u32;
    let class = |e: &WinEventSeen| {
        if e.hwnd == hwnds[0] {
            't'
        } else if e.hwnd == hwnds[1] {
            'p'
        } else {
            'o'
        }
    };
    say(
        test,
        format!(
            "trial=native lead={lead:?} wait={wait:?} stall_ms={stall_ms} round={round} \
             fg={} letters_sent={letters} letters_plain={} letters_pw={} \
             gate_letters_recorded={} held_ms={} send_after_ack_ms={} \
             held: {} | after: {} | {} | {pointer}",
            get(&state, "fg"),
            get(&state, "letters_plain"),
            get(&state, "letters_password"),
            decided.letters_recorded,
            ms_after(ack_at, woke_at),
            ms_after(ack_at, sent),
            sample_counts(&samples, sent, woke_at),
            sample_counts(&samples, woke_at.wrapping_add(300), after),
            event_line(&seen, pid, sent, woke_at, class),
        ),
    );
}

#[test]
#[ignore = "trial: prints the signals of a program that takes input or not; asserts nothing"]
fn trial_signals_of_the_native_fixture() {
    let test = "trial-native";
    with_deadline(test, Duration::from_secs(1_200), move || {
        source_built_by_test(test, "keyboard");
        let mut fixture = Fixture::open(test, false);
        let (sink, events) = capture(8192);
        let mut keyboard =
            RawInputSource::keyboard(Arc::new(Mutex::new(None)), PasswordGate::new());
        let start = keyboard.begin(SourceContext::new(sink.clone()));
        start_report(test, "keyboard", &start);
        let recorder = Recorder::start(false);
        let _ = events.drain_for(Duration::from_millis(500));
        for round in 0..3 {
            native_trial_round(
                test,
                &mut fixture,
                &recorder,
                (TrialLead::Plain, Wait::Sleeps, 0),
                round,
            );
            let _ = events.drain_for(Duration::from_millis(10));
        }
        for lead in [TrialLead::Tab, TrialLead::Click, TrialLead::OwnMove] {
            let mut lengths = vec![(Wait::Sleeps, 0)];
            for wait in [Wait::Sleeps, Wait::ServesSent] {
                for ms in [150, 300, 600, 2_000] {
                    lengths.push((wait, ms));
                }
            }
            for (wait, ms) in lengths {
                let rounds = if ms >= 2_000 { 2 } else { 3 };
                for round in 0..rounds {
                    native_trial_round(test, &mut fixture, &recorder, (lead, wait, ms), round);
                    let _ = events.drain_for(Duration::from_millis(10));
                }
            }
        }
        drop(recorder);
        let _ = stop_counted(&mut keyboard, "keyboard");
        drop(fixture);
        say(test, "trial=native ended=1");
    });
}
