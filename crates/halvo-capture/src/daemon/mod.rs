//! The capture daemon: the store owner, which is the one thread that holds the
//! capture store, and the gate's timed run on that owner.

#[cfg(target_os = "macos")]
pub(crate) mod agent_marks;
#[cfg(any(test, target_os = "macos", windows))]
pub mod barrier;
#[cfg(any(test, target_os = "macos", windows))]
pub mod batch;
#[cfg(any(test, target_os = "macos", windows))]
pub mod board;
pub mod children;
#[cfg(any(test, target_os = "macos", windows))]
pub mod counters;
pub mod custody_policy;
#[cfg(any(test, target_os = "macos", windows))]
pub mod delete_all;
pub mod instance;
pub mod lease;
pub mod main_loop;
#[cfg(any(test, target_os = "macos", windows))]
pub mod maintenance;
#[cfg(any(test, target_os = "macos", windows))]
mod mask;
#[cfg(test)]
pub(crate) mod model_store;
#[cfg(any(test, target_os = "macos", windows))]
pub mod owner;
pub mod owner_pid;
#[cfg(test)]
mod owner_props;
pub mod permissions;
pub mod port;
#[cfg(test)]
mod process_tests;
#[rustfmt::skip]
#[cfg(feature = "r2")] pub mod r2;
mod report;
#[cfg(any(target_os = "macos", windows))]
mod run_marker;
#[cfg(test)]
mod scripted;
#[cfg(any(test, target_os = "macos", windows))]
pub mod server;
pub mod signal;
#[cfg(any(test, target_os = "macos", windows))]
pub mod slot;
pub mod sources;
pub mod state;
#[cfg(test)]
mod stop_tests;
#[cfg(any(test, target_os = "macos", windows))]
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
       halvo-capture --delete-all --store-dir DIR
       halvo-capture --delete-all --data-dir DIR
       halvo-capture --default-data-dir
       halvo-capture --service-run [--lifeline [--no-press]] [--data-dir DIR]
       halvo-capture ctl [--data-dir DIR] VERB";

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
    /// `--self-check service-run --data-dir DIR`: the service run on DIR in
    /// its wait, while a second thread queues a block on the main run loop.
    #[cfg(debug_assertions)]
    SelfCheckServiceRun {
        data_dir: halvo_control::paths::DataDir,
    },
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
    /// `--service-run [--data-dir DIR]` (contracts.md C3.2), which a release
    /// build has on a Mac and on Windows (contracts.md C3.1).
    #[cfg(any(debug_assertions, target_os = "macos", windows))]
    ServiceRun {
        data_dir: halvo_control::paths::DataDir,
    },
    /// `--service-run --lifeline [--no-press] [--data-dir DIR]` (contracts.md
    /// C3.2 item 4, C14 items 2 and 4): the service run that ends with the app
    /// that started it, and with `--no-press` one that no person's press
    /// stands behind.
    #[cfg(any(debug_assertions, target_os = "macos", windows))]
    LifelineServiceRun {
        data_dir: halvo_control::paths::DataDir,
        behind: Behind,
    },
    /// `--delete-all --data-dir DIR`, the offline delete of a data directory
    /// (contracts.md C3.3), with DIR absolute. A macOS or Windows build runs
    /// it; a debug build of another system exits 64 and prints no text.
    #[cfg(any(debug_assertions, target_os = "macos", windows))]
    DeleteAllOfADataDir {
        data_dir: halvo_control::paths::DataDir,
    },
    /// `--default-data-dir`: the data directory that `--service-run` takes
    /// without `--data-dir`, which the binary prints on one line, so that the
    /// Windows uninstaller names it by the program and holds no copy of the
    /// rule of the paths.
    #[cfg(any(debug_assertions, target_os = "macos", windows))]
    DefaultDataDir {
        data_dir: halvo_control::paths::DataDir,
    },
    /// `ctl [--data-dir DIR] VERB` (contracts.md C2.2 to C2.7): one request
    /// to the service run of the data directory, through halvo-control's
    /// client and its text.
    #[cfg(any(debug_assertions, target_os = "macos", windows))]
    Ctl(halvo_control::ctl_text::Ctl),
    /// `--service-run [--data-dir DIR] --scripted-sources`, in a build with
    /// the feature test-seam alone: the service run with the scripted sources
    /// of [`seam`] in place of the system's.
    #[cfg(all(debug_assertions, feature = "test-seam"))]
    ScriptedServiceRun {
        data_dir: halvo_control::paths::DataDir,
    },
}

/// Whether the starter of a service run says that no person's press stands
/// behind its start (`--no-press` after `--lifeline`, contracts.md C14 item
/// 4): the app says it at each start that it makes by itself from the end of
/// another starter's service on, until a person's press.
#[cfg(any(debug_assertions, target_os = "macos", windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behind {
    /// Any other start: it serves whatever the run marker says.
    Unsaid,
    /// Before it serves, it reads in the run marker why the last life of its
    /// data directory ended, and where the table of `run_marker::LastEnd`
    /// answers a wait, it ends with `halvo_control::exit::WAITS_FOR_A_PRESS`
    /// before it binds its channel and before any read of the key or of the
    /// store, and leaves the marker as it found it.
    NoPress,
}

impl Mode {
    /// Whether the mode reads the signal flag that [`signal::prepare`]
    /// installs: the timed run, which stops its sources, settles and reports
    /// on it, and the service run, which ends on it through the shutdown
    /// entry.
    #[cfg(any(debug_assertions, target_os = "macos", windows))]
    pub fn reads_the_signal_flag(&self) -> bool {
        #[cfg(all(debug_assertions, feature = "test-seam"))]
        if matches!(self, Mode::ScriptedServiceRun { .. }) {
            return true;
        }
        #[cfg(debug_assertions)]
        if matches!(self, Mode::TimedRun(_)) {
            return true;
        }
        matches!(
            self,
            Mode::ServiceRun { .. } | Mode::LifelineServiceRun { .. }
        )
    }

    /// Whether the mode reads the signal flag: no mode of a release build
    /// on a system without the service run does.
    #[cfg(not(any(debug_assertions, target_os = "macos", windows)))]
    pub fn reads_the_signal_flag(&self) -> bool {
        false
    }
}

/// Installs the signal flag for a mode that reads it and nothing for any
/// other mode, which SIGINT and SIGTERM then end by the system's default
/// action (ruling DM-12 P3). On Windows a console's Ctrl-C and Ctrl-Break
/// serve a timed run alone (unit win-5, X-windows-03): a service run ends by
/// its session window, its lifeline or a Stop request.
pub fn prepare_signals(mode: &Mode) -> std::io::Result<()> {
    #[cfg(all(windows, debug_assertions))]
    if !matches!(mode, Mode::TimedRun(_)) {
        return Ok(());
    }
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

/// The stem of the line of an account that gives no data directory, which a
/// service run and `--default-data-dir` without `--data-dir` print before the
/// cause, on standard error with exit 1.
pub const NO_DATA_DIR: &str = "halvo-capture: the account gives no data directory";

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
    /// A service run with no `--data-dir` whose account gives no data
    /// directory, and so `--default-data-dir` too; it exits 1.
    #[error("{NO_DATA_DIR}: {0}")]
    NoServiceDir(PathError),
    /// A refused line of `ctl`, with the reason and ctl's own usage; it exits
    /// 64.
    #[cfg(any(debug_assertions, target_os = "macos", windows))]
    #[error("halvo-capture: {0}\n{usage}", usage = halvo_control::ctl_text::USAGE)]
    Ctl(String),
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
        #[cfg(any(debug_assertions, target_os = "macos", windows))]
        Refusal::Ctl(_) => halvo_control::exit::USAGE,
        Refusal::NoDataDir(_) | Refusal::NoServiceDir(_) => halvo_control::exit::ERROR,
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
    /// On Windows, the tail was refused the lock because the lock file stands
    /// marked for removal while another handle keeps it open.
    pub lock_file_kept: bool,
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
    /// and read no root id reads the same, and so does a tail refused the lock
    /// that read exactly those three standing and no root id.
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

    /// What stays, for standard error: first, on Windows, a lock file that
    /// another program keeps open behind its mark; the root files and partial
    /// root files by their names in the folder roots, every other entry there
    /// by kind; then the store's files, the count of its other files behind
    /// its words.
    fn what_stays(&self) -> String {
        let mut stays = Vec::new();
        if self.lock_file_kept {
            stays.push("another program holds the store's lock file open".to_owned());
        }
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
            stays.push(format!(
                "other files of the store that stay: {}",
                self.files_left
            ));
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
    #[cfg(not(any(debug_assertions, target_os = "macos", windows)))]
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
        Some("--self-check") if args.get(1).is_some_and(|form| form == "service-run") => {
            match &args[2..] {
                [flag, _] if flag == "--data-dir" => Ok(Mode::SelfCheckServiceRun {
                    data_dir: service_dir(&args[2..], account)?,
                }),
                _ => Err(Usage("--self-check service-run takes --data-dir DIR").into()),
            }
        }
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
        #[cfg(any(debug_assertions, target_os = "macos", windows))]
        Some("--delete-all") if args.get(1).is_some_and(|flag| flag == "--data-dir") => {
            match &args[1..] {
                [_, dir] => halvo_control::paths::DataDir::from_flag(dir.as_str())
                    .map(|data_dir| Mode::DeleteAllOfADataDir { data_dir })
                    .map_err(|_| Usage("--data-dir takes an absolute directory").into()),
                [_] => Err(Usage("--data-dir needs DIR").into()),
                _ => Err(Usage("--delete-all --data-dir takes DIR and no other flag").into()),
            }
        }
        #[cfg(any(debug_assertions, target_os = "macos", windows))]
        Some("--default-data-dir") => match &args[1..] {
            [] => Ok(Mode::DefaultDataDir {
                data_dir: service_dir(&[], account)?,
            }),
            _ => Err(Usage("--default-data-dir takes no other word").into()),
        },
        #[cfg(debug_assertions)]
        Some("--delete-all") => {
            let flags = gate::Flags::read(&args[1..], &["--store-dir"])?;
            let store_dir =
                gate::store_dir(flags.store_dir, account)?.without_a_data_directory_layout()?;
            Ok(Mode::DeleteAll { store_dir })
        }
        #[cfg(any(debug_assertions, target_os = "macos", windows))]
        Some("ctl") => {
            let words: Vec<OsString> = args[1..].iter().map(OsString::from).collect();
            halvo_control::ctl_text::parse(&words)
                .map(Mode::Ctl)
                .map_err(|refused| Refusal::Ctl(refused.0))
        }
        #[cfg(all(debug_assertions, feature = "test-seam"))]
        Some("--service-run") if args.last().is_some_and(|flag| flag == seam::FLAG) => {
            Ok(Mode::ScriptedServiceRun {
                data_dir: service_dir(&args[1..args.len() - 1], account)?,
            })
        }
        #[cfg(any(debug_assertions, target_os = "macos", windows))]
        Some("--service-run") if args.get(1).is_some_and(|flag| flag == "--lifeline") => {
            let (behind, rest) = match args.get(2) {
                Some(flag) if flag == "--no-press" => (Behind::NoPress, &args[3..]),
                _ => (Behind::Unsaid, &args[2..]),
            };
            Ok(Mode::LifelineServiceRun {
                data_dir: service_dir(rest, account)?,
                behind,
            })
        }
        #[cfg(any(debug_assertions, target_os = "macos", windows))]
        Some("--service-run") => Ok(Mode::ServiceRun {
            data_dir: service_dir(&args[1..], account)?,
        }),
        _ => Err(Usage("no mode of this build").into()),
    }
}

/// The answer of `--default-data-dir`: the path of `data_dir` and a newline
/// on `out`, exit 0. A path that holds a control character is no one line, so
/// it is refused on `err` with exit 1, as an account that names no directory.
#[cfg(any(debug_assertions, target_os = "macos", windows))]
pub fn default_data_dir_answer(
    data_dir: &halvo_control::paths::DataDir,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let text = data_dir.path().to_string_lossy();
    if text.chars().any(char::is_control) {
        let _ = writeln!(err, "{NO_DATA_DIR}: the path holds a control character");
        return halvo_control::exit::ERROR;
    }
    match writeln!(out, "{text}").and_then(|()| out.flush()) {
        Ok(()) => halvo_control::exit::CLEAN,
        Err(error) => {
            let _ = writeln!(err, "halvo-capture: the path was not written: {error}");
            halvo_control::exit::ERROR
        }
    }
}

