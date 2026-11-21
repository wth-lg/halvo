//! The request `owner_stream` (contracts.md C24.13 of the workflows run): a client asks the daemon for one end of a
//! new byte stream to the owner of the derived store.
//!
//! ```text
//! owner_stream{}   answers   owner_stream_ready{handles: [<n>,<n>]|null}   and hands the client one end of a new stream
//! ```
//!
//! The request is `{"v":2,"body":{"op":"owner_stream"}}`. The reply is
//! `{"v":2,"body":{"kind":"owner_stream_ready","handles":null}}` on macOS, where the end comes beside the frame, and
//! names the client's two handles of its end by their values, the one that reads first, on Windows. A refusal is an
//! error reply of v1 with no end beside it: `invalid_argument` for a body with a key, `busy` when the owner holds
//! four streams, `store_unavailable` at the end of the owner's channel, and `internal` when the stream cannot be made,
//! when the owner answers `refused` or does not answer within 4 s of the request, and when the end cannot be handed
//! to the client.
//!
//! The handler, in this order: it makes one new stream; it hands the owner's end to the owner with the frame `stream`
//! and reads the owner's answer, keeping its own copy of that end until the answer has come; only on
//! `{"stream":"taken"}` it hands the client's end to the client with the reply; and it closes every end that it still
//! holds. On macOS it keeps its copy of the client's end until the client has ended its request, which the client
//! does once it holds the end, or for 1 s.

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::client::{Client, ClientError, Connector, Transport};
use crate::proto::{
    decode_reply, encode_reply, ErrorCode, ErrorReply, Reply, ReplyError, UnknownRequest,
};
use crate::wire::{write_frame, FrameError, REQUEST_MAX_BYTES};

#[cfg(target_os = "macos")]
use super::owner_stream_macos as sys;
#[cfg(windows)]
use super::owner_stream_windows as sys;

pub use sys::{CarriesEnds, Connection, OwnerChannel};

/// The client's deadline, the relay's: the daemon may have to start the owner first.
pub const CLIENT_DEADLINE: Duration = Duration::from_secs(5);

/// How long the handler waits, from the start of the request, for the owner's answer to the frame `stream`; it leaves
/// 1 s of the client's 5 s.
pub const OWNER_ANSWER_WAIT: Duration = Duration::from_secs(4);

/// The owner's answers to the frame `stream` that the handler reads: any other frame is a refusal.
const TAKEN: &[u8] = br#"{"stream":"taken"}"#;
const FULL: &[u8] = br#"{"stream":"full"}"#;

/// The request `owner_stream`, a body of protocol v2 with no field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerStream;

#[derive(Serialize)]
struct Envelope<B> {
    v: u64,
    body: B,
}

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum RequestBody {
    OwnerStream,
}

impl OwnerStream {
    pub const OP: &'static str = "owner_stream";

    /// The bytes of the request: `{"v":2,"body":{"op":"owner_stream"}}`.
    pub fn encode(self) -> Vec<u8> {
        serde_json::to_vec(&Envelope {
            v: super::VERSION,
            body: RequestBody::OwnerStream,
        })
        .expect("a value of these types always serializes")
    }

    /// The request in a body that the server decoded as unknown: a body that holds a key beside `op` is refused with
    /// `invalid_argument`.
    pub fn from_request(request: &UnknownRequest) -> Result<Self, ErrorReply> {
        match request.body().len() {
            1 => Ok(OwnerStream),
            _ => Err(ErrorReply::new(
                ErrorCode::InvalidArgument,
                "the request owner_stream has no field",
            )),
        }
    }
}

