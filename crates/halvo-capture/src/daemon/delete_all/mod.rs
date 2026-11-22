//! The delete of a data directory, contracts.md C3.3 and C17.3: the offline
//! form `--delete-all --data-dir DIR`, the walk over the rows of the
//! registry, and the finish of an intent that a start finds.

#[cfg(test)]
mod process_tests;
#[cfg(all(test, windows))]
mod windows_tests;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_control::exit;
use halvo_control::paths::{one_line, DataDir, Entry, INTENT_FILE, STATE_FILE};

use super::custody_policy::{Custody, OpenAs, Selection};
use super::instance::InstanceLock;
pub use super::owner_pid::{owner_of, start_time_of, Owner};
use super::port::{
    Access, CaptureStore, DataDirStore, MaintainStore, OpenFault, Opened, Probe, StoreHost,
};
use super::state::{StateDir, StateFile, StateHealth};
use super::{DurableState, TailAnswer, TailReport, TailRoot};

/// One row of the registry of contracts.md C17 item 2 as the walk reads it:
/// the name of the entry under the data directory, its key files and its
/// owner's pid file, each a path under the data directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub name: &'static str,
    pub key_files: &'static [&'static str],
    pub owner_pid_file: Option<&'static str>,
}

impl From<&Entry> for Row {
    fn from(entry: &Entry) -> Row {
        Row {
            name: entry.name(),
            key_files: entry.key_files(),
            owner_pid_file: entry.owner_pid_file(),
        }
    }
}

/// The rows of halvo-control's registry, in table order.
pub fn rows() -> Vec<Row> {
    halvo_control::paths::ENTRIES
        .iter()
        .map(Row::from)
        .collect()
}

/// The rows whose files the store's own delete removes, root first
/// (contracts.md C11.4): the store and its roots.
const STORE_ROWS: [&str; 2] = ["store", "roots"];

/// The rows whose files stay (contracts.md C17 item 3, steps 3 and 5): run
/// keeps the run marker and the control socket, and logs keeps daemon.err,
/// which the delete truncates.
const KEPT_ROWS: [&str; 2] = ["run", "logs"];

/// The hands of the walk: what ends a row's owner, what destroys a key file
/// and what removes an entry. The product's are [`SystemHands`].
pub trait Hands {
    /// Ends `owner` when a process of its pid and start time runs; true when
    /// no such process runs after the call.
    fn end_owner(&self, owner: Owner) -> bool;

    /// Destroys the key file `file`, a path under `data_dir`, through the
    /// store's key-file helper; true when no entry stands at its name after.
    fn destroy_key_file(&self, data_dir: &Path, file: &str) -> bool;

    /// Removes the entry `name` of `data_dir` with every file under it, and
    /// answers how many entries stay under it, itself among them.
    fn remove_entry(&self, data_dir: &Path, name: &str) -> u64;
}

/// The product's hands.
pub struct SystemHands;

/// How long an owner has after SIGTERM before SIGKILL, and how long after
/// SIGKILL before the walk leaves its row.
#[cfg(unix)]
const OWNER_TERM_WAIT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(unix)]
const OWNER_KILL_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

impl Hands for SystemHands {
    fn end_owner(&self, owner: Owner) -> bool {
        owner.pid > 1 && end_by_signal(owner)
    }

    fn destroy_key_file(&self, data_dir: &Path, file: &str) -> bool {
        DataDirStore::at(data_dir).destroy_key_file(Path::new(file))
    }

    fn remove_entry(&self, data_dir: &Path, name: &str) -> u64 {
        let path = data_dir.join(name);
        let _ = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&path),
            Ok(_) => std::fs::remove_file(&path),
            Err(_) => Ok(()),
        };
        entries_under(&path)
    }
}

fn entries_under(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Err(_) => 0,
        Ok(meta) if !meta.is_dir() => 1,
        Ok(_) => {
            1 + std::fs::read_dir(path).map_or(0, |entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entries_under(&entry.path()))
                    .sum()
            })
        }
    }
}

fn runs(owner: Owner) -> bool {
    start_time_of(owner.pid) == Some(owner.started)
}

