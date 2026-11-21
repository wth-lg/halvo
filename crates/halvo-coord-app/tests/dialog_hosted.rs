//! The unit's own system dialog, built through `System::show` and read back through the
//! system's own objects, on a hosted runner of GitHub Actions alone (RD-79 E1). A control is
//! pressed by the system's own call for it, and no input event of the operating system is
//! posted. On every other machine the target shows nothing and prints one line. It takes no
//! argument and reads none, so a filter or a flag that a reader passes to the crate's tests
//! changes nothing. A case has a bound of 10 s: a case that reaches it closes its dialog and
//! fails. The last line counts the cases that ran. The early return with the one line is taken
//! only when a second read of the runner's two variables, made at that return, says that this
//! is no hosted runner; when it says that it is, the target fails (ruling RD-92). Every row
//! waits for the dialog's report on screen before it acts, and a row that presses a control
//! presses it after the hold of `arm_ms` from that report (ruling RD-91 item 7); on macOS the
//! target makes its app finish launching and become the active app, as the app is when a person
//! asks for the dialog, waits at its start until the app reads active, as guard_hosted does
//! (ruling RD-107, ruling RD-113 item 2), and otherwise runs no case and ends with its
//! inconclusive line, asks for its first show only after the sign that the test runtime's run
//! loop runs, and otherwise, after 10 s or when the sign's channel closed with no sign, runs no
//! case and fails by its own line (ruling RD-146 (4)), and brings an alert that has not reported
//! itself to the front once a second, with a line that says what was active before it; and on
//! Windows a dialog that has not reported itself in 3 s is brought to the foreground once, with
//! a line that says so.
//!
//! After its cases it reads every bound of the native card on the screen (ruling RD-90 items 1
//! and 9): a card of `CARD_LINES` lines, an empty line between two fields, each of `CARD_COLUMNS`
//! columns filled with the widest letter, with wide characters and with letters that each carry a
//! combining mark, is shown whole when its window and both controls lie inside the screen's
//! visible area and inside `SMALL`, the smallest supported screen less its menu bar and its dock
//! or task bar, and, on macOS, when the message's text field shows all of its text and no view
//! cuts it. Each bound case prints its reading and its time to display, and one line counts them;
//! a bound case that fails makes the target end with the status 1 after its end line.
//!
//! On macOS it then reads the ink of a line with stacked marks (rulings RD-131 item 1 and RD-136
//! P1): for each case of `ink_cases` it shows a card of five lines twice, the middle line plain
//! and with `chars::MOST_MARKS` marks, between two deciding lines and the empty lines that part
//! the fields, draws the message's text field into a bitmap and finds the rows that hold ink. A
//! line's box is its plain ink centre plus and less half the pitch, the pitch read from the first
//! and the last line; a field's room is its box and half of the empty line on each side. The
//! marked line's ink must stay inside its room and reach no row of another text line's box; with
//! the marked line last, the pixels of both controls in a drawing of the whole alert must equal
//! the plain card's, and the field must not overlap either control. A failed ink case makes the target end with the status 1 after its end line.
//! A 64-mark line is the control and must reach another line's box; lines of one mark more than
//! `MOST_MARKS` are probes, printed with their reading and never asserted.
//!
//! Last, one bounded try reads the card at the largest text size (ruling RD-90 item 9): it sets
//! the size with `defaults write` on macOS and with `reg add` of TextScaleFactor on Windows, runs
//! this target again with `DIALOG_HOSTED_TEXT_SIZE` set, which shows the narrow bound card once
//! and prints its reading, and sets the size back. It prints each command's status and output
//! and never changes the status.
//!
//! Named limits (ruling RD-92 (3)): the macOS cases queue the alert through Tauri's mock
//! runtime, whose `run_on_main_thread` sends a task to the mock loop on the main thread, and
//! not through the Wry runtime that the app hands to `System::new`, so the product's path to
//! the main thread runs in no test; and on macOS the target would end with the status 0 and no
//! case line if the mock runtime's `run` ever returned, which it does not at tauri 2.12.5.

#[cfg(target_os = "macos")]
#[path = "hosted/activation.rs"]
mod activation;
#[path = "hosted/gate.rs"]
mod gate;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[path = "hosted/ink.rs"]
mod ink;

use halvo_coord::items::chars::columns;
use halvo_coord::items::{ConfirmAnswer, CARD_COLUMNS, CARD_LINES};
use halvo_coord::types::{Class, Field, FieldFrom, Limits};
use halvo_coord_app::Card;
use ink::ink_cases;
#[cfg(target_os = "macos")]
use ink::{ink_reading, Ink};
use std::time::Duration;

/// The variable that makes this target show the narrow bound card once and print its reading.
const TEXT_SIZE: &str = "DIALOG_HOSTED_TEXT_SIZE";

const OFF: &str = "hosted alone: not run on this machine";
const BOUND: Duration = Duration::from_secs(10);
const MARGIN: Duration = Duration::from_millis(200);
/// The smallest supported screen, 1,024 by 768, less a menu bar and a dock or a task bar: the
/// most width and height of a card's window.
const SMALL: (f64, f64) = (1000.0, 700.0);

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

