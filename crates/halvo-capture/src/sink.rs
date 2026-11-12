//! Backpressure: the channel is bounded. A droppable modality (frames, audio,
//! camera) is dropped when the queue is full; every other modality blocks until
//! the writer makes room.

use std::cell::Cell;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver, RecvTimeoutError, SendError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::Arc;
use std::time::Duration;

use crate::event::{CaptureStreamEvent, Modality, MonotonicSequence};
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
    /// Emits whose fold record a full queue handed back to the source; it reads 0
    /// until an emit coalesces.
    pub coalesced: u64,
    /// Emits that returned `Disconnected`: the writer was gone when the emit
    /// came, or went while the emit was blocked.
    pub refused: u64,
}

/// The counts of every kind, read by [`SinkHandle::counts`]. Every clone of a
/// handle adds to the same counts, which start at 0 when the sink is built and
/// only rise. An emit is counted when it returns, so an event can be received
/// before its emit is counted. The counters are read one after another, not at
/// one instant: a sum of counters is a value that the total held while it was
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

#[derive(Clone)]
pub struct SinkHandle {
    tx: SyncSender<CaptureStreamEvent>,
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

impl EventSink for SinkHandle {
    fn emit(&self, event: CaptureStreamEvent) -> Result<EmitOutcome, SinkError> {
        let kind = event.modality;
        match backpressure(kind) {
            Backpressure::Drop => match self.tx.try_send(event) {
                Ok(()) => self.accepted(kind),
                Err(TrySendError::Full(_)) => self.dropped(kind),
                Err(TrySendError::Disconnected(_)) => self.refused(kind),
            },
            Backpressure::Block => match self.tx.send(event) {
                Ok(()) => self.accepted(kind),
                Err(SendError(_)) => self.refused(kind),
            },
        }
    }
}

/// The number that the writer gives an event when it receives it through
/// [`SinkWriter::recv_timeout`] or [`SinkWriter::try_recv`]. Numbers rise by
/// exactly one in the order in which events arrive there; a dropped or refused
/// event never gets one. The count restarts at 0 in every process and with every
/// sink, so a number is no read position: never store one as a cursor or compare
/// numbers of two processes.
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
#[derive(Debug)]
pub struct Received {
    event: CaptureStreamEvent,
    number: ArrivalNumber,
}

impl Received {
    pub fn number(&self) -> ArrivalNumber {
        self.number
    }

    pub fn event(&self) -> &CaptureStreamEvent {
        &self.event
    }

    pub fn is_before(&self, barrier: Barrier) -> bool {
        self.number.0 < barrier.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Barrier(u64);

pub struct SinkWriter {
    rx: Receiver<CaptureStreamEvent>,
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
    /// The event gets no [`ArrivalNumber`] and takes none.
    pub fn recv(&self) -> Option<CaptureStreamEvent> {
        self.rx.recv().ok()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Received, RecvTimeoutError> {
        self.rx
            .recv_timeout(timeout)
            .map(|event| self.number(event))
    }

    pub fn try_recv(&self) -> Result<Received, TryRecvError> {
        self.rx.try_recv().map(|event| self.number(event))
    }

    /// Draw a barrier at the next number that this writer will give. Every event
    /// received before the draw is before it. An event that was still queued, or
    /// still blocked in an emit, at the draw is not before it, so an owner that
    /// discards the events at or after a barrier discards those too, which errs
    /// toward not persisting.
    pub fn barrier(&self) -> Barrier {
        Barrier(self.next.get())
    }

    fn number(&self, event: CaptureStreamEvent) -> Received {
        let number = self.next.get();
        self.next.set(number + 1);
        Received {
            event,
            number: ArrivalNumber(number),
        }
    }
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
        let (handle, writer) = bounded(NonZeroUsize::new(2048).unwrap());
        let (alive, all_ended) = mpsc::channel::<()>();
        let emitters: Vec<_> = (0..EMITTERS)
            .map(|emitter| {
                let handle = handle.clone();
                let alive = alive.clone();
                thread::spawn(move || {
                    let _alive = alive;
                    let kind = if emitter % 2 == 0 {
                        Modality::Keystroke
                    } else {
                        Modality::Pointer
                    };
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
        let half = KindCounts {
            accepted: 200_000,
            ..KindCounts::default()
        };
        let counts = handle.counts();
        assert_eq!(counts.of(Modality::Keystroke), half);
        assert_eq!(counts.of(Modality::Pointer), half);
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
    fn a_queue_of_capacity_zero_drops_a_frame_unless_a_receive_waits() {
        let (handle, writer) = channel(0);
        let unwaited = handle
            .emit(unnumbered(Modality::ScreenFrame, 0, 0))
            .unwrap();
        assert_eq!(unwaited, EmitOutcome::Dropped);
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
}
