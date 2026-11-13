#![forbid(unsafe_code)]
//! The derived store of Halvo: one SQLCipher database under `derived/` of a data directory, which holds what
//! Halvo learns from what a person does. Every row stands in one record envelope whose kind fixes its retention
//! class: a window row lives no longer than its source segments, a bounded row the days of its kind's entry, and a
//! kept row until the person deletes its workflow or everything.
//!
//! # Hosts, steps and handlers
//!
//! A host supplies its code to the store in one value, built in this form:
//!
//! ```text
//! Host::new(clock, recurrence_step)
//!     .step(family, writer_step)              // after the recurrence step, in this order
//!     .handler(family, takes, handler)        // in front of the handlers of the same requests added after it
//!     .on_report(sink)                        // the report after each committed transaction
//! ```
//!
//! Each step and each handler names its own family. A step reads the summaries and the rows of its own family and
//! returns records of kinds of its own family; a handler commits records of kinds of its own family and reads the
//! rows of its own family. `Takes` names the requests that a handler takes: `runner` requests, the `coordinator_write`
//! bodies of a list of `op` names, or the requests of one further role. For one request the store passes the request
//! to the first handler that takes it; that handler is in front. It answers the request itself, or calls
//! `HandlerCx::pass_on`, which passes the request on unchanged to the next handler that takes it and returns that
//! handler's answer; the handler of the owning run is added last and has no handler behind it. Each handler of the
//! chain gets a `HandlerCx` of its own family, the handler behind as well, so a handler in front makes its typed
//! commits after `pass_on` has returned, in its own family. The owner reads the `op` of a `coordinator_write` and
//! nothing else of its body, so a body whose `workflow` and `agent` are null goes to the handler of its `op` as every
//! other body does.
//!
//! While it answers a request a handler gets five calls of the store and no other: `HandlerCx::commit`, the typed
//! commit; `HandlerCx::state`, the state call; `HandlerCx::held_ahead`, under a `coordinator_write` alone, the yes or
//! no whether a row of one kind of its own family under one workflow id lies more than two days after the clock's day;
//! `HandlerCx::rows`, the read of the rows of its own family, of one kind whole or of one kind under one workflow id
//! (`Rows::for_each_under`); and `HandlerCx::workflow` and `HandlerCx::evidence`, the interface read of one stored
//! workflow. The store knows for which request it runs a typed commit and takes it from no argument: a record of a kind
//! of origin own is stored for a `runner` request and a `coordinator_write`, and refused from a step and for a request
//! of a further role.
//!
//! # What a read hands out
//!
//! Every read hands out exactly the rows that an expiry at that read's clock would leave (DS-4): a bounded row while
//! its stored day of going lies after the clock's day, its day lies at most two days after the clock's day, and it
//! has a day; a window row while its `expires` lies after the clock's instant; a kept row always; and an evidence or
//! a support row only while the summary that it names is handed out. That holds for a step's summary read and row
//! read, a handler's `rows`, of one kind whole or under one workflow id, `workflow` and `evidence`, and the interface
//! read. The summary read leaves out, by a sentence of its own, a summary of a day after the clock's day as well. The
//! four conditions are written once, in `sql`, and the expiry's statements and the reads are made from them. No read
//! writes: a row that the reads leave out stays stored, counted and found by a delete or a purge until an expiry
//! removes it, and the revision rises then.
//!
//! The clock is the host's. The store reads it once at the start of each call of the host, `commit_batch` for the
//! reads of every step, `Store::interface_read` for one page, and `Store::handle` for every read of one request, the
//! handlers behind included; no read takes a day or an instant from its caller. A call that starts before a deadline
//! reads by its start, so a row that passes its bound while the call runs is handed out until the call ends. The
//! typed commit and the state call keep their own reading at the commit.
//!
//! # What the store reports to its host
//!
//! `commit_batch` commits the batch and then the proposals of the recurrence step and the records of each step, each
//! in a transaction of its own. When the typed commit of a step's records is refused, the batch, the proposals and
//! the other steps' records stand, and the store reports the refusal to its host in `CommitOutcome::steps`, in the
//! place of that step, as a `Refusal` that names the index of the record and the rule; the analysis write that led to
//! it is no less committed. After each committed transaction the store hands the sink of its host a `Report`: the
//! revision of every family, the count per retention class, the bytes on disk and the next expiry deadline. A call
//! that returns a fault after its transaction committed leaves that transaction in place.
//!
//! # What a host and a later unit must know
//!
//! - While a `Store` is open no other connection can read its database, also one that holds its key: the store keeps
//!   the database in exclusive locking mode.
//! - A handler learns of a window record that its typed commit dropped, past its expiry or naming a purged ordinal,
//!   from `Committed::dropped` alone; the commit refuses nothing for it.
//! - A stop between a batch's transaction and its steps leaves the batch committed with no proposal and no step
//!   record; the same batch sent again answers `repeated` and runs no step, and the store notes no owed step.
//! - An open fails while its entries give a stored kind another retention class, or a workflow key under which a
//!   stored row names another workflow than its workflow column holds; the crate holds no migration for either.
//! - A summary one or two days after the clock's day is stored and counted in the report, and no step sees it until
//!   the clock reaches its day.
//! - The list of the check `scripts/r2/ci.d/50-derived.sh` names the crate's tests by hand. The check runs the crate's
//!   test binaries and fails for each test that they report, ignored or not, that the list does not name, so a commit
//!   that adds a `#[test]` adds its line. It runs no doc-test, and it cannot list a `#[should_panic]` test, which the
//!   binaries report under its name followed by ` - should panic`, so the check fails while such a test exists.
//!
//! # What may be logged
//!
//! The store logs nothing. Its errors (`Error`, `Fault`, `OpenError`, `Refusal`) hold an error name of the reply form,
//! a result code, a rule word or the error text of `kinds::check_record`, which names a kind, a key and a rule, and
//! never a value of a record or the text of a serde error, so a host may log them. A host never logs the text of a
//! `serde_json::Error` from a record or a request, nor the `Debug` form of a request type of the schema.

