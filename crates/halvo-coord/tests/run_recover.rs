#[path = "run_support.rs"]
mod support;

use halvo_coord::guard::Table;
use halvo_coord::mem::Method;
use halvo_coord::ports::{PortError, Reaped};
use halvo_coord::run::{recover, recover_step, Act, Earlier, StopBy};
use halvo_coord::schedule::{Answer, Hold};
use halvo_coord::types::{
    Answered, Cause, DecisionEntry, Digest, EffectAnswer, EffectId, Event, ItemId, LapseCause,
    Lapsed, Limits, OtherLapse, Outcome, PastRun, Raised, ReadyAnswer, RunEnd, RunId, Trigger,
    What,
};
use std::num::NonZeroU32;
use support::{agent, flow, world, world_with, World, A, DAY, MANUAL, T0};

fn two_live() -> Limits {
    Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    }
}

fn started(w: &mut World, n: usize) -> RunId {
    let done = w.act(Act::Start { agent: agent(n) });
    assert_eq!(done.report.answer, Ok(Answer::Ok), "{done:?}");
    done.report.run.unwrap()
}

/// A new life of the app on the same store: the library is a new process with these marks,
/// and the coordinator recovers before anything else.
fn relife(w: &mut World, marks: &[(RunId, Reaped)]) -> Vec<(RunId, RunEnd)> {
    w.ports.new_life();
    w.ports.leave_marks(marks);
    w.forget();
    let life = recover(&w.ports, &w.limits).expect("recovery");
    w.board = life.board;
    w.table = Table::new();
    life.ended.into_iter().map(|e| (e.run, e.cause)).collect()
}

fn holds(w: &World, n: usize) -> Vec<Hold> {
    halvo_coord::schedule::holds(w.facts(n))
}

#[test]
fn refusal_outlives_app() {
    let mut w = world_with(&[MANUAL, MANUAL], two_live());
    let live = started(&mut w, 0);
    let stopping = started(&mut w, 1);
    w.act(Act::Stop {
        run: stopping.clone(),
        by: StopBy::Person,
    });
    let ended = relife(
        &mut w,
        &[
            (live.clone(), Reaped::Gone),
            (stopping.clone(), Reaped::Gone),
        ],
    );
    let calls = w.calls();
    assert_eq!(calls.first().map(String::as_str), Some("reap"));
    assert!(!calls.iter().any(|c| c == "start"));
    assert_eq!(
        ended,
        vec![
            (live.clone(), RunEnd::Interrupted),
            (stopping.clone(), RunEnd::Interrupted)
        ],
        "a stopping run ends interrupted too"
    );
    assert_eq!(w.events_of(0).last().unwrap(), "run_ended interrupted");
    assert_eq!(w.events_of(1).last().unwrap(), "run_ended interrupted");
    assert_eq!(
        calls,
        vec![
            "reap",
            "log run_ended interrupted",
            "log run_ended interrupted",
            "end_run",
            "end_run"
        ]
    );
}

#[test]
fn no_start_before_reap_answered() {
    let mut w = world(&[MANUAL]);
    started(&mut w, 0);
    w.ports.new_life();
    w.ports.refuse(Method::Reap, PortError::NoService);
    assert_eq!(
        recover(&w.ports, &w.limits).err(),
        Some(Cause::NoService),
        "no board, so no start, while reap has not answered"
    );
}

#[test]
fn refusal_reap_alive() {
    let mut w = world_with(&[MANUAL, MANUAL], two_live());
    let alive = started(&mut w, 0);
    let gone = started(&mut w, 1);
    let ended = relife(
        &mut w,
        &[(alive.clone(), Reaped::Alive), (gone.clone(), Reaped::Gone)],
    );
    assert_eq!(
        ended,
        vec![
            (alive.clone(), RunEnd::Interrupted),
            (gone.clone(), RunEnd::Interrupted)
        ]
    );
    let sent: Vec<String> = w.calls().into_iter().filter(|c| c == "end_run").collect();
    assert_eq!(sent.len(), 1);
    let end_runs = |w: &World| -> Vec<RunId> {
        w.ports
            .tried
            .borrow()
            .iter()
            .filter_map(|t| match &t.call {
                halvo_coord::mem::Call::Send(halvo_coord::types::Body::EndRun { run }) => {
                    Some(run.clone())
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(end_runs(&w), vec![gone.clone()]);
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Err(Cause::State),
        "the run that reap answered alive holds its agent"
    );
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(
        end_runs(&w),
        vec![gone.clone()],
        "no end_run for it in this life"
    );
    let ended = relife(&mut w, &[(alive.clone(), Reaped::Gone)]);
    assert!(ended.is_empty(), "its run_ended is stored already");
    assert_eq!(end_runs(&w), vec![gone, alive.clone()]);
    assert_eq!(
        w.events_of(0)
            .iter()
            .filter(|e| *e == "run_ended interrupted")
            .count(),
        1
    );
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok),
        "the store took the end_run of the next life"
    );
}

#[test]
fn refusal_interrupted_no_retry() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.ports.refuse(Method::EndRun, PortError::NoService);
    relife(&mut w, &[(run.clone(), Reaped::Gone)]);
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Err(Cause::State),
        "no start while the store has not ended the run"
    );
    w.forget();
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(w.calls(), vec!["end_run refused"], "owed and sent again");
    w.ports.clear_refusals();
    w.store_end(0, &run, Outcome::Failed, 1);
    w.set_clock(T0 + 20_000, DAY);
    w.act(Act::Tick);
    assert!(
        !w.calls().iter().any(|c| c.starts_with("end_run")),
        "the store holds the run's record: nothing is owed"
    );
    assert!(w.board.owed.is_empty());
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Err(Cause::Hold)
    );
    assert!(holds(&w, 0).contains(&Hold::Check));
}

