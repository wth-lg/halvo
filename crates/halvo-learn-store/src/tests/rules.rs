use halvo_schema::executable::RunnerReply;
use halvo_schema::learning::record::{LearnEdit, LearnRun, RunStatus};
use halvo_schema::ErrorName;
use serde_json::{json, Value};

use super::coordinator::edit_row;
use super::host::{
    active_world, agent, compiled, locator, run, wf, with_locators, workflow, World, DAY,
};
use crate::{CoordinatorAnswer, LearnHost, Refused};

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

fn pay() -> Value {
    locator("role", "button", Some("Pay"))
}

fn stored(n: u32) -> RunnerReply {
    RunnerReply::parse(format!(r#"{{"stored":{n}}}"#).as_bytes()).expect("a stored reply")
}

fn newest(w: &World) -> Vec<LearnEdit> {
    let mut out: Vec<LearnEdit> = Vec::new();
    for row in w.host.edits() {
        match out.iter_mut().find(|e| e.edit == row.edit) {
            Some(e) if e.rev < row.rev => *e = row,
            Some(_) => {}
            None => out.push(row),
        }
    }
    out
}

fn ended(w: &World, r: u32) -> LearnRun {
    w.host
        .runs()
        .into_iter()
        .find(|row| row.run.as_str() == run(r) && row.rev() == 2)
        .expect("the run's rev 2")
}

#[test]
fn assisted_counts_the_distinct_steps_with_a_recovered_line() {
    let mut w = active_world();
    w.replay(2);
    w.log(
        2,
        &[
            (1, "recovered", 2, None),
            (2, "recovered", 2, None),
            (3, "step_ok", 2, None),
            (4, "step_ok", 3, None),
        ],
    );
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert!(matches!(
        ended(&w, 2).status,
        RunStatus::Ended { assisted: 1, .. }
    ));
}

#[test]
fn the_last_recovered_line_of_a_step_decides_whether_its_held_proposal_is_kept() {
    let mut w = active_world();
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(1)));
    w.log(
        2,
        &[
            (1, "recovered", 2, None),
            (2, "step_failed", 2, Some("timeout")),
            (3, "recovered", 2, None),
            (4, "step_ok", 2, None),
        ],
    );
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert_eq!(newest(&w).len(), 1);
}

#[test]
fn the_base_of_a_run_row_is_the_number_that_the_run_loaded_when_a_compile_lands_before_its_finish()
{
    let mut w = active_world();
    w.replay(2);
    w.compile(20, 2, compiled());
    w.log(2, &[(1, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert!(matches!(ended(&w, 2).status, RunStatus::Ended { base: Some(n), .. } if n.get() == 1));
}

#[test]
fn a_second_load_of_a_run_changes_nothing_and_its_served_edit_still_takes_its_move() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.replay(3);
    w.load(3);
    w.log(3, &[(1, "step_failed", 2, Some("step_failed"))]);
    assert!(w.finish(3, "replay", "failed").is_ok());
    assert_eq!(newest(&w)[0].state.name(), "reverted");
}

fn second_agent(w: &mut World) {
    w.host
        .put_workflow(workflow(&wf(2), "utility_bill_payment", "approved", None));
    let reply = w.coordinator(json!({"op": "create_agent", "workflow": wf(2), "agent": agent(2)}));
    assert!(reply.is_ok(), "create_agent of the second agent: {reply:?}");
    let reply = w.coordinator(json!({
        "op": "start_run", "workflow": wf(2), "agent": agent(2), "run": run(40), "mode": "compile",
    }));
    assert!(reply.is_ok(), "start_run of the second agent: {reply:?}");
    let body = json!({"op": "load", "agent": agent(2), "run": run(40)});
    assert!(w.runner(body).is_ok());
    let body = json!({"op": "put_playbook", "agent": agent(2), "run": run(40), "number": 1, "steps": compiled()});
    assert!(w.runner(body).is_ok());
}

#[test]
fn a_put_playbook_whose_run_row_names_another_agent_is_refused_with_binding_and_withdraws_nothing()
{
    let mut w = active_world();
    second_agent(&mut w);
    let mut row: Value = serde_json::from_str(
        &edit_row(
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
        )
        .json,
    )
    .expect("JSON");
    row["workflow"] = json!(wf(2));
    row["agent"] = json!(agent(2));
    w.host
        .commit(vec![
            halvo_schema::TypedRecord {
                kind: "learn_edit",
                json: row.to_string(),
            },
            edit_row(
                2,
                1,
                1,
                2,
                "active",
                None,
                &[14, 15, 16],
                (DAY, DAY),
                2,
                vec![loc("B")],
            ),
        ])
        .expect("the rows are stored");
    w.start(41, "compile");
    let body = json!({"op": "put_playbook", "agent": agent(2), "run": run(41), "number": 2, "steps": compiled()});
    let binding = ErrorName::read("binding").expect("a declared name");
    assert_eq!(w.runner(body), Err(Refused::Error(binding)));
    assert!(newest(&w).iter().all(|e| e.state.name() == "active"));
    assert_eq!(w.host.edits().len(), 2);
}

#[test]
fn finish_reads_the_edit_limit_of_a_step_again_and_stores_no_edit_above_it() {
    let mut w = active_world();
    let rows: Vec<_> = (1..=7)
        .flat_map(|e| {
            let name = format!("R{e}");
            [
                edit_row(
                    e,
                    1,
                    1,
                    2,
                    "provisional",
                    None,
                    &[10 + e],
                    (DAY, DAY),
                    0,
                    vec![loc(&name)],
                ),
                edit_row(
                    e,
                    2,
                    1,
                    2,
                    "reverted",
                    None,
                    &[10 + e],
                    (DAY, DAY),
                    0,
                    vec![loc(&name)],
                ),
            ]
        })
        .collect();
    w.host.commit(rows).expect("the rows are stored");
    let served = w.replay(2);
    let held = with_locators(served[1].clone(), "button Pay now", vec![loc("X"), pay()]);
    assert_eq!(w.put(2, 1, vec![held]), Ok(stored(1)));
    w.learn(3, 2, "button Pay soon", vec![loc("Y"), pay()]);
    assert_eq!(newest(&w).len(), 8);
    w.failed_use(4, 2, "step_failed");
    assert!(newest(&w).iter().all(|e| e.state.name() == "reverted"));
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert_eq!(newest(&w).len(), 8);
}

#[test]
fn a_store_fault_of_the_agent_store_is_answered_as_that_error_and_never_as_a_refused_body() {
    let mut w = active_world();
    w.replay(2);
    let served = w.load_steps(2, 1, 1);
    w.host.agent.fault = true;
    let read =
        w.coordinator(json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(1)}));
    assert_eq!(read, Err(Refused::Error(ErrorName::Store)));
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(
        w.put(2, 1, vec![proposal]),
        Err(Refused::Error(ErrorName::Store))
    );
    w.host.agent.fault = false;
    assert!(matches!(
        w.coordinator(json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(1)})),
        Ok(CoordinatorAnswer::Refinements(_))
    ));
}

