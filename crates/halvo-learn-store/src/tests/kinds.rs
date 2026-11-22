use halvo_schema::executable::{ActionClass, CoordinatorReply, RunId, RunnerReply};
use halvo_schema::learning::record::{
    EditState, LearnEdit, LearnRoutine, LearnRun, RunStatus, Verdict,
};
use halvo_schema::{ErrorName, Summary, SummaryRead, WriterStep};
use serde_json::{json, Value};

use super::host::{
    active_world, active_world_with, agent, click, compiled, id, locator, run, wf, with_locators,
    World, DAY,
};
use crate::handlers::Fault;
use crate::{Clock, CoordinatorAnswer, LearnHandlers, LearnHost, LearnStep, Refused};

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

fn pay() -> Value {
    locator("role", "button", Some("Pay"))
}

fn next() -> Value {
    locator("role", "button", Some("Next"))
}

fn stored(n: u32) -> RunnerReply {
    RunnerReply::parse(format!(r#"{{"stored":{n}}}"#).as_bytes()).expect("a stored reply")
}

fn names(step: &Value) -> Vec<String> {
    step["target"]["locators"]
        .as_array()
        .expect("locators")
        .iter()
        .map(|l| l["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn rows_of(w: &World, kind: &str, n: u32) -> Vec<String> {
    w.host
        .learn_rows(kind)
        .into_iter()
        .filter(|row| {
            serde_json::from_str::<Value>(row).is_ok_and(|v| v["workflow"] == json!(wf(n)))
        })
        .collect()
}

fn edits(w: &World) -> Vec<LearnEdit> {
    w.host
        .learn_rows("learn_edit")
        .iter()
        .filter_map(|row| LearnEdit::parse(row.as_bytes()).ok())
        .collect()
}

fn of_run(w: &World, r: u32) -> Vec<LearnRun> {
    let mut rows: Vec<LearnRun> = w
        .host
        .learn_rows("learn_run")
        .iter()
        .filter_map(|row| LearnRun::parse(row.as_bytes()).ok())
        .filter(|row| row.run.as_str() == run(r))
        .collect();
    rows.sort_by_key(LearnRun::rev);
    rows
}

fn list(w: &mut World, n: u32) -> Result<CoordinatorAnswer, Refused> {
    w.coordinator(json!({"op": "list_refinements", "workflow": wf(n), "agent": agent(n)}))
}

fn refused_edit(n: u32) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":1,"base":1,"n":2,"state":"provisional","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"],"first":20767,"last":20767,"clean":3,"target":{{"desc":"button Pay","locators":[{{"by":"role","value":"button","name":"Pay"}}]}}}}"#,
        wf(n),
        agent(n)
    )
}

fn refused_run(n: u32) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":3,"day":20771,"mode":"replay","state":"started","cause":null,"base":null,"assisted":0,"kept":0,"failed_step":null}}"#,
        wf(n),
        agent(n)
    )
}

fn refused_routine(n: u32) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","rev":1,"day":20771,"category":"utility_bill_payment","site":null,"app":"com.example.browser","period":null,"seen":0,"first":null,"last":null,"next":null,"missed":0,"mark":"gone","since":null}}"#,
        wf(n),
        agent(n)
    )
}

struct NoSummaries;

impl SummaryRead for NoSummaries {
    fn for_each(&self, _visit: &mut dyn FnMut(&Summary)) {}
}

struct Today(u32);

impl Clock for Today {
    fn today(&self) -> u32 {
        self.0
    }
}

fn stepped(w: &World, n: u32) -> Vec<LearnRoutine> {
    LearnStep::new(Today(w.host.day))
        .after_commit(&NoSummaries, &w.host)
        .into_iter()
        .map(|r| LearnRoutine::parse(r.json.as_bytes()).expect("a routine row"))
        .filter(|r| r.workflow.as_str() == wf(n))
        .collect()
}

