#![forbid(unsafe_code)]
//! The store side of the interface run (contracts.md I7.3): the handler of the coordinator bodies `coord_log` and
//! `coord_history`, and the store in memory that serves it until the derived store lands. The handler reads a body
//! with the typed parser of `halvo_schema::coordinator`, checks it against the stored rows of the family `coord` and
//! stores what the typed writer writes by the typed commit; it reads the clock of its host and no clock, file or
//! network of its own.

mod handler;
mod host;
mod memory;

pub use handler::{coordinator, CoordReply, HISTORY_PAGE, OPS};
pub use host::{Clock, CommitRefused, DayFault, Host};
pub use memory::MemoryStore;