/// A card at the bound of the native card: `CARD_LINES` lines, an empty line between two, each
/// other line its label alone, `Field n: ` and then `fill` until it takes `CARD_COLUMNS` columns,
/// with `W` at its end where a fill of two columns leaves one over.
fn bound_card(lines: usize, width: usize, fill: &str) -> Card {
    let field = |label: String| Field {
        label,
        value: vec![],
        from: FieldFrom::Site,
        from_text: String::new(),
    };
    let fields: Vec<Field> = (0..lines)
        .map(|i| {
            if i % 2 == 1 {
                return field(String::new());
            }
            let mut line = format!("Field {}: ", i / 2 + 1);
            while columns(&line) + columns(fill) <= width {
                line.push_str(fill);
            }
            while columns(&line) < width {
                line.push('W');
            }
            field(line)
        })
        .collect();
    let card = Card::new(&[Class::Payment], &fields).expect("the words of payment");
    assert!(
        card.lines()
            .iter()
            .all(|l| l.is_empty() || columns(l) == width),
        "each line at its width"
    );
    card
}

/// The bound cases: a name and a card at the bound of the native card.
const BOUND_FILLS: [(&str, &str); 3] = [
    ("narrow", "W"),
    ("wide", "\u{4e00}"),
    ("marked", "e\u{301}"),
];

fn bound_cases() -> Vec<(&'static str, Card)> {
    BOUND_FILLS
        .iter()
        .map(|(name, fill)| (*name, bound_card(CARD_LINES, CARD_COLUMNS, fill)))
        .collect()
}

/// A card of five lines, a deciding line, an empty line, `middle`, an empty line and a deciding
/// line, or, for `last`, of three, a deciding line, an empty line and `middle`.
#[cfg(target_os = "macos")]
fn ink_card(middle: &str, last: bool) -> Card {
    let field = |label: &str| Field {
        label: label.into(),
        value: vec![],
        from: FieldFrom::Site,
        from_text: String::new(),
    };
    let mut lines = vec![
        field("Pay to (You gave this): City Power"),
        field(""),
        field(middle),
    ];
    if !last {
        lines.push(field(""));
        lines.push(field("Amount (From the site): 42.10 USD"));
    }
    Card::new(&[Class::Payment], &lines).expect("the words of payment")
}

/// Prints the line that counts the ink cases and returns how many failed.
#[cfg(target_os = "macos")]
fn ink_end(failed: usize, ran: usize, probes_out: usize, probes: usize) -> usize {
    println!(
        "dialog_hosted ink: cases ran {ran}, failed {failed}; probes of one mark more outside their room {probes_out} of {probes}"
    );
    failed
}

/// The one bounded try of RD-90 item 9: sets the largest text size, runs this target again with
/// `TEXT_SIZE` set and prints its bound reading, and sets the size back; it prints each
/// command's status and output and never fails.
#[expect(
    clippy::disallowed_methods,
    reason = "the hosted target sets the system's text size and runs itself again to read the card at it"
)]
fn text_size_try() {
    let (set, back): (&[&str], &[&str]) = if cfg!(target_os = "macos") {
        (
            &["defaults", "write", "-g", "NSFontSize", "-int", "24"],
            &["defaults", "delete", "-g", "NSFontSize"],
        )
    } else {
        (
            &[
                "reg",
                "add",
                "HKCU\\Software\\Microsoft\\Accessibility",
                "/v",
                "TextScaleFactor",
                "/t",
                "REG_DWORD",
                "/d",
                "225",
                "/f",
            ],
            &[
                "reg",
                "delete",
                "HKCU\\Software\\Microsoft\\Accessibility",
                "/v",
                "TextScaleFactor",
                "/f",
            ],
        )
    };
    let say = |what: &str, args: &[&str]| match std::process::Command::new(args[0])
        .args(&args[1..])
        .output()
    {
        Ok(out) => println!(
            "dialog_hosted text size {what}: {} status={} output={:?}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
                + String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => println!(
            "dialog_hosted text size {what}: {} did not start: {e}",
            args.join(" ")
        ),
    };
    say("set", set);
    match std::env::current_exe()
        .and_then(|exe| std::process::Command::new(exe).env(TEXT_SIZE, "1").output())
    {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut read = false;
            for line in text
                .lines()
                .filter(|l| l.starts_with("dialog_hosted bound reading"))
            {
                println!("dialog_hosted text size reading: {line}");
                read = true;
            }
            if !read {
                println!(
                    "dialog_hosted text size reading: none; the run again ended {}",
                    out.status
                );
            }
        }
        Err(e) => println!("dialog_hosted text size reading: the run again did not start: {e}"),
    }
    say("back", back);
}

