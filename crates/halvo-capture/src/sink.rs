//! Backpressure: the channel is bounded. A droppable modality (frames, audio,
//! camera) is dropped when the queue is full; a fold record of
//! [`SinkHandle::emit_continuous`] is handed back to its source; every other
//! event blocks the caller until the writer makes room.

use std::cell::Cell;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver, RecvTimeoutError, SendError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use uuid::Uuid;

use crate::event::{
    from_unix_nanos, CaptureStreamEvent, CapturedAt, FloorEvent, FoldEvent, Modality,
    MonotonicSequence, RetentionClass, SegmentKeyId, SegmentRef,
};
use crate::source::{EmitOutcome, EventSink};

#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    #[error("the capture writer has stopped; no more events can be queued")]
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backpressure {
    Drop,
    Block,
}

fn backpressure(kind: Modality) -> Backpressure {
    match kind {
        Modality::ScreenFrame => Backpressure::Drop,
        Modality::Keystroke => Backpressure::Block,
        Modality::Pointer => Backpressure::Block,
        Modality::ForegroundApp => Backpressure::Block,
        Modality::WindowTitle => Backpressure::Block,
        Modality::SystemAudio => Backpressure::Drop,
        Modality::Microphone => Backpressure::Drop,
        Modality::Camera => Backpressure::Drop,
    }
}

const KINDS: usize = 8;

fn kind_index(kind: Modality) -> usize {
    match kind {
        Modality::ScreenFrame => 0,
        Modality::Keystroke => 1,
        Modality::Pointer => 2,
        Modality::ForegroundApp => 3,
        Modality::WindowTitle => 4,
        Modality::SystemAudio => 5,
        Modality::Microphone => 6,
        Modality::Camera => 7,
    }
}

#[derive(Default)]
struct KindCounters {
    accepted: AtomicU64,
    dropped: AtomicU64,
    coalesced: AtomicU64,
    refused: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KindCounts {
    /// Emits that returned `Queued`.
    pub accepted: u64,
    /// Emits that returned `Dropped`: an event of a droppable kind met a full
    /// queue.
    pub dropped: u64,
    /// Fold records of [`SinkHandle::emit_continuous`] that a full queue handed
    /// back to their source; it reads 0 until a source emits fold records.
    pub coalesced: u64,
    /// Emits that returned `Disconnected` or `Refused`: the writer was gone when
    /// the emit came, or went while the emit was blocked.
    pub refused: u64,
}

/// The counts of every kind, read by [`SinkHandle::counts`]. Every clone of a
/// handle adds to the same counts, which start at 0 when the sink is built and
/// only rise. An emit is counted when it returns, so an event can be received
/// before its emit is counted. The counters are read one after another, not at
/// one moment: a sum of counters is a value that the total held while it was
/// read, and a difference or a comparison of two counters is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkCounts {
    kinds: [KindCounts; KINDS],
}

impl SinkCounts {
    pub fn of(&self, kind: Modality) -> KindCounts {
        self.kinds[kind_index(kind)]
    }
}

/// What the channel carries: an event of the old emit, or a floor value.
enum Item {
    Old(CaptureStreamEvent),
    Floor(FloorEvent),
    Fold(FoldEvent),
}

/// The answer of [`SinkHandle::emit_event`]. It is a plain value: a source
/// counts it or ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emitted {
    Queued,
    /// A screen frame met a full queue.
    Dropped,
    /// The writer was gone.
    Refused,
}

/// The answer of [`SinkHandle::emit_continuous`]. It is a plain value.
#[derive(Debug)]
pub enum FoldEmitted {
    Queued,
    /// The queue was full: the record comes back, and its source adds its count
    /// to the next interval.
    Coalesced(FoldEvent),
    /// The writer was gone.
    Refused,
}

#[derive(Clone)]
pub struct SinkHandle {
    tx: SyncSender<Item>,
    sequence: Arc<AtomicU64>,
    counts: Arc<[KindCounters; KINDS]>,
}

impl SinkHandle {
    /// The next monotonic sequence number for the stream.
    pub fn next_sequence(&self) -> MonotonicSequence {
        MonotonicSequence::new(self.sequence.fetch_add(1, Ordering::Relaxed))
    }

    pub fn dropped_count(&self) -> u64 {
        self.counts
            .iter()
            .map(|kind| kind.dropped.load(Ordering::Relaxed))
            .sum()
    }

    pub fn counts(&self) -> SinkCounts {
        SinkCounts {
            kinds: std::array::from_fn(|index| {
                let kind = &self.counts[index];
                KindCounts {
                    accepted: kind.accepted.load(Ordering::Relaxed),
                    dropped: kind.dropped.load(Ordering::Relaxed),
                    coalesced: kind.coalesced.load(Ordering::Relaxed),
                    refused: kind.refused.load(Ordering::Relaxed),
                }
            }),
        }
    }

    /// Emits one floor value. A screen frame is dropped on a full queue; every
    /// other kind blocks the calling thread until the writer makes room. When
    /// the writer is gone the value is refused.
    pub fn emit_event(&self, event: FloorEvent) -> Emitted {
        match self.send(event.kind(), Item::Floor(event)) {
            Ok(EmitOutcome::Queued) => Emitted::Queued,
            Ok(EmitOutcome::Dropped) => Emitted::Dropped,
            Err(SinkError::Disconnected) => Emitted::Refused,
        }
    }

    /// Emits one fold record and never blocks: on a full queue the record comes
    /// back in [`FoldEmitted::Coalesced`] and counts as coalesced for pointer.
    pub fn emit_continuous(&self, fold: FoldEvent) -> FoldEmitted {
        let kind = Modality::Pointer;
        match self.tx.try_send(Item::Fold(fold)) {
            Ok(()) => {
                self.counters(kind).accepted.fetch_add(1, Ordering::Relaxed);
                FoldEmitted::Queued
            }
            Err(TrySendError::Full(item)) => {
                self.counters(kind)
                    .coalesced
                    .fetch_add(1, Ordering::Relaxed);
                FoldEmitted::Coalesced(handed_back(item))
            }
            Err(TrySendError::Disconnected(_)) => {
                self.counters(kind).refused.fetch_add(1, Ordering::Relaxed);
                FoldEmitted::Refused
            }
        }
    }

    fn send(&self, kind: Modality, item: Item) -> Result<EmitOutcome, SinkError> {
        match backpressure(kind) {
            Backpressure::Drop => match self.tx.try_send(item) {
                Ok(()) => self.accepted(kind),
                Err(TrySendError::Full(_)) => self.dropped(kind),
                Err(TrySendError::Disconnected(_)) => self.refused(kind),
            },
            Backpressure::Block => match self.tx.send(item) {
                Ok(()) => self.accepted(kind),
                Err(SendError(_)) => self.refused(kind),
            },
        }
    }

    fn counters(&self, kind: Modality) -> &KindCounters {
        &self.counts[kind_index(kind)]
    }

    fn accepted(&self, kind: Modality) -> Result<EmitOutcome, SinkError> {
        self.counters(kind).accepted.fetch_add(1, Ordering::Relaxed);
        Ok(EmitOutcome::Queued)
    }

    fn dropped(&self, kind: Modality) -> Result<EmitOutcome, SinkError> {
        self.counters(kind).dropped.fetch_add(1, Ordering::Relaxed);
        Ok(EmitOutcome::Dropped)
    }

    fn refused(&self, kind: Modality) -> Result<EmitOutcome, SinkError> {
        self.counters(kind).refused.fetch_add(1, Ordering::Relaxed);
        Err(SinkError::Disconnected)
    }
}

