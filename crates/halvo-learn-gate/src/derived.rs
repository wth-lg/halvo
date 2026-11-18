use std::cell::{Cell, RefCell};
use std::collections::hash_map::RandomState;
use std::collections::BTreeSet;
use std::hash::{BuildHasher, Hasher};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use halvo_agent_store::{Host as AgentHost, Now};
use halvo_derived::{
    Clock as StoreClock, CommitOutcome, DbKey, Error, Handler, HandlerCx, Host, IdSource, MoveTo,
    Store, Takes,
};
use halvo_learn_store::{Clock, LearnHandlers, LearnHost, LearnStep};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::names::AgentState;
use halvo_schema::executable::RunnerReply;
use halvo_schema::kinds::registry;
use halvo_schema::request::decode;
use halvo_schema::{
    AgentId, AppId, Confidence, Decoded, Domain, DomainSource, Episode, ErrorName, Instant,
    InterfaceDecision, InterfaceRead, InterfaceRole, KindEntry, ObservedSite, Ordinal, Part,
    Position, Proposal, RawRequest, Recurrence, RecurrenceStep, ReplyResult, RequestId, RowRead,
    SeedCategory, SiteDomain, SiteValue, StoreId, SummaryId, SummaryRead, Trigger, TypedRecord,
    Workflow, WorkflowId, WorkflowItem,
};
use serde_json::value::RawValue;

use crate::host::{noon, PlayHost};
use crate::stub::{id, Arm};

/// The further role of the gate's own read of the rows of the family `learn` (W-C3.12, W-C25.5): a test host's
/// seam, which no product host has.
pub const READ_ROLE: &str = "learn_gate_read";

/// The app of every summary and workflow that the gate makes.
pub const APP: &str = "com.example.browser";

const AGENT_OPS: &[&str] = &[
    "create_agent",
    "set_bindings",
    "set_state",
    "start_run",
    "end_run",
    "add_origin",
    "list_checkpoints",
    "mark_checkpoint",
];

const LEARN_OPS: &[&str] = &[
    "create_agent",
    "set_bindings",
    "set_state",
    "start_run",
    "end_run",
    "add_origin",
    "list_checkpoints",
    "mark_checkpoint",
    "list_refinements",
];

