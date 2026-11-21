#![allow(dead_code)]

use halvo_agent_store::{Host, MemoryStore, Now};
use halvo_policy::preset;
use halvo_schema::executable::names::{Mode, Outcome, YesNo};
use halvo_schema::executable::text::RunId;
use halvo_schema::executable::{
    ActionClass, AgentBindings, AgentConfig, AgentEffect, AgentRun, AllowedClass, Autonomy,
    CoordinatorReply, RunnerReply,
};
use halvo_schema::{
    AgentId, Category, ErrorName, Instant, RowRead, TypedRecord, Workflow, WorkflowId,
};
use serde_json::{json, Value};

/// The instant of every request of a test, 2026-11-12T16:13:20Z, and its day in UTC, the memory store's calendar
/// unless a test sets an offset.
pub const NOW: Now = Now {
    at: Instant(1_794_500_000_000_000_000),
};
pub const TODAY: u32 = 20_769;

/// The instant of the rows that a test's runner sends, 2026-11-11T12:26:40Z, and its day in UTC.
pub const ROW_AT: i64 = 1_794_400_000_000_000_000;
pub const ROW_DAY: u32 = 20_768;

pub const NANOS_PER_DAY: i64 = 86_400_000_000_000;

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

pub fn effect(n: u32) -> String {
    id('6', n)
}

pub fn approval(n: u32) -> String {
    id('7', n)
}

pub fn digest(n: u32) -> String {
    format!("d{n:063x}")
}

pub fn declared(name: &str) -> ErrorName {
    ErrorName::read(name).expect("a declared error name")
}

pub fn workflow(id: &str, category: &str, state: &str, agent: Option<&str>) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": id,
        "state": state,
        "category": category,
        "site": null,
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": agent,
    }))
    .expect("a workflow record")
}

pub fn dismissed(id: &str) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": id,
        "state": "dismissed",
        "category": "email",
        "site": null,
        "app": "com.example.browser",
    }))
    .expect("a dismissed workflow record")
}

/// A zone five hours west of UTC, as the memory store's offset.
pub const WEST: i32 = -5 * 3600;

/// The instant, in nanoseconds of UTC, of `seconds` after the start of the local `day` in a zone of `offset`
/// seconds.
pub fn local(day: u32, seconds: i64, offset: i32) -> i64 {
    (i64::from(day) * 86_400 + seconds - i64::from(offset)) * 1_000_000_000
}

/// The row with its `day` and `at` set.
pub fn dated(mut row: Value, day: u32, at: i64) -> Value {
    row["day"] = json!(day);
    row["at"] = json!(at);
    row
}

pub fn coordinator(store: &mut MemoryStore, body: Value) -> Result<CoordinatorReply, ErrorName> {
    coordinator_at(store, NOW, body)
}

pub fn runner(store: &mut MemoryStore, body: Value) -> Result<RunnerReply, ErrorName> {
    runner_at(store, NOW, body)
}

pub fn coordinator_at(
    store: &mut MemoryStore,
    now: Now,
    body: Value,
) -> Result<CoordinatorReply, ErrorName> {
    halvo_agent_store::coordinator(store, now, body.to_string().as_bytes())
}

pub fn runner_at(store: &mut MemoryStore, now: Now, body: Value) -> Result<RunnerReply, ErrorName> {
    halvo_agent_store::runner(store, now, body.to_string().as_bytes())
}

pub fn put_at(store: &mut MemoryStore, now: Now, row: Value) -> Result<RunnerReply, ErrorName> {
    let body = json!({
        "op": "effect",
        "agent": row["agent"].clone(),
        "run": row["run"].clone(),
        "record": row,
    });
    runner_at(store, now, body)
}

pub fn at(nanos: i64) -> Now {
    Now { at: Instant(nanos) }
}

pub fn create(store: &mut MemoryStore, w: u32, a: u32) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "create_agent", "workflow": wf(w), "agent": agent(a)}),
    )
}

/// `start_run` of the mode `compile` for the run `r` of the agent `a` of the workflow `w`, at the instant `now`.
pub fn start_at(
    store: &mut MemoryStore,
    now: Now,
    w: u32,
    a: u32,
    r: u32,
) -> Result<CoordinatorReply, ErrorName> {
    coordinator_at(
        store,
        now,
        json!({"op": "start_run", "workflow": wf(w), "agent": agent(a), "run": run(r), "mode": "compile"}),
    )
}

/// `start_run` of the run `r` of the first agent, which every runner body of the run needs (CP-135 (5)).
pub fn start(store: &mut MemoryStore, r: u32) {
    start_at(store, NOW, 1, 1, r).expect("start_run of the first agent");
}

