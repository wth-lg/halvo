use std::fs;
use std::io::{self, PipeReader, PipeWriter};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use halvo_schema::{Instant, Position, StoreId};
use halvo_seam::conformance::{self, Subject};
use halvo_seam::fixture::{FixtureTransport, Replace};
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use halvo_seam::message::{
    decode_client, decode_server, Cause, ClientMessage, Entry, Expiry, OpenCursor, Refusal,
    ServerError, ServerMessage, TimeRange, ZERO,
};
use halvo_seam::record::{CaptureRecord, Kind, ServedKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    StoreChangedOnReadNamesAnotherId,
    StoreChangedAtZeroNamesAnotherId,
    StoreChangedToAFilteredOpenNamesAnotherId,
    StoreChangedAtANonZeroPositionNamesAnotherId,
    StoreChangedAtZeroEchoesTheOldId,
    OpenWhileOpenServes,
    OpenWhileOpenClosesTheCursor,
    OpenWhileOpenGetsBadPosition,
    OpenAtTheWatermarkGetsBadPosition,
    OpenAtTheWatermarkRewindsTheCursor,
    OpenWithSTakesItsRange,
    OpenWithSAndKindsTakesItsRange,
    OpenMidReadMovesTheCursorOnByOne,
    OpenWhileOpenMovesTheCursor,
    OpenWhileOpenTakesItsRange,
    OpenWithAnotherIdWhileOpenClosesIt,
    OpenAtTheWatermarkReplacesTheCursor,
    OpenWhileOpenServesAfterARestart,
    NullStoreOpenWhileOpenServes,
    OpenWhileOpenServedOnANullStoreConnection,
    OpenMidReadReplacesTheCursor,
    OpenPastTheWatermarkWhileOpenGetsBadPosition,
    OpenWhileOpenServedOnAFurtherConnection,
    WatermarkRefusedAfterAShred,
    RisenWatermarkRefusedAfterAShred,
    PhantomRecordAtTheWatermarkAfterAShred,
    PhantomRecordAfterEveryShred,
    WatermarkRefusedAfterOneShred,
    WatermarkFallsWhenEverySegmentIsShredded,
    ExpiryMovesFromReadToRead,
    ExpiryMovesFromConnectionToConnection,
    ExpiryMovesFromCursorToCursor,
    ExpiryMovesOnTheSecondCursorOfSetA,
    ExpiryMovesOnAFurtherConnectionToOneTransport,
    NothingOnAFurtherConnection,
    FilteredStopRepeats,
    FilteredStopRepeatsInTheNextBatch,
    FilteredStopSkipsARecord,
    NextInTheFirstSegmentAtAStop,
    DropsTheFirstRecordOfTheSecondFile,
    FirstRecordDroppedUnder(Form),
    FirstRecordDroppedFromZero,
    OutsideRecordInPlaceUnderARange,
    LowWatermarkOnTheOpenAfterEveryShred,
    RisenWatermarkRefusesWAfterOneShred,
    OpenWithKindsWhileOpenServes,
    PhantomAfterTheRefusedOpenAtTheWatermark,
    RangedReadEndsAtTheFirstAtFromItsEnd,
    RangedReadEndsAtTheFirstAtPastItsEnd,
    RangedReadEndsWhenABatchStartsFromItsEnd,
    RangedReadDropsAnAtBelowOneServed,
    LoadRoundsTheStepBackAway,
    LoadDropsTheLastLineOfD,
    LoadChangesTheKeystrokeOfD,
}

fn loaded(fault: Option<Fault>, text: &str) -> String {
    let d = text == conformance::SET_D[0];
    match fault {
        Some(LoadRoundsTheStepBackAway) => text
            .replace(
                r#""at":1768294980900000000,"seq":3,"#,
                r#""at":1768294981000000000,"seq":3,"#,
            )
            .replace(
                r#""at":1768294980000000000,"seq":4,"#,
                r#""at":1768294981000000000,"seq":4,"#,
            ),
        Some(LoadDropsTheLastLineOfD) if d => {
            let cut = text[..text.len() - 1].rfind('\n').map_or(0, |p| p + 1);
            text[..cut].to_owned()
        }
        Some(LoadChangesTheKeystrokeOfD) if d => text.replace(
            r#""seq":4,"kind":"keystroke","app":"com.example.mail","p":{"class":"enter"}"#,
            r#""seq":4,"kind":"keystroke","app":"com.example.mail","p":{"class":"tab"}"#,
        ),
        _ => text.to_owned(),
    }
}

use Fault::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    WindowTitle,
    Keystroke,
    Range,
    RangeWindowTitle,
    TwoNames,
}

