#![deny(clippy::undocumented_unsafe_blocks)]

pub mod model;
pub mod onboarding;
pub mod paths;
pub mod proto;
pub mod service;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod wire;
