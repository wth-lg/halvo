use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, PipeReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

use halvo_schema::{Instant, Position, StoreId};
use halvo_seam::conformance::{self, Subject, CASES};
use halvo_seam::fixture::FixtureTransport;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::{
    decode_client, Batch, Cause, ClientMessage, Entry, Expiry, OpenCursor, Refusal, ServerError,
    ServerMessage, ZERO,
};
use halvo_seam::record::{CaptureRecord, Kind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    EmptyKinds,
    ScreenFrameKind,
    KindTwice,
    EmptyRange,
    NullStoreAtAPosition,
    ReadWithNoCursor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    Conforming,
    RestartLosesASegment,
    ShredNothing,
    LoadKeepsTheOldStore,
    LoadDropsALine,
    IgnoreKinds,
    IgnoreRange,
    OversizeBatch,
    BatchNamesAnotherStore,
    WatermarkFalls,
    NextFalls,
    NextAboveWatermark,
    RepeatsARecord,
    ServesBelowTheCursor,
    RecordAtNext,
    ExpiryDropped,
    ExpiryNamesAnotherSegment,
    ExpiryOutOfOrder,
    ExpiryAtZero,
    ServesScreenFrame,
    StallsWithAnEmptyBatch,
    OversizeOpenReply,
    OpenHoldsARecord,
    OpenHoldsExpiry,
    OpenNextElsewhere,
    OpenNamesAnotherStore,
    BadPositionGetsStoreError,
    StoreChangedNamesTheOldId,
    StoreChangedGetsStoreError,
    RefusalGetsStoreError(Form),
    ReadGetsStoreError,
    OpenGetsClosed,
    ReadGetsClosed,
    CloseGetsBadRequest,
    NeverReachesTheWatermark,
    SendsNoMessage,
    EndsTheStream,
}

#[derive(Clone, Copy, Debug)]
enum Want {
    Pass,
    Fail(&'static str),
}

use Fault::*;
use Want::{Fail, Pass};

fn files_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
        for n in 0..100_000 {
            let dir = base.join(format!("halvo-seam-suite_controls-{n}"));
            if fs::create_dir(&dir).is_ok() {
                return dir;
            }
        }
        panic!("no new directory under CARGO_TARGET_TMPDIR after 100000 names");
    })
}

fn file_of(text: &str) -> Result<PathBuf, String> {
    static WRITTEN: Mutex<Vec<(u64, PathBuf)>> = Mutex::new(Vec::new());
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let key = hasher.finish();
    let mut written = WRITTEN.lock().map_err(|e| e.to_string())?;
    if let Some((_, path)) = written.iter().find(|(k, _)| *k == key) {
        return Ok(path.clone());
    }
    let path = files_dir().join(format!("text-{key:016x}.jsonl"));
    fs::write(&path, text).map_err(|e| e.to_string())?;
    written.push((key, path.clone()));
    Ok(path)
}

fn screen_frames() -> Vec<Entry> {
    let mut out = Vec::new();
    for (i, text) in conformance::SET_A.iter().enumerate() {
        for (k, line) in text.as_bytes().split_inclusive(|b| *b == b'\n').enumerate() {
            let record = CaptureRecord::read_line(line).unwrap();
            if record.kind() == Kind::ScreenFrame {
                let pos = Position {
                    seg: i as u64 + 1,
                    idx: k as u64,
                };
                out.push(Entry { pos, record });
            }
        }
    }
    out
}

fn first_record() -> Entry {
    let line = conformance::SET_A[0].split_inclusive('\n').next().unwrap();
    Entry {
        pos: Position { seg: 1, idx: 0 },
        record: CaptureRecord::read_line(line.as_bytes()).unwrap(),
    }
}

fn other(store: &StoreId) -> StoreId {
    let s = store.as_str();
    let first = if s.starts_with('0') { "1" } else { "0" };
    StoreId::parse(&format!("{first}{}", &s[1..])).unwrap()
}

fn error(cause: Cause) -> ServerMessage {
    ServerMessage::Error(ServerError::new(cause, "control").unwrap())
}

fn refused_form(payload: &[u8]) -> Form {
    let text = String::from_utf8_lossy(payload);
    if text.contains(r#""kinds":[]"#) {
        Form::EmptyKinds
    } else if text.contains("screen_frame") {
        Form::ScreenFrameKind
    } else if text.contains(r#""kinds":["keystroke","keystroke"]"#) {
        Form::KindTwice
    } else if text.contains(r#""store":null"#) {
        Form::NullStoreAtAPosition
    } else {
        Form::EmptyRange
    }
}

