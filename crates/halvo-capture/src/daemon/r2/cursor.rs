//! The cursor: the server of the seam protocol (contracts.md C5 of the
//! workflows run) for one client, the batch worker. It is the one way by
//! which captured records leave the capture store for the analysis.
//!
//! Two threads meet in a [`Cursor`]. The serving call, [`Cursor::serve`], runs
//! on the thread of its caller with the two ends of a stream that the caller
//! made: it binds no socket and opens no path. It reads one frame, decodes it
//! with halvo-seam's reader and writes the one reply. An `open_cursor` and a
//! `read_batch` need the store, which the owner's thread alone holds, so the
//! call parks such a message and waits. [`Cursor::pass`] runs in every pass
//! of the store owner: it answers the parked message, at most one a pass and
//! none in a pass that took an event, so a reply waits for the capture
//! (C5.6). A `close`, an `analysis_write` and a message that is not legal now
//! are answered by the serving call at once.
//!
//! What a pass answers:
//!
//! ```text
//! open_cursor   the store's id and its watermark, no record and no expiry
//!               entry; another store id than the one asked for is
//!               store-changed with the current id, a position after the
//!               watermark is bad-position
//! read_batch    the records from the cursor's position on, segment after
//!               segment, until 262,144 bytes of lines are taken, the payload
//!               would pass 262,144 bytes, or the store ends; a changed
//!               store id is store-changed, a segment that the store cannot
//!               read is store-error, and both close the cursor
//! ```
//!
//! A read asks the store for the first live segment at or after its position,
//! so a shredded segment is passed, and it passes in the same way a segment
//! whose expiry instant is at or before the owner's clock. Every line from
//! the position on goes through halvo-seam's reader. A record is served when
//! its kind is one of the five served kinds and the cursor's two filters hold
//! it; a record that is left out keeps its index. A line that the reader
//! refuses, a line of a kind that must not stand in the store among them, is
//! never served: it keeps its index, the read goes on, and
//! [`Cursor::left_out`] counts it. `next` equals `watermark` only when no
//! live segment follows the last line taken.
//!
//! Between two reads the cursor holds a store id, a position and the two
//! filters, and no line. The lines of a segment stand in the buffer that the
//! store's answer wipes when it drops, and the payloads of both directions
//! are wiped after their frame. No text of an error holds a value of a record
//! or of a request, and this file writes no log line.
//!
//! [`Cursor::drop_open`] is the call of a delete of everything, before it
//! removes anything: the connection that is served then is answered no
//! further message, whatever a later pass finds in the store. The process has
//! one cursor, [`cursor`].

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use halvo_schema::{Instant, Position, StoreId};
use halvo_seam::frame::{read_frame, write_frame, FrameError, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::{
    decode_client, Batch, Cause, ClientMessage, Entry, Expiry, KindFilter, OpenCursor, Refusal,
    ServerError, ServerMessage, TimeRange,
};
use halvo_seam::record::CaptureRecord;
use zeroize::Zeroizing;

use crate::daemon::port::{LiveSegment, MaintFault, Ordinal, SegmentRead};

/// The most bytes of record lines, line ends included, that one read takes
/// from the store's segments (contracts.md C5.7 item 5).
pub(in crate::daemon) const READ_MAX: usize = 262_144;

/// The ordinal that an `open_cursor` asks the store at: above every segment,
/// so the store answers its id and its highest ordinal and reads no line.
const PAST_EVERY_SEGMENT: Ordinal = Ordinal::new(u64::MAX);

/// The longest position that a payload holds, 2^63 - 1 in each part.
const LONGEST: Position = Position {
    seg: i64::MAX as u64,
    idx: i64::MAX as u64,
};

const NO_MESSAGE: &str = "the frame is no message of the protocol";
const BAD_VALUE: &str = "a value of the message is refused";
const CURSOR_OPEN: &str = "a cursor is open";
const NO_CURSOR: &str = "no cursor is open";
const NO_WRITE: &str = "this server takes no analysis write";
const STORE_DIFFERS: &str = "store id differs";
const PAST_WATERMARK: &str = "position after the watermark";
const UNREADABLE: &str = "the store cannot be read";

/// The entry of the store owner for a read of the cursor, as the cursor
/// reaches it. Its one impl is for the owner, in the builds that hold one.
pub(in crate::daemon) trait ReadEntry {
    /// The store's answer for the first live segment at or after `from`, and
    /// the wall clock's reading at that read.
    fn read_from(&mut self, from: Ordinal) -> (Result<SegmentRead, MaintFault>, SystemTime);
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
impl<S, C> ReadEntry for crate::daemon::owner::Owner<S, C>
where
    S: crate::daemon::port::CaptureStore,
    C: crate::daemon::maintenance::Clocks,
{
    fn read_from(&mut self, from: Ordinal) -> (Result<SegmentRead, MaintFault>, SystemTime) {
        self.read_segment(from)
    }
}

/// Why a serving call returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::daemon) enum Ended {
    /// The stream ended before a frame.
    EndOfFile,
    /// The call sent `bad-frame` for a frame that is no message.
    BadFrame,
    /// [`Cursor::drop_open`] ran while the call served; it sent nothing after
    /// it.
    Dropped,
    /// The stream ended inside a frame, a read or a write of it failed, or a
    /// reply could not be written as a frame.
    Failed,
    /// Another serving call runs. This one read nothing.
    Busy,
}

/// The cursor that a connection has open: the store that its open reply
/// named, the position of its next read and its two filters. It holds no
/// line and no record.
struct Open {
    store: StoreId,
    position: Position,
    kinds: Option<KindFilter>,
    range: Option<TimeRange>,
}

/// A message of the client that only a pass of the owner can answer.
enum Ask {
    Open(OpenCursor),
    Read,
}

/// A refusal: its cause and its fixed text.
type Refused = (Cause, &'static str);

/// What the serving call writes for one message.
enum Reply {
    Batch(Batch),
    Closed,
    Refused(Refused),
}

/// What stands between the serving call and the owner's pass.
enum Turn {
    /// No message waits.
    Idle,
    /// A message waits for a pass of the owner.
    Asked(Ask),
    /// A pass answered the message that waited, and the serving call has not
    /// taken the reply yet.
    Answered(Reply),
}

struct State {
    /// Whether a serving call runs.
    serving: bool,
    /// Whether `drop_open` ran while the serving call that runs served: that
    /// call writes no further reply.
    dropped: bool,
    open: Option<Open>,
    turn: Turn,
}

impl State {
    const fn idle() -> State {
        State {
            serving: false,
            dropped: false,
            open: None,
            turn: Turn::Idle,
        }
    }
}

/// The server of the seam protocol for one connection at a time.
pub(in crate::daemon) struct Cursor {
    state: Mutex<State>,
    turned: Condvar,
    left_out: AtomicU64,
}

/// Ends the serving call's hold of the cursor when the call returns.
struct Serving<'a>(&'a Cursor);

impl Drop for Serving<'_> {
    fn drop(&mut self) {
        *self.0.state() = State::idle();
    }
}

