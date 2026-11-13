//! The capture daemon: the always-on process that owns the single writer and
//! appends every captured event to the store. The platform source wiring is
//! filled in as the backends land (phases P3, P4); this phase lays the ownership
//! structure so the writer is the sole appender, and dispatches the gate's timed
//! run to the active platform backend.

use crate::sink::SinkWriter;
use crate::store::SegmentStore;

/// Owns the one [`SinkWriter`] and the sole-writer store. Construct it with
/// [`CaptureDaemon::new`], then [`CaptureDaemon::run`] drains the sink into the
/// store until every source is done.
pub struct CaptureDaemon {
    writer: SinkWriter,
    store: Box<dyn SegmentStore>,
}

impl CaptureDaemon {
    pub fn new(writer: SinkWriter, store: Box<dyn SegmentStore>) -> Self {
        Self { writer, store }
    }

    /// Drain the sink into the store. Returns when every [`crate::sink::SinkHandle`]
    /// has been dropped (the channel is closed).
    pub fn run(mut self) -> Result<(), DaemonError> {
        while let Some(event) = self.writer.recv() {
            self.store
                .append_event(&event)
                .map_err(|e| DaemonError::Store(e.to_string()))?;
        }
        Ok(())
    }
}

/// The result of a bounded timed run: per-modality event counts and the
/// secure-input keystroke count. [`TimedRunReport::render`] prints the exact
/// lines the gate's timed-run and secure-input sub-checks parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimedRunReport {
    pub seconds: u64,
    pub screen_frame: u64,
    pub keystroke: u64,
    pub pointer: u64,
    pub foreground_app: u64,
    pub window_title: u64,
    pub secure_input_keystrokes: u64,
}

impl TimedRunReport {
    /// Render the report in the gate's line format.
    pub fn render(&self) -> String {
        format!(
            "modality=screen_frame count={}\n\
             modality=keystroke count={}\n\
             modality=pointer count={}\n\
             modality=foreground_app count={}\n\
             modality=window_title count={}\n\
             secure_input_keystrokes={}\n",
            self.screen_frame,
            self.keystroke,
            self.pointer,
            self.foreground_app,
            self.window_title,
            self.secure_input_keystrokes,
        )
    }
}

/// Run a bounded capture for the gate's `--timed-run`, dispatched to the active
/// platform backend. The backends (phases P3, P4) start the platform sources,
/// run for `seconds` and report per-modality counts.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_renders_the_gate_lines() {
        let report = TimedRunReport {
            seconds: 600,
            screen_frame: 3,
            keystroke: 2,
            pointer: 5,
            foreground_app: 1,
            window_title: 4,
            secure_input_keystrokes: 0,
        };
        let text = report.render();
        assert!(text.contains("modality=screen_frame count=3"));
        assert!(text.contains("modality=keystroke count=2"));
        assert!(text.contains("modality=foreground_app count=1"));
        assert!(text.contains("secure_input_keystrokes=0"));
    }
}