/// The key of the gate's derived stores (contracts.md L10.1; the coordinator's ruling LM-89): 32 bytes of the
/// process's random keys, which `RandomState` takes from the operating system's generator, made once by a command,
/// handed to `Store::open` alone, held in memory and never written, printed or logged (L0.9). A store that the gate
/// made cannot be opened after its process has ended.
pub fn process_key() -> DbKey {
    let state = RandomState::new();
    let mut bytes = [0u8; 32];
    for (at, chunk) in (0u64..).zip(bytes.chunks_mut(8)) {
        let mut hasher = state.build_hasher();
        hasher.write_u64(at);
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    let key = DbKey::new(bytes);
    bytes.fill(0);
    std::hint::black_box(&bytes);
    key
}

/// The site of the workflow numbered `n`: each workflow of one store under a key of its own (W-C2.4).
pub fn site_of(n: u32) -> String {
    match n {
        1 => "northwind-power.test".to_owned(),
        _ => format!("northwind-power-{n}.test"),
    }
}

/// The clock of a play on the derived store: the day that the command sets, read by the store, the handlers and the
/// step.
#[derive(Clone, Default)]
pub struct Day(Rc<Cell<u32>>);

impl Day {
    pub fn get(&self) -> u32 {
        self.0.get()
    }

    pub fn set(&self, day: u32) {
        self.0.set(day);
    }
}

impl StoreClock for Day {
    fn now(&self) -> Instant {
        noon(self.get())
    }

    fn day(&self, at: Instant) -> u32 {
        day_of(at, 0)
    }

    fn day_start(&self, day: u32) -> Instant {
        Instant(i64::from(day).saturating_mul(86_400_000_000_000))
    }
}

impl Clock for Day {
    fn today(&self) -> u32 {
        self.get()
    }
}

struct Seq(u64);

impl IdSource for Seq {
    fn fill(&mut self, bytes: &mut [u8]) {
        for chunk in bytes.chunks_mut(8) {
            self.0 += 1;
            let word = self.0.to_be_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }
}

struct Proposer(Rc<RefCell<Vec<Proposal>>>);

impl RecurrenceStep for Proposer {
    fn propose(&self, _summaries: &dyn SummaryRead) -> Vec<Proposal> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

enum Call {
    Commit(Vec<TypedRecord>),
    State(WorkflowId, MoveTo, Option<AgentId>),
    Rows(String),
    Workflow(WorkflowId),
}

enum Answer {
    Done(Result<(), ErrorName>),
    Rows(Vec<String>),
    Workflow(Option<WorkflowItem>),
    Failed,
}

#[derive(Default)]
struct Channel {
    call: RefCell<Option<Call>>,
    answer: RefCell<Option<Answer>>,
    broken: Cell<bool>,
}

fn refused(error: Error) -> ErrorName {
    match error {
        Error::Refused(name) => name,
        Error::Fault(_) => ErrorName::Store,
    }
}

fn move_to(state: AgentState) -> MoveTo {
    match state {
        AgentState::Approved => MoveTo::Approved,
        AgentState::Active => MoveTo::Active,
        AgentState::Paused => MoveTo::Paused,
    }
}

fn raw(json: String) -> Result<Box<RawValue>, ErrorName> {
    RawValue::from_string(json).map_err(|_| ErrorName::Internal)
}

struct AgentCx<'c, 'x> {
    cx: &'c mut HandlerCx<'x>,
    fault: Cell<bool>,
}

impl AgentHost for AgentCx<'_, '_> {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.cx
            .commit(&records)
            .map(|_| ())
            .map_err(|_| ErrorName::Store)
    }

    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: AgentState,
        agent: Option<&AgentId>,
    ) -> Result<(), ErrorName> {
        self.cx
            .state(workflow, move_to(state), agent.cloned())
            .map_err(refused)
    }

    fn rows(&self) -> &dyn RowRead {
        self
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.cx.workflow(id).unwrap_or_else(|_| {
            self.fault.set(true);
            None
        })
    }

    fn day(&self, at: Instant) -> u32 {
        day_of(at, 0)
    }
}

impl RowRead for AgentCx<'_, '_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        self.cx.rows().for_each(kind, visit);
    }
}

fn serve(host: &mut AgentCx<'_, '_>, call: Call) -> Answer {
    match call {
        Call::Commit(records) => Answer::Done(AgentHost::commit(host, records)),
        Call::State(workflow, to, agent) => {
            Answer::Done(host.cx.state(&workflow, to, agent).map_err(refused))
        }
        Call::Rows(kind) => {
            let mut rows = Vec::new();
            host.cx
                .rows()
                .for_each(&kind, &mut |row| rows.push(row.to_owned()));
            Answer::Rows(rows)
        }
        Call::Workflow(id) => match host.cx.workflow(&id) {
            Ok(item) => Answer::Workflow(item),
            Err(_) => Answer::Failed,
        },
    }
}

/// The agent store's handler, the handler of the owning run, last in the host's order (W-C25.5, AG-A7.5), with the
/// calls of the family `agent`. Behind `LearnHandlers` it makes the calls of the family `agent` that the agent
/// store's code in front asks of it, each through one `HandlerCx::pass_on`.
struct AgentStore {
    day: Day,
    channel: Rc<Channel>,
}

impl Handler for AgentStore {
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let pending = self.channel.call.borrow_mut().take();
        let mut host = AgentCx {
            cx,
            fault: Cell::new(false),
        };
        if let Some(call) = pending {
            let answer = serve(&mut host, call);
            *self.channel.answer.borrow_mut() = Some(answer);
            return raw("null".to_owned());
        }
        let now = Now {
            at: noon(self.day.get()),
        };
        let body = request.body().get().as_bytes();
        let json = match request.name() {
            "runner" => halvo_agent_store::runner(&mut host, now, body)?
                .to_json()
                .map_err(|_| ErrorName::Internal)?,
            "coordinator_write" => halvo_agent_store::coordinator(&mut host, now, body)?
                .to_json()
                .map_err(|_| ErrorName::Internal)?,
            _ => return Err(ErrorName::Unsupported),
        };
        if host.fault.get() {
            return Err(ErrorName::Store);
        }
        raw(json)
    }
}

