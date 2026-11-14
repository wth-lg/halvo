#[path = "run_support.rs"]
mod support;

use halvo_coord::guard::Table;
use halvo_coord::mem::Call;
use halvo_coord::mem::Method;
use halvo_coord::ports::{Analysis, Clock, Owner, PortError, Reaped, Rng, Runs};
use halvo_coord::run::{recover, Act};
use halvo_coord::schedule::{drive as schedule_drive, holds, Answer, Hold, Move, Owing};
use halvo_coord::types::{
    Agent, AgentId, Answered, Body, Cause, CheckAnswer, Decision, Doing, Event, HandbackResult,
    ItemId, Lapsed, OtherLapse, Outcome, OwnItem, OwnItemKind, Raised, ReadyAnswer, Reply, RunId,
    RunSpec, What, Workflow,
};
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

fn ready_item(w: &World, n: usize) -> ItemId {
    match &w.facts(n).doing {
        Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready => o.item.clone(),
        other => panic!("no open ready item: {other:?}"),
    }
}

fn lapses(w: &World, n: usize) -> Vec<(ItemId, Lapsed)> {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n))
        .filter_map(|e| match &e.what {
            What::ItemLapsed { item, lapsed } => Some((item.clone(), *lapsed)),
            _ => None,
        })
        .collect()
}

fn skips(w: &World, n: usize) -> usize {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n) && matches!(e.what, What::SlotSkipped { .. }))
        .count()
}

fn ready_beside_an_unread_check(w: &mut World, n: usize) -> ItemId {
    w.act(Act::Tick);
    let item = ready_item(w, n);
    let run = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    w.store_end(n, &run, Outcome::EffectUnknown, 1);
    item
}

#[test]
fn a_check_lapses_an_open_ready_item_and_waits_for_a_failed_store() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0)], "the tick reads the record");
    assert!(holds_check_item(&w, 0));
    assert_eq!(
        lapses(&w, 0),
        vec![(ready, Lapsed::Ready(OtherLapse::Check))],
        "the ready item lapses with the cause check (I7.1, ruling RD-5)"
    );
    assert_eq!(check_items(&w, 0), 1);
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

fn answer(w: &mut World, n: usize, answer: CheckAnswer) -> Result<Answer, Cause> {
    let mv = Move::Check {
        agent: agent(n),
        answer,
    };
    schedule_drive(&w.ports, &mut w.board, &w.limits, mv).answer
}

fn acknowledged_bodies_from(
    w: &World,
    n: usize,
    from: usize,
) -> Vec<Vec<halvo_coord::types::Event>> {
    w.ports.tried.borrow()[from..]
        .iter()
        .filter_map(|t| match (&t.call, &t.refused) {
            (Call::Log(events), None) if events.iter().any(|e| e.agent == agent(n)) => {
                Some(events.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_answer_to_a_check_lapses_an_open_ready_item_and_answers_its_check_in_one_body() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let from = w.ports.tried.borrow().len();
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Ok(Answer::Ok));
    assert_eq!(w.facts(0).doing, Doing::Idle(None), "no item stays open");
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(logged.len(), 1, "one body: {logged:?}");
    let checked = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    match logged[0].iter().map(|e| &e.what).collect::<Vec<_>>()[..] {
        [What::ItemLapsed {
            item: lapsed,
            lapsed: Lapsed::Ready(OtherLapse::Check),
        }, What::ItemRaised {
            item: raised,
            raised: Raised::Check(None),
        }, What::ItemAnswered {
            item: answered,
            answer: Answered::Check(CheckAnswer::TryAgain),
        }, What::HoldCleared {
            cause:
                halvo_coord::types::Cleared::Check {
                    run,
                    answer: CheckAnswer::TryAgain,
                },
        }] => {
            assert_eq!(lapsed, &ready);
            assert_eq!(
                raised, answered,
                "the check that the body raises is the one it answers"
            );
            assert_eq!(run, &checked);
        }
        ref other => panic!("the body: {other:?}"),
    }
    assert_eq!(skips(&w, 0), 0);
    w.set_clock(T0 + 1_000, DAY);
    assert_eq!(
        w.act(Act::Tick).report.raised,
        vec![agent(0)],
        "after try_again the next tick raises ready again"
    );
}

#[test]
fn the_answer_went_through_over_an_open_ready_item_stores_its_slot_skipped_last() {
    let mut w = world(&[PERSON]);
    ready_beside_an_unread_check(&mut w, 0);
    let from = w.ports.tried.borrow().len();
    assert_eq!(answer(&mut w, 0, CheckAnswer::WentThrough), Ok(Answer::Ok));
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert_eq!(logged[0].len(), 5, "{logged:?}");
    assert!(matches!(logged[0][0].what, What::ItemLapsed { .. }));
    assert!(matches!(
        logged[0][4].what,
        What::SlotSkipped { slot: None }
    ));
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    w.set_clock(T0 + 1_000, DAY);
    assert!(w.act(Act::Tick).report.raised.is_empty());
}

#[test]
fn the_answer_after_a_refusal_seq_answers_the_check_that_the_read_shows() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let check = ItemId::parse(&format!("{:032x}", 0x7501)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemLapsed {
            item: ready.clone(),
            lapsed: Lapsed::Ready(OtherLapse::Check),
        },
    );
    stored_unacknowledged(
        &w,
        0,
        What::ItemRaised {
            item: check.clone(),
            raised: Raised::Check(None),
        },
    );
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Ok(Answer::Ok));
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    assert_eq!(
        lapses(&w, 0),
        vec![(ready, Lapsed::Ready(OtherLapse::Check))]
    );
    assert_eq!(
        check_items(&w, 0),
        1,
        "the check that the read showed, alone"
    );
    let answers: Vec<ItemId> = w
        .ports
        .events
        .borrow()
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemAnswered {
                item,
                answer: Answered::Check(_),
            } => Some(item.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(answers, vec![check]);
}

#[test]
fn an_answer_over_a_ready_item_stored_with_a_lost_acknowledgement_is_taken_once() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let checked = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    let check = ItemId::parse(&format!("{:032x}", 0x7601)).unwrap();
    for what in [
        What::ItemLapsed {
            item: ready.clone(),
            lapsed: Lapsed::Ready(OtherLapse::Check),
        },
        What::ItemRaised {
            item: check.clone(),
            raised: Raised::Check(None),
        },
        What::ItemAnswered {
            item: check,
            answer: Answered::Check(CheckAnswer::TryAgain),
        },
        What::HoldCleared {
            cause: halvo_coord::types::Cleared::Check {
                run: checked,
                answer: CheckAnswer::TryAgain,
            },
        },
    ] {
        stored_unacknowledged(&w, 0, what);
    }
    let before = w.ports.events.borrow().len();
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Ok(Answer::Ok));
    assert_eq!(
        w.ports.events.borrow().len(),
        before,
        "nothing stored again"
    );
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    assert_eq!(lapses(&w, 0).len(), 1);
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
fn an_open_ready_item_that_the_owner_stored_with_a_lost_acknowledgement_lapses_for_the_check() {
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
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0)]);
    assert!(
        holds_check_item(&w, 0),
        "the read shows the ready item open, and the move writes the body once more with its lapse"
    );
    assert_eq!(
        lapses(&w, 0),
        vec![(item, Lapsed::Ready(OtherLapse::Check))]
    );
    assert_eq!(check_items(&w, 0), 1);
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

