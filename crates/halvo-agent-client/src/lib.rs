#![deny(unsafe_code)]
//! The supervisor library of the agent-setup run (contracts.md A6.6): it starts a runner, relays its store
//! requests over the owner link, asks the person through its caller's traits, and ends a run that lost its way,
//! also after the app was killed, by the mark that it keeps of each run under the data directory.

pub mod marks;
pub mod wire;
