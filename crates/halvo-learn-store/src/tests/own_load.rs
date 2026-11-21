use halvo_agent_store::{Host, Now};
use halvo_schema::executable::body::StepsReply;
use halvo_schema::executable::names::{Mode, Outcome};
use halvo_schema::executable::record::AgentRun;
use halvo_schema::executable::{CoordinatorReply, RunId, RunnerReply};
use halvo_schema::learning::record::LearnRun;
use halvo_schema::{AgentId, ErrorName, RowRead, TypedRecord, WorkflowId, WorkflowItem};
use serde_json::{json, Value};

use super::host::{
    active_world, agent, click, compiled, locator, navigate, run, wf, with_locators, world,
    TestHost, World,
};
use crate::handlers::Fault;
use crate::{Clock, CoordinatorAnswer, LearnHost, Refused};

struct FaultAt<'a> {
    host: &'a mut TestHost,
    calls: u32,
    at: u32,
}

impl Clock for FaultAt<'_> {
    fn today(&self) -> u32 {
        self.host.today()
    }
}

impl LearnHost for FaultAt<'_> {
    fn agent_host(&mut self) -> &mut dyn Host {
        self.calls += 1;
        self.host.fault_first_agent_call = self.calls == self.at;
        self.host.agent_host()
    }

    fn agent_now(&self) -> Now {
        self.host.agent_now()
    }

    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        LearnHost::commit(self.host, records)
    }

    fn rows(&self) -> &dyn RowRead {
        LearnHost::rows(self.host)
    }

    fn for_each_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        LearnHost::for_each_under(self.host, kind, workflow, visit);
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        LearnHost::workflow(self.host, id)
    }
}

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

fn id(r: u32) -> RunId {
    RunId::parse(&run(r)).expect("a run id")
}

fn holds_nothing_of(w: &World, r: u32) -> bool {
    !w.handlers.facts.contains_key(&id(r)) && !w.handlers.starts.contains_key(&id(r))
}

fn of_run(w: &World, r: u32) -> Vec<LearnRun> {
    w.host
        .runs()
        .into_iter()
        .filter(|row| row.run.as_str() == run(r))
        .collect()
}

fn left_out(w: &World) -> bool {
    w.handlers.faults.last() == Some(&Fault::Error(ErrorName::Store))
}

fn learned_world() -> World {
    let mut w = active_world();
    w.learn(
        2,
        2,
        "button Pay now",
        vec![loc("A"), locator("role", "button", Some("Pay"))],
    );
    w
}

fn learns_nothing_more(w: &mut World, r: u32) {
    let edits = w.host.edits().len();
    let rows = of_run(w, r).len();
    let proposal = with_locators(compiled()[1].clone(), "button Pay later", vec![loc("B")]);
    let put = w.put(r, w.loaded, vec![proposal]);
    assert!(
        !matches!(put, Ok(RunnerReply::Stored(ref s)) if s.stored > 0),
        "the put_playbook took a proposal: {put:?}"
    );
    let finish = w.finish(r, "replay", "completed");
    assert!(finish.is_ok(), "the finish: {finish:?}");
    assert_eq!(w.host.edits().len(), edits);
    assert_eq!(of_run(w, r).len(), rows);
    let next = w.replay(r + 10);
    assert_eq!(names(&next[1]), ["A", "Pay"]);
}

fn coordinator_at(w: &mut World, at: u32, body: Value) -> Result<CoordinatorAnswer, Refused> {
    let mut host = FaultAt {
        host: &mut w.host,
        calls: 0,
        at,
    };
    let answer = w
        .handlers
        .coordinator(&mut host, body.to_string().as_bytes());
    assert!(host.calls >= at, "the request made {} calls", host.calls);
    answer
}

fn start_body(r: u32) -> Value {
    json!({"op": "start_run", "workflow": wf(1), "agent": agent(1), "run": run(r), "mode": "replay"})
}

#[test]
fn a_store_fault_at_the_handlers_own_load_at_an_accepted_start_run_returns_the_agent_stores_reply_and_loses_the_run(
) {
    let mut w = learned_world();
    w.replay(3);
    let commits = w.host.commits.get();
    let answer = coordinator_at(&mut w, 2, start_body(4));
    assert!(
        matches!(
            answer,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
        ),
        "{answer:?}"
    );
    assert!(left_out(&w));
    assert_eq!(w.host.commits.get(), commits);
    assert!(of_run(&w, 4).is_empty());
    assert!(lost_start(&w, 4));
    assert!(
        w.handlers.facts.contains_key(&id(3)),
        "a store fault at the load of run 3 says nothing of run 3"
    );
    w.load(4);
    assert!(lost_start(&w, 4));
    let served = w.load_steps(4, 1, 1);
    assert_eq!(served, compiled());
    w.loaded = 1;
    learns_nothing_more(&mut w, 4);
}

