use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::custody::dev_fallback::DevFallbackCustody;
use crate::custody::file_root::destroy_root_at;
use crate::custody::RootId;
use crate::store::db::{EncryptedStore, KeyFile};
use crate::store::StoreResult;

fn registry() -> &'static Mutex<HashMap<RootId, PathBuf>> {
    static ROOTS: OnceLock<Mutex<HashMap<RootId, PathBuf>>> = OnceLock::new();
    ROOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

const STORE_TEST_ROOTS: &str = "store-test-roots";

fn roots_base_outside_the_checked_temp_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("the test executable's path");
    let target = exe
        .ancestors()
        .nth(3)
        .expect("the test executable lies in <target>/<profile>/deps");
    target.join("tmp").join(STORE_TEST_ROOTS)
}

fn directory_of(root_id: RootId) -> PathBuf {
    let mut roots = registry().lock().unwrap();
    roots
        .entry(root_id)
        .or_insert_with(|| {
            let dir = roots_base_outside_the_checked_temp_dir().join(root_id.to_string());
            std::fs::create_dir_all(&dir).expect("a roots directory for the test");
            dir
        })
        .clone()
}

pub(crate) struct RootGuard {
    id: RootId,
    dir: PathBuf,
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
        self.dir.join("roots")
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

pub(crate) fn roots_of(root_id: RootId) -> PathBuf {
    if let Some(dir) = registry().lock().unwrap().get(&root_id) {
        return dir.join("roots");
    }
    match std::env::var_os(ROOTS_OF_THE_PARENT) {
        Some(roots) => PathBuf::from(roots),
        None => directory_of(root_id).join("roots"),
    }
}

pub(crate) const ROOTS_OF_THE_PARENT: &str = "HALVO_TEST_ROOTS_DIR";

pub(crate) fn custody(root_id: RootId) -> DevFallbackCustody {
    DevFallbackCustody::new(root_id, roots_of(root_id))
}

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
                .map(|dir| dir.join("roots"));
            match (known, std::env::var_os(ROOTS_OF_THE_PARENT)) {
                (Some(roots), _) => roots,
                (None, Some(roots)) => PathBuf::from(roots),
                (None, None) => dir.join("no-roots-directory-for-this-store"),
            }
        }
        None => dir.join("no-roots-directory-for-this-store"),
    }
}

pub(crate) fn open(dir: &Path, retention: Duration) -> StoreResult<EncryptedStore> {
    EncryptedStore::open(dir, &roots_for_store(dir), retention)
}
