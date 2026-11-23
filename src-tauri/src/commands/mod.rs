//! A capability limits other windows and origins, not script in the same page:
//! any script that runs in the page of the window main can call every command
//! that its capability allows.

pub mod agents;
pub mod capture;
pub mod control;
pub mod model_key;
pub mod model_limit;
pub mod names;
pub mod workflows;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use tauri::ipc::Invoke;
use tauri::{Runtime, Url};

use crate::app;
use crate::view_state::Status;

use names::Names;

pub trait Registration: Send + Sync {
    fn names(&self) -> &'static Names;

    fn on_status(&self, _status: &Status) {}

    /// Told before a delete of everything goes out, at either door (contracts.md C10.1), off
    /// the calling thread; the caller waits for it at most for STOP_WAIT_MS and then goes on.
    /// None of the five modules implements it today, so a module that holds what a delete
    /// removes stops nothing here unless it does: the agents' runs are held by the command
    /// itself (`agents::RunsHold`), and behind that the delete walk ends every runner that a
    /// mark names.
    fn before_delete_all(&self) {}
}

/// Tells every module of `modules` that a delete of everything is about to go out, each on a
/// thread of its own, and waits for them at most `within`; a module that has not returned by
/// then goes on in its thread.
pub fn tell_before_delete_all(modules: &[&'static dyn Registration], within: Duration) {
    let (done, answered) = std::sync::mpsc::channel();
    let mut asked = 0;
    for &module in modules {
        let done = done.clone();
        let spawned = std::thread::Builder::new()
            .name("halvo-before-delete".to_owned())
            .spawn(move || {
                module.before_delete_all();
                let _ = done.send(());
            });
        if spawned.is_ok() {
            asked += 1;
        }
    }
    drop(done);
    let until = Instant::now() + within;
    for _ in 0..asked {
        let left = until.saturating_duration_since(Instant::now());
        if answered.recv_timeout(left).is_err() {
            break;
        }
    }
}

pub struct Module<R: Runtime> {
    pub registration: &'static dyn Registration,
    pub handler: fn(Invoke<R>) -> bool,
}

impl<R: Runtime> Module<R> {
    pub fn names(&self) -> &'static Names {
        self.registration.names()
    }
}

pub fn table<R: Runtime>() -> Vec<Module<R>> {
    vec![
        Module {
            registration: &capture::Capture,
            handler: capture::handler::<R>,
        },
        Module {
            registration: &agents::Module,
            handler: agents::handler::<R>,
        },
        Module {
            registration: &workflows::Workflows,
            handler: workflows::handler::<R>,
        },
        Module {
            registration: &model_key::Module,
            handler: model_key::handler::<R>,
        },
        Module {
            registration: &model_limit::Module,
            handler: model_limit::handler::<R>,
        },
    ]
}

/// The answer to a call of a module's command that does not come from the
/// app's page in the webview main of the window main.
pub const REFUSED: &str = "this command answers the app's page in its window alone";

/// Whether the caller of `invoke` is the app's page: the webview main of the
/// window main, whose own address is of the app's origin by `app::is_app_url`,
/// and whose request carries no Origin header of another address.
pub fn from_the_app_page<R: Runtime>(invoke: &Invoke<R>) -> bool {
    let webview = invoke.message.webview();
    if webview.label() != "main" || webview.window().label() != "main" {
        return false;
    }
    if !webview.url().is_ok_and(|url| app::is_app_url(&url)) {
        return false;
    }
    match invoke.message.headers().get("Origin") {
        None => true,
        Some(origin) => origin
            .to_str()
            .ok()
            .and_then(|text| Url::parse(text).ok())
            .is_some_and(|url| app::is_app_url(&url)),
    }
}

/// The invoke handler of the app: the handler of the module whose names hold
/// the command, for a call from the app's page; the text `REFUSED` for a call
/// of a module's command from anywhere else, whatever a capability grants;
/// false, which Tauri answers as an unknown command, for no module.
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    let table = table::<R>();
    move |invoke| {
        let command = invoke.message.command();
        match table.iter().find(|m| m.names().commands.contains(&command)) {
            Some(_) if !from_the_app_page(&invoke) => {
                invoke.resolver.reject(REFUSED);
                true
            }
            Some(module) => (module.handler)(invoke),
            None => false,
        }
    }
}
