use halvo_schema::executable::RunnerReply;
use halvo_schema::learning::record::{EditState, LearnEdit, LearnRoutine, LearnRun, RunStatus};
use halvo_schema::{ErrorName, WorkflowId};
use serde_json::{json, Value};

use super::host::{active_world, agent, compiled, locator, run, wf, with_locators, World, DAY};
use crate::{CoordinatorAnswer, LearnHandlers, LearnHost, Refused};

const KINDS: [&str; 3] = ["learn_edit", "learn_run", "learn_routine"];

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

fn store_error() -> Result<CoordinatorAnswer, Refused> {
    Err(Refused::Error(ErrorName::Store))
}

/// A row of the kind that its parser refuses and whose text names workflow `n` and its agent.
fn refused(kind: &str, n: u32) -> String {
    let (workflow, agent) = (wf(n), agent(n));
    match kind {
        "learn_edit" => format!(
            r#"{{"v":1,"workflow":"{workflow}","agent":"{agent}","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":1,"base":1,"n":2,"state":"provisional","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"],"first":20767,"last":20767,"clean":3,"target":{{"desc":"button Pay","locators":[{{"by":"role","value":"button","name":"Pay"}}]}}}}"#
        ),
        "learn_run" => format!(
            r#"{{"v":1,"workflow":"{workflow}","agent":"{agent}","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":3,"day":20771,"mode":"replay","state":"started","cause":null,"base":null,"assisted":0,"kept":0,"failed_step":null}}"#
        ),
        _ => format!(
            r#"{{"v":1,"workflow":"{workflow}","agent":"{agent}","rev":1,"day":20771,"category":"utility_bill_payment","site":null,"app":"com.example.browser","period":null,"seen":0,"first":null,"last":null,"next":null,"missed":0,"mark":"gone","since":null}}"#
        ),
    }
}

/// A refused row of `kind` whose text names workflow `names`, held under the key of workflow `key`, or under none.
struct Planted {
    kind: &'static str,
    key: Option<u32>,
    names: u32,
}

/// Workflow 1 (A) and workflow 2 (B), each with its agent compiled under number 1 and active, and one learned edit on
/// step 2 of each.
fn two_workflows() -> World {
    let mut w = active_world();
    w.add_pair(2);
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.pair = 2;
    w.learn(3, 2, "button Pay now", vec![loc("B"), pay()]);
    w.pair = 1;
    w
}

/// What every body of the agent of one workflow gets, and what it stores.
#[derive(Clone, Debug, PartialEq)]
struct Seen {
    served: Vec<String>,
    put: Result<RunnerReply, Refused>,
    finish: Result<RunnerReply, Refused>,
    runs: Vec<(u32, Option<u32>)>,
    edits_added: usize,
    list_before: Result<CoordinatorAnswer, Refused>,
    list: Result<CoordinatorAnswer, Refused>,
    end_run: Result<CoordinatorAnswer, Refused>,
    ended: Vec<(u32, Option<u32>)>,
    routines_added: usize,
    all_withdrawn: bool,
}

