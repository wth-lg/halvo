use super::Queue;
use crate::ports::Reaped;
use crate::types::{AgentId, Defers, Facts, ItemId, Lapsed, RunEnd, RunId, WorkflowId};

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Stuck {
    ticks: u32,
    refused: bool,
    acked: bool,
    held: bool,
    for_life: bool,
    gone: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Dropped {
    count: u32,
    event: String,
    refusal: String,
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
    /// Whether its last `Runs::kill` was refused, so that the run outlived a kill (I5.7, ruling
    /// RD-54 (g)); false before any kill.
    pub kill_refused: bool,
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
    aside: Vec<(AgentId, Vec<(RunId, Reaped)>)>,
    defer_held: Vec<AgentId>,
    dropped: Vec<(AgentId, Dropped)>,
    stuck: Vec<(AgentId, Stuck)>,
    lasts: Vec<(AgentId, u32, usize)>,
    defers: Vec<(RunId, Defers)>,
    unread: Vec<AgentId>,
    dismissed: Vec<WorkflowId>,
    queue_held: bool,
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
            aside: Vec::new(),
            defer_held: Vec::new(),
            dropped: Vec::new(),
            stuck: Vec::new(),
            lasts: Vec::new(),
            defers: Vec::new(),
            unread: Vec::new(),
            dismissed: Vec::new(),
            queue_held: false,
        }
    }

    /// Holds the queue or frees it: while held, the end of a run starts nothing from the queue,
    /// which keeps its starts and gives them no reply, and a tick still serves it (ruling RD-133
    /// 3: a queued start waits while the mark of a tick that did not return stands).
    pub fn hold_queue(&mut self, held: bool) {
        self.queue_held = held;
    }

    /// Whether the queue is held, so that the end of a run serves none of it.
    pub fn queue_held(&self) -> bool {
        self.queue_held
    }

    /// Takes from the owner's list that a move read which workflows it shows dismissed (I7.2,
    /// ruling RD-122 C1).
    pub(crate) fn note_list(&mut self, list: &[crate::types::Workflow]) {
        self.dismissed = list
            .iter()
            .filter_map(|w| match w {
                crate::types::Workflow::Dismissed(d) => Some(d.id.clone()),
                crate::types::Workflow::Full(_) => None,
            })
            .collect();
    }

    /// Whether the owner's list, as a move last read it, shows the workflow dismissed, so that
    /// `binding` of it is class B (I7.2, ruling RD-122 C1).
    pub fn workflow_dismissed(&self, workflow: &WorkflowId) -> bool {
        self.dismissed.contains(workflow)
    }

    /// Keeps the agent of a workflow that the owner's list shows dismissed, whose history the
    /// owner refused with `binding`, as a member with no event read (I5.8, ruling XB-28 (a)).
    pub(crate) fn keep_unread(&mut self, agent: &AgentId) {
        if !self.unread.contains(agent) {
            self.unread.push(agent.clone());
        }
    }

    /// Whether the agent is one of a dismissed workflow that recovery kept with no event read,
    /// for which nothing is logged (I5.8, ruling RD-121).
    pub fn dismissed_unread(&self, agent: &AgentId) -> bool {
        self.unread.contains(agent)
    }

    /// Takes a run that this life put on the board with whether the defer of its launch was
    /// acknowledged (ruling RD-86 item 1).
    pub(crate) fn launched(&mut self, run: &RunId, launch: bool) {
        self.defers.retain(|(r, _)| r != run);
        self.defers.push((
            run.clone(),
            Defers {
                launch,
                ticks_failed: 0,
            },
        ));
    }

    /// Counts a tick at which no defer was acknowledged for each run that stands on the board
    /// (ruling RD-86 item 1).
    pub(crate) fn defer_failed(&mut self) {
        for (run, d) in &mut self.defers {
            if self.runs.iter().any(|r| &r.run == run) {
                d.ticks_failed = d.ticks_failed.saturating_add(1);
            }
        }
    }

    /// Counts a tick that a settle or a stop waiting for the clock held, and that returned, for
    /// each run on the board that no stop has reached: a run stopped while it waits counts the
    /// held ticks only up to its stop (ruling RD-133 I7.1, Addition 1 item 7).
    pub(crate) fn held_tick(&mut self) {
        for (run, d) in &mut self.defers {
            if self.runs.iter().any(|r| &r.run == run && r.stop.is_none()) {
                d.ticks_failed = d.ticks_failed.saturating_add(1);
            }
        }
    }

    /// Forgets the defers of every run that stands on the board after a tick that did not
    /// return, whose defer may or may not have been acknowledged, so that each such run's
    /// `run_ended` writes both keys null, which says not known, and no lower bound stands as a
    /// count (I7.1; rulings RD-80 4.1, RD-86 items 1 and 2, RD-122).
    pub(crate) fn defers_not_known(&mut self) {
        let runs = &self.runs;
        self.defers
            .retain(|(run, _)| !runs.iter().any(|r| &r.run == run));
    }

    /// What this life saw of the run's defers, or none where it did not see the run whole.
    pub(crate) fn defers(&self, run: &RunId) -> Option<Defers> {
        self.defers.iter().find(|(r, _)| r == run).map(|(_, d)| *d)
    }

    /// Forgets the run's defers once its `run_ended` needs them no more.
    pub(crate) fn forget_defers(&mut self, run: &RunId) {
        self.defers.retain(|(r, _)| r != run);
    }

    /// Whether recovery set the agent aside: the owner refused to read its history, so it is a
    /// member with no event read, which no tick raises or starts anything for (I5.8, ruling
    /// RD-64 1.4).
    pub fn aside(&self, agent: &AgentId) -> bool {
        self.aside.iter().any(|(a, _)| a == agent)
    }

    /// Sets the agent aside with this life's answers of `Runs::reap`, by which its own recovery
    /// is made once its history can be read (I5.8, ruling RD-64 1.4).
    pub fn set_aside(&mut self, agent: &AgentId, reaped: Vec<(RunId, Reaped)>) {
        self.aside.retain(|(a, _)| a != agent);
        self.aside.push((agent.clone(), reaped));
    }

    /// The agents that are set aside, each with the answers of `Runs::reap` of this life.
    pub(super) fn aside_agents(&self) -> Vec<(AgentId, Vec<(RunId, Reaped)>)> {
        self.aside.clone()
    }

    pub(super) fn take_aside(&mut self, agent: &AgentId) {
        self.aside.retain(|(a, _)| a != agent);
    }

    /// Whether the agent's start sends its request to defer only after its `run_started` is
    /// acknowledged: a `run_started` of it was refused with a name other than `seq`, and no store
    /// of it has been acknowledged since (I5.5, ruling RD-64 1.8).
    pub fn defer_held(&self, agent: &AgentId) -> bool {
        self.defer_held.contains(agent)
    }

    pub(super) fn hold_defer(&mut self, agent: &AgentId) {
        if !self.defer_held.contains(agent) {
            self.defer_held.push(agent.clone());
        }
    }

    /// The stored last of the agent of member `i` (I7.2): the reply's `last` of the newest read
    /// of the owner's events, or the `seq` of the newest acknowledged record, whichever came
    /// last, also where it is below a `seq` that the agent's events hold (ruling RD-64 3.2). Where
    /// no read or store set it, or events joined the facts since by another way, the `seq` of
    /// the agent's last event.
    pub fn stored_last(&self, i: usize) -> u32 {
        let facts = &self.members[i].facts;
        let newest = facts.events.last().map_or(0, |e| e.seq.get());
        match self.lasts.iter().find(|(a, _, _)| *a == facts.agent) {
            Some((_, last, seen)) if *seen == facts.events.len() => *last,
            _ => newest,
        }
    }

    /// Sets the stored last of the agent of member `i` to `last`, from a reply's `last` or an
    /// acknowledged store (ruling RD-64 3.2).
    pub fn set_last(&mut self, i: usize, last: u32) {
        let facts = &self.members[i].facts;
        let (agent, seen) = (facts.agent.clone(), facts.events.len());
        self.lasts.retain(|(a, _, _)| *a != agent);
        self.lasts.push((agent, last, seen));
    }

    /// A store of the agent was acknowledged.
    pub fn acknowledged(&mut self, agent: &AgentId) {
        self.defer_held.retain(|a| a != agent);
        let s = self.stuck_of(agent);
        s.acked = true;
        s.refused = false;
        s.for_life = false;
        s.gone = false;
    }

    fn stuck_of(&mut self, agent: &AgentId) -> &mut Stuck {
        let k = match self.stuck.iter().position(|(a, _)| a == agent) {
            Some(k) => k,
            None => {
                self.stuck.push((agent.clone(), Stuck::default()));
                self.stuck.len() - 1
            }
        };
        &mut self.stuck[k].1
    }

    /// A store or a read of the agent was refused with a name of class C or D, or with `seq` a
    /// second time for one log after its read, in this tick (ruling RD-64 2.5).
    pub fn refused(&mut self, agent: &AgentId) {
        self.stuck_of(agent).refused = true;
    }

    /// A refusal of a store of the agent that cannot pass in this life (`lasts_the_life`): it
    /// counts as `refused` does, and a stuck record then stops the tick's start for the life
    /// (ruling RD-123).
    pub fn refused_for_life(&mut self, agent: &AgentId) {
        let s = self.stuck_of(agent);
        s.refused = true;
        s.for_life = true;
    }

    /// A start of the agent was refused with a name of class B: it counts nothing toward the
    /// stuck line and shows no line, and no tick tries the start again in this life (I7.2 class
    /// B, ruling RD-125 (1)).
    pub fn start_gone(&mut self, agent: &AgentId) {
        self.stuck_of(agent).gone = true;
    }

    /// Takes a refusal of a write of the agent by its class, the same for every sender (I7.2,
    /// rulings RD-64 2.5, RD-123, RD-125 (1), RD-126 item 9): class B counts nothing toward the
    /// stuck line and marks nothing for the life, but for a body that holds the start's
    /// `run_started` (`start`), which stops the tick's start for the life (`start_gone`); a
    /// refusal that cannot pass in this life (`lasts_the_life`) counts and marks the agent for the
    /// life (`refused_for_life`); class C and every other name of class D count (`refused`);
    /// `seq` and a failure that is no refusal by name count nothing here. `dismissed` says
    /// whether the owner's list, as the sender last read it, shows the agent's workflow
    /// dismissed.
    pub(crate) fn note_refusal(
        &mut self,
        agent: &AgentId,
        error: &crate::ports::PortError,
        dismissed: bool,
        start: bool,
    ) {
        use crate::items::drive::{class_of, lasts_the_life, Class};
        match class_of(error, dismissed) {
            Some(Class::Gone) if start => self.start_gone(agent),
            Some(Class::Gone | Class::Seq) | None => {}
            Some(Class::One | Class::Whole) if lasts_the_life(error, dismissed) => {
                self.refused_for_life(agent);
            }
            Some(Class::One | Class::Whole) => self.refused(agent),
        }
    }

    /// Whether a record or a body of the agent's items' list keeps its place under class D or as
    /// an early record, in which case no tick starts the agent or raises an item for it (I5.7,
    /// ruling RD-64 2.5).
    pub fn hold_place(&mut self, agent: &AgentId, held: bool) {
        self.stuck_of(agent).held = held;
    }

    /// Whether no tick starts the agent or raises an item for it: it is set aside, or a record of
    /// its items' list keeps its place (I5.7, I5.8; ruling RD-64 1.4, 2.5).
    pub fn place_held(&self, agent: &AgentId) -> bool {
        self.aside(agent) || self.stuck.iter().any(|(a, s)| a == agent && s.held)
    }

    /// Ends the count of one tick (ruling RD-64 2.5): an agent for which such a refusal came at
    /// the tick counts one more tick in a row, one of which a store was acknowledged and none was
    /// refused ends its count, and an agent of which nothing was sent neither counts nor ends it.
    pub fn count_tick(&mut self) {
        for (_, s) in &mut self.stuck {
            if s.refused {
                s.ticks = s.ticks.saturating_add(1);
            } else if s.acked {
                s.ticks = 0;
            }
            s.refused = false;
            s.acked = false;
        }
    }

    /// Whether no tick tries the agent's start again in this life: its record is stuck by a
    /// refusal that cannot pass in this life, or a start of it was refused with a name of class B
    /// (`start_gone`); a record stuck by refusals that can pass does not stop the tick, whose
    /// start is itself the write that ends the count (ruling RD-123, RD-125, RD-64 2.5). A refusal
    /// that cannot pass in this life holds until a store of the agent is acknowledged, whatever
    /// names later refusals bear.
    pub fn stuck_for_life(&self, agent: &AgentId, limits: &crate::types::Limits) -> bool {
        self.stuck.iter().any(|(a, s)| a == agent && s.gone)
            || (self.record_stuck(agent, limits)
                && (self.aside(agent) || self.stuck.iter().any(|(a, s)| a == agent && s.for_life)))
    }

    /// Whether the agent's record is stuck, for the line `line.problem.record` (I5.7, I1.3): it is
    /// set aside, or `record_stuck_ticks` ticks in a row had such a refusal (ruling RD-64 1.4,
    /// 2.5).
    pub fn record_stuck(&self, agent: &AgentId, limits: &crate::types::Limits) -> bool {
        self.aside(agent)
            || self
                .stuck
                .iter()
                .any(|(a, s)| a == agent && s.ticks >= limits.record_stuck_ticks)
    }

    /// The number of the agent's records taken off in this life, with the event name and the
    /// refusal name of the last one, or none (I7.2, ruling RD-64 2.6).
    pub fn dropped(&self, agent: &AgentId) -> Option<(u32, &str, &str)> {
        self.dropped
            .iter()
            .find(|(a, _)| a == agent)
            .map(|(_, d)| (d.count, d.event.as_str(), d.refusal.as_str()))
    }

    /// Counts one record of the agent taken off, by its event name and the name of the refusal
    /// that took it off (ruling RD-64 2.6).
    pub fn count_dropped(&mut self, agent: &AgentId, event: &str, refusal: &str) {
        let k = match self.dropped.iter().position(|(a, _)| a == agent) {
            Some(k) => k,
            None => {
                self.dropped.push((agent.clone(), Dropped::default()));
                self.dropped.len() - 1
            }
        };
        let d = &mut self.dropped[k].1;
        d.count = d.count.saturating_add(1);
        event.clone_into(&mut d.event);
        refusal.clone_into(&mut d.refusal);
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
