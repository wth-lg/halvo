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
    RunEnd, RunSpec, Secrets, Supervisor, SystemValue,
};
use halvo_schema::executable::body::{CoordinatorCall, RunnerCall};
use halvo_schema::executable::frame::{
    ConfirmDecision, ConfirmRequest, HandbackRequest, SecretReply,
};
use halvo_schema::executable::names::{Answer, EffectState, Mode, Outcome};
use halvo_schema::executable::record::AgentRun;
use halvo_schema::executable::text::{CreationTime, EffectId, Name, RunId, WebOrigin};
use halvo_schema::request::{decode, CoordinatorRole, Decoded};
use halvo_schema::{AgentId, WorkflowId};

static COUNT: AtomicU32 = AtomicU32::new(0);

pub type Check = Box<dyn Fn(&RunId) + Send>;

pub fn peer() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agent-client-peer"))
}

pub fn scratch(name: &str) -> PathBuf {
    halvo_test_dir::make(
        &Path::new(env!("CARGO_TARGET_TMPDIR")).join("agent-client"),
        name,
    )
    .unwrap()
}

#[test]
fn two_scratches_of_one_name_are_two_directories_and_the_second_is_empty() {
    let first = scratch("one-name");
    fs::write(first.join("mine"), b"first").unwrap();
    let second = scratch("one-name");
    assert_ne!(first, second);
    assert_eq!(fs::read_dir(&second).unwrap().count(), 0);
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
/// with the next revision, `end_run` with `{"ok":"yes"}`, `list_checkpoints` with a page of `pages` and
/// `mark_checkpoint` with `{"stored":1}`, and makes
/// the moves of the run's end of A7.5 for the effects of a run whose `end_run` it gets: `end` for `held`, `cleared`
/// and `approved`, `lose` for `releasing`.
#[derive(Default)]
pub struct Store {
    pub requests: Mutex<Vec<String>>,
    pub rows: Mutex<HashMap<RunId, Rows>>,
    pub rev: AtomicU32,
    pub fail_add_origin: AtomicUsize,
    pub garbage_add_origin: AtomicUsize,
    pub error_add_origin: AtomicUsize,
    pub hold_add_origin: Mutex<bool>,
    pub fail_all: AtomicUsize,
    pub load_origins: Mutex<Vec<String>>,
    pub check: Mutex<Option<Check>>,
    pub hold_end_run: Mutex<bool>,
    pub wrong_id_runner: AtomicUsize,
    pub wrong_form_runner: AtomicUsize,
    pub wrong_id_coord: AtomicUsize,
    pub wrong_form_coord: AtomicUsize,
    /// The pages that a `list_checkpoints` is answered with, by its `after`: each the text that follows `"ok":` as
    /// the test wrote it, white space included. A body whose `after` names no page is answered `unsupported`.
    pub pages: Mutex<Vec<(Option<RunId>, String)>>,
    pub other_agent_runner: AtomicUsize,
    /// The count of the next `mark_checkpoint` bodies that get a link error and no reply, as when the link is cut.
    pub fail_mark: AtomicUsize,
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

    /// The agent that a reply of `load_steps` or `load_session` names: another agent's id while
    /// `other_agent_runner` counts down, else the asking agent's.
    fn reply_agent(&self, asking: &str) -> String {
        if take_one(&self.other_agent_runner) {
            "e".repeat(32)
        } else {
            asking.to_owned()
        }
    }
}

fn take_one(n: &AtomicUsize) -> bool {
    n.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
        .is_ok()
}

impl OwnerLink for Store {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        let reply = self.answer(request)?;
        let Ok(Decoded::Raw(raw)) = decode(request) else {
            return Ok(reply);
        };
        let id = raw.id().as_str();
        let (wrong_id, wrong_form, form) = if raw.name() == "runner" {
            (
                &self.wrong_id_runner,
                &self.wrong_form_runner,
                r#"{"stored":1}"#,
            )
        } else {
            (
                &self.wrong_id_coord,
                &self.wrong_form_coord,
                r#"{"ok":"yes"}"#,
            )
        };
        if take_one(wrong_id) {
            let other = "ab".repeat(16);
            return Ok(String::from_utf8_lossy(&reply)
                .replace(id, &other)
                .into_bytes());
        }
        if take_one(wrong_form) {
            return Ok(Store::reply(id, form));
        }
        Ok(reply)
    }
}

