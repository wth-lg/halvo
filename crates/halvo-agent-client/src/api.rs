use std::fmt;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use halvo_schema::executable::frame::{
    ConfirmDecision, ConfirmRequest, HandbackRequest, SecretReply,
};
use halvo_schema::executable::names::{Answer, Mode, Outcome};
use halvo_schema::executable::record::AgentRun;
use halvo_schema::executable::text::{Name, RunId, WebOrigin};
use halvo_schema::{AgentId, WorkflowId};

/// How long a start waits for the runner's `hello` before it ends what it made and answers `not_started`.
pub const START_WAIT: Duration = Duration::from_secs(10);

/// How long a runner whose link closed has to end by itself (A6.1, a constant of halvo-runner): after it the
/// library ends what is left of the run.
pub const LINK_END: Duration = Duration::from_secs(20);

/// The error of a call on a run: the closed list `started`, `not_started`, `unknown_run` and `ended` (A6.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallError {
    /// A start of a run id that the library has started, or whose mark of an earlier life stands.
    Started,
    /// The runner could not be started or was gone before the start answered; no process of the run exists.
    NotStarted,
    /// A call on a run id that the library did not start in this life of its process.
    UnknownRun,
    /// `cancel` or `answer_handback` of a run whose runner has ended, or that holds no open handback.
    Ended,
}

impl CallError {
    /// The name of the error.
    pub fn as_str(self) -> &'static str {
        match self {
            CallError::Started => "started",
            CallError::NotStarted => "not_started",
            CallError::UnknownRun => "unknown_run",
            CallError::Ended => "ended",
        }
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for CallError {}

/// What a start gives the runner: its arguments `--run`, `--agent`, `--workflow`, `--mode`, `--browser` as the
/// caller gave it, `--model` and `--headed`.
#[derive(Clone, Debug)]
pub struct RunSpec {
    pub run: RunId,
    pub agent: AgentId,
    pub workflow: WorkflowId,
    pub mode: Mode,
    pub browser: PathBuf,
    pub model: Option<PathBuf>,
    pub headed: bool,
}

/// The failure of one request on the owner link: the link returns it for the request in flight and does not
/// send that request again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkError;

/// The owner link: one call, request bytes in, reply bytes out.
pub trait OwnerLink: Send + Sync {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError>;
}

/// The refusal of `Confirmer::issued`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IssueRefused;

/// The confirmation of a run's effects.
pub trait Confirmer: Send + Sync {
    /// One answer per effect of the request, in its order.
    fn answers(&self, run: &RunId, request: &ConfirmRequest) -> Vec<Answer>;
    /// Records the decision before the runner gets it; an error makes the library reject every effect.
    fn issued(&self, run: &RunId, decision: &ConfirmDecision) -> Result<(), IssueRefused>;
}

/// The person's answer to the second step of a `sign_in` item, "This agent will work on <origin>".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginAnswer {
    Allow,
    Decline,
}

/// The person, as the caller shows the run to them.
pub trait Human: Send + Sync {
    /// Opens a handback; its result comes back by `Supervisor::answer_handback`.
    fn handback(&self, run: &RunId, request: &HandbackRequest);
    /// Asks whether the agent may work on `origin`.
    fn allow_origin(&self, run: &RunId, origin: &WebOrigin) -> OriginAnswer;
    /// The step that the runner reached, `step` at most `of`.
    fn progress(&self, run: &RunId, step: NonZeroU32, of: NonZeroU32);
}

/// The supervisor's secret store: the secret, none stored, or unreadable with its member.
pub trait Secrets: Send + Sync {
    fn read(&self, name: &Name) -> SecretReply;
}

/// How a run's end stood with the owner when the library had done with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndRun {
    /// No runner was made; the library sends no `end_run`, and its caller owes it.
    NoRunner,
    /// The owner acknowledged the run's `finish`; no `end_run` is owed.
    NotOwed,
    /// The owner acknowledged the library's `end_run`.
    Acknowledged,
    /// The library's `end_run` got no acknowledgement; the caller still owes it.
    NotAcknowledged,
}

/// The end of a run: the outcome of the runner's `done`, or none when it ended with no `done`; the record of its
/// acknowledged `finish`, which holds the counts of A14.5; and where its `end_run` stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunEnd {
    pub outcome: Option<Outcome>,
    pub finish: Option<AgentRun>,
    pub end_run: EndRun,
}

/// How a runner's process ended, as the system gave it to the library when the library reaped it: the status that
/// it exited with, of which 2 is a refusal (A0.3), or on macOS the signal that ended it. A runner that the library
/// ended exits by the signal SIGKILL on macOS and with the status 1 on Windows; a runner that the system reaped for
/// the library's process, as it does for a process that ignores SIGCHLD on macOS, leaves the library no status. On
/// Windows the status 1 is what a runner gives that the library ended and what a runner gives that exits with 1 by
/// itself: the two cannot be told apart there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerExit {
    /// The runner exited with this status.
    Status(i32),
    /// A signal of this number ended the runner (macOS).
    #[cfg_attr(
        all(windows, not(test)),
        expect(dead_code, reason = "no signal ends a process on Windows")
    )]
    Signal(i32),
}

