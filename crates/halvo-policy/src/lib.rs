#![forbid(unsafe_code)]
//! The confirm policy of the agent-setup run as pure functions: the class of a step and the check of a playbook,
//! the decision of the table and the typed decision that only `decide` makes, the step of an effect's state, the
//! digest of a bound object, the admission of its fields, the presets of the seed categories with their
//! validation, and the limits. No function reads a file, a clock or the network.

mod admit;
mod class;
mod config;
mod decide;
mod digest;
mod effect;
mod limits;

#[cfg(test)]
mod formal_traces;
#[cfg(test)]
mod step_traces;

pub use admit::{admit, Facts, Provenance};
pub use class::{check_playbook, class_of, ClassDiffers};
pub use config::{preset, validate, ConfigCause};
pub use decide::{decide, decide_row, Checkpoint, Decided, Decision};
pub use digest::digest;
pub use effect::{effect_step, Effect, EffectMove, StepCause};
pub use limits::{LimitTable, LIMITS};
