//! The coordinator of Halvo's interface run inside the app (contracts I8.1, I8.2, I10): the
//! page's commands of `halvo_coord::types::commands::LIST`, registered for the window `main`
//! alone, the state that reaches the page by one waiting call, the tray's count and the hooks of
//! the app's start, hide and quit. It is generic over the Tauri runtime, so that the app, the
//! shell and the mock runtime register the same commands.

mod args;
mod attention;
mod bell;
mod handler;
mod hub;
mod window;
mod wired;

pub use attention::Attention;
pub use bell::Bell;
pub use handler::{handler, MAIN, REFUSED};
pub use hub::{Agents, Core, Hub, Native, Present, Reply, Waits};
pub use window::{on_window_event, Close, CLOSE};
pub use wired::Wired;
