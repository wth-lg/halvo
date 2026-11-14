#[path = "run_support.rs"]
mod support;

use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::run::{Act, StopBy};
use halvo_coord::schedule::{Answer, Stop};
use halvo_coord::types::{Body, Doing, Limits, Live, Outcome, RunEnd, RunId, What};
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
