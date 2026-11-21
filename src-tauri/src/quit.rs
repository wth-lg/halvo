use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};

use halvo_control::client::ClientError;
use halvo_control::proto::{Capture, DaemonStatus};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Builder, Manager, Runtime};

use crate::commands::capture::AppState;
use crate::commands::control::Control;
use crate::launch::Exit;
use crate::tray::{QuitNotice, QuitWith};
use crate::view_state::{read_error, NoStatusService};

/// The app's daemon child as a quit ends it.
pub trait Children: Send + Sync {
    /// Closes the descriptor of the child of the moment, waits up to STOP_WAIT_MS for its end
    /// and kills it after that; no child starts after it.
    fn end(&self);
}

/// The children of the app, as main manages them once it has started the daemon.
pub struct AppChildren(pub Option<Arc<dyn Children>>);

/// What a quit comes to: the exit with its status, or a stay with the sentence that the app
/// shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quit {
    Exit(i32),
    Stay(&'static str),
}

/// The sentence of a quit that did not exit, because the capture service did not confirm the
/// pause.
pub const NOT_PAUSED: &str =
    "Halvo did not quit, because its capture service did not confirm the pause. Choose Quit again to try once more.";

/// The id of the app menu's Quit.
pub const APP_QUIT_ID: &str = "app-quit";

/// The keys of the app menu's Quit.
pub const QUIT_KEYS: &str = "CmdOrCtrl+Q";

/// An item of the system in the app menu; no item of this list quits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemItem {
    Hide,
    HideOthers,
    ShowAll,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Minimize,
    CloseWindow,
}

/// An entry of the app menu: an item of the system, a separator, or the app's own Quit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    System(SystemItem),
    Separator,
    Quit,
}

/// The app menu in place of the default macOS menu, whose predefined Quit runs no code of the
/// app: each submenu with its entries.
pub const APP_MENU: [(&str, &[Entry]); 3] = [
    (
        "Halvo",
        &[
            Entry::System(SystemItem::Hide),
            Entry::System(SystemItem::HideOthers),
            Entry::System(SystemItem::ShowAll),
            Entry::Separator,
            Entry::Quit,
        ],
    ),
    (
        "Edit",
        &[
            Entry::System(SystemItem::Undo),
            Entry::System(SystemItem::Redo),
            Entry::Separator,
            Entry::System(SystemItem::Cut),
            Entry::System(SystemItem::Copy),
            Entry::System(SystemItem::Paste),
            Entry::System(SystemItem::SelectAll),
        ],
    ),
    (
        "Window",
        &[
            Entry::System(SystemItem::Minimize),
            Entry::System(SystemItem::CloseWindow),
        ],
    ),
];

/// Whether a quit may go on after the answer to its Pause: a status that reads paused or
/// pausing, or no daemon that answers.
pub fn may_exit(answer: &Result<DaemonStatus, ClientError>) -> bool {
    match answer {
        Ok(status) => matches!(status.fields().capture, Capture::Paused | Capture::Pausing),
        Err(error) => matches!(
            read_error(error).service,
            NoStatusService::NotRunning | NoStatusService::Unsupported
        ),
    }
}

/// The steps of the app's one quit: Pause with the client's Pause deadline; on a status that
/// reads paused or pausing, or with no daemon that answers, the end of the child and the exit
/// with status 0; on any other answer a stay.
pub fn quit_and_pause(control: &dyn Control, children: Option<&dyn Children>) -> Quit {
    let answer = control.pause(control.places().make());
    if !may_exit(&answer) {
        return Quit::Stay(NOT_PAUSED);
    }
    if let Some(children) = children {
        children.end();
    }
    Quit::Exit(Exit::Quit.code())
}

static QUITTING: AtomicBool = AtomicBool::new(false);