impl RunnerExit {
    /// Whether the runner refused: it exited with the status 2.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "RunEnd and the start's answer carry it from the next unit of src/supervisor.rs (U4.9's report)"
        )
    )]
    pub fn is_refusal(self) -> bool {
        self == RunnerExit::Status(2)
    }
}

/// The calls before a runner starts and after it has ended, however it ended.
pub trait MarkSink: Send + Sync {
    fn start(&self, run: &RunId);
    fn end(&self, run: &RunId, end: &RunEnd);
}

/// The traits that the caller supplies.
#[derive(Clone)]
pub struct Parts {
    pub link: Arc<dyn OwnerLink>,
    pub confirmer: Arc<dyn Confirmer>,
    pub human: Arc<dyn Human>,
    pub secrets: Arc<dyn Secrets>,
    pub marks: Arc<dyn MarkSink>,
}

/// Where the library keeps its marks, the runner binary that it starts, how long a start waits, and how long a
/// runner whose link closed has to end by itself before the library ends what is left.
///
/// The library starts the runner with three of its handles or descriptors open in it and no other: the two ends of
/// the link as the runner's standard input and output, and the null device as its standard error, which nobody
/// reads. On Windows the runner gets a console of its own that shows no window, which a process that it starts
/// without a console flag of its own shares, so the runner starts attached to no console of the library's process;
/// a runner under the library's token can still attach itself to a console of the library's process by that
/// process's number, as it can open its parent process. A process that another thread of the library's process
/// starts in the moment in which a runner is started, by another start, the standard library's among them, can hold
/// the ends of the runner's link: on Windows while the three handles are inheritable for the start, on macOS between
/// the making of a pipe and its close-on-exec flag. While the run lives, such a process can read and write the
/// frames of the link. The library cannot keep such a process out. What bounds it: the library ends a runner
/// `link_end` after it closed the link, whether or not the runner saw the end of its input, and it takes a runner's
/// end from its process and not from the end of its output alone. The library closes the link of a run `link_end`
/// after the run's `finish` was stored when its runner has not ended by then, and from that close the runner has
/// `link_end`, as after any close. The library reads that bound between two events of the run: a call of its
/// caller in which it waits, such as `Confirmer::answers` for a `confirm` frame that a runner sends after its
/// stored `finish`, is not cut short by it, and the close comes once that call has returned.
/// The runner gets a whole environment of its own and nothing else of the library's: on macOS `HOME`, as the
/// library's process has it, and `TMPDIR`, the per-user temporary directory that the system answers; on Windows
/// `LOCALAPPDATA`, as the library's process has it, and `SystemRoot`, `TEMP` and `TMP`, as the system answers them
/// for the account of the library's token, `TEMP` and `TMP` the directory Temp under its LocalAppData known folder.
/// `HOME` and `LOCALAPPDATA` are not set where the library's process has none. The runner works in `/` on macOS
/// and in the system directory on Windows, never in the library's working directory or in `data_dir`, so `runner`,
/// `data_dir` and the paths of a `RunSpec` must be absolute; `absolute_paths` names the first that is not, and
/// `Supervisor::start` does not call it yet.
#[derive(Clone, Debug)]
pub struct Config {
    pub data_dir: PathBuf,
    pub runner: PathBuf,
    pub start_wait: Duration,
    pub link_end: Duration,
}

impl Config {
    /// The configuration with `START_WAIT` and `LINK_END`.
    pub fn new(data_dir: PathBuf, runner: PathBuf) -> Config {
        Config {
            data_dir,
            runner,
            start_wait: START_WAIT,
            link_end: LINK_END,
        }
    }
}

/// A path of a start that is not absolute, named by what it is (ruling CP-160 (1) (e)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelativePath {
    /// `Config::runner`, the runner binary.
    Runner,
    /// `RunSpec::browser`, which the library passes on as `--browser`.
    Browser,
    /// `RunSpec::model`, which the library passes on as `--model`.
    Model,
    /// `Config::data_dir`.
    DataDir,
}

/// Refuses a start whose runner path, `--browser`, `--model` or data directory is not absolute, naming the first of
/// them in that order: the runner works in the system's directory and not in the library's, where a relative path
/// names another file. `Supervisor::start` does not call it yet; until it does, a relative path reaches the runner
/// as its caller gave it.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "its call at the top of Supervisor::start is the first line of the next unit of src/supervisor.rs"
    )
)]
pub fn absolute_paths(config: &Config, spec: &RunSpec) -> Result<(), RelativePath> {
    let paths = [
        (Some(&config.runner), RelativePath::Runner),
        (Some(&spec.browser), RelativePath::Browser),
        (spec.model.as_ref(), RelativePath::Model),
        (Some(&config.data_dir), RelativePath::DataDir),
    ];
    for (path, name) in paths {
        if path.is_some_and(|p| !p.is_absolute()) {
            return Err(name);
        }
    }
    Ok(())
}

