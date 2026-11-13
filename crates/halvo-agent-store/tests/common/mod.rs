#![allow(dead_code)]

use halvo_agent_store::{Host, MemoryStore, Now};
use halvo_policy::preset;
use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{
    ActionClass, AgentBindings, AgentConfig, AgentEffect, AllowedClass, Autonomy, CoordinatorReply,
    RunnerReply,
};
use halvo_schema::{
    AgentId, Category, ErrorName, Instant, RowRead, TypedRecord, Workflow, WorkflowId,
};
use serde_json::{json, Value};

pub const NOW: Now = Now {
    at: Instant(1_794_500_000_000_000_000),
    day: 20_771,
};

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

pub fn coordinator(store: &mut MemoryStore, body: Value) -> Result<CoordinatorReply, ErrorName> {
    halvo_agent_store::coordinator(store, NOW, body.to_string().as_bytes())
}

pub fn runner(store: &mut MemoryStore, body: Value) -> Result<RunnerReply, ErrorName> {
    halvo_agent_store::runner(store, NOW, body.to_string().as_bytes())
}

pub fn create(store: &mut MemoryStore, w: u32, a: u32) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "create_agent", "workflow": wf(w), "agent": agent(a)}),
    )
}

pub fn store_with(category: &str) -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), category, "approved", None));
    create(&mut store, 1, 1).expect("create_agent of the first agent");
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
        "day": 20_770,
        "at": 1_794_400_000_000_000_000_i64,
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
    json!({
        "v": 1,
        "workflow": wf(1),
        "agent": agent(1),
        "run": run(r),
        "day": 20_770,
        "mode": mode,
        "outcome": outcome,
        "playbook": null,
        "steps": 3,
        "prompts": 1,
        "handbacks": 0,
        "confirmations": 1,
        "effects": 1,
        "unknown": 0,
        "model_calls": 0,
        "tokens_in": 0,
        "tokens_out": 0,
        "wall_s": 9,
        "model": null,
    })
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
