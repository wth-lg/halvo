//! The types of the coordinator (contracts I0.5, I0.10, I1.2, I3, I5, I8).

mod cause;
mod config;
mod event;
mod facts;
mod ids;
mod limits;
mod names;
mod run;
mod status;
mod workflow;

pub use cause::Cause;
pub use config::{Budget, ConfigView};
pub use event::{Answered, Body, Cleared, Event, Reply, Slot, What};
pub use facts::{Facts, OpenItem};
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
pub use run::{
    Answer, AskedEffect, BoundField, ConfirmAsk, Decision, DecisionEntry, HandbackAsk, RunEvent,
    RunSpec,
};
pub use status::StatusLine;
pub use workflow::{Agent, DismissedWorkflow, FullWorkflow, Kept, PastRun, Unknown, Workflow};

#[cfg(test)]
mod tests;
