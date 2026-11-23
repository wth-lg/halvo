//! The sources of a run: which start, in which order they stop, and the set
//! of running sources that the supervisor holds.

use std::time::{Duration, Instant};

use halvo_control::proto::{Platform, SourceName};

use crate::floor::{Switch, Switches};
use crate::source::{
    CaptureError, CaptureSource, HealthReason, SourceContext, SourceCounters, SourceState, Started,
    StopCause,
};

/// The run that starts the sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// A timed run; `key_class_flag` is true when its command line set
    /// `key_class` on with `--switch`.
    Timed { key_class_flag: bool },
    /// A service run, whatever its state file says.
    Service,
}

/// The stop order on macOS: input, foreground, screen (contracts.md C12 item 7).
pub const MACOS_STOP_ORDER: [SourceName; 3] = [
    SourceName::Input,
    SourceName::Foreground,
    SourceName::Screen,
];

/// The stop order on Windows: keyboard, pointer, window_title, foreground,
/// screen (contracts.md C12 item 7).
pub const WINDOWS_STOP_ORDER: [SourceName; 5] = [
    SourceName::Keyboard,
    SourceName::Pointer,
    SourceName::WindowTitle,
    SourceName::Foreground,
    SourceName::Screen,
];

/// The order in which every stop sequence of `platform` stops its sources.
pub const fn stop_order(platform: Platform) -> &'static [SourceName] {
    match platform {
        Platform::Macos => &MACOS_STOP_ORDER,
        Platform::Windows => &WINDOWS_STOP_ORDER,
    }
}

/// The time that a stop's answer gets past its source's deadline before it
/// reads as not stopped (ruling PL-3: "no return at its deadline plus
/// 250 ms").
pub const STOP_SLACK: Duration = Duration::from_millis(250);

/// The deadline that the source contract gives the stop of each source but
/// the screen source, on both systems: macos::input's and
/// macos::foreground's, and windows' INPUT_STOP_DEADLINE.
pub const STOP_DEADLINE: Duration = Duration::from_millis(1_000);

/// The deadline that the source contract gives the screen source's stop on
/// both systems: macos::screen's and windows' SCREEN_STOP_DEADLINE.
pub const SCREEN_STOP_DEADLINE: Duration = Duration::from_millis(6_000);

#[cfg(target_os = "macos")]
const _: () = assert!(
    crate::macos::screen::SCREEN_STOP_DEADLINE.as_millis() == SCREEN_STOP_DEADLINE.as_millis()
        && crate::macos::input::INPUT_STOP_DEADLINE.as_millis() == STOP_DEADLINE.as_millis()
        && crate::macos::foreground::FOREGROUND_STOP_DEADLINE.as_millis()
            == STOP_DEADLINE.as_millis()
);

#[cfg(windows)]
const _: () = assert!(
    crate::windows::SCREEN_STOP_DEADLINE.as_millis() == SCREEN_STOP_DEADLINE.as_millis()
        && crate::windows::INPUT_STOP_DEADLINE.as_millis() == STOP_DEADLINE.as_millis()
);

/// The bound of the stop of `name`: its contract deadline plus
/// [`STOP_SLACK`]. An answer later than this, on the real clock around the
/// call, is not a stop (ruling DM-25 N1).
pub fn stop_bound(name: SourceName) -> Duration {
    let deadline = match name {
        SourceName::Screen => SCREEN_STOP_DEADLINE,
        _ => STOP_DEADLINE,
    };
    deadline + STOP_SLACK
}

/// The stage of the timeout that a stop gives when it answered after its
/// bound; its `ms` is the time that the call took.
pub const PAST_BOUND: &str = "stop_past_bound";

/// The time by which the stop of `name` that gave `answer` ran past its
/// bound, when `answer` is the timeout of a stop that answered late.
pub fn past_bound(name: SourceName, answer: &Result<(), CaptureError>) -> Option<Duration> {
    match answer {
        Err(CaptureError::Timeout { stage, ms }) if *stage == PAST_BOUND => {
            Some(Duration::from_millis(*ms).saturating_sub(stop_bound(name)))
        }
        _ => None,
    }
}

