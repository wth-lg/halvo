//! The module agents (contracts I10.1, I8.1): the page's commands of
//! `halvo_coord::types::commands::LIST`, which the handler of halvo-coord-app answers for the
//! page of the window main alone, through the `Agents` that the app manages.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::commands::LIST;
use halvo_coord::types::{AgentId, Cause, ItemId, RunId, Snapshot};
use halvo_coord::words;
use halvo_coord_app::{Agents, Core, Reply};
use serde_json::Value;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Manager, Runtime};

use super::control::Control;
use super::names::{Names, AGENTS};
use super::Registration;
use halvo_control::paths::DataDir;

pub struct Module;

impl Registration for Module {
    fn names(&self) -> &'static Names {
        &AGENTS
    }
}

/// The handler of halvo-coord-app, but once the tray's Quit has begun: from then on every command
/// of `LIST` gets the fixed reply of `NoService` and reaches no call of the Core, while the quit
/// hook runs and after it has returned.
pub fn handler<R: Runtime>(invoke: Invoke<R>) -> bool {
    let quitting = crate::app::quitting(invoke.message.webview().app_handle());
    match LIST.iter().find(|c| c.name == invoke.message.command()) {
        Some(command) if quitting => {
            invoke.resolver.resolve(NoService.call(command.name, None));
            true
        }
        _ => halvo_coord_app::handler::<R>()(invoke),
    }
}

/// The app's hold on the agents' runs (the coordinator's lines 12397 and 12465 of the capture
/// run): it stands while a delete of everything runs at either door and while the view's store
/// reads deleting or error locked, and is lifted once neither stands. The core's `hold_runs`
/// refuses every start while it stands, the person's, the queue's and a tick's, and stops the
/// runs at work; calls to the core go one at a time, each with what is wanted at its turn.
#[derive(Debug, Default)]
pub struct RunsHold {
    wanted: Mutex<(usize, bool)>,
    applied: Mutex<bool>,
}

impl RunsHold {
    /// A delete of everything begins: the runs are held, and the call returns once the core has
    /// stopped them, within its grace.
    pub fn delete_begins(&self, core: Option<&dyn Core>) {
        self.wanted.lock().unwrap_or_else(PoisonError::into_inner).0 += 1;
        self.apply(core);
    }

    /// A delete of everything ended: the hold is lifted unless another delete runs or the view
    /// still reads deleting or locked.
    pub fn delete_ends(&self, core: Option<&dyn Core>) {
        {
            let mut wanted = self.wanted.lock().unwrap_or_else(PoisonError::into_inner);
            wanted.0 = wanted.0.saturating_sub(1);
        }
        self.apply(core);
    }

    /// Whether `view` bars the agents' runs: its store reads deleting or error locked.
    pub fn bars(view: &crate::view_state::StatusView) -> bool {
        match view {
            crate::view_state::StatusView::Running(running) => {
                running.store.state == halvo_control::proto::StoreState::Deleting
                    || running.store.error == Some(halvo_control::proto::StoreErrorKind::Locked)
            }
            crate::view_state::StatusView::NoStatus(_) => false,
        }
    }

    /// What the newest view reads, `bars` of it: the hold stands while it bars.
    pub fn view_reads(&self, core: Option<&dyn Core>, barred: bool) {
        self.wanted.lock().unwrap_or_else(PoisonError::into_inner).1 = barred;
        self.apply(core);
    }

    fn barred_by_the_view(&self) -> bool {
        self.wanted.lock().unwrap_or_else(PoisonError::into_inner).1
    }

