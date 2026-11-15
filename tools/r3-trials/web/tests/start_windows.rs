//! CP-190 (5): two constraints of the Windows start that only a comment stated (start.rs, the
//! `StartLock` and the standard handles of a start), each held by a test here.
#![cfg(windows)]

use r3_web::start::{self, Spec};
use std::os::windows::io::AsRawHandle;
use std::path::Path;

/// A start on the pipe of `cmd /C exit 0` with its standard error in `stderr`, ended and waited for
/// on every way out of the test. The start names its program to `CreateProcessW` as the
/// application, which no search of the path finds, so the program is the full path that
/// `ComSpec` gives.
fn start_exit(stderr: Option<&Path>) -> start::OnPipe {
    let cmd = std::path::PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec"));
    let spec = Spec {
        program: &cmd,
        args: vec!["/C".into(), "exit".into(), "0".into()],
        env: Vec::new(),
        stderr,
    };
    let held = start::lock();
    start::start_on_pipe(&held, &spec).expect("start")
}

/// While no start runs, the ends of a browser's pipe that this process keeps are not
/// inheritable: a start makes them inheritable inside the start lock alone.
#[test]
fn the_ends_that_this_process_keeps_are_not_inheritable_after_a_start() {
    let mut on = start_exit(None);
    let _held = start::lock();
    let mut handles = vec![
        on.from_browser.as_raw_handle(),
        on.to_browser.as_raw_handle(),
    ];
    handles.extend(on.kept.iter().map(|f| f.as_raw_handle()));
    for h in handles {
        let mut flags = 0u32;
        // SAFETY: `h` is an open handle of a file that `on` owns, and `flags` is a live u32.
        let ok = unsafe { windows_sys::Win32::Foundation::GetHandleInformation(h, &mut flags) };
        assert_ne!(ok, 0, "the handle could not be read");
        assert_eq!(
            flags & windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT,
            0,
            "an end of the pipe is inheritable outside a start"
        );
    }
    on.child.kill();
    on.child.wait();
}

/// A start opens the file of the child's standard error for the child alone and closes this
/// process's handle after the start: once the child has ended, the file can be removed, which a
/// handle left open without the share mode for a removal would refuse.
#[test]
fn a_start_closes_its_own_handle_of_the_child_s_error_file() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("r3-start-closes-the-error-file.txt");
    if path.exists() {
        std::fs::remove_file(&path).expect("an earlier run's file");
    }
    let mut on = start_exit(Some(&path));
    on.child.wait();
    drop(on);
    assert!(path.exists(), "the start made no error file");
    std::fs::remove_file(&path).expect("this process still holds the error file");
}