/// Prints the line that counts the bound cases and returns how many failed.
fn bounds_end(failed: &[String], ran: usize, listed: usize) -> usize {
    println!(
        "dialog_hosted bounds: cases ran {ran} of {listed}, failed {}",
        failed.len()
    );
    failed.len() + listed - ran
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

/// Ends the process: status 1 when a case failed, when a bound case failed (`bounds`, their
/// count), or when the runner's two variables, read here once more and directly, name a hosted
/// runner and fewer cases ran than the list holds.
fn end(verdicts: &[Verdict], listed: usize, bounds: usize) -> ! {
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
    std::process::exit(if failed > 0 || short || bounds > 0 {
        1
    } else {
        0
    });
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
    let once = std::env::var_os(TEXT_SIZE).is_some();
    std::thread::spawn(move || {
        let shows = if once {
            1
        } else {
            cases::LIST + BOUND_FILLS.len() as u32 + 2 * ink_cases().len() as u32 + 4
        };
        std::thread::sleep(BOUND * (shows + 3));
        println!("dialog_hosted watchdog FAIL: the cases did not end in time");
        std::process::exit(1);
    });
    cases::run(card());
}

#[cfg(target_os = "macos")]
mod cases {
    use super::{
        arm, bound_cases, bounds_end, end, ink_card, ink_cases, ink_end, ink_reading, print,
        text_size_try, word, Ink, Verdict, BOUND, BOUND_FILLS, MARGIN, SMALL, TEXT_SIZE,
    };
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::{Card, Dialog, System};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, MainThreadMarker};
    use objc2_foundation::{NSDefaultRunLoopMode, NSPoint, NSRect, NSSize, NSString};
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

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
    static STARTED: Mutex<Option<Instant>> = Mutex::new(None);
    static FRONTED: AtomicUsize = AtomicUsize::new(0);
    static PANEL: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());
    static MEASURED: Mutex<Option<Geometry>> = Mutex::new(None);
    static MESSAGE: Mutex<String> = Mutex::new(String::new());

    /// What a bound case reads of its alert on the main thread, in points of the screen.
    #[derive(Clone)]
    struct Geometry {
        window: NSRect,
        visible: NSRect,
        screen: NSRect,
        buttons: Vec<NSRect>,
        field: Option<(NSRect, NSRect, NSSize)>,
        field_screen: Option<NSRect>,
        ink: Vec<bool>,
        controls: Vec<Vec<u8>>,
    }

    /// The pixels of each control's rectangle in a drawing of the alert's whole content view, so
    /// that two drawings show whether anything drew over a control.
    ///
    /// # Safety
    /// `window` is the modal window, and the caller runs on the main thread.
    unsafe fn control_pixels(window: *mut AnyObject) -> Vec<Vec<u8>> {
        let content: *mut AnyObject = msg_send![window, contentView];
        if content.is_null() {
            return Vec::new();
        }
        let bounds: NSRect = msg_send![content, bounds];
        let rep: *mut AnyObject = msg_send![content, bitmapImageRepForCachingDisplayInRect: bounds];
        if rep.is_null() {
            return Vec::new();
        }
        let _: () = msg_send![content, cacheDisplayInRect: bounds, toBitmapImageRep: rep];
        let wide: isize = msg_send![rep, pixelsWide];
        let high: isize = msg_send![rep, pixelsHigh];
        let row: isize = msg_send![rep, bytesPerRow];
        let per: isize = msg_send![rep, samplesPerPixel];
        let data: *const u8 = msg_send![rep, bitmapData];
        let flipped: bool = msg_send![content, isFlipped];
        if data.is_null() || wide < 1 || high < 1 || bounds.size.width <= 0.0 {
            return Vec::new();
        }
        let scale = wide as f64 / bounds.size.width;
        let (wide, high, row, per) = (wide as usize, high as usize, row as usize, per as usize);
        let bytes = std::slice::from_raw_parts(data, row * high);
        let mut found = Vec::new();
        let mut buttons = Vec::new();
        let mut reading = Reading::default();
        walk(content, 0, &mut reading, &mut found);
        for view in found {
            let frame: NSRect = msg_send![view, bounds];
            let r: NSRect = msg_send![view, convertRect: frame, toView: content];
            let top = if flipped {
                r.origin.y
            } else {
                bounds.size.height - r.origin.y - r.size.height
            };
            let x0 = (r.origin.x * scale).floor().max(0.0) as usize;
            let y0 = (top * scale).floor().max(0.0) as usize;
            let x1 = (((r.origin.x + r.size.width) * scale).ceil() as usize).min(wide);
            let y1 = (((top + r.size.height) * scale).ceil() as usize).min(high);
            let mut pixels = Vec::new();
            for y in y0..y1 {
                pixels.extend_from_slice(&bytes[y * row + x0 * per..y * row + x1 * per]);
            }
            buttons.push(pixels);
        }
        buttons
    }

    fn overlap(a: NSRect, b: NSRect) -> bool {
        a.origin.x < b.origin.x + b.size.width
            && b.origin.x < a.origin.x + a.size.width
            && a.origin.y < b.origin.y + b.size.height
            && b.origin.y < a.origin.y + a.size.height
    }

    /// The rows of a bitmap of 8-bit samples that hold a pixel other than its top left pixel,
    /// which is the field's background.
    ///
    /// # Safety
    /// `rep` is an `NSBitmapImageRep` that lives for the call.
    unsafe fn ink_rows(rep: *mut AnyObject) -> Vec<bool> {
        let wide: isize = msg_send![rep, pixelsWide];
        let high: isize = msg_send![rep, pixelsHigh];
        let row: isize = msg_send![rep, bytesPerRow];
        let per: isize = msg_send![rep, samplesPerPixel];
        let bits: isize = msg_send![rep, bitsPerSample];
        let data: *const u8 = msg_send![rep, bitmapData];
        if data.is_null() || bits != 8 || per < 1 || wide < 1 || high < 1 || row < wide * per {
            return Vec::new();
        }
        let (wide, high, row, per) = (wide as usize, high as usize, row as usize, per as usize);
        let bytes = std::slice::from_raw_parts(data, row * high);
        let background = &bytes[..per];
        (0..high)
            .map(|y| {
                (0..wide).any(|x| {
                    let at = y * row + x * per;
                    bytes[at..at + per]
                        .iter()
                        .zip(background)
                        .any(|(a, b)| a.abs_diff(*b) > 48)
                })
            })
            .collect()
    }

    fn inside(a: NSRect, b: NSRect) -> bool {
        a.origin.x >= b.origin.x - 0.5
            && a.origin.y >= b.origin.y - 0.5
            && a.origin.x + a.size.width <= b.origin.x + b.size.width + 0.5
            && a.origin.y + a.size.height <= b.origin.y + b.size.height + 0.5
    }

    /// The failures of a reading: a window or a control outside the visible area, a window over
    /// `SMALL`, no text field that holds the message, or one whose visible part is smaller than
    /// its bounds (a view cuts its text) or that needs more height than it has.
    fn faults(g: &Geometry) -> Vec<String> {
        let mut faults = Vec::new();
        if !inside(g.window, g.visible) {
            faults.push("the window is not inside the visible area".to_owned());
        }
        if g.window.size.width > SMALL.0 || g.window.size.height > SMALL.1 {
            faults.push("the window is over the smallest supported screen".to_owned());
        }
        let seen = g.buttons.iter().filter(|b| inside(**b, g.visible)).count();
        if g.buttons.len() != 2 || seen != 2 {
            faults.push(format!("{seen} of {} controls in view", g.buttons.len()));
        }
        match g.field {
            None => faults.push("no text field holds the message".to_owned()),
            Some((bounds, visible, need)) => {
                if visible.size.width < bounds.size.width - 0.5
                    || visible.size.height < bounds.size.height - 0.5
                {
                    faults.push("a view cuts the message".to_owned());
                }
                if need.height > bounds.size.height + 0.5 {
                    faults.push("the message needs more height than its field has".to_owned());
                }
            }
        }
        faults
    }

    /// # Safety
    /// `view` is an `NSView` of `window`, and the caller runs on the main thread.
    unsafe fn on_screen(window: *mut AnyObject, view: *mut AnyObject) -> NSRect {
        let bounds: NSRect = msg_send![view, bounds];
        let in_window: NSRect =
            msg_send![view, convertRect: bounds, toView: std::ptr::null_mut::<AnyObject>()];
        msg_send![window, convertRectToScreen: in_window]
    }

    /// # Safety
    /// `view` is null or an `NSView` of `window`, and the caller runs on the main thread.
    unsafe fn gather(
        window: *mut AnyObject,
        view: *mut AnyObject,
        depth: usize,
        message: &str,
        g: &mut Geometry,
    ) {
        if view.is_null() || depth > 12 {
            return;
        }
        let is_button: bool = msg_send![view, isKindOfClass: class!(NSButton)];
        let is_field: bool = msg_send![view, isKindOfClass: class!(NSTextField)];
        if is_button {
            g.buttons.push(on_screen(window, view));
        } else if is_field {
            let value: *mut AnyObject = msg_send![view, stringValue];
            if text(value) == message {
                let bounds: NSRect = msg_send![view, bounds];
                let visible: NSRect = msg_send![view, visibleRect];
                let cell: *mut AnyObject = msg_send![view, cell];
                let tall = NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(bounds.size.width, 1.0e7),
                );
                let need: NSSize = msg_send![cell, cellSizeForBounds: tall];
                g.field = Some((bounds, visible, need));
                g.field_screen = Some(on_screen(window, view));
                let rep: *mut AnyObject =
                    msg_send![view, bitmapImageRepForCachingDisplayInRect: bounds];
                if !rep.is_null() {
                    let _: () = msg_send![view, cacheDisplayInRect: bounds, toBitmapImageRep: rep];
                    g.ink = ink_rows(rep);
                }
            }
        }
        let subviews: *mut AnyObject = msg_send![view, subviews];
        if subviews.is_null() {
            return;
        }
        let count: usize = msg_send![subviews, count];
        for i in 0..count {
            let child: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            gather(window, child, depth + 1, message, g);
        }
    }

    /// Reads the alert of the bound case `generation` once it has reported itself, then ends its
    /// modal session; an alert that has not reported itself is brought to the front once a second.
    fn measure(generation: usize) {
        if CASE.load(Ordering::SeqCst) != generation
            || ACTED.load(Ordering::SeqCst) == generation
            || MainThreadMarker::new().is_none()
        {
            return;
        }
        if REPORT_AT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
        {
            front(generation, "bound");
            return;
        }
        let message = MESSAGE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        // SAFETY: this runs on the main thread inside the alert's modal session, and every
        // object that it reads belongs to the modal window, which lives until the session ends.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let window: *mut AnyObject = msg_send![app, modalWindow];
            if window.is_null() {
                return;
            }
            ACTED.store(generation, Ordering::SeqCst);
            let mut screen: *mut AnyObject = msg_send![window, screen];
            if screen.is_null() {
                screen = msg_send![class!(NSScreen), mainScreen];
            }
            let zero = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
            let (visible, frame) = if screen.is_null() {
                (zero, zero)
            } else {
                let visible: NSRect = msg_send![screen, visibleFrame];
                let frame: NSRect = msg_send![screen, frame];
                (visible, frame)
            };
            let mut g = Geometry {
                window: msg_send![window, frame],
                visible,
                screen: frame,
                buttons: Vec::new(),
                field: None,
                field_screen: None,
                ink: Vec::new(),
                controls: Vec::new(),
            };
            let content: *mut AnyObject = msg_send![window, contentView];
            gather(window, content, 0, &message, &mut g);
            g.controls = control_pixels(window);
            *MEASURED.lock().unwrap_or_else(PoisonError::into_inner) = Some(g);
            let _: () = msg_send![app, abortModal];
        }
    }

    extern "C" fn poke_measure(context: *mut c_void) {
        if let Err(panic) = std::panic::catch_unwind(|| measure(context as usize)) {
            let why = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            println!("dialog_hosted read panicked: {why}");
        }
    }

    /// Shows `card` once and reads it as `measure` does: the answer, the reading and the
    /// milliseconds from the show to the report.
    fn read_once<R: tauri::Runtime>(
        dialog: &System<R>,
        card: &Card,
        generation: usize,
    ) -> (ConfirmAnswer, Option<Geometry>, String) {
        *MESSAGE.lock().unwrap_or_else(PoisonError::into_inner) = card.text();
        *MEASURED.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner) = None;
        REPORTS.store(0, Ordering::SeqCst);
        FRONTED.store(0, Ordering::SeqCst);
        let shown = Instant::now();
        *STARTED.lock().unwrap_or_else(PoisonError::into_inner) = Some(shown);
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
                        dispatch(generation, poke_measure);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        let answer = dialog.show(card, arm(), &|| {
            REPORTS.fetch_add(1, Ordering::SeqCst);
            REPORT_AT
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_with(Instant::now);
        });
        done.store(true, Ordering::SeqCst);
        let _ = poker.join();
        let to_report = REPORT_AT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map_or("no report".to_owned(), |at| {
                at.duration_since(shown).as_millis().to_string()
            });
        let read = MEASURED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        (answer, read, to_report)
    }

    /// Shows `card` once, reads it as `measure` does and prints the reading; the failures of a
    /// case.
    fn bound_case<R: tauri::Runtime>(
        dialog: &System<R>,
        name: &str,
        card: &Card,
        generation: usize,
    ) -> Vec<String> {
        let (answer, read, to_report) = read_once(dialog, card, generation);
        let mut failures = Vec::new();
        if answer != ConfirmAnswer::Reject {
            failures.push(format!("the answer was {}, not reject", word(answer)));
        }
        match read {
            None => failures.push("the alert was never read".to_owned()),
            Some(g) => {
                let (field, need, seen) = g.field.map_or((0.0, 0.0, 0.0), |(b, v, n)| {
                    (b.size.height, n.height, v.size.height)
                });
                println!(
                    "dialog_hosted bound reading {name}: lines={} window={:.0}x{:.0} visible={:.0}x{:.0} screen={:.0}x{:.0} field_h={field:.0} need_h={need:.0} seen_h={seen:.0} controls={} show_to_report_ms={to_report}",
                    card.lines().len(),
                    g.window.size.width,
                    g.window.size.height,
                    g.visible.size.width,
                    g.visible.size.height,
                    g.screen.size.width,
                    g.screen.size.height,
                    g.buttons.len()
                );
                failures.extend(faults(&g));
            }
        }
        failures
    }

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
        let Some(reported) = *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner) else {
            front(generation, name);
            return;
        };
        if name.starts_with("macos press") && reported.elapsed() < arm() + MARGIN {
            return;
        }
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

    /// Brings the alert of a case that has not reported itself to the front, once a second from
    /// the case's first second on, with a line that says what was active before it.
    fn front(generation: usize, name: &str) {
        let Some(started) = *STARTED.lock().unwrap_or_else(PoisonError::into_inner) else {
            return;
        };
        let due = started.elapsed().as_secs() as usize;
        if due == 0
            || FRONTED.swap(due, Ordering::SeqCst) == due
            || CASE.load(Ordering::SeqCst) != generation
        {
            return;
        }
        // SAFETY: this runs on the main thread inside the alert's modal session.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let window: *mut AnyObject = msg_send![app, modalWindow];
            if app.is_null() || window.is_null() {
                return;
            }
            let active: bool = msg_send![app, isActive];
            let key: bool = msg_send![window, isKeyWindow];
            activate_app(app);
            let _: () = msg_send![window, makeKeyAndOrderFront: std::ptr::null_mut::<AnyObject>()];
            println!("dialog_hosted reading {name}: no report after {due} s, brought to the front; before it: app_active={active} key={key}");
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
        *STARTED.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
        FRONTED.store(0, Ordering::SeqCst);
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
        let answer = dialog.show(card, arm(), &|| {
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

    /// # Safety
    /// `app` is the shared application, and the caller runs on the main thread.
    unsafe fn activate_app(app: *mut AnyObject) {
        let _: () = msg_send![app, activateIgnoringOtherApps: true];
        let running: *mut AnyObject = msg_send![class!(NSRunningApplication), currentApplication];
        if !running.is_null() {
            let _: bool = msg_send![running, activateWithOptions: 3usize];
        }
    }

    /// The panel of the target that its start brings to the front once, made once.
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

    /// # Safety
    /// Runs on the main thread.
    unsafe fn app() -> *mut AnyObject {
        msg_send![class!(NSApplication), sharedApplication]
    }

    /// Makes the target's app active once at its start and waits for it before the first case,
    /// as guard_hosted does (ruling RD-107, its Additions 1 and 2; ruling RD-113 item 2): orders
    /// its own panel to the front once, asks, and while the app does not read active on the main
    /// thread, takes the app's own events from its queue and sends them on for a second, then
    /// asks again, for at most 20 s; then orders the panel out, before any dialog. Not active
    /// then, the target runs no case, prints its inconclusive line with the pair as read and ends
    /// as failed.
    fn activate() {
        // SAFETY: this runs on the main thread, before any window of the target exists.
        unsafe {
            let app = app();
            let _: bool = msg_send![app, setActivationPolicy: 0isize];
            let _: () = msg_send![app, finishLaunching];
        }
        let sent = std::cell::Cell::new(0usize);
        let waited = super::activation::start(
            // SAFETY: as above; the panel is the target's own.
            || unsafe {
                let _: () =
                    msg_send![panel(), makeKeyAndOrderFront: std::ptr::null_mut::<AnyObject>()];
            },
            // SAFETY: as above.
            || unsafe { activate_app(app()) },
            // SAFETY: as above.
            || unsafe {
                let active: bool = msg_send![app(), isActive];
                active
            },
            |every| {
                // SAFETY: the shared application's own event queue, read and sent on on the main
                // thread until a date `every` from now or until the app reads active.
                unsafe {
                    let app = app();
                    let until: *mut AnyObject = msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: every.as_secs_f64()];
                    let mode: &NSString = NSDefaultRunLoopMode;
                    loop {
                        let event: *mut AnyObject = msg_send![app, nextEventMatchingMask: usize::MAX, untilDate: until, inMode: mode, dequeue: true];
                        if event.is_null() {
                            break;
                        }
                        let _: () = msg_send![app, sendEvent: event];
                        sent.set(sent.get() + 1);
                        let active: bool = msg_send![app, isActive];
                        if active {
                            break;
                        }
                    }
                }
            },
            // SAFETY: as above.
            || unsafe {
                let _: () = msg_send![panel(), orderOut: std::ptr::null_mut::<AnyObject>()];
            },
            Duration::from_secs(1),
            Duration::from_secs(20),
        );
        println!(
            "dialog_hosted start wait: the wait that takes the app's own events and sends them on ran; events sent on {}",
            sent.get()
        );
        match waited {
            super::activation::Waited::Active { asks } => {
                println!("dialog_hosted start: brought its own panel to the front once, the app read active after {asks} asks, and the panel is ordered out");
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
                    "dialog_hosted inconclusive: the app did not become active within 20 s; active={active} key_window={key} asks={asks}"
                );
                std::process::exit(1);
            }
        }
    }

    /// Runs the cases on a thread of their own while the test runtime's run loop runs on the main
    /// thread, the thread's first show after the sign that the run loop runs, as guard_hosted
    /// does (ruling RD-146 (4)); with no sign within `RUN_LOOP_BOUND`, or with the sign's channel
    /// closed and no sign, it prints its line, runs no case and ends as failed. The setting
    /// `LATE` holds the run call back after the thread has started: the forced form of the race.
    pub fn run(card: Card) {
        activate();
        let late =
            match super::activation::late(std::env::var_os(super::activation::LATE).as_deref()) {
                Ok(late) => late,
                Err(line) => {
                    println!("dialog_hosted {line}");
                    std::process::exit(1);
                }
            };
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let (ready, runs) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let start = Instant::now();
            let begun = super::activation::run_loop(
                |bound| runs.recv_timeout(bound),
                || start.elapsed(),
                super::activation::RUN_LOOP_BOUND,
            );
            let (line, go) = super::activation::begun("dialog_hosted", begun);
            println!("{line}");
            if !go {
                std::process::exit(1);
            }
            let dialog = System::new(handle);
            if std::env::var_os(TEXT_SIZE).is_some() {
                let (name, card) = bound_cases().swap_remove(0);
                let failures = bound_case(&dialog, name, &card, 1);
                println!("dialog_hosted text size case {name}: {failures:?}");
                std::process::exit(0);
            }
            let mut verdicts = Vec::new();
            for generation in 1..=CASES.len() {
                let verdict = case(&dialog, &card, generation);
                print(&verdict);
                verdicts.push(verdict);
            }
            let bounds = bound_cases();
            let mut failed = Vec::new();
            for (k, (name, card)) in bounds.iter().enumerate() {
                let failures = bound_case(&dialog, name, card, CASES.len() + 1 + k);
                if failures.is_empty() {
                    println!("dialog_hosted bound case macos {name} PASS");
                } else {
                    println!(
                        "dialog_hosted bound case macos {name} FAIL: {}",
                        failures.join("; ")
                    );
                    failed.push(name.to_string());
                }
            }
            let bounds_failed = bounds_end(&failed, bounds.len(), BOUND_FILLS.len());
            let mut generation = CASES.len() + bounds.len();
            let (mut ink_failed, mut ran, mut probes, mut probes_out) = (0, 0, 0, 0);
            for (name, plain, marked, ink) in ink_cases() {
                generation += 2;
                let last = ink == Ink::Last;
                let (_, plain_read, _) =
                    read_once(&dialog, &ink_card(&plain, last), generation - 1);
                let (_, marked_read, _) = read_once(&dialog, &ink_card(&marked, last), generation);
                let mut failures = Vec::new();
                if last {
                    match (plain_read, marked_read) {
                        (Some(p), Some(g)) => {
                            let bottom = g.ink.iter().rposition(|i| *i);
                            let height = g.ink.len();
                            let touched = g
                                .field_screen
                                .is_none_or(|f| g.buttons.iter().any(|b| overlap(f, *b)));
                            let differing: usize = if p.controls.len() == 2
                                && g.controls.len() == 2
                                && p.controls
                                    .iter()
                                    .zip(&g.controls)
                                    .all(|(a, b)| a.len() == b.len())
                            {
                                p.controls
                                    .iter()
                                    .zip(&g.controls)
                                    .map(|(a, b)| {
                                        a.iter()
                                            .zip(b)
                                            .filter(|(x, y)| x.abs_diff(**y) > 48)
                                            .count()
                                    })
                                    .sum()
                            } else {
                                usize::MAX
                            };
                            let drawn_over = differing != 0;
                            println!(
                                "dialog_hosted ink reading {name}: rows={height} last_ink={bottom:?} field_overlaps_a_control={touched} controls_read={} control_bytes_differing={differing}",
                                g.controls.len()
                            );
                            if drawn_over {
                                failures.push(
                                    "a control's pixels differ from the plain card's".to_owned(),
                                );
                            }
                            if touched {
                                failures.push("the field overlaps a control".to_owned());
                            }
                        }
                        _ => failures.push("the alert was never read".to_owned()),
                    }
                } else {
                    let rows = |g: Option<Geometry>| g.map(|g| g.ink).unwrap_or_default();
                    match ink_reading(&rows(plain_read), &rows(marked_read)) {
                        Ok(r) => {
                            println!(
                                "dialog_hosted ink reading {name}: pitch={:.1} room={:.1}..{:.1} ink={}..{} margin_above={:.1} margin_below={:.1} in_room={} reaches_another_line={}",
                                r.pitch, r.room.0, r.room.1, r.ink.0, r.ink.1, r.margin().0, r.margin().1, r.in_room, r.reaches
                            );
                            match ink {
                                Ink::Room if !r.in_room => {
                                    failures.push("the ink leaves its room".to_owned())
                                }
                                Ink::Room if r.reaches => {
                                    failures.push("the ink reaches another line".to_owned())
                                }
                                Ink::Control if !r.reaches => failures
                                    .push("the control's ink reaches no other line".to_owned()),
                                Ink::Probe => {
                                    probes += 1;
                                    probes_out += usize::from(!r.in_room || r.reaches);
                                }
                                _ => {}
                            }
                        }
                        Err(why) => failures.push(format!("no reading: {why}")),
                    }
                }
                if ink == Ink::Probe {
                    continue;
                }
                ran += 1;
                if failures.is_empty() {
                    println!("dialog_hosted ink case macos {name} PASS");
                } else {
                    println!(
                        "dialog_hosted ink case macos {name} FAIL: {}",
                        failures.join("; ")
                    );
                    ink_failed += 1;
                }
            }
            let ink_failed = ink_end(ink_failed, ran, probes_out, probes);
            text_size_try();
            end(&verdicts, CASES.len(), bounds_failed + ink_failed);
        });
        if let Some(late) = late {
            println!(
                "dialog_hosted forced form: the main thread holds its run call back {} ms",
                late.as_millis()
            );
            std::thread::sleep(late);
        }
        app.run(move |_, event| {
            if let tauri::RunEvent::Ready = event {
                let _ = ready.send(());
            }
        });
    }
}