fn handed_back(item: Item) -> FoldEvent {
    match item {
        Item::Fold(fold) => fold,
        Item::Old(_) | Item::Floor(_) => unreachable!("emit_continuous sends only a fold"),
    }
}

impl EventSink for SinkHandle {
    fn emit(&self, event: CaptureStreamEvent) -> Result<EmitOutcome, SinkError> {
        self.send(event.modality, Item::Old(event))
    }
}

/// The number that the writer gives an event when it receives it through
/// [`SinkWriter::recv_timeout`] or [`SinkWriter::try_recv`]. Numbers rise by
/// exactly one in the order in which events arrive there; a dropped, coalesced
/// or refused event never gets one. The count restarts at 0 in every process and
/// with every sink, so a number is no read position: never store one as a
/// cursor or compare numbers of two processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArrivalNumber(u64);

impl ArrivalNumber {
    pub const fn get(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    pub(crate) const fn for_test(value: u64) -> Self {
        Self(value)
    }
}

/// An event as the writer received it, with its [`ArrivalNumber`]. Its own
/// `captured_at.sequence` is the stamp that the emitting source took before a
/// send that can block, not the arrival number: order by [`Received::number`].
/// A floor value shows through [`Received::event`] as an event of its kind,
/// time and id with no app and no payload, and [`Received::into_floor`] hands
/// it over.
#[derive(Debug)]
pub struct Received {
    arrived: Arrived,
    number: ArrivalNumber,
}

#[derive(Debug)]
enum Arrived {
    Old(CaptureStreamEvent),
    Floor {
        shown: CaptureStreamEvent,
        event: FloorEvent,
    },
}

impl Received {
    pub fn number(&self) -> ArrivalNumber {
        self.number
    }

    pub fn event(&self) -> &CaptureStreamEvent {
        match &self.arrived {
            Arrived::Old(event) => event,
            Arrived::Floor { shown, .. } => shown,
        }
    }

    pub fn is_before(&self, barrier: Barrier) -> bool {
        self.number.0 < barrier.0
    }

    /// The floor value with its number, or this value back, boxed, when it
    /// holds an event of the old emit.
    pub fn into_floor(self) -> Result<FloorReceived, Box<Received>> {
        match self.arrived {
            Arrived::Floor { event, .. } => Ok(FloorReceived {
                event,
                number: self.number,
            }),
            Arrived::Old(event) => Err(Box::new(Received {
                arrived: Arrived::Old(event),
                number: self.number,
            })),
        }
    }
}

/// A floor value as the writer received it, with its [`ArrivalNumber`]: the one
/// input of the record encoder. Only the writer makes one.
#[derive(Debug)]
pub struct FloorReceived {
    event: FloorEvent,
    number: ArrivalNumber,
}

impl FloorReceived {
    #[cfg(test)]
    pub(crate) fn for_test(event: FloorEvent, number: ArrivalNumber) -> FloorReceived {
        FloorReceived { event, number }
    }

    pub fn number(&self) -> ArrivalNumber {
        self.number
    }

    pub fn event(&self) -> &FloorEvent {
        &self.event
    }

