use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, PoisonError};

use halvo_control::model::CaptureState;
use halvo_control::proto::Capture;
use halvo_coord_app::{Agents, Attention};
use tauri::image::Image;
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

pub const WINDOW_CANNOT_OPEN_ON_THIS_MAC: &str = "Halvo's window cannot open on this Mac right now. You can still pause or resume here. Quit Halvo and start it again; if the window still does not open, a newer Halvo is needed to see or delete what Halvo has stored.";

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

/// The tray's line while the window cannot open: on macOS one sentence for every cause, so
/// that no cause can be sorted wrongly; elsewhere the line by whether a restart cures it.
pub fn without_window(labels: TrayLabels, restart_cures: bool) -> TrayLabels {
    TrayLabels {
        status_text: if cfg!(target_os = "macos") {
            WINDOW_CANNOT_OPEN_ON_THIS_MAC
        } else if restart_cures {
            WINDOW_MISSING
        } else {
            WINDOW_CANNOT_OPEN_HERE
        },
        open_enabled: false,
        ..labels
    }
}

/// The app's one quit, which main manages on a system where the app starts its daemon; without
/// it the tray's Quit exits at once.
pub type QuitWith<R> = fn(&AppHandle<R>);

/// The sentence that a quit which did not exit puts in the tray's status line and tooltip
/// until the next quit.
#[derive(Default)]
pub struct QuitNotice(pub std::sync::Mutex<Option<&'static str>>);

pub fn shown<R: Runtime>(app: &AppHandle<R>, view: &StatusView) -> TrayLabels {
    let labels = if let Some(missing) = app.try_state::<crate::app::WindowMissing>() {
        without_window(labels(view), missing.restart_cures)
    } else {
        labels(view)
    };
    let notice = app.try_state::<QuitNotice>().and_then(|notice| {
        *notice
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    match notice {
        Some(sentence) => TrayLabels {
            status_text: sentence,
            tooltip: sentence,
            ..labels
        },
        None => labels,
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
    let marked = with_attention(app, view);
    let _ = handles.tray.set_title(Some(&marked.title));
    let _ = handles.tray.set_tooltip(Some(&marked.tooltip));
    let _ = handles.status_item.set_text(texts.status_text);
    let _ = handles.toggle_item.set_text(texts.toggle_text);
    let _ = handles.toggle_item.set_enabled(texts.toggle_enabled);
    let _ = handles.open_item.set_enabled(texts.open_enabled);
}

/// The count of agents that need the person, as the tray last took it (I10.3).
#[derive(Default)]
pub struct Needs(Mutex<u32>);

/// What the tray shows for a status and the count of agents that need the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marked {
    pub title: String,
    pub tooltip: String,
    pub variant: bool,
}

/// What the tray shows for `view` with the count that it last took, by `Attention::of`
/// (I10.3): above zero the icon variant and the tooltip `tray.needs` with the count; on macOS
/// the tray's own title with the count after it, never the count alone, because the title says
/// whether Halvo captures; at zero the tray's own title, tooltip and icon.
pub fn with_attention<R: Runtime>(app: &AppHandle<R>, view: &StatusView) -> Marked {
    let count = app.try_state::<Needs>().map_or(0, |needs| {
        *needs.0.lock().unwrap_or_else(PoisonError::into_inner)
    });
    let own = shown(app, view);
    let attention = Attention::of(count);
    Marked {
        title: match attention.title {
            Some(count) => format!("{} {count}", own.title),
            None => own.title.to_string(),
        },
        tooltip: attention.tooltip.unwrap_or_else(|| own.tooltip.to_string()),
        variant: attention.variant,
    }
}

/// The tray method of `Tray::attention(count)` (I10.3). The hub calls it with the
/// coordinator's lock held, so it posts the change to the main thread and does not wait for
/// it: a wait there while the main thread calls a hook that waits for that lock would stop the
/// app for good.
pub fn set_attention<R: Runtime>(app: &AppHandle<R>, count: u32) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if handle.try_state::<Needs>().is_none() {
            handle.manage(Needs::default());
        }
        *handle
            .state::<Needs>()
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = count;
        let Some(handles) = handle.try_state::<TrayHandles<R>>() else {
            return;
        };
        let marked = with_attention(&handle, &handle.state::<AppState>().views.view());
        let _ = handles.tray.set_title(Some(&marked.title));
        let _ = handles.tray.set_tooltip(Some(&marked.tooltip));
        if let Some(icon) = handle.default_window_icon() {
            let icon = if marked.variant {
                with_dot(icon)
            } else {
                icon.clone()
            };
            let _ = handles.tray.set_icon(Some(icon));
            #[cfg(target_os = "macos")]
            let _ = handles.tray.set_icon_as_template(true);
        }
    });
}

