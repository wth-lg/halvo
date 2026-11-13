//! Scripted sources for the daemon's tests, over core-1's source contract: a
//! source that emits its script when it begins or fails to begin, and a source
//! set for a timed run whose wait returns at once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::{DaemonError, FailedSource, RunSources, SourceName};
use crate::event::FloorEvent;
use crate::sink::{Emitted, SinkHandle};
use crate::source::{CaptureError, CaptureSource, SourceContext, SourceCounters, Started};

/// A source that emits `script` into the sink when it begins, or answers
/// `refuse` instead, and answers `stop` to its shutdown.
pub(crate) struct ScriptedSource {
    pub(crate) script: Vec<FloorEvent>,
    pub(crate) refuse: Option<CaptureError>,
    pub(crate) stop: Result<(), CaptureError>,
    started: bool,
}

impl ScriptedSource {
    pub(crate) fn emitting(script: Vec<FloorEvent>) -> ScriptedSource {
        ScriptedSource {
            script,
            refuse: None,
            stop: Ok(()),
            started: false,
        }
    }

    pub(crate) fn refusing(error: CaptureError) -> ScriptedSource {
        ScriptedSource {
            refuse: Some(error),
            ..ScriptedSource::emitting(Vec::new())
        }
    }
}

impl CaptureSource for ScriptedSource {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        if self.started {
            return Err(CaptureError::AlreadyStarted);
        }
        if let Some(error) = self.refuse.clone() {
            return Err(error);
        }
        self.started = true;
        for event in self.script.drain(..) {
            if ctx.sink().emit_event(event) == Emitted::Refused {
                return Err(CaptureError::ThreadEnded);
            }
        }
        Ok(Started::Running)
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        self.started = false;
        self.stop.clone()
    }
}

/// The sources of a scripted timed run, by name. The wait records the window,
/// emits `in_window` and, with `signal` set, sets the stop flag and
/// answers that it ended the wait.
pub(crate) struct ScriptedRun {
    pub(crate) sources: Vec<(SourceName, ScriptedSource)>,
    pub(crate) in_window: Vec<FloorEvent>,
    pub(crate) signal: bool,
    pub(crate) figures: SourceCounters,
    pub(crate) refusal: Option<i32>,
    pub(crate) asked: Vec<SourceName>,
    pub(crate) hold_after: Option<Duration>,
    pub(crate) waited: Option<Duration>,
    sink: Option<SinkHandle>,
}

impl ScriptedRun {
    pub(crate) fn new(sources: Vec<(SourceName, ScriptedSource)>) -> ScriptedRun {
        ScriptedRun {
            sources,
            in_window: Vec::new(),
            signal: false,
            figures: SourceCounters::default(),
            refusal: None,
            asked: Vec::new(),
            hold_after: None,
            waited: None,
            sink: None,
        }
    }
}

impl RunSources for ScriptedRun {
    fn start(
        &mut self,
        ctx: &SourceContext,
        wanted: &[SourceName],
        hold_after: Option<Duration>,
    ) -> Vec<FailedSource> {
        self.asked = wanted.to_vec();
        self.hold_after = hold_after;
        self.sink = Some(ctx.sink().clone());
        let mut failed = Vec::new();
        for (name, source) in &mut self.sources {
            if !wanted.contains(name) {
                continue;
            }
            if let Err(error) = source.begin(ctx.clone()) {
                failed.push(FailedSource {
                    name: name.as_str(),
                    error: error.to_string(),
                });
            }
        }
        failed
    }

    fn wait(&mut self, window: Duration, stop: &AtomicBool) -> bool {
        self.waited = Some(window);
        if let Some(sink) = &self.sink {
            for event in self.in_window.drain(..) {
                sink.emit_event(event);
            }
        }
        if self.signal {
            stop.store(true, Ordering::SeqCst);
        }
        stop.load(Ordering::SeqCst)
    }

    fn stop(&mut self) -> Vec<FailedSource> {
        self.sink = None;
        let mut failed = Vec::new();
        for (name, source) in &mut self.sources {
            if !source.started {
                continue;
            }
            failed.extend(super::stop_failure(*name, source.shutdown()));
        }
        failed
    }

    fn figures(&self) -> SourceCounters {
        self.figures
    }

    fn hold_refusal(&self) -> Option<DaemonError> {
        self.refusal.map(DaemonError::SecureInputRefused)
    }
}
