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
