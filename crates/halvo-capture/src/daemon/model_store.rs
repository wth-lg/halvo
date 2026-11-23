//! The model store of the owner's tests: it keeps acknowledged batches in
//! memory, logs every call, takes scripted faults and scripted shred results,
//! and has no expiry or eviction rule of its own. Under the feature `r2` it
//! answers the read of a segment from its batches, with the id, the shreds and
//! the expiry instants that a test gave it. Beside it, the manual clocks and
//! the event builders that the owner's tests share.

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use halvo_control::model::{StorageCap, UtcDay};

use super::maintenance::Clocks;
use super::owner::{Answer, Command, Owner, OwnerHandle, OwnerParts, Report};
use super::port::{
    ByteBound, CaptureStore, KindEntry, MaintFault, MaintainStore, Ordinal, Pass, SegmentId,
    SegmentWrite, Usage, WriteFault, Written,
};
use crate::event::{AppIdentity, EventClock, FloorEvent};
use crate::floor::cases::keep;
use crate::floor::exclude::{FloorHandle, NoLineage, ProcessId};
use crate::floor::kind;
use crate::floor::payload::{KeyClass, Title, TitleWhy};
use crate::floor::record::{decode, DecodedRecord};
use crate::floor::{Switch, Switches};
use crate::secure_input::{check, FixedProbe, SecureInputReading};
use crate::sink::{self, SinkHandle};

/// A batch that the model store acknowledged.
#[derive(Debug, Clone)]
pub(crate) struct StoredBatch {
    pub(crate) segment_id: SegmentId,
    pub(crate) ordinal: Ordinal,
    pub(crate) day: UtcDay,
    pub(crate) first_capture: i64,
    pub(crate) last_capture: i64,
    pub(crate) entries: Vec<KindEntry>,
    pub(crate) payload: Vec<u8>,
}

impl StoredBatch {
    pub(crate) fn records(&self) -> Vec<DecodedRecord> {
        let text = std::str::from_utf8(&self.payload).unwrap();
        let mut lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(
            lines.pop(),
            Some(""),
            "a payload does not end with a line end"
        );
        lines
            .into_iter()
            .map(|line| decode(line).unwrap())
            .collect()
    }
}

/// One call that the owner made on the model store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Call {
    Write {
        segment_id: SegmentId,
        answer: Result<Ordinal, WriteFault>,
    },
    ExpireDue {
        now: SystemTime,
        max: usize,
    },
    EvictTo {
        bound: ByteBound,
        max: usize,
    },
    Usage,
    FreeBytes,
    #[cfg(feature = "r2")]
    ReadSegment {
        from: Ordinal,
    },
}

/// A scripted answer to the next write.
#[derive(Debug, Clone, Copy)]
pub(crate) enum WriteScript {
    Fail(WriteFault),
    KeepThenFail(WriteFault),
}

struct State {
    next_ordinal: u64,
    batches: Vec<StoredBatch>,
    log: Vec<Call>,
    writes: VecDeque<WriteScript>,
    fail_every_write: Option<WriteFault>,
    expires: VecDeque<Result<Pass, MaintFault>>,
    evicts: VecDeque<Result<Pass, MaintFault>>,
    /// The ordinals of the scripted passes that answered, for `take_shredded`.
    noted: Vec<Ordinal>,
    free: Result<u64, io::ErrorKind>,
    #[cfg(feature = "r2")]
    read: ReadModel,
}

/// What the model answers to a read of a segment beside its batches: its id,
/// the ordinals that a test shredded, the expiry instants that a test set,
/// and the fault of every read while a test sets one.
#[cfg(feature = "r2")]
struct ReadModel {
    id: super::port::StoreId,
    shredded: Vec<Ordinal>,
    expiry: Vec<(Ordinal, i64)>,
    fault: Option<MaintFault>,
}

