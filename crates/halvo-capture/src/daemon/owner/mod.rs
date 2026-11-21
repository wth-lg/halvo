//! The store owner: the one thread that holds the capture store. Each loop
//! pass handles the waiting commands, then one received event, then the due
//! work: a flush or its retry, and one chunk of maintenance.

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use halvo_control::model::{StorageCap, UtcSecond};
use zeroize::Zeroize;

use super::barrier::Gate;
use super::batch::{utc_day, Admit, Batch, TextFilters};
use super::counters::{BatchCode, Counters, DiscardReason, Snapshot, SourceTallies};
use super::maintenance::{Clocks, Instants, Job, Schedule};
use super::port::{ByteBound, CaptureStore, EntryKind, MaintFault, WriteFault, PAYLOAD_MAX_BYTES};
use crate::floor::exclude::FloorHandle;
use crate::floor::record;
use crate::sink::{Barrier, Received, SinkWriter};

mod commands;
pub mod open;

/// The slots of the sink's queue that the owner drains.
pub const QUEUE_SLOTS: usize = 1_024;

/// The most events that the owner holds while its drain fails. It is a count
/// of its own and does not follow [`QUEUE_SLOTS`]; the spill is also bounded
/// in bytes by [`HELD_MAX_BYTES`].
pub const HELD_MAX_EVENTS: usize = 2_560;

/// The most record bytes, with their line ends, that the owner holds while its
/// drain fails: twice the batch budget.
pub const HELD_MAX_BYTES: usize = 2 * PAYLOAD_MAX_BYTES;

/// Under this many free bytes on the store's volume the owner tries no flush.
pub const FREE_SPACE_FLOOR_BYTES: u64 = 16_777_216;

/// A counted failure at this many due tries in a row sets the batch aside:
/// the same fault at each, or any of the eight codes of [`BatchCode`] at each.
pub const REJECT_AFTER_SAME_FAULTS: u32 = 3;

/// After this many batches set aside in a row, with no acknowledged write
/// between them, the owner sets no batch aside: it holds the next one as for
/// an I/O fault until a write is acknowledged. Batches given up as Refused or
/// Sealed neither count nor start the count again; an owner starts at 0.
pub const SET_ASIDES_BEFORE_STOP: u32 = 3;

/// The first wait before a failed write is tried again.
pub const RETRY_FIRST: Duration = Duration::from_secs(1);

/// The longest wait before a failed write is tried again.
pub const RETRY_MAX: Duration = Duration::from_secs(60);

/// The longest that one loop pass waits for an event.
pub const POLL: Duration = Duration::from_millis(50);

/// A command of the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Start discarding and answer the barrier `b`.
    OpenPause,
    /// Receive and discard what the sink holds, stop discarding and answer the
    /// barrier `r`; sources may start after it. An event whose emit began
    /// before the owner handled this command is discarded, whenever its send
    /// lands.
    ClosePause,
    /// Receive what the sink holds, flush, and answer the snapshot.
    Settle,
    /// Settle, answer the snapshot and end the loop.
    Shutdown,
}

/// The answer to a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Paused(Barrier),
    Resumed(Barrier),
    Settled(Snapshot),
    ShutDown(Snapshot),
}

/// Why the drain fails, with the tokens of the status text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainCause {
    NoSpace,
    Io,
    Db,
}

impl DrainCause {
    /// The token of the status text and of the loss line.
    pub const fn as_str(self) -> &'static str {
        match self {
            DrainCause::NoSpace => "no_space",
            DrainCause::Io => "io",
            DrainCause::Db => "db",
        }
    }
}

/// What the owner reports as it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report {
    /// Expiry ran at open; the owner takes events.
    StoreReady,
    DrainFailing {
        cause: DrainCause,
        since: SystemTime,
    },
    DrainHealthy,
    /// A batch was refused or sealed, or failed the same way at three due
    /// tries, and was dropped with its events counted as lost: their count,
    /// the first and the last capture instant in signed unix nanoseconds, and
    /// the fault of the last try.
    BatchRejected {
        events: u64,
        first_capture: i64,
        last_capture: i64,
        fault: WriteFault,
    },
    MaintenanceFailing {
        fault: MaintFault,
    },
    MaintenanceHealthy,
}

impl Report {
    /// The line that the daemon writes to its log for a dropped batch
    /// (contracts.md C11.3): `halvo-capture: dropped events=<n> first=<ts>
    /// last=<ts> error=<KIND>`, with the token of the batch's last fault;
    /// `None` for every other report.
    pub fn drop_line(&self) -> Option<String> {
        match *self {
            Report::BatchRejected {
                events,
                first_capture,
                last_capture,
                fault,
            } => Some(format!(
                "halvo-capture: dropped events={events} first={} last={} error={}",
                capture_second(Some(first_capture)),
                capture_second(Some(last_capture)),
                cause_of(fault).as_str()
            )),
            Report::StoreReady
            | Report::DrainFailing { .. }
            | Report::DrainHealthy
            | Report::MaintenanceFailing { .. }
            | Report::MaintenanceHealthy => None,
        }
    }
}

pub(crate) fn capture_second(at: Option<i64>) -> String {
    at.and_then(|nanos| UtcSecond::from_unix_seconds(nanos.div_euclid(1_000_000_000)).ok())
        .map_or_else(|| "-".to_owned(), |second| second.to_string())
}

/// The owner thread is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the store owner is gone")]
pub struct OwnerGone;

/// The owner thread panicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the store owner panicked")]
pub struct OwnerPanicked;

struct Asked {
    command: Command,
    reply: Sender<Answer>,
}

/// Sends commands to the owner. Clones reach the same owner.
#[derive(Debug, Clone)]
pub struct OwnerHandle {
    commands: Sender<Asked>,
}

/// A command sent and not yet answered.
#[derive(Debug)]
pub struct Pending {
    answer: Receiver<Answer>,
}

impl Pending {
    /// Waits for the answer.
    pub fn wait(self) -> Result<Answer, OwnerGone> {
        self.answer.recv().map_err(|_| OwnerGone)
    }

    /// The answer if the owner has given it.
    pub fn try_answer(&self) -> Option<Answer> {
        self.answer.try_recv().ok()
    }

    /// Waits for the answer at most `bound`; `None` when none came in it or
    /// the owner is gone.
    pub fn wait_for(&self, bound: Duration) -> Option<Answer> {
        self.answer.recv_timeout(bound).ok()
    }
}

impl OwnerHandle {
    /// Sends `command` and returns at once.
    pub fn ask(&self, command: Command) -> Result<Pending, OwnerGone> {
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(Asked { command, reply })
            .map_err(|_| OwnerGone)?;
        Ok(Pending { answer })
    }

    /// Sends `command` and waits for its answer.
    pub fn call(&self, command: Command) -> Result<Answer, OwnerGone> {
        self.ask(command)?.wait()
    }
}

/// What the owner is made of.
pub struct OwnerParts<S, C> {
    pub writer: SinkWriter,
    pub store: S,
    pub clocks: C,
    /// The exclusion decision that the owner applies to every received event.
    pub floor: FloorHandle,
    pub cap: StorageCap,
    /// The most segments of one shred call.
    pub shred_chunk: usize,
}

struct Spilled {
    kind: EntryKind,
    at: i64,
    record: SpilledRecord,
}

/// The copy of one spilled record, wiped when it drops.
struct SpilledRecord(Vec<u8>);

impl std::ops::Deref for SpilledRecord {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SpilledRecord {
    fn drop(&mut self) {
        #[cfg(test)]
        let (block, len) = (self.0.as_ptr(), self.0.len());
        self.0.zeroize();
        #[cfg(test)]
        super::batch::seen_at_free("spill", block, len);
    }
}

enum Drain {
    Healthy,
    Stopped {
        cause: DrainCause,
        since: SystemTime,
    },
    Failing {
        retry_at: Duration,
        delay: Duration,
        same: Option<(WriteFault, u32)>,
        cause: DrainCause,
        since: SystemTime,
    },
}

/// The owner, driven one pass at a time by [`spawn`]'s thread or by a test.
pub struct Owner<S, C> {
    writer: SinkWriter,
    commands: Receiver<Asked>,
    reports: Sender<Report>,
    store: S,
    clocks: C,
    floor: FloorHandle,
    filters: TextFilters,
    gate: Gate,
    batch: Batch,
    frozen: bool,
    spill: VecDeque<Spilled>,
    spill_bytes: usize,
    counters: Counters,
    drain: Drain,
    set_asides: u32,
    schedule: Schedule,
    maintenance_failing: bool,
    inbox: Option<Received>,
    stopping: bool,
    published: Arc<Mutex<Snapshot>>,
}

impl<S: CaptureStore, C: Clocks> Owner<S, C> {
    /// Opens the owner: reads the store's usage, runs expiry once and reports
    /// [`Report::StoreReady`]. It starts discarding.
    pub fn open(parts: OwnerParts<S, C>) -> (Owner<S, C>, OwnerHandle, Receiver<Report>) {
        let (commands_tx, commands) = mpsc::channel();
        let (reports, reports_rx) = mpsc::channel();
        let owner = Owner::open_with(parts, commands, reports);
        (
            owner,
            OwnerHandle {
                commands: commands_tx,
            },
            reports_rx,
        )
    }

    fn open_with(
        parts: OwnerParts<S, C>,
        commands: Receiver<Asked>,
        reports: Sender<Report>,
    ) -> Owner<S, C> {
        let schedule = Schedule::new(parts.shred_chunk, ByteBound::of_cap(parts.cap));
        let mut owner = Owner {
            writer: parts.writer,
            commands,
            reports,
            store: parts.store,
            clocks: parts.clocks,
            floor: parts.floor,
            filters: TextFilters::r1(),
            gate: Gate::closed(),
            batch: Batch::with_capacity(PAYLOAD_MAX_BYTES),
            frozen: false,
            spill: VecDeque::new(),
            spill_bytes: 0,
            counters: Counters::default(),
            drain: Drain::Healthy,
            set_asides: 0,
            schedule,
            maintenance_failing: false,
            inbox: None,
            stopping: false,
            published: Arc::new(Mutex::new(Counters::default().snapshot(0, false))),
        };
        owner.read_usage();
        let now = owner.clocks.now();
        owner.run_maintenance(now);
        owner.report(Report::StoreReady);
        owner.publish();
        owner
    }

    /// The handles of the counters that sources fill.
    pub fn tallies(&self) -> SourceTallies {
        self.counters.tallies().clone()
    }

    #[cfg(feature = "r2")]
    pub(crate) fn append_run_mark(&mut self, mark: crate::floor::record::RunMark) -> bool {
        if self.gate.is_discarding() {
            return false;
        }
        if self.frozen || !self.spill.is_empty() {
            let events = self.batch.events() + self.spill.len() + 1;
            let bytes = self.batch.record_bytes()
                + self.spill_bytes
                + crate::floor::record::LARGEST_RECORD_BYTES
                + 1;
            if events > HELD_MAX_EVENTS || bytes > HELD_MAX_BYTES {
                return false;
            }
        }
        let encoded = mark.encode(self.writer.number_next());
        let Ok(decoded) = record::decode(encoded.as_str()) else {
            return false;
        };
        self.counters.received(EntryKind::RUN_MARK);
        self.hold(
            EntryKind::RUN_MARK,
            decoded.at(),
            encoded.as_str().as_bytes(),
        );
        true
    }

    /// One loop pass: the waiting commands, then at most one event received
    /// within `wait`, then the due work. Returns false once the owner has
    /// answered [`Command::Shutdown`].
    pub fn pass(&mut self, wait: Duration) -> bool {
        self.receive_in_pass(wait);
        self.finish_pass()
    }

    fn receive_in_pass(&mut self, wait: Duration) {
        self.handle_commands();
        if !self.stopping {
            self.inbox = self.receive(wait);
        }
    }

    fn finish_pass(&mut self) -> bool {
        if !self.stopping {
            self.handle_commands();
            if let Some(received) = self.inbox.take() {
                self.take(received);
            }
        }
        if !self.stopping {
            let now = self.clocks.now();
            self.due_work(now);
            #[cfg(feature = "r2")]
            super::r2::owner_pass(self);
        }
        self.publish();
        !self.stopping
    }

    /// How long the next pass may wait for an event: [`POLL`] at most, less
    /// when a deadline comes sooner.
    pub fn wait(&self) -> Duration {
        let now = self.clocks.monotonic();
        let deadlines = [
            self.batch.deadline().filter(|_| !self.frozen),
            match self.drain {
                Drain::Failing { retry_at, .. } => Some(retry_at),
                Drain::Healthy | Drain::Stopped { .. } => None,
            },
            self.schedule.deadline().or(Some(now)),
        ];
        deadlines
            .into_iter()
            .flatten()
            .map(|deadline| deadline.saturating_sub(now))
            .fold(POLL, Duration::min)
    }

    /// The snapshot of this moment.
    pub fn snapshot(&self) -> Snapshot {
        self.counters.snapshot(self.pending(), self.stopped())
    }

    /// Runs the maintenance due at `now`: at most one shred call. It is the
    /// one entry of the maintenance schedule, and it takes the clocks'
    /// reading from its caller.
    pub fn run_maintenance(&mut self, now: Instants) {
        let Some(job) = self.schedule.next(now) else {
            return;
        };
        let answer = self.schedule.run(&mut self.store, job);
        self.schedule.done(job, &answer);
        match answer {
            Ok(pass) => {
                match job {
                    Job::Expire { .. } => {
                        self.counters.expired(now.wall);
                        if !pass.shredded.is_empty() {
                            self.read_usage();
                        }
                    }
                    Job::Evict => {
                        self.counters.evicted(now.wall);
                        self.read_usage();
                    }
                }
                if self.maintenance_failing && !self.schedule.is_failing() {
                    self.maintenance_failing = false;
                    self.report(Report::MaintenanceHealthy);
                }
            }
            Err(fault) => {
                self.maintenance_failing = true;
                self.report(Report::MaintenanceFailing { fault });
            }
        }
    }

    fn receive(&mut self, wait: Duration) -> Option<Received> {
        if wait.is_zero() {
            return self.writer.try_recv().ok();
        }
        match self.writer.recv_timeout(wait) {
            Ok(received) => Some(received),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                thread::sleep(wait);
                None
            }
        }
    }

    fn settle(&mut self) -> Snapshot {
        if self.frozen {
            let now = self.clocks.now();
            self.flush(now);
        }
        if let Some(received) = self.inbox.take() {
            self.take(received);
        }
        while let Ok(received) = self.writer.try_recv() {
            self.take(received);
        }
        let now = self.clocks.now();
        while !self.frozen && !(self.batch.is_empty() && self.spill.is_empty()) {
            self.flush(now);
        }
        self.publish();
        self.snapshot()
    }

    fn take(&mut self, received: Received) {
        self.counters
            .received(EntryKind::of(received.event().modality));
        if !self.gate.keeps(&received) {
            self.counters.discarded(DiscardReason::Barrier);
            return;
        }
        let received = match received.into_floor() {
            Ok(received) => received,
            Err(_) => {
                self.counters.discarded(DiscardReason::OldPath);
                return;
            }
        };
        if let Err(reason) = self.floor.recheck(received.event()) {
            self.counters.discarded(DiscardReason::Exclusion(reason));
            return;
        }
        let Ok(encoded) = record::encode(&received, &self.filters) else {
            let at = received.event().at();
            self.counters.lost(1, at, at);
            return;
        };
        let kind = EntryKind::of(received.event().kind());
        self.hold(kind, received.event().at(), encoded.as_str().as_bytes());
    }