mod batch;
mod db;
mod decide;
mod error;
mod handler;
mod hash;
mod host;
mod propose;
mod purge;
mod read;
mod report;
mod sql;
mod state;
mod typed;

use std::path::Path;

use halvo_schema_core::kinds::KindEntry;
use halvo_schema_core::record::RecurrenceStep;

pub use batch::{CommitOutcome, ProposalOutcome};
pub use error::{Error, Fault, OpenError, StoredFault};
pub use handler::{Cause, Committed, DayFault, Handler, HandlerCx, Refusal, Takes};
pub use host::{Clock, DbKey, Host, IdSource, ReportSink};
pub use read::ReadHandle;
pub use report::{Counts, Report};
pub use state::{HeldRefusal, MoveTo};
pub use typed::Rows;

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
    /// one of the four kinds of the workflow run, when another store holds the writer lock or the database, when `key`
    /// does not open the database, when a setting that the store makes (secure delete, exclusive locking, the
    /// write-ahead log, foreign keys, the page-cache cap) does not read back as set from the store's one connection,
    /// the connection of all its writes and reads, after it sets it, and when an existing database holds another
    /// schema. It also fails, with `OpenError::Stored`, before it writes anything, where rows of a kind
    /// are stored that the kind's entry cannot follow: rows stored under another retention class than the entry gives,
    /// or rows that the store deletes with one workflow while their record names another, or none, under the entry's
    /// workflow key; an open with the earlier entry then works as before. The tables and their indexes are created
    /// only in a database that holds no table. At every open the store reconciles: it runs the truncating checkpoint,
    /// adds a revision of 0 for each family of `entries` that has none, fills the workflow column of each row of a
    /// kind whose entry gives a `workflow_key` from the value under that key where the row holds none, removes every
    /// evidence or support row whose summary is not stored and every row that names a workflow that is not stored,
    /// brings the day on which each stored bounded row goes to the bound of its kind's entry in `entries` where that
    /// bound is the shorter, and then runs the expiry of `Store::expire`, which reads that day as the report's
    /// deadline does. `ids` gives the key of the site hash when the database is created. It also fails when a step or
    /// a handler of `host` names the family `workflow` or a family outside the form of 1 to 32 bytes of `a` to `z`,
    /// `0` to `9` and `_`.
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
        let db = Db::open(data_dir, key, entries, host.clock, host.report, ids)?;
        Ok(Store {
            db,
            recurrence: host.recurrence,
            steps: host.steps,
            handlers: host.handlers,
        })
    }
}

#[cfg(feature = "test-host")]
pub mod test_host;

#[cfg(test)]
mod tests;