    pub fn is_before(&self, barrier: Barrier) -> bool {
        self.number.0 < barrier.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Barrier(u64);

pub struct SinkWriter {
    rx: Receiver<Item>,
    next: Cell<u64>,
}

const _: fn() = || {
    trait AmbiguousIfCloneOrSync<Marker> {
        fn check() {}
    }
    impl<T> AmbiguousIfCloneOrSync<()> for T {}
    struct IsClone;
    impl<T: Clone> AmbiguousIfCloneOrSync<IsClone> for T {}
    struct IsSync;
    impl<T: Sync> AmbiguousIfCloneOrSync<IsSync> for T {}
    let _ = <SinkWriter as AmbiguousIfCloneOrSync<_>>::check;
};

impl SinkWriter {
    /// The event gets no [`ArrivalNumber`] and takes none; a floor value
    /// arrives as an event of its kind, time and id with no app and no payload.
    pub fn recv(&self) -> Option<CaptureStreamEvent> {
        self.rx.recv().ok().map(|item| match arrived(item) {
            Arrived::Old(event) => event,
            Arrived::Floor { shown, .. } => shown,
        })
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Received, RecvTimeoutError> {
        self.rx.recv_timeout(timeout).map(|item| self.number(item))
    }

    pub fn try_recv(&self) -> Result<Received, TryRecvError> {
        self.rx.try_recv().map(|item| self.number(item))
    }

    /// Draw a barrier at the next number that this writer will give. Every event
    /// received before the draw is before it. An event that was still queued, or
    /// still blocked in an emit, at the draw is not before it, so an owner that
    /// discards the events at or after a barrier discards those too, which errs
    /// toward not persisting.
    pub fn barrier(&self) -> Barrier {
        Barrier(self.next.get())
    }

    /// The next number of this writer's counter, for a record that did not
    /// arrive on the channel: the run mark that the owner's entry appends.
    #[cfg(feature = "r2")]
    pub(crate) fn number_next(&self) -> ArrivalNumber {
        let number = self.next.get();
        self.next.set(number + 1);
        ArrivalNumber(number)
    }

    fn number(&self, item: Item) -> Received {
        let number = self.next.get();
        self.next.set(number + 1);
        Received {
            arrived: arrived(item),
            number: ArrivalNumber(number),
        }
    }
}

fn arrived(item: Item) -> Arrived {
    match item {
        Item::Old(event) => Arrived::Old(event),
        Item::Floor(event) => shown(event),
        Item::Fold(fold) => shown(fold.into_event()),
    }
}

fn shown(event: FloorEvent) -> Arrived {
    let shown = CaptureStreamEvent {
        event_id: event.id().uuid(),
        captured_at: CapturedAt {
            wall_clock: from_unix_nanos(event.at()).unwrap_or(UNIX_EPOCH),
            sequence: MonotonicSequence::new(0),
        },
        modality: event.kind(),
        foreground_app: String::new(),
        segment_ref: SegmentRef {
            segment_id: Uuid::nil(),
            byte_offset: 0,
            byte_len: 0,
        },
        segment_key_id: SegmentKeyId(0),
        is_derived: false,
        retention_class: RetentionClass::Raw,
    };
    Arrived::Floor { shown, event }
}

/// With a capacity of 0 a droppable event is dropped unless the writer is
/// waiting in a receive.
pub fn channel(capacity: usize) -> (SinkHandle, SinkWriter) {
    build(capacity)
}

pub fn bounded(capacity: NonZeroUsize) -> (SinkHandle, SinkWriter) {
    build(capacity.get())
}

fn build(capacity: usize) -> (SinkHandle, SinkWriter) {
    let (tx, rx) = sync_channel(capacity);
    let handle = SinkHandle {
        tx,
        sequence: Arc::new(AtomicU64::new(0)),
        counts: Arc::default(),
    };
    let writer = SinkWriter {
        rx,
        next: Cell::new(0),
    };
    (handle, writer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{
        CaptureStreamEvent, CapturedAt, Modality, RetentionClass, SegmentKeyId, SegmentRef,
    };
    use std::num::NonZeroUsize;
    use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
    use std::thread;
    use std::time::{Duration, UNIX_EPOCH};
    use uuid::Uuid;

    const EVERY_KIND: [Modality; 8] = [
        Modality::ScreenFrame,
        Modality::Keystroke,
        Modality::Pointer,
        Modality::ForegroundApp,
        Modality::WindowTitle,
        Modality::SystemAudio,
        Modality::Microphone,
        Modality::Camera,
    ];

    fn unnumbered(kind: Modality, emitter: i64, index: u64) -> CaptureStreamEvent {
        CaptureStreamEvent {
            event_id: Uuid::nil(),
            captured_at: CapturedAt {
                wall_clock: UNIX_EPOCH,
                sequence: MonotonicSequence::new(0),
            },
            modality: kind,
            foreground_app: String::new(),
            segment_ref: SegmentRef {
                segment_id: Uuid::nil(),
                byte_offset: index,
                byte_len: 0,
            },
            segment_key_id: SegmentKeyId(emitter),
            is_derived: false,
            retention_class: RetentionClass::Raw,
        }
    }

    #[test]
    fn received_numbers_rise_by_one_under_four_emitters() {
        const EMITTERS: usize = 4;
        const PER_EMITTER: u64 = 100_000;
        const KINDS_OF_EMITTERS: [Modality; EMITTERS] = [
            Modality::Keystroke,
            Modality::Keystroke,
            Modality::ScreenFrame,
            Modality::ScreenFrame,
        ];
        let (handle, writer) = bounded(NonZeroUsize::new(400_000).unwrap());
        let (alive, all_ended) = mpsc::channel::<()>();
        let emitters: Vec<_> = (0..EMITTERS)
            .map(|emitter| {
                let handle = handle.clone();
                let alive = alive.clone();
                thread::spawn(move || {
                    let _alive = alive;
                    let kind = KINDS_OF_EMITTERS[emitter];
                    let tag = i64::try_from(emitter).unwrap();
                    for index in 0..PER_EMITTER {
                        let outcome = handle.emit(unnumbered(kind, tag, index)).unwrap();
                        assert_eq!(outcome, EmitOutcome::Queued);
                    }
                })
            })
            .collect();
        drop(alive);
        let mut next_index = [0u64; EMITTERS];
        let mut expected = 0u64;
        let mut idle_polls = 0;
        loop {
            let ended = matches!(all_ended.try_recv(), Err(TryRecvError::Disconnected));
            match writer.recv_timeout(Duration::from_millis(100)) {
                Ok(received) => {
                    idle_polls = 0;
                    assert_eq!(received.number(), ArrivalNumber::for_test(expected));
                    let event = received.event();
                    let emitter = usize::try_from(event.segment_key_id.0).unwrap();
                    assert_eq!(event.segment_ref.byte_offset, next_index[emitter]);
                    next_index[emitter] += 1;
                    expected += 1;
                }
                Err(RecvTimeoutError::Timeout) if ended => break,
                Err(RecvTimeoutError::Timeout) => {
                    idle_polls += 1;
                    assert!(idle_polls < 100, "no event in 10 s while emitters ran");
                }
                Err(RecvTimeoutError::Disconnected) => panic!("the test holds a handle"),
            }
        }
        for emitter in emitters {
            emitter.join().unwrap();
        }
        assert_eq!(next_index, [PER_EMITTER; EMITTERS]);
        assert_eq!(expected, 400_000);
        let accepted = |accepted| KindCounts {
            accepted,
            ..KindCounts::default()
        };
        let counts = handle.counts();
        assert_eq!(counts.of(Modality::Keystroke), accepted(200_000));
        assert_eq!(counts.of(Modality::Pointer), KindCounts::default());
        assert_eq!(counts.of(Modality::ScreenFrame), accepted(200_000));
        assert_eq!(handle.dropped_count(), 0);
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn everything_received_before_a_barrier_is_below_it() {
        const EMITTERS: usize = 4;
        const PER_EMITTER: u64 = 10_000;
        const BEFORE_THE_DRAW: usize = 5_000;
        let (handle, writer) = bounded(NonZeroUsize::new(64).unwrap());
        let emitters: Vec<_> = (0..EMITTERS)
            .map(|emitter| {
                let handle = handle.clone();
                thread::spawn(move || {
                    let tag = i64::try_from(emitter).unwrap();
                    for index in 0..PER_EMITTER {
                        handle
                            .emit(unnumbered(Modality::Keystroke, tag, index))
                            .unwrap();
                    }
                })
            })
            .collect();
        drop(handle);
        let receive = || writer.recv_timeout(Duration::from_secs(10));
        let before: Vec<Received> = (0..BEFORE_THE_DRAW)
            .map(|_| receive().expect("an event inside 10 s"))
            .collect();
        let barrier = writer.barrier();
        assert_eq!(writer.barrier(), barrier);
        let mut after = Vec::new();
        loop {
            match receive() {
                Ok(received) => after.push(received),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => panic!("no event and no disconnect in 10 s"),
            }
        }
        for emitter in emitters {
            emitter.join().unwrap();
        }
        assert_eq!(before.len() + after.len(), 40_000);
        assert!(before.iter().all(|received| received.is_before(barrier)));
        assert!(after.iter().all(|received| !received.is_before(barrier)));
    }

    #[test]
    fn a_refused_keystroke_returns_disconnected_and_is_counted() {
        let (handle, writer) = bounded(NonZeroUsize::new(4).unwrap());
        drop(writer);
        let refused = handle.emit(unnumbered(Modality::Keystroke, 0, 0));
        assert!(matches!(refused, Err(SinkError::Disconnected)));
        let only_refused = KindCounts {
            refused: 1,
            ..KindCounts::default()
        };
        assert_eq!(handle.counts().of(Modality::Keystroke), only_refused);
        for kind in EVERY_KIND {
            if kind == Modality::Keystroke {
                continue;
            }
            assert_eq!(handle.counts().of(kind), KindCounts::default(), "{kind:?}");
            let refused = handle.emit(unnumbered(kind, 0, 0));
            assert!(matches!(refused, Err(SinkError::Disconnected)), "{kind:?}");
        }
        let counts = handle.counts();
        for kind in EVERY_KIND {
            assert_eq!(counts.of(kind), only_refused, "{kind:?}");
        }
        assert_eq!(handle.dropped_count(), 0);
        for kind in [Modality::Keystroke, Modality::ScreenFrame] {
            let returned = outcomes_from_two_threads(&handle, kind, 10_000);
            let refused_by_both = KindCounts {
                refused: 20_000,
                ..KindCounts::default()
            };
            assert_eq!(returned, refused_by_both, "{kind:?}");
            let counted = KindCounts {
                refused: 20_001,
                ..KindCounts::default()
            };
            assert_eq!(handle.counts().of(kind), counted, "{kind:?}");
        }
    }

    fn outcomes_from_two_threads(handle: &SinkHandle, kind: Modality, each: u64) -> KindCounts {
        let threads: Vec<_> = (0..2_i64)
            .map(|emitter| {
                let handle = handle.clone();
                thread::spawn(move || {
                    let mut returned = KindCounts::default();
                    for index in 0..each {
                        match handle.emit(unnumbered(kind, emitter, index)) {
                            Ok(EmitOutcome::Queued) => returned.accepted += 1,
                            Ok(EmitOutcome::Dropped) => returned.dropped += 1,
                            Err(SinkError::Disconnected) => returned.refused += 1,
                        }
                    }
                    returned
                })
            })
            .collect();
        let mut total = KindCounts::default();
        for thread in threads {
            let part = thread.join().unwrap();
            total.accepted += part.accepted;
            total.dropped += part.dropped;
            total.refused += part.refused;
        }
        total
    }

    struct Emitting {
        started: mpsc::Receiver<()>,
        outcome: mpsc::Receiver<Result<EmitOutcome, SinkError>>,
        thread: thread::JoinHandle<()>,
    }

    fn emit_on_a_thread(handle: &SinkHandle, event: CaptureStreamEvent) -> Emitting {
        let (started_tx, started) = mpsc::channel();
        let (outcome_tx, outcome) = mpsc::channel();
        let handle = handle.clone();
        let thread = thread::spawn(move || {
            let _ = started_tx.send(());
            let _ = outcome_tx.send(handle.emit(event));
        });
        Emitting {
            started,
            outcome,
            thread,
        }
    }

    #[test]
    fn an_emitter_parked_on_a_full_queue_is_refused_and_counted_when_the_writer_drops() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        let frame = unnumbered(Modality::ScreenFrame, 0, 0);
        assert_eq!(handle.emit(frame).unwrap(), EmitOutcome::Queued);
        let parked = emit_on_a_thread(&handle, unnumbered(Modality::Keystroke, 0, 0));
        parked
            .started
            .recv_timeout(Duration::from_secs(1))
            .expect("the emitter started inside 1 s");
        assert!(
            matches!(
                parked.outcome.recv_timeout(Duration::from_millis(300)),
                Err(RecvTimeoutError::Timeout)
            ),
            "the keystroke emit returned while the queue was full and the writer alive"
        );
        drop(writer);
        let outcome = parked
            .outcome
            .recv_timeout(Duration::from_secs(1))
            .expect("the parked emit returned inside 1 s of the writer dropping");
        assert!(matches!(outcome, Err(SinkError::Disconnected)));
        parked.thread.join().unwrap();
        let counts = handle.counts();
        let refused_once = KindCounts {
            refused: 1,
            ..KindCounts::default()
        };
        let accepted_once = KindCounts {
            accepted: 1,
            ..KindCounts::default()
        };
        assert_eq!(counts.of(Modality::Keystroke), refused_once);
        assert_eq!(counts.of(Modality::ScreenFrame), accepted_once);
    }

    #[test]
    fn the_drop_policy_covers_every_kind() {
        let table = [
            (Modality::ScreenFrame, Backpressure::Drop),
            (Modality::Keystroke, Backpressure::Block),
            (Modality::Pointer, Backpressure::Block),
            (Modality::ForegroundApp, Backpressure::Block),
            (Modality::WindowTitle, Backpressure::Block),
            (Modality::SystemAudio, Backpressure::Drop),
            (Modality::Microphone, Backpressure::Drop),
            (Modality::Camera, Backpressure::Drop),
        ];
        assert_eq!(EVERY_KIND.len(), KINDS);
        for (index, kind) in EVERY_KIND.into_iter().enumerate() {
            assert_eq!(kind_index(kind), index, "{kind:?}");
            assert_eq!(table[index].0, kind);
        }
        for (kind, policy) in table {
            assert_eq!(backpressure(kind), policy, "{kind:?}");
        }
    }

    #[test]
    fn a_blocking_kind_on_a_full_queue_waits_and_is_queued_once() {
        let blocking = [
            Modality::Keystroke,
            Modality::Pointer,
            Modality::ForegroundApp,
            Modality::WindowTitle,
        ];
        for kind in blocking {
            let (handle, writer) = bounded(NonZeroUsize::MIN);
            let frame = unnumbered(Modality::ScreenFrame, 0, 0);
            assert_eq!(handle.emit(frame).unwrap(), EmitOutcome::Queued);
            let waiting = emit_on_a_thread(&handle, unnumbered(kind, 1, 0));
            waiting
                .started
                .recv_timeout(Duration::from_secs(1))
                .expect("the emitter started inside 1 s");
            assert!(
                matches!(
                    waiting.outcome.recv_timeout(Duration::from_millis(300)),
                    Err(RecvTimeoutError::Timeout)
                ),
                "{kind:?}: the emit returned while the queue was full"
            );
            assert_eq!(handle.dropped_count(), 0, "{kind:?}");
            let first = writer
                .recv_timeout(Duration::from_secs(1))
                .expect("the frame inside 1 s");
            assert_eq!(first.event().modality, Modality::ScreenFrame, "{kind:?}");
            let outcome = waiting
                .outcome
                .recv_timeout(Duration::from_secs(1))
                .expect("the emit returned inside 1 s of the writer making room");
            assert!(matches!(outcome, Ok(EmitOutcome::Queued)), "{kind:?}");
            let second = writer
                .recv_timeout(Duration::from_secs(1))
                .expect("the event inside 1 s");
            assert_eq!(second.event().modality, kind);
            assert_eq!(second.number(), ArrivalNumber::for_test(1), "{kind:?}");
            assert!(
                matches!(writer.try_recv(), Err(TryRecvError::Empty)),
                "{kind:?}: queued more than once"
            );
            waiting.thread.join().unwrap();
            let accepted_once = KindCounts {
                accepted: 1,
                ..KindCounts::default()
            };
            assert_eq!(handle.counts().of(kind), accepted_once, "{kind:?}");
        }
    }

    #[test]
    fn a_frame_on_a_full_queue_is_dropped_inside_a_bound() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        let kept = unnumbered(Modality::ScreenFrame, 0, 0);
        assert_eq!(handle.emit(kept).unwrap(), EmitOutcome::Queued);
        let dropping = emit_on_a_thread(&handle, unnumbered(Modality::ScreenFrame, 0, 1));
        let outcome = dropping
            .outcome
            .recv_timeout(Duration::from_secs(1))
            .expect("the second frame emit returned inside 1 s");
        assert!(matches!(outcome, Ok(EmitOutcome::Dropped)));
        dropping.thread.join().unwrap();
        assert_eq!(handle.dropped_count(), 1);
        let frames = KindCounts {
            accepted: 1,
            dropped: 1,
            ..KindCounts::default()
        };
        assert_eq!(handle.counts().of(Modality::ScreenFrame), frames);
        let returned = outcomes_from_two_threads(&handle, Modality::ScreenFrame, 10_000);
        let dropped_by_both = KindCounts {
            dropped: 20_000,
            ..KindCounts::default()
        };
        assert_eq!(returned, dropped_by_both);
        let frames = KindCounts {
            accepted: 1,
            dropped: 20_001,
            ..KindCounts::default()
        };
        assert_eq!(handle.counts().of(Modality::ScreenFrame), frames);
        assert_eq!(handle.dropped_count(), 20_001);
        let received = writer.try_recv().unwrap();
        assert_eq!(received.event().segment_ref.byte_offset, 0);
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn every_droppable_kind_on_a_full_queue_is_dropped_and_counted_as_itself() {
        let droppable = [
            Modality::ScreenFrame,
            Modality::SystemAudio,
            Modality::Microphone,
            Modality::Camera,
        ];
        for kind in droppable {
            let (handle, writer) = bounded(NonZeroUsize::MIN);
            let kept = unnumbered(kind, 0, 0);
            assert_eq!(handle.emit(kept).unwrap(), EmitOutcome::Queued, "{kind:?}");
            let dropping = emit_on_a_thread(&handle, unnumbered(kind, 0, 1));
            let outcome = dropping
                .outcome
                .recv_timeout(Duration::from_secs(1))
                .expect("the second emit of a droppable kind returned inside 1 s");
            assert!(matches!(outcome, Ok(EmitOutcome::Dropped)), "{kind:?}");
            dropping.thread.join().unwrap();
            let counts = handle.counts();
            let one_of_each = KindCounts {
                accepted: 1,
                dropped: 1,
                ..KindCounts::default()
            };
            assert_eq!(counts.of(kind), one_of_each, "{kind:?}");
            if kind != Modality::ScreenFrame {
                let frames = counts.of(Modality::ScreenFrame);
                assert_eq!(frames, KindCounts::default(), "{kind:?}");
            }
            assert_eq!(handle.dropped_count(), 1, "{kind:?}");
            let received = writer.try_recv().unwrap();
            assert_eq!(received.event().modality, kind);
            assert_eq!(received.event().segment_ref.byte_offset, 0, "{kind:?}");
            assert!(
                matches!(writer.try_recv(), Err(TryRecvError::Empty)),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_frame_on_a_full_queue_whose_writer_is_gone_is_refused_not_dropped() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        let kept = unnumbered(Modality::ScreenFrame, 0, 0);
        assert_eq!(handle.emit(kept).unwrap(), EmitOutcome::Queued);
        drop(writer);
        let refused = handle.emit(unnumbered(Modality::ScreenFrame, 0, 1));
        assert!(matches!(refused, Err(SinkError::Disconnected)));
        let frames = KindCounts {
            accepted: 1,
            refused: 1,
            ..KindCounts::default()
        };
        assert_eq!(handle.counts().of(Modality::ScreenFrame), frames);
        assert_eq!(handle.dropped_count(), 0);
    }

    #[test]
    fn drops_of_two_kinds_add_up_in_dropped_count() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        let kept = unnumbered(Modality::ScreenFrame, 0, 0);
        assert_eq!(handle.emit(kept).unwrap(), EmitOutcome::Queued);
        let (done_tx, done) = mpsc::channel();
        let emitter = {
            let handle = handle.clone();
            thread::spawn(move || {
                let mut outcomes = Vec::new();
                for (kind, times) in [(Modality::SystemAudio, 3), (Modality::Microphone, 2)] {
                    for index in 0..times {
                        outcomes.push((kind, handle.emit(unnumbered(kind, 0, index))));
                    }
                }
                let _ = done_tx.send(outcomes);
            })
        };
        let outcomes = done
            .recv_timeout(Duration::from_secs(1))
            .expect("the five emits on a full queue returned inside 1 s");
        emitter.join().unwrap();
        for (kind, outcome) in &outcomes {
            assert!(matches!(outcome, Ok(EmitOutcome::Dropped)), "{kind:?}");
        }
        let counts = handle.counts();
        assert_eq!(counts.of(Modality::SystemAudio).dropped, 3);
        assert_eq!(counts.of(Modality::Microphone).dropped, 2);
        assert_eq!(counts.of(Modality::ScreenFrame).dropped, 0);
        assert_eq!(handle.dropped_count(), 5);
        assert_eq!(outcomes.len(), 5);
        drop(writer);
    }

    #[test]
    fn a_burst_of_each_blocking_kind_on_a_one_slot_queue_is_queued_and_received_whole() {
        const BURST: u64 = 5_000;
        let blocking = [
            Modality::Keystroke,
            Modality::Pointer,
            Modality::ForegroundApp,
            Modality::WindowTitle,
        ];
        for kind in blocking {
            let (handle, writer) = bounded(NonZeroUsize::MIN);
            let emitter = {
                let handle = handle.clone();
                thread::spawn(move || {
                    let mut returned = KindCounts::default();
                    for index in 0..BURST {
                        match handle.emit(unnumbered(kind, 0, index)) {
                            Ok(EmitOutcome::Queued) => returned.accepted += 1,
                            Ok(EmitOutcome::Dropped) => returned.dropped += 1,
                            Err(SinkError::Disconnected) => returned.refused += 1,
                        }
                    }
                    returned
                })
            };
            drop(handle);
            let mut received = 0u64;
            let mut in_order = true;
            loop {
                match writer.recv_timeout(Duration::from_secs(10)) {
                    Ok(event) => {
                        in_order &= event.event().modality == kind;
                        in_order &= event.event().segment_ref.byte_offset == received;
                        in_order &= event.number() == ArrivalNumber::for_test(received);
                        received += 1;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        panic!("{kind:?}: no event and no disconnect in 10 s")
                    }
                }
            }
            let returned = emitter.join().unwrap();
            let all_queued = KindCounts {
                accepted: BURST,
                ..KindCounts::default()
            };
            assert_eq!(
                returned, all_queued,
                "{kind:?}: outcomes that the emits returned"
            );
            assert_eq!(received, BURST, "{kind:?}");
            assert!(in_order, "{kind:?}: out of order or with skipped numbers");
        }
    }

    #[test]
    fn a_queue_of_capacity_zero_drops_a_frame_unless_a_receive_waits() {
        let (handle, writer) = channel(0);
        let unwaited = emit_on_a_thread(&handle, unnumbered(Modality::ScreenFrame, 0, 0));
        let outcome = unwaited
            .outcome
            .recv_timeout(Duration::from_secs(1))
            .expect("a frame emit with no receive waiting returned inside 1 s");
        assert!(matches!(outcome, Ok(EmitOutcome::Dropped)));
        unwaited.thread.join().unwrap();
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
        let emitter = {
            let handle = handle.clone();
            thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                loop {
                    match handle.emit(unnumbered(Modality::ScreenFrame, 0, 1)) {
                        Ok(EmitOutcome::Queued) => return true,
                        Ok(EmitOutcome::Dropped) if std::time::Instant::now() < deadline => {
                            thread::sleep(Duration::from_millis(1));
                        }
                        _ => return false,
                    }
                }
            })
        };
        let received = writer
            .recv_timeout(Duration::from_secs(5))
            .expect("a frame emitted while a receive waited");
        assert_eq!(received.event().segment_ref.byte_offset, 1);
        assert_eq!(received.number(), ArrivalNumber::for_test(0));
        assert!(
            emitter.join().unwrap(),
            "no frame was queued while a receive waited"
        );
        let frames = handle.counts().of(Modality::ScreenFrame);
        assert_eq!(frames.accepted, 1);
        assert!(frames.dropped >= 1);
        assert_eq!(handle.dropped_count(), frames.dropped);
    }

    #[test]
    fn a_dropped_frame_takes_no_number() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        let frame = || unnumbered(Modality::ScreenFrame, 0, 0);
        assert_eq!(handle.emit(frame()).unwrap(), EmitOutcome::Queued);
        let dropping = emit_on_a_thread(&handle, frame());
        let outcome = dropping
            .outcome
            .recv_timeout(Duration::from_secs(1))
            .expect("the frame emit on a full queue returned inside 1 s");
        assert!(matches!(outcome, Ok(EmitOutcome::Dropped)));
        dropping.thread.join().unwrap();
        let first = writer.try_recv().unwrap();
        assert_eq!(first.number(), ArrivalNumber::for_test(0));
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
        assert!(matches!(
            writer.recv_timeout(Duration::from_millis(20)),
            Err(RecvTimeoutError::Timeout)
        ));
        assert_eq!(writer.barrier(), Barrier(1), "an empty poll took a number");
        assert_eq!(handle.emit(frame()).unwrap(), EmitOutcome::Queued);
        assert!(writer.recv().is_some());
        assert_eq!(writer.barrier(), Barrier(1), "recv took a number");
        assert_eq!(handle.emit(frame()).unwrap(), EmitOutcome::Queued);
        let second = writer.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(second.number(), ArrivalNumber::for_test(1));
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
        drop(handle);
        assert!(matches!(
            writer.recv_timeout(Duration::from_millis(20)),
            Err(RecvTimeoutError::Disconnected)
        ));
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Disconnected)));
        assert_eq!(
            writer.barrier(),
            Barrier(2),
            "a poll of a gone sink took a number"
        );
    }