#[test]
fn each_planted_row_is_refused_by_its_parser_and_names_its_workflow() {
    assert!(LearnEdit::parse(refused_edit(1).as_bytes()).is_err());
    assert!(LearnRun::parse(refused_run(1).as_bytes()).is_err());
    assert!(LearnRoutine::parse(refused_routine(1).as_bytes()).is_err());
    for (kind, text) in [
        ("learn_edit", refused_edit(1)),
        ("learn_run", refused_run(1)),
        ("learn_routine", refused_routine(1)),
    ] {
        let named = crate::rows::refused_workflow(kind, &text).expect("a workflow");
        assert_eq!(named.as_str(), wf(1));
    }
}

#[test]
fn a_refused_learn_edit_row_stops_the_edit_side_of_its_workflow_and_nothing_else() {
    let mut w = active_world();
    w.add_pair(2);
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.pair = 2;
    w.learn(3, 2, "button Pay now", vec![loc("B"), pay()]);
    w.pair = 1;
    w.host.put_raw("learn_edit", &refused_edit(1));
    let edits_of_a = rows_of(&w, "learn_edit", 1);
    w.host.day = DAY + 1;

    let served = w.replay(10);
    assert_eq!(names(&served[1]), ["Pay"]);
    let proposal = with_locators(
        served[2].clone(),
        "button Next step",
        vec![loc("C"), next()],
    );
    assert_eq!(w.put(10, w.loaded, vec![proposal]), Ok(stored(0)));
    w.log(
        10,
        &[
            (2, "step_ok", 2, None),
            (3, "recovered", 3, None),
            (4, "step_ok", 3, None),
        ],
    );
    assert_eq!(w.finish(10, "replay", "completed"), Ok(stored(1)));
    let rows = of_run(&w, 10);
    assert_eq!(rows.len(), 2);
    assert!(matches!(
        rows[1].status,
        RunStatus::Ended {
            verdict: Verdict::Verified,
            kept: 0,
            ..
        }
    ));
    let routine = stepped(&w, 1);
    assert_eq!(routine.len(), 1);
    assert_eq!(routine[0].last.map(|d| d.get()), Some(DAY + 1));

    w.start(11, "compile");
    w.load(11);
    assert!(matches!(
        w.put(11, 2, compiled()),
        Ok(RunnerReply::Stored(_))
    ));
    assert!(w.finish_of(11, "compile", "completed", Some(2)).is_ok());
    assert_eq!(list(&mut w, 1), Err(Refused::Error(ErrorName::Store)));
    assert_eq!(rows_of(&w, "learn_edit", 1), edits_of_a);
    assert!(w.handlers.faults.is_empty(), "a stopped read is no fault");

    w.pair = 2;
    let served = w.replay(12);
    assert_eq!(names(&served[1]), ["B", "Pay"]);
    w.log(12, &[(2, "step_ok", 2, None)]);
    assert!(w.finish(12, "replay", "completed").is_ok());
    let b_edit: Vec<LearnEdit> = edits(&w)
        .into_iter()
        .filter(|e| e.workflow.as_str() == wf(2))
        .collect();
    assert_eq!(b_edit.len(), 2);
    assert!(b_edit.iter().any(|e| e.clean == 1));
    let Ok(CoordinatorAnswer::Refinements(read)) = list(&mut w, 2) else {
        panic!("the read of workflow B is answered");
    };
    assert_eq!(read.refinements.len(), 1);
}