#[test]
fn one_body_lapses_the_ready_item_before_it_raises_the_check() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let from = w.ports.tried.borrow().len();
    w.act(Act::Tick);
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(
        logged.len(),
        1,
        "one log of the agent in the tick: {logged:?}"
    );
    let body = &logged[0];
    assert_eq!(body.len(), 2, "{body:?}");
    assert!(
        matches!(&body[0].what, What::ItemLapsed { item, lapsed: Lapsed::Ready(OtherLapse::Check) } if *item == ready),
        "{body:?}"
    );
    assert!(
        matches!(
            &body[1].what,
            What::ItemRaised {
                raised: Raised::Check(None),
                ..
            }
        ),
        "{body:?}"
    );
    assert_eq!(body[1].seq.get(), body[0].seq.get() + 1);
    assert!(holds_check_item(&w, 0));
    assert_eq!(skips(&w, 0), 0, "the lapse stores no slot_skipped");
}

#[test]
fn after_a_refusal_seq_the_check_reads_the_ready_item_anew_and_lapses_none_that_a_skip_closed() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    stored_unacknowledged(
        &w,
        0,
        What::ItemAnswered {
            item: ready,
            answer: Answered::Ready(ReadyAnswer::Skip),
        },
    );
    stored_unacknowledged(&w, 0, What::SlotSkipped { slot: None });
    let before = w.ports.tried.borrow().len();
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0)]);
    assert!(holds_check_item(&w, 0));
    assert!(
        lapses(&w, 0).is_empty(),
        "the skip closed the ready item, so no lapse is stored for it"
    );
    assert_eq!(check_items(&w, 0), 1);
    let refused_seq = w.ports.tried.borrow()[before..]
        .iter()
        .filter(|t| matches!(&t.refused, Some(PortError::Refused(n)) if n.as_str() == "seq"))
        .count();
    assert_eq!(refused_seq, 1, "the first body met seq");
}

#[test]
fn a_lapse_body_that_the_owner_stored_with_a_lost_acknowledgement_is_held_and_not_written_again() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let check = ItemId::parse(&format!("{:032x}", 0x7301)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemLapsed {
            item: ready.clone(),
            lapsed: Lapsed::Ready(OtherLapse::Check),
        },
    );
    stored_unacknowledged(
        &w,
        0,
        What::ItemRaised {
            item: check.clone(),
            raised: Raised::Check(None),
        },
    );
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0)]);
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(Some(OwnItem {
            item: check,
            kind: OwnItemKind::Check,
        }))
    );
    assert_eq!(
        lapses(&w, 0),
        vec![(ready, Lapsed::Ready(OtherLapse::Check))],
        "one lapse"
    );
    assert_eq!(check_items(&w, 0), 1, "one check");
}

fn set_doing(w: &mut World, n: usize, doing: Doing) {
    let member = w
        .board
        .members
        .iter_mut()
        .find(|m| m.facts.agent == agent(n));
    member.expect("a member").facts.doing = doing;
}

#[test]
fn the_check_waits_behind_an_open_setup_item() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    let setup = OwnItem {
        item: ItemId::parse(&format!("{:032x}", 0x7401)).unwrap(),
        kind: OwnItemKind::Setup,
    };
    set_doing(&mut w, 0, Doing::Idle(Some(setup.clone())));
    assert!(w.act(Act::Tick).checked.is_empty());
    assert_eq!(w.facts(0).doing, Doing::Idle(Some(setup)));
    assert_eq!((check_items(&w, 0), lapses(&w, 0).len()), (0, 0));
    set_doing(&mut w, 0, Doing::Idle(None));
    w.set_clock(T0 + 10_000, DAY);
    assert_eq!(
        w.act(Act::Tick).checked,
        vec![agent(0)],
        "raised once the setup item is closed"
    );
    assert!(lapses(&w, 0).is_empty());
}

#[test]
fn the_lapse_stores_no_slot_skipped_and_after_try_again_the_next_tick_raises_ready_again() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    w.set_clock(T0 + 1_000, DAY);
    assert_eq!(w.act(Act::Tick).checked, vec![agent(0)]);
    assert_eq!(skips(&w, 0), 0);
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
    w.set_clock(T0 + 2_000, DAY);
    let tick = w.act(Act::Tick);
    assert_eq!(tick.report.raised, vec![agent(0)], "ready again");
    assert_ne!(ready_item(&w, 0), ready);
    assert_eq!(
        skips(&w, 0),
        0,
        "neither the lapse nor try_again stores slot_skipped"
    );
}

fn lost_check_body(w: &World, n: usize, ready: &ItemId, check: &ItemId) {
    stored_unacknowledged(
        w,
        n,
        What::ItemLapsed {
            item: ready.clone(),
            lapsed: Lapsed::Ready(OtherLapse::Check),
        },
    );
    stored_unacknowledged(
        w,
        n,
        What::ItemRaised {
            item: check.clone(),
            raised: Raised::Check(None),
        },
    );
}

fn lost_start(w: &World, n: usize, ready: &ItemId, run: &RunId) {
    stored_unacknowledged(
        w,
        n,
        What::ItemAnswered {
            item: ready.clone(),
            answer: Answered::Ready(ReadyAnswer::Start),
        },
    );
    stored_unacknowledged(
        w,
        n,
        What::RunStarted {
            run: run.clone(),
            trigger: halvo_coord::types::Trigger::Person,
            slot: None,
        },
    );
}

fn ended_or_owed(w: &World, run: &RunId) -> bool {
    let stored = w
        .ports
        .events
        .borrow()
        .iter()
        .any(|e| matches!(&e.what, What::RunEnded { run: r, .. } if r == run));
    let owed = w.board.owed.iter().any(
        |o| matches!(&o.what, halvo_coord::schedule::Owing::RunEnded { run: r, .. } if r == run),
    );
    stored || owed
}

