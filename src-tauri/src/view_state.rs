use halvo_control::client::{ClientError, DeleteOutcome};
use halvo_control::model::{CaptureState, ObservationWindow, StorageCap, StoreId};
use halvo_control::proto::{
    Cap, Capture, CaptureReason, DaemonStatus, DeleteFields, DrainError, ErrorCode, ExpiryError,
    Health, InventoryReply, LastStop, Lease, LostRecord, PasswordGuard, Platform, RootDestroyed,
    Section, SourceName, SourceStateName, StopKind, StoreErrorKind, StoreState,
};
use halvo_control::texts::{texts, Texts};
use serde::Serialize;

/// What the app knows of the capture service (contracts.md C10.2 `service`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Connecting,
    Running,
    NotRunning,
    HeldUnreachable,
    StatusRefused,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoreView {
    pub state: StoreState,
    pub id: Option<StoreId>,
    pub error: Option<StoreErrorKind>,
    pub retry_in_s: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrainView {
    pub state: Health,
    pub error: Option<DrainError>,
    pub since: Option<halvo_control::model::UtcSecond>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExpiryView {
    pub state: Health,
    pub error: Option<ExpiryError>,
    pub last_at: Option<halvo_control::model::UtcSecond>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StopSourceView {
    pub name: SourceName,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LossView {
    pub events_pending: u64,
    pub events_lost: u64,
    pub last_stop: LastStop,
    pub last_stop_newest_capture: Option<halvo_control::model::UtcSecond>,
    pub last_stop_kind: Option<StopKind>,
    pub last_stop_lost: LostRecord,
    pub last_stop_sources: Vec<StopSourceView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceView {
    pub name: SourceName,
    pub state: SourceStateName,
    pub reason: Option<halvo_control::proto::Reason>,
    pub error: Option<String>,
}

/// The view of the capture service (contracts.md C10.2). With any `service` but `running`
/// every field that the daemon reports is null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusView {
    pub service: Service,
    pub last_exit: Option<i32>,
    pub platform: Platform,
    pub desired: Option<CaptureState>,
    pub capture: Option<Capture>,
    pub capture_reason: Option<CaptureReason>,
    pub lease: Option<Lease>,
    pub store: Option<StoreView>,
    pub store_bytes: Option<u64>,
    pub store_logical_bytes: Option<u64>,
    pub store_cap_bytes: Option<u64>,
    pub window: ObservationWindow,
    pub drain: Option<DrainView>,
    pub expiry: Option<ExpiryView>,
    pub loss: Option<LossView>,
    pub sources: Option<Vec<SourceView>>,
    pub lost_grants: Option<Vec<halvo_control::onboarding::Permission>>,
    pub password_guard: Option<PasswordGuard>,
    pub screen_filter_excludes_app: Option<bool>,
    pub texts: Option<Texts>,
    pub ext: Option<Vec<Section>>,
}

/// The name by which a module's registration receives the view (contracts.md C10.1).
pub type Status = StatusView;

pub const APP_PLATFORM: Platform = if cfg!(windows) {
    Platform::Windows
} else {
    Platform::Macos
};

impl StatusView {
    /// The view of a service that gave no status: every daemon field null.
    pub fn without_status(service: Service) -> Self {
        StatusView {
            service,
            last_exit: None,
            platform: APP_PLATFORM,
            desired: None,
            capture: None,
            capture_reason: None,
            lease: None,
            store: None,
            store_bytes: None,
            store_logical_bytes: None,
            store_cap_bytes: None,
            window: ObservationWindow::r1(),
            drain: None,
            expiry: None,
            loss: None,
            sources: None,
            lost_grants: None,
            password_guard: None,
            screen_filter_excludes_app: None,
            texts: None,
            ext: None,
        }
    }

    /// The view of a status that the daemon reported.
    pub fn of(status: &DaemonStatus) -> Self {
        let f = status.fields();
        StatusView {
            service: Service::Running,
            last_exit: None,
            platform: f.platform,
            desired: Some(f.desired),
            capture: Some(f.capture),
            capture_reason: f.capture_reason,
            lease: Some(f.lease),
            store: Some(StoreView {
                state: f.store,
                id: f.store_id,
                error: f.store_error,
                retry_in_s: f.store_retry_in_s,
            }),
            store_bytes: f.store_bytes,
            store_logical_bytes: f.store_logical_bytes,
            store_cap_bytes: match f.cap_bytes {
                Cap::Known(cap) => Some(cap.bytes()),
                Cap::Unknown => None,
            },
            window: ObservationWindow::r1(),
            drain: Some(DrainView {
                state: f.drain,
                error: f.drain_error,
                since: f.drain_since,
            }),
            expiry: Some(ExpiryView {
                state: f.expiry,
                error: f.expiry_error,
                last_at: f.last_expiry_at,
            }),
            loss: Some(LossView {
                events_pending: f.events_pending,
                events_lost: f.events_lost,
                last_stop: f.last_stop,
                last_stop_newest_capture: f.last_stop_newest_capture,
                last_stop_kind: f.last_stop_kind,
                last_stop_lost: f.last_stop_lost.clone(),
                last_stop_sources: f
                    .last_stop_sources
                    .iter()
                    .map(|line| StopSourceView {
                        name: line.name,
                        error: line.error.to_string(),
                    })
                    .collect(),
            }),
            sources: Some(
                f.sources
                    .iter()
                    .map(|line| SourceView {
                        name: line.name,
                        state: line.state.name(),
                        reason: line.state.reason(),
                        error: line.state.error().map(ToString::to_string),
                    })
                    .collect(),
            ),
            lost_grants: Some(f.onboarding.lost_grants.clone()),
            password_guard: Some(f.password_guard),
            screen_filter_excludes_app: f.screen_filter_excludes_app,
            texts: Some(texts(status)),
            ext: Some(f.ext.clone()),
        }
    }
}

/// An error of a command that the page words itself (contracts.md C10.1: a bad argument,
/// `busy`, a deadline, or a reply that the app cannot use).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum ActionError {
    Busy,
    Deadline { seconds: u64 },
    InvalidArgument,
    CapOutOfRange { min_bytes: u64, max_bytes: u64 },
    BadStoreId,
    UpdateTheApp,
    StoreUnavailable,
    StateFile,
    Internal,
    PeerRejected,
    BrokenReply,
    ChannelFailed,
    PaneNotOpened,
    NotAnswering { service: Service },
}

impl ActionError {
    pub fn cap_out_of_range() -> Self {
        ActionError::CapOutOfRange {
            min_bytes: StorageCap::MIN.bytes(),
            max_bytes: StorageCap::MAX.bytes(),
        }
    }
}

/// How the app reads an error of the client: the service that the view shows for it when a
/// status was asked, and the action error that a command returns for it, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub service: Service,
    pub action: Option<ActionError>,
}

/// The one reading of the client's errors. Each variant has its own arm, so a new variant of
/// `ClientError` or of `ErrorCode` is an arm that the compiler asks for.
pub fn read_error(error: &ClientError) -> Reading {
    let (service, action) = match error {
        ClientError::Unsupported => (Service::Unsupported, None),
        ClientError::Unreachable { .. } => (Service::NotRunning, None),
        ClientError::DataDirInUse { .. } => (Service::HeldUnreachable, None),
        ClientError::StateDoesNotHoldTogether(_) => (Service::StatusRefused, None),
        ClientError::PeerRejected(_) => (Service::HeldUnreachable, Some(ActionError::PeerRejected)),
        ClientError::Deadline { seconds } => (
            Service::HeldUnreachable,
            Some(ActionError::Deadline { seconds: *seconds }),
        ),
        ClientError::Io(_) => (Service::HeldUnreachable, Some(ActionError::ChannelFailed)),
        ClientError::Protocol(_) => (Service::StatusRefused, Some(ActionError::BrokenReply)),
        ClientError::UnexpectedReply { .. } => {
            (Service::StatusRefused, Some(ActionError::BrokenReply))
        }
        ClientError::Daemon(reply) => (
            Service::StatusRefused,
            Some(match reply.code {
                ErrorCode::Busy => ActionError::Busy,
                ErrorCode::InvalidArgument => ActionError::InvalidArgument,
                ErrorCode::UnsupportedVersion | ErrorCode::UnsupportedOp => {
                    ActionError::UpdateTheApp
                }
                ErrorCode::StoreUnavailable => ActionError::StoreUnavailable,
                ErrorCode::StateFile => ActionError::StateFile,
                ErrorCode::Internal => ActionError::Internal,
                ErrorCode::PeerRejected => ActionError::PeerRejected,
            }),
        ),
    };
    Reading { service, action }
}

/// The view for the answer to a status request.
pub fn view_of(answer: &Result<DaemonStatus, ClientError>) -> StatusView {
    match answer {
        Ok(status) => StatusView::of(status),
        Err(error) => StatusView::without_status(read_error(error).service),
    }
}

/// The view that a command returns for the answer to a request whose reply carries a status:
/// a state of the service is a view, and an action error is the command's error.
pub fn command_view(answer: Result<DaemonStatus, ClientError>) -> Result<StatusView, ActionError> {
    match answer {
        Ok(status) => Ok(StatusView::of(&status)),
        Err(error) => {
            let reading = read_error(&error);
            match reading.action {
                Some(action) => Err(action),
                None => Ok(StatusView::without_status(reading.service)),
            }
        }
    }
}

/// The reply of `set_capture_state` (contracts.md C10.4): the view of the reply, and whether
/// the reply reached the end of the verb by `proto::reaches_the_end`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureSet {
    #[serde(flatten)]
    pub view: StatusView,
    pub reached: bool,
}

/// The reply of `set_storage_cap` (contracts.md C10.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapSetView {
    #[serde(flatten)]
    pub view: StatusView,
    pub evicting: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InventoryRowView {
    pub day: halvo_control::model::UtcDay,
    pub modality: halvo_control::model::Modality,
    pub events: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TotalsView {
    pub events: u64,
    pub bytes: u64,
    pub disk_bytes: u64,
}

/// The reply of `inventory` (contracts.md C10.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InventoryView {
    pub available: bool,
    pub store_id: Option<StoreId>,
    pub as_of: Option<halvo_control::model::UtcSecond>,
    pub day_is: &'static str,
    pub rows: Vec<InventoryRowView>,
    pub totals: Option<TotalsView>,
    pub ext: Vec<Section>,
}

impl InventoryView {
    pub fn unavailable() -> Self {
        InventoryView {
            available: false,
            store_id: None,
            as_of: None,
            day_is: "utc",
            rows: Vec::new(),
            totals: None,
            ext: Vec::new(),
        }
    }

    pub fn of(reply: &InventoryReply) -> Self {
        let totals = reply.totals();
        InventoryView {
            available: true,
            store_id: Some(reply.store()),
            as_of: Some(reply.as_of()),
            day_is: "utc",
            rows: reply
                .rows()
                .rows()
                .iter()
                .map(|row| InventoryRowView {
                    day: row.day,
                    modality: row.modality,
                    events: row.events,
                    bytes: row.bytes,
                })
                .collect(),
            totals: Some(TotalsView {
                events: totals.events,
                bytes: totals.bytes,
                disk_bytes: totals.disk_bytes,
            }),
            ext: reply.ext().to_vec(),
        }
    }
}

/// The view of an inventory answer: a store that is not open, and a service that does not
/// answer, read as not available; an action error stays the command's error.
pub fn inventory_view(
    answer: Result<InventoryReply, ClientError>,
) -> Result<InventoryView, ActionError> {
    match answer {
        Ok(reply) => Ok(InventoryView::of(&reply)),
        Err(error) => match read_error(&error).action {
            None | Some(ActionError::StoreUnavailable) => Ok(InventoryView::unavailable()),
            Some(action) => Err(action),
        },
    }
}

/// The reply of `delete_all` (contracts.md C10.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DeleteView {
    Deleted {
        segments_shredded: Option<u64>,
        keys_destroyed: Option<u64>,
        root_destroyed: RootDestroyed,
        files_left: u64,
        database_left: bool,
        not_followed: u64,
        entries_left: u64,
        unknown_entries: u64,
        new_store_id: Option<StoreId>,
        cleanup_command: Option<String>,
        looked_in: Option<String>,
        nothing_left: bool,
    },
    AlreadyDeleted {
        current_store_id: StoreId,
    },
    DeleteRefused,
}

