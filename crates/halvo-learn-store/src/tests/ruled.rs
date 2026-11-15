use halvo_learn::AdmitCause;
use halvo_schema::executable::{RunId, RunnerCall, RunnerReply};
use halvo_schema::WorkflowId;
use serde_json::{json, Value};

use super::coordinator::edit_row;
use super::host::{
    active_world, active_world_with, agent, click, compiled, locator, run, wf, with_locators,
    workflow, World, DAY,
};
use crate::{LearnHandlers, LearnHost, RUNS_HELD_MAX};

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

fn pay() -> Value {
    locator("role", "button", Some("Pay"))
}

fn css(c: char, len: usize) -> Value {
    json!({"by": "css", "value": c.to_string().repeat(len), "name": null})
}

fn stored(n: u32) -> RunnerReply {
    RunnerReply::parse(format!(r#"{{"stored":{n}}}"#).as_bytes()).expect("a stored reply")
}

fn id(r: u32) -> RunId {
    RunId::parse(&run(r)).expect("a run id")
}

fn revs(w: &World, r: u32) -> Vec<u32> {
    let mut out: Vec<u32> = w
        .host
        .runs()
        .iter()
        .filter(|row| row.run.as_str() == run(r))
        .map(|row| row.rev())
        .collect();
    out.sort();
    out
}

fn answer(w: &mut World, r: u32, number: u32, step: Value) -> Result<(), AdmitCause> {
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": number, "steps": [step]});
    let Ok(RunnerCall::PutPlaybook(b)) = RunnerCall::parse(body.to_string().as_bytes()) else {
        panic!("a put_playbook body");
    };
    let answers = w
        .handlers
        .propose(&mut w.host, &b)
        .expect("the proposal is read");
    assert_eq!(answers.len(), 1);
    answers[0].1
}

fn holds(w: &World, r: u32) -> bool {
    w.handlers.facts.contains_key(&id(r)) || w.handlers.starts.contains_key(&id(r))
}

fn delete_workflow(w: &mut World, n: u32) {
    w.host
        .agent
        .gone
        .push(WorkflowId::parse(&wf(n)).expect("a workflow id"));
}

#[test]
fn f1_a_body_under_two_numbers_above_the_loaded_one_is_answered_base_and_holds_nothing() {
    let mut w = active_world();
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(
        answer(&mut w, 2, 3, proposal.clone()),
        Err(AdmitCause::Base)
    );
    assert_eq!(w.put(2, 3, vec![proposal]), Ok(stored(0)));
    assert!(w.handlers.facts[&id(2)].candidates.is_empty());
}

#[test]
fn f1_a_body_under_the_next_number_stores_an_edit_whose_base_is_the_loaded_number() {
    let mut w = active_world();
    let playbooks = w.host.agent_rows("agent_playbook");
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(w.put(2, 2, vec![proposal]), Ok(stored(1)));
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    let edits = w.host.edits();
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].rev.get(), edits[0].base.get()), (1, 1));
    assert_eq!(w.host.agent_rows("agent_playbook"), playbooks);
}

#[test]
fn f1_a_compile_that_stores_the_next_number_before_the_proposal_makes_it_base() {
    let mut w = active_world();
    let served = w.replay(2);
    w.compile(20, 2, compiled());
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(answer(&mut w, 2, 2, proposal), Err(AdmitCause::Base));
}

#[test]
fn f1_under_the_next_number_the_base_edit_of_the_loaded_number_makes_a_step_too_long() {
    let mut steps = compiled();
    steps[1] = click(2, "button Pay", vec![css('a', 600)]);
    let mut w = active_world_with(steps);
    w.host
        .commit(vec![edit_row(
            1,
            1,
            1,
            2,
            "active",
            None,
            &[11, 12, 13],
            (DAY, DAY),
            2,
            vec![css('b', 700)],
        )])
        .expect("the row is stored");
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![css('c', 700)]);
    assert_eq!(answer(&mut w, 2, 2, proposal), Err(AdmitCause::Length));
}

#[test]
fn f1_under_the_next_number_a_locator_that_a_reverted_edit_of_the_loaded_number_brought_is_rejected(
) {
    let mut w = active_world();
    w.host
        .commit(vec![
            edit_row(
                1,
                1,
                1,
                2,
                "provisional",
                None,
                &[11],
                (DAY, DAY),
                0,
                vec![loc("X")],
            ),
            edit_row(
                1,
                2,
                1,
                2,
                "reverted",
                None,
                &[11],
                (DAY, DAY),
                0,
                vec![loc("X")],
            ),
        ])
        .expect("the rows are stored");
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("X"), pay()]);
    assert_eq!(answer(&mut w, 2, 2, proposal), Err(AdmitCause::Rejected));
}

