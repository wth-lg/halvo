use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use halvo_control::client::{ClientError, DeleteOutcome};
use halvo_control::model::{CaptureState, StorageCap, StoreId};
use halvo_control::onboarding::{OnboardingState, Permission, RequestOutcome};
use halvo_control::paths::DataDir;
use halvo_control::proto::{
    reaches_the_end, Capture as CaptureValue, CaptureChange, DaemonStatus, ErrorCode, OnOff,
    Platform, SourceName, SourceState, StateFile, SwitchName,
};
use halvo_control::service::DATA_DIR_ARG;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Manager, Runtime, State};

use super::agents::RunsHold;
use super::control::{control_of, Control, Made};
use super::names::{Names, CAPTURE};
use super::{tell_before_delete_all, Registration};
use crate::poller::{self, Views};
#[cfg(target_os = "macos")]
use crate::view_state::Removal;
use crate::view_state::{
    a_delete_removed_nothing, command_view, delete_view, inventory_view, offline_view,
    onboarding_shown, read_error, removes_nothing, ActionError, CapSetView, CaptureSet, DeleteView,
    Hold, InventoryView, LastDelete, OfflineEnd, OutcomeError, StatusView,
};

/// What the consent click calls first to write the login item through login_item.rs, the plist
/// on macOS and the Run value on Windows: whether it stands written, false where the app's own
/// path makes none or the write failed.
pub type LoginItemWriter = Arc<dyn Fn() -> bool + Send + Sync>;

pub struct AppState {
    pub control: Arc<dyn Control>,
    pub views: Arc<Views>,
    pub data_dir: Option<DataDir>,
    pub open_pane: fn(Permission) -> Result<(), String>,
    pub tray_installed: AtomicBool,
    pub last_delete: Mutex<LastDelete>,
    /// The login item that the consent click writes, the plist on macOS and the Run value on
    /// Windows: none with --data-dir and on other systems.
    pub login_item: Option<LoginItemWriter>,
    /// What the offline delete runs, set once the app has started the supervisor of its child;
    /// main holds the same cell from before the state moves into the app, so that it sets it
    /// with no read of the app's managed state.
    pub offline: Arc<OnceLock<Offline>>,
    /// The answers delete_refused of the service in a row, which open the offline delete's
    /// second door at two (contracts.md C10.5).
    pub refused_in_a_row: AtomicU32,
    /// The deletes in a row, at either door, that did not finish (ruling DM-21).
    pub unfinished_in_a_row: AtomicU32,
    /// The app's hold on the agents' runs around a delete of everything.
    pub runs: Arc<RunsHold>,
    offline_running: Arc<AtomicBool>,
    /// The folder of the crash reports of the web view of Halvo's own window on Windows, which
    /// the one function `app::crash_report_folder` names; set once at the app's start before the
    /// app is built, never on a Mac and never in a test app.
    pub crash_reports: Arc<OnceLock<PathBuf>>,
    /// What Remove Halvo takes from main, set once the app has started the supervisor of its
    /// child, through the cell that main holds from before the state moves into the app; never
    /// set in a test app, whose remove_halvo so runs no step.
    #[cfg(target_os = "macos")]
    pub removal: Arc<OnceLock<RemoveHalvo>>,
}

impl AppState {
    pub fn keep_last_delete(&self, kept: LastDelete) {
        *self.last_delete.lock().unwrap_or_else(|e| e.into_inner()) = kept;
    }

    fn drop_a_kept_delete_in_flight(&self) {
        let mut kept = self.last_delete.lock().unwrap_or_else(|e| e.into_inner());
        if *kept == LastDelete::InFlight {
            *kept = LastDelete::None;
        }
    }

    pub fn new<E>(
        resolve: impl FnOnce() -> Result<DataDir, E>,
        open_pane: fn(Permission) -> Result<(), String>,
    ) -> AppState {
        let (control, data_dir) = control_of(resolve);
        AppState::with_control(control, data_dir, open_pane)
    }

    pub fn with_control(
        control: Arc<dyn Control>,
        data_dir: Option<DataDir>,
        open_pane: fn(Permission) -> Result<(), String>,
    ) -> AppState {
        AppState {
            control,
            views: Arc::default(),
            data_dir,
            open_pane,
            tray_installed: AtomicBool::new(false),
            last_delete: Mutex::new(LastDelete::None),
            login_item: None,
            offline: Arc::default(),
            refused_in_a_row: AtomicU32::new(0),
            unfinished_in_a_row: AtomicU32::new(0),
            runs: Arc::default(),
            offline_running: Arc::default(),
            crash_reports: Arc::default(),
            #[cfg(target_os = "macos")]
            removal: Arc::default(),
        }
    }
}

pub struct Capture;

impl Registration for Capture {
    fn names(&self) -> &'static Names {
        &CAPTURE
    }
}

/// Answers an invoke of a command of `names::CAPTURE`; returns false for any other name.
pub fn handler<R: Runtime>(invoke: Invoke<R>) -> bool {
    let handle: fn(Invoke<R>) -> bool = tauri::generate_handler![
        capture_status,
        inventory,
        set_capture_state,
        delete_all,
        set_storage_cap,
        onboarding_state,
        request_permission,
        open_permission_pane,
        last_delete,
        acknowledge_lost_permission,
        start_capture,
        delete_offline,
        remove_halvo,
        reset_settings,
        set_typing,
    ];
    handle(invoke)
}

async fn draw_off_the_main_thread(views: &Arc<Views>) {
    let views = Arc::clone(views);
    let _ = off_the_main_thread(move || views.draw_now()).await;
}

async fn off_the_main_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ActionError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| ActionError::Internal)
}

#[tauri::command]
pub async fn capture_status(state: State<'_, AppState>) -> Result<StatusView, ActionError> {
    Ok(state.views.view())
}

#[tauri::command]
pub async fn inventory(state: State<'_, AppState>) -> Result<InventoryView, ActionError> {
    let made = state.control.places().make();
    let control = Arc::clone(&state.control);
    inventory_view(off_the_main_thread(move || control.inventory(made)).await?)
}

#[tauri::command]
pub async fn set_capture_state(
    state: State<'_, AppState>,
    capture: CaptureState,
) -> Result<CaptureSet, ActionError> {
    let made = state.control.places().make();
    let control = Arc::clone(&state.control);
    let views = Arc::clone(&state.views);
    let change = match capture {
        CaptureState::Paused => CaptureChange::Pause,
        CaptureState::Running => CaptureChange::Resume,
    };
    let (answer, call, next_number_at_answer) = off_the_main_thread(move || {
        let (answer, call) = press(control.as_ref(), &views, made, change, START_ANSWER_WAIT);
        (answer, call, control.places().next_number())
    })
    .await?;
    let set = capture_set(&state.views, call, next_number_at_answer, change, answer);
    draw_off_the_main_thread(&state.views).await;
    set
}