    fn event(sink: &SinkHandle, modality: Modality) -> CaptureStreamEvent {
        CaptureStreamEvent {
            event_id: Uuid::now_v7(),
            captured_at: CapturedAt {
                wall_clock: UNIX_EPOCH,
                sequence: sink.next_sequence(),
            },
            modality,
            foreground_app: "test".to_string(),
            segment_ref: SegmentRef {
                segment_id: Uuid::now_v7(),
                byte_offset: 0,
                byte_len: 0,
            },
            segment_key_id: SegmentKeyId(1),
            is_derived: false,
            retention_class: RetentionClass::Raw,
        }
    }

    #[test]
    fn a_disconnected_sink_reports_an_error() {
        let (h, w) = channel(1);
        drop(w);
        assert!(matches!(
            h.emit(event(&h, Modality::ScreenFrame)),
            Err(SinkError::Disconnected)
        ));
    }

    #[test]
    fn the_sequence_is_monotonic() {
        let (h, _w) = channel(4);
        let a = h.next_sequence().get();
        let b = h.next_sequence().get();
        let c = h.next_sequence().get();
        assert!(a < b && b < c);
    }

    use crate::event::{AppIdentity, EventClock};
    use crate::floor::cases::keep;
    use crate::floor::payload::{FoldClass, FoldCount, Observation, PointerFold, Title, TitleWhy};
    use crate::floor::site::Site;
    use crate::floor::text::TEXT_MAX_BYTES;

