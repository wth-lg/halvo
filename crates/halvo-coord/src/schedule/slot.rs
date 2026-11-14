use crate::types::{Facts, FullWorkflow, Limits, RunEnd, RunId, What, WorkflowTrigger};

fn period(workflow: &FullWorkflow) -> Option<u64> {
    match workflow.trigger {
        WorkflowTrigger::Schedule => workflow.period.map(|p| u64::from(p.get())),
        WorkflowTrigger::AppContext | WorkflowTrigger::Manual => None,
    }
}

/// The current slot of the workflow on `today`: the latest day `last + k * period`, `k` of 1
/// and more, that is not after today; none before its first slot and for a workflow whose
/// trigger is not `schedule` or that has no period (I5.2).
pub fn current_slot(workflow: &FullWorkflow, today: u32) -> Option<u32> {
    let p = period(workflow)?;
    let (last, today) = (u64::from(workflow.last), u64::from(today));
    if today < last + p {
        return None;
    }
    u32::try_from(last + (today - last) / p * p).ok()
}

/// The next slot of the workflow: the first slot after `today`, or none for a workflow with no
/// slot (I5.2, the `next` of I2.5).
pub fn next_slot(workflow: &FullWorkflow, today: u32) -> Option<u32> {
    let p = period(workflow)?;
    let (last, today) = (u64::from(workflow.last), u64::from(today));
    let next = if today < last + p {
        last + p
    } else {
        last + ((today - last) / p + 1) * p
    };
    u32::try_from(next).ok()
}

fn start_day<C>(facts: &Facts<C>, run: &RunId) -> Option<u32> {
    facts.events.iter().find_map(|e| match &e.what {
        What::RunStarted { run: r, .. } if r == run => Some(e.day),
        _ => None,
    })
}

/// Whether the slot of the day `slot` is open for the agent (I5.2): no run of it that ended
/// `completed` or `nothing_to_do` has a day on or after the slot's day less half the period
/// rounded down, where the day of a run is the day of its `run_started` event, and no
/// `slot_skipped` event names the slot. A workflow with no schedule has no open slot.
pub fn slot_open<C>(facts: &Facts<C>, slot: u32) -> bool {
    let Some(p) = period(&facts.workflow) else {
        return false;
    };
    let from = u64::from(slot).saturating_sub(p / 2);
    let served = facts.events.iter().any(|e| match &e.what {
        What::RunEnded {
            run,
            cause: RunEnd::Completed | RunEnd::NothingToDo,
        } => start_day(facts, run).is_some_and(|d| u64::from(d) >= from),
        _ => false,
    });
    let skipped = facts
        .events
        .iter()
        .any(|e| matches!(e.what, What::SlotSkipped { slot: Some(s) } if s == slot));
    !served && !skipped
}

/// The slot for which the scheduler may start a run on the facts' day (I5.3): the current
/// slot when it is open, at most `catch_up_days` old and named by no `run_started` event,
/// a failed start among them; else none.
pub fn slot_due<C>(facts: &Facts<C>, limits: &Limits) -> Option<u32> {
    let slot = current_slot(&facts.workflow, facts.today)?;
    let fresh = facts.today - slot <= limits.catch_up_days;
    let named = facts
        .events
        .iter()
        .any(|e| matches!(e.what, What::RunStarted { slot: Some(s), .. } if s == slot));
    (fresh && !named && slot_open(facts, slot)).then_some(slot)
}

/// Whether the agent has a clean run, a run of it that ended `completed` or `nothing_to_do` by
/// its `run_ended` event, the pair of `slot_open` (I5.3, ruling RD-67 5.2): the raise of `ready`,
/// the slot of a skip and the slot of a start by the person read it, and the scheduler reads
/// `finished`.
pub fn clean<C>(facts: &Facts<C>) -> bool {
    facts.events.iter().any(|e| {
        matches!(
            e.what,
            What::RunEnded {
                cause: RunEnd::Completed | RunEnd::NothingToDo,
                ..
            }
        )
    })
}

/// Whether the agent has a finished run, that is a run of it that ended `completed` by its
/// `run_ended` event (I5.3); every run of an agent with none is a first run.
pub fn finished<C>(facts: &Facts<C>) -> bool {
    facts.events.iter().any(|e| {
        matches!(
            e.what,
            What::RunEnded {
                cause: RunEnd::Completed,
                ..
            }
        )
    })
}
