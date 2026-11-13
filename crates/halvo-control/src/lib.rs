#![deny(clippy::undocumented_unsafe_blocks)]

pub mod client;
#[cfg(target_os = "macos")]
pub mod client_unix;
pub mod ctl_text;
#[cfg(target_os = "macos")]
pub mod instance_lock;
pub mod model;
pub mod onboarding;
pub mod paths;
pub mod proto;
#[cfg(feature = "r2")]
#[rustfmt::skip]
pub mod r2;
pub mod service;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod texts;
pub mod wire;

pub const POLL_MS: u64 = 3_000;

pub const INDICATOR_LEASE_MS: u64 = 10_000;

pub const STOP_WAIT_MS: u64 = 20_000;

pub const SESSION_END_BUDGET_MS: u64 = 3_000;

/// How long the client waits after a whole reply frame for what follows it, and never past
/// the request's deadline (ruling PL-8 part (1)(b)).
pub const REPLY_END_WAIT_MS: u64 = 250;

/// The exit statuses of halvo-capture that a starter, ctl and the gate read (contracts.md 0.4).
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
    pub const USAGE: i32 = 64;
    pub const UNREACHABLE: i32 = 69;
    pub const BUSY: i32 = 75;
}