/// How long a press on Start goes on asking the child that it starts for a Status; a Status
/// asked just before the end keeps its own deadline of 2 s, so the wait can run to 12 s.
pub const START_ANSWER_WAIT: Duration = Duration::from_secs(10);

/// How long a press on Start waits between two Status requests to its child.
const START_STATUS_STEP: Duration = Duration::from_millis(250);

/// What a person's press of Pause or Resume sends, from the page's command and from the tray's
/// toggle: the request of `change` in the place `made`, and in place of a Resume a Start where
/// the hold reads that no further child starts, which waits for its child as
/// `start_and_resume` says. A Resume that reached its end with a screen that `screen_spent`
/// reads is followed by one renewal, `renew_the_screen`, whose Resume then answers the press. A
/// Pause press counts itself in the hold, so that no Resume behind a wait undoes it.
/// Answers its answer with the number of the call that it answers.
pub fn press(
    control: &dyn Control,
    views: &Views,
    made: Made,
    change: CaptureChange,
    within: Duration,
) -> (Result<DaemonStatus, ClientError>, u64) {
    let pauses = views.hold().pauses();
    if change == CaptureChange::Resume && views.hold().ask_start() {
        drop(made);
        return start_and_resume(control, views.hold(), within, pauses);
    }
    let call = made.number();
    let answer = match change {
        CaptureChange::Pause => {
            let _turn = views.hold().pause_pressed();
            control.pause(made)
        }
        CaptureChange::Resume => control.resume(made),
    };
    if change == CaptureChange::Resume {
        if let Ok(status) = &answer {
            if holds(change, status) {
                if let Some(renewed) = renew_where_spent(control, views.hold(), status, pauses) {
                    return renewed;
                }
            }
        }
    }
    (answer, call)
}

/// How long a renewal of the screen waits for the end of the app's child and for its next child
/// to answer a Status: the end within STOP_WAIT_MS and the 2 s in which the app ends a child that
/// it saw ending, then the wait of a press on Start.
pub const RENEWAL_WAIT: Duration = Duration::from_millis(
    halvo_control::STOP_WAIT_MS + 2_000 + START_ANSWER_WAIT.as_secs() * 1_000,
);

/// Whether `status` reads the screen of a Windows run spent: the run makes one screen session
/// (form B of contracts.md C14.3), so behind a pause, or a delete through the service, its
/// screen source reads stopped, or failed with the first session's error, while the switch
/// screen_activity reads on and capture runs. A Mac's stopped or failed screen is a permission
/// or a fault, which no new child cures, and reads no.
pub fn screen_spent(status: &DaemonStatus) -> bool {
    let f = status.fields();
    f.platform == Platform::Windows
        && matches!(f.capture, CaptureValue::Running | CaptureValue::Partial)
        && f.switches
            .iter()
            .any(|switch| switch.name == SwitchName::ScreenActivity && switch.value == OnOff::On)
        && f.sources.iter().any(|source| {
            source.name == SourceName::Screen
                && matches!(
                    source.state,
                    SourceState::Stopped { .. } | SourceState::Failed { .. }
                )
        })
}

/// A Status of `status` that reads the screen spent gets ONE renewal, `renew_the_screen`, for
/// the press or the delete whose start read `pauses` Pause presses, and its answer; one that does
/// not clears what the last renewal said, and gets none.
fn renew_where_spent(
    control: &dyn Control,
    hold: &Hold,
    status: &DaemonStatus,
    pauses: u64,
) -> Option<(Result<DaemonStatus, ClientError>, u64)> {
    if !screen_spent(status) {
        hold.screen_renewed(true);
        return None;
    }
    renew_the_screen(control, hold, RENEWAL_WAIT, pauses)
}

/// The one renewal of a spent screen: it asks the supervisor to end the app's child with the mark
/// of a wanted start (`Hold::ask_renewal`), waits until `within` for a later child that answers
/// a plain Status, and sends that child one Resume by `resume_unless_paused_since`, whose answer
/// it answers with the number of its call; it tells the hold whether that answer still reads the
/// screen spent. With no child of the app it runs nothing; with no later child in time it sends
/// no Resume, and the hold reads the screen not renewed.
pub fn renew_the_screen(
    control: &dyn Control,
    hold: &Hold,
    within: Duration,
    pauses: u64,
) -> Option<(Result<DaemonStatus, ClientError>, u64)> {
    let life = hold.ask_renewal()?;
    if !a_later_life_answers(control, hold, life, Instant::now() + within) {
        hold.screen_renewed(false);
        return None;
    }
    let (answer, call) = resume_unless_paused_since(control, hold, pauses);
    hold.screen_renewed(!answer.as_ref().is_ok_and(screen_spent));
    Some((answer, call))
}

/// The one Resume that a press sends behind a wait for a child, the Start's after the third end
/// with 5 and a renewal's of a spent screen: it goes out only where no Pause press came after the
/// press or the delete that it serves, whose start read `pauses` Pause presses, and under the
/// turn of the hold, so that a Pause in the same moment goes out after it; else a plain Status
/// goes out in its place. Answers the answer with the number of its call.
fn resume_unless_paused_since(
    control: &dyn Control,
    hold: &Hold,
    pauses: u64,
) -> (Result<DaemonStatus, ClientError>, u64) {
    let made = control.places().make();
    let call = made.number();
    let answer = match hold.no_pause_since(pauses) {
        Some(_turn) => control.resume(made),
        None => control.status(made, false),
    };
    (answer, call)
}

/// The press on Start after the third end with 5 in a row (contracts.md C14 item 4), once it
/// has asked the supervisor for exactly one child: asks for a plain Status until one is answered
/// or `within` has passed, the last Status keeping its own deadline of 2 s, and then sends the
/// Resume by `resume_unless_paused_since`, so that a Pause pressed after the press whose start
/// read `pauses` stands; with no answer it answers the deadline of `within`, sends no Resume,
/// and the hold stands. Where that child ends before a Status is answered, the asking goes on,
/// and the Resume goes to the next child of the backoff.
fn start_and_resume(
    control: &dyn Control,
    hold: &Hold,
    within: Duration,
    pauses: u64,
) -> (Result<DaemonStatus, ClientError>, u64) {
    let until = Instant::now() + within;
    let answered = hold.wait_for_the_asked_child(until) && status_until(control, until).is_some();
    if !answered {
        let call = control.places().make().number();
        return (
            Err(ClientError::Deadline {
                seconds: within.as_secs(),
            }),
            call,
        );
    }
    resume_unless_paused_since(control, hold, pauses)
}

