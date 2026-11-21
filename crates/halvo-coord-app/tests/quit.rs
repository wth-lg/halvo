mod support;

use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::types::{
    Action, AgentId, Body, Cause, Limits, Outcome, RunEnd, RunEvent, RunId, What,
};
use halvo_coord_app::{Core, Hub};
use std::sync::Arc;
use std::thread::{sleep, JoinHandle};
use std::time::{Duration, Instant};
use support::{
    agent, calls_since, events, mark, started, world, Counts, Dialogs, Hold, Shared, StartGate,
    QUIT_HOOK_THREAD, T0,
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

#[test]
fn u46d_a_quit_whose_kill_holds_past_the_bound_has_written_its_list_when_the_hook_returns() {
    let hub = hub(1);
    let _release = Release(hub.clone());
    let run = live_run(&hub);
    let ports = hub.coordinator().ports().inner();
    ports.hold_kills(Hold::Released);
    let (took, named) = quit_named(&hub, Some(ms(100))).join().unwrap();
    let holding = ports.kills_holding();
    let written: Named = events(&hub)
        .iter()
        .filter_map(|e| match &e.what {
            What::RunNotStopped { run } => Some((e.agent.clone(), run.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(holding, 1, "the kill still held when the hook returned");
    assert!(took < ms(1_900), "the hook returned at its bound: {took:?}");
    assert_eq!(named, [(agent(0), run.clone())]);
    assert_eq!(
        written,
        [(agent(0), run)],
        "the owner holds the list of the runs that did not stop, as an event of its own kind"
    );
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

fn wait_until_a_kill_holds(ports: &Shared) {
    let waited = Instant::now();
    while ports.kills_holding() == 0 && waited.elapsed() < ms(5_000) {
        sleep(ms(1));
    }
    assert_eq!(ports.kills_holding(), 1, "a kill holds");
}

#[test]
fn u46_no_queued_run_starts_after_the_hooks_call_while_a_ticks_kill_holds_and_then_answers_ok() {
    let hub = hub_of(2, 1, 1);
    let _release = Release(hub.clone());
    let c = hub.coordinator();
    let first = live_run_of(&hub, 0);
    assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
    assert_eq!(c.queued().len(), 1, "agent 1 waits in the queue");
    assert_eq!(c.control(&agent(0), Action::Stop), Ok(()));
    let ports = c.ports().inner();
    ports.with(|p| p.set_now(T0 + 1_000));
    ports.hold_kills(Hold::ReleasedThenOk);
    let ticker = {
        let hub = hub.clone();
        std::thread::spawn(move || hub.tick())
    };
    wait_until_a_kill_holds(ports);
    let (took, named) = quit_named(&hub, None).join().unwrap();
    ports.release_kills();
    ticker.join().unwrap();
    assert!(
        took < ms(1_900),
        "cancel_grace_s = 1 s for the first step: {took:?}"
    );
    assert_eq!(named, [(agent(0), first.clone())]);
    assert_eq!(
        started(&hub),
        [first],
        "no run starts after the call of the hook, though the tick's kill answered Ok and its \
         end served the queue"
    );
}

#[test]
fn u46_no_queued_run_starts_after_a_panic_at_the_entry_of_the_first_step() {
    let panics = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let armed = panics.clone();
    let limits = Limits {
        max_live_runs: 1,
        cancel_grace_s: 1,
        ..Limits::PRODUCT
    };
    let hub = Hub::new(
        world(2, limits),
        limits,
        Box::new(move |_| {
            if armed.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                armed.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                panic!("the tray's notification panics");
            }
        }),
        Dialogs::default().boxed(),
    );
    hub.tick();
    let c = hub.coordinator();
    let first = live_run_of(&hub, 0);
    panics.store(2, std::sync::atomic::Ordering::SeqCst);
    let queued = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.control(&agent(1), Action::RunNow)
    }));
    assert!(
        queued.is_err(),
        "the move that queued agent 1 panicked at its notification"
    );
    let named = hub.quit(Some(ms(100)));
    assert_eq!(
        panics.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the entry of the first step panicked at its notification too"
    );
    assert_eq!(named, [(agent(0), first.clone())]);
    c.ports().inner().with(|p| p.end_runner(&first));
    let _ = c.on_run(&first, RunEvent::Ended(None));
    assert_eq!(
        started(&hub),
        [first],
        "the end of the live run starts no queued run after the call of the hook"
    );
}

fn quit_on_the_hook_thread(
    hub: &Arc<Hub<Shared>>,
    budget: Option<Duration>,
) -> JoinHandle<(Duration, Named)> {
    let hub = hub.clone();
    std::thread::Builder::new()
        .name(QUIT_HOOK_THREAD.to_string())
        .spawn(move || {
            let start = Instant::now();
            let named = hub.quit(budget);
            (start.elapsed(), named)
        })
        .unwrap()
}

#[test]
fn u46_a_ticks_kill_held_inside_the_ports_turn_does_not_hold_quit() {
    let mut late = Vec::new();
    for (budget, bound) in [(None, ms(2_900)), (Some(ms(45)), ms(1_900))] {
        let hub = hub_of(2, 2, 1);
        let _release = Release(hub.clone());
        let c = hub.coordinator();
        let a = live_run_of(&hub, 0);
        let b = live_run_of(&hub, 1);
        c.control(&agent(0), Action::Stop).unwrap();
        c.ports().inner().with(|p| p.end_runner(&a));
        c.on_run(&a, RunEvent::Ended(None)).unwrap();
        let ports = c.ports().inner();
        ports.with(|p| p.set_now(T0 + 1_000));
        ports.hold_kills(Hold::Released);
        ports.hold_kills_inside_the_turn();
        ports.gate_the_hooks_clock_read();
        let cancels = ports.cancels_made();
        let called = Instant::now();
        let quitting = quit_on_the_hook_thread(&hub, budget);
        let waited = Instant::now();
        while !ports.hook_at_its_clock_read()
            && ports.cancels_made() == cancels
            && waited.elapsed() < ms(3_000)
        {
            sleep(ms(1));
        }
        assert!(
            ports.cancels_made() > cancels,
            "the first step sent its cancel"
        );
        let ticker = {
            let hub = hub.clone();
            std::thread::spawn(move || hub.tick())
        };
        wait_until_a_kill_holds(ports);
        ports.let_the_hooks_clock_read_go();
        let returned = finished_by(&quitting, called, bound);
        ports.release_kills();
        let (took, named) = quitting.join().unwrap();
        ticker.join().unwrap();
        if !returned || took >= bound {
            late.push(format!("budget {budget:?}: past {bound:?}, took {took:?}"));
        }
        assert_eq!(named, [(agent(0), a), (agent(1), b)], "budget {budget:?}");
    }
    assert!(
        late.is_empty(),
        "a kill held inside the ports' turn held the hook: {late:?}"
    );
}

#[test]
fn u46_a_port_panic_in_the_first_step_returns_at_once_through_the_hook_with_the_live_runs() {
    let hub = hub(5);
    let run = live_run(&hub);
    hub.coordinator().ports().inner().panic_at_the_next_cancel();
    let called = Instant::now();
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hub.quit(None)));
    let took = called.elapsed();
    let Ok(named) = answer else {
        panic!("the first step's panic unwound through the hook");
    };
    assert!(
        took < ms(1_000),
        "at once, not after cancel_grace_s = 5 s: {took:?}"
    );
    assert_eq!(named, [(agent(0), run)]);
}

