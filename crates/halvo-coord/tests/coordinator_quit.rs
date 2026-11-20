#[path = "run_support.rs"]
mod support;

#[allow(clippy::duplicate_mod)]
#[path = "coordinator_support.rs"]
mod coord;

use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::{PortError, Reaped};
use halvo_coord::run::{Act, StopBy};
use halvo_coord::schedule::{Answer, Stop};
use halvo_coord::types::{
    Action, AgentId, Body, Doing, HandbackAsk, HandbackReason, Limits, Live, Outcome, RunEnd,
    RunEvent, RunId, What,
};
use halvo_coord::{words, Coordinator};
use std::panic::{catch_unwind, AssertUnwindSafe};
use support::{agent, world_with, World, MANUAL, T0};

fn four() -> (World, Vec<RunId>) {
    let limits = Limits {
        max_live_runs: 3,
        ..Limits::PRODUCT
    };
    let mut w = world_with(&[MANUAL, MANUAL, MANUAL, MANUAL], limits);
    let mut runs = Vec::new();
    for n in 0..3 {
        runs.push(w.act(Act::Start { agent: agent(n) }).report.run.unwrap());
    }
    w.act(Act::Hello {
        run: runs[0].clone(),
    });
    w.act(Act::Hello {
        run: runs[2].clone(),
    });
    let stop = w.act(Act::Stop {
        run: runs[2].clone(),
        by: StopBy::Person,
    });
    assert_eq!(stop.report.answer, Ok(Answer::Ok));
    let queued = w.act(Act::Start { agent: agent(3) });
    assert_eq!(queued.report.answer, Ok(Answer::Queued));
    w.forget();
    (w, runs)
}

fn cancelled(w: &World) -> Vec<RunId> {
    w.ports
        .calls
        .borrow()
        .iter()
        .filter_map(|t| match &t.call {
            Call::Cancel(run) => Some(run.clone()),
            _ => None,
        })
        .collect()
}

fn killed(w: &World) -> Vec<RunId> {
    w.ports
        .tried
        .borrow()
        .iter()
        .filter_map(|t| match &t.call {
            Call::Kill(run) => Some(run.clone()),
            _ => None,
        })
        .collect()
}

fn ended_rows(w: &World) -> usize {
    w.ports
        .events
        .borrow()
        .iter()
        .filter(|e| matches!(e.what, What::RunEnded { .. }))
        .count()
}

#[test]
fn the_first_step_cancels_every_live_run_once_and_empties_the_queue() {
    let (mut w, runs) = four();
    let before = cancelled(&w);
    let done = w.act(Act::StopAll);
    assert_eq!(done.report.answer, Ok(Answer::Ok));
    let sent: Vec<RunId> = cancelled(&w)[before.len()..].to_vec();
    assert_eq!(sent, [runs[0].clone(), runs[1].clone()]);
    assert_eq!(done.report.stopped, [runs[0].clone(), runs[1].clone()]);
    for r in &w.board.runs {
        assert!(r.stop.is_some(), "{r:?}");
    }
    let stop = w.board.runs[0].stop.unwrap();
    assert_eq!(
        stop,
        Stop {
            at: T0,
            cause: RunEnd::Cancelled
        }
    );
    assert!(w.board.queue.is_empty());
    assert_eq!(w.facts(3).doing, Doing::Idle(None));
    assert!(w.table.is_stopped(&runs[0]) && w.table.is_stopped(&runs[1]));
    w.ports.end_runner(&runs[0]);
    let end = w.act(Act::End {
        run: runs[0].clone(),
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(end.report.ended[0].cause, RunEnd::Cancelled);
    assert!(
        end.report.ended[0].next.is_empty(),
        "no start leaves the queue"
    );
}

#[test]
fn the_first_step_stops_a_waiting_run_too_whose_item_the_caller_lapsed() {
    let mut w = world_with(&[MANUAL], Limits::PRODUCT);
    let run = w.act(Act::Start { agent: agent(0) }).report.run.unwrap();
    w.act(Act::Hello { run: run.clone() });
    w.act(Act::Wait {
        run: run.clone(),
        item: halvo_coord::types::RunItem {
            item: halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x77)).unwrap(),
            kind: halvo_coord::types::RunItemKind::Stuck,
        },
    });
    let before = cancelled(&w).len();
    let done = w.act(Act::StopAll);
    assert_eq!(done.report.stopped, std::slice::from_ref(&run));
    assert_eq!(w.facts(0).doing, Doing::Live(Live::Stopping));
    assert_eq!(cancelled(&w)[before..], *std::slice::from_ref(&run));
    let again = w.act(Act::StopAll);
    assert!(again.report.stopped.is_empty(), "a stop sends cancel once");
    assert_eq!(cancelled(&w).len(), before + 1);
}

