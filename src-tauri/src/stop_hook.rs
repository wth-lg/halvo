//! The app's one message at a stop: a start that fails, or a panic of the main thread, which
//! unwinds out of `main` and ends the app. A panic of another thread ends that thread alone and
//! shows nothing, as before. The message holds the two fixed sentences of [`STOPPED`] and nothing
//! of the fault, and it is shown at most once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread::ThreadId;

/// The title of the message.
pub const TITLE: &str = "Halvo";

/// The text of the message: two fixed sentences and nothing of the fault.
pub const STOPPED: &str = "Halvo stopped because of an error. Open Halvo again.";

/// Where the message goes: [`system`] in the app, a stand-in in a test.
pub type Show = fn(title: &str, text: &str);

/// Where the hook shows the message, and the thread that installed it: the main thread in the app.
struct Hook {
    show: Show,
    main: ThreadId,
}

static HOOK: OnceLock<Hook> = OnceLock::new();
static SHOWN: AtomicBool = AtomicBool::new(false);

/// Installs the hook for the thread that calls it, first in `main`: a later panic of that thread
/// runs the panic hook that stood before, which writes its line on standard error, and then
/// [`stopped`]. A second call changes nothing.
pub fn install(show: Show) {
    let hook = Hook {
        show,
        main: std::thread::current().id(),
    };
    if HOOK.set(hook).is_err() {
        return;
    }
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        before(info);
        if HOOK
            .get()
            .is_some_and(|hook| hook.main == std::thread::current().id())
        {
            stopped();
        }
    }));
}

/// A stop: shows the message once and waits until it is answered. It runs on a thread of its
/// own, so that no window of the stopping thread is served while the message waits. Before
/// [`install`] it shows nothing.
pub fn stopped() {
    let Some(hook) = HOOK.get() else {
        return;
    };
    if SHOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    let show = hook.show;
    match std::thread::Builder::new().spawn(move || show(TITLE, STOPPED)) {
        Ok(shown) => {
            let _ = shown.join();
        }
        Err(_) => show(TITLE, STOPPED),
    }
}

/// The system's message on Windows, with the system's error icon and its one OK button. It is
/// shown only where the app has no standard error, as for a start of the windows-subsystem build
/// from the Start menu, a shortcut or a login item, where a line on standard error reaches
/// nobody; where standard error is given, the line there says it and nothing is shown, so that no
/// start by a console or a script waits for a press.
#[cfg(windows)]
pub fn system(title: &str, text: &str) {
    use std::os::windows::io::AsRawHandle;
    if !std::io::stderr().as_raw_handle().is_null() {
        return;
    }
    windows_core::link!("user32.dll" "system" fn MessageBoxW(owner: *mut core::ffi::c_void, text: *const u16, caption: *const u16, style: u32) -> i32);
    const MB_ICONERROR: u32 = 0x0000_0010;
    const MB_SETFOREGROUND: u32 = 0x0001_0000;
    let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    // SAFETY: both pointers name NUL-terminated UTF-16 buffers that live until the call returns,
    // and a null owner window is allowed.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

/// Other systems show no message: the line on standard error stays.
#[cfg(not(windows))]
pub fn system(_title: &str, _text: &str) {}

#[cfg(test)]
mod tests {
    use super::install;
    use std::process::{Command, Stdio};

    const CHILD: &str = "HALVO_STOP_HOOK_CHILD";
    const TEST: &str =
        "stop_hook::tests::a_stop_shows_the_two_fixed_sentences_once_and_no_word_of_the_fault";
    const FAULT: &str = "a window title that Halvo recorded";

    /// The stand-in of the system's message: it adds the title and the text to the file that the
    /// child's variable names.
    fn record(title: &str, text: &str) {
        let Some(path) = std::env::var_os(CHILD) else {
            return;
        };
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("the record opens");
        std::io::Write::write_all(&mut file, format!("{title}\n{text}\n").as_bytes())
            .expect("the record is written");
    }

    /// The child: the hook with the stand-in, then a panic of another thread, a stop that is
    /// caught and a second stop, each with the fault in its words.
    fn in_the_child() {
        install(record);
        let other = std::thread::spawn(|| panic!("another thread: {FAULT}"));
        assert!(other.join().is_err(), "the other thread panics");
        assert!(std::panic::catch_unwind(|| panic!("the first stop: {FAULT}")).is_err());
        panic!("the second stop: {FAULT}");
    }

    #[test]
    fn a_stop_shows_the_two_fixed_sentences_once_and_no_word_of_the_fault() {
        if std::env::var_os(CHILD).is_some() {
            in_the_child();
        }
        let shown = crate::tests::scratch_dir("stop-hook").join("shown");
        let out = Command::new(std::env::current_exe().expect("the test binary has a path"))
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, &shown)
            .stdin(Stdio::null())
            .output()
            .expect("the test binary starts again");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "the child ends in its panic: {err}");
        assert!(
            err.contains(&format!("the second stop: {FAULT}")),
            "the fault reached the hook: {err}"
        );
        let record = std::fs::read_to_string(&shown).unwrap_or_default();
        assert_eq!(
            record,
            "Halvo\nHalvo stopped because of an error. Open Halvo again.\n"
        );
        assert!(!record.contains(FAULT), "{record}");
    }
}
