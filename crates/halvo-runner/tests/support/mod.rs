#![allow(dead_code)]

pub mod browser;
pub mod connect;
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use halvo_policy::preset;
use halvo_schema::executable::body::{
    LoadReply, LoadedBindings, RunnerCall, RunnerReply, StepsReply, Stored,
};
use halvo_schema::executable::frame::{HandbackReply, SecretReply, StoreReply, StoreReplyFrame};
use halvo_schema::executable::names::{HandbackOutcome, HandbackReason};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::{Name, OwnValue, V1};
use halvo_schema::executable::{
    AgentCheckpoint, AgentConfig, AgentEffect, AgentLog, AgentRun, CheckpointState, EffectState,
    ExecutableStep, FromRunner, HandbackResult, Outcome, RunId, ToRunner, WebOrigin,
};
use halvo_schema::{AgentId, Category, WorkflowId};

pub const RUNNER: &str = env!("CARGO_BIN_EXE_halvo-runner");

/// Fails a test before its runner starts when the binary at `RUNNER` lacks the marker of the loopback route. Cargo
/// builds that binary for these tests with the feature `loopback` (the package's dev-dependency on itself), so a
/// binary without the marker is one that another target built again at this path, and its runner would refuse the
/// `http` origin of a fixture server before any request. The file is read once a test binary.
fn assert_loopback_runner() {
    static HAS_MARKER: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let has_marker = *HAS_MARKER.get_or_init(|| {
        let binary = std::fs::read(RUNNER).expect("the runner's binary reads");
        let marker = halvo_connect::LOOPBACK_MARKER.as_bytes();
        binary.windows(marker.len()).any(|bytes| bytes == marker)
    });
    assert!(
        has_marker,
        "the runner at {RUNNER} was built without the feature loopback: another target replaced it \
         (a nested cargo build of halvo-runner into this target directory without --features loopback)"
    );
}

/// The instant now on the system's clock, in nanoseconds since 1970-01-01T00:00:00Z.
pub fn now_nanos() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970");
    i64::try_from(since.as_nanos()).expect("an instant of i64")
}

/// The offset that a binary test of the day gives a run that it starts now: [`offset_of_another_day_at`] of the
/// system's clock.
pub fn offset_of_another_day() -> i32 {
    offset_of_another_day_at(now_nanos())
}

/// The offset for a run started at `utc_ns` nanoseconds since 1970-01-01T00:00:00Z: +50,400 s when the UTC hour is
/// 12 or later, else -50,400 s (the coordinator's ruling CP-204 D7). From that instant the local day at the offset
/// differs from the UTC day until 14:00 UTC for -50,400 s, at least 2 hours, and until the next 00:00 UTC for
/// +50,400 s, which is 1 second for a call at 23:59:59 UTC. No offset of the range differs from the UTC day on both
/// sides of 00:00 UTC, so a test asserts the difference from the UTC day only for a row at whose instant the two
/// days differ (CP-212 V2).
pub fn offset_of_another_day_at(utc_ns: i64) -> i32 {
    let max = halvo_schema::executable::day::UTC_OFFSET_S_MAX;
    if utc_ns.div_euclid(1_000_000_000).rem_euclid(86_400) >= 12 * 3_600 {
        max
    } else {
        -max
    }
}

/// A directory of its own under the target's temporary directory, made anew for each call by
/// `halvo_test_dir::make` under the name `runner-<tag>`: a name that stands, left by an earlier process with the
/// same process id, is passed over for the next count.
pub fn scratch(tag: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"));
    halvo_test_dir::make(root, &format!("runner-{tag}")).expect("make the scratch directory")
}

pub fn ids(n: u8) -> (RunId, AgentId, WorkflowId) {
    (
        RunId::from_random([n; 16]),
        AgentId::from_random([n.wrapping_add(1); 16]),
        WorkflowId::from_random([n.wrapping_add(2); 16]),
    )
}

/// The corpus workbook of the fill task.
pub fn corpus_workbook() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/r3-trials/sheet/corpus/formulas.xlsx")
}