impl Cursor {
    /// A cursor that serves nothing and has counted nothing.
    pub(in crate::daemon) const fn new() -> Cursor {
        Cursor {
            state: Mutex::new(State::idle()),
            turned: Condvar::new(),
            left_out: AtomicU64::new(0),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Serves one connection on the two stream ends of its caller: one
    /// blocking call that starts no thread and returns when the connection
    /// ends. The cursor of the connection ends with it. While it waits in a
    /// read of `input`, nothing but a frame or the end of the stream wakes
    /// it.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the worker's supervisor of the workflows run calls it"
        )
    )]
    pub(in crate::daemon) fn serve<R: Read, W: Write>(&self, mut input: R, mut output: W) -> Ended {
        {
            let mut state = self.state();
            if state.serving {
                return Ended::Busy;
            }
            *state = State::idle();
            state.serving = true;
        }
        let _serving = Serving(self);
        self.serve_connection(&mut input, &mut output)
    }

    fn serve_connection(&self, input: &mut dyn Read, output: &mut dyn Write) -> Ended {
        loop {
            let decoded = match read_frame(input, CLIENT_FRAME_MAX) {
                Ok(None) => return Ended::EndOfFile,
                Ok(Some(payload)) => decode_client(&Zeroizing::new(payload)),
                Err(FrameError::Length(_)) => Err(Refusal::BadFrame),
                Err(FrameError::End | FrameError::Io(_)) => return Ended::Failed,
            };
            let state = self.state();
            if state.dropped {
                return Ended::Dropped;
            }
            let bad_frame = matches!(decoded, Err(Refusal::BadFrame));
            let answered = match decoded {
                Ok(message) => self.answer(state, message),
                Err(refusal) => {
                    drop(state);
                    Some(Reply::Refused(match refusal {
                        Refusal::BadRequest => (Cause::BadRequest, BAD_VALUE),
                        Refusal::BadFrame => (Cause::BadFrame, NO_MESSAGE),
                    }))
                }
            };
            let Some(reply) = answered else {
                return Ended::Dropped;
            };
            if !send(output, reply) {
                return Ended::Failed;
            }
            if bad_frame {
                return Ended::BadFrame;
            }
        }
    }

    /// The reply to `message`: at once for a message that needs no store, and
    /// after the pass that answered it for an `open_cursor` and a
    /// `read_batch`. None once `drop_open` ran.
    fn answer(&self, mut state: MutexGuard<'_, State>, message: ClientMessage) -> Option<Reply> {
        let refused = |text| Some(Reply::Refused((Cause::BadRequest, text)));
        let ask = match message {
            ClientMessage::OpenCursor(_) if state.open.is_some() => return refused(CURSOR_OPEN),
            ClientMessage::OpenCursor(open) => Ask::Open(open),
            ClientMessage::ReadBatch if state.open.is_none() => return refused(NO_CURSOR),
            ClientMessage::ReadBatch => Ask::Read,
            ClientMessage::Close => {
                return match state.open.take() {
                    Some(_) => Some(Reply::Closed),
                    None => refused(NO_CURSOR),
                };
            }
            ClientMessage::AnalysisWrite(_) => return refused(NO_WRITE),
        };
        state.turn = Turn::Asked(ask);
        loop {
            state = self
                .turned
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
            if state.dropped {
                return None;
            }
            match std::mem::replace(&mut state.turn, Turn::Idle) {
                Turn::Answered(reply) => return Some(reply),
                waiting => state.turn = waiting,
            }
        }
    }

    /// The cursor's part of one pass of the store owner: the message that
    /// waits is answered from the store, one message a pass, and none in a
    /// pass that took an event. The serving call writes the reply.
    pub(in crate::daemon) fn pass(&self, owner: &mut impl ReadEntry, took_event: bool) {
        if took_event {
            return;
        }
        let mut state = self.state();
        let ask = match std::mem::replace(&mut state.turn, Turn::Idle) {
            Turn::Asked(ask) => ask,
            other => {
                state.turn = other;
                return;
            }
        };
        let reply = match ask {
            Ask::Open(open) => opened(&mut state, owner, &open),
            Ask::Read => self.read(&mut state, owner),
        };
        state.turn = Turn::Answered(reply);
        drop(state);
        self.turned.notify_all();
    }

    /// The reply to a `read_batch`. A refusal closes the cursor.
    fn read(&self, state: &mut State, owner: &mut impl ReadEntry) -> Reply {
        let Some(cursor) = state.open.as_mut() else {
            return Reply::Refused((Cause::BadRequest, NO_CURSOR));
        };
        match self.batch(cursor, owner) {
            Ok(batch) => {
                cursor.position = batch.next;
                Reply::Batch(batch)
            }
            Err(refused) => {
                state.open = None;
                Reply::Refused(refused)
            }
        }
    }

    /// The Batch of one read from the position of `cursor` (C5.7): it asks
    /// the store for segment after segment until a bound stops it inside one
    /// or no live segment follows.
    fn batch(&self, cursor: &Open, owner: &mut impl ReadEntry) -> Result<Batch, Refused> {
        let unreadable = (Cause::StoreError, UNREADABLE);
        let mut taken = Taken::new(&cursor.store);
        let mut seg = cursor.position.seg;
        let mut idx = cursor.position.idx;
        loop {
            let (answer, now) = owner.read_from(Ordinal::new(seg));
            let read = answer.map_err(|_| unreadable.clone())?;
            let (store, watermark) = named(&read).ok_or_else(|| unreadable.clone())?;
            if store != cursor.store {
                return Err((Cause::StoreChanged(store), STORE_DIFFERS));
            }
            if watermark < cursor.position {
                return Err(unreadable);
            }
            let Some(live) = read.segment else {
                return Ok(taken.batch(store, watermark, watermark));
            };
            let ordinal = live.ordinal.get();
            if ordinal < seg || ordinal >= watermark.seg {
                return Err(unreadable);
            }
            if ordinal > seg {
                idx = 0;
            }
            if live.expiry > crate::event::unix_nanos(now) {
                if let Some(stop) = self.take_lines(&mut taken, cursor, &live, idx) {
                    return Ok(taken.batch(store, watermark, stop));
                }
            }
            seg = ordinal + 1;
            idx = 0;
        }
    }

    /// Takes the lines of `live` from the index `idx` on. Answers the
    /// position of the line before which a bound stopped the read, or none
    /// when the read reached the end of the segment.
    fn take_lines(
        &self,
        taken: &mut Taken,
        cursor: &Open,
        live: &LiveSegment,
        idx: u64,
    ) -> Option<Position> {
        let seg = live.ordinal.get();
        let before = usize::try_from(idx).unwrap_or(usize::MAX);
        let lines = live.lines.split_inclusive(|byte| *byte == b'\n');
        for (line, idx) in lines.skip(before).zip(idx..) {
            let pos = Position { seg, idx };
            if taken.lines > 0 && taken.lines + line.len() > READ_MAX {
                return Some(pos);
            }
            match CaptureRecord::read_line(line) {
                Ok(record) if serves(cursor, &record) => {
                    if !taken.push(pos, record, live.expiry) {
                        return Some(pos);
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    self.left_out.fetch_add(1, Ordering::Relaxed);
                }
            }
            taken.lines += line.len();
        }
        None
    }

    /// Closes the cursor for a delete of everything, before the delete
    /// removes anything. The cursor is closed, and a message that waits for a
    /// pass gets no reply. The serving call answers no message from here on:
    /// it returns at once when it waits for a pass, and at its next frame or
    /// at the end of its stream when it waits in a read. A reply that it took
    /// from a pass before this call it may still write.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the delete call site of the capture run calls it")
    )]
    pub(in crate::daemon) fn drop_open(&self) {
        let mut state = self.state();
        state.open = None;
        state.turn = Turn::Idle;
        state.dropped = state.serving;
        drop(state);
        self.turned.notify_all();
    }

    /// How many stored lines the reads of this process left out and counted:
    /// the lines that halvo-seam's reader refuses, a line of a kind that must
    /// not stand in the store among them. A line counts once for each read
    /// that passes it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the daemon's status reads it in a later unit")
    )]
    pub(in crate::daemon) fn left_out(&self) -> u64 {
        self.left_out.load(Ordering::Relaxed)
    }
}

