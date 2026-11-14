//! Dev-fallback key custody.

use aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use super::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

const XNONCE_LEN: usize = size_of::<XNonce>();

impl CustodyKind {
    /// Whether a release build accepts this custody kind: only a kind whose
    /// custody this build can rebuild and whose root it can destroy.
    pub fn is_acceptable_for_release(self) -> bool {
        match self {
            CustodyKind::DevFallback | CustodyKind::SecureEnclave => false,
            CustodyKind::Dpapi => cfg!(target_os = "windows"),
        }
    }

    pub(crate) fn wrapped_store_key_len(self) -> Option<usize> {
        match self {
            CustodyKind::DevFallback | CustodyKind::Dpapi => {
                Some(XNONCE_LEN + 32 + size_of::<chacha20poly1305::Tag>())
            }
            CustodyKind::SecureEnclave => None,
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

pub(crate) fn fill_random(buf: &mut [u8]) -> Result<(), CustodyError> {
    getrandom::getrandom(buf).map_err(|e| CustodyError::Os(e.raw_os_error()))
}

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
    const ERR_SEC_INTERACTION_NOT_ALLOWED: i32 = -25308;

    fn classify(e: &KeychainError) -> CustodyError {
        match e.code() {
            code if code == errSecItemNotFound => CustodyError::RootMissing,
            code if code == errSecAuthFailed => CustodyError::RootDenied,
            code if code == ERR_SEC_INTERACTION_NOT_ALLOWED => CustodyError::RootLocked(code),
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

    fn make_root(root_id: RootId) -> Result<Zeroizing<[u8; 32]>, CustodyError> {
        let mut root = Zeroizing::new([0u8; 32]);
        fill_random(root.as_mut_slice())?;
        set_generic_password(SERVICE, &root_id.to_string(), root.as_slice())
            .map_err(|e| classify(&e))?;
        Ok(root)
    }

    fn existing_or_new_root(root_id: RootId) -> Result<Zeroizing<[u8; 32]>, CustodyError> {
        match existing_root(root_id)? {
            Some(root) => Ok(root),
            None => make_root(root_id),
        }
    }

    pub(super) fn create_root(root_id: RootId) -> Result<(), CustodyError> {
        match existing_root(root_id)? {
            Some(_) => Err(CustodyError::RootExists),
            None => make_root(root_id).map(drop),
        }
    }

    fn seal(
        root_id: RootId,
        root: Zeroizing<[u8; 32]>,
        key: &StoreKey,
    ) -> Result<WrappedKey, CustodyError> {
        let sealed = aead_seal(&root, key.expose(), root_id.as_bytes())
            .map_err(|_| CustodyError::Crypto("sealing the store key failed".to_string()))?;
        Ok(WrappedKey(sealed))
    }

    pub(super) fn wrap(root_id: RootId, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        seal(root_id, existing_or_new_root(root_id)?, key)
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

    #[cfg(test)]
    pub(super) fn root_bytes(root_id: RootId) -> Result<Option<Zeroizing<[u8; 32]>>, CustodyError> {
        existing_root(root_id)
    }

    #[cfg(test)]
    pub(super) fn replace_root(root_id: RootId, root: &[u8; 32]) -> Result<(), CustodyError> {
        set_generic_password(SERVICE, &root_id.to_string(), root).map_err(|e| classify(&e))
    }
}

#[cfg(target_os = "windows")]
mod dpapi {
    use crate::custody::windows::user_roots;
    use crate::custody::{CustodyError, RootId, StoreKey, WrappedKey};

    pub(super) fn wrap(root_id: RootId, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        user_roots()?.wrap(root_id, key)
    }

    pub(super) fn create_root(root_id: RootId) -> Result<(), CustodyError> {
        user_roots()?.create_root(root_id)
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

    #[cfg(test)]
    pub(super) fn root_bytes(
        root_id: RootId,
    ) -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, CustodyError> {
        user_roots()?.root_bytes(root_id)
    }

    #[cfg(test)]
    pub(super) fn replace_root(root_id: RootId, root: &[u8; 32]) -> Result<(), CustodyError> {
        user_roots()?.replace_root(root_id, root)
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

    pub(super) fn create_root(_root_id: RootId) -> Result<(), CustodyError> {
        Err(no_root())
    }

    pub(super) fn unwrap(
        _root_id: RootId,
        _wrapped: &WrappedKey,
    ) -> Result<StoreKey, CustodyError> {
        Err(no_root())
    }

    pub(super) fn destroy_root(_root_id: RootId) -> Result<(), CustodyError> {
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn root_exists(_root_id: RootId) -> Result<bool, CustodyError> {
        Ok(false)
    }

    #[cfg(test)]
    pub(super) fn root_bytes(
        _root_id: RootId,
    ) -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, CustodyError> {
        Ok(None)
    }

    #[cfg(test)]
    pub(super) fn replace_root(_root_id: RootId, _root: &[u8; 32]) -> Result<(), CustodyError> {
        Err(no_root())
    }
}

#[cfg(target_os = "windows")]
use dpapi as platform;
#[cfg(target_os = "macos")]
use keychain as platform;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use unsupported as platform;

#[cfg(test)]
pub(crate) fn dev_root_exists(root_id: RootId) -> Result<bool, CustodyError> {
    platform::root_exists(root_id)
}

#[cfg(test)]
pub(crate) fn dev_root_bytes(root_id: RootId) -> Result<Option<Zeroizing<[u8; 32]>>, CustodyError> {
    platform::root_bytes(root_id)
}

#[cfg(test)]
pub(crate) fn replace_dev_root(root_id: RootId, root: &[u8; 32]) -> Result<(), CustodyError> {
    platform::replace_root(root_id, root)
}

/// Dev-fallback custody over the one store key. `root_id` is a non-secret
/// per-store locator (a UUID): on
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

    fn create_root(&self) -> Result<(), CustodyError> {
        platform::create_root(self.root_id)
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
    use crate::custody::file_root::test_support::{FileRootCustody, TestProtector};
    use crate::custody::file_root::{FileRoots, Protector};

    trait RootHooks: KeyCustody {
        fn root(&self) -> Zeroizing<[u8; 32]>;
        fn replace(&self, root: &[u8; 32]);
    }

    impl RootHooks for DevFallbackCustody {
        fn root(&self) -> Zeroizing<[u8; 32]> {
            dev_root_bytes(self.root_id())
                .unwrap()
                .expect("the platform holds the root after a wrap")
        }
        fn replace(&self, root: &[u8; 32]) {
            replace_dev_root(self.root_id(), root).unwrap();
        }
    }

    impl<P: Protector> RootHooks for FileRootCustody<P> {
        fn root(&self) -> Zeroizing<[u8; 32]> {
            self.roots
                .root_bytes(self.root_id)
                .unwrap()
                .expect("the root file exists after a wrap")
        }
        fn replace(&self, root: &[u8; 32]) {
            self.roots.replace_root(self.root_id, root).unwrap();
        }
    }

    struct DestroyedAtTheEnd<'a>(&'a dyn RootHooks);

    impl Drop for DestroyedAtTheEnd<'_> {
        fn drop(&mut self) {
            let _ = self.0.destroy_root();
        }
    }

    fn file_root_custody(dir: &std::path::Path) -> FileRootCustody<TestProtector> {
        FileRootCustody {
            roots: FileRoots::new(dir.join("roots"), TestProtector::new(0x5a).0),
            root_id: RootId::fresh(),
        }
    }

    fn a_key_that_is_no_root() -> [u8; 32] {
        let mut key = [0u8; 32];
        fill_random(&mut key).unwrap();
        key
    }

    fn the_wrap_opens_under_its_root_and_no_other_key(custody: &dyn RootHooks) {
        let _destroyed = DestroyedAtTheEnd(custody);
        let wrapped = custody
            .wrap_store_key(&StoreKey::from_bytes([0x3c; 32]))
            .unwrap();
        let root = custody.root();
        let label = custody.root_id();
        assert!(
            aead_open(&root, &wrapped.0, label.as_bytes()).is_ok(),
            "the wrapped store key does not open under its root"
        );
        let replacement = a_key_that_is_no_root();
        for (name, other) in [
            ("all zero", [0u8; 32]),
            ("all ones", [0xffu8; 32]),
            ("random", replacement),
        ] {
            assert!(
                aead_open(&other, &wrapped.0, label.as_bytes()).is_err(),
                "the wrapped store key opens under a key that is not its root ({name})"
            );
        }
        custody.replace(&replacement);
        let under_replacement = custody.unwrap_store_key(&wrapped);
        assert!(
            matches!(under_replacement, Err(CustodyError::WrapAuthFailed)),
            "with the root replaced under the same id, unwrap answered {:?}",
            under_replacement.err()
        );
        custody.replace(&root);
        let restored = custody.unwrap_store_key(&wrapped).unwrap();
        assert!(
            restored.expose() == &[0x3c; 32],
            "the restored root does not give back the store key"
        );
    }

    fn no_all_zero_root_and_no_other_store_root(first: &dyn RootHooks, second: &dyn RootHooks) {
        let _first = DestroyedAtTheEnd(first);
        let _second = DestroyedAtTheEnd(second);
        let wrapped = first
            .wrap_store_key(&StoreKey::from_bytes([0x3c; 32]))
            .unwrap();
        second
            .wrap_store_key(&StoreKey::from_bytes([0x4d; 32]))
            .unwrap();
        let (root, other_store_root) = (first.root(), second.root());
        let label = first.root_id();
        assert!(root.iter().any(|&b| b != 0), "the root is all zero");
        assert!(*root != *other_store_root, "two stores have one root");
        assert!(
            aead_open(&[0u8; 32], &wrapped.0, label.as_bytes()).is_err(),
            "the wrapped store key opens under an all-zero root"
        );
        assert!(
            aead_open(&other_store_root, &wrapped.0, label.as_bytes()).is_err(),
            "the wrapped store key opens under the root of a second store"
        );
        first.replace(&[0u8; 32]);
        assert!(
            first.unwrap_store_key(&wrapped).is_err(),
            "the wrapped store key unwraps with an all-zero root in place"
        );
    }

    fn a_root_made_again_under_one_id_differs_and_opens_no_wrap_of_the_first(
        first: &dyn RootHooks,
        fresh: &dyn RootHooks,
    ) {
        let _first = DestroyedAtTheEnd(first);
        let _fresh = DestroyedAtTheEnd(fresh);
        first.create_root().unwrap();
        let wrapped = first
            .wrap_store_key(&StoreKey::from_bytes([0x3c; 32]))
            .unwrap();
        let first_root = first.root();
        first.destroy_root().unwrap();
        fresh.create_root().unwrap();
        let fresh_root = fresh.root();
        assert!(
            *first_root != *fresh_root,
            "two roots made under one root id hold the same bytes"
        );
        let from_the_id_alone = fresh.unwrap_store_key(&wrapped);
        assert!(
            matches!(from_the_id_alone, Err(CustodyError::WrapAuthFailed)),
            "the wrapped store key and its public root id opened the store key under a new root: {:?}",
            from_the_id_alone.as_ref().map(|key| key.expose() == &[0x3c; 32])
        );
    }

    #[test]
    fn two_creates_under_one_root_id_make_two_roots_and_the_id_alone_opens_no_store_key() {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let root_id = RootId::fresh();
            a_root_made_again_under_one_id_differs_and_opens_no_wrap_of_the_first(
                &DevFallbackCustody::new(root_id),
                &DevFallbackCustody::new(root_id),
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let root_id = RootId::fresh();
        a_root_made_again_under_one_id_differs_and_opens_no_wrap_of_the_first(
            &FileRootCustody {
                roots: FileRoots::new(dir.path().join("first"), TestProtector::new(0x5a).0),
                root_id,
            },
            &FileRootCustody {
                roots: FileRoots::new(dir.path().join("fresh"), TestProtector::new(0x5a).0),
                root_id,
            },
        );
    }

    #[test]
    fn seals_under_one_key_use_distinct_nonces_that_vary_in_every_byte() {
        const SEALS: usize = 64;
        let key = [0x21u8; 32];
        let nonces: Vec<Vec<u8>> = (0..SEALS)
            .map(|_| {
                aead_seal(&key, b"one plaintext", b"one label").unwrap()[..XNONCE_LEN].to_vec()
            })
            .collect();
        let distinct: std::collections::HashSet<&Vec<u8>> = nonces.iter().collect();
        assert_eq!(
            distinct.len(),
            SEALS,
            "seals under one key repeated a nonce"
        );
        for position in 0..XNONCE_LEN {
            assert!(
                nonces
                    .iter()
                    .any(|nonce| nonce[position] != nonces[0][position]),
                "byte {position} of the nonce took one value in {SEALS} seals"
            );
        }
    }

    #[test]
    fn a_wrapped_store_key_opens_under_no_all_zero_root_and_no_other_store_root() {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        no_all_zero_root_and_no_other_store_root(
            &DevFallbackCustody::new(RootId::fresh()),
            &DevFallbackCustody::new(RootId::fresh()),
        );
        let dir = tempfile::tempdir().unwrap();
        no_all_zero_root_and_no_other_store_root(
            &file_root_custody(dir.path()),
            &file_root_custody(dir.path()),
        );
    }

    #[test]
    fn the_store_key_wrap_opens_under_its_root_and_under_no_other_key() {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        the_wrap_opens_under_its_root_and_no_other_key(&DevFallbackCustody::new(RootId::fresh()));
        let dir = tempfile::tempdir().unwrap();
        the_wrap_opens_under_its_root_and_no_other_key(&file_root_custody(dir.path()));
    }

    #[test]
    fn a_store_key_wrap_of_each_kind_that_this_build_makes_is_72_bytes() {
        assert_eq!(CustodyKind::DevFallback.wrapped_store_key_len(), Some(72));
        assert_eq!(CustodyKind::Dpapi.wrapped_store_key_len(), Some(72));
        assert_eq!(CustodyKind::SecureEnclave.wrapped_store_key_len(), None);
        let sealed = aead_seal(&[0x11; 32], &[0x22; 32], b"a root id").unwrap();
        assert_eq!(sealed.len(), 72, "a seal of a 32-byte key");
    }

    fn every_custody_kind() -> Vec<CustodyKind> {
        let kinds = vec![
            CustodyKind::DevFallback,
            CustodyKind::SecureEnclave,
            CustodyKind::Dpapi,
        ];
        for kind in &kinds {
            match kind {
                CustodyKind::DevFallback | CustodyKind::SecureEnclave | CustodyKind::Dpapi => {}
            }
        }
        kinds
    }

    #[test]
    fn a_release_accepts_only_a_kind_whose_custody_this_build_can_rebuild() {
        let mut wrong = Vec::new();
        for kind in every_custody_kind() {
            if !kind.is_acceptable_for_release() {
                continue;
            }
            if let Err(e) = crate::custody::custody_for(kind, RootId::fresh()) {
                wrong.push(format!(
                    "{kind:?} is accepted for a release and custody_for answers {e:?}"
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn a_release_accepts_only_a_kind_whose_delete_all_can_finish() {
        assert!(!CustodyKind::DevFallback.is_acceptable_for_release());
        assert!(!CustodyKind::SecureEnclave.is_acceptable_for_release());
        assert_eq!(
            CustodyKind::Dpapi.is_acceptable_for_release(),
            cfg!(target_os = "windows")
        );
    }

    fn destroying_its_root_twice_is_no_error(custody: &dyn KeyCustody) {
        custody.create_root().unwrap();
        let first = custody.destroy_root();
        let second = custody.destroy_root();
        assert!(
            first.is_ok() && second.is_ok(),
            "destroy_root answered {:?} and then {:?}",
            first.err(),
            second.err()
        );
    }

    #[test]
    fn destroying_a_root_twice_is_no_error_for_each_custody_the_product_builds() {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        destroying_its_root_twice_is_no_error(&DevFallbackCustody::new(RootId::fresh()));
        #[cfg(target_os = "windows")]
        destroying_its_root_twice_is_no_error(&crate::custody::windows::DpapiCustody::new(
            RootId::fresh(),
        ));
        let dir = tempfile::tempdir().unwrap();
        destroying_its_root_twice_is_no_error(&file_root_custody(dir.path()));
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
        let root = crate::store::roots::RootGuard::fresh();
        let custody = DevFallbackCustody::new(root.id());
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
            Err(CustodyError::RootLocked(-25308))
        ));
        assert!(matches!(
            keychain::absent_is_none::<u8>(Err(Error::from_code(-25291))),
            Err(CustodyError::Os(Some(-25291)))
        ));
    }
}
