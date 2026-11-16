#[path = "run_support.rs"]
mod support;

use halvo_coord::mem::Method;
use halvo_coord::ports::PortError;
use halvo_coord::run::Act;
use halvo_coord::schedule::Answer;
use halvo_coord::types::{Limits, Name, Outcome, RunId};
use support::{agent, world_with, World, A};

const SLOTTED: A = A {
    person: false,
    slotted: true,
};

fn refused(name: &str) -> PortError {
    PortError::Refused(Name::parse(name).unwrap())
}

fn started_of(w: &World, n: usize) -> Option<RunId> {
    w.board
        .runs
        .iter()
        .find(|r| r.agent == agent(n))
        .map(|r| r.run.clone())
}

fn count(calls: &[String], name: &str) -> usize {
    calls.iter().filter(|c| *c == name).count()
}

#[test]
fn a_start_refused_with_store_asks_for_no_further_defer_over_100_ticks() {
    let mut w = world_with(&[SLOTTED, SLOTTED], Limits::PRODUCT);
    w.ports
        .refuse_agent(Method::Log, &agent(0), refused("store"));
    let first = w.act(Act::Tick);
    let answers: Vec<_> = first
        .report
        .started
        .iter()
        .map(|s| (s.agent.clone(), s.answer))
        .collect();
    assert_eq!(
        answers,
        vec![
            (agent(0), Err(halvo_coord::types::Cause::NoService)),
            (agent(1), Ok(Answer::Ok))
        ],
        "the second agent starts"
    );
    let run = started_of(&w, 1).expect("the second agent's run");
    w.act(Act::End {
        run,
        outcome: Some(Outcome::Completed),
    });
    for _ in 0..100 {
        w.act(Act::Tick);
    }
    let calls = w.calls();
    assert_eq!(
        count(&calls, "defer"),
        2,
        "one defer for each agent's first start, and none for the 100 ticks after"
    );
    assert!(
        count(&calls, "log run_started refused") >= 100,
        "the start is tried again at each tick"
    );
}

#[test]
fn the_start_after_an_acknowledged_store_asks_for_its_defer_after_its_run_started() {
    let mut w = world_with(&[SLOTTED], Limits::PRODUCT);
    w.ports
        .refuse_agent(Method::Log, &agent(0), refused("store"));
    w.act(Act::Tick);
    w.act(Act::Tick);
    w.ports.clear_refusals();
    w.calls();
    w.act(Act::Tick);
    let calls: Vec<String> = w
        .calls()
        .into_iter()
        .filter(|c| c == "defer" || c.starts_with("log") || c == "start_run")
        .collect();
    assert_eq!(
        calls,
        vec!["log run_started", "defer", "start_run"],
        "the start stores its run_started first and asks for the defer after the acknowledgement"
    );
}

#[test]
fn a_start_refused_with_seq_still_asks_for_its_defer_first() {
    let mut w = world_with(&[SLOTTED], Limits::PRODUCT);
    w.ports.refuse_agent(Method::Log, &agent(0), refused("seq"));
    w.act(Act::Tick);
    w.act(Act::Tick);
    assert_eq!(
        count(&w.calls(), "defer"),
        2,
        "a refusal seq goes to the read"
    );
}
