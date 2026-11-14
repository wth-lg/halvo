#[path = "run_support.rs"]
mod support;

use halvo_coord::guard::Table;
use halvo_coord::mem::Method;
use halvo_coord::ports::{Analysis, Clock, Owner, PortError, Reaped, Rng, Runs};
use halvo_coord::run::{recover, recover_step, Act, Earlier, StopBy};
use halvo_coord::schedule::{Answer, Hold};
use halvo_coord::types::{
    Agent, AgentId, Answered, Body, Cause, Decision, DecisionEntry, Digest, EffectAnswer, EffectId,
    Event, HandbackResult, ItemId, LapseCause, Lapsed, Limits, OtherLapse, Outcome, PastRun,
    Raised, ReadyAnswer, Reply, RunEnd, RunId, RunSpec, Trigger, What, Workflow,
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
    assert!(
        !w.facts(0).store_ended && !w.facts(1).store_ended,
        "the new board reads from the owner that the store has not ended them"
    );
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
fn recovery_ends_the_runs_in_the_order_of_their_starts() {
    let mut w = world_with(&[MANUAL, MANUAL], two_live());
    let later_agent_first = started(&mut w, 1);
    w.set_clock(T0 + 1, DAY);
    let then = started(&mut w, 0);
    let ended = relife(&mut w, &[]);
    assert_eq!(
        ended,
        vec![
            (later_agent_first, RunEnd::Interrupted),
            (then, RunEnd::Interrupted)
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
fn a_new_life_keeps_the_agent_of_a_dismissed_workflow_and_owes_what_its_run_is_owed() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    {
        let mut list = w.ports.workflows.borrow_mut();
        list[0] = halvo_coord::types::Workflow::Dismissed(halvo_coord::types::DismissedWorkflow {
            id: flow(0),
            category: halvo_coord::types::Category::CalendarCleanup,
            site: None,
        });
    }
    let ended = relife(&mut w, &[(run.clone(), Reaped::Gone)]);
    assert_eq!(ended, vec![(run, RunEnd::Interrupted)]);
    assert_eq!(
        w.calls(),
        vec!["reap", "log run_ended interrupted", "end_run"]
    );
    assert_eq!(
        w.board.members.len(),
        1,
        "the agent is known to the new life"
    );
    w.set_clock(T0 + 10_000, DAY + 1);
    w.act(Act::Tick);
    assert!(
        !w.calls()
            .iter()
            .any(|c| c.starts_with("set_state") || c.starts_with("start_run")),
        "no active and no start by the schedule for the agent of a dismissed workflow"
    );
    assert_ne!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Err(Cause::UnknownAgent)
    );
}

#[test]
fn refusal_gone_outlives_app() {
    for answers in [vec![Reaped::Gone], vec![Reaped::Alive, Reaped::Gone]] {
        let mut w = world(&[MANUAL]);
        let run = started(&mut w, 0);
        let stored = w.ports.events.borrow().len();
        w.ports.workflows.borrow_mut().clear();
        for answer in &answers {
            let ended = relife(&mut w, &[(run.clone(), *answer)]);
            assert!(ended.is_empty(), "no run of the restart's ends");
            assert_eq!(w.calls(), vec!["reap"], "reap is called, nothing else acts");
            assert_eq!(
                w.act(Act::Start { agent: agent(0) }).report.answer,
                Err(Cause::UnknownAgent)
            );
        }
        let reaps = w
            .ports
            .tried
            .borrow()
            .iter()
            .filter(|t| matches!(t.call, halvo_coord::mem::Call::Reap))
            .count();
        assert_eq!(reaps, answers.len(), "each life's reap tries the run");
        assert_eq!(
            halvo_coord::ports::Runs::reap(&w.ports),
            Ok(vec![]),
            "the mark goes once reap answered gone"
        );
        assert_eq!(
            w.ports.events.borrow().len(),
            stored,
            "no event for that run"
        );
    }
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
    assert!(w.facts(0).store_ended);
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
fn an_owed_run_ended_that_the_owner_stored_with_a_lost_acknowledgement_is_paid_at_the_next_tick() {
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
    assert_eq!(w.board.owed.len(), 1, "the run_ended is owed");
    let last = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(0))
        .map(|e| e.seq.get())
        .max()
        .unwrap();
    w.ports.events.borrow_mut().push(Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what: What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Interrupted,
        },
    });
    w.ports.clear_refusals();
    for k in 1..3 {
        w.set_clock(T0 + k * 10_000, DAY);
        w.act(Act::Tick);
    }
    assert!(w.board.owed.is_empty(), "the owner holds it: paid");
    assert_eq!(
        w.events_of(0)
            .iter()
            .filter(|e| *e == "run_ended interrupted")
            .count(),
        1,
        "stored once"
    );
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok),
        "the agent is not held for good"
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
fn an_owed_end_run_is_dropped_once_the_store_holds_the_record_of_a_later_run_of_the_agent() {
    let mut w = world(&[MANUAL]);
    w.ports.refuse(Method::Start, PortError::NoService);
    w.ports.refuse(Method::EndRun, PortError::NoService);
    let first = w.act(Act::Start { agent: agent(0) }).report.run.unwrap();
    w.store_end(0, &first, Outcome::Failed, 0);
    let second = w.act(Act::Start { agent: agent(0) }).report.run.unwrap();
    assert_ne!(first, second);
    assert_eq!(w.board.owed.len(), 2);
    w.store_end(0, &second, Outcome::Failed, 0);
    w.forget();
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert!(
        !w.calls().iter().any(|c| c.starts_with("end_run")),
        "the store took a later run of the agent, so it ended the first"
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

#[test]
fn review_life_3() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::End {
        run: run.clone(),
        outcome: None,
    });
    w.ports.store_end(
        &agent(0),
        PastRun {
            run: run.clone(),
            day: DAY,
            outcome: Outcome::Failed,
            unknown: 1,
        },
    );
    let seq_of = |w: &World| {
        w.ports
            .events
            .borrow()
            .iter()
            .filter(|e| e.agent == agent(0))
            .map(|e| e.seq.get())
            .max()
            .unwrap_or(0)
    };
    let item = ItemId::parse(&format!("{:032x}", 0x7003)).unwrap();
    let last = seq_of(&w);
    w.ports.events.borrow_mut().push(Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what: What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    });
    w.ports.refuse(Method::Log, PortError::NoService);
    relife(&mut w, &[]);
    let last = seq_of(&w);
    w.ports.events.borrow_mut().push(Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what: What::ItemLapsed {
            item,
            lapsed: Lapsed::Ready(OtherLapse::AppEnded),
        },
    });
    w.ports.clear_refusals();
    for k in 1..4 {
        w.set_clock(T0 + k * 10_000, DAY);
        w.act(Act::Tick);
    }
    let checks = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| {
            e.agent == agent(0)
                && matches!(
                    &e.what,
                    What::ItemRaised {
                        raised: Raised::Check(_),
                        ..
                    }
                )
        })
        .count();
    assert_eq!(
        checks, 1,
        "XB-18: the item check is raised once the record is read"
    );
}

