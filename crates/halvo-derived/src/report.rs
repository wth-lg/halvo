use halvo_schema_core::record::Instant;

use crate::error::Fault;
use crate::Store;

/// The count of stored rows per retention class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub window: u64,
    pub bounded: u64,
    pub kept: u64,
}

/// The four values that the store gives its host after each committed transaction, which the owner's report to the
/// daemon carries: the revision of every family of the entries, in their order; the count per class; the bytes on
/// disk of the database file, its write-ahead log and its shared-memory file, summed; and the next expiry deadline,
/// the first instant at which a stored row passes its bound, or none where no stored row expires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub revisions: Vec<(&'static str, u64)>,
    pub counts: Counts,
    pub disk_bytes: u64,
    pub expiry: Option<Instant>,
}

impl Store {
    /// Runs the expiry that every open runs, on the store's clock as it reads now: it removes every bounded row past
    /// its bound, by the entry of its kind when it was stored and by the entry that the store was opened with, or
    /// whose day lies after the clock's day, every window row whose expiry has passed, and every row
    /// that then names a summary or a workflow that is not stored. It returns the number of rows removed and raises
    /// the revision of every family that lost a row; the truncating checkpoint runs after it.
    pub fn expire(&mut self) -> Result<u64, Fault> {
        let db = &self.db;
        db.arm();
        let now = db.now();
        db.write_checkpointed(|tx| tx.expire(now, &db.entries))
    }

    /// The four values of the report as they stand.
    pub fn report(&self) -> Result<Report, Fault> {
        self.db.arm();
        self.db.report()
    }
}