#[cfg(any(debug_assertions, target_os = "macos", windows))]
fn service_dir(
    args: &[String],
    account: &dyn AccountLookup,
) -> Result<halvo_control::paths::DataDir, Refusal> {
    use halvo_control::paths::DataDir;
    match args {
        [] => DataDir::for_account(account).map_err(Refusal::NoServiceDir),
        [flag] if flag == "--data-dir" => Err(Usage("--data-dir needs DIR").into()),
        [flag, dir] if flag == "--data-dir" => DataDir::from_flag(dir.as_str())
            .map_err(|_| Usage("--data-dir takes an absolute directory").into()),
        _ => Err(Usage("--service-run takes --data-dir DIR and no other flag").into()),
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
        if cfg!(target_os = "windows") && flags.switches.0.contains(&(Switch::KeyClass, true)) {
            return Err(Usage(
                "--switch key_class=on is refused on Windows: this build starts no keyboard source",
            )
            .into());
        }
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
    #[cfg(any(test, target_os = "macos", windows))]
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
    #[cfg(any(test, target_os = "macos", windows))]
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
#[cfg(any(test, target_os = "macos", windows))]
pub trait Waits {
    /// The time since a fixed start.
    fn clock(&self) -> Duration;

    /// The answer of `pending` if it comes within `bound`.
    fn wait_answer(&mut self, pending: &owner::Pending, bound: Duration) -> Option<owner::Answer>;

    /// Lets `wait` pass.
    fn pause_for(&mut self, wait: Duration);
}

/// The process's own clock and blocking waits.
#[cfg(any(test, target_os = "macos", windows))]
pub struct RealWaits;

#[cfg(any(test, target_os = "macos", windows))]
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

#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
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
#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
pub const STOP_PATH_MARGIN_MS: u64 = 1_000;

/// What is left of the stop path's budget, SHUTDOWN_BUDGET_MS less
/// [`STOP_PATH_MARGIN_MS`] from `start`.
#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
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
    run_timed_on(
        halvo_control::proto::Platform::Macos,
        run,
        sources,
        store,
        clocks,
        stop,
        settle_bound,
    )
}

/// [`run_timed`] with the sources that a timed run starts on `platform`, by
/// the names of that system: macOS's for the macOS run and every test, and
/// Windows's for the Windows run.
#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
pub fn run_timed_on<P, S, C>(
    platform: halvo_control::proto::Platform,
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
    let answer = run_timed_steps(platform, run, sources, store, clocks, stop, settle_bound);
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

#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
fn run_timed_steps<P, S, C>(
    platform: halvo_control::proto::Platform,
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
            cap: Some(port::ByteBound::of_cap(StorageCap::DEFAULT)),
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
    let wanted = sources::starting(platform, run.kind(), switches);
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
    let unreceived = emitted.saturating_sub(before.events_received());
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
        let loss = with_the_drain_short(loss, unreceived, &settled, &before, &reports);
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
    let loss = with_the_drain_short(loss, unreceived, &settled, &before, &reports);
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
#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
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
#[cfg(any(test, target_os = "macos", windows))]
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

#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
fn answer_within(
    waits: &mut dyn Waits,
    handle: &owner::OwnerHandle,
    command: owner::Command,
    bound: Duration,
) -> Option<owner::Answer> {
    let pending = handle.ask(command).ok()?;
    waits.wait_answer(&pending, bound)
}

#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
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

#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
/// For a stop that got no answer: an upper bound of what the life did not
/// store, from a snapshot read after the last wait and the sink's counts read
/// after the sources stopped: lost, pending, and accepted less received over
/// the kinds of `Modality::ALL`.
#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
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

#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
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

#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
fn with_the_drain_short(
    loss: Option<Loss>,
    unreceived: u64,
    settled: &counters::Snapshot,
    before: &counters::Snapshot,
    reports: &[owner::Report],
) -> Option<Loss> {
    let short = unreceived.min(
        settled
            .discarded_by_barrier
            .saturating_sub(before.discarded_by_barrier),
    );
    let cause = loss.as_ref().and_then(|loss| loss.cause);
    match cause {
        _ if short == 0 => loss,
        Some(
            LossCause::PauseUnanswered
            | LossCause::SettleUnanswered
            | LossCause::ShutdownUnanswered
            | LossCause::OwnerEnded,
        ) => loss,
        Some(
            LossCause::Pending
            | LossCause::LostSinceAcknowledged
            | LossCause::LostDuringStop
            | LossCause::DrainWaitRanOut,
        )
        | None => Some(Loss {
            cause: Some(cause.unwrap_or(LossCause::DrainWaitRanOut)),
            line: loss_line(
                settled,
                settled.events_lost + settled.events_pending + short,
                reports,
            ),
        }),
    }
}

#[cfg(any(test, target_os = "macos", windows))]
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

#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
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

/// Runs the service form of the run-loop self-check on `dir`: the service run
/// of [`service_run`], writing its state file through `writer`, in its wait
/// on the main run loop, while a second thread, once the state file stands,
/// queues a block on the main run loop and ends the wait when the block ran or
/// after [`main_loop::SELF_CHECK_WINDOW`]. It prints the self-check's line on
/// `out` and answers the service run's status when that is not 0, else the
/// self-check's.
#[cfg(debug_assertions)]
pub fn self_check_service_run(
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    out: &mut dyn std::io::Write,
) -> i32 {
    #[cfg(target_os = "macos")]
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex, PoisonError};
        use std::time::Instant;

        let main_loop = match crate::macos::run_loop::MainRunLoop::claim() {
            Ok(run_loop) => ServiceLoop {
                run_loop,
                started: Instant::now(),
            },
            Err(_) => return halvo_control::exit::ERROR,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let ran_after: Arc<Mutex<Option<Duration>>> = Arc::default();
        let queue = {
            let stop = stop.clone();
            let ran_after = ran_after.clone();
            let state_file = dir.state_file();
            std::thread::spawn(move || {
                let waiting = Instant::now();
                while !state_file.exists() && waiting.elapsed() < main_loop::SELF_CHECK_WINDOW {
                    std::thread::sleep(Duration::from_millis(10));
                }
                std::thread::sleep(Duration::from_millis(100));
                let queued = Instant::now();
                let ran = ran_after.clone();
                let ended = stop.clone();
                on_the_main_run_loop(move || {
                    *ran.lock().unwrap_or_else(PoisonError::into_inner) = Some(queued.elapsed());
                    ended.store(true, Ordering::SeqCst);
                });
                while !stop.load(Ordering::SeqCst)
                    && queued.elapsed() < main_loop::SELF_CHECK_WINDOW
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                stop.store(true, Ordering::SeqCst);
            })
        };
        let status = serve(
            dir,
            writer,
            main_loop,
            &stop,
            Arc::new(main_loop::ExitNow),
            &mut std::io::stderr(),
        );
        let _ = queue.join();
        let ran_after = *ran_after.lock().unwrap_or_else(PoisonError::into_inner);
        let (line, checked) = main_loop::self_check_answer(ran_after);
        if writeln!(out, "{line}").is_err() {
            return halvo_control::exit::ERROR;
        }
        if status == halvo_control::exit::CLEAN {
            checked
        } else {
            status
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (dir, writer, out);
        halvo_control::exit::ERROR
    }
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn on_the_main_run_loop(work: impl Fn() + 'static) {
    use core_foundation::base::TCFType;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop, CFRunLoopRef};

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRunLoopPerformBlock(
            run_loop: CFRunLoopRef,
            mode: *const std::ffi::c_void,
            block: &block2::Block<dyn Fn()>,
        );
        fn CFRunLoopWakeUp(run_loop: CFRunLoopRef);
    }

    let block = block2::RcBlock::new(work);
    let main = CFRunLoop::get_main();
    unsafe {
        CFRunLoopPerformBlock(
            main.as_concrete_TypeRef(),
            kCFRunLoopCommonModes.cast(),
            &block,
        );
        CFRunLoopWakeUp(main.as_concrete_TypeRef());
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
        crate::windows::timed_run(run)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = run;
        Err(DaemonError::NotImplemented("timed-run on this platform"))
    }
}

#[cfg(any(test, target_os = "macos", windows))]
impl<'a> port::SegmentWrite<'a> {
    fn new(
        segment_id: &'a port::SegmentId,
        day: halvo_control::model::UtcDay,
        first_capture: i64,
        last_capture: i64,
        entries: &'a [port::KindEntry],
        payload: &'a [u8],
    ) -> port::SegmentWrite<'a> {
        port::SegmentWrite {
            segment_id,
            day,
            first_capture,
            last_capture,
            entries,
            payload,
        }
    }
}

/// The scratch store of a timed run in its store directory, made through the
/// adapter with its root file under `roots` in that directory, and the guard
/// of that root, held from right after the create (contracts.md C4 item 5,
/// C20 item 7).
#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
pub fn scratch_store(run: &TimedRun) -> Result<port::ScratchStore, DaemonError> {
    port::ScratchDir::at(&run.store_dir)
        .create()
        .map_err(|fault| DaemonError::Store(format!("the scratch store was not made: {fault:?}")))
}

/// The answer of a timed run on a scratch store whose root `root` guards:
/// the root is destroyed now, and a destroy that fails makes the status 1,
/// with a line on standard error. After a panic the guard's drop destroys
/// the root.
#[cfg(any(
    test,
    all(target_os = "macos", debug_assertions),
    all(windows, debug_assertions)
))]
pub fn end_of_a_scratch_run(
    answer: Result<RunOutcome, DaemonError>,
    root: port::RootGuard,
) -> Result<RunOutcome, DaemonError> {
    use std::io::Write as _;
    match (answer, root.finish()) {
        (Ok(outcome), Err(error)) => {
            let _ = writeln!(
                std::io::stderr(),
                "halvo-capture: the root of the scratch store was not destroyed: {}",
                error.kind()
            );
            Ok(RunOutcome {
                status: halvo_control::exit::ERROR,
                ..outcome
            })
        }
        (answer, _) => answer,
    }
}

/// The writer of the state file through the store's durable-write helper
/// (contracts.md C3.7). The adapter of the store module gives it its
/// constructor.
pub struct DurableState;

/// Runs `--service-run` on `dir` (contracts.md C3.2) on the process main
/// thread's run loop until SIGINT or SIGTERM, writing the state file through
/// `writer`, and answers its exit status: 0 after a clean stop or when another
/// process holds the data directory, 1 when the directory cannot be used.
/// After the instance lock and the state file, the owner opens the store
/// under the data directory through the adapter, and creates it on a first
/// run; a SIGTERM closes it. This cut starts no source and binds no socket.
/// A Windows build serves the same run over the named pipe of `dir`
/// with the sources of [`crate::windows::sources::ServiceSet`]; there a Stop
/// request or a message of its session window ends it.
#[cfg(any(debug_assertions, target_os = "macos", windows))]
pub fn service_run(dir: &halvo_control::paths::DataDir, writer: Box<dyn state::StateWrite>) -> i32 {
    #[cfg(target_os = "macos")]
    {
        serve_the_mac(dir, writer, None, Behind::Unsaid)
    }
    #[cfg(windows)]
    {
        crate::windows::start_a_run(|| serve_windows(dir, writer, None, Behind::Unsaid))
            .unwrap_or_else(refused_start)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        use std::io::Write as _;
        let _ = (dir, writer);
        let refused = DaemonError::NotImplemented("the service run on this system");
        let _ = writeln!(std::io::stderr(), "halvo-capture: {refused}");
        halvo_control::exit::ERROR
    }
}

/// Runs `--service-run --lifeline` on `dir` (contracts.md C3.2 item 4, C14
/// item 2): a standard input that is no pipe exits 64 and a parent that is
/// gone already exits 0, each before the store opens; then the run of
/// [`service_run`], which also ends through its one shutdown entry once the
/// lifeline reaches its end of file or its parent is gone, with 0, or 1 when
/// the final flush failed. The parent's pid is the exclusion root of the
/// floor. On Windows the start opens the parent by its pid and the watch
/// waits on that handle. With `behind` [`Behind::NoPress`] the run first
/// reads why the last life of `dir` ended, as that variant says.
#[cfg(any(debug_assertions, target_os = "macos", windows))]
pub fn lifeline_service_run(
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    behind: Behind,
) -> i32 {
    #[cfg(target_os = "macos")]
    {
        match lease::lifeline::start_or_status(&mut std::io::stderr()) {
            Ok(lifeline) => serve_the_mac(dir, writer, Some(lifeline), behind),
            Err(status) => status,
        }
    }
    #[cfg(windows)]
    {
        crate::windows::start_a_run(|| {
            match lease::lifeline::start_or_status(&mut std::io::stderr()) {
                Ok(lifeline) => serve_windows(dir, writer, Some(lifeline), behind),
                Err(status) => status,
            }
        })
        .unwrap_or_else(refused_start)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        use std::io::Write as _;
        let _ = (dir, writer, behind);
        let refused = DaemonError::NotImplemented("the lifeline mode on this system");
        let _ = writeln!(std::io::stderr(), "halvo-capture: {refused}");
        halvo_control::exit::ERROR
    }
}