/// What the owner does at one log of `Late`.
enum Turn {
    /// It refuses the log with the error.
    Refuse(PortError),
    /// It first stores this event at the `seq` of the log, as a log of the earlier life that
    /// lands after recovery read the ledger.
    Late(What),
    /// It stores the log and answers `no_service`: the acknowledgement is lost. A log that the
    /// owner refuses gets the owner's refusal and leaves the turn to the next log.
    Lose,
}

/// The ports with a late writer: the owner takes the turns in order, one for each log, and
/// then stores each log as the memory owner does.
struct Late<'a, P> {
    ports: &'a P,
    turns: std::cell::RefCell<std::collections::VecDeque<Turn>>,
}

impl<'a, P> Late<'a, P> {
    fn new(ports: &'a P, turns: Vec<Turn>) -> Late<'a, P> {
        Late {
            ports,
            turns: std::cell::RefCell::new(turns.into()),
        }
    }
}

impl<P: Owner> Owner for Late<'_, P> {
    type Config = P::Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        let turn = self.turns.borrow_mut().pop_front();
        match (turn, events.first()) {
            (Some(Turn::Refuse(e)), _) => return Err(e),
            (Some(Turn::Lose), _) => {
                let stored = self.ports.log(events);
                if stored.is_err() {
                    self.turns.borrow_mut().push_front(Turn::Lose);
                }
                return stored.and(Err(PortError::NoService));
            }
            (Some(Turn::Late(what)), Some(first)) => {
                let late = Event {
                    what,
                    ..first.clone()
                };
                self.ports.log(&[late]).expect("the late log is stored");
            }
            _ => {}
        }
        self.ports.log(events)
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

#[test]
fn a_recovery_body_refused_with_seq_is_read_again_from_the_owners_events_and_logged_once_more() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    let item = ItemId::parse(&format!("{:032x}", 0x7101)).unwrap();
    let last = w.events_of(0).len() as u32;
    w.ports.events.borrow_mut().push(Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what: What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    });
    w.ports.new_life();
    w.ports.leave_marks(&[(run.clone(), Reaped::Gone)]);
    w.forget();
    let late = Late::new(
        &w.ports,
        vec![Turn::Late(What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Completed,
        })],
    );
    let life = recover(&late, &w.limits).expect("recovery");
    assert!(
        life.ended.is_empty(),
        "the run ended completed in the earlier life: recovery does not end it again"
    );
    assert!(
        !life
            .board
            .owed
            .iter()
            .any(|o| matches!(o.what, halvo_coord::schedule::Owing::RunEnded { .. })),
        "the body is stored, not owed"
    );
    assert_eq!(
        w.events_of(0)[last as usize..],
        [
            "item_raised ready",
            "run_ended completed",
            "item_lapsed app_ended"
        ],
        "the body after the late event, logged in recovery itself"
    );
    let refused = w
        .ports
        .tried
        .borrow()
        .iter()
        .filter(|t| matches!(&t.refused, Some(PortError::Refused(n)) if n.as_str() == "seq"))
        .count();
    assert_eq!(refused, 1, "one refusal, then the body once more");
    w.board = life.board;
    w.table = Table::new();
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok),
        "the agent's events hold the owner's last seq"
    );
}

