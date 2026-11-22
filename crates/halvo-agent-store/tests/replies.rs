mod common;

use common::*;
use halvo_agent_store::MemoryStore;
use halvo_schema::executable::body::{LoadRepliesReply, CHECKPOINTS_REPLY_MAX};
use halvo_schema::executable::{AgentReply, RunnerReply};
use halvo_schema::{ErrorName, TypedRecord, WorkflowId};
use serde_json::{json, Value};

const STATES: [&str; 6] = [
    "drafted",
    "begun",
    "committed",
    "unknown",
    "lost",
    "withheld",
];

fn reply(
    a: u32,
    r: u32,
    seq: u32,
    message: &str,
    draft: Option<&str>,
    effect_n: Option<u32>,
    state: &str,
) -> Value {
    json!({
        "v": 1,
        "workflow": wf(a),
        "agent": agent(a),
        "run": run(r),
        "seq": seq,
        "day": ROW_DAY,
        "at": ROW_AT,
        "message": message,
        "draft": draft,
        "effect": effect_n.map(effect),
        "state": state,
    })
}

fn after_drafted(a: u32, r: u32, seq: u32, message: &str, state: &str) -> Value {
    match state {
        "begun" | "committed" | "unknown" => reply(a, r, seq, message, Some("d1"), Some(1), state),
        "lost" => reply(a, r, seq, message, None, None, state),
        _ => reply(a, r, seq, message, Some("d1"), None, state),
    }
}

fn put_reply(store: &mut MemoryStore, row: Value) -> Result<RunnerReply, ErrorName> {
    let body = json!({
        "op": "reply_row",
        "agent": row["agent"].clone(),
        "run": row["run"].clone(),
        "record": row,
    });
    runner(store, body)
}

fn replies(store: &MemoryStore) -> Vec<AgentReply> {
    rows(store, "agent_reply")
        .iter()
        .map(|r| AgentReply::parse(r.as_bytes()).expect("a stored reply row"))
        .collect()
}

fn newest(store: &MemoryStore, a: u32, message: &str) -> Option<AgentReply> {
    replies(store)
        .into_iter()
        .filter(|r| r.agent.as_str() == agent(a) && r.message.as_str() == message)
        .max_by_key(|r| r.seq)
}

fn plant(store: &mut MemoryStore, rows: Vec<Value>) {
    store
        .typed_commit(
            rows.into_iter()
                .map(|row| TypedRecord {
                    kind: "agent_reply",
                    json: AgentReply::parse(row.to_string().as_bytes())
                        .expect("a planted reply row")
                        .to_json()
                        .expect("its text"),
                })
                .collect(),
        )
        .expect("the planted rows are stored");
}

fn agents(n: u32) -> MemoryStore {
    let mut store = MemoryStore::new();
    for k in 1..=n {
        store.put_workflow(workflow(&wf(k), "email", "approved", None));
        create(&mut store, k, k).expect("create_agent");
        start_at(&mut store, NOW, k, k, k).expect("start_run");
    }
    store
}

fn two_agents() -> MemoryStore {
    agents(2)
}

fn ok(store: &mut MemoryStore, row: Value) {
    assert_eq!(put_reply(store, row.clone()), Ok(stored(1)), "{row}");
}

fn refused(store: &mut MemoryStore, row: Value) {
    let before = rows(store, "agent_reply");
    assert_eq!(
        put_reply(store, row.clone()),
        Err(declared("effect")),
        "{row}"
    );
    assert_eq!(rows(store, "agent_reply"), before, "{row}");
}

#[test]
fn reply_row_stores_a_first_drafted_row() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    let stored = replies(&store);
    assert_eq!(stored.len(), 1);
    let text = stored[0].to_json().expect("the stored row's text");
    assert_eq!(
        serde_json::from_str::<Value>(&text).expect("json"),
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted")
    );
}

#[test]
fn a_first_row_that_is_not_drafted_is_refused_and_stores_nothing() {
    let mut store = two_agents();
    for state in ["begun", "committed", "unknown", "lost", "withheld"] {
        refused(&mut store, after_drafted(1, 1, 1, "m1", state));
    }
    refused(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), None, "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), Some(1), "drafted"),
    );
    assert!(replies(&store).is_empty());
}

#[test]
fn the_closed_order_of_one_message_is_stored_row_by_row() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
    ok(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(1), "committed"),
    );
    ok(
        &mut store,
        reply(1, 1, 1, "m2", Some("d2"), None, "drafted"),
    );
    ok(&mut store, reply(1, 1, 2, "m2", None, None, "lost"));
    ok(
        &mut store,
        reply(1, 1, 3, "m2", Some("d3"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 4, "m2", Some("d3"), Some(2), "begun"),
    );
    ok(
        &mut store,
        reply(1, 1, 5, "m2", Some("d3"), Some(2), "unknown"),
    );
    assert_eq!(replies(&store).len(), 8);
    assert_eq!(newest(&store, 1, "m1").map(|r| r.seq.get()), Some(3));
    assert_eq!(newest(&store, 1, "m2").map(|r| r.seq.get()), Some(5));
}