fn form_of(open: &OpenCursor) -> Option<Form> {
    let names: Vec<ServedKind> = open.kinds().map(|k| k.kinds().to_vec()).unwrap_or_default();
    match (names.as_slice(), open.range().is_some()) {
        ([ServedKind::WindowTitle], false) => Some(Form::WindowTitle),
        ([ServedKind::Keystroke], false) => Some(Form::Keystroke),
        ([], true) => Some(Form::Range),
        ([ServedKind::WindowTitle], true) => Some(Form::RangeWindowTitle),
        ([_, _, ..], false) => Some(Form::TwoNames),
        _ => None,
    }
}

struct Fixture {
    test: &'static str,
    fault: Option<Fault>,
    files: Vec<PathBuf>,
    first_live: u64,
    idle: Option<FixtureTransport>,
    serving: Option<JoinHandle<FixtureTransport>>,
    replace: Option<Replace>,
    connections: i64,
    since_open: i64,
    set_a: bool,
    restarted: bool,
}

impl Fixture {
    fn new(test: &'static str) -> Fixture {
        Fixture {
            test,
            fault: None,
            files: Vec::new(),
            first_live: 1,
            idle: None,
            serving: None,
            replace: None,
            connections: 0,
            since_open: 0,
            set_a: false,
            restarted: false,
        }
    }

    fn wrong(test: &'static str, fault: Fault) -> Fixture {
        Fixture {
            fault: Some(fault),
            ..Fixture::new(test)
        }
    }

    fn settle(&mut self) -> Result<(), String> {
        if let Some(handle) = self.serving.take() {
            let transport = handle
                .join()
                .map_err(|_| "the server thread panicked".to_owned())?;
            self.idle = Some(transport);
        }
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), String> {
        let transport = FixtureTransport::open(&self.files, self.first_live, None)
            .map_err(|e| format!("open failed: {e:?}"))?;
        self.replace = Some(transport.replace_handle());
        self.idle = Some(transport);
        self.since_open = 0;
        Ok(())
    }
}

impl Subject for Fixture {
    type Writer = PipeWriter;
    type Reader = PipeReader;

    fn load(&mut self, batch_files: &[&str]) -> Result<(), String> {
        self.settle()?;
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let mut n = 0;
        let dir = loop {
            let dir = base.join(format!("halvo-seam-{}-{n}", self.test));
            if fs::create_dir(&dir).is_ok() {
                break dir;
            }
            n += 1;
        };
        self.files = Vec::new();
        self.set_a = batch_files == conformance::SET_A;
        self.restarted = false;
        for (i, text) in batch_files.iter().enumerate() {
            let path = dir.join(format!("day-{:03}.jsonl", i + 1));
            fs::write(&path, loaded(self.fault, text)).map_err(|e| format!("write failed: {e}"))?;
            self.files.push(path);
        }
        self.first_live = 1;
        self.reopen()
    }

    fn connect(&mut self) -> Result<(PipeWriter, PipeReader), String> {
        self.settle()?;
        let mut transport = self.idle.take().ok_or("no store is loaded")?;
        let (requests_in, requests_out) = io::pipe().map_err(|e| e.to_string())?;
        let (replies_in, replies_out) = io::pipe().map_err(|e| e.to_string())?;
        let fault = self.fault;
        let shredded = self.first_live > 1;
        let all_shredded = self.first_live > self.files.len() as u64;
        let one_shred = self.first_live == 2;
        let two_files = self.files.len() == 2;
        let set_a = self.set_a;
        let restarted = self.restarted;
        self.connections += 1;
        self.since_open += 1;
        let further = self.since_open > 1;
        let connection = self.connections;
        self.serving = Some(std::thread::spawn(move || {
            match fault {
                None => {
                    let _ = transport.serve(requests_in, replies_out);
                }
                Some(fault) => {
                    let mut wrong = Wrong {
                        fault,
                        shredded,
                        all_shredded,
                        one_shred,
                        two_files,
                        set_a,
                        restarted,
                        at_watermark: false,
                        position: ZERO,
                        first_null: None,
                        cursor_reads: 0,
                        phantom_next: false,
                        cut: None,
                        connection,
                        further,
                        reads: 0,
                        opens: 0,
                        last: None,
                    };
                    wrong.serve(&mut transport, requests_in, replies_out);
                }
            }
            transport
        }));
        Ok((requests_out, replies_in))
    }

    fn restart(&mut self) -> Result<(), String> {
        self.settle()?;
        self.restarted = true;
        self.reopen()
    }

    fn shred_oldest(&mut self) -> Result<(), String> {
        self.settle()?;
        self.first_live += 1;
        if matches!(
            self.fault,
            Some(RisenWatermarkRefusedAfterAShred | RisenWatermarkRefusesWAfterOneShred)
        ) {
            let first = self.files.first().ok_or("no store is loaded")?;
            let path = first.with_file_name(format!("day-{:03}.jsonl", self.files.len() + 1));
            fs::write(&path, "").map_err(|e| format!("write failed: {e}"))?;
            self.files.push(path);
        }
        self.reopen()
    }

