//! The single-writer sink: the only path from a capture source to the store.
//!
//! [`channel`] returns a cloneable [`SinkHandle`] that sources emit through and a
//! non-cloneable [`SinkWriter`] that the daemon owns and drains. Because the
//! writer cannot be cloned, there is exactly one appender to the store, which is
//! the concurrency invariant from the kickoff plan (capture is the sole writer).
//!
//! Backpressure: the channel is bounded. A droppable modality (frames, audio,
//! camera) is dropped when the queue is full; every other modality blocks until
//! the writer makes room, so a keystroke is never lost.

use std::cell::Cell;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{
    sync_channel, Receiver, RecvTimeoutError, SendError, SyncSender, TryRecvError, TrySendError,
};
use std::sync::Arc;
use std::time::Duration;

use crate::event::{CaptureStreamEvent, MonotonicSequence};
use crate::source::{EmitOutcome, EventSink};

#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    #[error("the capture writer has stopped; no more events can be queued")]
    Disconnected,
}

/// The cloneable emit side held by every capture source. Clones share one queue
/// and one sequence authority, so sequences stay globally monotonic across
/// sources.
#[derive(Clone)]
pub struct SinkHandle {
    tx: SyncSender<CaptureStreamEvent>,
    sequence: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

impl SinkHandle {
    /// The next monotonic sequence number for the stream. The sink is the single
    /// authority, so numbers are unique and increasing no matter which source
    /// calls this.
    pub fn next_sequence(&self) -> MonotonicSequence {
        MonotonicSequence::new(self.sequence.fetch_add(1, Ordering::Relaxed))
    }

    /// How many droppable events have been dropped under backpressure so far.
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl EventSink for SinkHandle {
    fn emit(&self, event: CaptureStreamEvent) -> Result<EmitOutcome, SinkError> {
        if event.modality.is_droppable_under_backpressure() {
            match self.tx.try_send(event) {
                Ok(()) => Ok(EmitOutcome::Queued),
                Err(TrySendError::Full(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                    Ok(EmitOutcome::Dropped)
                }
                Err(TrySendError::Disconnected(_)) => Err(SinkError::Disconnected),
            }
        } else {
            match self.tx.send(event) {
                Ok(()) => Ok(EmitOutcome::Queued),
                Err(SendError(_)) => Err(SinkError::Disconnected),
            }
        }
    }
}

/// The number that the writer gives an event when it receives it through
/// [`SinkWriter::recv_timeout`] or [`SinkWriter::try_recv`]. Numbers rise by
/// exactly one in the order in which events arrive there; a dropped or refused
/// event never gets one. The count restarts at 0 in every process and with every
/// sink, so a number is no read position: never store one as a cursor or compare
/// numbers of two processes. Only the writer makes one.
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

/// An event as the writer received it, with its [`ArrivalNumber`]. The event is
/// shown read-only. Its own `captured_at.sequence` is the stamp that the emitting
/// source took before a send that can block, not the arrival number: order by
/// [`Received::number`].
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
}

/// The single drain side. Not `Clone` and not `Sync`: there is one receiver, and
/// it numbers what it receives. The daemon owns it.
pub struct SinkWriter {
    rx: Receiver<CaptureStreamEvent>,
    next: Cell<u64>,
}

impl SinkWriter {
    /// Block for the next event, or return `None` once every [`SinkHandle`] has
    /// been dropped. The event gets no [`ArrivalNumber`] and takes none.
    pub fn recv(&self) -> Option<CaptureStreamEvent> {
        self.rx.recv().ok()
    }

    /// Wait at most `timeout` for the next event and give it the next
    /// [`ArrivalNumber`]. `Disconnected` once every [`SinkHandle`] has been dropped
    /// and the queue is empty.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Received, RecvTimeoutError> {
        self.rx
            .recv_timeout(timeout)
            .map(|event| self.number(event))
    }

    /// Take the next event if one is queued and give it the next
    /// [`ArrivalNumber`].
    pub fn try_recv(&self) -> Result<Received, TryRecvError> {
        self.rx.try_recv().map(|event| self.number(event))
    }

