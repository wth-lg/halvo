use super::{
    clean, current_slot, finished, held_by_events, holds, needs_person, slot_due, slot_open, Board,
    Hold,
};
use crate::types::{
    AgentId, AgentState, ConfigView, Doing, Facts, LastCause, Limits, Outcome, RunEnd, RunId,
    Runtime, SpendLimit, Tier, What, WorkflowId, WorkflowState,
};

/// Whether the agent of `facts` rests after a run that handed its work back for a cause of a
/// hosted model (r3 U7.8): the agent store has ended its newest run, the newest `run_ended` of its
/// events is `handed_back`, its stored tier is a hosted tier, and the last cause still stands by
/// what the app handed beside its configuration at this list of the agents. `no_agreement` stands
/// until the agent's yes stands for its tier, `no_model_key` until a key is saved, `over_cap` while
/// the limit reads near or reached, and `shortfall` until a newer run ends, since the core does not
/// read the router's table. Where the app hands no use of a model, the agent rests; so it does for
/// `no_model_key` where the key's state does not read, and for `over_cap` where the limit does not
/// read. Nothing is stored, and the rest is no hold: the person's own start passes, and Start now
/// stays offered. While it rests, a tick raises no `ready` for the agent and starts no run of it
/// (`tick_step`), and its row says the sentence of the cause where no line of
/// `crate::roster::status_of` stands before it.
pub(crate) fn rests<C: ConfigView>(facts: &Facts<C>) -> bool {
    if !facts.store_ended || facts.config.tier() == Tier::OnDevice {
        return false;
    }
    let last = facts.events.iter().rev().find_map(|e| match e.what {
        What::RunEnded { cause, .. } => Some(cause),
        _ => None,
    });
    if last != Some(RunEnd::HandedBack) {
        return false;
    }
    let model = facts.config.model();
    match facts.last_cause {
        Some(LastCause::NoAgreement) => !model.is_some_and(|m| m.stands),
        Some(LastCause::NoModelKey) => model.and_then(|m| m.key) != Some(true),
        Some(LastCause::OverCap) => model.is_none_or(|m| match m.limit {
            SpendLimit::Stands { near, .. } => near,
            SpendLimit::NotSet => false,
            SpendLimit::Unread => true,
        }),
        Some(LastCause::Shortfall) => true,
        _ => false,
    }
}

/// Whether a tick sends `set_state` `active` for the agent (ruling IG-6a): the owner's list
/// shows its workflow `approved`, no hold stands on it (so its runtime is not `none`, which is
/// the hold `not_yet`), and for the runtime `web` its last run record has the outcome
/// `completed`, which is its first run in the mode `compile`. The one rule for every runtime; the
/// creation and the ends of runs send no `active`.
pub fn wants_active<C: ConfigView>(facts: &Facts<C>) -> bool {
    facts.workflow.state == WorkflowState::Approved && gives_active(facts)
}

/// The part of the rule for `active` that does not read the workflow's state: no hold stands,
/// and for the runtime `web` the last run record has the outcome `completed`. The control
/// `switch_on` sends `active` where it holds and `approved` otherwise (ruling IG-6a, IG-7 to
/// IG-12).
pub fn gives_active<C: ConfigView>(facts: &Facts<C>) -> bool {
    holds(facts).is_empty() && web_ready(facts)
}

fn web_ready<C: ConfigView>(facts: &Facts<C>) -> bool {
    facts.config.runtime() != Runtime::Web
        || facts
            .runs
            .first()
            .is_some_and(|r| r.outcome == Outcome::Completed)
}

/// The state that the control `switch_on` sends, before it stores anything (ruling RD-67 1.3
/// (a)): `active` where the rule of `active` gives it once the holds `off`, `sign_in_refused` and
/// `failures` that the switch-on ends are gone, the derived `off` among them, and `approved`
/// otherwise (ruling RD-73 4.3). The hold `site_declined`, which the switch-on does not end,
/// changes no state that it sends (r4 U2.4a).
pub fn switch_on_state<C: ConfigView>(facts: &Facts<C>) -> AgentState {
    let others = holds(facts).iter().all(|h| {
        matches!(
            h,
            Hold::Off | Hold::SignInRefused | Hold::Failures | Hold::SiteDeclined
        )
    });
    if others && web_ready(facts) {
        AgentState::Active
    } else {
        AgentState::Approved
    }
}

/// Whether a tick sends `set_state` `paused` for the agent by the rule over the facts of ruling
/// RD-67 1.3 (b), in every life: the hold `off` or `failures` stands by the stored events while
/// the owner's list holds the workflow in the full shape with another state than `paused`.
pub fn wants_paused<C>(facts: &Facts<C>) -> bool {
    !facts.stand_in && facts.workflow.state != WorkflowState::Paused && held_by_events(facts)
}

/// What one tick raises or starts for an agent (I5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Raise a `ready` item.
    Ready,
    /// Start a run by the scheduler for the due slot.
    Start,
    /// Nothing.
    Nothing,
}

