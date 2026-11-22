mod common;

use common::*;
use halvo_agent_store::MemoryStore;
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::LogEvent;
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
        "day": ROW_DAY,
        "at": ROW_AT,
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
    let fact = finished_row(1, 2, 2, 9);
    store
        .typed_commit(vec![fact])
        .expect("the fact of the compile");
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

    let dropped = json!({"op": "drop_session", "agent": agent(1), "run": run(1), "origin": origin});
    assert_eq!(runner(&mut store, dropped.clone()), Ok(stored(1)));
    let reply = load(&mut store, 1).expect("load");
    assert_eq!(
        (reply.sessions[0].rev.get(), reply.sessions[0].parts.get()),
        (4, 1),
        "the drop is the revision after every stored one"
    );
    let Ok(RunnerReply::Session(part)) = load_session(&mut store, origin, 4, 1) else {
        panic!("the drop's part");
    };
    assert!(part.data.is_dropped());
    let held = session_rows(&store).len();
    assert_eq!(runner(&mut store, dropped), Ok(stored(1)));
    assert_eq!(
        session_rows(&store).len(),
        held,
        "a second drop stores nothing"
    );
    put_session(&mut store, origin, 5, 1, 1, vec!["QUJD".into()]).expect("rev 5");
    assert_eq!(load(&mut store, 1).expect("load").sessions[0].rev.get(), 5);
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
    assert_eq!(rows[0].day, TODAY);
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
            br#"{"agent":"40000000000000000000000000000001","origin":"https://pay.test:443","rev":1,"part":2,"of":2,"data":["QUJD"]}"#
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
    let fact = finished_row(1, 1, 20, 9);
    store
        .typed_commit(vec![fact])
        .expect("the fact of the compile");
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
    let before = rows(&store, "agent_log").len();
    assert_eq!(
        put_log(&mut store, vec![log_line(2), log_line(3)]),
        Ok(stored(2))
    );
    assert_eq!(
        put_log(&mut store, vec![log_line(2), log_line(3)]),
        Ok(stored(2))
    );
    assert_eq!(
        put_log(&mut store, vec![log_line(3), log_line(4)]),
        Ok(stored(2))
    );
    let lines: Vec<AgentLog> = rows(&store, "agent_log")
        .iter()
        .map(|r| AgentLog::parse(r.as_bytes()).expect("a log row"))
        .filter(|l| l.event == LogEvent::StepOk)
        .collect();
    let mut seqs: Vec<u32> = lines.iter().map(|l| l.seq.get()).collect();
    seqs.sort_unstable();
    assert_eq!(seqs, [2, 3, 4]);
    let mut other = log_line(5);
    other["workflow"] = json!(wf(2));
    assert_eq!(put_log(&mut store, vec![other]), Err(declared("binding")));
    assert_eq!(rows(&store, "agent_log").len(), before + 3);
}

fn runner_bodies(r: u32) -> Vec<Value> {
    let origin = "https://pay.test:443";
    let mut line = log_line(2);
    line["run"] = json!(run(r));
    let mut effect_row = row(r, 1, 1, "held", "payment", 1, None, None);
    effect_row["run"] = json!(run(r));
    vec![
        json!({"op": "load", "agent": agent(1), "run": run(r)}),
        json!({"op": "load_steps", "agent": agent(1), "run": run(r), "number": 1, "from": 1}),
        json!({"op": "load_session", "agent": agent(1), "run": run(r), "origin": origin, "rev": 1, "part": 1}),
        json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": 1, "steps": navs(1, 1)}),
        json!({"op": "put_session", "agent": agent(1), "run": run(r), "origin": origin, "rev": 1, "part": 1, "of": 1, "data": ["QUJD"]}),
        json!({"op": "drop_session", "agent": agent(1), "run": run(r), "origin": origin}),
        json!({"op": "log", "agent": agent(1), "run": run(r), "records": [line]}),
        json!({"op": "effect", "agent": agent(1), "run": run(r), "record": effect_row}),
        json!({"op": "finish", "agent": agent(1), "run": run(r), "record": run_record(r, Some("replay"), "failed")}),
    ]
}

fn every_agent_row(store: &MemoryStore) -> Vec<Vec<String>> {
    halvo_agent_store::AGENT_KINDS
        .iter()
        .map(|(kind, _)| rows(store, kind))
        .collect()
}

