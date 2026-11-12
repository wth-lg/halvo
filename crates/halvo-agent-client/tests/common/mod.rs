#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use halvo_agent_client::marks::{self, Mark, ProcLine};
use halvo_agent_client::process;
use halvo_agent_client::{
    Config, Confirmer, Human, IssueRefused, LinkError, MarkSink, OriginAnswer, OwnerLink, Parts,
    RunEnd, RunSpec, Secrets, Supervisor,
};
use halvo_schema::executable::body::{CoordinatorCall, RunnerCall};
use halvo_schema::executable::frame::{
    ConfirmDecision, ConfirmRequest, HandbackRequest, SecretReply,
};
use halvo_schema::executable::names::{Answer, EffectState, Mode};
use halvo_schema::executable::text::{CreationTime, EffectId, Name, RunId, WebOrigin};
use halvo_schema::request::{decode, CoordinatorRole, Decoded};
use halvo_schema::{AgentId, WorkflowId};

static COUNT: AtomicU32 = AtomicU32::new(0);

pub type Check = Box<dyn Fn(&RunId) + Send>;

pub fn peer() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agent-client-peer"))
}

pub fn scratch(name: &str) -> PathBuf {
    let n = COUNT.fetch_add(1, Ordering::SeqCst);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("agent-client")
        .join(format!("{name}-{}-{n}-{}", std::process::id(), nanos()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

pub fn id16() -> [u8; 16] {
    let n = COUNT.fetch_add(1, Ordering::SeqCst);
    let mut b = [0u8; 16];
    b[..4].copy_from_slice(&std::process::id().to_be_bytes());
    b[4..8].copy_from_slice(&n.to_be_bytes());
    b[8..].copy_from_slice(&(nanos() as u64).to_be_bytes());
    b
}

pub fn run_id() -> RunId {
    RunId::from_random(id16())
}

pub fn origin(s: &str) -> WebOrigin {
    WebOrigin::parse(s).unwrap()
}

/// A script of the peer runner, with the log that it writes beside it.
pub struct Script {
    pub path: PathBuf,
}

impl Script {
    pub fn new(dir: &Path, lines: &[&str]) -> Script {
        let path = dir.join(format!("script-{}", COUNT.fetch_add(1, Ordering::SeqCst)));
        fs::write(&path, lines.join("\n")).unwrap();
        Script { path }
    }

    pub fn log(&self) -> String {
        fs::read_to_string(format!("{}.log", self.path.display())).unwrap_or_default()
    }

    pub fn wait_log(&self, needle: &str, within: Duration) -> bool {
        wait_until(within, || self.log().contains(needle))
    }

    pub fn replies(&self) -> Vec<String> {
        self.log()
            .lines()
            .filter_map(|l| l.strip_prefix("reply ").map(str::to_owned))
            .collect()
    }

    /// The processes that the peer logged as `<word> pid=<n> created=<c>`.
    pub fn logged(&self, word: &str) -> Vec<(u32, CreationTime)> {
        self.log()
            .lines()
            .filter_map(|l| l.strip_prefix(&format!("{word} ")))
            .filter_map(|rest| {
                let mut pid = None;
                let mut created = None;
                for part in rest.split(' ') {
                    if let Some(p) = part.strip_prefix("pid=") {
                        pid = p.parse().ok();
                    }
                    if let Some(c) = part.strip_prefix("created=") {
                        created = CreationTime::parse(c).ok();
                    }
                }
                Some((pid?, created?))
            })
            .collect()
    }
}

pub fn wait_until(within: Duration, mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if f() {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub fn gone(pid: u32, created: CreationTime) -> bool {
    !process::is_running(pid, created)
}

pub fn line_gone(line: &ProcLine) -> bool {
    gone(line.pid.get(), line.created)
}

pub fn mark_of(dir: &Path, run: &RunId) -> Option<Mark> {
    marks::read_all(dir)
        .unwrap()
        .into_iter()
        .find(|m| &m.run == run)
}

/// What the memory owner keeps of one effect: its rows' states, in order.
#[derive(Default)]
pub struct Rows {
    pub effects: HashMap<EffectId, Vec<EffectState>>,
    pub finished: bool,
    pub ended: bool,
}

/// The owner behind the link, in memory: it records each request, answers each runner body as stored, `add_origin`
/// with the next revision and `end_run` with `{"ok":"yes"}`, and makes the moves of the run's end of A7.5 for the
/// effects of a run whose `end_run` it gets: `end` for `held`, `cleared` and `approved`, `lose` for `releasing`.
#[derive(Default)]
pub struct Store {
    pub requests: Mutex<Vec<String>>,
    pub rows: Mutex<HashMap<RunId, Rows>>,
    pub rev: AtomicU32,
    pub fail_add_origin: AtomicUsize,
    pub fail_all: AtomicUsize,
    pub load_origins: Mutex<Vec<String>>,
    pub check: Mutex<Option<Check>>,
    pub hold_end_run: Mutex<bool>,
}

impl Store {
    pub fn bodies(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    pub fn count(&self, needle: &str) -> usize {
        self.bodies().iter().filter(|b| b.contains(needle)).count()
    }

    pub fn states(&self, run: &RunId, effect: &EffectId) -> Vec<EffectState> {
        self.rows
            .lock()
            .unwrap()
            .get(run)
            .and_then(|r| r.effects.get(effect).cloned())
            .unwrap_or_default()
    }

    fn reply(id: &str, ok: &str) -> Vec<u8> {
        format!(r#"{{"v":1,"id":"{id}","ok":{ok}}}"#).into_bytes()
    }

    fn error(id: &str, name: &str) -> Vec<u8> {
        format!(r#"{{"v":1,"id":"{id}","error":"{name}"}}"#).into_bytes()
    }
}

impl OwnerLink for Store {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        let text = String::from_utf8_lossy(request).into_owned();
        self.requests.lock().unwrap().push(text);
        if self
            .fail_all
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(LinkError);
        }
        let Ok(Decoded::Raw(raw)) = decode(request) else {
            return Err(LinkError);
        };
        let id = raw.id().as_str().to_owned();
        if raw.name() == "runner" {
            let body = RunnerCall::parse(raw.body().get().as_bytes()).unwrap();
            if let Some(check) = self.check.lock().unwrap().as_ref() {
                check(body.run());
            }
            let mut rows = self.rows.lock().unwrap();
            let r = rows.entry(body.run().clone()).or_default();
            return Ok(match body {
                RunnerCall::Load(_) => {
                    let origins: Vec<String> = self
                        .load_origins
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|o| format!("\"{o}\""))
                        .collect();
                    let bindings = if origins.is_empty() {
                        "null".to_owned()
                    } else {
                        format!(
                            r#"{{"rev":1,"origins":[{}],"slots":[]}}"#,
                            origins.join(",")
                        )
                    };
                    Store::reply(&id, &load_reply(&bindings))
                }
                RunnerCall::Effect(e) if r.ended => {
                    let _ = e;
                    Store::error(&id, "effect")
                }
                RunnerCall::Effect(e) => {
                    r.effects
                        .entry(e.record.effect.clone())
                        .or_default()
                        .push(e.record.state);
                    Store::reply(&id, r#"{"stored":1}"#)
                }
                RunnerCall::Finish(_) => {
                    r.finished = true;
                    for states in r.effects.values_mut() {
                        end_move(states);
                    }
                    Store::reply(&id, r#"{"stored":1}"#)
                }
                _ => Store::reply(&id, r#"{"stored":1}"#),
            });
        }
        let body = CoordinatorCall::parse(raw.body().get().as_bytes()).unwrap();
        match body {
            CoordinatorCall::AddOrigin(_) => {
                if self
                    .fail_add_origin
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_ok()
                {
                    return Err(LinkError);
                }
                let rev = self.rev.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(Store::reply(&id, &format!(r#"{{"rev":{rev}}}"#)))
            }
            CoordinatorCall::EndRun(e) => {
                while *self.hold_end_run.lock().unwrap() {
                    thread::sleep(Duration::from_millis(20));
                }
                let mut rows = self.rows.lock().unwrap();
                let r = rows.entry(e.run.clone()).or_default();
                if !r.finished && !r.ended {
                    for states in r.effects.values_mut() {
                        end_move(states);
                    }
                }
                r.ended = true;
                Ok(Store::reply(&id, r#"{"ok":"yes"}"#))
            }
            _ => Ok(Store::error(&id, "unsupported")),
        }
    }
}

fn end_move(states: &mut Vec<EffectState>) {
    let next = match states.last() {
        Some(EffectState::Held | EffectState::Cleared | EffectState::Approved) => {
            EffectState::Rejected
        }
        Some(EffectState::Releasing) => EffectState::Unknown,
        _ => return,
    };
    states.push(next);
}

fn load_reply(bindings: &str) -> String {
    format!(
        concat!(
            r#"{{"config":{{"v":1,"workflow":"00000000000000000000000000000001","#,
            r#""agent":"00000000000000000000000000000002","rev":1,"category":"inbox_triage","#,
            r#""reasoning":"low","autonomy":"draft_and_confirm","allow":[],"#,
            r#""caps":{{"untrusted":"no","private":"no","external":"no"}},"#,
            r#""limits":{{"steps":10,"wall_s":60,"model_tokens":1000}},"#,
            r#""budget":{{"run":1,"day":1}},"permissions":[],"runtime":"web"}},"#,
            r#""playbook":null,"bindings":{},"sessions":[]}}"#
        ),
        bindings
    )
}

/// The marks of starts and ends, with a wait for the end of a run.
#[derive(Default)]
pub struct Recorder {
    pub starts: Mutex<Vec<RunId>>,
    pub ends: Mutex<Vec<(RunId, RunEnd)>>,
    signal: Condvar,
}

impl Recorder {
    pub fn wait_end(&self, run: &RunId, within: Duration) -> Option<RunEnd> {
        let deadline = Instant::now() + within;
        let mut ends = self.ends.lock().unwrap();
        loop {
            if let Some((_, e)) = ends.iter().find(|(r, _)| r == run) {
                return Some(e.clone());
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            ends = self.signal.wait_timeout(ends, left).unwrap().0;
        }
    }
}

impl MarkSink for Recorder {
    fn start(&self, run: &RunId) {
        self.starts.lock().unwrap().push(run.clone());
    }

    fn end(&self, run: &RunId, end: &RunEnd) {
        self.ends.lock().unwrap().push((run.clone(), end.clone()));
        self.signal.notify_all();
    }
}

/// The person: answers by its settings and records what it was asked.
pub struct People {
    pub answers: Mutex<VecDeque<Vec<Answer>>>,
    pub issued: Mutex<Vec<ConfirmDecision>>,
    pub refuse_issue: Mutex<bool>,
    pub allow: Mutex<OriginAnswer>,
    pub asked_origins: Mutex<Vec<WebOrigin>>,
    pub handbacks: Mutex<Vec<HandbackRequest>>,
    pub progress: Mutex<Vec<(u32, u32)>>,
}

impl Default for People {
    fn default() -> People {
        People {
            answers: Mutex::new(VecDeque::new()),
            issued: Mutex::new(Vec::new()),
            refuse_issue: Mutex::new(false),
            allow: Mutex::new(OriginAnswer::Allow),
            asked_origins: Mutex::new(Vec::new()),
            handbacks: Mutex::new(Vec::new()),
            progress: Mutex::new(Vec::new()),
        }
    }
}

impl Confirmer for People {
    fn answers(&self, _run: &RunId, request: &ConfirmRequest) -> Vec<Answer> {
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| vec![Answer::AllowOnce; request.effects.len()])
    }

    fn issued(&self, _run: &RunId, decision: &ConfirmDecision) -> Result<(), IssueRefused> {
        self.issued.lock().unwrap().push(decision.clone());
        if *self.refuse_issue.lock().unwrap() {
            return Err(IssueRefused);
        }
        Ok(())
    }
}

impl Human for People {
    fn handback(&self, _run: &RunId, request: &HandbackRequest) {
        self.handbacks.lock().unwrap().push(request.clone());
    }

    fn allow_origin(&self, _run: &RunId, origin: &WebOrigin) -> OriginAnswer {
        self.asked_origins.lock().unwrap().push(origin.clone());
        *self.allow.lock().unwrap()
    }

    fn progress(&self, _run: &RunId, step: NonZeroU32, of: NonZeroU32) {
        self.progress.lock().unwrap().push((step.get(), of.get()));
    }
}

/// The secret store: one answer for every name.
pub struct Vault(pub Mutex<SecretReply>);

impl Secrets for Vault {
    fn read(&self, _name: &Name) -> SecretReply {
        self.0.lock().unwrap().clone()
    }
}

/// A supervisor over the memory parts, its data directory, and the parts.
pub struct Rig {
    pub dir: PathBuf,
    pub sup: Supervisor,
    pub store: Arc<Store>,
    pub people: Arc<People>,
    pub vault: Arc<Vault>,
    pub rec: Arc<Recorder>,
}

impl Rig {
    pub fn new(name: &str) -> Rig {
        Rig::with(name, peer(), Duration::from_secs(10))
    }

    pub fn with(name: &str, runner: PathBuf, start_wait: Duration) -> Rig {
        let dir = scratch(name);
        let store = Arc::new(Store::default());
        let people = Arc::new(People::default());
        let vault = Arc::new(Vault(Mutex::new(SecretReply::None)));
        let rec = Arc::new(Recorder::default());
        let parts = Parts {
            link: store.clone(),
            confirmer: people.clone(),
            human: people.clone(),
            secrets: vault.clone(),
            marks: rec.clone(),
        };
        let mut config = Config::new(dir.join("data"), runner);
        config.start_wait = start_wait;
        let sup = Supervisor::new(CoordinatorRole::claim(), config, parts);
        Rig {
            dir,
            sup,
            store,
            people,
            vault,
            rec,
        }
    }

    pub fn data(&self) -> PathBuf {
        self.dir.join("data")
    }

    pub fn script(&self, lines: &[&str]) -> Script {
        Script::new(&self.dir, lines)
    }

    pub fn spec(&self, script: &Script) -> RunSpec {
        RunSpec {
            run: run_id(),
            agent: AgentId::from_random(id16()),
            workflow: WorkflowId::from_random(id16()),
            mode: Mode::Replay,
            browser: script.path.clone(),
            model: None,
            headed: false,
        }
    }

    pub fn mark(&self, run: &RunId) -> Option<Mark> {
        mark_of(&self.data(), run)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        for mark in marks::read_all(&self.data()).unwrap_or_default() {
            let _ = self.sup.kill(&mark.run);
        }
    }
}
