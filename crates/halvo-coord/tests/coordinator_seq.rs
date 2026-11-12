#[path = "run_support.rs"]
mod support;

use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::run::Act;
use halvo_coord::schedule::{drive as schedule_drive, holds, Answer, Hold, Move};
use halvo_coord::types::{
    Answered, CheckAnswer, Cleared, Doing, Event, HoldSetCause, OwnItemKind, Raised, RunId, What,
};
use support::{agent, world, World, A, DAY, MANUAL, T0};

const PERSON: A = A {
    person: true,
    slotted: true,
};

fn owner_events(w: &World, n: usize) -> Vec<Event> {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n))
        .cloned()
        .collect()
}

fn agree(w: &World, n: usize) {
    assert_eq!(
        owner_events(w, n),
        w.facts(n).events,
        "the board's events of the agent are the owner's"
    );
}

/// The owner stored the body that it refused last, and only its acknowledgement was lost.
fn lose_the_acknowledgement(w: &World) {
    let tried = w.ports.tried.borrow().clone();
    let events = tried
        .iter()
        .rev()
        .find_map(|t| match (&t.call, &t.refused) {
            (Call::Log(events), Some(_)) => Some(events.clone()),
            _ => None,
        })
        .expect("a refused log");
    w.ports.events.borrow_mut().extend(events);
    w.ports.clear_refusals();
}

fn count(w: &World, n: usize, f: impl Fn(&What) -> bool) -> usize {
    owner_events(w, n).iter().filter(|e| f(&e.what)).count()
}

fn ended_unknown(w: &mut World, n: usize) -> RunId {
    let done = w.act(Act::Start { agent: agent(n) });
    let run = done.report.run.unwrap();
    w.act(Act::End {
        run: run.clone(),
        outcome: None,
    });
    run
}

#[test]
fn a_lost_acknowledgement_of_the_answer_to_a_check_is_read_back_and_the_agent_starts_again() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, halvo_coord::types::Outcome::Failed, 1);
    w.act(Act::Tick);
    let item = match &w.facts(0).doing {
        Doing::Idle(Some(o)) if o.kind == OwnItemKind::Check => o.item.clone(),
        other => panic!("{other:?}"),
    };
    w.ports.refuse(Method::Log, PortError::NoService);
    let check = Move::Check {
        agent: agent(0),
        answer: CheckAnswer::TryAgain,
    };
    let first = schedule_drive(&w.ports, &mut w.board, &w.limits, check.clone());
    assert_eq!(first.answer, Err(halvo_coord::types::Cause::NoService));
    lose_the_acknowledgement(&w);
    let again = schedule_drive(&w.ports, &mut w.board, &w.limits, check);
    assert_eq!(again.answer, Ok(Answer::Ok));
    assert_eq!(
        count(&w, 0, |what| matches!(
            what,
            What::ItemAnswered {
                item: i,
                answer: Answered::Check(CheckAnswer::TryAgain)
            } if *i == item
        )),
        1
    );
    assert_eq!(
        count(&w, 0, |what| matches!(
            what,
            What::HoldCleared {
                cause: Cleared::Check { .. }
            }
        )),
        1
    );
    for k in 1..4u64 {
        w.set_clock(T0 + 10_000 * k, DAY);
        w.act(Act::Tick);
    }
    assert!(!holds(w.facts(0)).contains(&Hold::Check));
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Ok(Answer::Ok));
    agree(&w, 0);
}

#[test]
fn a_lost_acknowledgement_of_a_ready_item_is_taken_and_not_raised_twice() {
    let mut w = world(&[PERSON]);
    w.ports.refuse(Method::Log, PortError::NoService);
    let first = w.act(Act::Tick);
    assert!(first.report.raised.is_empty());
    lose_the_acknowledgement(&w);
    w.set_clock(T0 + 3_000, DAY);
    w.act(Act::Tick);
    let ready = |what: &What| {
        matches!(
            what,
            What::ItemRaised {
                raised: Raised::Ready,
                ..
            }
        )
    };
    assert_eq!(
        count(&w, 0, ready),
        1,
        "one ready item in the owner's ledger"
    );
    assert!(
        matches!(&w.facts(0).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready),
        "{:?}",
        w.facts(0).doing
    );
    agree(&w, 0);
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Ok(Answer::Ok));
    agree(&w, 0);
}

#[test]
fn a_lost_acknowledgement_of_a_run_started_does_not_refuse_the_next_start() {
    let mut w = world(&[MANUAL]);
    w.ports.refuse(Method::Log, PortError::NoService);
    let first = w.act(Act::Start { agent: agent(0) });
    assert_eq!(
        first.report.answer,
        Err(halvo_coord::types::Cause::NoService)
    );
    lose_the_acknowledgement(&w);
    let again = w.act(Act::Start { agent: agent(0) });
    assert_eq!(again.report.answer, Ok(Answer::Ok));
    assert_eq!(
        count(&w, 0, |what| matches!(what, What::RunStarted { .. })),
        3,
        "the run of the world, the one with the lost acknowledgement and the new one"
    );
    agree(&w, 0);
}

#[test]
fn a_lost_acknowledgement_of_a_hold_is_read_back_and_the_hold_stands() {
    let mut w = world(&[MANUAL]);
    w.ports.refuse(Method::Log, PortError::NoService);
    let first = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(
        first.report.answer,
        Err(halvo_coord::types::Cause::NoService)
    );
    lose_the_acknowledgement(&w);
    let again = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(again.report.answer, Ok(Answer::Ok));
    assert_eq!(
        count(&w, 0, |what| matches!(
            what,
            What::HoldSet {
                cause: HoldSetCause::Person
            }
        )),
        1
    );
    assert!(holds(w.facts(0)).contains(&Hold::Off));
    agree(&w, 0);
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Err(halvo_coord::types::Cause::Hold));
}