#[test]
fn f1_under_the_next_number_eight_edits_of_the_step_under_the_loaded_number_are_full() {
    let mut w = active_world();
    let rows: Vec<_> = (1..=8)
        .map(|e| {
            edit_row(
                e,
                1,
                1,
                2,
                "withdrawn",
                None,
                &[10 + e],
                (DAY, DAY),
                0,
                vec![loc(&format!("W{e}"))],
            )
        })
        .collect();
    w.host.commit(rows).expect("the rows are stored");
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("N"), pay()]);
    assert_eq!(answer(&mut w, 2, 2, proposal), Err(AdmitCause::Full));
}

#[test]
fn f2_a_run_that_starts_again_on_a_new_value_before_its_load_is_still_a_lost_run() {
    let mut w = active_world();
    w.replay(2);
    w.handlers = LearnHandlers::new();
    w.start(2, "replay");
    w.load(2);
    let served = w.load_steps(2, 1, 1);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(0)));
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert!(w.host.edits().is_empty());
    assert_eq!(revs(&w, 2), [1]);
}

#[test]
fn f2_a_run_that_one_value_started_and_another_loads_is_served_the_compiled_steps_and_leaves_rev_1()
{
    let mut w = active_world();
    w.host
        .commit(vec![edit_row(
            1,
            1,
            1,
            2,
            "active",
            None,
            &[11, 12, 13],
            (DAY, DAY),
            2,
            vec![loc("A")],
        )])
        .expect("the row is stored");
    w.start(2, "replay");
    w.handlers = LearnHandlers::new();
    w.load(2);
    let served = w.load_steps(2, 1, 1);
    assert_eq!(served, compiled());
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("B"), pay()]);
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(0)));
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert_eq!(w.host.edits().len(), 1);
    assert_eq!(revs(&w, 2), [1]);
}

#[test]
fn f2_a_later_load_adds_the_slot_values_of_its_reply_to_the_screen_of_the_cause_value() {
    let mut w = active_world();
    let bind = |w: &mut World, value: &str| {
        let reply = w.coordinator(json!({
            "op": "set_bindings", "workflow": wf(1), "agent": agent(1),
            "slots": [{"name": "account", "value": [value]}],
        }));
        assert!(reply.is_ok(), "set_bindings: {reply:?}");
    };
    bind(&mut w, "1111-2222");
    let served = w.replay(2);
    bind(&mut w, "9999-8888");
    w.load(2);
    let proposal = with_locators(
        served[1].clone(),
        "button Pay now",
        vec![loc("Pay 9999-8888"), pay()],
    );
    assert_eq!(answer(&mut w, 2, 1, proposal), Err(AdmitCause::Value));
}

#[test]
fn f2_a_start_run_that_arrives_twice_in_one_life_changes_neither_the_rows_nor_what_the_value_holds()
{
    let mut w = active_world();
    w.start(2, "replay");
    let rows = w.host.learn_rows("learn_run");
    let held = format!("{:?}", w.handlers.starts);
    w.start(2, "replay");
    assert_eq!(w.host.learn_rows("learn_run"), rows);
    assert_eq!(format!("{:?}", w.handlers.starts), held);
}

#[test]
fn f3_a_replay_run_with_no_load_holds_nothing_stores_no_edit_and_keeps_rev_1() {
    let mut w = active_world();
    w.start(2, "replay");
    let proposal = with_locators(
        compiled()[1].clone(),
        "button Pay now",
        vec![loc("A"), pay()],
    );
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(0)));
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert!(w.host.edits().is_empty());
    assert_eq!(revs(&w, 2), [1]);
    assert!(!holds(&w, 2));
}

#[test]
fn f4_an_end_run_that_the_agent_store_refuses_for_a_deleted_workflow_drops_the_facts_too() {
    let mut w = active_world();
    w.replay(2);
    delete_workflow(&mut w, 1);
    let reply = w
        .coordinator(json!({"op": "end_run", "workflow": wf(1), "agent": agent(1), "run": run(2)}));
    assert!(
        reply.is_err(),
        "the agent store refuses the end_run: {reply:?}"
    );
    assert!(!holds(&w, 2));
}

