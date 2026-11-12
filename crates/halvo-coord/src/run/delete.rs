use crate::items::drive::Ledger;
use crate::schedule::{tick_plan, Board};
use crate::types::{AgentId, Limits, RunId, WorkflowId};

/// What a tick does for the workflows that are gone from the owner's list (I5.7): the runs
/// that no stop has reached, which it stops with the cause `deleted`, also in the guard's
/// table; the `stopping` runs, which take `deleted` as the cause of their stop; the agents
/// whose queued start or open item it drops with nothing stored; and the workflows that no
/// earlier tick saw gone, whose agents' events the coordinator sends no more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gone {
    /// The runs to stop, in the order of their starts.
    pub stop: Vec<RunId>,
    /// The `stopping` runs whose cause becomes `deleted`.
    pub recause: Vec<RunId>,
    /// The agents whose queued start or open item goes.
    pub dropped: Vec<AgentId>,
    /// The workflows seen gone for the first time.
    pub seen: Vec<WorkflowId>,
}

/// The part of a tick on the workflows that are not in `listed`, the owner's list of the tick
/// (I5.7, I5.9): the part of `tick_plan` that reads neither the clock nor the limits.
pub fn delete_step<C>(board: &Board<C>, listed: &[WorkflowId]) -> Gone {
    let plan = tick_plan(board, listed, 0, &Limits::PRODUCT);
    Gone {
        stop: plan.stop,
        recause: plan.recause,
        dropped: plan.dropped,
        seen: plan.seen,
    }
}

/// The ledger of an agent whose workflow is gone, with no event waiting: the owner removed
/// every row that names the workflow and refuses a new one with `no_workflow`, so that an
/// event of it is sent no more (I5.7, I7.2).
pub fn forgotten(workflow: WorkflowId, agent: AgentId, ledger: &Ledger) -> Ledger {
    Ledger::new(workflow, agent, ledger.last())
}
