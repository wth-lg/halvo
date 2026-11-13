//! The unit's own system dialog, built through `System::show` and read back through the
//! system's own objects, on a hosted runner of GitHub Actions alone (RD-79 E1). A control is
//! pressed by the system's own call for it, and no input event of the operating system is
//! posted. On every other machine the target shows nothing and prints one line. It takes no
//! argument and reads none, so a filter or a flag that a reader passes to the crate's tests
//! changes nothing. A case has a bound of 10 s: a case that reaches it closes its dialog and
//! fails. The last line counts the cases that ran. The early return with the one line is taken
//! only when a second read of the runner's two variables, made at that return, says that this
//! is no hosted runner; when it says that it is, the target fails (ruling RD-92).
//!
//! Named limits (ruling RD-92 (3)): the macOS cases queue the alert through Tauri's mock
//! runtime, whose `run_on_main_thread` sends a task to the mock loop on the main thread, and
//! not through the Wry runtime that the app hands to `System::new`, so the product's path to
//! the main thread runs in no test; and on macOS the target would end with the status 0 and no
//! case line if the mock runtime's `run` ever returned, which it does not at tauri 2.12.5.

#[path = "hosted/gate.rs"]
mod gate;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::{Class, Field, FieldFrom};
use halvo_coord_app::Card;
use std::time::Duration;

const OFF: &str = "hosted alone: not run on this machine";
const BOUND: Duration = Duration::from_secs(10);

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

/// One case's result: the failures that it found, none when it passed.
struct Verdict {
    name: &'static str,
    failures: Vec<String>,
}

