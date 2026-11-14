use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
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
    run_timed, stop_bound, DaemonError, FailedSource, LossCause, RunOutcome, RunSources,
    SecureInput, SourceName, StopBounds, SwitchFlags, TimedRun, DRAIN_BOUND, SETTLE_BOUND,
};
use crate::source::{CaptureError, HeldSamples, SourceContext, SourceCounters, SuppressedKeys};

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
        Duration::from_secs(2),
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

/// Ruling DM-13 (1): a fault of another kind at the Shutdown's own settle is
/// the kind of the loss line (the verifier's witness): every write answers Io
/// until the Shutdown, whose settle meets the database's code 11.
#[test]
fn a_fault_of_another_kind_at_the_shutdown_settle_is_the_kind_of_the_line() {
    let (outcome, _) = shutdown_case(Some(WriteFault::Db(11)));
    let line = outcome.loss.map(|loss| loss.line).unwrap_or_default();
    assert!(line.contains("lost events=4 "), "{line}");
    assert!(
        line.ends_with(" error=db"),
        "the last fault was the database's: {line}"
    );
}

/// Ruling DM-12 A2 (K2), adversary 2's third sequence: the first try stores
/// and answers Io, and a later try of the last settle answers AlreadyWritten,
/// which the owner acknowledges. Each wait takes 2 s: with the sequence's 90 ms
/// a hosted macOS runner answered the pause later in 117 of 200 runs.
#[test]
fn an_already_written_answer_at_the_last_settle_gives_no_loss() {
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
        Duration::from_secs(2),
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

/// Ruling DM-12 A3 (reading A of contracts.md C4 item 6): a signal that comes
/// after the window's end and before the run returns gives status 2.
#[test]
fn a_signal_after_the_window_and_before_the_return_gives_status_2() {
    let stop = AtomicBool::new(false);
    let mut sources = StopHook {
        inner: four_events(),
        at_stop: || stop.store(true, Ordering::SeqCst),
    };
    let outcome = run_timed(
        &run_of(5),
        &mut sources,
        ModelStore::new(),
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    )
    .unwrap();
    assert!(outcome.loss.is_none(), "{:?}", outcome.loss);
    assert_eq!(outcome.status, exit::TIMED_RUN_INTERRUPTED, "{outcome:?}");
}

/// Ruling DM-12 A3 (G4), adversary 1's adv1_w_f: a signal wins over a source
/// that did not stop, whose line the report keeps.
#[test]
fn a_signal_wins_over_a_source_that_did_not_stop() {
    let mut scripted = keys(4);
    scripted.signal = true;
    scripted.sources[0].1.stop = Err(CaptureError::Timeout {
        stage: "input_join",
        ms: 500,
    });
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(600),
        &mut scripted,
        ModelStore::new(),
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(outcome.status, exit::TIMED_RUN_INTERRUPTED, "{outcome:?}");
    assert_eq!(outcome.report.failed_sources.len(), 1, "{outcome:?}");
}

/// A model store whose first write sleeps `first` and then answers.
struct FirstWriteSleeps {
    inner: ModelStore,
    first: Duration,
    writes: u64,
}

impl MaintainStore for FirstWriteSleeps {
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

impl CaptureStore for FirstWriteSleeps {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        self.writes += 1;
        if self.writes == 1 {
            std::thread::sleep(self.first);
        }
        self.inner.write_batch(write)
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        self.inner.volume_free_bytes()
    }
}

/// Ruling DM-12 A3, K4 as a pin: no wait of the stop path is cut. A signalled
/// run behind a write of 3 s waits for that write, stores what was captured
/// and returns status 2 inside the sum of the stop path's named bounds.
#[test]
fn a_signalled_run_behind_a_slow_write_waits_for_it_inside_the_stop_bound() {
    assert_eq!(
        stop_bound(SETTLE_BOUND),
        Duration::from_secs(2) + 3 * Duration::from_secs(5)
    );
    let model = ModelStore::new();
    let mut scripted = keys(600);
    scripted.signal = true;
    let stop = AtomicBool::new(false);
    let started = Instant::now();
    let outcome = run_timed(
        &run_of(600),
        &mut scripted,
        FirstWriteSleeps {
            inner: model.clone(),
            first: Duration::from_secs(3),
            writes: 0,
        },
        ManualClocks::at(noon()),
        &stop,
        SETTLE_BOUND,
    )
    .unwrap();
    let took = started.elapsed();
    assert_eq!(outcome.status, exit::TIMED_RUN_INTERRUPTED, "{outcome:?}");
    assert!(outcome.loss.is_none(), "{:?}", outcome.loss);
    assert_eq!(
        model.records().len() as u64 + outcome.report.events_discarded,
        600,
        "every captured event is stored or discarded at the run's own pause"
    );
    assert!(took >= Duration::from_secs(3), "a wait was cut: {took:?}");
    assert!(
        took <= stop_bound(SETTLE_BOUND),
        "the stop took {took:?}, more than its named bounds {:?}",
        stop_bound(SETTLE_BOUND)
    );
}

/// Sources that emit one more event while they stop, after the pause.
pub(super) struct EmitAtStop {
    pub(super) inner: ScriptedRun,
    pub(super) sink: Option<crate::sink::SinkHandle>,
}

impl RunSources for EmitAtStop {
    fn start(
        &mut self,
        ctx: &SourceContext,
        wanted: &[SourceName],
        hold_after: Option<Duration>,
    ) -> Vec<FailedSource> {
        self.sink = Some(ctx.sink().clone());
        self.inner.start(ctx, wanted, hold_after)
    }

    fn wait(&mut self, window: Duration, stop: &AtomicBool) -> bool {
        self.inner.wait(window, stop)
    }

    fn stop(&mut self) -> Vec<FailedSource> {
        if let Some(sink) = self.sink.take() {
            sink.emit_event(foreground_now());
            std::thread::sleep(Duration::from_millis(300));
        }
        self.inner.stop()
    }

    fn figures(&self) -> SourceCounters {
        self.inner.figures()
    }

    fn hold_refusal(&self) -> Option<DaemonError> {
        self.inner.hold_refusal()
    }
}

/// Ruling DM-12 G1 (units/daemon-1.md line 72): the run pauses the owner
/// before the sources stop, so an event that a source emits inside its stop
/// is not stored and is counted as discarded (adversary 1's adv1_order).
#[test]
fn an_event_that_a_source_emits_inside_its_stop_is_discarded_and_not_stored() {
    let store = ModelStore::new();
    let mut sources = EmitAtStop {
        inner: keys(4),
        sink: None,
    };
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(5),
        &mut sources,
        store.clone(),
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(store.records().len(), 4);
    assert_eq!(
        (
            outcome.report.events_persisted,
            outcome.report.events_discarded
        ),
        (4, 1),
        "the event that a source emitted while it stopped"
    );
}

/// Ruling DM-12 G2: each figure of the report comes from its source: the
/// sources' counters, and the owner's counts with events_received read apart
/// from persisted and discarded (a refused store leaves 4 events lost, and an
/// event emitted inside the stop is discarded).
#[test]
fn each_figure_of_the_report_comes_from_its_source() {
    let store = ModelStore::new();
    store.fail_every_write(Some(WriteFault::Refused));
    let mut inner = four_events();
    inner.figures = SourceCounters {
        secure_input_keystrokes: 3,
        tap_disabled_timeout: 2,
        tap_disabled_user_input: 1,
        secure_released_keystrokes: 5,
        secure_held_keystrokes: 7,
        secure_held_suppressed: SuppressedKeys {
            down: 1,
            up: 2,
            modifier: 3,
        },
        secure_input_held: HeldSamples { held: 4, taken: 6 },
        secure_window_pointer: 8,
        secure_window_keys_typed: true,
        ..SourceCounters::default()
    };
    let figures = inner.figures;
    let mut sources = EmitAtStop { inner, sink: None };
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &TimedRun {
            secure_input: SecureInput::Hold,
            ..run_of(5)
        },
        &mut sources,
        store,
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    )
    .unwrap();
    let report = &outcome.report;
    let counts = report.screen_frame
        + report.keystroke
        + report.pointer
        + report.foreground_app
        + report.window_title
        + report.system_audio
        + report.microphone
        + report.camera;
    assert_eq!((report.keystroke, report.foreground_app), (3, 2));
    assert_eq!(
        report.events_received, counts,
        "events_received against the count lines"
    );
    assert_eq!(report.events_received, 5);
    assert_eq!(report.events_persisted, 0);
    assert_eq!(report.events_discarded, 1);
    assert_eq!(
        report.secure_input_keystrokes,
        figures.secure_input_keystrokes
    );
    assert_eq!(report.tap_disabled_timeout, figures.tap_disabled_timeout);
    assert_eq!(
        report.tap_disabled_user_input,
        figures.tap_disabled_user_input
    );
    let hold = report
        .hold
        .as_ref()
        .expect("no hold lines in a run that holds");
    assert_eq!(hold.released_keystrokes, figures.secure_released_keystrokes);
    assert_eq!(hold.held_keystrokes, figures.secure_held_keystrokes);
    assert_eq!(hold.suppressed, figures.secure_held_suppressed);
    assert_eq!(hold.held_samples, figures.secure_input_held);
    assert_eq!(hold.window_pointer, figures.secure_window_pointer);
    assert_eq!(hold.keys_typed, figures.secure_window_keys_typed);
}

