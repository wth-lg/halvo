use std::cell::{Cell, RefCell};

use halvo_agent_store::{Host, MemoryStore, Now};
use halvo_schema::executable::{CoordinatorReply, RunnerReply};
use halvo_schema::kinds::{check_record, registry};
use halvo_schema::learning::record::{LearnEdit, LearnRoutine, LearnRun};
use halvo_schema::learning::KINDS;
use halvo_schema::{
    ErrorName, Instant, RetentionClass, RowRead, TypedRecord, Workflow, WorkflowId, WorkflowItem,
};
use serde_json::{json, Value};

use crate::{Clock, CoordinatorAnswer, LearnHandlers, LearnHost, Refused};

pub const DAY: u32 = 20_771;

pub fn id(prefix: char, n: u32) -> String {
    format!("{prefix}{n:031x}")
}

pub fn wf(n: u32) -> String {
    id('3', n)
}

pub fn agent(n: u32) -> String {
    id('4', n)
}

pub fn run(n: u32) -> String {
    id('5', n)
}

/// The agent store's side of the test host: the store in memory, a switch that makes its rows read as a store
/// fault, an `agent_config` row that its parser refuses, and the run ids whose rows read as removed, a stand-in for
/// the cascade of a workflow's delete or an expiry, which the store in memory does not make.
pub struct AgentSide {
    pub store: MemoryStore,
    pub fault: bool,
    pub gone: Vec<WorkflowId>,
    pub hidden_runs: Vec<String>,
    once: bool,
}

impl RowRead for AgentSide {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        if (self.fault || self.once) && kind == "agent_config" {
            visit("{}");
        }
        let hidden: Vec<String> = self
            .hidden_runs
            .iter()
            .map(|r| format!("\"run\":\"{r}\""))
            .collect();
        self.store.for_each(kind, &mut |row| {
            if !hidden.iter().any(|h| row.contains(h.as_str())) {
                visit(row);
            }
        });
    }
}

impl Host for AgentSide {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.store.commit(records)
    }

    fn remove_person(&mut self, kind: &str, records: Vec<TypedRecord>) -> Result<u64, ErrorName> {
        self.store.remove_person(kind, records)
    }

    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: halvo_schema::executable::names::AgentState,
        agent: Option<&halvo_schema::AgentId>,
    ) -> Result<(), ErrorName> {
        self.store.set_state(workflow, state, agent)
    }

    fn rows(&self) -> &dyn RowRead {
        self
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        if self.gone.contains(id) {
            return None;
        }
        self.store.workflow(id)
    }

    fn day(&self, at: Instant) -> u32 {
        self.store.day(at)
    }
}

/// A row of the family `learn` as the test host holds it: its kind, the workflow key and the day that the store keeps
/// beside it, as the derived store keeps its columns set at commit, and its text.
struct Held {
    kind: &'static str,
    key: Option<WorkflowId>,
    day: Option<u64>,
    json: String,
}

/// The value under the key `day` at the top level of a row's text, as a store's commit reads it.
fn day_of(text: &str) -> Option<u64> {
    serde_json::from_str::<Value>(text)
        .ok()?
        .as_object()?
        .get("day")?
        .as_u64()
}

/// The workflow that a row's text names under its kind's workflow key, as a store's commit reads it.
fn key_of(kind: &str, text: &str) -> Option<WorkflowId> {
    let key = registry().find(|e| e.name == kind)?.workflow_key?;
    let value: Value = serde_json::from_str(text).ok()?;
    WorkflowId::parse(value.as_object()?.get(key)?.as_str()?).ok()
}

pub struct TestHost {
    pub agent: AgentSide,
    learn: Vec<Held>,
    pub day: u32,
    pub commits: Cell<u32>,
    /// The key of every row of the family `learn` that a read handed out, in the order of the reads.
    pub visited: RefCell<Vec<Option<String>>>,
    pub run_workflow: Option<String>,
    pub refuse: bool,
    pub fault_first_agent_call: bool,
}

impl TestHost {
    pub fn new() -> TestHost {
        TestHost {
            agent: AgentSide {
                store: MemoryStore::new(),
                fault: false,
                gone: Vec::new(),
                hidden_runs: Vec::new(),
                once: false,
            },
            learn: Vec::new(),
            day: DAY,
            commits: Cell::new(0),
            visited: RefCell::new(Vec::new()),
            run_workflow: None,
            refuse: false,
            fault_first_agent_call: false,
        }
    }

