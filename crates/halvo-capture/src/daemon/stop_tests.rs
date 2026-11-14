use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use halvo_control::exit;

use super::model_store::{key, noon, ManualClocks, ModelStore};
use super::port::{
    ByteBound, CaptureStore, MaintFault, MaintainStore, Pass, SegmentWrite, Usage as StoreUsage,
    WriteFault, Written,
};
use super::scripted::{ScriptedRun, ScriptedSource};
use super::{run_timed, LossCause, RunOutcome, SecureInput, SourceName, SwitchFlags, TimedRun};

pub(super) fn run_of(seconds: u64) -> TimedRun {
    TimedRun {
        window: Duration::from_secs(seconds),
        secure_input: SecureInput::DoNotHold,
        switches: SwitchFlags::default(),
        store_dir: PathBuf::from("/"),
    }
}

pub(super) fn keys(count: usize) -> ScriptedRun {
    ScriptedRun::new(vec![(
        SourceName::Input,
        ScriptedSource::emitting((0..count).map(|_| key()).collect()),
    )])
}

pub(super) fn accepted(outcome: &RunOutcome) -> u64 {
    outcome
        .report
        .sink_counts
        .iter()
        .map(|kind| kind.accepted)
        .sum()
}

pub(super) fn lost_in_line(outcome: &RunOutcome) -> Option<u64> {
    let line = &outcome.loss.as_ref()?.line;
    let rest = line.split("lost events=").nth(1)?;
    rest.split(' ').next()?.parse().ok()
}

/// A model store whose writes wait while `parked` reads true (at most 30 s).
pub(super) struct ParkingStore {
    pub(super) inner: ModelStore,
    pub(super) parked: Arc<AtomicBool>,
    pub(super) writes: Arc<AtomicU64>,
}

impl ParkingStore {
    pub(super) fn parked(inner: ModelStore) -> (ParkingStore, Arc<AtomicBool>) {
        let parked = Arc::new(AtomicBool::new(true));
        let store = ParkingStore {
            inner,
            parked: Arc::clone(&parked),
            writes: Arc::new(AtomicU64::new(0)),
        };
        (store, parked)
    }
}

impl MaintainStore for ParkingStore {
    fn expire_due(&mut self, now: SystemTime, max: usize) -> Result<Pass, MaintFault> {
        self.inner.expire_due(now, max)
    }

    fn evict_to(&mut self, bound: ByteBound, max: usize) -> Result<Pass, MaintFault> {
        self.inner.evict_to(bound, max)
    }

    fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
        self.inner.usage()
    }
}

impl CaptureStore for ParkingStore {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        let until = Instant::now() + Duration::from_secs(30);
        while self.parked.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.inner.write_batch(write)
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        self.inner.volume_free_bytes()
    }
}

/// Ruling DM-12 A1 (K1): a stop that gets no answer counts at least what the
/// sink accepted and no write stored.
#[test]
fn an_unanswered_stop_counts_at_least_the_accepted_events_that_no_write_stored() {
    let model = ModelStore::new();
    let (store, parked) = ParkingStore::parked(model.clone());
    let mut scripted = keys(1_200);
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(5),
        &mut scripted,
        store,
        ManualClocks::at(noon()),
        &stop,
        Duration::from_millis(300),
    )
    .unwrap();
    let stored = model.records().len() as u64;
    parked.store(false, Ordering::SeqCst);
    assert_eq!(outcome.status, exit::ERROR, "{outcome:?}");
    assert!(matches!(
        outcome.loss.as_ref().map(|loss| loss.cause),
        Some(
            LossCause::PauseUnanswered
                | LossCause::SettleUnanswered
                | LossCause::ShutdownUnanswered
        )
    ));
    assert_eq!(accepted(&outcome), 1_200);
    let count = lost_in_line(&outcome).expect("no count in the loss line");
    assert!(
        count >= 1_200 - stored,
        "the loss line counts {count}, fewer than the {} accepted events that no write stored",
        1_200 - stored
    );
}
