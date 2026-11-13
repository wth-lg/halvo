mod common;

use common::*;
use halvo_agent_store::MemoryStore;
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::{AgentLog, AgentPlaybook, AgentSession, RunnerReply};
use halvo_schema::ErrorName;
use serde_json::{json, Value};

fn load(store: &mut MemoryStore, a: u32) -> Result<LoadReply, ErrorName> {
    match runner(
        store,
        json!({"op": "load", "agent": agent(a), "run": run(1)}),
    )? {
        RunnerReply::Load(reply) => Ok(reply),
        other => panic!("not a load reply: {other:?}"),
    }
}

fn load_steps(store: &mut MemoryStore, number: u32, from: u32) -> Result<Vec<u32>, ErrorName> {
    match runner(
        store,
        json!({"op": "load_steps", "agent": agent(1), "run": run(1), "number": number, "from": from}),
    )? {
        RunnerReply::Steps(reply) => Ok(reply.steps.iter().map(|s| s.n.get()).collect()),
        other => panic!("not a steps reply: {other:?}"),
    }
}

fn navs(from: u32, to: u32) -> Vec<Value> {
    (from..=to).map(|n| step(n, "navigate", "read")).collect()
}

fn playbook_rows(store: &MemoryStore) -> Vec<AgentPlaybook> {
    rows(store, "agent_playbook")
        .iter()
        .map(|r| AgentPlaybook::parse(r.as_bytes()).expect("a playbook row"))
        .collect()
}

fn session_rows(store: &MemoryStore) -> Vec<AgentSession> {
    rows(store, "agent_session")
        .iter()
        .map(|r| AgentSession::parse(r.as_bytes()).expect("a session row"))
        .collect()
}

fn full_part() -> Vec<String> {
    vec!["A".repeat(1024); 48]
}

fn put_session(
    store: &mut MemoryStore,
    origin: &str,
    rev: u32,
    part: u32,
    of: u32,
    data: Vec<String>,
) -> Result<RunnerReply, ErrorName> {
    runner(
        store,
        json!({"op": "put_session", "agent": agent(1), "run": run(1), "origin": origin, "rev": rev, "part": part, "of": of, "data": data}),
    )
}

fn load_session(
    store: &mut MemoryStore,
    origin: &str,
    rev: u32,
    part: u32,
) -> Result<RunnerReply, ErrorName> {
    runner(
        store,
        json!({"op": "load_session", "agent": agent(1), "run": run(1), "origin": origin, "rev": rev, "part": part}),
    )
}

fn log_line(seq: u32) -> Value {
    json!({
        "v": 1,
        "workflow": wf(1),
        "agent": agent(1),
        "run": run(1),
        "seq": seq,
        "day": 20_770,
        "at": 1_794_400_000_000_000_000_i64,
        "event": "step_ok",
        "step": 1,
        "cause": null,
        "model": null,
        "ms": null,
        "tokens_in": null,
        "tokens_out": null,
    })
}

fn put_log(store: &mut MemoryStore, records: Vec<Value>) -> Result<RunnerReply, ErrorName> {
    runner(
        store,
        json!({"op": "log", "agent": agent(1), "run": run(1), "records": records}),
    )
}

#[test]
fn load_of_a_new_agent_holds_its_configuration_alone() {
    let mut store = store_with("utility_bill_payment");
    let reply = load(&mut store, 1).expect("load");
    assert_eq!([reply.config], configs(&store).as_slice());
    assert_eq!(reply.playbook, None);
    assert_eq!(reply.bindings, None);
    assert!(reply.sessions.is_empty());
}

