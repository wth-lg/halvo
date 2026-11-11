use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use super::dev_fallback::{aead_open, aead_seal, fill_random, store_key_from};
use super::{CustodyError, RootId, StoreKey, WrappedKey};

const ROOT_EXT: &str = "root";
const PARTIAL_EXT: &str = "root.tmp";

pub(crate) trait Protector {
    fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError>;
    fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError>;
}

pub(crate) struct FileRoots<P> {
    dir: PathBuf,
    protector: P,
}

fn root_io(e: std::io::Error) -> CustodyError {
    match e.kind() {
        ErrorKind::PermissionDenied => CustodyError::RootDenied,
        _ => CustodyError::Os(e.raw_os_error()),
    }
}

fn write_complete(partial: &Path, path: &Path, blob: &[u8]) -> Result<(), CustodyError> {
    let written = std::fs::File::create(partial)
        .and_then(|mut file| {
            file.write_all(blob)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(partial, path));
    if written.is_err() {
        let _ = std::fs::remove_file(partial);
    }
    written.map_err(root_io)
}

fn scrub_and_remove(path: &Path) -> Result<(), CustodyError> {
    match std::fs::File::options().write(true).open(path) {
        Ok(mut file) => {
            let len = file.metadata().map_err(root_io)?.len() as usize;
            file.write_all(&vec![0u8; len]).map_err(root_io)?;
            file.sync_all().map_err(root_io)?;
        }
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(root_io(e)),
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(root_io(e)),
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

impl<P: Protector> FileRoots<P> {
    pub(crate) fn new(dir: PathBuf, protector: P) -> Self {
        Self { dir, protector }
    }

    fn path(&self, root_id: RootId) -> PathBuf {
        self.dir.join(format!("{root_id}.{ROOT_EXT}"))
    }

    fn existing_root(&self, root_id: RootId) -> Result<Option<Zeroizing<[u8; 32]>>, CustodyError> {
        let blob = match std::fs::read(self.path(root_id)) {
            Ok(blob) => blob,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(root_io(e)),
        };
        if blob.iter().all(|&b| b == 0) {
            return Ok(None);
        }
        let plain = self.protector.unprotect(&blob, root_id.as_bytes())?;
        if plain.len() != 32 {
            return Err(CustodyError::RootCorrupt);
        }
        let mut root = Zeroizing::new([0u8; 32]);
        root.copy_from_slice(&plain);
        Ok(Some(root))
    }

    fn existing_or_new_root(&self, root_id: RootId) -> Result<Zeroizing<[u8; 32]>, CustodyError> {
        match self.existing_root(root_id)? {
            Some(root) => Ok(root),
            None => self.make_root(root_id),
        }
    }

    fn new_root(&self, root_id: RootId) -> Result<Zeroizing<[u8; 32]>, CustodyError> {
        match self.existing_root(root_id)? {
            Some(_) => Err(CustodyError::RootExists),
            None => self.make_root(root_id),
        }
    }

    fn make_root(&self, root_id: RootId) -> Result<Zeroizing<[u8; 32]>, CustodyError> {
        let mut root = Zeroizing::new([0u8; 32]);
        fill_random(root.as_mut_slice())?;
        let blob = self
            .protector
            .protect(root.as_slice(), root_id.as_bytes())?;
        std::fs::create_dir_all(&self.dir).map_err(root_io)?;
        let path = self.path(root_id);
        write_complete(&path.with_extension(PARTIAL_EXT), &path, &blob)?;
        Ok(root)
    }

    pub(crate) fn wrap(&self, root_id: RootId, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
        seal(root_id, self.existing_or_new_root(root_id)?, key)
    }

    pub(crate) fn wrap_under_new_root(
        &self,
        root_id: RootId,
        key: &StoreKey,
    ) -> Result<WrappedKey, CustodyError> {
        seal(root_id, self.new_root(root_id)?, key)
    }

    pub(crate) fn unwrap(
        &self,
        root_id: RootId,
        wrapped: &WrappedKey,
    ) -> Result<StoreKey, CustodyError> {
        let root = self
            .existing_root(root_id)?
            .ok_or(CustodyError::RootMissing)?;
        let plain = aead_open(&root, &wrapped.0, root_id.as_bytes())
            .map_err(|_| CustodyError::WrapAuthFailed)?;
        store_key_from(plain)
    }

    pub(crate) fn destroy_root(&self, root_id: RootId) -> Result<(), CustodyError> {
        scrub_and_remove(&self.path(root_id))
    }

    #[cfg(test)]
    pub(crate) fn root_exists(&self, root_id: RootId) -> Result<bool, CustodyError> {
        self.existing_root(root_id).map(|root| root.is_some())
    }

    #[cfg(test)]
    pub(crate) fn root_path(&self, root_id: RootId) -> PathBuf {
        self.path(root_id)
    }

    #[cfg(test)]
    pub(crate) fn root_bytes(
        &self,
        root_id: RootId,
    ) -> Result<Option<Zeroizing<[u8; 32]>>, CustodyError> {
        self.existing_root(root_id)
    }

    #[cfg(test)]
    pub(crate) fn replace_root(
        &self,
        root_id: RootId,
        root: &[u8; 32],
    ) -> Result<(), CustodyError> {
        let blob = self.protector.protect(root, root_id.as_bytes())?;
        std::fs::create_dir_all(&self.dir).map_err(root_io)?;
        let path = self.path(root_id);
        write_complete(&path.with_extension(PARTIAL_EXT), &path, &blob)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use zeroize::Zeroizing;

    use super::{FileRoots, Protector};
    use crate::custody::{CustodyError, CustodyKind, KeyCustody, RootId, StoreKey, WrappedKey};

    pub(crate) struct TestProtector {
        mask: u8,
        unprotects: Arc<AtomicUsize>,
        fail_next_unprotect: Mutex<Option<CustodyError>>,
    }

    impl TestProtector {
        pub(crate) fn new(mask: u8) -> (Self, Arc<AtomicUsize>) {
            let unprotects = Arc::new(AtomicUsize::new(0));
            let protector = Self {
                mask,
                unprotects: Arc::clone(&unprotects),
                fail_next_unprotect: Mutex::new(None),
            };
            (protector, unprotects)
        }

        pub(crate) fn failing_once(mask: u8, error: CustodyError) -> (Self, Arc<AtomicUsize>) {
            let (protector, unprotects) = Self::new(mask);
            *protector.fail_next_unprotect.lock().unwrap() = Some(error);
            (protector, unprotects)
        }

        fn transform(&self, bytes: &[u8], label: &[u8]) -> Vec<u8> {
            bytes
                .iter()
                .zip(label.iter().cycle())
                .map(|(b, l)| b ^ l ^ self.mask)
                .collect()
        }
    }

    impl Protector for TestProtector {
        fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError> {
            Ok(self.transform(plain, label))
        }

        fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
            self.unprotects.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = self.fail_next_unprotect.lock().unwrap().take() {
                return Err(error);
            }
            Ok(Zeroizing::new(self.transform(blob, label)))
        }
    }

    impl<P: Protector> Protector for Arc<P> {
        fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError> {
            P::protect(self, plain, label)
        }

        fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
            P::unprotect(self, blob, label)
        }
    }

    pub(crate) struct FileRootCustody<P> {
        pub(crate) roots: FileRoots<P>,
        pub(crate) root_id: RootId,
    }

    impl<P: Protector> KeyCustody for FileRootCustody<P> {
        fn kind(&self) -> CustodyKind {
            CustodyKind::Dpapi
        }

        fn root_id(&self) -> RootId {
            self.root_id
        }

        fn wrap_store_key(&self, key: &StoreKey) -> Result<WrappedKey, CustodyError> {
            self.roots.wrap(self.root_id, key)
        }

        fn wrap_store_key_under_new_root(
            &self,
            key: &StoreKey,
        ) -> Result<WrappedKey, CustodyError> {
            self.roots.wrap_under_new_root(self.root_id, key)
        }

        fn unwrap_store_key(&self, wrapped: &WrappedKey) -> Result<StoreKey, CustodyError> {
            self.roots.unwrap(self.root_id, wrapped)
        }

        fn destroy_root(&self) -> Result<(), CustodyError> {
            self.roots.destroy_root(self.root_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct XorProtector(u8);

    impl Protector for XorProtector {
        fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError> {
            Ok(plain
                .iter()
                .zip(label.iter().cycle())
                .map(|(b, l)| b ^ l ^ self.0)
                .collect())
        }
        fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError> {
            self.protect(blob, label).map(Zeroizing::new)
        }
    }

    fn roots(dir: &Path) -> FileRoots<XorProtector> {
        FileRoots::new(dir.join("roots"), XorProtector(0x5a))
    }

    #[test]
    fn wrap_unwrap_roundtrips_and_the_root_file_holds_no_raw_root() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh();
        let key = StoreKey::from_bytes([42u8; 32]);
        let wrapped = roots.wrap(root_id, &key).unwrap();
        assert_eq!(
            roots.unwrap(root_id, &wrapped).unwrap().expose(),
            &[42u8; 32]
        );
        assert!(roots.root_exists(root_id).unwrap());
        let file = std::fs::read(roots.path(root_id)).unwrap();
        let raw = roots.existing_root(root_id).unwrap().unwrap();
        assert_ne!(file.as_slice(), raw.as_slice());
        assert!(!dir
            .path()
            .join("roots")
            .join(format!("{root_id}.root.tmp"))
            .exists());
    }

    #[test]
    fn destroying_the_root_makes_the_wrapped_key_unusable_and_is_safe_twice() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh();
        let wrapped = roots
            .wrap(root_id, &StoreKey::from_bytes([7u8; 32]))
            .unwrap();
        roots.destroy_root(root_id).unwrap();
        assert!(!roots.root_exists(root_id).unwrap());
        assert!(matches!(
            roots.unwrap(root_id, &wrapped),
            Err(CustodyError::RootMissing)
        ));
        roots.destroy_root(root_id).unwrap();
        assert!(!roots.root_exists(RootId::fresh()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_in_place_of_the_root_file_is_an_os_error_for_unwrap_and_destroy() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let root_id = RootId::fresh();
        let wrapped = roots
            .wrap(root_id, &StoreKey::from_bytes([5u8; 32]))
            .unwrap();
        let path = roots.path(root_id);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let unwrapped = roots.unwrap(root_id, &wrapped);
        assert!(
            matches!(unwrapped, Err(CustodyError::Os(Some(_)))),
            "unwrap answered {:?}",
            unwrapped.err()
        );
        let destroyed = roots.destroy_root(root_id);
        assert!(
            matches!(destroyed, Err(CustodyError::Os(Some(_)))),
            "destroy answered {destroyed:?}"
        );
    }

    #[test]
    fn a_key_wrapped_under_one_root_does_not_unwrap_under_another() {
        let dir = tempfile::tempdir().unwrap();
        let roots = roots(dir.path());
        let (a, b) = (RootId::fresh(), RootId::fresh());
        let wrapped = roots.wrap(a, &StoreKey::from_bytes([1u8; 32])).unwrap();
        roots.wrap(b, &StoreKey::from_bytes([2u8; 32])).unwrap();
        assert!(matches!(
            roots.unwrap(b, &wrapped),
            Err(CustodyError::WrapAuthFailed)
        ));
        let other = FileRoots::new(dir.path().join("roots"), XorProtector(0x00));
        assert!(other.unwrap(a, &wrapped).is_err());
    }
}
