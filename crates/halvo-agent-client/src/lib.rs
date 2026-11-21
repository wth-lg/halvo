#![deny(unsafe_code)]
//! The supervisor library of the agent-setup run (contracts.md A6.6): it starts a runner, relays its store
//! requests over the owner link, asks the person through its caller's traits, and ends a run that lost its way,
//! also after the app was killed, by the mark that it keeps of each run under the data directory.

mod acts;
mod api;
mod checkpoints;
pub mod marks;
mod owner;
mod proc;
pub mod process;
mod supervisor;
mod sys;
pub mod wire;

pub use api::{
    CallError, Config, Confirmer, EndRun, Forced, ForcedPath, Human, IssueRefused, LinkError,
    ListFailed, MarkSink, NotStarted, OriginAnswer, OwnerLink, Parts, ReapAnswer, Reaped,
    RelativePath, RunEnd, RunSpec, RunnerExit, Secrets, StartError, SystemValue, LINK_END,
    START_WAIT,
};
#[doc(hidden)]
pub use api::{ReapHook, ReapStep};
pub use supervisor::Supervisor;
