//! The stall phases of the macOS keyboard source against a fixture program
//! with a secure field whose main thread waits before it takes events. Every
//! test here that posts keys or shows a window is ignored by default and
//! refuses to run unless the hosted step of a GitHub-hosted runner has set
//! `HALVO_STALL_FIXTURE` to the fixture's path and the fixture's window is in
//! front before the first key is posted. The one test that runs by default
//! drives the launch path with stand-ins for `open`, which open no window and
//! post no input. Every printed line holds names and numbers alone.
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
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
use halvo_capture::secure_input_macos::hold::{GAP_BOUND, HOLD_BOUND, READING_STEP};
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
const SHORT_WINDOW_AT_150: usize = 320;
const SHORT_WINDOW_AT_2000: usize = 62;
const SET_ON_TIMES: [i64; 7] = [0, 5, 10, 20, 40, 80, 200];
const SET_ON_TIME_ROUNDS: usize = 20;
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

fn seamed(bound: Duration, gap: Duration, step: Duration) -> (InputSource, DropCounts) {
    let drops = DropCounts::new();
    let source = InputSource::with_seam(
        ForegroundReader::nothing_in_front(),
        InputCounters::new(),
        HoldPlan::Never,
        StallSeam {
            bound,
            gap,
            step,
            session: session_reader().unwrap_or(read_session),
            drops: drops.clone(),
        },
    );
    (source, drops)
}

fn source_for(bound: Duration) -> (InputSource, DropCounts) {
    seamed(bound, test_gap(), test_step())
}

fn product_source(bound: Duration) -> (InputSource, DropCounts) {
    let _ = bound;
    seamed(HOLD_BOUND, GAP_BOUND, READING_STEP)
}

fn drops_line(drops: &DropCounts) -> String {
    let mut line: Vec<String> = drops
        .by_cause()
        .into_iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect();
    line.push(format!("widest_gap_us={}", drops.widest_gap().as_micros()));
    line.join(" ")
}

