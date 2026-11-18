//! The cases of the stress step on daemon-2 (the three adversaries of
//! interrogate-daemon2 and verifier-interrogate-24), brought in by ruling
//! DM-25 with the item that each holds. A child of `supervisor::tests`, so
//! that it reaches the fixtures there. Every ending here is a recorder: no
//! case ends a process, asks a grant or sends input.

use super::*;

/// An ending that records each status with the moment of its call.
#[derive(Clone, Default)]
struct Stamped {
    ended: Arc<Mutex<Vec<(i32, Instant)>>>,
}

impl EndCall for Stamped {
    fn end(&self, status: i32) {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((status, Instant::now()));
    }
}

impl Stamped {
    /// Each call as its status and the milliseconds since `from`.
    fn since(&self, from: Instant) -> Vec<(i32, u128)> {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(status, at)| (*status, at.saturating_duration_since(from).as_millis()))
            .collect()
    }

    fn statuses(&self) -> Vec<i32> {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(status, _)| *status)
            .collect()
    }
}

/// The time that a case on the real clock allows the watchdog's thread to
/// wake after its deadline (ruling DM-25 N1 (e)).
const WAKE: u128 = 300;

/// A loop that stands in while a fixture's supervisor is rebuilt.
fn placeholder() -> TestLoop {
    TestLoop {
        now: Rc::default(),
        harness: None,
        real: Duration::ZERO,
        hook: None,
    }
}

/// Gives the supervisor of `f` the ending `ending`, and with it a watchdog.
fn ending_with<F: SystemSources>(f: &mut Fixture<F>, empty: F, ending: Arc<dyn EndCall>) {
    f.supervisor = std::mem::replace(
        &mut f.supervisor,
        Supervisor::new(Platform::Macos, empty, placeholder()),
    )
    .with_ending(ending);
}

/// The watchdog of a supervisor, shared with a source's stop so that the
/// stop reads the arm while it runs.
type Peek = Rc<RefCell<Option<Watchdog>>>;

/// An input source whose stop records the watchdog's arm into `seen`, then
/// sleeps `real` on the real clock and answers `answer`.
fn input_stop(
    real: Duration,
    answer: Result<(), CaptureError>,
    peek: &Peek,
    seen: &Rc<RefCell<Vec<Option<i32>>>>,
) -> Factory {
    let (peek, seen) = (peek.clone(), seen.clone());
    Factory::adjusting(move |source| {
        if source.name == SourceName::Input {
            let (peek, seen) = (peek.clone(), seen.clone());
            let hook: Hook = Box::new(move |_| {
                seen.borrow_mut()
                    .push(peek.borrow().as_ref().and_then(Watchdog::armed));
                std::thread::sleep(real);
            });
            source.on_stop = Some(hook);
            source.stop = answer.clone();
        }
    })
}

/// What a stop for capture off left: its answer, its ending, the moment it
/// began, the arms that the input stop saw, the arm after it and its
/// records.
struct CaptureOffRun {
    outcome: Option<CaptureOff>,
    ending: Stamped,
    began: Instant,
    seen: Vec<Option<i32>>,
    armed: Option<i32>,
    records: Arc<Mutex<Vec<EndRecord>>>,
}

impl CaptureOffRun {
    fn endings(&self) -> Vec<(i32, u128)> {
        self.ending.since(self.began)
    }
}

/// A stop for capture off whose input stop sleeps `real` and answers
/// `answer`.
fn capture_off_with_input_stop(real: Duration, answer: Result<(), CaptureError>) -> CaptureOffRun {
    let (peek, seen): (Peek, Rc<RefCell<Vec<Option<i32>>>>) = Default::default();
    let mut f = fixture_on_owner(input_stop(real, answer, &peek, &seen), ModelStore::new());
    let ending = Stamped::default();
    ending_with(&mut f, Factory::new(), Arc::new(ending.clone()));
    let records: Arc<Mutex<Vec<EndRecord>>> = Arc::default();
    let calls = f.supervisor.factory().calls.clone();
    f.supervisor = std::mem::replace(
        &mut f.supervisor,
        Supervisor::new(Platform::Macos, Factory::new(), placeholder()),
    )
    .with_marker(Box::new(Records {
        calls,
        records: records.clone(),
    }));
    *peek.borrow_mut() = f.supervisor.watchdog.clone();
    f.start(&two());
    f.tick();
    let link = f.link.take().unwrap();
    let began = Instant::now();
    let outcome = f
        .supervisor
        .stop_for_capture_off(&link, StopKind::Pause, &mut |_| {});
    let seen = seen.borrow().clone();
    CaptureOffRun {
        outcome,
        ending,
        began,
        seen,
        armed: f.supervisor.watchdog_armed(),
        records,
    }
}

/// N1 (b), A1-F2b and v24: an input stop that answers Ok 1.5 s after its
/// call, past its bound of 1 s plus 250 ms, is not a stop.
#[test]
fn a_late_ok_in_a_stop_for_capture_off_reads_not_stopped() {
    let run = capture_off_with_input_stop(Duration::from_millis(1_500), Ok(()));
    let endings = run.endings();
    println!(
        "dm25 n1 late-ok outcome={:?} endings={endings:?} seen={:?} armed={:?}",
        run.outcome, run.seen, run.armed
    );
    assert_eq!(
        run.outcome,
        Some(CaptureOff::Ended(exit::SOURCE_NOT_STOPPED)),
        "a stop that answered Ok after 1,500 ms was read as a stop"
    );
    assert_eq!(
        run.seen,
        [Some(exit::SOURCE_NOT_STOPPED)],
        "the stop ran unarmed"
    );
    assert_eq!(run.armed, Some(exit::SOURCE_NOT_STOPPED));
    assert_eq!(
        run.ending.statuses(),
        [exit::SOURCE_NOT_STOPPED],
        "the end routine made no ending call with 5"
    );
}

/// N1 (a), A1-F2 and v24: an input stop that blocks 20 s on the real clock
/// is ended with 5 by the watchdog, armed before the stop, at most
/// 1,250 + 18,000 ms after the entry began (plus the watchdog's wake), while
/// the stop still blocks.
#[test]
fn a_blocked_stop_for_capture_off_is_ended_with_5_by_its_deadline() {
    let run = capture_off_with_input_stop(Duration::from_millis(20_000), Ok(()));
    let endings = run.endings();
    println!(
        "dm25 n1 blocked outcome={:?} endings={endings:?} seen={:?}",
        run.outcome, run.seen
    );
    assert_eq!(
        run.seen,
        [Some(exit::SOURCE_NOT_STOPPED)],
        "the stop ran unarmed"
    );
    let by = u128::from(ENTRY_DEADLINE_MS) + WAKE;
    assert!(
        matches!(endings.first(), Some((status, at)) if *status == exit::SOURCE_NOT_STOPPED && *at <= by),
        "a stop that blocked 20 s was not ended with 5 by {by} ms: {endings:?}"
    );
    assert_eq!(
        run.outcome,
        Some(CaptureOff::Ended(exit::SOURCE_NOT_STOPPED))
    );
}

/// N1 (d), in place of the case that read `watchdog_armed() == None` after
/// a stop for capture off whose sources all stop: the watchdog is armed with
/// 5 while the stops run, the arm is taken back after them, no record is
/// written, and no ending call comes by the deadline plus 500 ms.
#[test]
fn a_stop_for_capture_off_whose_sources_all_stop_ends_nothing() {
    let run = capture_off_with_input_stop(Duration::ZERO, Ok(()));
    assert_eq!(run.outcome, Some(CaptureOff::Stopped));
    assert_eq!(
        run.seen,
        [Some(exit::SOURCE_NOT_STOPPED)],
        "the stops ran unarmed"
    );
    assert_eq!(run.armed, None, "the arm stayed after every source stopped");
    assert!(run.records.lock().unwrap().is_empty());
    let after = Duration::from_millis(ENTRY_DEADLINE_MS + 500);
    std::thread::sleep(after.saturating_sub(run.began.elapsed()));
    assert!(
        run.ending.statuses().is_empty(),
        "an ending call came after every source stopped: {:?}",
        run.endings()
    );
}