#[test]
fn a_lost_start_that_the_checks_read_brings_is_closed() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let lost = RunId::parse(&format!("{:032x}", 0x7a01)).unwrap();
    lost_start(&w, 0, &ready, &lost);
    let tick = w.act(Act::Tick);
    assert!(ended_or_owed(&w, &lost), "the lost start is closed or owed");
    assert!(lapses(&w, 0).is_empty(), "the start closed the ready item");
    assert!(tick.checked.is_empty(), "the hold check no longer stands");
    assert_eq!(check_items(&w, 0), 0);
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
}

#[test]
fn a_lost_start_that_the_answers_read_brings_is_closed() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let lost = RunId::parse(&format!("{:032x}", 0x7a02)).unwrap();
    lost_start(&w, 0, &ready, &lost);
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Err(Cause::State));
    assert!(ended_or_owed(&w, &lost), "the lost start is closed or owed");
    assert!(lapses(&w, 0).is_empty());
    assert_eq!(check_items(&w, 0), 0);
}

#[test]
fn a_skip_whose_read_shows_the_ready_item_lapsed_is_stale_and_the_board_holds_the_check() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let check = ItemId::parse(&format!("{:032x}", 0x7901)).unwrap();
    lost_check_body(&w, 0, &ready, &check);
    let skip = schedule_drive(
        &w.ports,
        &mut w.board,
        &w.limits,
        Move::Skip { agent: agent(0) },
    );
    assert_eq!(skip.answer, Err(Cause::Stale));
    assert_eq!(skips(&w, 0), 0, "no slot_skipped after the lapse");
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(Some(OwnItem {
            item: check,
            kind: OwnItemKind::Check,
        }))
    );
    w.set_clock(T0 + 1_000, DAY);
    w.act(Act::Tick);
    assert_eq!(check_items(&w, 0), 1, "one check item, not two");
}

#[test]
fn a_skip_that_the_owner_stored_with_a_lost_acknowledgement_is_taken_once() {
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    let skip = |w: &mut World| {
        let mv = Move::Skip { agent: agent(0) };
        schedule_drive(&w.ports, &mut w.board, &w.limits, mv).answer
    };
    w.ports.refuse(Method::Log, PortError::NoService);
    assert_eq!(skip(&mut w), Err(Cause::NoService));
    let refused = w
        .ports
        .tried
        .borrow()
        .iter()
        .rev()
        .find_map(|t| match (&t.call, &t.refused) {
            (Call::Log(events), Some(_)) => Some(events.clone()),
            _ => None,
        })
        .expect("the skip's refused log");
    w.ports.events.borrow_mut().extend(refused);
    w.ports.clear_refusals();
    assert_eq!(skip(&mut w), Ok(Answer::Ok));
    assert_eq!(skips(&w, 0), 1);
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
}

#[test]
fn a_switch_off_whose_read_shows_the_check_body_holds_that_check() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let check = ItemId::parse(&format!("{:032x}", 0x7903)).unwrap();
    lost_check_body(&w, 0, &ready, &check);
    w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(Some(OwnItem {
            item: check,
            kind: OwnItemKind::Check,
        }))
    );
    w.set_clock(T0 + 1_000, DAY);
    w.act(Act::Tick);
    assert_eq!(check_items(&w, 0), 1, "one check item, not two");
}

#[test]
fn an_answer_whose_read_shows_the_check_answered_otherwise_is_stale() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.act(Act::Tick);
    let check = match &w.facts(0).doing {
        Doing::Idle(Some(o)) if o.kind == OwnItemKind::Check => o.item.clone(),
        other => panic!("{other:?}"),
    };
    stored_unacknowledged(
        &w,
        0,
        What::ItemAnswered {
            item: check,
            answer: Answered::Check(CheckAnswer::TryAgain),
        },
    );
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
    let before = w.ports.events.borrow().len();
    assert_eq!(
        answer(&mut w, 0, CheckAnswer::WentThrough),
        Err(Cause::Stale)
    );
    assert_eq!(w.ports.events.borrow().len(), before, "nothing stored");
    assert_eq!(skips(&w, 0), 0);
}

#[test]
fn the_answer_to_the_check_of_a_gone_workflow_is_state_and_sends_nothing() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.act(Act::Tick);
    assert!(holds_check_item(&w, 0));
    w.ports.workflows.borrow_mut().clear();
    let from = w.ports.tried.borrow().len();
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Err(Cause::State));
    let sent = w.ports.tried.borrow()[from..]
        .iter()
        .filter(|t| matches!(&t.call, Call::Log(_)))
        .count();
    assert_eq!(sent, 0, "no log of a gone workflow");
}

#[test]
fn a_start_raises_the_check_of_its_own_agent_alone() {
    let mut w = world(&[PERSON, MANUAL]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    w.ports.refuse(Method::Log, PortError::NoService);
    w.set_clock(T0 + 1_000, DAY);
    assert!(w.act(Act::Tick).checked.is_empty(), "the raise failed");
    w.ports.clear_refusals();
    let start = w.act(Act::Start { agent: agent(1) });
    assert!(start.checked.is_empty());
    assert_eq!(ready_item(&w, 0), ready);
    assert_eq!(check_items(&w, 0), 0);
}

#[test]
fn the_end_of_a_run_reads_the_check_of_every_agent() {
    let mut w = world(&[PERSON, MANUAL]);
    w.act(Act::Tick);
    let ready = ready_item(&w, 0);
    let run = w
        .act(Act::Start { agent: agent(1) })
        .report
        .run
        .expect("agent 1 starts");
    let checked = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    w.store_end(0, &checked, Outcome::EffectUnknown, 1);
    let from = w.ports.tried.borrow().len();
    let end = w.act(Act::End { run, outcome: None });
    assert_eq!(end.checked, vec![agent(0)]);
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        matches!(&logged[0][0].what, What::ItemLapsed { item, lapsed: Lapsed::Ready(OtherLapse::Check) } if *item == ready),
        "{logged:?}"
    );
    assert!(matches!(
        logged[0][1].what,
        What::ItemRaised {
            raised: Raised::Check(None),
            ..
        }
    ));
}

#[test]
fn the_board_holds_the_body_that_the_raise_stored() {
    let mut w = world(&[PERSON]);
    ready_beside_an_unread_check(&mut w, 0);
    w.act(Act::Tick);
    let owner: Vec<halvo_coord::types::Event> = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(0))
        .cloned()
        .collect();
    assert_eq!(w.facts(0).events, owner);
    let from = w.ports.tried.borrow().len();
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Ok(Answer::Ok));
    let refused = w.ports.tried.borrow()[from..]
        .iter()
        .filter(|t| t.refused.is_some())
        .count();
    assert_eq!(
        refused, 0,
        "the answer's log follows the body with no refusal"
    );
}

