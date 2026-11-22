//! The product's ports of the coordinator on a Mac (contracts I5.10; unit U5.4a): the hub of the app runs over them
//! in the place of the stand-in `NoService`.
//!
//! - `Owner` writes and reads as the role `coordinator` over an owner stream of the app's own (W-C24.13), through
//!   the product's link of the supervisor library: `coord_log` and `coord_history` for the events, `list_agents`,
//!   `agent_history`, `list_overlaps` and the bodies of `Body` for the agent store. The list of the workflows and the
//!   decision `delete` are reads and decisions of the role `interface`, which a stream refuses: they go through the
//!   app's relay call. No write of the coordinator goes through the relay.
//! - `Runs` is the supervisor library over a second owner stream: `start` hands the machine's zone offset of the
//!   moment as `utc_offset_s`; the first `reap` of a life answers what the start's own reap answered.
//! - `Clock` reads the system clock and the zone of the Mac, `Rng` the system's generator, `Policy` the table of
//!   halvo-policy (`decide_row`) and `Senders` the schema's reader of `reply_senders`.
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
#![cfg(target_os = "macos")]

use std::collections::HashMap;
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
    ReapAnswer, RunEnd, Secrets, Supervisor,
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
    HoldSetCause, ItemId, ItemKind, Kept, LapseCause, Lapsed, LinkAnswer, Name, Origin, OtherLapse,
    Outcome, PastEffect, PastRun, Progress, Raised, ReadyAnswer, Reply, ReplyRow,
    RunEnd as CoordRunEnd, RunEvent, RunHistory, RunId, RunSpec, SetupAnswer, SignInAnswer, Slot,
    Trigger, Unknown, What, Workflow, WorkflowId, WorkflowState, WorkflowTrigger,
};
use halvo_coord::types::{Autonomy, Budget, Limits, Permission, Reasoning, Runtime, Tier};
use halvo_schema::executable::frame::{
    ConfirmDecision, ConfirmRequest, HandbackRequest, SecretReply,
};
use halvo_schema::executable::names::Answer as EffectReply;
use halvo_schema::executable::text::{EffectId as WireEffect, Name as WireName, WebOrigin};
use halvo_schema_core::request::{decode_reply, InterfaceDecision, InterfaceRead, InterfaceRole};
use halvo_schema_core::request::{ErrorName, ReplyResult};
use serde_json::Value;

use crate::hub::{Core, Hub, Present};

/// The call of one end of a new owner stream.
pub type StreamCall = Arc<dyn Fn() -> Result<OwnerStreamEnd, ClientError> + Send + Sync>;
/// The call that relays one request of the role `interface` and gives the owner's reply.
pub type RelayCall = Arc<dyn Fn(&[u8]) -> Result<Vec<u8>, ClientError> + Send + Sync>;
/// The call of one run mark.
pub type MarkCall = Arc<dyn Fn(RunEdge) -> Result<RunMarked, ClientError> + Send + Sync>;

/// The app's client calls that the product's ports go through: a new owner stream, the relay of one request of the
/// role `interface`, and one run mark.
#[derive(Clone)]
pub struct Calls {
    /// One end of a new owner stream (`Control::owner_stream` of the app).
    pub owner_stream: StreamCall,
    /// One request of the role `interface` relayed to the owner, and the owner's reply (`Control::relay`).
    pub relay: RelayCall,
    /// One run mark (`Control::run_mark`).
    pub mark: MarkCall,
}

/// Where the product's runs live: the absolute data directory and the absolute paths that a start hands the
/// runner.
#[derive(Clone, Debug)]
pub struct Places {
    pub data_dir: PathBuf,
    pub runner: PathBuf,
    pub browser: PathBuf,
    pub model: Option<PathBuf>,
}

/// The hub of the app over the product's ports.
pub type ProductHub = Hub<Product>;

/// The hub over the product's ports of `calls` and `places`, whose count of agents that need the person goes to
/// `tray` and whose native confirmations go to `present`. It starts nothing and reads nothing: `start` takes the
/// steps of the app's start.
pub fn hub(
    calls: Calls,
    places: Places,
    tray: Box<dyn Fn(u32) + Send + Sync>,
    present: Box<dyn Present>,
) -> Arc<ProductHub> {
    let limits = Limits::PRODUCT;
    let (product, events) = Product::new(calls, places);
    let hub = Hub::new(product, limits, tray, present);
    events.attach(Arc::downgrade(&hub));
    hub
}

/// The steps of the app's start, in their order: the library's reap of what an earlier life left (it ends what is
/// left of each such run and removes its mark), and one mark `run_end` where it answered a run and no run of this
/// life is live; the hub's first tick, whose recovery takes that reap's answers and ends each such run with its
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

/// The library's `Secrets` as the library has them: none stored for every name (a named limit of the unit).
struct NoSecrets;

impl Secrets for NoSecrets {
    fn read(&self, _agent: &halvo_schema::AgentId, _name: &WireName) -> SecretReply {
        SecretReply::None
    }
}

/// The product's ports.
pub struct Product {
    link: Arc<dyn OwnerLink>,
    relay: RelayCall,
    supervisor: Supervisor,
    places: Places,
    marks: Arc<Marks>,
    runs: RunsOf,
    waits: Arc<Waits>,
    agents: Mutex<HashMap<AgentId, WorkflowId>>,
    reaped: Mutex<Option<Vec<ReapAnswer>>>,
    checkpoints: AtomicBool,
}