#[test]
fn a_drafted_or_a_lost_row_after_begun_is_refused_and_stores_nothing() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
    refused(&mut store, reply(1, 1, 3, "m1", None, None, "lost"));
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), None, "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(1), "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d9"), Some(1), "drafted"),
    );
    assert_eq!(newest(&store, 1, "m1").map(|r| r.seq.get()), Some(2));
}

#[test]
fn every_row_after_committed_or_unknown_is_refused_and_stores_nothing() {
    for end in ["committed", "unknown"] {
        let mut store = two_agents();
        ok(
            &mut store,
            reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
        );
        ok(
            &mut store,
            reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
        );
        ok(&mut store, reply(1, 1, 3, "m1", Some("d1"), Some(1), end));
        for state in STATES {
            refused(&mut store, after_drafted(1, 1, 4, "m1", state));
        }
        refused(
            &mut store,
            reply(1, 1, 4, "m1", Some("d2"), None, "drafted"),
        );
        assert_eq!(replies(&store).len(), 3, "{end}");
    }
}

#[test]
fn a_row_that_skips_or_repeats_a_seq_is_refused() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(1), "begun"),
    );
    refused(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), Some(1), "begun"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
}

#[test]
fn a_repeat_of_the_newest_row_is_answered_as_the_first_and_stores_nothing() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
    let mut again = reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun");
    again["at"] = json!(ROW_AT + 5);
    ok(&mut store, again);
    assert_eq!(replies(&store).len(), 2);
    refused(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
}

#[test]
fn a_begun_row_keeps_the_draft_of_its_drafted_row_and_committed_or_unknown_keep_the_begun_row() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 2, "m1", Some("d2"), Some(1), "begun"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
    for state in ["committed", "unknown"] {
        refused(&mut store, reply(1, 1, 3, "m1", Some("d2"), Some(1), state));
        refused(&mut store, reply(1, 1, 3, "m1", Some("d1"), Some(2), state));
    }
    start_at(&mut store, NOW, 1, 1, 3).expect("start_run");
    for state in ["committed", "unknown"] {
        refused(&mut store, reply(1, 3, 3, "m1", Some("d1"), Some(1), state));
    }
}

#[test]
fn a_drafted_row_after_lost_needs_a_new_draft_and_no_effect() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    ok(&mut store, reply(1, 1, 2, "m1", None, None, "lost"));
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), None, "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d2"), Some(1), "drafted"),
    );
    refused(&mut store, reply(1, 1, 3, "m1", None, None, "lost"));
    ok(
        &mut store,
        reply(1, 1, 3, "m1", Some("d2"), None, "drafted"),
    );
}

#[test]
fn a_drafted_row_after_begun_is_refused_unless_its_effect_s_newest_row_is_not_made() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    put(&mut store, row(1, 1, 1, "held", "send", 1, None, None)).expect("held");
    put(
        &mut store,
        row(1, 1, 2, "approved", "send", 1, Some(1), None),
    )
    .expect("approved");
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
    put(
        &mut store,
        row(1, 1, 3, "releasing", "send", 1, Some(1), None),
    )
    .expect("releasing");
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(1), "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), None, "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(2), "drafted"),
    );
    start_at(&mut store, NOW, 1, 1, 3).expect("start_run");
    put(&mut store, row(3, 3, 1, "held", "send", 1, None, None)).expect("held of run 3");
    ok(
        &mut store,
        reply(1, 1, 1, "m2", Some("d2"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m2", Some("d2"), Some(3), "begun"),
    );
    refused(
        &mut store,
        reply(1, 1, 3, "m2", Some("d2"), Some(3), "drafted"),
    );
    assert_eq!(newest(&store, 1, "m1").map(|r| r.seq.get()), Some(2));
    put(
        &mut store,
        row(
            1,
            1,
            4,
            "not_made",
            "send",
            1,
            Some(1),
            Some("unauthorized"),
        ),
    )
    .expect("not_made");
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(2), "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(1), "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 4, "m1", Some("d1"), Some(2), "begun"),
    );
    assert_eq!(
        newest(&store, 1, "m1").map(|r| (r.seq.get(), r.state.as_str())),
        Some((4, "begun"))
    );
}

#[test]
fn a_begun_row_that_names_again_a_not_made_effect_is_refused() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    put(&mut store, row(1, 1, 1, "held", "send", 1, None, None)).expect("held");
    put(
        &mut store,
        row(1, 1, 2, "approved", "send", 1, Some(1), None),
    )
    .expect("approved");
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
    put(
        &mut store,
        row(1, 1, 3, "releasing", "send", 1, Some(1), None),
    )
    .expect("releasing");
    put(
        &mut store,
        row(
            1,
            1,
            4,
            "not_made",
            "send",
            1,
            Some(1),
            Some("unauthorized"),
        ),
    )
    .expect("not_made");
    ok(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(1), "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 4, "m1", Some("d1"), Some(1), "begun"),
    );
    assert_eq!(
        newest(&store, 1, "m1").map(|r| (r.seq.get(), r.state.as_str())),
        Some((3, "drafted"))
    );
}

#[test]
fn withheld_is_allowed_after_a_first_drafted_row() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), None, "withheld"),
    );
}

#[test]
fn withheld_is_allowed_after_the_drafted_row_that_follows_lost() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    ok(&mut store, reply(1, 1, 2, "m1", None, None, "lost"));
    ok(
        &mut store,
        reply(1, 1, 3, "m1", Some("d2"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 4, "m1", Some("d2"), None, "withheld"),
    );
}