/// The five rows of a seed's `sheet`, each as its three texts.
pub fn seed_rows(seed: u8) -> Vec<Vec<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../tools/fixture-server/variants/data/seed-{seed}.json"
    ));
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("read the seed")).expect("json");
    value["sheet"]["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| {
            row.as_array()
                .expect("a row")
                .iter()
                .map(|cell| cell.as_str().expect("a text").to_string())
                .collect()
        })
        .collect()
}

/// The fill task's files in a scratch directory: the workbook and `source.csv` with the seed's rows.
pub struct FillFiles {
    pub dir: PathBuf,
    pub workbook: PathBuf,
    pub source: PathBuf,
    pub rows: Vec<Vec<String>>,
}

impl FillFiles {
    pub fn new(tag: &str, seed: u8) -> FillFiles {
        let dir = scratch(tag);
        let workbook = dir.join("formulas.xlsx");
        std::fs::copy(corpus_workbook(), &workbook).expect("copy the workbook");
        let rows = seed_rows(seed);
        let mut text = String::new();
        for row in &rows {
            text.push_str(&row.join(","));
            text.push('\n');
        }
        let source = dir.join("source.csv");
        std::fs::write(&source, text).expect("write the source");
        FillFiles {
            dir,
            workbook,
            source,
            rows,
        }
    }

    pub fn slots(&self) -> Vec<Slot> {
        vec![
            slot("workbook", &self.workbook.to_string_lossy()),
            slot("source", &self.source.to_string_lossy()),
            slot("sheet", "Ledger"),
            slot("range", "B2:D6"),
        ]
    }

    pub fn workbook_bytes(&self) -> Vec<u8> {
        std::fs::read(&self.workbook).expect("read the workbook")
    }

    /// The names of the files beside the workbook, sorted.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.dir)
            .expect("list")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

pub fn slot(name: &str, value: &str) -> Slot {
    Slot {
        name: Name::parse(name).expect("a name"),
        value: OwnValue::new(value),
    }
}

pub fn config(category: Category, agent: &AgentId, workflow: &WorkflowId) -> AgentConfig {
    preset(category, workflow.clone(), agent.clone()).expect("a preset")
}

/// How the test supervisor answers one frame of the runner.
pub enum Act {
    /// The answer of the in-memory store, or no answer for a frame that takes none.
    Answer,
    /// This reply.
    With(ToRunner),
    /// The answer after a pause.
    After(Duration, Box<Act>),
    /// No reply: the supervisor closes both ends of the link at once.
    Close,
    /// The supervisor closes the end on which it reads the runner's frames, then writes the answer and closes the
    /// end on which it writes.
    CloseReadThenAnswer,
    /// No reply, and the link stays open until the runner ends.
    Hold,
    /// This frame, then the act.
    Then(ToRunner, Box<Act>),
}

/// What the supervisor saw.
pub struct Seen {
    /// The data directory that the harness gave the runner, empty for a start with no `--data-dir`, and the runner's
    /// argument line.
    pub data_dir: PathBuf,
    pub line: Vec<String>,
    pub frames: Vec<FromRunner>,
    pub status: Option<ExitStatus>,
    pub stderr: String,
    /// The time from the close of the link to the runner's end, where the supervisor closed it.
    pub close_to_end: Option<Duration>,
    pub group_gone: Option<bool>,
}

impl Seen {
    pub fn calls(&self) -> Vec<RunnerCall> {
        self.frames
            .iter()
            .filter_map(|f| match f {
                FromRunner::Store(s) => Some(s.request.body().clone()),
                _ => None,
            })
            .collect()
    }

    pub fn ops(&self) -> Vec<&'static str> {
        self.calls().iter().map(RunnerCall::op).collect()
    }

