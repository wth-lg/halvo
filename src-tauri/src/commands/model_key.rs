//! The module model_key (r4 U3.18): the page's three commands of the person's key for a model's host,
//! `model_key_state`, `save_model_key` and `remove_model_key`, which `halvo_coord_app::model_key::ModelKey` answers
//! for the page of the window main alone. Each call goes over an owner stream of the app's own that it asks for at
//! the call and that ends with it, so it holds none of the owner's streams between calls. Behind the tray's Quit, with
//! no data directory and on a system other than a Mac and Windows, each command answers as with no service. No
//! command keeps the key or writes it to a log.

use serde_json::Value;
use tauri::ipc::{Invoke, InvokeBody, InvokeError};
use tauri::{AppHandle, Manager, Runtime};

use super::names::{Names, MODEL_KEY};
use super::Registration;

pub struct Module;

impl Registration for Module {
    fn names(&self) -> &'static Names {
        &MODEL_KEY
    }
}

/// Answers an invoke of a command of `names::MODEL_KEY` on a thread of its own; returns false for any other name.
pub fn handler<R: Runtime>(invoke: Invoke<R>) -> bool {
    let Some(&command) = MODEL_KEY
        .commands
        .iter()
        .find(|c| **c == invoke.message.command())
    else {
        return false;
    };
    let args = match invoke.message.payload() {
        InvokeBody::Json(value) => Some(value.clone()),
        InvokeBody::Raw(_) => None,
    };
    let app = invoke.message.webview().app_handle().clone();
    invoke.resolver.respond_async(async move {
        tauri::async_runtime::spawn_blocking(move || answer(&app, command, args.as_ref()))
            .await
            .map_err(InvokeError::from_error)
    });
    true
}

/// The reply of `command` with `args`: this build's host and the key's calls over a new owner stream of the app's
/// own, or the calls where no service answers behind the tray's Quit and with no data directory.
#[cfg(any(target_os = "macos", windows))]
fn answer<R: Runtime>(app: &AppHandle<R>, command: &str, args: Option<&Value>) -> Value {
    use halvo_coord_app::model_key::{host, ModelKey};
    use std::sync::Arc;
    let key = match app.try_state::<super::capture::AppState>() {
        Some(state) if state.data_dir.is_some() && !crate::app::quitting(app) => {
            let control = Arc::clone(&state.control);
            ModelKey::over(Arc::new(move || {
                control.owner_stream(control.places().make())
            }))
        }
        _ => ModelKey::no_service(),
    };
    serde_json::to_value(key.call(host().as_deref(), command, args)).unwrap_or(Value::Null)
}

/// The reply of `command` on a system with no owner stream: no host, and every save and removal not finished.
#[cfg(not(any(target_os = "macos", windows)))]
fn answer<R: Runtime>(_app: &AppHandle<R>, command: &str, _args: Option<&Value>) -> Value {
    match command {
        "model_key_state" => serde_json::json!({ "host": null }),
        _ => serde_json::json!({ "error": "not_finished" }),
    }
}
