//! The encrypted segment store: its traits, types and errors.

pub mod db;
pub(crate) mod durable;
pub(in crate::store) mod expiry;
#[cfg(test)]
pub(crate) mod failpoint;
pub(in crate::store) mod inventory;
pub(in crate::store) mod lock;
#[cfg(not(test))]
pub(in crate::store) mod failpoint {
    pub(crate) fn hit(_name: &'static str) -> super::StoreResult<()> {
        Ok(())
    }
}
pub(in crate::store) mod segment;
pub(in crate::store) mod verify;

#[cfg(test)]
mod formal_tests;
#[cfg(test)]
mod hardening_tests;
#[cfg(test)]
#[path = "test_support/pages.rs"]
pub(in crate::store) mod pages;
#[cfg(test)]
#[path = "test_support/roots.rs"]
pub(crate) mod roots;
#[cfg(test)]
use halvo_sqlcipher::rusqlite::Connection;
#[cfg(test)]
#[path = "../../../halvo-sqlcipher/tests/page_gap/rules.rs"]
mod page_gap_rules;

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::custody::{CustodyError, RootId};
use crate::event::{CaptureStreamEvent, SegmentKeyId, SegmentRef};

pub(in crate::store) const DEFAULT_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// The most bytes of one segment payload (contracts.md C11.2 and C18 item 5).
pub const PAYLOAD_MAX_BYTES: usize = 1_048_576;

/// The most per-kind entries of one segment write (contracts.md C11.2).
pub const KINDS_MAX: usize = 8;

/// The most bytes of a kind text.
pub const KIND_MAX_BYTES: usize = 32;

const NANOS_PER_DAY: i64 = 86_400 * 1_000_000_000;

/// A UTC day: whole days since 1970-01-01, negative before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcDay(i64);

impl UtcDay {
    pub const fn from_days_since_epoch(days: i64) -> UtcDay {
        UtcDay(days)
    }

    /// The UTC day of an instant in signed Unix nanoseconds.
    pub const fn of_instant(nanos: i64) -> UtcDay {
        UtcDay(nanos.div_euclid(NANOS_PER_DAY))
    }

    pub const fn days_since_epoch(self) -> i64 {
        self.0
    }
}

/// The store-wide ordinal of a segment: its key row's id, which the store
/// never gives twice, also after a shred of the newest segments and a reopen,
/// except in the case of ruling ST-39 (4): a program of the same user that
/// puts another file of more than 0 bytes at the log's name while SQLite
/// writes its log elsewhere can make the store give an ordinal twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ordinal(u64);

impl Ordinal {
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(in crate::store) const fn of_row(id: i64) -> Ordinal {
        Ordinal(id as u64)
    }
}

/// The records of one kind in a batch: the kind text that the caller takes
/// from the one codec, how many records, and their encoded bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindTotal<'a> {
    pub kind: &'a str,
    pub events: u64,
    pub bytes: u64,
}

/// One batch, borrowed from its caller, which keeps it on every answer. The
/// segment id is the replay key; the record count is the sum of the counts.
#[derive(Debug, Clone, Copy)]
pub struct SegmentWrite<'a> {
    pub segment_id: &'a uuid::Uuid,
    pub day: UtcDay,
    pub first_capture: i64,
    pub last_capture: i64,
    pub kinds: &'a [KindTotal<'a>],
    pub payload: &'a [u8],
}

/// Why the store refused a batch before it wrote anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The segment id is not a random version 4 UUID.
    NotRandomId,
    /// The payload holds more than [`PAYLOAD_MAX_BYTES`] bytes.
    PayloadOverCap,
    /// The counts add up to no record.
    NoEvent,
    /// More than [`KINDS_MAX`] kinds.
    TooManyKinds,
    /// One kind stands twice.
    RepeatedKind,
    /// A kind text that is not 1 to [`KIND_MAX_BYTES`] bytes of lower-case
    /// letters and `_`.
    KindNotText,
    /// The first capture comes after the last.
    InstantsOutOfOrder,
    /// The two instants fall on two UTC days.
    InstantsOnTwoDays,
    /// The day is not the UTC day of the instants.
    DayNotTheInstants,
}

/// A stored batch; the store measures both byte counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    pub ordinal: Ordinal,
    pub logical_bytes: u64,
    pub disk_bytes: u64,
}

/// Why a batch write did not answer `Written`. No row and no file remain
/// from it, except after an `Io` answer when the look after the commit could
/// not read the log's entry: then the row and the file may both stand, and a
/// retry after the next open answers `AlreadyWritten`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFault {
    /// The segment id was stored before, under this ordinal.
    AlreadyWritten {
        ordinal: Ordinal,
    },
    Refused(Refusal),
    Io(std::io::ErrorKind),
    /// A database error with its SQLite result code; 5 for a store that
    /// another party holds, 11 for damage, 0 for a seal that failed.
    Db(i32),
    /// The handle refuses writes because a delete-all began on it.
    Sealed,
}

