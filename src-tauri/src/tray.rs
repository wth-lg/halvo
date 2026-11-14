use halvo_control::model::CaptureState;
use halvo_control::proto::Capture;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::capture::AppState;
use crate::view_state::{NoStatusService, StatusView};

const TRAY_ID: &str = "halvo-status";
const TOGGLE_ID: &str = "toggle-capture";
const OPEN_ID: &str = "open-data-view";
const QUIT_ID: &str = "quit";

#[derive(Debug)]
pub struct TrayInstalled(());

pub struct TrayHandles<R: Runtime> {
    tray: TrayIcon<R>,
    status_item: MenuItem<R>,
    toggle_item: MenuItem<R>,
    open_item: MenuItem<R>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayLabels {
    pub title: &'static str,
    pub tooltip: &'static str,
    pub status_text: &'static str,
    pub toggle_text: &'static str,
    pub toggle_enabled: bool,
    pub open_enabled: bool,
}

pub const NOT_RUNNING: &str = "Capture service not running";

pub const WINDOW_MISSING: &str = "Halvo's window is not available. Quit Halvo and start it again.";

pub const WINDOW_CANNOT_OPEN_HERE: &str = "Halvo's window cannot open on this version of macOS. You can still pause or resume here. A newer Halvo is needed to see or delete what Halvo has stored.";

fn line(title: &'static str, status_text: &'static str, tooltip: &'static str) -> TrayLabels {
    TrayLabels {
        title,
        tooltip,
        status_text,
        toggle_text: "Pause capture",
        toggle_enabled: false,
        open_enabled: true,
    }
}

pub fn without_window(labels: TrayLabels, restart_cures: bool) -> TrayLabels {
    TrayLabels {
        status_text: if restart_cures {
            WINDOW_MISSING
        } else {
            WINDOW_CANNOT_OPEN_HERE
        },
        open_enabled: false,
        ..labels
    }
}

pub fn shown<R: Runtime>(app: &AppHandle<R>, view: &StatusView) -> TrayLabels {
    if let Some(missing) = app.try_state::<crate::app::WindowMissing>() {
        without_window(labels(view), missing.restart_cures)
    } else {
        labels(view)
    }
}

pub fn labels(view: &StatusView) -> TrayLabels {
    let mut labels = match view {
        StatusView::NoStatus(view) => match view.service {
            NoStatusService::Connecting => line(
                "Halvo",
                "Connecting",
                "Halvo is connecting to its capture service",
            ),
            NoStatusService::NotRunning => line("Halvo", NOT_RUNNING, NOT_RUNNING),
            NoStatusService::HeldUnreachable => line(
                "Halvo",
                "Capture service not answering",
                "Halvo's capture service is not answering",
            ),
            NoStatusService::StatusRefused => line(
                "Halvo",
                "Capture state unclear",
                "Halvo cannot rely on what its capture service reported",
            ),
            NoStatusService::AnotherVersion => line(
                "Halvo",
                "Update Halvo",
                "Halvo's capture service is of another version",
            ),
            NoStatusService::Unsupported => line(
                "Halvo",
                "Capture not available on Windows yet",
                "Capture is not available on Windows in this version",
            ),
        },
        StatusView::Running(view) => match view.capture {
            Capture::Running => line("Halvo ●", "Capturing", "Halvo is capturing"),
            Capture::Partial => line(
                "Halvo ●",
                "Capturing in part",
                "Halvo is capturing, but not every part runs",
            ),
            Capture::Starting => line("Halvo", "Starting", "Halvo's capture is starting"),
            Capture::Pausing => line("Halvo", "Stopping capture", "Halvo's capture is stopping"),
            Capture::Paused => line("Halvo ❚❚", "Paused", "Halvo capture is paused"),
            Capture::NoIndicator => line(
                "Halvo",
                "Not capturing",
                "Halvo is not capturing while it cannot show that it runs",
            ),
            Capture::AwaitingResume => line(
                "Halvo",
                "Capture off until you resume",
                "Halvo's capture is off until you resume it",
            ),
            Capture::Faulted => line(
                "Halvo",
                "Not capturing: a fault",
                "Halvo is not capturing because of a fault",
            ),
        },
    };
    if let StatusView::Running(view) = view {
        match view.desired {
            CaptureState::Running => {
                labels.toggle_text = "Pause capture";
                labels.toggle_enabled = true;
            }
            CaptureState::Paused => {
                labels.toggle_text = "Resume capture";
                labels.toggle_enabled = true;
            }
        }
    }
    labels
}

pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayInstalled> {
    let initial = shown(app, &app.state::<AppState>().views.view());
    let status_item = MenuItem::with_id(app, "status", initial.status_text, false, None::<&str>)?;
    let toggle_item = MenuItem::with_id(
        app,
        TOGGLE_ID,
        initial.toggle_text,
        initial.toggle_enabled,
        None::<&str>,
    )?;
    let open_item = MenuItem::with_id(
        app,
        OPEN_ID,
        "Open Halvo",
        initial.open_enabled,
        None::<&str>,
    )?;
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
        open_item,
    });
    Ok(TrayInstalled(()))
}

