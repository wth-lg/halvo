//! Halvo's memory cap: one child process under a limit in bytes, its peak, and the
//! model lock that lets one generative model load at a time for one user
//! (contracts.md C23).
//!
//! [`run`] starts one child under a limit and returns its exit, its peak and whether
//! the cap stopped it. On Windows the child runs in a new Job object with a per-process
//! memory limit of the limit and a working-set maximum of the limit, or without the
//! working-set maximum where the caller's token lacks its privilege, and the peak is the
//! Job's peak process memory. On macOS the parent reads the child's lifetime peak physical
//! footprint at the start, every 250 ms and once more when the child has ended, and kills
//! the child at the first read above the limit; the cap and the peak cover the started
//! process alone, so a caller wraps the program itself and never a shell.
//!
//! The cap bounds and measures a program that Halvo builds or chooses. It is no sandbox: a
//! program that works against it passes it. `capped=no` says that no read was above the
//! limit, and on Windows that no report of a refused allocation arrived; it never says that
//! the program held no more than the limit. The reading is one quantity on each system, and
//! the two lists below name, as a closed list, what that quantity does not count. A way that
//! a gate tried and that the reading counts is not in the lists.
//! docs/r2/trials/cap.md holds a `limit` line for each name on each machine of its system,
//! which a run of the crate's probe, the example `hold`, printed.
//!
//! On macOS, the lifetime peak physical footprint of the started process does not count:
//! - `child-process`: the memory of any other process, also of a process that the started
//!   one starts and of one that outlives it;
//! - `exec`: what the process held before it became another program by an exec, because
//!   the lifetime peak starts again;
//! - `mapped-read` and `mapped-written`: pages of a mapped file, read, or written through a
//!   shared mapping;
//! - `purgeable`: volatile purgeable memory;
//! - `reusable`: pages that the process marked reusable, also while they still hold their
//!   data;
//! - `kernel-message`: data that the kernel holds for the process in messages queued on a
//!   port of its own;
//! - `socket-buffer`: data in the buffers of sockets and pipes that the process filled.
//!
//! On Windows, the Job's peak committed memory of one process does not count:
//! - `child-process`: the other processes of the Job, each of which has the limit to
//!   itself;
//! - `mapped-file`: pages of a mapped file;
//! - `section`: a section backed by the paging file, whose commit the system charges to no
//!   process; the working-set maximum holds its resident pages at the limit in `mode=job`, and
//!   nothing holds them in `mode=job-commit`;
//! - `broker-process`: a process that a service of the system starts for the child (a Task
//!   Scheduler task is shown; WMI is not shown), which runs outside the Job.
//!
//! On Windows the cap does not depend on a privilege. Where setting the Job's limits with the
//! working-set maximum fails with error 1314, the library sets them without that flag and
//! goes on: the per-process memory limit, which is what refuses an allocation above the
//! limit, needs no privilege. Every other failure stays an error (`cap-unsupported`). The
//! first line of `measure-peak` and the outcome say which of the two a run had: `mode=job`
//! with the working-set maximum, `mode=job-commit` without it. The working-set maximum only
//! trims resident pages that are no process commit, which the list above names as not
//! counted (`section`, `mapped-file`), so a program that keeps the loader rules is bounded as
//! before; the `section` line of a run in `mode=job-commit` shows that its resident pages
//! are then not held at the limit. A standard user gets `mode=job-commit`.
//!
//! On macOS the cap ends a child at most one poll after the first read above the limit; it
//! does not keep it under the limit, and `peak_bytes` then shows how far the child went.
//!
//! [`start`] starts one child under a limit and returns at once: the child of an
//! [`EndsSpec`], which inherits the ends that the spec names and no other descriptor or
//! handle of its starter. [`Started::wait`] blocks as [`run`] does and returns the same
//! outcome, and an [`Ender`] ends the child from another thread.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

mod lock;

pub use lock::{model_lock_path, take_model_lock, LockError, ModelLock};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

#[cfg(not(any(target_os = "macos", windows)))]
mod other;
#[cfg(not(any(target_os = "macos", windows)))]
use other as platform;

/// A memory limit in bytes, above zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Limit(u64);

impl Limit {
    /// The limit of `bytes` bytes; `None` for zero.
    pub fn new(bytes: u64) -> Option<Limit> {
        (bytes > 0).then_some(Limit(bytes))
    }

    /// The limit in bytes.
    pub fn bytes(self) -> u64 {
        self.0
    }
}

