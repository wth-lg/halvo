//! The read of one analysis batch: the worker opens a cursor at the committed position, reads until the bound of
//! the batch and keeps the position of the first record that it did not take; then, where the batch's first records
//! hold a key or a click in front of any title record, it reads back in front of the committed position for the
//! window that they belong to (`carry`).

use std::fmt;

use halvo_identify::{GAP, LOOK_BACK};
use halvo_schema_core::ids::{Position, StoreId};
use halvo_seam::client::{Client, ClientError, Failure, Link};
use halvo_seam::message::{Entry, Expiry, OpenCursor, ZERO};
use halvo_seam::record::{Edge, Payload, Pointer};

/// The most records of one batch. The worker holds every record of its batch in memory for the one call of the
/// episode step, so this bounds its peak memory whatever a day holds.
pub(crate) const RECORDS_MAX: usize = 16_384;

const SECOND_NS: i64 = 1_000_000_000;
const DAY_S: i64 = 86_400;

/// A record is a pause when its instant lies more than this after the instant of every record before it: the
/// episode step's `GAP`, past which it continues no episode, and its `LOOK_BACK`, the farthest that it moves a
/// later record back, together. So no record that the step can move before a pause lies within `GAP` of a record
/// before the pause, and no episode holds records of both sides.
const PAUSE_NS: i64 = (GAP.as_nanos() + LOOK_BACK.as_nanos()) as i64;

/// Why a batch ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The read reached the newest record of the store.
    Newest,
    /// The next record lies on another local day than the batch's first.
    Day,
    /// The batch reached `RECORDS_MAX` records and a record follows them.
    Records,
}

impl End {
    /// The word of the log line.
    pub(crate) fn name(self) -> &'static str {
        match self {
            End::Newest => "newest",
            End::Day => "day",
            End::Records => "records",
        }
    }
}

/// One batch as the worker took it.
pub(crate) struct Taken {
    /// The store that served the batch.
    pub(crate) store: StoreId,
    /// The position that the cursor was opened at.
    pub(crate) from: Position,
    /// The records, in the order of place.
    pub(crate) entries: Vec<Entry>,
    /// One expiry entry for each segment that a read named, in rising order of the segment.
    pub(crate) expiry: Vec<Expiry>,
    /// Why the batch ended.
    pub(crate) end: End,
    /// The position of the first record that the batch did not take: the position that its commit carries.
    pub(crate) commit: Position,
    /// The offset of local time from UTC at the first record that the batch read, in seconds; 0 for a batch that
    /// read no record.
    pub(crate) utc_offset_s: i32,
}

impl Taken {
    /// Keeps the records before the `n`-th record of the batch, and commits that record's position.
    fn keep_before(&mut self, n: usize) {
        self.commit = self.entries[n].pos;
        self.entries.truncate(n);
    }
}

/// A failed read, as the log names it: a name and no text of the server.
pub(crate) struct ReadFault(ClientError);

impl fmt::Display for ReadFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            ClientError::Refused(cause) => f.write_str(cause.name()),
            ClientError::Failed(Failure::Io(kind)) => write!(f, "io:{kind:?}"),
            ClientError::Failed(Failure::End) => f.write_str("end"),
            ClientError::Failed(Failure::Frame) => f.write_str("frame"),
            ClientError::Failed(Failure::Message) => f.write_str("message"),
            ClientError::Failed(Failure::Order(_)) => f.write_str("order"),
            ClientError::Failed(Failure::BadFrame) => f.write_str("bad-frame"),
            ClientError::Failed(Failure::Stopped) => f.write_str("stopped"),
            ClientError::NotNow => f.write_str("not-now"),
            ClientError::Write(_) => f.write_str("write"),
        }
    }
}

