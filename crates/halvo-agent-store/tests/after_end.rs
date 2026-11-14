mod common;

use common::*;
use halvo_agent_store::MemoryStore;
use serde_json::{json, Value};

#[derive(Clone, Copy)]
enum End {
    Finish,
    EndRun,
}

const ORIGIN: &str = "https://bank.test:443";

fn session(r: u32, origin: &str) -> Value {
    json!({"op": "put_session", "agent": agent(1), "run": run(r), "origin": origin, "rev": r, "part": 1, "of": 1, "data": ["QUJD"]})
}

fn playbook(r: u32, number: u32, n: u32) -> Value {
    json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": number, "steps": [step(n, "navigate", "read")]})
}

fn log(r: u32, seq: u32) -> Value {
    json!({"op": "log", "agent": agent(1), "run": run(r), "records": [{
        "v": 1,
        "workflow": wf(1),
        "agent": agent(1),
        "run": run(r),
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
    }]})
}

fn load(r: u32) -> Value {
    json!({"op": "load", "agent": agent(1), "run": run(r)})
}

fn load_steps(r: u32) -> Value {
    json!({"op": "load_steps", "agent": agent(1), "run": run(r), "number": 1, "from": 1})
}

fn load_session(r: u32) -> Value {
    json!({"op": "load_session", "agent": agent(1), "run": run(r), "origin": ORIGIN, "rev": 1, "part": 1})
}

fn ended(end: End) -> MemoryStore {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(runner(&mut store, playbook(1, 1, 1)), Ok(stored(1)));
    assert_eq!(runner(&mut store, session(1, ORIGIN)), Ok(stored(1)));
    assert_eq!(runner(&mut store, log(1, 1)), Ok(stored(1)));
    match end {
        End::Finish => assert_eq!(
            finish(&mut store, run_record(1, Some("compile"), "completed")),
            Ok(stored(1))
        ),
        End::EndRun => assert_eq!(end_run(&mut store, 1), Ok(accepted())),
    }
    store
}

fn counts(store: &MemoryStore) -> [usize; 3] {
    [
        rows(store, "agent_playbook").len(),
        rows(store, "agent_session").len(),
        rows(store, "agent_log").len(),
    ]
}

fn refused_after(end: End, bodies: &[Value]) -> MemoryStore {
    let mut store = ended(end);
    let before = counts(&store);
    for body in bodies {
        assert_eq!(
            runner(&mut store, body.clone()),
            Err(declared("effect")),
            "{body}"
        );
    }
    assert_eq!(counts(&store), before);
    store
}

fn load_is_refused(end: End) {
    let mut store = refused_after(end, &[load(1)]);
    assert!(runner(&mut store, load(2)).is_ok());
}

fn load_steps_is_refused(end: End) {
    let mut store = refused_after(end, &[load_steps(1)]);
    assert!(runner(&mut store, load_steps(2)).is_ok());
}

fn load_session_is_refused(end: End) {
    let mut store = refused_after(end, &[load_session(1)]);
    assert!(runner(&mut store, load_session(2)).is_ok());
}

fn put_playbook_is_refused(end: End) {
    let mut store = refused_after(
        end,
        &[playbook(1, 1, 2), playbook(1, 1, 1), playbook(1, 2, 1)],
    );
    assert_eq!(runner(&mut store, playbook(2, 1, 2)), Ok(stored(1)));
}

fn put_session_is_refused(end: End) {
    let mut store = refused_after(
        end,
        &[session(1, "https://other.test:443"), session(1, ORIGIN)],
    );
    assert_eq!(runner(&mut store, session(2, ORIGIN)), Ok(stored(1)));
}

fn log_is_refused(end: End) {
    let mut store = refused_after(end, &[log(1, 2), log(1, 1)]);
    assert_eq!(runner(&mut store, log(2, 1)), Ok(stored(1)));
}

#[test]
fn load_after_finish_is_refused() {
    load_is_refused(End::Finish);
}

#[test]
fn load_after_end_run_is_refused() {
    load_is_refused(End::EndRun);
}

#[test]
fn load_steps_after_finish_is_refused() {
    load_steps_is_refused(End::Finish);
}

