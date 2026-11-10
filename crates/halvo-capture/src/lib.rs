//! halvo-capture: the always-on capture pipeline for macOS and Windows.
//!
//! This crate is the shared skeleton every R1 phase extends. The cross-platform
//! types and traits are the modules declared here. Per-OS backends live under
//! [`macos`] and [`windows`] behind `cfg`, and the segment store, key custody,
//! consent view and background service are filled in as sibling files inside
//! their own modules, so a later phase adds files without editing this file.

pub mod consent;
pub mod custody;
pub mod daemon;
pub mod event;
pub mod secure_input;
pub mod service;
pub mod sink;
pub mod source;
pub mod store;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub mod secure_input_macos;

#[cfg(target_os = "windows")]
pub mod windows;

/// Scratch line for the failing proof of the hook scan.
pub const SCRATCH_HOOK_PROBE: &str = "SetWindowsHookExW(WH_KEYBOARD_LL, None, None, 0)";