/// Ruling DM-12 G6 (DM-4 item 5): the last settle waits for the next due try,
/// so writes that fail three times at the last settle are stored by a later
/// try inside the bound (adversary 1's adv1_w_e, with three failures, because
/// the Shutdown's own settle makes one more try after the settles' loop).
#[test]
fn writes_that_fail_at_the_last_settle_are_stored_by_a_later_try() {
    let store = ModelStore::new();
    let fault = WriteFault::Io(io::ErrorKind::Other);
    store.script_writes([
        WriteScript::Fail(fault),
        WriteScript::Fail(fault),
        WriteScript::Fail(fault),
    ]);
    let mut scripted = keys(4);
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(5),
        &mut scripted,
        store.clone(),
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        (outcome.status, outcome.report.events_persisted),
        (exit::CLEAN, 4),
        "{outcome:?}"
    );
}

/// Ruling DM-12 G7 (contracts.md C5.2): the sink's counts are read after the
/// sources stopped, so an event emitted while they stop is in the accepted
/// count (adversary 1's adv1_w_l).
#[test]
fn the_sink_counts_hold_an_event_emitted_while_the_sources_stop() {
    let mut sources = EmitAtStop {
        inner: keys(4),
        sink: None,
    };
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(5),
        &mut sources,
        ModelStore::new(),
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(accepted(&outcome), 5);
}