fn names(step: &Value) -> Vec<String> {
    step["target"]["locators"]
        .as_array()
        .expect("locators")
        .iter()
        .map(|l| l["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn of_run(w: &World, r: u32) -> Vec<(u32, Option<u32>)> {
    let mut rows: Vec<(u32, Option<u32>)> = w
        .host
        .stored("learn_run")
        .iter()
        .filter_map(|row| LearnRun::parse(row.as_bytes()).ok())
        .filter(|row| row.run.as_str() == run(r))
        .map(|row| {
            let kept = match row.status {
                RunStatus::Ended { kept, .. } => Some(kept),
                RunStatus::Started => None,
            };
            (row.rev(), kept)
        })
        .collect();
    rows.sort();
    rows
}

fn edits_of(w: &World, n: u32) -> Vec<LearnEdit> {
    w.host
        .stored("learn_edit")
        .iter()
        .filter_map(|row| LearnEdit::parse(row.as_bytes()).ok())
        .filter(|e| e.workflow.as_str() == wf(n))
        .collect()
}

fn routines_of(w: &World, n: u32) -> usize {
    w.host
        .stored("learn_routine")
        .iter()
        .filter_map(|row| LearnRoutine::parse(row.as_bytes()).ok())
        .filter(|r| r.workflow.as_str() == wf(n))
        .count()
}

fn list(w: &mut World, n: u32) -> Result<CoordinatorAnswer, Refused> {
    w.coordinator(json!({"op": "list_refinements", "workflow": wf(n), "agent": agent(n)}))
}

/// Plants the row, then sends every body of the agent of workflow `n`: a replay run with a proposal, its log and
/// its finish; `list_refinements`; a run ended by `end_run`; a `start_run` 40 days on; and a compile of number 2.
fn observe(planted: Option<&Planted>, n: u32) -> Seen {
    let mut w = two_workflows();
    if let Some(p) = planted {
        let key = p.key.map(wf);
        w.host
            .put_raw_under(p.kind, key.as_deref(), &refused(p.kind, p.names));
    }
    w.pair = n;
    w.host.day = DAY + 1;
    let edits_before = edits_of(&w, n).len();
    let list_before = list(&mut w, n);
    let served = w.replay(10);
    let proposal = with_locators(
        served[2].clone(),
        "button Next step",
        vec![loc("C"), next()],
    );
    let put = w.put(10, w.loaded, vec![proposal]);
    w.log(
        10,
        &[
            (2, "step_ok", 2, None),
            (3, "recovered", 3, None),
            (4, "step_ok", 3, None),
        ],
    );
    let finish = w.finish(10, "replay", "completed");
    let runs = of_run(&w, 10);
    let edits_added = edits_of(&w, n).len() - edits_before;
    let listed = list(&mut w, n);
    w.replay(11);
    let end_run = w.coordinator(
        json!({"op": "end_run", "workflow": wf(n), "agent": agent(n), "run": run(11)}),
    );
    let ended = of_run(&w, 11);
    let routines_before = routines_of(&w, n);
    w.host.day = DAY + 40;
    w.start(12, "replay");
    let routines_added = routines_of(&w, n) - routines_before;
    w.compile(13, 2, compiled());
    let mut newest: std::collections::BTreeMap<String, LearnEdit> =
        std::collections::BTreeMap::new();
    for edit in edits_of(&w, n) {
        let id = edit.edit.as_str().to_owned();
        if newest.get(&id).is_none_or(|e| e.rev < edit.rev) {
            newest.insert(id, edit);
        }
    }
    let all_withdrawn = newest.values().all(|e| e.state == EditState::Withdrawn);
    Seen {
        served: names(&served[1]),
        put,
        finish,
        runs,
        edits_added,
        list_before,
        list: listed,
        end_run,
        ended,
        routines_added,
        all_withdrawn,
    }
}

/// What the bodies of a workflow get and store where a refused row of `kind` stands under its key (contracts.md L4.2;
/// the coordinator's ruling LM-58 (a), (b)), from what they get and store with no refused row.
fn stopped(kind: &str, clean: &Seen) -> Seen {
    let mut seen = clean.clone();
    match kind {
        "learn_edit" => {
            seen.served = vec!["Pay".to_owned()];
            seen.put = Ok(stored(0));
            seen.runs = vec![(1, None), (2, Some(0))];
            seen.edits_added = 0;
            seen.list_before = store_error();
            seen.list = store_error();
            seen.all_withdrawn = false;
        }
        "learn_run" => {
            seen.served = vec!["Pay".to_owned()];
            seen.put = Ok(stored(0));
            seen.runs = Vec::new();
            seen.edits_added = 0;
            seen.list = clean.list_before.clone();
            seen.ended = Vec::new();
            seen.routines_added = 0;
        }
        _ => {
            seen.list_before = store_error();
            seen.list = store_error();
            seen.routines_added = 0;
        }
    }
    seen
}

fn clean(n: u32) -> Seen {
    let seen = observe(None, n);
    let learned = if n == 1 { "A" } else { "B" };
    assert_eq!(seen.served, [learned, "Pay"]);
    assert_eq!(seen.put, Ok(stored(1)));
    assert_eq!(seen.runs, [(1, None), (2, Some(1))]);
    assert!(seen.edits_added > 0);
    assert!(
        matches!(&seen.list_before, Ok(CoordinatorAnswer::Refinements(r)) if r.refinements.len() == 1)
    );
    assert_ne!(seen.list, seen.list_before);
    assert!(
        matches!(&seen.list, Ok(CoordinatorAnswer::Refinements(r)) if !r.refinements.is_empty())
    );
    assert_eq!(seen.ended, [(1, None), (2, Some(0))]);
    assert_eq!(seen.routines_added, 1);
    assert!(seen.all_withdrawn);
    seen
}

fn under_the_key_of_b_naming_a(kind: &'static str) {
    let (clean_a, clean_b) = (clean(1), clean(2));
    let planted = Planted {
        kind,
        key: Some(2),
        names: 1,
    };
    assert_eq!(observe(Some(&planted), 1), clean_a, "{kind}: workflow A");
    assert_eq!(
        observe(Some(&planted), 2),
        stopped(kind, &clean_b),
        "{kind}: workflow B"
    );
}

#[test]
fn a_refused_learn_edit_row_under_the_key_of_b_whose_text_names_a_stops_the_edits_of_b_and_nothing_of_a(
) {
    under_the_key_of_b_naming_a("learn_edit");
}

#[test]
fn a_refused_learn_run_row_under_the_key_of_b_whose_text_names_a_stops_the_runs_of_b_and_nothing_of_a(
) {
    under_the_key_of_b_naming_a("learn_run");
}

#[test]
fn a_refused_learn_routine_row_under_the_key_of_b_whose_text_names_a_stops_the_routine_of_b_and_nothing_of_a(
) {
    under_the_key_of_b_naming_a("learn_routine");
}

#[test]
fn a_refused_row_under_the_key_of_a_whose_text_names_b_stops_a_and_nothing_of_b() {
    let (clean_a, clean_b) = (clean(1), clean(2));
    for kind in KINDS {
        let planted = Planted {
            kind,
            key: Some(1),
            names: 2,
        };
        assert_eq!(
            observe(Some(&planted), 1),
            stopped(kind, &clean_a),
            "{kind}: workflow A"
        );
        assert_eq!(observe(Some(&planted), 2), clean_b, "{kind}: workflow B");
    }
}

#[test]
fn a_refused_row_held_under_no_key_is_read_by_no_workflow_and_stops_nothing() {
    let (clean_a, clean_b) = (clean(1), clean(2));
    for kind in KINDS {
        let planted = Planted {
            kind,
            key: None,
            names: 1,
        };
        assert_eq!(observe(Some(&planted), 1), clean_a, "{kind}: workflow A");
        assert_eq!(observe(Some(&planted), 2), clean_b, "{kind}: workflow B");
    }
}

#[test]
fn a_refused_learn_run_row_under_the_key_of_a_run_that_was_served_answers_its_replay_put_with_the_count_0(
) {
    let mut w = two_workflows();
    let served = w.replay(10);
    let proposal = with_locators(
        served[2].clone(),
        "button Next step",
        vec![loc("C"), next()],
    );
    w.host
        .put_raw_under("learn_run", Some(&wf(1)), &refused("learn_run", 2));
    assert_eq!(w.put(10, w.loaded, vec![proposal.clone()]), Ok(stored(0)));
    w.pair = 2;
    let served = w.replay(20);
    let proposal = with_locators(
        served[2].clone(),
        "button Next step",
        vec![loc("C"), next()],
    );
    assert_eq!(w.put(20, w.loaded, vec![proposal]), Ok(stored(1)));
}

#[test]
fn while_a_run_of_a_is_held_a_refused_learn_edit_row_under_the_key_of_a_stops_nothing_of_a_run_of_b(
) {
    let mut w = two_workflows();
    w.host
        .put_raw_under("learn_edit", Some(&wf(1)), &refused("learn_edit", 1));
    w.host.day = DAY + 1;
    let held = w.replay(10);
    assert_eq!(names(&held[1]), ["Pay"]);
    w.pair = 2;
    let edits_before = edits_of(&w, 2).len();
    let served = w.replay(20);
    assert_eq!(names(&served[1]), ["B", "Pay"]);
    let proposal = with_locators(
        served[2].clone(),
        "button Next step",
        vec![loc("C"), next()],
    );
    assert_eq!(w.put(20, w.loaded, vec![proposal]), Ok(stored(1)));
    w.log(
        20,
        &[
            (2, "step_ok", 2, None),
            (3, "recovered", 3, None),
            (4, "step_ok", 3, None),
        ],
    );
    assert_eq!(w.finish(20, "replay", "completed"), Ok(stored(1)));
    assert_eq!(of_run(&w, 20), [(1, None), (2, Some(1))]);
    assert!(edits_of(&w, 2).len() > edits_before);
    w.pair = 1;
    w.log(10, &[(2, "step_ok", 2, None)]);
    assert_eq!(w.finish(10, "replay", "completed"), Ok(stored(1)));
    assert_eq!(of_run(&w, 10), [(1, None), (2, Some(0))]);
}

#[test]
fn a_replay_put_playbook_of_a_run_whose_start_is_held_reads_no_row_of_another_workflow() {
    let mut w = two_workflows();
    let served = w.replay(10);
    let proposal = with_locators(
        served[2].clone(),
        "button Next step",
        vec![loc("C"), next()],
    );
    w.host.visited.borrow_mut().clear();
    assert_eq!(w.put(10, w.loaded, vec![proposal.clone()]), Ok(stored(1)));
    let visited = w.host.visited.borrow().clone();
    assert!(!visited.is_empty());
    assert!(
        visited
            .iter()
            .all(|key| key.as_deref() == Some(wf(1).as_str())),
        "the put of a run of workflow A read rows of another key: {visited:?}"
    );
    w.handlers = LearnHandlers::new();
    w.host.visited.borrow_mut().clear();
    assert_eq!(w.put(10, w.loaded, vec![proposal]), Ok(stored(0)));
    assert!(
        w.host
            .visited
            .borrow()
            .iter()
            .any(|key| key.as_deref() == Some(wf(2).as_str())),
        "with no start held, the run row is found by its id among the rows of every workflow"
    );
}

#[test]
fn the_keyed_read_hands_out_the_rows_committed_under_the_key_in_byte_order_and_no_row_under_another_key_or_none(
) {
    let mut w = two_workflows();
    for kind in KINDS {
        w.host.put_raw_under(kind, None, &refused(kind, 1));
        w.host.put_raw_under(kind, Some(&wf(2)), &refused(kind, 1));
    }
    for n in [1, 2] {
        let workflow = WorkflowId::parse(&wf(n)).expect("a workflow id");
        for kind in KINDS {
            let mut committed: Vec<String> = w
                .host
                .stored(kind)
                .into_iter()
                .filter(|row| {
                    serde_json::from_str::<Value>(row).is_ok_and(|v| v["workflow"] == json!(wf(n)))
                })
                .filter(|row| *row != refused(kind, 1))
                .collect();
            if n == 2 {
                committed.push(refused(kind, 1));
            }
            committed.sort();
            let mut seen = Vec::new();
            LearnHost::for_each_under(&w.host, kind, &workflow, &mut |row| {
                seen.push(row.to_owned());
            });
            assert!(!seen.is_empty(), "{kind} of workflow {n}");
            assert_eq!(seen, committed, "{kind} of workflow {n}");
        }
    }
}
