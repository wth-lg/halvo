//! The platform-agnostic capture traits. Each modality's OS-specific source
//! implements [`CaptureSource`] and emits events through an [`EventSink`]. Per-OS
//! implementations live under [`crate::macos`] / [`crate::windows`] behind `cfg`;
//! the rest of the app sees only these traits.

use crate::event::{CaptureStreamEvent, Modality};
use crate::sink::{SinkError, SinkHandle};

/// One capture source for one modality. `start` installs the OS callback and
/// returns; the callback does the minimum (stamp, package, emit) and returns, so
/// a slow writer never blocks a capture callback.
///
/// The two pairs reach each other through their default bodies, so a source
/// implements `begin` and `shutdown` or `start` and `stop`. A source that
/// implements neither pair recurses without end.
pub trait CaptureSource {
    fn modality(&self) -> Modality;

    /// The old form of [`CaptureSource::begin`]. The default body begins with a
    /// context that holds `sink` and answers `Ok` for both [`Started`] values.
    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        self.begin(SourceContext::new(sink)).map(|_| ())
    }

    /// The old form of [`CaptureSource::shutdown`]. The default body shuts down
    /// and drops the result; [`CaptureSource::health`] still reports a failure.
    fn stop(&mut self) {
        let _ = self.shutdown();
    }

    /// Install the OS callback and return [`Started::Running`], or install nothing
    /// and return [`Started::Off`] for a source that never captures or whose
    /// switches are all off. A second `begin` on a started source is
    /// [`CaptureError::AlreadyStarted`]. The default body calls `start` with the
    /// context's sink.
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        self.start(ctx.sink).map(|()| Started::Running)
    }

    /// Remove the OS callback and join the source's thread: `Ok` when both are
    /// done, [`CaptureError::ThreadEnded`] when the thread had ended before,
    /// [`CaptureError::Timeout`] when the source's deadline passed. The default
    /// body calls `stop` and answers `Ok`.
    fn shutdown(&mut self) -> Result<(), CaptureError> {
        self.stop();
        Ok(())
    }

    /// The source's state and counters. The default body is
    /// [`SourceState::NotReported`] with every counter at zero.
    fn health(&self) -> SourceHealth {
        SourceHealth::default()
    }
}

/// What a source gets when it begins. Its fields are private, so a field added
/// later changes no source's signature.
#[derive(Clone)]
pub struct SourceContext {
    sink: SinkHandle,
}

impl SourceContext {
    pub fn new(sink: SinkHandle) -> Self {
        Self { sink }
    }

    /// The handle that the source emits through.
    pub fn sink(&self) -> &SinkHandle {
        &self.sink
    }
}

/// What [`CaptureSource::begin`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// The source installed its OS callback.
    Running,
    /// The source never captures, or every switch it reads is off; it installed
    /// nothing.
    Off,
}

/// What [`CaptureSource::health`] reports: a state and the source's counters. A
/// health value holds fixed text and OS codes only, never an event field or a
/// panic message.
#[derive(Debug, Default)]
pub struct SourceHealth {
    pub state: SourceState,
    pub counters: SourceCounters,
}

/// The state of a source.
#[derive(Debug, Default)]
pub enum SourceState {
    /// The source has not reported a state.
    #[default]
    NotReported,
    /// The source's OS callback is installed.
    Running,
    /// The source runs and part of its capture is missing.
    Impaired { reason: HealthReason },
    /// The source does not capture because of `error`.
    Failed {
        reason: HealthReason,
        error: CaptureError,
    },
    /// The source was stopped.
    Stopped(StopCause),
    /// The source never captures, or every switch it reads is off.
    Off,
}

/// Who stopped a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    /// The daemon asked the source to stop.
    Request,
    /// The person stopped the capture from the operating system's control.
    User,
}

/// Why a source is impaired or failed. One variant for each `reason=` token of
/// the status text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthReason {
    Permission,
    NoKeyEvents,
    SecureInputElsewhere,
    TitleReadErrors,
    StreamBlank,
    StreamSuspended,
    ThreadEnded,
    Timeout,
    OsError,
    NotImplemented,
    RunLoopStalled,
}

/// Every figure that the status text and the timed-run report print for a
/// source. Each count starts at zero and each yes or no at no.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceCounters {
    pub tap_disabled_timeout: u64,
    pub tap_disabled_user_input: u64,
    pub late_events: u64,
    pub max_lag_ms: u64,
    /// Key events that reached the tap while secure input was active; each was
    /// suppressed and never stored.
    pub secure_input_keystrokes: u64,
    pub title_read_errors: u64,
    pub pointer_coalesced: u64,
    /// Whether keys were withheld: a yes or no, never a count.
    pub keys_withheld: bool,
    pub secure_released_keystrokes: u64,
    pub secure_held_keystrokes: u64,
    pub secure_held_suppressed: SuppressedKeys,
    pub secure_input_held: HeldSamples,
    pub secure_window_pointer: u64,
    /// Whether the key counter of the system advanced in the held phase: a yes or
    /// no, never a count.
    pub secure_window_keys_typed: bool,
}

