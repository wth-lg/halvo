//! Dev-fallback key custody.

use aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use std::path::{Path, PathBuf};

use super::file_root::FileRoots;
#[cfg(not(target_os = "windows"))]
use super::file_root::Protector;
use super::{CustodyError, CustodyKind, KeyCustody, KeyLabel, RootId, StoreKey, WrappedKey};

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

#[cfg(not(target_os = "windows"))]
pub(crate) struct Plain;

#[cfg(not(target_os = "windows"))]
impl Protector for Plain {
    fn protect(&self, plain: &[u8], _label: &[u8]) -> Result<Vec<u8>, CustodyError> {
        Ok(plain.to_vec())
    }

    fn unprotect(&self, blob: &[u8], _label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
        Ok(Zeroizing::new(blob.to_vec()))
    }
}

#[cfg(not(target_os = "windows"))]
type PlatformProtector = Plain;
#[cfg(target_os = "windows")]
type PlatformProtector = crate::custody::windows::Dpapi;

#[cfg(not(target_os = "windows"))]
const PLATFORM_PROTECTOR: PlatformProtector = Plain;
#[cfg(target_os = "windows")]
const PLATFORM_PROTECTOR: PlatformProtector = crate::custody::windows::Dpapi;

/// The custody of a build without the feature `secure-enclave`: the root is a
/// file `<root id>.root` under the roots directory that the caller passes, of
/// mode 0600 in a directory of mode 0700. On macOS the file holds the 32 random
/// bytes; on Windows DPAPI protects them for this user.
pub struct DevFallbackCustody {
    root_id: RootId,
    roots: FileRoots<PlatformProtector>,
}

impl DevFallbackCustody {
    /// A custody bound to a per-store root id and the roots directory in use.
    pub fn new(root_id: RootId, roots: PathBuf) -> Self {
        Self {
            root_id,
            roots: FileRoots::new(roots, PLATFORM_PROTECTOR),
        }
    }

    #[cfg(test)]
    pub(crate) fn root_bytes(&self) -> Result<Option<Zeroizing<[u8; 32]>>, CustodyError> {
        self.roots.root_bytes(self.root_id)
    }

    #[cfg(test)]
    pub(crate) fn replace_root(&self, root: &[u8; 32]) -> Result<(), CustodyError> {
        self.roots.replace_root(self.root_id, root)
    }

    #[cfg(test)]
    pub(crate) fn root_exists(&self) -> Result<bool, CustodyError> {
        self.roots.root_exists(self.root_id)
    }
}

impl KeyCustody for DevFallbackCustody {
    fn kind(&self) -> CustodyKind {
        CustodyKind::DevFallback
    }

    fn root_id(&self) -> RootId {
        self.root_id
    }

    fn roots(&self) -> Option<&Path> {
        Some(self.roots.dir())
    }

    fn wrap_store_key(&self, key: &StoreKey, label: KeyLabel) -> Result<WrappedKey, CustodyError> {
        self.roots.wrap(self.root_id, label, key)
    }