/// The reply to an `open_cursor`, which opens the cursor when it is a Batch.
fn opened(state: &mut State, owner: &mut impl ReadEntry, open: &OpenCursor) -> Reply {
    let (answer, _) = owner.read_from(PAST_EVERY_SEGMENT);
    let Some((store, watermark)) = answer.ok().as_ref().and_then(named) else {
        return Reply::Refused((Cause::StoreError, UNREADABLE));
    };
    if open.store().is_some_and(|asked| *asked != store) {
        return Reply::Refused((Cause::StoreChanged(store), STORE_DIFFERS));
    }
    if open.position() > watermark {
        return Reply::Refused((Cause::BadPosition, PAST_WATERMARK));
    }
    state.open = Some(Open {
        store: store.clone(),
        position: open.position(),
        kinds: open.kinds().cloned(),
        range: open.range(),
    });
    Reply::Batch(Batch {
        store,
        watermark,
        records: Vec::new(),
        expiry: Vec::new(),
        next: open.position(),
    })
}

/// The store id and the watermark of a store's answer, in the protocol's
/// types (C5.3). None for a highest ordinal that no watermark lies above.
fn named(read: &SegmentRead) -> Option<(StoreId, Position)> {
    let store = StoreId::parse(&read.store.to_string()).ok()?;
    let highest = read.highest.map_or(0, Ordinal::get);
    let watermark = Position {
        seg: highest.checked_add(1)?,
        idx: 0,
    };
    Some((store, watermark))
}

