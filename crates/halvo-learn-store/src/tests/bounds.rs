use halvo_learn::AdmitCause;
use halvo_schema::executable::{RunnerCall, RunnerReply};
use halvo_schema::learning::record::LearnEdit;
use serde_json::{json, Value};

use super::coordinator::edit_row;
use super::host::{active_world_with, agent, click, compiled, run, with_locators, World, DAY};
use crate::LearnHost;

fn css(c: char, len: usize) -> Value {
    json!({"by": "css", "value": c.to_string().repeat(len), "name": null})
}

fn bytes(step: &Value) -> usize {
    serde_json::to_string(step).expect("JSON").len()
}

fn steps_with(compiled_len: usize) -> Vec<Value> {
    let mut steps = compiled();
    steps[1] = click(2, "button Pay", vec![css('a', compiled_len)]);
    steps
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

fn admit(w: &mut World, r: u32, step: Value) -> Result<(), AdmitCause> {
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": 1, "steps": [step]});
    let Ok(RunnerCall::PutPlaybook(b)) = RunnerCall::parse(body.to_string().as_bytes()) else {
        panic!("a put_playbook body");
    };
    let answers = w
        .handlers
        .propose(&mut w.host, &b)
        .expect("the proposal is read");
    assert_eq!(answers.len(), 1);
    answers[0].1
}

fn padding(total: usize, size: impl Fn(usize) -> usize) -> usize {
    total - size(0)
}

fn admitted_and_stored(total: usize) -> (Result<(), AdmitCause>, usize, Vec<Value>) {
    let compiled_step = steps_with(900)[1].clone();
    let served_with = |p: usize| {
        bytes(&with_locators(
            compiled_step.clone(),
            "button Pay",
            vec![css('p', p), css('a', 900)],
        ))
    };
    let p = padding(total, served_with);
    assert_eq!(served_with(p), total);
    let mut w = active_world_with(steps_with(900));
    let served = w.replay(2);
    let proposal = with_locators(served[1].clone(), "button Pay", vec![css('p', p)]);
    let answer = admit(&mut w, 2, proposal.clone());
    let taken = u32::from(answer.is_ok());
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(taken)));
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    let edits = newest(&w).len();
    (answer, edits, w.replay(3))
}

#[test]
fn a_recovery_whose_served_step_has_2048_bytes_is_admitted_stored_and_laid_and_one_of_2049_is_refused_with_length(
) {
    let (answer, edits, served) = admitted_and_stored(2048);
    assert_eq!(answer, Ok(()));
    assert_eq!(edits, 1);
    assert_eq!(bytes(&served[1]), 2048);
    assert_eq!(
        served[1]["target"]["locators"]
            .as_array()
            .expect("locators")
            .len(),
        2
    );

    let (answer, edits, served) = admitted_and_stored(2049);
    assert_eq!(answer, Err(AdmitCause::Length));
    assert_eq!(edits, 0);
    assert_eq!(served[1], steps_with(900)[1]);
}

fn laid(total: usize) -> Value {
    let compiled_step = steps_with(900)[1].clone();
    let served_with = |q: usize| {
        bytes(&with_locators(
            compiled_step.clone(),
            "button Pay now",
            vec![css('q', q), css('a', 900)],
        ))
    };
    let q = padding(total, served_with);
    assert_eq!(served_with(q), total);
    let mut w = active_world_with(steps_with(900));
    w.host
        .commit(vec![edit_row(
            1,
            1,
            1,
            2,
            "provisional",
            None,
            &[11],
            (DAY, DAY),
            0,
            vec![css('q', q)],
        )])
        .expect("the row is stored");
    w.replay(2)[1].clone()
}

#[test]
fn a_stored_edit_that_makes_its_step_2048_bytes_is_laid_and_one_that_makes_it_2049_is_not() {
    let at = laid(2048);
    assert_eq!(bytes(&at), 2048);
    assert_eq!(at["target"]["desc"], json!("button Pay now"));
    assert_eq!(laid(2049), steps_with(900)[1]);
}

fn kept_at_finish(total: usize) -> (usize, Vec<LearnEdit>) {
    let compiled_step = steps_with(600)[1].clone();
    let served_with = |e: usize| {
        bytes(&with_locators(
            compiled_step.clone(),
            "button Pay",
            vec![css('p', 500), css('e', e), css('a', 600)],
        ))
    };
    let e = padding(total, served_with);
    assert_eq!(served_with(e), total);
    let mut w = active_world_with(steps_with(600));
    let served = w.replay(2);
    let late = with_locators(served[1].clone(), "button Pay", vec![css('p', 500)]);
    assert_eq!(admit(&mut w, 2, late.clone()), Ok(()));
    assert_eq!(w.put(2, 1, vec![late]), Ok(stored(1)));
    w.learn(3, 2, "button Pay", vec![css('e', e)]);
    w.clean_use(4, 2);
    w.clean_use(5, 2);
    assert_eq!(newest(&w)[0].state.name(), "active");
    w.log(2, &[(1, "recovered", 2, None), (2, "step_ok", 2, None)]);
    assert!(w.finish(2, "replay", "completed").is_ok());
    (bytes(&w.replay(6)[1]), newest(&w))
}

#[test]
fn a_candidate_whose_step_on_the_base_at_finish_has_2048_bytes_is_stored_and_one_of_2049_is_not() {
    let (served, edits) = kept_at_finish(2048);
    assert_eq!(edits.len(), 2);
    assert_eq!(served, 2048);
    let (served, edits) = kept_at_finish(2049);
    assert_eq!(edits.len(), 1);
    assert!(served < 2048);
}

#[test]
fn the_bytes_that_fits_counts_are_the_bytes_that_the_step_writer_writes() {
    let mut w = active_world_with(steps_with(900));
    for step in w.replay(2) {
        let parsed = halvo_schema::executable::ExecutableStep::parse(step.to_string().as_bytes())
            .expect("a step");
        assert_eq!(
            crate::serve::written(&parsed),
            Some(parsed.to_json().expect("JSON").len())
        );
        assert_eq!(crate::serve::written(&parsed), Some(bytes(&step)));
    }
}
