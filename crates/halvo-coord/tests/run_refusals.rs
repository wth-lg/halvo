#[path = "run_support.rs"]
mod support;

use halvo_coord::items::drive::{flush, store, Ledger};
use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::Owner;
use halvo_coord::ports::PortError;
use halvo_coord::run::recover;
use halvo_coord::run::Act;
use halvo_coord::schedule::Answer;
use halvo_coord::schedule::{Owed, Owing};
use halvo_coord::types::{Event, Limits, Name, Outcome, RunEnd, RunId, Trigger, What};
use std::num::NonZeroU32;
use support::{agent, flow, world_with, World, A, DAY, MANUAL, T0};

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

const DAY_MS: u64 = 86_400_000;

fn skip(slot: u32) -> What {
    What::SlotSkipped { slot: Some(slot) }
}

fn slot_is(e: &Event, slot: u32) -> bool {
    matches!(e.what, What::SlotSkipped { slot: Some(s) } if s == slot)
}

fn second(e: &Event) -> bool {
    slot_is(e, 2)
}

fn ledger_of(w: &World) -> Ledger {
    let last = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(0))
        .map(|e| e.seq.get())
        .max()
        .unwrap_or(0);
    Ledger::new(flow(0), agent(0), last)
}

/// Three rows that wait, the second stamped at `second_at`, the others at the clock's instant.
fn three_waiting(w: &World, second_at: u64) -> Ledger {
    let mut l = ledger_of(w);
    w.ports.refuse(Method::Log, PortError::NoService);
    let _ = store(&w.ports, &mut l, &w.limits, vec![skip(1)]);
    w.ports.set_now(second_at);
    let _ = store(&w.ports, &mut l, &w.limits, vec![skip(2)]);
    w.ports.set_now(T0);
    let _ = store(&w.ports, &mut l, &w.limits, vec![skip(3)]);
    w.ports.clear_refusals();
    assert_eq!(l.waiting().len(), 3);
    l
}

fn stored_slots(w: &World) -> Vec<u32> {
    w.ports
        .events
        .borrow()
        .iter()
        .filter_map(|e| match e.what {
            What::SlotSkipped { slot: Some(s) } => Some(s),
            _ => None,
        })
        .collect()
}

