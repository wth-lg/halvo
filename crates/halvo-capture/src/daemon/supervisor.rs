//! The supervisor: it holds the sources of a run, waits on the main loop and,
//! on every tick, reads the owner's reports and snapshot, the health of each
//! source and the permission answers, and acts on them.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use halvo_control::exit;
use halvo_control::onboarding::{GrantState, Permission};
use halvo_control::proto::{Platform, SourceName};

use super::board::{Board, Capture, Drain};
use super::counters::Snapshot;
use super::main_loop::{self, EndCall, MainLoop, Watchdog};
use super::owner::{
    Answer, Command, OwnerGone, OwnerHandle, OwnerPanicked, OwnerThread, Pending, Report,
};
use super::permissions::{ProbeRunner, Probed};
use super::report::FailedSource;
use super::sources::{
    is_a_stop, past_bound, stop_bound, stop_order, RunningSources, SourceFactory,
};
use super::{DaemonError, RunSources, Waits};
use crate::source::{
    CaptureError, HealthReason, SourceContext, SourceCounters, SourceState, StopCause,
};

/// The whole exit budget of the shutdown entry, counted from its start
/// (rulings D3; contracts.md C14.6).
pub const SHUTDOWN_BUDGET_MS: u64 = 18_000;

const _: () = assert!(SHUTDOWN_BUDGET_MS < halvo_control::STOP_WAIT_MS);

/// The latest deadline of an entry's watchdog, counted from its arm before
/// the entry's first stop (ruling DM-25 N1): the longest stop bound in play
/// plus SHUTDOWN_BUDGET_MS, held under STOP_WAIT_MS. The bound of every
/// source but the screen source is 1,250 ms (its deadline of 1 s plus the
/// slack of 250 ms), which gives 19,250 ms; the screen source's 6,250 ms
/// would give 24,250 ms, past the 20,000 ms that `ctl stop` and the gate's
/// stop_deadline_s wait, so this cap holds every entry at 19,250 ms.
pub const ENTRY_DEADLINE_MS: u64 = SHUTDOWN_BUDGET_MS + 1_250;

const _: () = assert!(ENTRY_DEADLINE_MS < halvo_control::STOP_WAIT_MS);

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
    /// A stop for capture off met a source that did not stop, and the end
    /// routine ended the process with status 5.
    SourceNotStopped,
}

/// The kind of a stop for capture off (ruling PL-3 part (3)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopKind {
    Pause,
    Lapse,
    DeleteAll,
    /// The stop of a source whose grant went away (contracts.md C14 item 9).
    LostGrant,
}

impl StopKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            StopKind::Pause => "pause",
            StopKind::Lapse => "lapse",
            StopKind::DeleteAll => "delete_all",
            StopKind::LostGrant => "lost_grant",
        }
    }
}

/// What a stop for capture off ended with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureOff {
    /// Every source answered a stop; the process stays up.
    Stopped,
    /// A source did not stop and the end routine made the ending call with
    /// this status.
    Ended(i32),
}

/// The record of a life that a source which did not stop ended (ruling PL-3
/// part (3), steps 4 and 7): the kind of stop, the sources that did not stop
/// with their reason, and the owner's figures, first as they stood at the
/// first answer that was not a stop and then as the final settle left them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndRecord {
    pub kind: StopKind,
    pub final_figures: bool,
    pub not_stopped: Vec<(SourceName, String)>,
    pub events_persisted: u64,
    pub events_lost: u64,
    pub events_pending: u64,
}

impl EndRecord {
    /// The record as one line of the run marker.
    pub fn text(&self) -> String {
        let not_stopped: Vec<String> = self
            .not_stopped
            .iter()
            .map(|(name, reason)| format!("{}:{reason}", name.as_str()))
            .collect();
        format!(
            "last_stop=source_not_stopped kind={} final={} not_stopped={} events_persisted={} events_lost={} events_pending={}\n",
            self.kind.as_str(),
            if self.final_figures { "yes" } else { "no" },
            not_stopped.join(","),
            self.events_persisted,
            self.events_lost,
            self.events_pending
        )
    }
}

/// Where the end routine writes its record: the run marker of a service run.
pub trait Marker {
    fn write(&mut self, record: &EndRecord) -> std::io::Result<()>;
}

/// The run marker as a file, rewritten whole by a write to a file beside it,
/// a sync and a rename, and a sync of the directory.
pub struct MarkerFile {
    path: std::path::PathBuf,
}

impl MarkerFile {
    pub fn new(path: std::path::PathBuf) -> MarkerFile {
        MarkerFile { path }
    }
}

