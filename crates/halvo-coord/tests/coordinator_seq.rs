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
    let stored = owner_events(&w, 0)
        .iter()
        .find_map(|e| match &e.what {
            What::ItemRaised {
                item,
                raised: Raised::Ready,
            } => Some(item.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        matches!(&w.facts(0).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready && o.item == stored),
        "the board holds the item that the owner stored: {:?}",
        w.facts(0).doing
    );
    agree(&w, 0);
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Ok(Answer::Ok));
    agree(&w, 0);
}

#[test]
fn a_lost_acknowledgement_of_a_run_started_holds_the_next_start_until_its_run_is_closed() {
    let mut w = world(&[MANUAL]);
    w.ports.refuse(Method::Log, PortError::NoService);
    let first = w.act(Act::Start { agent: agent(0) });
    assert_eq!(
        first.report.answer,
        Err(halvo_coord::types::Cause::NoService)
    );
    lose_the_acknowledgement(&w);
    let lost = owner_events(&w, 0)
        .iter()
        .rev()
        .find_map(|e| match &e.what {
            What::RunStarted { run, .. } => Some(run.clone()),
            _ => None,
        })
        .unwrap();
    let again = w.act(Act::Start { agent: agent(0) });
    assert_eq!(
        again.report.answer,
        Err(halvo_coord::types::Cause::State),
        "step 3 of I5.5: the store has not ended the lost run"
    );
    assert_eq!(
        count(&w, 0, |what| matches!(what, What::RunStarted { .. })),
        2,
        "the run of the world and the one with the lost acknowledgement; no second while it has no end"
    );
    assert_eq!(
        count(&w, 0, |what| matches!(
            what,
            What::RunEnded { run, cause: halvo_coord::types::RunEnd::Interrupted } if *run == lost
        )),
        1
    );
    assert!(w
        .ports
        .tried
        .borrow()
        .iter()
        .any(|t| t.call == Call::Send(halvo_coord::types::Body::EndRun { run: lost.clone() })));
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    let next = w.act(Act::Start { agent: agent(0) });
    assert_eq!(next.report.answer, Ok(Answer::Ok));
    assert_ne!(next.report.run, Some(lost));
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

#[test]
fn a_later_log_that_reads_back_a_closed_item_takes_it_off_the_board() {
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    assert!(
        matches!(&w.facts(0).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready),
        "{:?}",
        w.facts(0).doing
    );
    w.ports.refuse(Method::Log, PortError::NoService);
    let skip = schedule_drive(
        &w.ports,
        &mut w.board,
        &w.limits,
        Move::Skip { agent: agent(0) },
    );
    assert_eq!(skip.answer, Err(halvo_coord::types::Cause::NoService));
    lose_the_acknowledgement(&w);
    assert!(matches!(&w.facts(0).doing, Doing::Idle(Some(_))));
    let off = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(off.report.answer, Ok(Answer::Ok));
    assert_eq!(
        w.facts(0).doing,
        Doing::Idle(None),
        "the skip closed the item"
    );
    agree(&w, 0);
}

#[test]
fn a_ready_item_that_another_writer_raised_and_closed_does_not_stand_for_a_new_one() {
    let mut w = world(&[PERSON]);
    let last = owner_events(&w, 0).last().map_or(0, |e| e.seq.get());
    let old = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x4400)).unwrap();
    let closed = [
        What::ItemRaised {
            item: old.clone(),
            raised: Raised::Ready,
        },
        What::ItemLapsed {
            item: old.clone(),
            lapsed: halvo_coord::types::Lapsed::Ready(halvo_coord::types::OtherLapse::AppEnded),
        },
    ];
    for (k, what) in closed.into_iter().enumerate() {
        w.ports.events.borrow_mut().push(Event {
            workflow: support::flow(0),
            agent: agent(0),
            seq: std::num::NonZeroU32::new(last + 1 + k as u32).unwrap(),
            day: DAY,
            at: 0,
            what,
        });
    }
    let tick = w.act(Act::Tick);
    assert_eq!(tick.report.raised, [agent(0)]);
    let open = match &w.facts(0).doing {
        Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready => o.item.clone(),
        other => panic!("{other:?}"),
    };
    assert_ne!(open, old, "a new ready item, not the closed one");
    assert_eq!(
        count(&w, 0, |what| matches!(
            what,
            What::ItemRaised {
                raised: Raised::Ready,
                ..
            }
        )),
        2
    );
    agree(&w, 0);
}

