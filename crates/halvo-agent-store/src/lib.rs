#![forbid(unsafe_code)]
//! The agent store of the agent-setup run (A7.5): the handlers of its runner and coordinator bodies and the store in
//! memory that serves them until the derived store lands. A handler decodes its body with the typed parser,
//! checks it against the stored rows of the family `agent` and stores rows by the typed commit or moves the
//! workflow by the state call; it reads no clock, no file and no network.

mod bind;
mod bindings;
mod coordinator;
mod effect;
mod end;
mod errors;
mod host;
mod memory;
mod rows;
mod runner;
mod workbook;

pub use coordinator::coordinator;
pub use host::{Host, Now};
pub use memory::{CommitRefused, MemoryStore, AGENT_KINDS};
pub use runner::runner;
pub use workbook::{workbook_path, PathForm, PathRefused};