#[test]
fn review_life_1() {
    let mut w = world(&[MANUAL]);
    let item = ItemId::parse(&format!("{:032x}", 0x7001)).unwrap();
    let last = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(0))
        .map(|e| e.seq.get())
        .max()
        .unwrap_or(0);
    w.ports.events.borrow_mut().push(Event {
        workflow: flow(0),
        agent: agent(0),
        seq: NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what: What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    });
    w.ports.refuse(Method::Log, PortError::NoService);
    relife(&mut w, &[]);
    w.ports.clear_refusals();
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    w.set_clock(T0 + 20_000, DAY);
    w.act(Act::Tick);
    let lapsed = w
        .ports
        .events
        .borrow()
        .iter()
        .any(|e| matches!(&e.what, What::ItemLapsed { item: i, .. } if i == &item));
    assert!(
        lapsed,
        "I5.8: item_lapsed with app_ended for each open item; the refused lapse is never stored"
    );
}

/// Stores `what` as the owner's next event of agent `n`, outside the coordinator's view.
fn stored_outside(w: &World, n: usize, what: What) {
    let last = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == agent(n))
        .map(|e| e.seq.get())
        .max()
        .unwrap_or(0);
    w.ports.events.borrow_mut().push(Event {
        workflow: flow(n),
        agent: agent(n),
        seq: NonZeroU32::new(last + 1).unwrap(),
        day: DAY,
        at: 0,
        what,
    });
}

/// A new life whose recovery body for agent 0 the owner refuses: the open ready item `item`
/// of the earlier life is owed its `item_lapsed`.
fn owed_lapse(item: &ItemId) -> World {
    let mut w = world(&[MANUAL]);
    stored_outside(
        &w,
        0,
        What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    );
    w.ports.refuse(Method::Log, PortError::NoService);
    relife(&mut w, &[]);
    w.ports.clear_refusals();
    assert_eq!(w.board.owed.len(), 1, "the lapse is owed");
    w
}

fn lapses_of(w: &World, item: &ItemId) -> usize {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| matches!(&e.what, What::ItemLapsed { item: i, .. } if i == item))
        .count()
}

#[test]
fn an_owed_item_lapsed_that_the_owner_stored_with_a_lost_acknowledgement_is_paid_once() {
    let item = ItemId::parse(&format!("{:032x}", 0x7301)).unwrap();
    let mut w = owed_lapse(&item);
    stored_outside(
        &w,
        0,
        What::ItemLapsed {
            item: item.clone(),
            lapsed: Lapsed::Ready(OtherLapse::AppEnded),
        },
    );
    for k in 1..3 {
        w.set_clock(T0 + k * 10_000, DAY);
        w.act(Act::Tick);
    }
    assert_eq!(lapses_of(&w, &item), 1, "stored once");
    assert!(w.board.owed.is_empty(), "the owner holds it: paid");
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Ok(Answer::Ok),
        "the agent's events hold the owner's last seq"
    );
}

