use halvo_schema::executable::{CoordinatorReply, RunId};
use halvo_schema::learning::names::{EditCause, RunCause};
use halvo_schema::learning::record::{EditState, LearnEdit, LearnRun, RunStatus, Verdict};
use halvo_schema::ErrorName;
use serde_json::{json, Value};

use super::host::{active_world, agent, compiled, locator, run, wf, World};
use crate::handlers::Fault;
use crate::{CoordinatorAnswer, LearnHost, Refused};

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

fn names(step: &Value) -> Vec<String> {
    step["target"]["locators"]
        .as_array()
        .expect("locators")
        .iter()
        .map(|l| l["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn edits(w: &World) -> Vec<LearnEdit> {
    let mut rows = w.host.edits();
    rows.sort_by_key(|e| e.rev);
    rows
}

fn of_run(w: &World, r: u32) -> Vec<LearnRun> {
    let mut rows: Vec<LearnRun> = w
        .host
        .runs()
        .into_iter()
        .filter(|row| row.run.as_str() == run(r))
        .collect();
    rows.sort_by_key(LearnRun::rev);
    rows
}

fn end_run(w: &mut World, r: u32, workflow: u32) -> Result<CoordinatorAnswer, Refused> {
    w.coordinator(
        json!({"op": "end_run", "workflow": wf(workflow), "agent": agent(1), "run": run(r)}),
    )
}

fn accepted(answer: &Result<CoordinatorAnswer, Refused>) -> bool {
    matches!(
        answer,
        Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
    )
}

fn holds_nothing_of(w: &World, r: u32) -> bool {
    let id = RunId::parse(&run(r)).expect("a run id");
    !w.handlers.facts.contains_key(&id) && !w.handlers.starts.contains_key(&id)
}

/// Run 2 learns an edit of step 2; run 3 is served it and logs a counted failure of that step.
fn served_and_failed() -> World {
    let mut w = active_world();
    w.learn(
        2,
        2,
        "button Pay now",
        vec![loc("A"), locator("role", "button", Some("Pay"))],
    );
    let served = w.replay(3);
    assert_eq!(names(&served[1]), ["A", "Pay"]);
    w.log(
        3,
        &[
            (1, "step_ok", 1, None),
            (2, "step_failed", 2, Some("step_failed")),
        ],
    );
    w
}

fn failed_rev_2(row: &LearnRun) -> bool {
    matches!(
        row.status,
        RunStatus::Ended {
            verdict: Verdict::Unverified(RunCause::Failed),
            base: Some(b),
            assisted: 0,
            kept: 0,
            failed_step: Some(s),
        } if b.get() == 1 && s.get() == 2
    )
}

#[test]
fn a_counted_line_then_an_accepted_end_run_stores_the_runs_rev_2_and_reverts_the_edit() {
    let mut w = served_and_failed();
    assert!(accepted(&end_run(&mut w, 3, 1)));
    let rows = of_run(&w, 3);
    assert_eq!(rows.len(), 2);
    assert!(failed_rev_2(&rows[1]), "{:?}", rows[1].status);
    let rows = edits(&w);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].state, EditState::Reverted(EditCause::StepFailed));
    assert!(holds_nothing_of(&w, 3));
    let next = w.replay(4);
    assert_eq!(names(&next[1]), ["Pay"]);
}

#[test]
fn the_number_is_read_before_the_end_run_is_passed_on_to_a_store_that_refuses_a_load_after_the_end()
{
    let mut w = served_and_failed();
    assert!(accepted(&end_run(&mut w, 3, 1)));
    let now = w.host.agent_now();
    let load = json!({"op": "load", "agent": agent(1), "run": run(3)});
    assert!(
        halvo_agent_store::runner(&mut w.host.agent, now, load.to_string().as_bytes()).is_err(),
        "the agent store refuses a load of a run that has ended"
    );
    assert_eq!(of_run(&w, 3).len(), 2);
    assert!(matches!(edits(&w)[1].state, EditState::Reverted(_)));
}

#[test]
fn an_end_run_of_a_run_with_a_start_and_no_load_leaves_rev_1() {
    let mut w = active_world();
    w.start(3, "replay");
    let commits = w.host.commits.get();
    assert!(accepted(&end_run(&mut w, 3, 1)));
    assert_eq!(of_run(&w, 3).len(), 1);
    assert_eq!(w.host.commits.get(), commits);
    assert!(holds_nothing_of(&w, 3));
}

#[test]
fn an_end_run_after_an_answered_finish_stores_nothing() {
    let mut w = served_and_failed();
    assert!(w.finish(3, "replay", "failed").is_ok());
    let (count, commits) = (w.host.count(), w.host.commits.get());
    assert!(accepted(&end_run(&mut w, 3, 1)));
    assert_eq!((w.host.count(), w.host.commits.get()), (count, commits));
}

