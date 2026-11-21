//! `agent-gate task` (contracts.md A14.5): plays one task from start to end through the gate host and prints one line:
//!
//! `task name=<task> mode=<mode> seed=<n> store=memory steps=<n> prompts=<n> handbacks=<n> confirmations=<n>
//! effects=<n> unknown=<n> model_calls=<n> marks=<start,end|miss> fixture=<ok|miss> result=<completed|outcome>`.
//!
//! This command builds `task fill --mode replay --seed N`, with `--link host` or with no `--link`, and with
//! `--browser PATH`, which it reads and does not use: `fill` opens no browser. Every other form of A14.5 (the tasks
//! `bill`, `order`, `reply` and `cleanup`, the modes `compile` and `recover`, `--model`, `--variant`, `--second` and
//! `--link product`) is refused as `usage` before anything is made, and so before a runner starts.
//!
//! The play (A1.2): the command makes a directory of its own under the system's temporary directory
//! (`std::env::temp_dir`), copies the corpus workbook `tools/r3-trials/sheet/corpus/formulas.xlsx` there as the
//! workbook, writes the five rows of the seed's `sheet` (`tools/fixture-server/variants/data/seed-<n>.json`) there
//! as `source.csv`, and makes the data directory `data` beside them; the directory stays after the command. On the
//! gate host (A7.6) it puts an approved workflow of the category `spreadsheet_fill`, sends `create_agent`, binds the
//! slots `workbook`, `source`, `sheet` (`Ledger`) and `range` (`B2:D6`) with `set_bindings` (`seed::bind`), moves
//! the agent to `active` with `set_state` and sends `start_run`. Then it starts the runner, the binary
//! `halvo-runner` beside its own, through the supervisor library's `Supervisor::start` with that data directory, the
//! gate host as the owner behind the link, which answers each `store` frame and each body of the library with the
//! store's handlers, and the stand-in (`crate::standin`) as the person.
//!
//! After the run's end the counts and the result come from the run's record in the gate host's store. `marks` is
//! `start,end` when the library called the gate's `MarkSink` with the run's start and then its end. `fixture` is
//! `ok` when the range did not hold the seed's rows before the run, holds them after it, as the sheet executor reads
//! the workbook and the source, and the run's checkpoint stands beside the workbook: the cells change only by a
//! `replace`, which renames only a copy that passed the check of A11.3.
//!
//! Refusals: `usage` for a form that is not built, and for a play that cannot run (no runner beside `agent-gate`, a
//! body of the set-up that the host refuses, a start that the library answers with an error); `fixture` for a seed
//! that cannot be read or a fixture file that cannot be made.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use halvo_agent_client::{
    Config, LinkError, MarkSink, OwnerLink, Parts, RunEnd, RunSpec, Secrets, Supervisor,
};
use halvo_schema::executable::body::{CreateAgent, SetState, StartRun};
use halvo_schema::executable::frame::{ConfirmRequest, SecretReply};
use halvo_schema::executable::names::{AgentState, Answer, Mode};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::{
    AgentRun, CoordinatorCall, EffectId, Name, Outcome, OwnValue, RunId, RunnerCall,
};
use halvo_schema::request::{decode, write_reply, CoordinatorRole, Decoded, ReplyResult};
use halvo_schema::{AgentId, ErrorName, RowRead, SeedCategory, WorkflowId};
use halvo_sheet::{Cell, Folder, Locks, Range, Value};
use serde_json::value::RawValue;

use crate::host::{minted, GateHost};
use crate::seed::{bind, Binding, StopCause};
use crate::standin::{self, Standin};
use crate::STORE;

/// The form that this command plays.
pub const USAGE: &str =
    "agent-gate task fill --mode replay --seed N [--link host] [--browser PATH]";

const TASKS: [&str; 5] = ["bill", "reply", "order", "cleanup", "fill"];
const MODES: [&str; 3] = ["replay", "compile", "recover"];
const LINKS: [&str; 2] = ["host", "product"];

