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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SendError, SyncSender, TrySendError};
use std::sync::Arc;

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

/// The single drain side. Not `Clone`: there is one writer. The daemon owns it
/// and appends every received event to the store.
pub struct SinkWriter {
    rx: Receiver<CaptureStreamEvent>,
}

impl SinkWriter {
    /// Block for the next event, or return `None` once every [`SinkHandle`] has
    /// been dropped.
    pub fn recv(&self) -> Option<CaptureStreamEvent> {
        self.rx.recv().ok()
    }

    /// The raw receiver, for a writer that wants to select or batch.
    pub fn receiver(&self) -> &Receiver<CaptureStreamEvent> {
        &self.rx
    }
}

/// Build the sink. `capacity` bounds the queue; past it, droppable modalities are
/// dropped and others block.
pub fn channel(capacity: usize) -> (SinkHandle, SinkWriter) {
    let (tx, rx) = sync_channel(capacity);
    let handle = SinkHandle {
        tx,
        sequence: Arc::new(AtomicU64::new(0)),
        dropped: Arc::new(AtomicU64::new(0)),
    };
    (handle, SinkWriter { rx })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{
        CaptureStreamEvent, CapturedAt, Modality, RetentionClass, SegmentKeyId, SegmentRef,
    };
    use std::thread;
    use std::time::UNIX_EPOCH;
    use uuid::Uuid;

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
