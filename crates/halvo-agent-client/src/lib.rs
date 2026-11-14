#![deny(unsafe_code)]
//! The supervisor library of the agent-setup run (contracts.md A6.6): it starts a runner, relays its store
//! requests over the owner link, asks the person through its caller's traits, and ends a run that lost its way,
//! also after the app was killed, by the mark that it keeps of each run under the data directory.

mod acts;
mod api;
pub mod marks;
mod owner;
mod proc;
pub mod process;
mod supervisor;
mod sys;
pub mod wire;

pub use api::{
    CallError, Config, Confirmer, EndRun, Human, IssueRefused, LinkError, MarkSink, OriginAnswer,
    OwnerLink, Parts, ReapAnswer, Reaped, RunEnd, RunSpec, Secrets, LINK_END, START_WAIT,
};
pub use supervisor::Supervisor;
