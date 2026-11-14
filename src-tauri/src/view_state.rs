use halvo_control::client::{ClientError, DeleteOutcome};
use halvo_control::model::{CaptureState, ObservationWindow, StorageCap, StoreId};
use halvo_control::onboarding::Permission;
use halvo_control::proto::{
    Cap, Capture, CaptureReason, DaemonStatus, DeleteFields, DrainError, ErrorCode, ExpiryError,
    Health, InventoryReply, LastStop, Lease, LostRecord, PasswordGuard, Platform, RootDestroyed,
    Section, SourceName, SourceStateName, StopKind, StoreErrorKind, StoreState,
};
use halvo_control::texts::{texts, Texts};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Connecting,
    Running,
    NotRunning,
    HeldUnreachable,
    StatusRefused,
    Unsupported,
    AnotherVersion,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoStatusService {
    Connecting,
    NotRunning,
    HeldUnreachable,
    StatusRefused,
    Unsupported,
    AnotherVersion,
}

impl NoStatusService {
    pub const fn word(self) -> Service {
        match self {
            NoStatusService::Connecting => Service::Connecting,
            NoStatusService::NotRunning => Service::NotRunning,
            NoStatusService::HeldUnreachable => Service::HeldUnreachable,
            NoStatusService::StatusRefused => Service::StatusRefused,
            NoStatusService::Unsupported => Service::Unsupported,
            NoStatusService::AnotherVersion => Service::AnotherVersion,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningView {
    pub platform: Platform,
    pub window: ObservationWindow,
    pub desired: CaptureState,
    pub capture: Capture,
    pub capture_reason: Option<CaptureReason>,
    pub lease: Lease,
    pub store: StoreView,
    pub store_bytes: Option<u64>,
    pub store_logical_bytes: Option<u64>,
    pub store_cap_bytes: Option<u64>,
    pub drain: DrainView,
    pub expiry: ExpiryView,
    pub loss: LossView,
    pub sources: Vec<SourceView>,
    pub lost_grants: Vec<Permission>,
    pub password_guard: PasswordGuard,
    pub screen_filter_excludes_app: Option<bool>,
    pub texts: Texts,
    pub ext: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoStatusView {
    pub service: NoStatusService,
    pub platform: Platform,
    pub window: ObservationWindow,
    pub last_exit: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusView {
    Running(Box<RunningView>),
    NoStatus(NoStatusView),
}

#[derive(Serialize)]
struct WireView<'a> {
    service: Service,
    last_exit: Option<i32>,
    platform: Platform,
    desired: Option<CaptureState>,
    capture: Option<Capture>,
    capture_reason: Option<CaptureReason>,
    lease: Option<Lease>,
    store: Option<&'a StoreView>,
    store_bytes: Option<u64>,
    store_logical_bytes: Option<u64>,
    store_cap_bytes: Option<u64>,
    window: ObservationWindow,
    drain: Option<&'a DrainView>,
    expiry: Option<&'a ExpiryView>,
    loss: Option<&'a LossView>,
    sources: Option<&'a [SourceView]>,
    lost_grants: Option<&'a [Permission]>,
    password_guard: Option<PasswordGuard>,
    screen_filter_excludes_app: Option<bool>,
    texts: Option<&'a Texts>,
    ext: Option<&'a [Section]>,
}

impl Serialize for StatusView {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            StatusView::Running(view) => WireView {
                service: Service::Running,
                last_exit: None,
                platform: view.platform,
                desired: Some(view.desired),
                capture: Some(view.capture),
                capture_reason: view.capture_reason,
                lease: Some(view.lease),
                store: Some(&view.store),
                store_bytes: view.store_bytes,
                store_logical_bytes: view.store_logical_bytes,
                store_cap_bytes: view.store_cap_bytes,
                window: view.window,
                drain: Some(&view.drain),
                expiry: Some(&view.expiry),
                loss: Some(&view.loss),
                sources: Some(&view.sources),
                lost_grants: Some(&view.lost_grants),
                password_guard: Some(view.password_guard),
                screen_filter_excludes_app: view.screen_filter_excludes_app,
                texts: Some(&view.texts),
                ext: Some(&view.ext),
            },
            StatusView::NoStatus(view) => WireView {
                service: view.service.word(),
                last_exit: view.last_exit,
                platform: view.platform,
                desired: None,
                capture: None,
                capture_reason: None,
                lease: None,
                store: None,
                store_bytes: None,
                store_logical_bytes: None,
                store_cap_bytes: None,
                window: view.window,
                drain: None,
                expiry: None,
                loss: None,
                sources: None,
                lost_grants: None,
                password_guard: None,
                screen_filter_excludes_app: None,
                texts: None,
                ext: None,
            },
        };
        wire.serialize(serializer)
    }
}

pub type Status = StatusView;

pub const APP_PLATFORM: Platform = if cfg!(windows) {
    Platform::Windows
} else {
    Platform::Macos
};

impl StatusView {
    pub fn no_status(service: NoStatusService, platform: Platform) -> Self {
        StatusView::NoStatus(NoStatusView {
            service,
            platform,
            window: ObservationWindow::r1(),
            last_exit: None,
        })
    }

    pub fn without_status(service: NoStatusService) -> Self {
        StatusView::no_status(service, APP_PLATFORM)
    }

    #[cfg(test)]
    pub fn service(&self) -> Service {
        match self {
            StatusView::Running(_) => Service::Running,
            StatusView::NoStatus(view) => view.service.word(),
        }
    }

    pub fn of(status: &DaemonStatus) -> Self {
        let f = status.fields();
        StatusView::Running(Box::new(RunningView {
            platform: f.platform,
            window: ObservationWindow::r1(),
            desired: f.desired,
            capture: f.capture,
            capture_reason: f.capture_reason,
            lease: f.lease,
            store: StoreView {
                state: f.store,
                id: f.store_id,
                error: f.store_error,
                retry_in_s: f.store_retry_in_s,
            },
            store_bytes: f.store_bytes,
            store_logical_bytes: f.store_logical_bytes,
            store_cap_bytes: match f.cap_bytes {
                Cap::Known(cap) => Some(cap.bytes()),
                Cap::Unknown => None,
            },
            drain: DrainView {
                state: f.drain,
                error: f.drain_error,
                since: f.drain_since,
            },
            expiry: ExpiryView {
                state: f.expiry,
                error: f.expiry_error,
                last_at: f.last_expiry_at,
            },
            loss: LossView {
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
            },
            sources: f
                .sources
                .iter()
                .map(|line| SourceView {
                    name: line.name,
                    state: line.state.name(),
                    reason: line.state.reason(),
                    error: line.state.error().map(ToString::to_string),
                })
                .collect(),
            lost_grants: f.onboarding.lost_grants.clone(),
            password_guard: f.password_guard,
            screen_filter_excludes_app: f.screen_filter_excludes_app,
            texts: texts(status),
            ext: f.ext.clone(),
        }))
    }
}

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
    NotAnswering { service: NoStatusService },
}

impl ActionError {
    pub fn cap_out_of_range() -> Self {
        ActionError::CapOutOfRange {
            min_bytes: StorageCap::MIN.bytes(),
            max_bytes: StorageCap::MAX.bytes(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub service: NoStatusService,
    pub command_error: Option<ActionError>,
}

#[deny(clippy::wildcard_enum_match_arm)]
pub fn read_error(error: &ClientError) -> Reading {
    let (service, command_error) = match error {
        ClientError::Unsupported => (NoStatusService::Unsupported, None),
        ClientError::Unreachable { .. } => (NoStatusService::NotRunning, None),
        ClientError::DataDirInUse { .. } => (NoStatusService::HeldUnreachable, None),
        ClientError::StateDoesNotHoldTogether(_) => (NoStatusService::StatusRefused, None),
        ClientError::ReplyOfAnotherVersion { .. } | ClientError::ValueUnknownToV1(_) => (
            NoStatusService::AnotherVersion,
            Some(ActionError::UpdateTheApp),
        ),
        ClientError::PeerRejected(_) => (
            NoStatusService::HeldUnreachable,
            Some(ActionError::PeerRejected),
        ),
        ClientError::Deadline { seconds } => (
            NoStatusService::HeldUnreachable,
            Some(ActionError::Deadline { seconds: *seconds }),
        ),
        ClientError::Io(_) => (
            NoStatusService::HeldUnreachable,
            Some(ActionError::ChannelFailed),
        ),
        ClientError::Protocol(_) => (
            NoStatusService::StatusRefused,
            Some(ActionError::BrokenReply),
        ),
        ClientError::UnexpectedReply { .. } => (
            NoStatusService::StatusRefused,
            Some(ActionError::BrokenReply),
        ),
        ClientError::Daemon(reply) => (
            match reply.code {
                ErrorCode::UnsupportedVersion | ErrorCode::UnsupportedOp => {
                    NoStatusService::AnotherVersion
                }
                ErrorCode::Busy
                | ErrorCode::InvalidArgument
                | ErrorCode::StoreUnavailable
                | ErrorCode::StateFile
                | ErrorCode::Internal
                | ErrorCode::PeerRejected => NoStatusService::StatusRefused,
            },
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
    Reading {
        service,
        command_error,
    }
}

pub fn view_of(answer: &Result<DaemonStatus, ClientError>) -> StatusView {
    match answer {
        Ok(status) => StatusView::of(status),
        Err(error) => StatusView::without_status(read_error(error).service),
    }
}

pub fn command_view(answer: Result<DaemonStatus, ClientError>) -> Result<StatusView, ActionError> {
    match answer {
        Ok(status) => Ok(StatusView::of(&status)),
        Err(error) => {
            let reading = read_error(&error);
            match reading.command_error {
                Some(action) => Err(action),
                None => Ok(StatusView::without_status(reading.service)),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureSet {
    #[serde(flatten)]
    pub view: StatusView,
    pub reached: bool,
}

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

pub fn inventory_view(
    answer: Result<InventoryReply, ClientError>,
) -> Result<InventoryView, ActionError> {
    match answer {
        Ok(reply) => Ok(InventoryView::of(&reply)),
        Err(error) => match read_error(&error).command_error {
            None | Some(ActionError::StoreUnavailable) => Ok(InventoryView::unavailable()),
            Some(action) => Err(action),
        },
    }
}

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

pub fn left_nothing(report: &DeleteFields) -> bool {
    report.root_destroyed == RootDestroyed::Yes && report.not_followed == 0
}

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

pub fn delete_view(
    answer: Result<DeleteOutcome, ClientError>,
    roots_dir: Option<&std::path::Path>,
) -> Result<DeleteView, ActionError> {
    match answer {
        Ok(outcome) => Ok(DeleteView::of(&outcome, roots_dir)),
        Err(error) => {
            let reading = read_error(&error);
            Err(reading.command_error.unwrap_or(ActionError::NotAnswering {
                service: reading.service,
            }))
        }
    }
}