/// Runs `ctl` (contracts.md C2.2 to C2.7): the request of its verb to the
/// service run of its data directory through halvo-control's client, the
/// text of the answer on `out` and `err`, and the exit status of C2.7.
#[cfg(any(debug_assertions, target_os = "macos", windows))]
pub fn ctl(
    ctl: &halvo_control::ctl_text::Ctl,
    out: &mut dyn std::io::Write,
    err: &mut dyn std::io::Write,
) -> i32 {
    let printed = match ctl.data_dir() {
        Ok(dir) => {
            let result = halvo_control::client::ctl_connector(&dir).and_then(|connector| {
                halvo_control::client::Client::new(connector).send(&ctl.verb.request())
            });
            halvo_control::ctl_text::print(&ctl.verb, result, &dir)
        }
        Err(error) => halvo_control::ctl_text::Printed {
            stdout: String::new(),
            stderr: format!("halvo-capture: the account gives no data directory: {error}\n"),
            status: halvo_control::exit::ERROR,
        },
    };
    let _ = out
        .write_all(printed.stdout.as_bytes())
        .and_then(|()| out.flush());
    let _ = err
        .write_all(printed.stderr.as_bytes())
        .and_then(|()| err.flush());
    printed.status
}

/// The switch of the feature test-seam: `--scripted-sources` after the
/// flags of `--service-run` gives the service run scripted sources in place
/// of the system's, so that a test of the built binary starts no real source.
/// A build without the feature has none of it and refuses the flag as usage.
#[cfg(all(debug_assertions, feature = "test-seam"))]
pub mod seam {
    /// The flag of the switch.
    pub const FLAG: &str = "--scripted-sources";

    /// The marker of a build with the seam, the bytes that check 62-build
    /// scans every shipped file for.
    pub static MARKER: [u8; 42] = *b"halvo-capture:test-seam:this-build-has-it!";

    /// The first line on standard error of a service run with scripted
    /// sources.
    pub const FIRST_LINE: &str =
        "halvo-capture: sources=scripted: this build has the feature test-seam";

    /// The app that each scripted foreground event names.
    pub const MARK_APP: &str = "dev.halvo.test-seam.scripted-front";

    /// The foreground events that a scripted source emits at each start.
    pub const EVENTS_A_START: u64 = 3;

    /// The factory of the scripted sources: a foreground source that emits
    /// [`EVENTS_A_START`] events naming [`MARK_APP`] at each start, and no
    /// other source.
    #[cfg(target_os = "macos")]
    pub struct ScriptedSources;

    #[cfg(target_os = "macos")]
    impl super::sources::SourceFactory for ScriptedSources {
        fn make(
            &mut self,
            name: super::SourceName,
        ) -> Option<Box<dyn crate::source::CaptureSource>> {
            (name == super::SourceName::Foreground)
                .then(|| Box::new(ScriptedFront) as Box<dyn crate::source::CaptureSource>)
        }
    }

    #[cfg(target_os = "macos")]
    impl super::supervisor::SystemSources for ScriptedSources {}

    #[cfg(target_os = "macos")]
    struct ScriptedFront;

    #[cfg(target_os = "macos")]
    impl crate::source::CaptureSource for ScriptedFront {
        fn begin(
            &mut self,
            ctx: crate::source::SourceContext,
        ) -> Result<crate::source::Started, crate::source::CaptureError> {
            use crate::floor::exclude::{PrivateWindow, ProcessId, Subject, Verdict};
            let probe = crate::secure_input_macos::SystemProbe::new();
            for _ in 0..EVENTS_A_START {
                let subject = Subject::new(
                    crate::event::AppIdentity::bundle_id(MARK_APP.to_owned()),
                    ProcessId::new(1),
                );
                let verdict = ctx.floor().decide::<crate::floor::kind::ForegroundApp>(
                    subject,
                    PrivateWindow::Unknown,
                    &probe,
                );
                if let Verdict::Keep(keep) = verdict {
                    ctx.sink()
                        .emit_event(crate::event::FloorEvent::foreground_app(
                            keep,
                            None,
                            crate::event::EventClock::system(),
                        ));
                }
            }
            Ok(crate::source::Started::Running)
        }

        fn shutdown(&mut self) -> Result<(), crate::source::CaptureError> {
            Ok(())
        }
    }