    /// Stores a row of the family `learn` as its text, with no check, as a row that its parser may refuse, under the
    /// workflow key that its text names, or none where the text names none.
    pub fn put_raw(&mut self, kind: &'static str, text: &str) {
        let key = key_of(kind, text);
        self.put_raw_under(kind, key.as_ref().map(WorkflowId::as_str), text);
    }

    /// Stores a row of the family `learn` as its text, with no check, under the workflow key `key`, or under none, and
    /// with the day that its text holds, or today's where it holds none: a row committed today whose text was damaged
    /// later.
    pub fn put_raw_under(&mut self, kind: &'static str, key: Option<&str>, text: &str) {
        let key = key.map(|k| WorkflowId::parse(k).expect("a workflow id"));
        let day = day_of(text).or(Some(u64::from(self.day)));
        self.learn.push(Held {
            kind,
            key,
            day,
            json: text.to_owned(),
        });
    }

    pub fn put_workflow(&mut self, workflow: Workflow) {
        self.agent.store.put_workflow(workflow);
    }

    /// Every stored row of the kind, those that the read withholds too.
    pub fn stored(&self, kind: &str) -> Vec<String> {
        self.learn
            .iter()
            .filter(|row| row.kind == kind)
            .map(|row| row.json.clone())
            .collect()
    }

    pub fn learn_rows(&self, kind: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.for_each(kind, &mut |row| out.push(row.to_owned()));
        out
    }

    pub fn edits(&self) -> Vec<LearnEdit> {
        self.learn_rows("learn_edit")
            .iter()
            .map(|r| LearnEdit::parse(r.as_bytes()).expect("a stored learn_edit row"))
            .collect()
    }

    pub fn runs(&self) -> Vec<LearnRun> {
        self.learn_rows("learn_run")
            .iter()
            .map(|r| LearnRun::parse(r.as_bytes()).expect("a stored learn_run row"))
            .collect()
    }

    pub fn routines(&self) -> Vec<LearnRoutine> {
        self.learn_rows("learn_routine")
            .iter()
            .map(|r| LearnRoutine::parse(r.as_bytes()).expect("a stored learn_routine row"))
            .collect()
    }

    pub fn agent_rows(&self, kind: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.agent
            .for_each(kind, &mut |row| out.push(row.to_owned()));
        out
    }

    pub fn count(&self) -> usize {
        self.learn.len()
    }
}

impl Clock for TestHost {
    fn today(&self) -> u32 {
        self.day
    }
}

fn withheld(kind: &str, day: Option<u64>, today: u32) -> bool {
    let Some(RetentionClass::Bounded { days }) =
        registry().find(|e| e.name == kind).map(|e| e.class)
    else {
        return false;
    };
    match day {
        None => true,
        Some(d) if d > u64::from(today) => d - u64::from(today) > 2,
        Some(d) => u64::from(today) - d >= u64::from(days),
    }
}

impl TestHost {
    /// Visits the rows of the kind that the store holds under the workflow key `workflow`, in ascending byte order,
    /// and no row held under another key or under none, as `Rows::for_each_under` of halvo-derived does.
    pub fn for_each_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        self.visit(kind, Some(workflow), visit);
    }

    fn visit(&self, kind: &str, workflow: Option<&WorkflowId>, visit: &mut dyn FnMut(&str)) {
        let held: Vec<&Held> = self
            .learn
            .iter()
            .filter(|row| row.kind == kind && !withheld(kind, row.day, self.day))
            .filter(|row| workflow.is_none_or(|w| row.key.as_ref() == Some(w)))
            .collect();
        self.visited.borrow_mut().extend(
            held.iter()
                .map(|row| row.key.as_ref().map(|k| k.as_str().to_owned())),
        );
        let mut rows: Vec<String> = held.iter().map(|row| row.json.clone()).collect();
        if let (Some(other), "learn_run") = (&self.run_workflow, kind) {
            for row in &mut rows {
                let mut value: Value = serde_json::from_str(row).expect("a stored row");
                value["workflow"] = Value::String(other.clone());
                *row = value.to_string();
            }
        }
        rows.sort();
        for row in &rows {
            visit(row);
        }
    }
}

impl RowRead for TestHost {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        self.visit(kind, None, visit);
    }
}

impl LearnHost for TestHost {
    fn agent_host(&mut self) -> &mut dyn Host {
        self.agent.once = std::mem::take(&mut self.fault_first_agent_call);
        &mut self.agent
    }