    fn replace_live(&mut self) -> Result<(), String> {
        self.replace.as_ref().ok_or("no store is loaded")?.replace();
        self.files = Vec::new();
        self.set_a = false;
        self.first_live = 1;
        Ok(())
    }
}

fn other(store: &StoreId) -> StoreId {
    let s = store.as_str();
    let first = if s.starts_with('0') { "1" } else { "0" };
    StoreId::parse(&format!("{first}{}", &s[1..])).unwrap()
}

fn error(cause: Cause) -> ServerMessage {
    ServerMessage::Error(ServerError::new(cause, "wrong server").unwrap())
}

struct Wrong {
    fault: Fault,
    shredded: bool,
    all_shredded: bool,
    one_shred: bool,
    two_files: bool,
    set_a: bool,
    restarted: bool,
    at_watermark: bool,
    position: Position,
    connection: i64,
    further: bool,
    reads: i64,
    opens: i64,
    last: Option<OpenCursor>,
    first_null: Option<bool>,
    cursor_reads: i64,
    phantom_next: bool,
    cut: Option<TimeRange>,
}

fn cuts(fault: Fault) -> bool {
    matches!(
        fault,
        RangedReadEndsAtTheFirstAtFromItsEnd
            | RangedReadEndsAtTheFirstAtPastItsEnd
            | RangedReadEndsWhenABatchStartsFromItsEnd
            | RangedReadDropsAnAtBelowOneServed
    )
}

fn without_range(open: &OpenCursor) -> Option<OpenCursor> {
    open.range()?;
    match open.store() {
        Some(store) => OpenCursor::at(store.clone(), open.position(), open.kinds().cloned(), None),
        None if open.position() == ZERO => Some(OpenCursor::fresh(open.kinds().cloned(), None)),
        None => None,
    }
}

