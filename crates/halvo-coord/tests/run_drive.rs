#[path = "run_support.rs"]
mod support;

use halvo_coord::mem::Method;
use halvo_coord::ports::PortError;
use halvo_coord::run::{Act, StopBy};
use halvo_coord::schedule::Answer;
use halvo_coord::types::{
    Cause, Doing, ItemId, Limits, Live, Name, Outcome, RunEnd, RunId, RunItem, RunItemKind,
};
use support::{agent, world, World, MANUAL, T0};

const GRACE_MS: u64 = Limits::PRODUCT.cancel_grace_s * 1000;

fn item() -> RunItem {
    RunItem {
        item: ItemId::parse(&format!("{:032x}", 0x77)).unwrap(),
        kind: RunItemKind::Confirm,
    }
}

fn started(w: &mut World, n: usize) -> RunId {
    let done = w.act(Act::Start { agent: agent(n) });
    assert_eq!(done.report.answer, Ok(Answer::Ok), "{done:?}");
    let run = done.report.run.unwrap();
    w.forget();
    run
}

fn phase(w: &World, n: usize) -> Doing {
    w.facts(n).doing.clone()
}

#[test]
fn refusal_stop_tree() {
    let mut w = world(&[MANUAL, MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::Hello { run: run.clone() });
    let stop = w.act(Act::Stop {
        run: run.clone(),
        by: StopBy::Person,
    });
    assert_eq!(stop.report.answer, Ok(Answer::Ok));
    assert_eq!(stop.report.stopped, vec![run.clone()]);
    assert_eq!(w.calls(), vec!["cancel"]);
    assert!(w.table.is_stopped(&run));
    w.set_clock(T0 + GRACE_MS - 1, support::DAY);
    w.act(Act::Tick);
    assert!(
        !w.calls().iter().any(|c| c == "kill"),
        "no kill before cancel_grace_s have passed"
    );
    w.set_clock(T0 + GRACE_MS, support::DAY);
    let tick = w.act(Act::Tick);
    assert_eq!(
        w.calls(),
        vec!["defer", "kill", "log run_ended cancelled"],
        "the kill at the first tick at which the grace has passed"
    );
    assert_eq!(tick.report.ended[0].cause, RunEnd::Cancelled);
    assert!(w.board.runs.is_empty());

    let other = started(&mut w, 1);
    w.set_clock(T0 + 2 * GRACE_MS, support::DAY);
    w.act(Act::Stop {
        run: other.clone(),
        by: StopBy::Person,
    });
    w.ports.end_runner(&other);
    let end = w.act(Act::End {
        run: other.clone(),
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(end.report.ended[0].cause, RunEnd::Cancelled);
    w.forget();
    w.set_clock(T0 + 3 * GRACE_MS, support::DAY);
    w.act(Act::Tick);
    assert_eq!(
        w.calls(),
        vec!["kill"],
        "a stopped run whose runner ended in between is killed too"
    );
}

fn failed_in_the_tick(refuse: Method) {
    let mut w = world(&[MANUAL, MANUAL]);
    let first = started(&mut w, 0);
    assert_eq!(
        w.act(Act::Start { agent: agent(1) }).report.answer,
        Ok(Answer::Queued)
    );
    w.act(Act::Stop {
        run: first,
        by: StopBy::Person,
    });
    w.ports.refuse(refuse, PortError::NoService);
    w.ports.refuse(Method::EndRun, PortError::NoService);
    w.set_clock(T0 + GRACE_MS, support::DAY);
    let tick = w.act(Act::Tick);
    let next = &tick.report.ended[0].next;
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].answer, Err(Cause::NoService));
    let starts_of_1 = tick
        .report
        .started
        .iter()
        .chain(tick.report.next.iter())
        .filter(|s| s.agent == agent(1))
        .count();
    assert_eq!(starts_of_1, 0, "no other start of that agent in the tick");
    assert!(
        !w.facts(1).store_ended,
        "its newest run is not ended by the store until the owner is read again"
    );
    w.ports.clear_refusals();
    assert_eq!(
        w.act(Act::Start { agent: agent(1) }).report.answer,
        Err(Cause::State)
    );
}

#[test]
fn a_start_that_fails_at_step_7_in_a_tick_reads_its_run_as_not_ended_by_the_store() {
    failed_in_the_tick(Method::StartRun);
}

#[test]
fn a_start_that_fails_at_step_8_in_a_tick_reads_its_run_as_not_ended_by_the_store() {
    failed_in_the_tick(Method::Start);
}

#[test]
fn hello_wait_and_resume_move_the_phase_and_every_other_pair_is_state() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    assert_eq!(
        w.act(Act::Wait {
            run: run.clone(),
            item: item()
        })
        .report
        .answer,
        Err(Cause::State)
    );
    assert_eq!(
        w.act(Act::Hello { run: run.clone() }).report.answer,
        Ok(Answer::Ok)
    );
    assert_eq!(phase(&w, 0), Doing::Live(Live::Working));
    assert_eq!(
        w.act(Act::Hello { run: run.clone() }).report.answer,
        Err(Cause::State)
    );
    w.act(Act::Wait {
        run: run.clone(),
        item: item(),
    });
    assert_eq!(phase(&w, 0), Doing::Live(Live::Waiting(item())));
    w.act(Act::Resume { run: run.clone() });
    assert_eq!(phase(&w, 0), Doing::Live(Live::Working));
    assert_eq!(
        w.act(Act::Resume { run: run.clone() }).report.answer,
        Err(Cause::State)
    );
    let unknown = RunId::parse(&format!("{:032x}", 0x99)).unwrap();
    assert_eq!(
        w.act(Act::Hello { run: unknown }).report.answer,
        Err(Cause::State)
    );
    assert!(w.calls().is_empty(), "the phases make no call");
}