#[test]
fn u46_a_quit_whose_first_step_cannot_take_the_lock_sends_no_kill_and_names_every_run() {
    let hub = hub_of(3, 3, 1);
    let _release = Release(hub.clone());
    let c = hub.coordinator();
    let stopped = live_run_of(&hub, 0);
    let dying = live_run_of(&hub, 1);
    let unstopped = live_run_of(&hub, 2);
    c.control(&agent(0), Action::Stop).unwrap();
    c.control(&agent(1), Action::Stop).unwrap();
    c.ports().inner().with(|p| p.end_runner(&dying));
    c.on_run(&dying, RunEvent::Ended(None)).unwrap();
    let ports = c.ports().inner();
    ports.with(|p| {
        p.set_now(T0 + 1_000);
        p.refuse(Method::Kill, PortError::NoService);
    });
    hub.tick();
    ports.with(|p| p.clear_refusals());
    ports.hold_kills(Hold::Released);
    let ticker = {
        let hub = hub.clone();
        std::thread::spawn(move || hub.tick())
    };
    wait_until_a_kill_holds(ports);
    let before = ports.recorded_kills().len();
    let called = Instant::now();
    let quitting = quit_named(&hub, None);
    let returned = finished_by(&quitting, called, ms(1_900));
    let kills = ports.recorded_kills().len() - before;
    ports.release_kills();
    let (took, named) = quitting.join().unwrap();
    ticker.join().unwrap();
    assert!(
        returned,
        "cancel_grace_s = 1 s for the first step and a margin: the hook had not returned"
    );
    assert!(took < ms(1_900), "{took:?}");
    assert_eq!(kills, 0, "that call of the hook sent no kill to all");
    assert_eq!(
        named,
        [
            (agent(0), stopped),
            (agent(1), dying),
            (agent(2), unstopped)
        ],
        "a stopped run whose kill holds, a dying tree whose kill was refused and a live run that \
         no stop reached, in the order of their starts"
    );
}