#[test]
fn a_log_and_a_propose_of_a_run_with_no_start_row_are_refused_with_binding_and_store_nothing() {
    let mut store = unstarted("utility_bill_payment");
    let before = every_agent_row(&store);
    assert_eq!(
        put_log(&mut store, vec![log_line(2)]),
        Err(declared("binding"))
    );
    assert_eq!(
        put(&mut store, row(1, 1, 1, "held", "payment", 1, None, None)),
        Err(declared("binding"))
    );
    assert_eq!(every_agent_row(&store), before);
}

#[test]
fn every_runner_body_of_a_run_with_no_start_row_of_its_agent_is_refused_with_binding() {
    let mut store = unstarted("utility_bill_payment");
    store.put_workflow(workflow(&wf(2), "utility_bill_payment", "approved", None));
    create(&mut store, 2, 2).expect("create_agent of the second agent");
    assert_eq!(start_at(&mut store, NOW, 2, 2, 5), Ok(accepted()));
    let before = every_agent_row(&store);
    for r in [1, 5] {
        for body in runner_bodies(r) {
            assert_eq!(
                runner(&mut store, body.clone()),
                Err(declared("binding")),
                "{body}"
            );
        }
    }
    assert_eq!(every_agent_row(&store), before);
}

#[test]
fn a_runner_body_after_the_bound_of_its_start_row_is_refused_with_binding() {
    let mut store = store_with("utility_bill_payment");
    let line = |days: i64| {
        let at = NOW.at.0 + days * NANOS_PER_DAY;
        let day = u32::try_from(at.div_euclid(NANOS_PER_DAY)).expect("a day");
        dated(log_line(2), day, at)
    };
    let body =
        |days: i64| json!({"op": "log", "agent": agent(1), "run": run(1), "records": [line(days)]});
    let late = at(NOW.at.0 + 93 * NANOS_PER_DAY);
    assert_eq!(
        runner_at(&mut store, late, body(93)),
        Err(declared("binding"))
    );
    assert_eq!(
        runner_at(
            &mut store,
            late,
            json!({"op": "load", "agent": agent(1), "run": run(1)})
        )
        .map(|_| ()),
        Err(declared("binding"))
    );
    let last = at(NOW.at.0 + 92 * NANOS_PER_DAY);
    assert_eq!(runner_at(&mut store, last, body(92)), Ok(stored(1)));
}