    /// Runs the service run on `dir` with [`ScriptedSources`], after
    /// [`FIRST_LINE`] on standard error; on another system the scripted
    /// sources are not built, and it says so on standard error and exits 1
    /// with no run.
    pub fn scripted_service_run(
        dir: &halvo_control::paths::DataDir,
        writer: Box<dyn super::state::StateWrite>,
    ) -> i32 {
        use std::io::Write as _;
        std::hint::black_box(&MARKER);
        #[cfg(target_os = "macos")]
        {
            let _ = writeln!(std::io::stderr(), "{FIRST_LINE}");
            super::serve_the_mac_with(dir, writer, None, super::Behind::Unsaid, |_| {
                scripted_parts()
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (dir, writer);
            let _ = writeln!(
                std::io::stderr(),
                "halvo-capture: the scripted sources are not built on this system"
            );
            halvo_control::exit::ERROR
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn scripted_parts() -> super::ServiceParts<ScriptedSources> {
        super::ServiceParts::of(ScriptedSources)
    }
}

#[cfg(target_os = "macos")]
fn serve_the_mac(
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    lifeline: Option<lease::lifeline::Lifeline>,
    behind: Behind,
) -> i32 {
    serve_the_mac_with(dir, writer, lifeline, behind, |(writer, reader, turns)| {
        system_parts(sources::ServiceSources::new(writer, reader, turns))
    })
}

#[cfg(target_os = "macos")]
fn system_parts<F: supervisor::SystemSources>(factory: F) -> ServiceParts<F> {
    let dir = children::own_dir().ok();
    let probe: Box<dyn permissions::ProbeRunner> = match dir.clone() {
        Some(dir) => Box::new(permissions::ChildProbe::new(dir, permissions::PROGRAM)),
        None => Box::new(permissions::Unanswered),
    };
    ServiceParts {
        probe: Some(probe),
        requests: dir.map(|dir| {
            Box::new(permissions::ChildRequests::new(dir)) as Box<dyn permissions::RequestStarter>
        }),
        ..ServiceParts::of(factory)
    }
}

#[cfg(target_os = "macos")]
type FrontParts = (
    crate::macos::foreground::ForegroundWriter,
    crate::macos::foreground::ForegroundReader,
    crate::macos::run_loop::RunLoopTurns,
);

#[cfg(target_os = "macos")]
fn serve_the_mac_with<F: supervisor::SystemSources>(
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    lifeline: Option<lease::lifeline::Lifeline>,
    behind: Behind,
    parts_of: impl FnOnce(FrontParts) -> ServiceParts<F>,
) -> i32 {
    use std::io::Write as _;
    let mut run_loop = match crate::macos::run_loop::MainRunLoop::claim() {
        Ok(run_loop) => run_loop,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "halvo-capture: {error:?}");
            return halvo_control::exit::ERROR;
        }
    };
    let Some((front_writer, front_reader)) = run_loop.foreground_state() else {
        let _ = writeln!(
            std::io::stderr(),
            "halvo-capture: the foreground state of the run loop was taken"
        );
        return halvo_control::exit::ERROR;
    };
    let parts = parts_of((front_writer, front_reader, run_loop.turns()));
    let main_loop = ServiceLoop {
        run_loop,
        started: std::time::Instant::now(),
    };
    serve_on(
        dir,
        writer,
        main_loop,
        signal::flag(),
        std::sync::Arc::new(main_loop::ExitNow),
        &mut std::io::stderr(),
        ServiceParts {
            lifeline,
            behind,
            ..parts
        },
    )
}

#[cfg(target_os = "macos")]
struct ServiceLoop {
    run_loop: crate::macos::run_loop::MainRunLoop,
    started: std::time::Instant,
}

#[cfg(target_os = "macos")]
impl main_loop::MainLoop for ServiceLoop {
    fn turn(&mut self, wait: Duration) {
        self.run_loop.turn_for(wait);
    }

    fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

/// The answer of a Windows run whose first step failed: its line on standard
/// error and the exit status of its own, 6, before any read of the key or of
/// the store.
#[cfg(windows)]
fn refused_start(refused: crate::windows::FaultReportsOn) -> i32 {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "halvo-capture: {refused}");
    halvo_control::exit::FAULT_REPORTS_ON
}

/// The service run on Windows: the session window first, whose messages of
/// a session's end and WM_CLOSE set the stop flag, then the run of `dir`,
/// which ends through its one shutdown entry once that flag reads true, the
/// lifeline reads ended or a Stop request comes. A session window that does
/// not start is a line on standard error, and the run goes on without it.
#[cfg(windows)]
fn serve_windows(
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    lifeline: Option<Lifeline>,
    behind: Behind,
) -> i32 {
    use std::io::Write as _;
    if let Err(error) = crate::windows::session::start() {
        let _ = writeln!(
            std::io::stderr(),
            "halvo-capture: the session window did not start: {}",
            error.kind()
        );
    }
    serve_on(
        dir,
        writer,
        WindowsLoop::new(),
        signal::flag(),
        std::sync::Arc::new(main_loop::ExitNow),
        &mut std::io::stderr(),
        ServiceParts {
            lifeline,
            behind,
            ..ServiceParts::of(crate::windows::sources::ServiceSet::new())
        },
    )
}

/// The main loop of a Windows run: the thread sleeps, because every source
/// and the session window pump their messages on threads of their own.
#[cfg(windows)]
pub(crate) struct WindowsLoop {
    started: std::time::Instant,
}

#[cfg(windows)]
impl WindowsLoop {
    pub(crate) fn new() -> WindowsLoop {
        WindowsLoop {
            started: std::time::Instant::now(),
        }
    }
}

#[cfg(windows)]
impl main_loop::MainLoop for WindowsLoop {
    fn turn(&mut self, wait: Duration) {
        std::thread::sleep(wait);
    }

    fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

#[cfg(windows)]
impl supervisor::SystemSources for crate::windows::sources::ServiceSet {}

#[cfg(any(target_os = "macos", windows))]
pub(crate) type Lifeline = lease::lifeline::Lifeline;

#[cfg(all(target_os = "macos", debug_assertions))]
struct NoSources;

#[cfg(all(target_os = "macos", debug_assertions))]
impl sources::SourceFactory for NoSources {
    fn make(&mut self, _name: SourceName) -> Option<Box<dyn crate::source::CaptureSource>> {
        None
    }
}

#[cfg(all(target_os = "macos", debug_assertions))]
impl supervisor::SystemSources for NoSources {}

#[cfg(any(target_os = "macos", windows))]
const SERVICE_WAIT: Duration = Duration::from_secs(60);

/// The service run of contracts.md C3.2 up to its wait, on the data directory
/// `dir`, with the loop `main_loop`, the stop flag `stop` and the ending
/// `ending`; its lines go to `err`. Step 2: an entry at the directory's path
/// that leads nowhere or is no directory exits 1, and a missing directory is
/// made with mode 0700. Step 3: the instance lock; when another process holds
/// it, the run answers 0 and writes and prints nothing; then the check of the
/// directory on the lock's descriptor, whose fault is a line and never ends
/// the run. Step 7: the state file in the folder that the lock holds, loaded
/// or written with the defaults, then the backup mark through the lock's
/// descriptor, whose failure is a line. Then the supervisor holds the state
/// and waits, paused and with no source, until `stop` reads true; the shutdown
/// entry, with no owner, gives the status.
#[cfg(all(target_os = "macos", debug_assertions))]
pub(crate) fn serve<L: main_loop::MainLoop>(
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    main_loop: L,
    stop: &std::sync::atomic::AtomicBool,
    ending: std::sync::Arc<dyn main_loop::EndCall>,
    err: &mut dyn std::io::Write,
) -> i32 {
    let parts = ServiceParts::of(NoSources);
    serve_on(dir, writer, main_loop, stop, ending, err, parts)
}

/// What a service run serves beside its data directory: the factory of its
/// sources, which start only while the run condition holds, the lifeline of
/// a run started with `--lifeline` and what its starter says of a press
/// behind it (`--no-press`), the probe whose answers the supervisor reads,
/// and in a test build the lease that the run starts with and a call that
/// gets the last settle of its shutdown entry.
#[cfg(any(target_os = "macos", windows))]
pub(crate) struct ServiceParts<F> {
    pub(crate) factory: F,
    pub(crate) lifeline: Option<Lifeline>,
    pub(crate) behind: Behind,
    pub(crate) probe: Option<Box<dyn permissions::ProbeRunner>>,
    pub(crate) requests: Option<Box<dyn permissions::RequestStarter>>,
    #[cfg(test)]
    pub(crate) typing: bool,
    #[cfg(test)]
    pub(crate) lease: lease::Lease,
    #[cfg(test)]
    pub(crate) settled: Option<Box<dyn FnOnce(Option<counters::Snapshot>)>>,
    #[cfg(test)]
    pub(crate) wrap_store: Option<WrapStore>,
}

#[cfg(all(test, any(target_os = "macos", windows)))]
pub(crate) type WrapStore =
    Box<dyn FnOnce(Box<dyn port::CaptureStore + Send>) -> Box<dyn port::CaptureStore + Send>>;

#[cfg(any(target_os = "macos", windows))]
impl<F: supervisor::SystemSources> ServiceParts<F> {
    pub(crate) fn of(factory: F) -> ServiceParts<F> {
        ServiceParts {
            factory,
            lifeline: None,
            behind: Behind::Unsaid,
            probe: None,
            requests: None,
            #[cfg(test)]
            typing: server::TYPING == Some(true),
            #[cfg(test)]
            lease: lease::Lease::NONE,
            #[cfg(test)]
            settled: None,
            #[cfg(test)]
            wrap_store: None,
        }
    }
}

/// [`serve`] with the sources, the lifeline and the lease of `parts`.
#[cfg(any(target_os = "macos", windows))]
pub(crate) fn serve_on<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    main_loop: L,
    stop: &std::sync::atomic::AtomicBool,
    ending: std::sync::Arc<dyn main_loop::EndCall>,
    err: &mut dyn std::io::Write,
    parts: ServiceParts<F>,
) -> i32 {
    let lock = match take_the_data_dir(dir, err) {
        Ok(Some(lock)) => lock,
        Ok(None) => return halvo_control::exit::CLEAN,
        Err(status) => return status,
    };
    serve_held_on(lock, dir, writer, main_loop, stop, ending, err, parts)
}

/// The service run after its take, on the data directory `dir` that `lock`
/// holds: the check of the directory on the lock's descriptor, the state file
/// in the folder that the lock holds, the backup mark, the wait and the
/// shutdown entry, as [`serve`] describes them.
#[cfg(all(test, target_os = "macos", debug_assertions))]
pub(crate) fn serve_held<L: main_loop::MainLoop>(
    lock: instance::InstanceLock,
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    main_loop: L,
    stop: &std::sync::atomic::AtomicBool,
    ending: std::sync::Arc<dyn main_loop::EndCall>,
    err: &mut dyn std::io::Write,
) -> i32 {
    let parts = ServiceParts::of(NoSources);
    serve_held_on(lock, dir, writer, main_loop, stop, ending, err, parts)
}

/// [`serve_held`] with the sources, the lifeline and the lease of `parts`:
/// once the store is open for capture, the supervisor serves the sources of
/// [`sources::starting`] for the switches of the state file, each begun on
/// the owner's sink and floor. With [`Behind::NoPress`] it first reads why
/// the last life ended and ends with WAITS_FOR_A_PRESS where the table of
/// `run_marker::LastEnd` says so, before the marker's begin. Where it accepts
/// a Stop request, or reads that a signal set the stop flag, it writes the
/// decision of that stop in the run marker first, the line beside its process
/// line (`run_marker::decided`); a life that ends with 0 then leaves the line
/// of the trigger of its end alone, and an end with 1 leaves the decision,
/// before it answers a Stop request and before it gives the lock up
/// (`run_marker::at_the_end`).
#[cfg(any(target_os = "macos", windows))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn serve_held_on<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    lock: instance::InstanceLock,
    dir: &halvo_control::paths::DataDir,
    writer: Box<dyn state::StateWrite>,
    main_loop: L,
    stop: &std::sync::atomic::AtomicBool,
    ending: std::sync::Arc<dyn main_loop::EndCall>,
    err: &mut dyn std::io::Write,
    parts: ServiceParts<F>,
) -> i32 {
    let mut dir_shut = match lock.check(dir) {
        Ok(_) => None,
        Err(fault) => {
            let _ = writeln!(err, "{}", obstructed_line(fault));
            Some(DirShut::first())
        }
    };
    if parts.behind == Behind::NoPress
        && run_marker::last_end(dir).no_press() == run_marker::NoPress::WaitsForAPress
    {
        let _ = writeln!(err, "{WAITS_FOR_A_PRESS_LINE}");
        return halvo_control::exit::WAITS_FOR_A_PRESS;
    }
    let begun = run_marker::begin(dir);
    let last_stop = begun.last_stop;
    if let Err(error) = &begun.written {
        let _ = writeln!(
            err,
            "halvo-capture: the run marker was not written: {}",
            error.kind()
        );
    }
    let listening = match server::Listening::bind(dir) {
        Ok(listening) => listening,
        Err(error) => {
            let _ = writeln!(
                err,
                "halvo-capture: the control channel could not be bound: {}",
                error.kind()
            );
            let _ = run_marker::give_back(dir, &begun);
            return halvo_control::exit::ERROR;
        }
    };
    let facts = server::RunFacts {
        started_at: second_now(),
        parent: if parts.lifeline.is_some() {
            halvo_control::proto::Parent::Watched
        } else {
            halvo_control::proto::Parent::None
        },
        last_stop,
        rss_peak_bytes: 0,
    };
    let folder = match lock.state_dir() {
        Ok(folder) => folder,
        Err(error) => {
            let _ = writeln!(
                err,
                "halvo-capture: the folder of the data directory could not be read on its lock: {}",
                error.kind()
            );
            return halvo_control::exit::ERROR;
        }
    };
    let state = state::StateFile::open(folder, writer);
    if let Some(line) = state_line(state.health()) {
        let _ = writeln!(err, "{line}");
    }
    let mut rule = owner::open::CreateRule::of(state.health(), state.state().store_made);
    let held_cap = cap_bound(state.state().storage_cap);
    let switches = state.state().switches;
    if let Err(error) = lock.exclude_from_backup() {
        let _ = writeln!(
            err,
            "halvo-capture: backup_exclusion=not_set error={}",
            error.kind()
        );
    }
    let root = parts
        .lifeline
        .as_ref()
        .map(|lifeline| crate::floor::exclude::ProcessId::new(lifeline.parent()));
    let mut supervisor = supervisor::Supervisor::new(
        halvo_control::proto::Platform::current(),
        parts.factory,
        main_loop,
    )
    .with_ending(ending)
    .with_state(state)
    .with_marker(Box::new(supervisor::MarkerFile::new(dir.run_marker())))
    .with_hold(run_marker::held(dir))
    .with_life(listening.admitting().ending_life());
    listening
        .admitting()
        .ending_life()
        .serve_from(supervisor.shared_state(), facts);
    if let Some(lifeline) = parts.lifeline {
        supervisor = supervisor.with_parent(Box::new(lifeline));
    }
    if let Some(probe) = parts.probe {
        supervisor = supervisor.with_probe(probe);
    }
    if let Some(requests) = parts.requests {
        supervisor = supervisor.with_requests(requests);
    }
    #[cfg(test)]
    {
        supervisor = supervisor.with_typing(parts.typing);
    }
    #[cfg(test)]
    supervisor.set_lease(parts.lease, None);
    if dir_shut.is_some() {
        let _ = writeln!(err, "{DIR_SHUT_LINE}");
    }
    let mut opener = if stop.load(std::sync::atomic::Ordering::SeqCst) {
        None
    } else if dir_shut.is_some() {
        finish_the_intent_alone(dir, err)
    } else {
        start_the_opener(dir, rule, err)
    };
    let mut store = HeldStore::None;
    let mut mark = if rule == owner::open::CreateRule::StoreMade {
        Mark::Settled
    } else {
        Mark::NotYet
    };
    let mut status = halvo_control::exit::CLEAN;
    let mut ended_by = None;
    let mut stop_asked = None;
    let mut deleted = None;
    let mut delete_ends = false;
    let mut delete_written = None;
    let mut reset_written = None;
    #[cfg(test)]
    let mut wrap_store = parts.wrap_store;
    loop {
        if delete_ends && deleted.is_none() {
            ended_by = Some(halvo_control::proto::StopKind::DeleteAll);
            break;
        }
        let opening = matches!(store, HeldStore::None)
            && opener.as_ref().is_some_and(owner::open::Opener::is_running);
        let window = match (dir_shut, opening, mark) {
            (Some(shut), _, _) => shut.left().clamp(owner::POLL, SERVICE_WAIT),
            (None, true, _) => owner::POLL,
            (None, false, Mark::Pending { .. }) => MARK_RETRY,
            (None, false, Mark::NotYet | Mark::Settled) => SERVICE_WAIT,
        };
        let window = match &store {
            HeldStore::Maintenance(upkeep) => window.min(upkeep.wait()),
            HeldStore::None | HeldStore::Capture(_) => window,
        };
        let serving = Serving {
            listening: &listening,
            facts: &facts,
            obstructed: dir_shut.map(|shut| shut.left()),
        };
        match wait_serving(&mut supervisor, &serving, window, stop, &store) {
            Waited::Window => {}
            Waited::Ended => {
                let trigger = trigger_of_the_wait(stop);
                if trigger == halvo_control::proto::StopKind::Signal {
                    decide_the_stop(dir, trigger, err);
                }
                ended_by = Some(trigger);
                break;
            }
            Waited::Stop(asked) => {
                listening.admitting().end_the_life();
                decide_the_stop(dir, halvo_control::proto::StopKind::Stop, err);
                ended_by = Some(halvo_control::proto::StopKind::Stop);
                stop_asked = Some(asked);
                break;
            }
            Waited::Reset(asked) => {
                if let Some(written) = reset_on_request(asked, &mut supervisor, &serving) {
                    reset_written = Some(written);
                    status = halvo_control::exit::RESTART;
                    break;
                }
            }
            Waited::Delete(asked) => {
                match delete_on_request(
                    asked,
                    &mut supervisor,
                    &serving,
                    (&mut store, &mut opener),
                    dir,
                    err,
                ) {
                    DeleteAsked::Answered => {}
                    DeleteAsked::Ending(written) => {
                        delete_written = Some(written);
                        ended_by = Some(halvo_control::proto::StopKind::DeleteAll);
                        break;
                    }
                    DeleteAsked::Reopen => {
                        mark = Mark::NotYet;
                        let health = supervisor.status_parts(None).health;
                        supervisor.set_slot(slot::Slot::Opening, None);
                        opener = if dir_shut.is_none() {
                            start_the_opener(dir, owner::open::CreateRule::of(health, true), err)
                        } else {
                            None
                        };
                    }
                    DeleteAsked::Ran {
                        asked,
                        report,
                        whole,
                        ending,
                    } => {
                        mark = Mark::NotYet;
                        opener = None;
                        delete_ends = ending;
                        if report.is_none() || !whole {
                            supervisor.set_slot(slot::Slot::DeleteUnfinished, None);
                        }
                        match report {
                            None => {
                                delete_written = Some(asked.answer(halvo_control::proto::Reply::Error(
                                    halvo_control::proto::ErrorReply::new(
                                        halvo_control::proto::ErrorCode::Internal,
                                        "the delete did not finish and its report could not be read; delete-all.intent stays",
                                    ),
                                )));
                            }
                            Some(report) if !whole => {
                                delete_written =
                                    Some(asked.answer(halvo_control::proto::Reply::Deleted(
                                        halvo_control::proto::Deleted {
                                            report,
                                            store: None,
                                            unfinished: true,
                                        },
                                    )));
                            }
                            Some(report) => {
                                rule = owner::open::CreateRule::of(
                                    supervisor.status_parts(None).health,
                                    false,
                                );
                                supervisor.set_slot(slot::Slot::Opening, None);
                                if dir_shut.is_none() {
                                    opener = start_the_opener(dir, rule, err);
                                }
                                deleted = Some((asked, report));
                                if opener.is_none() {
                                    delete_written = answer_the_delete(&mut deleted, None);
                                }
                            }
                        }
                    }
                }
            }
        }
        if RunSources::restart_asked(&supervisor) {
            status = halvo_control::exit::RESTART;
            break;
        }
        if store.link().is_some_and(|owner| !owner.is_running()) {
            status = halvo_control::exit::ERROR;
            break;
        }
        if let Some(shut) = dir_shut {
            if shut.left().is_zero() {
                let finishing = opener.as_ref().is_some_and(owner::open::Opener::is_running);
                dir_shut = match lock.check(dir) {
                    Ok(_) if !finishing => {
                        opener = open_after_the_check(&mut supervisor, || {
                            start_the_opener(dir, rule, err)
                        });
                        None
                    }
                    Ok(_) | Err(_) => Some(shut.again()),
                };
            }
            continue;
        }
        say_the_notes(opener.as_ref(), err);
        if let (HeldStore::None, Some(opener)) = (&store, &opener) {
            let read = opener.slot();
            if *supervisor.slot() != read {
                supervisor.set_slot(read, None);
            }
        }
        mark = mark_the_store_made(mark, &mut supervisor, err);
        if let HeldStore::Maintenance(upkeep) = &mut store {
            upkeep.turn();
        }
        let Some(held) = opener.as_ref().and_then(owner::open::Opener::take) else {
            if opener.as_ref().is_some_and(made_none) {
                delete_written = answer_the_delete(&mut deleted, None).or(delete_written);
            }
            continue;
        };
        mark = mark_the_store_made(mark.at_the_answer(), &mut supervisor, err);
        let made = match &held {
            owner::open::Held::Capture { id, .. } | owner::open::Held::Maintenance { id, .. } => {
                *id
            }
        };
        store = match held {
            owner::open::Held::Capture { store, id } => {
                #[cfg(test)]
                let store = match wrap_store.take() {
                    Some(wrap) => wrap(store),
                    None => store,
                };
                match capture_owner(store, held_cap, switches, root, dir.path()) {
                    Ok(owner) => {
                        let wanted = sources::starting(
                            halvo_control::proto::Platform::current(),
                            sources::RunKind::Service,
                            switches,
                        );
                        supervisor.serve_sources(owner.context(), wanted);
                        #[cfg(feature = "r2")]
                        listening
                            .admitting()
                            .hold_owner(Some(owner.watched.handle.clone()));
                        #[cfg(feature = "r2")]
                        r2::service::open(
                            dir.path(),
                            children::own_dir(),
                            r2::when::SystemReadings,
                            Box::new(std::io::stderr()),
                        );
                        let store = HeldStore::Capture(Box::new(owner));
                        supervisor.set_slot(slot::Slot::Open { id }, store.link());
                        store
                    }
                    Err(error) => {
                        let _ = writeln!(
                            err,
                            "halvo-capture: the store owner did not start: {}",
                            error.kind()
                        );
                        HeldStore::None
                    }
                }
            }
            owner::open::Held::Maintenance { store, id } => {
                supervisor.set_slot(slot::Slot::Maintenance { id: Some(id) }, None);
                HeldStore::Maintenance(Box::new(maintenance::Upkeep::new(
                    store,
                    maintenance::SystemClocks::new(),
                    held_cap,
                )))
            }
        };
        delete_written = answer_the_delete(&mut deleted, Some(made)).or(delete_written);
    }
    listening.admitting().end_the_life();
    answer_the_queue(&listening);
    answer_the_delete(&mut deleted, None);
    #[cfg(feature = "r2")]
    listening.admitting().hold_owner(None);
    if let Some(opener) = &opener {
        opener.stop();
        opener.wait_for_its_call(CLOSE_WAIT);
        say_the_notes(Some(opener), err);
        if opener.take().is_some() {
            mark = mark.at_the_answer();
        }
    }
    mark_the_store_made(mark, &mut supervisor, err);
    if let Some(written) = delete_written.filter(|_| delete_ends) {
        let _ = written.wait(WRITTEN_WAIT);
    }
    if let Some(written) = reset_written {
        let _ = written.wait(WRITTEN_WAIT);
    }
    let outcome = supervisor.shutdown_entry(store.link(), status);
    #[cfg(test)]
    if let Some(settled) = parts.settled {
        settled(outcome.as_ref().and_then(|outcome| outcome.settled));
    }
    let events_lost = outcome
        .as_ref()
        .and_then(|outcome| outcome.settled)
        .map_or(0, |settled| {
            settled.events_lost.saturating_add(settled.events_pending)
        });
    let status = outcome.map_or(status, |outcome| outcome.status);
    if let HeldStore::Capture(owner) = &store {
        if !supervisor::OwnerLink::is_running(&owner.watched) {
            let _ = writeln!(
                err,
                "{}",
                owner_ended(&owner.watched.snapshot(), &owner.sink.counts()).line
            );
        }
    }
    let status = match store {
        HeldStore::Capture(owner) => close_the_store(*owner, status, err),
        HeldStore::None | HeldStore::Maintenance(_) => status,
    };
    if let Err(error) = run_marker::at_the_end(dir, status, ended_by) {
        let _ = writeln!(
            err,
            "halvo-capture: the run marker's line of the end was not written: {}",
            error.kind()
        );
    }
    if let Some(asked) = stop_asked {
        let stopped = halvo_control::proto::Stopped {
            flushed: status != halvo_control::exit::ERROR,
            events_lost,
        };
        let _ = asked
            .answer(halvo_control::proto::Reply::Stopped(stopped))
            .wait(WRITTEN_WAIT);
    }
    drop(listening);
    drop(lock);
    status
}

/// Writes the decision of a stop, `run_marker::decided`, at the moment that
/// the loop of the service run accepts a Stop request or reads that a signal
/// set the stop flag, before the shutdown entry; a write that fails is a line.
#[cfg(any(target_os = "macos", windows))]
fn decide_the_stop(
    dir: &halvo_control::paths::DataDir,
    kind: halvo_control::proto::StopKind,
    err: &mut dyn std::io::Write,
) {
    if let Err(error) = run_marker::decided(dir, kind) {
        let _ = writeln!(
            err,
            "halvo-capture: the run marker's line of the stop was not written: {}",
            error.kind()
        );
    }
}

/// The line of a start with `--no-press` that ends with WAITS_FOR_A_PRESS.
#[cfg(any(target_os = "macos", windows))]
const WAITS_FOR_A_PRESS_LINE: &str = "halvo-capture: no press asked for this start, and the last life of the data directory ended at a stop that was asked or does not tell why it ended: capture waits for a press on Start";

/// The trigger, in the words of contracts.md C2.3, of an end that the wait of
/// the service run read: the Windows session's end, which marks itself
/// before it sets the stop flag; the stop flag, which SIGINT and SIGTERM set
/// on a Mac and the session window's WM_CLOSE sets on Windows (a console's
/// Ctrl-C and Ctrl-Break set it in a debug build alone; a release build
/// installs no console handler); and with neither, the lifeline's end.
#[cfg(any(target_os = "macos", windows))]
fn trigger_of_the_wait(stop: &std::sync::atomic::AtomicBool) -> halvo_control::proto::StopKind {
    if signal::session_end_budget().is_some() {
        halvo_control::proto::StopKind::SessionEnd
    } else if stop.load(std::sync::atomic::Ordering::SeqCst) {
        halvo_control::proto::StopKind::Signal
    } else {
        halvo_control::proto::StopKind::Lifeline
    }
}

#[cfg(any(target_os = "macos", windows))]
fn second_now() -> halvo_control::model::UtcSecond {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    halvo_control::model::UtcSecond::from_unix_seconds(i64::try_from(seconds).unwrap_or(0))
        .unwrap_or_else(|_| {
            halvo_control::model::UtcSecond::from_unix_seconds(0)
                .expect("the epoch is a second of the model")
        })
}

#[cfg(any(target_os = "macos", windows))]
struct Serving<'a> {
    listening: &'a server::Listening,
    facts: &'a server::RunFacts,
    obstructed: Option<Duration>,
}

