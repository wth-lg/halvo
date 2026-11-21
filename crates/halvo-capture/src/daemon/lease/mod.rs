//! The lease of contracts.md C14 item 3, the run condition that reads it, and
//! the lifeline of C14 item 2.

#[cfg(all(target_os = "macos", debug_assertions))]
pub mod lifeline;
#[cfg(all(test, target_os = "macos"))]
mod process_tests;
#[cfg(test)]
mod tests;

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
use std::time::Duration;

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
use super::slot::Slot;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
use super::state::Desired;

/// The indicator lease of a service run: a deadline on the supervisor's
/// monotonic clock, `INDICATOR_LEASE_MS` after the renewal that set it, or
/// none before the first claim. Outside a test build nothing renews it (the
/// renewal is daemon-3e's), so a service run of this build starts no source.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lease {
    deadline: Option<Duration>,
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
impl Lease {
    /// The lease of a run that no claim has renewed.
    pub const NONE: Lease = Lease { deadline: None };

    /// Whether the lease is fresh at `now`: before its deadline.
    pub fn is_fresh(self, now: Duration) -> bool {
        self.deadline.is_some_and(|deadline| now < deadline)
    }

    #[cfg(test)]
    pub(crate) fn renewed_at(now: Duration) -> Lease {
        Lease {
            deadline: Some(now + Duration::from_millis(halvo_control::INDICATOR_LEASE_MS)),
        }
    }

    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn never_lapsing() -> Lease {
        Lease {
            deadline: Some(Duration::MAX),
        }
    }
}

/// The run condition of contracts.md C3.2 step 10 and C14 item 3: desired
/// running, the slot Open and the lease fresh at `now`.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub fn holds(desired: Desired, slot: &Slot, lease: Lease, now: Duration) -> bool {
    desired == Desired::Running && slot.is_open() && lease.is_fresh(now)
}

/// What ends a run with its parent: on unix the lifeline, whose read end
/// reached its end of file or whose parent is no longer the one of its start.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub trait ParentWatch: Send {
    fn ended(&self) -> bool;
}