/// Reads one batch through `client`. With `from` it opens the cursor at that position of that store, and with none
/// at the zero position of whatever store the server holds. `utc_offset` gives the offset of local time from UTC,
/// in seconds, at a second since 1970-01-01.
///
/// The batch ends, whichever comes first, at the newest record (a read whose `next` is its watermark), at
/// `RECORDS_MAX` records that a record follows, or before the first record whose local day is not that of the
/// batch's first record. A batch that stops inside a read keeps the `pos` of the first record that it did not take,
/// and one that took the whole read keeps its `next`.
///
/// The read follows the run marks in the order of place, with the two states of C4.5: a `start` opens an agent's
/// run, an `end` closes it, and a `start` while a run is open or an `end` while none is open changes nothing. No
/// batch that ends at the bound or at the newest record commits the position of a record inside a run, so that
/// the episode step, which starts each call with no run open, never reads the rest of a run as the person's.
///
/// A batch that reaches `RECORDS_MAX` records ends before its last pause. A pause is a record after the batch's
/// first, before which no run is open, whose instant lies more than `PAUSE_NS` after the instant of every record
/// before it: the batch keeps the records before that record and its commit is that record's `pos`, so the next
/// start reads the rest. With no pause and a run open after its last record, it ends before that run's `start`
/// mark in the same way. With neither it keeps all `RECORDS_MAX` records. The episode step closes every episode at
/// the end of its call, so it makes two episodes of one that such a cut divides.
///
/// When the `start` mark of the run that is open after `RECORDS_MAX` records is the batch's first record, the batch
/// reads on, holding that mark and no later record, to the run's `end` mark: it keeps no record and commits that
/// mark's `pos`. When the newest record or another local day comes first, the batch ends there as below.
///
/// A batch that reaches the newest record while a run is open ends before that run's `start` mark; when the mark is
/// its first record, it keeps no record and commits the position that it started from, so the next start reads the
/// run again. A run that is open at the first record of another local day ends with the batch, which keeps the
/// records that it holds, and the next start reads the rest of the run as the person's.
pub(crate) fn take<L: Link>(
    client: &mut Client<L>,
    from: Option<(StoreId, Position)>,
    utc_offset: &dyn Fn(i64) -> i32,
) -> Result<Taken, ReadFault> {
    let (open, from) = match from {
        Some((store, position)) => (OpenCursor::at(store, position, None, None), position),
        None => (Some(OpenCursor::fresh(None, None)), ZERO),
    };
    let open = open.ok_or(ReadFault(ClientError::NotNow))?;
    let opened = client.open(open).map_err(ReadFault)?;
    let mut taken = Taken {
        store: opened.store,
        from,
        entries: Vec::new(),
        expiry: Vec::new(),
        end: End::Newest,
        commit: from,
        utc_offset_s: 0,
    };
    let mut first_day = None;
    let mut latest: Option<i64> = None;
    let mut pause: Option<usize> = None;
    // The index in `taken.entries` of the `start` mark of the run that is open after the last record read.
    let mut run: Option<usize> = None;
    // Whether the batch reads past a run that its first record opened and that is open after `RECORDS_MAX` records.
    let mut past = false;
    loop {
        let batch = client.read().map_err(ReadFault)?;
        for expiry in batch.expiry {
            if let Err(place) = taken.expiry.binary_search_by_key(&expiry.seg, |e| e.seg) {
                taken.expiry.insert(place, expiry);
            }
        }
        let mut records = batch.records.into_iter().peekable();
        while let Some(entry) = records.next() {
            let secs = entry.record.at().0.div_euclid(SECOND_NS);
            let offset = utc_offset(secs);
            let day = secs.saturating_add(i64::from(offset)).div_euclid(DAY_S);
            match first_day {
                None => {
                    first_day = Some(day);
                    taken.utc_offset_s = offset;
                }
                Some(first) if first != day => {
                    taken.end = End::Day;
                    taken.commit = entry.pos;
                    return Ok(taken);
                }
                Some(_) => {}
            }
            let edge = match entry.record.payload() {
                Payload::RunMark(edge) => Some(*edge),
                _ => None,
            };
            if past {
                if edge == Some(Edge::End) {
                    taken.entries.clear();
                    taken.end = End::Records;
                    taken.commit = entry.pos;
                    return Ok(taken);
                }
                continue;
            }
            let at = entry.record.at().0;
            if run.is_none() && latest.is_some_and(|latest| at.saturating_sub(latest) > PAUSE_NS) {
                pause = Some(taken.entries.len());
            }
            latest = Some(latest.map_or(at, |latest| latest.max(at)));
            match edge {
                Some(Edge::Start) if run.is_none() => run = Some(taken.entries.len()),
                Some(Edge::End) => run = None,
                _ => {}
            }
            taken.entries.push(entry);
            if taken.entries.len() == RECORDS_MAX {
                let follows = match records.peek() {
                    Some(left) => Some(left.pos),
                    None if batch.next != batch.watermark => Some(batch.next),
                    None => None,
                };
                if let Some(follows) = follows {
                    match (pause, run) {
                        (None, Some(0)) => {
                            past = true;
                            taken.entries.truncate(1);
                        }
                        (Some(cut), _) | (None, Some(cut)) => {
                            taken.end = End::Records;
                            taken.keep_before(cut);
                            return Ok(taken);
                        }
                        (None, None) => {
                            taken.end = End::Records;
                            taken.commit = follows;
                            return Ok(taken);
                        }
                    }
                }
            }
        }
        if batch.next == batch.watermark {
            taken.end = End::Newest;
            match run {
                None => taken.commit = batch.next,
                Some(0) => {
                    taken.entries.clear();
                    taken.commit = taken.from;
                }
                Some(start) => taken.keep_before(start),
            }
            return Ok(taken);
        }
    }
}