struct Proxy {
    fault: Fault,
    set_a: bool,
    frames: Vec<Entry>,
    first: Entry,
    store: Option<StoreId>,
    renamed: bool,
    cursor: Option<Position>,
    open_w: Position,
    reads: usize,
}

impl Proxy {
    fn opened(&mut self, open: &OpenCursor, reply: ServerMessage) -> ServerMessage {
        let mut b = match reply {
            ServerMessage::Batch(b) => b,
            ServerMessage::Error(e) => return self.refused(e.cause().clone(), open.store()),
            other => return other,
        };
        self.cursor = Some(open.position());
        self.store = Some(b.store.clone());
        self.open_w = b.watermark;
        self.reads = 0;
        self.renamed = self.fault == OpenNamesAnotherStore && open.store().is_some();
        match self.fault {
            WatermarkFalls => b.watermark.seg += 1,
            OpenHoldsARecord => b.records.push(self.first.clone()),
            OpenHoldsExpiry => b.expiry.push(Expiry {
                seg: 1,
                at: Instant(0),
            }),
            OpenNextElsewhere => b.next.idx += 1,
            OpenGetsClosed => return ServerMessage::Closed,
            _ => {}
        }
        if self.renamed {
            b.store = other(&b.store);
        }
        ServerMessage::Batch(b)
    }

    fn refused(&mut self, cause: Cause, asked: Option<&StoreId>) -> ServerMessage {
        let cause = match (self.fault, cause) {
            (BadPositionGetsStoreError, Cause::BadPosition) => Cause::StoreError,
            (StoreChangedNamesTheOldId, Cause::StoreChanged(id)) => {
                Cause::StoreChanged(asked.cloned().unwrap_or(id))
            }
            (StoreChangedGetsStoreError, Cause::StoreChanged(_)) => Cause::StoreError,
            (RefusalGetsStoreError(Form::ReadWithNoCursor), Cause::BadRequest) => Cause::StoreError,
            (_, cause) => cause,
        };
        error(cause)
    }

    fn empty(&self, next: Position) -> ServerMessage {
        ServerMessage::Batch(Batch {
            store: self.store.clone().unwrap(),
            watermark: self.open_w,
            records: Vec::new(),
            expiry: Vec::new(),
            next,
        })
    }

    fn read(&mut self, t: &mut FixtureTransport) -> ServerMessage {
        let cursor = self.cursor.unwrap_or(ZERO);
        let first = self.reads == 0;
        self.reads += 1;
        match self.fault {
            StallsWithAnEmptyBatch if first && cursor < self.open_w => return self.empty(cursor),
            NextFalls if first && cursor.idx >= 1 => {
                return self.empty(Position {
                    seg: cursor.seg,
                    idx: cursor.idx - 1,
                })
            }
            NeverReachesTheWatermark => {
                let next = Position {
                    seg: cursor.seg,
                    idx: cursor.idx + 1,
                };
                self.cursor = Some(next);
                return self.empty(next);
            }
            ReadGetsClosed => return ServerMessage::Closed,
            ReadGetsStoreError => return error(Cause::StoreError),
            _ => {}
        }
        let mut b = match t.handle(&ClientMessage::ReadBatch) {
            ServerMessage::Batch(b) => b,
            ServerMessage::Error(e) => return self.refused(e.cause().clone(), None),
            other => return other,
        };
        self.cursor = Some(b.next);
        let reached = !b.records.is_empty() && b.next == b.watermark;
        match self.fault {
            BatchNamesAnotherStore => b.store = other(&b.store),
            NextAboveWatermark if reached => {
                b.next = Position {
                    seg: b.watermark.seg,
                    idx: 1,
                }
            }
            RecordAtNext if reached => b.next = b.records.last().unwrap().pos,
            RepeatsARecord if !b.records.is_empty() => {
                let copy = b.records[0].clone();
                b.records.insert(1, copy);
            }
            ServesBelowTheCursor if !b.records.is_empty() && cursor.idx >= 1 => {
                let entry = Entry {
                    pos: Position {
                        seg: cursor.seg,
                        idx: cursor.idx - 1,
                    },
                    record: b.records[0].record.clone(),
                };
                b.records.insert(0, entry);
            }
            ExpiryDropped if !b.records.is_empty() => b.expiry.clear(),
            ExpiryNamesAnotherSegment => {
                if let Some(last) = b.expiry.last().copied() {
                    b.expiry.push(Expiry {
                        seg: last.seg + 1,
                        at: last.at,
                    });
                }
            }
            ExpiryOutOfOrder => b.expiry.reverse(),
            ExpiryAtZero => b.expiry.iter_mut().for_each(|x| x.at = Instant(0)),
            ServesScreenFrame if self.set_a => {
                let extra = self
                    .frames
                    .iter()
                    .filter(|e| e.pos >= cursor && e.pos < b.next)
                    .cloned();
                b.records.extend(extra);
                b.records.sort_by_key(|e| e.pos);
            }
            _ => {}
        }
        if self.renamed {
            b.store = other(&b.store);
        }
        ServerMessage::Batch(b)
    }

