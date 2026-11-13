use std::cell::Cell;

use halvo_agent_store::{Host, MemoryStore, Now};
use halvo_schema::executable::{CoordinatorReply, RunnerReply};
use halvo_schema::kinds::{check_record, registry};
use halvo_schema::learning::record::{LearnEdit, LearnRoutine, LearnRun};
use halvo_schema::learning::KINDS;
use halvo_schema::{ErrorName, Instant, RowRead, TypedRecord, Workflow, WorkflowId, WorkflowItem};
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

/// The agent store's side of the test host: the store in memory, and a switch that makes its rows read as a
/// store fault, an `agent_config` row that its parser refuses.
pub struct AgentSide {
    pub store: MemoryStore,
    pub fault: bool,
    pub gone: Vec<WorkflowId>,
}

impl RowRead for AgentSide {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        if self.fault && kind == "agent_config" {
            visit("{}");
        }
        self.store.for_each(kind, visit);
    }
}

impl Host for AgentSide {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.store.commit(records)
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
}

pub struct TestHost {
    pub agent: AgentSide,
    learn: Vec<(&'static str, String)>,
    pub day: u32,
    pub commits: Cell<u32>,
    pub run_workflow: Option<String>,
}

impl TestHost {
    pub fn new() -> TestHost {
        TestHost {
            agent: AgentSide {
                store: MemoryStore::new(),
                fault: false,
                gone: Vec::new(),
            },
            learn: Vec::new(),
            day: DAY,
            commits: Cell::new(0),
            run_workflow: None,
        }
    }

    pub fn put_workflow(&mut self, workflow: Workflow) {
        self.agent.store.put_workflow(workflow);
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

impl RowRead for TestHost {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        let mut rows: Vec<String> = self
            .learn
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, json)| json.clone())
            .collect();
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

impl LearnHost for TestHost {
    fn agent_host(&mut self) -> &mut dyn Host {
        &mut self.agent
    }

    fn agent_now(&self) -> Now {
        Now {
            at: Instant(1_794_500_000_000_000_000),
            day: self.day,
        }
    }

    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.commits.set(self.commits.get() + 1);
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
            self.learn.push((record.kind, record.json));
        }
        Ok(())
    }

    fn rows(&self) -> &dyn RowRead {
        self
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
            "op": "start_run", "workflow": wf(1), "agent": agent(1), "run": run(r), "mode": mode,
        }));
        assert!(reply.is_ok(), "start_run of run {r}: {reply:?}");
    }

    pub fn load(&mut self, r: u32) -> RunnerReply {
        self.runner(json!({"op": "load", "agent": agent(1), "run": run(r)}))
            .expect("the load is answered")
    }

    pub fn load_steps(&mut self, r: u32, number: u32, from: u32) -> Vec<Value> {
        let reply = self
            .runner(json!({"op": "load_steps", "agent": agent(1), "run": run(r), "number": number, "from": from}))
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
        self.runner(json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": number, "steps": steps}))
    }

    pub fn log(&mut self, r: u32, lines: &[(u32, &str, u32, Option<&str>)]) {
        let records: Vec<Value> = lines
            .iter()
            .map(|(seq, event, step, cause)| {
                json!({
                    "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "seq": seq,
                    "day": self.host.day, "at": 1_794_500_000_000_000_000_i64, "event": event,
                    "step": step, "cause": cause, "model": null, "ms": null,
                    "tokens_in": null, "tokens_out": null,
                })
            })
            .collect();
        let reply =
            self.runner(json!({"op": "log", "agent": agent(1), "run": run(r), "records": records}));
        assert!(reply.is_ok(), "log of run {r}: {reply:?}");
    }

    pub fn finish(&mut self, r: u32, mode: &str, outcome: &str) -> Result<RunnerReply, Refused> {
        let record = json!({
            "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "day": self.host.day,
            "mode": mode, "outcome": outcome, "playbook": null, "steps": 3, "prompts": 0,
            "handbacks": 0, "confirmations": 0, "effects": 0, "unknown": 0, "model_calls": 0,
            "tokens_in": 0, "tokens_out": 0, "wall_s": 9, "model": null,
        });
        self.runner(json!({"op": "finish", "agent": agent(1), "run": run(r), "record": record}))
    }

    pub fn compile(&mut self, r: u32, number: u32, steps: Vec<Value>) {
        self.start(r, "compile");
        self.load(r);
        let reply = self.put(r, number, steps);
        assert!(reply.is_ok(), "the compile's put_playbook: {reply:?}");
        let reply = self.finish(r, "compile", "completed");
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
        self.log(r, &[(1, "recovered", n, None), (2, "step_ok", n, None)]);
        let reply = self.finish(r, "replay", "completed");
        assert!(reply.is_ok(), "the replay's finish: {reply:?}");
    }

    pub fn clean_use(&mut self, r: u32, n: u32) {
        self.replay(r);
        self.log(r, &[(1, "step_ok", n, None)]);
        let reply = self.finish(r, "replay", "completed");
        assert!(reply.is_ok(), "the replay's finish: {reply:?}");
    }

    pub fn failed_use(&mut self, r: u32, n: u32, cause: &str) {
        self.replay(r);
        self.log(r, &[(1, "step_failed", n, Some(cause))]);
        let reply = self.finish(r, "replay", "failed");
        assert!(reply.is_ok(), "the replay's finish: {reply:?}");
    }
}

pub fn world() -> World {
    let mut w = World {
        host: TestHost::new(),
        handlers: LearnHandlers::new(),
        loaded: 0,
    };
    w.host
        .put_workflow(workflow(&wf(1), "utility_bill_payment", "approved", None));
    let reply = w.coordinator(json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)}));
    assert!(reply.is_ok(), "create_agent: {reply:?}");
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
    let reply = w.coordinator(json!({
        "op": "set_state", "workflow": wf(1), "agent": agent(1), "state": "active",
    }));
    assert!(
        matches!(
            reply,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::State(_)))
        ),
        "set_state: {reply:?}"
    );
    w
}
