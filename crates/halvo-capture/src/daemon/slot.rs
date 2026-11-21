//! The slot of the store: what the owner holds of it and why, one state of
//! contracts.md C11.3 for each result of an open.

use std::time::Duration;

use halvo_control::proto::{StoreErrorKind, StoreState};

use super::port::{OpenFault, StoreId};

/// What the owner holds of the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// The open has not answered yet.
    Opening,
    /// The store is open for capture.
    Open { id: StoreId },
    /// The store is open for maintenance alone; with no id, no store stands
    /// and the build makes none (contracts.md C20.3).
    Maintenance { id: Option<StoreId> },
    /// The store does not open until a delete-all, and no timer tries it
    /// again.
    Sealed {
        id: Option<StoreId>,
        cause: StoreErrorKind,
    },
    /// The store did not open for `cause`; with `retry_in`, the open is tried
    /// again after that wait.
    Unavailable {
        cause: StoreErrorKind,
        id: Option<StoreId>,
        retry_in: Option<Duration>,
    },
    /// A store of a format before the one that this build reads.
    OlderFormat,
    /// A store of a format after the one that this build reads.
    NewerFormat,
}

impl Slot {
    /// Whether the store is open for capture. Every other slot, and every
    /// slot that a later cut adds, answers false, so that the run condition
    /// starts no source on it.
    pub fn is_open(&self) -> bool {
        matches!(self, Slot::Open { .. })
    }

    /// The slot of an open that answered `fault` for the store `id`, tried
    /// again after `retry` on the rows of contracts.md C11.3 that a timer
    /// tries again: locked, io, no_space, obstructed and custody_locked.
    pub fn of_fault(fault: OpenFault, id: Option<StoreId>, retry: Duration) -> Slot {
        use StoreErrorKind as Cause;
        let unavailable = |cause, timer: bool| Slot::Unavailable {
            cause,
            id,
            retry_in: timer.then_some(retry),
        };
        let sealed = |cause| Slot::Sealed { id, cause };
        match fault {
            OpenFault::Locked => unavailable(Cause::Locked, true),
            OpenFault::Io => unavailable(Cause::Io, true),
            OpenFault::NoSpace => unavailable(Cause::NoSpace, true),
            OpenFault::Obstructed => unavailable(Cause::Obstructed, true),
            OpenFault::CustodyLocked => unavailable(Cause::CustodyLocked, true),
            OpenFault::RootDenied => unavailable(Cause::RootDenied, false),
            OpenFault::IoNotRetried => unavailable(Cause::Io, false),
            OpenFault::RootMissing => sealed(Cause::RootMissing),
            OpenFault::RootCorrupt => sealed(Cause::RootCorrupt),
            OpenFault::WrapAuth => sealed(Cause::WrapAuth),
            OpenFault::KeyFile => Slot::Sealed {
                id: None,
                cause: Cause::KeyFile,
            },
            OpenFault::CustodyKind => sealed(Cause::CustodyKind),
            OpenFault::Database => sealed(Cause::Database),
            OpenFault::OlderStore => Slot::OlderFormat,
            OpenFault::NewerStore => Slot::NewerFormat,
        }
    }

    /// The `store=` state of the status (contracts.md C11.3).
    pub fn state(&self) -> StoreState {
        match self {
            Slot::Opening => StoreState::Opening,
            Slot::Open { .. } => StoreState::Open,
            Slot::Maintenance { .. } => StoreState::NoReleaseCustody,
            Slot::Sealed { .. } => StoreState::Sealed,
            Slot::Unavailable { .. } => StoreState::Unavailable,
            Slot::OlderFormat | Slot::NewerFormat => StoreState::UnsupportedVersion,
        }
    }

    /// The `store_error=` cause of the status, none for a slot that has none.
    pub fn cause(&self) -> Option<StoreErrorKind> {
        match self {
            Slot::Opening | Slot::Open { .. } | Slot::Maintenance { .. } => None,
            Slot::Sealed { cause, .. } | Slot::Unavailable { cause, .. } => Some(*cause),
            Slot::OlderFormat => Some(StoreErrorKind::Older),
            Slot::NewerFormat => Some(StoreErrorKind::Newer),
        }
    }

    /// The wait before the next try of the open; none for a slot that no
    /// timer tries again.
    pub fn retry_in(&self) -> Option<Duration> {
        match self {
            Slot::Unavailable { retry_in, .. } => *retry_in,
            Slot::Opening
            | Slot::Open { .. }
            | Slot::Maintenance { .. }
            | Slot::Sealed { .. }
            | Slot::OlderFormat
            | Slot::NewerFormat => None,
        }
    }
}