#[test]
fn an_owed_item_lapsed_refused_with_seq_for_another_event_is_logged_after_it() {
    let item = ItemId::parse(&format!("{:032x}", 0x7302)).unwrap();
    let mut w = owed_lapse(&item);
    stored_outside(&w, 0, What::SlotSkipped { slot: None });
    for k in 1..3 {
        w.set_clock(T0 + k * 10_000, DAY);
        w.act(Act::Tick);
    }
    assert_eq!(lapses_of(&w, &item), 1, "logged after the other event");
    assert!(w.board.owed.is_empty());
    let at = w
        .ports
        .events
        .borrow()
        .iter()
        .find(|e| matches!(&e.what, What::ItemLapsed { item: i, .. } if *i == item))
        .map(|e| (e.day, e.at));
    assert_eq!(
        at,
        Some((DAY, i64::try_from(T0).unwrap() * 1_000_000)),
        "the lapse keeps the day and the instant of recovery"
    );
}

#[test]
fn an_owed_item_lapsed_of_a_gone_workflow_is_logged_once_more_and_ends_at_no_workflow() {
    let item = ItemId::parse(&format!("{:032x}", 0x7303)).unwrap();
    let mut w = owed_lapse(&item);
    w.ports
        .workflows
        .borrow_mut()
        .retain(|wf| wf.id() != &flow(0));
    for k in 1..3 {
        w.set_clock(T0 + k * 10_000, DAY);
        w.act(Act::Tick);
    }
    let gone = PortError::Refused(halvo_coord::types::Name::parse("no_workflow").unwrap());
    let sends: Vec<Option<PortError>> = w
        .ports
        .tried
        .borrow()
        .iter()
        .filter(|t| match &t.call {
            halvo_coord::mem::Call::Log(events) => events
                .iter()
                .any(|e| matches!(&e.what, What::ItemLapsed { item: i, .. } if *i == item)),
            _ => false,
        })
        .map(|t| t.refused.clone())
        .collect();
    assert_eq!(
        sends,
        vec![Some(PortError::NoService), Some(gone)],
        "recovery's body, then once more, and the refusal no_workflow ends the debt"
    );
    assert!(w.board.owed.is_empty());
}

#[test]
fn a_read_after_a_seq_refusal_of_recoverys_check_holds_no_item_of_the_earlier_life() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::End {
        run: run.clone(),
        outcome: None,
    });
    w.ports.store_end(
        &agent(0),
        PastRun {
            run,
            day: DAY,
            outcome: Outcome::Failed,
            unknown: 1,
        },
    );
    let item = ItemId::parse(&format!("{:032x}", 0x7304)).unwrap();
    stored_outside(
        &w,
        0,
        What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    );
    w.ports.new_life();
    w.forget();
    let late = Late::new(
        &w.ports,
        vec![
            Turn::Refuse(PortError::NoService),
            Turn::Late(What::SlotSkipped { slot: None }),
        ],
    );
    let life = recover(&late, &w.limits).expect("recovery");
    assert_eq!(life.board.owed.len(), 1, "the ready item's lapse is owed");
    assert_eq!(
        life.checked,
        vec![agent(0)],
        "the check is logged after the late event"
    );
    assert!(
        matches!(
            &life.board.members[0].facts.doing,
            halvo_coord::types::Doing::Idle(Some(o)) if o.item != item
        ),
        "the agent holds the check, not the item of the earlier life that lapses"
    );
}