#[test]
fn the_second_step_kills_every_live_run_and_every_dying_tree_and_stores_no_end() {
    let (mut w, runs) = four();
    w.act(Act::StopAll);
    w.ports.end_runner(&runs[0]);
    w.act(Act::End {
        run: runs[0].clone(),
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(w.board.dying.len(), 1);
    let rows = ended_rows(&w);
    let sends = w.ports.calls.borrow().len();
    let done = w.act(Act::KillAll);
    assert_eq!(done.report.answer, Ok(Answer::Ok));
    assert_eq!(killed(&w), runs);
    assert_eq!(done.killed, runs);
    assert!(w.board.runs.is_empty());
    assert!(w.board.dying.is_empty());
    assert_eq!(
        ended_rows(&w),
        rows,
        "the kill at the quit stores no run_ended"
    );
    let after: Vec<Call> = w.ports.calls.borrow()[sends..]
        .iter()
        .map(|t| t.call.clone())
        .collect();
    assert!(
        after.iter().all(|c| matches!(c, Call::Kill(_))),
        "{after:?}"
    );
    assert!(!after
        .iter()
        .any(|c| matches!(c, Call::Send(Body::EndRun { .. }) | Call::Start(_))));
    for n in 0..4 {
        assert!(!matches!(w.facts(n).doing, Doing::Live(_)));
    }
}

#[test]
fn a_kill_that_fails_at_the_second_step_leaves_its_run_live() {
    let (mut w, runs) = four();
    w.act(Act::StopAll);
    w.ports.refuse(Method::Kill, PortError::NoService);
    let done = w.act(Act::KillAll);
    assert!(done.killed.is_empty());
    assert_eq!(w.board.runs.len(), 3);
    assert_eq!(killed(&w), runs);
}

fn run_now(c: &Coordinator<coord::Both>, n: usize) -> RunId {
    assert_eq!(c.control(&agent(n), Action::RunNow), Ok(()));
    let run = coord::started(c).last().cloned().unwrap();
    assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
    run
}

fn two_live() -> (Coordinator<coord::Both>, [RunId; 2]) {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coord::coordinator(&[coord::MANUAL, coord::MANUAL], limits);
    let runs = [run_now(&c, 0), run_now(&c, 1)];
    (c, runs)
}

fn named(pairs: &[(usize, &RunId)]) -> Vec<(AgentId, RunId)> {
    pairs
        .iter()
        .map(|(n, run)| (agent(*n), (*run).clone()))
        .collect()
}

#[test]
fn u46_the_quit_names_each_live_run_with_its_agent_and_drops_a_run_that_ends_in_the_wait() {
    let (c, runs) = two_live();
    assert_eq!(
        c.not_stopped(),
        named(&[(0, &runs[0]), (1, &runs[1])]),
        "before the quit the list names the live runs too, so that a hook whose first step \
         cannot take the lock names them"
    );
    c.quit_stop();
    assert_eq!(
        c.not_stopped(),
        named(&[(0, &runs[0]), (1, &runs[1])]),
        "after the first step every live run, in the order of the starts"
    );
    c.ports().ports.end_runner(&runs[0]);
    assert_eq!(
        c.on_run(&runs[0], RunEvent::Ended(Some(Outcome::Completed))),
        Ok(())
    );
    assert_eq!(c.not_stopped(), named(&[(1, &runs[1])]));
}

#[test]
fn u46_a_refused_kill_leaves_its_run_named_and_kills_that_succeed_name_none() {
    let (c, runs) = two_live();
    c.quit_stop();
    c.ports().ports.refuse(Method::Kill, PortError::NoService);
    c.quit_kill();
    assert_eq!(c.live(), 2);
    assert_eq!(
        c.not_stopped(),
        named(&[(0, &runs[0]), (1, &runs[1])]),
        "both kills were refused"
    );
    let (c, _) = two_live();
    c.quit_stop();
    c.quit_kill();
    assert_eq!(c.live(), 0);
    assert!(c.not_stopped().is_empty(), "{:?}", c.not_stopped());
}

#[test]
fn u46_limit_the_next_life_shows_a_run_that_outlived_quit_as_if_nothing_ran() {
    let c = coord::coordinator(&[coord::MANUAL], Limits::PRODUCT);
    let run = run_now(&c, 0);
    c.quit_stop();
    c.ports().ports.refuse(Method::Kill, PortError::NoService);
    c.quit_kill();
    assert_eq!(c.not_stopped(), named(&[(0, &run)]));
    let next = coord::owner(&[], Limits::PRODUCT);
    *next.ports.workflows.borrow_mut() = c.ports().ports.workflows.borrow().clone();
    *next.ports.agents.borrow_mut() = c.ports().ports.agents.borrow().clone();
    *next.ports.events.borrow_mut() = c.ports().ports.events.borrow().clone();
    next.ports.leave_marks(&[(run.clone(), Reaped::Alive)]);
    let life = Coordinator::new(next, Limits::PRODUCT);
    life.tick();
    let row = life.snapshot().rows[0].clone();
    println!("NEXT LIFE ROW {:?}: {}", row.status, row.line);
    let ended: Vec<RunEnd> = life
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter_map(|e| match &e.what {
            What::RunEnded { run: r, cause, .. } if r == &run => Some(*cause),
            _ => None,
        })
        .collect();
    assert_eq!(ended, [RunEnd::Interrupted]);
    assert_eq!(
        row.line,
        words::text("line.waiting_turn.store", &[]).unwrap(),
        "a named limit of U4.6: the next life ends the run that outlived Quit as interrupted, \
         keeps reap's answer alive for an agent set aside alone (run/recover.rs, recover; \
         schedule/board.rs, set_aside) and its row says that Halvo wraps up the task and the \
         person needs to do nothing, with no word that the task may have run on after Quit"
    );
}

fn hand_back(c: &Coordinator<coord::Both>, n: usize, run: &RunId) {
    assert_eq!(
        c.on_run(
            run,
            RunEvent::Handback(HandbackAsk {
                run: run.clone(),
                agent: agent(n),
                reason: HandbackReason::StepFailed,
                origin: None,
            })
        ),
        Ok(())
    );
}

fn first_step_panics_at_its_lapse(c: &Coordinator<coord::Both>) {
    c.ports().panics.set(Some("log_lapsed"));
    let unwound = catch_unwind(AssertUnwindSafe(|| c.quit_stop())).is_err();
    c.ports().panics.set(None);
    assert!(
        unwound,
        "the owner's log panicked at the first step's lapse"
    );
}

#[test]
fn u46_no_queued_run_starts_after_a_quit_whose_first_step_panicked_in_a_lapse() {
    let limits = Limits {
        max_live_runs: 1,
        ..Limits::PRODUCT
    };
    let c = coord::coordinator(&[coord::MANUAL, coord::MANUAL], limits);
    let run = run_now(&c, 0);
    hand_back(&c, 0, &run);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    assert_eq!(c.queued().len(), 1);
    first_step_panics_at_its_lapse(&c);
    c.ports().ports.end_runner(&run);
    let _ = c.on_run(&run, RunEvent::Ended(Some(Outcome::Completed)));
    assert_eq!(
        coord::started(&c),
        [run],
        "I5.7: from the first step of the quit on, no start leaves the queue"
    );
}

#[test]
fn u46_the_list_names_every_live_run_after_a_panic_in_the_first_step() {
    let (c, runs) = two_live();
    hand_back(&c, 0, &runs[0]);
    first_step_panics_at_its_lapse(&c);
    assert_eq!(
        c.not_stopped(),
        named(&[(0, &runs[0]), (1, &runs[1])]),
        "read before any other call takes the coordinator's lock"
    );
}

#[test]
fn u46_the_list_keeps_the_order_of_starts_when_a_later_agent_starts_first() {
    let limits = Limits {
        max_live_runs: 3,
        ..Limits::PRODUCT
    };
    let c = coord::coordinator(&[coord::MANUAL, coord::MANUAL, coord::MANUAL], limits);
    let first = run_now(&c, 2);
    let second = run_now(&c, 0);
    let third = run_now(&c, 1);
    c.quit_stop();
    c.ports().ports.refuse(Method::Kill, PortError::NoService);
    c.quit_kill();
    assert_eq!(
        c.not_stopped(),
        named(&[(2, &first), (0, &second), (1, &third)])
    );
}