/// A lost grant of the input source whose stop sleeps `real` and answers
/// Ok: the tick's answer, the calls of the ending since the tick began and
/// the arms that the input stop saw.
fn lost_grant_with_input_stop(
    real: Duration,
) -> (Option<Ending>, Vec<(i32, u128)>, Vec<Option<i32>>) {
    let (peek, seen): (Peek, Rc<RefCell<Vec<Option<i32>>>>) = Default::default();
    let (mut f, answers, _) = probed(fixture_on_owner(
        input_stop(real, Ok(()), &peek, &seen),
        ModelStore::new(),
    ));
    let ending = Stamped::default();
    ending_with(&mut f, Factory::new(), Arc::new(ending.clone()));
    *peek.borrow_mut() = f.supervisor.watchdog.clone();
    answers
        .borrow_mut()
        .push_back(input_monitoring(GrantState::Granted));
    f.start(&two());
    f.tick();
    answers
        .borrow_mut()
        .push_back(input_monitoring(GrantState::NotGranted));
    let began = Instant::now();
    let tick = f.later(PROBE_INTERVAL);
    let seen = seen.borrow().clone();
    (tick, ending.since(began), seen)
}

/// N1 (b) and v24 on the lost grant: an input stop that answers Ok 1.5 s
/// after its call is not a stop, and the run ends with 5.
#[test]
fn a_late_ok_on_the_lost_grant_reads_not_stopped() {
    let (tick, endings, seen) = lost_grant_with_input_stop(Duration::from_millis(1_500));
    println!("dm25 n1 lost-grant late-ok tick={tick:?} endings={endings:?} seen={seen:?}");
    assert_eq!(
        tick,
        Some(Ending::SourceNotStopped),
        "a lost grant whose stop answered after 1,500 ms kept the process up"
    );
    assert_eq!(
        seen,
        [Some(exit::SOURCE_NOT_STOPPED)],
        "the stop ran unarmed"
    );
    assert_eq!(
        endings
            .iter()
            .map(|(status, _)| *status)
            .collect::<Vec<_>>(),
        [exit::SOURCE_NOT_STOPPED]
    );
}

/// N1 (a) and the verifier's kept item 1 on the lost grant: an input stop
/// that blocks 20 s is ended with 5 by the watchdog at most 1,250 + 18,000 ms
/// after the call (plus the watchdog's wake), while it still blocks.
#[test]
fn a_blocked_stop_on_the_lost_grant_is_ended_with_5_by_its_deadline() {
    let (tick, endings, seen) = lost_grant_with_input_stop(Duration::from_millis(20_000));
    println!("dm25 n1 lost-grant blocked tick={tick:?} endings={endings:?} seen={seen:?}");
    assert_eq!(
        seen,
        [Some(exit::SOURCE_NOT_STOPPED)],
        "the stop ran unarmed"
    );
    let by = u128::from(ENTRY_DEADLINE_MS) + WAKE;
    assert!(
        matches!(endings.first(), Some((status, at)) if *status == exit::SOURCE_NOT_STOPPED && *at <= by),
        "a stop that blocked 20 s was not ended with 5 by {by} ms: {endings:?}"
    );
    assert_eq!(tick, Some(Ending::SourceNotStopped));
}

/// N1 (c), A1-F9, A2-6 and v24: the shutdown entry arms the watchdog at its
/// start with its own status; an input stop that answers Timeout turns the
/// arm into 5, and the entry ends by its routine with 5.
#[test]
fn the_entry_that_meets_a_timeout_arms_with_its_status_and_ends_with_5() {
    let (peek, seen): (Peek, Rc<RefCell<Vec<Option<i32>>>>) = Default::default();
    let mut f = fixture_on_owner(
        input_stop(Duration::ZERO, Err(timeout()), &peek, &seen),
        ModelStore::new(),
    );
    let ending = Stamped::default();
    ending_with(&mut f, Factory::new(), Arc::new(ending.clone()));
    *peek.borrow_mut() = f.supervisor.watchdog.clone();
    f.start(&two());
    f.tick();
    let link = f.link.take().unwrap();
    let outcome = f.supervisor.shutdown_entry(&link, exit::CLEAN).unwrap();
    let seen = seen.borrow().clone();
    println!(
        "dm25 n1 entry status={} seen={seen:?} armed={:?} endings={:?}",
        outcome.status,
        f.supervisor.watchdog_armed(),
        ending.statuses()
    );
    assert_eq!(
        seen.first(),
        Some(&Some(exit::CLEAN)),
        "the entry did not arm the watchdog with its own status before its first stop"
    );
    assert_eq!(
        f.supervisor.watchdog_armed(),
        Some(exit::SOURCE_NOT_STOPPED)
    );
    assert_eq!(ending.statuses(), [exit::SOURCE_NOT_STOPPED]);
    assert_eq!(outcome.status, exit::SOURCE_NOT_STOPPED);
}

/// N1 (c) and A2's finder entry 1 (row 16): an input stop that takes 19 s of
/// the injected clock and answers Timeout does not carry the entry past its
/// deadline: it ends by its routine with 5 within ENTRY_DEADLINE_MS of its
/// arm. A2's own assertion, that the entry took less than
/// SHUTDOWN_BUDGET_MS, cannot hold for a stop that itself takes 19 s; the
/// item asks for the end within the deadline, which a6d6752b misses by
/// answering its own status with no ending call.
#[test]
fn an_overrunning_stop_does_not_carry_the_entry_past_its_deadline() {
    let clock: Rc<RefCell<Option<Rc<Cell<Duration>>>>> = Rc::default();
    let blocked = clock.clone();
    let mut f = fixture_on_owner(
        Factory::adjusting(move |source| {
            if source.name == SourceName::Input {
                let blocked = blocked.clone();
                let hook: Hook = Box::new(move |_| {
                    if let Some(now) = blocked.borrow().as_ref() {
                        now.set(now.get() + Duration::from_secs(19));
                    }
                });
                source.on_stop = Some(hook);
                source.stop = Err(timeout());
            }
        }),
        ModelStore::new(),
    );
    let ending = Stamped::default();
    ending_with(&mut f, Factory::new(), Arc::new(ending.clone()));
    *clock.borrow_mut() = Some(f.now.clone());
    f.start(&two());
    f.tick();
    let link = f.link.take().unwrap();
    let outcome = f.supervisor.shutdown_entry(&link, exit::CLEAN).unwrap();
    println!(
        "dm25 n1 entry-overrun took_ms={} status={} endings={:?}",
        outcome.took.as_millis(),
        outcome.status,
        ending.statuses()
    );
    assert!(
        outcome.took <= Duration::from_millis(ENTRY_DEADLINE_MS),
        "the entry took {:?}, past its deadline of {ENTRY_DEADLINE_MS} ms",
        outcome.took
    );
    assert_eq!(ending.statuses(), [exit::SOURCE_NOT_STOPPED]);
    assert_eq!(outcome.status, exit::SOURCE_NOT_STOPPED);
}

/// A main loop on the real clock: `now` is the time since it was made and a
/// turn sleeps its whole wait, so that the stop path's clock and the
/// watchdog's clock are one.
struct RealClock {
    began: Instant,
    hook: Box<dyn FnMut(Duration)>,
}

