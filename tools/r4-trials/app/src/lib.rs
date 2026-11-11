pub mod control;
mod options;

pub use options::{check_hosted, default_pages_dir, parse_args, Arm, Options, Refusal, SyncWay};

/// The trial app's builder for `options`, refused with `not_hosted` where neither `CI` nor
/// `R4_GUEST` is set. The caller adds its own plugins and runs it with a context.
pub fn builder(_options: Options) -> Result<tauri::Builder<tauri::Wry>, Refusal> {
    check_hosted(
        std::env::var_os("CI").as_deref(),
        std::env::var_os("R4_GUEST").as_deref(),
    )?;
    Ok(tauri::Builder::default())
}

/// The context of this crate's `tauri.conf.json` and capabilities.
pub fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}