    pub fn tags(&self) -> Vec<&'static str> {
        self.frames.iter().map(tag).collect()
    }

    pub fn effects(&self) -> Vec<AgentEffect> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                RunnerCall::Effect(b) => Some(b.record),
                _ => None,
            })
            .collect()
    }

    pub fn states(&self) -> Vec<EffectState> {
        self.effects().iter().map(|e| e.state).collect()
    }

    /// The records of the `checkpoint` bodies, in the order that the runner sent them.
    pub fn checkpoints(&self) -> Vec<AgentCheckpoint> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                RunnerCall::Checkpoint(b) => Some(b.record),
                _ => None,
            })
            .collect()
    }

    pub fn logs(&self) -> Vec<AgentLog> {
        self.calls()
            .into_iter()
            .flat_map(|c| match c {
                RunnerCall::Log(b) => b.records,
                _ => Vec::new(),
            })
            .collect()
    }

    pub fn finish(&self) -> Option<AgentRun> {
        self.calls().into_iter().find_map(|c| match c {
            RunnerCall::Finish(b) => Some(b.record),
            _ => None,
        })
    }

    pub fn done(&self) -> Option<Outcome> {
        self.frames.iter().find_map(|f| match f {
            FromRunner::Done(d) => Some(d.outcome),
            _ => None,
        })
    }

    pub fn handbacks(&self) -> Vec<HandbackReason> {
        self.frames
            .iter()
            .filter_map(|f| match f {
                FromRunner::Handback(h) => Some(h.request.reason),
                _ => None,
            })
            .collect()
    }

    pub fn code(&self) -> Option<i32> {
        self.status.and_then(|s| s.code())
    }
}

pub fn tag(frame: &FromRunner) -> &'static str {
    match frame {
        FromRunner::Hello(_) => "hello",
        FromRunner::Store(_) => "store",
        FromRunner::Confirm(_) => "confirm",
        FromRunner::Handback(_) => "handback",
        FromRunner::Secret(_) => "secret",
        FromRunner::Progress(_) => "progress",
        FromRunner::Done(_) => "done",
        FromRunner::Bind(_) => "bind",
        FromRunner::Browser(_) => "browser",
    }
}

/// The runner's process with its two pipes.
pub struct Runner {
    child: Child,
    input: Option<ChildStdin>,
    output: Option<ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
    reaped: Arc<AtomicBool>,
    data_dir: PathBuf,
    line: Vec<String>,
}

/// The longest a runner of a test may live: one still running then is ended, so that its test fails and does not
/// hang.
pub const DEADLINE: Duration = Duration::from_secs(90);

/// The made-up program of `--browser` that the start of a runner gives where its test gave none: a relative path,
/// which the executor's start refuses, so that no runner of a test finds the browser of the machine that runs it.
pub const NO_BROWSER: &str = "no-browser-in-this-build";

/// The one test whose runner is given no `--browser` and finds the browser of a hosted macOS runner by itself
/// (`Runner::start_finding_its_browser`).
pub const FINDS_ITS_BROWSER: &str = "a_run_given_no_browser_starts_the_one_it_finds";

/// Whether a runner's line holds the test's own `--browser` or none, for the one test that lets it find one.
enum Browser {
    /// The line's own `--browser`, or `NO_BROWSER` where it has none.
    Given,
    /// No `--browser`: the runner finds the browser of the machine.
    Found,
}

/// Ends the process `pid` unless `reaped` is set by then; the process is the test's own child and is not reaped
/// before `reaped` is set, so its number names no other process.
fn end_at_deadline(pid: u32, reaped: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < DEADLINE {
            if reaped.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !reaped.load(Ordering::SeqCst) {
            #[cfg(unix)]
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            #[cfg(not(unix))]
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .status();
        }
    });
}

impl Runner {
    /// Starts the runner with `--data-dir` and a data directory of the test's own in front of
    /// `args`, so that no runner of a test locks under this account's real data directory
    /// (CP-134); it refuses before the child starts a line without the argument.
    pub fn start(args: &[String]) -> Runner {
        Runner::start_with_data_dir(args, Browser::Given)
    }

    /// Starts the runner as `start` does, with no `--browser`, so that it finds the browser of the machine by
    /// itself: for the test `FINDS_ITS_BROWSER` alone, on a hosted macOS runner alone.
    pub fn start_finding_its_browser(test: &str, args: &[String]) -> Runner {
        assert_eq!(
            test, FINDS_ITS_BROWSER,
            "a runner of another test is given --browser"
        );
        assert!(
            cfg!(target_os = "macos") && std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
            "{test}: a runner that finds its browser starts on a hosted macOS runner alone"
        );
        assert!(
            !args.iter().any(|a| a == "--browser"),
            "{test}: a start with no --browser"
        );
        Runner::start_with_data_dir(args, Browser::Found)
    }

