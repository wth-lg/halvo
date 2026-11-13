use halvo_schema::executable::{RunnerCall, RunnerReply};
use halvo_schema::learning::record::{LearnEdit, RunStatus, Verdict};
use serde_json::{json, Value};

use super::host::{active_world, agent, compiled, locator, run, wf, with_locators, World, DAY};
use crate::{LearnHandlers, LearnHost, Refused};

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

fn names(row: &LearnEdit) -> Vec<String> {
    row.target
        .locators
        .iter()
        .map(|l| {
            l.name
                .as_ref()
                .map(|n| n.as_str().to_owned())
                .unwrap_or_default()
        })
        .collect()
}

fn locator_names(step: &Value) -> Vec<String> {
    step["target"]["locators"]
        .as_array()
        .expect("locators")
        .iter()
        .map(|l| l["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn run_row(w: &World, r: u32) -> halvo_schema::learning::record::LearnRun {
    w.host
        .runs()
        .into_iter()
        .filter(|row| row.run.as_str() == run(r))
        .max_by_key(|row| row.rev())
        .expect("a learn_run row")
}

#[test]
fn a_verified_recovered_run_stores_a_provisional_edit_that_two_clean_uses_make_active() {
    let mut w = active_world();
    w.host.day = DAY + 1;
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    let edits = w.host.edits();
    assert_eq!(edits.len(), 1);
    let e = &edits[0];
    assert_eq!(
        (e.rev.get(), e.base.get(), e.n.get(), e.state.name()),
        (1, 1, 2, "provisional")
    );
    assert_eq!(
        (e.first.get(), e.last.get(), e.clean, e.on.clone()),
        (DAY + 1, DAY + 1, 0, None)
    );
    assert_eq!(
        e.runs
            .iter()
            .map(|r| r.as_str().to_owned())
            .collect::<Vec<_>>(),
        vec![run(2)]
    );
    assert_eq!(e.target.desc.as_str(), "button Pay now");
    assert_eq!(names(e), vec!["A"]);
    let r2 = run_row(&w, 2);
    assert_eq!(r2.day.get(), DAY + 1);
    assert_eq!(
        r2.status,
        RunStatus::Ended {
            verdict: Verdict::Verified,
            base: std::num::NonZeroU32::new(1),
            assisted: 1,
            kept: 1,
            failed_step: None,
        }
    );
    w.host.day = DAY + 4;
    let served = w.replay(3);
    assert_eq!(locator_names(&served[1]), vec!["A", "Pay"]);
    w.log(3, &[(1, "step_ok", 2, None)]);
    assert!(w.finish(3, "replay", "completed").is_ok());
    let e = &newest(&w)[0];
    assert_eq!(
        (e.rev.get(), e.state.name(), e.clean, e.last.get()),
        (2, "provisional", 1, DAY + 4)
    );
    w.host.day = DAY + 2;
    w.clean_use(4, 2);
    let e = &newest(&w)[0];
    assert_eq!((e.rev.get(), e.state.name(), e.clean), (3, "active", 2));
    assert_eq!((e.first.get(), e.last.get()), (DAY + 1, DAY + 4));
    assert_eq!(e.runs.len(), 3);
    w.clean_use(5, 2);
    assert_eq!(w.host.edits().len(), 3);
    w.failed_use(6, 2, "step_failed");
    assert_eq!(w.host.edits().len(), 3);
    assert_eq!(locator_names(&w.replay(7)[1]), vec!["A", "Pay"]);
}

#[test]
fn a_candidate_on_a_step_with_an_active_edit_is_built_on_it() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.clean_use(3, 2);
    w.clean_use(4, 2);
    let first = newest(&w)[0].clone();
    assert_eq!(first.state.name(), "active");
    w.learn(5, 2, "button Pay today", vec![loc("B"), loc("A"), pay()]);
    let second = newest(&w)
        .into_iter()
        .find(|e| e.edit != first.edit)
        .expect("a second edit");
    assert_eq!(second.on, Some(first.edit.clone()));
    assert_eq!(names(&second), vec!["B", "A"]);
    assert_eq!(locator_names(&w.replay(6)[1]), vec!["B", "A", "Pay"]);
}

#[test]
fn a_counted_failure_reverts_the_provisional_edit_served_on_its_step_and_another_failure_does_not()
{
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.failed_use(3, 2, "timeout");
    assert_eq!(newest(&w)[0].state.name(), "provisional");
    w.failed_use(4, 3, "step_failed");
    assert_eq!(newest(&w)[0].state.name(), "provisional");
    w.replay(5);
    w.log(
        5,
        &[
            (1, "step_failed", 2, Some("class_mismatch")),
            (2, "step_ok", 2, None),
        ],
    );
    assert!(w.finish(5, "replay", "completed").is_ok());
    let e = &newest(&w)[0];
    assert_eq!(
        (
            e.rev.get(),
            e.state.name(),
            e.state.cause().map(|c| c.as_str())
        ),
        (2, "reverted", Some("class_mismatch"))
    );
    let r5 = run_row(&w, 5);
    assert!(
        matches!(r5.status, RunStatus::Ended { failed_step: Some(n), kept: 0, .. } if n.get() == 2)
    );
    assert_eq!(w.replay(6), compiled());
    let r4 = run_row(&w, 4);
    assert!(matches!(
        r4.status,
        RunStatus::Ended { verdict: Verdict::Unverified(c), failed_step: Some(_), .. } if c.as_str() == "failed"
    ));
}

#[test]
fn a_lost_run_and_a_repeated_finish_change_no_row() {
    let mut w = active_world();
    w.replay(2);
    let step = with_locators(
        compiled()[1].clone(),
        "button Pay now",
        vec![loc("A"), pay()],
    );
    assert_eq!(w.put(2, 1, vec![step]), Ok(stored(1)));
    w.log(2, &[(1, "recovered", 2, None)]);
    w.handlers = LearnHandlers::new();
    let count = w.host.count();
    let commits = w.host.commits.get();
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert_eq!((w.host.count(), w.host.commits.get()), (count, commits));
    assert_eq!(run_row(&w, 2).status, RunStatus::Started);
    w.learn(3, 2, "button Pay now", vec![loc("A"), pay()]);
    let count = w.host.count();
    assert!(w.finish(3, "replay", "completed").is_ok());
    assert_eq!(w.host.count(), count);
}

#[test]
fn refusal_reverted_not_served() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.clean_use(3, 2);
    w.clean_use(4, 2);
    w.learn(5, 2, "button Pay today", vec![loc("B"), loc("A"), pay()]);
    assert_eq!(locator_names(&w.replay(6)[1]), vec!["B", "A", "Pay"]);
    w.log(6, &[(1, "step_failed", 2, Some("step_failed"))]);
    assert!(w.finish(6, "replay", "failed").is_ok());
    let after_revert = w.replay(7);
    assert_eq!(locator_names(&after_revert[1]), vec!["A", "Pay"]);
    assert_eq!(after_revert[1]["target"]["desc"], json!("button Pay now"));
    let again = with_locators(
        after_revert[1].clone(),
        "button Pay today",
        vec![loc("B"), loc("A"), pay()],
    );
    assert_eq!(w.put(7, 1, vec![again]), Ok(stored(0)));

    let now = w.host.agent_now();
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(7), "number": 2, "steps": compiled()});
    assert!(halvo_agent_store::runner(&mut w.host.agent, now, body.to_string().as_bytes()).is_ok());
    let stopped = w.replay(8);
    assert_eq!(stopped, compiled());
    let lower_open = newest(&w)
        .iter()
        .filter(|e| e.state.name() != "withdrawn")
        .count();
    assert_eq!(lower_open, 2);
    w.log(8, &[(1, "step_ok", 2, None)]);
    assert!(w.finish(8, "replay", "completed").is_ok());
    assert!(newest(&w).iter().all(|e| e.state.name() == "withdrawn"));
    assert!(newest(&w).iter().all(|e| e.base.get() == 1));

    w.learn(9, 2, "button Pay now", vec![loc("C"), pay()]);
    assert_eq!(locator_names(&w.replay(10)[1]), vec!["C", "Pay"]);
    w.compile(11, 3, compiled());
    assert_eq!(w.replay(12), compiled());

    w.learn(13, 2, "button Pay now", vec![loc("D"), pay()]);
    w.start(14, "replay");
    let RunnerReply::Load(load) = w.load(14) else {
        panic!("a load reply");
    };
    assert_eq!(load.playbook.map(|p| p.number.get()), Some(3));
    w.compile(15, 4, compiled());
    assert_eq!(w.load_steps(14, 3, 1), compiled());
    assert_eq!(w.load_steps(14, 3, 2), compiled()[1..].to_vec());
    let late = with_locators(
        compiled()[1].clone(),
        "button Pay now",
        vec![loc("E"), pay()],
    );
    assert_eq!(w.put(14, 3, vec![late.clone()]), Ok(stored(0)));
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(14), "number": 3, "steps": [late]});
    let call = RunnerCall::parse(body.to_string().as_bytes());
    let Ok(RunnerCall::PutPlaybook(b)) = call else {
        panic!("a put_playbook body");
    };
    let answers = w
        .handlers
        .propose(&mut w.host, &b)
        .expect("the proposal is read");
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].1, Err(halvo_learn::AdmitCause::Base));
}

