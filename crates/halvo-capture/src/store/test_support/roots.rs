use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::file_root::destroy_root_at;
use crate::custody::RootId;
use crate::store::db::{EncryptedStore, KeyFile};
use crate::store::StoreResult;

fn registry() -> &'static Mutex<HashMap<RootId, Arc<tempfile::TempDir>>> {
    static ROOTS: OnceLock<Mutex<HashMap<RootId, Arc<tempfile::TempDir>>>> = OnceLock::new();
    ROOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn directory_of(root_id: RootId) -> Arc<tempfile::TempDir> {
    let mut roots = registry().lock().unwrap();
    Arc::clone(
        roots
            .entry(root_id)
            .or_insert_with(|| Arc::new(tempfile::tempdir().expect("a roots directory"))),
    )
}

/// The root of a test store: a root id with a roots directory of its own,
/// made under the test's temporary directory. Dropping it destroys the root
/// and removes the directory. A custody that a test makes for the id before
/// the guard uses the same directory.
pub(crate) struct RootGuard {
    id: RootId,
    dir: Arc<tempfile::TempDir>,
}

impl RootGuard {
    pub(crate) fn new(root_id: RootId) -> Self {
        Self {
            id: root_id,
            dir: directory_of(root_id),
        }
    }

    pub(crate) fn fresh() -> Self {
        Self::new(RootId::fresh().expect("a fresh root id"))
    }

    pub(crate) fn id(&self) -> RootId {
        self.id
    }

    pub(crate) fn roots(&self) -> PathBuf {
        self.dir.path().join("roots")
    }

    #[cfg(unix)]
    pub(crate) fn custody(&self) -> DevFallbackCustody {
        DevFallbackCustody::new(self.id, self.roots())
    }
}

impl Drop for RootGuard {
    fn drop(&mut self) {
        registry().lock().unwrap().remove(&self.id);
        let destroyed = destroy_root_at(&self.roots(), self.id);
        if !std::thread::panicking() {
            destroyed.expect("the root of the test is destroyed when the test ends");
        }
    }
}

/// The roots directory of `root_id`: that of its guard, made for the id when
/// no guard holds it yet.
pub(crate) fn roots_of(root_id: RootId) -> PathBuf {
    if let Some(dir) = registry().lock().unwrap().get(&root_id) {
        return dir.path().join("roots");
    }
    match std::env::var_os(ROOTS_OF_THE_PARENT) {
        Some(roots) => PathBuf::from(roots),
        None => directory_of(root_id).path().join("roots"),
    }
}

/// The variable that hands a child test process the roots directory of the
/// store that its parent made.
pub(crate) const ROOTS_OF_THE_PARENT: &str = "HALVO_TEST_ROOTS_DIR";

/// The development custody of `root_id` under the roots directory of its
/// live guard.
pub(crate) fn custody(root_id: RootId) -> DevFallbackCustody {
    DevFallbackCustody::new(root_id, roots_of(root_id))
}

/// The roots directory of the store in `dir`: that of the guard of the root
/// that its key file names.
pub(crate) fn roots_for_store(dir: &Path) -> PathBuf {
    match KeyFile::at(&dir.join("store.key"))
        .ok()
        .and_then(|place| place.named_root())
    {
        Some(root_id) => {
            let known = registry()
                .lock()
                .unwrap()
                .get(&root_id)
                .map(|dir| dir.path().join("roots"));
            match (known, std::env::var_os(ROOTS_OF_THE_PARENT)) {
                (Some(roots), _) => roots,
                (None, Some(roots)) => PathBuf::from(roots),
                (None, None) => dir.join("no-roots-directory-for-this-store"),
            }
        }
        None => dir.join("no-roots-directory-for-this-store"),
    }
}

/// `EncryptedStore::open` with the roots directory of the guard of the root
/// that the key file names.
pub(crate) fn open(dir: &Path, retention: Duration) -> StoreResult<EncryptedStore> {
    EncryptedStore::open(dir, &roots_for_store(dir), retention)
}
