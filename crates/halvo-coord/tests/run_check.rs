#[path = "run_support.rs"]
mod support;

use halvo_coord::guard::Table;
use halvo_coord::mem::Call;
use halvo_coord::mem::Method;
use halvo_coord::ports::{PortError, Reaped};
use halvo_coord::run::{recover, Act};
use halvo_coord::schedule::{drive as schedule_drive, Answer, Move};
use halvo_coord::types::{
    Answered, Cause, CheckAnswer, Doing, ItemId, Lapsed, OtherLapse, Outcome, OwnItem, OwnItemKind,
    Raised, ReadyAnswer, RunId, What,
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

/// The open `ready` item of agent `n`.
fn ready_item(w: &World, n: usize) -> ItemId {
    match &w.facts(n).doing {
        Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready => o.item.clone(),
        other => panic!("no open ready item: {other:?}"),
    }
}

/// The owner's `item_lapsed` rows of agent `n`, with the item and the lapse.
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

/// The owner's `slot_skipped` rows of agent `n`.
fn skips(w: &World, n: usize) -> usize {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n) && matches!(e.what, What::SlotSkipped { .. }))
        .count()
}

/// Agent `n` needs the person and holds an open `ready` item, and the agent store's newest
/// record of it has `unknown` above 0, which no move has read yet.
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

/// The person's answer to the check of agent `n`, through the schedule's drive.
fn answer(w: &mut World, n: usize, answer: CheckAnswer) -> Result<Answer, Cause> {
    let mv = Move::Check {
        agent: agent(n),
        answer,
    };
    schedule_drive(&w.ports, &mut w.board, &w.limits, mv).answer
}