#[test]
fn a_log_body_that_carries_the_start_row_s_event_or_count_is_refused_with_effect() {
    let mut store = store_with("utility_bill_payment");
    let before = rows(&store, "agent_log");
    let mut start_line = log_line(1);
    start_line["event"] = json!("start_accepted");
    start_line["step"] = json!(null);
    let mut later_start = start_line.clone();
    later_start["seq"] = json!(2);
    assert_eq!(
        put_log(&mut store, vec![later_start]),
        Err(ErrorName::Malformed)
    );
    for body in [
        vec![log_line(1)],
        vec![log_line(2), log_line(1)],
        vec![start_line],
    ] {
        assert_eq!(put_log(&mut store, body), Err(declared("effect")));
    }
    assert_eq!(rows(&store, "agent_log"), before);
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

fn every_row(store: &MemoryStore) -> Vec<(&'static str, Vec<String>)> {
    halvo_schema::executable::KINDS
        .iter()
        .map(|k| (k.name, rows(store, k.name)))
        .collect()
}

#[test]
fn the_bodies_reply_row_and_load_replies_are_answered_by_their_handlers() {
    let mut store = store_with("utility_bill_payment");
    let record = json!({
        "v": 1,
        "workflow": wf(1),
        "agent": agent(1),
        "run": run(1),
        "seq": 1,
        "day": ROW_DAY,
        "at": ROW_AT,
        "message": "msg-1",
        "draft": "draft-1",
        "effect": null,
        "state": "drafted",
    });
    let body = json!({"op": "reply_row", "agent": agent(1), "run": run(1), "record": record});
    assert_eq!(runner(&mut store, body), Ok(stored(1)));
    assert_eq!(rows(&store, "agent_reply").len(), 1);
    let before = every_row(&store);
    let body = json!({"op": "load_replies", "agent": agent(1), "run": run(1), "message_ids": ["msg-1", "msg-2"]});
    let reply = format!(
        r#"{{"replies":[{}],"blocked":[],"blocked_withheld":[]}}"#,
        record
    );
    assert_eq!(
        runner(&mut store, body),
        Ok(RunnerReply::parse(reply.as_bytes()).expect("a reply of load_replies"))
    );
    assert_eq!(every_row(&store), before);
}

#[test]
fn a_log_line_is_held_to_the_day_of_its_at_and_to_the_clock_and_a_refused_body_stores_nothing() {
    let mut store = store_with("utility_bill_payment").with_utc_offset(WEST);
    let evening = local(ROW_DAY, 23 * 3600 + 30 * 60, WEST);
    let line = |seq, day, at| dated(log_line(seq), day, at);
    let ahead = NOW.at.0 + NANOS_PER_DAY;
    let gone = local(TODAY - 93, 3600, WEST);
    for bad in [
        line(3, ROW_DAY + 1, evening),
        line(3, ROW_DAY - 1, evening),
        line(3, TODAY + 1, ahead),
        line(3, TODAY - 93, gone),
    ] {
        let body = vec![line(2, ROW_DAY, evening), bad.clone()];
        assert_eq!(put_log(&mut store, body), Err(declared("day")), "{bad}");
        assert_eq!(rows(&store, "agent_log").len(), 1, "{bad}");
    }
    assert_eq!(
        put_log(&mut store, vec![line(2, ROW_DAY, evening)]),
        Ok(stored(1))
    );
    assert_eq!(rows(&store, "agent_log").len(), 2);
}

#[test]
fn a_session_part_takes_the_local_day_of_the_request() {
    let mut store = unstarted("utility_bill_payment").with_utc_offset(WEST);
    let night = at(local(ROW_DAY, 21 * 3600, WEST));
    assert_eq!(start_at(&mut store, night, 1, 1, 1), Ok(accepted()));
    assert_eq!(night.at.0.div_euclid(NANOS_PER_DAY), i64::from(ROW_DAY) + 1);
    let body = json!({
        "op": "put_session", "agent": agent(1), "run": run(1), "origin": "https://pay.test:443",
        "rev": 1, "part": 1, "of": 1, "data": ["QUJD"],
    });
    assert_eq!(runner_at(&mut store, night, body), Ok(stored(1)));
    assert_eq!(session_rows(&store)[0].day, ROW_DAY);
}

#[test]
fn a_log_line_of_the_run_that_is_no_start_row_starts_nothing() {
    let mut store = unstarted("utility_bill_payment");
    let mut line = log_line(2);
    line["run"] = json!(run(3));
    store
        .typed_commit(vec![halvo_schema::TypedRecord {
            kind: "agent_log",
            json: line.to_string(),
        }])
        .expect("a log line stored before the start row existed");
    let load = json!({"op": "load", "agent": agent(1), "run": run(3)});
    assert_eq!(
        runner(&mut store, load).map(|_| ()),
        Err(declared("binding"))
    );
}

#[test]
fn a_row_whose_at_is_negative_is_refused_with_day() {
    let mut store = unstarted("utility_bill_payment");
    let early = at(10 * NANOS_PER_DAY);
    assert_eq!(start_at(&mut store, early, 1, 1, 1), Ok(accepted()));
    let before = rows(&store, "agent_effect");
    let held = dated(row(1, 1, 1, "held", "payment", 1, None, None), 0, -1);
    assert_eq!(put_at(&mut store, early, held), Err(declared("day")));
    let line = dated(log_line(2), 0, -1);
    let body = json!({"op": "log", "agent": agent(1), "run": run(1), "records": [line]});
    assert_eq!(runner_at(&mut store, early, body), Err(declared("day")));
    assert_eq!(rows(&store, "agent_effect"), before);
    let held = dated(row(1, 1, 1, "held", "payment", 1, None, None), 0, 0);
    assert_eq!(put_at(&mut store, early, held), Ok(stored(1)));
}

fn agent_1() -> halvo_schema::AgentId {
    halvo_schema::AgentId::parse(&agent(1)).expect("an agent id")
}

fn playbook_row(a: u32, number: u32, n: u32) -> halvo_schema::TypedRecord {
    let row = json!({
        "v": 1, "workflow": wf(1), "agent": agent(a), "number": number, "n": n,
        "step": step(n, "navigate", "read"),
    });
    halvo_schema::TypedRecord {
        kind: "agent_playbook",
        json: row.to_string(),
    }
}

#[test]
fn newest_playbook_and_the_key_playbook_of_load_agree_over_drawn_rows() {
    let mut seed: u64 = 0x5eed_0049;
    let mut draw = |below: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from((seed >> 33) % below).expect("a small number")
    };
    let (mut some, mut half, mut past) = (0, 0, 0);
    for case in 0..200 {
        let mut store = store_with("utility_bill_payment");
        let mut held = std::collections::BTreeMap::new();
        let mut finished = std::collections::BTreeMap::new();
        for _ in 0..draw(8) {
            let (a, number) = (1 + draw(3), 1 + draw(4));
            if held.contains_key(&(a, number)) {
                continue;
            }
            let count = 1 + draw(5);
            let rows = (1..=count).map(|n| playbook_row(a, number, n)).collect();
            store.typed_commit(rows).expect("playbook rows");
            held.insert((a, number), count);
            if draw(2) == 0 {
                store
                    .typed_commit(vec![finished_row(a, number, count, 90 + number)])
                    .expect("a finished row");
                finished.insert((a, number), count);
                let more = draw(3);
                let rows = (count + 1..=count + more)
                    .map(|n| playbook_row(a, number, n))
                    .collect();
                store
                    .typed_commit(rows)
                    .expect("rows stored after the finish");
                past += usize::from(a == 1 && more > 0);
            }
        }
        let read =
            halvo_agent_store::newest_playbook(&store, &agent_1()).expect("the rows are read");
        let loaded = load(&mut store, 1).expect("load").playbook;
        assert_eq!(read, loaded, "case {case}");
        let want = finished
            .iter()
            .filter(|((a, _), _)| *a == 1)
            .map(|((_, number), steps)| (*number, *steps))
            .max();
        let got = read.as_ref().map(|p| (p.number.get(), p.steps.get()));
        assert_eq!(got, want, "case {case}");
        some += usize::from(read.is_some());
        for &(a, number) in held.keys() {
            if a != 1 {
                continue;
            }
            let handed = load_steps(&mut store, number, 1);
            match finished.get(&(a, number)) {
                Some(&steps) => assert_eq!(handed, Ok((1..=steps).collect()), "case {case}"),
                None => {
                    assert_eq!(handed, Err(declared("playbook")), "case {case}");
                    half += 1;
                }
            }
        }
    }
    assert!(some > 50, "{some} of 200 cases had a finished playbook");
    assert!(half > 50, "{half} numbers of half playbooks were asked");
    assert!(
        past > 20,
        "{past} finished numbers had steps stored after their finish"
    );
}