/// The mechanism that holds the cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A Job object with the per-process memory limit and the working-set maximum (Windows).
    Job,
    /// A Job object with the per-process memory limit alone, where the caller's token lacks
    /// the privilege that the working-set maximum needs (Windows).
    JobCommit,
    /// The parent polls the peak physical footprint and kills on a breach (macOS).
    Poll,
    /// As `Poll`, with the child also started under `taskpolicy -m` (macOS).
    Taskpolicy,
}

impl Mode {
    /// The word that `measure-peak` prints for the mode: `job`, `job-commit`, `poll` or
    /// `taskpolicy`.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Job => "job",
            Mode::JobCommit => "job-commit",
            Mode::Poll => "poll",
            Mode::Taskpolicy => "taskpolicy",
        }
    }
}

/// The mechanism of this build: `Job` on Windows, and on macOS the branch of C23.6 that
/// docs/r2/trials/cap.md records. On Windows a run whose caller's token lacks the privilege
/// of the working-set maximum has `Mode::JobCommit` instead: [`check_supported`] and
/// [`Outcome::mode`] give the mode of a run.
pub const MODE: Mode = platform::MODE;

/// The name of this platform as `measure-peak` prints it: `macos` or `windows`.
pub const PLATFORM: &str = platform::PLATFORM;

/// An instant: a signed count of nanoseconds since 1970-01-01T00:00:00Z (contracts.md
/// C0.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixNanos(pub i64);

impl UnixNanos {
    /// The current instant of the system clock.
    pub fn now() -> UnixNanos {
        let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
            Err(before) => i64::try_from(before.duration().as_nanos())
                .map(|n| -n)
                .unwrap_or(i64::MIN),
        };
        UnixNanos(nanos)
    }
}

/// How the child ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// It exited with this code; on Windows the code is the 32-bit status read as signed.
    Code(i32),
    /// This signal ended it (macOS).
    Signal(i32),
    /// It could not be started.
    NotStarted,
}

/// What one run under the cap gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// How the child ended.
    pub exit: Exit,
    /// Its peak in bytes: on macOS the highest lifetime peak physical footprint read, on
    /// Windows the Job's peak committed memory of one process. 0 when it did not start.
    pub peak_bytes: u64,
    /// Whether the cap stopped it: on macOS a read above the limit, on Windows an
    /// allocation that the Job refused.
    ///
    /// `capped=no` says that no read was above the limit, and on Windows that no report of a
    /// refused allocation arrived; it never says that the program held no more than the
    /// limit, because the reading does not count what the crate's documentation lists. On
    /// Windows `capped=yes` says that the Job's report of a refused allocation arrived.
    /// Windows does not guarantee that report: a refusal whose report is lost reads
    /// `capped=no` and shows in the exit status of the run, and the control then prints
    /// `miss`. A runtime that reads the Job's limit and refuses its own allocation below it
    /// reads `capped=no` too. A child that survives a refused allocation runs to its own end.
    pub capped: bool,
    /// When it was started.
    pub start: UnixNanos,
    /// When it was seen ended; equal to `start` when it did not start.
    pub end: UnixNanos,
    /// The mechanism that held the cap in this run: on macOS the mode of this build, on
    /// Windows `Mode::Job` with the working-set maximum or `Mode::JobCommit` without it.
    pub mode: Mode,
}

impl Outcome {
    fn not_started(at: UnixNanos, mode: Mode) -> Outcome {
        Outcome {
            exit: Exit::NotStarted,
            peak_bytes: 0,
            capped: false,
            start: at,
            end: at,
            mode,
        }
    }
}

/// What to start. Each part is used as the caller gives it.
#[derive(Debug)]
pub struct ChildSpec {
    /// The program, started directly with no shell.
    pub program: OsString,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Its whole environment: the child gets these variables and no other.
    pub env: Vec<(OsString, OsString)>,
    /// Its current directory; `None` keeps the directory of this process.
    pub current_dir: Option<PathBuf>,
    /// Its standard input.
    pub stdin: Stdio,
    /// Its standard output.
    pub stdout: Stdio,
    /// Its standard error.
    pub stderr: Stdio,
}