/// The sources that a run of `kind` with `switches` starts on `platform`, in
/// the fixed order of the system's source names: the foreground source; the
/// screen source only while `screen_activity` is on; on macOS the input
/// source while `key_class` or `pointer_class` is on; on Windows the keyboard
/// source only in a timed run whose flag set `key_class` on (contracts.md
/// C13.8), the pointer source while `pointer_class` is on and the window_title
/// source while `title_text` is on; never a system_audio or a microphone
/// source.
pub fn starting(platform: Platform, kind: RunKind, switches: Switches) -> Vec<SourceName> {
    SourceName::of(platform)
        .iter()
        .copied()
        .filter(|name| starts(*name, kind, switches))
        .collect()
}

fn starts(name: SourceName, kind: RunKind, switches: Switches) -> bool {
    match name {
        SourceName::Foreground => true,
        SourceName::Screen => switches.is_on(Switch::ScreenActivity),
        SourceName::Input => {
            switches.is_on(Switch::KeyClass) || switches.is_on(Switch::PointerClass)
        }
        SourceName::Keyboard => {
            kind == RunKind::Timed {
                key_class_flag: true,
            } && switches.is_on(Switch::KeyClass)
        }
        SourceName::Pointer => switches.is_on(Switch::PointerClass),
        SourceName::WindowTitle => switches.is_on(Switch::TitleText),
        SourceName::SystemAudio | SourceName::Microphone => false,
    }
}

/// Makes the source of one name for a start; `None` when the system has no
/// source of that name.
pub trait SourceFactory {
    fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>>;
}

/// The factory of a service run on macOS: the foreground, screen and input
/// sources of macos-4 on the process's one foreground writer, the input
/// source with no hold planned (ruling MA-9 part (3)) and the product's
/// bound. The factory keeps the writer and gives each foreground source it
/// makes a share of it, so that a source started after a lapse and a renewal
/// writes the app in front again.
#[cfg(target_os = "macos")]
pub struct ServiceSources {
    foreground_writer: crate::macos::foreground::ForegroundWriter,
    foreground_reader: crate::macos::foreground::ForegroundReader,
    turns: crate::macos::run_loop::RunLoopTurns,
    input_counters: crate::macos::input::InputCounters,
    keys_refused: bool,
    #[cfg(test)]
    stand_in_titles: bool,
}

#[cfg(target_os = "macos")]
impl ServiceSources {
    pub fn new(
        writer: crate::macos::foreground::ForegroundWriter,
        reader: crate::macos::foreground::ForegroundReader,
        turns: crate::macos::run_loop::RunLoopTurns,
    ) -> ServiceSources {
        ServiceSources {
            foreground_writer: writer,
            foreground_reader: reader,
            turns,
            input_counters: crate::macos::input::InputCounters::new(),
            keys_refused: false,
            #[cfg(test)]
            stand_in_titles: false,
        }
    }

    /// The factory whose foreground sources read their titles through a
    /// stand-in that reads none, for a test that must read no title of the
    /// machine that runs it.
    #[cfg(all(test, debug_assertions))]
    pub(crate) fn with_stand_in_titles(mut self) -> ServiceSources {
        self.stand_in_titles = true;
        self
    }

    /// The hold plan of the input source that this factory makes: never.
    pub fn input_plan(&self) -> crate::macos::input::HoldPlan {
        crate::macos::input::HoldPlan::Never
    }
}

#[cfg(target_os = "macos")]
impl SourceFactory for ServiceSources {
    fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
        use crate::macos::{foreground, input, screen};
        match name {
            #[cfg(test)]
            SourceName::Foreground if self.stand_in_titles => {
                Some(Box::new(foreground::ForegroundSource::with_stand_in_title(
                    foreground::FOREGROUND_POLL,
                    self.foreground_writer.share(),
                    self.turns.clone(),
                )))
            }
            SourceName::Foreground => Some(Box::new(foreground::ForegroundSource::new(
                foreground::FOREGROUND_POLL,
                self.foreground_writer.share(),
                self.turns.clone(),
            ))),
            SourceName::Screen => Some(Box::new(screen::ScreenSource::new(
                screen::SCREEN_FRAME_CAP,
                self.foreground_reader.clone(),
            ))),
            SourceName::Input => Some(Box::new(
                input::InputSource::new(
                    self.foreground_reader.clone(),
                    self.input_counters.clone(),
                    self.input_plan(),
                )
                .with_keys_refused(self.keys_refused),
            )),
            SourceName::SystemAudio
            | SourceName::Microphone
            | SourceName::Keyboard
            | SourceName::Pointer
            | SourceName::WindowTitle => None,
        }
    }
}

