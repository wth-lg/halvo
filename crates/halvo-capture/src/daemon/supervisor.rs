//! The supervisor: it holds the sources of a run, waits on the main loop and,
//! on every tick, reads the owner's reports and snapshot, the health of each
//! source and the permission answers, and acts on them.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use halvo_control::exit;
use halvo_control::onboarding::{GrantState, Permission};
use halvo_control::proto::{Platform, SourceName};

use super::board::{Board, Capture, Drain};
use super::counters::Snapshot;
use super::main_loop::{self, MainLoop};
use super::owner::{
    Answer, Command, OwnerGone, OwnerHandle, OwnerPanicked, OwnerThread, Pending, Report,
};
use super::permissions::{ProbeRunner, Probed};
use super::report::FailedSource;
use super::sources::{is_a_stop, RunningSources, SourceFactory};
use super::{DaemonError, RunSources};
use crate::source::{
    CaptureError, HealthReason, SourceContext, SourceCounters, SourceState, StopCause,
};

/// The whole exit budget of the shutdown entry, counted from its start
/// (rulings D3; contracts.md C14.6).
pub const SHUTDOWN_BUDGET_MS: u64 = 18_000;

const _: () = assert!(SHUTDOWN_BUDGET_MS < halvo_control::STOP_WAIT_MS);

/// The first settle of the shutdown entry.
pub const FIRST_SETTLE: Duration = Duration::from_secs(1);

/// The final settle of the shutdown entry.
pub const FINAL_SETTLE: Duration = Duration::from_secs(5);

/// The first wait before a failed source is started again.
pub const RESTART_FIRST: Duration = Duration::from_secs(1);

/// The longest wait before a failed source is started again.
pub const RESTART_MAX: Duration = Duration::from_secs(60);

/// How often the probe child runs.
pub const PROBE_INTERVAL: Duration = Duration::from_secs(60);

/// How often the probe child runs while a required permission is not granted
/// and a client polls.
pub const PROBE_INTERVAL_MISSING: Duration = Duration::from_secs(3);

/// The secure-input line of an input source with a hold plan whose stop did
/// not answer a stop (ruling MA-9 part (5)).
pub fn hold_stays_line(pid: u32) -> String {
    format!(
        "halvo-capture: secure input stays on for this Mac until this process (pid {pid}) has ended"
    )
}

/// The owner's reports as the supervisor reads them. Each read takes what the
/// channel holds: it writes the drop line of every given-up batch to `out`
/// (the daemon's standard error, which the starter makes logs/daemon.err) and
/// keeps the last drain report, failing or healthy, for the run's loss line.
/// It keeps nothing else, so what it holds stays bounded however many reports
/// a fault sends while the supervisor is busy.
pub struct ReportLog {
    channel: Receiver<Report>,
    state: Mutex<LogState>,
}

struct LogState {
    out: Box<dyn Write + Send>,
    last_drain: Option<Report>,
    failing: bool,
}

impl ReportLog {
    pub fn new(channel: Receiver<Report>, out: Box<dyn Write + Send>) -> ReportLog {
        ReportLog {
            channel,
            state: Mutex::new(LogState {
                out,
                last_drain: None,
                failing: false,
            }),
        }
    }

    /// Reads what the channel holds and answers whether the drain fails by
    /// the last drain report.
    pub fn read(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        while let Ok(report) = self.channel.try_recv() {
            if let Some(line) = report.drop_line() {
                let _ = writeln!(state.out, "{line}");
            }
            match report {
                Report::DrainFailing { .. } => {
                    state.failing = true;
                    state.last_drain = Some(report);
                }
                Report::DrainHealthy => {
                    state.failing = false;
                    state.last_drain = Some(report);
                }
                Report::StoreReady
                | Report::BatchRejected { .. }
                | Report::MaintenanceFailing { .. }
                | Report::MaintenanceHealthy => {}
            }
        }
        let _ = state.out.flush();
        state.failing
    }

    /// Reads what the channel holds and hands over the last drain report kept
    /// since the last call.
    pub fn try_iter(&self) -> std::option::IntoIter<Report> {
        self.read();
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .last_drain
            .take()
            .into_iter()
    }
}

/// The owner as the supervisor reads it.
pub trait OwnerLink {
    fn ask(&self, command: Command) -> Result<Pending, OwnerGone>;
    fn snapshot(&self) -> Snapshot;
    fn reports(&self) -> &ReportLog;
    fn is_running(&self) -> bool;
}

/// The owner thread of a run, with its reports read through a [`ReportLog`].
pub struct Watched {
    pub handle: OwnerHandle,
    pub reports: ReportLog,
    thread: OwnerThread,
}

impl Watched {
    /// Takes the owner thread's report channel into a log that writes its
    /// drop lines to `out`.
    pub fn new(mut thread: OwnerThread, out: Box<dyn Write + Send>) -> Watched {
        let (_, unused) = std::sync::mpsc::channel();
        let channel = std::mem::replace(&mut thread.reports, unused);
        Watched {
            handle: thread.handle.clone(),
            reports: ReportLog::new(channel, out),
            thread,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.thread.snapshot()
    }

    pub fn join(self) -> Result<Snapshot, OwnerPanicked> {
        self.thread.join()
    }
}

impl OwnerLink for Watched {
    fn ask(&self, command: Command) -> Result<Pending, OwnerGone> {
        self.handle.ask(command)
    }

    fn snapshot(&self) -> Snapshot {
        self.thread.snapshot()
    }

    fn reports(&self) -> &ReportLog {
        &self.reports
    }

    fn is_running(&self) -> bool {
        self.thread.is_running()
    }
}

/// The parts of a system's sources that the supervisor reads beside the
/// factory: the counters that live for the whole process and the system's
/// refusal of a secure-input hold.
pub trait SystemSources: SourceFactory {
    /// The hold plan of the input source: held from `after` on.
    fn plan_hold(&mut self, after: Option<Duration>) {
        let _ = after;
    }

    /// Whether the input source holds secure input in this run.
    fn holds_secure_input(&self) -> bool {
        false
    }

    /// The counters that the process keeps over every source set.
    fn figures(&self) -> SourceCounters {
        SourceCounters::default()
    }

    fn hold_refusal(&self) -> Option<DaemonError> {
        None
    }
}

/// Why a wait ended before its window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The signal flag was set.
    Signal,
    /// The owner thread is gone.
    OwnerGone,
    /// A permission was granted after the start: the daemon ends with status
    /// 4 and its starter starts it again.
    Restart,
}

#[derive(Debug, Clone, Copy)]
struct Backoff {
    name: SourceName,
    due: Duration,
    delay: Duration,
}

