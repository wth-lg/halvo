//! macOS Secure Enclave key custody: the release path.

#![cfg(feature = "secure-enclave")]

use security_framework::access_control::{ProtectionMode, SecAccessControl};
use security_framework::item::Location;
use security_framework::key::{Algorithm, GenerateKeyOptions, KeyType, SecKey, Token};

use super::{CustodyError, CustodyKind, KeyCustody, StoreKey, WrappedKey};

const PRIVATE_KEY_USAGE: u32 = 1 << 30;

const ECIES: Algorithm = Algorithm::ECIESEncryptionCofactorX963SHA256AESGCM;

/// Secure Enclave custody keyed by a per-store label.
pub struct SecureEnclaveCustody {
    key: SecKey,
}

impl SecureEnclaveCustody {
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