impl ChildSpec {
    /// `program` with `args`, the environment and the current directory of this process,
    /// standard input from the null device and both outputs inherited.
    pub fn new<P, I, A>(program: P, args: I) -> ChildSpec
    where
        P: Into<OsString>,
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        ChildSpec {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: std::env::vars_os().collect(),
            current_dir: None,
            stdin: Stdio::null(),
            stdout: Stdio::inherit(),
            stderr: Stdio::inherit(),
        }
    }

    fn into_command(self) -> Command {
        let mut command = Command::new(self.program);
        command
            .args(self.args)
            .env_clear()
            .envs(self.env)
            .stdin(self.stdin)
            .stdout(self.stdout)
            .stderr(self.stderr);
        if let Some(dir) = self.current_dir {
            command.current_dir(dir);
        }
        command
    }
}

/// An end of a stream that a child inherits: a descriptor on macOS, a handle on Windows.
pub type End = platform::End;

/// What [`start`] starts: a program, its arguments and the ends of its starter that it
/// inherits.
///
/// The child gets the environment and the current directory of this process. Its standard
/// input and its standard output are the null device, and its standard error is that of
/// this process, or the null device where this process has none. Beside those three it
/// holds the ends of the spec and no other descriptor or handle of this process: on macOS
/// every other descriptor is closed before the program runs, and on Windows the child is
/// created with the list of exactly these handles (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`),
/// so a handle that another thread of this process holds inheritable at that moment does
/// not reach it.
#[derive(Debug)]
pub struct EndsSpec {
    /// The program, started directly with no shell.
    pub program: OsString,
    /// Its arguments.
    pub args: Vec<OsString>,
    #[cfg_attr(
        not(any(target_os = "macos", windows)),
        expect(dead_code, reason = "no child is started on this system")
    )]
    ends: Vec<End>,
}

impl EndsSpec {
    /// `program` with no argument and no end.
    pub fn new<P: Into<OsString>>(program: P) -> EndsSpec {
        EndsSpec {
            program: program.into(),
            args: Vec::new(),
            ends: Vec::new(),
        }
    }

    /// Lets the child inherit `end`, which this process closes when the start returns,
    /// and returns the number that names the end in the child: on macOS its descriptor, 3
    /// for the first end of the spec, 4 for the second and so on; on Windows the value of
    /// the handle. No two ends of one spec are one handle.
    pub fn inherit<E: Into<End>>(&mut self, end: E) -> u64 {
        let end = end.into();
        let name = platform::name_of(self.ends.len(), &end);
        self.ends.push(end);
        name
    }
}

/// A child that [`start`] started under a limit.
pub struct Started(platform::Started);

impl Started {
    /// The pid of the child; none for a program that could not be started.
    pub fn pid(&self) -> Option<u32> {
        self.0.pid()
    }

    /// The system's error number for a program that could not be started, where the system
    /// gave one; none for a child that was started.
    pub fn start_error(&self) -> Option<i32> {
        self.0.start_error()
    }

    /// The call that ends this child, for another thread than the one that waits.
    pub fn ender(&self) -> Ender {
        Ender(self.0.ending())
    }

    /// Blocks until the child has ended and returns what [`run`] returns: its exit, its
    /// peak, whether the cap stopped it, and the instants of its start and its end. A
    /// program that could not be started gives `Exit::NotStarted` at once. The error means
    /// that the cap could not be read; the child is then killed and reaped before the call
    /// returns. A value that is dropped with no call of `wait` ends its child.
    pub fn wait(self) -> Result<Outcome, CapError> {
        self.0.wait()
    }
}

/// Ends one child that [`start`] started.
#[derive(Clone)]
pub struct Ender(Arc<platform::Ending>);

impl Ender {
    /// Kills the child, unless [`Started::wait`] has seen it ended: on macOS with SIGKILL,
    /// which reaches the started process alone and never a later process of its pid; on
    /// Windows by the end of the Job, which ends every process of it. The call returns at
    /// once, and `wait` returns the outcome.
    pub fn end(&self) {
        self.0.end();
    }

    /// Whether a call of [`Ender::end`] reached the child before `wait` saw it ended. A
    /// child that had exited on its own in that moment keeps its own exit.
    pub fn reached(&self) -> bool {
        self.0.reached()
    }
}

#[cfg(any(windows, test))]
const PRIVILEGE_NOT_HELD: i32 = 1314;

#[cfg(any(windows, test))]
fn set_job_limits(mut set: impl FnMut(bool) -> io::Result<()>) -> io::Result<Mode> {
    match set(true) {
        Ok(()) => Ok(Mode::Job),
        Err(error) if error.raw_os_error() == Some(PRIVILEGE_NOT_HELD) => {
            set(false)?;
            Ok(Mode::JobCommit)
        }
        Err(error) => Err(error),
    }
}

