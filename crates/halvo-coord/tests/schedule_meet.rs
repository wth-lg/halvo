#[path = "run_support.rs"]
mod support;

use halvo_coord::mem::Method;
use halvo_coord::ports::PortError;
use halvo_coord::run::Act;
use halvo_coord::schedule::{drive as schedule_drive, holds, Answer, Hold, Move};
use halvo_coord::types::{
    Cause, CheckAnswer, Doing, ItemId, Lapsed, OtherLapse, Outcome, OwnItem, OwnItemKind, Raised,
    RunId, What,
};
use support::{agent, world, World, A, DAY, MANUAL, T0};

const PERSON: A = A {
    person: true,
    slotted: true,
};

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

fn check_items(w: &World, n: usize) -> usize {
    w.events_of(n)
        .iter()
        .filter(|name| *name == "item_raised check")
        .count()
}

#[test]
fn meet_a_a_switch_off_refused_busy_whose_read_shows_the_check_body_takes_up_that_check() {
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    let ready = match &w.facts(0).doing {
        Doing::Idle(Some(o)) if o.kind == OwnItemKind::Ready => o.item.clone(),
        other => panic!("no open ready item: {other:?}"),
    };
    let run = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    w.store_end(0, &run, Outcome::EffectUnknown, 1);
    let check = ItemId::parse(&format!("{:032x}", 0x7a11)).unwrap();
    stored_unacknowledged(
        &w,
        0,
        What::ItemLapsed {
            item: ready,
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
    w.ports.refuse(Method::Log, PortError::Busy);
    let off = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(off.report.answer, Err(Cause::Busy));
    let held = Doing::Idle(Some(OwnItem {
        item: check,
        kind: OwnItemKind::Check,
    }));
    assert_eq!(w.facts(0).doing, held, "the read took up the check");
    assert!(!holds(w.facts(0)).contains(&Hold::Off));
    w.ports.clear_refusals();
    w.set_clock(T0 + 1_000, DAY);
    w.act(Act::Tick);
    assert_eq!(check_items(&w, 0), 1, "one check item, not two");
    let off = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(off.report.answer, Ok(Answer::Ok));
    assert!(holds(w.facts(0)).contains(&Hold::Off));
    assert_eq!(
        w.facts(0).doing,
        held,
        "the switch-off keeps the check open"
    );
}

#[test]
fn meet_c_the_answer_to_the_check_of_a_switched_off_agent_ends_the_check_alone() {
    let mut w = world(&[MANUAL]);
    let done = w.act(Act::Start { agent: agent(0) });
    let run = done.report.run.unwrap();
    w.act(Act::End {
        run: run.clone(),
        outcome: None,
    });
    w.store_end(0, &run, Outcome::Failed, 1);
    w.act(Act::Tick);
    assert!(matches!(&w.facts(0).doing, Doing::Idle(Some(o)) if o.kind == OwnItemKind::Check));
    let off = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(off.report.answer, Ok(Answer::Ok));
    assert!(holds(w.facts(0)).contains(&Hold::Off));
    w.forget();
    let mv = Move::Check {
        agent: agent(0),
        answer: CheckAnswer::WentThrough,
    };
    let answered = schedule_drive(&w.ports, &mut w.board, &w.limits, mv);
    assert_eq!(answered.answer, Ok(Answer::Ok));
    assert_eq!(
        w.calls(),
        vec!["log item_answered, hold_cleared, slot_skipped".to_string()],
        "the answer's body alone, and no state"
    );
    assert_eq!(w.facts(0).doing, Doing::Idle(None));
    assert_eq!(
        holds(w.facts(0)),
        vec![Hold::Off],
        "the hold off stands alone"
    );
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Err(Cause::Hold)
    );
    w.set_clock(T0 + 1_000, DAY);
    w.act(Act::Tick);
    assert_eq!(check_items(&w, 0), 1, "no second check for an answered run");
    let on = Move::SwitchOn { agent: agent(0) };
    let on = schedule_drive(&w.ports, &mut w.board, &w.limits, on);
    assert_eq!(on.answer, Ok(Answer::Ok));
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok)
    );
}

