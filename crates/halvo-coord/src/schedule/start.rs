use super::{clean, current_slot, finished, holds, slot_due, slot_open, Hold};
use crate::types::{
    AgentState, Cause, ConfigView, Doing, Facts, Limits, Mode, Runtime, Tier, Trigger, What,
};

/// Whether the agent waits for the person in each run: `caps_external` is `yes` (I5.1).
pub fn needs_person<C: ConfigView>(config: &C) -> bool {
    config.caps_external()
}

/// Whether a run of the agent holds the on-device model: its tier is `on_device` and its
/// `model_tokens` is above 0 (I5.1).
pub fn needs_model<C: ConfigView>(config: &C) -> bool {
    config.tier() == Tier::OnDevice && config.model_tokens() > 0
}

/// The state that ends a hold in the agent store: `active`, or `approved` for the runtime
/// `web` with no playbook, for which A-A7.2 refuses `active` (I5.4).
pub fn active_state<C: ConfigView>(config: &C) -> AgentState {
    if config.runtime() == Runtime::Web && config.playbook().is_none() {
        AgentState::Approved
    } else {
        AgentState::Active
    }
}

/// What `start_step` reads beside the agent's facts: the owner's list for step 0 and the
/// coordinator's other runs and starts for step 4, which `Facts` does not hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Around {
    /// Whether the agent's workflow is in the owner's list that `Owner::workflows` gave for
    /// this start.
    pub listed: bool,
    /// The runs that are live, a `stopping` one among them.
    pub live: u32,
    /// Whether a live run holds the on-device model.
    pub model_held: bool,
    /// The starts that wait in the queue.
    pub queued: u32,
    /// Whether the agent has a stored session.
    pub session: bool,
    /// Whether this is the agent's own start leaving the queue: its queued start does not fail
    /// step 3, a failed step 4 keeps it at the head of the queue, and a start of the person
    /// ends no hold.
    pub from_queue: bool,
    /// Whether a `run_ended` of the agent that the owner did not acknowledge is owed (ruling
    /// XB-17): the agent takes no start while it is.
    pub owed: bool,
}

/// The end of the hold `failures` by a start of the person (I5.4): its `hold_cleared` event,
/// and the state to send when the hold `off` does not stand, else none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clear {
    /// The `set_state` to send, or none while the hold `off` stands.
    pub set_state: Option<AgentState>,
}

/// What steps 5 to 8 of I5.5 need for a start that passed step 4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Go {
    /// The `slot` of the `run_started` event (I5.3).
    pub slot: Option<u32>,
    /// `compile` for the runtime `web` with no playbook, `replay` otherwise.
    pub mode: Mode,
    /// Whether the runner's browser is headed: the mode is `compile` or no session is stored.
    pub headed: bool,
    /// Whether the run holds the on-device model.
    pub holds_model: bool,
    /// Milliseconds from now until which the analysis batch is deferred: the agent's `wall_s`
    /// and `wait_limit_s`.
    pub defer_ms: u64,
}

/// The answer of steps 0 to 4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Refused with `state`, `hold`, `budget` or `queue_full`.
    Refused(Cause),
    /// Joins the queue, or, for a start leaving the queue, stays at its head.
    Queued,
    /// Goes on with steps 5 to 8.
    Go(Go),
}

/// What `start_step` decides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartStep {
    /// The end of the hold `failures`, made after step 0 passed and before step 1.
    pub clear: Option<Clear>,
    /// The answer.
    pub verdict: Verdict,
}

/// Steps 0 to 4 of a start (I5.5), the first that fails giving the answer: (0) the workflow is
/// in the owner's list, else `state`; a start of the person that does not leave the queue then
/// ends the hold `failures`; (1) no hold stands, else `hold`; (2) fewer than `starts_per_day`
/// `run_started` events of the agent have the facts' day, else `budget`; (3) no run of the
/// agent is live or queued, the agent store has ended its newest run and no `run_ended` of the
/// agent is owed, else `state`; for a
/// start of the scheduler the slot rule of I5.3 then holds, for an agent that needs no person
/// and has a finished run, else `state`; (4) fewer than `max_live_runs` runs are live and, for
/// an agent that needs the model, no live run holds it, else `queued`, or `queue_full` when
/// `queue_max` starts wait.
pub fn start_step<C: ConfigView>(
    facts: &Facts<C>,
    trigger: Trigger,
    limits: &Limits,
    around: &Around,
) -> StartStep {
    let refused = |clear, cause| StartStep {
        clear,
        verdict: Verdict::Refused(cause),
    };
    if !around.listed {
        return refused(None, Cause::State);
    }
    let mut standing = holds(facts);
    let clear =
        (trigger == Trigger::Person && !around.from_queue && standing.contains(&Hold::Failures))
            .then(|| Clear {
                set_state: (!standing.contains(&Hold::Off)).then(|| active_state(&facts.config)),
            });
    if clear.is_some() {
        standing.retain(|h| *h != Hold::Failures);
    }
    if !standing.is_empty() {
        return refused(clear, Cause::Hold);
    }
    let today = facts
        .events
        .iter()
        .filter(|e| e.day == facts.today && matches!(e.what, What::RunStarted { .. }))
        .count();
    if today >= limits.starts_per_day as usize {
        return refused(clear, Cause::Budget);
    }
    let mine = match facts.doing {
        Doing::Live(_) => true,
        Doing::Queued(_) => !around.from_queue,
        Doing::Idle(_) => false,
    };
    if mine || !facts.store_ended || around.owed {
        return refused(clear, Cause::State);
    }
    let slot = match trigger {
        Trigger::Schedule => {
            let due =
                slot_due(facts, limits).filter(|_| !needs_person(&facts.config) && finished(facts));
            match due {
                Some(slot) => Some(slot),
                None => return refused(clear, Cause::State),
            }
        }
        Trigger::Person => current_slot(&facts.workflow, facts.today)
            .filter(|s| clean(facts) && slot_open(facts, *s)),
    };
    let holds_model = needs_model(&facts.config);
    if around.live >= limits.max_live_runs || (holds_model && around.model_held) {
        if !around.from_queue && around.queued >= limits.queue_max {
            return refused(clear, Cause::QueueFull);
        }
        return StartStep {
            clear,
            verdict: Verdict::Queued,
        };
    }
    let mode = if facts.config.runtime() == Runtime::Web && facts.config.playbook().is_none() {
        Mode::Compile
    } else {
        Mode::Replay
    };
    StartStep {
        clear,
        verdict: Verdict::Go(Go {
            slot,
            mode,
            headed: mode == Mode::Compile || !around.session,
            holds_model,
            defer_ms: facts
                .config
                .wall_s()
                .saturating_add(limits.wait_limit_s)
                .saturating_mul(1000),
        }),
    }
}
