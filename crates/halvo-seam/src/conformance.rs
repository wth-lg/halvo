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

    /// Destroys the store and starts an empty one while the server runs; every open connection stays open,
    /// and the next `read_batch` of a cursor that was open gets `store-changed`. For a daemon it is the
    /// delete-all request. It is the one call that the suite makes with a connection open.
    fn replace_live(&mut self) -> Result<(), String>;
}

/// The names of the four cases, in the order in which `run` runs them.
pub const CASES: [&str; 4] = ["resume", "shredded", "filters", "store_changed"];

/// The names of the checks of C5.13 item 12, in the order of its servers S1 to S5. The four cases make them
/// beside the checks of items 4 to 9, so `run` makes them too, and the failure text of each holds
/// `check <name>: `; where two names stand in one text, the last one names the check that failed.
/// `store_changed_names_the_current_id`, in the case `store_changed`: the `store-changed` of the read after
/// `replace_live` names the store that a null-store open serves next; after `load(B)`, three opens with the
/// old id, at a non-zero position, at the zero position, and at the non-zero position with the kinds filter
/// `keystroke` and a range, each get a `store-changed` that names the new store's id and not the old one.
/// `open_while_open`, in the cases `resume` after its restart and `filters` on the connection of the reference
/// read, whose first open had a null store, and on a second connection: with a cursor
/// open at the zero position, four `open_cursor` messages sent without the `close` that C5.6 asks for each get
/// `bad-request`: S at another position with the kinds filter `keystroke` and a range after every record,
/// another store id with that range, a null store at the zero position with that range, and S at a position
/// after the watermark with that range; the cursor then serves the whole reference read. A cursor read to the
/// watermark and not closed gets `bad-request` for a further open, and a `read_batch` after it serves nothing
/// and stands at the watermark. In `store_changed`, a cursor on the two-file store of `filtered_stops` that
/// has read one Batch, whose `next` is below the watermark, gets `bad-request` for a further open and then
/// serves the rest of that store.
/// `open_at_the_watermark_after_a_shred`, in the case `shredded`: after one `shred_oldest`, an `open_cursor`
/// with S at W, and at the highest watermark of the replies after the shred where that is above W, is no
/// error and serves no entry. Then `shred_oldest` is called once more for each segment that holds an entry
/// of the reference read, but for the first, so as many times in all as there are such segments, and an
/// `open_cursor` with S at W is no error, serves no entry, and neither its reply nor a Batch of its read names
/// a watermark below W.
/// `filtered_stops`, in the case `store_changed` after its read of set C: `load` of the fourth text of set A
/// and the text of set C, two batch files, whose read from the zero position must serve their records of a
/// served kind in order; then reads of that store under five filters (`window_title`, `keystroke` and
/// `window_title`, the range from the `at` of its 101st entry to that of its 401st, that range with
/// `window_title`, and `keystroke`), from the zero position and from its 151st entry, in which the 262,144
/// bytes of C5.7 item 5 stop Batches (a server that takes as many lines as the bound allows crosses from the
/// first file into the second before its first stop); each Batch holds every entry of that store that passes
/// the filter from the cursor's position up to its `next`, at its position, and no other, compared entry by
/// entry.
/// `one_expiry_instant`, in every case: each `expiry` entry for a segment of one store id holds the instant
/// that the first entry for that segment held in the case, in every reply to `read_batch`, on every cursor and
/// connection and after a restart, among them a second connection of the case `filters` to the store that its
/// first connection read, with no restart between; the check makes no claim across two cases.
pub const CHECKS: [&str; 5] = [
    "store_changed_names_the_current_id",
    "open_while_open",
    "open_at_the_watermark_after_a_shred",
    "filtered_stops",
    "one_expiry_instant",
];

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

/// Set C of C5.13 item 3: 1 batch-file text of 420 `window_title` lines of 1,445 bytes each with its LF, so
/// that the 262,144 bytes of C5.7 item 5 stop at least two Batches before its end. The script that writes sets
/// A and B writes it.
pub const SET_C: [&str; 1] = [include_str!("conformance/set_c.jsonl")];

