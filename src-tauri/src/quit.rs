use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};

use halvo_control::client::{ClientError, PeerRejection};
use halvo_control::proto::{Capture, DaemonStatus};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Builder, Manager, Runtime};

use crate::commands::capture::AppState;
use crate::commands::control::Control;
use crate::launch::Exit;
use crate::tray::{QuitNotice, QuitWith};

/// The app's daemon child as a quit ends it.
pub trait Children: Send + Sync {
    /// Whether a child of the app lives now.
    fn lives(&self) -> bool;
    /// Whether the app has ended its child for a quit; no child starts after it.
    fn ended(&self) -> bool;
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

/// The sentence of a quit that did not exit, because the app's own child took no connection or
/// gave no reply within the Pause's deadline: the app ended that child, and the next Quit exits.
pub const NOT_ANSWERED: &str =
    "Halvo's capture service did not answer the pause, so Halvo ended it. Choose Quit again to quit.";

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

/// Where the answer to a quit's Pause stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reached {
    /// A daemon confirmed: its status reads paused or pausing.
    Paused,
    /// No daemon was reached. None answers, or the system has no control channel. Or an error
    /// stands before any daemon: the client's own check refused the run directory or the
    /// socket entry, or an Io error came from the connect. Such an error carries a payload: the
    /// app's unresolved endpoint, a cause at the data folder, or the mark of the connect on an
    /// error while no daemon holds the directory. An error of the exchange carries none.
    NoDaemon,
    /// A daemon holds the directory and took no connection, gave no reply within the
    /// deadline, or broke the connection before its reply.
    NoReply,
    /// A daemon answered otherwise, or the server is not the app's own: busy, an error reply,
    /// a status that reads neither paused nor pausing, a reply that cannot be read, a server
    /// of another user or another process than the app's child.
    Answered,
}

/// Where `answer`, the answer to a quit's Pause, stands.
pub fn reached(answer: &Result<DaemonStatus, ClientError>) -> Reached {
    match answer {
        Ok(status) if matches!(status.fields().capture, Capture::Paused | Capture::Pausing) => {
            Reached::Paused
        }
        Ok(_) => Reached::Answered,
        Err(ClientError::Unreachable { .. } | ClientError::Unsupported) => Reached::NoDaemon,
        Err(ClientError::Io(error)) if error.get_ref().is_some() => Reached::NoDaemon,
        Err(ClientError::PeerRejected(
            PeerRejection::SocketDirNotPrivate
            | PeerRejection::SocketDirOfAnotherUser
            | PeerRejection::NotASocket
            | PeerRejection::SocketOfAnotherUser,
        )) => Reached::NoDaemon,
        Err(
            ClientError::Io(_) | ClientError::Deadline { .. } | ClientError::DataDirInUse { .. },
        ) => Reached::NoReply,
        Err(_) => Reached::Answered,
    }
}