#[test]
fn a_run_of_an_earlier_life_with_no_mark_and_no_record_is_owed_its_end_run() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    let ended = relife(&mut w, &[]);
    assert_eq!(ended, vec![(run.clone(), RunEnd::Interrupted)]);
    assert!(w.calls().contains(&"end_run".to_string()));
}

#[test]
fn an_owed_end_run_whose_run_ended_in_an_earlier_life_is_sent_and_no_second_run_ended() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::End {
        run: run.clone(),
        outcome: None,
    });
    assert_eq!(w.events_of(0).last().unwrap(), "run_ended interrupted");
    let ended = relife(&mut w, &[(run.clone(), Reaped::Gone)]);
    assert!(ended.is_empty());
    assert_eq!(w.calls(), vec!["reap", "end_run"]);
    assert_eq!(
        w.events_of(0)
            .iter()
            .filter(|e| e.starts_with("run_ended"))
            .count(),
        2,
        "the earlier run of the world and this one"
    );
}

#[test]
fn recovery_owes_nothing_for_an_agent_whose_newest_run_the_store_has_ended() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::End {
        run: run.clone(),
        outcome: Some(Outcome::Completed),
    });
    w.store_end(0, &run, Outcome::Completed, 0);
    assert!(relife(&mut w, &[]).is_empty());
    assert_eq!(w.calls(), vec!["reap"]);
}

#[test]
fn recovery_loads_every_event_of_an_agent_with_more_than_64() {
    let mut w = world(&[MANUAL]);
    let mut extra = Vec::new();
    let mut seq = 2;
    let event = |seq: u32, what: What| Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(seq).unwrap(),
        day: DAY - 1,
        at: 0,
        what,
    };
    for k in 0..40u32 {
        let run = RunId::parse(&format!("{:032x}", 0x5000 + k)).unwrap();
        for what in [
            What::RunStarted {
                run: run.clone(),
                trigger: Trigger::Person,
                slot: None,
            },
            What::RunEnded {
                run,
                cause: RunEnd::Completed,
            },
        ] {
            seq += 1;
            extra.push(event(seq, what));
        }
    }
    let last = RunId::parse(&format!("{:032x}", 0x5fff)).unwrap();
    seq += 1;
    extra.push(event(
        seq,
        What::RunStarted {
            run: last.clone(),
            trigger: Trigger::Person,
            slot: None,
        },
    ));
    w.ports.events.borrow_mut().extend(extra);
    let ended = relife(&mut w, &[]);
    assert_eq!(ended, vec![(last, RunEnd::Interrupted)]);
    assert_eq!(w.facts(0).events.len(), 84);
}

