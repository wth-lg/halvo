//! The commands of the view, one module each, and the module table.
//!
//! The capability files under src-tauri/capabilities/ let the window main of
//! the app's own origin call these commands and no other window or origin. A
//! capability limits other windows and origins, not script in the same page:
//! any script that runs in the page of the window main can call every command
//! that its capability allows.

pub mod capture;
pub mod names;

use tauri::ipc::Invoke;
use tauri::Runtime;

use names::Names;

/// One line of the module table: the names of a module and its invoke handler.
pub struct Module<R: Runtime> {
    pub names: &'static Names,
    pub handler: fn(Invoke<R>) -> bool,
}

/// The module table: one line for each module of `names::MODULES`, in its order.
pub fn table<R: Runtime>() -> Vec<Module<R>> {
    vec![Module {
        names: &names::CAPTURE,
        handler: capture::handler::<R>,
    }]
}

/// The invoke handler of the app: the handler of the module whose names hold
/// the command; false, which Tauri answers as an unknown command, for no module.
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    let table = table::<R>();
    move |invoke| {
        let command = invoke.message.command();
        match table.iter().find(|m| m.names.commands.contains(&command)) {
            Some(module) => (module.handler)(invoke),
            None => false,
        }
    }
}
