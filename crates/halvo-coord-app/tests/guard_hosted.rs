//! The hold of the unit's own system dialog (ruling RD-91 item 7), built through `System::show`
//! with the product's `arm_ms`, on a hosted runner of GitHub Actions alone, in the form of the
//! target `dialog_hosted` (RD-79 E1): a control is pressed by the system's own call for it, and
//! no input event of the operating system is posted. Each press is timed from the dialog's report
//! on screen, which the dialog makes when its window became the active window. A press of each
//! control 100 ms after the report leaves the dialog open; a press after the hold answers; a
//! refused press starts the hold anew; a second show holds again; a press 100 ms after the
//! window became active again leaves it open; and at each report the dialog's window was the
//! active window. A show that has not reported itself in 1 s is brought to the front once a
//! second, for at most 8 s, with a line that says what was active before it; the target makes its
//! app finish launching and become the active app first, as the app is when a person asks for the
//! dialog. On macOS a press is the button's `performClick:`, the
//! system's click call, or the button's action sent through the application, as a key equivalent
//! sends it; on Windows a press is the message TDM_CLICK_BUTTON for the allowing id, the rejecting
//! id and the cancel response that the Escape key and the close control give, or the message
//! WM_CLOSE. On every other machine the target shows nothing and prints one line, with the start
//! and the end of `dialog_hosted`. A case has a bound of 10 s for each show: a show that reaches
//! it is closed and its case fails. The last line counts the cases that ran.

#[path = "hosted/gate.rs"]
mod gate;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::{Class, Field, FieldFrom, Limits};
use halvo_coord_app::Card;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

const OFF: &str = "hosted alone: not run on this machine";
const BOUND: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_millis(150);
const REPORT_WAIT: Duration = Duration::from_secs(1);
const FRONT_LIMIT: Duration = Duration::from_secs(8);

fn arm() -> Duration {
    Duration::from_millis(Limits::PRODUCT.arm_ms)
}

fn word(answer: ConfirmAnswer) -> &'static str {
    match answer {
        ConfirmAnswer::Allow => "allow",
        ConfirmAnswer::Reject => "reject",
    }
}

fn card() -> Card {
    let field = |label: &str, value: &str, from: FieldFrom| Field {
        label: label.into(),
        value: vec![value.into()],
        from,
        from_text: String::new(),
    };
    Card::new(
        &[Class::Payment],
        &[
            field("Pay to", "City Power", FieldFrom::You),
            field("Amount", "42.10 USD", FieldFrom::Site),
        ],
    )
    .expect("the words file holds the entries of payment")
}

/// One press of a show: `at` from the show's report, by `control`; `answers` for the press that
/// must close the dialog, and for no other.
#[derive(Clone, Copy)]
struct Press<C: 'static> {
    at: u64,
    control: C,
    answers: bool,
}

