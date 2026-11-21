//! The worker's read channel on macOS: one socket pair. The worker gets its end at two descriptors, and the slot
//! closes its own side by a shutdown of the socket, which ends a read of the serving thread at once.

use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

/// The end that the serving call reads, and the end that it writes: two descriptors of the slot's one socket.
pub(super) type Input = UnixStream;
pub(super) type Output = UnixStream;

/// What closes the slot's side of the stream from another thread than the serving one.
pub(super) struct Closer(UnixStream);

impl Closer {
    /// Shuts the socket down in both directions: a read of the serving thread answers the end of the stream, and
    /// a write of it fails.
    pub(super) fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

/// A new read channel: the slot's side and the worker's two ends, the end that it reads and the end that it writes.
pub(super) struct Stream {
    pub(super) input: Input,
    pub(super) output: Output,
    pub(super) closer: Closer,
    pub(super) worker: (halvo_cap::End, halvo_cap::End),
}

pub(super) fn stream() -> io::Result<Stream> {
    let (ours, theirs) = UnixStream::pair()?;
    Ok(Stream {
        input: ours.try_clone()?,
        output: ours.try_clone()?,
        closer: Closer(ours),
        worker: (theirs.try_clone()?.into(), theirs.into()),
    })
}
