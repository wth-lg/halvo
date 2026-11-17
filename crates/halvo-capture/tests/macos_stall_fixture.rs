#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType};
use halvo_capture::event::Modality;
use halvo_capture::macos::foreground::ForegroundReader;
use halvo_capture::macos::input::{
    Arrived, Caller, DropCounts, HoldPlan, InputCounters, InputSource, RawEntry, StallSeam,
};
use halvo_capture::secure_input_macos::hold::Cause;
use halvo_capture::secure_input_macos::hold::SESSION_STEP;
use halvo_capture::secure_input_macos::hold::{Bounds, HeldKeys, Monotonic, Stamps, Step, Timing};
use halvo_capture::secure_input_macos::hold::{GAP_BOUND, HOLD_BOUND, READING_STEP};
use halvo_capture::secure_input_macos::notice::{Focus, Seen as NoticeSeen};
use halvo_capture::secure_input_macos::{read_session, secure_input_active, Reading, Session};
use halvo_capture::sink;
use halvo_capture::source::{CaptureSource, HealthReason, SourceContext, SourceState};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

extern "C" {
    fn clock_gettime_nsec_np(clock: libc::clockid_t) -> u64;
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut Timebase) -> i32;
    fn CGEventGetTimestamp(event: *const std::ffi::c_void) -> u64;
}

const FIXTURE: &str = "HALVO_STALL_FIXTURE";
const TAB: u16 = 48;
const RETURN: u16 = 36;
const LETTERS: [u16; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
const MARKERS: [u16; 2] = [8, 9];
const SHORT_BOUND: Duration = Duration::from_millis(1_000);
const LONG_BOUND: Duration = Duration::from_millis(3_000);
const JUST_UNDER: u64 = 850;
const ROUNDS: usize = 3;
const FAULT_ROUNDS: usize = 5;
const MARGIN: Duration = Duration::from_millis(750);
const LAUNCH_WAIT: Duration = Duration::from_secs(20);
const KEPT_MARGIN: Duration = Duration::from_millis(130);
const NEVER_ON_WAIT: Duration = Duration::from_secs(2);
const WINDOW_MARGIN: u64 = 200_000_000;
const SHORT_WINDOW_AT_150: usize = 40;
const SHORT_WINDOW_AT_2000: usize = 124;
const LEAVES_ASKED_AFTER_MS: [i64; 7] = [0, 5, 10, 20, 40, 80, 200];
const LEAVE_ROUNDS: usize = 5;
const LOOP_COUNTED: usize = 62;
const LOOP_CAP: usize = 68;
const BURST_STEP: Duration = Duration::from_millis(2);
const LETTER_TEXT: &str = "asdfhgzx";

fn raw_ns() -> u64 {
    // SAFETY: clock_gettime_nsec_np takes a clock id by value and returns a
    // number; it touches no memory of the caller.
    unsafe { clock_gettime_nsec_np(libc::CLOCK_MONOTONIC_RAW) }
}

fn uptime_ns() -> u64 {
    // SAFETY: clock_gettime_nsec_np takes a clock id by value and returns a
    // number; it touches no memory of the caller.
    unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) }
}

fn hosted_fixture() -> PathBuf {
    let hosted = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted");
    let path = std::env::var_os(FIXTURE);
    match (hosted, path) {
        (true, Some(path)) => path.into(),
        _ => panic!(
            "refused: this test posts keys and shows a window, so it runs only in the hosted \
             step of a GitHub-hosted runner that sets {FIXTURE}"
        ),
    }
}

fn at_the_console() -> Session {
    Session::AtConsole
}

fn session_reader() -> Option<fn() -> Session> {
    match std::env::var("HALVO_STALL_SESSION").as_deref() {
        Err(_) | Ok("real") => None,
        Ok("console") => Some(at_the_console),
        Ok(_) => panic!("refused: HALVO_STALL_SESSION is real or console"),
    }
}

fn session_line() -> String {
    let session = read_session();
    format!(
        "session_reader={} dictionary={} on_console={}",
        if session_reader().is_some() {
            "console"
        } else {
            "real"
        },
        u8::from(session != Session::NoDictionary),
        u8::from(session == Session::AtConsole)
    )
}

fn millis_of(variable: &str, default: u64) -> Duration {
    match std::env::var(variable) {
        Err(_) => Duration::from_millis(default),
        Ok(text) => Duration::from_millis(
            text.parse()
                .unwrap_or_else(|_| panic!("refused: {variable} is a count of milliseconds")),
        ),
    }
}

fn test_gap() -> Duration {
    millis_of("HALVO_STALL_GAP_MS", 30)
}

fn test_step() -> Duration {
    millis_of("HALVO_STALL_STEP_MS", 10)
}

/// The form of the hosted phases, a closed word that 51-macos-stall hands on
/// (ruling MA-23 Addition 1): `short`, or no value, for the every-push job, and
/// `long` for the scratch run that a change of the hold's files owes.
fn short_form() -> bool {
    match std::env::var("HALVO_STALL_FORM").as_deref() {
        Err(_) | Ok("short") => true,
        Ok("long") => false,
        Ok(other) => panic!("refused: HALVO_STALL_FORM is short or long, not {other}"),
    }
}

/// The short form: one counted round at each wait of a mended phase under a
/// cap of two rounds run, one round of the control, and one counted round of
/// the listed known fault D1 (ruling MA-23 Addition 1 (1)).
const SHORT_ROUNDS_FOR_EACH_WAIT: usize = 1;
const SHORT_CAP_FOR_EACH_WAIT: usize = 2;
const SHORT_CONTROL_ROUNDS: usize = 1;
const SHORT_FAULT_ROUNDS: usize = 1;
/// The return phase runs one round for each wait in both forms.
const RETURN_ROUNDS_FOR_EACH_WAIT: usize = 1;

fn phase_rounds() -> usize {
    if short_form() {
        SHORT_ROUNDS_FOR_EACH_WAIT
    } else {
        ROUNDS
    }
}

fn phase_cap() -> usize {
    if short_form() {
        SHORT_CAP_FOR_EACH_WAIT
    } else {
        ROUNDS + 1
    }
}

fn control_rounds() -> usize {
    if short_form() {
        SHORT_CONTROL_ROUNDS
    } else {
        ROUNDS
    }
}

fn fault_rounds() -> usize {
    if short_form() {
        SHORT_FAULT_ROUNDS
    } else {
        FAULT_ROUNDS
    }
}

static FRONT: AtomicI32 = AtomicI32::new(0);

fn fixture_front() -> Option<libc::pid_t> {
    match FRONT.load(Ordering::SeqCst) {
        0 => None,
        pid => Some(pid),
    }
}

fn no_front() -> Option<libc::pid_t> {
    None
}

fn seamed_with(
    bound: Duration,
    gap: Duration,
    step: Duration,
    session: fn() -> Session,
    front: fn() -> Option<libc::pid_t>,
) -> (InputSource, DropCounts) {
    let drops = DropCounts::new();
    let source = InputSource::with_seam(
        ForegroundReader::nothing_in_front(),
        InputCounters::new(),
        HoldPlan::Never,
        StallSeam {
            bound,
            gap,
            step,
            session,
            front,
            drops: drops.clone(),
        },
    );
    (source, drops)
}

fn seamed(bound: Duration, gap: Duration, step: Duration) -> (InputSource, DropCounts) {
    seamed_with(
        bound,
        gap,
        step,
        session_reader().unwrap_or(read_session),
        no_front,
    )
}

fn source_for(bound: Duration) -> (InputSource, DropCounts) {
    seamed(bound, test_gap(), test_step())
}

fn product_source(bound: Duration) -> (InputSource, DropCounts) {
    let _ = bound;
    seamed(HOLD_BOUND, GAP_BOUND, READING_STEP)
}

const SPARSE_STEP: Duration = Duration::from_millis(200);
const SPARSE_GAP: Duration = Duration::from_millis(450);

fn noticed_sparse_source(bound: Duration) -> (InputSource, DropCounts) {
    seamed_with(
        bound,
        SPARSE_GAP,
        SPARSE_STEP,
        session_reader().unwrap_or(read_session),
        fixture_front,
    )
}

fn noticed_product_source(bound: Duration) -> (InputSource, DropCounts) {
    let _ = bound;
    seamed_with(
        HOLD_BOUND,
        GAP_BOUND,
        READING_STEP,
        session_reader().unwrap_or(read_session),
        fixture_front,
    )
}

static SESSION_DELAY_MS: AtomicU64 = AtomicU64::new(0);

fn delayed_session() -> Session {
    let delay = SESSION_DELAY_MS.swap(0, Ordering::SeqCst);
    if delay > 0 {
        std::thread::sleep(Duration::from_millis(delay));
    }
    session_reader().map_or_else(read_session, |reader| reader())
}

fn delayed_session_source(bound: Duration) -> (InputSource, DropCounts) {
    seamed_with(bound, test_gap(), test_step(), delayed_session, no_front)
}

fn drops_line(drops: &DropCounts) -> String {
    let mut line: Vec<String> = drops
        .by_cause()
        .into_iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect();
    line.push(format!("late_by_the_entry={}", drops.late_by_the_entry()));
    line.push(format!("widest_gap_us={}", drops.widest_gap().as_micros()));
    line.join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NoReply {
    Late { reply: String, wait: Duration },
    Ended { reply: String },
}

impl NoReply {
    fn word(&self) -> String {
        let named = |reply: &str| reply.trim_end_matches('=').replace(' ', "_");
        match self {
            Self::Late { reply, wait } => {
                format!("late_reply_{}_within_{}_ms", named(reply), wait.as_millis())
            }
            Self::Ended { reply } => format!("output_ended_before_{}", named(reply)),
        }
    }
}

impl std::fmt::Display for NoReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Late { reply, wait } => write!(
                f,
                "the fixture gave no line that starts with {reply} within {} ms",
                wait.as_millis()
            ),
            Self::Ended { reply } => write!(
                f,
                "the fixture's output ended before a line that starts with {reply}"
            ),
        }
    }
}

const NOT_DRIVEN: &str = "a reply of the fixture did not come";

struct Fixture {
    child: Option<Child>,
    input: Box<dyn Write + Send>,
    lines: Receiver<String>,
    pid: i32,
    notes: Vec<String>,
    by: &'static str,
    wait: Duration,
    late: Vec<String>,
}

const ASK_WAIT: Duration = Duration::from_secs(10);

fn lines_of(output: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (tx, lines) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { return };
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    lines
}

fn ready_pid(lines: &Receiver<String>, wait: Duration) -> Result<i32, String> {
    let line = lines.recv_timeout(wait).map_err(|error| match error {
        RecvTimeoutError::Timeout => format!(
            "the wait for the fixture's ready line ran out after {} ms",
            wait.as_millis()
        ),
        RecvTimeoutError::Disconnected => {
            "the fixture's output ended before its ready line".to_owned()
        }
    })?;
    line.strip_prefix("ready pid=")
        .and_then(|pid| pid.parse().ok())
        .ok_or_else(|| "the fixture's first line was not its ready line".to_owned())
}

fn fifo(name: &str) -> PathBuf {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("stall-fixture-{}-{name}", std::process::id()));
    let text = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).expect("a path");
    // SAFETY: `text` is a NUL-terminated path that lives through the call.
    let made = unsafe { libc::mkfifo(text.as_ptr(), 0o600) };
    assert!(
        made == 0 || std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists,
        "the fifo {name} is made"
    );
    path
}

fn bundle_of(binary: &Path) -> PathBuf {
    let bundle = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("StallFixture.app");
    let macos = bundle.join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos).expect("the bundle's directory");
    std::fs::copy(binary, macos.join("stall-fixture")).expect("the bundle's program");
    std::fs::write(
        bundle.join("Contents").join("Info.plist"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
         <key>CFBundleIdentifier</key><string>com.halvo.test.stall-fixture</string>\
         <key>CFBundleExecutable</key><string>stall-fixture</string>\
         <key>CFBundlePackageType</key><string>APPL</string>\
         <key>CFBundleName</key><string>StallFixture</string></dict></plist>\n",
    )
    .expect("the bundle's property list");
    bundle
}

fn fifo_opened<T: Send + 'static>(
    path: PathBuf,
    open: impl FnOnce(&Path) -> std::io::Result<T> + Send + 'static,
) -> Receiver<std::io::Result<T>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(open(&path));
    });
    rx
}

fn free_a_blocked_open(path: &Path, write: bool) {
    let _ = std::fs::OpenOptions::new()
        .read(!write)
        .write(write)
        .custom_flags(libc::O_NONBLOCK)
        .open(path);
}

fn wait_for<T>(
    rx: &Receiver<std::io::Result<T>>,
    opener: &mut Child,
    wait: Duration,
    what: &str,
) -> Result<T, String> {
    let deadline = Instant::now() + wait;
    loop {
        match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(Ok(opened)) => return Ok(opened),
            Ok(Err(error)) => return Err(format!("{what} did not open: {:?}", error.kind())),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("the thread that opens {what} ended"))
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if let Ok(Some(status)) = opener.try_wait() {
            if !status.success() {
                return Err(format!("open ended with {status} before {what} opened"));
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the wait for {what} to open ran out after {} ms",
                wait.as_millis()
            ));
        }
    }
}

fn launched(opener: &Path, bundle: &Path, tag: &str, wait: Duration) -> Result<Fixture, String> {
    let to_fixture = fifo(&format!("{tag}in"));
    let from_fixture = fifo(&format!("{tag}out"));
    let output = fifo_opened(from_fixture.clone(), |path| std::fs::File::open(path));
    let input = fifo_opened(to_fixture.clone(), |path| {
        std::fs::OpenOptions::new().write(true).open(path)
    });
    let mut opener = Command::new(opener)
        .arg("-n")
        .arg("--stdin")
        .arg(&to_fixture)
        .arg("--stdout")
        .arg(&from_fixture)
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("open did not start: {:?}", error.kind()))?;
    let opened =
        wait_for(&output, &mut opener, wait, "the fixture's output fifo").and_then(|output| {
            wait_for(&input, &mut opener, wait, "the fixture's input fifo")
                .map(|input| (output, input))
        });
    let ended = opened.and_then(|files| {
        if until(wait, || matches!(opener.try_wait(), Ok(Some(_)))) {
            Ok(files)
        } else {
            Err(format!(
                "the wait for open to end ran out after {} ms",
                wait.as_millis()
            ))
        }
    });
    let _ = opener.kill();
    let status = opener
        .wait()
        .map_err(|error| format!("open could not be waited for: {:?}", error.kind()))?;
    let (output, input) = match ended {
        Ok(files) => files,
        Err(why) => {
            free_a_blocked_open(&from_fixture, true);
            free_a_blocked_open(&to_fixture, false);
            return Err(why);
        }
    };
    if !status.success() {
        return Err(format!("open ended with {status}"));
    }
    let lines = lines_of(output);
    let pid = ready_pid(&lines, wait)?;
    Ok(Fixture {
        child: None,
        input: Box::new(input),
        lines,
        pid,
        notes: Vec::new(),
        by: "opened",
        wait: ASK_WAIT,
        late: Vec::new(),
    })
}