#[test]
fn a_refused_learn_run_row_makes_the_runs_of_its_workflow_lost_runs_and_stops_nothing_else() {
    let mut w = active_world();
    w.add_pair(2);
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.start(30, "replay");
    w.host.put_raw("learn_run", &refused_run(1));
    let before: Vec<Vec<String>> = ["learn_edit", "learn_run", "learn_routine"]
        .iter()
        .map(|kind| rows_of(&w, kind, 1))
        .collect();

    let served = w.replay(10);
    assert!(of_run(&w, 10).is_empty());
    let starts = w.handlers.starts.clone();
    w.start(10, "replay");
    assert_eq!(w.handlers.starts, starts);
    assert_eq!(names(&served[1]), ["Pay"]);
    let proposal = with_locators(
        served[2].clone(),
        "button Next step",
        vec![loc("C"), next()],
    );
    assert_eq!(w.put(10, w.loaded, vec![proposal]), Ok(stored(0)));
    w.log(
        10,
        &[
            (2, "step_ok", 2, None),
            (3, "recovered", 3, None),
            (4, "step_ok", 3, None),
        ],
    );
    assert_eq!(w.finish(10, "replay", "completed"), Ok(stored(1)));

    w.start(11, "compile");
    w.load(11);
    assert!(matches!(
        w.put(11, 2, compiled()),
        Ok(RunnerReply::Stored(_))
    ));
    assert!(w.finish_of(11, "compile", "completed", Some(2)).is_ok());
    let withdrawn = edits(&w)
        .into_iter()
        .filter(|e| e.workflow.as_str() == wf(1) && e.state == EditState::Withdrawn)
        .count();
    assert_eq!(withdrawn, 1);
    assert_eq!(rows_of(&w, "learn_run", 1), before[1]);
    assert_eq!(rows_of(&w, "learn_routine", 1), before[2]);
    assert!(w.handlers.faults.is_empty(), "a stopped read is no fault");
    assert!(matches!(
        list(&mut w, 1),
        Ok(CoordinatorAnswer::Refinements(_))
    ));

    w.start(13, "replay");
    w.handlers = LearnHandlers::new();
    let binding = ErrorName::read("binding").expect("a declared name");
    assert_eq!(
        w.put(13, 2, vec![compiled()[1].clone()]),
        Err(Refused::Error(binding))
    );
    assert_eq!(w.put(30, 2, vec![compiled()[1].clone()]), Ok(stored(0)));

    w.pair = 2;
    w.learn(20, 2, "button Pay now", vec![loc("B"), pay()]);
    assert_eq!(of_run(&w, 20).len(), 2);
    assert!(edits(&w).iter().any(|e| e.workflow.as_str() == wf(2)));
}

#[test]
fn a_refused_learn_run_row_that_comes_after_a_runs_start_makes_it_a_lost_run() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.start(10, "replay");
    w.start(11, "replay");
    w.load(11);
    w.start(12, "replay");
    w.load(12);
    w.host.put_raw("learn_run", &refused_run(1));
    let runs = rows_of(&w, "learn_run", 1);
    w.load(10);
    let id = RunId::parse(&run(10)).expect("a run id");
    assert!(!w.handlers.facts.contains_key(&id));
    let served = w.load_steps(11, 1, 1);
    assert_eq!(names(&served[1]), ["Pay"]);
    let id = RunId::parse(&run(11)).expect("a run id");
    assert!(!w.handlers.facts.contains_key(&id));
    w.log(11, &[(2, "step_ok", 2, None)]);
    assert_eq!(w.finish(11, "replay", "completed"), Ok(stored(1)));
    w.log(12, &[(2, "step_ok", 2, None)]);
    assert_eq!(w.finish(12, "replay", "completed"), Ok(stored(1)));
    assert_eq!(rows_of(&w, "learn_run", 1), runs);
    assert!(w.handlers.faults.is_empty(), "a stopped read is no fault");
}

#[test]
fn a_run_started_while_its_run_rows_were_stopped_stays_lost_after_the_refused_row_leaves_by_its_bound(
) {
    let mut w = active_world();
    w.host.put_raw("learn_run", &refused_run(1));
    w.host.day = DAY + 92;
    w.start(10, "replay");
    assert!(of_run(&w, 10).is_empty());
    w.host.day = DAY + 93;
    w.load(10);
    let id = RunId::parse(&run(10)).expect("a run id");
    assert!(!w.handlers.facts.contains_key(&id));
    assert_eq!(
        w.put(
            10,
            1,
            vec![with_locators(
                compiled()[1].clone(),
                "button Pay now",
                vec![loc("C")]
            )]
        ),
        Ok(stored(0))
    );
}

