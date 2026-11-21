//! `agent-gate task` (contracts.md A14.5): plays one task from start to end through the gate host, or through the
//! product's owner, and prints one line:
//!
//! `task name=<task> mode=<mode> seed=<n> store=<memory|owner> steps=<n> prompts=<n> handbacks=<n>
//! confirmations=<n> effects=<n> unknown=<n> model_calls=<n> marks=<start,end|miss> fixture=<ok|miss>
//! result=<completed|outcome>`.
//!
//! This command builds `task fill --mode replay --seed N`, with `--link host` or with no `--link`, or with
//! `--link product --workflow ID [--data-dir DIR]`, and with `--browser PATH`, which it reads and does not use: `fill`
//! opens no browser. Every other form of A14.5 (the tasks `bill`, `order`, `reply` and `cleanup`, the modes `compile`
//! and `recover`, `--model`, `--variant` and `--second`) is refused as `usage` before anything is made, and so before
//! a runner starts. `--workflow` and `--data-dir` stand with `--link product` alone.
//!
//! With `--link product` (unit U9.4) the command makes no gate host and no store. It reaches the product's owner
//! through U9.2's link: `StreamLink` over `ClientEnds` over the call `owner_stream` of a client of its own, on the
//! control channel of the data directory `DIR`, or of the product's own data directory without `--data-dir`. So the
//! command is the second lawful client of the stream beside the app, with an end of its own. The workflow `ID` is an
//! approved workflow of the category `spreadsheet_fill` that the owner's store holds: no request of the stream
//! makes or reads a workflow, since a stream admits the roles `runner` and `coordinator` alone. The client's connector
//! is `ctl_connector`: the control socket of the data directory on macOS, its named pipe on Windows.
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
//! Over the product link the same set-up goes to the owner on the command's end of the stream, as
//! `coordinator_write` requests, from `create_agent` on, and the runner's requests go there through the library.
//!
//! After the run's end the counts and the result come from the run's record: in the gate host's store, or, over the
//! product link, the record of the run's `finish` that the owner acknowledged, as the library hands it to the gate's
//! `MarkSink` with the run's end (the stream serves no read of an agent's history). `marks` is `start,end` when the
//! library called the gate's `MarkSink` with the run's start and then its end. `fixture` is `ok` when the range did
//! not hold the seed's rows before the run, holds them after it, as the sheet executor reads the workbook and the
//! source, and the run's checkpoint stands beside the workbook: the cells change only by a `replace`, which renames
//! only a copy that passed the check of A11.3.
//!
//! Refusals: `usage` for a form that is not built, and for a play that cannot run (no runner beside `agent-gate`, a
//! body of the set-up that the host or the owner refuses or that the product link leaves without a reply, a start
//! that the library answers with an error, a run whose `finish` the owner did not acknowledge); `fixture` for a seed
//! that cannot be read or a fixture file that cannot be made. The product link answers a refused `owner_stream` (an owner that
//! cannot be reached, `busy`, an unknown request) with no reply, so each of them prints the one detail of the first
//! body, `create_agent`, that got no reply.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use halvo_agent_client::{
    Config, LinkError, MarkSink, OwnerLink, Parts, RunEnd, RunSpec, Secrets, Supervisor,
};
use halvo_schema::executable::body::{
    CoordinatorReply, CreateAgent, SetBindings, SetState, StartRun,
};
use halvo_schema::executable::frame::{ConfirmRequest, SecretReply};
use halvo_schema::executable::names::{AgentState, Answer, Mode};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::{
    AgentRun, CoordinatorCall, EffectId, Name, Outcome, OwnValue, RunId, RunnerCall,
};
use halvo_schema::request::{
    decode, decode_reply, write_reply, CoordinatorRole, CoordinatorWrite, Decoded, ReplyResult,
};
use halvo_schema::{AgentId, ErrorName, RequestId, RowRead, SeedCategory, WorkflowId};
use halvo_sheet::{Cell, Folder, Locks, Range, Value};
use serde_json::value::RawValue;