/// Whether the cursor serves `record` (C5.8): a served kind inside both
/// filters.
fn serves(cursor: &Open, record: &CaptureRecord) -> bool {
    let kind = record.kind();
    kind.served().is_some()
        && cursor.kinds.as_ref().is_none_or(|kinds| kinds.admits(kind))
        && cursor.range.is_none_or(|range| range.contains(record.at()))
}

/// The Batch that one read builds, with the two counts that bound it.
struct Taken {
    records: Vec<Entry>,
    expiry: Vec<Expiry>,
    /// The bytes of the lines that the read took, served or not.
    lines: usize,
    /// The bytes of the frame's payload with what is taken, counted with the
    /// longest watermark and the longest `next`.
    payload: usize,
}

impl Taken {
    fn new(store: &StoreId) -> Taken {
        let empty = ServerMessage::Batch(Batch {
            store: store.clone(),
            watermark: LONGEST,
            records: Vec::new(),
            expiry: Vec::new(),
            next: LONGEST,
        });
        Taken {
            records: Vec::new(),
            expiry: Vec::new(),
            lines: 0,
            payload: empty
                .to_payload()
                .map_or(usize::MAX, |payload| payload.len()),
        }
    }

    /// Takes `record` at `pos`, with the expiry entry of its segment when it
    /// is the first record of that segment, unless the payload would pass
    /// its bound; the first record of a read is always taken. False when the
    /// record was not taken.
    fn push(&mut self, pos: Position, record: CaptureRecord, expiry: i64) -> bool {
        let entry = Entry { pos, record };
        let first_of_segment = self.expiry.last().is_none_or(|last| last.seg != pos.seg);
        let names = first_of_segment.then_some(Expiry {
            seg: pos.seg,
            at: Instant(expiry),
        });
        let grown = self
            .payload
            .saturating_add(element_len(&entry))
            .saturating_add(names.as_ref().map_or(0, element_len));
        if !self.records.is_empty() && grown > SERVER_FRAME_MAX {
            return false;
        }
        self.payload = grown;
        self.expiry.extend(names);
        self.records.push(entry);
        true
    }

    fn batch(self, store: StoreId, watermark: Position, next: Position) -> Batch {
        Batch {
            store,
            watermark,
            records: self.records,
            expiry: self.expiry,
            next,
        }
    }
}

/// The bytes that `value` adds to a payload as one element of an array: its
/// compact JSON, as halvo-seam's writer writes it, and one comma.
fn element_len<T: serde::Serialize>(value: &T) -> usize {
    let mut counted = Counted(0);
    match serde_json::to_writer(&mut counted, value) {
        Ok(()) => counted.0.saturating_add(1),
        Err(_) => usize::MAX,
    }
}

/// Counts the bytes written to it and keeps none.
struct Counted(usize);

impl Write for Counted {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Writes `reply` as one frame and wipes the payload. False when the reply
/// could not be written.
fn send(output: &mut dyn Write, reply: Reply) -> bool {
    let message = match reply {
        Reply::Batch(batch) => ServerMessage::Batch(batch),
        Reply::Closed => ServerMessage::Closed,
        Reply::Refused((cause, detail)) => match ServerError::new(cause, detail) {
            Some(error) => ServerMessage::Error(error),
            None => return false,
        },
    };
    let Ok(payload) = message.to_payload() else {
        return false;
    };
    write_frame(output, &Zeroizing::new(payload), SERVER_FRAME_MAX).is_ok()
}

#[cfg(not(test))]
static CURSOR: Cursor = Cursor::new();

/// The one cursor of the process. The pass of the owner serves from it; the
/// worker's supervisor and the call site of a delete of everything reach it
/// here. In the crate's own tests every thread has a cursor of its own,
/// because every owner of every test passes through this entry.
pub(in crate::daemon) fn cursor() -> &'static Cursor {
    #[cfg(not(test))]
    {
        &CURSOR
    }
    #[cfg(test)]
    {
        tests::of_this_thread()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{PipeReader, PipeWriter};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant as Clock};

