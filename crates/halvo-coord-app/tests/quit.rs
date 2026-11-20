mod support;

use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::types::{Action, AgentId, Cause, Limits, Outcome, RunEvent, RunId, What};
use halvo_coord_app::{Core, Hub};
use std::sync::Arc;
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};
use support::{
    agent, calls_since, events, mark, started, world, Counts, Dialogs, Hold, Shared, T0,
};

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

type Named = Vec<(AgentId, RunId)>;

fn quit_named(hub: &Arc<Hub<Shared>>, budget: Option<Duration>) -> JoinHandle<(Duration, Named)> {
    let hub = hub.clone();
    std::thread::spawn(move || {
        let start = Instant::now();
        let named = hub.quit(budget);
        (start.elapsed(), named)
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

#[test]
fn u46_a_kill_held_26_s_and_refused_no_longer_holds_quit_and_its_run_is_named() {
    let hub = hub(1);
    let run = live_run(&hub);
    let ports = hub.coordinator().ports().inner();
    ports.hold_kills(Hold::For(ms(26_000)));
    let (took, named) = quit_named(&hub, None).join().unwrap();
    assert!(
        took >= ms(1_900) && took < ms(2_900),
        "cancel_grace_s = 1 s of wait for the run, then at most 1 s for the second step: {took:?}"
    );
    assert_eq!(
        named,
        [(agent(0), run.clone())],
        "the run that did not stop, with its agent"
    );
    let recorded: Vec<RunId> = ports.recorded_kills().into_iter().map(|(r, _)| r).collect();
    assert_eq!(
        recorded,
        [run],
        "the recorder took the kill and ended nothing"
    );
}

#[test]
fn u46_a_kill_that_never_returns_does_not_hold_quit_past_its_bound_and_names_the_run_meanwhile() {
    let hub = hub(1);
    let run = live_run(&hub);
    let ports = hub.coordinator().ports().inner();
    ports.hold_kills(Hold::Released);
    let quitting = quit_named(&hub, Some(ms(100)));
    let t0 = Instant::now();
    while !quitting.is_finished() && t0.elapsed() < ms(6_000) {
        sleep(ms(10));
    }
    let returned = quitting.is_finished();
    let holding = ports.kills_holding();
    let meanwhile = hub.coordinator().not_stopped();
    let probe = {
        let hub = hub.clone();
        std::thread::spawn(move || hub.coordinator().snapshot())
    };
    sleep(ms(100));
    let lock_held = !probe.is_finished();
    ports.release_kills();
    let (took, named) = quitting.join().unwrap();
    probe.join().unwrap();
    assert!(returned, "the hook returned while the kill still held");
    assert!(
        took >= ms(1_000) && took < ms(1_900),
        "a budget of 100 ms, then at most cancel_grace_s = 1 s for the second step: {took:?}"
    );
    assert_eq!(holding, 1, "the kill still held when the hook returned");
    assert!(lock_held, "the held kill holds the coordinator's lock");
    assert_eq!(named, [(agent(0), run.clone())]);
    assert_eq!(
        meanwhile, named,
        "the list answers while the kill holds the coordinator's lock"
    );
    assert_eq!(
        hub.coordinator().live(),
        1,
        "the refused kill leaves the run live"
    );
}

#[test]
fn u46_a_quit_whose_kill_succeeds_names_no_run_and_waits_out_no_bound() {
    let hub = hub(1);
    let run = live_run(&hub);
    let m = mark(&hub);
    let (took, named) = quit_named(&hub, Some(ms(100))).join().unwrap();
    assert!(
        took < ms(900),
        "the hook returns once the second step has returned: {took:?}"
    );
    assert_eq!(kills(&hub, m), [run]);
    assert!(named.is_empty(), "{named:?}");
}

struct Release(Arc<Hub<Shared>>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.coordinator().ports().inner().release_kills();
    }
}

fn hub_of(agents: usize, max_live_runs: u32, grace_s: u64) -> Arc<Hub<Shared>> {
    let limits = Limits {
        cancel_grace_s: grace_s,
        max_live_runs,
        ..Limits::PRODUCT
    };
    let hub = Hub::new(
        world(agents, limits),
        limits,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    hub.tick();
    hub
}

fn live_run_of(hub: &Hub<Shared>, n: usize) -> RunId {
    let c = hub.coordinator();
    c.control(&agent(n), Action::RunNow).unwrap();
    let run = started(hub).pop().unwrap();
    c.on_run(&run, RunEvent::Hello).unwrap();
    run
}

fn finished_by<T>(call: &JoinHandle<T>, called: Instant, limit: Duration) -> bool {
    while !call.is_finished() && called.elapsed() < limit {
        sleep(ms(10));
    }
    call.is_finished()
}

#[test]
fn u46_a_tick_whose_kill_holds_during_the_wait_does_not_hold_quit_past_its_bound() {
    let hub = hub_of(2, 2, 2);
    let _release = Release(hub.clone());
    let c = hub.coordinator();
    let a = live_run_of(&hub, 0);
    let b = live_run_of(&hub, 1);
    c.control(&agent(0), Action::Stop).unwrap();
    c.ports().inner().with(|p| p.end_runner(&a));
    c.on_run(&a, RunEvent::Ended(Some(Outcome::Completed)))
        .unwrap();
    let ports = c.ports().inner();
    ports.hold_kills(Hold::Released);
    let called = Instant::now();
    let quitting = quit_named(&hub, None);
    sleep(ms(500));
    ports.with(|p| p.set_now(T0 + 2_000));
    let ticker = {
        let hub = hub.clone();
        std::thread::spawn(move || hub.tick())
    };
    let returned = finished_by(&quitting, called, ms(4_900));
    let holding = ports.kills_holding();
    ports.release_kills();
    let (took, named) = quitting.join().unwrap();
    ticker.join().unwrap();
    assert!(
        returned,
        "2 x cancel_grace_s = 4 s and a margin: the hook had not returned while the tick's kill \
         of A's tree held"
    );
    assert!(took >= ms(3_900) && took < ms(4_900), "{took:?}");
    assert_eq!(
        holding, 1,
        "the tick's kill still held when the hook returned"
    );
    assert_eq!(
        named,
        [(agent(0), a), (agent(1), b)],
        "A's dying tree, whose kill holds, and B"
    );
}

#[test]
fn u46_a_quit_during_a_ticks_retry_of_a_stuck_kill_returns_within_its_bound() {
    let hub = hub(1);
    let _release = Release(hub.clone());
    let run = live_run(&hub);
    let ports = hub.coordinator().ports().inner();
    ports.with(|p| {
        p.workflows.borrow_mut().clear();
        p.refuse(Method::Kill, PortError::NoService);
    });
    hub.tick();
    ports.with(|p| p.set_now(T0 + 1_000));
    hub.tick();
    ports.with(|p| p.clear_refusals());
    ports.hold_kills(Hold::Released);
    let poller = {
        let hub = hub.clone();
        std::thread::spawn(move || hub.tick())
    };
    let waited = Instant::now();
    while ports.kills_holding() == 0 && waited.elapsed() < ms(5_000) {
        sleep(ms(10));
    }
    assert_eq!(
        ports.kills_holding(),
        1,
        "the tick retries the stuck kill and the recorder holds it"
    );
    let called = Instant::now();
    let quitting = quit_named(&hub, Some(ms(100)));
    let returned = finished_by(&quitting, called, ms(1_900));
    ports.release_kills();
    poller.join().unwrap();
    let (took, named) = quitting.join().unwrap();
    assert!(
        returned,
        "cancel_grace_s = 1 s for the first step and a margin: the hook had not returned while \
         the tick's retry held the coordinator's lock"
    );
    assert!(took < ms(1_900), "{took:?}");
    assert_eq!(named, [(agent(0), run)]);
}

#[test]
fn u46_a_second_quit_during_the_first_quits_wait_does_not_hold_it_past_its_bound() {
    let hub = hub(1);
    let _release = Release(hub.clone());
    let run = live_run(&hub);
    let ports = hub.coordinator().ports().inner();
    ports.hold_kills(Hold::Released);
    let called = Instant::now();
    let first = quit_named(&hub, None);
    sleep(ms(100));
    let (took2, named2) = quit_named(&hub, Some(ms(100))).join().unwrap();
    let returned = finished_by(&first, called, ms(2_900));
    ports.release_kills();
    let (took1, named1) = first.join().unwrap();
    assert!(
        returned,
        "2 x cancel_grace_s = 2 s and a margin: the first call had not returned while the second \
         call's kill held"
    );
    assert!(took1 < ms(2_900), "{took1:?}");
    assert!(took2 < ms(1_900), "{took2:?}");
    assert_eq!(named1, [(agent(0), run)]);
    assert_eq!(named2, named1);
}

#[test]
fn u46_a_second_quit_after_a_held_kill_returns_within_its_bound_and_names_the_run() {
    let hub = hub(1);
    let _release = Release(hub.clone());
    let run = live_run(&hub);
    let ports = hub.coordinator().ports().inner();
    ports.hold_kills(Hold::Released);
    let (took1, named1) = quit_named(&hub, Some(ms(100))).join().unwrap();
    assert_eq!(
        ports.kills_holding(),
        1,
        "the first call's kill still holds"
    );
    let called = Instant::now();
    let second = quit_named(&hub, Some(ms(100)));
    let returned = finished_by(&second, called, ms(1_900));
    ports.release_kills();
    let (took2, named2) = second.join().unwrap();
    assert!(took1 < ms(1_900), "{took1:?}");
    assert!(
        returned,
        "cancel_grace_s = 1 s for the first step and a margin: the second call had not returned \
         while the first call's kill held"
    );
    assert!(took2 < ms(1_900), "{took2:?}");
    assert_eq!(named1, [(agent(0), run)]);
    assert_eq!(named2, named1);
}

#[test]
fn u46_a_clock_step_back_during_the_first_step_does_not_stretch_the_wait() {
    let hub = hub(1);
    let run = live_run(&hub);
    hub.coordinator()
        .ports()
        .inner()
        .step_clock_at_cancel(T0 - 3_000);
    let m = mark(&hub);
    let took = quit(&hub, None).join().unwrap();
    assert!(
        took >= ms(900) && took < ms(1_900),
        "cancel_grace_s = 1 s, with the clock stepped back 3 s after the first step's read: \
         {took:?}"
    );
    assert_eq!(kills(&hub, m), [run]);
}

#[test]
fn u46_a_hub_taken_out_of_its_arc_still_bounds_the_second_step() {
    let Ok(raw) = Arc::try_unwrap(hub(1)) else {
        panic!("Hub::new hands out its one Arc");
    };
    let hub = Arc::new(raw);
    let _release = Release(hub.clone());
    let run = live_run(&hub);
    let ports = hub.coordinator().ports().inner();
    ports.hold_kills(Hold::Released);
    let called = Instant::now();
    let quitting = quit_named(&hub, Some(ms(100)));
    let returned = finished_by(&quitting, called, ms(1_900));
    ports.release_kills();
    let (took, named) = quitting.join().unwrap();
    let Ok(other) = Arc::try_unwrap(hub_of(1, 1, 1)) else {
        panic!("Hub::new hands out its one Arc");
    };
    live_run(&other);
    other.coordinator().ports().inner().hold_kills(Hold::Panics);
    let unwound =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| other.quit(Some(ms(100)))))
            .is_err();
    assert!(
        returned,
        "a budget of 100 ms, then cancel_grace_s = 1 s for the second step and a margin"
    );
    assert!(took < ms(1_900), "{took:?}");
    assert_eq!(named, [(agent(0), run)]);
    assert!(!unwound, "a kill's panic does not unwind through the hook");
}
