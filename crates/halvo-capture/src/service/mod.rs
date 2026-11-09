//! The background-service seam: install, uninstall and status for the macOS
//! LaunchAgent and the Windows logon task. Traits and types only; the per-OS
//! installers are service/macos.rs (phase P3) and service/windows.rs (phase P4),
//! added as the sibling files already declared here, so those phases never edit
//! this file and never edit each other's.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// Whether the capture service is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Installed,
    NotInstalled,
}

/// Install or remove the capture daemon as a per-user login service. Idempotent:
/// install writes the plist or task only if absent or out of date.
pub trait ServiceInstaller {
    fn install(&self) -> Result<(), ServiceError>;
    fn uninstall(&self) -> Result<(), ServiceError>;
    fn status(&self) -> Result<ServiceStatus, ServiceError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("the service operation failed: {0}")]
    Op(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}