    /// The raw receiver, for a writer that wants to select or batch.
    pub fn receiver(&self) -> &Receiver<CaptureStreamEvent> {
        &self.rx
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

/// Build the sink. `capacity` bounds the queue; past it, droppable modalities are
/// dropped and others block. With a capacity of 0 a droppable event is dropped
/// unless the writer is waiting in a receive; [`bounded`] cannot be given 0.
pub fn channel(capacity: usize) -> (SinkHandle, SinkWriter) {
    build(capacity)
}

/// Build the sink with a queue of `capacity` events. Past it, a droppable kind is
/// dropped and every other kind blocks the emitting thread until the writer makes
/// room.
pub fn bounded(capacity: NonZeroUsize) -> (SinkHandle, SinkWriter) {
    build(capacity.get())
}

fn build(capacity: usize) -> (SinkHandle, SinkWriter) {
    let (tx, rx) = sync_channel(capacity);
    let handle = SinkHandle {
        tx,
        sequence: Arc::new(AtomicU64::new(0)),
        dropped: Arc::new(AtomicU64::new(0)),
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
    use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
    use std::thread;
    use std::time::{Duration, UNIX_EPOCH};
    use uuid::Uuid;

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
        let emitters: Vec<_> = (0..EMITTERS)
            .map(|emitter| {
                let handle = handle.clone();
                thread::spawn(move || {
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
        drop(handle);
        let mut next_index = [0u64; EMITTERS];
        let mut expected = 0u64;
        loop {
            match writer.recv_timeout(Duration::from_secs(10)) {
                Ok(received) => {
                    assert_eq!(received.number(), ArrivalNumber::for_test(expected));
                    let event = received.event();
                    let emitter = usize::try_from(event.segment_key_id.0).unwrap();
                    assert_eq!(event.segment_ref.byte_offset, next_index[emitter]);
                    next_index[emitter] += 1;
                    expected += 1;
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => panic!("no event and no disconnect in 10 s"),
            }
        }
        for emitter in emitters {
            emitter.join().unwrap();
        }
        assert_eq!(next_index, [PER_EMITTER; EMITTERS]);
        assert_eq!(expected, 400_000);
    }

    #[test]
    fn a_dropped_frame_takes_no_number() {
        let (handle, writer) = bounded(NonZeroUsize::MIN);
        let frame = || unnumbered(Modality::ScreenFrame, 0, 0);
        assert_eq!(handle.emit(frame()).unwrap(), EmitOutcome::Queued);
        assert_eq!(handle.emit(frame()).unwrap(), EmitOutcome::Dropped);
        let first = writer.try_recv().unwrap();
        assert_eq!(first.number(), ArrivalNumber::for_test(0));
        assert_eq!(handle.emit(frame()).unwrap(), EmitOutcome::Queued);
        let second = writer.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(second.number(), ArrivalNumber::for_test(1));
        assert!(matches!(writer.try_recv(), Err(TryRecvError::Empty)));
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
    fn a_full_queue_drops_a_frame() {
        let (h, _w) = channel(1);
        assert_eq!(
            h.emit(event(&h, Modality::ScreenFrame)).unwrap(),
            EmitOutcome::Queued
        );
        assert_eq!(
            h.emit(event(&h, Modality::ScreenFrame)).unwrap(),
            EmitOutcome::Dropped
        );
        assert_eq!(h.dropped_count(), 1);
    }

    #[test]
    fn a_keystroke_waits_instead_of_dropping() {
        let (h, w) = channel(1);
        h.emit(event(&h, Modality::ScreenFrame)).unwrap();
        let h2 = h.clone();
        let emitter = thread::spawn(move || {
            let ev = event(&h2, Modality::Keystroke);
            h2.emit(ev).unwrap()
        });
        let first = w.recv().unwrap();
        let outcome = emitter.join().unwrap();
        let second = w.recv().unwrap();

        assert_eq!(outcome, EmitOutcome::Queued);
        assert_eq!(h.dropped_count(), 0);
        assert_eq!(first.modality, Modality::ScreenFrame);
        assert_eq!(second.modality, Modality::Keystroke);
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