#[test]
fn a_store_fault_at_the_handlers_own_load_of_a_start_run_that_arrives_again_drops_the_runs_facts_and_keeps_its_start_with_no_row(
) {
    let mut w = learned_world();
    w.replay(3);
    assert!(w.handlers.facts.contains_key(&id(3)));
    let commits = w.host.commits.get();
    let answer = coordinator_at(&mut w, 2, start_body(3));
    assert!(
        matches!(
            answer,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
        ),
        "{answer:?}"
    );
    assert!(left_out(&w));
    assert_eq!(w.host.commits.get(), commits);
    assert!(lost_start(&w, 3));
    assert_eq!(of_run(&w, 3).len(), 1);
    let served = w.load_steps(3, 1, 1);
    assert_eq!(served, compiled());
    learns_nothing_more(&mut w, 3);
}

#[test]
fn a_store_fault_at_the_handlers_own_load_at_a_start_run_under_another_agent_drops_nothing_of_the_held_run(
) {
    let mut w = learned_world();
    w.add_pair(2);
    w.replay(42);
    w.host.agent.hidden_runs.push(run(42));
    let body = json!({"op": "start_run", "workflow": wf(2), "agent": agent(2), "run": run(42), "mode": "replay"});
    let answer = coordinator_at(&mut w, 2, body);
    assert!(
        matches!(
            answer,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
        ),
        "{answer:?}"
    );
    assert!(left_out(&w));
    assert_eq!(w.handlers.facts[&id(42)].agent.as_str(), agent(1));
    assert_eq!(w.handlers.starts[&id(42)].agent.as_str(), agent(1));
}

fn runner_at(w: &mut World, at: u32, body: Value) -> Result<RunnerReply, Refused> {
    let mut host = FaultAt {
        host: &mut w.host,
        calls: 0,
        at,
    };
    let answer = w.handlers.runner(&mut host, body.to_string().as_bytes());
    assert!(host.calls >= at, "the request made {} calls", host.calls);
    answer
}

fn steps_of(reply: RunnerReply) -> Vec<Value> {
    let RunnerReply::Steps(StepsReply { steps, .. }) = reply else {
        panic!("a steps reply: {reply:?}");
    };
    steps
        .iter()
        .map(|s| serde_json::to_value(s).expect("a step's JSON"))
        .collect()
}

#[test]
fn a_store_fault_at_the_handlers_own_load_at_an_answered_load_steps_returns_the_compiled_steps_and_loses_the_run(
) {
    let mut w = learned_world();
    w.add_pair(2);
    w.pair = 2;
    w.replay(6);
    w.pair = 1;
    w.replay(5);
    w.start(3, "replay");
    w.load(3);
    w.loaded = 1;
    assert!(w.handlers.facts.contains_key(&id(3)));
    let commits = w.host.commits.get();
    let body =
        json!({"op": "load_steps", "agent": agent(1), "run": run(3), "number": 1, "from": 1});
    let answer = runner_at(&mut w, 2, body);
    let served = steps_of(answer.expect("the agent store's steps"));
    assert_eq!(served, compiled());
    assert!(left_out(&w));
    assert_eq!(w.host.commits.get(), commits);
    assert!(holds_nothing_of(&w, 3));
    assert!(
        w.handlers.facts.contains_key(&id(5)) && w.handlers.starts.contains_key(&id(5)),
        "another run of the same agent keeps what the handlers hold of it"
    );
    assert!(
        w.handlers.facts.contains_key(&id(6)) && w.handlers.starts.contains_key(&id(6)),
        "a run of another agent keeps what the handlers hold of it"
    );
    let again = w.load_steps(3, 1, 1);
    assert_eq!(again, compiled());
    learns_nothing_more(&mut w, 3);
}

fn finish_body(w: &World, r: u32) -> Value {
    let mut made = AgentRun::new(
        WorkflowId::parse(&wf(1)).expect("a workflow id"),
        AgentId::parse(&agent(1)).expect("an agent id"),
        id(r),
        w.host.day,
        Some(Mode::Replay),
        Outcome::Completed,
    );
    made.steps = 3;
    made.wall_s = 9;
    let text = made.to_json().expect("a made record");
    let record: Value = serde_json::from_str(&text).expect("the made record's text");
    json!({"op": "finish", "agent": agent(1), "run": run(r), "record": record})
}