#[test]
fn an_item_that_had_already_lapsed_gets_no_second_item_lapsed_in_a_new_life() {
    let mut w = world(&[MANUAL]);
    let lapsed = ItemId::parse(&format!("{:032x}", 0x7401)).unwrap();
    let open = ItemId::parse(&format!("{:032x}", 0x7402)).unwrap();
    stored_outside(
        &w,
        0,
        What::ItemRaised {
            item: lapsed.clone(),
            raised: Raised::Ready,
        },
    );
    stored_outside(
        &w,
        0,
        What::ItemLapsed {
            item: lapsed.clone(),
            lapsed: Lapsed::Ready(OtherLapse::AppEnded),
        },
    );
    stored_outside(
        &w,
        0,
        What::ItemRaised {
            item: open.clone(),
            raised: Raised::Ready,
        },
    );
    relife(&mut w, &[]);
    relife(&mut w, &[]);
    assert_eq!(
        lapses_of(&w, &lapsed),
        1,
        "I5.8: an item with an item_lapsed is not open, in any later life"
    );
    assert_eq!(
        lapses_of(&w, &open),
        1,
        "the open item lapses in the first new life and in no later one"
    );
}

#[test]
fn a_new_life_knows_the_agent_of_a_dismissed_workflow_as_paused_manual_with_no_period() {
    let mut w = world(&[A {
        person: false,
        slotted: true,
    }]);
    assert_eq!(
        w.facts(0).workflow.trigger,
        halvo_coord::types::WorkflowTrigger::Schedule
    );
    let site = Some("calendar.example".to_string());
    w.ports.workflows.borrow_mut()[0] =
        Workflow::Dismissed(halvo_coord::types::DismissedWorkflow {
            id: flow(0),
            category: halvo_coord::types::Category::CalendarCleanup,
            site: site.clone(),
        });
    relife(&mut w, &[]);
    assert_eq!(
        w.facts(0).workflow,
        halvo_coord::types::FullWorkflow {
            id: flow(0),
            state: halvo_coord::types::WorkflowState::Paused,
            category: halvo_coord::types::Category::CalendarCleanup,
            site,
            trigger: halvo_coord::types::WorkflowTrigger::Manual,
            period: None,
            last: DAY,
            agent: Some(agent(0)),
        },
        "paused, with the trigger manual and no period (ruling XB-28 (a))"
    );
}

#[test]
fn review_w9_an_owed_run_ended_that_the_checks_read_brought_in_is_stored_once() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.ports.store_end(
        &agent(0),
        PastRun {
            run: run.clone(),
            day: DAY,
            outcome: Outcome::Failed,
            unknown: 1,
        },
    );
    w.ports.refuse(Method::Log, PortError::NoService);
    relife(&mut w, &[(run.clone(), Reaped::Gone)]);
    let owed: Vec<String> = w
        .board
        .owed
        .iter()
        .map(|o| format!("{:?}", o.what))
        .collect();
    stored_outside(
        &w,
        0,
        What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Interrupted,
        },
    );
    w.ports.clear_refusals();
    let ended_count = |w: &World| {
        w.ports
            .events
            .borrow()
            .iter()
            .filter(|e| matches!(&e.what, What::RunEnded { run: r, .. } if *r == run))
            .count()
    };
    let start = w.act(Act::Start { agent: agent(0) });
    let mut log = vec![
        format!("owed after recovery: {owed:?}"),
        format!(
            "person's start: {:?}, checked {:?}, run_ended stored {}",
            start.report.answer,
            start.checked.len(),
            ended_count(&w)
        ),
    ];
    for k in 1..3u64 {
        w.set_clock(T0 + 10_000 * k, DAY);
        w.act(Act::Tick);
    }
    log.push(format!(
        "after two ticks: run_ended stored {}, owed {}",
        ended_count(&w),
        w.board.owed.len()
    ));
    println!("W9 {}", log.join(" | "));
    assert_eq!(ended_count(&w), 1, "W9 {}", log.join(" | "));
}

#[test]
fn review_w5_owed_lapse_after_a_resync_of_its_body_is_stored_once() {
    let item = ItemId::parse(&format!("{:032x}", 0x7505)).unwrap();
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    stored_outside(
        &w,
        0,
        What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    );
    w.ports.refuse(Method::Log, PortError::NoService);
    relife(&mut w, &[(run.clone(), Reaped::Gone)]);
    let owed: Vec<String> = w
        .board
        .owed
        .iter()
        .map(|o| format!("{:?}", o.what))
        .collect();
    stored_outside(
        &w,
        0,
        What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Interrupted,
        },
    );
    stored_outside(
        &w,
        0,
        What::ItemLapsed {
            item: item.clone(),
            lapsed: Lapsed::Ready(OtherLapse::AppEnded),
        },
    );
    w.ports.clear_refusals();
    for k in 1..3 {
        w.set_clock(T0 + k * 10_000, DAY);
        w.act(Act::Tick);
    }
    let ended = w
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| matches!(&e.what, What::RunEnded { run: r, .. } if *r == run))
        .count();
    println!(
        "W5 owed after recovery: {owed:?} | run_ended of the run: {ended} | item_lapsed of the item: {} | owed after two ticks: {}",
        lapses_of(&w, &item),
        w.board.owed.len()
    );
    assert_eq!(ended, 1, "one run_ended");
    assert_eq!(lapses_of(&w, &item), 1, "I5.8, L4: the item lapses once");
}