#[cfg(any(target_os = "macos", windows))]
enum Waited {
    Window,
    Ended,
    Stop(server::Asked),
    Delete(server::Asked),
    Reset(server::Asked),
}

#[cfg(any(target_os = "macos", windows))]
fn wait_serving<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    supervisor: &mut supervisor::Supervisor<F, L>,
    serving: &Serving<'_>,
    window: Duration,
    stop: &std::sync::atomic::AtomicBool,
    store: &HeldStore,
) -> Waited {
    let until = supervisor.main_loop().now() + window;
    let mut first = true;
    loop {
        let began = supervisor.main_loop().now();
        let slice = match main_loop::slice(began, until) {
            Some(slice) => slice,
            None if first => Duration::ZERO,
            None => return Waited::Window,
        };
        first = false;
        if supervisor.wait_on(slice, stop, store.link()) {
            return Waited::Ended;
        }
        while let Some(asked) = serving.listening.next() {
            if matches!(asked.request, halvo_control::proto::Request::Stop) {
                return Waited::Stop(asked);
            }
            if matches!(
                asked.request,
                halvo_control::proto::Request::DeleteAll { .. }
            ) {
                return Waited::Delete(asked);
            }
            if matches!(asked.request, halvo_control::proto::Request::ResetSettings) {
                return Waited::Reset(asked);
            }
            serve_request(asked, supervisor, serving, store);
        }
        if supervisor.main_loop().now() < began + slice {
            return Waited::Window;
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn serve_request<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    asked: server::Asked,
    supervisor: &mut supervisor::Supervisor<F, L>,
    serving: &Serving<'_>,
    store: &HeldStore,
) {
    use halvo_control::proto::{ErrorCode, ErrorReply, Reply, Request};
    #[cfg(feature = "r2")]
    r2::service::seen(&asked.request);
    if asked.request.claims_the_indicator() {
        supervisor.renew_lease(store.link());
    }
    let reply = match asked.request {
        Request::Status { indicator } => {
            if !indicator {
                supervisor.client_polled();
            }
            status_reply(supervisor, serving).map_or_else(
                |broken| Reply::Error(ErrorReply::new(ErrorCode::Internal, broken.rule())),
                Reply::Status,
            )
        }
        Request::Inventory => inventory_reply(supervisor, store),
        Request::SetStorageCap { bytes } => cap_set_reply(bytes, supervisor, serving, store),
        Request::AcknowledgeLostPermission { permission } => {
            acknowledged_reply(permission, supervisor, serving)
        }
        Request::RequestPermission { permission } => {
            permission_reply(supervisor.request_permission(permission))
        }
        Request::Pause | Request::Resume => {
            capture_change(asked, supervisor, serving, store);
            return;
        }
        _ => Reply::Error(ErrorReply::new(
            ErrorCode::UnsupportedOp,
            "this build of the capture service does not serve the op",
        )),
    };
    let _ = asked.answer(reply);
}

#[cfg(any(target_os = "macos", windows))]
const WRITTEN_WAIT: Duration = Duration::from_millis(2_500);

#[cfg(any(target_os = "macos", windows))]
fn answer_the_queue(listening: &server::Listening) {
    let life = listening.admitting().ending_life();
    while let Some(asked) = listening.next() {
        let admission = halvo_control::proto::admission(
            &asked.request,
            halvo_control::proto::PERMISSION_FORM,
            false,
            halvo_control::proto::Life::Ending,
        );
        let reply = match (life.answer(admission, &asked.request), admission) {
            (Some(reply), _) => reply,
            (None, halvo_control::proto::Admission::Ending) => halvo_control::proto::Reply::Error(
                halvo_control::proto::ErrorReply::busy_of_an_ending_life(),
            ),
            (None, _) => continue,
        };
        let _ = asked.answer(reply);
    }
}

#[cfg(any(target_os = "macos", windows))]
enum DeleteAsked {
    Answered,
    Reopen,
    Ending(server::Written),
    Ran {
        asked: server::Asked,
        report: Option<halvo_control::proto::DeleteReport>,
        whole: bool,
        ending: bool,
    },
}

#[cfg(any(target_os = "macos", windows))]
fn delete_on_request<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    asked: server::Asked,
    supervisor: &mut supervisor::Supervisor<F, L>,
    serving: &Serving<'_>,
    (store, opener): (&mut HeldStore, &mut Option<owner::open::Opener>),
    dir: &halvo_control::paths::DataDir,
    err: &mut dyn std::io::Write,
) -> DeleteAsked {
    use halvo_control::proto::{AlreadyDeleted, DeleteDecision, ErrorCode, ErrorReply, Reply};
    let armed = match &asked.request {
        halvo_control::proto::Request::DeleteAll { store } => *store,
        _ => None,
    };
    let status = match status_reply(supervisor, serving) {
        Ok(status) => status,
        Err(broken) => {
            let _ = asked.answer(Reply::Error(ErrorReply::new(
                ErrorCode::Internal,
                broken.rule(),
            )));
            return DeleteAsked::Answered;
        }
    };
    let fields = status.fields();
    let (store_read, unfinished) = the_store_that_the_rule_reads(fields, opener.as_ref());
    let refused = match halvo_control::proto::delete_decision(
        serving.listening.admitting().life(),
        fields.capture,
        store_read,
        fields.store_error,
        unfinished,
        fields.store_id,
        armed,
    ) {
        DeleteDecision::Accept => None,
        DeleteDecision::Busy => Some(Reply::Error(ErrorReply::new(
            ErrorCode::Busy,
            "the delete cannot start now; this request deleted nothing",
        ))),
        DeleteDecision::Ending => Some(Reply::Error(ErrorReply::busy_of_an_ending_life())),
        DeleteDecision::Refused => Some(Reply::DeleteRefused),
        DeleteDecision::AlreadyDeleted { current } => {
            Some(Reply::AlreadyDeleted(AlreadyDeleted { current }))
        }
    };
    if let Some(reply) = refused {
        let _ = asked.answer(reply);
        return DeleteAsked::Answered;
    }
    let mut went_on = false;
    supervisor.set_slot_answering(slot::Slot::Deleting, store.link(), &mut |_| {
        serving.listening.admitting().end_the_life();
        went_on = true;
    });
    if let Some(tries) = opener.take() {
        tries.stop();
        tries.wait_for_its_call(CLOSE_WAIT);
        drop(tries.take());
    }
    #[cfg(feature = "r2")]
    serving.listening.admitting().hold_owner(None);
    let figures = store.link().map(supervisor::OwnerLink::snapshot);
    if let HeldStore::Capture(owner) = std::mem::replace(store, HeldStore::None) {
        let _ = if went_on {
            close_unsettled(*owner, err)
        } else {
            close_the_store(*owner, halvo_control::exit::CLEAN, err)
        };
    }
    let rows = delete_all::rows();
    let how = delete_all::Offline {
        rows: &rows,
        select: custody_policy::of_this_build,
        hands: &delete_all::SystemHands,
    };
    let tail = std::cell::RefCell::new(&mut *supervisor);
    let mut after_the_intent = || {
        if went_on {
            tail.borrow_mut().record_the_delete(figures.as_ref(), false);
        }
    };
    let mut clear = || {
        match tail.borrow_mut().update_state(|state| state.store_made = false) {
        None | Some(Ok(())) | Some(Err(state::StateError::LeftAsItIs { .. })) => None,
        Some(Err(_)) => Some(
            "halvo-capture: state_file=write_failed: the mark store_made was not cleared in daemon-state.json"
                .to_owned(),
        ),
    }
    };
    #[cfg(feature = "r2")]
    r2::marks::handler().drop_held();
    #[cfg(feature = "r2")]
    r2::service::delete();
    match delete_all::steps_of_the_delete(dir.path(), &how, &mut after_the_intent, &mut clear) {
        delete_all::Steps::Busy => {
            let line = "another process holds the store; this request deleted nothing";
            let _ = writeln!(err, "halvo-capture: {line}");
            if went_on {
                supervisor.record_the_delete(figures.as_ref(), true);
            }
            let written = asked.answer(Reply::Error(ErrorReply::new(ErrorCode::Busy, line)));
            if went_on {
                DeleteAsked::Ending(written)
            } else {
                DeleteAsked::Reopen
            }
        }
        delete_all::Steps::Ran(ran) => {
            if went_on {
                supervisor.record_the_delete(None, true);
            }
            for cause in &ran.causes {
                let _ = writeln!(err, "{cause}");
            }
            let report = ran
                .report
                .as_ref()
                .and_then(|report| delete_report(&ran, report).ok());
            let whole = ran.whole && ran.synced;
            let unloaded = matches!(
                supervisor.status_parts(None).health,
                state::StateHealth::Corrupt | state::StateHealth::NewerVersion(_)
            );
            if whole && unloaded {
                if let Some(Err(error)) = supervisor.with_state_file(state::StateFile::reset) {
                    let _ = writeln!(
                        err,
                        "halvo-capture: state_file=write_failed: the defaults were not written over daemon-state.json after the delete: {}",
                        error.kind()
                    );
                }
            }
            DeleteAsked::Ran {
                asked,
                report,
                whole,
                ending: went_on,
            }
        }
    }
}

/// The store's state and its `store_delete_unfinished` that the delete rule reads: the
/// status's, except while an opener runs whose slot reads opening or deleting, whose own call
/// may yet begin or still runs the finish of a standing delete, also in a data directory that
/// failed its check, whose status reads unavailable: then its slot's, so that the rule answers
/// busy and no second walk begins beside that call. Whether it runs is read before its slot,
/// and its thread leaves those two slots only once that call has returned.
#[cfg(any(target_os = "macos", windows))]
fn the_store_that_the_rule_reads(
    fields: &halvo_control::proto::StatusFields,
    opener: Option<&owner::open::Opener>,
) -> (halvo_control::proto::StoreState, bool) {
    match opener
        .filter(|opener| opener.is_running())
        .map(owner::open::Opener::slot)
    {
        Some(slot @ (slot::Slot::Opening | slot::Slot::Deleting)) => (slot.state(), false),
        _ => (fields.store, fields.store_delete_unfinished),
    }
}

#[cfg(any(target_os = "macos", windows))]
fn delete_report(
    ran: &delete_all::Ran,
    report: &TailReport,
) -> Result<halvo_control::proto::DeleteReport, halvo_control::proto::DeleteReportError> {
    use halvo_control::proto::{DeleteFields, DeleteReport, IntentState, RootDestroyed};
    let (root_destroyed, root_id) = match &report.root {
        TailRoot::Destroyed => (RootDestroyed::Yes, None),
        TailRoot::Left(Some(hex)) => match hex.parse() {
            Ok(id) => (RootDestroyed::No, Some(id)),
            Err(_) => (RootDestroyed::Unknown, None),
        },
        TailRoot::Left(None) | TailRoot::Unnamed => (RootDestroyed::Unknown, None),
    };
    DeleteReport::new(DeleteFields {
        segments_shredded: report.keyed_segments,
        keys_destroyed: report.keyed_segments.map(|n| n.saturating_add(1)),
        root_destroyed,
        root_id,
        files_left: report.files_left,
        database_left: report.database_left,
        not_followed: report.not_followed,
        entries_removed: ran.entries_removed,
        entries_left: ran.entries_left,
        unknown_entries: ran.unknown_entries,
        intent: if ran.intent_written {
            IntentState::Written
        } else {
            IntentState::Failed
        },
    })
}

#[cfg(any(target_os = "macos", windows))]
type DeletedReply = Option<(server::Asked, halvo_control::proto::DeleteReport)>;

#[cfg(any(target_os = "macos", windows))]
fn answer_the_delete(
    deleted: &mut DeletedReply,
    store: Option<port::StoreId>,
) -> Option<server::Written> {
    let (asked, report) = deleted.take()?;
    Some(asked.answer(halvo_control::proto::Reply::Deleted(
        halvo_control::proto::Deleted {
            report,
            store: store.map(|id| halvo_control::model::StoreId::from_bytes(*id.as_bytes())),
            unfinished: false,
        },
    )))
}

#[cfg(any(target_os = "macos", windows))]
fn made_none(opener: &owner::open::Opener) -> bool {
    opener.tries() > 0
        && !matches!(
            opener.slot(),
            slot::Slot::Open { .. } | slot::Slot::Maintenance { id: Some(_) }
        )
}

#[cfg(any(target_os = "macos", windows))]
fn capture_change<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    asked: server::Asked,
    supervisor: &mut supervisor::Supervisor<F, L>,
    serving: &Serving<'_>,
    store: &HeldStore,
) {
    use halvo_control::proto::{ErrorCode, ErrorReply, Reply, Request};
    let desired = match asked.request {
        Request::Resume => state::Desired::Running,
        _ => state::Desired::Paused,
    };
    let facts = server::RunFacts {
        rss_peak_bytes: server::rss_peak_bytes(),
        ..*serving.facts
    };
    let changed = |parts: &server::StatusParts| {
        server::status_of(parts, &facts).map_or_else(
            |broken| Reply::Error(ErrorReply::new(ErrorCode::Internal, broken.rule())),
            Reply::CaptureChanged,
        )
    };
    let before = supervisor.status_parts(serving.obstructed);
    let mut pending = Some(asked);
    let _ = supervisor.set_desired(desired, store.link(), &mut |board| {
        if let Some(asked) = pending.take() {
            serving.listening.admitting().end_the_life();
            let parts = server::StatusParts {
                board: board.clone(),
                ..before.clone()
            };
            let _ = asked.answer(changed(&parts)).wait(WRITTEN_WAIT);
        }
    });
    if let Some(asked) = pending {
        let _ = asked.answer(changed(&supervisor.status_parts(serving.obstructed)));
    }
}

