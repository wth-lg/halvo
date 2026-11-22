//! The feature `client`, on by default, holds the client of the capture service, which can pause the capture and
//! delete everything, with every module and item that it speaks or names. Without it the crate holds `paths`, the
//! data directory, and the four modules beside it that name no client (`model`, `service`, `marks` and
//! `instance_lock`): the part that halvo-runner, the program of a person's agents, takes with
//! `default-features = false`, so that its tree holds no control client (scripts/r1/ci.d/21-tree-bans.sh).

#![deny(clippy::undocumented_unsafe_blocks)]

#[cfg(all(windows, feature = "client"))]
pub mod child_windows;
#[cfg(feature = "client")]
pub mod client;
#[cfg(all(target_os = "macos", feature = "client"))]
pub mod client_unix;
#[cfg(all(windows, feature = "client"))]
pub mod client_windows;
#[cfg(feature = "client")]
pub mod ctl_text;
#[cfg(target_os = "macos")]
pub mod instance_lock;
pub mod marks;
// The macro `one_spelling` of `model` serves `onboarding` alone, which the feature `client` holds.
#[cfg_attr(not(feature = "client"), allow(unused_macros, unused_imports))]
pub mod model;
#[cfg(feature = "client")]
pub mod onboarding;
pub mod paths;
#[cfg(feature = "client")]
pub mod proto;
#[cfg(all(feature = "client", feature = "r2"))]
#[rustfmt::skip]
pub mod r2;
pub mod service;
#[cfg(all(feature = "client", any(test, feature = "test-support")))]
pub mod testing;
#[cfg(feature = "client")]
pub mod texts;
#[cfg(feature = "client")]
pub mod wire;

#[cfg(feature = "client")]
pub const POLL_MS: u64 = 3_000;

#[cfg(feature = "client")]
pub const INDICATOR_LEASE_MS: u64 = 10_000;

#[cfg(feature = "client")]
pub const STOP_WAIT_MS: u64 = 20_000;

#[cfg(feature = "client")]
pub const SESSION_END_BUDGET_MS: u64 = 3_000;

/// How long the client waits after a whole reply frame for what follows it, and never past
/// the request's deadline (ruling PL-8 part (1)(b)).
#[cfg(feature = "client")]
pub const REPLY_END_WAIT_MS: u64 = 250;

/// The exit statuses of halvo-capture that a starter, ctl and the gate read (contracts.md 0.4).
#[cfg(feature = "client")]
pub mod exit {
    pub const CLEAN: i32 = 0;
    pub const ERROR: i32 = 1;
    pub const FINAL_FLUSH_FAILED: i32 = ERROR;
    pub const TIMED_RUN_INTERRUPTED: i32 = 2;
    pub const TIMED_RUN_SOURCE_FAILED: i32 = 3;
    pub const RESTART: i32 = 4;
    /// A service run ended itself because a source did not stop when a stop asked for
    /// capture to be off.
    pub const SOURCE_NOT_STOPPED: i32 = 5;
    /// A Windows service run whose first step did not take the program out of Windows Error
    /// Reporting, before any read of the key or of the store; the app starts no child again by
    /// itself after it.
    pub const FAULT_REPORTS_ON: i32 = 6;
    pub const USAGE: i32 = 64;
    /// The offline delete could not use its data directory, before any removal: an entry on its
    /// path that leads nowhere or is no directory, the directory or a folder on its path that
    /// cannot be read, or an instance lock that it could not open for any cause but a holder.
    /// Standard output holds `data_dir=` alone and nothing was removed (contracts.md C3.3).
    pub const DATA_DIR_UNUSABLE: i32 = 66;
    pub const UNREACHABLE: i32 = 69;
    pub const BUSY: i32 = 75;
}