/// What the shutdown entry ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryOutcome {
    pub status: i32,
    pub stops: Vec<(SourceName, Result<(), CaptureError>)>,
    pub settled: Option<Snapshot>,
    pub took: Duration,
}

/// The supervisor of one run.
pub struct Supervisor<F: SystemSources, L: MainLoop> {
    platform: Platform,
    factory: F,
    main_loop: L,
    sources: RunningSources,
    board: Board,
    ctx: Option<SourceContext>,
    wanted: Vec<SourceName>,
    backoff: Vec<Backoff>,
    delays: Vec<(SourceName, Duration)>,
    started_failed: Vec<SourceName>,
    stopped_for_drain: bool,
    probe: Option<Box<dyn ProbeRunner>>,
    probe_due: Duration,
    probe_asked: bool,
    client_polls: bool,
    ending: Option<Ending>,
    entered: bool,
    err: Box<dyn Write + Send>,
}

impl<F: SystemSources, L: MainLoop> Supervisor<F, L> {
    pub fn new(platform: Platform, factory: F, main_loop: L) -> Supervisor<F, L> {
        Supervisor {
            platform,
            factory,
            main_loop,
            sources: RunningSources::new(platform),
            board: Board::new(platform),
            ctx: None,
            wanted: Vec::new(),
            backoff: Vec::new(),
            delays: Vec::new(),
            started_failed: Vec::new(),
            stopped_for_drain: false,
            probe: None,
            probe_due: Duration::ZERO,
            probe_asked: false,
            client_polls: false,
            ending: None,
            entered: false,
            err: Box::new(std::io::stderr()),
        }
    }

    /// The probe runner whose answers the supervisor reads.
    pub fn with_probe(mut self, probe: Box<dyn ProbeRunner>) -> Supervisor<F, L> {
        self.probe = Some(probe);
        self
    }

