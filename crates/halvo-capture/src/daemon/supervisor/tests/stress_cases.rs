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
    let ending = Stamped::default();
    let mut supervisor =
        Supervisor::new(Platform::Macos, factory, main_loop).with_ending(Arc::new(ending.clone()));
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
    TimedAtLimits {
        outcome,
        returned: Instant::now(),
        ending,
        supervisor,
        gate: mode,
    }
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
