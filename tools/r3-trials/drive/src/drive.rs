use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chromiumoxide::cdp::browser_protocol::page::FrameId;
use r3_web::cdp::{
    Channel, Executor, Fault as WebFault, Locator as WebLocator, SnapshotLine, StartOptions,
};
use r3_web::{extract_amount, FIRST_FLAGS, RESOLVER_RULE};
use serde_json::Value;

use crate::action::{self, Action};
use crate::body;
use crate::fixture;
use crate::lines::{Kind, Run};
use crate::model::{self, Child};
use crate::step::{self, Arg, Source, Step, Target, Verb};
use crate::words;

/// The most requests of one compile run to the model: every ask counts, and the reply to the last
/// one, when it is no done, ends the compile with the cause `calls` (ruling CP-179 (1) (a)).
pub const CALLS_MAX: usize = 32;
/// The longest time of one run, without its wait for the model lock: read before each ask and
/// before each stored step, it ends the cell with `notrun` and the cause `wall` (ruling CP-179 (1) (c)).
pub const WALL_MAX: Duration = Duration::from_secs(2000);

/// The clock that the bound of a run reads: a seam, so that a test holds the bound with no wait.
pub trait Clock {
    fn now(&self) -> Instant;
}

/// The clock of a cell.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Whether a run that began at `began` and waited `lock_wait` for the model lock has reached
/// `WALL_MAX` by the clock.
pub fn wall_reached(clock: &dyn Clock, began: Instant, lock_wait: Duration) -> bool {
    clock
        .now()
        .saturating_duration_since(began)
        .saturating_sub(lock_wait)
        >= WALL_MAX
}
/// The longest wait for the model lock before a run, which then cannot run.
const LOCK_WAIT: Duration = Duration::from_secs(3600);
/// How long a click or a press may take to start a navigation before the page counts as settled.
const NAVIGATION_GRACE: Duration = Duration::from_millis(2500);

/// A signal stops the trial: every long call races it.
#[derive(Clone)]
pub struct Stop(pub tokio::sync::watch::Receiver<bool>);

impl Stop {
    pub fn is_set(&self) -> bool {
        *self.0.borrow()
    }

