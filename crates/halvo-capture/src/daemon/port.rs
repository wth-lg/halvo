//! The store port of contracts.md C11.2: the operations that the store owner
//! calls, and no per-event operation.

use std::fmt;
use std::io;
use std::time::SystemTime;

use halvo_control::model::{StorageCap, UtcDay};
use uuid::{Builder, Uuid, Variant};

use crate::event::Modality;

/// The most bytes of one segment payload.
pub const PAYLOAD_MAX_BYTES: usize = 1_048_576;

/// The most per-kind entries of one segment write.
pub const ENTRIES_MAX: usize = 8;

/// The id of one segment: a random 128-bit value in the form of a version 4
/// UUID. The owner mints one for each batch and keeps it across every retry of
/// that batch, so it is the replay key. Its `Debug` prints a placeholder.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SegmentId(Uuid);

impl SegmentId {
    /// A new id built from 16 random bytes of the system.
    pub fn mint() -> io::Result<SegmentId> {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes)
            .map_err(|_| io::Error::other("the random source refused"))?;
        Ok(SegmentId(Builder::from_random_bytes(bytes).into_uuid()))
    }

    /// Whether the id has the form of a random version 4 UUID.
    pub fn is_random_form(&self) -> bool {
        self.0.get_version_num() == 4 && self.0.get_variant() == Variant::RFC4122
    }

    pub const fn uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Debug for SegmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SegmentId(..)")
    }
}

/// The key of one per-kind entry of a segment write: the kind text of the
/// record codec, and the [`Modality`] when the kind is one. A key whose kind is
/// no `Modality` is the kind of a run-mark record, which only the feature `r2`
/// makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryKind {
    text: &'static str,
    modality: Option<Modality>,
}

impl EntryKind {
    #[cfg(feature = "r2")]
    pub const RUN_MARK: EntryKind = EntryKind {
        text: crate::floor::record::RUN_MARK_KIND,
        modality: None,
    };

    /// The key of the records of `kind`.
    pub const fn of(kind: Modality) -> EntryKind {
        EntryKind {
            text: kind.as_str(),
            modality: Some(kind),
        }
    }

    /// The kind text of the record codec.
    pub const fn as_str(self) -> &'static str {
        self.text
    }

    /// The `Modality` of the key, or `None` for a kind that is no `Modality`.
    pub const fn modality(self) -> Option<Modality> {
        self.modality
    }
}

/// The records of one kind in a segment: how many, and their encoded bytes
/// with their line ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindEntry {
    pub kind: EntryKind,
    pub events: u64,
    pub bytes: u64,
}

/// One batch as the store takes it, all or nothing. The record count is the
/// sum of the counts of `entries`. It has no row, span or id per record and no
/// text field: captured text is only in `payload`. Its fields are private and
/// its one constructor is visible inside `daemon` alone, so no code outside
/// the daemon can fill one: a store reads it.
pub struct SegmentWrite<'a> {
    segment_id: &'a SegmentId,
    day: UtcDay,
    first_capture: i64,
    last_capture: i64,
    entries: &'a [KindEntry],
    payload: &'a [u8],
}

impl<'a> SegmentWrite<'a> {
    pub(super) fn new(
        segment_id: &'a SegmentId,
        day: UtcDay,
        first_capture: i64,
        last_capture: i64,
        entries: &'a [KindEntry],
        payload: &'a [u8],
    ) -> SegmentWrite<'a> {
        SegmentWrite {
            segment_id,
            day,
            first_capture,
            last_capture,
            entries,
            payload,
        }
    }

    pub fn segment_id(&self) -> &'a SegmentId {
        self.segment_id
    }

    pub fn day(&self) -> UtcDay {
        self.day
    }

    pub fn first_capture(&self) -> i64 {
        self.first_capture
    }

    pub fn last_capture(&self) -> i64 {
        self.last_capture
    }

    pub fn entries(&self) -> &'a [KindEntry] {
        self.entries
    }

    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// Whether the store must refuse the write: more than [`ENTRIES_MAX`]
    /// entries, no record, a payload over [`PAYLOAD_MAX_BYTES`], instants out
    /// of order, or a segment id that is not of the random form.
    pub fn is_malformed(&self) -> bool {
        self.entries.len() > ENTRIES_MAX
            || self.entries.iter().map(|entry| entry.events).sum::<u64>() == 0
            || self.payload.len() > PAYLOAD_MAX_BYTES
            || self.first_capture > self.last_capture
            || !self.segment_id.is_random_form()
    }
}

/// The store-wide ordinal of a segment. The store never gives one twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ordinal(u64);

impl Ordinal {
    pub const fn new(value: u64) -> Ordinal {
        Ordinal(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The answer to a stored batch; the store measures both byte counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    pub ordinal: Ordinal,
    pub logical_bytes: u64,
    pub disk_bytes: u64,
}

/// Why a write stored nothing. On every variant the caller keeps the batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFault {
    /// The segment id was stored before: the first write landed.
    AlreadyWritten {
        ordinal: Ordinal,
    },
    /// A bad summary, a payload over the cap, or a segment id that is not of
    /// the random form.
    Refused,
    Io(io::ErrorKind),
    /// A database error with its SQLite result code.
    Db(i32),
    /// The handle refuses writes because a delete-all began on it.
    Sealed,
}

const SQLITE_FULL: i32 = 13;

impl WriteFault {
    /// Whether the fault means that the volume is full.
    pub fn is_no_space(self) -> bool {
        match self {
            WriteFault::Io(kind) => kind == io::ErrorKind::StorageFull,
            WriteFault::Db(code) => code & 0xff == SQLITE_FULL,
            WriteFault::AlreadyWritten { .. } | WriteFault::Refused | WriteFault::Sealed => false,
        }
    }
}

/// What one shred pass did: the ordinals it shredded, whether more segments
/// are due, and whether the store still owes work of a failed pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pass {
    pub shredded: Vec<Ordinal>,
    pub more_due: bool,
    pub owed: bool,
}

