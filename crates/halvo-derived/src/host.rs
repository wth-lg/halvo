use std::fmt;

use halvo_schema_core::record::{Instant, RecurrenceStep};

use crate::handler::{HandlerEntry, StepEntry};
use crate::report::Report;

/// The clock of a host: the instant now and the local calendar. The owner gives the machine's clock and time zone;
/// a replay gives a clock that it sets, with UTC as local time.
pub trait Clock {
    /// The instant now.
    fn now(&self) -> Instant;
    /// The local calendar day that holds the instant, counted in days from 1970-01-01.
    fn day(&self, at: Instant) -> u32;
    /// The first instant of a local calendar day.
    fn day_start(&self, day: u32) -> Instant;
}

/// Random bytes from the host: the operating system's generator in the owner, the seeded generator of a replay.
/// The store takes from it the id of each workflow that it stores and, when it creates a database, the key of
/// the site hash.
pub trait IdSource {
    /// Fills `bytes` with random bytes.
    fn fill(&mut self, bytes: &mut [u8]);
}

/// The key of the derived database: 32 bytes that SQLCipher takes as a raw key. `Debug` prints the type name
/// alone.
#[derive(Clone)]
pub struct DbKey([u8; 32]);

impl DbKey {
    /// The key of these bytes.
    pub fn new(bytes: [u8; 32]) -> DbKey {
        DbKey(bytes)
    }

    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for DbKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DbKey")
    }
}

impl Drop for DbKey {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&self.0);
    }
}

/// What a host supplies to the store: its clock and its recurrence step, which every host puts first.
pub struct Host {
    pub(crate) clock: Box<dyn Clock>,
    pub(crate) recurrence: Box<dyn RecurrenceStep>,
    pub(crate) steps: Vec<StepEntry>,
    pub(crate) handlers: Vec<HandlerEntry>,
    pub(crate) report: Option<ReportSink>,
}

/// Where the store hands its host the report after each committed transaction.
pub type ReportSink = Box<dyn FnMut(&Report)>;

impl Host {
    /// A host with its clock and its recurrence step: the test proposer in a test host, halvo-identify's step in
    /// `replay` and in the owner.
    pub fn new(clock: Box<dyn Clock>, recurrence: Box<dyn RecurrenceStep>) -> Host {
        Host {
            clock,
            recurrence,
            steps: Vec::new(),
            handlers: Vec::new(),
            report: None,
        }
    }

    /// Hands the host the report after each committed transaction of the store: the revision of every family, the
    /// count per retention class, the database's bytes on disk and its next expiry deadline.
    pub fn on_report(mut self, sink: ReportSink) -> Host {
        self.report = Some(sink);
        self
    }
}