/// The app's one quit, for the tray's Quit and the app menu's Quit: `quit_and_pause` on a
/// thread of its own; a stay shows its sentence in the tray's status line and on standard
/// error. A Quit while one runs does nothing.
pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    if QUITTING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("halvo-quit".to_owned())
        .spawn(move || {
            notice(&app, None);
            let state = app.state::<AppState>();
            let children = app.try_state::<AppChildren>();
            let children = children.as_ref().and_then(|held| held.0.as_deref());
            match quit_and_pause(state.control.as_ref(), children) {
                Quit::Exit(status) => app.exit(status),
                Quit::Stay(sentence) => {
                    eprintln!("halvo: {sentence}");
                    notice(&app, Some(sentence));
                    state.views.draw_now();
                    QUITTING.store(false, Ordering::SeqCst);
                }
            }
        });
    if spawned.is_err() {
        QUITTING.store(false, Ordering::SeqCst);
    }
}

fn notice<R: Runtime>(app: &AppHandle<R>, sentence: Option<&'static str>) {
    if let Some(notice) = app.try_state::<QuitNotice>() {
        *notice.0.lock().unwrap_or_else(PoisonError::into_inner) = sentence;
    }
}

/// The app menu of `APP_MENU`.
pub fn app_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    for (title, entries) in APP_MENU {
        let submenu = Submenu::new(app, title, true)?;
        for entry in entries {
            match entry {
                Entry::Quit => submenu.append(&MenuItem::with_id(
                    app,
                    APP_QUIT_ID,
                    "Quit Halvo",
                    true,
                    Some(QUIT_KEYS),
                )?)?,
                Entry::Separator => submenu.append(&PredefinedMenuItem::separator(app)?)?,
                Entry::System(item) => submenu.append(&system_item(app, *item)?)?,
            }
        }
        menu.append(&submenu)?;
    }
    Ok(menu)
}

fn system_item<R: Runtime>(
    app: &AppHandle<R>,
    item: SystemItem,
) -> tauri::Result<PredefinedMenuItem<R>> {
    match item {
        SystemItem::Hide => PredefinedMenuItem::hide(app, None),
        SystemItem::HideOthers => PredefinedMenuItem::hide_others(app, None),
        SystemItem::ShowAll => PredefinedMenuItem::show_all(app, None),
        SystemItem::Undo => PredefinedMenuItem::undo(app, None),
        SystemItem::Redo => PredefinedMenuItem::redo(app, None),
        SystemItem::Cut => PredefinedMenuItem::cut(app, None),
        SystemItem::Copy => PredefinedMenuItem::copy(app, None),
        SystemItem::Paste => PredefinedMenuItem::paste(app, None),
        SystemItem::SelectAll => PredefinedMenuItem::select_all(app, None),
        SystemItem::Minimize => PredefinedMenuItem::minimize(app, None),
        SystemItem::CloseWindow => PredefinedMenuItem::close_window(app, None),
    }
}

