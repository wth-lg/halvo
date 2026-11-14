use crate::hub::{stand_in, Agents, Reply, Waits, LOOK};
use halvo_coord::types::commands::{Command, LIST};
use halvo_coord::types::{Cause, Limits};
use serde_json::Value;
use std::time::Instant;
use tauri::ipc::{Invoke, InvokeBody, InvokeError};
use tauri::{AppHandle, Manager, Runtime, Webview};

/// The label of the one window whose page the commands answer (I8.1).
pub const MAIN: &str = "main";

/// The rejection of a command that a page of another window calls, whatever a capability
/// grants it.
pub const REFUSED: &str = "this command answers the page of the window main alone";

/// `handler_with` and the waits of the product's limits, whatever limits a hub was built
/// with; a shell with other limits calls `handler_with`.
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    handler_with(Waits::of(&Limits::PRODUCT))
}

/// The invoke handler of the commands of `LIST`: it takes a call of one of their names and
/// leaves every other name to the app's next handler. A call from any window but `main` is
/// rejected with `REFUSED` and reaches no coordinator. From the page of `main` it runs each
/// command off the async runtime's threads, since `agents_next` waits, through the `Agents`
/// that the app manages. While the app manages no `Agents` (ruling RD-74 3.2),
/// `agents_snapshot` answers the stand-in state (`seq` 0, empty lists, the note
/// `note.no_service`) at once; `agents_next` looks for `Agents` every 100 ms for at most
/// `waits.next`, hands the call to it once it is there, and else answers the stand-in; every
/// other command answers `{"error":"no_service"}` where its row of `LIST` names that cause, and
/// `{"error":"unknown_item"}` where it does not (`agents_setup_typed`). A held call that is
/// handed to the hub waits the hub's own wait anew, so a call made just before the app manages
/// `Agents` can be answered up to twice `waits.next` after it was made; before the hand-over
/// no hide rule binds it.
pub fn handler_with<R: Runtime>(
    waits: Waits,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<R>| {
        let Some(command) = LIST.iter().find(|c| c.name == invoke.message.command()) else {
            return false;
        };
        let webview = invoke.message.webview();
        if !from_main(&webview) {
            invoke.resolver.reject(REFUSED);
            return true;
        }
        let args = match invoke.message.payload() {
            InvokeBody::Json(value) => Some(value.clone()),
            InvokeBody::Raw(_) => None,
        };
        let name = command.name;
        if let Some(Agents(core)) = webview.try_state::<Agents>().map(|a| a.inner().clone()) {
            invoke.resolver.respond_async(async move {
                tauri::async_runtime::spawn_blocking(move || core.call(name, args.as_ref()))
                    .await
                    .map_err(InvokeError::from_error)
            });
            return true;
        }
        match name {
            "agents_snapshot" => invoke.resolver.resolve(Reply::Snapshot(stand_in())),
            "agents_next" => {
                let app = webview.app_handle().clone();
                invoke.resolver.respond_async(async move {
                    tauri::async_runtime::spawn_blocking(move || held(&app, waits, args))
                        .await
                        .map_err(InvokeError::from_error)
                });
            }
            _ => invoke.resolver.resolve(Reply::Error(unserved(command))),
        }
        true
    }
}

fn held<R: Runtime>(app: &AppHandle<R>, waits: Waits, args: Option<Value>) -> Reply {
    let deadline = Instant::now() + waits.next;
    loop {
        if let Some(agents) = app.try_state::<Agents>() {
            return agents.0.call("agents_next", args.as_ref());
        }
        let now = Instant::now();
        if now >= deadline {
            return Reply::Snapshot(stand_in());
        }
        std::thread::sleep(LOOK.min(deadline - now));
    }
}

fn unserved(command: &Command) -> Cause {
    if command.errors.contains(&Cause::NoService) {
        Cause::NoService
    } else {
        Cause::UnknownItem
    }
}

fn from_main<R: Runtime>(webview: &Webview<R>) -> bool {
    webview.label() == MAIN && webview.window().label() == MAIN
}
