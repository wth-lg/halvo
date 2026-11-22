//! The session window of a Windows service run: a hidden top-level window, on
//! a thread of its own, that takes the messages of a session's end and
//! WM_CLOSE and ends the run through its one shutdown entry (contracts.md
//! C14.6; unit win-5, RR-windows-15). It is no message-only window, because a
//! message-only window gets no WM_QUERYENDSESSION, and it is never shown.

use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, MSG, WINDOW_EX_STYLE, WM_CLOSE, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW,
    WS_OVERLAPPED,
};

/// The class name of the session window, by which a test finds the window
/// of its own child together with the child's process id.
pub const CLASS_NAME: &str = "HalvoCaptureSession";

const CLASS: PCWSTR = w!("HalvoCaptureSession");

/// How long WM_ENDSESSION holds its answer: the session's budget and one
/// second for the entry to begin and for the watchdog's call, so that the
/// system ends no process whose entry still runs.
const ENDSESSION_HOLD: Duration =
    Duration::from_millis(halvo_control::SESSION_END_BUDGET_MS + 1_000);

/// Starts the session window on the thread `halvo-session` and returns once
/// the window stands, or with the error of its thread, its class or its
/// window.
pub fn start() -> std::io::Result<()> {
    let (made, ready) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("halvo-session".to_owned())
        .spawn(move || match make_the_window() {
            Ok(_) => {
                let _ = made.send(Ok(()));
                pump();
            }
            Err(error) => {
                let _ = made.send(Err(error));
            }
        })?;
    ready
        .recv()
        .map_err(|_| std::io::Error::other("the session thread ended before its window"))?
}

fn make_the_window() -> std::io::Result<HWND> {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    // SAFETY: GetModuleHandleW with no name reads the handle of this
    // process's executable and takes no pointer.
    let instance = unsafe { GetModuleHandleW(None) }?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(session_proc),
        hInstance: instance.into(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    // SAFETY: `class` is a live WNDCLASSW whose name is a static wide string
    // and whose procedure has the WNDPROC signature.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the class name is a static wide string of the class registered
    // above; the window has no parent, no menu and no creation data.
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            CLASS,
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }?;
    Ok(window)
}

fn pump() {
    let mut msg = MSG::default();
    loop {
        // SAFETY: `msg` is a live MSG that GetMessageW fills; the filter takes
        // every window of this thread.
        let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if got.0 <= 0 {
            return;
        }
        // SAFETY: `msg` is the message that GetMessageW filled.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn session_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_QUERYENDSESSION => {
            crate::daemon::signal::end_for_the_session();
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                crate::daemon::signal::end_for_the_session();
                std::thread::sleep(ENDSESSION_HOLD);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            crate::daemon::signal::end_on_request();
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}
