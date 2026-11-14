//! The capture daemon: the store owner, which is the one thread that holds the
//! capture store, and the gate's timed run, dispatched to the active platform
//! backend.

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod barrier;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod batch;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod counters;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod discard_port;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod maintenance;
#[cfg(test)]
mod model_store;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod owner;
#[cfg(test)]
mod owner_props;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod port;
mod report;

pub use report::TimedRunReport;

/// Run a bounded capture for the gate's `--timed-run`, dispatched to the active
/// platform backend. The backends start the platform sources, run for
/// `seconds` and report per-modality counts.
pub fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::timed_run(seconds)
    }
    #[cfg(target_os = "windows")]
    {
        crate::windows::timed_run(seconds)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = seconds;
        Err(DaemonError::NotImplemented("timed-run on this platform"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("store error: {0}")]
    Store(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
    #[error("the system refused the secure-input hold (status {0})")]
    SecureInputRefused(i32),
}