#[test]
fn recovery_lapses_the_open_items_of_an_earlier_life_with_app_ended() {
    let mut w = world(&[MANUAL]);
    let item = |n: u32| ItemId::parse(&format!("{:032x}", 0x6000 + n)).unwrap();
    let run = RunId::parse(&format!("{:032x}", 0x0100)).unwrap();
    let entry = DecisionEntry {
        approval: halvo_coord::types::ApprovalId::parse(&format!("{:032x}", 9)).unwrap(),
        effect: EffectId::parse(&format!("{:032x}", 8)).unwrap(),
        digest: Digest::parse(&"d".repeat(64)).unwrap(),
        answer: EffectAnswer::AllowOnce,
    };
    let whats = vec![
        What::ItemRaised {
            item: item(1),
            raised: Raised::Confirm(run.clone()),
        },
        What::ItemAnswered {
            item: item(1),
            answer: Answered::Confirm(entry.clone()),
        },
        What::ItemRaised {
            item: item(2),
            raised: Raised::Confirm(run.clone()),
        },
        What::ItemAnswered {
            item: item(2),
            answer: Answered::Confirm(entry.clone()),
        },
        What::ItemReleased {
            item: item(2),
            entry,
        },
        What::ItemRaised {
            item: item(3),
            raised: Raised::Ready,
        },
        What::ItemAnswered {
            item: item(3),
            answer: Answered::Ready(ReadyAnswer::Start),
        },
        What::ItemRaised {
            item: item(4),
            raised: Raised::Ready,
        },
    ];
    let events: Vec<Event> = whats
        .into_iter()
        .zip(3u32..)
        .map(|(what, seq)| Event {
            workflow: flow(0),
            agent: agent(0),
            seq: NonZeroU32::new(seq).unwrap(),
            day: DAY,
            at: 0,
            what,
        })
        .collect();
    w.ports.events.borrow_mut().extend(events);
    relife(&mut w, &[]);
    let lapsed: Vec<(ItemId, Lapsed)> = w
        .ports
        .events
        .borrow()
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemLapsed { item, lapsed } => Some((item.clone(), *lapsed)),
            _ => None,
        })
        .collect();
    assert_eq!(
        lapsed,
        vec![
            (item(1), Lapsed::Confirm(LapseCause::AppEnded)),
            (item(4), Lapsed::Ready(OtherLapse::AppEnded))
        ]
    );
    assert_eq!(w.facts(0).doing, halvo_coord::types::Doing::Idle(None));
    assert!(w.board.queue.is_empty());
}

#[test]
fn a_run_ended_of_recovery_that_the_owner_does_not_take_is_owed_and_holds_the_agent() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.ports.store_end(
        &agent(0),
        PastRun {
            run: run.clone(),
            day: DAY,
            outcome: Outcome::Failed,
            unknown: 0,
        },
    );
    w.ports.refuse(Method::Log, PortError::NoService);
    relife(&mut w, &[]);
    assert!(!w
        .events_of(0)
        .contains(&"run_ended interrupted".to_string()));
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Err(Cause::State)
    );
    w.ports.clear_refusals();
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(w.events_of(0).last().unwrap(), "run_ended interrupted");
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok)
    );
}

#[test]
fn an_owed_end_run_of_a_failed_start_is_dropped_at_the_tick_once_the_store_holds_the_record() {
    let mut w = world(&[MANUAL]);
    w.ports.refuse(Method::Start, PortError::NoService);
    w.ports.refuse(Method::EndRun, PortError::NoService);
    let failed = w.act(Act::Start { agent: agent(0) });
    assert_eq!(failed.report.answer, Err(Cause::NoService));
    let run = failed.report.run.unwrap();
    assert_eq!(w.board.owed.len(), 1);
    w.ports.clear_refusals();
    w.store_end(0, &run, Outcome::Failed, 0);
    w.forget();
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert!(
        !w.calls().iter().any(|c| c.starts_with("end_run")),
        "the store took the run's record: it is acknowledged"
    );
    assert!(w.board.owed.is_empty());
}

#[test]
fn recover_step_owes_end_run_from_the_events_and_the_stores_last_record_alone() {
    let a = agent(0);
    let run = |n: u32| RunId::parse(&format!("{:032x}", 0x7000 + n)).unwrap();
    let ev = |seq: u32, what: What| Event {
        workflow: flow(0),
        agent: a.clone(),
        seq: NonZeroU32::new(seq).unwrap(),
        day: DAY,
        at: 0,
        what,
    };
    let started = |n: u32| What::RunStarted {
        run: run(n),
        trigger: Trigger::Person,
        slot: None,
    };
    let events = vec![
        ev(1, started(1)),
        ev(
            2,
            What::RunEnded {
                run: run(1),
                cause: RunEnd::Completed,
            },
        ),
        ev(3, started(2)),
    ];
    let record = |n: u32| PastRun {
        run: run(n),
        day: DAY,
        outcome: Outcome::Completed,
        unknown: 0,
    };
    let none = Earlier {
        agent: &a,
        events: &events,
        last: Some(&record(1)),
    };
    let step = recover_step(&none, &[]);
    assert_eq!(step.ended, vec![run(2)]);
    assert_eq!(step.owed, Some(run(2)));
    assert_eq!(
        recover_step(&none, &[(run(2), Reaped::Alive)]).owed,
        None,
        "reap answered alive"
    );
    assert_eq!(
        recover_step(&none, &[(run(2), Reaped::Gone)]).owed,
        Some(run(2))
    );
    let named = Earlier {
        last: Some(&record(2)),
        ..none
    };
    assert_eq!(recover_step(&named, &[]).owed, None);
    assert_eq!(recover_step(&named, &[]).ended, vec![run(2)]);
    let _ = A {
        person: false,
        slotted: false,
    };
}
