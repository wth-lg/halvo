//! macOS Secure Enclave key custody: the release path.
//!
//! A Secure Enclave P-256 key with `privateKeyUsage` only and the
//! `AfterFirstUnlockThisDeviceOnly` class wraps the store key by ECIES
//! (researcher-2 findings 17, 18, 19). The private key never leaves the enclave,
//! so moving the disk to another machine makes the store unreadable. This path
//! reports [`CustodyKind::SecureEnclave`], which the release privacy check
//! accepts.
//!
//! It is compiled only behind the `secure-enclave` feature, which is off by
//! default, because persisting the enclave key needs a Developer ID signed bundle
//! with an application identifier: the scratch probe generated and used an enclave
//! key but could not persist it from an unsigned binary (OSStatus -34018,
//! researcher-2). R1 builds and tests the dev fallback ([`super::dev_fallback`]);
//! this path is **untested** until a signed bundle exists (the owner's reserved
//! signing decision). Its tests are gated off.

#![cfg(feature = "secure-enclave")]

use security_framework::access_control::{ProtectionMode, SecAccessControl};
use security_framework::item::Location;
use security_framework::key::{Algorithm, GenerateKeyOptions, KeyType, SecKey, Token};

use super::{CustodyError, CustodyKind, KeyCustody, StoreKey, WrappedKey};

/// `kSecAccessControlPrivateKeyUsage`: the key may sign and perform key agreement
/// without biometry or passcode, which is what a background daemon needs.
const PRIVATE_KEY_USAGE: u32 = 1 << 30;

/// The ECIES algorithm used to wrap and unwrap the store key, as recommended by
/// Apple for Secure Enclave symmetric wrapping (researcher-2 finding 18).
const ECIES: Algorithm = Algorithm::ECIESEncryptionCofactorX963SHA256AESGCM;

/// Secure Enclave custody keyed by a per-store label. The private key lives in the
/// data-protection keychain of the owner; `wrap`/`unwrap` are ECIES encrypt/decrypt.
pub struct SecureEnclaveCustody {
    key: SecKey,
}

impl SecureEnclaveCustody {
    /// Generate (or the caller supplies) a Secure Enclave key for this store.
    ///
    /// Untested: persisting the key needs a signed bundle. Callers in a signed
    /// release wire this to a persisted key found by label; here it generates a
    /// fresh non-persistent key so the path type-checks.
    pub fn new(label: impl Into<String>) -> Result<Self, CustodyError> {
        let access = SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly),
            PRIVATE_KEY_USAGE as _,
        )
        .map_err(|e| CustodyError::RootUnavailable(format!("access control: {e}")))?;

        let mut options = GenerateKeyOptions::default();
        options
            .set_key_type(KeyType::ec())
            .set_size_in_bits(256)
            .set_token(Token::SecureEnclave)
            .set_location(Location::DataProtectionKeychain)
            .set_access_control(access)
            .set_label(label);

        let key = SecKey::new(&options)
            .map_err(|e| CustodyError::RootUnavailable(format!("enclave key: {e}")))?;
        Ok(Self { key })
    }
}

impl KeyCustody for SecureEnclaveCustody {
    fn kind(&self) -> CustodyKind {
        CustodyKind::SecureEnclave
    }

    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        let public = self
            .key
            .public_key()
            .ok_or_else(|| CustodyError::Crypto("enclave key has no public key".to_string()))?;
        let blob = public
            .encrypt_data(ECIES, key.expose())
            .map_err(|e| CustodyError::Crypto(format!("ECIES encrypt: {e}")))?;
        Ok(WrappedKey(blob))
    }

    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        let plain = self
            .key
            .decrypt_data(ECIES, &wrapped.0)
            .map_err(|e| CustodyError::Crypto(format!("ECIES decrypt: {e}")))?;
        let arr: [u8; 32] = plain
            .as_slice()
            .try_into()
            .map_err(|_| CustodyError::Crypto("store key is not 32 bytes".to_string()))?;
        Ok(StoreKey::from_bytes(arr))
    }
}