/// Puts the app's one quit in place: the default macOS menu off, the app menu with its own
/// Quit, and the quit that the tray's Quit calls.
pub fn installed<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
        .enable_macos_default_menu(false)
        .menu(app_menu)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == APP_QUIT_ID {
                crate::tray::quit_after_the_hook(app);
            }
        })
        .manage(quit::<R> as QuitWith<R>)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use halvo_control::client::DeleteOutcome;
    use halvo_control::model::{CaptureState, StorageCap, StoreId};
    use halvo_control::onboarding::{Permission, RequestOutcome};
    use halvo_control::proto::{
        CapSet, ErrorCode, ErrorReply, InventoryReply, Platform, SourceName, SourceState,
    };
    use halvo_control::testing::sample_status;

    use super::*;
    use crate::commands::control::{ChildPid, Made, Places};
    use crate::daemon_child::stand_in::{note, Record, StandInChild, StandInClock, StandInSpawner};
    use crate::daemon_child::Supervisor;

    type Row = (
        &'static str,
        Result<DaemonStatus, ClientError>,
        bool,
        Vec<String>,
    );

    struct Pausing {
        places: Arc<Places>,
        answer: Mutex<Option<Result<DaemonStatus, ClientError>>>,
        record: Record,
    }

    impl Control for Pausing {
        fn places(&self) -> &Arc<Places> {
            &self.places
        }

        fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
            unreachable!("a quit reads no status")
        }

        fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
            unreachable!("a quit reads no inventory")
        }

        fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            note(&self.record, "pause");
            self.answer
                .lock()
                .unwrap()
                .take()
                .expect("one Pause in a quit")
        }

        fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
            unreachable!("a quit sends no Resume")
        }

        fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
            unreachable!("a quit deletes nothing")
        }

        fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
            unreachable!("a quit sets no cap")
        }

        fn request_permission(
            &self,
            _: Made,
            _: Permission,
        ) -> Result<RequestOutcome, ClientError> {
            unreachable!("a quit asks for no permission")
        }
    }

    fn status(capture: Capture, desired: CaptureState) -> DaemonStatus {
        let mut f = sample_status(Platform::Macos).into_fields();
        f.capture = capture;
        f.desired = desired;
        if capture == Capture::Paused {
            for line in f.sources.iter_mut() {
                if matches!(line.name, SourceName::Foreground | SourceName::Input) {
                    line.state = SourceState::Stopped { by_user: false };
                }
            }
        }
        DaemonStatus::new(f).expect("the status holds together")
    }

    #[test]
    fn quit_pauses_closes_the_descriptor_waits_for_the_child_and_exits_zero() {
        let exit = |line: &[&str]| line.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        let rows: Vec<Row> = vec![
            (
                "paused",
                Ok(status(Capture::Paused, CaptureState::Paused)),
                true,
                exit(&["pause", "closed descriptor", "exit 0"]),
            ),
            (
                "pausing, and a child that does not end",
                Ok(status(Capture::Pausing, CaptureState::Paused)),
                false,
                exit(&["pause", "closed descriptor", "kill after 20 s", "exit 0"]),
            ),
            (
                "no daemon that answers",
                Err(ClientError::Unreachable {
                    endpoint: String::new(),
                }),
                true,
                exit(&["pause", "closed descriptor", "exit 0"]),
            ),
            (
                "busy",
                Err(ClientError::Daemon(ErrorReply::new(
                    ErrorCode::Busy,
                    "busy",
                ))),
                true,
                exit(&["pause", "stay"]),
            ),
            (
                "a passed deadline",
                Err(ClientError::Deadline { seconds: 10 }),
                true,
                exit(&["pause", "stay"]),
            ),
            (
                "neither paused nor pausing",
                Ok(sample_status(Platform::Macos)),
                true,
                exit(&["pause", "stay"]),
            ),
        ];
        for (name, answer, ends_at_close, expected) in rows {
            let record = Record::default();
            let clock = Arc::new(StandInClock::default());
            let pid = Arc::<ChildPid>::default();
            let spawner = StandInSpawner {
                children: [StandInChild {
                    ends_at_close,
                    ..StandInChild::new(7, &clock, &record)
                }]
                .into(),
                record: Arc::clone(&record),
            };
            let supervisor = Supervisor::start(spawner, Arc::clone(&pid), Arc::clone(&clock));
            let until = Instant::now() + Duration::from_secs(10);
            while pid.get() != Some(7) {
                assert!(Instant::now() < until, "{name}: the stand-in child starts");
                std::thread::sleep(Duration::from_millis(5));
            }
            record.lock().unwrap().clear();
            let control = Pausing {
                places: Arc::default(),
                answer: Mutex::new(Some(answer)),
                record: Arc::clone(&record),
            };
            match quit_and_pause(&control, Some(&supervisor)) {
                Quit::Exit(status) => note(&record, format!("exit {status}")),
                Quit::Stay(sentence) => {
                    assert_eq!(sentence, NOT_PAUSED, "{name}");
                    note(&record, "stay");
                }
            }
            assert_eq!(*record.lock().unwrap(), expected, "{name}");
            supervisor.end();
        }

        let quits = APP_MENU
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .filter(|entry| **entry == Entry::Quit)
            .count();
        assert_eq!(
            quits, 1,
            "one Quit in the app menu, and it is the app's own"
        );
        assert_eq!(QUIT_KEYS, "CmdOrCtrl+Q");
        let code = include_str!("quit.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default();
        assert!(code.contains(".enable_macos_default_menu(false)"));
        assert!(include_str!("main.rs").contains("let builder = quit::installed(builder);"));
    }
}
