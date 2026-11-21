//! The control server of a service run (contracts.md C2.1, C3.2 step 6). It
//! serves protocol v1, one request for each connection, to peers of the
//! daemon's own user alone, and hands each request that it admits to the
//! service run's loop, which answers it between two turns of its main loop.

#[cfg(target_os = "macos")]
pub mod unix;

use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use halvo_control::proto::{
    self, Admission, Decoded, ErrorCode, ErrorReply, Life, Reply, Request, PERMISSION_FORM,
};
use halvo_control::wire::{self, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};

/// How long the server waits for each read of a request (contracts.md C2.1).
pub const READ_DEADLINE: Duration = Duration::from_secs(2);

/// How long the server may take to write a reply: REPLY_WRITE_DEADLINE_MS of
/// contracts.md C2.1.
pub const REPLY_WRITE_DEADLINE: Duration = Duration::from_millis(2_000);

/// The longest that a connection waits for the service run's loop to answer
/// the request that it handed over: STOP_WAIT_MS, the longest deadline of a
/// client of this build's requests.
pub const ANSWER_WAIT: Duration = Duration::from_millis(halvo_control::STOP_WAIT_MS);

/// Whether the server serves a peer whose effective user id is `peer`, in a
/// daemon whose own effective user id is `own`: only when the two are one.
pub fn serves_peer(peer: u32, own: u32) -> bool {
    peer == own
}

/// What the connections of one server share with the service run's loop: the
/// mutation in flight and the fact that the life is ending (contracts.md
/// C2.1, ruling PL-17), which the loop sets once it accepts a Stop.
#[derive(Debug, Default)]
pub struct Admitting {
    in_flight: AtomicBool,
    ending: AtomicBool,
}

impl Admitting {
    /// Marks the life as ending: from now on every mutation but a Pause and a
    /// Resume answers the busy of an ending life.
    pub fn end_the_life(&self) {
        self.ending.store(true, Ordering::SeqCst);
    }

    fn life(&self) -> Life {
        if self.ending.load(Ordering::SeqCst) {
            Life::Ending
        } else {
            Life::Going
        }
    }
}

/// A request that the server admitted, handed to the service run's loop.
pub struct Asked {
    pub request: Request,
    reply: Sender<Reply>,
    written: Receiver<()>,
    admitting: Arc<Admitting>,
}

impl Asked {
    /// Hands `reply` to the connection; a mutation that it answers is in
    /// flight no longer. The answer tells when the connection has written the
    /// reply and closed.
    pub fn answer(self, reply: Reply) -> Written {
        let _ = self.reply.send(reply);
        if self.request.is_mutation() {
            self.admitting.in_flight.store(false, Ordering::SeqCst);
        }
        Written(self.written)
    }
}

/// The end of a connection whose request the loop answered.
pub struct Written(Receiver<()>);

impl Written {
    /// Waits at most `bound` for the connection to write its reply, shut down
    /// its writing side and close; true when it did.
    pub fn wait(&self, bound: Duration) -> bool {
        self.0.recv_timeout(bound).is_ok()
    }
}

/// Serves the one request of a connection on `stream`, whose peer has the
/// effective user id `peer` (`None` when it could not be read), in a daemon
/// whose own id is `own`. A peer of another user, or one whose id could not
/// be read, gets the error `peer_rejected` and no read of its request. A
/// request that the v1 table does not know is answered at its whole frame; a
/// known one only once its stream has ended after the frame. An admitted
/// request goes to the loop through `hand` and its answer is written. The
/// answer is the sender that the caller signals once it has shut down the
/// writing side and closed the connection, when the loop answered.
pub fn serve_connection<S: Read + Write>(
    stream: &mut S,
    peer: Option<u32>,
    own: u32,
    admitting: &Arc<Admitting>,
    hand: &Sender<Asked>,
) -> Option<Sender<()>> {
    if !peer.is_some_and(|peer| serves_peer(peer, own)) {
        write_reply(
            stream,
            &error(
                ErrorCode::PeerRejected,
                "the peer is not a process of the capture service's own user",
            ),
        );
        return None;
    }
    let request = match read_request(stream) {
        Got::Request(request) => request,
        Got::Answer(reply) => {
            write_reply(stream, &reply);
            return None;
        }
    };
    if let Err(reply) = admit(&request, admitting) {
        write_reply(stream, &reply);
        return None;
    }
    let (reply, answer) = mpsc::channel();
    let (written, written_rx) = mpsc::channel();
    let asked = Asked {
        request,
        reply,
        written: written_rx,
        admitting: admitting.clone(),
    };
    if hand.send(asked).is_err() {
        write_reply(
            stream,
            &error(ErrorCode::Internal, "the capture service is not serving"),
        );
        return None;
    }
    let reply = answer.recv_timeout(ANSWER_WAIT).ok()?;
    write_reply(stream, &reply);
    Some(written)
}

enum Got {
    Request(Request),
    Answer(Reply),
}

