#[cfg(any(test, target_os = "macos"))]
use super::outcome::from_alert;
#[cfg(any(test, windows))]
use super::outcome::from_task_dialog;
use super::{Card, Dialog};
use halvo_coord::items::ConfirmAnswer;
use std::time::Duration;
use tauri::{AppHandle, Runtime};

/// The native dialog in the form `system` of U0.4's verdict (I11.5, ruling RD-42 D2), never the
/// plugin's message dialog. Every outcome but the press on the allowing control answers `reject`, a
/// failed call among them, which the seam tests `wait_alert` and `task_answer` hold on both
/// systems, and neither form sets a time limit. Each show holds every control for `arm` (ruling
/// RD-91): it reports itself on screen at the first moment at which its window became the active
/// window and never before it, and a press of any control inside `arm` after the return of that
/// report, after each later moment at which the window became the active window again, or after a
/// press that the hold refused, closes nothing and answers nothing; the hold is decided at the press
/// from the monotonic clock, and no control waits for an event to become enabled. The test target
/// `guard_hosted`, on a hosted runner, holds that a press of each control 100 ms after the report
/// leaves the dialog open, that a press after the hold answers, that a refused press starts the
/// hold anew, that a second show holds again, and that the window was the active window at the
/// report. On macOS it is the alert of AppKit, shown on the main thread, whose buttons send their
/// action to the hold, which ends the modal session with the button's response when the button's
/// window is the modal window and the hold lets the press through. The window became the active
/// window when a reading on the main thread finds it the key window of the active app (ruling
/// RD-103): at the notices that it became the key window and that the app became active, which
/// read in the level form; at a reading repeated about every 50 ms in the main run loop's common
/// modes until the show's first moment; and at each press, before the press is decided. The
/// notices that it resigned key and that the app resigned active store not true without a
/// reading, so that the next reading of both true is a moment. The report comes at the first
/// moment, a press that takes a moment is refused, and a press while the window is not the active
/// window or not the modal window answers nothing and starts an existing hold anew; the decision
/// is the pure function of `Moment`. The test target `dialog_hosted`, on a hosted runner, holds its two buttons, the rejecting
/// text with the response 1000 and the allowing text with 1001; that no button is titled Cancel or
/// holds the Escape key; that its message text is the card's title and its informative text the
/// card's text; and that a press on each button after the hold, a stopped and an aborted modal
/// session answer as the mapping says, each after one report on screen. Which button the laid-out
/// alert holds as its default, and that its window has no close control, the target asserts where
/// it reads them; where it gives no reading, they rest on the trial's record and on the hand check
/// of I9.5. On Windows it is the task dialog, whose callback refuses a button's click inside the
/// hold, and whose window, subclassed at its creation, reports the show at its first activation and
/// refuses the message WM_CLOSE inside the hold; when that subclass cannot be set, the show
/// closes at once through its rejecting control and its allowing control answers nothing. The same target holds that a click of the allowing
/// id 100 allows, and that a click of the rejecting id 101, of the cancel response 2, which the
/// Escape key and the close control give, of an id that no button has, and the message WM_CLOSE
/// each reject, each after the hold and after one report; its default control and the flag that
/// allows the cancel response are held by the config test. On another system no dialog is shown and
/// the answer is `reject`.
pub struct System<R: Runtime> {
    #[cfg(target_os = "macos")]
    app: AppHandle<R>,
    #[cfg(not(target_os = "macos"))]
    runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> System<R> {
    /// The dialog of the app `app`.
    pub fn new(app: AppHandle<R>) -> System<R> {
        #[cfg(target_os = "macos")]
        return System { app };
        #[cfg(not(target_os = "macos"))]
        {
            drop(app);
            System {
                runtime: std::marker::PhantomData,
            }
        }
    }
}

impl<R: Runtime> Dialog for System<R> {
    fn show(&self, card: &Card, arm: Duration, on_screen: &dyn Fn()) -> ConfirmAnswer {
        #[cfg(target_os = "macos")]
        return macos::show(&self.app, card, arm, on_screen);
        #[cfg(windows)]
        return windows::show(card, arm, on_screen);
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = (card, arm, on_screen);
            super::decide(super::Outcome::<()>::Failed, ())
        }
    }
}

/// The two controls in the order in which the dialog adds them: the rejecting one first, so
/// that it is the default control.
#[cfg(any(test, target_os = "macos"))]
pub fn controls(card: &Card) -> [&str; 2] {
    [card.reject(), card.allow()]
}

