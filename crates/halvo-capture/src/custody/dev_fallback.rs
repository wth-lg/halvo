//! Dev-fallback key custody for unsigned builds.

use aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use super::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

const XNONCE_LEN: usize = size_of::<XNonce>();

impl CustodyKind {
    /// Whether this custody is strong enough for a release build.
    pub fn is_acceptable_for_release(self) -> bool {
        match self {
            CustodyKind::DevFallback => false,
            CustodyKind::SecureEnclave | CustodyKind::Dpapi => true,
        }
    }
}

/// Fail unless `custody` is acceptable for a release build.
pub fn reject_fallback_for_release(custody: &dyn KeyCustody) -> Result<(), CustodyError> {
    if custody.kind().is_acceptable_for_release() {
        Ok(())
    } else {
        Err(CustodyError::CustodyKindUnsupported(custody.kind()))
    }
}

pub(crate) fn aead_seal(
    key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, aead::Error> {
    let cipher = XChaCha20Poly1305::new(key.into());
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

pub(crate) fn aead_open(
    key: &[u8; 32],
    sealed: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, aead::Error> {
    if sealed.len() < XNONCE_LEN {
        return Err(aead::Error);
    }
    let (nonce_bytes, ciphertext) = sealed.split_at(XNONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XNonce::try_from(nonce_bytes).map_err(|_| aead::Error)?;
    cipher
        .decrypt(
            &nonce,
            aead::Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map(Zeroizing::new)
}

/// Fill `buf` from the OS random number generator.
pub(crate) fn fill_random(buf: &mut [u8]) -> Result<(), CustodyError> {
    getrandom::getrandom(buf).map_err(|e| CustodyError::Os(e.raw_os_error()))
}

/// Parse 32 unwrapped key bytes into a [`StoreKey`] without leaving a copy.
pub(crate) fn store_key_from(plain: Zeroizing<Vec<u8>>) -> Result<StoreKey, CustodyError> {
    if plain.len() != 32 {
        return Err(CustodyError::Crypto(
            "store key is not 32 bytes".to_string(),
        ));
    }
    let mut key = StoreKey::from_bytes([0u8; 32]);
    key.0.copy_from_slice(&plain);
    Ok(key)
}

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::base::Error as KeychainError;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };
    use security_framework_sys::base::{errSecAuthFailed, errSecItemNotFound};
    use zeroize::Zeroizing;

    use super::{aead_open, aead_seal, fill_random, store_key_from};
    use crate::custody::{CustodyError, RootId, StoreKey, WrappedKey};

    const SERVICE: &str = "com.halvo.capture.dev-fallback";

    fn classify(e: &KeychainError) -> CustodyError {
        match e.code() {
            code if code == errSecItemNotFound => CustodyError::RootMissing,
            code if code == errSecAuthFailed => CustodyError::RootDenied,
            code => CustodyError::Os(Some(code)),
        }
    }

    pub(super) fn absent_is_none<T>(
        result: Result<T, KeychainError>,
    ) -> Result<Option<T>, CustodyError> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.code() == errSecItemNotFound => Ok(None),
            Err(e) => Err(classify(&e)),
        }
    }

    fn existing_root(root_id: RootId) -> Result<Option<Zeroizing<[u8; 32]>>, CustodyError> {
        let Some(bytes) = absent_is_none(get_generic_password(SERVICE, &root_id.to_string()))?
        else {
            return Ok(None);
        };
        let bytes = Zeroizing::new(bytes);
        let root: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| CustodyError::RootCorrupt)?;
        Ok(Some(Zeroizing::new(root)))
    }

    fn existing_or_new_root(root_id: RootId) -> Result<Zeroizing<[u8; 32]>, CustodyError> {
        if let Some(root) = existing_root(root_id)? {
            return Ok(root);
        }
        let mut root = Zeroizing::new([0u8; 32]);
        fill_random(root.as_mut_slice())?;
        set_generic_password(SERVICE, &root_id.to_string(), root.as_slice())
            .map_err(|e| classify(&e))?;
        Ok(root)
    }

    pub(super) fn wrap(root_id: RootId, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        let root = existing_or_new_root(root_id)?;
        let sealed = aead_seal(&root, key.expose(), root_id.as_bytes())
            .map_err(|_| CustodyError::Crypto("sealing the store key failed".to_string()))?;
        Ok(WrappedKey(sealed))
    }

    pub(super) fn unwrap(root_id: RootId, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        let root = existing_root(root_id)?.ok_or(CustodyError::RootMissing)?;
        let plain = aead_open(&root, &wrapped.0, root_id.as_bytes())
            .map_err(|_| CustodyError::WrapAuthFailed)?;
        store_key_from(plain)
    }

    pub(super) fn destroy_root(root_id: RootId) -> Result<(), CustodyError> {
        absent_is_none(delete_generic_password(SERVICE, &root_id.to_string())).map(|_| ())
    }

    #[cfg(test)]
    pub(super) fn root_exists(root_id: RootId) -> Result<bool, CustodyError> {
        existing_root(root_id).map(|root| root.is_some())
    }
}

