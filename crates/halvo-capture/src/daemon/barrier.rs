//! The owner's one discarding state. A pause starts it when the owner handles
//! the command, at a barrier drawn from the sink's writer; a resume ends it at
//! a second barrier and a resume mark. The owner starts in it.

use crate::sink::{Barrier, Received, ResumeMark, SinkWriter};

/// Whether the owner writes the events that it receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Every received event is discarded.
    Discarding,
    /// Events numbered at or after `from` whose emit began after `mark` are
    /// kept.
    Writing { from: Barrier, mark: ResumeMark },
}

impl Gate {
    /// The state that the owner starts in.
    pub const fn closed() -> Gate {
        Gate::Discarding
    }

    /// Starts discarding and returns the barrier `b`: every event received
    /// before it was received before the pause. The owner reads no command
    /// while it waits for an event, so the event numbered b − 1, which a pass
    /// may hold received and not yet filtered, may have arrived after the
    /// pause was asked: it is discarded with the queued ones and counted as
    /// discarded by the barrier.
    pub fn open_pause(&mut self, writer: &SinkWriter) -> Barrier {
        *self = Gate::Discarding;
        writer.barrier()
    }

    /// Stops discarding and returns the barrier `r`. An event is kept from
    /// then on when it is numbered at or after `r` and its emit began after
    /// the resume mark that this call draws: an emit that began inside the
    /// pause is discarded, whenever its send lands.
    pub fn close_pause(&mut self, writer: &SinkWriter) -> Barrier {
        let mark = writer.resume_mark();
        let from = writer.barrier();
        *self = Gate::Writing { from, mark };
        from
    }

    pub fn is_discarding(&self) -> bool {
        matches!(self, Gate::Discarding)
    }

    /// Whether `received` is kept in the state of this moment.
    pub fn keeps(&self, received: &Received) -> bool {
        match self {
            Gate::Discarding => false,
            Gate::Writing { from, mark } => {
                !received.is_before(*from) && !received.began_before(*mark)
            }
        }
    }
}
