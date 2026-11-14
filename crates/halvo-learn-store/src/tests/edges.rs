use halvo_schema::executable::{RunnerCall, RunnerReply};
use halvo_schema::learning::record::{LearnEdit, RunStatus};
use serde_json::{json, Value};

use super::coordinator::edit_row;
use super::host::{
    active_world, active_world_with, agent, click, compiled, locator, navigate, run, wf,
    with_locators, World, DAY,
};
use crate::{CoordinatorAnswer, LearnHost};

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

fn direct_compile(w: &mut World, r: u32, number: u32) {
    let now = w.host.agent_now();
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": number, "steps": compiled()});
    let reply = halvo_agent_store::runner(&mut w.host.agent, now, body.to_string().as_bytes());
    assert!(
        reply.is_ok(),
        "the agent store stores number {number}: {reply:?}"
    );
}

fn names(step: &Value) -> Vec<String> {
    step["target"]["locators"]
        .as_array()
        .expect("locators")
        .iter()
        .map(|l| l["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn a_proposal_that_names_another_number_than_the_one_the_run_loaded_is_refused_with_base() {
    let mut w = active_world();
    w.compile(20, 2, compiled());
    let served = w.replay(2);
    assert_eq!(w.loaded, 2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    for (number, want) in [(1, Err(halvo_learn::AdmitCause::Base)), (3, Ok(()))] {
        let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(2), "number": number, "steps": [proposal]});
        let Ok(RunnerCall::PutPlaybook(b)) = RunnerCall::parse(body.to_string().as_bytes()) else {
            panic!("a put_playbook body");
        };
        let answers = w
            .handlers
            .propose(&mut w.host, &b)
            .expect("the proposal is read");
        assert_eq!(answers[0].1, want, "number {number}");
    }
}

#[test]
fn an_unverified_run_with_step_ok_makes_no_clean_use_and_a_failure_that_does_not_count_names_no_failed_step(
) {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.replay(3);
    w.log(
        3,
        &[
            (1, "step_ok", 2, None),
            (2, "step_failed", 3, Some("timeout")),
        ],
    );
    assert!(w.finish(3, "replay", "failed").is_ok());
    let e = &newest(&w)[0];
    assert_eq!(
        (e.rev.get(), e.state.name(), e.clean),
        (1, "provisional", 0)
    );
    let row = w
        .host
        .runs()
        .into_iter()
        .find(|r| r.run.as_str() == run(3) && r.rev() == 2)
        .expect("rev 2 of run 3");
    assert!(matches!(
        row.status,
        RunStatus::Ended {
            failed_step: None,
            ..
        }
    ));
}

#[test]
fn a_candidate_on_a_step_whose_provisional_edit_stays_provisional_is_dropped() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    let served = w.replay(3);
    let proposal = with_locators(
        served[1].clone(),
        "button Pay today",
        vec![loc("B"), loc("A"), pay()],
    );
    assert_eq!(w.put(3, 1, vec![proposal]), Ok(stored(1)));
    w.log(3, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(3, "replay", "completed").is_ok());
    let edits = newest(&w);
    assert_eq!(edits.len(), 1);
    assert_eq!((edits[0].state.name(), edits[0].clean), ("provisional", 1));
}

#[test]
fn a_candidate_of_a_run_whose_compile_number_is_no_longer_the_newest_at_finish_is_dropped() {
    let mut w = active_world();
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(1)));
    w.compile(20, 2, compiled());
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert!(w.host.edits().is_empty());
}