    /// Whether the hold stands as last told to the core.
    #[cfg(test)]
    pub fn stands(&self) -> bool {
        *self.applied.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn apply(&self, core: Option<&dyn Core>) {
        let mut applied = self.applied.lock().unwrap_or_else(PoisonError::into_inner);
        let held = {
            let wanted = self.wanted.lock().unwrap_or_else(PoisonError::into_inner);
            wanted.0 > 0 || wanted.1
        };
        if held != *applied {
            *applied = held;
            if let Some(core) = core {
                core.hold_runs(held);
            }
        }
    }
}

/// What a draw of the app's view tells the agents' hold, from the drawer of `app::build`: the
/// store of the view, read with the app's own state and core.
pub fn hold_by_the_view<R: Runtime>(app: &AppHandle<R>, view: &crate::view_state::StatusView) {
    let Some(state) = app.try_state::<super::capture::AppState>() else {
        return;
    };
    let barred = RunsHold::bars(view);
    if state.runs.barred_by_the_view() == barred {
        return;
    }
    let core = app
        .try_state::<Agents>()
        .map(|agents| Arc::clone(&agents.0));
    let runs = Arc::clone(&state.runs);
    let _ = std::thread::Builder::new()
        .name("halvo-runs-hold".to_owned())
        .spawn(move || runs.view_reads(core.as_deref(), barred));
}

/// The steps of the app's start that go with its `Core`, which the app takes on the thread of its
/// first tick.
pub type Start = Box<dyn FnOnce() + Send>;

/// The one place where the app makes its `Core`, and the steps of its start. On a Mac and on
/// Windows with a data directory it is the hub over the product's ports
/// (`halvo_coord_app::product`), whose count of agents that need the person goes to `tray` and
/// whose confirmations that only a native dialog answers are shown in the system's dialog of `app`
/// (`halvo_coord_app::System`: the alert on a Mac, the task dialog on Windows): its owner
/// streams, relay, run marks and defer go through `control`, its runs live under the data
/// directory, and its start's steps are `halvo_coord_app::product::start`. Elsewhere, and with no
/// data directory, it is `NoService`, and nothing calls `tray`.
pub fn core<R: Runtime>(
    app: &AppHandle<R>,
    tray: Box<dyn Fn(u32) + Send + Sync>,
    control: &Arc<dyn Control>,
    data_dir: Option<&DataDir>,
) -> (Agents, Start) {
    #[cfg(any(target_os = "macos", windows))]
    if let Some(dir) = data_dir {
        let hub = product_hub(app, tray, control, dir);
        let started = Arc::clone(&hub);
        return (
            Agents(hub),
            Box::new(move || halvo_coord_app::product::start(&started)),
        );
    }
    let _ = (app, tray, control, data_dir);
    (Agents(Arc::new(NoService)), Box::new(|| {}))
}

/// The runner of the app: the file halvo-runner with the system's suffix of an executable
/// (halvo-runner.exe on Windows) beside the app's own executable, absolute, as the supervisor
/// library takes it, or in `dir` where the app's path cannot be read.
#[cfg(any(target_os = "macos", windows))]
pub fn runner(dir: &DataDir) -> std::path::PathBuf {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| dir.path().to_path_buf());
    beside.join(format!("halvo-runner{}", std::env::consts::EXE_SUFFIX))
}

/// The hub over the product's ports, whose native confirmations are shown in the system's dialog
/// of `app`, and whose runner is `runner`.
#[cfg(any(target_os = "macos", windows))]
fn product_hub<R: Runtime>(
    app: &AppHandle<R>,
    tray: Box<dyn Fn(u32) + Send + Sync>,
    control: &Arc<dyn Control>,
    dir: &DataDir,
) -> Arc<halvo_coord_app::product::ProductHub> {
    use halvo_coord_app::product::{Calls, Places};
    let places = Places {
        data_dir: dir.path().to_path_buf(),
        runner: runner(dir),
        model: None,
    };
    let (streams, relayed, marked, deferred) = (
        Arc::clone(control),
        Arc::clone(control),
        Arc::clone(control),
        Arc::clone(control),
    );
    let calls = Calls {
        owner_stream: Arc::new(move || streams.owner_stream(streams.places().make())),
        relay: Arc::new(move |request: &[u8]| relayed.relay(relayed.places().make(), request)),
        mark: Arc::new(move |edge| marked.run_mark(marked.places().make(), edge)),
        defer: Arc::new(move |until| deferred.defer(deferred.places().make(), until)),
    };
    let dialog = Arc::new(halvo_coord_app::System::new(app.clone()));
    halvo_coord_app::product::hub(calls, places, tray, dialog)
}

/// The `Core` of the app until U5.4: `agents_snapshot` answers the stand-in state (`seq` 0,
/// empty lists, the note `note.no_service`), `agents_next` answers `{"closed":"yes"}` at once,
/// so that a view waits for nothing and calls again at its next show, every other command
/// answers `{"error":"no_service"}` where its row of `LIST` names that cause and
/// `{"error":"unknown_item"}` where it does not, and every hook does nothing.
pub struct NoService;

fn stand_in() -> Snapshot {
    Snapshot {
        seq: 0,
        rows: Vec::new(),
        items: Vec::new(),
        note: words::text("note.no_service", &[]).ok(),
    }
}

impl Core for NoService {
    fn call(&self, command: &str, _args: Option<&Value>) -> Reply {
        match LIST.iter().find(|c| c.name == command) {
            None => Reply::Error(Cause::Malformed),
            Some(c) if c.name == "agents_snapshot" => Reply::Snapshot(stand_in()),
            Some(c) if c.name == "agents_next" => Reply::Closed,
            Some(c) if c.errors.contains(&Cause::NoService) => Reply::Error(Cause::NoService),
            Some(_) => Reply::Error(Cause::UnknownItem),
        }
    }

    fn shown(&self) {}

    fn hidden(&self) {}

    fn destroyed(&self) {}

    fn tick(&self) {}

    fn quit(&self, _budget: Option<Duration>) -> Vec<(AgentId, RunId)> {
        Vec::new()
    }

    fn hold_runs(&self, _held: bool) -> Vec<(AgentId, RunId)> {
        Vec::new()
    }

    fn on_screen(&self, _item: &ItemId) {}

    fn answer_native(&self, _item: &ItemId, _answer: ConfirmAnswer) -> Result<(), Cause> {
        Err(Cause::NoService)
    }
}
