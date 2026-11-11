//! The types of the agent-setup run: an agent's configuration, the executable step and the playbook, the classes of
//! an action and the states of an effect, the bound object, the frames between a runner and its supervisor, the
//! runner and coordinator bodies with their replies, and the record shapes of the family `agent`. Every parser
//! refuses what the run's contract refuses and never repairs; its error, `Refusal`, holds no value of the input.

mod json;

pub mod body;
pub mod bound;
pub mod config;
pub mod frame;
pub mod names;
pub mod record;
pub mod step;
pub mod text;

pub use body::{CoordinatorCall, CoordinatorReply, RunnerCall, RunnerReply};
pub use bound::{Bound, BoundField, KeptField};
pub use config::{AgentConfig, Budget, Caps, Limits};
pub use frame::{
    ConfirmDecision, ConfirmRequest, FromRunner, HandbackRequest, HandbackResult, StoreReply,
    StoreRequest, ToRunner,
};
pub use json::{Refusal, RefusalCause};
pub use names::{
    ActionClass, AllowedClass, Autonomy, Cause, EffectState, ModelTier, Outcome, Reasoning, Runtime,
};
pub use record::{AgentBindings, AgentEffect, AgentLog, AgentPlaybook, AgentRun, AgentSession};
pub use step::{ExecutableStep, Playbook};
pub use text::{ApprovalId, Digest, EffectId, Name, OwnText, OwnValue, RunId, WebOrigin};

/// The error names that this run's handlers answer, in the reply form of the workflow run.
pub const ERROR_NAMES: &[&str] = &["binding", "playbook", "config", "class", "length", "effect"];

pub const KINDS: &[crate::kinds::KindEntry] = &[];