    fn start_with_data_dir(args: &[String], browser: Browser) -> Runner {
        let data_dir = scratch("data");
        let mut line = vec![
            "--data-dir".to_string(),
            data_dir.to_string_lossy().into_owned(),
        ];
        line.extend(args.iter().cloned());
        assert!(
            line.windows(2)
                .any(|pair| pair[0] == "--data-dir" && !pair[1].is_empty()),
            "a runner of a test starts with --data-dir and a directory of its own"
        );
        Runner::spawn(line, data_dir, browser)
    }

    /// Starts the runner with `args` alone and no `--data-dir` (U4.7), its data directory in `Seen` empty. Its file
    /// step fails before it locks: run it only at a head whose runner takes no default data directory, since an
    /// older runner locks under this account's real data directory there.
    pub fn start_without_data_dir(args: &[String]) -> Runner {
        assert!(
            !args.iter().any(|a| a == "--data-dir"),
            "a start with no --data-dir"
        );
        Runner::spawn(args.to_vec(), PathBuf::new(), Browser::Given)
    }

    /// The one start of every runner of a test. A line with no `--browser` gets `--browser NO_BROWSER` in front,
    /// unless the start is `start_finding_its_browser`'s.
    fn spawn(mut line: Vec<String>, data_dir: PathBuf, browser: Browser) -> Runner {
        assert_loopback_runner();
        if matches!(browser, Browser::Given) && !line.iter().any(|a| a == "--browser") {
            line.splice(0..0, ["--browser".to_string(), NO_BROWSER.to_string()]);
        }
        let mut command = Command::new(RUNNER);
        command
            .args(&line)
            .env("TMPDIR", env!("CARGO_TARGET_TMPDIR"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().expect("start the runner");
        let reaped = Arc::new(AtomicBool::new(false));
        end_at_deadline(child.id(), Arc::clone(&reaped));
        Runner {
            input: child.stdin.take(),
            output: child.stdout.take(),
            stderr: child.stderr.take(),
            child,
            reaped,
            data_dir,
            line,
        }
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn write_raw(&mut self, bytes: &[u8]) {
        if let Some(input) = self.input.as_mut() {
            let _ = input.write_all(bytes);
            let _ = input.flush();
        }
    }

    pub fn send(&mut self, frame: &ToRunner) {
        let json = frame.to_json().expect("a frame");
        let mut bytes = u32::try_from(json.len())
            .expect("a length")
            .to_be_bytes()
            .to_vec();
        bytes.extend_from_slice(json.as_bytes());
        self.write_raw(&bytes);
    }

    /// The next frame, or none at the end of the runner's output.
    pub fn recv(&mut self) -> Option<FromRunner> {
        let output = self.output.as_mut()?;
        let mut head = [0u8; 4];
        output.read_exact(&mut head).ok()?;
        let mut body = vec![0u8; u32::from_be_bytes(head) as usize];
        output.read_exact(&mut body).ok()?;
        Some(FromRunner::parse(&body).expect("the runner writes frames that parse"))
    }

    pub fn close_input(&mut self) {
        self.input = None;
    }

    pub fn close_output(&mut self) {
        self.output = None;
    }

    /// Waits for the runner's end, at most `limit`.
    pub fn wait_within(&mut self, limit: Duration) -> Option<ExitStatus> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                self.reaped.store(true, Ordering::SeqCst);
                return Some(status);
            }
            if start.elapsed() > limit {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.reaped.store(true, Ordering::SeqCst);
    }

    pub fn close_stderr(&mut self) {
        self.stderr = None;
    }

    pub fn stderr(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut err) = self.stderr.take() {
            let _ = err.read_to_string(&mut text);
        }
        text
    }

    /// Whether no process of the runner's process group is left (macOS and Linux); on Windows, where the runner
    /// starts no process of its own in this build, whether the runner itself is gone.
    pub fn group_gone(&mut self) -> bool {
        if self.child.try_wait().expect("try_wait").is_none() {
            return false;
        }
        #[cfg(unix)]
        {
            let group = self.child.id().to_string();
            let out = Command::new("ps")
                .args(["-A", "-o", "pgid="])
                .output()
                .expect("ps");
            !String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|line| line.trim() == group)
        }
        #[cfg(not(unix))]
        {
            true
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            self.kill();
        }
        self.reaped.store(true, Ordering::SeqCst);
    }
}

