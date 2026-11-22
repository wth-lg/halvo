//! The request `defer` (contracts.md C24.5 of the workflows run): the app asks the daemon to hold the search for
//! repeated tasks off until a deadline, and gets the deadline in force back.
//!
//! ```text
//! defer{until: <n>}      answers      deferred{until: <n or null>}
//! ```
//!
//! The request is `{"v":2,"body":{"op":"defer","until":<n>}}` and the reply
//! `{"v":2,"body":{"kind":"deferred","until":<n>}}`, or `"until":null` where no deadline is in force; `<n>` is an
//! integer of 64 bits, the nanoseconds since 1970. The daemon decides what is in force: [`serve_defer`] decodes the
//! request, hands its deadline to the setter that the daemon gives, and writes what the setter returned. It starts no
//! owner and uses no owner's channel. A body with a key beside `op` and `until`, or an `until` that is no integer of
//! 64 bits, gets the error reply `invalid_argument` of v1.

use std::time::Instant;

use serde::Deserialize;
use serde_json::Value;

use super::owner_stream::{v1_answer, CLIENT_DEADLINE};
use super::Connection;
use crate::client::{is_timeout, read_reply, Client, ClientError, Connector, Timed};
use crate::proto::{encode_reply, ErrorCode, ErrorReply, Reply, UnknownRequest};
use crate::wire::{write_frame, FrameError, REQUEST_MAX_BYTES};

#[cfg(target_os = "macos")]
use super::owner_stream_macos as sys;
#[cfg(windows)]
use super::owner_stream_windows as sys;

/// The request `defer`, which asks the daemon to hold the search for repeated tasks off until a deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Defer;

/// The reply `deferred`, which holds the deadline in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deferred;

/// Why a frame is no reply `deferred`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeferredError {
    #[error("the reply is not an envelope with a version")]
    NotAnEnvelope,
    #[error("the reply is of protocol version {0}")]
    Version(u64),
    #[error("the reply is no deferred of protocol version 2 in its one spelling")]
    Body,
}

#[derive(Deserialize)]
struct VersionOf {
    v: u64,
}

#[derive(Deserialize)]
struct UntilOf {
    body: BodyUntil,
}

#[derive(Deserialize)]
struct BodyUntil {
    until: Option<i64>,
}

impl Defer {
    pub const OP: &'static str = "defer";

