use std::io::Write;
use std::thread;

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, RunEvent, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::card::{self, Answer, Form};

/// A plugin that, once the app is ready, shows the dialog of `form` `shows` times one after
/// another on a thread of its own, and prints `card::shown_line` before each and
/// `card::answer_line` after each.
pub fn dialogs(form: Form, shows: u32) -> TauriPlugin<Wry> {
    tauri::plugin::Builder::new("r4-driver-dialog")
        .on_event(move |app, event| {
            if let RunEvent::Ready = event {
                let app = app.clone();
                let _ = thread::Builder::new()
                    .name("r4-dialog".into())
                    .spawn(move || {
                        for n in 1..=shows {
                            print_line(&card::shown_line(form, n));
                            let answer = show(&app, form);
                            print_line(&card::answer_line(form, n, answer));
                        }
                    });
            }
        })
        .build()
}

fn print_line(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn show(app: &AppHandle<Wry>, form: Form) -> Answer {
    match form {
        Form::Plugin => plugin(app),
        Form::System => system(app),
    }
}

fn plugin(app: &AppHandle<Wry>) -> Answer {
    let (first, second) = card::plugin_slots();
    let result = app
        .dialog()
        .message(card::body())
        .title(card::TITLE)
        .buttons(MessageDialogButtons::OkCancelCustom(
            first.to_string(),
            second.to_string(),
        ))
        .blocking_show_with_result();
    card::from_plugin(&result)
}

#[cfg(target_os = "macos")]
fn system(app: &AppHandle<Wry>) -> Answer {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSAlert;
    use objc2_foundation::NSString;

    let (tx, rx) = std::sync::mpsc::channel();
    let queued = app.run_on_main_thread(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = tx.send(None);
            return;
        };
        let alert = NSAlert::new(mtm);
        alert.addButtonWithTitle(&NSString::from_str(card::REJECT));
        alert.addButtonWithTitle(&NSString::from_str(card::ALLOW));
        alert.setMessageText(&NSString::from_str(card::TITLE));
        alert.setInformativeText(&NSString::from_str(&card::body()));
        let _ = tx.send(Some(alert.runModal()));
    });
    match (queued, rx.recv()) {
        (Ok(()), Ok(Some(response))) => card::from_alert(response),
        _ => Answer::Reject,
    }
}

#[cfg(windows)]
fn system(_app: &AppHandle<Wry>) -> Answer {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Controls::{
        TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION,
        TDF_SIZE_TO_CONTENT,
    };

    let title = HSTRING::from(card::TITLE);
    let body = HSTRING::from(card::body());
    let reject = HSTRING::from(card::REJECT);
    let allow = HSTRING::from(card::ALLOW);
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: card::REJECT_ID,
            pszButtonText: PCWSTR(reject.as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: card::ALLOW_ID,
            pszButtonText: PCWSTR(allow.as_ptr()),
        },
    ];
    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszContent: PCWSTR(body.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: card::REJECT_ID,
        ..Default::default()
    };
    let mut pressed = 0i32;
    let shown =
        unsafe { TaskDialogIndirect(&config, Some(&mut pressed as *mut i32), None, None) }.is_ok();
    card::from_task_dialog(shown, pressed)
}