/// A message of the main thread's alert to the thread that waits for it: the alert's window
/// became the key window for the first time, or the alert ended with this response.
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy)]
pub enum Step {
    Shown,
    Response(isize),
}

/// The answer of an alert whose show on the main thread was queued with `queued` and which sends
/// its two steps on `rx`: `on_screen` once the alert's window became the key window, then
/// `reported` once that report has returned, so that the hold starts after it, then `allow` for
/// the response 1001 alone. A failed queue and a channel that closes before the report answer
/// `reject` with no report; a channel that closes after the report and a response with no report
/// before it answer `reject`.
#[cfg(any(test, target_os = "macos"))]
pub fn wait_alert<E>(
    queued: Result<(), E>,
    rx: &std::sync::mpsc::Receiver<Step>,
    on_screen: &dyn Fn(),
    reported: &dyn Fn(),
) -> ConfirmAnswer {
    if queued.is_err() {
        return from_alert(None);
    }
    match rx.recv() {
        Ok(Step::Shown) => {
            on_screen();
            reported();
        }
        _ => return from_alert(None),
    }
    match rx.recv() {
        Ok(Step::Response(response)) => from_alert(Some(response)),
        _ => from_alert(None),
    }
}

/// The answer of a task dialog from the result of its call and the id of the pressed button:
/// `allow` for the allowing id from a call that returned, and `reject` for every other pair.
#[cfg(any(test, windows))]
pub fn task_answer<E>(called: Result<(), E>, pressed: i32) -> ConfirmAnswer {
    from_task_dialog(called.ok().map(|_| pressed))
}