    fn terminal() -> AppIdentity {
        AppIdentity::bundle_id("com.apple.Terminal".to_string())
    }

    fn fold_event(class: FoldClass, count: FoldCount) -> FoldEvent {
        let fold = PointerFold::new(class, count);
        FoldEvent::new(keep(terminal(), 7), fold, None, EventClock::system())
    }

    fn title_event(text: &str) -> FloorEvent {
        let title = Title::new(text.to_string(), TitleWhy::TitleChange, None);
        FloorEvent::window_title(keep(terminal(), 7), title, None, EventClock::system())
    }

    fn count_of(event: &FloorEvent) -> u64 {
        match event.observation() {
            Observation::Fold(fold) => fold.count().get(),
            other => panic!("not a fold: {other:?}"),
        }
    }

    #[test]
    fn the_old_receiver_sees_a_floor_value_without_its_payload() {
        let (handle, writer) = bounded(NonZeroUsize::new(4).unwrap());
        assert_eq!(
            handle.emit_event(title_event("Payroll 2026")),
            Emitted::Queued
        );
        let shown = writer.recv().expect("the old receive got the value");
        assert_eq!(shown.modality, Modality::WindowTitle);
        assert!(shown.foreground_app.is_empty());
        assert_eq!(shown.segment_ref.segment_id, Uuid::nil());
        assert!(!format!("{shown:?}").contains("Payroll"));
        let sent = title_event("Payroll 2026");
        let (id, at) = (sent.id(), sent.at());
        assert_eq!(handle.emit_event(sent), Emitted::Queued);
        let received = writer.try_recv().unwrap();
        assert_eq!(received.number(), ArrivalNumber::for_test(0));
        assert_eq!(received.event().modality, Modality::WindowTitle);
        assert_eq!(received.event().event_id, id.uuid());
        assert!(received.event().foreground_app.is_empty());
        let floor = received.into_floor().expect("a floor value");
        assert_eq!(floor.event().id(), id);
        assert_eq!(floor.event().at(), at);
        assert_eq!(floor.number(), ArrivalNumber::for_test(0));
        assert!(floor.is_before(writer.barrier()));
        let counted = KindCounts {
            accepted: 2,
            ..KindCounts::default()
        };
        assert_eq!(handle.counts().of(Modality::WindowTitle), counted);
    }

