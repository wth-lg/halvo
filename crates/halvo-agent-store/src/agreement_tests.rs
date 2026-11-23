//! r3 U7.7: the person's yes for ONE agent to a hosted model, one row of the kind `agent_agreement` under the agent's
//! workflow, written with the agent's ceiling tier by the coordinator body `set_model` (the coordinator's log line
//! 17342). The origins and the model words of these tests are made-up texts; no test speaks to a host. They are unit
//! tests of the library, which the merge gate runs in both jobs, over the store in memory; the helpers below are those
//! of the store's test targets, of which they hold the few that these tests use.

use halvo_schema::executable::names::AgentState;
use halvo_schema::executable::{CoordinatorReply, RunnerReply};
use halvo_schema::{
    AgentId, ErrorName, Instant, RowRead, TypedRecord, Workflow, WorkflowId, WorkflowItem,
};
use serde_json::{json, Value};

use crate::{Host, MemoryStore, Now};

/// The instant of every request of these tests, 2026-11-12T16:13:20Z, as in the store's test targets.
const NOW: Now = Now {
    at: Instant(1_794_500_000_000_000_000),
};

fn id(prefix: char, n: u32) -> String {
    format!("{prefix}{n:031x}")
}

fn wf(n: u32) -> String {
    id('3', n)
}

fn agent(n: u32) -> String {
    id('4', n)
}

fn run(n: u32) -> String {
    id('5', n)
}

fn declared(name: &str) -> ErrorName {
    ErrorName::read(name).expect("a declared error name")
}

fn workflow(id: &str, category: &str, state: &str, agent: Option<&str>) -> Workflow {
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

fn coordinator(store: &mut MemoryStore, body: Value) -> Result<CoordinatorReply, ErrorName> {
    crate::coordinator(store, NOW, body.to_string().as_bytes())
}

fn runner(store: &mut MemoryStore, body: Value) -> Result<RunnerReply, ErrorName> {
    crate::runner(store, NOW, body.to_string().as_bytes())
}

fn rows(store: &MemoryStore, kind: &str) -> Vec<String> {
    let mut out = Vec::new();
    store.for_each(kind, &mut |row| out.push(row.to_owned()));
    out
}

fn create(store: &mut MemoryStore, w: u32, a: u32) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({"op": "create_agent", "workflow": wf(w), "agent": agent(a)}),
    )
}

/// The first agent of an approved workflow of the category, with no run started.
fn unstarted(category: &str) -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(&wf(1), category, "approved", None));
    create(&mut store, 1, 1).expect("create_agent of the first agent");
    store
}

/// `start_run` of the mode `compile` for the run `r` of the first agent.
fn start(store: &mut MemoryStore, r: u32) {
    coordinator(
        store,
        json!({"op": "start_run", "workflow": wf(1), "agent": agent(1), "run": run(r), "mode": "compile"}),
    )
    .expect("start_run of the first agent");
}

/// The first agent of an approved workflow of the category, with its run 1 started.
fn store_with(category: &str) -> MemoryStore {
    let mut store = unstarted(category);
    start(&mut store, 1);
    store
}

const ORIGIN: &str = "https://hosted-model.test";
const SMALL: &str = "test-model-small";
/// A model word with blanks at both ends and inside, which the store keeps as given.
const LARGE: &str = " test model large ";

fn triple(tier: &str, origin: &str, model: &str) -> Value {
    json!({"tier": tier, "origin": origin, "model": model})
}

fn both() -> Value {
    json!([
        triple("low_cost_hosted", ORIGIN, SMALL),
        triple("frontier", ORIGIN, LARGE)
    ])
}

/// `set_model` of the agent `a` of the workflow `w` with the tier and the agreement.
fn set_model(
    store: &mut dyn Host,
    w: u32,
    a: u32,
    tier: &str,
    agreement: Value,
) -> Result<String, ErrorName> {
    let body = json!({
        "op": "set_model", "workflow": wf(w), "agent": agent(a), "tier": tier, "agreement": agreement,
    });
    crate::coordinator(store, NOW, body.to_string().as_bytes())
        .map(|r| r.to_json().expect("the reply's JSON"))
}

