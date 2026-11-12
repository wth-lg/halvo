//! The types of the interface run: the record of the kind `coord_event` with its events and closed lists. Every
//! parser refuses what the run's contract refuses and never repairs; its error, `Refusal`, holds no value of the
//! input, and a handler answers it with the error `malformed`.

mod json;

pub mod names;
pub mod record;

pub use json::Refusal;

pub use ::halvo_schema_core::coordinator::{ERROR_NAMES, KINDS};