const CORPUS: &str = "tools/r3-trials/sheet/corpus/formulas.xlsx";
const SEEDS: &str = "tools/fixture-server/variants/data";
const WORKBOOK: &str = "formulas.xlsx";
const SOURCE: &str = "source.csv";
const DATA: &str = "data";
const SHEET: &str = "Ledger";
const RANGE: &str = "B2:D6";

/// How long a read of the workbook waits for the lock of its directory, the wait of the runner's sheet executor.
const LOCK_WAIT: Duration = Duration::from_secs(10);

/// The cause of a refusal of `task`, which the command prints as `REFUSED <cause>: <detail>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Usage,
    Fixture,
}

impl Cause {
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Usage => "usage",
            Cause::Fixture => "fixture",
        }
    }
}

/// A refusal of `task`: its cause and one line that says why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub cause: Cause,
    pub detail: String,
}

fn usage(detail: impl Into<String>) -> Refused {
    Refused {
        cause: Cause::Usage,
        detail: detail.into(),
    }
}

fn fixture(detail: impl Into<String>) -> Refused {
    Refused {
        cause: Cause::Fixture,
        detail: detail.into(),
    }
}

/// The play that this command builds: `fill` in the mode `replay` with the seed `seed`, on the gate host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub seed: u32,
}

/// A form of `task` as given, each flag at most once.
#[derive(Debug, Default)]
struct Form {
    task: String,
    mode: Option<String>,
    seed: Option<u32>,
    variants: Vec<String>,
    model: Option<OsString>,
    second: bool,
    link: Option<String>,
    browser: bool,
}

/// Reads the arguments after `task`, refusing as `usage` a form that A14.5 does not have and then a form that this
/// command does not build.
pub fn parse(args: &[OsString]) -> Result<Fill, Refused> {
    let form = read(args)?;
    built(&form)?;
    let seed = form
        .seed
        .ok_or_else(|| usage(format!("no --seed: {USAGE}")))?;
    Ok(Fill { seed })
}

fn read(args: &[OsString]) -> Result<Form, Refused> {
    let mut args = args.iter();
    let mut form = Form {
        task: one_of(args.next(), &TASKS, "a task")?,
        ..Form::default()
    };
    while let Some(flag) = args.next() {
        let flag = flag.to_str().unwrap_or("");
        let twice = || usage(format!("{flag} twice"));
        match flag {
            "--mode" if form.mode.is_none() => {
                form.mode = Some(one_of(args.next(), &MODES, "a mode")?);
            }
            "--seed" if form.seed.is_none() => form.seed = Some(seed(args.next())?),
            "--variant" => form.variants.push(text(args.next(), flag)?),
            "--model" if form.model.is_none() => form.model = Some(value(args.next(), flag)?),
            "--second" if !form.second => form.second = true,
            "--link" if form.link.is_none() => {
                form.link = Some(one_of(args.next(), &LINKS, "a link")?);
            }
            "--browser" if !form.browser => {
                value(args.next(), flag)?;
                form.browser = true;
            }
            "--mode" | "--seed" | "--model" | "--second" | "--link" | "--browser" => {
                return Err(twice())
            }
            _ => return Err(usage(format!("no flag {flag:?}: {USAGE}"))),
        }
    }
    if form.mode.is_none() {
        return Err(usage(format!("no --mode: {USAGE}")));
    }
    Ok(form)
}

/// The one rule of what this command builds: the task `fill` in the mode `replay` over the host link, with no
/// `--variant`, `--model` or `--second`.
fn built(form: &Form) -> Result<(), Refused> {
    let not_yet = |what: String| Err(usage(format!("{what} is not built yet: {USAGE}")));
    if form.task != "fill" {
        return not_yet(format!("the task {}", form.task));
    }
    if form.mode.as_deref() != Some("replay") {
        return not_yet(format!("the mode {}", form.mode.as_deref().unwrap_or("")));
    }
    if !form.variants.is_empty() {
        return not_yet("--variant".into());
    }
    if form.model.is_some() {
        return not_yet("--model".into());
    }
    if form.second {
        return not_yet("--second".into());
    }
    if form.link.as_deref().is_some_and(|link| link != "host") {
        return not_yet("--link product".into());
    }
    Ok(())
}