struct Bridge<'r, 'c, 'x> {
    cx: &'r RefCell<&'c mut HandlerCx<'x>>,
    channel: &'r Channel,
}

impl Bridge<'_, '_, '_> {
    fn ask(&self, call: Call) -> Answer {
        *self.channel.call.borrow_mut() = Some(call);
        let passed = self.cx.borrow_mut().pass_on();
        let unanswered = self.channel.call.borrow_mut().take().is_some();
        let answer = self.channel.answer.borrow_mut().take();
        match (passed, unanswered, answer) {
            (Ok(_), false, Some(answer)) => answer,
            _ => {
                self.channel.broken.set(true);
                Answer::Failed
            }
        }
    }
}

impl AgentHost for Bridge<'_, '_, '_> {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        match self.ask(Call::Commit(records)) {
            Answer::Done(done) => done,
            _ => Err(ErrorName::Internal),
        }
    }

    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: AgentState,
        agent: Option<&AgentId>,
    ) -> Result<(), ErrorName> {
        match self.ask(Call::State(
            workflow.clone(),
            move_to(state),
            agent.cloned(),
        )) {
            Answer::Done(done) => done,
            _ => Err(ErrorName::Internal),
        }
    }

    fn rows(&self) -> &dyn RowRead {
        self
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        match self.ask(Call::Workflow(id.clone())) {
            Answer::Workflow(item) => item,
            _ => None,
        }
    }

    fn day(&self, at: Instant) -> u32 {
        day_of(at, 0)
    }
}

impl RowRead for Bridge<'_, '_, '_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        if let Answer::Rows(rows) = self.ask(Call::Rows(kind.to_owned())) {
            for row in &rows {
                visit(row);
            }
        }
    }
}

struct Front<'r, 'c, 'x> {
    cx: &'r RefCell<&'c mut HandlerCx<'x>>,
    agent: Bridge<'r, 'c, 'x>,
    day: u32,
    fault: Cell<bool>,
}

impl Clock for Front<'_, '_, '_> {
    fn today(&self) -> u32 {
        self.day
    }
}

impl LearnHost for Front<'_, '_, '_> {
    fn agent_host(&mut self) -> &mut dyn AgentHost {
        &mut self.agent
    }

    fn agent_now(&self) -> Now {
        Now { at: noon(self.day) }
    }

    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.cx
            .borrow_mut()
            .commit(&records)
            .map(|_| ())
            .map_err(|_| ErrorName::Store)
    }

    fn rows(&self) -> &dyn RowRead {
        self
    }

    fn for_each_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        let mut rows = Vec::new();
        self.cx
            .borrow()
            .rows()
            .for_each_under(kind, workflow, &mut |row| rows.push(row.to_owned()));
        for row in &rows {
            visit(row);
        }
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        let item = self.cx.borrow().workflow(id);
        item.unwrap_or_else(|_| {
            self.fault.set(true);
            None
        })
    }
}

impl RowRead for Front<'_, '_, '_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        let mut rows = Vec::new();
        self.cx
            .borrow()
            .rows()
            .for_each(kind, &mut |row| rows.push(row.to_owned()));
        for row in &rows {
            visit(row);
        }
    }
}

/// `LearnHandlers` as a handler of the store, in front of the agent store's handler for `runner` and
/// `coordinator_write` (contracts.md L4.1, L4.4; W-C25.5): its own calls are those of the family `learn` on its
/// `HandlerCx`, and each call of the family `agent` that the agent store's code makes for it goes behind by one
/// `HandlerCx::pass_on`, so that every handler keeps the calls of its own family. One value of `LearnHandlers` serves
/// every request of the host's life.
struct LearnFront {
    day: Day,
    handlers: Rc<RefCell<LearnHandlers>>,
    channel: Rc<Channel>,
}