fn chain(w: &mut World, with_revert: bool) {
    let a = vec![loc("a1"), loc("a2"), loc("a3"), loc("a4")];
    let mut rows = vec![edit_row(
        1,
        1,
        1,
        2,
        "active",
        None,
        &[11, 12, 13],
        (DAY, DAY),
        2,
        a,
    )];
    if with_revert {
        let r = vec![loc("r"), loc("a1"), loc("a2"), loc("a3")];
        rows.push(edit_row(
            2,
            1,
            1,
            2,
            "provisional",
            Some(1),
            &[14],
            (DAY, DAY),
            0,
            r.clone(),
        ));
        rows.push(edit_row(
            2,
            2,
            1,
            2,
            "reverted",
            Some(1),
            &[14],
            (DAY, DAY),
            0,
            r,
        ));
    }
    let n = vec![loc("n1"), loc("n2"), loc("a1"), loc("a2")];
    rows.push(edit_row(
        3,
        1,
        1,
        2,
        "active",
        Some(1),
        &[15, 16, 17],
        (DAY, DAY),
        2,
        n,
    ));
    w.host.commit(rows).expect("the rows are stored");
}

fn admitted(w: &mut World, r: u32, name: &str) -> Result<(), halvo_learn::AdmitCause> {
    let served = w.replay(r);
    let names: Vec<String> = served[1]["target"]["locators"]
        .as_array()
        .expect("locators")
        .iter()
        .map(|l| l["name"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(names, ["n1", "n2", "a1", "a2", "Pay"]);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc(name)]);
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": 1, "steps": [proposal]});
    let Ok(halvo_schema::executable::RunnerCall::PutPlaybook(b)) =
        halvo_schema::executable::RunnerCall::parse(body.to_string().as_bytes())
    else {
        panic!("a put_playbook body");
    };
    let answers = w
        .handlers
        .propose(&mut w.host, &b)
        .expect("the proposal is read");
    answers[0].1
}

#[test]
fn only_a_reverted_edit_bars_what_it_brought_and_an_active_edit_cut_from_the_served_list_bars_nothing(
) {
    let mut w = active_world();
    chain(&mut w, false);
    assert_eq!(admitted(&mut w, 2, "a4"), Ok(()));
    assert_eq!(admitted(&mut w, 3, "a3"), Ok(()));
}

#[test]
fn a_reverted_edit_bars_only_the_locators_that_its_base_edit_does_not_list() {
    let mut w = active_world();
    chain(&mut w, true);
    assert_eq!(admitted(&mut w, 2, "a3"), Ok(()));
    assert_eq!(
        admitted(&mut w, 3, "r"),
        Err(halvo_learn::AdmitCause::Rejected)
    );
}
