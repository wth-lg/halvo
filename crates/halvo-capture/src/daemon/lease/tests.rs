use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use halvo_control::paths::STATE_FILE;
use halvo_control::proto::{Platform, SourceName};
use halvo_control::INDICATOR_LEASE_MS;

use super::Lease;
use crate::daemon::board::Capture;
use crate::daemon::counters::Snapshot;
use crate::daemon::main_loop::{MainLoop, TICK};
use crate::daemon::model_store::{foreground_now, open_floor, Harness};
use crate::daemon::owner::{Answer, Command, OwnerGone, Pending};
use crate::daemon::port::StoreId;
use crate::daemon::slot::Slot;
use crate::daemon::sources::tests::Factory;
use crate::daemon::sources::{starting, RunKind};
use crate::daemon::state::tests::TestWrite;
use crate::daemon::state::{Desired, StateDir, StateFile};
use crate::daemon::supervisor::{OwnerLink, ReportLog, Supervisor, RESTART_MAX};
use crate::source::SourceContext;

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

struct Fixture {
    supervisor: Supervisor<Factory, TestLoop>,
    now: Rc<Cell<Duration>>,
    link: Link,
    wanted: Vec<SourceName>,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new(desired: Desired, factory: Factory) -> Fixture {
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