impl From<StoreError> for WriteFault {
    fn from(e: StoreError) -> WriteFault {
        match e {
            StoreError::Sealed => WriteFault::Sealed,
            StoreError::Io(kind) => WriteFault::Io(kind),
            StoreError::Db(code) => WriteFault::Db(code),
            StoreError::Seal => WriteFault::Db(0),
            StoreError::Locked => WriteFault::Db(halvo_sqlcipher::rusqlite::ffi::SQLITE_BUSY),
            StoreError::Corrupt(_) => {
                WriteFault::Db(halvo_sqlcipher::rusqlite::ffi::SQLITE_CORRUPT)
            }
            StoreError::SegmentMissing => WriteFault::Io(std::io::ErrorKind::NotFound),
            _ => WriteFault::Io(std::io::ErrorKind::Other),
        }
    }
}

/// One row of the per-day rollup: a UTC day, a kind text, and the records
/// and bytes of the live segments of that day and kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryRow {
    pub day: UtcDay,
    pub kind: String,
    pub events: u64,
    pub bytes: u64,
}

/// What the open-time reconcile did: the keyless segment files that it
/// removed, and the keyed rows whose file was missing that it shredded in
/// place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reconciled {
    pub files_removed: u64,
    pub rows_shredded: u64,
    /// A step failed; the next open does what it left.
    pub owed: bool,
}

/// The store the capture daemon appends to.
pub trait SegmentStore {
    /// Refused with [`StoreError::NotImplemented`]: the store keeps no
    /// per-event row. It touches nothing.
    fn append_event(&mut self, event: &CaptureStreamEvent) -> StoreResult<()>;

    /// Write raw capture bytes as a new encrypted segment with no totals, its
    /// instants the write instant, and return its ref and its ordinal.
    fn write_segment(&mut self, bytes: &mut dyn Read) -> StoreResult<(SegmentRef, SegmentKeyId)>;

    /// The summed lengths of the files under the store directory, for the cost
    /// bound. A directory that cannot be read is an error; one that
    /// went away is skipped.
    fn size_bytes(&self) -> StoreResult<u64>;
}

/// Window-expiry crypto-shred: destroy the per-segment key for every segment past
/// its retention bound, so its ciphertext becomes unrecoverable.
pub trait SegmentExpiry {
    /// Crypto-shred every segment past its retention bound. When a shred's
    /// commit changed a row and the look after the commit then fails, the
    /// pass answers that failure and reports no shred, the handle answers
    /// the same to every later call that reads or writes the database, and a
    /// later open finishes the shred. When the checkpoint after the commit
    /// fails, and fails again after the removal of the chunk's files, the
    /// pass answers that failure, the handle answers it to every later call
    /// that reads or writes the database but an expiry or eviction call, and
    /// such a later call, or a later open, finishes the checkpoint.
    fn crypto_shred_expired(&mut self) -> StoreResult<ShredReport>;

    /// Crypto-shred everything and destroy the store key (delete-all), in the
    /// order of [`db::EncryptedStore::delete_all`], which it calls: `Ok` with
    /// the count of keyed segments, and that count plus the store key as the
    /// keys destroyed, when the root is gone and nothing is left (an empty
    /// report when the count could not be read or a second call finished the
    /// work); otherwise [`StoreError::DeleteIncomplete`] with the report, after
    /// every step ran. A second call on the same handle resumes.
    fn crypto_shred_all(&mut self) -> StoreResult<ShredReport>;
}

/// What one expiry or eviction call did: the ordinals of the segments that it
/// shredded, in ascending order, whether more segments are due, and whether
/// the store still owes work that a call left undone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pass {
    pub shredded: Vec<Ordinal>,
    pub more_due: bool,
    pub owed: bool,
}

/// The store's two byte totals. `disk_bytes` is what the files take on the
/// volume: the allocated size of each live segment's file, the live pages of
/// the database (its pages less its free pages, times the page size) and the
/// bytes of its log. `logical_bytes` is the payload bytes of the live
/// segments. The cap compares `disk_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub disk_bytes: u64,
    pub logical_bytes: u64,
}

/// The result of a crypto-shred pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShredReport {
    pub segments_shredded: u64,
    pub keys_destroyed: u64,
}

/// A name of the store, for [`StoreError::Obstructed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreName {
    StoreDirectory,
    Parent,
    KeyFile,
    Database,
    Log,
    SharedMemory,
    Journal,
    WriterLock,
    Segments,
    Shard,
    SegmentFile,
}

