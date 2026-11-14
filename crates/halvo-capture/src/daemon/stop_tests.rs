use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use halvo_control::exit;

use super::model_store::{
    foreground_now, key, noon, Harness, ManualClocks, ModelStore, WriteScript,
};
use super::owner::{Answer, Command, Report};
use super::port::{
    ByteBound, CaptureStore, MaintFault, MaintainStore, Pass, SegmentWrite, Usage as StoreUsage,
    WriteFault, Written,
};
use super::scripted::{ScriptedRun, ScriptedSource};
use super::{
    run_timed, DaemonError, FailedSource, LossCause, RunOutcome, RunSources, SecureInput,
    SourceName, SwitchFlags, TimedRun,
};
use crate::source::{SourceContext, SourceCounters};

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

pub(super) fn four_events() -> ScriptedRun {
    ScriptedRun::new(vec![
        (
            SourceName::Foreground,
            ScriptedSource::emitting(vec![foreground_now()]),
        ),
        (
            SourceName::Input,
            ScriptedSource::emitting(vec![key(), key(), key()]),
        ),
    ])
}

/// Every write answers Io until the scripted run's `figures()`, between the
/// last Settle and the Shutdown, sets the store's fault to `then`.
fn shutdown_case(then: Option<WriteFault>) -> (RunOutcome, ModelStore) {
    let mut scripted = four_events();
    let store = ModelStore::new();
    store.fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
    let hook_store = store.clone();
    scripted.on_figures = Some(Box::new(move || hook_store.fail_every_write(then)));
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(5),
        &mut scripted,
        store.clone(),
        ManualClocks::at(noon()),
        &stop,
        Duration::from_millis(300),
    )
    .unwrap();
    (outcome, store)
}

/// Ruling DM-12 A2 (K2), adversary 2's first sequence: the Shutdown's own
/// settle stores the held batch, so nothing was lost.
#[test]
fn a_fault_that_clears_before_the_shutdown_gives_status_0_and_four_persisted() {
    let (outcome, store) = shutdown_case(None);
    assert_eq!(
        store.records().len(),
        4,
        "the Shutdown's settle stored the batch"
    );
    assert_eq!(outcome.status, exit::CLEAN, "{outcome:?}");
    assert!(outcome.loss.is_none(), "{:?}", outcome.loss);
    assert_eq!(outcome.report.events_persisted, 4);
}

/// Ruling DM-12 A2 (K2), adversary 2's second sequence: a refusal at the last
/// settle prints error=-, not the kind of the fault before it.
#[test]
fn a_refusal_at_the_shutdown_settle_prints_no_kind() {
    let (outcome, _) = shutdown_case(Some(WriteFault::Refused));
    let line = outcome.loss.map(|loss| loss.line).unwrap_or_default();
    assert!(
        line.ends_with(" error=-"),
        "the final flush was refused: {line}"
    );
}

/// Ruling DM-12 A2 (K2), adversary 2's third sequence: the first try stores
/// and answers Io, the Shutdown's try answers AlreadyWritten.
#[test]
fn an_already_written_answer_at_the_shutdown_settle_gives_no_loss() {
    let mut scripted = four_events();
    let store = ModelStore::new();
    store.script_writes([WriteScript::KeepThenFail(WriteFault::Io(
        io::ErrorKind::TimedOut,
    ))]);
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(5),
        &mut scripted,
        store.clone(),
        ManualClocks::at(noon()),
        &stop,
        Duration::from_millis(90),
    )
    .unwrap();
    assert!(outcome.loss.is_none(), "{:?}", outcome.loss);
}

/// Ruling DM-12 A2 (K3), adversary 1's adv1_p10: the line of an unanswered
/// stop names no kind for a fault that had healed.
#[test]
fn the_line_of_an_unanswered_stop_names_no_healed_fault() {
    let mut harness = Harness::new();
    assert!(matches!(
        harness.ask(Command::ClosePause),
        Answer::Resumed(_)
    ));
    let _ = harness.reports();
    harness
        .store
        .script_writes([WriteScript::Fail(WriteFault::Io(io::ErrorKind::Other))]);
    harness.emit(key());
    harness.pass();
    let Answer::Settled(first) = harness.ask(Command::Settle) else {
        panic!("no settle");
    };
    assert_eq!(first.events_pending, 1);
    harness.clocks.advance(Duration::from_secs(2));
    harness.pass();
    let snapshot = harness.owner.snapshot();
    let reports = harness.reports();
    assert!(reports.contains(&Report::DrainHealthy), "{reports:?}");
    assert_eq!((snapshot.events_persisted, snapshot.events_pending), (1, 0));
    let line = super::loss_line(&snapshot, 0, false, &reports);
    assert!(line.ends_with(" error=-"), "the drain had healed: {line}");
}

/// Sources whose stop runs a hook first.
pub(super) struct StopHook<F: FnMut()> {
    pub(super) inner: ScriptedRun,
    pub(super) at_stop: F,
}

impl<F: FnMut()> RunSources for StopHook<F> {
    fn start(
        &mut self,
        ctx: &SourceContext,
        wanted: &[SourceName],
        hold_after: Option<Duration>,
    ) -> Vec<FailedSource> {
        self.inner.start(ctx, wanted, hold_after)
    }

    fn wait(&mut self, window: Duration, stop: &AtomicBool) -> bool {
        self.inner.wait(window, stop)
    }

    fn stop(&mut self) -> Vec<FailedSource> {
        (self.at_stop)();
        self.inner.stop()
    }

    fn figures(&self) -> SourceCounters {
        self.inner.figures()
    }

    fn hold_refusal(&self) -> Option<DaemonError> {
        self.inner.hold_refusal()
    }
}

/// Ruling DM-12 A2 (G5), adversary 1's adv1_w_h: a drop at the last settle
/// after a healed Io fault prints error=-.
#[test]
fn a_drop_at_the_last_settle_after_a_healed_fault_prints_no_kind() {
    let store = ModelStore::new();
    store.script_writes([
        WriteScript::Fail(WriteFault::Io(io::ErrorKind::Other)),
        WriteScript::Fail(WriteFault::Refused),
    ]);
    let mut sources = StopHook {
        inner: keys(1_024),
        at_stop: || {},
    };
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(5),
        &mut sources,
        store,
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    )
    .unwrap();
    let line = outcome.loss.map(|loss| loss.line).unwrap_or_default();
    assert!(line.ends_with(" error=-"), "{line}");
}
