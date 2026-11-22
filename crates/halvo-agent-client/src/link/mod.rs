//! The product's owner link (A6.6, unit U9.2): `StreamLink` sends each request as one frame of C5.2 on an end of an
//! owner stream (the workflow run's W-C24.13) and reads one frame as its reply. It takes each end from `Ends`.
//!
//! A stream that ends before a whole reply is never a completed write: at an end-of-file, a read or write error or a
//! length outside the bound the link closes its end and returns `LinkError` for the request in flight, which it does
//! not send again, because the owner may have committed it. The next request asks `Ends` for a new end. The link sets
//! no deadline on a reply.
//!
//! An end that ended before a request is no fault (unit U9.9): before it writes the first byte of a request, the link
//! reads whether the far end has ended the stream, as an owner does that exits while the link is idle. Such an end
//! the link closes; it takes one new end from `Ends` in its place, at most once a request, and sends the request on
//! that end alone. A new end that has ended too costs the request `LinkError`, unsent. An end that comes after the
//! read is met as above.

pub mod product;

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

    /// Whether the far end has ended the stream: on macOS an end-of-file or a reset waits on the socket, on Windows
    /// the pipe that reads is broken. A byte that waits is no end. The link reads it between two requests alone, when
    /// no reply is in flight, and the read neither waits nor takes a byte (see `peek`).
    fn ended(&self) -> bool {
        #[cfg(unix)]
        let reads = &self.stream;
        #[cfg(windows)]
        let reads = &self.reads;
        peek::ended(reads)
    }
}

/// The one call that reads whether an end has ended.
mod peek {
    #![allow(unsafe_code)]
    #![deny(clippy::undocumented_unsafe_blocks)]

    /// One `recv` of at most one byte with `MSG_PEEK | MSG_DONTWAIT` (the values of macOS, the one unix system that
    /// this crate builds for): it leaves what it sees on the socket, and it answers at once with `EWOULDBLOCK` when
    /// nothing waits. It answers 0 at an end-of-file alone.
    #[cfg(unix)]
    pub(super) fn ended(stream: &std::os::unix::net::UnixStream) -> bool {
        use std::os::fd::AsRawFd;
        const MSG_PEEK: i32 = 0x2;
        const MSG_DONTWAIT: i32 = 0x80;
        extern "C" {
            fn recv(socket: i32, buffer: *mut std::ffi::c_void, length: usize, flags: i32)
                -> isize;
        }
        let mut byte = 0u8;
        loop {
            // SAFETY: recv writes at most one byte, into `byte`, which lives across the call, and reads the
            // descriptor that `stream` holds open.
            let got = unsafe {
                recv(
                    stream.as_raw_fd(),
                    (&raw mut byte).cast(),
                    1,
                    MSG_PEEK | MSG_DONTWAIT,
                )
            };
            if got >= 0 {
                return got == 0;
            }
            match std::io::Error::last_os_error().kind() {
                std::io::ErrorKind::Interrupted => {}
                kind => return kind == std::io::ErrorKind::ConnectionReset,
            }
        }
    }

    /// One `PeekNamedPipe` with no buffer on the handle that reads: it copies and takes nothing, and it answers at
    /// once. It fails with `ERROR_BROKEN_PIPE` when every handle that writes to the pipe is closed.
    #[cfg(windows)]
    pub(super) fn ended(reads: &std::fs::File) -> bool {
        use std::ffi::c_void;
        use std::os::windows::io::AsRawHandle;
        const ERROR_BROKEN_PIPE: i32 = 109;
        #[link(name = "kernel32")]
        extern "system" {
            fn PeekNamedPipe(
                pipe: *mut c_void,
                buffer: *mut c_void,
                size: u32,
                read: *mut u32,
                waiting: *mut u32,
                left: *mut u32,
            ) -> i32;
        }
        let mut waiting = 0u32;
        // SAFETY: with no buffer and a size of 0, PeekNamedPipe copies nothing; it writes the count of the bytes that
        // wait into `waiting`, which lives across the call, and reads the handle that `reads` holds open.
        let peeked = unsafe {
            PeekNamedPipe(
                reads.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &raw mut waiting,
                std::ptr::null_mut(),
            )
        };
        peeked == 0 && std::io::Error::last_os_error().raw_os_error() == Some(ERROR_BROKEN_PIPE)
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

    /// A new end from `ends`; at a refusal the link holds `Refused` from now.
    fn new_end(&self, state: &mut State) -> Result<End, LinkError> {
        self.ends.end().map_err(|_| {
            *state = State::Refused(Instant::now());
            LinkError
        })
    }
}

impl<E: Ends> OwnerLink for StreamLink<E> {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let end = match std::mem::replace(&mut *state, State::Closed) {
            State::Open(end) => end,
            State::Refused(at) if at.elapsed() < self.ask_again => {
                *state = State::Refused(at);
                return Err(LinkError);
            }
            State::Refused(_) | State::Closed => self.new_end(&mut state)?,
        };
        let mut end = if end.ended() {
            drop(end);
            let new = self.new_end(&mut state)?;
            if new.ended() {
                return Err(LinkError);
            }
            new
        } else {
            end
        };
        let reply = end.exchange(request).map_err(|_| LinkError)?;
        *state = State::Open(end);
        Ok(reply)
    }
}