#[test]
fn newest_playbook_of_an_agent_with_no_compile_number_is_none() {
    let mut store = store_with("utility_bill_payment");
    store
        .typed_commit(vec![playbook_row(2, 3, 1)])
        .expect("a playbook row of another agent");
    let before = every_agent_row(&store);
    assert_eq!(
        halvo_agent_store::newest_playbook(&store, &agent_1()),
        Ok(None)
    );
    assert_eq!(load(&mut store, 1).expect("load").playbook, None);
    assert_eq!(every_agent_row(&store), before);
    let other = halvo_schema::AgentId::parse(&agent(2)).expect("an agent id");
    assert_eq!(
        halvo_agent_store::newest_playbook(&store, &other),
        Err(declared("binding"))
    );
    store.put_workflow(dismissed(&wf(1)));
    assert_eq!(
        halvo_agent_store::newest_playbook(&store, &agent_1()),
        Err(declared("binding"))
    );
}

fn back(days: i64) -> halvo_agent_store::Now {
    at(NOW.at.0 - days * NANOS_PER_DAY)
}

fn start_rows_of(store: &MemoryStore, r: u32) -> usize {
    rows(store, "agent_log")
        .iter()
        .map(|row| AgentLog::parse(row.as_bytes()).expect("a log row"))
        .filter(|l| l.event == LogEvent::StartAccepted && l.run.as_str() == run(r))
        .count()
}

#[test]
fn a_runner_body_after_a_clock_set_back_one_day_across_midnight_is_served() {
    let mut store = store_with("utility_bill_payment");
    let line = dated(log_line(2), TODAY - 1, back(1).at.0);
    let body = json!({"op": "log", "agent": agent(1), "run": run(1), "records": [line]});
    assert_eq!(runner_at(&mut store, back(1), body), Ok(stored(1)));
    let load = json!({"op": "load", "agent": agent(1), "run": run(1)});
    assert!(runner_at(&mut store, back(1), load).is_ok());
    let ahead = dated(log_line(3), TODAY, NOW.at.0);
    let body = json!({"op": "log", "agent": agent(1), "run": run(1), "records": [ahead]});
    assert_eq!(
        runner_at(&mut store, back(1), body),
        Err(declared("day")),
        "a row dated the day after the request's clock is refused"
    );
}