fn the_way_back(store: &mut dyn Host, w: u32, a: u32) -> Result<String, ErrorName> {
    set_model(store, w, a, "on_device", Value::Null)
}

fn rev_text(n: u32) -> Result<String, ErrorName> {
    Ok(format!(r#"{{"rev":{n}}}"#))
}

fn parsed(rows: Vec<String>) -> Vec<Value> {
    rows.iter()
        .map(|r| serde_json::from_str(r).expect("a row's JSON"))
        .collect()
}

/// The stored revisions of the agent's set-up, in the order of `rev`.
fn revisions(store: &MemoryStore, a: u32) -> Vec<Value> {
    let mut all: Vec<Value> = parsed(rows(store, "agent_config"))
        .into_iter()
        .filter(|c| c["agent"] == json!(agent(a)))
        .collect();
    all.sort_by_key(|c| c["rev"].as_u64());
    all
}

/// The entry of the agent `a` in the roster of `list_agents`.
fn entry(store: &mut MemoryStore, a: u32) -> Value {
    let body = json!({"op": "list_agents", "workflow": null, "agent": null, "after": null});
    let reply = coordinator(store, body).expect("list_agents");
    let roster: Value = serde_json::from_str(&reply.to_json().expect("JSON")).expect("JSON");
    roster["agents"]
        .as_array()
        .expect("a list of agents")
        .iter()
        .find(|e| e["config"]["agent"] == json!(agent(a)))
        .cloned()
        .unwrap_or_else(|| panic!("no entry of the agent {a}: {roster}"))
}

/// The key `agreement` of the entry, which the entry must hold.
fn agreement_of(entry: &Value) -> Value {
    entry
        .as_object()
        .expect("an entry")
        .get("agreement")
        .cloned()
        .unwrap_or_else(|| panic!("the entry holds no key agreement: {entry}"))
}

/// A host over a memory store that records each write of a request (a typed commit or a removal call) with the
/// kinds of its records, and hands every call on.
struct Writes<'a> {
    store: &'a mut MemoryStore,
    writes: Vec<String>,
}

fn kinds(records: &[TypedRecord]) -> String {
    records.iter().map(|r| r.kind).collect::<Vec<_>>().join(",")
}

impl Host for Writes<'_> {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.writes.push(format!("commit [{}]", kinds(&records)));
        self.store.commit(records)
    }

    fn remove_under(
        &mut self,
        kind: &str,
        workflow: &WorkflowId,
        records: Vec<TypedRecord>,
    ) -> Result<u64, ErrorName> {
        self.writes.push(format!(
            "remove_under {kind} {} [{}]",
            workflow.as_str(),
            kinds(&records)
        ));
        self.store.remove_under(kind, workflow, records)
    }

    fn remove_person(&mut self, kind: &str, records: Vec<TypedRecord>) -> Result<u64, ErrorName> {
        self.writes
            .push(format!("remove_person {kind} [{}]", kinds(&records)));
        self.store.remove_person(kind, records)
    }

    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: AgentState,
        agent: Option<&AgentId>,
    ) -> Result<(), ErrorName> {
        self.writes.push("set_state".to_owned());
        self.store.set_state(workflow, state, agent)
    }

    fn rows(&self) -> &dyn RowRead {
        self.store.rows()
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.store.workflow(id)
    }

    fn day(&self, at: Instant) -> u32 {
        self.store.day(at)
    }
}

