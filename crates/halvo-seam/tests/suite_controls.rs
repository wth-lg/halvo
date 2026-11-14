use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, PipeReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;

use halvo_schema::{Instant, Position, StoreId};
use halvo_seam::conformance::{self, Subject, CASES};
use halvo_seam::fixture::{FixtureTransport, Replace};
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::{
    decode_client, Batch, Cause, ClientMessage, Entry, Expiry, KindFilter, OpenCursor, Refusal,
    ServerError, ServerMessage, ZERO,
};
use halvo_seam::record::{CaptureRecord, Kind, ServedKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    EmptyKinds,
    ScreenFrameKind,
    KindTwice,
    EmptyRange,
    NullStoreAtAPosition,
    ReadWithNoCursor,
    ReadAfterReplace,
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
    NoLookAhead,
    OpensInsideTheWatermarkSegment,
    IgnoresTheByteBound,
    RepeatsAtAStop,
    SkipsAtAStop,
    LoadCutsSetC(usize),
    DropsTheLastRecordOfSetC,
    KeepsServingAfterTheReplace,
    StoreChangedOnReadNamesTheOldId,
    StoreChangedOnReadGetsStoreError,
    ServesOldRecordsAfterTheReplace,
    RestartAddsASegment,
    RefusesPastASegmentEnd,
    RefusesFarPastASegmentEnd,
    FilteredOpenServesFromTheStart,
    OldStoreOpensAtZero,
    KindsOutOfOrderRefused,
    WatermarkLowAfterAShred,
    ReopenByIdGetsClosed,
    StoreChangedNamesAnAlias,
    LeavesOutTheFirstRecordAfterStoreChanged,
    EndsEachBatchAtASegmentEnd,
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
    set_c: bool,
    frames: Vec<Entry>,
    first: Entry,
    store: Option<StoreId>,
    renamed: bool,
    cursor: Option<Position>,
    open_w: Position,
    reads: usize,
    stop: Option<Entry>,
    changed_on_read: bool,
    old_files: Vec<PathBuf>,
    lines: Vec<u64>,
    shredded: bool,
    first_open_null: Option<bool>,
    after_close: bool,
    answer_closed: bool,
    alias_open: bool,
    changed: Option<StoreId>,
    skip_first: bool,
    last_open: Option<OpenCursor>,
}

impl Proxy {
    fn past_an_inner_end(&self, p: Position, beyond: u64) -> bool {
        p.seg >= 1
            && (p.seg as usize) < self.lines.len()
            && p.idx >= self.lines[p.seg as usize - 1] + beyond
    }
}

fn out_of_order(kinds: &KindFilter) -> bool {
    let rank = |k: &ServedKind| ServedKind::ALL.iter().position(|x| x == k);
    kinds.kinds().windows(2).any(|w| rank(&w[0]) > rank(&w[1]))
}

impl Proxy {
    fn opened(&mut self, open: &OpenCursor, reply: ServerMessage) -> ServerMessage {
        let mut b = match reply {
            ServerMessage::Batch(b) => b,
            ServerMessage::Error(e) => return self.refused(e.cause().clone(), open.store(), false),
            other => return other,
        };
        self.cursor = Some(open.position());
        self.store = Some(b.store.clone());
        self.open_w = b.watermark;
        self.reads = 0;
        self.stop = None;
        self.last_open = Some(open.clone());
        self.renamed =
            (self.fault == OpenNamesAnotherStore && open.store().is_some()) || self.alias_open;
        self.skip_first = self.fault == LeavesOutTheFirstRecordAfterStoreChanged
            && open.store().is_some()
            && open.store() == self.changed.as_ref();
        match self.fault {
            WatermarkFalls => b.watermark.seg += 1,
            WatermarkLowAfterAShred if self.shredded => b.watermark.seg -= 1,
            FilteredOpenServesFromTheStart => b.next = open.position(),
            OpenHoldsARecord => b.records.push(self.first.clone()),
            OpenHoldsExpiry => b.expiry.push(Expiry {
                seg: 1,
                at: Instant(0),
            }),
            OpenNextElsewhere => b.next.idx += 1,
            OpensInsideTheWatermarkSegment => b.next = open.position(),
            OpenGetsClosed => return ServerMessage::Closed,
            _ => {}
        }
        if self.renamed {
            b.store = other(&b.store);
        }
        ServerMessage::Batch(b)
    }