/// The first agent of an approved workflow of the category, with no run started.
pub fn unstarted(category: &str) -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), category, "approved", None));
    create(&mut store, 1, 1).expect("create_agent of the first agent");
    store
}

/// The first agent of an approved workflow of the category, with its run 1 started.
pub fn store_with(category: &str) -> MemoryStore {
    let mut store = unstarted(category);
    start(&mut store, 1);
    store
}

pub fn unattended_sender() -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), "email", "approved", Some(&agent(1))));
    let mut config = preset(
        Category::Email,
        WorkflowId::parse(&wf(1)).expect("an id"),
        AgentId::parse(&agent(1)).expect("an id"),
    )
    .expect("a preset");
    config.autonomy = Autonomy::Unattended;
    config.allow = vec![AllowedClass::Send];
    config.caps.untrusted = YesNo::No;
    store
        .commit(vec![TypedRecord {
            kind: "agent_config",
            json: config.to_json().expect("the config's JSON"),
        }])
        .expect("the config is stored");
    start(&mut store, 1);
    store
}

pub fn rows(store: &MemoryStore, kind: &str) -> Vec<String> {
    let mut out = Vec::new();
    store.for_each(kind, &mut |row| out.push(row.to_owned()));
    out
}

pub fn effects(store: &MemoryStore) -> Vec<AgentEffect> {
    rows(store, "agent_effect")
        .iter()
        .map(|r| AgentEffect::parse(r.as_bytes()).expect("a stored effect row"))
        .collect()
}

pub fn last_of(store: &MemoryStore, effect_id: &str) -> AgentEffect {
    let mut mine: Vec<AgentEffect> = effects(store)
        .into_iter()
        .filter(|e| e.effect.as_str() == effect_id)
        .collect();
    mine.sort_by_key(|e| e.seq);
    mine.pop().expect("a row of the effect")
}

pub fn configs(store: &MemoryStore) -> Vec<AgentConfig> {
    rows(store, "agent_config")
        .iter()
        .map(|r| AgentConfig::parse(r.as_bytes()).expect("a stored config row"))
        .collect()
}

pub fn bindings(store: &MemoryStore) -> Vec<AgentBindings> {
    rows(store, "agent_bindings")
        .iter()
        .map(|r| AgentBindings::parse(r.as_bytes()).expect("a stored bindings row"))
        .collect()
}

pub fn stored_workflow(store: &MemoryStore, w: u32) -> Workflow {
    store
        .workflow(&WorkflowId::parse(&wf(w)).expect("a workflow id"))
        .expect("a stored workflow")
        .workflow
}

pub fn kept(class: &str) -> Value {
    let class = ActionClass::parse(class).expect("a class");
    Value::Array(
        class
            .kept_fields()
            .into_iter()
            .map(|(name, _)| json!({"name": name.as_str(), "value": ["v"]}))
            .collect(),
    )
}

pub fn fields(class: &str) -> usize {
    ActionClass::parse(class)
        .expect("a class")
        .bound_fields()
        .len()
}

#[allow(clippy::too_many_arguments)]
pub fn row(
    r: u32,
    e: u32,
    seq: u32,
    state: &str,
    class: &str,
    d: u32,
    approval_id: Option<u32>,
    cause: Option<&str>,
) -> Value {
    json!({
        "v": 1,
        "workflow": wf(1),
        "agent": agent(1),
        "run": run(r),
        "effect": effect(e),
        "seq": seq,
        "day": ROW_DAY,
        "at": ROW_AT,
        "state": state,
        "class": class,
        "digest": digest(d),
        "kept": kept(class),
        "fields": fields(class),
        "approval": approval_id.map(approval),
        "cause": cause,
    })
}

pub fn put(store: &mut MemoryStore, row: Value) -> Result<RunnerReply, ErrorName> {
    let body = json!({
        "op": "effect",
        "agent": row["agent"].clone(),
        "run": row["run"].clone(),
        "record": row,
    });
    runner(store, body)
}