/// Sentence (1): a yes writes the ceiling and its agreement in one write, and the roster hands both. The agent of a
/// web workflow says yes up to `frontier` with a triple for each hosted tier; the body makes ONE write, the removal
/// call of `agent_agreement` under the agent's workflow with the new revision of the set-up and the new row, and
/// answers revision 2. Revision 2 is revision 1 with `rev` 2 and the tier `frontier` and every other key unchanged;
/// ONE agreement row stands, with the workflow, the agent, the ceiling, the triples as given (blanks and all) and the
/// request's instant; and the roster's entry hands the set-up of revision 2 and, under `agreement`, the tier and the
/// triples.
#[test]
fn a_yes_writes_the_ceiling_and_its_agreement_and_the_roster_hands_both() {
    let mut store = unstarted("utility_bill_payment");
    let mut host = Writes {
        store: &mut store,
        writes: Vec::new(),
    };
    let reply = set_model(&mut host, 1, 1, "frontier", both());
    let writes = host.writes;
    assert_eq!(reply, rev_text(2), "the reply of the yes");
    assert_eq!(
        writes,
        vec![format!(
            "remove_under agent_agreement {} [agent_config,agent_agreement]",
            wf(1)
        )],
        "the writes of the yes"
    );
    let revs = revisions(&store, 1);
    assert_eq!(revs.len(), 2, "{revs:?}");
    let mut expected = revs[0].clone();
    expected["rev"] = json!(2);
    expected["tier"] = json!("frontier");
    assert_eq!(revs[1], expected, "revision 2 of the set-up");
    let stored = parsed(rows(&store, "agent_agreement"));
    assert_eq!(
        stored,
        vec![json!({
            "v": 1, "workflow": wf(1), "agent": agent(1), "tier": "frontier", "hosts": both(),
            "at": NOW.at.0,
        })],
        "the agreement rows"
    );
    let listed = entry(&mut store, 1);
    assert_eq!(listed["config"]["rev"], json!(2));
    assert_eq!(listed["config"]["tier"], json!("frontier"));
    assert_eq!(
        agreement_of(&listed),
        json!({"tier": "frontier", "hosts": both()}),
        "the roster's agreement"
    );
}

/// Sentence (3): a hosted tier passes only with its agreement, and a list that point 2 refuses is refused. On a web
/// agent: a hosted tier with no agreement and `on_device` with a list are refused with `config` (validate's `tier`);
/// a list with no triple of the ceiling, with a tier above the ceiling, with a tier twice, with `on_device` in a
/// triple, or with an empty origin or model word is refused as `malformed`, and an origin or a model word of 1,025
/// bytes with `length`. A hosted tier for an agent whose runtime is `none` is refused with `config`. After every
/// refusal the set-ups and the agreement rows are what they were. A list whose words are 1,024 bytes each passes.
#[test]
fn a_hosted_tier_without_its_agreement_is_refused() {
    let mut store = unstarted("utility_bill_payment");
    store.put_workflow(workflow(&wf(2), "tenant_rent", "approved", None));
    create(&mut store, 2, 2).expect("create_agent of the agent with no runtime");
    let config = Err(declared("config"));
    let length = Err(declared("length"));
    let malformed = Err(ErrorName::Malformed);
    let long = "o".repeat(1025);
    let frontier = |origin: &str, model: &str| triple("frontier", origin, model);
    let cases: Vec<(u32, &str, Value, Result<String, ErrorName>)> = vec![
        (1, "frontier", Value::Null, config.clone()),
        (1, "low_cost_hosted", Value::Null, config.clone()),
        (
            1,
            "on_device",
            json!([frontier(ORIGIN, LARGE)]),
            config.clone(),
        ),
        (
            1,
            "frontier",
            json!([triple("low_cost_hosted", ORIGIN, SMALL)]),
            malformed.clone(),
        ),
        (1, "low_cost_hosted", both(), malformed.clone()),
        (
            1,
            "frontier",
            json!([frontier(ORIGIN, LARGE), frontier(ORIGIN, SMALL)]),
            malformed.clone(),
        ),
        (
            1,
            "frontier",
            json!([triple("on_device", ORIGIN, SMALL), frontier(ORIGIN, LARGE)]),
            malformed.clone(),
        ),
        (
            1,
            "frontier",
            json!([frontier("", LARGE)]),
            malformed.clone(),
        ),
        (
            1,
            "frontier",
            json!([frontier(ORIGIN, "")]),
            malformed.clone(),
        ),
        (
            1,
            "frontier",
            json!([frontier(&long, LARGE)]),
            length.clone(),
        ),
        (
            1,
            "frontier",
            json!([frontier(ORIGIN, &long)]),
            length.clone(),
        ),
        (
            2,
            "frontier",
            json!([frontier(ORIGIN, LARGE)]),
            config.clone(),
        ),
    ];
    let before = (
        rows(&store, "agent_config"),
        rows(&store, "agent_agreement"),
    );
    for (a, tier, agreement, refused) in cases {
        let shown = format!("agent {a}, {tier}, {agreement}");
        assert_eq!(
            set_model(&mut store, a, a, tier, agreement),
            refused,
            "{shown}"
        );
        assert_eq!(
            (
                rows(&store, "agent_config"),
                rows(&store, "agent_agreement")
            ),
            before,
            "the rows after {shown}"
        );
    }
    let word = "w".repeat(1024);
    assert_eq!(
        set_model(
            &mut store,
            1,
            1,
            "frontier",
            json!([frontier(&word, &word)])
        ),
        rev_text(2),
        "a list whose words are 1,024 bytes each"
    );
}