#[test]
fn a_refused_learn_routine_row_stops_the_routine_rows_of_its_workflow_and_no_run() {
    let mut w = active_world();
    w.add_pair(2);
    w.host.put_raw("learn_routine", &refused_routine(1));
    let routines_of_a = rows_of(&w, "learn_routine", 1);
    w.host.put_raw("learn_routine", &refused_routine(3));
    w.create(3);
    assert_eq!(rows_of(&w, "learn_routine", 3), [refused_routine(3)]);

    assert_eq!(list(&mut w, 1), Err(Refused::Error(ErrorName::Store)));
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    assert_eq!(of_run(&w, 2).len(), 2);
    assert_eq!(edits(&w).len(), 1);
    assert_eq!(rows_of(&w, "learn_routine", 1), routines_of_a);
    assert!(matches!(
        list(&mut w, 2),
        Ok(CoordinatorAnswer::Refinements(_))
    ));
}

#[test]
fn a_refused_row_whose_text_names_no_workflow_stops_only_the_workflow_under_whose_key_it_stands() {
    let mut w = active_world();
    w.add_pair(2);
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.pair = 2;
    w.learn(3, 2, "button Pay now", vec![loc("B"), pay()]);
    w.host
        .put_raw_under("learn_edit", Some(&wf(1)), r#"{"v":1,"workflow":7}"#);
    w.pair = 1;
    let served = w.replay(11);
    assert_eq!(names(&served[1]), ["Pay"]);
    assert_eq!(list(&mut w, 1), Err(Refused::Error(ErrorName::Store)));
    w.pair = 2;
    let served = w.replay(12);
    assert_eq!(names(&served[1]), ["B", "Pay"]);
    assert!(matches!(
        list(&mut w, 2),
        Ok(CoordinatorAnswer::Refinements(_))
    ));
}

#[test]
fn a_learning_commit_that_the_store_refuses_leaves_the_agent_stores_reply() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.replay(4);
    w.log(4, &[(2, "step_ok", 2, None)]);
    w.host.refuse = true;

    assert_eq!(w.finish(4, "replay", "completed"), Ok(stored(1)));
    assert_eq!(of_run(&w, 4).len(), 1);
    assert_eq!(w.handlers.faults, [Fault::Error(ErrorName::Store)]);

    w.start(5, "replay");
    w.load(5);
    let id = RunId::parse(&run(5)).expect("a run id");
    assert!(!w.handlers.facts.contains_key(&id));
    assert!(of_run(&w, 5).is_empty());
    assert_eq!(
        w.put(
            5,
            1,
            vec![with_locators(
                compiled()[1].clone(),
                "button Pay now",
                vec![loc("C")]
            )]
        ),
        Ok(stored(0))
    );

    w.start(6, "compile");
    w.load(6);
    assert!(matches!(
        w.put(6, 2, compiled()),
        Ok(RunnerReply::Stored(_))
    ));
    assert!(w.finish_of(6, "compile", "completed", Some(2)).is_ok());

    w.host.put_workflow(super::host::workflow(
        &wf(3),
        "utility_bill_payment",
        "approved",
        None,
    ));
    let reply = w.coordinator(json!({"op": "create_agent", "workflow": wf(3), "agent": agent(3)}));
    assert!(matches!(
        reply,
        Ok(CoordinatorAnswer::Agent(CoordinatorReply::Rev(_)))
    ));
    assert!(rows_of(&w, "learn_routine", 3).is_empty());
    assert!(edits(&w).iter().all(|e| e.state == EditState::Provisional));
}

#[test]
fn a_start_run_that_arrives_again_at_runs_held_max_drops_no_other_held_run() {
    let mut w = active_world();
    let last = 2 + crate::RUNS_HELD_MAX as u32 - 1;
    for r in 2..last {
        w.replay(r);
    }
    w.host.refuse = true;
    w.start(42, "replay");
    let held = |w: &World, r: u32| {
        w.handlers
            .starts
            .contains_key(&RunId::parse(&run(r)).expect("a run id"))
    };
    assert!((2..last).all(|r| held(&w, r)));
    assert!(held(&w, 42));

    w.start(42, "replay");
    assert!(
        held(&w, 2),
        "the start of run 42 that arrived again dropped run 2, the run made first"
    );
    assert!((2..last).all(|r| held(&w, r)));
}

