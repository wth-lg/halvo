use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, PoisonError};

use halvo_control::model::CaptureState;
use halvo_control::proto::{Capture, CaptureChange};
use halvo_coord_app::{Agents, Attention};
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::capture::{press, sort_change_answer, AppState, START_ANSWER_WAIT};
use crate::view_state::{NoStatusService, Press, RunningView, StatusView};

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
    drawn: DrawnMenu,
}

/// The labels that the tray's menu shows since its last draw, so that a press on the toggle
/// does what its item said.
#[derive(Debug, Default)]
pub struct DrawnMenu(std::sync::Mutex<Option<TrayLabels>>);

/// What a press on the tray's toggle does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TogglePress {
    /// The change that the drawn item named.
    Send(CaptureChange),
    /// The drawn item was not enabled.
    Nothing,
    /// The view or the hold changed since the draw: nothing is sent, and the menu is drawn anew,
    /// so that the person reads the new line before a second press.
    DrawAnew,
}

impl DrawnMenu {
    fn drawn(&self) -> std::sync::MutexGuard<'_, Option<TrayLabels>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The menu now shows `labels`.
    pub fn drew(&self, labels: TrayLabels) {
        *self.drawn() = Some(labels);
    }

    /// The labels of the last draw.
    #[cfg(test)]
    pub fn last(&self) -> Option<TrayLabels> {
        *self.drawn()
    }