fn value(arg: Option<&OsString>, flag: &str) -> Result<OsString, Refused> {
    arg.cloned()
        .ok_or_else(|| usage(format!("{flag} takes a value")))
}

fn text(arg: Option<&OsString>, flag: &str) -> Result<String, Refused> {
    value(arg, flag)?
        .into_string()
        .map_err(|_| usage(format!("the value of {flag} is not UTF-8")))
}

fn one_of(arg: Option<&OsString>, names: &[&str], what: &str) -> Result<String, Refused> {
    match arg.and_then(|a| a.to_str()) {
        Some(name) if names.contains(&name) => Ok(name.to_owned()),
        Some(name) => Err(usage(format!("{name:?} is not {what}: {USAGE}"))),
        None => Err(usage(format!("{what} is missing: {USAGE}"))),
    }
}

fn seed(arg: Option<&OsString>) -> Result<u32, Refused> {
    let digits = text(arg, "--seed")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(usage(format!("--seed takes a number, not {digits:?}")));
    }
    digits
        .parse()
        .map_err(|_| usage(format!("--seed {digits} is too large")))
}

/// The line of one play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub seed: u32,
    /// The run's record as the gate host's store holds it.
    pub record: AgentRun,
    /// Whether the library called the gate's `MarkSink` with the run's start and then its end.
    pub marks: bool,
    /// Whether the workbook reads as A1.2 says.
    pub fixture: bool,
}

impl Line {
    /// The line of A14.5 with its newline.
    pub fn render(&self) -> String {
        let r = &self.record;
        format!(
            "task name=fill mode=replay seed={} store={STORE} steps={} prompts={} handbacks={} confirmations={} \
             effects={} unknown={} model_calls={} marks={} fixture={} result={}\n",
            self.seed,
            r.steps,
            r.prompts,
            r.handbacks,
            r.confirmations,
            r.effects,
            r.unknown,
            r.model_calls,
            if self.marks { "start,end" } else { "miss" },
            if self.fixture { "ok" } else { "miss" },
            r.outcome.as_str(),
        )
    }

    /// Whether the play passed: the marks, the fixture and the result `completed`.
    pub fn pass(&self) -> bool {
        self.marks && self.fixture && self.record.outcome == Outcome::Completed
    }
}

/// What the link, the stand-in and the gate's `MarkSink` hand the command's thread, which holds the gate host,
/// while a run goes: the gate host is no `Send` value, so the library's threads reach it through this channel.
pub(crate) enum Event {
    /// A request on the owner link, with the channel of its reply.
    Request(Vec<u8>, Sender<Result<Vec<u8>, LinkError>>),
    /// A confirmation of the run, with the channel of the stand-in's answers.
    Confirm(RunId, ConfirmRequest, Sender<Vec<(EffectId, Answer)>>),
    /// A handback of the run.
    Handback(RunId),
    /// The run's end.
    End,
}