#[test]
fn a_start_run_that_arrives_again_after_its_refused_commit_stores_no_run_row_and_the_run_stays_lost(
) {
    let mut w = active_world();
    w.host.refuse = true;
    w.start(40, "replay");
    w.host.refuse = false;
    assert!(of_run(&w, 40).is_empty());
    let starts = w.handlers.starts.clone();

    w.start(40, "replay");
    assert!(
        of_run(&w, 40).is_empty(),
        "the start that arrived again stored the lost run's learn_run rev 1"
    );
    assert_eq!(w.handlers.starts, starts);
    w.load(40);
    let id = RunId::parse(&run(40)).expect("a run id");
    assert!(
        !w.handlers.facts.contains_key(&id),
        "the lost run has facts after a start that arrived again"
    );
}

#[test]
fn a_start_run_that_arrives_again_after_its_stopped_run_rows_left_by_their_bound_stores_no_run_row()
{
    let mut w = active_world();
    w.host.put_raw("learn_run", &refused_run(1));
    w.host.day = DAY + 92;
    w.start(41, "replay");
    assert!(of_run(&w, 41).is_empty());
    w.host.day = DAY + 93;

    w.start(41, "replay");
    assert!(
        of_run(&w, 41).is_empty(),
        "the start that arrived again stored the lost run's learn_run rev 1"
    );
    w.load(41);
    let id = RunId::parse(&run(41)).expect("a run id");
    assert!(
        !w.handlers.facts.contains_key(&id),
        "the lost run has facts after a start that arrived again"
    );
}

#[test]
fn a_start_run_that_arrives_again_where_its_row_stands_and_its_start_is_held_stores_the_due_routine_row_alone(
) {
    let mut w = active_world();
    w.start(50, "replay");
    assert_eq!(of_run(&w, 50).len(), 1);
    let starts = w.handlers.starts.clone();
    let routines = w.host.learn_rows("learn_routine").len();
    let commits = w.host.commits.get();
    w.host.day = DAY + 30;

    w.start(50, "replay");
    assert_eq!(w.host.commits.get(), commits + 1);
    assert_eq!(of_run(&w, 50).len(), 1);
    assert_eq!(w.host.learn_rows("learn_routine").len(), routines + 1);
    assert_eq!(w.handlers.starts, starts);
}

#[test]
fn a_start_run_of_a_held_run_under_another_agent_at_runs_held_max_drops_no_other_held_run() {
    let mut w = active_world();
    w.add_pair(2);
    let last = 2 + crate::RUNS_HELD_MAX as u32 - 1;
    for r in 2..last {
        w.replay(r);
    }
    w.host.refuse = true;
    w.start(42, "replay");
    w.host.refuse = false;
    let held = |w: &World, r: u32| {
        w.handlers
            .starts
            .contains_key(&RunId::parse(&run(r)).expect("a run id"))
    };
    assert!((2..last).all(|r| held(&w, r)));
    assert!(held(&w, 42));
    let reply = w.coordinator(json!({
        "op": "start_run", "workflow": wf(2), "agent": agent(2), "run": run(42), "mode": "replay",
    }));
    let binding = ErrorName::read("binding").expect("a declared name");
    assert_eq!(
        reply,
        Err(Refused::Error(binding)),
        "the run is the first agent's"
    );
    assert!(
        (2..last).all(|r| held(&w, r)),
        "a start of run 42 under agent 2 dropped a held run"
    );
    w.host.agent.hidden_runs.push(run(42));
    let reply = w.coordinator(json!({
        "op": "start_run", "workflow": wf(2), "agent": agent(2), "run": run(42), "mode": "replay",
    }));
    assert!(
        matches!(
            reply,
            Ok(CoordinatorAnswer::Agent(CoordinatorReply::Accepted(_)))
        ),
        "with the first agent's rows of run 42 removed the agent store accepts the start: {reply:?}"
    );
    assert!(
        (2..last).all(|r| held(&w, r)),
        "a start of the held run 42 under agent 2 dropped another held run"
    );
    let held_42 = &w.handlers.starts[&RunId::parse(&run(42)).expect("a run id")];
    assert_eq!(held_42.agent.as_str(), agent(2));
}