/// One show of a case: its presses and the answer that it must give.
type Show<C> = (&'static [Press<C>], ConfirmAnswer);

/// What one show of the dialog left: the reports, whether the window was the active window at
/// the first report, and the moment of the first report.
static REPORTS: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_AT_REPORT: AtomicBool = AtomicBool::new(false);
static REPORT_AT: Mutex<Option<Instant>> = Mutex::new(None);
static DONE: AtomicBool = AtomicBool::new(false);

fn reported() -> Option<Instant> {
    *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn on_screen(active: bool) {
    if REPORTS.fetch_add(1, Ordering::SeqCst) == 0 {
        ACTIVE_AT_REPORT.store(active, Ordering::SeqCst);
    }
    REPORT_AT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(Instant::now);
}

fn begin_show() {
    REPORTS.store(0, Ordering::SeqCst);
    ACTIVE_AT_REPORT.store(false, Ordering::SeqCst);
    *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner) = None;
    DONE.store(false, Ordering::SeqCst);
}

/// The system's side of a case: a press of a control, whether the dialog is open, a move of the
/// dialog to the front with a reading of what was active before it, and its close at the bound.
trait Ui<C> {
    fn press(&self, control: C) -> bool;
    fn open(&self) -> bool;
    fn front(&self) -> String;
    fn close(&self);
    fn word(control: C) -> String;
}

fn sleep_until(at: Instant) {
    let now = Instant::now();
    if at > now {
        std::thread::sleep(at - now);
    }
}

fn wait(limit: Duration, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

/// Drives one show's presses from the report on: the failures that it found.
fn drive<C: Copy, U: Ui<C>>(ui: &U, label: &str, presses: &[Press<C>]) -> Vec<String> {
    let start = Instant::now();
    let mut failures = Vec::new();
    let mut fronts = 0;
    while !wait(REPORT_WAIT, || reported().is_some()) && start.elapsed() < FRONT_LIMIT {
        fronts += 1;
        let state = ui.front();
        println!("guard_hosted reading {label}: no report yet, brought to the front {fronts} times; {state}");
    }
    let Some(base) = reported() else {
        failures.push("the dialog never reported itself on screen".into());
        while !DONE.load(Ordering::SeqCst) && start.elapsed() < BOUND {
            ui.close();
            std::thread::sleep(arm() + Duration::from_millis(100));
        }
        return failures;
    };
    for press in presses {
        sleep_until(base + Duration::from_millis(press.at));
        if DONE.load(Ordering::SeqCst) {
            failures.push(format!(
                "the dialog had closed before the press at {} ms",
                press.at
            ));
            break;
        }
        let sent = ui.press(press.control);
        if !sent {
            failures.push(format!(
                "the press at {} ms by {} found no control to press",
                press.at,
                U::word(press.control)
            ));
        }
        if !press.answers {
            std::thread::sleep(SETTLE);
            if DONE.load(Ordering::SeqCst) || !ui.open() {
                failures.push(format!(
                    "the press at {} ms by {} closed the dialog inside the hold",
                    press.at,
                    U::word(press.control)
                ));
                break;
            }
        }
    }
    let last = presses.iter().rev().find(|press| press.answers);
    let mut again = 0;
    while let Some(press) = last {
        if !failures.is_empty()
            || again == 3
            || wait(Duration::from_secs(1), || DONE.load(Ordering::SeqCst))
        {
            break;
        }
        again += 1;
        std::thread::sleep(arm() + Duration::from_millis(200));
        ui.press(press.control);
        println!("guard_hosted reading {label}: the press after the hold pressed again, {again} times, as the window became active again after it");
    }
    if !wait(Duration::from_secs(3), || DONE.load(Ordering::SeqCst)) {
        failures.push("the press after the hold did not close the dialog".into());
        while !DONE.load(Ordering::SeqCst) && start.elapsed() < BOUND {
            ui.close();
            std::thread::sleep(arm() + Duration::from_millis(100));
        }
    }
    failures
}

/// Checks one show after its call returned.
fn check(label: &str, expected: ConfirmAnswer, answer: ConfirmAnswer, failures: &mut Vec<String>) {
    let mut found = Vec::new();
    if answer != expected {
        found.push(format!(
            "the answer was {}, not {}",
            word(answer),
            word(expected)
        ));
    }
    let reports = REPORTS.load(Ordering::SeqCst);
    if reports != 1 {
        found.push(format!(
            "the report on screen came {reports} times, not once"
        ));
    }
    let active = ACTIVE_AT_REPORT.load(Ordering::SeqCst);
    if !active {
        found.push("the window was not the active window at the report".into());
    }
    println!(
        "guard_hosted reading {label}: reports={reports} active_at_report={active} answer={}",
        word(answer)
    );
    failures.extend(found.into_iter().map(|f| format!("{label}: {f}")));
}

/// One case's result: the failures that it found, none when it passed.
struct Verdict {
    name: &'static str,
    failures: Vec<String>,
}

fn print(verdict: &Verdict) {
    if verdict.failures.is_empty() {
        println!("guard_hosted case {} PASS", verdict.name);
    } else {
        println!(
            "guard_hosted case {} FAIL: {}",
            verdict.name,
            verdict.failures.join("; ")
        );
    }
}

/// Ends the process: status 1 when a case failed, or when the runner's two variables, read
/// here once more and directly, name a hosted runner and fewer cases ran than the list holds.
fn end(verdicts: &[Verdict], listed: usize) -> ! {
    let failed = verdicts.iter().filter(|v| !v.failures.is_empty()).count();
    let hosted = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted");
    let short = hosted && verdicts.len() != listed;
    println!(
        "guard_hosted end: cases ran {} of {listed}, failed {failed}{}",
        verdicts.len(),
        if short {
            ", FAIL: not every case ran"
        } else {
            ""
        }
    );
    std::process::exit(if failed > 0 || short { 1 } else { 0 });
}

/// Runs one case on the calling thread, each show through `show` while `ui` drives it from a
/// thread of its own.
fn case<C: Copy + Send + Sync + 'static, U: Ui<C> + Clone + Send + 'static>(
    ui: &U,
    name: &'static str,
    shows: &'static [Show<C>],
    show: &dyn Fn() -> ConfirmAnswer,
) -> Verdict {
    let mut failures = Vec::new();
    for (n, (presses, expected)) in shows.iter().enumerate() {
        let label = if shows.len() > 1 {
            format!("{name} show {}", n + 1)
        } else {
            name.to_string()
        };
        begin_show();
        let driver = std::thread::spawn({
            let ui = ui.clone();
            let label = label.clone();
            move || drive(&ui, &label, presses)
        });
        let answer = show();
        DONE.store(true, Ordering::SeqCst);
        match driver.join() {
            Ok(found) => failures.extend(found.into_iter().map(|f| format!("{label}: {f}"))),
            Err(_) => failures.push(format!("{label}: the driver panicked")),
        }
        check(&label, *expected, answer, &mut failures);
    }
    Verdict { name, failures }
}

fn main() {
    match gate::start(&gate::env) {
        gate::Start::Run => {}
        gate::Start::Off => {
            println!("{OFF}");
            return;
        }
        gate::Start::Fail(line) => {
            println!("guard_hosted {line}");
            std::process::exit(1);
        }
    }
    std::thread::spawn(|| {
        std::thread::sleep(BOUND * (cases::SHOWS + 3));
        println!("guard_hosted watchdog FAIL: the cases did not end in time");
        std::process::exit(1);
    });
    cases::run(card());
}

#[cfg(target_os = "macos")]
mod cases {
    use super::{arm, case, end, on_screen, print, Press, Show, Ui};
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::{Card, Dialog, System};
    use objc2::runtime::{AnyClass, AnyObject, Sel};
    use objc2::{class, msg_send};
    use objc2_app_kit::NSWindowDidBecomeKeyNotification;
    use objc2_foundation::NSString;
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    const REJECT: isize = 1000;
    const ALLOW: isize = 1001;

    /// A button by its response, pressed by its click call or by its action sent through the
    /// application, or the notification that the alert's window became the key window again.
    #[derive(Clone, Copy)]
    pub enum Control {
        Click(isize),
        Action(isize),
        Again,
    }

    const fn p(at: u64, control: Control, answers: bool) -> Press<Control> {
        Press {
            at,
            control,
            answers,
        }
    }

    const REJECT_ACTION: &[Press<Control>] = &[
        p(100, Control::Action(REJECT), false),
        p(800, Control::Action(REJECT), true),
    ];
    const ALLOW_ACTION: &[Press<Control>] = &[
        p(100, Control::Action(ALLOW), false),
        p(800, Control::Action(ALLOW), true),
    ];
    const ALLOW_CLICK_AGAIN: &[Press<Control>] = &[
        p(100, Control::Click(ALLOW), false),
        p(550, Control::Click(ALLOW), false),
        p(1_250, Control::Click(ALLOW), true),
    ];
    const ALLOW_CLICK_AFTER: &[Press<Control>] = &[p(700, Control::Click(ALLOW), true)];
    const REJECT_CLICK: &[Press<Control>] = &[
        p(100, Control::Click(REJECT), false),
        p(800, Control::Click(REJECT), true),
    ];
    const ALLOW_CLICK: &[Press<Control>] = &[
        p(100, Control::Click(ALLOW), false),
        p(800, Control::Click(ALLOW), true),
    ];
    const ALLOW_AFTER_KEY_AGAIN: &[Press<Control>] = &[
        p(700, Control::Again, false),
        p(800, Control::Click(ALLOW), false),
        p(1_400, Control::Click(ALLOW), true),
    ];

    type Case = (&'static str, &'static [Show<Control>]);
    const CASES: [Case; 6] = [
        (
            "macos reject_action_at_100ms",
            &[(REJECT_ACTION, ConfirmAnswer::Reject)],
        ),
        (
            "macos allow_action_at_100ms",
            &[(ALLOW_ACTION, ConfirmAnswer::Allow)],
        ),
        (
            "macos allow_click_at_100ms_and_550ms",
            &[(ALLOW_CLICK_AGAIN, ConfirmAnswer::Allow)],
        ),
        (
            "macos allow_click_after_the_hold",
            &[(ALLOW_CLICK_AFTER, ConfirmAnswer::Allow)],
        ),
        (
            "macos second_show",
            &[
                (REJECT_CLICK, ConfirmAnswer::Reject),
                (ALLOW_CLICK, ConfirmAnswer::Allow),
            ],
        ),
        (
            "macos allow_click_100ms_after_the_key_window_again",
            &[(ALLOW_AFTER_KEY_AGAIN, ConfirmAnswer::Allow)],
        ),
    ];
    pub const SHOWS: u32 = 7;

    static SHOW: AtomicUsize = AtomicUsize::new(0);

    #[repr(C)]
    struct Queue {
        private: [u8; 0],
    }

    extern "C" {
        #[link_name = "_dispatch_main_q"]
        static MAIN_QUEUE: Queue;
        fn dispatch_async_f(
            queue: *const Queue,
            context: *mut c_void,
            work: extern "C" fn(*mut c_void),
        );
    }

    type Job = Box<dyn FnOnce() + Send>;

    extern "C" fn trampoline(context: *mut c_void) {
        // SAFETY: `context` is the box that `on_main` leaked for this one call.
        let job = unsafe { Box::from_raw(context as *mut Job) };
        let _ = catch_unwind(AssertUnwindSafe(job));
    }

    /// Runs `f` on the main thread inside the show `show`'s modal session and returns its
    /// value, or `None` when the session did not run it within 2 s or the show has changed.
    fn on_main<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let show = SHOW.load(Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        let job: Job = Box::new(move || {
            if SHOW.load(Ordering::SeqCst) == show {
                let _ = tx.send(f());
            }
        });
        let context = Box::into_raw(Box::new(job)) as *mut c_void;
        // SAFETY: the main queue lives as long as the process, and `trampoline` takes the box
        // back once.
        unsafe { dispatch_async_f(std::ptr::addr_of!(MAIN_QUEUE), context, trampoline) };
        rx.recv_timeout(Duration::from_secs(2)).ok()
    }

    /// # Safety
    /// Runs on the main thread.
    unsafe fn app() -> *mut AnyObject {
        msg_send![class!(NSApplication), sharedApplication]
    }

    /// # Safety
    /// `view` is null or an `NSView`, and the caller runs on the main thread.
    unsafe fn button(view: *mut AnyObject, tag: isize, depth: usize) -> *mut AnyObject {
        if view.is_null() || depth > 12 {
            return std::ptr::null_mut();
        }
        let is_button: bool = msg_send![view, isKindOfClass: class!(NSButton)];
        if is_button {
            let own: isize = msg_send![view, tag];
            if own == tag {
                return view;
            }
        }
        let subviews: *mut AnyObject = msg_send![view, subviews];
        if subviews.is_null() {
            return std::ptr::null_mut();
        }
        let count: usize = msg_send![subviews, count];
        for i in 0..count {
            let child: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            let found = button(child, tag, depth + 1);
            if !found.is_null() {
                return found;
            }
        }
        std::ptr::null_mut()
    }

    #[derive(Clone)]
    struct Mac;

    impl Ui<Control> for Mac {
        fn press(&self, control: Control) -> bool {
            on_main(move || {
                // SAFETY: this runs on the main thread inside the alert's modal session, and the
                // objects that it reads belong to the modal window, which lives until it ends.
                unsafe {
                    let app = app();
                    let window: *mut AnyObject = msg_send![app, modalWindow];
                    if window.is_null() {
                        return false;
                    }
                    let tag = match control {
                        Control::Click(tag) | Control::Action(tag) => tag,
                        Control::Again => {
                            let center: *mut AnyObject =
                                msg_send![class!(NSNotificationCenter), defaultCenter];
                            let name: &NSString = NSWindowDidBecomeKeyNotification;
                            let _: () =
                                msg_send![center, postNotificationName: name, object: window];
                            return true;
                        }
                    };
                    let content: *mut AnyObject = msg_send![window, contentView];
                    let found = button(content, tag, 0);
                    if found.is_null() {
                        return false;
                    }
                    let target: *mut AnyObject = msg_send![found, target];
                    let held = AnyClass::get(c"HalvoDialogHold").is_some_and(|hold| {
                        !target.is_null() && {
                            let is: bool = msg_send![target, isKindOfClass: hold];
                            is
                        }
                    });
                    println!("guard_hosted reading press: target_is_the_hold={held}");
                    match control {
                        Control::Click(_) => {
                            let _: () =
                                msg_send![found, performClick: std::ptr::null_mut::<AnyObject>()];
                            true
                        }
                        Control::Again => true,
                        Control::Action(_) => {
                            let action: Option<Sel> = msg_send![found, action];
                            let Some(action) = action else {
                                return false;
                            };
                            let sent: bool =
                                msg_send![app, sendAction: action, to: target, from: found];
                            sent
                        }
                    }
                }
            })
            .unwrap_or(false)
        }

        fn open(&self) -> bool {
            on_main(|| {
                // SAFETY: as in `press`.
                unsafe {
                    let window: *mut AnyObject = msg_send![app(), modalWindow];
                    !window.is_null() && {
                        let visible: bool = msg_send![window, isVisible];
                        visible
                    }
                }
            })
            .unwrap_or(false)
        }

        fn front(&self) -> String {
            on_main(|| {
                // SAFETY: as in `press`.
                unsafe {
                    let app = app();
                    let active: bool = msg_send![app, isActive];
                    let window: *mut AnyObject = msg_send![app, modalWindow];
                    let key = !window.is_null() && {
                        let key: bool = msg_send![window, isKeyWindow];
                        key
                    };
                    activate_app(app);
                    if !window.is_null() {
                        let _: () = msg_send![window, makeKeyAndOrderFront: std::ptr::null_mut::<AnyObject>()];
                    }
                    format!("before it: app_active={active} modal_window={} key={key}", !window.is_null())
                }
            })
            .unwrap_or_else(|| "the modal session ran nothing in 2 s".into())
        }

        fn close(&self) {
            let _ = on_main(|| {
                // SAFETY: abortModal is made for a call from outside the handling of an event, on
                // the main thread.
                unsafe {
                    let _: () = msg_send![app(), abortModal];
                }
            });
        }

        fn word(control: Control) -> String {
            match control {
                Control::Click(tag) => format!("the click call on {tag}"),
                Control::Action(tag) => format!("the action of {tag}"),
                Control::Again => "the notification that the window became key".into(),
            }
        }
    }

    fn active_now() -> bool {
        // SAFETY: two reads of the shared application's windows, made off the main thread
        // while the modal session runs, by a test on a hosted runner.
        unsafe {
            let app = app();
            let key: *mut AnyObject = msg_send![app, keyWindow];
            let modal: *mut AnyObject = msg_send![app, modalWindow];
            !key.is_null() && key == modal
        }
    }

    /// # Safety
    /// `app` is the shared application, and the caller runs on the main thread.
    unsafe fn activate_app(app: *mut AnyObject) {
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
        let running: *mut AnyObject = msg_send![class!(NSRunningApplication), currentApplication];
        if !running.is_null() {
            let _: bool = msg_send![running, activateWithOptions: 3usize];
        }
    }

    fn activate() {
        // SAFETY: this runs on the main thread, before any window of the target exists.
        unsafe {
            let app = app();
            let _: bool = msg_send![app, setActivationPolicy: 0isize];
            let _: () = msg_send![app, finishLaunching];
            activate_app(app);
        }
    }

    pub fn run(card: Card) {
        activate();
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            let dialog = System::new(handle);
            let mut verdicts = Vec::new();
            for (name, shows) in CASES {
                let verdict = case(&Mac, name, shows, &|| {
                    SHOW.fetch_add(1, Ordering::SeqCst);
                    dialog.show(&card, arm(), &|| on_screen(active_now()))
                });
                print(&verdict);
                verdicts.push(verdict);
            }
            end(&verdicts, CASES.len());
        });
        app.run(|_, _| {});
    }
}

