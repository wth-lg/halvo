#![forbid(unsafe_code)]
//! The derived store of Halvo: one SQLCipher database under `derived/` of a data directory, which holds what
//! Halvo learns from what a person does. Every row stands in one record envelope whose kind fixes its retention
//! class: a window row lives no longer than its source segments, a bounded row the days of its kind's entry, and a
//! kept row until the person deletes its workflow or everything.
//!
//! # Hosts, steps, handlers and delete guards
//!
//! A host supplies its code to the store in one value, built in this form:
//!
//! ```text
//! Host::new(clock, recurrence_step)
//!     .step(family, writer_step)              // after the recurrence step, in this order
//!     .handler(family, takes, handler)        // in front of the handlers of the same requests added after it
//!     .delete_guard(family, guard)            // asked by each delete of a workflow, in this order
//!     .on_report(sink)                        // the report after each committed transaction
//! ```
//!
//! Each step, each handler and each delete guard names its own family. A step reads the summaries and the rows of its
//! own family and returns records of kinds of its own family; a handler commits records of kinds of its own family and
//! reads the rows of its own family. `Takes` names the requests that a handler takes: `runner` requests, the
//! `coordinator_write` bodies of a list of `op` names, or the requests of one further role. For one request the store
//! passes the request to the first handler that takes it; that handler is in front. It answers the request itself, or
//! calls `HandlerCx::pass_on`, which passes the request on unchanged to the next handler that takes it and returns
//! that handler's answer; the handler of the owning run is added last and has no handler behind it. Each handler of
//! the chain gets a `HandlerCx` of its own family, the handler behind as well, so a handler in front makes its typed
//! commits after `pass_on` has returned, in its own family. The owner reads the `op` of a `coordinator_write` and
//! nothing else of its body, so a body whose `workflow` and `agent` are null goes to the handler of its `op` as every
//! other body does.
//!
//! While it answers a request a handler gets six calls of the store and no other: `HandlerCx::commit`, the typed
//! commit; `HandlerCx::state`, the state call; `HandlerCx::held_ahead`, under a `coordinator_write` alone, the yes or
//! no whether a row of one kind of its own family under one workflow id lies more than two days after the clock's day;
//! `HandlerCx::remove_under`, under a `coordinator_write` alone, the removal of every row of one kind of its own family
//! under one workflow id, in one write with a typed commit, by the path of the delete cascade; `HandlerCx::rows`, the
//! read of the rows of its own family, of one kind whole or of one kind under one workflow id
//! (`Rows::for_each_under`); and `HandlerCx::workflow` and `HandlerCx::evidence`, the interface read of one stored
//! workflow. The store knows for which request it runs a typed commit and takes it from no argument: a record of a kind
//! of origin own is stored for a `runner` request and a `coordinator_write`, and refused from a step and for a request
//! of a further role.
//!
//! A delete guard (`DeleteGuard`) is a closure of a run whose rows can show that an agent still works under a workflow.
//! The delete of a stored workflow (`Store::decide`) calls each guard, in the order in which the host added them,
//! inside its own transaction and before it removes a row, with the workflow id and two inputs of the guard's own
//! family: a `RowRead` whose `for_each` visits the rows of one kind that are held under that workflow id and no other,
//! under the rule of the reads below (DS-4), and the held-ahead call by kind name (`HeldAhead`), which answers yes or
//! no for each kind of that family under that workflow and never `HeldRefusal::Role`. A guard answers
//! `GuardAnswer::Go` or `GuardAnswer::Wait`. At the first `Wait` the delete answers `state` and removes nothing, and
//! no guard behind it is called. A fault of the database in the read or in the held-ahead call that the store gives a
//! guard is the store's: the delete answers the fault and removes nothing, whatever the guard answered. So while a
//! guard reads a kind, a workflow with a row of that kind that the store cannot read is not deleted by itself; the
//! delete of everything still removes it. A delete of a workflow that is not stored calls no guard.
//!
//! # What a read hands out
//!
//! Every read hands out exactly the rows that an expiry at that read's clock would leave (DS-4): a bounded row while
//! its stored day of going lies after the clock's day, its day lies at most two days after the clock's day, and it
//! has a day; a window row while its `expires` lies after the clock's instant; a kept row always; and an evidence or
//! a support row only while the summary that it names is handed out. That holds for a step's summary read and row
//! read, a handler's `rows`, of one kind whole or under one workflow id, `workflow` and `evidence`, and the interface
//! read, and for the read that a delete hands its guards. The summary read leaves out, by a sentence of its own, a
//! summary of a day after the clock's day as well. The four conditions are written once, in `sql`, and the expiry's
//! statements and the reads are made from them. No read writes: a row that the reads leave out stays stored, counted
//! and found by a delete or a purge until an expiry removes it, and the revision rises then.
//!
//! The clock is the host's. The store reads it once at the start of each call of the host, `commit_batch` for the
//! reads of every step, `Store::interface_read` for one page, and `Store::handle` for every read of one request, the
//! handlers behind included; no read takes a day or an instant from its caller. A call that starts before a deadline
//! reads by its start, so a row that passes its bound while the call runs is handed out until the call ends. The
//! typed commit and the state call keep their own reading at the commit. A delete of `Store::decide` that calls a
//! guard reads the clock once, before its first guard, for the read and the held-ahead call of every guard.
//!
//! # What the store reports to its host
//!
//! `commit_batch` commits the batch and then the proposals of the recurrence step and the records of each step, each
//! in a transaction of its own. When the typed commit of a step's records is refused, the batch, the proposals and
//! the other steps' records stand, and the store reports the refusal to its host in `CommitOutcome::steps`, in the
//! place of that step, as a `Refusal` that names the index of the record and the rule; the analysis write that led to
//! it is no less committed. After each committed transaction the store hands the sink of its host a `Report`: the
//! revision of every family, the count per retention class, the bytes on disk, the next expiry deadline and the
//! committed position with its store id. A call that returns a fault after its transaction committed leaves that
//! transaction in place.
//!
//! # What a host and a later unit must know
//!
//! - While a `Store` is open no other connection can read its database, also one that holds its key: the store keeps
//!   the database in exclusive locking mode.
//! - At each answered commit and at each checkpoint the store asks the system to flush the drive: the open sets
//!   `synchronous` to FULL and `fullfsync` and `checkpoint_fullfsync` on, outside any transaction, reads each back and
//!   refuses by name a store where one reads otherwise. Where the file system refuses the full flush the database falls
//!   back to a plain sync and reports nothing, and the store cannot tell.
//! - A handler learns of a window record that its typed commit dropped, past its expiry or naming a purged ordinal,
//!   from `Committed::dropped` alone; the commit refuses nothing for it.
//! - A stop between a batch's transaction and its steps leaves the batch committed with no proposal and no step
//!   record; the same batch sent again answers `repeated` and runs no step, and the store notes no owed step.
//! - A request is not one transaction of the database: each typed commit and the state call commits on its own, and a
//!   stop between two of them leaves the earlier ones stored; so does an error answer or a panic of a handler behind an
//!   earlier store of the same request.
//! - An open fails while its entries give a stored kind another retention class or another family, a workflow key
//!   under which a stored row names another workflow than its workflow column holds, or no workflow key while a
//!   stored row of the kind names a workflow; the crate holds no migration for any of them.
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
mod guard;
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
pub use guard::{DeleteGuard, GuardAnswer, HeldAhead};
pub use handler::{
    Cause, Committed, DayFault, Handler, HandlerCx, Refusal, RemoveRefusal, Removed, Takes,
};
pub use host::{Clock, DbKey, Host, IdSource, ReportSink};
pub use read::ReadHandle;
pub use report::{CommittedPosition, Counts, PositionState, Report};
pub use state::{HeldRefusal, MoveTo};
pub use typed::Rows;