impl MainLoop for RealClock {
    fn turn(&mut self, wait: Duration) {
        std::thread::sleep(wait);
        let at = self.began.elapsed();
        (self.hook)(at);
    }

    fn now(&self) -> Duration {
        self.began.elapsed()
    }
}

/// What a timed run at its limits left: its answer, the moment it returned,
/// its ending and the store's gate.
struct TimedAtLimits {
    outcome: crate::daemon::RunOutcome,
    returned: Instant,
    ending: Stamped,
    supervisor: Supervisor<Factory, RealClock>,
    gate: Arc<std::sync::atomic::AtomicU8>,
}

impl TimedAtLimits {
    /// The calls of the ending as their status and the microseconds after
    /// the return, negative before it.
    fn after_return(&self) -> Vec<(i32, i128)> {
        self.ending
            .ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(status, at)| {
                let after = if *at >= self.returned {
                    i128::try_from(at.duration_since(self.returned).as_micros()).unwrap_or(0)
                } else {
                    -i128::try_from(self.returned.duration_since(*at).as_micros()).unwrap_or(0)
                };
                (*status, after)
            })
            .collect()
    }
}

/// The scenario of the three adversaries' timed-run cases (A1-F1c, A2-3,
/// A3-4) on one real clock: the store is closed for most of the drain wait,
/// its writes fail through the settle and wait at the Shutdown, and each
/// source answers Ok at its stop deadline (input 1 s, foreground 1 s,
/// screen 6 s), so that no source is let go and the stop path reaches its
/// cut. The run's own answer is status 1 with its loss line.
fn timed_run_at_its_limits() -> TimedAtLimits {
    let ending = Stamped::default();
    let (outcome, returned, supervisor, gate) =
        timed_run_at_its_limits_ending(Arc::new(ending.clone()));
    TimedAtLimits {
        outcome,
        returned,
        ending,
        supervisor,
        gate,
    }
}

/// The run of [`timed_run_at_its_limits`] with the ending `ending`: its
/// answer, the moment it returned, its supervisor and the store's gate.
fn timed_run_at_its_limits_ending(
    ending: Arc<dyn EndCall>,
) -> (
    crate::daemon::RunOutcome,
    Instant,
    Supervisor<Factory, RealClock>,
    Arc<std::sync::atomic::AtomicU8>,
) {
    let mode = Arc::new(std::sync::atomic::AtomicU8::new(CLOSED));
    let gate = mode.clone();
    let main_loop = RealClock {
        began: Instant::now(),
        hook: Box::new(move |at| {
            if at >= Duration::from_millis(2_900) && at < Duration::from_secs(9) {
                gate.store(OPEN, Ordering::SeqCst);
            }
            if at >= Duration::from_millis(15_500) {
                gate.store(CLOSED, Ordering::SeqCst);
            }
        }),
    };
    let deadlines = [
        (SourceName::Input, Duration::from_secs(1)),
        (SourceName::Foreground, Duration::from_secs(1)),
        (SourceName::Screen, Duration::from_secs(6)),
    ];
    let stop_mode = mode.clone();
    let factory = Factory::adjusting(move |source| {
        if source.name == SourceName::Foreground {
            source.script = (0..600).map(|_| foreground_now()).collect();
        }
        let deadline = deadlines
            .iter()
            .find(|(name, _)| *name == source.name)
            .map_or(Duration::ZERO, |(_, deadline)| *deadline);
        let mode = stop_mode.clone();
        let hook: Hook = Box::new(move |_| {
            mode.store(FAILING, Ordering::SeqCst);
            std::thread::sleep(deadline);
        });
        source.on_stop = Some(hook);
        source.stop = Ok(());
    });
    let mut supervisor = Supervisor::new(Platform::Macos, factory, main_loop).with_ending(ending);
    let mut run = run_of(1);
    run.switches = crate::daemon::SwitchFlags(vec![(crate::floor::Switch::ScreenActivity, true)]);
    let store = GateStore {
        inner: ModelStore::new(),
        mode: mode.clone(),
    };
    let outcome = run_timed(
        &run,
        &mut supervisor,
        store,
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    )
    .unwrap();
    (outcome, Instant::now(), supervisor, mode)
}

/// Asserts that no ending call came by 500 ms after the run returned, the
/// item's words for each of the three cases (ruling DM-25 N2 (b)).
fn no_call_by_500_ms_after_the_return(run: &TimedAtLimits, case: &str) {
    let wait = Duration::from_millis(500).saturating_sub(run.returned.elapsed());
    std::thread::sleep(wait);
    run.gate.store(OPEN, Ordering::SeqCst);
    let calls = run.after_return();
    println!(
        "dm25 n2 {case} status={} loss={:?} let_go={:?} calls(status,us_after_return)={calls:?}",
        run.outcome.status,
        run.outcome.loss.as_ref().map(|loss| loss.cause),
        run.outcome.let_go
    );
    assert!(run.outcome.let_go.is_empty(), "a source was let go");
    assert!(
        calls.is_empty(),
        "{case}: an ending call came by 500 ms after run_timed returned with status {}: {calls:?}",
        run.outcome.status
    );
}

/// N2 and the verifier's kept item 3: A1-F1c.
#[test]
fn adv1_f1c_on_one_clock_with_every_source_stopped_no_ending_call_follows_the_return() {
    let run = timed_run_at_its_limits();
    no_call_by_500_ms_after_the_return(&run, "a1-f1c");
}

/// N2: A2-3, whose assertion was that no call came before the loss line;
/// the item asks for no call by 500 ms after the return, the supervisor
/// dropped and the answer written as the binary writes it.
#[test]
fn adv2_no_ending_call_follows_a_timed_run_whose_stop_path_reaches_its_cut() {
    let mut run = timed_run_at_its_limits();
    let supervisor = std::mem::replace(
        &mut run.supervisor,
        Supervisor::new(
            Platform::Macos,
            Factory::new(),
            RealClock {
                began: Instant::now(),
                hook: Box::new(|_| {}),
            },
        ),
    );
    drop(supervisor);
    let mut report = Vec::new();
    let mut err = Out::default();
    let answered = crate::daemon::outcome_answer(&run.outcome, &mut report, &mut err);
    assert_eq!(answered, run.outcome.status);
    no_call_by_500_ms_after_the_return(&run, "a2-3");
}

/// N2: A3-4, whose assertion was that no call came before the return; the
/// item asks for no call by 500 ms after it.
#[test]
fn adv3_no_ending_call_follows_a_timed_run_whose_cut_stop_path_reaches_the_budget() {
    let run = timed_run_at_its_limits();
    no_call_by_500_ms_after_the_return(&run, "a3-4");
}

