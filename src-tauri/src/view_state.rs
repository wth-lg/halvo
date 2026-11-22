use halvo_control::client::{ClientError, DeleteOutcome};
use halvo_control::exit;
use halvo_control::model::{CaptureState, ObservationWindow, StorageCap, StoreId};
use halvo_control::onboarding::{GrantState, OnboardingState, Permission};
use halvo_control::proto::{
    Cap, Capture, CaptureReason, DaemonStatus, DeleteFields, DeleteReport, DrainError, ErrorCode,
    ExpiryError, Health, InventoryReply, LastStop, Lease, LostRecord, OnOff, PasswordGuard,
    Platform, RootDestroyed, Section, SourceName, SourceStateName, StateFile, StopKind,
    StoreErrorKind, StoreState, SwitchName,
};
use halvo_control::texts::{first_run, texts, Texts};
use serde::{Deserialize, Serialize};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

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
    NotStarted,
    DataDirUnusable,
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

/// Why the service's settings file did not load (contracts.md C3.7), as a view names it: the
/// page then says so in one sentence and offers the reset that Delete everything gives. A file
/// that loaded, and one whose last write failed, give no such word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsUnread {
    Corrupt,
    NewerVersion,
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
    pub hold: Option<HoldView>,
    pub login_item_loaded: Option<bool>,
    pub login_item_written: Option<bool>,
    pub data_folder: Option<String>,
    pub state_file: Option<SettingsUnread>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoStatusView {
    pub service: NoStatusService,
    pub platform: Platform,
    pub window: ObservationWindow,
    pub last_exit: Option<i32>,
    pub hold: Option<HoldView>,
    pub login_item_loaded: Option<bool>,
    pub login_item_written: Option<bool>,
    pub first_run: Option<FirstRun>,
    pub data_folder: Option<String>,
}

/// What a view without a status says before the app's first child (contracts.md C10.7): on a
/// data directory that is not provisioned the not-started state, whose Start is the consent,
/// with the first-run text of halvo-control's table; on a directory whose name leads nowhere,
/// is no directory or cannot be read its own cause, and never the consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstRun {
    NotStarted(Texts),
    DataDirUnusable,
}

impl NoStatusView {
    fn word(&self) -> Service {
        match self.first_run {
            Some(FirstRun::NotStarted(_)) => Service::NotStarted,
            Some(FirstRun::DataDirUnusable) => Service::DataDirUnusable,
            None => self.service.word(),
        }
    }
}

/// Why the app holds capture off: a child of the app ended itself with status 5, because a
/// source did not stop (contracts.md C14 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldCause {
    SourceNotStopped,
}

/// The press that ends the hold: Resume while the app starts its children, Start after the
/// third end with 5 in a row, when no further child starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Press {
    Resume,
    Start,
}

/// What a view says of the app's hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HoldView {
    pub cause: HoldCause,
    pub press: Press,
}

#[derive(Debug, Default)]
struct HoldState {
    held: bool,
    last_exit: Option<i32>,
    child_lives: bool,
    no_more_child: bool,
    start_asked: bool,
    another_version: bool,
    lives: u64,
    ended: Option<(u64, Option<i32>)>,
    between: bool,
    folder: Option<String>,
    awaits_consent: bool,
    data_dir_unusable: bool,
    consenting: bool,
    login_item_written: Option<bool>,
}

/// A consent click in flight, from `Hold::begin_consent` to its drop.
#[derive(Debug)]
pub struct Consenting<'a>(&'a Hold);

impl Drop for Consenting<'_> {
    fn drop(&mut self) {
        self.0.state().consenting = false;
    }
}

/// The app's hold on capture after a child of the app ended itself with status 5 (contracts.md
/// C14 item 4), and beside it what the supervisor of the children tells the view: the status of
/// the last end while no child lives, whether no further child starts, and a Start that waits
/// for the supervisor. An end with 5 sets the hold, and only a Resume of a person's press that
/// reached its end clears it, by [`Hold::release`].
#[derive(Debug, Default)]
pub struct Hold {
    state: Mutex<HoldState>,
    changed: Condvar,
}

