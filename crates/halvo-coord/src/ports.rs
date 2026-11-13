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
    /// Sends the result of a handback. The port's duty (ruling RD-46 (2) g): it returns an
    /// error other than the refusal `ended` only when the run's waiting call did not get the
    /// result, so that the coordinator, which closes the item after a success or after `ended`
    /// only, may send a result again.
    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError>;
    /// Sends the answer of the second step of a `sign_in` item, with which the supervisor
    /// library's call `Human::allow_origin` of the run returns (I3.1, I6.2). The port's duty
    /// (ruling RD-46 (2) g): it returns an error other than the refusal `ended` only when the
    /// waiting call did not get the answer, so that the coordinator, which closes the step after
    /// a success or after `ended` only, may send that answer again or `decline`.
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

/// The clock. It gives the day of an instant, so that a day and an instant are never handed
/// over as a pair that can disagree (I7.1; the agent-setup run's CP-122 (a)).
pub trait Clock {
    /// Milliseconds since 1970-01-01T00:00:00Z.
    fn now(&self) -> Result<u64, PortError>;
    /// The local calendar day of the instant `at`, in milliseconds as `now` gives them, as days
    /// since 1970-01-01 (W-C2.5).
    fn day(&self, at: u64) -> Result<u32, PortError>;
    /// The local calendar day as days since 1970-01-01: the day of one read of `now`.
    fn today(&self) -> Result<u32, PortError> {
        let at = self.now()?;
        self.day(at)
    }
}

/// The instant of a row and its day from one read of the clock: `Clock::now` once, and
/// `Clock::day` of that instant (I7.1). Only `Stamp::read` makes one, so a row's `day` is the
/// local day of its `at`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    now: u64,
    day: u32,
}

impl Stamp {
    /// The last instant, in milliseconds, whose nanoseconds a row's `at` holds.
    pub const LAST: u64 = i64::MAX as u64 / 1_000_000;

    /// Reads the clock once and gives the day of that instant; an instant after `Stamp::LAST`,
    /// which no `at` can hold, is the clock's failure `no_service`.
    pub fn read<P: Clock + ?Sized>(clock: &P) -> Result<Stamp, PortError> {
        let now = clock.now()?;
        if now > Stamp::LAST {
            return Err(PortError::NoService);
        }
        let day = clock.day(now)?;
        Ok(Stamp { now, day })
    }

    /// The instant in milliseconds, as `Clock::now` gave it.
    pub fn now(self) -> u64 {
        self.now
    }

    /// The local calendar day of the instant.
    pub fn day(self) -> u32 {
        self.day
    }

    /// The instant as a row's `at`: nanoseconds, `now` times 1,000,000.
    pub fn at(self) -> i64 {
        i64::try_from(self.now).map_or(i64::MAX, |n| n.saturating_mul(1_000_000))
    }
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
