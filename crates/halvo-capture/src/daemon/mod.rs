//! The capture daemon: the store owner, which is the one thread that holds the
//! capture store, and the gate's timed run on that owner.

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod barrier;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod batch;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod counters;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod discard_port;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod maintenance;
#[cfg(test)]
mod model_store;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod owner;
#[cfg(test)]
mod owner_props;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod port;
#[rustfmt::skip]
#[cfg(feature = "r2")] mod r2;
mod report;
#[cfg(test)]
mod scripted;
pub mod signal;

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use halvo_control::paths::AccountLookup;

use crate::floor::{Switch, Switches};
use crate::source::{SourceContext, SourceCounters};

pub use report::{FailedSource, HoldLines, RunOutcome, TimedRunReport};

/// The longest timed run: one week.
pub const SECONDS_MAX: u64 = 604_800;

/// How long a timed run waits for the owner's last settle.
pub const SETTLE_BOUND: Duration = Duration::from_secs(5);

/// How long a timed run waits at its deadline, before it pauses the owner,
/// for the owner to receive the events that the sink took inside the window.
pub const DRAIN_BOUND: Duration = Duration::from_secs(2);

/// The text that a refused command line prints after its reason.
pub const USAGE: &str = "usage: halvo-capture --version
       halvo-capture --timed-run SECONDS --store-dir DIR [--hold-secure-input] [--switch NAME=on|off]...
       halvo-capture --verify-store --store-dir DIR --marker MARKER
       halvo-capture --verify-deletion --store-dir DIR";

/// What a command line asks for. The gate's modes exist only in builds with
/// debug assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Version,
    #[cfg(debug_assertions)]
    TimedRun(TimedRun),
    #[cfg(debug_assertions)]
    VerifyStore {
        store_dir: PathBuf,
        marker: String,
    },
    #[cfg(debug_assertions)]
    VerifyDeletion {
        store_dir: PathBuf,
    },
}

/// A refused command line, with its reason; the binary prints it and
/// [`USAGE`] and exits 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("halvo-capture: {0}\n{USAGE}")]
pub struct Usage(pub &'static str);

/// Whether a timed run holds secure input in its second phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureInput {
    Hold,
    DoNotHold,
}

/// One timed run as its flags give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedRun {
    pub window: Duration,
    pub secure_input: SecureInput,
    pub switches: SwitchFlags,
    pub store_dir: PathBuf,
}

impl TimedRun {
    /// The released phase of a run with the hold: floor(SECONDS/2) seconds.
    pub fn hold_after(&self) -> Option<Duration> {
        match self.secure_input {
            SecureInput::Hold => Some(Duration::from_secs(self.window.as_secs() / 2)),
            SecureInput::DoNotHold => None,
        }
    }
}

/// The switches that `--switch` flags set for one run, each name once, with
/// its value. Nothing of it is written anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SwitchFlags(Vec<(Switch, bool)>);

impl SwitchFlags {
    /// The names that a flag set, in the order of the flags.
    pub fn set_by_flags(&self) -> &[(Switch, bool)] {
        &self.0
    }

    /// The defaults of contracts.md C13.5 with the flags over them.
    pub fn applied(&self) -> Switches {
        let mut switches = Switches::default();
        for (switch, on) in &self.0 {
            switches.set(*switch, *on);
        }
        switches
    }
}

/// Parses the arguments after the program name, read with `args_os`. The
/// store directory of a gate mode is checked against the data directory that
/// `account` gives.
pub fn parse(
    args: impl IntoIterator<Item = OsString>,
    account: &dyn AccountLookup,
) -> Result<Mode, Usage> {
    let args = args
        .into_iter()
        .map(OsString::into_string)
        .collect::<Result<Vec<String>, OsString>>()
        .map_err(|_| Usage("an argument is not valid UTF-8"))?;
    #[cfg(not(debug_assertions))]
    let _ = account;
    match args.first().map(String::as_str) {
        Some("--version") if args.len() == 1 => Ok(Mode::Version),
        #[cfg(debug_assertions)]
        Some("--timed-run") => gate::timed_run(&args[1..], account),
        #[cfg(debug_assertions)]
        Some("--verify-store") => {
            let flags = gate::Flags::read(&args[1..], &["--store-dir", "--marker"])?;
            let store_dir = gate::store_dir(flags.store_dir, account)?;
            let marker = flags.marker.ok_or(Usage("--verify-store needs --marker"))?;
            if !gate::is_marker(&marker) {
                return Err(Usage(
                    "MARKER is not 8 to 64 of A-Z, a-z, 0-9, '.', '_' and '-'",
                ));
            }
            Ok(Mode::VerifyStore { store_dir, marker })
        }
        #[cfg(debug_assertions)]
        Some("--verify-deletion") => {
            let flags = gate::Flags::read(&args[1..], &["--store-dir"])?;
            let store_dir = gate::store_dir(flags.store_dir, account)?;
            Ok(Mode::VerifyDeletion { store_dir })
        }
        _ => Err(Usage("no mode of this build")),
    }
}

#[cfg(debug_assertions)]
mod gate {
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::time::Duration;

    use halvo_control::paths::{AccountLookup, ScratchDir};

    use super::{Mode, SecureInput, SwitchFlags, TimedRun, Usage, SECONDS_MAX};
    use crate::floor::Switch;

    #[derive(Default)]
    pub(super) struct Flags {
        pub(super) store_dir: Option<String>,
        pub(super) marker: Option<String>,
        pub(super) hold: bool,
        pub(super) switches: SwitchFlags,
    }

