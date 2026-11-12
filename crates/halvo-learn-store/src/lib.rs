#![forbid(unsafe_code)]
//! The learning handlers of the learning run (contracts.md L4): `LearnHandlers`, which a host puts in front of
//! the agent store's handler for the request types `runner` and `coordinator_write`. They pass every request on,
//! serve learned targets in the reply of `load_steps`, hold the proposals of a replay run as its candidates,
//! read its log, and at its `finish` make one typed commit of the family `learn` with the rows that the rule of
//! halvo-learn gives; they answer the read `list_refinements`. They read no clock but the host's `Clock`, no file
//! and no network.

mod agent;
mod convert;
mod coordinator;
mod facts;
mod handlers;
mod host;
mod ids;
mod routine;
mod rows;
mod serve;

pub use handlers::{CoordinatorAnswer, LearnHandlers, Refused};
pub use host::{Clock, LearnHost};

#[cfg(test)]
mod tests;