/// The answer of `reap` for one mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaped {
    /// No runner of the run lives: it was gone or the library ended it, or its mark was not complete.
    Gone,
    /// The runner lives and could not be ended.
    Alive,
}

/// One answer of `reap`: the run, `gone` or `alive`, and the processes of a group whose leader is gone, which the
/// library reports and does not end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReapAnswer {
    pub run: RunId,
    pub answer: Reaped,
    pub left: Vec<NonZeroU32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    const ROOT: &str = "/halvo";
    #[cfg(windows)]
    const ROOT: &str = r"C:\halvo";

    fn absolute(name: &str) -> PathBuf {
        PathBuf::from(ROOT).join(name)
    }

    fn spec(browser: PathBuf, model: Option<PathBuf>) -> RunSpec {
        RunSpec {
            run: RunId::from_random([1; 16]),
            agent: AgentId::from_random([2; 16]),
            workflow: WorkflowId::from_random([3; 16]),
            mode: Mode::Replay,
            browser,
            model,
            headed: false,
        }
    }

    fn config(data_dir: PathBuf, runner: PathBuf) -> Config {
        Config::new(data_dir, runner)
    }

    #[test]
    fn a_start_whose_paths_are_all_absolute_passes_with_a_model_and_without() {
        let c = config(absolute("data"), absolute("runner"));
        assert_eq!(absolute_paths(&c, &spec(absolute("browser"), None)), Ok(()));
        assert_eq!(
            absolute_paths(&c, &spec(absolute("browser"), Some(absolute("model")))),
            Ok(())
        );
    }

    #[test]
    fn of_the_ends_of_a_runner_the_status_2_alone_is_a_refusal() {
        for status in [0, 1, 3, 255] {
            assert!(
                !RunnerExit::Status(status).is_refusal(),
                "the status {status} is no refusal"
            );
        }
        assert!(RunnerExit::Status(2).is_refusal(), "the status 2 is one");
        for signal in [2, 9] {
            assert!(
                !RunnerExit::Signal(signal).is_refusal(),
                "the signal {signal} is no refusal"
            );
        }
    }

    #[test]
    fn a_relative_runner_path_is_refused() {
        let c = config(absolute("data"), PathBuf::from("runner"));
        assert_eq!(
            absolute_paths(&c, &spec(absolute("browser"), None)),
            Err(RelativePath::Runner)
        );
    }

    #[test]
    fn a_relative_browser_path_is_refused() {
        let c = config(absolute("data"), absolute("runner"));
        assert_eq!(
            absolute_paths(&c, &spec(PathBuf::from("bin/browser"), None)),
            Err(RelativePath::Browser)
        );
    }

    #[test]
    fn a_relative_model_path_is_refused() {
        let c = config(absolute("data"), absolute("runner"));
        assert_eq!(
            absolute_paths(
                &c,
                &spec(absolute("browser"), Some(PathBuf::from("model.gguf")))
            ),
            Err(RelativePath::Model)
        );
    }

    #[test]
    fn a_relative_data_directory_is_refused() {
        let c = config(PathBuf::from("data"), absolute("runner"));
        assert_eq!(
            absolute_paths(&c, &spec(absolute("browser"), None)),
            Err(RelativePath::DataDir)
        );
    }

    #[test]
    fn of_several_relative_paths_the_first_in_the_order_runner_browser_model_data_directory_is_named(
    ) {
        let all = config(PathBuf::from("data"), PathBuf::from("runner"));
        let relative = spec(PathBuf::from("browser"), Some(PathBuf::from("model")));
        assert_eq!(absolute_paths(&all, &relative), Err(RelativePath::Runner));
        let data = config(PathBuf::from("data"), absolute("runner"));
        assert_eq!(absolute_paths(&data, &relative), Err(RelativePath::Browser));
        let model = spec(absolute("browser"), Some(PathBuf::from("model")));
        assert_eq!(absolute_paths(&data, &model), Err(RelativePath::Model));
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_with_no_drive_is_refused() {
        let c = config(PathBuf::from(r"\data"), absolute("runner"));
        assert_eq!(
            absolute_paths(&c, &spec(absolute("browser"), None)),
            Err(RelativePath::DataDir)
        );
    }

    #[test]
    fn a_new_config_waits_ten_seconds_for_hello_and_link_end_twenty_seconds_for_a_closed_link() {
        let c = Config::new(PathBuf::from("data"), PathBuf::from("runner"));
        assert_eq!(c.start_wait, Duration::from_secs(10));
        assert_eq!(c.link_end, Duration::from_secs(20));
        assert_eq!((START_WAIT, LINK_END), (c.start_wait, c.link_end));
    }
}