    fn agent_now(&self) -> Now {
        let day = 86_400_000_000_000_i64;
        Now {
            at: Instant(
                i64::from(self.day)
                    .saturating_mul(day)
                    .saturating_add(day / 2),
            ),
        }
    }

    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.commits.set(self.commits.get() + 1);
        if self.refuse {
            return Err(ErrorName::Store);
        }
        for record in &records {
            if !KINDS.iter().any(|k| k.name == record.kind) {
                return Err(ErrorName::Store);
            }
            let entry = registry()
                .find(|e| e.name == record.kind)
                .ok_or(ErrorName::Store)?;
            check_record(entry, &record.json).map_err(|_| ErrorName::Store)?;
            let value: Value = serde_json::from_str(&record.json).map_err(|_| ErrorName::Store)?;
            let workflow = value["workflow"]
                .as_str()
                .and_then(|w| WorkflowId::parse(w).ok())
                .ok_or(ErrorName::Store)?;
            if self.agent.workflow(&workflow).is_none() {
                return Err(ErrorName::Store);
            }
        }
        for record in records {
            let key = key_of(record.kind, &record.json);
            let day = day_of(&record.json);
            self.learn.push(Held {
                kind: record.kind,
                key,
                day,
                json: record.json,
            });
        }
        Ok(())
    }

    fn rows(&self) -> &dyn RowRead {
        self
    }

    fn for_each_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        TestHost::for_each_under(self, kind, workflow, visit);
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.agent.workflow(id)
    }
}

pub fn workflow(id: &str, category: &str, state: &str, agent: Option<&str>) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": id,
        "state": state,
        "category": category,
        "site": {"key": "northwind-power.test", "source": "address_field"},
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": agent,
    }))
    .expect("a workflow record")
}

pub fn locator(by: &str, value: &str, name: Option<&str>) -> Value {
    json!({"by": by, "value": value, "name": name})
}

pub fn navigate(n: u32) -> Value {
    json!({
        "v": 1,
        "n": n,
        "kind": "web",
        "verb": "navigate",
        "target": null,
        "args": [{"name": "url", "from": "slot", "value": null, "key": "start"}],
        "pre": [],
        "post": [],
        "effect": "none",
        "class": "read",
    })
}

pub fn click(n: u32, desc: &str, locators: Vec<Value>) -> Value {
    json!({
        "v": 1,
        "n": n,
        "kind": "web",
        "verb": "click",
        "target": {"desc": desc, "frame": [], "credential": "no", "locators": locators},
        "args": [],
        "pre": [],
        "post": [],
        "effect": "none",
        "class": "prepare",
    })
}

pub fn compiled() -> Vec<Value> {
    vec![
        navigate(1),
        click(
            2,
            "button Pay",
            vec![locator("role", "button", Some("Pay"))],
        ),
        click(
            3,
            "button Next",
            vec![locator("role", "button", Some("Next"))],
        ),
    ]
}

pub fn with_locators(mut step: Value, desc: &str, locators: Vec<Value>) -> Value {
    step["target"]["desc"] = Value::String(desc.to_owned());
    step["target"]["locators"] = Value::Array(locators);
    step
}

pub struct World {
    pub host: TestHost,
    pub handlers: LearnHandlers,
    pub loaded: u32,
    pub pair: u32,
}

impl World {
    pub fn runner(&mut self, body: Value) -> Result<RunnerReply, Refused> {
        self.handlers
            .runner(&mut self.host, body.to_string().as_bytes())
    }

    pub fn coordinator(&mut self, body: Value) -> Result<CoordinatorAnswer, Refused> {
        self.handlers
            .coordinator(&mut self.host, body.to_string().as_bytes())
    }

    pub fn start(&mut self, r: u32, mode: &str) {
        let reply = self.coordinator(json!({
            "op": "start_run", "workflow": wf(self.pair), "agent": agent(self.pair), "run": run(r), "mode": mode,
        }));
        assert!(reply.is_ok(), "start_run of run {r}: {reply:?}");
    }

    pub fn load(&mut self, r: u32) -> RunnerReply {
        self.runner(json!({"op": "load", "agent": agent(self.pair), "run": run(r)}))
            .expect("the load is answered")
    }

    pub fn load_steps(&mut self, r: u32, number: u32, from: u32) -> Vec<Value> {
        let reply = self
            .runner(json!({"op": "load_steps", "agent": agent(self.pair), "run": run(r), "number": number, "from": from}))
            .expect("the load_steps is answered");
        let RunnerReply::Steps(steps) = reply else {
            panic!("a steps reply");
        };
        steps
            .steps
            .iter()
            .map(|s| serde_json::to_value(s).expect("a step's JSON"))
            .collect()
    }

