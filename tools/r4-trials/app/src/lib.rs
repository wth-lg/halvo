mod app;
pub mod control;
mod options;
pub mod pages;
pub mod sync;
#[cfg(test)]
mod testing;
pub mod waits;

pub use options::{check_hosted, default_pages_dir, parse_args, Arm, Options, Refusal, SyncWay};

/// The trial app's builder for `options`. It binds the control port first and refuses with
/// `port` when that fails, then refuses with `not_hosted` where neither `CI` nor `R4_GUEST` is
/// set; both come before any Tauri code runs. The caller adds its own plugins and runs it with a
/// context.
pub fn builder(options: Options) -> Result<tauri::Builder<tauri::Wry>, Refusal> {
    let listener = control::listen(options.control_port).map_err(|e| {
        Refusal::Port(format!(
            "127.0.0.1:{} cannot be bound ({:?})",
            options.control_port.unwrap_or(0),
            e.kind()
        ))
    })?;
    check_hosted(
        std::env::var_os("CI").as_deref(),
        std::env::var_os("R4_GUEST").as_deref(),
    )?;
    Ok(app::make(options, listener))
}

/// Builds and runs `builder` with `context`. The app keeps running when its last window is
/// destroyed (arm `b`) and ends at `quit`, which exits with a code.
pub fn run(
    builder: tauri::Builder<tauri::Wry>,
    context: tauri::Context<tauri::Wry>,
) -> tauri::Result<()> {
    builder.build(context)?.run(|_, event| {
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    Ok(())
}

/// The context of this crate's `tauri.conf.json` and capabilities.
pub fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}