    fn hold(&mut self, kind: EntryKind, at: i64, record: &[u8]) {
        let now = self.clocks.now();
        while !self.frozen && !self.spill.is_empty() {
            self.flush(now);
        }
        if self.frozen || !self.spill.is_empty() {
            self.spill_or_lose(kind, at, record);
            return;
        }
        let day = utc_day(at);
        match self.batch.admits(day, record.len()) {
            Admit::Fits => self.batch.push(kind, at, record, now.monotonic),
            Admit::TooLarge => self.counters.lost(1, at, at),
            Admit::FlushFirst => {
                self.flush(now);
                if self.frozen {
                    self.spill_or_lose(kind, at, record);
                } else {
                    self.hold(kind, at, record);
                }
            }
        }
    }

    fn spill_or_lose(&mut self, kind: EntryKind, at: i64, record: &[u8]) {
        let events = self.batch.events() + self.spill.len() + 1;
        let bytes = self.batch.record_bytes() + self.spill_bytes + record.len() + 1;
        if events > HELD_MAX_EVENTS || bytes > HELD_MAX_BYTES {
            self.counters.lost(1, at, at);
            return;
        }
        let mut copy = SpilledRecord(Vec::with_capacity(record.len()));
        copy.0.extend_from_slice(record);
        self.spill_bytes += record.len() + 1;
        self.spill.push_back(Spilled {
            kind,
            at,
            record: copy,
        });
    }

    fn pending(&self) -> u64 {
        (self.batch.events() + self.spill.len()) as u64
    }

    fn due_work(&mut self, now: Instants) {
        let retry_due = match self.drain {
            Drain::Failing { retry_at, .. } => now.monotonic >= retry_at,
            Drain::Healthy | Drain::Stopped { .. } => true,
        };
        if self.frozen && retry_due {
            self.flush(now);
        }
        while !self.frozen && (self.batch.is_due(now.monotonic) || self.refill_blocked()) {
            self.flush(now);
        }
        self.run_maintenance(now);
    }

    fn refill_blocked(&self) -> bool {
        self.spill.front().is_some_and(|next| {
            self.batch.admits(utc_day(next.at), next.record.len()) == Admit::FlushFirst
        })
    }

    fn refill(&mut self, now: Instants) {
        while let Some(next) = self.spill.front() {
            match self.batch.admits(utc_day(next.at), next.record.len()) {
                Admit::Fits => {
                    if let Some(next) = self.spill.pop_front() {
                        self.spill_bytes -= next.record.len() + 1;
                        self.batch
                            .push(next.kind, next.at, &next.record, now.monotonic);
                    }
                }
                Admit::FlushFirst => return,
                Admit::TooLarge => {
                    if let Some(next) = self.spill.pop_front() {
                        self.spill_bytes -= next.record.len() + 1;
                        self.counters.lost(1, next.at, next.at);
                    }
                }
            }
        }
    }

    fn flush(&mut self, now: Instants) {
        if self.batch.is_empty() {
            self.frozen = false;
            self.refill(now);
            if self.batch.is_empty() {
                return;
            }
        }
        match self.store.volume_free_bytes() {
            Ok(free) if free < FREE_SPACE_FLOOR_BYTES => {
                self.fail(WriteFault::Io(io::ErrorKind::StorageFull), now);
                return;
            }
            Ok(_) | Err(_) => {}
        }
        let segment_id = match self.batch.segment_id() {
            Ok(id) => id,
            Err(_) => {
                self.fail(WriteFault::Db(0), now);
                return;
            }
        };
        let answer = match self.batch.write(&segment_id) {
            Some(write) => self.store.write_batch(write),
            None => return,
        };
        match answer {
            Ok(written) => self.acknowledged(written.disk_bytes, now),
            Err(WriteFault::AlreadyWritten { .. }) => {
                self.acknowledged(0, now);
                self.read_usage();
            }
            Err(fault @ (WriteFault::Refused | WriteFault::Sealed)) => self.reject(fault, now),
            Err(fault) => self.fail(fault, now),
        }
    }

    fn acknowledged(&mut self, disk_bytes: u64, now: Instants) {
        self.counters
            .persisted(self.batch.events() as u64, disk_bytes, now.wall);
        self.schedule.flushed();
        self.set_asides = 0;
        self.heal();
        self.release(now);
    }

    fn stopped(&self) -> bool {
        self.set_asides >= SET_ASIDES_BEFORE_STOP
    }

    fn set_aside(
        &mut self,
        fault: WriteFault,
        cause: DrainCause,
        since: SystemTime,
        now: Instants,
    ) {
        if let Some(code) = batch_code(fault) {
            self.counters.set_aside(code);
        }
        self.set_asides += 1;
        if !self.stopped() {
            self.reject(fault, now);
            return;
        }
        self.lose(fault);
        self.drain = Drain::Stopped { cause, since };
        self.report(Report::DrainFailing { cause, since });
        self.release(now);
    }

    fn reject(&mut self, fault: WriteFault, now: Instants) {
        self.lose(fault);
        self.heal();
        self.release(now);
    }

    fn lose(&mut self, fault: WriteFault) {
        let events = self.batch.events() as u64;
        let (first_capture, last_capture) = self.batch.captures();
        self.counters.lost(events, first_capture, last_capture);
        self.report(Report::BatchRejected {
            events,
            first_capture,
            last_capture,
            fault,
        });
    }

    fn heal(&mut self) {
        if !matches!(self.drain, Drain::Healthy) {
            self.drain = Drain::Healthy;
            self.report(Report::DrainHealthy);
        }
    }

    fn release(&mut self, now: Instants) {
        self.batch.clear();
        self.frozen = false;
        self.refill(now);
    }

    fn fail(&mut self, fault: WriteFault, now: Instants) {
        self.frozen = true;
        let counted = counts_toward_tries(fault);
        let cause = cause_of(fault);
        let (retry_at, delay, same, since, changed) = match self.drain {
            Drain::Healthy => (
                now.monotonic + RETRY_FIRST,
                RETRY_FIRST,
                counted.then_some((fault, 1)),
                now.wall,
                true,
            ),
            Drain::Stopped {
                cause: before,
                since,
            } => (
                now.monotonic + RETRY_FIRST,
                RETRY_FIRST,
                counted.then_some((fault, 1)),
                since,
                before != cause,
            ),
            Drain::Failing {
                retry_at,
                delay,
                same,
                cause: before,
                since,
            } if now.monotonic >= retry_at => {
                let same = match same {
                    Some((last, count)) if counted && one_run(last, fault) => {
                        Some((fault, count + 1))
                    }
                    _ => counted.then_some((fault, 1)),
                };
                let delay = (delay * 2).min(RETRY_MAX);
                (now.monotonic + delay, delay, same, since, before != cause)
            }
            Drain::Failing {
                retry_at,
                delay,
                same,
                cause: before,
                since,
            } => {
                let same = same.filter(|(last, _)| one_run(*last, fault));
                (retry_at, delay, same, since, before != cause)
            }
        };
        if !self.stopped() && same.is_some_and(|(_, count)| count >= REJECT_AFTER_SAME_FAULTS) {
            self.set_aside(fault, cause, since, now);
            return;
        }
        if changed {
            self.report(Report::DrainFailing { cause, since });
        }
        self.drain = Drain::Failing {
            retry_at,
            delay,
            same,
            cause,
            since,
        };
    }

    fn read_usage(&mut self) {
        if let Ok(usage) = self.store.usage() {
            self.counters.disk_bytes(usage.disk_bytes);
        }
    }

    fn report(&self, report: Report) {
        let _ = self.reports.send(report);
    }

    fn publish(&self) {
        let snapshot = self.snapshot();
        *self
            .published
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = snapshot;
    }

    #[cfg(test)]
    pub(crate) fn payload_block(&self) -> (*const u8, usize) {
        self.batch.payload_block()
    }

    #[cfg(test)]
    pub(crate) fn payload_bytes(&self) -> Vec<u8> {
        self.batch.payload_whole()
    }
}

/// The primary codes of the counted class at a write whose due tries count
/// while the same fault, by the whole code, ends each of them: those of the
/// open's sealed row of contracts.md C11.3, corrupt (11) and not a database
/// (26), and 0, the code of a failed mint of the segment id. The eight codes
/// of [`BatchCode`] are counted too, by their primary code. Every other
/// primary code, 13 (no space), I/O (10), busy (5) and locked (6) among them
/// and one that this build does not know, is of the class of Io: held, never
/// counted, kind io (ruling PR-30 PR4; ruling PR-31 item 3; ruling PR-32 with
/// its Addition 1). Nothing holds these lists in step with the store's own
/// table.
const DB_CODES_COUNTED: [i32; 3] = [11, 26, 0];

/// Whether a write's fault counts toward the due tries of
/// REJECT_AFTER_SAME_FAULTS.
fn counts_toward_tries(fault: WriteFault) -> bool {
    match fault {
        WriteFault::Db(code) if BatchCode::of(code).is_some() => true,
        WriteFault::Db(code) => DB_CODES_COUNTED.contains(&(code & 0xff)),
        WriteFault::Refused | WriteFault::Sealed => true,
        WriteFault::Io(_) | WriteFault::AlreadyWritten { .. } => false,
    }
}

fn batch_code(fault: WriteFault) -> Option<BatchCode> {
    match fault {
        WriteFault::Db(code) => BatchCode::of(code),
        WriteFault::Io(_)
        | WriteFault::AlreadyWritten { .. }
        | WriteFault::Refused
        | WriteFault::Sealed => None,
    }
}

fn one_run(last: WriteFault, fault: WriteFault) -> bool {
    last == fault || (batch_code(last).is_some() && batch_code(fault).is_some())
}

fn cause_of(fault: WriteFault) -> DrainCause {
    if fault.is_no_space() {
        return DrainCause::NoSpace;
    }
    match fault {
        WriteFault::Io(_) => DrainCause::Io,
        WriteFault::Db(_) if !counts_toward_tries(fault) => DrainCause::Io,
        WriteFault::AlreadyWritten { .. }
        | WriteFault::Refused
        | WriteFault::Db(_)
        | WriteFault::Sealed => DrainCause::Db,
    }
}

/// The running owner thread.
#[derive(Debug)]
pub struct OwnerThread {
    pub handle: OwnerHandle,
    pub reports: Receiver<Report>,
    tallies: SourceTallies,
    published: Arc<Mutex<Snapshot>>,
    thread: JoinHandle<Snapshot>,
}

