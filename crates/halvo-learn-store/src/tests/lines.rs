use std::num::NonZeroU32;

use halvo_schema::executable::RunId;
use halvo_schema::learning::record::{EditState, LearnEdit};
use serde_json::{json, Value};

use super::host::{active_world, locator, run, with_locators, World};
use crate::facts::Line;
use crate::LINES_HELD_MAX;

type Sent = (u32, &'static str, u32, Option<&'static str>);

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

/// The lines of run 3: a counted failure of step 2, a recovered step 3, then `step_ok` lines of step 1 up to `total`.
fn lines(total: usize) -> Vec<Sent> {
    let mut out: Vec<Sent> = vec![
        (1, "step_failed", 2, Some("step_failed")),
        (2, "recovered", 3, None),
        (3, "step_ok", 3, None),
    ];
    let mut seq = 4;
    while out.len() < total {
        out.push((seq, "step_ok", 1, None));
        seq += 1;
    }
    out
}

/// Run 2 learns an edit of step 2; run 3 is served it, proposes a target for step 3, sends `total` lines in bodies
/// of 2,048, then the first body again `again` times.
fn play(total: usize, again: usize) -> World {
    let mut w = active_world();
    w.learn(
        2,
        2,
        "button Pay now",
        vec![loc("A"), locator("role", "button", Some("Pay"))],
    );
    let served = w.replay(3);
    let proposal = with_locators(
        served[2].clone(),
        "button Next step",
        vec![loc("B"), locator("role", "button", Some("Next"))],
    );
    assert!(w.put(3, w.loaded, vec![proposal]).is_ok());
    let sent = lines(total);
    for body in sent.chunks(2048) {
        w.log(3, body);
    }
    for _ in 0..again {
        w.log(3, &sent[..2048]);
    }
    w
}

fn finished(mut w: World) -> World {
    assert!(w.finish(3, "replay", "completed").is_ok());
    w
}

fn revs_of_run(w: &World, r: u32) -> Vec<u32> {
    let mut revs: Vec<u32> = w
        .host
        .runs()
        .iter()
        .filter(|row| row.run.as_str() == run(r))
        .map(|row| row.rev())
        .collect();
    revs.sort_unstable();
    revs
}

fn of_step(w: &World, n: u32) -> Vec<LearnEdit> {
    let mut rows: Vec<LearnEdit> = w
        .host
        .edits()
        .into_iter()
        .filter(|e| e.n.get() == n)
        .collect();
    rows.sort_by_key(|e| e.rev);
    rows
}

fn learned_as_today(w: &World) {
    assert_eq!(revs_of_run(w, 3), [1, 2]);
    let step_2 = of_step(w, 2);
    assert_eq!(step_2.len(), 2);
    assert!(matches!(step_2[1].state, EditState::Reverted(_)));
    let step_3 = of_step(w, 3);
    assert_eq!(step_3.len(), 1);
    assert_eq!(step_3[0].state, EditState::Provisional);
}

#[test]
fn a_run_that_sends_exactly_lines_held_max_lines_finishes_as_today() {
    let w = finished(play(LINES_HELD_MAX, 0));
    learned_as_today(&w);
}

#[test]
fn a_run_one_line_past_lines_held_max_is_a_lost_run_and_the_next_run_learns_as_today() {
    let mut w = play(LINES_HELD_MAX + 1, 0);
    let id = RunId::parse(&run(3)).expect("a run id");
    assert!(!w.handlers.facts.contains_key(&id));
    assert!(!w.handlers.starts.contains_key(&id));
    w.load(3);
    assert!(!w.handlers.facts.contains_key(&id));
    let mut w = finished(w);
    assert_eq!(revs_of_run(&w, 3), [1]);
    let step_2 = of_step(&w, 2);
    assert_eq!(step_2.len(), 1);
    assert_eq!(step_2[0].state, EditState::Provisional);
    assert!(of_step(&w, 3).is_empty());
    w.failed_use(4, 2, "step_failed");
    assert_eq!(revs_of_run(&w, 4), [1, 2]);
    let step_2 = of_step(&w, 2);
    assert_eq!(step_2.len(), 2);
    assert!(matches!(step_2[1].state, EditState::Reverted(_)));
}

#[test]
fn a_log_body_that_arrives_twice_counts_its_lines_once() {
    let w = finished(play(LINES_HELD_MAX, 1));
    learned_as_today(&w);
}

#[test]
fn one_held_line_takes_at_most_16_bytes_and_lines_held_max_of_them_at_most_65_536_bytes() {
    assert_eq!(LINES_HELD_MAX, 4_096);
    let one = std::mem::size_of::<(NonZeroU32, Line)>();
    assert!(one <= 16, "one held line takes {one} bytes");
    assert!(LINES_HELD_MAX * one <= 65_536);
}
