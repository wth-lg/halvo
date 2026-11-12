//! The ports of the coordinator (I5.10). Every method is synchronous and returns
//! `Result<T, PortError>`; every method takes `&self`, so that one value can implement every
//! port and a port can call another while it runs, as `Runs::answer_confirm` calls `record`.

use crate::types::{
    Agent, AgentId, Answer, BindResult, Body, ConfigView, Decision, Event, HandbackResult, Name,
    Reply, RunHistory, RunId, RunSpec, Workflow, WorkflowId,
};

/// Why a port call did not do its work (I5.10). The coordinator answers a command with `busy`
/// or `no_service` accordingly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PortError {
    /// The owner or the relay answered busy.
    Busy,
    /// No daemon, no consent yet, or no acknowledgement.
    NoService,
    /// The owner refused, with the error name of its reply.
    Refused(Name),
}

/// The derived owner of the workflow run, which stores this run's events and answers its bodies.
pub trait Owner {
    /// The configuration type of the agents that `agents` lists.
    type Config: ConfigView;
    /// Stores the events; success is the acknowledgement. The owner refuses a body with the
    /// error names of I7.2 (`no_workflow`, `binding`, `seq`, `malformed`, `internal`) and then
    /// stores none of its events.
    fn log(&self, events: &[Event]) -> Result<(), PortError>;
    /// The events of the agent with a `seq` above `after`, in ascending `seq`, at most 64 for
    /// one call, as `coord_history` answers (I7.2); the caller asks again from the last `seq`
    /// that it holds.
    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError>;
    /// The owner's list of workflows.
    fn workflows(&self) -> Result<Vec<Workflow>, PortError>;
    /// The agents, as the reply of I6.1 lists them.
    fn agents(&self) -> Result<Vec<Agent<Self::Config>>, PortError>;
    /// Sends a body of A-A7.2 and gives its reply.
    fn send(&self, body: &Body) -> Result<Reply, PortError>;
    /// The agent's last `runs` runs, newest first, each with its effects, as the reply of the
    /// body `agent_history` gives them (I6.1, I2.6, ruling RD-10). An owner that does not read
    /// them answers `no_service`, which is the answer of this default.
    fn agent_history(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        runs: u32,
    ) -> Result<Vec<RunHistory>, PortError> {
        let _ = (workflow, agent, runs);
        Err(PortError::NoService)
    }
}

/// The runners of the agent-setup run, through its supervisor library.
pub trait Runs {
    /// Starts the runner of the run.
    fn start(&self, spec: &RunSpec) -> Result<(), PortError>;
    /// Mints the approval ids of the answers, calls `record` with the `Decision`, and sends the
    /// decision only when `record` returned success; else sends `reject_once` for every effect
    /// under approval ids that it mints anew and returns the error of `record`. So a success
    /// return means that the decision that `record` accepted was sent (I4.5).
    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError>;
    /// Sends the result of a handback.
    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError>;
    /// Sends the answer of the second step of a `sign_in` item, with which the supervisor
    /// library's call `Human::allow_origin` of the run returns (I3.1, I6.2).
    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError>;
    /// Sends the frame `cancel`.
    fn cancel(&self, run: &RunId) -> Result<(), PortError>;
    /// Ends the runner's whole tree.
    fn kill(&self, run: &RunId) -> Result<(), PortError>;
    /// Leaves no process of an earlier life of the app running and answers for every mark
    /// that the supervisor library holds, one answer per run (I5.8, I6.2): `gone` when the
    /// run's runner is gone, after which the library holds no mark of it, and `alive` when it
    /// lives.
    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError>;
}

/// The answer of `Runs::reap` for the mark of one run (I5.10, I6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaped {
    /// The runner's process is gone.
    Gone,
    /// The runner's process lives.
    Alive,
}

impl Reaped {
    /// The answer's name.
    pub const fn name(self) -> &'static str {
        match self {
            Reaped::Gone => "gone",
            Reaped::Alive => "alive",
        }
    }
}

/// The workflow run's analysis batch.
pub trait Analysis {
    /// Asks to defer the batch until `until`, in milliseconds as `Clock::now` gives them.
    fn defer(&self, until: u64) -> Result<(), PortError>;
}

/// The clock.
pub trait Clock {
    /// Milliseconds since 1970-01-01T00:00:00Z.
    fn now(&self) -> Result<u64, PortError>;
    /// The local calendar day as days since 1970-01-01.
    fn today(&self) -> Result<u32, PortError>;
}

/// The source of random bytes.
pub trait Rng {
    /// 16 random bytes.
    fn bytes16(&self) -> Result<[u8; 16], PortError>;
}

/// The tray of the app.
pub trait Tray {
    /// Shows the count of agents under `needs_you`.
    fn attention(&self, count: u32) -> Result<(), PortError>;
}

/// The page of the app.
pub trait Page {
    /// Tells the page's waiting call that the state changed.
    fn changed(&self) -> Result<(), PortError>;
}
