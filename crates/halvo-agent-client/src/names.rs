//! The read of a workbook's sheet names (r4 U3.22c): the library starts the runner's program in its mode
//! `--sheet-names`, with the workbook's path as one argument, reads what it prints under a bound of bytes, and ends
//! it with its process group at a bound of seconds. The caller's process reads no byte of the workbook.

use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::api::RunnerExit;
use crate::proc::{self, SpawnFailed, POLL};

/// How long the library waits for the mode to end before it ends the program with its group: a minute, so that a
/// person can read and answer the question that macOS asks before a program of Halvo reads Documents, the Desktop
/// or Downloads, which the open of the workbook waits for.
pub const SHEET_NAMES_WAIT: Duration = Duration::from_secs(60);

/// The most bytes of the mode's output that the library reads: 64 names of 255 bytes, each with its line feed, the
/// bounds of the sheet crate's read of the names.
pub const SHEET_NAMES_BYTES: usize = 16_384;

/// Why the library answers no names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotRead {
    /// No program was made, the system gave no value of the runner's environment, or the program's line or output
    /// could not be read, and the library ended it.
    NotStarted,
    /// The program did not end within the wait: the library ended it with its group.
    Wait,
    /// The program printed more than the bound of bytes: the library ended it with its group.
    Bytes,
    /// The program ended with another status than 0, or by a signal: a refusal of the mode among them.
    Refused,
    /// The program ended with the status 0 and its output does not read as names: not UTF-8, or a last line with no
    /// line feed.
    Unreadable,
}

/// The names of the worksheets of the workbook at `workbook`, in the workbook's order, as the runner's program at
/// `runner` prints them in its mode `--sheet-names` (crates/halvo-runner/src/names.rs): started with that flag and
/// the path as one argument, no shell, the runner's environment, its directory and its own process group (on
/// Windows its job), as a run's runner is started (`proc::spawn`), with its standard input closed at once. It reads
/// at most `SHEET_NAMES_BYTES` of what the program prints and waits at most `SHEET_NAMES_WAIT` for its end; beyond
/// either bound it ends the program with its group and answers not read, as it does for a refusal and for an output
/// that does not read. Each line of the output, ended by a line feed, or by a carriage return and a line feed, is
/// one name; an empty output is no name.
pub fn sheet_names(runner: &Path, workbook: &Path) -> Result<Vec<String>, NotRead> {
    let mut cmd = Command::new(runner);
    cmd.arg("--sheet-names").arg(workbook);
    read_names(cmd, SHEET_NAMES_WAIT, SHEET_NAMES_BYTES)
}

/// `sheet_names` of the program and the arguments of `cmd`, with the bounds `wait` and `bytes`.
pub(crate) fn read_names(
    cmd: Command,
    wait: Duration,
    bytes: usize,
) -> Result<Vec<String>, NotRead> {
    let mut cmd = cmd;
    let deadline = Instant::now() + wait;
    let env = proc::environment(proc::system_value).map_err(|_| NotRead::NotStarted)?;
    let mut run = match proc::spawn(&mut cmd, &env) {
        Ok(run) => run,
        Err(SpawnFailed::Made(mut run)) => {
            run.kill();
            return Err(NotRead::NotStarted);
        }
        Err(SpawnFailed::NotMade) => return Err(NotRead::NotStarted),
    };
    let Some((input, output)) = run.take_link() else {
        run.kill();
        return Err(NotRead::NotStarted);
    };
    drop(input);
    let (sent, got) = mpsc::channel();
    let room = u64::try_from(bytes).unwrap_or(u64::MAX).saturating_add(1);
    thread::spawn(move || {
        let mut out = Vec::new();
        let read = output.take(room).read_to_end(&mut out).map(|_| out);
        let _ = sent.send(read);
    });
    let mut printed = None;
    loop {
        if printed.is_none() {
            match got.try_recv() {
                Ok(Ok(out)) => printed = Some(out),
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    run.kill();
                    return Err(NotRead::NotStarted);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if printed.as_ref().is_some_and(|out| out.len() > bytes) {
            run.kill();
            return Err(NotRead::Bytes);
        }
        if printed.is_some() && run.exited() {
            break;
        }
        if Instant::now() >= deadline {
            run.kill();
            return Err(NotRead::Wait);
        }
        thread::sleep(POLL);
    }
    if run.exit() != Some(RunnerExit::Status(0)) {
        return Err(NotRead::Refused);
    }
    let text = String::from_utf8(printed.unwrap_or_default()).map_err(|_| NotRead::Unreadable)?;
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let lines = text.strip_suffix('\n').ok_or(NotRead::Unreadable)?;
    Ok(lines
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    fn prints_two_names() -> Command {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "printf 'Budget\\nSummary\\n'"]);
        c
    }

    #[cfg(target_os = "macos")]
    fn never_ends() -> Command {
        let mut c = Command::new("/bin/sleep");
        c.arg("60");
        c
    }

    #[cfg(target_os = "macos")]
    fn prints_past_the_bound() -> Command {
        let mut c = Command::new("/usr/bin/yes");
        c.arg("Sheet");
        c
    }

    #[cfg(windows)]
    fn shell(args: &[&str]) -> Command {
        use std::os::windows::ffi::OsStringExt;
        let mut path = std::path::PathBuf::from(std::ffi::OsString::from_wide(
            &crate::sys::system_directory().expect("the system directory"),
        ));
        path.push("cmd.exe");
        let mut c = Command::new(path);
        c.args(args);
        c
    }

    #[cfg(windows)]
    fn prints_two_names() -> Command {
        shell(&["/d", "/c", "echo", "Budget&", "echo", "Summary"])
    }

    #[cfg(windows)]
    fn never_ends() -> Command {
        shell(&["/d", "/c", "ping", "-n", "61", "127.0.0.1"])
    }

    #[cfg(windows)]
    fn prints_past_the_bound() -> Command {
        shell(&[
            "/d", "/c", "for", "/l", "%i", "in", "(0,0,1)", "do", "@echo", "Sheet",
        ])
    }

    /// r4 U3.22c: over stand-in programs made as the library's tests make one, a program that prints two names
    /// answers them; one that never ends is ended at the bound of seconds and answers not read; one that prints
    /// more than the bound of bytes answers not read.
    #[test]
    fn two_printed_names_are_read_and_a_program_that_never_ends_or_prints_past_the_bound_is_not_read(
    ) {
        let roomy = Duration::from_secs(30);
        assert_eq!(
            read_names(prints_two_names(), roomy, SHEET_NAMES_BYTES),
            Ok(vec!["Budget".to_string(), "Summary".to_string()])
        );
        let short = Duration::from_secs(1);
        let started = Instant::now();
        assert_eq!(
            read_names(never_ends(), short, SHEET_NAMES_BYTES),
            Err(NotRead::Wait)
        );
        let took = started.elapsed();
        assert!(
            took >= short && took < short + Duration::from_secs(10),
            "the program that never ends was ended after {took:?}"
        );
        assert_eq!(
            read_names(prints_past_the_bound(), roomy, SHEET_NAMES_BYTES),
            Err(NotRead::Bytes)
        );
    }
}