#[cfg(target_os = "macos")]
mod macos {
    use super::super::guard::Guard;
    use super::super::moment::{Decision, Event, Moment, Pair};
    use super::super::Card;
    use super::{controls, wait_alert, Step};
    use halvo_coord::items::ConfirmAnswer;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAlert, NSApplication, NSApplicationDidBecomeActiveNotification,
        NSApplicationDidResignActiveNotification, NSButton, NSWindowDidBecomeKeyNotification,
        NSWindowDidResignKeyNotification,
    };
    use objc2_foundation::{
        NSNotification, NSNotificationCenter, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer,
    };
    use std::cell::RefCell;
    use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Runtime};

    const READING: f64 = 0.05;

    struct State {
        guard: Guard,
        moment: Moment,
    }

    fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
        state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    struct Held {
        state: Arc<Mutex<State>>,
        shown: mpsc::Sender<Step>,
        button: Retained<NSButton>,
        timer: RefCell<Option<Retained<NSTimer>>>,
    }

    define_class!(
        // SAFETY: NSObject has no requirement of a subclass, and `Hold` implements no `Drop`.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "HalvoDialogHold"]
        #[ivars = Held]
        struct Hold;

        impl Hold {
            #[unsafe(method(press:))]
            fn press(&self, sender: Option<&NSButton>) {
                if let Some(button) = sender {
                    press(self, button);
                }
            }

            #[unsafe(method(becameKey:))]
            fn became_key(&self, note: &NSNotification) {
                if ours(self, note) {
                    decide(self, Event::Become);
                }
            }

            #[unsafe(method(becameActive:))]
            fn became_active(&self, _: &NSNotification) {
                decide(self, Event::Become);
            }

            #[unsafe(method(resignedKey:))]
            fn resigned_key(&self, note: &NSNotification) {
                if ours(self, note) {
                    decide(self, Event::Resign);
                }
            }

            #[unsafe(method(resignedActive:))]
            fn resigned_active(&self, _: &NSNotification) {
                decide(self, Event::Resign);
            }

            #[unsafe(method(reading:))]
            fn reading(&self, _: Option<&NSTimer>) {
                decide(self, Event::Reading);
            }
        }
    );

    fn ours(hold: &Hold, note: &NSNotification) -> bool {
        let Some(window) = hold.ivars().button.window() else {
            return false;
        };
        note.object().is_some_and(|object| {
            std::ptr::eq(Retained::as_ptr(&object), Retained::as_ptr(&window).cast())
        })
    }

    fn pair(hold: &Hold) -> Pair {
        let app = NSApplication::sharedApplication(hold.mtm());
        Pair {
            active: app.isActive(),
            key: hold
                .ivars()
                .button
                .window()
                .is_some_and(|window| window.isKeyWindow()),
        }
    }

    fn decide(hold: &Hold, event: Event) {
        let decision = {
            let mut state = lock(&hold.ivars().state);
            let now = Instant::now();
            let read = (event != Event::Resign).then(|| pair(hold));
            let State { guard, moment } = &mut *state;
            moment.decide(guard, event, read, now)
        };
        after(hold, decision);
    }

    fn press(hold: &Hold, button: &NSButton) {
        let app = NSApplication::sharedApplication(hold.mtm());
        let decision = {
            let mut state = lock(&hold.ivars().state);
            let now = Instant::now();
            let read = pair(hold);
            let modal = app
                .modalWindow()
                .zip(button.window())
                .is_some_and(|(modal, own)| {
                    std::ptr::eq(Retained::as_ptr(&modal), Retained::as_ptr(&own))
                });
            let State { guard, moment } = &mut *state;
            moment.decide(guard, Event::Press { modal }, Some(read), now)
        };
        after(hold, decision);
        if decision == Decision::Answer {
            app.stopModalWithCode(button.tag());
        }
    }

    fn after(hold: &Hold, decision: Decision) {
        if decision.reports() {
            stop_reading(hold);
            let _ = hold.ivars().shown.send(Step::Shown);
        }
    }

    fn stop_reading(hold: &Hold) {
        if let Some(timer) = hold.ivars().timer.borrow_mut().take() {
            timer.invalidate();
        }
    }

    pub fn show<R: Runtime>(
        app: &AppHandle<R>,
        card: &Card,
        arm: Duration,
        on_screen: &dyn Fn(),
    ) -> ConfirmAnswer {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(State {
            guard: Guard::new(arm),
            moment: Moment::default(),
        }));
        let held = state.clone();
        let title = card.title().to_owned();
        let text = card.text();
        let titles = controls(card).map(str::to_owned);
        let queued = app.run_on_main_thread(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let alert = NSAlert::new(mtm);
            let buttons = titles.map(|title| alert.addButtonWithTitle(&NSString::from_str(&title)));
            alert.setMessageText(&NSString::from_str(&title));
            alert.setInformativeText(&NSString::from_str(&text));
            alert.layout();
            let hold: Retained<Hold> = {
                let this = Hold::alloc(mtm).set_ivars(Held {
                    state: held,
                    shown: tx.clone(),
                    button: buttons[0].clone(),
                    timer: RefCell::new(None),
                });
                // SAFETY: `init` of NSObject on a newly allocated object.
                unsafe { msg_send![super(this), init] }
            };
            let target: &AnyObject = &hold;
            for button in &buttons {
                // SAFETY: `hold` answers the selector `press:` with a sender that is a button, and it
                // lives until after the modal session, the only time in which a button sends it.
                unsafe {
                    button.setTarget(Some(target));
                    button.setAction(Some(sel!(press:)));
                }
            }
            let center = NSNotificationCenter::defaultCenter();
            // SAFETY: `hold` answers each of the four selectors with a notification, and it is
            // removed as an observer before it is dropped.
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
            // SAFETY: `hold` answers the selector `reading:` with the timer, which retains it
            // until it is invalidated at the show's first moment or after the modal session.
            let timer = unsafe {
                NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                    READING,
                    target,
                    sel!(reading:),
                    None,
                    true,
                )
            };
            // SAFETY: the main run loop and its common modes, in which the modal session runs.
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
            *hold.ivars().timer.borrow_mut() = Some(timer);
            let response = alert.runModal();
            stop_reading(&hold);
            // SAFETY: `hold` is the observer that was added above.
            unsafe { center.removeObserver(target) };
            let _ = tx.send(Step::Response(response));
        });
        wait_alert(queued, &rx, on_screen, &|| {
            lock(&state).guard.reported(Instant::now())
        })
    }
}