/// The kinds of the families `agent` and `learn` whose rows a runner body may answer (docs/privacy-limits.md): seven
/// of the family `agent`, and of a `learn_edit` row its target, which `load_steps` lays on a step.
const ANSWERED: [&str; 8] = [
    "agent_config",
    "agent_finished",
    "agent_playbook",
    "agent_bindings",
    "agent_session",
    "agent_start",
    "agent_reply",
    "learn_edit",
];

const PLAYBOOK_MARK: &str = "playbook-mark-p7";
const BINDINGS_MARK: &str = "bindings-mark-b7";
const EDIT_MARK: &str = "edit-mark-e7";
const SESSION_MARK: &str = "sessionmarks7";
const REPLY_MARK: &str = "reply-mark-m7";
const EFFECT_MARK: &str = "effect-mark-k7";
const CHECKPOINT_MARK: &str = "checkpoint-mark-c7";
const ROUTINE_MARK: &str = "com.example.routinemark";
const RUN_WALL_S: u32 = 7_340_033;
const LOG_MS: u32 = 7_340_047;
const LEARN_RUN: u32 = 32_343;
const ORIGIN: &str = "https://northwind-power.test:443";

/// Every stored row of the kind: the agent store's for the family `agent`, the learning rows for `learn`.
fn every_row(w: &World, kind: &str) -> Vec<String> {
    if kind.starts_with("learn_") {
        w.host.stored(kind)
    } else {
        w.host.agent_rows(kind)
    }
}

/// The value holds the marker: a string that holds its text, or a number equal to it.
fn holds(value: &Value, marker: &Value) -> bool {
    match (value, marker) {
        (Value::String(text), Value::String(m)) => text.contains(m.as_str()),
        (Value::Number(n), Value::Number(m)) => n == m,
        (Value::Array(items), _) => items.iter().any(|v| holds(v, marker)),
        (Value::Object(map), _) => map.values().any(|v| holds(v, marker)),
        _ => false,
    }
}

fn text_holds(text: &str, marker: &Value) -> bool {
    holds(
        &serde_json::from_str(text).expect("the JSON of a row or a reply"),
        marker,
    )
}

fn at(w: &World, offset: i64) -> i64 {
    w.host.agent_now().at.0 + offset
}

fn effect_row(w: &World, r: u32) -> Value {
    let class = ActionClass::parse("payment").expect("a class");
    let kept: Vec<Value> = class
        .kept_fields()
        .into_iter()
        .map(|(name, _)| json!({"name": name.as_str(), "value": [EFFECT_MARK]}))
        .collect();
    json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "effect": id('6', r), "seq": 1,
        "day": w.host.day, "at": at(w, 11), "state": "held", "class": "payment",
        "digest": format!("d{r:063x}"), "kept": kept, "fields": class.bound_fields().len(),
        "approval": null, "cause": null,
    })
}

fn log_line(w: &World, r: u32) -> Value {
    json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "seq": 2, "day": w.host.day,
        "at": at(w, 13), "event": "step_ok", "step": 1, "cause": null, "model": null, "ms": LOG_MS,
        "tokens_in": null, "tokens_out": null,
    })
}

fn checkpoint_row(w: &World, r: u32) -> Value {
    let folder = format!("/data/{CHECKPOINT_MARK}");
    json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "seq": 1, "day": w.host.day,
        "at": at(w, 17), "folder": [folder], "workbook": [format!("{folder}/book-{r}.xlsx")],
        "copy": [format!("book-{r}.halvo-checkpoint-{}.xlsx", run(r))], "sha256": null, "bytes": null,
        "state": "planned", "left": null,
    })
}

fn message(r: u32) -> String {
    format!("msg-{REPLY_MARK}-{r}")
}

fn reply_row(w: &World, r: u32) -> Value {
    json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "seq": 1, "day": w.host.day,
        "at": at(w, 19), "message": message(r), "draft": format!("draft-{REPLY_MARK}-{r}"),
        "effect": null, "state": "drafted",
    })
}