    impl Flags {
        pub(super) fn read(args: &[String], allowed: &[&str]) -> Result<Flags, Usage> {
            let mut flags = Flags::default();
            let mut args = args.iter();
            while let Some(flag) = args.next() {
                if !allowed.contains(&flag.as_str()) {
                    return Err(Usage("a flag that this mode does not take"));
                }
                match flag.as_str() {
                    "--store-dir" => {
                        let dir = args.next().ok_or(Usage("--store-dir needs DIR"))?;
                        if flags.store_dir.replace(dir.clone()).is_some() {
                            return Err(Usage("--store-dir is given twice"));
                        }
                    }
                    "--marker" => {
                        let marker = args.next().ok_or(Usage("--marker needs MARKER"))?;
                        if flags.marker.replace(marker.clone()).is_some() {
                            return Err(Usage("--marker is given twice"));
                        }
                    }
                    "--hold-secure-input" => {
                        if flags.hold {
                            return Err(Usage("--hold-secure-input is given twice"));
                        }
                        flags.hold = true;
                    }
                    "--switch" => {
                        let value = args.next().ok_or(Usage("--switch needs NAME=on|off"))?;
                        let (switch, on) = switch_flag(value)?;
                        if flags.switches.0.iter().any(|(set, _)| *set == switch) {
                            return Err(Usage("a switch is named twice"));
                        }
                        flags.switches.0.push((switch, on));
                    }
                    _ => return Err(Usage("a flag that this mode does not take")),
                }
            }
            Ok(flags)
        }
    }

    fn switch_flag(value: &str) -> Result<(Switch, bool), Usage> {
        let (name, on) = value
            .split_once('=')
            .ok_or(Usage("--switch takes NAME=on|off"))?;
        let switch = Switch::from_str(name).map_err(|_| Usage("no switch has that name"))?;
        match on {
            "on" => Ok((switch, true)),
            "off" => Ok((switch, false)),
            _ => Err(Usage("a switch is on or off")),
        }
    }

    pub(super) fn timed_run(args: &[String], account: &dyn AccountLookup) -> Result<Mode, Usage> {
        let (seconds, flags) = args
            .split_first()
            .ok_or(Usage("--timed-run needs SECONDS"))?;
        let flags = Flags::read(flags, &["--store-dir", "--hold-secure-input", "--switch"])?;
        let seconds = seconds_of(seconds)?;
        let secure_input = if flags.hold {
            if cfg!(target_os = "windows") {
                return Err(Usage("--hold-secure-input is a flag of macOS"));
            }
            if seconds < 2 {
                return Err(Usage("SECONDS is at least 2 with --hold-secure-input"));
            }
            SecureInput::Hold
        } else {
            SecureInput::DoNotHold
        };
        let store_dir = store_dir(flags.store_dir, account)?;
        Ok(Mode::TimedRun(TimedRun {
            window: Duration::from_secs(seconds),
            secure_input,
            switches: flags.switches,
            store_dir,
        }))
    }

    fn seconds_of(text: &str) -> Result<u64, Usage> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Usage("SECONDS is a decimal integer"));
        }
        match text.parse::<u64>() {
            Ok(seconds @ 1..=SECONDS_MAX) => Ok(seconds),
            _ => Err(Usage("SECONDS is at least 1 and at most 604800")),
        }
    }

    pub(super) fn store_dir(
        dir: Option<String>,
        account: &dyn AccountLookup,
    ) -> Result<PathBuf, Usage> {
        let dir = dir.ok_or(Usage("--store-dir DIR is required"))?;
        let scratch = ScratchDir::new(PathBuf::from(dir), account)
            .map_err(|_| Usage("DIR is no absolute directory clear of the data directory"))?;
        if scratch.path().join("store.key").symlink_metadata().is_ok() {
            return Err(Usage("DIR holds store.key"));
        }
        Ok(scratch.path().to_path_buf())
    }

    pub(super) fn is_marker(marker: &str) -> bool {
        (8..=64).contains(&marker.len())
            && marker
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    }
}

/// A source of a timed run, by the name that its `failed_source=` line takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceName {
    Foreground,
    Screen,
    Input,
}

impl SourceName {
    pub const fn as_str(self) -> &'static str {
        match self {
            SourceName::Foreground => "foreground",
            SourceName::Screen => "screen",
            SourceName::Input => "input",
        }
    }
}

/// The `failed_source=` entry of a source's answer to its stop at the run's
/// end: none for `Ok` and for `ThreadEnded`, an entry for every other answer
/// (contracts.md C4 item 7).
pub fn stop_failure(
    name: SourceName,
    answer: Result<(), crate::source::CaptureError>,
) -> Option<FailedSource> {
    match answer {
        Ok(()) | Err(crate::source::CaptureError::ThreadEnded) => None,
        Err(error) => Some(FailedSource {
            name: name.as_str(),
            error: error.to_string(),
        }),
    }
}

/// The sources that a run with `switches` starts: the foreground source, the
/// screen source only while `screen_activity` is on, and the input source
/// only while `key_class` or `pointer_class` is on.
pub fn sources_for(switches: Switches) -> Vec<SourceName> {
    let mut sources = vec![SourceName::Foreground];
    if switches.is_on(Switch::ScreenActivity) {
        sources.push(SourceName::Screen);
    }
    if switches.is_on(Switch::KeyClass) || switches.is_on(Switch::PointerClass) {
        sources.push(SourceName::Input);
    }
    sources
}

/// The sources of one timed run on one system.
pub trait RunSources {
    /// Starts each source of `wanted` in `ctx`; the input source holds secure
    /// input from `hold_after` on when it is given. Returns the sources that
    /// did not start.
    fn start(
        &mut self,
        ctx: &SourceContext,
        wanted: &[SourceName],
        hold_after: Option<Duration>,
    ) -> Vec<FailedSource>;