/// A new life whose recovery runs on `turns`, one for each log of recovery.
fn relife_with(w: &mut World, turns: Vec<Turn>) {
    w.ports.new_life();
    w.forget();
    let life = recover(&Late::new(&w.ports, turns), &w.limits).expect("recovery");
    w.board = life.board;
    w.table = Table::new();
}

fn ends_of(w: &World, run: &RunId) -> usize {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| matches!(&e.what, What::RunEnded { run: r, .. } if r == run))
        .count()
}

fn two_ticks(w: &mut World) {
    for k in 1..3 {
        w.set_clock(T0 + k * 10_000, DAY);
        w.act(Act::Tick);
    }
}

#[test]
fn a_recovery_body_of_a_run_and_its_item_stored_with_a_lost_acknowledgement_is_stored_once() {
    let mut w = world(&[MANUAL]);
    let run = RunId::parse(&format!("{:032x}", 0xc001)).unwrap();
    let item = ItemId::parse(&format!("{:032x}", 0xd001)).unwrap();
    stored_outside(
        &w,
        0,
        What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Person,
            slot: None,
        },
    );
    stored_outside(
        &w,
        0,
        What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        },
    );
    relife_with(&mut w, vec![Turn::Lose]);
    assert_eq!(
        (ends_of(&w, &run), lapses_of(&w, &item)),
        (1, 1),
        "the owner stored the body"
    );
    assert_eq!(
        w.board
            .owed
            .iter()
            .filter(|o| !matches!(o.what, halvo_coord::schedule::Owing::EndRun { .. }))
            .count(),
        2,
        "its run_ended and its item_lapsed are owed"
    );
    two_ticks(&mut w);
    assert_eq!(
        (ends_of(&w, &run), lapses_of(&w, &item)),
        (1, 1),
        "the verifier's case x2: one run_ended and one item_lapsed (I5.8, L4)"
    );
    assert!(w.board.owed.is_empty());
}

#[test]
fn a_recovery_body_of_two_runs_stored_with_a_lost_acknowledgement_ends_each_run_once() {
    let mut w = world(&[MANUAL]);
    let runs: Vec<RunId> = [0xc001, 0xc002]
        .iter()
        .map(|n| RunId::parse(&format!("{n:032x}")).unwrap())
        .collect();
    for run in &runs {
        stored_outside(
            &w,
            0,
            What::RunStarted {
                run: run.clone(),
                trigger: Trigger::Person,
                slot: None,
            },
        );
    }
    relife_with(&mut w, vec![Turn::Lose]);
    two_ticks(&mut w);
    assert_eq!(
        runs.iter().map(|r| ends_of(&w, r)).collect::<Vec<_>>(),
        vec![1, 1],
        "the verifier's case x3: each run ends once"
    );
    assert!(w.board.owed.is_empty());
}

/// The items of the kind `check` of agent 0 that the owner holds open.
fn open_checks(w: &World) -> Vec<ItemId> {
    let events = w.ports.events.borrow();
    events
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemRaised {
                item,
                raised: Raised::Check(_),
            } if e.agent == agent(0) => Some(item.clone()),
            _ => None,
        })
        .filter(|item| {
            !events.iter().any(|e| match &e.what {
                What::ItemLapsed { item: i, .. }
                | What::ItemAnswered { item: i, .. }
                | What::ItemReleased { item: i, .. } => i == item,
                _ => false,
            })
        })
        .collect()
}