/// What stood at a store name in place of the store's own entry, for
/// [`StoreError::Obstructed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Met {
    Link,
    OtherKind,
    DirectoryNotAtItsName,
    DatabaseOutsideTheDirectory,
}

impl fmt::Display for StoreName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StoreName::StoreDirectory => "the store directory",
            StoreName::Parent => "a parent of the store directory",
            StoreName::KeyFile => "the store-key file",
            StoreName::Database => "the database file",
            StoreName::Log => "the write-ahead log",
            StoreName::SharedMemory => "the shared-memory file",
            StoreName::Journal => "the journal",
            StoreName::WriterLock => "the writer lock",
            StoreName::Segments => "the segments directory",
            StoreName::Shard => "a shard directory",
            StoreName::SegmentFile => "a segment file",
        })
    }
}

impl fmt::Display for Met {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Met::Link => "a link",
            Met::OtherKind => "an entry of another kind than the store keeps there",
            Met::DirectoryNotAtItsName => "a directory that is no longer at its name",
            Met::DatabaseOutsideTheDirectory => {
                "a database that SQLite holds outside the store directory"
            }
        })
    }
}

/// Where a data directory keeps its store and its roots, for the delete tail
/// of [`db::EncryptedStore::delete_data_dir`], which opens each from the
/// handle of the directory that holds it by its fixed name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The product's data directory (C3.5): the store directory `store` and
    /// the roots directory `roots`, side by side in it.
    DataDirectory,
    /// A scratch run's store directory (C4 item 9): the store in the
    /// directory itself, and its roots directory `roots` inside it.
    ScratchStore,
}

/// What a delete says of the store's root. `Destroyed`: this call, or an
/// earlier call on the same handle, destroyed that root's file, and the last
/// reading of the roots directory shows no root file of it. A root file is
/// destroyed only when its zero-fill went through its kept handle with the
/// zero-fill's full flush answered (ruling ST-50 Addition 1), and the root
/// reads destroyed only when the flush of the roots directory after the
/// step's removals answered too (ruling PR-30 PR9 (3)). With the identity
/// of the root file that the open or the create read, the file of that
/// identity wherever it lies in the roots directory; a name removed with no
/// write through it, because the name at which the file stood lets no write,
/// is no destruction (ruling ST-48 U2). By name alone, the file at the root's own name (a regular file
/// that is not empty and, off Windows, of the root file's length; an all-zero
/// file of that length counts; ruling ST-45 H4). `Left`: a root may stand,
/// the one named when the call knows its id. `Unnamed`: nothing that the call
/// could read names a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootReading {
    Destroyed,
    Left(Option<RootId>),
    Unnamed,
}

impl RootReading {
    /// The reading as the delete's line prints it: yes, no or unknown.
    pub fn as_text(&self) -> &'static str {
        match self {
            RootReading::Destroyed => "yes",
            RootReading::Left(_) => "no",
            RootReading::Unnamed => "unknown",
        }
    }
}

/// What a delete-all, or the delete tail on a directory, did: each step was
/// attempted. `keyed_segments` is the count of keyed segments read before the
/// root step, `None` when it could not be read. `root` reads the store's own
/// root. `roots` names the root files and partial root files that stay in the
/// roots directory after the root step, and counts by kind, naming none, the
/// entries that stay at any other name (ruling ST-48 U3); `root_not_handled`
/// names the root of a key file that
/// this build cannot destroy (a key file of another version, or of a custody
/// that keeps no root file under the roots directory). `not_followed` counts
/// the store names, the store directory's own name among them, at which the
/// delete met a link, an entry of another kind or a directory that was not
/// the handle's, and the data directory given to the tail when it is a link
/// in any spelling, which the tail does not follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    pub keyed_segments: Option<u64>,
    pub root: RootReading,
    pub roots: RootsLeft,
    pub root_not_handled: Option<RootId>,
    pub key_file_gone: bool,
    pub database_left: bool,
    pub files_left: u64,
    pub not_followed: u64,
}

pub(in crate::store) use crate::custody::file_root::RootsLeft;

impl DeleteReport {
    /// The store's root reads destroyed.
    pub fn root_destroyed(&self) -> bool {
        self.root == RootReading::Destroyed
    }

    /// The root reads destroyed or unnamed, the roots directory was listed and
    /// no entry that the step counted stays in it at any name, that directory
    /// still stands at its path, and the delete left nothing of the store: the
    /// report of an `Ok`. A root that reads left never gives `Ok`.
    pub(in crate::store) fn complete(&self) -> bool {
        matches!(self.root, RootReading::Destroyed | RootReading::Unnamed)
            && self.roots.none_left()
            && self.roots.at_its_path
            && self.root_not_handled.is_none()
            && self.key_file_gone
            && !self.database_left
            && self.files_left == 0
    }
}