fn agent_run_of(w: &World, r: u32) -> bool {
    let named = format!("\"run\":\"{}\"", run(r));
    w.host
        .agent_rows("agent_run")
        .iter()
        .any(|row| row.contains(named.as_str()))
}

#[test]
fn a_store_fault_at_the_handlers_own_load_before_a_finish_passes_it_on_and_commits_nothing() {
    let mut w = learned_world();
    let served = w.replay(3);
    assert_eq!(names(&served[1]), ["A", "Pay"]);
    w.log(3, &[(2, "step_ok", 2, None)]);
    let (count, commits) = (w.host.count(), w.host.commits.get());
    let body = finish_body(&w, 3);
    let answer = runner_at(&mut w, 1, body);
    assert!(matches!(answer, Ok(RunnerReply::Stored(_))), "{answer:?}");
    assert!(left_out(&w));
    assert!(
        agent_run_of(&w, 3),
        "the agent store keeps the run's record"
    );
    assert_eq!((w.host.count(), w.host.commits.get()), (count, commits));
    assert!(holds_nothing_of(&w, 3));
    assert_eq!(of_run(&w, 3).len(), 1);
    let edit = w.host.edits().into_iter().max_by_key(|e| e.rev);
    assert_eq!(edit.map(|e| e.clean), Some(0));
}

#[test]
fn a_store_fault_at_the_handlers_read_of_the_compiled_steps_before_a_finish_passes_it_on_and_commits_nothing(
) {
    let mut w = learned_world();
    let served = w.replay(3);
    let next = locator("role", "button", Some("Next"));
    let proposal = with_locators(served[2].clone(), "button Next page", vec![loc("N"), next]);
    let put = w.put(3, 1, vec![proposal]);
    assert!(put.is_ok(), "the put_playbook: {put:?}");
    assert!(!w.handlers.facts[&id(3)].candidates.is_empty());
    w.log(3, &[(2, "recovered", 3, None), (3, "step_ok", 3, None)]);
    let (count, commits) = (w.host.count(), w.host.commits.get());
    let body = finish_body(&w, 3);
    let answer = runner_at(&mut w, 2, body);
    assert!(matches!(answer, Ok(RunnerReply::Stored(_))), "{answer:?}");
    assert!(left_out(&w));
    assert!(
        agent_run_of(&w, 3),
        "the agent store keeps the run's record"
    );
    assert_eq!((w.host.count(), w.host.commits.get()), (count, commits));
    assert!(holds_nothing_of(&w, 3));
    assert_eq!(of_run(&w, 3).len(), 1);
}

#[test]
fn a_finish_of_a_run_with_no_candidate_makes_one_own_load_and_no_read_of_the_compiled_steps() {
    let mut w = learned_world();
    w.replay(3);
    w.log(3, &[(2, "step_ok", 2, None)]);
    assert!(w.handlers.facts[&id(3)].candidates.is_empty());
    let body = finish_body(&w, 3);
    let mut host = FaultAt {
        host: &mut w.host,
        calls: 0,
        at: 0,
    };
    let answer = w.handlers.runner(&mut host, body.to_string().as_bytes());
    assert_eq!(
        host.calls, 2,
        "the own load and the finish that it passes on"
    );
    assert!(matches!(answer, Ok(RunnerReply::Stored(_))), "{answer:?}");
}

#[test]
fn a_store_fault_at_the_first_load_of_the_sweep_still_drops_a_later_held_run_whose_load_is_refused()
{
    let mut w = learned_world();
    w.replay(3);
    w.replay(5);
    assert!(w.handlers.facts.contains_key(&id(3)));
    assert!(w.handlers.facts.contains_key(&id(5)));
    w.host.agent.hidden_runs.push(run(5));
    let answer = coordinator_at(&mut w, 2, start_body(6));
    assert!(
        matches!(
            answer,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
        ),
        "{answer:?}"
    );
    assert!(left_out(&w));
    assert!(
        w.handlers.facts.contains_key(&id(3)),
        "a store fault at the load of run 3 says nothing of run 3"
    );
    assert!(
        !w.handlers.facts.contains_key(&id(5)),
        "the agent store refuses the load of run 5, so its slot values go at the latest at this start (L4.3)"
    );
}

