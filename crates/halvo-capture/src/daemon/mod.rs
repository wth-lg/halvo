//! The capture daemon: the store owner, which is the one thread that holds the
//! capture store, and the gate's timed run on that owner.

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod barrier;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod batch;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod board;
pub mod children;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod counters;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod discard_port;
pub mod main_loop;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod maintenance;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
mod mask;
#[cfg(test)]
mod model_store;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod owner;
#[cfg(test)]
mod owner_props;
pub mod permissions;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod port;
#[rustfmt::skip]
#[cfg(feature = "r2")] mod r2;
mod report;
#[cfg(test)]
mod scripted;
pub mod signal;
pub mod sources;
#[cfg(test)]
mod stop_tests;
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub mod supervisor;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use halvo_control::onboarding::Permission;
use halvo_control::paths::{AccountLookup, PathError};
pub use halvo_control::proto::SourceName;

use crate::floor::{Switch, Switches};
use crate::source::{SourceContext, SourceCounters};

pub use report::{FailedSource, HoldLines, Loss, LossCause, RunOutcome, TimedRunReport};

/// The longest timed run: one week.
pub const SECONDS_MAX: u64 = 604_800;

/// How long a timed run waits for the owner's last settle.
pub const SETTLE_BOUND: Duration = Duration::from_secs(5);

/// How long a timed run waits at its deadline, before it pauses the owner,
/// for the owner to receive the events that the sink took inside the window.
pub const DRAIN_BOUND: Duration = Duration::from_secs(2);

/// The bounds of the named waits of a timed run's stop path, which
/// `run_timed` reads from this one value and from nowhere else: the drain
/// wait, and the waits for the owner's answers to the pause, the settle and
/// the Shutdown. The owner's open, the ClosePause before the window, the
/// sources' stop and the join of the owner's thread have no bound of their
/// own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopBounds {
    pub drain: Duration,
    pub pause: Duration,
    pub settle: Duration,
    pub shutdown: Duration,
}

impl StopBounds {
    /// The bounds of a run whose waits for the owner's answers take
    /// `settle_bound` each.
    pub fn of(settle_bound: Duration) -> StopBounds {
        StopBounds {
            drain: DRAIN_BOUND,
            pause: settle_bound,
            settle: settle_bound,
            shutdown: settle_bound,
        }
    }

    /// The whole bound of the named waits: the sum of the four parts.
    pub fn sum(self) -> Duration {
        self.drain + self.pause + self.settle + self.shutdown
    }
}

/// The sum of the named bounds of a timed run's stop path, `StopBounds::sum`.
pub fn stop_bound(settle_bound: Duration) -> Duration {
    StopBounds::of(settle_bound).sum()
}

/// The text that a refused command line prints after its reason.
pub const USAGE: &str = "usage: halvo-capture --version
       halvo-capture --probe-permissions [PERMISSION]...
       halvo-capture --request-permission PERMISSION
       halvo-capture --timed-run SECONDS --store-dir DIR [--hold-secure-input] [--switch NAME=on|off]...
       halvo-capture --verify-store --store-dir DIR --marker MARKER
       halvo-capture --verify-deletion --store-dir DIR
       halvo-capture --delete-all --store-dir DIR";

/// What a command line asks for. The gate's modes exist only in builds with
/// debug assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Version,
    /// `--probe-permissions [P...]`: one line for each permission named, a
    /// check of no other (contracts.md C3.6).
    ProbePermissions(Vec<Permission>),
    /// `--request-permission P` (contracts.md C3.6).
    RequestPermission(Permission),
    /// `--self-check run-loop`: the wait of the timed run with no source,
    /// while a second thread queues a block on the main run loop.
    #[cfg(debug_assertions)]
    SelfCheckRunLoop,
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
    /// The scratch delete (contracts.md C4 item 9) of `store_dir`, the path as
    /// the command line gave it, so that the store's delete tail meets a link
    /// at its last part in any spelling and refuses it. Before the tail's call
    /// the parse refuses, with exit 64, a directory that holds a data
    /// directory's layout, an entry at the name store beside one at the name
    /// roots, read through DIR as resolved and without following a link at
    /// either name, and opens, changes and removes nothing in it. This look is
    /// the one look before the tail's call: every gate trap calls this mode,
    /// and a wrong argument must not cost a person's store its root (ruling
    /// DM-22). A look that cannot read either name, for a cause other than its
    /// absence or a DIR that is no folder (which the tail refuses as a link
    /// or a file), refuses DIR too, with a line of its own (ruling DM-22
    /// Addition 1).
    #[cfg(debug_assertions)]
    DeleteAll {
        store_dir: PathBuf,
    },
}

impl Mode {
    /// Whether the mode reads the signal flag that [`signal::prepare`]
    /// installs: the timed run alone, which stops its sources, settles and
    /// reports on it.
    #[cfg(debug_assertions)]
    pub fn reads_the_signal_flag(&self) -> bool {
        matches!(self, Mode::TimedRun(_))
    }

    /// Whether the mode reads the signal flag: no mode of a release build does.
    #[cfg(not(debug_assertions))]
    pub fn reads_the_signal_flag(&self) -> bool {
        false
    }
}

/// Installs the signal flag for a mode that reads it and nothing for any
/// other mode, which SIGINT and SIGTERM then end by the system's default
/// action (ruling DM-12 P3).
pub fn prepare_signals(mode: &Mode) -> std::io::Result<()> {
    if mode.reads_the_signal_flag() {
        signal::prepare()
    } else {
        Ok(())
    }
}

/// A refused command line, with its reason; the binary prints it and
/// [`USAGE`] and exits 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("halvo-capture: {0}\n{USAGE}")]
pub struct Usage(pub &'static str);

/// Why a command line is not run: a refused line, which exits 64, or an
/// account that gives no data directory to check a store directory against,
/// which is no fault of the line and exits 1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error(transparent)]
    Usage(#[from] Usage),
    #[error(
        "halvo-capture: the account gives no data directory to check the store directory against: {0}"
    )]
    NoDataDir(PathError),
    /// A store directory that holds a data directory's layout, by its text as
    /// given; the scratch delete refuses it with exit 64.
    #[error(
        "halvo-capture: {0} holds store/ beside roots/, the layout of a data directory; the scratch delete takes a scratch store directory alone, and nothing in this one was opened or removed"
    )]
    DataDirLayout(String),
    /// A store directory in which the names store and roots cannot be read,
    /// by its text as given; the scratch delete refuses it with exit 64.
    #[error(
        "halvo-capture: the names store and roots in {0} cannot be read, so the scratch delete cannot tell whether it holds a data directory's layout; nothing in it was opened or removed"
    )]
    DataDirUnread(String),
}

/// The binary's answer to a command line that is not run: the refusal's text
/// on `err`, and exit 64 for a refused line or a data directory's layout, or
/// exit 1 for an account that gives no data directory.
pub fn usage_answer<R: Clone + Into<Refusal>>(refused: &R, err: &mut dyn std::io::Write) -> i32 {
    let refusal: Refusal = refused.clone().into();
    let _ = writeln!(err, "{refusal}");
    match refusal {
        Refusal::Usage(_) | Refusal::DataDirLayout(_) | Refusal::DataDirUnread(_) => {
            halvo_control::exit::USAGE
        }
        Refusal::NoDataDir(_) => halvo_control::exit::ERROR,
    }
}

/// The binary's answer to a timed run's outcome: the report on `out`, the
/// loss line on `err`, and the run's status; a run that let a source go ends
/// here by [`finish_run`] with that status. A report that cannot be written,
/// as into a pipe whose reader is gone, gives exit 1 with a line, and the
/// loss line still follows it. A run whose hold the system refused answers
/// the refusal's line as the binary's error arm writes it, no report and
/// status 1, and it too ends by [`finish_run`] when it let a source go.
pub fn outcome_answer(
    outcome: &RunOutcome,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    answer_with(outcome, out, err, process_ending())
}

/// [`outcome_answer`] with the ending `ending`.
fn answer_with(
    outcome: &RunOutcome,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
    ending: &dyn main_loop::EndCall,
) -> i32 {
    if let Some(refusal) = &outcome.refusal {
        let _ = writeln!(err, "halvo-capture: {refusal}");
        let _ = err.flush();
        finish_run(outcome, halvo_control::exit::ERROR, ending, err);
        return halvo_control::exit::ERROR;
    }
    let printed = out
        .write_all(outcome.report.render().as_bytes())
        .and_then(|()| out.flush());
    if let Err(error) = &printed {
        let _ = writeln!(err, "halvo-capture: the report was not written: {error}");
    }
    if let Some(loss) = &outcome.loss {
        let _ = writeln!(err, "{loss}");
    }
    let status = match printed {
        Ok(()) => outcome.status,
        Err(_) => halvo_control::exit::ERROR,
    };
    finish_run(outcome, status, ending, err);
    status
}

/// The ending that [`outcome_answer`] makes for a run that let a source go:
/// the product's in a build of the crate, and in the crate's own tests one
/// that panics, so that no test of this crate ends its own process.
fn process_ending() -> &'static dyn main_loop::EndCall {
    #[cfg(not(test))]
    {
        &main_loop::ExitNow
    }
    #[cfg(test)]
    {
        &main_loop::tests::PanicInTests
    }
}

/// What the store's delete tail answered for a scratch store directory, in
/// the scratch layout, as the binary hands it to [`scratch_delete`]. After a
/// delete that was stopped between its root step and its file step, or when
/// the root file was removed by other hands, the next delete says that it is
/// not finished although nothing stays after it, because it did not itself
/// destroy the root; the delete after that says finished (ruling DM-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailAnswer {
    /// The tail answered Ok.
    Finished(TailReport),
    /// The tail answered that the delete is not finished.
    NotFinished(TailReport),
    /// Another process holds the store.
    Held,
    /// Any other error of the tail, by its text, which holds no path.
    Failed(String),
}

