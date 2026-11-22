//! The request `relay` (contracts.md C24.4 of the workflows run): the app hands one request of the role interface to
//! the owner of the derived store through the daemon, and gets the owner's reply back as the owner wrote it.
//!
//! ```text
//! relay{request: <object>}      answers      relayed{reply: <object>}
//! ```
//!
//! The request is `{"v":2,"body":{"op":"relay","request":{..}}}` and the reply
//! `{"v":2,"body":{"kind":"relayed","reply":{..}}}`; the request names no role. The handler reads one name inside the
//! object of `request` and nothing else of it: the key of the daemon's own frame `stream` on the owner's channel,
//! `STREAM_KEY` of owner_stream.rs, which the owner reads before its request decoder. Any other object it sends to the
//! owner as one frame on the owner's channel, and the owner decodes it strictly and takes the role from that channel,
//! which admits the role interface alone. A refusal is an error reply of v1: `invalid_argument` for a body with a key
//! beside `op` and `request`, for a `request` that is no JSON object, for an object that holds the key of the frame
//! `stream` at its top level, and for an object whose frame to the owner would hold more than 4,096 bytes;
//! `store_unavailable` at the end of the owner's channel, at the write or at the answer; and `internal` when the owner
//! does not answer within 4 s of the handler's start, when its frame is no JSON object or would not fit the reply
//! frame, and for any other failure of the channel.

use std::io::ErrorKind;
use std::time::Instant;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::owner_stream::{v1_answer, CLIENT_DEADLINE, OWNER_ANSWER_WAIT, STREAM_KEY};
use super::{Connection, OwnerChannel};
use crate::client::{is_timeout, read_reply, Client, ClientError, Connector, Timed};
use crate::proto::{encode_reply, ErrorCode, ErrorReply, Reply, UnknownRequest};
use crate::wire::{write_frame, FrameError, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};

#[cfg(target_os = "macos")]
use super::owner_stream_macos as sys;
#[cfg(windows)]
use super::owner_stream_windows as sys;

/// The bytes of a request `relay` before its object, and those of a reply `relayed` before the owner's frame; both
/// end with [`TAIL`].
const REQUEST_HEAD: &[u8] = br#"{"v":2,"body":{"op":"relay","request":"#;
const REPLY_HEAD: &[u8] = br#"{"v":2,"body":{"kind":"relayed","reply":"#;
const TAIL: &[u8] = b"}}";

/// The request `relay`, which carries one serialized request of the role interface to the owner of the derived store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relay;

/// The reply `relayed`, which carries the owner's reply back to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relayed;

/// Why bytes cannot be put into a frame of `relay` or `relayed`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelayError {
    #[error("the relayed bytes are no JSON object")]
    NotAnObject,
    #[error("the frame would hold {len} bytes, above the limit of {max}")]
    TooLong { len: usize, max: usize },
}

/// Why a frame is no reply `relayed`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelayedError {
    #[error("the reply is not an envelope with a version")]
    NotAnEnvelope,
    #[error("the reply is of protocol version {0}")]
    Version(u64),
    #[error("the reply is no relayed of protocol version 2 with an object as its reply")]
    Body,
}

#[derive(Deserialize)]
struct VersionOf {
    v: u64,
}

fn is_object(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Map<String, Value>>(bytes).is_ok()
}

/// `object` between `head` and [`TAIL`], where `object` is a JSON object and the frame holds at most `max` bytes.
fn wrap(head: &[u8], object: &[u8], max: usize) -> Result<Vec<u8>, RelayError> {
    if !is_object(object) {
        return Err(RelayError::NotAnObject);
    }
    let len = head.len() + object.len() + TAIL.len();
    if len > max {
        return Err(RelayError::TooLong { len, max });
    }
    Ok([head, object, TAIL].concat())
}

fn invalid_argument(what: &'static str) -> ErrorReply {
    ErrorReply::new(ErrorCode::InvalidArgument, what)
}

fn internal(what: &'static str) -> ErrorReply {
    ErrorReply::new(ErrorCode::Internal, what)
}

impl Relay {
    pub const OP: &'static str = "relay";

    /// The bytes of the request around `request`, which the owner reads, unchanged: an error, and nothing to send,
    /// where `request` is no JSON object or the frame would hold more than 4,096 bytes.
    pub fn encode(request: &[u8]) -> Result<Vec<u8>, RelayError> {
        wrap(REQUEST_HEAD, request, REQUEST_MAX_BYTES)
    }