    pub fn put(&mut self, r: u32, number: u32, steps: Vec<Value>) -> Result<RunnerReply, Refused> {
        self.runner(json!({"op": "put_playbook", "agent": agent(self.pair), "run": run(r), "number": number, "steps": steps}))
    }

    pub fn log(&mut self, r: u32, lines: &[(u32, &str, u32, Option<&str>)]) {
        let records: Vec<Value> = lines
            .iter()
            .map(|(seq, event, step, cause)| {
                json!({
                    "v": 1, "workflow": wf(self.pair), "agent": agent(self.pair), "run": run(r), "seq": seq,
                    "day": self.host.day, "at": self.host.agent_now().at.0, "event": event,
                    "step": step, "cause": cause, "model": null, "ms": null,
                    "tokens_in": null, "tokens_out": null, "page_amount": null,
                })
            })
            .collect();
        let reply = self.runner(
            json!({"op": "log", "agent": agent(self.pair), "run": run(r), "records": records}),
        );
        assert!(reply.is_ok(), "log of run {r}: {reply:?}");
    }

    pub fn finish(&mut self, r: u32, mode: &str, outcome: &str) -> Result<RunnerReply, Refused> {
        self.finish_of(r, mode, outcome, None)
    }

    /// `finish` of the run whose record names `playbook`, the number under which a compile stored its steps (A7.4).
    pub fn finish_of(
        &mut self,
        r: u32,
        mode: &str,
        outcome: &str,
        playbook: Option<u32>,
    ) -> Result<RunnerReply, Refused> {
        self.runner(self.finish_body(r, mode, outcome, playbook))
    }

    fn finish_body(&self, r: u32, mode: &str, outcome: &str, playbook: Option<u32>) -> Value {
        let record = json!({
            "v": 1, "workflow": wf(self.pair), "agent": agent(self.pair), "run": run(r), "day": self.host.day,
            "mode": mode, "outcome": outcome, "playbook": playbook, "steps": 3, "prompts": 0,
            "handbacks": 0, "confirmations": 0, "effects": 0, "unknown": 0, "model_calls": 0,
            "tokens_in": 0, "tokens_out": 0, "wall_s": 9, "model": null,
            "before_start": null, "cut": null, "no_key": null, "not_to_account": null,
            "incomplete": null, "reply_to_other": null, "more_pages": null, "held_recipient": null,
            "organized": null, "other_task": null, "held_earlier": null, "earlier_page": null,
        });
        json!({"op": "finish", "agent": agent(self.pair), "run": run(r), "record": record})
    }

    /// The compile of the run `r` that starts, stores `number` and finishes it in the agent store alone, so that the
    /// agent store holds a newer finished number that the handlers did not see.
    pub fn direct_compile(&mut self, r: u32, number: u32) {
        let now = self.host.agent_now();
        let start = json!({"op": "start_run", "workflow": wf(self.pair), "agent": agent(self.pair), "run": run(r), "mode": "compile"});
        let started =
            halvo_agent_store::coordinator(&mut self.host.agent, now, start.to_string().as_bytes());
        assert!(
            started.is_ok(),
            "the agent store starts run {r}: {started:?}"
        );
        let body = json!({"op": "put_playbook", "agent": agent(self.pair), "run": run(r), "number": number, "steps": compiled()});
        let put = halvo_agent_store::runner(&mut self.host.agent, now, body.to_string().as_bytes());
        assert!(
            put.is_ok(),
            "the agent store stores number {number}: {put:?}"
        );
        self.direct_finish(r, number);
    }

    /// The finish of the compile of the run `r` with the outcome `completed`, sent to the agent store alone.
    pub fn direct_finish(&mut self, r: u32, number: u32) {
        let now = self.host.agent_now();
        let body = self.finish_body(r, "compile", "completed", Some(number));
        let reply =
            halvo_agent_store::runner(&mut self.host.agent, now, body.to_string().as_bytes());
        assert!(
            reply.is_ok(),
            "the agent store finishes number {number}: {reply:?}"
        );
    }

    pub fn compile(&mut self, r: u32, number: u32, steps: Vec<Value>) {
        self.start(r, "compile");
        self.load(r);
        let reply = self.put(r, number, steps);
        assert!(reply.is_ok(), "the compile's put_playbook: {reply:?}");
        let reply = self.finish_of(r, "compile", "completed", Some(number));
        assert!(reply.is_ok(), "the compile's finish: {reply:?}");
    }

