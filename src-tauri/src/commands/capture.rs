use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use halvo_control::client::{ClientError, DeleteOutcome};
use halvo_control::model::{CaptureState, StorageCap, StoreId};
use halvo_control::onboarding::{OnboardingState, Permission, RequestOutcome};
use halvo_control::paths::DataDir;
use halvo_control::proto::{reaches_the_end, CaptureChange, DaemonStatus, ErrorCode, StateFile};
use tauri::ipc::Invoke;
use tauri::{Runtime, State};

use super::control::{control_of, Control, Made};
use super::names::{Names, CAPTURE};
use super::Registration;
use crate::poller::{self, Views};
use crate::view_state::{
    a_delete_removed_nothing, command_view, delete_view, inventory_view, read_error,
    removes_nothing, ActionError, CapSetView, CaptureSet, DeleteView, Hold, InventoryView,
    LastDelete, OutcomeError, StatusView,
};

pub struct AppState {
    pub control: Arc<dyn Control>,
    pub views: Arc<Views>,
    pub data_dir: Option<DataDir>,
    pub open_pane: fn(Permission) -> Result<(), String>,
    pub tray_installed: AtomicBool,
    pub last_delete: Mutex<LastDelete>,
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
/// `start_and_resume` says.
/// Answers its answer with the number of the call that it answers.
pub fn press(
    control: &dyn Control,
    views: &Views,
    made: Made,
    change: CaptureChange,
    within: Duration,
) -> (Result<DaemonStatus, ClientError>, u64) {
    if change == CaptureChange::Resume && views.hold().ask_start() {
        drop(made);
        return start_and_resume(control, views.hold(), within);
    }
    let call = made.number();
    let answer = match change {
        CaptureChange::Pause => control.pause(made),
        CaptureChange::Resume => control.resume(made),
    };
    (answer, call)
}

/// The press on Start after the third end with 5 in a row (contracts.md C14 item 4), once it
/// has asked the supervisor for exactly one child: asks for a plain Status until one is answered
/// or `within` has passed, the last Status keeping its own deadline of 2 s, and then sends the
/// Resume; with no answer it answers the deadline of `within`, sends no Resume, and the hold
/// stands. Where that child ends before a Status is answered, the asking goes on, and the
/// Resume goes to the next child of the backoff.
fn start_and_resume(
    control: &dyn Control,
    hold: &Hold,
    within: Duration,
) -> (Result<DaemonStatus, ClientError>, u64) {
    let until = Instant::now() + within;
    let answered = hold.wait_for_the_asked_child(until)
        && loop {
            if control.status(control.places().make(), false).is_ok() {
                break true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break false;
            }
            std::thread::sleep(left.min(START_STATUS_STEP));
        };
    let made = control.places().make();
    let call = made.number();
    if !answered {
        return (
            Err(ClientError::Deadline {
                seconds: within.as_secs(),
            }),
            call,
        );
    }
    (control.resume(made), call)
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
/// request (contracts.md C2.1).
pub fn ask_to_delete(
    control: &dyn Control,
    views: &Views,
    armed: Option<StoreId>,
    waits: DeleteWaits,
) -> DeleteAsked {
    let hold = views.hold();
    let life = hold.life();
    let (answer, call, next_number_at_answer) = ask_once(control, views, armed);
    let first = DeleteAsked {
        end: DeleteEnd::Answered(answer),
        call,
        next_number_at_answer,
    };
    let (Some(life), Some(_), DeleteEnd::Answered(Err(error))) = (life, armed, &first.end) else {
        return first;
    };
    if !asks_again(error) || !hold.ended_with_5(life, Instant::now() + waits.asked_life_end) {
        return first;
    }
    if !a_later_life_answers(control, hold, life, Instant::now() + waits.next_life) {
        let DeleteEnd::Answered(Err(error)) = first.end else {
            return first;
        };
        return DeleteAsked {
            end: DeleteEnd::NoLifeAnswered(error),
            call: first.call,
            next_number_at_answer: control.places().next_number(),
        };
    }
    let (answer, call, next_number_at_answer) = ask_once(control, views, armed);
    DeleteAsked {
        end: DeleteEnd::Answered(answer),
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

#[tauri::command]
pub async fn delete_all(
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
    let ended =
        off_the_main_thread(move || ask_to_delete(control.as_ref(), &views, armed, DELETE_WAITS))
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
    let (view, nothing_removed) = match end {
        DeleteEnd::Answered(answer) => {
            let nothing_removed = answer.as_ref().err().is_some_and(a_delete_removed_nothing);
            (delete_view(answer, roots.as_deref()), nothing_removed)
        }
        DeleteEnd::NoLifeAnswered(first) => (
            Err(ActionError::ServiceNotBack),
            a_delete_removed_nothing(&first),
        ),
    };
    match &view {
        Ok(DeleteView::Deleted { .. } | DeleteView::AlreadyDeleted { .. }) => {
            state.views.take_delete(call, next_number_at_answer);
        }
        Ok(DeleteView::DeleteRefused) => state.views.take_refusal(next_number_at_answer),
        Err(_) if nothing_removed => state.views.take_refusal(next_number_at_answer),
        Err(_) => state.views.take_delete(call, next_number_at_answer),
    }
    draw_off_the_main_thread(&state.views).await;
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

#[tauri::command]
pub async fn onboarding_state(state: State<'_, AppState>) -> Result<OnboardingState, ActionError> {
    Ok(state
        .views
        .status()
        .map_or_else(no_onboarding, |status| status.fields().onboarding.clone()))
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
        .map_err(|error| {
            let reading = read_error(&error);
            reading.command_error.unwrap_or(ActionError::NotAnswering {
                service: reading.service,
            })
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