    use halvo_control::r2::RunEdge;
    use halvo_schema::{Instant, Position, StoreId};
    use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
    use halvo_seam::message::{
        decode_server, Batch, Cause, ClientMessage, Expiry, KindFilter, OpenCursor, ServerError,
        ServerMessage, TimeRange, ZERO,
    };
    use halvo_seam::record::{Kind, Payload, ServedKind};

    use super::{Cursor, Ended, Turn, READ_MAX};
    use crate::daemon::model_store::{
        app, noon, open_floor, write_one, Harness, ModelStore, AT_NOON, MODEL_ID, PID,
    };
    use crate::daemon::owner::{Answer, Command};
    use crate::daemon::port::{self, MaintFault, Ordinal};
    use crate::daemon::r2::marks::Marks;
    use crate::event::{unix_nanos, AppIdentity, FloorEvent};
    use crate::floor::cases::keep;
    use crate::floor::kind;
    use crate::floor::payload::{Button, KeyClass, Title, TitleWhy};
    use crate::secure_input::{check, FixedProbe, SecureInputReading};

    thread_local! {
        static OF_THIS_THREAD: &'static Cursor = Box::leak(Box::new(Cursor::new()));
    }

    /// The cursor of the calling thread, which lives as long as the process:
    /// the owner that a test passes on its own thread serves this one, and
    /// the owner of no other test does.
    pub(super) fn of_this_thread() -> &'static Cursor {
        OF_THIS_THREAD.with(|cursor| *cursor)
    }

    /// One connection to the cursor of the test's thread: the client's two
    /// stream ends, and the serving call on a thread of its own.
    struct Connection {
        cursor: &'static Cursor,
        requests: PipeWriter,
        replies: PipeReader,
        served: JoinHandle<Ended>,
    }

    impl Connection {
        fn new() -> Connection {
            let cursor = of_this_thread();
            let (requests_in, requests) = std::io::pipe().unwrap();
            let (replies, replies_out) = std::io::pipe().unwrap();
            let served = thread::spawn(move || cursor.serve(requests_in, replies_out));
            Connection {
                cursor,
                requests,
                replies,
                served,
            }
        }

        fn send(&mut self, message: &ClientMessage) {
            write_frame(&mut self.requests, &message.to_payload(), CLIENT_FRAME_MAX).unwrap();
        }

        /// Whether a message waits for a pass.
        fn waits(&self) -> bool {
            matches!(self.cursor.state().turn, Turn::Asked(_))
        }

        /// Returns once the message sent last waits for a pass.
        fn parked(&self) {
            let patience = Clock::now() + Duration::from_secs(20);
            while !self.waits() {
                assert!(Clock::now() < patience, "no message waits for a pass");
                thread::sleep(Duration::from_millis(1));
            }
        }

        /// The next frame of the server, as its message and the bytes of its
        /// payload, or none at end-of-file.
        fn frame(&mut self) -> Option<(ServerMessage, usize)> {
            let payload = read_frame(&mut self.replies, SERVER_FRAME_MAX).unwrap()?;
            Some((decode_server(&payload).unwrap(), payload.len()))
        }

        fn reply(&mut self) -> ServerMessage {
            self.frame().expect("the stream ended").0
        }

        /// Sends a message that needs the store, runs one pass of the owner
        /// and reads the reply of that pass.
        fn in_a_pass(&mut self, harness: &mut Harness, message: &ClientMessage) -> ServerMessage {
            self.send(message);
            self.parked();
            harness.pass();
            assert!(!self.waits(), "the pass answered nothing");
            self.reply()
        }

        /// Sends a message that the serving call answers with no pass.
        fn at_once(&mut self, message: &ClientMessage) -> ServerMessage {
            self.send(message);
            self.reply()
        }

        fn open(&mut self, harness: &mut Harness, open: OpenCursor) -> ServerMessage {
            self.in_a_pass(harness, &ClientMessage::OpenCursor(open))
        }

        fn read(&mut self, harness: &mut Harness) -> ServerMessage {
            self.in_a_pass(harness, &ClientMessage::ReadBatch)
        }

        /// Ends the client's stream and answers why the serving call
        /// returned.
        fn end(self) -> Ended {
            drop(self.requests);
            self.served.join().unwrap()
        }
    }

    fn batch(message: ServerMessage) -> Batch {
        match message {
            ServerMessage::Batch(batch) => batch,
            other => panic!("no batch: {other:?}"),
        }
    }

    fn error(message: ServerMessage) -> ServerError {
        match message {
            ServerMessage::Error(error) => error,
            other => panic!("no error: {other:?}"),
        }
    }

    fn pos(seg: u64, idx: u64) -> Position {
        Position { seg, idx }
    }

    fn positions(batch: &Batch) -> Vec<Position> {
        batch.records.iter().map(|entry| entry.pos).collect()
    }

    fn seam_id(id: port::StoreId) -> StoreId {
        StoreId::parse(&id.to_string()).unwrap()
    }

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn resumed(harness: &mut Harness) {
        assert!(matches!(
            harness.ask(Command::ClosePause),
            Answer::Resumed(_)
        ));
    }

    /// Stores `events` as one segment through the owner and answers its
    /// ordinal.
    fn stored(harness: &mut Harness, events: Vec<FloorEvent>) -> Ordinal {
        let before = harness.store.batches().len();
        for event in events {
            harness.emit(event);
        }
        assert!(matches!(harness.ask(Command::Settle), Answer::Settled(_)));
        let batches = harness.store.batches();
        assert_eq!(batches.len(), before + 1, "the events are no one segment");
        batches[before].ordinal
    }

    fn foreground(age: u64) -> FloorEvent {
        crate::daemon::model_store::foreground(&AT_NOON, secs(age))
    }

    fn title(app: AppIdentity, text: String, age: u64) -> FloorEvent {
        FloorEvent::window_title(
            keep::<kind::WindowTitle>(app, PID),
            Title::new(text, TitleWhy::TitleChange, None),
            Some(secs(age)),
            &AT_NOON,
        )
    }

    fn key() -> FloorEvent {
        let clearance = check(&FixedProbe(SecureInputReading::Inactive), None).unwrap();
        FloorEvent::keystroke(
            keep::<kind::Keystroke>(app(), PID),
            clearance,
            KeyClass::Printable,
            &AT_NOON,
        )
    }

    fn click(age: u64) -> FloorEvent {
        FloorEvent::click(
            keep::<kind::Pointer>(app(), PID),
            Button::Primary,
            Some(secs(age)),
            &AT_NOON,
        )
    }

    fn frame(age: u64) -> FloorEvent {
        FloorEvent::screen_frame(
            keep::<kind::ScreenFrame>(app(), PID),
            Some(secs(age)),
            &AT_NOON,
        )
    }

    #[test]
    fn one_read_a_pass() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        stored(&mut harness, vec![foreground(1)]);
        let mut client = Connection::new();
        batch(client.open(&mut harness, OpenCursor::fresh(None, None)));
        let asked = harness.store.read_calls().len();
        client.send(&ClientMessage::ReadBatch);
        client.parked();
        harness.emit(foreground(0));
        harness.pass();
        assert!(client.waits(), "the pass that took the event answered");
        assert_eq!(harness.store.read_calls().len(), asked);
        harness.pass();
        assert!(!client.waits(), "the pass after it answered nothing");
        let first = batch(client.reply());
        assert_eq!(positions(&first), [pos(1, 0)]);
        assert_eq!(first.next, first.watermark);
        let second = batch(client.read(&mut harness));
        assert!(second.records.is_empty());
        assert_eq!(second.next, second.watermark);
        assert_eq!(client.end(), Ended::EndOfFile);
    }

    #[test]
    fn a_batch_names_its_store() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        let mut instants = Vec::new();
        for day in 1..=3_u32 {
            let ordinal = stored(&mut harness, vec![foreground(u64::from(day))]);
            let instant = unix_nanos(noon() + secs(86_400 * u64::from(day)));
            harness.store.set_expiry(ordinal, instant);
            instants.push(Expiry {
                seg: ordinal.get(),
                at: Instant(instant),
            });
        }
        let mut client = Connection::new();
        let opened = batch(client.open(&mut harness, OpenCursor::fresh(None, None)));
        assert!(opened.records.is_empty() && opened.expiry.is_empty());
        assert_eq!(opened.next, ZERO);
        let read = batch(client.read(&mut harness));
        assert_eq!(positions(&read), [pos(1, 0), pos(2, 0), pos(3, 0)]);
        assert_eq!(read.expiry, instants);
        assert_eq!(read.next, pos(4, 0));
        let last = batch(client.read(&mut harness));
        assert!(last.records.is_empty() && last.expiry.is_empty());
        for reply in [&opened, &read, &last] {
            assert_eq!(reply.store, seam_id(MODEL_ID));
            assert_eq!(reply.watermark, pos(4, 0));
        }
        assert_eq!(client.end(), Ended::EndOfFile);
    }

    #[test]
    fn filters_keep_every_index() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        let mark = noon() - secs(6);
        assert!(Marks::new().request(&mut harness.owner, mark, RunEdge::Start));
        let minutes = title(app(), "Minutes".to_owned(), 4);
        let events = vec![foreground(5), minutes, key(), frame(2), click(1)];
        stored(&mut harness, events);
        let mut client = Connection::new();
        let mut served = |kinds: Option<KindFilter>, range: Option<TimeRange>| {
            batch(client.open(&mut harness, OpenCursor::fresh(kinds, range)));
            let read = batch(client.read(&mut harness));
            assert_eq!(read.next, read.watermark);
            let closed = client.at_once(&ClientMessage::Close);
            assert!(matches!(closed, ServerMessage::Closed));
            let entries = read.records.iter();
            entries
                .map(|entry| (entry.pos, entry.record.kind()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            served(None, None),
            [
                (pos(1, 0), Kind::RunMark),
                (pos(1, 1), Kind::ForegroundApp),
                (pos(1, 2), Kind::WindowTitle),
                (pos(1, 3), Kind::Keystroke),
                (pos(1, 5), Kind::Pointer),
            ]
        );
        let pointers = KindFilter::new(&[ServedKind::Pointer]);
        assert!(pointers.is_some());
        assert_eq!(served(pointers, None), [(pos(1, 5), Kind::Pointer)]);
        let from = Instant(unix_nanos(noon() - secs(5)));
        let range = TimeRange::new(from, Instant(unix_nanos(noon() - secs(1))));
        assert!(range.is_some());
        assert_eq!(
            served(None, range),
            [
                (pos(1, 1), Kind::ForegroundApp),
                (pos(1, 2), Kind::WindowTitle),
            ]
        );
        assert_eq!(client.end(), Ended::EndOfFile);
    }

    #[test]
    fn a_shredded_or_expired_segment_is_passed() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        let ordinals: Vec<Ordinal> = (1..=3)
            .map(|age| stored(&mut harness, vec![foreground(age)]))
            .collect();
        harness.store.shred(ordinals[0]);
        let minute = unix_nanos(noon() + secs(60));
        let after = unix_nanos(noon() + secs(61));
        harness.store.set_expiry(ordinals[1], minute);
        harness.store.set_expiry(ordinals[2], after);
        harness.clocks.advance(secs(60));
        let mut client = Connection::new();
        batch(client.open(&mut harness, OpenCursor::fresh(None, None)));
        let read = batch(client.read(&mut harness));
        assert_eq!(positions(&read), [pos(3, 0)]);
        let named = Expiry {
            seg: 3,
            at: Instant(after),
        };
        assert_eq!(read.expiry, [named]);
        assert_eq!(read.next, read.watermark);
        assert_eq!(client.end(), Ended::EndOfFile);
    }

    #[test]
    fn a_read_stops_at_the_bound() {
        let mut encoder = Harness::over(ModelStore::new(), open_floor(), 4_096);
        resumed(&mut encoder);
        for _ in 0..130 {
            let app = AppIdentity::bundle_id("a".repeat(1_024));
            encoder.emit(title(app, "t".repeat(1_024), 0));
        }
        for _ in 0..2_500 {
            encoder.emit(key());
        }
        assert!(matches!(encoder.ask(Command::Settle), Answer::Settled(_)));
        let encoded = encoder.store.batches();
        let payload: Vec<u8> = encoded
            .iter()
            .flat_map(|segment| segment.payload.iter().copied())
            .collect();
        let lines: Vec<&[u8]> = payload.split_inclusive(|byte| *byte == b'\n').collect();
        assert_eq!(lines.len(), 2_630);
        assert!(payload.len() > READ_MAX, "the segment is under the bound");
        let mut harness = Harness::new();
        write_one(&mut harness.store, &payload).unwrap();
        let mut client = Connection::new();
        batch(client.open(&mut harness, OpenCursor::fresh(None, None)));
        let mut served = Vec::new();
        let mut stops = Vec::new();
        loop {
            client.send(&ClientMessage::ReadBatch);
            client.parked();
            harness.pass();
            let (message, bytes) = client.frame().expect("the stream ended");
            let read = batch(message);
            assert!(bytes <= SERVER_FRAME_MAX, "a payload of {bytes} bytes");
            let from = served.len();
            served.extend(positions(&read));
            let taken: usize = lines[from..served.len()]
                .iter()
                .map(|line| line.len())
                .sum();
            assert!(taken <= READ_MAX, "a read took {taken} bytes of lines");
            if read.next == read.watermark {
                break;
            }
            assert_eq!(read.next, pos(1, u64::try_from(served.len()).unwrap()));
            stops.push((taken + lines[served.len()].len(), bytes));
            assert!(stops.len() < 100, "the reads do not end");
        }
        let every: Vec<Position> = (0..2_630).map(|idx| pos(1, idx)).collect();
        assert_eq!(served, every);
        assert_eq!(stops.len(), 2);
        let (with_the_next_line, _) = stops[0];
        assert!(with_the_next_line > READ_MAX, "no stop at the line bound");
        let (with_the_next_line, bytes) = stops[1];
        assert!(with_the_next_line <= READ_MAX && bytes + 512 > SERVER_FRAME_MAX);
        assert_eq!(client.end(), Ended::EndOfFile);
    }

    #[test]
    fn refusals_name_their_cause() {
        const TITLE: &str = "Northgrid ledger";
        let other = port::StoreId::from_bytes([0xc3; 16]);
        let mut harness = Harness::new();
        resumed(&mut harness);
        stored(&mut harness, vec![title(app(), TITLE.to_owned(), 0)]);
        let mut client = Connection::new();
        let mut refusals = Vec::new();

        let named = OpenCursor::at(seam_id(other), ZERO, None, None).unwrap();
        refusals.push(error(client.open(&mut harness, named)));
        assert_eq!(refusals[0].cause(), &Cause::StoreChanged(seam_id(MODEL_ID)));

        refusals.push(error(client.at_once(&ClientMessage::ReadBatch)));
        assert_eq!(refusals[1].cause(), &Cause::BadRequest);

        let past = OpenCursor::at(seam_id(MODEL_ID), pos(2, 1), None, None).unwrap();
        refusals.push(error(client.open(&mut harness, past)));
        assert_eq!(refusals[2].cause(), &Cause::BadPosition);

        let fresh = || OpenCursor::fresh(None, None);
        batch(client.open(&mut harness, fresh()));
        let again = ClientMessage::OpenCursor(fresh());
        refusals.push(error(client.at_once(&again)));
        assert_eq!(refusals[3].cause(), &Cause::BadRequest);
        let read = batch(client.read(&mut harness));
        assert_eq!(positions(&read), [pos(1, 0)]);
        let Payload::WindowTitle(fed) = read.records[0].record.payload() else {
            panic!("the fed line is no title");
        };
        assert_eq!(fed.text.as_str(), TITLE);

        harness.store.set_id(other);
        refusals.push(error(client.read(&mut harness)));
        assert_eq!(refusals[4].cause(), &Cause::StoreChanged(seam_id(other)));

        batch(client.open(&mut harness, fresh()));
        let fault = MaintFault::Io(std::io::ErrorKind::InvalidData);
        harness.store.fail_reads(Some(fault));
        refusals.push(error(client.read(&mut harness)));
        assert_eq!(refusals[5].cause(), &Cause::StoreError);
        harness.store.fail_reads(None);
        batch(client.open(&mut harness, fresh()));

        for refusal in &refusals {
            let detail = refusal.detail();
            assert!(!detail.is_empty(), "an error with no detail");
            assert!(
                !TITLE.split(' ').any(|word| detail.contains(word)),
                "{detail}"
            );
        }
        assert_eq!(client.end(), Ended::EndOfFile);
    }

    #[test]
    fn a_delete_drops_the_open_cursor() {
        let mut harness = Harness::new();
        resumed(&mut harness);
        stored(&mut harness, vec![foreground(1)]);
        let cursor = of_this_thread();

        let mut client = Connection::new();
        batch(client.open(&mut harness, OpenCursor::fresh(None, None)));
        client.send(&ClientMessage::ReadBatch);
        client.parked();
        let asked = harness.store.read_calls().len();
        cursor.drop_open();
        let Connection {
            mut requests,
            mut replies,
            served,
            ..
        } = client;
        assert_eq!(served.join().unwrap(), Ended::Dropped);
        assert!(read_frame(&mut replies, SERVER_FRAME_MAX)
            .unwrap()
            .is_none());
        let one_more = ClientMessage::ReadBatch.to_payload();
        let _ = write_frame(&mut requests, &one_more, CLIENT_FRAME_MAX);
        harness.pass();
        assert!(read_frame(&mut replies, SERVER_FRAME_MAX)
            .unwrap()
            .is_none());
        assert_eq!(harness.store.read_calls().len(), asked);

        let mut client = Connection::new();
        batch(client.open(&mut harness, OpenCursor::fresh(None, None)));
        let asked = harness.store.read_calls().len();
        cursor.drop_open();
        client.send(&ClientMessage::ReadBatch);
        harness.pass();
        assert!(client.frame().is_none(), "a reply after the drop");
        assert_eq!(client.end(), Ended::Dropped);
        harness.pass();
        assert_eq!(harness.store.read_calls().len(), asked);
    }

    #[test]
    fn a_line_that_must_not_be_served_is_left_out_and_counted() {
        let lines = concat!(
            r#"{"v":1,"id":"019b78c9-0a80-7a1b-8f21-3b5d6c7e8f90","at":1767258000000000000,"seq":0,"kind":"foreground_app","app":"org.example.editor","p":{}}"#,
            "\n",
            r#"{"v":1,"id":"019b78c9-0c1c-7c3d-9a4e-1f20718293a4","at":1767258001000000000,"seq":1,"kind":"microphone","app":"org.example.editor","p":{}}"#,
            "\n",
            r#"{"v":1,"id":"019b78c9-1638-70e4-b7d2-c5a6f8091b2c","at":1767258003000000000,"seq":2,"kind":"keystroke","app":"org.example.editor","p":{"class":"printable"}}"#,
            "\n",
        );
        let mut harness = Harness::new();
        write_one(&mut harness.store, lines.as_bytes()).unwrap();
        let mut client = Connection::new();
        batch(client.open(&mut harness, OpenCursor::fresh(None, None)));
        assert_eq!(of_this_thread().left_out(), 0);
        let read = batch(client.read(&mut harness));
        let served: Vec<_> = read.records.iter().collect();
        assert_eq!(positions(&read), [pos(1, 0), pos(1, 2)]);
        assert_eq!(served[0].record.kind(), Kind::ForegroundApp);
        assert_eq!(served[1].record.kind(), Kind::Keystroke);
        assert_eq!(read.next, read.watermark);
        assert_eq!(of_this_thread().left_out(), 1);
        assert_eq!(client.end(), Ended::EndOfFile);
    }
}
