//! The lease of contracts.md C14 item 3, the run condition that reads it, and
//! the lifeline of C14 item 2.

#[cfg(any(target_os = "macos", windows))]
pub mod lifeline;
#[cfg(all(test, target_os = "macos"))]
mod process_tests;
#[cfg(test)]
mod tests;

#[cfg(any(test, target_os = "macos", windows))]
use std::time::Duration;

#[cfg(any(test, target_os = "macos", windows))]
use super::slot::Slot;
#[cfg(any(test, target_os = "macos", windows))]
use super::state::Desired;

/// The indicator lease of a service run: a deadline on the supervisor's
/// monotonic clock, `INDICATOR_LEASE_MS` after the renewal that set it, or
/// none before the first claim. A client's Status with `indicator: true`
/// renews it (contracts.md C14 item 3).
#[cfg(any(test, target_os = "macos", windows))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lease {
    deadline: Option<Duration>,
}

#[cfg(any(test, target_os = "macos", windows))]
impl Lease {
    /// The lease of a run that no claim has renewed.
    pub const NONE: Lease = Lease { deadline: None };

    /// Whether the lease is fresh at `now`: before its deadline.
    pub fn is_fresh(self, now: Duration) -> bool {
        self.deadline.is_some_and(|deadline| now < deadline)
    }

    /// The lease as Status reports it at `now`: none before the first claim,
    /// fresh before its deadline and lapsed after it.
    pub fn reading(self, now: Duration) -> halvo_control::proto::Lease {
        match self.deadline {
            None => halvo_control::proto::Lease::None,
            Some(_) if self.is_fresh(now) => halvo_control::proto::Lease::Fresh,
            Some(_) => halvo_control::proto::Lease::Lapsed,
        }
    }

    /// The lease renewed at `now`: fresh until `INDICATOR_LEASE_MS` after it.
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
#[cfg(any(test, target_os = "macos", windows))]
pub fn holds(desired: Desired, slot: &Slot, lease: Lease, now: Duration) -> bool {
    desired == Desired::Running && slot.is_open() && lease.is_fresh(now)
}

/// What ends a run with its parent: the lifeline, whose read end reached its
/// end of file or whose parent is no longer the one of its start (on Windows,
/// whose parent's handle is signalled).
#[cfg(any(test, target_os = "macos", windows))]
pub trait ParentWatch: Send {
    fn ended(&self) -> bool;
}