#[test]
fn a_check_stored_with_a_lost_acknowledgement_beside_an_owed_lapse_is_raised_once() {
    let mut w = world(&[MANUAL]);
    let first = w.ports.agents.borrow()[0].last.clone().unwrap().run;
    w.ports.store_end(
        &agent(0),
        PastRun {
            run: first,
            day: DAY,
            outcome: Outcome::Completed,
            unknown: 1,
        },
    );
    w.act(Act::Tick);
    assert_eq!(open_checks(&w).len(), 1, "the first life raises the check");
    relife_with(&mut w, vec![Turn::Lose, Turn::Lose]);
    let open = open_checks(&w);
    assert_eq!(
        open.len(),
        1,
        "the owner stored the lapse of the first check and a second check item"
    );
    assert_eq!(w.board.owed.len(), 1, "the lapse is owed");
    assert_eq!(
        w.facts(0).doing,
        halvo_coord::types::Doing::Idle(None),
        "the board does not hold the check item whose acknowledgement was lost"
    );
    two_ticks(&mut w);
    assert_eq!(
        open_checks(&w),
        open,
        "the verifier's case x1: no check item is raised a second time (RD-12 (b))"
    );
    assert_eq!(
        w.facts(0).doing,
        halvo_coord::types::Doing::Idle(Some(halvo_coord::types::OwnItem {
            item: open[0].clone(),
            kind: halvo_coord::types::OwnItemKind::Check,
        })),
        "the board holds the open check item"
    );
    assert!(w.board.owed.is_empty());
}

#[test]
fn a_debts_read_takes_up_a_check_item_that_the_owner_stored_with_a_lost_acknowledgement() {
    let mut w = world(&[MANUAL]);
    let first = w.ports.agents.borrow()[0].last.clone().unwrap().run;
    w.ports.store_end(
        &agent(0),
        PastRun {
            run: first,
            day: DAY,
            outcome: Outcome::Completed,
            unknown: 1,
        },
    );
    w.act(Act::Tick);
    let earlier = open_checks(&w);
    assert_eq!(earlier.len(), 1, "the first life raises the check");
    relife_with(&mut w, vec![Turn::Refuse(PortError::NoService), Turn::Lose]);
    let open = open_checks(&w);
    assert_eq!(
        open.len(),
        2,
        "the owner refused the body and stored the new check item"
    );
    let stored = open.iter().find(|i| !earlier.contains(i)).unwrap().clone();
    assert_eq!(
        w.board.owed.len(),
        1,
        "the lapse of the earlier check is owed"
    );
    assert_eq!(w.facts(0).doing, halvo_coord::types::Doing::Idle(None));
    let held = halvo_coord::types::Doing::Idle(Some(halvo_coord::types::OwnItem {
        item: stored.clone(),
        kind: halvo_coord::types::OwnItemKind::Check,
    }));
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(
        w.facts(0).doing,
        held,
        "the lapse's log is refused with seq, and its read takes up the open check item (RD-12 (b))"
    );
    w.set_clock(T0 + 20_000, DAY);
    w.act(Act::Tick);
    assert_eq!(
        open_checks(&w),
        vec![stored],
        "the earlier check lapsed and no check item is raised a second time"
    );
    assert_eq!(w.facts(0).doing, held);
    assert!(w.board.owed.is_empty());
}

#[test]
fn an_owed_run_ended_refused_with_seq_for_another_event_is_logged_after_it() {
    let mut w = world(&[MANUAL]);
    let earlier = RunId::parse(&format!("{:032x}", 0xc101)).unwrap();
    let run = RunId::parse(&format!("{:032x}", 0xc102)).unwrap();
    for what in [
        What::RunStarted {
            run: earlier.clone(),
            trigger: Trigger::Person,
            slot: None,
        },
        What::RunEnded {
            run: earlier.clone(),
            cause: RunEnd::Completed,
        },
        What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Person,
            slot: None,
        },
    ] {
        stored_outside(&w, 0, what);
    }
    w.ports.refuse(Method::Log, PortError::NoService);
    relife(&mut w, &[]);
    w.ports.clear_refusals();
    assert!(
        w.board.owed.iter().any(|o| matches!(
            &o.what,
            halvo_coord::schedule::Owing::RunEnded { run: r, .. } if *r == run
        )),
        "the run_ended of the newer run is owed"
    );
    stored_outside(&w, 0, What::SlotSkipped { slot: None });
    two_ticks(&mut w);
    assert_eq!(
        ends_of(&w, &run),
        1,
        "the owed run_ended is logged after the other event, not paid by the end of another run"
    );
    assert!(!w
        .board
        .owed
        .iter()
        .any(|o| matches!(o.what, halvo_coord::schedule::Owing::RunEnded { .. })));
}

