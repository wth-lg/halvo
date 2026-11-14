#![forbid(unsafe_code)]
//! The gate host of the agent-setup run (A7.6): this run's test host, which opens the store behind the handlers of
//! halvo-agent-store and sends their bodies as the product's parties send them. It makes one approved workflow per
//! seed category and its agent through `create_agent` (A14.5 `configs`), and binds a seed's slots with
//! `set_bindings` and its origins with `add_origin`. It is no product binary and has no packaging file.
//!
//! The store is the memory store of halvo-agent-store while the derived store has not landed (requirements W2 and
//! W3 of plan.md); the crate builds with the feature `memory-store` alone until then, and the count line of
//! `configs` names the store with `store=memory`.
//!
//! The check scripts/r3/ci.d/20-r3-policy.sh starts this crate's binary and every test binary of this crate,
//! halvo-policy and halvo-agent-store itself, from the paths in cargo's messages of a build that compiles nothing. It
//! holds two lists: 19 tests, each run by its name alone in its own target's binary, and the 25 test targets of the
//! three crates, each of which must have exactly one test binary. It does not hold against code of the job that
//! writes a test binary or a tool on the PATH before the check reads them, a test that keeps its name with another
//! body, a required target with its test harness switched off, code that runs before main in a required test file, or
//! a compiler wrapper or a cargo configuration that changes what is built (ruling S-53), nor against a test that
//! stands in no list and is removed from its target, which no list holds without naming every test.

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