enum Turn {
    Refuse(PortError),
    Lose,
}

struct Late<'a, P> {
    ports: &'a P,
    turns: std::cell::RefCell<std::collections::VecDeque<Turn>>,
}

impl<P: Owner> Owner for Late<'_, P> {
    type Config = P::Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        match self.turns.borrow_mut().pop_front() {
            Some(Turn::Refuse(e)) => Err(e),
            Some(Turn::Lose) => self.ports.log(events).and(Err(PortError::NoService)),
            None => self.ports.log(events),
        }
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        self.ports.history(agent, after)
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.ports.workflows()
    }

    fn agents(&self) -> Result<Vec<Agent<P::Config>>, PortError> {
        self.ports.agents()
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        self.ports.send(body)
    }
}

impl<P: Runs> Runs for Late<'_, P> {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.ports.start(spec)
    }

    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[halvo_coord::types::Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        self.ports.answer_confirm(run, answers, record)
    }

    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        self.ports.answer_handback(run, result)
    }

    fn answer_bind(
        &self,
        run: &RunId,
        result: halvo_coord::types::BindResult,
    ) -> Result<(), PortError> {
        self.ports.answer_bind(run, result)
    }

    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        self.ports.cancel(run)
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.ports.kill(run)
    }

    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        self.ports.reap()
    }
}

impl<P: Analysis> Analysis for Late<'_, P> {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.ports.defer(until)
    }
}

impl<P: Clock> Clock for Late<'_, P> {
    fn now(&self) -> Result<u64, PortError> {
        self.ports.now()
    }

    fn today(&self) -> Result<u32, PortError> {
        self.ports.today()
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.ports.day(at)
    }
}

impl<P: Rng> Rng for Late<'_, P> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.ports.bytes16()
    }
}

fn new_life_returning_checked_agents(w: &mut World, turns: Vec<Turn>) -> Vec<AgentId> {
    w.ports.new_life();
    w.forget();
    let late = Late {
        ports: &w.ports,
        turns: std::cell::RefCell::new(turns.into()),
    };
    let life = recover(&late, &w.limits).expect("recovery");
    w.board = life.board;
    w.table = Table::new();
    life.checked
}

fn owed_ready(w: &World, n: usize) -> Vec<ItemId> {
    w.board
        .owed
        .iter()
        .filter(|o| o.agent == agent(n))
        .filter_map(|o| match &o.what {
            Owing::ItemLapsed {
                item,
                lapsed: Lapsed::Ready(_),
                ..
            } => Some(item.clone()),
            _ => None,
        })
        .collect()
}

fn closes(w: &World, item: &ItemId) -> Vec<What> {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| match &e.what {
            What::ItemLapsed { item: i, .. } | What::ItemAnswered { item: i, .. } => i == item,
            _ => false,
        })
        .map(|e| e.what.clone())
        .collect()
}

fn never_ready_beside_check(w: &World) {
    let events = w.ports.events.borrow().clone();
    let mut life = 0;
    let mut logs: Vec<(usize, Vec<Event>, bool)> = Vec::new();
    for t in w.ports.tried.borrow().iter() {
        match (&t.call, &t.refused) {
            (Call::Reap, _) => life += 1,
            (Call::Log(body), refused) => logs.push((life, body.clone(), refused.is_none())),
            _ => {}
        }
    }
    let raised_in = |item: &ItemId| {
        logs.iter().find_map(|(l, body, _)| {
            body.iter()
                .any(|e| matches!(&e.what, What::ItemRaised { item: i, .. } if i == item))
                .then_some(*l)
        })
    };
    let owed = |item: &ItemId| {
        w.board
            .owed
            .iter()
            .any(|o| matches!(&o.what, Owing::ItemLapsed { item: i, .. } if i == item))
    };
    for (life, body, _) in logs.iter().filter(|(_, _, stored)| *stored) {
        let counts = |item: &ItemId| raised_in(item).is_none_or(|l| l == *life) && !owed(item);
        for e in body {
            let upto: Vec<&Event> = events
                .iter()
                .filter(|x| x.agent == e.agent && x.seq <= e.seq)
                .collect();
            let closed = |item: &ItemId| {
                upto.iter().any(|x| {
                    matches!(&x.what, What::ItemLapsed { item: i, .. } | What::ItemAnswered { item: i, .. } if i == item)
                })
            };
            let open = |kind: fn(&Raised) -> bool| {
                upto.iter().any(|x| {
                    matches!(&x.what, What::ItemRaised { item, raised } if kind(raised) && !closed(item))
                })
            };
            let ready = open(|r| matches!(r, Raised::Ready));
            let check = upto.iter().any(|x| {
                matches!(&x.what, What::ItemRaised { item, raised: Raised::Check(None) } if !closed(item) && counts(item))
            });
            let cleared = matches!(
                &e.what,
                What::HoldCleared {
                    cause: halvo_coord::types::Cleared::Check { .. }
                }
            );
            assert!(
                !(ready && (check || cleared)),
                "after the stored body {body:?} the agent holds an open ready item beside an open check"
            );
        }
    }
}

fn ready_into_a_new_life(w: &mut World, turns: Vec<Turn>) -> (ItemId, Vec<AgentId>) {
    let ready = ready_beside_an_unread_check(w, 0);
    let checked = new_life_returning_checked_agents(w, turns);
    (ready, checked)
}

fn body_is_lapse_then_raise_only(body: &[Event], item: &ItemId) -> bool {
    matches!(
        body.iter().map(|e| &e.what).collect::<Vec<_>>()[..],
        [What::ItemLapsed { item: i, lapsed: Lapsed::Ready(OtherLapse::Check) }, What::ItemRaised { raised: Raised::Check(None), .. }] if i == item
    )
}

#[test]
fn review_seq_3_a_start_lapses_the_owed_ready_item_with_the_check() {
    let mut w = world(&[PERSON]);
    let refused = vec![
        Turn::Refuse(PortError::NoService),
        Turn::Refuse(PortError::NoService),
    ];
    let (ready, checked) = ready_into_a_new_life(&mut w, refused);
    assert!(checked.is_empty(), "recovery's raise was refused too");
    assert_eq!(owed_ready(&w, 0), vec![ready.clone()]);
    w.set_clock(T0 + 1_000, DAY);
    let from = w.ports.tried.borrow().len();
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Err(Cause::Hold));
    assert_eq!(start.checked, vec![agent(0)]);
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        body_is_lapse_then_raise_only(&logged[0], &ready),
        "{logged:?}"
    );
    assert_eq!(owed_ready(&w, 0), vec![ready.clone()], "the debt stays");
    never_ready_beside_check(&w);
    let from = w.ports.tried.borrow().len();
    w.set_clock(T0 + 2_000, DAY);
    w.act(Act::Tick);
    assert!(owed_ready(&w, 0).is_empty(), "the tick drops the debt");
    assert!(
        acknowledged_bodies_from(&w, 0, from)
            .iter()
            .flatten()
            .all(|e| !matches!(
                &e.what,
                What::ItemLapsed { item, .. } if *item == ready
            )),
        "with no send"
    );
    assert_eq!(closes(&w, &ready).len(), 1, "one closing row");
    never_ready_beside_check(&w);
}