    fn payload(&self, request: &ClientMessage, reply: &ServerMessage) -> Vec<u8> {
        if self.fault == SendsNoMessage && matches!(request, ClientMessage::ReadBatch) {
            return br#"{"msg":"nothing"}"#.to_vec();
        }
        let mut payload = reply.to_payload().unwrap();
        let pad = match request {
            ClientMessage::ReadBatch => self.fault == OversizeBatch,
            ClientMessage::OpenCursor(_) => self.fault == OversizeOpenReply,
            _ => false,
        };
        if pad && matches!(reply, ServerMessage::Batch(_)) {
            payload.resize(SERVER_FRAME_MAX + 1, b' ');
        }
        payload
    }
}

struct Control {
    fault: Fault,
    files: Vec<PathBuf>,
    first_live: u64,
    set_a: bool,
    idle: Option<FixtureTransport>,
    serving: Option<JoinHandle<FixtureTransport>>,
}

impl Control {
    fn new(fault: Fault) -> Control {
        Control {
            fault,
            files: Vec::new(),
            first_live: 1,
            set_a: false,
            idle: None,
            serving: None,
        }
    }

    fn settle(&mut self) {
        if let Some(handle) = self.serving.take() {
            self.idle = Some(handle.join().unwrap());
        }
    }

    fn reopen(&mut self) -> Result<(), String> {
        let t = FixtureTransport::open(&self.files, self.first_live, None)
            .map_err(|e| format!("{e:?}"))?;
        self.idle = Some(t);
        Ok(())
    }
}

fn unfiltered(open: &OpenCursor, fault: Fault) -> OpenCursor {
    let kinds = if fault == IgnoreKinds {
        None
    } else {
        open.kinds().cloned()
    };
    let range = if fault == IgnoreRange {
        None
    } else {
        open.range()
    };
    match open.store() {
        None => OpenCursor::fresh(kinds, range),
        Some(s) => OpenCursor::at(s.clone(), open.position(), kinds, range).unwrap(),
    }
}

impl Subject for Control {
    type Writer = PipeWriter;
    type Reader = PipeReader;

    fn load(&mut self, batch_files: &[&str]) -> Result<(), String> {
        self.settle();
        if self.fault == LoadKeepsTheOldStore && batch_files == conformance::SET_B {
            return self.reopen();
        }
        self.set_a = batch_files == conformance::SET_A;
        self.files = Vec::new();
        for (i, text) in batch_files.iter().enumerate() {
            let mut text = text.to_string();
            if self.fault == LoadDropsALine && i == 1 {
                let cut = text[..text.len() - 1].rfind('\n').map_or(0, |p| p + 1);
                text.truncate(cut);
            }
            self.files.push(file_of(&text)?);
        }
        self.first_live = 1;
        self.reopen()
    }