/// Plays `fill` from start to end.
pub fn play(fill: &Fill) -> Result<Line, Refused> {
    let rows = seed_rows(fill.seed)?;
    let runner = runner_beside()?;
    let dir = make_dir()?;
    let workbook = dir.join(WORKBOOK);
    let source = dir.join(SOURCE);
    let data = dir.join(DATA);
    fs::copy(repo().join(CORPUS), &workbook).map_err(|_| {
        fixture("the corpus workbook cannot be copied into the command's directory")
    })?;
    fs::write(&source, rows)
        .map_err(|_| fixture("source.csv cannot be written in the command's directory"))?;
    fs::create_dir(&data)
        .map_err(|_| fixture("the data directory cannot be made in the command's directory"))?;
    let before = holds_rows(&dir, &data, &workbook, &source);

    let mut host = GateHost::new();
    let run = RunId::from_random(minted());
    let slots = slots(&workbook, &source)?;
    let (workflow, agent) = set_up(&mut host, slots, &run).map_err(usage)?;
    let (events, ended) = mpsc::channel();
    let standin = Arc::new(Standin::new(events.clone()));
    let marks = Arc::new(Marks {
        seen: Mutex::new(Vec::new()),
        events: events.clone(),
    });
    let parts = Parts {
        link: Arc::new(HostLink(events)),
        confirmer: standin.clone(),
        human: standin,
        secrets: Arc::new(NoSecrets),
        marks: marks.clone(),
    };
    let supervisor = Supervisor::new(
        CoordinatorRole::claim(),
        Config::new(data.clone(), runner),
        parts,
    );
    supervisor
        .start(RunSpec {
            run: run.clone(),
            agent,
            workflow,
            mode: Mode::Replay,
            // `fill` opens no browser, and the runner checks no more than the form of its `--browser`.
            browser: dir.join("no-browser"),
            model: None,
            headed: false,
            utc_offset_s: 0,
        })
        .map_err(|e| {
            usage(format!(
                "the supervisor library answered the start with {e}"
            ))
        })?;
    loop {
        match ended.recv() {
            Ok(Event::Request(request, reply)) => {
                let _ = reply.send(serve(&mut host, &request));
            }
            Ok(Event::Confirm(of, request, reply)) => {
                let _ = reply.send(standin::answers(host.rows(), &of, &request));
            }
            Ok(Event::Handback(of)) => {
                // `answer_handback` waits for the library's driver, which may wait for this thread: answer from
                // a thread of its own.
                let supervisor = supervisor.clone();
                thread::spawn(move || supervisor.answer_handback(&of, standin::unscripted()));
            }
            Ok(Event::End) => break,
            Err(_) => return Err(usage("the run ended with no word of its end")),
        }
    }
    drop(ended);

    let record = record_of(host.rows(), &run)
        .ok_or_else(|| usage("the gate host's store holds no record of the run"))?;
    let checkpoint = halvo_checkpoint::RunId::parse(run.as_str())
        .and_then(|run| halvo_checkpoint::checkpoint_name(&workbook, &run))
        .is_some_and(|name| dir.join(name).is_file());
    let after = holds_rows(&dir, &data, &workbook, &source);
    let marked = *lock(&marks.seen) == ["start", "end"];
    Ok(Line {
        seed: fill.seed,
        record,
        marks: marked,
        fixture: !before && after && checkpoint,
    })
}

