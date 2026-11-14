use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use halvo_control::model::{CaptureState, StorageCap, StoreId};
use halvo_control::onboarding::{OnboardingState, Permission, RequestOutcome};
use halvo_control::paths::DataDir;
use halvo_control::proto::{reaches_the_end, CaptureChange, DaemonStatus, StateFile};
use tauri::ipc::Invoke;
use tauri::{AppHandle, Runtime, State};

use super::control::{control_of, Control};
use super::names::{Names, CAPTURE};
use super::Registration;
use crate::poller::Views;
use crate::tray;
use crate::view_state::{
    command_view, delete_view, inventory_view, read_error, ActionError, CapSetView, CaptureSet,
    DeleteView, InventoryView, StatusView,
};

pub struct AppState {
    pub control: Arc<dyn Control>,
    pub views: Arc<Views>,
    pub data_dir: Option<DataDir>,
    pub open_pane: fn(Permission) -> Result<(), String>,
    pub tray_installed: AtomicBool,
}

impl AppState {
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
    ];
    handle(invoke)
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
pub async fn set_capture_state<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    capture: CaptureState,
) -> Result<CaptureSet, ActionError> {
    let made = state.control.places().make();
    let control = Arc::clone(&state.control);
    let change = match capture {
        CaptureState::Paused => CaptureChange::Pause,
        CaptureState::Running => CaptureChange::Resume,
    };
    let (answer, next_number_at_answer) = off_the_main_thread(move || {
        let answer = match change {
            CaptureChange::Pause => control.pause(made),
            CaptureChange::Resume => control.resume(made),
        };
        (answer, control.places().next_number())
    })
    .await?;
    let reached = answer.as_ref().is_ok_and(|status| holds(change, status));
    if let Ok(status) = &answer {
        state.views.take_mutation(next_number_at_answer, status);
    }
    let view = command_view(answer)?;
    tray::refresh(&app, &state.views.view());
    Ok(CaptureSet { view, reached })
}

pub fn holds(change: CaptureChange, status: &DaemonStatus) -> bool {
    reaches_the_end(change, status) && status.fields().state_file == StateFile::Ok
}

#[tauri::command]
pub async fn delete_all(
    state: State<'_, AppState>,
    store_id: Option<String>,
) -> Result<DeleteView, ActionError> {
    let armed = match store_id {
        None => None,
        Some(text) => Some(
            text.parse::<StoreId>()
                .map_err(|_| ActionError::BadStoreId)?,
        ),
    };
    let made = state.control.places().make();
    let control = Arc::clone(&state.control);
    let answer = off_the_main_thread(move || control.delete_all(made, armed)).await?;
    let roots = state.data_dir.as_ref().map(DataDir::roots_dir);
    delete_view(answer, roots.as_deref())
}

#[tauri::command]
pub async fn set_storage_cap(
    state: State<'_, AppState>,
    cap_bytes: u64,
) -> Result<CapSetView, ActionError> {
    let cap = StorageCap::from_bytes(cap_bytes).map_err(|_| ActionError::cap_out_of_range())?;
    let made = state.control.places().make();
    let control = Arc::clone(&state.control);
    let (answer, next_number_at_answer) = off_the_main_thread(move || {
        let answer = control.set_storage_cap(made, cap);
        (answer, control.places().next_number())
    })
    .await?;
    match answer {
        Ok(set) => {
            state
                .views
                .take_mutation(next_number_at_answer, &set.status);
            Ok(CapSetView {
                view: StatusView::of(&set.status),
                evicting: Some(set.evicting),
            })
        }
        Err(error) => Ok(CapSetView {
            view: command_view(Err(error))?,
            evicting: None,
        }),
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
