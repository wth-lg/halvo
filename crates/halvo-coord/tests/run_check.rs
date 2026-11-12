#[path = "run_support.rs"]
mod support;

use halvo_coord::guard::Table;
use halvo_coord::mem::Method;
use halvo_coord::ports::{PortError, Reaped};
use halvo_coord::run::{recover, Act};
use halvo_coord::schedule::{drive as schedule_drive, Answer, Move};
use halvo_coord::types::{Cause, CheckAnswer, Doing, Outcome, OwnItemKind, Raised, RunId, What};
use support::{agent, world, World, A, DAY, MANUAL, T0};

const SLOTTED: A = A {
    person: false,
    slotted: true,
};
const PERSON: A = A {
    person: true,
    slotted: true,
};

fn ended_unknown(w: &mut World, n: usize) -> RunId {
    let done = w.act(Act::Start { agent: agent(n) });
    let run = done.report.run.unwrap();
    w.act(Act::End {
        run: run.clone(),
        outcome: None,
    });
    run
}

fn check_items(w: &World, n: usize) -> usize {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| {
            e.agent == agent(n)
                && matches!(
                    e.what,
                    What::ItemRaised {
                        raised: Raised::Check(None),
                        ..
                    }
                )
        })
        .count()
}

fn holds_check_item(w: &World, n: usize) -> bool {
    matches!(&w.facts(n).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Check)
}

#[test]
fn refusal_check_raised() {
    let mut w = world(&[SLOTTED, PERSON]);
    let a = ended_unknown(&mut w, 0);
    let b = ended_unknown(&mut w, 1);
    let before = w.act(Act::Start { agent: agent(0) });
    assert_eq!(before.report.answer, Err(Cause::State));
    assert!(before.checked.is_empty(), "the store has not ended the run");
    w.store_end(0, &a, Outcome::Failed, 1);
    w.store_end(1, &b, Outcome::Failed, 1);
    w.forget();
    w.set_clock(T0 + 2000, DAY);
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0), agent(1)]);
    assert_eq!((check_items(&w, 0), check_items(&w, 1)), (1, 1));
    assert!(holds_check_item(&w, 0) && holds_check_item(&w, 1));
    assert!(tick.report.started.is_empty() && tick.report.raised.is_empty());
    assert!(
        !w.calls().iter().any(|c| c == "start" || c == "start_run"),
        "no start in that move"
    );
    w.set_clock(T0 + 3000, DAY);
    w.act(Act::Tick);
    assert_eq!(check_items(&w, 0), 1, "raised once");
}

#[test]
fn a_start_that_first_reads_the_record_raises_the_check_and_starts_nothing() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Err(Cause::Hold));
    assert_eq!(start.checked, vec![agent(0)]);
    assert!(holds_check_item(&w, 0));
}

#[test]
fn the_answer_to_the_check_closes_its_item_and_ends_the_hold() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.act(Act::Tick);
    assert!(holds_check_item(&w, 0));
    let answered = schedule_drive(
        &w.ports,
        &mut w.board,
        &w.limits,
        Move::Check {
            agent: agent(0),
            answer: CheckAnswer::TryAgain,
        },
    );
    assert_eq!(answered.answer, Ok(Answer::Ok));
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    w.act(Act::Tick);
    assert_eq!(check_items(&w, 0), 1, "no second check for an answered run");
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok)
    );
}

#[test]
fn no_check_item_for_an_agent_with_an_open_item_or_a_failed_store() {
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    assert!(matches!(&w.facts(0).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready));
    let run = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    w.store_end(0, &run, Outcome::EffectUnknown, 1);
    let tick = w.act(Act::Tick);
    assert!(tick.checked.is_empty(), "the agent holds its ready item");
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.ports.refuse(Method::Log, PortError::NoService);
    assert!(w.act(Act::Tick).checked.is_empty());
    w.ports.clear_refusals();
    assert_eq!(
        w.act(Act::Tick).checked,
        vec![agent(0)],
        "raised at the next move"
    );
}

#[test]
fn recovery_raises_the_check_of_an_earlier_life_again() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.act(Act::Tick);
    assert!(holds_check_item(&w, 0));
    w.ports.new_life();
    w.ports.leave_marks(&[(run, Reaped::Gone)]);
    let life = recover(&w.ports, &w.limits).unwrap();
    w.board = life.board;
    w.table = Table::new();
    assert_eq!(life.checked, vec![agent(0)]);
    assert!(holds_check_item(&w, 0));
    assert_eq!(check_items(&w, 0), 2, "the first lapsed with app_ended");
}