impl Handler for LearnFront {
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let body = request.body().get().as_bytes();
        let cx = RefCell::new(cx);
        let mut front = Front {
            cx: &cx,
            agent: Bridge {
                cx: &cx,
                channel: &self.channel,
            },
            day: self.day.get(),
            fault: Cell::new(false),
        };
        self.channel.broken.set(false);
        let mut handlers = self.handlers.borrow_mut();
        let json = match request.name() {
            "runner" => handlers
                .runner(&mut front, body)
                .map_err(|e| e.error_name())?
                .to_json()
                .map_err(|_| ErrorName::Internal)?,
            "coordinator_write" => handlers
                .coordinator(&mut front, body)
                .map_err(|e| e.error_name())?
                .to_json()?,
            _ => return Err(ErrorName::Unsupported),
        };
        if self.channel.broken.get() || front.fault.get() {
            return Err(ErrorName::Internal);
        }
        raw(json)
    }
}

/// The gate's read of the rows of one kind of the family `learn` as the store hands them out, under the further role
/// `READ_ROLE`: the body `{"kind":<kind>}`, the reply the list of the rows.
struct GateRead;

impl Handler for GateRead {
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let body: serde_json::Value =
            serde_json::from_str(request.body().get()).map_err(|_| ErrorName::Malformed)?;
        let kind = body["kind"].as_str().ok_or(ErrorName::Malformed)?;
        let mut rows: Vec<String> = Vec::new();
        cx.rows()
            .for_each(kind, &mut |row| rows.push(row.to_owned()));
        raw(serde_json::to_string(&rows).map_err(|_| ErrorName::Internal)?)
    }
}

/// The episode of one summary of the workflow numbered `n` on `day`: the seed category, the site of the workflow
/// (`site_of`) in the address field, the app `APP`, one segment `ordinal` and no observed step, so that its summary
/// holds the workflow's key (W-C2.4) and the step reads it as the user's doing of the task (L6.1).
pub fn episode(summary: SummaryId, n: u32, day: u32, ordinal: u64) -> Result<Episode, String> {
    let start = noon(day);
    Ok(Episode {
        id: summary,
        category: SeedCategory::UtilityBillPayment,
        site: Some(ObservedSite::AddressField {
            domain: Domain::parse(&site_of(n)).map_err(|_| "the site of a summary")?,
        }),
        app: AppId::parse(APP).map_err(|_| "the app of a summary")?,
        day,
        part: Part::Evening,
        minutes: 5,
        keys: None,
        pointer: None,
        segments: vec![Ordinal::new(ordinal).map_err(|_| "the ordinal of a summary")?],
        expires: Instant(i64::MAX),
        start,
        end: Instant(start.0.saturating_add(1_000)),
        steps: Vec::new(),
    })
}

fn copy_store(from: &Path, to: &Path) -> Result<(), String> {
    let (from, to) = (from.join("derived"), to.join("derived"));
    let fail = |_| "a copy of the derived store cannot be made".to_owned();
    std::fs::create_dir_all(&to).map_err(fail)?;
    for entry in std::fs::read_dir(&from).map_err(fail)? {
        let entry = entry.map_err(fail)?;
        if entry.file_type().map_err(fail)?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(fail)?;
        }
    }
    Ok(())
}

