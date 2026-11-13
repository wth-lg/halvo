//! The web executor of A11.1: the agent's browser driven over a pipe by the one process that
//! started it, through a relay that reaches only the person's bound sites.

pub mod checks;
#[cfg(test)]
mod child;
pub mod guard;
pub mod peer;
pub mod pipe;
pub mod procs;
pub mod start;
