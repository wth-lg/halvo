//! The types of the learning run: the records of its three kinds of the family `learn` (`learn_run`,
//! `learn_edit` and `learn_routine`) with their closed lists. Every parser refuses what the run's contract
//! refuses and never repairs; its error, `Refusal`, holds no value of the input, and a handler answers it with
//! the error `malformed`.

mod json;

pub mod names;
pub mod record;

pub use json::Refusal;

pub use halvo_schema_core::learning::{ERROR_NAMES, KINDS};

/// The most learned locators of one edit's list; it equals `LOCATORS_MAX` of halvo-learn.
pub const LOCATORS_BOUND: usize = 4;

/// The most run ids of one edit's source set; it equals 1 + `K_CLEAN` of halvo-learn.
pub const RUNS_BOUND: usize = 3;

/// The highest `clean` of an edit; it equals `K_CLEAN` of halvo-learn.
pub const CLEAN_BOUND: u32 = 2;