/// The steps of the app's one quit (contracts.md C14 item 5): Pause with the client's Pause
/// deadline. On paused or pausing, where no daemon is reached, and at a press after the app has
/// ended its own child, it ends the child and exits with status 0. Where the app's own child
/// took no connection or gave no reply, it ends that child and stays with `NOT_ANSWERED`, so
/// that the next press exits. On any other answer it stays with `NOT_PAUSED`.
pub fn quit_and_pause(control: &dyn Control, children: Option<&dyn Children>) -> Quit {
    let ended_before = children.is_some_and(|children| children.ended());
    let answer = control.pause(control.places().make());
    let own_child = children.filter(|children| children.lives());
    match (reached(&answer), own_child) {
        _ if ended_before => {}
        (Reached::Paused | Reached::NoDaemon, _) => {}
        (Reached::NoReply, Some(child)) => {
            child.end();
            return Quit::Stay(NOT_ANSWERED);
        }
        (Reached::NoReply | Reached::Answered, _) => return Quit::Stay(NOT_PAUSED),
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
    use crate::commands::control::{ChildPid, Gated, Made, Places, Unresolved};
    use crate::daemon_child::stand_in::{note, Record, StandInChild, StandInClock, StandInSpawner};
    use crate::daemon_child::Supervisor;

    type Row = (
        &'static str,
        Vec<Result<DaemonStatus, ClientError>>,
        Option<bool>,
        Vec<String>,
    );

    struct Pausing {
        places: Arc<Places>,
        answers: Mutex<Vec<Result<DaemonStatus, ClientError>>>,
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
            let mut answers = self.answers.lock().unwrap();
            assert!(!answers.is_empty(), "one Pause for each press");
            answers.remove(0)
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
        let unresolved = Gated::new(Unresolved);
        let rows: Vec<Row> = vec![
            (
                "paused",
                vec![Ok(status(Capture::Paused, CaptureState::Paused))],
                Some(true),
                exit(&["pause", "closed descriptor", "exit 0"]),
            ),
            (
                "pausing, and a child that does not end",
                vec![Ok(status(Capture::Pausing, CaptureState::Paused))],
                Some(false),
                exit(&["pause", "closed descriptor", "kill after 20 s", "exit 0"]),
            ),
            (
                "no daemon that answers",
                vec![Err(ClientError::Unreachable {
                    endpoint: String::new(),
                })],
                Some(true),
                exit(&["pause", "closed descriptor", "exit 0"]),
            ),
            (
                "busy",
                vec![Err(ClientError::Daemon(ErrorReply::new(
                    ErrorCode::Busy,
                    "busy",
                )))],
                Some(true),
                exit(&["pause", "stay, not paused"]),
            ),
            (
                "a passed deadline of a daemon that is not the app's child",
                vec![Err(ClientError::Deadline { seconds: 10 })],
                None,
                exit(&["pause", "stay, not paused"]),
            ),
            (
                "neither paused nor pausing",
                vec![Ok(sample_status(Platform::Macos))],
                Some(true),
                exit(&["pause", "stay, not paused"]),
            ),
            (
                "an endpoint that the app cannot resolve: an error before any daemon",
                vec![unresolved.pause(unresolved.places().make())],
                Some(true),
                exit(&["pause", "closed descriptor", "exit 0"]),
            ),
            (
                "the app's child gives no reply in the deadline: ended, and the next press exits",
                vec![
                    Err(ClientError::Deadline { seconds: 10 }),
                    Err(ClientError::Deadline { seconds: 10 }),
                ],
                Some(false),
                exit(&[
                    "pause",
                    "closed descriptor",
                    "kill after 20 s",
                    "stay, not answered",
                    "pause",
                    "exit 0",
                ]),
            ),
        ];
        for (name, answers, child, expected) in rows {
            let record = Record::default();
            let clock = Arc::new(StandInClock::default());
            let pid = Arc::<ChildPid>::default();
            let spawner = StandInSpawner {
                children: child
                    .map(|ends_at_close| StandInChild {
                        ends_at_close,
                        ..StandInChild::new(7, &clock, &record)
                    })
                    .into_iter()
                    .collect(),
                record: Arc::clone(&record),
            };
            let supervisor = Supervisor::start(spawner, Arc::clone(&pid), Arc::clone(&clock));
            let until = Instant::now() + Duration::from_secs(10);
            while child.is_some() && pid.get() != Some(7) {
                assert!(Instant::now() < until, "{name}: the stand-in child starts");
                std::thread::sleep(Duration::from_millis(5));
            }
            record.lock().unwrap().clear();
            let presses = answers.len();
            let control = Pausing {
                places: Arc::default(),
                answers: Mutex::new(answers),
                record: Arc::clone(&record),
            };
            for _ in 0..presses {
                match quit_and_pause(&control, Some(&supervisor)) {
                    Quit::Exit(status) => {
                        note(&record, format!("exit {status}"));
                        break;
                    }
                    Quit::Stay(NOT_PAUSED) => note(&record, "stay, not paused"),
                    Quit::Stay(NOT_ANSWERED) => note(&record, "stay, not answered"),
                    Quit::Stay(other) => panic!("{name}: a stay with {other}"),
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
        let source = |name: &str| {
            std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src")
                    .join(name),
            )
            .unwrap_or_else(|e| panic!("src/{name} reads: {e}"))
        };
        let quit = source("quit.rs");
        let code = quit.split("#[cfg(test)]").next().unwrap_or_default();
        assert!(code.contains(".enable_macos_default_menu(false)"));
        assert!(source("main.rs").contains("let builder = quit::installed(builder);"));
    }
}