/// The supervisor of a test: an in-memory store with one agent's configuration, bindings and stored playbook, and
/// a rule per frame.
pub struct Supervisor {
    pub run: RunId,
    pub agent: AgentId,
    pub workflow: WorkflowId,
    pub config: AgentConfig,
    pub origins: Vec<WebOrigin>,
    pub slots: Vec<Slot>,
    pub steps: Vec<ExecutableStep>,
    pub mode: &'static str,
    /// The run's `--utc-offset`, 0 unless a test sets it.
    pub utc_offset_s: i32,
    /// Whether `serve` starts the runner with `--data-dir` and a directory of the test's own; true unless a test
    /// clears it for `Runner::start_without_data_dir`.
    pub data_dir: bool,
}

impl Supervisor {
    pub fn new(n: u8, category: Category, slots: Vec<Slot>) -> Supervisor {
        let (run, agent, workflow) = ids(n);
        let config = config(category, &agent, &workflow);
        Supervisor {
            run,
            agent,
            workflow,
            config,
            origins: Vec::new(),
            slots,
            steps: Vec::new(),
            mode: "replay",
            utc_offset_s: 0,
            data_dir: true,
        }
    }

    pub fn args(&self) -> Vec<String> {
        vec![
            "--run".into(),
            self.run.as_str().into(),
            "--agent".into(),
            self.agent.as_str().into(),
            "--workflow".into(),
            self.workflow.as_str().into(),
            "--mode".into(),
            self.mode.into(),
            "--browser".into(),
            "no-browser-in-this-build".into(),
            "--utc-offset".into(),
            self.utc_offset_s.to_string(),
        ]
    }

    /// The store's answer to a body.
    pub fn store_answer(&self, call: &RunnerCall) -> RunnerReply {
        match call {
            RunnerCall::Load(_) => RunnerReply::Load(LoadReply {
                config: self.config.clone(),
                playbook: NonZeroU32::new(u32::try_from(self.steps.len()).expect("count")).map(
                    |steps| halvo_schema::executable::body::LoadedPlaybook {
                        number: NonZeroU32::MIN,
                        steps,
                    },
                ),
                next_playbook: NonZeroU32::MIN.saturating_add(1),
                bindings: Some(LoadedBindings {
                    rev: NonZeroU32::MIN,
                    origins: self.origins.clone(),
                    slots: self.slots.clone(),
                }),
                sessions: Vec::new(),
                start: None,
            }),
            RunnerCall::LoadSteps(b) => {
                let from = b.from.get() as usize;
                RunnerReply::Steps(StepsReply {
                    agent: self.config.agent.clone(),
                    steps: self.steps.iter().skip(from - 1).take(16).cloned().collect(),
                })
            }
            RunnerCall::Log(b) => RunnerReply::Stored(Stored {
                stored: u32::try_from(b.records.len()).expect("count"),
            }),
            _ => RunnerReply::Stored(Stored { stored: 1 }),
        }
    }

    /// The default answer to a frame: the store's for `store`, and none for a frame that takes no reply.
    pub fn answer(&self, frame: &FromRunner) -> Option<ToRunner> {
        match frame {
            FromRunner::Store(s) => Some(ToRunner::StoreReply(StoreReplyFrame {
                reply: StoreReply {
                    id: Some(s.request.id().clone()),
                    outcome: Ok(self.store_answer(s.request.body())),
                },
            })),
            FromRunner::Secret(_) => Some(ToRunner::SecretReply(SecretReply::None)),
            FromRunner::Handback(_) => Some(handback_reply(HandbackOutcome::Declined, None)),
            _ => None,
        }
    }

