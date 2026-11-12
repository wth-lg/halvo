//! The coordinator of Halvo's interface run: a pure library with no clock, no file, no
//! thread and no network of its own (contracts I0, I5.10). This crate holds the types,
//! the limits, the ports and their memory implementations, the words table, the list of
//! the page's commands and the writer of the page's fixtures.

pub mod guard;
pub mod items;
pub mod mem;
pub mod ports;
pub mod run;
pub mod schedule;
pub mod types;
pub mod words;