/// Key events suppressed in the held phase, by type: counts of events, never a
/// key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SuppressedKeys {
    pub down: u64,
    pub up: u64,
    pub modifier: u64,
}

/// Samples of the held phase that found secure input held, of all samples taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeldSamples {
    pub held: u64,
    pub taken: u64,
}

/// The non-blocking emit path a source uses to hand an event to the single
/// writer. The only implementor is the sink handle (see [`crate::sink`]).
pub trait EventSink {
    /// Enqueue an event for the single writer. Never drops a non-droppable
    /// modality; see [`crate::sink`] for the backpressure policy.
    fn emit(&self, event: CaptureStreamEvent) -> Result<EmitOutcome, SinkError>;
}

/// What happened to an emitted event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitOutcome {
    /// The event was queued for the writer.
    Queued,
    /// A droppable event was dropped because the queue was full.
    Dropped,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("the capture permission for {0:?} is not granted")]
    PermissionDenied(Modality),
    #[error("the capture source failed to start: {0}")]
    Start(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
    #[error("the capture source timed out at {stage} after {ms} ms")]
    Timeout { stage: &'static str, ms: u64 },
    #[error("the capture thread had ended before the source was shut down")]
    ThreadEnded,
    #[error("the capture source is already started")]
    AlreadyStarted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{CapturedAt, MonotonicSequence, RetentionClass, SegmentKeyId, SegmentRef};
    use crate::sink;
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant, UNIX_EPOCH};
    use uuid::Uuid;

    fn context() -> SourceContext {
        let (handle, _writer) = sink::channel(1);
        SourceContext::new(handle)
    }

    #[derive(Default)]
    struct PanickingSource {
        thread: Option<JoinHandle<()>>,
        thread_ended: bool,
    }

    impl PanickingSource {
        fn wait_until_the_thread_ended(&self) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.thread.as_ref().is_some_and(JoinHandle::is_finished) {
                assert!(Instant::now() < deadline, "the thread did not end in 5 s");
                thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl CaptureSource for PanickingSource {
        fn modality(&self) -> Modality {
            Modality::Keystroke
        }

        fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
            if self.thread.is_some() {
                return Err(CaptureError::AlreadyStarted);
            }
            let sink = ctx.sink().clone();
            self.thread = Some(thread::spawn(move || {
                let _sink = sink;
                panic!("scripted capture thread failure");
            }));
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            let Some(thread) = self.thread.take() else {
                return Ok(());
            };
            let ended_before = thread.is_finished();
            if thread.join().is_err() || ended_before {
                self.thread_ended = true;
                return Err(CaptureError::ThreadEnded);
            }
            Ok(())
        }

        fn health(&self) -> SourceHealth {
            let state = if self.thread_ended {
                SourceState::Failed {
                    reason: HealthReason::ThreadEnded,
                    error: CaptureError::ThreadEnded,
                }
            } else if self.thread.is_some() {
                SourceState::Running
            } else {
                SourceState::NotReported
            };
            SourceHealth {
                state,
                counters: SourceCounters::default(),
            }
        }
    }

    #[test]
    fn a_scripted_source_whose_thread_panics_returns_thread_ended_and_reports_failed() {
        let mut source = PanickingSource::default();
        assert!(matches!(source.begin(context()), Ok(Started::Running)));
        assert!(matches!(
            source.begin(context()),
            Err(CaptureError::AlreadyStarted)
        ));
        source.wait_until_the_thread_ended();
        assert!(matches!(source.shutdown(), Err(CaptureError::ThreadEnded)));
        let health = source.health();
        assert!(matches!(
            health.state,
            SourceState::Failed {
                reason: HealthReason::ThreadEnded,
                error: CaptureError::ThreadEnded,
            }
        ));
        let shown = format!("{health:?} {}", CaptureError::ThreadEnded);
        assert!(
            !shown.contains("scripted capture thread failure"),
            "{shown}"
        );

        let mut stopped_the_old_way = PanickingSource::default();
        let (handle, _writer) = sink::channel(1);
        stopped_the_old_way.start(handle).unwrap();
        stopped_the_old_way.wait_until_the_thread_ended();
        stopped_the_old_way.stop();
        assert!(matches!(
            stopped_the_old_way.health().state,
            SourceState::Failed {
                reason: HealthReason::ThreadEnded,
                error: CaptureError::ThreadEnded,
            }
        ));
    }

    struct OffSource;

    impl CaptureSource for OffSource {
        fn modality(&self) -> Modality {
            Modality::SystemAudio
        }

        fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
            Ok(Started::Off)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            Ok(())
        }

        fn health(&self) -> SourceHealth {
            SourceHealth {
                state: SourceState::Off,
                counters: SourceCounters::default(),
            }
        }
    }

    #[test]
    fn an_off_source_answers_off() {
        let mut source = OffSource;
        assert!(matches!(source.begin(context()), Ok(Started::Off)));
        assert!(matches!(source.health().state, SourceState::Off));
        assert!(source.shutdown().is_ok());
        let (handle, _writer) = sink::channel(1);
        assert!(source.start(handle).is_ok());
        source.stop();
        assert!(matches!(source.health().state, SourceState::Off));
    }

    #[derive(Default)]
    struct OldPair {
        calls: Vec<&'static str>,
        sink: Option<SinkHandle>,
    }

    impl CaptureSource for OldPair {
        fn modality(&self) -> Modality {
            Modality::ForegroundApp
        }

        fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
            self.calls.push("start");
            self.sink = Some(sink);
            Ok(())
        }

        fn stop(&mut self) {
            self.calls.push("stop");
            self.sink = None;
        }
    }

    #[derive(Default)]
    struct NewPair {
        calls: Vec<&'static str>,
        refuse: bool,
    }

    impl CaptureSource for NewPair {
        fn modality(&self) -> Modality {
            Modality::Pointer
        }

        fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
            self.calls.push("begin");
            if self.refuse {
                return Err(CaptureError::Start("scripted refusal".to_string()));
            }
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            self.calls.push("shutdown");
            Ok(())
        }
    }

    fn frame() -> CaptureStreamEvent {
        CaptureStreamEvent {
            event_id: Uuid::nil(),
            captured_at: CapturedAt {
                wall_clock: UNIX_EPOCH,
                sequence: MonotonicSequence::new(0),
            },
            modality: Modality::ScreenFrame,
            foreground_app: String::new(),
            segment_ref: SegmentRef {
                segment_id: Uuid::nil(),
                byte_offset: 0,
                byte_len: 0,
            },
            segment_key_id: SegmentKeyId(0),
            is_derived: false,
            retention_class: RetentionClass::Raw,
        }
    }

    #[test]
    fn a_source_that_implements_one_pair_is_reached_through_the_other() {
        let mut old = OldPair::default();
        let (handle, writer) = sink::channel(1);
        assert!(matches!(
            old.begin(SourceContext::new(handle)),
            Ok(Started::Running)
        ));
        let given = old.sink.as_ref().expect("start got the context's sink");
        assert!(matches!(given.emit(frame()), Ok(EmitOutcome::Queued)));
        assert!(writer.try_recv().is_ok());
        assert!(old.shutdown().is_ok());
        assert_eq!(old.calls, ["start", "stop"]);
        assert!(matches!(old.health().state, SourceState::NotReported));

        let mut new = NewPair::default();
        let (handle, _writer) = sink::channel(1);
        assert!(new.start(handle).is_ok());
        new.stop();
        assert_eq!(new.calls, ["begin", "shutdown"]);

        let mut refusing = NewPair {
            refuse: true,
            ..NewPair::default()
        };
        let (handle, _writer) = sink::channel(1);
        assert!(matches!(
            refusing.start(handle),
            Err(CaptureError::Start(text)) if text == "scripted refusal"
        ));
    }

    #[test]
    fn the_counters_default_to_zero_and_no() {
        let zero = SourceCounters {
            tap_disabled_timeout: 0,
            tap_disabled_user_input: 0,
            late_events: 0,
            max_lag_ms: 0,
            secure_input_keystrokes: 0,
            title_read_errors: 0,
            pointer_coalesced: 0,
            keys_withheld: false,
            secure_released_keystrokes: 0,
            secure_held_keystrokes: 0,
            secure_held_suppressed: SuppressedKeys {
                down: 0,
                up: 0,
                modifier: 0,
            },
            secure_input_held: HeldSamples { held: 0, taken: 0 },
            secure_window_pointer: 0,
            secure_window_keys_typed: false,
        };
        assert_eq!(SourceCounters::default(), zero);
        let health = SourceHealth::default();
        assert!(matches!(health.state, SourceState::NotReported));
        assert_eq!(health.counters, zero);
    }
}