#[test]
fn withheld_is_refused_after_the_drafted_row_that_follows_begun() {
    let mut store = two_agents();
    plant(
        &mut store,
        vec![
            reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
            reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
            reply(1, 1, 3, "m1", Some("d1"), Some(1), "drafted"),
        ],
    );
    refused(
        &mut store,
        reply(1, 1, 4, "m1", Some("d1"), None, "withheld"),
    );
}

#[test]
fn withheld_as_a_first_row_after_begun_and_after_lost_is_refused() {
    let mut store = two_agents();
    refused(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "withheld"),
    );
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), None, "withheld"),
    );
    ok(
        &mut store,
        reply(1, 1, 1, "m2", Some("d2"), None, "drafted"),
    );
    ok(&mut store, reply(1, 1, 2, "m2", None, None, "lost"));
    refused(
        &mut store,
        reply(1, 1, 3, "m2", Some("d2"), None, "withheld"),
    );
}

#[test]
fn withheld_keeps_the_draft_and_the_run_of_its_drafted_row() {
    let mut store = two_agents();
    start_at(&mut store, NOW, 1, 1, 3).expect("start_run");
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 2, "m1", Some("d2"), None, "withheld"),
    );
    refused(
        &mut store,
        reply(1, 3, 2, "m1", Some("d1"), None, "withheld"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), None, "withheld"),
    );
}

#[test]
fn every_row_after_withheld_is_refused_and_stores_nothing() {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    ok(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), None, "withheld"),
    );
    for state in STATES {
        refused(&mut store, after_drafted(1, 1, 3, "m1", state));
    }
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d2"), None, "drafted"),
    );
    assert_eq!(replies(&store).len(), 2);
}

#[test]
fn a_reply_row_is_bound_to_its_agent_s_workflow_and_started_open_run() {
    let mut store = two_agents();
    let mut other = reply(1, 1, 1, "m1", Some("d1"), None, "drafted");
    other["workflow"] = json!(wf(2));
    assert_eq!(put_reply(&mut store, other), Err(declared("binding")));
    assert_eq!(
        put_reply(
            &mut store,
            reply(1, 7, 1, "m1", Some("d1"), None, "drafted")
        ),
        Err(declared("binding"))
    );
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    end_run(&mut store, 1).expect("end_run");
    assert_eq!(
        put_reply(
            &mut store,
            reply(1, 1, 1, "m2", Some("d2"), None, "drafted")
        ),
        Err(declared("effect"))
    );
    assert_eq!(replies(&store).len(), 1);
}

#[test]
fn a_reply_row_is_held_to_the_day_of_its_at_and_to_the_clock() {
    let mut store = two_agents();
    let mut wrong_day = reply(1, 1, 1, "m1", Some("d1"), None, "drafted");
    wrong_day["day"] = json!(ROW_DAY + 1);
    let mut ahead = reply(1, 1, 1, "m1", Some("d1"), None, "drafted");
    ahead["day"] = json!(TODAY + 1);
    ahead["at"] = json!(NOW.at.0 + NANOS_PER_DAY);
    for bad in [wrong_day, ahead] {
        assert_eq!(
            put_reply(&mut store, bad.clone()),
            Err(declared("day")),
            "{bad}"
        );
    }
    assert!(replies(&store).is_empty());
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
}

fn ended_unknown(a: u32, r: u32, seq: u32, message: &str, draft: &str, effect_n: u32) -> Value {
    let mut row = reply(a, r, seq, message, Some(draft), Some(effect_n), "unknown");
    row["day"] = json!(TODAY);
    row["at"] = json!(NOW.at.0);
    row
}

fn as_value(row: &AgentReply) -> Value {
    serde_json::from_str(&row.to_json().expect("the row's text")).expect("json")
}

