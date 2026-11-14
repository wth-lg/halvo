//! The store owner: the one thread that holds the capture store. Each loop
//! pass handles the waiting commands, then one received event, then the due
//! work: a flush or its retry, and one chunk of maintenance.

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use halvo_control::model::StorageCap;
use zeroize::Zeroize;

use super::barrier::Gate;
use super::batch::{utc_day, Admit, Batch, TextFilters};
use super::counters::{Counters, DiscardReason, Snapshot, SourceTallies};
use super::maintenance::{Clocks, Instants, Job, Schedule};
use super::port::{ByteBound, CaptureStore, EntryKind, MaintFault, WriteFault, PAYLOAD_MAX_BYTES};
use crate::floor::exclude::FloorHandle;
use crate::floor::record;
use crate::sink::{Barrier, Received, SinkWriter};

/// The slots of the sink's queue that the owner drains.
pub const QUEUE_SLOTS: usize = 1_024;

/// The most events that the owner holds while its drain fails: the 2,048
/// slots of the sink and one batch.
pub const HELD_MAX_EVENTS: usize = QUEUE_SLOTS + super::batch::BATCH_MAX_EVENTS;

/// The most record bytes, with their line ends, that the owner holds while its
/// drain fails: twice the batch budget.
pub const HELD_MAX_BYTES: usize = 2 * PAYLOAD_MAX_BYTES;

/// Under this many free bytes on the store's volume the owner tries no flush.
pub const FREE_SPACE_FLOOR_BYTES: u64 = 16_777_216;

/// The same failure that is not I/O this many times in a row drops the batch.
pub const REJECT_AFTER_SAME_FAULTS: u32 = 3;

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
            schedule,
            maintenance_failing: false,
            inbox: None,
            stopping: false,
            published: Arc::new(Mutex::new(Counters::default().snapshot(0))),
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

    /// Appends a run-mark record to the batch that the owner fills, numbered
    /// by the receiver at the append and placed by its `at` (contracts.md C18
    /// item 8). False, with no number taken and no counter changed, while the
    /// owner discards, and while the drain fails when the mark at the largest
    /// record's length would pass either bound of what the owner holds.
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
                Drain::Healthy => None,
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
        self.counters.snapshot(self.pending())
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

    fn handle_commands(&mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(asked) => {
                    let answer = self.handle(asked.command);
                    let _ = asked.reply.send(answer);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    if !self.stopping {
                        self.settle();
                        self.stopping = true;
                    }
                    return;
                }
            }
            if self.stopping {
                return;
            }
        }
    }

    fn handle(&mut self, command: Command) -> Answer {
        match command {
            Command::OpenPause => Answer::Paused(self.gate.open_pause(&self.writer)),
            Command::ClosePause => {
                if let Some(received) = self.inbox.take() {
                    self.take(received);
                }
                while let Ok(received) = self.writer.try_recv() {
                    self.take(received);
                }
                Answer::Resumed(self.gate.close_pause(&self.writer))
            }
            Command::Settle => Answer::Settled(self.settle()),
            Command::Shutdown => {
                let snapshot = self.settle();
                self.stopping = true;
                Answer::ShutDown(snapshot)
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
            Drain::Healthy => true,
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
        self.release(now);
    }

    fn reject(&mut self, fault: WriteFault, now: Instants) {
        let events = self.batch.events() as u64;
        let (first_capture, last_capture) = self.batch.captures();
        self.counters.lost(events, first_capture, last_capture);
        self.report(Report::BatchRejected {
            events,
            first_capture,
            last_capture,
            fault,
        });
        self.release(now);
    }

    fn release(&mut self, now: Instants) {
        self.batch.clear();
        self.frozen = false;
        if matches!(self.drain, Drain::Failing { .. }) {
            self.drain = Drain::Healthy;
            self.report(Report::DrainHealthy);
        }
        self.refill(now);
    }

    fn fail(&mut self, fault: WriteFault, now: Instants) {
        self.frozen = true;
        let counted = !matches!(fault, WriteFault::Io(_)) && !fault.is_no_space();
        let cause = cause_of(fault);
        let (retry_at, delay, same, since, changed) = match self.drain {
            Drain::Healthy => (
                now.monotonic + RETRY_FIRST,
                RETRY_FIRST,
                counted.then_some((fault, 1)),
                now.wall,
                true,
            ),
            Drain::Failing {
                retry_at,
                delay,
                same,
                cause: before,
                since,
            } if now.monotonic >= retry_at => {
                let same = match same {
                    Some((last, count)) if counted && last == fault => Some((fault, count + 1)),
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
                let same = same.filter(|(last, _)| *last == fault);
                (retry_at, delay, same, since, before != cause)
            }
        };
        if same.is_some_and(|(_, count)| count >= REJECT_AFTER_SAME_FAULTS) {
            self.reject(fault, now);
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

fn cause_of(fault: WriteFault) -> DrainCause {
    if fault.is_no_space() {
        return DrainCause::NoSpace;
    }
    match fault {
        WriteFault::Io(_) => DrainCause::Io,
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
            let ready = crate::daemon::signal::peak_rss_bytes().unwrap();
            let started = std::time::Instant::now();
            let mut queued_high_water = 0u64;
            for record in 0..RECORDS {
                if record % 1024 == 0 {
                    let counts = sink.counts();
                    let accepted: u64 = Modality::ALL
                        .into_iter()
                        .map(|kind| counts.of(kind).accepted)
                        .sum();
                    let received = thread.snapshot().events_received();
                    queued_high_water = queued_high_water.max(accepted.saturating_sub(received));
                }
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
            assert_eq!(settled.events_persisted, RECORDS);
            println!(
                "owner_growth_bytes={} records={RECORDS} elapsed_ms={}",
                peak.saturating_sub(ready),
                started.elapsed().as_millis()
            );
            println!(
                "owner_growth_detail peak_before_first_record={ready} peak_after_settle={peak} queued_high_water={queued_high_water} queue_slots={QUEUE_SLOTS}"
            );
            let _ = thread.handle.call(Command::Shutdown);
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "the child failed: {text}");
        let line = text
            .lines()
            .find_map(|line| line.find("owner_growth_bytes=").map(|at| &line[at..]))
            .expect("the child printed no growth line");
        println!("{line}");
        if let Some(detail) = text
            .lines()
            .find_map(|line| line.find("owner_growth_detail").map(|at| &line[at..]))
        {
            println!("{detail}");
        }
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
}
