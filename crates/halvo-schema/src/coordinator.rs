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

/// The names that this run's record, bodies and replies write: the family, the kind name, the two `op` values,
/// every key and every name of the closed lists that they hold. Left out: `role`, which the names file never
/// holds, `none`, the choice of `setup` that stores no row, and `deleted`, the end of a run that stores no row.
pub const NAMES: &[&str] = &[
    "coord",
    "coord_event",
    "coord_log",
    "coord_history",
    "v",
    "workflow",
    "agent",
    "seq",
    "day",
    "at",
    "event",
    "run",
    "slot",
    "item",
    "kind",
    "effect",
    "approval",
    "digest",
    "answer",
    "cause",
    "trigger",
    "origin",
    "op",
    "records",
    "stored",
    "after",
    "last",
    "run_started",
    "run_ended",
    "item_raised",
    "item_answered",
    "item_lapsed",
    "item_released",
    "hold_set",
    "hold_cleared",
    "slot_skipped",
    "confirm",
    "sign_in",
    "link_account",
    "stuck",
    "check",
    "setup",
    "ready",
    "show",
    "person",
    "schedule",
    "completed",
    "nothing_to_do",
    "declined",
    "handed_back",
    "class_mismatch",
    "refused",
    "limit",
    "effect_unknown",
    "failed",
    "cancelled",
    "lapsed",
    "interrupted",
    "budget",
    "wait_limit",
    "no_record",
    "app_ended",
    "failures",
    "allow_once",
    "reject_once",
    "decline",
    "done",
    "allow",
    "try_again",
    "went_through",
    "choose",
    "skip",
    "start",
];

/// The most records of one `coord_log` body; it equals `request_records` of `Limits::PRODUCT` of halvo-coord.
pub const REQUEST_RECORDS: usize = 8;

/// The most bytes of one `coord_log` body as written; it equals `request_bytes` of `Limits::PRODUCT` of
/// halvo-coord.
pub const REQUEST_BYTES: usize = 4096;

/// The most records of one reply of `coord_history`.
pub const HISTORY_RECORDS: usize = 64;
