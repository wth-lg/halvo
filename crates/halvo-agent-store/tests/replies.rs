mod common;

use common::*;
use halvo_agent_store::MemoryStore;
use halvo_schema::executable::{AgentReply, RunnerReply};
use halvo_schema::{ErrorName, TypedRecord};
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

fn two_agents() -> MemoryStore {
    let mut store = MemoryStore::new();
    for n in [1, 2] {
        store.put_workflow(workflow(&wf(n), "email", "approved", None));
        create(&mut store, n, n).expect("create_agent");
        start_at(&mut store, NOW, n, n, n).expect("start_run");
    }
    store
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