impl Fixture {
    fn spawned() -> Result<Self, String> {
        let mut child = Command::new(hosted_fixture())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("the fixture did not start: {:?}", error.kind()))?;
        let input = child.stdin.take().expect("the fixture's input");
        let lines = lines_of(child.stdout.take().expect("the fixture's output"));
        let pid = match ready_pid(&lines, LAUNCH_WAIT) {
            Ok(pid) => pid,
            Err(why) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(why);
            }
        };
        Ok(Self {
            child: Some(child),
            input: Box::new(input),
            lines,
            pid,
            notes: Vec::new(),
            by: "spawned",
            wait: ASK_WAIT,
            late: Vec::new(),
        })
    }

    fn opened() -> Result<Self, String> {
        launched(
            Path::new("/usr/bin/open"),
            &bundle_of(&hosted_fixture()),
            "",
            LAUNCH_WAIT,
        )
    }

    fn start() -> Result<(Self, Option<&'static str>), String> {
        let mut fixture = Self::spawned()?;
        if let Some(how) = fixture.bring_to_front(Duration::from_secs(5)) {
            return Ok((fixture, Some(how)));
        }
        drop(fixture);
        let mut fixture = Self::opened()?;
        let how = fixture.bring_to_front(Duration::from_secs(5));
        Ok((fixture, how))
    }

    fn send(&mut self, command: &str) {
        writeln!(self.input, "{command}").expect("the fixture takes a command");
        self.input.flush().expect("the fixture takes a command");
    }

    fn try_send(&mut self, command: &str, reply: &str) -> Result<(), NoReply> {
        writeln!(self.input, "{command}")
            .and_then(|()| self.input.flush())
            .map_err(|_| NoReply::Ended {
                reply: reply.to_owned(),
            })
    }

    fn try_reply(&mut self, prefix: &str, within: Duration) -> Result<String, NoReply> {
        if let Some(at) = self.notes.iter().position(|note| note.starts_with(prefix)) {
            return Ok(self.notes.remove(at));
        }
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    if let Some(at) = self.late.iter().position(|late| line.starts_with(late)) {
                        self.late.remove(at);
                    } else if line.starts_with(prefix) {
                        return Ok(line);
                    } else {
                        self.notes.push(line);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.late.push(prefix.to_owned());
                    return Err(NoReply::Late {
                        reply: prefix.to_owned(),
                        wait: within,
                    });
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(NoReply::Ended {
                        reply: prefix.to_owned(),
                    })
                }
            }
        }
    }

    fn reply(&mut self, prefix: &str, within: Duration) -> String {
        self.try_reply(prefix, within)
            .unwrap_or_else(|no_reply| panic!("{no_reply}"))
    }

    fn try_request(
        &mut self,
        command: &str,
        prefix: &str,
        within: Duration,
    ) -> Result<String, NoReply> {
        self.try_send(command, prefix)?;
        self.try_reply(prefix, within)
    }

    fn try_ask(&mut self, command: &str, prefix: &str) -> Result<String, NoReply> {
        self.try_request(command, prefix, self.wait)
    }

    fn ask(&mut self, command: &str, prefix: &str) -> String {
        self.try_ask(command, prefix)
            .unwrap_or_else(|no_reply| panic!("{no_reply}"))
    }

    fn try_state(&mut self) -> Result<FixtureState, NoReply> {
        self.try_ask("state", "front=")
            .map(|line| FixtureState::parse(&line))
    }

    fn state(&mut self) -> FixtureState {
        self.try_state()
            .unwrap_or_else(|no_reply| panic!("{no_reply}"))
    }

    fn try_in_front(&mut self) -> Result<bool, NoReply> {
        let state = self.try_state()?;
        Ok(state.front && state.key && frontmost_pid() == Some(self.pid))
    }

    fn in_front(&mut self) -> bool {
        self.try_in_front()
            .unwrap_or_else(|no_reply| panic!("{no_reply}"))
    }

    fn bring_to_front(&mut self, within: Duration) -> Option<&'static str> {
        if until(within, || self.in_front()) {
            return Some("own");
        }
        self.ask("activate", "asked");
        if until(Duration::from_secs(1), || self.in_front()) {
            return Some("asked");
        }
        activate_from_here(self.pid);
        until(Duration::from_secs(1), || self.in_front()).then_some("other_process")
    }

    fn try_focus(&mut self, field: &str) -> Result<(), NoReply> {
        self.try_ask(field, "focus=").map(drop)
    }

    fn focus(&mut self, field: &str) {
        self.try_focus(field)
            .unwrap_or_else(|no_reply| panic!("{no_reply}"));
    }

    fn try_keys(&mut self, step: Duration, keys: &str) -> Result<(), NoReply> {
        self.try_request(
            &format!("keys {} {keys}", step.as_millis()),
            "posted=",
            Duration::from_secs(20),
        )
        .map(drop)
    }

    fn keys(&mut self, step: Duration, keys: &str) {
        self.try_keys(step, keys)
            .unwrap_or_else(|no_reply| panic!("{no_reply}"));
    }

    fn try_stamps(&mut self) -> Result<Vec<Stamp>, NoReply> {
        self.try_ask("stamps", "stamps n=")
            .map(|line| parse_stamps(&line))
    }

    fn stamps(&mut self) -> Vec<Stamp> {
        self.try_stamps()
            .unwrap_or_else(|no_reply| panic!("{no_reply}"))
    }

    fn try_click(&mut self, at: &str) -> Result<(), NoReply> {
        self.try_request(&format!("click {at}"), "clicked", Duration::from_secs(10))
            .map(drop)
    }

    fn click(&mut self, at: &str) {
        self.try_click(at)
            .unwrap_or_else(|no_reply| panic!("{no_reply}"));
    }

    fn ended(&mut self) -> bool {
        match &mut self.child {
            Some(child) => matches!(child.try_wait(), Ok(Some(_))),
            // SAFETY: kill with the signal 0 sends nothing; it only asks whether
            // the process with this pid exists.
            None => (unsafe { libc::kill(self.pid, 0) }) != 0,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "quit");
        let _ = self.input.flush();
        if until(Duration::from_secs(5), || self.ended()) {
            if let Some(child) = &mut self.child {
                let _ = child.wait();
            }
            return;
        }
        match &mut self.child {
            Some(child) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            // SAFETY: the pid is that of the fixture that this test opened and
            // that answered ready with it; SIGKILL only ends that process.
            None => unsafe {
                libc::kill(self.pid, libc::SIGKILL);
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FixtureState {
    front: bool,
    key: bool,
    focus: String,
    plain: usize,
    secure: usize,
}

impl FixtureState {
    fn parse(line: &str) -> Self {
        let mut state = Self {
            front: false,
            key: false,
            focus: String::new(),
            plain: 0,
            secure: 0,
        };
        for word in line.split(' ') {
            match word.split_once('=') {
                Some(("front", v)) => state.front = v == "1",
                Some(("key", v)) => state.key = v == "1",
                Some(("focus", v)) => state.focus = v.to_owned(),
                Some(("plain", v)) => state.plain = v.parse().unwrap_or(usize::MAX),
                Some(("secure", v)) => state.secure = v.parse().unwrap_or(usize::MAX),
                _ => {}
            }
        }
        state
    }
}

fn frontmost_pid() -> Option<i32> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

fn frontmost_bundle() -> Option<String> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|app| app.bundleIdentifier())
        .map(|id| id.to_string())
}

fn activate_from_here(pid: i32) -> bool {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .is_some_and(|app| app.activateWithOptions(NSApplicationActivationOptions::empty()))
}

fn codes(codes: &[u16]) -> String {
    codes
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

fn secure_field_centre(fixture: &mut Fixture) -> String {
    try_secure_field_centre(fixture).unwrap_or_else(|no_reply| panic!("{no_reply}"))
}

fn try_secure_field_centre(fixture: &mut Fixture) -> Result<String, NoReply> {
    let line = fixture.try_ask("where", "x=")?;
    let mut point = (String::new(), String::new());
    for word in line.split(' ') {
        match word.split_once('=') {
            Some(("x", v)) => point.0 = v.to_owned(),
            Some(("y", v)) => point.1 = v.to_owned(),
            _ => {}
        }
    }
    Ok(format!("{} {}", point.0, point.1))
}

fn until(within: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn state_reads(on: bool, within: Duration) -> bool {
    until(within, || secure_input_active() == on)
}

struct Recorder {
    keys: Arc<Mutex<Vec<SystemTime>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    source: InputSource,
    drops: DropCounts,
}

impl Recorder {
    fn start((mut source, drops): (InputSource, DropCounts)) -> Self {
        let (handle, writer) = sink::channel(4096);
        source
            .begin(SourceContext::new(handle))
            .expect("the input source starts on the hosted runner");
        let keys = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let keys = keys.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match writer.recv_timeout(Duration::from_millis(50)) {
                        Ok(received) => {
                            let event = received.event();
                            if event.modality == Modality::Keystroke {
                                keys.lock()
                                    .unwrap_or_else(PoisonError::into_inner)
                                    .push(event.captured_at.wall_clock);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })
        };
        Self {
            keys,
            stop,
            thread: Some(thread),
            source,
            drops,
        }
    }
}

trait Records {
    fn between(&self, from: SystemTime, to: SystemTime) -> usize;
    fn take_entries(&self) -> Vec<RawEntry>;
}

impl Records for Recorder {
    fn between(&self, from: SystemTime, to: SystemTime) -> usize {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|&&at| at >= from && at < to)
            .count()
    }

    fn take_entries(&self) -> Vec<RawEntry> {
        self.drops.take_entries()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.source.shutdown();
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    Tab,
    Click,
    Itself,
    Return,
    LeavingKeyUp,
    WholePassword,
    WholePasswordBurst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keep {
    Mended,
    AsIs,
    Autoleave,
}

#[derive(Debug)]
struct Round {
    wait_ms: u64,
    counted: bool,
    why_not: &'static str,
    ahead: usize,
    markers: usize,
    plain: usize,
    secure: usize,
    which: String,
    never_on: bool,
    from: u64,
    stamps: Vec<Stamp>,
    entries: Vec<RawEntry>,
    case: Option<Case>,
    verdict: Option<Verdict>,
    not_driven: Option<NoReply>,
    markers_at: Option<u64>,
}

fn return_start(stamps: &[Stamp]) -> u64 {
    stamps
        .iter()
        .position(|stamp| stamp.kind == "cmd_away")
        .and_then(|away| stamps[away..].iter().find(|stamp| !stamp.on))
        .map_or(0, |stamp| stamp.ns)
}

fn keep_the_field(fixture: &mut Fixture, how: Move) -> Result<bool, NoReply> {
    if !state_reads(true, NEVER_ON_WAIT) {
        return Ok(false);
    }
    let stamps = fixture.try_stamps()?;
    let since = if how == Move::Return {
        return_start(&stamps)
    } else {
        0
    };
    let first_on = stamps
        .iter()
        .find(|stamp| stamp.on && stamp.ns >= since)
        .map_or_else(raw_ns, |stamp| stamp.ns);
    let kept = u64::try_from((test_gap() + KEPT_MARGIN).as_nanos()).unwrap_or(u64::MAX);
    let until = first_on.saturating_add(kept);
    while raw_ns() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(true)
}

fn letter_step(wait_ms: u64) -> Duration {
    Duration::from_millis((wait_ms.saturating_sub(60) / 9).max(10))
}

fn round(
    fixture: &mut Fixture,
    recorder: &impl Records,
    how: Move,
    wait_ms: u64,
    bound: Duration,
    keep: Keep,
) -> Round {
    let _ = recorder.take_entries();
    let from = raw_ns();
    let mut round = round_inner(fixture, recorder, how, wait_ms, bound, keep);
    round.from = from;
    match fixture.try_stamps() {
        Ok(stamps) => round.stamps = stamps,
        Err(no_reply) => {
            if round.not_driven.is_none() {
                round.counted = false;
                round.why_not = NOT_DRIVEN;
                round.not_driven = Some(no_reply);
            }
        }
    }
    round.entries = recorder.take_entries();
    let found = evidence(&round.stamps, &round.entries, since_of(how, &round.stamps));
    round.case = (how != Move::Return || round.counted).then(|| short_case(&found));
    if keep == Keep::Mended {
        round.verdict = Some(mended_verdict(
            round.never_on,
            (!round.counted).then_some(round.why_not),
            round.ahead,
            &found,
            bound,
        ));
    }
    if round.not_driven.is_some() {
        round.verdict = Some(not_driven_verdict(&round));
    }
    round
}

fn not_driven_verdict(round: &Round) -> Verdict {
    let recorded = if round.stamps.is_empty() {
        let released = released_keys(&round.entries);
        round.ahead > 0
            || released.len() > MARKERS.len()
            || released
                .iter()
                .any(|&(arrival, _)| match (arrival, round.markers_at) {
                    (Some(arrival), Some(posted)) => arrival < posted,
                    _ => true,
                })
    } else {
        let excused = round.verdict == Some(Verdict::NotCounted(NOT_DRIVEN));
        round.verdict == Some(Verdict::Recorded) || round.ahead > 0 && !excused
    } || round.markers > MARKERS.len();
    if recorded {
        Verdict::Recorded
    } else {
        Verdict::NotDriven
    }
}

fn since_of(how: Move, stamps: &[Stamp]) -> u64 {
    if how == Move::Return {
        return_start(stamps)
    } else {
        0
    }
}

fn round_inner(
    fixture: &mut Fixture,
    recorder: &impl Records,
    how: Move,
    wait_ms: u64,
    bound: Duration,
    keep: Keep,
) -> Round {
    let mut round = Round {
        wait_ms,
        counted: false,
        why_not: "",
        ahead: 0,
        markers: 0,
        plain: 0,
        secure: 0,
        which: String::from("-"),
        never_on: false,
        from: 0,
        stamps: Vec::new(),
        entries: Vec::new(),
        case: None,
        verdict: None,
        not_driven: None,
        markers_at: None,
    };
    let drive = Drive {
        how,
        wait_ms,
        bound,
        keep,
    };
    let mut window = Window {
        ahead_from: SystemTime::now(),
        markers_from: None,
    };
    if let Err(no_reply) = drive.round(fixture, recorder, &mut round, &mut window) {
        std::thread::sleep(bound + MARGIN);
        let now = SystemTime::now();
        round.counted = false;
        round.why_not = NOT_DRIVEN;
        round.ahead = recorder.between(window.ahead_from, window.markers_from.unwrap_or(now));
        round.markers = window
            .markers_from
            .map_or(0, |markers_from| recorder.between(markers_from, now));
        round.not_driven = Some(no_reply);
    }
    round
}

struct Drive {
    how: Move,
    wait_ms: u64,
    bound: Duration,
    keep: Keep,
}

struct Window {
    ahead_from: SystemTime,
    markers_from: Option<SystemTime>,
}

impl Drive {
    fn round(
        &self,
        fixture: &mut Fixture,
        recorder: &impl Records,
        round: &mut Round,
        window: &mut Window,
    ) -> Result<(), NoReply> {
        let Self {
            how,
            wait_ms,
            bound,
            keep,
        } = *self;
        fixture.try_ask("clear", "cleared")?;
        if how == Move::Return {
            fixture.try_focus("secure")?;
            if !state_reads(true, Duration::from_secs(2)) {
                round.why_not = "the secure field did not turn secure input on";
                return Ok(());
            }
            if fixture.try_ask("away", "away=")? != "away=1" {
                round.why_not = "no Finder to yield the front to";
                return Ok(());
            }
            if !until(Duration::from_secs(3), || {
                frontmost_pid() != Some(fixture.pid) && !secure_input_active()
            }) {
                round.why_not = "the fixture kept the front or secure input after it yielded";
                return Ok(());
            }
        } else {
            fixture.try_focus("plain")?;
            if !state_reads(false, Duration::from_secs(2)) {
                round.why_not = "secure input did not read off before the round";
                return Ok(());
            }
            if !fixture.try_in_front()? {
                round.why_not = "the fixture was not in front before the first key";
                return Ok(());
            }
        }
        let centre = if matches!(how, Move::Click | Move::Return) {
            Some(try_secure_field_centre(fixture)?)
        } else {
            None
        };
        let whole = matches!(how, Move::WholePassword | Move::WholePasswordBurst);
        let step = if how == Move::WholePasswordBurst {
            BURST_STEP
        } else {
            letter_step(wait_ms)
        };
        window.ahead_from = SystemTime::now();
        let command = if how == Move::Itself {
            format!("stall {wait_ms} self")
        } else {
            format!("stall {wait_ms}")
        };
        fixture.try_request(&command, "waiting", Duration::from_secs(5))?;
        let letters = codes(&LETTERS);
        match how {
            Move::Tab | Move::LeavingKeyUp => {
                fixture.try_keys(step, &format!("{TAB} {letters}"))?
            }
            Move::WholePassword | Move::WholePasswordBurst => {
                fixture.try_keys(step, &format!("{TAB} {letters} {RETURN}"))?;
            }
            Move::Click | Move::Return => {
                fixture.try_click(&centre.expect("the centre of the secure field"))?;
                fixture.try_keys(step, &letters)?;
            }
            Move::Itself => fixture.try_keys(step, &letters)?,
        }
        fixture.try_reply(
            "woke",
            Duration::from_millis(wait_ms) + Duration::from_secs(5),
        )?;
        let mut no_reply = None;
        let landed = until(Duration::from_secs(3), || match fixture.try_state() {
            Ok(state) => {
                round.secure = state.secure;
                state.secure == LETTERS.len()
                    && (whole || keep == Keep::Autoleave || state.focus == "secure")
                    && (how != Move::Return || state.front)
            }
            Err(failed) => {
                no_reply = Some(failed);
                true
            }
        });
        if let Some(failed) = no_reply {
            return Err(failed);
        }
        round.which = fixture
            .try_ask(&format!("which {LETTER_TEXT}"), "which=")?
            .trim_start_matches("which=")
            .to_owned();
        if !landed {
            std::thread::sleep(bound + MARGIN);
            round.ahead = recorder.between(window.ahead_from, SystemTime::now());
            round.why_not = "the letters did not land in the secure field";
            return Ok(());
        }
        if keep == Keep::Mended && !keep_the_field(fixture, how)? {
            round.never_on = true;
            round.why_not = "never_on";
            std::thread::sleep(bound + MARGIN);
            round.ahead = recorder.between(window.ahead_from, SystemTime::now());
            let _ = fixture.try_focus("plain");
            return Ok(());
        }
        if how == Move::LeavingKeyUp {
            fixture.try_keys(Duration::ZERO, &format!("{RETURN}d"))?;
            fixture.try_reply("left_secure", Duration::from_secs(3))?;
            if !state_reads(false, Duration::from_secs(2)) {
                round.why_not = "secure input did not read off after the leaving key";
                std::thread::sleep(bound + MARGIN);
                round.ahead = recorder.between(window.ahead_from, SystemTime::now());
                return Ok(());
            }
            fixture.try_keys(Duration::ZERO, &format!("{RETURN}u"))?;
        } else if !whole {
            fixture.try_focus("plain")?;
        }
        if !state_reads(false, Duration::from_secs(2)) {
            round.why_not = "secure input did not read off before the markers";
            std::thread::sleep(bound + MARGIN);
            round.ahead = recorder.between(window.ahead_from, SystemTime::now());
            return Ok(());
        }
        let markers_from = SystemTime::now();
        window.markers_from = Some(markers_from);
        round.markers_at = Some(raw_ns());
        fixture.try_keys(Duration::from_millis(30), &codes(&MARKERS))?;
        std::thread::sleep(bound + MARGIN);
        let state = fixture.try_state()?;
        round.plain = state.plain;
        round.ahead = recorder.between(window.ahead_from, markers_from);
        round.markers = recorder.between(markers_from, SystemTime::now());
        if state.plain != MARKERS.len() {
            round.why_not = "the markers did not land in the plain field";
        } else if round.markers < MARKERS.len() {
            round.why_not = "the markers were not recorded";
        } else {
            round.counted = true;
        }
        Ok(())
    }
}

fn started_fixture(name: &str) -> Fixture {
    println!("{name} {}", session_line());
    let (fixture, front) = Fixture::start().unwrap_or_else(|why| panic!("{name}: {why}"));
    FRONT.store(fixture.pid, Ordering::SeqCst);
    println!(
        "{name} fixture={} front_by={}",
        fixture.by,
        front.unwrap_or("none")
    );
    assert!(
        front.is_some(),
        "refused: the fixture's window is not in front, so no key was posted"
    );
    fixture
}

#[derive(Debug, Clone, Copy)]
struct Plan {
    count: usize,
    cap: usize,
    keep: Keep,
}

const AS_IS_ONCE: Plan = Plan {
    count: 1,
    cap: 1,
    keep: Keep::AsIs,
};

fn as_is(count: usize) -> Plan {
    Plan {
        count,
        cap: count,
        keep: Keep::AsIs,
    }
}

fn mended(count: usize, cap: usize) -> Plan {
    Plan {
        count,
        cap,
        keep: Keep::Mended,
    }
}

fn phase_with(
    name: &str,
    how: Move,
    waits: &[(u64, Duration)],
    source: fn(Duration) -> (InputSource, DropCounts),
    plan: Plan,
) -> Vec<Round> {
    let mut fixture = started_fixture(name);
    phase_on(&mut fixture, name, how, waits, source, plan).0
}

fn round_line(
    name: &str,
    number: usize,
    round: &Round,
    bound: Duration,
    keep: Keep,
    how: Move,
) -> String {
    let found = evidence(&round.stamps, &round.entries, since_of(how, &round.stamps));
    let at = |ns: Option<u64>| {
        ns.map_or_else(
            || String::from("-"),
            |ns| relative_us(ns, round.from).to_string(),
        )
    };
    let runs: Vec<String> = found
        .runs
        .iter()
        .map(|run| {
            format!(
                "{}..{}",
                relative_us(run.first_on, round.from),
                relative_us(run.last_on, round.from)
            )
        })
        .collect();
    let run = proof_run(&found);
    let span_on = run.map_or(0, |run| on_in_span(&found, run).len());
    let verdict = round.verdict.map_or_else(
        || String::from("-"),
        |verdict| format!("{verdict:?}").to_lowercase(),
    );
    let case = round
        .case
        .map_or_else(|| String::from("-"), |case| format!("{case:?}"));
    format!(
        "{name} round={number} wait_ms={} bound_ms={} counted={} ahead={} markers={} plain={} \
         secure={} which={} keep={keep:?} never_on={} on_runs_us={} last_off_us={} off_us={} \
         inside_on={} span_on={span_on} on_outside={} inside_off={} first_release_us={} \
         on_drops={} case={case} rv={verdict} not_driven={}",
        round.wait_ms,
        bound.as_millis(),
        u8::from(round.counted),
        round.ahead,
        round.markers,
        round.plain,
        round.secure,
        round.which,
        u8::from(round.never_on),
        if runs.is_empty() {
            String::from("-")
        } else {
            runs.join(",")
        },
        at(run.and_then(|run| run.last_off_before)),
        at(run.and_then(|run| run.first_off_after)),
        on_in_narrow(&found),
        on_outside(&found),
        off_in_narrow(&found),
        at(found.first_release),
        found.on_drops.len(),
        round
            .not_driven
            .as_ref()
            .map_or_else(|| String::from("-"), NoReply::word),
    )
}

fn phase_counts(counted: usize, not_driven: usize) -> String {
    format!("phase_counted={counted} phase_not_driven={not_driven}")
}

fn whole_entries(round: &Round) -> bool {
    round.ahead > 0
        || !round.counted
        || round
            .verdict
            .is_some_and(|verdict| verdict != Verdict::Held)
}

fn print_entries(name: &str, number: usize, round: &Round, how: Move) {
    let found = evidence(&round.stamps, &round.entries, since_of(how, &round.stamps));
    let window = if whole_entries(round) {
        None
    } else {
        proof_run(&found).map(|run| {
            let (start, end) = run.span();
            (
                start.min(run.first_on.saturating_sub(WINDOW_MARGIN)),
                end.saturating_add(WINDOW_MARGIN),
            )
        })
    };
    for line in entry_lines(
        name,
        number,
        round.from,
        &round.stamps,
        &round.entries,
        window,
    ) {
        println!("{line}");
    }
}

fn phase_on(
    fixture: &mut Fixture,
    name: &str,
    how: Move,
    waits: &[(u64, Duration)],
    source: fn(Duration) -> (InputSource, DropCounts),
    plan: Plan,
) -> (Vec<Round>, Vec<DropCounts>) {
    println!(
        "{name} gap_ms={} step_ms={} form={}",
        test_gap().as_millis(),
        test_step().as_millis(),
        if short_form() { "short" } else { "long" }
    );
    let mut rounds = Vec::new();
    let mut running: Option<(Duration, Recorder)> = None;
    let mut all_drops = Vec::new();
    for &(wait_ms, bound) in waits {
        if running.as_ref().map(|(at, _)| *at) != Some(bound) {
            drop(running.take());
            let recorder = Recorder::start(source(bound));
            all_drops.push(recorder.drops.clone());
            running = Some((bound, recorder));
        }
        let recorder = &running.as_ref().expect("a source for the bound").1;
        let mut good = 0;
        let mut run = 0;
        while run < plan.cap && good < plan.count {
            let round = round(fixture, recorder, how, wait_ms, bound, plan.keep);
            let number = rounds.len();
            let counted = rounds.iter().filter(|round: &&Round| round.counted).count()
                + usize::from(round.counted);
            let not_driven = rounds
                .iter()
                .filter(|round: &&Round| round.not_driven.is_some())
                .count()
                + usize::from(round.not_driven.is_some());
            println!(
                "{} {}",
                round_line(name, number, &round, bound, plan.keep, how),
                phase_counts(counted, not_driven)
            );
            println!(
                "{name} drops round={number} {}",
                drops_line(&recorder.drops)
            );
            print_entries(name, number, &round, how);
            run += 1;
            if plan.keep != Keep::Mended || round.verdict == Some(Verdict::Held) {
                good += 1;
            }
            rounds.push(round);
        }
    }
    (rounds, all_drops)
}

fn phase(name: &str, how: Move, waits: &[(u64, Duration)]) -> Vec<Round> {
    phase_with(
        name,
        how,
        waits,
        source_for,
        mended(phase_rounds(), phase_cap()),
    )
}

fn driven_note(round: &Round) -> String {
    round
        .not_driven
        .as_ref()
        .map_or_else(String::new, |no_reply| {
            format!(" (not driven: {})", no_reply.word())
        })
}

fn assert_some_round_driven(name: &str, rounds: &[Round]) {
    let not_driven: Vec<String> = rounds
        .iter()
        .filter_map(|round| {
            round
                .not_driven
                .as_ref()
                .map(|no_reply| format!("wait {} ms: {}", round.wait_ms, no_reply.word()))
        })
        .collect();
    assert!(
        rounds.is_empty() || not_driven.len() < rounds.len(),
        "{name}: every round of the phase was not driven: {}",
        not_driven.join(", ")
    );
}

fn at_wait(rounds: &[Round], wait_ms: u64, verdict: Verdict) -> usize {
    rounds
        .iter()
        .filter(|round| round.wait_ms == wait_ms && round.verdict == Some(verdict))
        .count()
}

fn by_wait_line(name: &str, rounds: &[Round], waits: &[(u64, Duration)]) -> String {
    let of = |verdict: Verdict| {
        waits
            .iter()
            .map(|&(wait_ms, _)| format!("{wait_ms}:{}", at_wait(rounds, wait_ms, verdict)))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{name} by_wait held={} not_driven={}",
        of(Verdict::Held),
        of(Verdict::NotDriven)
    )
}

fn judge_mended(name: &str, rounds: &[Round], waits: &[(u64, Duration)], count: usize) {
    judge_mended_in(name, rounds, waits, count, short_form());
}

fn judge_mended_in(
    name: &str,
    rounds: &[Round],
    waits: &[(u64, Duration)],
    count: usize,
    short: bool,
) {
    assert!(!rounds.is_empty(), "{name}: the phase ran no round");
    println!("{}", by_wait_line(name, rounds, waits));
    let of = |want: fn(Verdict) -> bool| -> Vec<String> {
        rounds
            .iter()
            .filter(|round| round.verdict.is_some_and(want))
            .map(|round| {
                format!(
                    "wait {} ms: ahead {}{}",
                    round.wait_ms,
                    round.ahead,
                    driven_note(round)
                )
            })
            .collect()
    };
    for round in rounds
        .iter()
        .filter(|round| round.verdict == Some(Verdict::LateClick))
    {
        println!(
            "{name} late_click wait_ms={} ahead={}",
            round.wait_ms, round.ahead
        );
    }
    let recorded = of(|verdict| verdict == Verdict::Recorded);
    assert!(
        recorded.is_empty(),
        "{name}: recorded keys beside the markers in rounds that counted or were not driven: {}",
        recorded.join(", ")
    );
    assert_some_round_driven(name, rounds);
    let never_on = of(|verdict| verdict == Verdict::NeverOn);
    assert!(
        never_on.is_empty(),
        "{name}: never_on: the state did not read on within 2 s of the landing: {}",
        never_on.join(", ")
    );
    let gap_not_kept: Vec<String> = rounds
        .iter()
        .filter(|round| {
            matches!(round.verdict, Some(Verdict::NotCounted(_)))
                && round.ahead == 0
                && gap_after_first_arrival(&round.entries)
        })
        .map(|round| format!("wait {} ms", round.wait_ms))
        .collect();
    assert!(
        gap_not_kept.is_empty(),
        "{name}: the runner did not keep the measured gap of {} ms: {}; run the job again once, and a \
         second red goes to the coordinator with the rounds' entries",
        test_gap().as_millis(),
        gap_not_kept.join(", ")
    );
    let not_counted: Vec<String> = rounds
        .iter()
        .filter_map(|round| match round.verdict {
            Some(Verdict::NotCounted(why)) => Some(format!("wait {} ms: {why}", round.wait_ms)),
            _ => None,
        })
        .collect();
    assert!(
        not_counted.is_empty(),
        "{name}: rounds that did not count: {}",
        not_counted.join(", ")
    );
    if short {
        let outside: Vec<String> = rounds
            .iter()
            .filter(|round| round.not_driven.is_none())
            .filter_map(|round| {
                let found = on_outside(&evidence(&round.stamps, &round.entries, 0));
                (found > 0).then(|| format!("wait {} ms: {found}", round.wait_ms))
            })
            .collect();
        assert!(
            outside.is_empty(),
            "{name}: readings of on outside every span of the round: {}",
            outside.join(", ")
        );
    }
    let no_proof = of(|verdict| verdict == Verdict::NoProof);
    assert!(
        no_proof.is_empty(),
        "{name}: counted rounds whose entries show no reading of on inside the on-time and \
         a drop for on: {}",
        no_proof.join(", ")
    );
    let inexact: Vec<String> = rounds
        .iter()
        .filter(|round| round.verdict == Some(Verdict::Held) && round.markers != MARKERS.len())
        .map(|round| format!("wait {} ms: {}", round.wait_ms, round.markers))
        .collect();
    assert!(
        inexact.is_empty(),
        "{name}: records of the markers other than one for each marker: {}",
        inexact.join(", ")
    );
    if short {
        assert!(
            rounds
                .iter()
                .any(|round| round.verdict == Some(Verdict::Held)),
            "{name}: no round of the phase was held: the short form passes a phase with one held \
             round and no failed round"
        );
        return;
    }
    let not_driven_waits: Vec<u64> = waits
        .iter()
        .map(|&(wait_ms, _)| wait_ms)
        .filter(|&wait_ms| at_wait(rounds, wait_ms, Verdict::Held) == 0)
        .collect();
    if !not_driven_waits.is_empty() {
        println!(
            "{name} not_driven_waits={}",
            not_driven_waits
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    assert!(
        not_driven_waits.is_empty(),
        "{name}: not driven at {}: in the long form every wait needs a held round, and the long \
         form takes no record from this run",
        not_driven_waits
            .iter()
            .map(|wait_ms| format!("wait {wait_ms} ms"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let short_of_count: Vec<String> = waits
        .iter()
        .filter_map(|&(wait_ms, _)| {
            let held = at_wait(rounds, wait_ms, Verdict::Held);
            let not_driven = at_wait(rounds, wait_ms, Verdict::NotDriven);
            (held + not_driven < count)
                .then(|| format!("wait {wait_ms} ms: {held} of {count}, {not_driven} not driven"))
        })
        .collect();
    assert!(
        short_of_count.is_empty(),
        "{name}: rounds that did not count by the cap of rounds run: {}",
        short_of_count.join(", ")
    );
}

fn measured(name: &str, form: &str, rounds: &[Round]) {
    let count = |case: Case| {
        rounds
            .iter()
            .filter(|round| round.case == Some(case))
            .count()
    };
    let recorded: Vec<&Round> = rounds
        .iter()
        .filter(|round| round.counted && round.ahead > 0)
        .collect();
    let uncounted_with_keys = rounds
        .iter()
        .filter(|round| !round.counted && round.ahead > 0)
        .count();
    let recorded_case = |case: Case| {
        recorded
            .iter()
            .filter(|round| round.case == Some(case))
            .count()
    };
    println!(
        "{name} measured form={form} wait_ms={} rounds={} counted={} recorded_rounds={} \
         recorded_cases=A:{},B:{},between:{},C:{},D:{},seen:{} \
         uncounted_with_keys={uncounted_with_keys} cases=A:{},B:{},between:{},C:{},D:{},seen:{}",
        rounds.first().map_or(0, |round| round.wait_ms),
        rounds.len(),
        rounds.iter().filter(|round| round.counted).count(),
        recorded.len(),
        recorded_case(Case::A),
        recorded_case(Case::B),
        recorded_case(Case::Between),
        recorded_case(Case::C),
        recorded_case(Case::D),
        recorded_case(Case::Seen),
        count(Case::A),
        count(Case::B),
        count(Case::Between),
        count(Case::C),
        count(Case::D),
        count(Case::Seen),
    );
}

fn judge(name: &str, rounds: &[Round]) {
    let leaked: Vec<String> = rounds
        .iter()
        .filter(|round| {
            round.counted && round.ahead > 0 || round.verdict == Some(Verdict::Recorded)
        })
        .map(|round| {
            format!(
                "wait {} ms: {}{}",
                round.wait_ms,
                round.ahead,
                driven_note(round)
            )
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "{name}: recorded keys beside the markers in rounds that counted or were not driven: {}",
        leaked.join(", ")
    );
    assert_some_round_driven(name, rounds);
    let not_counted: Vec<String> = rounds
        .iter()
        .filter(|round| !round.counted && round.not_driven.is_none())
        .map(|round| format!("wait {} ms: {}", round.wait_ms, round.why_not))
        .collect();
    assert!(
        not_counted.is_empty(),
        "{name}: rounds that did not count: {}",
        not_counted.join(", ")
    );
    let inexact: Vec<String> = rounds
        .iter()
        .filter(|round| round.counted && round.markers != MARKERS.len())
        .map(|round| format!("wait {} ms: {}", round.wait_ms, round.markers))
        .collect();
    assert!(
        inexact.is_empty(),
        "{name}: records of the markers other than one for each marker: {}",
        inexact.join(", ")
    );
}

fn stall_waits() -> Vec<(u64, Duration)> {
    vec![
        (150, SHORT_BOUND),
        (300, SHORT_BOUND),
        (600, SHORT_BOUND),
        (JUST_UNDER, SHORT_BOUND),
        (2_000, LONG_BOUND),
    ]
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_ahead_into_a_secure_field_is_not_recorded() {
    let name = "a_key_typed_ahead_into_a_secure_field_is_not_recorded";
    let waits = stall_waits();
    judge_mended(
        name,
        &phase(name, Move::Tab, &waits),
        &waits,
        phase_rounds(),
    );
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn known_fault_a_key_typed_ahead_of_a_stall_longer_than_the_bound_is_not_recorded() {
    let name = "known_fault_a_key_typed_ahead_of_a_stall_longer_than_the_bound_is_not_recorded";
    judge(
        name,
        &phase_with(
            name,
            Move::Tab,
            &[(1_500, SHORT_BOUND)],
            source_for,
            as_is(fault_rounds()),
        ),
    );
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn measured_a_password_typed_whole_ahead_at_machine_speed() {
    let name = "measured_a_password_typed_whole_ahead_at_machine_speed";
    let rounds = phase_with(
        name,
        Move::WholePasswordBurst,
        &[(300, SHORT_BOUND)],
        source_for,
        as_is(FAULT_ROUNDS),
    );
    measured(name, "burst_2ms", &rounds);
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn measured_a_password_typed_whole_ahead_at_a_persons_pace() {
    let name = "measured_a_password_typed_whole_ahead_at_a_persons_pace";
    let rounds = phase_with(
        name,
        Move::WholePassword,
        &[(300, SHORT_BOUND)],
        source_for,
        as_is(FAULT_ROUNDS),
    );
    measured(name, "pace_26ms", &rounds);
}

fn short_on_time_rounds() -> usize {
    if short_form() {
        3
    } else {
        20
    }
}

fn short_on_time_product_rounds() -> usize {
    if short_form() {
        1
    } else {
        3
    }
}

fn noticed_drops(round: &Round) -> usize {
    round
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry,
                RawEntry::Dropped {
                    cause: Cause::Notice,
                    ..
                }
            )
        })
        .count()
}

fn on_time_ns(round: &Round) -> Option<u64> {
    let found = evidence(&round.stamps, &round.entries, 0);
    proof_run(&found).map(|run| {
        let (start, end) = run.span();
        end.saturating_sub(start)
    })
}

fn judge_short_on_time(name: &str, rounds: &[Round], spacing: Option<Duration>) {
    let shorter = spacing.map_or(0, |spacing| {
        let spacing = u64::try_from(spacing.as_nanos()).unwrap_or(u64::MAX);
        rounds
            .iter()
            .filter(|round| round.counted && on_time_ns(round).is_some_and(|ns| ns < spacing))
            .count()
    });
    let on_us: Vec<String> = rounds
        .iter()
        .map(|round| {
            on_time_ns(round)
                .map_or(-1, |ns| (ns / 1_000) as i64)
                .to_string()
        })
        .collect();
    println!(
        "{name} short_on_time rounds={} counted={} spacing_ms={} shorter_than_spacing={shorter} \
         notice_drops={} on_us={}",
        rounds.len(),
        rounds.iter().filter(|round| round.counted).count(),
        spacing.map_or(0, |spacing| spacing.as_millis()),
        rounds.iter().map(noticed_drops).sum::<usize>(),
        on_us.join(",")
    );
    judge(name, rounds);
    if spacing.is_some() {
        assert!(
            shorter > 0,
            "{name}: no counted round had its on-time shorter than the spacing of two readings"
        );
    }
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_password_typed_whole_ahead_into_a_field_secure_for_less_than_the_spacing_of_two_readings_is_not_recorded(
) {
    let name = "a_password_typed_whole_ahead_into_a_field_secure_for_less_than_the_spacing_of_two_readings_is_not_recorded";
    let rounds = phase_with(
        name,
        Move::WholePasswordBurst,
        &[(300, SHORT_BOUND)],
        noticed_sparse_source,
        as_is(short_on_time_rounds()),
    );
    judge_short_on_time(name, &rounds, Some(SPARSE_STEP));
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn at_the_products_constants_a_password_typed_whole_ahead_is_not_recorded() {
    let name = "at_the_products_constants_a_password_typed_whole_ahead_is_not_recorded";
    let rounds = phase_with(
        name,
        Move::WholePasswordBurst,
        &[(300, HOLD_BOUND)],
        noticed_product_source,
        as_is(short_on_time_product_rounds()),
    );
    judge_short_on_time(name, &rounds, None);
}

const LAG_DELAY_MS: u64 = 150;
const LEAVE_AFTER_MS: u64 = 30;

fn lag_rounds() -> usize {
    if short_form() {
        2
    } else {
        10
    }
}

#[derive(Debug, Default)]
struct LagRound {
    counted: bool,
    why_not: &'static str,
    ahead: usize,
    markers: usize,
    plain: usize,
    secure: usize,
    late: u64,
    not_driven: Option<NoReply>,
}

fn lag_round_inner(
    fixture: &mut Fixture,
    recorder: &Recorder,
    bound: Duration,
    round: &mut LagRound,
    ahead_from: &mut SystemTime,
) -> Result<(), NoReply> {
    let wait_out = |round: &mut LagRound, from: SystemTime| {
        std::thread::sleep(bound + MARGIN);
        round.ahead = recorder.between(from, SystemTime::now());
    };
    fixture.try_ask("clear", "cleared")?;
    fixture.try_focus("plain")?;
    if !state_reads(false, NEVER_ON_WAIT) {
        round.why_not = "secure input did not read off before the round";
        return Ok(());
    }
    if !fixture.try_in_front()? {
        round.why_not = "the fixture was not in front before the first key";
        return Ok(());
    }
    fixture.try_focus("secure")?;
    if !state_reads(true, NEVER_ON_WAIT) {
        round.why_not = "the secure field did not turn secure input on";
        return Ok(());
    }
    std::thread::sleep(SESSION_STEP + Duration::from_millis(100));
    *ahead_from = SystemTime::now();
    SESSION_DELAY_MS.store(LAG_DELAY_MS, Ordering::SeqCst);
    fixture.try_ask(&format!("leave {LEAVE_AFTER_MS}"), "leaving")?;
    fixture.try_keys(Duration::ZERO, &format!("{}d", LETTERS[0]))?;
    let left = state_reads(false, NEVER_ON_WAIT);
    std::thread::sleep(Duration::from_millis(LAG_DELAY_MS + 150));
    fixture.try_keys(Duration::ZERO, &format!("{}u", LETTERS[0]))?;
    SESSION_DELAY_MS.store(0, Ordering::SeqCst);
    let state = fixture.try_state()?;
    round.secure = state.secure;
    if !left {
        round.why_not = "secure input did not read off after the leave";
        wait_out(round, *ahead_from);
        return Ok(());
    }
    if state.secure != 1 || state.focus != "plain" {
        round.why_not = "the letter did not land in the secure field before the leave";
        wait_out(round, *ahead_from);
        return Ok(());
    }
    let markers_from = SystemTime::now();
    fixture.try_keys(Duration::from_millis(30), &codes(&MARKERS))?;
    std::thread::sleep(bound + MARGIN);
    let state = fixture.try_state()?;
    round.plain = state.plain;
    round.ahead = recorder.between(*ahead_from, markers_from);
    round.markers = recorder.between(markers_from, SystemTime::now());
    if state.plain != MARKERS.len() {
        round.why_not = "the markers did not land in the plain field";
    } else if round.markers < MARKERS.len() {
        round.why_not = "the markers were not recorded";
    } else {
        round.counted = true;
    }
    Ok(())
}

fn lag_round(fixture: &mut Fixture, recorder: &Recorder, bound: Duration) -> LagRound {
    let late_before = recorder.drops.of("late");
    let _ = recorder.take_entries();
    let mut round = LagRound::default();
    let mut ahead_from = SystemTime::now();
    if let Err(no_reply) = lag_round_inner(fixture, recorder, bound, &mut round, &mut ahead_from) {
        SESSION_DELAY_MS.store(0, Ordering::SeqCst);
        std::thread::sleep(bound + MARGIN);
        round.counted = false;
        round.why_not = NOT_DRIVEN;
        round.ahead = recorder.between(ahead_from, SystemTime::now());
        round.not_driven = Some(no_reply);
    }
    round.late = recorder.drops.of("late") - late_before;
    round
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_whose_own_reading_ends_after_the_lag_bound_is_not_recorded() {
    let name = "a_key_whose_own_reading_ends_after_the_lag_bound_is_not_recorded";
    let mut fixture = started_fixture(name);
    println!(
        "{name} gap_ms={} step_ms={} session_delay_ms={LAG_DELAY_MS} leave_after_ms={LEAVE_AFTER_MS} form={}",
        test_gap().as_millis(),
        test_step().as_millis(),
        if short_form() { "short" } else { "long" }
    );
    let recorder = Recorder::start(delayed_session_source(SHORT_BOUND));
    let mut rounds = Vec::new();
    for number in 0..lag_rounds() {
        let round = lag_round(&mut fixture, &recorder, SHORT_BOUND);
        println!(
            "{name} round={number} wait_ms=0 counted={} ahead={} markers={} plain={} secure={} \
             late={} not_driven={}",
            u8::from(round.counted),
            round.ahead,
            round.markers,
            round.plain,
            round.secure,
            round.late,
            round
                .not_driven
                .as_ref()
                .map_or_else(|| String::from("-"), NoReply::word)
        );
        println!(
            "{name} drops round={number} {}",
            drops_line(&recorder.drops)
        );
        rounds.push(round);
    }
    let leaked: Vec<usize> = rounds
        .iter()
        .enumerate()
        .filter(|(_, round)| round.ahead > 0 && (round.counted || round.secure > 0))
        .map(|(number, _)| number)
        .collect();
    assert!(
        leaked.is_empty(),
        "{name}: recorded keys beside the markers in rounds that counted or were not driven: rounds {leaked:?}"
    );
    let not_counted: Vec<&str> = rounds
        .iter()
        .filter(|round| !round.counted && round.not_driven.is_none())
        .map(|round| round.why_not)
        .collect();
    assert!(
        not_counted.is_empty(),
        "{name}: rounds that did not count: {}",
        not_counted.join(", ")
    );
    assert!(
        rounds.iter().any(|round| round.counted),
        "{name}: every round of the phase was not driven"
    );
    assert!(
        rounds
            .iter()
            .filter(|round| round.counted)
            .all(|round| round.late > 0),
        "{name}: a counted round withheld no key as late, so it did not show the lag rule"
    );
}

fn trial_round(fixture: &mut Fixture, form: &str) -> Result<(), NoReply> {
    fixture.try_ask("clear", "cleared")?;
    fixture.try_focus("plain")?;
    state_reads(false, NEVER_ON_WAIT);
    match form {
        "blink" => {
            fixture.try_ask("blink", "blinked")?;
        }
        "autoleave_0" => {
            fixture.try_ask("autoleave 0", "autoleave=")?;
            fixture.try_focus("secure")?;
            std::thread::sleep(Duration::from_millis(200));
            fixture.try_ask("autoleave -1", "autoleave=")?;
        }
        "burst" => {
            fixture.try_request("stall 300", "waiting", Duration::from_secs(5))?;
            fixture.try_keys(BURST_STEP, &format!("{TAB} {} {RETURN}", codes(&LETTERS)))?;
            fixture.try_reply("woke", Duration::from_secs(5))?;
        }
        "stay" => {
            fixture.try_focus("secure")?;
            std::thread::sleep(Duration::from_millis(300));
            fixture.try_focus("plain")?;
        }
        _ => {
            fixture.try_keys(Duration::from_millis(20), &codes(&LETTERS[..3]))?;
        }
    }
    std::thread::sleep(Duration::from_millis(600));
    Ok(())
}

fn percentile(sorted: &[u64], part: usize) -> i64 {
    if sorted.is_empty() {
        return -1;
    }
    let at = (sorted.len() * part / 100).min(sorted.len() - 1);
    (sorted[at] / 1_000) as i64
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn the_trial_of_the_focus_notice_reports_its_reach_and_lateness() {
    let name = "the_trial_of_the_focus_notice_reports_its_reach_and_lateness";
    let mut fixture = started_fixture(name);
    let recorder = Recorder::start(noticed_sparse_source(SHORT_BOUND));
    std::thread::sleep(Duration::from_millis(500));
    let start: Vec<String> = recorder
        .take_entries()
        .iter()
        .filter_map(|entry| match *entry {
            RawEntry::Notice { seen, focus, .. } => {
                Some(format!("{}:{}", seen_word(seen), focus_word(focus)))
            }
            _ => None,
        })
        .collect();
    println!("{name} watch_start={}", start.join(","));
    for (form, count) in [
        ("blink", 20),
        ("autoleave_0", 10),
        ("burst", 10),
        ("stay", 5),
        ("plain", 5),
    ] {
        let mut lateness = Vec::new();
        let (mut reached, mut focused, mut driven) = (0, 0, 0);
        for number in 0..count {
            let _ = recorder.take_entries();
            let from = raw_ns();
            let outcome = trial_round(&mut fixture, form);
            let stamps = fixture.try_stamps().unwrap_or_default();
            let entries = recorder.take_entries();
            let focus_in = stamps
                .iter()
                .find(|stamp| stamp.kind == "fr_secure" && stamp.detail == 1 && stamp.ns >= from)
                .map(|stamp| stamp.ns);
            let runs: Vec<String> = on_runs(&stamps)
                .iter()
                .map(|(first, last)| ((last - first) / 1_000).to_string())
                .collect();
            let notices: Vec<(u64, NoticeSeen, Focus)> = entries
                .iter()
                .filter_map(|entry| match *entry {
                    RawEntry::Notice { at, seen, focus } if at >= from => Some((at, seen, focus)),
                    _ => None,
                })
                .collect();
            let count_of = |want: Focus| {
                notices
                    .iter()
                    .filter(|(_, _, focus)| *focus == want)
                    .count()
            };
            let first_mark = notices
                .iter()
                .find(|(_, _, focus)| focus.marks())
                .map(|(at, _, _)| *at);
            let late = focus_in
                .zip(first_mark)
                .map(|(focus_in, at)| at.saturating_sub(focus_in));
            let notice_drop = entries
                .iter()
                .filter(|entry| {
                    matches!(
                        entry,
                        RawEntry::Dropped {
                            cause: Cause::Notice,
                            ..
                        }
                    )
                })
                .count();
            driven += usize::from(outcome.is_ok());
            if focus_in.is_some() {
                focused += 1;
                if first_mark.is_some() {
                    reached += 1;
                }
            }
            lateness.extend(late);
            println!(
                "{name} form={form} round={number} driven={} focus_in={} on_runs_us={} notices={} \
                 secure={} doubt={} other={} late_us={} notice_drops={notice_drop}",
                u8::from(outcome.is_ok()),
                u8::from(focus_in.is_some()),
                if runs.is_empty() {
                    String::from("-")
                } else {
                    runs.join(",")
                },
                notices.len(),
                count_of(Focus::Secure),
                count_of(Focus::Doubt),
                count_of(Focus::Other),
                late.map_or(-1, |ns| (ns / 1_000) as i64)
            );
        }
        lateness.sort_unstable();
        println!(
            "{name} summary form={form} rounds={count} driven={driven} focused={focused} \
             reached={reached} late_p50_us={} late_max_us={}",
            percentile(&lateness, 50),
            percentile(&lateness, 100)
        );
    }
    drop(recorder);
    drop(fixture);
    let masked = "<!doctype html><title>style mask</title>\
         <input id=m type=text style=\"-webkit-text-security:disc\" autofocus>\
         <script>addEventListener('load', () => document.getElementById('m').focus())</script>";
    style_probe("safari", &["-a", "Safari"], "com.apple.Safari", masked);
    let profile = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("stall-trial-chrome");
    let profile_arg = format!("--user-data-dir={}", profile.display());
    style_probe(
        "chrome",
        &[
            "-na",
            "Google Chrome",
            "--args",
            "--no-first-run",
            "--no-default-browser-check",
            &profile_arg,
        ],
        "com.google.Chrome",
        masked,
    );
    println!("{name} done");
}

fn style_probe(name: &str, open: &[&str], bundle: &str, body: &str) {
    let page = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("stall-trial-style.html");
    std::fs::write(&page, body).expect("the page is written");
    let opened = Command::new("/usr/bin/open")
        .args(open)
        .arg(&page)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let front = until(Duration::from_secs(30), || {
        frontmost_bundle().as_deref() == Some(bundle)
    });
    std::thread::sleep(Duration::from_secs(3));
    let mut on = 0;
    let reads = 50;
    for _ in 0..reads {
        on += u32::from(secure_input_active());
        std::thread::sleep(Duration::from_millis(20));
    }
    println!(
        "probe style_mask page={name} opened={} front={} reads={reads} secure_on_reads={on}",
        u8::from(opened),
        u8::from(front)
    );
    if let Some(app) = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .filter(|app| app.bundleIdentifier().map(|id| id.to_string()).as_deref() == Some(bundle))
    {
        app.forceTerminate();
    }
    state_reads(false, Duration::from_secs(10));
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn short_window_a_the_click_round_as_it_was_at_wait_150_ms() {
    let name = "short_window_a_the_click_round_as_it_was_at_wait_150_ms";
    let rounds = phase_with(
        name,
        Move::Click,
        &[(150, SHORT_BOUND)],
        source_for,
        as_is(SHORT_WINDOW_AT_150),
    );
    measured(name, "a", &rounds);
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn short_window_a_the_click_round_as_it_was_at_wait_2000_ms() {
    let name = "short_window_a_the_click_round_as_it_was_at_wait_2000_ms";
    let rounds = phase_with(
        name,
        Move::Click,
        &[(2_000, LONG_BOUND)],
        source_for,
        as_is(SHORT_WINDOW_AT_2000),
    );
    measured(name, "a", &rounds);
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn short_window_b_the_leave_asked_after_n_ms() {
    let name = "short_window_b_the_leave_asked_after_n_ms";
    let mut fixture = started_fixture(name);
    for asked_ms in LEAVES_ASKED_AFTER_MS {
        fixture.ask(&format!("autoleave {asked_ms}"), "autoleave=");
        let (rounds, _) = phase_on(
            &mut fixture,
            name,
            Move::Click,
            &[(150, SHORT_BOUND)],
            source_for,
            Plan {
                count: LEAVE_ROUNDS,
                cap: LEAVE_ROUNDS,
                keep: Keep::Autoleave,
            },
        );
        measured(name, &format!("leave_asked_after_{asked_ms}ms"), &rounds);
    }
    fixture.ask("autoleave -1", "autoleave=");
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn the_mended_click_phase_counts_62_rounds_at_wait_2000_ms() {
    let name = "the_mended_click_phase_counts_62_rounds_at_wait_2000_ms";
    let waits = [(2_000, LONG_BOUND)];
    let rounds = phase_with(
        name,
        Move::Click,
        &waits,
        source_for,
        mended(LOOP_COUNTED, LOOP_CAP),
    );
    judge_mended(name, &rounds, &waits, LOOP_COUNTED);
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_ahead_of_a_click_into_a_secure_field_is_not_recorded() {
    let name = "a_key_typed_ahead_of_a_click_into_a_secure_field_is_not_recorded";
    let waits = stall_waits();
    judge_mended(
        name,
        &phase(name, Move::Click, &waits),
        &waits,
        phase_rounds(),
    );
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded() {
    let name = "a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded";
    let waits = stall_waits();
    judge_mended(
        name,
        &phase(name, Move::Itself, &waits),
        &waits,
        phase_rounds(),
    );
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_on_a_return_to_a_secure_field_is_not_recorded() {
    let name = "a_key_typed_on_a_return_to_a_secure_field_is_not_recorded";
    judge_three_ways(
        name,
        &phase_with(
            name,
            Move::Return,
            &stall_waits(),
            source_for,
            mended(RETURN_ROUNDS_FOR_EACH_WAIT, RETURN_ROUNDS_FOR_EACH_WAIT),
        ),
    );
}

fn three_way_recorded(round: &Round) -> bool {
    round.ahead > 0 && (round.counted || round.not_driven.is_some() || round.secure > 0)
        || round.verdict == Some(Verdict::Recorded)
}

fn three_way_verdict(rounds: &[Round]) -> (&'static str, usize, usize, usize) {
    let failed = rounds
        .iter()
        .filter(|round| three_way_recorded(round))
        .count();
    let held = rounds
        .iter()
        .filter(|round| round.counted && !three_way_recorded(round))
        .count();
    let not_driven = rounds.len() - failed - held;
    let every_round_not_driven = rounds.iter().all(|round| round.not_driven.is_some());
    let verdict = match (rounds.len(), failed, not_driven) {
        (0, _, _) | (_, 1.., _) => "failed",
        _ if every_round_not_driven => "failed",
        (_, 0, 0) => "held",
        (_, 0, _) => "notdriven",
    };
    (verdict, held, failed, not_driven)
}

fn judge_three_ways(name: &str, rounds: &[Round]) {
    let (verdict, held, failed, not_driven) = three_way_verdict(rounds);
    for round in rounds
        .iter()
        .filter(|round| !round.counted && !three_way_recorded(round))
    {
        println!(
            "{name} notdriven wait_ms={} why={}",
            round.wait_ms,
            round.why_not.replace(' ', "_")
        );
    }
    println!("{name} verdict={verdict} held={held} failed={failed} notdriven={not_driven}");
    let inexact: Vec<String> = rounds
        .iter()
        .filter(|round| round.counted && round.markers != MARKERS.len())
        .map(|round| format!("wait {} ms: {}", round.wait_ms, round.markers))
        .collect();
    assert!(!rounds.is_empty(), "{name}: the phase ran no round");
    assert!(
        failed == 0,
        "{name}: a key was recorded in {failed} rounds, counted or not: a key ahead of the markers \
         in a counted round or in a round not driven, a key ahead with a letter landed in the \
         secure field, or a round judged recorded"
    );
    assert_some_round_driven(name, rounds);
    assert!(
        inexact.is_empty(),
        "{name}: records of the markers other than one for each marker: {}",
        inexact.join(", ")
    );
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn the_up_of_the_key_that_leaves_a_secure_field_is_not_recorded() {
    let name = "the_up_of_the_key_that_leaves_a_secure_field_is_not_recorded";
    let waits = stall_waits();
    judge_mended(
        name,
        &phase(name, Move::LeavingKeyUp, &waits),
        &waits,
        phase_rounds(),
    );
}

#[derive(Debug)]
struct Control {
    counted: bool,
    why_not: &'static str,
    landed_plain: usize,
    landed_secure: usize,
    plain: usize,
    secure: usize,
    after: usize,
    not_driven: Option<NoReply>,
}

#[derive(Default)]
struct ControlWindow {
    plain_from: Option<SystemTime>,
    secure_from: Option<SystemTime>,
    after_from: Option<SystemTime>,
}

fn control_round(fixture: &mut Fixture, recorder: &impl Records, bound: Duration) -> Control {
    let mut control = Control {
        counted: false,
        why_not: "",
        landed_plain: 0,
        landed_secure: 0,
        plain: 0,
        secure: 0,
        after: 0,
        not_driven: None,
    };
    let mut window = ControlWindow::default();
    if let Err(no_reply) = drive_control(fixture, recorder, bound, &mut control, &mut window) {
        std::thread::sleep(bound + MARGIN);
        let now = SystemTime::now();
        let between = |from: Option<SystemTime>, to: Option<SystemTime>| {
            from.map_or(0, |from| recorder.between(from, to.unwrap_or(now)))
        };
        control.counted = false;
        control.why_not = NOT_DRIVEN;
        control.plain = between(window.plain_from, window.secure_from);
        control.secure = between(window.secure_from, window.after_from);
        control.after = between(window.after_from, None);
        control.not_driven = Some(no_reply);
    }
    control
}

fn drive_control(
    fixture: &mut Fixture,
    recorder: &impl Records,
    bound: Duration,
    control: &mut Control,
    window: &mut ControlWindow,
) -> Result<(), NoReply> {
    fixture.try_ask("clear", "cleared")?;
    fixture.try_focus("plain")?;
    if !state_reads(false, Duration::from_secs(2)) || !fixture.try_in_front()? {
        control.why_not = "the fixture was not in front with secure input off";
        return Ok(());
    }
    let plain_from = SystemTime::now();
    window.plain_from = Some(plain_from);
    fixture.try_keys(Duration::from_millis(30), &codes(&LETTERS[..5]))?;
    std::thread::sleep(bound + MARGIN);
    let secure_from = SystemTime::now();
    window.secure_from = Some(secure_from);
    control.plain = recorder.between(plain_from, secure_from);
    fixture.try_focus("secure")?;
    if !state_reads(true, Duration::from_secs(2)) {
        control.why_not = "the secure field did not turn secure input on";
        return Ok(());
    }
    fixture.try_keys(Duration::from_millis(30), &codes(&LETTERS[5..]))?;
    fixture.try_focus("plain")?;
    if !state_reads(false, Duration::from_secs(2)) {
        control.why_not = "secure input did not read off after the secure field";
        std::thread::sleep(bound + MARGIN);
        control.secure = recorder.between(secure_from, SystemTime::now());
        return Ok(());
    }
    let after_from = SystemTime::now();
    window.after_from = Some(after_from);
    fixture.try_keys(Duration::from_millis(30), &codes(&LETTERS[..3]))?;
    std::thread::sleep(bound + MARGIN);
    control.secure = recorder.between(secure_from, after_from);
    control.after = recorder.between(after_from, SystemTime::now());
    let state = fixture.try_state()?;
    control.landed_plain = state.plain;
    control.landed_secure = state.secure;
    if state.plain != 8 || state.secure != 3 {
        control.why_not = "the letters did not land where they were typed";
        return Ok(());
    }
    control.counted = true;
    Ok(())
}

fn control_verdict(control: &Control) -> &'static str {
    match (&control.not_driven, control.secure) {
        (None, _) => "-",
        (Some(_), 0) => "notdriven",
        (Some(_), _) => "recorded",
    }
}

fn control_rounds_on(
    fixture: &mut Fixture,
    name: &str,
    bound: Duration,
    source: (InputSource, DropCounts),
    count: usize,
) -> (Vec<Control>, DropCounts) {
    let recorder = Recorder::start(source);
    let mut rounds = Vec::new();
    for number in 0..count {
        let control = control_round(fixture, &recorder, bound);
        rounds.push(control);
        let control = &rounds[number];
        println!(
            "{name} control={number} bound_ms={} counted={} landed_plain={} landed_secure={} \
             plain_records={} secure_records={} after_records={} rv={} not_driven={} {}",
            bound.as_millis(),
            u8::from(control.counted),
            control.landed_plain,
            control.landed_secure,
            control.plain,
            control.secure,
            control.after,
            control_verdict(control),
            control
                .not_driven
                .as_ref()
                .map_or_else(|| String::from("-"), NoReply::word),
            phase_counts(
                rounds.iter().filter(|control| control.counted).count(),
                rounds
                    .iter()
                    .filter(|control| control.not_driven.is_some())
                    .count()
            )
        );
        println!(
            "{name} drops control={number} {}",
            drops_line(&recorder.drops)
        );
    }
    let drops = recorder.drops.clone();
    (rounds, drops)
}

fn control_phase(name: &str, bound: Duration, source: (InputSource, DropCounts), count: usize) {
    let mut fixture = started_fixture(name);
    let (rounds, _) = control_rounds_on(&mut fixture, name, bound, source, count);
    judge_control(name, &rounds);
}

fn control_note(control: &Control) -> String {
    control
        .not_driven
        .as_ref()
        .map_or_else(String::new, |no_reply| {
            format!(" (not driven: {})", no_reply.word())
        })
}

fn judge_control(name: &str, rounds: &[Control]) {
    assert!(!rounds.is_empty(), "{name}: the control ran no round");
    let recorded: Vec<String> = rounds
        .iter()
        .filter(|control| control_verdict(control) == "recorded")
        .map(|control| format!("{}{}", control.secure, control_note(control)))
        .collect();
    assert!(
        recorded.is_empty(),
        "{name}: records of the secure field in control rounds not driven: {}",
        recorded.join(", ")
    );
    let not_driven: Vec<String> = rounds.iter().map(control_note).collect();
    assert!(
        not_driven.iter().any(String::is_empty),
        "{name}: every control round was not driven:{}",
        not_driven.join(",")
    );
    let not_counted: Vec<&str> = rounds
        .iter()
        .filter(|control| !control.counted && control.not_driven.is_none())
        .map(|control| control.why_not)
        .collect();
    assert!(
        not_counted.is_empty(),
        "{name}: control rounds that did not count: {}",
        not_counted.join(", ")
    );
    for control in rounds.iter().filter(|control| control.not_driven.is_none()) {
        assert_eq!(
            (control.plain, control.secure, control.after),
            (5, 0, 3),
            "{name}: records of five letters in the plain field, three in the secure field and \
             three after it"
        );
    }
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn the_control_records_the_keys_typed_in_the_plain_field_and_none_of_the_secure_field() {
    let name = "the_control_records_the_keys_typed_in_the_plain_field_and_none_of_the_secure_field";
    control_phase(name, SHORT_BOUND, source_for(SHORT_BOUND), control_rounds());
}

trait Line {
    type Fixture;
    fn start(&mut self, name: &str) -> Self::Fixture;
    fn phase(&mut self, name: &str, fixture: &mut Self::Fixture) -> (Vec<Round>, Vec<DropCounts>);
    fn control(&mut self, name: &str, fixture: &mut Self::Fixture) -> (Vec<Control>, DropCounts);
}

struct HostedLine;

impl Line for HostedLine {
    type Fixture = Fixture;

    fn start(&mut self, name: &str) -> Fixture {
        started_fixture(name)
    }

    fn phase(&mut self, name: &str, fixture: &mut Fixture) -> (Vec<Round>, Vec<DropCounts>) {
        phase_on(
            fixture,
            name,
            Move::Tab,
            &[(300, HOLD_BOUND)],
            product_source,
            AS_IS_ONCE,
        )
    }

    fn control(&mut self, name: &str, fixture: &mut Fixture) -> (Vec<Control>, DropCounts) {
        control_rounds_on(fixture, name, HOLD_BOUND, product_source(HOLD_BOUND), 1)
    }
}

fn gap_after_first_arrival(entries: &[RawEntry]) -> bool {
    let Some(first) = entries.iter().find_map(|entry| match *entry {
        RawEntry::Arrival { at, .. } => Some(at),
        _ => None,
    }) else {
        return false;
    };
    entries.iter().any(|entry| {
        matches!(*entry, RawEntry::Dropped { at, cause: Cause::Gap, keys, .. } if at >= first && keys > 0)
    })
}

fn product_verdict(rounds: &[Round], controls: &[Control], control_gap_drops: u64) -> &'static str {
    let failed = rounds
        .iter()
        .any(|round| round.ahead > 0 && round.secure > 0)
        || controls.iter().any(|control| control.secure > 0);
    let exact = |control: &Control| control.counted && (control.plain, control.after) == (5, 3);
    let held = rounds
        .iter()
        .all(|round| round.counted && round.markers == MARKERS.len())
        && controls.iter().all(exact);
    let rounds_not_kept = rounds.iter().all(|round| {
        if round.counted {
            return round.markers == MARKERS.len();
        }
        round.why_not == "the markers were not recorded"
            && round.ahead == 0
            && gap_after_first_arrival(&round.entries)
    });
    let controls_not_kept = controls
        .iter()
        .all(|control| exact(control) || control_gap_drops > 0);
    if failed || rounds.is_empty() {
        "failed"
    } else if held {
        "held"
    } else if rounds_not_kept && controls_not_kept {
        "notkept"
    } else {
        "failed"
    }
}

fn product_line(name: &str, line: &mut impl Line) -> &'static str {
    let mut fixture = line.start(name);
    let (rounds, phase_drops) = line.phase(name, &mut fixture);
    let (controls, drops) = line.control(name, &mut fixture);
    let gap_drops = drops.of("gap") + phase_drops.iter().map(|d| d.of("gap")).sum::<u64>();
    let widest = phase_drops
        .iter()
        .map(DropCounts::widest_gap)
        .chain([drops.widest_gap()])
        .max()
        .unwrap_or_default();
    let verdict = product_verdict(&rounds, &controls, drops.of("gap"));
    println!(
        "{name} verdict={verdict} widest_gap_us={} gap_drops={gap_drops}",
        widest.as_micros()
    );
    verdict
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn at_the_bound_of_the_product_a_stall_records_nothing_and_the_control_records_its_keys() {
    let name =
        "at_the_bound_of_the_product_a_stall_records_nothing_and_the_control_records_its_keys";
    let verdict = product_line(name, &mut HostedLine);
    assert!(
        verdict != "failed",
        "{name}: at the product's constants a key was recorded that the rule forbids, or a round \
         did not count for another cause than a gap"
    );
}

fn stand_in_round(counted: bool, ahead: usize) -> Round {
    Round {
        wait_ms: 300,
        counted,
        why_not: "",
        ahead,
        markers: if counted { MARKERS.len() } else { 0 },
        plain: MARKERS.len(),
        secure: LETTERS.len(),
        which: String::from("ssssssss"),
        never_on: false,
        from: 0,
        stamps: Vec::new(),
        entries: Vec::new(),
        case: None,
        verdict: None,
        not_driven: None,
        markers_at: None,
    }
}

fn stand_in_control(counted: bool, plain: usize, secure: usize) -> Control {
    Control {
        counted,
        why_not: "",
        landed_plain: 8,
        landed_secure: 3,
        plain,
        secure,
        after: 3,
        not_driven: None,
    }
}

#[derive(Default)]
struct CountingLine {
    starts: usize,
    calls: Vec<(&'static str, usize)>,
}

impl Line for CountingLine {
    type Fixture = usize;

    fn start(&mut self, _name: &str) -> usize {
        self.starts += 1;
        self.starts
    }

    fn phase(&mut self, _name: &str, fixture: &mut usize) -> (Vec<Round>, Vec<DropCounts>) {
        self.calls.push(("phase", *fixture));
        (vec![stand_in_round(false, 0)], vec![DropCounts::new()])
    }

    fn control(&mut self, _name: &str, fixture: &mut usize) -> (Vec<Control>, DropCounts) {
        self.calls.push(("control", *fixture));
        (vec![stand_in_control(true, 5, 0)], DropCounts::new())
    }
}

#[test]
fn the_line_at_the_products_constants_starts_one_fixture_for_its_phase_and_its_control() {
    let mut line = CountingLine::default();
    let _ = product_line("stand-in", &mut line);
    println!("launch-count starts={} calls={:?}", line.starts, line.calls);
    assert_eq!(line.starts, 1, "one fixture started for the whole line");
    assert_eq!(
        line.calls,
        [("phase", 1), ("control", 1)],
        "the phase and then the control ran on the one fixture"
    );
}

fn not_counted_round(why_not: &'static str, ahead: usize, entries: Vec<RawEntry>) -> Round {
    Round {
        why_not,
        entries,
        ..stand_in_round(false, ahead)
    }
}

type ProductRow = (&'static str, Vec<Round>, Vec<Control>, u64, &'static str);

#[test]
fn the_verdict_at_the_products_constants_holds_each_rule_on_a_table() {
    const MARKERS_NOT_RECORDED: &str = "the markers were not recorded";
    let gap_after = || vec![arrived_at(10), gap_drop_at(40)];
    let control = || vec![stand_in_control(true, 5, 0)];
    let not_exact = || {
        vec![Control {
            why_not: "secure input did not read off after the secure field",
            ..stand_in_control(false, 5, 0)
        }]
    };
    let rows: [ProductRow; 16] = [
        (
            "counted exact rounds and an exact control",
            vec![stand_in_round(true, 0)],
            control(),
            0,
            "held",
        ),
        (
            "a round whose markers were not recorded, with a gap drop after its first arrival",
            vec![not_counted_round(MARKERS_NOT_RECORDED, 0, gap_after())],
            control(),
            0,
            "notkept",
        ),
        (
            "that round with no gap drop in its entries",
            vec![not_counted_round(
                MARKERS_NOT_RECORDED,
                0,
                vec![arrived_at(10)],
            )],
            control(),
            6,
            "failed",
        ),
        (
            "that round with its gap drop before its first arrival",
            vec![not_counted_round(
                MARKERS_NOT_RECORDED,
                0,
                vec![gap_drop_at(5), arrived_at(10)],
            )],
            control(),
            0,
            "failed",
        ),
        (
            "that round with a key recorded ahead of the markers",
            vec![not_counted_round(MARKERS_NOT_RECORDED, 1, gap_after())],
            control(),
            0,
            "failed",
        ),
        (
            "a counted round with a key recorded ahead of the markers",
            vec![stand_in_round(true, 1)],
            control(),
            6,
            "failed",
        ),
        (
            "a counted control with a record of the secure field",
            vec![stand_in_round(true, 0)],
            vec![stand_in_control(true, 5, 1)],
            6,
            "failed",
        ),
        (
            "T2: a round not counted for its markers with 8 letters recorded and landed in the \
             secure field",
            vec![not_counted_round(MARKERS_NOT_RECORDED, 8, gap_after())],
            control(),
            1,
            "failed",
        ),
        (
            "a round not counted for another cause, with a gap drop in another round and in the \
             control's counts",
            vec![
                not_counted_round(
                    "the letters did not land in the secure field",
                    0,
                    Vec::new(),
                ),
                not_counted_round(MARKERS_NOT_RECORDED, 0, gap_after()),
            ],
            control(),
            1,
            "failed",
        ),
        (
            "a control that did not count with a record of the secure field",
            vec![stand_in_round(true, 0)],
            vec![stand_in_control(false, 5, 1)],
            1,
            "failed",
        ),
        (
            "a control that is not exact with a gap drop in its own counts",
            vec![not_counted_round(MARKERS_NOT_RECORDED, 0, gap_after())],
            not_exact(),
            1,
            "notkept",
        ),
        (
            "a control that is not exact with no gap drop in its own counts",
            vec![not_counted_round(MARKERS_NOT_RECORDED, 0, gap_after())],
            not_exact(),
            0,
            "failed",
        ),
        (
            "counted exact rounds and a control that is not exact with no gap drop of its own",
            vec![stand_in_round(true, 0)],
            not_exact(),
            0,
            "failed",
        ),
        (
            "a phase that ran no round, with an exact control",
            Vec::new(),
            control(),
            0,
            "failed",
        ),
        (
            "a phase that ran no round and a control that ran no round",
            Vec::new(),
            Vec::new(),
            0,
            "failed",
        ),
        (
            "a counted round with more records of the markers than markers, beside a round not \
             kept for a gap",
            vec![
                Round {
                    markers: MARKERS.len() + 1,
                    ..stand_in_round(true, 0)
                },
                not_counted_round(MARKERS_NOT_RECORDED, 0, gap_after()),
            ],
            control(),
            0,
            "failed",
        ),
    ];
    let mut wrong = Vec::new();
    for (what, rounds, controls, control_gap_drops, want) in rows {
        let got = product_verdict(&rounds, &controls, control_gap_drops);
        println!("product row: {what}: want={want} got={got}");
        if got != want {
            wrong.push(what);
        }
    }
    assert!(
        wrong.is_empty(),
        "rows that gave another verdict: {wrong:?}"
    );
}

type ThreeWayRow = (&'static str, Vec<Round>, &'static str);

#[test]
fn the_three_way_verdict_of_the_return_phase_holds_each_rule_on_a_table() {
    let landed = |counted: bool, ahead: usize| stand_in_round(counted, ahead);
    let none_landed = |counted: bool, ahead: usize| Round {
        secure: 0,
        ..stand_in_round(counted, ahead)
    };
    let judged_recorded = |counted: bool| Round {
        verdict: Some(Verdict::Recorded),
        ..stand_in_round(counted, 0)
    };
    let rows: [ThreeWayRow; 10] = [
        (
            "a counted round with no key ahead",
            vec![landed(true, 0)],
            "held",
        ),
        (
            "a round that did not count",
            vec![landed(false, 0)],
            "notdriven",
        ),
        (
            "a counted round and a round that did not count",
            vec![landed(true, 0), landed(false, 0)],
            "notdriven",
        ),
        (
            "a counted round with a key ahead",
            vec![landed(true, 1)],
            "failed",
        ),
        (
            "N2: a round that did not count, with a key ahead and the letters landed in the \
             secure field",
            vec![landed(false, 1)],
            "failed",
        ),
        (
            "that round beside a counted round",
            vec![landed(true, 0), landed(false, 8)],
            "failed",
        ),
        (
            "a round that did not count, with a key ahead and no letter landed",
            vec![none_landed(false, 1)],
            "notdriven",
        ),
        (
            "N2: a round that did not count, judged recorded",
            vec![judged_recorded(false)],
            "failed",
        ),
        (
            "N2: a counted round with no key ahead, judged recorded",
            vec![judged_recorded(true)],
            "failed",
        ),
        ("a phase that ran no round", Vec::new(), "failed"),
    ];
    let mut wrong = Vec::new();
    for (what, rounds, want) in rows {
        let (got, held, failed, not_driven) = three_way_verdict(&rounds);
        println!(
            "three-way row: {what}: want={want} got={got} held={held} failed={failed} \
             notdriven={not_driven}"
        );
        assert_eq!(
            held + failed + not_driven,
            rounds.len(),
            "{what}: each round counted once"
        );
        if got != want {
            wrong.push(what);
        }
    }
    assert!(
        wrong.is_empty(),
        "rows that gave another verdict: {wrong:?}"
    );
}

fn side(fixture: &mut Fixture, gap: Duration, step: Duration) -> (usize, bool, DropCounts) {
    let recorder = Recorder::start(seamed(SHORT_BOUND, gap, step));
    fixture.ask("clear", "cleared");
    fixture.focus("plain");
    assert!(
        state_reads(false, Duration::from_secs(2)) && fixture.in_front(),
        "refused: the fixture is not in front with secure input off"
    );
    let from = SystemTime::now();
    fixture.keys(Duration::from_millis(30), &codes(&MARKERS));
    std::thread::sleep(SHORT_BOUND + MARGIN);
    let recorded = recorder.between(from, SystemTime::now());
    let impaired = recorder.source.health().state
        == SourceState::Impaired {
            reason: HealthReason::RunLoopStalled,
        };
    let drops = recorder.drops.clone();
    (recorded, impaired, drops)
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn the_gap_rule_through_the_real_timer_drops_every_key_when_every_tick_is_a_gap_and_none_when_none_is(
) {
    let name = "the_gap_rule_through_the_real_timer_drops_every_key_when_every_tick_is_a_gap_and_none_when_none_is";
    let mut fixture = started_fixture(name);
    let every = side(
        &mut fixture,
        Duration::from_millis(30),
        Duration::from_millis(60),
    );
    println!(
        "{name} side=every_tick_a_gap recorded={} impaired={} {}",
        every.0,
        u8::from(every.1),
        drops_line(&every.2)
    );
    let none = side(
        &mut fixture,
        Duration::from_millis(10_000),
        Duration::from_millis(10),
    );
    println!(
        "{name} side=no_tick_a_gap recorded={} impaired={} {}",
        none.0,
        u8::from(none.1),
        drops_line(&none.2)
    );
    assert!(
        every.0 == 0 && every.1 && every.2.of("gap") > 0,
        "{name}: with every tick a gap a key was recorded or the source did not read impaired"
    );
    assert_eq!(
        (none.0, none.2.of("gap")),
        (MARKERS.len(), 0),
        "{name}: with no tick a gap the markers were not released"
    );
}

fn stand_in(name: &str, body: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("open-stand-in-{name}"));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("the stand-in is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .expect("the stand-in can run");
    path
}

#[test]
fn the_launch_path_fails_with_a_line_that_names_the_wait_that_ran_out() {
    let wait = Duration::from_millis(1_500);
    let bundle = Path::new(env!("CARGO_TARGET_TMPDIR")).join("NoBundle.app");
    let cases = [
        (
            "never",
            "exec sleep 30",
            "the wait for the fixture's output fifo to open ran out after 1500 ms",
        ),
        (
            "fails",
            "exit 3",
            "open ended with exit status: 3 before the fixture's output fifo opened",
        ),
        (
            "output",
            "exec 3>\"$5\"\nexec sleep 30",
            "the wait for the fixture's input fifo to open ran out after 1500 ms",
        ),
        (
            "hangs",
            "exec 3>\"$5\" 4<\"$3\"\nexec sleep 30",
            "the wait for open to end ran out after 1500 ms",
        ),
        (
            "silent",
            "(exec 3>\"$5\" 4<\"$3\"; exec sleep 3) &\nexit 0",
            "the wait for the fixture's ready line ran out after 1500 ms",
        ),
    ];
    for (case, body, want) in cases {
        let started = Instant::now();
        let got = launched(
            &stand_in(case, body),
            &bundle,
            &format!("stand-in-{case}-"),
            wait,
        )
        .err();
        let seconds = started.elapsed().as_secs_f64();
        println!(
            "launch case={case} failed={} named_the_wait={} seconds={seconds:.1}",
            u8::from(got.is_some()),
            u8::from(got.as_deref() == Some(want))
        );
        assert_eq!(got.as_deref(), Some(want), "the stand-in {case}");
        assert!(
            seconds < 3.0 * wait.as_secs_f64() + 1.0,
            "the stand-in {case} ended within its deadlines"
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    kind: String,
    detail: i64,
    ns: u64,
    on: bool,
}

fn parse_stamps(line: &str) -> Vec<Stamp> {
    line.split(' ')
        .skip(3)
        .filter_map(|word| {
            let mut parts = word.split(':');
            let kind = parts.next()?.to_owned();
            let detail = parts.next()?.parse().ok()?;
            let ns = parts.next()?.parse().ok()?;
            let on = parts.next()? == "1";
            Some(Stamp {
                kind,
                detail,
                ns,
                on,
            })
        })
        .collect()
}

fn on_runs(stamps: &[Stamp]) -> Vec<(u64, u64)> {
    let mut runs = Vec::new();
    let mut open: Option<(u64, u64)> = None;
    for stamp in stamps {
        match (stamp.on, open) {
            (true, None) => open = Some((stamp.ns, stamp.ns)),
            (true, Some((from, _))) => open = Some((from, stamp.ns)),
            (false, Some(run)) => {
                runs.push(run);
                open = None;
            }
            (false, None) => {}
        }
    }
    runs.extend(open);
    runs
}

fn first_off_after(stamps: &[Stamp], at: u64) -> Option<u64> {
    stamps
        .iter()
        .find(|stamp| stamp.ns > at && !stamp.on)
        .map(|stamp| stamp.ns)
}

fn off_around(stamps: &[Stamp], first_on: u64, last_on: u64) -> (Option<u64>, Option<u64>) {
    let before = stamps
        .iter()
        .filter(|stamp| !stamp.on && stamp.ns < first_on)
        .map(|stamp| stamp.ns)
        .max();
    let after = stamps
        .iter()
        .find(|stamp| !stamp.on && stamp.ns > last_on)
        .map(|stamp| stamp.ns);
    (before, after)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Run {
    first_on: u64,
    last_on: u64,
    last_off_before: Option<u64>,
    first_off_after: Option<u64>,
}

impl Run {
    fn span(&self) -> (u64, u64) {
        (
            self.last_off_before.unwrap_or(self.first_on),
            self.first_off_after.unwrap_or(self.last_on),
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Evidence {
    runs: Vec<Run>,
    since: u64,
    first_stamp: Option<u64>,
    first_stamp_on: bool,
    readings: Vec<(u64, u64, Reading)>,
    first_release: Option<u64>,
    on_drops: Vec<u64>,
    releases: Vec<(Option<u64>, u64)>,
}

fn evidence(stamps: &[Stamp], entries: &[RawEntry], since: u64) -> Evidence {
    let mut found = Evidence {
        runs: on_runs(stamps)
            .into_iter()
            .map(|(first_on, last_on)| {
                let (last_off_before, first_off_after) = off_around(stamps, first_on, last_on);
                Run {
                    first_on,
                    last_on,
                    last_off_before,
                    first_off_after,
                }
            })
            .collect(),
        since,
        first_stamp: stamps.first().map(|stamp| stamp.ns),
        first_stamp_on: stamps.first().is_some_and(|stamp| stamp.on),
        ..Evidence::default()
    };
    for entry in entries {
        match *entry {
            RawEntry::Reading {
                before,
                after,
                value,
                ..
            } => found.readings.push((before, after, value)),
            RawEntry::Released { at } => {
                found.first_release = Some(found.first_release.map_or(at, |first| first.min(at)));
            }
            RawEntry::Dropped {
                at,
                cause: Cause::On,
                ..
            } => found.on_drops.push(at),
            _ => {}
        }
    }
    found.releases = released_keys(entries);
    found
}

fn released_keys(entries: &[RawEntry]) -> Vec<(Option<u64>, u64)> {
    let mut held: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
    let mut releases = Vec::new();
    for entry in entries {
        match *entry {
            RawEntry::Arrival {
                at,
                admission: Arrived::Held,
            } => held.push_back(at),
            RawEntry::Arrival {
                admission: Arrived::Withheld { dropped, .. },
                ..
            } => held.drain(..dropped.min(held.len())).for_each(drop),
            RawEntry::Dropped { keys, .. } => held.drain(..keys.min(held.len())).for_each(drop),
            RawEntry::Released { at } => releases.push((held.pop_front(), at)),
            _ => {}
        }
    }
    releases
}

fn proof_run(found: &Evidence) -> Option<&Run> {
    found.runs.iter().find(|run| run.first_on >= found.since)
}

fn on_in_span(found: &Evidence, run: &Run) -> Vec<u64> {
    let (start, end) = run.span();
    found
        .readings
        .iter()
        .filter(|&&(before, after, value)| value == Reading::On && before >= start && after <= end)
        .map(|&(before, _, _)| before)
        .collect()
}

fn on_in_narrow(found: &Evidence) -> usize {
    found
        .readings
        .iter()
        .filter(|&&(before, after, value)| {
            value == Reading::On
                && found
                    .runs
                    .iter()
                    .any(|run| before >= run.first_on && after <= run.last_on)
        })
        .count()
}

fn off_in_narrow(found: &Evidence) -> usize {
    found
        .readings
        .iter()
        .filter(|&&(before, after, value)| {
            value == Reading::Off
                && found
                    .runs
                    .iter()
                    .any(|run| before >= run.first_on && after <= run.last_on)
        })
        .count()
}

fn on_outside(found: &Evidence) -> usize {
    let from = found.first_stamp.unwrap_or(0);
    found
        .readings
        .iter()
        .filter(|&&(before, after, value)| {
            value == Reading::On
                && before >= from
                && !found.runs.iter().any(|run| {
                    let (start, end) = run.span();
                    before >= start && after <= end
                })
        })
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    A,
    B,
    Between,
    C,
    D,
    Seen,
}

fn short_case(found: &Evidence) -> Case {
    let Some(run) = proof_run(found) else {
        return Case::C;
    };
    if off_in_narrow(found) > 0 {
        return Case::D;
    }
    let (start, _) = run.span();
    if let Some(release) = found.first_release {
        if release < start {
            return Case::B;
        }
        if release < run.first_on {
            return Case::Between;
        }
    }
    if on_in_span(found, run).is_empty() {
        Case::A
    } else {
        Case::Seen
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Held,
    LateClick,
    NeverOn,
    NotCounted(&'static str),
    NoProof,
    Recorded,
    NotDriven,
}

fn mended_verdict(
    never_on: bool,
    why_not: Option<&'static str>,
    ahead: usize,
    found: &Evidence,
    bound: Duration,
) -> Verdict {
    if never_on {
        return Verdict::NeverOn;
    }
    if found.first_stamp_on {
        return Verdict::NotCounted("the round's first stamp read on");
    }
    let run = proof_run(found);
    let cut = run.and_then(|run| run.first_off_after);
    let recorded: Vec<(Option<u64>, u64)> = found
        .releases
        .iter()
        .copied()
        .filter(|&(arrival, _)| arrival.is_none_or(|arrival| cut.is_none_or(|cut| arrival < cut)))
        .collect();
    if ahead > 0 || !recorded.is_empty() {
        let bound = u64::try_from(bound.as_nanos()).unwrap_or(u64::MAX);
        let last_off = run.and_then(|run| run.last_off_before);
        let excused = |&(arrival, release): &(Option<u64>, u64)| {
            last_off.is_some_and(|off| release < off)
                && arrival.is_some_and(|arrival| release.saturating_sub(arrival) >= bound)
        };
        if recorded.is_empty() || !recorded.iter().all(excused) {
            return Verdict::Recorded;
        }
        return why_not.map_or(Verdict::LateClick, Verdict::NotCounted);
    }
    if let Some(why) = why_not {
        return Verdict::NotCounted(why);
    }
    if run.is_some_and(|run| {
        on_in_span(found, run)
            .iter()
            .any(|before| found.on_drops.contains(before))
    }) {
        Verdict::Held
    } else {
        Verdict::NoProof
    }
}

fn key_fates(entries: &[RawEntry]) -> Vec<(u64, String, Option<u64>)> {
    let mut fates: Vec<(u64, String, Option<u64>)> = Vec::new();
    let mut held: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let resolve = |fates: &mut Vec<(u64, String, Option<u64>)>,
                   held: &mut std::collections::VecDeque<usize>,
                   count: usize,
                   fate: String,
                   at: u64| {
        for _ in 0..count {
            if let Some(index) = held.pop_front() {
                fates[index].1.clone_from(&fate);
                fates[index].2 = Some(at);
            }
        }
    };
    for entry in entries {
        match *entry {
            RawEntry::Arrival { at, admission } => match admission {
                Arrived::Held => {
                    held.push_back(fates.len());
                    fates.push((at, String::from("open"), None));
                }
                Arrived::Suppressed => fates.push((at, String::from("suppressed"), Some(at))),
                Arrived::Withheld { cause, dropped } => {
                    let fate = format!("dropped_{cause:?}").to_lowercase();
                    resolve(&mut fates, &mut held, dropped, fate.clone(), at);
                    fates.push((at, format!("withheld_{cause:?}").to_lowercase(), Some(at)));
                }
            },
            RawEntry::Released { at } => {
                resolve(&mut fates, &mut held, 1, String::from("released"), at);
            }
            RawEntry::Dropped {
                at, cause, keys, ..
            } => {
                let fate = format!("dropped_{cause:?}").to_lowercase();
                resolve(&mut fates, &mut held, keys, fate, at);
            }
            RawEntry::Reading { .. } | RawEntry::Notice { .. } => {}
        }
    }
    fates
}

fn relative_us(ns: u64, from: u64) -> i64 {
    let micros = |value: u64| i64::try_from(value / 1_000).unwrap_or(i64::MAX);
    if ns >= from {
        micros(ns - from)
    } else {
        -micros(from - ns)
    }
}

fn value_word(value: Reading) -> &'static str {
    match value {
        Reading::Off => "off",
        Reading::On => "on",
        Reading::Unknown => "unknown",
    }
}

fn seen_word(seen: NoticeSeen) -> &'static str {
    match seen {
        NoticeSeen::Notice => "notice",
        NoticeSeen::Start => "start",
        NoticeSeen::Unanswered => "unanswered",
    }
}

fn focus_word(focus: Focus) -> &'static str {
    match focus {
        Focus::Secure => "secure",
        Focus::Other => "other",
        Focus::Doubt => "doubt",
    }
}

fn caller_word(caller: Caller) -> &'static str {
    match caller {
        Caller::Event => "event",
        Caller::Tick => "tick",
        Caller::ReleaseLoop => "release",
    }
}

fn entry_lines(
    name: &str,
    number: usize,
    from: u64,
    stamps: &[Stamp],
    entries: &[RawEntry],
    window: Option<(u64, u64)>,
) -> Vec<String> {
    let fixture: Vec<String> = stamps
        .iter()
        .map(|stamp| {
            format!(
                "{}:{}:{}:{}",
                stamp.kind,
                stamp.detail,
                relative_us(stamp.ns, from),
                u8::from(stamp.on)
            )
        })
        .collect();
    let keys: Vec<String> = key_fates(entries)
        .iter()
        .map(|(arrival, fate, at)| {
            format!(
                "{}:{fate}:{}",
                relative_us(*arrival, from),
                at.map_or_else(|| String::from("-"), |at| relative_us(at, from).to_string())
            )
        })
        .collect();
    let readings: Vec<String> = entries
        .iter()
        .filter_map(|entry| match *entry {
            RawEntry::Reading {
                before,
                after,
                value,
                caller,
                held,
            } if window.is_none_or(|(low, high)| after >= low && before <= high) => Some(format!(
                "{}+{}:{}:{}:{held}",
                relative_us(before, from),
                after.saturating_sub(before) / 1_000,
                value_word(value),
                caller_word(caller)
            )),
            _ => None,
        })
        .collect();
    let part = if window.is_some() {
        "readings_window"
    } else {
        "readings"
    };
    let notices: Vec<String> = entries
        .iter()
        .filter_map(|entry| match *entry {
            RawEntry::Notice { at, seen, focus } => Some(format!(
                "{}:{}:{}",
                relative_us(at, from),
                seen_word(seen),
                focus_word(focus)
            )),
            _ => None,
        })
        .collect();
    vec![
        format!(
            "{name} entries round={number} part=notices {}",
            notices.join(" ")
        ),
        format!(
            "{name} entries round={number} part=fixture {}",
            fixture.join(" ")
        ),
        format!("{name} entries round={number} part=keys {}", keys.join(" ")),
        format!(
            "{name} entries round={number} part={part} {}",
            readings.join(" ")
        ),
    ]
}

fn ms_ns(ms: u64) -> u64 {
    ms * 1_000_000
}

fn reading_at(before_ms: u64, after_ms: u64, value: Reading) -> RawEntry {
    RawEntry::Reading {
        before: ms_ns(before_ms),
        after: ms_ns(after_ms),
        value,
        caller: Caller::Tick,
        held: 1,
    }
}

fn stamps_at(points: &[(u64, bool)]) -> Vec<Stamp> {
    points
        .iter()
        .map(|&(ms, on)| Stamp {
            kind: String::from("ev"),
            detail: 0,
            ns: ms_ns(ms),
            on,
        })
        .collect()
}

const ROUND: [(u64, bool); 5] = [
    (0, false),
    (90, false),
    (100, true),
    (400, true),
    (420, false),
];

fn table_evidence(points: &[(u64, bool)], entries: &[RawEntry]) -> Evidence {
    evidence(&stamps_at(points), entries, 0)
}

fn on_drop_at(ms: u64) -> RawEntry {
    RawEntry::Dropped {
        at: ms_ns(ms),
        cause: Cause::On,
        keys: 8,
        unread: false,
    }
}

fn released_at(ms: u64) -> RawEntry {
    RawEntry::Released { at: ms_ns(ms) }
}

fn arrived_at(ms: u64) -> RawEntry {
    RawEntry::Arrival {
        at: ms_ns(ms),
        admission: Arrived::Held,
    }
}

fn gap_drop_at(ms: u64) -> RawEntry {
    RawEntry::Dropped {
        at: ms_ns(ms),
        cause: Cause::Gap,
        keys: 1,
        unread: false,
    }
}

const TABLE_BOUND: Duration = Duration::from_millis(50);

type MendedRow = (
    &'static str,
    bool,
    Option<&'static str>,
    usize,
    Evidence,
    Verdict,
);

#[test]
fn the_verdict_of_a_mended_round_holds_each_rule_on_a_table_of_entries() {
    let proved = |before: u64, after: u64| {
        table_evidence(
            &ROUND,
            &[reading_at(before, after, Reading::On), on_drop_at(before)],
        )
    };
    let rows: [MendedRow; 23] = [
        (
            "a reading of on inside the narrow span and its drop",
            false,
            None,
            0,
            proved(110, 111),
            Verdict::Held,
        ),
        (
            "a reading of on between the last stamp of off and the first stamp of on",
            false,
            None,
            0,
            proved(92, 95),
            Verdict::Held,
        ),
        (
            "a reading of on that straddles the first stamp of on",
            false,
            None,
            0,
            proved(98, 102),
            Verdict::Held,
        ),
        (
            "a reading of on that straddles the last stamp of off",
            false,
            None,
            0,
            proved(89, 91),
            Verdict::NoProof,
        ),
        (
            "a reading of on wholly before the last stamp of off",
            false,
            None,
            0,
            proved(50, 51),
            Verdict::NoProof,
        ),
        (
            "a reading of on between the last stamp of on and the first stamp of off after",
            false,
            None,
            0,
            proved(405, 410),
            Verdict::Held,
        ),
        (
            "a reading of on that straddles the first stamp of off after",
            false,
            None,
            0,
            proved(418, 422),
            Verdict::NoProof,
        ),
        (
            "a reading of off in a widened part proves nothing",
            false,
            None,
            0,
            table_evidence(&ROUND, &[reading_at(92, 95, Reading::Off), on_drop_at(92)]),
            Verdict::NoProof,
        ),
        (
            "a reading of on inside the span with the drop made by a reading outside it",
            false,
            None,
            0,
            table_evidence(
                &ROUND,
                &[
                    reading_at(50, 51, Reading::On),
                    on_drop_at(50),
                    reading_at(150, 151, Reading::On),
                ],
            ),
            Verdict::NoProof,
        ),
        (
            "a reading of on inside the span and no drop for on in the round",
            false,
            None,
            0,
            table_evidence(&ROUND, &[reading_at(150, 151, Reading::On)]),
            Verdict::NoProof,
        ),
        (
            "a first release before the last stamp of off",
            false,
            None,
            8,
            table_evidence(
                &ROUND,
                &[
                    arrived_at(20),
                    released_at(80),
                    reading_at(150, 151, Reading::On),
                ],
            ),
            Verdict::LateClick,
        ),
        (
            "a first release between the last stamp of off and the first stamp of on",
            false,
            None,
            8,
            table_evidence(
                &ROUND,
                &[
                    arrived_at(20),
                    released_at(95),
                    reading_at(150, 151, Reading::On),
                ],
            ),
            Verdict::Recorded,
        ),
        (
            "T3: a first release before the last stamp of off and a second inside the on-run",
            false,
            None,
            8,
            table_evidence(
                &ROUND,
                &[
                    released_at(80),
                    reading_at(150, 151, Reading::On),
                    on_drop_at(150),
                    released_at(200),
                ],
            ),
            Verdict::Recorded,
        ),
        (
            "T3 with the arrivals of both keys",
            false,
            None,
            8,
            table_evidence(
                &ROUND,
                &[
                    arrived_at(20),
                    arrived_at(25),
                    released_at(80),
                    reading_at(150, 151, Reading::On),
                    released_at(200),
                ],
            ),
            Verdict::Recorded,
        ),
        (
            "T4: the entries of T3 in a round whose markers were not recorded",
            false,
            Some("the markers were not recorded"),
            8,
            table_evidence(
                &ROUND,
                &[
                    released_at(80),
                    reading_at(150, 151, Reading::On),
                    on_drop_at(150),
                    released_at(200),
                ],
            ),
            Verdict::Recorded,
        ),
        (
            "T5: one release before the last stamp of off, with no arrival in the entries, in a round \
             that did not read off before the markers",
            false,
            Some("secure input did not read off before the markers"),
            0,
            table_evidence(&ROUND, &[released_at(80)]),
            Verdict::Recorded,
        ),
        (
            "T5 with the key's arrival: every recorded key excused and another cause of not counting",
            false,
            Some("secure input did not read off before the markers"),
            0,
            table_evidence(&ROUND, &[arrived_at(20), released_at(80)]),
            Verdict::NotCounted("secure input did not read off before the markers"),
        ),
        (
            "a release before the last stamp of off but under the bound after its arrival",
            false,
            None,
            8,
            table_evidence(
                &ROUND,
                &[
                    arrived_at(50),
                    released_at(80),
                    reading_at(150, 151, Reading::On),
                ],
            ),
            Verdict::Recorded,
        ),
        (
            "the round's first stamp read on",
            false,
            None,
            0,
            table_evidence(
                &[(100, true), (400, true), (420, false)],
                &[reading_at(150, 151, Reading::On), on_drop_at(150)],
            ),
            Verdict::NotCounted("the round's first stamp read on"),
        ),
        (
            "the test's read never said on",
            true,
            None,
            0,
            proved(110, 111),
            Verdict::NeverOn,
        ),
        (
            "a key recorded in a round that kept the on-time",
            false,
            None,
            2,
            proved(110, 111),
            Verdict::Recorded,
        ),
        (
            "a round that did not count for a cause of F6",
            false,
            Some("the markers were not recorded"),
            0,
            proved(110, 111),
            Verdict::NotCounted("the markers were not recorded"),
        ),
        (
            "the proof read on the first run after the return",
            false,
            None,
            0,
            {
                let mut found = table_evidence(
                    &[
                        (0, false),
                        (20, true),
                        (30, true),
                        (40, false),
                        (90, false),
                        (100, true),
                        (400, true),
                        (420, false),
                    ],
                    &[reading_at(25, 26, Reading::On), on_drop_at(25)],
                );
                found.since = ms_ns(60);
                found
            },
            Verdict::NoProof,
        ),
    ];
    for (what, never_on, why_not, ahead, found, want) in rows {
        assert_eq!(
            mended_verdict(never_on, why_not, ahead, &found, TABLE_BOUND),
            want,
            "{what}"
        );
    }
}

#[test]
fn the_case_of_a_short_window_round_holds_each_rule_on_a_table_of_entries() {
    let rows: [(&str, Evidence, Case); 12] = [
        (
            "no on-time at all",
            table_evidence(
                &[(0, false), (10, false)],
                &[reading_at(10, 11, Reading::Off)],
            ),
            Case::C,
        ),
        (
            "a reading of off inside the narrow span is (D) still",
            table_evidence(
                &ROUND,
                &[
                    reading_at(110, 112, Reading::Off),
                    reading_at(115, 116, Reading::On),
                ],
            ),
            Case::D,
        ),
        (
            "a reading of off before the first stamp of on is not (D)",
            table_evidence(&ROUND, &[reading_at(92, 95, Reading::Off)]),
            Case::A,
        ),
        (
            "a reading of off after the last stamp of on is not (D)",
            table_evidence(&ROUND, &[reading_at(405, 410, Reading::Off)]),
            Case::A,
        ),
        (
            "a reading of on between the last stamp of off and the first stamp of on",
            table_evidence(&ROUND, &[reading_at(92, 95, Reading::On)]),
            Case::Seen,
        ),
        (
            "a reading of on between the last stamp of on and the first stamp of off after",
            table_evidence(&ROUND, &[reading_at(405, 410, Reading::On)]),
            Case::Seen,
        ),
        (
            "a reading of on that straddles the last stamp of off",
            table_evidence(&ROUND, &[reading_at(89, 91, Reading::On)]),
            Case::A,
        ),
        (
            "no reading in the span",
            table_evidence(
                &ROUND,
                &[
                    reading_at(80, 85, Reading::Off),
                    reading_at(430, 431, Reading::Off),
                ],
            ),
            Case::A,
        ),
        (
            "a first release before the last stamp of off",
            table_evidence(
                &ROUND,
                &[released_at(80), reading_at(150, 151, Reading::On)],
            ),
            Case::B,
        ),
        (
            "a first release between the last stamp of off and the first stamp of on",
            table_evidence(
                &ROUND,
                &[released_at(95), reading_at(150, 151, Reading::On)],
            ),
            Case::Between,
        ),
        (
            "a run with no stamp of off before it and a reading before its first stamp",
            table_evidence(
                &[(100, true), (400, true), (420, false)],
                &[reading_at(50, 51, Reading::On)],
            ),
            Case::A,
        ),
        (
            "a reading of on inside the narrow span",
            table_evidence(&ROUND, &[reading_at(110, 112, Reading::On)]),
            Case::Seen,
        ),
    ];
    for (what, found, want) in rows {
        assert_eq!(short_case(&found), want, "{what}");
    }
}

type OutsideRow = (&'static str, Vec<(u64, bool)>, RawEntry, usize);

#[test]
fn a_reading_of_on_outside_every_span_is_counted_from_the_rounds_first_stamp() {
    let rows: [OutsideRow; 4] = [
        (
            "a reading of on before the last stamp of off",
            ROUND.to_vec(),
            reading_at(50, 51, Reading::On),
            1,
        ),
        (
            "a reading of on inside the span",
            ROUND.to_vec(),
            reading_at(92, 95, Reading::On),
            0,
        ),
        (
            "a reading of on after the first stamp of off after",
            ROUND.to_vec(),
            reading_at(430, 431, Reading::On),
            1,
        ),
        (
            "a reading of on before the round's first stamp",
            [(10, false)]
                .iter()
                .chain(ROUND[1..].iter())
                .copied()
                .collect(),
            reading_at(5, 6, Reading::On),
            0,
        ),
    ];
    for (what, points, reading, want) in rows {
        assert_eq!(
            on_outside(&table_evidence(&points, &[reading])),
            want,
            "{what}"
        );
    }
}

#[test]
fn the_on_time_and_the_fates_of_the_keys_are_read_from_the_raw_entries() {
    let stamp = |ns: u64, on: bool| Stamp {
        kind: String::from("ev"),
        detail: 10,
        ns,
        on,
    };
    let stamps = [
        stamp(10, false),
        stamp(20, true),
        stamp(30, true),
        stamp(40, false),
        stamp(50, true),
    ];
    assert_eq!(on_runs(&stamps), [(20, 30), (50, 50)]);
    assert_eq!(first_off_after(&stamps, 30), Some(40));
    let parsed = parse_stamps("stamps n=2 lost=0 fr_secure:1:2000500:1 cmd_plain:0:3000000:0");
    assert_eq!(
        parsed,
        [
            Stamp {
                kind: String::from("fr_secure"),
                detail: 1,
                ns: 2_000_500,
                on: true
            },
            Stamp {
                kind: String::from("cmd_plain"),
                detail: 0,
                ns: 3_000_000,
                on: false
            }
        ]
    );
    let entries = [
        RawEntry::Arrival {
            at: 1,
            admission: Arrived::Held,
        },
        RawEntry::Arrival {
            at: 2,
            admission: Arrived::Held,
        },
        RawEntry::Released { at: 5 },
        RawEntry::Arrival {
            at: 6,
            admission: Arrived::Held,
        },
        RawEntry::Dropped {
            at: 7,
            cause: Cause::On,
            keys: 2,
            unread: false,
        },
        RawEntry::Arrival {
            at: 8,
            admission: Arrived::Suppressed,
        },
        RawEntry::Arrival {
            at: 9,
            admission: Arrived::Held,
        },
    ];
    assert_eq!(
        key_fates(&entries),
        [
            (1, String::from("released"), Some(5)),
            (2, String::from("dropped_on"), Some(7)),
            (6, String::from("dropped_on"), Some(7)),
            (8, String::from("suppressed"), Some(8)),
            (9, String::from("open"), None),
        ]
    );
}

#[test]
fn the_fixture_gives_out_its_stamps_on_a_command_through_the_launch_path() {
    let fixture_body = "exec 3>\"$2\" 4<\"$1\"\nprintf 'ready pid=%s\\n' \"$$\" >&3\n\
        while IFS= read -r line <&4; do\n  case $line in\n    \
        stamps) printf 'stamps n=3 lost=0 cmd_clear:0:1000:0 fr_secure:1:2000500:1 cmd_stamps:0:3000000:1\\n' >&3 ;;\n    \
        quit) exit 0 ;;\n    *) printf 'unknown\\n' >&3 ;;\n  esac\ndone";
    let fixture_script = stand_in("stamps-fixture", fixture_body);
    let opener = stand_in(
        "stamps-opener",
        &format!("\"{}\" \"$3\" \"$5\" &\nexit 0", fixture_script.display()),
    );
    let bundle = Path::new(env!("CARGO_TARGET_TMPDIR")).join("NoBundle.app");
    let mut fixture = launched(&opener, &bundle, "stand-in-stamps-", Duration::from_secs(5))
        .expect("the stand-in fixture answers ready");
    let stamps = fixture.stamps();
    println!(
        "stand-in stamps n={} on_runs={:?}",
        stamps.len(),
        on_runs(&stamps)
    );
    assert_eq!(
        stamps
            .iter()
            .map(|stamp| stamp.kind.as_str())
            .collect::<Vec<_>>(),
        ["cmd_clear", "fr_secure", "cmd_stamps"]
    );
    assert_eq!(on_runs(&stamps), [(2_000_500, 3_000_000)]);
}

type Seen = Arc<Mutex<Vec<(u32, u64, u64, u64)>>>;

struct Tap {
    seen: Seen,
    run_loop: core_foundation::runloop::CFRunLoop,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Tap {
    fn start() -> Option<Self> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = channel();
        let thread = {
            let seen = seen.clone();
            std::thread::spawn(move || {
                use core_graphics::event::{
                    CGEventTap, CGEventTapOptions, CGEventTapPlacement, CallbackResult,
                };
                let tap = CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    vec![CGEventType::KeyDown, CGEventType::KeyUp],
                    move |_proxy, etype, event: &CGEvent| {
                        let uptime = uptime_ns();
                        // SAFETY: mach_absolute_time takes nothing and returns a number.
                        let mach = unsafe { mach_absolute_time() };
                        let event: &core_graphics::event::CGEventRef = event;
                        // SAFETY: a reference to a foreign-types borrowed type is the
                        // pointer of the C object, here the live event that the tap
                        // hands to its callback; the call only reads it.
                        let stamp =
                            unsafe { CGEventGetTimestamp(std::ptr::from_ref(event).cast()) };
                        seen.lock().unwrap_or_else(PoisonError::into_inner).push((
                            etype as u32,
                            stamp,
                            mach,
                            uptime,
                        ));
                        CallbackResult::Keep
                    },
                );
                let Ok(tap) = tap else {
                    let _ = tx.send(None);
                    return;
                };
                let source = tap
                    .mach_port()
                    .create_runloop_source(0)
                    .expect("the tap's run loop source");
                let run_loop = core_foundation::runloop::CFRunLoop::get_current();
                // SAFETY: kCFRunLoopCommonModes is a constant that CoreFoundation
                // exports and never writes.
                run_loop.add_source(&source, unsafe {
                    core_foundation::runloop::kCFRunLoopCommonModes
                });
                tap.enable();
                let _ = tx.send(Some(run_loop));
                core_foundation::runloop::CFRunLoop::run_current();
            })
        };
        let run_loop = rx.recv().ok().flatten()?;
        Some(Self {
            seen,
            run_loop,
            thread: Some(thread),
        })
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.run_loop.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn on_intervals(within: Duration, during: impl FnOnce()) -> Vec<u64> {
    let stop = Arc::new(AtomicBool::new(false));
    let poller = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut intervals = Vec::new();
            let mut on_since = None;
            while !stop.load(Ordering::SeqCst) {
                let on = secure_input_active();
                let now = uptime_ns();
                match (on, on_since) {
                    (true, None) => on_since = Some(now),
                    (false, Some(since)) => {
                        intervals.push(now - since);
                        on_since = None;
                    }
                    _ => {}
                }
            }
            if let Some(since) = on_since {
                intervals.push(uptime_ns() - since);
            }
            intervals
        })
    };
    during();
    std::thread::sleep(within);
    stop.store(true, Ordering::SeqCst);
    poller.join().expect("the poller ends")
}

fn first_on_after(at: u64, within: Duration) -> Option<u64> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if secure_input_active() {
            return Some(uptime_ns().saturating_sub(at));
        }
    }
    None
}

fn reply_time(line: &str) -> u64 {
    line.rsplit_once("at=")
        .and_then(|(_, at)| at.parse().ok())
        .expect("the reply carries a time")
}

fn page_probe(name: &str, open: &[&str], bundle: &str) {
    let page = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("stall-probe-page.html");
    std::fs::write(
        &page,
        "<!doctype html><title>stall probe</title><input id=p type=text>\
         <input id=s type=password autofocus>\
         <script>addEventListener('load', () => document.getElementById('s').focus())</script>",
    )
    .expect("the page is written");
    let started = Instant::now();
    let mut command = Command::new("/usr/bin/open");
    command.args(open).arg(&page);
    let opened = command.status().map(|s| s.success()).unwrap_or(false);
    let front = until(Duration::from_secs(30), || {
        frontmost_bundle().as_deref() == Some(bundle)
    });
    let front_ms = started.elapsed().as_millis();
    let on = state_reads(true, Duration::from_secs(15));
    let on_ms = started.elapsed().as_millis();
    println!(
        "probe page={name} opened={} front={} front_ms={front_ms} secure_on={} on_ms={on_ms}",
        u8::from(opened),
        u8::from(front),
        u8::from(on)
    );
    if let Some(app) = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .filter(|app| app.bundleIdentifier().map(|id| id.to_string()).as_deref() == Some(bundle))
    {
        app.forceTerminate();
    }
    let off = state_reads(false, Duration::from_secs(10));
    println!("probe page={name} secure_off_after_quit={}", u8::from(off));
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn the_probe_reports_what_the_stall_phases_rest_on() {
    let mut info = Timebase { numer: 0, denom: 0 };
    // SAFETY: `info` is a writable mach_timebase_info.
    unsafe { mach_timebase_info(&mut info) };
    println!(
        "probe clocks timebase_numer={} timebase_denom={}",
        info.numer, info.denom
    );
    println!("probe {}", session_line());
    let started = Instant::now();
    let (mut fixture, front) = Fixture::start().unwrap_or_else(|why| panic!("probe: {why}"));
    println!(
        "probe front fixture={} by={} ms={}",
        fixture.by,
        front.unwrap_or("none"),
        started.elapsed().as_millis()
    );
    assert!(
        front.is_some(),
        "refused: the fixture's window is not in front, so no key was posted"
    );
    let tap = Tap::start();
    println!("probe tap made={}", u8::from(tap.is_some()));

    fixture.focus("plain");
    fixture.keys(Duration::from_millis(20), &codes(&LETTERS[..3]));
    std::thread::sleep(Duration::from_millis(300));
    let state = fixture.state();
    println!(
        "probe plain_typing focus={} plain={} secure={} state_on={}",
        state.focus,
        state.plain,
        state.secure,
        u8::from(secure_input_active())
    );
    if let Some(tap) = &tap {
        for (etype, stamp, mach, uptime) in tap
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            let as_ticks = u128::from(*stamp) * u128::from(info.numer) / u128::from(info.denom);
            println!(
                "probe event type={etype} stamp_raw={stamp} mach_raw={mach} uptime_ns={uptime} \
                 lag_if_ns={} lag_if_ticks={}",
                i128::from(*uptime) - i128::from(*stamp),
                i128::from(*uptime) - as_ticks as i128
            );
        }
    }

    for attempt in 0..3 {
        fixture.ask("clear", "cleared");
        fixture.focus("plain");
        state_reads(false, Duration::from_secs(2));
        let at = reply_time(&fixture.ask("secure", "focus="));
        let on_after = first_on_after(at, Duration::from_secs(2));
        fixture.keys(Duration::from_millis(20), &codes(&LETTERS[..3]));
        std::thread::sleep(Duration::from_millis(300));
        let state = fixture.state();
        let leave = reply_time(&fixture.ask("plain", "focus="));
        let off = until(Duration::from_secs(2), || !secure_input_active());
        println!(
            "probe secure_focus attempt={attempt} on_after_focus_us={} secure={} plain={} \
             off_after_leave={} off_us={}",
            on_after.map_or(-1, |ns| (ns / 1_000) as i64),
            state.secure,
            state.plain,
            u8::from(off),
            (uptime_ns().saturating_sub(leave)) / 1_000
        );
    }

    for how in [Move::Tab, Move::Click, Move::Itself, Move::Return] {
        fixture.ask("clear", "cleared");
        if how == Move::Return {
            fixture.focus("secure");
            state_reads(true, Duration::from_secs(2));
            fixture.ask("hide", "hidden");
            let hidden = until(Duration::from_secs(3), || {
                frontmost_pid() != Some(fixture.pid) && !secure_input_active()
            });
            println!("probe return hidden={}", u8::from(hidden));
        } else {
            fixture.focus("plain");
            state_reads(false, Duration::from_secs(2));
        }
        let centre = secure_field_centre(&mut fixture);
        let command = if how == Move::Itself {
            "stall 300 self"
        } else {
            "stall 300"
        };
        fixture.send(command);
        let waiting = reply_time(&fixture.reply("waiting", Duration::from_secs(5)));
        let letters = codes(&LETTERS[..3]);
        match how {
            Move::Tab => fixture.keys(Duration::from_millis(10), &format!("{TAB} {letters}")),
            Move::Click => {
                fixture.click(&centre);
                fixture.keys(Duration::from_millis(10), &letters);
            }
            Move::Return => {
                let asked = activate_from_here(fixture.pid);
                println!("probe return activate_asked={}", u8::from(asked));
                fixture.keys(Duration::from_millis(10), &letters);
            }
            _ => fixture.keys(Duration::from_millis(10), &letters),
        }
        let woke = reply_time(&fixture.reply("woke", Duration::from_secs(5)));
        let on_after_wake = first_on_after(woke, Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(300));
        let state = fixture.state();
        println!(
            "probe stall how={how:?} slept_us={} on_after_wake_us={} front={} focus={} \
             plain={} secure={}",
            (woke - waiting) / 1_000,
            on_after_wake.map_or(-1, |ns| (ns / 1_000) as i64),
            u8::from(state.front),
            state.focus,
            state.plain,
            state.secure
        );
        fixture.focus("plain");
        state_reads(false, Duration::from_secs(2));
    }

    for attempt in 0..5 {
        fixture.ask("clear", "cleared");
        fixture.focus("plain");
        state_reads(false, Duration::from_secs(2));
        let intervals = on_intervals(Duration::from_millis(1_500), || {
            fixture.send("stall 300");
            fixture.reply("waiting", Duration::from_secs(5));
            fixture.keys(
                Duration::from_millis(2),
                &format!("{TAB} {} {RETURN}", codes(&LETTERS)),
            );
        });
        let state = fixture.state();
        let shortest = intervals.iter().min().copied();
        println!(
            "probe whole_password attempt={attempt} on_intervals={} shortest_on_us={} \
             secure={} plain={} focus={}",
            intervals.len(),
            shortest.map_or(-1, |ns| (ns / 1_000) as i64),
            state.secure,
            state.plain,
            state.focus
        );
    }
    drop(tap);
    drop(fixture);

    page_probe("safari", &["-a", "Safari"], "com.apple.Safari");
    let profile = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("stall-probe-chrome");
    let profile_arg = format!("--user-data-dir={}", profile.display());
    page_probe(
        "chrome",
        &[
            "-na",
            "Google Chrome",
            "--args",
            "--no-first-run",
            "--no-default-browser-check",
            &profile_arg,
        ],
        "com.google.Chrome",
    );
    println!("probe done");
}

#[test]
#[ignore = "a listed known fault of 51-macos-stall: D2 (MA-12 D3) held by stand-ins at the product's gap (ruling MA-22 L10)"]
fn known_fault_secure_input_on_for_less_than_the_spacing_of_two_readings_releases_the_keys() {
    let name =
        "known_fault_secure_input_on_for_less_than_the_spacing_of_two_readings_releases_the_keys";
    const KEYS: u64 = 8;
    let gap_ms = u64::try_from(GAP_BOUND.as_millis()).expect("the gap bound in ms");
    let hold_ms = u64::try_from(HOLD_BOUND.as_millis()).expect("the bound in ms");
    let both = |ms: u64| Stamps {
        before: Monotonic(ms * 1_000_000),
        after: Monotonic(ms * 1_000_000),
    };
    let first_tick = KEYS + 2;
    let on = (first_tick + 5, first_tick + gap_ms - 5);
    let read = |ms: u64| {
        if (on.0..on.1).contains(&ms) {
            Reading::On
        } else {
            Reading::Off
        }
    };
    let mut keys: HeldKeys<u64> = HeldKeys::new(Bounds::PRODUCT);
    let (mut released, mut dropped) = (0u64, 0usize);
    let mut settle = |keys: &mut HeldKeys<u64>, ms: u64| {
        let mut step = keys.reading(read(ms), both(ms));
        loop {
            match step {
                Step::Released(_) => {
                    released += 1;
                    if keys.is_empty() {
                        return;
                    }
                    step = keys.reading(read(ms), both(ms));
                }
                Step::Dropped { keys: count, .. } => {
                    dropped += count;
                    return;
                }
                Step::Waiting => return,
            }
        }
    };
    for key in 0..KEYS {
        settle(&mut keys, key);
        let _ = keys.admit(
            read(key),
            Timing::InTime {
                lag: Duration::ZERO,
            },
            Monotonic(key * 1_000_000),
            key,
        );
    }
    let mut ms = first_tick;
    while !keys.is_empty() && ms <= hold_ms + 2 * gap_ms + KEYS {
        settle(&mut keys, ms);
        ms += gap_ms;
    }
    println!(
        "{name} gap_ms={gap_ms} readings_ms_apart={gap_ms} on_ms={} keys={KEYS} released={released} dropped={dropped}",
        on.1 - on.0
    );
    assert_eq!(
        released,
        0,
        "{name}: recorded keys beside the markers in rounds that counted: D2 at the product's gap: {released} \
         of {KEYS} keys typed ahead into a field that was secure for {} ms between two readings of off {gap_ms} \
         ms apart were released",
        on.1 - on.0
    );
}

#[test]
#[ignore = "prints the form of the hosted phases for 51-macos-stall; it opens no window and posts no input"]
fn the_form_of_the_hosted_phases() {
    let waits: Vec<String> = stall_waits()
        .iter()
        .map(|(wait_ms, bound)| format!("{wait_ms}:{}", bound.as_millis()))
        .collect();
    println!(
        "form={} waits_ms_and_bounds_ms={} rounds_for_each_wait={} cap_for_each_wait={} \
         return_rounds_for_each_wait={RETURN_ROUNDS_FOR_EACH_WAIT} control_rounds={} known_fault_d1_rounds={}",
        if short_form() { "short" } else { "long" },
        waits.join(","),
        phase_rounds(),
        phase_cap(),
        control_rounds(),
        fault_rounds()
    );
    if short_form() {
        println!(
            "left_out counted rounds {} to {ROUNDS} of each wait of each mended phase",
            SHORT_ROUNDS_FOR_EACH_WAIT + 1
        );
        println!(
            "left_out {} of the {ROUNDS} rounds of the control",
            ROUNDS - SHORT_CONTROL_ROUNDS
        );
        println!(
            "left_out {} of the {FAULT_ROUNDS} rounds of the listed known fault D1",
            FAULT_ROUNDS - SHORT_FAULT_ROUNDS
        );
    }
}

fn panic_text(result: std::thread::Result<()>) -> Option<String> {
    result.err().map(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
            })
            .unwrap_or_default()
    })
}

fn held_round(wait_ms: u64) -> Round {
    Round {
        wait_ms,
        verdict: Some(Verdict::Held),
        ..stand_in_round(true, 0)
    }
}

type JudgeRow = (&'static str, fn(&mut Round), &'static str);

#[test]
fn each_judge_of_a_mended_phase_fails_it_at_one_round_for_each_wait() {
    let waits = stall_waits();
    let one_each: Vec<Round> = waits
        .iter()
        .map(|&(wait_ms, _)| held_round(wait_ms))
        .collect();
    let judged = |rounds: &[Round]| {
        panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || {
                judge_mended_in("stand-in", rounds, &waits, 1, true);
            },
        )))
    };
    assert_eq!(
        judged(&one_each),
        None,
        "one held round at each wait passes"
    );
    let rows: [JudgeRow; 7] = [
        (
            "a recorded key",
            |round| round.verdict = Some(Verdict::Recorded),
            "recorded keys beside the markers in rounds that counted",
        ),
        (
            "never on",
            |round| round.verdict = Some(Verdict::NeverOn),
            "never_on: the state did not read on",
        ),
        (
            "a round that did not count",
            |round| round.verdict = Some(Verdict::NotCounted("the markers were not recorded")),
            "rounds that did not count: wait",
        ),
        (
            "no proof",
            |round| round.verdict = Some(Verdict::NoProof),
            "counted rounds whose entries show no reading of on",
        ),
        (
            "a record of the markers other than one for each",
            |round| round.markers = MARKERS.len() + 1,
            "records of the markers other than one for each marker",
        ),
        (
            "the runner's gap not kept",
            |round| {
                round.verdict = Some(Verdict::NotCounted("the markers were not recorded"));
                round.entries = vec![arrived_at(10), gap_drop_at(40)];
            },
            "the runner did not keep the measured gap of",
        ),
        (
            "a reading of on outside every span",
            |round| {
                round.stamps = stamps_at(&ROUND);
                round.entries = vec![reading_at(500, 501, Reading::On)];
            },
            "readings of on outside every span of the round",
        ),
    ];
    for (what, change, text) in rows {
        let mut rounds: Vec<Round> = waits
            .iter()
            .map(|&(wait_ms, _)| held_round(wait_ms))
            .collect();
        change(&mut rounds[2]);
        let found = judged(&rounds);
        assert!(
            found.as_deref().is_some_and(|found| found.contains(text)),
            "{what}: {found:?}"
        );
    }
    let mut late_click: Vec<Round> = waits
        .iter()
        .map(|&(wait_ms, _)| held_round(wait_ms))
        .collect();
    late_click[2].verdict = Some(Verdict::LateClick);
    assert_eq!(
        judged(&late_click),
        None,
        "a wait with a late click and no held round fails nothing in the short form (MA-27 item 2)"
    );
    let long = panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || judge_mended_in("stand-in", &late_click, &waits, 1, false),
    )));
    assert!(
        long.as_deref()
            .is_some_and(|found| found.contains("not driven at wait 600 ms")),
        "a wait with no held round reads not driven in the long form: {long:?}"
    );
    assert!(
        judged(&[]).is_some_and(|found| found.contains("the phase ran no round")),
        "a phase that ran no round fails"
    );
    let control = |rounds: &[Control]| {
        panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || {
                judge_control("stand-in", rounds);
            },
        )))
    };
    assert_eq!(control(&[stand_in_control(true, 5, 0)]), None);
    assert!(control(&[stand_in_control(false, 5, 0)])
        .is_some_and(|found| found.contains("control rounds that did not count")));
    assert!(control(&[stand_in_control(true, 5, 1)])
        .is_some_and(|found| found.contains("three in the secure field")));
    assert!(control(&[]).is_some_and(|found| found.contains("the control ran no round")));
}

#[test]
fn the_waits_rounds_and_kept_time_of_both_forms_are_the_ruled_ones() {
    let second = Duration::from_millis(1_000);
    assert_eq!(
        stall_waits(),
        [
            (150, second),
            (300, second),
            (600, second),
            (850, second),
            (2_000, Duration::from_millis(3_000)),
        ],
        "the five waits of MA-12 F6 with their bounds, in both forms"
    );
    assert_eq!(
        (
            SHORT_ROUNDS_FOR_EACH_WAIT,
            SHORT_CAP_FOR_EACH_WAIT,
            SHORT_CONTROL_ROUNDS,
            SHORT_FAULT_ROUNDS,
            RETURN_ROUNDS_FOR_EACH_WAIT
        ),
        (1, 2, 1, 1, 1),
        "the short form of MA-23 Addition 1 (1)"
    );
    assert_eq!(
        (ROUNDS, ROUNDS + 1, FAULT_ROUNDS),
        (3, 4, 5),
        "the long form: three counted rounds for each wait under a cap of four, three controls, five rounds of D1"
    );
    assert_eq!(
        KEPT_MARGIN,
        Duration::from_millis(130),
        "the kept time of MA-19 (4)"
    );
}

const STAND_IN_WAIT: Duration = Duration::from_millis(300);

struct StandInRecords {
    keys: usize,
}

impl Records for StandInRecords {
    fn between(&self, _from: SystemTime, _to: SystemTime) -> usize {
        self.keys
    }

    fn take_entries(&self) -> Vec<RawEntry> {
        Vec::new()
    }
}

fn stand_in_fixture(case: &str, on_clear: &str) -> Fixture {
    stand_in_fixture_with(case, &format!("clear) {on_clear} ;;"))
}

fn stand_in_fixture_with(case: &str, answers: &str) -> Fixture {
    let body = format!(
        "exec 3>\"$2\" 4<\"$1\"\nprintf 'ready pid=%s\\n' \"$$\" >&3\n\
         while IFS= read -r line <&4; do\n  case $line in\n    {answers}\n    \
         quit) exit 0 ;;\n    *) printf 'unknown\\n' >&3 ;;\n  esac\ndone"
    );
    let fixture_script = stand_in(&format!("{case}-fixture"), &body);
    let opener = stand_in(
        &format!("{case}-opener"),
        &format!("\"{}\" \"$3\" \"$5\" &\nexit 0", fixture_script.display()),
    );
    let bundle = Path::new(env!("CARGO_TARGET_TMPDIR")).join("NoBundle.app");
    let mut fixture = launched(
        &opener,
        &bundle,
        &format!("stand-in-{case}-"),
        Duration::from_secs(5),
    )
    .expect("the stand-in fixture answers ready");
    fixture.wait = STAND_IN_WAIT;
    fixture
}

fn stand_in_line(name: &str, round: &Round, keep: Keep) -> String {
    let line = round_line(name, 0, round, Duration::ZERO, keep, Move::Tab);
    println!("{line}");
    line
}

fn judged_mended(rounds: &[Round]) -> Option<String> {
    let waits = stall_waits();
    panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || judge_mended_in("stand-in", rounds, &waits, 1, true),
    )))
}

fn held_at_each_wait() -> Vec<Round> {
    stall_waits()
        .iter()
        .map(|&(wait_ms, _)| held_round(wait_ms))
        .collect()
}

#[test]
fn a_round_whose_fixture_output_ends_before_a_reply_is_not_driven() {
    let mut fixture = stand_in_fixture("ended", "exit 0");
    let none = StandInRecords { keys: 0 };
    let first = round(
        &mut fixture,
        &none,
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    let line = stand_in_line("stand-in-ended", &first, Keep::Mended);
    assert!(line.contains(" counted=0 "), "{line}");
    assert!(
        line.ends_with(" not_driven=output_ended_before_cleared"),
        "{line}"
    );
    assert!(line.contains(" rv=notdriven "), "{line}");
    let second = round(
        &mut fixture,
        &none,
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    assert!(
        stand_in_line("stand-in-ended", &second, Keep::Mended)
            .ends_with(" not_driven=output_ended_before_cleared"),
        "a fixture whose output ended drives no later round"
    );
    let control = control_round(&mut fixture, &none, Duration::ZERO);
    let control_line = format!("{control:?}");
    println!("stand-in-ended control {control_line}");
    let mut beside = held_at_each_wait();
    beside.push(first);
    assert_eq!(
        judged_mended(&beside),
        None,
        "one round not driven beside a held round at each wait passes"
    );
    let every = judged_mended(&[second]);
    assert!(
        every
            .as_deref()
            .is_some_and(|text| text.contains("every round of the phase was not driven")),
        "{every:?}"
    );
    let controls = |rounds: &[Control]| {
        panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || judge_control("stand-in", rounds),
        )))
    };
    let alone = controls(std::slice::from_ref(&control));
    assert!(
        alone
            .as_deref()
            .is_some_and(|text| text.contains("every control round was not driven")),
        "{alone:?}"
    );
    assert_eq!(
        controls(&[stand_in_control(true, 5, 0), control]),
        None,
        "one control not driven beside a counted control passes"
    );
}

#[test]
fn a_round_whose_stamps_reply_does_not_come_is_not_driven() {
    let mut fixture = stand_in_fixture_with(
        "stamps-ended",
        "clear) printf 'cleared\\n' >&3 ;;\n    plain) printf 'focus=plain\\n' >&3 ;;\n    \
         state) printf 'front=1 key=1 focus=plain plain=0 secure=0\\n' >&3 ;;\n    \
         stamps) exit 0 ;;",
    );
    let round = round(
        &mut fixture,
        &StandInRecords { keys: 0 },
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    let line = stand_in_line("stand-in-stamps-ended", &round, Keep::Mended);
    assert!(line.contains(" counted=0 "), "{line}");
    assert!(
        line.ends_with(" not_driven=output_ended_before_stamps_n"),
        "{line}"
    );
    assert!(line.contains(" rv=notdriven "), "{line}");
}

#[test]
fn a_round_whose_reply_comes_after_its_wait_is_not_driven() {
    let mut fixture = stand_in_fixture("late", "sleep 1; printf 'cleared\\n' >&3");
    let round = round(
        &mut fixture,
        &StandInRecords { keys: 0 },
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    let line = stand_in_line("stand-in-late", &round, Keep::Mended);
    assert!(line.contains(" counted=0 "), "{line}");
    assert!(
        line.ends_with(" not_driven=late_reply_cleared_within_300_ms"),
        "{line}"
    );
    assert!(line.contains(" rv=notdriven "), "{line}");
    let rounds = [round];
    let (verdict, held, failed, not_driven) = three_way_verdict(&rounds);
    assert_eq!(
        (verdict, held, failed, not_driven),
        ("failed", 0, 0, 1),
        "a return phase whose every round was not driven fails"
    );
    let judged = panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || judge_three_ways("stand-in-late", &rounds),
    )));
    assert!(
        judged
            .as_deref()
            .is_some_and(|text| text.contains("every round of the phase was not driven")),
        "{judged:?}"
    );
}

fn not_driven_round(wait_ms: u64, ahead: usize) -> Round {
    Round {
        wait_ms,
        why_not: NOT_DRIVEN,
        secure: 0,
        verdict: Some(if ahead > 0 {
            Verdict::Recorded
        } else {
            Verdict::NotDriven
        }),
        not_driven: Some(NoReply::Late {
            reply: String::from("left_secure"),
            wait: Duration::from_secs(3),
        }),
        ..stand_in_round(false, ahead)
    }
}

type DrivenRow = (&'static str, Vec<Round>, Option<&'static str>);

#[test]
fn each_judge_reads_a_round_not_driven_by_its_rule_on_a_table() {
    let waits = stall_waits();
    let held_but_at = |at: usize, rounds: Vec<Round>| -> Vec<Round> {
        let mut all: Vec<Round> = waits
            .iter()
            .enumerate()
            .filter(|&(index, _)| index != at)
            .map(|(_, &(wait_ms, _))| held_round(wait_ms))
            .collect();
        all.extend(rounds);
        all
    };
    let outside = Round {
        stamps: stamps_at(&ROUND),
        entries: vec![reading_at(500, 501, Reading::On)],
        ..not_driven_round(600, 0)
    };
    let late_click = Round {
        verdict: Some(Verdict::LateClick),
        ..stand_in_round(false, 0)
    };
    let mended_rows: [DrivenRow; 6] = [
        (
            "a round not driven in place of the held round of one wait",
            held_but_at(2, vec![not_driven_round(600, 0)]),
            None,
        ),
        (
            "a round not driven with readings of on outside every span, beside held rounds",
            held_but_at(2, vec![outside]),
            None,
        ),
        (
            "every round not driven",
            waits
                .iter()
                .map(|&(wait_ms, _)| not_driven_round(wait_ms, 0))
                .collect(),
            Some("every round of the phase was not driven"),
        ),
        (
            "no held round: a late click and rounds not driven",
            vec![
                late_click,
                not_driven_round(150, 0),
                not_driven_round(300, 0),
                not_driven_round(600, 0),
                not_driven_round(850, 0),
                not_driven_round(2_000, 0),
            ],
            Some("no round of the phase was held"),
        ),
        (
            "a round not driven with a recorded key, beside held rounds",
            held_but_at(2, vec![not_driven_round(600, 2)]),
            Some("recorded keys beside the markers in rounds that counted or were not driven: wait 600 ms: ahead 2 (not driven: late_reply_left_secure_within_3000_ms)"),
        ),
        (
            "a round not driven is no round that did not count",
            vec![
                held_round(150),
                held_round(300),
                held_round(600),
                held_round(850),
                held_round(2_000),
                not_driven_round(300, 0),
            ],
            None,
        ),
    ];
    let mut wrong = Vec::new();
    for (what, rounds, want) in mended_rows {
        let got = judged_mended(&rounds);
        println!("mended row: {what}: want={want:?} got={got:?}");
        let right = match want {
            None => got.is_none(),
            Some(text) => got.is_some_and(|got| got.contains(text)),
        };
        if !right {
            wrong.push(what);
        }
    }
    let as_is = |rounds: &[Round]| {
        panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || judge("stand-in", rounds),
        )))
    };
    for (what, got, want) in [
        (
            "as is: a round not driven beside a counted round",
            as_is(&[stand_in_round(true, 0), not_driven_round(1_500, 0)]),
            None,
        ),
        (
            "as is: every round not driven",
            as_is(&[not_driven_round(1_500, 0)]),
            Some("every round of the phase was not driven"),
        ),
        (
            "as is: a round not driven with a recorded key",
            as_is(&[stand_in_round(true, 0), not_driven_round(1_500, 1)]),
            Some("recorded keys beside the markers in rounds that counted"),
        ),
    ] {
        println!("as-is row: {what}: want={want:?} got={got:?}");
        let right = match want {
            None => got.is_none(),
            Some(text) => got.is_some_and(|got| got.contains(text)),
        };
        if !right {
            wrong.push(what);
        }
    }
    let key_alone = Round {
        verdict: Some(Verdict::NotDriven),
        ..not_driven_round(300, 1)
    };
    for (what, rounds, want) in [
        (
            "three-way: a key ahead in a round not driven, no letter landed, not judged recorded",
            vec![stand_in_round(true, 0), key_alone],
            "failed",
        ),
        (
            "three-way: a round not driven with no key beside a counted round",
            vec![stand_in_round(true, 0), not_driven_round(300, 0)],
            "notdriven",
        ),
        (
            "three-way: every round not driven",
            vec![not_driven_round(300, 0), not_driven_round(600, 0)],
            "failed",
        ),
    ] {
        let (got, ..) = three_way_verdict(&rounds);
        println!("three-way row: {what}: want={want} got={got}");
        if got != want {
            wrong.push(what);
        }
    }
    let control = |rounds: &[Control]| {
        panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || judge_control("stand-in", rounds),
        )))
    };
    let not_driven_control = |secure: usize| Control {
        why_not: NOT_DRIVEN,
        not_driven: Some(NoReply::Ended {
            reply: String::from("focus="),
        }),
        ..stand_in_control(false, 5, secure)
    };
    for (what, got, want) in [
        (
            "control: a round not driven beside a counted round",
            control(&[stand_in_control(true, 5, 0), not_driven_control(0)]),
            None,
        ),
        (
            "control: a round not driven with a record of the secure field",
            control(&[stand_in_control(true, 5, 0), not_driven_control(1)]),
            Some("records of the secure field in control rounds not driven: 1 (not driven: output_ended_before_focus)"),
        ),
        (
            "control: every round not driven",
            control(&[not_driven_control(0)]),
            Some("every control round was not driven"),
        ),
    ] {
        println!("control row: {what}: want={want:?} got={got:?}");
        let right = match want {
            None => got.is_none(),
            Some(text) => got.is_some_and(|got| got.contains(text)),
        };
        if !right {
            wrong.push(what);
        }
    }
    let read = |ahead: usize, markers: usize, stamps: bool, judged: Option<Verdict>| Round {
        markers,
        stamps: if stamps {
            stamps_at(&ROUND)
        } else {
            Vec::new()
        },
        verdict: judged,
        ..not_driven_round(300, ahead)
    };
    for (what, round, want) in [
        (
            "verdict: no record and no stamps",
            read(0, 0, false, None),
            Verdict::NotDriven,
        ),
        (
            "verdict: a key ahead and no stamps, judged otherwise",
            read(1, 0, false, Some(Verdict::NotCounted(NOT_DRIVEN))),
            Verdict::Recorded,
        ),
        (
            "verdict: a key ahead as is, with the stamps read",
            read(1, 0, true, None),
            Verdict::Recorded,
        ),
        (
            "verdict: a key ahead excused by the stamps",
            read(1, 0, true, Some(Verdict::NotCounted(NOT_DRIVEN))),
            Verdict::NotDriven,
        ),
        (
            "verdict: a key ahead in a round judged not counted for another cause",
            read(
                1,
                0,
                true,
                Some(Verdict::NotCounted("the round's first stamp read on")),
            ),
            Verdict::Recorded,
        ),
        (
            "verdict: judged recorded with the stamps read and no key ahead",
            read(0, 0, true, Some(Verdict::Recorded)),
            Verdict::Recorded,
        ),
        (
            "verdict: more records after the markers than markers",
            read(
                0,
                MARKERS.len() + 1,
                true,
                Some(Verdict::NotCounted(NOT_DRIVEN)),
            ),
            Verdict::Recorded,
        ),
        (
            "verdict: the markers recorded once each",
            read(
                0,
                MARKERS.len(),
                true,
                Some(Verdict::NotCounted(NOT_DRIVEN)),
            ),
            Verdict::NotDriven,
        ),
    ] {
        let got = not_driven_verdict(&round);
        println!("verdict row: {what}: want={want:?} got={got:?}");
        if got != want {
            wrong.push(what);
        }
    }
    assert!(
        wrong.is_empty(),
        "rows that gave another verdict: {wrong:?}"
    );
}

#[test]
fn a_recorded_key_in_a_round_not_driven_fails_its_phase() {
    let mut fixture = stand_in_fixture("recorded", "exit 0");
    let one = StandInRecords { keys: 1 };
    let mended = round(
        &mut fixture,
        &one,
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    let line = stand_in_line("stand-in-recorded", &mended, Keep::Mended);
    assert!(line.contains(" ahead=1 "), "{line}");
    assert!(line.contains(" rv=recorded "), "{line}");
    let as_is = round(
        &mut fixture,
        &one,
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::AsIs,
    );
    let as_is_line = stand_in_line("stand-in-recorded", &as_is, Keep::AsIs);
    assert!(as_is_line.contains(" rv=recorded "), "{as_is_line}");
    let returned = round(
        &mut fixture,
        &one,
        Move::Return,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    assert_eq!(
        returned.secure, 0,
        "no letter was read in the secure field of the stand-in"
    );
    let (verdict, _, failed, _) = three_way_verdict(std::slice::from_ref(&returned));
    assert_eq!(
        (verdict, failed),
        ("failed", 1),
        "a recorded key in a return round not driven fails, with no letter read"
    );
    let mut beside = held_at_each_wait();
    beside.push(mended);
    let found = judged_mended(&beside);
    assert!(
        found
            .as_deref()
            .is_some_and(|text| text.contains("recorded keys beside the markers")),
        "{found:?}"
    );
    let as_is_judged = panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || judge("stand-in", &[as_is]),
    )));
    assert!(
        as_is_judged
            .as_deref()
            .is_some_and(|text| text.contains("recorded keys beside the markers")),
        "{as_is_judged:?}"
    );
}

#[test]
fn a_key_ahead_in_a_round_not_driven_whose_first_stamp_reads_on_fails_its_phase() {
    let stamps_line = "stamps n=2 lost=0 fr_secure:1:2000500:1 cmd_plain:0:3000000:0";
    let mut fixture = stand_in_fixture_with(
        "first-on",
        &format!("clear) : ;;\n    stamps) printf '{stamps_line}\\n' >&3 ;;"),
    );
    let not_driven = round(
        &mut fixture,
        &StandInRecords { keys: 1 },
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    let line = stand_in_line("stand-in-first-on", &not_driven, Keep::Mended);
    assert!(line.contains(" ahead=1 "), "{line}");
    assert!(
        line.ends_with(" not_driven=late_reply_cleared_within_300_ms"),
        "{line}"
    );
    let stamps = parse_stamps(stamps_line);
    let counted = Round {
        stamps: stamps.clone(),
        verdict: Some(mended_verdict(
            false,
            None,
            1,
            &evidence(&stamps, &[], 0),
            Duration::ZERO,
        )),
        ..stand_in_round(true, 1)
    };
    let mut counted_beside = held_at_each_wait();
    counted_beside.push(counted);
    assert!(
        judged_mended(&counted_beside).is_some(),
        "a counted round with this key ahead and these stamps fails its phase"
    );
    let mut beside = held_at_each_wait();
    beside.push(not_driven);
    let found = judged_mended(&beside);
    assert!(
        found.is_some(),
        "a key ahead in a round not driven whose first stamp reads on fails its phase as it \
         fails a counted round, but the phase passed: {line}"
    );
}

#[test]
fn the_late_reply_of_one_round_is_not_taken_as_the_reply_of_the_next() {
    let mut fixture = stand_in_fixture_with(
        "stale",
        "clear) sleep 0.5; printf 'cleared\\n' >&3 ;;\n    \
         plain) printf 'focus=plain\\n' >&3 ;;\n    \
         stamps) printf 'stamps n=0 lost=0\\n' >&3 ;;",
    );
    let none = StandInRecords { keys: 0 };
    let first = round(
        &mut fixture,
        &none,
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    let first_line = stand_in_line("stand-in-stale", &first, Keep::Mended);
    assert!(
        first_line.ends_with(" not_driven=late_reply_cleared_within_300_ms"),
        "{first_line}"
    );
    let second = round(
        &mut fixture,
        &none,
        Move::Tab,
        150,
        Duration::ZERO,
        Keep::Mended,
    );
    let second_line = stand_in_line("stand-in-stale", &second, Keep::Mended);
    assert!(
        second_line.ends_with(" not_driven=late_reply_cleared_within_300_ms"),
        "the second round took the first round's late 'cleared' as its own reply to 'clear' and \
         named the reply after it: {second_line}"
    );
}

const MARKERS_POSTED_AT: u64 = 4_000_000;

fn after_its_markers(
    no_reply: NoReply,
    ahead: usize,
    held_before: &[u64],
    marker_releases: usize,
) -> Round {
    let mut entries = vec![
        RawEntry::Arrival {
            at: 1_000,
            admission: Arrived::Held,
        },
        RawEntry::Dropped {
            at: 2_000,
            cause: Cause::On,
            keys: 1,
            unread: false,
        },
    ];
    for &at in held_before {
        entries.push(RawEntry::Arrival {
            at,
            admission: Arrived::Held,
        });
    }
    for index in 0..marker_releases as u64 {
        entries.push(RawEntry::Arrival {
            at: 5_000_000 + index,
            admission: Arrived::Held,
        });
    }
    for index in 0..(held_before.len() + marker_releases) as u64 {
        entries.push(RawEntry::Released {
            at: 1_005_000_000 + index,
        });
    }
    let found = evidence(&[], &entries, since_of(Move::Tab, &[]));
    let mut round = Round {
        counted: false,
        why_not: NOT_DRIVEN,
        markers: MARKERS.len(),
        plain: MARKERS.len(),
        entries,
        not_driven: Some(no_reply),
        markers_at: Some(MARKERS_POSTED_AT),
        ..stand_in_round(false, ahead)
    };
    round.verdict = Some(mended_verdict(
        false,
        Some(NOT_DRIVEN),
        ahead,
        &found,
        Duration::from_secs(1),
    ));
    round.verdict = Some(not_driven_verdict(&round));
    round
}

fn late_stamps() -> NoReply {
    NoReply::Late {
        reply: String::from("stamps n="),
        wait: ASK_WAIT,
    }
}

fn ended_stamps() -> NoReply {
    NoReply::Ended {
        reply: String::from("stamps n="),
    }
}

fn records_nothing_after_its_markers(name: &str, no_reply: NoReply) {
    let round = after_its_markers(no_reply, 0, &[], MARKERS.len());
    let line = stand_in_line(name, &round, Keep::Mended);
    assert_eq!(
        round.verdict,
        Some(Verdict::NotDriven),
        "the only keys released are the two markers of the plain field: {line}"
    );
    assert!(line.contains(" rv=notdriven "), "{line}");
    let mut beside = held_at_each_wait();
    beside.push(round);
    assert_eq!(
        judged_mended(&beside),
        None,
        "a round not driven after its markers fails no phase"
    );
}

#[test]
fn a_round_whose_last_stamps_reply_comes_late_after_its_markers_records_nothing() {
    records_nothing_after_its_markers("stand-in-late-stamps", late_stamps());
}

#[test]
fn a_round_whose_output_ended_before_its_last_stamps_reply_records_nothing() {
    records_nothing_after_its_markers("stand-in-ended-stamps", ended_stamps());
}

#[test]
fn a_round_not_driven_after_its_markers_reads_recorded_by_its_own_keys_alone() {
    let mut wrong = Vec::new();
    for (what, round, want) in [
        (
            "a key ahead",
            after_its_markers(late_stamps(), 1, &[], MARKERS.len()),
            Verdict::Recorded,
        ),
        (
            "a letter released that arrived before the markers were posted",
            after_its_markers(ended_stamps(), 0, &[3_000_000], MARKERS.len()),
            Verdict::Recorded,
        ),
        (
            "a letter released before the markers and one marker released, as many releases as markers",
            after_its_markers(late_stamps(), 0, &[3_000_000], MARKERS.len() - 1),
            Verdict::Recorded,
        ),
        (
            "more releases than markers",
            after_its_markers(late_stamps(), 0, &[], MARKERS.len() + 1),
            Verdict::Recorded,
        ),
        (
            "a release with no markers posted",
            Round {
                markers_at: None,
                verdict: Some(Verdict::NotCounted(NOT_DRIVEN)),
                ..after_its_markers(late_stamps(), 0, &[3_000_000], 0)
            },
            Verdict::Recorded,
        ),
        (
            "the markers alone",
            after_its_markers(ended_stamps(), 0, &[], MARKERS.len()),
            Verdict::NotDriven,
        ),
    ] {
        let got = if round.markers_at.is_none() {
            not_driven_verdict(&round)
        } else {
            round.verdict.unwrap_or(Verdict::NoProof)
        };
        println!("after-markers row: {what}: want={want:?} got={got:?}");
        if got != want {
            wrong.push(what);
        }
    }
    let mut beside = held_at_each_wait();
    beside.push(after_its_markers(late_stamps(), 1, &[], MARKERS.len()));
    let found = judged_mended(&beside);
    assert!(
        found.as_deref().is_some_and(|text| text.contains(
            "recorded keys beside the markers in rounds that counted or were not driven"
        )),
        "{found:?}"
    );
    assert!(
        wrong.is_empty(),
        "rows that gave another verdict: {wrong:?}"
    );
}

fn one_held_and_the_rest_not_driven() -> Vec<Round> {
    let mut rounds = vec![held_round(150)];
    for &(wait_ms, _) in &stall_waits()[1..] {
        rounds.push(not_driven_round(wait_ms, 0));
        rounds.push(not_driven_round(wait_ms, 0));
    }
    rounds
}

fn judged_in_form(rounds: &[Round], count: usize, short: bool) -> Option<String> {
    let waits = stall_waits();
    panic_text(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || judge_mended_in("stand-in", rounds, &waits, count, short),
    )))
}

#[test]
fn in_the_short_form_one_held_round_passes_a_phase_and_rounds_not_driven_are_printed_by_wait() {
    let rounds = one_held_and_the_rest_not_driven();
    assert_eq!(
        judged_in_form(&rounds, SHORT_ROUNDS_FOR_EACH_WAIT, true),
        None,
        "one held round and no failed round pass the short form"
    );
    assert_eq!(
        by_wait_line("stand-in", &rounds, &stall_waits()),
        "stand-in by_wait held=150:1,300:0,600:0,850:0,2000:0 \
         not_driven=150:0,300:2,600:2,850:2,2000:2"
    );
    let late_click = Round {
        wait_ms: 300,
        verdict: Some(Verdict::LateClick),
        ..stand_in_round(false, 0)
    };
    assert_eq!(
        judged_in_form(&[held_round(150), late_click], 1, true),
        None,
        "a wait with a late click and no held round fails nothing in the short form"
    );
    let found = judged_in_form(&[not_driven_round(150, 0), held_round(300)], 1, true);
    assert_eq!(found, None, "one held round beside one not driven passes");
    let none_held = Round {
        wait_ms: 300,
        verdict: Some(Verdict::LateClick),
        ..stand_in_round(false, 0)
    };
    let found = judged_in_form(&[not_driven_round(150, 0), none_held], 1, true);
    assert!(
        found
            .as_deref()
            .is_some_and(|text| text.contains("no round of the phase was held")),
        "{found:?}"
    );
}

#[test]
fn in_the_long_form_a_wait_with_no_held_round_reads_not_driven_and_takes_no_record() {
    let found = judged_in_form(&one_held_and_the_rest_not_driven(), ROUNDS, false);
    assert!(
        found.as_deref().is_some_and(|text| text
            .contains("not driven at wait 300 ms, wait 600 ms, wait 850 ms, wait 2000 ms")
            && text.contains("the long form takes no record from this run")),
        "{found:?}"
    );
    let held_at_each: Vec<Round> = stall_waits()
        .iter()
        .flat_map(|&(wait_ms, _)| {
            [
                held_round(wait_ms),
                not_driven_round(wait_ms, 0),
                not_driven_round(wait_ms, 0),
            ]
        })
        .collect();
    assert_eq!(
        judged_in_form(&held_at_each, ROUNDS, false),
        None,
        "a held round at every wait and rounds not driven beside it pass the long form"
    );
    let mut short_of_count = held_at_each;
    short_of_count.retain(|round| round.wait_ms != 600 || round.verdict == Some(Verdict::Held));
    short_of_count.push(Round {
        wait_ms: 600,
        verdict: Some(Verdict::LateClick),
        ..stand_in_round(false, 0)
    });
    let found = judged_in_form(&short_of_count, ROUNDS, false);
    assert!(
        found
            .as_deref()
            .is_some_and(|text| text
                .contains("rounds that did not count by the cap of rounds run: wait 600 ms")),
        "{found:?}"
    );
}

#[test]
fn a_phase_whose_every_round_was_not_driven_reads_not_driven_in_both_forms() {
    let every: Vec<Round> = stall_waits()
        .iter()
        .map(|&(wait_ms, _)| not_driven_round(wait_ms, 0))
        .collect();
    for (short, count) in [(true, SHORT_ROUNDS_FOR_EACH_WAIT), (false, ROUNDS)] {
        let found = judged_in_form(&every, count, short);
        assert!(
            found
                .as_deref()
                .is_some_and(|text| text.contains("every round of the phase was not driven")),
            "short={short}: {found:?}"
        );
    }
}