/// Set D of C5.13 item 3: 1 batch-file text of 8 lines of a served kind inside the hour from 09:00:00 UTC of
/// 2026-01-13. Its 5th line, a `keystroke` whose `at` is a whole second, follows a `scroll` pointer whose `at`
/// lies 999 ms after it, so `at` steps back by under 1 s, as the resolutions of C4.7 allow; every other step
/// rises by at least 60 s.
pub const SET_D: [&str; 1] = [concat!(
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000006000000","at":1768294800000000000,"seq":0,"kind":"foreground_app","app":"com.example.mail","p":{}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000006000001","at":1768294860000000000,"seq":1,"kind":"window_title","app":"com.example.mail","p":{"text":"Inbox 61","cut":false,"why":"app_change"}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000006000002","at":1768294920000000000,"seq":2,"kind":"keystroke","app":"com.example.mail","p":{"class":"printable"}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000006000003","at":1768294980999000000,"seq":3,"kind":"pointer","app":"com.example.mail","p":{"class":"scroll","count":63}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000006000004","at":1768294980000000000,"seq":4,"kind":"keystroke","app":"com.example.mail","p":{"class":"enter"}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000006000005","at":1768295040000000000,"seq":5,"kind":"window_title","app":"com.example.mail","p":{"text":"Draft reply 65","cut":false,"why":"title_change"}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000006000006","at":1768295100000000000,"seq":6,"kind":"pointer","app":"com.example.mail","p":{"class":"move","count":66}}"#,
    "\n",
    r#"{"v":1,"id":"0195f000-0000-7000-8000-000006000007","at":1768295160000000000,"seq":7,"kind":"keystroke","app":"com.example.mail","p":{"class":"tab"}}"#,
    "\n",
)];

const STEP_BACK: usize = 4;
const STEP_BACK_NS: i64 = 999_000_000;

/// Runs the four cases in the order of `CASES` and returns, for each, its name and a pass or a failure text.
pub fn run<S: Subject>(subject: &mut S) -> Vec<(&'static str, Result<(), String>)> {
    vec![
        (CASES[0], resume(subject)),
        (CASES[1], shredded(subject)),
        (CASES[2], filters(subject)),
        (CASES[3], store_changed(subject)),
    ]
}

type Instants = Vec<(StoreId, u64, Instant)>;

struct Conn<S: Subject> {
    writer: S::Writer,
    reader: S::Reader,
    instants: Instants,
}

const READ_BATCH: &str = r#"{"msg":"read_batch"}"#;
const CLOSE: &str = r#"{"msg":"close"}"#;

fn connect<S: Subject>(subject: &mut S, instants: Instants) -> Result<Conn<S>, String> {
    let (writer, reader) = subject.connect()?;
    Ok(Conn {
        writer,
        reader,
        instants,
    })
}

impl<S: Subject> Conn<S> {
    fn end(self) -> Instants {
        self.instants
    }

    fn one_instant(&mut self, b: &Batch) -> Result<(), String> {
        for x in &b.expiry {
            let seen = self
                .instants
                .iter()
                .find(|(store, seg, _)| store == &b.store && *seg == x.seg);
            match seen {
                Some((_, _, at)) if *at != x.at => {
                    return Err(format!(
                        "check {}: segment {} of one store got the expiry instant {} in one reply and {} in another",
                        CHECKS[4], x.seg, at.0, x.at.0
                    ))
                }
                Some(_) => {}
                None => self.instants.push((b.store.clone(), x.seg, x.at)),
            }
        }
        Ok(())
    }

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

