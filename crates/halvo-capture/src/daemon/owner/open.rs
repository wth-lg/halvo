//! The open path of the owner: one try of the open through the store's host,
//! mapped to a slot, and the thread that runs the tries, so that a call of the
//! store that does not return never stops the main loop (ruling ST-29).

use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{POLL, RETRY_FIRST, RETRY_MAX};
use crate::daemon::custody_policy::{Custody, OpenAs, Selection};
use crate::daemon::port::{
    Access, CaptureStore, MaintainStore, OpenFault, Opened, Probe, StoreHost, StoreId,
};
use crate::daemon::slot::Slot;
use crate::daemon::state::StateHealth;

/// Whether a life may create a store where no key file stands (contracts.md
/// C3.2 step 8, ruling ST-34).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateRule {
    /// The state file loaded, or this life wrote it, and it holds no mark
    /// `store_made`: a first run, or a life after a delete whose clear
    /// succeeded.
    FirstRun,
    /// The state file holds the mark `store_made`: a store was made or opened
    /// here in an earlier life.
    StoreMade,
    /// A state file stands and did not load.
    StateNotLoaded,
}

impl CreateRule {
    /// The rule of a life whose state file reads `health` and holds the mark
    /// `store_made` or not.
    pub fn of(health: StateHealth, store_made: bool) -> CreateRule {
        match health {
            StateHealth::Corrupt | StateHealth::NewerVersion(_) => CreateRule::StateNotLoaded,
            StateHealth::Ok | StateHealth::WriteFailed(_) if store_made => CreateRule::StoreMade,
            StateHealth::Ok | StateHealth::WriteFailed(_) => CreateRule::FirstRun,
        }
    }
}

/// The handle that an open which succeeded hands the owner.
pub enum Held {
    /// The store, open for capture.
    Capture {
        store: Box<dyn CaptureStore + Send>,
        id: StoreId,
    },
    /// The store, open for maintenance alone.
    Maintenance {
        store: Box<dyn MaintainStore + Send>,
        id: StoreId,
    },
}

/// What a try where no key file stands says in one line of the service run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// No key file stands while the store's database or its segments do, so
    /// no store is made over them.
    KeylessStoreStands,
    /// An unfinished create, with no key file and no segment beside the
    /// store's marker create.pending, was made again.
    UnfinishedCreateMadeAgain,
}

impl Note {
    /// The line of the note; it holds no path.
    pub const fn line(self) -> &'static str {
        match self {
            Note::KeylessStoreStands => {
                "halvo-capture: store=unavailable store_error=obstructed: store.key is missing while the store's database or segments stand; no store was made over them"
            }
            Note::UnfinishedCreateMadeAgain => {
                "halvo-capture: an unfinished create of the store stood, with no key file and no segment; the store was made again"
            }
        }
    }
}

/// One try of the open on `host` under the custody policy `select`, with
/// `rule` for a missing key file and `retry` as the wait of a slot that a
/// timer tries again: the slot that the try reads, and the handle when the
/// store opened. Only a first run creates: no key file, no database and no
/// segments stand, and the mark `store_made` is not set. Over a key file, a
/// database or segments that stand nothing is created, so a sealed store is
/// never wiped.
pub fn try_open(
    host: &dyn StoreHost,
    select: fn(Option<Custody>) -> Selection,
    rule: CreateRule,
    retry: Duration,
) -> (Slot, Option<Held>) {
    let (slot, held, _) = try_open_and_say(host, select, rule, retry);
    (slot, held)
}