#[test]
fn a_switch_off_after_another_writers_event_is_stored_although_the_log_holds_an_earlier_one() {
    let mut w = world(&[MANUAL]);
    let off = Move::SwitchOff { agent: agent(0) };
    let on = Move::SwitchOn { agent: agent(0) };
    let first = schedule_drive(&w.ports, &mut w.board, &w.limits, off.clone());
    assert_eq!(first.answer, Ok(Answer::Ok));
    let back = schedule_drive(&w.ports, &mut w.board, &w.limits, on);
    assert_eq!(back.answer, Ok(Answer::Ok));
    assert!(!holds(w.facts(0)).contains(&Hold::Off));
    let last = owner_events(&w, 0).last().map_or(0, |e| e.seq.get());
    let old = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x4500)).unwrap();
    let closed = [
        What::ItemRaised {
            item: old.clone(),
            raised: Raised::Ready,
        },
        What::ItemLapsed {
            item: old,
            lapsed: halvo_coord::types::Lapsed::Ready(halvo_coord::types::OtherLapse::AppEnded),
        },
    ];
    for (k, what) in closed.into_iter().enumerate() {
        w.ports.events.borrow_mut().push(Event {
            workflow: support::flow(0),
            agent: agent(0),
            seq: std::num::NonZeroU32::new(last + 1 + k as u32).unwrap(),
            day: DAY,
            at: 0,
            what,
        });
    }
    let again = schedule_drive(&w.ports, &mut w.board, &w.limits, off);
    assert_eq!(again.answer, Ok(Answer::Ok));
    assert_eq!(
        count(&w, 0, |what| matches!(
            what,
            What::HoldSet {
                cause: HoldSetCause::Person
            }
        )),
        2,
        "the second hold_set is stored after the read of the other writer's events"
    );
    assert!(holds(w.facts(0)).contains(&Hold::Off));
    agree(&w, 0);
}

/// A queued agent whose `ready` item the owner stored with a lost acknowledgement, and whose hold
/// `off` the owner then stored too: the queued start, served at the end of the live run, meets
/// `seq`, and its read brings both. The agent takes up the revealed item, and the start leaves the
/// queue answered `hold` (ruling RD-52 (a)) with the item kept. The hold is drawn at the owner
/// because a switch-off in this life takes the start out of the queue (ruling RD-52 (d)); it was
/// `a_queued_agent_that_adopts_a_revealed_ready_item_stays_queued`, whose switch-off read both.
#[test]
fn a_queued_agent_whose_start_reads_a_hold_and_a_revealed_ready_item_keeps_that_item() {
    let limits = halvo_coord::types::Limits {
        max_live_runs: 1,
        ..halvo_coord::types::Limits::PRODUCT
    };
    let mut w = support::world_with(&[PERSON, MANUAL], limits);
    w.ports.refuse(Method::Log, PortError::NoService);
    let first = w.act(Act::Tick);
    assert!(first.report.raised.is_empty());
    lose_the_acknowledgement(&w);
    let ready = owner_events(&w, 0)
        .iter()
        .find_map(|e| match &e.what {
            What::ItemRaised {
                item,
                raised: Raised::Ready,
            } => Some(item.clone()),
            _ => None,
        })
        .expect("the owner stored the ready item");
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    let live = w.act(Act::Start { agent: agent(1) });
    assert_eq!(live.report.answer, Ok(Answer::Ok));
    let run = live.report.run.expect("the live run");
    let queued = w.act(Act::Start { agent: agent(0) });
    assert_eq!(queued.report.answer, Ok(Answer::Queued));
    assert_eq!(w.facts(0).doing, Doing::Queued(None));
    assert!(w.board.queue.holds(&agent(0)));
    stored_by_another_writer(
        &w,
        0,
        What::HoldSet {
            cause: HoldSetCause::Person,
        },
    );
    let end = w.act(Act::End { run, outcome: None });
    let served = &end.report.ended[0].next;
    assert_eq!(served.len(), 1);
    assert_eq!(
        (served[0].agent.clone(), served[0].answer),
        (agent(0), Err(halvo_coord::types::Cause::Hold))
    );
    assert!(
        matches!(&w.facts(0).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready && o.item == ready),
        "the agent holds the revealed item: {:?}",
        w.facts(0).doing
    );
    assert!(!w.board.queue.holds(&agent(0)));
    assert!(holds(w.facts(0)).contains(&Hold::Off));
    agree(&w, 0);
}

fn stored_by_another_writer(w: &World, n: usize, what: What) {
    let last = owner_events(w, n).last().map_or(0, |e| e.seq.get());
    w.ports.events.borrow_mut().push(Event {
        workflow: support::flow(n),
        agent: agent(n),
        seq: std::num::NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what,
    });
}

fn owe_run_ended(w: &mut World, run: &RunId) {
    w.board.owed.push(halvo_coord::schedule::Owed {
        agent: agent(0),
        workflow: support::flow(0),
        what: halvo_coord::schedule::Owing::RunEnded {
            run: run.clone(),
            cause: halvo_coord::types::RunEnd::Completed,
            day: DAY,
            at: 0,
        },
    });
}