#[test]
fn an_end_run_that_the_agent_store_refuses_stores_nothing_and_drops_the_facts() {
    let mut w = served_and_failed();
    let (count, commits) = (w.host.count(), w.host.commits.get());
    assert!(matches!(end_run(&mut w, 3, 9), Err(Refused::Error(_))));
    assert_eq!((w.host.count(), w.host.commits.get()), (count, commits));
    assert!(holds_nothing_of(&w, 3));
    assert_eq!(of_run(&w, 3).len(), 1);
    assert_eq!(edits(&w).len(), 1);
}

#[test]
fn a_store_fault_at_the_commit_of_an_end_run_leaves_the_reply_accepted_the_rows_and_no_facts() {
    let mut w = served_and_failed();
    let count = w.host.count();
    w.host.refuse = true;
    assert!(accepted(&end_run(&mut w, 3, 1)));
    assert_eq!(w.handlers.faults, [Fault::Error(ErrorName::Store)]);
    assert_eq!(w.host.count(), count);
    assert!(holds_nothing_of(&w, 3));
}

#[test]
fn a_store_fault_at_the_commit_of_a_finish_leaves_the_reply_the_rows_and_no_facts() {
    let mut w = served_and_failed();
    let count = w.host.count();
    w.host.refuse = true;
    let reply = w.finish(3, "replay", "failed");
    assert!(
        matches!(reply, Ok(halvo_schema::executable::RunnerReply::Stored(_))),
        "{reply:?}"
    );
    assert_eq!(w.handlers.faults, [Fault::Error(ErrorName::Store)]);
    assert_eq!(w.host.count(), count);
    assert_eq!(of_run(&w, 3).len(), 1);
    assert!(holds_nothing_of(&w, 3));
}

#[test]
fn an_end_run_after_a_stop_between_a_compiles_two_commits_withdraws_the_lower_edit_and_reverts_nothing(
) {
    let mut w = served_and_failed();
    w.start(4, "compile");
    w.load(4);
    let now = w.host.agent_now();
    let put = json!({"op": "put_playbook", "agent": agent(1), "run": run(4), "number": 2, "steps": compiled()});
    assert!(
        halvo_agent_store::runner(&mut w.host.agent, now, put.to_string().as_bytes()).is_ok(),
        "the agent store stores number 2, and the handlers' commit does not follow"
    );
    assert!(accepted(&end_run(&mut w, 3, 1)));
    let rows = edits(&w);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].state, EditState::Withdrawn);
    let rows = of_run(&w, 3);
    assert_eq!(rows.len(), 2);
    assert!(failed_rev_2(&rows[1]), "{:?}", rows[1].status);
}

#[test]
fn a_store_fault_at_the_handlers_own_load_before_an_end_run_commits_nothing_and_returns_the_agent_stores_reply(
) {
    let mut w = served_and_failed();
    let (count, commits) = (w.host.count(), w.host.commits.get());
    w.host.fault_first_agent_call = true;
    assert!(accepted(&end_run(&mut w, 3, 1)));
    assert_eq!((w.host.count(), w.host.commits.get()), (count, commits));
    assert!(holds_nothing_of(&w, 3));
    assert_eq!(of_run(&w, 3).len(), 1);
}

/// The limit of L4.3 (ruling LM-81, B3): a stop is no counted cause, so a provisional edit whose step logged `step_ok` is served again after the person stops the run.
#[test]
fn an_end_run_after_step_ok_on_the_step_of_a_provisional_edit_keeps_the_edit_served() {
    let mut w = active_world();
    w.learn(
        2,
        2,
        "button Pay now",
        vec![loc("A"), locator("role", "button", Some("Pay"))],
    );
    let served = w.replay(3);
    assert_eq!(names(&served[1]), ["A", "Pay"]);
    w.log(3, &[(1, "step_ok", 1, None), (2, "step_ok", 2, None)]);
    assert!(accepted(&end_run(&mut w, 3, 1)));
    assert_eq!(
        edits(&w).last().expect("an edit").state,
        EditState::Provisional
    );
    let next = w.replay(4);
    assert_eq!(names(&next[1]), ["A", "Pay"]);
}

/// The limit of L3.7 (ruling LM-81, B3): an active edit whose step logged a counted failure stays active, so it is served again after the person stops the run.
#[test]
fn an_end_run_after_a_counted_failure_of_an_active_edit_keeps_the_edit_served() {
    let mut w = active_world();
    w.learn(
        2,
        2,
        "button Pay now",
        vec![loc("A"), locator("role", "button", Some("Pay"))],
    );
    w.clean_use(3, 2);
    w.clean_use(4, 2);
    assert_eq!(edits(&w).last().expect("an edit").state, EditState::Active);
    let served = w.replay(5);
    assert_eq!(names(&served[1]), ["A", "Pay"]);
    w.log(5, &[(1, "step_failed", 2, Some("step_failed"))]);
    assert!(accepted(&end_run(&mut w, 5, 1)));
    assert_eq!(edits(&w).last().expect("an edit").state, EditState::Active);
    let next = w.replay(6);
    assert_eq!(names(&next[1]), ["A", "Pay"]);
}
