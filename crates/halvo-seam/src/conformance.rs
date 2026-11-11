use std::io::{Read, Write};

use halvo_schema::{Instant, Position, StoreId};

use crate::frame::{read_frame, write_frame, FrameError, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
use crate::message::{decode_server, Batch, Cause, Entry, ServerMessage, ZERO};
use crate::record::{CaptureRecord, Kind};

/// The server under test (C5.13 item 2). A server's own test implements it and calls `run` or one case.
pub trait Subject {
    /// The stream end that the suite writes frames to.
    type Writer: Write;
    /// The stream end that the suite reads frames from.
    type Reader: Read;

    /// Destroys the store that the subject holds, if any, makes a new store that holds the records of the
    /// given batch-file texts in their order and nothing else, and starts the server on it. It may give each
    /// record a new `id` and `seq` and move every `at` by one offset rounded to the resolution of C4.7; it
    /// keeps the order, the `kind`, the `app` and the `p` of every record.
    fn load(&mut self, batch_files: &[&str]) -> Result<(), String>;

    /// Opens a new connection: one stream to write frames to and one to read frames from. The suite closes
    /// both ends of a connection before it calls `connect`, `load`, `restart` or `shred_oldest` again.
    fn connect(&mut self) -> Result<(Self::Writer, Self::Reader), String>;

    /// Stops the server and starts it again on the same store.
    fn restart(&mut self) -> Result<(), String>;

    /// Destroys the live segment with the lowest ordinal, as expiry does.
    fn shred_oldest(&mut self) -> Result<(), String>;
}

/// The names of the four cases, in the order in which `run` runs them.
pub const CASES: [&str; 4] = ["resume", "shredded", "filters", "store_changed"];

/// The most `read_batch` messages of one read to the watermark.
pub const MAX_READS: usize = 1_000;

/// Set A of C5.13 item 3: 4 batch-file texts of 12 lines, each with 2 `foreground_app`, 3 `window_title`,
/// 3 `keystroke`, 3 `pointer` and 1 `screen_frame` record; text i lies in the hour from 09:00:00 UTC of
/// day i of four days in a row, line k of every text at the same clock time, 60 s apart.
pub const SET_A: [&str; 4] = [
    concat!(
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000000","at":1767603600000000000,"seq":0,"kind":"foreground_app","app":"com.example.mail","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000001","at":1767603660000000000,"seq":1,"kind":"window_title","app":"com.example.mail","p":{"text":"Inbox 11","cut":false,"why":"app_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000002","at":1767603720000000000,"seq":2,"kind":"keystroke","app":"com.example.mail","p":{"class":"printable"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000003","at":1767603780000000000,"seq":3,"kind":"pointer","app":"com.example.mail","p":{"class":"click","button":"primary"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000004","at":1767603840000000000,"seq":4,"kind":"keystroke","app":"com.example.mail","p":{"class":"enter"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000005","at":1767603900000000000,"seq":5,"kind":"window_title","app":"com.example.mail","p":{"text":"Draft reply 15","cut":false,"why":"title_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000006","at":1767603960000000000,"seq":6,"kind":"pointer","app":"com.example.mail","p":{"class":"scroll","count":16}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000007","at":1767604020000000000,"seq":7,"kind":"screen_frame","app":"com.example.mail","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000008","at":1767604080000000000,"seq":8,"kind":"foreground_app","app":"com.example.sheet","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000009","at":1767604140000000000,"seq":9,"kind":"window_title","app":"com.example.sheet","p":{"text":"Budget 19","cut":true,"why":"app_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000010","at":1767604200000000000,"seq":10,"kind":"keystroke","app":"com.example.sheet","p":{"class":"tab"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000001000011","at":1767604260000000000,"seq":11,"kind":"pointer","app":"com.example.sheet","p":{"class":"move","count":21}}"#,
        "\n",
    ),
    concat!(
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000000","at":1767690000000000000,"seq":0,"kind":"foreground_app","app":"com.example.mail","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000001","at":1767690060000000000,"seq":1,"kind":"window_title","app":"com.example.mail","p":{"text":"Inbox 21","cut":false,"why":"app_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000002","at":1767690120000000000,"seq":2,"kind":"keystroke","app":"com.example.mail","p":{"class":"printable"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000003","at":1767690180000000000,"seq":3,"kind":"pointer","app":"com.example.mail","p":{"class":"click","button":"primary"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000004","at":1767690240000000000,"seq":4,"kind":"keystroke","app":"com.example.mail","p":{"class":"enter"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000005","at":1767690300000000000,"seq":5,"kind":"window_title","app":"com.example.mail","p":{"text":"Draft reply 25","cut":false,"why":"title_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000006","at":1767690360000000000,"seq":6,"kind":"pointer","app":"com.example.mail","p":{"class":"scroll","count":26}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000007","at":1767690420000000000,"seq":7,"kind":"screen_frame","app":"com.example.mail","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000008","at":1767690480000000000,"seq":8,"kind":"foreground_app","app":"com.example.sheet","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000009","at":1767690540000000000,"seq":9,"kind":"window_title","app":"com.example.sheet","p":{"text":"Budget 29","cut":true,"why":"app_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000010","at":1767690600000000000,"seq":10,"kind":"keystroke","app":"com.example.sheet","p":{"class":"tab"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000002000011","at":1767690660000000000,"seq":11,"kind":"pointer","app":"com.example.sheet","p":{"class":"move","count":31}}"#,
        "\n",
    ),
    concat!(
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000000","at":1767776400000000000,"seq":0,"kind":"foreground_app","app":"com.example.mail","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000001","at":1767776460000000000,"seq":1,"kind":"window_title","app":"com.example.mail","p":{"text":"Inbox 31","cut":false,"why":"app_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000002","at":1767776520000000000,"seq":2,"kind":"keystroke","app":"com.example.mail","p":{"class":"printable"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000003","at":1767776580000000000,"seq":3,"kind":"pointer","app":"com.example.mail","p":{"class":"click","button":"primary"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000004","at":1767776640000000000,"seq":4,"kind":"keystroke","app":"com.example.mail","p":{"class":"enter"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000005","at":1767776700000000000,"seq":5,"kind":"window_title","app":"com.example.mail","p":{"text":"Draft reply 35","cut":false,"why":"title_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000006","at":1767776760000000000,"seq":6,"kind":"pointer","app":"com.example.mail","p":{"class":"scroll","count":36}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000007","at":1767776820000000000,"seq":7,"kind":"screen_frame","app":"com.example.mail","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000008","at":1767776880000000000,"seq":8,"kind":"foreground_app","app":"com.example.sheet","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000009","at":1767776940000000000,"seq":9,"kind":"window_title","app":"com.example.sheet","p":{"text":"Budget 39","cut":true,"why":"app_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000010","at":1767777000000000000,"seq":10,"kind":"keystroke","app":"com.example.sheet","p":{"class":"tab"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000003000011","at":1767777060000000000,"seq":11,"kind":"pointer","app":"com.example.sheet","p":{"class":"move","count":41}}"#,
        "\n",
    ),
    concat!(
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000000","at":1767862800000000000,"seq":0,"kind":"foreground_app","app":"com.example.mail","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000001","at":1767862860000000000,"seq":1,"kind":"window_title","app":"com.example.mail","p":{"text":"Inbox 41","cut":false,"why":"app_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000002","at":1767862920000000000,"seq":2,"kind":"keystroke","app":"com.example.mail","p":{"class":"printable"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000003","at":1767862980000000000,"seq":3,"kind":"pointer","app":"com.example.mail","p":{"class":"click","button":"primary"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000004","at":1767863040000000000,"seq":4,"kind":"keystroke","app":"com.example.mail","p":{"class":"enter"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000005","at":1767863100000000000,"seq":5,"kind":"window_title","app":"com.example.mail","p":{"text":"Draft reply 45","cut":false,"why":"title_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000006","at":1767863160000000000,"seq":6,"kind":"pointer","app":"com.example.mail","p":{"class":"scroll","count":46}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000007","at":1767863220000000000,"seq":7,"kind":"screen_frame","app":"com.example.mail","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000008","at":1767863280000000000,"seq":8,"kind":"foreground_app","app":"com.example.sheet","p":{}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000009","at":1767863340000000000,"seq":9,"kind":"window_title","app":"com.example.sheet","p":{"text":"Budget 49","cut":true,"why":"app_change"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000010","at":1767863400000000000,"seq":10,"kind":"keystroke","app":"com.example.sheet","p":{"class":"tab"}}"#,
        "\n",
        r#"{"v":1,"id":"0195f000-0000-7000-8000-000004000011","at":1767863460000000000,"seq":11,"kind":"pointer","app":"com.example.sheet","p":{"class":"move","count":51}}"#,
        "\n",
    ),
];

/// Set B of C5.13 item 3: 1 text of 6 lines, 1 `foreground_app`, 2 `window_title`, 2 `keystroke` and
/// 1 `pointer` record.
pub const SET_B: [&str; 1] = [concat!(
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000005000000","at":1768208400000000000,"seq":0,"kind":"foreground_app","app":"com.example.calendar","p":{}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000005000001","at":1768208460000000000,"seq":1,"kind":"window_title","app":"com.example.calendar","p":{"text":"Week view","cut":false,"why":"app_change"}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000005000002","at":1768208520000000000,"seq":2,"kind":"keystroke","app":"com.example.calendar","p":{"class":"navigation"}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000005000003","at":1768208580000000000,"seq":3,"kind":"window_title","app":"com.example.calendar","p":{"text":"Event details","cut":false,"why":"title_change"}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000005000004","at":1768208640000000000,"seq":4,"kind":"keystroke","app":"com.example.calendar","p":{"class":"shortcut"}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000005000005","at":1768208700000000000,"seq":5,"kind":"pointer","app":"com.example.calendar","p":{"class":"click","button":"secondary"}}"#,
    "\n",
)];

/// Runs the four cases in the order of `CASES` and returns, for each, its name and a pass or a failure text.
pub fn run<S: Subject>(subject: &mut S) -> Vec<(&'static str, Result<(), String>)> {
    vec![
        (CASES[0], resume(subject)),
        (CASES[1], shredded(subject)),
        (CASES[2], filters(subject)),
        (CASES[3], store_changed(subject)),
    ]
}

struct Conn<S: Subject> {
    writer: S::Writer,
    reader: S::Reader,
}

const READ_BATCH: &str = r#"{"msg":"read_batch"}"#;
const CLOSE: &str = r#"{"msg":"close"}"#;

fn connect<S: Subject>(subject: &mut S) -> Result<Conn<S>, String> {
    let (writer, reader) = subject.connect()?;
    Ok(Conn { writer, reader })
}

impl<S: Subject> Conn<S> {
    fn send(&mut self, payload: &str) -> Result<ServerMessage, String> {
        write_frame(&mut self.writer, payload.as_bytes(), CLIENT_FRAME_MAX)
            .map_err(|e| format!("a frame could not be written: {e:?}"))?;
        let reply = match read_frame(&mut self.reader, SERVER_FRAME_MAX) {
            Ok(Some(reply)) => reply,
            Ok(None) => return Err("the server ended the stream before its reply".to_owned()),
            Err(FrameError::Length(n)) => {
                return Err(format!(
                    "a reply frame of {n} payload bytes, outside 1 to 262,144"
                ))
            }
            Err(e) => return Err(format!("the reply could not be read: {e:?}")),
        };
        decode_server(&reply).map_err(|_| "a reply that is no message of C5.5".to_owned())
    }

    fn close(&mut self) -> Result<(), String> {
        match self.send(CLOSE)? {
            ServerMessage::Closed => Ok(()),
            _ => Err("close got a reply other than closed".to_owned()),
        }
    }

    fn error(&mut self, payload: &str) -> Result<Cause, String> {
        match self.send(payload)? {
            ServerMessage::Error(e) => Ok(e.cause().clone()),
            _ => Err("a message that is due an error got another reply".to_owned()),
        }
    }
}

fn show(p: Position) -> String {
    format!("{{seg {}, idx {}}}", p.seg, p.idx)
}

fn open_text(
    store: Option<&StoreId>,
    position: Position,
    kinds: Option<&[&str]>,
    range: Option<(Instant, Instant)>,
) -> String {
    let store = store.map_or("null".to_owned(), |s| format!("\"{}\"", s.as_str()));
    let kinds = kinds.map_or("null".to_owned(), |k| {
        let names: Vec<String> = k.iter().map(|n| format!("\"{n}\"")).collect();
        format!("[{}]", names.join(","))
    });
    let range = range.map_or("null".to_owned(), |(start, end)| {
        format!(r#"{{"start":{},"end":{}}}"#, start.0, end.0)
    });
    format!(
        r#"{{"msg":"open_cursor","store":{store},"position":{{"seg":{},"idx":{}}},"kinds":{kinds},"range":{range}}}"#,
        position.seg, position.idx
    )
}

type Span = Option<(Instant, Instant)>;
type Filter<'a> = (&'a str, Option<&'a [&'a str]>, Span);
type FilterCase<'a> = (&'a str, Option<&'a [&'a str]>, Span, Vec<Entry>);

struct Reading {
    store: StoreId,
    entries: Vec<Entry>,
    watermark: Position,
    low: Position,
    top: Position,
    first_full_next: Option<Position>,
    expiry_segs: Vec<u64>,
}

fn check(
    b: &Batch,
    store: &StoreId,
    cursor: Position,
    last: Option<Position>,
    watermark: Position,
) -> Result<(), String> {
    if &b.store != store {
        return Err("a Batch names another store than the reply to its open_cursor".to_owned());
    }
    if b.watermark < watermark {
        return Err("the watermark fell".to_owned());
    }
    if b.next < cursor {
        return Err("next fell".to_owned());
    }
    if b.next > b.watermark {
        return Err("next is above the watermark".to_owned());
    }
    let mut prev = last;
    for e in &b.records {
        if e.pos < cursor || prev.is_some_and(|p| e.pos <= p) {
            return Err(format!("the position {} does not rise", show(e.pos)));
        }
        if e.pos >= b.next {
            return Err(format!("the position {} is not below next", show(e.pos)));
        }
        if e.record.kind() == Kind::ScreenFrame {
            return Err("a screen_frame record was served".to_owned());
        }
        prev = Some(e.pos);
    }
    let mut segs: Vec<u64> = b.records.iter().map(|e| e.pos.seg).collect();
    segs.dedup();
    let named: Vec<u64> = b.expiry.iter().map(|e| e.seg).collect();
    if named != segs {
        return Err(
            "expiry does not name exactly the segments of the records, in rising order".to_owned(),
        );
    }
    for x in &b.expiry {
        if b.records
            .iter()
            .any(|e| e.pos.seg == x.seg && e.record.at() >= x.at)
        {
            return Err(format!(
                "the expiry instant of segment {} is not after its records",
                x.seg
            ));
        }
    }
    if b.records.is_empty() && b.next <= cursor && b.next != b.watermark {
        return Err(
            "an empty Batch whose next neither rose nor stands at the watermark".to_owned(),
        );
    }
    Ok(())
}

fn read<S: Subject>(
    conn: &mut Conn<S>,
    store: Option<&StoreId>,
    position: Position,
    kinds: Option<&[&str]>,
    range: Option<(Instant, Instant)>,
) -> Result<Reading, String> {
    let opened = match conn.send(&open_text(store, position, kinds, range))? {
        ServerMessage::Batch(b) => b,
        ServerMessage::Error(e) => {
            return Err(format!(
                "open_cursor at {} got the error {}",
                show(position),
                e.cause().name()
            ))
        }
        _ => return Err("open_cursor got a reply other than batch or error".to_owned()),
    };
    if !opened.records.is_empty() || !opened.expiry.is_empty() {
        return Err("the reply to open_cursor holds records or expiry entries".to_owned());
    }
    if opened.next != position {
        return Err(format!(
            "the reply to open_cursor at {} has next {}",
            show(position),
            show(opened.next)
        ));
    }
    if store.is_some_and(|s| s != &opened.store) {
        return Err(
            "the reply to open_cursor names another store than the one asked for".to_owned(),
        );
    }
    let mut reading = Reading {
        store: opened.store.clone(),
        entries: Vec::new(),
        watermark: opened.watermark,
        low: opened.watermark,
        top: opened.watermark,
        first_full_next: None,
        expiry_segs: Vec::new(),
    };
    let mut cursor = position;
    let mut last: Option<Position> = None;
    let mut watermark = opened.watermark;
    for _ in 0..MAX_READS {
        let b = match conn.send(READ_BATCH)? {
            ServerMessage::Batch(b) => b,
            ServerMessage::Error(e) => {
                return Err(format!("read_batch got the error {}", e.cause().name()))
            }
            _ => return Err("read_batch got a reply other than batch or error".to_owned()),
        };
        check(&b, &reading.store, cursor, last, watermark)?;
        if !b.records.is_empty() && reading.first_full_next.is_none() {
            reading.first_full_next = Some(b.next);
        }
        last = b.records.last().map(|e| e.pos).or(last);
        cursor = b.next;
        watermark = b.watermark;
        reading.low = reading.low.min(b.watermark);
        reading.top = reading.top.max(b.watermark);
        reading.expiry_segs.extend(b.expiry.iter().map(|e| e.seg));
        reading.entries.extend(b.records);
        if b.next == b.watermark {
            reading.watermark = b.watermark;
            conn.close()?;
            return Ok(reading);
        }
    }
    Err(format!(
        "the read from {} did not reach the watermark in {MAX_READS} reads",
        show(position)
    ))
}

fn records_of(texts: &[&str]) -> Result<Vec<CaptureRecord>, String> {
    let mut out = Vec::new();
    for text in texts {
        for line in text.as_bytes().split_inclusive(|b| *b == b'\n') {
            let record = CaptureRecord::read_line(line)
                .map_err(|e| format!("the suite's own data fails the reader: {e}"))?;
            if record.kind() != Kind::ScreenFrame {
                out.push(record);
            }
        }
    }
    Ok(out)
}

fn alike(a: &CaptureRecord, b: &CaptureRecord) -> bool {
    a.kind() == b.kind() && a.app() == b.app() && a.payload() == b.payload()
}

fn same(got: &[Entry], want: &[Entry]) -> bool {
    got.len() == want.len()
        && got
            .iter()
            .zip(want)
            .all(|(g, w)| g.pos == w.pos && alike(&g.record, &w.record))
}

struct Reference {
    r: Vec<Entry>,
    s: StoreId,
    w: Position,
    first_full_next: Position,
}

fn reference<S: Subject>(subject: &mut S) -> Result<(Conn<S>, Reference), String> {
    subject.load(&SET_A)?;
    let mut conn = connect(subject)?;
    let reading =
        read(&mut conn, None, ZERO, None, None).map_err(|e| format!("the reference read: {e}"))?;
    let data = records_of(&SET_A)?;
    let n = reading.entries.len();
    if n != 44
        || data.len() != 44
        || !reading
            .entries
            .iter()
            .zip(&data)
            .all(|(e, d)| alike(&e.record, d))
    {
        return Err(format!(
            "the reference read served {n} entries; want the 44 records of set A of a served kind, in order"
        ));
    }
    let mut segs: Vec<u64> = reading.entries.iter().map(|e| e.pos.seg).collect();
    segs.dedup();
    if segs.len() < 3 {
        return Err(format!(
            "the reference read served {} segments; want at least 3",
            segs.len()
        ));
    }
    let first_full_next = reading
        .first_full_next
        .ok_or("the reference read served no record")?;
    Ok((
        conn,
        Reference {
            r: reading.entries,
            s: reading.store,
            w: reading.watermark,
            first_full_next,
        },
    ))
}

/// The case `resume` (C5.13 item 6): after `restart`, a read from each of five start positions serves the
/// entries of the reference read at or after it, at the same positions, from the same store; a position
/// beyond every watermark gets `bad-position`, and a legal open after it works.
pub fn resume<S: Subject>(subject: &mut S) -> Result<(), String> {
    let (conn, rf) = reference(subject)?;
    drop(conn);
    subject.restart()?;
    let mut conn = connect(subject)?;
    let second =
        rf.r.iter()
            .find(|e| e.pos.seg != rf.r[0].pos.seg)
            .map(|e| e.pos)
            .ok_or("the reference read served one segment")?;
    let starts = [ZERO, rf.r[10].pos, second, rf.first_full_next, rf.w];
    let mut top = rf.w;
    for (n, start) in starts.into_iter().enumerate() {
        let reading = read(&mut conn, Some(&rf.s), start, None, None)
            .map_err(|e| format!("resume from start {n} at {}: {e}", show(start)))?;
        let want: Vec<Entry> = rf.r.iter().filter(|e| e.pos >= start).cloned().collect();
        if !same(&reading.entries, &want) {
            return Err(format!(
                "resume from start {n} at {}: {} entries served, want the {} entries of the reference read at or after it",
                show(start),
                reading.entries.len(),
                want.len()
            ));
        }
        if reading.low < rf.w {
            return Err(format!(
                "resume from start {n}: a watermark below that of the reference read"
            ));
        }
        top = top.max(reading.top);
    }
    let beyond = Position {
        seg: top.seg + 1,
        idx: 0,
    };
    let cause = conn.error(&open_text(Some(&rf.s), beyond, None, None))?;
    if cause != Cause::BadPosition {
        return Err(format!(
            "an open_cursor at {} got {}; want bad-position",
            show(beyond),
            cause.name()
        ));
    }
    read(&mut conn, Some(&rf.s), ZERO, None, None)
        .map_err(|e| format!("the open after bad-position: {e}"))?;
    Ok(())
}

/// The case `shredded` (C5.13 item 7): after `shred_oldest`, a read from the last position of the lowest
/// segment and one from the zero position serve the entries above that segment, at the same positions, and
/// no `expiry` names it.
pub fn shredded<S: Subject>(subject: &mut S) -> Result<(), String> {
    let (conn, rf) = reference(subject)?;
    drop(conn);
    let s = rf.r[0].pos.seg;
    let p =
        rf.r.iter()
            .rfind(|e| e.pos.seg == s)
            .map(|e| e.pos)
            .ok_or("the reference read is empty")?;
    subject.shred_oldest()?;
    let mut conn = connect(subject)?;
    let want: Vec<Entry> = rf.r.iter().filter(|e| e.pos.seg > s).cloned().collect();
    for start in [p, ZERO] {
        let reading = read(&mut conn, Some(&rf.s), start, None, None)
            .map_err(|e| format!("after shred_oldest, from {}: {e}", show(start)))?;
        if !same(&reading.entries, &want) {
            return Err(format!(
                "after shred_oldest, from {}: {} entries served, want the {} entries above segment {s}",
                show(start),
                reading.entries.len(),
                want.len()
            ));
        }
        if reading.expiry_segs.contains(&s) {
            return Err(format!("an expiry entry names the shredded segment {s}"));
        }
        if reading.low < rf.w {
            return Err(
                "after shred_oldest, a watermark below that of the reference read".to_owned(),
            );
        }
    }
    Ok(())
}

/// The case `filters` (C5.13 item 8): each kinds filter, the run-mark filter, a time range with and without a
/// kind, and a range after every record serve exactly the entries of the reference read that pass them, at
/// their positions; four illegal opens get `bad-request` and leave the connection usable.
pub fn filters<S: Subject>(subject: &mut S) -> Result<(), String> {
    let (mut conn, rf) = reference(subject)?;
    let pick = |kinds: &[Kind], range: Option<(Instant, Instant)>| -> Vec<Entry> {
        rf.r.iter()
            .filter(|e| kinds.is_empty() || kinds.contains(&e.record.kind()))
            .filter(|e| range.is_none_or(|(a, b)| e.record.at() >= a && e.record.at() < b))
            .cloned()
            .collect()
    };
    let a = rf.r[11].record.at();
    let b = rf.r[30].record.at();
    let after =
        rf.r.iter()
            .map(|e| e.record.at())
            .max()
            .unwrap_or(Instant(0));
    let late = (
        Instant(after.0.saturating_add(1)),
        Instant(after.0.saturating_add(2)),
    );
    let cases: [FilterCase; 9] = [
        (
            "foreground_app",
            Some(&["foreground_app"]),
            None,
            pick(&[Kind::ForegroundApp], None),
        ),
        (
            "window_title",
            Some(&["window_title"]),
            None,
            pick(&[Kind::WindowTitle], None),
        ),
        (
            "keystroke",
            Some(&["keystroke"]),
            None,
            pick(&[Kind::Keystroke], None),
        ),
        (
            "pointer",
            Some(&["pointer"]),
            None,
            pick(&[Kind::Pointer], None),
        ),
        (
            "foreground_app and window_title",
            Some(&["foreground_app", "window_title"]),
            None,
            pick(&[Kind::ForegroundApp, Kind::WindowTitle], None),
        ),
        ("run_mark", Some(&["run_mark"]), None, Vec::new()),
        ("the range", None, Some((a, b)), pick(&[], Some((a, b)))),
        (
            "the range with keystroke",
            Some(&["keystroke"]),
            Some((a, b)),
            pick(&[Kind::Keystroke], Some((a, b))),
        ),
        ("a range after every record", None, Some(late), Vec::new()),
    ];
    for (name, kinds, range, want) in cases {
        let reading = read(&mut conn, Some(&rf.s), ZERO, kinds, range)
            .map_err(|e| format!("the filter {name}: {e}"))?;
        if !same(&reading.entries, &want) {
            return Err(format!(
                "the filter {name}: {} entries served, want {}",
                reading.entries.len(),
                want.len()
            ));
        }
    }
    let illegal: [Filter; 4] = [
        ("an empty kinds", Some(&[]), None),
        ("kinds with screen_frame", Some(&["screen_frame"]), None),
        (
            "kinds with a name twice",
            Some(&["keystroke", "keystroke"]),
            None,
        ),
        ("a range whose end equals its start", None, Some((a, a))),
    ];
    for (name, kinds, range) in illegal {
        let cause = conn.error(&open_text(Some(&rf.s), ZERO, kinds, range))?;
        if cause != Cause::BadRequest {
            return Err(format!("{name} got {}; want bad-request", cause.name()));
        }
        let reading = read(&mut conn, Some(&rf.s), ZERO, None, None)
            .map_err(|e| format!("the open after {name}: {e}"))?;
        if !same(&reading.entries, &rf.r) {
            return Err(format!(
                "the read after {name} differs from the reference read"
            ));
        }
    }
    Ok(())
}

/// The case `store_changed` (C5.13 item 9): after `load(B)`, the old store id gets `store-changed` with
/// another id and opens no cursor, a null store with a non-zero position gets `bad-request`, and the new
/// store serves the records of set B from the zero position, with a null store and with its new id.
pub fn store_changed<S: Subject>(subject: &mut S) -> Result<(), String> {
    let (conn, rf) = reference(subject)?;
    drop(conn);
    let p = rf.r[10].pos;
    subject.load(&SET_B)?;
    let mut conn = connect(subject)?;
    let new = match conn.error(&open_text(Some(&rf.s), p, None, None))? {
        Cause::StoreChanged(id) if id != rf.s => id,
        Cause::StoreChanged(_) => return Err("store-changed names the old store id".to_owned()),
        other => {
            return Err(format!(
                "an open_cursor with the old store id got {}; want store-changed",
                other.name()
            ))
        }
    };
    let cause = conn.error(READ_BATCH)?;
    if cause != Cause::BadRequest {
        return Err(format!(
            "read_batch after store-changed got {}; want bad-request",
            cause.name()
        ));
    }
    let cause = conn.error(&open_text(None, p, None, None))?;
    if cause != Cause::BadRequest {
        return Err(format!(
            "a null store with a non-zero position got {}; want bad-request",
            cause.name()
        ));
    }
    let data = records_of(&SET_B)?;
    let first = read(&mut conn, None, ZERO, None, None)
        .map_err(|e| format!("the new store with a null store id: {e}"))?;
    if first.store != new {
        return Err("the new store's reply names another id than store-changed did".to_owned());
    }
    let matches = first.entries.len() == data.len()
        && first
            .entries
            .iter()
            .zip(&data)
            .all(|(e, d)| alike(&e.record, d));
    if data.len() != 6 || !matches {
        return Err(format!(
            "the new store served {} entries; want the 6 records of set B in order",
            first.entries.len()
        ));
    }
    let again = read(&mut conn, Some(&new), ZERO, None, None)
        .map_err(|e| format!("the new store with its new id: {e}"))?;
    if !same(&again.entries, &first.entries) {
        return Err("the new store served other entries with its new id".to_owned());
    }
    Ok(())
}