/// N2 (c) and A2's finder timed 2: a timed run whose window ends with a
/// grant restart arms the watchdog with its own status, 4, which each stop
/// of its stop path reads.
#[test]
fn a_timed_run_ended_by_a_grant_restart_arms_the_watchdog_with_4() {
    let (peek, seen): (Peek, Rc<RefCell<Vec<Option<i32>>>>) = Default::default();
    let (stop_peek, stop_seen) = (peek.clone(), seen.clone());
    let factory = Factory::adjusting(move |source| {
        let (peek, seen) = (stop_peek.clone(), stop_seen.clone());
        let hook: Hook = Box::new(move |_| {
            seen.borrow_mut()
                .push(peek.borrow().as_ref().and_then(Watchdog::armed));
        });
        source.on_stop = Some(hook);
    });
    let answers: Answers = Rc::default();
    answers
        .borrow_mut()
        .push_back(input_monitoring(GrantState::NotDetermined));
    answers
        .borrow_mut()
        .push_back(input_monitoring(GrantState::Granted));
    let main_loop = TestLoop {
        now: Rc::default(),
        harness: None,
        real: Duration::from_millis(1),
        hook: None,
    };
    let ending = Stamped::default();
    let mut supervisor = Supervisor::new(Platform::Macos, factory, main_loop)
        .with_ending(Arc::new(ending.clone()))
        .with_probe(Box::new(FakeProbe {
            answers: answers.clone(),
            asked: Rc::default(),
        }));
    *peek.borrow_mut() = supervisor.watchdog.clone();
    supervisor.set_client_polls(true);
    let outcome = run_timed(
        &run_of(10),
        &mut supervisor,
        ModelStore::new(),
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    )
    .unwrap();
    let seen = seen.borrow().clone();
    println!(
        "dm25 n2 grant-restart status={} seen={seen:?} armed_after={:?}",
        outcome.status,
        supervisor.watchdog_armed()
    );
    assert!(
        RunSources::restart_asked(&supervisor),
        "the probe did not end the window with a restart"
    );
    assert_eq!(outcome.status, exit::RESTART, "{:?}", outcome.loss);
    assert!(!seen.is_empty(), "no source was stopped");
    assert!(
        seen.iter().all(|armed| *armed == Some(exit::RESTART)),
        "the stop path ran under an arm other than the run's own status 4: {seen:?}"
    );
    assert_eq!(
        supervisor.watchdog_armed(),
        None,
        "the arm stayed after the return"
    );
    assert!(ending.statuses().is_empty());
}

/// What each stop of a source read of the watchdog: its deadline and status
/// with the moment of the read.
type Reads = Rc<RefCell<Vec<(Option<(Instant, i32)>, Instant)>>>;

/// A factory whose every source's stop reads the watchdog into `reads` and
/// answers `input` for the input source.
fn reading_stops(peek: &Peek, reads: &Reads, input: Result<(), CaptureError>) -> Factory {
    let (peek, reads) = (peek.clone(), reads.clone());
    Factory::adjusting(move |source| {
        let (peek, reads) = (peek.clone(), reads.clone());
        let hook: Hook = Box::new(move |_| {
            let armed = peek.borrow().as_ref().and_then(Watchdog::armed_until);
            reads.borrow_mut().push((armed, Instant::now()));
        });
        source.on_stop = Some(hook);
        if source.name == SourceName::Input {
            source.stop = input.clone();
        }
    })
}

/// The milliseconds from `at` to the deadline of `armed`.
fn ahead(armed: Option<(Instant, i32)>, at: Instant) -> Option<(u128, i32)> {
    armed.map(|(deadline, status)| (deadline.saturating_duration_since(at).as_millis(), status))
}

/// N9 (a) at the capture-off site (m2): the arm before the first stop reads
/// ENTRY_DEADLINE_MS ahead with 5, and the arm moved at a Timeout reads
/// SHUTDOWN_BUDGET_MS after that answer. A deadline ten times longer at
/// either arm fails it.
#[test]
fn the_capture_off_arms_read_their_deadlines() {
    let (peek, reads): (Peek, Reads) = Default::default();
    let mut f = fixture_on_owner(
        reading_stops(&peek, &reads, Err(timeout())),
        ModelStore::new(),
    );
    let ending = Stamped::default();
    ending_with(&mut f, Factory::new(), Arc::new(ending.clone()));
    *peek.borrow_mut() = f.supervisor.watchdog.clone();
    f.start(&two());
    f.tick();
    let link = f.link.take().unwrap();
    let began = Instant::now();
    let outcome = f
        .supervisor
        .stop_for_capture_off(&link, StopKind::Pause, &mut |_| {});
    let returned = Instant::now();
    let first = reads
        .borrow()
        .first()
        .copied()
        .expect("no source was stopped");
    let at_stop = ahead(first.0, first.1);
    let moved = ahead(
        f.supervisor
            .watchdog
            .as_ref()
            .and_then(Watchdog::armed_until),
        first.1,
    );
    println!(
        "dm25 n9 capture-off at_stop(ms_ahead,status)={at_stop:?} moved(ms_after_first_answer,status)={moved:?} took_ms={}",
        returned.duration_since(began).as_millis()
    );
    assert_eq!(outcome, Some(CaptureOff::Ended(exit::SOURCE_NOT_STOPPED)));
    let entry = u128::from(ENTRY_DEADLINE_MS);
    assert!(
        matches!(at_stop, Some((ms, 5)) if ms <= entry && ms + 1_000 >= entry),
        "the arm before the first stop read {at_stop:?}, not {entry} ms ahead with 5"
    );
    let budget = u128::from(SHUTDOWN_BUDGET_MS);
    assert!(
        matches!(moved, Some((ms, 5)) if ms <= budget + 50 && ms + 1_000 >= budget),
        "the arm after the Timeout read {moved:?}, not {budget} ms after it with 5"
    );
}

/// N9 (a) at the timed-run site (m3): each stop of the stop path reads the
/// arm of the end of the window, SHUTDOWN_BUDGET_MS from that end with 3. A
/// deadline ten times longer fails it.
#[test]
fn the_timed_run_arm_reads_its_deadline() {
    let (peek, reads): (Peek, Reads) = Default::default();
    let main_loop = TestLoop {
        now: Rc::default(),
        harness: None,
        real: Duration::from_millis(1),
        hook: None,
    };
    let ending = Stamped::default();
    let mut supervisor = Supervisor::new(
        Platform::Macos,
        reading_stops(&peek, &reads, Ok(())),
        main_loop,
    )
    .with_ending(Arc::new(ending.clone()));
    *peek.borrow_mut() = supervisor.watchdog.clone();
    let outcome = run_timed(
        &run_of(1),
        &mut supervisor,
        ModelStore::new(),
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    )
    .unwrap();
    let read: Vec<Option<(u128, i32)>> = reads
        .borrow()
        .iter()
        .map(|(armed, at)| ahead(*armed, *at))
        .collect();
    println!(
        "dm25 n9 timed-run status={} at_stops(ms_ahead,status)={read:?}",
        outcome.status
    );
    assert!(!read.is_empty(), "no source was stopped");
    let budget = u128::from(SHUTDOWN_BUDGET_MS);
    assert!(
        read.iter()
            .all(|armed| matches!(armed, Some((ms, 3)) if *ms <= budget && ms + 1_000 >= budget)),
        "the stops read {read:?}, not an arm {budget} ms from the end of the window with 3"
    );
}

/// A store whose first write goes to the model store and whose later writes
/// panic, so that the owner ends after it persisted some events.
struct PersistsThenPanics {
    inner: ModelStore,
    writes: usize,
}

impl MaintainStore for PersistsThenPanics {
    fn expire_due(&mut self, now: std::time::SystemTime, max: usize) -> Result<Pass, MaintFault> {
        self.inner.expire_due(now, max)
    }

    fn evict_to(&mut self, bound: ByteBound, max: usize) -> Result<Pass, MaintFault> {
        self.inner.evict_to(bound, max)
    }

    fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
        self.inner.usage()
    }
}

impl CaptureStore for PersistsThenPanics {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        self.writes += 1;
        if self.writes > 1 {
            panic!("the scripted store panics at its second write");
        }
        self.inner.write_batch(write)
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        self.inner.volume_free_bytes()
    }
}

