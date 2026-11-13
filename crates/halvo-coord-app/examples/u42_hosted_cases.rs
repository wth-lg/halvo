//! Hosted only: cases of the reading of unit U4.2 that a hosted runner alone can show.
//!
//! The program builds the unit's own system dialog through `System::show`, reads the built
//! dialog's controls back through the system's own objects, closes the dialog and prints one
//! line for each case. It posts no input event of the operating system. It does nothing unless
//! `HALVO_U42_HOSTED_CASES=1` and `GITHUB_ACTIONS=true` are set, and it always ends with the
//! status 0, within 50 seconds, with every dialog that it opened.

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::Class;
use halvo_coord_app::Card;
use std::time::Duration;

fn word(answer: ConfirmAnswer) -> &'static str {
    if answer == ConfirmAnswer::Allow {
        "allow"
    } else {
        "reject"
    }
}

fn main() {
    let hosted = std::env::var("HALVO_U42_HOSTED_CASES").as_deref() == Ok("1")
        && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true");
    if !hosted {
        println!("case u42-hosted skipped: not a hosted runner");
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(50));
        println!("case u42-hosted watchdog: 50 s passed, the process ends with its dialogs");
        std::process::exit(0);
    });
    let Ok(card) = Card::new(&[Class::Payment], &[]) else {
        println!("case u42-hosted card: the words file gave no card for payment");
        return;
    };
    println!(
        "case u42-hosted card: title={:?} reject={:?} allow={:?} word_reject={} word_allow={}",
        card.title(),
        card.reject(),
        card.allow(),
        word(ConfirmAnswer::Reject),
        word(ConfirmAnswer::Allow)
    );
    cases::run(card);
}

#[cfg(target_os = "macos")]
mod cases {
    use super::word;
    use halvo_coord_app::{Card, Dialog, System};
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, MainThreadMarker};
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    const ACTIONS: [&str; 4] = ["abort", "press_allow", "press_reject", "stop"];

    static CASE: AtomicUsize = AtomicUsize::new(0);
    static ACTED: AtomicUsize = AtomicUsize::new(0);
    static FELL_BACK: AtomicUsize = AtomicUsize::new(0);
    static TEXTS: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

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

    type Button = (*mut AnyObject, String, isize, String);

    /// The text of an `NSString`, or of nothing.
    ///
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

    /// Collects the buttons under `view`, in the order of the view tree.
    ///
    /// # Safety
    /// `view` is null or an `NSView`, and the caller runs on the main thread.
    unsafe fn walk(view: *mut AnyObject, depth: usize, out: &mut Vec<Button>) {
        if view.is_null() || depth > 12 {
            return;
        }
        let is_button: bool = msg_send![view, isKindOfClass: class!(NSButton)];
        if is_button {
            let title: *mut AnyObject = msg_send![view, title];
            let tag: isize = msg_send![view, tag];
            let key: *mut AnyObject = msg_send![view, keyEquivalent];
            out.push((view, text(title), tag, text(key)));
        }
        let subviews: *mut AnyObject = msg_send![view, subviews];
        if subviews.is_null() {
            return;
        }
        let count: usize = msg_send![subviews, count];
        for i in 0..count {
            let child: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
            walk(child, depth + 1, out);
        }
    }

    fn act(generation: usize) {
        if CASE.load(Ordering::SeqCst) != generation || ACTED.load(Ordering::SeqCst) == generation {
            return;
        }
        if MainThreadMarker::new().is_none() {
            return;
        }
        let action = ACTIONS[(generation - 1) % ACTIONS.len()];
        let (reject, allow) = TEXTS.lock().unwrap_or_else(PoisonError::into_inner).clone();
        // SAFETY: this runs on the main thread, inside the modal session of the alert, and every
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
            let visible: bool = msg_send![window, isVisible];
            let content: *mut AnyObject = msg_send![window, contentView];
            let mut buttons = Vec::new();
            walk(content, 0, &mut buttons);
            let read: Vec<String> = buttons
                .iter()
                .map(|(_, title, tag, key)| {
                    format!("[title={title:?} tag={tag} key={:?}]", key.as_str())
                })
                .collect();
            println!(
                "case u42-hosted-mac {action} read: window_visible={visible} buttons={} {}",
                buttons.len(),
                read.join(" ")
            );
            let press = |wanted: &str| {
                if let Some((button, _, tag, _)) =
                    buttons.iter().find(|(_, title, _, _)| title == wanted)
                {
                    println!("case u42-hosted-mac {action} act: performClick on the button titled {wanted:?} with tag {tag}");
                    let _: () = msg_send![*button, performClick: std::ptr::null_mut::<AnyObject>()];
                } else {
                    println!("case u42-hosted-mac {action} act: no button titled {wanted:?}");
                }
            };
            match action {
                "press_allow" => press(&allow),
                "press_reject" => press(&reject),
                "stop" => {
                    println!("case u42-hosted-mac {action} act: stopModal");
                    let _: () = msg_send![app, stopModal];
                }
                _ => {
                    println!("case u42-hosted-mac {action} act: abortModal");
                    let _: () = msg_send![app, abortModal];
                }
            }
        }
    }

    fn fall_back(generation: usize) {
        if CASE.load(Ordering::SeqCst) != generation
            || FELL_BACK.swap(generation, Ordering::SeqCst) == generation
        {
            return;
        }
        if MainThreadMarker::new().is_none() {
            return;
        }
        println!("case u42-hosted-mac fallback: abortModal 5 s after the case began");
        // SAFETY: this runs on the main thread, and `abortModal` is made for a call from outside
        // the handling of an event.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            if !app.is_null() {
                let _: () = msg_send![app, abortModal];
            }
        }
    }

    extern "C" fn poke(context: *mut c_void) {
        let _ = std::panic::catch_unwind(|| act(context as usize));
    }

    extern "C" fn poke_fall_back(context: *mut c_void) {
        let _ = std::panic::catch_unwind(|| fall_back(context as usize));
    }

    fn case<R: tauri::Runtime>(dialog: &System<R>, card: &Card, generation: usize) {
        let action = ACTIONS[(generation - 1) % ACTIONS.len()];
        *TEXTS.lock().unwrap_or_else(PoisonError::into_inner) =
            (card.reject().to_owned(), card.allow().to_owned());
        CASE.store(generation, Ordering::SeqCst);
        let done = Arc::new(AtomicBool::new(false));
        let poker = std::thread::spawn({
            let done = done.clone();
            move || {
                let start = Instant::now();
                while !done.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(9) {
                    let work: extern "C" fn(*mut c_void) =
                        if start.elapsed() > Duration::from_secs(5) {
                            poke_fall_back
                        } else {
                            poke
                        };
                    // SAFETY: the main queue lives as long as the process, the context is a
                    // number and no pointer, and `work` takes that number alone.
                    unsafe {
                        dispatch_async_f(
                            std::ptr::addr_of!(MAIN_QUEUE),
                            generation as *mut c_void,
                            work,
                        );
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        });
        let on_screen = AtomicBool::new(false);
        let answer = dialog.show(card, &|| on_screen.store(true, Ordering::SeqCst));
        done.store(true, Ordering::SeqCst);
        let _ = poker.join();
        println!(
            "case u42-hosted-mac {action} result: unit_answer={} on_screen={} acted={} fallback_abort={}",
            word(answer),
            on_screen.load(Ordering::SeqCst),
            ACTED.load(Ordering::SeqCst) == generation,
            FELL_BACK.load(Ordering::SeqCst) == generation
        );
    }

    pub fn run(card: Card) {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let dialog = System::new(handle);
            for generation in 1..=ACTIONS.len() {
                case(&dialog, &card, generation);
            }
            println!("case u42-hosted-mac end: every case ran");
            std::process::exit(0);
        });
        app.run(|_, _| {});
    }
}

