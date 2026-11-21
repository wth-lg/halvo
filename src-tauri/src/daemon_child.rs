use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use halvo_control::exit;
use halvo_control::paths::DataDir;
use halvo_control::service::{DAEMON_ARG, DATA_DIR_ARG};

use crate::commands::control::ChildPid;
use crate::view_state::Hold;

/// The flag that makes the daemon watch its parent through the descriptor on its standard
/// input (contracts.md C14 item 1).
pub const LIFELINE_ARG: &str = "--lifeline";

/// How often the supervisor reads whether the child of the moment has ended.
pub const CHILD_POLL: Duration = Duration::from_millis(250);

/// The first wait of the backoff before a child starts again.
pub const BACKOFF_FIRST: Duration = Duration::from_secs(1);

/// The longest wait of the backoff; a child that lived as long starts the backoff again from
/// its first wait.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// After this many ends with status 5 in a row no further child starts.
pub const FIVES_IN_A_ROW: u32 = 3;

/// How long a quit waits for the child after it closed the descriptor, before it kills the
/// child (contracts.md C14 item 5).
pub const STOP_WAIT: Duration = Duration::from_millis(halvo_control::STOP_WAIT_MS);

/// How often a quit reads whether the child has ended.
pub const STOP_POLL: Duration = Duration::from_millis(50);

/// The program that the app starts and the arguments that stand before the daemon's own: the
/// daemon beside the app's executable with none in the app, the test binary with its child
/// entry in the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub path: PathBuf,
    pub leading: Vec<OsString>,
}

/// The daemon's arguments: `--service-run --lifeline`, and `--data-dir DIR` only when the app
/// itself got DIR.
pub fn arguments(given: Option<&Path>) -> Vec<OsString> {
    let mut args = vec![OsString::from(DAEMON_ARG), OsString::from(LIFELINE_ARG)];
    if let Some(dir) = given {
        args.push(OsString::from(DATA_DIR_ARG));
        args.push(dir.as_os_str().to_owned());
    }
    args
}

/// How a child ended: with an exit status, by a signal, or before it ran, when its start
/// failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Status(i32),
    Signal,
    NotStarted,
}

/// What the supervisor does after the child of the moment has ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// No start again: where a daemon then answers, another daemon holds the directory, and
    /// the app talks to it with no expected pid and no descriptor.
    Attach,
    StartNow,
    StartAfter(Duration),
    /// No further child in this life of the app.
    NoMoreChild,
}

/// What the rule keeps from one end of a child to the next: the last wait of the backoff and
/// the count of ends with status 5 in a row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Restarts {
    waited: Option<Duration>,
    fives: u32,
}

/// The rule of a start again (contracts.md C14 item 4), by how the child ended and how long it
/// lived: 0 no start; 4 after a life of 60 s or more a start at once; 5 the backoff, and no
/// child after the third 5 in a row; 1, a 4 within 60 s of its start, a status that this list
/// does not name, an end by a signal and a failed start the backoff, which doubles from 1 s to
/// 60 s and starts again from 1 s after a 4 that starts at once and after a child that lived
/// 60 s.
pub fn next(ending: Ending, lived: Duration, before: Restarts) -> (Next, Restarts) {
    let fives = match ending {
        Ending::Status(exit::SOURCE_NOT_STOPPED) => before.fives + 1,
        _ => 0,
    };
    match ending {
        Ending::Status(exit::CLEAN) => (Next::Attach, Restarts::default()),
        Ending::Status(exit::RESTART) if lived >= BACKOFF_MAX => {
            (Next::StartNow, Restarts::default())
        }
        _ if fives >= FIVES_IN_A_ROW => (
            Next::NoMoreChild,
            Restarts {
                waited: None,
                fives,
            },
        ),
        _ => {
            let waited = match before.waited {
                Some(last) if lived < BACKOFF_MAX => (last * 2).min(BACKOFF_MAX),
                _ => BACKOFF_FIRST,
            };
            (
                Next::StartAfter(waited),
                Restarts {
                    waited: Some(waited),
                    fives,
                },
            )
        }
    }
}

/// A started child as the supervisor watches it.
pub trait Watched: Send + 'static {
    fn pid(&self) -> u32;
    fn ended(&mut self) -> io::Result<Option<Ending>>;
    fn close_lifeline(&mut self);
    fn kill(&mut self) -> io::Result<()>;
}

