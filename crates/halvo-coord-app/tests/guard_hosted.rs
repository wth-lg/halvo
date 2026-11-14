//! The hold of the unit's own system dialog (ruling RD-91 item 7), built through `System::show`
//! with the product's `arm_ms`, on a hosted runner of GitHub Actions alone, in the form of the
//! target `dialog_hosted` (RD-79 E1): a control is pressed by the system's own call for it, and
//! no input event of the operating system is posted. Each press is timed from the dialog's report
//! on screen, which the dialog makes when its window became the active window. A press of each
//! control 100 ms after the report leaves the dialog open; a press after the hold answers; a
//! refused press starts the hold anew; a second show holds again; a press 100 ms after the
//! window became active again leaves it open; and at each report the dialog's window was the
//! active window. No show is helped (ruling RD-103 item 6): a case brings a dialog to the front
//! only when it names that, and none does; a show that has not reported itself in 3 s with no
//! press fails, and is then pressed once so that its lines go on. The end line counts the uses of
//! the front before a report, the reports made at a press and the retries of a last press; the
//! first two must be 0. The target makes its app finish launching and become the active app at
//! its start, as the app is when a person asks for the dialog. On macOS the target's own
//! observers print the pair (the app active, the alert's window key) at each of the four notices,
//! each press prints the pair and the modal fact, and each show prints which reading made its
//! report. On macOS a press is the button's `performClick:`, the
//! system's click call, or the button's action sent through the application, as a key equivalent
//! sends it; on Windows a press is the message TDM_CLICK_BUTTON for the allowing id, the rejecting
//! id and the cancel response that the Escape key and the close control give, or the message
//! WM_CLOSE. On every other machine the target shows nothing and prints one line, with the start
//! and the end of `dialog_hosted`. A case has a bound of 10 s for each show: a show that reaches
//! it is closed and its case fails. The last line counts the cases that ran.

#[cfg(target_os = "macos")]
#[path = "hosted/activation.rs"]
mod activation;
#[path = "hosted/gate.rs"]
mod gate;
#[path = "hosted/schedule.rs"]
mod schedule;

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
const UNHELPED: Duration = Duration::from_secs(3);
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

/// One press of a show: `at` milliseconds from `from`, by `control`; `answers` for the press that
/// must close the dialog, and for no other. A press timed from the press before it that answers
/// nothing must fall inside that press's hold, and its case fails with "the schedule slipped"
/// when the two ran `arm_ms` or more apart.
#[derive(Clone, Copy)]
struct Press<C: 'static> {
    at: u64,
    from: schedule::From,
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
static PRESSED_BEFORE_REPORT: AtomicBool = AtomicBool::new(false);
static HELPED: AtomicUsize = AtomicUsize::new(0);
static PRESS_MOMENTS: AtomicUsize = AtomicUsize::new(0);
static RETRIES: AtomicUsize = AtomicUsize::new(0);
static SHOW_AT: Mutex<Option<Instant>> = Mutex::new(None);
static LABEL: Mutex<String> = Mutex::new(String::new());

/// The label of the show that runs and the milliseconds since it began.
fn now_in_show() -> (String, u128) {
    let label = LABEL.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let at = SHOW_AT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .map_or(0, |at| at.elapsed().as_millis());
    (label, at)
}

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

fn begin_show(label: &str) {
    *LABEL.lock().unwrap_or_else(PoisonError::into_inner) = label.to_string();
    *SHOW_AT.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    PRESSED_BEFORE_REPORT.store(false, Ordering::SeqCst);
    REPORTS.store(0, Ordering::SeqCst);
    ACTIVE_AT_REPORT.store(false, Ordering::SeqCst);
    *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner) = None;
    DONE.store(false, Ordering::SeqCst);
}

