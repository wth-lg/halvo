#![deny(clippy::undocumented_unsafe_blocks)]

pub mod apps;
pub mod audio;
pub mod foreground;
pub mod input;
pub mod mic;
pub mod run_loop;
pub mod screen;
pub(crate) mod secure_input_byte;
pub mod window_title;

#[cfg(test)]
use crate::event::Modality;
use crate::source::{CaptureError, HealthReason, SourceState};

pub(crate) fn source_failure(error: CaptureError) -> SourceState {
    let reason = match &error {
        CaptureError::PermissionDenied(_) => HealthReason::Permission,
        CaptureError::Timeout { .. } => HealthReason::Timeout,
        CaptureError::ThreadEnded => HealthReason::ThreadEnded,
        CaptureError::NotImplemented(_) => HealthReason::NotImplemented,
        CaptureError::Os { .. } | CaptureError::Start(_) | CaptureError::AlreadyStarted => {
            HealthReason::OsError
        }
    };
    SourceState::Failed { reason, error }
}

#[cfg(debug_assertions)]
pub(crate) use timed::timed_run;

#[cfg(debug_assertions)]
mod timed {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use super::foreground::FOREGROUND_POLL;
    use super::{foreground, input, run_loop, screen, secure_input_byte};
    use crate::daemon::{DaemonError, FailedSource, RunOutcome, RunSources, SourceName, TimedRun};
    use crate::source::{CaptureSource, SourceContext, SourceCounters};

    /// Runs the gate's timed run on the process main thread, into the port that
    /// keeps nothing for the store directory of `run`.
    pub(crate) fn timed_run(run: &TimedRun) -> Result<RunOutcome, DaemonError> {
        let mut sources = MacSources::claim()?;
        crate::daemon::run_timed(
            run,
            &mut sources,
            crate::daemon::discard_port::DiscardPort::new(run.store_dir.clone()),
            crate::daemon::maintenance::SystemClocks::new(),
            crate::daemon::signal::flag(),
            crate::daemon::SETTLE_BOUND,
        )
    }

    /// The sources of a timed run on macOS, on the main run loop that the run
    /// waits in.
    struct MacSources {
        main_loop: run_loop::MainRunLoop,
        foreground_state: Option<(foreground::ForegroundWriter, foreground::ForegroundReader)>,
        input_counters: input::InputCounters,
        started: Vec<(SourceName, Box<dyn CaptureSource>)>,
        hold_refusal: Option<DaemonError>,
        input: Option<input::InputSource>,
    }

    impl MacSources {
        fn claim() -> Result<MacSources, DaemonError> {
            let off_the_main_thread =
                || DaemonError::NotImplemented("a second timed run, or one off the main thread,");
            let mut main_loop =
                run_loop::MainRunLoop::claim().map_err(|_| off_the_main_thread())?;
            let foreground_state = Some(
                main_loop
                    .foreground_state()
                    .ok_or_else(off_the_main_thread)?,
            );
            Ok(MacSources {
                main_loop,
                foreground_state,
                input_counters: input::InputCounters::new(),
                started: Vec::new(),
                hold_refusal: None,
                input: None,
            })
        }
    }

    impl RunSources for MacSources {
        fn start(
            &mut self,
            ctx: &SourceContext,
            wanted: &[SourceName],
            hold_after: Option<Duration>,
        ) -> Vec<FailedSource> {
            let Some((foreground_writer, foreground_reader)) = self.foreground_state.take() else {
                return Vec::new();
            };
            let mut failed = Vec::new();
            let mut foreground_writer = Some(foreground_writer);
            for name in wanted {
                let mut source: Box<dyn CaptureSource> = match name {
                    SourceName::Foreground => match foreground_writer.take() {
                        Some(writer) => Box::new(foreground::ForegroundSource::new(
                            FOREGROUND_POLL,
                            writer,
                            self.main_loop.turns(),
                        )),
                        None => continue,
                    },
                    SourceName::Screen => Box::new(screen::ScreenSource::new(
                        screen::SCREEN_FRAME_CAP,
                        foreground_reader.clone(),
                    )),
                    SourceName::Input => {
                        let plan =
                            hold_after.map_or(input::HoldPlan::Never, input::HoldPlan::After);
                        let mut input = input::InputSource::new(
                            foreground_reader.clone(),
                            self.input_counters.clone(),
                            plan,
                        );
                        match input.begin(ctx.clone()) {
                            Ok(_) => self.input = Some(input),
                            Err(error) => failed.push(FailedSource {
                                name: name.as_str(),
                                error: error.to_string(),
                            }),
                        }
                        continue;
                    }
                };
                match source.begin(ctx.clone()) {
                    Ok(_) => self.started.push((*name, source)),
                    Err(error) => failed.push(FailedSource {
                        name: name.as_str(),
                        error: error.to_string(),
                    }),
                }
            }
            failed
        }