/// Sources that learn of the system's refusal of the hold at their stop, as
/// MacSources does.
struct RefuseAtStop {
    inner: ScriptedRun,
}

impl RunSources for RefuseAtStop {
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
        self.inner.refusal = Some(-25_293);
        self.inner.stop()
    }

    fn figures(&self) -> SourceCounters {
        self.inner.figures()
    }

    fn hold_refusal(&self) -> Option<DaemonError> {
        self.inner.hold_refusal()
    }
}

/// Ruling DM-12 G8 (contracts.md C4 item 10): the hold's refusal is read
/// after the sources stop, so a refusal that the stop learns ends the run
/// with it and no report.
#[test]
fn a_refusal_that_the_sources_learn_at_their_stop_ends_the_run() {
    let mut sources = RefuseAtStop {
        inner: four_events(),
    };
    let stop = AtomicBool::new(false);
    let answer = run_timed(
        &TimedRun {
            secure_input: SecureInput::Hold,
            ..run_of(5)
        },
        &mut sources,
        ModelStore::new(),
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    );
    assert!(
        matches!(answer, Err(DaemonError::SecureInputRefused(-25_293))),
        "{answer:?}"
    );
}

/// Ruling DM-12 G3: the binary answers a refused command line with its usage
/// text and exit 64.
#[test]
fn a_usage_refusal_gives_its_text_and_exit_64() {
    let mut err = Vec::new();
    let status = super::usage_answer(&super::Usage("SECONDS is a decimal integer"), &mut err);
    assert_eq!(status, 64);
    let text = String::from_utf8(err).unwrap();
    assert!(
        text.starts_with("halvo-capture: SECONDS is a decimal integer\n"),
        "{text}"
    );
}