#[cfg(target_os = "windows")]
mod dpapi {
    use crate::custody::windows::user_roots;
    use crate::custody::{CustodyError, RootId, StoreKey, WrappedKey};

    pub(super) fn wrap(root_id: RootId, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        user_roots()?.wrap(root_id, key)
    }

    pub(super) fn unwrap(root_id: RootId, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        user_roots()?.unwrap(root_id, wrapped)
    }

    pub(super) fn destroy_root(root_id: RootId) -> Result<(), CustodyError> {
        user_roots()?.destroy_root(root_id)
    }

    #[cfg(test)]
    pub(super) fn root_exists(root_id: RootId) -> Result<bool, CustodyError> {
        user_roots()?.root_exists(root_id)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod unsupported {
    use crate::custody::{CustodyError, CustodyKind, RootId, StoreKey, WrappedKey};

    fn no_root() -> CustodyError {
        CustodyError::CustodyKindUnsupported(CustodyKind::DevFallback)
    }

    pub(super) fn wrap(_root_id: RootId, _key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        Err(no_root())
    }

    pub(super) fn unwrap(
        _root_id: RootId,
        _wrapped: &WrappedKey,
    ) -> Result<StoreKey, CustodyError> {
        Err(no_root())
    }

    pub(super) fn destroy_root(_root_id: RootId) -> Result<(), CustodyError> {
        Err(no_root())
    }

    #[cfg(test)]
    pub(super) fn root_exists(_root_id: RootId) -> Result<bool, CustodyError> {
        Ok(false)
    }
}

#[cfg(target_os = "windows")]
use dpapi as platform;
#[cfg(target_os = "macos")]
use keychain as platform;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use unsupported as platform;

/// Whether the platform still holds the dev-fallback root for `root_id`.
#[cfg(test)]
pub(crate) fn dev_root_exists(root_id: RootId) -> Result<bool, CustodyError> {
    platform::root_exists(root_id)
}

/// Dev-fallback custody over the one store key. `root_id` is a non-secret
/// per-store locator (a UUID) so two stores never share one platform root: on
/// macOS it is the keychain account, on Windows the DPAPI entropy label.
pub struct DevFallbackCustody {
    root_id: RootId,
}

impl DevFallbackCustody {
    /// Build a custody bound to a per-store root locator.
    pub fn new(root_id: RootId) -> Self {
        Self { root_id }
    }
}

impl KeyCustody for DevFallbackCustody {
    fn kind(&self) -> CustodyKind {
        CustodyKind::DevFallback
    }

    fn root_id(&self) -> RootId {
        self.root_id
    }

    fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        platform::wrap(self.root_id, key)
    }

    fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
        platform::unwrap(self.root_id, wrapped)
    }

    fn destroy_root(&self) -> Result<(), CustodyError> {
        platform::destroy_root(self.root_id)
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
        assert_eq!(opened.as_slice(), b"thirty-two-byte-secret-padding!!");
        assert!(aead_open(&key, &sealed, b"other").is_err());
        assert!(aead_open(&[8u8; 32], &sealed, aad).is_err());
        let mut bad = sealed.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        assert!(aead_open(&key, &bad, aad).is_err());
    }

    #[test]
    fn reject_helper_matches_kind() {
        let custody = DevFallbackCustody::new(RootId::fresh());
        assert!(matches!(
            reject_fallback_for_release(&custody),
            Err(CustodyError::CustodyKindUnsupported(
                CustodyKind::DevFallback
            ))
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_wrap_unwrap_roundtrips_with_no_prompt() {
        let custody = DevFallbackCustody::new(RootId::fresh());
        let key = StoreKey::from_bytes([42u8; 32]);
        let wrapped = custody.wrap_store_key(&key).unwrap();
        let unwrapped = custody.unwrap_store_key(&wrapped).unwrap();
        assert_eq!(unwrapped.expose(), &[42u8; 32]);
        custody.destroy_root().unwrap();
        assert!(matches!(
            custody.unwrap_store_key(&wrapped),
            Err(CustodyError::RootMissing)
        ));
        assert!(custody.destroy_root().is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn only_a_missing_keychain_item_counts_as_absent() {
        use security_framework::base::Error;
        use security_framework_sys::base::{errSecAuthFailed, errSecItemNotFound};

        assert_eq!(keychain::absent_is_none(Ok(7u8)).unwrap(), Some(7));
        assert_eq!(
            keychain::absent_is_none::<u8>(Err(Error::from_code(errSecItemNotFound))).unwrap(),
            None
        );
        assert!(matches!(
            keychain::absent_is_none::<u8>(Err(Error::from_code(errSecAuthFailed))),
            Err(CustodyError::RootDenied)
        ));
        assert!(matches!(
            keychain::absent_is_none::<u8>(Err(Error::from_code(-25308))),
            Err(CustodyError::Os(Some(-25308)))
        ));
    }
}
