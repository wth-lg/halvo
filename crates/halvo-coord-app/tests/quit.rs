mod support;

use halvo_coord::mem::Call;
use halvo_coord::types::{Action, Cause, Limits, Outcome, RunEvent, RunId, What};
use halvo_coord_app::{Core, Hub};
use std::sync::Arc;
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};
use support::{agent, calls_since, events, mark, started, world, Counts, Dialogs, Shared, T0};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn hub(grace_s: u64) -> Arc<Hub<Shared>> {
    let limits = Limits {
        cancel_grace_s: grace_s,
        ..Limits::PRODUCT
    };
    let hub = Hub::new(
        world(1, limits),
        limits,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    hub.tick();
    hub
}

fn live_run(hub: &Hub<Shared>) -> RunId {
    let c = hub.coordinator();
    c.control(&agent(0), Action::RunNow).unwrap();
    let run = started(hub).pop().unwrap();
    c.on_run(&run, RunEvent::Hello).unwrap();
    run
}

fn quit(hub: &Arc<Hub<Shared>>, budget: Option<Duration>) -> JoinHandle<Duration> {
    let hub = hub.clone();
    std::thread::spawn(move || {
        let start = Instant::now();
        hub.quit(budget);
        start.elapsed()
    })
}

fn kills(hub: &Hub<Shared>, from: usize) -> Vec<RunId> {
    calls_since(hub, from)
        .iter()
        .filter_map(|c| match c {
            Call::Kill(r) => Some(r.clone()),
            _ => None,
        })
        .collect()
}

fn ended(hub: &Hub<Shared>) -> usize {
    events(hub)
        .iter()
        .filter(|e| matches!(e.what, What::RunEnded { .. }))
        .count()
}

#[test]
fn with_no_live_run_the_hook_takes_the_second_step_at_once_and_starts_refuse() {
    let hub = hub(5);
    let took = quit(&hub, None).join().unwrap();
    assert!(took < ms(2_000), "{took:?}");
    assert_eq!(
        hub.coordinator().control(&agent(0), Action::RunNow),
        Err(Cause::State)
    );
    assert!(started(&hub).is_empty());
}

#[test]
fn the_hook_waits_for_the_run_that_obeys_its_cancel_with_no_move_held_open() {
    let hub = hub(5);
    let run = live_run(&hub);
    let m = mark(&hub);
    let quitting = quit(&hub, None);
    sleep(ms(1_300));
    assert!(
        calls_since(&hub, m).contains(&Call::Cancel(run.clone())),
        "the first step sent cancel"
    );
    assert!(kills(&hub, m).is_empty(), "no kill before the run ends");
    assert!(!quitting.is_finished());
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.end_runner(&run));
    let k = mark(&hub);
    assert_eq!(
        hub.coordinator()
            .on_run(&run, RunEvent::Ended(Some(Outcome::Completed))),
        Ok(()),
        "the run's end is taken while the hook waits"
    );
    let took = quitting.join().unwrap();
    assert!(took < ms(3_000), "{took:?}");
    assert_eq!(
        kills(&hub, k),
        [run],
        "the second step kills the dying tree"
    );
    assert_eq!(hub.coordinator().live(), 0);
}

#[test]
fn a_run_that_does_not_obey_its_cancel_is_killed_at_the_grace_with_nothing_stored() {
    let hub = hub(1);
    let run = live_run(&hub);
    let before = ended(&hub);
    let m = mark(&hub);
    let took = quit(&hub, None).join().unwrap();
    assert!(took >= ms(900) && took < ms(3_000), "{took:?}");
    assert_eq!(kills(&hub, m), [run]);
    assert_eq!(hub.coordinator().live(), 0);
    assert_eq!(
        ended(&hub),
        before,
        "the kill at the quit stores no run_ended"
    );
}

#[test]
fn where_the_system_gives_the_quit_less_time_the_second_step_comes_then() {
    let hub = hub(5);
    let run = live_run(&hub);
    let m = mark(&hub);
    let took = quit(&hub, Some(ms(300))).join().unwrap();
    assert!(took >= ms(250) && took < ms(2_500), "{took:?}");
    assert_eq!(kills(&hub, m), [run]);
}

#[test]
fn for_a_run_stopped_before_the_quit_the_grace_counts_from_its_stop() {
    let hub = hub(2);
    let run = live_run(&hub);
    hub.coordinator().control(&agent(0), Action::Stop).unwrap();
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.set_now(T0 + 1_700));
    let m = mark(&hub);
    let took = quit(&hub, None).join().unwrap();
    assert!(took >= ms(250) && took < ms(1_200), "{took:?}");
    assert_eq!(kills(&hub, m), [run]);
}

#[test]
fn the_grace_counts_from_the_end_of_a_slow_first_step() {
    let hub = hub(3);
    live_run(&hub);
    let m = mark(&hub);
    let holder = {
        let hub = hub.clone();
        std::thread::spawn(move || hub.coordinator().ports().inner().with(|_| sleep(ms(2_000))))
    };
    sleep(ms(50));
    let t0 = Instant::now();
    let quitting = quit(&hub, None);
    let (mut cancel_at, mut kill_at) = (None, None);
    while kill_at.is_none() && t0.elapsed() < ms(12_000) {
        let calls = calls_since(&hub, m);
        if cancel_at.is_none() && calls.iter().any(|c| matches!(c, Call::Cancel(_))) {
            cancel_at = Some(t0.elapsed());
        }
        if calls.iter().any(|c| matches!(c, Call::Kill(_))) {
            kill_at = Some(t0.elapsed());
        }
        sleep(ms(10));
    }
    holder.join().unwrap();
    quitting.join().unwrap();
    let (cancel, kill) = (cancel_at.unwrap(), kill_at.unwrap());
    assert!(
        kill - cancel >= ms(2_000),
        "the kill came {:?} after the cancel, less than cancel_grace_s = 3 s less a second",
        kill - cancel
    );
}

#[test]
fn a_clock_that_fails_after_the_first_step_leaves_the_whole_grace() {
    let hub = hub(1);
    let run = live_run(&hub);
    hub.coordinator().ports().inner().fail_clock_at_cancel();
    let m = mark(&hub);
    let took = quit(&hub, None).join().unwrap();
    assert!(took >= ms(900) && took < ms(3_000), "{took:?}");
    assert_eq!(kills(&hub, m), [run]);
    assert!(
        halvo_coord::ports::Clock::now(hub.coordinator().ports()).is_err(),
        "the clock failed"
    );
}

#[test]
fn the_budget_counts_from_the_hooks_start_through_a_slow_first_step() {
    let hub = hub(30);
    let run = live_run(&hub);
    let m = mark(&hub);
    let holder = {
        let hub = hub.clone();
        std::thread::spawn(move || hub.coordinator().ports().inner().with(|_| sleep(ms(2_000))))
    };
    sleep(ms(50));
    let took = quit(&hub, Some(ms(3_000))).join().unwrap();
    holder.join().unwrap();
    assert!(
        took >= ms(2_900) && took < ms(4_000),
        "the budget of 3 s counts from the hook's start, not from the end of a 2 s first step: \
         {took:?}"
    );
    assert_eq!(kills(&hub, m), [run]);
}