/// Ruling DM-12 G3 (contracts.md line 216): the binary prints the report,
/// prints the loss line on stderr and returns the run's status.
#[test]
fn an_outcome_with_a_loss_prints_its_report_and_line_and_returns_its_status() {
    let store = ModelStore::new();
    store.fail_every_write(Some(WriteFault::Refused));
    let mut scripted = four_events();
    let stop = AtomicBool::new(false);
    let outcome = run_timed(
        &run_of(5),
        &mut scripted,
        store,
        ManualClocks::at(noon()),
        &stop,
        Duration::from_secs(2),
    )
    .unwrap();
    let line = outcome.loss.as_ref().expect("no loss").line.clone();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = super::outcome_answer(&outcome, &mut out, &mut err);
    assert_eq!(status, exit::ERROR);
    assert_eq!(String::from_utf8(out).unwrap(), outcome.report.render());
    assert_eq!(String::from_utf8(err).unwrap(), format!("{line}\n"));
}

/// A model store whose first expiry after `armed` sleeps `sleep`, and says so
/// by `entered`.
struct ExpiryArmed {
    inner: ModelStore,
    armed: Arc<AtomicBool>,
    entered: Arc<AtomicBool>,
    sleep: Duration,
}

impl MaintainStore for ExpiryArmed {
    fn expire_due(&mut self, now: SystemTime, max: usize) -> Result<Pass, MaintFault> {
        if self.armed.load(Ordering::SeqCst) && !self.entered.swap(true, Ordering::SeqCst) {
            std::thread::sleep(self.sleep);
        }
        self.inner.expire_due(now, max)
    }

    fn evict_to(&mut self, bound: ByteBound, max: usize) -> Result<Pass, MaintFault> {
        self.inner.evict_to(bound, max)
    }

    fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
        self.inner.usage()
    }
}

impl CaptureStore for ExpiryArmed {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        self.inner.write_batch(write)
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        self.inner.volume_free_bytes()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// After the window, before the drain wait and the pause.
    Wait,
    /// After the pause's answer, before the settle.
    Stop,
    /// After the last settle, before the Shutdown.
    Figures,
}

/// Sources that run a hook at one phase of the stop path.
struct PhaseHook<F: FnMut()> {
    inner: ScriptedRun,
    phase: Phase,
    hook: std::cell::RefCell<F>,
}

impl<F: FnMut()> RunSources for PhaseHook<F> {
    fn start(
        &mut self,
        ctx: &SourceContext,
        wanted: &[SourceName],
        hold_after: Option<Duration>,
    ) -> Vec<FailedSource> {
        self.inner.start(ctx, wanted, hold_after)
    }

    fn wait(&mut self, window: Duration, stop: &AtomicBool) -> bool {
        let signalled = self.inner.wait(window, stop);
        if self.phase == Phase::Wait {
            (self.hook.borrow_mut())();
        }
        signalled
    }

    fn stop(&mut self) -> Vec<FailedSource> {
        if self.phase == Phase::Stop {
            (self.hook.borrow_mut())();
        }
        self.inner.stop()
    }

    fn figures(&self) -> SourceCounters {
        if self.phase == Phase::Figures {
            (self.hook.borrow_mut())();
        }
        self.inner.figures()
    }

    fn hold_refusal(&self) -> Option<DaemonError> {
        self.inner.hold_refusal()
    }
}

/// A run whose owner is held in an expiry of `held` from `phase` on, with
/// `bound` as the settle bound.
fn owner_held_at(phase: Phase, held: Duration, bound: Duration) -> RunOutcome {
    let clocks = ManualClocks::at(noon());
    let armed = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicBool::new(false));
    let store = ExpiryArmed {
        inner: ModelStore::new(),
        armed: Arc::clone(&armed),
        entered: Arc::clone(&entered),
        sleep: held,
    };
    let hook_clocks = clocks.clone();
    let hook = move || {
        std::thread::sleep(Duration::from_millis(100));
        armed.store(true, Ordering::SeqCst);
        hook_clocks.advance(Duration::from_secs(61));
        let until = Instant::now() + Duration::from_secs(5);
        while !entered.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(entered.load(Ordering::SeqCst), "the owner ran no expiry");
    };
    let mut sources = PhaseHook {
        inner: four_events(),
        phase,
        hook: std::cell::RefCell::new(hook),
    };
    let stop = AtomicBool::new(false);
    run_timed(&run_of(5), &mut sources, store, clocks, &stop, bound).unwrap()
}