/// The logs of agent `n` that the owner acknowledged from the tried call `from` on.
fn bodies(w: &World, n: usize, from: usize) -> Vec<Vec<halvo_coord::types::Event>> {
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

/// The person's answer to a check is a move that reads the hold `check`: when it finds an open
/// `ready` item and no item `check`, it lapses the `ready` item with the cause `check`, raises the
/// check and answers it, in one body, and the agent then holds no item (I5.4, rulings RD-5 and
/// RD-38). Turned round from `the_answer_to_a_check_keeps_an_open_ready_item_open`.
#[test]
fn the_answer_to_a_check_lapses_an_open_ready_item_and_answers_its_check_in_one_body() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let from = w.ports.tried.borrow().len();
    assert_eq!(answer(&mut w, 0, CheckAnswer::TryAgain), Ok(Answer::Ok));
    assert_eq!(w.facts(0).doing, Doing::Idle(None), "no item stays open");
    let logged = bodies(&w, 0, from);
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

/// The answer `went_through` over an open `ready` item stores the same body with the
/// `slot_skipped` of the checked run's slot last, and that skip decides the next tick (I5.4): the
/// checked run names no slot, so no `ready` item is raised again that day (I5.3).
#[test]
fn the_answer_went_through_over_an_open_ready_item_stores_its_slot_skipped_last() {
    let mut w = world(&[PERSON]);
    ready_beside_an_unread_check(&mut w, 0);
    let from = w.ports.tried.borrow().len();
    assert_eq!(answer(&mut w, 0, CheckAnswer::WentThrough), Ok(Answer::Ok));
    let logged = bodies(&w, 0, from);
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

/// The answer's body meets `seq` because the owner holds a raise of the check, with its lapse of
/// the `ready` item, whose acknowledgement was lost: the answer reads the owner's events, finds
/// the `ready` item closed and that check open, and answers that check, with no second lapse
/// and no second check (I5.4, I7.2, rulings XB-31 and RD-38).
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

/// The owner stored the answer's own body with a lost acknowledgement: the next answer meets
/// `seq`, its read shows the hold `check` answered, and it is taken with nothing stored again.
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

/// The tick that first reads the record lapses the open `ready` item with the cause `check` and
/// raises the item `check` in one log, the lapse first (I5.4, ruling RD-5).
#[test]
fn one_body_lapses_the_ready_item_before_it_raises_the_check() {
    let mut w = world(&[PERSON]);
    let ready = ready_beside_an_unread_check(&mut w, 0);
    let from = w.ports.tried.borrow().len();
    w.act(Act::Tick);
    let logged = bodies(&w, 0, from);
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

/// After a refusal `seq` the move reads the open item anew from the owner's events: a skip of the
/// `ready` item that the owner stored with a lost acknowledgement closed it, so the body that the
/// move writes once more raises the check alone and lapses nothing (I5.4, ruling RD-5).
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

/// The owner stored the move's body, the lapse and the raise, with a lost acknowledgement: the
/// read after the refusal `seq` shows the `ready` item closed and the check open, and the board
/// holds that check with nothing written again.
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

/// Sets what agent `n` is doing on the board, as the move that opened or closed its item left it.
fn set_doing(w: &mut World, n: usize, doing: Doing) {
    let member = w
        .board
        .members
        .iter_mut()
        .find(|m| m.facts.agent == agent(n));
    member.expect("a member").facts.doing = doing;
}

/// An agent that holds an open `setup` item gets no check, and its item no lapse: the check waits
/// until that item is closed, and the next move after it raises the check (I5.4).
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

/// The lapse stores no `slot_skipped`: after the answer `try_again` the next tick raises `ready`
/// again (I5.4, ruling RD-5).
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

/// Stores, as the owner's events of agent `n` with a lost acknowledgement, the raise of a check
/// with its lapse of the `ready` item: the body of a move whose acknowledgement was lost.
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

/// Stores, as the owner's events of agent `n` with a lost acknowledgement, a person's start over
/// the `ready` item: its `item_answered` with `start` and the `run_started` of `run`.
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

/// Whether the owner holds a `run_ended` of `run`, or the board owes one.
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

/// The check's body meets `seq` because the owner holds a person's start over the `ready` item
/// whose acknowledgement was lost. The read shows the `ready` item closed by that start and brings
/// a `run_started` that no runner took up: the move closes that start, `run_ended` with
/// `interrupted` and its `end_run`, as every read after a refusal `seq` does (ruling RD-34 (b)
/// (5)). The agent's newest run is then that start, which the agent store has not ended, so the
/// hold `check` does not stand and no check is raised in that move (I5.4).
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

/// The answer's body meets `seq` because the owner holds a person's start over the `ready` item
/// whose acknowledgement was lost: the answer's read closes that start (ruling RD-34 (b) (5)), and
/// the hold `check` then no longer stands, so the answer is `state` with nothing more stored.
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

/// A skip of the `ready` item meets `seq` because the owner holds the check's body with its lapse
/// of that item, stored with a lost acknowledgement: the skip pressed a lapsed card, so it is
/// answered `stale` and stores no `slot_skipped` (I5.3, ruling RD-5), and the board holds the
/// check that the read showed, so the next tick raises no second check.
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

/// A skip that the owner stored with a lost acknowledgement is taken when it is pressed again:
/// the read shows the `ready` item answered with `skip`, and nothing is stored twice.
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

/// A switch-off's `hold_set` meets `seq` because the owner holds the check's body with its lapse
/// of the `ready` item, stored with a lost acknowledgement: the read takes the `ready` item off
/// the board and takes up the check, so the next tick raises no second check.
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

/// The owner stored the answer `try_again` with a lost acknowledgement, and the person then
/// answers `went_through`: the read shows the check answered otherwise, so the second answer is
/// `stale` and nothing of it is stored (I5.4, I7.2, ruling RD-38).
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

/// The answer to the check of an agent whose workflow the owner's list no longer holds is `state`
/// and sends nothing, as the model's move `check` answers for a gone agent (I5.7).
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

/// A start reads the hold `check` of its own agent alone, as the model's `start` does: another
/// agent whose check stands beside its open `ready` item, after a move whose raise failed, keeps
/// that item until a move that reads its record.
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

/// The end of one agent's run reads the hold `check` of every agent, as the model's `end` does:
/// it lapses another agent's open `ready` item with the cause `check` and raises its check, in
/// one body, the lapse first (I5.4, ruling RD-5).
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
    let logged = bodies(&w, 0, from);
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

/// The board's events of the agent after the raise are the owner's, the stored body among them,
/// so that the agent's next log follows it with no refusal `seq` (I7.2).
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