/// The model store. Its clones share one state, so a new owner over a clone is
/// a restart over the same store.
#[derive(Clone)]
pub(crate) struct ModelStore {
    state: Arc<Mutex<State>>,
}

impl ModelStore {
    pub(crate) fn new() -> ModelStore {
        ModelStore {
            state: Arc::new(Mutex::new(State {
                next_ordinal: 1,
                batches: Vec::new(),
                log: Vec::new(),
                writes: VecDeque::new(),
                fail_every_write: None,
                expires: VecDeque::new(),
                evicts: VecDeque::new(),
                noted: Vec::new(),
                free: Ok(u64::MAX),
                #[cfg(feature = "r2")]
                read: ReadModel {
                    id: MODEL_ID,
                    shredded: Vec::new(),
                    expiry: Vec::new(),
                    fault: None,
                },
            })),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn script_writes(&self, scripts: impl IntoIterator<Item = WriteScript>) {
        self.state().writes.extend(scripts);
    }

    pub(crate) fn clear_write_scripts(&self) {
        self.state().writes.clear();
    }

    pub(crate) fn fail_every_write(&self, fault: Option<WriteFault>) {
        self.state().fail_every_write = fault;
    }

    pub(crate) fn script_expire(&self, results: Vec<Result<Pass, MaintFault>>) {
        self.state().expires.extend(results);
    }

    pub(crate) fn script_evict(&self, results: Vec<Result<Pass, MaintFault>>) {
        self.state().evicts.extend(results);
    }

    pub(crate) fn set_free_bytes(&self, free: Result<u64, io::ErrorKind>) {
        self.state().free = free;
    }

    pub(crate) fn batches(&self) -> Vec<StoredBatch> {
        self.state().batches.clone()
    }

    pub(crate) fn records(&self) -> Vec<DecodedRecord> {
        self.batches()
            .iter()
            .flat_map(StoredBatch::records)
            .collect()
    }

    pub(crate) fn log(&self) -> Vec<Call> {
        self.state().log.clone()
    }

    pub(crate) fn write_calls(&self) -> Vec<(SegmentId, Result<Ordinal, WriteFault>)> {
        self.log()
            .into_iter()
            .filter_map(|call| match call {
                Call::Write { segment_id, answer } => Some((segment_id, answer)),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn expire_calls(&self) -> Vec<SystemTime> {
        self.log()
            .into_iter()
            .filter_map(|call| match call {
                Call::ExpireDue { now, .. } => Some(now),
                _ => None,
            })
            .collect()
    }

    /// Shreds the batch with `ordinal`, as expiry or eviction does: no read
    /// answers it again. The model keeps the batch in [`ModelStore::batches`].
    #[cfg(feature = "r2")]
    pub(crate) fn shred(&self, ordinal: Ordinal) {
        self.state().read.shredded.push(ordinal);
    }

    /// Sets the expiry instant that a read answers for the batch with
    /// `ordinal`; a batch with none set answers the highest instant.
    #[cfg(feature = "r2")]
    pub(crate) fn set_expiry(&self, ordinal: Ordinal, at: i64) {
        self.state().read.expiry.push((ordinal, at));
    }

    /// Gives the model another id, as a store that was deleted and made again
    /// has.
    #[cfg(feature = "r2")]
    pub(crate) fn set_id(&self, id: super::port::StoreId) {
        self.state().read.id = id;
    }

    /// Makes every read of a segment fail with `fault`, or none fail.
    #[cfg(feature = "r2")]
    pub(crate) fn fail_reads(&self, fault: Option<MaintFault>) {
        self.state().read.fault = fault;
    }

    /// The ordinals that the reads of a segment asked for, in order.
    #[cfg(feature = "r2")]
    pub(crate) fn read_calls(&self) -> Vec<Ordinal> {
        self.log()
            .into_iter()
            .filter_map(|call| match call {
                Call::ReadSegment { from } => Some(from),
                _ => None,
            })
            .collect()
    }
}

/// The id of a model store that no test gave another.
#[cfg(feature = "r2")]
pub(crate) const MODEL_ID: super::port::StoreId = super::port::StoreId::from_bytes([0x5a; 16]);

/// A one-record write of `payload` under a fresh id, for the port tests.
pub(crate) fn write_one<S: CaptureStore>(
    store: &mut S,
    payload: &[u8],
) -> Result<Written, WriteFault> {
    let id = SegmentId::mint().unwrap();
    let entries = [KindEntry {
        kind: super::port::EntryKind::of(crate::event::Modality::ForegroundApp),
        events: 1,
        bytes: payload.len() as u64,
    }];
    store.write_batch(SegmentWrite::new(
        &id,
        UtcDay::from_days_since_epoch(0).unwrap(),
        0,
        0,
        &entries,
        payload,
    ))
}

impl MaintainStore for ModelStore {
    fn expire_due(&mut self, now: SystemTime, max_segments: usize) -> Result<Pass, MaintFault> {
        let mut state = self.state();
        state.log.push(Call::ExpireDue {
            now,
            max: max_segments,
        });
        let answer = state
            .expires
            .pop_front()
            .unwrap_or_else(|| Ok(Pass::default()));
        if let Ok(pass) = &answer {
            state.noted.extend(pass.shredded.iter().copied());
        }
        answer
    }

    fn evict_to(&mut self, bound: ByteBound, max_segments: usize) -> Result<Pass, MaintFault> {
        let mut state = self.state();
        state.log.push(Call::EvictTo {
            bound,
            max: max_segments,
        });
        let answer = state
            .evicts
            .pop_front()
            .unwrap_or_else(|| Ok(Pass::default()));
        if let Ok(pass) = &answer {
            state.noted.extend(pass.shredded.iter().copied());
        }
        answer
    }

    /// The model logs no call for the take: a scripted error names no ordinal.
    fn take_shredded(&mut self) -> Vec<Ordinal> {
        std::mem::take(&mut self.state().noted)
    }

    fn usage(&mut self) -> Result<Usage, MaintFault> {
        let mut state = self.state();
        state.log.push(Call::Usage);
        let logical = state.batches.iter().map(|b| b.payload.len() as u64).sum();
        Ok(Usage {
            disk_bytes: logical,
            logical_bytes: logical,
        })
    }
}

impl CaptureStore for ModelStore {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        let mut state = self.state();
        let answer = model_write(&mut state, &write);
        state.log.push(Call::Write {
            segment_id: *write.segment_id(),
            answer: answer.map(|written| written.ordinal),
        });
        answer
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        let mut state = self.state();
        state.log.push(Call::FreeBytes);
        state.free.map_err(io::Error::from)
    }

    #[cfg(feature = "r2")]
    fn read_segment(&mut self, from: Ordinal) -> Result<super::port::SegmentRead, MaintFault> {
        let mut state = self.state();
        state.log.push(Call::ReadSegment { from });
        if let Some(fault) = state.read.fault {
            return Err(fault);
        }
        let read = &state.read;
        let segment = state
            .batches
            .iter()
            .filter(|batch| batch.ordinal >= from && !read.shredded.contains(&batch.ordinal))
            .min_by_key(|batch| batch.ordinal)
            .map(|batch| super::port::LiveSegment {
                ordinal: batch.ordinal,
                lines: zeroize::Zeroizing::new(batch.payload.clone()),
                expiry: read
                    .expiry
                    .iter()
                    .rev()
                    .find(|(ordinal, _)| *ordinal == batch.ordinal)
                    .map_or(i64::MAX, |(_, at)| *at),
            });
        let mut gone: Vec<Ordinal> = match &segment {
            Some(live) if from == Ordinal::new(0) => read
                .shredded
                .iter()
                .copied()
                .filter(|ordinal| *ordinal > live.ordinal)
                .collect(),
            _ => Vec::new(),
        };
        gone.sort_unstable();
        gone.dedup();
        Ok(super::port::SegmentRead {
            store: read.id,
            highest: state
                .next_ordinal
                .checked_sub(1)
                .filter(|highest| *highest > 0)
                .map(Ordinal::new),
            segment,
            gone,
        })
    }
}

fn model_write(state: &mut State, write: &SegmentWrite<'_>) -> Result<Written, WriteFault> {
    if write.is_malformed() {
        return Err(WriteFault::Refused);
    }
    if let Some(stored) = state
        .batches
        .iter()
        .find(|batch| batch.segment_id == *write.segment_id())
    {
        return Err(WriteFault::AlreadyWritten {
            ordinal: stored.ordinal,
        });
    }
    let script = state.writes.pop_front();
    if let Some(WriteScript::Fail(fault)) = script {
        return Err(fault);
    }
    if let Some(fault) = state.fail_every_write {
        return Err(fault);
    }
    let ordinal = Ordinal::new(state.next_ordinal);
    state.next_ordinal += 1;
    state.batches.push(StoredBatch {
        segment_id: *write.segment_id(),
        ordinal,
        day: write.day(),
        first_capture: write.first_capture(),
        last_capture: write.last_capture(),
        entries: write.entries().to_vec(),
        payload: write.payload().to_vec(),
    });
    if let Some(WriteScript::KeepThenFail(fault)) = script {
        return Err(fault);
    }
    let logical = write.payload().len() as u64;
    Ok(Written {
        ordinal,
        logical_bytes: logical,
        disk_bytes: logical.div_ceil(4096) * 4096,
    })
}

/// Clocks that move only when a test moves them. Clones share one reading.
#[derive(Clone)]
pub(crate) struct ManualClocks {
    reading: Arc<Mutex<(Duration, SystemTime)>>,
}

impl ManualClocks {
    pub(crate) fn at(wall: SystemTime) -> ManualClocks {
        ManualClocks {
            reading: Arc::new(Mutex::new((Duration::from_secs(1000), wall))),
        }
    }

    pub(crate) fn advance(&self, by: Duration) {
        let mut reading = self.reading.lock().unwrap_or_else(PoisonError::into_inner);
        reading.0 += by;
        reading.1 += by;
    }

    pub(crate) fn step_wall_forward(&self, by: Duration) {
        self.reading
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1 += by;
    }

    pub(crate) fn step_wall_back(&self, by: Duration) {
        self.reading
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1 -= by;
    }
}

impl Clocks for ManualClocks {
    fn monotonic(&self) -> Duration {
        self.reading
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .0
    }

    fn wall(&self) -> SystemTime {
        self.reading
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
    }
}

/// 2026-11-13T12:00:00Z.
pub(crate) const NOON: u64 = 1_794_571_200;

pub(crate) fn noon() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOON)
}

fn just_after_midnight() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOON - 12 * 3600 + 1)
}