    fn create_root(&self) -> Result<(), CustodyError> {
        self.roots.create_root(self.root_id)
    }

    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        label: KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        self.roots.unwrap(self.root_id, label, wrapped)
    }

    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.roots.destroy_root(self.root_id)
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
            self.root_bytes()
                .unwrap()
                .expect("the root file exists after a wrap")
        }
        fn replace(&self, root: &[u8; 32]) {
            self.replace_root(root).unwrap();
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
            root_id: RootId::fresh().unwrap(),
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
            .wrap_store_key(
                &StoreKey::from_bytes([0x3c; 32]),
                crate::custody::KeyLabel::STORE,
            )
            .unwrap();
        let root = custody.root();
        let label = custody.root_id();
        assert!(
            aead_open(
                &root,
                &wrapped.0,
                &crate::custody::associated_data(label, KeyLabel::STORE)
            )
            .is_ok(),
            "the wrapped store key does not open under its root"
        );
        let replacement = a_key_that_is_no_root();
        for (name, other) in [
            ("all zero", [0u8; 32]),
            ("all ones", [0xffu8; 32]),
            ("random", replacement),
        ] {
            assert!(
                aead_open(
                    &other,
                    &wrapped.0,
                    &crate::custody::associated_data(label, KeyLabel::STORE)
                )
                .is_err(),
                "the wrapped store key opens under a key that is not its root ({name})"
            );
        }
        custody.replace(&replacement);
        let under_replacement = custody.unwrap_store_key(&wrapped, crate::custody::KeyLabel::STORE);
        assert!(
            matches!(under_replacement, Err(CustodyError::WrapAuthFailed)),
            "with the root replaced under the same id, unwrap answered {:?}",
            under_replacement.err()
        );
        custody.replace(&root);
        let restored = custody
            .unwrap_store_key(&wrapped, crate::custody::KeyLabel::STORE)
            .unwrap();
        assert!(
            restored.expose() == &[0x3c; 32],
            "the restored root does not give back the store key"
        );
    }

    fn no_all_zero_root_and_no_other_store_root(first: &dyn RootHooks, second: &dyn RootHooks) {
        let _first = DestroyedAtTheEnd(first);
        let _second = DestroyedAtTheEnd(second);
        let wrapped = first
            .wrap_store_key(
                &StoreKey::from_bytes([0x3c; 32]),
                crate::custody::KeyLabel::STORE,
            )
            .unwrap();
        second
            .wrap_store_key(
                &StoreKey::from_bytes([0x4d; 32]),
                crate::custody::KeyLabel::STORE,
            )
            .unwrap();
        let (root, other_store_root) = (first.root(), second.root());
        let label = first.root_id();
        assert!(root.iter().any(|&b| b != 0), "the root is all zero");
        assert!(*root != *other_store_root, "two stores have one root");
        assert!(
            aead_open(
                &[0u8; 32],
                &wrapped.0,
                &crate::custody::associated_data(label, KeyLabel::STORE)
            )
            .is_err(),
            "the wrapped store key opens under an all-zero root"
        );
        assert!(
            aead_open(
                &other_store_root,
                &wrapped.0,
                &crate::custody::associated_data(label, KeyLabel::STORE)
            )
            .is_err(),
            "the wrapped store key opens under the root of a second store"
        );
        first.replace(&[0u8; 32]);
        assert!(
            first
                .unwrap_store_key(&wrapped, crate::custody::KeyLabel::STORE)
                .is_err(),
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
            .wrap_store_key(
                &StoreKey::from_bytes([0x3c; 32]),
                crate::custody::KeyLabel::STORE,
            )
            .unwrap();
        let first_root = first.root();
        first.destroy_root().unwrap();
        fresh.create_root().unwrap();
        let fresh_root = fresh.root();
        assert!(
            *first_root != *fresh_root,
            "two roots made under one root id hold the same bytes"
        );
        let from_the_id_alone = fresh.unwrap_store_key(&wrapped, crate::custody::KeyLabel::STORE);
        assert!(
            matches!(from_the_id_alone, Err(CustodyError::WrapAuthFailed)),
            "the wrapped store key and its public root id opened the store key under a new root: {:?}",
            from_the_id_alone.as_ref().map(|key| key.expose() == &[0x3c; 32])
        );
    }

    #[test]
    fn two_creates_under_one_root_id_make_two_roots_and_the_id_alone_opens_no_store_key() {
        {
            let dev = tempfile::tempdir().unwrap();
            let root_id = RootId::fresh().unwrap();
            a_root_made_again_under_one_id_differs_and_opens_no_wrap_of_the_first(
                &DevFallbackCustody::new(root_id, dev.path().join("roots")),
                &DevFallbackCustody::new(root_id, dev.path().join("roots")),
            );
        }
        let dir = tempfile::tempdir().unwrap();
        let root_id = RootId::fresh().unwrap();
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
        let dev = tempfile::tempdir().unwrap();
        no_all_zero_root_and_no_other_store_root(
            &DevFallbackCustody::new(RootId::fresh().unwrap(), dev.path().join("roots")),
            &DevFallbackCustody::new(RootId::fresh().unwrap(), dev.path().join("roots")),
        );
        let dir = tempfile::tempdir().unwrap();
        no_all_zero_root_and_no_other_store_root(
            &file_root_custody(dir.path()),
            &file_root_custody(dir.path()),
        );
    }

    #[test]
    fn the_store_key_wrap_opens_under_its_root_and_under_no_other_key() {
        let dev = tempfile::tempdir().unwrap();
        the_wrap_opens_under_its_root_and_no_other_key(&DevFallbackCustody::new(
            RootId::fresh().unwrap(),
            dev.path().join("roots"),
        ));
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
        let dev = tempfile::tempdir().unwrap();
        let mut wrong = Vec::new();
        for kind in every_custody_kind() {
            if !kind.is_acceptable_for_release() {
                continue;
            }
            if let Err(e) = crate::custody::custody_for(&crate::custody::CustodyLocator {
                kind,
                root_id: RootId::fresh().unwrap(),
                roots: dev.path().join("roots"),
            }) {
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
        let dev = tempfile::tempdir().unwrap();
        destroying_its_root_twice_is_no_error(&DevFallbackCustody::new(
            RootId::fresh().unwrap(),
            dev.path().join("roots"),
        ));
        #[cfg(target_os = "windows")]
        destroying_its_root_twice_is_no_error(&crate::custody::windows::DpapiCustody::new(
            RootId::fresh().unwrap(),
            dev.path().join("dpapi-roots"),
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
        let dev = tempfile::tempdir().unwrap();
        let custody = DevFallbackCustody::new(RootId::fresh().unwrap(), dev.path().join("roots"));
        assert!(matches!(
            reject_fallback_for_release(&custody),
            Err(CustodyError::CustodyKindUnsupported(
                CustodyKind::DevFallback
            ))
        ));
    }
}