impl OwnerThread {
    /// The snapshot that the owner published at the end of its last pass.
    pub fn snapshot(&self) -> Snapshot {
        *self
            .published
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub fn tallies(&self) -> SourceTallies {
        self.tallies.clone()
    }

    pub fn is_running(&self) -> bool {
        !self.thread.is_finished()
    }

    /// Waits for the thread to end and returns its last snapshot.
    pub fn join(self) -> Result<Snapshot, OwnerPanicked> {
        self.thread.join().map_err(|_| OwnerPanicked)
    }
}

/// Starts the owner on a thread of its own. The owner opens there, so expiry
/// at open runs on that thread before [`Report::StoreReady`].
pub fn spawn<S, C>(parts: OwnerParts<S, C>) -> io::Result<OwnerThread>
where
    S: CaptureStore + Send + 'static,
    C: Clocks + 'static,
{
    let (commands_tx, commands) = mpsc::channel();
    let (reports_tx, reports) = mpsc::channel();
    let (opened_tx, opened) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("halvo-store-owner".to_owned())
        .spawn(move || {
            let mut owner = Owner::open_with(parts, commands, reports_tx);
            let _ = opened_tx.send((owner.tallies(), owner.published.clone()));
            while owner.pass(owner.wait()) {}
            owner.snapshot()
        })?;
    let (tallies, published) = opened
        .recv()
        .map_err(|_| io::Error::other("the store owner ended while it opened"))?;
    Ok(OwnerThread {
        handle: OwnerHandle {
            commands: commands_tx,
        },
        reports,
        tallies,
        published,
        thread,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::daemon::maintenance::{Clocks, SHRED_CHUNK};
    use crate::daemon::model_store::{
        floor_rooted_at_the_test_app, foreground, foreground_now, frame, key, noon, open_floor,
        title_of, Call, Harness, ManualClocks, ModelStore, WriteScript, AT_NOON,
    };
    use crate::event::Modality;
    use crate::floor::{Switch, Switches};
    use crate::sink;
    use halvo_control::proto::{StoreErrorKind, StoreState};

    use super::open::{try_open, CreateRule, Held, Opener};
    use crate::daemon::custody_policy::{self, Custody, Selection};
    use crate::daemon::port::{Access, OpenFault, Opened, Probe, StoreHost, StoreId};
    use crate::daemon::slot::Slot;

    fn resumed(harness: &mut Harness) -> Barrier {
        match harness.ask(Command::ClosePause) {
            Answer::Resumed(barrier) => barrier,
            other => panic!("{other:?}"),
        }
    }

    fn settled(harness: &mut Harness) -> Snapshot {
        match harness.ask(Command::Settle) {
            Answer::Settled(snapshot) => snapshot,
            other => panic!("{other:?}"),
        }
    }

    fn eventually(what: &str, mut done: impl FnMut() -> bool) {
        for _ in 0..2000 {
            if done() {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("{what} did not happen within 10 s");
    }

    #[test]
    fn waiting_commands_are_handled_before_the_next_received_event() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        harness.emit(foreground_now());
        harness.pass();
        harness.emit(foreground_now());
        let pending = harness.handle.ask(Command::OpenPause).unwrap();
        harness.pass();
        let Some(Answer::Paused(_)) = pending.try_answer() else {
            panic!("the pause was not answered in the pass");
        };
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.events_received(), 2);
        assert_eq!(
            snapshot.events_persisted, 1,
            "an event waiting behind the pause was kept"
        );
        assert_eq!(snapshot.discarded_by_barrier, 1);
    }

    #[test]
    fn an_emit_that_began_inside_the_pause_is_discarded_whenever_its_send_lands() {
        let mut harness = Harness::new();
        let (marked_tx, marked_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let sink = harness.sink.clone();
        let event = key();
        let id = event.id().uuid();
        let emitter = thread::spawn(move || {
            sink.emit_held(event, move || {
                marked_tx.send(()).unwrap();
                go_rx.recv().unwrap();
            })
        });
        marked_rx.recv().unwrap();
        let r = resumed(&mut harness);
        go_tx.send(()).unwrap();
        assert_eq!(emitter.join().unwrap(), sink::Emitted::Queued);
        harness.pass();
        let snapshot = settled(&mut harness);
        assert!(
            !harness
                .store
                .records()
                .iter()
                .any(|record| record.id() == id),
            "an emit that began inside the pause and landed after r was stored"
        );
        assert_eq!(snapshot.events_received(), 1);
        assert_eq!(snapshot.discarded_by_barrier, 1);
        harness.emit(key());
        harness.pass();
        let after = settled(&mut harness);
        assert_eq!(
            after.events_persisted, 1,
            "an emit after the resume was not kept"
        );
        let seqs: Vec<u64> = harness
            .store
            .records()
            .iter()
            .map(|record| record.seq())
            .collect();
        assert_eq!(seqs, [1]);
        assert_ne!(r, resumed(&mut harness));
    }

    #[test]
    fn the_owner_starts_discarding() {
        let mut harness = Harness::new();
        harness.emit(foreground_now());
        harness.pass();
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.discarded_by_barrier, 1);
        assert!(harness.store.batches().is_empty());
    }

    #[test]
    fn a_pause_drawn_after_510_events_and_two_more_keeps_nothing_at_or_after_b() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        for _ in 0..510 {
            harness.emit(foreground_now());
            harness.pass();
        }
        harness.emit(foreground_now());
        harness.emit(foreground_now());
        let b = match harness.ask(Command::OpenPause) {
            Answer::Paused(b) => b,
            other => panic!("{other:?}"),
        };
        harness.emit(foreground_now());
        settled(&mut harness);
        let seqs: Vec<u64> = harness.store.records().iter().map(|r| r.seq()).collect();
        assert_eq!(seqs.len(), 510);
        assert_eq!(*seqs.last().unwrap(), 509);
        let r = resumed(&mut harness);
        harness.emit(foreground_now());
        harness.pass();
        let snapshot = settled(&mut harness);
        let seqs: Vec<u64> = harness.store.records().iter().map(|r| r.seq()).collect();
        assert_eq!(
            seqs.len(),
            511,
            "the first event after the resume was not kept"
        );
        assert_eq!(*seqs.last().unwrap(), 513);
        assert_ne!(b, r);
        assert_eq!(snapshot.discarded_by_barrier, 3);
        assert!(snapshot.is_conserved());
    }

    #[test]
    fn a_rejected_batch_leaves_no_partial_acknowledgement() {
        let mut harness = Harness::new();
        harness
            .store
            .script_writes([WriteScript::Fail(WriteFault::Io(io::ErrorKind::Other))]);
        resumed(&mut harness);
        for _ in 0..3 {
            harness.emit(foreground_now());
        }
        let snapshot = settled(&mut harness);
        assert!(
            harness.store.records().is_empty(),
            "a record of a failed batch is readable"
        );
        assert_eq!(snapshot.events_pending, 3);
        assert_eq!(snapshot.events_persisted, 0);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        let writes = harness.store.write_calls();
        assert_eq!(writes.len(), 2);
        assert_eq!(
            writes[0].0, writes[1].0,
            "the retry carried another segment id"
        );
        assert_eq!(
            harness.store.records().len(),
            3,
            "the same events were not offered again"
        );
        assert_eq!(harness.owner.snapshot().events_persisted, 3);
    }

    #[test]
    fn a_retry_after_a_lost_answer_is_acknowledged_once() {
        let mut harness = Harness::new();
        harness
            .store
            .script_writes([WriteScript::KeepThenFail(WriteFault::Io(
                io::ErrorKind::Other,
            ))]);
        resumed(&mut harness);
        for _ in 0..4 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        let writes = harness.store.write_calls();
        assert_eq!(writes.len(), 2);
        assert_eq!(
            writes[0].0, writes[1].0,
            "the retry carried another segment id"
        );
        assert!(matches!(
            writes[1].1,
            Err(WriteFault::AlreadyWritten { .. })
        ));
        assert_eq!(
            harness.store.batches().len(),
            1,
            "the batch was stored twice"
        );
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.events_persisted, 4);
        assert_eq!(snapshot.segments_written, 1);
        assert_eq!(snapshot.events_pending, 0);
    }

    #[test]
    fn two_failed_writes_then_recovery_persists_every_event() {
        let store = ModelStore::new();
        let io = WriteFault::Io(io::ErrorKind::Other);
        store.script_writes([WriteScript::Fail(io), WriteScript::Fail(io)]);
        let clocks = ManualClocks::at(noon());
        let (sink, writer) = sink::channel(2048);
        let owner = spawn(OwnerParts {
            writer,
            store: store.clone(),
            clocks: clocks.clone(),
            floor: open_floor(),
            cap: StorageCap::DEFAULT,
            shred_chunk: SHRED_CHUNK,
        })
        .unwrap();
        owner.handle.call(Command::ClosePause).unwrap();
        for _ in 0..20 {
            sink.emit_event(foreground_now());
        }
        let Answer::Settled(first) = owner.handle.call(Command::Settle).unwrap() else {
            panic!("no settle answer");
        };
        assert_eq!(first.events_pending, 20);
        clocks.advance(RETRY_FIRST);
        eventually("the second write", || store.write_calls().len() >= 2);
        assert!(owner.is_running());
        clocks.advance(2 * RETRY_FIRST);
        eventually("the third write", || store.write_calls().len() >= 3);
        let Answer::Settled(last) = owner.handle.call(Command::Settle).unwrap() else {
            panic!("no settle answer");
        };
        assert_eq!(last.events_persisted, 20);
        assert!(last.is_conserved());
        let reports: Vec<Report> = owner.reports.try_iter().collect();
        assert_eq!(reports[0], Report::StoreReady);
        assert!(matches!(
            reports[1],
            Report::DrainFailing {
                cause: DrainCause::Io,
                ..
            }
        ));
        assert_eq!(reports[2], Report::DrainHealthy);
        assert!(owner.is_running(), "the owner thread ended");
        let Answer::ShutDown(_) = owner.handle.call(Command::Shutdown).unwrap() else {
            panic!("no shutdown answer");
        };
        assert_eq!(owner.join().unwrap().events_persisted, 20);
    }

    #[test]
    fn the_retry_waits_from_1_s_and_doubles_to_60_s() {
        let mut harness = Harness::new();
        harness
            .store
            .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        resumed(&mut harness);
        harness.emit(foreground_now());
        settled(&mut harness);
        let start = harness.clocks.monotonic();
        let mut writes_at = Vec::new();
        let mut seen = harness.store.write_calls().len();
        for _ in 0..300 {
            harness.clocks.advance(Duration::from_secs(1));
            harness.pass();
            let now = harness.store.write_calls().len();
            if now > seen {
                seen = now;
                writes_at.push((harness.clocks.monotonic() - start).as_secs());
            }
        }
        assert_eq!(writes_at, [1, 3, 7, 15, 31, 63, 123, 183, 243]);
        assert_eq!(
            harness.owner.snapshot().events_lost,
            0,
            "an I/O fault dropped the batch"
        );
    }

    #[test]
    fn the_class_of_each_write_fault_counts_and_names_it() {
        let table = [
            (WriteFault::Db(5), false, DrainCause::Io),
            (WriteFault::Db(6), false, DrainCause::Io),
            (WriteFault::Db(10), false, DrainCause::Io),
            (WriteFault::Db(266), false, DrainCause::Io),
            (WriteFault::Db(11), true, DrainCause::Db),
            (WriteFault::Db(267), true, DrainCause::Db),
            (WriteFault::Db(26), true, DrainCause::Db),
            (WriteFault::Db(0), true, DrainCause::Db),
            (WriteFault::Db(13), false, DrainCause::NoSpace),
            (WriteFault::Io(io::ErrorKind::Other), false, DrainCause::Io),
            (WriteFault::Refused, true, DrainCause::Db),
            (WriteFault::Sealed, true, DrainCause::Db),
        ];
        for (fault, counted, cause) in table {
            assert_eq!(
                (counts_toward_tries(fault), cause_of(fault)),
                (counted, cause),
                "{fault:?}"
            );
        }
        let of_the_io_class = [3, 4, 7, 8, 9, 14, 15, 16, 17, 22, 23, 24, 27, 28, 99];
        for code in of_the_io_class {
            let fault = WriteFault::Db(code);
            assert_eq!(
                (counts_toward_tries(fault), cause_of(fault)),
                (false, DrainCause::Io),
                "{fault:?}"
            );
        }
        let bound_to_the_batch_read_again_by_ruling_pr_31 = [1, 2, 12, 18, 19, 20, 21, 25];
        for code in bound_to_the_batch_read_again_by_ruling_pr_31 {
            for fault in [WriteFault::Db(code), WriteFault::Db(code | (1 << 8))] {
                assert_eq!(
                    (counts_toward_tries(fault), cause_of(fault)),
                    (true, DrainCause::Db),
                    "{fault:?}"
                );
            }
        }
    }

    /// Ruling DM-15 (2): a stand-in owner answers the settle's first ask with
    /// 4 events pending, and its command channel is gone before that answer
    /// is sent, so the second ask cannot be sent; the settle answers the first
    /// snapshot and not `None`, which would read as an unanswered settle.
    #[test]
    fn a_settle_whose_owner_is_gone_before_its_second_ask_keeps_the_first_answer() {
        let (commands, inbox) = mpsc::channel();
        let handle = OwnerHandle { commands };
        let stand_in = thread::spawn(move || {
            let Asked { command, reply } = inbox.recv().expect("no first ask");
            assert!(matches!(command, Command::Settle), "{command:?}");
            drop(inbox);
            reply
                .send(Answer::Settled(Counters::default().snapshot(4, false)))
                .expect("the settle stopped waiting");
        });
        let settled = crate::daemon::settle_within(
            &mut crate::daemon::RealWaits,
            &handle,
            Duration::from_secs(5),
        );
        stand_in.join().expect("the stand-in owner panicked");
        assert_eq!(
            settled.map(|snapshot| snapshot.events_pending),
            Some(4),
            "the first answer was forgotten when the second ask could not be sent"
        );
    }

    #[test]
    fn three_due_tries_answered_busy_leave_the_batch_held() {
        let mut harness = Harness::new();
        let busy = WriteFault::Db(5);
        harness.store.script_writes([
            WriteScript::Fail(busy),
            WriteScript::Fail(busy),
            WriteScript::Fail(busy),
        ]);
        resumed(&mut harness);
        for _ in 0..5 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        assert_eq!(harness.store.write_calls().len(), 3);
        let snapshot = harness.owner.snapshot();
        assert_eq!(
            snapshot.events_lost, 0,
            "three busy answers dropped the batch"
        );
        assert_eq!(snapshot.events_pending, 5);
        assert!(rejected(&harness).is_empty());
    }

    #[test]
    fn a_batch_rejected_three_times_is_dropped_and_counted() {
        let mut harness = Harness::new();
        let db = WriteFault::Db(11);
        harness.store.script_writes([
            WriteScript::Fail(db),
            WriteScript::Fail(db),
            WriteScript::Fail(db),
        ]);
        resumed(&mut harness);
        for _ in 0..5 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        assert_eq!(
            harness.owner.snapshot().events_lost,
            0,
            "dropped after two failures"
        );
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        let snapshot = harness.owner.snapshot();
        assert_eq!(snapshot.events_lost, 5);
        assert_eq!(snapshot.events_pending, 0);
        assert!(harness.reports().iter().any(|report| matches!(
            report,
            Report::BatchRejected { events: 5, fault, .. } if *fault == db
        )));
        harness.emit(foreground_now());
        let after = settled(&mut harness);
        assert_eq!(
            after.events_persisted, 1,
            "the owner did not resume after the drop"
        );
        assert!(after.is_conserved());
    }

    #[test]
    fn a_refused_batch_is_dropped_at_once() {
        let mut harness = Harness::new();
        harness
            .store
            .script_writes([WriteScript::Fail(WriteFault::Refused)]);
        resumed(&mut harness);
        harness.emit(foreground_now());
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.events_lost, 1);
        assert_eq!(snapshot.events_pending, 0);
    }

    #[test]
    fn a_failing_maintenance_pass_leaves_the_owner_alive() {
        let store = ModelStore::new();
        store.script_expire(vec![
            Ok(super::super::port::Pass::default()),
            Err(MaintFault::CheckpointIncomplete),
        ]);
        let mut harness = Harness::over(store, open_floor(), 2048);
        resumed(&mut harness);
        harness.clocks.advance(Duration::from_secs(61));
        harness.pass();
        assert!(harness.reports().contains(&Report::MaintenanceFailing {
            fault: MaintFault::CheckpointIncomplete
        }));
        let calls = harness.store.expire_calls().len();
        harness.clocks.advance(Duration::from_secs(30));
        harness.pass();
        assert_eq!(
            harness.store.expire_calls().len(),
            calls,
            "retried before the deadline"
        );
        harness.clocks.advance(Duration::from_secs(31));
        harness.pass();
        assert_eq!(harness.store.expire_calls().len(), calls + 1);
        assert!(harness.reports().contains(&Report::MaintenanceHealthy));
        harness.emit(foreground_now());
        assert_eq!(settled(&mut harness).events_persisted, 1);
    }

    #[test]
    fn under_the_floor_no_flush_is_tried_and_maintenance_still_runs() {
        let mut harness = Harness::new();
        harness.store.set_free_bytes(Ok(FREE_SPACE_FLOOR_BYTES - 1));
        resumed(&mut harness);
        for _ in 0..3 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        let expiries = harness.store.expire_calls().len();
        for _ in 0..70 {
            harness.clocks.advance(Duration::from_secs(1));
            harness.pass();
        }
        assert!(
            harness.store.write_calls().is_empty(),
            "a flush was tried under the floor"
        );
        assert!(
            harness.store.expire_calls().len() > expiries,
            "expiry stopped under the floor"
        );
        assert!(harness.reports().iter().any(|report| matches!(
            report,
            Report::DrainFailing {
                cause: DrainCause::NoSpace,
                ..
            }
        )));
        harness.store.set_free_bytes(Ok(FREE_SPACE_FLOOR_BYTES));
        harness.clocks.advance(RETRY_MAX);
        harness.pass();
        assert_eq!(harness.owner.snapshot().events_persisted, 3);
        let disk_full = WriteFault::Io(io::ErrorKind::StorageFull);
        assert_eq!(cause_of(disk_full), DrainCause::NoSpace);
        assert_eq!(cause_of(WriteFault::Db(13)), DrainCause::NoSpace);
    }

    #[test]
    fn the_spill_stops_at_its_bounds() {
        let mut harness = Harness::over(ModelStore::new(), open_floor(), 64);
        harness
            .store
            .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        resumed(&mut harness);
        for _ in 0..3000 {
            harness.emit(foreground_now());
            harness.pass();
        }
        let snapshot = harness.owner.snapshot();
        assert_eq!(snapshot.events_pending, HELD_MAX_EVENTS as u64);
        assert_eq!(snapshot.events_lost, 3000 - HELD_MAX_EVENTS as u64);
        assert!(snapshot.is_conserved());

        let mut harness = Harness::over(ModelStore::new(), open_floor(), 64);
        harness
            .store
            .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        resumed(&mut harness);
        for _ in 0..1500 {
            harness.emit(title_of(1024, 1024));
            harness.pass();
        }
        let snapshot = harness.owner.snapshot();
        let held = harness.owner.batch.record_bytes() + harness.owner.spill_bytes;
        assert!(held <= HELD_MAX_BYTES, "{held} bytes held");
        assert!(
            held + 2200 > HELD_MAX_BYTES,
            "the bytes bound stopped early at {held}"
        );
        assert!(snapshot.events_pending < HELD_MAX_EVENTS as u64);
        assert_eq!(snapshot.events_lost, 1500 - snapshot.events_pending);
        assert!(snapshot.is_conserved());
        harness.store.fail_every_write(None);
        harness.clocks.advance(RETRY_MAX);
        harness.pass();
        let recovered = settled(&mut harness);
        assert_eq!(recovered.events_persisted, snapshot.events_pending);
        assert_eq!(recovered.events_pending, 0);
        assert_eq!(HELD_MAX_EVENTS, 2_560);
        assert_eq!(HELD_MAX_BYTES, 2_097_152);
    }

    #[test]
    fn the_held_count_stays_2560_whatever_the_depth_of_the_queue() {
        assert_eq!(
            HELD_MAX_EVENTS, 2_560,
            "the held count follows the queue's {QUEUE_SLOTS} slots"
        );
    }