#[test]
fn recoverys_own_raise_after_its_refused_body_lapses_the_owed_ready_item() {
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    let ready = ready_item(&w, 0);
    let run = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    w.store_end(0, &run, Outcome::EffectUnknown, 1);
    let from = w.ports.tried.borrow().len();
    let checked =
        new_life_returning_checked_agents(&mut w, vec![Turn::Refuse(PortError::NoService)]);
    assert_eq!(checked, vec![agent(0)]);
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        body_is_lapse_then_raise_only(&logged[0], &ready),
        "{logged:?}"
    );
    assert_eq!(owed_ready(&w, 0), vec![ready]);
    never_ready_beside_check(&w);
}

#[test]
fn a_recovery_body_stored_with_a_lost_acknowledgement_lets_the_check_go_alone() {
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    let ready = ready_item(&w, 0);
    let run = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    w.store_end(0, &run, Outcome::EffectUnknown, 1);
    let checked = new_life_returning_checked_agents(&mut w, vec![Turn::Lose]);
    assert_eq!(checked, vec![agent(0)]);
    assert_eq!(
        closes(&w, &ready),
        vec![What::ItemLapsed {
            item: ready.clone(),
            lapsed: Lapsed::Ready(OtherLapse::AppEnded),
        }]
    );
    assert_eq!(check_items(&w, 0), 1);
    never_ready_beside_check(&w);
}

#[test]
fn a_raise_that_meets_seq_for_another_writer_still_lapses_the_owed_ready_item() {
    let mut w = world(&[PERSON]);
    let refused = vec![
        Turn::Refuse(PortError::NoService),
        Turn::Refuse(PortError::NoService),
    ];
    let (ready, _) = ready_into_a_new_life(&mut w, refused);
    stored_unacknowledged(&w, 0, What::SlotSkipped { slot: None });
    let from = w.ports.tried.borrow().len();
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.checked, vec![agent(0)]);
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        body_is_lapse_then_raise_only(&logged[0], &ready),
        "{logged:?}"
    );
    let refused_seq = w.ports.tried.borrow()[from..]
        .iter()
        .filter(|t| matches!(&t.refused, Some(PortError::Refused(n)) if n.as_str() == "seq"))
        .count();
    assert_eq!(refused_seq, 1);
    never_ready_beside_check(&w);
}

#[test]
fn a_tick_whose_list_does_not_answer_still_lapses_the_owed_ready_item() {
    let mut w = world(&[PERSON]);
    let refused = vec![
        Turn::Refuse(PortError::NoService),
        Turn::Refuse(PortError::NoService),
    ];
    let (ready, _) = ready_into_a_new_life(&mut w, refused);
    w.ports.refuse(Method::Workflows, PortError::NoService);
    w.set_clock(T0 + 1_000, DAY);
    let from = w.ports.tried.borrow().len();
    let tick = w.act(Act::Tick);
    w.ports.clear_refusals();
    assert_eq!(tick.checked, vec![agent(0)]);
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        body_is_lapse_then_raise_only(&logged[0], &ready),
        "{logged:?}"
    );
    never_ready_beside_check(&w);
}

#[test]
fn the_answer_over_an_owed_ready_item_lapses_it_in_its_body() {
    for (answer_is, stored) in [
        (CheckAnswer::TryAgain, None),
        (CheckAnswer::WentThrough, None),
        (CheckAnswer::TryAgain, Some(true)),
        (CheckAnswer::WentThrough, Some(false)),
    ] {
        let mut w = world(&[PERSON]);
        let refused = vec![
            Turn::Refuse(PortError::NoService),
            Turn::Refuse(PortError::NoService),
        ];
        let (ready, _) = ready_into_a_new_life(&mut w, refused);
        match stored {
            Some(true) => stored_unacknowledged(
                &w,
                0,
                What::ItemLapsed {
                    item: ready.clone(),
                    lapsed: Lapsed::Ready(OtherLapse::AppEnded),
                },
            ),
            Some(false) => stored_unacknowledged(&w, 0, What::SlotSkipped { slot: None }),
            None => {}
        }
        let late = stored == Some(true);
        let from = w.ports.tried.borrow().len();
        assert_eq!(answer(&mut w, 0, answer_is), Ok(Answer::Ok), "{answer_is}");
        let logged = acknowledged_bodies_from(&w, 0, from);
        assert_eq!(logged.len(), 1, "{answer_is} {stored:?}: {logged:?}");
        let whats: Vec<&What> = logged[0].iter().map(|e| &e.what).collect();
        if late {
            assert!(
                whats.iter().all(|w| !matches!(w, What::ItemLapsed { .. })),
                "no second lapse: {whats:?}"
            );
        } else {
            assert!(
                matches!(whats[0], What::ItemLapsed { item, lapsed: Lapsed::Ready(OtherLapse::Check) } if *item == ready),
                "{answer_is}: {whats:?}"
            );
            assert!(
                matches!(
                    whats[1],
                    What::ItemRaised {
                        raised: Raised::Check(None),
                        ..
                    }
                ),
                "{whats:?}"
            );
            let last = whats.last().unwrap();
            assert_eq!(
                matches!(last, What::SlotSkipped { .. }),
                answer_is == CheckAnswer::WentThrough,
                "{whats:?}"
            );
        }
        assert_eq!(closes(&w, &ready).len(), 1, "{answer_is} {stored:?}");
        never_ready_beside_check(&w);
    }
}

