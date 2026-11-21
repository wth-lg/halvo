//! The read of one analysis batch: the worker opens a cursor at the committed position, reads until the bound of
//! the batch and keeps the position of the first record that it did not take.

use std::fmt;

use halvo_identify::{GAP, LOOK_BACK};
use halvo_schema_core::ids::{Position, StoreId};
use halvo_seam::client::{Client, ClientError, Failure, Link};
use halvo_seam::message::{Entry, Expiry, OpenCursor, ZERO};
use halvo_seam::record::{Edge, Payload};

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
