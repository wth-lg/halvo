//! Halvo's memory cap: one child process under a limit in bytes, its peak, and the
//! model lock that lets one generative model load at a time for one user.

mod lock;

pub use lock::{model_lock_path, take_model_lock, LockError, ModelLock};