    #[test]
    fn an_old_path_event_keeps_its_fields_and_has_no_floor_value() {
        let (handle, writer) = bounded(NonZeroUsize::new(4).unwrap());
        let mut old = unnumbered(Modality::Keystroke, 3, 9);
        old.foreground_app = "com.apple.Terminal".to_string();
        assert_eq!(handle.emit(old.clone()).unwrap(), EmitOutcome::Queued);
        let received = writer.try_recv().unwrap();
        assert_eq!(received.event(), &old);
        let back = received
            .into_floor()
            .expect_err("an old event has no floor value");
        assert_eq!(back.event(), &old);
        assert_eq!(back.number(), ArrivalNumber::for_test(0));
    }

    #[test]
    fn a_floor_value_is_numbered_in_arrival_order_with_old_events() {
        let (handle, writer) = bounded(NonZeroUsize::new(8).unwrap());
        handle.emit(unnumbered(Modality::Keystroke, 0, 0)).unwrap();
        assert_eq!(handle.emit_event(title_event("a")), Emitted::Queued);
        let fold = fold_event(FoldClass::Move, FoldCount::one());
        assert!(matches!(handle.emit_continuous(fold), FoldEmitted::Queued));
        let kinds: Vec<(u64, Modality, bool)> = (0..3)
            .map(|_| {
                let received = writer.try_recv().unwrap();
                let number = received.number().get();
                let kind = received.event().modality;
                (number, kind, received.into_floor().is_ok())
            })
            .collect();
        assert_eq!(
            kinds,
            [
                (0, Modality::Keystroke, false),
                (1, Modality::WindowTitle, true),
                (2, Modality::Pointer, true),
            ]
        );
        assert_eq!(handle.counts().of(Modality::Pointer).accepted, 1);
    }

    #[test]
    fn a_floor_frame_on_a_full_queue_is_dropped_and_counted() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        let frame = || {
            let keep = keep(terminal(), 7);
            FloorEvent::screen_frame(keep, None, EventClock::system())
        };
        assert_eq!(handle.emit_event(frame()), Emitted::Queued);
        let (done_tx, done) = mpsc::channel();
        let emitter = {
            let handle = handle.clone();
            let second = frame();
            thread::spawn(move || {
                let _ = done_tx.send(handle.emit_event(second));
            })
        };
        let outcome = done
            .recv_timeout(Duration::from_secs(1))
            .expect("a frame on a full queue returned inside 1 s");
        emitter.join().unwrap();
        assert_eq!(outcome, Emitted::Dropped);
        let frames = KindCounts {
            accepted: 1,
            dropped: 1,
            ..KindCounts::default()
        };
        assert_eq!(handle.counts().of(Modality::ScreenFrame), frames);
        assert_eq!(
            writer.try_recv().unwrap().number(),
            ArrivalNumber::for_test(0)
        );
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn a_floor_value_is_refused_when_the_writer_is_gone() {
        let (handle, writer) = bounded(NonZeroUsize::new(2).unwrap());
        drop(writer);
        assert_eq!(handle.emit_event(title_event("x")), Emitted::Refused);
        let fold = fold_event(FoldClass::Scroll, FoldCount::one());
        assert!(matches!(handle.emit_continuous(fold), FoldEmitted::Refused));
        let refused = KindCounts {
            refused: 1,
            ..KindCounts::default()
        };
        assert_eq!(handle.counts().of(Modality::WindowTitle), refused);
        assert_eq!(handle.counts().of(Modality::Pointer), refused);
    }

