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

    #[test]
    fn a_new_config_waits_ten_seconds_for_hello_and_link_end_twenty_seconds_for_a_closed_link() {
        let c = Config::new(PathBuf::from("data"), PathBuf::from("runner"));
        assert_eq!(c.start_wait, Duration::from_secs(10));
        assert_eq!(c.link_end, Duration::from_secs(20));
        assert_eq!((START_WAIT, LINK_END), (c.start_wait, c.link_end));
    }
}
