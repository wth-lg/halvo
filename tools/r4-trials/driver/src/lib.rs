pub mod card;
mod native;
pub mod options;

use r4_trial_app::Refusal;
use tauri::{Builder, Wry};

use crate::options::Options;

/// The trial's builder: `r4_trial_app::builder` with its refusals, the dialog plugin, the
/// WebDriver server of the build's candidate on `--webdriver-port`, and, with `--dialog`, the
/// plugin that shows the dialog. Start it with `r4_trial_app::run`, as the trial app says.
pub fn builder(options: &Options) -> Result<Builder<Wry>, Refusal> {
    let builder = r4_trial_app::builder(options.trial())?.plugin(tauri_plugin_dialog::init());
    let builder = with_candidate(builder, options.webdriver_port);
    Ok(match options.dialog {
        Some(form) => builder.plugin(native::dialogs(form, options.shows)),
        None => builder,
    })
}

#[cfg(feature = "webdriver")]
fn with_candidate(builder: Builder<Wry>, port: Option<u16>) -> Builder<Wry> {
    match port {
        Some(port) => builder.plugin(tauri_plugin_webdriver::init_with_port(port)),
        None => builder,
    }
}

#[cfg(feature = "wdio")]
fn with_candidate(builder: Builder<Wry>, port: Option<u16>) -> Builder<Wry> {
    match port {
        Some(port) => builder.plugin(tauri_plugin_wdio_webdriver::init_with_port(port)),
        None => builder,
    }
}

#[cfg(not(any(feature = "webdriver", feature = "wdio")))]
fn with_candidate(builder: Builder<Wry>, _port: Option<u16>) -> Builder<Wry> {
    builder
}
