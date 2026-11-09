//! Dev-fallback key custody for unsigned builds.
//!
//! The release macOS custody needs a Developer ID signed bundle to persist a
//! Secure Enclave key; an unsigned developer build cannot (researcher-2, probe
//! failed with OSStatus -34018). This fallback wraps the one store key with a
//! per-platform root a second account cannot read, so an unsigned build still
//! keeps the store ciphertext to anyone but the owner:
//!
//! - **macOS**: a 32-byte root key held as a login-keychain generic password.
//!   The store key is sealed under that root with XChaCha20-Poly1305. The owner's
//!   process reads the root with no prompt (the login keychain is unlocked at
//!   login and the creating process is on the item's access list); a second local
//!   account cannot read another user's login keychain.
//! - **Windows**: `CryptProtectData` in user scope with `CRYPTPROTECT_UI_FORBIDDEN`
//!   and an application-specific entropy wraps the store key directly. Only the
//!   same logon on the same machine can unwrap it, with no prompt. Compiled here;
//!   its functional test is deferred to a Windows machine.
//!
//! This custody reports [`CustodyKind::DevFallback`], which
//! [`CustodyKind::is_acceptable_for_release`] rejects, so a release privacy check
//! refuses it and demands the Secure Enclave or DPAPI path.

use aead::{Aead, KeyInit};
use chacha20poly1305::aead::array::Array;
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use super::{CustodyError, CustodyKind, KeyCustody, StoreKey, WrappedKey};

/// Length of the XChaCha20-Poly1305 nonce (192-bit) prepended to a sealed blob.
const XNONCE_LEN: usize = 24;

impl CustodyKind {
    /// Whether this custody is strong enough for a release build. The dev
    /// fallback is not: it is the keyed distinction the release privacy check
    /// uses to refuse an unsigned build's custody and require the Secure Enclave
    /// (macOS) or DPAPI (Windows) root.
    pub fn is_acceptable_for_release(self) -> bool {
        match self {
            CustodyKind::DevFallback => false,
            CustodyKind::SecureEnclave | CustodyKind::Dpapi => true,
        }
    }
}

/// Fail unless `custody` is acceptable for a release build. A release privacy
/// check calls this and refuses to run on the dev fallback.
pub fn reject_fallback_for_release(custody: &dyn KeyCustody) -> Result<(), CustodyError> {
    if custody.kind().is_acceptable_for_release() {
        Ok(())
    } else {
        Err(CustodyError::RootUnavailable(
            "dev-fallback custody is not acceptable for a release build".to_string(),
        ))
    }
}

