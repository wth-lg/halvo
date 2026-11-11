//! The types of the agent-setup run: an agent's configuration, the executable step and the playbook, the classes of
//! an action and the states of an effect, the bound object, the frames between a runner and its supervisor, the
//! runner and coordinator bodies with their replies, and the record shapes of the family `agent`. Every parser
//! refuses what the run's contract refuses and never repairs; its error, `Refusal`, holds no value of the input.

mod json;

pub mod config;
pub mod names;
pub mod text;

pub use config::{AgentConfig, Budget, Caps, Limits};
pub use json::{Refusal, RefusalCause};
pub use names::{
    ActionClass, AllowedClass, Autonomy, Cause, EffectState, ModelTier, Outcome, Reasoning, Runtime,
};
pub use text::{ApprovalId, Digest, EffectId, Name, OwnText, OwnValue, RunId, WebOrigin};

pub const KINDS: &[crate::kinds::KindEntry] = &[];
