#![deny(clippy::undocumented_unsafe_blocks)]

pub mod apps;
pub mod audio;
pub(crate) mod browser_reads;
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
pub(crate) use timed::{self_check_run_loop, timed_run};

#[cfg(debug_assertions)]
mod timed {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use core_foundation::base::TCFType;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopRef};
    use halvo_control::proto::{Platform, SourceName};

    use super::foreground::{ForegroundReader, ForegroundWriter, FOREGROUND_POLL};
    use super::run_loop::{ClaimError, MainRunLoop, RunLoopTurns};
    use super::{foreground, input, screen, secure_input_byte};
    use crate::daemon::main_loop::{MainLoop, SELF_CHECK_WINDOW};
    use crate::daemon::permissions::ChildProbe;
    use crate::daemon::sources::SourceFactory;
    use crate::daemon::supervisor::{Supervisor, SystemSources};
    use crate::daemon::{children, DaemonError, RunOutcome, RunSources, TimedRun};
    use crate::source::{
        CaptureError, CaptureSource, SourceContext, SourceCounters, SourceHealth, Started,
    };

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRunLoopPerformBlock(
            run_loop: CFRunLoopRef,
            mode: *const c_void,
            block: &block2::Block<dyn Fn()>,
        );
        fn CFRunLoopWakeUp(run_loop: CFRunLoopRef);
    }

    /// The process main thread's run loop as the supervisor's loop.
    pub(crate) struct MacLoop {
        run_loop: MainRunLoop,
        started: Instant,
    }

    impl MainLoop for MacLoop {
        fn turn(&mut self, wait: Duration) {
            self.run_loop.turn_for(wait);
        }

        fn now(&self) -> Duration {
            self.started.elapsed()
        }
    }

    fn claim() -> Result<MainRunLoop, DaemonError> {
        MainRunLoop::claim().map_err(|error| match error {
            ClaimError::NotMainThread => DaemonError::NotMainThread,
            ClaimError::AlreadyClaimed => DaemonError::NotImplemented("a second timed run"),
        })
    }

    pub(crate) fn timed_run(run: &TimedRun) -> Result<RunOutcome, DaemonError> {
        let mut run_loop = claim()?;
        let (writer, reader) = run_loop
            .foreground_state()
            .ok_or(DaemonError::NotImplemented("a second timed run"))?;
        let factory = MacSources {
            foreground_writer: Some(writer),
            foreground_reader: reader,
            turns: run_loop.turns(),
            input_counters: input::InputCounters::new(),
            hold_after: None,
            keys_refused: false,
            refusal: Arc::default(),
        };
        let main_loop = MacLoop {
            run_loop,
            started: Instant::now(),
        };
        let mut supervisor = Supervisor::new(Platform::Macos, factory, main_loop)
            .with_ending(Arc::new(crate::daemon::main_loop::ExitNow));
        if let Ok(dir) = children::own_dir() {
            supervisor = supervisor.with_probe(Box::new(ChildProbe::new(dir, "halvo-capture")));
        }
        let scratch = crate::daemon::scratch_store(run)?;
        let answer = crate::daemon::run_timed(
            run,
            &mut supervisor,
            scratch.store,
            crate::daemon::maintenance::SystemClocks::new(),
            crate::daemon::signal::flag(),
            crate::daemon::SETTLE_BOUND,
        );
        crate::daemon::end_of_a_scratch_run(answer, scratch.root)
    }

    struct NoSources;

    impl SourceFactory for NoSources {
        fn make(&mut self, _name: SourceName) -> Option<Box<dyn CaptureSource>> {
            None
        }
    }

    impl SystemSources for NoSources {}

    /// The time from the queue of a block on the main run loop, by a second
    /// thread, to its run, while the main thread waits as a timed run waits
    /// with no source started; `None` when the block did not run inside the
    /// self-check's window.
    pub(crate) fn self_check_run_loop() -> Result<Option<Duration>, DaemonError> {
        let main_loop = MacLoop {
            run_loop: claim()?,
            started: Instant::now(),
        };
        let mut supervisor = Supervisor::new(Platform::Macos, NoSources, main_loop);
        let ran = Arc::new(AtomicBool::new(false));
        let ran_after: Arc<Mutex<Option<Duration>>> = Arc::default();
        let queue = {
            let ran = ran.clone();
            let ran_after = ran_after.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                let queued = Instant::now();
                let block = block2::RcBlock::new(move || {
                    *ran_after.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(queued.elapsed());
                    ran.store(true, Ordering::SeqCst);
                });
                let main = CFRunLoop::get_main();
                // SAFETY: the main run loop lives as long as the process, the
                // mode is a constant CFString, and the call copies the block.
                unsafe {
                    CFRunLoopPerformBlock(
                        main.as_concrete_TypeRef(),
                        kCFRunLoopCommonModes.cast(),
                        &block,
                    );
                }
                // SAFETY: the main run loop lives as long as the process.
                unsafe { CFRunLoopWakeUp(main.as_concrete_TypeRef()) };
            })
        };
        supervisor.wait(SELF_CHECK_WINDOW, &ran);
        let _ = queue.join();
        let ran_after = *ran_after.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(ran_after)
    }

    struct MacSources {
        foreground_writer: Option<ForegroundWriter>,
        foreground_reader: ForegroundReader,
        turns: RunLoopTurns,
        input_counters: input::InputCounters,
        hold_after: Option<Duration>,
        keys_refused: bool,
        refusal: Arc<Mutex<Option<DaemonError>>>,
    }

    /// The input source, which hands the system's refusal of its hold to the
    /// supervisor when it stops.
    struct InputWithRefusal {
        source: input::InputSource,
        refusal: Arc<Mutex<Option<DaemonError>>>,
    }

    impl CaptureSource for InputWithRefusal {
        fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
            self.source.begin(ctx)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            let answer = self.source.shutdown();
            *self.refusal.lock().unwrap_or_else(PoisonError::into_inner) =
                secure_input_byte::refusal_or_none(self.source.hold_refusal()).err();
            answer
        }

        fn health(&self) -> SourceHealth {
            self.source.health()
        }
    }

    impl SourceFactory for MacSources {
        fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
            match name {
                SourceName::Foreground => {
                    let writer = self.foreground_writer.take()?;
                    Some(Box::new(foreground::ForegroundSource::new(
                        FOREGROUND_POLL,
                        writer,
                        self.turns.clone(),
                    )))
                }
                SourceName::Screen => Some(Box::new(screen::ScreenSource::new(
                    screen::SCREEN_FRAME_CAP,
                    self.foreground_reader.clone(),
                ))),
                SourceName::Input => {
                    let plan = self
                        .hold_after
                        .map_or(input::HoldPlan::Never, input::HoldPlan::After);
                    Some(Box::new(InputWithRefusal {
                        source: input::InputSource::new(
                            self.foreground_reader.clone(),
                            self.input_counters.clone(),
                            plan,
                        )
                        .with_keys_refused(self.keys_refused),
                        refusal: self.refusal.clone(),
                    }))
                }
                SourceName::SystemAudio
                | SourceName::Microphone
                | SourceName::Keyboard
                | SourceName::Pointer
                | SourceName::WindowTitle => None,
            }
        }
    }

    impl SystemSources for MacSources {
        fn plan_hold(&mut self, after: Option<Duration>) {
            self.hold_after = after;
        }

        fn holds_secure_input(&self) -> bool {
            self.hold_after.is_some()
        }

        fn refuse_keys(&mut self, refused: bool) {
            self.keys_refused = refused;
        }

        fn figures(&self) -> SourceCounters {
            self.input_counters.figures()
        }

        fn hold_refusal(&self) -> Option<DaemonError> {
            match &*self.refusal.lock().unwrap_or_else(PoisonError::into_inner) {
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
