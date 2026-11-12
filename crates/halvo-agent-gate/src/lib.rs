#![forbid(unsafe_code)]
//! The gate host of the agent-setup run (A7.6): this run's test host, which opens the store behind the handlers of
//! halvo-agent-store and sends their bodies as the product's parties send them. It makes one approved workflow per
//! seed category and its agent through `create_agent` (A14.5 `configs`), and binds a seed's slots with
//! `set_bindings` and its origins with `add_origin`. It is no product binary and has no packaging file.
//!
//! The store is the memory store of halvo-agent-store while the derived store has not landed (requirements W2 and
//! W3 of plan.md); the crate builds with the feature `memory-store` alone until then, and every line that it
//! prints names the store with `store=memory`.

#[cfg(not(feature = "memory-store"))]
compile_error!(
    "halvo-agent-gate builds with the feature memory-store until the derived store lands (W2, W3)"
);

pub mod configs;
mod host;
pub mod seed;

pub use host::{GateHost, Sent};

/// The name of the store behind the gate host, as the gate's lines print it after `store=`.
pub const STORE: &str = "memory";