impl Store {
    fn answer(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
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
                RunnerCall::Load(load) => {
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
                    Store::reply(&id, &load_reply(&bindings, load.agent.as_str()))
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
                RunnerCall::LoadSteps(b) => {
                    let agent = self.reply_agent(b.agent.as_str());
                    Store::reply(&id, &format!(r#"{{"agent":"{agent}","steps":[]}}"#))
                }
                RunnerCall::LoadSession(b) => {
                    let agent = self.reply_agent(b.agent.as_str());
                    let part = format!(
                        r#""origin":"{}","rev":{},"part":{},"of":{},"data":["QUJD"]"#,
                        b.origin.as_str(),
                        b.rev,
                        b.part,
                        b.part
                    );
                    Store::reply(&id, &format!(r#"{{"agent":"{agent}",{part}}}"#))
                }
                _ => Store::reply(&id, r#"{"stored":1}"#),
            });
        }
        let body = CoordinatorCall::parse(raw.body().get().as_bytes()).unwrap();
        match body {
            CoordinatorCall::AddOrigin(_) => {
                while *self.hold_add_origin.lock().unwrap() {
                    thread::sleep(Duration::from_millis(10));
                }
                if take_one(&self.garbage_add_origin) {
                    return Ok(b"not a reply".to_vec());
                }
                if take_one(&self.error_add_origin) {
                    return Ok(Store::error(&id, "internal"));
                }
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
            CoordinatorCall::ListCheckpoints(list) => {
                let pages = self.pages.lock().unwrap();
                Ok(match pages.iter().find(|(after, _)| *after == list.after) {
                    Some((_, page)) => Store::reply(&id, page),
                    None => Store::error(&id, "unsupported"),
                })
            }
            CoordinatorCall::MarkCheckpoint(_) if take_one(&self.fail_mark) => Err(LinkError),
            CoordinatorCall::MarkCheckpoint(_) => Ok(Store::reply(&id, r#"{"stored":1}"#)),
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

const CONFIG: &str = r#"{"v":1,"workflow":"30303030303030303030303030303030","agent":"40404040404040404040404040404040","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{"untrusted":"yes","private":"yes","external":"yes"},"limits":{"steps":20,"wall_s":120,"model_tokens":0},"budget":null,"permissions":[],"runtime":"web","playbook":null}"#;

fn load_reply(bindings: &str, agent: &str) -> String {
    let config = CONFIG.replacen("40404040404040404040404040404040", agent, 1);
    format!(
        r#"{{"config":{config},"playbook":null,"next_playbook":1,"bindings":{bindings},"sessions":[],"start":null}}"#
    )
}

pub fn kill_pid(pid: u32) {
    let status = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .status()
    } else {
        std::process::Command::new("/bin/kill")
            .args(["-KILL", &pid.to_string()])
            .status()
    };
    assert!(status.is_ok_and(|s| s.success()));
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
    pub answers: Mutex<VecDeque<Vec<(EffectId, Answer)>>>,
    pub issued: Mutex<Vec<ConfirmDecision>>,
    pub refuse_issue: Mutex<bool>,
    pub allow: Mutex<OriginAnswer>,
    pub asked_origins: Mutex<Vec<WebOrigin>>,
    pub handbacks: Mutex<Vec<HandbackRequest>>,
    pub progress: Mutex<Vec<(u32, u32)>>,
    pub hold_answers: Mutex<bool>,
    pub asked_confirms: AtomicUsize,
    pub hold_allow: Mutex<bool>,
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
            hold_answers: Mutex::new(false),
            asked_confirms: AtomicUsize::new(0),
            hold_allow: Mutex::new(false),
        }
    }
}

impl Confirmer for People {
    fn answers(&self, _run: &RunId, request: &ConfirmRequest) -> Vec<(EffectId, Answer)> {
        self.asked_confirms.fetch_add(1, Ordering::SeqCst);
        while *self.hold_answers.lock().unwrap() {
            thread::sleep(Duration::from_millis(10));
        }
        self.answers.lock().unwrap().pop_front().unwrap_or_else(|| {
            request
                .effects
                .iter()
                .map(|e| (e.effect.clone(), Answer::AllowOnce))
                .collect()
        })
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
        while *self.hold_allow.lock().unwrap() {
            thread::sleep(Duration::from_millis(10));
        }
        *self.allow.lock().unwrap()
    }

    fn progress(&self, _run: &RunId, step: NonZeroU32, of: NonZeroU32) {
        self.progress.lock().unwrap().push((step.get(), of.get()));
    }
}

/// The secret store: the answer that it holds for an agent and a name, one answer for every other read, and each
/// read as the agent and the name that it got.
pub struct Vault {
    pub held: Mutex<Vec<(AgentId, Name, SecretReply)>>,
    pub other: Mutex<SecretReply>,
    pub calls: Mutex<Vec<(AgentId, Name)>>,
}

impl Vault {
    pub fn reads(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Secrets for Vault {
    fn read(&self, agent: &AgentId, name: &Name) -> SecretReply {
        self.calls
            .lock()
            .unwrap()
            .push((agent.clone(), name.clone()));
        let held = self.held.lock().unwrap();
        match held.iter().find(|(a, n, _)| a == agent && n == name) {
            Some((_, _, reply)) => reply.clone(),
            None => self.other.lock().unwrap().clone(),
        }
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
        Rig::configured(name, |c| {
            c.runner = runner;
            c.start_wait = start_wait;
        })
    }

    pub fn configured(name: &str, set: impl FnOnce(&mut Config)) -> Rig {
        Rig::built(name, set, None, None)
    }

    pub fn reading(name: &str, read: fn(SystemValue) -> Option<std::ffi::OsString>) -> Rig {
        Rig::built(name, |_| {}, Some(read), None)
    }

    /// A rig whose supervisor sends its owner requests over `link`; its memory store gets none of them.
    pub fn linked(name: &str, link: Arc<dyn OwnerLink>) -> Rig {
        Rig::built(name, |_| {}, None, Some(link))
    }

    fn built(
        name: &str,
        set: impl FnOnce(&mut Config),
        read: Option<fn(SystemValue) -> Option<std::ffi::OsString>>,
        link: Option<Arc<dyn OwnerLink>>,
    ) -> Rig {
        let dir = scratch(name);
        let store = Arc::new(Store::default());
        let people = Arc::new(People::default());
        let vault = Arc::new(Vault {
            held: Mutex::new(Vec::new()),
            other: Mutex::new(SecretReply::None),
            calls: Mutex::new(Vec::new()),
        });
        let rec = Arc::new(Recorder::default());
        let parts = Parts {
            link: link.unwrap_or_else(|| store.clone()),
            confirmer: people.clone(),
            human: people.clone(),
            secrets: vault.clone(),
            marks: rec.clone(),
        };
        let mut config = Config::new(dir.join("data"), peer());
        config.start_wait = Duration::from_secs(10);
        set(&mut config);
        let sup = match read {
            Some(read) => {
                Supervisor::with_system_reader(CoordinatorRole::claim(), config, parts, read)
            }
            None => Supervisor::new(CoordinatorRole::claim(), config, parts),
        };
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
            utc_offset_s: 0,
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
        end_logged(&self.dir);
    }
}

/// Ends every stand-in process that a peer under `dir` logged as made, while it still has its logged number and
/// creation time: the runners, their children, the browsers and the browsers' children.
pub fn end_logged(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "log") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        for line in text.lines() {
            let word = line.split(' ').next().unwrap_or("");
            if !matches!(word, "runner" | "child" | "browser-real" | "browser-child") {
                continue;
            }
            let mut pid = None;
            let mut created = None;
            for part in line.split(' ') {
                if let Some(p) = part.strip_prefix("pid=") {
                    pid = p.parse::<u32>().ok();
                }
                if let Some(c) = part.strip_prefix("created=") {
                    created = CreationTime::parse(c).ok();
                }
            }
            if let (Some(pid), Some(created)) = (pid, created) {
                if process::is_running(pid, created) {
                    let _ = kill_quiet(pid);
                }
            }
        }
    }
}

fn kill_quiet(pid: u32) -> bool {
    let status = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    } else {
        std::process::Command::new("/bin/kill")
            .args(["-KILL", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
    };
    status.is_ok_and(|s| s.success())
}
pub const PAYMENT: &str = r#"{"v":1,"workflow":"{workflow}","agent":"{agent}","run":"{run}","effect":"EFFECT","seq":SEQ,"day":20768,"at":1794339000000000001,"state":"STATE","class":"payment","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","kept":[{"name":"payee","value":["Northwind Power"]},{"name":"amount","value":["84.20"]},{"name":"instrument","value":["card 4242"]},{"name":"origin","value":["http://pay.test:18090"]}],"fields":4,"approval":APPROVAL,"cause":null}"#;

pub fn ask_store(id: &str, body: &str) -> String {
    let envelope = r#"{"v":1,"id":"ID","request":"runner","body":BODY}"#
        .replace("ID", id)
        .replace("BODY", body);
    format!(r#"ask {{"t":"store","request":{envelope}}}"#)
}

pub fn effect(effect: &str, seq: u32, state: &str) -> String {
    let approval = match state {
        "approved" | "releasing" => r#""70707070707070707070707070707070""#,
        _ => "null",
    };
    let record = PAYMENT
        .replace("EFFECT", effect)
        .replace("SEQ", &seq.to_string())
        .replace("STATE", state)
        .replace("APPROVAL", approval);
    r#"{"op":"effect","agent":"{agent}","run":"{run}","record":RECORD}"#.replace("RECORD", &record)
}

/// A completed run's record made by `AgentRun::new`, with `{workflow}`, `{agent}` and `{run}` in place of its ids,
/// which the peer fills in as it does for every body of a script.
pub fn run_record() -> String {
    let workflow = WorkflowId::from_random([0xf1; 16]);
    let agent = AgentId::from_random([0xf2; 16]);
    let run = RunId::from_random([0xf3; 16]);
    let mut made = AgentRun::new(
        workflow.clone(),
        agent.clone(),
        run.clone(),
        20768,
        Some(Mode::Replay),
        Outcome::Completed,
    );
    made.playbook = NonZeroU32::new(1);
    made.steps = 9;
    made.prompts = 2;
    made.handbacks = 1;
    made.confirmations = 1;
    made.effects = 1;
    made.wall_s = 41;
    made.to_json()
        .expect("a made record")
        .replacen(workflow.as_str(), "{workflow}", 1)
        .replacen(agent.as_str(), "{agent}", 1)
        .replacen(run.as_str(), "{run}", 1)
}

pub fn ask_finish() -> String {
    let finish = r#"{"op":"finish","agent":"{agent}","run":"{run}","record":RECORD}"#
        .replace("RECORD", &run_record());
    ask_store(&id(0x51), &finish)
}

pub fn id(n: u8) -> String {
    format!("{n:02x}").repeat(16)
}
