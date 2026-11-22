use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use halvo_control::exit;
use halvo_control::paths::DataDir;
use halvo_control::service::{DAEMON_ARG, DATA_DIR_ARG};

use crate::commands::control::{ChildPid, Sight};
use crate::poller::Views;
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
    /// No start again while a daemon answers or holds the directory: another daemon holds it,
    /// and the app talks to it with no expected pid and no descriptor; once no daemon answers,
    /// a start again from a fresh count.
    Attach,
    StartNow,
    StartAfter(Duration),
    /// No further child in this life of the app.
    NoMoreChild,
}

/// Why the app ends its own child (contracts.md C14 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The child read another version: one child starts again at once, since the app asked for
    /// the stop in order to restart, unless the child ended with 5.
    Restart,
    /// A reply or a Status of the child read capture=pausing with a source that did not stop,
    /// also one that came while a stop of another kind waited for the child's end: the end
    /// counts as an end with 5, whether the child ended or was killed, and what starts next is
    /// the rule of `next`.
    SourceNotStopped,
    /// A request of the app to the child passed its deadline with no reply: what starts next is
    /// the rule of `next`, and a kill sets no hold.
    NoReply,
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
    match crate::instance::private_dir(&dir.logs_dir()) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let mut file = std::fs::OpenOptions::new();
    file.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o600);
    file.open(dir.daemon_err())
}

#[derive(Debug)]
struct State<C> {
    quitting: bool,
    child: Option<C>,
    version_restarted: bool,
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
    /// `start_holding` with views of its own.
    #[cfg(test)]
    pub fn start<S: Spawner<Child = C>>(spawner: S, pid: Arc<ChildPid>, clock: Arc<K>) -> Self {
        Self::start_holding(spawner, pid, clock, Arc::default())
    }

