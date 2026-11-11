//! The types of the coordinator (contracts I0.5, I0.10, I1.2, I3, I5, I8).

mod cause;
mod ids;
mod limits;
mod names;

pub use cause::Cause;
pub use ids::{
    hex, AgentId, ApprovalId, Digest, EffectId, ItemId, Name, Origin, RunId, WorkflowId,
};
pub use limits::Limits;
pub use names::{
    Action, AgentState, ArgType, Autonomy, BoundFrom, CandidateFrom, Category, Choice, Class,
    Control, EffectAnswer, Failure, FieldFrom, HandbackReason, HandbackResult, HoldSetCause,
    ItemKind, LapseCause, LineKey, LivePhase, Mode, Outcome, Param, Period, Permission, Presenter,
    Reasoning, RunEnd, Runtime, Status, Tier, Trigger, WorkflowState, WorkflowTrigger, YesNo,
};

#[cfg(test)]
mod tests;
