use halvo_schema::executable::RunId;
use serde_json::{json, Value};

use super::host::{active_world, agent, compiled, locator, run, wf, with_locators};
use crate::LearnHandlers;

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

fn pay() -> Value {
    locator("role", "button", Some("Pay"))
}

#[test]
fn a_run_that_loads_after_a_restart_is_a_lost_run_and_learns_nothing() {
    let mut w = active_world();
    w.replay(2);
    w.log(2, &[(1, "step_ok", 1, None)]);
    w.handlers = LearnHandlers::new();
    w.load(2);
    let served = w.load_steps(2, w.loaded, 1);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert!(w.put(2, w.loaded, vec![proposal]).is_ok());
    w.log(2, &[(2, "recovered", 2, None), (3, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert!(w.host.edits().is_empty());
    let revs: Vec<u32> = w
        .host
        .runs()
        .iter()
        .filter(|r| r.run.as_str() == run(2))
        .map(|r| r.rev())
        .collect();
    assert_eq!(revs, [1]);
}

#[test]
fn a_compile_run_that_sends_no_load_keeps_its_run_row_at_rev_1() {
    let mut w = active_world();
    w.start(20, "compile");
    assert!(w.put(20, 2, compiled()).is_ok());
    assert!(w.finish(20, "compile", "completed").is_ok());
    let rows: Vec<_> = w
        .host
        .runs()
        .into_iter()
        .filter(|r| r.run.as_str() == run(20))
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].rev(), 1);
}

#[test]
fn an_end_run_drops_the_facts_of_its_run() {
    let mut w = active_world();
    w.replay(2);
    let reply = w
        .coordinator(json!({"op": "end_run", "workflow": wf(1), "agent": agent(1), "run": run(2)}));
    assert!(reply.is_ok(), "end_run: {reply:?}");
    let id = RunId::parse(&run(2)).expect("a run id");
    assert!(!w.handlers.facts.contains_key(&id));
    assert!(!w.handlers.starts.contains_key(&id));
}

#[test]
fn a_proposal_for_a_step_that_was_not_served_to_its_run_is_refused_with_base_and_holds_nothing() {
    let mut w = active_world();
    w.start(2, "replay");
    w.load(2);
    let served = w.load_steps(2, 1, 3);
    assert_eq!(served.len(), 1);
    let proposal = with_locators(
        compiled()[1].clone(),
        "button Pay now",
        vec![loc("A"), pay()],
    );
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(2), "number": 1, "steps": [proposal]});
    let Ok(halvo_schema::executable::RunnerCall::PutPlaybook(b)) =
        halvo_schema::executable::RunnerCall::parse(body.to_string().as_bytes())
    else {
        panic!("a put_playbook body");
    };
    let answers = w
        .handlers
        .propose(&mut w.host, &b)
        .expect("the proposal is read");
    assert_eq!(answers[0].1, Err(halvo_learn::AdmitCause::Base));
    let id = RunId::parse(&run(2)).expect("a run id");
    assert!(w.handlers.facts[&id].candidates.is_empty());
}
