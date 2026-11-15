use std::ffi::OsString;
use std::fmt::Write as _;

use crate::client::ClientError;
use crate::model::{StorageCap, StoreId};
use crate::paths::{DataDir, ROOT_FILE_EXT};
use crate::proto::{
    AlreadyDeleted, Cap, CapSet, DaemonStatus, Deleted, ErrorCode, InventoryReply, Reply, Request,
    RootDestroyed, StateFile, Stopped,
};

/// The most bytes of a peer's text that ctl prints on one line of standard error.
pub const PEER_TEXT_MAX_BYTES: usize = 200;

pub const USAGE: &str = "usage: halvo-capture ctl [--data-dir DIR] status [--indicator] | inventory | pause | resume | stop | delete-all (STORE_ID | -) | set-cap BYTES";

/// One verb of the grammar of contracts.md C2.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Status { indicator: bool },
    Inventory,
    Pause,
    Resume,
    Stop,
    DeleteAll(Option<StoreId>),
    SetCap(StorageCap),
}

impl Verb {
    pub const fn request(self) -> Request {
        match self {
            Verb::Status { indicator } => Request::Status { indicator },
            Verb::Inventory => Request::Inventory,
            Verb::Pause => Request::Pause,
            Verb::Resume => Request::Resume,
            Verb::Stop => Request::Stop,
            Verb::DeleteAll(store) => Request::DeleteAll { store },
            Verb::SetCap(bytes) => Request::SetStorageCap { bytes },
        }
    }
}

/// `halvo-capture ctl [--data-dir DIR] VERB`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctl {
    data_dir: Option<DataDir>,
    pub verb: Verb,
}

impl Ctl {
    /// The directory of `--data-dir`, or without the flag `DataDir::for_current_user()`.
    pub fn data_dir(&self) -> Result<DataDir, crate::paths::PathError> {
        match &self.data_dir {
            Some(dir) => Ok(dir.clone()),
            None => DataDir::for_current_user(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(pub String);

impl UsageError {
    pub fn printed(&self) -> Printed {
        Printed {
            stdout: String::new(),
            stderr: format!("halvo-capture: {}\n{USAGE}\n", self.0),
            status: crate::exit::USAGE,
        }
    }
}

/// Parses the words after `ctl`. Every word must be UTF-8; `--data-dir` takes an absolute
/// directory; `set-cap` takes decimal digits between the two bounds of the cap.
pub fn parse(args: &[OsString]) -> Result<Ctl, UsageError> {
    let usage = |why: &str| UsageError(why.to_string());
    let words: Vec<&str> = args
        .iter()
        .map(|word| word.to_str())
        .collect::<Option<_>>()
        .ok_or_else(|| usage("an argument is not UTF-8"))?;
    let (data_dir, rest) = match words.as_slice() {
        ["--data-dir", dir, rest @ ..] => (
            Some(DataDir::from_flag(*dir).map_err(|e| usage(&format!("--data-dir: {e}")))?),
            rest,
        ),
        ["--data-dir"] => return Err(usage("--data-dir needs a directory")),
        rest => (None, rest),
    };
    let verb = match rest {
        ["status"] => Verb::Status { indicator: false },
        ["status", "--indicator"] => Verb::Status { indicator: true },
        ["inventory"] => Verb::Inventory,
        ["pause"] => Verb::Pause,
        ["resume"] => Verb::Resume,
        ["stop"] => Verb::Stop,
        ["delete-all", "-"] => Verb::DeleteAll(None),
        ["delete-all", id] => {
            Verb::DeleteAll(Some(id.parse().map_err(|_| {
                usage("delete-all takes the store id that status shows, or -")
            })?))
        }
        ["set-cap", bytes] => Verb::SetCap(parse_cap(bytes)?),
        _ => return Err(usage("not a verb of ctl")),
    };
    Ok(Ctl { data_dir, verb })
}

fn parse_cap(text: &str) -> Result<StorageCap, UsageError> {
    let bounds = || {
        UsageError(format!(
            "set-cap takes BYTES from {} to {}",
            StorageCap::MIN.bytes(),
            StorageCap::MAX.bytes()
        ))
    };
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bounds());
    }
    let bytes: u64 = text.parse().map_err(|_| bounds())?;
    StorageCap::from_bytes(bytes).map_err(|_| bounds())
}

/// What `ctl` prints and the status it exits with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printed {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn or_dash<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_string(), |v| v.to_string())
}

fn or_unknown<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "unknown".to_string(), |v| v.to_string())
}

/// The status text of contracts.md C2.3, one token group per line in its order.
pub fn status_text(status: &DaemonStatus) -> String {
    let f = status.fields();
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };
    line(format!("protocol={}", crate::proto::PROTOCOL_VERSION));
    line(format!("daemon_version={}", f.daemon_version));
    line(format!("pid={}", f.pid));
    line(format!("started_at={}", f.started_at));
    line(format!("rss_peak_bytes={}", f.rss_peak_bytes));
    line(format!("parent={}", f.parent.as_str()));
    line(format!("desired={}", f.desired.as_str()));
    line(format!("capture={}", f.capture.as_str()));
    line(format!(
        "capture_reason={}",
        or_dash(f.capture_reason.map(|r| r.as_str()))
    ));
    line(format!("lease={}", f.lease.as_str()));
    line(format!("store={}", f.store.as_str()));
    line(format!("store_id={}", or_dash(f.store_id)));
    line(format!(
        "store_error={}",
        or_dash(f.store_error.map(|e| e.as_str()))
    ));
    line(format!("store_retry_in_s={}", or_dash(f.store_retry_in_s)));
    line(format!("store_bytes={}", or_dash(f.store_bytes)));
    line(format!(
        "store_logical_bytes={}",
        or_dash(f.store_logical_bytes)
    ));
    line(format!(
        "cap_bytes={}",
        match f.cap_bytes {
            Cap::Known(cap) => cap.bytes().to_string(),
            Cap::Unknown => "unknown".to_string(),
        }
    ));
    line(format!("drain={}", f.drain.as_str()));
    line(format!(
        "drain_error={}",
        or_dash(f.drain_error.map(|e| e.as_str()))
    ));
    line(format!("drain_since={}", or_dash(f.drain_since)));
    line(format!("events_pending={}", f.events_pending));
    line(format!("events_lost={}", f.events_lost));
    line(format!("dropped_frames={}", f.dropped_frames));
    line(format!("pointer_coalesced={}", f.pointer_coalesced));
    line(format!("tap_disabled_timeout={}", f.tap_disabled_timeout));
    line(format!(
        "tap_disabled_user_input={}",
        f.tap_disabled_user_input
    ));
    line(format!("late_events={}", f.late_events));
    line(format!("max_lag_ms={}", f.max_lag_ms));
    if let Some(count) = f.secure_input_keystrokes {
        line(format!("secure_input_keystrokes={count}"));
    }
    if let Some(withheld) = f.keys_withheld {
        line(format!("keys_withheld={}", yes_no(withheld)));
    }
    line(format!("title_read_errors={}", f.title_read_errors));
    line(format!("last_flush_at={}", or_dash(f.last_flush_at)));
    line(format!("expiry={}", f.expiry.as_str()));
    line(format!(
        "expiry_error={}",
        or_dash(f.expiry_error.map(|e| e.as_str()))
    ));
    line(format!("last_expiry_at={}", or_dash(f.last_expiry_at)));
    line(format!("last_eviction_at={}", or_dash(f.last_eviction_at)));
    line(format!("last_stop={}", f.last_stop.as_str()));
    line(format!(
        "last_stop_newest_capture={}",
        or_dash(f.last_stop_newest_capture)
    ));
    line(format!(
        "last_stop_kind={}",
        or_dash(f.last_stop_kind.map(|k| k.as_str()))
    ));
    let lost = &f.last_stop_lost;
    line(format!(
        "last_stop_lost events={} first={} last={} error={} final={}",
        lost.events,
        or_dash(lost.first),
        or_dash(lost.last),
        or_dash(lost.error.as_ref()),
        yes_no(lost.final_figures)
    ));
    for source in &f.last_stop_sources {
        line(format!(
            "last_stop_source name={} error={}",
            source.name.as_str(),
            source.error
        ));
    }
    line(format!("state_file={}", f.state_file.as_str()));
    line(format!("platform={}", f.platform.as_str()));
    let enabled: Vec<&str> = f.enabled_sources.iter().map(|s| s.as_str()).collect();
    line(format!(
        "enabled_sources={}",
        if enabled.is_empty() {
            "-".to_string()
        } else {
            enabled.join(",")
        }
    ));
    for switch in &f.switches {
        line(format!(
            "switch name={} value={}",
            switch.name.as_str(),
            switch.value.as_str()
        ));
    }
    for permission in &f.permissions {
        line(format!(
            "permission name={} grant={} at_start={} asked={} once_granted={}",
            permission.name.as_str(),
            permission.grant.as_str(),
            permission.at_start.as_str(),
            yes_no(permission.asked),
            yes_no(permission.once_granted)
        ));
    }
    for source in &f.sources {
        let mut text = format!(
            "source name={} state={}",
            source.name.as_str(),
            source.state.name().as_str()
        );
        if let Some(reason) = source.state.reason() {
            let _ = write!(text, " reason={}", reason.as_str());
        }
        if let Some(error) = source.state.error() {
            let _ = write!(text, " error={error}");
        }
        line(text);
    }
    for kind in &f.kinds {
        line(format!(
            "kind name={} dropped={} refused={}",
            kind.name.as_str(),
            kind.dropped,
            kind.refused
        ));
    }
    out
}

