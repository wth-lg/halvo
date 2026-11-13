use halvo_capture::consent::{CaptureState, ConsentView};
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::capture::AppState;

const TRAY_ID: &str = "halvo-status";
const TOGGLE_ID: &str = "toggle-capture";
const OPEN_ID: &str = "open-data-view";
const QUIT_ID: &str = "quit";

pub struct TrayHandles<R: Runtime> {
    tray: TrayIcon<R>,
    status_item: MenuItem<R>,
    toggle_item: MenuItem<R>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayLabels {
    pub title: &'static str,
    pub tooltip: &'static str,
    pub status_text: &'static str,
    pub toggle_text: &'static str,
}

pub fn labels(state: CaptureState) -> TrayLabels {
    match state {
        CaptureState::Running => TrayLabels {
            title: "Halvo ●",
            tooltip: "Halvo is capturing",
            status_text: "Capturing",
            toggle_text: "Pause capture",
        },
        CaptureState::Paused => TrayLabels {
            title: "Halvo ❚❚",
            tooltip: "Halvo capture is paused",
            status_text: "Paused",
            toggle_text: "Resume capture",
        },
    }
}

fn current_state<R: Runtime>(app: &AppHandle<R>) -> CaptureState {
    let state = app.state::<AppState>();
    let controller = state.controller.lock().unwrap_or_else(|e| e.into_inner());
    controller.status().state
}

pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let initial = labels(current_state(app));
    let status_item = MenuItem::with_id(app, "status", initial.status_text, false, None::<&str>)?;
    let toggle_item = MenuItem::with_id(app, TOGGLE_ID, initial.toggle_text, true, None::<&str>)?;
    let open_item = MenuItem::with_id(app, OPEN_ID, "Open Halvo", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, QUIT_ID, "Quit Halvo", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status_item,
            &PredefinedMenuItem::separator(app)?,
            &toggle_item,
            &open_item,
            &PredefinedMenuItem::separator(app)?,
            &quit_item,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .title(initial.title)
        .tooltip(initial.tooltip)
        .on_menu_event(on_menu_event);
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon).icon_as_template(true);
    }
    let tray = builder.build(app)?;

    app.manage(TrayHandles {
        tray,
        status_item,
        toggle_item,
    });
    Ok(())
}

pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let Some(handles) = app.try_state::<TrayHandles<R>>() else {
        return;
    };
    let texts = labels(current_state(app));
    let _ = handles.tray.set_title(Some(texts.title));
    let _ = handles.tray.set_tooltip(Some(texts.tooltip));
    let _ = handles.status_item.set_text(texts.status_text);
    let _ = handles.toggle_item.set_text(texts.toggle_text);
}

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        TOGGLE_ID => {
            {
                let state = app.state::<AppState>();
                let mut controller = state.controller.lock().unwrap_or_else(|e| e.into_inner());
                let next = match controller.status().state {
                    CaptureState::Running => CaptureState::Paused,
                    CaptureState::Paused => CaptureState::Running,
                };
                if let Err(e) = controller.set_capture_state(next) {
                    eprintln!("halvo: could not change the capture state: {e}");
                }
            }
            refresh(app);
            show_main_window(app);
        }
        OPEN_ID => show_main_window(app),
        QUIT_ID => app.exit(crate::launch::Exit::Quit.code()),
        _ => {}
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.eval(crate::app::WINDOW_SHOWN);
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_indicator_reflects_paused_and_running() {
        let running = labels(CaptureState::Running);
        let paused = labels(CaptureState::Paused);
        assert_ne!(running.title, paused.title);
        assert_eq!(running.toggle_text, "Pause capture");
        assert_eq!(paused.toggle_text, "Resume capture");
        assert!(running.tooltip.contains("capturing"));
        assert!(paused.tooltip.contains("paused"));
    }
}