#[cfg(unix)]
fn gone_within(owner: Owner, wait: std::time::Duration) -> bool {
    let started = std::time::Instant::now();
    while runs(owner) {
        if started.elapsed() >= wait {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    true
}

#[cfg(unix)]
fn end_by_signal(owner: Owner) -> bool {
    let Ok(pid) = libc::pid_t::try_from(owner.pid) else {
        return true;
    };
    if !runs(owner) {
        return true;
    }
    // SAFETY: kill takes a pid and a signal number and touches no memory.
    unsafe { libc::kill(pid, libc::SIGTERM) };
    if gone_within(owner, OWNER_TERM_WAIT) {
        return true;
    }
    if runs(owner) {
        // SAFETY: as above.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    gone_within(owner, OWNER_KILL_WAIT)
}

#[cfg(windows)]
const OWNER_END_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(windows)]
fn end_by_signal(owner: Owner) -> bool {
    use windows::Win32::System::Threading::{
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
    let Some(process) = super::owner_pid::ProcessHandle::open(owner.pid, access) else {
        return !runs(owner);
    };
    if process.started() != Some(owner.started) {
        return true;
    }
    process.end_within(1, OWNER_END_WAIT)
}

#[cfg(not(any(unix, windows)))]
fn end_by_signal(owner: Owner) -> bool {
    !runs(owner)
}

/// What the offline delete reads besides its data directory: the rows of the
/// registry, the custody selection and the walk's hands.
pub struct Offline<'a> {
    pub rows: &'a [Row],
    pub select: fn(Option<Custody>) -> Selection,
    pub hands: &'a dyn Hands,
}

/// The offline form of contracts.md C3.3 with the product's rows, selection
/// and hands, as the binary runs it.
pub fn offline(dir: &DataDir, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let rows = rows();
    run(
        dir,
        &Offline {
            rows: &rows,
            select: super::custody_policy::of_this_build,
            hands: &SystemHands,
        },
        out,
        err,
    )
}

/// What the walk over the rows did: the rows handled and left, and whether
/// it removed an entry of the data directory itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Walked {
    removed: u64,
    left: u64,
    removed_an_entry: bool,
}

/// What steps 3 to 5 of contracts.md C17 item 3 did.
struct Finished {
    store: TailAnswer,
    walked: Walked,
    unknown: u64,
    not_synced: Vec<PathBuf>,
    intent_kept: bool,
}

impl Finished {
    fn store_report(&self) -> Option<&TailReport> {
        match &self.store {
            TailAnswer::Finished(report) | TailAnswer::NotFinished(report) => Some(report),
            TailAnswer::Held | TailAnswer::Failed(_) => None,
        }
    }

    fn whole(&self) -> bool {
        matches!(self.store, TailAnswer::Finished(_)) && self.walked.left == 0 && !self.intent_kept
    }
}

/// The line of an offline delete that did not finish whole and leaves its
/// intent.
const INTENT_STAYS_LINE: &str = "halvo-capture: delete-all.intent stays until a delete finishes; until then a start of the service run opens no store";

/// The line of a delete that another process holds back.
const BUSY_LINE: &str =
    "halvo-capture: another process holds the data directory or its store; this request deleted nothing";

/// The offline delete of `dir` with `how` (contracts.md C3.3): it prints
/// `data_dir=` and then `no_store`, or the `deleted` line of C2.6 without
/// `store_id=`, on `out`, and on `err` what stays. It takes the instance
/// lock first. It answers 0 once the root and store.key are gone and every
/// row was handled, 75 when another process holds the directory's instance
/// lock or its store, which this request then leaves as it found it, and 1
/// when something stays or the directory cannot be used.
pub fn run(dir: &DataDir, how: &Offline<'_>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let path = dir.path();
    let mut text = format!("data_dir={}\n", one_line(&path.to_string_lossy()));
    if let Err(cause) = dir.is_provisioned() {
        return print(
            out,
            err,
            &text,
            Some(format!("halvo-capture: {cause}")),
            exit::ERROR,
        );
    }
    if std::fs::symlink_metadata(path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound) {
        text.push_str("no_store\n");
        return print(out, err, &text, None, exit::CLEAN);
    }
    let _held = match InstanceLock::take(dir) {
        Ok(Some(lock)) => lock,
        Ok(None) => return print(out, err, &text, Some(BUSY_LINE.to_owned()), exit::BUSY),
        Err(error) => {
            let cause = format!(
                "halvo-capture: the data directory could not be opened: {}",
                error.kind()
            );
            return print(out, err, &text, Some(cause), exit::ERROR);
        }
    };
    if nothing_stands(path, how.rows) {
        text.push_str("no_store\n");
        return print(out, err, &text, None, exit::CLEAN);
    }
    let ran = match steps_of_the_delete(path, how, &mut || {}, &mut || clear_the_mark(path)) {
        Steps::Busy => return print(out, err, &text, Some(BUSY_LINE.to_owned()), exit::BUSY),
        Steps::Ran(ran) => ran,
    };
    if let Some(line) = &ran.line {
        text.push_str(line);
    }
    let status = if ran.whole { exit::CLEAN } else { exit::ERROR };
    let causes = (!ran.causes.is_empty()).then(|| ran.causes.join("\n"));
    print(out, err, &text, causes, status)
}

/// What the steps of a delete did in a data directory whose instance lock
/// their caller holds.
pub enum Steps {
    /// Another process holds the store: nothing was deleted, and an intent
    /// that these steps wrote was removed again.
    Busy,
    /// The steps ran to their end.
    Ran(Box<Ran>),
}

/// What steps 2 to 5 of contracts.md C17 item 3 did, field by field.
pub struct Ran {
    /// The `deleted` line of contracts.md C2.6 without `store_id=`, none when
    /// the store's delete answered no report.
    pub line: Option<String>,
    /// The report of the store's delete, none when it failed with no report.
    pub report: Option<TailReport>,
    pub entries_removed: u64,
    pub entries_left: u64,
    pub unknown_entries: u64,
    pub intent_written: bool,
    /// What stays, a line each for standard error; no line holds a path of
    /// the store.
    pub causes: Vec<String>,
    /// The offline form's exit 0: the store's delete finished, no row stays
    /// and no intent was kept by a failed removal.
    pub whole: bool,
    /// Whether every folder that the steps flush reached the drive, which a
    /// whole finish needs before its intent goes.
    pub synced: bool,
}

/// Steps 2 to 5 of contracts.md C17 item 3 in `path`, a data directory whose
/// instance lock the caller holds, with the rows, selection and hands of
/// `how`: the open of the store for the delete, the intent, then
/// `after_the_intent` before any removal (the service run writes there the
/// first record of a delete that went on past a source that did not stop,
/// ruling PL-3 part (3) item 5), the store's
/// delete, then the clear of the mark `store_made` by `clear` (which answers
/// a line when the clear failed) once that delete did not answer busy, so a
/// delete that removed nothing leaves the mark as it was, the walk over the
/// rows, the count of the entries that the registry does not know, the
/// truncation of daemon.err, the flushes and the removal of the intent after
/// a whole finish. The offline form runs
/// them after it takes the lock, and the service run, which holds the lock
/// for its life, on a delete request once its owner's thread has closed the
/// store.
pub fn steps_of_the_delete(
    path: &Path,
    how: &Offline<'_>,
    after_the_intent: &mut dyn FnMut(),
    clear: &mut dyn FnMut() -> Option<String>,
) -> Steps {
    let host = DataDirStore::at(path);
    let probe = host.probe();
    let handle = match open_for_the_delete(&host, probe, how.select) {
        Ok(handle) => handle,
        Err(Held) => return Steps::Busy,
    };
    let intent_stood = stands(&path.join(INTENT_FILE));
    let intent = write_intent(path, how.rows, probe);
    after_the_intent();
    let mut causes = Vec::new();
    let store = delete_the_store(&host, handle);
    if matches!(store, TailAnswer::Held) {
        if !intent_stood {
            let _ = host.remove_durably(INTENT_FILE);
        }
        return Steps::Busy;
    }
    if let Some(line) = clear() {
        causes.push(line);
    }
    let finished = steps_3_to_5(path, how.rows, how.hands, store);
    let ran = |line, report, causes| {
        Steps::Ran(Box::new(Ran {
            line,
            report,
            entries_removed: finished.walked.removed,
            entries_left: finished.walked.left,
            unknown_entries: finished.unknown,
            intent_written: intent.is_ok(),
            causes,
            whole: finished.whole(),
            synced: finished.not_synced.is_empty(),
        }))
    };
    let Some(report) = finished.store_report() else {
        if let TailAnswer::Failed(error) = &finished.store {
            causes.push(format!("halvo-capture: {error}"));
        }
        if stands(&path.join(INTENT_FILE)) {
            causes.push(INTENT_STAYS_LINE.to_owned());
        }
        return ran(None, None, causes);
    };
    let line = deleted_line(report, &finished, intent.is_ok());
    if let TailAnswer::NotFinished(report) = &finished.store {
        causes.push(match report.what_stays() {
            stays if stays.is_empty() => "halvo-capture: the delete is not finished".to_owned(),
            stays => format!("halvo-capture: the delete is not finished: {stays}"),
        });
    }
    if finished.walked.left > 0 {
        causes.push(format!(
            "halvo-capture: {} registered entries stay; a process that a pid file names did not end, or their files could not be removed",
            finished.walked.left
        ));
    }
    if !finished.whole() && stands(&path.join(INTENT_FILE)) {
        causes.push(INTENT_STAYS_LINE.to_owned());
    }
    for folder in &finished.not_synced {
        causes.push(format!(
            "halvo-capture: the folder {} was not flushed to the drive; the intent stays, and the next start finishes the delete",
            one_line(&folder.to_string_lossy())
        ));
    }
    ran(Some(line), Some(report.clone()), causes)
}

/// Another process holds the store.
struct Held;

/// A handle of the store, open for capture or for maintenance alone.
enum Handle {
    Capture(Box<dyn CaptureStore + Send>),
    Maintenance(Box<dyn MaintainStore + Send>),
}

impl Handle {
    fn delete_all(&mut self) -> TailAnswer {
        match self {
            Handle::Capture(store) => store.delete_all(),
            Handle::Maintenance(store) => store.delete_all(),
        }
    }
}

/// The open of a store that stands, as the custody selection `select`
/// opens it: read-write, for maintenance alone (a store that the selection
/// keeps from capture, whose handle deletes it and never writes it), or not
/// at all, when the selection refuses the store or the open fails. A store
/// that another process holds answers [`Held`].
fn open_for_the_delete(
    host: &DataDirStore,
    probe: Probe,
    select: fn(Option<Custody>) -> Selection,
) -> Result<Option<Handle>, Held> {
    let Probe::KeyFile { custody, .. } = probe else {
        return Ok(None);
    };
    let opened = match select(Some(custody)) {
        Selection::Open(OpenAs::ReadWrite) => host.open(Access::ReadWrite),
        Selection::Open(OpenAs::MaintenanceOnly) => host.open(Access::MaintenanceOnly),
        Selection::Open(OpenAs::Refused) | Selection::Create(_) => return Ok(None),
    };
    match opened {
        Opened::ReadWrite { store, .. } => Ok(Some(Handle::Capture(store))),
        Opened::MaintenanceOnly { store, .. } => Ok(Some(Handle::Maintenance(store))),
        Opened::Fault {
            fault: OpenFault::Locked,
            ..
        } => Err(Held),
        Opened::Fault { .. } => Ok(None),
    }
}

/// Step 3 for the store (contracts.md C11.4 and ruling ST-44 (a)): the
/// handle's delete when the store opened, and the tail on the directory
/// when it did not or when the handle's delete is not finished, with the
/// handle dropped first. What the tail cannot read, the count of keyed
/// segments and the root that the handle named, comes from the handle's
/// report.
fn delete_the_store(host: &DataDirStore, handle: Option<Handle>) -> TailAnswer {
    let Some(mut handle) = handle else {
        return host.delete();
    };
    let by_the_handle = handle.delete_all();
    drop(handle);
    if matches!(by_the_handle, TailAnswer::Finished(_)) {
        return by_the_handle;
    }
    let Some(read) = (match &by_the_handle {
        TailAnswer::Finished(report) | TailAnswer::NotFinished(report) => Some(report.clone()),
        TailAnswer::Held | TailAnswer::Failed(_) => None,
    }) else {
        return host.delete();
    };
    match host.delete() {
        TailAnswer::Finished(mut report) => {
            report.keyed_segments = report.keyed_segments.or(read.keyed_segments);
            if report.root == TailRoot::Unnamed && read.root == TailRoot::Destroyed {
                report.root = TailRoot::Destroyed;
            }
            TailAnswer::Finished(report)
        }
        TailAnswer::NotFinished(mut report) => {
            report.keyed_segments = report.keyed_segments.or(read.keyed_segments);
            if report.root == TailRoot::Unnamed && matches!(read.root, TailRoot::Left(_)) {
                report.root = read.root;
            }
            TailAnswer::NotFinished(report)
        }
        other => other,
    }
}

fn print(
    out: &mut dyn Write,
    err: &mut dyn Write,
    text: &str,
    cause: Option<String>,
    status: i32,
) -> i32 {
    let printed = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    if let Err(error) = &printed {
        let _ = writeln!(err, "halvo-capture: the report was not written: {error}");
    }
    if let Some(cause) = cause.filter(|cause| !cause.is_empty()) {
        let _ = writeln!(err, "{cause}");
    }
    match printed {
        Ok(()) => status,
        Err(_) => exit::ERROR,
    }
}

fn deleted_line(report: &TailReport, finished: &Finished, intent_written: bool) -> String {
    let line = report.deleted_line();
    format!(
        "{} entries_removed={} entries_left={} unknown_entries={} intent={}\n",
        line.trim_end(),
        finished.walked.removed,
        finished.walked.left,
        finished.unknown,
        if intent_written { "written" } else { "failed" },
    )
}

fn stands(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn holds_a_file(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Err(_) => false,
        Ok(meta) if !meta.is_dir() => true,
        Ok(_) => match std::fs::read_dir(path) {
            Err(_) => true,
            Ok(entries) => entries
                .filter_map(Result::ok)
                .any(|entry| holds_a_file(&entry.path())),
        },
    }
}

/// Whether `dir` holds nothing of a store (contracts.md C3.3): no store.key,
/// no database, no segment and no intent, no root file under roots/, the
/// mark `store_made` not set, and no entry of a row that the walk removes.
fn nothing_stands(dir: &Path, rows: &[Row]) -> bool {
    let store = dir.join("store");
    let store_files = ["store.key", "store.db", "store.db-wal", "store.db-journal"];
    !store_files.iter().any(|name| stands(&store.join(name)))
        && !holds_a_file(&store.join("segments"))
        && !stands(&dir.join(INTENT_FILE))
        && !holds_a_file(&dir.join("roots"))
        && !store_made(dir)
        && rows
            .iter()
            .filter(|row| !STORE_ROWS.contains(&row.name) && !KEPT_ROWS.contains(&row.name))
            .all(|row| !stands(&dir.join(row.name)))
}

fn store_made(dir: &Path) -> bool {
    std::fs::read(dir.join(STATE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|state| state["store_made"] == serde_json::Value::Bool(true))
}

fn custody_text(custody: Custody) -> &'static str {
    match custody {
        Custody::RootFile => "root_file",
        Custody::Dpapi => "dpapi",
        Custody::SecureEnclave => "secure_enclave",
    }
}

/// Step 2: the intent, one element per row that has a key file, written
/// through the durable-write helper (contracts.md C17 item 3).
fn write_intent(dir: &Path, rows: &[Row], probe: Probe) -> io::Result<()> {
    let entries: Vec<serde_json::Value> = rows
        .iter()
        .filter(|row| !row.key_files.is_empty())
        .map(|row| match (row.name, probe) {
            ("store", Probe::KeyFile { custody, id }) => serde_json::json!({
                "name": row.name,
                "custody_kind": custody_text(custody),
                "root_id": id.to_string(),
            }),
            (name, _) => serde_json::json!({ "name": name }),
        })
        .collect();
    let intent = serde_json::json!({
        "v": 1,
        "started_at": utc_text(SystemTime::now()),
        "entries": entries,
    });
    DurableState::writer().write_whole(
        &StateDir::open(&DataDirStore::at(dir).resolved_path()?)?,
        std::ffi::OsStr::new(INTENT_FILE),
        intent.to_string().as_bytes(),
    )
}

/// Step 2, after the intent: the clear of the mark `store_made`, only when
/// the state file stands and loads; never a new file. A failed clear is a
/// line and never stops the delete.
fn clear_the_mark(dir: &Path) -> Option<String> {
    if !stands(&dir.join(STATE_FILE)) {
        return None;
    }
    let folder = StateDir::open(&DataDirStore::at(dir).resolved_path().ok()?).ok()?;
    let mut state = StateFile::open(folder, DurableState::writer());
    if state.health() != StateHealth::Ok || !state.state().store_made {
        return None;
    }
    state.update(|state| state.store_made = false).err().map(|_| {
        "halvo-capture: state_file=write_failed: the mark store_made was not cleared in daemon-state.json".to_owned()
    })
}

/// Steps 3 to 5 of contracts.md C17 item 3 after the store's delete answered
/// `store`: the walk over the other rows, the count of the entries that the
/// registry does not know, the truncation of daemon.err, the flush of store/,
/// roots/ and, after the walk removed an entry, the data directory, and the
/// removal of the intent only after a whole finish: the store's delete
/// answered finished, no row stays and every flush succeeded. After any other
/// finish the intent stays, so that the next start tries again.
fn steps_3_to_5(dir: &Path, rows: &[Row], hands: &dyn Hands, store: TailAnswer) -> Finished {
    let store_done = matches!(store, TailAnswer::Finished(_));
    let walked = walk(dir, rows, store_done, hands);
    let unknown = unknown_entries(dir, rows);
    truncate_the_log(dir);
    let mut not_synced = Vec::new();
    let host = DataDirStore::at(dir);
    let mut folders = vec![PathBuf::from("store"), PathBuf::from("roots")];
    if walked.removed_an_entry {
        folders.push(PathBuf::new());
    }
    for folder in folders {
        if stands(&dir.join(&folder)) && host.sync_folder(&folder).is_err() {
            not_synced.push(folder);
        }
    }
    let intent_kept = matches!(store, TailAnswer::Finished(_))
        && walked.left == 0
        && not_synced.is_empty()
        && host.remove_durably(INTENT_FILE).is_err();
    Finished {
        store,
        walked,
        unknown,
        not_synced,
        intent_kept,
    }
}

/// Step 3 for every row, in table order and in the same way: the process
/// that its owner pid file names is ended before any unlink, its key files
/// go through the key-file helper, and its entry is removed. The store's
/// rows are the store's delete, which ran first; the kept rows keep their
/// files.
fn walk(dir: &Path, rows: &[Row], store_done: bool, hands: &dyn Hands) -> Walked {
    let mut walked = Walked::default();
    for row in rows {
        let handled = if STORE_ROWS.contains(&row.name) {
            store_done
        } else {
            walk_a_row(dir, row, hands, &mut walked)
        };
        if handled {
            walked.removed += 1;
        } else {
            walked.left += 1;
        }
    }
    walked
}

fn walk_a_row(dir: &Path, row: &Row, hands: &dyn Hands, walked: &mut Walked) -> bool {
    if let Some(pid_file) = row.owner_pid_file {
        match owner_of(&dir.join(pid_file)) {
            Ok(None) => {}
            Ok(Some(owner)) => {
                if !hands.end_owner(owner) {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    let mut handled = true;
    for file in row.key_files {
        if stands(&dir.join(file)) && !hands.destroy_key_file(dir, file) {
            handled = false;
        }
    }
    if !KEPT_ROWS.contains(&row.name) && stands(&dir.join(row.name)) {
        if hands.remove_entry(dir, row.name) == 0 {
            walked.removed_an_entry = true;
        } else {
            handled = false;
        }
    }
    handled
}

/// Step 4: the entries under `dir` that the registry does not know. The
/// state file and the intent are known; on Windows the instance lock's file
/// is too.
fn unknown_entries(dir: &Path, rows: &[Row]) -> u64 {
    let known = |name: &str| {
        rows.iter().any(|row| row.name == name)
            || name == STATE_FILE
            || name == INTENT_FILE
            || (cfg!(windows) && name == "daemon.lock")
    };
    std::fs::read_dir(dir).map_or(0, |entries| {
        let unknown = entries
            .filter_map(Result::ok)
            .filter(|entry| !entry.file_name().to_str().is_some_and(known))
            .count();
        u64::try_from(unknown).unwrap_or(u64::MAX)
    })
}

fn truncate_the_log(dir: &Path) {
    let log = dir.join("logs").join(halvo_control::paths::DAEMON_ERR);
    if std::fs::symlink_metadata(&log).is_ok_and(|meta| meta.is_file()) {
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&log);
    }
}

/// `at` as `YYYY-MM-DDTHH:MM:SSZ` in UTC.
fn utc_text(at: SystemTime) -> String {
    let seconds = at
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let of_day = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3_600,
        of_day % 3_600 / 60,
        of_day % 60
    )
}

/// What a start found at the intent's name and did with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtStart {
    /// No intent stands.
    NoIntent,
    /// The intent named the store whose store.key stands, or no store.key
    /// that names a store stood beside it: the delete was run from step 3.
    /// `whole` is true when the store's delete answered finished, every row
    /// was handled and every flush succeeded, and the intent was removed; when
    /// it is false the intent stays and this start opens no store.
    Finished { whole: bool },
    /// The intent named another root: it was removed and nothing else.
    Stale,
    /// The intent cannot be read, or is of a version that this build does not
    /// read: it was left as it is.
    Unreadable,
}

impl AtStart {
    /// The line for standard error, none when no intent stood.
    pub fn line(self) -> Option<&'static str> {
        match self {
            AtStart::NoIntent => None,
            AtStart::Finished { whole: true } => {
                Some("halvo-capture: a delete-all that was cut short was finished before the open")
            }
            AtStart::Finished { whole: false } => Some(
                "halvo-capture: a delete-all that was cut short could not be finished: delete-all.intent stays, no store is opened in this run, and the next start tries again",
            ),
            AtStart::Stale => {
                Some("halvo-capture: delete-all.intent named another store and was removed")
            }
            AtStart::Unreadable => Some(
                "halvo-capture: delete-all.intent cannot be read by this build and was left as it is",
            ),
        }
    }
}

/// The root id that an intent of version 1 names for the store, `Some(None)`
/// for one that names none, and `None` for a text that is no intent of
/// version 1.
fn intent_root(bytes: &[u8]) -> Option<Option<String>> {
    let intent: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if intent["v"].as_u64() != Some(1) {
        return None;
    }
    let entries = intent["entries"].as_array()?;
    Some(
        entries
            .iter()
            .find(|entry| entry["name"].as_str() == Some("store"))
            .and_then(|entry| entry["root_id"].as_str())
            .map(str::to_owned),
    )
}

/// The finish of an intent that a start of the service run finds in
/// `data_dir` (contracts.md C17 item 3, "At start"), before the open's first
/// call: an intent of store.key's root id, or one with no readable store.key
/// beside it (also where a database, segments or an unfinished create stand
/// with no key file), is finished from step 3 through `host`'s tail and the walk over
/// `rows` by `hands`, and `deleting` is called before that step; a tail that
/// another process holds back deletes nothing, and no row is walked; the
/// intent stays unless the finish is whole. An intent of another root is
/// removed and nothing else; an intent that cannot be read is left.
pub fn at_start(
    data_dir: &Path,
    rows: &[Row],
    hands: &dyn Hands,
    host: &dyn StoreHost,
    deleting: &dyn Fn(),
) -> AtStart {
    let bytes = match std::fs::read(data_dir.join(INTENT_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return AtStart::NoIntent,
        Err(_) => return AtStart::Unreadable,
    };
    let Some(named) = intent_root(&bytes) else {
        return AtStart::Unreadable;
    };
    let ours = match host.probe() {
        Probe::KeyFile { id, .. } => named == Some(id.to_string()),
        Probe::NoKeyFile | Probe::KeylessStore | Probe::UnfinishedCreate | Probe::Fault { .. } => {
            true
        }
    };
    if !ours {
        let _ = DataDirStore::at(data_dir).remove_durably(INTENT_FILE);
        return AtStart::Stale;
    }
    deleting();
    let store = host.delete();
    if matches!(store, TailAnswer::Held) {
        return AtStart::Finished { whole: false };
    }
    let finished = steps_3_to_5(data_dir, rows, hands, store);
    AtStart::Finished {
        whole: finished.whole() && finished.not_synced.is_empty(),
    }
}

/// The start's call of the service run on `data_dir`: [`at_start`] with the
/// product's rows and hands, whose line goes to standard error.
pub fn at_start_of(data_dir: &Path) -> super::owner::open::AtStartCall {
    let data_dir = data_dir.to_path_buf();
    Box::new(move |host, deleting| {
        let found = at_start(&data_dir, &rows(), &SystemHands, host, deleting);
        if let Some(line) = found.line() {
            let _ = writeln!(io::stderr(), "{line}");
        }
        found
    })
}