/// The gate host on the derived store (contracts.md L10.1): halvo-derived opened by `Store::open` on a data
/// directory with the command's key and `kinds::registry()`, its clock on the day that the command sets, the test
/// proposer as its recurrence step, `LearnStep` after it where a play asks for the step, and the handlers of the arm:
/// `LearnHandlers` in front of the agent store's handler in the arm `admit`, the agent store's handler alone in the
/// arms `none` and `plain`, and the gate's read under `READ_ROLE`. A workflow comes from one summary that the test
/// proposer proposes and an `interface_decision` with `approve`; a restart drops the store and opens it again with
/// a new value of `LearnHandlers`; the delete of a workflow is the decision `delete`. The host writes under its own
/// directory alone.
pub struct DerivedHost {
    root: PathBuf,
    data: PathBuf,
    key: DbKey,
    day: Day,
    arm: Arm,
    step: bool,
    seed_day: Option<u32>,
    handlers: Rc<RefCell<LearnHandlers>>,
    proposals: Rc<RefCell<Vec<Proposal>>>,
    store: Option<Store>,
    ids: Seq,
    batches: u64,
    requests: u64,
    copies: u32,
    kept: Option<PathBuf>,
    seen_runs: BTreeSet<String>,
    counted_runs: BTreeSet<String>,
}

impl DerivedHost {
    /// A host of a store under `root`, which holds the store's `derived/` and, in the arm `none`, the copies of the
    /// store after the compile, with its clock on `day`; the store opens at its first use.
    pub fn new(root: PathBuf, key: DbKey, day: u32) -> DerivedHost {
        let clock = Day::default();
        clock.set(day);
        DerivedHost {
            data: root.clone(),
            root,
            key,
            day: clock,
            arm: Arm::Admit,
            step: false,
            seed_day: None,
            handlers: Rc::new(RefCell::new(LearnHandlers::new())),
            proposals: Rc::new(RefCell::new(Vec::new())),
            store: None,
            ids: Seq(0),
            batches: 0,
            requests: 0,
            copies: 0,
            kept: None,
            seen_runs: BTreeSet::new(),
            counted_runs: BTreeSet::new(),
        }
    }

    fn host(&self) -> Host {
        let channel = Rc::new(Channel::default());
        let mut host = Host::new(
            Box::new(self.day.clone()),
            Box::new(Proposer(Rc::clone(&self.proposals))),
        );
        if self.step {
            host = host.step("learn", Box::new(LearnStep::new(self.day.clone())));
        }
        let front = || LearnFront {
            day: self.day.clone(),
            handlers: Rc::clone(&self.handlers),
            channel: Rc::clone(&channel),
        };
        let behind = || AgentStore {
            day: self.day.clone(),
            channel: Rc::clone(&channel),
        };
        host = match self.arm {
            Arm::Admit => host
                .handler("learn", Takes::Runner, Box::new(front()))
                .handler("learn", Takes::Coordinator(LEARN_OPS), Box::new(front()))
                .handler("agent", Takes::Runner, Box::new(behind()))
                .handler("agent", Takes::Coordinator(LEARN_OPS), Box::new(behind())),
            Arm::None | Arm::Plain => host
                .handler("agent", Takes::Runner, Box::new(behind()))
                .handler("agent", Takes::Coordinator(AGENT_OPS), Box::new(behind())),
        };
        host.handler("learn", Takes::Role(READ_ROLE), Box::new(GateRead))
    }

    fn open(&mut self) -> Result<(), String> {
        if self.store.is_none() {
            let entries: Vec<&'static KindEntry> = registry().collect();
            let host = self.host();
            let store = Store::open(&self.data, &self.key, &entries, host, &mut self.ids)
                .map_err(|e| format!("the derived store does not open: {e}"))?;
            self.store = Some(store);
        }
        Ok(())
    }

    fn store(&mut self) -> Result<&mut Store, String> {
        self.open()?;
        self.store
            .as_mut()
            .ok_or_else(|| "the derived store is not open".to_owned())
    }

    /// Drops the store and opens it again on the clock's day, with the same handlers: the open's expiry removes the
    /// rows that passed their bound.
    pub fn reopen(&mut self) -> Result<(), String> {
        self.store = None;
        self.open()
    }

    fn request_id(&mut self) -> RequestId {
        self.requests += 1;
        let mut bytes = [0u8; 16];
        bytes[8..].copy_from_slice(&self.requests.to_be_bytes());
        RequestId::from_random(bytes)
    }