/// Who makes the start that a test holds at a gate behind its first reading of the mark.
#[derive(Clone, Copy)]
enum Mover {
    /// The person's start of agent 0, with no run live and one live run allowed.
    Person,
    /// The queue's start of agent 1 at the end of agent 0's run, with one live run allowed.
    QueueAtAnEnd,
    /// The person's start of agent 1 beside agent 0's live run, with two live runs allowed and
    /// every kill answered `no_service` at once.
    PersonBesideALiveRun,
}

/// What a start held at a gate did across a call of the hook.
struct HeldAcross {
    first: Option<RunId>,
    before: Vec<RunId>,
    at_return: Vec<RunId>,
    after: Vec<RunId>,
    took: Duration,
    named: Named,
    answer: Result<(), String>,
    stored_from: usize,
}

/// Holds the start of `mover` at `gate`, behind its first reading of the mark, and calls the
/// hook; the start goes on after the hook returned, or `go_after` after the hook's call. Reads
/// the runs started before the call, at its return and after the start's move returned.
fn start_held_across_the_hook(
    gate: StartGate,
    mover: Mover,
    go_after: Option<Duration>,
) -> (Arc<Hub<Shared>>, HeldAcross) {
    let max_live = if matches!(mover, Mover::PersonBesideALiveRun) {
        2
    } else {
        1
    };
    let hub = hub_of(2, max_live, 1);
    let stored_from = events(&hub).len();
    let c = hub.coordinator();
    let first = match mover {
        Mover::Person => None,
        Mover::QueueAtAnEnd | Mover::PersonBesideALiveRun => Some(live_run_of(&hub, 0)),
    };
    if matches!(mover, Mover::QueueAtAnEnd) {
        assert_eq!(c.control(&agent(1), Action::RunNow), Ok(()));
        assert_eq!(c.queued().len(), 1, "agent 1 waits in the queue");
    }
    if matches!(mover, Mover::PersonBesideALiveRun) {
        c.ports().inner().hold_kills(Hold::For(ms(0)));
    }
    let ports = c.ports().inner();
    ports.hold_the_next_start_at(gate);
    let moving = {
        let hub = hub.clone();
        let first = first.clone();
        std::thread::spawn(move || {
            let c = hub.coordinator();
            match (mover, first) {
                (Mover::QueueAtAnEnd, Some(run)) => {
                    c.ports().inner().with(|p| p.end_runner(&run));
                    c.on_run(&run, RunEvent::Ended(None))
                        .map_err(|e| format!("{e:?}"))
                }
                (Mover::PersonBesideALiveRun, _) => c
                    .control(&agent(1), Action::RunNow)
                    .map_err(|e| format!("{e:?}")),
                _ => c
                    .control(&agent(0), Action::RunNow)
                    .map_err(|e| format!("{e:?}")),
            }
        })
    };
    let waited = Instant::now();
    while !ports.start_at_its_gate() && waited.elapsed() < ms(5_000) {
        sleep(ms(1));
    }
    assert!(
        ports.start_at_its_gate(),
        "the start holds at {gate:?}, behind its first reading of the mark"
    );
    let before = started(&hub);
    let quitting = quit_named(&hub, None);
    if let Some(go_after) = go_after {
        sleep(go_after);
        ports.let_the_start_go();
    }
    let (took, named) = quitting.join().unwrap();
    let at_return = started(&hub);
    ports.let_the_start_go();
    let answer = moving.join().unwrap();
    let after = started(&hub);
    let seen = HeldAcross {
        first,
        before,
        at_return,
        after,
        took,
        named,
        answer,
        stored_from,
    };
    (hub, seen)
}