#[cfg(any(target_os = "macos", windows))]
fn reset_on_request<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    asked: server::Asked,
    supervisor: &mut supervisor::Supervisor<F, L>,
    serving: &Serving<'_>,
) -> Option<server::Written> {
    use halvo_control::proto::{ErrorCode, ErrorReply, Reply};
    let refused = match supervisor.with_state_file(state::StateFile::reset_settings) {
        Some(Ok(())) => None,
        Some(Err(refused)) => Some(ErrorReply::new(ErrorCode::StateFile, refused.to_string())),
        None => Some(ErrorReply::new(
            ErrorCode::Internal,
            "the capture service holds no state file",
        )),
    };
    if let Some(refused) = refused {
        let _ = asked.answer(Reply::Error(refused));
        return None;
    }
    serving.listening.admitting().end_the_life();
    let reply = status_reply(supervisor, serving).map_or_else(
        |broken| Reply::Error(ErrorReply::new(ErrorCode::Internal, broken.rule())),
        Reply::Status,
    );
    Some(asked.answer(reply))
}

#[cfg(any(target_os = "macos", windows))]
const INVENTORY_WAIT: Duration = Duration::from_millis(1_500);

/// Writes `cap` through the state file's one writer (contracts.md C2.1,
/// C2.5) and answers the write's answer with the bound of the cap that the
/// state holds after it, which the owner evicts to: none for a cap that is
/// not known, as after a write into a state file that does not load, which
/// writes nothing.
#[cfg(any(target_os = "macos", windows))]
fn set_cap(
    state: &mut state::StateFile,
    cap: halvo_control::model::StorageCap,
) -> (Result<(), state::StateError>, Option<port::ByteBound>) {
    let written = state.update(|state| state.storage_cap = state::Cap::Known(cap));
    (written, cap_bound(state.state().storage_cap))
}

/// The answer of a set-cap of `cap` whose write answered `written`, on a
/// store whose disk total reads `disk_bytes`: whether that total stands above
/// the cap, which the reply says as `evicting`, or the error `state_file`
/// that names the file when the write did not land.
#[cfg(any(target_os = "macos", windows))]
fn cap_set_answer(
    written: &Result<(), state::StateError>,
    cap: halvo_control::model::StorageCap,
    disk_bytes: Option<u64>,
) -> Result<bool, halvo_control::proto::ErrorReply> {
    match written {
        Ok(()) => Ok(disk_bytes.is_some_and(|disk| disk > cap.bytes())),
        Err(error) => Err(halvo_control::proto::ErrorReply::new(
            halvo_control::proto::ErrorCode::StateFile,
            format!("the cap was not changed: {error}"),
        )),
    }
}

#[cfg(any(target_os = "macos", windows))]
fn cap_set_reply<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    cap: halvo_control::model::StorageCap,
    supervisor: &mut supervisor::Supervisor<F, L>,
    serving: &Serving<'_>,
    store: &HeldStore,
) -> halvo_control::proto::Reply {
    use halvo_control::proto::{CapSet, ErrorCode, ErrorReply, Reply};
    let Some((written, bound)) = supervisor.with_state_file(|state| set_cap(state, cap)) else {
        return Reply::Error(ErrorReply::new(
            ErrorCode::Internal,
            "the capture service holds no state file",
        ));
    };
    if let Some(owner) = store.link() {
        if let Ok(pending) = owner.ask(owner::Command::SetBound(bound)) {
            let _ = supervisor.answer_within(&pending, INVENTORY_WAIT);
        }
    }
    let disk_bytes = supervisor
        .status_parts(serving.obstructed)
        .board
        .snapshot
        .and_then(|snapshot| snapshot.disk_bytes);
    match cap_set_answer(&written, cap, disk_bytes) {
        Err(refused) => Reply::Error(refused),
        Ok(evicting) => status_reply(supervisor, serving).map_or_else(
            |broken| Reply::Error(ErrorReply::new(ErrorCode::Internal, broken.rule())),
            |status| Reply::CapSet(CapSet { status, evicting }),
        ),
    }
}

#[cfg(any(target_os = "macos", windows))]
fn permission_reply(requested: supervisor::AskAnswer) -> halvo_control::proto::Reply {
    use halvo_control::onboarding::RequestOutcome;
    use halvo_control::proto::{ErrorCode, ErrorReply, PermissionRequested, Reply};
    use supervisor::AskAnswer;
    let outcome = |outcome| Reply::PermissionRequested(PermissionRequested { outcome });
    match requested {
        AskAnswer::AlreadyGranted => outcome(RequestOutcome::AlreadyGranted),
        AskAnswer::Requested => outcome(RequestOutcome::Requested),
        AskAnswer::AnotherRuns => Reply::Error(ErrorReply::new(
            ErrorCode::Busy,
            "the system's question about another permission is still open; this request asked nothing",
        )),
        AskAnswer::NotServed => Reply::Error(ErrorReply::new(
            ErrorCode::UnsupportedOp,
            "this build of the capture service does not serve the op",
        )),
        AskAnswer::NoStateFile => Reply::Error(ErrorReply::new(
            ErrorCode::Internal,
            "the capture service holds no state file",
        )),
        AskAnswer::NotRecorded(error) => Reply::Error(ErrorReply::new(
            ErrorCode::StateFile,
            format!("the request was not recorded and asked nothing: {error}"),
        )),
        AskAnswer::NotStarted(error) => Reply::Error(ErrorReply::new(
            ErrorCode::Internal,
            format!("the request child did not start: {}", error.kind()),
        )),
    }
}