/// The tail's report, field by field: the store's own root, the names of the
/// root files and partial root files that stay in the folder roots, the
/// entries that stay there at any other name counted by kind and never
/// named, and what stays of the store's files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TailReport {
    pub keyed_segments: Option<u64>,
    pub root: TailRoot,
    pub root_files_left: Vec<String>,
    pub others_left: EntriesLeft,
    pub roots_listed: bool,
    pub roots_at_their_path: bool,
    pub key_file_gone: bool,
    pub database_left: bool,
    pub files_left: u64,
    pub not_followed: u64,
}

/// What the tail says of the store's root: destroyed by this call, left
/// (with its id in hex when the call knows it), or named by nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TailRoot {
    Destroyed,
    Left(Option<String>),
    #[default]
    Unnamed,
}

/// The entries that stay in the folder roots at a name that is no root name
/// and no partial name, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntriesLeft {
    pub root_shaped: u64,
    pub directories: u64,
    pub other: u64,
}

impl TailReport {
    /// The report of a tail that refused the store directory itself, a link
    /// or a file in any spelling: not followed, the root unread, the folder
    /// roots not listed and nothing removed.
    fn refused_the_store_dir(&self) -> bool {
        self.not_followed == 1
            && self.root == TailRoot::Left(None)
            && !self.roots_listed
            && self.root_files_left.is_empty()
            && self.others_left == EntriesLeft::default()
            && self.keyed_segments.is_none()
            && !self.key_file_gone
            && self.database_left
            && self.files_left == 1
    }

    /// The report of a tail that could not open the store directory, or that
    /// answered through its not-opened report for another cause: the root left
    /// with no id, and store.key, the database and one file read as not removed
    /// because the tail never looked. A removal that left exactly those three
    /// and read no root id reads the same.
    fn not_opened(&self) -> bool {
        self.root == TailRoot::Left(None)
            && !self.key_file_gone
            && self.database_left
            && self.files_left == 1
            && self.keyed_segments.is_none()
    }

    /// The scratch form of the `deleted` line of contracts.md C2.6 and C3.3:
    /// the seven keys whose values the tail's report holds, and not
    /// `entries_removed`, `entries_left`, `unknown_entries`, `intent` or
    /// `store_id`, which belong to a data directory's registry, its intent and
    /// its fresh store, of which a scratch store directory has none.
    fn deleted_line(&self) -> String {
        let count = |n: Option<u64>| n.map_or_else(|| "unknown".to_owned(), |n| n.to_string());
        let (destroyed, id) = match &self.root {
            TailRoot::Destroyed => ("yes", "-"),
            TailRoot::Left(id) => ("no", id.as_deref().unwrap_or("-")),
            TailRoot::Unnamed => ("unknown", "-"),
        };
        format!(
            "deleted segments_shredded={} keys_destroyed={} root_destroyed={destroyed} root_id={id} files_left={} database_left={} not_followed={}\n",
            count(self.keyed_segments),
            count(self.keyed_segments.map(|n| n + 1)),
            self.files_left,
            if self.database_left { "yes" } else { "no" },
            self.not_followed,
        )
    }

    /// What stays, for standard error: the root files and partial root files
    /// by their names in the folder roots, every other entry there by kind.
    fn what_stays(&self) -> String {
        let mut stays = Vec::new();
        if matches!(self.root, TailRoot::Left(_)) {
            stays.push("this call did not destroy the store's root".to_owned());
        }
        for name in &self.root_files_left {
            stays.push(format!("the root file roots/{name} stays"));
        }
        let others = self.others_left;
        for (n, what) in [
            (others.root_shaped, "files that can hold a root"),
            (others.directories, "folders"),
            (others.other, "other entries"),
        ] {
            if n > 0 {
                stays.push(format!(
                    "{n} {what} stay in the folder roots at other names"
                ));
            }
        }
        if !self.roots_listed {
            stays.push("the folder roots could not be listed".to_owned());
        }
        if !self.roots_at_their_path {
            stays.push("the folder roots no longer stands at its path".to_owned());
        }
        if !self.key_file_gone {
            stays.push("store.key stays".to_owned());
        }
        if self.database_left {
            stays.push("a database file stays".to_owned());
        }
        if self.files_left > 0 {
            stays.push(format!("{} other files of the store stay", self.files_left));
        }
        if self.not_followed > 0 {
            stays.push(format!(
                "{} names were a link or an entry of another kind and were not followed",
                self.not_followed
            ));
        }
        stays.join("; ")
    }
}

fn has_a_part_that_windows_trims(path: &Path) -> bool {
    path.components().any(|part| match part {
        std::path::Component::Normal(name) => {
            let name = name.to_string_lossy();
            name.ends_with('.') || name.ends_with(' ')
        }
        _ => false,
    })
}

/// The scratch delete of `--delete-all --store-dir DIR` (contracts.md C4
/// item 9): one call of `tail` on `store_dir`, which the binary makes the
/// store's delete tail in the scratch layout. It prints on `out`
/// `store_dir=<the path as given>` and then `no_store` when the tail answered
/// Ok, nothing named a root and no name was a link or an entry of another
/// kind, or the `deleted` line of
/// [`TailReport`]'s seven keys; on `err` the cause of an answer that is not
/// Ok. It returns 0 only when the tail answered Ok, 1 when the delete is not
/// finished or failed and 75 when another process holds the store; a line that
/// cannot be written gives 1. After a run that destroyed its own root while
/// its store.key stays, as `--verify-store` and the timed run do, the tail
/// reads that root left, because no tail answers destroyed for a root that
/// the call did not find and remove (ruling ST-46 K1): the first delete
/// removes the store's files and exits 1 with `root_destroyed=no`, and the
/// next prints `no_store` and exits 0.
pub fn scratch_delete(
    store_dir: &Path,
    tail: impl FnOnce(&Path) -> TailAnswer,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let answer = tail(store_dir);
    let shown = halvo_control::paths::one_line(&store_dir.to_string_lossy());
    let mut text = format!("store_dir={shown}\n");
    let (cause, status) = match &answer {
        TailAnswer::Finished(report) => {
            if report.root == TailRoot::Unnamed && report.not_followed == 0 {
                text.push_str("no_store\n");
            } else {
                text.push_str(&report.deleted_line());
            }
            (None, halvo_control::exit::CLEAN)
        }
        TailAnswer::NotFinished(report) => {
            text.push_str(&report.deleted_line());
            let cause = if report.refused_the_store_dir()
                && !(cfg!(windows) && has_a_part_that_windows_trims(store_dir))
            {
                format!("halvo-capture: {shown} is a link or a file, not a folder; nothing was removed; give the path of the folder itself")
            } else if report.not_opened() {
                "halvo-capture: the delete is not finished: the store directory could not be opened, so this call cannot say what stays in it".to_owned()
            } else {
                match report.what_stays() {
                    stays if stays.is_empty() => {
                        "halvo-capture: the delete is not finished".to_owned()
                    }
                    stays => format!("halvo-capture: the delete is not finished: {stays}"),
                }
            };
            (Some(cause), halvo_control::exit::ERROR)
        }
        TailAnswer::Held => (
            Some(
                "halvo-capture: another process holds the store; this request removed nothing"
                    .to_owned(),
            ),
            halvo_control::exit::BUSY,
        ),
        TailAnswer::Failed(error) => (
            Some(format!("halvo-capture: {error}")),
            halvo_control::exit::ERROR,
        ),
    };
    let printed = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    if let Err(error) = &printed {
        let _ = writeln!(err, "halvo-capture: the report was not written: {error}");
    }
    if let Some(cause) = cause {
        let _ = writeln!(err, "{cause}");
    }
    match printed {
        Ok(()) => status,
        Err(_) => halvo_control::exit::ERROR,
    }
}

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
) -> Result<Mode, Refusal> {
    let args = args
        .into_iter()
        .map(OsString::into_string)
        .collect::<Result<Vec<String>, OsString>>()
        .map_err(|_| Usage("an argument is not valid UTF-8"))?;
    #[cfg(not(debug_assertions))]
    let _ = account;
    match args.first().map(String::as_str) {
        Some("--version") if args.len() == 1 => Ok(Mode::Version),
        Some("--probe-permissions") => Ok(Mode::ProbePermissions(permissions_of(&args[1..])?)),
        Some("--request-permission") => match permissions_of(&args[1..])?[..] {
            [permission] => Ok(Mode::RequestPermission(permission)),
            _ => Err(Usage("--request-permission takes one PERMISSION").into()),
        },
        #[cfg(debug_assertions)]
        Some("--self-check") if args[1..] == ["run-loop"] => Ok(Mode::SelfCheckRunLoop),
        #[cfg(debug_assertions)]
        Some("--timed-run") => gate::timed_run(&args[1..], account),
        #[cfg(debug_assertions)]
        Some("--verify-store") => {
            let flags = gate::Flags::read(&args[1..], &["--store-dir", "--marker"])?;
            let store_dir = gate::store_dir(flags.store_dir, account)?.without_store_key()?;
            let marker = flags.marker.ok_or(Usage("--verify-store needs --marker"))?;
            if !gate::is_marker(&marker) {
                return Err(
                    Usage("MARKER is not 8 to 64 of A-Z, a-z, 0-9, '.', '_' and '-'").into(),
                );
            }
            Ok(Mode::VerifyStore { store_dir, marker })
        }
        #[cfg(debug_assertions)]
        Some("--verify-deletion") => {
            let flags = gate::Flags::read(&args[1..], &["--store-dir"])?;
            let store_dir = gate::store_dir(flags.store_dir, account)?.without_store_key()?;
            Ok(Mode::VerifyDeletion { store_dir })
        }
        #[cfg(debug_assertions)]
        Some("--delete-all") => {
            let flags = gate::Flags::read(&args[1..], &["--store-dir"])?;
            let store_dir =
                gate::store_dir(flags.store_dir, account)?.without_a_data_directory_layout()?;
            Ok(Mode::DeleteAll { store_dir })
        }
        _ => Err(Usage("no mode of this build").into()),
    }
}

