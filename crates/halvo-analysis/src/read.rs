//! The read of one analysis batch: the worker opens a cursor at the committed position, reads until the bound of
//! the batch and keeps the position of the first record that it did not take.

use std::fmt;

use halvo_schema_core::ids::{Position, StoreId};
use halvo_seam::client::{Client, ClientError, Failure, Link};
use halvo_seam::message::{Entry, Expiry, OpenCursor, ZERO};

/// The most records of one batch.
pub(crate) const RECORDS_MAX: usize = 2_000_000;

const SECOND_NS: i64 = 1_000_000_000;
const DAY_S: i64 = 86_400;

/// Why a batch ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// The read reached the newest record of the store.
    Newest,
    /// The next record lies on another local day than the batch's first.
    Day,
    /// The batch holds `RECORDS_MAX` records.
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
    /// The offset of local time from UTC at the batch's first record, in seconds; 0 for a batch with no record.
    pub(crate) utc_offset_s: i32,
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
/// The batch ends, whichever comes first, at the newest record (a read whose `next` is its watermark), after
/// `RECORDS_MAX` records, or before the first record whose local day is not that of the batch's first record. A
/// batch that stops inside a read keeps the `pos` of the first record that it did not take, and one that took the
/// whole read keeps its `next`.
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
    loop {
        let batch = client.read().map_err(ReadFault)?;
        for expiry in batch.expiry {
            if let Err(place) = taken.expiry.binary_search_by_key(&expiry.seg, |e| e.seg) {
                taken.expiry.insert(place, expiry);
            }
        }
        let mut records = batch.records.into_iter();
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
            taken.entries.push(entry);
            if taken.entries.len() == RECORDS_MAX {
                match records.next() {
                    Some(left) => {
                        taken.end = End::Records;
                        taken.commit = left.pos;
                        return Ok(taken);
                    }
                    None if batch.next != batch.watermark => {
                        taken.end = End::Records;
                        taken.commit = batch.next;
                        return Ok(taken);
                    }
                    None => {}
                }
            }
        }
        if batch.next == batch.watermark {
            taken.end = End::Newest;
            taken.commit = batch.next;
            return Ok(taken);
        }
    }
}
