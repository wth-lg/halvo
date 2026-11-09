//! Key custody: wrapping the one store key under a per-platform root. Traits and
//! types only. The dev-fallback custody and the per-OS implementations
//! (dev_fallback.rs, macos.rs, windows.rs) are phase P2, added as the sibling
//! files already declared here, so P2 never edits this file.

pub mod dev_fallback;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// The 32-byte store key, in the clear only in memory and zeroed on drop. It is
/// never serialized; only its wrapped form leaves memory.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct StoreKey([u8; 32]);

impl StoreKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw key bytes, for the store to open the database. Keep the borrow
    /// short; the key zeroes on drop.
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

/// The store key in its wrapped (encrypted) form, as written to disk. Opaque
/// bytes whose meaning is the wrapping custody's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKey(pub Vec<u8>);

/// Which custody wrapped the store key. The release privacy check rejects
/// [`CustodyKind::DevFallback`]; the signed-build paths are the real roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyKind {
    /// Login-keychain (macOS) or DPAPI (Windows) fallback for unsigned dev
    /// builds. Not acceptable for release.
    DevFallback,
    /// macOS Secure Enclave.
    SecureEnclave,
    /// Windows DPAPI user scope.
    Dpapi,
}

/// Wrap and unwrap the store key under one per-platform root. The owner's process
/// unwraps with no prompt; a second account or an admin cannot.
pub trait KeyCustody {
    fn kind(&self) -> CustodyKind;
    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError>;
    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CustodyError {
    #[error("the platform root key is not available: {0}")]
    RootUnavailable(String),
    #[error("wrapping or unwrapping the store key failed: {0}")]
    Crypto(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}