struct Fixture {
    child: Option<Child>,
    input: Box<dyn Write + Send>,
    lines: Receiver<String>,
    pid: i32,
    notes: Vec<String>,
    by: &'static str,
}

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

    fn reply(&mut self, prefix: &str, within: Duration) -> String {
        if let Some(at) = self.notes.iter().position(|note| note.starts_with(prefix)) {
            return self.notes.remove(at);
        }
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) if line.starts_with(prefix) => return line,
                Ok(line) => self.notes.push(line),
                Err(_) => panic!("the fixture gave no line that starts with {prefix} in time"),
            }
        }
    }

    fn ask(&mut self, command: &str, prefix: &str) -> String {
        self.send(command);
        self.reply(prefix, Duration::from_secs(10))
    }

    fn state(&mut self) -> FixtureState {
        FixtureState::parse(&self.ask("state", "front="))
    }

    fn in_front(&mut self) -> bool {
        let state = self.state();
        state.front && state.key && frontmost_pid() == Some(self.pid)
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

    fn focus(&mut self, field: &str) {
        self.ask(field, "focus=");
    }

    fn keys(&mut self, step: Duration, keys: &str) {
        self.send(&format!("keys {} {keys}", step.as_millis()));
        self.reply("posted=", Duration::from_secs(20));
    }

    fn stamps(&mut self) -> Vec<Stamp> {
        parse_stamps(&self.ask("stamps", "stamps n="))
    }

    fn click(&mut self, at: &str) {
        self.send(&format!("click {at}"));
        self.reply("clicked", Duration::from_secs(10));
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
    let line = fixture.ask("where", "x=");
    let mut point = (String::new(), String::new());
    for word in line.split(' ') {
        match word.split_once('=') {
            Some(("x", v)) => point.0 = v.to_owned(),
            Some(("y", v)) => point.1 = v.to_owned(),
            _ => {}
        }
    }
    format!("{} {}", point.0, point.1)
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

    fn between(&self, from: SystemTime, to: SystemTime) -> usize {
        self.keys
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|&&at| at >= from && at < to)
            .count()
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
    case: Case,
    verdict: Option<Verdict>,
}

fn return_start(stamps: &[Stamp]) -> u64 {
    stamps
        .iter()
        .position(|stamp| stamp.kind == "cmd_away")
        .and_then(|away| stamps[away..].iter().find(|stamp| !stamp.on))
        .map_or(0, |stamp| stamp.ns)
}

fn keep_the_field(fixture: &mut Fixture, how: Move) -> bool {
    if !state_reads(true, NEVER_ON_WAIT) {
        return false;
    }
    let stamps = fixture.stamps();
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
    true
}

fn letter_step(wait_ms: u64) -> Duration {
    Duration::from_millis((wait_ms.saturating_sub(60) / 9).max(10))
}

fn round(
    fixture: &mut Fixture,
    recorder: &Recorder,
    how: Move,
    wait_ms: u64,
    bound: Duration,
    keep: Keep,
) -> Round {
    let _ = recorder.drops.take_entries();
    let from = raw_ns();
    let mut round = round_inner(fixture, recorder, how, wait_ms, bound, keep);
    round.from = from;
    round.stamps = fixture.stamps();
    round.entries = recorder.drops.take_entries();
    let found = evidence(on_runs(&round.stamps), &round.entries);
    round.case = short_case(&found);
    if keep == Keep::Mended {
        round.verdict = Some(mended_verdict(
            round.never_on,
            (!round.counted).then_some(round.why_not),
            round.ahead,
            &found,
        ));
    }
    round
}

fn round_inner(
    fixture: &mut Fixture,
    recorder: &Recorder,
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
        case: Case::C,
        verdict: None,
    };
    fixture.ask("clear", "cleared");
    if how == Move::Return {
        fixture.focus("secure");
        if !state_reads(true, Duration::from_secs(2)) {
            round.why_not = "the secure field did not turn secure input on";
            return round;
        }
        if fixture.ask("away", "away=") != "away=1" {
            round.why_not = "no Finder to yield the front to";
            return round;
        }
        if !until(Duration::from_secs(3), || {
            frontmost_pid() != Some(fixture.pid) && !secure_input_active()
        }) {
            round.why_not = "the fixture kept the front or secure input after it yielded";
            return round;
        }
    } else {
        fixture.focus("plain");
        if !state_reads(false, Duration::from_secs(2)) {
            round.why_not = "secure input did not read off before the round";
            return round;
        }
        if !fixture.in_front() {
            round.why_not = "the fixture was not in front before the first key";
            return round;
        }
    }
    let centre = matches!(how, Move::Click | Move::Return).then(|| secure_field_centre(fixture));
    let whole = matches!(how, Move::WholePassword | Move::WholePasswordBurst);
    let step = if how == Move::WholePasswordBurst {
        BURST_STEP
    } else {
        letter_step(wait_ms)
    };
    let ahead_from = SystemTime::now();
    let command = if how == Move::Itself {
        format!("stall {wait_ms} self")
    } else {
        format!("stall {wait_ms}")
    };
    fixture.send(&command);
    fixture.reply("waiting", Duration::from_secs(5));
    let letters = codes(&LETTERS);
    match how {
        Move::Tab | Move::LeavingKeyUp => fixture.keys(step, &format!("{TAB} {letters}")),
        Move::WholePassword | Move::WholePasswordBurst => {
            fixture.keys(step, &format!("{TAB} {letters} {RETURN}"));
        }
        Move::Click => {
            fixture.click(&centre.expect("the centre of the secure field"));
            fixture.keys(step, &letters);
        }
        Move::Return => {
            fixture.click(&centre.expect("the centre of the secure field"));
            fixture.keys(step, &letters);
        }
        Move::Itself => fixture.keys(step, &letters),
    }
    fixture.reply(
        "woke",
        Duration::from_millis(wait_ms) + Duration::from_secs(5),
    );
    let landed = until(Duration::from_secs(3), || {
        let state = fixture.state();
        round.secure = state.secure;
        state.secure == LETTERS.len()
            && (whole || keep == Keep::Autoleave || state.focus == "secure")
            && (how != Move::Return || state.front)
    });
    round.which = fixture
        .ask(&format!("which {LETTER_TEXT}"), "which=")
        .trim_start_matches("which=")
        .to_owned();
    if !landed {
        std::thread::sleep(bound + MARGIN);
        round.ahead = recorder.between(ahead_from, SystemTime::now());
        round.why_not = "the letters did not land in the secure field";
        return round;
    }
    if keep == Keep::Mended && !keep_the_field(fixture, how) {
        round.never_on = true;
        round.why_not = "never_on";
        std::thread::sleep(bound + MARGIN);
        round.ahead = recorder.between(ahead_from, SystemTime::now());
        fixture.focus("plain");
        return round;
    }
    if how == Move::LeavingKeyUp {
        fixture.keys(Duration::ZERO, &format!("{RETURN}d"));
        fixture.reply("left_secure", Duration::from_secs(3));
        if !state_reads(false, Duration::from_secs(2)) {
            round.why_not = "secure input did not read off after the leaving key";
            std::thread::sleep(bound + MARGIN);
            round.ahead = recorder.between(ahead_from, SystemTime::now());
            return round;
        }
        fixture.keys(Duration::ZERO, &format!("{RETURN}u"));
    } else if !whole {
        fixture.focus("plain");
    }
    if !state_reads(false, Duration::from_secs(2)) {
        round.why_not = "secure input did not read off before the markers";
        std::thread::sleep(bound + MARGIN);
        round.ahead = recorder.between(ahead_from, SystemTime::now());
        return round;
    }
    let markers_from = SystemTime::now();
    fixture.keys(Duration::from_millis(30), &codes(&MARKERS));
    std::thread::sleep(bound + MARGIN);
    let state = fixture.state();
    round.plain = state.plain;
    round.ahead = recorder.between(ahead_from, markers_from);
    round.markers = recorder.between(markers_from, SystemTime::now());
    if state.plain != MARKERS.len() {
        round.why_not = "the markers did not land in the plain field";
    } else if round.markers < MARKERS.len() {
        round.why_not = "the markers were not recorded";
    } else {
        round.counted = true;
    }
    round
}