/// N9 (b) (m1): an owner that persisted some events and then ended gives
/// the owner-ended line with the events accepted less those persisted. The
/// run emits 1,100 events, so that the owner writes its first full batch and
/// ends at its second inside the window.
#[test]
fn an_owner_that_ended_after_some_writes_counts_only_the_events_not_persisted() {
    let mut run = OneShot {
        script: (0..1_100).map(|_| foreground_now()).collect(),
        linger: Duration::from_millis(300),
        at_stop: None,
    };
    let model = ModelStore::new();
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = run_timed(
        &run_of(1),
        &mut run,
        PersistsThenPanics {
            inner: model.clone(),
            writes: 0,
        },
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    );
    std::panic::set_hook(quiet);
    let outcome = outcome.unwrap();
    let persisted = model.records().len();
    let loss = outcome.loss.clone().expect("no loss line");
    println!(
        "dm25 n9 owner-ended persisted={persisted} line={:?}",
        loss.line
    );
    assert!(
        persisted > 0,
        "the store persisted nothing before the owner ended"
    );
    assert_eq!(loss.cause, Some(LossCause::OwnerEnded));
    assert_eq!(
        loss.line,
        format!(
            "halvo-capture: owner ended; events accepted and not persisted={}",
            1_100 - persisted
        )
    );
}

/// N2 (b): a timed run that returns with a source let go keeps its arm, and
/// the arm carries the run's own status (here 1, for the drain's loss),
/// where at a6d6752b it kept the 3 of the end of the window.
#[test]
fn a_timed_run_with_a_source_let_go_keeps_an_arm_with_its_own_status() {
    let main_loop = TestLoop {
        now: Rc::default(),
        harness: None,
        real: Duration::from_millis(1),
        hook: None,
    };
    let ending = Stamped::default();
    let mut supervisor = Supervisor::new(
        Platform::Macos,
        Factory::adjusting(|source| {
            if source.name == SourceName::Foreground {
                source.script = (0..10).map(|_| foreground_now()).collect();
            }
            if source.name == SourceName::Input {
                source.stop = Err(timeout());
            }
        }),
        main_loop,
    )
    .with_ending(Arc::new(ending.clone()));
    let store = ModelStore::new();
    store.fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
    let outcome = run_timed(
        &run_of(1),
        &mut supervisor,
        store,
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    )
    .unwrap();
    println!(
        "dm25 n2 let-go status={} let_go={:?} armed={:?}",
        outcome.status,
        outcome.let_go,
        supervisor.watchdog_armed()
    );
    assert_eq!(outcome.let_go, ["input"]);
    assert_eq!(outcome.status, exit::ERROR, "{:?}", outcome.loss);
    assert_eq!(supervisor.watchdog_armed(), Some(outcome.status));
    assert!(ending.statuses().is_empty());
}

/// A writer that puts each line into the call log, so that its place among
/// the stops, the reply and the records shows.
struct Lines(crate::daemon::sources::tests::Calls);

impl Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(bytes).trim_end().to_owned();
        if text.starts_with("halvo-capture: secure input stays on") {
            self.0.push("err <MA-9 line>".to_owned());
        } else if !text.is_empty() {
            self.0.push("err <other line>".to_owned());
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The sources of a run whose input source has a hold plan.
struct Holds(Factory);

impl SourceFactory for Holds {
    fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
        self.0.make(name)
    }
}

impl SystemSources for Holds {
    fn holds_secure_input(&self) -> bool {
        true
    }
}

/// A fixture whose input source holds secure input and answers Timeout at
/// its first stop and `second` at the next ask, with a recorder, a marker
/// and an error stream that write into the call log.
fn holding_fixture(
    second: Result<(), CaptureError>,
) -> (
    Fixture<Holds>,
    crate::daemon::sources::tests::Calls,
    Answers,
) {
    let mut f = fixture_on_owner(
        Holds(Factory::adjusting(|source| {
            if source.name == SourceName::Input {
                source.stop = Err(timeout());
            }
        })),
        ModelStore::new(),
    );
    let calls = f.supervisor.factory().0.calls.clone();
    let answers: Answers = Rc::default();
    f.supervisor = std::mem::replace(
        &mut f.supervisor,
        Supervisor::new(Platform::Macos, Holds(Factory::new()), placeholder()),
    )
    .with_probe(Box::new(FakeProbe {
        answers: answers.clone(),
        asked: Rc::default(),
    }))
    .with_ending(Arc::new(Recorder {
        ended: Arc::default(),
        calls: Some(calls.clone()),
    }))
    .with_marker(Box::new(Records {
        calls: calls.clone(),
        records: Arc::default(),
    }))
    .with_err(Box::new(Lines(calls.clone())));
    answers
        .borrow_mut()
        .push_back(input_monitoring(GrantState::Granted));
    f.start(&two());
    f.tick();
    f.supervisor
        .factory_mut()
        .0
        .on_second_stop(SourceName::Input, second);
    (f, calls, answers)
}

/// The call log of a run, without the starts.
fn after_the_starts(calls: &crate::daemon::sources::tests::Calls) -> Vec<String> {
    calls
        .taken()
        .into_iter()
        .filter(|call| !call.starts_with("begin "))
        .collect()
}

/// A stop for capture off of a fixture of [`holding_fixture`].
fn capture_off_with_hold(second: Result<(), CaptureError>) -> Vec<String> {
    let (mut f, calls, _) = holding_fixture(second);
    let link = f.link.take().unwrap();
    let replies = calls.clone();
    let _ = f
        .supervisor
        .stop_for_capture_off(&link, StopKind::Pause, &mut |_| {
            replies.push("reply".to_owned());
        });
    after_the_starts(&calls)
}

/// A lost grant of a fixture of [`holding_fixture`].
fn lost_grant_with_hold(second: Result<(), CaptureError>) -> Vec<String> {
    let (mut f, calls, answers) = holding_fixture(second);
    answers
        .borrow_mut()
        .push_back(input_monitoring(GrantState::NotGranted));
    assert_eq!(f.later(PROBE_INTERVAL), Some(Ending::SourceNotStopped));
    after_the_starts(&calls)
}

/// Asserts that MA-9's line comes right after the first input stop, once.
fn line_right_after_the_first_stop(case: &str, calls: &[String]) {
    println!("dm25 n3 {case} calls={calls:?}");
    let first_stop = calls.iter().position(|call| call == "stop input");
    let line = calls.iter().position(|call| call == "err <MA-9 line>");
    assert_eq!(
        line,
        first_stop.map(|at| at + 1),
        "{case}: MA-9's line is not right after the first input stop: {calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| *call == "err <MA-9 line>")
            .count(),
        1,
        "{case}: MA-9's line is written more than once: {calls:?}"
    );
}

/// N3 and v24 (A2-1, A1-F3, A3-3) on a stop for capture off: the line
/// follows the first input stop, before the reply and the first record,
/// whether the second ask answers Timeout or Ok.
#[test]
fn the_ma9_line_of_a_stop_for_capture_off_comes_at_the_first_answer() {
    line_right_after_the_first_stop(
        "capture-off second=Timeout",
        &capture_off_with_hold(Err(timeout())),
    );
    line_right_after_the_first_stop("capture-off second=Ok", &capture_off_with_hold(Ok(())));
}

/// N3 and A1-F3 on the lost grant: the same, on the stop of a lost grant.
#[test]
fn the_ma9_line_of_a_lost_grant_comes_at_the_first_answer() {
    line_right_after_the_first_stop(
        "lost-grant second=Timeout",
        &lost_grant_with_hold(Err(timeout())),
    );
    line_right_after_the_first_stop("lost-grant second=Ok", &lost_grant_with_hold(Ok(())));
}

/// The sources of a run whose hold of secure input the system refused.
struct Refusing(Factory);

impl SourceFactory for Refusing {
    fn make(&mut self, name: SourceName) -> Option<Box<dyn CaptureSource>> {
        self.0.make(name)
    }
}

impl SystemSources for Refusing {
    fn hold_refusal(&self) -> Option<DaemonError> {
        Some(DaemonError::SecureInputRefused(-25_293))
    }
}

