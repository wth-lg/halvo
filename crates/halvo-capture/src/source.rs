//! The platform-agnostic capture traits. Each modality's OS-specific source
//! implements [`CaptureSource`] and emits events through an [`EventSink`]. Per-OS
//! implementations live under [`crate::macos`] / [`crate::windows`] behind `cfg`;
//! the rest of the app sees only these traits.

use crate::event::{CaptureStreamEvent, Modality};
use crate::sink::{SinkError, SinkHandle};

/// One capture source for one modality. `start` installs the OS callback and
/// returns; the callback does the minimum (stamp, package, emit) and returns, so
/// a slow writer never blocks a capture callback.
pub trait CaptureSource {
    fn modality(&self) -> Modality;
    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError>;
    fn stop(&mut self);
}

/// The non-blocking emit path a source uses to hand an event to the single
/// writer. The only implementor is the sink handle (see [`crate::sink`]).
pub trait EventSink {
    /// Enqueue an event for the single writer. Never drops a non-droppable
    /// modality; see [`crate::sink`] for the backpressure policy.
    fn emit(&self, event: CaptureStreamEvent) -> Result<EmitOutcome, SinkError>;
}

/// What happened to an emitted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitOutcome {
    /// The event was queued for the writer.
    Queued,
    /// A droppable event was dropped because the queue was full.
    Dropped,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("the capture permission for {0:?} is not granted")]
    PermissionDenied(Modality),
    #[error("the capture source failed to start: {0}")]
    Start(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}