#[test]
fn load_steps_after_end_run_is_refused() {
    load_steps_is_refused(End::EndRun);
}

#[test]
fn load_session_after_finish_is_refused() {
    load_session_is_refused(End::Finish);
}

#[test]
fn load_session_after_end_run_is_refused() {
    load_session_is_refused(End::EndRun);
}

#[test]
fn put_playbook_after_finish_is_refused() {
    put_playbook_is_refused(End::Finish);
}

#[test]
fn put_playbook_after_end_run_is_refused() {
    put_playbook_is_refused(End::EndRun);
}

#[test]
fn put_session_after_finish_is_refused() {
    put_session_is_refused(End::Finish);
}

#[test]
fn put_session_after_end_run_is_refused() {
    put_session_is_refused(End::EndRun);
}

#[test]
fn log_after_finish_is_refused() {
    log_is_refused(End::Finish);
}

#[test]
fn log_after_end_run_is_refused() {
    log_is_refused(End::EndRun);
}

#[test]
fn the_runner_s_finish_repeated_after_its_end_is_answered_as_the_first() {
    let mut store = ended(End::Finish);
    assert_eq!(
        finish(&mut store, run_record(1, Some("compile"), "completed")),
        Ok(stored(1))
    );
    assert_eq!(rows(&store, "agent_run").len(), 1);
    let mut store = ended(End::EndRun);
    assert_eq!(
        finish(&mut store, run_record(1, Some("compile"), "completed")),
        Err(declared("effect"))
    );
}

fn second_agent(store: &mut MemoryStore) {
    store.put_workflow(workflow(&wf(2), "utility_bill_payment", "approved", None));
    create(store, 2, 2).expect("create_agent of the second agent");
}

#[test]
fn load_of_another_agent_that_names_an_ended_run_is_refused() {
    let mut store = ended(End::Finish);
    second_agent(&mut store);
    let got = runner(
        &mut store,
        json!({"op": "load", "agent": agent(2), "run": run(1)}),
    );
    assert_eq!(got.map(|_| ()), Err(declared("effect")));
}

#[test]
fn log_of_another_agent_that_names_an_ended_run_is_refused_with_binding_first() {
    let mut store = ended(End::Finish);
    second_agent(&mut store);
    let mut body = log(1, 1);
    body["agent"] = json!(agent(2));
    body["records"][0]["agent"] = json!(agent(2));
    body["records"][0]["workflow"] = json!(wf(2));
    assert_eq!(runner(&mut store, body), Err(declared("binding")));
}

#[test]
fn load_of_an_agent_that_is_not_the_workflow_s_names_an_ended_run_with_binding_first() {
    let mut store = ended(End::Finish);
    let got = runner(
        &mut store,
        json!({"op": "load", "agent": agent(3), "run": run(1)}),
    );
    assert_eq!(got.map(|_| ()), Err(declared("binding")));
}

#[test]
fn load_steps_of_an_ended_run_with_an_unknown_number_is_refused_with_effect_first() {
    let mut store = ended(End::Finish);
    let got = runner(
        &mut store,
        json!({"op": "load_steps", "agent": agent(1), "run": run(1), "number": 5, "from": 1}),
    );
    assert_eq!(got.map(|_| ()), Err(declared("effect")));
}

#[test]
fn load_session_of_an_ended_run_with_a_missing_part_is_refused_with_effect_first() {
    let mut store = ended(End::EndRun);
    let got = runner(
        &mut store,
        json!({"op": "load_session", "agent": agent(1), "run": run(1), "origin": ORIGIN, "rev": 9, "part": 1}),
    );
    assert_eq!(got.map(|_| ()), Err(declared("effect")));
}

#[test]
fn put_playbook_of_an_ended_run_with_a_bad_number_is_refused_with_effect_first() {
    let mut store = ended(End::Finish);
    let got = runner(
        &mut store,
        json!({"op": "put_playbook", "agent": agent(1), "run": run(1), "number": 5, "steps": [step(1, "navigate", "read")]}),
    );
    assert_eq!(got, Err(declared("effect")));
}