impl Marker for MarkerFile {
    fn write(&mut self, record: &EndRecord) -> std::io::Result<()> {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".new");
        let next = self.path.with_file_name(name);
        let mut file = std::fs::File::create(&next)?;
        file.write_all(record.text().as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&next, &self.path)?;
        #[cfg(unix)]
        if let Some(dir) = self.path.parent() {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    }
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
    probe_last: Option<Duration>,
    probe_retry_at: Option<Duration>,
    probe_missing: bool,
    probe_asked: bool,
    client_polls: bool,
    ending: Option<Ending>,
    entered: bool,
    err: Box<dyn Write + Send>,
    end_call: Option<Arc<dyn EndCall>>,
    watchdog: Option<Watchdog>,
    marker: Option<Box<dyn Marker>>,
    ends_by: Option<Duration>,
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
            probe_last: None,
            probe_retry_at: None,
            probe_missing: false,
            probe_asked: false,
            client_polls: false,
            ending: None,
            entered: false,
            err: Box::new(std::io::stderr()),
            end_call: None,
            watchdog: None,
            marker: None,
            ends_by: None,
        }
    }

    /// The probe runner whose answers the supervisor reads.
    pub fn with_probe(mut self, probe: Box<dyn ProbeRunner>) -> Supervisor<F, L> {
        self.probe = Some(probe);
        self
    }

    /// The call that ends the process, and the watchdog that makes it at the
    /// budget's end, started now.
    pub fn with_ending(mut self, end_call: Arc<dyn EndCall>) -> Supervisor<F, L> {
        self.watchdog = Some(Watchdog::start(end_call.clone()));
        self.end_call = Some(end_call);
        self
    }

    /// Where the end routine writes its record.
    pub fn with_marker(mut self, marker: Box<dyn Marker>) -> Supervisor<F, L> {
        self.marker = Some(marker);
        self
    }

    /// The status that the watchdog is armed with, if it is.
    pub fn watchdog_armed(&self) -> Option<i32> {
        self.watchdog.as_ref().and_then(Watchdog::armed)
    }

    pub fn factory_mut(&mut self) -> &mut F {
        &mut self.factory
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
        self.probe_last = None;
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
        if let Some(ending) = self.read_permissions(now, owner) {
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
        let wanted: Vec<SourceName> = self
            .wanted
            .iter()
            .copied()
            .filter(|name| !self.missing_permission(*name) && !self.stopped_by_user(*name))
            .collect();
        for name in wanted {
            if let Some(Err(error)) = self.sources.start_one(&mut self.factory, &ctx, name) {
                self.board.record_failure(name, error);
                self.schedule_restart(name, now);
            }
        }
    }

    fn stopped_by_user(&self, name: SourceName) -> bool {
        self.sources
            .state(name)
            .is_some_and(|(state, _)| *state == SourceState::Stopped(StopCause::User))
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
            self.board.permissions.iter().any(|(answered, grant)| {
                *answered == permission && *grant == GrantState::NotGranted
            })
        })
    }

    fn required(&self) -> Vec<Permission> {
        self.wanted
            .iter()
            .filter_map(|name| required_permission(self.platform, *name))
            .collect()
    }

    fn read_permissions(&mut self, now: Duration, owner: Option<&dyn OwnerLink>) -> Option<Ending> {
        let required = self.required();
        let probe = self.probe.as_mut()?;
        if required.is_empty() {
            return None;
        }
        let interval = if self.probe_missing && self.client_polls {
            PROBE_INTERVAL_MISSING
        } else {
            PROBE_INTERVAL
        };
        let due = self.probe_retry_at.is_some_and(|at| now >= at)
            || self.probe_last.is_none_or(|at| now >= at + interval);
        if !self.probe_asked && due {
            probe.start(&required);
            self.probe_asked = true;
        }
        if !self.probe_asked {
            return None;
        }
        let answers = match probe.poll() {
            Probed::Running => return None,
            Probed::Failed => {
                self.probe_asked = false;
                self.probe_retry_at = Some(now + PROBE_INTERVAL_MISSING);
                return None;
            }
            Probed::Answered(answers) => answers,
        };
        self.probe_asked = false;
        self.probe_retry_at = None;
        self.probe_last = Some(now);
        self.probe_missing = answers
            .iter()
            .any(|(_, grant)| *grant != GrantState::Granted);
        let granted: Vec<Permission> = answers
            .iter()
            .filter(|(_, grant)| *grant == GrantState::Granted)
            .map(|(permission, _)| *permission)
            .collect();
        self.board.permissions = answers;
        match self.board.granted_at_start.clone() {
            None => self.board.granted_at_start = Some(granted),
            Some(at_start) => {
                if granted
                    .iter()
                    .any(|permission| !at_start.contains(permission))
                {
                    return Some(Ending::Restart);
                }
            }
        }
        if let Some(ending) = self.stop_without_permission(owner) {
            return Some(ending);
        }
        self.restart_granted_again(now);
        None
    }

    fn stop_without_permission(&mut self, owner: Option<&dyn OwnerLink>) -> Option<Ending> {
        let names: Vec<SourceName> = stop_order(self.platform)
            .iter()
            .copied()
            .filter(|name| self.wanted.contains(name) && self.missing_permission(*name))
            .collect();
        let in_play = self.held(&names);
        let armed = owner.is_some()
            && !in_play.is_empty()
            && self.ends_by.is_none()
            && self.watchdog_armed().is_none();
        if armed {
            self.arm_entry(&in_play, exit::SOURCE_NOT_STOPPED);
        }
        for name in names {
            self.backoff.retain(|backoff| backoff.name != name);
            let error = CaptureError::Os {
                stage: "permission_probe",
                domain: "tcc",
                code: 0,
            };
            if let Some(answer) = self.sources.stop_one(name) {
                if !is_a_stop(&answer) {
                    if let Some(owner) = owner {
                        self.arm_unstopped(name, &answer);
                        self.end_after_unstopped(owner, StopKind::LostGrant, &mut |_| {});
                        return Some(Ending::SourceNotStopped);
                    }
                    continue;
                }
            }
            self.sources.set_state(
                name,
                SourceState::Failed {
                    reason: HealthReason::Permission,
                    error: error.clone(),
                },
            );
            self.board.record_failure(name, error);
        }
        if armed {
            self.disarm();
        }
        None
    }

    /// A stop for capture off (Pause, a lapsed lease, step 1 of delete-all; daemon-3 calls it): the
    /// watchdog is armed with status 5 for the longest stop bound in play plus SHUTDOWN_BUDGET_MS,
    /// at most [`ENTRY_DEADLINE_MS`]; then the owner's barrier, then each source in the stop order
    /// until the first answer that is not a stop, a stop past its bound included (ruling DM-25 N1).
    /// When every source stops the arm is taken back and the process stays up. At the first answer
    /// that is not a stop the arm moves to SHUTDOWN_BUDGET_MS after that answer, or after the
    /// moment that a late stop's bound passed, the earlier deadline kept, and the end routine runs:
    /// `reply` gets the Board, the first record is written, the owner settles, every source still
    /// held is asked once more when its stop bound fits before the deadline, the owner settles
    /// again, the final record is written and the ending call is made with status 5 (ruling PL-3
    /// parts (2) to (4)). The settles are cut to what is left before the deadline, and the watchdog
    /// ends the process with 5 only when the routine has not. It runs once: a second trigger
    /// answers `None`.
    pub fn stop_for_capture_off(
        &mut self,
        owner: &dyn OwnerLink,
        kind: StopKind,
        reply: &mut dyn FnMut(&Board),
    ) -> Option<CaptureOff> {
        if self.entered {
            return None;
        }
        let in_play = self.held(stop_order(self.platform));
        self.arm_entry(&in_play, exit::SOURCE_NOT_STOPPED);
        if kind != StopKind::LostGrant {
            self.barrier(owner);
        }
        self.backoff.clear();
        let mut unstopped = false;
        for name in stop_order(self.platform) {
            let Some(answer) = self.sources.stop_one(*name) else {
                continue;
            };
            if !is_a_stop(&answer) {
                self.arm_unstopped(*name, &answer);
                unstopped = true;
                break;
            }
        }
        self.publish();
        if !unstopped {
            self.disarm();
            return Some(CaptureOff::Stopped);
        }
        Some(self.end_after_unstopped(owner, kind, reply))
    }

    /// The sources of `names` that the supervisor holds.
    fn held(&self, names: &[SourceName]) -> Vec<SourceName> {
        names
            .iter()
            .copied()
            .filter(|name| self.sources.holds(*name))
            .collect()
    }

    /// Arms the end routine's watchdog before an entry's first stop, with
    /// `status`: the longest stop bound of `in_play` plus SHUTDOWN_BUDGET_MS,
    /// at most [`ENTRY_DEADLINE_MS`] (ruling DM-25 N1 (a)).
    fn arm_entry(&mut self, in_play: &[SourceName], status: i32) {
        let longest = in_play
            .iter()
            .map(|name| stop_bound(*name))
            .max()
            .unwrap_or_default();
        let after = (longest + Duration::from_millis(SHUTDOWN_BUDGET_MS))
            .min(Duration::from_millis(ENTRY_DEADLINE_MS));
        self.arm_until(after, status);
    }

    /// Moves the arm, with status 5, to SHUTDOWN_BUDGET_MS after the moment
    /// that the stop of `name` gave `answer`, an answer that is not a stop, or
    /// for a stop that answered after its bound, after the moment that its
    /// bound passed. The earlier deadline stays.
    fn arm_unstopped(&mut self, name: SourceName, answer: &Result<(), CaptureError>) {
        let since = past_bound(name, answer).unwrap_or_default();
        self.arm_until(
            Duration::from_millis(SHUTDOWN_BUDGET_MS).saturating_sub(since),
            exit::SOURCE_NOT_STOPPED,
        );
    }

    /// Arms the watchdog for `after` with `status` and keeps the same
    /// deadline on the main loop's clock, to which the end routine cuts its
    /// waits; the earlier deadline stays.
    fn arm_until(&mut self, after: Duration, status: i32) {
        let until = self.main_loop.now() + after;
        self.ends_by = Some(self.ends_by.map_or(until, |earlier| earlier.min(until)));
        if let Some(watchdog) = &self.watchdog {
            watchdog.arm(after, status);
        }
    }

    /// Takes back the arm of an entry whose every stop answered within its
    /// bound.
    fn disarm(&mut self) {
        self.ends_by = None;
        if let Some(watchdog) = &self.watchdog {
            watchdog.disarm();
        }
    }

    /// `bound`, cut to what is left before the entry's deadline.
    fn before_the_deadline(&self, bound: Duration) -> Duration {
        match self.ends_by {
            Some(until) => bound.min(until.saturating_sub(self.main_loop.now())),
            None => bound,
        }
    }

    fn barrier(&mut self, owner: &dyn OwnerLink) {
        if let Ok(pending) = owner.ask(Command::OpenPause) {
            let _ = self.answer_within(&pending, FIRST_SETTLE);
        }
    }

    fn end_after_unstopped(
        &mut self,
        owner: &dyn OwnerLink,
        kind: StopKind,
        reply: &mut dyn FnMut(&Board),
    ) -> CaptureOff {
        self.entered = true;
        if kind == StopKind::LostGrant {
            self.barrier(owner);
        }
        self.publish();
        self.board.capture = Capture::Stopped;
        reply(&self.board);
        let first = owner.snapshot();
        self.write_record(kind, false, &first);
        if let Ok(pending) = owner.ask(Command::Settle) {
            let _ = self.answer_within(&pending, self.before_the_deadline(FIRST_SETTLE));
        }
        let _ = self.stop_sources(true);
        let last = owner.ask(Command::Settle).ok().and_then(|pending| {
            self.answer_within(&pending, self.before_the_deadline(FINAL_SETTLE))
        });
        let settled = match last {
            Some(Answer::Settled(snapshot)) => snapshot,
            _ => owner.snapshot(),
        };
        self.write_record(kind, true, &settled);
        if let Some(end_call) = &self.end_call {
            end_call.end(exit::SOURCE_NOT_STOPPED);
        }
        CaptureOff::Ended(exit::SOURCE_NOT_STOPPED)
    }

    fn write_record(&mut self, kind: StopKind, final_figures: bool, snapshot: &Snapshot) {
        let not_stopped = self
            .sources
            .states()
            .filter(|(_, _, unstopped)| *unstopped)
            .map(|(name, state, _)| {
                let reason = match state {
                    SourceState::Failed { reason, .. } => {
                        super::board::reason_token(*reason).as_str()
                    }
                    _ => "timeout",
                };
                (name, reason.to_owned())
            })
            .collect();
        let record = EndRecord {
            kind,
            final_figures,
            not_stopped,
            events_persisted: snapshot.events_persisted,
            events_lost: snapshot.events_lost,
            events_pending: snapshot.events_pending,
        };
        if let Some(marker) = self.marker.as_mut() {
            let _ = marker.write(&record);
        }
    }

    fn restart_granted_again(&mut self, now: Duration) {
        if self.stopped_for_drain {
            return;
        }
        let wanted = self.wanted.clone();
        for name in wanted {
            let lost_its_grant = self.sources.state(name).is_some_and(|(state, _)| {
                matches!(
                    state,
                    SourceState::Failed {
                        reason: HealthReason::Permission,
                        ..
                    }
                )
            });
            if lost_its_grant
                && !self.missing_permission(name)
                && !self.sources.holds(name)
                && !self.backoff.iter().any(|backoff| backoff.name == name)
            {
                self.backoff.push(Backoff {
                    name,
                    due: now,
                    delay: RESTART_FIRST,
                });
            }
        }
    }

    /// Stops every source in the stop order and returns each answer. The
    /// secure-input line of an input source with a hold plan is written the
    /// moment that its stop answers anything but a stop (ruling MA-9 part (5)).
    pub fn stop_every_source(&mut self) -> Vec<(SourceName, Result<(), CaptureError>)> {
        self.stop_sources(false)
    }

    /// Stops the sources as [`Supervisor::stop_every_source`] does; with
    /// `within_deadline`, the end routine's second ask, a source whose stop
    /// bound does not fit in what is left before the entry's deadline is not
    /// asked, so that the routine reaches its last record and its ending
    /// call before the watchdog would.
    fn stop_sources(
        &mut self,
        within_deadline: bool,
    ) -> Vec<(SourceName, Result<(), CaptureError>)> {
        self.backoff.clear();
        let mut answers = Vec::new();
        for name in stop_order(self.platform) {
            let bound = stop_bound(*name);
            if within_deadline && self.before_the_deadline(bound) < bound {
                continue;
            }
            let Some(answer) = self.sources.stop_one(*name) else {
                continue;
            };
            if self.ends_by.is_some() && !is_a_stop(&answer) {
                self.arm_unstopped(*name, &answer);
            }
            if *name == SourceName::Input
                && !is_a_stop(&answer)
                && self.factory.holds_secure_input()
            {
                let _ = writeln!(self.err, "{}", hold_stays_line(std::process::id()));
                let _ = self.err.flush();
            }
            answers.push((*name, answer));
        }
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

    /// The shutdown entry: the watchdog is armed with `status` for the
    /// longest stop bound in play plus SHUTDOWN_BUDGET_MS, at most
    /// [`ENTRY_DEADLINE_MS`]; the owner settles the open batch (at most 1 s),
    /// the sources stop in the stop order, and the owner settles once more
    /// (at most 5 s). It runs once: a second trigger answers `None`. At the
    /// first answer that is not a stop, a stop past its bound included, the
    /// arm moves to SHUTDOWN_BUDGET_MS after it with status 5, the earlier
    /// deadline kept; the owner settles, every source still held is asked
    /// once more when its stop bound fits before the deadline, the last
    /// settle follows, each cut to what is left before
    /// the deadline, and the entry makes the ending call with 5 and answers
    /// 5 (ruling DM-25 N1 (c)). Otherwise the arm is taken back, and the
    /// status is 1 when the drain still fails at the last settle, when the
    /// last settle gets no answer or when the life lost events between the
    /// two settles, else `status`.
    pub fn shutdown_entry(&mut self, owner: &dyn OwnerLink, status: i32) -> Option<EntryOutcome> {
        if self.entered {
            return None;
        }
        self.entered = true;
        let started = self.main_loop.now();
        let in_play = self.held(stop_order(self.platform));
        self.arm_entry(&in_play, status);
        let first = owner.ask(Command::Settle).ok().and_then(|pending| {
            self.answer_within(&pending, self.before_the_deadline(FIRST_SETTLE))
        });
        let before = match first {
            Some(Answer::Settled(snapshot)) => Some(snapshot),
            _ => None,
        };
        let stops = self.stop_every_source();
        let unstopped = stops.iter().any(|(_, answer)| !is_a_stop(answer));
        if unstopped {
            if let Ok(pending) = owner.ask(Command::Settle) {
                let _ = self.answer_within(&pending, self.before_the_deadline(FIRST_SETTLE));
            }
            let _ = self.stop_sources(true);
        }
        let last = owner.ask(Command::Settle).ok().and_then(|pending| {
            self.answer_within(&pending, self.before_the_deadline(FINAL_SETTLE))
        });
        let settled = match last {
            Some(Answer::Settled(snapshot)) => Some(snapshot),
            _ => None,
        };
        let failing = owner.reports().read() || settled.is_some_and(|after| after.drain_hold);
        let lost_more = match (before, settled) {
            (Some(before), Some(after)) => after.events_lost > before.events_lost,
            _ => false,
        };
        let status = if unstopped {
            exit::SOURCE_NOT_STOPPED
        } else if settled.is_none() || failing || lost_more {
            exit::ERROR
        } else {
            status
        };
        let last = settled.unwrap_or_else(|| owner.snapshot());
        if status == exit::ERROR || last.events_lost + last.events_pending > 0 {
            let reports: Vec<Report> = owner.reports().try_iter().collect();
            let line = super::loss_line(&last, last.events_lost + last.events_pending, &reports);
            let _ = writeln!(self.err, "{line}");
        }
        if unstopped {
            if let Some(end_call) = &self.end_call {
                end_call.end(exit::SOURCE_NOT_STOPPED);
            }
        } else {
            self.disarm();
        }
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
        self.entered = true;
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
            if let Some(entry) = super::stop_failure(name, answer) {
                match failed.iter_mut().find(|failed| failed.name == entry.name) {
                    Some(failed) => *failed = entry,
                    None if !started_failed.contains(&entry.name) => failed.push(entry),
                    None => {}
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

    fn waits(&mut self) -> Option<&mut dyn Waits> {
        Some(self)
    }

    fn arm_watchdog(&mut self, status: i32) {
        if let Some(watchdog) = &self.watchdog {
            watchdog.arm(Duration::from_millis(SHUTDOWN_BUDGET_MS), status);
        }
    }

    fn disarm_watchdog(&mut self) {
        self.disarm();
    }

    fn let_go(&self) -> Vec<SourceName> {
        self.sources
            .states()
            .filter(|(_, _, unstopped)| *unstopped)
            .map(|(name, _, _)| name)
            .collect()
    }
}

impl<F: SystemSources, L: MainLoop> Waits for Supervisor<F, L> {
    fn clock(&self) -> Duration {
        self.main_loop.now()
    }

    fn wait_answer(&mut self, pending: &Pending, bound: Duration) -> Option<Answer> {
        self.answer_within(pending, bound)
    }

    fn pause_for(&mut self, wait: Duration) {
        self.main_loop.turn(wait);
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
    use crate::daemon::{run_timed, LossCause, SETTLE_BOUND};
    use crate::floor::Switches;
    use crate::source::{CaptureSource, SourceHealth, Started};

    /// The test's loop: its time moves by each turn and by what a test adds;
    /// a turn runs one pass of the harness's owner when there is one.
    struct TestLoop {
        now: Rc<Cell<Duration>>,
        harness: Option<Rc<RefCell<Harness>>>,
        real: Duration,
        hook: Option<Box<dyn FnMut(Duration)>>,
    }

    impl MainLoop for TestLoop {
        fn turn(&mut self, wait: Duration) {
            if let Some(harness) = &self.harness {
                let mut harness = harness.borrow_mut();
                harness.clocks.advance(wait);
                harness.owner.pass(Duration::ZERO);
            }
            if !self.real.is_zero() {
                std::thread::sleep(wait.min(self.real));
            }
            self.now.set(self.now.get() + wait);
            if let Some(hook) = self.hook.as_mut() {
                hook(self.now.get());
            }
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
            real: Duration::ZERO,
            hook: None,
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
            real: Duration::ZERO,
            hook: None,
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
                    // Ruling DM-25 N1 (c): the entry now asks a source that did
                    // not stop once more; only the first stop blocks.
                    let hook: Hook = Box::new(move |_| {
                        if let Some(now) = blocked.borrow_mut().take() {
                            seen.set(Some(watched.write_calls().len()));
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
        // Ruling DM-25 N1 (c): a stop that answers Timeout turns the entry's
        // status into 5, where it answered the status that it was given.
        assert_eq!(outcome.status, exit::SOURCE_NOT_STOPPED);
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

    struct Emitter {
        total: u64,
        sent: Arc<AtomicU64>,
        quit: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl CaptureSource for Emitter {
        fn begin(&mut self, ctx: SourceContext) -> Result<Started, CaptureError> {
            let (total, sent, quit) = (self.total, self.sent.clone(), self.quit.clone());
            self.thread = Some(std::thread::spawn(move || {
                for _ in 0..total {
                    if quit.load(Ordering::SeqCst) {
                        return;
                    }
                    if ctx.sink().emit_event(key()) == crate::sink::Emitted::Queued {
                        sent.fetch_add(1, Ordering::SeqCst);
                    }
                }
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
    }

    #[test]
    fn a_drain_failing_stop_returns_within_2_s_with_an_emit_parked_and_every_event_is_stored() {
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        let (handle, writer) = crate::sink::channel(16);
        let thread = crate::daemon::owner::spawn(crate::daemon::owner::OwnerParts {
            writer,
            store: store.clone(),
            clocks: crate::daemon::maintenance::SystemClocks::new(),
            floor: open_floor(),
            cap: halvo_control::model::StorageCap::DEFAULT,
            shred_chunk: crate::daemon::maintenance::SHRED_CHUNK,
        })
        .unwrap();
        let owner = Watched::new(thread, Box::new(Out::default()));
        assert!(matches!(
            owner.handle.call(Command::ClosePause),
            Ok(Answer::Resumed(_))
        ));
        let sent = Arc::new(AtomicU64::new(0));
        let first = Rc::new(Cell::new(true));
        let counted = sent.clone();
        let made = Rc::new(RefCell::new(Vec::new()));
        let mut supervisor = Supervisor::new(
            Platform::Macos,
            Makes {
                make: Box::new(move |_| {
                    Some(Box::new(Emitter {
                        total: if first.replace(false) { 1_500 } else { 0 },
                        sent: counted.clone(),
                        quit: Arc::default(),
                        thread: None,
                    }))
                }),
                made: made.clone(),
                secure: Arc::default(),
            },
            real_loop(),
        );
        let ctx = SourceContext::with_floor(handle, open_floor());
        supervisor.start_sources(&ctx, &[SourceName::Input]);
        drop(ctx);
        let waited = Instant::now();
        let mut stop_took = None;
        while waited.elapsed() < Duration::from_secs(20) {
            let before = Instant::now();
            supervisor.tick(Some(&owner));
            if supervisor.board().capture == Capture::Stopped {
                stop_took = Some(before.elapsed());
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let stop_took = stop_took.expect("the drain never failed");
        assert!(
            stop_took < Duration::from_secs(2),
            "the stop took {stop_took:?}"
        );
        store.fail_every_write(None);
        let healed = Instant::now();
        while supervisor.board().capture == Capture::Stopped
            && healed.elapsed() < Duration::from_secs(20)
        {
            supervisor.tick(Some(&owner));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(made.borrow().len(), 2, "the sources did not start again");
        let settled = match owner.handle.call(Command::Settle) {
            Ok(Answer::Settled(snapshot)) => snapshot,
            other => panic!("{other:?}"),
        };
        let sent = sent.load(Ordering::SeqCst);
        assert!(sent > 512, "only {sent} events were sent");
        assert_eq!(settled.events_lost, 0);
        assert_eq!(settled.events_persisted, sent, "{settled:?}");
        RunSources::stop(&mut supervisor);
        assert!(matches!(
            owner.handle.call(Command::Shutdown),
            Ok(Answer::ShutDown(_))
        ));
        assert!(owner.join().is_ok());
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
    fn a_permission_that_reads_not_determined_stops_no_source() {
        let (mut f, answers, _) = probed(fixture(Factory::new()));
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::NotDetermined));
        f.start(&two());
        f.tick();
        for _ in 0..3 {
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::NotDetermined));
            f.later(PROBE_INTERVAL);
        }
        assert_eq!(f.state(SourceName::Input), SourceState::Running);
        assert!(f.supervisor.factory().calls.of("stop input").is_empty());
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
            real: Duration::from_millis(20),
            hook: None,
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

    #[test]
    fn the_probe_runs_every_3_s_while_a_permission_is_missing_and_a_client_polls() {
        let (mut f, answers, asked) = probed(fixture(Factory::new()));
        let not_granted = || input_monitoring(GrantState::NotGranted);
        answers.borrow_mut().push_back(not_granted());
        f.start(&two());
        f.tick();
        assert_eq!(asked.borrow().len(), 1);
        f.later(PROBE_INTERVAL_MISSING);
        assert_eq!(
            asked.borrow().len(),
            1,
            "a probe within 60 s with no client"
        );
        f.supervisor.set_client_polls(true);
        answers.borrow_mut().push_back(not_granted());
        f.tick();
        assert_eq!(asked.borrow().len(), 2, "no probe once a client polls");
        answers.borrow_mut().push_back(not_granted());
        f.later(PROBE_INTERVAL_MISSING);
        assert_eq!(
            asked.borrow().len(),
            3,
            "no probe 3 s after a missing answer"
        );
        f.supervisor.set_client_polls(false);
        answers.borrow_mut().push_back(not_granted());
        f.later(PROBE_INTERVAL_MISSING);
        assert_eq!(asked.borrow().len(), 3, "a probe 3 s after with no client");
        f.later(PROBE_INTERVAL - PROBE_INTERVAL_MISSING);
        assert_eq!(
            asked.borrow().len(),
            4,
            "no probe 60 s after with no client"
        );
    }

    #[test]
    fn the_entry_exits_1_when_events_were_lost_between_its_two_settles() {
        let store = ModelStore::new();
        let watched = store.clone();
        let mut f = fixture_on_owner(
            Factory::adjusting(move |source| {
                if source.name == SourceName::Input {
                    let watched = watched.clone();
                    let hook: Hook = Box::new(move |ctx| {
                        watched.fail_every_write(Some(WriteFault::Refused));
                        let _ = ctx.sink().emit_event(key());
                    });
                    source.on_stop = Some(hook);
                }
            }),
            store.clone(),
        );
        f.start(&two());
        let link = f.link.take().unwrap();
        let outcome = f.supervisor.shutdown_entry(&link, exit::CLEAN).unwrap();
        assert_eq!(outcome.settled.map(|s| s.events_lost), Some(1));
        assert_eq!(outcome.status, exit::ERROR);
    }

    #[test]
    fn owner_death_ends_the_wait_before_the_window() {
        let mut factory = Factory::adjusting(|source| {
            if source.name == SourceName::Foreground {
                source.script = (0..=BATCH_MAX_EVENTS).map(|_| foreground_now()).collect();
            }
        });
        factory.calls = Factory::new().calls;
        let mut supervisor = Supervisor::new(Platform::Macos, factory, real_loop());
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let started = Instant::now();
        let outcome = run_timed(
            &run_of(3_600),
            &mut supervisor,
            PanickingStore(ModelStore::new()),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            Duration::from_millis(500),
        );
        std::panic::set_hook(quiet);
        assert_eq!(outcome.unwrap().status, exit::ERROR);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the run waited {:?} after its owner died",
            started.elapsed()
        );
    }

    #[test]
    fn a_grant_after_the_start_gives_the_timed_run_status_4() {
        let answers: Answers = Rc::default();
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::NotGranted));
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::Granted));
        let mut supervisor = Supervisor::new(Platform::Macos, Factory::new(), real_loop())
            .with_probe(Box::new(FakeProbe {
                answers: answers.clone(),
                asked: Rc::default(),
            }));
        let outcome = run_timed(
            &run_of(600),
            &mut supervisor,
            ModelStore::new(),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        assert_eq!(outcome.status, exit::RESTART);
    }

    #[test]
    fn an_input_source_with_a_hold_that_did_not_stop_prints_the_secure_input_line() {
        struct Holding(Factory);
        impl SourceFactory for Holding {
            fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
                self.0.make(name)
            }
        }
        impl SystemSources for Holding {
            fn holds_secure_input(&self) -> bool {
                true
            }
        }
        let mut f = fixture(Holding(Factory::adjusting(|source| {
            if source.name == SourceName::Input {
                source.stop = Err(CaptureError::Timeout {
                    stage: "input_tap_join",
                    ms: 1_000,
                });
            }
        })));
        f.start(&two());
        let failed = RunSources::stop(&mut f.supervisor);
        assert_eq!(failed.len(), 1);
        assert_eq!(
            f.out.text(),
            format!("{}\n", hold_stays_line(std::process::id()))
        );
    }

    /// The sources of a run that only emits its script at its start; its wait
    /// sleeps `linger`, and its stop moves `at_stop` past the owner's retry.
    struct OneShot {
        script: Vec<crate::event::FloorEvent>,
        linger: Duration,
        at_stop: Option<crate::daemon::model_store::ManualClocks>,
    }

    impl RunSources for OneShot {
        fn start(
            &mut self,
            ctx: &SourceContext,
            _wanted: &[SourceName],
            _hold_after: Option<Duration>,
        ) -> Vec<FailedSource> {
            for event in self.script.drain(..) {
                let _ = ctx.sink().emit_event(event);
            }
            Vec::new()
        }

        fn wait(&mut self, _window: Duration, _stop: &AtomicBool) -> bool {
            std::thread::sleep(self.linger);
            false
        }

        fn stop(&mut self) -> Vec<FailedSource> {
            if let Some(clocks) = &self.at_stop {
                clocks.advance(4 * RETRY_FIRST);
                std::thread::sleep(Duration::from_millis(300));
            }
            Vec::new()
        }

        fn figures(&self) -> SourceCounters {
            SourceCounters::default()
        }

        fn hold_refusal(&self) -> Option<DaemonError> {
            None
        }
    }

    #[test]
    fn a_batch_dropped_while_the_sources_stop_makes_the_run_exit_1() {
        use crate::daemon::model_store::{noon, ManualClocks, WriteScript};
        let model = ModelStore::new();
        model.script_writes([
            WriteScript::Fail(WriteFault::Io(io::ErrorKind::Other)),
            WriteScript::Fail(WriteFault::Refused),
        ]);
        let clocks = ManualClocks::at(noon());
        let mut run = OneShot {
            script: (0..600).map(|_| foreground_now()).collect(),
            linger: Duration::ZERO,
            at_stop: Some(clocks.clone()),
        };
        let outcome = run_timed(
            &run_of(1),
            &mut run,
            model.clone(),
            clocks,
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        let loss = outcome.loss.clone().expect("no loss line");
        assert_eq!(outcome.report.events_received, 600, "{}", loss.line);
        assert_eq!(outcome.report.events_persisted, 88, "{}", loss.line);
        assert_eq!(loss.cause, Some(LossCause::LostDuringStop), "{}", loss.line);
        assert_eq!(outcome.status, exit::ERROR);
    }

    #[test]
    fn an_owner_that_panicked_gives_the_owner_ended_line_and_exit_1_at_once() {
        let mut run = OneShot {
            script: (0..600).map(|_| foreground_now()).collect(),
            linger: Duration::from_millis(300),
            at_stop: None,
        };
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let started = Instant::now();
        let outcome = run_timed(
            &run_of(1),
            &mut run,
            PanickingStore(ModelStore::new()),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        );
        std::panic::set_hook(quiet);
        let outcome = outcome.unwrap();
        let took = started.elapsed();
        let loss = outcome.loss.clone().expect("no loss line");
        assert_eq!(
            loss.line,
            "halvo-capture: owner ended; events accepted and not persisted=600"
        );
        assert_eq!(loss.cause, Some(LossCause::OwnerEnded));
        assert_eq!(outcome.status, exit::ERROR);
        assert!(took < Duration::from_millis(1_500), "the run took {took:?}");
    }

    #[test]
    fn a_source_that_died_in_the_window_is_named_in_the_report_and_the_run_exits_3() {
        let mut supervisor = Supervisor::new(
            Platform::Macos,
            Factory::adjusting(|source| {
                if source.name == SourceName::Foreground {
                    source.health.set(SourceState::Failed {
                        reason: HealthReason::ThreadEnded,
                        error: CaptureError::ThreadEnded,
                    });
                }
            }),
            real_loop(),
        );
        let outcome = run_timed(
            &run_of(1),
            &mut supervisor,
            ModelStore::new(),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        let text = outcome.report.render();
        assert!(
            text.contains(
                "\nfailed_source=foreground: the capture thread had ended before the source was shut down\n"
            ),
            "{text}"
        );
        assert_eq!(outcome.status, exit::TIMED_RUN_SOURCE_FAILED);
    }

    const OPEN: u8 = 0;
    const CLOSED: u8 = 1;
    const FAILING: u8 = 2;

    /// A store whose writes wait while its mode reads CLOSED (at most 60 s),
    /// fail at once with an I/O fault while it reads FAILING, and go to the
    /// model store while it reads OPEN.
    struct GateStore {
        inner: ModelStore,
        mode: Arc<std::sync::atomic::AtomicU8>,
    }

    impl MaintainStore for GateStore {
        fn expire_due(
            &mut self,
            now: std::time::SystemTime,
            max: usize,
        ) -> Result<Pass, MaintFault> {
            self.inner.expire_due(now, max)
        }

        fn evict_to(&mut self, bound: ByteBound, max: usize) -> Result<Pass, MaintFault> {
            self.inner.evict_to(bound, max)
        }

        fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
            self.inner.usage()
        }
    }

    impl CaptureStore for GateStore {
        fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
            let until = Instant::now() + Duration::from_secs(60);
            while self.mode.load(Ordering::SeqCst) == CLOSED && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(1));
            }
            if self.mode.load(Ordering::SeqCst) == FAILING {
                return Err(WriteFault::Io(io::ErrorKind::Other));
            }
            self.inner.write_batch(write)
        }

        fn volume_free_bytes(&self) -> io::Result<u64> {
            self.inner.volume_free_bytes()
        }
    }

    /// The end of a timed run's cut stop path: SHUTDOWN_BUDGET_MS less the
    /// margin before the watchdog's deadline (ruling DM-25 N2 (a)).
    const CUT: u64 = SHUTDOWN_BUDGET_MS - crate::daemon::STOP_PATH_MARGIN_MS;

    /// The stop path of a timed run on the injected clock with every source
    /// at its stop deadline (input 1 s, foreground 1 s, screen 6 s), a drain
    /// wait that runs most of its 2 s while the store is closed, a settle whose
    /// writes fail for its whole 5 s, and a Shutdown whose write waits: the
    /// parts would take about 1.9 + 8 + 5 + 5 s, and the whole path ends at
    /// its cut, SHUTDOWN_BUDGET_MS less STOP_PATH_MARGIN_MS.
    #[test]
    fn the_stop_path_with_every_wait_at_its_limit_ends_inside_the_budget() {
        let mode = Arc::new(std::sync::atomic::AtomicU8::new(CLOSED));
        let now: Rc<Cell<Duration>> = Rc::default();
        let stopped: Rc<Cell<Option<Duration>>> = Rc::default();
        let gate = mode.clone();
        let main_loop = TestLoop {
            now: now.clone(),
            harness: None,
            real: Duration::from_millis(1),
            hook: Some(Box::new(move |at| {
                if at >= Duration::from_millis(2_900) && at < Duration::from_secs(9) {
                    gate.store(OPEN, Ordering::SeqCst);
                }
                if at >= Duration::from_millis(15_500) {
                    gate.store(CLOSED, Ordering::SeqCst);
                }
            })),
        };
        let deadlines = [
            (SourceName::Input, Duration::from_secs(1)),
            (SourceName::Foreground, Duration::from_secs(1)),
            (SourceName::Screen, Duration::from_secs(6)),
        ];
        let (stop_clock, stop_mode, first_stop) = (now.clone(), mode.clone(), stopped.clone());
        let factory = Factory::adjusting(move |source| {
            if source.name == SourceName::Foreground {
                source.script = (0..600).map(|_| foreground_now()).collect();
            }
            let deadline = deadlines
                .iter()
                .find(|(name, _)| *name == source.name)
                .map_or(Duration::ZERO, |(_, deadline)| *deadline);
            let (clock, mode, first) = (stop_clock.clone(), stop_mode.clone(), first_stop.clone());
            let hook: Hook = Box::new(move |_| {
                if first.get().is_none() {
                    first.set(Some(clock.get()));
                }
                mode.store(FAILING, Ordering::SeqCst);
                clock.set(clock.get() + deadline);
            });
            source.on_stop = Some(hook);
            source.stop = Err(CaptureError::Timeout {
                stage: "stop",
                ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
            });
        });
        let mut supervisor = Supervisor::new(Platform::Macos, factory, main_loop);
        let mut run = run_of(1);
        run.switches =
            crate::daemon::SwitchFlags(vec![(crate::floor::Switch::ScreenActivity, true)]);
        let store = GateStore {
            inner: ModelStore::new(),
            mode: mode.clone(),
        };
        let outcome = run_timed(
            &run,
            &mut supervisor,
            store,
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        mode.store(OPEN, Ordering::SeqCst);
        let window = Duration::from_secs(1);
        let path = now.get().saturating_sub(window);
        let first_stop = stopped.get().expect("no source was stopped");
        assert!(
            first_stop >= window + Duration::from_millis(1_800),
            "the drain wait ended at {first_stop:?}"
        );
        assert_eq!(
            supervisor.factory().calls.of("stop "),
            ["stop input", "stop foreground", "stop screen"]
        );
        assert_eq!(
            outcome.loss.as_ref().and_then(|loss| loss.cause),
            Some(LossCause::ShutdownUnanswered),
            "{:?}",
            outcome.loss
        );
        assert!(
            path <= Duration::from_millis(CUT) + Duration::from_millis(50),
            "the stop path took {path:?} on the injected clock"
        );
        assert!(
            path >= Duration::from_millis(CUT) - Duration::from_millis(500),
            "the stop path took {path:?}: the waits did not reach the budget"
        );
    }

    /// The pid, the process's start time and the status of each ending that a
    /// recorder took.
    type Ended = Arc<Mutex<Vec<(u32, Option<u64>, i32)>>>;

    /// What a stop for capture off left: the fixture, the recorder of its
    /// ending, its records and its answer.
    type CaptureOffCase = (
        Fixture<Factory>,
        Recorder,
        Arc<Mutex<Vec<EndRecord>>>,
        Option<CaptureOff>,
    );

    /// What an ending would have ended: the pid, the process's start time
    /// where the system gives it, and the status. It ends nothing.
    #[derive(Clone, Default)]
    struct Recorder {
        ended: Ended,
        calls: Option<crate::daemon::sources::tests::Calls>,
    }

    impl EndCall for Recorder {
        fn end(&self, status: i32) {
            self.ended
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((std::process::id(), process_start_seconds(), status));
            if let Some(calls) = &self.calls {
                calls.push(format!("end {status}"));
            }
        }
    }

    impl Recorder {
        fn statuses(&self) -> Vec<i32> {
            self.ended
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .map(|(_, _, status)| *status)
                .collect()
        }
    }

    #[cfg(target_os = "macos")]
    fn process_start_seconds() -> Option<u64> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let read = unsafe {
            libc::proc_pidinfo(
                std::process::id() as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                (&raw mut info).cast(),
                size,
            )
        };
        (read == size).then_some(info.pbi_start_tvsec)
    }

    #[cfg(not(target_os = "macos"))]
    fn process_start_seconds() -> Option<u64> {
        None
    }

    struct Records {
        calls: crate::daemon::sources::tests::Calls,
        records: Arc<Mutex<Vec<EndRecord>>>,
    }

    impl Marker for Records {
        fn write(&mut self, record: &EndRecord) -> io::Result<()> {
            self.calls.push(format!(
                "record {}",
                if record.final_figures {
                    "final"
                } else {
                    "first"
                }
            ));
            self.records
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(record.clone());
            Ok(())
        }
    }

    /// Waits on the real clock until `recorder` holds a call or 5 s have
    /// passed, and gives the time since `from`.
    fn first_call(recorder: &Recorder, from: Instant) -> Duration {
        while recorder.statuses().is_empty() && from.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        from.elapsed()
    }

    /// Ruling DM-25 W: a second arm keeps the earlier deadline, whichever arm
    /// set it, and carries the later status.
    #[test]
    fn the_watchdog_ends_at_its_earlier_deadline_with_its_later_status() {
        let recorder = Recorder::default();
        let watchdog = Watchdog::start(Arc::new(recorder.clone()));
        let armed = Instant::now();
        watchdog.arm(Duration::from_millis(200), exit::ERROR);
        watchdog.arm(Duration::from_secs(60), exit::SOURCE_NOT_STOPPED);
        let took = first_call(&recorder, armed);
        assert_eq!(
            recorder.statuses(),
            [exit::SOURCE_NOT_STOPPED],
            "a later arm did not carry its status"
        );
        assert!(
            took >= Duration::from_millis(200) && took < Duration::from_secs(5),
            "it ended after {took:?}, not at the earlier deadline of 200 ms"
        );
        let moved = Recorder::default();
        let earlier = Watchdog::start(Arc::new(moved.clone()));
        let armed_late = Instant::now();
        earlier.arm(Duration::from_secs(60), exit::CLEAN);
        earlier.arm(Duration::from_millis(200), exit::SOURCE_NOT_STOPPED);
        let took_late = first_call(&moved, armed_late);
        assert_eq!(
            moved.statuses(),
            [exit::SOURCE_NOT_STOPPED],
            "a second arm with an earlier deadline did not move it; it ended after {took_late:?}"
        );
        assert!(
            took_late >= Duration::from_millis(200),
            "it ended after {took_late:?}"
        );
        let (pid, start, _) = recorder.ended.lock().unwrap()[0];
        assert_eq!(pid, std::process::id());
        if cfg!(target_os = "macos") {
            assert!(start.is_some_and(|start| start > 0));
        }
    }

    /// Ruling DM-25 W: a disarm takes the arm back, and a later arm counts.
    #[test]
    fn a_disarmed_watchdog_ends_nothing_and_a_later_arm_counts_again() {
        let recorder = Recorder::default();
        let watchdog = Watchdog::start(Arc::new(recorder.clone()));
        watchdog.arm(Duration::from_millis(100), exit::ERROR);
        watchdog.disarm();
        assert_eq!(watchdog.armed(), None);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            recorder.statuses().is_empty(),
            "a disarmed watchdog ended with {:?}",
            recorder.statuses()
        );
        let armed = Instant::now();
        watchdog.arm(Duration::from_millis(100), exit::SOURCE_NOT_STOPPED);
        let took = first_call(&recorder, armed);
        assert_eq!(recorder.statuses(), [exit::SOURCE_NOT_STOPPED]);
        assert!(
            took >= Duration::from_millis(100),
            "it ended after {took:?}"
        );
    }

    #[test]
    fn an_unarmed_watchdog_ends_nothing() {
        let recorder = Recorder::default();
        let _watchdog = Watchdog::start(Arc::new(recorder.clone()));
        std::thread::sleep(Duration::from_millis(300));
        assert!(recorder.statuses().is_empty());
    }

    fn timeout() -> CaptureError {
        CaptureError::Timeout {
            stage: "input_tap_join",
            ms: 1_000,
        }
    }

    fn capture_off_case(
        factory: Factory,
        second: Option<Result<(), CaptureError>>,
    ) -> CaptureOffCase {
        let mut f = fixture_on_owner(factory, ModelStore::new());
        let calls = f.supervisor.factory().calls.clone();
        let recorder = Recorder {
            ended: Arc::default(),
            calls: Some(calls.clone()),
        };
        let records: Arc<Mutex<Vec<EndRecord>>> = Arc::default();
        f.supervisor = std::mem::replace(
            &mut f.supervisor,
            Supervisor::new(
                Platform::Macos,
                Factory::new(),
                TestLoop {
                    now: Rc::default(),
                    harness: None,
                    real: Duration::ZERO,
                    hook: None,
                },
            ),
        )
        .with_ending(Arc::new(recorder.clone()))
        .with_marker(Box::new(Records {
            calls: calls.clone(),
            records: records.clone(),
        }));
        f.start(&two());
        f.tick();
        f.link.as_ref().unwrap().emit(3);
        f.link.as_ref().unwrap().harness.borrow_mut().passes(3);
        if let Some(second) = second {
            f.supervisor
                .factory_mut()
                .on_second_stop(SourceName::Input, second);
        }
        let link = f.link.take().unwrap();
        let replies = calls.clone();
        let outcome = f
            .supervisor
            .stop_for_capture_off(&link, StopKind::Pause, &mut |board| {
                replies.push(format!("reply capture={:?}", board.capture));
            });
        f.link = Some(link);
        (f, recorder, records, outcome)
    }

    #[test]
    fn a_stop_for_capture_off_that_meets_a_timeout_ends_with_5_in_the_order_of_pl3() {
        let started = Duration::ZERO;
        let (f, recorder, records, outcome) = capture_off_case(
            Factory::adjusting(|source| {
                if source.name == SourceName::Input {
                    source.stop = Err(timeout());
                }
            }),
            None,
        );
        assert_eq!(outcome, Some(CaptureOff::Ended(exit::SOURCE_NOT_STOPPED)));
        let calls = f.supervisor.factory().calls.taken();
        let after_start: Vec<&str> = calls
            .iter()
            .map(String::as_str)
            .filter(|call| !call.starts_with("begin "))
            .collect();
        assert_eq!(
            after_start,
            [
                "stop input",
                "reply capture=Stopped",
                "record first",
                "stop input",
                "stop foreground",
                "record final",
                "end 5",
            ],
            "{calls:?}"
        );
        assert_eq!(recorder.statuses(), [exit::SOURCE_NOT_STOPPED]);
        assert_eq!(
            f.supervisor.watchdog_armed(),
            Some(exit::SOURCE_NOT_STOPPED)
        );
        let records = records.lock().unwrap().clone();
        assert_eq!(records[0].kind, StopKind::Pause);
        assert_eq!(records[0].not_stopped[0].0, SourceName::Input);
        assert!(!records[0].final_figures);
        assert!(records[1].final_figures);
        assert_eq!(records[1].events_persisted, 3, "{:?}", records[1]);
        assert!(f.now.get() - started < Duration::from_millis(SHUTDOWN_BUDGET_MS));
        let mut f = f;
        let link = f.link.take().unwrap();
        let again = f
            .supervisor
            .stop_for_capture_off(&link, StopKind::Pause, &mut |_| {});
        assert_eq!(again, None, "a second trigger ran the end again");
    }

    #[test]
    fn a_stop_for_capture_off_draws_the_barrier_before_its_first_stop() {
        let mut f = fixture_on_owner(
            Factory::adjusting(|source| {
                if source.name == SourceName::Input {
                    source.stop = Err(timeout());
                }
            }),
            ModelStore::new(),
        );
        f.supervisor = std::mem::replace(
            &mut f.supervisor,
            Supervisor::new(
                Platform::Macos,
                Factory::new(),
                TestLoop {
                    now: Rc::default(),
                    harness: None,
                    real: Duration::ZERO,
                    hook: None,
                },
            ),
        )
        .with_ending(Arc::new(Recorder::default()));
        f.start(&two());
        f.tick();
        f.link.as_ref().unwrap().emit(2);
        let link = f.link.take().unwrap();
        let outcome = f
            .supervisor
            .stop_for_capture_off(&link, StopKind::Pause, &mut |_| {});
        assert_eq!(outcome, Some(CaptureOff::Ended(exit::SOURCE_NOT_STOPPED)));
        let snapshot = link.settle();
        assert_eq!(snapshot.discarded_by_barrier, 2, "{snapshot:?}");
        assert_eq!(snapshot.events_persisted, 0);
    }

    #[test]
    fn a_stop_for_capture_off_that_meets_an_os_error_ends_the_same_way() {
        let (_f, recorder, _records, outcome) = capture_off_case(
            Factory::adjusting(|source| {
                if source.name == SourceName::Input {
                    source.stop = Err(CaptureError::Os {
                        stage: "stop_capture",
                        domain: "ScreenCaptureKit",
                        code: -3808,
                    });
                }
            }),
            None,
        );
        assert_eq!(outcome, Some(CaptureOff::Ended(exit::SOURCE_NOT_STOPPED)));
        assert_eq!(recorder.statuses(), [exit::SOURCE_NOT_STOPPED]);
    }

    #[test]
    fn a_timeout_then_ok_at_the_entrys_pass_still_ends_with_5() {
        let (f, recorder, _records, outcome) = capture_off_case(
            Factory::adjusting(|source| {
                if source.name == SourceName::Input {
                    source.stop = Err(timeout());
                }
            }),
            Some(Ok(())),
        );
        assert_eq!(outcome, Some(CaptureOff::Ended(exit::SOURCE_NOT_STOPPED)));
        assert_eq!(recorder.statuses(), [exit::SOURCE_NOT_STOPPED]);
        assert_eq!(f.supervisor.factory().calls.of("stop input").len(), 2);
    }

    #[test]
    fn a_lost_grant_whose_stop_does_not_stop_ends_the_process_with_5() {
        let (mut f, answers, _) = probed(fixture_on_owner(
            Factory::adjusting(|source| {
                if source.name == SourceName::Input {
                    source.stop = Err(timeout());
                }
            }),
            ModelStore::new(),
        ));
        let recorder = Recorder::default();
        f.supervisor = std::mem::replace(
            &mut f.supervisor,
            Supervisor::new(
                Platform::Macos,
                Factory::new(),
                TestLoop {
                    now: Rc::default(),
                    harness: None,
                    real: Duration::ZERO,
                    hook: None,
                },
            ),
        )
        .with_ending(Arc::new(recorder.clone()));
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::Granted));
        f.start(&two());
        f.tick();
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::NotGranted));
        assert_eq!(f.later(PROBE_INTERVAL), Some(Ending::SourceNotStopped));
        assert_eq!(recorder.statuses(), [exit::SOURCE_NOT_STOPPED]);
    }

    #[test]
    fn a_rebuild_whose_stop_does_not_stop_keeps_the_process_up_and_starts_no_second_source() {
        let recorder = Recorder::default();
        let mut f = fixture(Factory::adjusting(|source| {
            if source.name == SourceName::Input {
                source.stop = Err(timeout());
            }
        }));
        f.supervisor = std::mem::replace(
            &mut f.supervisor,
            Supervisor::new(
                Platform::Macos,
                Factory::new(),
                TestLoop {
                    now: Rc::default(),
                    harness: None,
                    real: Duration::ZERO,
                    hook: None,
                },
            ),
        )
        .with_ending(Arc::new(recorder.clone()));
        f.start(&two());
        f.tick();
        f.supervisor
            .factory()
            .health_of(SourceName::Input)
            .set(SourceState::Failed {
                reason: HealthReason::ItemClosed,
                error: timeout(),
            });
        for _ in 0..5 {
            assert_eq!(f.later(RESTART_MAX), None);
        }
        assert!(
            recorder.statuses().is_empty(),
            "the rebuild ended the process"
        );
        assert_eq!(f.supervisor.factory().calls.of("begin input").len(), 1);
    }

    #[test]
    fn the_secure_input_line_is_written_when_the_input_stop_answers() {
        struct Holding(Factory);
        impl SourceFactory for Holding {
            fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
                self.0.make(name)
            }
        }
        impl SystemSources for Holding {
            fn holds_secure_input(&self) -> bool {
                true
            }
        }
        let out = Out::default();
        let seen = out.clone();
        let written_at_foreground: Rc<Cell<Option<bool>>> = Rc::default();
        let at_foreground = written_at_foreground.clone();
        let mut f = fixture(Holding(Factory::adjusting(move |source| {
            if source.name == SourceName::Input {
                source.stop = Err(timeout());
            }
            if source.name == SourceName::Foreground {
                let (seen, at_foreground) = (seen.clone(), at_foreground.clone());
                let hook: Hook = Box::new(move |_| {
                    at_foreground.set(Some(seen.text().contains("secure input stays on")));
                });
                source.on_stop = Some(hook);
            }
        })));
        f.supervisor = std::mem::replace(
            &mut f.supervisor,
            Supervisor::new(
                Platform::Macos,
                Holding(Factory::new()),
                TestLoop {
                    now: Rc::default(),
                    harness: None,
                    real: Duration::ZERO,
                    hook: None,
                },
            ),
        )
        .with_err(Box::new(out.clone()));
        f.start(&two());
        let _ = RunSources::stop(&mut f.supervisor);
        assert_eq!(
            written_at_foreground.get(),
            Some(true),
            "the line came after the next source's stop"
        );
    }

    #[test]
    fn a_timed_run_whose_source_does_not_stop_names_it_and_ends_by_the_routine() {
        for (signal, status) in [
            (false, exit::TIMED_RUN_SOURCE_FAILED),
            (true, exit::TIMED_RUN_INTERRUPTED),
        ] {
            let recorder = Recorder::default();
            let mut supervisor = Supervisor::new(
                Platform::Macos,
                Factory::adjusting(|source| {
                    if source.name == SourceName::Input {
                        source.stop = Err(timeout());
                    }
                }),
                real_loop(),
            )
            .with_ending(Arc::new(recorder.clone()));
            let outcome = run_timed(
                &run_of(1),
                &mut supervisor,
                ModelStore::new(),
                crate::daemon::maintenance::SystemClocks::new(),
                &AtomicBool::new(signal),
                SETTLE_BOUND,
            )
            .unwrap();
            assert_eq!(outcome.status, status);
            assert_eq!(outcome.let_go, ["input"]);
            assert!(outcome
                .report
                .render()
                .contains("\nfailed_source=input: the capture source timed out at input_tap_join after 1000 ms\n"));
            assert_eq!(supervisor.watchdog_armed(), Some(status));
            let mut err = Vec::new();
            crate::daemon::finish_run(&outcome, outcome.status, &recorder, &mut err);
            assert_eq!(recorder.statuses(), [status]);
            assert_eq!(
                String::from_utf8_lossy(&err),
                "halvo-capture: the source input did not stop; the process ends without waiting for it\n"
            );
        }
    }

    #[test]
    #[should_panic(expected = "a test reached the process's ending call with status 3")]
    fn the_answer_of_a_run_that_let_a_source_go_makes_the_ending_call() {
        let mut supervisor = Supervisor::new(
            Platform::Macos,
            Factory::adjusting(|source| {
                if source.name == SourceName::Input {
                    source.stop = Err(timeout());
                }
            }),
            real_loop(),
        )
        .with_ending(Arc::new(Recorder::default()));
        let outcome = run_timed(
            &run_of(1),
            &mut supervisor,
            ModelStore::new(),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let _ = crate::daemon::outcome_answer(&outcome, &mut out, &mut err);
    }

    #[test]
    fn a_timed_run_whose_sources_all_stopped_returns_without_the_routine() {
        let recorder = Recorder::default();
        let mut supervisor = Supervisor::new(Platform::Macos, Factory::new(), real_loop())
            .with_ending(Arc::new(recorder.clone()));
        let outcome = run_timed(
            &run_of(1),
            &mut supervisor,
            ModelStore::new(),
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        assert!(outcome.let_go.is_empty());
        let mut err = Vec::new();
        crate::daemon::finish_run(&outcome, outcome.status, &recorder, &mut err);
        assert!(recorder.statuses().is_empty());
        assert!(err.is_empty());
    }

    #[test]
    fn the_marker_file_holds_the_last_record_whole() {
        let dir = crate::daemon::children::tests::test_dir();
        let path = dir.path().join("marker");
        let mut marker = MarkerFile::new(path.clone());
        let mut record = EndRecord {
            kind: StopKind::LostGrant,
            final_figures: false,
            not_stopped: vec![(SourceName::Input, "timeout".to_owned())],
            events_persisted: 0,
            events_lost: 0,
            events_pending: 2,
        };
        marker.write(&record).unwrap();
        record.final_figures = true;
        record.events_persisted = 2;
        record.events_pending = 0;
        marker.write(&record).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "last_stop=source_not_stopped kind=lost_grant final=yes not_stopped=input:timeout events_persisted=2 events_lost=0 events_pending=0\n"
        );
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["marker"], "a temporary file stayed");
    }

    /// The real end, run as a child of this test binary: a watchdog with the
    /// product's ending, armed for 300 ms, ends a process whose main thread
    /// blocks, with the armed status. It runs on the hosted jobs alone (the
    /// round's step names it with --ignored); no process is ended on this Mac.
    #[test]
    #[ignore = "ruling DM-24: the real end runs on the hosted jobs alone"]
    fn the_watchdog_ends_a_blocked_process_with_its_status() {
        const NAME: &str =
            "daemon::supervisor::tests::the_watchdog_ends_a_blocked_process_with_its_status";
        const CHILD: &str = "HALVO_DAEMON_WATCHDOG_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let watchdog = Watchdog::start(Arc::new(crate::daemon::main_loop::ExitNow));
            watchdog.arm(Duration::from_millis(300), exit::SOURCE_NOT_STOPPED);
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        let started = Instant::now();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                NAME,
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let took = started.elapsed();
        println!(
            "watchdog_child status={:?} took_ms={}",
            output.status.code(),
            took.as_millis()
        );
        assert_eq!(output.status.code(), Some(exit::SOURCE_NOT_STOPPED));
        assert!(took < Duration::from_secs(20), "{took:?}");
    }

    /// A pause answered late (the store closed through the drain wait and
    /// most of the pause, then failing), every source at its stop deadline
    /// and a settle whose writes fail: the settle is cut to what is left of
    /// SHUTDOWN_BUDGET_MS less STOP_PATH_MARGIN_MS, where its own part would
    /// take the path to about 19.8 s.
    #[test]
    fn the_settle_after_a_late_pause_is_cut_to_the_budget() {
        let mode = Arc::new(std::sync::atomic::AtomicU8::new(CLOSED));
        let now: Rc<Cell<Duration>> = Rc::default();
        let gate = mode.clone();
        let main_loop = TestLoop {
            now: now.clone(),
            harness: None,
            real: Duration::from_millis(1),
            hook: Some(Box::new(move |at| {
                if at >= Duration::from_millis(7_800) && at < Duration::from_secs(9) {
                    gate.store(FAILING, Ordering::SeqCst);
                }
            })),
        };
        let deadlines = [
            (SourceName::Input, Duration::from_secs(1)),
            (SourceName::Foreground, Duration::from_secs(1)),
            (SourceName::Screen, Duration::from_secs(6)),
        ];
        let stop_clock = now.clone();
        let factory = Factory::adjusting(move |source| {
            if source.name == SourceName::Foreground {
                source.script = (0..600).map(|_| foreground_now()).collect();
            }
            let deadline = deadlines
                .iter()
                .find(|(name, _)| *name == source.name)
                .map_or(Duration::ZERO, |(_, deadline)| *deadline);
            let clock = stop_clock.clone();
            let hook: Hook = Box::new(move |_| clock.set(clock.get() + deadline));
            source.on_stop = Some(hook);
        });
        let mut supervisor = Supervisor::new(Platform::Macos, factory, main_loop);
        let mut run = run_of(1);
        run.switches =
            crate::daemon::SwitchFlags(vec![(crate::floor::Switch::ScreenActivity, true)]);
        let store = GateStore {
            inner: ModelStore::new(),
            mode: mode.clone(),
        };
        let outcome = run_timed(
            &run,
            &mut supervisor,
            store,
            crate::daemon::maintenance::SystemClocks::new(),
            &AtomicBool::new(false),
            SETTLE_BOUND,
        )
        .unwrap();
        mode.store(OPEN, Ordering::SeqCst);
        let path = now.get().saturating_sub(Duration::from_secs(1));
        assert!(
            matches!(
                outcome.loss.as_ref().and_then(|loss| loss.cause),
                Some(LossCause::ShutdownUnanswered | LossCause::Pending)
            ),
            "{:?}",
            outcome.loss
        );
        assert_eq!(outcome.status, exit::ERROR);
        assert!(
            path <= Duration::from_millis(CUT) + Duration::from_millis(50),
            "the stop path took {path:?} on the injected clock"
        );
        assert!(
            path >= Duration::from_millis(CUT) - Duration::from_millis(500),
            "the stop path took {path:?}"
        );
    }

    #[test]
    fn a_lost_grant_draws_the_barrier_at_its_first_answer_that_is_not_a_stop() {
        let (mut f, answers, _) = probed(fixture_on_owner(
            Factory::adjusting(|source| {
                if source.name == SourceName::Input {
                    source.stop = Err(timeout());
                }
            }),
            ModelStore::new(),
        ));
        f.supervisor = std::mem::replace(
            &mut f.supervisor,
            Supervisor::new(
                Platform::Macos,
                Factory::new(),
                TestLoop {
                    now: Rc::default(),
                    harness: None,
                    real: Duration::ZERO,
                    hook: None,
                },
            ),
        )
        .with_ending(Arc::new(Recorder::default()));
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::Granted));
        f.start(&two());
        f.tick();
        f.link.as_ref().unwrap().emit(2);
        answers
            .borrow_mut()
            .push_back(input_monitoring(GrantState::NotGranted));
        f.now.set(f.now.get() + PROBE_INTERVAL);
        let link = f.link.take().unwrap();
        assert_eq!(
            f.supervisor.tick(Some(&link)),
            Some(Ending::SourceNotStopped)
        );
        let snapshot = link.settle();
        assert_eq!(snapshot.discarded_by_barrier, 2, "{snapshot:?}");
        assert_eq!(snapshot.events_persisted, 0);
    }

    mod review_cases {
        use super::*;
        // Unit text :56 (X-daemon-07, D-09): "It skips a source whose permission reads not granted";
        // :137 "no restart without the permission (RR-daemon-04)"; :42 "on DrainHealthy it runs the start
        // sequence if the run still wants its sources". The run wants input, but its grant reads
        // NotGranted: the start on DrainHealthy must not begin a second input source.
        #[test]
        fn the_start_on_drain_healthy_skips_an_input_source_whose_grant_reads_not_granted() {
            let store = ModelStore::new();
            let (mut f, answers, _) = probed(fixture_on_owner(Factory::new(), store.clone()));
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
            assert_eq!(f.supervisor.factory().calls.of("begin input").len(), 1);
            drain_fails(&f, &store, WriteFault::Io(io::ErrorKind::Other));
            f.tick();
            assert_eq!(f.supervisor.board().capture, Capture::Stopped);
            store.fail_every_write(None);
            f.supervisor.main_loop().turn(RETRY_FIRST);
            f.link.as_ref().unwrap().settle();
            f.tick();
            assert_eq!(
                f.supervisor.factory().calls.of("begin foreground").len(),
                2,
                "the drain did not recover in this test"
            );
            assert_eq!(
                f.supervisor.factory().calls.of("begin input").len(),
                1,
                "DrainHealthy began an input source whose Input Monitoring grant reads not granted"
            );
        }

        // Same rule, before the drain stop: a restart of input that was scheduled before the grant
        // went away must not fire after the drain recovers either (restarts scheduled before a drain
        // stop do not fire while stopped, and the permission still reads not granted after it).
        #[test]
        fn an_input_restart_scheduled_before_the_grant_went_away_does_not_fire_after_drain_recovery(
        ) {
            let store = ModelStore::new();
            let (mut f, answers, _) = probed(fixture_on_owner(Factory::new(), store.clone()));
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::Granted));
            f.start(&two());
            f.tick();
            f.supervisor
                .factory()
                .health_of(SourceName::Input)
                .set(SourceState::Failed {
                    reason: HealthReason::ItemClosed,
                    error: CaptureError::Os {
                        stage: "capture_item",
                        domain: "test",
                        code: 1,
                    },
                });
            f.tick();
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::NotGranted));
            f.later(PROBE_INTERVAL);
            // The restart due at 1 s fires in this same tick before the probe answer is read, and the
            // answer then stops it: count the begins from here on.
            let begun = f.supervisor.factory().calls.of("begin input").len();
            assert!(!f.supervisor.factory().calls.of("stop input").is_empty());
            drain_fails(&f, &store, WriteFault::Io(io::ErrorKind::Other));
            f.tick();
            store.fail_every_write(None);
            f.supervisor.main_loop().turn(RETRY_FIRST);
            f.link.as_ref().unwrap().settle();
            f.tick();
            f.later(RESTART_MAX);
            assert_eq!(
                f.supervisor.factory().calls.of("begin input").len(),
                begun,
                "an input source began again while its grant reads not granted"
            );
        }

        // Unit text :56 (X-daemon-07, D-09): the supervisor "does not restart a screen stream the user
        // stopped (code -3817, stopped reason=user) before the next explicit Resume". A drain that fails
        // and heals is no explicit Resume: the start on DrainHealthy (:42) must leave that stream off.
        #[test]
        fn the_start_on_drain_healthy_does_not_begin_a_screen_stream_the_user_stopped() {
            let store = ModelStore::new();
            let mut f = fixture_on_owner(Factory::new(), store.clone());
            let wanted = [SourceName::Foreground, SourceName::Screen];
            f.start(&wanted);
            f.tick();
            f.supervisor
                .factory()
                .health_of(SourceName::Screen)
                .set(SourceState::Stopped(StopCause::User));
            f.tick();
            f.later(RESTART_MAX);
            assert_eq!(f.supervisor.factory().calls.of("begin screen").len(), 1);
            drain_fails(&f, &store, WriteFault::Io(io::ErrorKind::Other));
            f.tick();
            assert_eq!(f.supervisor.board().capture, Capture::Stopped);
            store.fail_every_write(None);
            f.supervisor.main_loop().turn(RETRY_FIRST);
            f.link.as_ref().unwrap().settle();
            f.tick();
            assert_eq!(
                f.supervisor.factory().calls.of("begin foreground").len(),
                2,
                "the drain did not recover in this test"
            );
            f.later(RESTART_MAX);
            assert_eq!(
                f.supervisor.factory().calls.of("begin screen").len(),
                1,
                "DrainHealthy began again a screen stream that the user stopped"
            );
        }

        // Unit text :116, PL-3 part (3): "at the run's deadline or a signal a source whose answer is not a
        // stop is let go, the report carries `failed_source=<S>: <the stop error>` from the answer". Here
        // input failed after its start (health failed, restart not yet due) and then its stop at the run's
        // end answers Timeout: the report must carry the stop error of that answer.
        #[test]
        fn the_report_carries_the_stop_error_of_a_source_that_had_failed_before_the_end() {
            let timeout = CaptureError::Timeout {
                stage: "input_tap_join",
                ms: 1_000,
            };
            let answer = timeout.clone();
            let mut f = fixture(Factory::adjusting(move |source| {
                if source.name == SourceName::Input {
                    source.stop = Err(answer.clone());
                }
            }));
            assert!(f.start(&two()).is_empty());
            f.tick();
            f.supervisor
                .factory()
                .health_of(SourceName::Input)
                .set(SourceState::Failed {
                    reason: HealthReason::ItemClosed,
                    error: CaptureError::Os {
                        stage: "capture_item",
                        domain: "test",
                        code: 1,
                    },
                });
            f.tick();
            let failed = RunSources::stop(&mut f.supervisor);
            assert_eq!(
                f.supervisor
                    .factory()
                    .calls
                    .of("stop ")
                    .first()
                    .map(String::as_str),
                Some("stop input")
            );
            assert!(
                failed
                    .iter()
                    .any(|line| line.name == "input" && line.error == timeout.to_string()),
                "no failed_source=input line with the stop error {:?}: {failed:?}",
                timeout.to_string()
            );
        }

        // Unit text :28 (RR-daemon-13) "the input source stops first" and :130 "the input source stops
        // first" in every stop sequence, with PL-3 part (4) (:116) "the stop of a source whose grant went
        // away (C14 item 9)" being one of those stops. When one probe answer takes both grants away, the
        // supervisor stops input before screen (the stop order of contracts.md C12 item 7), not in the
        // fixed order of the source names (foreground, screen, input).
        #[test]
        fn two_grants_lost_in_one_answer_stop_input_before_screen() {
            let (mut f, answers, _) = probed(fixture(Factory::new()));
            let both = |grant: GrantState| {
                Probed::Answered(vec![
                    (Permission::ScreenAndSystemAudioRecording, grant),
                    (Permission::InputMonitoring, grant),
                ])
            };
            answers.borrow_mut().push_back(both(GrantState::Granted));
            f.start(&[
                SourceName::Foreground,
                SourceName::Screen,
                SourceName::Input,
            ]);
            f.tick();
            assert_eq!(f.state(SourceName::Screen), SourceState::Running);
            assert_eq!(f.state(SourceName::Input), SourceState::Running);
            answers.borrow_mut().push_back(both(GrantState::NotGranted));
            f.later(PROBE_INTERVAL);
            assert_eq!(
                f.supervisor.factory().calls.of("stop "),
                ["stop input", "stop screen"],
                "the stop for the lost grants did not stop input first"
            );
            assert_eq!(f.state(SourceName::Foreground), SourceState::Running);
        }

        // Unit text, RR-daemon-09 (line 36): the entry exits 1 with "the drain failing at the last settle"
        // (contracts.md C3.2 as ruling DM-4 items 5 to 7 give them); [Pass 36, DM-19 to DM-22]: "a Refused
        // or Sealed give-up inside the hold reports DrainHealthy while the hold goes on", so the supervisor
        // reads the hold from Snapshot.drain_hold and not from the last DrainHealthy. The entry reads only
        // the report log's `failing`, so a shutdown inside the hold after such a give-up exits 0.
        #[test]
        fn the_entry_exits_1_when_the_hold_goes_on_at_the_last_settle_after_a_refused_or_sealed_give_up(
        ) {
            for answer in [WriteFault::Refused, WriteFault::Sealed] {
                let store = ModelStore::new();
                let mut f = fixture_on_owner(Factory::new(), store.clone());
                f.start(&two());
                f.tick();
                into_the_hold(&f, &store);
                f.tick();
                drain_fails(&f, &store, answer);
                f.tick();
                assert!(
                    f.supervisor.board().drain.hold,
                    "{answer:?}: no hold before the entry"
                );
                let link = f.link.take().unwrap();
                let outcome = f.supervisor.shutdown_entry(&link, exit::CLEAN).unwrap();
                assert_eq!(
                    outcome.settled.as_ref().map(|s| s.drain_hold),
                    Some(true),
                    "{answer:?}: the last settle left the hold"
                );
                assert_eq!(
                    outcome.status,
                    exit::ERROR,
                    "{answer:?}: the entry exited {} with the hold on at the last settle",
                    outcome.status
                );
            }
        }

        // Same source: the plain hold (three set-asides of a counted fault) at the last settle exits 1.
        #[test]
        fn the_entry_exits_1_when_the_entry_starts_inside_the_hold() {
            let store = ModelStore::new();
            let mut f = fixture_on_owner(Factory::new(), store.clone());
            f.start(&two());
            f.tick();
            into_the_hold(&f, &store);
            f.tick();
            let link = f.link.take().unwrap();
            let outcome = f.supervisor.shutdown_entry(&link, exit::CLEAN).unwrap();
            assert_eq!(outcome.status, exit::ERROR);
        }

        // Unit text, RR-daemon-09 (line 36): "With the drain still failing it writes one loss line (event
        // count, first and last capture time, error kind), keeps the record for the next Status and exits
        // 1"; line 197 (inbox entry 118 (d) P1): "this unit's run driver turns events_pending of the last
        // snapshot into its loss line and exit 1". shutdown_entry returns status 1 but writes no line to
        // the daemon's error stream, and no production caller of the entry writes it either.
        #[test]
        fn the_entry_writes_one_loss_line_when_the_drain_still_fails_at_the_last_settle() {
            let store = ModelStore::new();
            let mut f = fixture_on_owner(Factory::new(), store.clone());
            f.start(&two());
            drain_fails(&f, &store, WriteFault::Io(io::ErrorKind::Other));
            let link = f.link.take().unwrap();
            let outcome = f.supervisor.shutdown_entry(&link, exit::CLEAN).unwrap();
            assert_eq!(outcome.status, exit::ERROR);
            let text = f.out.text();
            let lines: Vec<&str> = text
                .lines()
                .filter(|line| line.starts_with("halvo-capture: lost events="))
                .collect();
            assert_eq!(lines.len(), 1, "not one loss line: {text:?}");
            assert!(
                lines[0].starts_with("halvo-capture: lost events=2 first="),
                "the loss line does not count the 2 events the drain still holds: {text:?}"
            );
            assert!(
                !lines[0].ends_with("error=-"),
                "the loss line names no error kind: {text:?}"
            );
        }

        // Unit text, RR-daemon-09 (line 36): "fix: one entry. Callers here: the signal flag, the end of a
        // timed run and the grant restart." and "The entry runs once, however many triggers arrive."
        // run_timed (mod.rs) never calls shutdown_entry: it runs its own OpenPause, stop, settle sequence,
        // so after a timed run has ended the entry has not run and a later trigger runs it a second time
        // (stopping and settling again).
        #[test]
        fn the_end_of_a_timed_run_goes_through_the_entry_so_a_later_trigger_does_not_run_it() {
            let f = fixture_on_owner(Factory::new(), ModelStore::new());
            let link = f.link.unwrap();
            let mut supervisor = Supervisor::new(Platform::Macos, Factory::new(), real_loop());
            let outcome = run_timed(
                &run_of(1),
                &mut supervisor,
                ModelStore::new(),
                crate::daemon::maintenance::SystemClocks::new(),
                &AtomicBool::new(false),
                SETTLE_BOUND,
            )
            .unwrap();
            assert_eq!(outcome.status, exit::CLEAN);
            assert!(
                supervisor.shutdown_entry(&link, exit::CLEAN).is_none(),
                "the end of the timed run did not go through the shutdown entry"
            );
        }

        // Same source, the signal flag as the trigger: a timed run whose stop flag is set ends through
        // the entry too, and a second trigger answers None.
        #[test]
        fn a_signalled_timed_run_goes_through_the_entry_once() {
            let f = fixture_on_owner(Factory::new(), ModelStore::new());
            let link = f.link.unwrap();
            let mut supervisor = Supervisor::new(Platform::Macos, Factory::new(), real_loop());
            let _ = run_timed(
                &run_of(30),
                &mut supervisor,
                ModelStore::new(),
                crate::daemon::maintenance::SystemClocks::new(),
                &AtomicBool::new(true),
                SETTLE_BOUND,
            )
            .unwrap();
            assert!(
                supervisor.shutdown_entry(&link, exit::CLEAN).is_none(),
                "the signalled timed run did not go through the shutdown entry"
            );
        }

        // RR-daemon-04 fix: "Not granted: the source stops in the same tick, shows failed
        // with reason=permission, and is not restarted until the probe reads granted."
        // A permission granted at the first answer, then revoked, then granted again: the
        // source must come back (a restart of the source, or Ending::Restart), and must not
        // stay failed for the rest of the run.
        #[test]
        fn a_source_stopped_for_a_revoked_permission_comes_back_when_the_probe_reads_granted_again()
        {
            let (mut f, answers, _) = probed(fixture(Factory::new()));
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
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::Granted));
            let mut ending = f.later(PROBE_INTERVAL);
            for _ in 0..4 {
                if ending.is_some() {
                    break;
                }
                ending = f.later(RESTART_MAX);
            }
            let begun = f.supervisor.factory().calls.of("begin input").len();
            assert!(
        ending == Some(Ending::Restart)
            || (begun == 2 && f.state(SourceName::Input) == SourceState::Running),
        "the probe read granted again, yet the input source stays down: ending {:?}, begins {}, state {:?}",
        ending,
        begun,
        f.state(SourceName::Input)
    );
        }

        // RR-daemon-04 fix: "Not granted: the source stops in the same tick, shows failed
        // with reason=permission, and is not restarted until the probe reads granted."
        // X-daemon-07: "no restart without the permission (RR-daemon-04)".
        // The drain stop and its recovery are a restart path of their own (start_wanted):
        // the recovery must not start a source whose permission still reads not_granted.
        #[test]
        fn a_drain_recovery_does_not_start_a_source_whose_permission_reads_not_granted() {
            let store = ModelStore::new();
            let (mut f, answers, _) = probed(fixture_on_owner(Factory::new(), store.clone()));
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::NotGranted));
            f.start(&two());
            f.tick();
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
            drain_fails(&f, &store, WriteFault::Io(std::io::ErrorKind::Other));
            f.tick();
            store.fail_every_write(None);
            f.supervisor.main_loop().turn(RETRY_FIRST);
            f.link.as_ref().unwrap().settle();
            f.tick();
            assert!(
                f.supervisor.factory().calls.of("begin foreground").len() >= 2,
                "the drain did not recover, so the case proves nothing: {:?}",
                f.supervisor.factory().calls.of("begin ")
            );
            assert_eq!(
        f.supervisor.factory().calls.of("begin input").len(),
        1,
        "the drain recovery started the input source while Input Monitoring reads not_granted"
    );
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
        }

        // RR-daemon-04 fix: "Not granted: the source stops in the same tick, shows failed
        // with reason=permission, and is not restarted until the probe reads granted."
        // A source whose start failed holds nothing to stop, but its Board line must still
        // show failed with reason=permission once the probe reads not_granted.
        #[test]
        fn a_source_whose_start_failed_shows_reason_permission_once_the_probe_reads_not_granted() {
            let (mut f, answers, _) = probed(fixture(Factory::adjusting(|source| {
                if source.name == SourceName::Input {
                    source.begin = Err(CaptureError::Start("the tap was not made".to_owned()));
                }
            })));
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::NotGranted));
            f.start(&two());
            f.tick();
            assert!(
                matches!(
                    f.state(SourceName::Input),
                    SourceState::Failed {
                        reason: HealthReason::Permission,
                        ..
                    }
                ),
                "Input Monitoring reads not_granted, yet the input line is {:?}",
                f.state(SourceName::Input)
            );
            for _ in 0..3 {
                answers
                    .borrow_mut()
                    .push_back(input_monitoring(GrantState::NotGranted));
                f.later(PROBE_INTERVAL);
            }
            assert_eq!(
                f.supervisor.factory().calls.of("begin input").len(),
                1,
                "the input source was started again while the answer stays not_granted"
            );
        }

        // RR-daemon-04 fix: "The supervisor runs the probe child when a run starts and then
        // on a timer with a deadline: every 60 s, and every 3 s while a required permission
        // is not granted and a client polls". Once a client starts polling while a required
        // permission reads not_granted, the next probe comes within 3 s, not at the end of
        // the 60 s timer set before the client came.
        #[test]
        fn a_client_that_starts_polling_brings_the_next_probe_within_3_s() {
            let (mut f, answers, asked) = probed(fixture(Factory::new()));
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::NotGranted));
            f.start(&two());
            f.tick();
            assert_eq!(asked.borrow().len(), 1);
            f.later(Duration::from_secs(1));
            f.supervisor.set_client_polls(true);
            for _ in 0..(PROBE_INTERVAL_MISSING.as_millis() / main_loop::TICK.as_millis() + 1) {
                f.later(main_loop::TICK);
            }
            assert!(
        asked.borrow().len() >= 2,
        "a client polls and Input Monitoring reads not_granted, yet no probe ran within 3 s: {} asked",
        asked.borrow().len()
    );
        }

        // RR-daemon-04: "every 3 s while a required permission is not granted and a client
        // polls" -- the 3 s cadence holds for each later answer too, not only the first.
        #[test]
        fn while_a_client_polls_and_the_permission_stays_not_granted_the_probe_runs_every_3_s() {
            let (mut f, answers, asked) = probed(fixture(Factory::new()));
            f.supervisor.set_client_polls(true);
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::NotGranted));
            f.start(&two());
            f.tick();
            for round in 0..4 {
                answers
                    .borrow_mut()
                    .push_back(input_monitoring(GrantState::NotGranted));
                f.later(PROBE_INTERVAL_MISSING);
                assert_eq!(asked.borrow().len(), round + 2, "round {round}");
            }
            f.supervisor.set_client_polls(false);
            answers
                .borrow_mut()
                .push_back(input_monitoring(GrantState::NotGranted));
            f.later(PROBE_INTERVAL_MISSING);
            let after = asked.borrow().len();
            f.later(PROBE_INTERVAL_MISSING);
            assert_eq!(
                asked.borrow().len(),
                after,
                "with no client polling the probe still ran every 3 s"
            );
        }
    }

    mod stress_cases;
}