#[test]
fn of_two_agents_only_the_one_that_owes_lapses_its_ready_item() {
    let mut w = world(&[PERSON, PERSON]);
    w.act(Act::Tick);
    let first = ready_item(&w, 0);
    for n in 0..2 {
        let run = RunId::parse(&format!("{:032x}", 0x0100 + n)).unwrap();
        w.store_end(n, &run, Outcome::EffectUnknown, 1);
    }
    let from = w.ports.tried.borrow().len();
    let checked =
        new_life_returning_checked_agents(&mut w, vec![Turn::Refuse(PortError::NoService)]);
    assert_eq!(checked, vec![agent(0), agent(1)]);
    let of_first = acknowledged_bodies_from(&w, 0, from);
    assert!(
        body_is_lapse_then_raise_only(of_first.last().unwrap(), &first),
        "{of_first:?}"
    );
    let of_second = acknowledged_bodies_from(&w, 1, from);
    let raise = of_second.last().unwrap();
    assert!(
        matches!(
            &raise[..],
            [Event {
                what: What::ItemRaised {
                    raised: Raised::Check(None),
                    ..
                },
                ..
            }]
        ),
        "{raise:?}"
    );
    assert!(owed_ready(&w, 1).is_empty());
    never_ready_beside_check(&w);
}

#[test]
fn the_raise_writes_no_owed_lapse_of_a_check_item() {
    let mut w = world(&[MANUAL]);
    let run = ended_unknown(&mut w, 0);
    w.store_end(0, &run, Outcome::Failed, 1);
    w.act(Act::Tick);
    let earlier = match &w.facts(0).doing {
        Doing::Idle(Some(o)) if o.kind == OwnItemKind::Check => o.item.clone(),
        other => panic!("{other:?}"),
    };
    let from = w.ports.tried.borrow().len();
    let checked =
        new_life_returning_checked_agents(&mut w, vec![Turn::Refuse(PortError::NoService)]);
    assert_eq!(checked, vec![agent(0)]);
    let logged = acknowledged_bodies_from(&w, 0, from);
    let raise = logged.last().unwrap();
    assert!(
        raise
            .iter()
            .all(|e| !matches!(&e.what, What::ItemLapsed { item, .. } if *item == earlier)),
        "{raise:?}"
    );
    assert!(w.board.owed.iter().any(|o| matches!(
        &o.what,
        Owing::ItemLapsed { item, lapsed: Lapsed::Check(_), .. } if *item == earlier
    )));
}

fn with_a_lost_acknowledgement(w: &mut World, act: Act) -> (halvo_coord::run::Done, Vec<Event>) {
    let from = w.ports.tried.borrow().len();
    w.ports.refuse(Method::Log, PortError::NoService);
    let done = w.act(act);
    w.ports.clear_refusals();
    let refused: Vec<Vec<Event>> = w.ports.tried.borrow()[from..]
        .iter()
        .filter_map(|t| match (&t.call, &t.refused) {
            (Call::Log(events), Some(_)) => Some(events.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 1, "one body: {refused:?}");
    w.ports.events.borrow_mut().extend(refused[0].clone());
    (done, refused[0].clone())
}

fn ready_raised_with_a_lost_acknowledgement(w: &mut World) -> ItemId {
    let (_, body) = with_a_lost_acknowledgement(w, Act::Tick);
    let item = match &body[..] {
        [Event {
            what:
                What::ItemRaised {
                    item,
                    raised: Raised::Ready,
                },
            ..
        }] => item.clone(),
        other => panic!("the tick's body: {other:?}"),
    };
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    item
}

fn whats(bodies: &[Vec<Event>]) -> Vec<Vec<What>> {
    bodies
        .iter()
        .map(|b| b.iter().map(|e| e.what.clone()).collect())
        .collect()
}

fn answered_start(item: &ItemId) -> What {
    What::ItemAnswered {
        item: item.clone(),
        answer: Answered::Ready(ReadyAnswer::Start),
    }
}

fn open_ready(w: &World, n: usize) -> Vec<ItemId> {
    let events = w.ports.events.borrow();
    events
        .iter()
        .filter(|e| e.agent == agent(n))
        .filter_map(|e| match &e.what {
            What::ItemRaised {
                item,
                raised: Raised::Ready,
            } if !events.iter().any(|x| {
                matches!(&x.what, What::ItemLapsed { item: i, .. } | What::ItemAnswered { item: i, .. } if i == item)
            }) =>
            {
                Some(item.clone())
            }
            _ => None,
        })
        .collect()
}

fn answers_then_starts(bodies: &[Vec<Event>], item: &ItemId, run: &RunId) -> bool {
    matches!(
        &whats(bodies)[..],
        [body] if matches!(
            &body[..],
            [answer, What::RunStarted { run: r, .. }] if *answer == answered_start(item) && r == run
        )
    )
}

#[test]
fn a_start_whose_read_brings_an_open_ready_item_answers_it_first_in_its_body() {
    let mut w = world(&[PERSON]);
    let ready = ready_raised_with_a_lost_acknowledgement(&mut w);
    let from = w.ports.tried.borrow().len();
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Ok(Answer::Ok));
    let run = start.report.run.clone().unwrap();
    let bodies = acknowledged_bodies_from(&w, 0, from);
    assert!(
        answers_then_starts(&bodies, &ready, &run),
        "{:?}",
        whats(&bodies)
    );
    assert_eq!(closes(&w, &ready), vec![answered_start(&ready)]);
    w.act(Act::End {
        run: run.clone(),
        outcome: None,
    });
    w.store_end(0, &run, Outcome::EffectUnknown, 1);
    w.set_clock(T0 + 10_000, DAY);
    let from = w.ports.tried.borrow().len();
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0)]);
    let raised = whats(&acknowledged_bodies_from(&w, 0, from));
    assert!(
        matches!(
            &raised[..],
            [body] if matches!(&body[..], [What::ItemRaised { raised: Raised::Check(None), .. }])
        ),
        "{raised:?}"
    );
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Ok(Answer::Ok));
    assert_eq!(closes(&w, &ready), vec![answered_start(&ready)]);
    never_ready_beside_check(&w);
}