fn failed_record(w: &World, r: u32) -> Value {
    json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "day": w.host.day,
        "mode": "replay", "outcome": "failed", "playbook": null, "steps": 3, "prompts": 0,
        "handbacks": 0, "confirmations": 0, "effects": 1, "unknown": 0, "model_calls": 0,
        "tokens_in": 0, "tokens_out": 0, "wall_s": RUN_WALL_S, "model": null,
        "before_start": null, "cut": null, "no_key": null, "not_to_account": null,
        "incomplete": null, "reply_to_other": null, "more_pages": null, "held_recipient": null,
        "organized": null, "other_task": null, "held_earlier": null, "earlier_page": null,
    })
}

/// Sends one runner body through `LearnHandlers::runner` and keeps the JSON of its reply under its op.
fn answer(w: &mut World, op: &str, body: Value, replies: &mut Vec<(String, String)>) -> String {
    assert_eq!(body["op"], json!(op));
    let reply = w.runner(body);
    let Ok(reply) = reply else {
        panic!("the body {op} is refused: {reply:?}");
    };
    let text = reply.to_json().expect("the reply's JSON");
    replies.push((op.to_owned(), text.clone()));
    text
}

/// The bodies of the replay `r` that store a row: its session, an effect, a log line, a checkpoint, a reply row and
/// its `finish` with the outcome `failed`, since the effect stays `held`.
fn put_bodies(w: &mut World, r: u32, replies: &mut Vec<(String, String)>) {
    let a = agent(1);
    let bodies = [
        json!({"op": "put_session", "agent": a, "run": run(r), "origin": ORIGIN, "rev": r, "part": 1, "of": 1, "data": [SESSION_MARK]}),
        json!({"op": "effect", "agent": a, "run": run(r), "record": effect_row(w, r)}),
        json!({"op": "log", "agent": a, "run": run(r), "records": [log_line(w, r)]}),
        json!({"op": "checkpoint", "agent": a, "run": run(r), "record": checkpoint_row(w, r)}),
        json!({"op": "reply_row", "agent": a, "run": run(r), "record": reply_row(w, r)}),
    ];
    for body in bodies {
        let op = body["op"].as_str().expect("an op").to_owned();
        answer(w, &op, body, replies);
    }
}