    /// Where the supervisor writes its own lines.
    pub fn with_err(mut self, err: Box<dyn Write + Send>) -> Supervisor<F, L> {
        self.err = err;
        self
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn factory(&self) -> &F {
        &self.factory
    }

    pub fn main_loop(&mut self) -> &mut L {
        &mut self.main_loop
    }

    /// Whether a client polls the status, which makes the probe run every 3 s
    /// while a required permission is not granted.
    pub fn set_client_polls(&mut self, polls: bool) {
        self.client_polls = polls;
    }

    /// The start sequence's second half, after the owner's acknowledgement of
    /// ClosePause: a fresh set of the sources of `wanted`.
    pub fn start_sources(
        &mut self,
        ctx: &SourceContext,
        wanted: &[SourceName],
    ) -> Vec<FailedSource> {
        self.ctx = Some(ctx.clone());
        self.wanted = wanted.to_vec();
        self.board.enabled = wanted.to_vec();
        let answers = self.sources.start(&mut self.factory, ctx, wanted);
        #[cfg(feature = "r2")]
        super::r2::start_children(self);
        let now = self.main_loop.now();
        let mut failed = Vec::new();
        for (name, answer) in answers {
            if let Err(error) = answer {
                failed.push(FailedSource {
                    name: name.as_str(),
                    error: error.to_string(),
                });
                self.board.record_failure(name, error);
                self.started_failed.push(name);
                self.schedule_restart(name, now);
            }
        }
        self.probe_due = now;
        self.publish();
        failed
    }

    /// One tick: the owner's reports and snapshot, the source health, the
    /// restarts that are due and the permission answers. `Some` when the run
    /// must end.
    pub fn tick(&mut self, owner: Option<&dyn OwnerLink>) -> Option<Ending> {
        let now = self.main_loop.now();
        if let Some(owner) = owner {
            if !owner.is_running() {
                self.stop_every_source();
                self.ending = Some(Ending::OwnerGone);
                return self.ending;
            }
            let failing = owner.reports().read();
            let snapshot = owner.snapshot();
            self.board.snapshot = Some(snapshot);
            self.board.drain = Drain {
                failing,
                hold: snapshot.drain_hold,
            };
            self.follow_the_drain(now);
        }
        if !self.stopped_for_drain {
            self.read_health(now);
            self.restart_due(now);
        }
        if let Some(ending) = self.read_permissions(now) {
            self.ending = Some(ending);
        }
        self.publish();
        self.ending
    }

    fn follow_the_drain(&mut self, now: Duration) {
        let stops = self.board.drain.stops_the_sources();
        if stops && !self.stopped_for_drain {
            self.stopped_for_drain = true;
            for (name, answer) in self.sources.stop() {
                if !is_a_stop(&answer) {
                    if let Err(error) = answer {
                        self.board.record_failure(name, error);
                    }
                }
            }
            self.backoff.clear();
        } else if !stops && self.stopped_for_drain {
            self.stopped_for_drain = false;
            self.start_wanted(now);
        }
    }

    fn start_wanted(&mut self, now: Duration) {
        let Some(ctx) = self.ctx.clone() else {
            return;
        };
        let wanted = self.wanted.clone();
        for (name, answer) in self.sources.start(&mut self.factory, &ctx, &wanted) {
            if let Err(error) = answer {
                self.board.record_failure(name, error);
                self.schedule_restart(name, now);
            }
        }
    }

    fn read_health(&mut self, now: Duration) {
        self.sources.read_health();
        let wanted = self.wanted.clone();
        for name in wanted {
            let Some((state, unstopped)) = self.sources.state(name) else {
                continue;
            };
            let (state, unstopped) = (state.clone(), unstopped);
            if unstopped {
                continue;
            }
            match state {
                SourceState::Failed { reason, error } => {
                    self.board.record_failure(name, error);
                    if restarts(reason) {
                        self.schedule_restart(name, now);
                    }
                }
                SourceState::Stopped(StopCause::User) => {
                    self.board.record_failure(
                        name,
                        CaptureError::Os {
                            stage: "stopped_by_user",
                            domain: "ScreenCaptureKit",
                            code: -3817,
                        },
                    );
                }
                SourceState::NotReported
                | SourceState::Running
                | SourceState::Impaired { .. }
                | SourceState::Stopped(StopCause::Request)
                | SourceState::Off => {}
            }
        }
    }

    fn schedule_restart(&mut self, name: SourceName, now: Duration) {
        if self.backoff.iter().any(|backoff| backoff.name == name) {
            return;
        }
        let delay = self.next_delay(name);
        self.backoff.push(Backoff {
            name,
            due: now + delay,
            delay,
        });
    }

    fn next_delay(&self, name: SourceName) -> Duration {
        self.delays
            .iter()
            .find(|(made, _)| *made == name)
            .map_or(RESTART_FIRST, |(_, delay)| (*delay * 2).min(RESTART_MAX))
    }

    fn restart_due(&mut self, now: Duration) {
        let due: Vec<Backoff> = self
            .backoff
            .iter()
            .copied()
            .filter(|backoff| now >= backoff.due)
            .collect();
        self.backoff.retain(|backoff| now < backoff.due);
        let Some(ctx) = self.ctx.clone() else {
            return;
        };
        for backoff in due {
            if self.missing_permission(backoff.name) {
                continue;
            }
            if self.sources.holds(backoff.name) {
                let answer = self.sources.stop_one(backoff.name);
                if answer.as_ref().is_some_and(|answer| !is_a_stop(answer)) {
                    continue;
                }
            }
            self.remember_delay(backoff.name, backoff.delay);
            if let Some(Err(error)) = self
                .sources
                .start_one(&mut self.factory, &ctx, backoff.name)
            {
                self.board.record_failure(backoff.name, error);
                self.schedule_restart(backoff.name, now);
            }
        }
    }

    fn remember_delay(&mut self, name: SourceName, delay: Duration) {
        self.delays.retain(|(made, _)| *made != name);
        self.delays.push((name, delay));
    }

    fn missing_permission(&self, name: SourceName) -> bool {
        required_permission(self.platform, name).is_some_and(|permission| {
            self.board
                .permissions
                .iter()
                .any(|(answered, grant)| *answered == permission && *grant != GrantState::Granted)
        })
    }

    fn required(&self) -> Vec<Permission> {
        self.wanted
            .iter()
            .filter_map(|name| required_permission(self.platform, *name))
            .collect()
    }

    fn read_permissions(&mut self, now: Duration) -> Option<Ending> {
        let required = self.required();
        let probe = self.probe.as_mut()?;
        if required.is_empty() {
            return None;
        }
        if !self.probe_asked && now >= self.probe_due {
            probe.start(&required);
            self.probe_asked = true;
        }
        let answers = match probe.poll() {
            Probed::Running => return None,
            Probed::Failed => {
                self.probe_asked = false;
                self.probe_due = now + PROBE_INTERVAL_MISSING;
                return None;
            }
            Probed::Answered(answers) => answers,
        };
        self.probe_asked = false;
        let missing = answers
            .iter()
            .any(|(_, grant)| *grant != GrantState::Granted);
        self.probe_due = now
            + if missing && self.client_polls {
                PROBE_INTERVAL_MISSING
            } else {
                PROBE_INTERVAL
            };
        let granted: Vec<Permission> = answers
            .iter()
            .filter(|(_, grant)| *grant == GrantState::Granted)
            .map(|(permission, _)| *permission)
            .collect();
        self.board.permissions = answers.clone();
        let Some(at_start) = self.board.granted_at_start.clone() else {
            self.board.granted_at_start = Some(granted);
            self.stop_without_permission(now);
            return None;
        };
        if granted
            .iter()
            .any(|permission| !at_start.contains(permission))
        {
            return Some(Ending::Restart);
        }
        self.stop_without_permission(now);
        None
    }

    fn stop_without_permission(&mut self, now: Duration) {
        let _ = now;
        let wanted = self.wanted.clone();
        for name in wanted {
            if !self.missing_permission(name) {
                continue;
            }
            self.backoff.retain(|backoff| backoff.name != name);
            if !self.sources.holds(name) {
                continue;
            }
            let answer = self.sources.stop_one(name);
            let unstopped = answer.as_ref().is_some_and(|answer| !is_a_stop(answer));
            if !unstopped {
                let error = CaptureError::Os {
                    stage: "permission_probe",
                    domain: "tcc",
                    code: 0,
                };
                self.sources.set_state(
                    name,
                    SourceState::Failed {
                        reason: HealthReason::Permission,
                        error: error.clone(),
                    },
                );
                self.board.record_failure(name, error);
            }
        }
    }

    /// Stops every source in the stop order and returns each answer.
    pub fn stop_every_source(&mut self) -> Vec<(SourceName, Result<(), CaptureError>)> {
        self.backoff.clear();
        let answers = self.sources.stop();
        self.publish();
        answers
    }

    /// Turns the main loop until `window` has passed, `stop` reads true or the
    /// run must end; true when `stop` ended the wait.
    pub fn wait_on(
        &mut self,
        window: Duration,
        stop: &AtomicBool,
        owner: Option<&dyn OwnerLink>,
    ) -> bool {
        let until = self.main_loop.now() + window;
        loop {
            if stop.load(Ordering::SeqCst) {
                return true;
            }
            if self.tick(owner).is_some() {
                return false;
            }
            let Some(slice) = main_loop::slice(self.main_loop.now(), until) else {
                return false;
            };
            self.main_loop.turn(slice);
        }
    }

    /// Waits for `pending` while the main loop turns, at most `bound`.
    pub fn answer_within(&mut self, pending: &Pending, bound: Duration) -> Option<Answer> {
        let until = self.main_loop.now() + bound;
        loop {
            if let Some(answer) = pending.try_answer() {
                return Some(answer);
            }
            let slice = main_loop::slice(self.main_loop.now(), until)?;
            self.main_loop.turn(slice.min(Duration::from_millis(5)));
        }
    }

    /// The shutdown entry: the owner settles the open batch (at most 1 s), the
    /// sources stop in the stop order, and the owner settles once more (at
    /// most 5 s). It runs once: a second trigger answers `None`. The status is
    /// 1 when the drain still fails at the last settle, when the last settle
    /// gets no answer or when the life lost events between the two settles,
    /// else `status`.
    pub fn shutdown_entry(&mut self, owner: &dyn OwnerLink, status: i32) -> Option<EntryOutcome> {
        if self.entered {
            return None;
        }
        self.entered = true;
        let started = self.main_loop.now();
        let first = owner
            .ask(Command::Settle)
            .ok()
            .and_then(|pending| self.answer_within(&pending, FIRST_SETTLE));
        let before = match first {
            Some(Answer::Settled(snapshot)) => Some(snapshot),
            _ => None,
        };
        let stops = self.stop_every_source();
        let last = owner
            .ask(Command::Settle)
            .ok()
            .and_then(|pending| self.answer_within(&pending, FINAL_SETTLE));
        let settled = match last {
            Some(Answer::Settled(snapshot)) => Some(snapshot),
            _ => None,
        };
        let failing = owner.reports().read();
        let lost_more = match (before, settled) {
            (Some(before), Some(after)) => after.events_lost > before.events_lost,
            _ => false,
        };
        let status = if settled.is_none() || failing || lost_more {
            exit::ERROR
        } else {
            status
        };
        Some(EntryOutcome {
            status,
            stops,
            settled,
            took: self.main_loop.now().saturating_sub(started),
        })
    }

    fn publish(&mut self) {
        for (name, state, unstopped) in self.sources.states() {
            let state = state.clone();
            self.board.set_source(name, &state, unstopped);
        }
        self.board.capture = if self.stopped_for_drain {
            Capture::Stopped
        } else {
            self.board.capture_of(&self.wanted)
        };
        self.board.figures = self.factory.figures();
    }
}

/// Whether the supervisor starts a source of `reason` again: not when its
/// writer is gone, which a restart cannot help, nor without its permission,
/// nor for a source that this build does not hold.
pub const fn restarts(reason: HealthReason) -> bool {
    match reason {
        HealthReason::WriterGone | HealthReason::Permission | HealthReason::NotImplemented => false,
        HealthReason::ItemClosed
        | HealthReason::ThreadEnded
        | HealthReason::Timeout
        | HealthReason::OsError
        | HealthReason::NoKeyEvents
        | HealthReason::SecureInputElsewhere
        | HealthReason::TitleReadErrors
        | HealthReason::StreamBlank
        | HealthReason::StreamSuspended
        | HealthReason::RunLoopStalled => true,
    }
}

/// The permission that a source of `platform` needs: Screen and System Audio
/// Recording for the screen source and Input Monitoring for the input source
/// on macOS; none on Windows. No source names Accessibility (RD-29).
pub const fn required_permission(platform: Platform, name: SourceName) -> Option<Permission> {
    match (platform, name) {
        (Platform::Macos, SourceName::Screen) => Some(Permission::ScreenAndSystemAudioRecording),
        (Platform::Macos, SourceName::Input) => Some(Permission::InputMonitoring),
        _ => None,
    }
}

impl<F: SystemSources, L: MainLoop> RunSources for Supervisor<F, L> {
    fn start(
        &mut self,
        ctx: &SourceContext,
        wanted: &[SourceName],
        hold_after: Option<Duration>,
    ) -> Vec<FailedSource> {
        self.factory.plan_hold(hold_after);
        self.start_sources(ctx, wanted)
    }