#[cfg(windows)]
pub(super) mod windows {
    use super::super::guard::Show;
    use super::super::outcome::{TASK_ALLOW, TASK_REJECT};
    use super::super::Card;
    use super::task_answer;
    use halvo_coord::items::ConfirmAnswer;
    use std::time::{Duration, Instant};
    use windows::core::{HRESULT, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, S_FALSE, S_OK, WPARAM};
    use windows::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TASKDIALOG_FLAGS,
        TASKDIALOG_NOTIFICATIONS, TDF_ALLOW_DIALOG_CANCELLATION, TDF_SIZE_TO_CONTENT,
        TDM_CLICK_BUTTON, TDN_BUTTON_CLICKED, TDN_CREATED, TDN_DESTROYED,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        PostMessageW, WA_INACTIVE, WM_ACTIVATE, WM_CLOSE,
    };

    const SUBCLASS: usize = 1;

    pub struct Texts {
        title: HSTRING,
        text: HSTRING,
        reject: HSTRING,
        allow: HSTRING,
    }

    pub const FLAGS: TASKDIALOG_FLAGS =
        TASKDIALOG_FLAGS(TDF_ALLOW_DIALOG_CANCELLATION.0 | TDF_SIZE_TO_CONTENT.0);

    impl Texts {
        pub fn new(card: &Card) -> Texts {
            Texts {
                title: HSTRING::from(card.title()),
                text: HSTRING::from(card.text()),
                reject: HSTRING::from(card.reject()),
                allow: HSTRING::from(card.allow()),
            }
        }

        pub fn buttons(&self) -> [TASKDIALOG_BUTTON; 2] {
            [
                TASKDIALOG_BUTTON {
                    nButtonID: TASK_REJECT,
                    pszButtonText: PCWSTR(self.reject.as_ptr()),
                },
                TASKDIALOG_BUTTON {
                    nButtonID: TASK_ALLOW,
                    pszButtonText: PCWSTR(self.allow.as_ptr()),
                },
            ]
        }

        pub fn config(&self, buttons: &[TASKDIALOG_BUTTON; 2], show: &Show) -> TASKDIALOGCONFIG {
            TASKDIALOGCONFIG {
                cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
                dwFlags: FLAGS,
                pszWindowTitle: PCWSTR(self.title.as_ptr()),
                pszContent: PCWSTR(self.text.as_ptr()),
                cButtons: buttons.len() as u32,
                pButtons: buttons.as_ptr(),
                nDefaultButton: TASK_REJECT,
                pfCallback: Some(callback),
                lpCallbackData: show as *const Show as isize,
                ..Default::default()
            }
        }
    }

    unsafe extern "system" fn callback(
        window: HWND,
        msg: TASKDIALOG_NOTIFICATIONS,
        wparam: WPARAM,
        _: LPARAM,
        data: isize,
    ) -> HRESULT {
        // SAFETY: `data` is the address of the `Show` that `show` holds for the whole call of
        // `TaskDialogIndirect`, the only call that hands it here.
        let show = unsafe { &*(data as *const Show) };
        match msg {
            TDN_CREATED => {
                // SAFETY: the subclass reads `data` as the same `Show`, and it is removed at
                // TDN_DESTROYED, before the call returns.
                let set =
                    unsafe { SetWindowSubclass(window, Some(subclass), SUBCLASS, data as usize) };
                if !set.as_bool() {
                    show.lose();
                    // SAFETY: a message to the dialog's own window, which lives for this call.
                    let _ = unsafe {
                        PostMessageW(
                            Some(window),
                            TDM_CLICK_BUTTON.0 as u32,
                            WPARAM(TASK_REJECT as usize),
                            LPARAM(0),
                        )
                    };
                // SAFETY: a read of the active window of this thread.
                } else if unsafe { GetActiveWindow() } == window {
                    show.active(&Instant::now);
                }
                S_OK
            }
            TDN_BUTTON_CLICKED if !show.click(wparam.0 == TASK_ALLOW as usize, Instant::now()) => {
                S_FALSE
            }
            TDN_DESTROYED => {
                // SAFETY: the subclass that TDN_CREATED set on this window.
                let _ = unsafe { RemoveWindowSubclass(window, Some(subclass), SUBCLASS) };
                S_OK
            }
            _ => S_OK,
        }
    }

    unsafe extern "system" fn subclass(
        window: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _: usize,
        data: usize,
    ) -> LRESULT {
        // SAFETY: `data` is the address of the `Show` that the callback handed over, which lives
        // until the call of `TaskDialogIndirect` returns, after TDN_DESTROYED removed this subclass.
        let show = unsafe { &*(data as *const Show) };
        match msg {
            WM_ACTIVATE if (wparam.0 & 0xffff) as u32 != WA_INACTIVE => show.active(&Instant::now),
            WM_CLOSE if !show.press(Instant::now()) => return LRESULT(0),
            _ => {}
        }
        // SAFETY: the message as this subclass got it, for the window that it got it for.
        unsafe { DefSubclassProc(window, msg, wparam, lparam) }
    }

    pub fn show(card: &Card, arm: Duration, on_screen: &dyn Fn()) -> ConfirmAnswer {
        let texts = Texts::new(card);
        let buttons = texts.buttons();
        let show = Show::new(arm, on_screen);
        let config = texts.config(&buttons, &show);
        let mut pressed = 0i32;
        // SAFETY: `config` points into `texts`, `buttons` and `show`, each of which outlives the
        // call.
        let called = unsafe { TaskDialogIndirect(&config, Some(&mut pressed), None, None) };
        task_answer(called, pressed)
    }
}