/// The runs that a `run_started` stored from the event `from` on names and that no
/// `Runs::start` started, each with the causes of its stored `run_ended` and whether the body
/// `end_run` was sent for it.
fn refused_late(hub: &Hub<Shared>, from: usize) -> Vec<(RunId, Vec<RunEnd>, bool)> {
    let launched = started(hub);
    let stored = events(hub).split_off(from);
    let calls = calls_since(hub, 0);
    stored
        .iter()
        .filter_map(|e| match &e.what {
            What::RunStarted { run, .. } if !launched.contains(run) => Some(run.clone()),
            _ => None,
        })
        .map(|run| {
            let ends = stored
                .iter()
                .filter_map(|e| match &e.what {
                    What::RunEnded { run: r, cause, .. } if r == &run => Some(*cause),
                    _ => None,
                })
                .collect();
            let end_run = calls
                .iter()
                .any(|c| matches!(c, Call::Send(Body::EndRun { run: r }) if r == &run));
            (run, ends, end_run)
        })
        .collect()
}

/// The late refusal of R8: one run that `run_started` names was never started, stored its end
/// `interrupted`, had `end_run` sent, and is named nowhere.
fn assert_one_late_refusal(hub: &Hub<Shared>, seen: &HeldAcross) {
    let late = refused_late(hub, seen.stored_from);
    assert_eq!(
        late.len(),
        1,
        "one start refused after its run_started: {late:?}"
    );
    let (run, ends, end_run) = &late[0];
    assert_eq!(
        ends,
        &[RunEnd::Interrupted],
        "its run ends interrupted, no failure"
    );
    assert!(end_run, "end_run is sent for it");
    assert!(
        !seen.named.iter().any(|(_, r)| r == run),
        "the hook does not name a run that no Runs::start started"
    );
    assert!(
        !hub.coordinator()
            .not_stopped()
            .iter()
            .any(|(_, r)| r == run),
        "nor does the list after the start's move"
    );
}

#[test]
fn u46_r8_a_persons_start_held_at_its_send_across_the_hooks_call_never_starts() {
    let (hub, seen) = start_held_across_the_hook(StartGate::SendOfStartRun, Mover::Person, None);
    assert_eq!(
        seen.after, seen.before,
        "Runs::start was called after the hook's call, for a start held behind its first \
         reading of the mark"
    );
    assert_eq!(seen.at_return, seen.before);
    assert!(seen.took < ms(1_900), "{:?}", seen.took);
    assert!(seen.named.is_empty(), "{:?}", seen.named);
    assert_eq!(seen.answer, Err("State".to_string()));
    assert_one_late_refusal(&hub, &seen);
    assert_eq!(hub.coordinator().live(), 0);
}

#[test]
fn u46_r8_the_queues_start_at_an_end_held_at_its_send_across_the_hooks_call_never_starts() {
    let (hub, seen) =
        start_held_across_the_hook(StartGate::SendOfStartRun, Mover::QueueAtAnEnd, None);
    let first = seen.first.clone().unwrap();
    assert_eq!(
        seen.after, seen.before,
        "Runs::start was called after the hook's call, for the queue's start held behind its \
         first reading of the mark"
    );
    assert_eq!(seen.before, std::slice::from_ref(&first));
    assert!(seen.took < ms(1_900), "{:?}", seen.took);
    assert_eq!(
        seen.named,
        [(agent(0), first)],
        "the list as the last publish before the end's move wrote it"
    );
    assert_eq!(seen.answer, Ok(()), "the end of the run is taken");
    assert_one_late_refusal(&hub, &seen);
    assert_eq!(hub.coordinator().live(), 0);
}

#[test]
fn u46_r8_a_start_whose_send_answers_300_ms_after_the_hooks_call_never_starts() {
    let (hub, seen) = start_held_across_the_hook(
        StartGate::SendOfStartRun,
        Mover::PersonBesideALiveRun,
        Some(ms(300)),
    );
    let first = seen.first.clone().unwrap();
    assert_eq!(
        seen.after, seen.before,
        "Runs::start was called after the hook's call, for a start whose send answered 300 ms \
         after it"
    );
    assert_eq!(seen.at_return, seen.before);
    assert!(seen.took < ms(2_900), "{:?}", seen.took);
    assert_eq!(
        seen.named,
        [(agent(0), first)],
        "the live run, whose kill answered no_service, alone"
    );
    assert_eq!(seen.answer, Err("State".to_string()));
    assert_one_late_refusal(&hub, &seen);
}