#[cfg(any(target_os = "macos", windows))]
fn acknowledged_reply<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    permission: Permission,
    supervisor: &mut supervisor::Supervisor<F, L>,
    serving: &Serving<'_>,
) -> halvo_control::proto::Reply {
    use halvo_control::proto::{ErrorCode, ErrorReply, Reply};
    match supervisor.with_state_file(|state| state.forget_granted(permission)) {
        None => Reply::Error(ErrorReply::new(
            ErrorCode::Internal,
            "the capture service holds no state file",
        )),
        Some(Err(error)) => Reply::Error(ErrorReply::new(
            ErrorCode::StateFile,
            format!("the lost permission was not put away: {error}"),
        )),
        Some(Ok(())) => status_reply(supervisor, serving).map_or_else(
            |broken| Reply::Error(ErrorReply::new(ErrorCode::Internal, broken.rule())),
            Reply::Status,
        ),
    }
}

#[cfg(any(target_os = "macos", windows))]
fn inventory_reply<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    supervisor: &mut supervisor::Supervisor<F, L>,
    store: &HeldStore,
) -> halvo_control::proto::Reply {
    use halvo_control::proto::{ErrorCode, ErrorReply, InventoryReply, Reply, Totals};
    let parts = supervisor.status_parts(None);
    let (slot::Slot::Open { id }, Some(owner)) = (&parts.slot, store.link()) else {
        return Reply::Error(ErrorReply::new(
            ErrorCode::StoreUnavailable,
            parts.slot.state().as_str(),
        ));
    };
    let answer = match owner.ask(owner::Command::Inventory) {
        Ok(pending) => supervisor.answer_within(&pending, INVENTORY_WAIT),
        Err(owner::OwnerGone) => None,
    };
    let rows = match answer {
        Some(owner::Answer::Inventory(Ok(rows))) => rows,
        Some(owner::Answer::Inventory(Err(_))) => {
            return Reply::Error(ErrorReply::new(
                ErrorCode::StoreUnavailable,
                "the store's listing could not be read",
            ))
        }
        _ => {
            return Reply::Error(ErrorReply::new(
                ErrorCode::Internal,
                "the store owner did not answer the listing in time",
            ))
        }
    };
    let totals = Totals {
        events: rows.iter().map(|row| row.events).sum(),
        bytes: rows.iter().map(|row| row.bytes).sum(),
        disk_bytes: parts
            .board
            .snapshot
            .and_then(|snapshot| snapshot.disk_bytes)
            .unwrap_or(0),
    };
    InventoryReply::new(
        halvo_control::model::StoreId::from_bytes(*id.as_bytes()),
        second_now(),
        rows,
        totals,
        Vec::new(),
    )
    .map_or_else(
        |broken| Reply::Error(ErrorReply::new(ErrorCode::Internal, broken.to_string())),
        Reply::Inventory,
    )
}

#[cfg(any(target_os = "macos", windows))]
fn status_reply<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    supervisor: &supervisor::Supervisor<F, L>,
    serving: &Serving<'_>,
) -> Result<halvo_control::proto::DaemonStatus, halvo_control::proto::StatusError> {
    let facts = server::RunFacts {
        rss_peak_bytes: server::rss_peak_bytes(),
        ..*serving.facts
    };
    server::status_of(&supervisor.status_parts(serving.obstructed), &facts)
}

#[cfg(any(target_os = "macos", windows))]
const CLOSE_WAIT: Duration = Duration::from_secs(1);

#[cfg(any(target_os = "macos", windows))]
fn close_the_store(owner: CaptureOwner, status: i32, err: &mut dyn std::io::Write) -> i32 {
    close_by(owner, owner::Command::Shutdown, status, err)
}

#[cfg(any(target_os = "macos", windows))]
fn close_unsettled(owner: CaptureOwner, err: &mut dyn std::io::Write) -> i32 {
    close_by(
        owner,
        owner::Command::EndUnsettled,
        halvo_control::exit::CLEAN,
        err,
    )
}

#[cfg(any(target_os = "macos", windows))]
fn close_by(
    owner: CaptureOwner,
    command: owner::Command,
    status: i32,
    err: &mut dyn std::io::Write,
) -> i32 {
    let answered = owner
        .watched
        .handle
        .ask(command)
        .ok()
        .and_then(|pending| pending.wait_for(CLOSE_WAIT));
    let unclosed = if !matches!(answered, Some(owner::Answer::ShutDown(_))) {
        Some("the store owner did not answer the Shutdown within 1 s")
    } else if owner.watched.join().is_err() {
        Some("the store owner's thread ended by a panic")
    } else {
        None
    };
    let Some(why) = unclosed else {
        return status;
    };
    let _ = writeln!(
        err,
        "halvo-capture: {why}; the store was not closed in order"
    );
    match status {
        halvo_control::exit::CLEAN => halvo_control::exit::ERROR,
        status => status,
    }
}

#[cfg(any(target_os = "macos", windows))]
enum HeldStore {
    None,
    Capture(Box<CaptureOwner>),
    Maintenance(Box<MaintenanceUpkeep>),
}

#[cfg(any(target_os = "macos", windows))]
type MaintenanceUpkeep =
    maintenance::Upkeep<Box<dyn port::MaintainStore + Send>, maintenance::SystemClocks>;

