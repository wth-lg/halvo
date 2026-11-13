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
fn the_answer_to_a_check_keeps_an_open_ready_item_open() {
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    assert!(matches!(&w.facts(0).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready));
    let before = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    w.store_end(0, &before, Outcome::EffectUnknown, 1);
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
    assert!(
        matches!(&w.facts(0).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready),
        "the ready item stays open: only its answer or its lapse closes it (I7.1)"
    );
}

#[test]
fn no_check_is_sent_for_the_agent_of_a_workflow_that_is_gone() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.ports.workflows.borrow_mut().clear();
    w.act(Act::Tick);
    w.act(Act::Tick);
    let sent = w
        .ports
        .tried
        .borrow()
        .iter()
        .filter(|t| match &t.call {
            halvo_coord::mem::Call::Log(events) => events.iter().any(|e| {
                matches!(
                    e.what,
                    What::ItemRaised {
                        raised: Raised::Check(None),
                        ..
                    }
                )
            }),
            _ => false,
        })
        .count();
    assert_eq!(sent, 0, "no event of a gone workflow is sent");
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

#[test]
fn review_p2_check_item_with_a_lost_acknowledgement() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.ports.refuse(Method::Log, PortError::NoService);
    assert!(w.act(Act::Tick).checked.is_empty());
    let last = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(0))
        .map(|e| e.seq.get())
        .max()
        .unwrap();
    w.ports.events.borrow_mut().push(halvo_coord::types::Event {
        workflow: support::flow(0),
        agent: agent(0),
        seq: std::num::NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what: What::ItemRaised {
            item: halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x77)).unwrap(),
            raised: Raised::Check(None),
        },
    });
    w.ports.clear_refusals();
    let mut log = Vec::new();
    for k in 1..=4u64 {
        w.set_clock(T0 + 10_000 * k, DAY);
        let tick = w.act(Act::Tick);
        log.push(format!("tick {k}: checked {}", tick.checked.len()));
    }
    let start = w.act(Act::Start { agent: agent(0) });
    log.push(format!(
        "start: {:?}, checked {}",
        start.report.answer,
        start.checked.len()
    ));
    let refused_seq = w
        .ports
        .tried
        .borrow()
        .iter()
        .filter(|t| matches!(&t.refused, Some(PortError::Refused(n)) if n.as_str() == "seq"))
        .count();
    log.push(format!("logs refused with seq: {refused_seq}"));
    log.push(format!(
        "agent holds a check item: {}",
        holds_check_item(&w, 0)
    ));
    println!("P2 {}", log.join(" | "));
    assert!(holds_check_item(&w, 0), "P2 {}", log.join(" | "));
    assert_eq!(
        start.report.answer,
        Err(Cause::Hold),
        "a held agent with its card"
    );
}

#[test]
fn review_life_2() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.ports.refuse(Method::Log, PortError::NoService);
    assert!(w.act(Act::Tick).checked.is_empty());
    let last = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(0))
        .map(|e| e.seq.get())
        .max()
        .unwrap();
    let item = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x7002)).unwrap();
    w.ports.events.borrow_mut().push(halvo_coord::types::Event {
        workflow: support::flow(0),
        agent: agent(0),
        seq: std::num::NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what: What::ItemRaised {
            item,
            raised: Raised::Check(None),
        },
    });
    w.ports.clear_refusals();
    for k in 1..3 {
        w.set_clock(T0 + k * 10_000, DAY);
        w.act(Act::Tick);
    }
    let answered = schedule_drive(
        &w.ports,
        &mut w.board,
        &w.limits,
        Move::Check {
            agent: agent(0),
            answer: CheckAnswer::TryAgain,
        },
    );
    assert_eq!(
        answered.answer,
        Ok(Answer::Ok),
        "I5.4: check is ended by the person's answer to the item check"
    );
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok),
        "the agent is not held for good"
    );
}

/// Stores `what` as the owner's next event of agent `n`, as a log of the coordinator whose
/// acknowledgement was lost.
fn stored_unacknowledged(w: &World, n: usize, what: What) {
    let last = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n))
        .map(|e| e.seq.get())
        .max()
        .unwrap_or(0);
    w.ports.events.borrow_mut().push(halvo_coord::types::Event {
        workflow: support::flow(n),
        agent: agent(n),
        seq: std::num::NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what,
    });
}

#[test]
fn a_check_item_that_the_owner_stored_with_a_lost_acknowledgement_is_held_and_not_raised_again() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    let item = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x7201)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemRaised {
            item: item.clone(),
            raised: Raised::Check(None),
        },
    );
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0)], "the check stands");
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(Some(halvo_coord::types::OwnItem {
            item,
            kind: OwnItemKind::Check,
        })),
        "the agent holds the item that the owner stored"
    );
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(check_items(&w, 0), 1, "no second check item");
}

#[test]
fn an_open_ready_item_that_the_owner_stored_with_a_lost_acknowledgement_is_held() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    let item = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x7202)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    );
    w.act(Act::Tick);
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(Some(halvo_coord::types::OwnItem {
            item,
            kind: OwnItemKind::Ready,
        })),
        "the board holds the open item that the owner holds"
    );
}