impl Wrong {
    fn open(&mut self, t: &mut FixtureTransport, asked: &OpenCursor) -> ServerMessage {
        let unranged = match self.fault {
            fault if cuts(fault) => without_range(asked),
            _ => None,
        };
        let open = unranged.as_ref().unwrap_or(asked);
        let first_null = *self.first_null.get_or_insert(open.store().is_none());
        let refuses = matches!(
            self.fault,
            WatermarkRefusedAfterAShred | RisenWatermarkRefusedAfterAShred
        ) || (self.fault == WatermarkRefusedAfterOneShred && self.one_shred);
        let fallen = self.fault == WatermarkFallsWhenEverySegmentIsShredded
            && self.all_shredded
            && open.position() > Position { seg: 1, idx: 0 };
        let below_risen = Position {
            seg: t.watermark().seg.saturating_sub(1),
            idx: 0,
        };
        let refuses_w = self.fault == RisenWatermarkRefusesWAfterOneShred
            && self.one_shred
            && open.position() == below_risen;
        if fallen || refuses_w || (refuses && self.shredded && open.position() == t.watermark()) {
            return error(Cause::BadPosition);
        }
        let mut reply = open_reply(t, self.fault, open);
        if self.fault == LowWatermarkOnTheOpenAfterEveryShred
            && self.all_shredded
            && open.position() == t.watermark()
        {
            if let ServerMessage::Batch(b) = &mut reply {
                b.watermark.seg -= 1;
            }
        }
        let refused = matches!(&reply, ServerMessage::Error(e) if *e.cause() == Cause::BadRequest);
        let names_s = open.store() == Some(t.store());
        let reply = match self.fault {
            OpenWhileOpenGetsBadPosition if refused && !self.at_watermark => {
                error(Cause::BadPosition)
            }
            OpenAtTheWatermarkGetsBadPosition if refused && self.at_watermark => {
                error(Cause::BadPosition)
            }
            OpenAtTheWatermarkRewindsTheCursor if refused && self.at_watermark => {
                if let Some(first) = self.last.clone() {
                    let back =
                        OpenCursor::at(t.store().clone(), ZERO, first.kinds().cloned(), None)
                            .unwrap();
                    t.handle(&ClientMessage::Close);
                    t.handle(&ClientMessage::OpenCursor(back));
                }
                reply
            }
            OpenWhileOpenTakesItsRange | OpenWithSTakesItsRange
                if refused && (self.fault == OpenWhileOpenTakesItsRange || names_s) =>
            {
                if let Some(first) = self.last.clone() {
                    let taken = OpenCursor::at(
                        t.store().clone(),
                        self.position,
                        first.kinds().cloned(),
                        open.range(),
                    )
                    .unwrap();
                    t.handle(&ClientMessage::Close);
                    t.handle(&ClientMessage::OpenCursor(taken));
                }
                reply
            }
            OpenWithSAndKindsTakesItsRange if refused && names_s && open.kinds().is_some() => {
                if let Some(first) = self.last.clone() {
                    let taken = OpenCursor::at(
                        t.store().clone(),
                        self.position,
                        first.kinds().cloned(),
                        open.range(),
                    )
                    .unwrap();
                    t.handle(&ClientMessage::Close);
                    t.handle(&ClientMessage::OpenCursor(taken));
                }
                reply
            }
            OpenWithAnotherIdWhileOpenClosesIt
                if refused && open.store().is_some_and(|s| s != t.store()) =>
            {
                t.handle(&ClientMessage::Close);
                error(Cause::StoreChanged(t.store().clone()))
            }
            OpenAtTheWatermarkReplacesTheCursor if refused && self.at_watermark => {
                t.handle(&ClientMessage::Close);
                t.handle(&ClientMessage::OpenCursor(open.clone()))
            }
            OpenWhileOpenServesAfterARestart if refused && self.restarted => {
                t.handle(&ClientMessage::Close);
                t.handle(&ClientMessage::OpenCursor(open.clone()))
            }
            OpenWithKindsWhileOpenServes if refused && open.kinds().is_some() => {
                t.handle(&ClientMessage::Close);
                t.handle(&ClientMessage::OpenCursor(open.clone()))
            }
            PhantomAfterTheRefusedOpenAtTheWatermark if refused && self.at_watermark => {
                self.phantom_next = true;
                reply
            }
            NullStoreOpenWhileOpenServes if refused && open.store().is_none() => {
                t.handle(&ClientMessage::Close);
                t.handle(&ClientMessage::OpenCursor(open.clone()))
            }
            OpenWhileOpenServedOnANullStoreConnection if refused && first_null => {
                t.handle(&ClientMessage::Close);
                t.handle(&ClientMessage::OpenCursor(open.clone()))
            }
            OpenMidReadMovesTheCursorOnByOne
                if refused && self.cursor_reads > 0 && !self.at_watermark =>
            {
                if let Some(first) = self.last.clone() {
                    let on = Position {
                        seg: self.position.seg,
                        idx: self.position.idx + 1,
                    };
                    let moved = OpenCursor::at(
                        t.store().clone(),
                        on,
                        first.kinds().cloned(),
                        first.range(),
                    )
                    .unwrap();
                    t.handle(&ClientMessage::Close);
                    t.handle(&ClientMessage::OpenCursor(moved));
                }
                reply
            }
            OpenMidReadReplacesTheCursor
                if refused && self.cursor_reads > 0 && !self.at_watermark =>
            {
                t.handle(&ClientMessage::Close);
                t.handle(&ClientMessage::OpenCursor(open.clone()))
            }
            OpenWhileOpenServedOnAFurtherConnection if refused && self.further => {
                t.handle(&ClientMessage::Close);
                t.handle(&ClientMessage::OpenCursor(open.clone()))
            }
            OpenPastTheWatermarkWhileOpenGetsBadPosition
                if refused && open.position() > t.watermark() =>
            {
                error(Cause::BadPosition)
            }
            _ => reply,
        };
        if matches!(reply, ServerMessage::Batch(_)) {
            self.cut = asked.range();
            self.last = Some(open.clone());
            self.opens += 1;
            self.at_watermark = false;
            self.position = open.position();
            self.cursor_reads = 0;
        }
        reply
    }