/// The clock of events stamped one second after the UTC midnight that opens
/// the day of [`NOON`].
pub(crate) static AFTER_MIDNIGHT: EventClock = EventClock::new(just_after_midnight);

/// The clock of events stamped at [`NOON`].
pub(crate) static AT_NOON: EventClock = EventClock::new(noon);

pub(crate) const APP: &str = "com.example.editor";
pub(crate) const PID: u32 = 4242;

pub(crate) fn app() -> AppIdentity {
    AppIdentity::bundle_id(APP.to_owned())
}

pub(crate) fn foreground(clock: &EventClock, age: Duration) -> FloorEvent {
    FloorEvent::foreground_app(keep::<kind::ForegroundApp>(app(), PID), Some(age), clock)
}

pub(crate) fn foreground_now() -> FloorEvent {
    FloorEvent::foreground_app(
        keep::<kind::ForegroundApp>(app(), PID),
        None,
        EventClock::system(),
    )
}

pub(crate) fn key() -> FloorEvent {
    let clearance = check(&FixedProbe(SecureInputReading::Inactive), None).unwrap();
    FloorEvent::keystroke(
        keep::<kind::Keystroke>(app(), PID),
        clearance,
        KeyClass::Printable,
        EventClock::system(),
    )
}

pub(crate) fn frame() -> FloorEvent {
    FloorEvent::screen_frame(
        keep::<kind::ScreenFrame>(app(), PID),
        None,
        EventClock::system(),
    )
}

