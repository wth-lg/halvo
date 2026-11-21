use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use halvo_control::paths::STATE_FILE;
use halvo_control::proto::{Platform, SourceName};
use halvo_control::{exit, INDICATOR_LEASE_MS};

use super::Lease;
use crate::daemon::board::Capture;
use crate::daemon::counters::Snapshot;
use crate::daemon::main_loop::{EndCall, MainLoop, TICK};
use crate::daemon::model_store::{foreground_now, open_floor, Harness};
use crate::daemon::owner::{Answer, Command, OwnerGone, Pending, RETRY_FIRST};
use crate::daemon::port::{StoreId, WriteFault};
use crate::daemon::slot::Slot;
use crate::daemon::sources::tests::Factory;
use crate::daemon::sources::{starting, RunKind};
use crate::daemon::state::tests::TestWrite;
use crate::daemon::state::{Desired, StateDir, StateFile};
use crate::daemon::supervisor::{
    Ending, OwnerLink, ReportLog, Supervisor, SystemSources, RESTART_MAX,
};
use crate::source::{CaptureError, SourceContext};

struct TestLoop {
    now: Rc<Cell<Duration>>,
    harness: Rc<RefCell<Harness>>,
}

impl MainLoop for TestLoop {
    fn turn(&mut self, wait: Duration) {
        {
            let mut harness = self.harness.borrow_mut();
            harness.clocks.advance(wait);
            harness.owner.pass(Duration::ZERO);
        }
        self.now.set(self.now.get() + wait);
    }

    fn now(&self) -> Duration {
        self.now.get()
    }
}

struct Link {
    harness: Rc<RefCell<Harness>>,
    log: ReportLog,
}

impl OwnerLink for Link {
    fn ask(&self, command: Command) -> Result<Pending, OwnerGone> {
        self.harness.borrow().handle.ask(command)
    }

    fn snapshot(&self) -> Snapshot {
        self.harness.borrow().owner.snapshot()
    }

    fn reports(&self) -> &ReportLog {
        &self.log
    }

    fn is_running(&self) -> bool {
        true
    }
}

struct Fixture<F: SystemSources = Factory> {
    supervisor: Supervisor<F, TestLoop>,
    now: Rc<Cell<Duration>>,
    link: Link,
    wanted: Vec<SourceName>,
    dir: tempfile::TempDir,
}

impl<F: SystemSources> Fixture<F> {
    fn new(desired: Desired, factory: F) -> Fixture<F> {
        Fixture::ending_by(desired, factory, None)
    }

    fn ending_by(desired: Desired, factory: F, ending: Option<Arc<dyn EndCall>>) -> Fixture<F> {
        let dir = tempfile::tempdir().unwrap();
        let mut state = StateFile::open(
            StateDir::open(dir.path()).unwrap(),
            Box::new(TestWrite::default()),
        );
        state.update(|state| state.desired = desired).unwrap();
        let switches = state.state().switches;
        let harness = Harness::new();
        let ctx = SourceContext::with_floor(harness.sink.clone(), open_floor());
        let harness = Rc::new(RefCell::new(harness));
        let (_unused, empty) = std::sync::mpsc::channel();
        let reports = std::mem::replace(&mut harness.borrow_mut().reports, empty);
        let now = Rc::new(Cell::new(Duration::ZERO));
        let main_loop = TestLoop {
            now: now.clone(),
            harness: harness.clone(),
        };
        let wanted = starting(Platform::Macos, RunKind::Service, switches);
        let mut supervisor = Supervisor::new(Platform::Macos, factory, main_loop)
            .with_err(Box::new(std::io::sink()))
            .with_state(state);
        if let Some(ending) = ending {
            supervisor = supervisor.with_ending(ending);
        }
        supervisor.serve_sources(ctx, wanted.clone());
        Fixture {
            supervisor,
            now,
            link: Link {
                harness,
                log: ReportLog::new(reports, Box::new(std::io::sink())),
            },
            wanted,
            dir,
        }
    }

    fn tick(&mut self) {
        self.supervisor.tick(Some(&self.link));
    }

    fn later(&mut self, by: Duration) {
        self.now.set(self.now.get() + by);
        self.tick();
    }

    fn set_slot(&mut self, slot: Slot) {
        self.supervisor.set_slot(slot, Some(&self.link));
    }

    fn claim(&mut self) {
        let now = self.now.get();
        self.supervisor
            .set_lease(Lease::renewed_at(now), Some(&self.link));
    }

    fn emit(&self, count: usize) {
        for _ in 0..count {
            self.link.harness.borrow().emit(foreground_now());
        }
    }

    fn stored(&self) -> usize {
        let mut harness = self.link.harness.borrow_mut();
        let settled = harness.ask(Command::Settle);
        assert!(matches!(settled, Answer::Settled(_)), "{settled:?}");
        harness.store.records().len()
    }

    fn state_bytes(&self) -> Vec<u8> {
        std::fs::read(self.dir.path().join(STATE_FILE)).unwrap()
    }
}

