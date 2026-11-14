//! The stall phases of the macOS keyboard source against a fixture program
//! with a secure field whose main thread waits before it takes events. Every
//! test here posts keys and shows a window: each is ignored by default and
//! refuses to run unless the hosted step of a GitHub-hosted runner has set
//! `HALVO_STALL_FIXTURE` to the fixture's path and the fixture's window is in
//! front before the first key is posted. Every printed line holds names and
//! numbers alone.
#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType};
use halvo_capture::event::Modality;
use halvo_capture::macos::foreground::ForegroundReader;
use halvo_capture::macos::input::{DropCounts, HoldPlan, InputCounters, InputSource, StallSeam};
use halvo_capture::secure_input_macos::hold::{GAP_BOUND, HOLD_BOUND, READING_STEP};
use halvo_capture::secure_input_macos::{read_session, secure_input_active, Session};
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

fn uptime_ns() -> u64 {
    // SAFETY: clock_gettime_nsec_np takes a clock id by value and returns a
    // number; it touches no memory of the caller.
    unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) }
}

fn hosted_fixture() -> std::path::PathBuf {
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

fn ready_pid(lines: &Receiver<String>) -> Option<i32> {
    lines
        .recv_timeout(Duration::from_secs(20))
        .ok()?
        .strip_prefix("ready pid=")?
        .parse()
        .ok()
}

fn fifo(name: &str) -> std::path::PathBuf {
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

fn bundle_of(binary: &std::path::Path) -> std::path::PathBuf {
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

impl Fixture {
    fn spawned() -> Self {
        let mut child = Command::new(hosted_fixture())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the fixture starts");
        let input = child.stdin.take().expect("the fixture's input");
        let lines = lines_of(child.stdout.take().expect("the fixture's output"));
        let pid = ready_pid(&lines).expect("the fixture says that it is ready within 20 s");
        Self {
            child: Some(child),
            input: Box::new(input),
            lines,
            pid,
            notes: Vec::new(),
            by: "spawned",
        }
    }

    fn opened() -> Self {
        let bundle = bundle_of(&hosted_fixture());
        let to_fixture = fifo("in");
        let from_fixture = fifo("out");
        let status = Command::new("/usr/bin/open")
            .arg("-n")
            .arg("--stdin")
            .arg(&to_fixture)
            .arg("--stdout")
            .arg(&from_fixture)
            .arg(&bundle)
            .status()
            .expect("open runs");
        assert!(status.success(), "open launched the fixture's bundle");
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let output = std::fs::File::open(&from_fixture);
            let input = std::fs::OpenOptions::new().write(true).open(&to_fixture);
            let _ = tx.send((output, input));
        });
        let (output, input) = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the fixture opens its fifos within 20 s");
        let lines = lines_of(output.expect("the fixture's output fifo"));
        let pid = ready_pid(&lines).expect("the fixture says that it is ready within 20 s");
        Self {
            child: None,
            input: Box::new(input.expect("the fixture's input fifo")),
            lines,
            pid,
            notes: Vec::new(),
            by: "opened",
        }
    }

    fn start() -> (Self, Option<&'static str>) {
        let mut fixture = Self::spawned();
        if let Some(how) = fixture.bring_to_front(Duration::from_secs(5)) {
            return (fixture, Some(how));
        }
        drop(fixture);
        let mut fixture = Self::opened();
        let how = fixture.bring_to_front(Duration::from_secs(5));
        (fixture, how)
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
) -> Round {
    let mut round = Round {
        wait_ms,
        counted: false,
        why_not: "",
        ahead: 0,
        markers: 0,
        plain: 0,
        secure: 0,
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
    let step = letter_step(wait_ms);
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
        Move::WholePassword => fixture.keys(step, &format!("{TAB} {letters} {RETURN}")),
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
            && (how == Move::WholePassword || state.focus == "secure")
            && (how != Move::Return || state.front)
    });
    if !landed {
        round.why_not = "the letters did not land in the secure field";
        return round;
    }
    if how == Move::LeavingKeyUp {
        fixture.keys(Duration::ZERO, &format!("{RETURN}d"));
        fixture.reply("left_secure", Duration::from_secs(3));
        if !state_reads(false, Duration::from_secs(2)) {
            round.why_not = "secure input did not read off after the leaving key";
            return round;
        }
        fixture.keys(Duration::ZERO, &format!("{RETURN}u"));
    } else if how != Move::WholePassword {
        fixture.focus("plain");
    }
    if !state_reads(false, Duration::from_secs(2)) {
        round.why_not = "secure input did not read off before the markers";
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
    let (fixture, front) = Fixture::start();
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

fn phase_with(
    name: &str,
    how: Move,
    waits: &[(u64, Duration)],
    source: fn(Duration) -> (InputSource, DropCounts),
    count: usize,
) -> Vec<Round> {
    let mut fixture = started_fixture(name);
    println!(
        "{name} gap_ms={} step_ms={}",
        test_gap().as_millis(),
        test_step().as_millis()
    );
    let mut rounds = Vec::new();
    let mut running: Option<(Duration, Recorder)> = None;
    for &(wait_ms, bound) in waits {
        if running.as_ref().map(|(at, _)| *at) != Some(bound) {
            drop(running.take());
            running = Some((bound, Recorder::start(source(bound))));
        }
        let recorder = &running.as_ref().expect("a source for the bound").1;
        for number in 0..count {
            let round = round(&mut fixture, recorder, how, wait_ms, bound);
            println!(
                "{name} round={number} wait_ms={wait_ms} bound_ms={} counted={} ahead={} \
                 markers={} plain={} secure={}",
                bound.as_millis(),
                u8::from(round.counted),
                round.ahead,
                round.markers,
                round.plain,
                round.secure
            );
            println!(
                "{name} drops round={number} {}",
                drops_line(&recorder.drops)
            );
            rounds.push(round);
        }
    }
    rounds
}

fn phase(name: &str, how: Move, waits: &[(u64, Duration)]) -> Vec<Round> {
    phase_with(name, how, waits, source_for, ROUNDS)
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
    judge(name, &phase(name, Move::Tab, &stall_waits()));
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
            FAULT_ROUNDS,
        ),
    );
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn known_fault_a_password_typed_whole_ahead_of_a_stall_is_not_recorded() {
    let name = "known_fault_a_password_typed_whole_ahead_of_a_stall_is_not_recorded";
    judge(
        name,
        &phase_with(
            name,
            Move::WholePassword,
            &[(300, SHORT_BOUND)],
            source_for,
            FAULT_ROUNDS,
        ),
    );
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_ahead_of_a_click_into_a_secure_field_is_not_recorded() {
    let name = "a_key_typed_ahead_of_a_click_into_a_secure_field_is_not_recorded";
    judge(name, &phase(name, Move::Click, &stall_waits()));
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded() {
    let name = "a_key_typed_ahead_of_a_focus_move_by_the_program_is_not_recorded";
    judge(name, &phase(name, Move::Itself, &stall_waits()));
}

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn a_key_typed_on_a_return_to_a_secure_field_is_not_recorded() {
    let name = "a_key_typed_on_a_return_to_a_secure_field_is_not_recorded";
    judge_three_ways(
        name,
        &phase_with(name, Move::Return, &stall_waits(), source_for, 1),
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
    judge(name, &phase(name, Move::LeavingKeyUp, &stall_waits()));
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

fn control_rounds(
    name: &str,
    bound: Duration,
    source: (InputSource, DropCounts),
    count: usize,
) -> (Vec<Control>, DropCounts) {
    let mut fixture = started_fixture(name);
    let recorder = Recorder::start(source);
    let mut rounds = Vec::new();
    for number in 0..count {
        let control = control_round(&mut fixture, &recorder, bound);
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
    let (rounds, _) = control_rounds(name, bound, source, count);
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

#[test]
#[ignore = "posts keys and shows a window: the hosted step alone runs it"]
fn at_the_bound_of_the_product_a_stall_records_nothing_and_the_control_records_its_keys() {
    let name =
        "at_the_bound_of_the_product_a_stall_records_nothing_and_the_control_records_its_keys";
    let rounds = phase_with(name, Move::Tab, &[(300, HOLD_BOUND)], product_source, 1);
    let (controls, drops) = control_rounds(name, HOLD_BOUND, product_source(HOLD_BOUND), 1);
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
    let verdict = if failed {
        "failed"
    } else if held {
        "held"
    } else if drops.of("gap") > 0 {
        "notkept"
    } else {
        "failed"
    };
    println!(
        "{name} verdict={verdict} widest_gap_us={} gap_drops={}",
        drops.widest_gap().as_micros(),
        drops.of("gap")
    );
    assert!(
        verdict != "failed",
        "{name}: at the product's constants a key was recorded that the rule forbids, or a round \
         did not count for another cause than a gap"
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
    let (mut fixture, front) = Fixture::start();
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