    pub fn replay(&mut self, r: u32) -> Vec<Value> {
        self.start(r, "replay");
        let RunnerReply::Load(load) = self.load(r) else {
            panic!("a load reply");
        };
        let number = load.playbook.expect("a stored playbook").number.get();
        self.loaded = number;
        self.load_steps(r, number, 1)
    }

    pub fn learn(&mut self, r: u32, n: u32, desc: &str, locators: Vec<Value>) {
        let served = self.replay(r);
        let proposed = with_locators(served[n as usize - 1].clone(), desc, locators);
        let reply = self.put(r, self.loaded, vec![proposed]);
        assert!(reply.is_ok(), "the replay's put_playbook: {reply:?}");
        self.log(r, &[(2, "recovered", n, None), (3, "step_ok", n, None)]);
        let reply = self.finish(r, "replay", "completed");
        assert!(reply.is_ok(), "the replay's finish: {reply:?}");
    }

    pub fn clean_use(&mut self, r: u32, n: u32) {
        self.replay(r);
        self.log(r, &[(2, "step_ok", n, None)]);
        let reply = self.finish(r, "replay", "completed");
        assert!(reply.is_ok(), "the replay's finish: {reply:?}");
    }

    pub fn failed_use(&mut self, r: u32, n: u32, cause: &str) {
        self.replay(r);
        self.log(r, &[(2, "step_failed", n, Some(cause))]);
        let reply = self.finish(r, "replay", "failed");
        assert!(reply.is_ok(), "the replay's finish: {reply:?}");
    }
}

pub fn world() -> World {
    let mut w = bare_world();
    w.create(1);
    w
}

/// A world with no workflow and no agent.
pub fn bare_world() -> World {
    World {
        host: TestHost::new(),
        handlers: LearnHandlers::new(),
        loaded: 0,
        pair: 1,
    }
}

/// Workflow 1 and its agent 1 as a stop behind the state call of `create_agent` leaves them: the agent store's rows
/// and no row of the family `learn` (the workflow run's ruling S-69).
pub fn world_with_no_routine_row() -> World {
    let mut w = bare_world();
    w.host
        .put_workflow(workflow(&wf(1), "utility_bill_payment", "approved", None));
    let now = w.host.agent_now();
    let body = json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)});
    let reply = halvo_agent_store::coordinator(&mut w.host.agent, now, body.to_string().as_bytes());
    assert!(reply.is_ok(), "the agent store's create_agent: {reply:?}");
    w
}

pub fn fill(n: u32, text: &str) -> Value {
    json!({
        "v": 1,
        "n": n,
        "kind": "web",
        "verb": "fill",
        "target": {"desc": "textbox Note", "frame": [], "credential": "no", "locators": [locator("label", "Note", None)]},
        "args": [{"name": "text", "from": "const", "value": text, "key": null}],
        "pre": [],
        "post": [],
        "effect": "none",
        "class": "prepare",
    })
}

pub fn active_world() -> World {
    active_world_with(compiled())
}

pub fn active_world_with(steps: Vec<Value>) -> World {
    let mut w = world();
    w.compile(1, 1, steps);
    w.activate();
    w
}

impl World {
    /// Stores workflow `n` and sends the `create_agent` of agent `n` for it.
    pub fn create(&mut self, n: u32) {
        self.host
            .put_workflow(workflow(&wf(n), "utility_bill_payment", "approved", None));
        let reply =
            self.coordinator(json!({"op": "create_agent", "workflow": wf(n), "agent": agent(n)}));
        assert!(reply.is_ok(), "create_agent: {reply:?}");
    }

    /// Workflow `n` with its agent `n` compiled under number 1 by run 1000 + `n` and active; the pair of the helpers
    /// stays as it was.
    pub fn add_pair(&mut self, n: u32) {
        let pair = self.pair;
        self.create(n);
        self.pair = n;
        self.compile(1000 + n, 1, compiled());
        self.activate();
        self.pair = pair;
    }

    fn activate(&mut self) {
        let reply = self.coordinator(json!({
            "op": "set_state", "workflow": wf(self.pair), "agent": agent(self.pair), "state": "active",
        }));
        assert!(
            matches!(
                reply,
                Ok(CoordinatorAnswer::Agent(CoordinatorReply::State(_)))
            ),
            "set_state: {reply:?}"
        );
    }
}
