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

/// One try of the open on `host` under the custody policy `select`, with
/// `rule` for a missing key file and `retry` as the wait of a slot that a
/// timer tries again: the slot that the try reads, and the handle when the
/// store opened. Where no key file stands, only a first run creates; over a
/// key file that stands nothing is created, so a sealed store is never wiped.
pub fn try_open(
    host: &dyn StoreHost,
    select: fn(Option<Custody>) -> Selection,
    rule: CreateRule,
    retry: Duration,
) -> (Slot, Option<Held>) {
    let opened = match host.probe() {
        Probe::Fault { fault, id } => return (Slot::of_fault(fault, id, retry), None),
        Probe::NoKeyFile if rule != CreateRule::FirstRun => {
            return (Slot::of_fault(OpenFault::Obstructed, None, retry), None)
        }
        Probe::NoKeyFile => match select(None) {
            Selection::Create(Some(custody)) => host.create(custody),
            Selection::Create(None) | Selection::Open(_) => {
                return (Slot::Maintenance { id: None }, None)
            }
        },
        Probe::KeyFile { custody, id } => match select(Some(custody)) {
            Selection::Open(OpenAs::ReadWrite) => host.open(Access::ReadWrite),
            Selection::Open(OpenAs::MaintenanceOnly) => host.open(Access::MaintenanceOnly),
            Selection::Open(OpenAs::Refused) | Selection::Create(_) => {
                return (Slot::of_fault(OpenFault::CustodyKind, Some(id), retry), None)
            }
        },
    };
    match opened {
        Opened::ReadWrite { store, id } => (Slot::Open { id }, Some(Held::Capture { store, id })),
        Opened::MaintenanceOnly { store, id } => (
            Slot::Maintenance { id: Some(id) },
            Some(Held::Maintenance { store, id }),
        ),
        Opened::Fault { fault, id } => (Slot::of_fault(fault, id, retry), None),
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
        let thread = thread::Builder::new()
            .name("halvo-store-open".to_owned())
            .spawn({
                let slot = slot.clone();
                let tries = tries.clone();
                let stop = stop.clone();
                move || {
                    let mut retry = RETRY_FIRST;
                    loop {
                        let (now, opened) = try_open(host.as_ref(), select, rule, retry);
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

    /// Whether the tries go on.
    pub fn is_running(&self) -> bool {
        !self.thread.is_finished()
    }

    /// Ends the tries at the next wait; a call of the store that runs is let
    /// go.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
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
