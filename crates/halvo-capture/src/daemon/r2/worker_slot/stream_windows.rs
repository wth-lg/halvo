//! The worker's read channel on Windows: two anonymous pipes. A read of the serving thread that waits on a pipe
//! cannot be ended from another thread, so the slot's side closes when the serving call returns: at the end of the
//! stream that the worker's end through the Job gives, or at once where the call waits for a pass of the owner.

use std::io::{self, PipeReader, PipeWriter};

/// The end that the serving call reads, and the end that it writes.
pub(super) type Input = PipeReader;
pub(super) type Output = PipeWriter;

/// The slot holds no end of the stream beside those of the serving call, so there is nothing to close.
pub(super) struct Closer;

impl Closer {
    pub(super) fn close(&self) {}
}

/// A new read channel: the slot's side and the worker's two ends, the end that it reads and the end that it writes.
pub(super) struct Stream {
    pub(super) input: Input,
    pub(super) output: Output,
    pub(super) closer: Closer,
    pub(super) worker: (halvo_cap::End, halvo_cap::End),
}

pub(super) fn stream() -> io::Result<Stream> {
    let (input, worker_writes) = io::pipe()?;
    let (worker_reads, output) = io::pipe()?;
    Ok(Stream {
        input,
        output,
        closer: Closer,
        worker: (worker_reads.into(), worker_writes.into()),
    })
}
