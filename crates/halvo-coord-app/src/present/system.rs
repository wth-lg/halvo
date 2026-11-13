use super::{Card, Dialog};
use halvo_coord::items::ConfirmAnswer;
use tauri::{AppHandle, Runtime};

/// The native dialog in the form `system` of U0.4's verdict (I11.5, ruling RD-42 D2), never
/// the plugin's message dialog: on macOS the alert of AppKit, shown on the main thread with the
/// rejecting control first, which is its default control, and with no cancel and no close
/// control; on Windows the task dialog, with the rejecting control as its default, whose
/// Escape key and close control give the cancel response. Neither sets a time limit. Every
/// outcome but the press on the allowing control answers `reject`, a failed call among them;
/// on another system no dialog is shown and the answer is `reject`.
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
    fn show(&self, card: &Card, on_screen: &dyn Fn()) -> ConfirmAnswer {
        #[cfg(target_os = "macos")]
        return macos::show(&self.app, card, on_screen);
        #[cfg(windows)]
        return windows::show(card, on_screen);
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = (card, on_screen);
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

#[cfg(target_os = "macos")]
mod macos {
    use super::super::outcome::from_alert;
    use super::super::Card;
    use super::controls;
    use halvo_coord::items::ConfirmAnswer;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSAlert;
    use objc2_foundation::NSString;
    use std::sync::mpsc;
    use tauri::{AppHandle, Runtime};

    enum Step {
        Shown,
        Response(isize),
    }

    pub fn show<R: Runtime>(
        app: &AppHandle<R>,
        card: &Card,
        on_screen: &dyn Fn(),
    ) -> ConfirmAnswer {
        let (tx, rx) = mpsc::channel();
        let title = card.title().to_owned();
        let text = card.text();
        let titles = controls(card).map(str::to_owned);
        let queued = app.run_on_main_thread(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let alert = NSAlert::new(mtm);
            for title in &titles {
                alert.addButtonWithTitle(&NSString::from_str(title));
            }
            alert.setMessageText(&NSString::from_str(&title));
            alert.setInformativeText(&NSString::from_str(&text));
            let _ = tx.send(Step::Shown);
            let _ = tx.send(Step::Response(alert.runModal()));
        });
        if queued.is_err() {
            return from_alert(None);
        }
        match rx.recv() {
            Ok(Step::Shown) => on_screen(),
            _ => return from_alert(None),
        }
        match rx.recv() {
            Ok(Step::Response(response)) => from_alert(Some(response)),
            _ => from_alert(None),
        }
    }
}

#[cfg(windows)]
pub(super) mod windows {
    use super::super::outcome::{from_task_dialog, TASK_ALLOW, TASK_REJECT};
    use super::super::Card;
    use halvo_coord::items::ConfirmAnswer;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use windows::core::{HRESULT, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, S_OK, WPARAM};
    use windows::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TASKDIALOG_FLAGS,
        TASKDIALOG_NOTIFICATIONS, TDF_ALLOW_DIALOG_CANCELLATION, TDF_SIZE_TO_CONTENT, TDN_CREATED,
    };

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

        pub fn config(
            &self,
            buttons: &[TASKDIALOG_BUTTON; 2],
            on_screen: &&dyn Fn(),
        ) -> TASKDIALOGCONFIG {
            TASKDIALOGCONFIG {
                cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
                dwFlags: FLAGS,
                pszWindowTitle: PCWSTR(self.title.as_ptr()),
                pszContent: PCWSTR(self.text.as_ptr()),
                cButtons: buttons.len() as u32,
                pButtons: buttons.as_ptr(),
                nDefaultButton: TASK_REJECT,
                pfCallback: Some(created),
                lpCallbackData: on_screen as *const &dyn Fn() as isize,
                ..Default::default()
            }
        }
    }

    unsafe extern "system" fn created(
        _: HWND,
        msg: TASKDIALOG_NOTIFICATIONS,
        _: WPARAM,
        _: LPARAM,
        data: isize,
    ) -> HRESULT {
        if msg == TDN_CREATED {
            // SAFETY: `data` is the address of the `&dyn Fn()` that `show` holds for the
            // whole call of `TaskDialogIndirect`, the only call that hands it here.
            let on_screen = unsafe { &*(data as *const &dyn Fn()) };
            let _ = catch_unwind(AssertUnwindSafe(on_screen));
        }
        S_OK
    }

    pub fn show(card: &Card, on_screen: &dyn Fn()) -> ConfirmAnswer {
        let texts = Texts::new(card);
        let buttons = texts.buttons();
        let config = texts.config(&buttons, &on_screen);
        let mut pressed = 0i32;
        // SAFETY: `config` points into `texts`, `buttons` and `on_screen`, each of which
        // outlives the call.
        let shown = unsafe { TaskDialogIndirect(&config, Some(&mut pressed), None, None) };
        from_task_dialog(shown.ok().map(|()| pressed))
    }
}
