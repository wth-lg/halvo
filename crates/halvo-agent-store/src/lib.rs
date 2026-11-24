#![forbid(unsafe_code)]
//! The agent store of the agent-setup run (A7.5): the handlers of its runner and coordinator bodies and the store in
//! memory that serves them until the derived store lands. A handler decodes its body with the typed parser,
//! checks it against the stored rows of the family `agent` and stores rows by the typed commit or moves the
//! workflow by the state call; it reads no clock, no file and no network.

mod bind;
mod bindings;
mod checkpoint;
mod coordinator;
mod count;
mod day;
mod effect;
mod end;
mod errors;
mod host;
mod memory;
mod reply;
mod rows;
mod runner;
mod workbook;

#[cfg(test)]
mod agreement_tests;
#[cfg(test)]
mod answers_tests;

pub use coordinator::{coordinator, GOES_ON_AFTER};
pub use effect::allowed_not_done;
pub use host::{Host, Now};
pub use memory::{CommitRefused, MemoryStore, AGENT_KINDS};
pub use runner::{newest_playbook, runner};
pub use workbook::{workbook_path, PathForm, PathRefused};
