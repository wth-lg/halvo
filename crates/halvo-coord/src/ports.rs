//! The ports of the coordinator (I5.10). Every method is synchronous and returns
//! `Result<T, PortError>`; every method takes `&self`, so that one value can implement every
//! port and a port can call another while it runs, as `Runs::answer_confirm` calls `record`.

use crate::types::{
    Agent, AgentId, Answer, BindResult, Body, ConfigView, Decision, Event, HandbackResult, Name,
    Reply, ReplyRow, RunHistory, RunId, RunSpec, Slot, Workflow, WorkflowId,
};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::fmt;
use std::num::NonZeroU32;

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
    /// one call, as `coord_history` answers (I7.2), and the reply's `last`: the largest `seq`
    /// among the agent's rows that the read hands out, 0 with none, whatever `after` is, which
    /// the three places of ruling RD-64 3.2, the items' ledger, the schedule's log and recovery's
    /// body, take as the stored last, also where it is below a `seq` that they hold (I5.10,
    /// I7.2; ruling RD-64 3.1, 3.2); the caller asks again from the last `seq` that it holds.
    fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError>;
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

    /// The rows of `agent_reply` of the run `run` of the agent, in the order in which the store
    /// keeps them (ruling RD-67 3.4). A row that the reader cannot read is `ReplyRow::Unreadable`
    /// and fails no other row; a read that did not answer is an error, never an empty list. The
    /// owner refuses a workflow that is not in its list with `no_workflow` and an agent that is
    /// not the workflow's with `binding`. The coordinator asks it for an agent of the category
    /// `email` alone (I5.4; ruling RD-81 8.4).
    fn replies(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        run: &RunId,
    ) -> Result<Vec<ReplyRow>, PortError>;

    /// The read `list_overlaps` of the agent-setup run's A7.2, whose body holds `op`, then
    /// `workflow` and `agent`, then `slots`: the other live agents that answer mail from a sender
    /// of the agent on its mailbox, with its slots as they would stand with `slots` merged by
    /// name into its newest bindings, or as they stand where `slots` is none. It stores nothing.
    /// A read that did not answer is an error, never an empty list; the owner refuses a workflow
    /// that is not in its list with `no_workflow` and an agent that is not the workflow's with
    /// `binding`. An owner that does not read them answers `no_service`, which is the answer of
    /// this default (CP-150 (1) (e), CP-234 (a); ruling RD-67 3.2).
    fn overlaps(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        slots: Option<&[Slot]>,
    ) -> Result<Overlaps, PortError> {
        let _ = (workflow, agent, slots);
        Err(PortError::NoService)
    }

    /// Sends the decision `delete` of the workflow, the one delete of a workflow and its agent
    /// (W-C3.10); success is the acknowledgement, after which the owner holds no row that names
    /// the workflow. The owner refuses a delete while a run of the workflow's agent is live
    /// with the name `state`, and a workflow that is not in its list with `no_workflow`
    /// (rulings RD-19 (d) (1), RD-30 (1)). An owner that does not take the decision answers
    /// `no_service`, which is the answer of this default.
    fn delete(&self, workflow: &WorkflowId) -> Result<(), PortError> {
        let _ = workflow;
        Err(PortError::NoService)
    }
}

/// The most entries of one answer of `Owner::overlaps` (the agent-setup run's A7.2).
pub const OVERLAPS_MAX: usize = 16;

/// The answer of `Owner::overlaps`: at most `OVERLAPS_MAX` other agents in ascending order of
/// workflow id, each workflow and each agent once, and `more`, which says that more agents share
/// senders than the entries name (the agent-setup run's A7.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overlaps {
    /// The other agents, each with the senders that it shares.
    pub entries: Vec<Overlap>,
    /// Whether more agents share senders than `entries` names.
    pub more: bool,
}

/// One other live agent of `Overlaps`: its workflow, its agent and the senders that it shares
/// with the asking agent, at least one, in the spelling of the asking agent's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overlap {
    /// The other agent's workflow.
    pub workflow: WorkflowId,
    /// The other agent.
    pub agent: AgentId,
    /// The shared senders.
    pub senders: Vec<String>,
}

/// What the core holds to show for a refusal `overlap` (I3.5 (c), (d), (f); ruling RD-67 3.2),
/// always from the read that followed the newest refusal and never from an earlier one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shared {
    /// The read answered with these entries, at least one.
    Named(Overlaps),
    /// The read did not answer, so the sentence names nobody; `again` says whether waiting can
    /// change that (no answer, `busy`, `store_unavailable`, `internal`), so that a tick asks the
    /// read once more.
    Unread {
        /// Whether a tick asks the read once more.
        again: bool,
    },
}