/// The root of the repository, two directories above this crate.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The text of `source.csv`: the rows of the seed's `sheet`, one per line, three cells separated by commas, no header
/// and no quoting (A1.2).
fn seed_rows(seed: u32) -> Result<String, Refused> {
    let bytes = fs::read(repo().join(SEEDS).join(format!("seed-{seed}.json")))
        .map_err(|_| fixture(format!("the seed {seed} cannot be read")))?;
    let unread = || {
        fixture(format!(
            "the seed {seed} has no rows of sheet that A1.2 can write"
        ))
    };
    let seed: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| unread())?;
    let rows = seed
        .get("sheet")
        .and_then(|sheet| sheet.get("rows"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(unread)?;
    let mut text = String::new();
    for row in rows {
        let cells: Vec<&str> = row
            .as_array()
            .and_then(|cells| cells.iter().map(serde_json::Value::as_str).collect())
            .ok_or_else(unread)?;
        if cells.iter().any(|cell| cell.contains([',', '\n', '\r'])) {
            return Err(unread());
        }
        text.push_str(&cells.join(","));
        text.push('\n');
    }
    Ok(text)
}

/// The runner's binary in the directory of `agent-gate`'s own.
fn runner_beside() -> Result<PathBuf, Refused> {
    let name = format!("halvo-runner{}", std::env::consts::EXE_SUFFIX);
    let missing = || {
        usage(format!(
            "{name} does not stand beside agent-gate: build it with cargo build -p halvo-runner"
        ))
    };
    let runner = std::env::current_exe()
        .map_err(|_| missing())?
        .with_file_name(&name);
    if runner.is_file() {
        Ok(runner)
    } else {
        Err(missing())
    }
}

/// A new directory of the command's own under the system's temporary directory.
fn make_dir() -> Result<PathBuf, Refused> {
    let base = std::env::temp_dir();
    for n in 0u32..1_000 {
        let path = base.join(format!("agent-gate-task-{}-{n}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(_) => break,
        }
    }
    Err(fixture("the command's directory cannot be made"))
}

fn slots(workbook: &Path, source: &Path) -> Result<Vec<Slot>, Refused> {
    let path = |p: &Path| {
        p.to_str()
            .map(str::to_owned)
            .ok_or_else(|| fixture("the command's directory is no UTF-8 path"))
    };
    [
        ("workbook", path(workbook)?),
        ("source", path(source)?),
        ("sheet", SHEET.to_owned()),
        ("range", RANGE.to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| {
        Ok(Slot {
            name: Name::parse(name).map_err(|_| usage(format!("{name} is no slot name")))?,
            value: OwnValue::new(&value),
        })
    })
    .collect()
}

/// The set-up of a run on the gate host: an approved workflow of `spreadsheet_fill`, its agent by `create_agent`,
/// the slots by `set_bindings`, the agent `active` by `set_state`, and `start_run` in the mode `replay`. It stops at
/// the first body that the host does not answer with the form of its call.
fn set_up(
    host: &mut GateHost,
    slots: Vec<Slot>,
    run: &RunId,
) -> Result<(WorkflowId, AgentId), String> {
    let workflow = host.put_seed_workflow(SeedCategory::SpreadsheetFill);
    let agent = AgentId::from_random(minted());
    let send = |host: &mut GateHost, call: CoordinatorCall| match host.send(&call) {
        Ok(reply) if reply.answers(&call) => Ok(()),
        Ok(_) => Err(other_form(call.op())),
        Err(error) => Err(refused_body(call.op(), &error)),
    };
    send(
        host,
        CoordinatorCall::CreateAgent(CreateAgent {
            workflow: workflow.clone(),
            agent: agent.clone(),
        }),
    )?;
    let binding = Binding {
        origins: Vec::new(),
        slots,
    };
    bind(host, &workflow, &agent, run, &binding).map_err(|stopped| match stopped.cause {
        StopCause::Refused(error) => refused_body(stopped.op, &error),
        StopCause::Reply => other_form(stopped.op),
    })?;
    send(
        host,
        CoordinatorCall::SetState(SetState {
            workflow: workflow.clone(),
            agent: agent.clone(),
            state: AgentState::Active,
        }),
    )?;
    send(
        host,
        CoordinatorCall::StartRun(StartRun {
            workflow: workflow.clone(),
            agent: agent.clone(),
            run: run.clone(),
            mode: Mode::Replay,
        }),
    )?;
    Ok((workflow, agent))
}

/// The detail of a set-up body that the gate host refused: the body's op and the handler's refusal, no value.
fn refused_body(op: &str, error: &ErrorName) -> String {
    format!(
        "the gate host refused the set-up body {op} with {}",
        error.as_str()
    )
}

/// The detail of a set-up body that the gate host answered with a reply of another form than its call's.
fn other_form(op: &str) -> String {
    format!("the gate host answered the set-up body {op} with another form")
}

/// Whether the range of the workbook holds the rows of the source, cell for cell, as the sheet executor reads both.
fn holds_rows(dir: &Path, data: &Path, workbook: &Path, source: &Path) -> bool {
    let Ok(range) = Range::parse(RANGE) else {
        return false;
    };
    let Some(folder) = Locks::open(data, LOCK_WAIT)
        .ok()
        .and_then(|locks| Folder::new(dir, locks).ok())
    else {
        return false;
    };
    let (Ok(cells), Ok(rows)) = (
        folder.read(workbook, SHEET, range),
        folder.read_source(source),
    ) else {
        return false;
    };
    cells.len() == rows.len()
        && cells.iter().zip(&rows).all(|(cells, row)| {
            cells.len() == row.len() && cells.iter().zip(row).all(|(c, v)| same(c, v))
        })
}

fn same(cell: &Cell, value: &Value) -> bool {
    match (cell, value) {
        (Cell::Number(a), Value::Number(b)) => a == b,
        (Cell::Text(a), Value::Text(b)) => a == b,
        _ => false,
    }
}

/// The record of `run` in the store, the run's `finish` or the record of its `end_run`.
fn record_of(rows: &dyn RowRead, run: &RunId) -> Option<AgentRun> {
    let mut found = None;
    rows.for_each("agent_run", &mut |row| {
        if let Ok(record) = AgentRun::parse(row.as_bytes()) {
            if &record.run == run {
                found = Some(record);
            }
        }
    });
    found
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The owner link to the gate host: each request goes to the command's thread, which answers it (`serve`).
struct HostLink(Sender<Event>);

impl OwnerLink for HostLink {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        let (tx, rx) = mpsc::channel();
        self.0
            .send(Event::Request(request.to_vec(), tx))
            .map_err(|_| LinkError)?;
        rx.recv().map_err(|_| LinkError)?
    }
}

/// The gate host as the owner: it answers a runner request and a coordinator write with the store's handlers, each
/// reply written by the one writer of a reply.
fn serve(host: &mut GateHost, request: &[u8]) -> Result<Vec<u8>, LinkError> {
    let reply = match decode(request) {
        Ok(Decoded::Raw(raw)) => {
            let answer = answer(host, raw.name(), raw.body());
            write_reply(Some(raw.id()), answer.as_ref().map_err(Clone::clone))
        }
        Ok(other) => write_reply(Some(other.id()), Err(ErrorName::Unsupported)),
        Err(e) => write_reply(e.id.as_ref(), Err(e.error)),
    };
    reply.map_err(|_| LinkError)
}

fn answer(host: &mut GateHost, name: &str, body: &RawValue) -> Result<ReplyResult, ErrorName> {
    let body = body.get().as_bytes();
    let json = match name {
        "runner" => {
            let call = RunnerCall::parse(body).map_err(|_| ErrorName::Malformed)?;
            host.runner(&call)?.to_json()
        }
        "coordinator_write" => {
            let call = CoordinatorCall::parse(body).map_err(|_| ErrorName::Malformed)?;
            host.send(&call)?.to_json()
        }
        _ => return Err(ErrorName::Unsupported),
    };
    let json = json.map_err(|_| ErrorName::Internal)?;
    RawValue::from_string(json)
        .map(ReplyResult::Raw)
        .map_err(|_| ErrorName::Internal)
}

/// The gate's `MarkSink`: what the library called for the run, in order, and the end handed to the command.
struct Marks {
    seen: Mutex<Vec<&'static str>>,
    events: Sender<Event>,
}

impl MarkSink for Marks {
    fn start(&self, _run: &RunId) {
        lock(&self.seen).push("start");
    }

    fn end(&self, _run: &RunId, _end: &RunEnd) {
        lock(&self.seen).push("end");
        let _ = self.events.send(Event::End);
    }
}

/// The gate host's secret store, which holds no secret in this command: `fill` asks for none.
struct NoSecrets;

impl Secrets for NoSecrets {
    fn read(&self, _agent: &AgentId, _name: &Name) -> SecretReply {
        SecretReply::None
    }
}