#[test]
fn u46_r8_a_persons_start_held_at_its_store_of_run_started_across_the_hooks_call_never_starts() {
    let (hub, seen) = start_held_across_the_hook(StartGate::StoreOfRunStarted, Mover::Person, None);
    assert_eq!(
        seen.after, seen.before,
        "Runs::start was called after the hook's call, for a start held at its store of \
         run_started"
    );
    assert_eq!(seen.at_return, seen.before);
    assert!(seen.took < ms(1_900), "{:?}", seen.took);
    assert!(seen.named.is_empty(), "{:?}", seen.named);
    assert_eq!(seen.answer, Err("State".to_string()));
    assert_one_late_refusal(&hub, &seen);
    assert_eq!(hub.coordinator().live(), 0);
}

#[test]
fn u46_r8_a_start_held_in_runs_start_after_its_last_reading_stands_in_the_hooks_return() {
    let (hub, seen) = start_held_across_the_hook(StartGate::RunsStart, Mover::Person, None);
    assert!(seen.before.is_empty() && seen.at_return.is_empty());
    assert_eq!(
        seen.after.len(),
        1,
        "the start passed its last reading before the hook's call"
    );
    let run = seen.after[0].clone();
    assert!(seen.took < ms(1_900), "{:?}", seen.took);
    assert_eq!(
        seen.named,
        [(agent(0), run.clone())],
        "the hook returned while the start's Runs::start held, and its list must name that run"
    );
    assert_eq!(seen.answer, Ok(()));
    assert!(refused_late(&hub, seen.stored_from).is_empty());
    assert!(hub.coordinator().not_stopped().contains(&(agent(0), run)));
}

#[test]
fn u46_r9_a_quit_whose_first_step_cannot_take_the_lock_kills_nothing_also_after_the_lock_goes() {
    let hub = hub_of(3, 3, 1);
    let _release = Release(hub.clone());
    let c = hub.coordinator();
    let stopped = live_run_of(&hub, 0);
    let dying = live_run_of(&hub, 1);
    let unstopped = live_run_of(&hub, 2);
    c.control(&agent(0), Action::Stop).unwrap();
    c.control(&agent(1), Action::Stop).unwrap();
    c.ports().inner().with(|p| p.end_runner(&dying));
    c.on_run(&dying, RunEvent::Ended(None)).unwrap();
    let ports = c.ports().inner();
    ports.with(|p| {
        p.set_now(T0 + 1_000);
        p.refuse(Method::Kill, PortError::NoService);
    });
    hub.tick();
    ports.with(|p| p.clear_refusals());
    ports.hold_kills(Hold::Released);
    let ticker = {
        let hub = hub.clone();
        std::thread::spawn(move || hub.tick())
    };
    wait_until_a_kill_holds(ports);
    let before = ports.recorded_kills().len();
    let cancels = ports.cancels_made();
    let called = Instant::now();
    let quitting = quit_named(&hub, None);
    let returned = finished_by(&quitting, called, ms(1_900));
    ports.release_kills();
    let (_, named) = quitting.join().unwrap();
    ticker.join().unwrap();
    assert!(
        returned,
        "the hook returned within cancel_grace_s and a margin"
    );
    let waited = Instant::now();
    while ports.cancels_made() == cancels && waited.elapsed() < ms(5_000) {
        sleep(ms(1));
    }
    assert!(
        ports.cancels_made() > cancels,
        "the first step took the lock once the tick's kill let it go, and cancelled"
    );
    sleep(ms(500));
    let unstopped_kills: Vec<RunId> = ports.recorded_kills()[before..]
        .iter()
        .map(|(run, _)| run.clone())
        .filter(|run| run == &unstopped)
        .collect();
    assert!(
        unstopped_kills.is_empty(),
        "a kill to all came after the lock went, from a second step that the hook started and \
         did not wait for: {unstopped_kills:?}"
    );
    assert_eq!(
        named,
        [
            (agent(0), stopped),
            (agent(1), dying),
            (agent(2), unstopped)
        ]
    );
}

#[test]
fn u46_r9_a_first_step_whose_own_read_of_the_clock_fails_leaves_the_whole_grace() {
    let hub = hub(1);
    let run = live_run(&hub);
    let ports = hub.coordinator().ports().inner();
    ports.fail_the_clock_now();
    let m = mark(&hub);
    let took = quit(&hub, None).join().unwrap();
    assert!(
        calls_since(&hub, m).contains(&Call::Cancel(run.clone())),
        "the first step sent cancel"
    );
    assert!(
        took >= ms(900) && took < ms(3_000),
        "the whole cancel_grace_s = 1 s from the first step's return: {took:?}"
    );
    assert_eq!(kills(&hub, m), [run]);
    assert!(
        halvo_coord::ports::Clock::now(hub.coordinator().ports()).is_err(),
        "the clock failed"
    );
}
