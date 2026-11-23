use std::io::{ErrorKind, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::client::{ClientError, Connector, Transport};
use crate::model::{CaptureState, InventoryRow, Modality, StorageCap, StoreId, UtcDay, UtcSecond};
use crate::onboarding::{
    assess, AssessInput, GrantState, Grants, Permission, PermissionSet, RequestOutcome,
};
#[cfg(target_os = "macos")]
use crate::paths::DataDir;
use crate::proto::{
    admission, decode_request, delete_decision, encode_reply, unknown_answer, Admission,
    AlreadyDeleted, Cap, CapSet, Capture, CaptureReason, Custody, DaemonStatus, DaemonVersion,
    Decoded, DeleteDecision, DeleteFields, DeleteReport, Deleted, ErrorCode, ErrorReply, Health,
    IntentState, InventoryReply, KindLine, LastStop, Lease, Life, OnOff, Parent, PasswordGuard,
    PermissionForm, PermissionLine, PermissionRequested, Platform, Reason, Reply, Request,
    RootDestroyed, Section, SourceLine, SourceName, SourceState, StateFile, StatusFields, Stopped,
    StoreErrorKind, StoreState, SwitchLine, SwitchName, Totals, PERMISSION_FORM,
};
use crate::wire::{read_one_frame, write_frame, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};

/// The store id of the status fixtures of gate-2.
pub const FIXTURE_STORE_ID: StoreId = StoreId::from_bytes([
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
]);

/// A store id whose last byte is `n` and whose other bytes are zero.
pub const fn store_id(n: u8) -> StoreId {
    let mut bytes = [0u8; 16];
    bytes[15] = n;
    StoreId::from_bytes(bytes)
}

fn at(text: &str) -> UtcSecond {
    text.parse().expect("a fixed instant")
}

/// The onboarding object that a daemon reports for these permission facts.
pub fn onboarding_of(
    platform: Platform,
    lines: &[PermissionLine],
) -> crate::onboarding::OnboardingState {
    let set = |pick: fn(&PermissionLine) -> bool| -> PermissionSet {
        lines.iter().filter(|l| pick(l)).map(|l| l.name).collect()
    };
    let now = lines.iter().fold(Grants::default(), |grants, line| {
        grants.with(line.name, line.grant)
    });
    let required: &[Permission] = match platform {
        Platform::Macos => &Permission::R1_MACOS,
        Platform::Windows => &[],
    };
    assess(&AssessInput {
        required,
        now,
        granted_at_capture_start: set(|l| l.at_start == GrantState::Granted),
        asked: set(|l| l.asked),
        once_granted: set(|l| l.once_granted),
        macos_major: None,
        settings_entry_name: "Halvo",
        screen_activity: false,
    })
}

/// The status of a daemon with its default switches that captures with every grant, as
/// gate-2's fixtures c2-status-macos.txt and c2-status-windows.txt show it.
pub fn sample_status(platform: Platform) -> DaemonStatus {
    let running = |name| SourceLine {
        name,
        state: SourceState::Running,
        unstopped: false,
    };
    let off = |name| SourceLine {
        name,
        state: SourceState::Off,
        unstopped: false,
    };
    let (secure, permissions, sources, enabled, guard, custody) = match platform {
        Platform::Macos => (
            Some(false),
            Permission::R1_MACOS
                .iter()
                .map(|&name| PermissionLine {
                    name,
                    grant: GrantState::Granted,
                    at_start: GrantState::Granted,
                    asked: true,
                    once_granted: true,
                })
                .collect(),
            vec![
                running(SourceName::Foreground),
                off(SourceName::Screen),
                running(SourceName::Input),
                off(SourceName::SystemAudio),
                off(SourceName::Microphone),
            ],
            vec![SourceName::Foreground, SourceName::Input],
            PasswordGuard::Os,
            Custody::RootFile,
        ),
        Platform::Windows => (
            None,
            Vec::new(),
            vec![
                running(SourceName::Foreground),
                off(SourceName::Screen),
                off(SourceName::Keyboard),
                running(SourceName::Pointer),
                running(SourceName::WindowTitle),
                off(SourceName::SystemAudio),
            ],
            vec![
                SourceName::Foreground,
                SourceName::Pointer,
                SourceName::WindowTitle,
            ],
            PasswordGuard::None,
            Custody::Dpapi,
        ),
    };
    let onboarding = onboarding_of(platform, &permissions);
    DaemonStatus::new(StatusFields {
        daemon_version: DaemonVersion {
            major: 0,
            minor: 0,
            patch: 0,
        },
        pid: 4242,
        started_at: at("2026-11-12T09:00:00Z"),
        rss_peak_bytes: 25_264_128,
        parent: Parent::Watched,
        data_dir_resolved: None,
        desired: CaptureState::Running,
        capture: Capture::Running,
        capture_reason: None,
        lease: Lease::Fresh,
        store: StoreState::Open,
        store_id: Some(FIXTURE_STORE_ID),
        store_error: None,
        store_retry_in_s: None,
        store_delete_unfinished: false,
        store_bytes: Some(1_048_576),
        store_logical_bytes: Some(524_288),
        cap_bytes: Cap::Known(StorageCap::DEFAULT),
        cap_not_set: false,
        drain: Health::Healthy,
        drain_error: None,
        drain_since: None,
        events_pending: 0,
        events_lost: 0,
        dropped_frames: 0,
        pointer_coalesced: 0,
        tap_disabled_timeout: 0,
        tap_disabled_user_input: 0,
        late_events: false,
        max_lag_ms: 0,
        secure_input_keystrokes: secure,
        typing: None,
        keys_withheld: false,
        title_read_errors: 0,
        last_flush_at: Some(at("2026-11-12T09:00:05Z")),
        expiry: Health::Healthy,
        expiry_error: None,
        last_expiry_at: Some(at("2026-11-12T09:00:01Z")),
        last_eviction_at: None,
        last_stop: LastStop::Clean,
        last_stop_newest_capture: None,
        last_stop_kind: None,
        last_stop_lost: crate::proto::LostRecord::NONE,
        last_stop_sources: Vec::new(),
        state_file: StateFile::Ok,
        platform,
        enabled_sources: enabled,
        switches: SwitchName::ALL
            .iter()
            .map(|&name| SwitchLine {
                name,
                value: if name.default_on() {
                    OnOff::On
                } else {
                    OnOff::Off
                },
            })
            .collect(),
        permissions,
        sources,
        kinds: Modality::ALL
            .iter()
            .map(|&name| KindLine {
                name,
                dropped: 0,
                refused: 0,
            })
            .collect(),
        password_guard: guard,
        password_guard_bound_ms: (guard == PasswordGuard::Os).then_some(30_000),
        screen_filter_excludes_app: None,
        custody,
        onboarding,
        ext: Vec::new(),
    })
    .expect("the sample status holds together")
}

/// The inventory of gate-2's fixture c2-inventory.txt, with the sections given.
pub fn sample_inventory(ext: Vec<Section>) -> InventoryReply {
    let eleventh = UtcDay::from_year_month_day(2026, 11, 11).expect("a fixed day");
    let twelfth = UtcDay::from_year_month_day(2026, 11, 12).expect("a fixed day");
    let row = |day, modality, events, bytes| InventoryRow {
        day,
        modality,
        events,
        bytes,
    };
    InventoryReply::new(
        FIXTURE_STORE_ID,
        at("2026-11-12T09:00:05Z"),
        vec![
            row(eleventh, Modality::Keystroke, 5, 160),
            row(eleventh, Modality::Pointer, 7, 266),
            row(eleventh, Modality::ForegroundApp, 3, 144),
            row(eleventh, Modality::WindowTitle, 2, 230),
            row(twelfth, Modality::Keystroke, 4, 128),
            row(twelfth, Modality::ForegroundApp, 1, 48),
        ],
        Totals {
            events: 22,
            bytes: 976,
            disk_bytes: 1_048_576,
        },
        ext,
    )
    .expect("the sample inventory holds together")
}

/// The delete report of gate-2's fixture c2-deleted.txt.
pub fn sample_delete_report() -> DeleteReport {
    DeleteReport::new(DeleteFields {
        segments_shredded: Some(2),
        keys_destroyed: Some(3),
        root_destroyed: RootDestroyed::Yes,
        root_id: None,
        files_left: 0,
        database_left: false,
        not_followed: 0,
        entries_removed: 4,
        entries_left: 0,
        unknown_entries: 0,
        intent: IntentState::Written,
    })
    .expect("the sample report holds together")
}

fn start_stopped_sources(f: &mut StatusFields) {
    for line in &mut f.sources {
        if line.state == (SourceState::Stopped { by_user: false }) {
            line.state = SourceState::Running;
        }
    }
}

fn end_the_person_s_stops(f: &mut StatusFields) {
    for line in &mut f.sources {
        if line.state == (SourceState::Stopped { by_user: true }) {
            line.state = SourceState::Stopped { by_user: false };
        }
    }
}

/// Sets capture and its reason from desired, `hold`, the lease and the states of the sources
/// that are not off: paused when desired is paused; else awaiting_resume under `hold`; else
/// no_indicator when the lease is not fresh; else running when each of those sources is running or impaired, partial when some
/// are, and otherwise faulted with the reason that stood, or no_source when none stood.
fn settle(f: &mut StatusFields, hold: bool) {
    settle_sources(f, hold);
    if f.sources.iter().any(|s| s.unstopped)
        && matches!(f.capture, Capture::Paused | Capture::NoIndicator)
    {
        f.capture = Capture::Pausing;
    }
}

fn settle_sources(f: &mut StatusFields, hold: bool) {
    let required: Vec<&SourceLine> = f
        .sources
        .iter()
        .filter(|s| s.state != SourceState::Off)
        .collect();
    let running = required
        .iter()
        .filter(|s| matches!(s.state, SourceState::Running | SourceState::Impaired { .. }))
        .count();
    let (capture, reason) = if f.desired == CaptureState::Paused {
        (Capture::Paused, None)
    } else if hold {
        (Capture::AwaitingResume, None)
    } else if f.lease != Lease::Fresh {
        (Capture::NoIndicator, None)
    } else if required.is_empty() {
        (Capture::Faulted, Some(CaptureReason::NoSource))
    } else if running == required.len() {
        (Capture::Running, None)
    } else if running > 0 {
        (Capture::Partial, None)
    } else {
        (
            Capture::Faulted,
            f.capture_reason.or(Some(CaptureReason::NoSource)),
        )
    };
    f.capture = capture;
    f.capture_reason = reason;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StopAnswer {
    #[default]
    Ok,
    ThreadEnded,
    Timeout,
    Os,
}

impl StopAnswer {
    /// Whether the answer is a stop: Ok and ThreadEnded are, Timeout and the Os error are not.
    pub const fn is_a_stop(self) -> bool {
        matches!(self, StopAnswer::Ok | StopAnswer::ThreadEnded)
    }
}

fn ask_to_stop(line: &mut SourceLine, answer: StopAnswer) {
    let failed = |reason| SourceState::Failed {
        reason,
        error: None,
    };
    let off = line.state == SourceState::Off;
    (line.state, line.unstopped) = if answer.is_a_stop() && off {
        (SourceState::Off, false)
    } else if answer.is_a_stop() {
        (SourceState::Stopped { by_user: false }, false)
    } else if off {
        (SourceState::Off, true)
    } else if answer == StopAnswer::Timeout {
        (failed(Reason::Timeout), true)
    } else {
        (failed(Reason::OsError), true)
    };
}

/// A daemon in memory that answers requests through the real decoder, the delete rule, the
/// permission form, one mutation at a time and the rule of a life that is ending. It records every request that reached its
/// handlers.
#[derive(Debug, Clone)]
pub struct ScriptedDaemon {
    status: DaemonStatus,
    form: PermissionForm,
    in_flight: bool,
    silent: bool,
    raw_reply: Option<Vec<u8>>,
    stop_reply: Stopped,
    inventory: Option<InventoryReply>,
    fresh_store: Option<StoreId>,
    next_store: u8,
    requests: Vec<Request>,
    refused_change: bool,
    /// The hold of ruling PL-3: set when the status given reads awaiting_resume, kept through a
    /// claim and a pause, and cleared by a Resume.
    hold: bool,
    stop_answers: Vec<(SourceName, StopAnswer)>,
    life: Life,
}

fn error(code: ErrorCode, message: &str) -> Reply {
    Reply::Error(ErrorReply::new(code, message))
}

impl ScriptedDaemon {
    pub fn new(status: DaemonStatus) -> Self {
        Self {
            hold: status.fields().capture == Capture::AwaitingResume,
            status,
            form: PERMISSION_FORM,
            in_flight: false,
            silent: false,
            raw_reply: None,
            stop_reply: Stopped {
                flushed: true,
                events_lost: 0,
            },
            inventory: None,
            fresh_store: Some(store_id(0x10)),
            next_store: 0x11,
            requests: Vec::new(),
            refused_change: false,
            stop_answers: Vec::new(),
            life: Life::Going,
        }
    }

    /// Sets the daemon's fact that its life is ending, or that it is not (ruling PL-17 (a)); the
    /// daemon never sets it by itself. A script that gives a stop answer that is not a stop, or
    /// that sends Stop, and then sends any further request calls `set_life(Life::Ending)` first.
    /// Without it the daemon answers as a going life: a set-cap and the permission mutations of
    /// its form are served and write, a Stop answers stopped, a delete after the timed-out Pause
    /// answers the plain busy, and after a Stop a delete with the shown id is accepted (ruling
    /// CT-13). In a life that is ending capture reads pausing with no reason (contracts.md C2.3):
    /// the status held, and every status set or changed after it, reads so, in either order of
    /// `set_life` and `set_status`. `set_life(Life::Going)` changes no status: pausing stands until
    /// the process is gone, and a script that means the next life calls `set_status` (ruling
    /// CT-14 K3).
    pub fn set_life(&mut self, life: Life) {
        self.life = life;
        self.status = self.in_this_life(self.status.clone());
    }

    fn in_this_life(&self, status: DaemonStatus) -> DaemonStatus {
        let f = status.fields();
        if self.life == Life::Going || (f.capture == Capture::Pausing && f.capture_reason.is_none())
        {
            return status;
        }
        let mut fields = status.into_fields();
        fields.capture = Capture::Pausing;
        fields.capture_reason = None;
        DaemonStatus::new(fields).expect(
            "the status type takes capture pausing with no reason beside every status that it took",
        )
    }

    pub fn set_stop_answer(&mut self, source: SourceName, answer: StopAnswer) {
        self.stop_answers.retain(|(name, _)| *name != source);
        self.stop_answers.push((source, answer));
    }

    pub fn status(&self) -> &DaemonStatus {
        &self.status
    }

    pub fn set_status(&mut self, status: DaemonStatus) {
        self.hold = status.fields().capture == Capture::AwaitingResume;
        self.status = self.in_this_life(status);
    }

    /// From the first answer that reads pausing until the process is gone, C2.3 has capture
    /// read pausing: a claim, a pause and a resume then change nothing.
    fn pausing(&self) -> bool {
        self.status.fields().capture == Capture::Pausing
    }

    pub fn set_form(&mut self, form: PermissionForm) {
        self.form = form;
    }

    /// While set, a mutation that this daemon's permission form serves answers `busy`; the
    /// request of the other form still answers `unsupported_op`, and a read is answered.
    pub fn set_mutation_in_flight(&mut self, in_flight: bool) {
        self.in_flight = in_flight;
    }

    /// While set, the daemon answers nothing, whatever arrives; a request that decodes still
    /// reaches the handlers and is recorded.
    pub fn set_silent(&mut self, silent: bool) {
        self.silent = silent;
    }

    /// Sends these bytes as they are in place of every reply frame.
    pub fn set_raw_reply(&mut self, frame: Vec<u8>) {
        self.raw_reply = Some(frame);
    }

    pub fn set_stop_reply(&mut self, stopped: Stopped) {
        self.stop_reply = stopped;
    }

    pub fn set_inventory(&mut self, inventory: InventoryReply) {
        self.inventory = Some(inventory);
    }

    /// The id of the store that the next accepted delete creates; `None` when it creates none.
    pub fn set_fresh_store(&mut self, store: Option<StoreId>) {
        self.fresh_store = store;
    }

    pub fn requests(&self) -> &[Request] {
        &self.requests
    }

    fn edit(&mut self, change: impl FnOnce(&mut StatusFields)) {
        let mut fields = self.status.clone().into_fields();
        change(&mut fields);
        fields.onboarding = onboarding_of(fields.platform, &fields.permissions);
        match DaemonStatus::new(fields) {
            Ok(status) => self.status = self.in_this_life(status),
            Err(_) => self.refused_change = true,
        }
    }

    fn permission(&mut self, permission: Permission, change: impl FnOnce(&mut PermissionLine)) {
        self.edit(|f| {
            if let Some(line) = f.permissions.iter_mut().find(|l| l.name == permission) {
                change(line);
            }
        });
    }

    fn delete(&mut self, armed: Option<StoreId>) -> Reply {
        let f = self.status.fields();
        match delete_decision(
            self.life,
            f.capture,
            f.store,
            f.store_error,
            f.store_delete_unfinished,
            f.store_id,
            armed,
        ) {
            DeleteDecision::Busy => error(ErrorCode::Busy, "the delete cannot start now"),
            DeleteDecision::Ending => Reply::Error(ErrorReply::busy_of_an_ending_life()),
            DeleteDecision::Refused => Reply::DeleteRefused,
            DeleteDecision::AlreadyDeleted { current } => {
                Reply::AlreadyDeleted(AlreadyDeleted { current })
            }
            DeleteDecision::Accept => {
                let fresh = self.fresh_store;
                self.fresh_store = fresh.map(|_| {
                    let next = store_id(self.next_store);
                    self.next_store = self.next_store.wrapping_add(1);
                    next
                });
                self.edit(|f| {
                    f.store_id = fresh;
                    if fresh.is_some() {
                        f.store = StoreState::Open;
                        f.store_error = None;
                    } else {
                        f.store = StoreState::Unavailable;
                        f.store_error = Some(StoreErrorKind::Io);
                    }
                });
                Reply::Deleted(Deleted {
                    report: sample_delete_report(),
                    store: fresh,
                    unfinished: false,
                })
            }
        }
    }

    fn handle(&mut self, request: Request) -> Reply {
        let reply = self.handle_request(request);
        if std::mem::take(&mut self.refused_change) {
            return error(
                ErrorCode::Internal,
                "the scripted change does not hold together",
            );
        }
        reply
    }

    fn handle_request(&mut self, request: Request) -> Reply {
        self.requests.push(request.clone());
        match admission(&request, self.form, self.in_flight, self.life) {
            Admission::OtherForm => {
                return error(
                    ErrorCode::UnsupportedOp,
                    "this daemon serves the other permission form",
                )
            }
            Admission::InFlight => return error(ErrorCode::Busy, "another mutation is in flight"),
            Admission::Ending => return Reply::Error(ErrorReply::busy_of_an_ending_life()),
            Admission::StatusAsItStands => return Reply::CaptureChanged(self.status.clone()),
            Admission::ClaimStartingNothing => return Reply::Status(self.status.clone()),
            Admission::PauseKeepingTheWord => {
                if self.status.fields().desired != CaptureState::Paused {
                    self.edit(|f| f.desired = CaptureState::Paused);
                }
                return Reply::CaptureChanged(self.status.clone());
            }
            Admission::Serve => {}
        }
        match request {
            Request::Status { indicator } => {
                if indicator && self.status.fields().lease != Lease::Fresh && !self.pausing() {
                    let hold = self.hold;
                    self.edit(|f| {
                        f.lease = Lease::Fresh;
                        if f.desired == CaptureState::Running && !hold {
                            start_stopped_sources(f);
                        }
                        settle(f, hold);
                    });
                }
                Reply::Status(self.status.clone())
            }
            Request::Inventory => {
                if self.status.fields().store == StoreState::Open {
                    Reply::Inventory(
                        self.inventory
                            .clone()
                            .unwrap_or_else(|| sample_inventory(Vec::new())),
                    )
                } else {
                    error(
                        ErrorCode::StoreUnavailable,
                        &format!("store unavailable: {}", self.status.fields().store.as_str()),
                    )
                }
            }
            Request::Pause => {
                let pausing = self.pausing();
                let answers = self.stop_answers.clone();
                self.edit(|f| {
                    for line in &mut f.sources {
                        let asked = !matches!(
                            line.state,
                            SourceState::Off | SourceState::Stopped { by_user: true }
                        );
                        if line.unstopped || (!pausing && asked) {
                            let answer = answers
                                .iter()
                                .find(|(name, _)| *name == line.name)
                                .map(|(_, answer)| *answer)
                                .unwrap_or_default();
                            ask_to_stop(line, answer);
                        }
                    }
                    f.desired = CaptureState::Paused;
                    if !pausing {
                        f.capture_reason = None;
                        f.capture = if f.sources.iter().any(|s| s.unstopped) {
                            Capture::Pausing
                        } else {
                            Capture::Paused
                        };
                    }
                });
                Reply::CaptureChanged(self.status.clone())
            }
            Request::Resume => {
                if !self.pausing() {
                    self.edit(|f| {
                        f.desired = CaptureState::Running;
                        end_the_person_s_stops(f);
                        if f.lease == Lease::Fresh {
                            start_stopped_sources(f);
                        }
                        settle(f, false);
                    });
                    if !self.refused_change {
                        self.hold = false;
                    }
                }
                Reply::CaptureChanged(self.status.clone())
            }
            Request::Stop => Reply::Stopped(self.stop_reply),
            Request::DeleteAll { store } => self.delete(store),
            Request::SetStorageCap { bytes } => {
                self.edit(|f| f.cap_bytes = Cap::Known(bytes));
                let evicting = self
                    .status
                    .fields()
                    .store_bytes
                    .is_some_and(|stored| stored > bytes.bytes());
                Reply::CapSet(CapSet {
                    status: self.status.clone(),
                    evicting,
                })
            }
            Request::RequestPermission { permission } => {
                let granted = self
                    .status
                    .fields()
                    .permissions
                    .iter()
                    .any(|l| l.name == permission && l.grant == GrantState::Granted);
                self.permission(permission, |line| line.asked = true);
                Reply::PermissionRequested(PermissionRequested {
                    outcome: if granted {
                        RequestOutcome::AlreadyGranted
                    } else {
                        RequestOutcome::Requested
                    },
                })
            }
            Request::PermissionAsked { permission } => {
                self.permission(permission, |line| line.asked = true);
                Reply::Status(self.status.clone())
            }
            Request::AcknowledgeLostPermission { permission } => {
                self.permission(permission, |line| line.once_granted = false);
                Reply::Status(self.status.clone())
            }
            Request::ResetSettings => {
                if self.status.fields().state_file != StateFile::Corrupt {
                    return error(
                        ErrorCode::StateFile,
                        "the state file loads or is of a newer version, so the settings were not reset",
                    );
                }
                self.edit(|f| {
                    f.state_file = StateFile::Ok;
                    f.cap_bytes = Cap::Unknown;
                });
                Reply::Status(self.status.clone())
            }
        }
    }

    /// The reply, as an envelope and not framed, to one request body.
    pub fn answer_frame(&mut self, body: &[u8]) -> Option<Vec<u8>> {
        let reply = match decode_request(body) {
            Ok(Decoded::Known(request)) => self.handle(request),
            Ok(Decoded::Unknown(unknown)) => unknown_answer(&unknown),
            Err(refused) => refused.answer(),
        };
        (!self.silent).then(|| encode_reply(&reply))
    }

    /// The reply frame to the bytes that a client sent on one connection.
    pub fn serve(&mut self, received: &[u8]) -> Option<Vec<u8>> {
        if self.silent {
            if let Ok(body) = read_one_frame(&mut &received[..], REQUEST_MAX_BYTES) {
                self.answer_frame(&body);
            }
            return None;
        }
        if let Some(raw) = &self.raw_reply {
            return Some(raw.clone());
        }
        let body = match read_one_frame(&mut &received[..], REQUEST_MAX_BYTES) {
            Ok(body) => self.answer_frame(&body)?,
            Err(_) => encode_reply(&error(ErrorCode::InvalidArgument, "not one request frame")),
        };
        let mut frame = Vec::new();
        if write_frame(&mut frame, &body, REPLY_MAX_BYTES).is_err() {
            frame.clear();
            let internal = encode_reply(&error(ErrorCode::Internal, "the reply is too long"));
            write_frame(&mut frame, &internal, REPLY_MAX_BYTES).ok()?;
        }
        Some(frame)
    }
}

#[derive(Debug, Default)]
struct Shared {
    daemon: Option<ScriptedDaemon>,
    longest_timeout: Option<Duration>,
    ended: usize,
}

/// A connector whose transport hands each request to a [`ScriptedDaemon`] in memory, after
/// the client has ended the request.
#[derive(Debug, Clone)]
pub struct ScriptedConnector {
    shared: Arc<Mutex<Shared>>,
}

impl ScriptedConnector {
    pub fn new(daemon: ScriptedDaemon) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                daemon: Some(daemon),
                ..Shared::default()
            })),
        }
    }

    fn with<T>(&self, read: impl FnOnce(&mut Shared) -> T) -> T {
        read(&mut self.shared.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn requests(&self) -> Vec<Request> {
        self.with(|s| {
            s.daemon
                .as_ref()
                .map(|d| d.requests.clone())
                .unwrap_or_default()
        })
    }

    pub fn status(&self) -> DaemonStatus {
        self.with(|s| s.daemon.as_ref().expect("a daemon").status.clone())
    }

    /// The longest timeout that the client set on a transport of this connector.
    pub fn longest_timeout(&self) -> Option<Duration> {
        self.with(|s| s.longest_timeout)
    }

    pub fn ended_requests(&self) -> usize {
        self.with(|s| s.ended)
    }
}

#[derive(Debug)]
pub struct ScriptedTransport {
    shared: Arc<Mutex<Shared>>,
    written: Vec<u8>,
    ended: bool,
    reply: Option<std::io::Cursor<Vec<u8>>>,
}

impl Read for ScriptedTransport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.reply.is_none() {
            if !self.ended {
                return Err(ErrorKind::TimedOut.into());
            }
            let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let daemon = shared.daemon.as_mut().expect("a daemon");
            match daemon.serve(&self.written) {
                Some(frame) => self.reply = Some(std::io::Cursor::new(frame)),
                None => return Err(ErrorKind::TimedOut.into()),
            }
        }
        self.reply.as_mut().map_or(Ok(0), |reply| reply.read(buf))
    }
}