#[cfg(any(target_os = "macos", windows))]
impl HeldStore {
    fn link(&self) -> Option<&dyn supervisor::OwnerLink> {
        match self {
            HeldStore::Capture(owner) => Some(&owner.watched),
            HeldStore::None | HeldStore::Maintenance(_) => None,
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn cap_bound(cap: state::Cap) -> Option<port::ByteBound> {
    match cap {
        state::Cap::Known(cap) => Some(port::ByteBound::of_cap(cap)),
        state::Cap::OutOfRange(_) | state::Cap::Unread => None,
    }
}

#[cfg(any(target_os = "macos", windows))]
struct CaptureOwner {
    watched: supervisor::Watched,
    sink: crate::sink::SinkHandle,
    floor: crate::floor::exclude::FloorHandle,
    #[cfg(target_os = "macos")]
    _marks: agent_marks::MarkListing,
}

#[cfg(any(target_os = "macos", windows))]
impl CaptureOwner {
    fn context(&self) -> SourceContext {
        SourceContext::with_floor(self.sink.clone(), self.floor.clone())
    }
}

#[cfg(any(target_os = "macos", windows))]
fn system_lineage() -> std::sync::Arc<dyn crate::floor::exclude::Lineage> {
    #[cfg(target_os = "macos")]
    {
        std::sync::Arc::new(crate::macos::foreground::ProcessLineage)
    }
    #[cfg(windows)]
    {
        std::sync::Arc::new(crate::windows::exclusion::ProcessLineage::new())
    }
}

#[cfg(any(target_os = "macos", windows))]
fn capture_owner(
    store: Box<dyn port::CaptureStore + Send>,
    cap: Option<port::ByteBound>,
    switches: crate::floor::Switches,
    root: Option<crate::floor::exclude::ProcessId>,
    data_dir: &std::path::Path,
) -> std::io::Result<CaptureOwner> {
    use crate::floor::exclude::FloorHandle;
    let (sink, writer) = crate::sink::channel(owner::QUEUE_SLOTS);
    #[cfg(target_os = "macos")]
    let marks = agent_marks::MarkListing::start(data_dir)?;
    #[cfg(windows)]
    let _ = data_dir;
    let floor = FloorHandle::new(root, system_lineage(), switches);
    #[cfg(target_os = "macos")]
    let floor = floor.with_marks(marks.marks());
    let thread = owner::spawn(owner::OwnerParts {
        writer,
        store,
        clocks: maintenance::SystemClocks::new(),
        floor: floor.clone(),
        cap,
        shred_chunk: maintenance::SHRED_CHUNK,
    })?;
    Ok(CaptureOwner {
        watched: supervisor::Watched::new(thread, Box::new(std::io::stderr())),
        sink,
        floor,
        #[cfg(target_os = "macos")]
        _marks: marks,
    })
}

#[cfg(any(target_os = "macos", windows))]
const MARK_RETRY: Duration = Duration::from_secs(1);

#[cfg(any(target_os = "macos", windows))]
const DIR_SHUT_LINE: &str = "halvo-capture: store=unavailable store_error=obstructed: the data directory failed its check; no store was made or opened in it";

#[cfg(any(target_os = "macos", windows))]
#[derive(Debug, Clone, Copy)]
struct DirShut {
    next: std::time::Instant,
    delay: Duration,
}

#[cfg(any(target_os = "macos", windows))]
impl DirShut {
    fn first() -> DirShut {
        DirShut {
            next: std::time::Instant::now() + owner::RETRY_FIRST,
            delay: owner::RETRY_FIRST,
        }
    }

    fn again(self) -> DirShut {
        let delay = (self.delay * 2).min(owner::RETRY_MAX);
        DirShut {
            next: std::time::Instant::now() + delay,
            delay,
        }
    }

    fn left(self) -> Duration {
        self.next
            .saturating_duration_since(std::time::Instant::now())
    }
}

#[cfg(any(target_os = "macos", windows))]
fn start_the_opener(
    dir: &halvo_control::paths::DataDir,
    rule: owner::open::CreateRule,
    err: &mut dyn std::io::Write,
) -> Option<owner::open::Opener> {
    match owner::open::Opener::start_with(
        delete_all::at_start_of(dir.path()),
        Box::new(port::DataDirStore::at(dir.path())),
        custody_policy::of_this_build,
        rule,
    ) {
        Ok(opener) => Some(opener),
        Err(error) => {
            let _ = writeln!(
                err,
                "halvo-capture: store=unavailable the open did not start: {}",
                error.kind()
            );
            None
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn open_after_the_check<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    supervisor: &mut supervisor::Supervisor<F, L>,
    start: impl FnOnce() -> Option<owner::open::Opener>,
) -> Option<owner::open::Opener> {
    supervisor.set_slot(slot::Slot::Opening, None);
    start()
}

/// The finish of a delete-all intent where the data directory failed its
/// check, with no open after it: a failed check never gates a delete
/// (contracts.md C3.2 step 3). The open starts once the check passes and
/// this finish has ended.
#[cfg(any(target_os = "macos", windows))]
fn finish_the_intent_alone(
    dir: &halvo_control::paths::DataDir,
    err: &mut dyn std::io::Write,
) -> Option<owner::open::Opener> {
    match owner::open::Opener::finish_alone(
        delete_all::at_start_of(dir.path()),
        Box::new(port::DataDirStore::at(dir.path())),
    ) {
        Ok(finish) => Some(finish),
        Err(error) => {
            let _ = writeln!(
                err,
                "halvo-capture: the finish of delete-all.intent did not start: {}",
                error.kind()
            );
            None
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    NotYet,
    Pending { said: bool },
    Settled,
}

#[cfg(any(target_os = "macos", windows))]
impl Mark {
    fn at_the_answer(self) -> Mark {
        match self {
            Mark::NotYet => Mark::Pending { said: false },
            Mark::Pending { .. } | Mark::Settled => self,
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn say_the_notes(opener: Option<&owner::open::Opener>, err: &mut dyn std::io::Write) {
    while let Some(note) = opener.and_then(owner::open::Opener::take_note) {
        let _ = writeln!(err, "{}", note.line());
    }
}

#[cfg(any(target_os = "macos", windows))]
fn mark_the_store_made<F: supervisor::SystemSources, L: main_loop::MainLoop>(
    mark: Mark,
    supervisor: &mut supervisor::Supervisor<F, L>,
    err: &mut dyn std::io::Write,
) -> Mark {
    use state::StateError;
    let Mark::Pending { said } = mark else {
        return mark;
    };
    let kind = match supervisor.update_state(|state| state.store_made = true) {
        None | Some(Ok(())) | Some(Err(StateError::LeftAsItIs { .. })) => return Mark::Settled,
        Some(Err(
            StateError::NotWritten { kind, .. }
            | StateError::NotFlushed { kind, .. }
            | StateError::MayHoldTheChange { kind, .. },
        )) => kind,
    };
    if !said {
        let _ = writeln!(err, "halvo-capture: state_file=write_failed error={kind}");
    }
    Mark::Pending { said: true }
}

#[cfg(any(target_os = "macos", windows))]
fn take_the_data_dir(
    dir: &halvo_control::paths::DataDir,
    err: &mut dyn std::io::Write,
) -> Result<Option<instance::InstanceLock>, i32> {
    use halvo_control::paths::EntryAtDataDir;
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::DirBuilderExt;

    match dir.is_provisioned() {
        Err(cause @ (EntryAtDataDir::LinkLeadsNowhere | EntryAtDataDir::NotADirectory)) => {
            let _ = writeln!(err, "halvo-capture: {cause}");
            return Err(halvo_control::exit::ERROR);
        }
        Ok(_) | Err(EntryAtDataDir::CannotBeRead) => {}
    }
    if std::fs::symlink_metadata(dir.path())
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(target_os = "macos")]
        builder.mode(0o700);
        let made = builder.create(dir.path());
        if let Err(error) = made {
            let _ = writeln!(
                err,
                "halvo-capture: the data directory could not be made: {}",
                error.kind()
            );
            return Err(halvo_control::exit::ERROR);
        }
    }
    instance::InstanceLock::take(dir).map_err(|error| {
        let _ = match halvo_control::paths::entry_cause(&error) {
            Some(cause) => writeln!(err, "halvo-capture: {cause}"),
            None => writeln!(
                err,
                "halvo-capture: the data directory could not be opened: {}",
                error.kind()
            ),
        };
        halvo_control::exit::ERROR
    })
}

#[cfg(target_os = "macos")]
fn obstructed_line(fault: halvo_control::paths::DataDirFault) -> String {
    use halvo_control::paths::{DataDirFault, EntryAtDataDir};
    let cause = match fault {
        DataDirFault::Entry(EntryAtDataDir::LinkLeadsNowhere) => "link_leads_nowhere".to_owned(),
        DataDirFault::Entry(EntryAtDataDir::NotADirectory) => "not_a_directory".to_owned(),
        DataDirFault::Entry(EntryAtDataDir::CannotBeRead) => "cannot_be_read".to_owned(),
        DataDirFault::OtherOwner => "other_owner".to_owned(),
        DataDirFault::LinkedAndOpenToOthers => "linked_and_open_to_others".to_owned(),
        DataDirFault::Io(kind) => format!("io error={kind}"),
        DataDirFault::OwnOnAVolumeThatIgnoresOwnership => {
            "own_on_a_volume_that_ignores_ownership".to_owned()
        }
        DataDirFault::LinkedToAVolumeThatIgnoresOwnership => {
            "linked_to_a_volume_that_ignores_ownership".to_owned()
        }
        DataDirFault::ModeDidNotTake => "mode_did_not_take".to_owned(),
    };
    format!("halvo-capture: data_dir=obstructed cause={cause}")
}

#[cfg(windows)]
fn obstructed_line(fault: std::convert::Infallible) -> String {
    match fault {}
}

#[cfg(any(target_os = "macos", windows))]
fn state_line(health: state::StateHealth) -> Option<String> {
    use state::StateHealth;
    match health {
        StateHealth::Ok => None,
        StateHealth::Corrupt => Some(
            "halvo-capture: state_file=corrupt; capture stays paused and the file is left as it is"
                .to_owned(),
        ),
        StateHealth::NewerVersion(version) => Some(format!(
            "halvo-capture: state_file=newer_version version={version}; capture stays paused and the file is left as it is"
        )),
        StateHealth::WriteFailed(kind) => {
            Some(format!("halvo-capture: state_file=write_failed error={kind}"))
        }
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
    /// A Windows run whose first step failed: it read no key and opened no
    /// store; the text names the call and its code.
    #[error("{0}")]
    StartRefused(String),
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

    #[cfg(all(windows, debug_assertions))]
    #[test]
    fn a_windows_timed_run_takes_the_screen_switch_and_refuses_the_key_switch() {
        use halvo_control::proto::Platform;
        let dirs = dirs();
        let d = dirs.store.as_str();
        let Ok(Mode::TimedRun(run)) = timed(
            &dirs,
            &["5", "--store-dir", d, "--switch", "screen_activity=on"],
        ) else {
            panic!("the screen switch was refused");
        };
        let wanted = sources::starting(Platform::Windows, run.kind(), run.switches.applied());
        let refused = timed(&dirs, &["5", "--store-dir", d, "--switch", "key_class=on"]);
        let mut err = Vec::new();
        let status = refused
            .as_ref()
            .map_or_else(|refusal| usage_answer(refusal, &mut err), |_| 0);
        let text = String::from_utf8(err).unwrap();
        assert_eq!(
            run.switches.set_by_flags(),
            [(Switch::ScreenActivity, true)]
        );
        assert!(wanted.contains(&SourceName::Screen), "{wanted:?}");
        assert!(!wanted.contains(&SourceName::Keyboard), "{wanted:?}");
        assert_eq!(status, exit::USAGE, "{refused:?}: {text}");
        assert!(
            text.contains("--switch key_class=on is refused on Windows"),
            "{text}"
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

    #[cfg(debug_assertions)]
    #[test]
    fn the_service_run_takes_an_absolute_data_dir_or_the_accounts_own() {
        let dirs = dirs();
        let absolute = dirs.data.to_str().unwrap();
        let parsed = |line: &[&str]| parse(args(line), &dirs.account);
        assert_eq!(
            parsed(&["--service-run", "--data-dir", absolute]),
            Ok(Mode::ServiceRun {
                data_dir: DataDir::from_flag(absolute).unwrap()
            })
        );
        assert_eq!(
            parsed(&["--service-run"]),
            Ok(Mode::ServiceRun {
                data_dir: DataDir::for_account(&dirs.account).unwrap()
            })
        );
        for line in [
            &["--service-run", "--data-dir", "relative/path"][..],
            &["--service-run", "--data-dir", "/a/../b"][..],
            &["--service-run", "--data-dir"][..],
            &["--service-run", "--data-dir", "/a", "--data-dir", "/b"][..],
            &["--service-run", "--data-dir", absolute, "--lifeline"][..],
            &["--service-run", "--lifeline", "--lifeline"][..],
            &["--service-run", "--store-dir", "/a"][..],
            &["--service-run", "--no-press"][..],
            &["--service-run", "--no-press", "--lifeline"][..],
            &["--service-run", "--lifeline", "--no-press", "--no-press"][..],
            &[
                "--service-run",
                "--lifeline",
                "--data-dir",
                absolute,
                "--no-press",
            ][..],
        ] {
            let refused = parsed(line);
            let Err(refusal @ Refusal::Usage(_)) = refused else {
                panic!("{line:?} gave {refused:?}");
            };
            assert_eq!(usage_answer(&refusal, &mut Vec::new()), 64, "{line:?}");
        }
        let without = parse(args(&["--service-run"]), &FailedLookup(Ok(None)));
        let Err(refusal @ Refusal::NoServiceDir(_)) = without else {
            panic!("{without:?}");
        };
        assert_eq!(usage_answer(&refusal, &mut Vec::new()), 1);
        let mode = parsed(&["--service-run", "--data-dir", absolute]).unwrap();
        assert!(mode.reads_the_signal_flag());
        let lifeline = parsed(&["--service-run", "--lifeline", "--data-dir", absolute]);
        assert_eq!(
            lifeline,
            Ok(Mode::LifelineServiceRun {
                data_dir: DataDir::from_flag(absolute).unwrap(),
                behind: Behind::Unsaid,
            })
        );
        assert!(lifeline.unwrap().reads_the_signal_flag());
        assert_eq!(
            parsed(&["--service-run", "--lifeline"]),
            Ok(Mode::LifelineServiceRun {
                data_dir: DataDir::for_account(&dirs.account).unwrap(),
                behind: Behind::Unsaid,
            })
        );
        let no_press = parsed(&[
            "--service-run",
            "--lifeline",
            "--no-press",
            "--data-dir",
            absolute,
        ]);
        assert_eq!(
            no_press,
            Ok(Mode::LifelineServiceRun {
                data_dir: DataDir::from_flag(absolute).unwrap(),
                behind: Behind::NoPress,
            })
        );
        assert!(no_press.unwrap().reads_the_signal_flag());
        assert_eq!(
            parsed(&["--service-run", "--lifeline", "--no-press"]),
            Ok(Mode::LifelineServiceRun {
                data_dir: DataDir::for_account(&dirs.account).unwrap(),
                behind: Behind::NoPress,
            })
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_service_form_of_the_self_check_takes_its_data_dir_alone() {
        let dirs = dirs();
        let absolute = dirs.data.to_str().unwrap();
        let parsed = |line: &[&str]| parse(args(line), &dirs.account);
        assert_eq!(
            parsed(&["--self-check", "service-run", "--data-dir", absolute]),
            Ok(Mode::SelfCheckServiceRun {
                data_dir: DataDir::from_flag(absolute).unwrap()
            })
        );
        for line in [
            &["--self-check", "service-run"][..],
            &["--self-check", "service-run", "--data-dir"][..],
            &["--self-check", "service-run", "--data-dir", "relative"][..],
            &["--self-check", "service-run", "--data-dir", "/a", "x"][..],
            &["--self-check", "service-run", "--store-dir", "/a"][..],
        ] {
            assert!(
                matches!(parsed(line), Err(Refusal::Usage(_))),
                "{line:?} gave {:?}",
                parsed(line)
            );
        }
        let mode = parsed(&["--self-check", "service-run", "--data-dir", absolute]).unwrap();
        assert!(!mode.reads_the_signal_flag());
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
                "site_read=on",
                "--switch",
                "key_class=off",
            ],
        ) else {
            panic!("the flags were refused");
        };
        assert_eq!(
            run.switches.set_by_flags(),
            [(Switch::SiteRead, true), (Switch::KeyClass, false)]
        );
        let switches = run.switches.applied();
        assert!(switches.is_on(Switch::SiteRead));
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
            "halvo-capture: the delete is not finished: the folder roots could not be listed; the folder roots no longer stands at its path; store.key stays; a database file stays; other files of the store that stay: 2; 2 names were a link or an entry of another kind and were not followed\n"
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
            "halvo-capture: the delete is not finished: other files of the store that stay: 1\n"
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
                    && err.contains("other files of the store that stay: 1"),
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

    #[cfg(any(debug_assertions, target_os = "macos", windows))]
    #[test]
    fn the_default_data_dir_form_prints_the_directory_of_a_service_run_without_data_dir() {
        let dirs = dirs();
        let Ok(Mode::ServiceRun { data_dir: service }) =
            parse(args(&["--service-run"]), &dirs.account)
        else {
            panic!("--service-run did not parse as a service run");
        };
        let parsed = parse(args(&["--default-data-dir"]), &dirs.account);
        let Ok(Mode::DefaultDataDir { data_dir }) = parsed else {
            panic!("--default-data-dir parsed as {parsed:?}");
        };
        assert_eq!(data_dir, service);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = default_data_dir_answer(&data_dir, &mut out, &mut err);
        assert_eq!(
            (
                status,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap()
            ),
            (
                exit::CLEAN,
                format!("{}\n", dirs.data.display()),
                String::new()
            )
        );

        let refused = parse(args(&["--default-data-dir", "x"]), &dirs.account).unwrap_err();
        let mut err = Vec::new();
        assert_eq!(usage_answer(&refused, &mut err), exit::USAGE);
        assert!(String::from_utf8(err)
            .unwrap()
            .starts_with("halvo-capture: --default-data-dir takes no other word\n"));
        let refused = parse(args(&["--default-data-dir"]), &FailedLookup(Ok(None))).unwrap_err();
        let mut err = Vec::new();
        assert_eq!(usage_answer(&refused, &mut err), exit::ERROR);
        assert!(String::from_utf8(err)
            .unwrap()
            .starts_with("halvo-capture: the account gives no data directory: "));
        let line = DataDir::from_flag(dirs.data.join("a\nb")).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = default_data_dir_answer(&line, &mut out, &mut err);
        assert_eq!((status, out.len()), (exit::ERROR, 0));
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "halvo-capture: the account gives no data directory: the path holds a control character\n"
        );
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
            &["--service-run", "--data-dir", d, "--scripted-sources"][..],
            &["--self-check", "run-loop"][..],
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

    struct LateStore(ModelStore);

    impl MaintainStore for LateStore {
        fn expire_due(
            &mut self,
            now: std::time::SystemTime,
            max: usize,
        ) -> Result<Pass, MaintFault> {
            self.0.expire_due(now, max)
        }

        fn evict_to(
            &mut self,
            bound: super::port::ByteBound,
            max: usize,
        ) -> Result<Pass, MaintFault> {
            self.0.evict_to(bound, max)
        }

        fn usage(&mut self) -> Result<StoreUsage, MaintFault> {
            self.0.usage()
        }
    }

    impl CaptureStore for LateStore {
        fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
            std::thread::sleep(DRAIN_BOUND + Duration::from_secs(1));
            self.0.write_batch(write)
        }

        fn volume_free_bytes(&self) -> std::io::Result<u64> {
            self.0.volume_free_bytes()
        }
    }

    #[test]
    fn a_timed_run_whose_drain_wait_ran_out_exits_1_with_its_loss_line() {
        let late = 88;
        let keys = (0..batch::BATCH_MAX_EVENTS + late).map(|_| key()).collect();
        let mut scripted =
            ScriptedRun::new(vec![(SourceName::Input, ScriptedSource::emitting(keys))]);
        let stop = AtomicBool::new(false);
        let outcome = run_timed(
            &run_of(5, SecureInput::DoNotHold, &[]),
            &mut scripted,
            LateStore(ModelStore::new()),
            ManualClocks::at(noon()),
            &stop,
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(outcome.status, exit::ERROR, "{outcome:?}");
        let loss = outcome.loss.expect("no loss line");
        assert_eq!(loss.cause, Some(LossCause::DrainWaitRanOut));
        assert_eq!(
            loss.line,
            format!("halvo-capture: lost events={late} first=- last=- error=-")
        );
        assert_eq!(
            outcome.report.events_persisted,
            batch::BATCH_MAX_EVENTS as u64
        );
    }
}