        fn wait(&mut self, window: Duration, stop: &AtomicBool) -> bool {
            let slice = Duration::from_millis(100);
            let until = std::time::Instant::now() + window;
            loop {
                if stop.load(std::sync::atomic::Ordering::SeqCst) {
                    return true;
                }
                let left = until.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return false;
                }
                self.main_loop.turn_for(left.min(slice));
            }
        }

        fn stop(&mut self) -> Vec<FailedSource> {
            let mut failed = Vec::new();
            if let Some(mut input) = self.input.take() {
                failed.extend(crate::daemon::stop_failure(
                    SourceName::Input,
                    input.shutdown(),
                ));
                self.hold_refusal = secure_input_byte::refusal_or_none(input.hold_refusal()).err();
            }
            for (name, mut source) in self.started.drain(..) {
                failed.extend(crate::daemon::stop_failure(name, source.shutdown()));
            }
            failed
        }

        fn figures(&self) -> SourceCounters {
            self.input_counters.figures()
        }

        fn hold_refusal(&self) -> Option<DaemonError> {
            match &self.hold_refusal {
                Some(DaemonError::SecureInputRefused(status)) => {
                    Some(DaemonError::SecureInputRefused(*status))
                }
                _ => None,
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod off_source_tests {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    use crate::sink;
    use crate::source::{CaptureError, CaptureSource, SourceContext, SourceState, Started};

    pub(crate) fn the_off_source_keeps_its_lifecycle(mut source: impl CaptureSource) {
        let (handle, writer) = sink::channel(8);
        let first = source.begin(SourceContext::new(handle.clone()));
        let second = source.begin(SourceContext::new(handle.clone()));
        let called = Instant::now();
        let shut = source.shutdown();
        let took = called.elapsed();
        let again = source.begin(SourceContext::new(handle.clone()));
        let state = source.health().state;
        drop(handle);
        let received = writer
            .recv_timeout(Duration::from_secs(2))
            .map(|received| received.event().modality);
        assert_eq!(first, Ok(Started::Off), "the first begin");
        assert_eq!(second, Err(CaptureError::AlreadyStarted), "a second begin");
        assert!(
            shut.is_ok() && took < Duration::from_secs(1),
            "shutdown {shut:?} after {took:?}"
        );
        assert_eq!(again, Ok(Started::Off), "a begin after the shutdown");
        assert_eq!(state, SourceState::Off, "the health state");
        assert_eq!(
            received,
            Err(RecvTimeoutError::Disconnected),
            "the off source emitted an event or kept a handle of the sink"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_of_a_source_reads_its_own_health_reason() {
        let reason_of = |error| match source_failure(error) {
            SourceState::Failed { reason, .. } => reason,
            other => panic!("source_failure answered {other:?}"),
        };
        assert_eq!(
            reason_of(CaptureError::Timeout {
                stage: "input_tap_join",
                ms: 1000
            }),
            HealthReason::Timeout
        );
        assert_eq!(
            reason_of(CaptureError::ThreadEnded),
            HealthReason::ThreadEnded
        );
        assert_eq!(
            reason_of(CaptureError::PermissionDenied(Modality::Keystroke)),
            HealthReason::Permission
        );
        assert_eq!(
            reason_of(CaptureError::Os {
                stage: "stop_capture",
                domain: "ScreenCaptureKit",
                code: -3808
            }),
            HealthReason::OsError
        );
    }
}