/// N4 and A1-F8 (the verifier's kept item 5): a timed run whose input stop
/// answers Timeout and whose hold the system refused reaches the binary's
/// answer with the refusal and the source let go: the refusal's line as the
/// error arm writes it, status 1, the line that input did not stop, and the
/// ending call with 1, here a recorder.
#[test]
fn adv1_f8_a_refused_hold_keeps_the_source_that_was_let_go() {
    let ending = Stamped::default();
    let mut supervisor = Supervisor::new(
        Platform::Macos,
        Refusing(Factory::adjusting(|source| {
            if source.name == SourceName::Input {
                source.stop = Err(timeout());
            }
        })),
        real_loop(),
    )
    .with_ending(Arc::new(ending.clone()));
    let result = run_timed(
        &run_of(1),
        &mut supervisor,
        ModelStore::new(),
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    );
    println!(
        "dm25 n4 a1-f8 result={:?}",
        result
            .as_ref()
            .map(|outcome| (
                outcome.status,
                outcome.let_go.clone(),
                outcome.refusal.clone()
            ))
            .map_err(ToString::to_string)
    );
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => panic!(
            "input was let go and run_timed returned Err({error}): the binary's error arm prints it and returns from main, with no finish_run"
        ),
    };
    assert_eq!(outcome.let_go, ["input"]);
    assert_eq!(supervisor.watchdog_armed(), Some(exit::ERROR));
    let answered = Stamped::default();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = crate::daemon::answer_with(&outcome, &mut out, &mut err, &answered);
    let err = String::from_utf8(err).unwrap();
    println!(
        "dm25 n4 a1-f8 status={status} err={err:?} ended={:?}",
        answered.statuses()
    );
    assert_eq!(status, exit::ERROR);
    assert!(out.is_empty(), "a refused run wrote a report");
    assert_eq!(
        err,
        "halvo-capture: the system refused the secure-input hold (status -25293)\n\
         halvo-capture: the source input did not stop; the process ends without waiting for it\n"
    );
    assert_eq!(answered.statuses(), [exit::ERROR]);
}

/// The sources of a one-shot run whose hold of secure input the system
/// refused, with `let_go` let go at the stop.
struct RefusedShot {
    script: Vec<crate::event::FloorEvent>,
    linger: Duration,
    let_go: Vec<SourceName>,
}

impl RunSources for RefusedShot {
    fn start(
        &mut self,
        ctx: &SourceContext,
        _wanted: &[SourceName],
        _hold_after: Option<Duration>,
    ) -> Vec<FailedSource> {
        for event in self.script.drain(..) {
            let _ = ctx.sink().emit_event(event);
        }
        Vec::new()
    }

    fn wait(&mut self, _window: Duration, _stop: &AtomicBool) -> bool {
        std::thread::sleep(self.linger);
        false
    }

    fn stop(&mut self) -> Vec<FailedSource> {
        self.let_go
            .iter()
            .map(|name| FailedSource {
                name: name.as_str(),
                error: "timeout".to_owned(),
            })
            .collect()
    }

    fn figures(&self) -> SourceCounters {
        SourceCounters::default()
    }

    fn hold_refusal(&self) -> Option<DaemonError> {
        Some(DaemonError::SecureInputRefused(-25_293))
    }

    fn let_go(&self) -> Vec<SourceName> {
        self.let_go.clone()
    }
}

/// N4 and A3-2: a refused run whose stop let a source go keeps it in its
/// answer.
#[test]
fn adv3_a_refused_run_with_a_source_let_go_keeps_it_in_its_answer() {
    let result = run_timed(
        &run_of(1),
        &mut RefusedShot {
            script: Vec::new(),
            linger: Duration::ZERO,
            let_go: vec![SourceName::Screen],
        },
        ModelStore::new(),
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    );
    let shown = match &result {
        Ok(outcome) => format!("Ok status={} let_go={:?}", outcome.status, outcome.let_go),
        Err(error) => format!("Err {error}"),
    };
    println!("dm25 n4 a3-refused-let-go answer={shown:?}");
    assert!(
        matches!(&result, Ok(outcome) if outcome.let_go == ["screen"] && outcome.refusal.is_some()),
        "the refused run dropped the source it let go: {shown}"
    );
}

/// N4 and A3-2 (P10): an owner that ended in a run whose hold was refused
/// gives the owner-ended line after the refusal, not the line of an
/// unanswered pause.
#[test]
fn adv3_p10_an_owner_that_ended_in_a_run_whose_hold_was_refused_gives_the_owner_ended_line() {
    let mut run = RefusedShot {
        script: (0..600).map(|_| foreground_now()).collect(),
        linger: Duration::from_millis(300),
        let_go: Vec::new(),
    };
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let answer = run_timed(
        &run_of(1),
        &mut run,
        PanickingStore(ModelStore::new()),
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    );
    std::panic::set_hook(quiet);
    let text = match &answer {
        Ok(outcome) => format!("Ok status={} refusal={:?}", outcome.status, outcome.refusal),
        Err(error) => format!("Err {error}"),
    };
    println!("dm25 n4 a3-p10 answer={text:?}");
    assert!(
        matches!(&answer, Ok(outcome) if outcome.refusal.as_deref() == Some(
            "the system refused the secure-input hold (status -25293)\n\
             halvo-capture: owner ended; events accepted and not persisted=600"
        ) && outcome.status == exit::ERROR),
        "{text}"
    );
}

/// N5 and A3-1 (the verifier's kept item 6, drain stop): a source that
/// died just before a drain stop is named in the run's list.
#[test]
fn adv3_a_source_that_died_just_before_a_drain_stop_is_named() {
    let named = |drain_fails_too: bool| {
        let store = ModelStore::new();
        let mut made = 0;
        let mut f = fixture_on_owner(
            Factory::adjusting(move |source| {
                if source.name == SourceName::Foreground {
                    made += 1;
                    if made == 1 {
                        source.stop = Err(CaptureError::ThreadEnded);
                    }
                }
            }),
            store.clone(),
        );
        f.start(&two());
        f.tick();
        f.supervisor
            .factory()
            .health_of(SourceName::Foreground)
            .set(SourceState::Failed {
                reason: HealthReason::ThreadEnded,
                error: CaptureError::ThreadEnded,
            });
        if drain_fails_too {
            drain_fails(&f, &store, WriteFault::Io(io::ErrorKind::Other));
            f.tick();
            store.fail_every_write(None);
            f.supervisor.main_loop().turn(RETRY_FIRST);
            f.link.as_ref().unwrap().settle();
        }
        for _ in 0..30 {
            f.later(Duration::from_millis(100));
        }
        let board = f.supervisor.board().failed_sources();
        let stopped = RunSources::stop(&mut f.supervisor);
        println!(
            "dm25 n5 adv3-p4 drain_fails_too={drain_fails_too} board_failed={:?} run_stop_failed={:?} begins_foreground={}",
            board.iter().map(|e| e.name).collect::<Vec<_>>(),
            stopped.iter().map(|e| e.name).collect::<Vec<_>>(),
            f.supervisor.factory().calls.of("begin foreground").len()
        );
        stopped.iter().any(|entry| entry.name == "foreground")
    };
    assert!(
        named(false),
        "control: the death is named without a drain stop"
    );
    assert!(
        named(true),
        "the foreground source died in the window and the run names it nowhere"
    );
}