#[test]
fn after_a_start_that_answered_the_ready_item_its_read_brought_the_next_slot_raises_one_ready_item()
{
    let mut w = world(&[PERSON]);
    let ready = ready_raised_with_a_lost_acknowledgement(&mut w);
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Ok(Answer::Ok));
    let run = start.report.run.clone().unwrap();
    w.act(Act::End {
        run: run.clone(),
        outcome: Some(Outcome::Completed),
    });
    w.store_end(0, &run, Outcome::Completed, 0);
    w.set_clock(T0 + 2 * 86_400_000, DAY + 2);
    let from = w.ports.tried.borrow().len();
    let tick = w.act(Act::Tick);
    assert!(tick.checked.is_empty());
    let raised = whats(&acknowledged_bodies_from(&w, 0, from));
    let new = match &raised[..] {
        [body] => match &body[..] {
            [What::ItemRaised {
                item,
                raised: Raised::Ready,
            }] => item.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert_eq!(closes(&w, &ready), vec![answered_start(&ready)]);
    assert_eq!(open_ready(&w, 0), vec![new], "one ready item is open");
}

#[test]
fn a_start_that_leaves_the_queue_answers_the_ready_item_that_its_read_brings() {
    let limits = halvo_coord::types::Limits {
        max_live_runs: 1,
        ..halvo_coord::types::Limits::PRODUCT
    };
    let mut w = support::world_with(&[PERSON, MANUAL], limits);
    let ready = ready_raised_with_a_lost_acknowledgement(&mut w);
    let other = w.act(Act::Start { agent: agent(1) });
    assert_eq!(other.report.answer, Ok(Answer::Ok));
    let queued = w.act(Act::Start { agent: agent(0) });
    assert_eq!(queued.report.answer, Ok(Answer::Queued));
    assert_eq!(w.facts(0).doing, Doing::Queued(None));
    let from = w.ports.tried.borrow().len();
    w.act(Act::End {
        run: other.report.run.clone().unwrap(),
        outcome: Some(Outcome::Completed),
    });
    let run = w
        .board
        .runs
        .iter()
        .find(|r| r.agent == agent(0))
        .map(|r| r.run.clone())
        .expect("the queue's start is live");
    let bodies = acknowledged_bodies_from(&w, 0, from);
    assert!(
        answers_then_starts(&bodies, &ready, &run),
        "{:?}",
        whats(&bodies)
    );
    assert_eq!(closes(&w, &ready), vec![answered_start(&ready)]);
}

#[test]
fn a_start_whose_read_brings_the_ready_item_closes_it_though_step_7_or_8_fails() {
    for step in [Method::StartRun, Method::Start] {
        let mut w = world(&[PERSON]);
        let ready = ready_raised_with_a_lost_acknowledgement(&mut w);
        w.ports.refuse(step, PortError::NoService);
        let start = w.act(Act::Start { agent: agent(0) });
        w.ports.clear_refusals();
        assert_eq!(start.report.answer, Err(Cause::NoService), "{step:?}");
        assert_eq!(
            closes(&w, &ready),
            vec![answered_start(&ready)],
            "{step:?}: the item is closed in the ledger"
        );
        assert_eq!(w.facts(0).doing, Doing::Idle(None), "{step:?}");
    }
}

#[test]
fn a_switch_off_whose_read_brings_an_open_ready_item_stores_hold_set_alone_and_leaves_it_open() {
    let mut w = world(&[PERSON]);
    let ready = ready_raised_with_a_lost_acknowledgement(&mut w);
    let from = w.ports.tried.borrow().len();
    let off = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(off.report.answer, Ok(Answer::Ok));
    assert_eq!(
        whats(&acknowledged_bodies_from(&w, 0, from)),
        vec![vec![What::HoldSet {
            cause: halvo_coord::types::HoldSetCause::Person,
        }]]
    );
    assert!(closes(&w, &ready).is_empty(), "the ready item stays open");
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(Some(OwnItem {
            item: ready,
            kind: OwnItemKind::Ready,
        })),
        "the board holds the item that the read brought"
    );
}

#[test]
fn a_start_whose_log_meets_seq_for_another_writers_event_answers_its_ready_item_once() {
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    let ready = ready_item(&w, 0);
    stored_unacknowledged(
        &w,
        0,
        What::HoldCleared {
            cause: halvo_coord::types::Cleared::Failures,
        },
    );
    let from = w.ports.tried.borrow().len();
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Ok(Answer::Ok));
    let run = start.report.run.clone().unwrap();
    let refused = w.ports.tried.borrow()[from..]
        .iter()
        .filter(|t| matches!((&t.call, &t.refused), (Call::Log(_), Some(PortError::Refused(name))) if name.as_str() == "seq"))
        .count();
    assert_eq!(refused, 1, "the start's log meets seq");
    let bodies = acknowledged_bodies_from(&w, 0, from);
    assert!(
        answers_then_starts(&bodies, &ready, &run),
        "{:?}",
        whats(&bodies)
    );
    assert_eq!(closes(&w, &ready), vec![answered_start(&ready)]);
}

fn act_late(w: &mut World, turns: Vec<Turn>, act: Act) -> halvo_coord::run::Done {
    let late = Late {
        ports: &w.ports,
        turns: std::cell::RefCell::new(turns.into()),
    };
    halvo_coord::run::drive(&late, &mut w.board, &mut w.table, &w.limits, act)
}

#[test]
fn a_tick_may_raise_ready_beside_the_check_item_of_an_earlier_life_whose_lapse_is_owed() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    let earlier = match &w.facts(0).doing {
        Doing::Idle(Some(o)) if o.kind == OwnItemKind::Check => o.item.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(closes(&w, &ready).len(), 1);
    let checked =
        new_life_returning_checked_agents(&mut w, vec![Turn::Refuse(PortError::NoService)]);
    assert_eq!(checked, vec![agent(0)]);
    assert!(w.board.owed.iter().any(|o| matches!(
        &o.what,
        Owing::ItemLapsed { item, lapsed: Lapsed::Check(OtherLapse::AppEnded), .. } if *item == earlier
    )));
    assert_eq!(check_items(&w, 0), 2);
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Ok(Answer::Ok));
    w.set_clock(T0 + 20_000, DAY);
    let from = w.ports.tried.borrow().len();
    act_late(&mut w, vec![Turn::Refuse(PortError::NoService)], Act::Tick);
    let raised = whats(&acknowledged_bodies_from(&w, 0, from));
    assert!(
        matches!(
            &raised[..],
            [body] if matches!(&body[..], [What::ItemRaised { raised: Raised::Ready, .. }])
        ),
        "the tick raises ready: {raised:?}"
    );
    assert!(
        closes(&w, &earlier).is_empty(),
        "the earlier check item is open"
    );
    assert!(
        !holds(w.facts(0)).contains(&Hold::Check),
        "the hold does not stand"
    );
    w.set_clock(T0 + 30_000, DAY);
    w.act(Act::Tick);
    assert_eq!(
        closes(&w, &earlier),
        vec![What::ItemLapsed {
            item: earlier.clone(),
            lapsed: Lapsed::Check(OtherLapse::AppEnded),
        }],
        "after the payment the earlier item has one closing row"
    );
    never_ready_beside_check(&w);
}

#[test]
fn a_switch_off_that_first_reads_the_record_raises_no_check_and_the_next_tick_lapses_and_raises() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let from = w.ports.tried.borrow().len();
    let off = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(off.report.answer, Ok(Answer::Ok));
    assert_eq!(
        whats(&acknowledged_bodies_from(&w, 0, from)),
        vec![vec![What::HoldSet {
            cause: halvo_coord::types::HoldSetCause::Person,
        }]]
    );
    assert!(closes(&w, &ready).is_empty(), "the ready item stays open");
    assert_eq!(check_items(&w, 0), 0);
    w.set_clock(T0 + 10_000, DAY);
    let from = w.ports.tried.borrow().len();
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0)]);
    let logged = acknowledged_bodies_from(&w, 0, from);
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        body_is_lapse_then_raise_only(&logged[0], &ready),
        "{logged:?}"
    );
    never_ready_beside_check(&w);
}

