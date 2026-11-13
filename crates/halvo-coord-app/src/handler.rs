use crate::hub::Agents;
use halvo_coord::types::commands::LIST;
use tauri::ipc::{Invoke, InvokeBody, InvokeError};
use tauri::{Manager, Runtime, Webview};

/// The label of the one window whose page the commands answer (I8.1).
pub const MAIN: &str = "main";

/// The rejection of a command that a page of another window calls, whatever a capability
/// grants it.
pub const REFUSED: &str = "this command answers the page of the window main alone";

/// The invoke handler of the commands of `LIST`: it takes a call of one of their names and
/// leaves every other name to the app's next handler. It answers only the page of the window
/// `main`, through the `Agents` that the app manages, and runs each command off the async
/// runtime's threads, since `agents_next` waits; a call from any other window is rejected with
/// `REFUSED` and reaches no coordinator.
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    |invoke: Invoke<R>| {
        let Some(command) = LIST.iter().find(|c| c.name == invoke.message.command()) else {
            return false;
        };
        let webview = invoke.message.webview();
        let agents = webview.try_state::<Agents>().map(|a| a.inner().clone());
        let (true, Some(Agents(core))) = (from_main(&webview), agents) else {
            invoke.resolver.reject(REFUSED);
            return true;
        };
        let args = match invoke.message.payload() {
            InvokeBody::Json(value) => Some(value.clone()),
            InvokeBody::Raw(_) => None,
        };
        let name = command.name;
        invoke.resolver.respond_async(async move {
            tauri::async_runtime::spawn_blocking(move || core.call(name, args.as_ref()))
                .await
                .map_err(InvokeError::from_error)
        });
        true
    }
}

fn from_main<R: Runtime>(webview: &Webview<R>) -> bool {
    webview.label() == MAIN && webview.window().label() == MAIN
}
