//! The guard on the link (I4.7): `guard_step` decides whether an effect record of a runner
//! may pass to the owner, from the guard's table of acknowledged `item_answered` rows and the
//! answer of the port `Policy`; `check` asks the port and calls it.

use crate::items::presenter;
use crate::ports::PortError;
use crate::types::{
    AgentId, ApprovalId, Cause, Class, Decision, DecisionEntry, Digest, EffectAnswer, EffectId,
    ItemId, Presenter, RunId,
};

/// The states of an effect record, the eight of the effect machine of A-A5.2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectState {
    /// `refused`.
    Refused,
    /// `cleared`.
    Cleared,
    /// `held`.
    Held,
    /// `approved`.
    Approved,
    /// `rejected`.
    Rejected,
    /// `releasing`.
    Releasing,
    /// `committed`.
    Committed,
    /// `unknown`.
    Unknown,
}

impl EffectState {
    /// Every state, in the order of A-A5.2.
    pub const ALL: &'static [EffectState] = &[
        EffectState::Refused,
        EffectState::Cleared,
        EffectState::Held,
        EffectState::Approved,
        EffectState::Rejected,
        EffectState::Releasing,
        EffectState::Committed,
        EffectState::Unknown,
    ];

    /// The state's name.
    pub const fn name(self) -> &'static str {
        match self {
            EffectState::Refused => "refused",
            EffectState::Cleared => "cleared",
            EffectState::Held => "held",
            EffectState::Approved => "approved",
            EffectState::Rejected => "rejected",
            EffectState::Releasing => "releasing",
            EffectState::Committed => "committed",
            EffectState::Unknown => "unknown",
        }
    }

    /// Reads a state's name; any other string is `malformed`.
    pub fn parse(text: &str) -> Result<EffectState, Cause> {
        EffectState::ALL
            .iter()
            .copied()
            .find(|s| s.name() == text)
            .ok_or(Cause::Malformed)
    }
}

impl std::fmt::Display for EffectState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What the guard reads of a body with the `op` `effect`: its record `agent_effect` (A-A7.1,
/// A-A7.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectRecord {
    /// `run`.
    pub run: RunId,
    /// `agent`.
    pub agent: AgentId,
    /// `effect`.
    pub effect: EffectId,
    /// `state`.
    pub state: EffectState,
    /// `class`.
    pub class: Class,
    /// `digest`.
    pub digest: Digest,
    /// `approval`, or none for null.
    pub approval: Option<ApprovalId>,
}

/// The port `Policy` (I5.10, ruling XA-1a): whether the agent-setup run's table gives `run`
/// for an agent's configuration and a class, for `local_edit` with either checkpoint. The app
/// implements it with `decide_row` of halvo-policy; the memory `Policy` answers from the rows
/// that a test gives it.
pub trait Policy {
    /// The configuration that the coordinator holds for an agent.
    type Config;
    /// Whether the table gives `run` for `config` and `class`.
    fn gives_run(&self, config: &Self::Config, class: Class) -> Result<bool, PortError>;
}

/// What the port `Policy` answered for the class of a record and the configuration that the
/// coordinator holds for its agent: `run`, `confirm` (also for a port that failed), or no
/// configuration held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gives {
    /// The table gives `run`.
    Run,
    /// The table gives anything else, or the port failed.
    Confirm,
    /// The coordinator holds no configuration for the agent.
    NoConfig,
}

/// A refusal of the guard: the reply error `approval`, with which the record is answered and
/// not passed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `approval`.
    Approval,
}

impl Refusal {
    /// The reply error's name.
    pub const fn name(self) -> &'static str {
        match self {
            Refusal::Approval => "approval",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    item: ItemId,
    run: RunId,
    entry: DecisionEntry,
    release: Option<u32>,
    used: bool,
}

/// The guard's table (I4.7): the `item_answered` rows whose acknowledgement came before the
/// release of their decision, each with the number of its decision's release, counted from 1 in
/// this life (the rows stand in the order of acknowledgement, and an item id holds no order), and
/// marked when a record used it, the runs that were stopped, and for each run and effect id the
/// fact that the effect has begun (ruling IG-32). A row counts for a record only from the release
/// on; the rows of an item that lapsed leave the table, whatever the cause. A new life of the app
/// starts with an empty table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    rows: Vec<Row>,
    stopped: Vec<RunId>,
    releases: u32,
    begun: Vec<(RunId, EffectId)>,
}