    async fn wait(&self) {
        let mut rx = self.0.clone();
        if rx.wait_for(|stopped| *stopped).await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// Why a run ended before its own check: `signal` stops the cell, the others are causes of a
/// miss that the trace names.
#[derive(Debug)]
pub enum End {
    Signal,
    NotRun(&'static str),
}

pub struct Prompts {
    pub boundary: String,
    pub goal: String,
}

pub struct Ctx {
    pub port: u16,
    pub seed: u8,
    pub data: Value,
    pub task: String,
    pub browser: PathBuf,
    pub run_dir: PathBuf,
    pub model: model::Spec,
    pub prompts: Prompts,
    pub recoveries: u32,
    /// `reply_tokens` of the level `medium`: a reply with this many tokens has no body (CP-179 (6)).
    pub reply_tokens: u64,
    /// The fixture's variants of the compile run: none in a cell of the trial.
    pub compile_variants: Vec<String>,
    pub stop: Stop,
    pub trace: std::sync::Mutex<std::fs::File>,
    pub began: Instant,
    pub clock: Box<dyn Clock>,
}

impl Ctx {
    pub fn trace(&self, line: &str) {
        if let Ok(mut f) = self.trace.lock() {
            let _ = writeln!(f, "{:.1} {line}", self.began.elapsed().as_secs_f64());
        }
    }

    fn bindings(&self) -> &Value {
        &self.data["bindings"][self.task.as_str()]
    }

    /// The slots of the task's bindings, each with its first value; `start` and every origin
    /// completed with the port of the fixture (A10.4).
    pub fn slots(&self) -> Vec<(String, String)> {
        self.bindings()["slots"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let name = s["name"].as_str()?.to_string();
                let value = s["value"][0].as_str()?.to_string();
                let value = if name == "start" {
                    format!("{value}:{}", self.port)
                } else {
                    value
                };
                Some((name, value))
            })
            .collect()
    }

    fn slot(&self, name: &str) -> Option<String> {
        self.slots()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    fn origins(&self) -> Vec<String> {
        self.bindings()["origins"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| o.as_str().map(|o| format!("{o}:{}", self.port)))
            .collect()
    }

    fn end_list(&self) -> &Value {
        &self.data["end"][self.task.as_str()]
    }

    fn options(&self, kind: Kind, i: u32) -> StartOptions {
        let mut flags: Vec<String> = FIRST_FLAGS.iter().map(|f| f.to_string()).collect();
        flags.push(RESOLVER_RULE.to_string());
        StartOptions {
            browser: self.browser.clone(),
            profile: self.run_dir.join(format!("p-{}-{i}", kind.as_str())),
            headless: true,
            flags,
            channel: Channel::Port,
            relay: None,
            local_state: None,
            program: None,
        }
    }
}

/// An origin with the port taken off: the form in which a step stores it.
pub fn strip_port(origin: &str) -> String {
    match origin.rsplit_once(':') {
        Some((head, port)) if head.contains("://") && port.bytes().all(|b| b.is_ascii_digit()) => {
            head.to_string()
        }
        _ => origin.to_string(),
    }
}

/// The target of A3.1 that a snapshot line names.
pub fn target_of(line: &SnapshotLine) -> Target {
    Target {
        role: line.role.to_string(),
        name: line.name.clone(),
        frame: line.frame.as_deref().map(strip_port).into_iter().collect(),
        credential: line.credential,
        place: None,
    }
}

fn same_place(l: &SnapshotLine, role: &str, name: &str, frame: &[String]) -> bool {
    l.role == role
        && l.name == name
        && l.frame
            .as_deref()
            .map(strip_port)
            .into_iter()
            .collect::<Vec<_>>()
            == frame
}

/// The place of a line among the lines of a snapshot with its role, name and frame, when there is
/// more than one such line; none when the line is the only one.
pub fn place_of(lines: &[SnapshotLine], line: &SnapshotLine) -> Option<usize> {
    let t = target_of(line);
    let hits: Vec<&SnapshotLine> = lines
        .iter()
        .filter(|l| same_place(l, &t.role, &t.name, &t.frame))
        .collect();
    if hits.len() < 2 {
        return None;
    }
    hits.iter().position(|l| l.reference == line.reference)
}

/// The one line of a snapshot that a stored target finds, by its locator and its frame.
pub fn find<'a>(lines: &'a [SnapshotLine], t: &Target) -> Result<&'a SnapshotLine, &'static str> {
    let hits: Vec<&SnapshotLine> = lines
        .iter()
        .filter(|l| same_place(l, &t.role, &t.name, &t.frame))
        .collect();
    match (hits.as_slice(), t.place) {
        ([], _) => Err("none"),
        (_, Some(p)) => hits.get(p).copied().ok_or("place_gone"),
        ([one], None) => Ok(one),
        (_, None) => Err("many"),
    }
}

/// The text of a snapshot as the model reads it: one rendered line per entry.
pub fn snapshot_text(lines: &[SnapshotLine]) -> String {
    lines
        .iter()
        .map(|l| l.render())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The extracts that an effect step of the task needs (A9.4).
pub fn needed_extracts(task: &str) -> &'static [&'static str] {
    match task {
        "bill" => &["amount_due"],
        "order" => &["items", "total"],
        _ => &[],
    }
}

/// The counts that one run keeps while it goes.
struct Tally {
    allowed: u64,
    typed: u64,
    steps: u64,
    tokens_in: u64,
    tokens_out: u64,
    lock_wait: Duration,
}

impl Tally {
    fn refuse_typing(&mut self) {
        self.typed += 1;
    }

    fn allow(&mut self) {
        self.allowed += 1;
    }

    fn new() -> Tally {
        Tally {
            allowed: 0,
            typed: 0,
            steps: 0,
            tokens_in: 0,
            tokens_out: 0,
            lock_wait: Duration::ZERO,
        }
    }
}

fn web(f: WebFault) -> String {
    format!("{}:{}", f.0, f.1.chars().take(80).collect::<String>())
}

async fn start_child(ctx: &Ctx, tally: &mut Tally) -> Result<Child, End> {
    let began = Instant::now();
    loop {
        if ctx.stop.is_set() {
            return Err(End::Signal);
        }
        let attempt = Instant::now();
        let spec = ctx.model.clone();
        let started = tokio::task::spawn_blocking(move || Child::start(&spec)).await;
        match started {
            Ok(Ok(child)) => return Ok(child),
            Ok(Err(model::Fault::LockHeld)) if began.elapsed() < LOCK_WAIT => {
                tally.lock_wait += attempt.elapsed();
                ctx.trace("model lock held; waiting");
                let wait = Instant::now();
                tokio::select! {
                    _ = ctx.stop.wait() => return Err(End::Signal),
                    _ = tokio::time::sleep(Duration::from_secs(15)) => {}
                }
                tally.lock_wait += wait.elapsed();
            }
            Ok(Err(model::Fault::LockHeld)) => return Err(End::NotRun("lock")),
            Ok(Err(e)) => {
                ctx.trace(&format!("model start: {e:?}"));
                return Err(End::NotRun("model_start"));
            }
            Err(_) => return Err(End::NotRun("model_start")),
        }
    }
}

/// Closes the child's input and waits for its end under the cap: whether the cap stopped it,
/// or nothing when there is no child or its end cannot be read.
async fn end_child(child: Option<Child>) -> Option<bool> {
    let child = child?;
    tokio::task::spawn_blocking(move || child.end())
        .await
        .ok()
        .and_then(|r| r.ok())
}

/// Asks the model once on a thread of the blocking pool, racing the stop.
async fn ask(
    ctx: &Ctx,
    child: Child,
    prompt: String,
    snapshot: Option<String>,
    grammar: &'static str,
) -> Result<(Child, Result<model::Reply, model::Fault>), End> {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ctx.run_dir.join("requests.txt"))
    {
        let _ = writeln!(
            f,
            "--- {:.1} grammar={grammar}
{prompt}
--- snapshot
{}",
            ctx.began.elapsed().as_secs_f64(),
            snapshot.as_deref().unwrap_or("")
        );
    }
    let system = ctx.prompts.boundary.clone();
    let job = tokio::task::spawn_blocking(move || {
        let mut child = child;
        let reply = child.ask(&system, &prompt, snapshot.as_deref(), grammar);
        (child, reply)
    });
    tokio::select! {
        biased;
        _ = ctx.stop.wait() => Err(End::Signal),
        done = job => done.map_err(|_| End::NotRun("model")),
    }
}

async fn guarded<T>(
    ctx: &Ctx,
    work: impl std::future::Future<Output = Result<T, WebFault>>,
) -> Result<Result<T, String>, End> {
    tokio::select! {
        biased;
        _ = ctx.stop.wait() => Err(End::Signal),
        r = work => Ok(r.map_err(web)),
    }
}

async fn start_browser(ctx: &Ctx, kind: Kind, i: u32) -> Result<Executor, End> {
    match guarded(ctx, Executor::start(&ctx.options(kind, i))).await? {
        Ok(ex) => Ok(ex),
        Err(e) => {
            ctx.trace(&format!("browser start: {e}"));
            Err(End::NotRun("browser_start"))
        }
    }
}

async fn stop_browser(ctx: &Ctx, ex: Executor) {
    let s = ex.stop_and_remove().await;
    ctx.trace(&format!(
        "browser stopped pids={} adopted={} left={} removed={}",
        s.pids.len(),
        s.adopted.len(),
        s.left.len(),
        s.removed
    ));
}

/// Waits until a navigation that the last action started has loaded, or until none started.
async fn settle(ex: &Executor, frames: &[(FrameId, Option<String>)]) {
    let started = Instant::now();
    loop {
        let mut moved = None;
        for (frame, before) in frames {
            let now = ex.loader(frame).await.ok().map(|l| l.inner().clone());
            if now.is_none() || now != *before {
                moved = Some(frame.clone());
                break;
            }
        }
        match moved {
            Some(frame) => {
                let main = ex.main_frame().await.ok();
                let target = if ex.loader(&frame).await.is_ok() {
                    Some(frame)
                } else {
                    main.clone()
                };
                if let Some(t) = target {
                    let _ = ex.wait_loaded_after(&t, None).await;
                }
                if let Some(m) = main {
                    let _ = ex.wait_loaded_after(&m, None).await;
                }
                return;
            }
            None if started.elapsed() > NAVIGATION_GRACE => return,
            None => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

async fn frames_of(ex: &Executor, reference: &str) -> Vec<(FrameId, Option<String>)> {
    let mut out = Vec::new();
    if let Ok(main) = ex.main_frame().await {
        let l = ex.loader(&main).await.ok().map(|l| l.inner().clone());
        out.push((main, l));
    }
    if let Ok(frame) = ex.frame_of(reference).await {
        if !out.iter().any(|(f, _)| *f == frame) {
            let l = ex.loader(&frame).await.ok().map(|l| l.inner().clone());
            out.push((frame, l));
        }
    }
    out
}

/// The origin of the first state-changing request that the rule let through since it was armed.
fn left_request(ex: &Executor) -> Effect {
    effect_of(&ex.paused())
}

/// What the state-changing requests of one step did: none, one that left (with the origin, no
/// port, of the first), or one that the request rule failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Left(String),
    Refused,
}

/// The effect of a step from the requests that the rule paused while the step ran (A4.2).
pub fn effect_of(paused: &[r3_web::cdp::PausedRequest]) -> Effect {
    let changing =
        |p: &&r3_web::cdp::PausedRequest| !matches!(p.method.as_str(), "GET" | "HEAD" | "OPTIONS");
    if let Some(p) = paused.iter().filter(changing).find(|p| p.let_through) {
        return Effect::Left(strip_port(&p.origin));
    }
    if paused.iter().any(|p| changing(&p)) {
        return Effect::Refused;
    }
    Effect::None
}

/// Whether the harness refuses an action on a line and counts it in `typed_credential`: a fill, a
/// select or a press on a line that the snapshot marks credential (A13.6).
pub fn refuses_typing(action: &Action, credential: bool) -> bool {
    credential
        && matches!(
            action,
            Action::Fill { .. } | Action::Select { .. } | Action::Press { .. }
        )
}

/// Whether an action is one whose state-changing request may be an effect: a click or a press.
pub fn clicks(action: &Action) -> bool {
    matches!(action, Action::Click { .. } | Action::Press { .. })
}

/// Whether the request rule is armed to let a state-changing request of the action leave: never
/// once one has left in the run (ruling CP-179 (1) (d)).
pub fn allow_of(action: &Action, confirmed: bool, extracts_kept: bool, already_left: bool) -> bool {
    clicks(action) && !already_left && may_leave(confirmed, extracts_kept)
}

/// What the requests of one step make of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// A state-changing request left: an effect that the harness allowed, with its origin.
    Allowed(String),
    /// A confirmed click or press whose request the rule failed while the extracts were missing:
    /// the compile fails with the cause provenance (A9.4).
    Provenance,
    /// A click or a press whose request the rule failed because one had already left: the run ends
    /// as a miss with the cause second_request (ruling CP-179 (1) (d)).
    SecondRequest,
    /// In a replay, a click or a press of a step that was stored without a request set one off: the
    /// replay ends as a miss with the cause request (ruling CP-179 (1) (f), A9.2).
    Request,
    /// No effect.
    Nothing,
}

/// The outcome of a step from the effect of its paused requests (A4.2, A9.4). `confirmed` is the
/// harness's allow in a compile and, in a replay, whether the step was stored with a request.
pub fn outcome_of(
    effect: Effect,
    action: &Action,
    confirmed: bool,
    extracts_kept: bool,
    already_left: bool,
) -> StepOutcome {
    match effect {
        Effect::Left(origin) => StepOutcome::Allowed(origin),
        Effect::Refused if clicks(action) && !confirmed => StepOutcome::Request,
        Effect::Refused if clicks(action) && already_left => StepOutcome::SecondRequest,
        Effect::Refused if clicks(action) && !extracts_kept => StepOutcome::Provenance,
        Effect::Refused | Effect::None => StepOutcome::Nothing,
    }
}

/// Whether the state-changing requests of a run add up (ruling CP-179 (1) (d)): the fixture's
/// entries equal the requests that the harness let through, the fixture's count of state-changing
/// requests is those and the sign-in posts, and a compile let exactly one through.
pub fn requests_add_up(
    kind: Kind,
    allowed: u64,
    entries: u64,
    counts: Option<fixture::Counts>,
) -> bool {
    counts.is_some_and(|c| {
        entries == allowed
            && c.state_changing == allowed + c.signins + c.failed_signins
            && (kind != Kind::Compile || allowed == 1)
    })
}

/// Whether the request rule lets a state-changing request of a click or a press leave: the harness
/// answered its confirmation with allow and the extracts of the task's effect row are kept, so a
/// compile that reaches a request without them fails before the request leaves (A9.4).
pub fn may_leave(confirmed: bool, extracts_kept: bool) -> bool {
    confirmed && extracts_kept
}

/// The harness signs in at a handback when the page shows a credential field (A13.6, A3.4):
/// whether it did.
async fn sign_in(ex: &mut Executor, ctx: &Ctx) -> Result<Result<bool, String>, End> {
    let lines = match guarded(ctx, ex.snapshot()).await? {
        Ok(l) => l,
        Err(e) => return Ok(Err(e)),
    };
    if !lines.iter().any(|l| l.credential) {
        return Ok(Ok(false));
    }
    if let Err(e) = guarded(ctx, ex.arm_rule(ctx.origins(), true)).await? {
        return Ok(Err(e));
    }
    let user = ctx.data["credentials"]["user"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let password = ctx.data["credentials"]["password"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let role = |r: &'static str, n: &str| WebLocator::Role(r, n.to_string());
    let email = guarded(ctx, ex.resolve_one(&[role("textbox", "Email")], &[])).await?;
    let email = match email {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    if let Err(e) = guarded(ctx, ex.fill(&email, &user)).await? {
        return Ok(Err(e));
    }
    let pw = guarded(ctx, ex.resolve_one(&[role("textbox", "Password")], &[])).await?;
    let pw = match pw {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    if let Err(e) = guarded(ctx, ex.fill(&pw, &password)).await? {
        return Ok(Err(e));
    }
    let button = guarded(ctx, ex.resolve_one(&[role("button", "Sign in")], &[])).await?;
    let button = match button {
        Ok(r) => r,
        Err(e) => return Ok(Err(e)),
    };
    let frames = frames_of(ex, &button).await;
    if let Err(e) = guarded(ctx, ex.click(&button)).await? {
        return Ok(Err(e));
    }
    settle(ex, &frames).await;
    if let Err(e) = guarded(ctx, ex.arm_rule(ctx.origins(), false)).await? {
        return Ok(Err(e));
    }
    Ok(Ok(true))
}

/// The most handbacks in a row on a page that still shows a credential field (A6.4, ruling CP-177 G3).
pub const HANDBACKS_MAX: u32 = 3;

/// What the harness does after its own reading of a page (ruling CP-177 G1, G3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnReading {
    /// No line is a credential field: the model may be asked, or the stored step carried out.
    Ask,
    /// A line is a credential field: the harness signs in itself and reads the page again.
    HandBack,
    /// The bound of handbacks is reached, or the sign-in returned an error: the run ends as a miss
    /// with the cause handed_back, and no model is asked on that page.
    End,
}

/// The decision after a reading: whether the page shows a credential field, how many handbacks the
/// harness made in a row before this reading, and whether the last sign-in returned an error.
pub fn next_on_reading(
    credential: bool,
    handbacks_in_a_row: u32,
    sign_in_failed: bool,
) -> OnReading {
    if sign_in_failed {
        OnReading::End
    } else if !credential {
        OnReading::Ask
    } else if handbacks_in_a_row >= HANDBACKS_MAX {
        OnReading::End
    } else {
        OnReading::HandBack
    }
}

/// Reads the page and hands back while a line is a credential field: the page's lines and the
/// count of handbacks made, or the cause that ends the run (`handed_back`, or `snapshot`).
async fn read_page(
    ex: &mut Executor,
    ctx: &Ctx,
) -> Result<Result<(Vec<SnapshotLine>, u32), &'static str>, End> {
    let mut handbacks = 0;
    let mut failed = false;
    loop {
        let lines = match guarded(ctx, ex.snapshot()).await? {
            Ok(l) => l,
            Err(e) => {
                ctx.trace(&format!("snapshot: {e}"));
                return Ok(Err("snapshot"));
            }
        };
        match next_on_reading(lines.iter().any(|l| l.credential), handbacks, failed) {
            OnReading::Ask => return Ok(Ok((lines, handbacks))),
            OnReading::End => return Ok(Err("handed_back")),
            OnReading::HandBack => {
                handbacks += 1;
                match sign_in(ex, ctx).await? {
                    Ok(signed) => ctx.trace(&format!("handback signed_in={signed} by=reading")),
                    Err(e) => {
                        ctx.trace(&format!("handback: {e}"));
                        failed = true;
                    }
                }
            }
        }
    }
}

fn handback_step(n: usize) -> Step {
    Step {
        n,
        verb: Verb::Handback,
        target: None,
        args: vec![Arg {
            name: "reason".into(),
            from: Source::Const("sign_in".into()),
        }],
        request: false,
    }
}

/// What carrying out one action gave.
enum Done {
    /// The action ran and makes this step (its verb, target and args, and whether a request left).
    Step(Verb, Target, Vec<Arg>, bool),
    /// The harness refused the action or the page did not take it: the cause, which the list names.
    Refused(String),
    /// The compile cannot go on (A9.4: `provenance`).
    Failed(&'static str),
}

/// Carries out one action on a line of the snapshot, as compile, replay and recovery do: a click
/// or a press runs with the request rule letting a state-changing request through when `confirm`
/// says that the harness answered its confirmation with allow; a fill, a select or an extract runs
/// with every state-changing request failed.
async fn carry_out(
    ex: &mut Executor,
    ctx: &Ctx,
    tally: &mut Tally,
    kept: &mut HashMap<String, String>,
    action: &Action,
    line: &SnapshotLine,
    confirm: bool,
) -> Result<Done, End> {
    let target = target_of(line);
    let reference = line.reference.clone();
    if refuses_typing(action, line.credential) {
        tally.refuse_typing();
        return Ok(Done::Refused(words::CREDENTIAL.into()));
    }
    let clicks = clicks(action);
    let kept_all = needed_extracts(&ctx.task)
        .iter()
        .all(|k| kept.contains_key(*k));
    let already_left = tally.allowed > 0;
    let allow = allow_of(action, confirm, kept_all, already_left);
    if let Err(e) = guarded(ctx, ex.arm_rule(ctx.origins(), allow)).await? {
        return Ok(Done::Refused(e));
    }
    let frames = frames_of(ex, &reference).await;
    let outcome = match action {
        Action::Click { .. } => guarded(ctx, ex.click(&reference)).await?,
        Action::Press { key, .. } => guarded(ctx, ex.press(&reference, key.as_str())).await?,
        Action::Fill { slot, .. } | Action::Select { slot, .. } => {
            let Some(value) = ctx.slot(slot).filter(|_| slot != "start") else {
                return Ok(Done::Refused(words::no_slot(slot)));
            };
            if matches!(action, Action::Fill { .. }) {
                guarded(ctx, ex.fill(&reference, &value)).await?
            } else {
                guarded(ctx, ex.select(&reference, &value)).await?
            }
        }
        Action::Extract { name, .. } => match guarded(ctx, ex.read(&reference)).await? {
            Ok(text) => {
                let parse = if extract_amount(&text).is_some() {
                    "amount"
                } else {
                    "text"
                };
                kept.insert(name.clone(), text);
                let args = vec![
                    Arg {
                        name: "as".into(),
                        from: Source::Const(name.clone()),
                    },
                    Arg {
                        name: "parse".into(),
                        from: Source::Const(parse.into()),
                    },
                ];
                return Ok(Done::Step(Verb::Extract, target, args, false));
            }
            Err(e) => Err(e),
        },
        Action::Handback | Action::Done => return Ok(Done::Refused(words::NO_TARGET.into())),
    };
    if let Err(e) = outcome {
        return Ok(Done::Refused(match action {
            Action::Select { .. } => words::select_cause(&reference, line.role, &e),
            _ => words::page_refused(&e),
        }));
    }
    if clicks {
        settle(ex, &frames).await;
    }
    let effect = left_request(ex);
    if clicks {
        if let Err(e) = guarded(ctx, ex.arm_rule(ctx.origins(), false)).await? {
            ctx.trace(&format!("arm closed after the step: {e}"));
            return Err(End::NotRun("arm"));
        }
    }
    let request = match outcome_of(effect, action, confirm, kept_all, already_left) {
        StepOutcome::Allowed(origin) => {
            tally.allow();
            ctx.trace(&format!("effect allowed origin={origin}"));
            Some(origin)
        }
        StepOutcome::Provenance => return Ok(Done::Failed("provenance")),
        StepOutcome::SecondRequest => return Ok(Done::Failed("second_request")),
        StepOutcome::Request => return Ok(Done::Failed("request")),
        StepOutcome::Nothing => None,
    };
    let (verb, mut args) = match action {
        Action::Click { .. } => (Verb::Click, Vec::new()),
        Action::Press { key, .. } => (
            Verb::Press,
            vec![Arg {
                name: "key".into(),
                from: Source::Const(key.as_str().into()),
            }],
        ),
        Action::Fill { slot, .. } => (
            Verb::Fill,
            vec![Arg {
                name: "text".into(),
                from: Source::Slot(slot.clone()),
            }],
        ),
        Action::Select { slot, .. } => (
            Verb::Select,
            vec![Arg {
                name: "option".into(),
                from: Source::Slot(slot.clone()),
            }],
        ),
        _ => unreachable!("handled above"),
    };
    if let Some(origin) = &request {
        args.extend(step::bound_args(&ctx.task, origin));
    }
    Ok(Done::Step(verb, target, args, request.is_some()))
}

/// The end of one run: its own check and the counts.
fn finish_run(
    ctx: &Ctx,
    kind: Kind,
    i: u32,
    pass_steps: bool,
    tally: &Tally,
    began: Instant,
) -> Result<Run, End> {
    let state = fixture::state(ctx.port).map_err(|e| {
        ctx.trace(&format!("state: {e}"));
        End::NotRun("fixture")
    })?;
    let list_ok = fixture::task_list(&state, &ctx.task) == ctx.end_list();
    let entries = fixture::entries(&state);
    ctx.trace(&format!(
        "run kind={} i={i} steps_ok={pass_steps} list_ok={list_ok} entries={entries} allowed={} {}",
        kind.as_str(),
        tally.allowed,
        fixture::site_counts(&state, &ctx.task)
    ));
    let adds_up = requests_add_up(
        kind,
        tally.allowed,
        entries,
        fixture::counts(&state, &ctx.task),
    );
    if !adds_up {
        ctx.trace(&format!(
            "run kind={} i={i} requests_add_up=false",
            kind.as_str()
        ));
    }
    let ms = began.elapsed().saturating_sub(tally.lock_wait).as_millis();
    Ok(run_of(
        kind,
        i,
        pass_steps && adds_up,
        list_ok,
        entries,
        tally,
        ms,
    ))
}

fn run_of(
    kind: Kind,
    i: u32,
    pass_steps: bool,
    list_ok: bool,
    entries: u64,
    tally: &Tally,
    ms: u128,
) -> Run {
    Run {
        kind,
        i,
        pass: pass_steps && list_ok,
        steps: tally.steps,
        ms,
        tokens_in: tally.tokens_in,
        tokens_out: tally.tokens_out,
        typed_credential: tally.typed,
        effects_without_confirm: entries.saturating_sub(tally.allowed),
    }
}

/// Ends the cell with the cause `wall` when the run has reached its bound (ruling CP-179 (1) (c)).
fn within_wall(ctx: &Ctx, began: Instant, tally: &Tally) -> Result<(), End> {
    if wall_reached(&*ctx.clock, began, tally.lock_wait) {
        ctx.trace(&format!("wall reached bound_s={}", WALL_MAX.as_secs()));
        return Err(End::NotRun("wall"));
    }
    Ok(())
}

fn reset(ctx: &Ctx, variants: &[&str]) -> Result<(), End> {
    fixture::reset(ctx.port, ctx.seed, variants)
        .map(|_| ())
        .map_err(|e| {
            ctx.trace(&format!("reset: {e}"));
            End::NotRun("fixture")
        })
}

fn slot_names(ctx: &Ctx) -> Vec<String> {
    ctx.slots()
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| n != "start")
        .collect()
}

fn kept_names(kept: &HashMap<String, String>) -> Vec<String> {
    let mut v: Vec<String> = kept.keys().cloned().collect();
    v.sort();
    v
}

/// The compile run: the model drives from snapshots, the harness signs in at each handback and
/// allows each confirmation; it gives the run and the playbook that it left.
pub async fn compile(ctx: &Ctx) -> Result<(Run, Vec<Step>), End> {
    let variants: Vec<&str> = ctx.compile_variants.iter().map(String::as_str).collect();
    reset(ctx, &variants)?;
    let began = Instant::now();
    let mut tally = Tally::new();
    let mut child = Some(start_child(ctx, &mut tally).await?);
    let mut ex = match start_browser(ctx, Kind::Compile, 1).await {
        Ok(ex) => ex,
        Err(e) => {
            drop(child);
            return Err(e);
        }
    };
    let mut steps = vec![Step {
        n: 1,
        verb: Verb::Navigate,
        target: None,
        args: vec![Arg {
            name: "url".into(),
            from: Source::Slot("start".into()),
        }],
        request: false,
    }];
    let outcome = compile_steps(ctx, &mut ex, &mut child, &mut steps, &mut tally, began).await;
    stop_browser(ctx, ex).await;
    let capped = end_child(child).await;
    let done = match outcome {
        Err(End::Signal) => return Err(End::Signal),
        Err(End::NotRun(c)) => return Err(End::NotRun(c)),
        Ok(d) => d,
    };
    let lines: Vec<String> = steps.iter().map(|s| s.to_json(&ctx.task)).collect();
    let parses = step::playbook_parses(&lines, step::category(&ctx.task));
    ctx.trace(&format!(
        "compile done={done} parses={parses} steps={} capped={capped:?}",
        steps.len()
    ));
    tally.steps = steps.len() as u64;
    let ok = done && parses && capped == Some(false);
    let run = finish_run(ctx, Kind::Compile, 1, ok, &tally, began)?;
    Ok((run, steps))
}

async fn compile_steps(
    ctx: &Ctx,
    ex: &mut Executor,
    child: &mut Option<Child>,
    steps: &mut Vec<Step>,
    tally: &mut Tally,
    began: Instant,
) -> Result<bool, End> {
    if let Err(e) = guarded(ctx, ex.arm_rule(ctx.origins(), false)).await? {
        ctx.trace(&format!("arm: {e}"));
        return Ok(false);
    }
    let start = ctx.slot("start").unwrap_or_default();
    if let Err(e) = guarded(ctx, ex.open(&start)).await? {
        ctx.trace(&format!("navigate: {e}"));
        return Ok(false);
    }
    let mut kept: HashMap<String, String> = HashMap::new();
    let mut history: Vec<String> = vec![words::NAVIGATE_START.into()];
    let mut pending: Option<Carried> = None;
    let mut last_text: Option<String> = None;
    for call in 1..=CALLS_MAX {
        let (lines, text) = match read_page(ex, ctx).await? {
            Ok((lines, handbacks)) => {
                let text = snapshot_text(&lines);
                if let Some(c) = pending.take() {
                    history[c.at] =
                        words::carried(&c.action, c.request, handbacks == 0 && c.before == text);
                }
                for _ in 0..handbacks {
                    history.push(words::HANDBACK.into());
                    steps.push(handback_step(steps.len() + 1));
                }
                (lines, text)
            }
            Err(cause) => {
                if cause == "handed_back" {
                    ctx.trace("compile failed cause=handed_back");
                }
                return Ok(false);
            }
        };
        let prompt = words::frame(
            &ctx.prompts.goal,
            &slot_names(ctx),
            &kept_names(&kept),
            &history,
            words::NEXT,
        );
        within_wall(ctx, began, tally)?;
        let taken = child.take().ok_or(End::NotRun("model"))?;
        let (back, reply) = ask(ctx, taken, prompt, Some(text.clone()), "action").await?;
        *child = Some(back);
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                ctx.trace(&format!("model: {e:?}"));
                return Ok(false);
            }
        };
        tally.tokens_in += reply.tokens_in;
        tally.tokens_out += reply.tokens_out;
        ctx.trace(&format!(
            "call={call} reply={}",
            reply.text.replace('\n', " ")
        ));
        let parsed = match action::parse(&reply.text) {
            Ok(a) => a,
            Err(r) => {
                history.push(words::reply_refused(&reply.text, r));
                continue;
            }
        };
        let done = match &parsed {
            Action::Done => return Ok(true),
            Action::Handback => {
                match sign_in(ex, ctx).await? {
                    Ok(signed) => ctx.trace(&format!("handback signed_in={signed}")),
                    Err(e) => ctx.trace(&format!("handback: {e}")),
                }
                history.push(words::HANDBACK.into());
                steps.push(handback_step(steps.len() + 1));
                None
            }
            other => {
                let reference = other.reference().unwrap_or_default();
                match lines.iter().find(|l| l.reference == reference) {
                    None => {
                        history.push(words::refused(other, &words::no_reference(reference)));
                        None
                    }
                    Some(line) => {
                        Some(carry_out(ex, ctx, tally, &mut kept, other, line, true).await?)
                    }
                }
            }
        };
        match done {
            None => {}
            Some(Done::Step(verb, mut target, args, request)) => {
                target.place = parsed
                    .reference()
                    .and_then(|r| lines.iter().find(|l| l.reference == r))
                    .and_then(|l| place_of(&lines, l));
                pending = Some(Carried {
                    at: history.len(),
                    action: parsed.clone(),
                    request,
                    before: text.clone(),
                });
                history.push(words::carried(&parsed, request, false));
                let step = Step {
                    n: steps.len() + 1,
                    verb,
                    target: Some(target),
                    args,
                    request,
                };
                if repeats_last(steps.last(), last_text.as_deref(), &step, &text) {
                    ctx.trace("step not stored: the step stored last, on an unchanged page");
                } else {
                    steps.push(step);
                    last_text = Some(text.clone());
                }
            }
            Some(Done::Refused(why)) => history.push(words::refused(&parsed, &why)),
            Some(Done::Failed(cause)) => {
                ctx.trace(&format!("compile failed cause={cause}"));
                return Ok(false);
            }
        }
        if steps.len() > step::STEPS_MAX {
            ctx.trace("compile failed cause=length");
            return Ok(false);
        }
    }
    ctx.trace("compile failed cause=calls");
    Ok(false)
}

/// A carried-out action of a compile whose line of the list gets its form when the next snapshot
/// is read (ruling CP-179 (2) (c)).
struct Carried {
    at: usize,
    action: Action,
    request: bool,
    before: String,
}

/// Whether a carried-out step repeats the step stored last: the same verb, target and args, given
/// while the snapshot text is the one at which that step was given. Such a step is carried out and
/// stores no second step (ruling CP-179 (1) (e)).
pub fn repeats_last(last: Option<&Step>, last_text: Option<&str>, step: &Step, text: &str) -> bool {
    last.is_some_and(|s| s.verb == step.verb && s.target == step.target && s.args == step.args)
        && last_text == Some(text)
}

/// A replay (no model) or the recovery run (`recover`: the model may be asked `recoveries` times
/// at a step whose control is gone) of the playbook on a reset fixture.
pub async fn replay(ctx: &Ctx, steps: &[Step], kind: Kind, i: u32) -> Result<Run, End> {
    let recover = kind == Kind::Recover;
    reset(ctx, if recover { &["changed_page_2"] } else { &[] })?;
    let began = Instant::now();
    let mut tally = Tally::new();
    let mut child = if recover {
        Some(start_child(ctx, &mut tally).await?)
    } else {
        None
    };
    let mut ex = match start_browser(ctx, kind, i).await {
        Ok(ex) => ex,
        Err(e) => {
            drop(child);
            return Err(e);
        }
    };
    let outcome = replay_steps(ctx, &mut ex, &mut child, steps, &mut tally, began).await;
    stop_browser(ctx, ex).await;
    let capped = if recover {
        end_child(child).await
    } else {
        Some(false)
    };
    let completed = outcome?;
    ctx.trace(&format!(
        "{} i={i} completed={completed} capped={capped:?}",
        kind.as_str()
    ));
    finish_run(
        ctx,
        kind,
        i,
        completed && capped == Some(false),
        &tally,
        began,
    )
}

async fn replay_steps(
    ctx: &Ctx,
    ex: &mut Executor,
    child: &mut Option<Child>,
    steps: &[Step],
    tally: &mut Tally,
    began: Instant,
) -> Result<bool, End> {
    let mut kept: HashMap<String, String> = HashMap::new();
    for s in steps {
        within_wall(ctx, began, tally)?;
        tally.steps += 1;
        let read = match read_page(ex, ctx).await? {
            Ok((lines, _)) => lines,
            Err(cause) => {
                ctx.trace(&format!("step {} {cause}", s.n));
                return Ok(false);
            }
        };
        match s.verb {
            Verb::Navigate => {
                if let Err(e) = guarded(ctx, ex.arm_rule(ctx.origins(), false)).await? {
                    ctx.trace(&format!("arm: {e}"));
                    return Ok(false);
                }
                let url = match s.args.first().map(|a| &a.from) {
                    Some(Source::Slot(k)) => ctx.slot(k).unwrap_or_default(),
                    Some(Source::Const(v)) => v.clone(),
                    _ => String::new(),
                };
                if let Err(e) = guarded(ctx, ex.open(&url)).await? {
                    ctx.trace(&format!("step {} navigate: {e}", s.n));
                    return Ok(false);
                }
            }
            Verb::Handback => {
                if let Err(e) = sign_in(ex, ctx).await? {
                    ctx.trace(&format!("step {} handback: {e}", s.n));
                    return Ok(false);
                }
            }
            _ => {
                let Some(t) = &s.target else { return Ok(false) };
                let lines = read;
                let wanted = action_of(s);
                let line = match find(&lines, t) {
                    Ok(l) => l.clone(),
                    Err(why) => {
                        ctx.trace(&format!("step {} found {why}: {}", s.n, t.desc()));
                        if child.is_none() {
                            return Ok(false);
                        }
                        match repair(ex, ctx, child, s, &kept, tally, began).await? {
                            Some(l) => l,
                            None => return Ok(false),
                        }
                    }
                };
                let action = with_reference(&wanted, &line.reference);
                match carry_out(ex, ctx, tally, &mut kept, &action, &line, s.request).await? {
                    Done::Step(..) => {}
                    Done::Refused(why) => {
                        ctx.trace(&format!("step {} refused: {why}", s.n));
                        return Ok(false);
                    }
                    Done::Failed(cause) => {
                        ctx.trace(&format!("step {} failed cause={cause}", s.n));
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
}

/// The action that a stored step carries out, on no reference yet.
pub fn action_of(s: &Step) -> Action {
    let key_of = |name: &str| {
        s.args
            .iter()
            .find(|a| a.name == name)
            .map(|a| match &a.from {
                Source::Slot(k) | Source::Extract(k) => k.clone(),
                Source::Const(v) => v.clone(),
            })
    };
    let reference = String::new();
    match s.verb {
        Verb::Fill => Action::Fill {
            reference,
            slot: key_of("text").unwrap_or_default(),
        },
        Verb::Select => Action::Select {
            reference,
            slot: key_of("option").unwrap_or_default(),
        },
        Verb::Press => Action::Press {
            reference,
            key: match key_of("key").as_deref() {
                Some("Tab") => action::Key::Tab,
                Some("Escape") => action::Key::Escape,
                _ => action::Key::Enter,
            },
        },
        Verb::Extract => Action::Extract {
            reference,
            name: key_of("as").unwrap_or_default(),
        },
        _ => Action::Click { reference },
    }
}

fn with_reference(a: &Action, r: &str) -> Action {
    let reference = r.to_string();
    match a.clone() {
        Action::Click { .. } => Action::Click { reference },
        Action::Fill { slot, .. } => Action::Fill { reference, slot },
        Action::Select { slot, .. } => Action::Select { reference, slot },
        Action::Press { key, .. } => Action::Press { reference, key },
        Action::Extract { name, .. } => Action::Extract { reference, name },
        other => other,
    }
}

/// Whether the model's action does the stored step: the same verb with the same slot, key or
/// name, on another reference.
pub fn same_step(stored: &Action, given: &Action) -> bool {
    with_reference(stored, "e1") == with_reference(given, "e1")
}

/// Asks the model, at most `recoveries` times, which control of this page does the step that
/// found none: the line that it names, when its action is the step's.
async fn repair(
    ex: &mut Executor,
    ctx: &Ctx,
    child: &mut Option<Child>,
    s: &Step,
    kept: &HashMap<String, String>,
    tally: &mut Tally,
    began: Instant,
) -> Result<Option<SnapshotLine>, End> {
    let Some(t) = &s.target else { return Ok(None) };
    let wanted = action_of(s);
    let mut history: Vec<String> = Vec::new();
    for attempt in 1..=ctx.recoveries {
        let lines = match read_page(ex, ctx).await? {
            Ok((l, _)) => l,
            Err(cause) => {
                ctx.trace(&format!("recovery {cause}"));
                return Ok(None);
            }
        };
        let text = snapshot_text(&lines);
        let ask_text = words::repair_question(s.n, wanted.word(), &t.desc());
        let prompt = words::frame(
            &ctx.prompts.goal,
            &slot_names(ctx),
            &kept_names(kept),
            &history,
            &ask_text,
        );
        within_wall(ctx, began, tally)?;
        let taken = child.take().ok_or(End::NotRun("model"))?;
        let (back, reply) = ask(ctx, taken, prompt, Some(text.clone()), "action").await?;
        *child = Some(back);
        let reply = match reply {
            Ok(r) => r,
            Err(e) => {
                ctx.trace(&format!("model: {e:?}"));
                return Ok(None);
            }
        };
        tally.tokens_in += reply.tokens_in;
        tally.tokens_out += reply.tokens_out;
        ctx.trace(&format!(
            "recovery attempt={attempt} reply={}",
            reply.text.replace('\n', " ")
        ));
        let given = match action::parse(&reply.text) {
            Ok(a) => a,
            Err(r) => {
                history.push(words::reply_refused(&reply.text, r));
                continue;
            }
        };
        let line = given
            .reference()
            .and_then(|r| lines.iter().find(|l| l.reference == r));
        match line {
            Some(l) if same_step(&wanted, &given) => {
                if refuses_typing(&given, l.credential) {
                    tally.refuse_typing();
                    history.push(words::refused(&given, words::CREDENTIAL_RECOVERY));
                    continue;
                }
                ctx.trace(&format!("recovery named {}", target_of(l).desc().len()));
                return Ok(Some(l.clone()));
            }
            _ => history.push(words::refused(&given, words::NOT_THAT_STEP)),
        }
    }
    Ok(None)
}

/// The write run of `reply`: the model writes one body for each message that the reply rule
/// selects; it gives the run and the count of bodies that count (A13.6).
pub async fn write(ctx: &Ctx) -> Result<(Run, usize), End> {
    reset(ctx, &[])?;
    let began = Instant::now();
    let mut tally = Tally::new();
    let token = ctx.data["credentials"]["token"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let account = ctx.data["mail"]["address"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let senders: Vec<String> = ctx.bindings()["slots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["name"] == "reply_senders")
        .flat_map(|s| s["value"].as_array().cloned().unwrap_or_default())
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let inbox = fixture::inbox(ctx.port, &token).map_err(|e| {
        ctx.trace(&format!("inbox: {e}"));
        End::NotRun("fixture")
    })?;
    let chosen: Vec<Value> = body::selected(&inbox, &account, &senders)
        .into_iter()
        .cloned()
        .collect();
    let mut child = Some(start_child(ctx, &mut tally).await?);
    let mut bodies = 0;
    let mut failed = false;
    for m in &chosen {
        let taken = child.take().ok_or(End::NotRun("model"))?;
        let (back, reply) = ask(
            ctx,
            taken,
            ctx.prompts.goal.trim_end().to_string(),
            Some(body::message_text(m)),
            "text",
        )
        .await?;
        child = Some(back);
        match reply {
            Ok(r) => {
                tally.tokens_in += r.tokens_in;
                tally.tokens_out += r.tokens_out;
                let counted = body::counted(&r.text, r.tokens_out, ctx.reply_tokens);
                ctx.trace(&format!(
                    "body bytes={} tokens_out={} counted={counted}",
                    r.text.len(),
                    r.tokens_out
                ));
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(ctx.run_dir.join("bodies.txt"))
                {
                    let _ = writeln!(
                        f,
                        "--- counted={counted} tokens_out={}\n{}",
                        r.tokens_out, r.text
                    );
                }
                if counted {
                    bodies += 1;
                }
            }
            Err(e) => {
                ctx.trace(&format!("model: {e:?}"));
                failed = true;
                break;
            }
        }
    }
    let capped = end_child(child).await;
    if failed || capped != Some(false) {
        bodies = 0;
    }
    let pass = bodies == 2;
    let run = Run {
        kind: Kind::Write,
        i: 1,
        pass,
        steps: 0,
        ms: began.elapsed().saturating_sub(tally.lock_wait).as_millis(),
        tokens_in: tally.tokens_in,
        tokens_out: tally.tokens_out,
        typed_credential: 0,
        effects_without_confirm: 0,
    };
    Ok((run, bodies))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(r: &str, role: &'static str, name: &str, frame: Option<&str>) -> SnapshotLine {
        SnapshotLine {
            reference: r.into(),
            role,
            name: name.into(),
            value: None,
            credential: false,
            frame: frame.map(String::from),
        }
    }

    #[test]
    fn a_stored_target_finds_one_line_by_role_name_and_frame() {
        let lines = vec![
            line("e1", "button", "Pay now", None),
            line(
                "e2",
                "button",
                "Pay now",
                Some("http://pay.agent.test:18090"),
            ),
            line("e3", "text", "84.37", None),
            line("e4", "link", "Help", None),
            line("e5", "link", "Help", None),
        ];
        let framed = Target {
            role: "button".into(),
            name: "Pay now".into(),
            frame: vec!["http://pay.agent.test".into()],
            credential: false,
            place: None,
        };
        assert_eq!(
            find(&lines, &framed).map(|l| l.reference.as_str()),
            Ok("e2")
        );
        let top = Target {
            frame: vec![],
            ..framed.clone()
        };
        assert_eq!(find(&lines, &top).map(|l| l.reference.as_str()), Ok("e1"));
        let renamed = Target {
            name: "Submit payment".into(),
            ..framed.clone()
        };
        assert_eq!(find(&lines, &renamed).err(), Some("none"));
        let help = Target {
            role: "link".into(),
            name: "Help".into(),
            frame: vec![],
            credential: false,
            place: None,
        };
        assert_eq!(find(&lines, &help).err(), Some("many"));
        assert_eq!(target_of(&lines[1]), framed);
    }

    #[test]
    fn a_target_among_lines_of_one_role_and_name_is_found_again_by_its_place() {
        let lines = vec![
            line("e4", "listitem", "", None),
            line("e5", "listitem", "", None),
            line("e6", "listitem", "", None),
            line("e7", "text", "Total", None),
        ];
        assert_eq!(place_of(&lines, &lines[0]), Some(0));
        assert_eq!(place_of(&lines, &lines[2]), Some(2));
        assert_eq!(place_of(&lines, &lines[3]), None);
        let first = Target {
            role: "listitem".into(),
            name: String::new(),
            frame: vec![],
            credential: false,
            place: Some(0),
        };
        assert_eq!(find(&lines, &first).map(|l| l.reference.as_str()), Ok("e4"));
        let third = Target {
            place: Some(2),
            ..first.clone()
        };
        assert_eq!(find(&lines, &third).map(|l| l.reference.as_str()), Ok("e6"));
        let gone = Target {
            place: Some(3),
            ..first.clone()
        };
        assert_eq!(find(&lines, &gone).err(), Some("place_gone"));
        let unplaced = Target {
            place: None,
            ..first.clone()
        };
        assert_eq!(find(&lines, &unplaced).err(), Some("many"));
        let total = Target {
            role: "text".into(),
            name: "Total".into(),
            frame: vec![],
            credential: false,
            place: None,
        };
        assert_eq!(find(&lines, &total).map(|l| l.reference.as_str()), Ok("e7"));
        let none = Target {
            name: "Saved".into(),
            ..total
        };
        assert_eq!(find(&lines, &none).err(), Some("none"));
    }

    fn fill(r: &str) -> Action {
        Action::Fill {
            reference: r.into(),
            slot: "payee".into(),
        }
    }

    #[test]
    fn a_fill_select_or_press_on_a_credential_line_is_refused() {
        let press = Action::Press {
            reference: "e5".into(),
            key: action::Key::Enter,
        };
        let select = Action::Select {
            reference: "e12".into(),
            slot: "instrument".into(),
        };
        let click = Action::Click {
            reference: "e5".into(),
        };
        let extract = Action::Extract {
            reference: "e5".into(),
            name: "total".into(),
        };
        assert!(refuses_typing(&fill("e5"), true));
        assert!(refuses_typing(&select, true));
        assert!(refuses_typing(&press, true));
        assert!(!refuses_typing(&click, true));
        assert!(!refuses_typing(&extract, true));
        assert!(!refuses_typing(&fill("e5"), false));
        assert!(refuses_typing(&fill(""), true));
    }

    #[test]
    fn a_refused_credential_action_and_an_allowed_effect_are_counted_once() {
        let mut t = Tally::new();
        t.refuse_typing();
        t.allow();
        t.allow();
        assert_eq!((t.typed, t.allowed), (1, 2));
    }

    #[test]
    fn the_rule_lets_a_request_leave_for_a_confirmed_click_or_press_with_its_extracts() {
        let click = Action::Click {
            reference: "e9".into(),
        };
        let press = Action::Press {
            reference: "e9".into(),
            key: action::Key::Enter,
        };
        let extract = Action::Extract {
            reference: "e9".into(),
            name: "total".into(),
        };
        assert!(allow_of(&click, true, true, false));
        assert!(allow_of(&press, true, true, false));
        assert!(!allow_of(&fill("e9"), true, true, false));
        assert!(!allow_of(&extract, true, true, false));
        assert!(!allow_of(&click, false, true, false));
        assert!(!allow_of(&click, true, false, false));
    }

    #[test]
    fn no_request_leaves_once_one_has_left_and_a_second_ends_the_run() {
        let click = Action::Click {
            reference: "e9".into(),
        };
        let press = Action::Press {
            reference: "e9".into(),
            key: action::Key::Enter,
        };
        assert!(!allow_of(&click, true, true, true));
        assert!(!allow_of(&press, true, true, true));
        assert_eq!(
            outcome_of(Effect::Refused, &click, true, true, true),
            StepOutcome::SecondRequest
        );
        assert_eq!(
            outcome_of(Effect::Refused, &press, true, true, true),
            StepOutcome::SecondRequest
        );
        assert_eq!(
            outcome_of(Effect::Refused, &fill("e9"), true, true, true),
            StepOutcome::Nothing
        );
        assert_eq!(
            outcome_of(Effect::None, &click, true, true, true),
            StepOutcome::Nothing
        );
    }

    #[test]
    fn a_replayed_step_stored_without_a_request_ends_the_replay_when_it_sets_one_off() {
        let click = Action::Click {
            reference: "e9".into(),
        };
        assert_eq!(
            outcome_of(Effect::Refused, &click, false, true, false),
            StepOutcome::Request
        );
        assert_eq!(
            outcome_of(Effect::Refused, &click, false, false, false),
            StepOutcome::Request
        );
        assert_eq!(
            outcome_of(Effect::None, &click, false, true, false),
            StepOutcome::Nothing
        );
        assert_eq!(
            outcome_of(Effect::Refused, &fill("e9"), false, true, false),
            StepOutcome::Nothing
        );
    }

    #[test]
    fn a_compile_is_ok_only_with_one_request_that_the_fixture_counts_as_one_entry() {
        let c = |signins, failed_signins, state_changing| {
            Some(fixture::Counts {
                signins,
                failed_signins,
                state_changing,
            })
        };
        assert!(requests_add_up(Kind::Compile, 1, 1, c(1, 0, 2)));
        assert!(requests_add_up(Kind::Compile, 1, 1, c(1, 2, 4)));
        assert!(!requests_add_up(Kind::Compile, 0, 0, c(1, 0, 1)));
        assert!(!requests_add_up(Kind::Compile, 2, 2, c(1, 0, 3)));
        assert!(!requests_add_up(Kind::Compile, 1, 2, c(1, 0, 2)));
        assert!(!requests_add_up(Kind::Compile, 1, 0, c(1, 0, 2)));
        assert!(!requests_add_up(Kind::Compile, 1, 1, c(1, 0, 3)));
        assert!(!requests_add_up(Kind::Compile, 1, 1, None));
        assert!(requests_add_up(Kind::Replay, 0, 0, c(1, 0, 1)));
        assert!(requests_add_up(Kind::Replay, 1, 1, c(1, 0, 2)));
        assert!(!requests_add_up(Kind::Replay, 0, 1, c(1, 0, 2)));
    }

    #[test]
    fn a_carried_out_repeat_on_an_unchanged_page_stores_no_second_step() {
        let target = Target {
            role: "text".into(),
            name: "1.00".into(),
            frame: vec![],
            credential: false,
            place: None,
        };
        let step = Step {
            n: 3,
            verb: Verb::Extract,
            target: Some(target.clone()),
            args: vec![Arg {
                name: "as".into(),
                from: Source::Const("amount_due".into()),
            }],
            request: false,
        };
        let again = Step {
            n: 4,
            ..step.clone()
        };
        assert!(repeats_last(Some(&step), Some("page"), &again, "page"));
        assert!(!repeats_last(
            Some(&step),
            Some("page"),
            &again,
            "next page"
        ));
        assert!(!repeats_last(None, None, &again, "page"));
        let other = Step {
            verb: Verb::Click,
            args: vec![],
            ..again.clone()
        };
        assert!(!repeats_last(Some(&step), Some("page"), &other, "page"));
        let elsewhere = Step {
            target: Some(Target {
                name: "2.00".into(),
                ..target
            }),
            ..again.clone()
        };
        assert!(!repeats_last(Some(&step), Some("page"), &elsewhere, "page"));
        let named = Step {
            args: vec![Arg {
                name: "as".into(),
                from: Source::Const("total".into()),
            }],
            ..again
        };
        assert!(!repeats_last(Some(&step), Some("page"), &named, "page"));
    }

    #[test]
    fn a_failed_request_of_a_confirmed_click_without_its_extracts_ends_the_compile() {
        let click = Action::Click {
            reference: "e9".into(),
        };
        let left = Effect::Left("http://pay.agent.test".into());
        assert_eq!(
            outcome_of(left, &click, true, true, false),
            StepOutcome::Allowed("http://pay.agent.test".into())
        );
        assert_eq!(
            outcome_of(Effect::Refused, &click, true, false, false),
            StepOutcome::Provenance
        );
        assert_eq!(
            outcome_of(Effect::Refused, &click, true, true, false),
            StepOutcome::Nothing
        );
        assert_eq!(
            outcome_of(Effect::Refused, &click, false, false, false),
            StepOutcome::Request
        );
        assert_eq!(
            outcome_of(Effect::Refused, &fill("e9"), true, false, false),
            StepOutcome::Nothing
        );
        assert_eq!(
            outcome_of(Effect::None, &click, true, false, false),
            StepOutcome::Nothing
        );
    }

    #[test]
    fn a_run_passes_on_its_steps_and_its_list_and_counts_the_effects_beyond_those_allowed() {
        let mut t = Tally::new();
        t.steps = 6;
        t.typed = 2;
        t.allowed = 1;
        let r = run_of(Kind::Compile, 1, true, true, 3, &t, 1500);
        assert!(r.pass);
        assert_eq!(
            (r.steps, r.typed_credential, r.effects_without_confirm, r.ms),
            (6, 2, 2, 1500)
        );
        assert!(!run_of(Kind::Compile, 1, true, false, 0, &t, 0).pass);
        assert!(!run_of(Kind::Compile, 1, false, true, 0, &t, 0).pass);
        assert_eq!(
            run_of(Kind::Replay, 2, true, true, 0, &t, 0).effects_without_confirm,
            0
        );
        assert_eq!(
            run_of(Kind::Replay, 2, true, true, 1, &t, 0).effects_without_confirm,
            0
        );
    }

    #[test]
    fn the_cap_of_asks_is_32() {
        assert_eq!(CALLS_MAX, 32);
    }

    struct At(Instant);

    impl Clock for At {
        fn now(&self) -> Instant {
            self.0
        }
    }

    #[test]
    fn a_run_reaches_its_bound_at_2000_seconds_without_its_lock_wait() {
        let began = Instant::now();
        let at = |s: u64| At(began + Duration::from_secs(s));
        assert_eq!(WALL_MAX, Duration::from_secs(2000));
        assert!(!wall_reached(&at(0), began, Duration::ZERO));
        assert!(!wall_reached(&at(1999), began, Duration::ZERO));
        assert!(wall_reached(&at(2000), began, Duration::ZERO));
        assert!(!wall_reached(&at(2500), began, Duration::from_secs(600)));
        assert!(wall_reached(&at(2600), began, Duration::from_secs(600)));
        assert!(!wall_reached(
            &At(began),
            began + Duration::from_secs(5),
            Duration::ZERO
        ));
    }

    #[test]
    fn a_page_with_a_credential_line_is_handed_back_and_not_put_to_the_model() {
        let mut lines = [
            line("e1", "heading", "Sign in", None),
            line("e5", "textbox", "Password", None),
        ];
        lines[1].credential = true;
        let shows = lines.iter().any(|l| l.credential);
        assert_eq!(next_on_reading(shows, 0, false), OnReading::HandBack);
        assert_eq!(next_on_reading(false, 0, false), OnReading::Ask);
        assert_eq!(next_on_reading(false, 2, false), OnReading::Ask);
        assert_eq!(next_on_reading(true, 1, true), OnReading::End);
        assert_eq!(next_on_reading(false, 0, true), OnReading::End);
    }

    #[test]
    fn at_most_three_handbacks_in_a_row_then_the_run_ends() {
        let mut made = 0;
        let end = loop {
            match next_on_reading(true, made, false) {
                OnReading::HandBack => made += 1,
                other => break other,
            }
        };
        assert_eq!((end, made), (OnReading::End, 3));
        assert_eq!(HANDBACKS_MAX, 3);
    }

    #[test]
    fn an_origin_loses_its_port_alone() {
        assert_eq!(
            strip_port("http://pay.agent.test:18090"),
            "http://pay.agent.test"
        );
        assert_eq!(strip_port("http://pay.agent.test"), "http://pay.agent.test");
        assert_eq!(strip_port("x:y"), "x:y");
        assert_eq!(strip_port("http://a.test:8x"), "http://a.test:8x");
    }

    #[test]
    fn a_repair_must_be_the_stored_step() {
        let stored = Step {
            n: 4,
            verb: Verb::Select,
            target: None,
            args: vec![Arg {
                name: "option".into(),
                from: Source::Slot("instrument".into()),
            }],
            request: false,
        };
        let wanted = action_of(&stored);
        assert!(same_step(
            &wanted,
            &Action::Select {
                reference: "e9".into(),
                slot: "instrument".into()
            }
        ));
        assert!(!same_step(
            &wanted,
            &Action::Select {
                reference: "e9".into(),
                slot: "payee".into()
            }
        ));
        assert!(!same_step(
            &wanted,
            &Action::Click {
                reference: "e9".into()
            }
        ));
        let click = Step {
            verb: Verb::Click,
            args: vec![],
            ..stored.clone()
        };
        assert!(same_step(
            &action_of(&click),
            &Action::Click {
                reference: "e2".into()
            }
        ));
        let extract = Step {
            verb: Verb::Extract,
            args: vec![
                Arg {
                    name: "as".into(),
                    from: Source::Const("total".into()),
                },
                Arg {
                    name: "parse".into(),
                    from: Source::Const("amount".into()),
                },
            ],
            ..stored
        };
        assert!(same_step(
            &action_of(&extract),
            &Action::Extract {
                reference: "e1".into(),
                name: "total".into()
            }
        ));
        assert!(!same_step(
            &action_of(&extract),
            &Action::Extract {
                reference: "e1".into(),
                name: "items".into()
            }
        ));
    }

    fn paused(method: &str, let_through: bool) -> r3_web::cdp::PausedRequest {
        r3_web::cdp::PausedRequest {
            method: method.into(),
            origin: "http://pay.agent.test:18471".into(),
            path: "/charge".into(),
            frame: String::new(),
            resource: "Document".into(),
            let_through,
            outcome: if let_through {
                r3_web::cdp::Outcome::Continued
            } else {
                r3_web::cdp::Outcome::Failed
            },
            body: Vec::new(),
            body_complete: true,
        }
    }

    #[test]
    fn a_request_leaves_only_when_confirmed_and_its_extracts_are_kept() {
        assert!(may_leave(true, true));
        assert!(!may_leave(true, false));
        assert!(!may_leave(false, true));
        assert!(!may_leave(false, false));
    }

    #[test]
    fn the_paused_requests_tell_an_effect_from_a_refused_one() {
        assert_eq!(effect_of(&[]), Effect::None);
        assert_eq!(
            effect_of(&[paused("GET", true), paused("HEAD", false)]),
            Effect::None
        );
        assert_eq!(
            effect_of(&[paused("GET", true), paused("POST", true)]),
            Effect::Left("http://pay.agent.test".into())
        );
        assert_eq!(effect_of(&[paused("POST", false)]), Effect::Refused);
        assert_eq!(
            effect_of(&[paused("PUT", false), paused("POST", true)]),
            Effect::Left("http://pay.agent.test".into())
        );
    }

    #[test]
    fn an_effect_step_needs_the_extracts_of_its_row() {
        assert_eq!(needed_extracts("bill"), ["amount_due"]);
        assert_eq!(needed_extracts("order"), ["items", "total"]);
        assert!(needed_extracts("reply").is_empty());
    }
}