    /// Starts the runner and serves it until its end, with `rule` choosing the act for each frame; the default act
    /// is `Act::Answer`. At most `limit` for the whole run.
    pub fn serve(
        &self,
        mut rule: impl FnMut(&FromRunner, &[FromRunner]) -> Act,
        limit: Duration,
    ) -> Seen {
        let mut runner = if self.data_dir {
            Runner::start(&self.args())
        } else {
            Runner::start_without_data_dir(&self.args())
        };
        let mut frames: Vec<FromRunner> = Vec::new();
        let mut closed_at: Option<Instant> = None;
        let start = Instant::now();
        while let Some(frame) = runner.recv() {
            let act = rule(&frame, &frames);
            frames.push(frame.clone());
            if !self.act(&mut runner, &frame, act, &mut closed_at) {
                break;
            }
            if start.elapsed() > limit {
                break;
            }
        }
        let left = limit.saturating_sub(start.elapsed());
        let status = runner.wait_within(left);
        let close_to_end = match (closed_at, status) {
            (Some(at), Some(_)) => Some(at.elapsed()),
            _ => None,
        };
        let group_gone = closed_at.map(|_| runner.group_gone());
        if status.is_none() {
            runner.kill();
        }
        let stderr = runner.stderr();
        Seen {
            data_dir: runner.data_dir.clone(),
            line: runner.line.clone(),
            frames,
            status,
            stderr,
            close_to_end,
            group_gone,
        }
    }

    fn act(
        &self,
        runner: &mut Runner,
        frame: &FromRunner,
        act: Act,
        closed_at: &mut Option<Instant>,
    ) -> bool {
        match act {
            Act::Answer => {
                if let Some(reply) = self.answer(frame) {
                    runner.send(&reply);
                }
                true
            }
            Act::With(reply) => {
                runner.send(&reply);
                true
            }
            Act::After(pause, then) => {
                std::thread::sleep(pause);
                self.act(runner, frame, *then, closed_at)
            }
            Act::Close => {
                runner.close_input();
                runner.close_output();
                *closed_at = Some(Instant::now());
                false
            }
            Act::CloseReadThenAnswer => {
                runner.close_output();
                if let Some(reply) = self.answer(frame) {
                    runner.send(&reply);
                }
                runner.close_input();
                *closed_at = Some(Instant::now());
                false
            }
            Act::Hold => true,
            Act::Then(first, then) => {
                runner.send(&first);
                self.act(runner, frame, *then, closed_at)
            }
        }
    }
}

pub fn handback_reply(outcome: HandbackOutcome, secret: Option<&str>) -> ToRunner {
    ToRunner::HandbackReply(HandbackReply {
        result: HandbackResult {
            v: V1,
            outcome,
            secret: secret.map(|s| Name::parse(s).expect("a name")),
        },
    })
}

pub fn store_error(frame: &FromRunner, name: &str) -> ToRunner {
    let FromRunner::Store(s) = frame else {
        panic!("not a store frame")
    };
    store_error_for(s.request.id().as_str(), name)
}