    #[test]
    fn a_recovered_drain_writes_its_spill_before_it_spills_more() {
        let mut harness = Harness::over(ModelStore::new(), open_floor(), 2048);
        harness
            .store
            .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        resumed(&mut harness);
        for _ in 0..2500 {
            harness.emit(foreground_now());
            harness.pass();
        }
        assert_eq!(harness.owner.snapshot().events_pending, 2500);
        harness.store.fail_every_write(None);
        for _ in 0..2000 {
            harness.emit(foreground_now());
        }
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.events_lost, 0, "a recovered drain lost events");
        assert_eq!(snapshot.events_persisted, 4500);
        assert!(snapshot.is_conserved());
    }

    #[test]
    fn a_spilled_record_is_wiped_before_it_is_given_back() {
        let mut harness = Harness::over(ModelStore::new(), open_floor(), 2048);
        harness
            .store
            .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        resumed(&mut harness);
        for _ in 0..600 {
            harness.emit(foreground_now());
            harness.pass();
        }
        let spilled = harness.owner.spill.len();
        assert_eq!(spilled, 600 - crate::daemon::batch::BATCH_MAX_EVENTS);
        harness.store.fail_every_write(None);
        let seen = crate::daemon::batch::watch_wipes(|| {
            settled(&mut harness);
        });
        let spill: Vec<bool> = seen
            .iter()
            .filter(|(what, _)| *what == "spill")
            .map(|(_, zero)| *zero)
            .collect();
        assert_eq!(
            spill.len(),
            spilled,
            "a spilled record was given back unseen"
        );
        assert!(
            spill.iter().all(|zero| *zero),
            "a spilled record was given back unwiped"
        );
        assert_eq!(harness.owner.snapshot().events_persisted, 600);
    }

    #[test]
    fn dropping_every_handle_settles_the_held_events_and_ends_the_loop() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        for _ in 0..3 {
            harness.emit(foreground_now());
            harness.pass();
        }
        assert!(harness.store.records().is_empty());
        let Harness {
            mut owner,
            handle,
            store,
            ..
        } = harness;
        drop(handle);
        assert!(
            !owner.pass(Duration::ZERO),
            "the loop went on with no handle"
        );
        assert_eq!(store.records().len(), 3, "the held events were not settled");
    }

    #[test]
    fn three_faults_that_differ_drop_no_batch() {
        let mut harness = Harness::new();
        harness.store.script_writes([
            WriteScript::Fail(WriteFault::Db(11)),
            WriteScript::Fail(WriteFault::Db(5)),
            WriteScript::Fail(WriteFault::Db(11)),
        ]);
        resumed(&mut harness);
        for _ in 0..4 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        assert_eq!(harness.store.write_calls().len(), 3);
        let snapshot = harness.owner.snapshot();
        assert_eq!(
            snapshot.events_lost, 0,
            "three faults that differ dropped the batch"
        );
        assert_eq!(snapshot.events_pending, 4);
    }

    /// Ruling DM-16 (1) b, the review's B2 (its probe r38-two-counted-faults):
    /// three due tries answered by two counted codes that differ, corrupt
    /// (11), not a database (26) and corrupt (11), are not the same fault
    /// three times, so the batch stays held and nothing is lost.
    #[test]
    fn three_due_tries_with_two_counted_codes_drop_no_batch() {
        let mut harness = Harness::new();
        harness.store.script_writes([
            WriteScript::Fail(WriteFault::Db(11)),
            WriteScript::Fail(WriteFault::Db(26)),
            WriteScript::Fail(WriteFault::Db(11)),
        ]);
        resumed(&mut harness);
        for _ in 0..4 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        assert_eq!(harness.store.write_calls().len(), 3);
        let snapshot = harness.owner.snapshot();
        assert_eq!(
            snapshot.events_lost, 0,
            "two counted codes that differ dropped the batch"
        );
        assert_eq!(snapshot.events_pending, 4);
    }

    #[test]
    fn three_writes_answered_database_full_drop_no_batch_and_say_no_space() {
        let mut harness = Harness::new();
        harness.store.fail_every_write(Some(WriteFault::Db(13)));
        resumed(&mut harness);
        for _ in 0..4 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        assert_eq!(harness.store.write_calls().len(), 3);
        let snapshot = harness.owner.snapshot();
        assert_eq!(snapshot.events_lost, 0, "a full database dropped the batch");
        assert_eq!(snapshot.events_pending, 4);
        assert!(harness.reports().iter().any(|report| matches!(
            report,
            Report::DrainFailing {
                cause: DrainCause::NoSpace,
                ..
            }
        )));
    }

    #[test]
    fn an_owner_over_the_discard_port_flushes_its_batch() {
        let (sink, writer) = sink::channel(64);
        let (mut owner, handle, _reports) = Owner::open(OwnerParts {
            writer,
            store: crate::daemon::discard_port::DiscardPort::new(std::path::PathBuf::from(env!(
                "CARGO_MANIFEST_DIR"
            ))),
            clocks: ManualClocks::at(noon()),
            floor: open_floor(),
            cap: StorageCap::DEFAULT,
            shred_chunk: SHRED_CHUNK,
        });
        let ask = |owner: &mut Owner<_, _>, command| {
            let pending = handle.ask(command).unwrap();
            owner.pass(Duration::ZERO);
            pending.try_answer().unwrap()
        };
        ask(&mut owner, Command::ClosePause);
        assert_eq!(sink.emit_event(foreground_now()), sink::Emitted::Queued);
        let Answer::Settled(snapshot) = ask(&mut owner, Command::Settle) else {
            panic!("no settle answer");
        };
        assert_eq!(snapshot.events_persisted, 1);
        assert_eq!(snapshot.segments_written, 1);
    }

    #[test]
    fn the_disk_total_adds_each_write_and_reads_the_usage_after_an_eviction() {
        let mut harness = Harness::over(ModelStore::new(), open_floor(), 2048);
        resumed(&mut harness);
        for _ in 0..600 {
            harness.emit(foreground_now());
        }
        let settled = settled(&mut harness);
        let batches = harness.store.batches();
        assert_eq!(batches.len(), 2);
        let written: u64 = batches
            .iter()
            .map(|batch| (batch.payload.len() as u64).div_ceil(4096) * 4096)
            .sum();
        assert_eq!(
            settled.disk_bytes,
            Some(written),
            "the disk total is not the sum of the writes"
        );
        harness.pass();
        let usage: u64 = batches.iter().map(|batch| batch.payload.len() as u64).sum();
        assert!(harness
            .store
            .log()
            .iter()
            .any(|call| matches!(call, Call::EvictTo { .. })));
        assert_eq!(
            harness.owner.snapshot().disk_bytes,
            Some(usage),
            "the disk total is not the usage after an eviction"
        );
    }

    #[test]
    fn last_expiry_at_is_the_wall_time_of_a_good_expiry_after_a_step() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        harness.clocks.advance(Duration::from_secs(10));
        harness.pass();
        harness.clocks.step_wall_back(Duration::from_secs(3600));
        let calls = harness.store.expire_calls().len();
        harness.clocks.advance(Duration::from_secs(1));
        harness.pass();
        assert_eq!(harness.store.expire_calls().len(), calls + 1);
        assert_eq!(
            harness.owner.snapshot().last_expiry_at,
            Some(harness.clocks.wall())
        );
    }

    #[test]
    fn an_event_of_a_listed_app_never_reaches_a_batch() {
        let mut harness = Harness::over(ModelStore::new(), floor_rooted_at_the_test_app(), 64);
        resumed(&mut harness);
        harness.emit(foreground_now());
        harness.emit(key());
        let snapshot = settled(&mut harness);
        assert!(
            harness.store.batches().is_empty(),
            "an excluded event reached a batch"
        );
        assert_eq!(snapshot.discarded_by_exclusion, 2);

        let mut switches = Switches::default();
        switches.set(Switch::KeyClass, false);
        let floor = FloorHandle::new(None, Arc::new(crate::floor::exclude::NoLineage), switches);
        let mut harness = Harness::over(ModelStore::new(), floor, 64);
        resumed(&mut harness);
        harness.emit(key());
        harness.emit(frame());
        harness.emit(foreground_now());
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.discarded_by_exclusion, 2);
        let kinds: Vec<_> = harness.store.records().iter().map(|r| r.kind()).collect();
        assert_eq!(kinds, ["foreground_app"]);
    }

    #[test]
    fn an_event_of_the_old_emit_is_discarded_and_counted() {
        use crate::source::EventSink;
        let mut harness = Harness::new();
        resumed(&mut harness);
        let old = crate::event::CaptureStreamEvent::new_raw(
            crate::event::CapturedAt {
                wall_clock: noon(),
                sequence: harness.sink.next_sequence(),
            },
            Modality::Keystroke,
            String::new(),
            crate::event::SegmentRef {
                segment_id: uuid::Uuid::nil(),
                byte_offset: 0,
                byte_len: 0,
            },
            crate::event::SegmentKeyId(0),
        );
        harness.sink.emit(old).unwrap();
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.discarded_old_path, 1);
        assert_eq!(snapshot.received(Modality::Keystroke), 1);
        assert!(harness.store.batches().is_empty());
    }

    #[test]
    fn eviction_runs_after_each_flush_and_again_while_work_is_owed() {
        let store = ModelStore::new();
        store.script_evict(vec![
            Ok(super::super::port::Pass {
                shredded: Vec::new(),
                more_due: false,
                owed: true,
            }),
            Ok(super::super::port::Pass::default()),
        ]);
        let mut harness = Harness::over(store, open_floor(), 64);
        resumed(&mut harness);
        harness.emit(foreground(&AT_NOON, Duration::ZERO));
        settled(&mut harness);
        harness.passes(3);
        let evicts: Vec<_> = harness
            .store
            .log()
            .into_iter()
            .filter(|call| matches!(call, Call::EvictTo { .. }))
            .collect();
        assert_eq!(evicts.len(), 2);
        assert_eq!(
            evicts[0],
            Call::EvictTo {
                bound: ByteBound::of_cap(StorageCap::DEFAULT),
                max: SHRED_CHUNK
            }
        );
    }

    mod taken {
        use std::io;
        use std::time::Duration;

        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;
        use std::time::SystemTime;

        use halvo_control::model::StorageCap;

        use crate::daemon::counters::Snapshot;
        use crate::daemon::maintenance::{
            Clocks, CLOCK_STEP_GRACE, MAINTENANCE_INTERVAL, SHRED_CHUNK,
        };
        use crate::daemon::model_store::{
            foreground, foreground_now, noon, open_floor, Call, Harness, ManualClocks, ModelStore,
            WriteScript, AT_NOON,
        };
        use crate::daemon::owner::{
            Answer, Command, DrainCause, Owner, OwnerParts, Report, FREE_SPACE_FLOOR_BYTES,
        };
        use crate::daemon::port::{
            ByteBound, CaptureStore, MaintFault, MaintainStore, Ordinal, Pass, SegmentWrite, Usage,
            WriteFault, Written,
        };
        use crate::sink;

        const SECOND: Duration = Duration::from_secs(1);

        fn resumed(harness: &mut Harness) {
            assert!(matches!(
                harness.ask(Command::ClosePause),
                Answer::Resumed(_)
            ));
        }

        fn settled(harness: &mut Harness) -> Snapshot {
            match harness.ask(Command::Settle) {
                Answer::Settled(snapshot) => snapshot,
                other => panic!("{other:?}"),
            }
        }

        fn evict_calls(harness: &Harness) -> usize {
            harness
                .store
                .log()
                .into_iter()
                .filter(|call| matches!(call, Call::EvictTo { .. }))
                .count()
        }

        /// One event flushed, its eviction failed, and the report of that fault read.
        fn after_one_failed_eviction() -> Harness {
            let store = ModelStore::new();
            store.script_evict(vec![Err(MaintFault::Io(io::ErrorKind::Other))]);
            let mut harness = Harness::over(store, open_floor(), 64);
            resumed(&mut harness);
            harness.emit(foreground(&AT_NOON, Duration::ZERO));
            settled(&mut harness);
            harness.passes(2);
            let reports = harness.reports();
            println!("ADV2 f1 reports up to the failed eviction: {reports:?}");
            assert_eq!(evict_calls(&harness), 1);
            assert!(reports.contains(&Report::MaintenanceFailing {
                fault: MaintFault::Io(io::ErrorKind::Other)
            }));
            harness
        }

        #[test]
        fn a_second_forward_step_inside_the_grace_waits_its_own_grace() {
            let mut harness = Harness::new();
            resumed(&mut harness);
            let start_wall = harness.clocks.wall();
            let day = Duration::from_secs(86_400);
            harness.clocks.step_wall_forward(15 * day);
            let first_step = harness.clocks.monotonic();
            while harness.clocks.monotonic() - first_step < CLOCK_STEP_GRACE - 10 * SECOND {
                harness.clocks.advance(5 * SECOND);
                harness.pass();
            }
            harness.clocks.step_wall_forward(15 * day);
            let second_step = harness.clocks.monotonic();
            let calls_before = harness.store.expire_calls().len();
            while harness.clocks.monotonic() - second_step < 120 * SECOND {
                harness.clocks.advance(5 * SECOND);
                harness.pass();
            }
            let days_ahead: Vec<u64> = harness.store.expire_calls()[calls_before..]
                .iter()
                .map(|now| now.duration_since(start_wall).unwrap().as_secs() / 86_400)
                .collect();
            println!(
                "ADV2 f6 whole days ahead of the start that expire_due read in the 120 s after the second step: {days_ahead:?}"
            );
            assert!(
                days_ahead.iter().all(|days| *days < 30),
                "expire_due read a wall clock 30 days ahead within 120 s of the step that put it there: {days_ahead:?}"
            );
        }

        #[test]
        fn a_failed_eviction_is_called_again_at_the_next_deadline() {
            let mut harness = after_one_failed_eviction();
            harness.ask(Command::OpenPause);
            let mut later = Vec::new();
            for _ in 0..5 {
                harness.clocks.advance(MAINTENANCE_INTERVAL + SECOND);
                harness.passes(2);
                later.extend(harness.reports());
            }
            let evicts = evict_calls(&harness);
            println!(
                "ADV2 f1a after 5 deadlines (305 s) with no flush: evict_to calls = {evicts}, expire_due calls = {}, reports = {later:?}",
                harness.store.expire_calls().len()
            );
            assert!(
                evicts >= 2,
                "a failed eviction was not called again in 5 deadlines: evict_to calls = {evicts}"
            );
        }

        #[test]
        fn healthy_is_not_reported_while_the_failed_eviction_was_never_called_again() {
            let mut harness = after_one_failed_eviction();
            harness.ask(Command::OpenPause);
            harness.clocks.advance(MAINTENANCE_INTERVAL + SECOND);
            harness.pass();
            let before = harness.reports();
            assert_eq!(
                evict_calls(&harness),
                1,
                "the deadline's expiry was not alone in its pass"
            );
            assert!(
                !before.contains(&Report::MaintenanceHealthy),
                "MaintenanceHealthy was reported while the failed eviction was never called again: {before:?}"
            );
            harness.pass();
            assert_eq!(evict_calls(&harness), 2);
            assert_eq!(
                harness.reports(),
                [Report::MaintenanceHealthy],
                "the eviction that succeeded was not reported healthy"
            );
        }

        #[test]
        fn healthy_waits_for_a_good_eviction_after_two_eviction_faults_in_a_row() {
            let store = ModelStore::new();
            store.script_evict(vec![
                Err(MaintFault::Io(io::ErrorKind::Other)),
                Err(MaintFault::Io(io::ErrorKind::Other)),
            ]);
            let mut harness = Harness::over(store, open_floor(), 64);
            resumed(&mut harness);
            harness.emit(foreground(&AT_NOON, Duration::ZERO));
            settled(&mut harness);
            harness.passes(2);
            assert_eq!(evict_calls(&harness), 1);
            harness.ask(Command::OpenPause);
            harness.clocks.advance(MAINTENANCE_INTERVAL + SECOND);
            harness.passes(2);
            assert_eq!(
                evict_calls(&harness),
                2,
                "the failed eviction was not called again"
            );
            harness.clocks.advance(MAINTENANCE_INTERVAL + SECOND);
            harness.pass();
            let before = harness.reports();
            assert!(
                !before.contains(&Report::MaintenanceHealthy),
                "MaintenanceHealthy came after a good expiry while no eviction had succeeded: {before:?}"
            );
            harness.pass();
            assert_eq!(evict_calls(&harness), 3);
            let after = harness.reports();
            assert_eq!(
                after
                    .iter()
                    .filter(|report| **report == Report::MaintenanceHealthy)
                    .count(),
                1,
                "the eviction that succeeded was not reported healthy once: {after:?}"
            );
        }

        #[test]
        fn healthy_is_not_reported_while_every_expiry_fails() {
            let store = ModelStore::new();
            let mut script = vec![Ok(Pass::default())];
            script.extend((0..6).map(|_| Err(MaintFault::Db(10))));
            store.script_expire(script);
            let mut harness = Harness::over(store, open_floor(), 64);
            resumed(&mut harness);
            let mut seen = Vec::new();
            for _ in 0..5 {
                harness.clocks.advance(MAINTENANCE_INTERVAL + SECOND);
                harness.pass();
                harness.emit(foreground(&AT_NOON, Duration::ZERO));
                settled(&mut harness);
                harness.passes(2);
                seen.extend(harness.reports());
            }
            let snapshot = harness.owner.snapshot();
            let healthy = seen
                .iter()
                .filter(|report| **report == Report::MaintenanceHealthy)
                .count();
            println!(
                "ADV2 f1c expire_due calls = {} (the one at open good, the rest failed), last_expiry_at moved = {}, reports = {seen:?}",
                harness.store.expire_calls().len(),
                snapshot.last_expiry_at != Some(noon())
            );
            assert_eq!(
                healthy,
                0,
                "MaintenanceHealthy was reported {healthy} times while each of the 5 expiry calls after open failed; the last report is {:?}",
                seen.last()
            );
        }

        #[test]
        fn the_reported_cause_follows_the_fault_under_the_floor() {
            let mut harness = Harness::new();
            harness
                .store
                .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
            resumed(&mut harness);
            harness.emit(foreground_now());
            settled(&mut harness);
            let writes_before = harness.store.write_calls().len();
            harness.store.set_free_bytes(Ok(FREE_SPACE_FLOOR_BYTES - 1));
            for _ in 0..200 {
                harness.clocks.advance(SECOND);
                harness.pass();
            }
            let reports = harness.reports();
            println!(
                "ADV2 f2 200 s under the floor after an I/O fault: write calls {writes_before} -> {}, reports = {reports:?}",
                harness.store.write_calls().len()
            );
            assert!(
                reports.iter().any(|report| matches!(
                    report,
                    Report::DrainFailing {
                        cause: DrainCause::NoSpace,
                        ..
                    }
                )),
                "under the floor for 200 s no report carries the cause no_space: {reports:?}"
            );
        }

        #[test]
        fn the_reported_cause_follows_the_fault_in_the_other_order_and_keeps_its_since() {
            let mut harness = Harness::new();
            harness.store.set_free_bytes(Ok(FREE_SPACE_FLOOR_BYTES - 1));
            resumed(&mut harness);
            harness.emit(foreground_now());
            settled(&mut harness);
            let first_since = harness.clocks.wall();
            harness
                .store
                .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
            harness.store.set_free_bytes(Ok(FREE_SPACE_FLOOR_BYTES));
            for _ in 0..200 {
                harness.clocks.advance(SECOND);
                harness.pass();
            }
            let failing: Vec<(DrainCause, std::time::SystemTime)> = harness
                .reports()
                .into_iter()
                .filter_map(|report| match report {
                    Report::DrainFailing { cause, since } => Some((cause, since)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                failing,
                [
                    (DrainCause::NoSpace, first_since),
                    (DrainCause::Io, first_since)
                ],
                "one report per change of the cause, each with the since of the first fault"
            );
        }

        /// A store whose expiry frees what it shreds, as the port's text says.
        struct Shrinking {
            inner: ModelStore,
            disk: Arc<AtomicU64>,
        }

        impl MaintainStore for Shrinking {
            fn expire_due(
                &mut self,
                now: SystemTime,
                max_segments: usize,
            ) -> Result<Pass, MaintFault> {
                let pass = self.inner.expire_due(now, max_segments)?;
                if !pass.shredded.is_empty() {
                    self.disk.store(0, Ordering::Relaxed);
                }
                Ok(pass)
            }

            fn evict_to(
                &mut self,
                bound: ByteBound,
                max_segments: usize,
            ) -> Result<Pass, MaintFault> {
                self.inner.evict_to(bound, max_segments)
            }

            fn usage(&mut self) -> Result<Usage, MaintFault> {
                let disk = self.disk.load(Ordering::Relaxed);
                Ok(Usage {
                    disk_bytes: disk,
                    logical_bytes: disk,
                })
            }
        }

        impl CaptureStore for Shrinking {
            fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
                let written = self.inner.write_batch(write)?;
                self.disk.fetch_add(written.disk_bytes, Ordering::Relaxed);
                Ok(written)
            }

            fn volume_free_bytes(&self) -> io::Result<u64> {
                self.inner.volume_free_bytes()
            }
        }

        #[test]
        fn the_disk_total_follows_an_expiry_that_shredded() {
            let model = ModelStore::new();
            let disk = Arc::new(AtomicU64::new(0));
            let clocks = ManualClocks::at(noon());
            let (sink, writer) = sink::channel(64);
            let (mut owner, handle, _reports) = Owner::open(OwnerParts {
                writer,
                store: Shrinking {
                    inner: model.clone(),
                    disk: disk.clone(),
                },
                clocks: clocks.clone(),
                floor: open_floor(),
                cap: StorageCap::DEFAULT,
                shred_chunk: SHRED_CHUNK,
            });
            let ask = |owner: &mut Owner<Shrinking, ManualClocks>, command| {
                let pending = handle.ask(command).unwrap();
                owner.pass(Duration::ZERO);
                pending.try_answer().unwrap()
            };
            ask(&mut owner, Command::ClosePause);
            for _ in 0..3 {
                sink.emit_event(foreground(&AT_NOON, Duration::ZERO));
            }
            ask(&mut owner, Command::Settle);
            owner.pass(Duration::ZERO);
            let before = owner.snapshot().disk_bytes;
            model.script_expire(vec![Ok(Pass {
                shredded: vec![Ordinal::new(1)],
                more_due: false,
                owed: false,
            })]);
            ask(&mut owner, Command::OpenPause);
            for _ in 0..10 {
                clocks.advance(MAINTENANCE_INTERVAL + SECOND);
                owner.pass(Duration::ZERO);
                owner.pass(Duration::ZERO);
            }
            let after = owner.snapshot();
            println!(
                "ADV2 f4 disk_bytes before the expiry = {before:?}; 10 deadlines after an expiry that shredded the one segment: snapshot.disk_bytes = {:?}, the store's usage() = {}, last_expiry_at moved = {}",
                after.disk_bytes,
                disk.load(Ordering::Relaxed),
                after.last_expiry_at != Some(noon())
            );
            assert_eq!(
                after.disk_bytes,
                Some(disk.load(Ordering::Relaxed)),
                "the snapshot's disk total is not the store's usage 10 deadlines after the expiry"
            );
        }

        #[test]
        fn a_retry_after_a_lost_answer_counts_the_disk_bytes_of_the_stored_segment() {
            let mut harness = Harness::new();
            harness
                .store
                .script_writes([WriteScript::KeepThenFail(WriteFault::Io(
                    io::ErrorKind::Other,
                ))]);
            harness.ask(Command::ClosePause);
            harness.emit(foreground_now());
            harness.ask(Command::Settle);
            assert_eq!(
                harness.store.batches().len(),
                1,
                "the store did not keep the batch"
            );
            let Answer::Settled(snapshot) = harness.ask(Command::Settle) else {
                panic!("no settle answer");
            };
            assert_eq!(
                snapshot.events_persisted, 1,
                "the retry was not acknowledged"
            );
            println!(
                "ADV2 a-state3 the Settled answer's disk_bytes = {:?}; the snapshot one pass later = {:?}",
                snapshot.disk_bytes,
                {
                    harness.pass();
                    harness.owner.snapshot().disk_bytes
                }
            );
            assert_ne!(
                snapshot.disk_bytes,
                Some(0),
                "the snapshot counts no disk bytes for the stored segment"
            );
        }

        #[test]
        fn a_port_that_cannot_read_the_free_space_writes_its_batch() {
            for kind in [io::ErrorKind::Unsupported, io::ErrorKind::PermissionDenied] {
                let mut harness = Harness::new();
                harness.store.set_free_bytes(Err(kind));
                resumed(&mut harness);
                for _ in 0..3 {
                    harness.emit(foreground_now());
                }
                let snapshot = settled(&mut harness);
                let reads = harness
                    .store
                    .log()
                    .iter()
                    .filter(|call| matches!(call, Call::FreeBytes))
                    .count();
                println!(
                    "ADV2 q5 volume_free_bytes answers Err({kind:?}): reads = {reads}, write calls = {}, persisted = {}, reports = {:?}",
                    harness.store.write_calls().len(),
                    snapshot.events_persisted,
                    harness.reports()
                );
                assert_eq!(snapshot.events_persisted, 3);
            }
        }
    }

    fn rejected(harness: &Harness) -> Vec<Report> {
        harness
            .reports()
            .into_iter()
            .filter(|report| matches!(report, Report::BatchRejected { .. }))
            .collect()
    }

    #[test]
    fn one_fault_and_two_settles_with_the_clock_still_drop_nothing() {
        let db = WriteFault::Db(11);
        let mut harness = Harness::new();
        harness.store.script_writes([
            WriteScript::Fail(db),
            WriteScript::Fail(db),
            WriteScript::Fail(db),
        ]);
        resumed(&mut harness);
        for _ in 0..5 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        settled(&mut harness);
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.events_lost, 0, "an early try counted");
        assert_eq!(snapshot.events_pending, 5);
        assert!(rejected(&harness).is_empty());
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        let after = settled(&mut harness);
        assert_eq!(after.events_persisted, 5);
        assert_eq!(after.events_lost, 0);
        assert!(after.is_conserved());
    }

    #[test]
    fn a_fault_that_does_not_end_drops_the_batch_at_its_third_due_try() {
        let db = WriteFault::Db(11);
        let mut harness = Harness::new();
        harness.store.fail_every_write(Some(db));
        resumed(&mut harness);
        for _ in 0..5 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        settled(&mut harness);
        assert_eq!(
            harness.owner.snapshot().events_lost,
            0,
            "dropped before the third due try"
        );
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        assert_eq!(harness.owner.snapshot().events_lost, 5);
        assert_eq!(rejected(&harness).len(), 1);
    }

    #[test]
    fn another_fault_at_an_early_try_ends_the_run_of_identical_failures() {
        let db = WriteFault::Db(11);
        let mut harness = Harness::new();
        harness.store.script_writes([
            WriteScript::Fail(db),
            WriteScript::Fail(WriteFault::Db(5)),
            WriteScript::Fail(db),
            WriteScript::Fail(db),
        ]);
        resumed(&mut harness);
        for _ in 0..5 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        let snapshot = harness.owner.snapshot();
        assert_eq!(
            snapshot.events_lost, 0,
            "the run of identical failures went on"
        );
        assert_eq!(snapshot.events_pending, 5);
        assert!(rejected(&harness).is_empty());
    }

    #[test]
    fn settles_during_an_io_fault_leave_the_next_due_try_where_it_was() {
        let mut harness = Harness::new();
        harness
            .store
            .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        resumed(&mut harness);
        for _ in 0..5 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        for _ in 0..3 {
            settled(&mut harness);
        }
        harness.store.fail_every_write(None);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        assert_eq!(
            harness.owner.snapshot().events_persisted,
            5,
            "the next due try moved past 1 s"
        );
    }

    #[test]
    fn a_sealed_batch_is_given_up_at_its_first_answer() {
        let mut harness = Harness::new();
        harness
            .store
            .script_writes([WriteScript::Fail(WriteFault::Sealed)]);
        resumed(&mut harness);
        for _ in 0..4 {
            harness.emit(foreground_now());
        }
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.events_lost, 4);
        assert_eq!(snapshot.events_pending, 0);
        assert_eq!(harness.store.write_calls().len(), 1);
        assert!(matches!(
            rejected(&harness)[..],
            [Report::BatchRejected {
                events: 4,
                fault: WriteFault::Sealed,
                ..
            }]
        ));
    }

    #[test]
    fn a_drop_then_a_good_write_clears_the_count_since_the_last_acknowledged_write() {
        let mut harness = Harness::new();
        harness
            .store
            .script_writes([WriteScript::Fail(WriteFault::Refused)]);
        resumed(&mut harness);
        for _ in 0..3 {
            harness.emit(foreground_now());
        }
        let dropped = settled(&mut harness);
        assert_eq!(dropped.events_lost, 3);
        assert_eq!(
            dropped.lost_since_acknowledged, 3,
            "the count was cleared at the drop"
        );
        harness.emit(foreground_now());
        harness.emit(foreground_now());
        let written = settled(&mut harness);
        assert_eq!(written.events_persisted, 2);
        assert_eq!(written.events_lost, 3);
        assert_eq!(written.lost_since_acknowledged, 0);
    }

    #[test]
    fn a_store_that_takes_nothing_keeps_the_count_between_two_drops() {
        let mut harness = Harness::new();
        harness.store.fail_every_write(Some(WriteFault::Refused));
        resumed(&mut harness);
        for _ in 0..3 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        harness.passes(3);
        assert_eq!(harness.owner.snapshot().lost_since_acknowledged, 3);
        harness.emit(foreground_now());
        harness.emit(foreground_now());
        let second = settled(&mut harness);
        assert_eq!(second.lost_since_acknowledged, 5);
        assert_eq!(rejected(&harness).len(), 2);
    }

    #[test]
    fn a_dropped_batch_reports_its_instants_and_fault_and_the_snapshot_keeps_them() {
        let mut harness = Harness::new();
        harness
            .store
            .script_writes([WriteScript::Fail(WriteFault::Refused)]);
        resumed(&mut harness);
        harness.emit(foreground(&AT_NOON, Duration::from_secs(3)));
        harness.emit(foreground(&AT_NOON, Duration::from_secs(1)));
        let snapshot = settled(&mut harness);
        let [Report::BatchRejected {
            events,
            first_capture,
            last_capture,
            fault,
        }] = rejected(&harness)[..]
        else {
            panic!("no one BatchRejected");
        };
        assert_eq!((events, fault), (2, WriteFault::Refused));
        assert!(first_capture <= last_capture);
        assert_eq!(snapshot.lost_first_at, Some(first_capture));
        assert_eq!(snapshot.lost_last_at, Some(last_capture));
        harness
            .store
            .script_writes([WriteScript::Fail(WriteFault::Refused)]);
        harness.emit(foreground_now());
        let later = settled(&mut harness);
        assert_eq!(later.lost_first_at, Some(first_capture));
        assert!(later.lost_last_at >= Some(last_capture));
    }

    /// Ruling DM-12 A7 (PR2 as a pin): OpenPause draws the barrier, and a
    /// Shutdown after it stores nothing that reached the owner after the pause.
    #[test]
    fn a_shutdown_after_open_pause_stores_nothing_that_came_after_the_pause() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        harness.emit(key());
        harness.pass();
        let stored_before = settled(&mut harness);
        assert_eq!(stored_before.events_persisted, 1);
        assert!(matches!(harness.ask(Command::OpenPause), Answer::Paused(_)));
        harness.emit(key());
        harness.emit(key());
        harness.owner.receive_in_pass(Duration::ZERO);
        harness.emit(key());
        let pending = harness.handle.ask(Command::Shutdown).unwrap();
        harness.owner.finish_pass();
        let Some(Answer::ShutDown(last)) = pending.try_answer() else {
            panic!("the owner did not answer the Shutdown");
        };
        assert_eq!(last.events_persisted, 1, "{last:?}");
        assert_eq!(harness.store.records().len(), 1);
        assert_eq!(
            last.discarded_by_barrier,
            stored_before.discarded_by_barrier + 3,
            "{last:?}"
        );
        assert_eq!(last.events_lost, 0);
    }

    #[test]
    fn an_event_in_the_hands_of_a_pass_at_open_pause_is_not_stored_and_is_discarded_by_the_barrier()
    {
        let mut harness = Harness::new();
        resumed(&mut harness);
        harness.emit(foreground_now());
        harness.owner.receive_in_pass(Duration::ZERO);
        let pending = harness.handle.ask(Command::OpenPause).unwrap();
        assert!(harness.owner.finish_pass());
        assert!(matches!(pending.try_answer(), Some(Answer::Paused(_))));
        let snapshot = settled(&mut harness);
        assert_eq!(snapshot.discarded_by_barrier, 1);
        assert_eq!(snapshot.events_lost, 0);
        assert_eq!(snapshot.events_persisted, 0);
        assert!(harness.store.batches().is_empty());
        assert!(snapshot.is_conserved());
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "ruling DM-10: about eight minutes in a debug build, so the release build of scripts/r1/ci.d/26-daemon-owner-growth.sh runs it in the macos job"]
    fn two_million_floor_records_grow_the_owner_by_at_most_the_bound() {
        const NAME: &str =
            "daemon::owner::tests::two_million_floor_records_grow_the_owner_by_at_most_the_bound";
        const CHILD: &str = "HALVO_DAEMON_GROWTH_CHILD";
        const RECORDS: u64 = 2_000_000;
        const OWNER_GROWTH_MAX_BYTES: u64 = 8_388_608;
        if std::env::var_os(CHILD).is_some() {
            let (sink, writer) = sink::channel(QUEUE_SLOTS);
            let thread = spawn(OwnerParts {
                writer,
                store: crate::daemon::discard_port::DiscardPort::new(std::env::temp_dir()),
                clocks: crate::daemon::maintenance::SystemClocks::new(),
                floor: open_floor(),
                cap: StorageCap::DEFAULT,
                shred_chunk: SHRED_CHUNK,
            })
            .unwrap();
            assert!(matches!(
                thread.handle.call(Command::ClosePause),
                Ok(Answer::Resumed(_))
            ));
            let ready_heap = crate::daemon::signal::heap_bytes_in_use().unwrap();
            let ready_footprint = crate::daemon::signal::phys_footprint_bytes().unwrap();
            let ready = crate::daemon::signal::peak_rss_bytes().unwrap();
            let started = std::time::Instant::now();
            for _ in 0..RECORDS {
                assert_eq!(
                    sink.emit_event(title_of(
                        crate::floor::text::TEXT_MAX_BYTES,
                        crate::floor::text::TEXT_MAX_BYTES
                    )),
                    sink::Emitted::Queued
                );
            }
            let Ok(Answer::Settled(settled)) = thread.handle.call(Command::Settle) else {
                panic!("the owner did not settle");
            };
            let peak = crate::daemon::signal::peak_rss_bytes().unwrap();
            let settled_heap = crate::daemon::signal::heap_bytes_in_use().unwrap();
            let settled_footprint = crate::daemon::signal::phys_footprint_bytes().unwrap();
            assert_eq!(settled.events_persisted, RECORDS);
            let grown = |after: u64, before: u64| after as i128 - before as i128;
            println!(
                "owner_heap_bytes held all_zones_ready={} all_zones_settled={} all_zones_growth={} default_zone_ready={} default_zone_settled={} default_zone_growth={} footprint_ready={ready_footprint} footprint_settled={settled_footprint} footprint_growth={}",
                ready_heap.all_zones,
                settled_heap.all_zones,
                grown(settled_heap.all_zones, ready_heap.all_zones),
                ready_heap.default_zone,
                settled_heap.default_zone,
                grown(settled_heap.default_zone, ready_heap.default_zone),
                grown(settled_footprint, ready_footprint)
            );
            println!(
                "owner_growth_bytes={} records={RECORDS} elapsed_ms={}",
                peak.saturating_sub(ready),
                started.elapsed().as_millis()
            );
            let _ = thread.handle.call(Command::Shutdown);
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                NAME,
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "the child failed: {text}");
        let line = text
            .lines()
            .find_map(|line| line.find("owner_growth_bytes=").map(|at| &line[at..]))
            .expect("the child printed no growth line");
        if let Some(heap) = text
            .lines()
            .find_map(|line| line.find("owner_heap_bytes ").map(|at| &line[at..]))
        {
            println!("{heap}");
        }
        if let Err(over) = heap_growth_held(&text) {
            panic!("{over}");
        }
        println!("{line}");
        let growth: u64 = line["owner_growth_bytes=".len()..]
            .split(' ')
            .next()
            .and_then(|n| n.parse().ok())
            .expect("the growth is no number");
        assert!(
            growth <= OWNER_GROWTH_MAX_BYTES,
            "the owner grew by {growth} bytes, above {OWNER_GROWTH_MAX_BYTES}"
        );
    }

    #[cfg(all(test, feature = "r2"))]
    fn run_mark(at: SystemTime, p: &str) -> crate::floor::record::RunMark {
        crate::floor::record::RunMark::new(at, serde_json::from_str(p).unwrap()).unwrap()
    }

    #[cfg(all(test, feature = "r2"))]
    #[test]
    fn a_run_mark_record_pushed_through_the_entry_is_stored_and_counted_under_its_kind() {
        use crate::floor::record::DecodedPayload;
        let mut harness = Harness::new();
        resumed(&mut harness);
        let before = harness.owner.snapshot();
        let at = noon() - Duration::from_secs(13 * 3600);
        assert!(harness
            .owner
            .append_run_mark(run_mark(at, r#"{"phase":"start"}"#)));
        let after = settled(&mut harness);
        assert_eq!(after.events_received(), before.events_received() + 1);
        assert_eq!(after.events_persisted, before.events_persisted + 1);
        assert_eq!(
            harness.owner.counters.received_under(EntryKind::RUN_MARK),
            1
        );
        let batches = harness.store.batches();
        let [batch] = &batches[..] else {
            panic!("{} batches", batches.len());
        };
        let records = batch.records();
        let [record] = &records[..] else {
            panic!("{} records", records.len());
        };
        assert_eq!(record.kind(), "run_mark");
        let DecodedPayload::RunMark(p) = record.payload() else {
            panic!("no run-mark payload");
        };
        assert_eq!(p.get("phase").and_then(|v| v.as_str()), Some("start"));
        assert_eq!(batch.entries.len(), 1);
        assert_eq!(batch.entries[0].kind.as_str(), "run_mark");
        assert_eq!(batch.entries[0].events, 1);
        assert_eq!(batch.day, utc_day(record.at()));
        assert_eq!(
            (batch.first_capture, batch.last_capture),
            (record.at(), record.at())
        );
        assert_ne!(batch.day, utc_day(crate::event::unix_nanos(noon())));

        harness.emit(foreground(&AT_NOON, Duration::ZERO));
        harness.pass();
        assert!(harness.owner.append_run_mark(run_mark(noon(), "{}")));
        harness.emit(foreground(&AT_NOON, Duration::ZERO));
        harness.pass();
        settled(&mut harness);
        let batches = harness.store.batches();
        let numbers: Vec<u64> = batches[1].records().iter().map(|r| r.seq()).collect();
        let kinds: Vec<&str> = batches[1].records().iter().map(|r| r.kind()).collect();
        assert_eq!(kinds, ["foreground_app", "run_mark", "foreground_app"]);
        assert_eq!(numbers, [numbers[0], numbers[0] + 1, numbers[0] + 2]);
    }

    #[cfg(all(test, feature = "r2"))]
    #[test]
    fn the_entry_refuses_a_run_mark_record_while_the_owner_discards() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        harness.emit(foreground_now());
        harness.pass();
        assert!(matches!(harness.ask(Command::OpenPause), Answer::Paused(_)));
        let before = harness.owner.snapshot();
        assert!(!harness.owner.append_run_mark(run_mark(noon(), "{}")));
        assert!(!harness.owner.append_run_mark(run_mark(noon(), "{}")));
        assert_eq!(
            harness.owner.snapshot(),
            before,
            "a refused offer changed a counter"
        );
        resumed(&mut harness);
        harness.emit(foreground_now());
        let snapshot = settled(&mut harness);
        let numbers: Vec<u64> = harness.store.records().iter().map(|r| r.seq()).collect();
        assert_eq!(numbers.len(), 2);
        assert_eq!(numbers[1], numbers[0] + 1, "a refused mark took a number");
        assert!(snapshot.is_conserved());
    }

    #[cfg(all(test, feature = "r2"))]
    #[test]
    fn the_entry_refuses_a_run_mark_record_past_the_bound_of_a_failing_drain() {
        let mut harness = Harness::new();
        harness
            .store
            .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        resumed(&mut harness);
        harness.emit(foreground_now());
        harness.pass();
        settled(&mut harness);
        let within = harness.owner.snapshot();
        assert!(harness.owner.append_run_mark(run_mark(noon(), "{}")));
        let held = harness.owner.snapshot();
        assert_eq!(held.events_received(), within.events_received() + 1);
        assert_eq!(held.events_pending, within.events_pending + 1);
        while (harness.owner.snapshot().events_pending as usize) < HELD_MAX_EVENTS {
            harness.emit(foreground_now());
            harness.pass();
        }
        let full = harness.owner.snapshot();
        assert!(!harness.owner.append_run_mark(run_mark(noon(), "{}")));
        assert_eq!(
            harness.owner.snapshot(),
            full,
            "a refused offer changed a counter"
        );
        harness.store.fail_every_write(None);
        harness.clocks.advance(RETRY_MAX);
        let after = settled(&mut harness);
        assert!(after.is_conserved());
        let marks = harness
            .store
            .records()
            .iter()
            .filter(|record| record.kind() == "run_mark")
            .count();
        assert_eq!(marks, 1);
    }

    #[test]
    fn a_pass_answers_the_waiting_commands_before_it_waits_for_an_event() {
        let mut harness = Harness::new();
        let pending = harness.handle.ask(Command::ClosePause).unwrap();
        harness.owner.receive_in_pass(Duration::ZERO);
        assert!(
            matches!(pending.try_answer(), Some(Answer::Resumed(_))),
            "the command waited for the receive"
        );
        assert!(harness.owner.finish_pass());
    }

    use crate::daemon::counters::BatchCode;

    fn tried_once(store: ModelStore, events: usize) -> Harness {
        let mut harness = Harness::over(store, open_floor(), 2048);
        resumed(&mut harness);
        let _ = harness.reports();
        for _ in 0..events {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        harness
    }

    fn two_due_tries(harness: &mut Harness) {
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
    }

    fn set_aside_counts(snapshot: &Snapshot) -> Vec<(i32, u64)> {
        BatchCode::ALL
            .into_iter()
            .map(|code| (code.primary(), snapshot.batches_set_aside_by_code.of(code)))
            .filter(|(_, count)| *count > 0)
            .collect()
    }

    fn three_due_tries_set_the_batch_aside(code: i32) {
        let fault = WriteFault::Db(code);
        let store = ModelStore::new();
        store.script_writes([WriteScript::Fail(fault); 3]);
        let mut harness = tried_once(store, 5);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        let between = harness.reports();
        assert!(
            matches!(
                between[..],
                [Report::DrainFailing {
                    cause: DrainCause::Db,
                    ..
                }]
            ),
            "code {code} between the tries: {between:?}"
        );
        assert_eq!(harness.owner.snapshot().events_lost, 0, "code {code}");
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        assert_eq!(harness.store.write_calls().len(), 3, "code {code}");
        let snapshot = harness.owner.snapshot();
        assert_eq!(
            (snapshot.events_lost, snapshot.events_pending),
            (5, 0),
            "code {code}"
        );
        assert_eq!(set_aside_counts(&snapshot), [(code, 1)]);
        let reports = harness.reports();
        assert!(
            matches!(
                reports[..],
                [
                    Report::BatchRejected { events: 5, fault: rejected, .. },
                    Report::DrainHealthy
                ] if rejected == fault
            ),
            "code {code} at the third due try: {reports:?}"
        );
        harness.emit(foreground_now());
        let after = settled(&mut harness);
        assert_eq!(after.events_persisted, 1, "code {code}");
        assert!(after.is_conserved());
    }

    #[test]
    fn three_due_tries_answered_1_set_the_batch_aside() {
        three_due_tries_set_the_batch_aside(1);
    }

    #[test]
    fn three_due_tries_answered_2_set_the_batch_aside() {
        three_due_tries_set_the_batch_aside(2);
    }

    #[test]
    fn three_due_tries_answered_12_set_the_batch_aside() {
        three_due_tries_set_the_batch_aside(12);
    }

    #[test]
    fn three_due_tries_answered_18_set_the_batch_aside() {
        three_due_tries_set_the_batch_aside(18);
    }

    #[test]
    fn three_due_tries_answered_19_set_the_batch_aside() {
        three_due_tries_set_the_batch_aside(19);
    }

    #[test]
    fn three_due_tries_answered_20_set_the_batch_aside() {
        three_due_tries_set_the_batch_aside(20);
    }

    #[test]
    fn three_due_tries_answered_21_set_the_batch_aside() {
        three_due_tries_set_the_batch_aside(21);
    }

    #[test]
    fn three_due_tries_answered_25_set_the_batch_aside() {
        three_due_tries_set_the_batch_aside(25);
    }

    #[test]
    fn three_due_tries_answered_19_20_and_19_set_the_batch_aside() {
        let store = ModelStore::new();
        store.script_writes([
            WriteScript::Fail(WriteFault::Db(19)),
            WriteScript::Fail(WriteFault::Db(20)),
            WriteScript::Fail(WriteFault::Db(19)),
        ]);
        let mut harness = tried_once(store, 4);
        two_due_tries(&mut harness);
        let snapshot = harness.owner.snapshot();
        assert_eq!((snapshot.events_lost, snapshot.events_pending), (4, 0));
        assert_eq!(set_aside_counts(&snapshot), [(19, 1)]);
        assert!(matches!(
            rejected(&harness)[..],
            [Report::BatchRejected {
                events: 4,
                fault: WriteFault::Db(19),
                ..
            }]
        ));
    }

    #[test]
    fn extended_codes_of_one_of_the_eight_are_read_as_that_code() {
        let unique = WriteFault::Db(19 | (8 << 8));
        let check = WriteFault::Db(19 | (1 << 8));
        let store = ModelStore::new();
        store.script_writes([
            WriteScript::Fail(unique),
            WriteScript::Fail(check),
            WriteScript::Fail(unique),
        ]);
        let mut harness = tried_once(store, 3);
        two_due_tries(&mut harness);
        let snapshot = harness.owner.snapshot();
        assert_eq!(snapshot.events_lost, 3);
        assert_eq!(set_aside_counts(&snapshot), [(19, 1)]);
        assert!(matches!(
            rejected(&harness)[..],
            [Report::BatchRejected { events: 3, fault, .. }] if fault == unique
        ));
    }

    #[test]
    fn three_due_tries_answered_10_or_an_unknown_code_leave_the_batch_held() {
        for code in [10, 99] {
            let store = ModelStore::new();
            store.fail_every_write(Some(WriteFault::Db(code)));
            let mut harness = tried_once(store, 4);
            two_due_tries(&mut harness);
            harness.clocks.advance(4 * RETRY_FIRST);
            harness.pass();
            assert_eq!(harness.store.write_calls().len(), 4, "code {code}");
            let snapshot = harness.owner.snapshot();
            assert_eq!(
                (snapshot.events_lost, snapshot.events_pending),
                (0, 4),
                "code {code}"
            );
            assert!(set_aside_counts(&snapshot).is_empty(), "code {code}");
            let reports = harness.reports();
            assert!(
                matches!(
                    reports[..],
                    [Report::DrainFailing {
                        cause: DrainCause::Io,
                        ..
                    }]
                ),
                "code {code}: {reports:?}"
            );
        }
    }

    #[test]
    fn early_tries_that_meet_one_of_the_eight_do_not_count() {
        let store = ModelStore::new();
        store.script_writes([
            WriteScript::Fail(WriteFault::Db(19)),
            WriteScript::Fail(WriteFault::Db(20)),
            WriteScript::Fail(WriteFault::Db(19)),
            WriteScript::Fail(WriteFault::Db(25)),
            WriteScript::Fail(WriteFault::Db(21)),
        ]);
        let mut harness = tried_once(store, 5);
        settled(&mut harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        settled(&mut harness);
        assert_eq!(harness.store.write_calls().len(), 4);
        assert_eq!(
            harness.owner.snapshot().events_lost,
            0,
            "set aside before the third due try"
        );
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        assert_eq!(harness.store.write_calls().len(), 5);
        let snapshot = harness.owner.snapshot();
        assert_eq!(snapshot.events_lost, 5);
        assert_eq!(set_aside_counts(&snapshot), [(21, 1)]);
    }

    #[test]
    fn an_early_try_outside_the_eight_ends_the_run_and_leaves_the_batch_held() {
        let store = ModelStore::new();
        store.script_writes([
            WriteScript::Fail(WriteFault::Db(19)),
            WriteScript::Fail(WriteFault::Db(19)),
            WriteScript::Fail(WriteFault::Db(10)),
            WriteScript::Fail(WriteFault::Db(19)),
        ]);
        let mut harness = tried_once(store, 4);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        settled(&mut harness);
        harness.clocks.advance(2 * RETRY_FIRST);
        harness.pass();
        assert_eq!(harness.store.write_calls().len(), 4);
        let snapshot = harness.owner.snapshot();
        assert_eq!(
            (snapshot.events_lost, snapshot.events_pending),
            (0, 4),
            "an early try outside the eight did not end the run"
        );
        assert!(rejected(&harness).is_empty());
    }

    #[test]
    fn a_batch_that_the_second_try_stores_after_19_is_written_once() {
        let store = ModelStore::new();
        store.script_writes([WriteScript::Fail(WriteFault::Db(19))]);
        let mut harness = tried_once(store, 4);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        let writes = harness.store.write_calls();
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].0, writes[1].0);
        assert_eq!(harness.store.batches().len(), 1);
        assert_eq!(harness.store.records().len(), 4);
        let snapshot = harness.owner.snapshot();
        assert_eq!((snapshot.events_persisted, snapshot.events_lost), (4, 0));
        assert!(set_aside_counts(&snapshot).is_empty());
        let reports = harness.reports();
        assert!(
            matches!(
                reports[..],
                [
                    Report::DrainFailing {
                        cause: DrainCause::Db,
                        ..
                    },
                    Report::DrainHealthy
                ]
            ),
            "{reports:?}"
        );
    }

    #[test]
    fn a_batch_kept_and_answered_19_is_acknowledged_by_its_next_try() {
        let store = ModelStore::new();
        store.script_writes([WriteScript::KeepThenFail(WriteFault::Db(19))]);
        let mut harness = tried_once(store, 4);
        two_due_tries(&mut harness);
        let writes = harness.store.write_calls();
        assert_eq!(writes.len(), 2);
        assert!(matches!(
            writes[1].1,
            Err(WriteFault::AlreadyWritten { .. })
        ));
        assert_eq!(harness.store.batches().len(), 1);
        let snapshot = harness.owner.snapshot();
        assert_eq!((snapshot.events_persisted, snapshot.events_lost), (4, 0));
        assert!(rejected(&harness).is_empty());
    }

    #[test]
    fn a_batch_set_aside_leaves_nothing_of_it_in_the_store_or_in_memory() {
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Db(19)));
        let mut harness = tried_once(store, 4);
        two_due_tries(&mut harness);
        let writes = harness.store.write_calls();
        assert_eq!(writes.len(), 3);
        let set_aside = writes[0].0;
        assert!(writes.iter().all(|(id, _)| *id == set_aside));
        assert_eq!(harness.owner.snapshot().events_lost, 4);
        assert!(
            harness.owner.payload_bytes().iter().all(|byte| *byte == 0),
            "the set-aside batch is still in the payload"
        );
        harness.store.fail_every_write(None);
        harness.emit(foreground_now());
        settled(&mut harness);
        let batches = harness.store.batches();
        assert_eq!(batches.len(), 1);
        assert_ne!(batches[0].segment_id, set_aside);
        assert_eq!(harness.store.records().len(), 1);
    }

    fn batch_through_three_tries(harness: &mut Harness, events: usize) {
        for _ in 0..events {
            harness.emit(foreground_now());
        }
        settled(harness);
        two_due_tries(harness);
    }

    fn after_the_set_aside(reports: &[Report]) -> Option<Report> {
        match reports {
            [Report::DrainFailing {
                cause: DrainCause::Db,
                ..
            }, Report::BatchRejected { events: 2, .. }, after] => Some(*after),
            _ => None,
        }
    }

    fn is_the_stop(after: Option<Report>) -> bool {
        matches!(
            after,
            Some(Report::DrainFailing {
                cause: DrainCause::Db,
                ..
            })
        )
    }

    fn stopped_by(fault: WriteFault) -> (Harness, SystemTime) {
        let store = ModelStore::new();
        store.fail_every_write(Some(fault));
        let mut harness = Harness::over(store, open_floor(), 2048);
        resumed(&mut harness);
        let _ = harness.reports();
        for n in 1..=2 {
            batch_through_three_tries(&mut harness, 2);
            let reports = harness.reports();
            assert_eq!(
                after_the_set_aside(&reports),
                Some(Report::DrainHealthy),
                "set-aside {n} of {fault:?}: {reports:?}"
            );
        }
        batch_through_three_tries(&mut harness, 2);
        let reports = harness.reports();
        assert!(
            is_the_stop(after_the_set_aside(&reports)),
            "the third set-aside of {fault:?}: {reports:?}"
        );
        let [Report::DrainFailing { since: began, .. }, _, Report::DrainFailing { since, .. }] =
            reports[..]
        else {
            panic!("{reports:?}");
        };
        assert_eq!(since, began, "the stop names another time: {reports:?}");
        (harness, since)
    }

    fn three_set_asides_in_a_row_hold_the_next_batch_until_a_good_write(
        fault: WriteFault,
    ) -> Vec<(i32, u64)> {
        let (mut harness, _) = stopped_by(fault);
        assert_eq!(harness.owner.snapshot().events_lost, 6);
        for _ in 0..2 {
            harness.emit(foreground_now());
        }
        settled(&mut harness);
        for _ in 0..10 {
            harness.clocks.advance(RETRY_MAX);
            harness.pass();
        }
        assert_eq!(harness.store.write_calls().len(), 9 + 11);
        let held = harness.owner.snapshot();
        assert_eq!(
            (held.events_lost, held.events_pending),
            (6, 2),
            "the fourth batch of {fault:?} was not held"
        );
        let reports = harness.reports();
        assert!(reports.is_empty(), "{fault:?} in the hold: {reports:?}");
        harness.store.fail_every_write(None);
        harness.clocks.advance(RETRY_MAX);
        harness.pass();
        let healed = harness.owner.snapshot();
        assert_eq!((healed.events_persisted, healed.events_pending), (2, 0));
        assert_eq!(harness.reports(), [Report::DrainHealthy]);
        harness.store.fail_every_write(Some(fault));
        batch_through_three_tries(&mut harness, 2);
        let reports = harness.reports();
        assert_eq!(
            after_the_set_aside(&reports),
            Some(Report::DrainHealthy),
            "after the good write {fault:?}: {reports:?}"
        );
        set_aside_counts(&harness.owner.snapshot())
    }

    #[test]
    fn three_set_asides_in_a_row_of_19_hold_the_next_batch_until_a_good_write() {
        let counts =
            three_set_asides_in_a_row_hold_the_next_batch_until_a_good_write(WriteFault::Db(19));
        assert_eq!(counts, [(19, 4)]);
    }

    #[test]
    fn three_set_asides_in_a_row_of_11_hold_the_next_batch_until_a_good_write() {
        let counts =
            three_set_asides_in_a_row_hold_the_next_batch_until_a_good_write(WriteFault::Db(11));
        assert!(counts.is_empty(), "{counts:?}");
    }

    #[test]
    fn a_good_write_between_set_asides_starts_the_count_again() {
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Db(19)));
        let mut harness = Harness::over(store, open_floor(), 2048);
        resumed(&mut harness);
        let _ = harness.reports();
        for _ in 0..2 {
            batch_through_three_tries(&mut harness, 2);
            assert_eq!(
                after_the_set_aside(&harness.reports()),
                Some(Report::DrainHealthy)
            );
        }
        harness.store.fail_every_write(None);
        harness.emit(foreground_now());
        assert_eq!(settled(&mut harness).events_persisted, 1);
        harness.store.fail_every_write(Some(WriteFault::Db(19)));
        for n in 1..=2 {
            batch_through_three_tries(&mut harness, 2);
            let reports = harness.reports();
            assert_eq!(
                after_the_set_aside(&reports),
                Some(Report::DrainHealthy),
                "set-aside {n} after the good write: {reports:?}"
            );
        }
        batch_through_three_tries(&mut harness, 2);
        assert!(is_the_stop(after_the_set_aside(&harness.reports())));
    }

    #[test]
    fn sealed_or_refused_batches_neither_count_toward_the_hold_nor_end_its_count() {
        for answer in [WriteFault::Sealed, WriteFault::Refused] {
            let store = ModelStore::new();
            store.fail_every_write(Some(WriteFault::Db(19)));
            let mut harness = Harness::over(store, open_floor(), 2048);
            resumed(&mut harness);
            let _ = harness.reports();
            for _ in 0..2 {
                batch_through_three_tries(&mut harness, 2);
            }
            let _ = harness.reports();
            harness.store.fail_every_write(Some(answer));
            for _ in 0..3 {
                harness.emit(foreground_now());
                harness.emit(foreground_now());
                settled(&mut harness);
                let reports = harness.reports();
                assert!(
                    matches!(
                        reports[..],
                        [Report::BatchRejected { events: 2, fault, .. }] if fault == answer
                    ),
                    "{answer:?}: {reports:?}"
                );
            }
            harness.store.fail_every_write(Some(WriteFault::Db(19)));
            batch_through_three_tries(&mut harness, 2);
            let reports = harness.reports();
            assert!(
                is_the_stop(after_the_set_aside(&reports)),
                "after three {answer:?}: {reports:?}"
            );
        }
    }

    #[test]
    fn a_new_owner_over_the_same_store_starts_the_count_of_set_asides_at_0() {
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Db(19)));
        let mut first = Harness::over(store.clone(), open_floor(), 2048);
        resumed(&mut first);
        let _ = first.reports();
        for _ in 0..2 {
            batch_through_three_tries(&mut first, 2);
        }
        assert_eq!(first.owner.snapshot().events_lost, 4);
        drop(first);
        let mut second = Harness::over(store, open_floor(), 2048);
        resumed(&mut second);
        let _ = second.reports();
        for n in 1..=2 {
            batch_through_three_tries(&mut second, 2);
            let reports = second.reports();
            assert_eq!(
                after_the_set_aside(&reports),
                Some(Report::DrainHealthy),
                "set-aside {n} of the new life: {reports:?}"
            );
        }
        batch_through_three_tries(&mut second, 2);
        assert!(is_the_stop(after_the_set_aside(&second.reports())));
    }

    #[test]
    fn a_batch_given_up_in_the_hold_ends_no_hold() {
        for answer in [WriteFault::Refused, WriteFault::Sealed] {
            let (mut harness, _) = stopped_by(WriteFault::Db(19));
            harness.store.fail_every_write(Some(answer));
            harness.emit(foreground_now());
            harness.emit(foreground_now());
            settled(&mut harness);
            let reports = harness.reports();
            assert!(
                matches!(
                    reports[..],
                    [Report::BatchRejected { events: 2, fault, .. }, Report::DrainHealthy]
                        if fault == answer
                ),
                "{answer:?} in the hold: {reports:?}"
            );
            harness.store.fail_every_write(Some(WriteFault::Db(19)));
            harness.emit(foreground_now());
            harness.emit(foreground_now());
            settled(&mut harness);
            for _ in 0..5 {
                harness.clocks.advance(RETRY_MAX);
                harness.pass();
            }
            let held = harness.owner.snapshot();
            assert_eq!(
                (held.events_lost, held.events_pending),
                (8, 2),
                "after {answer:?} the batch answered 19 was not held"
            );
            assert!(rejected(&harness).is_empty());
        }
    }

    #[test]
    fn the_snapshot_publishes_the_hold_through_a_give_up_until_a_good_write() {
        let fresh = Harness::new();
        assert!(!fresh.owner.snapshot().drain_hold, "a new owner holds");
        for answer in [WriteFault::Refused, WriteFault::Sealed] {
            let (mut harness, _) = stopped_by(WriteFault::Db(19));
            assert!(harness.owner.snapshot().drain_hold, "the third set-aside");
            harness.store.fail_every_write(Some(answer));
            harness.emit(foreground_now());
            harness.emit(foreground_now());
            settled(&mut harness);
            let reports = harness.reports();
            assert_eq!(reports.last(), Some(&Report::DrainHealthy), "{reports:?}");
            assert!(
                harness.owner.snapshot().drain_hold,
                "{answer:?} in the hold ended the published hold"
            );
            harness.store.fail_every_write(None);
            harness.emit(foreground_now());
            settled(&mut harness);
            assert!(
                !harness.owner.snapshot().drain_hold,
                "an acknowledged write after {answer:?} left the hold published"
            );
        }
    }

    /// The largest growth of the bytes in use over all malloc zones that the
    /// growth test holds: the Revision of DM-11 (4), as check 26 holds it.
    const HEAP_GROWTH_MAX_BYTES: i128 = 27_872;

    /// The largest `all_zones_growth` of the heap lines of `text`, when it is
    /// at or under [`HEAP_GROWTH_MAX_BYTES`].
    fn heap_growth_held(text: &str) -> Result<i128, String> {
        let largest = text
            .lines()
            .filter_map(|line| line.find("owner_heap_bytes ").map(|at| &line[at..]))
            .flat_map(|line| line.split(' '))
            .filter_map(|field| field.strip_prefix("all_zones_growth="))
            .filter_map(|value| value.parse::<i128>().ok())
            .max()
            .ok_or_else(|| "the test printed no bytes-in-use growth".to_owned())?;
        if largest <= HEAP_GROWTH_MAX_BYTES {
            Ok(largest)
        } else {
            Err(format!(
                "the bytes in use over all malloc zones grew by {largest}, above {HEAP_GROWTH_MAX_BYTES}"
            ))
        }
    }

    #[test]
    fn the_bytes_in_use_line_holds_the_largest_growth_at_or_under_27872() {
        let line = |growth: &str| {
            format!(
                "owner_heap_bytes held all_zones_ready=1331072 all_zones_settled=1331136 all_zones_growth={growth} default_zone_ready=69664\n"
            )
        };
        assert_eq!(HEAP_GROWTH_MAX_BYTES, 27_872);
        for (text, held) in [
            (line("64"), Ok(64)),
            (line("-3520"), Ok(-3520)),
            (line("27872"), Ok(27_872)),
            (line("27873"), Err(())),
            (line("211168"), Err(())),
            (format!("{}{}", line("64"), line("211168")), Err(())),
            (format!("{}{}", line("211168"), line("64")), Err(())),
            (line("99999999999999999999"), Err(())),
            (line("-"), Err(())),
            ("owner_rss_bytes all_zones_growth=64\n".to_owned(), Err(())),
            (String::new(), Err(())),
        ] {
            assert_eq!(heap_growth_held(&text).map_err(|_| ()), held, "{text:?}");
        }
    }

    #[test]
    fn the_drop_line_names_the_count_the_two_seconds_and_the_kind() {
        let noon_nanos = 1_794_571_200_i64 * 1_000_000_000;
        let dropped = |fault| Report::BatchRejected {
            events: 5,
            first_capture: noon_nanos + 999_999_999,
            last_capture: noon_nanos + 62_000_000_000,
            fault,
        };
        for fault in [
            WriteFault::Db(19 | (8 << 8)),
            WriteFault::Db(11),
            WriteFault::Db(0),
            WriteFault::Refused,
            WriteFault::Sealed,
        ] {
            assert_eq!(
                dropped(fault).drop_line().as_deref(),
                Some(
                    "halvo-capture: dropped events=5 first=2026-11-13T12:00:00Z \
                     last=2026-11-13T12:01:02Z error=db"
                ),
                "{fault:?}"
            );
        }
        let before_the_epoch = Report::BatchRejected {
            events: 1,
            first_capture: -1_000_000_001,
            last_capture: -1,
            fault: WriteFault::Db(25),
        };
        assert_eq!(
            before_the_epoch.drop_line().as_deref(),
            Some(
                "halvo-capture: dropped events=1 first=1969-12-31T23:59:58Z \
                 last=1969-12-31T23:59:59Z error=db"
            )
        );
        for report in [
            Report::StoreReady,
            Report::DrainHealthy,
            Report::DrainFailing {
                cause: DrainCause::Db,
                since: noon(),
            },
            Report::MaintenanceFailing {
                fault: MaintFault::CheckpointIncomplete,
            },
            Report::MaintenanceHealthy,
        ] {
            assert_eq!(report.drop_line(), None, "{report:?}");
        }
    }

    #[test]
    fn a_batch_set_aside_gives_its_drop_line() {
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Db(21)));
        let mut harness = Harness::over(store, open_floor(), 2048);
        resumed(&mut harness);
        harness.emit(foreground(&AT_NOON, Duration::from_secs(3)));
        harness.emit(foreground(&AT_NOON, Duration::from_secs(1)));
        settled(&mut harness);
        two_due_tries(&mut harness);
        let lines: Vec<String> = rejected(&harness)
            .iter()
            .filter_map(Report::drop_line)
            .collect();
        assert_eq!(
            lines,
            [
                "halvo-capture: dropped events=2 first=2026-11-13T11:59:57Z \
              last=2026-11-13T11:59:59Z error=db"
            ]
        );
    }

    #[test]
    fn the_first_batch_that_the_store_takes_after_the_stop_ends_the_hold() {
        let (mut harness, _) = stopped_by(WriteFault::Db(19));
        assert_eq!(
            harness.owner.wait(),
            POLL,
            "the owner waits less than a poll while nothing is held"
        );
        harness.store.fail_every_write(None);
        harness.emit(foreground_now());
        harness.emit(foreground_now());
        assert_eq!(settled(&mut harness).events_persisted, 2);
        assert_eq!(harness.reports(), [Report::DrainHealthy]);
        harness.store.fail_every_write(Some(WriteFault::Db(19)));
        batch_through_three_tries(&mut harness, 2);
        assert_eq!(
            after_the_set_aside(&harness.reports()),
            Some(Report::DrainHealthy)
        );
    }

    #[test]
    fn an_io_fault_in_the_hold_reports_io_since_the_stop() {
        let (mut harness, since) = stopped_by(WriteFault::Db(19));
        harness
            .store
            .fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        harness.emit(foreground_now());
        settled(&mut harness);
        assert_eq!(
            harness.reports(),
            [Report::DrainFailing {
                cause: DrainCause::Io,
                since
            }]
        );
        two_due_tries(&mut harness);
        let held = harness.owner.snapshot();
        assert_eq!((held.events_lost, held.events_pending), (6, 1));
    }

    #[test]
    fn due_tries_of_11_or_26_before_19_start_the_run_of_the_eight_at_19() {
        for before in [WriteFault::Db(11), WriteFault::Db(26)] {
            let store = ModelStore::new();
            store.script_writes([
                WriteScript::Fail(before),
                WriteScript::Fail(before),
                WriteScript::Fail(WriteFault::Db(19)),
                WriteScript::Fail(WriteFault::Db(19)),
                WriteScript::Fail(WriteFault::Db(19)),
            ]);
            let mut harness = tried_once(store, 4);
            two_due_tries(&mut harness);
            assert_eq!(harness.store.write_calls().len(), 3, "{before:?}");
            let held = harness.owner.snapshot();
            assert_eq!(
                (held.events_lost, held.events_pending),
                (0, 4),
                "{before:?}, {before:?} and 19 set the batch aside"
            );
            harness.clocks.advance(4 * RETRY_FIRST);
            harness.pass();
            assert_eq!(
                harness.owner.snapshot().events_lost,
                0,
                "{before:?}, {before:?}, 19 and 19 set the batch aside"
            );
            harness.clocks.advance(8 * RETRY_FIRST);
            harness.pass();
            assert_eq!(harness.store.write_calls().len(), 5, "{before:?}");
            let snapshot = harness.owner.snapshot();
            assert_eq!(
                (snapshot.events_lost, snapshot.events_pending),
                (4, 0),
                "{before:?}"
            );
            assert_eq!(set_aside_counts(&snapshot), [(19, 1)], "{before:?}");
        }
    }

    #[test]
    fn the_hold_tries_its_batch_after_1_2_4_8_16_32_and_60_s() {
        let (mut harness, _) = stopped_by(WriteFault::Db(19));
        harness.emit(foreground_now());
        settled(&mut harness);
        let mut writes = harness.store.write_calls().len();
        let short = Duration::from_millis(1);
        for wait in [1, 2, 4, 8, 16, 32, 60, 60] {
            harness.clocks.advance(Duration::from_secs(wait) - short);
            harness.pass();
            assert_eq!(
                harness.store.write_calls().len(),
                writes,
                "a try before {wait} s"
            );
            harness.clocks.advance(short);
            harness.pass();
            writes += 1;
            assert_eq!(
                harness.store.write_calls().len(),
                writes,
                "no try at {wait} s"
            );
        }
        let held = harness.owner.snapshot();
        assert_eq!((held.events_lost, held.events_pending), (6, 1));
        assert!(rejected(&harness).is_empty());
    }

    fn kept_then_already_written(harness: &mut Harness) {
        harness.store.fail_every_write(None);
        harness
            .store
            .script_writes([WriteScript::KeepThenFail(WriteFault::Db(19))]);
        harness.emit(foreground_now());
        harness.emit(foreground_now());
        settled(harness);
        harness.clocks.advance(RETRY_FIRST);
        harness.pass();
        let writes = harness.store.write_calls();
        assert!(
            matches!(
                writes.last(),
                Some((_, Err(WriteFault::AlreadyWritten { .. })))
            ),
            "{writes:?}"
        );
        let reports = harness.reports();
        assert_eq!(reports.last(), Some(&Report::DrainHealthy), "{reports:?}");
        assert_eq!(harness.owner.snapshot().events_pending, 0);
        harness.store.fail_every_write(Some(WriteFault::Db(19)));
    }

    fn the_count_starts_again(harness: &mut Harness, place: &str) {
        for n in 1..=2 {
            batch_through_three_tries(harness, 2);
            let reports = harness.reports();
            assert_eq!(
                after_the_set_aside(&reports),
                Some(Report::DrainHealthy),
                "set-aside {n} after AlreadyWritten {place}: {reports:?}"
            );
        }
        batch_through_three_tries(harness, 2);
        let reports = harness.reports();
        assert!(
            is_the_stop(after_the_set_aside(&reports)),
            "the third set-aside after AlreadyWritten {place}: {reports:?}"
        );
    }

    #[test]
    fn an_already_written_answer_starts_the_count_of_set_asides_again_in_the_hold_and_outside_it() {
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Db(19)));
        let mut harness = Harness::over(store, open_floor(), 2048);
        resumed(&mut harness);
        let _ = harness.reports();
        for _ in 0..2 {
            batch_through_three_tries(&mut harness, 2);
            assert_eq!(
                after_the_set_aside(&harness.reports()),
                Some(Report::DrainHealthy)
            );
        }
        kept_then_already_written(&mut harness);
        the_count_starts_again(&mut harness, "outside the hold");

        let (mut harness, _) = stopped_by(WriteFault::Db(19));
        kept_then_already_written(&mut harness);
        the_count_starts_again(&mut harness, "in the hold");
    }

    #[derive(Clone)]
    struct ModelHost {
        probe: Probe,
        opens: Result<(), OpenFault>,
        creates: Result<(), OpenFault>,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    const HOST_ID: StoreId = StoreId::from_bytes([7; 16]);

    impl ModelHost {
        fn new(probe: Probe, opens: Result<(), OpenFault>) -> ModelHost {
            ModelHost {
                probe,
                opens,
                creates: Ok(()),
                calls: Arc::default(),
            }
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }

        fn handle(&self, answer: Result<(), OpenFault>, access: Access) -> Opened {
            match (answer, access) {
                (Err(fault), _) => Opened::Fault {
                    fault,
                    id: Some(HOST_ID),
                },
                (Ok(()), Access::ReadWrite) => Opened::ReadWrite {
                    store: Box::new(ModelStore::new()),
                    id: HOST_ID,
                },
                (Ok(()), Access::MaintenanceOnly) => Opened::MaintenanceOnly {
                    store: Box::new(ModelStore::new()),
                    id: HOST_ID,
                },
            }
        }
    }

    impl StoreHost for ModelHost {
        fn probe(&self) -> Probe {
            self.calls.lock().unwrap().push("probe");
            self.probe
        }

        fn open(&self, access: Access) -> Opened {
            self.calls.lock().unwrap().push("open");
            self.handle(self.opens, access)
        }

        fn create(&self, _custody: Custody) -> Opened {
            self.calls.lock().unwrap().push("create");
            self.handle(self.creates, Access::ReadWrite)
        }
    }

    const STANDS: Probe = Probe::KeyFile {
        custody: Custody::RootFile,
        id: HOST_ID,
    };

    fn root_file_build(recorded: Option<Custody>) -> Selection {
        custody_policy::select(false, custody_policy::System::Macos, recorded)
    }

    fn row(fault: OpenFault) -> (StoreState, StoreErrorKind, bool) {
        use OpenFault as F;
        use StoreErrorKind as K;
        use StoreState as S;
        match fault {
            F::Locked => (S::Unavailable, K::Locked, true),
            F::Io => (S::Unavailable, K::Io, true),
            F::NoSpace => (S::Unavailable, K::NoSpace, true),
            F::Obstructed => (S::Unavailable, K::Obstructed, true),
            F::CustodyLocked => (S::Unavailable, K::CustodyLocked, true),
            F::RootDenied => (S::Unavailable, K::RootDenied, false),
            F::IoNotRetried => (S::Unavailable, K::Io, false),
            F::RootMissing => (S::Sealed, K::RootMissing, false),
            F::RootCorrupt => (S::Sealed, K::RootCorrupt, false),
            F::WrapAuth => (S::Sealed, K::WrapAuth, false),
            F::KeyFile => (S::Sealed, K::KeyFile, false),
            F::CustodyKind => (S::Sealed, K::CustodyKind, false),
            F::Database => (S::Sealed, K::Database, false),
            F::OlderStore => (S::UnsupportedVersion, K::Older, false),
            F::NewerStore => (S::UnsupportedVersion, K::Newer, false),
        }
    }

    const FAULTS: [OpenFault; 15] = [
        OpenFault::Locked,
        OpenFault::Io,
        OpenFault::NoSpace,
        OpenFault::Obstructed,
        OpenFault::CustodyLocked,
        OpenFault::RootDenied,
        OpenFault::IoNotRetried,
        OpenFault::RootMissing,
        OpenFault::RootCorrupt,
        OpenFault::WrapAuth,
        OpenFault::KeyFile,
        OpenFault::CustodyKind,
        OpenFault::Database,
        OpenFault::OlderStore,
        OpenFault::NewerStore,
    ];

    #[test]
    fn every_open_fault_has_a_slot_and_the_owner_stays_up() {
        let retry = Duration::from_secs(1);
        for fault in FAULTS {
            let (state, cause, timer) = row(fault);
            for probe in [true, false] {
                let host = if probe {
                    ModelHost::new(Probe::Fault { fault, id: Some(HOST_ID) }, Ok(()))
                } else {
                    ModelHost::new(STANDS, Err(fault))
                };
                let (slot, held) = try_open(&host, root_file_build, CreateRule::FirstRun, retry);
                assert!(held.is_none(), "{fault:?}");
                assert_eq!(slot.state(), state, "{fault:?}");
                assert_eq!(slot.cause(), Some(cause), "{fault:?}");
                assert!(slot.state().takes(slot.cause()), "{fault:?}");
                assert_eq!(slot.retry_in(), timer.then_some(retry), "{fault:?}");
                assert!(!slot.is_open(), "{fault:?}");
                let calls = host.calls();
                assert!(!calls.contains(&"create"), "{fault:?} created over a store: {calls:?}");
                assert_eq!(calls, if probe { vec!["probe"] } else { vec!["probe", "open"] });
            }
        }
        let (open, held) = try_open(
            &ModelHost::new(STANDS, Ok(())),
            root_file_build,
            CreateRule::FirstRun,
            retry,
        );
        assert_eq!(open, Slot::Open { id: HOST_ID });
        assert!(open.is_open() && matches!(held, Some(Held::Capture { .. })));

        let retried = ModelHost::new(STANDS, Err(OpenFault::Io));
        let sealed = ModelHost::new(STANDS, Err(OpenFault::Database));
        let openers = [retried.clone(), sealed.clone()].map(|host| {
            Opener::start(Box::new(host), root_file_build, CreateRule::FirstRun).unwrap()
        });
        std::thread::sleep(RETRY_FIRST + Duration::from_millis(500));
        for opener in &openers {
            assert!(opener.is_running(), "the open ended on a fault");
            assert!(opener.take().is_none());
        }
        assert_eq!(openers[0].slot().cause(), Some(StoreErrorKind::Io));
        assert!(openers[0].tries() >= 2, "an io fault was not tried again");
        assert_eq!(openers[1].slot().cause(), Some(StoreErrorKind::Database));
        assert_eq!(openers[1].tries(), 1, "a sealed store was tried again");
        assert_eq!(sealed.calls(), ["probe", "open"]);
        for opener in &openers {
            opener.stop();
        }
        let stopped = std::time::Instant::now();
        while openers.iter().any(Opener::is_running) {
            assert!(stopped.elapsed() < Duration::from_secs(2), "the open did not stop");
            std::thread::sleep(POLL);
        }
    }
}
