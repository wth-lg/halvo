use std::num::NonZeroU32;

use halvo_schema::executable::RunId;
use halvo_schema::learning::record::{EditState, LearnEdit};
use serde_json::{json, Value};

use super::host::{active_world, compiled, locator, run, World};

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

fn rows(w: &World) -> Vec<LearnEdit> {
    let mut rows = w.host.edits();
    rows.sort_by_key(|e| e.rev);
    rows
}

fn learn_on_step_2(w: &mut World, r: u32) {
    w.learn(
        r,
        2,
        "button Pay now",
        vec![loc("A"), locator("role", "button", Some("Pay"))],
    );
}

fn marked(w: &World, r: u32) -> Vec<u32> {
    let id = RunId::parse(&run(r)).expect("a run id");
    w.handlers.facts[&id]
        .marked
        .iter()
        .map(|n| n.get())
        .collect()
}

#[test]
fn two_replies_that_carried_different_edits_for_a_step_give_it_no_clean_use() {
    let mut w = active_world();
    let first = w.replay(2);
    assert_eq!(names(&first[1]), ["Pay"]);
    learn_on_step_2(&mut w, 3);
    let second = w.load_steps(2, 1, 1);
    assert_eq!(names(&second[1]), ["A", "Pay"]);
    assert_eq!(marked(&w, 2), [2]);
    w.log(2, &[(2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    let rows = rows(&w);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].state, rows[0].clean), (EditState::Provisional, 0));
}

#[test]
fn two_replies_that_carried_two_different_edits_for_a_step_give_it_no_clean_use() {
    let mut w = active_world();
    learn_on_step_2(&mut w, 2);
    let first = w.replay(3);
    assert_eq!(names(&first[1]), ["A", "Pay"]);
    w.failed_use(4, 2, "step_failed");
    w.learn(
        5,
        2,
        "button Pay now",
        vec![loc("B"), locator("role", "button", Some("Pay"))],
    );
    let second = w.load_steps(3, 1, 1);
    assert_eq!(names(&second[1]), ["B", "Pay"]);
    assert_eq!(marked(&w, 3), [2]);
    w.log(3, &[(2, "step_ok", 2, None)]);
    assert!(w.finish(3, "replay", "completed").is_ok());
    let second_edit: Vec<LearnEdit> = rows(&w)
        .into_iter()
        .filter(|e| {
            e.target.locators[0]
                .name
                .as_ref()
                .is_some_and(|n| n.as_str() == "B")
        })
        .collect();
    assert_eq!(second_edit.len(), 1);
    assert_eq!(
        (second_edit[0].state, second_edit[0].clean),
        (EditState::Provisional, 0)
    );
}

#[test]
fn two_replies_that_carried_different_edits_lay_a_counted_failure_on_the_edit_of_the_last_reply() {
    let mut w = active_world();
    w.replay(2);
    learn_on_step_2(&mut w, 3);
    w.load_steps(2, 1, 1);
    w.log(2, &[(2, "step_failed", 2, Some("step_failed"))]);
    assert!(w.finish(2, "replay", "failed").is_ok());
    let rows = rows(&w);
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[1].state, EditState::Reverted(_)));
}

#[test]
fn a_newer_revision_of_the_same_edit_between_two_replies_leaves_the_clean_use() {
    let mut w = active_world();
    learn_on_step_2(&mut w, 2);
    w.replay(3);
    w.clean_use(4, 2);
    let again = w.load_steps(3, 1, 1);
    assert_eq!(names(&again[1]), ["A", "Pay"]);
    assert!(marked(&w, 3).is_empty());
    w.log(3, &[(2, "step_ok", 2, None)]);
    assert!(w.finish(3, "replay", "completed").is_ok());
    let rows = rows(&w);
    assert_eq!(rows.len(), 3);
    assert_eq!((rows[2].state, rows[2].clean), (EditState::Active, 2));
}

#[test]
fn a_reply_under_another_number_than_the_loaded_one_changes_nothing_that_the_run_holds() {
    let mut w = active_world();
    learn_on_step_2(&mut w, 2);
    w.replay(3);
    w.start(4, "compile");
    w.load(4);
    assert!(w.put(4, 2, compiled()).is_ok());
    assert!(w.finish_of(4, "compile", "completed", Some(2)).is_ok());
    let id = RunId::parse(&run(3)).expect("a run id");
    let held = |w: &World| -> Vec<(NonZeroU32, Option<String>)> {
        w.handlers.facts[&id]
            .served
            .iter()
            .map(|(n, s)| (*n, s.edit.as_ref().map(|e| e.as_str().to_owned())))
            .collect()
    };
    let before = held(&w);
    let other = w.load_steps(3, 2, 1);
    assert_eq!(names(&other[1]), ["Pay"]);
    assert_eq!(held(&w), before);
    assert!(marked(&w, 3).is_empty());
    let count = w.host.count();
    w.log(3, &[(2, "step_ok", 2, None)]);
    assert!(w.finish(3, "replay", "completed").is_ok());
    assert_eq!(w.host.count(), count + 1);
    assert_eq!(rows(&w).len(), 2);
}