pub fn stored(n: u32) -> RunnerReply {
    RunnerReply::parse(format!(r#"{{"stored":{n}}}"#).as_bytes()).expect("a stored reply")
}

pub fn run_record(r: u32, mode: Option<&str>, outcome: &str) -> Value {
    let mut made = AgentRun::new(
        WorkflowId::parse(&wf(1)).expect("a workflow id"),
        AgentId::parse(&agent(1)).expect("an agent id"),
        RunId::parse(&run(r)).expect("a run id"),
        ROW_DAY,
        Some(Mode::Replay),
        Outcome::Completed,
    );
    made.steps = 3;
    made.prompts = 1;
    made.confirmations = 1;
    made.effects = 1;
    made.wall_s = 9;
    let text = made.to_json().expect("a made record");
    let mut record: Value = serde_json::from_str(&text).expect("the made record's text");
    record["mode"] = json!(mode);
    record["outcome"] = json!(outcome);
    record
}

/// The record of the compile `r` of the first agent with the outcome `completed`, naming the playbook number under
/// which it stored its steps; its `finish` writes the row `agent_finished` of that number (CP-182).
pub fn compiled(r: u32, number: u32) -> Value {
    let mut record = run_record(r, Some("compile"), "completed");
    record["playbook"] = json!(number);
    record
}

/// A row `agent_finished` of the agent `a` of the first workflow: the compile of the run `r` finished the playbook
/// number with `steps` steps.
pub fn finished_row(a: u32, number: u32, steps: u32, r: u32) -> TypedRecord {
    let row = json!({
        "v": 1, "workflow": wf(1), "agent": agent(a), "number": number, "steps": steps, "run": run(r),
    });
    TypedRecord {
        kind: "agent_finished",
        json: row.to_string(),
    }
}

pub fn finish(store: &mut MemoryStore, record: Value) -> Result<RunnerReply, ErrorName> {
    let body = json!({
        "op": "finish",
        "agent": record["agent"].clone(),
        "run": record["run"].clone(),
        "record": record,
    });
    runner(store, body)
}

pub fn end_run(store: &mut MemoryStore, r: u32) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "end_run", "workflow": wf(1), "agent": agent(1), "run": run(r)}),
    )
}

pub fn step(n: u32, verb: &str, class: &str) -> Value {
    let (target, args) = match verb {
        "navigate" => (
            Value::Null,
            json!([{"name": "url", "from": "slot", "value": null, "key": "start"}]),
        ),
        "click" => (
            json!({
                "desc": "button Pay",
                "frame": [],
                "credential": "no",
                "locators": [{"by": "role", "value": "button", "name": "Pay"}],
            }),
            json!([]),
        ),
        other => panic!("no step helper for the verb {other}"),
    };
    json!({
        "v": 1,
        "n": n,
        "kind": "web",
        "verb": verb,
        "target": target,
        "args": args,
        "pre": [],
        "post": [],
        "effect": "none",
        "class": class,
    })
}

pub fn put_steps(
    store: &mut MemoryStore,
    number: u32,
    steps: Vec<Value>,
) -> Result<RunnerReply, ErrorName> {
    runner(
        store,
        json!({"op": "put_playbook", "agent": agent(1), "run": run(1), "number": number, "steps": steps}),
    )
}

pub fn accepted() -> CoordinatorReply {
    CoordinatorReply::parse(br#"{"ok":"yes"}"#).expect("an accepted reply")
}

pub fn rev(n: u32) -> CoordinatorReply {
    CoordinatorReply::parse(format!(r#"{{"rev":{n}}}"#).as_bytes()).expect("a rev reply")
}

/// A host over a memory store that counts the typed commits of one request and, with `stop_after` set to n, ends
/// the request behind its n-th commit: that commit is stored and answers `internal`, and every later commit or state
/// call stores nothing and answers `internal`, as a request whose process stops behind its store (S-69 (f)).
pub struct Stopper<'a> {
    pub store: &'a mut MemoryStore,
    pub commits: u32,
    pub stop_after: Option<u32>,
    pub stopped: bool,
}

impl<'a> Stopper<'a> {
    pub fn new(store: &'a mut MemoryStore, stop_after: Option<u32>) -> Stopper<'a> {
        Stopper {
            store,
            commits: 0,
            stop_after,
            stopped: false,
        }
    }
}

impl Host for Stopper<'_> {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        if self.stopped {
            return Err(ErrorName::Internal);
        }
        self.store.commit(records)?;
        self.commits += 1;
        if self.stop_after == Some(self.commits) {
            self.stopped = true;
            return Err(ErrorName::Internal);
        }
        Ok(())
    }

    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: halvo_schema::executable::names::AgentState,
        agent: Option<&AgentId>,
    ) -> Result<(), ErrorName> {
        if self.stopped {
            return Err(ErrorName::Internal);
        }
        self.store.set_state(workflow, state, agent)
    }

    fn rows(&self) -> &dyn RowRead {
        self.store.rows()
    }

    fn workflow(&self, id: &WorkflowId) -> Option<halvo_schema::WorkflowItem> {
        self.store.workflow(id)
    }

    fn day(&self, at: Instant) -> u32 {
        self.store.day(at)
    }
}