/// Ruling DM-13 (3): the named waits of the stop path take their bounds from
/// one value. Its parts at the product's values sum to 17 s, and each wait
/// gives up at its own part: the drain wait and the pause of an owner parked
/// in a write, and the pause, the settle and the Shutdown of an owner held for
/// half a second past its part; a pause answered inside its part is taken.
#[test]
fn each_named_wait_of_the_stop_path_gives_up_at_its_own_part() {
    assert_eq!(
        StopBounds::of(SETTLE_BOUND),
        StopBounds {
            drain: Duration::from_secs(2),
            pause: Duration::from_secs(5),
            settle: Duration::from_secs(5),
            shutdown: Duration::from_secs(5),
        }
    );
    assert_eq!(stop_bound(SETTLE_BOUND), Duration::from_secs(17));
    assert_eq!(StopBounds::of(SETTLE_BOUND).sum(), stop_bound(SETTLE_BOUND));

    let bound = Duration::from_secs(1);
    let tick = Duration::from_millis(500);
    let cause = |outcome: &RunOutcome| outcome.loss.as_ref().map(|loss| loss.cause);

    let (store, parked) = ParkingStore::parked(ModelStore::new());
    let mut scripted = keys(1_200);
    let stop = AtomicBool::new(false);
    let started = Instant::now();
    let outcome = run_timed(
        &run_of(5),
        &mut scripted,
        store,
        ManualClocks::at(noon()),
        &stop,
        bound,
    )
    .unwrap();
    let took = started.elapsed();
    parked.store(false, Ordering::SeqCst);
    assert_eq!(cause(&outcome), Some(LossCause::PauseUnanswered));
    assert!(
        took >= DRAIN_BOUND + bound && took < DRAIN_BOUND + bound + Duration::from_secs(1),
        "the drain wait and the pause of a parked owner took {took:?}"
    );

    let outcome = owner_held_at(Phase::Wait, bound + tick, bound);
    assert_eq!(
        cause(&outcome),
        Some(LossCause::PauseUnanswered),
        "{outcome:?}"
    );
    let outcome = owner_held_at(Phase::Wait, bound - tick / 2, bound);
    assert!(
        outcome.loss.is_none(),
        "a pause answered inside its part was not taken: {outcome:?}"
    );
    let outcome = owner_held_at(Phase::Stop, bound + tick, bound);
    assert_eq!(
        cause(&outcome),
        Some(LossCause::SettleUnanswered),
        "{outcome:?}"
    );
    let outcome = owner_held_at(Phase::Figures, bound + tick, bound);
    assert_eq!(
        cause(&outcome),
        Some(LossCause::ShutdownUnanswered),
        "{outcome:?}"
    );
}

/// What the write number `nth` of an [`NthWrite`] does first.
enum AtWrite {
    /// Sleeps; `nth` 0 sleeps at every write.
    Sleep(Duration),
    /// Waits until the run's figures hook opens the gate, and at most 10 s.
    Gate(mpsc::Receiver<()>),
    /// Panics, which ends the owner.
    Panic,
}

/// A model store whose every write answers Io and whose write number `nth`
/// does what `at` says first.
struct NthWrite {
    inner: ModelStore,
    nth: u64,
    at: AtWrite,
    writes: u64,
}

impl NthWrite {
    fn new(nth: u64, at: AtWrite) -> NthWrite {
        let inner = ModelStore::new();
        inner.fail_every_write(Some(WriteFault::Io(io::ErrorKind::Other)));
        NthWrite {
            inner,
            nth,
            at,
            writes: 0,
        }
    }
}