fn bodies(w: &World) -> Vec<usize> {
    w.ports
        .tried
        .borrow()
        .iter()
        .filter_map(|t| match &t.call {
            Call::Log(events)
                if t.refused != Some(PortError::NoService)
                    && events
                        .iter()
                        .any(|e| matches!(e.what, What::SlotSkipped { .. })) =>
            {
                Some(events.len())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn class_b_a_body_refused_with_no_workflow_leaves_the_list_and_is_not_sent_again() {
    let w = world_with(&[MANUAL], Limits::PRODUCT);
    let mut l = three_waiting(&w, T0);
    w.ports.refuse(Method::Log, refused("no_workflow"));
    assert!(flush(&w.ports, &mut l, &w.limits).is_err());
    assert!(l.waiting().is_empty(), "the body is not sent again");
    assert!(l.taken_off().is_empty(), "no fault and nothing counted");
    let before = bodies(&w).len();
    flush(&w.ports, &mut l, &w.limits).unwrap();
    assert_eq!(bodies(&w).len(), before);
}

#[test]
fn class_c_the_records_go_one_to_a_body_and_one_proven_by_the_next_is_taken_off() {
    let w = world_with(&[MANUAL], Limits::PRODUCT);
    let mut l = three_waiting(&w, T0);
    w.ports.refuse_record(second, refused("malformed"));
    flush(&w.ports, &mut l, &w.limits).unwrap();
    assert_eq!(bodies(&w), vec![3, 1, 1, 1], "the body, then one to a body");
    assert_eq!(
        stored_slots(&w),
        vec![1, 3],
        "the third under the second's seq"
    );
    assert_eq!(
        l.taken_off(),
        vec![("slot_skipped".to_string(), "malformed".to_string())]
    );
    assert!(l.waiting().is_empty());
}

#[test]
fn class_c_an_early_record_keeps_its_place_and_nothing_behind_it_is_sent() {
    let w = world_with(&[MANUAL], Limits::PRODUCT);
    let mut l = three_waiting(&w, T0 + DAY_MS);
    w.ports.refuse_record(second, refused("malformed"));
    assert!(flush(&w.ports, &mut l, &w.limits).is_err());
    assert_eq!(stored_slots(&w), vec![1]);
    assert_eq!(l.waiting().len(), 2, "it keeps its place");
    assert!(l.holds_place());
    assert!(l.taken_off().is_empty());
}

#[test]
fn class_c_a_record_more_than_two_days_ahead_is_taken_off_and_counted() {
    let w = world_with(&[MANUAL], Limits::PRODUCT);
    let mut l = three_waiting(&w, T0 + 3 * DAY_MS);
    w.ports.refuse_record(second, refused("too_large"));
    flush(&w.ports, &mut l, &w.limits).unwrap();
    assert_eq!(stored_slots(&w), vec![1, 3]);
    assert_eq!(
        l.taken_off(),
        vec![("slot_skipped".to_string(), "too_large".to_string())]
    );
}

#[test]
fn class_c_a_record_outside_its_bound_is_taken_off_and_counted() {
    let w = world_with(&[MANUAL], Limits::PRODUCT);
    let mut l = three_waiting(&w, T0 - 94 * DAY_MS);
    w.ports.refuse_record(second, refused("malformed"));
    flush(&w.ports, &mut l, &w.limits).unwrap();
    assert_eq!(stored_slots(&w), vec![1, 3]);
    assert_eq!(l.taken_off().len(), 1);
}

#[test]
fn class_c_a_lone_record_waits_for_a_later_write_under_its_seq_and_nothing_where_that_fails() {
    let w = world_with(&[MANUAL], Limits::PRODUCT);
    let mut l = ledger_of(&w);
    w.ports.refuse(Method::Log, PortError::NoService);
    let _ = store(&w.ports, &mut l, &w.limits, vec![skip(2)]);
    w.ports.clear_refusals();
    w.ports.refuse_record(second, refused("malformed"));
    assert!(flush(&w.ports, &mut l, &w.limits).is_err());
    assert_eq!(l.waiting().len(), 1, "it keeps its place");
    assert!(
        !l.holds_place(),
        "a lone record that waits for its proof holds nothing"
    );
    assert!(!l.refused(), "and its refusal alone does not count");
    let seq = l.waiting()[0].seq;
    let other = Event {
        seq,
        what: skip(9),
        ..l.waiting()[0].clone()
    };
    w.ports.log(std::slice::from_ref(&other)).unwrap();
    l.take(&[other]);
    assert!(
        l.waiting().is_empty(),
        "the later write under its seq proves it"
    );
    assert_eq!(l.taken_off().len(), 1);
}

#[test]
fn class_d_the_body_stays_whole_and_first_and_goes_again() {
    let w = world_with(&[MANUAL], Limits::PRODUCT);
    let mut l = three_waiting(&w, T0);
    w.ports.refuse(Method::Log, refused("store"));
    assert!(flush(&w.ports, &mut l, &w.limits).is_err());
    assert_eq!(l.waiting().len(), 3);
    assert!(l.holds_place());
    assert!(l.refused());
    w.ports.clear_refusals();
    flush(&w.ports, &mut l, &w.limits).unwrap();
    assert_eq!(
        bodies(&w),
        vec![3, 3],
        "the same body, whole, at the next send"
    );
    assert!(l.taken_off().is_empty());
}

fn owed_run_ended(w: &mut World, day_back: u32) -> RunId {
    let run = RunId::parse(&format!("{:032x}", 0x0e00)).unwrap();
    let at = (T0 - u64::from(day_back) * DAY_MS) as i64 * 1_000_000;
    w.board.owed.push(Owed {
        agent: agent(0),
        workflow: flow(0),
        what: Owing::RunEnded {
            run: run.clone(),
            cause: RunEnd::Interrupted,
            day: DAY - day_back,
            at,
        },
    });
    run
}

#[test]
fn class_c_a_debt_outside_its_bound_is_dropped_and_counted_and_one_inside_stays_owed() {
    let mut w = world_with(&[MANUAL, MANUAL], Limits::PRODUCT);
    owed_run_ended(&mut w, 100);
    fn ended(e: &Event) -> bool {
        matches!(e.what, What::RunEnded { .. })
    }
    w.ports.refuse_record(ended, refused("malformed"));
    w.act(Act::Tick);
    assert!(w.board.owed.is_empty(), "dropped");
    assert_eq!(
        w.board.dropped(&agent(0)),
        Some((1, "run_ended", "malformed"))
    );
    owed_run_ended(&mut w, 1);
    w.act(Act::Tick);
    assert_eq!(w.board.owed.len(), 1, "a debt inside its bound stays owed");
    assert_eq!(w.board.dropped(&agent(0)).map(|d| d.0), Some(1));
}

#[test]
fn class_d_a_debt_stays_owed_and_class_b_ends_it() {
    let mut w = world_with(&[MANUAL], Limits::PRODUCT);
    owed_run_ended(&mut w, 0);
    w.ports.refuse(Method::Log, refused("internal"));
    w.act(Act::Tick);
    w.act(Act::Tick);
    assert_eq!(w.board.owed.len(), 1, "class D: owed at every tick");
    w.ports.refuse(Method::Log, refused("no_workflow"));
    w.act(Act::Tick);
    assert!(w.board.owed.is_empty(), "class B ends the debt");
    assert_eq!(w.board.dropped(&agent(0)), None);
}

/// Nine runs of an earlier life with no end, of one agent, as nine `run_started` rows.
fn nine_open_runs(w: &World) -> Vec<RunId> {
    let mut runs = Vec::new();
    let mut events = w.ports.events.borrow_mut();
    let mut seq = events
        .iter()
        .filter(|e| e.agent == agent(0))
        .map(|e| e.seq.get())
        .max()
        .unwrap_or(0);
    for k in 0..9u32 {
        let run = RunId::parse(&format!("{:032x}", 0x0f00 + k)).unwrap();
        seq += 1;
        events.push(Event {
            workflow: flow(0),
            agent: agent(0),
            seq: NonZeroU32::new(seq).unwrap(),
            day: DAY,
            at: T0 as i64 * 1_000_000,
            what: What::RunStarted {
                run: run.clone(),
                trigger: Trigger::Person,
                slot: None,
            },
        });
        runs.push(run);
    }
    runs
}

fn recovery_bodies(w: &World) -> Vec<(usize, bool)> {
    w.ports
        .tried
        .borrow()
        .iter()
        .filter_map(|t| match &t.call {
            Call::Log(events)
                if events
                    .iter()
                    .all(|e| matches!(e.what, What::RunEnded { .. })) =>
            {
                Some((events.len(), t.refused.is_none()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn recoverys_body_is_cut_between_debts_by_records() {
    let limits = Limits {
        request_records: 8,
        ..Limits::PRODUCT
    };
    let mut w = world_with(&[MANUAL], limits);
    nine_open_runs(&w);
    w.ports.new_life();
    let life = recover(&w.ports, &w.limits).expect("recovery");
    w.board = life.board;
    assert_eq!(recovery_bodies(&w), vec![(8, true), (1, true)]);
    assert!(w.board.owed.is_empty(), "every end stored, nothing owed");
}

#[test]
fn recoverys_body_is_cut_between_debts_by_bytes() {
    let limits = Limits {
        request_bytes: 2_000,
        ..Limits::PRODUCT
    };
    let mut w = world_with(&[MANUAL], limits);
    nine_open_runs(&w);
    w.ports.new_life();
    let life = recover(&w.ports, &w.limits).expect("recovery");
    w.board = life.board;
    let cut = recovery_bodies(&w);
    assert!(cut.len() > 1 && cut.iter().all(|(_, ok)| *ok), "{cut:?}");
    assert_eq!(cut.iter().map(|(n, _)| n).sum::<usize>(), 9);
    assert!(w.board.owed.is_empty());
}

#[test]
fn recoverys_body_refused_with_no_workflow_owes_nothing() {
    let mut w = world_with(&[MANUAL], Limits::PRODUCT);
    nine_open_runs(&w);
    w.ports.new_life();
    w.ports.refuse(Method::Log, refused("no_workflow"));
    let life = recover(&w.ports, &w.limits).expect("recovery");
    w.board = life.board;
    assert!(
        !w.board
            .owed
            .iter()
            .any(|o| matches!(o.what, Owing::RunEnded { .. })),
        "class B: no debt"
    );
}
