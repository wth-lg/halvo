//! The product's ports of the coordinator on a Mac and on Windows (contracts I5.10; units U5.4a and U5.4w): the hub of
//! the app runs over them in the place of the stand-in `NoService`.
//!
//! - `Owner` writes and reads as the role `coordinator` over an owner stream of the app's own (W-C24.13), through
//!   the product's link of the supervisor library: `coord_log` and `coord_history` for the events, `list_agents`,
//!   `agent_history`, `list_overlaps` and the bodies of `Body` for the agent store. The list of the workflows and the
//!   decision `delete` are reads and decisions of the role `interface`, which a stream refuses: they go through the
//!   app's relay call. No write of the coordinator goes through the relay. Where this build's table of hosts names a
//!   host, each list of the agents hands each agent's configuration what the core shows of an AI model (r4 U3.17b):
//!   the host, the model words, whether `agreement::stands` holds for the stored tier, and, where an agent's runtime
//!   asks a model, the key's state and the count's state, read by ONE body each through `ModelLimit` with a memory of
//!   the rows' own.
//! - `Runs` is the supervisor library over a second owner stream: `start` hands the machine's zone offset of the
//!   moment as `utc_offset_s`, and a hosted tier as `hosted` only where `crate::agreement::hosted` gives one for the
//!   agent as the newest list kept it (r4 U3.17a); the first `reap` of a life answers what the start's own reap
//!   answered.
//! - `Analysis` sends the capture service the request `defer` of the core's deadline at each call (unit U5.4c).
//! - `Clock` reads the system clock and the system's zone, `Rng` the system's generator, `Policy` the table of
//!   halvo-policy (`decide_row`) and `Senders` the schema's reader of `reply_senders`.
//! - The library's `Secrets` is the app's arm of `crate::model_key` over the product's own owner link: it reads the
//!   person's key for a model's host by ONE body at the library's call (r4 U3.18).
//!
//! The library's calls back (`MarkSink`, `Human`, `Confirmer`) reach the coordinator through one thread of their own,
//! so that no thread of a run ever waits for the coordinator's lock: a confirmation and the second step of a sign-in
//! wait for the person's answer, which `Runs::answer_confirm` and `Runs::answer_bind` hand them, or for the run's
//! cancel or kill, which rejects and declines. The supervisor's link puts each effect row that a runner stores to the
//! core's guard first (`Coordinator::check_record`, which asks `Policy`), answers a refused one `approval` without
//! sending it, and so one that the guard cannot read and every one while no hub is attached, and reports each one that
//! the owner acknowledged (`Coordinator::stored`). Each run of this life ends through `MarkSink::end`; a run
//! of an earlier life that the start's reap answers is ended by the core's recovery, and the start sends the one
//! mark `run_end` for it itself, because the library's reap calls no `MarkSink`.
#![cfg(any(target_os = "macos", windows))]

use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::io::Read;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use halvo_agent_client::link::product::ClientEnds;
use halvo_agent_client::link::StreamLink;
use halvo_agent_client::{
    Confirmer, Human, IssueRefused, LinkError, MarkSink, OriginAnswer, OwnerLink, Parts,
    ReapAnswer, RunEnd, Supervisor,
};
use halvo_control::client::ClientError;
use halvo_control::r2::{OwnerStreamEnd, RunEdge, RunMarked};
use halvo_coord::guard::{EffectRecord, EffectState, Policy, Refusal as GuardRefusal};
use halvo_coord::items::drive::log_text;
use halvo_coord::ports::{
    Analysis, Clock, Overlap, Overlaps, Owner, PortError, Reading, Reaped, Refused, Rng, Runs,
    Senders,
};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Answer, Answered, ApprovalId, AskedEffect, BindAnswer, BindAsk,
    BindResult, Body, BoundField, BoundFrom, Category, CheckAnswer, Class, Cleared, ConfigView,
    ConfirmAsk, Decision, DecisionEntry, Defers, Digest, DismissedWorkflow, DoneAnswer,
    EffectAnswer, EffectId, Event, FullWorkflow, HandbackAsk, HandbackReason, HandbackResult,
    HoldSetCause, ItemId, ItemKind, Kept, LapseCause, Lapsed, LastCause, LinkAnswer, Name, Origin,
    OtherLapse, Outcome, PastEffect, PastRun, Progress, Raised, ReadyAnswer, Reply, ReplyRow,
    RunEnd as CoordRunEnd, RunEvent, RunHistory, RunId, RunSpec, SetupAnswer, SignInAnswer, Slot,
    Trigger, Unknown, What, Workflow, WorkflowId, WorkflowState, WorkflowTrigger,
};
use halvo_coord::types::{Autonomy, Budget, Limits, Permission, Reasoning, Runtime, Tier};
use halvo_coord::types::{ModelUse, SpendLimit};
use halvo_schema::executable::frame::{ConfirmDecision, ConfirmRequest, HandbackRequest};
use halvo_schema::executable::names::Answer as EffectReply;
use halvo_schema::executable::text::{EffectId as WireEffect, WebOrigin};
use halvo_schema_core::request::{decode_reply, InterfaceDecision, InterfaceRead, InterfaceRole};
use halvo_schema_core::request::{ErrorName, ReplyResult};
use serde_json::Value;

use crate::agreement::{self, Agreement, KeptAgent};
use crate::hub::{Core, Hub};
use crate::model_key::{KeyState, ModelKey};
use crate::model_limit::{LimitReply, LimitState, Memory, ModelLimit};
use crate::present::{dialog_hub, Dialog};

/// The call of one end of a new owner stream.
pub type StreamCall = Arc<dyn Fn() -> Result<OwnerStreamEnd, ClientError> + Send + Sync>;
/// The call that relays one request of the role `interface` and gives the owner's reply.
pub type RelayCall = Arc<dyn Fn(&[u8]) -> Result<Vec<u8>, ClientError> + Send + Sync>;
/// The call of one run mark.
pub type MarkCall = Arc<dyn Fn(RunEdge) -> Result<RunMarked, ClientError> + Send + Sync>;
/// The call of one request `defer` (`Client::defer`): the deadline in nanoseconds since 1970, and the deadline in
/// force that the service names, none where none is.
pub type DeferCall = Arc<dyn Fn(i64) -> Result<Option<i64>, ClientError> + Send + Sync>;

/// The app's client calls that the product's ports go through: a new owner stream, the relay of one request of the
/// role `interface`, one run mark, and one request `defer`.
#[derive(Clone)]
pub struct Calls {
    /// One end of a new owner stream (`Control::owner_stream` of the app).
    pub owner_stream: StreamCall,
    /// One request of the role `interface` relayed to the owner, and the owner's reply (`Control::relay`).
    pub relay: RelayCall,
    /// One run mark (`Control::run_mark`).
    pub mark: MarkCall,
    /// One request `defer` of the analysis batch (`Control::defer`).
    pub defer: DeferCall,
}

/// Where the product's runs live: the absolute data directory and the absolute paths that a start hands the
/// runner. A start hands it no browser: the runner finds the person's browser itself.
#[derive(Clone, Debug)]
pub struct Places {
    pub data_dir: PathBuf,
    pub runner: PathBuf,
    pub model: Option<PathBuf>,
}

/// The hub of the app over the product's ports.
pub type ProductHub = Hub<Product>;

/// The hub over the product's ports of `calls` and `places`, whose count of agents that need the person goes to
/// `tray` and whose native confirmations are shown in `dialog` with the card that the core composes
/// (`crate::dialog_hub`). It starts nothing and reads nothing: `start` takes the steps of the app's start.
pub fn hub<D: Dialog>(
    calls: Calls,
    places: Places,
    tray: Box<dyn Fn(u32) + Send + Sync>,
    dialog: Arc<D>,
) -> Arc<ProductHub> {
    let limits = Limits::PRODUCT;
    let (product, events) = Product::new(calls, places);
    let hub = dialog_hub(product, limits, tray, dialog);
    events.attach(Arc::downgrade(&hub));
    hub
}

/// The steps of the app's start, in their order: the library's reap of what an earlier life left (it ends what is
/// left of each such run and removes its mark), and one mark `run_end` where it answered a run gone, none alive,
/// and no run of this life is live; the hub's first tick, whose recovery takes that reap's answers and ends each such run with its
/// store; then the removal of the checkpoints that may go, once, before any start of a run (a start inside that
/// first tick removes them first).
pub fn start(hub: &ProductHub) {
    let product = hub.coordinator().ports().inner();
    product.reap_earlier_life();
    Core::tick(hub);
    product.remove_checkpoints_once();
}

/// The configuration of a product's agent: the coordinator's reading of the record of A-A2.1 and the agent store's
/// typed record, from which `Policy` reads the autonomy, the allowed classes and the caps.
#[derive(Clone, Debug)]
pub struct AgentConfig {
    view: halvo_coord::mem::Config,
    typed: halvo_schema::executable::AgentConfig,
}

impl AgentConfig {
    fn parse(json: &str) -> Option<AgentConfig> {
        Some(AgentConfig {
            view: halvo_coord::mem::Config::parse(json).ok()?,
            typed: halvo_schema::executable::AgentConfig::parse(json.as_bytes()).ok()?,
        })
    }
}

impl ConfigView for AgentConfig {
    fn model(&self) -> Option<ModelUse> {
        self.view.model()
    }

    fn tier(&self) -> Tier {
        self.view.tier()
    }

    fn reasoning(&self) -> Reasoning {
        self.view.reasoning()
    }

    fn autonomy(&self) -> Autonomy {
        self.view.autonomy()
    }

    fn allow_is_empty(&self) -> bool {
        self.view.allow_is_empty()
    }

    fn caps_external(&self) -> bool {
        self.view.caps_external()
    }

    fn wall_s(&self) -> u64 {
        self.view.wall_s()
    }

    fn model_tokens(&self) -> u64 {
        self.view.model_tokens()
    }

    fn budget(&self) -> Option<Budget> {
        self.view.budget()
    }

    fn permissions(&self) -> Vec<Permission> {
        self.view.permissions()
    }

    fn runtime(&self) -> Runtime {
        self.view.runtime()
    }

    fn playbook(&self) -> Option<u32> {
        self.view.playbook()
    }

    fn rev(&self) -> u32 {
        self.view.rev()
    }