    fn refused(&mut self, cause: Cause, asked: Option<&StoreId>, on_read: bool) -> ServerMessage {
        let cause = match (self.fault, cause) {
            (BadPositionGetsStoreError, Cause::BadPosition) => Cause::StoreError,
            (StoreChangedNamesTheOldId, Cause::StoreChanged(id)) => {
                Cause::StoreChanged(asked.cloned().unwrap_or(id))
            }
            (StoreChangedNamesAnAlias, Cause::StoreChanged(id)) if !on_read => {
                Cause::StoreChanged(other(&id))
            }
            (StoreChangedGetsStoreError, Cause::StoreChanged(_)) if !on_read => Cause::StoreError,
            (StoreChangedOnReadNamesTheOldId, Cause::StoreChanged(id)) if on_read => {
                Cause::StoreChanged(self.store.clone().unwrap_or(id))
            }
            (StoreChangedOnReadGetsStoreError, Cause::StoreChanged(_)) if on_read => {
                Cause::StoreError
            }
            (RefusalGetsStoreError(Form::ReadWithNoCursor), Cause::BadRequest)
                if on_read && !self.changed_on_read =>
            {
                Cause::StoreError
            }
            (RefusalGetsStoreError(Form::ReadAfterReplace), Cause::BadRequest)
                if on_read && self.changed_on_read =>
            {
                Cause::StoreError
            }
            (_, cause) => cause,
        };
        if let Cause::StoreChanged(id) = &cause {
            self.changed_on_read = on_read;
            if !on_read {
                self.changed = Some(id.clone());
            }
        }
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
            ReopenByIdGetsClosed if first && self.answer_closed => return ServerMessage::Closed,
            _ => {}
        }
        let mut b = match t.handle(&ClientMessage::ReadBatch) {
            ServerMessage::Batch(b) => b,
            ServerMessage::Error(e) => return self.refused(e.cause().clone(), None, true),
            other => return other,
        };
        if self.fault == EndsEachBatchAtASegmentEnd {
            if let (Some(seg), Some(open)) = (b.records.first().map(|e| e.pos.seg), &self.last_open)
            {
                if b.records.iter().any(|e| e.pos.seg != seg) {
                    b.records.retain(|e| e.pos.seg == seg);
                    b.expiry.retain(|x| x.seg == seg);
                    b.next = Position {
                        seg: seg + 1,
                        idx: 0,
                    };
                    let again = OpenCursor::at(
                        b.store.clone(),
                        b.next,
                        open.kinds().cloned(),
                        open.range(),
                    )
                    .unwrap();
                    t.handle(&ClientMessage::Close);
                    t.handle(&ClientMessage::OpenCursor(again));
                }
            }
        }
        if self.skip_first && !b.records.is_empty() {
            b.records.remove(0);
            self.skip_first = false;
        }
        if self.fault == IgnoresTheByteBound && !b.records.is_empty() && b.next != b.watermark {
            if let ServerMessage::Batch(more) = t.handle(&ClientMessage::ReadBatch) {
                for x in more.expiry {
                    if !b.expiry.iter().any(|e| e.seg == x.seg) {
                        b.expiry.push(x);
                    }
                }
                b.records.extend(more.records);
                b.next = more.next;
                b.watermark = more.watermark;
            }
        }
        let stopped = self.stop.take();
        if let (Some(last), Some(first)) = (&stopped, b.records.first_mut()) {
            match self.fault {
                RepeatsAtAStop => first.record = last.record.clone(),
                SkipsAtAStop => {
                    b.records.remove(0);
                }
                _ => {}
            }
        }
        if b.next != b.watermark {
            self.stop = b.records.last().cloned();
        }
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
            NoLookAhead if reached => {
                let last = b.records.last().unwrap().pos;
                b.next = Position {
                    seg: last.seg,
                    idx: last.idx + 1,
                };
            }
            RepeatsARecord if !b.records.is_empty() => {
                let copy = b.records[0].clone();
                b.records.insert(1, copy);
            }
            ServesBelowTheCursor if self.set_a && !b.records.is_empty() && cursor.idx >= 1 => {
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
            DropsTheLastRecordOfSetC if self.set_c && reached => {
                b.records.pop();
            }
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
    set_c: bool,
    idle: Option<FixtureTransport>,
    serving: Option<JoinHandle<FixtureTransport>>,
    replace: Option<Replace>,
}

impl Control {
    fn new(fault: Fault) -> Control {
        Control {
            fault,
            files: Vec::new(),
            first_live: 1,
            set_a: false,
            set_c: false,
            idle: None,
            serving: None,
            replace: None,
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
        self.replace = Some(t.replace_handle());
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
        self.set_c = batch_files == conformance::SET_C;
        let cut_c = match self.fault {
            LoadCutsSetC(lines) if batch_files == conformance::SET_C => Some(lines),
            _ => None,
        };
        self.files = Vec::new();
        for (i, text) in batch_files.iter().enumerate() {
            let mut text = text.to_string();
            if let Some(lines) = cut_c {
                text = text.split_inclusive('\n').take(lines).collect();
            }
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
            set_c: self.set_c,
            frames: screen_frames(),
            first: first_record(),
            store: None,
            renamed: false,
            cursor: None,
            open_w: ZERO,
            reads: 0,
            stop: None,
            changed_on_read: false,
            old_files: self.files.clone(),
            lines: self
                .files
                .iter()
                .map(|f| {
                    let text = fs::read(f).unwrap_or_default();
                    text.iter().filter(|b| **b == b'\n').count() as u64
                })
                .collect(),
            shredded: self.first_live > 1,
            first_open_null: None,
            after_close: false,
            answer_closed: false,
            alias_open: false,
            changed: None,
            skip_first: false,
            last_open: None,
        };
        let (mut requests_in, requests_out) = io::pipe().map_err(|e| e.to_string())?;
        let (replies_in, mut replies_out) = io::pipe().map_err(|e| e.to_string())?;
        self.serving = Some(std::thread::spawn(move || {
            while let Ok(Some(payload)) = read_frame(&mut requests_in, CLIENT_FRAME_MAX) {
                let (request, reply) = match decode_client(&payload) {
                    Ok(ClientMessage::OpenCursor(open)) => {
                        if proxy.fault == ServesOldRecordsAfterTheReplace && proxy.changed_on_read {
                            t = FixtureTransport::open(&proxy.old_files, 1, None).unwrap();
                            proxy.changed_on_read = false;
                        }
                        let w = t.watermark();
                        let p = open.position();
                        let fault = proxy.fault;
                        let first_null =
                            *proxy.first_open_null.get_or_insert(open.store().is_none());
                        proxy.answer_closed = fault == ReopenByIdGetsClosed
                            && first_null
                            && proxy.after_close
                            && open.store().is_some()
                            && open.kinds().is_none()
                            && open.range().is_none();
                        proxy.alias_open = fault == StoreChangedNamesAnAlias
                            && open.store() == Some(&other(t.store()));
                        let sent = match open.store() {
                            Some(s)
                                if fault == OpensInsideTheWatermarkSegment
                                    && p.seg == w.seg
                                    && p.idx > 0 =>
                            {
                                OpenCursor::at(s.clone(), w, open.kinds().cloned(), open.range())
                                    .unwrap()
                            }
                            Some(s)
                                if fault == FilteredOpenServesFromTheStart
                                    && open.kinds().is_some()
                                    && p != ZERO =>
                            {
                                let start = Position { seg: 1, idx: 0 };
                                OpenCursor::at(
                                    s.clone(),
                                    start,
                                    open.kinds().cloned(),
                                    open.range(),
                                )
                                .unwrap()
                            }
                            Some(s)
                                if fault == OldStoreOpensAtZero && s != t.store() && p == ZERO =>
                            {
                                OpenCursor::fresh(open.kinds().cloned(), open.range())
                            }
                            Some(_) if proxy.alias_open => OpenCursor::at(
                                t.store().clone(),
                                p,
                                open.kinds().cloned(),
                                open.range(),
                            )
                            .unwrap(),
                            _ => unfiltered(&open, fault),
                        };
                        let refuse = match fault {
                            RefusesPastASegmentEnd => proxy.past_an_inner_end(p, 0),
                            RefusesFarPastASegmentEnd => proxy.past_an_inner_end(p, 1),
                            _ => false,
                        };
                        let reply = if refuse {
                            error(Cause::BadPosition)
                        } else if fault == KindsOutOfOrderRefused
                            && open.kinds().is_some_and(out_of_order)
                        {
                            error(Cause::BadRequest)
                        } else {
                            t.handle(&ClientMessage::OpenCursor(sent))
                        };
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
                proxy.after_close = matches!(request, ClientMessage::Close);
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
        if self.fault == RestartAddsASegment {
            self.files.push(file_of("")?);
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

    fn replace_live(&mut self) -> Result<(), String> {
        if self.fault != KeepsServingAfterTheReplace {
            self.replace.as_ref().ok_or("no store")?.replace();
        }
        Ok(())
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
        [
            Pass,
            Pass,
            Fail("the filter foreground_app: "),
            Fail("check filtered_stops: "),
        ],
    );
    check(
        IgnoreRange,
        [
            Pass,
            Pass,
            Fail("the filter the range: "),
            Fail("check filtered_stops: "),
        ],
    );
    check(
        LoadKeepsTheOldStore,
        [
            Pass,
            Pass,
            Pass,
            Fail("an open_cursor with the old store id got another reply than an error"),
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
        [
            Fail("next fell"),
            Fail("next fell"),
            Fail("the filter foreground_app from {seg 2, idx 10}: next fell"),
            Fail("check filtered_stops: "),
        ],
    );
}

#[test]
fn control_a_next_above_the_watermark() {
    check(
        NextAboveWatermark,
        everywhere("next is above the watermark"),
    );
}

#[test]
fn control_a_position_that_repeats_inside_a_batch() {
    check(RepeatsARecord, everywhere("does not rise"));
}

#[test]
fn control_an_entry_below_the_cursor_position() {
    check(
        ServesBelowTheCursor,
        [
            Fail("does not rise"),
            Fail("does not rise"),
            Fail("the filter foreground_app from {seg 2, idx 10}: the position {seg 2, idx 9} does not rise"),
            Pass,
        ],
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
    check(
        ServesScreenFrame,
        everywhere("a screen_frame record was served"),
    );
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
        [
            Pass,
            Pass,
            Pass,
            Fail("store-changed names the old store id"),
        ],
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
        (
            Form::EmptyKinds,
            "an empty kinds got store-error; want bad-request",
        ),
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
        check(RefusalGetsStoreError(form), [Pass, Pass, Fail(text), Pass]);
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
        RefusalGetsStoreError(Form::ReadAfterReplace),
        [
            Pass,
            Pass,
            Pass,
            Fail("a second read_batch after replace_live got store-error; want bad-request"),
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
    check(
        ReadGetsStoreError,
        everywhere("read_batch got the error store-error"),
    );
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
    check(
        SendsNoMessage,
        everywhere("a reply that is no message of C5.5"),
    );
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

#[test]
fn control_a_server_without_look_ahead() {
    check(
        NoLookAhead,
        everywhere(
            "the reply that serves the last entry of the reference read has next {seg 4, idx 12}",
        ),
    );
}

#[test]
fn control_a_server_that_opens_inside_the_watermark_segment() {
    check(
        OpensInsideTheWatermarkSegment,
        [
            Fail("an open_cursor at {seg 5, idx 1} got another reply than an error"),
            Pass,
            Pass,
            Pass,
        ],
    );
}

#[test]
fn control_a_server_that_ignores_the_byte_bound() {
    check(
        IgnoresTheByteBound,
        [
            Pass,
            Pass,
            Pass,
            Fail("the read of set C: a reply frame of "),
        ],
    );
}

#[test]
fn control_a_server_that_repeats_the_record_at_a_stop() {
    check(
        RepeatsAtAStop,
        [
            Pass,
            Pass,
            Pass,
            Fail("the read of set C served 420 entries, the first that differs at {seg 1, idx "),
        ],
    );
}

#[test]
fn control_a_server_that_skips_the_record_at_a_stop() {
    check(
        SkipsAtAStop,
        [
            Pass,
            Pass,
            Pass,
            Fail("at the stop of the read of set C at {seg 1, idx "),
        ],
    );
}

#[test]
fn control_a_set_c_too_short_to_stop_a_batch() {
    check(
        LoadCutsSetC(20),
        [
            Pass,
            Pass,
            Pass,
            Fail("the read of set C ended 0 Batches before the store ended; want at least 2"),
        ],
    );
}

#[test]
fn control_a_set_c_that_stops_one_batch_before_its_end() {
    check(
        LoadCutsSetC(264),
        [
            Pass,
            Pass,
            Pass,
            Fail("the read of set C ended 1 Batches before the store ended; want at least 2"),
        ],
    );
}

#[test]
fn a_server_that_ends_each_batch_at_a_segment_end_passes() {
    check(EndsEachBatchAtASegmentEnd, [Pass, Pass, Pass, Pass]);
}

#[test]
fn control_a_filtered_cursor_opened_past_the_zero_position_that_serves_from_the_start() {
    check(
        FilteredOpenServesFromTheStart,
        [
            Pass,
            Pass,
            Fail("the filter foreground_app from {seg 2, idx 10}: the position {seg 1, idx 0} does not rise"),
            Fail("check filtered_stops: "),
        ],
    );
}

#[test]
fn control_another_store_id_at_the_zero_position_that_opens_a_cursor() {
    check(
        OldStoreOpensAtZero,
        [
            Pass,
            Pass,
            Pass,
            Fail("an open_cursor with the old store id at the zero position got another reply than an error"),
        ],
    );
}

#[test]
fn control_a_kinds_list_out_of_the_order_of_c4_4_that_gets_bad_request() {
    check(
        KindsOutOfOrderRefused,
        [
            Pass,
            Pass,
            Fail("the filter window_title and foreground_app: open_cursor at {seg 0, idx 0} got the error bad-request"),
            Fail("check filtered_stops: the filter keystroke and window_title on the fourth text of set A and set C from {seg 0, idx 0}: open_cursor at {seg 0, idx 0} got the error bad-request"),
        ],
    );
}

#[test]
fn control_a_server_that_refuses_a_position_two_past_the_end_of_an_inner_segment() {
    check(
        RefusesFarPastASegmentEnd,
        [
            Fail("resume from start 6 at {seg 1, idx 13}: open_cursor at {seg 1, idx 13} got the error bad-position"),
            Pass,
            Pass,
            Pass,
        ],
    );
}

#[test]
fn control_an_open_reply_whose_watermark_falls_below_the_reference_after_a_shred() {
    check(
        WatermarkLowAfterAShred,
        [
            Pass,
            Fail("after shred_oldest, a watermark below that of the reference read"),
            Pass,
            Pass,
        ],
    );
}

#[test]
fn control_a_read_before_the_replace_that_gets_closed() {
    check(
        ReopenByIdGetsClosed,
        [
            Pass,
            Pass,
            Fail("check open_while_open: the read before an open_cursor while a cursor stands at the watermark: read_batch got a reply other than batch or error"),
            Fail("the read before replace_live got a reply other than batch or error"),
        ],
    );
}

#[test]
fn control_a_store_changed_whose_id_the_new_store_does_not_carry() {
    check(
        StoreChangedNamesAnAlias,
        [
            Pass,
            Pass,
            Pass,
            Fail("the new store's reply names another id than store-changed did"),
        ],
    );
}

#[test]
fn control_a_cursor_opened_with_the_new_id_that_serves_other_entries() {
    check(
        LeavesOutTheFirstRecordAfterStoreChanged,
        [
            Pass,
            Pass,
            Pass,
            Fail("the new store served other entries with its new id"),
        ],
    );
}

#[test]
fn control_a_server_that_goes_on_serving_after_the_replace() {
    check(
        KeepsServingAfterTheReplace,
        [
            Pass,
            Pass,
            Pass,
            Fail("read_batch after replace_live got another reply than an error"),
        ],
    );
}

#[test]
fn control_a_store_changed_after_the_replace_that_names_the_old_store() {
    check(
        StoreChangedOnReadNamesTheOldId,
        [
            Pass,
            Pass,
            Pass,
            Fail("store-changed after replace_live names the old store id"),
        ],
    );
}

#[test]
fn control_an_error_of_another_cause_than_store_changed_after_the_replace() {
    check(
        StoreChangedOnReadGetsStoreError,
        [
            Pass,
            Pass,
            Pass,
            Fail("read_batch after replace_live got store-error; want store-changed"),
        ],
    );
}

#[test]
fn control_a_server_that_serves_the_old_records_after_the_replace() {
    check(
        ServesOldRecordsAfterTheReplace,
        [
            Pass,
            Pass,
            Pass,
            Fail("the store after replace_live served 44 entries; want none"),
        ],
    );
}

#[test]
fn control_a_server_that_drops_the_last_record_of_set_c() {
    check(
        DropsTheLastRecordOfSetC,
        [
            Pass,
            Pass,
            Pass,
            Fail("the read of set C served 419 entries, the first that differs at its end"),
        ],
    );
}

#[test]
fn a_server_whose_restart_adds_a_segment_passes() {
    check(RestartAddsASegment, [Pass, Pass, Pass, Pass]);
}

#[test]
fn control_a_server_that_refuses_a_position_past_the_end_of_an_inner_segment() {
    check(
        RefusesPastASegmentEnd,
        [
            Fail("open_cursor at {seg 1, idx 12} got the error bad-position"),
            Pass,
            Pass,
            Pass,
        ],
    );
}