#[test]
fn load_reads_the_newest_revision_of_each() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(put_steps(&mut store, 1, navs(1, 3)), Ok(stored(3)));
    assert_eq!(put_steps(&mut store, 2, navs(1, 2)), Ok(stored(2)));
    coordinator(
        &mut store,
        json!({"op": "set_bindings", "workflow": wf(1), "agent": agent(1), "slots": [{"name": "payee", "value": ["Northwind"]}]}),
    )
    .expect("set_bindings");
    coordinator(
        &mut store,
        json!({"op": "add_origin", "workflow": wf(1), "agent": agent(1), "run": run(1), "origin": "https://pay.test:443", "way": "sign_in"}),
    )
    .expect("add_origin");
    assert_eq!(
        put_session(
            &mut store,
            "https://pay.test:443",
            1,
            1,
            1,
            vec!["QUJD".into()]
        ),
        Ok(stored(1))
    );
    assert_eq!(
        put_session(
            &mut store,
            "https://a.test:443",
            1,
            1,
            1,
            vec!["QUJD".into()]
        ),
        Ok(stored(1))
    );
    let reply = load(&mut store, 1).expect("load");
    let playbook = reply.playbook.expect("a playbook");
    assert_eq!((playbook.number.get(), playbook.steps.get()), (2, 2));
    let bindings = reply.bindings.expect("bindings");
    assert_eq!(bindings.rev.get(), 2);
    assert_eq!(bindings.origins.len(), 1);
    assert_eq!(bindings.slots.len(), 1);
    let sessions: Vec<(String, u32, u32)> = reply
        .sessions
        .iter()
        .map(|s| (s.origin.as_str().to_owned(), s.rev.get(), s.parts.get()))
        .collect();
    assert_eq!(
        sessions,
        [
            ("https://a.test:443".to_owned(), 1, 1),
            ("https://pay.test:443".to_owned(), 1, 1)
        ]
    );
}

#[test]
fn load_gives_the_newest_session_revision_whose_parts_are_all_stored() {
    let mut store = store_with("utility_bill_payment");
    let origin = "https://pay.test:443";
    put_session(&mut store, origin, 1, 1, 1, vec!["QUJD".into()]).expect("rev 1");
    put_session(&mut store, origin, 2, 1, 2, full_part()).expect("rev 2 part 1");
    let reply = load(&mut store, 1).expect("load");
    assert_eq!(
        (reply.sessions[0].rev.get(), reply.sessions[0].parts.get()),
        (1, 1)
    );
    put_session(&mut store, origin, 2, 2, 2, vec!["QUJD".into()]).expect("rev 2 part 2");
    let reply = load(&mut store, 1).expect("load");
    assert_eq!(reply.sessions.len(), 1);
    assert_eq!(
        (reply.sessions[0].rev.get(), reply.sessions[0].parts.get()),
        (2, 2)
    );
    put_session(&mut store, origin, 3, 2, 2, vec!["QUJD".into()]).expect("rev 3 part 2");
    let reply = load(&mut store, 1).expect("load");
    assert_eq!(reply.sessions[0].rev.get(), 2);
}

#[test]
fn put_session_stores_a_part_once_with_the_day_of_the_request() {
    let mut store = store_with("utility_bill_payment");
    let origin = "https://pay.test:443";
    assert_eq!(
        put_session(&mut store, origin, 1, 1, 1, vec!["QUJD".into()]),
        Ok(stored(1))
    );
    assert_eq!(
        put_session(&mut store, origin, 1, 1, 1, vec!["QUJD".into()]),
        Ok(stored(1))
    );
    let rows = session_rows(&store);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].day, NOW.day);
    assert_eq!(rows[0].workflow.as_str(), wf(1));
    assert_eq!(
        put_session(&mut store, origin, 1, 1, 1, vec!["QUJE".into()]),
        Err(declared("binding"))
    );
    assert_eq!(
        put_session(&mut store, origin, 1, 1, 2, full_part()),
        Err(declared("binding"))
    );
    assert_eq!(session_rows(&store).len(), 1);
}

#[test]
fn put_session_refuses_the_same_data_at_a_stored_place_with_another_of() {
    let mut store = store_with("utility_bill_payment");
    let origin = "https://bank.test:443";
    assert_eq!(
        put_session(&mut store, origin, 1, 1, 2, full_part()),
        Ok(stored(1))
    );
    assert_eq!(
        put_session(&mut store, origin, 1, 1, 3, full_part()),
        Err(declared("binding"))
    );
    assert_eq!(session_rows(&store).len(), 1);
}

