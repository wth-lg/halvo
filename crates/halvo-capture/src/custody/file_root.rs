//! Per-user protected root files: the platform root for a custody whose key
//! store protects blobs but cannot destroy a key per store (DPAPI). Each store's
//! root is a random 32-byte key in one file under a per-user directory,
//! protected by the platform; the store key is wrapped under that root. To
//! destroy the root is to scrub and unlink the file, after which no retained or
//! restored wrapped store key unwraps, exactly as deleting the macOS keychain
//! item does.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use super::dev_fallback::{aead_open, aead_seal, fill_random, store_key_from};
use super::{CustodyError, RootId, StoreKey, WrappedKey};

const ROOT_EXT: &str = "root";
const PARTIAL_EXT: &str = "root.tmp";

/// The platform primitive that keeps a root file readable by this user only.
pub(crate) trait Protector {
    fn protect(&self, plain: &[u8], label: &[u8]) -> Result<Vec<u8>, CustodyError>;
    fn unprotect(&self, blob: &[u8], label: &[u8]) -> Result<Zeroizing<Vec<u8>>, CustodyError>;
}

pub(crate) struct FileRoots<P> {
    dir: PathBuf,
    protector: P,
}

fn root_io(e: std::io::Error) -> CustodyError {
    CustodyError::RootUnavailable(format!("root file: {e}"))
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
        let plain = self.protector.unprotect(&blob, root_id.as_bytes())?;
        if plain.len() != 32 {
            return Err(CustodyError::RootUnavailable(
                "root file is not 32 bytes".to_string(),
            ));
        }
        let mut root = Zeroizing::new([0u8; 32]);
        root.copy_from_slice(&plain);
        Ok(Some(root))
    }

    fn existing_or_new_root(&self, root_id: RootId) -> Result<Zeroizing<[u8; 32]>, CustodyError> {
        if let Some(root) = self.existing_root(root_id)? {
            return Ok(root);
        }
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
        let root = self.existing_or_new_root(root_id)?;
        let sealed = aead_seal(&root, key.expose(), root_id.as_bytes())
            .map_err(|_| CustodyError::Crypto("sealing the store key failed".to_string()))?;
        Ok(WrappedKey(sealed))
    }

    pub(crate) fn unwrap(
        &self,
        root_id: RootId,
        wrapped: &WrappedKey,
    ) -> Result<StoreKey, CustodyError> {
        let root = self.existing_root(root_id)?.ok_or_else(|| {
            CustodyError::RootUnavailable(format!("no root file for store {root_id}"))
        })?;
        let plain = aead_open(&root, &wrapped.0, root_id.as_bytes())
            .map_err(|_| CustodyError::Crypto("unwrapping the store key failed".to_string()))?;
        store_key_from(plain)
    }

    /// Scrub and unlink the root file. Succeeds when it is already gone.
    pub(crate) fn destroy_root(&self, root_id: RootId) -> Result<(), CustodyError> {
        scrub_and_remove(&self.path(root_id))
    }

    #[cfg(test)]
    pub(crate) fn root_exists(&self, root_id: RootId) -> Result<bool, CustodyError> {
        self.existing_root(root_id).map(|root| root.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for the platform: a keyed byte transform that is not identity,
    /// so the tests see that the protector is applied and the label is bound.
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
            Err(CustodyError::RootUnavailable(_))
        ));
        roots.destroy_root(root_id).unwrap();
        assert!(!roots.root_exists(RootId::fresh()).unwrap());
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
            Err(CustodyError::Crypto(_))
        ));
        let other = FileRoots::new(dir.path().join("roots"), XorProtector(0x00));
        assert!(other.unwrap(a, &wrapped).is_err());
    }
}