impl Hold {
    fn state(&self) -> MutexGuard<'_, HoldState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the hold stands.
    pub fn stands(&self) -> bool {
        self.state().held
    }

    /// The hold as a view says it, none while it does not stand.
    pub fn view(&self) -> Option<HoldView> {
        let state = self.state();
        state.held.then_some(HoldView {
            cause: HoldCause::SourceNotStopped,
            press: if state.no_more_child {
                Press::Start
            } else {
                Press::Resume
            },
        })
    }

    /// `view` with the hold, and on `not_running` the status of the last end while no child of
    /// the app lives; while the app leaves its own child of another daemon_version, a running
    /// view reads another_version. A view without a status reads data_dir_unusable while the
    /// supervisor found the data directory's own cause, and a not_running view reads
    /// not_started while the supervisor awaits the consent.
    pub fn mark(&self, view: StatusView) -> StatusView {
        let hold = self.view();
        let (last_exit, another_version, folder, awaits_consent, data_dir_unusable, written) = {
            let state = self.state();
            (
                state.last_exit,
                state.another_version,
                state.folder.clone(),
                state.awaits_consent,
                state.data_dir_unusable,
                state.login_item_written,
            )
        };
        let view = match view {
            StatusView::Running(running) if another_version => {
                StatusView::no_status(NoStatusService::AnotherVersion, running.platform)
            }
            StatusView::NoStatus(mut view) if data_dir_unusable => {
                view.first_run = Some(FirstRun::DataDirUnusable);
                StatusView::NoStatus(view)
            }
            StatusView::NoStatus(mut view)
                if awaits_consent && view.service == NoStatusService::NotRunning =>
            {
                view.first_run = Some(FirstRun::NotStarted(first_run(view.platform)));
                StatusView::NoStatus(view)
            }
            other => other,
        };
        match view {
            StatusView::Running(mut view) => {
                view.hold = hold;
                view.data_folder = folder.clone();
                view.login_item_written = written;
                StatusView::Running(view)
            }
            StatusView::NoStatus(mut view) => {
                view.hold = hold;
                view.data_folder = folder;
                view.login_item_written = written;
                if view.service == NoStatusService::NotRunning {
                    view.last_exit = last_exit;
                }
                StatusView::NoStatus(view)
            }
        }
    }

    /// Names the data folder, as resolved, that every view gives the page for the questions of
    /// the offline delete (contracts.md C10.5).
    #[cfg(target_os = "macos")]
    pub fn set_folder(&self, folder: String) {
        self.state().folder = Some(folder);
    }

    /// Ends the hold: called only for a Resume of a person's press that reached its end.
    pub fn release(&self) {
        self.state().held = false;
        self.changed.notify_all();
    }

    /// A person's press asks the supervisor for one child where the hold reads Start, that is
    /// where it stands and no further child starts, and where no child of the app lives between
    /// the end of one child and the start of the next, which the supervisor takes at that start;
    /// the read and the ask stand under one lock, so that no ask is left standing once the
    /// supervisor has taken one. Answers whether it asked.
    pub fn ask_start(&self) -> bool {
        let mut state = self.state();
        let start = (state.held && state.no_more_child) || (state.between && !state.child_lives);
        if start {
            state.start_asked = true;
            drop(state);
            self.changed.notify_all();
        }
        start
    }

    /// The click on Start, the consent, asks the supervisor for one child where it awaits the
    /// consent, no ask stands and no child lives; the read and the ask stand under one lock, so
    /// that a second click in flight asks for none. Answers whether it asked.
    pub fn ask_consent(&self) -> bool {
        let mut state = self.state();
        let ask = state.awaits_consent && !state.start_asked && !state.child_lives;
        if ask {
            state.start_asked = true;
            drop(state);
            self.changed.notify_all();
        }
        ask
    }