use crate::host::{minted, GateHost};
use crate::seed::{bind, Binding, StopCause};
use crate::standin::{self, Standin};
use crate::STORE;

/// The form that this command plays.
pub const USAGE: &str = "agent-gate task fill --mode replay --seed N [--link host | --link product --workflow ID [--data-dir DIR]] [--browser PATH]";

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

/// The play that this command builds: `fill` in the mode `replay` with the seed `seed`, over `link`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fill {
    pub seed: u32,
    pub link: Link,
}

/// Where the play's store requests go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// The gate host on the memory store, in this process.
    Host,
    /// The product's owner through U9.2's link, on the control channel of `data_dir`, or of the product's own data
    /// directory where it is none; `workflow` is the approved workflow of the agent that the play makes.
    Product {
        workflow: WorkflowId,
        data_dir: Option<PathBuf>,
    },
}

impl Link {
    /// The store of the line, after `store=`.
    pub fn store(&self) -> &'static str {
        match self {
            Link::Host => STORE,
            Link::Product { .. } => "owner",
        }
    }
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
    workflow: Option<WorkflowId>,
    data_dir: Option<PathBuf>,
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
    let link = match form.link.as_deref() {
        Some("product") => Link::Product {
            workflow: form
                .workflow
                .ok_or_else(|| usage(format!("--link product takes --workflow: {USAGE}")))?,
            data_dir: form.data_dir,
        },
        _ => Link::Host,
    };
    Ok(Fill { seed, link })
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
            "--workflow" if form.workflow.is_none() => {
                let id = text(args.next(), flag)?;
                let workflow = WorkflowId::parse(&id)
                    .map_err(|_| usage(format!("--workflow takes a workflow id, not {id:?}")))?;
                form.workflow = Some(workflow);
            }
            "--data-dir" if form.data_dir.is_none() => {
                form.data_dir = Some(value(args.next(), flag)?.into());
            }
            "--browser" if !form.browser => {
                value(args.next(), flag)?;
                form.browser = true;
            }
            "--mode" | "--seed" | "--model" | "--second" | "--link" | "--workflow"
            | "--data-dir" | "--browser" => return Err(twice()),
            _ => return Err(usage(format!("no flag {flag:?}: {USAGE}"))),
        }
    }
    if form.mode.is_none() {
        return Err(usage(format!("no --mode: {USAGE}")));
    }
    Ok(form)
}

/// The one rule of what this command builds: the task `fill` in the mode `replay` over the host link or the product
/// link, with no `--variant`, `--model` or `--second`, and with `--workflow` and `--data-dir` over the product link
/// alone.
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
    if form.link.as_deref() != Some("product") {
        for (flag, given) in [
            ("--workflow", form.workflow.is_some()),
            ("--data-dir", form.data_dir.is_some()),
        ] {
            if given {
                return Err(usage(format!(
                    "{flag} stands with --link product alone: {USAGE}"
                )));
            }
        }
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
    /// The store behind the link: `memory` for the gate host, `owner` for the product's owner.
    pub store: &'static str,
    /// The run's record as the store behind the link holds it.
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
            "task name=fill mode=replay seed={} store={} steps={} prompts={} handbacks={} confirmations={} \
             effects={} unknown={} model_calls={} marks={} fixture={} result={}\n",
            self.seed,
            self.store,
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

    let run = RunId::from_random(minted());
    let slots = slots(&workbook, &source)?;
    let start = Start {
        runner,
        data: data.clone(),
        browser: dir.join("no-browser"),
        run: run.clone(),
    };
    let played = match &fill.link {
        Link::Host => on_host(start, slots)?,
        Link::Product { workflow, data_dir } => {
            on_owner(start, slots, workflow, data_dir.as_deref())?
        }
    };

    let checkpoint = halvo_checkpoint::RunId::parse(run.as_str())
        .and_then(|run| halvo_checkpoint::checkpoint_name(&workbook, &run))
        .is_some_and(|name| dir.join(name).is_file());
    let after = holds_rows(&dir, &data, &workbook, &source);
    Ok(Line {
        seed: fill.seed,
        store: fill.link.store(),
        record: played.record,
        marks: played.marks,
        fixture: !before && after && checkpoint,
    })
}