/// N5 and A3-1 (kept item 6, signal), P4, signal trigger: the foreground source died in the last slice of the
/// window and the signal came in the same slice: wait_on returns at its
/// flag check with no tick, and the stop's ThreadEnded is a stop.
#[test]
fn adv3_p4_a_source_that_died_in_the_slice_of_a_signal_is_named() {
    let mut f = fixture(Factory::adjusting(|source| {
        if source.name == SourceName::Foreground {
            source.stop = Err(CaptureError::ThreadEnded);
        }
    }));
    assert!(f.start(&two()).is_empty());
    assert!(f.tick().is_none());
    f.supervisor
        .factory()
        .health_of(SourceName::Foreground)
        .set(SourceState::Failed {
            reason: HealthReason::ThreadEnded,
            error: CaptureError::ThreadEnded,
        });
    assert!(f
        .supervisor
        .wait_on(Duration::from_secs(1), &AtomicBool::new(true), None));
    let failed = RunSources::stop(&mut f.supervisor);
    println!(
        "dm25 n5 adv3-p4-signal failed={:?}",
        failed.iter().map(|e| e.name).collect::<Vec<_>>()
    );
    assert!(
        failed.iter().any(|entry| entry.name == "foreground"),
        "a source that died in the slice of the signal is named nowhere: {failed:?}"
    );
}

/// N5 and A3-1 (kept item 6, owner's end), P4, owner-gone trigger: the owner ended in the slice in which the
/// foreground source died: tick stops every source and returns OwnerGone
/// before the health read.
#[test]
fn adv3_p4_a_source_that_died_in_the_slice_of_the_owner_end_is_named() {
    let mut f = fixture_on_owner(
        Factory::adjusting(|source| {
            if source.name == SourceName::Foreground {
                source.stop = Err(CaptureError::ThreadEnded);
            }
        }),
        ModelStore::new(),
    );
    assert!(f.start(&two()).is_empty());
    assert!(f.tick().is_none());
    f.supervisor
        .factory()
        .health_of(SourceName::Foreground)
        .set(SourceState::Failed {
            reason: HealthReason::ThreadEnded,
            error: CaptureError::ThreadEnded,
        });
    f.link.as_ref().expect("an owner link").running.set(false);
    assert!(matches!(f.tick(), Some(Ending::OwnerGone)));
    let failed = RunSources::stop(&mut f.supervisor);
    println!(
        "dm25 n5 adv3-p4-owner failed={:?}",
        failed.iter().map(|e| e.name).collect::<Vec<_>>()
    );
    assert!(
        failed.iter().any(|entry| entry.name == "foreground"),
        "a source that died in the slice of the owner's end is named nowhere: {failed:?}"
    );
}

/// N6 and A1-F4 (the verifier's kept item 7): a stop for capture off whose
/// input stop answers Timeout and then Ok at the second ask writes a final
/// record that still names input, with the Timeout's stage and code.
#[test]
fn adv1_f4_the_final_record_of_a_timeout_then_ok_names_the_source() {
    let (_f, recorder, records, outcome) = capture_off_case(
        Factory::adjusting(|source| {
            if source.name == SourceName::Input {
                source.stop = Err(timeout());
            }
        }),
        Some(Ok(())),
    );
    let records = records.lock().unwrap().clone();
    let texts: Vec<String> = records.iter().map(EndRecord::text).collect();
    println!(
        "dm25 n6 outcome={outcome:?} statuses={:?} records={texts:?} errors={:?}",
        recorder.statuses(),
        records
            .iter()
            .map(|record| record.errors.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(outcome, Some(CaptureOff::Ended(exit::SOURCE_NOT_STOPPED)));
    assert_eq!(records.len(), 2);
    for record in &records {
        assert_eq!(
            record.not_stopped,
            [(SourceName::Input, "timeout".to_owned())],
            "a record of a life that ended with 5 does not name input: {}",
            record.text()
        );
        assert_eq!(
            record.errors,
            [(
                SourceName::Input,
                CaptureError::Timeout {
                    stage: "input_tap_join",
                    ms: 1_000
                }
            )]
        );
    }
}

/// A stop for capture off whose owner runs its passes only from
/// `answers_at` after the Pause, `None` for never: the calls after the
/// starts, and the loop's time at the end.
fn capture_off_with_a_late_barrier(answers_at: Option<Duration>) -> (Vec<String>, Duration) {
    let mut f = fixture_on_owner(Factory::new(), ModelStore::new());
    let calls = f.supervisor.factory().calls.clone();
    f.start(&two());
    f.tick();
    let harness = f.link.as_ref().unwrap().harness.clone();
    let passes = calls.clone();
    let from = f.now.get();
    let main_loop = f.supervisor.main_loop();
    main_loop.harness = None;
    main_loop.hook = Some(Box::new(move |at| {
        if answers_at.is_some_and(|answers_at| at >= from + answers_at) {
            harness.borrow_mut().owner.pass(Duration::ZERO);
            passes.push(format!("owner pass at {} ms", (at - from).as_millis()));
        }
    }));
    let link = f.link.take().unwrap();
    let outcome = f
        .supervisor
        .stop_for_capture_off(&link, StopKind::Pause, &mut |_| {});
    assert_eq!(outcome, Some(CaptureOff::Stopped));
    (after_the_starts(&calls), f.now.get() - from)
}

/// N7 (a) and A1-F6 (the verifier's kept item 8): the owner answers the
/// barrier 1.5 s after the Pause, past FIRST_SETTLE; no source is asked to
/// stop before that answer.
#[test]
fn adv1_f6_the_capture_off_stop_asks_no_source_before_the_barrier_is_answered() {
    let (taken, at) = capture_off_with_a_late_barrier(Some(Duration::from_millis(1_500)));
    println!("dm25 n7 a1-f6 at_ms={} calls={taken:?}", at.as_millis());
    let first_stop = taken.iter().position(|call| call.starts_with("stop "));
    let first_pass = taken.iter().position(|call| call.starts_with("owner pass"));
    assert!(
        matches!((first_pass, first_stop), (Some(pass), Some(stop)) if pass < stop),
        "the first source was asked to stop before the owner answered the barrier: {taken:?}"
    );
}

/// N7 (a): an owner that never answers the barrier still lets the stops
/// begin, once BARRIER_WAIT has passed.
#[test]
fn a_barrier_that_is_never_answered_lets_the_stops_begin_after_its_wait() {
    let (taken, at) = capture_off_with_a_late_barrier(None);
    println!("dm25 n7 never at_ms={} calls={taken:?}", at.as_millis());
    assert_eq!(taken, ["stop input", "stop foreground"]);
    assert!(
        at >= BARRIER_WAIT && at < BARRIER_WAIT + Duration::from_millis(500),
        "the stops began {at:?} after the Pause"
    );
}

/// N7 (b) and A2-5 (kept item 8, the lost grant): the watchdog's call comes
/// at most SHUTDOWN_BUDGET_MS after the first answer that is not a stop,
/// while the routine's second ask blocks 19.5 s on the real clock. Each turn
/// of the loop sleeps its whole wait, so that the barrier's wait is real.
#[test]
fn adv2_the_lost_grant_arms_the_watchdog_at_its_first_answer() {
    let start = Instant::now();
    let first_answer: Rc<Cell<Option<Instant>>> = Rc::default();
    let asked: Rc<Cell<u32>> = Rc::default();
    let (first, count) = (first_answer.clone(), asked.clone());
    let (mut f, answers, _) = probed(fixture_on_owner(
        Factory::adjusting(move |source| {
            if source.name == SourceName::Input {
                source.stop = Err(timeout());
                let (first, count) = (first.clone(), count.clone());
                let hook: Hook = Box::new(move |_| {
                    count.set(count.get() + 1);
                    if count.get() == 1 {
                        first.set(Some(Instant::now()));
                    } else {
                        std::thread::sleep(Duration::from_millis(19_500));
                    }
                });
                source.on_stop = Some(hook);
            }
        }),
        ModelStore::new(),
    ));
    let ending = Stamped::default();
    ending_with(&mut f, Factory::new(), Arc::new(ending.clone()));
    answers
        .borrow_mut()
        .push_back(input_monitoring(GrantState::Granted));
    f.start(&two());
    f.tick();
    f.link.as_ref().unwrap().emit(2);
    answers
        .borrow_mut()
        .push_back(input_monitoring(GrantState::NotGranted));
    f.supervisor.main_loop().harness = None;
    f.supervisor.main_loop().real = Duration::from_secs(3_600);
    f.now.set(f.now.get() + PROBE_INTERVAL);
    let link = f.link.take().unwrap();
    let tick = f.supervisor.tick(Some(&link));
    let from = first_answer
        .get()
        .expect("the input source was not stopped");
    let calls = ending.since(from);
    println!(
        "dm25 n7 a2-lost-grant tick={tick:?} took_ms={} calls(status,ms_after_first_answer)={calls:?}",
        start.elapsed().as_millis()
    );
    let by = u128::from(SHUTDOWN_BUDGET_MS) + WAKE;
    assert!(
        matches!(calls.first(), Some((status, at)) if *status == exit::SOURCE_NOT_STOPPED && *at <= by),
        "the first ending call came {calls:?} after the first answer that is not a stop, past {by} ms"
    );
}

/// A store whose every write panics, so that the owner thread ends.
struct PanicStore(ModelStore);

impl MaintainStore for PanicStore {
    fn expire_due(&mut self, now: std::time::SystemTime, max: usize) -> Result<Pass, MaintFault> {
        self.0.expire_due(now, max)
    }

    fn evict_to(&mut self, bound: ByteBound, max: usize) -> Result<Pass, MaintFault> {
        self.0.evict_to(bound, max)
    }

    fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
        self.0.usage()
    }
}

impl CaptureStore for PanicStore {
    fn write_batch(&mut self, _write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        panic!("the store ends the owner thread");
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        self.0.volume_free_bytes()
    }
}

/// N8 and A2-4 (the verifier's kept item 9): the owner ends inside the
/// window and the input source, whose stop answers Timeout, is let go; it is
/// asked to stop once.
#[test]
fn adv2f_a_source_let_go_when_the_owner_ends_is_not_shut_down_a_second_time() {
    let now: Rc<Cell<Duration>> = Rc::default();
    let first_stop: Rc<Cell<Option<Duration>>> = Rc::default();
    let main_loop = TestLoop {
        now: now.clone(),
        harness: None,
        real: Duration::from_millis(20),
        hook: None,
    };
    let (clock, first) = (now.clone(), first_stop.clone());
    let factory = Factory::adjusting(move |source| {
        if source.name == SourceName::Foreground {
            source.script = (0..600).map(|_| foreground_now()).collect();
        }
        if source.name == SourceName::Input {
            source.stop = Err(timeout());
            let (clock, first) = (clock.clone(), first.clone());
            let hook: Hook = Box::new(move |_| {
                if first.get().is_none() {
                    first.set(Some(clock.get()));
                }
                clock.set(clock.get() + Duration::from_secs(1));
            });
            source.on_stop = Some(hook);
        }
    });
    let mut supervisor = Supervisor::new(Platform::Macos, factory, main_loop);
    let window = Duration::from_secs(30);
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = run_timed(
        &run_of(30),
        &mut supervisor,
        PanicStore(ModelStore::new()),
        crate::daemon::maintenance::SystemClocks::new(),
        &AtomicBool::new(false),
        SETTLE_BOUND,
    );
    std::panic::set_hook(quiet);
    let outcome = outcome.unwrap();
    let stops = supervisor.factory().calls.of("stop ");
    println!("dm25 n8 let_go={:?} stops={stops:?}", outcome.let_go);
    let stopped_at = first_stop
        .get()
        .expect("the input source was never stopped");
    assert!(
        stopped_at < window,
        "the owner did not end inside the window (first stop at {stopped_at:?}); the case was not reached"
    );
    assert_eq!(outcome.let_go, ["input"]);
    assert_eq!(
        supervisor.factory().calls.of("stop input").len(),
        1,
        "a source let go when the owner ended was shut down again: {stops:?}"
    );
}

/// Runs this test binary's test `name` alone as a child with `env` set, and
/// gives its exit status, its standard output and the time it took.
fn child_of(name: &str, env: &str) -> (Option<i32>, String, Duration) {
    let started = Instant::now();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            name,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(env, "1")
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        started.elapsed(),
    )
}