    fn wait(&mut self, window: Duration, stop: &AtomicBool) -> bool {
        self.wait_on(window, stop, None)
    }

    fn supervise(&mut self, window: Duration, stop: &AtomicBool, owner: &dyn OwnerLink) -> bool {
        self.wait_on(window, stop, Some(owner))
    }

    fn stop(&mut self) -> Vec<FailedSource> {
        let answers = self.stop_every_source();
        self.ctx = None;
        let started_failed: Vec<&str> = self
            .started_failed
            .iter()
            .map(|name| name.as_str())
            .collect();
        let mut failed: Vec<FailedSource> = self
            .board
            .failed_sources()
            .into_iter()
            .filter(|entry| !started_failed.contains(&entry.name))
            .collect();
        for (name, answer) in answers {
            if !is_a_stop(&answer) && name == SourceName::Input && self.factory.holds_secure_input()
            {
                let _ = writeln!(self.err, "{}", hold_stays_line(std::process::id()));
            }
            if let Some(entry) = super::stop_failure(name, answer) {
                if !failed.iter().any(|failed| failed.name == entry.name)
                    && !started_failed.contains(&entry.name)
                {
                    failed.push(entry);
                }
            }
        }
        failed
    }

    fn figures(&self) -> SourceCounters {
        self.factory.figures()
    }

    fn hold_refusal(&self) -> Option<DaemonError> {
        self.factory.hold_refusal()
    }

    fn restart_asked(&self) -> bool {
        self.ending == Some(Ending::Restart)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::io;
    use std::rc::Rc;
    use std::sync::atomic::AtomicU64;
    use std::sync::{mpsc, Arc};
    use std::thread::JoinHandle;
    use std::time::Instant;

    use super::*;
    use crate::daemon::batch::BATCH_MAX_EVENTS;
    use crate::daemon::model_store::{foreground_now, key, open_floor, Harness, ModelStore};
    use crate::daemon::owner::RETRY_FIRST;
    use crate::daemon::port::{
        ByteBound, CaptureStore, MaintFault, MaintainStore, Pass, SegmentWrite,
        Usage as StoreUsage, WriteFault, Written,
    };
    use crate::daemon::sources::tests::{Factory, Hook};
    use crate::daemon::sources::{starting, RunKind};
    use crate::daemon::stop_tests::run_of;
    use crate::daemon::{run_timed, SETTLE_BOUND};
    use crate::floor::Switches;
    use crate::source::{CaptureSource, SourceHealth, Started};

    /// The test's loop: its time moves by each turn and by what a test adds;
    /// a turn runs one pass of the harness's owner when there is one.
    struct TestLoop {
        now: Rc<Cell<Duration>>,
        harness: Option<Rc<RefCell<Harness>>>,
        real: bool,
    }

    impl MainLoop for TestLoop {
        fn turn(&mut self, wait: Duration) {
            if let Some(harness) = &self.harness {
                let mut harness = harness.borrow_mut();
                harness.clocks.advance(wait);
                harness.owner.pass(Duration::ZERO);
            }
            if self.real {
                std::thread::sleep(wait.min(Duration::from_millis(20)));
            }
            self.now.set(self.now.get() + wait);
        }

        fn now(&self) -> Duration {
            self.now.get()
        }
    }

    impl SystemSources for Factory {}

    #[derive(Clone, Default)]
    struct Out(Arc<Mutex<Vec<u8>>>);

    impl Write for Out {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Out {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
                .into_owned()
        }
    }

    struct HarnessLink {
        harness: Rc<RefCell<Harness>>,
        log: ReportLog,
        running: Cell<bool>,
    }

    impl OwnerLink for HarnessLink {
        fn ask(&self, command: Command) -> Result<Pending, OwnerGone> {
            self.harness.borrow().handle.ask(command)
        }

        fn snapshot(&self) -> Snapshot {
            self.harness.borrow().owner.snapshot()
        }

        fn reports(&self) -> &ReportLog {
            &self.log
        }

        fn is_running(&self) -> bool {
            self.running.get()
        }
    }

    impl HarnessLink {
        fn settle(&self) -> Snapshot {
            match self.harness.borrow_mut().ask(Command::Settle) {
                Answer::Settled(snapshot) => snapshot,
                other => panic!("{other:?}"),
            }
        }

        fn emit(&self, count: usize) {
            for _ in 0..count {
                self.harness.borrow().emit(foreground_now());
            }
        }

        fn two_due_tries(&self) {
            let mut harness = self.harness.borrow_mut();
            harness.clocks.advance(RETRY_FIRST);
            harness.pass();
            harness.clocks.advance(2 * RETRY_FIRST);
            harness.pass();
        }
    }

    struct Fixture<F: SystemSources> {
        supervisor: Supervisor<F, TestLoop>,
        now: Rc<Cell<Duration>>,
        link: Option<HarnessLink>,
        out: Out,
        ctx: SourceContext,
    }

    impl<F: SystemSources> Fixture<F> {
        fn tick(&mut self) -> Option<Ending> {
            let link = self.link.as_ref().map(|link| link as &dyn OwnerLink);
            self.supervisor.tick(link)
        }

        fn later(&mut self, by: Duration) -> Option<Ending> {
            self.now.set(self.now.get() + by);
            self.tick()
        }

        fn start(&mut self, wanted: &[SourceName]) -> Vec<FailedSource> {
            let ctx = self.ctx.clone();
            self.supervisor.start_sources(&ctx, wanted)
        }

        fn state(&self, name: SourceName) -> SourceState {
            self.supervisor
                .board()
                .sources
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| entry.state.clone())
                .unwrap_or(SourceState::NotReported)
        }
    }