#[test]
fn a_runner_body_answers_no_kind_outside_the_closed_list() {
    let mut steps = compiled();
    steps[2] = click(3, &format!("button Next {PLAYBOOK_MARK}"), vec![next()]);
    let mut w = active_world_with(steps);
    let reply = w.coordinator(json!({
        "op": "set_bindings", "workflow": wf(1), "agent": agent(1),
        "slots": [{"name": "payee", "value": [BINDINGS_MARK]}],
    }));
    assert!(reply.is_ok(), "set_bindings: {reply:?}");
    w.learn(
        2,
        2,
        &format!("button Pay {EDIT_MARK}"),
        vec![loc("Pay e7"), pay()],
    );
    let edits = w.host.edits();
    assert_eq!(edits.len(), 1, "the replay of run 2 kept one edit");
    let edit = edits[0].edit.as_str().to_owned();

    let mut planting = Vec::new();
    w.start(3, "replay");
    let a = agent(1);
    answer(
        &mut w,
        "load",
        json!({"op": "load", "agent": a, "run": run(3)}),
        &mut planting,
    );
    let body =
        json!({"op": "load_steps", "agent": a, "run": run(3), "number": w.loaded, "from": 1});
    answer(&mut w, "load_steps", body, &mut planting);
    put_bodies(&mut w, 3, &mut planting);
    let body = json!({"op": "finish", "agent": a, "run": run(3), "record": failed_record(&w, 3)});
    answer(&mut w, "finish", body, &mut planting);

    let mut learn_run: Value =
        serde_json::from_str(&w.host.learn_rows("learn_run")[0]).expect("a learn_run row");
    learn_run["run"] = json!(run(LEARN_RUN));
    let learn_run = learn_run.to_string();
    assert!(LearnRun::parse(learn_run.as_bytes()).is_ok());
    w.host.put_raw("learn_run", &learn_run);
    let mut routine = w.host.routines()[0].clone();
    let mut text: Value =
        serde_json::from_str(&routine.to_json().expect("a routine's JSON")).expect("JSON");
    text["app"] = json!(ROUTINE_MARK);
    text["rev"] = json!(routine.rev.get() + 1);
    routine = LearnRoutine::parse(text.to_string().as_bytes()).expect("a routine row");
    w.host.put_raw(
        "learn_routine",
        &routine.to_json().expect("a routine's JSON"),
    );

    let outside = [
        ("agent_run", json!(RUN_WALL_S)),
        ("agent_effect", json!(EFFECT_MARK)),
        ("agent_log", json!(LOG_MS)),
        ("agent_checkpoint", json!(CHECKPOINT_MARK)),
        ("learn_run", json!(run(LEARN_RUN))),
        ("learn_routine", json!(ROUTINE_MARK)),
        ("learn_edit", json!(edit)),
    ];
    let kinds: Vec<&str> = halvo_schema::executable::KINDS
        .iter()
        .chain(halvo_schema::learning::KINDS)
        .map(|k| k.name)
        .collect();
    assert_eq!(kinds.len(), 14);
    for kind in &kinds {
        assert!(!every_row(&w, kind).is_empty(), "no row of the kind {kind}");
        assert!(
            ANSWERED.contains(kind) || outside.iter().any(|(k, _)| k == kind),
            "the kind {kind} is neither in the closed list nor marked"
        );
    }
    for (kind, marker) in &outside {
        for other in &kinds {
            let held = every_row(&w, other)
                .iter()
                .any(|row| text_holds(row, marker));
            assert_eq!(
                held,
                other == kind,
                "the marker {marker} of {kind} in the rows of {other}"
            );
        }
    }

    let mut replies = Vec::new();
    w.start(4, "replay");
    answer(
        &mut w,
        "load",
        json!({"op": "load", "agent": a, "run": run(4)}),
        &mut replies,
    );
    let body =
        json!({"op": "load_steps", "agent": a, "run": run(4), "number": w.loaded, "from": 1});
    let served: Value =
        serde_json::from_str(&answer(&mut w, "load_steps", body, &mut replies)).expect("JSON");
    let body = json!({"op": "load_session", "agent": a, "run": run(4), "origin": ORIGIN, "rev": 3, "part": 1});
    answer(&mut w, "load_session", body, &mut replies);
    let proposal = with_locators(
        served["steps"][1].clone(),
        "button Pay again",
        vec![loc("Pay z"), pay()],
    );
    let body = json!({"op": "put_playbook", "agent": a, "run": run(4), "number": w.loaded, "steps": [proposal]});
    answer(&mut w, "put_playbook", body, &mut replies);
    put_bodies(&mut w, 4, &mut replies);
    let body = json!({"op": "drop_session", "agent": a, "run": run(4), "origin": ORIGIN});
    answer(&mut w, "drop_session", body, &mut replies);
    let body = json!({"op": "load_replies", "agent": a, "run": run(4), "message_ids": [message(3), message(4)]});
    answer(&mut w, "load_replies", body, &mut replies);
    let body = json!({"op": "finish", "agent": a, "run": run(4), "record": failed_record(&w, 4)});
    answer(&mut w, "finish", body, &mut replies);

    let mut ops: Vec<&str> = replies.iter().map(|(op, _)| op.as_str()).collect();
    ops.sort_unstable();
    ops.dedup();
    assert_eq!(ops.len(), 12, "the bodies of run 4: {ops:?}");
    for (op, text) in planting.iter().chain(&replies) {
        for (kind, marker) in &outside {
            assert!(
                !text_holds(text, marker),
                "the reply of {op} holds {marker}, the marker of {kind}: {text}"
            );
        }
    }
    for (op, marker) in [
        ("load", BINDINGS_MARK),
        ("load_steps", EDIT_MARK),
        ("load_steps", PLAYBOOK_MARK),
        ("load_session", SESSION_MARK),
        ("load_replies", REPLY_MARK),
    ] {
        let seen = replies
            .iter()
            .any(|(o, text)| o == op && text_holds(text, &json!(marker)));
        assert!(seen, "the reply of {op} holds no {marker}");
    }
}
