//! The module agents (contracts I10.1, I8.1): the page's commands of
//! `halvo_coord::types::commands::LIST`, which the handler of halvo-coord-app answers for the
//! page of the window main alone, through the `Agents` that the app manages.

use std::sync::Arc;
use std::time::Duration;

use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::commands::LIST;
use halvo_coord::types::{Cause, ItemId, Snapshot};
use halvo_coord::words;
use halvo_coord_app::{Agents, Core, Reply};
use serde_json::Value;
use tauri::ipc::Invoke;
use tauri::Runtime;

use super::names::{Names, AGENTS};
use super::Registration;

pub struct Module;

impl Registration for Module {
    fn names(&self) -> &'static Names {
        &AGENTS
    }
}

pub fn handler<R: Runtime>(invoke: Invoke<R>) -> bool {
    halvo_coord_app::handler::<R>()(invoke)
}

/// The one place where the app makes its `Core`: until the coordinator's ports exist (U5.4)
/// it is `NoService`.
pub fn core() -> Agents {
    Agents(Arc::new(NoService))
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

    fn quit(&self, _budget: Option<Duration>) {}

    fn on_screen(&self, _item: &ItemId) {}

    fn answer_native(&self, _item: &ItemId, _answer: ConfirmAnswer) -> Result<(), Cause> {
        Err(Cause::NoService)
    }
}