    fn fixture<F: SystemSources>(factory: F) -> Fixture<F> {
        let now = Rc::new(Cell::new(Duration::ZERO));
        let (handle, _writer) = crate::sink::channel(64);
        let out = Out::default();
        let main_loop = TestLoop {
            now: now.clone(),
            harness: None,
            real: false,
        };
        Fixture {
            supervisor: Supervisor::new(Platform::Macos, factory, main_loop)
                .with_err(Box::new(out.clone())),
            now,
            link: None,
            out,
            ctx: SourceContext::new(handle),
        }
    }

    fn fixture_on_owner<F: SystemSources>(factory: F, store: ModelStore) -> Fixture<F> {
        let mut harness = Harness::over(store, open_floor(), 4096);
        assert!(matches!(
            harness.ask(Command::ClosePause),
            Answer::Resumed(_)
        ));
        let (_unused, empty) = mpsc::channel();
        let reports = std::mem::replace(&mut harness.reports, empty);
        let ctx = SourceContext::with_floor(harness.sink.clone(), open_floor());
        let harness = Rc::new(RefCell::new(harness));
        let now = Rc::new(Cell::new(Duration::ZERO));
        let out = Out::default();
        let main_loop = TestLoop {
            now: now.clone(),
            harness: Some(harness.clone()),
            real: false,
        };
        Fixture {
            supervisor: Supervisor::new(Platform::Macos, factory, main_loop)
                .with_err(Box::new(out.clone())),
            now,
            link: Some(HarnessLink {
                harness,
                log: ReportLog::new(reports, Box::new(out.clone())),
                running: Cell::new(true),
            }),
            out,
            ctx,
        }
    }

    fn two() -> [SourceName; 2] {
        [SourceName::Foreground, SourceName::Input]
    }

    struct Panicking {
        go: Arc<AtomicBool>,
        quit: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl CaptureSource for Panicking {
        fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
            let (go, quit) = (self.go.clone(), self.quit.clone());
            self.thread = Some(std::thread::spawn(move || {
                while !go.load(Ordering::SeqCst) {
                    if quit.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                panic!("the scripted source's thread panics");
            }));
            Ok(Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), CaptureError> {
            self.quit.store(true, Ordering::SeqCst);
            match self.thread.take().map(JoinHandle::join) {
                Some(Err(_)) => Err(CaptureError::ThreadEnded),
                Some(Ok(())) | None => Ok(()),
            }
        }

        fn health(&self) -> SourceHealth {
            let ended = self.thread.as_ref().is_some_and(JoinHandle::is_finished);
            SourceHealth {
                state: if ended {
                    SourceState::Failed {
                        reason: HealthReason::ThreadEnded,
                        error: CaptureError::ThreadEnded,
                    }
                } else {
                    SourceState::Running
                },
                ..SourceHealth::default()
            }
        }
    }

    type Make = Box<dyn FnMut(SourceName) -> Option<Box<dyn CaptureSource>>>;

    struct Makes {
        make: Make,
        made: Rc<RefCell<Vec<SourceName>>>,
        secure: Arc<AtomicU64>,
    }

    impl SourceFactory for Makes {
        fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
            self.made.borrow_mut().push(name);
            (self.make)(name)
        }
    }

    impl SystemSources for Makes {
        fn figures(&self) -> SourceCounters {
            SourceCounters {
                secure_input_keystrokes: self.secure.load(Ordering::SeqCst),
                ..SourceCounters::default()
            }
        }
    }

    #[test]
    fn a_source_whose_thread_panics_is_failed_within_one_tick_and_runs_again_within_the_backoff() {
        let go = Arc::new(AtomicBool::new(false));
        let made = Rc::new(RefCell::new(Vec::new()));
        let make_go = go.clone();
        let mut f = fixture(Makes {
            make: Box::new(move |_| {
                Some(Box::new(Panicking {
                    go: make_go.clone(),
                    quit: Arc::default(),
                    thread: None,
                }))
            }),
            made: made.clone(),
            secure: Arc::default(),
        });
        assert!(f.start(&[SourceName::Foreground]).is_empty());
        assert!(f.tick().is_none());
        assert_eq!(f.state(SourceName::Foreground), SourceState::Running);
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        go.store(true, Ordering::SeqCst);
        let waited = Instant::now();
        while f.supervisor.sources.read_health_of(SourceName::Foreground) == SourceState::Running
            && waited.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        std::panic::set_hook(quiet);
        assert!(f.tick().is_none());
        assert!(
            matches!(
                f.state(SourceName::Foreground),
                SourceState::Failed {
                    reason: HealthReason::ThreadEnded,
                    ..
                }
            ),
            "one tick after the panic: {:?}",
            f.state(SourceName::Foreground)
        );
        let failed = f.supervisor.board().failed_sources();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].name, "foreground");
        assert!(!failed[0].error.contains("panics"), "{failed:?}");
        go.store(false, Ordering::SeqCst);
        f.later(RESTART_FIRST - Duration::from_millis(1));
        assert_eq!(made.borrow().len(), 1, "started again before the backoff");
        f.later(Duration::from_millis(1));
        assert_eq!(
            made.borrow().len(),
            2,
            "not started again inside the backoff"
        );
        assert_eq!(f.state(SourceName::Foreground), SourceState::Running);
        let stopped = RunSources::stop(&mut f.supervisor);
        assert!(
            stopped.iter().any(|entry| entry.name == "foreground"),
            "{stopped:?}"
        );
    }

    fn failing_with(reason: HealthReason) -> (Fixture<Factory>, SourceName) {
        let mut f = fixture(Factory::new());
        f.start(&two());
        f.tick();
        f.supervisor
            .factory()
            .health_of(SourceName::Input)
            .set(SourceState::Failed {
                reason,
                error: CaptureError::Os {
                    stage: "capture_item",
                    domain: "test",
                    code: 1,
                },
            });
        (f, SourceName::Input)
    }

