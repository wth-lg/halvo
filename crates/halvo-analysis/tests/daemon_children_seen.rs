//! The test of the app's liveness signal in the capture service's run (halvo-capture, daemon::r2::service::seen). It
//! runs in a process of its own, because the signal stands for the rest of a process's life, and it turns a driver
//! over stand-ins of the readings, the source and the slot.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_capture::daemon::r2::service;
use halvo_capture::daemon::r2::when::{Driver, Reading, Readings, Slot, Source};
use halvo_capture::daemon::r2::worker_slot::{Answer, MayRun, Origin};
use halvo_control::proto::Request;
use halvo_schema_core::ids::{Position, StoreId};

/// The watermark that the stand-in source gives.
const WATERMARK: Position = Position { seg: 2, idx: 5 };

/// The readings of a computer on mains, not in low-power mode, idle for 10 min: a worker may run once the app is
/// seen.
struct Allowed;

impl Readings for Allowed {
    fn read(&mut self) -> Reading {
        Reading {
            idle: Some(Duration::from_secs(600)),
            mains: Some(true),
            low_power: Some(false),
        }
    }
}

/// A source that names one capture store from the zero position, with the watermark.
struct Given;

impl Source for Given {
    fn origin(&mut self) -> Option<(Origin, Option<Position>)> {
        let origin = Origin {
            capture: StoreId::parse("0123456789abcdef0123456789abcdef").unwrap(),
            committed: None,
        };
        Some((origin, Some(WATERMARK)))
    }
}

/// A slot that keeps the answer of each pass.
#[derive(Clone, Default)]
struct Passes(Arc<Mutex<Vec<MayRun>>>);

impl Slot for Passes {
    fn pass(&mut self, _now: Instant, may_run: MayRun, _origin: &Origin) -> Answer {
        self.0.lock().unwrap().push(may_run);
        match may_run {
            MayRun::Yes { .. } => Answer::Started,
            MayRun::No => Answer::Idle,
        }
    }
}

/// Rule 3: after a status that does not claim the indicator, and after the other requests of the service, the
/// driver answers no; after a status that claims it, yes.
#[test]
fn only_a_status_that_claims_sets_the_flag() {
    let passes = Passes::default();
    let mut driver = Driver::new(Allowed, Given, passes.clone());
    for request in [
        Request::Status { indicator: false },
        Request::Inventory,
        Request::Pause,
        Request::Resume,
    ] {
        service::seen(&request);
    }
    assert_eq!(driver.turn(Duration::from_secs(600)), Some(Answer::Idle));
    service::seen(&Request::Status { indicator: true });
    assert_eq!(driver.turn(Duration::from_secs(601)), Some(Answer::Started));
    assert_eq!(
        *passes.0.lock().unwrap(),
        [
            MayRun::No,
            MayRun::Yes {
                watermark: Some(WATERMARK)
            }
        ]
    );
}