/// A title record at the caps: an app id and a title of `bytes` bytes each.
pub(crate) fn title_of(app_bytes: usize, title_bytes: usize) -> FloorEvent {
    let app = AppIdentity::bundle_id("a".repeat(app_bytes));
    let title = Title::new("t".repeat(title_bytes), TitleWhy::TitleChange, None);
    FloorEvent::window_title(
        keep::<kind::WindowTitle>(app, PID),
        title,
        None,
        EventClock::system(),
    )
}

/// Every switch on, no root.
pub(crate) fn open_floor() -> FloorHandle {
    let mut switches = Switches::default();
    for switch in Switch::ALL {
        switches.set(switch, true);
    }
    FloorHandle::new(None, Arc::new(NoLineage), switches)
}

/// A floor whose exclusion root is the process of the test events.
pub(crate) fn floor_rooted_at_the_test_app() -> FloorHandle {
    FloorHandle::new(
        Some(ProcessId::new(PID)),
        Arc::new(NoLineage),
        open_floor().switches(),
    )
}

/// An owner over a model store and manual clocks, driven one pass at a time
/// on the test's thread, with the sink of its events.
pub(crate) struct Harness {
    pub(crate) owner: Owner<ModelStore, ManualClocks>,
    pub(crate) handle: OwnerHandle,
    pub(crate) reports: Receiver<Report>,
    pub(crate) sink: SinkHandle,
    pub(crate) store: ModelStore,
    pub(crate) clocks: ManualClocks,
}