    fn connect(&mut self) -> Result<(PipeWriter, PipeReader), String> {
        self.settle();
        let mut t = self.idle.take().ok_or("no store")?;
        let mut proxy = Proxy {
            fault: self.fault,
            set_a: self.set_a,
            frames: screen_frames(),
            first: first_record(),
            store: None,
            renamed: false,
            cursor: None,
            open_w: ZERO,
            reads: 0,
        };
        let (mut requests_in, requests_out) = io::pipe().map_err(|e| e.to_string())?;
        let (replies_in, mut replies_out) = io::pipe().map_err(|e| e.to_string())?;
        self.serving = Some(std::thread::spawn(move || {
            while let Ok(Some(payload)) = read_frame(&mut requests_in, CLIENT_FRAME_MAX) {
                let (request, reply) = match decode_client(&payload) {
                    Ok(ClientMessage::OpenCursor(open)) => {
                        let reply = t.handle(&ClientMessage::OpenCursor(unfiltered(
                            &open,
                            proxy.fault,
                        )));
                        let reply = proxy.opened(&open, reply);
                        (ClientMessage::OpenCursor(open), reply)
                    }
                    Ok(ClientMessage::ReadBatch) => {
                        if proxy.fault == EndsTheStream {
                            break;
                        }
                        let reply = proxy.read(&mut t);
                        (ClientMessage::ReadBatch, reply)
                    }
                    Ok(ClientMessage::Close) => {
                        proxy.cursor = None;
                        let reply = match t.handle(&ClientMessage::Close) {
                            ServerMessage::Closed if proxy.fault == CloseGetsBadRequest => {
                                error(Cause::BadRequest)
                            }
                            reply => reply,
                        };
                        (ClientMessage::Close, reply)
                    }
                    Ok(other) => {
                        let reply = t.handle(&other);
                        (other, reply)
                    }
                    Err(Refusal::BadRequest) => {
                        let cause = match proxy.fault {
                            RefusalGetsStoreError(form) if form == refused_form(&payload) => {
                                Cause::StoreError
                            }
                            _ => Cause::BadRequest,
                        };
                        (ClientMessage::ReadBatch, error(cause))
                    }
                    Err(Refusal::BadFrame) => break,
                };
                let reply = proxy.payload(&request, &reply);
                if write_frame(&mut replies_out, &reply, 1 << 20).is_err() {
                    break;
                }
            }
            t
        }));
        Ok((requests_out, replies_in))
    }

    fn restart(&mut self) -> Result<(), String> {
        self.settle();
        if self.fault == RestartLosesASegment {
            self.first_live += 1;
        }
        self.reopen()
    }

    fn shred_oldest(&mut self) -> Result<(), String> {
        self.settle();
        if self.fault != ShredNothing {
            self.first_live += 1;
        }
        self.reopen()
    }
}

fn check(fault: Fault, want: [Want; 4]) {
    let mut subject = Control::new(fault);
    let results = conformance::run(&mut subject);
    let names: Vec<&str> = results.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, CASES);
    for ((name, got), want) in results.iter().zip(want) {
        match (want, got) {
            (Pass, Ok(())) => {}
            (Fail(text), Err(e)) if e.contains(text) => {}
            _ => panic!("{fault:?}, case {name}: want {want:?}, got {got:?}"),
        }
    }
}

#[test]
fn run_passes_a_conforming_server_and_fails_each_broken_one_in_the_case_that_shows_it() {
    let shown = "entries served, want the";
    check(Conforming, [Pass, Pass, Pass, Pass]);
    check(RestartLosesASegment, [Fail(shown), Pass, Pass, Pass]);
    check(ShredNothing, [Pass, Fail(shown), Pass, Pass]);
    check(
        IgnoreKinds,
        [Pass, Pass, Fail("the filter foreground_app: "), Pass],
    );
    check(IgnoreRange, [Pass, Pass, Fail("the filter the range: "), Pass]);
    check(
        LoadKeepsTheOldStore,
        [
            Pass,
            Pass,
            Pass,
            Fail("a message that is due an error got another reply"),
        ],
    );
    let short = "the reference read served 43 entries";
    check(
        LoadDropsALine,
        [Fail(short), Fail(short), Fail(short), Fail(short)],
    );
}

fn everywhere(text: &'static str) -> [Want; 4] {
    [Fail(text), Fail(text), Fail(text), Fail(text)]
}

#[test]
fn control_a_reply_frame_over_the_server_limit() {
    check(OversizeBatch, everywhere("outside 1 to 262,144"));
}

#[test]
fn control_a_batch_of_another_store_than_its_open_reply() {
    check(
        BatchNamesAnotherStore,
        everywhere("a Batch names another store than the reply to its open_cursor"),
    );
}

#[test]
fn control_a_watermark_that_falls() {
    check(WatermarkFalls, everywhere("the watermark fell"));
}

#[test]
fn control_a_next_that_falls() {
    check(
        NextFalls,
        [Fail("next fell"), Fail("next fell"), Pass, Pass],
    );
}

#[test]
fn control_a_next_above_the_watermark() {
    check(NextAboveWatermark, everywhere("next is above the watermark"));
}

#[test]
fn control_a_position_that_repeats_inside_a_batch() {
    check(RepeatsARecord, everywhere("does not rise"));
}

#[test]
fn control_an_entry_below_the_cursor_position() {
    check(
        ServesBelowTheCursor,
        [Fail("does not rise"), Fail("does not rise"), Pass, Pass],
    );
}

#[test]
fn control_an_entry_at_next() {
    check(RecordAtNext, everywhere("is not below next"));
}

#[test]
fn control_an_expiry_that_misses_a_segment_of_its_records() {
    check(
        ExpiryDropped,
        everywhere("expiry does not name exactly the segments of the records"),
    );
}