fn read_request<S: Read>(stream: &mut S) -> Got {
    let Ok(body) = wire::read_frame(stream, REQUEST_MAX_BYTES) else {
        return Got::Answer(error(
            ErrorCode::InvalidArgument,
            "the request is not one whole frame",
        ));
    };
    match proto::decode_request(&body) {
        Err(refused) => Got::Answer(refused.answer()),
        Ok(Decoded::Unknown(unknown)) => Got::Answer(proto::unknown_answer(&unknown)),
        Ok(Decoded::Known(request)) if at_the_end(stream) => Got::Request(request),
        Ok(Decoded::Known(_)) => Got::Answer(error(
            ErrorCode::InvalidArgument,
            "the request is not one frame followed by the end of its stream",
        )),
    }
}

fn at_the_end<S: Read>(stream: &mut S) -> bool {
    let mut probe = [0u8; 1];
    loop {
        match stream.read(&mut probe) {
            Ok(0) => return true,
            Ok(_) => return false,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
}

fn served_by_this_build(request: &Request) -> bool {
    matches!(
        request,
        Request::Status { .. }
            | Request::Inventory
            | Request::Pause
            | Request::Resume
            | Request::Stop
    )
}

fn admit(request: &Request, admitting: &Admitting) -> Result<(), Reply> {
    if PERMISSION_FORM.serves(request) && !served_by_this_build(request) {
        return Err(error(
            ErrorCode::UnsupportedOp,
            "this build of the capture service does not serve the op",
        ));
    }
    let mutation = request.is_mutation() && PERMISSION_FORM.serves(request);
    let claimed = mutation
        && admitting
            .in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
    let refused = match proto::admission(
        request,
        PERMISSION_FORM,
        mutation && !claimed,
        admitting.life(),
    ) {
        Admission::OtherForm => Some(error(
            ErrorCode::UnsupportedOp,
            "this build of the capture service does not serve the op",
        )),
        Admission::InFlight => Some(error(
            ErrorCode::Busy,
            "another request that changes the capture service is in flight",
        )),
        Admission::Ending => Some(Reply::Error(ErrorReply::busy_of_an_ending_life())),
        Admission::PauseKeepingTheWord
        | Admission::StatusAsItStands
        | Admission::ClaimStartingNothing
        | Admission::Serve => None,
    };
    match refused {
        Some(reply) => {
            if claimed {
                admitting.in_flight.store(false, Ordering::SeqCst);
            }
            Err(reply)
        }
        None => Ok(()),
    }
}

fn error(code: ErrorCode, message: &str) -> Reply {
    Reply::Error(ErrorReply::new(code, message))
}

fn write_reply<S: Write>(stream: &mut S, reply: &Reply) {
    let encoded = proto::encode_reply(reply);
    let body = if encoded.len() > REPLY_MAX_BYTES {
        proto::encode_reply(&error(
            ErrorCode::Internal,
            "the reply is larger than a reply may be",
        ))
    } else {
        encoded
    };
    if wire::write_frame(stream, &body, REPLY_MAX_BYTES).is_ok() {
        let _ = stream.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorded {
        input: std::io::Cursor<Vec<u8>>,
        reads: usize,
        out: Vec<u8>,
    }

    impl Recorded {
        fn holding(request: &Request) -> Recorded {
            let mut input = Vec::new();
            wire::write_frame(
                &mut input,
                &proto::encode_request(request),
                REQUEST_MAX_BYTES,
            )
            .unwrap();
            Recorded {
                input: std::io::Cursor::new(input),
                reads: 0,
                out: Vec::new(),
            }
        }

        fn reply(&self) -> Reply {
            let frame = wire::read_frame(&mut &self.out[..], REPLY_MAX_BYTES).unwrap();
            proto::decode_reply(&frame).unwrap()
        }
    }

    impl Read for Recorded {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reads += 1;
            self.input.read(buf)
        }
    }

    impl Write for Recorded {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.out.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_server_refuses_a_peer_of_another_user() {
        let own = 501;
        assert!(serves_peer(own, own), "the daemon's own user is served");
        for other in [0, 502, u32::MAX] {
            assert!(!serves_peer(other, own), "the user {other} is served");
        }
        let admitting = Arc::new(Admitting::default());
        let (hand, handed) = mpsc::channel();
        for peer in [Some(502), Some(0), None] {
            let mut stream = Recorded::holding(&Request::Stop);
            let written = serve_connection(&mut stream, peer, own, &admitting, &hand);
            assert!(written.is_none(), "{peer:?}");
            assert_eq!(stream.reads, 0, "a refused connection was read: {peer:?}");
            assert_eq!(
                stream.reply().code(),
                Some(ErrorCode::PeerRejected),
                "{peer:?}"
            );
        }
        assert!(handed.try_recv().is_err(), "a refused request reached the loop");
        admitting.in_flight.store(true, Ordering::SeqCst);
        let mut stream = Recorded::holding(&Request::Stop);
        assert!(serve_connection(&mut stream, Some(own), own, &admitting, &hand).is_none());
        assert!(stream.reads > 0, "the own user's request was not read");
        assert_eq!(stream.reply().code(), Some(ErrorCode::Busy));
    }
}