    fn handle(&mut self, request: &str, body: &str) -> Result<Box<RawValue>, String> {
        let id = self.request_id();
        let text = format!(
            r#"{{"v":1,"id":"{}","request":"{request}","body":{body}}}"#,
            id.as_str()
        );
        let raw = match decode(text.as_bytes()) {
            Ok(Decoded::Raw(raw)) => raw,
            _ => return Err("a request that the owner's reader refuses".to_owned()),
        };
        self.store()?.handle(&raw).map_err(|e| format!("{e:?}"))
    }

    fn read(&mut self, kind: &str) -> Result<Vec<String>, String> {
        let body = serde_json::json!({ "kind": kind }).to_string();
        let reply = self.handle(READ_ROLE, &body)?;
        serde_json::from_str(reply.get()).map_err(|_| "the gate's read gives no list".to_owned())
    }

    /// Commits one batch of the analysis with these episodes under the next position, after which the store runs its
    /// steps; refused when a step's records are refused.
    pub fn batch(&mut self, episodes: &[Episode]) -> Result<CommitOutcome, String> {
        self.batches += 1;
        let position = Position {
            seg: self.batches,
            idx: 0,
        };
        let source = StoreId::parse(&id('8', 1)).map_err(|_| "the gate's store id")?;
        self.open()?;
        let store = self
            .store
            .as_mut()
            .ok_or_else(|| "the derived store is not open".to_owned())?;
        let outcome = store
            .commit_batch(&source, episodes, position, &mut self.ids)
            .map_err(|e| format!("the batch is refused: {e}"))?;
        if outcome.repeated {
            return Err("the batch reads as repeated".to_owned());
        }
        for step in &outcome.steps {
            if let Err(refusal) = step {
                return Err(format!("a step's records are refused: {refusal}"));
            }
        }
        Ok(outcome)
    }

    /// The next ordinal of a batch's episode.
    pub fn next_ordinal(&self) -> u64 {
        self.batches + 1
    }

    fn listed(&mut self) -> Result<Vec<Workflow>, String> {
        let role = InterfaceRole::claim();
        let mut found = Vec::new();
        let mut after = None;
        loop {
            let read = InterfaceRead::workflows(&role, self.request_id(), after);
            let page = match self.store()?.interface_read(&read) {
                Ok(ReplyResult::Workflows(page)) => page,
                _ => return Err("the read of the workflows is refused".to_owned()),
            };
            found.extend(page.workflows.into_iter().map(|item| item.workflow));
            match page.next {
                Some(next) => after = Some(next),
                None => return Ok(found),
            }
        }
    }

    fn seed_workflow(&mut self, n: u32, day: u32) -> Result<WorkflowId, String> {
        let site = site_of(n);
        let summary = SummaryId::parse(&id('7', n)).map_err(|_| "the seed summary's id")?;
        self.proposals.borrow_mut().push(Proposal {
            category: SeedCategory::UtilityBillPayment,
            site: Some(SiteValue::Domain {
                key: SiteDomain::parse(&site).map_err(|_| "the site of a workflow")?,
                source: DomainSource::AddressField,
            }),
            app: AppId::parse(APP).map_err(|_| "the app of a workflow")?,
            trigger: Trigger::Schedule,
            recurrence: Recurrence {
                count: 1,
                period: None,
                first: day,
                last: day,
            },
            confidence: Confidence(87),
            summaries: vec![summary.clone()],
        });
        let seed = episode(summary, n, day, self.next_ordinal())?;
        let outcome = self.batch(&[seed])?;
        if outcome.proposals.stored != 1 {
            return Err("the test proposer's workflow is not stored".to_owned());
        }
        let id = self
            .listed()?
            .into_iter()
            .find_map(|workflow| match workflow {
                Workflow::Full(full)
                    if matches!(&full.site, Some(SiteValue::Domain { key, .. }) if key.as_str() == site) =>
                {
                    Some(full.id)
                }
                _ => None,
            })
            .ok_or("the test proposer's workflow is not listed")?;
        let approve =
            InterfaceDecision::approve(&InterfaceRole::claim(), self.request_id(), id.clone());
        self.store()?
            .decide(&approve)
            .map_err(|e| format!("the approval is refused: {e}"))?;
        Ok(id)
    }
}

