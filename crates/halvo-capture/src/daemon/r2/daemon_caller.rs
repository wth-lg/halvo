//! The daemon's own caller of the owner slot and of the worker slot (contracts.md C24.4 and C24.5 of the workflows
//! run), the one source of the worker's driver, and the cell in which the cursor leaves the figures of its reads.
//!
//! - The cell ([`ReadCell`], [`cell`]). No thread but the store owner's reads the capture store. The cursor leaves
//!   the figures of every answer that it reads there: the store's id and its watermark, and from an answer to a read
//!   from ordinal 0 the lowest live ordinal, which is the ordinal of the first live segment, or the watermark's
//!   segment where no segment lives. An answer to any other read names no lowest live ordinal: the one that stands
//!   for the same store stays, which can be too low and never too high; figures of another store are dropped and a
//!   read is asked; and where no figures stand, such an answer leaves none and asks nothing. The cell also holds the
//!   word that a read is asked: the next pass of the store owner that took no event and answered no message of the
//!   cursor reads the store once, from ordinal 0, on the owner's thread. A read that fails leaves the figures as they
//!   were. The process has one cell, which its one cursor holds.
//! - The key. [`Caller::key`] calls `DataDirStore::derived_store_key` anew at each call, so the daemon holds no key
//!   of the derived store between two starts of the owner. A fault answers none and writes one line, which names the
//!   fault and holds no byte of a key or of a key file:
//!
//!   ```text
//!   halvo-capture: no key of the derived store: <the fault>
//!   ```
//!
//! - The delete of everything. [`Caller::deleting`] is true while `delete-all.intent` stands in the data directory,
//!   read at each call. `key` reads the intent first and answers none while it stands, with no line, so that no ask
//!   makes `derived/master.key` anew after the intent is written. While the intent stands the caller forgets the
//!   figures and the ask of the cell and its committed position: they name the stores that the delete removes, and
//!   a report frame changes nothing.
//! - The committed position. [`Caller::report`] keeps the key `position` of the newest report frame of the owner:
//!   the store that the derived store is bound to and the position up to which batches are committed, or none for
//!   its null. A frame that is no report with that key changes nothing.
//! - No owner before the first read. [`Caller::first_live`] answers the lowest live ordinal of the newest read, and
//!   [`Caller::read_stands`] says whether the cell holds figures; where it does not, it asks a read and the owner
//!   slot starts no owner. So the purge at an owner's start gets a figure that a read of the store gave. Where the
//!   cell lost its figures between the two calls of one start, `first_live` answers 0, below which nothing is purged.
//! - The source. [`DaemonCaller::reading`] gives the worker's driver what one yes carries: the capture store's id
//!   with the committed position, and the watermark, each as it stands at the call. Before the first read it answers
//!   none. It asks a read at its first call and then when 60 s have passed since its last ask, and reads no store.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use halvo_control::paths::INTENT_FILE;
use halvo_schema::{Position, StoreId};

use crate::custody::StoreKey;
use crate::daemon::port::DataDirStore;
use crate::daemon::r2::cursor::cursor;
use crate::daemon::r2::owner_slot::Caller;
use crate::daemon::r2::worker_slot::{Committed, Origin};

/// How long the source waits after an ask before it asks the next read.
pub const ASK_EVERY: Duration = Duration::from_secs(60);

/// The figures of the newest read of the capture store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figures {
    /// The id of the capture store.
    pub store: StoreId,
    /// The store's watermark: the position after its highest ordinal.
    pub watermark: Position,
    /// The lowest live ordinal, as the newest read from ordinal 0 of this store named it: the ordinal of its first
    /// live segment, or the watermark's segment where no segment lived.
    pub first_live: u64,
}

struct Kept {
    figures: Option<Figures>,
    asked: bool,
}

/// The cell between the cursor, which writes the figures of each read, and the daemon's caller, which reads them
/// and asks for a read.
pub struct ReadCell(Mutex<Kept>);

impl ReadCell {
    /// A cell with no figures and no ask.
    pub(in crate::daemon) const fn new() -> ReadCell {
        ReadCell(Mutex::new(Kept {
            figures: None,
            asked: false,
        }))
    }

    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps the figures of one answer of the store, as the cursor does at each read: `first_live` is the lowest
    /// live ordinal where the answer names it, and none for an answer to a read that did not ask from ordinal 0.
    /// With none the lowest live ordinal that stands for `store` stays; figures of another store are dropped and a
    /// read is asked; and where no figures stand the answer leaves none and asks nothing.
    pub fn keep(&self, store: StoreId, watermark: Position, first_live: Option<u64>) {
        let mut kept = self.kept();
        let standing = kept
            .figures
            .as_ref()
            .map(|figures| (figures.store == store, figures.first_live));
        let first_live = match (first_live, standing) {
            (Some(first_live), _) | (None, Some((true, first_live))) => first_live,
            (None, Some((false, _))) => {
                kept.figures = None;
                kept.asked = true;
                return;
            }
            (None, None) => return,
        };
        kept.figures = Some(Figures {
            store,
            watermark,
            first_live,
        });
    }