use db::{Db, Keying};

/// The derived store, open. It holds the writer lock of `derived/` and the exclusive lock of its database until it
/// is dropped.
pub struct Store {
    db: Db,
    recurrence: Box<dyn RecurrenceStep>,
    steps: Vec<handler::StepEntry>,
    handlers: Vec<handler::HandlerEntry>,
    guards: Vec<guard::GuardEntry>,
}

impl Store {
    /// Opens the store under `<data_dir>/derived/`, creating the directory and the database where they are missing.
    /// `entries` are the kind entries the store takes: `kinds::registry()` in every host, with the test-only entries of
    /// a test host beside them. The open fails when `kinds::check_entries` reports a fault for them, when they lack one
    /// of the four kinds of the workflow run, when another store holds the writer lock or the database, when `key` does
    /// not open the database, when a setting that the store makes (secure delete, temporary storage in memory,
    /// exclusive locking, the write-ahead log, `synchronous` FULL, `fullfsync` and `checkpoint_fullfsync` on, foreign
    /// keys, the page-cache cap) does not read back as set from the store's one connection, the connection of all its
    /// writes and reads, after it sets it, when the database's `auto_vacuum` is not 0 or its page size is not 4,096,
    /// when the linked database is not Halvo's build that zeroes the gap of a page that it builds again or edits
    /// (`OpenError::Setting("page_gap_mend_linked")`, before any file is touched), and when an existing database holds
    /// another schema. Where `derived/derived.db` exists and is not empty, the open first tests `key` on a read-only
    /// connection of its own, before it takes the writer lock: a key that does not open the database answers
    /// `OpenError::Sealed` with no writer lock taken and no byte of the file changed, and a database that another
    /// connection holds answers `OpenError::Locked`; every other answer goes on to the open behind the writer lock,
    /// which tests the key again. Behind the lock the open reads the database's `auto_vacuum`, its page size and its
    /// schema before it changes a byte of the file, so an open that one of them refuses leaves the file as it was, a
    /// file in rollback-journal mode as well; only then does it set the write-ahead log and the other settings and
    /// create the schema in a database that holds none. It also fails, with `OpenError::Stored`, before it changes a
    /// row, where rows of a kind are stored that the kind's entry cannot follow: rows stored under another retention
    /// class or another family than the entry gives, rows that the store deletes with one workflow while their record
    /// names another, or none, under the entry's workflow key, or rows that the store deletes with a workflow while the
    /// entry gives no workflow key; an open with the earlier entry then works as before. The tables and their indexes
    /// are created only in a database that holds no table. At every open the store reconciles: it runs the truncating
    /// checkpoint, adds a revision of 0 for each family of `entries` that has none, fills the workflow column of each
    /// row of a kind whose entry gives a `workflow_key` from the value under that key where the row holds none, removes
    /// every evidence or support row whose summary is not stored and every row that names a workflow that is not
    /// stored, brings the day on which each stored bounded row goes to the bound of its kind's entry in `entries` where
    /// that bound is the shorter, gives a bounded row of a kind of `entries` with a day and no day of going its day
    /// plus the days of its entry, and then runs the expiry of `Store::expire`, which reads that day as the report's
    /// deadline does. `ids` gives the key of the site hash when the database is created. It also fails when a step, a
    /// handler or a delete guard of `host` names the family `workflow` or a family outside the form of 1 to 32 bytes of
    /// `a` to `z`, `0` to `9` and `_`.
    pub fn open(
        data_dir: &Path,
        key: &DbKey,
        entries: &[&'static KindEntry],
        host: Host,
        ids: &mut dyn IdSource,
    ) -> Result<Store, OpenError> {
        Store::open_keyed(data_dir, Keying::Key(key), entries, host, ids)
    }

    pub(crate) fn open_keyed(
        data_dir: &Path,
        keying: Keying<'_>,
        entries: &[&'static KindEntry],
        host: Host,
        ids: &mut dyn IdSource,
    ) -> Result<Store, OpenError> {
        let families = host
            .steps
            .iter()
            .map(|s| s.family)
            .chain(host.handlers.iter().map(|h| h.family))
            .chain(host.guards.iter().map(|g| g.family));
        for family in families {
            let form = (1..=32).contains(&family.len())
                && family
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !form || family == "workflow" {
                return Err(OpenError::Family(family));
            }
        }
        let db = Db::open(data_dir, keying, entries, host.clock, host.report, ids)?;
        Ok(Store {
            db,
            recurrence: host.recurrence,
            steps: host.steps,
            handlers: host.handlers,
            guards: host.guards,
        })
    }
}

#[cfg(feature = "test-host")]
pub mod test_host;

#[cfg(test)]
mod tests;