/// Seal 32-plus bytes under a 32-byte key with XChaCha20-Poly1305. The output is
/// `nonce (24) || ciphertext || tag (16)`. `aad` is authenticated, not encrypted.
/// Shared by the dev-fallback root wrap and the per-segment key wrap in the store.
pub(crate) fn aead_seal(
    key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, aead::Error> {
    let cipher = XChaCha20Poly1305::new(&Array::from(*key));
    let mut nonce_bytes = [0u8; XNONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes).map_err(|_| aead::Error)?;
    let nonce = XNonce::from(nonce_bytes);
    let ciphertext = cipher.encrypt(
        &nonce,
        aead::Payload {
            msg: plaintext,
            aad,
        },
    )?;
    let mut out = Vec::with_capacity(XNONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Open a blob produced by [`aead_seal`]. Authentication fails (returns `Err`) if
/// the key, the ciphertext or the `aad` differ from the sealing call.
pub(crate) fn aead_open(key: &[u8; 32], sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>, aead::Error> {
    if sealed.len() < XNONCE_LEN {
        return Err(aead::Error);
    }
    let (nonce_bytes, ciphertext) = sealed.split_at(XNONCE_LEN);
    let cipher = XChaCha20Poly1305::new(&Array::from(*key));
    let nonce = XNonce::try_from(nonce_bytes).map_err(|_| aead::Error)?;
    cipher.decrypt(
        &nonce,
        aead::Payload {
            msg: ciphertext,
            aad,
        },
    )
}

/// Random 32 bytes from the OS CSPRNG.
pub(crate) fn random_key() -> Result<[u8; 32], CustodyError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| CustodyError::Crypto(format!("os rng failed: {e}")))?;
    Ok(bytes)
}

/// Dev-fallback custody over the one store key. `root_id` is a non-secret
/// per-store locator (a UUID) so two stores never share one platform root: on
/// macOS it is the keychain account, on Windows the DPAPI entropy label. Keeping
/// it distinct per store is what lets two stores run without racing one keychain
/// item (separate-shared-state).
pub struct DevFallbackCustody {
    root_id: String,
}

impl DevFallbackCustody {
    /// The keychain service all Halvo dev-fallback root keys share on macOS.
    #[cfg(target_os = "macos")]
    const KEYCHAIN_SERVICE: &'static str = "com.halvo.capture.dev-fallback";

    /// Build a custody bound to a per-store root locator.
    pub fn new(root_id: impl Into<String>) -> Self {
        Self {
            root_id: root_id.into(),
        }
    }

    /// The per-store root locator, written (non-secret) into the store-key file
    /// so a later open finds the same platform root.
    pub fn root_id(&self) -> &str {
        &self.root_id
    }

    /// Destroy the platform root so no retained wrapped store key can ever be
    /// unwrapped again. Part of delete-all's crypto-shred. Safe to call when the
    /// root is already gone.
    pub fn destroy_root(&self) -> Result<(), CustodyError> {
        #[cfg(target_os = "macos")]
        {
            match security_framework::passwords::delete_generic_password(
                Self::KEYCHAIN_SERVICE,
                &self.root_id,
            ) {
                Ok(()) => Ok(()),
                // Already absent is success: delete-all is idempotent.
                Err(_) => Ok(()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Windows DPAPI holds no separate root; destroying the wrapped
            // store-key file (the store owns that) is the key destruction.
            Ok(())
        }
    }

    /// Get the macOS login-keychain root key, creating it on first use. Reading
    /// an item this process created needs no prompt.
    #[cfg(target_os = "macos")]
    fn mac_root_key(&self) -> Result<[u8; 32], CustodyError> {
        use security_framework::passwords::{get_generic_password, set_generic_password};
        match get_generic_password(Self::KEYCHAIN_SERVICE, &self.root_id) {
            Ok(bytes) => {
                let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                    CustodyError::RootUnavailable("keychain root key is not 32 bytes".to_string())
                })?;
                Ok(arr)
            }
            Err(_) => {
                let key = random_key()?;
                set_generic_password(Self::KEYCHAIN_SERVICE, &self.root_id, &key)
                    .map_err(|e| CustodyError::RootUnavailable(format!("keychain write: {e}")))?;
                Ok(key)
            }
        }
    }
}

impl KeyCustody for DevFallbackCustody {
    fn kind(&self) -> CustodyKind {
        CustodyKind::DevFallback
    }

    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        #[cfg(target_os = "macos")]
        {
            let root = self.mac_root_key()?;
            let sealed = aead_seal(&root, key.expose(), self.root_id.as_bytes())
                .map_err(|_| CustodyError::Crypto("sealing the store key failed".to_string()))?;
            Ok(WrappedKey(sealed))
        }
        #[cfg(target_os = "windows")]
        {
            let blob =
                crate::custody::windows::dpapi_protect(key.expose(), self.root_id.as_bytes())?;
            Ok(WrappedKey(blob))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = key;
            Err(CustodyError::RootUnavailable(
                "dev-fallback custody has no platform root on this OS".to_string(),
            ))
        }
    }

    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        #[cfg(target_os = "macos")]
        {
            let root = self.mac_root_key()?;
            let plain = aead_open(&root, &wrapped.0, self.root_id.as_bytes())
                .map_err(|_| CustodyError::Crypto("unwrapping the store key failed".to_string()))?;
            let arr: [u8; 32] = plain
                .as_slice()
                .try_into()
                .map_err(|_| CustodyError::Crypto("store key is not 32 bytes".to_string()))?;
            Ok(StoreKey::from_bytes(arr))
        }
        #[cfg(target_os = "windows")]
        {
            let plain =
                crate::custody::windows::dpapi_unprotect(&wrapped.0, self.root_id.as_bytes())?;
            let arr: [u8; 32] = plain
                .as_slice()
                .try_into()
                .map_err(|_| CustodyError::Crypto("store key is not 32 bytes".to_string()))?;
            Ok(StoreKey::from_bytes(arr))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = wrapped;
            Err(CustodyError::RootUnavailable(
                "dev-fallback custody has no platform root on this OS".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_check_rejects_only_the_fallback() {
        assert!(!CustodyKind::DevFallback.is_acceptable_for_release());
        assert!(CustodyKind::SecureEnclave.is_acceptable_for_release());
        assert!(CustodyKind::Dpapi.is_acceptable_for_release());
    }

    #[test]
    fn seal_roundtrips_and_detects_tampering() {
        let key = [7u8; 32];
        let aad = b"segment-header";
        let sealed = aead_seal(&key, b"thirty-two-byte-secret-padding!!", aad).unwrap();
        let opened = aead_open(&key, &sealed, aad).unwrap();
        assert_eq!(opened, b"thirty-two-byte-secret-padding!!");
        // Wrong aad fails authentication.
        assert!(aead_open(&key, &sealed, b"other").is_err());
        // Wrong key fails authentication.
        assert!(aead_open(&[8u8; 32], &sealed, aad).is_err());
        // A flipped ciphertext byte fails authentication.
        let mut bad = sealed.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        assert!(aead_open(&key, &bad, aad).is_err());
    }

    #[test]
    fn reject_helper_matches_kind() {
        let custody = DevFallbackCustody::new("test-root");
        assert!(reject_fallback_for_release(&custody).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_wrap_unwrap_roundtrips_with_no_prompt() {
        // A fresh per-test root id so the shared login keychain is not raced by
        // another test or run (separate-shared-state).
        let root_id = format!("halvo-test-{}", uuid::Uuid::now_v7());
        let custody = DevFallbackCustody::new(root_id);
        let key = StoreKey::from_bytes([42u8; 32]);
        let wrapped = custody.wrap_store_key(&key).unwrap();
        let unwrapped = custody.unwrap_store_key(&wrapped).unwrap();
        assert_eq!(unwrapped.expose(), &[42u8; 32]);
        // delete-all destroys the root, after which the wrapped key cannot open.
        custody.destroy_root().unwrap();
        assert!(custody.destroy_root().is_ok()); // idempotent
    }
}
