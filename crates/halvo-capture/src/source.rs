use std::cell::RefCell;

use crate::event::{CaptureStreamEvent, Modality};
use crate::floor::exclude::FloorHandle;
use crate::sink::{SinkError, SinkHandle};

/// The default bodies of `start`, `stop`, `begin` and `shutdown` reach the
/// other pair. A default member must not be entered again for the same source
/// while it runs, and two boxed sources of one zero-sized type count as one
/// source, because every such box has one address: either panics with the text
/// for a source that implements neither pair.
pub trait CaptureSource {
    /// Retired: no product code calls it, and the default body panics.
    fn modality(&self) -> Modality {
        panic!("CaptureSource::modality is retired and has no default answer")
    }

    /// The old form of [`CaptureSource::begin`].
    fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
        let _call = DefaultPairCall::enter(&*self, DefaultMember::Start);
        self.begin(SourceContext::new(sink)).map(|_| ())
    }

    /// The old form of [`CaptureSource::shutdown`].
    fn stop(&mut self) {
        let _call = DefaultPairCall::enter(&*self, DefaultMember::Stop);
        let _ = self.shutdown();
    }

    /// An implementer answers a second `begin` on a source that is already
    /// started with [`CaptureError::AlreadyStarted`]; the default body does not.
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        let _call = DefaultPairCall::enter(&*self, DefaultMember::Begin);
        self.start(ctx.sink).map(|()| Started::Running)
    }

    /// Remove the OS callback and join the source's thread: `Ok` when both are
    /// done, [`CaptureError::ThreadEnded`] when the thread had ended before,
    /// [`CaptureError::Timeout`] when the source's deadline passed.
    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let _call = DefaultPairCall::enter(&*self, DefaultMember::Shutdown);
        self.stop();
        Ok(())
    }

    fn health(&self) -> SourceHealth {
        SourceHealth::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefaultMember {
    Start,
    Stop,
    Begin,
    Shutdown,
}

thread_local! {
    static DEFAULT_PAIR_CALLS: RefCell<Vec<(usize, &'static str, DefaultMember)>> =
        const { RefCell::new(Vec::new()) };
}

struct DefaultPairCall {
    call: (usize, &'static str, DefaultMember),
}

impl DefaultPairCall {
    fn enter<S: ?Sized>(source: &S, member: DefaultMember) -> Self {
        let call = (
            (source as *const S).cast::<()>().addr(),
            std::any::type_name::<S>(),
            member,
        );
        let again = DEFAULT_PAIR_CALLS
            .try_with(|calls| {
                let mut calls = calls.borrow_mut();
                let again = calls.contains(&call);
                if !again {
                    calls.push(call);
                }
                again
            })
            .unwrap_or(false);
        if again {
            panic!("CaptureSource implements neither begin and shutdown nor start and stop");
        }
        Self { call }
    }
}

impl Drop for DefaultPairCall {
    fn drop(&mut self) {
        let _ = DEFAULT_PAIR_CALLS.try_with(|calls| {
            let mut calls = calls.borrow_mut();
            if let Some(at) = calls.iter().rposition(|call| *call == self.call) {
                calls.remove(at);
            }
        });
    }
}

/// Owns the started state of one source: a second `begin` while the source is
/// started is [`CaptureError::AlreadyStarted`], whatever the source answers
/// itself. A `shutdown` that returns `Ok` or [`CaptureError::ThreadEnded`]
/// ends the started state; after any other answer the source counts as started.
/// The daemon starts every source through it.
pub struct StartGuard {
    source: Box<dyn CaptureSource>,
    started: bool,
}

impl StartGuard {
    pub fn new(source: Box<dyn CaptureSource>) -> StartGuard {
        StartGuard {
            source,
            started: false,
        }
    }

    pub fn is_started(&self) -> bool {
        self.started
    }
}

impl CaptureSource for StartGuard {
    fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
        if self.started {
            return Err(CaptureError::AlreadyStarted);
        }
        let started = self.source.begin(ctx)?;
        self.started = true;
        Ok(started)
    }

    fn shutdown(&mut self) -> Result<(), CaptureError> {
        let stopped = self.source.shutdown();
        if matches!(stopped, Ok(()) | Err(CaptureError::ThreadEnded)) {
            self.started = false;
        }
        stopped
    }

    fn health(&self) -> SourceHealth {
        self.source.health()
    }
}

#[derive(Clone)]
pub struct SourceContext {
    sink: SinkHandle,
    floor: FloorHandle,
}

impl SourceContext {
    /// A context with the default floor handle: no exclusion root and the
    /// default switches.
    pub fn new(sink: SinkHandle) -> Self {
        Self::with_floor(sink, FloorHandle::default())
    }

    /// A context whose sources decide with `floor`: the daemon's exclusion
    /// root, lineage probe and switches.
    pub fn with_floor(sink: SinkHandle, floor: FloorHandle) -> Self {
        Self { sink, floor }
    }

    pub fn sink(&self) -> &SinkHandle {
        &self.sink
    }

    /// The exclusion decision and the switches for the source's events.
    pub fn floor(&self) -> &FloorHandle {
        &self.floor
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
    Impaired {
        reason: HealthReason,
    },
    Failed {
        reason: HealthReason,
        error: CaptureError,
    },
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
    /// The old emit. It takes any `CaptureStreamEvent`, with no keep verdict,
    /// no clearance and no text filter point, and stays the remaining route of
    /// a capture event to a stored record until core-3 removes it.
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
    /// Only for a real denial: a service that cannot be reached is
    /// [`CaptureError::Os`] or [`CaptureError::Timeout`].
    #[error("the capture permission for {} is not granted", .0.as_str())]
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
                "the capture permission for screen_frame is not granted",
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

    #[derive(Default)]
    struct StopThatShutsDownOnceMore {
        calls: Vec<&'static str>,
        inside: bool,
    }

    impl CaptureSource for StopThatShutsDownOnceMore {
        fn start(&mut self, _sink: SinkHandle) -> Result<(), CaptureError> {
            self.calls.push("start");
            Ok(())
        }

        fn stop(&mut self) {
            self.calls.push("stop");
            if !self.inside {
                self.inside = true;
                let _ = self.shutdown();
                self.inside = false;
            }
        }
    }

    #[test]
    fn a_stop_that_enters_the_default_shutdown_again_panics_with_the_fixed_text() {
        let neither = "CaptureSource implements neither begin and shutdown nor start and stop";
        let mut source = StopThatShutsDownOnceMore::default();
        let shutdown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = source.shutdown();
        }));
        assert_eq!(panic_text(shutdown), neither);
        assert_eq!(source.calls, ["stop"]);
    }

    thread_local! {
        static ZERO_SIZED_CHILD_STARTED: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }

    struct ZeroSized;

    impl CaptureSource for ZeroSized {
        fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
            if !ZERO_SIZED_CHILD_STARTED.with(|started| started.replace(true)) {
                let mut child = Box::new(ZeroSized);
                child.start(ctx.sink().clone())?;
            }
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            Ok(())
        }
    }

    #[test]
    fn two_boxed_sources_of_one_zero_sized_type_count_as_one_source() {
        let neither = "CaptureSource implements neither begin and shutdown nor start and stop";
        let mut parent = Box::new(ZeroSized);
        let other = Box::new(ZeroSized);
        assert!(std::ptr::eq(&*parent, &*other));
        let (handle, _writer) = sink::channel(1);
        let start = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = parent.start(handle);
        }));
        assert_eq!(panic_text(start), neither);
    }

    #[derive(Default)]
    struct TidiesThroughTheOtherPair {
        calls: Vec<&'static str>,
    }

    impl CaptureSource for TidiesThroughTheOtherPair {
        fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
            self.stop();
            self.calls.push("begin");
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            self.calls.push("shutdown");
            Ok(())
        }
    }

    #[test]
    fn a_source_may_call_the_other_pair_inside_its_own() {
        let mut source = TidiesThroughTheOtherPair::default();
        let (handle, _writer) = sink::channel(1);
        assert!(source.start(handle).is_ok());
        assert_eq!(source.calls, ["shutdown", "begin"]);
        source.stop();
        assert_eq!(source.calls, ["shutdown", "begin", "shutdown"]);
    }

    #[derive(Default)]
    struct OldPairOverNewPair(NewPair);

    impl CaptureSource for OldPairOverNewPair {
        fn start(&mut self, sink: SinkHandle) -> Result<(), CaptureError> {
            self.0.start(sink)
        }

        fn stop(&mut self) {
            self.0.stop();
        }
    }

    #[derive(Default)]
    struct NewPairOverNewPair(NewPair);

    impl CaptureSource for NewPairOverNewPair {
        fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
            self.0.start(ctx.sink().clone()).map(|()| Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            self.0.stop();
            Ok(())
        }
    }

    #[test]
    fn a_wrapper_reaches_the_source_inside_it_at_the_same_address() {
        let mut old_over_new = OldPairOverNewPair::default();
        let outer = (&old_over_new as *const OldPairOverNewPair).cast::<()>();
        assert_eq!(outer, (&old_over_new.0 as *const NewPair).cast::<()>());
        assert!(matches!(
            old_over_new.begin(context()),
            Ok(Started::Running)
        ));
        assert!(old_over_new.shutdown().is_ok());
        assert_eq!(old_over_new.0.calls, ["begin", "shutdown"]);

        let mut new_over_new = NewPairOverNewPair::default();
        let outer = (&new_over_new as *const NewPairOverNewPair).cast::<()>();
        assert_eq!(outer, (&new_over_new.0 as *const NewPair).cast::<()>());
        let (handle, _writer) = sink::channel(1);
        assert!(new_over_new.start(handle).is_ok());
        new_over_new.stop();
        assert_eq!(new_over_new.0.calls, ["begin", "shutdown"]);
    }

    struct Leaf(std::rc::Rc<std::cell::Cell<u32>>);

    impl CaptureSource for Leaf {
        fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
            self.0.set(self.0.get() + 1);
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            self.0.set(self.0.get() + 100);
            Ok(())
        }
    }

    struct Group {
        children: Vec<Box<dyn CaptureSource>>,
    }

    impl CaptureSource for Group {
        fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
            for child in &mut self.children {
                child.start(ctx.sink().clone())?;
            }
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            for child in &mut self.children {
                child.stop();
            }
            Ok(())
        }
    }

    #[test]
    fn a_group_of_groups_starts_and_stops_through_the_old_pair() {
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let inner = Group {
            children: vec![Box::new(Leaf(count.clone()))],
        };
        let mut outer: Box<dyn CaptureSource> = Box::new(Group {
            children: vec![Box::new(inner)],
        });
        let (handle, _writer) = sink::channel(1);
        let started =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| outer.start(handle).is_ok()));
        assert!(
            matches!(started, Ok(true)),
            "the old start panicked or failed"
        );
        let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| outer.stop()));
        assert!(stopped.is_ok(), "the old stop panicked");
        assert_eq!(count.get(), 101);
    }

    struct HeldUntilTheThreadEnds {
        source: Option<NewPair>,
        report: std::sync::mpsc::Sender<std::thread::Result<Vec<&'static str>>>,
    }

    impl Drop for HeldUntilTheThreadEnds {
        fn drop(&mut self) {
            if let Some(mut source) = self.source.take() {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    source.stop();
                    source.calls.clone()
                }));
                let _ = self.report.send(outcome);
            }
        }
    }

    thread_local! {
        static HELD: std::cell::RefCell<Option<HeldUntilTheThreadEnds>> =
            const { std::cell::RefCell::new(None) };
    }

    #[test]
    fn a_default_stop_in_a_thread_local_destructor_returns() {
        let (report, outcomes) = std::sync::mpsc::channel();
        thread::spawn(move || {
            HELD.with(|held| {
                *held.borrow_mut() = Some(HeldUntilTheThreadEnds {
                    source: None,
                    report,
                });
            });
            let mut source = NewPair::default();
            let (handle, _writer) = sink::channel(1);
            source.start(handle).unwrap();
            HELD.with(|held| held.borrow_mut().as_mut().unwrap().source = Some(source));
        })
        .join()
        .unwrap();
        let outcome = outcomes
            .recv_timeout(Duration::from_secs(5))
            .expect("the holder reported inside 5 s");
        let calls = outcome.expect("the default stop panicked in a thread-local destructor");
        assert_eq!(calls, ["begin", "shutdown"]);
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

    #[test]
    fn permission_denied_names_the_kind_in_snake_case() {
        let expected = [
            "screen_frame",
            "keystroke",
            "pointer",
            "foreground_app",
            "window_title",
            "system_audio",
            "microphone",
            "camera",
        ];
        for (kind, name) in Modality::ALL.into_iter().zip(expected) {
            assert_eq!(
                CaptureError::PermissionDenied(kind).to_string(),
                format!("the capture permission for {name} is not granted")
            );
        }
    }

    struct StandIn {
        calls: std::rc::Rc<std::cell::RefCell<Vec<&'static str>>>,
        refuse_begin: bool,
        stop_answer: Result<(), CaptureError>,
    }

    impl CaptureSource for StandIn {
        fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
            self.calls.borrow_mut().push("begin");
            if self.refuse_begin {
                return Err(CaptureError::Start("scripted refusal".to_string()));
            }
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            self.calls.borrow_mut().push("shutdown");
            self.stop_answer.clone()
        }
    }

    fn guarded(
        refuse_begin: bool,
        stop_answer: Result<(), CaptureError>,
    ) -> (
        StartGuard,
        std::rc::Rc<std::cell::RefCell<Vec<&'static str>>>,
    ) {
        let calls = std::rc::Rc::default();
        let source = StandIn {
            calls: std::rc::Rc::clone(&calls),
            refuse_begin,
            stop_answer,
        };
        (StartGuard::new(Box::new(source)), calls)
    }

    #[test]
    fn a_second_begin_through_the_start_guard_is_already_started_for_every_source() {
        let (mut guard, calls) = guarded(false, Ok(()));
        assert!(!guard.is_started());
        assert!(matches!(guard.begin(context()), Ok(Started::Running)));
        assert!(guard.is_started());
        assert!(matches!(
            guard.begin(context()),
            Err(CaptureError::AlreadyStarted)
        ));
        let (handle, _writer) = sink::channel(1);
        assert!(matches!(
            guard.start(handle),
            Err(CaptureError::AlreadyStarted)
        ));
        assert!(guard.shutdown().is_ok());
        assert!(!guard.is_started());
        assert!(matches!(guard.begin(context()), Ok(Started::Running)));
        assert_eq!(*calls.borrow(), ["begin", "shutdown", "begin"]);

        let (mut refusing, calls) = guarded(true, Ok(()));
        assert!(matches!(
            refusing.begin(context()),
            Err(CaptureError::Start(_))
        ));
        assert!(!refusing.is_started());
        assert!(matches!(
            refusing.begin(context()),
            Err(CaptureError::Start(_))
        ));
        assert_eq!(*calls.borrow(), ["begin", "begin"]);

        let (mut ended, _) = guarded(false, Err(CaptureError::ThreadEnded));
        ended.begin(context()).unwrap();
        assert!(matches!(ended.shutdown(), Err(CaptureError::ThreadEnded)));
        assert!(!ended.is_started());

        let timeout = CaptureError::Timeout {
            stage: "stop",
            ms: 1000,
        };
        let (mut stuck, calls) = guarded(false, Err(timeout));
        stuck.begin(context()).unwrap();
        assert!(matches!(
            stuck.shutdown(),
            Err(CaptureError::Timeout { .. })
        ));
        assert!(stuck.is_started());
        assert!(matches!(
            stuck.begin(context()),
            Err(CaptureError::AlreadyStarted)
        ));
        assert_eq!(*calls.borrow(), ["begin", "shutdown"]);

        let mut old = StartGuard::new(Box::<OldPair>::default());
        let (handle, _writer) = sink::channel(1);
        assert!(old.start(handle).is_ok());
        let (handle, _writer) = sink::channel(1);
        assert!(matches!(
            old.start(handle),
            Err(CaptureError::AlreadyStarted)
        ));
        old.stop();
        assert!(!old.is_started());
    }
}
