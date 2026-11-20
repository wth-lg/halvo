//! The one place that starts a child of the daemon: the program is found in
//! the directory of the daemon's own executable and never through PATH, the
//! child holds its three standard streams and no other descriptor of the
//! parent, and the child is ended at its deadline.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// The path of `program` in `dir`, with the system's executable suffix.
pub fn beside(dir: &Path, program: &str) -> PathBuf {
    dir.join(format!("{program}{}", std::env::consts::EXE_SUFFIX))
}

/// The directory of the running executable.
pub fn own_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("the executable has no directory"))
}

/// What a child ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The child exited with `status` and printed `out` on its standard output.
    Exited { status: Option<i32>, out: String },
    /// The deadline passed and the child was ended.
    Deadline,
}

/// A started child of the daemon.
#[derive(Debug)]
pub struct Child {
    process: std::process::Child,
    out: Receiver<String>,
    deadline: Instant,
}

/// Starts `program` from `dir` with `args`: standard input and standard error
/// on the null device, standard output to a pipe, and no other descriptor of
/// the parent, whether or not the parent opened it with close-on-exec.
pub fn start(dir: &Path, program: &str, args: &[&str], deadline: Duration) -> io::Result<Child> {
    let mut command = Command::new(beside(dir, program));
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    keep_only_the_standard_streams(&mut command);
    let mut process = command.spawn()?;
    let (send, out) = mpsc::channel();
    if let Some(mut pipe) = process.stdout.take() {
        std::thread::Builder::new()
            .name("halvo-child-output".to_owned())
            .spawn(move || {
                let mut text = String::new();
                let _ = pipe.read_to_string(&mut text);
                let _ = send.send(text);
            })?;
    }
    Ok(Child {
        process,
        out,
        deadline: Instant::now() + deadline,
    })
}

#[cfg(unix)]
fn keep_only_the_standard_streams(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            let highest = match libc::sysconf(libc::_SC_OPEN_MAX) {
                limit if limit > 3 => limit.min(65_536) as libc::c_int,
                _ => 65_536,
            };
            for descriptor in 3..highest {
                let flags = libc::fcntl(descriptor, libc::F_GETFD);
                if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                    libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC);
                }
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn keep_only_the_standard_streams(command: &mut Command) {
    let _ = command;
}