impl Write for ScriptedTransport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.ended {
            return Err(ErrorKind::BrokenPipe.into());
        }
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Transport for ScriptedTransport {
    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()> {
        let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.longest_timeout = Some(shared.longest_timeout.map_or(timeout, |t| t.max(timeout)));
        Ok(())
    }

    fn end_request(&mut self) -> std::io::Result<()> {
        self.ended = true;
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).ended += 1;
        Ok(())
    }
}

impl Connector for ScriptedConnector {
    type Transport = ScriptedTransport;

    fn endpoint(&self) -> String {
        "scripted".to_string()
    }

    fn connect(&self, _timeout: Duration) -> Result<ScriptedTransport, ClientError> {
        Ok(ScriptedTransport {
            shared: Arc::clone(&self.shared),
            written: Vec::new(),
            ended: false,
            reply: None,
        })
    }
}

/// Makes `dir` and its missing parents, and gives `dir` mode 0700.
#[cfg(unix)]
pub fn make_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// A [`ScriptedDaemon`] behind a real unix socket at the control socket of a data
/// directory, served by a thread of this process until the value is dropped.
#[cfg(target_os = "macos")]
pub struct ScriptedUnixServer {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    socket: std::path::PathBuf,
}

#[cfg(target_os = "macos")]
impl ScriptedUnixServer {
    pub fn start(data_dir: &DataDir, daemon: ScriptedDaemon) -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicBool, Ordering};

        make_private_dir(&data_dir.run_dir())?;
        let socket = data_dir.control_socket();
        let listener = std::os::unix::net::UnixListener::bind(&socket)?;
        listener.set_nonblocking(true)?;
        let shared = Arc::new(Mutex::new(Shared {
            daemon: Some(daemon),
            ..Shared::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => serve_one(stream, &shared, &stop),
                        Err(e) if e.kind() == ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Ok(Self {
            shared,
            stop,
            thread: Some(thread),
            socket,
        })
    }

    pub fn requests(&self) -> Vec<Request> {
        let shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        shared
            .daemon
            .as_ref()
            .map(|d| d.requests.clone())
            .unwrap_or_default()
    }
}

#[cfg(target_os = "macos")]
fn serve_one(
    mut stream: std::os::unix::net::UnixStream,
    shared: &Mutex<Shared>,
    stop: &std::sync::atomic::AtomicBool,
) {
    use std::os::unix::io::AsRawFd;

    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: the descriptor is the open socket of `stream`; the out pointers are live locals.
    let read = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    // SAFETY: geteuid reads the process's effective user id; it takes no pointer and cannot fail.
    if read != 0 || uid != unsafe { libc::geteuid() } {
        return;
    }
    if stream.set_nonblocking(false).is_err()
        || stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .is_err()
    {
        return;
    }
    let mut received = Vec::new();
    let mut limited = (&mut stream).take(REQUEST_MAX_BYTES as u64 + 8);
    if limited.read_to_end(&mut received).is_err() {
        return;
    }
    let reply = {
        let mut shared = shared.lock().unwrap_or_else(|e| e.into_inner());
        shared.daemon.as_mut().and_then(|d| d.serve(&received))
    };
    match reply {
        Some(frame) => {
            let _ = stream.write_all(&frame);
        }
        None => {
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while !stop.load(std::sync::atomic::Ordering::Relaxed)
                && std::time::Instant::now() < until
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for ScriptedUnixServer {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No request of the double reaches its `internal` answer: every arm changes a status that
    /// the type took into one that it takes. So the test makes the change step refuse a change
    /// that breaks the sum of the dropped counts, then sends a request.
    #[test]
    fn a_change_that_does_not_hold_together_keeps_the_status_answers_internal_and_the_next_request()
    {
        let before = sample_status(Platform::Macos);
        let mut daemon = ScriptedDaemon::new(before.clone());
        daemon.edit(|f| f.dropped_frames += 1);
        assert_eq!(daemon.status(), &before);
        let refused = daemon.handle(Request::Status { indicator: false });
        assert!(
            matches!(&refused, Reply::Error(e) if e.code() == ErrorCode::Internal),
            "{refused:?}"
        );
        assert_eq!(daemon.status(), &before);
        assert_eq!(
            daemon.handle(Request::Status { indicator: false }),
            Reply::Status(before)
        );
    }

    fn line(status: &DaemonStatus, name: SourceName) -> SourceLine {
        status
            .fields()
            .sources
            .iter()
            .find(|l| l.name == name)
            .cloned()
            .expect("a source line of the platform")
    }

    fn changed(reply: Reply) -> DaemonStatus {
        match reply {
            Reply::CaptureChanged(status) => status,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_stop_answered_with_timeout_or_os_marks_the_source_and_a_later_ok_or_thread_ended_clears_the_mark(
    ) {
        for platform in [Platform::Macos, Platform::Windows] {
            let sample = sample_status(platform);
            let first = sample.fields().enabled_sources[0];
            for (no_stop, reason) in [
                (StopAnswer::Timeout, Reason::Timeout),
                (StopAnswer::Os, Reason::OsError),
            ] {
                for stop in [StopAnswer::Ok, StopAnswer::ThreadEnded] {
                    let case = format!("{platform:?} {no_stop:?} then {stop:?}");
                    assert!(stop.is_a_stop() && !no_stop.is_a_stop(), "{case}");
                    let mut daemon = ScriptedDaemon::new(sample.clone());
                    daemon.set_stop_answer(first, no_stop);
                    let status = changed(daemon.handle(Request::Pause));
                    let f = status.fields();
                    assert_eq!(
                        (f.desired, f.capture),
                        (CaptureState::Paused, Capture::Pausing),
                        "{case}"
                    );
                    assert_eq!(
                        line(&status, first),
                        SourceLine {
                            name: first,
                            state: SourceState::Failed {
                                reason,
                                error: None
                            },
                            unstopped: true,
                        },
                        "{case}"
                    );
                    for other in &f.sources {
                        if other.name != first {
                            assert!(!other.unstopped, "{case} {other:?}");
                        }
                    }
                    for request in [Request::Status { indicator: true }, Request::Resume] {
                        assert_ne!(
                            daemon.handle(request.clone()),
                            Reply::Error(ErrorReply::new(
                                ErrorCode::Internal,
                                "the scripted change does not hold together"
                            )),
                            "{case} {request:?}"
                        );
                        assert_eq!(daemon.status(), &status, "{case} {request:?}");
                    }
                    daemon.handle(Request::Pause);
                    assert_eq!(daemon.status(), &status, "{case}: asked again, no stop yet");
                    daemon.set_stop_answer(first, stop);
                    let cleared = changed(daemon.handle(Request::Pause));
                    assert_eq!(
                        line(&cleared, first),
                        SourceLine {
                            name: first,
                            state: SourceState::Stopped { by_user: false },
                            unstopped: false,
                        },
                        "{case}"
                    );
                    assert_eq!(cleared.fields().capture, Capture::Pausing, "{case}");
                }
            }
            for stop in [StopAnswer::Ok, StopAnswer::ThreadEnded] {
                let mut daemon = ScriptedDaemon::new(sample.clone());
                daemon.set_stop_answer(first, stop);
                let status = changed(daemon.handle(Request::Pause));
                assert_eq!(status.fields().capture, Capture::Paused, "{stop:?}");
                assert!(
                    status.fields().sources.iter().all(|l| !l.unstopped),
                    "{stop:?}"
                );
            }
        }
    }

    fn walk_starts(platform: Platform) -> Vec<DaemonStatus> {
        let sample = sample_status(platform);
        let first = sample.fields().enabled_sources[0];
        let with = |change: &dyn Fn(&mut StatusFields)| {
            let mut f = sample.clone().into_fields();
            for line in &mut f.sources {
                if line.state != SourceState::Off {
                    line.state = SourceState::Stopped { by_user: false };
                }
            }
            change(&mut f);
            DaemonStatus::new(f).expect("a start of the walk holds together")
        };
        vec![
            sample.clone(),
            with(&|f| {
                f.desired = CaptureState::Paused;
                f.capture = Capture::Paused;
            }),
            with(&|f| {
                f.lease = Lease::Lapsed;
                f.capture = Capture::NoIndicator;
            }),
            with(&|f| f.capture = Capture::AwaitingResume),
            with(&|f| {
                f.desired = CaptureState::Paused;
                f.lease = Lease::Lapsed;
                f.capture = Capture::Starting;
                if let Some(line) = f.sources.iter_mut().find(|l| l.name == first) {
                    line.state = SourceState::Failed {
                        reason: Reason::Timeout,
                        error: None,
                    };
                    line.unstopped = true;
                }
            }),
        ]
    }

    #[derive(Debug, Clone, Copy)]
    enum Step {
        Ask(fn() -> Request),
        Answer(usize, StopAnswer),
    }

    #[test]
    fn every_status_that_the_scripted_daemon_builds_from_its_stop_answers_is_taken() {
        let steps = [
            Step::Ask(|| Request::Status { indicator: true }),
            Step::Ask(|| Request::Pause),
            Step::Ask(|| Request::Resume),
            Step::Answer(0, StopAnswer::Timeout),
            Step::Answer(0, StopAnswer::Ok),
            Step::Answer(1, StopAnswer::Os),
            Step::Answer(1, StopAnswer::ThreadEnded),
        ];
        let mut walked = 0usize;
        let mut marked = 0usize;
        let mut marked_from_an_unmarked_start = 0usize;
        for platform in [Platform::Macos, Platform::Windows] {
            let enabled = sample_status(platform).fields().enabled_sources.clone();
            for start in walk_starts(platform) {
                let start_carries_a_mark = start.fields().sources.iter().any(|l| l.unstopped);
                for n in 0..steps.len().pow(4) {
                    let mut daemon = ScriptedDaemon::new(start.clone());
                    let mut taken = Vec::new();
                    for depth in 0..4 {
                        let step = steps[n / steps.len().pow(depth) % steps.len()];
                        taken.push(step);
                        match step {
                            Step::Answer(at, answer) => daemon.set_stop_answer(enabled[at], answer),
                            Step::Ask(request) => {
                                let reply = daemon.handle(request());
                                assert!(
                                    !matches!(&reply, Reply::Error(e) if e.code() == ErrorCode::Internal),
                                    "{platform:?} from {:?}, steps {taken:?}: {reply:?}",
                                    start.fields().capture
                                );
                                let f = daemon.status().fields();
                                if f.sources.iter().any(|l| l.unstopped) {
                                    marked += 1;
                                    if !start_carries_a_mark {
                                        marked_from_an_unmarked_start += 1;
                                    }
                                    assert!(
                                        !matches!(
                                            f.capture,
                                            Capture::Paused
                                                | Capture::NoIndicator
                                                | Capture::AwaitingResume
                                        ),
                                        "{platform:?} steps {taken:?}"
                                    );
                                }
                                walked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(walked > 0 && marked > 0, "walked {walked}, marked {marked}");
        assert!(
            marked_from_an_unmarked_start > 0,
            "no status beside a mark from a start that carried none: walked {walked}, marked {marked}"
        );
        println!(
            "statuses read: {walked}, of them beside a mark: {marked}, from a start with no mark: {marked_from_an_unmarked_start}"
        );
    }

    #[test]
    fn a_pause_keeps_the_person_s_stop_of_a_source_whatever_the_stop_answers_and_a_resume_ends_it()
    {
        let mut f = sample_status(Platform::Macos).into_fields();
        for line in &mut f.sources {
            if line.name == SourceName::Screen {
                line.state = SourceState::Stopped { by_user: true };
            }
        }
        f.enabled_sources = vec![
            SourceName::Foreground,
            SourceName::Screen,
            SourceName::Input,
        ];
        f.capture = Capture::Partial;
        let start = DaemonStatus::new(f).expect("a screen stream that the person stopped");
        for answer in [
            StopAnswer::Ok,
            StopAnswer::ThreadEnded,
            StopAnswer::Timeout,
            StopAnswer::Os,
        ] {
            let mut daemon = ScriptedDaemon::new(start.clone());
            daemon.set_stop_answer(SourceName::Screen, answer);
            let paused = changed(daemon.handle(Request::Pause));
            assert_eq!(
                line(&paused, SourceName::Screen),
                SourceLine {
                    name: SourceName::Screen,
                    state: SourceState::Stopped { by_user: true },
                    unstopped: false,
                },
                "{answer:?}"
            );
            assert_eq!(paused.fields().capture, Capture::Paused, "{answer:?}");
            let resumed = changed(daemon.handle(Request::Resume));
            assert_eq!(
                line(&resumed, SourceName::Screen).state,
                SourceState::Running,
                "{answer:?}"
            );
        }
    }

    #[test]
    fn a_pause_asks_a_marked_line_that_reads_off_and_keeps_it_off_whatever_the_stop_answers() {
        for answer in [
            StopAnswer::Ok,
            StopAnswer::ThreadEnded,
            StopAnswer::Timeout,
            StopAnswer::Os,
        ] {
            let mut f = sample_status(Platform::Windows).into_fields();
            for line in &mut f.sources {
                if line.name == SourceName::Keyboard {
                    line.unstopped = true;
                }
            }
            f.capture = Capture::Pausing;
            let mut daemon = ScriptedDaemon::new(DaemonStatus::new(f).unwrap());
            daemon.set_stop_answer(SourceName::Keyboard, answer);
            let paused = changed(daemon.handle(Request::Pause));
            assert_eq!(
                line(&paused, SourceName::Keyboard),
                SourceLine {
                    name: SourceName::Keyboard,
                    state: SourceState::Off,
                    unstopped: !answer.is_a_stop(),
                },
                "{answer:?}"
            );
        }
    }

    #[test]
    fn ct13_the_scripted_daemon_s_own_timed_out_pause_and_its_own_stop_leave_its_life_as_it_was() {
        for platform in [Platform::Macos, Platform::Windows] {
            let sample = sample_status(platform);
            let shown = sample.fields().store_id;
            let mut daemon = ScriptedDaemon::new(sample.clone());
            daemon.set_stop_answer(sample.fields().enabled_sources[0], StopAnswer::Timeout);
            let paused = changed(daemon.handle(Request::Pause));
            assert_eq!(paused.fields().capture, Capture::Pausing, "{platform:?}");
            assert_eq!(
                daemon.life,
                Life::Going,
                "{platform:?}: after the timed-out Pause"
            );
            let delete = daemon.handle(Request::DeleteAll { store: shown });
            assert!(
                matches!(&delete, Reply::Error(e) if e.code() == ErrorCode::Busy && !e.ending()),
                "{platform:?}: a delete after the timed-out Pause: {delete:?}"
            );
            let mut daemon = ScriptedDaemon::new(sample.clone());
            assert!(matches!(daemon.handle(Request::Stop), Reply::Stopped(_)));
            assert_eq!(daemon.life, Life::Going, "{platform:?}: after the Stop");
            let cap = daemon.handle(Request::SetStorageCap {
                bytes: StorageCap::MIN,
            });
            assert!(
                matches!(&cap, Reply::CapSet(set) if set.status.fields().cap_bytes == Cap::Known(StorageCap::MIN)),
                "{platform:?}: a set-cap after the Stop: {cap:?}"
            );
            let delete = daemon.handle(Request::DeleteAll { store: shown });
            assert!(
                matches!(delete, Reply::Deleted(_)),
                "{platform:?}: a delete with the shown id after the Stop: {delete:?}"
            );
        }
    }

    fn prepared(platform: Platform) -> Vec<DaemonStatus> {
        let mut out = walk_starts(platform);
        let mut f = sample_status(platform).into_fields();
        for line in &mut f.sources {
            if line.state != SourceState::Off {
                line.state = SourceState::Failed {
                    reason: Reason::Permission,
                    error: None,
                };
            }
        }
        f.capture = Capture::Faulted;
        f.capture_reason = Some(CaptureReason::Permission);
        out.push(DaemonStatus::new(f).unwrap());
        let mut f = sample_status(platform).into_fields();
        f.enabled_sources.clear();
        for line in &mut f.sources {
            line.state = SourceState::Off;
        }
        f.capture = Capture::Faulted;
        f.capture_reason = Some(CaptureReason::NoSource);
        out.push(DaemonStatus::new(f).unwrap());
        out
    }

    fn ending_in_order(status: &DaemonStatus, life_first: bool) -> ScriptedDaemon {
        let mut daemon = ScriptedDaemon::new(sample_status(status.fields().platform));
        if life_first {
            daemon.set_life(Life::Ending);
            daemon.set_status(status.clone());
        } else {
            daemon.set_status(status.clone());
            daemon.set_life(Life::Ending);
        }
        daemon
    }

    #[test]
    fn k3_both_orders_of_set_status_and_set_life_give_the_same_pausing_status_and_hold() {
        for platform in [Platform::Macos, Platform::Windows] {
            for status in prepared(platform) {
                let given = status.fields().capture;
                let life_first = ending_in_order(&status, true);
                let status_first = ending_in_order(&status, false);
                assert_eq!(
                    life_first.status(),
                    status_first.status(),
                    "{platform:?} {given:?}"
                );
                assert_eq!(life_first.hold, status_first.hold, "{platform:?} {given:?}");
                assert_eq!(
                    life_first.hold,
                    given == Capture::AwaitingResume,
                    "{platform:?} {given:?}"
                );
                let f = life_first.status().fields();
                assert_eq!(
                    (f.capture, f.capture_reason),
                    (Capture::Pausing, None),
                    "{platform:?} {given:?}"
                );
            }
        }
    }

    #[test]
    fn k3_after_either_order_a_resume_exits_1_with_its_sentence_and_a_pause_keeps_the_word() {
        let data_dir = crate::paths::DataDir::from_flag(if cfg!(windows) {
            r"C:\halvo-data"
        } else {
            "/halvo-data"
        })
        .unwrap();
        for platform in [Platform::Macos, Platform::Windows] {
            for status in prepared(platform) {
                for life_first in [true, false] {
                    let at = format!(
                        "{platform:?} {:?} life_first={life_first}",
                        status.fields().capture
                    );
                    let mut daemon = ending_in_order(&status, life_first);
                    let resumed = daemon.handle(Request::Resume);
                    let printed = crate::ctl_text::print(
                        &crate::ctl_text::Verb::Resume,
                        Ok(resumed),
                        &data_dir,
                    );
                    assert_eq!(printed.status, 1, "{at}: {printed:?}");
                    assert!(
                        printed
                            .stderr
                            .contains("capture=pausing, not the state that resume asks for"),
                        "{at}: {printed:?}"
                    );
                    let paused = changed(daemon.handle(Request::Pause));
                    assert_eq!(paused.fields().desired, CaptureState::Paused, "{at}");
                }
            }
        }
    }

    #[test]
    fn k6_set_life_ending_from_a_faulted_status_clears_the_reason() {
        for platform in [Platform::Macos, Platform::Windows] {
            let faulted = prepared(platform).pop().unwrap();
            assert_eq!(faulted.fields().capture, Capture::Faulted);
            let mut daemon = ScriptedDaemon::new(faulted);
            daemon.set_life(Life::Ending);
            let f = daemon.status().fields();
            assert_eq!(
                (f.capture, f.capture_reason),
                (Capture::Pausing, None),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn k6_set_life_going_changes_no_status_and_a_pausing_capture_stays() {
        for platform in [Platform::Macos, Platform::Windows] {
            for status in prepared(platform) {
                let mut daemon = ending_in_order(&status, true);
                let ending = daemon.status().clone();
                daemon.set_life(Life::Going);
                assert_eq!(daemon.status(), &ending, "{platform:?}");
                assert_eq!(daemon.life, Life::Going);
                let mut daemon = ScriptedDaemon::new(status.clone());
                daemon.set_life(Life::Going);
                assert_eq!(daemon.status(), &status, "{platform:?}");
            }
        }
    }

    fn ending_status(platform: Platform, desired: CaptureState) -> DaemonStatus {
        let sample = sample_status(platform);
        let first = sample.fields().enabled_sources[0];
        let mut f = sample.into_fields();
        for line in &mut f.sources {
            if line.state != SourceState::Off {
                line.state = SourceState::Stopped { by_user: false };
            }
            if line.name == first {
                line.state = SourceState::Failed {
                    reason: Reason::Timeout,
                    error: None,
                };
                line.unstopped = true;
            }
        }
        f.desired = desired;
        f.capture = Capture::Pausing;
        f.capture_reason = None;
        DaemonStatus::new(f).expect("pausing takes every state and the mark")
    }

    fn ending_daemon(status: DaemonStatus) -> ScriptedDaemon {
        let mut daemon = ScriptedDaemon::new(status);
        daemon.set_life(Life::Ending);
        daemon
    }

    fn is_the_busy_of_an_ending_life(reply: &Reply) -> bool {
        matches!(reply, Reply::Error(e) if e.code() == ErrorCode::Busy && e.ending())
    }

    #[test]
    fn pl17_in_a_life_that_is_ending_every_mutation_but_pause_and_resume_answers_the_typed_busy_and_writes_nothing(
    ) {
        for platform in [Platform::Macos, Platform::Windows] {
            let start = ending_status(platform, CaptureState::Running);
            for request in [
                Request::SetStorageCap {
                    bytes: StorageCap::MIN,
                },
                Request::AcknowledgeLostPermission {
                    permission: Permission::Accessibility,
                },
                Request::RequestPermission {
                    permission: Permission::Accessibility,
                },
                Request::Stop,
                Request::DeleteAll {
                    store: start.fields().store_id,
                },
                Request::DeleteAll { store: None },
            ] {
                let mut daemon = ending_daemon(start.clone());
                let reply = daemon.handle(request.clone());
                assert!(
                    is_the_busy_of_an_ending_life(&reply),
                    "{platform:?} {request:?}: {reply:?}"
                );
                assert_eq!(daemon.status(), &start, "{platform:?} {request:?}");
                assert_eq!(daemon.requests(), [request]);
            }
            let mut daemon = ending_daemon(start.clone());
            daemon.set_form(PermissionForm::A);
            let other_form = daemon.handle(Request::PermissionAsked {
                permission: Permission::Accessibility,
            });
            assert!(
                matches!(&other_form, Reply::Error(e) if e.code() == ErrorCode::UnsupportedOp && !e.ending()),
                "{platform:?}: {other_form:?}"
            );
            for read in [Request::Status { indicator: false }, Request::Inventory] {
                let reply = daemon.handle(read.clone());
                assert!(
                    !matches!(reply, Reply::Error(ref e) if e.code() == ErrorCode::Busy),
                    "{platform:?} {read:?}: {reply:?}"
                );
            }
        }
    }

    #[test]
    fn pl17_the_flag_of_a_life_that_is_ending_makes_capture_read_pausing() {
        for platform in [Platform::Macos, Platform::Windows] {
            let mut daemon = ScriptedDaemon::new(sample_status(platform));
            daemon.set_life(Life::Ending);
            assert_eq!(
                daemon.status().fields().capture,
                Capture::Pausing,
                "{platform:?}"
            );
            assert_eq!(daemon.status().fields().capture_reason, None);
            let resumed = changed(daemon.handle(Request::Resume));
            assert!(!crate::proto::reaches_the_end(
                crate::proto::CaptureChange::Resume,
                &resumed
            ));
        }
    }

    #[test]
    fn pl17_a_claim_in_a_life_that_is_ending_is_served_and_starts_no_source() {
        for platform in [Platform::Macos, Platform::Windows] {
            let mut f = ending_status(platform, CaptureState::Running).into_fields();
            f.lease = Lease::Lapsed;
            let start = DaemonStatus::new(f).unwrap();
            let mut daemon = ending_daemon(start.clone());
            let reply = daemon.handle(Request::Status { indicator: true });
            assert_eq!(reply, Reply::Status(start.clone()), "{platform:?}");
            assert_eq!(daemon.status(), &start);
        }
    }

    #[test]
    fn pl17_a_resume_in_a_life_that_is_ending_answers_the_status_as_it_stands_and_keeps_desired_and_the_hold(
    ) {
        for platform in [Platform::Macos, Platform::Windows] {
            for desired in [CaptureState::Running, CaptureState::Paused] {
                let start = ending_status(platform, desired);
                for hold in [false, true] {
                    let mut daemon = ending_daemon(start.clone());
                    daemon.hold = hold;
                    let resumed = changed(daemon.handle(Request::Resume));
                    assert_eq!(resumed, start, "{platform:?} {desired:?} hold {hold}");
                    assert_eq!(daemon.status(), &start);
                    assert_eq!(daemon.hold, hold, "{platform:?} {desired:?}");
                    assert!(!crate::proto::reaches_the_end(
                        crate::proto::CaptureChange::Resume,
                        &resumed
                    ));
                }
            }
        }
    }

    #[test]
    fn pl17_a_pause_in_a_life_that_is_ending_persists_desired_paused_and_the_next_read_shows_it() {
        for platform in [Platform::Macos, Platform::Windows] {
            for desired in [CaptureState::Running, CaptureState::Paused] {
                let start = ending_status(platform, desired);
                let mut daemon = ending_daemon(start.clone());
                daemon.hold = true;
                daemon.set_stop_answer(start.fields().enabled_sources[0], StopAnswer::Ok);
                let paused = changed(daemon.handle(Request::Pause));
                assert!(
                    daemon.hold,
                    "{platform:?} {desired:?}: the Pause cleared the hold"
                );
                let mut want = start.clone().into_fields();
                want.desired = CaptureState::Paused;
                assert_eq!(
                    paused.into_fields(),
                    want.clone(),
                    "{platform:?} {desired:?}"
                );
                let read = match daemon.handle(Request::Status { indicator: true }) {
                    Reply::Status(status) => status,
                    other => panic!("{other:?}"),
                };
                assert_eq!(read.into_fields(), want, "{platform:?} {desired:?}");
            }
        }
    }

    #[test]
    fn pl17_in_a_stop_sequence_that_is_not_ending_a_pause_is_served_as_before_and_the_next_claim_starts_no_source(
    ) {
        for platform in [Platform::Macos, Platform::Windows] {
            let mut f = ending_status(platform, CaptureState::Running).into_fields();
            f.lease = Lease::Lapsed;
            let start = DaemonStatus::new(f).unwrap();
            let first = start.fields().enabled_sources[0];
            let mut daemon = ScriptedDaemon::new(start.clone());
            daemon.set_stop_answer(first, StopAnswer::Ok);
            let paused = changed(daemon.handle(Request::Pause));
            assert_eq!(paused.fields().capture, Capture::Pausing, "{platform:?}");
            assert_eq!(
                paused.fields().desired,
                CaptureState::Paused,
                "{platform:?}: the Pause in a stop sequence keeps the person's word"
            );
            assert_eq!(
                line(&paused, first),
                SourceLine {
                    name: first,
                    state: SourceState::Stopped { by_user: false },
                    unstopped: false,
                },
                "{platform:?}: the Pause asks the marked source again"
            );
            let claimed = match daemon.handle(Request::Status { indicator: true }) {
                Reply::Status(status) => status,
                other => panic!("{other:?}"),
            };
            let running = claimed
                .fields()
                .sources
                .iter()
                .filter(|l| l.state == SourceState::Running)
                .count();
            assert_eq!(
                running, 0,
                "{platform:?}: the claim started {running} sources"
            );
        }
    }

    #[test]
    fn pl17_mark_01_a_pause_in_a_life_that_is_ending_persists_desired_paused() {
        for platform in [Platform::Macos, Platform::Windows] {
            let sample = sample_status(platform);
            let first = sample.fields().enabled_sources[0];
            let mut f = ending_status(platform, CaptureState::Paused).into_fields();
            f.lease = Lease::Lapsed;
            f.capture = Capture::Starting;
            let mut walked = ScriptedDaemon::new(DaemonStatus::new(f).unwrap());
            let via = changed(walked.handle(Request::Resume));
            assert_eq!(
                (via.fields().desired, via.fields().capture),
                (CaptureState::Running, Capture::Pausing),
                "{platform:?}: the scripted daemon publishes pausing with desired running"
            );
            for start in [ending_status(platform, CaptureState::Running), via] {
                let mut daemon = ending_daemon(start);
                daemon.set_stop_answer(first, StopAnswer::Timeout);
                let status = changed(daemon.handle(Request::Pause));
                assert_eq!(status.fields().capture, Capture::Pausing, "{platform:?}");
                assert_eq!(
                    status.fields().desired,
                    CaptureState::Paused,
                    "{platform:?}: the person's pause was not kept"
                );
            }
        }
    }

    #[test]
    fn pl17_mark_02_a_resume_in_a_life_that_is_ending_beside_a_marked_source_starts_nothing() {
        for platform in [Platform::Macos, Platform::Windows] {
            let mut f = ending_status(platform, CaptureState::Paused).into_fields();
            f.capture = Capture::Starting;
            let start = DaemonStatus::new(f).expect("starting takes the mark");
            let mut daemon = ending_daemon(start);
            let before = daemon.status().clone();
            let status = changed(daemon.handle(Request::Resume));
            let started = status
                .fields()
                .sources
                .iter()
                .filter(|l| l.state == SourceState::Running)
                .count();
            assert_eq!(
                started, 0,
                "{platform:?}: a Resume in a life that is ending started {started} sources"
            );
            assert_eq!(status, before, "{platform:?}");
            assert!(
                !crate::proto::reaches_the_end(crate::proto::CaptureChange::Resume, &status),
                "{platform:?}: the resume reached its end, ctl exits 0"
            );
        }
    }
}