fn status_until(control: &dyn Control, until: Instant) -> Option<DaemonStatus> {
    loop {
        if let Ok(status) = control.status(control.places().make(), false) {
            return Some(status);
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        std::thread::sleep(left.min(START_STATUS_STEP));
    }
}

/// The consent click, `start_capture` (contracts.md C10.7), in this order: it writes the login
/// item through `login_item`, where one is given; asks the supervisor for one child where it
/// awaits the consent, none where a child lives, a daemon answers or another click is in
/// flight; asks for a Status until one is answered or `within` has passed; reads the open
/// steps from it through `onboarding_shown`; and sends one Resume only when no step is open.
/// It answers the view: that of the Resume, or of the Status with an open step; with no Status
/// it answers the deadline of `within`, and no Resume goes out. Where the hold stands after an
/// end with 5 its press is the hold's, and the click, like one on a data directory that reads
/// its own cause or one while another is in flight, does nothing and answers the view of the
/// moment.
pub fn consent(
    control: &dyn Control,
    views: &Views,
    login_item: Option<&dyn Fn() -> bool>,
    within: Duration,
) -> Result<StatusView, ActionError> {
    let hold = views.hold();
    let Some(_click) = hold.begin_consent() else {
        return Ok(views.view());
    };
    if let Some(write) = login_item {
        hold.login_item_written(write());
    }
    let until = Instant::now() + within;
    let status = if views.status().is_none() && hold.ask_consent() {
        if hold.wait_for_the_asked_child(until) {
            status_until(control, until)
        } else {
            None
        }
    } else {
        control.status(control.places().make(), false).ok()
    };
    let Some(status) = status else {
        return Err(ActionError::Deadline {
            seconds: within.as_secs(),
        });
    };
    if !onboarding_shown(&status).walkback.is_empty() {
        return Ok(views.marked(StatusView::of(&status)));
    }
    let made = control.places().make();
    let call = made.number();
    let answer = control.resume(made);
    let next_number_at_answer = control.places().next_number();
    capture_set(
        views,
        call,
        next_number_at_answer,
        CaptureChange::Resume,
        answer,
    )
    .map(|set| set.view)
}

/// The eleventh command, the consent click: `consent` off the main thread, then a draw of the
/// views.
#[tauri::command]
pub async fn start_capture(state: State<'_, AppState>) -> Result<StatusView, ActionError> {
    let control = Arc::clone(&state.control);
    let views = Arc::clone(&state.views);
    let login_item = state.login_item.clone();
    let answer = off_the_main_thread(move || {
        let write = login_item
            .as_deref()
            .map(|write| write as &dyn Fn() -> bool);
        consent(control.as_ref(), &views, write, START_ANSWER_WAIT)
    })
    .await?;
    draw_off_the_main_thread(&state.views).await;
    answer
}

/// Sorts the answer of a press of Pause or Resume, for the page's command and for the tray's
/// toggle: it reached its end only where `holds` reads its status. A Pause happened only there:
/// no reply in time, a broken exchange, a child that ended before its reply, either busy and a
/// view with no status read as a Pause that did not happen, which the tray says until the next
/// such answer. The hold ends at a Resume that reached its end, and at no other place
/// (contracts.md C14 item 4).
pub fn sort_change_answer(
    views: &Views,
    change: CaptureChange,
    answer: &Result<DaemonStatus, ClientError>,
) -> bool {
    let reached = answer.as_ref().is_ok_and(|status| holds(change, status));
    views.set_pause_missed(change == CaptureChange::Pause && !reached);
    if change == CaptureChange::Resume && reached {
        views.hold().release();
    }
    reached
}

/// The answer of `set_capture_state` to the page: the view of the answer with the app's hold,
/// and whether the change reached its end; an error where the view gives none.
pub fn capture_set(
    views: &Views,
    call: u64,
    next_number_at_answer: u64,
    change: CaptureChange,
    answer: Result<DaemonStatus, ClientError>,
) -> Result<CaptureSet, ActionError> {
    let reached = sort_change_answer(views, change, &answer);
    poller::take_command_answer(views, call, next_number_at_answer, &answer);
    let view = views.marked(command_view(answer)?);
    Ok(CaptureSet { view, reached })
}

pub fn holds(change: CaptureChange, status: &DaemonStatus) -> bool {
    reaches_the_end(change, status) && status.fields().state_file == StateFile::Ok
}

/// Sets the kept delete to none when `delete_all` ends while it still reads in flight: a panic,
/// a dropped future or any early return (ruling AP-17 G5).
struct NotLeftInFlight<'a>(&'a AppState);

impl Drop for NotLeftInFlight<'_> {
    fn drop(&mut self) {
        self.0.drop_a_kept_delete_in_flight();
    }
}

/// How long the flow of a delete waits before its second ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteWaits {
    /// For the end of the life that the first ask reached, from the first answer.
    pub asked_life_end: Duration,
    /// For a later life to answer a plain Status, from the end of that life.
    pub next_life: Duration,
}

/// The waits of a person's delete: the end of the asked life within STOP_WAIT_MS and 2 s, the
/// time in which the app ends a child that it saw ending; a later life within the longest wait
/// of the backoff and the wait of a Start.
pub const DELETE_WAITS: DeleteWaits = DeleteWaits {
    asked_life_end: Duration::from_millis(halvo_control::STOP_WAIT_MS + 2_000),
    next_life: Duration::from_secs(70),
};

/// How the flow of a person's delete ended.
#[derive(Debug)]
pub enum DeleteEnd {
    /// The answer of the last delete request that the flow sent.
    Answered(Result<DeleteOutcome, ClientError>),
    /// The life that the first request reached ended with status 5 and no later life answered
    /// within the wait, so no second request was sent: the first request's error.
    NoLifeAnswered(ClientError),
    /// The first request read no reply and the second request's answer removed nothing by
    /// itself, so what the first request did stays not known: the first request's error.
    FirstNotKnown(ClientError),
}

/// What the flow of a person's delete ended with, the number of the call of the delete request
/// of its end, and the next number of the line at that end.
#[derive(Debug)]
pub struct DeleteAsked {
    pub end: DeleteEnd,
    pub call: u64,
    pub next_number_at_answer: u64,
}