#[test]
fn a_waiting_run_ends_only_once_its_item_is_closed() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::Hello { run: run.clone() });
    w.act(Act::Wait {
        run: run.clone(),
        item: item(),
    });
    let refused = w.act(Act::End {
        run: run.clone(),
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(refused.report.answer, Err(Cause::State));
    assert!(w.calls().is_empty());
    assert_eq!(phase(&w, 0), Doing::Live(Live::Waiting(item())));
    w.act(Act::Resume { run: run.clone() });
    let end = w.act(Act::End {
        run: run.clone(),
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(end.report.ended[0].cause, RunEnd::Completed);
    assert_eq!(w.calls(), vec!["log run_ended completed"]);
}

#[test]
fn a_stop_that_no_lapse_made_sends_cancel_and_stops_the_run_in_the_table_and_one_after_a_lapse_sends_nothing(
) {
    let two = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let mut w = support::world_with(&[MANUAL, MANUAL], two);
    let run = started(&mut w, 0);
    w.act(Act::Hello { run: run.clone() });
    w.act(Act::Wait {
        run: run.clone(),
        item: item(),
    });
    w.table = w.table.stop(&run);
    let stop = w.act(Act::Stop {
        run: run.clone(),
        by: StopBy::WaitLimit,
    });
    assert_eq!(stop.report.answer, Ok(Answer::Ok));
    assert!(
        w.calls().is_empty(),
        "the lapse that stopped the run in the table sent its cancel"
    );
    assert_eq!(phase(&w, 0), Doing::Live(Live::Stopping));
    assert_eq!(w.board.runs[0].stop.map(|s| s.cause), Some(RunEnd::Lapsed));

    let other = started(&mut w, 1);
    w.act(Act::Stop {
        run: other.clone(),
        by: StopBy::WaitLimit,
    });
    assert_eq!(w.calls(), vec!["cancel"]);
    assert!(w.table.is_stopped(&other));
    assert_eq!(
        w.act(Act::Stop {
            run: other,
            by: StopBy::Person
        })
        .report
        .answer,
        Err(Cause::State),
        "a stopping run is not stopped again"
    );
}

#[test]
fn the_switch_off_control_stops_the_run_in_the_guards_table() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    let off = w.act(Act::SwitchOff { agent: agent(0) });
    assert_eq!(off.report.stopped, vec![run.clone()]);
    assert!(w.table.is_stopped(&run));
    assert_eq!(
        w.calls(),
        vec!["log hold_set person", "cancel", "set_state paused"],
        "the hold before anything else"
    );
}

#[test]
fn a_kill_that_fails_is_tried_again_at_every_tick_and_the_run_stays_live() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::Stop {
        run: run.clone(),
        by: StopBy::Person,
    });
    w.ports.refuse(Method::Kill, PortError::Busy);
    for k in 1..4 {
        w.forget();
        w.set_clock(T0 + k * GRACE_MS, support::DAY);
        w.act(Act::Tick);
        assert_eq!(w.calls(), vec!["defer", "kill refused"]);
        assert_eq!(phase(&w, 0), Doing::Live(Live::Stopping));
    }
    w.ports.clear_refusals();
    w.set_clock(T0 + 5 * GRACE_MS, support::DAY);
    let tick = w.act(Act::Tick);
    assert_eq!(tick.report.ended[0].cause, RunEnd::Cancelled);
}

#[test]
fn a_stopping_run_whose_kill_is_unknown_run_ends_interrupted_in_that_tick_with_no_end_run() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::Stop {
        run: run.clone(),
        by: StopBy::Person,
    });
    w.ports.refuse(
        Method::Kill,
        PortError::Refused(Name::parse("unknown_run").unwrap()),
    );
    w.forget();
    w.set_clock(T0 + GRACE_MS, support::DAY);
    let tick = w.act(Act::Tick);
    assert_eq!(tick.lost, vec![run.clone()]);
    assert_eq!(
        w.calls(),
        vec!["defer", "kill refused", "log run_ended interrupted"]
    );
    assert!(w.board.runs.is_empty() && w.board.dying.is_empty());
    assert_eq!(phase(&w, 0), Doing::Idle(None));
    w.ports.clear_refusals();
    w.set_clock(T0 + 2 * GRACE_MS, support::DAY);
    w.act(Act::Tick);
    assert!(
        !w.calls().iter().any(|c| c == "kill" || c == "end_run"),
        "no second kill and no end_run in this life"
    );
    assert_eq!(
        w.act(Act::Start { agent: agent(0) }).report.answer,
        Err(Cause::State),
        "the agent takes no start until the store has ended the run"
    );
}