#[test]
fn a_store_fault_at_the_handlers_own_load_of_a_start_run_that_arrives_again_keeps_a_start_held_with_no_facts_with_no_row(
) {
    let mut w = learned_world();
    w.replay(3);
    w.start(4, "replay");
    assert!(w.handlers.starts.contains_key(&id(4)));
    assert!(!w.handlers.facts.contains_key(&id(4)));
    let answer = coordinator_at(&mut w, 2, start_body(4));
    assert!(
        matches!(
            answer,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
        ),
        "{answer:?}"
    );
    assert!(left_out(&w));
    assert!(lost_start(&w, 4));
    w.load(4);
    assert!(lost_start(&w, 4));
}

#[test]
fn a_store_fault_at_the_handlers_own_load_before_a_finish_that_the_agent_store_refuses_still_drops_the_run(
) {
    let mut w = learned_world();
    w.replay(3);
    w.log(3, &[(2, "step_failed", 2, Some("step_failed"))]);
    let mut body = finish_body(&w, 3);
    body["record"]["workflow"] = json!(wf(2));
    let commits = w.host.commits.get();
    let answer = runner_at(&mut w, 1, body);
    assert!(
        matches!(answer, Err(Refused::Error(name)) if name != ErrorName::Store && name != ErrorName::Internal),
        "{answer:?}"
    );
    assert!(left_out(&w));
    assert!(holds_nothing_of(&w, 3));
    assert_eq!(w.host.commits.get(), commits);
    let end = w
        .coordinator(json!({"op": "end_run", "workflow": wf(1), "agent": agent(1), "run": run(3)}));
    assert!(
        matches!(
            end,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
        ),
        "{end:?}"
    );
    assert_eq!(
        of_run(&w, 3).len(),
        1,
        "the lost run makes no commit at its end"
    );
}

#[test]
fn a_store_fault_at_the_second_page_of_the_compiled_steps_before_a_finish_passes_it_on_and_commits_nothing(
) {
    let mut steps = vec![navigate(1)];
    for n in 2..=17 {
        let name = format!("S{n}");
        steps.push(click(
            n,
            &format!("button {name}"),
            vec![locator("role", "button", Some(&name))],
        ));
    }
    let mut w = world();
    w.start(1, "compile");
    w.load(1);
    for page in steps.chunks(16) {
        let put = w.put(1, 1, page.to_vec());
        assert!(put.is_ok(), "a page of the compile: {put:?}");
    }
    let finish = w.finish(1, "compile", "completed");
    assert!(finish.is_ok(), "the compile's finish: {finish:?}");
    let state = w.coordinator(
        json!({"op": "set_state", "workflow": wf(1), "agent": agent(1), "state": "active"}),
    );
    assert!(state.is_ok(), "{state:?}");
    let served = w.replay(2);
    assert_eq!(served.len(), 16);
    let s3 = locator("role", "button", Some("S3"));
    let proposal = with_locators(served[2].clone(), "button S3 now", vec![loc("N"), s3]);
    let put = w.put(2, 1, vec![proposal]);
    assert!(put.is_ok(), "the put_playbook: {put:?}");
    assert!(!w.handlers.facts[&id(2)].candidates.is_empty());
    w.log(2, &[(2, "recovered", 3, None), (3, "step_ok", 3, None)]);
    let (count, commits) = (w.host.count(), w.host.commits.get());
    let body = finish_body(&w, 2);
    let answer = runner_at(&mut w, 3, body);
    assert!(matches!(answer, Ok(RunnerReply::Stored(_))), "{answer:?}");
    assert!(left_out(&w));
    assert!(
        agent_run_of(&w, 2),
        "the agent store keeps the run's record"
    );
    assert_eq!((w.host.count(), w.host.commits.get()), (count, commits));
    assert!(holds_nothing_of(&w, 2));
    assert_eq!(of_run(&w, 2).len(), 1);
}

fn lost_start(w: &World, r: u32) -> bool {
    !w.handlers.facts.contains_key(&id(r))
        && w.handlers
            .starts
            .get(&id(r))
            .is_some_and(|s| !s.row && s.agent.as_str() == agent(1))
}

fn compile_body(r: u32) -> Value {
    json!({"op": "start_run", "workflow": wf(1), "agent": agent(1), "run": run(r), "mode": "compile"})
}

fn playbook_rows(w: &World, number: u32) -> usize {
    let named = format!("\"agent\":\"{}\"", agent(1));
    let numbered = format!("\"number\":{number},");
    w.host
        .agent_rows("agent_playbook")
        .iter()
        .filter(|row| row.contains(named.as_str()) && row.contains(numbered.as_str()))
        .count()
}

