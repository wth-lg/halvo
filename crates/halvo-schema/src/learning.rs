//! The types of the learning run: the records of its three kinds of the family `learn` (`learn_run`,
//! `learn_edit` and `learn_routine`) with their closed lists, and the coordinator body `list_refinements` with
//! its reply. Every parser refuses what the run's contract refuses and never repairs; its error, `Refusal`,
//! holds no value of the input, and a handler answers it with the error `malformed`.

mod json;

pub mod body;
pub mod names;
pub mod record;

pub use json::Refusal;

pub use ::halvo_schema_core::learning::{ERROR_NAMES, KINDS};

/// The names that this run's records, body and reply write: the family, the three kind names, the `op` value,
/// every key at every depth and every name of the closed lists that they hold. Left out: `role`, which the names
/// file never holds.
pub const NAMES: &[&str] = &[
    "learn",
    "learn_edit",
    "learn_run",
    "learn_routine",
    "list_refinements",
    "v",
    "workflow",
    "agent",
    "run",
    "edit",
    "rev",
    "day",
    "base",
    "n",
    "mode",
    "state",
    "cause",
    "on",
    "runs",
    "first",
    "last",
    "clean",
    "assisted",
    "kept",
    "failed_step",
    "target",
    "desc",
    "locators",
    "by",
    "value",
    "name",
    "category",
    "site",
    "key",
    "source",
    "app",
    "period",
    "seen",
    "next",
    "missed",
    "mark",
    "since",
    "op",
    "refinements",
    "what",
    "compile",
    "replay",
    "started",
    "verified",
    "unverified",
    "nothing_to_do",
    "declined",
    "handed_back",
    "class_mismatch",
    "refused",
    "limit",
    "effect_unknown",
    "failed",
    "unknown",
    "provisional",
    "active",
    "reverted",
    "withdrawn",
    "step_failed",
    "origin",
    "label",
    "text",
    "test_id",
    "css",
    "none",
    "stale",
    "target_learned",
    "target_kept",
    "target_revoked",
    "routine_stale",
];

/// The most learned locators of one edit's list; it equals `LOCATORS_MAX` of halvo-learn.
pub const LOCATORS_BOUND: usize = 4;

/// The most bytes of a learned target's compact JSON, the bytes that its writer writes and a `learn_edit` row
/// stores: the target stands inside a step of at most 2,048 bytes (AG-A3.4), so one row stays within the 2,560
/// bytes of contracts.md L1.7.
pub const TARGET_BYTES: usize = 2048;

/// The most run ids of one edit's source set; it equals 1 + `K_CLEAN` of halvo-learn.
pub const RUNS_BOUND: usize = 3;

/// The highest `clean` of an edit; it equals `K_CLEAN` of halvo-learn.
pub const CLEAN_BOUND: u32 = 2;
