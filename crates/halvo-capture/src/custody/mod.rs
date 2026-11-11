//! Key custody: wrapping the one store key under a per-platform root.

pub mod dev_fallback;
#[cfg(any(target_os = "windows", test))]
pub(crate) mod file_root;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// The 32-byte store key, in the clear only in memory and zeroed on drop. It is
/// never serialized; only its wrapped form leaves memory.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct StoreKey([u8; 32]);

pub(crate) const fn wiped_on_drop<T: ZeroizeOnDrop>() {}

const _: () = wiped_on_drop::<StoreKey>();

impl StoreKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// A fresh random store key, generated in place so no unscrubbed copy of
    /// its bytes is left behind.
    pub fn random() -> Result<Self, CustodyError> {
        let mut key = Self([0u8; 32]);
        dev_fallback::fill_random(&mut key.0)?;
        Ok(key)
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

/// The non-secret per-store locator of the platform root that wraps the store
/// key, so two stores never share one root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RootId(Uuid);

impl RootId {
    pub fn fresh() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }
}

impl fmt::Display for RootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.simple())
    }
}

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

    /// The per-store root locator, written (non-secret) into the store-key file
    /// so a later open finds the same platform root.
    fn root_id(&self) -> RootId;

    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError>;

    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError>;

    /// Destroy the platform root so no retained wrapped store key can ever be
    /// unwrapped again. Succeeds when the root is already gone.
    fn destroy_root(&self) -> Result<(), CustodyError>;
}

pub(crate) fn opens_under_an_all_zero_root(root_id: RootId, wrapped: &WrappedKey) -> bool {
    dev_fallback::aead_open(&[0u8; 32], &wrapped.0, root_id.as_bytes()).is_ok()
}

/// The custody that wrapped a store key, rebuilt from the kind and root id the
/// store-key file records.
pub fn custody_for(
    kind: CustodyKind,
    root_id: RootId,
) -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
    match kind {
        CustodyKind::DevFallback => Ok(Box::new(dev_fallback::DevFallbackCustody::new(root_id))),
        CustodyKind::SecureEnclave => Err(CustodyError::CustodyKindUnsupported(kind)),
        #[cfg(target_os = "windows")]
        CustodyKind::Dpapi => Ok(Box::new(windows::DpapiCustody::new(root_id))),
        #[cfg(not(target_os = "windows"))]
        CustodyKind::Dpapi => Err(CustodyError::CustodyKindUnsupported(kind)),
    }
}

/// Why a custody call failed, classified where the failure happens so that the
/// owner can match every state: a state that can clear is [`RootLocked`], and
/// [`Crypto`] is a permanent failure only.
///
/// [`RootLocked`]: CustodyError::RootLocked
/// [`Crypto`]: CustodyError::Crypto
#[derive(Debug, thiserror::Error)]
pub enum CustodyError {
    /// No root exists for this store, also an empty or all-zero root file.
    #[error("the root of this store is missing")]
    RootMissing,
    /// The root exists and does not hold 32 bytes.
    #[error("the root of this store does not hold 32 bytes")]
    RootCorrupt,
    /// The root exists and this process may not read or destroy it.
    #[error("this process may not read or destroy the root of this store")]
    RootDenied,
    /// A platform state that can clear, with the platform's code.
    #[error("the root of this store is locked for now (code {0:#x})")]
    RootLocked(i32),
    /// This build has no custody of the kind that the store-key file names.
    #[error("this build has no {0:?} custody")]
    CustodyKindUnsupported(CustodyKind),
    /// The wrapped store key does not authenticate under the root.
    #[error("the wrapped store key does not open under the root")]
    WrapAuthFailed,
    /// The platform refused the call with a code that no other variant names.
    #[error("the platform refused the custody call (code {})", os_code_text(*.0))]
    Os(Option<i32>),
    /// A permanent failure of the wrap or the unwrap.
    #[error("wrapping or unwrapping the store key failed: {0}")]
    Crypto(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}

fn os_code_text(code: Option<i32>) -> String {
    code.map_or_else(|| "none".to_string(), |code| code.to_string())
}