/// The inventory text of contracts.md C2.4.
pub fn inventory_text(inventory: &InventoryReply) -> String {
    let mut out = format!("store_id={}\n", inventory.store());
    for row in inventory.rows().rows() {
        let _ = writeln!(
            out,
            "row day={} modality={} events={} bytes={}",
            row.day,
            row.modality.as_str(),
            row.events,
            row.bytes
        );
    }
    let totals = inventory.totals();
    let _ = writeln!(
        out,
        "total events={} bytes={} disk_bytes={}",
        totals.events, totals.bytes, totals.disk_bytes
    );
    out
}

/// The `deleted` line of contracts.md C2.6.
pub fn deleted_text(deleted: &Deleted) -> String {
    let r = deleted.report.fields();
    format!(
        "deleted segments_shredded={} keys_destroyed={} root_destroyed={} root_id={} files_left={} database_left={} not_followed={} entries_removed={} entries_left={} unknown_entries={} intent={} store_id={}\n",
        or_unknown(r.segments_shredded),
        or_unknown(r.keys_destroyed),
        r.root_destroyed.as_str(),
        or_dash(r.root_id),
        r.files_left,
        yes_no(r.database_left),
        r.not_followed,
        r.entries_removed,
        r.entries_left,
        r.unknown_entries,
        r.intent.as_str(),
        or_dash(deleted.store),
    )
}

pub fn already_deleted_text(already: &AlreadyDeleted) -> String {
    format!("already_deleted store_id={}\n", already.current)
}

/// What ctl says on standard error for `delete_refused` (ruling PL-8 part (4)(c)).
pub const DELETE_REFUSED_TEXT: &str = "this request deleted nothing: the store id in the request is not the id that the status shows, or one of the two is missing; an earlier delete may already have removed that store; read the status and ask again";

pub fn stopped_text(stopped: &Stopped) -> String {
    format!(
        "stopped flushed={} events_lost={}\n",
        yes_no(stopped.flushed),
        stopped.events_lost
    )
}

pub fn cap_set_text(set: &CapSet) -> String {
    let cap = match set.status.fields().cap_bytes {
        Cap::Known(cap) => cap.bytes().to_string(),
        Cap::Unknown => "unknown".to_string(),
    };
    format!("cap_bytes={cap} evicting={}\n", yes_no(set.evicting))
}

/// A peer's text as ctl prints it: every character outside ASCII 0x20 to 0x7e becomes `?`, and
/// the text is cut to [`PEER_TEXT_MAX_BYTES`].
pub fn peer_text(text: &str) -> String {
    text.chars()
        .map(|c| if (' '..='~').contains(&c) { c } else { '?' })
        .take(PEER_TEXT_MAX_BYTES)
        .collect()
}

fn failed(message: impl std::fmt::Display, status: i32) -> Printed {
    Printed {
        stdout: String::new(),
        stderr: format!("halvo-capture: {message}\n"),
        status,
    }
}

fn printed(stdout: String, status: i32) -> Printed {
    Printed {
        stdout,
        stderr: String::new(),
        status,
    }
}