/// A link over the owner streams that `calls` asks for.
fn link_of(calls: &Calls) -> Arc<dyn OwnerLink> {
    let call = Arc::clone(&calls.owner_stream);
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
            secrets: Arc::new(NoSecrets),
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
            link: link_of(&calls),
            relay: Arc::clone(&calls.relay),
            supervisor,
            places,
            marks,
            runs,
            waits,
            agents: Mutex::new(HashMap::new()),
            reaped: Mutex::new(None),
            checkpoints: AtomicBool::new(false),
        };
        (product, events)
    }

    /// The library's reap of what an earlier life left, kept for the first `Runs::reap`, and the caller's own mark
    /// `run_end` where it answered a run and no run of this life is live: the library's reap calls no `MarkSink`.
    fn reap_earlier_life(&self) {
        let Ok(answers) = self.supervisor.reap() else {
            return;
        };
        let any = !answers.is_empty();
        *lock(&self.reaped) = Some(answers);
        if any && self.marks.live.load(Ordering::SeqCst) == 0 {
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
        if let Some(w) = lock(&self.agents).get(agent).cloned() {
            return Ok(w);
        }
        self.agents()?;
        lock(&self.agents)
            .get(agent)
            .cloned()
            .ok_or_else(|| PortError::Refused(name("binding")))
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn name(text: &str) -> Name {
    Name::parse(text).unwrap_or_else(|_| Name::parse("internal").expect("a name"))
}

fn refused(error: ErrorName) -> PortError {
    PortError::Refused(name(error.as_str()))
}

/// The result of the owner's reply `bytes` to the request `id`.
fn result_of(bytes: &[u8], id: Option<&str>) -> Result<Value, PortError> {
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

fn json_str(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

/// The text of a coordinator body of `Body`, its keys in the order of A-A7.2; `end_run` takes the workflow and the
/// agent of the run as the product knows them.
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
    })
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
            cause: HoldSetCause::parse(s("cause")?).ok()?,
        },
        "hold_cleared" => What::HoldCleared {
            cause: match s("cause")? {
                "person" => Cleared::Person,
                "failures" => Cleared::Failures,
                "check" => Cleared::Check {
                    run: run()?,
                    answer: CheckAnswer::parse(s("answer")?).ok()?,
                },
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

/// One entry of the reply of `list_agents`.
fn agent_of(entry: &Value) -> Option<Agent<AgentConfig>> {
    let config = AgentConfig::parse(&entry["config"].to_string())?;
    let last = match &entry["last"] {
        Value::Null => None,
        record => Some(past_run(record)?),
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
        last,
        unknown,
        config,
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
                listed.push(agent_of(entry).ok_or(PortError::NoService)?);
            }
            match reply["next"].as_str() {
                Some(next) if json_str(next) != after => after = json_str(next),
                Some(_) => return Err(PortError::NoService),
                None => break,
            }
        }
        let mut agents = lock(&self.agents);
        for a in &listed {
            agents.insert(a.agent.clone(), a.workflow.clone());
        }
        Ok(listed)
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

    /// The rows of `agent_reply` are read by the read that serves `interface_read` alone (ruling RD-65 (d)), which
    /// no request of the relay carries yet: the read does not answer.
    fn replies(
        &self,
        _workflow: &WorkflowId,
        _agent: &AgentId,
        _run: &RunId,
    ) -> Result<Vec<ReplyRow>, PortError> {
        Err(PortError::NoService)
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
            browser: self.places.browser.clone(),
            model: self.places.model.clone(),
            headed: spec.headed,
            utc_offset_s: zone_offset_s(SystemTime::now()).ok_or(PortError::NoService)?,
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

/// The daemon's request that defers the analysis batch is not in the control crate: the batch is not deferred, and
/// the core asks again at its ticks.
impl Analysis for Product {
    fn defer(&self, _until: u64) -> Result<(), PortError> {
        Err(PortError::NoService)
    }
}

/// The offset of the Mac's zone from UTC at `at`, in whole seconds east of UTC, as the system's zone gives it.
pub fn zone_offset_s(at: SystemTime) -> Option<i32> {
    use objc2_foundation::{NSDate, NSTimeZone};
    let seconds = at.duration_since(UNIX_EPOCH).ok()?.as_secs_f64();
    NSTimeZone::resetSystemTimeZone();
    let zone = NSTimeZone::localTimeZone();
    let date = NSDate::dateWithTimeIntervalSince1970(seconds);
    i32::try_from(zone.secondsFromGMTForDate(&date)).ok()
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
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        let mut bytes = [0u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|_| PortError::NoService)?;
        Ok(bytes)
    }
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
        };
        let places = Places {
            data_dir: PathBuf::from("/nowhere/halvo-u54a"),
            runner: PathBuf::from("/nowhere/halvo-runner"),
            browser: PathBuf::from("/nowhere/halvo-browser"),
            model: None,
        };
        let hub = hub(fails, places, Box::new(|_| {}), Box::new(Drops));
        assert!(attached.set(Arc::downgrade(&hub)).is_ok());
        let unread = guarded.request(&effect(&record("no_such_class"))).unwrap();
        assert_eq!(error_of(&unread).as_deref(), Some("approval"));
        assert!(
            lock(&kept.0).is_empty(),
            "a record that the guard cannot read went on"
        );
    }

    /// A presenter that drops each dialog.
    struct Drops;

    impl Present for Drops {
        fn present(&self, _dialog: crate::hub::Native) {}
    }
}
