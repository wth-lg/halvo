//! Halvo's memory cap: one child process under a limit in bytes, its peak, and the
//! model lock that lets one generative model load at a time for one user
//! (contracts.md C23).
//!
//! [`run`] starts one child under a limit and returns its exit, its peak and whether
//! the cap stopped it. On Windows the child runs in a new Job object with a per-process
//! memory limit and a working-set maximum of the limit, and the peak is the Job's peak
//! process memory. On macOS the parent reads the child's lifetime peak physical
//! footprint at the start, every 250 ms and once more when the child has ended, and kills
//! the child at the first read above the limit; the cap and the peak cover the started
//! process alone, so a caller wraps the program itself and never a shell.
//!
//! What the macOS reading does not see: the memory of a grandchild; pages of a file that
//! the child maps and only reads; volatile purgeable memory; and memory that the child
//! holds only before it execs another program, because the lifetime peak starts again at
//! an exec. Between two polls the child may hold more than the limit for up to 250 ms
//! before it is killed, and `peak_bytes` then shows how far it went.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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
    /// A Job object (Windows).
    Job,
    /// The parent polls the peak physical footprint and kills on a breach (macOS).
    Poll,
    /// As `Poll`, with the child also started under `taskpolicy -m` (macOS).
    Taskpolicy,
}

impl Mode {
    /// The word that `measure-peak` prints for the mode: `job`, `poll` or `taskpolicy`.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Job => "job",
            Mode::Poll => "poll",
            Mode::Taskpolicy => "taskpolicy",
        }
    }
}

/// The mechanism of this build: `Job` on Windows, and on macOS the branch of C23.6 that
/// docs/r2/trials/cap.md records.
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
    pub capped: bool,
    /// When it was started.
    pub start: UnixNanos,
    /// When it was seen ended; equal to `start` when it did not start.
    pub end: UnixNanos,
}

impl Outcome {
    fn not_started(at: UnixNanos) -> Outcome {
        Outcome {
            exit: Exit::NotStarted,
            peak_bytes: 0,
            capped: false,
            start: at,
            end: at,
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
/// process can be read. The error names the call that failed.
pub fn check_supported(limit: Limit) -> Result<(), CapError> {
    platform::check_supported(limit)
}

/// Starts one child under `limit`, blocks until it has ended, and returns its exit, its
/// peak, whether the cap stopped it, and the instants of its start and its end.
///
/// A program that cannot be started gives `Exit::NotStarted`, not an error. The error
/// means that the cap could not be set or read; a child that was started is then killed
/// and reaped before the call returns. On Windows the function sets the creation flags of
/// the child; the processes that the child leaves behind are ended with it, because the
/// Job holds them. A caller that must go on beside the child calls this on a thread of
/// its own.
pub fn run(spec: ChildSpec, limit: Limit) -> Result<Outcome, CapError> {
    platform::run(spec, limit)
}