    fn read(&mut self, t: &mut FixtureTransport) -> ServerMessage {
        let mut b = match read_reply(t, self.fault) {
            ServerMessage::Batch(b) => b,
            reply => return reply,
        };
        self.reads += 1;
        self.cursor_reads += 1;
        if let (Some(range), true) = (self.cut, cuts(self.fault)) {
            let mut kept: Vec<Entry> = Vec::new();
            for (i, e) in std::mem::take(&mut b.records).into_iter().enumerate() {
                let at = e.record.at();
                let ends = match self.fault {
                    RangedReadEndsAtTheFirstAtFromItsEnd => at >= range.end(),
                    RangedReadEndsAtTheFirstAtPastItsEnd => at > range.end(),
                    RangedReadEndsWhenABatchStartsFromItsEnd => i == 0 && at >= range.end(),
                    _ => false,
                };
                if ends {
                    b.next = b.watermark;
                    break;
                }
                let below = self.fault == RangedReadDropsAnAtBelowOneServed
                    && kept.last().is_some_and(|k| at < k.record.at());
                if range.contains(at) && !below {
                    kept.push(e);
                }
            }
            b.records = kept;
            let segs: Vec<u64> = b.records.iter().map(|e| e.pos.seg).collect();
            b.expiry.retain(|x| segs.contains(&x.seg));
        }
        let filtered = self
            .last
            .as_ref()
            .is_some_and(|o| o.kinds().is_some() || o.range().is_some());
        let dropped = match self.fault {
            FirstRecordDroppedUnder(form) => self.last.as_ref().and_then(form_of) == Some(form),
            FirstRecordDroppedFromZero => {
                filtered && self.last.as_ref().is_some_and(|o| o.position() == ZERO)
            }
            _ => false,
        };
        let first_of_two_files = self.two_files && self.cursor_reads == 1 && !b.records.is_empty();
        if dropped && first_of_two_files {
            b.records.remove(0);
            let segs: Vec<u64> = b.records.iter().map(|e| e.pos.seg).collect();
            b.expiry.retain(|x| segs.contains(&x.seg));
        }
        let ranged = self.last.as_ref().is_some_and(|o| o.range().is_some());
        if self.fault == OutsideRecordInPlaceUnderARange && ranged && first_of_two_files {
            let line = conformance::SET_A[3].split_inclusive('\n').next().unwrap();
            b.records[0].record = CaptureRecord::read_line(line.as_bytes()).unwrap();
        }
        let shift = match self.fault {
            ExpiryMovesFromReadToRead => self.reads,
            ExpiryMovesFromConnectionToConnection => self.connection,
            ExpiryMovesFromCursorToCursor => self.opens,
            ExpiryMovesOnTheSecondCursorOfSetA if self.set_a && self.opens == 2 => 1,
            ExpiryMovesOnAFurtherConnectionToOneTransport if self.further => 1,
            _ => 0,
        };
        let w = t.watermark();
        let at_w = self.last.as_ref().is_some_and(|o| o.position() == w);
        let phantom = match self.fault {
            PhantomRecordAtTheWatermarkAfterAShred => self.shredded && !self.all_shredded,
            PhantomRecordAfterEveryShred => self.all_shredded,
            _ => false,
        };
        let after_refusal = self.fault == PhantomAfterTheRefusedOpenAtTheWatermark
            && std::mem::take(&mut self.phantom_next);
        if ((phantom && at_w) || after_refusal) && b.records.is_empty() {
            let line = conformance::SET_A[0].split_inclusive('\n').next().unwrap();
            let record = CaptureRecord::read_line(line.as_bytes()).unwrap();
            b.expiry = vec![Expiry {
                seg: w.seg,
                at: Instant(record.at().0 + 1_209_600_000_000_000),
            }];
            b.records = vec![Entry {
                pos: Position { seg: w.seg, idx: 0 },
                record,
            }];
            b.watermark = Position {
                seg: w.seg + 1,
                idx: 0,
            };
            b.next = b.watermark;
        }
        for x in &mut b.expiry {
            x.at = Instant(x.at.0.saturating_add(shift * 1_000_000_000));
        }
        if self.fault == NextInTheFirstSegmentAtAStop && b.next != b.watermark {
            if let (Some(first), Some(last)) = (b.records.first(), b.records.last()) {
                if first.pos.seg != last.pos.seg {
                    b.next = Position {
                        seg: first.pos.seg,
                        idx: first.pos.idx + b.records.len() as u64,
                    };
                }
            }
        }
        if self.fault == DropsTheFirstRecordOfTheSecondFile && self.two_files {
            b.records.retain(|e| e.pos != Position { seg: 2, idx: 0 });
            let segs: Vec<u64> = b.records.iter().map(|e| e.pos.seg).collect();
            b.expiry.retain(|x| segs.contains(&x.seg));
        }
        if self.fault == NothingOnAFurtherConnection && self.further {
            b.records.clear();
            b.expiry.clear();
            b.next = b.watermark;
        }
        self.at_watermark = b.next == b.watermark;
        self.position = b.next;
        let Some(open) = self
            .last
            .as_ref()
            .filter(|o| o.kinds().is_some() || o.range().is_some())
        else {
            return ServerMessage::Batch(b);
        };
        if let (true, Some(last)) = (b.next != b.watermark, b.records.last()) {
            let at = match self.fault {
                FilteredStopRepeats | FilteredStopRepeatsInTheNextBatch => Some(last.pos),
                FilteredStopSkipsARecord => Some(Position {
                    seg: b.next.seg,
                    idx: b.next.idx + 1,
                }),
                _ => None,
            };
            if let Some(at) = at {
                let again =
                    OpenCursor::at(b.store.clone(), at, open.kinds().cloned(), open.range())
                        .unwrap();
                t.handle(&ClientMessage::Close);
                t.handle(&ClientMessage::OpenCursor(again));
                if self.fault == FilteredStopRepeats {
                    b.next = at;
                }
            }
        }
        ServerMessage::Batch(b)
    }