    /// The frame that the handler hands to the owner for a body that the server decoded as unknown: the object of
    /// `request` as serde_json writes it again, since the server keeps the decoded body and not the client's bytes. A
    /// body with a key beside `op` and `request`, a `request` that is no JSON object, an object that holds the key of
    /// the daemon's own frame `stream` at its top level, which the owner would read as that frame, and an object whose
    /// frame would hold more than 4,096 bytes are refused with `invalid_argument`.
    pub fn from_request(request: &UnknownRequest) -> Result<Vec<u8>, ErrorReply> {
        let body = request.body();
        let object = match body.get("request") {
            Some(Value::Object(object)) if body.len() == 2 && request.op() == Some(Self::OP) => {
                object
            }
            _ => {
                return Err(invalid_argument(
                    "the request relay holds op and an object request alone",
                ))
            }
        };
        if object.contains_key(STREAM_KEY) {
            return Err(invalid_argument(
                "the relayed request holds the key of the daemon's own frame stream",
            ));
        }
        let frame = serde_json::to_vec(object).expect("a map of JSON values always serializes");
        if frame.len() > REQUEST_MAX_BYTES {
            return Err(invalid_argument(
                "the relayed request holds more than 4096 bytes",
            ));
        }
        Ok(frame)
    }
}

impl Relayed {
    pub const KIND: &'static str = "relayed";

    /// The bytes of the reply around `reply`, the owner's frame, unchanged: an error where `reply` is no JSON object
    /// or the frame would hold more than 262,144 bytes. A reply of 261,120 bytes, the bound of the owner's reply,
    /// always fits.
    pub fn encode(reply: &[u8]) -> Result<Vec<u8>, RelayError> {
        wrap(REPLY_HEAD, reply, REPLY_MAX_BYTES)
    }

    /// The bytes of the owner's reply in a reply frame: the envelope's version first, then the one spelling of
    /// [`Relayed::encode`] around a JSON object.
    pub fn decode(frame: &[u8]) -> Result<Vec<u8>, RelayedError> {
        let VersionOf { v } =
            serde_json::from_slice(frame).map_err(|_| RelayedError::NotAnEnvelope)?;
        if v != super::VERSION {
            return Err(RelayedError::Version(v));
        }
        frame
            .strip_prefix(REPLY_HEAD)
            .and_then(|rest| rest.strip_suffix(TAIL))
            .filter(|reply| is_object(reply))
            .map(<[u8]>::to_vec)
            .ok_or(RelayedError::Body)
    }
}

impl<C: Connector> Client<C> {
    /// Relays `request`, one serialized request of the role interface, to the owner of the derived store and returns
    /// the bytes of the owner's reply, within 5 s from the connect (contracts.md C24.4 item 5 of the workflows run).
    /// The call makes one connection, sends one frame and ends its request. Bytes that are no JSON object, or whose
    /// frame would hold more than 4,096 bytes, are [`ClientError::Protocol`] with nothing sent; an error reply of v1 is
    /// [`ClientError::Daemon`], or [`ClientError::ServiceEnding`] where it carries the mark `ending`; no reply in time
    /// is [`ClientError::Deadline`].
    pub fn relay(&self, request: &[u8]) -> Result<Vec<u8>, ClientError> {
        let frame =
            Relay::encode(request).map_err(|error| ClientError::Protocol(error.to_string()))?;
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
        match Relayed::decode(&reply) {
            Ok(bytes) => Ok(bytes),
            Err(RelayedError::Version(1)) => Err(v1_answer(&reply)),
            Err(RelayedError::Version(version)) => {
                Err(ClientError::ReplyOfAnotherVersion { version })
            }
            Err(other) => Err(ClientError::Protocol(other.to_string())),
        }
    }
}

/// The refusal for a failure of the owner's channel: its end is `store_unavailable`, anything else `internal`.
fn channel_failed(error: &std::io::Error) -> ErrorReply {
    match error.kind() {
        ErrorKind::BrokenPipe => ended(),
        _ => internal("the request cannot be relayed to the owner of the derived store"),
    }
}

fn ended() -> ErrorReply {
    ErrorReply::new(
        ErrorCode::StoreUnavailable,
        "the owner of the derived store ended before it answered",
    )
}

