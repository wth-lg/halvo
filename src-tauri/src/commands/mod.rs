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