#[test]
fn a_first_web_run_keeps_the_mode_compile_through_its_moves_and_its_end_sends_no_active() {
    let mut w = world(&[MANUAL]);
    let web = |playbook: &str| {
        halvo_coord::mem::Config::parse(&support::config_text(0, false).replace(
            "\"runtime\":\"graph_calendar\",\"playbook\":1",
            &format!("\"runtime\":\"web\",\"playbook\":{playbook}"),
        ))
        .unwrap()
    };
    w.ports.agents.borrow_mut()[0].config = web("null");
    w.board.members[0].facts.config = web("null");
    let run = started(&mut w, 0);
    let spec = w.ports.calls.borrow().iter().find_map(|t| match &t.call {
        halvo_coord::mem::Call::Start(s) => Some(s.clone()),
        _ => None,
    });
    assert_eq!(
        spec.map(|s| (s.mode, s.headed)),
        Some((halvo_coord::types::Mode::Compile, true))
    );
    w.act(Act::Hello { run: run.clone() });
    w.act(Act::Wait {
        run: run.clone(),
        item: item(),
    });
    assert_eq!(phase(&w, 0), Doing::Live(Live::Waiting(item())));
    w.act(Act::Resume { run: run.clone() });
    assert_eq!(phase(&w, 0), Doing::Live(Live::Working));
    w.forget();
    w.act(Act::End {
        run: run.clone(),
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(
        w.calls(),
        vec!["log run_ended completed"],
        "no set_state at the end"
    );
}

#[test]
fn review_stop_1() {
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::Stop {
        run: run.clone(),
        by: StopBy::Person,
    });
    w.ports.end_runner(&run);
    let end = w.act(Act::End {
        run: run.clone(),
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(end.report.ended[0].cause, RunEnd::Cancelled);
    assert_eq!(w.board.dying.len(), 1, "the ended run waits for its kill");
    w.ports.refuse(
        Method::Kill,
        PortError::Refused(Name::parse("unknown_run").unwrap()),
    );
    w.forget();
    w.set_clock(T0 + GRACE_MS, support::DAY);
    let tick = w.act(Act::Tick);
    assert!(
        w.calls().iter().any(|c| c == "kill refused"),
        "the kill at the first tick at which the grace has passed"
    );
    assert!(tick.lost.is_empty(), "a run that had ended is not lost");
    assert!(
        w.board.dying.is_empty(),
        "a kill refused with unknown_run is not tried again"
    );
    w.set_clock(T0 + 2 * GRACE_MS, support::DAY);
    w.act(Act::Tick);
    assert!(
        !w.calls().iter().any(|c| c.starts_with("kill")),
        "no second kill of a run that the library does not know"
    );
}

/// Whether the calls since the last read hold a kill.
fn killed(w: &mut World) -> bool {
    w.calls().iter().any(|c| c == "kill")
}

#[test]
fn a_tick_whose_clock_reads_earlier_than_a_stop_kills_the_run_and_the_tree_of_a_run_that_ended_after_it(
) {
    const HOUR: u64 = 3_600_000;
    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::Stop {
        run: run.clone(),
        by: StopBy::Person,
    });
    w.forget();
    w.act(Act::Tick);
    assert!(
        !killed(&mut w),
        "a clock that reads the instant of the stop kills nothing"
    );
    w.set_clock(T0 - HOUR, support::DAY);
    let tick = w.act(Act::Tick);
    assert!(
        killed(&mut w),
        "the clock stepped back one hour after the stop: the next tick kills the run"
    );
    assert_eq!(tick.report.ended[0].cause, RunEnd::Cancelled);
    assert!(w.board.runs.is_empty());

    let mut w = world(&[MANUAL]);
    let run = started(&mut w, 0);
    w.act(Act::Stop {
        run: run.clone(),
        by: StopBy::Person,
    });
    w.ports.end_runner(&run);
    w.act(Act::End {
        run,
        outcome: Some(Outcome::Completed),
    });
    assert_eq!(w.board.dying.len(), 1, "the runner ended after its stop");
    w.forget();
    w.act(Act::Tick);
    assert!(!killed(&mut w));
    w.set_clock(T0 - HOUR, support::DAY);
    w.act(Act::Tick);
    assert!(killed(&mut w), "the next tick kills the dying tree");
    assert!(w.board.dying.is_empty());

    let mut w = world(&[MANUAL]);
    started(&mut w, 0);
    w.ports
        .workflows
        .borrow_mut()
        .retain(|wf| wf.id() != &support::flow(0));
    w.forget();
    let stop = w.act(Act::Tick);
    assert_eq!(stop.report.stopped.len(), 1, "the tick stops the run");
    assert!(!killed(&mut w), "a tick never kills a run that it stopped");
    w.set_clock(T0 - HOUR, support::DAY);
    w.act(Act::Tick);
    assert!(killed(&mut w));
}