#[test]
fn no_check_item_after_a_read_that_shows_the_persons_answer_to_the_check() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    stored_unacknowledged(
        &w,
        0,
        What::HoldCleared {
            cause: halvo_coord::types::Cleared::Check {
                run,
                answer: CheckAnswer::TryAgain,
            },
        },
    );
    let tick = w.act(Act::Tick);
    assert!(
        tick.checked.is_empty(),
        "the person answered the check: it is not asked again"
    );
    assert_eq!(check_items(&w, 0), 0);
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok)
    );
}

#[test]
fn an_open_setup_item_that_the_owner_stored_with_a_lost_acknowledgement_is_held() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    let item = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x7203)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemRaised {
            item: item.clone(),
            raised: Raised::Setup,
        },
    );
    w.act(Act::Tick);
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(Some(halvo_coord::types::OwnItem {
            item,
            kind: OwnItemKind::Setup,
        })),
        "the board holds the open setup item that the owner holds"
    );
}

#[test]
fn a_read_whose_later_events_raise_and_close_an_item_takes_up_no_item() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    let item = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x7204)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    );
    stored_unacknowledged(
        &w,
        0,
        What::ItemLapsed {
            item: item.clone(),
            lapsed: halvo_coord::types::Lapsed::Ready(halvo_coord::types::OtherLapse::AppEnded),
        },
    );
    let tick = w.act(Act::Tick);
    assert_eq!(
        tick.checked,
        vec![agent(0)],
        "the check is logged after the read"
    );
    assert!(
        holds_check_item(&w, 0),
        "the board holds the check, not the item that the owner closed"
    );
    assert_eq!(check_items(&w, 0), 1);
}

/// The end of the local day `DAY` in the zone of `world`.
const MIDNIGHT: u64 = T0 + 6_400_000;

/// The `item_raised` row of a check that a start raises, with the clock from 8 ms before local
/// midnight to 3 ms after it and moving on 1 ms at each read, carries the local day of its `at`
/// (I7.1).
#[test]
fn a_raised_check_row_carries_the_day_of_its_at_across_midnight() {
    let mut days = Vec::new();
    for start in MIDNIGHT - 8..MIDNIGHT + 4 {
        let mut w = world(&[MANUAL]);
        let run = ended_unknown(&mut w, 0);
        w.store_end(0, &run, Outcome::Failed, 1);
        w.ports.set_now(start);
        w.ports.step_reads(1);
        let moved = w.act(Act::Start { agent: agent(0) });
        assert_eq!(moved.checked, vec![agent(0)], "from {start}");
        let events = w.ports.events.borrow();
        let raised = events.iter().filter(|e| {
            matches!(
                e.what,
                What::ItemRaised {
                    raised: Raised::Check(None),
                    ..
                }
            )
        });
        for e in raised {
            let at = u64::try_from(e.at / 1_000_000).unwrap();
            let day = halvo_coord::ports::Clock::day(&w.ports, at);
            assert_eq!(day, Ok(e.day), "from {start}");
            days.push(e.day);
        }
    }
    assert_eq!(days.len(), 12);
    assert!(days.contains(&DAY) && days.contains(&(DAY + 1)));
}

/// The rows of the moves that go through the run drive and so through its clock wrapper `Watch`
/// (src/run/drive.rs): a start, a tick and a switch-off, with the clock from 8 ms before local
/// midnight to 3 ms after it and moving on 1 ms at each read. Every row carries the memory
/// clock's day of its `at` (I7.1; ruling RD-69 E1).
#[test]
fn rows_through_the_run_drive_carry_the_day_of_their_at_across_midnight() {
    let mut days = Vec::new();
    let mut take = |w: &World, before: usize, start: u64, least: usize| {
        let events = w.ports.events.borrow();
        assert!(events.len() >= before + least, "rows from {start}");
        for e in &events[before..] {
            let at = u64::try_from(e.at / 1_000_000).unwrap();
            let day = halvo_coord::ports::Clock::day(&w.ports, at);
            assert_eq!(day, Ok(e.day), "{:?} from {start}", e.what);
            days.push(e.day);
        }
    };
    for start in MIDNIGHT - 8..MIDNIGHT + 4 {
        let mut w = world(&[MANUAL]);
        let before = w.ports.events.borrow().len();
        w.ports.set_now(start);
        w.ports.step_reads(1);
        let moved = w.act(Act::Start { agent: agent(0) });
        assert!(moved.report.run.is_some(), "a start from {start}");
        take(&w, before, start, 1);
        let mut w = world(&[SLOTTED]);
        let before = w.ports.events.borrow().len();
        w.ports.set_now(start);
        w.ports.step_reads(1);
        w.act(Act::Tick);
        w.act(Act::SwitchOff { agent: agent(0) });
        take(&w, before, start, 2);
    }
    assert!(days.contains(&DAY) && days.contains(&(DAY + 1)), "{days:?}");
}