#[test]
fn load_takes_no_session_revision_whose_parts_disagree_on_of() {
    let mut store = store_with("utility_bill_payment");
    let origin = "https://bank.test:443";
    assert_eq!(
        put_session(&mut store, origin, 1, 1, 2, full_part()),
        Ok(stored(1))
    );
    assert_eq!(
        put_session(&mut store, origin, 1, 2, 3, full_part()),
        Ok(stored(1))
    );
    assert_eq!(
        put_session(&mut store, origin, 1, 3, 3, vec!["QUJD".into()]),
        Ok(stored(1))
    );
    let reply = load(&mut store, 1).expect("load");
    assert!(reply.sessions.is_empty(), "{:?}", reply.sessions);
}

#[test]
fn load_session_reads_one_stored_part() {
    let mut store = store_with("utility_bill_payment");
    let origin = "https://pay.test:443";
    put_session(&mut store, origin, 1, 1, 2, full_part()).expect("part 1");
    put_session(&mut store, origin, 1, 2, 2, vec!["QUJD".into()]).expect("part 2");
    let reply = load_session(&mut store, origin, 1, 2).expect("load_session");
    assert_eq!(
        reply,
        RunnerReply::parse(
            br#"{"origin":"https://pay.test:443","rev":1,"part":2,"of":2,"data":["QUJD"]}"#
        )
        .expect("a session reply")
    );
    assert_eq!(
        load_session(&mut store, origin, 2, 1),
        Err(declared("binding"))
    );
    assert_eq!(
        load_session(&mut store, "https://a.test:443", 1, 1),
        Err(declared("binding"))
    );
}

#[test]
fn put_playbook_stores_each_step_once_and_answers_a_repeat_as_the_first() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(put_steps(&mut store, 1, navs(1, 16)), Ok(stored(16)));
    assert_eq!(put_steps(&mut store, 1, navs(17, 20)), Ok(stored(4)));
    assert_eq!(put_steps(&mut store, 1, navs(17, 20)), Ok(stored(4)));
    assert_eq!(put_steps(&mut store, 1, navs(1, 16)), Ok(stored(16)));
    assert_eq!(playbook_rows(&store).len(), 20);
    assert_eq!(put_steps(&mut store, 1, navs(19, 22)), Ok(stored(4)));
    assert_eq!(playbook_rows(&store).len(), 22);
    assert_eq!(
        put_steps(&mut store, 1, vec![step(5, "click", "prepare")]),
        Err(declared("playbook"))
    );
    assert_eq!(playbook_rows(&store).len(), 22);
}

#[test]
fn put_playbook_numbers_rise_by_one() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        put_steps(&mut store, 2, navs(1, 1)),
        Err(declared("playbook"))
    );
    assert_eq!(put_steps(&mut store, 1, navs(1, 2)), Ok(stored(2)));
    assert_eq!(
        put_steps(&mut store, 3, navs(1, 1)),
        Err(declared("playbook"))
    );
    assert_eq!(put_steps(&mut store, 2, navs(1, 1)), Ok(stored(1)));
    assert_eq!(
        put_steps(&mut store, 1, navs(3, 3)),
        Err(declared("playbook"))
    );
    assert_eq!(put_steps(&mut store, 2, navs(2, 2)), Ok(stored(1)));
    assert_eq!(playbook_rows(&store).len(), 4);
}

#[test]
fn put_playbook_refuses_a_gap() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        put_steps(&mut store, 1, navs(2, 3)),
        Err(declared("playbook"))
    );
    assert_eq!(put_steps(&mut store, 1, navs(1, 3)), Ok(stored(3)));
    assert_eq!(
        put_steps(&mut store, 1, navs(5, 6)),
        Err(declared("playbook"))
    );
    assert_eq!(put_steps(&mut store, 1, navs(4, 6)), Ok(stored(3)));
    assert_eq!(playbook_rows(&store).len(), 6);
}