    /// Waits for `window`, or until `stop` reads true; true when `stop` ended
    /// the wait.
    fn wait(&mut self, window: Duration, stop: &std::sync::atomic::AtomicBool) -> bool;

    /// Stops every source that started; returns the sources whose stop failed.
    fn stop(&mut self) -> Vec<FailedSource>;

    /// The counters that the sources keep.
    fn figures(&self) -> SourceCounters;

    /// The system's refusal of the hold of secure input, when there was one.
    fn hold_refusal(&self) -> Option<DaemonError>;
}

/// Runs one timed run on the owner: the owner over `store` resumes, the
/// sources start and run for the window, the owner receives what the sink took
/// in the window (at most [`DRAIN_BOUND`]), the owner pauses, the sources stop
/// and the owner settles. The status is 1 with a loss line when the last
/// settle finds the drain failing, events lost since the last acknowledged
/// write or more events lost than before the first settle, or gives no answer
/// inside `settle_bound`; else 2 when `stop` ended the window, 3 when a source
/// failed, and 0. The pause and the shutdown each wait at most `settle_bound`
/// for the owner's answer, and an owner that gives none is not joined.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub fn run_timed<P, S, C>(
    run: &TimedRun,
    sources: &mut P,
    store: S,
    clocks: C,
    stop: &std::sync::atomic::AtomicBool,
    settle_bound: Duration,
) -> Result<RunOutcome, DaemonError>
where
    P: RunSources,
    S: port::CaptureStore + Send + 'static,
    C: maintenance::Clocks + 'static,
{
    use std::sync::Arc;

    use halvo_control::exit;
    use halvo_control::model::StorageCap;

    use crate::floor::exclude::{FloorHandle, NoLineage};
    use owner::{Answer, Command, OwnerParts};

    let switches = run.switches.applied();
    let floor = FloorHandle::new(None, Arc::new(NoLineage), switches);
    let (handle, writer) = crate::sink::channel(owner::QUEUE_SLOTS);
    let owner = owner::spawn(OwnerParts {
        writer,
        store,
        clocks,
        floor: floor.clone(),
        cap: StorageCap::DEFAULT,
        shred_chunk: maintenance::SHRED_CHUNK,
    })
    .map_err(|error| DaemonError::Store(error.to_string()))?;
    if !matches!(
        owner.handle.call(Command::ClosePause),
        Ok(Answer::Resumed(_))
    ) {
        return Err(DaemonError::DrainPanicked);
    }
    let ctx = SourceContext::with_floor(handle.clone(), floor);
    let mut failed = sources.start(&ctx, &sources_for(switches), run.hold_after());
    drop(ctx);
    let signalled = sources.wait(run.window, stop);
    let counts = handle.counts();
    let emitted: u64 = crate::event::Modality::ALL
        .into_iter()
        .map(|kind| counts.of(kind).accepted)
        .sum();
    let drained_by = std::time::Instant::now() + DRAIN_BOUND;
    while owner.snapshot().events_received() < emitted && std::time::Instant::now() < drained_by {
        std::thread::sleep(Duration::from_millis(5));
    }
    let paused = answer_within(&owner.handle, Command::OpenPause, settle_bound);
    failed.extend(sources.stop());
    let sink_counts = handle.counts();
    drop(handle);
    let before = owner.snapshot();
    let last = match paused {
        Some(Answer::Paused(_)) => settle_within(&owner.handle, settle_bound),
        _ => None,
    };
    let reports: Vec<owner::Report> = owner.reports.try_iter().collect();
    let figures = sources.figures();
    if let Some(refusal) = sources.hold_refusal() {
        return Err(refusal);
    }
    let published = owner.snapshot();
    let last = match last {
        Some(snapshot) => match answer_within(&owner.handle, Command::Shutdown, settle_bound) {
            Some(Answer::ShutDown(_)) => {
                owner.join().map_err(|_| DaemonError::DrainPanicked)?;
                Some(snapshot)
            }
            _ => None,
        },
        None => None,
    };
    let settled = last.unwrap_or(published);
    let loss = (last.is_none()
        || settled.events_pending > 0
        || settled.lost_since_acknowledged > 0
        || settled.events_lost > before.events_lost)
        .then(|| loss_line(&settled, last.is_some(), &reports));
    let status = if loss.is_some() {
        exit::ERROR
    } else if signalled {
        exit::TIMED_RUN_INTERRUPTED
    } else if !failed.is_empty() {
        exit::TIMED_RUN_SOURCE_FAILED
    } else {
        exit::CLEAN
    };
    Ok(RunOutcome {
        report: report_of(&settled, &sink_counts, &figures, failed, run.secure_input),
        status,
        loss,
    })
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn answer_within(
    handle: &owner::OwnerHandle,
    command: owner::Command,
    bound: Duration,
) -> Option<owner::Answer> {
    handle.ask(command).ok()?.wait_for(bound)
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn settle_within(handle: &owner::OwnerHandle, bound: Duration) -> Option<counters::Snapshot> {
    let started = std::time::Instant::now();
    loop {
        let pending = handle.ask(owner::Command::Settle).ok()?;
        let left = bound.saturating_sub(started.elapsed());
        let owner::Answer::Settled(snapshot) = pending.wait_for(left)? else {
            return None;
        };
        let left = bound.saturating_sub(started.elapsed());
        if snapshot.events_pending == 0 || left < 2 * owner::POLL {
            return Some(snapshot);
        }
        std::thread::sleep(owner::POLL);
    }
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn loss_line(settled: &counters::Snapshot, answered: bool, reports: &[owner::Report]) -> String {
    let instant = |at: Option<i64>| {
        at.and_then(|nanos| {
            halvo_control::model::UtcSecond::from_unix_seconds(nanos.div_euclid(1_000_000_000)).ok()
        })
        .map_or_else(|| "-".to_owned(), |second| second.to_string())
    };
    let final_failed = !answered || settled.events_pending > 0;
    let kind = reports
        .iter()
        .rev()
        .find_map(|report| match report {
            owner::Report::DrainFailing { cause, .. } => Some(cause.as_str()),
            _ => None,
        })
        .filter(|_| final_failed)
        .unwrap_or("-");
    format!(
        "halvo-capture: lost events={} first={} last={} error={kind}",
        settled.events_lost + settled.events_pending,
        instant(settled.lost_first_at),
        instant(settled.lost_last_at),
    )
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn report_of(
    snapshot: &counters::Snapshot,
    sink: &crate::sink::SinkCounts,
    figures: &SourceCounters,
    failed_sources: Vec<FailedSource>,
    secure_input: SecureInput,
) -> TimedRunReport {
    use crate::event::Modality;
    TimedRunReport {
        screen_frame: snapshot.received(Modality::ScreenFrame),
        keystroke: snapshot.received(Modality::Keystroke),
        pointer: snapshot.received(Modality::Pointer),
        foreground_app: snapshot.received(Modality::ForegroundApp),
        window_title: snapshot.received(Modality::WindowTitle),
        system_audio: snapshot.received(Modality::SystemAudio),
        microphone: snapshot.received(Modality::Microphone),
        camera: snapshot.received(Modality::Camera),
        secure_input_keystrokes: figures.secure_input_keystrokes,
        segments_written: snapshot.segments_written,
        events_received: snapshot.events_received(),
        events_persisted: snapshot.events_persisted,
        events_discarded: snapshot.events_discarded(),
        sink_counts: Modality::ALL.map(|kind| sink.of(kind)),
        tap_disabled_timeout: figures.tap_disabled_timeout,
        tap_disabled_user_input: figures.tap_disabled_user_input,
        rss_peak_bytes: signal::peak_rss_bytes().unwrap_or(0),
        failed_sources,
        hold: (secure_input == SecureInput::Hold).then_some(HoldLines {
            released_keystrokes: figures.secure_released_keystrokes,
            held_keystrokes: figures.secure_held_keystrokes,
            suppressed: figures.secure_held_suppressed,
            held_samples: figures.secure_input_held,
            window_pointer: figures.secure_window_pointer,
            keys_typed: figures.secure_window_keys_typed,
        }),
    }
}

/// Runs the gate's timed run on this system.
#[cfg(debug_assertions)]
pub fn timed_run(run: &TimedRun) -> Result<RunOutcome, DaemonError> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::timed_run(run)
    }
    #[cfg(target_os = "windows")]
    {
        crate::windows::timed_run(run.window.as_secs()).map(|report| RunOutcome {
            report,
            status: halvo_control::exit::CLEAN,
            loss: None,
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = run;
        Err(DaemonError::NotImplemented("timed-run on this platform"))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("store error: {0}")]
    Store(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
    #[error("the system refused the secure-input hold (status {0})")]
    SecureInputRefused(i32),
    #[error("the store owner panicked")]
    DrainPanicked,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use halvo_control::exit;
    use halvo_control::paths::{AccountLookup, DarwinUserTempDir, DataDir, PathError};

    use super::model_store::{
        foreground_now, frame, key, noon, ManualClocks, ModelStore, WriteScript,
    };
    use super::port::{
        CaptureStore, MaintFault, MaintainStore, Pass, SegmentWrite, Usage as StoreUsage,
        WriteFault, Written,
    };
    use super::scripted::{ScriptedRun, ScriptedSource};
    use super::*;
    use crate::event::Modality;
    use crate::source::CaptureError;

    fn test_dir() -> tempfile::TempDir {
        let target = std::env::current_exe()
            .unwrap()
            .ancestors()
            .nth(3)
            .unwrap()
            .join("tmp");
        std::fs::create_dir_all(&target).unwrap();
        tempfile::Builder::new()
            .prefix("daemon-tests-")
            .tempdir_in(target)
            .unwrap()
    }

    struct TestAccount(PathBuf);

    impl AccountLookup for TestAccount {
        fn base_dir(&self) -> Result<Option<OsString>, PathError> {
            Ok(Some(self.0.clone().into_os_string()))
        }

        fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
            Ok(None)
        }
    }

    fn args(line: &[&str]) -> Vec<OsString> {
        line.iter().map(OsString::from).collect()
    }

    struct Dirs {
        _root: tempfile::TempDir,
        account: TestAccount,
        store: String,
        data: PathBuf,
    }

    fn dirs() -> Dirs {
        let root = test_dir();
        let home = root.path().join("home");
        let store = root.path().join("store");
        std::fs::create_dir_all(&store).unwrap();
        let account = TestAccount(home);
        let data = DataDir::for_account(&account).unwrap().path().to_path_buf();
        std::fs::create_dir_all(&data).unwrap();
        Dirs {
            store: store.to_str().unwrap().to_owned(),
            _root: root,
            account,
            data,
        }
    }

    fn timed(dirs: &Dirs, rest: &[&str]) -> Result<Mode, Usage> {
        let mut line = vec!["--timed-run"];
        line.extend_from_slice(rest);
        parse(args(&line), &dirs.account)
    }

    #[test]
    fn seconds_outside_1_to_604800_give_usage() {
        let dirs = dirs();
        for seconds in [
            "10s",
            "-5",
            "",
            "0",
            "+5",
            "1e3",
            "604801",
            "99999999999999999999",
        ] {
            assert!(
                timed(&dirs, &[seconds, "--store-dir", &dirs.store]).is_err(),
                "SECONDS {seconds:?} was taken"
            );
        }
        for seconds in ["1", "604800"] {
            assert!(timed(&dirs, &[seconds, "--store-dir", &dirs.store]).is_ok());
        }
    }

    #[test]
    fn a_bad_switch_flag_and_a_switch_beside_another_mode_give_usage() {
        let dirs = dirs();
        let d = dirs.store.as_str();
        for line in [
            &["1", "--store-dir", d, "--switch", "screen=on"][..],
            &["1", "--store-dir", d, "--switch", "screen_activity=maybe"],
            &["1", "--store-dir", d, "--switch", "key_class"],
            &["1", "--store-dir", d, "--switch"],
            &[
                "1",
                "--store-dir",
                d,
                "--switch",
                "key_class=on",
                "--switch",
                "key_class=off",
            ],
            &[
                "1",
                "--store-dir",
                d,
                "--switch",
                "key_class=on",
                "--switch",
                "key_class=on",
            ],
        ] {
            assert!(timed(&dirs, line).is_err(), "{line:?} was taken");
        }
        let deletion = parse(
            args(&[
                "--verify-deletion",
                "--store-dir",
                d,
                "--switch",
                "key_class=on",
            ]),
            &dirs.account,
        );
        assert!(deletion.is_err());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn two_switch_flags_parse_to_their_values_with_both_names_set() {
        let dirs = dirs();
        let d = dirs.store.as_str();
        let Ok(Mode::TimedRun(run)) = timed(
            &dirs,
            &[
                "3",
                "--store-dir",
                d,
                "--switch",
                "screen_activity=on",
                "--switch",
                "key_class=off",
            ],
        ) else {
            panic!("the flags were refused");
        };
        assert_eq!(
            run.switches.set_by_flags(),
            [(Switch::ScreenActivity, true), (Switch::KeyClass, false)]
        );
        let switches = run.switches.applied();
        assert!(switches.is_on(Switch::ScreenActivity));
        assert!(!switches.is_on(Switch::KeyClass));
        assert!(switches.is_on(Switch::TitleText));
        assert_eq!(run.window, Duration::from_secs(3));
    }

    #[test]
    fn a_store_dir_is_required_absolute_existing_and_clear_of_the_data_directory() {
        let dirs = dirs();
        assert!(timed(&dirs, &["1"]).is_err(), "no --store-dir");
        assert!(
            timed(&dirs, &["1", "--store-dir", "store"]).is_err(),
            "relative"
        );
        let inside = dirs.data.join("scratch");
        std::fs::create_dir_all(&inside).unwrap();
        for dir in [
            dirs.data.as_path(),
            inside.as_path(),
            dirs.account.0.as_path(),
        ] {
            assert!(
                timed(&dirs, &["1", "--store-dir", dir.to_str().unwrap()]).is_err(),
                "{dir:?} was taken"
            );
        }
        let missing = Path::new(&dirs.store).join("missing");
        assert!(timed(&dirs, &["1", "--store-dir", missing.to_str().unwrap()]).is_err());
        assert!(timed(
            &dirs,
            &["1", "--store-dir", &dirs.store, "--store-dir", &dirs.store]
        )
        .is_err());
        assert!(timed(&dirs, &["1", "--store-dir", &dirs.store]).is_ok());
        std::fs::write(Path::new(&dirs.store).join("store.key"), b"{}").unwrap();
        assert!(
            timed(&dirs, &["1", "--store-dir", &dirs.store]).is_err(),
            "store.key"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_verify_modes_take_their_flags_and_nothing_else() {
        let dirs = dirs();
        let d = dirs.store.as_str();
        let store = |line: &[&str]| parse(args(line), &dirs.account);
        assert!(matches!(
            store(&[
                "--verify-store",
                "--store-dir",
                d,
                "--marker",
                "M4rker.ok_-"
            ]),
            Ok(Mode::VerifyStore { .. })
        ));
        for line in [
            &["--verify-store", "--store-dir", d][..],
            &["--verify-store", "--store-dir", d, "--marker", "short"],
            &["--verify-store", "--store-dir", d, "--marker", "bad marker"],
            &["--verify-store", "--marker", "M4rker.ok_-"],
            &["--verify-deletion"],
            &["--verify-deletion", "--store-dir", d, "--hold-secure-input"],
            &["--timed-run"],
            &["--version", "--timed-run"],
            &["--delete-everything"],
            &[],
        ] {
            assert!(store(line).is_err(), "{line:?} was taken");
        }
        assert!(matches!(
            store(&["--verify-deletion", "--store-dir", d]),
            Ok(Mode::VerifyDeletion { .. })
        ));
        assert_eq!(store(&["--version"]), Ok(Mode::Version));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_hold_needs_two_seconds_and_is_a_flag_of_macos() {
        let dirs = dirs();
        let d = dirs.store.as_str();
        assert!(timed(&dirs, &["1", "--store-dir", d, "--hold-secure-input"]).is_err());
        let held = timed(&dirs, &["2", "--store-dir", d, "--hold-secure-input"]);
        if cfg!(target_os = "macos") {
            let Ok(Mode::TimedRun(run)) = held else {
                panic!("the hold was refused");
            };
            assert_eq!(run.secure_input, SecureInput::Hold);
        } else {
            assert!(held.is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_utf8_gives_usage() {
        use std::os::unix::ffi::OsStringExt;
        let dirs = dirs();
        let line = vec![
            OsString::from("--timed-run"),
            OsString::from_vec(vec![0x31, 0xff]),
            OsString::from("--store-dir"),
            OsString::from(&dirs.store),
        ];
        assert!(parse(line, &dirs.account).is_err());
    }

    fn run_of(seconds: u64, secure_input: SecureInput, flags: &[(Switch, bool)]) -> TimedRun {
        TimedRun {
            window: Duration::from_secs(seconds),
            secure_input,
            switches: SwitchFlags(flags.to_vec()),
            store_dir: PathBuf::from("/"),
        }
    }

    fn sources() -> ScriptedRun {
        ScriptedRun::new(vec![
            (
                SourceName::Foreground,
                ScriptedSource::emitting(vec![foreground_now()]),
            ),
            (
                SourceName::Screen,
                ScriptedSource::emitting(vec![frame(), frame()]),
            ),
            (
                SourceName::Input,
                ScriptedSource::emitting(vec![key(), key(), key()]),
            ),
        ])
    }

    fn run_scripted(
        run: &TimedRun,
        scripted: &mut ScriptedRun,
        store: ModelStore,
        clocks: ManualClocks,
    ) -> RunOutcome {
        let stop = AtomicBool::new(false);
        run_timed(
            run,
            scripted,
            store,
            clocks,
            &stop,
            Duration::from_millis(300),
        )
        .unwrap()
    }

    fn line_of<'a>(text: &'a str, start: &str) -> &'a str {
        text.lines()
            .find(|line| line.starts_with(start))
            .unwrap_or_else(|| panic!("no line {start} in\n{text}"))
    }

    #[test]
    fn a_default_timed_run_starts_no_screen_source_and_the_switch_flag_starts_it() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        let outcome = run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            ModelStore::new(),
            clocks.clone(),
        );
        assert_eq!(scripted.asked, [SourceName::Foreground, SourceName::Input]);
        let text = outcome.report.render();
        assert_eq!(
            line_of(&text, "modality=screen_frame"),
            "modality=screen_frame count=0"
        );
        assert_eq!(
            line_of(&text, "modality=keystroke"),
            "modality=keystroke count=3"
        );
        assert_eq!(
            text.lines()
                .filter(|line| line.starts_with("modality="))
                .count(),
            8
        );
        assert_eq!(outcome.status, exit::CLEAN);

        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        let outcome = run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[(Switch::ScreenActivity, true)]),
            &mut scripted,
            ModelStore::new(),
            clocks.clone(),
        );
        assert_eq!(
            scripted.asked,
            [
                SourceName::Foreground,
                SourceName::Screen,
                SourceName::Input
            ]
        );
        let text = outcome.report.render();
        assert_eq!(
            line_of(&text, "modality=screen_frame"),
            "modality=screen_frame count=2"
        );
        assert_eq!(outcome.report.events_received, 6);
        assert_eq!(outcome.report.events_persisted, 6);
    }

    #[test]
    fn a_source_that_fails_to_start_leaves_a_full_length_run_that_exits_3() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        scripted.sources[2].1 =
            ScriptedSource::refusing(CaptureError::PermissionDenied(Modality::Keystroke));
        scripted.in_window = vec![foreground_now(), foreground_now()];
        let outcome = run_scripted(
            &run_of(7, SecureInput::DoNotHold, &[]),
            &mut scripted,
            ModelStore::new(),
            clocks.clone(),
        );
        assert_eq!(scripted.waited, Some(Duration::from_secs(7)));
        assert_eq!(outcome.status, exit::TIMED_RUN_SOURCE_FAILED);
        let text = outcome.report.render();
        assert_eq!(line_of(&text, "failed_sources="), "failed_sources=1");
        assert_eq!(
            line_of(&text, "failed_source="),
            "failed_source=input: the capture permission for keystroke is not granted"
        );
        assert_eq!(
            line_of(&text, "modality=foreground_app"),
            "modality=foreground_app count=3"
        );
    }

    #[test]
    fn a_signalled_timed_run_prints_its_report() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        scripted.signal = true;
        let outcome = run_scripted(
            &run_of(600, SecureInput::DoNotHold, &[]),
            &mut scripted,
            ModelStore::new(),
            clocks.clone(),
        );
        assert_eq!(outcome.status, exit::TIMED_RUN_INTERRUPTED);
        assert!(outcome.loss.is_none());
        let text = outcome.report.render();
        assert_eq!(
            line_of(&text, "modality=keystroke"),
            "modality=keystroke count=3"
        );
        assert_eq!(line_of(&text, "events_persisted="), "events_persisted=4");
    }

    #[test]
    fn the_hold_is_taken_after_the_released_phase_ends() {
        for (seconds, released) in [(2, 1), (5, 2), (9, 4)] {
            let clocks = ManualClocks::at(noon());
            let mut scripted = sources();
            scripted.figures.secure_released_keystrokes = 3;
            scripted.figures.secure_input_held = crate::source::HeldSamples { held: 8, taken: 8 };
            let outcome = run_scripted(
                &run_of(seconds, SecureInput::Hold, &[]),
                &mut scripted,
                ModelStore::new(),
                clocks.clone(),
            );
            assert_eq!(scripted.hold_after, Some(Duration::from_secs(released)));
            let text = outcome.report.render();
            assert_eq!(
                line_of(&text, "secure_released_keystrokes="),
                "secure_released_keystrokes=3"
            );
            assert_eq!(
                line_of(&text, "secure_held_keystrokes="),
                "secure_held_keystrokes=0"
            );
            assert_eq!(
                line_of(&text, "secure_input_held="),
                "secure_input_held=8/8"
            );
        }
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            ModelStore::new(),
            clocks.clone(),
        );
        assert_eq!(scripted.hold_after, None);
    }

    #[test]
    fn a_tap_that_delivers_no_key_fails_the_control() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        scripted.sources[2].1 = ScriptedSource::emitting(Vec::new());
        let outcome = run_scripted(
            &run_of(4, SecureInput::Hold, &[]),
            &mut scripted,
            ModelStore::new(),
            clocks.clone(),
        );
        let text = outcome.report.render();
        assert_eq!(
            line_of(&text, "modality=keystroke"),
            "modality=keystroke count=0"
        );
        assert_eq!(
            line_of(&text, "secure_released_keystrokes="),
            "secure_released_keystrokes=0"
        );
        assert_eq!(
            line_of(&text, "secure_window_keys_typed="),
            "secure_window_keys_typed=no"
        );
    }

    #[test]
    fn a_refused_hold_ends_the_run_with_no_report() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        scripted.refusal = Some(-25291);
        let stop = AtomicBool::new(false);
        let ended = run_timed(
            &run_of(4, SecureInput::Hold, &[]),
            &mut scripted,
            ModelStore::new(),
            clocks,
            &stop,
            Duration::from_millis(300),
        );
        assert!(matches!(
            ended,
            Err(DaemonError::SecureInputRefused(-25291))
        ));
    }

    #[test]
    fn a_batch_refused_at_the_final_settle_gives_exit_1_and_a_loss_line() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Refused));
        let outcome = run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            store,
            clocks.clone(),
        );
        assert_eq!(outcome.status, exit::ERROR);
        let loss = outcome.loss.expect("no loss line");
        assert!(
            loss.starts_with("halvo-capture: lost events=4 first="),
            "{loss}"
        );
        assert!(loss.ends_with(" error=-"), "{loss}");
    }

    #[test]
    fn a_drain_that_still_fails_at_the_deadline_gives_exit_1_with_the_held_events() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Io(std::io::ErrorKind::Other)));
        let outcome = run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            store,
            clocks.clone(),
        );
        assert_eq!(outcome.status, exit::ERROR);
        let loss = outcome.loss.expect("no loss line");
        assert!(loss.starts_with("halvo-capture: lost events=4 "), "{loss}");
        assert!(loss.ends_with(" error=io"), "{loss}");
    }

    #[test]
    fn a_drop_with_no_write_since_gives_exit_1_and_one_that_a_good_write_followed_exit_0() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        scripted.sources[0].1 = ScriptedSource::emitting(Vec::new());
        scripted.sources[2].1 = ScriptedSource::emitting((0..512).map(|_| key()).collect());
        let store = ModelStore::new();
        store.script_writes([WriteScript::Fail(WriteFault::Refused)]);
        let outcome = run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            store,
            clocks.clone(),
        );
        assert_eq!(
            outcome.status,
            exit::ERROR,
            "a drop with no write since: {outcome:?}"
        );
        assert!(outcome.loss.is_some());

        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        scripted.sources[2].1 = ScriptedSource::emitting((0..512).map(|_| key()).collect());
        scripted.in_window = vec![foreground_now()];
        let store = ModelStore::new();
        store.script_writes([WriteScript::Fail(WriteFault::Refused)]);
        let outcome = run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            store,
            clocks.clone(),
        );
        assert_eq!(
            outcome.status,
            exit::CLEAN,
            "a drop that a good write followed"
        );
        assert!(outcome.loss.is_none());
    }

    struct SlowStore;

    impl MaintainStore for SlowStore {
        fn expire_due(
            &mut self,
            _now: std::time::SystemTime,
            _max: usize,
        ) -> Result<Pass, MaintFault> {
            Ok(Pass::default())
        }

        fn evict_to(
            &mut self,
            _bound: super::port::ByteBound,
            _max: usize,
        ) -> Result<Pass, MaintFault> {
            Ok(Pass::default())
        }

        fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
            Ok(StoreUsage::default())
        }
    }

    impl CaptureStore for SlowStore {
        fn write_batch(&mut self, _write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
            std::thread::sleep(Duration::from_secs(3));
            Err(WriteFault::Io(std::io::ErrorKind::TimedOut))
        }

        fn volume_free_bytes(&self) -> std::io::Result<u64> {
            Ok(u64::MAX)
        }
    }

    #[test]
    fn a_last_settle_with_no_answer_gives_exit_1() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        let stop = AtomicBool::new(false);
        let outcome = run_timed(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            SlowStore,
            clocks,
            &stop,
            Duration::from_millis(200),
        )
        .unwrap();
        assert_eq!(outcome.status, exit::ERROR);
        assert!(outcome.loss.is_some());
    }

    #[test]
    fn nine_events_of_the_kinds_with_no_source_give_three_lines_with_count_3() {
        use crate::source::EventSink;
        let clocks = ManualClocks::at(noon());
        let mut harness = super::model_store::Harness::with_clocks(
            ModelStore::new(),
            super::model_store::open_floor(),
            64,
            clocks,
        );
        assert!(matches!(
            harness.ask(owner::Command::ClosePause),
            owner::Answer::Resumed(_)
        ));
        for kind in [
            Modality::SystemAudio,
            Modality::Microphone,
            Modality::Camera,
        ] {
            for _ in 0..3 {
                let old = crate::event::CaptureStreamEvent::new_raw(
                    crate::event::CapturedAt {
                        wall_clock: noon(),
                        sequence: harness.sink.next_sequence(),
                    },
                    kind,
                    String::new(),
                    crate::event::SegmentRef {
                        segment_id: uuid::Uuid::nil(),
                        byte_offset: 0,
                        byte_len: 0,
                    },
                    crate::event::SegmentKeyId(0),
                );
                harness.sink.emit(old).unwrap();
            }
        }
        let owner::Answer::Settled(snapshot) = harness.ask(owner::Command::Settle) else {
            panic!("no settle");
        };
        let report = report_of(
            &snapshot,
            &harness.sink.counts(),
            &SourceCounters::default(),
            Vec::new(),
            SecureInput::DoNotHold,
        );
        let text = report.render();
        for kind in ["system_audio", "microphone", "camera"] {
            assert_eq!(
                line_of(&text, &format!("modality={kind} ")),
                format!("modality={kind} count=3")
            );
        }
        assert_eq!(line_of(&text, "events_received="), "events_received=9");
        let sum: u64 = Modality::ALL
            .into_iter()
            .map(|kind| report.count(kind))
            .sum();
        assert_eq!(sum, report.events_received);
    }

    #[test]
    fn the_sources_of_a_run_leave_out_what_the_switches_turn_off() {
        let with = |flags: &[(Switch, bool)]| sources_for(SwitchFlags(flags.to_vec()).applied());
        let (f, s, i) = (
            SourceName::Foreground,
            SourceName::Screen,
            SourceName::Input,
        );
        assert_eq!(with(&[]), [f, i]);
        assert_eq!(with(&[(Switch::KeyClass, false)]), [f, i]);
        assert_eq!(with(&[(Switch::PointerClass, false)]), [f, i]);
        assert_eq!(
            with(&[(Switch::KeyClass, false), (Switch::PointerClass, false)]),
            [f]
        );
        assert_eq!(with(&[(Switch::ScreenActivity, true)]), [f, s, i]);
        assert_eq!(with(&[(Switch::TitleText, false)]), [f, i]);
    }

    #[test]
    fn a_source_that_does_not_stop_adds_its_line_and_exits_3() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        let timeout = CaptureError::Timeout {
            stage: "screen_stop",
            ms: 6_000,
        };
        scripted.sources[1].1.stop = Err(timeout.clone());
        scripted.sources[2].1.stop = Err(CaptureError::ThreadEnded);
        let outcome = run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[(Switch::ScreenActivity, true)]),
            &mut scripted,
            ModelStore::new(),
            clocks,
        );
        assert_eq!(outcome.status, exit::TIMED_RUN_SOURCE_FAILED);
        let text = outcome.report.render();
        assert_eq!(line_of(&text, "failed_sources="), "failed_sources=1");
        assert_eq!(
            line_of(&text, "failed_source="),
            format!("failed_source=screen: {timeout}")
        );
    }

    #[test]
    fn a_drop_at_the_last_settle_that_a_good_write_follows_gives_exit_1() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        scripted.sources[2].1 = ScriptedSource::emitting((0..512).map(|_| key()).collect());
        scripted.in_window = (0..100).map(|_| key()).collect();
        let store = ModelStore::new();
        store.script_writes([
            WriteScript::Fail(WriteFault::Io(std::io::ErrorKind::Other)),
            WriteScript::Fail(WriteFault::Refused),
        ]);
        let outcome = run_scripted(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            store.clone(),
            clocks,
        );
        assert_eq!(
            store.write_calls().len(),
            3,
            "an Io fault, a refusal and a good write"
        );
        assert_eq!(outcome.report.events_persisted, 101);
        assert_eq!(outcome.status, exit::ERROR, "{outcome:?}");
        let loss = outcome.loss.expect("no loss line");
        assert!(
            loss.starts_with("halvo-capture: lost events=512 "),
            "{loss}"
        );
    }

    struct ParkedStore;

    impl MaintainStore for ParkedStore {
        fn expire_due(
            &mut self,
            _now: std::time::SystemTime,
            _max: usize,
        ) -> Result<Pass, MaintFault> {
            Ok(Pass::default())
        }

        fn evict_to(
            &mut self,
            _bound: super::port::ByteBound,
            _max: usize,
        ) -> Result<Pass, MaintFault> {
            Ok(Pass::default())
        }

        fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
            Ok(StoreUsage::default())
        }
    }

    impl CaptureStore for ParkedStore {
        fn write_batch(&mut self, _write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
            std::thread::sleep(Duration::from_secs(8));
            Err(WriteFault::Io(std::io::ErrorKind::TimedOut))
        }

        fn volume_free_bytes(&self) -> std::io::Result<u64> {
            Ok(u64::MAX)
        }
    }

    #[test]
    fn a_store_write_that_parks_the_owner_in_the_window_ends_the_run_inside_its_bounds() {
        let old = super::model_store::foreground(
            &super::model_store::AT_NOON,
            Duration::from_secs(2 * 86_400),
        );
        let mut scripted = ScriptedRun::new(vec![(
            SourceName::Foreground,
            ScriptedSource::emitting(vec![old, foreground_now(), foreground_now()]),
        )]);
        let stop = AtomicBool::new(false);
        let settle_bound = Duration::from_millis(200);
        let started = std::time::Instant::now();
        let outcome = run_timed(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            ParkedStore,
            ManualClocks::at(noon()),
            &stop,
            settle_bound,
        )
        .unwrap();
        let took = started.elapsed();
        assert_eq!(outcome.status, exit::ERROR, "{outcome:?}");
        assert!(
            took < DRAIN_BOUND + settle_bound + Duration::from_secs(1),
            "the run took {took:?}, more than DRAIN_BOUND {DRAIN_BOUND:?} + settle bound \
             {settle_bound:?} + 1s: the OpenPause call waited for the parked write"
        );
    }
}