/// The tray's icon with the mark of agents that need the person: a filled dot in its upper
/// right corner inside a clear ring, so that the mark also stands out in the template form of
/// macOS, which reads the alpha alone.
fn with_dot(icon: &Image<'_>) -> Image<'static> {
    let (width, height) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    let radius = width.min(height) as f32 / 4.0;
    let (cx, cy) = (width as f32 - radius, radius);
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let distance = (dx * dx + dy * dy).sqrt();
            let at = ((y * width + x) * 4) as usize;
            let Some(pixel) = rgba.get_mut(at..at + 4) else {
                continue;
            };
            if distance <= radius * 0.7 {
                pixel.copy_from_slice(&[0xd7, 0x26, 0x1e, 0xff]);
            } else if distance <= radius {
                pixel[3] = 0;
            }
        }
    }
    Image::new_owned(rgba, width, height)
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
    let call = made.number();
    let control = std::sync::Arc::clone(&state.control);
    let views = std::sync::Arc::clone(&state.views);
    std::thread::spawn(move || {
        let answer = match next {
            CaptureState::Paused => control.pause(made),
            CaptureState::Running => control.resume(made),
        };
        take_toggle_answer(&views, call, control.places().next_number(), &answer);
        views.draw_now();
    });
}

/// The answer of the tray's own Pause or Resume, as the commands take theirs: an Ok status or a
/// no-status answer replaces the view, an Err moves the mark.
pub fn take_toggle_answer(
    views: &crate::poller::Views,
    call: u64,
    next_number_at_answer: u64,
    answer: &Result<halvo_control::proto::DaemonStatus, halvo_control::client::ClientError>,
) {
    if let Err(e) = answer {
        eprintln!(
            "halvo: the capture service did not take the change: {:?}",
            crate::view_state::read_error(e).service
        );
    }
    crate::poller::take_command_answer(views, call, next_number_at_answer, answer);
}

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        TOGGLE_ID => {
            toggle(app);
            show_main_window(app);
        }
        OPEN_ID => show_main_window(app),
        QUIT_ID => quit_after_the_hook(app),
        _ => {}
    }
}

/// The tray's Quit. On a thread of its own the first Quit calls `Core::quit` with no budget,
/// since the tray sets the quit no time limit, and then `exit`; from its start no tick and no
/// other hook calls the Core, and a later Quit does nothing. A quit hook that unwinds on a
/// port's panic leaves its runs alive, and the next life's reap is their kill (ruling RD-34
/// (e)), so the exit follows all the same.
pub fn quit<R: Runtime>(app: &AppHandle<R>, exit: impl FnOnce(&AppHandle<R>) + Send + 'static) {
    let handle = app.clone();
    std::thread::spawn(move || {
        if !crate::app::begin_quit(&handle) {
            return;
        }
        if let Some(agents) = handle.try_state::<Agents>() {
            let core = agents.0.clone();
            let _ = catch_unwind(AssertUnwindSafe(|| core.quit(None)));
        }
        exit(&handle);
    });
}

/// The tray's Quit and the app menu's Quit. The first runs `quit`, which calls the Core's quit
/// hook and then the app's own quit; a later one, once that hook has run, runs the app's own
/// quit alone, so that a quit that stayed can be tried again and the Core is not called again.
pub fn quit_after_the_hook<R: Runtime>(app: &AppHandle<R>) {
    if crate::app::quitting(app) {
        quit_of_the_app(app);
    } else {
        quit(app, quit_of_the_app);
    }
}

fn quit_of_the_app<R: Runtime>(app: &AppHandle<R>) {
    match app.try_state::<QuitWith<R>>() {
        Some(quit) => (*quit)(app),
        None => app.exit(crate::launch::Exit::Quit.code()),
    }
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        if let Some(core) = crate::app::core_of(app) {
            core.shown();
        }
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
