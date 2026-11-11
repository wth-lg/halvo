use std::io::{self, Read, Write};

use halvo_schema::request::{decode_reply, Reply};
use halvo_schema::{AnalysisWrite, ErrorName, Position, StoreId};

use crate::fixture::FixtureTransport;
use crate::frame::{read_frame, write_frame, FrameError, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use crate::message::{
    decode_server, Batch, Cause, ClientMessage, KindFilter, OpenCursor, ServerError, ServerMessage,
    TimeRange,
};
use crate::record::Kind;

/// Why a read failed (C5.10): the client stopped reading, reported the failure and sends nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// A read or a write of the stream failed.
    Io(io::ErrorKind),
    /// The stream ended before or inside the reply.
    End,
    /// The reply's frame is outside C5.2: a length of 0 or above 262,144 bytes.
    Frame,
    /// The reply's payload is no message of C5.5, or a served record fails the reader of C4.6.
    Message,
    /// The reply is a message of C5.5 that breaks the order of C5.6 or a rule of C5.7 or C5.8; the text
    /// names the rule.
    Order(&'static str),
    /// The server answered `bad-frame`, after which it closes the stream.
    BadFrame,
    /// An earlier call failed, so this one sent nothing.
    Stopped,
}

/// Why a call of the client returned no reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    /// The server answered with an error of C5.10 other than `bad-frame`. After `bad-request` the cursor is as
    /// it was; after `store-changed`, `bad-position` and `store-error` no cursor is open. `store-changed`
    /// holds the id of the server's current store, which the reader reads from the zero position.
    Refused(Cause),
    /// The read failed; the client sends nothing more.
    Failed(Failure),
    /// The call is not legal in the client's state (C5.6): an open while a cursor is open, a read or a close
    /// with none open. Nothing was sent.
    NotNow,
    /// `to_bytes()` of the analysis write refused it with this name. Nothing was sent.
    Write(ErrorName),
}

/// One exchange of the protocol: the client sends one message and waits for the one reply (C5.6).
pub trait Link {
    /// Sends `message` and returns the reply, or the failure that stops the client.
    fn exchange(&mut self, message: &ClientMessage) -> Result<ServerMessage, Failure>;
}

impl<L: Link + ?Sized> Link for &mut L {
    fn exchange(&mut self, message: &ClientMessage) -> Result<ServerMessage, Failure> {
        (**self).exchange(message)
    }
}

/// The in-process entry of the fixture transport, with no framing (C5.12 item 11).
impl Link for FixtureTransport {
    fn exchange(&mut self, message: &ClientMessage) -> Result<ServerMessage, Failure> {
        Ok(self.handle(message))
    }
}

/// The protocol over an inherited byte stream (C5.2): frames of at most 65,536 payload bytes go out and
/// frames of at most 262,144 come in. The two ends may be one stream or one for each direction.
pub struct StreamLink<R, W> {
    reader: R,
    writer: W,
}

impl<R: Read, W: Write> StreamLink<R, W> {
    /// A link that reads replies from `reader` and writes messages to `writer`.
    pub fn new(reader: R, writer: W) -> StreamLink<R, W> {
        StreamLink { reader, writer }
    }

    /// The two stream ends.
    pub fn into_parts(self) -> (R, W) {
        (self.reader, self.writer)
    }
}

impl<R: Read, W: Write> Link for StreamLink<R, W> {
    fn exchange(&mut self, message: &ClientMessage) -> Result<ServerMessage, Failure> {
        let failed = |e: FrameError| match e {
            FrameError::End => Failure::End,
            FrameError::Length(_) => Failure::Frame,
            FrameError::Io(e) => Failure::Io(e.kind()),
        };
        write_frame(&mut self.writer, &message.to_payload(), CLIENT_FRAME_MAX).map_err(failed)?;
        let payload = read_frame(&mut self.reader, SERVER_FRAME_MAX)
            .map_err(failed)?
            .ok_or(Failure::End)?;
        decode_server(&payload).map_err(|_| Failure::Message)
    }
}

/// What the reply to an `open_cursor` gave: the store id that every Batch of the cursor carries, and the
/// watermark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    pub store: StoreId,
    pub watermark: Position,
}

struct Open {
    store: StoreId,
    position: Position,
    watermark: Position,
    kinds: Option<KindFilter>,
    range: Option<TimeRange>,
}

enum State {
    Idle,
    Open(Open),
    Failed,
}

/// The client of the seam protocol over one connection with at most one cursor. It checks every reply
/// against C5.5 to C5.8 and stops at the first that breaks them. It holds no bound of an analysis batch: a
/// reader that stops inside a Batch keeps the `pos` of the first record it did not take as its position, and
/// one that took the whole Batch keeps `next` (C5.7 item 7).
pub struct Client<L> {
    link: L,
    state: State,
}

impl<L: Link> Client<L> {
    /// A client on `link`, with no cursor open.
    pub fn new(link: L) -> Client<L> {
        Client {
            link,
            state: State::Idle,
        }
    }

    /// The link.
    pub fn into_link(self) -> L {
        self.link
    }

    /// The position of the open cursor, the `next` of its last Batch, or none when no cursor is open.
    pub fn position(&self) -> Option<Position> {
        match &self.state {
            State::Open(open) => Some(open.position),
            _ => None,
        }
    }

    fn fail(&mut self, failure: Failure) -> ClientError {
        self.state = State::Failed;
        ClientError::Failed(failure)
    }

    fn refused(&mut self, error: ServerError) -> ClientError {
        match error.cause() {
            Cause::BadFrame => self.fail(Failure::BadFrame),
            Cause::BadRequest => ClientError::Refused(Cause::BadRequest),
            cause => {
                self.state = State::Idle;
                ClientError::Refused(cause.clone())
            }
        }
    }

