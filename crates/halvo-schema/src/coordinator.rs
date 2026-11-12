//! The types of the interface run: the record of the kind `coord_event` with its events and closed lists, and the
//! coordinator bodies `coord_log` and `coord_history` with their replies. Every parser refuses what the run's
//! contract refuses and never repairs; its error, `Refusal`, holds no value of the input, and a handler answers it
//! with the error `malformed`.

mod json;

pub mod body;
pub mod names;
pub mod record;

pub use json::Refusal;

pub use ::halvo_schema_core::coordinator::{ERROR_NAMES, KINDS};

/// The most records of one `coord_log` body; it equals `request_records` of `Limits::PRODUCT` of halvo-coord.
pub const REQUEST_RECORDS: usize = 8;

/// The most bytes of one `coord_log` body as written; it equals `request_bytes` of `Limits::PRODUCT` of
/// halvo-coord.
pub const REQUEST_BYTES: usize = 4096;

/// The most records of one reply of `coord_history`.
pub const HISTORY_RECORDS: usize = 64;