impl Child {
    /// What the child ended with, once it has exited and its standard output
    /// has closed: `None` before that while the deadline has not passed. At
    /// the deadline the child is killed and the answer is
    /// [`Ended::Deadline`]; no call waits.
    pub fn poll(&mut self) -> Option<Ended> {
        let before_the_deadline = Instant::now() < self.deadline;
        match self.process.try_wait() {
            Ok(Some(status)) => match self.out.try_recv() {
                Ok(out) => Some(Ended::Exited {
                    status: status.code(),
                    out,
                }),
                Err(mpsc::TryRecvError::Empty) if before_the_deadline => None,
                Err(_) => Some(Ended::Deadline),
            },
            Ok(None) if before_the_deadline => None,
            Ok(None) | Err(_) => {
                let _ = self.process.kill();
                let _ = self.process.wait();
                Some(Ended::Deadline)
            }
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Ok(None) = self.process.try_wait() {
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A directory under the target directory's tmp, removed when it drops.
    pub(crate) fn test_dir() -> tempfile::TempDir {
        let target = std::env::current_exe()
            .unwrap()
            .ancestors()
            .nth(3)
            .unwrap()
            .join("tmp");
        std::fs::create_dir_all(&target).unwrap();
        tempfile::Builder::new()
            .prefix("children-tests-")
            .tempdir_in(target)
            .unwrap()
    }

    #[cfg(unix)]
    fn wait_for(child: &mut Child) -> Ended {
        loop {
            if let Some(ended) = child.poll() {
                return ended;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_path_is_the_program_in_the_given_directory() {
        let dir = Path::new("daemon-dir");
        assert_eq!(
            beside(dir, "halvo-capture"),
            dir.join(format!("halvo-capture{}", std::env::consts::EXE_SUFFIX))
        );
    }

    #[cfg(unix)]
    fn stand_in(dir: &Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_child_is_resolved_beside_the_daemon_and_not_through_path() {
        let root = test_dir();
        let daemon = root.path().join("daemon");
        let decoy = root.path().join("decoy");
        std::fs::create_dir_all(&daemon).unwrap();
        std::fs::create_dir_all(&decoy).unwrap();
        stand_in(&daemon, "probe-stand-in", "echo beside");
        stand_in(&decoy, "probe-stand-in", "echo decoy");
        let mut child = start(&daemon, "probe-stand-in", &[], Duration::from_secs(10)).unwrap();
        assert_eq!(
            wait_for(&mut child),
            Ended::Exited {
                status: Some(0),
                out: "beside\n".to_owned()
            }
        );
        let missing = start(
            &root.path().join("empty"),
            "probe-stand-in",
            &[],
            Duration::from_secs(1),
        );
        assert!(
            missing.is_err(),
            "a program missing beside the daemon was found elsewhere"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_holds_no_descriptor_that_the_call_did_not_name() {
        const HELD: i32 = 47;
        let root = test_dir();
        stand_in(
            root.path(),
            "fd-stand-in",
            "if (true <&47) 2>/dev/null; then echo open; else echo closed; fi",
        );
        let file = std::fs::File::open(root.path().join("fd-stand-in")).unwrap();
        let opened = std::os::fd::AsRawFd::as_raw_fd(&file);
        assert_eq!(unsafe { libc::dup2(opened, HELD) }, HELD);
        let flags = unsafe { libc::fcntl(HELD, libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::FD_CLOEXEC == 0, "flags {flags}");
        let mut child = start(root.path(), "fd-stand-in", &[], Duration::from_secs(10)).unwrap();
        let ended = wait_for(&mut child);
        let mut plain = std::process::Command::new(root.path().join("fd-stand-in"))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut control = String::new();
        if let Some(mut pipe) = plain.stdout.take() {
            let _ = pipe.read_to_string(&mut control);
        }
        let _ = plain.wait();
        unsafe { libc::close(HELD) };
        assert_eq!(
            control, "open\n",
            "the control child did not inherit descriptor {HELD}"
        );
        assert_eq!(
            ended,
            Ended::Exited {
                status: Some(0),
                out: "closed\n".to_owned()
            },
            "the child holds descriptor {HELD}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_past_its_deadline_is_ended() {
        let root = test_dir();
        stand_in(root.path(), "slow-stand-in", "exec sleep 30");
        let started = Instant::now();
        let mut child = start(
            root.path(),
            "slow-stand-in",
            &[],
            Duration::from_millis(200),
        )
        .unwrap();
        assert_eq!(wait_for(&mut child), Ended::Deadline);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    mod review_cases {
        use super::*;

        // R2-Q9/R2-Q17 fix: children.rs "applies a deadline"; Child::poll doc: "`None` while
        // it runs and its deadline has not passed. At the deadline the child is killed."
        // RR-daemon-04: the tick keeps firing while a probe runs. A child that exits and
        // leaves a grandchild holding its standard output must not make poll block past the
        // deadline (the supervisor's tick calls poll).
        #[cfg(unix)]
        #[test]
        fn a_poll_returns_by_the_deadline_when_a_grandchild_holds_the_output_pipe() {
            let root = test_dir();
            stand_in(root.path(), "gc-stand-in", "sleep 8 &\necho started");
            let started = Instant::now();
            let mut child =
                start(root.path(), "gc-stand-in", &[], Duration::from_millis(500)).unwrap();
            let mut longest = Duration::ZERO;
            let ended = loop {
                let before = Instant::now();
                let polled = child.poll();
                longest = longest.max(before.elapsed());
                if let Some(ended) = polled {
                    break ended;
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            assert!(
                longest < Duration::from_secs(2),
                "one poll blocked for {longest:?} (deadline 500 ms); ended {ended:?}"
            );
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "the child with a 500 ms deadline took {:?} to end",
                started.elapsed()
            );
        }
    }
}