/// The flow of a person's press of Delete everything, off the main thread: the delete request
/// armed with `armed`, marked in `views` as the app's own delete while it is in flight. Where it
/// read a plain busy or no reply, a store id was armed and the child that it reached ended with
/// status 5, the flow waits for a later child that answers a plain Status and asks once more with
/// the same id; it sends no Status that claims the indicator and no Resume, and never a third
/// request (contracts.md C2.1). Where the first request read no reply, a second answer that removes
/// nothing by itself leaves what the first request did not known.
pub fn ask_to_delete(
    control: &dyn Control,
    views: &Views,
    armed: Option<StoreId>,
    waits: DeleteWaits,
) -> DeleteAsked {
    let hold = views.hold();
    let life = hold.life();
    let (answer, call, next_number_at_answer) = ask_once(control, views, armed);
    let (life, first) = match (life, armed, answer) {
        (Some(life), Some(_), Err(first)) if asks_again(&first) => (life, first),
        (_, _, answer) => {
            return DeleteAsked {
                end: DeleteEnd::Answered(answer),
                call,
                next_number_at_answer,
            }
        }
    };
    if !hold.ended_with_5(life, Instant::now() + waits.asked_life_end) {
        return DeleteAsked {
            end: DeleteEnd::Answered(Err(first)),
            call,
            next_number_at_answer,
        };
    }
    if !a_later_life_answers(control, hold, life, Instant::now() + waits.next_life) {
        return DeleteAsked {
            end: DeleteEnd::NoLifeAnswered(first),
            call,
            next_number_at_answer: control.places().next_number(),
        };
    }
    let (answer, call, next_number_at_answer) = ask_once(control, views, armed);
    let end = match answer {
        Err(second)
            if matches!(first, ClientError::Deadline { .. })
                && a_delete_removed_nothing(&second) =>
        {
            DeleteEnd::FirstNotKnown(first)
        }
        answer => DeleteEnd::Answered(answer),
    };
    DeleteAsked {
        end,
        call,
        next_number_at_answer,
    }
}

fn asks_again(error: &ClientError) -> bool {
    match error {
        ClientError::Deadline { .. } => true,
        ClientError::Daemon(reply) => reply.code() == ErrorCode::Busy,
        _ => false,
    }
}