#[cfg(windows)]
mod cases {
    use super::{arm, case, end, on_screen, print, Press, Show, Ui};
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::{Card, Dialog, System};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsWindowVisible, PostMessageW, SendMessageW, SetForegroundWindow, WA_ACTIVE,
        WM_ACTIVATE, WM_CLOSE, WM_USER,
    };

    const TDM_CLICK_BUTTON: u32 = WM_USER + 102;

    /// A press: the click message for a button id, or the message WM_CLOSE; or the message
    /// WM_ACTIVATE that the window became active again.
    #[derive(Clone, Copy)]
    pub enum Control {
        Click(usize),
        Close,
        Again,
    }

    const fn p(at: u64, control: Control, answers: bool) -> Press<Control> {
        Press {
            at,
            control,
            answers,
        }
    }

    const CLICK_100: &[Press<Control>] = &[
        p(100, Control::Click(100), false),
        p(800, Control::Click(100), true),
    ];
    const CLICK_101: &[Press<Control>] = &[
        p(100, Control::Click(101), false),
        p(800, Control::Click(101), true),
    ];
    const CLICK_2: &[Press<Control>] = &[
        p(100, Control::Click(2), false),
        p(800, Control::Click(2), true),
    ];
    const CLOSE: &[Press<Control>] = &[p(100, Control::Close, false), p(800, Control::Close, true)];
    const CLICK_100_AGAIN: &[Press<Control>] = &[
        p(100, Control::Click(100), false),
        p(550, Control::Click(100), false),
        p(1_250, Control::Click(100), true),
    ];
    const CLICK_100_AFTER: &[Press<Control>] = &[p(700, Control::Click(100), true)];
    const CLICK_100_AFTER_ACTIVE_AGAIN: &[Press<Control>] = &[
        p(700, Control::Again, false),
        p(800, Control::Click(100), false),
        p(1_400, Control::Click(100), true),
    ];

    type Case = (&'static str, &'static [Show<Control>]);
    const CASES: [Case; 8] = [
        (
            "windows click_100_at_100ms",
            &[(CLICK_100, ConfirmAnswer::Allow)],
        ),
        (
            "windows click_101_at_100ms",
            &[(CLICK_101, ConfirmAnswer::Reject)],
        ),
        (
            "windows click_2_cancel_at_100ms",
            &[(CLICK_2, ConfirmAnswer::Reject)],
        ),
        (
            "windows wm_close_at_100ms",
            &[(CLOSE, ConfirmAnswer::Reject)],
        ),
        (
            "windows click_100_at_100ms_and_550ms",
            &[(CLICK_100_AGAIN, ConfirmAnswer::Allow)],
        ),
        (
            "windows click_100_after_the_hold",
            &[(CLICK_100_AFTER, ConfirmAnswer::Allow)],
        ),
        (
            "windows second_show",
            &[
                (CLICK_101, ConfirmAnswer::Reject),
                (CLICK_100, ConfirmAnswer::Allow),
            ],
        ),
        (
            "windows click_100_100ms_after_active_again",
            &[(CLICK_100_AFTER_ACTIVE_AGAIN, ConfirmAnswer::Allow)],
        ),
    ];
    pub const SHOWS: u32 = 9;

    #[derive(Clone)]
    struct Win {
        title: HSTRING,
    }

    impl Win {
        fn window(&self) -> Option<HWND> {
            // SAFETY: the title is a string that lives for the call.
            unsafe { FindWindowW(PCWSTR::null(), &self.title) }.ok()
        }
    }

    impl Ui<Control> for Win {
        fn press(&self, control: Control) -> bool {
            let Some(window) = self.window() else {
                return false;
            };
            match control {
                Control::Click(id) => {
                    // SAFETY: `window` is the window that the call above found; a message to a
                    // window that has gone fails and does nothing.
                    let _ =
                        unsafe { SendMessageW(window, TDM_CLICK_BUTTON, Some(WPARAM(id)), None) };
                }
                Control::Close => {
                    // SAFETY: as above.
                    let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
                }
                Control::Again => {
                    // SAFETY: as above.
                    let _ = unsafe {
                        SendMessageW(window, WM_ACTIVATE, Some(WPARAM(WA_ACTIVE as usize)), None)
                    };
                }
            }
            true
        }

        fn open(&self) -> bool {
            // SAFETY: as in `press`; a window that has gone reads as not visible.
            self.window()
                .is_some_and(|window| unsafe { IsWindowVisible(window) }.as_bool())
        }

        fn front(&self) -> String {
            // SAFETY: as in `press`.
            let set = self
                .window()
                .is_some_and(|window| unsafe { SetForegroundWindow(window) }.as_bool());
            format!("set as the foreground window: {set}")
        }

        fn close(&self) {
            if let Some(window) = self.window() {
                // SAFETY: as in `press`.
                let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
        }

        fn word(control: Control) -> String {
            match control {
                Control::Click(id) => format!("the click message for {id}"),
                Control::Close => "WM_CLOSE".into(),
                Control::Again => "WM_ACTIVATE".into(),
            }
        }
    }

    pub fn run(card: Card) {
        let app = tauri::test::mock_app();
        let dialog = System::new(app.handle().clone());
        let ui = Win {
            title: HSTRING::from(card.title()),
        };
        let mut verdicts = Vec::new();
        for (name, shows) in CASES {
            let verdict = case(&ui, name, shows, &|| {
                dialog.show(&card, arm(), &|| {
                    // SAFETY: a read of the active window of this thread, the dialog's.
                    on_screen(!unsafe { GetActiveWindow() }.is_invalid())
                })
            });
            print(&verdict);
            verdicts.push(verdict);
        }
        end(&verdicts, CASES.len());
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod cases {
    pub const SHOWS: u32 = 0;

    pub fn run(card: halvo_coord_app::Card) {
        let _ = card;
        super::end(&[], 0);
    }
}