#[cfg(target_os = "macos")]
impl super::supervisor::SystemSources for ServiceSources {
    fn figures(&self) -> SourceCounters {
        self.input_counters.figures()
    }

    fn refuse_keys(&mut self, refused: bool) {
        self.keys_refused = refused;
    }
}

/// The health state that a failed start or stop gives a source.
pub fn failure(error: CaptureError) -> SourceState {
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

/// Whether a source's answer to `shutdown` is a stop: Ok, or ThreadEnded.
pub fn is_a_stop(answer: &Result<(), CaptureError>) -> bool {
    matches!(answer, Ok(()) | Err(CaptureError::ThreadEnded))
}

struct Entry {
    name: SourceName,
    source: Option<Box<dyn CaptureSource>>,
    state: SourceState,
    unstopped: bool,
    counters: SourceCounters,
}

/// One entry for every source name of the system, in its fixed order, each
/// with the source that runs under it, if any. A source whose `shutdown`
/// answered neither Ok nor ThreadEnded stays held under its name with the mark
/// `unstopped`, and no second source of that name begins while it is held.
pub struct RunningSources {
    platform: Platform,
    entries: Vec<Entry>,
}

impl RunningSources {
    /// Every source of `platform`, each off.
    pub fn new(platform: Platform) -> RunningSources {
        RunningSources {
            platform,
            entries: SourceName::of(platform)
                .iter()
                .map(|name| Entry {
                    name: *name,
                    source: None,
                    state: SourceState::Off,
                    unstopped: false,
                    counters: SourceCounters::default(),
                })
                .collect(),
        }
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Starts each source of `wanted` that holds no source, in the fixed
    /// order, each with a clone of `ctx`; a source outside `wanted` that holds
    /// no source reads off. Returns each start's answer.
    pub fn start(
        &mut self,
        factory: &mut dyn SourceFactory,
        ctx: &SourceContext,
        wanted: &[SourceName],
    ) -> Vec<(SourceName, Result<Started, CaptureError>)> {
        let names: Vec<SourceName> = self.entries.iter().map(|entry| entry.name).collect();
        let mut answers = Vec::new();
        for name in names {
            if !wanted.contains(&name) {
                if let Some(entry) = self.entry_mut(name).filter(|entry| entry.source.is_none()) {
                    entry.state = SourceState::Off;
                }
                continue;
            }
            if let Some(answer) = self.start_one(factory, ctx, name) {
                answers.push((name, answer));
            }
        }
        answers
    }

    /// Starts the source of `name` when its entry holds none: `None` when it
    /// holds one, running or held. A source that answers off reads off, or
    /// stopped where its health reports a stop at once; a source that answers
    /// running reads impaired where its health reports so at once, as the input
    /// source whose tap asked for the pointer types alone does.
    pub fn start_one(
        &mut self,
        factory: &mut dyn SourceFactory,
        ctx: &SourceContext,
        name: SourceName,
    ) -> Option<Result<Started, CaptureError>> {
        if self.entry_mut(name)?.source.is_some() {
            return None;
        }
        let made = factory.make(name);
        let entry = self.entry_mut(name)?;
        let Some(mut source) = made else {
            entry.state = SourceState::Off;
            return Some(Ok(Started::Off));
        };
        let answer = source.begin(ctx.clone());
        match &answer {
            Ok(Started::Running) => {
                entry.state = match source.health().state {
                    impaired @ SourceState::Impaired { .. } => impaired,
                    _ => SourceState::Running,
                };
                entry.source = Some(source);
            }
            Ok(Started::Off) => {
                entry.state = match source.health().state {
                    stopped @ SourceState::Stopped(_) => stopped,
                    _ => SourceState::Off,
                };
                entry.source = Some(source);
            }
            Err(error) => entry.state = failure(error.clone()),
        }
        Some(answer)
    }

    /// Stops every source that runs or is held, in the stop order of the
    /// system, and returns each answer.
    pub fn stop(&mut self) -> Vec<(SourceName, Result<(), CaptureError>)> {
        stop_order(self.platform)
            .iter()
            .filter_map(|name| self.stop_one(*name).map(|answer| (*name, answer)))
            .collect()
    }

    /// Stops the source of `name`; `None` when its entry holds none. A stop
    /// ends the source and its mark; any other answer keeps it held, marked
    /// `unstopped`, failed with reason timeout or os_error. A stop (Ok or
    /// ThreadEnded) that answers after [`stop_bound`], read on the real clock
    /// around the call, answers a timeout of stage [`PAST_BOUND`] in its
    /// place; a late error keeps its own error, so that the arm after it
    /// counts from its answer and not from the moment its bound passed. The
    /// supervisor's own clock of ruling PL-3 is the real one, as the main
    /// run loop's is, and a test loop's injected clock does not move while a
    /// call blocks.
    pub fn stop_one(&mut self, name: SourceName) -> Option<Result<(), CaptureError>> {
        let entry = self.entry_mut(name)?;
        let source = entry.source.as_mut()?;
        let called = Instant::now();
        let answer = source.shutdown();
        let took = called.elapsed();
        let answer = if is_a_stop(&answer) && took > stop_bound(name) {
            Err(CaptureError::Timeout {
                stage: PAST_BOUND,
                ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX),
            })
        } else {
            answer
        };
        if is_a_stop(&answer) {
            entry.source = None;
            entry.unstopped = false;
            if !matches!(
                entry.state,
                SourceState::Failed { .. } | SourceState::Stopped(StopCause::User)
            ) {
                entry.state = SourceState::Stopped(StopCause::Request);
            }
        } else if let Err(error) = &answer {
            entry.unstopped = true;
            entry.state = match failure(error.clone()) {
                SourceState::Failed {
                    reason: HealthReason::Timeout,
                    error,
                } => SourceState::Failed {
                    reason: HealthReason::Timeout,
                    error,
                },
                SourceState::Failed { error, .. } => SourceState::Failed {
                    reason: HealthReason::OsError,
                    error,
                },
                other => other,
            };
        }
        Some(answer)
    }

    /// Reads the health of every source that runs and keeps its state and its
    /// counters; a source that has not reported yet keeps the state of its
    /// start.
    pub fn read_health(&mut self) {
        for entry in &mut self.entries {
            if entry.unstopped {
                continue;
            }
            let Some(source) = entry.source.as_ref() else {
                continue;
            };
            let health = source.health();
            entry.counters = health.counters;
            if health.state != SourceState::NotReported {
                entry.state = health.state;
            }
        }
    }

    /// What the source of `name` reports now, without keeping it: off when
    /// the entry holds no source.
    pub fn read_health_of(&self, name: SourceName) -> SourceState {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .and_then(|entry| entry.source.as_ref())
            .map_or(SourceState::Off, |source| source.health().state)
    }

    /// The state of `name` and its mark `unstopped`.
    pub fn state(&self, name: SourceName) -> Option<(&SourceState, bool)> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| (&entry.state, entry.unstopped))
    }

    /// Every source name of the system, in its fixed order, with its state and
    /// its mark `unstopped`.
    pub fn states(&self) -> impl Iterator<Item = (SourceName, &SourceState, bool)> {
        self.entries
            .iter()
            .map(|entry| (entry.name, &entry.state, entry.unstopped))
    }

    /// Whether the entry of `name` holds a source, running or held.
    pub fn holds(&self, name: SourceName) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.name == name && entry.source.is_some())
    }

    /// The counters of the last health read of each source.
    pub fn counters(&self) -> impl Iterator<Item = (SourceName, SourceCounters)> + '_ {
        self.entries
            .iter()
            .map(|entry| (entry.name, entry.counters))
    }

    /// Marks `name` failed with `state`, whatever it read before.
    pub fn set_state(&mut self, name: SourceName, state: SourceState) {
        if let Some(entry) = self.entry_mut(name) {
            entry.state = state;
        }
    }

    fn entry_mut(&mut self, name: SourceName) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|entry| entry.name == name)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use halvo_control::proto::{source_of, Platform, SourceName};

    use super::*;
    use crate::event::{FloorEvent, Modality};
    use crate::floor::exclude::{FloorHandle, NoLineage, ProcessId};
    use crate::sink::{self, Emitted};
    use crate::source::{SourceHealth, SourceState};

    /// What scripted sources did, in order: `begin <name>`, `stop <name>`.
    #[derive(Clone, Default)]
    pub(crate) struct Calls(Arc<Mutex<Vec<String>>>);

    impl Calls {
        pub(crate) fn push(&self, call: String) {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(call);
        }

        pub(crate) fn taken(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        pub(crate) fn of(&self, prefix: &str) -> Vec<String> {
            self.taken()
                .into_iter()
                .filter(|call| call.starts_with(prefix))
                .collect()
        }
    }

    /// The health that a test sets for a scripted source.
    #[derive(Clone, Default)]
    pub(crate) struct Health(Arc<Mutex<SourceHealth>>);

    impl Health {
        pub(crate) fn set(&self, state: SourceState) {
            self.0.lock().unwrap_or_else(PoisonError::into_inner).state = state;
        }

        fn read(&self) -> SourceHealth {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    /// A source that records its calls and the floor it was given, emits its
    /// script at its begin, and answers as the test set it.
    pub(crate) struct Scripted {
        pub(crate) name: SourceName,
        pub(crate) calls: Calls,
        pub(crate) begin: Result<Started, CaptureError>,
        pub(crate) stop: Result<(), CaptureError>,
        pub(crate) health: Health,
        pub(crate) script: Vec<FloorEvent>,
        pub(crate) floors: Arc<Mutex<Vec<String>>>,
        pub(crate) ctx: Arc<Mutex<Option<SourceContext>>>,
        pub(crate) on_begin: Option<Hook>,
        pub(crate) on_stop: Option<Hook>,
        pub(crate) later: Later,
    }

    /// The answer that a scripted source gives from its second stop on.
    pub(crate) type Later = Arc<Mutex<Option<Result<(), CaptureError>>>>;

    /// A call that a scripted source makes with its context.
    pub(crate) type Hook = Box<dyn FnMut(&SourceContext)>;

    impl CaptureSource for Scripted {
        fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
            self.calls.push(format!("begin {}", self.name.as_str()));
            self.floors
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(format!("{:?}", ctx.floor()));
            if let Some(hook) = self.on_begin.as_mut() {
                hook(&ctx);
            }
            let answer = self.begin.clone();
            if answer == Ok(Started::Running) {
                for event in self.script.drain(..) {
                    if ctx.sink().emit_event(event) == Emitted::Refused {
                        return Err(CaptureError::ThreadEnded);
                    }
                }
                *self.ctx.lock().unwrap_or_else(PoisonError::into_inner) = Some(ctx);
            }
            answer
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            self.calls.push(format!("stop {}", self.name.as_str()));
            let held = self
                .ctx
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let (Some(hook), Some(ctx)) = (self.on_stop.as_mut(), held.as_ref()) {
                hook(ctx);
            }
            if is_a_stop(&self.stop) {
                *self.ctx.lock().unwrap_or_else(PoisonError::into_inner) = None;
            }
            let answer = self.stop.clone();
            if let Some(later) = self
                .later
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                self.stop = later;
            }
            answer
        }

        fn health(&self) -> SourceHealth {
            self.health.read()
        }
    }

    /// Makes scripted sources that share one call log; `adjust` sets each one
    /// up by its name before it is handed out.
    pub(crate) struct Factory {
        pub(crate) calls: Calls,
        pub(crate) floors: Arc<Mutex<Vec<String>>>,
        pub(crate) made: Vec<SourceName>,
        pub(crate) health: Vec<(SourceName, Health)>,
        pub(crate) ctx: Vec<(SourceName, Arc<Mutex<Option<SourceContext>>>)>,
        pub(crate) later: Vec<(SourceName, Later)>,
        pub(crate) adjust: Box<dyn FnMut(&mut Scripted)>,
        /// The input sources made read as a tap that asked for the pointer
        /// types alone: the system does not allow listening to keys.
        pub(crate) listen_denied: bool,
        /// The supervisor's latch, as `SystemSources::refuse_keys` set it.
        pub(crate) keys_refused: bool,
    }

    impl Factory {
        pub(crate) fn new() -> Factory {
            Factory::adjusting(|_| {})
        }

        pub(crate) fn adjusting(adjust: impl FnMut(&mut Scripted) + 'static) -> Factory {
            Factory {
                calls: Calls::default(),
                floors: Arc::default(),
                made: Vec::new(),
                health: Vec::new(),
                ctx: Vec::new(),
                later: Vec::new(),
                adjust: Box::new(adjust),
                listen_denied: false,
                keys_refused: false,
            }
        }

        /// Makes the last source made under `name` answer `answer` from its
        /// second stop on.
        pub(crate) fn on_second_stop(
            &mut self,
            name: SourceName,
            answer: Result<(), CaptureError>,
        ) {
            if let Some((_, later)) = self.later.iter().rev().find(|(made, _)| *made == name) {
                *later.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
            }
        }

        /// The health handle of the last source made under `name`.
        pub(crate) fn health_of(&self, name: SourceName) -> Health {
            self.health
                .iter()
                .rev()
                .find(|(made, _)| *made == name)
                .map(|(_, health)| health.clone())
                .unwrap_or_default()
        }
    }

    impl SourceFactory for Factory {
        fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
            let mut source = Scripted {
                name,
                calls: self.calls.clone(),
                begin: Ok(Started::Running),
                stop: Ok(()),
                health: Health::default(),
                script: Vec::new(),
                floors: self.floors.clone(),
                ctx: Arc::default(),
                on_begin: None,
                on_stop: None,
                later: Arc::default(),
            };
            if name == SourceName::Input && (self.listen_denied || self.keys_refused) {
                source.health.set(SourceState::Impaired {
                    reason: crate::source::HealthReason::Permission,
                });
            }
            (self.adjust)(&mut source);
            self.made.push(name);
            self.health.push((name, source.health.clone()));
            self.ctx.push((name, source.ctx.clone()));
            self.later.push((name, source.later.clone()));
            Some(Box::new(source))
        }
    }

    fn ctx_with(switches: Switches) -> SourceContext {
        let (handle, _writer) = sink::channel(8);
        SourceContext::with_floor(
            handle,
            FloorHandle::new(None, Arc::new(NoLineage), switches),
        )
    }

    fn timed() -> RunKind {
        RunKind::Timed {
            key_class_flag: false,
        }
    }

    fn switches_with(switch: Switch, on: bool) -> Switches {
        let mut switches = Switches::default();
        switches.set(switch, on);
        switches
    }

    #[test]
    fn input_stops_before_screen_in_every_stop_sequence() {
        let mut every = Switches::default();
        every.set(Switch::ScreenActivity, true);
        for platform in [Platform::Macos, Platform::Windows] {
            let kind = RunKind::Timed {
                key_class_flag: true,
            };
            let wanted = starting(platform, kind, every);
            let mut factory = Factory::new();
            let mut sources = RunningSources::new(platform);
            sources.start(&mut factory, &ctx_with(every), &wanted);
            let answers = sources.stop();
            let stopped: Vec<SourceName> = answers.iter().map(|(name, _)| *name).collect();
            let expected: Vec<SourceName> = stop_order(platform)
                .iter()
                .copied()
                .filter(|name| wanted.contains(name))
                .collect();
            assert_eq!(stopped, expected, "{platform:?}");
            let calls: Vec<String> = factory.calls.of("stop ");
            let wanted_calls: Vec<String> = expected
                .iter()
                .map(|name| format!("stop {}", name.as_str()))
                .collect();
            assert_eq!(calls, wanted_calls, "{platform:?}");
        }
        assert_eq!(
            MACOS_STOP_ORDER,
            [
                SourceName::Input,
                SourceName::Foreground,
                SourceName::Screen
            ]
        );
        let input = MACOS_STOP_ORDER
            .iter()
            .position(|name| *name == SourceName::Input);
        let screen = MACOS_STOP_ORDER
            .iter()
            .position(|name| *name == SourceName::Screen);
        assert!(
            input < screen,
            "input stops at {input:?}, screen at {screen:?}"
        );
    }

    #[test]
    fn the_screen_source_is_not_started_with_screen_activity_off() {
        for platform in [Platform::Macos, Platform::Windows] {
            let wanted = starting(platform, timed(), Switches::default());
            assert!(
                !wanted.contains(&SourceName::Screen),
                "{platform:?} {wanted:?}"
            );
            let mut factory = Factory::new();
            let mut sources = RunningSources::new(platform);
            sources.start(&mut factory, &ctx_with(Switches::default()), &wanted);
            assert!(!factory.made.contains(&SourceName::Screen), "{platform:?}");
            assert_eq!(
                sources.state(SourceName::Screen),
                Some((&SourceState::Off, false)),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_screen_source_starts_with_screen_activity_on() {
        let on = switches_with(Switch::ScreenActivity, true);
        for platform in [Platform::Macos, Platform::Windows] {
            for kind in [timed(), RunKind::Service] {
                let wanted = starting(platform, kind, on);
                assert!(
                    wanted.contains(&SourceName::Screen),
                    "{platform:?} {kind:?}"
                );
                let mut factory = Factory::new();
                let mut sources = RunningSources::new(platform);
                sources.start(&mut factory, &ctx_with(on), &wanted);
                assert_eq!(
                    sources.state(SourceName::Screen),
                    Some((&SourceState::Running, false)),
                    "{platform:?} {kind:?}"
                );
            }
        }
    }

    #[test]
    fn the_windows_set_starts_the_keyboard_source_only_under_the_timed_run_flag() {
        let cases = [
            ("the default set", timed(), Switches::default(), false),
            (
                "a service run whose state has key_class on",
                RunKind::Service,
                switches_with(Switch::KeyClass, true),
                false,
            ),
            (
                "a timed run with the flag",
                RunKind::Timed {
                    key_class_flag: true,
                },
                switches_with(Switch::KeyClass, true),
                true,
            ),
        ];
        for (case, kind, switches, expected) in cases {
            let wanted = starting(Platform::Windows, kind, switches);
            let mut factory = Factory::new();
            let mut sources = RunningSources::new(Platform::Windows);
            sources.start(&mut factory, &ctx_with(switches), &wanted);
            assert_eq!(
                factory.made.contains(&SourceName::Keyboard),
                expected,
                "{case}: {wanted:?}"
            );
            let state = sources
                .state(SourceName::Keyboard)
                .map(|(state, _)| state.clone());
            let wanted_state = if expected {
                SourceState::Running
            } else {
                SourceState::Off
            };
            assert_eq!(state, Some(wanted_state), "{case}");
        }
        let flag_off = RunKind::Timed {
            key_class_flag: true,
        };
        let off = switches_with(Switch::KeyClass, false);
        assert!(!starting(Platform::Windows, flag_off, off).contains(&SourceName::Keyboard));
    }

    #[test]
    fn every_source_receives_the_exclusion_root_the_list_and_the_default_switches() {
        let floor = FloorHandle::new(
            Some(ProcessId::new(4242)),
            Arc::new(NoLineage),
            Switches::default(),
        );
        let (handle, _writer) = sink::channel(8);
        let ctx = SourceContext::with_floor(handle, floor.clone());
        for platform in [Platform::Macos, Platform::Windows] {
            let wanted = starting(platform, timed(), Switches::default());
            let mut factory = Factory::new();
            let mut sources = RunningSources::new(platform);
            sources.start(&mut factory, &ctx, &wanted);
            let floors = factory
                .floors
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            assert_eq!(floors.len(), wanted.len(), "{platform:?}");
            for given in &floors {
                assert_eq!(given, &format!("{floor:?}"), "{platform:?}");
                assert!(given.contains("Some(ProcessId(4242))"), "{given}");
            }
        }
        assert_eq!(ctx.floor().switches(), Switches::default());
    }

    #[test]
    fn system_audio_and_microphone_read_off_with_capture_running() {
        let mut every = Switches::default();
        for switch in Switch::ALL {
            every.set(switch, true);
        }
        let wanted = starting(Platform::Macos, timed(), every);
        let mut factory = Factory::new();
        let mut sources = RunningSources::new(Platform::Macos);
        sources.start(&mut factory, &ctx_with(every), &wanted);
        assert_eq!(
            sources.state(SourceName::Foreground),
            Some((&SourceState::Running, false))
        );
        for name in [SourceName::SystemAudio, SourceName::Microphone] {
            assert_eq!(
                sources.state(name),
                Some((&SourceState::Off, false)),
                "{name:?}"
            );
            assert!(!factory.made.contains(&name), "{name:?} was made");
        }
    }

    #[test]
    fn no_source_for_system_audio_microphone_or_camera_ever_emits() {
        let mut every = Switches::default();
        for switch in Switch::ALL {
            every.set(switch, true);
        }
        for platform in [Platform::Macos, Platform::Windows] {
            for kind in [
                RunKind::Timed {
                    key_class_flag: true,
                },
                RunKind::Service,
            ] {
                let wanted = starting(platform, kind, every);
                for modality in [
                    Modality::SystemAudio,
                    Modality::Microphone,
                    Modality::Camera,
                ] {
                    if let Some(name) = source_of(modality, platform) {
                        assert!(!wanted.contains(&name), "{platform:?} {kind:?} {name:?}");
                    }
                }
                let (handle, writer) = sink::channel(64);
                let ctx = SourceContext::with_floor(
                    handle,
                    FloorHandle::new(None, Arc::new(NoLineage), every),
                );
                let mut factory = Factory::adjusting(|source| {
                    source.script = vec![crate::daemon::model_store::foreground_now()];
                });
                let mut sources = RunningSources::new(platform);
                sources.start(&mut factory, &ctx, &wanted);
                drop(ctx);
                sources.stop();
                let mut kinds = Vec::new();
                while let Ok(received) = writer.try_recv() {
                    kinds.push(received.event().modality);
                }
                assert_eq!(kinds.len(), wanted.len(), "{platform:?} {kind:?}");
                assert!(
                    kinds.iter().all(|kind| !matches!(
                        kind,
                        Modality::SystemAudio | Modality::Microphone | Modality::Camera
                    )),
                    "{kinds:?}"
                );
            }
        }
    }

    #[test]
    fn a_source_that_does_not_stop_is_held_and_no_second_one_begins() {
        let mut factory = Factory::adjusting(|source| {
            if source.name == SourceName::Input {
                source.stop = Err(CaptureError::Timeout {
                    stage: "input_tap_join",
                    ms: 1_000,
                });
            }
        });
        let ctx = ctx_with(Switches::default());
        let mut sources = RunningSources::new(Platform::Macos);
        let wanted = starting(Platform::Macos, timed(), Switches::default());
        sources.start(&mut factory, &ctx, &wanted);
        sources.stop();
        assert!(matches!(
            sources.state(SourceName::Input),
            Some((
                SourceState::Failed {
                    reason: HealthReason::Timeout,
                    ..
                },
                true
            ))
        ));
        assert!(sources.holds(SourceName::Input));
        assert_eq!(
            sources.start_one(&mut factory, &ctx, SourceName::Input),
            None
        );
        assert_eq!(factory.calls.of("begin input").len(), 1);
        let stopped = sources.stop();
        assert_eq!(
            stopped.len(),
            1,
            "the held source is asked again: {stopped:?}"
        );
    }

    #[test]
    fn a_running_source_reads_running_only_after_it_installed_its_callback() {
        let mut factory = Factory::adjusting(|source| {
            if source.name == SourceName::Screen {
                source.begin = Ok(Started::Off);
            }
            if source.name == SourceName::Input {
                source.begin = Err(CaptureError::Start("the tap was not made".to_owned()));
            }
        });
        let on = switches_with(Switch::ScreenActivity, true);
        let mut sources = RunningSources::new(Platform::Macos);
        let wanted = starting(Platform::Macos, timed(), on);
        sources.start(&mut factory, &ctx_with(on), &wanted);
        assert_eq!(
            sources.state(SourceName::Foreground),
            Some((&SourceState::Running, false))
        );
        assert_eq!(
            sources.state(SourceName::Screen),
            Some((&SourceState::Off, false))
        );
        assert!(matches!(
            sources.state(SourceName::Input),
            Some((
                SourceState::Failed {
                    reason: HealthReason::OsError,
                    ..
                },
                false
            ))
        ));
    }
}
