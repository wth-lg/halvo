//! The types of the coordinator (contracts I0.5, I0.10, I1.2, I3, I5, I8).

pub mod commands;
pub mod fixtures;

mod cause;
mod config;
mod event;
mod facts;
mod ids;
mod limits;
mod names;
mod reply;
mod run;
mod status;
pub(crate) mod strict;
mod workflow;

pub use cause::Cause;
pub use config::{Budget, ConfigView};
pub use event::{
    Answered, BindAnswer, BindResult, Body, CheckAnswer, Cleared, Defers, DoneAnswer, Event,
    ItemLapseCause, Lapsed, LinkAnswer, OtherLapse, Raised, ReadyAnswer, Reply, SetupAnswer,
    SignInAnswer, Slot, What,
};
pub use facts::{
    Doing, Facts, Live, OpenItem, OwnItem, OwnItemKind, Replies, ReplyRow, ReplyState, RunItem,
    RunItemKind,
};
pub use ids::{
    hex, AgentId, ApprovalId, Digest, EffectId, ItemId, Name, Origin, RunId, WorkflowId,
};
pub use limits::Limits;
pub use names::{
    Action, AgentState, ArgType, Autonomy, BoundFrom, CandidateFrom, Category, Choice, Class,
    Control, EffectAnswer, Failure, FieldFrom, HandbackReason, HandbackResult, HistoryRead,
    HoldSetCause, ItemKind, LapseCause, LineKey, LivePhase, Mode, Outcome, Param, Period,
    Permission, Presenter, Reasoning, RunEnd, Runtime, Status, Tier, Trigger, WorkflowState,
    WorkflowTrigger, YesNo,
};
pub use reply::{Ask, AskAct, AskControl, AskEffect, AskKind};
pub use reply::{
    BusyHistory, Candidate, Card, CardEffect, CardLine, CardRun, ChoiceText, ClosedReply,
    ControlText, ErrorReply, Field, Item, OkReply, Overlapping, ParamEntry, Refusal, Row, Snapshot,
    Unread,
};
pub use run::{
    Answer, AskedEffect, BindAsk, BoundField, ConfirmAsk, Decision, DecisionEntry, HandbackAsk,
    Progress, RunEvent, RunSpec,
};
pub use status::{StatusLine, System};
pub use workflow::{
    Agent, DismissedWorkflow, FullWorkflow, Kept, PastEffect, PastRun, RunHistory, Unknown,
    Workflow,
};

#[cfg(test)]
mod tests;
