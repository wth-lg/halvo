use super::Queue;
use crate::types::{AgentId, Facts, ItemId, Lapsed, RunEnd, RunId, WorkflowId};

/// What the coordinator owes the owner for an agent, sent again at each tick until the owner
/// acknowledges it (I5.8, I5.9, rulings XB-14, XB-17 and XB-32).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owing {
    /// The body `end_run` of a run that has no runner: its step 7 or 8 failed (XB-14).
    EndRun {
        /// The run.
        run: RunId,
    },
    /// The event `run_ended` of a run with its cause, its day and its instant in nanoseconds,
    /// which the owner did not acknowledge (XB-17).
    RunEnded {
        /// The run.
        run: RunId,
        /// The cause of I5.6.
        cause: RunEnd,
        /// The day of the end.
        day: u32,
        /// The instant of the end.
        at: i64,
    },
    /// The event `item_lapsed` of an item that an earlier life left open, with its lapse, its
    /// day and its instant in nanoseconds, in a body of recovery that the owner did not
    /// acknowledge (I5.8, XB-32).
    ItemLapsed {
        /// The item.
        item: ItemId,
        /// The lapse, of the cause `app_ended`.
        lapsed: Lapsed,
        /// The day of recovery.
        day: u32,
        /// The instant of recovery.
        at: i64,
    },
}

/// One thing that the coordinator owes for an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owed {
    /// The agent.
    pub agent: AgentId,
    /// The agent's workflow.
    pub workflow: WorkflowId,
    /// What is owed.
    pub what: Owing,
}

/// One agent on the board: its facts (I5.1) and whether it has a stored session, which the
/// head of a run reads (I5.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member<C> {
    /// The facts of I5.1; the board keeps their day and their run state.
    pub facts: Facts<C>,
    /// Whether the agent has a stored session (I6.1 `session`).
    pub session: bool,
}

/// The stop of a live run: when it came, in milliseconds of `Clock::now`, and its cause,
/// `cancelled`, `lapsed` or `deleted` (I5.6, I5.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    /// `Clock::now()` at the stop.
    pub at: u64,
    /// The cause with which the run ends.
    pub cause: RunEnd,
}

/// A live run that this life of the app started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveRun {
    /// The run.
    pub run: RunId,
    /// Its agent.
    pub agent: AgentId,
    /// Its agent's workflow.
    pub workflow: WorkflowId,
    /// Whether it holds the on-device model.
    pub holds_model: bool,
    /// The milliseconds of its request to defer (I5.5 step 5).
    pub defer_ms: u64,
    /// The rank of its start in this life: the order in which runs are stopped and killed.
    pub rank: u64,
    /// Its stop, or none.
    pub stop: Option<Stop>,
}

/// A run that was stopped and whose runner ended before the kill: the first tick at which
/// `cancel_grace_s` have passed since the stop still calls `Runs::kill` for it (I5.7, I5.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dying {
    /// The run.
    pub run: RunId,
    /// The rank of its start.
    pub rank: u64,
    /// `Clock::now()` at its stop.
    pub stopped: u64,
}

/// The coordinator's state of the schedule in one life of the app, which `drive` reads and
/// changes: the day of its last tick, the agents in ascending id, the queue, the live runs and
/// the stopped runs that wait for their kill in the order of their starts, the workflows that a
/// tick saw gone, whether the last creation found an approved workflow beyond `roster_max` (I2.5
/// `note.roster_full`), and what the coordinator owes the owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board<C> {
    /// The coordinator's day (I5.9).
    pub day: u32,
    /// The agents, in ascending agent id.
    pub members: Vec<Member<C>>,
    /// The starts that wait.
    pub queue: Queue,
    /// The live runs, in the order of their starts.
    pub runs: Vec<LiveRun>,
    /// The stopped runs whose runner ended before the kill.
    pub dying: Vec<Dying>,
    /// The workflows that a tick saw gone from the owner's list.
    pub gone: Vec<WorkflowId>,
    /// Whether the last creation left an approved workflow with no agent for `roster_max`.
    pub roster_full: bool,
    /// What the coordinator owes the owner, in the order in which it came to be owed.
    pub owed: Vec<Owed>,
    ranks: u64,
    listed: bool,
}

impl<C> Board<C> {
    /// The board of a new life of the app on `day` (I5.8, I5.9): the members in ascending
    /// agent id, each with that day, and an empty queue, no live run and nothing seen gone.
    pub fn new(day: u32, mut members: Vec<Member<C>>) -> Board<C> {
        members.sort_by(|a, b| a.facts.agent.cmp(&b.facts.agent));
        for m in &mut members {
            m.facts.today = day;
        }
        Board {
            day,
            members,
            queue: Queue::default(),
            runs: Vec::new(),
            dying: Vec::new(),
            gone: Vec::new(),
            roster_full: false,
            owed: Vec::new(),
            ranks: 0,
            listed: false,
        }
    }

    /// Whether this is the life's first creation move that read the owner's list, which raises
    /// `setup` again (I5.8): later in the life an agent's slots on the board can be older than its
    /// acknowledged bindings, so that the hold `setup` reads as standing when it no longer does.
    pub(super) fn first_listing(&mut self) -> bool {
        !std::mem::replace(&mut self.listed, true)
    }

    pub(super) fn rank(&mut self) -> u64 {
        self.ranks += 1;
        self.ranks
    }

    pub(super) fn index(&self, agent: &AgentId) -> Option<usize> {
        self.members.iter().position(|m| &m.facts.agent == agent)
    }
}