/// The window that a batch carries over from before its committed position: the last title record before that
/// position, and the count of the batch's first records, in the order of place, that belong to it.
pub(crate) struct Carried {
    /// The last title record before the committed position.
    pub(crate) title: Entry,
    /// The count of the batch's first records that the window reaches.
    pub(crate) until: usize,
}

/// The window that `taken` carries over, read back through `client` in front of the position that the batch was read
/// from, or none. It writes nothing and keeps nothing beyond the call.
///
/// It reads back only for a batch read from a committed position whose records in front of its first title record
/// hold a key or a click. It closes the batch's cursor and opens one at the first index of the committed position's
/// segment, then of each segment below it, and reads in each the records before the committed position, until a
/// segment holds a title record: the last of them is the last title record before the committed position. There is
/// no carry where a segment that it asks for is not the one that the cursor serves (a missing or shredded segment),
/// where a read is refused or fails, where more than `RECORDS_MAX` records lie between that title record and the
/// committed position, or where the title record holds no app.
///
/// The one rule: from the title record on, in the order of place, through the records read back and on into the
/// batch, each record's `seq` is the `seq` of the record before it plus one, no record is a `foreground_app` or a
/// `run_mark` record, and each record holds the title record's app. A record read back that breaks it leaves no
/// carry; in the batch the first record that breaks it, or the batch's first title record, ends the carry.
pub(crate) fn carry<L: Link>(client: &mut Client<L>, taken: &Taken) -> Option<Carried> {
    if taken.from == ZERO {
        return None;
    }
    let lead = taken
        .entries
        .iter()
        .position(is_title)
        .unwrap_or(taken.entries.len());
    let moves = |entry: &Entry| {
        matches!(
            entry.record.payload(),
            Payload::Keystroke(_) | Payload::Pointer(Pointer::Click(_))
        )
    };
    if !taken.entries[..lead].iter().any(moves) {
        return None;
    }
    client.close().ok()?;
    let mut back: Vec<Entry> = Vec::new();
    let mut seg = taken.from.seg;
    let at = loop {
        let mut here = segment_before(client, &taken.store, seg, taken.from)?;
        let found = here.iter().rposition(is_title);
        here.append(&mut back);
        back = here;
        if back.len() > RECORDS_MAX {
            return None;
        }
        if let Some(at) = found {
            break at;
        }
        seg = seg.checked_sub(1)?;
    };
    let mut rest = back.split_off(at);
    let title = rest.remove(0);
    let app = title.record.app()?.clone();
    let mut seq = title.record.seq().get();
    if !rest.iter().all(|entry| follows(entry, &mut seq, &app)) {
        return None;
    }
    let until = taken.entries[..lead]
        .iter()
        .take_while(|entry| follows(entry, &mut seq, &app))
        .count();
    (until > 0).then_some(Carried { title, until })
}