/// N1's real end, run as a child of this test binary: a stop for capture off
/// whose input stop blocks 30 s, on a supervisor with the product's ending,
/// is ended with 5 by the watchdog at its deadline. It runs on the hosted
/// jobs alone (the round's step names it with --ignored); no process is
/// ended on this Mac.
#[test]
#[ignore = "ruling DM-25: the real end runs on the hosted jobs alone"]
fn a_blocked_stop_for_capture_off_ends_its_process_with_5() {
    const NAME: &str = "daemon::supervisor::tests::stress_cases::a_blocked_stop_for_capture_off_ends_its_process_with_5";
    const CHILD: &str = "HALVO_DAEMON_N1_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let (peek, seen): (Peek, Rc<RefCell<Vec<Option<i32>>>>) = Default::default();
        let mut f = fixture_on_owner(
            input_stop(Duration::from_secs(30), Ok(()), &peek, &seen),
            ModelStore::new(),
        );
        ending_with(
            &mut f,
            Factory::new(),
            Arc::new(crate::daemon::main_loop::ExitNow),
        );
        f.start(&two());
        f.tick();
        let link = f.link.take().unwrap();
        let _ = f
            .supervisor
            .stop_for_capture_off(&link, StopKind::Pause, &mut |_| {});
        return;
    }
    let (status, _, took) = child_of(NAME, CHILD);
    println!(
        "daemon-2 n1 real end exit_status={status:?} took_ms={}",
        took.as_millis()
    );
    assert_eq!(status, Some(exit::SOURCE_NOT_STOPPED));
    assert!(
        took >= Duration::from_millis(ENTRY_DEADLINE_MS)
            && took < Duration::from_millis(ENTRY_DEADLINE_MS + 3_000),
        "{took:?}"
    );
}

/// N2's real end, run as a child of this test binary: the timed run of
/// [`timed_run_at_its_limits`] on a supervisor with the product's ending
/// returns with no source let go, and the child exits with the run's own
/// status 500 ms after the return; no watchdog call ends it first with 3. It
/// runs on the hosted jobs alone; no process is ended on this Mac.
#[test]
#[ignore = "ruling DM-25: the real end runs on the hosted jobs alone"]
fn a_timed_run_ends_its_process_with_its_own_status() {
    const NAME: &str =
        "daemon::supervisor::tests::stress_cases::a_timed_run_ends_its_process_with_its_own_status";
    const CHILD: &str = "HALVO_DAEMON_N2_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let (outcome, _, supervisor, _) =
            timed_run_at_its_limits_ending(Arc::new(crate::daemon::main_loop::ExitNow));
        println!(
            "child status={} let_go={}",
            outcome.status,
            outcome.let_go.len()
        );
        drop(supervisor);
        std::thread::sleep(Duration::from_millis(500));
        std::process::exit(outcome.status);
    }
    let (status, out, took) = child_of(NAME, CHILD);
    let answered = out
        .lines()
        .find_map(|line| line.strip_prefix("child status="))
        .and_then(|rest| rest.split(' ').next())
        .and_then(|status| status.parse::<i32>().ok());
    println!(
        "daemon-2 n2 real end exit_status={status:?} run_status={answered:?} took_ms={}",
        took.as_millis()
    );
    assert!(answered.is_some(), "the child printed no status");
    assert_eq!(
        status, answered,
        "the child did not end with the run's own status"
    );
    assert_ne!(status, Some(exit::TIMED_RUN_SOURCE_FAILED));
}