/// What the start of the runner takes beside the link: the runner's binary, the library's data directory, the
/// browser path that `fill` does not open, and the run.
struct Start {
    runner: PathBuf,
    data: PathBuf,
    /// `fill` opens no browser, and the runner checks no more than the form of its `--browser`.
    browser: PathBuf,
    run: RunId,
}

/// What one run left: its record, and whether the library called the gate's `MarkSink` with its start and then its
/// end.
struct Played {
    record: AgentRun,
    marks: bool,
}

/// The play on the gate host, in this process: the set-up on the host, the runner over the host link, and the record
/// from the host's store.
fn on_host(start: Start, slots: Vec<Slot>) -> Result<Played, Refused> {
    let mut host = GateHost::new();
    let (workflow, agent) = set_up(&mut host, slots, &start.run).map_err(usage)?;
    let (events, ended) = mpsc::channel();
    let link = Arc::new(HostLink(events.clone()));
    let run = start.run.clone();
    let (marks, _) = run_to_end(start, link, (workflow, agent), (events, ended), &mut host)?;
    let record = record_of(host.rows(), &run)
        .ok_or_else(|| usage("the gate host's store holds no record of the run"))?;
    Ok(Played { record, marks })
}

/// The play on the product's owner: no gate host and no store in this process. The set-up and the runner's requests
/// go over one `StreamLink`, whose one end the owner holds as one of its streams. The record is the run's `finish` as
/// the owner acknowledged it, which the library hands the gate's `MarkSink` with the run's end: the stream serves no
/// read of the agent's history.
fn on_owner(
    start: Start,
    slots: Vec<Slot>,
    workflow: &WorkflowId,
    data_dir: Option<&Path>,
) -> Result<Played, Refused> {
    let link: Arc<dyn OwnerLink> = Arc::new(product::link(data_dir)?);
    let agent = AgentId::from_random(minted());
    set_up_over(&*link, workflow, &agent, slots, &start.run)?;
    let (events, ended) = mpsc::channel();
    let (marks, finish) = run_to_end(
        start,
        link,
        (workflow.clone(), agent),
        (events, ended),
        &mut Owner,
    )?;
    let record = finish.ok_or_else(|| usage("the owner acknowledged no finish of the run"))?;
    Ok(Played { record, marks })
}

/// The product link of `--link product`: `StreamLink` over `ClientEnds` over the call `owner_stream` of a client of
/// the command's own, on the control channel of the data directory: its socket on macOS, its named pipe on Windows,
/// by `ctl_connector`, the connector of `ctl`.
#[cfg(any(target_os = "macos", windows))]
mod product {
    use std::path::Path;

    use halvo_agent_client::link::product::ClientEnds;
    use halvo_agent_client::link::{Ends, StreamLink};
    use halvo_control::client::{ctl_connector, Client};
    use halvo_control::paths::DataDir;

    use super::{usage, Refused};

    /// The link to the owner behind the control channel of `data_dir`, or of the product's own data directory.
    pub(super) fn link(data_dir: Option<&Path>) -> Result<StreamLink<impl Ends>, Refused> {
        let data_dir = match data_dir {
            Some(dir) => DataDir::from_flag(dir).map_err(|_| {
                usage("--data-dir takes an absolute UTF-8 path with no part \"..\"")
            })?,
            None => DataDir::for_current_user()
                .map_err(|_| usage("the product's data directory cannot be named"))?,
        };
        let client = Client::new(
            ctl_connector(&data_dir)
                .map_err(|_| usage("the control channel of the data directory cannot be named"))?,
        );
        Ok(StreamLink::new(ClientEnds::new(move || {
            client.owner_stream()
        })))
    }
}