    fn error(&mut self, payload: &str, what: &str) -> Result<Cause, String> {
        match self.send(payload)? {
            ServerMessage::Error(e) => Ok(e.cause().clone()),
            _ => Err(format!("{what} got another reply than an error")),
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
type RangeRead<'a> = (&'a str, (Instant, Instant), Option<&'a [&'a str]>);

struct Reading {
    store: StoreId,
    entries: Vec<Entry>,
    watermark: Position,
    low: Position,
    top: Position,
    first_full_next: Option<Position>,
    tail: Option<(Position, Position)>,
    batches: Vec<(Option<Position>, Position, Position)>,
    spans: Vec<(Position, Position, usize)>,
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

fn open<S: Subject>(
    conn: &mut Conn<S>,
    store: Option<&StoreId>,
    position: Position,
    kinds: Option<&[&str]>,
    range: Option<(Instant, Instant)>,
) -> Result<Batch, String> {
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
    Ok(opened)
}

fn read<S: Subject>(
    conn: &mut Conn<S>,
    store: Option<&StoreId>,
    position: Position,
    kinds: Option<&[&str]>,
    range: Option<(Instant, Instant)>,
) -> Result<Reading, String> {
    let opened = open(conn, store, position, kinds, range)?;
    drain(conn, opened, position, true)
}

fn drain<S: Subject>(
    conn: &mut Conn<S>,
    opened: Batch,
    position: Position,
    close: bool,
) -> Result<Reading, String> {
    let mut reading = Reading {
        store: opened.store.clone(),
        entries: Vec::new(),
        watermark: opened.watermark,
        low: opened.watermark,
        top: opened.watermark,
        first_full_next: None,
        tail: None,
        batches: Vec::new(),
        spans: Vec::new(),
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
        conn.one_instant(&b)?;
        if !b.records.is_empty() && reading.first_full_next.is_none() {
            reading.first_full_next = Some(b.next);
        }
        if !b.records.is_empty() {
            reading.tail = Some((b.next, b.watermark));
        }
        reading
            .batches
            .push((b.records.first().map(|e| e.pos), b.next, b.watermark));
        reading.spans.push((cursor, b.next, b.records.len()));
        last = b.records.last().map(|e| e.pos).or(last);
        cursor = b.next;
        watermark = b.watermark;
        reading.low = reading.low.min(b.watermark);
        reading.top = reading.top.max(b.watermark);
        reading.expiry_segs.extend(b.expiry.iter().map(|e| e.seg));
        reading.entries.extend(b.records);
        if b.next == b.watermark {
            reading.watermark = b.watermark;
            if close {
                conn.close()?;
            }
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
    let mut conn = connect(subject, Vec::new())?;
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
    if let Some((next, w)) = reading.tail.filter(|(next, w)| next != w) {
        return Err(format!(
            "the reply that serves the last entry of the reference read has next {}; want its watermark {}",
            show(next),
            show(w)
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

/// The case `resume` (C5.13 item 6): after `restart`, a read from each of eight start positions serves the
/// entries of the reference read at or after it (the sixth, seventh and eighth are the positions one and two
/// past the last entry of the first segment and the index 2^63 - 1 of that segment, which a server reads as
/// the first position of the next segment), at the same positions, from the same store; a position
/// beyond every watermark and the index 1 of the segment of the highest watermark after the restart get
/// `bad-position`, and a legal open after them works; before those two opens, the check `open_while_open` of
/// `CHECKS`.
pub fn resume<S: Subject>(subject: &mut S) -> Result<(), String> {
    let (conn, rf) = reference(subject)?;
    let instants = conn.end();
    subject.restart()?;
    let mut conn = connect(subject, instants)?;
    let second =
        rf.r.iter()
            .find(|e| e.pos.seg != rf.r[0].pos.seg)
            .map(|e| e.pos)
            .ok_or("the reference read served one segment")?;
    let first = rf.r[0].pos.seg;
    let past_first =
        rf.r.iter()
            .rfind(|e| e.pos.seg == first)
            .map(|e| Position {
                seg: first,
                idx: e.pos.idx + 1,
            })
            .ok_or("the reference read is empty")?;
    let starts = [
        ZERO,
        rf.r[10].pos,
        second,
        rf.first_full_next,
        rf.w,
        past_first,
        Position {
            seg: first,
            idx: past_first.idx + 1,
        },
        Position {
            seg: first,
            idx: i64::MAX as u64,
        },
    ];
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
    open_while_open(&mut conn, &rf).map_err(|e| format!("check {}: {e}", CHECKS[1]))?;
    let beyond = Position {
        seg: top.seg + 1,
        idx: 0,
    };
    let inside = Position {
        seg: top.seg,
        idx: 1,
    };
    for at in [beyond, inside] {
        let what = format!("an open_cursor at {}", show(at));
        let cause = conn.error(&open_text(Some(&rf.s), at, None, None), &what)?;
        if cause != Cause::BadPosition {
            return Err(format!("{what} got {}; want bad-position", cause.name()));
        }
    }
    read(&mut conn, Some(&rf.s), ZERO, None, None)
        .map_err(|e| format!("the open after bad-position: {e}"))?;
    Ok(())
}

/// The case `shredded` (C5.13 item 7): after `shred_oldest`, a read from the last position of the lowest
/// segment and one from the zero position serve the entries above that segment, at the same positions, and
/// no `expiry` names it; last, the check `open_at_the_watermark_after_a_shred` of `CHECKS`, which shreds every
/// segment of the reference read.
pub fn shredded<S: Subject>(subject: &mut S) -> Result<(), String> {
    let (conn, rf) = reference(subject)?;
    let instants = conn.end();
    let s = rf.r[0].pos.seg;
    let p =
        rf.r.iter()
            .rfind(|e| e.pos.seg == s)
            .map(|e| e.pos)
            .ok_or("the reference read is empty")?;
    subject.shred_oldest()?;
    let mut conn = connect(subject, instants)?;
    let want: Vec<Entry> = rf.r.iter().filter(|e| e.pos.seg > s).cloned().collect();
    let mut top = rf.w;
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
        top = top.max(reading.top);
    }
    at_the_watermark(&mut conn, &rf, top).map_err(|e| format!("check {}: {e}", CHECKS[2]))?;
    every_segment_shredded(subject, conn, &rf).map_err(|e| format!("check {}: {e}", CHECKS[2]))
}

fn every_segment_shredded<S: Subject>(
    subject: &mut S,
    conn: Conn<S>,
    rf: &Reference,
) -> Result<(), String> {
    let mut segs: Vec<u64> = rf.r.iter().map(|e| e.pos.seg).collect();
    segs.dedup();
    let instants = conn.end();
    for _ in 1..segs.len() {
        subject.shred_oldest()?;
    }
    let mut conn = connect(subject, instants)?;
    let what = "after shred_oldest of every segment of the reference read";
    let reading = read(&mut conn, Some(&rf.s), rf.w, None, None)
        .map_err(|e| format!("{what}, the read from the watermark {}: {e}", show(rf.w)))?;
    if !reading.entries.is_empty() {
        return Err(format!(
            "{what}, the read from the watermark {} served {} entries; want none",
            show(rf.w),
            reading.entries.len()
        ));
    }
    if reading.low < rf.w {
        return Err(format!(
            "{what}, a watermark below that of the reference read"
        ));
    }
    Ok(())
}

fn at_the_watermark<S: Subject>(
    conn: &mut Conn<S>,
    rf: &Reference,
    top: Position,
) -> Result<(), String> {
    let mut marks = vec![rf.w];
    if top != rf.w {
        marks.push(top);
    }
    for at in marks {
        let reading = read(conn, Some(&rf.s), at, None, None).map_err(|e| {
            format!(
                "after shred_oldest, the read from the watermark {}: {e}",
                show(at)
            )
        })?;
        if !reading.entries.is_empty() {
            return Err(format!(
                "after shred_oldest, the read from the watermark {} served {} entries; want none",
                show(at),
                reading.entries.len()
            ));
        }
    }
    Ok(())
}

/// The case `filters` (C5.13 item 8): each kinds filter, a list of two kinds in either order, the run-mark
/// filter, a time range with and without a kind, and a range after every record serve exactly the entries of
/// the reference read that pass them, at their positions, from the zero position and from the position of
/// the 21st entry; four illegal opens get `bad-request` and leave the connection usable; then the check
/// `open_while_open` of `CHECKS` on that connection, a read of the reference read on a second connection,
/// with no restart between, and `open_while_open` again on the second connection. Last, after `load(D)`, a
/// read of set D from the zero position must serve its records in order, the `at` of the `keystroke` after the
/// step back must lie exactly 999 ms below that of the pointer before it, as `load` keeps it when it moves every
/// `at` by a whole number of seconds (C5.13 item 2), and five reads, each from the zero position and from the
/// position of that pointer, must each serve every entry of D at or after the start position that passes the
/// read's filter, at its position: the range from the `at` of the first entry of D to that of the pointer
/// (C5.13 item 8), the range from the same start to 1 ns after the `at` of the `keystroke`, the range from the
/// `at` of the `keystroke` to that of the entry after it, the range from 1 ns after the `at` of the `keystroke`
/// to the same end, and the first range with the kinds filter `keystroke`. A server fails that ends a ranged
/// read at the first entry whose `at` is at or after the end of the range, or after it by less than the step
/// of D, also only under a kinds filter or only from a position other than zero, or that ends it at the first
/// line of a Batch whose `at` is at or after the end, or that drops an entry whose `at` is below that of an
/// entry that it served before, or that finds the first line of a range by a binary search over `at`, or that
/// checks the start only until the first `at` at or after it.
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
    let cases: [FilterCase; 10] = [
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
        (
            "window_title and foreground_app",
            Some(&["window_title", "foreground_app"]),
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
    for start in [ZERO, rf.r[20].pos] {
        for (name, kinds, range, want) in &cases {
            let name = if start == ZERO {
                (*name).to_owned()
            } else {
                format!("{name} from {}", show(start))
            };
            let want: Vec<Entry> = want.iter().filter(|e| e.pos >= start).cloned().collect();
            let reading = read(&mut conn, Some(&rf.s), start, *kinds, *range)
                .map_err(|e| format!("the filter {name}: {e}"))?;
            if !same(&reading.entries, &want) {
                return Err(format!(
                    "the filter {name}: {} entries served, want {}",
                    reading.entries.len(),
                    want.len()
                ));
            }
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
        let cause = conn.error(&open_text(Some(&rf.s), ZERO, kinds, range), name)?;
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
    open_while_open(&mut conn, &rf).map_err(|e| format!("check {}: {e}", CHECKS[1]))?;
    let instants = conn.end();
    let mut conn = connect(subject, instants)?;
    let again = read(&mut conn, Some(&rf.s), ZERO, None, None)
        .map_err(|e| format!("check {}: the read on a second connection: {e}", CHECKS[4]))?;
    if !same(&again.entries, &rf.r) {
        return Err(format!(
            "check {}: the read on a second connection differs from the reference read",
            CHECKS[4]
        ));
    }
    open_while_open(&mut conn, &rf).map_err(|e| format!("check {}: {e}", CHECKS[1]))?;
    step_back(subject, conn.end())
}

fn differs(got: &[Entry], want: &[Entry]) -> String {
    if got.len() != want.len() {
        return format!("{} entries served, want {}", got.len(), want.len());
    }
    match got
        .iter()
        .zip(want)
        .position(|(g, w)| g.pos != w.pos || !alike(&g.record, &w.record))
    {
        Some(i) if got[i].pos != want[i].pos => format!(
            "entry {} served at {}, want it at {}",
            i + 1,
            show(got[i].pos),
            show(want[i].pos)
        ),
        Some(i) => format!(
            "entry {} at {} is another record than set D holds there",
            i + 1,
            show(got[i].pos)
        ),
        None => "the entries served are the entries wanted".to_owned(),
    }
}

fn step_back<S: Subject>(subject: &mut S, instants: Instants) -> Result<(), String> {
    subject.load(&SET_D)?;
    let mut conn = connect(subject, instants)?;
    let d =
        read(&mut conn, None, ZERO, None, None).map_err(|e| format!("the read of set D: {e}"))?;
    let data = records_of(&SET_D)?;
    if d.entries.len() != data.len() {
        return Err(format!(
            "the read of set D served {} entries; want the {} records of D in order",
            d.entries.len(),
            data.len()
        ));
    }
    if let Some(i) = d
        .entries
        .iter()
        .zip(&data)
        .position(|(e, r)| !alike(&e.record, r))
    {
        return Err(format!(
            "the read of set D served at {} another record than line {} of D",
            show(d.entries[i].pos),
            i + 1
        ));
    }
    let first = d.entries[0].record.at();
    let pointer = &d.entries[STEP_BACK - 1];
    let keystroke = d.entries[STEP_BACK].record.at();
    let step = pointer.record.at().0.saturating_sub(keystroke.0);
    if step != STEP_BACK_NS {
        return Err(format!(
            "set D as served steps back by {step} ns from the pointer at {} to the keystroke at {}; want {STEP_BACK_NS} ns, because load moves every at by a whole number of seconds and rounds none (C5.13 item 2)",
            pointer.record.at().0,
            keystroke.0
        ));
    }
    let after = d.entries[STEP_BACK + 1].record.at();
    let just_after = Instant(keystroke.0.saturating_add(1));
    let keystrokes: &[&str] = &["keystroke"];
    let reads: [RangeRead; 5] = [
        (
            "up to the pointer before its step back",
            (first, pointer.record.at()),
            None,
        ),
        (
            "up to just after the keystroke of its step back",
            (first, just_after),
            None,
        ),
        (
            "from the keystroke of its step back",
            (keystroke, after),
            None,
        ),
        (
            "from just after the keystroke of its step back",
            (just_after, after),
            None,
        ),
        (
            "up to the pointer before its step back with keystroke",
            (first, pointer.record.at()),
            Some(keystrokes),
        ),
    ];
    for start in [ZERO, pointer.pos] {
        for (name, (a, b), kinds) in reads {
            let name = if start == ZERO {
                format!("the range of set D {name}")
            } else {
                format!("the range of set D {name} from {}", show(start))
            };
            let want: Vec<Entry> = d
                .entries
                .iter()
                .filter(|e| e.pos >= start && e.record.at() >= a && e.record.at() < b)
                .filter(|e| kinds.is_none_or(|k| k.contains(&e.record.kind().as_str())))
                .cloned()
                .collect();
            let reading = read(&mut conn, Some(&d.store), start, kinds, Some((a, b)))
                .map_err(|e| format!("{name}: {e}"))?;
            if !same(&reading.entries, &want) {
                return Err(format!("{name}: {}", differs(&reading.entries, &want)));
            }
        }
    }
    Ok(())
}

fn another(store: &StoreId) -> Result<StoreId, String> {
    let s = store.as_str();
    let first = if s.starts_with('0') { "1" } else { "0" };
    StoreId::parse(&format!("{first}{}", &s[1..]))
        .map_err(|_| "a store id of the reply does not parse".to_owned())
}

fn open_while_open<S: Subject>(conn: &mut Conn<S>, rf: &Reference) -> Result<(), String> {
    let after =
        rf.r.iter()
            .map(|e| e.record.at())
            .max()
            .unwrap_or(Instant(0));
    let late = (
        Instant(after.0.saturating_add(1)),
        Instant(after.0.saturating_add(2)),
    );
    let opened =
        open(conn, Some(&rf.s), ZERO, None, None).map_err(|e| format!("the first open: {e}"))?;
    let seconds = [
        (
            "an open_cursor with S at another position with a filter, while a cursor is open",
            open_text(Some(&rf.s), rf.r[10].pos, Some(&["keystroke"]), Some(late)),
        ),
        (
            "an open_cursor with another store id, while a cursor is open",
            open_text(Some(&another(&rf.s)?), ZERO, None, Some(late)),
        ),
        (
            "an open_cursor with a null store at the zero position, while a cursor is open",
            open_text(None, ZERO, None, Some(late)),
        ),
        (
            "an open_cursor with S after the watermark, while a cursor is open",
            open_text(
                Some(&rf.s),
                Position {
                    seg: opened.watermark.seg + 1,
                    idx: 0,
                },
                None,
                Some(late),
            ),
        ),
    ];
    for (what, text) in &seconds {
        let cause = conn.error(text, what)?;
        if cause != Cause::BadRequest {
            return Err(format!("{what} got {}; want bad-request", cause.name()));
        }
    }
    let reading = drain(conn, opened, ZERO, true)
        .map_err(|e| format!("the cursor that was open, after the refused opens: {e}"))?;
    if !same(&reading.entries, &rf.r) {
        return Err(format!(
            "the cursor that was open served {} entries after the refused opens; want the {} entries of the reference read",
            reading.entries.len(),
            rf.r.len()
        ));
    }
    let what = "an open_cursor while a cursor stands at the watermark";
    let opened = open(conn, Some(&rf.s), ZERO, None, None)
        .map_err(|e| format!("the open before {what}: {e}"))?;
    let store = opened.store.clone();
    let reading =
        drain(conn, opened, ZERO, false).map_err(|e| format!("the read before {what}: {e}"))?;
    let cause = conn.error(&open_text(Some(&rf.s), ZERO, None, None), what)?;
    if cause != Cause::BadRequest {
        return Err(format!("{what} got {}; want bad-request", cause.name()));
    }
    match conn.send(READ_BATCH)? {
        ServerMessage::Batch(b) => {
            check(
                &b,
                &store,
                reading.watermark,
                reading.entries.last().map(|e| e.pos),
                reading.watermark,
            )
            .map_err(|e| format!("the read_batch after {what}: {e}"))?;
            if !b.records.is_empty() || b.next != b.watermark {
                return Err(format!(
                    "the read_batch after {what} served {} entries with next {}; want none at the watermark",
                    b.records.len(),
                    show(b.next)
                ));
            }
        }
        ServerMessage::Error(e) => {
            return Err(format!(
                "the read_batch after {what} got the error {}",
                e.cause().name()
            ))
        }
        _ => {
            return Err(format!(
                "the read_batch after {what} got a reply other than batch or error"
            ))
        }
    }
    conn.close()
}

fn old_store<S: Subject>(
    conn: &mut Conn<S>,
    old: &StoreId,
    at: Position,
    filter: (Option<&[&str]>, Span),
    what: &str,
) -> Result<StoreId, String> {
    let new = match conn.error(&open_text(Some(old), at, filter.0, filter.1), what)? {
        Cause::StoreChanged(id) if &id != old => id,
        Cause::StoreChanged(_) => {
            return Err(format!(
                "check {}: store-changed names the old store id",
                CHECKS[0]
            ))
        }
        other => return Err(format!("{what} got {}; want store-changed", other.name())),
    };
    let cause = conn.error(READ_BATCH, "read_batch after store-changed")?;
    if cause != Cause::BadRequest {
        return Err(format!(
            "read_batch after store-changed got {}; want bad-request",
            cause.name()
        ));
    }
    Ok(new)
}

/// The case `store_changed` (C5.13 item 9): first, on the connection of the reference read, a cursor opened
/// with S that took one Batch gets `store-changed` with another id at its next `read_batch` after
/// `replace_live`, a further `read_batch` gets `bad-request`, and a cursor opened then serves nothing from the
/// store that the error named. Then, after `load(B)`, the old store id, at a position other than zero and at
/// the zero position, gets `store-changed` with the new store's id and opens no cursor, a null store with a
/// non-zero position gets `bad-request`, and the new
/// store serves the records of set B from the zero position, with a null store and with its new id. Last,
/// after `load(C)`, a read from the zero position serves the records of set C in order, each once; at least
/// two of its Batches end before the store ends, and the Batch after each such stop starts at its `next`;
/// then the check `filtered_stops` of `CHECKS`, and `open_while_open` on the store that it loads.
pub fn store_changed<S: Subject>(subject: &mut S) -> Result<(), String> {
    let (mut conn, rf) = reference(subject)?;
    let opened = open(&mut conn, Some(&rf.s), ZERO, None, None)
        .map_err(|e| format!("the open before replace_live: {e}"))?;
    match conn.send(READ_BATCH)? {
        ServerMessage::Batch(b) => {
            check(&b, &opened.store, ZERO, None, opened.watermark)
                .map_err(|e| format!("the read before replace_live: {e}"))?;
            conn.one_instant(&b)?;
        }
        ServerMessage::Error(e) => {
            return Err(format!(
                "the read before replace_live got the error {}",
                e.cause().name()
            ))
        }
        _ => {
            return Err(
                "the read before replace_live got a reply other than batch or error".to_owned(),
            )
        }
    }
    subject.replace_live()?;
    let replaced = match conn.error(READ_BATCH, "read_batch after replace_live")? {
        Cause::StoreChanged(id) if id != rf.s => id,
        Cause::StoreChanged(_) => {
            return Err("store-changed after replace_live names the old store id".to_owned())
        }
        other => {
            return Err(format!(
                "read_batch after replace_live got {}; want store-changed",
                other.name()
            ))
        }
    };
    let cause = conn.error(READ_BATCH, "a second read_batch after replace_live")?;
    if cause != Cause::BadRequest {
        return Err(format!(
            "a second read_batch after replace_live got {}; want bad-request",
            cause.name()
        ));
    }
    let empty = read(&mut conn, None, ZERO, None, None)
        .map_err(|e| format!("the open after replace_live: {e}"))?;
    if !empty.entries.is_empty() {
        return Err(format!(
            "the store after replace_live served {} entries; want none",
            empty.entries.len()
        ));
    }
    if empty.store != replaced {
        return Err(format!(
            "check {}: store-changed after replace_live names another id than the store that an open with a null store serves then",
            CHECKS[0]
        ));
    }
    let instants = conn.end();
    let p = rf.r[10].pos;
    subject.load(&SET_B)?;
    let mut conn = connect(subject, instants)?;
    let new = old_store(
        &mut conn,
        &rf.s,
        p,
        (None, None),
        "an open_cursor with the old store id",
    )?;
    let at_zero = old_store(
        &mut conn,
        &rf.s,
        ZERO,
        (None, None),
        "an open_cursor with the old store id at the zero position",
    )?;
    let filtered = old_store(
        &mut conn,
        &rf.s,
        p,
        (Some(&["keystroke"]), Some((Instant(0), Instant(i64::MAX)))),
        "an open_cursor with the old store id, a kinds filter and a range",
    )?;
    let cause = conn.error(
        &open_text(None, p, None, None),
        "a null store with a non-zero position",
    )?;
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
        return Err(format!(
            "check {}: the new store's reply names another id than store-changed did",
            CHECKS[0]
        ));
    }
    if first.store != at_zero {
        return Err(format!(
            "check {}: store-changed to an open_cursor with the old store id at the zero position names another id than the new store's reply",
            CHECKS[0]
        ));
    }
    if first.store != filtered {
        return Err(format!(
            "check {}: store-changed to an open_cursor with the old store id, a kinds filter and a range names another id than the new store's reply",
            CHECKS[0]
        ));
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
    let instants = conn.end();
    subject.load(&SET_C)?;
    let mut conn = connect(subject, instants)?;
    let c =
        read(&mut conn, None, ZERO, None, None).map_err(|e| format!("the read of set C: {e}"))?;
    let stops: Vec<usize> = (0..c.batches.len())
        .filter(|&i| c.batches[i].0.is_some() && c.batches[i].1 != c.batches[i].2)
        .collect();
    if stops.len() < 2 {
        return Err(format!(
            "the read of set C ended {} Batches before the store ended; want at least 2",
            stops.len()
        ));
    }
    for i in stops {
        let next = c.batches[i].1;
        match c.batches.get(i + 1).and_then(|b| b.0) {
            Some(first) if first == next => {}
            Some(first) => {
                return Err(format!(
                    "at the stop of the read of set C at {}, the next Batch starts at {}",
                    show(next),
                    show(first)
                ))
            }
            None => {
                return Err(format!(
                    "at the stop of the read of set C at {}, the next Batch holds no record",
                    show(next)
                ))
            }
        }
    }
    let data_c = records_of(&SET_C)?;
    let differs = c
        .entries
        .iter()
        .zip(&data_c)
        .position(|(e, d)| !alike(&e.record, d));
    if c.entries.len() != data_c.len() || differs.is_some() {
        let at = differs.map_or("its end".to_owned(), |i| show(c.entries[i].pos));
        return Err(format!(
            "the read of set C served {} entries, the first that differs at {at}; want the {} records of C in order, each once",
            c.entries.len(),
            data_c.len()
        ));
    }
    let (mut conn, c) =
        filtered_stops(subject, conn).map_err(|e| format!("check {}: {e}", CHECKS[3]))?;
    open_while_open_mid_read(&mut conn, &c).map_err(|e| format!("check {}: {e}", CHECKS[1]))
}

fn open_while_open_mid_read<S: Subject>(conn: &mut Conn<S>, c: &Reading) -> Result<(), String> {
    let what = "an open_cursor while a cursor that has read one Batch is open";
    let opened = open(conn, Some(&c.store), ZERO, None, None)
        .map_err(|e| format!("the open before {what}: {e}"))?;
    let first = match conn.send(READ_BATCH)? {
        ServerMessage::Batch(b) => b,
        ServerMessage::Error(e) => {
            return Err(format!(
                "the read before {what} got the error {}",
                e.cause().name()
            ))
        }
        _ => {
            return Err(format!(
                "the read before {what} got a reply other than batch or error"
            ))
        }
    };
    check(&first, &opened.store, ZERO, None, opened.watermark)
        .map_err(|e| format!("the read before {what}: {e}"))?;
    conn.one_instant(&first)?;
    if first.next == first.watermark {
        return Err(format!(
            "the first Batch before {what} reached the watermark; want one that stops before it"
        ));
    }
    let cause = conn.error(&open_text(Some(&c.store), ZERO, None, None), what)?;
    if cause != Cause::BadRequest {
        return Err(format!("{what} got {}; want bad-request", cause.name()));
    }
    let rest = Batch {
        store: first.store.clone(),
        watermark: first.watermark,
        records: Vec::new(),
        expiry: Vec::new(),
        next: first.next,
    };
    let reading = drain(conn, rest, first.next, true)
        .map_err(|e| format!("the cursor that was open, after {what}: {e}"))?;
    let mut served = first.records;
    served.extend(reading.entries);
    if !same(&served, &c.entries) {
        return Err(format!(
            "the cursor that was open served {} entries in all after {what}; want the {} entries of the store",
            served.len(),
            c.entries.len()
        ));
    }
    Ok(())
}

fn filtered_stops<S: Subject>(
    subject: &mut S,
    conn: Conn<S>,
) -> Result<(Conn<S>, Reading), String> {
    let texts = [SET_A[3], SET_C[0]];
    let instants = conn.end();
    subject.load(&texts)?;
    let mut conn = connect(subject, instants)?;
    let what = "the read of the fourth text of set A and set C";
    let c = read(&mut conn, None, ZERO, None, None).map_err(|e| format!("{what}: {e}"))?;
    let data = records_of(&texts)?;
    let matches = c.entries.len() == data.len()
        && c.entries
            .iter()
            .zip(&data)
            .all(|(e, d)| alike(&e.record, d));
    if !matches {
        return Err(format!(
            "{what} served {} entries; want the {} records of a served kind in order",
            c.entries.len(),
            data.len()
        ));
    }
    let entry = |i: usize| c.entries.get(i).ok_or("the store served too few entries");
    let range = (entry(100)?.record.at(), entry(400)?.record.at());
    let filters: [Filter; 5] = [
        ("window_title", Some(&["window_title"]), None),
        (
            "keystroke and window_title",
            Some(&["keystroke", "window_title"]),
            None,
        ),
        ("the range", None, Some(range)),
        (
            "the range with window_title",
            Some(&["window_title"]),
            Some(range),
        ),
        ("keystroke", Some(&["keystroke"]), None),
    ];
    for start in [ZERO, entry(150)?.pos] {
        for (name, kinds, range) in filters {
            let name = format!(
                "the filter {name} on the fourth text of set A and set C from {}",
                show(start)
            );
            let want: Vec<Entry> = c
                .entries
                .iter()
                .filter(|e| kinds.is_none_or(|k| k.contains(&e.record.kind().as_str())))
                .filter(|e| range.is_none_or(|(a, b)| e.record.at() >= a && e.record.at() < b))
                .cloned()
                .collect();
            let reading = read(&mut conn, Some(&c.store), start, kinds, range)
                .map_err(|e| format!("{name}: {e}"))?;
            let mut from = 0;
            for &(cursor, next, n) in &reading.spans {
                let got = reading
                    .entries
                    .get(from..from + n)
                    .ok_or("the read lost count of its entries")?;
                from += n;
                let due: Vec<Entry> = want
                    .iter()
                    .filter(|e| e.pos >= cursor && e.pos < next)
                    .cloned()
                    .collect();
                if !same(got, &due) {
                    return Err(format!(
                        "{name}: the Batch from {} to next {} served {} entries; want the {} records of that store that pass the filter from its position up to next, each once",
                        show(cursor),
                        show(next),
                        got.len(),
                        due.len()
                    ));
                }
            }
        }
    }
    Ok((conn, c))
}