#[test]
fn a_run_ended_debts_read_takes_up_a_check_item_that_the_owner_stored_with_a_lost_acknowledgement()
{
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.ports.store_end(
        &agent(0),
        PastRun {
            run: run.clone(),
            day: DAY,
            outcome: Outcome::Failed,
            unknown: 1,
        },
    );
    relife_with(&mut w, vec![Turn::Refuse(PortError::NoService), Turn::Lose]);
    let open = open_checks(&w);
    assert_eq!(open.len(), 1, "the owner stored the check item");
    assert!(
        w.board.owed.iter().any(|o| matches!(
            &o.what,
            halvo_coord::schedule::Owing::RunEnded { run: r, .. } if *r == run
        )),
        "the run_ended of the refused body is owed"
    );
    let held = halvo_coord::types::Doing::Idle(Some(halvo_coord::types::OwnItem {
        item: open[0].clone(),
        kind: halvo_coord::types::OwnItemKind::Check,
    }));
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    assert_eq!(
        w.facts(0).doing,
        held,
        "the run_ended's log is refused with seq, and its read takes up the open check item (RD-12 (b))"
    );
    w.set_clock(T0 + 20_000, DAY);
    w.act(Act::Tick);
    assert_eq!(
        open_checks(&w),
        open,
        "no check item is raised a second time"
    );
    assert_eq!(ends_of(&w, &run), 1);
    assert!(w.board.owed.is_empty());
}

#[test]
fn review_m1_a_read_after_seq_takes_up_no_item_for_an_agent_that_holds_one() {
    const PERSON: A = A {
        person: true,
        slotted: true,
    };
    let mut w = world(&[PERSON]);
    w.act(Act::Tick);
    let before = format!("{:?}", w.facts(0).doing);
    assert!(before.contains("Ready"), "{before}");
    let run = RunId::parse(&format!("{:032x}", 0x7b01)).unwrap();
    w.board.owed.push(halvo_coord::schedule::Owed {
        agent: agent(0),
        workflow: flow(0),
        what: halvo_coord::schedule::Owing::RunEnded {
            run: run.clone(),
            cause: RunEnd::Interrupted,
            day: DAY,
            at: 0,
        },
    });
    stored_outside(
        &w,
        0,
        What::ItemRaised {
            item: ItemId::parse(&format!("{:032x}", 0x7b02)).unwrap(),
            raised: Raised::Check(None),
        },
    );
    w.set_clock(T0 + 10_000, DAY);
    w.act(Act::Tick);
    let after = format!("{:?}", w.facts(0).doing);
    println!("M1 before {before} | after the tick {after}");
    assert!(after.contains("Ready"), "M1 after the tick {after}");
}

/// The end of the local day `DAY` in the zone of `world`.
const MIDNIGHT: u64 = T0 + 6_400_000;

/// The `run_ended` row that recovery logs for a run of an earlier life, with the clock from 8 ms
/// before local midnight to 3 ms after it and moving on 1 ms at each read, carries the local day
/// of its `at` (I7.1).
#[test]
fn the_rows_of_recovery_carry_the_day_of_their_at_across_midnight() {
    let mut days = Vec::new();
    for start in MIDNIGHT - 8..MIDNIGHT + 4 {
        let mut w = world(&[MANUAL]);
        let run = started(&mut w, 0);
        w.ports.set_now(start);
        w.ports.step_reads(1);
        let ended = relife(&mut w, &[(run.clone(), Reaped::Gone)]);
        assert_eq!(
            ended,
            vec![(run.clone(), RunEnd::Interrupted)],
            "from {start}"
        );
        let events = w.ports.events.borrow();
        let rows: Vec<&Event> = events
            .iter()
            .filter(|e| matches!(&e.what, What::RunEnded { run: r, .. } if r == &run))
            .collect();
        assert_eq!(rows.len(), 1, "from {start}");
        for e in rows {
            let at = u64::try_from(e.at / 1_000_000).unwrap();
            assert_eq!(w.ports.day(at), Ok(e.day), "from {start}");
            days.push(e.day);
        }
    }
    assert!(days.contains(&DAY) && days.contains(&(DAY + 1)));
}