fn a_later_life_answers(control: &dyn Control, hold: &Hold, life: u64, until: Instant) -> bool {
    loop {
        if !hold.wait_for_a_life_after(life, until) {
            return false;
        }
        if control.status(control.places().make(), false).is_ok() {
            return true;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        std::thread::sleep(left.min(START_STATUS_STEP));
    }
}

fn ask_once(
    control: &dyn Control,
    views: &Views,
    armed: Option<StoreId>,
) -> (Result<DeleteOutcome, ClientError>, u64, u64) {
    let made = control.places().make();
    let call = made.number();
    let answer = {
        let _in_flight = views.own_delete();
        control.delete_all(made, armed)
    };
    (answer, call, control.places().next_number())
}

/// What the app does before a delete of everything goes out, at either door: every registered
/// module is told (`Registration::before_delete_all`), and then the agents' runs are held, the
/// runs at work stopped within the core's grace, until a reading of a poll made after the
/// delete's end is drawn whose view does not bar (`agents::RunsHold`).
#[derive(Clone)]
pub struct Before {
    pub modules: Vec<&'static dyn Registration>,
    pub runs: Arc<RunsHold>,
    pub core: Option<Arc<dyn halvo_coord_app::Core>>,
}

impl Before {
    /// `Before` of the app of `app`: its modules, its hold and its agents' core.
    pub fn of<R: Runtime>(app: &AppHandle<R>, state: &AppState) -> Before {
        Before {
            modules: super::table::<R>()
                .iter()
                .map(|module| module.registration)
                .collect(),
            runs: Arc::clone(&state.runs),
            core: app
                .try_state::<halvo_coord_app::Agents>()
                .map(|agents| Arc::clone(&agents.0)),
        }
    }

    /// Tells every module, waiting for them at most STOP_WAIT_MS, and holds the agents' runs;
    /// this delete ends in the hold when the answer is dropped, also where the hold's own call
    /// unwinds, and the hold stands on until a reading of a poll made after that end is drawn
    /// whose view does not bar.
    pub fn tell(&self) -> HeldForADelete {
        tell_before_delete_all(
            &self.modules,
            Duration::from_millis(halvo_control::STOP_WAIT_MS),
        );
        let held = HeldForADelete(self.clone());
        self.runs.delete_begins(self.core.as_deref());
        held
    }
}

/// One delete of everything in the hold of the agents' runs, until it is dropped; from then on
/// the hold owes a reading (`agents::RunsHold::delete_ends`).
pub struct HeldForADelete(Before);

impl HeldForADelete {
    /// Tells the hold, at this moment and before the drop, that the answer of this delete left
    /// the service maybe still deleting: it passed its deadline, the transport failed (behind the
    /// request, or before it, where the mark holds the hold longer than it must) or the client
    /// could not take what came back, or the service answered with anything but a finished
    /// delete (`agents::RunsHold::may_still_delete`).
    pub fn may_still_delete(&self) {
        self.0.runs.may_still_delete(Instant::now());
    }

    /// Tells the hold, at this moment and before the drop, that this delete ends at a door that
    /// ended the service's child or may end it (`agents::RunsHold::ended_with_no_service`).
    pub fn ends_with_no_service(&self) {
        self.0.runs.ended_with_no_service(Instant::now());
    }
}

impl Drop for HeldForADelete {
    fn drop(&mut self) {
        self.0.runs.delete_ends(self.0.core.as_deref());
    }
}

/// Whether the flow of a delete ended where the service may still delete, so that the guard
/// marks the hold late: its end is a deadline, a transport that failed (`Io`: behind the sent
/// request, or before it, where no request reached a service and the mark holds the hold up to
/// 120 s longer than it must), a reply that the client could not take (`Protocol`), or an answer
/// of the service that is no finished delete: a deleted answer whose run ended not whole, an
/// already deleted, a refusal, an error reply of any code, the busy of an ending life, or a
/// reply that the client read whole and cannot take. A finished delete marks nothing, and nor
/// does an end at which the client knows that no request reached a service: nothing listens, or
/// the client refused the peer before it sent.
#[deny(clippy::wildcard_enum_match_arm)]
fn marks_the_hold_late(end: &DeleteEnd) -> bool {
    let error = match end {
        DeleteEnd::Answered(Ok(DeleteOutcome::Deleted(deleted))) => return deleted.unfinished,
        DeleteEnd::Answered(Ok(DeleteOutcome::AlreadyDeleted(_) | DeleteOutcome::Refused)) => {
            return true
        }
        DeleteEnd::Answered(Err(error))
        | DeleteEnd::NoLifeAnswered(error)
        | DeleteEnd::FirstNotKnown(error) => error,
    };
    match error {
        ClientError::Deadline { .. }
        | ClientError::Daemon(_)
        | ClientError::ServiceEnding(_)
        | ClientError::ReplyOfAnotherVersion { .. }
        | ClientError::ValueUnknownToV1(_)
        | ClientError::StateDoesNotHoldTogether(_)
        | ClientError::UnexpectedReply { .. }
        | ClientError::AlreadyDeletedToThisRequest(_)
        | ClientError::Io(_)
        | ClientError::Protocol(_) => true,
        ClientError::Unreachable { .. }
        | ClientError::DataDirInUse { .. }
        | ClientError::Unsupported
        | ClientError::PeerRejected(_) => false,
    }
}

/// The flow of a person's press at the service's door: `Before::tell`, then `ask_to_delete`;
/// where the flow ended in a deadline, a failed transport, a reply that it could not take or an
/// answer of the service that is no finished delete (`marks_the_hold_late`), the guard tells the
/// hold so before it drops; behind a deleted answer, where the view of the moment reads a
/// Windows run, the guard tells the hold that this delete ends with no service before it drops,
/// and then one plain Status goes out, and where it reads the screen spent ONE renewal of the
/// screen, as at a Resume.
pub fn delete_everything(
    before: &Before,
    control: &dyn Control,
    views: &Views,
    armed: Option<StoreId>,
    waits: DeleteWaits,
) -> DeleteAsked {
    let pauses = views.hold().pauses();
    let (asked, may_renew) = {
        let held = before.tell();
        let asked = ask_to_delete(control, views, armed, waits);
        if marks_the_hold_late(&asked.end) {
            held.may_still_delete();
        }
        let may_renew = matches!(
            asked.end,
            DeleteEnd::Answered(Ok(DeleteOutcome::Deleted(_)))
        ) && views.view().platform() == Platform::Windows;
        if may_renew {
            held.ends_with_no_service();
        }
        (asked, may_renew)
    };
    if may_renew {
        if let Ok(status) = control.status(control.places().make(), false) {
            let _ = renew_where_spent(control, views.hold(), &status, pauses);
        }
    }
    asked
}

#[tauri::command]
pub async fn delete_all<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    store_id: Option<String>,
) -> Result<DeleteView, OutcomeError> {
    let armed = match store_id {
        None => None,
        Some(text) => Some(text.parse::<StoreId>().map_err(|_| OutcomeError {
            error: ActionError::BadStoreId,
            outcome_not_known: false,
        })?),
    };
    state.keep_last_delete(LastDelete::InFlight);
    let _in_flight = NotLeftInFlight(state.inner());
    let control = Arc::clone(&state.control);
    let views = Arc::clone(&state.views);
    let before = Before::of(&app, state.inner());
    let ended = off_the_main_thread(move || {
        delete_everything(&before, control.as_ref(), &views, armed, DELETE_WAITS)
    })
    .await;
    let Ok(DeleteAsked {
        end,
        call,
        next_number_at_answer,
    }) = ended
    else {
        let next = state.control.places().next_number();
        state.views.take_delete(next, next);
        draw_off_the_main_thread(&state.views).await;
        return Err(OutcomeError {
            error: ActionError::Internal,
            outcome_not_known: true,
        });
    };
    let roots = state.data_dir.as_ref().map(DataDir::roots_dir);
    let (mut view, nothing_removed) = match end {
        DeleteEnd::Answered(answer) => {
            let nothing_removed = answer.as_ref().err().is_some_and(a_delete_removed_nothing);
            (delete_view(answer, roots.as_deref()), nothing_removed)
        }
        DeleteEnd::NoLifeAnswered(first) => (
            Err(ActionError::ServiceNotBack),
            a_delete_removed_nothing(&first),
        ),
        DeleteEnd::FirstNotKnown(first) => (delete_view(Err(first), roots.as_deref()), false),
    };
    if let Ok(answer) = &mut view {
        answer.count_at_the_services_door(&state.unfinished_in_a_row, state.data_dir.as_ref());
        remove_the_crash_reports(
            answer,
            state.crash_reports.get().map(PathBuf::as_path),
            &|file| std::fs::remove_file(file),
        );
    }
    match &view {
        Ok(
            DeleteView::Deleted { .. }
            | DeleteView::AlreadyDeleted { .. }
            | DeleteView::NoStore { .. },
        ) => {
            state.views.take_delete(call, next_number_at_answer);
        }
        Ok(DeleteView::DeleteRefused) => state.views.take_refusal(next_number_at_answer),
        Err(_) if nothing_removed => state.views.take_refusal(next_number_at_answer),
        Err(_) => state.views.take_delete(call, next_number_at_answer),
    }
    draw_off_the_main_thread(&state.views).await;
    state.refused_in_a_row.store(
        match &view {
            Ok(DeleteView::DeleteRefused) => state.refused_in_a_row.load(Ordering::SeqCst) + 1,
            _ => 0,
        },
        Ordering::SeqCst,
    );
    state.keep_last_delete(match &view {
        Ok(answer) => LastDelete::Answered {
            at_ms: now_ms(),
            answer: answer.clone(),
        },
        Err(_) => LastDelete::None,
    });
    view.map_err(|error| OutcomeError {
        error,
        outcome_not_known: !nothing_removed,
    })
}

/// The supervisor of the app's daemon child, as a run that must not meet the child holds it.
pub trait WithoutTheChild: Send + Sync {
    /// Ends the app's child of the moment by its descriptor, starts no child while `run` runs,
    /// and after `run`, one that returns or one that unwinds, starts no child by itself, whatever
    /// the end of that child, until a person's press on Start behind the run or the next launch
    /// of the app.
    #[cfg_attr(
        not(target_os = "macos"),
        allow(dead_code, reason = "Remove Halvo, the door's caller, is macOS code")
    )]
    fn without_the_child(&self, run: &mut dyn FnMut());

    /// `without_the_child` for the offline delete, which wants a start after its run: the end
    /// of the child of the moment counts as a stop that the app asked for in order to restart,
    /// so that its end with 0 is followed by one start once `run` has ended.
    fn without_the_child_then_a_start(&self, run: &mut dyn FnMut());
}