/// The product link of `--link product` elsewhere: the request `owner_stream` is built on macOS and Windows alone.
#[cfg(not(any(target_os = "macos", windows)))]
mod product {
    use std::path::Path;

    use halvo_agent_client::link::{End, Ends, Refusal, StreamLink};

    use super::{usage, Refused};

    /// The ends of a system with no request `owner_stream`: never an end.
    pub(super) struct NoEnds;

    impl Ends for NoEnds {
        fn end(&self) -> Result<End, Refusal> {
            Err(Refusal::Internal)
        }
    }

    pub(super) fn link(_data_dir: Option<&Path>) -> Result<StreamLink<NoEnds>, Refused> {
        Err(usage("--link product is built on macOS and Windows alone"))
    }
}

/// Starts the runner through the supervisor library over `link`, and answers on this thread what the stand-in, the
/// gate's `MarkSink` and a host link hand it, until the run's end. Returns whether the marks are the run's start and
/// then its end, and the record of the run's acknowledged `finish` that the end carried.
fn run_to_end(
    start: Start,
    link: Arc<dyn OwnerLink>,
    (workflow, agent): (WorkflowId, AgentId),
    (events, ended): (Sender<Event>, Receiver<Event>),
    behind: &mut dyn Behind,
) -> Result<(bool, Option<AgentRun>), Refused> {
    let standin = Arc::new(Standin::new(events.clone()));
    let marks = Arc::new(Marks {
        seen: Mutex::new(Vec::new()),
        finish: Mutex::new(None),
        events,
    });
    let parts = Parts {
        link,
        confirmer: standin.clone(),
        human: standin,
        secrets: Arc::new(NoSecrets),
        marks: marks.clone(),
    };
    let supervisor = Supervisor::new(
        CoordinatorRole::claim(),
        Config::new(start.data, start.runner),
        parts,
    );
    supervisor
        .start(RunSpec {
            run: start.run,
            agent,
            workflow,
            mode: Mode::Replay,
            browser: start.browser,
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
                let _ = reply.send(behind.serve(&request));
            }
            Ok(Event::Confirm(of, request, reply)) => {
                let _ = reply.send(standin::answers(behind.rows(), &of, &request));
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
    let marked = *lock(&marks.seen) == ["start", "end"];
    let finish = lock(&marks.finish).take();
    Ok((marked, finish))
}

/// What stands behind the link on the command's thread while a run goes.
trait Behind {
    /// The reply to a request that a host link handed this thread.
    fn serve(&mut self, request: &[u8]) -> Result<Vec<u8>, LinkError>;
    /// The rows from which the stand-in answers a confirmation.
    fn rows(&self) -> &dyn RowRead;
}

impl Behind for GateHost {
    fn serve(&mut self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        serve(self, request)
    }

    fn rows(&self) -> &dyn RowRead {
        GateHost::rows(self)
    }
}

/// The product's owner, which no thread of this command holds: the product link hands this thread no request, and
/// the command reads no row of the owner's store, so the stand-in answers every effect of a confirmation
/// `reject_once`.
struct Owner;

impl Behind for Owner {
    fn serve(&mut self, _request: &[u8]) -> Result<Vec<u8>, LinkError> {
        Err(LinkError)
    }

    fn rows(&self) -> &dyn RowRead {
        &NoRows
    }
}

/// No row of any kind.
struct NoRows;

impl RowRead for NoRows {
    fn for_each(&self, _kind: &str, _visit: &mut dyn FnMut(&str)) {}
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

/// Why one coordinator body over the product link brought no reply that answers it.
enum Missed {
    /// The link gave no reply: the owner could not be reached, refused the stream, or the stream ended.
    NoReply,
    /// The owner refused the body with this error name.
    Refused(ErrorName),
    /// A reply that `decode_reply` refuses, with another id, or of another form than the body's call answers with.
    OtherForm,
}

impl Missed {
    /// The detail of a refusal for the body `op`: the op and the owner's error name, no value.
    fn detail(&self, op: &str) -> String {
        match self {
            Missed::NoReply => format!("the product link gave no reply to the body {op}"),
            Missed::Refused(error) => {
                format!("the owner refused the body {op} with {}", error.as_str())
            }
            Missed::OtherForm => format!("the owner answered the body {op} with another form"),
        }
    }
}

/// Sends one coordinator body over the product link under a new request id and reads its reply, as the supervisor
/// library does with its own bodies: the reply counts only with the body's id and the form that the body's call
/// answers with.
fn exchange(link: &dyn OwnerLink, call: CoordinatorCall) -> Result<CoordinatorReply, Missed> {
    let id = RequestId::from_random(minted());
    let write = CoordinatorWrite::new(&CoordinatorRole::claim(), id.clone(), call);
    let bytes = write.to_bytes().map_err(|_| Missed::OtherForm)?;
    let reply = link.request(&bytes).map_err(|_| Missed::NoReply)?;
    let reply = decode_reply(&reply).map_err(|_| Missed::OtherForm)?;
    if reply.id.as_ref() != Some(&id) {
        return Err(Missed::OtherForm);
    }
    let raw = match reply.outcome {
        Ok(ReplyResult::Raw(raw)) => raw,
        Ok(_) => return Err(Missed::OtherForm),
        Err(error) => return Err(Missed::Refused(error)),
    };
    match CoordinatorReply::parse(raw.get().as_bytes()) {
        Ok(value) if value.answers(write.body()) => Ok(value),
        _ => Err(Missed::OtherForm),
    }
}

/// The set-up of a run on the owner over the product link, on the stored workflow `workflow`: the agent by
/// `create_agent`, the slots by `set_bindings`, the agent `active` by `set_state`, and `start_run` in the mode
/// `replay`, the bodies that `set_up` sends to the gate host after its workflow. It stops at the first body that
/// does not get the reply of its call.
fn set_up_over(
    link: &dyn OwnerLink,
    workflow: &WorkflowId,
    agent: &AgentId,
    slots: Vec<Slot>,
    run: &RunId,
) -> Result<(), Refused> {
    let calls = [
        CoordinatorCall::CreateAgent(CreateAgent {
            workflow: workflow.clone(),
            agent: agent.clone(),
        }),
        CoordinatorCall::SetBindings(SetBindings {
            workflow: workflow.clone(),
            agent: agent.clone(),
            slots,
        }),
        CoordinatorCall::SetState(SetState {
            workflow: workflow.clone(),
            agent: agent.clone(),
            state: AgentState::Active,
        }),
        CoordinatorCall::StartRun(StartRun {
            workflow: workflow.clone(),
            agent: agent.clone(),
            run: run.clone(),
            mode: Mode::Replay,
        }),
    ];
    for call in calls {
        let op = call.op();
        exchange(link, call).map_err(|missed| usage(missed.detail(op)))?;
    }
    Ok(())
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

/// The gate's `MarkSink`: what the library called for the run, in order, the record of the run's acknowledged
/// `finish` that the end carries, and the end handed to the command.
struct Marks {
    seen: Mutex<Vec<&'static str>>,
    finish: Mutex<Option<AgentRun>>,
    events: Sender<Event>,
}

impl MarkSink for Marks {
    fn start(&self, _run: &RunId) {
        lock(&self.seen).push("start");
    }

    fn end(&self, _run: &RunId, end: &RunEnd) {
        lock(&self.seen).push("end");
        lock(&self.finish).clone_from(&end.finish);
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