#[test]
fn a_start_row_two_days_ahead_of_the_request_counts_and_one_three_days_ahead_does_not() {
    let mut store = store_with("utility_bill_payment");
    let load = json!({"op": "load", "agent": agent(1), "run": run(1)});
    assert!(runner_at(&mut store, back(2), load.clone()).is_ok());
    assert_eq!(
        runner_at(&mut store, back(3), load).map(|_| ()),
        Err(declared("binding"))
    );
}

#[test]
fn a_start_run_repeated_after_a_clock_set_back_one_day_stores_nothing() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(start_rows_of(&store, 1), 1);
    assert_eq!(start_at(&mut store, back(1), 1, 1, 1), Ok(accepted()));
    assert_eq!(start_rows_of(&store, 1), 1);
}

#[test]
fn after_a_clock_set_back_one_day_a_repeat_of_an_effect_a_checkpoint_and_a_finish_is_answered_as_the_first(
) {
    let mut store = store_with("spreadsheet_fill");
    let edit = row(1, 1, 1, "cleared", "local_edit", 1, None, None);
    assert_eq!(put_at(&mut store, NOW, edit.clone()), Ok(stored(1)));
    let planned = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(1), "seq": 1, "day": ROW_DAY,
        "at": ROW_AT, "folder": ["/data/books"], "workbook": ["/data/books/book.xlsx"],
        "copy": ["book.halvo-checkpoint.xlsx"], "sha256": null, "bytes": null, "state": "planned",
        "left": null,
    });
    let checkpoint =
        json!({"op": "checkpoint", "agent": agent(1), "run": run(1), "record": planned});
    assert_eq!(
        runner_at(&mut store, NOW, checkpoint.clone()),
        Ok(stored(1))
    );
    let before = (
        effects(&store).len(),
        rows(&store, "agent_checkpoint").len(),
    );
    assert_eq!(put_at(&mut store, back(1), edit), Ok(stored(1)));
    assert_eq!(runner_at(&mut store, back(1), checkpoint), Ok(stored(1)));
    assert_eq!(
        (
            effects(&store).len(),
            rows(&store, "agent_checkpoint").len()
        ),
        before
    );
    let record = run_record(1, Some("replay"), "failed");
    let finish = json!({"op": "finish", "agent": agent(1), "run": run(1), "record": record});
    assert_eq!(runner_at(&mut store, NOW, finish.clone()), Ok(stored(1)));
    let ended = (effects(&store).len(), rows(&store, "agent_run").len());
    assert_eq!(runner_at(&mut store, back(1), finish), Ok(stored(1)));
    let new_effect = row(1, 2, 1, "cleared", "local_edit", 1, None, None);
    assert_eq!(
        put_at(&mut store, back(1), new_effect),
        Err(declared("effect"))
    );
    assert_eq!(
        (effects(&store).len(), rows(&store, "agent_run").len()),
        ended
    );
}

#[test]
fn a_row_stamped_by_day_of_at_the_store_s_offset_is_taken_and_its_utc_stamp_is_refused() {
    use halvo_schema::executable::day::{day_of, UTC_OFFSET_S_MAX};
    use halvo_schema::Instant;
    for offset in [WEST, -UTC_OFFSET_S_MAX] {
        let night = local(TODAY, 21 * 3600 + 30 * 60, offset);
        let local_day = day_of(Instant(night), offset);
        let utc_day = day_of(Instant(night), 0);
        assert_eq!((local_day, utc_day), (TODAY, TODAY + 1), "{offset}");
        let mut store = unstarted("utility_bill_payment").with_utc_offset(offset);
        assert_eq!(
            start_at(&mut store, at(night), 1, 1, 1),
            Ok(accepted()),
            "{offset}"
        );
        let log = |day| json!({"op": "log", "agent": agent(1), "run": run(1), "records": [dated(log_line(2), day, night)]});
        assert_eq!(
            runner_at(&mut store, at(night), log(utc_day)),
            Err(declared("day")),
            "{offset}"
        );
        assert_eq!(
            runner_at(&mut store, at(night), log(local_day)),
            Ok(stored(1)),
            "{offset}"
        );
        let held = |day| dated(row(1, 1, 1, "held", "payment", 1, None, None), day, night);
        assert_eq!(
            put_at(&mut store, at(night), held(utc_day)),
            Err(declared("day")),
            "{offset}"
        );
        assert_eq!(
            put_at(&mut store, at(night), held(local_day)),
            Ok(stored(1)),
            "{offset}"
        );
        let finish = |day| {
            let mut record = run_record(1, Some("replay"), "failed");
            record["day"] = json!(day);
            json!({"op": "finish", "agent": agent(1), "run": run(1), "record": record})
        };
        assert_eq!(
            runner_at(&mut store, at(night), finish(utc_day)),
            Err(declared("day")),
            "{offset}"
        );
        assert!(rows(&store, "agent_run").is_empty(), "{offset}");
        assert_eq!(
            runner_at(&mut store, at(night), finish(local_day)),
            Ok(stored(1)),
            "{offset}"
        );
        assert_eq!(rows(&store, "agent_run").len(), 1, "{offset}");
    }
}