#[test]
fn f4_a_finish_that_the_agent_store_refuses_for_a_run_that_still_goes_keeps_the_facts() {
    let mut w = active_world();
    w.replay(2);
    w.log(2, &[(1, "step_ok", 2, None)]);
    let record = |workflow: String| {
        json!({
            "v": 1, "workflow": workflow, "agent": agent(1), "run": run(2), "day": DAY,
            "mode": "replay", "outcome": "completed", "playbook": null, "steps": 3, "prompts": 0,
            "handbacks": 0, "confirmations": 0, "effects": 0, "unknown": 0, "model_calls": 0,
            "tokens_in": 0, "tokens_out": 0, "wall_s": 9, "model": null,
            "before_start": null, "cut": null, "no_key": null, "not_to_account": null,
            "incomplete": null, "reply_to_other": null, "more_pages": null, "held_recipient": null,
            "organized": null, "other_task": null, "held_earlier": null, "earlier_page": null,
        })
    };
    let wrong = json!({"op": "finish", "agent": agent(1), "run": run(2), "record": record(wf(2))});
    assert!(w.runner(wrong).is_err());
    assert!(w.handlers.facts.contains_key(&id(2)));
    let right = json!({"op": "finish", "agent": agent(1), "run": run(2), "record": record(wf(1))});
    assert!(w.runner(right).is_ok());
    assert_eq!(revs(&w, 2), [1, 2]);
}

#[test]
fn f4_a_refused_log_body_of_a_run_whose_workflow_is_gone_drops_its_facts() {
    let mut w = active_world();
    w.replay(2);
    delete_workflow(&mut w, 1);
    let line = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(2), "seq": 1, "day": DAY,
        "at": 1_794_500_000_000_000_000_i64, "event": "step_ok", "step": 2, "cause": null,
        "model": null, "ms": null, "tokens_in": null, "tokens_out": null,
    });
    let body = json!({"op": "log", "agent": agent(1), "run": run(2), "records": [line]});
    assert!(w.runner(body).is_err());
    assert!(!holds(&w, 2));
}

#[test]
fn f4_a_start_run_of_another_agent_drops_the_facts_of_a_run_whose_workflow_is_gone() {
    let mut w = active_world();
    w.replay(2);
    w.host
        .put_workflow(workflow(&wf(2), "utility_bill_payment", "approved", None));
    let reply = w.coordinator(json!({"op": "create_agent", "workflow": wf(2), "agent": agent(2)}));
    assert!(reply.is_ok(), "create_agent: {reply:?}");
    delete_workflow(&mut w, 1);
    assert!(holds(&w, 2));
    let reply = w.coordinator(json!({
        "op": "start_run", "workflow": wf(2), "agent": agent(2), "run": run(40), "mode": "compile",
    }));
    assert!(reply.is_ok(), "start_run: {reply:?}");
    assert!(!holds(&w, 2));
    assert!(holds(&w, 40));
}