/// Why a store call failed. A custody failure, also one of `open`'s unwrap,
/// arrives as [`StoreError::Custody`] with its own variant. No Display or
/// Debug form holds a path, a file name of the account, a segment id or a
/// text that another library built.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store i/o failed: {0}")]
    Io(std::io::ErrorKind),
    /// SQLite returned this extended result code.
    #[error("the store database failed with SQLite code {0}")]
    Db(i32),
    /// Sealing a segment or a segment key failed: the random source or the
    /// cipher refused.
    #[error("sealing a segment or a segment key failed")]
    Seal,
    /// The write-ahead log checkpoint was busy or left bytes in the log.
    #[error("the checkpoint left the write-ahead log busy ({busy}) or holding {wal_bytes} bytes")]
    CheckpointIncomplete { busy: i64, wal_bytes: u64 },
    /// No store-key file is in the directory,
    /// the handle was sealed by delete-all, or the segment was shredded.
    #[error("the store is sealed; its key is not available")]
    Sealed,
    /// Damage: data that does not decode or authenticate.
    #[error("the store holds corrupt or tampered data: {0}")]
    Corrupt(String),
    /// At a store name stands something that is not the store's own entry, or
    /// the store directory does not stand at its name. The store looked
    /// behind nothing.
    #[error("the store is obstructed: at {name} stands {met}")]
    Obstructed { name: StoreName, met: Met },
    #[error("another writer holds the store")]
    Locked,
    #[error("a store already exists")]
    AlreadyExists,
    /// store.key is empty, zeroed or does not parse.
    #[error("the store-key file does not parse")]
    KeyFileUnparseable,
    /// The key file's namespace is a valid label other than the one asked for.
    #[error("the key file holds a key of another label")]
    KeyFileOfAnotherLabel,
    /// store.db is missing or empty: `open` makes no database in its place.
    #[error("the store database is missing or empty")]
    DatabaseMissing,
    /// The store key that store.key unwraps to does not open store.db: a key
    /// of another store, a damaged page or a cut file.
    #[error("the store key does not open the store database")]
    DatabaseUnreadable,
    /// store.key or store.db has a format version above the one this build
    /// reads.
    #[error(
        "the store has format version {found}, above version {supported} that this build reads"
    )]
    NewerStore { found: i64, supported: i64 },
    /// store.key or store.db has a format version below the one this build
    /// reads, among them a key file of version 1, which an older build wrote.
    /// No migration exists; the delete tail of
    /// [`db::EncryptedStore::delete_data_dir`] removes such a store.
    #[error(
        "the store has format version {found}, below version {supported} that this build reads"
    )]
    OlderStore { found: i64, supported: i64 },
    /// A segment whose key is live has no file.
    #[error("a segment whose key is live has no file")]
    SegmentMissing,
    /// A setting that the store requires does not read back.
    #[error("the store database does not have {0} in effect")]
    SettingNotInEffect(&'static str),
    /// A delete-all ran every step and left part of the store; the report
    /// says what.
    #[error("delete-all left part of the store")]
    DeleteIncomplete(Box<DeleteReport>),
    #[error(transparent)]
    Custody(#[from] CustodyError),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// Run the `--verify-store` measurements on a fresh store in the directory
/// that HALVO_STORE_DIR names, or in a temporary one, with the marker of
/// HALVO_GATE_MARKER, and return the line that the gate judges.
pub fn verify_store() -> StoreResult<String> {
    let dir = verify::StoreDir::from_env();
    measure_store(dir.path(), &verify::marker_from_env())
}

/// Run the `--verify-deletion` measurements in the directory that
/// HALVO_STORE_DIR names, or in a temporary one, and return the line that the
/// gate judges.
pub fn verify_deletion() -> StoreResult<String> {
    let dir = verify::StoreDir::from_env();
    measure_deletion(dir.path())
}

/// Run the `--verify-store` measurements on a fresh store made in `dir` that
/// holds `marker`, and return the line that the gate judges. It reads neither
/// HALVO_STORE_DIR nor HALVO_GATE_MARKER and uses no default marker.
/// Fails when a file of the store holds the raw store key.
pub fn measure_store(dir: &Path, marker: &str) -> StoreResult<String> {
    verify::run_store_check_in(dir, marker).map(|check| check.to_string())
}

/// Run the `--verify-deletion` measurements on a fresh store made in `dir`:
/// one expired segment shredded beside a live one, then delete-all. Returns
/// the line that the gate judges. It reads neither HALVO_STORE_DIR nor
/// HALVO_GATE_MARKER. Fails when the state of the root after delete-all is
/// unknown.
pub fn measure_deletion(dir: &Path) -> StoreResult<String> {
    verify::run_deletion_check_in(dir).map(|check| check.to_string())
}