    fn serve(&mut self, t: &mut FixtureTransport, mut input: PipeReader, mut output: PipeWriter) {
        while let Ok(Some(payload)) = read_frame(&mut input, CLIENT_FRAME_MAX) {
            let reply = match decode_client(&payload) {
                Ok(ClientMessage::OpenCursor(open)) => self.open(t, &open),
                Ok(ClientMessage::ReadBatch) => self.read(t),
                Ok(message) => t.handle(&message),
                Err(Refusal::BadRequest) => error(Cause::BadRequest),
                Err(Refusal::BadFrame) => break,
            };
            let Ok(bytes) = reply.to_payload() else {
                break;
            };
            if write_frame(&mut output, &bytes, SERVER_FRAME_MAX).is_err() {
                break;
            }
        }
    }
}

fn open_reply(t: &mut FixtureTransport, fault: Fault, open: &OpenCursor) -> ServerMessage {
    let reply = t.handle(&ClientMessage::OpenCursor(open.clone()));
    match (fault, reply) {
        (StoreChangedAtANonZeroPositionNamesAnotherId, ServerMessage::Error(e))
            if open.position() != ZERO && open.kinds().is_none() && open.range().is_none() =>
        {
            match e.cause() {
                Cause::StoreChanged(id) => error(Cause::StoreChanged(other(id))),
                _ => ServerMessage::Error(e),
            }
        }
        (StoreChangedAtZeroEchoesTheOldId, ServerMessage::Error(e)) if open.position() == ZERO => {
            match (e.cause(), open.store()) {
                (Cause::StoreChanged(_), Some(old)) => error(Cause::StoreChanged(old.clone())),
                _ => ServerMessage::Error(e),
            }
        }
        (StoreChangedToAFilteredOpenNamesAnotherId, ServerMessage::Error(e))
            if open.kinds().is_some() || open.range().is_some() =>
        {
            match e.cause() {
                Cause::StoreChanged(id) => error(Cause::StoreChanged(other(id))),
                _ => ServerMessage::Error(e),
            }
        }
        (StoreChangedAtZeroNamesAnotherId, ServerMessage::Error(e)) if open.position() == ZERO => {
            match e.cause() {
                Cause::StoreChanged(id) => error(Cause::StoreChanged(other(id))),
                _ => ServerMessage::Error(e),
            }
        }
        (OpenWhileOpenServes, ServerMessage::Error(e)) if *e.cause() == Cause::BadRequest => {
            t.handle(&ClientMessage::Close);
            t.handle(&ClientMessage::OpenCursor(open.clone()))
        }
        (OpenWhileOpenClosesTheCursor, ServerMessage::Error(e))
            if *e.cause() == Cause::BadRequest =>
        {
            t.handle(&ClientMessage::Close);
            ServerMessage::Error(e)
        }
        (OpenWhileOpenMovesTheCursor, ServerMessage::Error(e))
            if *e.cause() == Cause::BadRequest =>
        {
            t.handle(&ClientMessage::Close);
            t.handle(&ClientMessage::OpenCursor(open.clone()));
            ServerMessage::Error(e)
        }
        (_, reply) => reply,
    }
}

fn read_reply(t: &mut FixtureTransport, fault: Fault) -> ServerMessage {
    match (fault, t.handle(&ClientMessage::ReadBatch)) {
        (StoreChangedOnReadNamesAnotherId, ServerMessage::Error(e)) => match e.cause() {
            Cause::StoreChanged(id) => error(Cause::StoreChanged(other(id))),
            _ => ServerMessage::Error(e),
        },
        (_, reply) => reply,
    }
}

