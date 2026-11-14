//! Key custody: wrapping the one store key under a per-platform root.

pub mod dev_fallback;
pub(crate) mod file_root;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// The 32-byte store key, in the clear only in memory and zeroed on drop. It is
/// never serialized; only its wrapped form leaves memory.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct StoreKey(Box<[u8; 32]>);

pub(crate) const fn wiped_on_drop<T: ZeroizeOnDrop>() {}

const _: () = wiped_on_drop::<StoreKey>();

impl StoreKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Box::new(bytes))
    }

    /// A fresh random store key, generated in place on the heap, so that a
    /// move of the key copies a pointer and no key byte.
    pub fn random() -> Result<Self, CustodyError> {
        let mut key = Self(Box::new([0u8; 32]));
        dev_fallback::fill_random(key.0.as_mut_slice())?;
        Ok(key)
    }

    /// The raw key bytes. Keep the borrow short; the key zeroes on drop.
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

/// The store key in its wrapped (encrypted) form, as written to disk. Opaque
/// bytes whose meaning is the wrapping custody's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKey(pub Vec<u8>);

/// The non-secret per-store locator of the platform root that wraps the store
/// key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RootId(Uuid);

impl RootId {
    /// A fresh id from 16 random bytes; it reads no clock.
    pub fn fresh() -> Result<Self, CustodyError> {
        let mut bytes = [0u8; 16];
        dev_fallback::fill_random(&mut bytes)?;
        Ok(Self(uuid::Builder::from_random_bytes(bytes).into_uuid()))
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }

    /// The id that `text` holds, in its 32-hex form or the hyphenated one.
    pub fn parse(text: &str) -> Option<Self> {
        let hyphenated = text.len() == 36;
        let simple = text.len() == 32;
        let lower = text
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'-'));
        if !(simple || hyphenated) || !lower {
            return None;
        }
        Uuid::try_parse(text).ok().map(Self)
    }
}

impl fmt::Display for RootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.simple())
    }
}

impl Serialize for RootId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RootId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        RootId::parse(&text).ok_or_else(|| serde::de::Error::custom("not a root id"))
    }
}

/// What a wrapped key protects, `[a-z0-9-]{1,32}`. It goes into the
/// associated data of the wrap beside the root id, so a key wrapped under one
/// label does not unwrap under another, and a key file records it as its
/// namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyLabel(&'static str);

impl KeyLabel {
    /// The label of the store's own key file.
    pub const STORE: KeyLabel = KeyLabel("store");

    /// A label, or `None` for a text outside `[a-z0-9-]{1,32}`.
    pub const fn new(text: &'static str) -> Option<Self> {
        if is_label(text.as_bytes()) {
            Some(Self(text))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

pub(crate) const fn is_label(text: &[u8]) -> bool {
    if text.is_empty() || text.len() > 32 {
        return false;
    }
    let mut at = 0;
    while at < text.len() {
        if !matches!(text[at], b'a'..=b'z' | b'0'..=b'9' | b'-') {
            return false;
        }
        at += 1;
    }
    true
}

pub(crate) fn associated_data(root_id: RootId, label: KeyLabel) -> Vec<u8> {
    let mut aad = Vec::with_capacity(16 + label.0.len());
    aad.extend_from_slice(root_id.as_bytes());
    aad.extend_from_slice(label.0.as_bytes());
    aad
}

/// Where the root of a store is: the custody kind and the root id that its key
/// file records, and the roots directory that the caller passes. The store
/// never resolves a per-user path itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustodyLocator {
    pub kind: CustodyKind,
    pub root_id: RootId,
    pub roots: PathBuf,
}

/// Which custody wrapped the store key.
/// [`dev_fallback::reject_fallback_for_release`] rejects every kind that
/// [`CustodyKind::is_acceptable_for_release`] does not accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyKind {
    /// The custody of a build without the feature `secure-enclave`: a root file
    /// under the roots directory in use, protected on macOS by its mode alone
    /// and on Windows by DPAPI. Not acceptable for release.
    DevFallback,
    /// macOS Secure Enclave.
    SecureEnclave,
    /// Windows DPAPI user scope.
    Dpapi,
}

/// Wrap and unwrap keys under one per-store root, and destroy that root.
///
/// What it holds against whom: a second account cannot unwrap. On Windows an
/// administrator who copies the files sees ciphertext, and any process of the
/// owner can unwrap under DPAPI user scope. On macOS without the feature
/// `secure-enclave` the root file is protected by its mode alone, so a process
/// of the owner, or an administrator who reads the owner's files, can unwrap.
pub trait KeyCustody {
    fn kind(&self) -> CustodyKind;

