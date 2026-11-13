use std::sync::Mutex;

use halvo_capture::consent::onboarding::{
    assess, GrantState, OnboardingState, Permission, PermissionProbe, RequestOutcome,
};
use halvo_capture::consent::status::{ConsentController, Inventory, ObservationWindow, StorageCap};
use halvo_capture::consent::{CaptureState, CaptureStatus, ConsentView};
use halvo_capture::store::ShredReport;
use serde::Serialize;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Runtime, State};

use crate::tray;

pub struct AppState {
    pub controller: Mutex<ConsentController>,
    pub probe: Box<dyn PermissionProbe + Send + Sync>,
    pub store_backend: &'static str,
    pub open_pane: fn(Permission) -> Result<(), String>,
}

impl AppState {
    fn with_controller<T>(
        &self,
        f: impl FnOnce(&mut ConsentController) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut controller = self
            .controller
            .lock()
            .map_err(|_| "the consent controller is poisoned".to_string())?;
        f(&mut controller)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusView {
    #[serde(flatten)]
    pub status: CaptureStatus,
    pub window: ObservationWindow,
    pub store_backend: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ShredView {
    pub segments_shredded: u64,
    pub keys_destroyed: u64,
}

impl From<ShredReport> for ShredView {
    fn from(report: ShredReport) -> Self {
        Self {
            segments_shredded: report.segments_shredded,
            keys_destroyed: report.keys_destroyed,
        }
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

fn status_view(state: &AppState, controller: &mut ConsentController) -> Result<StatusView, String> {
    controller.inventory().map_err(|e| e.to_string())?;
    Ok(StatusView {
        status: controller.status(),
        window: controller.window(),
        store_backend: state.store_backend,
    })
}

#[tauri::command]
pub fn capture_status(state: State<'_, AppState>) -> Result<StatusView, String> {
    state.with_controller(|c| status_view(&state, c))
}

#[tauri::command]
pub fn inventory(state: State<'_, AppState>) -> Result<Inventory, String> {
    state.with_controller(|c| c.inventory().cloned().map_err(|e| e.to_string()))
}

#[tauri::command]
pub fn set_capture_state<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    capture: CaptureState,
) -> Result<StatusView, String> {
    let view = state.with_controller(|c| {
        c.set_capture_state(capture).map_err(|e| e.to_string())?;
        status_view(&state, c)
    })?;
    tray::refresh(&app);
    Ok(view)
}

#[tauri::command]
pub fn delete_all(state: State<'_, AppState>) -> Result<ShredView, String> {
    state.with_controller(|c| {
        c.delete_everything()
            .map(ShredView::from)
            .map_err(|e| e.to_string())
    })
}

#[tauri::command]
pub fn set_storage_cap(state: State<'_, AppState>, cap_bytes: u64) -> Result<StatusView, String> {
    let cap = StorageCap::from_bytes(cap_bytes).map_err(|e| e.to_string())?;
    state.with_controller(|c| {
        c.set_storage_cap(cap).map_err(|e| e.to_string())?;
        status_view(&state, c)
    })
}

#[tauri::command]
pub fn onboarding_state(state: State<'_, AppState>) -> Result<OnboardingState, String> {
    state.with_controller(|c| {
        let onboarding = assess(
            state.probe.as_ref(),
            Permission::required_on_this_platform(),
            &c.settings().permissions_once_granted,
        );
        let granted: Vec<Permission> = onboarding
            .permissions
            .iter()
            .filter(|p| p.grant == GrantState::Granted)
            .map(|p| p.permission)
            .collect();
        c.record_granted_permissions(&granted)
            .map_err(|e| e.to_string())?;
        Ok(onboarding)
    })
}

#[tauri::command]
pub fn request_permission(
    state: State<'_, AppState>,
    permission: Permission,
) -> Result<RequestOutcome, String> {
    Ok(state.probe.request(permission))
}

#[tauri::command]
pub fn open_permission_pane(
    state: State<'_, AppState>,
    permission: Permission,
) -> Result<(), String> {
    (state.open_pane)(permission)
}