/// What starts the app's child: `Ok(None)` when no child may start, because the data
/// directory is not provisioned.
pub trait Spawner: Send + 'static {
    type Child: Watched;
    fn start(&mut self) -> io::Result<Option<Self::Child>>;
}

/// The time of the supervisor: the system's in the app, a stand-in in the tests.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
    fn sleep(&self, period: Duration);
    fn wait<'a, T>(
        &self,
        changed: &Condvar,
        held: MutexGuard<'a, T>,
        period: Duration,
    ) -> MutexGuard<'a, T>;
}

/// The system's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, period: Duration) {
        std::thread::sleep(period);
    }

    fn wait<'a, T>(
        &self,
        changed: &Condvar,
        held: MutexGuard<'a, T>,
        period: Duration,
    ) -> MutexGuard<'a, T> {
        changed
            .wait_timeout(held, period)
            .unwrap_or_else(PoisonError::into_inner)
            .0
    }
}

/// A child that the app started: the process and the write end of the pipe on its standard
/// input, which the app alone holds while the child lives.
#[derive(Debug)]
pub struct Running {
    child: Child,
    lifeline: Option<ChildStdin>,
}

impl Watched for Running {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn ended(&mut self) -> io::Result<Option<Ending>> {
        Ok(self
            .child
            .try_wait()?
            .map(|status| status.code().map_or(Ending::Signal, Ending::Status)))
    }

    fn close_lifeline(&mut self) {
        self.lifeline = None;
    }

    fn kill(&mut self) -> io::Result<()> {
        self.child.kill()?;
        self.child.wait().map(|_| ())
    }
}

/// The one start of a child of the app: `program` with `args`; standard input is the read end
/// of a pipe whose write end the returned value holds, standard output is the null device,
/// standard error is `err`, and every other descriptor is closed in the child at its exec,
/// also one that lacks close-on-exec.
pub fn start_child(program: &Program, args: &[OsString], err: File) -> io::Result<Running> {
    let mut command = Command::new(&program.path);
    command
        .args(&program.leading)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(err);
    only_the_standard_streams(&mut command);
    let mut child = command.spawn()?;
    let lifeline = child.stdin.take();
    Ok(Running { child, lifeline })
}

