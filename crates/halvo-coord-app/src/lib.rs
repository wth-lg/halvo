//! The coordinator of Halvo's interface run inside the app (contracts I8.1, I8.2, I10): the
//! page's commands of `halvo_coord::types::commands::LIST`, registered for the window `main`
//! alone, the state that reaches the page by one waiting call, the tray's count and the hooks of
//! the app's start, hide and quit. It is generic over the Tauri runtime, so that the app, the
//! shell and the mock runtime register the same commands. What its tests on the mock runtime
//! cannot show: a second window main, a webview whose label is not its window's (that needs
//! Tauri's feature `unstable`), and a real window's events, which the tests hand to the hooks
//! themselves.

mod args;
mod attention;
mod bell;
mod handler;
mod hub;
mod present;
mod window;
mod wired;

pub use attention::Attention;
pub use bell::Bell;
pub use handler::{handler, handler_with, MAIN, REFUSED};
pub use hub::{Agents, Core, Hub, Native, Present, Reply, Waits};
#[cfg(feature = "scripted")]
pub use present::Scripted;
pub use present::{decide, Card, Dialog, DialogPresenter, Outcome, Read, System};
pub use window::{on_window_event, Close, CLOSE};
pub use wired::Wired;