/// The reply `owner_stream_ready`. On macOS `handles` is null and the client's end comes beside the frame; on Windows
/// it names the client's two handles of its end by their values in the client's process, the one that reads first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerStreamReady {
    pub handles: Option<[u64; 2]>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReadyKind {
    OwnerStreamReady,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadyBody {
    kind: ReadyKind,
    #[serde(deserialize_with = "Option::deserialize")]
    handles: Option<[u64; 2]>,
}

#[derive(Deserialize)]
struct VersionOf {
    v: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadyEnvelope {
    #[serde(rename = "v")]
    _version: u64,
    body: ReadyBody,
}

/// Why a frame is no reply `owner_stream_ready`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadyError {
    #[error("the reply is not an envelope with a version")]
    NotAnEnvelope,
    #[error("the reply is of protocol version {0}")]
    Version(u64),
    #[error("the reply is no owner_stream_ready of protocol version 2: {0}")]
    Body(String),
}

impl OwnerStreamReady {
    /// The bytes of the reply.
    pub fn encode(self) -> Vec<u8> {
        serde_json::to_vec(&Envelope {
            v: super::VERSION,
            body: ReadyBody {
                kind: ReadyKind::OwnerStreamReady,
                handles: self.handles,
            },
        })
        .expect("a value of these types always serializes")
    }

    /// Reads a reply frame: the envelope's version first, then a body with the two keys `kind` and `handles` alone,
    /// whose `handles` is null or two integers.
    pub fn decode(frame: &[u8]) -> Result<Self, ReadyError> {
        let VersionOf { v } =
            serde_json::from_slice(frame).map_err(|_| ReadyError::NotAnEnvelope)?;
        if v != super::VERSION {
            return Err(ReadyError::Version(v));
        }
        let ready: ReadyEnvelope =
            serde_json::from_slice(frame).map_err(|e| ReadyError::Body(e.to_string()))?;
        Ok(Self {
            handles: ready.body.handles,
        })
    }
}

/// The client's end of a stream to the owner of the derived store. It closes the end when it is dropped, has no
/// `Clone`, and gives the end up only by a conversion that consumes it: into one `OwnedFd` on macOS, and on Windows
/// into two `OwnedHandle` values, the one that reads and then the one that writes.
#[derive(Debug)]
pub struct OwnerStreamEnd(pub(super) sys::End);

/// The reply `internal` that the client call returns when the daemon's reply comes without the end, or with more than
/// one end, or, on Windows, with values that fail a check.
fn no_end() -> ClientError {
    ClientError::Daemon(ErrorReply::new(
        ErrorCode::Internal,
        "the reply owner_stream_ready came without one end of a stream",
    ))
}

fn is_timeout(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
}

/// The error of the client call for a reply of v1.
pub(super) fn v1_answer(frame: &[u8]) -> ClientError {
    match decode_reply(frame) {
        Ok(Reply::Error(error)) if error.ending() => ClientError::ServiceEnding(error),
        Ok(Reply::Error(error)) => ClientError::Daemon(error),
        Ok(other) => ClientError::UnexpectedReply { kind: other.kind() },
        Err(ReplyError::Status(broken)) => ClientError::StateDoesNotHoldTogether(broken),
        Err(ReplyError::UnknownValue(outside)) => ClientError::ValueUnknownToV1(outside),
        Err(other) => ClientError::Protocol(other.to_string()),
    }
}

impl<C: Connector> Client<C>
where
    C::Transport: CarriesEnds,
{
    /// Asks the daemon for one end of a new stream to the owner of the derived store, within 5 s from the connect.
    /// The call sends the request and does not end it; it reads the reply with the end, sets close-on-exec on what it
    /// received, and then ends its request, which is its answer to the daemon. An error reply is
    /// [`ClientError::Daemon`]; a reply `owner_stream_ready` without exactly one end, or, on Windows, with values that
    /// fail a check, closes what came and is `internal`.
    pub fn owner_stream(&self) -> Result<OwnerStreamEnd, ClientError> {
        let deadline = Instant::now() + CLIENT_DEADLINE;
        let timed_out = || ClientError::Deadline {
            seconds: CLIENT_DEADLINE.as_secs(),
        };
        let frame_error = |error: FrameError| match error {
            FrameError::Io(e) if is_timeout(&e) => timed_out(),
            FrameError::Io(e) => ClientError::Io(e),
            other => ClientError::Protocol(other.to_string()),
        };
        let mut transport = match self.connect(deadline) {
            Err(ClientError::Deadline { .. }) => return Err(timed_out()),
            other => other?,
        };
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(timed_out());
        }
        transport
            .set_timeout(left)
            .map_err(|e| frame_error(FrameError::Io(e)))?;
        write_frame(&mut transport, &OwnerStream.encode(), REQUEST_MAX_BYTES)
            .map_err(frame_error)?;
        let (frame, received) = sys::read_reply(&mut transport, deadline).map_err(frame_error)?;
        let end = match OwnerStreamReady::decode(&frame) {
            Ok(ready) => sys::take_end(&transport, ready, received).ok_or_else(no_end)?,
            Err(ReadyError::Version(1)) => return Err(v1_answer(&frame)),
            Err(ReadyError::Version(version)) => {
                return Err(ClientError::ReplyOfAnotherVersion { version })
            }
            Err(other) => return Err(ClientError::Protocol(other.to_string())),
        };
        // The end is this process's now: an error of the end of the request leaves the daemon to close its copy at
        // the end of its wait.
        let _ = transport.end_request();
        Ok(OwnerStreamEnd(end))
    }
}

/// The refusal that the owner's answer to the frame `stream` gives, or none for `taken`. A broken pipe on the
/// channel is its end.
pub(super) fn judge(answer: std::io::Result<Option<Vec<u8>>>) -> Result<(), ErrorReply> {
    let ended = || {
        ErrorReply::new(
            ErrorCode::StoreUnavailable,
            "the owner of the derived store ended before it answered",
        )
    };
    match answer {
        Ok(Some(frame)) if frame == TAKEN => Ok(()),
        Ok(Some(frame)) if frame == FULL => Err(ErrorReply::new(
            ErrorCode::Busy,
            "the owner of the derived store holds four streams",
        )),
        Ok(None) => Err(ended()),
        Err(e) if e.kind() == ErrorKind::BrokenPipe => Err(ended()),
        Ok(Some(_)) | Err(_) => Err(ErrorReply::new(
            ErrorCode::Internal,
            "the owner of the derived store did not take the end of the stream",
        )),
    }
}

/// The refusal when the stream cannot be made or its end cannot be handed to the client.
pub(super) fn cannot(what: &'static str) -> ErrorReply {
    ErrorReply::new(ErrorCode::Internal, what)
}

/// Answers the request `owner_stream` on `connection` (contracts.md C24.13 item 5 of the workflows run).
pub(super) fn serve(
    request: &UnknownRequest,
    connection: Connection<'_>,
    owner: &mut dyn OwnerChannel,
) {
    let answer_by = Instant::now() + OWNER_ANSWER_WAIT;
    let refusal = match OwnerStream::from_request(request) {
        Ok(OwnerStream) => match sys::serve(connection, owner, answer_by) {
            Ok(()) => return,
            Err(refusal) => refusal,
        },
        Err(refusal) => refusal,
    };
    // The client reads no reply when this write fails, and no end was handed over.
    let _ = sys::write_reply(connection, &encode_reply(&Reply::Error(refusal)));
}