    /// What a press does for `view`, the view of the moment, whose labels are `now`: the change
    /// that the drawn item named where the drawn labels are those of the moment, so that a
    /// Resume that ends the hold leaves only where the drawn status line named the cause;
    /// otherwise nothing is sent and the menu is drawn anew (contracts.md C14 item 4).
    pub fn press(&self, view: &StatusView, now: &TrayLabels) -> TogglePress {
        match *self.drawn() {
            Some(drawn) if drawn == *now => {
                toggled(view, now).map_or(TogglePress::Nothing, TogglePress::Send)
            }
            _ => TogglePress::DrawAnew,
        }
    }
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

/// The tray's status line while the app holds capture off after its child ended itself with
/// status 5: it names the cause, and a Resume of the toggle ends the hold only beside it.
pub const HELD: &str = "Capture off: one part did not stop in time";

/// The tray's status line and tooltip after a person's Pause whose answer does not read paused,
/// until the next answer of a Pause or a Resume: the page's words for it.
pub const PAUSE_NOT_DONE: &str =
    "Capture was not paused. Try again, or quit Halvo to stop capture.";

/// The tray's status line while a status names a lost grant, until the grant is back or put
/// away; the window's Permissions view says which one.
pub const LOST_GRANT: &str = "A permission that capture needs was turned off";

const PAUSED_TITLE: &str = "Halvo ❚❚";

/// The tray's tooltip while the app holds capture off.
pub const HELD_TOOLTIP: &str =
    "Halvo shut its capture service down because one part of capture did not stop in time";

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
    let window_missing = app
        .try_state::<crate::app::WindowMissing>()
        .map(|missing| missing.restart_cures);
    let notice = app.try_state::<QuitNotice>().and_then(|notice| {
        *notice
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    let pause_missed = app
        .try_state::<AppState>()
        .is_some_and(|state| state.views.pause_missed());
    labels_with(view, window_missing, pause_missed, notice)
}

/// The tray's labels for `view`: where the window cannot open its line, unless the hold stands,
/// whose line names its cause; after a Pause that did not happen its sentence, unless the hold
/// stands, and never the paused title; a quit's notice over every line; and while the hold
/// stands a toggle only where the status line names the cause (contracts.md C14 item 4).
pub fn labels_with(
    view: &StatusView,
    window_missing: Option<bool>,
    pause_missed: bool,
    quit_notice: Option<&'static str>,
) -> TrayLabels {
    let held = view.hold().is_some();
    let mut labels = labels(view);
    if let Some(restart_cures) = window_missing {
        labels = if held {
            TrayLabels {
                open_enabled: false,
                ..labels
            }
        } else {
            without_window(labels, restart_cures)
        };
    }
    if pause_missed && !held {
        labels = TrayLabels {
            title: if labels.title == PAUSED_TITLE {
                "Halvo"
            } else {
                labels.title
            },
            status_text: PAUSE_NOT_DONE,
            tooltip: PAUSE_NOT_DONE,
            ..labels
        };
    }
    if let Some(sentence) = quit_notice {
        labels = TrayLabels {
            status_text: sentence,
            tooltip: sentence,
            ..labels
        };
    }
    if held && labels.status_text != HELD {
        labels.toggle_enabled = false;
    }
    labels
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
            Capture::Paused => line(PAUSED_TITLE, "Paused", "Halvo capture is paused"),
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
        labels.toggle_text = match toggle_change(view) {
            CaptureChange::Pause => "Pause capture",
            CaptureChange::Resume => "Resume capture",
        };
        labels.toggle_enabled = true;
        if !view.lost_grants.is_empty() {
            labels.status_text = LOST_GRANT;
        }
    }
    if let Some(hold) = view.hold() {
        labels = TrayLabels {
            title: "Halvo",
            tooltip: HELD_TOOLTIP,
            status_text: HELD,
            toggle_text: match hold.press {
                Press::Resume => "Resume capture",
                Press::Start => "Start capture",
            },
            toggle_enabled: match hold.press {
                Press::Resume => matches!(view, StatusView::Running(_)),
                Press::Start => true,
            },
            ..labels
        };
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

    let drawn = DrawnMenu::default();
    drawn.drew(initial);
    app.manage(TrayHandles {
        tray,
        status_item,
        toggle_item,
        open_item,
        drawn,
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
    handles.drawn.drew(texts);
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
    let Some(handles) = app.try_state::<TrayHandles<R>>() else {
        return;
    };
    let view = state.views.view();
    let views = std::sync::Arc::clone(&state.views);
    let change = match handles.drawn.press(&view, &shown(app, &view)) {
        TogglePress::Send(change) => change,
        TogglePress::Nothing => return,
        TogglePress::DrawAnew => {
            std::thread::spawn(move || views.draw_now());
            return;
        }
    };
    let made = state.control.places().make();
    let control = std::sync::Arc::clone(&state.control);
    std::thread::spawn(move || {
        let (answer, call) = press(control.as_ref(), &views, made, change, START_ANSWER_WAIT);
        sort_change_answer(&views, change, &answer);
        take_toggle_answer(&views, call, control.places().next_number(), &answer);
        views.draw_now();
    });
}

/// The change that a press on the tray's toggle sends for `view` beside the labels `shown`:
/// none where the toggle is not enabled; while the hold stands the Resume, which the toggle is
/// enabled for only where its status line names the cause.
fn toggled(view: &StatusView, shown: &TrayLabels) -> Option<CaptureChange> {
    if !shown.toggle_enabled {
        return None;
    }
    if view.hold().is_some() {
        return Some(CaptureChange::Resume);
    }
    let StatusView::Running(view) = view else {
        return None;
    };
    Some(toggle_change(view))
}

fn toggle_change(view: &RunningView) -> CaptureChange {
    match (view.capture, view.desired) {
        (Capture::AwaitingResume, _) | (_, CaptureState::Paused) => CaptureChange::Resume,
        (_, CaptureState::Running) => CaptureChange::Pause,
    }
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

    fn answered(capture: Capture, desired: CaptureState) -> halvo_control::proto::DaemonStatus {
        let mut f = sample_status(Platform::Macos).into_fields();
        f.capture = capture;
        f.desired = desired;
        if capture == Capture::Paused {
            for line in f.sources.iter_mut() {
                if matches!(
                    line.name,
                    halvo_control::proto::SourceName::Foreground
                        | halvo_control::proto::SourceName::Input
                ) {
                    line.state = halvo_control::proto::SourceState::Stopped { by_user: false };
                }
            }
        }
        halvo_control::proto::DaemonStatus::new(f).expect("the status holds together")
    }

    #[test]
    fn a_pause_with_no_answer_reads_not_paused_in_the_command_and_in_the_tray() {
        use crate::commands::capture::{capture_set, sort_change_answer};
        use crate::poller::Views;
        use halvo_control::client::ClientError;
        use halvo_control::proto::{DaemonStatus, ErrorCode, ErrorReply};
        use std::io::{Error, ErrorKind};
        let rows = || -> Vec<(&str, Result<DaemonStatus, ClientError>)> {
            vec![
                (
                    "no reply inside the deadline",
                    Err(ClientError::Deadline { seconds: 10 }),
                ),
                (
                    "a broken exchange",
                    Err(ClientError::Io(Error::from(ErrorKind::ConnectionReset))),
                ),
                (
                    "a reply that cannot be read",
                    Err(ClientError::Protocol("a cut frame".to_owned())),
                ),
                (
                    "a child that ended before its reply",
                    Err(ClientError::Io(Error::from(ErrorKind::UnexpectedEof))),
                ),
                (
                    "the typed busy of a life that is ending",
                    Err(ClientError::ServiceEnding(
                        ErrorReply::busy_of_an_ending_life(),
                    )),
                ),
                (
                    "the plain busy",
                    Err(ClientError::Daemon(ErrorReply::new(
                        ErrorCode::Busy,
                        "busy",
                    ))),
                ),
                (
                    "a view with no status",
                    Err(ClientError::Unreachable {
                        endpoint: String::new(),
                    }),
                ),
                (
                    "a status that reads pausing",
                    Ok(answered(Capture::Pausing, CaptureState::Paused)),
                ),
            ]
        };
        let pause = CaptureChange::Pause;
        let (command, toggle) = (Views::default(), Views::default());
        for (n, ((name, by_command), (_, by_toggle))) in rows().into_iter().zip(rows()).enumerate()
        {
            let call = 2 * n as u64;
            if let Ok(set) = capture_set(&command, call, call + 1, pause, by_command) {
                assert!(!set.reached, "{name}: the command's reached");
            }
            assert!(!sort_change_answer(&toggle, pause, &by_toggle), "{name}");
            take_toggle_answer(&toggle, call, call + 1, &by_toggle);
            for views in [&command, &toggle] {
                let shown = labels_with(&views.view(), None, views.pause_missed(), None);
                assert_eq!(shown.status_text, PAUSE_NOT_DONE, "{name}");
                assert_eq!(shown.tooltip, PAUSE_NOT_DONE, "{name}");
                assert_ne!(shown.title, PAUSED_TITLE, "{name}");
            }
        }
        let paused = || Ok(answered(Capture::Paused, CaptureState::Paused));
        let set = capture_set(&command, 100, 101, pause, paused()).expect("a status");
        assert!(set.reached, "a Pause that happened");
        assert!(sort_change_answer(&toggle, pause, &paused()));
        for views in [&command, &toggle] {
            let shown = labels_with(&views.view(), None, views.pause_missed(), None);
            assert_ne!(
                shown.status_text, PAUSE_NOT_DONE,
                "the next answer ends the line"
            );
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

    #[test]
    fn at_awaiting_resume_the_toggle_reads_resume_and_its_press_sends_the_resume() {
        let view = running_with(Capture::AwaitingResume, CaptureState::Running);
        let shown = labels(&view);
        assert_eq!(
            (shown.status_text, shown.toggle_text, shown.toggle_enabled),
            ("Capture off until you resume", "Resume capture", true)
        );
        let menu = DrawnMenu::default();
        menu.drew(shown);
        assert_eq!(
            menu.press(&view, &shown),
            TogglePress::Send(CaptureChange::Resume)
        );
    }
}
