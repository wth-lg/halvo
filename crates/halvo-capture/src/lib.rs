pub mod consent;
pub mod custody;
pub mod daemon;
pub mod event;
pub mod floor;
pub mod secure_input;
pub mod sink;
pub mod source;
pub mod store;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use secure_input::macos as secure_input_macos;

#[cfg(target_os = "windows")]
pub mod windows;
