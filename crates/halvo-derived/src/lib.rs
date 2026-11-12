#![forbid(unsafe_code)]
//! The derived store of Halvo: one SQLCipher database under `derived/` of a data directory, which holds what
//! Halvo learns from what a person does.

mod batch;
mod db;
mod decide;
mod error;
mod handler;
mod hash;
mod host;
mod propose;
mod purge;
mod sql;
mod typed;

use std::path::Path;

use halvo_schema_core::kinds::KindEntry;
use halvo_schema_core::record::RecurrenceStep;

pub use batch::{CommitOutcome, ProposalOutcome};
pub use error::{Error, Fault, OpenError};
pub use handler::{Cause, Committed, Handler, HandlerCx, Refusal, Takes};
pub use host::{Clock, DbKey, Host, IdSource};

use db::Db;

/// The derived store, open. It holds the writer lock of `derived/` and the exclusive lock of its database until it
/// is dropped.
pub struct Store {
    db: Db,
    recurrence: Box<dyn RecurrenceStep>,
    steps: Vec<handler::StepEntry>,
    handlers: Vec<handler::HandlerEntry>,
}

impl Store {
    /// Opens the store under `<data_dir>/derived/`, creating the directory and the database where they are missing.
    ///
    /// `entries` are the kind entries the store takes: `kinds::registry()` in every host, with the test-only entries
    /// of a test host beside them. The open fails when `kinds::check_entries` reports a fault for them, when they lack
    /// one of the four kinds of the workflow run, when another store holds the writer lock or the database, when
    /// `key` does not open the database, when a setting that the store makes (secure delete, exclusive locking, the
    /// write-ahead log, foreign keys, the page-cache cap) is not in effect after it, and when an existing database
    /// holds another schema. The tables and their indexes are created only in a database that holds no table. At
    /// every open the store reconciles: it runs the truncating checkpoint, adds a revision of 0 for each family of
    /// `entries` that has none, and removes every evidence or support row whose summary is not stored and every row
    /// that names a workflow that is not stored. `ids` gives the key of the site hash when the database is created.
    /// It also fails when a step or a handler of `host` names the family `workflow` or a family outside the form of
    /// 1 to 32 bytes of `a` to `z`, `0` to `9` and `_`.
    pub fn open(
        data_dir: &Path,
        key: &DbKey,
        entries: &[&'static KindEntry],
        host: Host,
        ids: &mut dyn IdSource,
    ) -> Result<Store, OpenError> {
        let families = host
            .steps
            .iter()
            .map(|s| s.family)
            .chain(host.handlers.iter().map(|h| h.family));
        for family in families {
            let form = (1..=32).contains(&family.len())
                && family
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !form || family == "workflow" {
                return Err(OpenError::Family(family));
            }
        }
        let db = Db::open(data_dir, key, entries, host.clock, ids)?;
        Ok(Store {
            db,
            recurrence: host.recurrence,
            steps: host.steps,
            handlers: host.handlers,
        })
    }
}

#[cfg(feature = "test-host")]
pub mod test_host {}

#[cfg(test)]
mod tests;