#[test]
fn meet_r13_a_queued_agent_whose_refused_switch_off_reveals_a_ready_item_stays_queued() {
    let mut w = world(&[MANUAL, PERSON]);
    let first = w
        .act(Act::Start { agent: agent(0) })
        .report
        .run
        .expect("agent 0 runs");
    let ready = ItemId::parse(&format!("{:032x}", 0x7a21)).unwrap();
    stored_unacknowledged(
        &w,
        1,
        What::ItemRaised {
            item: ready.clone(),
            raised: Raised::Ready,
        },
    );
    let starts_before = w
        .events_of(1)
        .iter()
        .filter(|e| *e == "run_started")
        .count();
    let queued = w.act(Act::Start { agent: agent(1) });
    assert_eq!(queued.report.answer, Ok(Answer::Queued));
    w.ports.refuse(Method::Log, PortError::Busy);
    let off = w.act(Act::SwitchOff { agent: agent(1) });
    assert_eq!(off.report.answer, Err(Cause::Busy));
    assert_eq!(
        w.facts(1).doing,
        Doing::Queued(Some(OwnItem {
            item: ready.clone(),
            kind: OwnItemKind::Ready,
        })),
        "the agent takes up the item and stays queued"
    );
    assert!(!holds(w.facts(1)).contains(&Hold::Off));
    let waiting = |w: &World| {
        w.board
            .queue
            .entries()
            .iter()
            .filter(|e| e.agent == agent(1))
            .count()
    };
    assert_eq!(waiting(&w), 1, "the queue holds the agent once");
    assert!(!w.events_of(1).iter().any(|e| e == "hold_set person"));
    assert!(
        !halvo_coord::roster::controls(w.facts(1), None, &w.limits)
            .contains(&halvo_coord::types::Control::RunNow),
        "the row offers no Start now"
    );
    w.ports.clear_refusals();
    let again = w.act(Act::Start { agent: agent(1) });
    assert_eq!(again.report.answer, Err(Cause::State), "a second start");
    assert_eq!(waiting(&w), 1, "the queue still holds the agent once");
    let end = w.act(Act::End {
        run: first,
        outcome: None,
    });
    let next = &end.report.ended[0].next;
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].agent, agent(1));
    assert_eq!(next[0].answer, Ok(Answer::Ok));
    assert!(next[0].run.is_some());
    let names = w.events_of(1);
    assert_eq!(
        names[names.len() - 3..],
        ["item_raised ready", "item_answered", "run_started"],
        "{names:?}"
    );
    let starts = names.iter().filter(|e| *e == "run_started").count();
    assert_eq!(starts, starts_before + 1, "one start");
    assert!(w.board.queue.is_empty());
}

#[test]
fn a_switch_off_that_drops_a_queued_start_keeps_the_revealed_ready_item_open() {
    let mut w = world(&[MANUAL, PERSON]);
    w.act(Act::Start { agent: agent(0) })
        .report
        .run
        .expect("agent 0 runs");
    let ready = ItemId::parse(&format!("{:032x}", 0x7a31)).unwrap();
    stored_unacknowledged(
        &w,
        1,
        What::ItemRaised {
            item: ready.clone(),
            raised: Raised::Ready,
        },
    );
    let queued = w.act(Act::Start { agent: agent(1) });
    assert_eq!(queued.report.answer, Ok(Answer::Queued));
    let off = w.act(Act::SwitchOff { agent: agent(1) });
    assert_eq!(off.report.answer, Ok(Answer::Ok));
    let open = Doing::Idle(Some(OwnItem {
        item: ready.clone(),
        kind: OwnItemKind::Ready,
    }));
    assert_eq!(w.facts(1).doing, open, "the item stays open after the drop");
    assert!(holds(w.facts(1)).contains(&Hold::Off));
    assert!(w.board.queue.is_empty(), "the queued start is dropped");
    let closing = |w: &World| {
        w.events_of(1)
            .iter()
            .filter(|e| *e == "item_answered" || e.starts_with("item_lapsed"))
            .count()
    };
    let start = w.act(Act::Start { agent: agent(1) });
    assert_eq!(start.report.answer, Err(Cause::Hold));
    assert_eq!(closing(&w), 0, "the item's Start stores no closing row");
    assert_eq!(w.facts(1).doing, open);
    let raised = |w: &World| {
        w.events_of(1)
            .iter()
            .filter(|e| *e == "item_raised ready")
            .count()
    };
    assert_eq!(raised(&w), 1);
    let on = Move::SwitchOn { agent: agent(1) };
    let on = schedule_drive(&w.ports, &mut w.board, &w.limits, on);
    assert_eq!(on.answer, Ok(Answer::Ok));
    w.set_clock(T0 + 1_000, DAY);
    w.act(Act::Tick);
    assert_eq!(raised(&w), 1, "no second ready item beside the open one");
    assert_eq!(w.facts(1).doing, open);
}

#[test]
fn the_rest_of_a_switch_off_with_no_hold_standing_answers_state_and_stops_and_sends_nothing() {
    let mut w = world(&[MANUAL, PERSON]);
    let run = w
        .act(Act::Start { agent: agent(0) })
        .report
        .run
        .expect("agent 0 runs");
    let queued = w.act(Act::Start { agent: agent(1) });
    assert_eq!(queued.report.answer, Ok(Answer::Queued));
    w.forget();
    for n in [0, 1] {
        let off = Move::Off { agent: agent(n) };
        let off = schedule_drive(&w.ports, &mut w.board, &w.limits, off);
        assert_eq!(off.answer, Err(Cause::State), "agent {n}");
        assert!(holds(w.facts(n)).is_empty(), "agent {n}");
    }
    assert_eq!(
        w.calls(),
        Vec::<String>::new(),
        "no cancel and no set_state"
    );
    assert!(
        w.board
            .runs
            .iter()
            .any(|r| r.run == run && r.stop.is_none()),
        "the run is not stopped"
    );
    assert_eq!(w.board.queue.len(), 1, "the queued start stays");
}