    fn category(&self) -> Category {
        self.view.category()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The calls back of the library, handed to the coordinator on one thread of their own.
struct Events {
    to: Mutex<Sender<(RunId, RunEvent)>>,
    hub: Arc<OnceLock<Weak<ProductHub>>>,
}

impl Events {
    fn new() -> Arc<Events> {
        let (to, from) = mpsc::channel::<(RunId, RunEvent)>();
        let hub: Arc<OnceLock<Weak<ProductHub>>> = Arc::new(OnceLock::new());
        let seen = Arc::clone(&hub);
        std::thread::spawn(move || {
            for (run, event) in from {
                if let Some(hub) = seen.get().and_then(Weak::upgrade) {
                    let _ = hub.coordinator().on_run(&run, event);
                }
            }
        });
        Arc::new(Events {
            to: Mutex::new(to),
            hub,
        })
    }

    fn attach(&self, hub: Weak<ProductHub>) {
        let _ = self.hub.set(hub);
    }

    fn send(&self, run: RunId, event: RunEvent) {
        let _ = lock(&self.to).send((run, event));
    }
}

/// What the product knows of a run of its own: its workflow and agent, from its start or from a stored
/// `run_started`.
type RunsOf = Arc<Mutex<HashMap<RunId, (WorkflowId, AgentId)>>>;

/// The level of the run marks (the workflow run's S-59 (g)): how many runs of this life are live, and the one edge
/// that waits to be sent until a daemon has answered it.
struct Marks {
    calls: Calls,
    live: AtomicUsize,
    owed: Mutex<Option<RunEdge>>,
    sending: AtomicBool,
}

impl Marks {
    /// Keeps `edge` as the one edge owed and sends it on a thread of its own, so that no start and no end of a run
    /// waits for a mark.
    fn owe(self: &Arc<Self>, edge: RunEdge) {
        *lock(&self.owed) = Some(edge);
        self.send_owed();
    }

    /// Sends the edge owed, unless a send is under way; a daemon's answer, `stored` false among them, clears it.
    fn send_owed(self: &Arc<Self>) {
        if lock(&self.owed).is_none() || self.sending.swap(true, Ordering::SeqCst) {
            return;
        }
        let marks = Arc::clone(self);
        std::thread::spawn(move || {
            marks.send_now();
            marks.sending.store(false, Ordering::SeqCst);
        });
    }

    /// Sends the edge owed on this thread.
    fn send_now(&self) {
        let Some(edge) = *lock(&self.owed) else {
            return;
        };
        if (self.calls.mark)(edge).is_ok() {
            let mut owed = lock(&self.owed);
            if *owed == Some(edge) {
                *owed = None;
            }
        }
    }
}

/// The library's `MarkSink`: the level of the run marks, and the end of each run of this life for the
/// coordinator.
struct Sink {
    marks: Arc<Marks>,
    events: Arc<Events>,
}

impl MarkSink for Sink {
    fn start(&self, _run: &halvo_schema::executable::text::RunId) {
        if self.marks.live.fetch_add(1, Ordering::SeqCst) == 0 {
            self.marks.owe(RunEdge::Start);
        }
    }

    fn end(&self, run: &halvo_schema::executable::text::RunId, end: &RunEnd) {
        let mut live = self.marks.live.load(Ordering::SeqCst);
        let left = loop {
            let next = live.saturating_sub(1);
            match self
                .marks
                .live
                .compare_exchange(live, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => break next,
                Err(now) => live = now,
            }
        };
        if left == 0 {
            self.marks.owe(RunEdge::End);
        }
        let Ok(run) = RunId::parse(run.as_str()) else {
            return;
        };
        let outcome = end.outcome.and_then(|o| Outcome::parse(o.as_str()).ok());
        self.events.send(run, RunEvent::Ended(outcome));
    }
}

/// The calls of the library that wait for the person, by run: a confirmation's answers, the decision of the answers
/// that `Runs::answer_confirm` sent, with its record's verdict, and the answer of the second step of a sign-in. A
/// dropped sender ends the wait with reject and decline.
#[derive(Default)]
struct Waits {
    confirms: Mutex<HashMap<RunId, Sender<Replies>>>,
    issues: Mutex<HashMap<RunId, Issue>>,
    binds: Mutex<HashMap<RunId, Sender<BindResult>>>,
}

/// The library's answers to a confirmation: one per effect, in the request's order.
type Replies = Vec<(WireEffect, EffectReply)>;

/// The library's decision of the answers sent, for `Runs::answer_confirm`, and its record's verdict back.
type Issue = (Sender<ConfirmDecision>, Receiver<bool>);

impl Waits {
    /// Ends every wait of `run`: a confirmation rejects each effect and a sign-in declines.
    fn release(&self, run: &RunId) {
        lock(&self.confirms).remove(run);
        lock(&self.issues).remove(run);
        lock(&self.binds).remove(run);
    }
}

/// How long `Runs::answer_confirm` waits for the library's decision of the answers that it sent.
const ISSUE_WAIT: Duration = Duration::from_secs(10);

/// The library's `Human`: a handback, the second step of a sign-in and a progress go to the coordinator; the second
/// step waits for `Runs::answer_bind`.
struct Person {
    events: Arc<Events>,
    runs: RunsOf,
    waits: Arc<Waits>,
}

impl Human for Person {
    fn handback(&self, run: &halvo_schema::executable::text::RunId, request: &HandbackRequest) {
        let Ok(run) = RunId::parse(run.as_str()) else {
            return;
        };
        let Some((_, agent)) = lock(&self.runs).get(&run).cloned() else {
            return;
        };
        let Ok(value) = serde_json::to_value(request) else {
            return;
        };
        let Some(reason) = value["reason"]
            .as_str()
            .and_then(|r| HandbackReason::parse(r).ok())
        else {
            return;
        };
        let origin = value["origin"].as_str().and_then(|o| Origin::parse(o).ok());
        self.events.send(
            run.clone(),
            RunEvent::Handback(HandbackAsk {
                run,
                agent,
                reason,
                origin,
            }),
        );
    }

    fn allow_origin(
        &self,
        run: &halvo_schema::executable::text::RunId,
        origin: &WebOrigin,
    ) -> OriginAnswer {
        let (Ok(run), Ok(origin)) = (RunId::parse(run.as_str()), Origin::parse(origin.as_str()))
        else {
            return OriginAnswer::Decline;
        };
        let Some((_, agent)) = lock(&self.runs).get(&run).cloned() else {
            return OriginAnswer::Decline;
        };
        let (to, answer) = mpsc::channel();
        lock(&self.waits.binds).insert(run.clone(), to);
        self.events.send(
            run.clone(),
            RunEvent::Bind(BindAsk {
                run: run.clone(),
                agent,
                origin,
            }),
        );
        let given = answer.recv();
        lock(&self.waits.binds).remove(&run);
        match given {
            Ok(BindResult::Allow) => OriginAnswer::Allow,
            _ => OriginAnswer::Decline,
        }
    }

    fn progress(
        &self,
        run: &halvo_schema::executable::text::RunId,
        step: NonZeroU32,
        of: NonZeroU32,
    ) {
        let (Ok(run), Ok(progress)) = (
            RunId::parse(run.as_str()),
            Progress::new(step.get(), of.get()),
        ) else {
            return;
        };
        self.events.send(run, RunEvent::Progress(progress));
    }
}

/// The library's `Confirmer`: a confirmation goes to the coordinator with the comparison of I4.4 (each effect's
/// digest computed again from its bound fields) and waits for `Runs::answer_confirm`; a confirmation that cannot be
/// put to the coordinator, or whose wait ends by the run's cancel or kill, rejects each effect once. The decision
/// that the library makes of the answers is issued only when the coordinator's record of it succeeded.
struct Asks {
    events: Arc<Events>,
    runs: RunsOf,
    waits: Arc<Waits>,
}

/// The confirmation of the frame as the coordinator takes it.
fn confirm_ask(run: &RunId, agent: &AgentId, request: &ConfirmRequest) -> Option<ConfirmAsk> {
    let effects = request
        .effects
        .iter()
        .map(|e| {
            let fields = e
                .bound
                .fields()
                .iter()
                .map(|f| {
                    Some(BoundField {
                        name: Name::parse(f.name.as_str()).ok()?,
                        from: BoundFrom::parse(f.from.as_str()).ok()?,
                        value: f.value.pieces().to_vec(),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            Some(AskedEffect {
                effect: EffectId::parse(e.effect.as_str()).ok()?,
                class: Class::parse(e.class.as_str()).ok()?,
                digest: Digest::parse(e.digest.as_str()).ok()?,
                digest_ok: halvo_policy::digest(&e.bound).is_ok_and(|d| d == e.digest),
                fields,
                new_origin: match &e.new_origin {
                    Some(o) => Some(Origin::parse(o.as_str()).ok()?),
                    None => None,
                },
            })
        })
        .collect::<Option<Vec<_>>>()?;
    ConfirmAsk::new(run.clone(), agent.clone(), effects).ok()
}

/// The coordinator's decision of the library's decision.
fn decision_of(decision: &ConfirmDecision) -> Option<Decision> {
    let entries = decision
        .approvals
        .iter()
        .map(|a| {
            Some(DecisionEntry {
                approval: ApprovalId::parse(a.approval.as_str()).ok()?,
                effect: EffectId::parse(a.effect.as_str()).ok()?,
                digest: Digest::parse(a.digest.as_str()).ok()?,
                answer: EffectAnswer::parse(a.answer.as_str()).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Decision::new(entries).ok()
}

impl Confirmer for Asks {
    fn answers(
        &self,
        run: &halvo_schema::executable::text::RunId,
        request: &ConfirmRequest,
    ) -> Vec<(WireEffect, EffectReply)> {
        let reject = || {
            request
                .effects
                .iter()
                .map(|e| (e.effect.clone(), EffectReply::RejectOnce))
                .collect()
        };
        let Ok(run) = RunId::parse(run.as_str()) else {
            return reject();
        };
        let Some((_, agent)) = lock(&self.runs).get(&run).cloned() else {
            return reject();
        };
        let Some(ask) = confirm_ask(&run, &agent, request) else {
            return reject();
        };
        let (to, answers) = mpsc::channel();
        lock(&self.waits.confirms).insert(run.clone(), to);
        self.events.send(run.clone(), RunEvent::Confirm(ask));
        let given = answers.recv();
        lock(&self.waits.confirms).remove(&run);
        given.unwrap_or_else(|_| reject())
    }

    fn issued(
        &self,
        run: &halvo_schema::executable::text::RunId,
        decision: &ConfirmDecision,
    ) -> Result<(), IssueRefused> {
        let run = RunId::parse(run.as_str()).map_err(|_| IssueRefused)?;
        let Some((to, verdict)) = lock(&self.waits.issues).remove(&run) else {
            return Err(IssueRefused);
        };
        to.send(decision.clone()).map_err(|_| IssueRefused)?;
        match verdict.recv() {
            Ok(true) => Ok(()),
            _ => Err(IssueRefused),
        }
    }
}

/// The supervisor's link with the core's guard. Each effect row that a runner stores is put to
/// `Coordinator::check_record` first, and each one that the owner acknowledged is reported with
/// `Coordinator::stored`, never one that the guard alone passed. The guard refuses what it cannot read: a request
/// that is no JSON object naming its request, a runner's `effect` whose record it cannot read, and every runner's
/// `effect` while no hub is attached. A refused request never reaches the owner; its sender gets the error reply
/// of the guard's refusal, `approval`, with the request's id.
struct Guarded {
    link: Arc<dyn OwnerLink>,
    hub: Arc<OnceLock<Weak<ProductHub>>>,
}

/// What the guard reads of one request.
enum Asked {
    /// A request that is no runner's `effect`: it goes on unchecked.
    Other,
    /// A request that the guard cannot read, with its id where it reads one.
    Unread(Value),
    /// A runner's `effect`, with what the guard reads of its record and the request's id.
    Effect(Row, Value),
}

/// What the guard reads of the record of a runner's `effect`.
enum Row {
    /// The record, read whole.
    Read(EffectRecord),
    /// A record of the state `not_made`, which the schema names and the coordinator's reader does not, read whole
    /// but for that: no row of that state begins or approves an effect, so the guard holds nothing for it.
    NotMade,
    /// A record that the guard cannot read.
    Unread,
}

/// What the guard reads of `request`.
fn asked(request: &[u8]) -> Asked {
    let Ok(v) = serde_json::from_slice::<Value>(request) else {
        return Asked::Unread(Value::Null);
    };
    let id = match &v["id"] {
        Value::String(id) => Value::String(id.clone()),
        _ => Value::Null,
    };
    let Some(name) = v["request"].as_str() else {
        return Asked::Unread(id);
    };
    if name != "runner" || v["body"]["op"] != "effect" {
        return Asked::Other;
    }
    let row = row_of(&v["body"]["record"]).unwrap_or(Row::Unread);
    Asked::Effect(row, id)
}

/// The record of a runner's `effect` as the guard reads it, none where a key does not read.
fn row_of(r: &Value) -> Option<Row> {
    let run = RunId::parse(r["run"].as_str()?).ok()?;
    let agent = AgentId::parse(r["agent"].as_str()?).ok()?;
    let effect = EffectId::parse(r["effect"].as_str()?).ok()?;
    let class = Class::parse(r["class"].as_str()?).ok()?;
    let digest = Digest::parse(r["digest"].as_str()?).ok()?;
    let approval = match &r["approval"] {
        Value::Null => None,
        a => Some(ApprovalId::parse(a.as_str()?).ok()?),
    };
    Some(match r["state"].as_str()? {
        "not_made" => Row::NotMade,
        state => Row::Read(EffectRecord {
            run,
            agent,
            effect,
            state: EffectState::parse(state).ok()?,
            class,
            digest,
            approval,
        }),
    })
}

/// The error reply of the guard's refusal `refusal` to the request of `id`.
fn refused_reply(id: &Value, refusal: GuardRefusal) -> Vec<u8> {
    format!(r#"{{"v":1,"id":{id},"error":"{}"}}"#, refusal.name()).into_bytes()
}

impl OwnerLink for Guarded {
    fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
        let (row, id) = match asked(request) {
            Asked::Other => return self.link.request(request),
            Asked::Unread(id) => return Ok(refused_reply(&id, GuardRefusal::Approval)),
            Asked::Effect(row, id) => (row, id),
        };
        let Some(hub) = self.hub.get().and_then(Weak::upgrade) else {
            return Ok(refused_reply(&id, GuardRefusal::Approval));
        };
        let record = match row {
            Row::Unread => return Ok(refused_reply(&id, GuardRefusal::Approval)),
            Row::NotMade => return self.link.request(request),
            Row::Read(record) => record,
        };
        if let Err(refusal) = hub.coordinator().check_record(&record) {
            return Ok(refused_reply(&id, refusal));
        }
        let reply = self.link.request(request)?;
        if decode_reply(&reply).is_ok_and(|r| r.outcome.is_ok()) {
            hub.coordinator().stored(&record);
        }
        Ok(reply)
    }
}

/// The product's ports.
pub struct Product {
    link: Arc<dyn OwnerLink>,
    relay: RelayCall,
    defer: DeferCall,
    supervisor: Supervisor,
    places: Places,
    marks: Arc<Marks>,
    runs: RunsOf,
    waits: Arc<Waits>,
    agents: Mutex<HashMap<AgentId, KeptAgent>>,
    /// The memory that the read for the agents' rows keeps (r4 U3.17b): its own and never the part's.
    rows_memory: RowsMemory,
    reaped: Mutex<Option<Vec<ReapAnswer>>>,
    checkpoints: AtomicBool,
}

/// A link over the owner streams that `calls` asks for.
fn link_of(calls: &Calls) -> Arc<dyn OwnerLink> {
    link_over(Arc::clone(&calls.owner_stream))
}

/// A link over the owner streams that `call` gives.
pub(crate) fn link_over(call: StreamCall) -> Arc<dyn OwnerLink> {
    Arc::new(StreamLink::new(ClientEnds::new(move || call())))
}

impl Product {
    fn new(calls: Calls, places: Places) -> (Product, Arc<Events>) {
        let events = Events::new();
        let runs: RunsOf = Arc::default();
        let marks = Arc::new(Marks {
            calls: calls.clone(),
            live: AtomicUsize::new(0),
            owed: Mutex::new(None),
            sending: AtomicBool::new(false),
        });
        let waits = Arc::new(Waits::default());
        let link = link_of(&calls);
        let parts = Parts {
            link: Arc::new(Guarded {
                link: link_of(&calls),
                hub: Arc::clone(&events.hub),
            }),
            confirmer: Arc::new(Asks {
                events: Arc::clone(&events),
                runs: Arc::clone(&runs),
                waits: Arc::clone(&waits),
            }),
            human: Arc::new(Person {
                events: Arc::clone(&events),
                runs: Arc::clone(&runs),
                waits: Arc::clone(&waits),
            }),
            secrets: Arc::new(ModelKey::new(Arc::clone(&link))),
            marks: Arc::new(Sink {
                marks: Arc::clone(&marks),
                events: Arc::clone(&events),
            }),
        };
        let config =
            halvo_agent_client::Config::new(places.data_dir.clone(), places.runner.clone());
        let supervisor = Supervisor::new(
            halvo_schema::request::CoordinatorRole::claim(),
            config,
            parts,
        );
        let product = Product {
            link,
            relay: Arc::clone(&calls.relay),
            defer: Arc::clone(&calls.defer),
            supervisor,
            places,
            marks,
            runs,
            waits,
            agents: Mutex::new(HashMap::new()),
            rows_memory: RowsMemory::default(),
            reaped: Mutex::new(None),
            checkpoints: AtomicBool::new(false),
        };
        (product, events)
    }

    /// The library's reap of what an earlier life left, kept for the first `Runs::reap`, and the caller's own mark
    /// `run_end` where it answered a run `gone`, answered none `alive` and no run of this life is live: the library's
    /// reap calls no `MarkSink`, and a runner that the reap answers `alive` still acts, so the level is not that no
    /// run is live.
    fn reap_earlier_life(&self) {
        let Ok(answers) = self.supervisor.reap() else {
            return;
        };
        let gone = answers
            .iter()
            .any(|a| a.answer == halvo_agent_client::Reaped::Gone);
        let alive = answers
            .iter()
            .any(|a| a.answer == halvo_agent_client::Reaped::Alive);
        *lock(&self.reaped) = Some(answers);
        if gone && !alive && self.marks.live.load(Ordering::SeqCst) == 0 {
            *lock(&self.marks.owed) = Some(RunEdge::End);
            self.marks.send_now();
        }
    }

    /// `Supervisor::remove_checkpoints` once in this life.
    fn remove_checkpoints_once(&self) {
        if !self.checkpoints.swap(true, Ordering::SeqCst) {
            let _ = self.supervisor.remove_checkpoints();
        }
    }

    /// A new request id.
    fn request_id(&self) -> Result<String, PortError> {
        Ok(hex(&self.bytes16()?))
    }

    /// Sends one request of `name` with `body` on the owner stream and gives the reply's result, `null` for a null
    /// result; the owner's error name is `Refused`, and no reply is `no_service`.
    fn write(&self, name: &str, body: &str) -> Result<Value, PortError> {
        let id = self.request_id()?;
        let request = format!(r#"{{"v":1,"id":"{id}","request":"{name}","body":{body}}}"#);
        let bytes = self
            .link
            .request(request.as_bytes())
            .map_err(|_| PortError::NoService)?;
        result_of(&bytes, Some(&id))
    }

    /// Sends one `coordinator_write` body.
    fn coordinator(&self, body: &str) -> Result<Value, PortError> {
        self.write("coordinator_write", body)
    }

    /// Relays one request of the role `interface` and gives the reply's result.
    fn interface(&self, request: &[u8]) -> Result<ReplyResult, PortError> {
        let bytes = (self.relay)(request).map_err(|_| PortError::NoService)?;
        let reply = decode_reply(&bytes).map_err(|_| PortError::NoService)?;
        reply.outcome.map_err(refused)
    }

    /// The workflow of `agent`, as the newest list of the agents gave it, read again where it names none.
    fn workflow_of(&self, agent: &AgentId) -> Result<WorkflowId, PortError> {
        let kept =
            |agents: &HashMap<AgentId, KeptAgent>| agents.get(agent).map(|a| a.workflow.clone());
        if let Some(w) = kept(&lock(&self.agents)) {
            return Ok(w);
        }
        self.agents()?;
        kept(&lock(&self.agents)).ok_or_else(|| PortError::Refused(name("binding")))
    }

    /// Keeps the workflow and the agent of each run that `events` start.
    fn note(&self, events: &[Event]) {
        let mut runs = lock(&self.runs);
        for e in events {
            if let What::RunStarted { run, .. } = &e.what {
                runs.insert(run.clone(), (e.workflow.clone(), e.agent.clone()));
            }
        }
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn name(text: &str) -> Name {
    Name::parse(text).unwrap_or_else(|_| Name::parse("internal").expect("a name"))
}

fn refused(error: ErrorName) -> PortError {
    PortError::Refused(name(error.as_str()))
}

/// The result of the owner's reply `bytes` to the request `id`.
pub(crate) fn result_of(bytes: &[u8], id: Option<&str>) -> Result<Value, PortError> {
    let reply = decode_reply(bytes).map_err(|_| PortError::NoService)?;
    if id.is_some() && reply.id.as_ref().map(|i| i.as_str()) != id {
        return Err(PortError::NoService);
    }
    match reply.outcome {
        Ok(ReplyResult::Null) => Ok(Value::Null),
        Ok(ReplyResult::Raw(raw)) => {
            serde_json::from_str(raw.get()).map_err(|_| PortError::NoService)
        }
        Ok(_) => Err(PortError::NoService),
        Err(error) => Err(refused(error)),
    }
}

pub(crate) fn json_str(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

/// The text of a coordinator body of `Body`, its keys in the order of A-A7.2; `end_run` takes the workflow and the
/// agent of the run as the product knows them, and the yes of `set_model` the rows of this build's table of hosts, none
/// where its ruling allows no hosted tier.
fn body_text(body: &Body, runs: &RunsOf) -> Option<String> {
    let head = |op: &str, w: &WorkflowId, a: &AgentId| {
        format!(
            r#"{{"op":"{op}","workflow":"{}","agent":"{}""#,
            w.as_str(),
            a.as_str()
        )
    };
    Some(match body {
        Body::CreateAgent { workflow, agent } => {
            format!("{}}}", head("create_agent", workflow, agent))
        }
        Body::SetBindings {
            workflow,
            agent,
            slots,
        } => format!(
            r#"{},"slots":{}}}"#,
            head("set_bindings", workflow, agent),
            slots_text(slots)
        ),
        Body::SetState {
            workflow,
            agent,
            state,
        } => format!(
            r#"{},"state":"{}"}}"#,
            head("set_state", workflow, agent),
            state.name()
        ),
        Body::StartRun {
            workflow,
            agent,
            run,
            mode,
        } => format!(
            r#"{},"run":"{}","mode":"{}"}}"#,
            head("start_run", workflow, agent),
            run.as_str(),
            mode.name()
        ),
        Body::EndRun { run } => {
            let (workflow, agent) = lock(runs).get(run).cloned()?;
            format!(
                r#"{},"run":"{}"}}"#,
                head("end_run", &workflow, &agent),
                run.as_str()
            )
        }
        Body::DropSessions { workflow, agent } => {
            format!("{}}}", head("drop_sessions", workflow, agent))
        }
        Body::AgreeHosted { workflow, agent } => {
            let ruling = halvo_route::release::ruling();
            let rows: Vec<[&str; 3]> = ruling
                .allowed()
                .iter()
                .filter_map(|tier| ruling.row(*tier))
                .map(|row| [row.tier().as_str(), row.origin(), row.model()])
                .collect();
            yes_text(head("set_model", workflow, agent), &rows)?
        }
        Body::WithdrawHosted { workflow, agent } => format!(
            r#"{},"tier":"on_device","agreement":null}}"#,
            head("set_model", workflow, agent)
        ),
    })
}

/// The text of the yes of `set_model` (r3 U7.7, r4 U3.17a) after `head`, the text of its keys `op`, `workflow` and
/// `agent`: the ceiling tier, the tier of the last of `rows`, and one triple of tier, origin and model word for each of
/// `rows`, which are the tier, the origin and the model word of each hosted tier that the ruling allows, in the order
/// of the tiers. None where `rows` is empty.
fn yes_text(head: String, rows: &[[&str; 3]]) -> Option<String> {
    let [ceiling, ..] = rows.last()?;
    let hosts: Vec<String> = rows
        .iter()
        .map(|[tier, origin, model]| {
            format!(
                r#"{{"tier":"{tier}","origin":{},"model":{}}}"#,
                json_str(origin),
                json_str(model)
            )
        })
        .collect();
    Some(format!(
        r#"{head},"tier":"{ceiling}","agreement":[{}]}}"#,
        hosts.join(",")
    ))
}

fn slots_text(slots: &[Slot]) -> String {
    let each: Vec<String> = slots
        .iter()
        .map(|s| {
            let values: Vec<String> = s.value.iter().map(|v| json_str(v)).collect();
            format!(
                r#"{{"name":"{}","value":[{}]}}"#,
                s.name.as_str(),
                values.join(",")
            )
        })
        .collect();
    format!("[{}]", each.join(","))
}

/// A stored record of `coord_event` as the coordinator's event: the inverse of the record that `log_text` writes.
fn event_of(r: &Value) -> Option<Event> {
    let s = |key: &str| r.get(key).and_then(Value::as_str);
    let run = || RunId::parse(s("run")?).ok();
    let item = || ItemId::parse(s("item")?).ok();
    let slot = r
        .get("slot")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    let kind = || ItemKind::parse(s("kind")?).ok();
    let entry = || {
        Some(DecisionEntry {
            approval: ApprovalId::parse(s("approval")?).ok()?,
            effect: EffectId::parse(s("effect")?).ok()?,
            digest: Digest::parse(s("digest")?).ok()?,
            answer: EffectAnswer::parse(s("answer")?).ok()?,
        })
    };
    let what = match s("event")? {
        "run_started" => What::RunStarted {
            run: run()?,
            trigger: Trigger::parse(s("trigger")?).ok()?,
            slot,
        },
        "run_ended" => What::RunEnded {
            run: run()?,
            cause: CoordRunEnd::parse(s("cause")?).ok()?,
            defers: match (
                s("defer_launch"),
                r.get("defer_ticks_failed").and_then(Value::as_u64),
            ) {
                (Some(launch), Some(ticks)) => Some(Defers {
                    launch: launch == "yes",
                    ticks_failed: u32::try_from(ticks).ok()?,
                }),
                _ => None,
            },
        },
        "item_raised" => What::ItemRaised {
            item: item()?,
            raised: match kind()? {
                ItemKind::Confirm => Raised::Confirm(run()?),
                ItemKind::SignIn => Raised::SignIn(run()?),
                ItemKind::LinkAccount => Raised::LinkAccount(run()?),
                ItemKind::Stuck => Raised::Stuck(run()?),
                ItemKind::Check => Raised::Check(run()),
                ItemKind::Setup => Raised::Setup,
                ItemKind::Ready => Raised::Ready,
                ItemKind::Show => Raised::Show(run()?),
            },
        },
        "item_answered" => {
            let answer = || s("answer");
            let origin = || Origin::parse(s("origin")?).ok();
            What::ItemAnswered {
                item: item()?,
                answer: match kind()? {
                    ItemKind::Confirm => Answered::Confirm(entry()?),
                    ItemKind::SignIn => match origin() {
                        Some(origin) => Answered::SignIn(SignInAnswer::Second(BindAnswer {
                            answer: BindResult::parse(answer()?).ok()?,
                            origin,
                        })),
                        None => Answered::SignIn(SignInAnswer::First(
                            DoneAnswer::parse(answer()?).ok()?,
                        )),
                    },
                    ItemKind::LinkAccount => Answered::LinkAccount(LinkAnswer {
                        answer: DoneAnswer::parse(answer()?).ok()?,
                        origin: origin()?,
                    }),
                    ItemKind::Stuck => Answered::Stuck(DoneAnswer::parse(answer()?).ok()?),
                    ItemKind::Check => Answered::Check(CheckAnswer::parse(answer()?).ok()?),
                    ItemKind::Setup => Answered::Setup(SetupAnswer::parse(answer()?).ok()?),
                    ItemKind::Ready => Answered::Ready(ReadyAnswer::parse(answer()?).ok()?),
                    ItemKind::Show => Answered::Show(DoneAnswer::parse(answer()?).ok()?),
                },
            }
        }
        "item_lapsed" => {
            let other = || OtherLapse::parse(s("cause")?).ok();
            What::ItemLapsed {
                item: item()?,
                lapsed: match kind()? {
                    ItemKind::Confirm => Lapsed::Confirm(LapseCause::parse(s("cause")?).ok()?),
                    ItemKind::SignIn => Lapsed::SignIn(other()?),
                    ItemKind::LinkAccount => Lapsed::LinkAccount(other()?),
                    ItemKind::Stuck => Lapsed::Stuck(other()?),
                    ItemKind::Check => Lapsed::Check(other()?),
                    ItemKind::Setup => Lapsed::Setup(other()?),
                    ItemKind::Ready => Lapsed::Ready(other()?),
                    ItemKind::Show => Lapsed::Show(other()?),
                },
            }
        }
        "item_released" => What::ItemReleased {
            item: item()?,
            entry: entry()?,
        },
        "hold_set" => What::HoldSet {
            cause: match s("cause")? {
                "person" => HoldSetCause::Person,
                "failures" => HoldSetCause::Failures,
                "sign_in_refused" => HoldSetCause::SignInRefused { run: run()? },
                _ => return None,
            },
        },
        "hold_cleared" => What::HoldCleared {
            cause: match s("cause")? {
                "person" => Cleared::Person,
                "failures" => Cleared::Failures,
                "check" => Cleared::Check {
                    run: run()?,
                    answer: CheckAnswer::parse(s("answer")?).ok()?,
                },
                "sign_in_refused" => Cleared::SignInRefused { run: run()? },
                _ => return None,
            },
        },
        "slot_skipped" => What::SlotSkipped { slot },
        "run_not_stopped" => What::RunNotStopped { run: run()? },
        _ => return None,
    };
    Some(Event {
        workflow: WorkflowId::parse(s("workflow")?).ok()?,
        agent: AgentId::parse(s("agent")?).ok()?,
        seq: NonZeroU32::new(u32::try_from(r.get("seq")?.as_u64()?).ok()?)?,
        day: u32::try_from(r.get("day")?.as_u64()?).ok()?,
        at: r.get("at")?.as_i64()?,
        what,
    })
}

/// A workflow of the owner's list as the coordinator reads it.
fn workflow_of(w: &halvo_schema_core::Workflow) -> Option<Workflow> {
    let v = serde_json::to_value(w).ok()?;
    let id = WorkflowId::parse(v["id"].as_str()?).ok()?;
    let category = Category::parse(v["category"].as_str()?).ok()?;
    let site = match &v["site"] {
        Value::Object(o) if o.get("source").and_then(Value::as_str) != Some("title") => {
            o.get("key").and_then(Value::as_str).map(str::to_owned)
        }
        _ => None,
    };
    if v["state"].as_str()? == "dismissed" {
        return Some(Workflow::Dismissed(DismissedWorkflow {
            id,
            category,
            site,
        }));
    }
    let recurrence = &v["recurrence"];
    Some(Workflow::Full(FullWorkflow {
        id,
        state: WorkflowState::parse(v["state"].as_str()?).ok()?,
        category,
        site,
        trigger: WorkflowTrigger::parse(v["trigger"].as_str()?).ok()?,
        period: recurrence["period"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .and_then(NonZeroU32::new),
        last: u32::try_from(recurrence["last"].as_u64()?).ok()?,
        agent: match &v["agent"] {
            Value::Null => None,
            a => Some(AgentId::parse(a.as_str()?).ok()?),
        },
    }))
}

/// The kept fields of a row: each name with its value as a list of texts.
fn kept_of(v: &Value) -> Option<Vec<Kept>> {
    v.as_array()?
        .iter()
        .map(|k| {
            let value = match &k["value"] {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a
                    .iter()
                    .map(|x| x.as_str().map(str::to_owned))
                    .collect::<Option<_>>()?,
                _ => return None,
            };
            Some(Kept {
                name: Name::parse(k["name"].as_str()?).ok()?,
                value,
            })
        })
        .collect()
}

/// The coordinator's reading of a stored run record `agent_run`.
fn past_run(record: &Value) -> Option<PastRun> {
    Some(PastRun {
        run: RunId::parse(record["run"].as_str()?).ok()?,
        day: u32::try_from(record["day"].as_u64()?).ok()?,
        outcome: Outcome::parse(record["outcome"].as_str()?).ok()?,
        unknown: u32::try_from(record["unknown"].as_u64().unwrap_or(0)).ok()?,
    })
}

/// One entry of the reply of `list_agents`; its `refused_run` is read as `last` is: null gives none, and any other
/// value is a run id or the entry is not read (I6.1; ruling RD-73 4.5). Its `last_cause` is a name of the core's list
/// `LastCause` or none: null, a missing key and any other value, a name of the store's longer list among them, give
/// none and never stop the read of the entry (the ruling of 2026-11-22 on `last_cause`). Its `window` is read as
/// `session` is: `yes` as yes, any other text as no, and an entry without it is not read (r3 U5.15).
fn agent_of(entry: &Value) -> Option<Agent<AgentConfig>> {
    let config = AgentConfig::parse(&entry["config"].to_string())?;
    let last = match &entry["last"] {
        Value::Null => None,
        record => Some(past_run(record)?),
    };
    let refused_run = match &entry["refused_run"] {
        Value::Null => None,
        run => Some(RunId::parse(run.as_str()?).ok()?),
    };
    let unknown = entry["unknown"]
        .as_array()?
        .iter()
        .map(|u| {
            Some(Unknown {
                run: RunId::parse(u["run"].as_str()?).ok()?,
                effect: EffectId::parse(u["effect"].as_str()?).ok()?,
                day: u32::try_from(u["day"].as_u64()?).ok()?,
                class: Class::parse(u["class"].as_str()?).ok()?,
                kept: kept_of(&u["kept"])?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Agent {
        workflow: config.view.workflow.clone(),
        agent: config.view.agent.clone(),
        state: AgentState::parse(entry["state"].as_str()?).ok()?,
        slots: entry["slots"]
            .as_array()?
            .iter()
            .map(|s| Name::parse(s.as_str()?).ok())
            .collect::<Option<_>>()?,
        session: entry["session"].as_str()? == "yes",
        window: entry["window"].as_str()? == "yes",
        last,
        unknown,
        refused_run,
        last_cause: entry["last_cause"]
            .as_str()
            .and_then(|cause| LastCause::parse(cause).ok()),
        config,
    })
}

/// One entry of the reply of `list_agents` as `agent_of` reads it, with the agreement of its key `agreement` (r3 U7.7):
/// an entry without the key, or with a value that does not read, gives none, and its read goes on (r4 U3.17a).
fn entry_of(entry: &Value) -> Option<(Agent<AgentConfig>, Option<Agreement>)> {
    Some((agent_of(entry)?, Agreement::of(&entry["agreement"])))
}

/// The memory of the limit's part (`model_limit::Memory`) that the read for the agents' rows keeps (r4 U3.17b): a
/// type of its own, so that no memory of the part, the static of the part's own commands in the app among them, can be
/// handed to that read, and the read calls none of those commands. That memory guards the start of a new count, and
/// only an answer of the part itself may move it (the coordinator's log line 18047).
#[derive(Debug, Default)]
struct RowsMemory(Memory);

/// What one list of the agents read of this build's host for the agents' rows (r4 U3.17b): the host, the model words
/// that a yes covers, whether a key is saved (none where its state does not read) and the person's spending limit.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Hosting {
    host: String,
    models: String,
    key: Option<bool>,
    limit: SpendLimit,
}

/// The read of `host` for one list of the agents: none, with nothing sent, where there is no host; else the host and
/// the model words `models` joined by ` and `, and, only where `asks` says that an agent of the list has a runtime
/// that asks a model (`roster::MODEL_RUNTIMES`), the key's state by ONE body `model_key_state` and the count's state by
/// ONE body `model_room`, through `ModelLimit` with `memory`, the rows' own memory, never a memory of the limit's
/// part. Where no agent asks a model nothing is read and the states stand as not read, which no row shows: no row then
/// offers the question.
fn hosting(
    link: &Arc<dyn OwnerLink>,
    memory: &RowsMemory,
    host: Option<String>,
    models: &[&str],
    asks: bool,
) -> Option<Hosting> {
    let host = host?;
    let (key, limit) = if asks {
        let key = match ModelKey::new(Arc::clone(link)).state() {
            KeyState::Saved => Some(true),
            KeyState::None => Some(false),
            KeyState::NotKnown => None,
        };
        let limit = spend_limit(&ModelLimit::new(Arc::clone(link), &memory.0).state(&host));
        (key, limit)
    } else {
        (None, SpendLimit::Unread)
    };
    Some(Hosting {
        host,
        models: models.join(" and "),
        key,
        limit,
    })
}

/// `hosting` with this build's host (`model_key::host`) and the model words of the rows of its table, in the order of
/// `Ruling::allowed()`. With main's table, which holds no row, it is none and sends nothing.
fn hosting_of_build(link: &Arc<dyn OwnerLink>, memory: &RowsMemory, asks: bool) -> Option<Hosting> {
    let ruling = halvo_route::release::ruling();
    let models: Vec<&str> = ruling
        .allowed()
        .iter()
        .filter_map(|tier| ruling.row(*tier))
        .map(|row| row.model())
        .collect();
    hosting(link, memory, crate::model_key::host(), &models, asks)
}

/// The spending limit of the reply of the limit's view: a limit stands in the states `ok`, `near`, `reached` and
/// `none` with the reply's `cap` and `period`; `no_limit` is no limit; every other reply does not read.
fn spend_limit(reply: &LimitReply) -> SpendLimit {
    match reply {
        LimitReply::State {
            state: LimitState::Ok | LimitState::Near | LimitState::Reached | LimitState::None,
            cap: Some(cap),
            period,
            ..
        } => SpendLimit::Stands {
            amount: cap.clone(),
            period: (*period).to_owned(),
        },
        LimitReply::State {
            state: LimitState::NoLimit,
            ..
        } => SpendLimit::NotSet,
        _ => SpendLimit::Unread,
    }
}

/// What the app hands the core of an agent whose stored tier is `tier` and whose entry held `agreement`, where the
/// list read `hosting`: none without it; else its host, models and states, and whether the yes stands for the stored
/// tier, by the gate's ONE function `agreement::stands`.
fn handed(
    hosting: Option<&Hosting>,
    agreement: Option<&Agreement>,
    tier: Tier,
) -> Option<ModelUse> {
    let hosting = hosting?;
    Some(ModelUse {
        host: hosting.host.clone(),
        models: hosting.models.clone(),
        stands: agreement::stands(agreement, tier),
        key: hosting.key,
        limit: hosting.limit.clone(),
    })
}

/// One run of the reply of `agent_history`.
fn history_of(run: &Value) -> Option<RunHistory> {
    let effects = run["effects"]
        .as_array()?
        .iter()
        .map(|e| {
            Some(PastEffect {
                effect: EffectId::parse(e["effect"].as_str()?).ok()?,
                digest: Digest::parse(e["digest"].as_str()?).ok()?,
                class: Class::parse(e["class"].as_str()?).ok()?,
                state: EffectState::parse(e["state"].as_str()?).ok()?,
                cause: match &e["cause"] {
                    Value::Null => None,
                    c => Some(Name::parse(c.as_str()?).ok()?),
                },
                kept: kept_of(&e["kept"])?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RunHistory {
        run: past_run(&run["run"])?,
        effects,
    })
}

/// The rows of `agent_reply` of the run `run` of the agent, read by r3 U9.8's coordinator body `list_replies` through
/// `call`, which sends one coordinator body and gives its result: from `after` null, then with the message id that
/// the reply's `next` names, to `next` null. Each entry of `run` gives its state, in the reply's order: a name of
/// `ReplyState` is `ReplyRow::Read` of it and any other is `ReplyRow::Unreadable`; an entry of another run is dropped.
/// A refusal of the owner passes under its own name. A reply that the reader cannot trust is `no_service`, with no row
/// of the pages before it: one whose `replies` is no list, an entry whose run is no run id or whose state is no text,
/// and a `next` that is neither null nor a message id greater by its bytes than the `after` sent, since the reply
/// ascends by message id; so a store that names the same page again ends the read.
fn list_replies(
    mut call: impl FnMut(&str) -> Result<Value, PortError>,
    workflow: &WorkflowId,
    agent: &AgentId,
    run: &RunId,
) -> Result<Vec<ReplyRow>, PortError> {
    let mut rows = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let reply = call(&format!(
            r#"{{"op":"list_replies","workflow":"{}","agent":"{}","after":{}}}"#,
            workflow.as_str(),
            agent.as_str(),
            after.as_deref().map_or("null".to_owned(), json_str)
        ))?;
        for entry in reply["replies"].as_array().ok_or(PortError::NoService)? {
            let of = entry["run"].as_str().and_then(|r| RunId::parse(r).ok());
            let state = entry["state"].as_str();
            let (Some(of), Some(state)) = (of, state) else {
                return Err(PortError::NoService);
            };
            if of == *run {
                rows.push(
                    halvo_coord::types::ReplyState::parse(state)
                        .map_or(ReplyRow::Unreadable, ReplyRow::Read),
                );
            }
        }
        match reply.get("next") {
            Some(Value::Null) => return Ok(rows),
            Some(Value::String(next)) if after.as_ref().is_none_or(|before| next > before) => {
                after = Some(next.clone());
            }
            _ => return Err(PortError::NoService),
        }
    }
}

impl Owner for Product {
    type Config = AgentConfig;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        let Some(first) = events.first() else {
            return Ok(());
        };
        self.note(events);
        let reply = self.coordinator(&log_text(&first.workflow, &first.agent, events))?;
        match reply["stored"].as_u64() {
            Some(n) if usize::try_from(n) == Ok(events.len()) => Ok(()),
            _ => Err(PortError::NoService),
        }
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError> {
        let workflow = self.workflow_of(agent)?;
        let after = if after == 0 {
            "null".to_owned()
        } else {
            after.to_string()
        };
        let reply = self.coordinator(&format!(
            r#"{{"op":"coord_history","workflow":"{}","agent":"{}","after":{after}}}"#,
            workflow.as_str(),
            agent.as_str()
        ))?;
        let events = reply["records"]
            .as_array()
            .ok_or(PortError::NoService)?
            .iter()
            .map(event_of)
            .collect::<Option<Vec<_>>>()
            .ok_or(PortError::NoService)?;
        let last = reply["last"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or(PortError::NoService)?;
        self.note(&events);
        Ok((events, last))
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        let role = InterfaceRole::claim();
        let mut listed = Vec::new();
        let mut after: Option<halvo_schema_core::WorkflowId> = None;
        loop {
            let id = halvo_schema_core::RequestId::from_random(self.bytes16()?);
            let request = InterfaceRead::workflows(&role, id, after.clone())
                .to_bytes()
                .map_err(|_| PortError::NoService)?;
            let ReplyResult::Workflows(page) = self.interface(&request)? else {
                return Err(PortError::NoService);
            };
            for item in &page.workflows {
                listed.push(workflow_of(&item.workflow).ok_or(PortError::NoService)?);
            }
            match page.next {
                Some(next) if after.as_ref().is_none_or(|before| next > *before) => {
                    after = Some(next);
                }
                Some(_) => return Err(PortError::NoService),
                None => return Ok(listed),
            }
        }
    }

    fn agents(&self) -> Result<Vec<Agent<AgentConfig>>, PortError> {
        let mut listed = Vec::new();
        let mut after = "null".to_owned();
        loop {
            let reply = self.coordinator(&format!(
                r#"{{"op":"list_agents","workflow":null,"agent":null,"after":{after}}}"#
            ))?;
            for entry in reply["agents"].as_array().ok_or(PortError::NoService)? {
                listed.push(entry_of(entry).ok_or(PortError::NoService)?);
            }
            match reply["next"].as_str() {
                Some(next) if json_str(next) != after => after = json_str(next),
                Some(_) => return Err(PortError::NoService),
                None => break,
            }
        }
        let asks = listed
            .iter()
            .any(|(a, _)| halvo_coord::roster::asks_a_model(&a.config));
        let hosting = hosting_of_build(&self.link, &self.rows_memory, asks);
        for (a, agreement) in &mut listed {
            let model = handed(hosting.as_ref(), agreement.as_ref(), a.config.tier());
            a.config.view.set_model(model);
        }
        *lock(&self.agents) = listed
            .iter()
            .map(|(a, agreement)| {
                let kept = KeptAgent {
                    workflow: a.workflow.clone(),
                    tier: a.config.tier(),
                    agreement: agreement.clone(),
                };
                (a.agent.clone(), kept)
            })
            .collect();
        Ok(listed.into_iter().map(|(a, _)| a).collect())
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        let text = body_text(body, &self.runs).ok_or(PortError::NoService)?;
        self.coordinator(&text)
    }

    fn agent_history(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        runs: u32,
    ) -> Result<Vec<RunHistory>, PortError> {
        let reply = self.coordinator(&format!(
            r#"{{"op":"agent_history","workflow":"{}","agent":"{}","runs":{runs}}}"#,
            workflow.as_str(),
            agent.as_str()
        ))?;
        reply["runs"]
            .as_array()
            .ok_or(PortError::NoService)?
            .iter()
            .map(history_of)
            .collect::<Option<Vec<_>>>()
            .ok_or(PortError::NoService)
    }

    /// The rows of `agent_reply` of the run `run`, read by `list_replies` with the body `list_replies` of r3 U9.8, a
    /// `coordinator_write` on the app's own owner stream.
    fn replies(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        run: &RunId,
    ) -> Result<Vec<ReplyRow>, PortError> {
        list_replies(|body| self.coordinator(body), workflow, agent, run)
    }

    fn overlaps(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        slots: Option<&[Slot]>,
    ) -> Result<Overlaps, PortError> {
        let slots = slots.map_or("null".to_owned(), slots_text);
        let reply = self.coordinator(&format!(
            r#"{{"op":"list_overlaps","workflow":"{}","agent":"{}","slots":{slots}}}"#,
            workflow.as_str(),
            agent.as_str()
        ))?;
        let entries = reply["overlaps"]
            .as_array()
            .ok_or(PortError::NoService)?
            .iter()
            .map(|o| {
                Some(Overlap {
                    workflow: WorkflowId::parse(o["workflow"].as_str()?).ok()?,
                    agent: AgentId::parse(o["agent"].as_str()?).ok()?,
                    senders: o["senders"]
                        .as_array()?
                        .iter()
                        .map(|s| s.as_str().map(str::to_owned))
                        .collect::<Option<_>>()?,
                })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(PortError::NoService)?;
        Ok(Overlaps {
            entries,
            more: reply["more"].as_str() == Some("yes"),
        })
    }

    fn delete(&self, workflow: &WorkflowId) -> Result<(), PortError> {
        let id = halvo_schema_core::RequestId::from_random(self.bytes16()?);
        let workflow = halvo_schema_core::WorkflowId::parse(workflow.as_str())
            .map_err(|_| PortError::NoService)?;
        let request = InterfaceDecision::delete(&InterfaceRole::claim(), id, workflow)
            .to_bytes()
            .map_err(|_| PortError::NoService)?;
        match self.interface(&request)? {
            ReplyResult::Null => Ok(()),
            _ => Err(PortError::NoService),
        }
    }
}

/// The library's run id of the coordinator's.
fn wire_run(run: &RunId) -> Result<halvo_schema::executable::text::RunId, PortError> {
    halvo_schema::executable::text::RunId::parse(run.as_str()).map_err(|_| PortError::NoService)
}

fn call_error(error: halvo_agent_client::CallError) -> PortError {
    PortError::Refused(name(error.as_str()))
}

impl Runs for Product {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.remove_checkpoints_once();
        let wire = halvo_agent_client::RunSpec {
            run: wire_run(&spec.run)?,
            agent: halvo_schema::AgentId::parse(spec.agent.as_str())
                .map_err(|_| PortError::NoService)?,
            workflow: halvo_schema::WorkflowId::parse(spec.workflow.as_str())
                .map_err(|_| PortError::NoService)?,
            mode: halvo_schema::executable::names::Mode::parse(spec.mode.name())
                .map_err(|_| PortError::NoService)?,
            browser: None,
            model: self.places.model.clone(),
            headed: spec.headed,
            utc_offset_s: zone_offset_s(SystemTime::now()).ok_or(PortError::NoService)?,
            hosted: agreement::hosted(lock(&self.agents).get(&spec.agent)),
        };
        lock(&self.runs).insert(
            spec.run.clone(),
            (spec.workflow.clone(), spec.agent.clone()),
        );
        self.supervisor
            .start(wire)
            .map_err(|e| call_error(e.call_error()))
    }

    /// Hands the answers to the confirmation that waits, takes the decision that the library makes of them with its
    /// approval ids, and lets the library issue it only when `record` accepted it; else the library rejects each
    /// effect and the error of `record` comes back. `ended` where no confirmation of the run waits.
    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        let wire = answers
            .iter()
            .map(|a| {
                Some((
                    WireEffect::parse(a.effect.as_str()).ok()?,
                    EffectReply::parse(a.answer.name()).ok()?,
                ))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(PortError::NoService)?;
        let Some(to) = lock(&self.waits.confirms).remove(run) else {
            return Err(PortError::Refused(name("ended")));
        };
        let (decided, decision) = mpsc::channel();
        let (verdict, issued) = mpsc::channel();
        lock(&self.waits.issues).insert(run.clone(), (decided, issued));
        if to.send(wire).is_err() {
            lock(&self.waits.issues).remove(run);
            return Err(PortError::Refused(name("ended")));
        }
        let Ok(made) = decision.recv_timeout(ISSUE_WAIT) else {
            lock(&self.waits.issues).remove(run);
            return Err(PortError::NoService);
        };
        let Some(made) = decision_of(&made) else {
            let _ = verdict.send(false);
            return Err(PortError::NoService);
        };
        match record(&made) {
            Ok(()) => {
                let _ = verdict.send(true);
                Ok(())
            }
            Err(e) => {
                let _ = verdict.send(false);
                Err(e)
            }
        }
    }

    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        let text = format!(r#"{{"v":1,"outcome":"{}","secret":null}}"#, result.name());
        let result = halvo_schema::executable::frame::HandbackResult::parse(text.as_bytes())
            .map_err(|_| PortError::NoService)?;
        self.supervisor
            .answer_handback(&wire_run(run)?, result)
            .map_err(call_error)
    }

    /// Hands the answer to the second step of a sign-in that waits; `ended` where none of the run waits, so that an
    /// error other than `ended` never follows an answer that the call took.
    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        let Some(to) = lock(&self.waits.binds).remove(run) else {
            return Err(PortError::Refused(name("ended")));
        };
        to.send(result)
            .map_err(|_| PortError::Refused(name("ended")))
    }

    /// The frame `cancel`, after the run's waits are ended with reject and decline.
    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        self.waits.release(run);
        self.supervisor.cancel(&wire_run(run)?).map_err(call_error)
    }

    /// The end of the run's tree, after the run's waits are ended with reject and decline.
    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.waits.release(run);
        self.supervisor.kill(&wire_run(run)?).map_err(call_error)
    }

    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        let answers = match lock(&self.reaped).take() {
            Some(answers) => answers,
            None => self.supervisor.reap().map_err(|_| PortError::NoService)?,
        };
        answers
            .iter()
            .map(|a| {
                let run = RunId::parse(a.run.as_str()).map_err(|_| PortError::NoService)?;
                let reaped = match a.answer {
                    halvo_agent_client::Reaped::Gone => Reaped::Gone,
                    halvo_agent_client::Reaped::Alive => Reaped::Alive,
                };
                Ok((run, reaped))
            })
            .collect()
    }
}

/// One request `defer` of the capture service at each call of the core (at a run's start and at each tick while a
/// run is live), with the core's deadline as it is, turned from milliseconds into the nanoseconds of
/// `Client::defer`; a deadline beyond an `i64` goes as the largest, which the service caps. The port keeps nothing
/// between calls, so each tick renews the deadline. It answers `Ok` where the service names a deadline in force, and
/// `no_service` for every error of the call and where the service holds no deadline, as a failed relay call answers:
/// the core then stops nothing and asks again at its next tick.
impl Analysis for Product {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        let nanos = i64::try_from(u128::from(until) * 1_000_000).unwrap_or(i64::MAX);
        match (self.defer)(nanos) {
            Ok(Some(_)) => Ok(()),
            Ok(None) | Err(_) => Err(PortError::NoService),
        }
    }
}

/// The offset of the Mac's zone from UTC at `at`, in whole seconds east of UTC, as the system's zone gives it.
#[cfg(target_os = "macos")]
pub fn zone_offset_s(at: SystemTime) -> Option<i32> {
    use objc2_foundation::{NSDate, NSTimeZone};
    let seconds = at.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    NSTimeZone::resetSystemTimeZone();
    let zone = NSTimeZone::localTimeZone();
    let date = NSDate::dateWithTimeIntervalSince1970(seconds);
    i32::try_from(zone.secondsFromGMTForDate(&date)).ok()
}

/// The offset of the Windows zone from UTC at `at`, in whole seconds east of UTC: the local time that the system
/// gives for the instant in the zone in force, less the instant's time in UTC.
#[cfg(windows)]
pub fn zone_offset_s(at: SystemTime) -> Option<i32> {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    const FROM_1601_TO_1970_S: u64 = 11_644_473_600;
    let ticks = at
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs()
        .checked_add(FROM_1601_TO_1970_S)?
        .checked_mul(10_000_000)?;
    let file = FILETIME {
        dwLowDateTime: u32::try_from(ticks & u64::from(u32::MAX)).ok()?,
        dwHighDateTime: u32::try_from(ticks >> 32).ok()?,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: both pointers come from references to values on this stack that live for the call.
    unsafe { FileTimeToSystemTime(&file, &mut utc) }.ok()?;
    // SAFETY: no zone is given, so the system takes the one in force; both pointers come from references to
    // values on this stack that live for the call.
    unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.ok()?;
    let wall = |t: SYSTEMTIME| Wall {
        year: t.wYear,
        month: t.wMonth,
        day: t.wDay,
        hour: t.wHour,
        minute: t.wMinute,
        second: t.wSecond,
    };
    Some(offset_between(wall(utc), wall(local)))
}

/// A calendar date and a time of day to the second, as a `SYSTEMTIME` of Windows holds them.
#[cfg(any(test, windows))]
#[derive(Clone, Copy, Debug)]
struct Wall {
    year: u16,
    month: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
}

/// The offset of a zone from UTC in whole seconds east of UTC, from the time in UTC and the local time of one
/// instant: the two dates differ by one day at most.
#[cfg(any(test, windows))]
fn offset_between(utc: Wall, local: Wall) -> i32 {
    let date = |w: Wall| (w.year, w.month, w.day);
    let of_day =
        |w: Wall| i32::from(w.hour) * 3_600 + i32::from(w.minute) * 60 + i32::from(w.second);
    let days = match date(local).cmp(&date(utc)) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    days * 86_400 + of_day(local) - of_day(utc)
}

impl Clock for Product {
    fn now(&self) -> Result<u64, PortError> {
        self.marks.send_owed();
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| PortError::NoService)?;
        u64::try_from(since.as_millis()).map_err(|_| PortError::NoService)
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        let instant = UNIX_EPOCH + Duration::from_millis(at);
        let offset = zone_offset_s(instant).ok_or(PortError::NoService)?;
        let local = i64::try_from(at / 1000).map_err(|_| PortError::NoService)? + i64::from(offset);
        u32::try_from(local.div_euclid(86_400)).map_err(|_| PortError::NoService)
    }
}

impl Rng for Product {
    /// Sixteen bytes of the system's generator (`random16`).
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        random16()
    }
}

/// Sixteen bytes of the system's generator: `/dev/urandom` on a Mac, the system's preferred generator of
/// `BCryptGenRandom` on Windows.
pub(crate) fn random16() -> Result<[u8; 16], PortError> {
    let mut bytes = [0u8; 16];
    #[cfg(target_os = "macos")]
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| PortError::NoService)?;
    #[cfg(windows)]
    {
        use windows::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        // SAFETY: the buffer is a live slice that the call writes within its length; no algorithm handle is
        // given, which the flag for the system's preferred generator asks.
        unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }
            .ok()
            .map_err(|_| PortError::NoService)?;
    }
    Ok(bytes)
}

impl Policy for Product {
    type Config = AgentConfig;

    /// The row of the table of halvo-policy for the agent's autonomy, allowed classes and caps; a `local_edit` is
    /// asked with no restorable copy known, which never gives `run` where the table would not.
    fn gives_run(&self, config: &AgentConfig, class: Class) -> Result<bool, PortError> {
        let class = halvo_schema::executable::names::ActionClass::parse(class.name())
            .map_err(|_| PortError::NoService)?;
        let row = halvo_policy::decide_row(
            config.typed.autonomy,
            class,
            &config.typed.allow,
            &config.typed.caps,
            halvo_policy::Checkpoint::No,
        );
        Ok(row == halvo_policy::Decision::Run)
    }
}

impl Senders for Product {
    fn read(&self, text: &str) -> Result<Reading, PortError> {
        use halvo_schema::executable::SendersError;
        let n = |at: std::num::NonZeroUsize| {
            u32::try_from(at.get())
                .ok()
                .and_then(NonZeroU32::new)
                .ok_or(PortError::NoService)
        };
        Ok(match halvo_schema::executable::reply_senders(text) {
            Ok(_) => Reading::Taken,
            Err(SendersError::Empty(at)) => Reading::Refused(Refused::Empty(n(at)?)),
            Err(SendersError::NotAscii(at)) => Reading::Refused(Refused::NotAscii(n(at)?)),
            Err(SendersError::NotAnAddress(at)) => Reading::Refused(Refused::NotAnAddress(n(at)?)),
            Err(SendersError::Twice { earlier, later }) => Reading::Refused(Refused::Twice {
                earlier: n(earlier)?,
                later: n(later)?,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agreement::Triple;

    /// A link that keeps each request that reaches it and answers each with a null result.
    struct Kept(Mutex<Vec<Vec<u8>>>);

    impl OwnerLink for Kept {
        fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
            lock(&self.0).push(request.to_vec());
            Ok(br#"{"v":1,"id":null,"ok":null}"#.to_vec())
        }
    }

    /// A runner's `effect` request whose body holds `record`.
    fn effect(record: &str) -> Vec<u8> {
        format!(
            r#"{{"v":1,"id":"{}","request":"runner","body":{{"op":"effect","agent":"{}","run":"{}","record":{record}}}}}"#,
            "01".repeat(16),
            "a1".repeat(16),
            "d1".repeat(16)
        )
        .into_bytes()
    }

    /// The key `error` of a reply as its text holds it, or none: `decode_reply` takes no name `approval`.
    fn error_of(reply: &[u8]) -> Option<String> {
        let reply: Value = serde_json::from_slice(reply).ok()?;
        reply.get("error")?.as_str().map(str::to_owned)
    }

    /// U5.4a round 1, item (1): the guard on the supervisor's link refuses a record of `effect` that its reader
    /// cannot read, and every such record while no hub is attached: neither reaches the owner, and its sender gets
    /// the guard's refusal `approval`.
    #[test]
    fn the_links_guard_refuses_what_it_cannot_read_and_everything_while_no_hub_is_attached() {
        let kept = Arc::new(Kept(Mutex::new(Vec::new())));
        let attached: Arc<OnceLock<Weak<ProductHub>>> = Arc::default();
        let guarded = Guarded {
            link: kept.clone(),
            hub: Arc::clone(&attached),
        };
        let record = |class: &str| {
            format!(
                r#"{{"v":1,"workflow":"{}","agent":"{}","run":"{}","effect":"{}","seq":1,"day":20778,"at":1,"state":"approved","class":"{class}","digest":"{}","kept":[],"fields":0,"approval":"{}","cause":null}}"#,
                "f1".repeat(16),
                "a1".repeat(16),
                "d1".repeat(16),
                "e1".repeat(16),
                "0a".repeat(32),
                "c1".repeat(16)
            )
        };

        let unattached = guarded.request(&effect(&record("send"))).unwrap();
        assert_eq!(error_of(&unattached).as_deref(), Some("approval"));
        assert!(
            lock(&kept.0).is_empty(),
            "a record went on while no hub was attached"
        );

        let fails: Calls = Calls {
            owner_stream: Arc::new(|| Err(ClientError::Unsupported)),
            relay: Arc::new(|_: &[u8]| Err(ClientError::Unsupported)),
            mark: Arc::new(|_| Err(ClientError::Unsupported)),
            defer: Arc::new(|_| Err(ClientError::Unsupported)),
        };
        let places = Places {
            data_dir: PathBuf::from("/nowhere/halvo-u54a"),
            runner: PathBuf::from("/nowhere/halvo-runner"),
            model: None,
        };
        let hub = hub(fails, places, Box::new(|_| {}), Arc::new(Unseen));
        assert!(attached.set(Arc::downgrade(&hub)).is_ok());
        let unread = guarded.request(&effect(&record("no_such_class"))).unwrap();
        assert_eq!(error_of(&unread).as_deref(), Some("approval"));
        assert!(
            lock(&kept.0).is_empty(),
            "a record that the guard cannot read went on"
        );
    }

    /// A hub over the product's ports whose request `defer` is `defer` and whose every other call fails; its places
    /// name a data directory of this test's own, which no code opens.
    fn deferring(defer: DeferCall) -> Arc<ProductHub> {
        let calls = Calls {
            owner_stream: Arc::new(|| Err(ClientError::Unsupported)),
            relay: Arc::new(|_: &[u8]| Err(ClientError::Unsupported)),
            mark: Arc::new(|_| Err(ClientError::Unsupported)),
            defer,
        };
        let places = Places {
            data_dir: PathBuf::from("/nowhere/halvo-u54c"),
            runner: PathBuf::from("/nowhere/halvo-runner"),
            model: None,
        };
        hub(calls, places, Box::new(|_| {}), Arc::new(Unseen))
    }

    /// U5.4c, sentence 1: each call of the port `Analysis::defer` sends one request `defer` with the core's time in
    /// nanoseconds since 1970, and answers `Ok` where the service names a deadline in force.
    #[test]
    fn the_port_defer_sends_each_deadline_of_the_core_in_nanoseconds() {
        let sent: Arc<Mutex<Vec<i64>>> = Arc::default();
        let kept = Arc::clone(&sent);
        let hub = deferring(Arc::new(move |until| {
            lock(&kept).push(until);
            Ok(Some(until))
        }));
        let ports = hub.coordinator().ports();

        assert_eq!(ports.defer(1_800_000_000_000), Ok(()));
        assert_eq!(ports.defer(1_800_000_003_000), Ok(()));
        assert_eq!(
            *lock(&sent),
            [1_800_000_000_000_000_000, 1_800_000_003_000_000_000]
        );
    }

    /// U5.4c, sentence 2: an error reply of v1, no answer in time and the answer that no deadline is in force are each
    /// `no_service` to the core, one request a call.
    #[test]
    fn the_port_defer_answers_no_service_for_a_failed_request_and_for_no_deadline() {
        let answers = Mutex::new(std::collections::VecDeque::from([
            Err(ClientError::Daemon(halvo_control::proto::ErrorReply::new(
                halvo_control::proto::ErrorCode::Busy,
                "busy",
            ))),
            Err(ClientError::Deadline { seconds: 5 }),
            Ok(None),
        ]));
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let hub = deferring(Arc::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            lock(&answers).pop_front().unwrap_or(Ok(None))
        }));
        let ports = hub.coordinator().ports();

        assert_eq!(ports.defer(1_800_000_000_000), Err(PortError::NoService));
        assert_eq!(ports.defer(1_800_000_003_000), Err(PortError::NoService));
        assert_eq!(ports.defer(1_800_000_006_000), Err(PortError::NoService));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    /// U2.4h, sentence 5 (I6.1 with its revision of 2026-11-22, I7.1; ruling RD-73 4.5): the reader of
    /// `list_agents` takes `refused_run` of an entry, null as none, and the reader of stored events reads back the
    /// `hold_set` and the `hold_cleared` of `sign_in_refused` that `log_text` writes, each with its run.
    #[test]
    fn the_readers_take_the_refused_run_and_read_back_the_hold_of_a_refused_sign_in() {
        let workflow = WorkflowId::parse(&"f1".repeat(16)).unwrap();
        let agent = AgentId::parse(&"a1".repeat(16)).unwrap();
        let run = RunId::parse(&"d1".repeat(16)).unwrap();
        let entry = |refused: &str| -> Value {
            serde_json::from_str(&format!(
                r#"{{"config":{{"v":1,"workflow":"{workflow}","agent":"{agent}","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{{"untrusted":"yes","private":"yes","external":"yes"}},"limits":{{"steps":20,"wall_s":120,"model_tokens":0}},"budget":null,"permissions":[],"runtime":"web","playbook":null}},"state":"active","slots":[],"session":"yes","last":null,"unknown":[],"refused_run":{refused},"reply_unknown":"no","refused_slots":[],"window":"yes"}}"#
            ))
            .unwrap()
        };
        assert_eq!(
            agent_of(&entry(&format!(r#""{run}""#))).map(|a| a.refused_run),
            Some(Some(run.clone()))
        );
        assert_eq!(agent_of(&entry("null")).map(|a| a.refused_run), Some(None));
        let event = |seq: u32, what: What| Event {
            workflow: workflow.clone(),
            agent: agent.clone(),
            seq: NonZeroU32::new(seq).unwrap(),
            day: 20_778,
            at: 1,
            what,
        };
        let events = vec![
            event(
                1,
                What::HoldSet {
                    cause: HoldSetCause::SignInRefused { run: run.clone() },
                },
            ),
            event(
                2,
                What::HoldCleared {
                    cause: Cleared::SignInRefused { run: run.clone() },
                },
            ),
        ];
        let body: Value = serde_json::from_str(&log_text(&workflow, &agent, &events)).unwrap();
        let read: Vec<Option<Event>> = body["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(event_of)
            .collect();
        assert_eq!(read, events.into_iter().map(Some).collect::<Vec<_>>());
    }

    /// r4 U5.4r, sentence 1: the read of `Owner::replies` sends r3 U9.8's body `list_replies` with the keys `op`,
    /// `workflow`, `agent` and `after` in that order, `after` null and then the `next` of the page before, to `next`
    /// null, and keeps the states of the asked run in the reply's order: a state that the core lacks is `Unreadable`,
    /// and an entry of another run is dropped.
    #[test]
    fn the_read_of_replies_pages_by_after_and_keeps_the_states_of_the_asked_run() {
        let workflow = WorkflowId::parse(&"f1".repeat(16)).unwrap();
        let agent = AgentId::parse(&"a1".repeat(16)).unwrap();
        let run = RunId::parse(&"d1".repeat(16)).unwrap();
        let other = "d2".repeat(16);
        let mut pages = std::collections::VecDeque::from([
            format!(
                r#"{{"replies":[{{"message":"m1","run":"{run}","state":"committed"}},{{"message":"m2","run":"{other}","state":"committed"}},{{"message":"m3","run":"{run}","state":"unknown"}}],"next":"m3"}}"#
            ),
            format!(
                r#"{{"replies":[{{"message":"m4","run":"{run}","state":"sent"}}],"next":null}}"#
            ),
        ]);
        let mut sent = Vec::new();
        let rows = list_replies(
            |body| {
                sent.push(body.to_owned());
                let page = pages.pop_front().ok_or(PortError::NoService)?;
                Ok(serde_json::from_str(&page).unwrap())
            },
            &workflow,
            &agent,
            &run,
        );
        assert_eq!(
            sent,
            [
                r#"{"op":"list_replies","workflow":"f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1","agent":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","after":null}"#,
                r#"{"op":"list_replies","workflow":"f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1","agent":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","after":"m3"}"#,
            ]
        );
        assert_eq!(
            rows,
            Ok(vec![
                ReplyRow::Read(halvo_coord::types::ReplyState::Committed),
                ReplyRow::Read(halvo_coord::types::ReplyState::Unknown),
                ReplyRow::Unreadable,
            ])
        );
    }

    /// r4 U5.4r, sentence 2: no answer is never an empty list. The owner's refusal passes under its own name after one
    /// call, a call that fails is `no_service`, and so is a reply that the reader cannot trust: one with no list
    /// `replies`, and a `next` that is not greater than the `after` sent, which answers no row of the page before it.
    #[test]
    fn the_read_of_replies_passes_a_refusal_and_answers_no_service_for_a_reply_that_it_cannot_trust(
    ) {
        let workflow = WorkflowId::parse(&"f1".repeat(16)).unwrap();
        let agent = AgentId::parse(&"a1".repeat(16)).unwrap();
        let run = RunId::parse(&"d1".repeat(16)).unwrap();
        let read = |answers: Vec<Result<Value, PortError>>| {
            let mut answers = std::collections::VecDeque::from(answers);
            let mut calls = 0;
            let rows = list_replies(
                |_| {
                    calls += 1;
                    answers.pop_front().unwrap_or(Err(PortError::NoService))
                },
                &workflow,
                &agent,
                &run,
            );
            (rows, calls)
        };
        let page =
            |text: &str| -> Result<Value, PortError> { Ok(serde_json::from_str(text).unwrap()) };
        assert_eq!(
            read(vec![Err(PortError::Refused(name("no_workflow")))]),
            (Err(PortError::Refused(name("no_workflow"))), 1)
        );
        assert_eq!(
            read(vec![Err(PortError::Refused(name("binding")))]),
            (Err(PortError::Refused(name("binding"))), 1)
        );
        assert_eq!(
            read(vec![Err(PortError::NoService)]),
            (Err(PortError::NoService), 1)
        );
        assert_eq!(
            read(vec![page(r#"{"next":null}"#)]),
            (Err(PortError::NoService), 1)
        );
        let first = format!(
            r#"{{"replies":[{{"message":"m1","run":"{run}","state":"committed"}},{{"message":"m2","run":"{run}","state":"lost"}}],"next":"m2"}}"#
        );
        assert_eq!(
            read(vec![page(&first), page(r#"{"replies":[],"next":"m2"}"#)]),
            (Err(PortError::NoService), 2)
        );
    }

    /// r4 U2.4y, sentence 3: the body `drop_sessions` goes out as r3 U5.2c built it, with the keys `op`,
    /// `workflow` and `agent` in that order and nothing else.
    #[test]
    fn the_body_drop_sessions_is_written_as_r3_built_it() {
        let body = Body::DropSessions {
            workflow: WorkflowId::parse(&"f1".repeat(16)).unwrap(),
            agent: AgentId::parse(&"a1".repeat(16)).unwrap(),
        };
        assert_eq!(
            body_text(&body, &RunsOf::default()).as_deref(),
            Some(
                r#"{"op":"drop_sessions","workflow":"f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1","agent":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1"}"#
            )
        );
    }

    /// r4 U3.17a sentence 6: the way back goes out as r3 U7.7's body `set_model` with the tier `on_device` and the
    /// agreement null, and a yes with the ceiling tier, the highest that the ruling allows, and one triple for each
    /// hosted tier that it allows up to it, from the table's rows: the keys in the order op, workflow, agent, tier,
    /// agreement. Each parses as the schema's own `SetModel` and holds those values. No allowed tier gives none, and
    /// so does main's ruling.
    #[test]
    fn the_yes_and_the_way_back_are_written_as_the_set_model_of_r3() {
        use halvo_schema::executable::body::SetModel;
        use halvo_schema::executable::names::ModelTier;
        let (w, a) = ("f1".repeat(16), "a1".repeat(16));
        let workflow = WorkflowId::parse(&w).unwrap();
        let agent = AgentId::parse(&a).unwrap();
        let head = format!(r#"{{"op":"set_model","workflow":"{w}","agent":"{a}""#);
        let small = [
            "low_cost_hosted",
            "https://hosted-model.test",
            "test-model-small",
        ];
        let large = ["frontier", "https://hosted-model.test", "test-model-large"];
        let triple = |[tier, origin, model]: [&str; 3]| {
            format!(r#"{{"tier":"{tier}","origin":"{origin}","model":"{model}"}}"#)
        };
        let back = Body::WithdrawHosted {
            workflow: workflow.clone(),
            agent: agent.clone(),
        };
        let texts = [
            (
                body_text(&back, &RunsOf::default()),
                format!(r#"{head},"tier":"on_device","agreement":null}}"#),
                ModelTier::OnDevice,
                None,
            ),
            (
                yes_text(head.clone(), &[large]),
                format!(
                    r#"{head},"tier":"frontier","agreement":[{}]}}"#,
                    triple(large)
                ),
                ModelTier::Frontier,
                Some(vec![large]),
            ),
            (
                yes_text(head.clone(), &[small, large]),
                format!(
                    r#"{head},"tier":"frontier","agreement":[{},{}]}}"#,
                    triple(small),
                    triple(large)
                ),
                ModelTier::Frontier,
                Some(vec![small, large]),
            ),
        ];
        for (text, want, tier, hosts) in texts {
            assert_eq!(text.as_deref(), Some(want.as_str()));
            let body = SetModel::parse(want.as_bytes()).expect("the schema's own SetModel");
            assert_eq!(
                (body.workflow.as_str(), body.agent.as_str(), body.tier),
                (w.as_str(), a.as_str(), tier)
            );
            let read: Option<Vec<[&str; 3]>> = body.agreement.as_ref().map(|hosts| {
                hosts
                    .iter()
                    .map(|h| [h.tier.as_str(), h.origin.as_str(), h.model.as_str()])
                    .collect()
            });
            assert_eq!(read, hosts, "{want}");
        }
        assert_eq!(yes_text(head, &[]), None, "no allowed tier");
        assert_eq!(
            body_text(&Body::AgreeHosted { workflow, agent }, &RunsOf::default()),
            None,
            "main's ruling allows no hosted tier"
        );
    }

    /// `list_agents` takes `last_cause` of an entry: a name of the core's list as its cause, null as none, and a name
    /// of the store's list that the core's lacks as none too, with the entry still read (I6.1).
    #[test]
    fn the_reader_takes_the_last_cause_and_reads_a_cause_that_the_core_lacks_as_none() {
        let workflow = WorkflowId::parse(&"f1".repeat(16)).unwrap();
        let agent = AgentId::parse(&"a1".repeat(16)).unwrap();
        let entry = |cause: &str| -> Value {
            serde_json::from_str(&format!(
                r#"{{"config":{{"v":1,"workflow":"{workflow}","agent":"{agent}","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{{"untrusted":"yes","private":"yes","external":"yes"}},"limits":{{"steps":20,"wall_s":120,"model_tokens":0}},"budget":null,"permissions":[],"runtime":"web","playbook":null}},"state":"active","slots":[],"session":"yes","last":null,"unknown":[],"refused_run":null,"reply_unknown":"no","refused_slots":[],"last_cause":{cause},"window":"yes"}}"#
            ))
            .unwrap()
        };
        assert_eq!(
            agent_of(&entry(r#""no_browser""#)).map(|a| a.last_cause),
            Some(Some(LastCause::NoBrowser))
        );
        assert_eq!(agent_of(&entry("null")).map(|a| a.last_cause), Some(None));
        assert_eq!(
            agent_of(&entry(r#""wall""#)).map(|a| a.last_cause),
            Some(None)
        );
    }

    /// r4 U3.17a sentence 2: `list_agents` takes an entry's `agreement` as r3 U7.7 hands it: null as none, and the
    /// ceiling tier with one triple or with two as that agreement; an entry without the key, or with a value of another
    /// shape, gives none and is still read.
    #[test]
    fn the_reader_takes_the_agreement_of_an_entry_and_reads_any_other_value_as_none() {
        let workflow = WorkflowId::parse(&"f1".repeat(16)).unwrap();
        let agent = AgentId::parse(&"a1".repeat(16)).unwrap();
        let entry = |agreement: &str| -> Value {
            serde_json::from_str(&format!(
                r#"{{"config":{{"v":1,"workflow":"{workflow}","agent":"{agent}","rev":1,"category":"utility_bill_payment","tier":"frontier","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{{"untrusted":"yes","private":"yes","external":"yes"}},"limits":{{"steps":20,"wall_s":120,"model_tokens":0}},"budget":null,"permissions":[],"runtime":"web","playbook":null}},"state":"active","slots":[],"session":"yes","last":null,"unknown":[],"refused_run":null,"reply_unknown":"no","refused_slots":[],"last_cause":null,"window":"yes"{agreement}}}"#
            ))
            .unwrap()
        };
        let small = r#"{"tier":"low_cost_hosted","origin":"https://hosted-model.test","model":"test-model-small"}"#;
        let large = r#"{"tier":"frontier","origin":"https://hosted-model.test","model":"test-model-large"}"#;
        let triple = |tier, model: &str| Triple {
            tier,
            origin: "https://hosted-model.test".to_owned(),
            model: model.to_owned(),
        };
        let rows = [
            (r#","agreement":null"#.to_owned(), None),
            (
                format!(r#","agreement":{{"tier":"low_cost_hosted","hosts":[{small}]}}"#),
                Some(Agreement {
                    ceiling: Tier::LowCostHosted,
                    hosts: vec![triple(Tier::LowCostHosted, "test-model-small")],
                }),
            ),
            (
                format!(r#","agreement":{{"tier":"frontier","hosts":[{small},{large}]}}"#),
                Some(Agreement {
                    ceiling: Tier::Frontier,
                    hosts: vec![
                        triple(Tier::LowCostHosted, "test-model-small"),
                        triple(Tier::Frontier, "test-model-large"),
                    ],
                }),
            ),
            (String::new(), None),
            (format!(r#","agreement":[{large}]"#), None),
        ];
        for (text, want) in rows {
            let read = entry_of(&entry(&text));
            assert!(read.is_some(), "the entry with {text:?} is read");
            assert_eq!(read.map(|(_, agreement)| agreement), Some(want), "{text:?}");
        }
    }

    /// `list_agents` takes `browser_start` and `no_channel` of an entry's `last_cause` as the causes of those names
    /// (I6.1).
    #[test]
    fn the_reader_takes_browser_start_and_no_channel_as_causes_of_their_names() {
        let workflow = WorkflowId::parse(&"f1".repeat(16)).unwrap();
        let agent = AgentId::parse(&"a1".repeat(16)).unwrap();
        let entry = |cause: &str| -> Value {
            serde_json::from_str(&format!(
                r#"{{"config":{{"v":1,"workflow":"{workflow}","agent":"{agent}","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{{"untrusted":"yes","private":"yes","external":"yes"}},"limits":{{"steps":20,"wall_s":120,"model_tokens":0}},"budget":null,"permissions":[],"runtime":"web","playbook":null}},"state":"active","slots":[],"session":"yes","last":null,"unknown":[],"refused_run":null,"reply_unknown":"no","refused_slots":[],"last_cause":"{cause}","window":"yes"}}"#
            ))
            .unwrap()
        };
        for name in ["browser_start", "no_channel"] {
            assert_eq!(
                agent_of(&entry(name)).map(|a| a.last_cause.map(LastCause::name)),
                Some(Some(name)),
                "{name}"
            );
        }
    }

    /// U5.4w, sentence 3: the Windows zone's offset is the local time of an instant less its time in UTC, in whole
    /// seconds east of UTC, also where the local date is the day before the date in UTC.
    #[test]
    fn the_offset_of_the_windows_zone_is_the_local_time_less_the_time_in_utc() {
        let at = |year, month, day, hour, minute| Wall {
            year,
            month,
            day,
            hour,
            minute,
            second: 0,
        };
        let noon = at(2026, 11, 22, 12, 0);
        assert_eq!(offset_between(noon, at(2026, 11, 22, 13, 0)), 3_600);
        assert_eq!(offset_between(noon, at(2026, 11, 22, 7, 0)), -18_000);
        assert_eq!(offset_between(noon, at(2026, 11, 22, 17, 30)), 19_800);
        assert_eq!(
            offset_between(at(2026, 1, 1, 2, 0), at(2025, 12, 31, 21, 0)),
            -18_000
        );
    }

    /// A dialog that is never on screen and answers `reject`: no native confirmation is answered.
    struct Unseen;

    impl Dialog for Unseen {
        fn show(
            &self,
            _: &crate::Card,
            _: Duration,
            _: &dyn Fn(),
        ) -> halvo_coord::items::ConfirmAnswer {
            halvo_coord::items::ConfirmAnswer::Reject
        }
    }

    /// The configuration of an agent of `tier` with the runtime `web`, as an entry of `list_agents` holds it.
    fn model_config(tier: &str) -> AgentConfig {
        AgentConfig::parse(&format!(
            r#"{{"v":1,"workflow":"{}","agent":"{}","rev":1,"category":"utility_bill_payment","tier":"{tier}","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{{"untrusted":"yes","private":"yes","external":"yes"}},"limits":{{"steps":20,"wall_s":120,"model_tokens":0}},"budget":null,"permissions":[],"runtime":"web","playbook":null}}"#,
            "f1".repeat(16),
            "a1".repeat(16)
        ))
        .expect("a configuration of the agent store")
    }

    /// r4 U3.17b sentence 1: over this build's table of hosts (`release::ruling()`, which holds no row on main), the
    /// read for a list with an agent whose runtime asks a model sends nothing and hands no model, so that no row offers
    /// either control, whatever the agent's tier and agreement, and no card shows a line of a host.
    #[test]
    fn over_the_table_of_this_build_a_list_reads_nothing_and_offers_no_ai_model() {
        let kept = Arc::new(Kept(Mutex::new(Vec::new())));
        let link: Arc<dyn OwnerLink> = kept.clone();
        let memory = RowsMemory::default();
        let hosting = hosting_of_build(&link, &memory, true);
        assert_eq!(hosting, None);
        assert!(lock(&kept.0).is_empty(), "no request");
        let both = Agreement::of(&serde_json::json!({
            "tier": "frontier",
            "hosts": [
                {"tier": "low_cost_hosted", "origin": "https://hosted-model.test", "model": "test-model-small"},
                {"tier": "frontier", "origin": "https://hosted-model.test", "model": "test-model-large"}
            ]
        }));
        assert!(both.is_some());
        for (tier, agreement) in [("on_device", None), ("frontier", both.as_ref())] {
            let mut config = model_config(tier);
            assert_eq!(config.runtime(), Runtime::Web);
            config
                .view
                .set_model(handed(hosting.as_ref(), agreement, config.tier()));
            assert_eq!(config.model(), None, "{tier}");
            assert!(
                halvo_coord::roster::model_controls(&config).is_empty(),
                "{tier}"
            );
            assert!(
                !halvo_coord::roster::lines(&config)
                    .iter()
                    .any(|line| line.text.contains("AI model")),
                "{tier}"
            );
        }
    }

    /// What the stand-in owner of `Ops` answers a body: a result, or a refusal by its error name.
    type OpAnswer = (&'static str, Result<&'static str, &'static str>);

    /// A link that answers each coordinator body by its `op` from its answers, and gives no reply to an op that they
    /// do not name; it keeps the op of each body that reaches it.
    struct Ops {
        answers: Vec<OpAnswer>,
        asked: Mutex<Vec<String>>,
    }

    impl Ops {
        fn new(answers: &[OpAnswer]) -> Arc<Ops> {
            Arc::new(Ops {
                answers: answers.to_vec(),
                asked: Mutex::new(Vec::new()),
            })
        }
    }

    impl OwnerLink for Ops {
        fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
            let request: Value = serde_json::from_slice(request).map_err(|_| LinkError)?;
            let op = request["body"]["op"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let id = request["id"].to_string();
            lock(&self.asked).push(op.clone());
            match self.answers.iter().find(|(o, _)| *o == op).map(|(_, a)| *a) {
                Some(Ok(result)) => {
                    Ok(format!(r#"{{"v":1,"id":{id},"ok":{result}}}"#).into_bytes())
                }
                Some(Err(name)) => {
                    Ok(format!(r#"{{"v":1,"id":{id},"error":"{name}"}}"#).into_bytes())
                }
                None => Err(LinkError),
            }
        }
    }

    const SAVED: OpAnswer = ("model_key_state", Ok(r#"{"stands":"yes"}"#));
    const ROOM: OpAnswer = (
        "model_room",
        Ok(r#"{"room":"reads","cap_minor":2000,"spent":0,"held":0,"period_end_ms":1800000000000}"#),
    );

    /// r4 U3.17b sentence 2: for a host with one allowed tier and with two, where a key is saved and a limit of 20.00
    /// USD stands, the question of Use an AI model holds its title, its four texts in their order, the host and the
    /// model words, joined by ` and `, the limit's amount and period, and its two answers, Not now first, each byte for
    /// byte; nothing more is shown in its place.
    #[test]
    fn the_question_names_the_host_the_models_and_the_limit_byte_for_byte() {
        let host = "hosted-model.test";
        for (models, said) in [
            (
                &["test-model-small"][..],
                "It would use test-model-small there.",
            ),
            (
                &["test-model-small", "test-model-large"][..],
                "It would use test-model-small and test-model-large there.",
            ),
        ] {
            let link: Arc<dyn OwnerLink> = Ops::new(&[SAVED, ROOM]);
            let memory = RowsMemory::default();
            let read = hosting(&link, &memory, Some(host.to_owned()), models, true);
            let model = handed(read.as_ref(), None, Tier::OnDevice).expect("a model");
            let (ask, note) = halvo_coord::roster::use_model_ask(&model).expect("the words");
            assert_eq!(note, None, "{models:?}");
            assert_eq!(
                serde_json::to_value(ask).unwrap(),
                serde_json::json!({
                    "kind": "use_model",
                    "title": "Let this agent use an AI model on someone else's computers?",
                    "text": [
                        "To work out a step, this agent would send the text of the page that it has open, or of the message that it is answering, to hosted-model.test.",
                        said,
                        "It never sends what is in a password field, your saved sign-ins or anything that Halvo recorded of your own work.",
                        "hosted-model.test bills the key that you gave Halvo. Halvo counts what its own requests cost and stops at 20.00 USD a month."
                    ],
                    "effects": [],
                    "record": null,
                    "controls": [
                        {"act": "reject", "text": "Not now"},
                        {"act": "allow", "text": "Allow for this agent"}
                    ]
                }),
                "{models:?}"
            );
        }
    }

    /// r4 U3.17b sentence 2: the question's last text is, byte for byte, the sentence `bill` that the view of the limit
    /// shows for the same reply of its state: `MODEL_LIMIT_BILL` of ui/app/words.js with `{host}`, `{amount}` and
    /// `{period}` filled as ui/app/views/model-limit.js fills them, from the reply's host, cap and period.
    #[test]
    fn the_last_text_of_the_question_is_the_bill_of_the_limit_view_for_the_same_reply() {
        let link: Arc<dyn OwnerLink> = Ops::new(&[SAVED, ROOM]);
        let memory = Memory::new();
        let reply = ModelLimit::new(Arc::clone(&link), &memory).state("hosted-model.test");
        let LimitReply::State {
            host,
            cap: Some(cap),
            period,
            ..
        } = &reply
        else {
            panic!("a state with a limit: {reply:?}");
        };
        let bill = include_str!("../../../ui/app/words.js")
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("MODEL_LIMIT_BILL: \"")?
                    .strip_suffix("\",")
            })
            .expect("the view's sentence bill");
        let shown = bill
            .replace("{host}", host)
            .replace("{amount}", cap)
            .replace("{period}", period);
        let model = ModelUse {
            host: host.clone(),
            models: "test-model-small".to_owned(),
            stands: false,
            key: Some(true),
            limit: spend_limit(&reply),
        };
        let (ask, _) = halvo_coord::roster::use_model_ask(&model).expect("the words");
        let ask = ask.expect("the question");
        assert_eq!(ask.text.last(), Some(&shown));
    }

    /// r4 U3.17b sentence 8: the read of the key's and the count's states for the agents' rows goes through
    /// `ModelLimit` with a memory of its own (`RowsMemory`), so that it never moves the memory of the limit's part.
    /// Behind a part whose last answer was the state made from the fault of the store's read (set), a read for the
    /// rows that meets a room leaves it set: a press whose own read faults again still starts a new count. Behind a
    /// part whose last answer was a room (clear), a read for the rows that meets the fault leaves it clear: a press
    /// whose read faults answers failed and starts none.
    #[test]
    fn a_read_for_the_rows_leaves_the_memory_of_the_limits_part_as_it_was() {
        const FAULT: OpAnswer = ("model_room", Err("internal"));
        const NEW: OpAnswer = (
            "new_model_count",
            Ok(r#"{"cap_minor":2500,"new_count":"yes"}"#),
        );
        let host = "hosted-model.test";
        for (part_last, rows_meet, starts) in [(FAULT, ROOM, true), (ROOM, FAULT, false)] {
            let part = Memory::new();
            let last: Arc<dyn OwnerLink> = Ops::new(&[part_last]);
            ModelLimit::new(last, &part).state(host);
            let rows = Ops::new(&[SAVED, rows_meet]);
            let link: Arc<dyn OwnerLink> = rows.clone();
            let read = hosting(
                &link,
                &RowsMemory::default(),
                Some(host.to_owned()),
                &["test-model-small"],
                true,
            );
            assert!(read.is_some(), "{starts}");
            assert_eq!(*lock(&rows.asked), ["model_key_state", "model_room"]);
            let press = Ops::new(&[FAULT, NEW]);
            let to: Arc<dyn OwnerLink> = press.clone();
            let answer = ModelLimit::new(to, &part).set("25");
            let asked = lock(&press.asked).clone();
            assert_eq!(
                asked.iter().any(|op| op == "new_model_count"),
                starts,
                "{asked:?}"
            );
            assert_eq!(
                matches!(answer, LimitReply::Set { .. }),
                starts,
                "{answer:?}"
            );
        }
    }
}
