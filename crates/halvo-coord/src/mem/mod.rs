//! Memory implementations of every port of `crate::ports`, for tests (I5.10), and a
//! configuration that reads the record of A-A2.1 (I2.1).

mod config;
pub mod policy;
mod ports;

pub use config::Config;
pub use ports::{Call, Method, Ports, Timed};

#[cfg(test)]
mod tests;