impl Shared {
    /// What a read's answer gives to show: its entries where it named one, none for an answer
    /// with no entry, and the half that is known where it did not answer.
    pub fn of(read: Result<Overlaps, PortError>) -> Option<Shared> {
        match read {
            Ok(o) if o.entries.is_empty() => None,
            Ok(o) => Some(Shared::Named(o)),
            Err(e) => Some(Shared::Unread {
                again: waiting_can_change(&e),
            }),
        }
    }

    /// Whether the agent `agent` is one that the sentence names.
    pub fn names(&self, agent: &AgentId) -> bool {
        matches!(self, Shared::Named(o) if o.entries.iter().any(|e| &e.agent == agent))
    }
}

/// Whether a failed call can answer otherwise after a wait: no answer, `busy`,
/// `store_unavailable` or `internal` (I3.5 (d)).
pub fn waiting_can_change(error: &PortError) -> bool {
    match error {
        PortError::Busy | PortError::NoService => true,
        PortError::Refused(name) => {
            matches!(name.as_str(), "busy" | "store_unavailable" | "internal")
        }
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

/// The reader of the senders of a set-up (I3.5, I5.10; ruling RD-81 9.1 and 9.2): whether a text
/// for the parameter `reply_senders`, typed or offered, is taken or refused by entry. It decides
/// and never rewrites, so a text that it takes is kept exactly as the person gave it. Like
/// `guard::Policy` it is a bound of `Coordinator` with no default answer; the app fills it with
/// the schema's reader (U5.4), and the memory `Ports` answers from the rows that a test gives.
pub trait Senders {
    /// What the reader answers for `text`.
    fn read(&self, text: &str) -> Result<Reading, PortError>;
}

/// What `Senders::read` answers: taken, or refused by entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// The text is taken as it stands.
    Taken,
    /// The text is refused whole, by the entry that `Refused` names.
    Refused(Refused),
}

/// A refusal of `Senders::read` by entry, in the kinds of the agent-setup run's CP-165 (5) (c),
/// with the number of the entry counted from 1, both numbers for an address that stands twice.
/// It reaches the page as the key `refused` of the parameter (I3.5, I8.1), `{"entry":<n>,
/// "kind":"<kind>"}` with `"earlier":<n>` for `twice`, and the numbers go into no row, log line
/// or frame: `Debug` prints none.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The entry is empty.
    Empty(NonZeroU32),
    /// The entry holds a letter outside ASCII, which Halvo does not take: a limit of Halvo, apart
    /// from `NotAnAddress`, so that the person is not told that they typed something wrong.
    NotAscii(NonZeroU32),
    /// The entry is no address by the rule for a sender.
    NotAnAddress(NonZeroU32),
    /// The entry `later` is one address with the entry `earlier`.
    Twice {
        /// The number of the earlier entry.
        earlier: NonZeroU32,
        /// The number of the later entry.
        later: NonZeroU32,
    },
}

impl Refused {
    /// The number of the refused entry, the later one for `Twice`.
    pub const fn entry(self) -> NonZeroU32 {
        match self {
            Refused::Empty(n) | Refused::NotAscii(n) | Refused::NotAnAddress(n) => n,
            Refused::Twice { later, .. } => later,
        }
    }

    /// The kind's name: `empty`, `not_ascii`, `not_an_address` or `twice`.
    pub const fn kind(self) -> &'static str {
        match self {
            Refused::Empty(_) => "empty",
            Refused::NotAscii(_) => "not_ascii",
            Refused::NotAnAddress(_) => "not_an_address",
            Refused::Twice { .. } => "twice",
        }
    }
}

impl fmt::Debug for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Refused({})", self.kind())
    }
}

impl Serialize for Refused {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let earlier = match self {
            Refused::Twice { earlier, .. } => Some(earlier.get()),
            _ => None,
        };
        let mut out = s.serialize_struct("Refused", 2 + usize::from(earlier.is_some()))?;
        out.serialize_field("entry", &self.entry().get())?;
        out.serialize_field("kind", self.kind())?;
        if let Some(earlier) = earlier {
            out.serialize_field("earlier", &earlier)?;
        }
        out.end()
    }
}