/// The system's side of a case: a press of a control, whether the dialog is open, a move of the
/// dialog to the front with a reading of what was active before it, and its close at the bound.
trait Ui<C> {
    /// Presses `control`: the time at which the press ran on the dialog's thread, `None` when it
    /// found no control to press.
    fn press(&self, control: C) -> Option<Instant>;
    fn open(&self) -> bool;
    fn front(&self) -> String;
    fn close(&self);
    fn word(control: C) -> String;
    /// The reading that made a report that no press came before: a notice or the repeating one.
    fn kind(&self) -> &'static str;
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
fn drive<C: Copy, U: Ui<C>>(ui: &U, label: &str, presses: &[Press<C>], help: bool) -> Vec<String> {
    let start = Instant::now();
    let mut failures = Vec::new();
    let mut fronts = 0;
    while help && !wait(REPORT_WAIT, || reported().is_some()) && start.elapsed() < FRONT_LIMIT {
        fronts += 1;
        HELPED.fetch_add(1, Ordering::SeqCst);
        let state = ui.front();
        println!("guard_hosted reading {label}: no report yet, brought to the front {fronts} times; {state}");
    }
    if !wait(UNHELPED, || reported().is_some()) {
        failures.push(format!(
            "no report in {} s with no press and no help",
            UNHELPED.as_secs()
        ));
        if let Some(first) = presses.first() {
            PRESSED_BEFORE_REPORT.store(true, Ordering::SeqCst);
            ui.press(first.control);
            wait(Duration::from_secs(2), || reported().is_some());
        }
    }
    let Some(base) = reported() else {
        failures.push("the dialog never reported itself on screen".into());
        while !DONE.load(Ordering::SeqCst) && start.elapsed() < BOUND {
            ui.close();
            std::thread::sleep(arm() + Duration::from_millis(100));
        }
        return failures;
    };
    let mut previous = None;
    for press in presses {
        sleep_until(schedule::due(base, previous, press.from, press.at));
        if DONE.load(Ordering::SeqCst) {
            failures.push(format!(
                "the dialog had closed before the press at {} ms",
                press.at
            ));
            break;
        }
        let ran = ui.press(press.control);
        let Some(ran) = ran else {
            failures.push(format!(
                "the press at {} ms by {} found no control to press",
                press.at,
                U::word(press.control)
            ));
            break;
        };
        if let (schedule::From::Previous, Some(before)) = (press.from, previous) {
            let gap = ran.saturating_duration_since(before).as_millis();
            println!(
                "guard_hosted schedule {label}: the press before ran at +{} ms, this one at +{} ms, gap {gap} ms",
                before.saturating_duration_since(base).as_millis(),
                ran.saturating_duration_since(base).as_millis()
            );
            if !press.answers {
                if let Some(line) = schedule::slipped(before, ran, arm()) {
                    failures.push(line);
                    break;
                }
            }
        }
        previous = Some(ran);
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
        RETRIES.fetch_add(1, Ordering::SeqCst);
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
        "guard_hosted end: cases ran {} of {listed}, failed {failed}, helped {}, moments at a press {}, retries {}{}",
        verdicts.len(),
        HELPED.load(Ordering::SeqCst),
        PRESS_MOMENTS.load(Ordering::SeqCst),
        RETRIES.load(Ordering::SeqCst),
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
        begin_show(&label);
        let driver = std::thread::spawn({
            let ui = ui.clone();
            let label = label.clone();
            move || drive(&ui, &label, presses, false)
        });
        let answer = show();
        DONE.store(true, Ordering::SeqCst);
        let kind = if PRESSED_BEFORE_REPORT.load(Ordering::SeqCst) {
            PRESS_MOMENTS.fetch_add(1, Ordering::SeqCst);
            "a press"
        } else {
            ui.kind()
        };
        println!("guard_hosted reading {label}: report made by {kind}");
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
    use super::schedule::From;
    use super::{arm, case, end, now_in_show, on_screen, print, Press, Show, Ui};
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::{Card, Dialog, System};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject, Sel};
    use objc2::{class, define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplicationDidBecomeActiveNotification, NSApplicationDidResignActiveNotification,
        NSWindowDidBecomeKeyNotification, NSWindowDidResignKeyNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSPoint, NSRect, NSSize};
    use std::ffi::c_void;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
    use std::sync::{mpsc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    const REJECT: isize = 1000;
    const ALLOW: isize = 1001;

    /// A button by its response, pressed by its click call or by its action sent through the
    /// application, or a real resign and return: a panel of the target takes key, and the alert's
    /// window takes it back.
    #[derive(Clone, Copy)]
    pub enum Control {
        Click(isize),
        Action(isize),
        Again,
    }

    const fn p(at: u64, control: Control, answers: bool) -> Press<Control> {
        Press {
            at,
            from: From::Report,
            control,
            answers,
        }
    }

    const fn after(at: u64, control: Control, answers: bool) -> Press<Control> {
        Press {
            at,
            from: From::Previous,
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
        p(300, Control::Click(ALLOW), false),
        after(250, Control::Click(ALLOW), false),
        after(750, Control::Click(ALLOW), true),
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
    const ALLOW_WITH_NO_PRESS: &[Press<Control>] = &[p(2_000, Control::Click(ALLOW), true)];

    type Case = (&'static str, &'static [Show<Control>]);
    const CASES: [Case; 7] = [
        (
            "macos reject_action_at_100ms",
            &[(REJECT_ACTION, ConfirmAnswer::Reject)],
        ),
        (
            "macos allow_action_at_100ms",
            &[(ALLOW_ACTION, ConfirmAnswer::Allow)],
        ),
        (
            "macos allow_click_at_300ms_and_250ms_after_it",
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
        (
            "macos report_with_no_press_and_no_help",
            &[(ALLOW_WITH_NO_PRESS, ConfirmAnswer::Allow)],
        ),
    ];
    pub const SHOWS: u32 = 8;

    static SHOW: AtomicUsize = AtomicUsize::new(0);
    static NOTICE_BOTH: Mutex<Option<Instant>> = Mutex::new(None);
    static PANEL: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());

    /// The pair now, read on the main thread: whether the app is active and whether the modal
    /// window is its key window, and whether a modal window exists.
    ///
    /// # Safety
    /// Runs on the main thread.
    unsafe fn pair_now() -> (bool, bool, bool) {
        let app = app();
        let active: bool = msg_send![app, isActive];
        let window: *mut AnyObject = msg_send![app, modalWindow];
        let key = !window.is_null() && {
            let key: bool = msg_send![window, isKeyWindow];
            key
        };
        (active, key, !window.is_null())
    }

    fn notice(name: &str, note: &NSNotification, become_: bool) {
        // SAFETY: an observer's method runs on the main thread.
        let (active, key, modal) = unsafe { pair_now() };
        let object = note.object().map_or(std::ptr::null(), |o| {
            Retained::as_ptr(&o).cast::<AnyObject>()
        });
        // SAFETY: as above.
        let window: *mut AnyObject = unsafe { msg_send![app(), modalWindow] };
        let (label, at) = now_in_show();
        println!(
            "guard_hosted notice {label} +{at} ms: {name} active={active} key={key} modal_window={modal} object_is_the_alert={}",
            !window.is_null() && object == window.cast_const()
        );
        if become_ && active && key {
            NOTICE_BOTH
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_with(Instant::now);
        }
    }

    define_class!(
        // SAFETY: NSObject has no requirement of a subclass, and `Watch` implements no `Drop`.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "GuardHostedWatch"]
        struct Watch;

        impl Watch {
            #[unsafe(method(becameKey:))]
            fn became_key(&self, note: &NSNotification) {
                notice("became key", note, true);
            }

            #[unsafe(method(becameActive:))]
            fn became_active(&self, note: &NSNotification) {
                notice("became active", note, true);
            }

            #[unsafe(method(resignedKey:))]
            fn resigned_key(&self, note: &NSNotification) {
                notice("resigned key", note, false);
            }

            #[unsafe(method(resignedActive:))]
            fn resigned_active(&self, note: &NSNotification) {
                notice("resigned active", note, false);
            }
        }
    );

    fn watch(mtm: MainThreadMarker) {
        // SAFETY: `init` of NSObject on a newly allocated object.
        let watch: Retained<Watch> = unsafe { msg_send![Watch::alloc(mtm), init] };
        let center = NSNotificationCenter::defaultCenter();
        let target: &AnyObject = &watch;
        // SAFETY: `watch` answers each selector with a notification and lives for the process.
        unsafe {
            for (selector, name) in [
                (sel!(becameKey:), NSWindowDidBecomeKeyNotification),
                (
                    sel!(becameActive:),
                    NSApplicationDidBecomeActiveNotification,
                ),
                (sel!(resignedKey:), NSWindowDidResignKeyNotification),
                (
                    sel!(resignedActive:),
                    NSApplicationDidResignActiveNotification,
                ),
            ] {
                center.addObserver_selector_name_object(target, selector, Some(name), None);
            }
        }
        std::mem::forget(watch);
    }

    /// The panel of the target that takes key from the alert, made once.
    ///
    /// # Safety
    /// Runs on the main thread.
    unsafe fn panel() -> *mut AnyObject {
        let made = PANEL.load(Ordering::SeqCst);
        if !made.is_null() {
            return made;
        }
        let rect = NSRect::new(NSPoint::new(40.0, 40.0), NSSize::new(240.0, 80.0));
        let panel: *mut AnyObject = msg_send![class!(NSPanel), alloc];
        let panel: *mut AnyObject = msg_send![panel, initWithContentRect: rect, styleMask: 1usize, backing: 2usize, defer: false];
        let _: () = msg_send![panel, setWorksWhenModal: true];
        let _: () = msg_send![panel, setReleasedWhenClosed: false];
        let _: () = msg_send![panel, setBecomesKeyOnlyIfNeeded: false];
        PANEL.store(panel, Ordering::SeqCst);
        panel
    }

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
        fn press(&self, control: Control) -> Option<Instant> {
            on_main(move || {
                // SAFETY: this runs on the main thread inside the alert's modal session, and the
                // objects that it reads belong to the modal window, which lives until it ends.
                unsafe {
                    let app = app();
                    let window: *mut AnyObject = msg_send![app, modalWindow];
                    if window.is_null() {
                        return None;
                    }
                    let (label, at) = now_in_show();
                    let tag = match control {
                        Control::Click(tag) | Control::Action(tag) => tag,
                        Control::Again => {
                            let panel = panel();
                            let _: () = msg_send![panel, makeKeyAndOrderFront: std::ptr::null_mut::<AnyObject>()];
                            let panel_key: bool = msg_send![panel, isKeyWindow];
                            let _: () = msg_send![window, makeKeyWindow];
                            let alert_key: bool = msg_send![window, isKeyWindow];
                            let _: () = msg_send![panel, orderOut: std::ptr::null_mut::<AnyObject>()];
                            println!("guard_hosted again {label} +{at} ms: the panel took key={panel_key}, the alert took it back={alert_key}");
                            return Some(Instant::now());
                        }
                    };
                    let (active, key, _) = pair_now();
                    let content: *mut AnyObject = msg_send![window, contentView];
                    let found = button(content, tag, 0);
                    if found.is_null() {
                        return None;
                    }
                    let target: *mut AnyObject = msg_send![found, target];
                    let held = AnyClass::get(c"HalvoDialogHold").is_some_and(|hold| {
                        !target.is_null() && {
                            let is: bool = msg_send![target, isKindOfClass: hold];
                            is
                        }
                    });
                    let own: *mut AnyObject = msg_send![found, window];
                    println!(
                        "guard_hosted press {label} +{at} ms: {} active={active} key={key} modal={} target_is_the_hold={held}",
                        Mac::word(control),
                        own == window
                    );
                    let ran = Instant::now();
                    match control {
                        Control::Click(_) => {
                            let _: () =
                                msg_send![found, performClick: std::ptr::null_mut::<AnyObject>()];
                            Some(ran)
                        }
                        Control::Again => Some(ran),
                        Control::Action(_) => {
                            let action: Option<Sel> = msg_send![found, action];
                            let action = action?;
                            let sent: bool =
                                msg_send![app, sendAction: action, to: target, from: found];
                            sent.then_some(ran)
                        }
                    }
                }
            })
            .flatten()
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
                Control::Again => "a panel's taking of key and its return to the alert".into(),
            }
        }

        fn kind(&self) -> &'static str {
            let notice = *NOTICE_BOTH.lock().unwrap_or_else(PoisonError::into_inner);
            let report = super::reported();
            match (notice, report) {
                (Some(notice), Some(report)) if notice <= report => "a become notice",
                (_, Some(_)) => "the repeating reading",
                _ => "no report",
            }
        }
    }

    /// Whether the alert's window is the active window at the report: the modal session had begun
    /// (read here, off the main thread, so that a report made before it is seen), and the app is
    /// active and the modal window its key window (read on the main thread).
    fn active_now() -> bool {
        // SAFETY: one read of a pointer of the shared application off the main thread, by a test
        // on a hosted runner.
        let begun = unsafe {
            let modal: *mut AnyObject = msg_send![app(), modalWindow];
            !modal.is_null()
        };
        // SAFETY: `on_main` runs the read on the main thread.
        let pair = on_main(|| unsafe { pair_now() });
        begun && pair.is_some_and(|(active, key, _)| active && key)
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

    /// Makes the target's app active once at its start and waits for it before the first case
    /// (ruling RD-107): asks, and while the app does not read active on the main thread, lets the
    /// main run loop run for a second and asks again, for at most 20 s. Not active then, the target
    /// runs no case, prints its inconclusive line with the pair as read and ends as failed.
    fn activate() {
        // SAFETY: this runs on the main thread, before any window of the target exists.
        unsafe {
            let app = app();
            let _: bool = msg_send![app, setActivationPolicy: 0isize];
            let _: () = msg_send![app, finishLaunching];
        }
        let waited = super::activation::wait(
            // SAFETY: as above.
            || unsafe { activate_app(app()) },
            // SAFETY: as above.
            || unsafe {
                let active: bool = msg_send![app(), isActive];
                active
            },
            |every| {
                // SAFETY: the main thread's own run loop, run until a date `every` from now.
                unsafe {
                    let date: *mut AnyObject = msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: every.as_secs_f64()];
                    let run_loop: *mut AnyObject = msg_send![class!(NSRunLoop), currentRunLoop];
                    let _: () = msg_send![run_loop, runUntilDate: date];
                }
            },
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        match waited {
            super::activation::Waited::Active { asks } => {
                println!("guard_hosted start: the app read active after {asks} asks");
            }
            super::activation::Waited::Inactive { asks } => {
                // SAFETY: as above.
                let (active, key) = unsafe {
                    let app = app();
                    let active: bool = msg_send![app, isActive];
                    let key: *mut AnyObject = msg_send![app, keyWindow];
                    (active, !key.is_null())
                };
                println!(
                    "guard_hosted inconclusive: the app did not become active within 20 s; active={active} key_window={key} asks={asks}"
                );
                std::process::exit(1);
            }
        }
    }

    pub fn run(card: Card) {
        activate();
        if let Some(mtm) = MainThreadMarker::new() {
            watch(mtm);
        }
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            let dialog = System::new(handle);
            let mut verdicts = Vec::new();
            for (name, shows) in CASES {
                let verdict = case(&Mac, name, shows, &|| {
                    *NOTICE_BOTH.lock().unwrap_or_else(PoisonError::into_inner) = None;
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
    use super::schedule::From;
    use super::{arm, case, end, now_in_show, on_screen, print, Press, Show, Ui};
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::{Card, Dialog, System};
    use std::time::Instant;
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
            from: From::Report,
            control,
            answers,
        }
    }

    const fn after(at: u64, control: Control, answers: bool) -> Press<Control> {
        Press {
            at,
            from: From::Previous,
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
        p(300, Control::Click(100), false),
        after(250, Control::Click(100), false),
        after(750, Control::Click(100), true),
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
            "windows click_100_at_300ms_and_250ms_after_it",
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
        fn press(&self, control: Control) -> Option<Instant> {
            let (label, at) = now_in_show();
            println!(
                "guard_hosted press {label} +{at} ms: {}",
                Win::word(control)
            );
            let window = self.window()?;
            let ran = Instant::now();
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
            Some(ran)
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

        fn kind(&self) -> &'static str {
            "the activation at TDN_CREATED or WM_ACTIVATE"
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