impl PlayHost for DerivedHost {
    fn link(&mut self, arm: Arm) {
        self.arm = arm;
    }

    fn set_day(&mut self, day: u32) {
        self.day.set(day);
    }

    fn now(&self) -> Now {
        Now {
            at: noon(self.day.get()),
        }
    }

    fn request_day(&self) -> u32 {
        day_of(noon(self.day.get()), 0)
    }

    fn store_workflow(&mut self, n: u32, _workflow: &str) -> Result<String, String> {
        let today = self.day.get();
        let seed = self.seed_day.unwrap_or(today);
        self.day.set(seed);
        let made = self.seed_workflow(n, seed);
        self.day.set(today);
        let id = made?;
        if seed != today {
            self.reopen()?;
        }
        Ok(id.as_str().to_owned())
    }

    fn coordinator(&mut self, body: &[u8]) -> Result<(), String> {
        let body = std::str::from_utf8(body).map_err(|_| "a body that is no UTF-8".to_owned())?;
        self.handle("coordinator_write", body).map(|_| ())
    }

    fn runner(&mut self, body: &[u8]) -> Result<RunnerReply, String> {
        let body = std::str::from_utf8(body).map_err(|_| "a body that is no UTF-8".to_owned())?;
        let reply = self.handle("runner", body)?;
        RunnerReply::parse(reply.get().as_bytes())
            .map_err(|_| "a runner reply that its parser refuses".to_owned())
    }

    fn learn_rows(&mut self, kind: &str) -> Result<Vec<String>, String> {
        self.read(kind)
    }

    fn ended(&mut self) -> Result<(), String> {
        let rows = self.read("learn_run")?;
        self.seen_runs.extend(rows);
        Ok(())
    }

    fn restart(&mut self) -> Result<u32, String> {
        self.handlers = Rc::new(RefCell::new(LearnHandlers::new()));
        self.reopen()?;
        let now: BTreeSet<String> = self.read("learn_run")?.into_iter().collect();
        let gone: Vec<String> = self
            .seen_runs
            .iter()
            .filter(|row| !now.contains(*row) && !self.counted_runs.contains(*row))
            .cloned()
            .collect();
        self.counted_runs.extend(gone.iter().cloned());
        self.seen_runs.extend(now);
        Ok(u32::try_from(gone.len()).unwrap_or(u32::MAX))
    }

    fn keep_after_compile(&mut self) -> Result<(), String> {
        self.store = None;
        let kept = self.root.join("after-compile");
        copy_store(&self.data, &kept)?;
        self.kept = Some(kept);
        self.open()
    }

    fn back_to_after_compile(&mut self) -> Result<(), String> {
        let Some(kept) = self.kept.clone() else {
            return Ok(());
        };
        self.store = None;
        self.copies += 1;
        let data = self.root.join(format!("none-{}", self.copies));
        copy_store(&kept, &data)?;
        self.data = data;
        self.open()
    }
}

/// A directory of the gate's own under the system's temporary directory (`std::env::temp_dir`), made for one
/// command, with one directory under it for each play, and removed with everything in it when the value is dropped,
/// so that a command leaves no file there. It holds the derived stores of `drift` and `routine`, which take no data
/// directory.
pub struct Scratch {
    path: PathBuf,
    plays: AtomicU32,
}

impl Scratch {
    pub fn make() -> Result<Scratch, String> {
        let base = std::env::temp_dir();
        for n in 0u32..1_000 {
            let path = base.join(format!("learn-gate-{}-{n}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Scratch {
                        path,
                        plays: AtomicU32::new(0),
                    })
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }
        Err("the gate's temporary directory cannot be made".to_owned())
    }

    /// A new empty directory for one play.
    pub fn play_dir(&self) -> Result<PathBuf, String> {
        let n = self.plays.fetch_add(1, Ordering::Relaxed) + 1;
        let path = self.path.join(format!("play-{n}"));
        std::fs::create_dir(&path).map_err(|_| "a play's directory cannot be made".to_owned())?;
        Ok(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