impl Harness {
    pub(crate) fn new() -> Harness {
        Harness::over(ModelStore::new(), open_floor(), 2048)
    }

    pub(crate) fn over(store: ModelStore, floor: FloorHandle, capacity: usize) -> Harness {
        let clocks = ManualClocks::at(noon());
        Harness::with_clocks(store, floor, capacity, clocks)
    }

    pub(crate) fn with_clocks(
        store: ModelStore,
        floor: FloorHandle,
        capacity: usize,
        clocks: ManualClocks,
    ) -> Harness {
        let (sink, writer) = sink::channel(capacity);
        let (owner, handle, reports) = Owner::open(OwnerParts {
            writer,
            store: store.clone(),
            clocks: clocks.clone(),
            floor,
            cap: Some(ByteBound::of_cap(StorageCap::DEFAULT)),
            shred_chunk: super::maintenance::SHRED_CHUNK,
        });
        Harness {
            owner,
            handle,
            reports,
            sink,
            store,
            clocks,
        }
    }

    /// One pass with no wait.
    pub(crate) fn pass(&mut self) {
        assert!(self.owner.pass(Duration::ZERO), "the owner stopped");
    }

    pub(crate) fn passes(&mut self, count: usize) {
        for _ in 0..count {
            self.pass();
        }
    }

    /// Sends `command`, runs one pass and returns the answer.
    pub(crate) fn ask(&mut self, command: Command) -> Answer {
        let pending = self.handle.ask(command).unwrap();
        self.owner.pass(Duration::ZERO);
        pending
            .try_answer()
            .expect("the owner did not answer in one pass")
    }

    pub(crate) fn emit(&self, event: FloorEvent) {
        assert_eq!(self.sink.emit_event(event), sink::Emitted::Queued);
    }

    pub(crate) fn reports(&self) -> Vec<Report> {
        self.reports.try_iter().collect()
    }
}