/// This system cannot enforce the cap; `call` names the call that failed.
#[derive(Debug)]
pub struct CapError {
    /// The call that failed.
    pub call: &'static str,
    /// What the system said.
    pub source: io::Error,
}

impl fmt::Display for CapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.call, self.source)
    }
}

impl std::error::Error for CapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Checks, before a first run, that this system can enforce `limit`: on Windows a Job
/// object with the limit can be made, on macOS the peak physical footprint of this
/// process can be read. The error names the call that failed. It returns the mode that a
/// run has here: on Windows `Mode::Job`, or `Mode::JobCommit` where the token lacks the
/// privilege of the working-set maximum.
pub fn check_supported(limit: Limit) -> Result<Mode, CapError> {
    platform::check_supported(limit)
}

/// Starts one child under `limit`, blocks until it has ended, and returns its exit, its
/// peak, whether the cap stopped it, and the instants of its start and its end.
///
/// A program that cannot be started gives `Exit::NotStarted`, not an error. The error
/// means that the cap could not be set or read; a child that was started is then killed
/// and reaped before the call returns. On Windows the function sets the creation flags of
/// the child. A caller that must go on beside the child calls this on a thread of its own.
///
/// Which processes outlive a run: on macOS a run ends the started process alone. The
/// processes that it started go on after the run, also after a run that the cap stopped,
/// and nothing caps them. When the process that called the cap ends before the child, by a
/// signal or otherwise, the child goes on with no cap. A child of the same user can stop or
/// end the capping process. On Windows the Job ends every process of the tree when the run
/// ends and when the caller ends, but for a child whose caller ends between its start and
/// its entry into the Job. A program whose peak is a measurement starts no helper process,
/// so its run leaves nothing behind; a gate that wraps a program that does start processes
/// reads the process table after the run and ends what the run started.
pub fn run(spec: ChildSpec, limit: Limit) -> Result<Outcome, CapError> {
    platform::run(spec, limit)
}

/// Starts the child of `spec` under `limit` and returns at once. The cap is that of
/// [`run`]: [`Started::wait`] holds it and returns the outcome, on a thread that the caller
/// gives it, and [`Started::ender`] ends the child from another thread.
///
/// A program that cannot be started gives a value whose `pid` is none, not an error. The
/// error means that the cap could not be set; a child that was started is then killed and
/// reaped before the call returns.
pub fn start(spec: EndsSpec, limit: Limit) -> Result<Started, CapError> {
    platform::start(spec, limit).map(Started)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choose(answers: &[Option<i32>]) -> (io::Result<Mode>, Vec<bool>) {
        let mut asked = Vec::new();
        let mut answers = answers.iter();
        let chosen = set_job_limits(|working_set| {
            asked.push(working_set);
            match answers.next().copied().flatten() {
                None => Ok(()),
                Some(code) => Err(io::Error::from_raw_os_error(code)),
            }
        });
        (chosen, asked)
    }

    #[test]
    fn the_job_drops_the_working_set_maximum_only_when_the_privilege_is_missing() {
        let (chosen, asked) = choose(&[None]);
        assert_eq!((chosen.unwrap(), asked), (Mode::Job, vec![true]));
        let (chosen, asked) = choose(&[Some(1314), None]);
        assert_eq!(
            (chosen.unwrap(), asked),
            (Mode::JobCommit, vec![true, false])
        );
        for code in (1200..=1430)
            .chain([2, 5, 6, 8, 87, 1450, 1453, 1455])
            .filter(|&code| code != 1314)
        {
            let (chosen, asked) = choose(&[Some(code)]);
            assert_eq!(
                (chosen.unwrap_err().raw_os_error(), asked),
                (Some(code), vec![true]),
                "{code}"
            );
        }
        let (chosen, asked) = choose(&[Some(1314), Some(87)]);
        assert_eq!(
            (chosen.unwrap_err().raw_os_error(), asked),
            (Some(87), vec![true, false])
        );
    }

    #[test]
    fn each_mode_has_its_word() {
        assert_eq!(Mode::Job.as_str(), "job");
        assert_eq!(Mode::JobCommit.as_str(), "job-commit");
        assert_eq!(Mode::Poll.as_str(), "poll");
        assert_eq!(Mode::Taskpolicy.as_str(), "taskpolicy");
    }
}