#[cfg(windows)]
mod cases {
    use super::{
        arm, bound_cases, bounds_end, end, print, text_size_try, word, Verdict, BOUND, BOUND_FILLS,
        MARGIN, SMALL, TEXT_SIZE,
    };
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::{Card, Dialog, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Instant;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetSystemMetrics, GetWindowRect, IsWindowVisible, PostMessageW, SendMessageW,
        SetForegroundWindow, SystemParametersInfoW, SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA,
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_CLOSE, WM_USER,
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
                let mut brought = false;
                while !done.load(Ordering::SeqCst) {
                    // SAFETY: the title is a string that lives for the call.
                    let found = unsafe { FindWindowW(PCWSTR::null(), &title) };
                    if let Ok(window) = found {
                        // SAFETY: `window` is the window that the call above found; a window that has gone
                        // reads as not visible.
                        let visible = unsafe { IsWindowVisible(window) }.as_bool();
                        let reported = *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner);
                        if visible
                            && reported.is_none()
                            && !brought
                            && start.elapsed() > std::time::Duration::from_secs(3)
                        {
                            brought = true;
                            // SAFETY: as above.
                            let set = unsafe { SetForegroundWindow(window) }.as_bool();
                            println!("dialog_hosted reading {name}: no report in 3 s, brought to the foreground: {set}");
                        }
                        let ready =
                            visible && reported.is_some_and(|at| at.elapsed() >= arm() + MARGIN);
                        if at_act.is_none() && !ready && start.elapsed() <= BOUND {
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
        let answer = dialog.show(card, arm(), &|| {
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

    /// What a bound case reads of its dialog's window: the window and the work area, in pixels
    /// of the screen as the target sees it, and the screen's size.
    struct Geometry {
        window: RECT,
        work: RECT,
        screen: (i32, i32),
    }

    fn faults(g: &Geometry) -> Vec<String> {
        let mut faults = Vec::new();
        let (w, h) = (
            g.window.right - g.window.left,
            g.window.bottom - g.window.top,
        );
        if g.window.left < g.work.left
            || g.window.top < g.work.top
            || g.window.right > g.work.right
            || g.window.bottom > g.work.bottom
        {
            faults.push("the window is not inside the work area".to_owned());
        }
        if f64::from(w) > SMALL.0 || f64::from(h) > SMALL.1 {
            faults.push("the window is over the smallest supported screen".to_owned());
        }
        faults
    }

    /// Shows `card` once, reads its window once it is visible and has reported itself, closes it
    /// and prints the reading; the failures of the case.
    fn bound_case<R: tauri::Runtime>(dialog: &System<R>, name: &str, card: &Card) -> Vec<String> {
        REPORTS.store(0, Ordering::SeqCst);
        *REPORT_AT.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let title = HSTRING::from(card.title());
        let shown = Instant::now();
        let done = Arc::new(AtomicBool::new(false));
        let poker = std::thread::spawn({
            let done = done.clone();
            move || {
                let mut read = None;
                while !done.load(Ordering::SeqCst) {
                    // SAFETY: the title is a string that lives for the call.
                    let found = unsafe { FindWindowW(PCWSTR::null(), &title) };
                    if let Ok(window) = found {
                        // SAFETY: `window` is the window that the call above found; a window that
                        // has gone reads as not visible and its calls fail.
                        let visible = unsafe { IsWindowVisible(window) }.as_bool();
                        let reported = REPORT_AT
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .is_some();
                        if read.is_none() && visible && reported {
                            let mut g = Geometry {
                                window: RECT::default(),
                                work: RECT::default(),
                                screen: (0, 0),
                            };
                            // SAFETY: as above, and each out pointer is a RECT of this frame.
                            let ok = unsafe {
                                GetWindowRect(window, &mut g.window).is_ok()
                                    && SystemParametersInfoW(
                                        SPI_GETWORKAREA,
                                        0,
                                        Some(std::ptr::from_mut(&mut g.work).cast()),
                                        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                                    )
                                    .is_ok()
                            };
                            // SAFETY: two reads of the system's metrics.
                            g.screen = unsafe {
                                (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
                            };
                            read = Some(ok.then_some(g));
                            // SAFETY: as above.
                            let _ = unsafe {
                                PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0))
                            };
                        } else if shown.elapsed() > BOUND {
                            // SAFETY: as above.
                            let _ = unsafe {
                                PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0))
                            };
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                read
            }
        });
        let answer = dialog.show(card, arm(), &|| {
            REPORTS.fetch_add(1, Ordering::SeqCst);
            REPORT_AT
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert_with(Instant::now);
        });
        done.store(true, Ordering::SeqCst);
        let read = poker.join().unwrap_or(None);
        let to_report = REPORT_AT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .map_or("no report".to_owned(), |at| {
                at.duration_since(shown).as_millis().to_string()
            });
        let mut failures = Vec::new();
        if answer != ConfirmAnswer::Reject {
            failures.push(format!("the answer was {}, not reject", word(answer)));
        }
        match read {
            None => failures.push("the dialog was never read".to_owned()),
            Some(None) => failures.push("the window or the work area could not be read".to_owned()),
            Some(Some(g)) => {
                println!(
                    "dialog_hosted bound reading {name}: lines={} window={}x{} work={}x{} screen={}x{} show_to_report_ms={to_report}",
                    card.lines().len(),
                    g.window.right - g.window.left,
                    g.window.bottom - g.window.top,
                    g.work.right - g.work.left,
                    g.work.bottom - g.work.top,
                    g.screen.0,
                    g.screen.1
                );
                failures.extend(faults(&g));
            }
        }
        failures
    }

    pub fn run(card: Card) {
        let app = tauri::test::mock_app();
        let dialog = System::new(app.handle().clone());
        if std::env::var_os(TEXT_SIZE).is_some() {
            let (name, card) = bound_cases().swap_remove(0);
            let failures = bound_case(&dialog, name, &card);
            println!("dialog_hosted text size case {name}: {failures:?}");
            std::process::exit(0);
        }
        let mut verdicts = Vec::new();
        for each in CASES {
            let verdict = case(&dialog, &card, each);
            print(&verdict);
            verdicts.push(verdict);
        }
        let bounds = bound_cases();
        let mut failed = Vec::new();
        for (name, card) in &bounds {
            let failures = bound_case(&dialog, name, card);
            if failures.is_empty() {
                println!("dialog_hosted bound case windows {name} PASS");
            } else {
                println!(
                    "dialog_hosted bound case windows {name} FAIL: {}",
                    failures.join("; ")
                );
                failed.push(name.to_string());
            }
        }
        let bounds = bounds_end(&failed, bounds.len(), BOUND_FILLS.len());
        text_size_try();
        end(&verdicts, CASES.len(), bounds);
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod cases {
    pub const LIST: u32 = 0;

    pub fn run(card: halvo_coord_app::Card) {
        let _ = card;
        super::end(&[], 0, 0);
    }
}