fn wrong(test: &'static str, servers: &[(Fault, [Option<&str>; 4])]) {
    let mut missed = Vec::new();
    for &(fault, want) in servers {
        let mut subject = Fixture::wrong(test, fault);
        let results = conformance::run(&mut subject);
        for ((case, got), want) in results.iter().zip(want) {
            match (want, got) {
                (None, Ok(())) => {}
                (Some(check), Err(e)) if e.contains(&format!("check {check}: ")) => {}
                _ => missed.push(format!(
                    "{fault:?}, case {case}: want {want:?}, got {got:?}"
                )),
            }
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

#[test]
fn conformance_resume() {
    let mut subject = Fixture::new("conformance_resume");
    conformance::resume(&mut subject).unwrap();
}

#[test]
fn conformance_shredded() {
    let mut subject = Fixture::new("conformance_shredded");
    conformance::shredded(&mut subject).unwrap();
}

#[test]
fn conformance_filters() {
    let mut subject = Fixture::new("conformance_filters");
    conformance::filters(&mut subject).unwrap();
}

#[test]
fn conformance_store_changed() {
    let mut subject = Fixture::new("conformance_store_changed");
    conformance::store_changed(&mut subject).unwrap();
}

#[test]
fn conformance_check_store_changed_names_the_current_id() {
    let check = Some("store_changed_names_the_current_id");
    wrong(
        "conformance_check_store_changed_names_the_current_id",
        &[
            (StoreChangedOnReadNamesAnotherId, [None, None, None, check]),
            (StoreChangedAtZeroNamesAnotherId, [None, None, None, check]),
            (
                StoreChangedToAFilteredOpenNamesAnotherId,
                [None, None, None, check],
            ),
            (
                StoreChangedAtANonZeroPositionNamesAnotherId,
                [None, None, None, check],
            ),
            (StoreChangedAtZeroEchoesTheOldId, [None, None, None, check]),
        ],
    );
}

#[test]
fn conformance_check_open_while_open() {
    let check = Some("open_while_open");
    wrong(
        "conformance_check_open_while_open",
        &[
            (OpenWhileOpenServes, [check, None, check, check]),
            (OpenWhileOpenClosesTheCursor, [check, None, check, check]),
            (OpenWhileOpenGetsBadPosition, [check, None, check, check]),
            (
                OpenAtTheWatermarkGetsBadPosition,
                [check, None, check, None],
            ),
            (
                OpenAtTheWatermarkRewindsTheCursor,
                [check, None, check, None],
            ),
            (OpenWithSTakesItsRange, [check, None, check, None]),
            (OpenWithSAndKindsTakesItsRange, [check, None, check, None]),
            (OpenWhileOpenMovesTheCursor, [check, None, check, check]),
            (OpenWhileOpenTakesItsRange, [check, None, check, None]),
            (
                OpenWithAnotherIdWhileOpenClosesIt,
                [check, None, check, None],
            ),
            (
                OpenAtTheWatermarkReplacesTheCursor,
                [check, None, check, None],
            ),
            (OpenWhileOpenServesAfterARestart, [check, None, None, None]),
            (NullStoreOpenWhileOpenServes, [check, None, check, None]),
            (
                OpenWhileOpenServedOnANullStoreConnection,
                [None, None, check, check],
            ),
            (OpenMidReadReplacesTheCursor, [None, None, None, check]),
            (OpenMidReadMovesTheCursorOnByOne, [None, None, None, check]),
            (OpenWithKindsWhileOpenServes, [check, None, check, None]),
            (
                PhantomAfterTheRefusedOpenAtTheWatermark,
                [check, None, check, None],
            ),
            (
                OpenPastTheWatermarkWhileOpenGetsBadPosition,
                [check, None, check, None],
            ),
            (
                OpenWhileOpenServedOnAFurtherConnection,
                [None, None, check, None],
            ),
        ],
    );
}

#[test]
fn conformance_check_open_at_the_watermark_after_a_shred() {
    let check = Some("open_at_the_watermark_after_a_shred");
    wrong(
        "conformance_check_open_at_the_watermark_after_a_shred",
        &[
            (WatermarkRefusedAfterAShred, [None, check, None, None]),
            (RisenWatermarkRefusedAfterAShred, [None, check, None, None]),
            (
                PhantomRecordAtTheWatermarkAfterAShred,
                [None, check, None, None],
            ),
            (PhantomRecordAfterEveryShred, [None, check, None, None]),
            (WatermarkRefusedAfterOneShred, [None, check, None, None]),
            (
                LowWatermarkOnTheOpenAfterEveryShred,
                [None, check, None, None],
            ),
            (
                RisenWatermarkRefusesWAfterOneShred,
                [None, check, None, None],
            ),
            (
                WatermarkFallsWhenEverySegmentIsShredded,
                [None, check, None, None],
            ),
        ],
    );
}

#[test]
fn conformance_check_one_expiry_instant() {
    let check = Some("one_expiry_instant");
    wrong(
        "conformance_check_one_expiry_instant",
        &[
            (ExpiryMovesFromReadToRead, [check, check, check, check]),
            (
                ExpiryMovesFromConnectionToConnection,
                [check, check, check, None],
            ),
            (ExpiryMovesFromCursorToCursor, [check, check, check, check]),
            (
                ExpiryMovesOnTheSecondCursorOfSetA,
                [check, check, check, check],
            ),
            (
                ExpiryMovesOnAFurtherConnectionToOneTransport,
                [None, None, check, None],
            ),
            (NothingOnAFurtherConnection, [None, None, check, None]),
        ],
    );
}

#[test]
fn conformance_check_filtered_stops() {
    let check = Some("filtered_stops");
    wrong(
        "conformance_check_filtered_stops",
        &[
            (FilteredStopRepeats, [None, None, None, check]),
            (FilteredStopRepeatsInTheNextBatch, [None, None, None, check]),
            (FilteredStopSkipsARecord, [None, None, None, check]),
            (NextInTheFirstSegmentAtAStop, [None, None, None, check]),
            (
                DropsTheFirstRecordOfTheSecondFile,
                [None, None, None, check],
            ),
            (
                FirstRecordDroppedUnder(Form::TwoNames),
                [None, None, None, check],
            ),
            (
                FirstRecordDroppedUnder(Form::WindowTitle),
                [None, None, None, check],
            ),
            (
                FirstRecordDroppedUnder(Form::Keystroke),
                [None, None, None, check],
            ),
            (
                FirstRecordDroppedUnder(Form::Range),
                [None, None, None, check],
            ),
            (
                FirstRecordDroppedUnder(Form::RangeWindowTitle),
                [None, None, None, check],
            ),
            (FirstRecordDroppedFromZero, [None, None, None, check]),
            (OutsideRecordInPlaceUnderARange, [None, None, None, check]),
        ],
    );
}

#[test]
fn conformance_filters_step_back() {
    let servers = [
        (
            RangedReadEndsAtTheFirstAtFromItsEnd,
            "the range of set D up to the pointer before its step back: 3 entries served, want 4",
        ),
        (
            RangedReadEndsAtTheFirstAtPastItsEnd,
            "the range of set D up to just after the keystroke of its step back: 3 entries served, want 4",
        ),
        (
            RangedReadEndsWhenABatchStartsFromItsEnd,
            "the range of set D up to the pointer before its step back from {seg 1, idx 3}: 0 entries served, want 1",
        ),
        (
            RangedReadDropsAnAtBelowOneServed,
            "the range of set D from the keystroke of its step back: 1 entries served, want 2",
        ),
        (
            LoadRoundsTheStepBackAway,
            "set D as served holds no keystroke whose at is below that of the pointer before it",
        ),
        (
            LoadDropsTheLastLineOfD,
            "the read of set D served 7 entries; want the 8 records of D in order",
        ),
        (
            LoadChangesTheKeystrokeOfD,
            "the read of set D served 8 entries; want the 8 records of D in order",
        ),
    ];
    let mut missed = Vec::new();
    for (fault, reason) in servers {
        let mut subject = Fixture::wrong("conformance_filters_step_back", fault);
        let results = conformance::run(&mut subject);
        for (case, got) in &results {
            let caught = match (*case, got) {
                ("filters", Err(e)) => e.ends_with(reason),
                (_, Ok(())) => *case != "filters",
                _ => false,
            };
            if !caught {
                missed.push(format!("{fault:?}, case {case}: got {got:?}"));
            }
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

#[test]
fn conformance_set_d_holds_its_step_back_when_served() {
    let lines: Vec<&str> = conformance::SET_D[0].split_inclusive('\n').collect();
    let records: Vec<CaptureRecord> = lines
        .iter()
        .map(|l| CaptureRecord::read_line(l.as_bytes()).unwrap())
        .collect();
    let ats: Vec<i64> = records.iter().map(|r| r.at().0).collect();
    assert_eq!(ats.len(), 8);
    assert!(lines.iter().all(|l| !l.contains("\"site\"")));
    let kinds = [
        Kind::ForegroundApp,
        Kind::WindowTitle,
        Kind::Keystroke,
        Kind::Pointer,
    ];
    assert!(records.iter().all(|r| kinds.contains(&r.kind())));
    assert!(ats.iter().all(|at| (at - ats[0]) < 3_600_000_000_000));
    assert!(lines[3].contains(r#""kind":"pointer""#) && lines[3].contains(r#""class":"scroll""#));
    assert!(lines[4].contains(r#""kind":"keystroke""#));
    assert_eq!(ats[4] % 1_000_000_000, 0);
    assert_eq!(ats[3] % 1_000_000, 0);
    assert_eq!(ats[3] - ats[4], 900_000_000);
    for i in 1..ats.len() {
        if i != 4 {
            assert!(ats[i] - ats[i - 1] >= 2_000_000_000, "step {i}");
        }
    }
    let mut subject = Fixture::new("conformance_set_d_holds_its_step_back_when_served");
    subject.load(&conformance::SET_D).unwrap();
    let (mut writer, mut reader) = subject.connect().unwrap();
    let mut served = Vec::new();
    let open = ClientMessage::OpenCursor(OpenCursor::fresh(None, None));
    let mut next = Vec::new();
    for message in [open, ClientMessage::ReadBatch] {
        write_frame(&mut writer, &message.to_payload(), CLIENT_FRAME_MAX).unwrap();
        let reply = read_frame(&mut reader, SERVER_FRAME_MAX).unwrap().unwrap();
        match decode_server(&reply).unwrap() {
            ServerMessage::Batch(b) => {
                served.extend(b.records.iter().map(|e| e.record.at().0));
                next.push(b.next == b.watermark);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(next, [false, true]);
    assert_eq!(served, ats);
}