impl Table {
    /// An empty table.
    pub fn new() -> Table {
        Table::default()
    }

    /// The table with the rows of `decision`, acknowledged for `item` of `run` at the move
    /// `record`; they count for no record before `release`.
    pub fn record(&self, item: &ItemId, run: &RunId, decision: &Decision) -> Table {
        let mut t = self.clone();
        t.rows.extend(decision.approvals().iter().map(|entry| Row {
            item: item.clone(),
            run: run.clone(),
            entry: entry.clone(),
            release: None,
            used: false,
        }));
        t
    }

    /// The table in which the rows of `item` count from now on: its decision was released, as the
    /// next release of this life, whose number its rows take.
    pub fn release(&self, item: &ItemId) -> Table {
        let mut t = self.clone();
        let number = t.releases.saturating_add(1);
        let mut any = false;
        for row in t
            .rows
            .iter_mut()
            .filter(|r| &r.item == item && r.release.is_none())
        {
            row.release = Some(number);
            any = true;
        }
        if any {
            t.releases = number;
        }
        t
    }

    /// The table without the rows of `item`, which lapsed.
    pub fn lapse(&self, item: &ItemId) -> Table {
        let mut t = self.clone();
        t.rows.retain(|r| &r.item != item);
        t
    }

    /// The table in which `run` was stopped (I5.7).
    pub fn stop(&self, run: &RunId) -> Table {
        let mut t = self.clone();
        if !t.stopped.contains(run) {
            t.stopped.push(run.clone());
        }
        t
    }

    /// Whether `run` was stopped.
    pub fn is_stopped(&self, run: &RunId) -> bool {
        self.stopped.contains(run)
    }

    /// The answer of the newest decision that was released for `effect` of `run` in this life,
    /// or none: the newest is the one with the highest number of release, and whether a record
    /// used its row plays no part. A row whose decision was not released gives nothing, the rows
    /// of an item that lapsed are no longer in the table, so the decision released before them
    /// decides, and a new life gives none (I4.3, I4.7, rulings IG-25, IG-32).
    pub fn newest_released(&self, run: &RunId, effect: &EffectId) -> Option<EffectAnswer> {
        self.rows
            .iter()
            .filter(|r| r.run == *run && r.entry.effect == *effect)
            .filter_map(|r| r.release.map(|number| (number, r.entry.answer)))
            .max_by_key(|(number, _)| *number)
            .map(|(_, answer)| answer)
    }

    /// Whether the newest decision that was released for `effect` of `run` in this life is
    /// `allow_once` (`newest_released`): the fact on which a later item of the run that names the
    /// effect shows the line `confirm.changed` (I4.3, rulings IG-6d, IG-25).
    pub fn newest_allows(&self, run: &RunId, effect: &EffectId) -> bool {
        self.newest_released(run, effect) == Some(EffectAnswer::AllowOnce)
    }

    /// The table after the owner acknowledged the store of `record`: a record `releasing`,
    /// `committed` or `unknown` sets the fact that its effect has begun, for its run and effect
    /// id, and any other record changes nothing. The caller calls it at the acknowledgement and
    /// not where the guard passes the record, because a record whose store failed began nothing
    /// (I4.7, ruling IG-32). Nothing in this crate calls it: the link of U5.4 calls it at each
    /// acknowledgement of an effect record, and until then no effect has begun in a running app,
    /// while `items::raise_confirm`, which U2.4's coordinator calls for a confirm request, reads
    /// the fact (ruling RD-23 (d)).
    pub fn stored(&self, record: &EffectRecord) -> Table {
        let mut t = self.clone();
        let begins = matches!(
            record.state,
            EffectState::Releasing | EffectState::Committed | EffectState::Unknown
        );
        let key = (record.run.clone(), record.effect.clone());
        if begins && !t.begun.contains(&key) {
            t.begun.push(key);
        }
        t
    }

    /// Whether the owner acknowledged a record that began `effect` of `run` in this life.
    pub fn has_begun(&self, run: &RunId, effect: &EffectId) -> bool {
        self.begun.iter().any(|(r, e)| r == run && e == effect)
    }