pub fn refresh<R: Runtime>(app: &AppHandle<R>, view: &StatusView) {
    let Some(handles) = app.try_state::<TrayHandles<R>>() else {
        return;
    };
    let texts = shown(app, view);
    let _ = handles.tray.set_title(Some(texts.title));
    let _ = handles.tray.set_tooltip(Some(texts.tooltip));
    let _ = handles.status_item.set_text(texts.status_text);
    let _ = handles.toggle_item.set_text(texts.toggle_text);
    let _ = handles.toggle_item.set_enabled(texts.toggle_enabled);
    let _ = handles.open_item.set_enabled(texts.open_enabled);
}

fn toggle<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let StatusView::Running(view) = state.views.view() else {
        return;
    };
    let next = match view.desired {
        CaptureState::Running => CaptureState::Paused,
        CaptureState::Paused => CaptureState::Running,
    };
    let made = state.control.places().make();
    let control = std::sync::Arc::clone(&state.control);
    let views = std::sync::Arc::clone(&state.views);
    let app = app.clone();
    std::thread::spawn(move || {
        let answer = match next {
            CaptureState::Paused => control.pause(made),
            CaptureState::Running => control.resume(made),
        };
        match answer {
            Ok(status) => views.take_mutation(control.places().next_number(), &status),
            Err(e) => eprintln!(
                "halvo: the capture service did not take the change: {:?}",
                crate::view_state::read_error(&e).service
            ),
        }
        refresh(&app, &views.view());
    });
}

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        TOGGLE_ID => {
            toggle(app);
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
pub fn installed_for_tests() -> TrayInstalled {
    TrayInstalled(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_state::StatusView;
    use halvo_control::proto::Platform;
    use halvo_control::testing::sample_status;

    fn running_with(capture: Capture, desired: CaptureState) -> StatusView {
        let StatusView::Running(mut view) = StatusView::of(&sample_status(Platform::Macos)) else {
            panic!("the sample status makes a running view")
        };
        view.capture = capture;
        view.desired = desired;
        StatusView::Running(view)
    }

    #[test]
    fn the_indicator_reflects_paused_and_running() {
        let running = labels(&running_with(Capture::Running, CaptureState::Running));
        let paused = labels(&running_with(Capture::Paused, CaptureState::Paused));
        assert_ne!(running.title, paused.title);
        assert_eq!(running.toggle_text, "Pause capture");
        assert_eq!(paused.toggle_text, "Resume capture");
        assert!(running.tooltip.contains("capturing"));
        assert!(paused.tooltip.contains("paused"));
    }

    #[test]
    fn with_no_status_the_tray_names_the_service_and_claims_neither_capturing_nor_paused() {
        for service in [
            NoStatusService::Connecting,
            NoStatusService::NotRunning,
            NoStatusService::HeldUnreachable,
            NoStatusService::StatusRefused,
            NoStatusService::Unsupported,
            NoStatusService::AnotherVersion,
        ] {
            let texts = labels(&StatusView::no_status(service, Platform::Macos));
            for text in [texts.title, texts.tooltip, texts.status_text] {
                let lower = text.to_lowercase();
                assert!(
                    !lower.contains("capturing") && !lower.contains("paused"),
                    "{service:?}: {text}"
                );
            }
            assert!(!texts.toggle_enabled, "{service:?}");
        }
        assert_eq!(
            labels(&StatusView::no_status(
                NoStatusService::NotRunning,
                Platform::Macos
            ))
            .status_text,
            "Capture service not running"
        );
        for service in [
            NoStatusService::HeldUnreachable,
            NoStatusService::StatusRefused,
        ] {
            assert!(!labels(&StatusView::no_status(service, Platform::Macos))
                .status_text
                .contains("not running"));
        }
    }

    #[test]
    fn pausing_never_reads_paused_and_the_toggle_follows_desired() {
        let pausing = labels(&running_with(Capture::Pausing, CaptureState::Paused));
        assert!(!pausing.status_text.to_lowercase().contains("paused"));
        assert!(!pausing.tooltip.to_lowercase().contains("paused"));
        assert_eq!(pausing.toggle_text, "Resume capture");
        let starting = labels(&running_with(Capture::Starting, CaptureState::Running));
        assert_eq!(starting.status_text, "Starting");
        assert_eq!(starting.toggle_text, "Pause capture");
        for capture in Capture::ALL
            .iter()
            .copied()
            .filter(|c| *c != Capture::Paused)
        {
            let texts = labels(&running_with(capture, CaptureState::Running));
            assert!(!texts.status_text.contains("Paused"), "{capture:?}");
        }
    }
}