#[test]
fn a_skip_that_first_reads_the_record_gives_the_slot_away_and_the_next_tick_raises_the_check_alone()
{
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let from = w.ports.tried.borrow().len();
    let skip = schedule_drive(
        &w.ports,
        &mut w.board,
        &w.limits,
        Move::Skip { agent: agent(0) },
    );
    assert_eq!(skip.answer, Ok(Answer::Ok));
    let bodies = whats(&acknowledged_bodies_from(&w, 0, from));
    assert!(
        matches!(
            &bodies[..],
            [body] if matches!(
                &body[..],
                [What::ItemAnswered { item, answer: Answered::Ready(ReadyAnswer::Skip) }, What::SlotSkipped { .. }] if *item == ready
            )
        ),
        "{bodies:?}"
    );
    assert_eq!(check_items(&w, 0), 0);
    w.set_clock(T0 + 10_000, DAY);
    let from = w.ports.tried.borrow().len();
    let tick = w.act(Act::Tick);
    assert_eq!(tick.checked, vec![agent(0)]);
    let raised = whats(&acknowledged_bodies_from(&w, 0, from));
    assert!(
        matches!(
            &raised[..],
            [body] if matches!(&body[..], [What::ItemRaised { raised: Raised::Check(None), .. }])
        ),
        "{raised:?}"
    );
    assert_eq!(skips(&w, 0), 1, "the skip's slot_skipped stands");
    never_ready_beside_check(&w);
}

#[test]
fn review28_a_skip_beside_an_earlier_skip_of_another_item_whose_read_shows_the_ready_item_lapsed_is_stale(
) {
    let mut w = world(&[PERSON]);
    let earlier = ItemId::parse(&format!("{:032x}", 0x28e1)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemRaised {
            item: earlier.clone(),
            raised: Raised::Ready,
        },
    );
    stored_unacknowledged(
        &w,
        0,
        What::ItemAnswered {
            item: earlier.clone(),
            answer: Answered::Ready(ReadyAnswer::Skip),
        },
    );
    let ready = ready_beside_an_unread_check(&mut w, 0);
    assert_ne!(ready, earlier);
    let check = ItemId::parse(&format!("{:032x}", 0x28e2)).unwrap();
    lost_check_body(&w, 0, &ready, &check);
    let before = skips(&w, 0);
    let skip = schedule_drive(
        &w.ports,
        &mut w.board,
        &w.limits,
        Move::Skip { agent: agent(0) },
    );
    assert_eq!(
        skip.answer,
        Err(Cause::Stale),
        "a press on the lapsed card is stale"
    );
    assert_eq!(skips(&w, 0), before, "no slot_skipped after the lapse");
}

#[test]
fn a_skip_after_a_queued_start_whose_answer_the_owner_stored_with_a_lost_acknowledgement_is_stale()
{
    let limits = halvo_coord::types::Limits {
        max_live_runs: 1,
        ..halvo_coord::types::Limits::PRODUCT
    };
    let mut w = support::world_with(&[PERSON, MANUAL], limits);
    w.act(Act::Tick);
    let ready = ready_item(&w, 0);
    let other = w.act(Act::Start { agent: agent(1) });
    assert_eq!(other.report.answer, Ok(Answer::Ok));
    let (queued, body) = with_a_lost_acknowledgement(&mut w, Act::Start { agent: agent(0) });
    assert_eq!(queued.report.answer, Ok(Answer::Queued));
    assert_eq!(whats(&[body]), vec![vec![answered_start(&ready)]]);
    assert_eq!(
        w.facts(0).doing,
        Doing::Queued(Some(OwnItem {
            item: ready.clone(),
            kind: OwnItemKind::Ready,
        })),
        "the board keeps the item whose answer it did not see acknowledged"
    );
    let from = w.ports.tried.borrow().len();
    let skip = schedule_drive(
        &w.ports,
        &mut w.board,
        &w.limits,
        Move::Skip { agent: agent(0) },
    );
    assert_eq!(skip.answer, Err(Cause::Stale), "the skip is stale");
    assert!(
        acknowledged_bodies_from(&w, 0, from).is_empty() && skips(&w, 0) == 0,
        "the skip stores nothing"
    );
    assert_eq!(
        w.facts(0).doing,
        Doing::Queued(None),
        "the board holds the queued agent with no item"
    );
    assert!(
        w.board.queue.holds(&agent(0)),
        "the queue still holds the start"
    );
    assert_eq!(
        closes(&w, &ready),
        vec![answered_start(&ready)],
        "the item has one closing row"
    );
}

#[test]
fn no_check_is_raised_while_the_board_holds_the_run_live_though_the_store_recorded_it() {
    let mut w = world(&[MANUAL]);
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Ok(Answer::Ok));
    let run = start.report.run.clone().unwrap();
    w.store_end(0, &run, Outcome::EffectUnknown, 1);
    w.set_clock(T0 + 10_000, DAY);
    let from = w.ports.tried.borrow().len();
    let tick = w.act(Act::Tick);
    assert!(
        tick.checked.is_empty(),
        "nothing is raised while the run is live"
    );
    assert!(acknowledged_bodies_from(&w, 0, from).is_empty());
    assert!(
        w.board.runs.iter().any(|r| r.run == run),
        "the board holds the run live"
    );
    let end = w.act(Act::End {
        run,
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(end.checked, vec![agent(0)], "the end raises the check");
    assert!(holds_check_item(&w, 0));
    assert_eq!(check_items(&w, 0), 1);
}

#[test]
fn a_runs_check_item_that_a_read_brings_is_not_taken_up_as_the_agents_item() {
    let mut w = world(&[MANUAL]);
    let item = ItemId::parse(&format!("{:032x}", 0x7d33)).unwrap();
    let run = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemRaised {
            item,
            raised: Raised::Check(Some(run)),
        },
    );
    let from = w.ports.tried.borrow().len();
    let off = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(off.report.answer, Ok(Answer::Ok));
    let seq = w.ports.tried.borrow()[from..]
        .iter()
        .any(|t| matches!((&t.call, &t.refused), (Call::Log(_), Some(PortError::Refused(name))) if name.as_str() == "seq"));
    assert!(seq, "the switch-off's log meets seq");
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(None),
        "the board takes no item"
    );
}
