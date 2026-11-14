//! A capability limits other windows and origins, not script in the same page:
//! any script that runs in the page of the window main can call every command
//! that its capability allows.

pub mod capture;
pub mod names;

use tauri::ipc::Invoke;
use tauri::{Runtime, Url};

use crate::app;

use names::Names;

pub struct Module<R: Runtime> {
    pub names: &'static Names,
    pub handler: fn(Invoke<R>) -> bool,
}

pub fn table<R: Runtime>() -> Vec<Module<R>> {
    vec![Module {
        names: &names::CAPTURE,
        handler: capture::handler::<R>,
    }]
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
        match table.iter().find(|m| m.names.commands.contains(&command)) {
            Some(_) if !from_the_app_page(&invoke) => {
                invoke.resolver.reject(REFUSED);
                true
            }
            Some(module) => (module.handler)(invoke),
            None => false,
        }
    }
}