    /// Begins a consent click: none while the hold stands, whose press is then the hold's and
    /// never a new consent, while the data directory reads its own cause, and while another
    /// consent click is in flight.
    pub fn begin_consent(&self) -> Option<Consenting<'_>> {
        let mut state = self.state();
        if state.held || state.data_dir_unusable || state.consenting {
            return None;
        }
        state.consenting = true;
        Some(Consenting(self))
    }

    /// What the consent click read of its write of the login item: whether the plist stands
    /// written; every view says it from then on.
    pub fn login_item_written(&self, written: bool) {
        self.state().login_item_written = Some(written);
    }

    /// The number of the app's child that lives at this moment, counted from the first child of
    /// this life of the app; none while no child lives.
    pub fn life(&self) -> Option<u64> {
        let state = self.state();
        state.child_lives.then_some(state.lives)
    }

    /// Waits until `until` for the end of the child of number `life`: true where it ended with
    /// status 5, false where it ended otherwise, where a later child ended first, and at `until`.
    pub fn ended_with_5(&self, life: u64, until: Instant) -> bool {
        let mut state = self.state();
        loop {
            match state.ended {
                Some((of, status)) if of == life => {
                    return status == Some(halvo_control::exit::SOURCE_NOT_STOPPED)
                }
                Some((of, _)) if of > life => return false,
                _ => {}
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Waits until `until` for a child after the child of number `life`: true once one lives,
    /// false at `until` and once no further child starts.
    pub fn wait_for_a_life_after(&self, life: u64, until: Instant) -> bool {
        let mut state = self.state();
        loop {
            if state.child_lives && state.lives > life {
                return true;
            }
            if state.no_more_child {
                return false;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Waits until `until` for the child that a Start asked for: true once the supervisor took
    /// the ask and a child lives.
    pub fn wait_for_the_asked_child(&self, until: Instant) -> bool {
        let mut state = self.state();
        loop {
            if !state.start_asked && state.child_lives {
                return true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// After the third end with 5 in a row no further child starts until a Start.
    #[cfg(any(target_os = "macos", test))]
    pub fn no_more_child(&self) {
        self.state().no_more_child = true;
        self.changed.notify_all();
    }

    /// Takes the ask of a Start or of the consent, if one waits: the press then asked for the
    /// next child.
    #[cfg(any(target_os = "macos", test))]
    pub fn take_start(&self) -> bool {
        let mut state = self.state();
        let asked = state.start_asked;
        if asked {
            state.start_asked = false;
            state.no_more_child = false;
            state.between = false;
            state.awaits_consent = false;
        }
        asked
    }

    /// The supervisor starts a child: an ask of a Start between two children is taken by it.
    #[cfg(any(target_os = "macos", test))]
    pub fn take_the_ask(&self) {
        let mut state = self.state();
        state.start_asked = false;
        state.between = false;
        drop(state);
        self.changed.notify_all();
    }

    /// The supervisor's spawner reads the data directory not provisioned: no child starts before
    /// the person's click on Start, the consent (contracts.md C10.7).
    #[cfg(any(target_os = "macos", test))]
    pub fn awaits_consent(&self) {
        self.state().awaits_consent = true;
        self.changed.notify_all();
    }

    /// The supervisor's spawner reads the data directory's own cause (ruling ST-36 (e)): no child
    /// starts and no consent is offered.
    #[cfg(any(target_os = "macos", test))]
    pub fn data_dir_unusable(&self) {
        self.state().data_dir_unusable = true;
        self.changed.notify_all();
    }

    /// The app leaves its own child that reads another daemon_version after the one restart of
    /// the launch: views read another_version until a child starts again.
    #[cfg(any(target_os = "macos", test))]
    pub fn another_version_left(&self) {
        self.state().another_version = true;
    }

    /// A child of the app started.
    #[cfg(any(target_os = "macos", test))]
    pub fn child_started(&self) {
        let mut state = self.state();
        state.lives += 1;
        state.child_lives = true;
        state.last_exit = None;
        state.another_version = false;
        drop(state);
        self.changed.notify_all();
    }

    /// The child of the moment ended, with its exit status where it has one; an end with 5
    /// sets the hold.
    #[cfg(any(target_os = "macos", test))]
    pub fn child_ended(&self, status: Option<i32>) {
        let mut state = self.state();
        if state.child_lives {
            state.ended = Some((state.lives, status));
        }
        state.child_lives = false;
        state.between = true;
        state.last_exit = status;
        if status == Some(halvo_control::exit::SOURCE_NOT_STOPPED) {
            state.held = true;
        }
        drop(state);
        self.changed.notify_all();
    }
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
    hold: Option<HoldView>,
    login_item_loaded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    login_item_written: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    data_folder: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_file: Option<SettingsUnread>,
}

impl Serialize for StatusView {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            StatusView::Running(view) => WireView {
                service: Service::Running,
                last_exit: None,
                hold: view.hold,
                login_item_loaded: view.login_item_loaded,
                login_item_written: view.login_item_written,
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
                data_folder: view.data_folder.as_deref(),
                state_file: view.state_file,
            },
            StatusView::NoStatus(view) => WireView {
                service: view.word(),
                last_exit: view.last_exit,
                hold: view.hold,
                login_item_loaded: view.login_item_loaded,
                login_item_written: view.login_item_written,
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
                texts: match &view.first_run {
                    Some(FirstRun::NotStarted(texts)) => Some(texts),
                    Some(FirstRun::DataDirUnusable) | None => None,
                },
                ext: None,
                data_folder: view.data_folder.as_deref(),
                state_file: None,
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
            hold: None,
            login_item_loaded: None,
            data_folder: None,
            login_item_written: None,
            first_run: None,
        })
    }

    /// Whether this view reads that no capture service runs: the one state in which the app
    /// runs the offline delete (contracts.md C10.5).
    pub fn reads_not_running(&self) -> bool {
        matches!(self, StatusView::NoStatus(view) if view.service == NoStatusService::NotRunning)
    }

    pub fn without_status(service: NoStatusService) -> Self {
        StatusView::no_status(service, APP_PLATFORM)
    }

    /// The app's hold as this view says it.
    pub fn hold(&self) -> Option<HoldView> {
        match self {
            StatusView::Running(view) => view.hold,
            StatusView::NoStatus(view) => view.hold,
        }
    }

    /// This view with what the app read of its login item at its start: whether launchd holds
    /// it, none where nothing was read.
    pub fn with_login_item_loaded(mut self, loaded: Option<bool>) -> Self {
        match &mut self {
            StatusView::Running(view) => view.login_item_loaded = loaded,
            StatusView::NoStatus(view) => view.login_item_loaded = loaded,
        }
        self
    }

    #[cfg(test)]
    pub fn service(&self) -> Service {
        match self {
            StatusView::Running(_) => Service::Running,
            StatusView::NoStatus(view) => view.word(),
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
                    name: match line.name {
                        SourceName::Input if !typing_shown(status) => SourceName::Pointer,
                        name => name,
                    },
                    state: line.state.name(),
                    reason: line.state.reason(),
                    error: line.state.error().map(ToString::to_string),
                })
                .collect(),
            lost_grants: onboarding_shown(status).lost_grants,
            password_guard: f.password_guard,
            screen_filter_excludes_app: f.screen_filter_excludes_app,
            texts: texts_shown(status),
            ext: f.ext.clone(),
            hold: None,
            login_item_loaded: None,
            data_folder: None,
            state_file: match f.state_file {
                StateFile::Corrupt => Some(SettingsUnread::Corrupt),
                StateFile::NewerVersion => Some(SettingsUnread::NewerVersion),
                StateFile::Ok | StateFile::WriteFailed => None,
            },
            login_item_written: None,
        }))
    }
}

/// The one switch of what a view says about typing on a Mac: whether the status's typing line
/// reads on. A Mac status that reads it off or holds no such line shows none of what typing
/// would record or need: the row of keys, the sentences on passwords and on a key pressed early,
/// the card of Input Monitoring, and the input source's name for typing. A Windows status is
/// shown as it is.
pub fn typing_shown(status: &DaemonStatus) -> bool {
    let f = status.fields();
    f.platform == Platform::Windows || f.typing == Some(true)
}

/// The onboarding rows of `status` that a view shows: a row with its steps and its lost grant
/// stays only for a permission that the app can use, Input Monitoring by [`typing_shown`] and
/// Screen and System Audio Recording while the switch screen_activity reads on.
pub fn onboarding_shown(status: &DaemonStatus) -> OnboardingState {
    let f = status.fields();
    let screen = f
        .switches
        .iter()
        .any(|line| line.name == SwitchName::ScreenActivity && line.value == OnOff::On);
    let shown = |permission: Permission| match permission {
        Permission::InputMonitoring => typing_shown(status),
        Permission::ScreenAndSystemAudioRecording => screen,
        Permission::Accessibility => true,
    };
    let mut onboarding = f.onboarding.clone();
    onboarding.permissions.retain(|row| shown(row.permission));
    onboarding
        .lost_grants
        .retain(|&permission| shown(permission));
    onboarding.walkback.retain(|step| shown(step.permission));
    onboarding.all_granted = onboarding
        .permissions
        .iter()
        .all(|row| row.grant == GrantState::Granted);
    onboarding
}

/// The sentences of `status` that a view shows: by [`typing_shown`], without the row of keys,
/// the sentence on passwords (left empty) and the sentence on a key pressed early.
pub fn texts_shown(status: &DaemonStatus) -> Texts {
    let mut shown = texts(status);
    if !typing_shown(status) {
        shown
            .recorded
            .retain(|row| row.switch != Some(SwitchName::KeyClass));
        shown.passwords = "";
        shown.typed_ahead = None;
    }
    shown
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum ActionError {
    Busy,
    Deadline {
        seconds: u64,
    },
    InvalidArgument,
    CapOutOfRange {
        min_bytes: u64,
        max_bytes: u64,
    },
    BadStoreId,
    UpdateTheApp,
    StoreUnavailable,
    StateFile,
    Internal,
    PeerRejected,
    BrokenReply,
    ChannelFailed,
    PaneNotOpened,
    NotAnswering {
        service: NoStatusService,
    },
    /// The busy of a life that is ending: the service took no request, and a delete removed
    /// nothing.
    ServiceEnding,
    /// An already_deleted that answers no request of a person's delete: that press removed
    /// nothing.
    NothingRemoved,
    /// The service ended with status 5 during a person's delete and no later life answered
    /// within the wait, so the flow could not ask again.
    ServiceNotBack,
    /// The service does not serve a request of this kind; the page says it in its own words for
    /// the request, never the message of the service's reply.
    UnsupportedOp,
    /// The offline delete ended with 75: another process holds the data directory or its store,
    /// and it deleted nothing (contracts.md C3.3).
    DeleteHeld {
        looked_in: String,
    },
    /// The program of the offline delete did not start, or the app has none to start: nothing
    /// was deleted.
    DeleteNotStarted,
    /// The offline delete ended in a way that tells nothing of what it removed: another exit
    /// status, a signal, lines that do not read, or no end within its wait.
    DeleteNotKnown {
        looked_in: Option<String>,
    },
}

impl ActionError {
    pub fn cap_out_of_range() -> Self {
        ActionError::CapOutOfRange {
            min_bytes: StorageCap::MIN.bytes(),
            max_bytes: StorageCap::MAX.bytes(),
        }
    }
}

/// The error of a delete or of a cap Save as the page reads it: the error's own fields, and
/// whether the outcome of the command is not known (rulings AP-17 G6 and AP-19 H3). A cap Save
/// takes that from `removes_nothing`. A delete takes it from `a_delete_removed_nothing`, which adds
/// an already_deleted that the client does not take for the delete, and it stays not known where
/// the first request read no reply and the second answer removed nothing by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutcomeError {
    #[serde(flatten)]
    pub error: ActionError,
    pub outcome_not_known: bool,
}

/// Whether a delete or a cap Save that ended in `error` changed nothing: the error came before the request was
/// written, or the service gave it before anything is removed (contracts.md C2.1 line 52,
/// unsupported_version read before the shape; line 61, unsupported_op; C2.6 lines 170 and 174,
/// busy before the store's state and the ids are read, the busy of a life that is ending among
/// them; C15 item 3, peer_rejected by the client before it sends). Every other error leaves the
/// outcome not known, an already_deleted that answers no request of the call among them; a
/// delete reads that one by `a_delete_removed_nothing`.
#[deny(clippy::wildcard_enum_match_arm)]
pub fn removes_nothing(error: &ClientError) -> bool {
    match error {
        ClientError::Unsupported
        | ClientError::Unreachable { .. }
        | ClientError::DataDirInUse { .. }
        | ClientError::PeerRejected(_)
        | ClientError::ServiceEnding(_) => true,
        ClientError::Daemon(reply) => match reply.code() {
            ErrorCode::Busy | ErrorCode::UnsupportedVersion | ErrorCode::UnsupportedOp => true,
            ErrorCode::InvalidArgument
            | ErrorCode::StoreUnavailable
            | ErrorCode::StateFile
            | ErrorCode::Internal
            | ErrorCode::PeerRejected => false,
        },
        ClientError::Deadline { .. }
        | ClientError::Protocol(_)
        | ClientError::ReplyOfAnotherVersion { .. }
        | ClientError::ValueUnknownToV1(_)
        | ClientError::StateDoesNotHoldTogether(_)
        | ClientError::UnexpectedReply { .. }
        | ClientError::AlreadyDeletedToThisRequest(_)
        | ClientError::Io(_) => false,
    }
}

/// Whether a person's delete that ended in `error` removed nothing: the errors of
/// `removes_nothing`, and an already_deleted that the client does not take for this delete
/// (contracts.md C2.6 line 173).
pub fn a_delete_removed_nothing(error: &ClientError) -> bool {
    removes_nothing(error) || matches!(error, ClientError::AlreadyDeletedToThisRequest(_))
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
        ClientError::ServiceEnding(_) => (
            NoStatusService::StatusRefused,
            Some(ActionError::ServiceEnding),
        ),
        ClientError::AlreadyDeletedToThisRequest(_) => (
            NoStatusService::StatusRefused,
            Some(ActionError::NotAnswering {
                service: NoStatusService::StatusRefused,
            }),
        ),
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
            match reply.code() {
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
            Some(match reply.code() {
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
        /// How many deletes in a row, this one the last, did not finish (ruling DM-21): only
        /// where this one did not.
        #[serde(skip_serializing_if = "Option::is_none")]
        unfinished_in_a_row: Option<u32>,
        /// The run that gave this answer ended as one that is not whole, whatever its fields
        /// read: the offline delete's end with 1 (contracts.md C3.3).
        #[serde(skip)]
        not_whole: bool,
    },
    AlreadyDeleted {
        current_store_id: StoreId,
    },
    DeleteRefused,
    /// The offline delete found nothing of a store in the directory that it looked in.
    NoStore {
        looked_in: String,
    },
}

/// What the app keeps of the last delete, in memory alone: none, a delete in flight, or the
/// answer with the time at which it came, in milliseconds since the Unix epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LastDelete {
    None,
    InFlight,
    Answered { at_ms: u64, answer: DeleteView },
}

pub fn left_nothing(report: &DeleteFields) -> bool {
    report.root_destroyed == RootDestroyed::Yes
        && report.not_followed == 0
        && report.files_left == 0
        && !report.database_left
        && report.entries_left == 0
        && report.unknown_entries == 0
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
    fn deleted(
        f: &DeleteFields,
        new_store_id: Option<StoreId>,
        roots_dir: Option<&std::path::Path>,
        looked_in: Option<String>,
        not_whole: bool,
    ) -> Self {
        DeleteView::Deleted {
            segments_shredded: f.segments_shredded,
            keys_destroyed: f.keys_destroyed,
            root_destroyed: f.root_destroyed,
            files_left: f.files_left,
            database_left: f.database_left,
            not_followed: f.not_followed,
            entries_left: f.entries_left,
            unknown_entries: f.unknown_entries,
            new_store_id,
            cleanup_command: match (f.root_destroyed, f.root_id, roots_dir) {
                (RootDestroyed::No, Some(id), Some(dir)) => Some(cleanup_command(dir, id)),
                _ => None,
            },
            looked_in,
            nothing_left: !not_whole && left_nothing(f),
            unfinished_in_a_row: None,
            not_whole,
        }
    }

    /// Counts this answer into the deletes in a row that did not finish, at either door: a
    /// deleted answer that left a key, a file, a database or an entry of Halvo's own, or whose run
    /// ended as not whole, adds one and carries the count; one that left none, an already_deleted
    /// and a no_store end the row; any other answer leaves the count as it stands.
    pub fn count_unfinished(&mut self, in_a_row: &std::sync::atomic::AtomicU32) {
        use std::sync::atomic::Ordering;
        let finished = self.finished();
        match self {
            DeleteView::Deleted {
                unfinished_in_a_row,
                ..
            } => {
                if finished {
                    in_a_row.store(0, Ordering::SeqCst);
                } else {
                    let count = in_a_row.load(Ordering::SeqCst).saturating_add(1);
                    in_a_row.store(count, Ordering::SeqCst);
                    *unfinished_in_a_row = Some(count);
                }
            }
            DeleteView::AlreadyDeleted { .. } | DeleteView::NoStore { .. } => {
                in_a_row.store(0, Ordering::SeqCst);
            }
            DeleteView::DeleteRefused => {}
        }
    }

    /// Whether this is a deleted answer that finished: its run ended as whole, and it left no
    /// key, no file, no database and no entry of Halvo's own; an entry that Halvo does not know
    /// does not count.
    pub fn finished(&self) -> bool {
        match self {
            DeleteView::Deleted {
                root_destroyed,
                not_followed,
                files_left,
                database_left,
                entries_left,
                not_whole,
                ..
            } => {
                !*not_whole
                    && *root_destroyed == RootDestroyed::Yes
                    && *not_followed == 0
                    && *files_left == 0
                    && !*database_left
                    && *entries_left == 0
            }
            DeleteView::AlreadyDeleted { .. }
            | DeleteView::NoStore { .. }
            | DeleteView::DeleteRefused => false,
        }
    }

    pub fn of(outcome: &DeleteOutcome, roots_dir: Option<&std::path::Path>) -> Self {
        match outcome {
            DeleteOutcome::Deleted(deleted) => DeleteView::deleted(
                deleted.report.fields(),
                deleted.store,
                roots_dir,
                None,
                false,
            ),
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
        Err(ClientError::AlreadyDeletedToThisRequest(_)) => Err(ActionError::NothingRemoved),
        Err(error) => {
            let reading = read_error(&error);
            Err(reading.command_error.unwrap_or(ActionError::NotAnswering {
                service: reading.service,
            }))
        }
    }
}

/// How the program of the offline delete ended (contracts.md C3.3), with what it printed on
/// its standard output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfflineEnd {
    Exited { code: i32, out: String },
    Signal { out: String },
    NotStarted,
    NoEnd,
}

/// What the offline delete printed after its `data_dir=` line.
enum Printed {
    Nothing,
    NoStore,
    Deleted(DeleteFields),
}

/// The answer of the offline delete by how its program ended and what it printed (contracts.md
/// C3.3 and C10.5): `looked_in` is its `data_dir=`. An end with 0 and `no_store`, and an end with
/// 0 or 1 and the `deleted` line, are its outcome, and an end with 1 is never a whole delete,
/// whatever the line reads; an end with 75 deleted nothing; any other end tells nothing of what
/// was removed, an end with 1 that printed no `deleted` line among them, which halvo-capture
/// gives both where it could not use the directory and where its walk removed entries before
/// the store's own delete failed with no report.
pub fn offline_view(
    end: &OfflineEnd,
    roots_dir: Option<&std::path::Path>,
) -> Result<DeleteView, OutcomeError> {
    let removed_nothing = |error| {
        Err(OutcomeError {
            error,
            outcome_not_known: false,
        })
    };
    let not_known = |looked_in| {
        Err(OutcomeError {
            error: ActionError::DeleteNotKnown { looked_in },
            outcome_not_known: true,
        })
    };
    let (code, out) = match end {
        OfflineEnd::NotStarted => return removed_nothing(ActionError::DeleteNotStarted),
        OfflineEnd::NoEnd => return not_known(None),
        OfflineEnd::Signal { out } => return not_known(printed(out).map(|(dir, _)| dir)),
        OfflineEnd::Exited { code, out } => (*code, out),
    };
    let Some((looked_in, printed)) = printed(out) else {
        return not_known(None);
    };
    match (code, printed) {
        (exit::CLEAN, Printed::NoStore) => Ok(DeleteView::NoStore { looked_in }),
        (exit::CLEAN | exit::ERROR, Printed::Deleted(fields)) => Ok(DeleteView::deleted(
            &fields,
            None,
            roots_dir,
            Some(looked_in),
            code == exit::ERROR,
        )),
        (exit::BUSY, Printed::Nothing) => removed_nothing(ActionError::DeleteHeld { looked_in }),
        _ => not_known(Some(looked_in)),
    }
}

fn printed(out: &str) -> Option<(String, Printed)> {
    let mut lines = out
        .lines()
        .filter(|line| !line.starts_with("data_dir_resolved="));
    let looked_in = lines.next()?.strip_prefix("data_dir=")?.to_owned();
    let printed = match (lines.next(), lines.next()) {
        (None, _) => Printed::Nothing,
        (Some("no_store"), None) => Printed::NoStore,
        (Some(line), None) => Printed::Deleted(deleted_fields(line)?),
        (Some(_), Some(_)) => return None,
    };
    Some((looked_in, printed))
}

/// The fields of the offline form's `deleted` line: the keys of contracts.md C2.6 without
/// `store_id=`, each once, in a report that holds together.
fn deleted_fields(line: &str) -> Option<DeleteFields> {
    let mut words = line.split(' ');
    if words.next()? != "deleted" {
        return None;
    }
    let mut values = std::collections::BTreeMap::new();
    for word in words {
        let (key, value) = word.split_once('=')?;
        if values.insert(key, value).is_some() {
            return None;
        }
    }
    if values.len() != 11 {
        return None;
    }
    let text = |key: &str| values.get(key).copied();
    let count = |key: &str| text(key)?.parse::<u64>().ok();
    let known = |key: &str| match text(key)? {
        "unknown" => Some(None),
        number => number.parse::<u64>().ok().map(Some),
    };
    let yes = |key: &str| match text(key)? {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    };
    let fields = DeleteFields {
        segments_shredded: known("segments_shredded")?,
        keys_destroyed: known("keys_destroyed")?,
        root_destroyed: text("root_destroyed")?.parse().ok()?,
        root_id: match text("root_id")? {
            "-" => None,
            id => Some(id.parse().ok()?),
        },
        files_left: count("files_left")?,
        database_left: yes("database_left")?,
        not_followed: count("not_followed")?,
        entries_removed: count("entries_removed")?,
        entries_left: count("entries_left")?,
        unknown_entries: count("unknown_entries")?,
        intent: text("intent")?.parse().ok()?,
    };
    DeleteReport::new(fields).ok().map(|_| fields)
}

/// A step of Remove Halvo (contracts.md C10.7).
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Account,
    DeleteAll,
    Keychain,
    LoginItem,
    AppFiles,
    DataDir,
}

/// The steps of Remove Halvo in their order; a failed step ends the list.
#[cfg(target_os = "macos")]
pub const STEPS: [Step; 6] = [
    Step::Account,
    Step::DeleteAll,
    Step::Keychain,
    Step::LoginItem,
    Step::AppFiles,
    Step::DataDir,
];

/// How a step ended.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Done,
    Skipped,
    Failed,
}

/// What a step tells beside its outcome: the folder that it names, a count, and the cause of a
/// skip or a failure.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Detail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// One step of the answer.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepResult {
    pub step: Step,
    pub result: Outcome,
    pub detail: Detail,
}

/// The answer of `remove_halvo`: the steps that ran, in their order, up to a failed one; none
/// where the list did not start.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Removal {
    pub steps: Vec<StepResult>,
}

#[cfg(target_os = "macos")]
impl Removal {
    /// Whether the list ran to its end: every step, and none failed.
    pub fn whole(&self) -> bool {
        self.steps.len() == STEPS.len()
            && self.steps.iter().all(|step| step.result != Outcome::Failed)
    }

    /// The exit of the app after this answer: 0 after a whole list, none after one that
    /// stopped.
    pub fn exit(&self) -> Option<i32> {
        self.whole().then(|| crate::launch::Exit::Quit.code())
    }
}
