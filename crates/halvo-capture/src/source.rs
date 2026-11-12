use std::cell::RefCell;

use crate::event::{CaptureStreamEvent, Modality};
use crate::sink::{SinkError, SinkHandle};

pub trait CaptureSource {
    fn modality(&self) -> Modality {
        panic!("CaptureSource::modality is retired and has no default answer")
    }

    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let _call = DefaultPairCall::enter(&*self);
        self.begin(SourceContext::new(sink)).map(|_| ())
    }

    fn stop(&mut self) {
        let _call = DefaultPairCall::enter(&*self);
        let _ = self.shutdown();
    }

    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let _call = DefaultPairCall::enter(&*self);
        self.start(ctx.sink).map(|()| Started::Running)
    }

    /// Remove the OS callback and join the source's thread: `Ok` when both are
    /// done, [`CaptureError::ThreadEnded`] when the thread had ended before,
    /// [`CaptureError::Timeout`] when the source's deadline passed.
    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let _call = DefaultPairCall::enter(&*self);
        self.stop();
        Ok(())
    }

    fn health(&self) -> SourceHealth {
        SourceHealth::default()
    }
}

thread_local! {
    static DEFAULT_PAIR_CALLS: RefCell<Vec<(usize, &'static str)>> =
        const { RefCell::new(Vec::new()) };
}

struct DefaultPairCall {
    source: (usize, &'static str),
}

impl DefaultPairCall {
    fn enter<S: ?Sized>(source: &S) -> Self {
        let source = (
            (source as *const S).cast::<()>().addr(),
            std::any::type_name::<S>(),
        );
        let again = DEFAULT_PAIR_CALLS.with(|calls| {
            let mut calls = calls.borrow_mut();
            let again = calls.contains(&source);
            if !again {
                calls.push(source);
            }
            again
        });
        if again {
            panic!("CaptureSource implements neither begin and shutdown nor start and stop");
        }
        Self { source }
    }
}

impl Drop for DefaultPairCall {
    fn drop(&mut self) {
        let _ = DEFAULT_PAIR_CALLS.try_with(|calls| {
            let mut calls = calls.borrow_mut();
            if let Some(at) = calls.iter().rposition(|source| *source == self.source) {
                calls.remove(at);
            }
        });
    }
}

#[derive(Clone)]
pub struct SourceContext {
    sink: SinkHandle,
}

impl SourceContext {
    pub fn new(sink: SinkHandle) -> Self {
        Self { sink }
    }

    pub fn sink(&self) -> &SinkHandle {
        &self.sink
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// The source installed its OS callback.
    Running,
    /// The source never captures, or every switch it reads is off; it installed
    /// nothing.
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceHealth {
    pub state: SourceState,
    pub counters: SourceCounters,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SourceState {
    #[default]
    NotReported,
    /// The source's OS callback is installed.
    Running,
    /// The source runs and part of its capture is missing.
    Impaired { reason: HealthReason },
    Failed {
        reason: HealthReason,
        error: CaptureError,
    },
    /// The source was stopped.
    Stopped(StopCause),
    /// The source never captures, or every switch it reads is off.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    /// The daemon asked the source to stop.
    Request,
    /// The person stopped the capture from the operating system's control.
    User,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceCounters {
    pub tap_disabled_timeout: u64,
    pub tap_disabled_user_input: u64,
    pub late_events: u64,
    pub max_lag_ms: u64,
    /// Key events that reached the tap while secure input was active.
    pub secure_input_keystrokes: u64,
    pub title_read_errors: u64,
    pub pointer_coalesced: u64,
    pub keys_withheld: bool,
    pub secure_released_keystrokes: u64,
    pub secure_held_keystrokes: u64,
    pub secure_held_suppressed: SuppressedKeys,
    pub secure_input_held: HeldSamples,
    pub secure_window_pointer: u64,
    /// Whether the key counter of the system advanced in the held phase.
    pub secure_window_keys_typed: bool,
}

/// Key events suppressed in the held phase, by type.
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

/// Only the sink handle implements it; an implementation in another crate does
/// not compile:
///
/// ```compile_fail
/// use halvo_capture::event::CaptureStreamEvent;
/// use halvo_capture::sink::SinkError;
/// use halvo_capture::source::{EmitOutcome, EventSink};
///
/// struct Elsewhere;
///
/// impl EventSink for Elsewhere {
///     fn emit(&self, _event: CaptureStreamEvent) -> Result<EmitOutcome, SinkError> {
///         Ok(EmitOutcome::Queued)
///     }
/// }
/// ```
///
/// The same code compiles when the method is not an implementation of the trait:
///
/// ```
/// use halvo_capture::event::CaptureStreamEvent;
/// use halvo_capture::sink::SinkError;
/// use halvo_capture::source::{EmitOutcome, EventSink};
///
/// struct Elsewhere;
///
/// impl Elsewhere {
///     fn emit(&self, _event: CaptureStreamEvent) -> Result<EmitOutcome, SinkError> {
///         Ok(EmitOutcome::Queued)
///     }
/// }
///
/// fn emits_through(_sink: &impl EventSink) {}
/// let (handle, _writer) = halvo_capture::sink::channel(1);
/// emits_through(&handle);
/// ```
pub trait EventSink: sealed::Sealed {
    fn emit(&self, event: CaptureStreamEvent) -> Result<EmitOutcome, SinkError>;
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for crate::sink::SinkHandle {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitOutcome {
    Queued,
    Dropped,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("the capture permission for {0:?} is not granted")]
    PermissionDenied(Modality),
    #[error("the capture source failed to start: {0}")]
    Start(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
    #[error("the capture source timed out at {stage} after {ms} ms")]
    Timeout { stage: &'static str, ms: u64 },
    #[error("the capture source failed at {stage}: {domain} code {code}")]
    Os {
        stage: &'static str,
        domain: &'static str,
        code: i64,
    },
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
    fn each_error_prints_fixed_text_and_its_code() {
        let cases = [
            (
                CaptureError::PermissionDenied(Modality::ScreenFrame),
                "the capture permission for ScreenFrame is not granted",
            ),
            (
                CaptureError::Start("no display to capture".to_string()),
                "the capture source failed to start: no display to capture",
            ),
            (
                CaptureError::NotImplemented("system audio capture"),
                "system audio capture is not implemented in this phase",
            ),
            (
                CaptureError::Timeout {
                    stage: "stop",
                    ms: 1000,
                },
                "the capture source timed out at stop after 1000 ms",
            ),
            (
                CaptureError::Os {
                    stage: "start",
                    domain: "OSStatus",
                    code: -3801,
                },
                "the capture source failed at start: OSStatus code -3801",
            ),
            (
                CaptureError::ThreadEnded,
                "the capture thread had ended before the source was shut down",
            ),
            (
                CaptureError::AlreadyStarted,
                "the capture source is already started",
            ),
        ];
        for (error, text) in &cases {
            assert_eq!(error.to_string(), *text);
            assert_eq!(error.clone(), *error);
        }
        for (left, (a, _)) in cases.iter().enumerate() {
            for (right, (b, _)) in cases.iter().enumerate() {
                assert_eq!(a == b, left == right, "{a:?} against {b:?}");
            }
        }
        let failed = SourceHealth {
            state: SourceState::Failed {
                reason: HealthReason::OsError,
                error: CaptureError::Os {
                    stage: "start",
                    domain: "OSStatus",
                    code: -3801,
                },
            },
            counters: SourceCounters::default(),
        };
        assert_eq!(failed.clone(), failed);
        assert_ne!(failed, SourceHealth::default());
    }

    struct NeitherPair;

    impl CaptureSource for NeitherPair {}

    fn panic_text(outcome: std::thread::Result<()>) -> String {
        let payload = outcome.expect_err("the call returned instead of panicking");
        match payload.downcast::<&str>() {
            Ok(text) => (*text).to_string(),
            Err(payload) => *payload.downcast::<String>().expect("a text payload"),
        }
    }

    #[test]
    fn a_source_that_implements_neither_pair_panics_with_fixed_text() {
        let neither = "CaptureSource implements neither begin and shutdown nor start and stop";
        let mut source = NeitherPair;
        let begin = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = source.begin(context());
        }));
        assert_eq!(panic_text(begin), neither);
        let start = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (handle, _writer) = sink::channel(1);
            let _ = source.start(handle);
        }));
        assert_eq!(panic_text(start), neither);
        let shutdown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = source.shutdown();
        }));
        assert_eq!(panic_text(shutdown), neither);
        let stop = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.stop()));
        assert_eq!(panic_text(stop), neither);
        let mut old = OldPair::default();
        assert!(matches!(old.begin(context()), Ok(Started::Running)));
        assert!(old.shutdown().is_ok());
        assert_eq!(old.calls, ["start", "stop"]);
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