impl Fixture {
    fn begins(&self, name: SourceName) -> usize {
        self.supervisor
            .factory()
            .calls
            .of(&format!("begin {}", name.as_str()))
            .len()
    }

    fn stops(&self, name: SourceName) -> usize {
        self.supervisor
            .factory()
            .calls
            .of(&format!("stop {}", name.as_str()))
            .len()
    }
}

fn open() -> Slot {
    Slot::Open {
        id: StoreId::from_bytes([3; 16]),
    }
}

#[test]
fn no_source_starts_unless_the_run_condition_holds() {
    let cases = [
        ("all three", Desired::Running, open(), true, 1),
        ("desired paused", Desired::Paused, open(), true, 0),
        ("slot opening", Desired::Running, Slot::Opening, true, 0),
        (
            "slot maintenance",
            Desired::Running,
            Slot::Maintenance { id: None },
            true,
            0,
        ),
        ("slot deleting", Desired::Running, Slot::Deleting, true, 0),
        ("no claim", Desired::Running, open(), false, 0),
    ];
    for (case, desired, slot, claimed, starts) in cases {
        let mut f = Fixture::new(desired, Factory::new());
        assert!(
            !f.wanted.is_empty(),
            "{case}: the service run wants no source"
        );
        f.set_slot(slot);
        if claimed {
            f.claim();
        }
        for _ in 0..5 {
            f.later(TICK);
        }
        for name in f.wanted.clone() {
            assert_eq!(f.begins(name), starts, "{case}: begins of {name:?}");
        }
    }
    #[cfg(target_os = "macos")]
    {
        use crate::daemon::sources::ServiceSources;
        use crate::daemon::supervisor::SystemSources;
        use crate::macos::input::HoldPlan;
        let (writer, reader) = crate::macos::foreground::foreground_state_for_test();
        let turns = crate::macos::run_loop::RunLoopTurns::for_test(std::time::Instant::now());
        let factory = ServiceSources::new(writer, reader, turns);
        assert_eq!(factory.input_plan(), HoldPlan::Never);
        assert!(!factory.holds_secure_input());
    }
}

#[test]
fn a_lapsed_lease_stops_every_source_and_leaves_the_state_file_byte_identical() {
    let mut f = Fixture::new(
        Desired::Running,
        Factory::adjusting(|source| {
            source.on_stop = Some(Box::new(|ctx: &SourceContext| {
                let _ = ctx.sink().emit_event(foreground_now());
            }));
        }),
    );
    f.set_slot(open());
    let claimed = f.now.get();
    f.claim();
    let deadline = claimed + Duration::from_millis(INDICATOR_LEASE_MS);
    for name in f.wanted.clone() {
        assert_eq!(f.begins(name), 1, "begins of {name:?}");
    }
    f.emit(2);
    assert_eq!(f.stored(), 2);
    let before = f.state_bytes();
    let stopped_at = loop {
        let at = f.now.get() + TICK;
        f.later(TICK);
        if f.wanted.iter().all(|name| f.stops(*name) == 1) {
            break at;
        }
        assert!(
            at < deadline + 10 * TICK,
            "no source stopped by {at:?} after a claim at {claimed:?}"
        );
    };
    assert!(
        stopped_at <= deadline + TICK,
        "the sources stopped at {stopped_at:?}, the lease ended at {deadline:?}"
    );
    f.emit(3);
    assert_eq!(
        f.stored(),
        2,
        "a record received at or after the lapse barrier was stored"
    );
    assert!(
        f.state_bytes() == before,
        "the lapse changed the state file"
    );
    assert_eq!(f.supervisor.board().capture, Capture::NoIndicator);
    f.later(RESTART_MAX);
    for name in f.wanted.clone() {
        assert_eq!(f.begins(name), 1, "a restart of {name:?} while lapsed");
    }
    f.claim();
    for name in f.wanted.clone() {
        assert_eq!(f.begins(name), 2, "begins of {name:?} after the renewal");
    }
    f.emit(1);
    assert_eq!(f.stored(), 3, "the event after the renewal was not stored");
}

#[derive(Clone, Default)]
struct Ended(Arc<Mutex<Vec<i32>>>);

impl EndCall for Ended {
    fn end(&self, status: i32) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(status);
    }
}