#[test]
fn a_candidate_whose_list_on_an_edit_made_active_after_its_admission_would_not_fit_is_dropped() {
    let long = |c: char| c.to_string().repeat(650);
    let mut steps = compiled();
    steps[1] = click(2, "button Pay", vec![locator("css", &long('a'), None)]);
    let mut w = active_world_with(steps);
    let served = w.replay(2);
    let late = with_locators(
        served[1].clone(),
        "button Pay",
        vec![locator("css", &long('p'), None)],
    );
    assert_eq!(w.put(2, 1, vec![late]), Ok(stored(1)));
    let first = w.replay(3);
    let edit = with_locators(
        first[1].clone(),
        "button Pay",
        vec![locator("css", &long('e'), None)],
    );
    assert_eq!(w.put(3, 1, vec![edit]), Ok(stored(1)));
    w.log(3, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(3, "replay", "completed").is_ok());
    w.clean_use(4, 2);
    w.clean_use(5, 2);
    assert_eq!(newest(&w)[0].state.name(), "active");
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert_eq!(newest(&w).len(), 1);
}

#[test]
fn the_read_lists_no_edit_of_a_lower_compile_number_after_a_stop_between_a_compiles_two_commits() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.start(3, "replay");
    w.load(3);
    direct_compile(&mut w, 3, 2);
    let reply =
        w.coordinator(json!({"op": "list_refinements", "workflow": wf(1), "agent": agent(1)}));
    let Ok(CoordinatorAnswer::Refinements(reply)) = reply else {
        panic!("the read's reply");
    };
    assert!(reply.refinements.is_empty());
    assert_eq!(newest(&w)[0].state.name(), "provisional");
}

#[test]
fn a_run_is_served_no_edit_on_a_number_that_it_did_not_load() {
    let mut w = active_world();
    w.start(2, "replay");
    w.load(2);
    direct_compile(&mut w, 30, 2);
    w.host
        .commit(vec![edit_row(
            1,
            1,
            2,
            2,
            "provisional",
            None,
            &[11],
            (DAY, DAY),
            0,
            vec![loc("A")],
        )])
        .expect("the row is stored");
    assert_eq!(w.load_steps(2, 2, 1), compiled());
    assert_eq!(names(&w.replay(3)[1]), vec!["A", "Pay"]);
}

#[test]
fn the_edit_in_force_is_the_last_of_the_chain_of_active_edits_whatever_their_days() {
    let mut w = active_world();
    w.host
        .commit(vec![
            edit_row(
                1,
                1,
                1,
                2,
                "active",
                None,
                &[11, 12, 13],
                (DAY - 9, DAY - 2),
                2,
                vec![loc("A")],
            ),
            edit_row(
                2,
                1,
                1,
                2,
                "active",
                Some(1),
                &[14, 15, 16],
                (DAY - 8, DAY - 5),
                2,
                vec![loc("B"), loc("A")],
            ),
        ])
        .expect("the rows are stored");
    assert_eq!(names(&w.replay(2)[1]), vec!["B", "A", "Pay"]);
}

#[test]
fn a_proposal_on_a_step_after_the_first_page_of_sixteen_is_admitted_and_stored() {
    let mut steps = vec![navigate(1)];
    for n in 2..=18 {
        steps.push(click(
            n,
            &format!("button S{n}"),
            vec![locator("role", "button", Some(&format!("S{n}")))],
        ));
    }
    let mut w = super::host::world();
    w.start(1, "compile");
    w.load(1);
    assert_eq!(w.put(1, 1, steps[..16].to_vec()), Ok(stored(16)));
    assert_eq!(w.put(1, 1, steps[16..].to_vec()), Ok(stored(2)));
    assert!(w.finish(1, "compile", "completed").is_ok());
    let reply = w.coordinator(
        json!({"op": "set_state", "workflow": wf(1), "agent": agent(1), "state": "active"}),
    );
    assert!(reply.is_ok(), "set_state: {reply:?}");
    let mut served = w.replay(2);
    served.extend(w.load_steps(2, 1, 17));
    assert_eq!(served.len(), 18);
    let proposal = with_locators(served[17].clone(), "button S18 now", vec![loc("A")]);
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(1)));
    w.log(2, &[(1, "recovered", 18, None), (2, "step_ok", 18, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    let edits = newest(&w);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].n.get(), 18);
}