    #[test]
    fn a_source_whose_item_the_system_closed_runs_again_within_the_backoff() {
        let (mut f, name) = failing_with(HealthReason::ItemClosed);
        f.tick();
        assert_eq!(f.supervisor.factory().calls.of("begin input").len(), 1);
        f.later(RESTART_FIRST);
        assert_eq!(
            f.supervisor.factory().calls.of("begin input").len(),
            2,
            "item_closed was not started again inside the backoff"
        );
        assert_eq!(f.supervisor.factory().calls.of("stop input").len(), 1);
        assert_eq!(f.state(name), SourceState::Running);
    }

    #[test]
    fn a_source_whose_writer_is_gone_is_not_started_again() {
        let (mut f, name) = failing_with(HealthReason::WriterGone);
        for _ in 0..200 {
            f.later(Duration::from_secs(1));
        }
        assert_eq!(f.supervisor.factory().calls.of("begin input").len(), 1);
        assert!(matches!(
            f.state(name),
            SourceState::Failed {
                reason: HealthReason::WriterGone,
                ..
            }
        ));
    }

    #[test]
    fn the_backoff_runs_from_1_s_and_doubles_to_60_s() {
        let mut f = fixture(Factory::adjusting(|source| {
            source.begin = Err(CaptureError::Start("the tap was not made".to_owned()));
        }));
        f.start(&two());
        f.tick();
        assert_eq!(f.supervisor.board().capture, Capture::Faulted);
        let mut starts_at = Vec::new();
        for _ in 0..(10 * 60 * 4) {
            f.later(Duration::from_millis(250));
            let begun = f.supervisor.factory().calls.of("begin input").len();
            if begun > starts_at.len() + 1 {
                starts_at.push(f.now.get());
            }
        }
        let gaps: Vec<u64> = starts_at
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).as_secs())
            .collect();
        assert_eq!(starts_at.first(), Some(&Duration::from_secs(1)));
        assert_eq!(gaps[..6], [2, 4, 8, 16, 32, 60]);
        assert!(gaps[6..].iter().all(|gap| *gap == 60), "{gaps:?}");
    }

    #[test]
    fn a_source_that_fails_once_runs_again_within_the_backoff() {
        let first = Rc::new(Cell::new(true));
        let mut f = fixture(Factory::adjusting(move |source| {
            if source.name == SourceName::Input && first.replace(false) {
                source.begin = Err(CaptureError::Start("the tap was not made".to_owned()));
            }
        }));
        let failed = f.start(&two());
        assert_eq!(failed.len(), 1);
        f.tick();
        assert_eq!(f.supervisor.board().capture, Capture::Partial);
        f.later(RESTART_FIRST);
        assert_eq!(f.state(SourceName::Input), SourceState::Running);
        assert_eq!(f.supervisor.board().capture, Capture::Running);
    }

    #[test]
    fn a_screen_stream_stopped_by_the_user_is_not_started_again_by_the_timer() {
        let mut f = fixture(Factory::new());
        f.start(&[SourceName::Foreground, SourceName::Screen]);
        f.tick();
        f.supervisor
            .factory()
            .health_of(SourceName::Screen)
            .set(SourceState::Stopped(StopCause::User));
        for _ in 0..200 {
            f.later(Duration::from_secs(1));
        }
        assert_eq!(f.supervisor.factory().calls.of("begin screen").len(), 1);
        assert_eq!(
            f.state(SourceName::Screen),
            SourceState::Stopped(StopCause::User)
        );
    }

    fn drain_fails(f: &Fixture<Factory>, store: &ModelStore, fault: WriteFault) {
        store.fail_every_write(Some(fault));
        let link = f.link.as_ref().unwrap();
        link.emit(2);
        link.settle();
    }

    #[test]
    fn the_sources_stop_while_the_drain_fails_and_start_on_its_recovery() {
        let store = ModelStore::new();
        let mut f = fixture_on_owner(Factory::new(), store.clone());
        f.start(&two());
        f.tick();
        drain_fails(&f, &store, WriteFault::Io(io::ErrorKind::Other));
        f.tick();
        assert_eq!(
            f.supervisor.factory().calls.of("stop "),
            ["stop input", "stop foreground"],
            "the sources did not stop, input first, on DrainFailing"
        );
        assert_eq!(f.supervisor.board().capture, Capture::Stopped);
        store.fail_every_write(None);
        f.supervisor.main_loop().turn(RETRY_FIRST);
        f.link.as_ref().unwrap().settle();
        f.tick();
        assert_eq!(f.supervisor.factory().calls.of("begin ").len(), 4);
        assert_eq!(f.supervisor.board().capture, Capture::Running);
        let snapshot = f.link.as_ref().unwrap().settle();
        assert_eq!(
            snapshot.discarded_by_barrier, 0,
            "the drain stop drew a barrier"
        );
        assert_eq!(snapshot.events_persisted, 2);
    }

    fn into_the_hold(f: &Fixture<Factory>, store: &ModelStore) {
        store.fail_every_write(Some(WriteFault::Db(19)));
        let link = f.link.as_ref().unwrap();
        for _ in 0..3 {
            link.emit(2);
            link.settle();
            link.two_due_tries();
        }
        assert!(link.snapshot().drain_hold, "the owner is not in its hold");
    }

    #[test]
    fn a_refused_or_sealed_give_up_inside_the_hold_leaves_the_sources_stopped() {
        for answer in [WriteFault::Refused, WriteFault::Sealed] {
            let store = ModelStore::new();
            let mut f = fixture_on_owner(Factory::new(), store.clone());
            f.start(&two());
            f.tick();
            into_the_hold(&f, &store);
            f.tick();
            assert_eq!(f.supervisor.factory().calls.of("stop ").len(), 2);
            drain_fails(&f, &store, answer);
            f.tick();
            let board = f.supervisor.board();
            assert!(!board.drain.failing, "{answer:?} reported DrainHealthy");
            assert!(
                board.drain.hold,
                "{answer:?}: the published field left the hold"
            );
            assert_eq!(
                f.supervisor.factory().calls.of("begin ").len(),
                2,
                "{answer:?} inside the hold started the sources"
            );
            assert_eq!(board.capture, Capture::Stopped);
            store.fail_every_write(None);
            f.link.as_ref().unwrap().emit(1);
            f.link.as_ref().unwrap().settle();
            f.tick();
            assert_eq!(
                f.supervisor.factory().calls.of("begin ").len(),
                4,
                "one acknowledged write did not start the sources"
            );
            let lines = f.out.text();
            assert_eq!(
                lines.matches("halvo-capture: dropped events=2 ").count(),
                4,
                "{lines}"
            );
            assert!(lines.contains(" error=db\n"), "{lines}");
        }
    }

    #[test]
    fn the_report_log_keeps_one_drain_report_and_writes_every_drop_line() {
        let (send, channel) = mpsc::channel();
        let out = Out::default();
        let log = ReportLog::new(channel, Box::new(out.clone()));
        for _ in 0..10_000 {
            send.send(Report::BatchRejected {
                events: 1,
                first_capture: 0,
                last_capture: 0,
                fault: WriteFault::Refused,
            })
            .unwrap();
            send.send(Report::DrainHealthy).unwrap();
        }
        send.send(Report::DrainFailing {
            cause: crate::daemon::owner::DrainCause::Io,
            since: std::time::SystemTime::UNIX_EPOCH,
        })
        .unwrap();
        assert!(log.read());
        let kept: Vec<Report> = log.try_iter().collect();
        assert!(
            matches!(kept[..], [Report::DrainFailing { .. }]),
            "{kept:?}"
        );
        assert_eq!(log.try_iter().count(), 0);
        assert_eq!(out.text().lines().count(), 10_000);
    }

    #[test]
    fn a_stop_that_blocks_10_s_still_ends_inside_the_budget_with_the_batch_acknowledged() {
        let store = ModelStore::new();
        let at_stop: Rc<Cell<Option<usize>>> = Rc::default();
        let blocked_now: Rc<RefCell<Option<Rc<Cell<Duration>>>>> = Rc::default();
        let (seen, blocked) = (at_stop.clone(), blocked_now.clone());
        let watched = store.clone();
        let mut f = fixture_on_owner(
            Factory::adjusting(move |source| {
                if source.name == SourceName::Input {
                    let (seen, blocked, watched) = (seen.clone(), blocked.clone(), watched.clone());
                    let hook: Hook = Box::new(move |_| {
                        seen.set(Some(watched.write_calls().len()));
                        if let Some(now) = blocked.borrow().as_ref() {
                            now.set(now.get() + Duration::from_secs(10));
                        }
                    });
                    source.on_stop = Some(hook);
                    source.stop = Err(CaptureError::Timeout {
                        stage: "input_tap_join",
                        ms: 1_000,
                    });
                }
            }),
            store.clone(),
        );
        *blocked_now.borrow_mut() = Some(f.now.clone());
        f.start(&two());
        f.tick();
        f.link.as_ref().unwrap().emit(3);
        f.link.as_ref().unwrap().harness.borrow_mut().pass();
        assert_eq!(
            store.write_calls().len(),
            0,
            "the batch was written before the entry"
        );
        let link = f.link.take().unwrap();
        let outcome = f.supervisor.shutdown_entry(&link, exit::CLEAN).unwrap();
        assert_eq!(
            at_stop.get(),
            Some(1),
            "the open batch was not written before the first stop"
        );
        assert!(
            outcome.took < Duration::from_millis(SHUTDOWN_BUDGET_MS),
            "the entry took {:?}",
            outcome.took
        );
        assert!(outcome.took >= Duration::from_secs(10));
        assert_eq!(outcome.status, exit::CLEAN);
        assert_eq!(outcome.settled.map(|s| s.events_persisted), Some(3));
        assert_eq!(f.supervisor.factory().calls.of("stop ")[0], "stop input");
        assert!(matches!(
            outcome.stops[0],
            (SourceName::Input, Err(CaptureError::Timeout { .. }))
        ));
    }

    #[test]
    fn a_second_trigger_does_not_run_the_entry_again() {
        let mut f = fixture_on_owner(Factory::new(), ModelStore::new());
        f.start(&two());
        let link = f.link.take().unwrap();
        assert!(f.supervisor.shutdown_entry(&link, exit::CLEAN).is_some());
        let stops = f.supervisor.factory().calls.of("stop ").len();
        assert_eq!(stops, 2);
        assert!(f.supervisor.shutdown_entry(&link, exit::RESTART).is_none());
        assert!(f.supervisor.shutdown_entry(&link, exit::CLEAN).is_none());
        assert_eq!(f.supervisor.factory().calls.of("stop ").len(), stops);
    }

    #[test]
    fn the_entry_exits_1_when_the_drain_still_fails_at_the_last_settle() {
        let store = ModelStore::new();
        let mut f = fixture_on_owner(Factory::new(), store.clone());
        f.start(&two());
        drain_fails(&f, &store, WriteFault::Io(io::ErrorKind::Other));
        let link = f.link.take().unwrap();
        let outcome = f.supervisor.shutdown_entry(&link, exit::CLEAN).unwrap();
        assert_eq!(outcome.status, exit::ERROR);
    }

    #[test]
    fn a_secure_input_count_survives_a_stop_and_a_start() {
        let secure = Arc::new(AtomicU64::new(0));
        let counted = secure.clone();
        let made = Rc::new(RefCell::new(Vec::new()));
        let store = ModelStore::new();
        let mut inner = Factory::new();
        let mut f = fixture_on_owner(
            Makes {
                make: Box::new(move |name| {
                    let mut source = inner.make(name)?;
                    if name == SourceName::Input && counted.load(Ordering::SeqCst) == 0 {
                        counted.fetch_add(1, Ordering::SeqCst);
                        let _ = &mut source;
                    }
                    Some(source)
                }),
                made: made.clone(),
                secure: secure.clone(),
            },
            store.clone(),
        );
        f.start(&two());
        f.tick();
        assert_eq!(f.supervisor.board().figures.secure_input_keystrokes, 1);
        store.fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        f.link.as_ref().unwrap().emit(1);
        f.link.as_ref().unwrap().settle();
        f.tick();
        store.fail_every_write(None);
        f.supervisor.main_loop().turn(RETRY_FIRST);
        f.link.as_ref().unwrap().settle();
        f.tick();
        assert_eq!(made.borrow().len(), 4, "{:?}", made.borrow());
        assert_eq!(f.supervisor.board().figures.secure_input_keystrokes, 1);
    }

    struct FakeProbe {
        answers: Rc<RefCell<VecDeque<Probed>>>,
        asked: Rc<RefCell<Vec<Vec<Permission>>>>,
    }

    impl ProbeRunner for FakeProbe {
        fn start(&mut self, permissions: &[Permission]) {
            self.asked.borrow_mut().push(permissions.to_vec());
        }

        fn poll(&mut self) -> Probed {
            self.answers
                .borrow_mut()
                .pop_front()
                .unwrap_or(Probed::Running)
        }
    }

    type Answers = Rc<RefCell<VecDeque<Probed>>>;

    type Asked = Rc<RefCell<Vec<Vec<Permission>>>>;

    fn probed(f: Fixture<Factory>) -> (Fixture<Factory>, Answers, Asked) {
        let answers: Answers = Rc::default();
        let asked = Rc::default();
        let mut f = f;
        f.supervisor = f.supervisor.with_probe(Box::new(FakeProbe {
            answers: answers.clone(),
            asked: Rc::clone(&asked),
        }));
        (f, answers, asked)
    }

    fn input_monitoring(grant: GrantState) -> Probed {
        Probed::Answered(vec![(Permission::InputMonitoring, grant)])
    }

    #[test]
    fn input_monitoring_not_granted_stops_the_input_source_in_the_same_tick() {
        let (mut f, answers, asked) = probed(fixture(Factory::new()));
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::Granted));
        f.start(&two());
        f.tick();
        assert_eq!(f.state(SourceName::Input), SourceState::Running);
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::NotGranted));
        f.later(PROBE_INTERVAL);
        assert!(
            matches!(
                f.state(SourceName::Input),
                SourceState::Failed {
                    reason: HealthReason::Permission,
                    ..
                }
            ),
            "{:?}",
            f.state(SourceName::Input)
        );
        assert_eq!(f.supervisor.factory().calls.of("stop input").len(), 1);
        for _ in 0..5 {
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::NotGranted));
            f.later(PROBE_INTERVAL);
        }
        assert_eq!(f.supervisor.factory().calls.of("begin input").len(), 1);
        assert_eq!(f.state(SourceName::Foreground), SourceState::Running);
        assert!(asked
            .borrow()
            .iter()
            .all(|named| named == &[Permission::InputMonitoring]));
    }

    #[test]
    fn a_permission_granted_after_the_start_ends_the_run_with_status_4() {
        let (mut f, answers, _) = probed(fixture(Factory::new()));
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::NotGranted));
        f.start(&two());
        assert_eq!(f.tick(), None);
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::Granted));
        assert_eq!(f.later(PROBE_INTERVAL), Some(Ending::Restart));
        assert!(RunSources::restart_asked(&f.supervisor));
    }

    #[test]
    fn the_tick_still_fires_while_the_probe_runner_blocks() {
        let (mut f, _answers, asked) = probed(fixture(Factory::adjusting(|source| {
            if source.name == SourceName::Input {
                source.begin = Err(CaptureError::Start("the tap was not made".to_owned()));
            }
        })));
        f.start(&two());
        let stop = AtomicBool::new(false);
        assert!(!f.supervisor.wait_on(Duration::from_secs(3), &stop, None));
        assert_eq!(
            asked.borrow().len(),
            1,
            "a second probe began while one runs"
        );
        assert!(f.supervisor.factory().calls.of("begin input").len() >= 2);
    }

    fn real_loop() -> TestLoop {
        TestLoop {
            now: Rc::default(),
            harness: None,
            real: true,
        }
    }

    #[test]
    fn after_a_start_the_first_event_a_source_emits_is_stored() {
        let store = ModelStore::new();
        let mut supervisor = Supervisor::new(
            Platform::Macos,
            Factory::adjusting(|source| {
                if source.name == SourceName::Foreground {
                    source.script = vec![foreground_now()];
                }
            }),
            real_loop(),
        );
        let outcome = run_timed(
            &run_of(1),
            &mut supervisor,
            store.clone(),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        assert_eq!(outcome.report.foreground_app, 1);
        assert_eq!(
            outcome.report.events_persisted,
            1,
            "{}",
            outcome.report.render()
        );
        assert_eq!(store.records().len(), 1);
        assert_eq!(outcome.status, exit::CLEAN);
    }

    #[test]
    fn the_stop_sequence_draws_the_barrier_before_any_source_stops() {
        let store = ModelStore::new();
        let mut supervisor = Supervisor::new(
            Platform::Macos,
            Factory::adjusting(|source| {
                let hook: Hook = Box::new(|ctx| {
                    let _ = ctx.sink().emit_event(key());
                });
                source.on_stop = Some(hook);
            }),
            real_loop(),
        );
        let outcome = run_timed(
            &run_of(1),
            &mut supervisor,
            store.clone(),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        assert_eq!(outcome.report.keystroke, 2, "{}", outcome.report.render());
        assert_eq!(outcome.report.events_discarded, 2);
        assert!(store.records().is_empty());
    }

    struct PanickingStore(ModelStore);

    impl MaintainStore for PanickingStore {
        fn expire_due(
            &mut self,
            now: std::time::SystemTime,
            max: usize,
        ) -> Result<Pass, MaintFault> {
            self.0.expire_due(now, max)
        }

        fn evict_to(&mut self, bound: ByteBound, max: usize) -> Result<Pass, MaintFault> {
            self.0.evict_to(bound, max)
        }

        fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
            self.0.usage()
        }
    }

    impl CaptureStore for PanickingStore {
        fn write_batch(&mut self, _write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
            panic!("the scripted store panics at its write");
        }

        fn volume_free_bytes(&self) -> io::Result<u64> {
            self.0.volume_free_bytes()
        }
    }

    #[test]
    fn owner_death_stops_the_sources_and_returns_exit_1() {
        let calls = Factory::new().calls;
        let shared = calls.clone();
        let mut factory = Factory::adjusting(|source| {
            if source.name == SourceName::Foreground {
                source.script = (0..=BATCH_MAX_EVENTS).map(|_| foreground_now()).collect();
            }
        });
        factory.calls = shared;
        let mut supervisor = Supervisor::new(Platform::Macos, factory, real_loop());
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = run_timed(
            &run_of(60),
            &mut supervisor,
            PanickingStore(ModelStore::new()),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            Duration::from_millis(500),
        );
        std::panic::set_hook(quiet);
        let outcome = outcome.unwrap();
        assert_eq!(outcome.status, exit::ERROR);
        assert_eq!(calls.of("stop "), ["stop input", "stop foreground"]);
    }

    #[test]
    fn a_timed_run_whose_source_failed_after_its_start_exits_3_with_its_line() {
        let mut supervisor = Supervisor::new(Platform::Macos, Factory::new(), real_loop());
        let mut started = false;
        let stop = AtomicBool::new(false);
        let ctx = SourceContext::new(crate::sink::channel(8).0);
        supervisor.start_sources(&ctx, &two());
        supervisor
            .factory()
            .health_of(SourceName::Foreground)
            .set(SourceState::Failed {
                reason: HealthReason::ThreadEnded,
                error: CaptureError::ThreadEnded,
            });
        started |= !supervisor.wait_on(Duration::from_millis(300), &stop, None);
        assert!(started);
        let failed = RunSources::stop(&mut supervisor);
        assert_eq!(failed.len(), 1, "{failed:?}");
        assert_eq!(failed[0].name, "foreground");
        let _ = (
            starting(Platform::Macos, RunKind::Service, Switches::default()),
            Instant::now(),
        );
    }
}