/// The reply frame to `request`: `relayed` with the owner's frame, or the refusal of the module's text.
fn reply_to(
    request: &UnknownRequest,
    owner: &mut dyn OwnerChannel,
    answer_by: Instant,
) -> Result<Vec<u8>, ErrorReply> {
    let frame = Relay::from_request(request)?;
    owner.send(&frame).map_err(|error| channel_failed(&error))?;
    let reply = match owner.answer(answer_by.saturating_duration_since(Instant::now())) {
        Ok(Some(reply)) => reply,
        Ok(None) => return Err(ended()),
        Err(error) if error.kind() == ErrorKind::TimedOut => {
            return Err(internal(
                "the owner of the derived store did not answer within 4 s",
            ))
        }
        Err(error) => return Err(channel_failed(&error)),
    };
    Relayed::encode(&reply)
        .map_err(|_| internal("the owner's reply is no JSON object that fits the reply frame"))
}

/// Answers the request `relay` on `connection` (contracts.md C24.4 of the workflows run).
pub(super) fn serve(
    request: &UnknownRequest,
    connection: Connection<'_>,
    owner: &mut dyn OwnerChannel,
) {
    let answer_by = Instant::now() + OWNER_ANSWER_WAIT;
    let reply = reply_to(request, owner, answer_by)
        .unwrap_or_else(|refusal| encode_reply(&Reply::Error(refusal)));
    // The client reads no reply when this write fails.
    let _ = sys::write_reply(connection, &reply);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Transport;
    use crate::proto::{decode_request, Decoded};
    use std::io::{self, Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// An owner's channel that records each frame of `send`, or ends there, and gives one scripted answer.
    struct Channel {
        sent: Vec<Vec<u8>>,
        ends_at_send: bool,
        answer: Option<io::Result<Option<Vec<u8>>>>,
    }

    impl Channel {
        fn answering(answer: io::Result<Option<Vec<u8>>>) -> Channel {
            Channel {
                sent: Vec::new(),
                ends_at_send: false,
                answer: Some(answer),
            }
        }

        #[cfg(target_os = "macos")]
        fn ending_at_send() -> Channel {
            Channel {
                sent: Vec::new(),
                ends_at_send: true,
                answer: None,
            }
        }
    }

    impl OwnerChannel for Channel {
        #[cfg(target_os = "macos")]
        fn send_stream(&mut self, _: &[u8], _: std::os::fd::BorrowedFd<'_>) -> io::Result<()> {
            unreachable!("the handler of relay hands no end")
        }

        #[cfg(windows)]
        fn owner_process(&self) -> std::os::windows::io::BorrowedHandle<'_> {
            unreachable!("the handler of relay hands no handle")
        }

        #[cfg(windows)]
        fn send_stream(&mut self, _: &[u8]) -> io::Result<()> {
            unreachable!("the handler of relay hands no end")
        }

        fn send(&mut self, frame: &[u8]) -> io::Result<()> {
            if self.ends_at_send {
                return Err(ErrorKind::BrokenPipe.into());
            }
            self.sent.push(frame.to_vec());
            Ok(())
        }

        fn answer(&mut self, _: Duration) -> io::Result<Option<Vec<u8>>> {
            self.answer.take().expect("the handler reads one answer")
        }
    }

    fn unknown(frame: &[u8]) -> UnknownRequest {
        match decode_request(frame) {
            Ok(Decoded::Unknown(request)) => request,
            other => panic!("the request decodes as {other:?}"),
        }
    }

    fn answer_by() -> Instant {
        Instant::now() + OWNER_ANSWER_WAIT
    }

    /// An object `{"s":"aaa.."}` of `len` bytes.
    fn object_of(len: usize) -> Vec<u8> {
        format!(r#"{{"s":"{}"}}"#, "a".repeat(len - 8)).into_bytes()
    }

    /// Serves `request` on the server's end of a socket pair as the daemon's server does, with `owner` as the owner's
    /// channel: whether `dispatch_unknown` answered it, the channel, and the reply frame that the client read.
    #[cfg(target_os = "macos")]
    fn served(request: &[u8], owner: Channel) -> (bool, Channel, Vec<u8>) {
        use crate::wire::read_frame;
        use std::os::unix::net::UnixStream;
        let (mut client, server) = UnixStream::pair().unwrap();
        let handler = std::thread::spawn(move || {
            let mut owner = owner;
            let frame = read_frame(&mut &server, REQUEST_MAX_BYTES).unwrap();
            let answered = crate::r2::dispatch_unknown(&unknown(&frame), &server, &mut owner);
            (answered, owner)
        });
        client
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        write_frame(&mut client, request, REQUEST_MAX_BYTES).unwrap();
        let reply = read_frame(&mut client, REPLY_MAX_BYTES).unwrap();
        let (answered, owner) = handler.join().unwrap();
        (answered, owner, reply)
    }

    /// Ruling 1: one spelling of the request and of the reply; a body with a third key, a `request` that is a string,
    /// an object that the handler would write above 4,096 bytes and an object that holds the key of the daemon's own
    /// frame `stream` are `invalid_argument`, and the owner gets no frame.
    #[test]
    fn relay_has_one_spelling() {
        let read = br#"{"v":1,"id":null,"request":"interface_read","body":{"read":"workflows"}}"#;
        assert_eq!(
            Relay::encode(read).unwrap(),
            br#"{"v":2,"body":{"op":"relay","request":{"v":1,"id":null,"request":"interface_read","body":{"read":"workflows"}}}}"#
        );
        let reply = br#"{"v":1,"id":null,"ok":null}"#;
        let relayed = br#"{"v":2,"body":{"kind":"relayed","reply":{"v":1,"id":null,"ok":null}}}"#;
        assert_eq!(Relayed::encode(reply).unwrap(), relayed);
        assert_eq!(Relayed::decode(relayed).unwrap(), reply);

        let floats = format!(r#"{{"n":[{}]}}"#, vec!["1e9"; 1_000].join(","));
        for (frame, why) in [
            (
                br#"{"v":2,"body":{"op":"relay","request":{},"x":1}}"#.to_vec(),
                "a third key",
            ),
            (
                br#"{"v":2,"body":{"op":"relay","request":"{}"}}"#.to_vec(),
                "a request that is a string",
            ),
            (
                Relay::encode(floats.as_bytes()).unwrap(),
                "an object written again above 4,096 bytes",
            ),
            (
                Relay::encode(br#"{"stream":{"handles":[3,4]}}"#).unwrap(),
                "the frame stream with two handles",
            ),
            (
                Relay::encode(br#"{"stream":"taken"}"#).unwrap(),
                "the answer taken",
            ),
            (
                Relay::encode(br#"{"v":1,"stream":{"handles":null}}"#).unwrap(),
                "the key stream beside another key",
            ),
        ] {
            let mut owner = Channel::answering(Ok(Some(b"{}".to_vec())));
            let answer = reply_to(&unknown(&frame), &mut owner, answer_by());
            assert!(owner.sent.is_empty(), "{why}: the owner got a frame");
            assert_eq!(
                answer.unwrap_err().code(),
                ErrorCode::InvalidArgument,
                "{why}"
            );
        }
    }

    /// Ruling 2: `dispatch_unknown` serves `relay`, hands the owner the object of `request` as one frame, also one
    /// that names the role runner, and answers `relayed` with the owner's frame.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_handler_hands_the_object_to_the_owner_and_answers_relayed() {
        let object = br#"{"v":1,"id":"0123","request":"runner","body":{"op":"load"}}"#;
        let answer = br#"{"v":1,"id":"0123","error":"role"}"#;
        let (answered, owner, reply) = served(
            &Relay::encode(object).unwrap(),
            Channel::answering(Ok(Some(answer.to_vec()))),
        );
        assert!(answered, "dispatch_unknown left the request to the server");
        assert_eq!(owner.sent.len(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&owner.sent[0]).unwrap(),
            serde_json::from_slice::<Value>(object).unwrap()
        );
        assert_eq!(Relayed::decode(&reply).unwrap(), answer);
    }

    /// Ruling 3: the end of the owner's channel, at the write and at the answer, is `store_unavailable`; no answer in
    /// time and a frame that is no JSON object are `internal`; each an error reply of v1.
    #[cfg(target_os = "macos")]
    #[test]
    fn each_refusal_of_the_handler_has_its_code() {
        use crate::proto::decode_reply;
        let request = Relay::encode(br#"{"read":"workflows"}"#).unwrap();
        for (owner, code, why) in [
            (
                Channel::ending_at_send(),
                ErrorCode::StoreUnavailable,
                "the end at the write",
            ),
            (
                Channel::answering(Ok(None)),
                ErrorCode::StoreUnavailable,
                "the end at the answer",
            ),
            (
                Channel::answering(Err(ErrorKind::TimedOut.into())),
                ErrorCode::Internal,
                "no answer in time",
            ),
            (
                Channel::answering(Ok(Some(b"[1]".to_vec()))),
                ErrorCode::Internal,
                "a frame that is no object",
            ),
        ] {
            let (answered, _, reply) = served(&request, owner);
            assert!(answered, "{why}: dispatch_unknown left the request");
            match decode_reply(&reply).unwrap() {
                Reply::Error(error) => assert_eq!(error.code(), code, "{why}"),
                other => panic!("{why}: the reply is {other:?}"),
            }
        }
    }

    /// Ruling 4: a request frame of 4,097 bytes is the encoder's error, one of 4,096 is not; a reply of exactly
    /// 261,120 bytes encodes to at most 262,144; a reply that does not fit is `internal`.
    #[test]
    fn a_long_request_is_not_sent_and_a_full_reply_fits() {
        assert_eq!(
            Relay::encode(&object_of(4_096 - 40)).map(|frame| frame.len()),
            Ok(4_096)
        );
        assert_eq!(
            Relay::encode(&object_of(4_097 - 40)),
            Err(RelayError::TooLong {
                len: 4_097,
                max: REQUEST_MAX_BYTES
            })
        );
        let full = object_of(261_120);
        let frame = Relayed::encode(&full).unwrap();
        assert!(frame.len() <= REPLY_MAX_BYTES, "{} bytes", frame.len());
        assert_eq!(Relayed::decode(&frame).unwrap(), full);

        let request = unknown(&Relay::encode(br#"{"read":"workflows"}"#).unwrap());
        let mut owner = Channel::answering(Ok(Some(object_of(REPLY_MAX_BYTES))));
        let refusal = reply_to(&request, &mut owner, answer_by()).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::Internal);
    }

    /// A stand-in transport: what is written goes to `written`, and reads give `reads`, or `TimedOut` with none.
    struct Standin {
        written: Arc<Mutex<Vec<u8>>>,
        reads: Option<io::Cursor<Vec<u8>>>,
    }

    impl Read for Standin {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.reads.as_mut() {
                Some(reads) => reads.read(buf),
                None => Err(ErrorKind::TimedOut.into()),
            }
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

    /// A stand-in connector: its connection reads `reply` as one frame, or times out where there is none.
    struct Connect {
        written: Arc<Mutex<Vec<u8>>>,
        reply: Option<Vec<u8>>,
    }

    impl Connector for Connect {
        type Transport = Standin;

        fn endpoint(&self) -> String {
            "the stand-in".to_owned()
        }

        fn connect(&self, _: Duration) -> Result<Standin, ClientError> {
            let reads = self.reply.as_ref().map(|reply| {
                let mut framed = Vec::new();
                write_frame(&mut framed, reply, REPLY_MAX_BYTES).unwrap();
                io::Cursor::new(framed)
            });
            Ok(Standin {
                written: Arc::clone(&self.written),
                reads,
            })
        }
    }

    /// The client call with `request` against a daemon that answers `reply`, and the bytes that it wrote.
    fn call(reply: Option<Vec<u8>>, request: &[u8]) -> (Result<Vec<u8>, ClientError>, Vec<u8>) {
        let written = Arc::new(Mutex::new(Vec::new()));
        let client = Client::new(Connect {
            written: Arc::clone(&written),
            reply,
        });
        let result = client.relay(request);
        let written = written.lock().unwrap().clone();
        (result, written)
    }

    /// Ruling 5: the client call returns the bytes of `reply`; an error reply `busy` is `Daemon`, no reply in time
    /// `Deadline { seconds: 5 }`, and request bytes that are no JSON object or too long are `Protocol` with nothing
    /// written.
    #[test]
    fn the_client_call_returns_the_reply_bytes_and_names_each_error() {
        let read = br#"{"read":"workflows"}"#;
        let answer = br#"{"v":1,"id":null,"ok":null}"#;
        let (result, written) = call(Some(Relayed::encode(answer).unwrap()), read);
        assert_eq!(result.unwrap(), answer);
        let mut sent = Vec::new();
        write_frame(&mut sent, &Relay::encode(read).unwrap(), REQUEST_MAX_BYTES).unwrap();
        assert_eq!(written, sent);

        let busy = encode_reply(&Reply::Error(ErrorReply::new(
            ErrorCode::Busy,
            "the delete of everything runs",
        )));
        match call(Some(busy), read).0 {
            Err(ClientError::Daemon(error)) => assert_eq!(error.code(), ErrorCode::Busy),
            other => panic!("a reply busy gives {other:?}"),
        }
        match call(None, read).0 {
            Err(ClientError::Deadline { seconds: 5 }) => {}
            other => panic!("no reply gives {other:?}"),
        }
        for request in [b"[]".to_vec(), object_of(4_097 - 40)] {
            let (result, written) = call(Some(Relayed::encode(answer).unwrap()), &request);
            assert!(
                matches!(result, Err(ClientError::Protocol(_))),
                "{result:?}"
            );
            assert!(written.is_empty(), "the call wrote {} bytes", written.len());
        }
    }
}