fn ends_of(w: &World, run: &RunId) -> usize {
    count(
        w,
        0,
        |what| matches!(what, What::RunEnded { run: r, .. } if r == run),
    )
}

#[test]
fn an_owed_run_ended_whose_log_meets_seq_is_read_then_logged_at_the_next_tick() {
    let mut w = world(&[MANUAL]);
    let run = RunId::parse(&format!("{:032x}", 0xe701)).unwrap();
    owe_run_ended(&mut w, &run);
    stored_by_another_writer(&w, 0, What::SlotSkipped { slot: None });
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(
        ends_of(&w, &run),
        0,
        "the debt's log meets seq; its read holds no end of the run, so the debt stays owed"
    );
    assert_eq!(w.board.owed.len(), 1);
    w.set_clock(T0 + 20_000, DAY);
    w.act(Act::Tick);
    assert_eq!(ends_of(&w, &run), 1);
    assert!(w.board.owed.is_empty());
    agree(&w, 0);
}

#[test]
fn an_owed_run_ended_whose_run_the_read_shows_ended_with_another_cause_is_not_logged_again() {
    let mut w = world(&[MANUAL]);
    let run = RunId::parse(&format!("{:032x}", 0xe702)).unwrap();
    owe_run_ended(&mut w, &run);
    stored_by_another_writer(
        &w,
        0,
        What::RunEnded {
            run: run.clone(),
            cause: halvo_coord::types::RunEnd::Interrupted,
        },
    );
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(ends_of(&w, &run), 1, "the run ends once (I7.2)");
    assert!(w.board.owed.is_empty());
    agree(&w, 0);
}

#[test]
fn the_end_of_a_run_whose_log_meets_seq_logs_nothing_when_the_read_shows_the_run_ended() {
    let mut w = world(&[MANUAL]);
    let start = w.act(Act::Start { agent: agent(0) });
    let run = start.report.run.unwrap();
    w.ports.end_runner(&run);
    stored_by_another_writer(
        &w,
        0,
        What::RunEnded {
            run: run.clone(),
            cause: halvo_coord::types::RunEnd::Interrupted,
        },
    );
    w.act(Act::End {
        run: run.clone(),
        outcome: Some(halvo_coord::types::Outcome::Completed),
    });
    assert_eq!(ends_of(&w, &run), 1, "the run ends once (I7.2)");
    assert!(w.board.owed.is_empty());
    agree(&w, 0);
}

#[test]
fn a_start_closes_a_lost_run_started_that_its_events_already_hold() {
    let mut w = world(&[MANUAL]);
    let lost = RunId::parse(&format!("{:032x}", 0xe801)).unwrap();
    stored_by_another_writer(
        &w,
        0,
        What::RunStarted {
            run: lost.clone(),
            trigger: halvo_coord::types::Trigger::Person,
            slot: None,
        },
    );
    let read = owner_events(&w, 0).last().cloned().unwrap();
    w.board.members[0].facts.events.push(read);
    let start = w.act(Act::Start { agent: agent(0) });
    assert_eq!(start.report.answer, Err(halvo_coord::types::Cause::State));
    assert_eq!(ends_of(&w, &lost), 1, "run_ended with interrupted");
    assert!(w
        .ports
        .tried
        .borrow()
        .iter()
        .any(|t| t.call == Call::Send(halvo_coord::types::Body::EndRun { run: lost.clone() })));
    assert_eq!(
        count(&w, 0, |what| matches!(what, What::RunStarted { .. })),
        2
    );
    agree(&w, 0);
}

#[test]
fn an_owed_item_lapsed_whose_log_meets_seq_is_read_then_logged_at_the_next_tick() {
    let mut w = world(&[MANUAL]);
    let item = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0xe901)).unwrap();
    let lapsed = halvo_coord::types::Lapsed::Stuck(halvo_coord::types::OtherLapse::AppEnded);
    w.board.owed.push(halvo_coord::schedule::Owed {
        agent: agent(0),
        workflow: support::flow(0),
        what: halvo_coord::schedule::Owing::ItemLapsed {
            item: item.clone(),
            lapsed,
            day: DAY,
            at: 0,
        },
    });
    stored_by_another_writer(&w, 0, What::SlotSkipped { slot: None });
    let lapses = |w: &World| {
        count(
            w,
            0,
            |what| matches!(what, What::ItemLapsed { item: i, .. } if *i == item),
        )
    };
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(
        lapses(&w),
        0,
        "the debt's log meets seq and its read holds no close, so it stays owed"
    );
    assert_eq!(w.board.owed.len(), 1);
    w.set_clock(T0 + 20_000, DAY);
    w.act(Act::Tick);
    assert_eq!(lapses(&w), 1);
    assert!(w.board.owed.is_empty());
    agree(&w, 0);
}