#[test]
fn f4_at_most_runs_held_max_runs_are_held_and_the_first_made_goes_first() {
    assert_eq!(RUNS_HELD_MAX, 8);
    let mut w = active_world();
    for r in 2..2 + RUNS_HELD_MAX as u32 {
        w.replay(r);
    }
    assert!((2..2 + RUNS_HELD_MAX as u32).all(|r| holds(&w, r)));
    let last = 2 + RUNS_HELD_MAX as u32;
    w.start(last, "replay");
    assert!(!holds(&w, 2));
    assert!((3..=last).all(|r| holds(&w, r)));
    let commits = w.host.commits.get();
    w.log(2, &[(1, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert_eq!(w.host.commits.get(), commits);
    assert_eq!(revs(&w, 2), [1]);
}

#[test]
fn f4_the_value_holds_no_slot_value_of_a_compile_run() {
    let mut w = active_world();
    let reply = w.coordinator(json!({
        "op": "set_bindings", "workflow": wf(1), "agent": agent(1),
        "slots": [{"name": "account", "value": ["1111-2222"]}],
    }));
    assert!(reply.is_ok(), "set_bindings: {reply:?}");
    w.start(20, "compile");
    w.load(20);
    assert!(w.handlers.facts[&id(20)].slots.is_empty());
    w.replay(21);
    assert_eq!(w.handlers.facts[&id(21)].slots, ["1111-2222"]);
}

#[test]
fn f5_a_step_whose_n_lies_beyond_the_playbook_is_answered_base() {
    let mut w = active_world();
    w.replay(2);
    let mut step = with_locators(compiled()[2].clone(), "button Next now", vec![loc("A")]);
    step["n"] = json!(4);
    assert_eq!(answer(&mut w, 2, 1, step), Err(AdmitCause::Base));
}

#[test]
fn f5_steps_fetched_under_another_number_than_the_loaded_one_are_answered_base() {
    let mut w = active_world();
    w.compile(20, 2, compiled());
    w.start(2, "replay");
    w.load(2);
    let served = w.load_steps(2, 1, 1);
    assert_eq!(served, compiled());
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(answer(&mut w, 2, 2, proposal), Err(AdmitCause::Base));
}

#[test]
fn f4_a_load_that_a_store_fault_refuses_drops_nothing() {
    let mut w = active_world();
    w.replay(2);
    w.host.agent.fault = true;
    assert!(w
        .runner(json!({"op": "load", "agent": agent(1), "run": run(2)}))
        .is_err());
    w.host.agent.fault = false;
    assert!(holds(&w, 2));
    assert!(w.handlers.facts.contains_key(&id(2)));
}

#[test]
fn f4_a_refused_load_of_a_run_that_has_only_a_start_drops_the_start() {
    let mut w = active_world();
    w.start(2, "replay");
    assert!(holds(&w, 2));
    delete_workflow(&mut w, 1);
    assert!(w
        .runner(json!({"op": "load", "agent": agent(1), "run": run(2)}))
        .is_err());
    assert!(!holds(&w, 2));
}

fn bind(w: &mut World, value: &str) {
    let reply = w.coordinator(json!({
        "op": "set_bindings", "workflow": wf(1), "agent": agent(1),
        "slots": [{"name": "account", "value": [value]}],
    }));
    assert!(reply.is_ok(), "set_bindings: {reply:?}");
}

fn second_agent(w: &mut World) {
    w.host
        .put_workflow(workflow(&wf(2), "utility_bill_payment", "approved", None));
    let reply = w.coordinator(json!({"op": "create_agent", "workflow": wf(2), "agent": agent(2)}));
    assert!(reply.is_ok(), "create_agent: {reply:?}");
}

#[test]
fn g1_a_refused_load_under_another_agent_leaves_the_run_as_it_was() {
    let mut w = active_world();
    bind(&mut w, "1111-2222");
    w.replay(2);
    let slots = w.handlers.facts[&id(2)].slots.clone();
    let start = format!("{:?}", w.handlers.starts.get(&id(2)));
    assert!(w
        .runner(json!({"op": "load", "agent": agent(9), "run": run(2)}))
        .is_err());
    assert!(w.handlers.facts.contains_key(&id(2)));
    assert_eq!(w.handlers.facts[&id(2)].slots, slots);
    assert_eq!(format!("{:?}", w.handlers.starts.get(&id(2))), start);
}

#[test]
fn g2_a_later_load_keeps_the_loaded_number_after_a_compile_of_another_run() {
    let mut w = active_world();
    let served = w.replay(2);
    w.compile(20, 2, compiled());
    w.load(2);
    assert_eq!(w.handlers.facts[&id(2)].loaded.map(|n| n.get()), Some(1));
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(answer(&mut w, 2, 2, proposal), Err(AdmitCause::Base));
}

#[test]
fn g3_a_later_load_keeps_the_slot_values_of_the_first() {
    let mut w = active_world();
    bind(&mut w, "1111-2222");
    let served = w.replay(2);
    bind(&mut w, "9999-8888");
    w.load(2);
    let proposal = with_locators(
        served[1].clone(),
        "button Pay now",
        vec![loc("Pay 1111-2222"), pay()],
    );
    assert_eq!(answer(&mut w, 2, 1, proposal), Err(AdmitCause::Value));
}

#[test]
fn g4_a_load_under_another_agent_than_the_start_makes_no_facts() {
    let mut w = active_world();
    second_agent(&mut w);
    w.start(2, "replay");
    assert!(w
        .runner(json!({"op": "load", "agent": agent(2), "run": run(2)}))
        .is_ok());
    assert!(!w.handlers.facts.contains_key(&id(2)));
    assert!(w.handlers.starts.contains_key(&id(2)));
}

#[test]
fn g4_an_end_run_under_another_agent_drops_nothing() {
    let mut w = active_world();
    second_agent(&mut w);
    w.replay(2);
    let reply = w
        .coordinator(json!({"op": "end_run", "workflow": wf(2), "agent": agent(2), "run": run(2)}));
    assert!(reply.is_ok(), "end_run: {reply:?}");
    assert!(w.handlers.facts.contains_key(&id(2)));
    assert!(w.handlers.starts.contains_key(&id(2)));
}

#[test]
fn g4_a_store_fault_in_the_sweep_at_a_start_is_answered_store_and_a_repeat_stores_the_row_once() {
    let mut w = active_world();
    w.replay(2);
    w.host.agent.fault = true;
    let body = json!({"op": "start_run", "workflow": wf(1), "agent": agent(1), "run": run(3), "mode": "replay"});
    assert_eq!(
        w.coordinator(body.clone()),
        Err(crate::Refused::Error(halvo_schema::ErrorName::Store))
    );
    assert!(revs(&w, 3).is_empty());
    w.host.agent.fault = false;
    assert!(w.coordinator(body).is_ok());
    assert_eq!(revs(&w, 3), [1]);
}
