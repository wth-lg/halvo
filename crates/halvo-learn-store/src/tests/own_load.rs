use halvo_agent_store::{Host, Now};
use halvo_schema::executable::body::StepsReply;
use halvo_schema::executable::{CoordinatorReply, RunId, RunnerReply};
use halvo_schema::learning::record::LearnRun;
use halvo_schema::{ErrorName, RowRead, TypedRecord, WorkflowId, WorkflowItem};
use serde_json::{json, Value};

use super::host::{
    active_world, agent, compiled, locator, run, wf, with_locators, TestHost, World,
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
    assert!(holds_nothing_of(&w, 4));
    assert!(
        w.handlers.facts.contains_key(&id(3)),
        "a store fault at the load of run 3 says nothing of run 3"
    );
    w.load(4);
    assert!(holds_nothing_of(&w, 4));
    let served = w.load_steps(4, 1, 1);
    assert_eq!(served, compiled());
    w.loaded = 1;
    learns_nothing_more(&mut w, 4);
}

#[test]
fn a_store_fault_at_the_handlers_own_load_of_a_start_run_that_arrives_again_drops_the_runs_facts_and_start(
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
    assert!(holds_nothing_of(&w, 3));
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
    let RunnerReply::Steps(StepsReply { steps }) = reply else {
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
    let again = w.load_steps(3, 1, 1);
    assert_eq!(again, compiled());
    learns_nothing_more(&mut w, 3);
}