/// How the app runs the program of the offline delete: the system's run in the app, a stand-in
/// in the tests.
pub type OfflineRun = Arc<dyn Fn(&Path, &[OsString]) -> OfflineEnd + Send + Sync>;

/// What the offline delete of contracts.md C10.5 runs: halvo-capture beside the app's own
/// executable, on the app's data directory, while the supervisor of the child holds every start.
#[derive(Clone)]
pub struct Offline {
    pub program: PathBuf,
    pub data_dir: DataDir,
    pub children: Option<Arc<dyn WithoutTheChild>>,
    pub run: OfflineRun,
}

/// How long the app waits for the offline delete: the client's own wait for a delete_all.
pub const OFFLINE_WAIT: Duration = halvo_control::client::Deadlines::V1.delete_all;

/// The arguments of the offline delete: `--delete-all --data-dir DIR`, the one form of a
/// release build (contracts.md C3.3).
pub fn offline_arguments(data_dir: &DataDir) -> Vec<OsString> {
    vec![
        OsString::from("--delete-all"),
        OsString::from(DATA_DIR_ARG),
        data_dir.path().as_os_str().to_owned(),
    ]
}

/// The offline delete of a person's press, off the main thread (contracts.md C10.5): only where
/// the view reads not_running, or after two answers delete_refused in a row, and otherwise
/// delete_refused with nothing run (contracts.md C10.5, a delete armed at one door is never
/// carried out at the other). It takes `Before::tell`, ends the app's child, runs the program
/// and maps its lines, and the supervisor then goes on by its rule; with no program to run
/// nothing is deleted.
pub fn ask_offline(
    views: &Views,
    refused_in_a_row: u32,
    offline: Option<&Offline>,
    before: &Before,
) -> Result<DeleteView, OutcomeError> {
    if !views.view().reads_not_running() && refused_in_a_row < 2 {
        return Ok(DeleteView::DeleteRefused);
    }
    let Some(offline) = offline else {
        return Err(OutcomeError {
            error: ActionError::DeleteNotStarted,
            outcome_not_known: false,
        });
    };
    run_offline(offline, before)
}

/// The run of the offline delete, the one entry of the command and of the launch form
/// `--delete-all`: `Before::tell`, the app's child ended, the program run on the data directory,
/// its lines mapped, and the supervisor then on by its rule; behind the run, and before its
/// guard drops, it tells the hold that this delete ends with no service
/// (`HeldForADelete::ends_with_no_service`).
pub fn run_offline(offline: &Offline, before: &Before) -> Result<DeleteView, OutcomeError> {
    let held = before.tell();
    let args = offline_arguments(&offline.data_dir);
    let mut end = OfflineEnd::NotStarted;
    let mut run = || end = (offline.run)(&offline.program, &args);
    match &offline.children {
        Some(children) => children.without_the_child_then_a_start(&mut run),
        None => run(),
    }
    held.ends_with_no_service();
    offline_view(&end, Some(&offline.data_dir.roots_dir()))
}

/// The mark of an offline delete that runs, until it is dropped.
struct OfflineRuns(Arc<AtomicBool>);

impl OfflineRuns {
    fn take(running: &Arc<AtomicBool>) -> Option<Self> {
        (!running.swap(true, Ordering::SeqCst)).then(|| OfflineRuns(Arc::clone(running)))
    }
}