#[cfg(unix)]
fn only_the_standard_streams(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: getdtablesize reads the descriptor limit of this process; it takes no pointer.
    let highest = unsafe { sys::getdtablesize() };
    // SAFETY: the closure runs in the forked child before its exec and calls fcntl alone, which
    // is async-signal-safe; it touches no memory of the parent.
    unsafe {
        command.pre_exec(move || {
            for descriptor in 3..highest {
                sys::fcntl(descriptor, sys::F_SETFD, sys::FD_CLOEXEC);
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn only_the_standard_streams(command: &mut Command) {
    let _ = command;
}

#[cfg(unix)]
mod sys {
    use std::os::raw::c_int;

    #[cfg(test)]
    pub const F_GETFD: c_int = 1;
    pub const F_SETFD: c_int = 2;
    pub const FD_CLOEXEC: c_int = 1;

    extern "C" {
        pub fn fcntl(descriptor: c_int, command: c_int, ...) -> c_int;
        pub fn getdtablesize() -> c_int;
    }
}

/// The start of the app's daemon on its data directory.
#[derive(Debug, Clone)]
pub struct Plan {
    pub program: Program,
    pub data_dir: DataDir,
    pub given: bool,
}

impl Spawner for Plan {
    type Child = Running;

    /// Starts the daemon when `is_provisioned` answers true; on false or on its error it starts
    /// nothing and makes no entry in the directory.
    fn start(&mut self) -> io::Result<Option<Running>> {
        if !matches!(self.data_dir.is_provisioned(), Ok(true)) {
            return Ok(None);
        }
        let err = daemon_err(&self.data_dir)?;
        let given = self.given.then(|| self.data_dir.path());
        start_child(&self.program, &arguments(given), err).map(Some)
    }
}

fn daemon_err(dir: &DataDir) -> io::Result<File> {
    match private_dir(&dir.logs_dir()) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let mut file = std::fs::OpenOptions::new();
    file.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
    file.open(dir.daemon_err())
}

#[cfg(unix)]
fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

#[derive(Debug)]
struct State<C> {
    quitting: bool,
    child: Option<C>,
}

#[derive(Debug)]
struct Shared<C> {
    state: Mutex<State<C>>,
    changed: Condvar,
}

impl<C> Shared<C> {
    fn lock(&self) -> MutexGuard<'_, State<C>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The app's hold on its daemon child: a thread of its own starts the child and watches it.
pub struct Supervisor<C, K> {
    shared: Arc<Shared<C>>,
    clock: Arc<K>,
}

impl<C: Watched, K: Clock> Supervisor<C, K> {
    /// `start_holding` with a hold of its own.
    #[cfg(test)]
    pub fn start<S: Spawner<Child = C>>(spawner: S, pid: Arc<ChildPid>, clock: Arc<K>) -> Self {
        Self::start_holding(spawner, pid, clock, Arc::default())
    }

    /// Starts the supervisor's thread, which starts the child off the main thread, gives `pid`
    /// the pid of the child of the moment and tells `hold` each start and end of a child.
    pub fn start_holding<S: Spawner<Child = C>>(
        spawner: S,
        pid: Arc<ChildPid>,
        clock: Arc<K>,
        hold: Arc<Hold>,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                quitting: false,
                child: None,
            }),
            changed: Condvar::new(),
        });
        let (watched, time) = (Arc::clone(&shared), Arc::clone(&clock));
        let started = std::thread::Builder::new()
            .name("halvo-daemon-child".to_owned())
            .spawn(move || {
                supervise_and_start_again(spawner, &watched, &pid, time.as_ref(), &hold)
            });
        if let Err(e) = started {
            eprintln!("halvo: the capture service is not started: {e}");
        }
        Supervisor { shared, clock }
    }

    /// Ends the supervision: no child starts after it, and the child of the moment, if any, is
    /// handed back with its descriptor closed.
    pub fn close(&self) -> Option<C> {
        let mut state = self.shared.lock();
        state.quitting = true;
        let child = state.child.take();
        drop(state);
        self.shared.changed.notify_all();
        child.map(|mut child| {
            child.close_lifeline();
            child
        })
    }
}

impl<C: Watched, K: Clock> crate::quit::Children for Supervisor<C, K> {
    fn lives(&self) -> bool {
        self.shared.lock().child.is_some()
    }

    fn ended(&self) -> bool {
        self.shared.lock().quitting
    }

    fn end(&self) {
        if let Some(mut child) = self.close() {
            end_child(&mut child, self.clock.as_ref(), STOP_WAIT);
        }
    }
}

fn end_child<C: Watched, K: Clock>(child: &mut C, clock: &K, wait: Duration) {
    let until = clock.now() + wait;
    loop {
        if let Ok(Some(_)) = child.ended() {
            return;
        }
        if clock.now() >= until {
            if let Err(e) = child.kill() {
                eprintln!("halvo: the capture service could not be ended: {e}");
            }
            return;
        }
        clock.sleep(STOP_POLL);
    }
}

/// The supervisor's thread: after the third end with 5 in a row it starts no further child
/// until a person's press on Start asks for one, and then supervises again with the count of
/// 5s at zero; a quit ends the wait.
fn supervise_and_start_again<S: Spawner, K: Clock>(
    spawner: S,
    shared: &Shared<S::Child>,
    pid: &ChildPid,
    clock: &K,
    hold: &Hold,
) {
    let mut spawner = spawner;
    while let Some(back) = supervise(spawner, shared, pid, clock, hold) {
        if !asked_to_start_before_a_quit(shared, clock, hold) {
            return;
        }
        spawner = back;
    }
}

fn asked_to_start_before_a_quit<C, K: Clock>(shared: &Shared<C>, clock: &K, hold: &Hold) -> bool {
    let mut state = shared.lock();
    loop {
        if state.quitting {
            return false;
        }
        if hold.take_start() {
            return true;
        }
        state = clock.wait(&shared.changed, state, CHILD_POLL);
    }
}

/// Starts the child and starts it again by the rule of `next`; hands the spawner back after
/// the third end with 5 in a row, and none otherwise.
fn supervise<S: Spawner, K: Clock>(
    mut spawner: S,
    shared: &Shared<S::Child>,
    pid: &ChildPid,
    clock: &K,
    hold: &Hold,
) -> Option<S> {
    let mut restarts = Restarts::default();
    loop {
        let began = clock.now();
        let ending = match spawner.start() {
            Ok(None) => return None,
            Ok(Some(child)) => {
                let mut state = shared.lock();
                if state.quitting {
                    return None;
                }
                pid.0.store(child.pid(), Ordering::SeqCst);
                state.child = Some(child);
                drop(state);
                hold.child_started();
                watch(shared, clock)?
            }
            Err(e) => {
                eprintln!("halvo: the capture service did not start: {e}");
                Ending::NotStarted
            }
        };
        pid.0.store(0, Ordering::SeqCst);
        hold.child_ended(match ending {
            Ending::Status(status) => Some(status),
            Ending::Signal | Ending::NotStarted => None,
        });
        eprintln!("halvo: the capture service ended: {ending:?}");
        let (step, after) = next(
            ending,
            clock.now().saturating_duration_since(began),
            restarts,
        );
        restarts = after;
        match step {
            Next::Attach => return None,
            Next::NoMoreChild => {
                hold.no_more_child();
                return Some(spawner);
            }
            Next::StartNow => {}
            Next::StartAfter(period) => {
                if waited_into_a_quit(shared, clock, period) {
                    return None;
                }
            }
        }
    }
}

fn waited_into_a_quit<C, K: Clock>(shared: &Shared<C>, clock: &K, period: Duration) -> bool {
    let until = clock.now() + period;
    let mut state = shared.lock();
    loop {
        if state.quitting {
            return true;
        }
        let left = until.saturating_duration_since(clock.now());
        if left.is_zero() {
            return false;
        }
        state = clock.wait(&shared.changed, state, left);
    }
}

fn watch<C: Watched, K: Clock>(shared: &Shared<C>, clock: &K) -> Option<Ending> {
    let mut state = shared.lock();
    loop {
        if state.quitting {
            return None;
        }
        let child = state.child.as_mut()?;
        if let Ok(Some(ending)) = child.ended() {
            state.child = None;
            return Some(ending);
        }
        state = clock.wait(&shared.changed, state, CHILD_POLL);
    }
}

#[cfg(test)]
pub mod stand_in {
    use std::collections::VecDeque;

    use super::*;

    /// A record of what the stand-ins and the test saw, in order.
    pub type Record = Arc<Mutex<Vec<String>>>;

    pub fn note(record: &Record, line: impl Into<String>) {
        record
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.into());
    }

    /// A clock whose time moves only by its waits, which it records.
    #[derive(Debug)]
    pub struct StandInClock {
        base: Instant,
        at: Mutex<Duration>,
        pub waits: Mutex<Vec<Duration>>,
    }

    impl Default for StandInClock {
        fn default() -> Self {
            StandInClock {
                base: Instant::now(),
                at: Mutex::new(Duration::ZERO),
                waits: Mutex::new(Vec::new()),
            }
        }
    }

    impl StandInClock {
        pub fn advance(&self, by: Duration) {
            *self.at.lock().unwrap_or_else(PoisonError::into_inner) += by;
        }

        pub fn waits(&self) -> Vec<Duration> {
            self.waits
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    impl Clock for StandInClock {
        fn now(&self) -> Instant {
            self.base + *self.at.lock().unwrap_or_else(PoisonError::into_inner)
        }

        fn sleep(&self, period: Duration) {
            self.advance(period);
        }

        fn wait<'a, T>(
            &self,
            changed: &Condvar,
            held: MutexGuard<'a, T>,
            period: Duration,
        ) -> MutexGuard<'a, T> {
            self.waits
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(period);
            self.advance(period);
            changed
                .wait_timeout(held, Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner)
                .0
        }
    }

    /// The ending of a child that the test gives while the child lives.
    pub type Later = Arc<Mutex<Option<Ending>>>;

    /// A child that ends as its row says, after the clock has moved by the time it lived, or,
    /// with no ending, once the test gives one in `later`, once its descriptor is closed when it
    /// ends at the close, and when it is killed.
    #[derive(Debug)]
    pub struct StandInChild {
        pub pid: u32,
        pub ending: Option<Ending>,
        pub later: Option<Later>,
        pub lived: Duration,
        pub ends_at_close: bool,
        pub clock: Arc<StandInClock>,
        pub record: Record,
        pub closed_at: Option<Instant>,
        pub killed: bool,
    }

    impl StandInChild {
        pub fn new(pid: u32, clock: &Arc<StandInClock>, record: &Record) -> Self {
            StandInChild {
                pid,
                ending: None,
                later: None,
                lived: Duration::ZERO,
                ends_at_close: false,
                clock: Arc::clone(clock),
                record: Arc::clone(record),
                closed_at: None,
                killed: false,
            }
        }
    }

    impl Watched for StandInChild {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn ended(&mut self) -> io::Result<Option<Ending>> {
            if self.killed {
                return Ok(Some(Ending::Signal));
            }
            if self.ends_at_close && self.closed_at.is_some() {
                return Ok(Some(Ending::Status(0)));
            }
            if let Some(later) = &self.later {
                if let Some(ending) = *later.lock().unwrap_or_else(PoisonError::into_inner) {
                    return Ok(Some(ending));
                }
            }
            if self.ending.is_some() {
                self.clock.advance(self.lived);
            }
            Ok(self.ending)
        }

        fn close_lifeline(&mut self) {
            if self.closed_at.is_none() {
                self.closed_at = Some(self.clock.now());
                note(&self.record, "closed descriptor");
            }
        }

        fn kill(&mut self) -> io::Result<()> {
            let after = self.closed_at.map_or(Duration::ZERO, |at| {
                self.clock.now().saturating_duration_since(at)
            });
            note(&self.record, format!("kill after {} s", after.as_secs()));
            self.killed = true;
            Ok(())
        }
    }

    /// A spawner that hands out its children in order and then answers that no child may
    /// start.
    pub struct StandInSpawner {
        pub children: VecDeque<StandInChild>,
        pub record: Record,
    }

    impl Spawner for StandInSpawner {
        type Child = StandInChild;

        fn start(&mut self) -> io::Result<Option<StandInChild>> {
            let child = self.children.pop_front();
            if let Some(child) = &child {
                note(&self.record, format!("start {}", child.pid));
            }
            Ok(child)
        }
    }
}

#[cfg(test)]
mod tests {
    use halvo_control::client::{ClientError, DeleteOutcome};
    use halvo_control::model::{StorageCap, StoreId};
    use halvo_control::onboarding::{Permission, RequestOutcome};
    use halvo_control::proto::{
        CapSet, Capture, CaptureChange, DaemonStatus, InventoryReply, Lease, Platform, SourceName,
        SourceState,
    };
    use halvo_control::testing::sample_status;

    use super::*;
    use crate::commands::capture::{capture_set, press, START_ANSWER_WAIT};
    use crate::commands::control::{Control, Made, Places};
    #[cfg(unix)]
    use crate::launch::{parse, Launch};
    use crate::poller::{pass, Views};
    use crate::tests::{fixtures_dir, read_json};
    use crate::view_state::{ActionError, Press};
    use stand_in::{note, Later, Record, StandInChild, StandInClock, StandInSpawner};

    /// A control whose Status answers `status`, or no daemon where it holds none, and whose
    /// Resume answers the held child's status; it records each request.
    struct Recording {
        places: Arc<Places>,
        record: Record,
        status: Mutex<Option<DaemonStatus>>,
    }

    impl Recording {
        fn new(record: &Record, status: Option<DaemonStatus>) -> Self {
            Recording {
                places: Arc::default(),
                record: Arc::clone(record),
                status: Mutex::new(status),
            }
        }
    }

    impl Control for Recording {
        fn places(&self) -> &Arc<Places> {
            &self.places
        }

        fn status(&self, _: Made, claim: bool) -> Result<DaemonStatus, ClientError> {
            note(&self.record, if claim { "status, claim" } else { "status" });
            self.status
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| ClientError::Unreachable {
                    endpoint: String::new(),
                })
        }

        fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
            unreachable!("no inventory here")
        }

        fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            unreachable!("no Pause here")
        }

        fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            note(&self.record, "resume");
            Ok(held_child_status())
        }

        fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
            unreachable!("no delete here")
        }

        fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
            unreachable!("no cap here")
        }

        fn request_permission(
            &self,
            _: Made,
            _: Permission,
        ) -> Result<RequestOutcome, ClientError> {
            unreachable!("no permission here")
        }
    }

    /// The status of a new life that no claim has reached: desired running, no indicator.
    fn held_child_status() -> DaemonStatus {
        let mut f = sample_status(Platform::Macos).into_fields();
        f.capture = Capture::NoIndicator;
        f.lease = Lease::None;
        for line in f.sources.iter_mut() {
            if matches!(line.name, SourceName::Foreground | SourceName::Input) {
                line.state = SourceState::Stopped { by_user: false };
            }
        }
        DaemonStatus::new(f).expect("the status holds together")
    }

    fn until(what: &str, done: impl Fn() -> bool) {
        let by = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < by, "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn after_an_end_with_5_the_next_child_gets_no_claim_and_no_resume_before_the_press() {
        let record = Record::default();
        let clock = Arc::new(StandInClock::default());
        let second = Later::default();
        let child = |pid, ending, later| StandInChild {
            ending,
            later,
            ends_at_close: true,
            ..StandInChild::new(pid, &clock, &record)
        };
        let spawner = StandInSpawner {
            children: [
                child(1000, Some(Ending::Status(5)), None),
                child(1001, None, Some(Arc::clone(&second))),
                child(1002, None, None),
            ]
            .into_iter()
            .collect(),
            record: Arc::clone(&record),
        };
        let views = Views::default();
        let pid = Arc::<ChildPid>::default();
        let supervisor = Supervisor::start_holding(
            spawner,
            Arc::clone(&pid),
            Arc::clone(&clock),
            Arc::clone(views.hold()),
        );
        let control = Recording::new(&record, Some(held_child_status()));
        let tray = crate::tray::installed_for_tests();

        until("the child after the 5 starts", || pid.get() == Some(1001));
        pass(&control, &views, Some(&tray));
        pass(&control, &views, Some(&tray));
        assert_eq!(
            serde_json::to_value(views.view()).unwrap(),
            read_json(&fixtures_dir().join("held-child.json")),
            "the held child's view names the cause and Resume"
        );
        *second.lock().unwrap() = Some(Ending::Status(1));
        until("the child after the 1 starts", || pid.get() == Some(1002));
        pass(&control, &views, Some(&tray));

        let resume = CaptureChange::Resume;
        let within = START_ANSWER_WAIT;
        let (answer, call) = press(&control, &views, control.places().make(), resume, within);
        let next = control.places().next_number();
        let set = capture_set(&views, call, next, CaptureChange::Resume, answer)
            .expect("the press's Resume answers a status");
        assert!(set.reached);
        assert_eq!(set.view.hold(), None, "the press ends the hold");
        pass(&control, &views, Some(&tray));
        let seen = record.lock().unwrap().clone();
        drop(supervisor.close());
        assert_eq!(
            seen,
            [
                "start 1000",
                "start 1001",
                "status",
                "status",
                "start 1002",
                "status",
                "resume",
                "status, claim",
            ]
        );
    }

    #[cfg(unix)]
    const STUB: &str = "daemon_child::tests::stub_child";
    type Row = (&'static str, Vec<(Ending, u64)>, usize, Vec<Duration>);
    #[cfg(unix)]
    const WITHIN: Duration = Duration::from_secs(60);

    #[cfg(unix)]
    fn stub_program() -> Program {
        Program {
            path: std::env::current_exe().expect("the test binary has a path"),
            leading: ["--exact", "--test-threads=1", "--nocapture", "--", STUB]
                .into_iter()
                .map(OsString::from)
                .collect(),
        }
    }

    #[cfg(unix)]
    fn open_descriptors() -> Vec<i32> {
        // SAFETY: getdtablesize reads the descriptor limit of this process; it takes no pointer.
        let highest = unsafe { sys::getdtablesize() };
        // SAFETY: F_GETFD reads the flags of a descriptor number; it takes no pointer.
        (3..highest)
            .filter(|descriptor| unsafe { sys::fcntl(*descriptor, sys::F_GETFD) } >= 0)
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn stub_child() {
        use std::io::Read;
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        let args: Vec<String> = std::env::args().collect();
        if !args.iter().any(|arg| arg == LIFELINE_ARG) {
            return;
        }
        let open = open_descriptors();
        let after = args
            .iter()
            .position(|arg| arg == STUB)
            .map_or(&[][..], |at| &args[at + 1..]);
        let fifo = std::fs::metadata("/dev/fd/0").is_ok_and(|m| m.file_type().is_fifo());
        let null = std::fs::metadata("/dev/fd/1")
            .is_ok_and(|m| std::fs::metadata("/dev/null").is_ok_and(|n| n.rdev() == m.rdev()));
        eprintln!("pid {}", std::process::id());
        eprintln!("args {}", after.join(" "));
        eprintln!("descriptors {open:?}");
        eprintln!("stdin fifo {fifo}");
        eprintln!("stdout null {null}");
        let _ = std::io::stdin().read_to_end(&mut Vec::new());
        eprintln!("eof");
        std::process::exit(0);
    }

    #[cfg(unix)]
    fn lines_within(path: &Path, done: impl Fn(&str) -> bool) -> String {
        let until = Instant::now() + WITHIN;
        loop {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if done(&text) || Instant::now() > until {
                return text;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_app_starts_one_child_on_a_provisioned_directory_and_none_before_it() {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::io::AsRawFd;
        let root = crate::tests::scratch_dir("daemon-child");
        let empty = root.join("empty");
        let provisioned = root.join("provisioned");
        for dir in [&empty, &provisioned] {
            std::fs::create_dir(dir).unwrap();
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let started = |dir: &Path| {
            parse(
                ["--background", "--data-dir", dir.to_str().unwrap()]
                    .into_iter()
                    .map(OsString::from),
            )
        };
        assert_eq!(
            started(&provisioned),
            Ok(Launch::Run {
                background: true,
                data_dir: Some(provisioned.clone())
            })
        );
        assert_eq!(
            parse([OsString::from("--data-dir"), OsString::from("relative")]),
            Err(crate::launch::Usage::RelativeDir)
        );

        let mut on_empty = Plan {
            program: stub_program(),
            data_dir: DataDir::from_flag(&empty).unwrap(),
            given: true,
        };
        assert!(on_empty.start().unwrap().is_none());
        assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);

        let data_dir = DataDir::from_flag(&provisioned).unwrap();
        std::fs::write(data_dir.state_file(), b"").unwrap();
        let held = File::create(root.join("held")).unwrap();
        // SAFETY: F_SETFD sets the flags of a descriptor that `held` owns; it takes no pointer.
        unsafe { sys::fcntl(held.as_raw_fd(), sys::F_SETFD, 0) };
        // SAFETY: F_GETFD reads the flags of a descriptor that `held` owns; it takes no pointer.
        let flags = unsafe { sys::fcntl(held.as_raw_fd(), sys::F_GETFD) };
        assert_eq!(flags, 0, "the test's descriptor lacks close-on-exec");

        let pid = Arc::<ChildPid>::default();
        let plan = Plan {
            program: stub_program(),
            data_dir: data_dir.clone(),
            given: true,
        };
        let supervisor = Supervisor::start(plan, Arc::clone(&pid), Arc::new(SystemClock));
        let err = data_dir.daemon_err();
        let report = lines_within(&err, |text| text.contains("stdout null"));
        let child = pid.get().expect("the client expects the child's pid");
        let expected = format!(
            "pid {child}\nargs --service-run --lifeline --data-dir {}\ndescriptors []\nstdin fifo true\nstdout null true\n",
            provisioned.display()
        );
        assert_eq!(report, expected);
        let mode = std::fs::metadata(&err).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let mut running = supervisor.close().expect("the child of the moment");
        let until = Instant::now() + WITHIN;
        let ending = loop {
            match running.ended().unwrap() {
                Some(ending) => break ending,
                None if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
                None => panic!("the stub did not end after its descriptor closed"),
            }
        };
        assert_eq!(ending, Ending::Status(0));
        assert_eq!(
            std::fs::read_to_string(&err).unwrap(),
            format!("{expected}eof\n")
        );
        drop(held);
    }

    #[test]
    fn after_the_third_5_the_view_names_the_cause_and_start_brings_one_child_back() {
        assert_eq!(START_ANSWER_WAIT, Duration::from_secs(10));
        for answers in [true, false] {
            let record = Record::default();
            let clock = Arc::new(StandInClock::default());
            let fourth = Later::default();
            let child = |pid, ending, later| StandInChild {
                ending,
                later,
                ends_at_close: true,
                ..StandInChild::new(pid, &clock, &record)
            };
            let five = Some(Ending::Status(5));
            let spawner = StandInSpawner {
                children: [
                    child(1000, five, None),
                    child(1001, five, None),
                    child(1002, five, None),
                    child(1003, None, Some(Arc::clone(&fourth))),
                    child(1004, None, None),
                ]
                .into_iter()
                .collect(),
                record: Arc::clone(&record),
            };
            let views = Views::default();
            let pid = Arc::<ChildPid>::default();
            let supervisor = Supervisor::start_holding(
                spawner,
                Arc::clone(&pid),
                Arc::clone(&clock),
                Arc::clone(views.hold()),
            );
            let tray = crate::tray::installed_for_tests();

            until("the hold reads Start after the third 5", || {
                views
                    .hold()
                    .view()
                    .is_some_and(|hold| hold.press == Press::Start)
            });
            pass(&Recording::new(&record, None), &views, Some(&tray));
            assert_eq!(
                *record.lock().unwrap(),
                ["start 1000", "start 1001", "start 1002", "status"],
                "no fourth child after the third 5"
            );
            assert_eq!(
                serde_json::to_value(views.view()).unwrap(),
                read_json(&fixtures_dir().join("held-no-child.json")),
                "not_running with last_exit 5, the cause and Start"
            );
            let labels = crate::tray::labels(&views.view());
            assert_eq!(
                (
                    labels.status_text,
                    labels.toggle_text,
                    labels.toggle_enabled
                ),
                (crate::tray::HELD, "Start capture", true)
            );

            let control = Recording::new(&record, answers.then(held_child_status));
            let within = if answers {
                START_ANSWER_WAIT
            } else {
                Duration::from_secs(1)
            };
            let resume = CaptureChange::Resume;
            let (answer, call) = press(&control, &views, control.places().make(), resume, within);
            let next = control.places().next_number();
            let set = capture_set(&views, call, next, resume, answer);
            if answers {
                assert!(set.expect("the Start's Resume answers a status").reached);
                assert_eq!(
                    views.hold().view(),
                    None,
                    "the Start's Resume ends the hold"
                );
                *fourth.lock().unwrap() = five;
                until("a 5 after the Start counts as the first", || {
                    pid.get() == Some(1004)
                });
            } else {
                assert!(matches!(set, Err(ActionError::Deadline { .. })));
                assert!(views.hold().stands(), "with no answer the hold stands");
            }
            let seen = record.lock().unwrap().clone();
            drop(supervisor.close());
            let after: Vec<&str> = seen[4..].iter().map(String::as_str).collect();
            if answers {
                assert_eq!(after, ["start 1003", "status", "resume", "start 1004"]);
            } else {
                assert_eq!(after[0], "start 1003", "one child by the Start");
                assert!(after.len() > 1, "the Start asked its child for a Status");
                assert!(after[1..].iter().all(|line| *line == "status"), "{after:?}");
            }
        }
    }

    #[test]
    fn the_child_is_started_again_by_its_exit_status() {
        let s = Duration::from_secs;
        let status = Ending::Status;
        let rows: Vec<Row> = vec![
            (
                "0: no start again",
                vec![(status(0), 0), (status(1), 0)],
                1,
                vec![],
            ),
            (
                "1: the backoff from 1 s to 60 s",
                vec![(status(1), 0); 8],
                8,
                vec![s(1), s(2), s(4), s(8), s(16), s(32), s(60), s(60)],
            ),
            (
                "4 within 60 s of its start: the backoff of 1, not four starts a second",
                vec![(status(4), 0); 3],
                3,
                vec![s(1), s(2), s(4)],
            ),
            (
                "5: the backoff, and no child after the third 5 in a row",
                vec![(status(5), 0); 4],
                3,
                vec![s(1), s(2)],
            ),
            (
                "5: a 1 between them ends the row",
                vec![
                    (status(5), 0),
                    (status(5), 0),
                    (status(1), 0),
                    (status(5), 0),
                    (status(5), 0),
                    (status(5), 0),
                    (status(1), 0),
                ],
                6,
                vec![s(1), s(2), s(4), s(8), s(16)],
            ),
            (
                "a status that the list does not name and a signal: as 1",
                vec![(status(3), 0), (Ending::Signal, 0)],
                2,
                vec![s(1), s(2)],
            ),
            (
                "4 after 60 s starts at once, and it and a child that lived 60 s start the backoff again",
                vec![
                    (status(1), 0),
                    (status(1), 0),
                    (status(4), 60),
                    (status(1), 0),
                    (status(1), 0),
                    (status(1), 60),
                    (status(1), 0),
                ],
                7,
                vec![s(1), s(2), s(1), s(2), s(1), s(2)],
            ),
        ];
        for (name, endings, starts, waits) in rows {
            let record = Record::default();
            let clock = Arc::new(StandInClock::default());
            let children = endings
                .into_iter()
                .enumerate()
                .map(|(n, (ending, lived))| StandInChild {
                    ending: Some(ending),
                    lived: s(lived),
                    ..StandInChild::new(1000 + n as u32, &clock, &record)
                })
                .collect();
            let spawner = StandInSpawner {
                children,
                record: Arc::clone(&record),
            };
            let shared = Shared {
                state: Mutex::new(State {
                    quitting: false,
                    child: None,
                }),
                changed: Condvar::new(),
            };
            let pid = ChildPid::default();
            supervise(spawner, &shared, &pid, clock.as_ref(), &Hold::default());
            let started = record
                .lock()
                .unwrap()
                .iter()
                .filter(|line| line.starts_with("start"))
                .count();
            assert_eq!(started, starts, "{name}: children started");
            assert_eq!(
                clock.waits(),
                waits,
                "{name}: the waits before a start again"
            );
            assert_eq!(pid.get(), None, "{name}: no pid once no child lives");
        }
    }
}