    fn send(&mut self, message: &ClientMessage) -> Result<ServerMessage, ClientError> {
        if matches!(self.state, State::Failed) {
            return Err(ClientError::Failed(Failure::Stopped));
        }
        self.link.exchange(message).map_err(|f| self.fail(f))
    }

    fn stopped_or(&self, error: ClientError) -> ClientError {
        match self.state {
            State::Failed => ClientError::Failed(Failure::Stopped),
            _ => error,
        }
    }

    /// Opens a cursor. The reply must be a Batch with no record and no expiry entry, with `next` the position
    /// asked for, not above its watermark, and with the store asked for when one was named.
    pub fn open(&mut self, open: OpenCursor) -> Result<Opened, ClientError> {
        if !matches!(self.state, State::Idle) {
            return Err(self.stopped_or(ClientError::NotNow));
        }
        match self.send(&ClientMessage::OpenCursor(open.clone()))? {
            ServerMessage::Batch(b) => {
                let fits = b.has_form()
                    && b.records.is_empty()
                    && b.expiry.is_empty()
                    && b.next == open.position()
                    && b.next <= b.watermark
                    && open.store().is_none_or(|s| s == &b.store);
                if !fits {
                    return Err(self.fail(Failure::Order("the reply to open_cursor")));
                }
                self.state = State::Open(Open {
                    store: b.store.clone(),
                    position: b.next,
                    watermark: b.watermark,
                    kinds: open.kinds().cloned(),
                    range: open.range(),
                });
                Ok(Opened {
                    store: b.store,
                    watermark: b.watermark,
                })
            }
            ServerMessage::Error(e) => Err(self.refused(e)),
            _ => Err(self.fail(Failure::Order("a reply other than batch or error"))),
        }
    }

    /// Reads the next Batch of the open cursor and checks it against C5.7 and C5.8: the store of the open
    /// reply; a watermark that does not fall; `next` not below the cursor's position and not above the
    /// watermark; records at or after the position, below `next`, in rising order, of a served kind and
    /// inside the cursor's filters; and an empty `records` only with a risen `next` or at the watermark.
    pub fn read(&mut self) -> Result<Batch, ClientError> {
        if !matches!(self.state, State::Open(_)) {
            return Err(self.stopped_or(ClientError::NotNow));
        }
        let reply = self.send(&ClientMessage::ReadBatch)?;
        let b = match reply {
            ServerMessage::Batch(b) => b,
            ServerMessage::Error(e) => return Err(self.refused(e)),
            _ => return Err(self.fail(Failure::Order("a reply other than batch or error"))),
        };
        let State::Open(open) = &self.state else {
            return Err(self.fail(Failure::Order("no cursor")));
        };
        if let Some(rule) = broken(open, &b) {
            return Err(self.fail(Failure::Order(rule)));
        }
        if let State::Open(open) = &mut self.state {
            open.position = b.next;
            open.watermark = b.watermark;
        }
        Ok(b)
    }

    /// Closes the open cursor.
    pub fn close(&mut self) -> Result<(), ClientError> {
        if !matches!(self.state, State::Open(_)) {
            return Err(self.stopped_or(ClientError::NotNow));
        }
        match self.send(&ClientMessage::Close)? {
            ServerMessage::Closed => {
                self.state = State::Idle;
                Ok(())
            }
            ServerMessage::Error(e) => Err(self.refused(e)),
            _ => Err(self.fail(Failure::Order("a reply other than closed or error"))),
        }
    }

    /// Sends the analysis write as the bytes that its `to_bytes()` returns (C5.11) and returns the reply that
    /// `request::decode_reply` reads from the `write_ack`. Legal at any time; it changes no cursor.
    pub fn analysis_write(&mut self, write: &AnalysisWrite) -> Result<Reply, ClientError> {
        if matches!(self.state, State::Failed) {
            return Err(ClientError::Failed(Failure::Stopped));
        }
        let message = ClientMessage::analysis_write(write).map_err(ClientError::Write)?;
        match self.send(&message)? {
            ServerMessage::WriteAck(reply) => {
                decode_reply(reply.get().as_bytes()).map_err(|_| self.fail(Failure::Message))
            }
            ServerMessage::Error(e) => Err(self.refused(e)),
            _ => Err(self.fail(Failure::Order("a reply other than write_ack or error"))),
        }
    }
}

fn broken(open: &Open, b: &Batch) -> Option<&'static str> {
    if !b.has_form() {
        return Some("the watermark and the segment ordinals of C5.3");
    }
    if b.store != open.store {
        return Some("one store per cursor");
    }
    if b.watermark < open.watermark {
        return Some("the watermark never falls");
    }
    if b.next < open.position {
        return Some("next is not below the position");
    }
    if b.next > b.watermark {
        return Some("next is not above the watermark");
    }
    let mut last: Option<Position> = None;
    for e in &b.records {
        if e.pos < open.position || e.pos >= b.next || last.is_some_and(|l| e.pos <= l) {
            return Some("positions rise from the position to below next");
        }
        last = Some(e.pos);
        let kind = e.record.kind();
        if kind == Kind::ScreenFrame {
            return Some("no server serves screen_frame");
        }
        if open.kinds.as_ref().is_some_and(|k| !k.admits(kind)) {
            return Some("the kinds filter");
        }
        if open.range.is_some_and(|r| !r.contains(e.record.at())) {
            return Some("the range filter");
        }
    }
    if b.records.is_empty() && b.next == open.position && b.next != b.watermark {
        return Some("an empty batch moves next or stands at the watermark");
    }
    None
}
