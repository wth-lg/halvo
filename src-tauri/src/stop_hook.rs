//! The app's one message at a stop: a start that fails, an end with no window and no tray, or a
//! panic of the main thread, which unwinds out of `main` and ends the app. A panic of another
//! thread ends that thread alone and shows nothing, as before. The message holds the two fixed
//! sentences of [`STOPPED`] and nothing of the fault, and it is shown at most once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread::ThreadId;
use std::time::Duration;

/// The title of the message.
pub const TITLE: &str = "Halvo";

/// The text of the message: two fixed sentences and nothing of the fault.
pub const STOPPED: &str = "Halvo stopped because of an error. Open Halvo again.";

/// How long a stop waits for its message: [`stopped`] returns after it on every system, answered
/// or not, and the macOS alert leaves by itself after it.
pub const WAIT: Duration = Duration::from_secs(60);

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

/// A stop: shows the message once and waits until it is answered, for at most [`WAIT`]; then it
/// returns, answered or not, and its caller ends the process. Before [`install`] it shows
/// nothing.
pub fn stopped() {
    let Some(hook) = HOOK.get() else {
        return;
    };
    if SHOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    show_within(hook.show, WAIT);
}

/// Runs `show` on a thread of its own, so that no window of the stopping thread is served while
/// the message waits, and waits until it returns, for at most `bound`. Where no thread can be
/// made, `show` runs on this thread with no bound.
fn show_within(show: Show, bound: Duration) {
    let (shown, answered) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().spawn(move || {
        show(TITLE, STOPPED);
        let _ = shown.send(());
    });
    match spawned {
        Ok(_) => {
            let _ = answered.recv_timeout(bound);
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

/// The system's alert on macOS, with the title, the text and the system's one OK button; it waits
/// until the person answers it, and it leaves by itself after [`WAIT`]. It is shown only where the
/// app's standard error is the null device, as for a start from the Finder or the Dock, where a
/// line on standard error reaches nobody, and never at a start with `--background`, which is the
/// login item's, where launchd starts the app again after a failed end; where standard error is a
/// terminal, a pipe or a file, the line there says it and nothing is shown, so that no start by a
/// console or a script waits for a press.
#[cfg(target_os = "macos")]
pub fn system(title: &str, text: &str) {
    use std::ffi::c_void;
    use std::os::fd::AsFd;
    if a_background_start(std::env::args_os().skip(1))
        || !the_null_device(std::io::stderr().as_fd())
    {
        return;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithBytes(
            allocator: *const c_void,
            bytes: *const u8,
            length: isize,
            encoding: u32,
            external: u8,
        ) -> *const c_void;
        fn CFUserNotificationDisplayAlert(
            timeout: f64,
            flags: usize,
            icon: *const c_void,
            sound: *const c_void,
            localization: *const c_void,
            header: *const c_void,
            message: *const c_void,
            default_button: *const c_void,
            alternate_button: *const c_void,
            other_button: *const c_void,
            response: *mut usize,
        ) -> i32;
        fn CFRelease(object: *const c_void);
    }
    const UTF8: u32 = 0x0800_0100;
    const STOP_ALERT_LEVEL: usize = 0;
    let string = |text: &str| -> *const c_void {
        let Ok(length) = isize::try_from(text.len()) else {
            return std::ptr::null();
        };
        // SAFETY: the bytes are a live UTF-8 slice of `length` bytes, and a null allocator is the
        // default one; the call copies the bytes and returns a string that this function owns, or
        // null.
        unsafe { CFStringCreateWithBytes(std::ptr::null(), text.as_ptr(), length, UTF8, 0) }
    };
    let header = string(title);
    let message = string(text);
    if !header.is_null() && !message.is_null() {
        let mut response = 0;
        let none = std::ptr::null();
        // SAFETY: the header and the message are strings that this function made and holds until
        // the call returns; every other reference is null, which the call allows, and the response
        // is a local that the call writes.
        unsafe {
            CFUserNotificationDisplayAlert(
                WAIT.as_secs_f64(),
                STOP_ALERT_LEVEL,
                none,
                none,
                none,
                header,
                message,
                none,
                none,
                none,
                &mut response,
            );
        }
    }
    for made in [header, message] {
        if !made.is_null() {
            // SAFETY: each string was made above by this function, which owns it and releases it
            // once, here.
            unsafe { CFRelease(made) };
        }
    }
}

/// Whether the arguments after the program's name are a start of the app with `--background`, as
/// the app's own parser reads them.
#[cfg(target_os = "macos")]
fn a_background_start(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    matches!(
        crate::launch::parse(arguments),
        Ok(crate::launch::Launch::Run {
            background: true,
            ..
        })
    )
}

/// Whether `descriptor` is the null device: a character device with the device number of
/// /dev/null. A terminal, a pipe, a file and a descriptor that cannot be read are not.
#[cfg(target_os = "macos")]
fn the_null_device(descriptor: std::os::fd::BorrowedFd<'_>) -> bool {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let given = descriptor
        .try_clone_to_owned()
        .and_then(|owned| std::fs::File::from(owned).metadata());
    match (given, std::fs::metadata("/dev/null")) {
        (Ok(given), Ok(null)) => {
            given.file_type().is_char_device()
                && null.file_type().is_char_device()
                && given.rdev() == null.rdev()
        }
        _ => false,
    }
}

/// Other systems show no message: the line on standard error stays.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn system(_title: &str, _text: &str) {}

#[cfg(test)]
mod tests {
    use super::install;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};

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

    static HELD: Mutex<()> = Mutex::new(());

    /// The stand-in of a message that nobody answers: it returns only once the test lets it go.
    fn unanswered(_title: &str, _text: &str) {
        drop(HELD.lock().unwrap_or_else(PoisonError::into_inner));
    }

    #[test]
    fn a_stop_returns_at_its_bound_when_the_message_is_not_answered() {
        let held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        let bound = Duration::from_millis(200);
        let started = Instant::now();
        super::show_within(unanswered, bound);
        let waited = started.elapsed();
        drop(held);
        assert!(waited >= bound && waited < super::WAIT, "{waited:?}");
    }

    /// The criterion of the macOS alert over descriptors that the test opens itself: the null
    /// device shows it; another character device, both ends of a pipe and a file do not.
    #[cfg(target_os = "macos")]
    #[test]
    fn on_a_mac_only_the_null_device_as_standard_error_shows_the_alert() {
        use std::os::fd::AsFd;
        let open = |path: &str| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap_or_else(|e| panic!("{path} opens: {e}"))
        };
        assert!(super::the_null_device(open("/dev/null").as_fd()));
        assert!(!super::the_null_device(open("/dev/zero").as_fd()));
        let (reader, writer) = std::io::pipe().expect("a pipe is made");
        assert!(!super::the_null_device(reader.as_fd()));
        assert!(!super::the_null_device(writer.as_fd()));
        let file = crate::tests::scratch_dir("stop-hook-criterion").join("standard-error");
        let file = std::fs::File::create(file).expect("the file is made");
        assert!(!super::the_null_device(file.as_fd()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_a_mac_a_start_with_background_shows_no_alert() {
        let background = |arguments: &[&str]| {
            super::a_background_start(arguments.iter().map(std::ffi::OsString::from))
        };
        assert!(background(&["--background"]));
        assert!(background(&["--data-dir", "/halvo", "--background"]));
        assert!(!background(&[]));
        assert!(!background(&["--data-dir", "/halvo"]));
        assert!(!background(&["--background", "--background"]));
    }

    /// Every Rust file under `dir`, with its path from `src` written with slashes and its text
    /// with LF line ends.
    fn sources(src: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).expect("a source directory lists") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                sources(src, &path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let name = path
                    .strip_prefix(src)
                    .expect("under src")
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                let text = std::fs::read_to_string(&path).expect("a source file reads");
                out.push((name, text.replace("\r\n", "\n")));
            }
        }
    }

    /// The app's end with no window and no tray shows the message before its exit with 3: the
    /// call stands in the arm `Opening::End` of app.rs, before `app.exit`. Beside it `stopped` is
    /// called where the build of the app fails in main.rs, and in no other file but this one.
    #[test]
    fn an_end_with_no_window_and_no_tray_shows_the_message_before_its_exit_and_stopped_has_no_third_caller(
    ) {
        const CALL: &str = "stop_hook::stopped";
        const EXIT: &str = "app.exit(Exit::GuardNotInstalled.code());";
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&src, &src, &mut files);
        files.sort();
        let app = &files
            .iter()
            .find(|(name, _)| name == "app.rs")
            .expect("app.rs reads")
            .1;
        assert_eq!(app.matches(EXIT).count(), 1, "one end with exit 3");
        let before = &app[..app.find(EXIT).expect("the exit")];
        let arm = before.rfind("Opening::").expect("an arm before the exit");
        assert!(
            before[arm..].starts_with("Opening::End("),
            "{}",
            &before[arm..]
        );
        let call = before.rfind(CALL).expect("the call before the exit");
        assert!(arm < call, "the call stands in the arm, before the exit");
        let others = files.iter().filter(|(name, _)| name != "stop_hook.rs");
        let callers: Vec<(&str, usize)> = others
            .clone()
            .map(|(name, text)| (name.as_str(), text.matches(CALL).count()))
            .filter(|(_, count)| *count > 0)
            .collect();
        assert_eq!(callers, [("app.rs", 1), ("main.rs", 1)]);
        for (name, text) in others {
            for import in ["stop_hook::{", "stop_hook::*"] {
                assert!(!text.contains(import), "{name} imports from stop_hook");
            }
        }
    }
}