fn permissions_of(names: &[String]) -> Result<Vec<Permission>, Usage> {
    names
        .iter()
        .map(|name| {
            name.parse::<Permission>()
                .map_err(|_| Usage("no permission has that name"))
        })
        .collect()
}

#[cfg(debug_assertions)]
mod gate {
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::time::Duration;

    use halvo_control::paths::{AccountLookup, DataDir, ScratchDir};

    use super::{Mode, Refusal, SecureInput, SwitchFlags, TimedRun, Usage, SECONDS_MAX};
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

    pub(super) fn timed_run(args: &[String], account: &dyn AccountLookup) -> Result<Mode, Refusal> {
        let (seconds, flags) = args
            .split_first()
            .ok_or(Usage("--timed-run needs SECONDS"))?;
        let flags = Flags::read(flags, &["--store-dir", "--hold-secure-input", "--switch"])?;
        let seconds = seconds_of(seconds)?;
        let secure_input = if flags.hold {
            if cfg!(target_os = "windows") {
                return Err(Usage("--hold-secure-input is a flag of macOS").into());
            }
            if seconds < 2 {
                return Err(Usage("SECONDS is at least 2 with --hold-secure-input").into());
            }
            SecureInput::Hold
        } else {
            SecureInput::DoNotHold
        };
        let store_dir = store_dir(flags.store_dir, account)?.without_store_key()?;
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

    pub(super) struct StoreDir {
        given: PathBuf,
        resolved: PathBuf,
    }

    impl StoreDir {
        pub(super) fn without_a_data_directory_layout(self) -> Result<PathBuf, Refusal> {
            let shown = || halvo_control::paths::one_line(&self.given.to_string_lossy());
            let mut stands = 0;
            for name in ["store", "roots"] {
                match self.resolved.join(name).symlink_metadata() {
                    Ok(_) => stands += 1,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                        ) => {}
                    Err(_) => return Err(Refusal::DataDirUnread(shown())),
                }
            }
            if stands == 2 {
                return Err(Refusal::DataDirLayout(shown()));
            }
            Ok(self.given)
        }

        pub(super) fn without_store_key(self) -> Result<PathBuf, Refusal> {
            match self.resolved.join("store.key").symlink_metadata() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(self.resolved)
                }
                Ok(_) => Err(Usage("DIR holds store.key").into()),
                Err(_) => Err(Usage(
                    "DIR/store.key cannot be read, as in a DIR that this user cannot search, so DIR may hold store.key",
                )
                .into()),
            }
        }
    }

    pub(super) fn store_dir(
        dir: Option<String>,
        account: &dyn AccountLookup,
    ) -> Result<StoreDir, Refusal> {
        let dir = dir.ok_or(Usage("--store-dir DIR is required"))?;
        let given = PathBuf::from(dir);
        if !given.is_absolute()
            || given
                .components()
                .any(|part| part == std::path::Component::ParentDir)
        {
            return Err(Usage("DIR is no absolute directory clear of the data directory").into());
        }
        DataDir::for_account(account).map_err(Refusal::NoDataDir)?;
        let scratch = ScratchDir::new(given.clone(), account)
            .map_err(|_| Usage("DIR is no absolute directory clear of the data directory"))?;
        Ok(StoreDir {
            given,
            resolved: scratch.path().to_path_buf(),
        })
    }

    pub(super) fn is_marker(marker: &str) -> bool {
        (8..=64).contains(&marker.len())
            && marker
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    }
}

/// The `failed_source=` entry of a source's answer to its stop at the run's
/// end: none for `Ok`, an entry for every other answer (contracts.md C4 item
/// 7). `ThreadEnded` is a stop for the stop sequence and the end routine,
/// and it is named here: the source had died before its stop (ruling DM-25
/// N5).
pub fn stop_failure(
    name: SourceName,
    answer: Result<(), crate::source::CaptureError>,
) -> Option<FailedSource> {
    match answer {
        Ok(()) => None,
        Err(error) => Some(FailedSource {
            name: name.as_str(),
            error: error.to_string(),
        }),
    }
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

    /// Waits as [`RunSources::wait`] does while the supervisor reads `owner`.
    #[cfg(any(test, all(target_os = "macos", debug_assertions)))]
    fn supervise(
        &mut self,
        window: Duration,
        stop: &std::sync::atomic::AtomicBool,
        owner: &dyn supervisor::OwnerLink,
    ) -> bool {
        let _ = owner;
        self.wait(window, stop)
    }

    /// Whether a permission granted after the start asks the run to end with
    /// status 4.
    fn restart_asked(&self) -> bool {
        false
    }

    /// The clock and the waits of the stop path, when the sources keep their
    /// own: the supervisor's main loop. `None` gives the real clock.
    #[cfg(any(test, all(target_os = "macos", debug_assertions)))]
    fn waits(&mut self) -> Option<&mut dyn Waits> {
        None
    }

    /// Arms the watchdog of the run, if the sources keep one, to end the
    /// process with `status` once SHUTDOWN_BUDGET_MS has passed; when it is
    /// armed already, the earlier deadline stays and `status` replaces the
    /// armed one.
    fn arm_watchdog(&mut self, status: i32) {
        let _ = status;
    }

    /// Takes back the arm of the run's watchdog, if the sources keep one.
    fn disarm_watchdog(&mut self) {}

    /// The sources whose stop answered neither Ok nor ThreadEnded and that
    /// are let go without a wait.
    fn let_go(&self) -> Vec<SourceName> {
        Vec::new()
    }
}

/// The end of a timed run in which a source did not stop: the line that says
/// so on `err`, then the ending call with `status`, so that the answer of a
/// run that let a source go does not return from `main` (ruling PL-3 part
/// (4)). A run whose sources all stopped ends nothing here. A run that
/// returns Err never reaches it: the binary's error arm returns from `main`
/// with any source let go still held, and the watchdog, armed with 1 for
/// such a source, ends the process only should that return not come first.
pub fn finish_run(
    outcome: &RunOutcome,
    status: i32,
    ending: &dyn main_loop::EndCall,
    err: &mut dyn std::io::Write,
) {
    if outcome.let_go.is_empty() {
        return;
    }
    let _ = writeln!(
        err,
        "halvo-capture: the source{} {} did not stop; the process ends without waiting for it",
        if outcome.let_go.len() == 1 { "" } else { "s" },
        outcome.let_go.join(", ")
    );
    let _ = err.flush();
    ending.end(status);
}

/// The clock and the waits of a timed run's stop path.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub trait Waits {
    /// The time since a fixed start.
    fn clock(&self) -> Duration;

    /// The answer of `pending` if it comes within `bound`.
    fn wait_answer(&mut self, pending: &owner::Pending, bound: Duration) -> Option<owner::Answer>;

    /// Lets `wait` pass.
    fn pause_for(&mut self, wait: Duration);
}

/// The process's own clock and blocking waits.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub struct RealWaits;

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
impl Waits for RealWaits {
    fn clock(&self) -> Duration {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed()
    }

    fn wait_answer(&mut self, pending: &owner::Pending, bound: Duration) -> Option<owner::Answer> {
        pending.wait_for(bound)
    }

    fn pause_for(&mut self, wait: Duration) {
        std::thread::sleep(wait);
    }
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn waits_of<'a, P: RunSources>(sources: &'a mut P, real: &'a mut RealWaits) -> &'a mut dyn Waits {
    match sources.waits() {
        Some(waits) => waits,
        None => real,
    }
}

/// The margin by which a timed run's stop path ends before its watchdog's
/// deadline, SHUTDOWN_BUDGET_MS (18,000 ms) after the end of the window: the
/// settle and the Shutdown are cut to 17,000 ms, and the margin is the time
/// that `run_timed` has to join the owner, build its answer and take the arm
/// back, or carry the run's own status in it, before the watchdog's call
/// (ruling DM-25 N2). The deadline stays at 18,000 ms, under STOP_WAIT_MS;
/// the gate's kill of a timed child comes at its seconds plus
/// child_deadline_extra_s (25 s, docs/baseline/r1-gate-bounds.txt), counted
/// from the child's start, so the margin moves no end past it.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub const STOP_PATH_MARGIN_MS: u64 = 1_000;

/// What is left of the stop path's budget, SHUTDOWN_BUDGET_MS less
/// [`STOP_PATH_MARGIN_MS`] from `start`.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn left_of(start: Duration, waits: &dyn Waits) -> Duration {
    (start + Duration::from_millis(supervisor::SHUTDOWN_BUDGET_MS - STOP_PATH_MARGIN_MS))
        .saturating_sub(waits.clock())
}

impl TimedRun {
    /// The run kind of this timed run for the source set: whether its flags
    /// set `key_class` on.
    pub fn kind(&self) -> sources::RunKind {
        sources::RunKind::Timed {
            key_class_flag: self
                .switches
                .set_by_flags()
                .contains(&(Switch::KeyClass, true)),
        }
    }
}