fn begun_of_run_1(store: &mut MemoryStore) {
    ok(store, reply(1, 1, 1, "m1", Some("d1"), None, "drafted"));
    ok(store, reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"));
}

fn finish_body(r: u32) -> Value {
    let record = run_record(r, Some("replay"), "failed");
    json!({"op": "finish", "agent": agent(1), "run": run(r), "record": record})
}

#[test]
fn finish_leaves_a_begun_row_of_its_run_unknown() {
    let mut store = two_agents();
    begun_of_run_1(&mut store);
    assert_eq!(runner(&mut store, finish_body(1)), Ok(stored(1)));
    let newest = newest(&store, 1, "m1").expect("a row");
    assert_eq!(as_value(&newest), ended_unknown(1, 1, 3, "m1", "d1", 1));
    assert_eq!(replies(&store).len(), 3);
}

#[test]
fn end_run_leaves_a_begun_row_of_its_run_unknown() {
    let mut store = two_agents();
    begun_of_run_1(&mut store);
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let newest = newest(&store, 1, "m1").expect("a row");
    assert_eq!(as_value(&newest), ended_unknown(1, 1, 3, "m1", "d1", 1));
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert_eq!(replies(&store).len(), 3);
}

#[test]
fn a_run_s_end_leaves_every_other_row_as_it_stands() {
    for end in ["finish", "end_run"] {
        let mut store = two_agents();
        start_at(&mut store, NOW, 1, 1, 3).expect("start_run");
        ok(
            &mut store,
            reply(1, 1, 1, "m2", Some("d2"), None, "drafted"),
        );
        ok(
            &mut store,
            reply(1, 1, 1, "m3", Some("d3"), None, "drafted"),
        );
        ok(
            &mut store,
            reply(1, 1, 2, "m3", Some("d3"), None, "withheld"),
        );
        ok(
            &mut store,
            reply(1, 1, 1, "m4", Some("d4"), None, "drafted"),
        );
        ok(&mut store, reply(1, 1, 2, "m4", None, None, "lost"));
        ok(
            &mut store,
            reply(1, 1, 1, "m5", Some("d5"), None, "drafted"),
        );
        ok(
            &mut store,
            reply(1, 1, 2, "m5", Some("d5"), Some(5), "begun"),
        );
        ok(
            &mut store,
            reply(1, 1, 3, "m5", Some("d5"), Some(5), "committed"),
        );
        ok(
            &mut store,
            reply(1, 3, 1, "m6", Some("d6"), None, "drafted"),
        );
        ok(
            &mut store,
            reply(1, 3, 2, "m6", Some("d6"), Some(6), "begun"),
        );
        ok(
            &mut store,
            reply(2, 2, 1, "m7", Some("d7"), None, "drafted"),
        );
        ok(
            &mut store,
            reply(2, 2, 2, "m7", Some("d7"), Some(7), "begun"),
        );
        let before = rows(&store, "agent_reply");
        match end {
            "finish" => assert_eq!(runner(&mut store, finish_body(1)), Ok(stored(1))),
            _ => assert_eq!(end_run(&mut store, 1), Ok(accepted())),
        }
        assert_eq!(rows(&store, "agent_reply"), before, "{end}");
    }
}

#[test]
fn the_unknown_rows_of_a_run_s_end_are_in_the_one_commit_of_its_record() {
    for end in ["finish", "end_run"] {
        let mut store = two_agents();
        begun_of_run_1(&mut store);
        ok(
            &mut store,
            reply(1, 1, 1, "m2", Some("d2"), None, "drafted"),
        );
        ok(
            &mut store,
            reply(1, 1, 2, "m2", Some("d2"), Some(2), "begun"),
        );
        let mut host = Stopper::new(&mut store, Some(1));
        let answer = match end {
            "finish" => {
                halvo_agent_store::runner(&mut host, NOW, finish_body(1).to_string().as_bytes())
                    .map(|_| ())
            }
            _ => halvo_agent_store::coordinator(
                &mut host,
                NOW,
                json!({"op": "end_run", "workflow": wf(1), "agent": agent(1), "run": run(1)})
                    .to_string()
                    .as_bytes(),
            )
            .map(|_| ()),
        };
        assert_eq!(answer, Err(ErrorName::Internal), "{end}");
        assert_eq!(host.commits, 1, "{end}");
        assert_eq!(rows(&store, "agent_run").len(), 1, "{end}");
        assert_eq!(
            newest(&store, 1, "m1").map(|r| r.state.as_str()),
            Some("unknown"),
            "{end}"
        );
        assert_eq!(
            newest(&store, 1, "m2").map(|r| r.state.as_str()),
            Some("unknown"),
            "{end}"
        );
    }
}

#[test]
fn after_a_run_s_end_its_reply_rows_are_refused() {
    let mut store = two_agents();
    begun_of_run_1(&mut store);
    end_run(&mut store, 1).expect("end_run");
    refused(
        &mut store,
        reply(1, 1, 3, "m1", Some("d1"), Some(1), "committed"),
    );
    refused(
        &mut store,
        reply(1, 1, 4, "m1", Some("d1"), Some(1), "committed"),
    );
}

fn load_replies(
    store: &mut MemoryStore,
    a: u32,
    r: u32,
    ids: &[&str],
) -> Result<RunnerReply, ErrorName> {
    runner(
        store,
        json!({"op": "load_replies", "agent": agent(a), "run": run(r), "message_ids": ids}),
    )
}

fn answer(store: &mut MemoryStore, a: u32, ids: &[&str]) -> LoadRepliesReply {
    match load_replies(store, a, a, ids) {
        Ok(RunnerReply::Replies(reply)) => reply,
        other => panic!("a reply of load_replies, not {other:?}"),
    }
}

fn ids_of(list: &[halvo_schema::executable::ProviderId]) -> Vec<&str> {
    list.iter().map(|id| id.as_str()).collect()
}

fn messages(reply: &LoadRepliesReply) -> Vec<(&str, u32)> {
    reply
        .replies
        .iter()
        .map(|r| (r.message.as_str(), r.seq.get()))
        .collect()
}

fn other_agent_holds(store: &mut MemoryStore, message: &str, state: &str) {
    let mut chain = vec![reply(2, 2, 1, message, Some("dz"), None, "drafted")];
    match state {
        "drafted" => {}
        "lost" => chain.push(reply(2, 2, 2, message, None, None, "lost")),
        "withheld" => chain.push(reply(2, 2, 2, message, Some("dz"), None, "withheld")),
        "begun" => chain.push(reply(2, 2, 2, message, Some("dz"), Some(9), "begun")),
        end => {
            chain.push(reply(2, 2, 2, message, Some("dz"), Some(9), "begun"));
            chain.push(reply(2, 2, 3, message, Some("dz"), Some(9), end));
        }
    }
    for row in chain {
        ok(store, row);
    }
}

#[test]
fn load_replies_answers_the_newest_own_row_of_each_id_in_the_order_asked() {
    let mut store = two_agents();
    begun_of_run_1(&mut store);
    ok(
        &mut store,
        reply(1, 1, 1, "m2", Some("d2"), None, "drafted"),
    );
    let reply = answer(&mut store, 1, &["m2", "m3", "m1"]);
    assert_eq!(messages(&reply), [("m2", 1), ("m1", 2)]);
    assert!(reply.blocked.is_empty() && reply.blocked_withheld.is_empty());
}

#[test]
fn an_id_whose_only_row_is_another_agent_s_begun_committed_or_unknown_is_blocked_and_no_key_of_it_leaves(
) {
    for state in ["begun", "committed", "unknown"] {
        let mut store = two_agents();
        other_agent_holds(&mut store, "m1", state);
        let reply = answer(&mut store, 1, &["m1", "m2"]);
        assert!(reply.replies.is_empty(), "{state}");
        assert_eq!(ids_of(&reply.blocked), ["m1"], "{state}");
        assert!(reply.blocked_withheld.is_empty(), "{state}");
        let text = RunnerReply::Replies(reply)
            .to_json()
            .expect("the reply's text");
        for key in [
            wf(2),
            agent(2),
            run(2),
            effect(9),
            "dz".to_owned(),
            state.to_owned(),
        ] {
            assert!(!text.contains(&key), "{state}: {key} in {text}");
        }
    }
}

#[test]
fn another_agent_s_drafted_or_lost_row_is_answered_nowhere() {
    for state in ["drafted", "lost"] {
        let mut store = two_agents();
        other_agent_holds(&mut store, "m1", state);
        let reply = answer(&mut store, 1, &["m1"]);
        assert!(reply.replies.is_empty(), "{state}");
        assert!(reply.blocked.is_empty(), "{state}");
        assert!(reply.blocked_withheld.is_empty(), "{state}");
    }
}

#[test]
fn another_agent_s_withheld_row_is_in_blocked_withheld_unless_one_holds_a_sent_state() {
    let mut store = agents(3);
    other_agent_holds(&mut store, "m1", "withheld");
    other_agent_holds(&mut store, "m2", "withheld");
    for row in [
        reply(3, 3, 1, "m2", Some("dy"), None, "drafted"),
        reply(3, 3, 2, "m2", Some("dy"), Some(8), "begun"),
    ] {
        ok(&mut store, row);
    }
    let reply = answer(&mut store, 1, &["m1", "m2"]);
    assert_eq!(ids_of(&reply.blocked), ["m2"]);
    assert_eq!(ids_of(&reply.blocked_withheld), ["m1"]);
}

#[test]
fn the_ids_are_blocked_also_with_the_other_agent_paused_and_with_its_workflow_not_stored() {
    for case in ["paused", "not_stored"] {
        let mut store = two_agents();
        other_agent_holds(&mut store, "m1", "committed");
        other_agent_holds(&mut store, "m2", "withheld");
        other_agent_holds(&mut store, "m3", "drafted");
        match case {
            "paused" => {
                coordinator(
                    &mut store,
                    json!({"op": "set_state", "workflow": wf(2), "agent": agent(2), "state": "paused"}),
                )
                .expect("paused");
            }
            _ => store.remove_workflow(&WorkflowId::parse(&wf(2)).expect("an id")),
        }
        let reply = answer(&mut store, 1, &["m1", "m2", "m3"]);
        assert_eq!(ids_of(&reply.blocked), ["m1"], "{case}");
        assert_eq!(ids_of(&reply.blocked_withheld), ["m2"], "{case}");
        assert!(reply.replies.is_empty(), "{case}");
    }
}

#[test]
fn the_rows_of_one_agent_are_ordered_by_seq_and_two_agents_rows_are_not_ordered_by_their_instants()
{
    let mut answers = Vec::new();
    for shift in [1, -1] {
        let mut store = two_agents();
        let at = |k: i64| json!(ROW_AT + 3_600_000_000_000 + shift * k * 1_000_000_000);
        let mut rows_of = vec![
            reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
            reply(2, 2, 1, "m1", Some("dz"), None, "drafted"),
            reply(2, 2, 2, "m1", Some("dz"), Some(9), "begun"),
            reply(1, 1, 1, "m2", Some("d2"), None, "drafted"),
            reply(1, 1, 2, "m2", Some("d2"), Some(2), "begun"),
            reply(2, 2, 1, "m2", Some("dy"), None, "drafted"),
        ];
        for (k, row) in rows_of.iter_mut().enumerate() {
            row["at"] = at(i64::try_from(k).expect("small"));
        }
        for row in rows_of {
            ok(&mut store, row);
        }
        let mine = answer(&mut store, 1, &["m1", "m2"]);
        let theirs = answer(&mut store, 2, &["m1", "m2"]);
        answers.push((
            messages(&mine)
                .into_iter()
                .map(|(m, s)| (m.to_owned(), s))
                .collect::<Vec<_>>(),
            ids_of(&mine.blocked).join(","),
            messages(&theirs)
                .into_iter()
                .map(|(m, s)| (m.to_owned(), s))
                .collect::<Vec<_>>(),
            ids_of(&theirs.blocked).join(","),
        ));
    }
    assert_eq!(answers[0], answers[1]);
    assert_eq!(
        answers[0],
        (
            vec![("m1".to_owned(), 1), ("m2".to_owned(), 2)],
            "m1".to_owned(),
            vec![("m1".to_owned(), 2), ("m2".to_owned(), 1)],
            "m2".to_owned(),
        )
    );
}

fn widest(a: u32, message: &str, draft: &str, state: &str) -> Value {
    let mut row = reply(a, a, 1, message, Some(draft), Some(a), state);
    row["seq"] = json!(u32::MAX);
    row["day"] = json!(u32::MAX);
    row["at"] = json!(i64::MIN);
    row
}

fn a_full_page(width: usize) -> (MemoryStore, Vec<String>) {
    let mut store = two_agents();
    let ids: Vec<String> = (0..25)
        .map(|n| format!("{n:02}{}", "m".repeat(width - 2)))
        .collect();
    let mut planted = Vec::new();
    for id in &ids {
        let draft = format!("{}d", &id[..width - 1]);
        planted.push(widest(1, id, &draft, "committed"));
        planted.push(widest(2, id, &draft, "committed"));
    }
    plant(&mut store, planted);
    (store, ids)
}

#[test]
fn asked_for_25_ids_that_are_all_blocked_the_handler_answers_every_one() {
    let mut store = two_agents();
    let ids: Vec<String> = (0..25).map(|n| format!("m{n}")).collect();
    for id in &ids {
        other_agent_holds(&mut store, id, "unknown");
        ok(&mut store, reply(1, 1, 1, id, Some("d1"), None, "drafted"));
    }
    let asked: Vec<&str> = ids.iter().map(String::as_str).collect();
    let reply = answer(&mut store, 1, &asked);
    assert_eq!(ids_of(&reply.blocked), asked);
    assert_eq!(reply.replies.len(), 25);
}

#[test]
fn asked_for_25_ids_of_776_bytes_whose_rows_cannot_fit_the_handler_returns_no_shorter_list() {
    let (mut store, ids) = a_full_page(776);
    let asked: Vec<&str> = ids.iter().map(String::as_str).collect();
    let before = rows(&store, "agent_reply");
    assert_eq!(
        load_replies(&mut store, 1, 1, &asked),
        Err(ErrorName::Internal)
    );
    assert_eq!(rows(&store, "agent_reply"), before);
    let (mut store, ids) = a_full_page(775);
    let asked: Vec<&str> = ids.iter().map(String::as_str).collect();
    let reply = answer(&mut store, 1, &asked);
    assert_eq!(reply.replies.len(), 25);
    assert_eq!(ids_of(&reply.blocked), asked);
    assert!(reply.to_json().is_ok());
}

#[test]
fn load_replies_is_bound_to_a_started_open_run_of_its_agent() {
    let mut store = two_agents();
    assert_eq!(
        load_replies(&mut store, 1, 7, &["m1"]),
        Err(declared("binding"))
    );
    assert_eq!(
        load_replies(&mut store, 1, 2, &["m1"]),
        Err(declared("binding"))
    );
    end_run(&mut store, 1).expect("end_run");
    assert_eq!(
        load_replies(&mut store, 1, 1, &["m1"]),
        Err(declared("effect"))
    );
}

fn keeper_case(other: &str) -> (MemoryStore, Result<RunnerReply, ErrorName>) {
    let mut store = two_agents();
    ok(
        &mut store,
        reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
    );
    other_agent_holds(&mut store, "m1", other);
    let answer = put_reply(
        &mut store,
        reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
    );
    (store, answer)
}

#[test]
fn a_begun_row_is_refused_while_another_agent_holds_begun_committed_or_unknown() {
    for other in ["begun", "committed", "unknown"] {
        let (mut store, answer) = keeper_case(other);
        assert_eq!(answer, Err(declared("effect")), "{other}");
        let mine = newest(&store, 1, "m1").expect("a row");
        assert_eq!(
            (mine.seq.get(), mine.state.as_str()),
            (1, "drafted"),
            "{other}"
        );
        let before = rows(&store, "agent_reply");
        end_run(&mut store, 1).expect("end_run");
        let after = rows(&store, "agent_reply");
        assert_eq!(after.len(), before.len(), "{other}");
    }
}

#[test]
fn a_begun_row_is_stored_while_another_agent_holds_drafted_lost_or_withheld() {
    for other in ["drafted", "lost", "withheld"] {
        let (store, answer) = keeper_case(other);
        assert_eq!(answer, Ok(stored(1)), "{other}");
        assert_eq!(
            newest(&store, 1, "m1").map(|r| r.state.as_str()),
            Some("begun"),
            "{other}"
        );
    }
}

#[test]
fn a_begun_row_after_the_drafted_row_that_follows_begun_meets_the_same_keeper() {
    for other in STATES {
        let mut store = two_agents();
        plant(
            &mut store,
            vec![
                reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
                reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun"),
                reply(1, 1, 3, "m1", Some("d1"), Some(1), "drafted"),
            ],
        );
        other_agent_holds(&mut store, "m1", other);
        let answer = put_reply(
            &mut store,
            reply(1, 1, 4, "m1", Some("d1"), Some(3), "begun"),
        );
        let want = match other {
            "begun" | "committed" | "unknown" => Err(declared("effect")),
            _ => Ok(stored(1)),
        };
        assert_eq!(answer, want, "{other}");
    }
}

#[test]
fn of_two_agents_with_drafted_rows_the_second_begun_row_is_refused_in_either_order() {
    for first in [1, 2] {
        let second = 3 - first;
        let mut store = two_agents();
        ok(
            &mut store,
            reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
        );
        ok(
            &mut store,
            reply(2, 2, 1, "m1", Some("d2"), None, "drafted"),
        );
        let begun = |a: u32| {
            reply(
                a,
                a,
                2,
                "m1",
                Some(["d1", "d2"][a as usize - 1]),
                Some(a),
                "begun",
            )
        };
        assert_eq!(
            put_reply(&mut store, begun(first)),
            Ok(stored(1)),
            "{first}"
        );
        assert_eq!(
            put_reply(&mut store, begun(second)),
            Err(declared("effect")),
            "{first}"
        );
        assert_eq!(
            newest(&store, second, "m1").map(|r| r.state.as_str()),
            Some("drafted"),
            "{first}"
        );
    }
}

#[test]
fn a_repeat_of_the_asking_agent_s_own_begun_row_is_answered_as_the_first_whatever_others_hold() {
    for other in STATES {
        let mut store = two_agents();
        begun_of_run_1(&mut store);
        let mut chain = vec![reply(2, 2, 1, "m1", Some("dz"), None, "drafted")];
        match other {
            "drafted" => {}
            "lost" => chain.push(reply(2, 2, 2, "m1", None, None, "lost")),
            "withheld" => chain.push(reply(2, 2, 2, "m1", Some("dz"), None, "withheld")),
            "begun" => chain.push(reply(2, 2, 2, "m1", Some("dz"), Some(9), "begun")),
            end => {
                chain.push(reply(2, 2, 2, "m1", Some("dz"), Some(9), "begun"));
                chain.push(reply(2, 2, 3, "m1", Some("dz"), Some(9), end));
            }
        }
        plant(&mut store, chain);
        let mut again = reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun");
        again["at"] = json!(ROW_AT + 9);
        assert_eq!(put_reply(&mut store, again), Ok(stored(1)), "{other}");
    }
}

#[test]
fn the_keeper_and_the_list_blocked_agree() {
    for other in STATES {
        let (_, answer) = keeper_case(other);
        let mut fresh = two_agents();
        other_agent_holds(&mut fresh, "m1", other);
        let blocked = !answer_of(&mut fresh).blocked.is_empty();
        assert_eq!(answer.is_err(), blocked, "{other}");
    }
}

fn answer_of(store: &mut MemoryStore) -> LoadRepliesReply {
    answer(store, 1, &["m1"])
}

#[test]
fn the_keeper_does_not_order_two_agents_rows_by_their_instants() {
    for (mine, theirs) in [(2, 1), (1, 2)] {
        let mut store = two_agents();
        let hour = |h: i64| json!(ROW_AT + h * 3_600_000_000_000);
        let mut drafted = reply(1, 1, 1, "m1", Some("d1"), None, "drafted");
        drafted["at"] = hour(mine);
        ok(&mut store, drafted);
        for mut row in [
            reply(2, 2, 1, "m1", Some("dz"), None, "drafted"),
            reply(2, 2, 2, "m1", Some("dz"), Some(9), "begun"),
        ] {
            row["at"] = hour(theirs);
            ok(&mut store, row);
        }
        let mut begun = reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun");
        begun["at"] = hour(3);
        assert_eq!(
            put_reply(&mut store, begun),
            Err(declared("effect")),
            "{mine} {theirs}"
        );
    }
}

#[test]
fn after_a_run_s_end_a_repeat_of_the_newest_row_is_answered_as_the_first_and_a_new_row_is_refused()
{
    let mut store = two_agents();
    begun_of_run_1(&mut store);
    ok(
        &mut store,
        reply(1, 1, 1, "m2", Some("d2"), None, "drafted"),
    );
    end_run(&mut store, 1).expect("end_run");
    let before = rows(&store, "agent_reply");
    let mut again = reply(1, 1, 1, "m2", Some("d2"), None, "drafted");
    again["at"] = json!(ROW_AT + 11);
    assert_eq!(put_reply(&mut store, again), Ok(stored(1)));
    assert_eq!(
        put_reply(&mut store, ended_unknown(1, 1, 3, "m1", "d1", 1)),
        Ok(stored(1))
    );
    assert_eq!(rows(&store, "agent_reply"), before);
    refused(
        &mut store,
        reply(1, 1, 1, "m3", Some("d3"), None, "drafted"),
    );
    refused(
        &mut store,
        reply(1, 1, 2, "m2", Some("d2"), Some(2), "begun"),
    );
}

#[test]
fn a_repeat_of_a_begun_row_a_day_later_with_the_day_of_its_at_is_answered_as_the_first() {
    let mut store = two_agents();
    begun_of_run_1(&mut store);
    let before = rows(&store, "agent_reply");
    let mut again = reply(1, 1, 2, "m1", Some("d1"), Some(1), "begun");
    again["at"] = json!(ROW_AT + NANOS_PER_DAY);
    again["day"] = json!(ROW_DAY + 1);
    assert_eq!(put_reply(&mut store, again), Ok(stored(1)));
    assert_eq!(rows(&store, "agent_reply"), before);
}

/// The text of the reply of `list_replies` of agent `a` in workflow `w`, after the message `after` or from the first.
fn list_replies(
    store: &mut MemoryStore,
    w: u32,
    a: u32,
    after: Option<&str>,
) -> Result<String, ErrorName> {
    let body = json!({"op": "list_replies", "workflow": wf(w), "agent": agent(a), "after": after});
    coordinator(store, body).map(|reply| reply.to_json().expect("the reply's text"))
}

fn listed(store: &mut MemoryStore, w: u32, a: u32, after: Option<&str>) -> Value {
    let text = list_replies(store, w, a, after).expect("the reply of list_replies");
    serde_json::from_str(&text).expect("json")
}

#[test]
fn list_replies_answers_the_newest_row_of_each_message_of_the_agent_alone() {
    let mut store = two_agents();
    plant(
        &mut store,
        vec![
            reply(1, 1, 1, "m3", Some("d1"), None, "drafted"),
            reply(1, 1, 2, "m3", None, None, "lost"),
            reply(1, 3, 3, "m3", Some("d3"), None, "drafted"),
            reply(2, 2, 1, "m1", Some("dz"), None, "drafted"),
            reply(2, 2, 2, "m1", Some("dz"), Some(9), "begun"),
            reply(2, 2, 3, "m1", Some("dz"), Some(9), "committed"),
            reply(2, 2, 1, "m0", Some("dz"), None, "drafted"),
            reply(2, 2, 2, "m0", Some("dz"), None, "withheld"),
            reply(1, 1, 1, "m1", Some("d1"), None, "drafted"),
            reply(1, 1, 3, "m2", Some("d1"), Some(1), "unknown"),
            reply(1, 1, 2, "m2", Some("d1"), Some(1), "begun"),
            reply(1, 1, 1, "m2", Some("d1"), None, "drafted"),
        ],
    );
    let before = rows(&store, "agent_reply");
    let entry = |message: &str, r: u32, state: &str| json!({"message": message, "run": run(r), "state": state});
    let all = [
        entry("m1", 1, "drafted"),
        entry("m2", 1, "unknown"),
        entry("m3", 3, "drafted"),
    ];
    assert_eq!(
        listed(&mut store, 1, 1, None),
        json!({"replies": all, "next": null})
    );
    assert_eq!(
        listed(&mut store, 1, 1, Some("m0")),
        json!({"replies": all, "next": null})
    );
    assert_eq!(
        listed(&mut store, 1, 1, Some("m1")),
        json!({"replies": all[1..], "next": null})
    );
    assert_eq!(
        listed(&mut store, 1, 1, Some("m3")),
        json!({"replies": [], "next": null})
    );
    assert_eq!(
        listed(&mut store, 2, 2, None),
        json!({"replies": [entry("m0", 2, "withheld"), entry("m1", 2, "committed")], "next": null})
    );
    assert_eq!(
        list_replies(&mut store, 9, 1, None),
        Err(ErrorName::NoWorkflow)
    );
    assert_eq!(
        list_replies(&mut store, 2, 1, None),
        Err(declared("binding"))
    );
    assert_eq!(rows(&store, "agent_reply"), before);
}

#[test]
fn a_page_of_list_replies_stays_under_the_reply_bound_and_names_the_next() {
    let mut store = two_agents();
    let ids: Vec<String> = (0..600)
        .map(|n| format!("{n:04}{}", "m".repeat(1020)))
        .collect();
    assert!(ids.iter().all(|id| id.len() == 1024));
    plant(
        &mut store,
        ids.iter()
            .rev()
            .map(|id| reply(1, 1, 1, id, Some("d1"), None, "drafted"))
            .collect(),
    );
    let mut all: Vec<String> = Vec::new();
    let mut after: Option<String> = None;
    let mut pages = 0;
    loop {
        let text = list_replies(&mut store, 1, 1, after.as_deref()).expect("a page");
        assert!(text.len() <= CHECKPOINTS_REPLY_MAX, "{}", text.len());
        let page: Value = serde_json::from_str(&text).expect("json");
        let messages: Vec<String> = page["replies"]
            .as_array()
            .expect("the list")
            .iter()
            .map(|e| e["message"].as_str().expect("a message id").to_owned())
            .collect();
        assert!(!messages.is_empty(), "page {pages}");
        pages += 1;
        all.extend(messages.iter().cloned());
        let Some(next) = page["next"].as_str() else {
            break;
        };
        assert_eq!(Some(next), messages.last().map(String::as_str));
        let following =
            json!({"message": ids[all.len()], "run": run(1), "state": "drafted"}).to_string();
        assert!(
            text.len() + 1 + following.len() > CHECKPOINTS_REPLY_MAX,
            "page {pages} of {} bytes leaves out an entry of {} bytes that fits",
            text.len(),
            following.len()
        );
        after = Some(next.to_owned());
    }
    assert_eq!(all, ids);
    assert!(pages >= 3, "{pages}");
}
