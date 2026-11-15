//! macOS Secure Enclave key custody.

#![cfg(feature = "secure-enclave")]

use security_framework::key::{Algorithm, SecKey};
use zeroize::Zeroizing;

use super::{CustodyError, CustodyKind, KeyCustody, KeyLabel, RootId, StoreKey, WrappedKey};

const ECIES: Algorithm = Algorithm::ECIESEncryptionCofactorX963SHA256AESGCM;

pub struct SecureEnclaveCustody {
    root_id: RootId,
    key: SecKey,
}

impl SecureEnclaveCustody {
    pub fn from_persisted_key(root_id: RootId, key: SecKey) -> Self {
        Self { root_id, key }
    }
}

impl KeyCustody for SecureEnclaveCustody {
    fn kind(&self) -> CustodyKind {
        CustodyKind::SecureEnclave
    }

    fn root_id(&self) -> RootId {
        self.root_id
    }

    fn roots(&self) -> Option<&std::path::Path> {
        None
    }

    fn wrap_store_key(&self, key: &StoreKey, _label: KeyLabel) -> Result<WrappedKey, CustodyError> {
        let public = self
            .key
            .public_key()
            .ok_or_else(|| CustodyError::Crypto("enclave key has no public key".to_string()))?;
        let blob = public
            .encrypt_data(ECIES, key.expose())
            .map_err(|e| CustodyError::Crypto(format!("ECIES encrypt: {e}")))?;
        Ok(WrappedKey(blob))
    }

    fn create_root(&self) -> Result<(), CustodyError> {
        Err(CustodyError::NotImplemented("secure-enclave root creation"))
    }

    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        _label: KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        let plain = self
            .key
            .decrypt_data(ECIES, &wrapped.0)
            .map(Zeroizing::new)
            .map_err(|e| CustodyError::Crypto(format!("ECIES decrypt: {e}")))?;
        super::dev_fallback::store_key_from(plain)
    }

    fn destroy_root(&self) -> Result<(), CustodyError> {
        Err(CustodyError::NotImplemented(
            "secure-enclave root destruction",
        ))
    }
}