/// Runs one timed run on the owner: the owner over `store` resumes, the
/// sources start and run for the window, the owner receives what the sink took
/// in the window (at most [`DRAIN_BOUND`]), the owner pauses, the sources stop
/// and the owner settles.
/// The named waits of the stop path take their bounds from
/// `StopBounds::of(settle_bound)`; the settle and the Shutdown, which follow
/// the sources' stops, are cut to what is left of SHUTDOWN_BUDGET_MS counted
/// from the end of the window, so that the waits and the stops together end
/// inside it (the drain wait and the pause begin with 16 s or more left),
/// [`STOP_PATH_MARGIN_MS`] before the watchdog's deadline. An owner that
/// gives no answer is not joined.
///
/// The watchdog is armed at the end of the window with 4 for a grant
/// restart, 2 after a signal, else 3. When the run returns with no source
/// let go, the arm is taken back, so that no ending call follows the run's
/// own answer (ruling DM-25 N2). When a source is let go, the arm stays at
/// its deadline and carries the run's own status (1 for an error): between
/// the return and `finish_run`'s line and ending call, that arm alone guards
/// the run, through the drop of the sources, the report and the loss line.
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
    let answer = run_timed_steps(run, sources, store, clocks, stop, settle_bound);
    if sources.let_go().is_empty() {
        sources.disarm_watchdog();
    } else {
        sources.arm_watchdog(match &answer {
            Ok(outcome) => outcome.status,
            Err(_) => halvo_control::exit::ERROR,
        });
    }
    answer
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn run_timed_steps<P, S, C>(
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
    use halvo_control::proto::Platform;

    use crate::floor::exclude::{FloorHandle, NoLineage};
    use owner::{Answer, Command, OwnerParts};

    let bounds = StopBounds::of(settle_bound);
    let switches = run.switches.applied();
    let floor = FloorHandle::new(None, Arc::new(NoLineage), switches);
    let (handle, writer) = crate::sink::channel(owner::QUEUE_SLOTS);
    let owner = supervisor::Watched::new(
        owner::spawn(OwnerParts {
            writer,
            store,
            clocks,
            floor: floor.clone(),
            cap: StorageCap::DEFAULT,
            shred_chunk: maintenance::SHRED_CHUNK,
        })
        .map_err(|error| DaemonError::Store(error.to_string()))?,
        Box::new(std::io::stderr()),
    );
    if !matches!(
        owner.handle.call(Command::ClosePause),
        Ok(Answer::Resumed(_))
    ) {
        return Err(DaemonError::DrainPanicked);
    }
    let ctx = SourceContext::with_floor(handle.clone(), floor);
    let wanted = sources::starting(Platform::Macos, run.kind(), switches);
    let mut failed = sources.start(&ctx, &wanted, run.hold_after());
    drop(ctx);
    let signalled = sources.supervise(run.window, stop, &owner);
    sources.arm_watchdog(if sources.restart_asked() {
        exit::RESTART
    } else if signalled || stop.load(std::sync::atomic::Ordering::SeqCst) {
        exit::TIMED_RUN_INTERRUPTED
    } else {
        exit::TIMED_RUN_SOURCE_FAILED
    });
    let mut real = RealWaits;
    let started = waits_of(sources, &mut real).clock();
    let counts = handle.counts();
    let emitted: u64 = crate::event::Modality::ALL
        .into_iter()
        .map(|kind| counts.of(kind).accepted)
        .sum();
    let waits = waits_of(sources, &mut real);
    let drained_by = waits.clock() + bounds.drain;
    while owner.snapshot().events_received() < emitted
        && waits.clock() < drained_by
        && supervisor::OwnerLink::is_running(&owner)
    {
        waits.pause_for(Duration::from_millis(5));
    }
    let before = owner.snapshot();
    let paused = answer_within(waits, &owner.handle, Command::OpenPause, bounds.pause);
    failed.extend(sources.stop());
    let sink_counts = handle.counts();
    drop(handle);
    let last = match paused {
        Some(Answer::Paused(_)) => {
            let waits = waits_of(sources, &mut real);
            let bound = bounds.settle.min(left_of(started, waits));
            settle_within(waits, &owner.handle, bound).ok_or(LossCause::SettleUnanswered)
        }
        _ => Err(LossCause::PauseUnanswered),
    };
    let figures = sources.figures();
    if let Some(refusal) = sources.hold_refusal() {
        let settled = match last {
            Ok(snapshot) => snapshot,
            Err(_) => owner.snapshot(),
        };
        let reports: Vec<owner::Report> = owner.reports.try_iter().collect();
        let loss = if last.is_err() && !supervisor::OwnerLink::is_running(&owner) {
            Some(owner_ended(&settled, &sink_counts))
        } else {
            stop_loss(last.map(|_| ()), &settled, &before, &sink_counts, &reports)
        };
        let refusal = match loss.clone() {
            Some(loss) => DaemonError::RefusedWithLoss {
                refusal: Box::new(refusal),
                loss,
            },
            None => refusal,
        };
        return Ok(RunOutcome {
            report: report_of(&settled, &sink_counts, &figures, failed, run.secure_input),
            status: exit::ERROR,
            loss,
            let_go: let_go_of(sources),
            refusal: Some(refusal.to_string()),
        });
    }
    let last = match last {
        Ok(_) => {
            let waits = waits_of(sources, &mut real);
            let bound = bounds.shutdown.min(left_of(started, waits));
            match answer_within(waits, &owner.handle, Command::Shutdown, bound) {
                Some(Answer::ShutDown(snapshot)) => Ok(snapshot),
                _ => Err(LossCause::ShutdownUnanswered),
            }
        }
        Err(cause) => Err(cause),
    };
    let reports: Vec<owner::Report> = owner.reports.try_iter().collect();
    let settled = match last {
        Ok(snapshot) => snapshot,
        Err(_) => owner.snapshot(),
    };
    let owner_alive = supervisor::OwnerLink::is_running(&owner);
    if last.is_ok() {
        owner.join().map_err(|_| DaemonError::DrainPanicked)?;
    }
    let loss = if last.is_err() && !owner_alive {
        Some(owner_ended(&settled, &sink_counts))
    } else {
        stop_loss(last.map(|_| ()), &settled, &before, &sink_counts, &reports)
    };
    let failing = match loss.as_ref().map(|loss| loss.cause) {
        Some(Some(_)) => true,
        Some(None) | None => false,
    };
    let signalled = signalled || stop.load(std::sync::atomic::Ordering::SeqCst);
    let status = if failing {
        exit::ERROR
    } else if sources.restart_asked() {
        exit::RESTART
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
        let_go: let_go_of(sources),
        refusal: None,
    })
}

/// The names of the sources that `sources` let go.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn let_go_of<P: RunSources>(sources: &P) -> Vec<&'static str> {
    sources
        .let_go()
        .into_iter()
        .map(SourceName::as_str)
        .collect()
}

