#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("halvo-agent-client runs on macOS and Windows");