fn accepted(answer: &Result<CoordinatorAnswer, Refused>) -> bool {
    matches!(
        answer,
        Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
    )
}

fn compiles_after_the_fault(w: &mut World, r: u32, number: u32) {
    let rows = playbook_rows(w, number);
    w.load(r);
    let put = w.put(r, number, compiled());
    assert!(
        matches!(put, Ok(RunnerReply::Stored(ref s)) if s.stored == 3),
        "the compile's put_playbook reaches the agent store: {put:?}"
    );
    assert_eq!(playbook_rows(w, number), rows + 3);
    assert!(lost_start(w, r));
    let commits = w.host.commits.get();
    let finish = w.finish(r, "compile", "completed");
    assert!(finish.is_ok(), "{finish:?}");
    assert_eq!(
        w.host.commits.get(),
        commits,
        "the lost run makes no learning commit"
    );
    assert!(of_run(w, r).is_empty(), "the lost run has no learn_run row");
    let state = w.coordinator(
        json!({"op": "set_state", "workflow": wf(1), "agent": agent(1), "state": "active"}),
    );
    assert!(
        matches!(
            state,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::State(_)))
        ),
        "{state:?}"
    );
}

#[test]
fn a_store_fault_in_the_sweep_at_a_first_compiles_start_run_lets_its_put_playbook_reach_the_agent_store(
) {
    let mut w = world();
    w.start(1, "compile");
    w.load(1);
    assert!(w.handlers.facts.contains_key(&id(1)));
    let answer = coordinator_at(&mut w, 2, compile_body(2));
    assert!(accepted(&answer), "{answer:?}");
    assert!(left_out(&w));
    assert_eq!(playbook_rows(&w, 1), 0);
    compiles_after_the_fault(&mut w, 2, 1);
}

#[test]
fn a_store_fault_in_the_sweep_at_a_first_compiles_start_run_at_another_agents_run_lets_its_put_playbook_reach_the_agent_store(
) {
    let mut w = world();
    w.add_pair(2);
    w.pair = 2;
    w.replay(5);
    w.pair = 1;
    assert!(w.handlers.facts.contains_key(&id(5)));
    let answer = coordinator_at(&mut w, 2, compile_body(1));
    assert!(accepted(&answer), "{answer:?}");
    assert!(left_out(&w));
    assert_eq!(playbook_rows(&w, 1), 0);
    compiles_after_the_fault(&mut w, 1, 1);
    assert!(w.handlers.facts.contains_key(&id(5)));
}

#[test]
fn a_store_fault_in_the_sweep_at_a_recompiles_start_run_moves_the_agent_to_the_new_number() {
    for other in [false, true] {
        let mut w = active_world();
        if other {
            w.add_pair(2);
            w.pair = 2;
            w.replay(3);
            w.pair = 1;
        } else {
            w.replay(3);
        }
        let answer = coordinator_at(&mut w, 2, compile_body(4));
        assert!(accepted(&answer), "other {other}: {answer:?}");
        assert!(left_out(&w));
        compiles_after_the_fault(&mut w, 4, 2);
        w.replay(5);
        assert_eq!(
            w.loaded, 2,
            "other {other}: the next run loads the new number"
        );
    }
}

#[test]
fn a_faulted_repeat_of_a_start_whose_commit_was_refused_keeps_the_run_lost_through_a_clean_repeat_and_its_finish(
) {
    let mut w = learned_world();
    w.replay(3);
    w.host.refuse = true;
    let first = w.coordinator(start_body(4));
    w.host.refuse = false;
    assert!(accepted(&first), "{first:?}");
    assert!(lost_start(&w, 4));
    let commits = w.host.commits.get();
    let faulted = coordinator_at(&mut w, 2, start_body(4));
    assert!(accepted(&faulted), "{faulted:?}");
    assert!(left_out(&w));
    assert!(
        lost_start(&w, 4),
        "the faulted repeat keeps the start with no row"
    );
    let again = w.coordinator(start_body(4));
    assert!(accepted(&again), "{again:?}");
    assert!(lost_start(&w, 4));
    w.load(4);
    let served = w.load_steps(4, 1, 1);
    assert_eq!(names(&served[1]), ["Pay"]);
    assert_eq!(served, compiled());
    w.log(4, &[(2, "step_ok", 2, None)]);
    let finish = w.finish(4, "replay", "completed");
    assert!(finish.is_ok(), "{finish:?}");
    assert!(
        of_run(&w, 4).is_empty(),
        "the lost run has no learn_run row"
    );
    assert_eq!(
        w.host.commits.get(),
        commits,
        "the lost run makes no commit"
    );
}