/// What `ctl VERB` prints for the reply or the error of the client, with the exit codes of
/// contracts.md C2.5 to C2.7.
pub fn print(verb: &Verb, result: Result<Reply, ClientError>, data_dir: &DataDir) -> Printed {
    let reply = match result {
        Ok(reply) => reply,
        Err(ClientError::Daemon(error)) => Reply::Error(error),
        Err(ClientError::Unreachable { endpoint }) => {
            return failed(
                format!("capture service not running at {endpoint}"),
                crate::exit::UNREACHABLE,
            )
        }
        Err(ClientError::Unsupported) => {
            return failed(
                "the control channel is not available on Windows yet",
                crate::exit::ERROR,
            )
        }
        Err(ClientError::Protocol(text)) => {
            return failed(
                peer_text(&format!("the reply could not be read: {text}")),
                crate::exit::ERROR,
            )
        }
        Err(other) => return failed(other, crate::exit::ERROR),
    };
    match (verb, reply) {
        (_, Reply::Error(error)) => {
            let status = match error.code {
                ErrorCode::Busy => crate::exit::BUSY,
                ErrorCode::InvalidArgument => crate::exit::USAGE,
                _ => crate::exit::ERROR,
            };
            failed(peer_text(&error.message), status)
        }
        (Verb::Status { .. }, Reply::Status(status)) => {
            printed(status_text(&status), crate::exit::CLEAN)
        }
        (Verb::Inventory, Reply::Inventory(inventory)) => {
            printed(inventory_text(&inventory), crate::exit::CLEAN)
        }
        (Verb::Pause | Verb::Resume, Reply::CaptureChanged(status)) => {
            let f = status.fields();
            let (change, name) = if *verb == Verb::Pause {
                (crate::proto::CaptureChange::Pause, "pause")
            } else {
                (crate::proto::CaptureChange::Resume, "resume")
            };
            let reached = crate::proto::reaches_the_end(change, &status);
            let stdout = format!("capture={}\n", f.capture.as_str());
            let mut stderr = String::new();
            if *verb == Verb::Pause && f.capture == crate::proto::Capture::Pausing {
                stderr.push_str(
                    "halvo-capture: capture did not stop in time; the capture service is ending\n",
                );
            } else if !reached {
                stderr.push_str(&format!(
                    "halvo-capture: the capture service answered desired={} capture={}, not the state that {name} asks for\n",
                    f.desired.as_str(),
                    f.capture.as_str()
                ));
            }
            let written = f.state_file == StateFile::Ok;
            if !written {
                stderr.push_str(&format!(
                    "halvo-capture: the state file was not written: {}\n",
                    data_dir.state_file().display()
                ));
            }
            let status = if reached && written {
                crate::exit::CLEAN
            } else {
                crate::exit::ERROR
            };
            Printed {
                stdout,
                stderr,
                status,
            }
        }
        (Verb::Stop, Reply::Stopped(stopped)) => printed(
            stopped_text(&stopped),
            if stopped.flushed {
                crate::exit::CLEAN
            } else {
                crate::exit::ERROR
            },
        ),
        (Verb::DeleteAll(_), Reply::Deleted(deleted)) => {
            let stdout = deleted_text(&deleted);
            let report = deleted.report.fields();
            match (report.root_destroyed, report.root_id) {
                (RootDestroyed::No, Some(root)) => Printed {
                    stdout,
                    stderr: format!(
                        "halvo-capture: the root survived the delete; remove its file {}\n",
                        data_dir
                            .roots_dir()
                            .join(format!("{root}.{ROOT_FILE_EXT}"))
                            .display()
                    ),
                    status: crate::exit::ERROR,
                },
                _ => printed(stdout, crate::exit::CLEAN),
            }
        }
        (Verb::DeleteAll(armed), Reply::AlreadyDeleted(already))
            if crate::client::already_deleted_answers(*armed, &already) =>
        {
            printed(already_deleted_text(&already), crate::exit::CLEAN)
        }
        (Verb::DeleteAll(_), Reply::DeleteRefused) => Printed {
            stdout: "delete_refused\n".to_string(),
            stderr: format!("halvo-capture: {DELETE_REFUSED_TEXT}\n"),
            status: crate::exit::ERROR,
        },
        (Verb::SetCap(_), Reply::CapSet(set)) => printed(cap_set_text(&set), crate::exit::CLEAN),
        (_, other) => failed(
            format!("the daemon answered {} to this verb", other.kind()),
            crate::exit::ERROR,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lf(checked_out: &str) -> String {
        checked_out.replace("\r\n", "\n")
    }

    const NO_RECORD: &str =
        "last_stop_kind=-\nlast_stop_lost events=0 first=- last=- error=- final=yes\n";

    /// gate-2's `deleted` fixture with the count `not_followed=0` of ruling ST-30 after
    /// `database_left`, until gate-3 adds it to the file.
    fn deleted_fixture(checked_out: &str) -> String {
        let text = lf(checked_out);
        if text.contains(" not_followed=") {
            return text;
        }
        let at = text.find(" entries_removed=").unwrap();
        format!("{} not_followed=0{}", &text[..at], &text[at..])
    }

    /// gate-2's status fixture with the two record lines of C2.3, `last_stop_kind` and
    /// `last_stop_lost`, after `last_stop_newest_capture`, until gate-3 adds them to the file.
    fn status_fixture(checked_out: &str) -> String {
        let text = lf(checked_out);
        if text.contains("\nlast_stop_kind=") {
            return text;
        }
        let at = text.find("\nlast_stop_newest_capture=").unwrap() + 1;
        let end = at + text[at..].find('\n').unwrap() + 1;
        format!("{}{NO_RECORD}{}", &text[..end], &text[end..])
    }
    use crate::client::ClientError;
    use crate::model::{StorageCap, StoreId};
    use crate::proto::{
        AlreadyDeleted, Cap, CapSet, Capture, CaptureReason, DaemonStatus, DeleteFields,
        DeleteReport, Deleted, ErrorCode, ErrorReply, IntentState, Lease, Platform, Reason, Reply,
        RootDestroyed, SourceError, SourceLine, SourceName, SourceState, Stopped, StoreErrorKind,
        StoreState,
    };
    use crate::testing::{sample_delete_report, sample_inventory, sample_status};
    use std::ffi::OsString;

    const STATUS_MACOS: &str =
        include_str!("../../../scripts/r1-gate/fixtures/c2-status-macos.txt");
    const STATUS_WINDOWS: &str =
        include_str!("../../../scripts/r1-gate/fixtures/c2-status-windows.txt");
    const STATUS_NO_INDICATOR: &str =
        include_str!("../../../scripts/r1-gate/fixtures/c2-status-no-indicator.txt");
    const STATUS_UNSUPPORTED_VERSION: &str =
        include_str!("../../../scripts/r1-gate/fixtures/c2-status-unsupported-version.txt");
    const INVENTORY: &str = include_str!("../../../scripts/r1-gate/fixtures/c2-inventory.txt");
    const DELETED: &str = include_str!("../../../scripts/r1-gate/fixtures/c2-deleted.txt");
    const ALREADY_DELETED: &str =
        include_str!("../../../scripts/r1-gate/fixtures/c2-already-deleted.txt");
    const STOPPED: &str = include_str!("../../../scripts/r1-gate/fixtures/c2-stopped.txt");
    const UNREACHABLE: &str = include_str!("../../../scripts/r1-gate/fixtures/c2-unreachable.txt");

    const FRESH: StoreId = StoreId::from_bytes([
        0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32,
        0x10,
    ]);

    fn edited(
        platform: Platform,
        edit: impl FnOnce(&mut crate::proto::StatusFields),
    ) -> DaemonStatus {
        let mut fields = sample_status(platform).into_fields();
        edit(&mut fields);
        DaemonStatus::new(fields).unwrap()
    }

    fn stopped_sources(f: &mut crate::proto::StatusFields) {
        for line in &mut f.sources {
            if line.state == SourceState::Running {
                line.state = SourceState::Stopped { by_user: false };
            }
        }
    }

    const C2_5_LINE: &str =
        "halvo-capture: capture did not stop in time; the capture service is ending\n";

    /// A status whose `desired`, `capture` and `state_file` are the ones given, with the lease,
    /// the sources and the reason that `capture` needs; `None` when the type refuses it.
    fn capture_status(
        desired: crate::model::CaptureState,
        capture: Capture,
        state_file: StateFile,
    ) -> Option<DaemonStatus> {
        let mut f = sample_status(Platform::Macos).into_fields();
        f.desired = desired;
        f.capture = capture;
        f.capture_reason = None;
        f.state_file = state_file;
        match capture {
            Capture::NoIndicator => {
                f.lease = crate::proto::Lease::Lapsed;
                stopped_sources(&mut f);
            }
            Capture::Paused | Capture::AwaitingResume => stopped_sources(&mut f),
            Capture::Faulted => {
                stopped_sources(&mut f);
                f.capture_reason = Some(CaptureReason::NoSource);
            }
            Capture::Partial => {
                f.sources[2].state = SourceState::Failed {
                    reason: crate::proto::Reason::Permission,
                    error: None,
                };
            }
            Capture::Starting | Capture::Running | Capture::Pausing => {}
        }
        DaemonStatus::new(f).ok()
    }

    /// The rows of ruling PL-8 part (5)(a) whose status reaches the verb's end.
    const REACHED: [(&str, &str, &str); 6] = [
        ("pause", "paused", "paused"),
        ("resume", "running", "starting"),
        ("resume", "running", "running"),
        ("resume", "running", "partial"),
        ("resume", "running", "faulted"),
        ("resume", "running", "no_indicator"),
    ];

    #[test]
    fn pause_and_resume_exit_0_only_when_the_reply_reaches_the_verb_s_end_and_the_state_file_was_written(
    ) {
        use crate::model::CaptureState;
        let mut rows = 0;
        for (verb, name) in [(Verb::Pause, "pause"), (Verb::Resume, "resume")] {
            for desired in [CaptureState::Running, CaptureState::Paused] {
                for &capture in Capture::ALL {
                    for state_file in [
                        StateFile::Ok,
                        StateFile::Corrupt,
                        StateFile::NewerVersion,
                        StateFile::WriteFailed,
                    ] {
                        let Some(status) = capture_status(desired, capture, state_file) else {
                            continue;
                        };
                        rows += 1;
                        let (d, c) = (desired.as_str(), capture.as_str());
                        let row =
                            format!("{name} desired={d} capture={c} state_file={state_file:?}");
                        let reached = REACHED.contains(&(name, d, c));
                        let mut stderr = String::new();
                        if verb == Verb::Pause && capture == Capture::Pausing {
                            stderr.push_str(C2_5_LINE);
                        } else if !reached {
                            stderr.push_str(&format!("halvo-capture: the capture service answered desired={d} capture={c}, not the state that {name} asks for\n"));
                        }
                        if state_file != StateFile::Ok {
                            stderr.push_str(&format!(
                                "halvo-capture: the state file was not written: {}\n",
                                data_dir().state_file().display()
                            ));
                        }
                        let want = (
                            format!("capture={c}\n"),
                            stderr,
                            if reached && state_file == StateFile::Ok {
                                0
                            } else {
                                1
                            },
                        );
                        let printed = print(&verb, Ok(Reply::CaptureChanged(status)), &data_dir());
                        assert_eq!(
                            (printed.stdout, printed.stderr, printed.status),
                            want,
                            "row: {row}"
                        );
                    }
                }
            }
        }
        assert!(rows >= 80, "only {rows} rows were taken by the type");
    }

    #[test]
    fn c1_a_pause_answered_with_capture_running_exits_1() {
        use crate::model::CaptureState;
        let running =
            capture_status(CaptureState::Running, Capture::Running, StateFile::Ok).unwrap();
        let printed = print(
            &Verb::Pause,
            Ok(Reply::CaptureChanged(running)),
            &data_dir(),
        );
        assert_eq!(printed.stdout, "capture=running\n");
        assert_eq!(printed.stderr, "halvo-capture: the capture service answered desired=running capture=running, not the state that pause asks for\n");
        assert_eq!(printed.status, 1);
    }

    #[test]
    fn c2_a_resume_answered_with_awaiting_resume_paused_or_pausing_exits_1_with_no_line_of_c2_5() {
        use crate::model::CaptureState;
        for (desired, capture) in [
            (CaptureState::Running, Capture::AwaitingResume),
            (CaptureState::Paused, Capture::Paused),
            (CaptureState::Running, Capture::Pausing),
            (CaptureState::Paused, Capture::Pausing),
        ] {
            let status = capture_status(desired, capture, StateFile::Ok).unwrap();
            let printed = print(
                &Verb::Resume,
                Ok(Reply::CaptureChanged(status)),
                &data_dir(),
            );
            let (d, c) = (desired.as_str(), capture.as_str());
            assert_eq!(printed.stdout, format!("capture={c}\n"));
            assert_eq!(
                printed.stderr,
                format!("halvo-capture: the capture service answered desired={d} capture={c}, not the state that resume asks for\n"),
                "resume on desired={d} capture={c}"
            );
            assert_eq!(printed.status, 1, "resume on desired={d} capture={c}");
        }
    }

    #[test]
    fn a_pause_and_a_resume_whose_state_file_write_failed_exit_1_with_the_file_named() {
        use crate::model::CaptureState;
        for (verb, desired, capture) in [
            (Verb::Pause, CaptureState::Paused, Capture::Paused),
            (Verb::Resume, CaptureState::Running, Capture::Running),
        ] {
            let status = capture_status(desired, capture, StateFile::WriteFailed).unwrap();
            let printed = print(&verb, Ok(Reply::CaptureChanged(status)), &data_dir());
            assert_eq!(
                printed.stderr,
                format!(
                    "halvo-capture: the state file was not written: {}\n",
                    data_dir().state_file().display()
                ),
                "{verb:?}"
            );
            assert_eq!(printed.status, 1, "{verb:?}");
        }
    }

    fn data_dir() -> DataDir {
        DataDir::from_flag(if cfg!(windows) {
            r"C:\halvo-data"
        } else {
            "/halvo-data"
        })
        .unwrap()
    }

    #[test]
    fn the_macos_status_renders_gate_2_s_fixture_byte_for_byte() {
        assert_eq!(
            status_text(&sample_status(Platform::Macos)),
            status_fixture(STATUS_MACOS)
        );
    }

    #[test]
    fn the_windows_status_renders_gate_2_s_fixture_byte_for_byte() {
        assert_eq!(
            status_text(&sample_status(Platform::Windows)),
            status_fixture(STATUS_WINDOWS)
        );
    }

    #[test]
    fn a_lapsed_lease_renders_the_no_indicator_fixture() {
        let status = edited(Platform::Macos, |f| {
            f.capture = Capture::NoIndicator;
            f.lease = Lease::Lapsed;
            stopped_sources(f);
        });
        assert_eq!(status_text(&status), status_fixture(STATUS_NO_INDICATOR));
    }

    #[test]
    fn an_older_store_renders_the_unsupported_version_fixture() {
        let status = edited(Platform::Macos, |f| {
            f.capture = Capture::Faulted;
            f.capture_reason = Some(CaptureReason::Store);
            f.store = StoreState::UnsupportedVersion;
            f.store_error = Some(StoreErrorKind::Older);
            f.store_bytes = None;
            f.store_logical_bytes = None;
            f.last_flush_at = None;
            f.last_expiry_at = None;
            stopped_sources(f);
        });
        assert_eq!(
            status_text(&status),
            status_fixture(STATUS_UNSUPPORTED_VERSION)
        );
    }

    #[test]
    fn the_inventory_renders_its_fixture() {
        assert_eq!(inventory_text(&sample_inventory(Vec::new())), lf(INVENTORY));
    }

    #[test]
    fn the_deleted_and_the_already_deleted_replies_render_their_fixtures_with_not_followed_0_put_into_c2_deleted_until_gate_3_adds_it(
    ) {
        let deleted = Deleted {
            report: sample_delete_report(),
            store: Some(FRESH),
        };
        assert_eq!(deleted_text(&deleted), deleted_fixture(DELETED));
        assert_eq!(
            already_deleted_text(&AlreadyDeleted { current: FRESH }),
            lf(ALREADY_DELETED)
        );
    }

    #[test]
    fn the_stop_renders_its_fixture_and_exits_0_when_flushed() {
        let printed = print(
            &Verb::Stop,
            Ok(Reply::Stopped(Stopped {
                flushed: true,
                events_lost: 0,
            })),
            &data_dir(),
        );
        assert_eq!(printed.stdout, lf(STOPPED));
        assert_eq!(printed.status, crate::exit::CLEAN);
    }

    #[test]
    fn a_stop_that_did_not_flush_prints_its_loss_and_exits_1() {
        let printed = print(
            &Verb::Stop,
            Ok(Reply::Stopped(Stopped {
                flushed: false,
                events_lost: 3,
            })),
            &data_dir(),
        );
        assert_eq!(printed.stdout, "stopped flushed=no events_lost=3\n");
        assert_eq!(printed.status, 1);
    }

    #[test]
    fn no_daemon_prints_the_unreachable_fixture_and_exits_69() {
        let endpoint = "/Users/example/Library/Application Support/Halvo/run/control.sock";
        let printed = print(
            &Verb::Status { indicator: false },
            Err(ClientError::Unreachable {
                endpoint: endpoint.to_string(),
            }),
            &data_dir(),
        );
        assert_eq!(printed.stderr, lf(UNREACHABLE));
        assert_eq!(printed.stdout, "");
        assert_eq!(printed.status, 69);
    }

    #[test]
    fn every_status_fixture_has_its_line_counts_and_the_five_switches_in_order() {
        let count =
            |text: &str, prefix: &str| text.lines().filter(|l| l.starts_with(prefix)).count();
        let macos = lf(STATUS_MACOS);
        let switches: Vec<&str> = macos.lines().filter(|l| l.starts_with("switch ")).collect();
        assert_eq!(
            switches,
            [
                "switch name=title_text value=on",
                "switch name=key_class value=on",
                "switch name=pointer_class value=on",
                "switch name=screen_activity value=off",
                "switch name=site_read value=off"
            ]
        );
        for (text, sources, permissions) in [
            (lf(STATUS_MACOS), 5, 3),
            (lf(STATUS_WINDOWS), 6, 0),
            (lf(STATUS_NO_INDICATOR), 5, 3),
            (lf(STATUS_UNSUPPORTED_VERSION), 5, 3),
        ] {
            assert_eq!(count(&text, "source "), sources);
            assert_eq!(count(&text, "permission "), permissions);
            assert_eq!(count(&text, "switch "), 5);
            assert_eq!(count(&text, "kind "), 8);
        }
        assert!(lf(STATUS_WINDOWS).contains("\nsource name=keyboard state=off\n"));
        assert!(lf(STATUS_MACOS).contains("\nsource name=screen state=off\n"));
    }

    #[test]
    fn the_fixtures_hold_the_cause_the_leftovers_and_the_expiry_lines() {
        for key in [
            "store_error=",
            "expiry=",
            "expiry_error=",
            "last_expiry_at=",
        ] {
            assert!(lf(STATUS_MACOS).contains(key), "{key}");
        }
        for key in ["files_left=", "database_left=", "entries_left=", "intent="] {
            assert!(lf(DELETED).contains(key), "{key}");
        }
    }

    #[test]
    fn no_status_text_prints_the_custody_the_guard_or_the_filter() {
        for platform in [Platform::Macos, Platform::Windows] {
            let text = status_text(&sample_status(platform));
            for key in [
                "custody",
                "password_guard",
                "screen_filter",
                "onboarding",
                "ext",
            ] {
                assert!(
                    !text.lines().any(|l| l.starts_with(key)),
                    "{platform:?} prints {key}"
                );
            }
        }
    }

    #[test]
    fn a_failed_source_prints_its_reason_and_its_typed_error_last() {
        let status = edited(Platform::Macos, |f| {
            f.capture = Capture::Partial;
            f.sources[2] = SourceLine {
                name: SourceName::Input,
                state: SourceState::Failed {
                    reason: Reason::OsError,
                    error: Some(SourceError {
                        kind: "os_error".parse().unwrap(),
                        stage: "start".parse().unwrap(),
                        os_domain: "osstatus".parse().unwrap(),
                        os_code: -3801,
                    }),
                },
                unstopped: false,
            };
            f.sources[0].state = SourceState::Impaired {
                reason: Reason::TitleReadErrors,
                error: None,
            };
        });
        let text = status_text(&status);
        assert!(text.contains(
            "\nsource name=input state=failed reason=os_error error=os_error during start: osstatus -3801\n"
        ));
        assert!(text.contains("\nsource name=foreground state=impaired reason=title_read_errors\n"));
        let user = edited(Platform::Macos, |f| {
            f.enabled_sources.insert(1, SourceName::Screen);
            f.sources[1].state = SourceState::Stopped { by_user: true };
            f.capture = Capture::Partial;
        });
        assert!(status_text(&user).contains("\nsource name=screen state=stopped reason=user\n"));
    }

    #[test]
    fn an_unknown_cap_and_an_empty_source_set_print_their_words() {
        let status = edited(Platform::Macos, |f| {
            f.cap_bytes = Cap::Unknown;
            f.enabled_sources.clear();
            for line in &mut f.sources {
                line.state = SourceState::Off;
            }
        });
        let text = status_text(&status);
        assert!(text.contains("\ncap_bytes=unknown\n"));
        assert!(text.contains("\nenabled_sources=-\n"));
    }

    #[test]
    fn a_store_whose_key_file_does_not_parse_prints_no_id() {
        let status = edited(Platform::Macos, |f| {
            f.store = StoreState::Sealed;
            f.store_error = Some(StoreErrorKind::KeyFile);
            f.store_id = None;
        });
        assert!(status_text(&status).contains("\nstore=sealed\nstore_id=-\nstore_error=key_file\n"));
    }

    #[test]
    fn a_deleted_reply_with_no_new_store_and_unknown_counts_prints_dash_and_unknown() {
        let report = DeleteReport::new(DeleteFields {
            segments_shredded: None,
            keys_destroyed: None,
            root_destroyed: RootDestroyed::Unknown,
            root_id: None,
            files_left: 2,
            database_left: true,
            not_followed: 0,
            entries_removed: 3,
            entries_left: 1,
            unknown_entries: 5,
            intent: IntentState::Failed,
        })
        .unwrap();
        let deleted = Deleted {
            report,
            store: None,
        };
        let printed = print(
            &Verb::DeleteAll(None),
            Ok(Reply::Deleted(deleted.clone())),
            &data_dir(),
        );
        assert_eq!(
            printed.stdout,
            "deleted segments_shredded=unknown keys_destroyed=unknown root_destroyed=unknown root_id=- files_left=2 database_left=yes not_followed=0 entries_removed=3 entries_left=1 unknown_entries=5 intent=failed store_id=-\n"
        );
        assert_eq!(printed.status, 0);
    }

    #[test]
    fn a_surviving_root_exits_1_and_names_its_file() {
        let report = DeleteReport::new(DeleteFields {
            root_destroyed: RootDestroyed::No,
            root_id: Some(FRESH),
            ..*sample_delete_report().fields()
        })
        .unwrap();
        let printed = print(
            &Verb::DeleteAll(Some(FRESH)),
            Ok(Reply::Deleted(Deleted {
                report,
                store: None,
            })),
            &data_dir(),
        );
        assert!(printed
            .stdout
            .contains(" root_destroyed=no root_id=fedcba9876543210fedcba9876543210 "));
        assert_eq!(printed.status, 1);
        let root = data_dir()
            .roots_dir()
            .join("fedcba9876543210fedcba9876543210.root");
        assert!(
            printed.stderr.contains(root.to_str().unwrap()),
            "{}",
            printed.stderr
        );
    }

    #[test]
    fn already_deleted_exits_0_and_prints_the_current_id() {
        let printed = print(
            &Verb::DeleteAll(Some(crate::testing::store_id(9))),
            Ok(Reply::AlreadyDeleted(AlreadyDeleted { current: FRESH })),
            &data_dir(),
        );
        assert_eq!(
            printed.stdout,
            format!("already_deleted store_id={FRESH}\n")
        );
        assert_eq!(printed.status, 0);
    }

    fn one_clean_line(printed: &Printed) {
        let line = printed.stderr.strip_suffix('\n').expect("a line end");
        assert!(!line.contains('\n'), "{:?}", printed.stderr);
        assert!(
            line.bytes().all(|b| (0x20..=0x7e).contains(&b)),
            "{:?}",
            printed.stderr
        );
        assert!(line.starts_with("halvo-capture: "), "{:?}", printed.stderr);
        assert!(line.len() <= "halvo-capture: ".len() + PEER_TEXT_MAX_BYTES);
    }

    #[test]
    fn a2_an_error_message_of_the_peer_is_printed_as_one_line_of_printable_ascii() {
        let message = "\u{1b}]0;Halvo\u{7}\u{1b}[2K\rcapture=paused\nhalvo-capture: capture service not running at /x \u{e9}";
        let printed = print(
            &Verb::Pause,
            Ok(Reply::Error(ErrorReply {
                code: ErrorCode::Internal,
                message: message.into(),
            })),
            &data_dir(),
        );
        one_clean_line(&printed);
        assert_eq!(
            printed.stderr,
            "halvo-capture: ?]0;Halvo??[2K?capture=paused?halvo-capture: capture service not running at /x ?\n"
        );
        assert_eq!(printed.status, 1);
    }

    #[test]
    fn a2b_a_reply_kind_that_is_not_known_puts_no_byte_of_the_peer_beyond_printable_ascii_on_stderr(
    ) {
        let body =
            br#"{"v":1,"body":{"kind":"\u001b[2K\rcapture=paused\nhalvo-capture: deleted"}}"#;
        let error = crate::proto::decode_reply(body).unwrap_err();
        let printed = print(
            &Verb::Pause,
            Err(ClientError::Protocol(error.to_string())),
            &data_dir(),
        );
        one_clean_line(&printed);
        assert_eq!(printed.status, 1);
        assert_eq!(printed.stdout, "");
    }

    #[test]
    fn a2c_an_error_message_of_256_kib_is_cut_to_one_bounded_line() {
        let filler = "halvo-capture: x\n".repeat(14_000);
        for code in [
            ErrorCode::Internal,
            ErrorCode::Busy,
            ErrorCode::InvalidArgument,
        ] {
            let printed = print(
                &Verb::Status { indicator: false },
                Ok(Reply::Error(ErrorReply {
                    code,
                    message: filler.clone(),
                })),
                &data_dir(),
            );
            one_clean_line(&printed);
            assert_eq!(
                printed.stderr.len(),
                "halvo-capture: ".len() + PEER_TEXT_MAX_BYTES + 1
            );
            assert_eq!(
                printed.status,
                [1, 75, 64][[
                    ErrorCode::Internal,
                    ErrorCode::Busy,
                    ErrorCode::InvalidArgument
                ]
                .iter()
                .position(|c| *c == code)
                .unwrap()]
            );
        }
    }

    #[test]
    fn peer_text_turns_tab_us_del_and_0x80_into_a_question_mark_and_keeps_space_and_tilde() {
        for (given, printed) in [
            ("\u{9}", "?"),
            ("\u{1f}", "?"),
            ("\u{7f}", "?"),
            ("\u{80}", "?"),
            (" ", " "),
            ("~", "~"),
            ("a\u{9}b\u{1f}c\u{7f}d\u{80}e f~", "a?b?c?d?e f~"),
        ] {
            assert_eq!(peer_text(given), printed, "{given:?}");
        }
    }

    #[test]
    fn peer_text_prints_the_first_200_of_201_printable_bytes() {
        assert_eq!(PEER_TEXT_MAX_BYTES, 200);
        let given: String = (0..201u8).map(|i| char::from(b' ' + i % 95)).collect();
        assert_eq!(given.len(), 201);
        let printed = peer_text(&given);
        assert_eq!(printed.len(), 200);
        assert_eq!(printed, given[..200]);
    }

    #[test]
    fn a_data_directory_in_use_prints_its_one_line_with_exit_1() {
        let printed = print(
            &Verb::Pause,
            Err(ClientError::DataDirInUse {
                endpoint: "/halvo-data/run/control.sock".into(),
            }),
            &data_dir(),
        );
        assert_eq!(printed.stdout, "");
        assert_eq!(
            printed.stderr,
            "halvo-capture: the data directory is in use and no connection could be made at /halvo-data/run/control.sock; capture may be running\n"
        );
        assert_eq!(printed.status, 1);
    }

    #[test]
    fn ctl_s_own_paths_are_not_cleaned() {
        let endpoint = "/tmp/\u{e9}t\u{e9}/run/control.sock";
        let printed = print(
            &Verb::Stop,
            Err(ClientError::Unreachable {
                endpoint: endpoint.into(),
            }),
            &data_dir(),
        );
        assert_eq!(
            printed.stderr,
            format!("halvo-capture: capture service not running at {endpoint}\n")
        );
    }

    #[test]
    fn inventory_of_a_store_that_is_not_open_names_the_store_value_through_the_scripted_daemon() {
        for (store, error, text) in [
            (
                StoreState::Sealed,
                Some(StoreErrorKind::RootMissing),
                "sealed",
            ),
            (
                StoreState::Unavailable,
                Some(StoreErrorKind::Io),
                "unavailable",
            ),
            (StoreState::Opening, None, "opening"),
        ] {
            let status = edited(Platform::Macos, |f| {
                f.store = store;
                f.store_error = error;
                if store == StoreState::Opening {
                    f.capture = Capture::Starting;
                } else {
                    f.capture = Capture::Faulted;
                    f.capture_reason = Some(CaptureReason::Store);
                }
                stopped_sources(f);
            });
            let mut daemon = crate::testing::ScriptedDaemon::new(status);
            let body = daemon
                .answer_frame(&crate::proto::encode_request(
                    &crate::proto::Request::Inventory,
                ))
                .unwrap();
            let reply = crate::proto::decode_reply(&body).unwrap();
            let printed = print(&Verb::Inventory, Ok(reply), &data_dir());
            assert_eq!(
                printed.stderr,
                format!("halvo-capture: store unavailable: {text}\n")
            );
            assert_eq!(printed.status, 1);
            assert_eq!(printed.stdout, "");
        }
    }

    #[test]
    fn e2_an_error_of_the_typed_client_prints_and_exits_as_the_error_reply_does() {
        let mut wrong = Vec::new();
        for (code, message, want) in [
            (ErrorCode::Busy, "another mutation is in flight", 75),
            (
                ErrorCode::InvalidArgument,
                "delete-all - is taken only when the status shows no store id",
                64,
            ),
            (ErrorCode::Internal, "a\u{1b}[2Kb", 1),
        ] {
            let reply = ErrorReply {
                code,
                message: message.into(),
            };
            let as_reply = print(&Verb::Pause, Ok(Reply::Error(reply.clone())), &data_dir());
            let as_error = print(&Verb::Pause, Err(ClientError::Daemon(reply)), &data_dir());
            if as_error != as_reply || as_error.status != want {
                wrong.push(code.as_str());
            }
        }
        assert!(wrong.is_empty(), "{wrong:?}");
        let busy = print(
            &Verb::Pause,
            Err(ClientError::Daemon(ErrorReply {
                code: ErrorCode::Busy,
                message: "another mutation is in flight".into(),
            })),
            &data_dir(),
        );
        assert_eq!(
            busy.stderr,
            "halvo-capture: another mutation is in flight\n"
        );
    }

    #[test]
    fn busy_exits_75_an_invalid_argument_64_and_another_error_1_with_its_message() {
        for (code, status) in [
            (ErrorCode::Busy, 75),
            (ErrorCode::InvalidArgument, 64),
            (ErrorCode::StoreUnavailable, 1),
            (ErrorCode::StateFile, 1),
            (ErrorCode::Internal, 1),
            (ErrorCode::UnsupportedOp, 1),
            (ErrorCode::UnsupportedVersion, 1),
            (ErrorCode::PeerRejected, 1),
        ] {
            let printed = print(
                &Verb::Pause,
                Ok(Reply::Error(ErrorReply {
                    code,
                    message: "store unavailable: sealed".into(),
                })),
                &data_dir(),
            );
            assert_eq!(printed.status, status, "{code:?}");
            assert_eq!(printed.stdout, "");
            assert_eq!(printed.stderr, "halvo-capture: store unavailable: sealed\n");
        }
    }

    #[test]
    fn a_deadline_prints_no_reply_within_its_seconds_and_exits_1() {
        let printed = print(
            &Verb::DeleteAll(None),
            Err(ClientError::Deadline { seconds: 120 }),
            &data_dir(),
        );
        assert_eq!(printed.stderr, "halvo-capture: no reply within 120 s\n");
        assert_eq!(printed.status, 1);
    }

    #[test]
    fn on_a_system_without_a_channel_ctl_says_so_and_exits_1() {
        let printed = print(&Verb::Stop, Err(ClientError::Unsupported), &data_dir());
        assert_eq!(
            printed.stderr,
            "halvo-capture: the control channel is not available on Windows yet\n"
        );
        assert_eq!(printed.status, 1);
    }

    #[test]
    fn pause_and_resume_print_the_capture_state_after_they_settle() {
        let paused = edited(Platform::Macos, |f| {
            f.desired = crate::model::CaptureState::Paused;
            f.capture = Capture::Paused;
            stopped_sources(f);
        });
        let printed = print(&Verb::Pause, Ok(Reply::CaptureChanged(paused)), &data_dir());
        assert_eq!(printed.stdout, "capture=paused\n");
        assert_eq!(printed.status, 0);
        let printed = print(
            &Verb::Resume,
            Ok(Reply::CaptureChanged(sample_status(Platform::Macos))),
            &data_dir(),
        );
        assert_eq!(printed.stdout, "capture=running\n");
    }

    #[test]
    fn a_pause_whose_state_file_write_failed_prints_paused_names_the_file_and_exits_1() {
        let paused = edited(Platform::Macos, |f| {
            f.desired = crate::model::CaptureState::Paused;
            f.capture = Capture::Paused;
            f.state_file = crate::proto::StateFile::WriteFailed;
            stopped_sources(f);
        });
        let printed = print(&Verb::Pause, Ok(Reply::CaptureChanged(paused)), &data_dir());
        assert_eq!(printed.stdout, "capture=paused\n");
        assert!(printed
            .stderr
            .contains(data_dir().state_file().to_str().unwrap()));
        assert_eq!(printed.status, 1);
    }

    #[test]
    fn a_pause_whose_source_did_not_stop_prints_pausing_says_the_service_ends_and_exits_1() {
        let pausing = edited(Platform::Macos, |f| {
            f.desired = crate::model::CaptureState::Paused;
            f.capture = Capture::Pausing;
            stopped_sources(f);
            f.sources[2].state = SourceState::Failed {
                reason: Reason::Timeout,
                error: None,
            };
        });
        let printed = print(
            &Verb::Pause,
            Ok(Reply::CaptureChanged(pausing)),
            &data_dir(),
        );
        assert_eq!(printed.stdout, "capture=pausing\n");
        assert_eq!(
            printed.stderr,
            "halvo-capture: capture did not stop in time; the capture service is ending\n"
        );
        assert_eq!(printed.status, 1);
        let paused = edited(Platform::Macos, |f| {
            f.desired = crate::model::CaptureState::Paused;
            f.capture = Capture::Paused;
            stopped_sources(f);
        });
        let printed = print(&Verb::Pause, Ok(Reply::CaptureChanged(paused)), &data_dir());
        assert_eq!(
            (printed.stdout.as_str(), printed.status),
            ("capture=paused\n", 0)
        );
        assert_eq!(printed.stderr, "");
    }

    const STATUS_AWAITING_RESUME: &str = "protocol=1\ndaemon_version=0.0.0\npid=4242\nstarted_at=2026-11-12T09:00:00Z\nrss_peak_bytes=25264128\nparent=watched\ndesired=running\ncapture=awaiting_resume\ncapture_reason=-\nlease=fresh\nstore=open\nstore_id=0123456789abcdef0123456789abcdef\nstore_error=-\nstore_retry_in_s=-\nstore_bytes=1048576\nstore_logical_bytes=524288\ncap_bytes=26843545600\ndrain=healthy\ndrain_error=-\ndrain_since=-\nevents_pending=0\nevents_lost=0\ndropped_frames=0\npointer_coalesced=0\ntap_disabled_timeout=0\ntap_disabled_user_input=0\nlate_events=0\nmax_lag_ms=0\nsecure_input_keystrokes=0\ntitle_read_errors=0\nlast_flush_at=2026-11-12T09:00:05Z\nexpiry=healthy\nexpiry_error=-\nlast_expiry_at=2026-11-12T09:00:01Z\nlast_eviction_at=-\nlast_stop=source_not_stopped\nlast_stop_newest_capture=-\nlast_stop_kind=pause\nlast_stop_lost events=3 first=2026-11-12T08:59:58Z last=2026-11-12T08:59:59Z error=- final=no\nlast_stop_source name=input error=timeout during shutdown: osstatus 0\nstate_file=ok\nplatform=macos\nenabled_sources=foreground,input\nswitch name=title_text value=on\nswitch name=key_class value=on\nswitch name=pointer_class value=on\nswitch name=screen_activity value=off\nswitch name=site_read value=off\npermission name=screen_and_system_audio_recording grant=granted at_start=granted asked=yes once_granted=yes\npermission name=accessibility grant=granted at_start=granted asked=yes once_granted=yes\npermission name=input_monitoring grant=granted at_start=granted asked=yes once_granted=yes\nsource name=foreground state=stopped\nsource name=screen state=off\nsource name=input state=stopped\nsource name=system_audio state=off\nsource name=microphone state=off\nkind name=screen_frame dropped=0 refused=0\nkind name=keystroke dropped=0 refused=0\nkind name=pointer dropped=0 refused=0\nkind name=foreground_app dropped=0 refused=0\nkind name=window_title dropped=0 refused=0\nkind name=system_audio dropped=0 refused=0\nkind name=microphone dropped=0 refused=0\nkind name=camera dropped=0 refused=0\n";

    #[test]
    fn a_life_after_a_source_that_did_not_stop_prints_awaiting_resume_and_source_not_stopped() {
        let status = edited(Platform::Macos, |f| {
            f.capture = Capture::AwaitingResume;
            f.last_stop = crate::proto::LastStop::SourceNotStopped;
            f.last_stop_kind = Some(crate::proto::StopKind::Pause);
            f.last_stop_lost = crate::proto::LostRecord {
                events: 3,
                first: Some("2026-11-12T08:59:58Z".parse().unwrap()),
                last: Some("2026-11-12T08:59:59Z".parse().unwrap()),
                error: None,
                final_figures: false,
            };
            f.last_stop_sources = vec![crate::proto::StopSourceLine {
                name: SourceName::Input,
                error: SourceError {
                    kind: "timeout".parse().unwrap(),
                    stage: "shutdown".parse().unwrap(),
                    os_domain: "osstatus".parse().unwrap(),
                    os_code: 0,
                },
            }];
            stopped_sources(f);
        });
        assert_eq!(status_text(&status), STATUS_AWAITING_RESUME);
        println!("BEGIN c2-status-awaiting-resume");
        print!("{STATUS_AWAITING_RESUME}");
        println!("END c2-status-awaiting-resume");
    }

    #[test]
    fn a_default_status_prints_the_record_lines_of_no_record() {
        let text = status_text(&sample_status(Platform::Windows));
        assert!(text.contains(
            "\nlast_stop_newest_capture=-\nlast_stop_kind=-\nlast_stop_lost events=0 first=- last=- error=- final=yes\nstate_file=ok\n"
        ));
        assert!(!text.contains("last_stop_source "));
    }

    #[test]
    fn a_last_stop_source_line_holds_the_kind_the_stage_and_the_os_code_and_no_other_text() {
        let status = edited(Platform::Windows, |f| {
            f.last_stop = crate::proto::LastStop::SourceNotStopped;
            f.last_stop_kind = Some(crate::proto::StopKind::Lapse);
            f.last_stop_lost.final_figures = false;
            f.last_stop_sources = [SourceName::Screen, SourceName::Pointer]
                .into_iter()
                .map(|name| crate::proto::StopSourceLine {
                    name,
                    error: SourceError {
                        kind: "os_error".parse().unwrap(),
                        stage: "shutdown".parse().unwrap(),
                        os_domain: "hresult".parse().unwrap(),
                        os_code: -2147467259,
                    },
                })
                .collect();
        });
        let lines: Vec<&str> = status_text(&status)
            .lines()
            .filter(|l| l.starts_with("last_stop_source "))
            .map(|l| l.to_string().leak() as &str)
            .collect();
        assert_eq!(
            lines,
            [
                "last_stop_source name=screen error=os_error during shutdown: hresult -2147467259",
                "last_stop_source name=pointer error=os_error during shutdown: hresult -2147467259"
            ]
        );
    }

    #[test]
    fn a_source_that_failed_with_writer_gone_or_item_closed_prints_its_reason() {
        for (reason, token) in [
            (Reason::WriterGone, "writer_gone"),
            (Reason::ItemClosed, "item_closed"),
        ] {
            let status = edited(Platform::Windows, |f| {
                f.capture = Capture::Partial;
                f.sources[4].state = SourceState::Failed {
                    reason,
                    error: None,
                };
            });
            assert!(status_text(&status).contains(&format!(
                "\nsource name=window_title state=failed reason={token}\n"
            )));
        }
    }

    #[test]
    fn set_cap_prints_the_cap_and_whether_it_evicts() {
        let status = edited(Platform::Macos, |f| {
            f.cap_bytes = Cap::Known(StorageCap::MIN)
        });
        let printed = print(
            &Verb::SetCap(StorageCap::MIN),
            Ok(Reply::CapSet(CapSet {
                status,
                evicting: true,
            })),
            &data_dir(),
        );
        assert_eq!(printed.stdout, "cap_bytes=1073741824 evicting=yes\n");
        assert_eq!(printed.status, 0);
    }

    #[test]
    fn a_reply_of_another_kind_than_the_verb_asks_is_an_error() {
        let printed = print(
            &Verb::Stop,
            Ok(Reply::Status(sample_status(Platform::Macos))),
            &data_dir(),
        );
        assert_eq!(printed.status, 1);
        assert!(printed.stderr.starts_with("halvo-capture: "));
        assert_eq!(printed.stdout, "");
    }

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn ctl_takes_the_directory_of_the_flag_and_without_it_the_default_directory_of_this_user() {
        let dir = if cfg!(windows) { r"C:\d" } else { "/d" };
        let flagged = parse(&args(&["--data-dir", dir, "status"])).unwrap();
        assert_eq!(
            flagged.data_dir().unwrap(),
            DataDir::from_flag(dir).unwrap()
        );
        let plain = parse(&args(&["status"])).unwrap();
        assert_eq!(plain.data_dir(), DataDir::for_current_user());
    }

    #[test]
    fn the_grammar_takes_each_verb_of_c2_2() {
        let dir = if cfg!(windows) { r"C:\d" } else { "/d" };
        let cases: Vec<(Vec<&str>, Verb)> = vec![
            (vec!["status"], Verb::Status { indicator: false }),
            (
                vec!["status", "--indicator"],
                Verb::Status { indicator: true },
            ),
            (vec!["inventory"], Verb::Inventory),
            (vec!["pause"], Verb::Pause),
            (vec!["resume"], Verb::Resume),
            (vec!["stop"], Verb::Stop),
            (vec!["delete-all", "-"], Verb::DeleteAll(None)),
            (
                vec!["delete-all", "fedcba9876543210fedcba9876543210"],
                Verb::DeleteAll(Some(FRESH)),
            ),
            (
                vec!["delete-all", "fedcba98-7654-3210-fedc-ba9876543210"],
                Verb::DeleteAll(Some(FRESH)),
            ),
            (vec!["set-cap", "1073741824"], Verb::SetCap(StorageCap::MIN)),
            (
                vec!["set-cap", "1099511627776"],
                Verb::SetCap(StorageCap::MAX),
            ),
        ];
        for (words, verb) in cases {
            let parsed = parse(&args(&words)).unwrap();
            assert_eq!(parsed.verb, verb, "{words:?}");
            assert_eq!(parsed.data_dir(), DataDir::for_current_user());
            let mut with_dir = vec!["--data-dir", dir];
            with_dir.extend(&words);
            let parsed = parse(&args(&with_dir)).unwrap();
            assert_eq!(parsed.verb, verb, "{with_dir:?}");
            assert_eq!(parsed.data_dir().unwrap().path(), std::path::Path::new(dir));
        }
    }

    #[test]
    fn each_verb_sends_its_request() {
        assert_eq!(
            Verb::Status { indicator: false }.request(),
            crate::proto::Request::Status { indicator: false }
        );
        assert_eq!(
            Verb::Status { indicator: true }.request(),
            crate::proto::Request::Status { indicator: true }
        );
        assert_eq!(
            Verb::DeleteAll(None).request(),
            crate::proto::Request::DeleteAll { store: None }
        );
        assert_eq!(
            Verb::SetCap(StorageCap::MIN).request(),
            crate::proto::Request::SetStorageCap {
                bytes: StorageCap::MIN
            }
        );
        assert_eq!(Verb::Stop.request(), crate::proto::Request::Stop);
    }

    #[test]
    fn anything_else_is_a_usage_error() {
        for words in [
            vec![],
            vec!["Status"],
            vec!["status", "--indicator", "x"],
            vec!["status", "--Indicator"],
            vec!["inventory", "x"],
            vec!["stop", "now"],
            vec!["delete-all"],
            vec!["delete-all", "x"],
            vec!["delete-all", "FEDCBA9876543210FEDCBA9876543210"],
            vec!["delete-all", "-", "-"],
            vec!["set-cap"],
            vec!["set-cap", "+1073741824"],
            vec!["set-cap", "1 GiB"],
            vec!["set-cap", "18446744073709551616"],
            vec!["--data-dir"],
            vec!["--data-dir", "relative", "status"],
            vec!["--data-dir", "/d"],
            vec!["status", "--data-dir", "/d"],
            vec!["run_start"],
            vec!["run-start"],
            vec!["request-permission", "accessibility"],
        ] {
            assert!(parse(&args(&words)).is_err(), "{words:?}");
        }
    }

    #[test]
    fn a_cap_outside_one_gib_and_one_tib_is_a_usage_error_that_names_both_bounds() {
        for bytes in ["1073741823", "1099511627777", "0"] {
            let error = parse(&args(&["set-cap", bytes])).unwrap_err();
            assert!(error.0.contains("1073741824"), "{}", error.0);
            assert!(error.0.contains("1099511627776"), "{}", error.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_argument_that_is_not_utf8_is_a_usage_error() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b's', 0xff]);
        assert!(parse(&[bad]).is_err());
        let bad_dir = OsString::from_vec(vec![b'/', 0xff]);
        assert!(parse(&[
            OsString::from("--data-dir"),
            bad_dir,
            OsString::from("status")
        ])
        .is_err());
    }

    #[test]
    fn the_usage_line_is_the_grammar() {
        assert_eq!(
            USAGE,
            "usage: halvo-capture ctl [--data-dir DIR] status [--indicator] | inventory | pause | resume | stop | delete-all (STORE_ID | -) | set-cap BYTES"
        );
        let printed = UsageError("bad".into()).printed();
        assert_eq!(printed.status, 64);
        assert!(printed.stderr.ends_with(&format!("{USAGE}\n")));
    }
}