    /// The per-store root locator, written (non-secret) into the store-key file
    /// so a later open finds the same platform root.
    fn root_id(&self) -> RootId;

    /// The roots directory of this custody, or `None` for a custody that keeps
    /// no root file.
    fn roots(&self) -> Option<&Path>;

    /// Wrap `key` under the root, with `label` in the associated data.
    fn wrap_store_key(&self, key: &StoreKey, label: KeyLabel) -> Result<WrappedKey, CustodyError>;

    /// Make the root of a new store. Fails [`CustodyError::RootExists`] when a
    /// root with this id exists or appears while the call runs, and with the
    /// custody's own error when the root of this id cannot be read, so that a
    /// create never takes, replaces or destroys the root of another store.
    fn create_root(&self) -> Result<(), CustodyError>;

    /// Unwrap a key that was wrapped under the root with the same `label`.
    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        label: KeyLabel,
    ) -> Result<StoreKey, CustodyError>;

    /// Destroy the platform root so that no key wrapped under it, of any label,
    /// can be unwrapped again.
    fn destroy_root(&self) -> Result<(), CustodyError>;
}

pub(crate) fn opens_under_an_all_zero_root(
    root_id: RootId,
    label: KeyLabel,
    wrapped: &WrappedKey,
) -> bool {
    dev_fallback::aead_open(&[0u8; 32], &wrapped.0, &associated_data(root_id, label)).is_ok()
}

/// The custody that wrapped a key, rebuilt from the kind and root id that its
/// key file records and the roots directory that the caller passes, or
/// [`CustodyError::CustodyKindUnsupported`] for a kind that this build cannot
/// rebuild.
pub fn custody_for(locator: &CustodyLocator) -> Result<Box<dyn KeyCustody + Send>, CustodyError> {
    match locator.kind {
        CustodyKind::DevFallback => Ok(Box::new(dev_fallback::DevFallbackCustody::new(
            locator.root_id,
            locator.roots.clone(),
        ))),
        CustodyKind::SecureEnclave => Err(CustodyError::CustodyKindUnsupported(locator.kind)),
        #[cfg(target_os = "windows")]
        CustodyKind::Dpapi => Ok(Box::new(windows::DpapiCustody::new(
            locator.root_id,
            locator.roots.clone(),
        ))),
        #[cfg(not(target_os = "windows"))]
        CustodyKind::Dpapi => Err(CustodyError::CustodyKindUnsupported(locator.kind)),
    }
}

/// Why a custody call failed.
#[derive(Debug, thiserror::Error)]
pub enum CustodyError {
    /// No root exists for this store, also an empty or all-zero root file.
    #[error("the root of this store is missing")]
    RootMissing,
    #[error("the root of this store does not hold 32 bytes")]
    RootCorrupt,
    #[error("this process may not read or destroy the root of this store")]
    RootDenied,
    /// This user has no directory for root files: `LOCALAPPDATA` is not set.
    #[error("this user has no directory for root files")]
    NoRootDirectory,
    /// A platform state that can clear, with the platform's code: a DPAPI
    /// failure other than data that does not decode.
    #[error("the root of this store is locked for now (code {0:#x})")]
    RootLocked(i32),
    #[error("this build has no {0:?} custody")]
    CustodyKindUnsupported(CustodyKind),
    /// A root with this id exists, or appeared while a create wrote its own; a
    /// create takes only a root that it makes and never replaces one.
    #[error("a root with this id exists already")]
    RootExists,
    /// The wrapped store key does not authenticate under the root.
    #[error("the wrapped store key does not open under the root")]
    WrapAuthFailed,
    #[error("the platform refused the custody call (code {})", os_code_text(*.0))]
    Os(Option<i32>),
    /// A failure of the wrap or the unwrap that no other variant names, among
    /// them DPAPI data that does not decode.
    #[error("wrapping or unwrapping the store key failed: {0}")]
    Crypto(String),
    #[error("{0} is not implemented in this phase")]
    NotImplemented(&'static str),
}

fn os_code_text(code: Option<i32>) -> String {
    code.map_or_else(|| "none".to_string(), |code| code.to_string())
}