fn started_fixture(name: &str) -> Fixture {
    println!("{name} {}", session_line());
    let (fixture, front) = Fixture::start().unwrap_or_else(|why| panic!("{name}: {why}"));
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

fn round_line(name: &str, number: usize, round: &Round, bound: Duration, keep: Keep) -> String {
    let found = evidence(on_runs(&round.stamps), &round.entries);
    let (inside_on, inside_off) = inside(&found);
    let runs: Vec<String> = found
        .on_runs
        .iter()
        .map(|&(on_from, on_to)| {
            format!(
                "{}..{}",
                relative_us(on_from, round.from),
                relative_us(on_to, round.from)
            )
        })
        .collect();
    let off = found
        .on_runs
        .first()
        .and_then(|&(_, on_to)| first_off_after(&round.stamps, on_to))
        .map_or_else(
            || String::from("-"),
            |at| relative_us(at, round.from).to_string(),
        );
    let release = found.first_release.map_or_else(
        || String::from("-"),
        |at| relative_us(at, round.from).to_string(),
    );
    let verdict = round.verdict.map_or_else(
        || String::from("-"),
        |verdict| format!("{verdict:?}").to_lowercase(),
    );
    format!(
        "{name} round={number} wait_ms={} bound_ms={} counted={} ahead={} markers={} plain={} \
         secure={} which={} keep={keep:?} never_on={} on_runs_us={} off_us={off} \
         inside_on={inside_on} inside_off={inside_off} first_release_us={release} \
         on_drops={} case={:?} rv={verdict}",
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
        found.on_drops,
        round.case,
    )
}

fn whole_entries(round: &Round) -> bool {
    round.ahead > 0
        || !round.counted
        || round
            .verdict
            .is_some_and(|verdict| verdict != Verdict::Held)
}

fn print_entries(name: &str, number: usize, round: &Round) {
    let runs = on_runs(&round.stamps);
    let window = if whole_entries(round) {
        None
    } else {
        runs.first().map(|&(on_from, on_to)| {
            let off = first_off_after(&round.stamps, on_to).unwrap_or(on_to);
            (
                on_from.saturating_sub(WINDOW_MARGIN),
                off.saturating_add(WINDOW_MARGIN),
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
        "{name} gap_ms={} step_ms={}",
        test_gap().as_millis(),
        test_step().as_millis()
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
            println!("{}", round_line(name, number, &round, bound, plan.keep));
            println!(
                "{name} drops round={number} {}",
                drops_line(&recorder.drops)
            );
            print_entries(name, number, &round);
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
    phase_with(name, how, waits, source_for, mended(ROUNDS, ROUNDS + 1))
}

fn judge_mended(name: &str, rounds: &[Round], waits: &[(u64, Duration)], count: usize) {
    let of = |want: fn(Verdict) -> bool| -> Vec<String> {
        rounds
            .iter()
            .filter(|round| round.verdict.is_some_and(want))
            .map(|round| format!("wait {} ms: ahead {}", round.wait_ms, round.ahead))
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
        "{name}: recorded keys beside the markers in rounds that counted: {}",
        recorded.join(", ")
    );
    let never_on = of(|verdict| verdict == Verdict::NeverOn);
    assert!(
        never_on.is_empty(),
        "{name}: never_on: the state did not read on within 2 s of the landing: {}",
        never_on.join(", ")
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
    let short: Vec<String> = waits
        .iter()
        .filter_map(|&(wait_ms, _)| {
            let held = rounds
                .iter()
                .filter(|round| round.wait_ms == wait_ms && round.verdict == Some(Verdict::Held))
                .count();
            (held < count).then(|| format!("wait {wait_ms} ms: {held} of {count}"))
        })
        .collect();
    assert!(
        short.is_empty(),
        "{name}: rounds that did not count by the cap of rounds run: {}",
        short.join(", ")
    );
}

fn measured(name: &str, form: &str, rounds: &[Round]) {
    let count = |case: Case| rounds.iter().filter(|round| round.case == case).count();
    let recorded: Vec<&Round> = rounds
        .iter()
        .filter(|round| round.counted && round.ahead > 0)
        .collect();
    let uncounted_with_keys = rounds
        .iter()
        .filter(|round| !round.counted && round.ahead > 0)
        .count();
    let recorded_case = |case: Case| recorded.iter().filter(|round| round.case == case).count();
    println!(
        "{name} measured form={form} wait_ms={} rounds={} counted={} recorded_rounds={} \
         recorded_cases=A:{},B:{},C:{},D:{},seen:{} uncounted_with_keys={uncounted_with_keys} \
         cases=A:{},B:{},C:{},D:{},seen:{}",
        rounds.first().map_or(0, |round| round.wait_ms),
        rounds.len(),
        rounds.iter().filter(|round| round.counted).count(),
        recorded.len(),
        recorded_case(Case::A),
        recorded_case(Case::B),
        recorded_case(Case::C),
        recorded_case(Case::D),
        recorded_case(Case::Seen),
        count(Case::A),
        count(Case::B),
        count(Case::C),
        count(Case::D),
        count(Case::Seen),
    );
}

fn judge(name: &str, rounds: &[Round]) {
    let leaked: Vec<String> = rounds
        .iter()
        .filter(|round| round.counted && round.ahead > 0)
        .map(|round| format!("wait {} ms: {}", round.wait_ms, round.ahead))
        .collect();
    assert!(
        leaked.is_empty(),
        "{name}: recorded keys beside the markers in rounds that counted: {}",
        leaked.join(", ")
    );
    let not_counted: Vec<String> = rounds
        .iter()
        .filter(|round| !round.counted)
        .map(|round| format!("wait {} ms: {}", round.wait_ms, round.why_not))
        .collect();
    assert!(
        not_counted.is_empty(),
        "{name}: rounds that did not count: {}",
        not_counted.join(", ")
    );
    let inexact: Vec<String> = rounds
        .iter()
        .filter(|round| round.markers != MARKERS.len())
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
    judge_mended(name, &phase(name, Move::Tab, &waits), &waits, ROUNDS);
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
            as_is(FAULT_ROUNDS),
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
fn short_window_b_a_set_on_time() {
    let name = "short_window_b_a_set_on_time";
    let mut fixture = started_fixture(name);
    for on_time in SET_ON_TIMES {
        fixture.ask(&format!("autoleave {on_time}"), "autoleave=");
        let (rounds, _) = phase_on(
            &mut fixture,
            name,
            Move::Click,
            &[(150, SHORT_BOUND)],
            source_for,
            Plan {
                count: SET_ON_TIME_ROUNDS,
                cap: SET_ON_TIME_ROUNDS,
                keep: Keep::Autoleave,
            },
        );
        measured(name, &format!("b_{on_time}ms"), &rounds);
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
    judge_mended(name, &phase(name, Move::Click, &waits), &waits, ROUNDS);
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded() {
    let name = "a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded";
    let waits = stall_waits();
    judge_mended(name, &phase(name, Move::Itself, &waits), &waits, ROUNDS);
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_on_a_return_to_a_secure_field_is_not_recorded() {
    let name = "a_key_typed_on_a_return_to_a_secure_field_is_not_recorded";
    judge_three_ways(
        name,
        &phase_with(name, Move::Return, &stall_waits(), source_for, mended(1, 1)),
    );
}

fn judge_three_ways(name: &str, rounds: &[Round]) {
    let failed = rounds.iter().filter(|r| r.counted && r.ahead > 0).count();
    let held = rounds.iter().filter(|r| r.counted && r.ahead == 0).count();
    let not_driven = rounds.len() - failed - held;
    for round in rounds.iter().filter(|round| !round.counted) {
        println!(
            "{name} notdriven wait_ms={} why={}",
            round.wait_ms,
            round.why_not.replace(' ', "_")
        );
    }
    let verdict = match (failed, not_driven) {
        (0, 0) => "held",
        (0, _) => "notdriven",
        _ => "failed",
    };
    println!("{name} verdict={verdict} held={held} failed={failed} notdriven={not_driven}");
    let inexact: Vec<String> = rounds
        .iter()
        .filter(|round| round.counted && round.markers != MARKERS.len())
        .map(|round| format!("wait {} ms: {}", round.wait_ms, round.markers))
        .collect();
    assert!(
        failed == 0,
        "{name}: recorded keys beside the markers in rounds that counted: {failed} rounds"
    );
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
        ROUNDS,
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
}

fn control_round(fixture: &mut Fixture, recorder: &Recorder, bound: Duration) -> Control {
    let mut control = Control {
        counted: false,
        why_not: "",
        landed_plain: 0,
        landed_secure: 0,
        plain: 0,
        secure: 0,
        after: 0,
    };
    fixture.ask("clear", "cleared");
    fixture.focus("plain");
    if !state_reads(false, Duration::from_secs(2)) || !fixture.in_front() {
        control.why_not = "the fixture was not in front with secure input off";
        return control;
    }
    let plain_from = SystemTime::now();
    fixture.keys(Duration::from_millis(30), &codes(&LETTERS[..5]));
    std::thread::sleep(bound + MARGIN);
    let secure_from = SystemTime::now();
    control.plain = recorder.between(plain_from, secure_from);
    fixture.focus("secure");
    if !state_reads(true, Duration::from_secs(2)) {
        control.why_not = "the secure field did not turn secure input on";
        return control;
    }
    fixture.keys(Duration::from_millis(30), &codes(&LETTERS[5..]));
    fixture.focus("plain");
    if !state_reads(false, Duration::from_secs(2)) {
        control.why_not = "secure input did not read off after the secure field";
        return control;
    }
    let after_from = SystemTime::now();
    fixture.keys(Duration::from_millis(30), &codes(&LETTERS[..3]));
    std::thread::sleep(bound + MARGIN);
    control.secure = recorder.between(secure_from, after_from);
    control.after = recorder.between(after_from, SystemTime::now());
    let state = fixture.state();
    control.landed_plain = state.plain;
    control.landed_secure = state.secure;
    if state.plain != 8 || state.secure != 3 {
        control.why_not = "the letters did not land where they were typed";
        return control;
    }
    control.counted = true;
    control
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
        println!(
            "{name} control={number} bound_ms={} counted={} landed_plain={} landed_secure={} \
             plain_records={} secure_records={} after_records={}",
            bound.as_millis(),
            u8::from(control.counted),
            control.landed_plain,
            control.landed_secure,
            control.plain,
            control.secure,
            control.after
        );
        println!(
            "{name} drops control={number} {}",
            drops_line(&recorder.drops)
        );
        rounds.push(control);
    }
    let drops = recorder.drops.clone();
    (rounds, drops)
}

fn control_phase(name: &str, bound: Duration, source: (InputSource, DropCounts), count: usize) {
    let mut fixture = started_fixture(name);
    let (rounds, _) = control_rounds_on(&mut fixture, name, bound, source, count);
    let not_counted: Vec<&str> = rounds
        .iter()
        .filter(|control| !control.counted)
        .map(|control| control.why_not)
        .collect();
    assert!(
        not_counted.is_empty(),
        "{name}: control rounds that did not count: {}",
        not_counted.join(", ")
    );
    for control in &rounds {
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
    control_phase(name, SHORT_BOUND, source_for(SHORT_BOUND), ROUNDS);
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

fn product_verdict(rounds: &[Round], controls: &[Control], gap_drops: u64) -> &'static str {
    let failed = rounds.iter().any(|round| round.counted && round.ahead > 0)
        || controls
            .iter()
            .any(|control| control.counted && control.secure > 0);
    let held = rounds
        .iter()
        .all(|round| round.counted && round.markers == MARKERS.len())
        && controls
            .iter()
            .all(|control| control.counted && (control.plain, control.after) == (5, 3));
    if failed {
        "failed"
    } else if held {
        "held"
    } else if gap_drops > 0 {
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
    let verdict = product_verdict(&rounds, &controls, gap_drops);
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
        case: Case::C,
        verdict: None,
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
    let counted = [stand_in_round(true, 0)];
    let not_counted = [stand_in_round(false, 0)];
    let control = [stand_in_control(true, 5, 0)];
    assert_eq!(product_verdict(&counted, &control, 0), "held");
    assert_eq!(product_verdict(&not_counted, &control, 6), "notkept");
    assert_eq!(product_verdict(&not_counted, &control, 0), "failed");
    assert_eq!(
        product_verdict(&[stand_in_round(true, 1)], &control, 6),
        "failed"
    );
    assert_eq!(
        product_verdict(&not_counted, &[stand_in_control(true, 5, 1)], 6),
        "failed"
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Evidence {
    on_runs: Vec<(u64, u64)>,
    readings: Vec<(u64, u64, Reading)>,
    first_release: Option<u64>,
    on_drops: usize,
}

fn evidence(runs: Vec<(u64, u64)>, entries: &[RawEntry]) -> Evidence {
    let mut found = Evidence {
        on_runs: runs,
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
                cause: Cause::On, ..
            } => found.on_drops += 1,
            _ => {}
        }
    }
    found
}

fn inside(found: &Evidence) -> (usize, usize) {
    let within = |before: u64, after: u64| {
        found
            .on_runs
            .iter()
            .any(|&(from, to)| before >= from && after <= to)
    };
    let mut counts = (0, 0);
    for &(before, after, value) in &found.readings {
        if within(before, after) {
            match value {
                Reading::On => counts.0 += 1,
                Reading::Off => counts.1 += 1,
                Reading::Unknown => {}
            }
        }
    }
    counts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    A,
    B,
    C,
    D,
    Seen,
}

fn short_case(found: &Evidence) -> Case {
    let Some(&(first_on, _)) = found.on_runs.first() else {
        return Case::C;
    };
    let (on_inside, off_inside) = inside(found);
    if off_inside > 0 {
        Case::D
    } else if found
        .first_release
        .is_some_and(|release| first_on > release)
    {
        Case::B
    } else if on_inside == 0 {
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
}

fn mended_verdict(
    never_on: bool,
    why_not: Option<&'static str>,
    ahead: usize,
    found: &Evidence,
) -> Verdict {
    if never_on {
        return Verdict::NeverOn;
    }
    let first_on = found.on_runs.first().map(|&(from, _)| from);
    if let (Some(on), Some(release)) = (first_on, found.first_release) {
        if on > release {
            return Verdict::LateClick;
        }
    }
    if let Some(why) = why_not {
        return Verdict::NotCounted(why);
    }
    if ahead > 0 {
        return Verdict::Recorded;
    }
    if inside(found).0 > 0 && found.on_drops > 0 {
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
            RawEntry::Dropped { at, cause, keys } => {
                let fate = format!("dropped_{cause:?}").to_lowercase();
                resolve(&mut fates, &mut held, keys, fate, at);
            }
            RawEntry::Reading { .. } => {}
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
    vec![
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

fn table_evidence(runs: &[(u64, u64)], entries: &[RawEntry]) -> Evidence {
    evidence(
        runs.iter()
            .map(|&(from, to)| (ms_ns(from), ms_ns(to)))
            .collect(),
        entries,
    )
}

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
    let on_drop = RawEntry::Dropped {
        at: ms_ns(120),
        cause: Cause::On,
        keys: 8,
    };
    let seen = table_evidence(&[(100, 400)], &[reading_at(110, 111, Reading::On), on_drop]);
    let rows: [MendedRow; 9] = [
        (
            "a reading of on inside the on-time and a drop for on",
            false,
            None,
            0,
            seen.clone(),
            Verdict::Held,
        ),
        (
            "no reading of on inside the on-time",
            false,
            None,
            0,
            table_evidence(
                &[(100, 400)],
                &[
                    reading_at(90, 95, Reading::Off),
                    reading_at(450, 451, Reading::On),
                    on_drop,
                ],
            ),
            Verdict::NoProof,
        ),
        (
            "a reading of on that straddles the start of the on-time",
            false,
            None,
            0,
            table_evidence(&[(100, 400)], &[reading_at(99, 101, Reading::On), on_drop]),
            Verdict::NoProof,
        ),
        (
            "a reading of on inside and no drop for on in the round",
            false,
            None,
            0,
            table_evidence(&[(100, 400)], &[reading_at(110, 111, Reading::On)]),
            Verdict::NoProof,
        ),
        (
            "the test's read never said on",
            true,
            None,
            0,
            seen.clone(),
            Verdict::NeverOn,
        ),
        (
            "the first stamp of on after the first release",
            false,
            None,
            8,
            table_evidence(
                &[(5_000, 5_400)],
                &[
                    RawEntry::Released { at: ms_ns(3_011) },
                    reading_at(5_010, 5_011, Reading::On),
                ],
            ),
            Verdict::LateClick,
        ),
        (
            "the first stamp of on before the first release",
            false,
            None,
            1,
            table_evidence(
                &[(100, 400)],
                &[
                    RawEntry::Released { at: ms_ns(3_011) },
                    reading_at(110, 111, Reading::On),
                    on_drop,
                ],
            ),
            Verdict::Recorded,
        ),
        (
            "a key recorded in a round that kept the on-time",
            false,
            None,
            2,
            seen.clone(),
            Verdict::Recorded,
        ),
        (
            "a round that did not count for a cause of F6",
            false,
            Some("the markers were not recorded"),
            0,
            seen,
            Verdict::NotCounted("the markers were not recorded"),
        ),
    ];
    for (what, never_on, why_not, ahead, found, want) in rows {
        assert_eq!(
            mended_verdict(never_on, why_not, ahead, &found),
            want,
            "{what}"
        );
    }
}

#[test]
fn the_case_of_a_short_window_round_holds_each_rule_on_a_table_of_entries() {
    let rows: [(&str, Evidence, Case); 6] = [
        (
            "no on-time at all",
            table_evidence(&[], &[reading_at(10, 11, Reading::Off)]),
            Case::C,
        ),
        (
            "no reading inside the on-time",
            table_evidence(
                &[(100, 130)],
                &[
                    reading_at(90, 95, Reading::Off),
                    reading_at(140, 145, Reading::Off),
                ],
            ),
            Case::A,
        ),
        (
            "a reading that straddles the end of the on-time",
            table_evidence(&[(100, 130)], &[reading_at(125, 135, Reading::Off)]),
            Case::A,
        ),
        (
            "an on-time that begins after the first release",
            table_evidence(
                &[(5_000, 5_300)],
                &[
                    RawEntry::Released { at: ms_ns(3_011) },
                    reading_at(5_010, 5_011, Reading::On),
                ],
            ),
            Case::B,
        ),
        (
            "a reading of off with both stamps inside the on-time",
            table_evidence(
                &[(100, 130)],
                &[
                    reading_at(110, 112, Reading::Off),
                    reading_at(115, 116, Reading::On),
                ],
            ),
            Case::D,
        ),
        (
            "a reading of on inside the on-time",
            table_evidence(&[(100, 130)], &[reading_at(110, 112, Reading::On)]),
            Case::Seen,
        ),
    ];
    for (what, found, want) in rows {
        assert_eq!(short_case(&found), want, "{what}");
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
