//! Where the check of U2.3b (I5.4, rulings RD-5 and RD-38) and the switch-off of U2.4b (I5.7,
//! ruling RD-52 (b)) meet on one agent.

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

fn check_items(w: &World, n: usize) -> usize {
    w.events_of(n)
        .iter()
        .filter(|name| *name == "item_raised check")
        .count()
}

/// The meeting (a) at the read that follows a refusal other than `seq` (ruling RD-52 (b) rule 1):
/// the owner holds the check's body with its lapse of the `ready` item, stored with a lost
/// acknowledgement, and refuses the switch-off's `hold_set` with `busy`. The switch-off's read
/// shows no hold `off`, so it is refused with `busy`, and the board takes up the check that the
/// read showed (U2.3b's `settle`): the `ready` item leaves it, and no later move raises a second
/// check. A switch-off after that stores the hold and keeps the check open.
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

/// The meeting (c): the person answers the check of an agent that is switched off. The answer
/// needs no hold but `check` (I5.4), so it is stored as for any agent, `went_through` with its
/// `slot_skipped` last; it ends the hold `check` alone, sends no state, and the hold `off` keeps
/// the agent from a start until a switch-on, after which it starts.
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