    /// The bytes of the request with the deadline `until`, in nanoseconds since 1970.
    pub fn encode(until: i64) -> Vec<u8> {
        format!(r#"{{"v":2,"body":{{"op":"defer","until":{until}}}}}"#).into_bytes()
    }
}

impl Deferred {
    /// The bytes of the reply with the deadline in force, in nanoseconds since 1970; none is `null`.
    pub fn encode(until: Option<i64>) -> Vec<u8> {
        let until = until.map_or_else(|| "null".to_owned(), |until| until.to_string());
        format!(r#"{{"v":2,"body":{{"kind":"deferred","until":{until}}}}}"#).into_bytes()
    }

    /// The deadline in force that a reply frame holds: the envelope's version first, then the one spelling of
    /// [`Deferred::encode`], so a body with a second key beside `kind` and `until` is an error.
    pub fn decode(frame: &[u8]) -> Result<Option<i64>, DeferredError> {
        let VersionOf { v } =
            serde_json::from_slice(frame).map_err(|_| DeferredError::NotAnEnvelope)?;
        if v != super::VERSION {
            return Err(DeferredError::Version(v));
        }
        let UntilOf {
            body: BodyUntil { until },
        } = serde_json::from_slice(frame).map_err(|_| DeferredError::Body)?;
        if Deferred::encode(until) != frame {
            return Err(DeferredError::Body);
        }
        Ok(until)
    }
}

impl<C: Connector> Client<C> {
    /// Asks the daemon to hold the search for repeated tasks off until `until`, in nanoseconds since 1970, and
    /// returns the deadline in force, none where none is, within 5 s from the connect (contracts.md C24.5 of the
    /// workflows run). The call makes one connection, sends one frame and ends its request. An error reply of v1 is
    /// [`ClientError::Daemon`], or [`ClientError::ServiceEnding`] where it carries the mark `ending`; no reply in time
    /// is [`ClientError::Deadline`].
    pub fn defer(&self, until: i64) -> Result<Option<i64>, ClientError> {
        let frame = Defer::encode(until);
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
        let mut timed = Timed {
            transport: &mut transport,
            deadline,
        };
        write_frame(&mut timed, &frame, REQUEST_MAX_BYTES).map_err(frame_error)?;
        timed
            .end_request()
            .map_err(|e| frame_error(FrameError::Io(e)))?;
        let reply = read_reply(&mut timed).map_err(frame_error)?;
        match Deferred::decode(&reply) {
            Ok(until) => Ok(until),
            Err(DeferredError::Version(1)) => Err(v1_answer(&reply)),
            Err(DeferredError::Version(version)) => {
                Err(ClientError::ReplyOfAnotherVersion { version })
            }
            Err(other) => Err(ClientError::Protocol(other.to_string())),
        }
    }
}

/// The deadline of a request `defer`: `invalid_argument` for a body with a key beside `op` and `until`, and for an
/// `until` that is no integer of 64 bits.
fn until_of(request: &UnknownRequest) -> Result<i64, ErrorReply> {
    let body = request.body();
    match body.get("until").and_then(Value::as_i64) {
        Some(until) if body.len() == 2 => Ok(until),
        _ => Err(ErrorReply::new(
            ErrorCode::InvalidArgument,
            "the request defer holds op and an integer until of 64 bits alone",
        )),
    }
}

/// Answers the request `defer` on `connection` (contracts.md C24.5 of the workflows run): true when the request is a
/// `defer` of version 2, false with nothing written for any other. For a good body the call hands its deadline to
/// `set` once and writes `deferred` with what `set` returned; for any other `defer` it writes `invalid_argument` and
/// does not call `set`. The server checks the peer before this call, by its `PeerPolicy`.
pub fn serve_defer(
    request: &UnknownRequest,
    connection: Connection<'_>,
    set: impl FnOnce(i64) -> Option<i64>,
) -> bool {
    if request.version() != super::VERSION || request.op() != Some(Defer::OP) {
        return false;
    }
    let reply = match until_of(request) {
        Ok(until) => Deferred::encode(set(until)),
        Err(refusal) => encode_reply(&Reply::Error(refusal)),
    };
    // The client reads no reply when this write fails.
    let _ = sys::write_reply(connection, &reply);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Transport;
    use crate::wire::REPLY_MAX_BYTES;
    use std::io::{self, Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// A deadline in nanoseconds since 1970: 2027-01-15.
    const UNTIL: i64 = 1_800_000_000_000_000_000;

    /// Ruling 1: one spelling of the request and of the reply, for a number and for null; a reply with a second key
    /// beside `kind` and `until` is an error.
    #[test]
    fn defer_has_one_spelling() {
        assert_eq!(
            Defer::encode(5),
            br#"{"v":2,"body":{"op":"defer","until":5}}"#
        );
        let held = br#"{"v":2,"body":{"kind":"deferred","until":1800000000000000000}}"#;
        let none = br#"{"v":2,"body":{"kind":"deferred","until":null}}"#;
        assert_eq!(Deferred::encode(Some(UNTIL)), held);
        assert_eq!(Deferred::encode(None), none);
        assert_eq!(Deferred::decode(held), Ok(Some(UNTIL)));
        assert_eq!(Deferred::decode(none), Ok(None));
        assert_eq!(
            Deferred::decode(br#"{"v":2,"body":{"kind":"deferred","until":5,"x":1}}"#),
            Err(DeferredError::Body)
        );
    }

    /// A stand-in transport: what is written goes to `written`, and reads give `reads`.
    struct Standin {
        written: Arc<Mutex<Vec<u8>>>,
        reads: io::Cursor<Vec<u8>>,
    }

    impl Read for Standin {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.reads.read(buf)
        }
    }

    impl Write for Standin {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Transport for Standin {
        fn set_timeout(&mut self, _: Duration) -> io::Result<()> {
            Ok(())
        }

        fn end_request(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// A stand-in connector: its connection reads `reply` as one frame.
    struct Connect {
        written: Arc<Mutex<Vec<u8>>>,
        reply: Vec<u8>,
    }

    impl Connector for Connect {
        type Transport = Standin;

        fn endpoint(&self) -> String {
            "the stand-in".to_owned()
        }

        fn connect(&self, _: Duration) -> Result<Standin, ClientError> {
            let mut framed = Vec::new();
            write_frame(&mut framed, &self.reply, REPLY_MAX_BYTES).unwrap();
            Ok(Standin {
                written: Arc::clone(&self.written),
                reads: io::Cursor::new(framed),
            })
        }
    }

    /// The client call with the deadline `UNTIL` against a daemon that answers `reply`, and the bytes that it wrote.
    fn call(reply: Vec<u8>) -> (Result<Option<i64>, ClientError>, Vec<u8>) {
        let written = Arc::new(Mutex::new(Vec::new()));
        let client = Client::new(Connect {
            written: Arc::clone(&written),
            reply,
        });
        let result = client.defer(UNTIL);
        let written = written.lock().unwrap().clone();
        (result, written)
    }

    /// Ruling 2: the client call sends the request and returns the deadline of `deferred`, none for null; an error
    /// reply of v1 is `Daemon`.
    #[test]
    fn the_client_call_returns_the_deadline() {
        let (result, written) = call(Deferred::encode(Some(UNTIL - 1)));
        assert_eq!(result.unwrap(), Some(UNTIL - 1));
        let mut sent = Vec::new();
        write_frame(&mut sent, &Defer::encode(UNTIL), REQUEST_MAX_BYTES).unwrap();
        assert_eq!(written, sent);

        assert_eq!(call(Deferred::encode(None)).0.unwrap(), None);

        let refused = encode_reply(&Reply::Error(ErrorReply::new(
            ErrorCode::InvalidArgument,
            "the request defer holds op and an integer until of 64 bits alone",
        )));
        match call(refused).0 {
            Err(ClientError::Daemon(error)) => {
                assert_eq!(error.code(), ErrorCode::InvalidArgument)
            }
            other => panic!("an error reply of v1 gives {other:?}"),
        }
    }

    /// Serves `request` on the server's end of a socket pair as the daemon's server does, with a setter that records
    /// each deadline and returns 7: whether `serve_defer` answered it, the deadlines that reached the setter, and
    /// every byte that the client read until the server's end closed.
    #[cfg(target_os = "macos")]
    fn served(request: &[u8]) -> (bool, Vec<i64>, Vec<u8>) {
        use crate::proto::{decode_request, Decoded};
        use crate::wire::read_frame;
        use std::os::unix::net::UnixStream;
        let (mut client, server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        write_frame(&mut client, request, REQUEST_MAX_BYTES).unwrap();
        let frame = read_frame(&mut &server, REQUEST_MAX_BYTES).unwrap();
        let Ok(Decoded::Unknown(unknown)) = decode_request(&frame) else {
            panic!("the request is no request of protocol v2");
        };
        let mut set = Vec::new();
        let answered = serve_defer(&unknown, &server, |until| {
            set.push(until);
            Some(7)
        });
        drop(server);
        let mut written = Vec::new();
        client.read_to_end(&mut written).unwrap();
        (answered, set, written)
    }

    /// Ruling 3: a good body reaches the setter once and the client reads `deferred` with what the setter returned;
    /// a third key and an `until` that is no integer get `invalid_argument` with the setter not called; an op
    /// `relay` returns false with nothing written.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_handler_answers_and_refuses() {
        use crate::proto::decode_reply;
        use crate::wire::read_frame;
        let (answered, set, written) = served(&Defer::encode(UNTIL));
        assert!(answered, "serve_defer left the request to the server");
        assert_eq!(set, [UNTIL]);
        let reply = read_frame(&mut written.as_slice(), REPLY_MAX_BYTES).unwrap();
        assert_eq!(Deferred::decode(&reply), Ok(Some(7)));

        for (request, why) in [
            (
                &br#"{"v":2,"body":{"op":"defer","until":5,"x":1}}"#[..],
                "a third key",
            ),
            (
                &br#"{"v":2,"body":{"op":"defer","until":5.5}}"#[..],
                "an until that is no integer",
            ),
        ] {
            let (answered, set, written) = served(request);
            assert!(answered, "{why}: serve_defer left the request");
            assert!(set.is_empty(), "{why}: the setter was called");
            let reply = read_frame(&mut written.as_slice(), REPLY_MAX_BYTES).unwrap();
            match decode_reply(&reply).unwrap() {
                Reply::Error(error) => assert_eq!(error.code(), ErrorCode::InvalidArgument, "{why}"),
                other => panic!("{why}: the reply is {other:?}"),
            }
        }

        let relay = super::super::Relay::encode(br#"{"read":"workflows"}"#).unwrap();
        let (answered, set, written) = served(&relay);
        assert!(!answered, "serve_defer answered a relay");
        assert!(set.is_empty(), "a relay reached the setter");
        assert!(written.is_empty(), "serve_defer wrote {} bytes", written.len());
    }
}