/// Whether a delete left nothing: the root is destroyed and the delete followed nothing
/// (ruling ST-30 (2)(e), (2)(f)).
pub fn left_nothing(report: &DeleteFields) -> bool {
    report.root_destroyed == RootDestroyed::Yes && report.not_followed == 0
}

/// The command that removes the root file that a delete left, for the root of `root_id`
/// under `roots_dir`.
pub fn cleanup_command(roots_dir: &std::path::Path, root_id: StoreId) -> String {
    let file = roots_dir.join(format!("{root_id}.{}", halvo_control::paths::ROOT_FILE_EXT));
    let text = file.display().to_string();
    if cfg!(windows) {
        format!("del \"{text}\"")
    } else {
        format!("rm '{}'", text.replace('\'', r"'\''"))
    }
}

impl DeleteView {
    pub fn of(outcome: &DeleteOutcome, roots_dir: Option<&std::path::Path>) -> Self {
        match outcome {
            DeleteOutcome::Deleted(deleted) => {
                let f = deleted.report.fields();
                DeleteView::Deleted {
                    segments_shredded: f.segments_shredded,
                    keys_destroyed: f.keys_destroyed,
                    root_destroyed: f.root_destroyed,
                    files_left: f.files_left,
                    database_left: f.database_left,
                    not_followed: f.not_followed,
                    entries_left: f.entries_left,
                    unknown_entries: f.unknown_entries,
                    new_store_id: deleted.store,
                    cleanup_command: match (f.root_destroyed, f.root_id, roots_dir) {
                        (RootDestroyed::No, Some(id), Some(dir)) => Some(cleanup_command(dir, id)),
                        _ => None,
                    },
                    looked_in: None,
                    nothing_left: left_nothing(f),
                }
            }
            DeleteOutcome::AlreadyDeleted(already) => DeleteView::AlreadyDeleted {
                current_store_id: already.current,
            },
            DeleteOutcome::Refused => DeleteView::DeleteRefused,
        }
    }
}

/// The view of a delete answer: a service that does not answer is the command's error, and
/// never a delete that is done.
pub fn delete_view(
    answer: Result<DeleteOutcome, ClientError>,
    roots_dir: Option<&std::path::Path>,
) -> Result<DeleteView, ActionError> {
    match answer {
        Ok(outcome) => Ok(DeleteView::of(&outcome, roots_dir)),
        Err(error) => {
            let reading = read_error(&error);
            Err(reading.action.unwrap_or(ActionError::NotAnswering {
                service: reading.service,
            }))
        }
    }
}