/// Why a maintenance call failed. The store keeps what the pass owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintFault {
    Io(io::ErrorKind),
    Db(i32),
    CheckpointIncomplete,
    SegmentMissing,
}

/// The bytes of the store on disk and in its payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub disk_bytes: u64,
    pub logical_bytes: u64,
}

/// A raw byte bound for eviction. Its one constructor in the product takes a
/// [`StorageCap`], which is valid by type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteBound(u64);

impl ByteBound {
    pub const fn of_cap(cap: StorageCap) -> ByteBound {
        ByteBound(cap.bytes())
    }

    pub const fn raw(bytes: u64) -> ByteBound {
        ByteBound(bytes)
    }

    pub const fn bytes(self) -> u64 {
        self.0
    }
}

/// The maintenance calls of a store handle.
pub trait MaintainStore {
    /// Shreds at most `max_segments` segments whose expiry instant is at or
    /// before `now`.
    fn expire_due(&mut self, now: SystemTime, max_segments: usize) -> Result<Pass, MaintFault>;

    /// While disk bytes exceed `bound`, shreds the oldest segments by first
    /// capture, at most `max_segments` of them.
    fn evict_to(&mut self, bound: ByteBound, max_segments: usize) -> Result<Pass, MaintFault>;

    fn usage(&mut self) -> Result<Usage, MaintFault>;
}

/// The read-write handle that the owner holds.
pub trait CaptureStore: MaintainStore {
    /// Stores one batch, all or nothing.
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault>;

    /// The free bytes of the volume that holds the store directory.
    fn volume_free_bytes(&self) -> io::Result<u64>;
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::daemon::model_store::{write_one, ModelStore};

    /// The one check of `expire_due` and `evict_to` that every implementation
    /// runs: a pass lists at most `max_segments` ordinals, each of them one
    /// that the store acknowledged, and no ordinal twice across the passes.
    pub(crate) fn a_pass_keeps_to_its_chunk_and_lists_acknowledged_ordinals<S: MaintainStore>(
        store: &mut S,
        acknowledged: &[Ordinal],
    ) {
        let mut seen = Vec::new();
        let far = UNIX_EPOCH + Duration::from_secs(4_000_000_000);
        for _ in 0..4 {
            for pass in [
                store.expire_due(far, 2).unwrap(),
                store.evict_to(ByteBound::raw(0), 2).unwrap(),
            ] {
                assert!(pass.shredded.len() <= 2, "a pass shredded past its chunk");
                for ordinal in pass.shredded {
                    assert!(
                        acknowledged.contains(&ordinal),
                        "{ordinal:?} was never acknowledged"
                    );
                    assert!(!seen.contains(&ordinal), "{ordinal:?} was shredded twice");
                    seen.push(ordinal);
                }
            }
        }
    }

    #[test]
    fn an_ack_carries_an_ordinal_and_a_shred_pass_lists_ordinals() {
        let store = ModelStore::new();
        let mut first = store.clone();
        let a = write_one(&mut first, b"{}\n").unwrap();
        let b = write_one(&mut first, b"{}\n").unwrap();
        assert!(b.ordinal > a.ordinal);
        assert_eq!(a.logical_bytes, 3);
        let mut restarted = store.clone();
        let c = write_one(&mut restarted, b"{}\n").unwrap();
        assert!(
            c.ordinal > b.ordinal,
            "an ordinal repeated across a restart"
        );
        store.script_expire(vec![Ok(Pass {
            shredded: vec![a.ordinal, b.ordinal],
            more_due: true,
            owed: false,
        })]);
        store.script_evict(vec![Ok(Pass {
            shredded: vec![c.ordinal],
            more_due: false,
            owed: false,
        })]);
        a_pass_keeps_to_its_chunk_and_lists_acknowledged_ordinals(
            &mut restarted,
            &[a.ordinal, b.ordinal, c.ordinal],
        );
    }

    #[test]
    fn a_segment_id_is_random_and_of_the_version_4_form() {
        let a = SegmentId::mint().unwrap();
        let b = SegmentId::mint().unwrap();
        assert!(a.is_random_form() && b.is_random_form());
        assert_ne!(a, b);
        assert_eq!(format!("{a:?}"), "SegmentId(..)");
        assert!(!SegmentId(Uuid::nil()).is_random_form());
        assert!(!SegmentId(Uuid::now_v7()).is_random_form());
    }

    #[test]
    fn no_space_is_a_full_volume_or_sqlite_full() {
        assert!(WriteFault::Io(io::ErrorKind::StorageFull).is_no_space());
        assert!(WriteFault::Db(13).is_no_space());
        assert!(WriteFault::Db(13 | (1 << 8)).is_no_space());
        assert!(!WriteFault::Db(5).is_no_space());
        assert!(!WriteFault::Io(io::ErrorKind::Other).is_no_space());
        assert!(!WriteFault::Refused.is_no_space());
    }
}