#[test]
fn refusal_replay_put_not_stored() {
    let mut w = active_world();
    let playbooks = w.host.agent_rows("agent_playbook");
    w.replay(2);
    w.replay(3);
    let count = w.host.count();
    let proposal = with_locators(
        compiled()[1].clone(),
        "button Pay now",
        vec![loc("X"), pay()],
    );
    assert_eq!(w.put(2, 1, vec![proposal.clone()]), Ok(stored(1)));
    assert_eq!(w.put(3, 1, vec![proposal.clone()]), Ok(stored(1)));
    assert_eq!(w.host.agent_rows("agent_playbook"), playbooks);
    assert_eq!(w.host.count(), count);
    w.log(3, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(3, "replay", "completed").is_ok());
    assert_eq!(newest(&w).len(), 1);
    w.failed_use(4, 2, "origin");
    assert_eq!(newest(&w)[0].state.name(), "reverted");
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    let edits = w.host.edits().len();
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert_eq!(w.host.edits().len(), edits);
    assert!(matches!(
        run_row(&w, 2).status,
        RunStatus::Ended { kept: 0, .. }
    ));

    w.replay(5);
    let other = with_locators(compiled()[2].clone(), "button Next now", vec![loc("Y")]);
    assert_eq!(w.put(5, 1, vec![other]), Ok(stored(1)));
    w.log(
        5,
        &[
            (1, "recovered", 3, None),
            (2, "step_failed", 3, Some("timeout")),
            (3, "step_ok", 3, None),
        ],
    );
    assert!(w.finish(5, "replay", "completed").is_ok());
    assert_eq!(w.host.edits().len(), edits);
    assert_eq!(w.host.agent_rows("agent_playbook"), playbooks);
}