/// [`try_open`], and the note of a try where no key file stands.
pub fn try_open_and_say(
    host: &dyn StoreHost,
    select: fn(Option<Custody>) -> Selection,
    rule: CreateRule,
    retry: Duration,
) -> (Slot, Option<Held>, Option<Note>) {
    let mut note = None;
    let opened = match host.probe() {
        Probe::Fault { fault, id } => return (Slot::of_fault(fault, id, retry), None, None),
        Probe::KeylessStore => {
            return (
                Slot::of_fault(OpenFault::Obstructed, None, retry),
                None,
                Some(Note::KeylessStoreStands),
            )
        }
        Probe::NoKeyFile | Probe::UnfinishedCreate if rule != CreateRule::FirstRun => {
            return (
                Slot::of_fault(OpenFault::Obstructed, None, retry),
                None,
                None,
            )
        }
        probe @ (Probe::NoKeyFile | Probe::UnfinishedCreate) => match select(None) {
            Selection::Create(Some(custody)) => {
                if probe == Probe::UnfinishedCreate {
                    note = Some(Note::UnfinishedCreateMadeAgain);
                }
                host.create(custody)
            }
            Selection::Create(None) | Selection::Open(_) => {
                return (Slot::Maintenance { id: None }, None, None)
            }
        },
        Probe::KeyFile { custody, id } => match select(Some(custody)) {
            Selection::Open(OpenAs::ReadWrite) => host.open(Access::ReadWrite),
            Selection::Open(OpenAs::MaintenanceOnly) => host.open(Access::MaintenanceOnly),
            Selection::Open(OpenAs::Refused) | Selection::Create(_) => {
                return (
                    Slot::of_fault(OpenFault::CustodyKind, Some(id), retry),
                    None,
                    None,
                )
            }
        },
    };
    match opened {
        Opened::ReadWrite { store, id } => {
            (Slot::Open { id }, Some(Held::Capture { store, id }), note)
        }
        Opened::MaintenanceOnly { store, id } => (
            Slot::Maintenance { id: Some(id) },
            Some(Held::Maintenance { store, id }),
            None,
        ),
        Opened::Fault { fault, id } => (Slot::of_fault(fault, id, retry), None, None),
    }
}

/// The owner's open on a thread of its own. It publishes the slot of each
/// try, hands the handle over once the store opened, and then ends. After a
/// fault it tries again on the slot's timer, from RETRY_FIRST doubled up to
/// RETRY_MAX, or, on a slot that no timer tries again, waits for its stop: it
/// never ends on a fault.
pub struct Opener {
    slot: Arc<Mutex<Slot>>,
    tries: Arc<AtomicU64>,
    held: Receiver<Held>,
    said: Receiver<Note>,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Opener {
    /// Starts the tries of the open on `host` under `select` and `rule`.
    pub fn start(
        host: Box<dyn StoreHost>,
        select: fn(Option<Custody>) -> Selection,
        rule: CreateRule,
    ) -> io::Result<Opener> {
        let slot = Arc::new(Mutex::new(Slot::Opening));
        let tries = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (handed, held) = mpsc::channel();
        let (says, said) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("halvo-store-open".to_owned())
            .spawn({
                let slot = slot.clone();
                let tries = tries.clone();
                let stop = stop.clone();
                move || {
                    let mut retry = RETRY_FIRST;
                    let mut told = Vec::new();
                    loop {
                        let (now, opened, note) =
                            try_open_and_say(host.as_ref(), select, rule, retry);
                        if let Some(why) = note.filter(|why| !told.contains(why)) {
                            told.push(why);
                            let _ = says.send(why);
                        }
                        let wait = now.retry_in();
                        *slot.lock().unwrap_or_else(PoisonError::into_inner) = now;
                        tries.fetch_add(1, Ordering::SeqCst);
                        if let Some(opened) = opened {
                            let _ = handed.send(opened);
                            return;
                        }
                        if !wait_unless_stopped(&stop, wait) {
                            return;
                        }
                        retry = (retry * 2).min(RETRY_MAX);
                    }
                }
            })?;
        Ok(Opener {
            slot,
            tries,
            held,
            said,
            stop,
            thread,
        })
    }

    /// The slot that the last try read.
    pub fn slot(&self) -> Slot {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many tries answered.
    pub fn tries(&self) -> u64 {
        self.tries.load(Ordering::SeqCst)
    }

    /// The handle of an open that succeeded, once.
    pub fn take(&self) -> Option<Held> {
        self.held.try_recv().ok()
    }

    /// Why a try made nothing, once for each cause in the life of the tries.
    pub fn take_note(&self) -> Option<Note> {
        self.said.try_recv().ok()
    }

    /// Whether the tries go on.
    pub fn is_running(&self) -> bool {
        !self.thread.is_finished()
    }

    /// Ends the tries at the next wait; a call of the store that runs is let
    /// go.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Waits at most `bound` for the tries to end after [`Opener::stop`], so
    /// that a call of the store that returns in time is not cut by the
    /// process's exit; true when they ended.
    pub fn wait_for_its_call(&self, bound: Duration) -> bool {
        let started = Instant::now();
        while self.is_running() && started.elapsed() < bound {
            thread::sleep(POLL);
        }
        !self.is_running()
    }
}

impl Drop for Opener {
    fn drop(&mut self) {
        self.stop();
    }
}

fn wait_unless_stopped(stop: &AtomicBool, wait: Option<Duration>) -> bool {
    let started = Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        if wait.is_some_and(|wait| started.elapsed() >= wait) {
            return true;
        }
        thread::sleep(POLL);
    }
}
