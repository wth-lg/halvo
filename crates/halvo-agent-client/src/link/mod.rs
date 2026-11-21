//! The product's owner link (A6.6, unit U9.2): `StreamLink` sends each request as one frame of C5.2 on an end of an
//! owner stream (the workflow run's W-C24.13) and reads one frame as its reply. It takes each end from `Ends`.
//!
//! A stream that ends before a whole reply is never a completed write: at an end-of-file, a read or write error or a
//! length outside the bound the link closes its end and returns `LinkError` for the request in flight, which it does
//! not send again, because the owner may have committed it. The next request asks `Ends` for a new end. The link sets
//! no deadline on a reply.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use halvo_seam::frame::{read_frame, write_frame, FrameError, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};

use crate::api::{LinkError, OwnerLink};

/// How long the link asks nobody for an end after `Ends` refused one: a request in that time gets `LinkError`.
pub const ASK_AGAIN: Duration = Duration::from_secs(1);

/// Why `Ends` gave no end: the refusals of the request `owner_stream` that leave the link without a stream
/// (W-C24.13 item 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Busy,
    StoreUnavailable,
    Internal,
}

/// Where the link gets each end of an owner stream. In the product it is the call of the request `owner_stream`.
pub trait Ends: Send + Sync {
    fn end(&self) -> Result<End, Refusal>;
}

/// One end of an owner stream, which the link alone holds: one descriptor on macOS; on Windows two handles, the one
/// that reads and then the one that writes. Dropped, it is closed; on macOS the socket is shut down first, so that the
/// owner reads end-of-file also where another copy of the descriptor is open.
pub struct End {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    #[cfg(windows)]
    reads: std::fs::File,
    #[cfg(windows)]
    writes: std::fs::File,
}

impl End {
    #[cfg(unix)]
    pub fn new(end: std::os::fd::OwnedFd) -> End {
        End { stream: end.into() }
    }

    #[cfg(windows)]
    pub fn new(
        reads: std::os::windows::io::OwnedHandle,
        writes: std::os::windows::io::OwnedHandle,
    ) -> End {
        End {
            reads: reads.into(),
            writes: writes.into(),
        }
    }

    /// Writes `request` as one frame and reads one frame as its reply. An end-of-file before the reply's first
    /// byte is an error, as an end inside it is.
    fn exchange(&mut self, request: &[u8]) -> Result<Vec<u8>, FrameError> {
        #[cfg(unix)]
        let (mut reads, mut writes) = (&self.stream, &self.stream);
        #[cfg(windows)]
        let (mut reads, mut writes) = (&self.reads, &self.writes);
        write_frame(&mut writes, request, CLIENT_FRAME_MAX)?;
        read_frame(&mut reads, SERVER_FRAME_MAX)?.ok_or(FrameError::End)
    }
}

#[cfg(unix)]
impl Drop for End {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// What the link holds between two requests.
enum State {
    /// No end: the next request asks `Ends`.
    Closed,
    /// `Ends` refused at this instant: until `ask_again` after it a request asks nobody.
    Refused(Instant),
    Open(End),
}

/// The product's `OwnerLink` over an end of an owner stream. It sends one request at a time: a request waits for the
/// reply of the one before it.
pub struct StreamLink<E> {
    ends: E,
    ask_again: Duration,
    state: Mutex<State>,
}

impl<E: Ends> StreamLink<E> {
    /// A link that asks `ends` for its first end at its first request and waits `ASK_AGAIN` after a refusal.
    pub fn new(ends: E) -> StreamLink<E> {
        StreamLink::with_ask_again(ends, ASK_AGAIN)
    }

    /// A link that waits `ask_again` after a refusal of `ends`.
    pub fn with_ask_again(ends: E, ask_again: Duration) -> StreamLink<E> {
        StreamLink {
            ends,
            ask_again,
            state: Mutex::new(State::Closed),
        }
    }
}

impl<E: Ends> OwnerLink for StreamLink<E> {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let mut end = match std::mem::replace(&mut *state, State::Closed) {
            State::Open(end) => end,
            State::Refused(at) if at.elapsed() < self.ask_again => {
                *state = State::Refused(at);
                return Err(LinkError);
            }
            State::Refused(_) | State::Closed => match self.ends.end() {
                Ok(end) => end,
                Err(_) => {
                    *state = State::Refused(Instant::now());
                    return Err(LinkError);
                }
            },
        };
        let reply = end.exchange(request).map_err(|_| LinkError)?;
        *state = State::Open(end);
        Ok(reply)
    }
}