#[test]
fn put_playbook_runs_check_playbook_on_what_it_holds() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        put_steps(
            &mut store,
            1,
            vec![step(1, "navigate", "read"), step(2, "navigate", "prepare")]
        ),
        Err(declared("class"))
    );
    assert!(playbook_rows(&store).is_empty());
    assert_eq!(
        put_steps(
            &mut store,
            1,
            vec![step(1, "navigate", "read"), step(2, "click", "prepare")]
        ),
        Ok(stored(2))
    );
    assert_eq!(
        put_steps(&mut store, 1, vec![step(3, "click", "read")]),
        Err(declared("class"))
    );
    assert_eq!(playbook_rows(&store).len(), 2);
}

#[test]
fn load_steps_reads_at_most_sixteen_steps_from_from() {
    let mut store = store_with("utility_bill_payment");
    put_steps(&mut store, 1, navs(1, 16)).expect("steps 1 to 16");
    put_steps(&mut store, 1, navs(17, 20)).expect("steps 17 to 20");
    assert_eq!(load_steps(&mut store, 1, 1), Ok((1..=16).collect()));
    assert_eq!(load_steps(&mut store, 1, 5), Ok((5..=20).collect()));
    assert_eq!(load_steps(&mut store, 1, 17), Ok((17..=20).collect()));
    assert_eq!(load_steps(&mut store, 1, 21), Ok(vec![]));
    assert_eq!(load_steps(&mut store, 2, 1), Err(declared("playbook")));
    assert_eq!(load_steps(&mut store, 1, 65), Err(declared("length")));
}

#[test]
fn log_lines_are_stored_by_the_runner_body() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        put_log(&mut store, vec![log_line(1), log_line(2)]),
        Ok(stored(2))
    );
    assert_eq!(
        put_log(&mut store, vec![log_line(1), log_line(2)]),
        Ok(stored(2))
    );
    assert_eq!(
        put_log(&mut store, vec![log_line(2), log_line(3)]),
        Ok(stored(2))
    );
    let lines: Vec<AgentLog> = rows(&store, "agent_log")
        .iter()
        .map(|r| AgentLog::parse(r.as_bytes()).expect("a log row"))
        .collect();
    let mut seqs: Vec<u32> = lines.iter().map(|l| l.seq.get()).collect();
    seqs.sort_unstable();
    assert_eq!(seqs, [1, 2, 3]);
    let mut other = log_line(4);
    other["workflow"] = json!(wf(2));
    assert_eq!(put_log(&mut store, vec![other]), Err(declared("binding")));
    assert_eq!(rows(&store, "agent_log").len(), 3);
}

#[test]
fn runner_bodies_of_an_agent_that_is_not_the_workflow_s_are_refused() {
    let mut store = store_with("utility_bill_payment");
    let other = json!({"op": "load", "agent": agent(2), "run": run(1)});
    assert_eq!(runner(&mut store, other), Err(declared("binding")));
    store.put_workflow(workflow(
        &wf(1),
        "utility_bill_payment",
        "approved",
        Some(&agent(3)),
    ));
    assert_eq!(load(&mut store, 1).map(|_| ()), Err(declared("binding")));
    assert_eq!(
        put_steps(&mut store, 1, navs(1, 1)),
        Err(declared("binding"))
    );
    store.put_workflow(dismissed(&wf(1)));
    assert_eq!(load(&mut store, 1).map(|_| ()), Err(declared("binding")));
}

#[test]
fn a_runner_body_that_is_not_the_form_is_refused() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        runner(&mut store, json!({"op": "load", "agent": agent(1)})),
        Err(ErrorName::Malformed)
    );
    assert_eq!(
        runner(
            &mut store,
            json!({"op": "checkpoint", "agent": agent(1), "run": run(1)})
        ),
        Err(ErrorName::Malformed)
    );
    assert_eq!(
        put_steps(&mut store, 1, navs(1, 17)),
        Err(ErrorName::Malformed)
    );
    assert!(playbook_rows(&store).is_empty());
}