/// Sentence (4): a set-up from before reads with no agreement. The roster's entry of an agent whose set-up is the
/// preset that `create_agent` stored, with no agreement row, holds the key `agreement` with null; so does the entry
/// of an agent whose newest set-up holds a hosted tier with no row of that tier: none at all, and then a row of
/// another tier.
#[test]
fn a_set_up_from_before_reads_with_no_agreement() {
    let mut store = unstarted("utility_bill_payment");
    let listed = entry(&mut store, 1);
    assert_eq!(listed["config"]["tier"], json!("on_device"));
    assert_eq!(agreement_of(&listed), Value::Null, "the preset's entry");

    let mut hosted = revisions(&store, 1)[0].clone();
    hosted["rev"] = json!(2);
    hosted["tier"] = json!("frontier");
    store
        .commit(vec![TypedRecord {
            kind: "agent_config",
            json: hosted.to_string(),
        }])
        .expect("a hosted revision stored with no row");
    let listed = entry(&mut store, 1);
    assert_eq!(listed["config"]["tier"], json!("frontier"));
    assert_eq!(
        agreement_of(&listed),
        Value::Null,
        "a hosted tier with no row"
    );

    let other = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "tier": "low_cost_hosted",
        "hosts": [triple("low_cost_hosted", ORIGIN, SMALL)], "at": NOW.at.0,
    });
    store
        .commit(vec![TypedRecord {
            kind: "agent_agreement",
            json: other.to_string(),
        }])
        .expect("a row of another tier");
    assert_eq!(
        agreement_of(&entry(&mut store, 1)),
        Value::Null,
        "a row of another tier than the newest set-up's"
    );
}

/// Sentence (5): every reader reads the newest revision of a set-up. After a yes the runner's `load` of a run started
/// behind it answers revision 2 with the tier `frontier`, and the roster's entry holds the same revision.
#[test]
fn a_second_revision_of_a_set_up_is_what_the_runner_loads() {
    let mut store = unstarted("utility_bill_payment");
    assert_eq!(set_model(&mut store, 1, 1, "frontier", both()), rev_text(2));
    start(&mut store, 1);
    let loaded = runner(
        &mut store,
        json!({"op": "load", "agent": agent(1), "run": run(1)}),
    )
    .expect("load");
    let loaded: Value = serde_json::from_str(&loaded.to_json().expect("JSON")).expect("JSON");
    let newest = revisions(&store, 1).pop().expect("a set-up");
    assert_eq!(newest["rev"], json!(2));
    assert_eq!(loaded["config"], newest, "the set-up that load answers");
    assert_eq!(entry(&mut store, 1)["config"]["rev"], json!(2));
}

/// Sentence (9): a yes and the way back are taken while a run of the agent is open. With run 1 started and no record
/// of it stored, `drop_sessions` answers `state`; `set_model` takes the yes (revision 2, one agreement row) and then
/// the way back (revision 3, `on_device`, no agreement row), and the roster hands no agreement.
#[test]
fn the_way_back_is_taken_while_a_run_is_open() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        coordinator(
            &mut store,
            json!({"op": "drop_sessions", "workflow": wf(1), "agent": agent(1)})
        ),
        Err(ErrorName::State),
        "the run is open"
    );
    assert_eq!(set_model(&mut store, 1, 1, "frontier", both()), rev_text(2));
    assert_eq!(rows(&store, "agent_agreement").len(), 1);
    assert_eq!(the_way_back(&mut store, 1, 1), rev_text(3));
    assert_eq!(revisions(&store, 1)[2]["tier"], json!("on_device"));
    assert_eq!(rows(&store, "agent_agreement"), Vec::<String>::new());
    assert_eq!(agreement_of(&entry(&mut store, 1)), Value::Null);
}
