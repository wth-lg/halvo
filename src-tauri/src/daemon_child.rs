use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use halvo_control::paths::DataDir;
use halvo_control::service::{DAEMON_ARG, DATA_DIR_ARG};

use crate::commands::control::ChildPid;

/// The flag that makes the daemon watch its parent through the descriptor on its standard
/// input (contracts.md C14 item 1).
pub const LIFELINE_ARG: &str = "--lifeline";

/// How often the supervisor reads whether the child of the moment has ended.
pub const CHILD_POLL: Duration = Duration::from_millis(250);

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

/// A started child as the supervisor watches it.
pub trait Watched: Send + 'static {
    fn pid(&self) -> u32;
    fn ended(&mut self) -> io::Result<Option<Ending>>;
    fn close_lifeline(&mut self);
}

/// What starts the app's child: `Ok(None)` when no child may start, because the data
/// directory is not provisioned.
pub trait Spawner: Send + 'static {
    type Child: Watched;
    fn start(&mut self) -> io::Result<Option<Self::Child>>;
}

/// The time of the supervisor: the system's in the app, a stand-in in the tests.
pub trait Clock: Send + Sync + 'static {
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
pub struct Supervisor<C> {
    shared: Arc<Shared<C>>,
}

impl<C: Watched> Supervisor<C> {
    /// Starts the supervisor's thread, which starts the child off the main thread and gives
    /// `pid` the pid of the child of the moment.
    pub fn start<S: Spawner<Child = C>, K: Clock>(
        spawner: S,
        pid: Arc<ChildPid>,
        clock: Arc<K>,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                quitting: false,
                child: None,
            }),
            changed: Condvar::new(),
        });
        let watched = Arc::clone(&shared);
        let started = std::thread::Builder::new()
            .name("halvo-daemon-child".to_owned())
            .spawn(move || supervise(spawner, &watched, &pid, clock.as_ref()));
        if let Err(e) = started {
            eprintln!("halvo: the capture service is not started: {e}");
        }
        Supervisor { shared }
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

fn supervise<S: Spawner, K: Clock>(
    mut spawner: S,
    shared: &Shared<S::Child>,
    pid: &ChildPid,
    clock: &K,
) {
    let ending = match spawner.start() {
        Ok(None) => return,
        Ok(Some(child)) => {
            let mut state = shared.lock();
            if state.quitting {
                return;
            }
            pid.0.store(child.pid(), Ordering::SeqCst);
            state.child = Some(child);
            drop(state);
            match watch(shared, clock) {
                Some(ending) => ending,
                None => return,
            }
        }
        Err(e) => {
            eprintln!("halvo: the capture service did not start: {e}");
            Ending::NotStarted
        }
    };
    pid.0.store(0, Ordering::SeqCst);
    eprintln!("halvo: the capture service ended: {ending:?}");
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
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::launch::{parse, Launch};

    #[cfg(unix)]
    const STUB: &str = "daemon_child::tests::stub_child";
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
}