#[test]
fn a_lapse_whose_stop_does_not_stop_ends_with_5_and_reads_pausing() {
    let ended = Ended::default();
    let mut f = Fixture::ending_by(
        Desired::Running,
        Factory::adjusting(|source| {
            if source.name == SourceName::Input {
                source.stop = Err(CaptureError::Timeout {
                    stage: "scripted_stop",
                    ms: 1_000,
                });
            }
        }),
        Some(Arc::new(ended.clone())),
    );
    assert!(f.wanted.contains(&SourceName::Input), "{:?}", f.wanted);
    f.set_slot(open());
    let claimed = f.now.get();
    f.claim();
    assert_eq!(f.begins(SourceName::Input), 1);
    let before = f.state_bytes();
    f.now
        .set(claimed + Duration::from_millis(INDICATOR_LEASE_MS));
    let mut replies = Vec::new();
    let answer = f
        .supervisor
        .follow_the_run_condition(Some(&f.link), &mut |board| replies.push(board.capture));
    assert_eq!(answer, Some(Ending::SourceNotStopped));
    assert_eq!(
        *ended.0.lock().unwrap_or_else(PoisonError::into_inner),
        [exit::SOURCE_NOT_STOPPED]
    );
    assert_eq!(replies, [Capture::Pausing]);
    assert_eq!(f.supervisor.board().capture, Capture::Pausing);
    for _ in 0..3 {
        f.later(TICK);
        assert_eq!(f.supervisor.board().capture, Capture::Pausing);
    }
    assert!(
        f.state_bytes() == before,
        "the lapse changed the state file"
    );
}

#[test]
fn a_renewal_while_the_drain_fails_begins_no_source_until_the_heal() {
    let mut f = Fixture::new(Desired::Running, Factory::new());
    f.set_slot(open());
    f.claim();
    for name in f.wanted.clone() {
        assert_eq!(f.begins(name), 1, "first begin of {name:?}");
    }
    let store = f.link.harness.borrow().store.clone();
    store.fail_every_write(Some(WriteFault::Io(std::io::ErrorKind::Other)));
    let mut turns = 0;
    while !f.supervisor.board().drain.stops_the_sources() {
        assert!(turns < 3_000, "the drain never failed");
        f.emit(1);
        f.supervisor.main_loop().turn(TICK);
        f.tick();
        turns += 1;
    }
    f.supervisor.set_lease(Lease::NONE, Some(&f.link));
    f.claim();
    for _ in 0..5 {
        f.supervisor.main_loop().turn(TICK);
        f.tick();
        for name in f.wanted.clone() {
            assert_eq!(f.begins(name), 1, "{name:?} began while the drain fails");
            assert_eq!(f.stops(name), 1, "{name:?} runs while the drain fails");
        }
        assert_eq!(f.supervisor.board().capture, Capture::Stopped);
    }
    store.fail_every_write(None);
    f.supervisor.main_loop().turn(RETRY_FIRST);
    let mut turns = 0;
    while f.supervisor.board().drain.stops_the_sources() {
        assert!(turns < 600, "the drain never healed");
        f.supervisor.main_loop().turn(TICK);
        f.tick();
        turns += 1;
    }
    for _ in 0..5 {
        f.supervisor.main_loop().turn(TICK);
        f.tick();
    }
    for name in f.wanted.clone() {
        assert_eq!(f.begins(name), 2, "begins of {name:?} after the heal");
    }
}

#[test]
fn a_start_whose_owner_answers_after_the_lease_ended_begins_no_source() {
    let mut f = Fixture::new(Desired::Running, Factory::new());
    let claimed = f.now.get();
    f.claim();
    let deadline = claimed + Duration::from_millis(INDICATOR_LEASE_MS);
    f.now.set(deadline - Duration::from_millis(1));
    f.set_slot(open());
    assert!(
        f.now.get() > deadline,
        "the owner answered at {:?}, before the lease ended at {deadline:?}",
        f.now.get()
    );
    for _ in 0..3 {
        f.later(TICK);
    }
    for name in f.wanted.clone() {
        assert_eq!(f.begins(name), 0, "{name:?} began after the lease ended");
    }
}

#[cfg(all(target_os = "macos", debug_assertions))]
#[test]
fn after_a_lapse_and_a_renewal_every_source_of_the_run_condition_runs_again_the_foreground_source_too(
) {
    use crate::daemon::sources::ServiceSources;
    use crate::source::{SourceState, StopCause};
    let (writer, reader) = crate::macos::foreground::foreground_state_for_test();
    let factory = ServiceSources::new(
        writer,
        reader,
        crate::macos::run_loop::RunLoopTurns::for_test(std::time::Instant::now()),
    )
    .with_stand_in_titles();
    let mut f = Fixture::new(Desired::Running, factory);
    let ctx = {
        let harness = f.link.harness.borrow();
        SourceContext::with_floor(harness.sink.clone(), open_floor())
    };
    f.supervisor
        .serve_sources(ctx, vec![SourceName::Foreground]);
    f.set_slot(open());
    let front = |f: &Fixture<ServiceSources>| {
        f.supervisor
            .board()
            .sources
            .iter()
            .find(|entry| entry.name == SourceName::Foreground)
            .map(|entry| entry.state.clone())
    };
    f.claim();
    assert_eq!(front(&f), Some(SourceState::Running), "the first start");
    f.later(Duration::from_millis(INDICATOR_LEASE_MS) + TICK);
    assert_eq!(f.supervisor.board().capture, Capture::NoIndicator);
    assert_eq!(
        front(&f),
        Some(SourceState::Stopped(StopCause::Request)),
        "the lapse"
    );
    f.claim();
    let after = front(&f);
    f.supervisor.stop_every_source();
    assert_eq!(after, Some(SourceState::Running), "after the renewal");
}
