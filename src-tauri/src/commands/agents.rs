//! The module agents (contracts I10.1, I8.1): the page's commands of
//! `halvo_coord::types::commands::LIST`, which the handler of halvo-coord-app answers for the
//! page of the window main alone, through the `Agents` that the app manages.

use std::sync::Arc;
use std::time::Duration;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::commands::LIST;
use halvo_coord::types::{AgentId, Cause, ItemId, RunId, Snapshot};
use halvo_coord::words;
use halvo_coord_app::{Agents, Core, Reply};
use serde_json::Value;
use tauri::ipc::Invoke;
use tauri::{Manager, Runtime};

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

/// The steps of the app's start that go with its `Core`, which the app takes on the thread of its
/// first tick.
pub type Start = Box<dyn FnOnce() + Send>;

/// The one place where the app makes its `Core`, and the steps of its start. On a Mac with a data
/// directory it is the hub over the product's ports (`halvo_coord_app::product`), whose count of
/// agents that need the person goes to `tray`: its owner streams and relay go through
/// `control`, its runs live under the data directory, and its start's steps are
/// `halvo_coord_app::product::start`. Elsewhere, and on Windows until the capture run's win-5 cut
/// 2, it is `NoService`, and nothing calls `tray`.
pub fn core(
    tray: Box<dyn Fn(u32) + Send + Sync>,
    control: &Arc<dyn Control>,
    data_dir: Option<&DataDir>,
) -> (Agents, Start) {
    #[cfg(target_os = "macos")]
    if let Some(dir) = data_dir {
        let hub = product_hub(tray, control, dir);
        let started = Arc::clone(&hub);
        return (
            Agents(hub),
            Box::new(move || halvo_coord_app::product::start(&started)),
        );
    }
    let _ = (tray, control, data_dir);
    (Agents(Arc::new(NoService)), Box::new(|| {}))
}

/// The hub over the product's ports: the runner and the browser stand beside the app's own
/// executable, absolute, as the supervisor library takes them.
#[cfg(target_os = "macos")]
fn product_hub(
    tray: Box<dyn Fn(u32) + Send + Sync>,
    control: &Arc<dyn Control>,
    dir: &DataDir,
) -> Arc<halvo_coord_app::product::ProductHub> {
    use halvo_coord_app::product::{Calls, Places};
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| dir.path().to_path_buf());
    let places = Places {
        data_dir: dir.path().to_path_buf(),
        runner: beside.join("halvo-runner"),
        browser: beside.join("halvo-browser"),
        model: None,
    };
    let (streams, relayed) = (Arc::clone(control), Arc::clone(control));
    let calls = Calls {
        owner_stream: Arc::new(move || streams.owner_stream(streams.places().make())),
        relay: Arc::new(move |request: &[u8]| relayed.relay(relayed.places().make(), request)),
    };
    halvo_coord_app::product::hub(calls, places, tray, Box::new(NoDialog))
}

/// The presenter of the product's hub: no confirmation reaches a native dialog yet, because the
/// product's library rejects each effect that needs a yes (a named limit of U5.4a).
#[cfg(target_os = "macos")]
struct NoDialog;

#[cfg(target_os = "macos")]
impl halvo_coord_app::Present for NoDialog {
    fn present(&self, _dialog: halvo_coord_app::Native) {}
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

    fn on_screen(&self, _item: &ItemId) {}

    fn answer_native(&self, _item: &ItemId, _answer: ConfirmAnswer) -> Result<(), Cause> {
        Err(Cause::NoService)
    }
}