#[cfg(windows)]
mod cases {
    use super::word;
    use halvo_coord_app::{Card, Dialog, System};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, PostMessageW, SendMessageW, DM_GETDEFID, WM_CLOSE, WM_USER,
    };

    const TDM_CLICK_BUTTON: u32 = WM_USER + 102;

    fn case<R: tauri::Runtime>(dialog: &System<R>, card: &Card, action: &str, id: usize) {
        let title = HSTRING::from(card.title());
        let done = Arc::new(AtomicBool::new(false));
        let poker = std::thread::spawn({
            let done = done.clone();
            move || {
                let start = Instant::now();
                let mut acted = false;
                let mut fell_back = false;
                let mut default = String::from("unread");
                while !done.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(9) {
                    // SAFETY: the title is a string that lives for the call.
                    let found = unsafe { FindWindowW(PCWSTR::null(), &title) };
                    if let Ok(window) = found {
                        if !acted {
                            acted = true;
                            // SAFETY: `window` is the window that the call above found; a
                            // message to a window that has gone fails and does nothing.
                            let answer = unsafe { SendMessageW(window, DM_GETDEFID, None, None) };
                            default = format!("{:#x}", answer.0);
                            if id == 0 {
                                // SAFETY: as above.
                                let _ = unsafe {
                                    PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0))
                                };
                            } else {
                                // SAFETY: as above.
                                let _ = unsafe {
                                    SendMessageW(window, TDM_CLICK_BUTTON, Some(WPARAM(id)), None)
                                };
                            }
                        } else if !fell_back && start.elapsed() > Duration::from_secs(5) {
                            fell_back = true;
                            // SAFETY: as above.
                            let _ = unsafe {
                                PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0))
                            };
                        }
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                (acted, fell_back, default)
            }
        });
        let on_screen = AtomicBool::new(false);
        let answer = dialog.show(card, &|| on_screen.store(true, Ordering::SeqCst));
        done.store(true, Ordering::SeqCst);
        let (acted, fell_back, default) =
            poker
                .join()
                .unwrap_or((false, false, String::from("unread")));
        println!(
            "case u42-hosted-win {action} result: unit_answer={} on_screen={} acted={acted} fallback_close={fell_back} dm_getdefid={default}",
            word(answer),
            on_screen.load(Ordering::SeqCst)
        );
    }

    pub fn run(card: Card) {
        let app = tauri::test::mock_app();
        let dialog = System::new(app.handle().clone());
        let actions = [
            ("click_id_100", 100),
            ("click_id_101", 101),
            ("click_id_2", 2),
            ("close", 0),
            ("click_id_1", 1),
        ];
        for (action, id) in actions {
            case(&dialog, &card, action, id);
        }
        println!("case u42-hosted-win end: every case ran");
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod cases {
    pub fn run(card: halvo_coord_app::Card) {
        println!(
            "case u42-hosted other: no system dialog on this system, title={:?} answer={}",
            card.title(),
            super::word(halvo_coord::items::ConfirmAnswer::Reject)
        );
    }
}