fn print(verdict: &Verdict) {
    if verdict.failures.is_empty() {
        println!("dialog_hosted case {} PASS", verdict.name);
    } else {
        println!(
            "dialog_hosted case {} FAIL: {}",
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
        "dialog_hosted end: cases ran {} of {listed}, failed {failed}{}",
        verdicts.len(),
        if short {
            ", FAIL: not every case ran"
        } else {
            ""
        }
    );
    std::process::exit(if failed > 0 || short { 1 } else { 0 });
}

fn main() {
    match gate::start(&gate::env) {
        gate::Start::Run => {}
        gate::Start::Off => {
            println!("{OFF}");
            return;
        }
        gate::Start::Fail(line) => {
            println!("dialog_hosted {line}");
            std::process::exit(1);
        }
    }
    std::thread::spawn(|| {
        std::thread::sleep(BOUND * (cases::LIST + 3));
        println!("dialog_hosted watchdog FAIL: the cases did not end in time");
        std::process::exit(1);
    });
    cases::run(card());
}

#[cfg(target_os = "macos")]
mod cases {
    use super::{end, print, word, Verdict, BOUND};
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::{Card, Dialog, System};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, MainThreadMarker};
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Instant;

    pub const LIST: u32 = 4;
    const CASES: [(&str, ConfirmAnswer); LIST as usize] = [
        ("macos press_allow", ConfirmAnswer::Allow),
        ("macos press_reject", ConfirmAnswer::Reject),
        ("macos stop", ConfirmAnswer::Reject),
        ("macos abort", ConfirmAnswer::Reject),
    ];
    const CLOSABLE: usize = 1 << 1;

    static CASE: AtomicUsize = AtomicUsize::new(0);
    static ACTED: AtomicUsize = AtomicUsize::new(0);
    static BOUND_HIT: AtomicUsize = AtomicUsize::new(0);
    static REPORTS: AtomicUsize = AtomicUsize::new(0);
    static REPORTS_AT_ACT: AtomicUsize = AtomicUsize::new(usize::MAX);
    static REPORT_AT: Mutex<Option<Instant>> = Mutex::new(None);
    static VISIBLE: Mutex<Option<String>> = Mutex::new(None);
    static TEXTS: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));
    static READ: Mutex<Option<Reading>> = Mutex::new(None);

    #[derive(Clone, Default)]
    struct Reading {
        buttons: Vec<(String, isize, String)>,
        texts: Vec<String>,
        default_title: Option<String>,
        style_mask: usize,
        close_button: bool,
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

    /// # Safety
    /// `string` is null or an `NSString` that lives for the call.
    unsafe fn text(string: *mut AnyObject) -> String {
        if string.is_null() {
            return String::new();
        }
        let utf8: *const c_char = msg_send![string, UTF8String];
        if utf8.is_null() {
            return String::new();
        }
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }

    /// # Safety
    /// `view` is null or an `NSView`, and the caller runs on the main thread.
    unsafe fn walk(
        view: *mut AnyObject,
        depth: usize,
        out: &mut Reading,
        found: &mut Vec<*mut AnyObject>,
    ) {
        if view.is_null() || depth > 12 {
            return;
        }
        let is_button: bool = msg_send![view, isKindOfClass: class!(NSButton)];
        let is_field: bool = msg_send![view, isKindOfClass: class!(NSTextField)];
        if is_button {
            let title: *mut AnyObject = msg_send![view, title];
            let tag: isize = msg_send![view, tag];
            let key: *mut AnyObject = msg_send![view, keyEquivalent];
            out.buttons.push((text(title), tag, text(key)));
            found.push(view);
        } else if is_field {
            let value: *mut AnyObject = msg_send![view, stringValue];
            out.texts.push(text(value));
        }
        let subviews: *mut AnyObject = msg_send![view, subviews];
        if subviews.is_null() {
            return;
        }
        let count: usize = msg_send![subviews, count];
        for i in 0..count {
            let child: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            walk(child, depth + 1, out, found);
        }
    }

    fn act(generation: usize) {
        if CASE.load(Ordering::SeqCst) != generation || ACTED.load(Ordering::SeqCst) == generation {
            return;
        }
        if MainThreadMarker::new().is_none() {
            return;
        }
        let (name, _) = CASES[generation - 1];
        let (reject, allow) = TEXTS.lock().unwrap_or_else(PoisonError::into_inner).clone();
        // SAFETY: this runs on the main thread inside the alert's modal session, and every
        // object that it reads belongs to the modal window, which lives until the session ends.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            if app.is_null() {
                return;
            }
            let window: *mut AnyObject = msg_send![app, modalWindow];
            if window.is_null() {
                return;
            }
            ACTED.store(generation, Ordering::SeqCst);
            REPORTS_AT_ACT.store(REPORTS.load(Ordering::SeqCst), Ordering::SeqCst);
            let visible: bool = msg_send![window, isVisible];
            let at = *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner);
            *VISIBLE.lock().unwrap_or_else(PoisonError::into_inner) = Some(match (visible, at) {
                (true, Some(at)) => at.elapsed().as_millis().to_string(),
                (false, _) => "not visible at the first read".into(),
                (true, None) => "no report before the first read".into(),
            });
            let mut reading = Reading::default();
            let mut found = Vec::new();
            let content: *mut AnyObject = msg_send![window, contentView];
            walk(content, 0, &mut reading, &mut found);
            let cell: *mut AnyObject = msg_send![window, defaultButtonCell];
            if !cell.is_null() {
                let title: *mut AnyObject = msg_send![cell, title];
                reading.default_title = Some(text(title));
            }
            reading.style_mask = msg_send![window, styleMask];
            let close: *mut AnyObject = msg_send![window, standardWindowButton: 0usize];
            reading.close_button = !close.is_null();
            *READ.lock().unwrap_or_else(PoisonError::into_inner) = Some(reading.clone());
            let press = |wanted: &str| {
                let at = reading
                    .buttons
                    .iter()
                    .position(|(title, _, _)| title == wanted);
                if let Some(i) = at {
                    let _: () =
                        msg_send![found[i], performClick: std::ptr::null_mut::<AnyObject>()];
                }
            };
            match name {
                "macos press_allow" => press(&allow),
                "macos press_reject" => press(&reject),
                "macos stop" => {
                    let _: () = msg_send![app, stopModal];
                }
                _ => {
                    let _: () = msg_send![app, abortModal];
                }
            }
        }
    }

    fn close_at_bound(generation: usize) {
        if CASE.load(Ordering::SeqCst) != generation
            || BOUND_HIT.swap(generation, Ordering::SeqCst) == generation
            || MainThreadMarker::new().is_none()
        {
            return;
        }
        // SAFETY: this runs on the main thread, and abortModal is made for a call from outside
        // the handling of an event.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            if !app.is_null() {
                let _: () = msg_send![app, abortModal];
            }
        }
    }

    extern "C" fn poke(context: *mut c_void) {
        if let Err(panic) = std::panic::catch_unwind(|| act(context as usize)) {
            let why = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            println!("dialog_hosted read panicked: {why}");
        }
    }

    extern "C" fn poke_bound(context: *mut c_void) {
        let _ = std::panic::catch_unwind(|| close_at_bound(context as usize));
    }

    fn dispatch(generation: usize, work: extern "C" fn(*mut c_void)) {
        // SAFETY: the main queue lives as long as the process, the context is a number and no
        // pointer, and `work` takes that number alone.
        unsafe {
            dispatch_async_f(
                std::ptr::addr_of!(MAIN_QUEUE),
                generation as *mut c_void,
                work,
            );
        }
    }

    fn check(
        card: &Card,
        expected: ConfirmAnswer,
        answer: ConfirmAnswer,
        generation: usize,
    ) -> Vec<String> {
        let mut failures = Vec::new();
        if BOUND_HIT.load(Ordering::SeqCst) == generation {
            failures.push(format!(
                "the case reached its bound of {} s and its dialog was closed",
                BOUND.as_secs()
            ));
        }
        if answer != expected {
            failures.push(format!(
                "the answer was {}, not {}",
                word(answer),
                word(expected)
            ));
        }
        let reports = REPORTS.load(Ordering::SeqCst);
        if reports != 1 {
            failures.push(format!(
                "the report on screen came {reports} times, not once"
            ));
        }
        let before = REPORTS_AT_ACT.load(Ordering::SeqCst);
        if before != 1 {
            failures.push(format!(
                "at the press the report had come {before} times, not once"
            ));
        }
        let Some(read) = READ.lock().unwrap_or_else(PoisonError::into_inner).clone() else {
            failures.push("the alert was never read".into());
            return failures;
        };
        let mut buttons = read.buttons.clone();
        buttons.sort_by_key(|(_, tag, _)| *tag);
        let held: Vec<(&str, isize)> = buttons
            .iter()
            .map(|(t, tag, _)| (t.as_str(), *tag))
            .collect();
        if held != [(card.reject(), 1000), (card.allow(), 1001)] {
            failures.push(format!("the buttons by their responses were {held:?}"));
        }
        if read
            .buttons
            .iter()
            .any(|(title, _, key)| title == "Cancel" || key == "\u{1b}")
        {
            failures.push("a button is titled Cancel or holds the Escape key".into());
        }
        if !read.texts.iter().any(|t| t == card.title()) {
            failures.push(format!(
                "no text of the alert is the card's title; texts {:?}",
                read.texts
            ));
        }
        if !read.texts.iter().any(|t| *t == card.text()) {
            failures.push(format!(
                "no text of the alert is the card's text; texts {:?}",
                read.texts
            ));
        }
        if read
            .buttons
            .iter()
            .any(|(title, _, key)| key == "\r" && title != card.reject())
        {
            failures.push("a button other than the rejecting one holds the Return key".into());
        }
        if let Some(title) = read.default_title.as_deref().filter(|t| !t.is_empty()) {
            if title != card.reject() {
                failures.push(format!("the window's default button is {title:?}"));
            }
        }
        if read.close_button || read.style_mask & CLOSABLE != 0 {
            failures.push(format!(
                "the alert's window has a close control: button={} style_mask={:#x}",
                read.close_button, read.style_mask
            ));
        }
        let keys: Vec<&str> = read.buttons.iter().map(|(_, _, k)| k.as_str()).collect();
        println!(
            "dialog_hosted reading {}: buttons={:?} keys={:?} default={} style_mask={:#x} close_button={} texts={}",
            CASES[generation - 1].0,
            held,
            keys,
            read.default_title.as_deref().map_or("no reading".to_string(), |t| format!("{t:?}")),
            read.style_mask,
            read.close_button,
            read.texts.len()
        );
        println!(
            "dialog_hosted timing {}: report_to_visible_ms={}",
            CASES[generation - 1].0,
            VISIBLE
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .unwrap_or_else(|| "no read".into())
        );
        failures
    }

    fn case<R: tauri::Runtime>(dialog: &System<R>, card: &Card, generation: usize) -> Verdict {
        let (name, expected) = CASES[generation - 1];
        *TEXTS.lock().unwrap_or_else(PoisonError::into_inner) =
            (card.reject().to_owned(), card.allow().to_owned());
        *READ.lock().unwrap_or_else(PoisonError::into_inner) = None;
        REPORTS.store(0, Ordering::SeqCst);
        REPORTS_AT_ACT.store(usize::MAX, Ordering::SeqCst);
        *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *VISIBLE.lock().unwrap_or_else(PoisonError::into_inner) = None;
        CASE.store(generation, Ordering::SeqCst);
        let done = Arc::new(AtomicBool::new(false));
        let poker = std::thread::spawn({
            let done = done.clone();
            move || {
                let start = Instant::now();
                while !done.load(Ordering::SeqCst) {
                    if start.elapsed() > BOUND {
                        dispatch(generation, poke_bound);
                    } else {
                        dispatch(generation, poke);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        });
        let answer = dialog.show(card, &|| {
            REPORTS.fetch_add(1, Ordering::SeqCst);
            REPORT_AT
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_with(Instant::now);
        });
        done.store(true, Ordering::SeqCst);
        let _ = poker.join();
        Verdict {
            name,
            failures: check(card, expected, answer, generation),
        }
    }

    pub fn run(card: Card) {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            let dialog = System::new(handle);
            let mut verdicts = Vec::new();
            for generation in 1..=CASES.len() {
                let verdict = case(&dialog, &card, generation);
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
    use super::{end, print, word, Verdict, BOUND};
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::{Card, Dialog, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Instant;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsWindowVisible, PostMessageW, SendMessageW, WM_CLOSE, WM_USER,
    };

    pub const LIST: u32 = 5;
    const TDM_CLICK_BUTTON: u32 = WM_USER + 102;
    const CLOSE: usize = 0;
    const CASES: [(&str, usize, ConfirmAnswer); LIST as usize] = [
        ("windows click_100", 100, ConfirmAnswer::Allow),
        ("windows click_101", 101, ConfirmAnswer::Reject),
        ("windows click_2_cancel", 2, ConfirmAnswer::Reject),
        ("windows wm_close", CLOSE, ConfirmAnswer::Reject),
        ("windows click_1_no_such_button", 1, ConfirmAnswer::Reject),
    ];

    static REPORTS: AtomicUsize = AtomicUsize::new(0);
    static REPORT_AT: Mutex<Option<Instant>> = Mutex::new(None);

    fn case<R: tauri::Runtime>(
        dialog: &System<R>,
        card: &Card,
        (name, id, expected): (&'static str, usize, ConfirmAnswer),
    ) -> Verdict {
        REPORTS.store(0, Ordering::SeqCst);
        *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let title = HSTRING::from(card.title());
        let done = Arc::new(AtomicBool::new(false));
        let poker = std::thread::spawn({
            let done = done.clone();
            move || {
                let start = Instant::now();
                let mut at_act = None;
                let mut timing = String::from("no read");
                let mut bound = false;
                while !done.load(Ordering::SeqCst) {
                    // SAFETY: the title is a string that lives for the call.
                    let found = unsafe { FindWindowW(PCWSTR::null(), &title) };
                    if let Ok(window) = found {
                        // SAFETY: `window` is the window that the call above found; a window that has gone
                        // reads as not visible.
                        let visible = unsafe { IsWindowVisible(window) }.as_bool();
                        if at_act.is_none() && !visible && start.elapsed() <= BOUND {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            continue;
                        }
                        if at_act.is_none() {
                            at_act = Some(REPORTS.load(Ordering::SeqCst));
                            let at = *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner);
                            timing = match at {
                                Some(at) if visible => at.elapsed().as_millis().to_string(),
                                Some(_) => "not visible at the bound".into(),
                                None => "no report before the window was found".into(),
                            };
                            if id == CLOSE {
                                // SAFETY: `window` is the window that the call above found; a
                                // message to a window that has gone fails and does nothing.
                                let _ = unsafe {
                                    PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0))
                                };
                            } else {
                                // SAFETY: as above.
                                let _ = unsafe {
                                    SendMessageW(window, TDM_CLICK_BUTTON, Some(WPARAM(id)), None)
                                };
                            }
                        } else if !bound && start.elapsed() > BOUND {
                            bound = true;
                            // SAFETY: as above.
                            let _ = unsafe {
                                PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0))
                            };
                        }
                    } else if start.elapsed() > BOUND {
                        bound = true;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                (at_act, bound, timing)
            }
        });
        let answer = dialog.show(card, &|| {
            REPORTS.fetch_add(1, Ordering::SeqCst);
            REPORT_AT
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_with(Instant::now);
        });
        done.store(true, Ordering::SeqCst);
        let (at_act, bound, timing) = poker
            .join()
            .unwrap_or((None, true, String::from("no read")));
        println!("dialog_hosted timing {name}: report_to_visible_ms={timing}");
        let mut failures = Vec::new();
        if bound {
            failures.push(format!(
                "the case reached its bound of {} s",
                BOUND.as_secs()
            ));
        }
        if answer != expected {
            failures.push(format!(
                "the answer was {}, not {}",
                word(answer),
                word(expected)
            ));
        }
        let reports = REPORTS.load(Ordering::SeqCst);
        if reports != 1 {
            failures.push(format!(
                "the report on screen came {reports} times, not once"
            ));
        }
        if at_act != Some(1) {
            failures.push(format!(
                "at the press the report had come {at_act:?} times, not once"
            ));
        }
        Verdict { name, failures }
    }

    pub fn run(card: Card) {
        let app = tauri::test::mock_app();
        let dialog = System::new(app.handle().clone());
        let mut verdicts = Vec::new();
        for each in CASES {
            let verdict = case(&dialog, &card, each);
            print(&verdict);
            verdicts.push(verdict);
        }
        end(&verdicts, CASES.len());
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod cases {
    pub const LIST: u32 = 0;

    pub fn run(card: halvo_coord_app::Card) {
        let _ = card;
        super::end(&[], 0);
    }
}