fn file_step(n: u32, verb: &str, file: &str) -> Value {
    let (args, class, sheet) = match verb {
        "read" => (
            json!([{"name": "as", "from": "const", "value": "rows", "key": null}]),
            "read",
            Value::Null,
        ),
        _ => (
            json!([{"name": "values", "from": "extract", "value": null, "key": "rows"}]),
            "local_edit",
            json!("sheet"),
        ),
    };
    json!({
        "v": 1,
        "n": n,
        "kind": "file",
        "verb": verb,
        "target": {"file": file, "sheet": sheet.clone(), "range": if sheet.is_null() { Value::Null } else { json!("range") }},
        "args": args,
        "pre": [],
        "post": [],
        "effect": "none",
        "class": class,
    })
}

#[test]
fn put_playbook_answers_a_write_to_a_file_that_is_not_the_slot_workbook_with_playbook() {
    for category in ["spreadsheet_fill", "utility_bill_payment"] {
        let mut store = store_with(category);
        for file in ["dest", "workbooks", "old_workbook"] {
            assert_eq!(
                put_steps(
                    &mut store,
                    1,
                    vec![file_step(1, "read", "source"), file_step(2, "write", file)]
                ),
                Err(declared("playbook")),
                "{category} {file}"
            );
            assert!(playbook_rows(&store).is_empty(), "{category} {file}");
        }
        assert_eq!(
            put_steps(
                &mut store,
                1,
                vec![
                    file_step(1, "read", "source"),
                    file_step(2, "write", "workbook")
                ]
            ),
            Ok(stored(2)),
            "{category}"
        );
    }
}

#[test]
fn a_repeat_of_a_stored_stopped_row_stores_nothing_and_is_answered_with_its_count() {
    let mut store = store_with("utility_bill_payment");
    let mut stopped = log_line(2);
    stopped["event"] = json!("stopped");
    stopped["cause"] = json!("no_start");
    for _ in 0..3 {
        assert_eq!(put_log(&mut store, vec![stopped.clone()]), Ok(stored(1)));
        assert_eq!(rows(&store, "agent_log").len(), 2);
    }
    assert_eq!(
        put_log(&mut store, vec![stopped.clone(), log_line(3)]),
        Ok(stored(2))
    );
    assert_eq!(
        put_log(&mut store, vec![stopped, log_line(3)]),
        Ok(stored(2))
    );
    assert_eq!(rows(&store, "agent_log").len(), 3);
}

#[test]
fn the_two_replies_name_the_agent_of_their_rows() {
    let mut store = store_with("utility_bill_payment");
    let origin = "https://pay.test:443";
    assert_eq!(put_steps(&mut store, 1, navs(1, 3)), Ok(stored(3)));
    let fact = finished_row(1, 1, 3, 9);
    store
        .typed_commit(vec![fact])
        .expect("the fact of the compile");
    put_session(&mut store, origin, 1, 1, 1, vec!["QUJD".into()]).expect("a part");
    let text = |reply: Result<RunnerReply, ErrorName>| {
        reply.expect("a reply").to_json().expect("its text")
    };
    for from in [1, 4] {
        let steps = text(runner(
            &mut store,
            json!({"op": "load_steps", "agent": agent(1), "run": run(1), "number": 1, "from": from}),
        ));
        let named = format!(r#"{{"agent":"{}","steps":["#, agent(1));
        assert!(steps.starts_with(&named), "from {from}: {steps}");
    }
    assert_eq!(
        text(load_session(&mut store, origin, 1, 1)),
        format!(
            r#"{{"agent":"{}","origin":"{origin}","rev":1,"part":1,"of":1,"data":["QUJD"]}}"#,
            agent(1)
        )
    );
}