    /// The figures of the newest read, or none where no read stands.
    pub fn figures(&self) -> Option<Figures> {
        self.kept().figures.clone()
    }

    /// Whether a read is asked and no pass of the store owner has taken the ask.
    pub fn asked(&self) -> bool {
        self.kept().asked
    }

    /// Asks for one read of the store from ordinal 0.
    pub(in crate::daemon) fn ask(&self) {
        self.kept().asked = true;
    }

    /// Takes the ask: true when a read was asked.
    pub(in crate::daemon) fn take_ask(&self) -> bool {
        std::mem::take(&mut self.kept().asked)
    }

    /// Forgets the figures and the ask.
    fn forget(&self) {
        let mut kept = self.kept();
        kept.figures = None;
        kept.asked = false;
    }
}

/// The one cell of the process, which its one cursor holds. In the crate's own tests it is the cell of the calling
/// thread's cursor.
pub fn cell() -> &'static ReadCell {
    cursor().cell()
}

/// What one yes of the worker's driver carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// The id of the capture store, from the newest read, and the committed position of the newest report frame.
    pub origin: Origin,
    /// The watermark of the capture store, from the newest read.
    pub watermark: Position,
}

/// The daemon's caller of the two slots over one data directory.
pub struct DaemonCaller {
    store: DataDirStore,
    intent: PathBuf,
    committed: Mutex<Option<Committed>>,
    /// The clock's reading at the source's last ask.
    asked_at: Mutex<Option<Instant>>,
    log: Mutex<Box<dyn Write + Send>>,
}

impl DaemonCaller {
    /// The caller over `data_dir`, with `log` as the writer of its lines. It reads nothing here.
    pub fn new(data_dir: &Path, log: Box<dyn Write + Send>) -> DaemonCaller {
        DaemonCaller {
            store: DataDirStore::at(data_dir),
            intent: data_dir.join(INTENT_FILE),
            committed: Mutex::new(None),
            asked_at: Mutex::new(None),
            log: Mutex::new(log),
        }
    }

    /// What a yes of the driver carries at the clock's reading `now`, or none before the first read. The call asks
    /// a read where it is the first or 60 s have passed since the call that asked the last.
    pub fn reading(&self, now: Instant) -> Option<Reading> {
        {
            let mut asked_at = lock(&self.asked_at);
            if asked_at.is_none_or(|at| now.saturating_duration_since(at) >= ASK_EVERY) {
                *asked_at = Some(now);
                cell().ask();
            }
        }
        let figures = cell().figures()?;
        Some(Reading {
            origin: Origin {
                capture: figures.store,
                committed: lock(&self.committed).clone(),
            },
            watermark: figures.watermark,
        })
    }

    /// Writes one line; a line that cannot be written is dropped.
    fn line(&self, line: &str) {
        let mut log = lock(&self.log);
        let _ = writeln!(log, "{line}");
        let _ = log.flush();
    }
}

impl Caller for DaemonCaller {
    fn key(&self) -> Option<StoreKey> {
        if self.deleting() {
            return None;
        }
        match self.store.derived_store_key() {
            Ok(key) => Some(key),
            Err(fault) => {
                self.line(&format!(
                    "halvo-capture: no key of the derived store: {fault}"
                ));
                None
            }
        }
    }

    fn first_live(&self) -> u64 {
        cell().figures().map_or(0, |figures| figures.first_live)
    }

    fn deleting(&self) -> bool {
        let stands = fs::symlink_metadata(&self.intent).is_ok();
        if stands {
            cell().forget();
            *lock(&self.committed) = None;
        }
        stands
    }

    fn report(&self, frame: &[u8]) {
        let Some(position) = position_of(frame) else {
            return;
        };
        if !self.deleting() {
            *lock(&self.committed) = position;
        }
    }

    fn read_stands(&self) -> bool {
        let stands = cell().figures().is_some();
        if !stands {
            cell().ask();
        }
        stands
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The key `position` of a report frame (contracts.md C24.9): the committed position, none for its null. No answer
/// for a frame that is no report with that key, or whose position is not a store id with two numbers.
fn position_of(frame: &[u8]) -> Option<Option<Committed>> {
    let frame: serde_json::Value = serde_json::from_slice(frame).ok()?;
    let position = frame.get("report")?.get("position")?;
    if position.is_null() {
        return Some(None);
    }
    let store = StoreId::parse(position.get("store")?.as_str()?).ok()?;
    let seg = position.get("seg")?.as_u64()?;
    let idx = position.get("idx")?.as_u64()?;
    Some(Some(Committed {
        store,
        position: Position { seg, idx },
    }))
}