fn held_under(w: &World, r: u32, n: u32, row: bool) -> bool {
    w.handlers
        .starts
        .get(&id(r))
        .is_some_and(|s| s.row == row && s.agent.as_str() == agent(n))
}

fn facts_under(w: &World, r: u32, n: u32) -> bool {
    w.handlers
        .facts
        .get(&id(r))
        .is_some_and(|f| f.agent.as_str() == agent(n))
}

fn two_agents_world() -> World {
    let mut w = learned_world();
    w.add_pair(2);
    w.replay(3);
    w.pair = 2;
    w.replay(6);
    w.pair = 1;
    assert!(facts_under(&w, 3, 1) && facts_under(&w, 6, 2));
    w
}

fn start_of_agent_2(r: u32) -> Value {
    json!({"op": "start_run", "workflow": wf(2), "agent": agent(2), "run": run(r), "mode": "replay"})
}

#[test]
fn a_store_fault_at_the_handlers_own_load_before_a_finish_drops_nothing_of_another_agents_run() {
    let mut w = two_agents_world();
    let body = finish_body(&w, 3);
    let answer = runner_at(&mut w, 1, body);
    assert!(matches!(answer, Ok(RunnerReply::Stored(_))), "{answer:?}");
    assert!(left_out(&w));
    assert!(holds_nothing_of(&w, 3));
    assert!(facts_under(&w, 6, 2) && held_under(&w, 6, 2, true));
}

#[test]
fn a_store_fault_in_the_sweep_of_a_repeated_start_run_keeps_its_run_with_no_row_whichever_agents_load_faults(
) {
    for at in 2..=3 {
        let mut w = two_agents_world();
        let answer = coordinator_at(&mut w, at, start_of_agent_2(6));
        assert!(accepted(&answer), "at {at}: {answer:?}");
        assert!(left_out(&w), "at {at}");
        assert!(!w.handlers.facts.contains_key(&id(6)), "at {at}");
        assert!(held_under(&w, 6, 2, false), "at {at}");
        assert!(
            facts_under(&w, 3, 1) && held_under(&w, 3, 1, true),
            "at {at}"
        );
    }
}

#[test]
fn a_store_fault_at_another_agents_load_in_the_sweep_leaves_out_the_learning_of_a_new_start_run() {
    let mut w = two_agents_world();
    let answer = coordinator_at(&mut w, 2, start_of_agent_2(7));
    assert!(accepted(&answer), "{answer:?}");
    assert!(left_out(&w));
    assert!(
        of_run(&w, 7).is_empty(),
        "the lost run has no learn_run row"
    );
    assert!(!w.handlers.facts.contains_key(&id(7)));
    assert!(held_under(&w, 7, 2, false));
    assert!(facts_under(&w, 3, 1) && facts_under(&w, 6, 2));
}

#[test]
fn a_store_fault_in_the_sweep_still_sends_the_load_of_another_agents_run_and_drops_it_when_refused()
{
    let mut w = two_agents_world();
    w.host.agent.hidden_runs.push(run(6));
    let answer = coordinator_at(&mut w, 2, start_body(7));
    assert!(accepted(&answer), "{answer:?}");
    assert!(left_out(&w));
    assert!(
        facts_under(&w, 3, 1),
        "a fault at run 3's load says nothing of run 3"
    );
    assert!(
        holds_nothing_of(&w, 6),
        "agent 2's run 6, refused by the agent store, is dropped"
    );
    assert!(lost_start(&w, 7));
}

#[test]
fn a_store_fault_at_a_start_run_under_another_agent_leaves_a_start_held_with_no_facts_as_it_was() {
    let mut w = learned_world();
    w.add_pair(2);
    w.replay(3);
    w.start(42, "replay");
    assert!(!w.handlers.facts.contains_key(&id(42)) && held_under(&w, 42, 1, true));
    w.host.agent.hidden_runs.push(run(42));
    let answer = coordinator_at(&mut w, 2, start_of_agent_2(42));
    assert!(accepted(&answer), "{answer:?}");
    assert!(left_out(&w));
    assert!(
        held_under(&w, 42, 1, true),
        "agent 1's start of run 42 stays as it was"
    );
    assert!(!w.handlers.facts.contains_key(&id(42)));
}