    #[test]
    fn a_fold_emit_on_a_full_queue_returns_at_once_with_the_record_and_is_counted() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        assert_eq!(
            handle
                .emit(unnumbered(Modality::ScreenFrame, 0, 0))
                .unwrap(),
            EmitOutcome::Queued
        );
        let mut three = FoldCount::one();
        three.add_one();
        three.add_one();
        let (done_tx, done) = mpsc::channel();
        let emitter = {
            let handle = handle.clone();
            thread::spawn(move || {
                let _ = done_tx.send(handle.emit_continuous(fold_event(FoldClass::Drag, three)));
            })
        };
        let outcome = done
            .recv_timeout(Duration::from_secs(1))
            .expect("the fold emit on a full queue returned inside 1 s");
        emitter.join().unwrap();
        let FoldEmitted::Coalesced(back) = outcome else {
            panic!("not coalesced: {outcome:?}");
        };
        assert_eq!(back.fold(), PointerFold::new(FoldClass::Drag, three));
        let pointer = KindCounts {
            coalesced: 1,
            ..KindCounts::default()
        };
        assert_eq!(handle.counts().of(Modality::Pointer), pointer);
        assert_eq!(handle.dropped_count(), 0);
        let frame = writer.try_recv().unwrap();
        assert_eq!(frame.event().modality, Modality::ScreenFrame);
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn a_fold_count_handed_back_can_be_added_to_the_next() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        let mut three = FoldCount::one();
        three.add_one();
        three.add_one();
        assert!(matches!(
            handle.emit_continuous(fold_event(FoldClass::Scroll, FoldCount::one())),
            FoldEmitted::Queued
        ));
        let FoldEmitted::Coalesced(back) =
            handle.emit_continuous(fold_event(FoldClass::Scroll, three))
        else {
            panic!("the second fold on a one-slot queue was not handed back");
        };
        let first = writer.try_recv().unwrap().into_floor().unwrap();
        assert_eq!(count_of(first.event()), 1);
        let mut next = FoldCount::one();
        next.add(back.fold().count());
        assert!(matches!(
            handle.emit_continuous(fold_event(back.fold().class(), next)),
            FoldEmitted::Queued
        ));
        let second = writer.try_recv().unwrap().into_floor().unwrap();
        assert_eq!(count_of(second.event()), 4);
        assert_eq!(second.number(), ArrivalNumber::for_test(1));
        let pointer = handle.counts().of(Modality::Pointer);
        assert_eq!((pointer.accepted, pointer.coalesced), (2, 1));
    }

    #[test]
    fn a_keystroke_after_a_flood_of_fold_emits_is_queued_after_one_receive() {
        const SLOTS: usize = 2_048;
        let (handle, writer) = bounded(NonZeroUsize::new(SLOTS).unwrap());
        let (flooded_tx, flooded) = mpsc::channel();
        let flood = {
            let handle = handle.clone();
            thread::spawn(move || {
                let mut coalesced = 0u64;
                for index in 0..SLOTS * 2 {
                    let class = FoldClass::ALL[index % 3];
                    if let FoldEmitted::Coalesced(_) =
                        handle.emit_continuous(fold_event(class, FoldCount::one()))
                    {
                        coalesced += 1;
                    }
                }
                let _ = flooded_tx.send(coalesced);
            })
        };
        let coalesced = flooded
            .recv_timeout(Duration::from_secs(10))
            .expect("4,096 fold emits on a queue of 2,048 returned inside 10 s");
        flood.join().unwrap();
        assert_eq!(coalesced, 2_048);
        let waiting = emit_on_a_thread(&handle, unnumbered(Modality::Keystroke, 0, 0));
        waiting
            .started
            .recv_timeout(Duration::from_secs(1))
            .expect("the emitter started inside 1 s");
        let first = writer.try_recv().unwrap();
        assert_eq!(first.event().modality, Modality::Pointer);
        let outcome = waiting
            .outcome
            .recv_timeout(Duration::from_secs(1))
            .expect("the keystroke returned inside 1 s of one receive");
        assert!(matches!(outcome, Ok(EmitOutcome::Queued)));
        waiting.thread.join().unwrap();
        let mut last = None;
        while let Ok(received) = writer.try_recv() {
            last = Some((received.number().get(), received.event().modality));
        }
        assert_eq!(last, Some((2_048, Modality::Keystroke)));
        let pointer = handle.counts().of(Modality::Pointer);
        assert_eq!((pointer.accepted, pointer.coalesced), (2_048, 2_048));
    }

    #[test]
    fn a_full_queue_of_the_largest_events_stays_under_the_byte_bound() {
        const QUEUED: usize = 2_048;
        const BATCHED: usize = 512;
        const BOUND: usize = 6_291_456;
        const SITE_MAX_BYTES: usize = crate::floor::site::SITE_MAX_BYTES;
        let label = "a".repeat(63);
        let address = format!("https://www.{label}.co.uk/{}", "p".repeat(900));
        let largest = || {
            let path = format!("C:\\Apps\\{}", "b".repeat(2 * TEXT_MAX_BYTES));
            let app = AppIdentity::executable_path(path);
            let site = Site::cut(address.clone()).expect("a site");
            let words = "t".repeat(2 * TEXT_MAX_BYTES);
            let title = Title::new(words, TitleWhy::AppChange, Some(site));
            FloorEvent::window_title(keep(app, 7), title, None, EventClock::system())
        };
        let (handle, writer) = bounded(NonZeroUsize::new(QUEUED).unwrap());
        for _ in 0..QUEUED {
            assert_eq!(handle.emit_event(largest()), Emitted::Queued);
        }
        let slot = std::mem::size_of::<Item>() + std::mem::size_of::<usize>();
        let mut heap = 0usize;
        let mut site_heap = 0usize;
        for _ in 0..QUEUED {
            let received = writer.try_recv().unwrap().into_floor().unwrap();
            heap += received.event().heap_bytes();
            site_heap += received.event().site_heap_bytes();
        }
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
        let batch: Vec<FloorEvent> = (0..BATCHED).map(|_| largest()).collect();
        for event in &batch {
            heap += event.heap_bytes();
            site_heap += event.site_heap_bytes();
        }
        let events = QUEUED + BATCHED;
        let sum = events * slot + heap;
        let site_at_its_cap = sum - site_heap + events * SITE_MAX_BYTES;
        println!("sink_queue slot={slot} events={events} sum={sum} site_at_its_cap={site_at_its_cap} bound={BOUND}");
        assert!(heap >= events * 2 * TEXT_MAX_BYTES);
        assert!(sum < BOUND, "{sum}");
        assert!(site_at_its_cap < BOUND, "{site_at_its_cap}");
    }

    /// Words of a timed wait or of giving up, read without case.
    const WAIT_WORDS: [&str; 7] = [
        "timeout", "sleep", "instant", "deadline", "park", "yield", "spin",
    ];

    /// The receive-side lines of the product text that may hold a wait word.
    const RECEIVE_SIDE: [&str; 4] = [
        "sync_channel, Receiver, RecvTimeoutError, SendError, SyncSender, TryRecvError, TrySendError,",
        "/// [`SinkWriter::recv_timeout`] or [`SinkWriter::try_recv`]. Numbers rise by",
        "pub fn recv_timeout(&self, timeout: Duration) -> Result<Received, RecvTimeoutError> {",
        "self.rx.recv_timeout(timeout).map(|item| self.number(item))",
    ];

    /// The blocking arm of `send`, whole, with its white space collapsed.
    const BLOCK_ARM: &str = "Backpressure::Block => match self.tx.send(item) { Ok(()) => self.accepted(kind), Err(SendError(_)) => self.refused(kind), },";

    const COUNTER_WRITES: [&str; 5] = [
        ".store(",
        ".swap(",
        ".fetch_sub(",
        ".fetch_update(",
        ".compare_exchange",
    ];

    fn collapsed(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The name of the function whose body holds the byte at `at`.
    fn enclosing_fn(text: &str, at: usize) -> &str {
        let before = &text[..at];
        let start = before.rfind("fn ").map_or(0, |i| i + 3);
        let name = &before[start..];
        &name[..name.find(['(', '<']).unwrap_or(name.len())]
    }

    fn rules_broken(product: &str) -> Vec<String> {
        let mut broken = Vec::new();
        for (number, line) in product.lines().enumerate() {
            let lower = line.to_lowercase();
            for word in WAIT_WORDS {
                if lower.contains(word) && !RECEIVE_SIDE.contains(&line.trim()) {
                    broken.push(format!("line {}: a word of waiting ({word})", number + 1));
                }
            }
        }
        let arms = product.matches("Backpressure::Block =>").count();
        let arm = product
            .find("Backpressure::Block =>")
            .map(|at| &product[at..])
            .and_then(|rest| rest.find("},").map(|end| collapsed(&rest[..end + 2])));
        if arms != 1 || arm.as_deref() != Some(BLOCK_ARM) {
            broken.push("the blocking arm is not `self.tx.send(item)` whole".to_string());
        }
        if product.matches("self.tx.send(").count() != 1 {
            broken.push("a send other than the blocking arm's".to_string());
        }
        let places: Vec<&str> = product
            .match_indices("try_send")
            .map(|(at, _)| enclosing_fn(product, at))
            .collect();
        if places != ["emit_continuous", "send"] {
            broken.push(format!(
                "try_send stands in {places:?}, not in emit_continuous and the drop arm of send"
            ));
        }
        if !product.contains("Backpressure::Drop => match self.tx.try_send(item) {") {
            broken.push("the drop arm does not try_send".to_string());
        }
        for write in COUNTER_WRITES {
            if product.contains(write) {
                broken.push(format!("a counter write other than fetch_add ({write})"));
            }
        }
        for counter in [".accepted", ".dropped", ".coalesced", ".refused"] {
            for (at, _) in product.match_indices(counter) {
                let after = product[at + counter.len()..].trim_start();
                let Some(method) = after.strip_prefix('.') else {
                    continue;
                };
                if !method.starts_with("fetch_add(") && !method.starts_with("load(") {
                    broken.push(format!("{counter} changed by other than fetch_add"));
                }
            }
        }
        broken
    }

    /// `product` with `added` put in after the first `anchor`.
    fn inserted(product: &str, anchor: &str, added: &str) -> String {
        let at = product
            .find(anchor)
            .unwrap_or_else(|| panic!("the fixture's anchor is gone: {anchor}"))
            + anchor.len();
        format!("{}{added}{}", &product[..at], &product[at..])
    }

    fn breaks(fixture: &str, rule: &str) -> bool {
        rules_broken(fixture)
            .iter()
            .any(|broken| broken.contains(rule))
    }

    #[test]
    fn the_emit_path_has_no_timed_wait_and_every_counter_changes_by_fetch_add_alone() {
        let source = include_str!("sink.rs").replace("\r\n", "\n");
        let product = &source[..source.find("#[cfg(test)]\nmod tests {").unwrap()];
        assert!(product.contains("pub fn emit_event("));
        assert_eq!(rules_broken(product), Vec::<String>::new());

        let block = "Backpressure::Block => ";
        let m8 = inserted(
            product,
            block,
            "{ let deadline = std::time::Instant::now() + Duration::from_secs(2); let mut item = item; loop { match self.tx.try_send(item) { Ok(()) => break self.accepted(kind), Err(TrySendError::Full(back)) if std::time::Instant::now() < deadline => { item = back; std::thread::sleep(Duration::from_millis(1)); } Err(_) => break self.dropped(kind), } } } ",
        );
        for rule in [
            "(sleep)",
            "(instant)",
            "(deadline)",
            "blocking arm",
            "try_send stands",
        ] {
            assert!(
                breaks(&m8, rule),
                "the 2 s form in the blocking arm: {rule}"
            );
        }
        let yields = inserted(
            product,
            block,
            "{ let mut item = item; let mut tries = 0u32; loop { match self.tx.try_send(item) { Ok(()) => break self.accepted(kind), Err(TrySendError::Full(back)) if tries < 2_000_000 => { item = back; tries += 1; } Err(_) => break self.dropped(kind), } } } ",
        );
        assert!(
            breaks(&yields, "blocking arm"),
            "a count of tries with no clock word"
        );
        assert!(
            breaks(&yields, "try_send stands"),
            "a count of tries with no clock word"
        );
        let yielded = yields.replacen(
            "tries += 1;",
            "tries += 1; std::thread::yield_now(); std::hint::spin_loop();",
            1,
        );
        assert!(breaks(&yielded, "(yield)") && breaks(&yielded, "(spin)"));
        let helper = "\nfn wait_for_room(tx: &SyncSender<Item>, mut item: Item) -> bool {\n    let deadline = std::time::Instant::now() + Duration::from_secs(2);\n    loop {\n        match tx.try_send(item) {\n            Ok(()) => return true,\n            Err(TrySendError::Full(back)) if std::time::Instant::now() < deadline => {\n                item = back;\n                std::thread::sleep(Duration::from_millis(1));\n            }\n            Err(_) => return false,\n        }\n    }\n}\n";
        let outside = inserted(product, "\nfn handed_back(", helper);
        let outside = inserted(
            &outside,
            block,
            "if wait_for_room(&self.tx, item) { self.accepted(kind) } else { self.dropped(kind) } } fn _unused(&self, kind: Modality, item: Item) -> Result<EmitOutcome, SinkError> { match backpressure(kind) { Backpressure::Drop => self.dropped(kind), ",
        );
        for rule in ["(sleep)", "(instant)", "(deadline)", "try_send stands"] {
            assert!(
                breaks(&outside, rule),
                "the 2 s wait outside the impl blocks: {rule}"
            );
        }
        let keystrokes = inserted(
            product,
            "    fn send(&self, kind: Modality, item: Item) -> Result<EmitOutcome, SinkError> {\n",
            "        if kind == Modality::Keystroke {\n            return if wait_for_room(&self.tx, item) { self.accepted(kind) } else { self.dropped(kind) };\n        }\n",
        );
        let keystrokes = inserted(&keystrokes, "\nfn handed_back(", helper);
        assert!(keystrokes.contains("Backpressure::Block => match self.tx.send(item) {"));
        assert!(!breaks(&keystrokes, "blocking arm"));
        for rule in ["(sleep)", "(instant)", "(deadline)", "try_send stands"] {
            assert!(
                breaks(&keystrokes, rule),
                "keystrokes alone, the arm left in place: {rule}"
            );
        }

        let lost = product.replacen(
            "self.counters(kind).accepted.fetch_add(1, Ordering::Relaxed);\n        Ok(",
            "let c = &self.counters(kind).accepted;\n        c.store(c.load(Ordering::Relaxed) + 1, Ordering::Relaxed);\n        Ok(",
            1,
        );
        assert_ne!(lost, product);
        assert!(breaks(&lost, ".store("));
        let swapped = product.replacen(
            "kind.dropped.load(Ordering::Relaxed)",
            "kind.dropped.swap(0, Ordering::Relaxed)",
            1,
        );
        assert_ne!(swapped, product);
        assert!(!rules_broken(&swapped).is_empty());
    }
}