#[test]
fn refusal_revoke_keeps_row() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    let before = w.host.learn_rows("learn_edit");
    let count = w.host.count();
    w.failed_use(3, 2, "step_failed");
    let after = w.host.learn_rows("learn_edit");
    assert!(before.iter().all(|row| after.contains(row)));
    assert_eq!(after.len(), 2);
    let edits = w.host.edits();
    let states: Vec<&str> = edits.iter().map(|e| e.state.name()).collect();
    assert!(states.contains(&"provisional") && states.contains(&"reverted"));
    assert!(edits.iter().all(|e| names(e) == vec!["A"]));
    assert_eq!(w.host.count(), count + 1 + 1 + 1);
}

#[test]
fn refusal_cross_workflow_source() {
    let mut w = active_world();
    w.replay(2);
    let step = with_locators(
        compiled()[1].clone(),
        "button Pay now",
        vec![loc("A"), pay()],
    );
    assert_eq!(w.put(2, 1, vec![step]), Ok(stored(1)));
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    w.host.run_workflow = Some(wf(2));
    let commits = w.host.commits.get();
    let count = w.host.count();
    assert_eq!(w.finish(2, "replay", "completed"), Err(Refused::Source));
    assert_eq!((w.host.commits.get(), w.host.count()), (commits, count));
    assert_eq!(w.host.agent_rows("agent_run").len(), 2);
    assert!(w.host.edits().is_empty());
}

#[test]
fn a_counted_line_that_a_second_body_follows_with_the_same_seq_still_reverts() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(newest(&w)[0].state.name(), "provisional");
    let served = w.replay(3);
    assert_eq!(locator_names(&served[1])[0], "A");
    let logs_before = w.host.agent_rows("agent_log").len();
    w.log(3, &[(1, "step_failed", 2, Some("step_failed"))]);
    w.log(3, &[(1, "step_ok", 2, None)]);
    assert_eq!(w.host.agent_rows("agent_log").len(), logs_before + 2);
    assert!(w.finish(3, "replay", "completed").is_ok());
    let e = &newest(&w)[0];
    assert_eq!(
        (
            e.rev.get(),
            e.state.name(),
            e.state.cause().map(|c| c.as_str()),
            e.clean
        ),
        (2, "reverted", Some("step_failed"), 0)
    );
    assert!(matches!(
        run_row(&w, 3).status,
        RunStatus::Ended { failed_step: Some(n), .. } if n.get() == 2
    ));
}

#[test]
fn log_lines_are_read_in_the_order_of_their_seq_whatever_the_order_of_their_bodies() {
    let mut w = active_world();
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(1)));
    w.log(2, &[(2, "recovered", 2, None), (3, "step_ok", 2, None)]);
    w.log(2, &[(1, "step_failed", 2, Some("timeout"))]);
    w.log(2, &[(1, "step_failed", 2, Some("timeout"))]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert_eq!(newest(&w).len(), 1);
    assert!(matches!(
        run_row(&w, 2).status,
        RunStatus::Ended {
            kept: 1,
            assisted: 1,
            ..
        }
    ));
}
