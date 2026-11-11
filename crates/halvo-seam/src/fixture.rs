use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use halvo_schema::{Instant, Position, StoreId};
use sha2::{Digest, Sha256};

use crate::frame::{read_frame, write_frame, FrameError, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use crate::input_set::{InputSet, SetError};
use crate::message::{
    decode_client, Batch, Cause, ClientMessage, Entry, Expiry, KindFilter, Refusal, ServerError,
    ServerMessage, TimeRange,
};
use crate::record::{CaptureRecord, Line, Lines};

/// The most bytes of record lines, LF included, that one read takes from the batch files (C5.7 item 5).
pub const READ_MAX: usize = 262_144;

/// The observation window that the fixture transport adds to the first instant of a segment, lowered to a
/// whole second, to give its expiry instant (C5.12 item 5): 14 days in nanoseconds.
pub const WINDOW: i64 = 1_209_600_000_000_000;

const SECOND: i128 = 1_000_000_000;

/// Why the fixture transport did not open (C5.12 item 12). No protocol message goes with it.
#[derive(Debug)]
pub enum OpenError {
    /// `first_live` is 0.
    FirstLive,
    /// The batch file of the segment with this ordinal is missing or unreadable.
    File { segment: u64, kind: io::ErrorKind },
    /// The set reader refused the input-set file.
    InputSet(SetError),
}

/// Why a connection of the stream entry ended with an error: the stream ended inside a frame, a read or a
/// write of a stream end failed, or a reply could not be written within its bound. No message was sent for it.
#[derive(Debug, PartialEq, Eq)]
pub enum ServeError {
    End,
    Io(io::ErrorKind),
    Oversize,
}

/// A server of the seam protocol inside the reader's process, over an ordered list of batch files (C5.12).
/// The k-th file is the segment with the ordinal k; segments below `first_live` are shredded. It holds one
/// connection with at most one cursor, one Batch and one line in memory, and never a whole file. It writes
/// and removes no file.
pub struct FixtureTransport {
    files: Vec<PathBuf>,
    first_live: u64,
    set: Option<InputSet>,
    store: StoreId,
    cursor: Option<Cursor>,
}

struct Cursor {
    position: Position,
    kinds: Option<KindFilter>,
    range: Option<TimeRange>,
    scan: Option<Scan>,
}

#[derive(Clone, Copy)]
struct Pending {
    pos: Position,
    line: Line,
    expiry: Option<Instant>,
}

struct Scan {
    seg: u64,
    idx: u64,
    lines: Option<Lines<BufReader<File>>>,
    expiry: Option<Instant>,
    pending: Option<Pending>,
    buf: Vec<u8>,
}

enum Fault {
    Line(Position),
    Read(u64),
}

impl Fault {
    fn detail(&self) -> String {
        match self {
            Fault::Line(p) => format!("segment {} line {}", p.seg, p.idx),
            Fault::Read(k) => format!("segment {k} unreadable"),
        }
    }
}

fn expiry_instant(at: Instant) -> Instant {
    let floor = i128::from(at.0).div_euclid(SECOND) * SECOND;
    Instant(i64::try_from(floor + i128::from(WINDOW)).unwrap_or(i64::MAX))
}

fn watermark(n: u64) -> Position {
    Position { seg: n + 1, idx: 0 }
}

impl Scan {
    fn new(start: Position, first_live: u64) -> Scan {
        let (seg, idx) = if start.seg < first_live {
            (first_live, 0)
        } else {
            (start.seg, start.idx)
        };
        Scan {
            seg,
            idx,
            lines: None,
            expiry: None,
            pending: None,
            buf: Vec::new(),
        }
    }

    fn advance(&mut self) {
        self.seg += 1;
        self.idx = 0;
        self.lines = None;
        self.expiry = None;
    }

    fn open_segment(&mut self, files: &[PathBuf]) -> Result<(), Fault> {
        let seg = self.seg;
        let path = usize::try_from(seg - 1)
            .ok()
            .and_then(|i| files.get(i))
            .ok_or(Fault::Read(seg))?;
        let file = File::open(path).map_err(|_| Fault::Read(seg))?;
        let mut lines = Lines::new(BufReader::new(file));
        self.expiry = None;
        if self.idx > 0 {
            let first = lines
                .next_line(&mut self.buf)
                .map_err(|_| Fault::Read(seg))?;
            if let Some(line) = first {
                let bad = Fault::Line(Position { seg, idx: 0 });
                let record = match line {
                    Line::Long(_) => return Err(bad),
                    Line::Bytes => CaptureRecord::read_line(&self.buf).map_err(|_| bad)?,
                };
                self.expiry = Some(expiry_instant(record.at()));
                let mut dropped = 1;
                while dropped < self.idx && lines.skip_line().map_err(|_| Fault::Read(seg))? {
                    dropped += 1;
                }
            }
        }
        self.lines = Some(lines);
        Ok(())
    }

    fn peek(&mut self, files: &[PathBuf]) -> Result<Option<Pending>, Fault> {
        if let Some(p) = self.pending {
            return Ok(Some(p));
        }
        loop {
            if self.seg > files.len() as u64 {
                return Ok(None);
            }
            if self.lines.is_none() {
                self.open_segment(files)?;
            }
            let Some(lines) = self.lines.as_mut() else {
                return Err(Fault::Read(self.seg));
            };
            match lines.next_line(&mut self.buf) {
                Ok(Some(line)) => {
                    let pos = Position {
                        seg: self.seg,
                        idx: self.idx,
                    };
                    self.idx += 1;
                    let expiry = if pos.idx == 0 { None } else { self.expiry };
                    let pending = Pending { pos, line, expiry };
                    self.pending = Some(pending);
                    return Ok(Some(pending));
                }
                Ok(None) => self.advance(),
                Err(_) => return Err(Fault::Read(self.seg)),
            }
        }
    }

    fn note_expiry(&mut self, pos: Position, expiry: Instant) {
        if let Some(p) = self.pending.as_mut() {
            p.expiry = Some(expiry);
        }
        if self.seg == pos.seg {
            self.expiry = Some(expiry);
        }
    }

    fn after_pending(&mut self, files: &[PathBuf]) -> Result<Position, Fault> {
        let n = files.len() as u64;
        loop {
            if self.seg > n {
                return Ok(watermark(n));
            }
            if self.lines.is_none() {
                self.open_segment(files)?;
            }
            let Some(lines) = self.lines.as_mut() else {
                return Err(Fault::Read(self.seg));
            };
            if !lines.at_end().map_err(|_| Fault::Read(self.seg))? {
                return Ok(Position {
                    seg: self.seg,
                    idx: self.idx,
                });
            }
            self.advance();
        }
    }

    fn take(&mut self) {
        self.pending = None;
    }
}

struct Counter(usize);

impl Write for Counter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn digits(n: u64) -> usize {
    n.checked_ilog10().map_or(1, |d| d as usize + 1)
}

fn int_len(n: i64) -> usize {
    digits(n.unsigned_abs()) + usize::from(n < 0)
}

fn pos_len(p: Position) -> usize {
    r#"{"seg":"#.len() + digits(p.seg) + r#","idx":"#.len() + digits(p.idx) + 1
}

#[derive(Clone, Copy)]
struct Size {
    fixed: usize,
    records: usize,
    n_records: usize,
    expiry: usize,
    n_expiry: usize,
}

impl Size {
    fn new(w: Position) -> Size {
        let fixed = r#"{"msg":"batch","store":""#.len()
            + 32
            + r#"","watermark":"#.len()
            + pos_len(w)
            + r#","records":["#.len()
            + r#"],"expiry":["#.len()
            + r#"],"next":"#.len()
            + 1;
        Size {
            fixed,
            records: 0,
            n_records: 0,
            expiry: 0,
            n_expiry: 0,
        }
    }

    fn with(mut self, entry: Option<usize>, expiry: Option<usize>) -> Size {
        if let Some(n) = entry {
            self.records += n;
            self.n_records += 1;
        }
        if let Some(n) = expiry {
            self.expiry += n;
            self.n_expiry += 1;
        }
        self
    }

    fn total(&self, next: Position) -> usize {
        self.fixed
            + self.records
            + self.n_records.saturating_sub(1)
            + self.expiry
            + self.n_expiry.saturating_sub(1)
            + pos_len(next)
    }
}

fn entry_len(pos: Position, record: &CaptureRecord) -> usize {
    let mut counter = Counter(0);
    let _ = serde_json::to_writer(&mut counter, record);
    r#"{"pos":"#.len() + pos_len(pos) + r#","record":"#.len() + counter.0 + 1
}

fn expiry_len(seg: u64, at: Instant) -> usize {
    r#"{"seg":"#.len() + digits(seg) + r#","at":"#.len() + int_len(at.0) + 1
}

fn passes(record: &CaptureRecord, kinds: Option<&KindFilter>, range: Option<TimeRange>) -> bool {
    let kind = record.kind();
    kind.served().is_some()
        && kinds.is_none_or(|k| k.admits(kind))
        && range.is_none_or(|r| r.contains(record.at()))
}

fn error(cause: Cause, detail: &str) -> ServerMessage {
    ServerMessage::Error(ServerError::bounded(cause, detail))
}

impl FixtureTransport {
    /// Opens the transport on `files`, in order, with `first_live` (at least 1) and at most one input-set file.
    /// Computes the store id at open in one streaming read: the first 32 hex digits of the SHA-256 of the bytes
    /// of every file of the list, live or shredded, joined in list order.
    pub fn open<P: AsRef<Path>>(
        files: &[P],
        first_live: u64,
        input_set: Option<&Path>,
    ) -> Result<FixtureTransport, OpenError> {
        if first_live == 0 {
            return Err(OpenError::FirstLive);
        }
        let set = input_set
            .map(InputSet::read)
            .transpose()
            .map_err(OpenError::InputSet)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 65_536];
        for (i, path) in files.iter().enumerate() {
            let segment = i as u64 + 1;
            let fail = |e: io::Error| OpenError::File {
                segment,
                kind: e.kind(),
            };
            let mut file = File::open(path.as_ref()).map_err(fail)?;
            loop {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => hasher.update(&buf[..n]),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(fail(e)),
                }
            }
        }
        let hex: String = hasher.finalize()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let store = StoreId::parse(&hex).map_err(|_| OpenError::FirstLive)?;
        Ok(FixtureTransport {
            files: files.iter().map(|p| p.as_ref().to_owned()).collect(),
            first_live,
            set,
            store,
            cursor: None,
        })
    }

    /// The store id.
    pub fn store(&self) -> &StoreId {
        &self.store
    }

    /// The watermark `{"seg":N+1,"idx":0}` for a list of N files.
    pub fn watermark(&self) -> Position {
        watermark(self.files.len() as u64)
    }

    /// The in-process entry: answers one message of the client with the reply of C5.6, or with an error of
    /// C5.10. It builds the same Batches as the stream entry, counting the payload bytes that the frame of the
    /// Batch would have before the reduction.
    pub fn handle(&mut self, message: &ClientMessage) -> ServerMessage {
        let w = self.watermark();
        match message {
            ClientMessage::OpenCursor(open) => {
                if self.cursor.is_some() {
                    return error(Cause::BadRequest, "a cursor is open");
                }
                if open.store().is_some_and(|s| s != &self.store) {
                    return error(Cause::StoreChanged(self.store.clone()), "store id differs");
                }
                if open.position() > w {
                    return error(Cause::BadPosition, "position after the watermark");
                }
                self.cursor = Some(Cursor {
                    position: open.position(),
                    kinds: open.kinds().cloned(),
                    range: open.range(),
                    scan: None,
                });
                ServerMessage::Batch(Batch {
                    store: self.store.clone(),
                    watermark: w,
                    records: Vec::new(),
                    expiry: Vec::new(),
                    next: open.position(),
                })
            }
            ClientMessage::ReadBatch => {
                if self.cursor.is_none() {
                    return error(Cause::BadRequest, "no cursor is open");
                }
                match self.read() {
                    Ok(batch) => ServerMessage::Batch(batch),
                    Err(fault) => {
                        self.cursor = None;
                        error(Cause::StoreError, &fault.detail())
                    }
                }
            }
            ClientMessage::Close => {
                if self.cursor.take().is_none() {
                    return error(Cause::BadRequest, "no cursor is open");
                }
                ServerMessage::Closed
            }
            ClientMessage::AnalysisWrite(_) => error(
                Cause::BadRequest,
                "the fixture transport takes no analysis write",
            ),
        }
    }

    fn read(&mut self) -> Result<Batch, Fault> {
        let FixtureTransport {
            files,
            first_live,
            set,
            store,
            cursor,
        } = self;
        let Some(cursor) = cursor.as_mut() else {
            return Err(Fault::Read(0));
        };
        let w = watermark(files.len() as u64);
        let p = cursor.position;
        let mut records = Vec::new();
        let mut expiry: Vec<Expiry> = Vec::new();
        let next = if p >= w {
            w
        } else {
            let scan = cursor.scan.get_or_insert_with(|| Scan::new(p, *first_live));
            let mut size = Size::new(w);
            let mut lines_read = 0usize;
            loop {
                let Some(pending) = scan.peek(files)? else {
                    break w;
                };
                let len = match pending.line {
                    Line::Bytes => scan.buf.len(),
                    Line::Long(n) => n,
                };
                if lines_read > 0 && lines_read + len > READ_MAX {
                    break pending.pos;
                }
                let bad = Fault::Line(pending.pos);
                let record = match pending.line {
                    Line::Long(_) => return Err(bad),
                    Line::Bytes => CaptureRecord::read_line(&scan.buf).map_err(|_| bad)?,
                };
                let seg_expiry = match pending.expiry {
                    Some(e) => e,
                    None => {
                        let e = expiry_instant(record.at());
                        scan.note_expiry(pending.pos, e);
                        e
                    }
                };
                let served = passes(&record, cursor.kinds.as_ref(), cursor.range);
                let after = scan.after_pending(files)?;
                let new_seg = served && expiry.last().map(|e| e.seg) != Some(pending.pos.seg);
                let grown = size.with(
                    served.then(|| entry_len(pending.pos, &record)),
                    new_seg.then(|| expiry_len(pending.pos.seg, seg_expiry)),
                );
                if lines_read > 0 && grown.total(after) > SERVER_FRAME_MAX {
                    break pending.pos;
                }
                scan.take();
                lines_read += len;
                size = grown;
                if new_seg {
                    expiry.push(Expiry {
                        seg: pending.pos.seg,
                        at: seg_expiry,
                    });
                }
                if served {
                    records.push(Entry {
                        pos: pending.pos,
                        record,
                    });
                }
                if after == w {
                    break w;
                }
            }
        };
        cursor.position = next;
        if let Some(set) = set {
            records = records
                .into_iter()
                .filter_map(|e| {
                    set.reduce(&e.record)
                        .map(|record| Entry { pos: e.pos, record })
                })
                .collect();
        }
        Ok(Batch {
            store: store.clone(),
            watermark: w,
            records,
            expiry,
            next,
        })
    }

    /// The stream entry: serves one connection by C5.2 on the two stream ends that the caller gives, one
    /// blocking call that starts no thread. Returns at end-of-file before a frame, and after it has sent the
    /// `bad-frame` error of a frame or a payload that is no message; returns an error, and sends no message,
    /// when the stream ends inside a frame or a read or a write fails. The cursor of the connection ends with
    /// it, and the same transport serves a further connection with no cursor open.
    pub fn serve<R: Read, W: Write>(
        &mut self,
        mut input: R,
        mut output: W,
    ) -> Result<(), ServeError> {
        let result = self.serve_connection(&mut input, &mut output);
        self.cursor = None;
        result
    }

    fn serve_connection(
        &mut self,
        input: &mut dyn Read,
        output: &mut dyn Write,
    ) -> Result<(), ServeError> {
        loop {
            let reply = match read_frame(input, CLIENT_FRAME_MAX) {
                Ok(None) => return Ok(()),
                Ok(Some(payload)) => match decode_client(&payload) {
                    Ok(message) => self.handle(&message),
                    Err(Refusal::BadRequest) => error(Cause::BadRequest, "a value is refused"),
                    Err(Refusal::BadFrame) => {
                        send(
                            output,
                            &error(Cause::BadFrame, "no message of the protocol"),
                        )?;
                        return Ok(());
                    }
                },
                Err(FrameError::Length(_)) => {
                    send(output, &error(Cause::BadFrame, "frame length refused"))?;
                    return Ok(());
                }
                Err(FrameError::End) => return Err(ServeError::End),
                Err(FrameError::Io(e)) => return Err(ServeError::Io(e.kind())),
            };
            send(output, &reply)?;
        }
    }
}

fn send(output: &mut dyn Write, message: &ServerMessage) -> Result<(), ServeError> {
    let payload = message.to_payload().map_err(|_| ServeError::Oversize)?;
    write_frame(output, &payload, SERVER_FRAME_MAX).map_err(|e| match e {
        FrameError::Io(e) => ServeError::Io(e.kind()),
        _ => ServeError::Oversize,
    })
}