    /// Starts the supervisor's thread, which starts the child off the main thread, gives `pid`
    /// the pid of the child of the moment, tells the hold of `views` each start and end of a
    /// child and draws the views after each end.
    pub fn start_holding<S: Spawner<Child = C>>(
        spawner: S,
        pid: Arc<ChildPid>,
        clock: Arc<K>,
        views: Arc<Views>,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                quitting: false,
                child: None,
                version_restarted: false,
            }),
            changed: Condvar::new(),
        });
        let (watched, time) = (Arc::clone(&shared), Arc::clone(&clock));
        let started = std::thread::Builder::new()
            .name("halvo-daemon-child".to_owned())
            .spawn(move || {
                supervise_and_start_again(spawner, &watched, &pid, time.as_ref(), &views)
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
    views: &Views,
) {
    let mut spawner = spawner;
    while let Some(back) = supervise(spawner, shared, pid, clock, views) {
        if !asked_to_start_before_a_quit(shared, clock, views.hold()) {
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

/// Starts the child and starts it again by the rule of `next`, after a 0 once the other daemon
/// is gone; hands the spawner back after the third end with 5 in a row, and none otherwise.
/// After each end it draws the views, so that the set of the hold reaches the tray's menu at
/// once.
fn supervise<S: Spawner, K: Clock>(
    mut spawner: S,
    shared: &Shared<S::Child>,
    pid: &ChildPid,
    clock: &K,
    views: &Views,
) -> Option<S> {
    let hold = views.hold();
    let mut restarts = Restarts::default();
    loop {
        let began = clock.now();
        let by_the_rule = |ending: Ending| {
            let (step, after) = next(
                ending,
                clock.now().saturating_duration_since(began),
                restarts,
            );
            (ending, step, after)
        };
        let (ending, step, after) = match spawner.start() {
            Ok(None) => return None,
            Ok(Some(child)) => {
                let mut state = shared.lock();
                if state.quitting {
                    return None;
                }
                pid.set(child.pid());
                state.child = Some(child);
                drop(state);
                hold.child_started();
                match watch(shared, pid, clock, views)? {
                    Seen::Ended(ending) => by_the_rule(ending),
                    Seen::Stop(stop) => stop_and_start(shared, pid, clock, stop, began, restarts)?,
                }
            }
            Err(e) => {
                eprintln!("halvo: the capture service did not start: {e}");
                by_the_rule(Ending::NotStarted)
            }
        };
        pid.set(0);
        hold.child_ended(match ending {
            Ending::Status(status) => Some(status),
            Ending::Signal | Ending::NotStarted => None,
        });
        views.draw_now();
        eprintln!("halvo: the capture service ended: {ending:?}");
        restarts = after;
        match step {
            Next::Attach => {
                if waited_for_no_daemon(shared, pid, clock) {
                    return None;
                }
            }
            Next::NoMoreChild => {
                hold.no_more_child();
                views.draw_now();
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

fn waited_for_no_daemon<C, K: Clock>(shared: &Shared<C>, pid: &ChildPid, clock: &K) -> bool {
    let after = pid.answers();
    if waited_into_a_quit(shared, clock, BACKOFF_FIRST) {
        return true;
    }
    let mut state = shared.lock();
    loop {
        if state.quitting {
            return true;
        }
        if pid.no_daemon_after(after) {
            return false;
        }
        state = clock.wait(&shared.changed, state, CHILD_POLL);
    }
}

enum Seen {
    Ended(Ending),
    Stop(Stop),
}

fn watch<C: Watched, K: Clock>(
    shared: &Shared<C>,
    pid: &ChildPid,
    clock: &K,
    views: &Views,
) -> Option<Seen> {
    let mut state = shared.lock();
    loop {
        if state.quitting {
            return None;
        }
        let child = state.child.as_mut()?;
        if let Ok(Some(ending)) = child.ended() {
            state.child = None;
            return Some(Seen::Ended(ending));
        }
        match pid.take_sight(child.pid()) {
            Some(Sight::SourceNotStopped) => return Some(Seen::Stop(Stop::SourceNotStopped)),
            Some(Sight::NoReply) => return Some(Seen::Stop(Stop::NoReply)),
            Some(Sight::AnotherVersion) if !state.version_restarted => {
                state.version_restarted = true;
                return Some(Seen::Stop(Stop::Restart));
            }
            Some(Sight::AnotherVersion) => {
                drop(state);
                views.hold().another_version_left();
                views.draw_now();
                state = shared.lock();
                continue;
            }
            None => {}
        }
        state = clock.wait(&shared.changed, state, CHILD_POLL);
    }
}

fn stop_and_start<C: Watched, K: Clock>(
    shared: &Shared<C>,
    pid: &ChildPid,
    clock: &K,
    stop: Stop,
    began: Instant,
    before: Restarts,
) -> Option<(Ending, Next, Restarts)> {
    let mut stop = stop;
    let until = clock.now() + STOP_WAIT;
    let mut state = shared.lock();
    state.child.as_mut()?.close_lifeline();
    let ended = loop {
        if state.quitting {
            return None;
        }
        let child = state.child.as_mut()?;
        if let Ok(Some(ending)) = child.ended() {
            break ending;
        }
        if pid.take_sight(child.pid()) == Some(Sight::SourceNotStopped) {
            stop = Stop::SourceNotStopped;
        }
        if clock.now() >= until {
            if let Err(e) = child.kill() {
                eprintln!("halvo: the capture service could not be ended: {e}");
            }
            break Ending::Signal;
        }
        state = clock.wait(&shared.changed, state, STOP_POLL);
    };
    state.child = None;
    drop(state);
    let ending = match stop {
        Stop::SourceNotStopped => Ending::Status(exit::SOURCE_NOT_STOPPED),
        Stop::Restart | Stop::NoReply => ended,
    };
    if stop == Stop::Restart && ending != Ending::Status(exit::SOURCE_NOT_STOPPED) {
        return Some((ending, Next::StartNow, Restarts::default()));
    }
    let (step, after) = next(ending, clock.now().saturating_duration_since(began), before);
    Some((ending, step, after))
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
            let real = if period <= STOP_POLL {
                Duration::ZERO
            } else {
                Duration::from_millis(5)
            };
            changed
                .wait_timeout(held, real)
                .unwrap_or_else(PoisonError::into_inner)
                .0
        }
    }

    /// The ending of a child that the test gives while the child lives.
    pub type Later = Arc<Mutex<Option<Ending>>>;

    /// A call that a stand-in child makes once, at its first look after its descriptor closed:
    /// a step of a test that lands inside the stop's wait whatever the real time.
    #[derive(Clone)]
    pub struct AtClose(pub Arc<dyn Fn() + Send + Sync>);

    impl std::fmt::Debug for AtClose {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("AtClose")
        }
    }

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
        pub at_close: Option<AtClose>,
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
                at_close: None,
            }
        }
    }

    impl Watched for StandInChild {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn ended(&mut self) -> io::Result<Option<Ending>> {
            if self.closed_at.is_some() {
                if let Some(AtClose(call)) = self.at_close.take() {
                    call();
                }
            }
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
    use stand_in::{note, AtClose, Later, Record, StandInChild, StandInClock, StandInSpawner};

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

        fn status_of_a_pass(
            &self,
            made: Made,
            claim: bool,
            hold: &Hold,
        ) -> Result<DaemonStatus, ClientError> {
            self.status(made, claim && !hold.stands())
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

        fn relay(&self, _: Made, _: &[u8]) -> Result<Vec<u8>, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn owner_stream(&self, _: Made) -> Result<halvo_control::r2::OwnerStreamEnd, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn run_mark(
            &self,
            _: Made,
            _: halvo_control::r2::RunEdge,
        ) -> Result<halvo_control::r2::RunMarked, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn defer(&self, _: Made, _: i64) -> Result<Option<i64>, ClientError> {
            Err(ClientError::Unsupported)
        }

        fn acknowledge_lost_permission(
            &self,
            _: Made,
            _: Permission,
        ) -> Result<DaemonStatus, ClientError> {
            Err(ClientError::Unsupported)
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
        let views = Arc::new(Views::default());
        let pid = Arc::<ChildPid>::default();
        let supervisor = Supervisor::start_holding(
            spawner,
            Arc::clone(&pid),
            Arc::clone(&clock),
            Arc::clone(&views),
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
    type Row4 = (
        &'static str,
        Vec<(u32, &'static str, Result<DaemonStatus, ClientError>)>,
        Vec<(u32, bool)>,
        Vec<&'static str>,
        (bool, bool, Option<crate::view_state::Service>),
    );
    type Row3 = (
        &'static str,
        Vec<Result<DaemonStatus, ClientError>>,
        Option<bool>,
        Vec<String>,
    );
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
            let views = Arc::new(Views::default());
            let pid = Arc::<ChildPid>::default();
            let supervisor = Supervisor::start_holding(
                spawner,
                Arc::clone(&pid),
                Arc::clone(&clock),
                Arc::clone(&views),
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
            let mut expected = read_json(&fixtures_dir().join("held-no-child.json"));
            expected["platform"] = serde_json::to_value(crate::view_state::APP_PLATFORM).unwrap();
            assert_eq!(
                serde_json::to_value(views.view()).unwrap(),
                expected,
                "not_running with last_exit 5, the cause and Start, on the app's own system"
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
    fn a_press_on_the_tray_sends_what_its_drawn_item_said_and_the_set_of_the_hold_draws_it() {
        use crate::poller::Drawer;
        use crate::tray::{labels_with, DrawnMenu, TogglePress, HELD};
        let drawn_into = |views: &Views, menu: &Arc<DrawnMenu>| {
            let menu = Arc::clone(menu);
            views.set_drawer(Drawer::new(
                Box::new(move |view| menu.drew(labels_with(view, None, false, None))),
                Vec::new(),
            ));
        };
        let press = |views: &Views, menu: &DrawnMenu| {
            let view = views.view();
            menu.press(&view, &labels_with(&view, None, false, None))
        };
        let record = Record::default();
        let tray = crate::tray::installed_for_tests();

        let views = Views::default();
        let menu = Arc::new(DrawnMenu::default());
        drawn_into(&views, &menu);
        let capturing = Recording::new(&record, Some(sample_status(Platform::Macos)));
        pass(&capturing, &views, Some(&tray));
        views.draw_now();
        assert_eq!(
            press(&views, &menu),
            TogglePress::Send(CaptureChange::Pause)
        );
        views.hold().child_ended(Some(5));
        assert_eq!(
            press(&views, &menu),
            TogglePress::DrawAnew,
            "an item drawn while capturing sends no Resume after the end with 5"
        );
        assert!(views.hold().stands(), "the hold stands");
        views.draw_now();
        assert_eq!(menu.last().map(|labels| labels.status_text), Some(HELD));
        assert_eq!(
            press(&views, &menu),
            TogglePress::Send(CaptureChange::Resume),
            "a press on the drawn line that names the cause"
        );

        let clock = Arc::new(StandInClock::default());
        let ends = Later::default();
        let spawner = StandInSpawner {
            children: [StandInChild {
                later: Some(Arc::clone(&ends)),
                ends_at_close: true,
                ..StandInChild::new(1000, &clock, &record)
            }]
            .into_iter()
            .collect(),
            record: Arc::clone(&record),
        };
        let views = Arc::new(Views::default());
        let menu = Arc::new(DrawnMenu::default());
        drawn_into(&views, &menu);
        let pid = Arc::<ChildPid>::default();
        let supervisor =
            Supervisor::start_holding(spawner, Arc::clone(&pid), clock, Arc::clone(&views));
        until("the child starts", || pid.get() == Some(1000));
        views.draw_now();
        assert_ne!(menu.last().map(|labels| labels.status_text), Some(HELD));
        *ends.lock().unwrap() = Some(Ending::Status(5));
        until("the set of the hold draws the menu", || {
            menu.last().map(|labels| labels.status_text) == Some(HELD)
        });
        drop(supervisor.close());
    }

    #[test]
    fn a_second_press_on_start_leaves_no_ask_for_the_next_third_5() {
        let five = Some(halvo_control::exit::SOURCE_NOT_STOPPED);
        let hold = Hold::default();
        hold.child_ended(five);
        hold.no_more_child();
        assert!(hold.ask_start(), "the first press asks for a child");
        assert!(hold.take_start(), "the supervisor takes that ask");
        assert!(
            !hold.ask_start(),
            "a second press that comes after the take asks for nothing"
        );
        hold.child_started();
        hold.child_ended(five);
        hold.no_more_child();
        assert!(
            !hold.take_start(),
            "at the next third 5 no child starts with nobody pressing"
        );
        assert_eq!(hold.view().map(|view| view.press), Some(Press::Start));
    }

    struct Scripted {
        places: Arc<Places>,
        record: Record,
        statuses: Mutex<std::collections::VecDeque<Result<DaemonStatus, ClientError>>>,
        pauses: Mutex<std::collections::VecDeque<Result<DaemonStatus, ClientError>>>,
    }

    impl Scripted {
        fn new(
            record: &Record,
            statuses: Vec<Result<DaemonStatus, ClientError>>,
            pauses: Vec<Result<DaemonStatus, ClientError>>,
        ) -> Self {
            Scripted {
                places: Arc::default(),
                record: Arc::clone(record),
                statuses: Mutex::new(statuses.into()),
                pauses: Mutex::new(pauses.into()),
            }
        }
    }

    impl Control for Scripted {
        fn places(&self) -> &Arc<Places> {
            &self.places
        }

        fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
            note(&self.record, "status");
            self.statuses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Err(ClientError::Unreachable {
                        endpoint: String::new(),
                    })
                })
        }

        fn status_of_a_pass(
            &self,
            made: Made,
            claim: bool,
            hold: &Hold,
        ) -> Result<DaemonStatus, ClientError> {
            self.status(made, claim && !hold.stands())
        }

        fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
            unreachable!("no inventory here")
        }

        fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            note(&self.record, "pause");
            let next = self.pauses.lock().unwrap().pop_front();
            next.expect("one Pause for each press")
        }

        fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            unreachable!("no Resume here")
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

        fn relay(&self, _: Made, _: &[u8]) -> Result<Vec<u8>, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn owner_stream(&self, _: Made) -> Result<halvo_control::r2::OwnerStreamEnd, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn run_mark(
            &self,
            _: Made,
            _: halvo_control::r2::RunEdge,
        ) -> Result<halvo_control::r2::RunMarked, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn defer(&self, _: Made, _: i64) -> Result<Option<i64>, ClientError> {
            Err(ClientError::Unsupported)
        }

        fn acknowledge_lost_permission(
            &self,
            _: Made,
            _: Permission,
        ) -> Result<DaemonStatus, ClientError> {
            Err(ClientError::Unsupported)
        }
    }

    fn looked_again(clock: &StandInClock) {
        let seen = clock.waits().len();
        until("the supervisor looks again", || {
            clock.waits().len() > seen + 2
        });
    }

    #[test]
    fn after_a_0_the_child_starts_again_once_the_other_daemon_is_gone_and_a_second_quit_exits() {
        use crate::commands::control::Watching;
        use crate::quit::{
            quit_and_pause, Presses, Quit, CAPTURE_GOES_ON, CAPTURE_MAY_GO_ON, NOT_ANSWERED,
            NOT_PAUSED,
        };
        use halvo_control::client::PeerRejection;
        use halvo_control::proto::{ErrorCode, ErrorReply};

        let record = Record::default();
        let clock = Arc::new(StandInClock::default());
        let spawner = StandInSpawner {
            children: [
                StandInChild {
                    ending: Some(Ending::Status(0)),
                    ..StandInChild::new(1000, &clock, &record)
                },
                StandInChild {
                    ends_at_close: true,
                    ..StandInChild::new(1001, &clock, &record)
                },
            ]
            .into_iter()
            .collect(),
            record: Arc::clone(&record),
        };
        let views = Arc::new(Views::default());
        let pid = Arc::<ChildPid>::default();
        let supervisor = Supervisor::start_holding(
            spawner,
            Arc::clone(&pid),
            Arc::clone(&clock),
            Arc::clone(&views),
        );
        let other = sample_status(Platform::Macos);
        let in_use = ClientError::DataDirInUse {
            endpoint: String::new(),
        };
        let control = Watching::new(
            Scripted::new(&record, vec![Ok(other), Err(in_use)], Vec::new()),
            Arc::clone(&pid),
        );
        let tray = crate::tray::installed_for_tests();
        until("the supervisor waits after the 0", || {
            clock.waits().len() > 2
        });
        assert_eq!(
            clock.waits()[0],
            BACKOFF_FIRST,
            "never faster than the backoff of status 1"
        );
        for what in ["answers", "holds the directory"] {
            pass(&control, &views, Some(&tray));
            looked_again(&clock);
            assert_eq!(pid.get(), None, "no child while the other daemon {what}");
        }
        pass(&control, &views, Some(&tray));
        until("a child once no daemon answers", || pid.get() == Some(1001));
        drop(supervisor.close());
        assert_eq!(
            *record.lock().unwrap(),
            [
                "start 1000",
                "status",
                "status",
                "status",
                "start 1001",
                "closed descriptor"
            ]
        );

        let lease_s = halvo_control::INDICATOR_LEASE_MS.div_ceil(1000);
        assert_eq!(
            *CAPTURE_GOES_ON,
            format!(
                "Halvo did not quit, because it could not confirm that capture has stopped. Choose Quit again to quit anyway: capture then goes on for up to {lease_s} seconds, and it begins again when you open Halvo."
            ),
            "the sentence names the seconds of the lease's own constant"
        );
        let lines = |line: &[&str]| line.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        let goes_on_and_exit = lines(&["pause", "stay, goes on", "pause", "exit 0"]);
        let may_go_on_and_exit = lines(&["pause", "stay, cannot tell", "pause", "exit 0"]);
        let may_go_on_and_exit_ending_the_child = lines(&[
            "pause",
            "stay, cannot tell",
            "pause",
            "closed descriptor",
            "exit 0",
        ]);
        let stays = lines(&["pause", "stay, not paused", "pause", "stay, not paused"]);
        let no_daemon = || {
            Err(ClientError::Unreachable {
                endpoint: String::new(),
            })
        };
        let rows: Vec<Row3> = vec![
            (
                "a passed deadline while no child of the app lives",
                vec![
                    Err(ClientError::Deadline { seconds: 10 }),
                    Err(ClientError::Deadline { seconds: 10 }),
                ],
                None,
                goes_on_and_exit.clone(),
            ),
            (
                "after one stay for another, a press whose Pause reads busy stays",
                vec![
                    Err(ClientError::Deadline { seconds: 10 }),
                    Err(ClientError::Daemon(ErrorReply::new(
                        ErrorCode::Busy,
                        "busy",
                    ))),
                ],
                None,
                lines(&["pause", "stay, goes on", "pause", "stay, not paused"]),
            ),
            (
                "a held directory while no child of the app lives",
                vec![
                    Err(ClientError::DataDirInUse {
                        endpoint: String::new(),
                    }),
                    no_daemon(),
                ],
                None,
                goes_on_and_exit.clone(),
            ),
            (
                "a broken exchange while no child of the app lives",
                vec![
                    Err(ClientError::Io(io::Error::from(io::ErrorKind::BrokenPipe))),
                    no_daemon(),
                ],
                None,
                goes_on_and_exit,
            ),
            (
                "another version of a daemon that is no child of the app",
                vec![
                    Err(ClientError::ReplyOfAnotherVersion { version: 2 }),
                    no_daemon(),
                ],
                None,
                may_go_on_and_exit,
            ),
            (
                "a server of another user",
                vec![
                    Err(ClientError::PeerRejected(
                        PeerRejection::ServerOfAnotherUser,
                    )),
                    no_daemon(),
                ],
                Some(true),
                may_go_on_and_exit_ending_the_child.clone(),
            ),
            (
                "a server that is not the app's child",
                vec![
                    Err(ClientError::PeerRejected(
                        PeerRejection::NotTheExpectedServer,
                    )),
                    Err(ClientError::PeerRejected(
                        PeerRejection::NotTheExpectedServer,
                    )),
                ],
                Some(true),
                may_go_on_and_exit_ending_the_child,
            ),
            (
                "busy: a later press can pause it",
                vec![
                    Err(ClientError::Daemon(ErrorReply::new(
                        ErrorCode::Busy,
                        "busy",
                    ))),
                    Err(ClientError::Daemon(ErrorReply::new(
                        ErrorCode::Busy,
                        "busy",
                    ))),
                ],
                Some(true),
                stays.clone(),
            ),
            (
                "an error reply",
                vec![
                    Err(ClientError::Daemon(ErrorReply::new(
                        ErrorCode::Internal,
                        "internal",
                    ))),
                    Ok(sample_status(Platform::Macos)),
                ],
                Some(true),
                stays,
            ),
            (
                "the app's own child answers in another version and does not end: one press",
                vec![Err(ClientError::ReplyOfAnotherVersion { version: 2 })],
                Some(false),
                lines(&["pause", "closed descriptor", "kill after 20 s", "exit 0"]),
            ),
        ];
        for (name, answers, child, expected) in rows {
            let record = Record::default();
            let clock = Arc::new(StandInClock::default());
            let pid = Arc::<ChildPid>::default();
            let spawner = StandInSpawner {
                children: child
                    .map(|ends_at_close| StandInChild {
                        ends_at_close,
                        ..StandInChild::new(7, &clock, &record)
                    })
                    .into_iter()
                    .collect(),
                record: Arc::clone(&record),
            };
            let supervisor = Supervisor::start(spawner, Arc::clone(&pid), Arc::clone(&clock));
            if child.is_some() {
                until(name, || pid.get() == Some(7));
            }
            record.lock().unwrap().clear();
            let control = Scripted::new(&record, Vec::new(), answers);
            let presses = Presses::default();
            for _ in 0..2 {
                match quit_and_pause(&control, Some(&supervisor), &presses) {
                    Quit::Exit(status) => {
                        note(&record, format!("exit {status}"));
                        break;
                    }
                    Quit::Stay(s) if s == CAPTURE_GOES_ON.as_str() => {
                        note(&record, "stay, goes on")
                    }
                    Quit::Stay(CAPTURE_MAY_GO_ON) => note(&record, "stay, cannot tell"),
                    Quit::Stay(NOT_PAUSED) => note(&record, "stay, not paused"),
                    Quit::Stay(NOT_ANSWERED) => note(&record, "stay, not answered"),
                    Quit::Stay(other) => panic!("{name}: a stay with {other}"),
                }
            }
            assert_eq!(*record.lock().unwrap(), expected, "{name}");
            crate::quit::Children::end(&supervisor);
        }
    }

    struct SlowDelete {
        places: Arc<Places>,
        record: Record,
        statuses: Mutex<std::collections::VecDeque<Result<DaemonStatus, ClientError>>>,
        deletes: Mutex<std::sync::mpsc::Receiver<Result<DeleteOutcome, ClientError>>>,
    }

    impl Control for SlowDelete {
        fn places(&self) -> &Arc<Places> {
            &self.places
        }

        fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
            note(&self.record, "status");
            let next = self.statuses.lock().unwrap().pop_front();
            next.expect("one answer for each Status")
        }

        fn status_of_a_pass(
            &self,
            made: Made,
            claim: bool,
            hold: &Hold,
        ) -> Result<DaemonStatus, ClientError> {
            self.status(made, claim && !hold.stands())
        }

        fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
            unreachable!("no inventory here")
        }

        fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            unreachable!("no Pause here")
        }

        fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            unreachable!("no Resume here")
        }

        fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
            note(&self.record, "delete begins");
            let answer = self.deletes.lock().unwrap().recv();
            note(&self.record, "delete answers");
            answer.expect("the test gives the delete its answer")
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

        fn relay(&self, _: Made, _: &[u8]) -> Result<Vec<u8>, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn owner_stream(&self, _: Made) -> Result<halvo_control::r2::OwnerStreamEnd, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn run_mark(
            &self,
            _: Made,
            _: halvo_control::r2::RunEdge,
        ) -> Result<halvo_control::r2::RunMarked, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn defer(&self, _: Made, _: i64) -> Result<Option<i64>, ClientError> {
            Err(ClientError::Unsupported)
        }

        fn acknowledge_lost_permission(
            &self,
            _: Made,
            _: Permission,
        ) -> Result<DaemonStatus, ClientError> {
            Err(ClientError::Unsupported)
        }
    }

    struct EachItsOwn {
        places: Arc<Places>,
    }

    fn own<T>(kind: &'static str) -> Result<T, ClientError> {
        Err(ClientError::UnexpectedReply { kind })
    }

    fn kind_of<T>(answer: Result<T, ClientError>) -> &'static str {
        match answer {
            Err(ClientError::UnexpectedReply { kind }) => kind,
            Err(ClientError::Unsupported) => "unsupported",
            Err(_) => "another error",
            Ok(_) => "an answer",
        }
    }

    impl Control for EachItsOwn {
        fn places(&self) -> &Arc<Places> {
            &self.places
        }

        fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
            own("status")
        }

        fn status_of_a_pass(
            &self,
            _: Made,
            _: bool,
            _: &Hold,
        ) -> Result<DaemonStatus, ClientError> {
            own("status_of_a_pass")
        }

        fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
            own("inventory")
        }

        fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            own("pause")
        }

        fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            own("resume")
        }

        fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
            own("delete_all")
        }

        fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
            own("set_storage_cap")
        }

        fn request_permission(
            &self,
            _: Made,
            _: Permission,
        ) -> Result<RequestOutcome, ClientError> {
            own("request_permission")
        }

        fn relay(&self, _: Made, _: &[u8]) -> Result<Vec<u8>, ClientError> {
            own("relay")
        }

        fn acknowledge_lost_permission(
            &self,
            _: Made,
            _: Permission,
        ) -> Result<DaemonStatus, ClientError> {
            own("acknowledge_lost_permission")
        }

        #[cfg(target_os = "macos")]
        fn owner_stream(&self, _: Made) -> Result<halvo_control::r2::OwnerStreamEnd, ClientError> {
            own("owner_stream")
        }

        #[cfg(target_os = "macos")]
        fn run_mark(
            &self,
            _: Made,
            _: halvo_control::r2::RunEdge,
        ) -> Result<halvo_control::r2::RunMarked, ClientError> {
            own("run_mark")
        }

        #[cfg(target_os = "macos")]
        fn defer(&self, _: Made, _: i64) -> Result<Option<i64>, ClientError> {
            own("defer")
        }
    }

    fn status_of(pid: u32, versions_up: u32, pausing: bool) -> DaemonStatus {
        let app: halvo_control::proto::DaemonVersion =
            crate::app::VERSION.parse().expect("the app's version");
        let mut f = sample_status(Platform::Macos).into_fields();
        f.pid = pid;
        f.daemon_version = halvo_control::proto::DaemonVersion {
            major: app.major + versions_up,
            ..app
        };
        if pausing {
            f.capture = Capture::Pausing;
            let line = f
                .sources
                .iter_mut()
                .find(|line| line.state != SourceState::Off);
            line.expect("a source that runs").unstopped = true;
        }
        DaemonStatus::new(f).expect("the status holds together")
    }

    #[test]
    fn another_daemon_version_restarts_the_own_child_once_and_a_child_seen_ending_is_killed() {
        use crate::commands::control::Watching;
        use crate::view_state::Service;
        assert_eq!(STOP_WAIT, Duration::from_millis(20_000));
        let deadline = || Err(ClientError::Deadline { seconds: 2 });
        let rows: Vec<Row4> = vec![
            (
                "another daemon_version of the own child: one end and one start at once, none at a second reading",
                vec![
                    (1000, "", Ok(status_of(1000, 1, false))),
                    (1001, "", Ok(status_of(1001, 1, false))),
                ],
                vec![(1000, true), (1001, true)],
                vec!["start 1000", "status", "closed descriptor", "start 1001", "status"],
                (false, false, Some(Service::AnotherVersion)),
            ),
            (
                "a reply of another version from the own child: one end and one start at once",
                vec![(1000, "", Err(ClientError::ReplyOfAnotherVersion { version: 2 }))],
                vec![(1000, false), (1001, true)],
                vec![
                    "start 1000",
                    "status",
                    "closed descriptor",
                    "kill after 20 s",
                    "start 1001",
                ],
                (false, false, None),
            ),
            (
                "another daemon_version of a daemon that is not the app's child: none",
                vec![(1000, "", Ok(status_of(4242, 1, false)))],
                vec![(1000, true)],
                vec!["start 1000", "status"],
                (false, false, Some(Service::Running)),
            ),
            (
                "pausing beside a source that did not stop: the kill after STOP_WAIT_MS, and the hold",
                vec![(1000, "", Ok(status_of(1000, 0, true)))],
                vec![(1000, false), (1001, true)],
                vec![
                    "start 1000",
                    "status",
                    "closed descriptor",
                    "kill after 20 s",
                    "start 1001",
                ],
                (true, true, None),
            ),
            (
                "pausing beside a source that did not stop, and an end of its own after the close: the hold",
                vec![(1000, "", Ok(status_of(1000, 0, true)))],
                vec![(1000, true), (1001, true)],
                vec!["start 1000", "status", "closed descriptor", "start 1001"],
                (true, true, None),
            ),
            (
                "a passed deadline: the kill after STOP_WAIT_MS, and no hold",
                vec![(1000, "", deadline())],
                vec![(1000, false), (1001, true)],
                vec![
                    "start 1000",
                    "status",
                    "closed descriptor",
                    "kill after 20 s",
                    "start 1001",
                ],
                (false, true, None),
            ),
            (
                "an own child of the app's version that answers unsupported_op: no restart, no flag",
                vec![
                    (
                        1000,
                        "",
                        Err(ClientError::Daemon(halvo_control::proto::ErrorReply::new(
                            halvo_control::proto::ErrorCode::UnsupportedOp,
                            "this build of the capture service does not serve the op",
                        ))),
                    ),
                    (1000, "", Ok(status_of(1000, 0, false))),
                ],
                vec![(1000, true), (1001, true)],
                vec!["start 1000", "status", "status"],
                (false, false, Some(Service::Running)),
            ),
            (
                "a deadline, then pausing beside a source that did not stop inside the stop's wait: the hold",
                vec![
                    (1000, "", deadline()),
                    (1000, "closed descriptor", Ok(status_of(1000, 0, true))),
                ],
                vec![(1000, false), (1001, true)],
                vec![
                    "start 1000",
                    "status",
                    "closed descriptor",
                    "status",
                    "kill after 20 s",
                    "start 1001",
                ],
                (true, true, None),
            ),
        ];
        let tray = crate::tray::installed_for_tests();
        for (name, steps, children, expected, (held, backoff, service)) in rows {
            let record = Record::default();
            let clock = Arc::new(StandInClock::default());
            let views = Arc::new(Views::default());
            let pid = Arc::<ChildPid>::default();
            let mut waits = Vec::new();
            let mut answers = Vec::new();
            for (child, line, answer) in steps {
                waits.push((child, line));
                answers.push(answer);
            }
            let control = Arc::new(Watching::new(
                Scripted::new(&record, answers, Vec::new()),
                Arc::clone(&pid),
            ));
            let at_close = |child: u32| {
                let inside_the_stop = waits
                    .iter()
                    .any(|(of, line)| *of == child && *line == "closed descriptor");
                inside_the_stop.then(|| {
                    let (control, views) = (Arc::clone(&control), Arc::clone(&views));
                    AtClose(Arc::new(move || {
                        let tray = crate::tray::installed_for_tests();
                        pass(control.as_ref(), &views, Some(&tray));
                    }))
                })
            };
            let spawner = StandInSpawner {
                children: children
                    .into_iter()
                    .map(|(pid, ends_at_close)| StandInChild {
                        ends_at_close,
                        at_close: at_close(pid),
                        ..StandInChild::new(pid, &clock, &record)
                    })
                    .collect(),
                record: Arc::clone(&record),
            };
            let supervisor = Supervisor::start_holding(
                spawner,
                Arc::clone(&pid),
                Arc::clone(&clock),
                Arc::clone(&views),
            );
            for (child, line) in waits {
                if line == "closed descriptor" {
                    continue;
                }
                until(name, || {
                    pid.get() == Some(child)
                        && (line.is_empty()
                            || record.lock().unwrap().iter().any(|seen| seen == line))
                });
                pass(control.as_ref(), &views, Some(&tray));
            }
            until(name, || record.lock().unwrap().len() >= expected.len());
            looked_again(&clock);
            assert_eq!(*record.lock().unwrap(), expected, "{name}");
            assert_eq!(views.hold().stands(), held, "{name}: the hold");
            assert_eq!(
                clock.waits().contains(&BACKOFF_FIRST),
                backoff,
                "{name}: the backoff before the next child"
            );
            if let Some(service) = service {
                assert_eq!(views.view().service(), service, "{name}: the view");
            }
            drop(supervisor.close());
        }

        let places = Arc::<Places>::default();
        let control = crate::commands::control::watched(
            EachItsOwn {
                places: Arc::clone(&places),
            },
            Arc::default(),
        );
        assert!(
            Arc::ptr_eq(control.places(), &places),
            "places is handed on"
        );
        let made = || control.places().make();
        #[cfg(target_os = "macos")]
        let of_the_system = vec![
            kind_of(control.owner_stream(made())),
            kind_of(control.run_mark(made(), halvo_control::r2::RunEdge::Start)),
            kind_of(control.defer(made(), 0)),
        ];
        #[cfg(not(target_os = "macos"))]
        let of_the_system: Vec<&str> = Vec::new();
        let seen: Vec<&str> = [
            kind_of(control.status(made(), false)),
            kind_of(control.status_of_a_pass(made(), true, &Hold::default())),
            kind_of(control.inventory(made())),
            kind_of(control.pause(made())),
            kind_of(control.resume(made())),
            kind_of(control.delete_all(made(), None)),
            kind_of(control.set_storage_cap(made(), StorageCap::DEFAULT)),
            kind_of(control.request_permission(made(), Permission::Accessibility)),
            kind_of(control.relay(made(), b"{}")),
            kind_of(control.acknowledge_lost_permission(made(), Permission::Accessibility)),
        ]
        .into_iter()
        .chain(of_the_system)
        .collect();
        let every: Vec<&str> = [
            "status",
            "status_of_a_pass",
            "inventory",
            "pause",
            "resume",
            "delete_all",
            "set_storage_cap",
            "request_permission",
            "relay",
            "acknowledge_lost_permission",
        ]
        .into_iter()
        .chain(if cfg!(target_os = "macos") {
            vec!["owner_stream", "run_mark", "defer"]
        } else {
            Vec::new()
        })
        .collect();
        assert_eq!(
            seen, every,
            "every method of Control comes back through the wrapper as the control inside answered it"
        );

        let record = Record::default();
        let clock = Arc::new(StandInClock::default());
        let spawner = StandInSpawner {
            children: [(1000, false), (1001, true)]
                .into_iter()
                .map(|(pid, ends_at_close)| StandInChild {
                    ends_at_close,
                    ..StandInChild::new(pid, &clock, &record)
                })
                .collect(),
            record: Arc::clone(&record),
        };
        let views = Arc::new(Views::default());
        let pid = Arc::<ChildPid>::default();
        let supervisor = Supervisor::start_holding(
            spawner,
            Arc::clone(&pid),
            Arc::clone(&clock),
            Arc::clone(&views),
        );
        let (give, deletes) = std::sync::mpsc::channel();
        let control = Arc::new(Watching::new(
            SlowDelete {
                places: Arc::default(),
                record: Arc::clone(&record),
                statuses: Mutex::new(vec![deadline(), deadline()].into()),
                deletes: Mutex::new(deletes),
            },
            Arc::clone(&pid),
        ));
        let delete = |control: &Arc<Watching<SlowDelete>>| {
            let control = Arc::clone(control);
            std::thread::spawn(move || control.delete_all(control.places().make(), None))
        };
        let lines = || record.lock().unwrap().clone();
        let name = "a delete of the app in flight";
        until(name, || pid.get() == Some(1000));
        let deleting = delete(&control);
        until(name, || lines().contains(&"delete begins".to_owned()));
        pass(control.as_ref(), &views, Some(&tray));
        looked_again(&clock);
        assert_eq!(
            lines(),
            ["start 1000", "delete begins", "status"],
            "{name}: a Status that passes its deadline stops nothing"
        );
        assert_eq!(
            views.view().service(),
            Service::HeldUnreachable,
            "{name}: the view of the moment reads that the service does not answer"
        );
        give.send(Ok(DeleteOutcome::Deleted(halvo_control::proto::Deleted {
            report: halvo_control::testing::sample_delete_report(),
            store: None,
        })))
        .unwrap();
        let answer = deleting.join().unwrap();
        assert!(
            matches!(
                crate::view_state::delete_view(answer, None),
                Ok(crate::view_state::DeleteView::Deleted { .. })
            ),
            "{name}: the page reads the delete as done"
        );
        looked_again(&clock);
        assert_eq!(pid.get(), Some(1000), "{name}: the child lives");
        pass(control.as_ref(), &views, Some(&tray));
        until(name, || lines().contains(&"start 1001".to_owned()));
        let deleting = delete(&control);
        until(name, || {
            lines()
                .iter()
                .filter(|line| *line == "delete begins")
                .count()
                == 2
        });
        give.send(Err(ClientError::Deadline { seconds: 120 }))
            .unwrap();
        assert!(deleting.join().unwrap().is_err());
        until(name, || {
            lines()
                .iter()
                .filter(|line| *line == "closed descriptor")
                .count()
                == 2
        });
        drop(supervisor.close());
        assert_eq!(
            lines(),
            [
                "start 1000",
                "delete begins",
                "status",
                "delete answers",
                "status",
                "closed descriptor",
                "kill after 20 s",
                "start 1001",
                "delete begins",
                "delete answers",
                "closed descriptor",
            ],
            "{name}: after its answer a passed deadline is a sight again, and so is the delete's own"
        );
    }

    #[test]
    fn a_timed_out_status_during_the_own_delete_changes_no_view_and_ends_no_child() {
        use crate::commands::capture::{ask_to_delete, DeleteEnd, DELETE_WAITS};
        use crate::commands::control::Watching;
        use crate::view_state::Service;
        let deadline = || Err(ClientError::Deadline { seconds: 2 });
        let record = Record::default();
        let clock = Arc::new(StandInClock::default());
        let spawner = StandInSpawner {
            children: [(1000, false), (1001, true)]
                .into_iter()
                .map(|(pid, ends_at_close)| StandInChild {
                    ends_at_close,
                    ..StandInChild::new(pid, &clock, &record)
                })
                .collect(),
            record: Arc::clone(&record),
        };
        let views = Arc::new(Views::default());
        let pid = Arc::<ChildPid>::default();
        let supervisor = Supervisor::start_holding(
            spawner,
            Arc::clone(&pid),
            Arc::clone(&clock),
            Arc::clone(&views),
        );
        let (give, deletes) = std::sync::mpsc::channel();
        let control = Arc::new(Watching::new(
            SlowDelete {
                places: Arc::default(),
                record: Arc::clone(&record),
                statuses: Mutex::new(
                    vec![
                        Ok(status_of(1000, 0, false)),
                        deadline(),
                        deadline(),
                        deadline(),
                    ]
                    .into(),
                ),
                deletes: Mutex::new(deletes),
            },
            Arc::clone(&pid),
        ));
        let tray = crate::tray::installed_for_tests();
        let lines = || record.lock().unwrap().clone();
        until("the first child starts", || pid.get() == Some(1000));
        pass(control.as_ref(), &views, Some(&tray));
        let before = serde_json::to_value(views.view()).unwrap();
        assert_eq!(before["service"], serde_json::json!("running"));
        let deleting = {
            let (control, views) = (Arc::clone(&control), Arc::clone(&views));
            std::thread::spawn(move || ask_to_delete(control.as_ref(), &views, None, DELETE_WAITS))
        };
        until("the delete is in flight", || {
            lines().contains(&"delete begins".to_owned())
        });
        pass(control.as_ref(), &views, Some(&tray));
        pass(control.as_ref(), &views, Some(&tray));
        looked_again(&clock);
        assert_eq!(
            serde_json::to_value(views.view()).unwrap(),
            before,
            "the view of before the press stands while the app's own delete is in flight"
        );
        assert_eq!(
            lines(),
            ["start 1000", "status", "delete begins", "status", "status"],
            "no lifeline is closed, no child is killed and none is started"
        );
        give.send(Ok(DeleteOutcome::Deleted(halvo_control::proto::Deleted {
            report: halvo_control::testing::sample_delete_report(),
            store: None,
        })))
        .unwrap();
        assert!(matches!(
            deleting.join().unwrap().end,
            DeleteEnd::Answered(Ok(_))
        ));
        pass(control.as_ref(), &views, Some(&tray));
        until("the child seen giving no reply is replaced", || {
            lines().contains(&"start 1001".to_owned())
        });
        let seen = lines();
        drop(supervisor.close());
        assert_eq!(
            views.view().service(),
            Service::HeldUnreachable,
            "with no delete in flight a passed deadline reads as before"
        );
        assert_eq!(
            seen,
            [
                "start 1000",
                "status",
                "delete begins",
                "status",
                "status",
                "delete answers",
                "status",
                "closed descriptor",
                "kill after 20 s",
                "start 1001",
            ],
            "with no delete in flight the same Status makes part 2's sight"
        );
    }

    /// A scripted answer of a delete request, with the end of the child of the moment that
    /// comes after it.
    type ScriptedDelete = (Result<DeleteOutcome, ClientError>, Option<Later>);

    /// A control whose delete requests answer from a script, each with the end of the child of
    /// the moment that comes after its answer; it records each delete with its id and the child
    /// that it reached, each Status with its claim, and each Resume.
    struct Deletes {
        places: Arc<Places>,
        record: Record,
        pid: Arc<ChildPid>,
        answers: Mutex<std::collections::VecDeque<ScriptedDelete>>,
    }

    impl Control for Deletes {
        fn places(&self) -> &Arc<Places> {
            &self.places
        }

        fn status(&self, _: Made, claim: bool) -> Result<DaemonStatus, ClientError> {
            note(&self.record, if claim { "status, claim" } else { "status" });
            Ok(status_of(4242, 0, false))
        }

        fn status_of_a_pass(
            &self,
            made: Made,
            claim: bool,
            hold: &Hold,
        ) -> Result<DaemonStatus, ClientError> {
            self.status(made, claim && !hold.stands())
        }

        fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
            unreachable!("no inventory here")
        }

        fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            unreachable!("no Pause here")
        }

        fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            note(&self.record, "resume");
            Ok(status_of(4242, 0, false))
        }

        fn delete_all(
            &self,
            _: Made,
            armed: Option<StoreId>,
        ) -> Result<DeleteOutcome, ClientError> {
            let id = armed.map_or_else(|| "null".to_owned(), |id| id.to_string());
            let child = self.pid.get().unwrap_or(0);
            note(&self.record, format!("delete {id} to {child}"));
            let (answer, ends) = self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .expect("a scripted answer for each delete");
            if let Some(later) = ends {
                *later.lock().unwrap() = Some(Ending::Status(5));
            }
            answer
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

        fn relay(&self, _: Made, _: &[u8]) -> Result<Vec<u8>, ClientError> {
            Err(ClientError::Unsupported)
        }

        fn acknowledge_lost_permission(
            &self,
            _: Made,
            _: Permission,
        ) -> Result<DaemonStatus, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn owner_stream(&self, _: Made) -> Result<halvo_control::r2::OwnerStreamEnd, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn run_mark(
            &self,
            _: Made,
            _: halvo_control::r2::RunEdge,
        ) -> Result<halvo_control::r2::RunMarked, ClientError> {
            Err(ClientError::Unsupported)
        }

        #[cfg(target_os = "macos")]
        fn defer(&self, _: Made, _: i64) -> Result<Option<i64>, ClientError> {
            Err(ClientError::Unsupported)
        }
    }

    #[test]
    fn after_a_5_inside_a_delete_the_flow_asks_once_more_with_the_old_id_and_stops_on_the_typed_busy(
    ) {
        use crate::commands::capture::{ask_to_delete, DeleteEnd, DeleteWaits};
        use crate::commands::control::Watching;
        use halvo_control::proto::{AlreadyDeleted, ErrorCode, ErrorReply, Life};
        type Answer = fn() -> Result<DeleteOutcome, ClientError>;
        fn armed_id() -> StoreId {
            "00000000000000000000000000000009".parse().unwrap()
        }
        fn busy() -> Result<DeleteOutcome, ClientError> {
            Err(ClientError::Daemon(ErrorReply::new(
                ErrorCode::Busy,
                "another process holds the store; this request deleted nothing",
            )))
        }
        fn no_reply() -> Result<DeleteOutcome, ClientError> {
            Err(ClientError::Deadline { seconds: 120 })
        }
        fn already_deleted() -> Result<DeleteOutcome, ClientError> {
            Ok(DeleteOutcome::AlreadyDeleted(AlreadyDeleted {
                current: "00000000000000000000000000000010".parse().unwrap(),
            }))
        }
        fn refused() -> Result<DeleteOutcome, ClientError> {
            Ok(DeleteOutcome::Refused)
        }
        fn typed_busy() -> Result<DeleteOutcome, ClientError> {
            let mut daemon = halvo_control::testing::ScriptedDaemon::new(status_of(1001, 0, false));
            daemon.set_life(Life::Ending);
            let connector = halvo_control::testing::ScriptedConnector::new(daemon);
            halvo_control::client::Client::new(connector).delete_all(Some(armed_id()))
        }
        assert!(
            matches!(typed_busy(), Err(ClientError::ServiceEnding(_))),
            "the client reads the busy of an ending life as its own kind"
        );
        let id = armed_id();
        let long = DeleteWaits {
            asked_life_end: Duration::from_secs(10),
            next_life: Duration::from_secs(10),
        };
        let short = DeleteWaits {
            asked_life_end: Duration::from_millis(200),
            ..long
        };
        let lines = |of: &[&str]| of.iter().map(|line| (*line).to_owned()).collect::<Vec<_>>();
        let first = format!("delete {id} to 1000");
        let second = format!("delete {id} to 1001");
        let twice = lines(&["start 1000", &first, "start 1001", "status", &second]);
        type AskRow = (
            &'static str,
            Option<StoreId>,
            Vec<(Answer, bool)>,
            DeleteWaits,
            Vec<String>,
            &'static str,
        );
        let rows: Vec<AskRow> = vec![
            (
                "a busy and an end with 5: one second ask with the old id to the next child, and already_deleted is done",
                Some(id),
                vec![(busy as Answer, true), (already_deleted as Answer, false)],
                long,
                twice.clone(),
                "done",
            ),
            (
                "no reply and an end with 5: one second ask with the old id, and delete_refused is not done",
                Some(id),
                vec![(no_reply as Answer, true), (refused as Answer, false)],
                long,
                twice.clone(),
                "refused",
            ),
            (
                "a busy and an end with 5, then the typed busy: no third ask",
                Some(id),
                vec![(busy as Answer, true), (typed_busy as Answer, false)],
                long,
                twice,
                "ending",
            ),
            (
                "a plain busy in a life that goes on: no second ask",
                Some(id),
                vec![(busy as Answer, false)],
                short,
                lines(&["start 1000", &first]),
                "busy",
            ),
            (
                "a busy and an end with 5 after a delete with no id: no second ask",
                None,
                vec![(busy as Answer, true)],
                long,
                lines(&["start 1000", "delete null to 1000", "start 1001"]),
                "busy",
            ),
        ];
        for (name, armed, script, waits, expected, end) in rows {
            let record = Record::default();
            let clock = Arc::new(StandInClock::default());
            let first_ends = Later::default();
            let spawner = StandInSpawner {
                children: [
                    StandInChild {
                        later: Some(Arc::clone(&first_ends)),
                        ..StandInChild::new(1000, &clock, &record)
                    },
                    StandInChild::new(1001, &clock, &record),
                ]
                .into_iter()
                .collect(),
                record: Arc::clone(&record),
            };
            let views = Arc::new(Views::default());
            let pid = Arc::<ChildPid>::default();
            let supervisor = Supervisor::start_holding(
                spawner,
                Arc::clone(&pid),
                Arc::clone(&clock),
                Arc::clone(&views),
            );
            let fives = script.iter().filter(|(_, ends)| *ends).count();
            let control = Watching::new(
                Deletes {
                    places: Arc::default(),
                    record: Arc::clone(&record),
                    pid: Arc::clone(&pid),
                    answers: Mutex::new(
                        script
                            .into_iter()
                            .map(|(answer, ends)| (answer(), ends.then(|| Arc::clone(&first_ends))))
                            .collect(),
                    ),
                },
                Arc::clone(&pid),
            );
            until(name, || pid.get() == Some(1000));
            let ended = ask_to_delete(&control, &views, armed, waits);
            if fives > 0 {
                until(name, || {
                    record
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|line| line == "start 1001")
                });
            }
            let seen: Vec<String> = record
                .lock()
                .unwrap()
                .iter()
                .filter(|line| !line.starts_with("closed") && !line.starts_with("kill"))
                .cloned()
                .collect();
            drop(supervisor.close());
            assert_eq!(seen, expected, "{name}");
            let read = match &ended.end {
                DeleteEnd::Answered(Ok(
                    DeleteOutcome::Deleted(_) | DeleteOutcome::AlreadyDeleted(_),
                )) => "done",
                DeleteEnd::Answered(Ok(DeleteOutcome::Refused)) => "refused",
                DeleteEnd::Answered(Err(ClientError::ServiceEnding(_))) => "ending",
                DeleteEnd::Answered(Err(ClientError::Daemon(reply)))
                    if reply.code() == ErrorCode::Busy =>
                {
                    "busy"
                }
                _ => "another end",
            };
            assert_eq!(read, end, "{name}: {:?}", ended.end);
            assert_eq!(
                views.hold().stands(),
                fives > 0,
                "{name}: part 1's hold after a 5"
            );
        }
    }

    #[test]
    fn after_a_first_no_reply_a_second_ask_that_removes_nothing_leaves_the_outcome_not_known() {
        use crate::commands::capture::AppState;
        use crate::commands::control::Watching;
        use crate::tests::{
            invoke, macos_fixture_daemon, main_window, scratch_dir, test_app_from, test_parts,
            APP_URL,
        };
        use halvo_control::proto::ErrorReply;
        fn no_pane(_: Permission) -> Result<(), String> {
            Ok(())
        }
        let id: StoreId = "00000000000000000000000000000009".parse().unwrap();
        let record = Record::default();
        let clock = Arc::new(StandInClock::default());
        let first_ends = Later::default();
        let spawner = StandInSpawner {
            children: [
                StandInChild {
                    later: Some(Arc::clone(&first_ends)),
                    ..StandInChild::new(1000, &clock, &record)
                },
                StandInChild::new(1001, &clock, &record),
            ]
            .into_iter()
            .collect(),
            record: Arc::clone(&record),
        };
        let pid = Arc::<ChildPid>::default();
        let control = Watching::new(
            Deletes {
                places: Arc::default(),
                record: Arc::clone(&record),
                pid: Arc::clone(&pid),
                answers: Mutex::new(
                    [
                        (
                            Err(ClientError::Deadline { seconds: 120 }),
                            Some(Arc::clone(&first_ends)),
                        ),
                        (
                            Err(ClientError::ServiceEnding(
                                ErrorReply::busy_of_an_ending_life(),
                            )),
                            None,
                        ),
                    ]
                    .into(),
                ),
            },
            Arc::clone(&pid),
        );
        let state = AppState::with_control(Arc::new(control), None, no_pane);
        let supervisor = Supervisor::start_holding(
            spawner,
            Arc::clone(&pid),
            Arc::clone(&clock),
            Arc::clone(&state.views),
        );
        let test = test_app_from(
            test_parts(scratch_dir("delete-first-no-reply"), state),
            crate::app::context(),
            halvo_control::testing::ScriptedConnector::new(macos_fixture_daemon()),
        );
        until("the first child starts", || pid.get() == Some(1000));
        let reply = invoke(
            &main_window(&test.app),
            APP_URL,
            "delete_all",
            serde_json::json!({ "storeId": id.to_string() }),
        );
        let seen = record.lock().unwrap().clone();
        drop(supervisor.close());
        let asked: Vec<&String> = seen
            .iter()
            .filter(|line| line.starts_with("delete"))
            .collect();
        assert_eq!(
            asked,
            [
                &format!("delete {id} to 1000"),
                &format!("delete {id} to 1001")
            ],
            "a first ask with no reply, an end with 5, and one second ask to the next child"
        );
        assert_eq!(
            reply,
            Err(serde_json::json!({ "error": "deadline", "seconds": 120, "outcome_not_known": true })),
            "the typed busy of the second ask removed nothing by itself: what the first ask did stays not known, and the page draws DELETE_UNKNOWN_OUTCOME for it"
        );
    }

    #[test]
    fn the_child_is_started_again_by_its_exit_status() {
        let s = Duration::from_secs;
        let status = Ending::Status;
        let rows: Vec<Row> = vec![
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
                    version_restarted: false,
                }),
                changed: Condvar::new(),
            };
            let pid = ChildPid::default();
            supervise(spawner, &shared, &pid, clock.as_ref(), &Views::default());
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