#[test]
fn control_an_expiry_that_names_a_segment_with_no_record() {
    check(
        ExpiryNamesAnotherSegment,
        everywhere("expiry does not name exactly the segments of the records"),
    );
}

#[test]
fn control_an_expiry_out_of_rising_order() {
    check(
        ExpiryOutOfOrder,
        everywhere("expiry does not name exactly the segments of the records"),
    );
}

#[test]
fn control_an_expiry_instant_not_after_its_records() {
    check(ExpiryAtZero, everywhere("is not after its records"));
}

#[test]
fn control_a_screen_frame_record_served() {
    check(ServesScreenFrame, everywhere("a screen_frame record was served"));
}

#[test]
fn control_an_empty_batch_whose_next_does_not_rise() {
    check(
        StallsWithAnEmptyBatch,
        everywhere("an empty Batch whose next neither rose nor stands at the watermark"),
    );
}

#[test]
fn control_an_open_reply_frame_over_the_server_limit() {
    check(OversizeOpenReply, everywhere("outside 1 to 262,144"));
}

#[test]
fn control_an_open_reply_with_a_record() {
    check(
        OpenHoldsARecord,
        everywhere("the reply to open_cursor holds records or expiry entries"),
    );
}

#[test]
fn control_an_open_reply_with_an_expiry_entry() {
    check(
        OpenHoldsExpiry,
        everywhere("the reply to open_cursor holds records or expiry entries"),
    );
}

#[test]
fn control_an_open_reply_whose_next_is_not_the_position_asked_for() {
    check(OpenNextElsewhere, everywhere("has next"));
}

#[test]
fn control_an_open_reply_of_another_store_than_the_one_asked_for() {
    let text = "names another store than the one asked for";
    check(
        OpenNamesAnotherStore,
        [Fail(text), Fail(text), Fail(text), Fail(text)],
    );
}

#[test]
fn control_an_error_of_another_cause_than_bad_position() {
    check(
        BadPositionGetsStoreError,
        [Fail("got store-error; want bad-position"), Pass, Pass, Pass],
    );
}

#[test]
fn control_a_store_changed_that_names_the_old_store() {
    check(
        StoreChangedNamesTheOldId,
        [Pass, Pass, Pass, Fail("store-changed names the old store id")],
    );
}

#[test]
fn control_an_error_of_another_cause_than_store_changed() {
    check(
        StoreChangedGetsStoreError,
        [
            Pass,
            Pass,
            Pass,
            Fail("an open_cursor with the old store id got store-error; want store-changed"),
        ],
    );
}

#[test]
fn control_an_error_of_another_cause_than_bad_request_for_each_refused_message() {
    let filters = [
        (Form::EmptyKinds, "an empty kinds got store-error; want bad-request"),
        (
            Form::ScreenFrameKind,
            "kinds with screen_frame got store-error; want bad-request",
        ),
        (
            Form::KindTwice,
            "kinds with a name twice got store-error; want bad-request",
        ),
        (
            Form::EmptyRange,
            "a range whose end equals its start got store-error; want bad-request",
        ),
    ];
    for (form, text) in filters {
        check(
            RefusalGetsStoreError(form),
            [Pass, Pass, Fail(text), Pass],
        );
    }
    check(
        RefusalGetsStoreError(Form::NullStoreAtAPosition),
        [
            Pass,
            Pass,
            Pass,
            Fail("a null store with a non-zero position got store-error; want bad-request"),
        ],
    );
    check(
        RefusalGetsStoreError(Form::ReadWithNoCursor),
        [
            Pass,
            Pass,
            Pass,
            Fail("read_batch after store-changed got store-error; want bad-request"),
        ],
    );
}

#[test]
fn control_replies_of_the_wrong_message() {
    check(ReadGetsStoreError, everywhere("read_batch got the error store-error"));
    check(
        OpenGetsClosed,
        everywhere("open_cursor got a reply other than batch or error"),
    );
    check(
        ReadGetsClosed,
        everywhere("read_batch got a reply other than batch or error"),
    );
    check(
        CloseGetsBadRequest,
        everywhere("close got a reply other than closed"),
    );
    check(SendsNoMessage, everywhere("a reply that is no message of C5.5"));
    check(
        EndsTheStream,
        everywhere("the server ended the stream before its reply"),
    );
}

#[test]
fn control_a_read_that_never_reaches_the_watermark() {
    check(
        NeverReachesTheWatermark,
        everywhere("did not reach the watermark in 1000 reads"),
    );
}