/// What one tick raises or starts for the agent of `facts` (I5.3, I5.9): nothing while its
/// workflow is not `listed`, a hold stands, it rests after a run that handed its work back for
/// a cause of a hosted model that still stands (`rests`, r3 U7.8), a run of it is live, a start
/// of it is queued or an item of it is open; for an agent that needs the person, and for every
/// agent with no finished run, `ready` when it has no clean run (`clean`) or its current slot is
/// open, unless a `slot_skipped` event with a null slot stands: until a slot opens after that
/// event's day, and for an agent with no slot while the event is handed out (rulings RD-67 5.2,
/// RD-73 1.2); for any other agent a start when its slot is due (`slot_due`).
/// The tick of `drive` reads it on the facts with the ends that the row reads (`Ends::read`),
/// so that a run that a record names `completed` or `nothing_to_do` counts so here whatever
/// `interrupted` row it has (RD-156 (1)).
pub fn tick_step<C: ConfigView>(facts: &Facts<C>, listed: bool, limits: &Limits) -> Ask {
    if !listed || !holds(facts).is_empty() || rests(facts) || facts.doing != Doing::Idle(None) {
        return Ask::Nothing;
    }
    let first = !finished(facts);
    if needs_person(&facts.config) || first {
        let current = current_slot(&facts.workflow, facts.today);
        let open = current.is_some_and(|s| slot_open(facts, s));
        let skipped = facts.events.iter().any(|e| {
            matches!(e.what, What::SlotSkipped { slot: None }) && current.is_none_or(|s| s <= e.day)
        });
        if (!clean(facts) || open) && !skipped {
            Ask::Ready
        } else {
            Ask::Nothing
        }
    } else if slot_due(facts, limits).is_some() {
        Ask::Start
    } else {
        Ask::Nothing
    }
}

/// The stages of one tick before it serves the queue and visits the agents (I5.9, I5.7), read
/// from the board before the tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The milliseconds of the request to defer that the tick sends again, the largest of the
    /// live runs', when a run is live before the tick.
    pub defer_ms: Option<u64>,
    /// The runs of the workflows that are not listed and that no stop has reached, in the order
    /// of their starts: each gets `Runs::cancel` and stops with the cause `deleted`.
    pub stop: Vec<RunId>,
    /// The `stopping` runs of the workflows that are not listed: their cause becomes `deleted`.
    pub recause: Vec<RunId>,
    /// The agents of the workflows that are not listed whose queued start or open item goes,
    /// with nothing stored.
    pub dropped: Vec<AgentId>,
    /// The workflows that are not listed and that no earlier tick saw gone.
    pub seen: Vec<WorkflowId>,
    /// In the order of their starts, the runs whose stop came before the tick and at least
    /// `cancel_grace_s` before `now`, or whose stop the clock reads as later than `now` because
    /// it stepped back, live or ended in between: each gets `Runs::kill` (ruling RD-7). A stop
    /// at the instant `now` kills nothing.
    pub kill: Vec<RunId>,
    /// Whether the tick serves the queue: no run is live before it, so none is live after the
    /// kills and no kill ends a run.
    pub serve: bool,
}

/// The plan of the stages of one tick that come before its queue and its agents (I5.9): the
/// request to defer while a run is live, the workflows that are gone from `listed` (I5.7), and
/// the kills at `now` by the clock of `Clock::now`.
pub fn tick_plan<C>(board: &Board<C>, listed: &[WorkflowId], now: u64, limits: &Limits) -> Plan {
    let grace = limits.cancel_grace_s.saturating_mul(1000);
    let passed = |at: u64| now < at || now - at >= grace;
    let mut kill: Vec<(u64, RunId)> = board
        .runs
        .iter()
        .filter(|r| r.stop.is_some_and(|s| passed(s.at)))
        .map(|r| (r.rank, r.run.clone()))
        .chain(
            board
                .dying
                .iter()
                .filter(|d| passed(d.stopped))
                .map(|d| (d.rank, d.run.clone())),
        )
        .collect();
    kill.sort();
    let unlisted = |w: &WorkflowId| !listed.contains(w);
    let mut seen: Vec<WorkflowId> = Vec::new();
    for w in board
        .members
        .iter()
        .map(|m| &m.facts.workflow.id)
        .chain(board.runs.iter().map(|r| &r.workflow))
    {
        if unlisted(w) && !board.gone.contains(w) && !seen.contains(w) {
            seen.push(w.clone());
        }
    }
    Plan {
        defer_ms: board.runs.iter().map(|r| r.defer_ms).max(),
        stop: board
            .runs
            .iter()
            .filter(|r| unlisted(&r.workflow) && r.stop.is_none())
            .map(|r| r.run.clone())
            .collect(),
        recause: board
            .runs
            .iter()
            .filter(|r| unlisted(&r.workflow) && r.stop.is_some_and(|s| s.cause != RunEnd::Deleted))
            .map(|r| r.run.clone())
            .collect(),
        dropped: board
            .members
            .iter()
            .filter(|m| unlisted(&m.facts.workflow.id))
            .filter(|m| {
                board.queue.holds(&m.facts.agent)
                    || matches!(m.facts.doing, Doing::Idle(Some(_)) | Doing::Queued(Some(_)))
            })
            .map(|m| m.facts.agent.clone())
            .collect(),
        seen,
        kill: kill.into_iter().map(|(_, r)| r).collect(),
        serve: board.runs.is_empty(),
    }
}