impl Drop for OfflineRuns {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The fourteenth command: the offline delete of contracts.md C10.5, which takes no id. One runs
/// at a time: a second press while one runs answers busy and runs nothing.
#[tauri::command]
pub async fn delete_offline<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<DeleteView, OutcomeError> {
    let refused = |error| OutcomeError {
        error,
        outcome_not_known: false,
    };
    let Some(running) = OfflineRuns::take(&state.offline_running) else {
        return Err(refused(ActionError::Busy));
    };
    let refused_in_a_row = state.refused_in_a_row.load(Ordering::SeqCst);
    let offline = state.offline.get().cloned();
    let views = Arc::clone(&state.views);
    let before = Before::of(&app, state.inner());
    state.keep_last_delete(LastDelete::InFlight);
    let _in_flight = NotLeftInFlight(state.inner());
    let mut answer = off_the_main_thread(move || {
        let _running = running;
        ask_offline(&views, refused_in_a_row, offline.as_ref(), &before)
    })
    .await
    .unwrap_or(Err(OutcomeError {
        error: ActionError::Internal,
        outcome_not_known: true,
    }));
    if let Ok(view) = &mut answer {
        view.count_unfinished(&state.unfinished_in_a_row);
        remove_the_crash_reports(
            view,
            state.crash_reports.get().map(PathBuf::as_path),
            &|file| std::fs::remove_file(file),
        );
    }
    if answer != Ok(DeleteView::DeleteRefused) {
        let next = state.control.places().next_number();
        state.views.take_delete(next, next);
        state.refused_in_a_row.store(0, Ordering::SeqCst);
    }
    draw_off_the_main_thread(&state.views).await;
    state.keep_last_delete(match &answer {
        Ok(view) => LastDelete::Answered {
            at_ms: now_ms(),
            answer: view.clone(),
        },
        Err(_) => LastDelete::None,
    });
    answer
}

/// The fifteenth command, Remove Halvo (contracts.md C10.7), which takes no id: the list of
/// `RemoveHalvo` off the main thread, holding the mark of the offline delete, so that a
/// second call, an offline delete, and a call while a delete is in flight answer busy and run
/// nothing. With no wiring, as in a test app, it answers no step. After a whole list the app ends
/// with 0 once the page has had `LAST_WORDS_WAIT` to show the answer.
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn remove_halvo<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<Removal, ActionError> {
    let Some(remove) = state.removal.get().cloned() else {
        return Ok(Removal::default());
    };
    if *state.last_delete.lock().unwrap_or_else(|e| e.into_inner()) == LastDelete::InFlight {
        return Err(ActionError::Busy);
    }
    let Some(running) = OfflineRuns::take(&state.offline_running) else {
        return Err(ActionError::Busy);
    };
    let offline = state.offline.get().cloned();
    let data_dir = state.data_dir.clone();
    let before = Before::of(&app, state.inner());
    let removal = off_the_main_thread(move || {
        let _running = running;
        remove(data_dir.as_ref(), offline.as_ref(), &before)
    })
    .await?;
    draw_off_the_main_thread(&state.views).await;
    if let Some(code) = removal.exit() {
        exit_after_the_last_words(&app, code);
    }
    Ok(removal)
}

/// How long the app goes on after a whole removal, so that the page shows its last words.
#[cfg(target_os = "macos")]
pub const LAST_WORDS_WAIT: Duration = Duration::from_secs(10);

/// Ends the app with `code` once `LAST_WORDS_WAIT` has passed, on a thread of its own.
#[cfg(target_os = "macos")]
pub fn exit_after_the_last_words<R: Runtime>(app: &AppHandle<R>, code: i32) {
    let ending = app.clone();
    let spawned = std::thread::Builder::new()
        .name("halvo-removed".to_owned())
        .spawn(move || {
            std::thread::sleep(LAST_WORDS_WAIT);
            ending.exit(code);
        });
    if spawned.is_err() {
        app.exit(code);
    }
}

/// What Remove Halvo runs on the app's data folder and offline delete, after `Before`: the list
/// of the module remove, which main wraps, so that this module names no part of it.
#[cfg(target_os = "macos")]
pub type RemoveHalvo =
    Arc<dyn Fn(Option<&DataDir>, Option<&Offline>, &Before) -> Removal + Send + Sync>;

/// Remove Halvo off macOS: it runs nothing and answers unsupported_op.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn remove_halvo() -> Result<(), ActionError> {
    Err(ActionError::UnsupportedOp)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The thirteenth command: the last delete as the app keeps it, for the page that loads again
/// after a delete's answer.
#[tauri::command]
pub async fn last_delete(state: State<'_, AppState>) -> Result<LastDelete, ActionError> {
    Ok(state
        .last_delete
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone())
}

#[tauri::command]
pub async fn set_storage_cap(
    state: State<'_, AppState>,
    cap_bytes: u64,
) -> Result<CapSetView, OutcomeError> {
    let cap = StorageCap::from_bytes(cap_bytes).map_err(|_| OutcomeError {
        error: ActionError::cap_out_of_range(),
        outcome_not_known: false,
    })?;
    let made = state.control.places().make();
    let call = made.number();
    let control = Arc::clone(&state.control);
    let (answer, next_number_at_answer) = off_the_main_thread(move || {
        let answer = control.set_storage_cap(made, cap);
        (answer, control.places().next_number())
    })
    .await
    .map_err(|error| OutcomeError {
        error,
        outcome_not_known: true,
    })?;
    match answer {
        Ok(set) => {
            state
                .views
                .take_mutation(call, next_number_at_answer, &set.status);
            draw_off_the_main_thread(&state.views).await;
            Ok(CapSetView {
                view: state.views.marked(StatusView::of(&set.status)),
                evicting: Some(set.evicting),
            })
        }
        Err(error) => {
            let nothing_changed = removes_nothing(&error);
            let answer = Err(error);
            poller::take_command_answer(&state.views, call, next_number_at_answer, &answer);
            draw_off_the_main_thread(&state.views).await;
            Ok(CapSetView {
                view: state
                    .views
                    .marked(command_view(answer).map_err(|error| OutcomeError {
                        error,
                        outcome_not_known: !nothing_changed,
                    })?),
                evicting: None,
            })
        }
    }
}

fn no_onboarding() -> OnboardingState {
    OnboardingState {
        permissions: Vec::new(),
        lost_grants: Vec::new(),
        all_granted: false,
        walkback: Vec::new(),
    }
}

/// The rows of the Permissions view, which the page asks every 3 s while it shows: each call
/// sends one Status without the indicator claim, beside the poller's claiming pass, so that the
/// service reads the grants every 3 s while the view is open; the view of the app stays the
/// poller's. Without an answer the rows come from the poller's last status.
#[tauri::command]
pub async fn onboarding_state(state: State<'_, AppState>) -> Result<OnboardingState, ActionError> {
    let made = state.control.places().make();
    let control = Arc::clone(&state.control);
    let answer = off_the_main_thread(move || control.status(made, false)).await?;
    Ok(match answer {
        Ok(status) => onboarding_shown(&status),
        Err(_) => state
            .views
            .status()
            .map_or_else(no_onboarding, |status| onboarding_shown(&status)),
    })
}

#[tauri::command]
pub async fn request_permission(
    state: State<'_, AppState>,
    permission: Permission,
) -> Result<RequestOutcome, ActionError> {
    let made = state.control.places().make();
    let control = Arc::clone(&state.control);
    off_the_main_thread(move || control.request_permission(made, permission))
        .await?
        .map_err(|error| match &error {
            ClientError::Daemon(reply) if reply.code() == ErrorCode::UnsupportedOp => {
                ActionError::UnsupportedOp
            }
            _ => not_taken(&error),
        })
}

/// Puts a lost grant away with one acknowledge_lost_permission request, takes the
/// status that comes back into the view, so that the tray's line goes with it, and answers the
/// onboarding rows of that status.
#[tauri::command]
pub async fn acknowledge_lost_permission(
    state: State<'_, AppState>,
    permission: Permission,
) -> Result<OnboardingState, ActionError> {
    let made = state.control.places().make();
    let call = made.number();
    let control = Arc::clone(&state.control);
    let (answer, next_number_at_answer) = off_the_main_thread(move || {
        let answer = control.acknowledge_lost_permission(made, permission);
        (answer, control.places().next_number())
    })
    .await?;
    let status = answer.map_err(|error| not_taken(&error))?;
    state
        .views
        .take_mutation(call, next_number_at_answer, &status);
    draw_off_the_main_thread(&state.views).await;
    Ok(onboarding_shown(&status))
}

/// The reset of the settings alone, the command `reset_settings`: one reset_settings request,
/// whose status the views take as the answer of a mutation, and its view. It writes nothing of
/// its own, no login item among it: the file that the service writes is the record of what was
/// asked. A refusal is answered as the command's error, and no second request goes out.
pub fn reset(control: &dyn Control, views: &Views) -> Result<StatusView, ActionError> {
    let made = control.places().make();
    let call = made.number();
    let answer = control.reset_settings(made);
    let next_number_at_answer = control.places().next_number();
    let status = answer.map_err(|error| not_taken(&error))?;
    views.take_mutation(call, next_number_at_answer, &status);
    Ok(views.marked(StatusView::of(&status)))
}

#[tauri::command]
pub async fn reset_settings(state: State<'_, AppState>) -> Result<StatusView, ActionError> {
    let control = Arc::clone(&state.control);
    let views = Arc::clone(&state.views);
    let answer = off_the_main_thread(move || reset(control.as_ref(), &views)).await?;
    draw_off_the_main_thread(&state.views).await;
    answer
}

/// The turn of the switch of keys, the command `set_typing`: one set_typing request, whose
/// status the views take as the answer of a mutation, and its view. A refusal is answered as the
/// command's error, and no second request goes out: an older service's unsupported_op is
/// [`ActionError::UpdateTheApp`].
pub fn typing(control: &dyn Control, views: &Views, on: bool) -> Result<StatusView, ActionError> {
    let made = control.places().make();
    let call = made.number();
    let answer = control.set_typing(made, on);
    let next_number_at_answer = control.places().next_number();
    let status = answer.map_err(|error| not_taken(&error))?;
    views.take_mutation(call, next_number_at_answer, &status);
    Ok(views.marked(StatusView::of(&status)))
}

#[tauri::command]
pub async fn set_typing(state: State<'_, AppState>, on: bool) -> Result<StatusView, ActionError> {
    let control = Arc::clone(&state.control);
    let views = Arc::clone(&state.views);
    let answer = off_the_main_thread(move || typing(control.as_ref(), &views, on)).await?;
    draw_off_the_main_thread(&state.views).await;
    answer
}

fn not_taken(error: &ClientError) -> ActionError {
    let reading = read_error(error);
    reading.command_error.unwrap_or(ActionError::NotAnswering {
        service: reading.service,
    })
}

#[tauri::command]
pub async fn open_permission_pane(
    state: State<'_, AppState>,
    permission: Permission,
) -> Result<(), ActionError> {
    let open = state.open_pane;
    off_the_main_thread(move || open(permission))
        .await?
        .map_err(|_| ActionError::PaneNotOpened)
}

/// The end of a whole Delete everything beside Halvo's data directory: where `view` finished and
/// `folder`, the folder of the crash reports of the web view of Halvo's own window, is known,
/// every file under it goes by `remove`; where one stays, the answer names the folder, and the
/// delete of Halvo's data stays whole by it.
pub fn remove_the_crash_reports(
    view: &mut DeleteView,
    folder: Option<&Path>,
    remove: &dyn Fn(&Path) -> std::io::Result<()>,
) {
    let Some(folder) = folder else {
        return;
    };
    if !view.finished() {
        return;
    }
    if let DeleteView::Deleted {
        crash_reports_left, ..
    } = view
    {
        if !every_file_goes(folder, crate::app::REPORT_PARTS.len(), remove) {
            *crash_reports_left = Some(halvo_control::paths::one_line(&folder.to_string_lossy()));
        }
    }
}

/// Removes every file under `dir` by `remove`, folders kept: whether none stayed. A folder that
/// does not stand holds none. Where `dir` or one of the `above` folders above it is a link,
/// nothing goes and the answer is that one stays: the walk opens a folder by its path, so a link
/// there would lead it to files that are not the window's. Each folder below is read so again
/// before its walk.
pub(crate) fn every_file_goes(
    dir: &Path,
    above: usize,
    remove: &dyn Fn(&Path) -> std::io::Result<()>,
) -> bool {
    if a_link_on_the_way(dir, above) {
        return false;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => return error.kind() == std::io::ErrorKind::NotFound,
    };
    let mut none_stayed = true;
    for entry in entries {
        let Ok(entry) = entry else {
            none_stayed = false;
            continue;
        };
        let path = entry.path();
        let gone = match entry.file_type() {
            Ok(kind) if kind.is_dir() => every_file_goes(&path, 0, remove),
            Ok(_) => remove(&path).is_ok(),
            Err(_) => false,
        };
        none_stayed &= gone;
    }
    none_stayed
}

/// Whether `dir` or one of the `above` folders above it is a link, each read as itself and not
/// followed. On Windows the standard library reads a reparse point with a name surrogate's tag as
/// a link, so a junction counts. A part that cannot be read, for any cause but its absence,
/// counts as one.
pub(crate) fn a_link_on_the_way(dir: &Path, above: usize) -> bool {
    dir.ancestors()
        .take(above + 1)
        .any(|part| match std::fs::symlink_metadata(part) {
            Ok(meta) => meta.file_type().is_symlink(),
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deleted(code: i32, files_left: u64) -> DeleteView {
        let out = format!(
            "data_dir=/scratch/halvo-data\ndeleted segments_shredded=2 keys_destroyed=3 root_destroyed=yes root_id=- files_left={files_left} database_left=no not_followed=0 entries_removed=4 entries_left=0 unknown_entries=0 intent=written\n"
        );
        offline_view(&OfflineEnd::Exited { code, out }, None).expect("a deleted answer")
    }

    fn left(view: &DeleteView) -> Option<String> {
        match view {
            DeleteView::Deleted {
                crash_reports_left, ..
            } => crash_reports_left.clone(),
            other => panic!("not a deleted answer: {other:?}"),
        }
    }

    /// Test 7 of app-4b: at the end of a whole delete the crash reports of the window go; one that
    /// a stand-in removal refuses stays, and the answer names its folder, the delete still whole;
    /// a delete that did not finish removes none.
    #[test]
    fn a_crash_report_that_stays_at_a_whole_deletes_end_is_named_by_its_folder_and_the_delete_stays_whole(
    ) {
        let folder = std::env::current_exe()
            .expect("the test binary has a path")
            .ancestors()
            .nth(3)
            .expect("the test binary sits under target/<profile>/deps")
            .join("tmp")
            .join(format!("halvo-app-crash-reports-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("the folder of the reports");
        let shown = halvo_control::paths::one_line(&folder.to_string_lossy());
        let report = |name: &str| {
            let file = folder.join(name);
            std::fs::write(&file, b"a dump of the window").expect("a report");
            file
        };
        let (a, b) = (report("a.dmp"), report("b.dmp"));
        let refuses_b = |file: &Path| {
            if file == b.as_path() {
                Err(std::io::Error::other("held by the web view"))
            } else {
                std::fs::remove_file(file)
            }
        };
        let mut view = deleted(0, 0);
        remove_the_crash_reports(&mut view, Some(&folder), &refuses_b);
        assert_eq!(left(&view).as_deref(), Some(shown.as_str()));
        assert!(view.finished(), "the delete of Halvo's data stays whole");
        assert!(!a.exists() && b.exists());

        let mut view = deleted(0, 0);
        remove_the_crash_reports(&mut view, Some(&folder), &|file| std::fs::remove_file(file));
        assert_eq!(left(&view), None);
        assert!(!b.exists());

        let c = report("c.dmp");
        let mut unfinished = deleted(1, 1);
        remove_the_crash_reports(&mut unfinished, Some(&folder), &|file| {
            std::fs::remove_file(file)
        });
        assert_eq!(left(&unfinished), None);
        assert!(c.exists(), "a delete that did not finish removes no report");
    }
}