/// A store refusal `name` that names the request id `id`, which the test chooses.
pub fn store_error_for(id: &str, name: &str) -> ToRunner {
    let text = format!(r#"{{"t":"store_reply","reply":{{"v":1,"id":"{id}","error":"{name}"}}}}"#);
    ToRunner::parse(text.as_bytes()).expect("a store reply")
}

/// The reply `{"stored":<count>}` to the store frame `frame`.
pub fn stored(frame: &FromRunner, count: u32) -> ToRunner {
    let FromRunner::Store(s) = frame else {
        panic!("not a store frame")
    };
    let text = format!(
        r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","ok":{{"stored":{count}}}}}}}"#,
        s.request.id().as_str()
    );
    ToRunner::parse(text.as_bytes()).expect("a store reply")
}

pub fn is_op(frame: &FromRunner, op: &str) -> bool {
    matches!(frame, FromRunner::Store(s) if s.request.body().op() == op)
}

pub fn is_effect(frame: &FromRunner, state: EffectState) -> bool {
    matches!(frame, FromRunner::Store(s) if matches!(s.request.body(), RunnerCall::Effect(b) if b.record.state == state))
}

pub fn is_checkpoint(frame: &FromRunner, state: CheckpointState) -> bool {
    matches!(frame, FromRunner::Store(s) if matches!(s.request.body(), RunnerCall::Checkpoint(b) if b.record.state == state))
}

/// A frame by its tag, a `store` frame also by its call and, for an effect or a checkpoint row, by its state.
pub fn named(frame: &FromRunner) -> String {
    match frame {
        FromRunner::Store(s) => match s.request.body() {
            RunnerCall::Effect(b) => format!("store effect {:?}", b.record.state),
            RunnerCall::Checkpoint(b) => format!("store checkpoint {:?}", b.record.state),
            body => format!("store {}", body.op()),
        },
        other => tag(other).to_string(),
    }
}

/// The cells of the fill range as the sheet executor reads them, each written as text.
pub fn range_texts(dir: &Path, workbook: &Path) -> Vec<Vec<String>> {
    let data = scratch("read-data");
    let locks =
        halvo_sheet::Locks::open(&data, LONG).expect("the locks of a data directory of its own");
    let folder = halvo_sheet::Folder::new(dir, locks).expect("a folder");
    let range = halvo_sheet::Range::parse("B2:D6").expect("a range");
    folder
        .read(workbook, "Ledger", range)
        .expect("read the range")
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| match cell {
                    halvo_sheet::Cell::Text(t) => t,
                    halvo_sheet::Cell::Number(n) => format!("{n:.2}"),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

pub const LONG: Duration = Duration::from_secs(60);

/// The load reply that the supervisor gives with `change` applied to its configuration and slots.
pub fn load_with(sup: &Supervisor, frame: &FromRunner, change: impl FnOnce(&mut LoadReply)) -> Act {
    let FromRunner::Store(s) = frame else {
        unreachable!("a load is a store frame")
    };
    let mut reply = sup.store_answer(s.request.body());
    if let RunnerReply::Load(l) = &mut reply {
        change(l);
    }
    Act::With(ToRunner::StoreReply(StoreReplyFrame {
        reply: StoreReply {
            id: Some(s.request.id().clone()),
            outcome: Ok(reply),
        },
    }))
}

/// Serves a fill run in which `change` may alter the reply to each load, counted from 1 (the run's first load), and
/// `at_load` runs before that load is answered.
pub fn serve_loads(
    sup: &Supervisor,
    mut change: impl FnMut(u32, &mut LoadReply),
    mut at_load: impl FnMut(u32),
) -> Seen {
    let mut loads = 0;
    sup.serve(
        |frame, _| {
            if !is_op(frame, "load") {
                return Act::Answer;
            }
            loads += 1;
            at_load(loads);
            load_with(sup, frame, |l| change(loads, l))
        },
        LONG,
    )
}

pub fn set_slot(l: &mut LoadReply, name: &str, value: &str) {
    let bindings = l.bindings.as_mut().expect("bindings");
    let at = bindings
        .slots
        .iter()
        .position(|s| s.name.as_str() == name)
        .expect("the slot");
    bindings.slots[at] = slot(name, value);
}

/// The person's own change of the workbook: one cell outside the fill range, written through the sheet executor
/// under a data directory and a run of the test's own; the workbook's bytes after it.
pub fn person_changes(files: &FillFiles) -> Vec<u8> {
    let data = scratch("person-data");
    let locks = halvo_sheet::Locks::open(&data, LONG).expect("locks");
    let folder = halvo_sheet::Folder::new(&files.dir, locks).expect("a folder");
    let run = halvo_sheet::RunId::parse(&"5e".repeat(16)).expect("a run id");
    let range = halvo_sheet::Range::parse("F9:F9").expect("a range");
    let values = vec![vec![halvo_sheet::Value::Text(
        "the person's note".to_string(),
    )]];
    let prepared = folder
        .prepare(&files.workbook, &run, "Ledger", range, &values)
        .expect("prepare");
    folder.replace(&prepared).expect("replace");
    files.workbook_bytes()
}