fn is_title(entry: &Entry) -> bool {
    matches!(entry.record.payload(), Payload::WindowTitle(_))
}

/// Whether `entry` keeps the carry after the record whose `seq` is `seq`: its `seq` is one above, it is no
/// `foreground_app` and no `run_mark` record, and it holds `app`. Where it keeps it, `seq` becomes its `seq`.
fn follows(entry: &Entry, seq: &mut u64, app: &halvo_schema_core::site::AppId) -> bool {
    let record = &entry.record;
    let keeps = Some(record.seq().get()) == seq.checked_add(1)
        && !matches!(
            record.payload(),
            Payload::ForegroundApp | Payload::RunMark(_)
        )
        && record.app() == Some(app);
    if keeps {
        *seq = record.seq().get();
    }
    keeps
}

/// The records of the segment `seg` of `store` that lie before `from`, read through a cursor opened at the segment's
/// first index and closed after it; none where the cursor serves another segment first, or a read is refused or
/// fails. A segment at or after `from`'s holds no record before it.
fn segment_before<L: Link>(
    client: &mut Client<L>,
    store: &StoreId,
    seg: u64,
    from: Position,
) -> Option<Vec<Entry>> {
    let first = Position { seg, idx: 0 };
    if first >= from {
        return Some(Vec::new());
    }
    client
        .open(OpenCursor::at(store.clone(), first, None, None)?)
        .ok()?;
    let mut out = Vec::new();
    let mut done = false;
    while !done {
        let batch = client.read().ok()?;
        for entry in batch.records {
            if entry.pos >= from || entry.pos.seg != seg {
                done = true;
                break;
            }
            out.push(entry);
        }
        done = done || batch.next == batch.watermark || batch.next.seg != seg;
    }
    client.close().ok()?;
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use std::io::{PipeReader, PipeWriter};
    use std::path::{Path, PathBuf};
    use std::thread::JoinHandle;

    use halvo_identify::{episode_step, Call, Carry, StageA};
    use halvo_schema_core::ids::SummaryId;
    use halvo_schema_core::record::{Instant, Text};
    use halvo_schema_core::site::AppId;
    use halvo_seam::client::StreamLink;
    use halvo_seam::fixture::FixtureTransport;
    use halvo_seam::record::{
        Button, CaptureRecord, Count, KeyClass, Motion, RecordId, Seq, Title,
    };

    use super::*;

    const NUMBERS: &str = "com.apple.iWork.Numbers";
    const OTHER: &str = "com.microsoft.Excel";
    /// 2026-01-01T12:00:00Z, in seconds since 1970; every record lies on that day with no offset of local time.
    const NOON_S: i64 = 1_767_268_800;

    /// A new directory under the system's directory of temporary files, removed when the value is dropped.
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// One record to write: its `seq`, its app and its payload; the n-th record of a case lies n seconds after noon.
    #[derive(Clone)]
    struct Line(u64, &'static str, Payload);

    fn title(seq: u64) -> Line {
        Line(
            seq,
            NUMBERS,
            Payload::WindowTitle(Title {
                text: Text::parse("Budget").unwrap(),
                cut: Some(false),
                why: None,
                site: None,
            }),
        )
    }

    fn key(seq: u64, app: &'static str) -> Line {
        Line(seq, app, Payload::Keystroke(Some(KeyClass::Printable)))
    }

    /// Writes each segment of `segments` to a batch file of its own, the k-th file the segment k, and returns the
    /// files with their directory.
    fn files(name: &str, segments: &[&[Line]]) -> (Scratch, Vec<PathBuf>) {
        let dir = halvo_test_dir::make(Path::new(&std::env::temp_dir()), name).unwrap();
        let mut n = 0u64;
        let mut paths = Vec::new();
        for (k, lines) in segments.iter().enumerate() {
            let mut bytes = Vec::new();
            for Line(seq, app, payload) in lines.iter().cloned() {
                n += 1;
                let id = RecordId::parse(&format!("00000000-0000-7000-8000-{n:012x}")).unwrap();
                let at = Instant((NOON_S + i64::try_from(n).unwrap()) * SECOND_NS);
                let app =
                    (!matches!(payload, Payload::RunMark(_))).then(|| AppId::parse(app).unwrap());
                let record =
                    CaptureRecord::new(id, at, Seq::new(seq).unwrap(), app, payload).unwrap();
                bytes.extend(record.to_line());
            }
            let path = dir.join(format!("day-{:03}.jsonl", k + 1));
            std::fs::write(&path, bytes).unwrap();
            paths.push(path);
        }
        (Scratch(dir), paths)
    }

    /// What a batch carries, as a test reads it: the position of the title record and the reach, or none.
    type Seen = Option<(Position, usize)>;

    /// The batch that a worker reads from the first index of the segment `from_seg` of a store whose segments below
    /// `first_live` are shredded, with the window that it carries.
    fn read_back(
        name: &str,
        segments: &[&[Line]],
        first_live: u64,
        from_seg: u64,
    ) -> (Taken, Option<Carried>) {
        let (_scratch, paths) = files(name, segments);
        let mut transport = FixtureTransport::open(&paths, first_live, None).unwrap();
        let store = transport.store().clone();
        let (server_reads, client_writes): (PipeReader, PipeWriter) = std::io::pipe().unwrap();
        let (client_reads, server_writes) = std::io::pipe().unwrap();
        let serving: JoinHandle<()> = std::thread::spawn(move || {
            let _ = transport.serve(server_reads, server_writes);
        });
        let mut client = Client::new(StreamLink::new(client_reads, client_writes));
        let from = Position {
            seg: from_seg,
            idx: 0,
        };
        let taken = take(&mut client, Some((store, from)), &|_| 0)
            .unwrap_or_else(|fault| panic!("the batch was not read: {fault}"));
        let carried = carry(&mut client, &taken);
        drop(client);
        serving.join().unwrap();
        (taken, carried)
    }

    /// The window that such a batch carries: the position of the title record and the reach.
    fn carried(name: &str, segments: &[&[Line]], first_live: u64, from_seg: u64) -> Seen {
        let (_, carried) = read_back(name, segments, first_live, from_seg);
        carried.map(|carried| (carried.title.pos, carried.until))
    }

    /// The control of the rule: a `foreground_app` record of the same app in front of the batch's keys, which a
    /// source writes at its start and at each change of the app in front, leaves no carry; the same records with a
    /// key in its place carry the title record of the segment before.
    #[test]
    fn a_foreground_app_record_in_front_of_the_keys_leaves_no_carry() {
        let before: &[Line] = &[title(1), key(2, NUMBERS), key(3, NUMBERS)];
        let front = Line(4, NUMBERS, Payload::ForegroundApp);
        assert_eq!(
            carried(
                "carry-front",
                &[before, &[front, key(5, NUMBERS), key(6, NUMBERS)]],
                1,
                2
            ),
            None
        );
        assert_eq!(
            carried(
                "carry-twin",
                &[before, &[key(4, NUMBERS), key(5, NUMBERS), key(6, NUMBERS)]],
                1,
                2
            ),
            Some((Position { seg: 1, idx: 0 }, 3))
        );
    }

    /// A record of another app in front of the title record's app's own records takes no window and does not end the
    /// carry. On Windows a click and a pointer fold hold the app of the top-level window under the pointer, so a
    /// pointer that crosses the taskbar or another program on its way back to the workbook writes such records. Here
    /// a fold and a click of another app stand in front of five keys and a click of Numbers: the carry reaches all
    /// eight records, and the one episode that the step makes holds the six records of Numbers alone, each with the
    /// carried title.
    #[test]
    fn a_record_of_another_app_takes_no_window_and_does_not_end_the_carry() {
        let pointer = |seq, app, pointer| Line(seq, app, Payload::Pointer(pointer));
        let batch = [
            pointer(3, OTHER, Pointer::Motion(Motion::Move, Count::new(4))),
            pointer(4, OTHER, Pointer::Click(Some(Button::Primary))),
            key(5, NUMBERS),
            key(6, NUMBERS),
            pointer(7, NUMBERS, Pointer::Click(Some(Button::Primary))),
            key(8, NUMBERS),
            key(9, NUMBERS),
            key(10, NUMBERS),
        ];
        let before: &[Line] = &[title(1), key(2, NUMBERS)];
        let (taken, carried) = read_back("carry-other-app", &[before, &batch], 1, 2);
        let carried = carried.expect("the batch carries no window");
        assert_eq!(
            (carried.title.pos, carried.until),
            (Position { seg: 1, idx: 0 }, 8)
        );
        let mut n = 0u8;
        let mut ids = || {
            n += 1;
            SummaryId::from_random([n; 16])
        };
        let call = Call {
            entries: &taken.entries,
            expiry: &taken.expiry,
            utc_offset_s: 0,
            carry: Some(Carry {
                title: &carried.title,
                until: carried.until,
            }),
        };
        let episodes = episode_step(&call, &StageA, &mut ids).episodes;
        assert_eq!(episodes.len(), 1);
        let steps = &episodes[0].steps;
        assert_eq!(steps.len(), 6);
        for step in steps {
            assert_eq!(
                step.app.as_ref().map(|app| app.value.as_str()),
                Some(NUMBERS)
            );
            assert_eq!(
                step.title.as_ref().map(|title| title.value.as_str()),
                Some("Budget")
            );
        }
    }

    /// Each break that the store marks ends the carry where it stands: a `seq` that is not one above, a `seq` that
    /// starts again (a new life of the store owner), a `run_mark`, another app, a shredded segment between the title
    /// record and the batch, and a batch with no key or click in front of its first title record. With none of them
    /// a batch carries the title record of two segments back across the segment between.
    #[test]
    fn each_break_that_the_store_marks_ends_the_carry_there() {
        let before: &[Line] = &[title(1), key(2, NUMBERS)];
        let between: &[Line] = &[key(3, NUMBERS), key(4, NUMBERS)];
        let start = Line(5, NUMBERS, Payload::RunMark(Edge::Start));
        let cases: [(&str, Vec<Line>, u64, Seen); 7] = [
            (
                "clean",
                vec![key(5, NUMBERS), key(6, NUMBERS)],
                1,
                Some((Position { seg: 1, idx: 0 }, 2)),
            ),
            (
                "gap",
                vec![key(5, NUMBERS), key(7, NUMBERS)],
                1,
                Some((Position { seg: 1, idx: 0 }, 1)),
            ),
            ("new-life", vec![key(1, NUMBERS), key(2, NUMBERS)], 1, None),
            ("run-mark", vec![start, key(6, NUMBERS)], 1, None),
            ("other-app", vec![key(5, OTHER), key(6, NUMBERS)], 1, None),
            ("shredded", vec![key(5, NUMBERS)], 2, None),
            ("titled", vec![title(5), key(6, NUMBERS)], 1, None),
        ];
        for (name, batch, first_live, want) in cases {
            let got = carried(
                &format!("carry-{name}"),
                &[before, between, &batch],
                first_live,
                3,
            );
            assert_eq!(got, want, "{name}");
        }
    }
}