    /// Whether a confirm request of `run` that names `effect` shows no card and ends the run with
    /// `kind`: the effect has begun, or its newest released decision is `reject_once` (I3.1,
    /// I4.7, ruling IG-32).
    pub fn asks_too_late(&self, run: &RunId, effect: &EffectId) -> bool {
        self.has_begun(run, effect)
            || self.newest_released(run, effect) == Some(EffectAnswer::RejectOnce)
    }

    /// The approval ids of the rows that the table holds, with whether each counts and whether
    /// a record used it, in the order of their record.
    pub fn approvals(&self) -> Vec<(ApprovalId, bool, bool)> {
        self.rows
            .iter()
            .map(|r| (r.entry.approval.clone(), r.release.is_some(), r.used))
            .collect()
    }

    fn matching(&self, record: &EffectRecord, approval: &ApprovalId) -> Option<usize> {
        self.rows.iter().position(|r| {
            r.release.is_some()
                && r.run == record.run
                && r.entry.answer == EffectAnswer::AllowOnce
                && r.entry.approval == *approval
                && r.entry.effect == record.effect
                && r.entry.digest == record.digest
        })
    }
}

/// Whether a class reaches `releasing` only from `approved`: `purchase`, `payment` and `sign`,
/// the classes that the presenter table names for the native dialog (A-A5.3 P1, I3.1, I4.1).
const fn approved_only(class: Class) -> bool {
    matches!(class, Class::Purchase | Class::Payment | Class::Sign)
}

/// The guard's rule for one effect record (I4.7, L7 to L10): the table after it, or the
/// refusal `approval`. A record with neither the state `approved` nor `releasing` passes. One
/// of those two states is refused for a run that was stopped, and for an approval id that a row
/// of a `reject_once` decision holds. A record `approved` passes only on a counted
/// `allow_once` row of its approval id with its run, its effect and its digest, and marks
/// the row used; a repeat of the record that used the row passes again and marks nothing. A
/// record `releasing` with an approval passes only on such a row, and of a class of the
/// presenter `native` only on such a row that a record `approved` used. A record `releasing`
/// with no approval passes only when `gives` is `run`, and never for `purchase`, `payment` or
/// `sign`, whatever `gives` is.
pub fn guard_step(table: &Table, record: &EffectRecord, gives: Gives) -> Result<Table, Refusal> {
    if !matches!(record.state, EffectState::Approved | EffectState::Releasing) {
        return Ok(table.clone());
    }
    if table.is_stopped(&record.run) {
        return Err(Refusal::Approval);
    }
    let rejected = record.approval.as_ref().is_some_and(|approval| {
        table
            .rows
            .iter()
            .any(|r| r.entry.approval == *approval && r.entry.answer == EffectAnswer::RejectOnce)
    });
    if rejected {
        return Err(Refusal::Approval);
    }
    let row = record
        .approval
        .as_ref()
        .and_then(|approval| table.matching(record, approval));
    let native = presenter(record.class) == Presenter::Native;
    match (record.state, &record.approval, row) {
        (EffectState::Approved, _, Some(i)) => {
            let mut t = table.clone();
            t.rows[i].used = true;
            Ok(t)
        }
        (EffectState::Releasing, Some(_), Some(i)) if !native || table.rows[i].used => {
            Ok(table.clone())
        }
        (EffectState::Releasing, None, _)
            if !approved_only(record.class) && gives == Gives::Run =>
        {
            Ok(table.clone())
        }
        _ => Err(Refusal::Approval),
    }
}

/// The guard for one effect record: asks `policy` for the record's class and the configuration
/// that the coordinator holds for its agent, none when it holds none, and applies `guard_step`.
/// A port that fails counts as `confirm`.
pub fn check<P: Policy>(
    policy: &P,
    config: Option<&P::Config>,
    table: &Table,
    record: &EffectRecord,
) -> Result<Table, Refusal> {
    let gives = match config {
        None => Gives::NoConfig,
        Some(config) => match policy.gives_run(config, record.class) {
            Ok(true) => Gives::Run,
            Ok(false) | Err(_) => Gives::Confirm,
        },
    };
    guard_step(table, record, gives)
}