impl MaintainStore for NthWrite {
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

impl CaptureStore for NthWrite {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        self.writes += 1;
        if self.writes == self.nth || self.nth == 0 {
            match &self.at {
                AtWrite::Sleep(sleep) => std::thread::sleep(*sleep),
                AtWrite::Gate(gate) => {
                    let _ = gate.recv_timeout(Duration::from_secs(10));
                }
                AtWrite::Panic => panic!("the store's write {} panics", self.writes),
            }
        }
        self.inner.write_batch(write)
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        self.inner.volume_free_bytes()
    }
}

fn run_over(store: NthWrite, bound: Duration, gate: Option<mpsc::Sender<()>>) -> RunOutcome {
    let mut scripted = four_events();
    if let Some(gate) = gate {
        scripted.on_figures = Some(Box::new(move || {
            let _ = gate.send(());
        }));
    }
    let stop = AtomicBool::new(false);
    run_timed(
        &run_of(5),
        &mut scripted,
        store,
        ManualClocks::at(noon()),
        &stop,
        bound,
    )
    .unwrap()
}

/// Ruling DM-14 (5): the settle's first ask is answered while the drain
/// fails, and its second is held by a write that waits until the figures
/// hook, which runs after the settle has given up, opens its gate: the run
/// goes on to the Shutdown and reports from its answer, with the cause
/// Pending, the count of that answer's lost and pending events and the kind
/// read from the reports.
#[test]
fn a_late_second_ask_of_the_settle_leaves_the_first_answer_and_the_shutdown_reports() {
    let (open, gate) = mpsc::channel();
    let outcome = run_over(
        NthWrite::new(2, AtWrite::Gate(gate)),
        Duration::from_secs(2),
        Some(open),
    );
    assert_eq!(outcome.status, exit::ERROR, "{outcome:?}");
    let loss = outcome.loss.expect("no loss");
    assert_eq!(loss.cause, LossCause::Pending, "{}", loss.line);
    assert!(
        loss.line.starts_with("halvo-capture: lost events=4 ") && loss.line.ends_with(" error=io"),
        "{}",
        loss.line
    );
}

/// Ruling DM-14 (4), accepted in words: an owner that is gone after its first
/// answer gives ShutdownUnanswered, status 1, and a count of at least the
/// events that no write stored.
#[test]
fn an_owner_gone_after_its_first_answer_gives_an_unanswered_shutdown() {
    let outcome = run_over(
        NthWrite::new(2, AtWrite::Panic),
        Duration::from_secs(2),
        None,
    );
    assert_eq!(outcome.status, exit::ERROR, "{outcome:?}");
    assert_eq!(
        outcome.loss.as_ref().map(|loss| loss.cause),
        Some(LossCause::ShutdownUnanswered)
    );
    assert!(
        lost_in_line(&outcome).is_some_and(|count| count >= 4),
        "{outcome:?}"
    );
}

/// Ruling DM-14 (5): a store that takes about 0.6 of the Shutdown's bound for
/// each write; the test prints what the run gives. Status 1 with Pending or
/// with ShutdownUnanswered and the upper bound is accepted, and the stop
/// path's 17 s stand.
#[test]
fn a_store_that_takes_most_of_each_bound_for_each_write_still_ends_inside_the_stop_bound() {
    let bound = Duration::from_secs(2);
    let started = Instant::now();
    let outcome = run_over(
        NthWrite::new(0, AtWrite::Sleep(bound * 6 / 10)),
        bound,
        None,
    );
    let took = started.elapsed();
    println!(
        "slow store: status={} cause={:?} line={:?} took={took:?}",
        outcome.status,
        outcome.loss.as_ref().map(|loss| loss.cause),
        outcome.loss.as_ref().map(|loss| loss.line.clone())
    );
    assert_eq!(outcome.status, exit::ERROR);
    assert!(matches!(
        outcome.loss.as_ref().map(|loss| loss.cause),
        Some(LossCause::Pending | LossCause::ShutdownUnanswered)
    ));
    assert!(
        lost_in_line(&outcome).is_some_and(|count| count >= 4),
        "{outcome:?}"
    );
    assert!(took <= stop_bound(bound), "{took:?}");
    assert_eq!(stop_bound(SETTLE_BOUND), Duration::from_secs(17));
}
