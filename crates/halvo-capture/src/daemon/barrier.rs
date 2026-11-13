//! The owner's one discarding state. A pause starts it when the owner handles
//! the command, at a barrier drawn from the sink's writer; a resume ends it at
//! a second barrier. The owner starts in it.

use crate::sink::{Barrier, Received, SinkWriter};

/// Whether the owner writes the events that it receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Every received event is discarded.
    Discarding,
    /// Events numbered at or after `from` are kept.
    Writing { from: Barrier },
}

impl Gate {
    /// The state that the owner starts in.
    pub const fn closed() -> Gate {
        Gate::Discarding
    }

    /// Starts discarding and returns the barrier `b`: every event received
    /// before it was received before the pause.
    pub fn open_pause(&mut self, writer: &SinkWriter) -> Barrier {
        *self = Gate::Discarding;
        writer.barrier()
    }

    /// Stops discarding and returns the barrier `r`: the first event numbered
    /// at or after it is kept.
    pub fn close_pause(&mut self, writer: &SinkWriter) -> Barrier {
        let from = writer.barrier();
        *self = Gate::Writing { from };
        from
    }

    pub fn is_discarding(&self) -> bool {
        matches!(self, Gate::Discarding)
    }

    /// Whether `received` is kept in the state of this moment.
    pub fn keeps(&self, received: &Received) -> bool {
        match self {
            Gate::Discarding => false,
            Gate::Writing { from } => !received.is_before(*from),
        }
    }
}