/// The loss line of a run whose owner thread ended before it answered the
/// stop: the events that the sink accepted and no write stored (contracts.md
/// C3.2).
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn owner_ended(settled: &counters::Snapshot, sink: &crate::sink::SinkCounts) -> Loss {
    let accepted: u64 = crate::event::Modality::ALL
        .into_iter()
        .map(|kind| sink.of(kind).accepted)
        .sum();
    Loss {
        cause: Some(LossCause::OwnerEnded),
        line: format!(
            "halvo-capture: owner ended; events accepted and not persisted={}",
            accepted.saturating_sub(settled.events_persisted)
        ),
    }
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn answer_within(
    waits: &mut dyn Waits,
    handle: &owner::OwnerHandle,
    command: owner::Command,
    bound: Duration,
) -> Option<owner::Answer> {
    let pending = handle.ask(command).ok()?;
    waits.wait_answer(&pending, bound)
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
/// Asks Settle until nothing is pending or the bound runs out, and answers the
/// last snapshot that an ask got: a later ask that is not answered in time
/// leaves the one before it, and only a settle whose every ask went
/// unanswered gives `None` (ruling DM-14).
fn settle_within(
    waits: &mut dyn Waits,
    handle: &owner::OwnerHandle,
    bound: Duration,
) -> Option<counters::Snapshot> {
    let started = waits.clock();
    let mut last = None;
    loop {
        let Ok(pending) = handle.ask(owner::Command::Settle) else {
            return last;
        };
        let left = bound.saturating_sub(waits.clock().saturating_sub(started));
        let Some(owner::Answer::Settled(snapshot)) = waits.wait_answer(&pending, left) else {
            return last;
        };
        let left = bound.saturating_sub(waits.clock().saturating_sub(started));
        if snapshot.events_pending == 0 || left < 2 * owner::POLL {
            return Some(snapshot);
        }
        last = Some(snapshot);
        waits.pause_for(owner::POLL);
    }
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
/// For a stop that got no answer: an upper bound of what the life did not
/// store, from a snapshot read after the last wait and the sink's counts read
/// after the sources stopped: lost, pending, and accepted less received over
/// the kinds of `Modality::ALL`.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn unstored_bound(settled: &counters::Snapshot, sink: &crate::sink::SinkCounts) -> u64 {
    use crate::event::Modality;
    let accepted: u64 = Modality::ALL
        .into_iter()
        .map(|kind| sink.of(kind).accepted)
        .sum();
    let received: u64 = Modality::ALL
        .into_iter()
        .map(|kind| settled.received(kind))
        .sum();
    settled.events_lost + settled.events_pending + accepted.saturating_sub(received)
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn stop_loss(
    answered: Result<(), LossCause>,
    settled: &counters::Snapshot,
    before: &counters::Snapshot,
    sink: &crate::sink::SinkCounts,
    reports: &[owner::Report],
) -> Option<Loss> {
    let count = match answered {
        Ok(()) => settled.events_lost + settled.events_pending,
        Err(_) => unstored_bound(settled, sink),
    };
    let cause = match answered {
        Err(cause) => Some(cause),
        Ok(()) if settled.events_pending > 0 => Some(LossCause::Pending),
        Ok(()) if settled.lost_since_acknowledged > 0 => Some(LossCause::LostSinceAcknowledged),
        Ok(()) if settled.events_lost > before.events_lost => Some(LossCause::LostDuringStop),
        Ok(()) => None,
    };
    (cause.is_some() || count > 0).then(|| Loss {
        cause,
        line: loss_line(settled, count, reports),
    })
}

#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
fn loss_line(settled: &counters::Snapshot, count: u64, reports: &[owner::Report]) -> String {
    let kind = reports
        .iter()
        .rev()
        .find_map(|report| match report {
            owner::Report::DrainFailing { cause, .. } => Some(Some(cause.as_str())),
            owner::Report::DrainHealthy => Some(None),
            _ => None,
        })
        .flatten()
        .unwrap_or("-");
    format!(
        "halvo-capture: lost events={count} first={} last={} error={kind}",
        owner::capture_second(settled.lost_first_at),
        owner::capture_second(settled.lost_last_at),
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

/// The report of a timed run from what the Board holds: its owner snapshot,
/// the counters of the process and the failed sources.
#[cfg(any(test, all(target_os = "macos", debug_assertions)))]
pub fn report_of_board(
    board: &board::Board,
    sink: &crate::sink::SinkCounts,
    secure_input: SecureInput,
) -> TimedRunReport {
    let snapshot = board
        .snapshot
        .unwrap_or_else(|| counters::Counters::default().snapshot(0, false));
    report_of(
        &snapshot,
        sink,
        &board.figures,
        board.failed_sources(),
        secure_input,
    )
}

/// The answer of `--probe-permissions`: its lines on `out`, nothing on a
/// system without a probe, and status 0.
pub fn probe_answer(permissions: &[Permission], out: &mut dyn std::io::Write) -> i32 {
    let text = permissions::system_probe()
        .map(|probe| permissions::probe_lines(&*probe, permissions))
        .unwrap_or_default();
    write_or_error(out, &text)
}

/// The answer of `--request-permission P`: its line on `out` and status 0.
pub fn request_answer(permission: Permission, out: &mut dyn std::io::Write) -> i32 {
    let text = permissions::system_probe()
        .map(|probe| permissions::request_line(&*probe, permission))
        .unwrap_or_default();
    write_or_error(out, &text)
}

fn write_or_error(out: &mut dyn std::io::Write, text: &str) -> i32 {
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => halvo_control::exit::CLEAN,
        Err(_) => halvo_control::exit::ERROR,
    }
}

/// Runs the self-check of the main run loop and prints its line on `out`.
#[cfg(debug_assertions)]
pub fn self_check_run_loop(out: &mut dyn std::io::Write) -> i32 {
    #[cfg(target_os = "macos")]
    {
        match crate::macos::self_check_run_loop() {
            Ok(ran_after) => {
                let (line, status) = main_loop::self_check_answer(ran_after);
                match writeln!(out, "{line}") {
                    Ok(()) => status,
                    Err(_) => halvo_control::exit::ERROR,
                }
            }
            Err(_) => halvo_control::exit::ERROR,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = out;
        halvo_control::exit::ERROR
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
            let_go: Vec::new(),
            refusal: None,
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
    /// The refusal of the hold of a run whose life lost events: the
    /// refusal's text, then the loss line on a line of its own (contracts.md
    /// C4 item 10).
    #[error("{refusal}\n{loss}")]
    RefusedWithLoss {
        refusal: Box<DaemonError>,
        loss: Loss,
    },
    #[error("the store owner panicked")]
    DrainPanicked,
    #[error("the timed run is entered on the process main thread")]
    NotMainThread,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
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

    fn timed(dirs: &Dirs, rest: &[&str]) -> Result<Mode, Refusal> {
        let mut line = vec!["--timed-run"];
        line.extend_from_slice(rest);
        parse(args(&line), &dirs.account)
    }

    #[test]
    fn the_permission_modes_parse_in_every_build() {
        let dirs = dirs();
        let parsed = |line: &[&str]| parse(args(line), &dirs.account);
        assert_eq!(
            parsed(&["--probe-permissions"]),
            Ok(Mode::ProbePermissions(Vec::new()))
        );
        assert_eq!(
            parsed(&[
                "--probe-permissions",
                "screen_and_system_audio_recording",
                "input_monitoring"
            ]),
            Ok(Mode::ProbePermissions(vec![
                Permission::ScreenAndSystemAudioRecording,
                Permission::InputMonitoring
            ]))
        );
        assert_eq!(
            parsed(&["--request-permission", "accessibility"]),
            Ok(Mode::RequestPermission(Permission::Accessibility))
        );
        for line in [
            &["--probe-permissions", "camera"][..],
            &["--request-permission"][..],
            &["--request-permission", "input_monitoring", "accessibility"][..],
            &["--request-permission", "microphone"][..],
        ] {
            let refused = parsed(line).unwrap_err();
            assert_eq!(
                usage_answer(&refused, &mut Vec::new()),
                halvo_control::exit::USAGE,
                "{line:?}"
            );
        }
    }

    #[test]
    fn the_kind_of_a_timed_run_holds_whether_its_flags_set_key_class_on() {
        let run = |flags: Vec<(Switch, bool)>| TimedRun {
            window: Duration::from_secs(1),
            secure_input: SecureInput::DoNotHold,
            switches: SwitchFlags(flags),
            store_dir: PathBuf::from("/"),
        };
        let flagged = |on| sources::RunKind::Timed { key_class_flag: on };
        assert_eq!(run(Vec::new()).kind(), flagged(false));
        assert_eq!(run(vec![(Switch::KeyClass, true)]).kind(), flagged(true));
        assert_eq!(run(vec![(Switch::KeyClass, false)]).kind(), flagged(false));
        assert_eq!(
            run(vec![(Switch::ScreenActivity, true)]).kind(),
            flagged(false)
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_self_check_takes_run_loop_and_nothing_else() {
        let dirs = dirs();
        let parsed = |line: &[&str]| parse(args(line), &dirs.account);
        assert_eq!(
            parsed(&["--self-check", "run-loop"]),
            Ok(Mode::SelfCheckRunLoop)
        );
        assert!(parsed(&["--self-check"]).is_err());
        assert!(parsed(&["--self-check", "run-loop", "again"]).is_err());
        assert!(parsed(&["--self-check", "other"]).is_err());
    }

    #[cfg(all(target_os = "macos", debug_assertions))]
    #[test]
    fn timed_run_refuses_a_thread_that_is_not_the_main_one() {
        let dir = test_dir();
        let run = TimedRun {
            window: Duration::from_secs(1),
            secure_input: SecureInput::DoNotHold,
            switches: SwitchFlags::default(),
            store_dir: dir.path().to_path_buf(),
        };
        let answer = std::thread::spawn(move || timed_run(&run).map(|outcome| outcome.status))
            .join()
            .unwrap();
        assert!(
            matches!(answer, Err(DaemonError::NotMainThread)),
            "{answer:?}"
        );
    }

    #[cfg(debug_assertions)]
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

    #[cfg(debug_assertions)]
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

    /// Unix alone, and not as root, whom a mode of 0600 does not stop.
    #[cfg(all(unix, debug_assertions))]
    #[test]
    fn a_dir_whose_store_key_cannot_be_read_is_refused_by_the_first_three_modes() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dirs = dirs();
        let d = dirs.store.as_str();
        let key = Path::new(d).join("store.key");
        std::fs::write(&key, b"{}").unwrap();
        if std::fs::metadata(&key).unwrap().uid() == 0 {
            println!("not run: the test runs as root, whom a mode of 0600 does not stop");
            return;
        }
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o600)).unwrap();
        let refused = [
            parse(args(&["--timed-run", "1", "--store-dir", d]), &dirs.account),
            parse(
                args(&[
                    "--verify-store",
                    "--store-dir",
                    d,
                    "--marker",
                    "M4rker.unread",
                ]),
                &dirs.account,
            ),
            parse(
                args(&["--verify-deletion", "--store-dir", d]),
                &dirs.account,
            ),
        ];
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o755)).unwrap();
        for answer in refused {
            assert!(
                answer.is_err(),
                "a DIR that cannot be searched was taken: {answer:?}"
            );
        }
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

    #[cfg(all(target_os = "macos", debug_assertions))]
    #[test]
    fn a_term_ends_every_mode_but_the_timed_run_by_the_default_action() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::process::ExitStatusExt;
        use std::time::Instant;

        const NAME: &str =
            "daemon::tests::a_term_ends_every_mode_but_the_timed_run_by_the_default_action";
        const CHILD: &str = "HALVO_DAEMON_MODE_SIGNAL_CHILD";
        const HOME: &str = "HALVO_DAEMON_MODE_SIGNAL_HOME";
        const STORE: &str = "HALVO_DAEMON_MODE_SIGNAL_STORE";
        if let Some(mode) = std::env::var_os(CHILD) {
            let account = TestAccount(PathBuf::from(std::env::var_os(HOME).unwrap()));
            let store = std::env::var(STORE).unwrap();
            let line = match mode.to_str().unwrap() {
                "timed-run" => vec!["--timed-run", "1", "--store-dir", &store],
                "verify-store" => vec![
                    "--verify-store",
                    "--store-dir",
                    &store,
                    "--marker",
                    "M4rker.term",
                ],
                "verify-deletion" => vec!["--verify-deletion", "--store-dir", &store],
                "delete-all" => vec!["--delete-all", "--store-dir", &store],
                other => panic!("no mode {other}"),
            };
            let mode = parse(args(&line), &account).unwrap();
            prepare_signals(&mode).unwrap();
            let mut out = std::io::stdout().lock();
            writeln!(out, "mode-child: ready").unwrap();
            out.flush().unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !signal::flag().load(Ordering::SeqCst) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            writeln!(
                out,
                "mode-child: flag={}",
                signal::flag().load(Ordering::SeqCst)
            )
            .unwrap();
            out.flush().unwrap();
            return;
        }
        let dirs = dirs();
        for (mode, ended_by_term) in [
            ("verify-store", true),
            ("verify-deletion", true),
            ("delete-all", true),
            ("timed-run", false),
        ] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
                .env(CHILD, mode)
                .env(HOME, &dirs.account.0)
                .env(STORE, &dirs.store)
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let pid = child.id().to_string();
            let (lines_tx, lines) = std::sync::mpsc::channel();
            let stdout = child.stdout.take().unwrap();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(at) = line.find("mode-child: ") {
                        let _ = lines_tx.send(line[at + "mode-child: ".len()..].to_owned());
                    }
                }
            });
            let ready = lines.recv_timeout(Duration::from_secs(15));
            if ready.as_deref() != Ok("ready") {
                let _ = child.kill();
                panic!("{mode}: the child did not get ready: {ready:?}");
            }
            let sent = std::process::Command::new("/bin/kill")
                .args(["-s", "TERM", &pid])
                .status()
                .unwrap();
            assert!(sent.success(), "{mode}: kill answered {sent}");
            let until = Instant::now() + Duration::from_secs(10);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= until {
                    let _ = child.kill();
                    panic!("{mode}: the child lived 10 s after TERM");
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            reader.join().unwrap();
            let after: Vec<String> = lines.try_iter().collect();
            if ended_by_term {
                assert_eq!(
                    status.signal(),
                    Some(libc::SIGTERM),
                    "{mode}: {status} after {after:?}"
                );
            } else {
                assert!(status.success(), "{mode}: {status} after {after:?}");
                assert_eq!(after, ["flag=true"], "{mode}");
            }
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_scratch_delete_takes_its_store_dir_as_given_and_no_other_flag() {
        let dirs = dirs();
        let d = dirs.store.as_str();
        std::fs::write(Path::new(d).join("store.key"), b"{}").unwrap();
        let parse_line = |line: &[&str]| parse(args(line), &dirs.account);
        for given in [d.to_owned(), format!("{d}/"), format!("{d}/.")] {
            assert_eq!(
                parse_line(&["--delete-all", "--store-dir", &given]),
                Ok(Mode::DeleteAll {
                    store_dir: PathBuf::from(&given)
                }),
                "{given}"
            );
        }
        for line in [
            &["--delete-all"][..],
            &["--delete-all", "--store-dir"],
            &["--delete-all", "--store-dir", "relative"],
            &["--delete-all", "--store-dir", d, "--store-dir", d],
            &[
                "--delete-all",
                "--store-dir",
                d,
                "--marker",
                "M4rker.delete",
            ],
            &["--delete-all", "--store-dir", d, "--hold-secure-input"],
            &["--delete-all", "--store-dir", d, "--switch", "key_class=on"],
            &["--delete-all", "--data-dir", d],
        ] {
            let refused = parse_line(line);
            assert!(
                matches!(refused, Err(Refusal::Usage(_))),
                "{line:?} gave {refused:?}"
            );
        }
        let refused = parse(
            args(&["--delete-all", "--store-dir", d]),
            &FailedLookup(Ok(None)),
        );
        assert_eq!(refused, Err(Refusal::NoDataDir(PathError::Unset)));
    }

    fn scratch_answer(store_dir: &Path, answer: TailAnswer) -> (i32, String, String, Vec<PathBuf>) {
        let mut calls = Vec::new();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = scratch_delete(
            store_dir,
            |dir| {
                calls.push(dir.to_path_buf());
                answer
            },
            &mut out,
            &mut err,
        );
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            calls,
        )
    }

    fn finished_report(root: TailRoot) -> TailReport {
        TailReport {
            root,
            roots_listed: true,
            roots_at_their_path: true,
            key_file_gone: true,
            ..TailReport::default()
        }
    }

    #[test]
    fn a_scratch_delete_with_nothing_named_prints_no_store_and_exits_0() {
        let dir = Path::new("/scratch/empty");
        let (status, out, err, calls) = scratch_answer(
            dir,
            TailAnswer::Finished(finished_report(TailRoot::Unnamed)),
        );
        assert_eq!(status, exit::CLEAN);
        assert_eq!(out, "store_dir=/scratch/empty\nno_store\n");
        assert_eq!(err, "");
        assert_eq!(calls, [dir]);
    }

    #[test]
    fn an_ok_answer_that_met_a_link_or_an_entry_of_another_kind_prints_the_deleted_line() {
        let report = TailReport {
            not_followed: 1,
            ..finished_report(TailRoot::Unnamed)
        };
        let (status, out, err, _) = scratch_answer(Path::new("/s"), TailAnswer::Finished(report));
        assert_eq!(status, exit::CLEAN);
        assert_eq!(
            out,
            "store_dir=/s\ndeleted segments_shredded=unknown keys_destroyed=unknown root_destroyed=unknown root_id=- files_left=0 database_left=no not_followed=1\n"
        );
        assert_eq!(err, "");
    }

    #[test]
    fn a_not_finished_answer_with_nothing_to_name_still_says_it_is_not_finished() {
        let report = finished_report(TailRoot::Destroyed);
        let (status, _, err, _) = scratch_answer(Path::new("/s"), TailAnswer::NotFinished(report));
        assert_eq!(status, exit::ERROR);
        assert_eq!(err, "halvo-capture: the delete is not finished\n");
    }

    #[test]
    fn a_line_fault_of_the_store_dir_is_usage_also_when_the_account_lookup_fails() {
        let dirs = dirs();
        let parent = format!("{}/../store", dirs.store);
        for given in ["relative", parent.as_str()] {
            for mode in [
                &["--delete-all", "--store-dir", given][..],
                &["--verify-deletion", "--store-dir", given],
                &["--timed-run", "1", "--store-dir", given],
            ] {
                let refused = parse(args(mode), &FailedLookup(Err(PathError::Lookup(2))));
                assert!(
                    matches!(refused, Err(Refusal::Usage(_))),
                    "{mode:?} gave {refused:?}"
                );
            }
        }
    }

    #[test]
    fn a_scratch_delete_destroys_the_root_through_its_one_tail_call() {
        let guard = test_dir();
        let root = guard.path().join("roots").join("stand-in.root");
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        std::fs::write(&root, [7u8; 32]).unwrap();
        let mut calls = 0;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = scratch_delete(
            guard.path(),
            |dir| {
                calls += 1;
                std::fs::remove_file(dir.join("roots").join("stand-in.root")).unwrap();
                TailAnswer::Finished(finished_report(TailRoot::Destroyed))
            },
            &mut out,
            &mut err,
        );
        let out = String::from_utf8(out).unwrap();
        assert!(
            !root.exists(),
            "the root stays after the scratch delete: {out}"
        );
        assert_eq!(calls, 1);
        assert_eq!(status, exit::CLEAN, "{out}");
        assert_eq!(
            out.lines().nth(1),
            Some("deleted segments_shredded=unknown keys_destroyed=unknown root_destroyed=yes root_id=- files_left=0 database_left=no not_followed=0")
        );
        assert!(err.is_empty());
    }

    #[test]
    fn the_scratch_deleted_line_has_the_seven_keys_of_the_report_and_not_the_four_of_a_data_directory(
    ) {
        let report = TailReport {
            keyed_segments: Some(4),
            root: TailRoot::Left(Some("0123456789abcdef0123456789abcdef".to_owned())),
            database_left: true,
            files_left: 2,
            not_followed: 3,
            ..finished_report(TailRoot::Unnamed)
        };
        let (status, out, _, _) = scratch_answer(Path::new("/s"), TailAnswer::NotFinished(report));
        assert_eq!(status, exit::ERROR);
        let line = out.lines().nth(1).unwrap();
        let keys: Vec<&str> = line
            .split(' ')
            .skip(1)
            .map(|token| token.split_once('=').unwrap().0)
            .collect();
        assert_eq!(
            keys,
            [
                "segments_shredded",
                "keys_destroyed",
                "root_destroyed",
                "root_id",
                "files_left",
                "database_left",
                "not_followed"
            ]
        );
        assert_eq!(
            line,
            "deleted segments_shredded=4 keys_destroyed=5 root_destroyed=no root_id=0123456789abcdef0123456789abcdef files_left=2 database_left=yes not_followed=3"
        );
        for absent in [
            "entries_removed=",
            "entries_left=",
            "unknown_entries=",
            "intent=",
            "store_id=",
        ] {
            assert!(!out.contains(absent), "{absent} in {out}");
        }
    }

    #[test]
    fn a_scratch_delete_that_is_not_finished_exits_1_and_names_root_files_alone() {
        let report = TailReport {
            root: TailRoot::Left(Some("0123456789abcdef0123456789abcdef".to_owned())),
            root_files_left: vec!["0123456789abcdef0123456789abcdef.root".to_owned()],
            others_left: EntriesLeft {
                root_shaped: 2,
                directories: 1,
                other: 3,
            },
            ..finished_report(TailRoot::Unnamed)
        };
        let (status, out, err, _) =
            scratch_answer(Path::new("/scratch/s"), TailAnswer::NotFinished(report));
        assert_eq!(status, exit::ERROR);
        assert!(out.starts_with("store_dir=/scratch/s\ndeleted "), "{out}");
        assert!(out.contains(" root_destroyed=no root_id=0123456789abcdef0123456789abcdef "));
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: this call did not destroy the store's root; the root file roots/0123456789abcdef0123456789abcdef.root stays; 2 files that can hold a root stay in the folder roots at other names; 1 folders stay in the folder roots at other names; 3 other entries stay in the folder roots at other names\n"
        );
        let unlisted = TailReport {
            root: TailRoot::Destroyed,
            roots_listed: false,
            roots_at_their_path: false,
            key_file_gone: false,
            database_left: true,
            files_left: 2,
            not_followed: 2,
            ..TailReport::default()
        };
        let (status, out, err, _) =
            scratch_answer(Path::new("/s"), TailAnswer::NotFinished(unlisted));
        assert_eq!(status, exit::ERROR);
        assert!(out.contains(" root_destroyed=yes root_id=- "), "{out}");
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: the folder roots could not be listed; the folder roots no longer stands at its path; store.key stays; a database file stays; 2 other files of the store stay; 2 names were a link or an entry of another kind and were not followed\n"
        );
        let unnamed = TailReport {
            files_left: 1,
            ..finished_report(TailRoot::Unnamed)
        };
        let (status, out, err, _) =
            scratch_answer(Path::new("/s"), TailAnswer::NotFinished(unnamed));
        assert_eq!(status, exit::ERROR);
        assert!(out.contains(" root_destroyed=unknown root_id=- "), "{out}");
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: 1 other files of the store stay\n"
        );
    }

    #[test]
    fn a_not_opened_report_says_the_store_dir_could_not_be_opened_and_nothing_of_what_stays() {
        let report = TailReport {
            root: TailRoot::Left(None),
            database_left: true,
            files_left: 1,
            roots_at_their_path: true,
            ..TailReport::default()
        };
        let (status, out, err, _) =
            scratch_answer(Path::new("/s"), TailAnswer::NotFinished(report.clone()));
        assert_eq!(status, exit::ERROR);
        assert!(
            out.ends_with(" files_left=1 database_left=yes not_followed=0\n"),
            "{out}"
        );
        assert_eq!(
            err,
            "halvo-capture: the delete is not finished: the store directory could not be opened, so this call cannot say what stays in it\n",
            "{report:?}"
        );
        let removal_left_these = TailReport {
            root: TailRoot::Destroyed,
            roots_listed: true,
            database_left: true,
            files_left: 2,
            roots_at_their_path: true,
            ..TailReport::default()
        };
        let (_, _, err, _) =
            scratch_answer(Path::new("/s"), TailAnswer::NotFinished(removal_left_these));
        assert!(
            err.contains("store.key stays; a database file stays"),
            "{err}"
        );
        let not_opened = TailReport {
            root: TailRoot::Left(None),
            roots_listed: true,
            database_left: true,
            files_left: 1,
            roots_at_their_path: true,
            ..TailReport::default()
        };
        for (what, other) in [
            (
                "the key file gone",
                TailReport {
                    key_file_gone: true,
                    ..not_opened.clone()
                },
            ),
            (
                "no database left",
                TailReport {
                    database_left: false,
                    ..not_opened.clone()
                },
            ),
            (
                "the root destroyed",
                TailReport {
                    root: TailRoot::Destroyed,
                    ..not_opened.clone()
                },
            ),
            (
                "the root left with its id",
                TailReport {
                    root: TailRoot::Left(Some("0123456789abcdef0123456789abcdef".to_owned())),
                    ..not_opened.clone()
                },
            ),
            (
                "the segments read",
                TailReport {
                    keyed_segments: Some(1),
                    ..not_opened.clone()
                },
            ),
        ] {
            let (_, _, err, _) = scratch_answer(Path::new("/s"), TailAnswer::NotFinished(other));
            assert!(
                !err.contains("could not be opened")
                    && err.contains("1 other files of the store stay"),
                "{what}: {err}"
            );
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_data_directory_layout_is_refused_by_the_parse_and_its_half_is_not() {
        let dirs = dirs();
        let d = Path::new(&dirs.store);
        let line = |dir: &Path| {
            parse(
                args(&["--delete-all", "--store-dir", dir.to_str().unwrap()]),
                &dirs.account,
            )
        };
        std::fs::create_dir(d.join("store")).unwrap();
        assert!(
            matches!(line(d), Ok(Mode::DeleteAll { .. })),
            "store/ alone"
        );
        std::fs::create_dir(d.join("roots")).unwrap();
        let refused = line(d);
        assert!(
            matches!(refused, Err(ref refusal) if !matches!(refusal, Refusal::NoDataDir(_))),
            "store/ beside roots/: {refused:?}"
        );
        std::fs::remove_dir(d.join("store")).unwrap();
        assert!(
            matches!(line(d), Ok(Mode::DeleteAll { .. })),
            "roots/ alone"
        );
        std::fs::write(d.join("store"), b"a file at the name").unwrap();
        assert!(
            line(d).is_err(),
            "an entry of any kind at store beside roots/"
        );
        #[cfg(unix)]
        {
            std::fs::remove_file(d.join("store")).unwrap();
            std::os::unix::fs::symlink(d.join("nowhere"), d.join("store")).unwrap();
            assert!(
                line(d).is_err(),
                "a link that leads nowhere at store, not followed"
            );
        }
    }

    fn refused_report() -> TailReport {
        TailReport {
            root: TailRoot::Left(None),
            database_left: true,
            files_left: 1,
            not_followed: 1,
            roots_at_their_path: true,
            ..TailReport::default()
        }
    }

    #[test]
    fn a_store_dir_that_the_tail_refuses_as_a_link_or_a_file_says_so_and_exits_1() {
        let (status, out, err, _) = scratch_answer(
            Path::new("/scratch/E-link/"),
            TailAnswer::NotFinished(refused_report()),
        );
        assert_eq!(status, exit::ERROR);
        assert!(
            out.starts_with("store_dir=/scratch/E-link/\ndeleted "),
            "{out}"
        );
        assert_eq!(
            err,
            "halvo-capture: /scratch/E-link/ is a link or a file, not a folder; nothing was removed; give the path of the folder itself\n"
        );
        for other in [
            TailReport {
                not_followed: 0,
                ..refused_report()
            },
            TailReport {
                roots_listed: true,
                ..refused_report()
            },
            TailReport {
                files_left: 0,
                ..refused_report()
            },
            TailReport {
                key_file_gone: true,
                ..refused_report()
            },
            TailReport {
                root: TailRoot::Left(Some("0123456789abcdef0123456789abcdef".to_owned())),
                ..refused_report()
            },
            TailReport {
                keyed_segments: Some(0),
                ..refused_report()
            },
            TailReport {
                database_left: false,
                ..refused_report()
            },
            TailReport {
                root_files_left: vec!["0123456789abcdef0123456789abcdef.root".to_owned()],
                ..refused_report()
            },
            TailReport {
                others_left: EntriesLeft {
                    other: 1,
                    ..EntriesLeft::default()
                },
                ..refused_report()
            },
        ] {
            let (status, _, err, _) =
                scratch_answer(Path::new("/s"), TailAnswer::NotFinished(other.clone()));
            assert_eq!(status, exit::ERROR);
            assert!(
                err.starts_with("halvo-capture: the delete is not finished: "),
                "{other:?}: {err}"
            );
        }
    }

    #[test]
    fn a_path_with_a_part_that_windows_trims_never_reads_as_a_link_on_windows() {
        for (path, trimmed) in [
            ("/scratch/name./E", true),
            ("/scratch/name /E", true),
            ("/scratch/E.", true),
            ("/scratch/E/", false),
            ("/scratch/E/.", false),
            ("/scratch/E", false),
            ("/scratch/.hidden/E", false),
        ] {
            assert_eq!(
                has_a_part_that_windows_trims(Path::new(path)),
                trimmed,
                "{path}"
            );
        }
        let (status, _, err, _) = scratch_answer(
            Path::new("/scratch/name./E"),
            TailAnswer::NotFinished(refused_report()),
        );
        assert_eq!(status, exit::ERROR);
        let link_line = err.contains("is a link or a file, not a folder");
        assert_eq!(link_line, !cfg!(windows), "{err}");
    }

    #[test]
    fn a_held_store_exits_75_and_a_failed_tail_exits_1_each_with_its_line() {
        let (status, out, err, _) = scratch_answer(Path::new("/s"), TailAnswer::Held);
        assert_eq!(status, exit::BUSY);
        assert_eq!(out, "store_dir=/s\n");
        assert_eq!(
            err,
            "halvo-capture: another process holds the store; this request removed nothing\n"
        );
        let (status, out, err, _) = scratch_answer(
            Path::new("/s"),
            TailAnswer::Failed("store i/o failed: permission denied".to_owned()),
        );
        assert_eq!(status, exit::ERROR);
        assert_eq!(out, "store_dir=/s\n");
        assert_eq!(err, "halvo-capture: store i/o failed: permission denied\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_scratch_delete_prints_a_control_character_of_its_path_as_a_question_mark() {
        let (_, out, _, calls) = scratch_answer(
            Path::new("/scratch/a\nb"),
            TailAnswer::Finished(finished_report(TailRoot::Unnamed)),
        );
        assert_eq!(out, "store_dir=/scratch/a?b\nno_store\n");
        assert_eq!(calls, [Path::new("/scratch/a\nb")]);
    }

    #[test]
    fn a_scratch_delete_whose_lines_cannot_be_written_exits_1_after_the_tail() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        drop(reader);
        let mut calls = 0;
        let mut err = Vec::new();
        let status = scratch_delete(
            Path::new("/s"),
            |_| {
                calls += 1;
                TailAnswer::Finished(finished_report(TailRoot::Destroyed))
            },
            &mut writer,
            &mut err,
        );
        assert_eq!((status, calls), (exit::ERROR, 1));
        assert!(String::from_utf8(err)
            .unwrap()
            .starts_with("halvo-capture: the report was not written: "),);
    }

    struct FailedLookup(Result<Option<&'static str>, PathError>);

    impl AccountLookup for FailedLookup {
        fn base_dir(&self) -> Result<Option<OsString>, PathError> {
            self.0.map(|home| home.map(OsString::from))
        }

        fn user_temp_dir(&self) -> Result<Option<DarwinUserTempDir>, PathError> {
            Ok(None)
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    fn an_account_that_gives_no_data_directory_is_no_usage_refusal_and_names_no_dir() {
        let dirs = dirs();
        let d = dirs.store.as_str();
        for account in [
            FailedLookup(Err(PathError::Lookup(2))),
            FailedLookup(Ok(None)),
            FailedLookup(Ok(Some(""))),
            FailedLookup(Ok(Some("relative-home"))),
        ] {
            for line in [
                &["--timed-run", "1", "--store-dir", d][..],
                &[
                    "--verify-store",
                    "--store-dir",
                    d,
                    "--marker",
                    "M4rker.no-home",
                ],
                &["--verify-deletion", "--store-dir", d],
            ] {
                let refused = parse(args(line), &account).unwrap_err();
                let mut err = Vec::new();
                let status = usage_answer(&refused, &mut err);
                let text = String::from_utf8(err).unwrap();
                assert_eq!(status, exit::ERROR, "{line:?}: {text}");
                assert!(
                    text.starts_with("halvo-capture: the account gives no data directory to check the store directory against: "),
                    "{line:?}: {text}"
                );
                assert!(
                    !text.contains("DIR") && !text.contains("usage:") && !text.contains(d),
                    "{line:?}: {text}"
                );
            }
        }
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn a_release_build_refuses_the_three_gate_modes_as_usage() {
        let dirs = dirs();
        let d = dirs.store.as_str();
        for line in [
            &["--timed-run", "5", "--store-dir", d][..],
            &["--timed-run", "5", "--store-dir", d, "--hold-secure-input"][..],
            &[
                "--timed-run",
                "5",
                "--store-dir",
                d,
                "--switch",
                "screen_activity=on",
            ][..],
            &[
                "--verify-store",
                "--store-dir",
                d,
                "--marker",
                "M4rker.release",
            ][..],
            &["--verify-deletion", "--store-dir", d][..],
            &["--delete-all", "--store-dir", d][..],
        ] {
            let without_a_data_dir = parse(args(line), &FailedLookup(Ok(None)));
            assert!(
                matches!(without_a_data_dir, Err(Refusal::Usage(_))),
                "{line:?} gave {without_a_data_dir:?}"
            );
            let refused = parse(args(line), &dirs.account);
            let Err(refusal @ Refusal::Usage(_)) = refused else {
                panic!("{line:?} gave {refused:?}");
            };
            let mut err = Vec::new();
            assert_eq!(usage_answer(&refusal, &mut err), 64, "{line:?}");
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
        run_timed(run, scripted, store, clocks, &stop, Duration::from_secs(2)).unwrap()
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
        // Ruling DM-25 N4: the refusal is carried in the outcome, and the
        // binary's answer to it is its line and status 1 with no report.
        let outcome = ended.unwrap();
        assert_eq!(
            outcome.refusal,
            Some(DaemonError::SecureInputRefused(-25291).to_string())
        );
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(outcome_answer(&outcome, &mut out, &mut err), 1);
        assert!(out.is_empty());
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "halvo-capture: the system refused the secure-input hold (status -25291)\n"
        );
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
        assert_eq!(loss.cause, Some(LossCause::LostSinceAcknowledged));
        assert!(
            loss.line.starts_with("halvo-capture: lost events=4 first="),
            "{loss}"
        );
        assert!(loss.line.ends_with(" error=-"), "{loss}");
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
        assert_eq!(loss.cause, Some(LossCause::Pending));
        assert!(
            loss.line.starts_with("halvo-capture: lost events=4 "),
            "{loss}"
        );
        assert!(loss.line.ends_with(" error=io"), "{loss}");
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
        assert_eq!(
            outcome.loss.and_then(|loss| loss.cause),
            Some(LossCause::LostSinceAcknowledged)
        );

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
        let loss = outcome
            .loss
            .expect("a life that lost events printed no line");
        assert_eq!(loss.cause, None, "{loss}");
        assert!(
            loss.line
                .starts_with("halvo-capture: lost events=512 first=2"),
            "{loss}"
        );
        assert!(loss.line.ends_with(" error=-"), "{loss}");
    }

    /// A store whose every write takes 6 s, three times the settle bound of
    /// the test that uses it, and fails.
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
            std::thread::sleep(Duration::from_secs(6));
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
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(outcome.status, exit::ERROR);
        assert_eq!(
            outcome.loss.and_then(|loss| loss.cause),
            Some(LossCause::SettleUnanswered)
        );
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
        let with = |flags: &[(Switch, bool)]| {
            sources::starting(
                halvo_control::proto::Platform::Macos,
                sources::RunKind::Timed {
                    key_class_flag: false,
                },
                SwitchFlags(flags.to_vec()).applied(),
            )
        };
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
        // Ruling DM-25 N5: a stop that answers ThreadEnded is a stop, and
        // its source, which died before the stop, is named in the list.
        assert_eq!(line_of(&text, "failed_sources="), "failed_sources=2");
        assert!(
            text.contains(&format!("\nfailed_source=screen: {timeout}\n")),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "\nfailed_source={}: {}\n",
                scripted.sources[2].0.as_str(),
                CaptureError::ThreadEnded
            )),
            "{text}"
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
        assert_eq!(loss.cause, Some(LossCause::LostDuringStop));
        assert!(
            loss.line.starts_with("halvo-capture: lost events=512 "),
            "{loss}"
        );
    }

    /// A model store whose expiry, once armed, blocks the owner for `block`.
    struct ExpiryThatBlocksOnceArmed {
        inner: ModelStore,
        armed: Arc<AtomicBool>,
        entered: Arc<AtomicBool>,
        block: Duration,
    }

    impl MaintainStore for ExpiryThatBlocksOnceArmed {
        fn expire_due(
            &mut self,
            now: std::time::SystemTime,
            max: usize,
        ) -> Result<Pass, MaintFault> {
            if self.armed.load(Ordering::SeqCst) {
                self.entered.store(true, Ordering::SeqCst);
                std::thread::sleep(self.block);
            }
            self.inner.expire_due(now, max)
        }

        fn evict_to(
            &mut self,
            bound: super::port::ByteBound,
            max: usize,
        ) -> Result<Pass, MaintFault> {
            self.inner.evict_to(bound, max)
        }

        fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
            self.inner.usage()
        }
    }

    impl CaptureStore for ExpiryThatBlocksOnceArmed {
        fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
            self.inner.write_batch(write)
        }

        fn volume_free_bytes(&self) -> std::io::Result<u64> {
            self.inner.volume_free_bytes()
        }
    }

    /// Pin of the coordinator's answer msg_3e2afe836f9b (a): a Shutdown that is
    /// not answered after a last settle that answered clean gives status 1, a
    /// cause that ruling DM-4 item 5 does not list.
    #[test]
    fn a_shutdown_unanswered_after_a_clean_settle_gives_exit_1() {
        let clocks = ManualClocks::at(noon());
        let bound = Duration::from_secs(2);
        let armed = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(AtomicBool::new(false));
        let store = ExpiryThatBlocksOnceArmed {
            inner: ModelStore::new(),
            armed: Arc::clone(&armed),
            entered: Arc::clone(&entered),
            block: 3 * bound,
        };
        let mut scripted = sources();
        let hook_clocks = clocks.clone();
        scripted.on_figures = Some(Box::new(move || {
            armed.store(true, Ordering::SeqCst);
            hook_clocks.advance(Duration::from_secs(61));
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while !entered.load(Ordering::SeqCst) && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(entered.load(Ordering::SeqCst), "the owner ran no expiry");
        }));
        let stop = AtomicBool::new(false);
        let outcome = run_timed(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            store,
            clocks,
            &stop,
            bound,
        )
        .unwrap();
        assert_eq!(outcome.status, exit::ERROR, "{outcome:?}");
        let loss = outcome.loss.expect("no loss line");
        assert_eq!(loss.cause, Some(LossCause::ShutdownUnanswered));
        assert_eq!(
            outcome.report.events_persisted, 4,
            "the last settle was clean"
        );
    }

    /// Ruling DM-16 (1) c, the verifier's Issue 1: the Shutdown's wait from
    /// below. The figures hook arms an expiry that blocks the owner for three
    /// quarters of the Shutdown's part, so the Shutdown's answer comes after
    /// half of the part and before its end; the run takes that answer and
    /// stores what it holds. It waits on time, with a margin of a quarter of
    /// the part, 1 s, on each side.
    #[test]
    fn a_shutdown_answered_late_in_its_part_is_taken_and_stores_the_events() {
        let clocks = ManualClocks::at(noon());
        let bound = Duration::from_secs(4);
        assert_eq!(StopBounds::of(bound).shutdown, bound);
        let armed = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(AtomicBool::new(false));
        let store = ExpiryThatBlocksOnceArmed {
            inner: ModelStore::new(),
            armed: Arc::clone(&armed),
            entered: Arc::clone(&entered),
            block: bound * 3 / 4,
        };
        let model = store.inner.clone();
        let mut scripted = sources();
        let hook_clocks = clocks.clone();
        scripted.on_figures = Some(Box::new(move || {
            armed.store(true, Ordering::SeqCst);
            hook_clocks.advance(Duration::from_secs(61));
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while !entered.load(Ordering::SeqCst) && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(entered.load(Ordering::SeqCst), "the owner ran no expiry");
        }));
        let stop = AtomicBool::new(false);
        let outcome = run_timed(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            store,
            clocks,
            &stop,
            bound,
        )
        .unwrap();
        assert!(outcome.loss.is_none(), "{:?}", outcome.loss);
        assert_eq!(outcome.status, exit::CLEAN, "{outcome:?}");
        assert_eq!(outcome.report.events_persisted, 4);
        assert_eq!(model.records().len(), 4);
    }

    /// Pin of the coordinator's answer msg_3e2afe836f9b (b): a signalled run
    /// with a loss gives status 1, because the loss is taken first, where
    /// contracts.md C4 item 6 says that a signalled run exits 2.
    #[test]
    fn a_signalled_run_with_a_loss_gives_exit_1_and_not_2() {
        let clocks = ManualClocks::at(noon());
        let mut scripted = sources();
        scripted.signal = true;
        let store = ModelStore::new();
        store.fail_every_write(Some(WriteFault::Refused));
        let outcome = run_scripted(
            &run_of(600, SecureInput::DoNotHold, &[]),
            &mut scripted,
            store,
            clocks,
        );
        assert_eq!(outcome.status, exit::ERROR, "{outcome:?}");
        assert_eq!(
            outcome.loss.and_then(|loss| loss.cause),
            Some(LossCause::LostSinceAcknowledged)
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
        assert_eq!(
            outcome.loss.as_ref().and_then(|loss| loss.cause),
            Some(LossCause::PauseUnanswered)
        );
        assert!(
            took < DRAIN_BOUND + settle_bound + Duration::from_secs(1),
            "the run took {took:?}, more than DRAIN_BOUND {DRAIN_BOUND:?} + settle bound \
             {settle_bound:?} + 1s: the OpenPause call waited for the parked write"
        );
    }
}
