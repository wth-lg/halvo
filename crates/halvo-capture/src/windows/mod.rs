//! Windows capture backend root. Phase P4 adds the per-modality sources (screen,
//! input, foreground, window title, audio, apps) as sibling files and fills
//! [`timed_run`]. This phase provides only the daemon entry point so the
//! cross-platform dispatch in [`crate::daemon`] compiles and reports clearly.

use crate::daemon::{DaemonError, TimedRunReport};

/// Run the Windows sources for a bounded window and report per-modality counts.
pub fn timed_run(seconds: u64) -> Result<TimedRunReport, DaemonError> {
    let _ = seconds;
    Err(DaemonError::NotImplemented("Windows capture"))
}
