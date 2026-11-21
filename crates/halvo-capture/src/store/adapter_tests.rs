use std::collections::BTreeSet;
use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::custody::{custody_for, CustodyKind, CustodyLocator, KeyLabel, RootId, WrappedKey};
use crate::daemon::custody_policy::{Custody, System};
use crate::daemon::port::{DataDirStore, Opened, StoreHost};

fn derived() -> KeyLabel {
    KeyLabel::new("derived").unwrap()
}

fn own_custody() -> Custody {
    match System::this() {
        System::Macos => Custody::RootFile,
        System::Windows => Custody::Dpapi,
    }
}

fn a_store(dir: &Path) -> Opened {
    let made = DataDirStore::at(dir).create(own_custody());
    assert!(
        matches!(made, Opened::ReadWrite { .. }),
        "no store was made"
    );
    made
}

fn fields(file: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

#[test]
fn the_first_ask_makes_the_derived_key_under_the_stores_root() {
    let dir = tempfile::tempdir().unwrap();
    let _held = a_store(dir.path());
    assert!(!dir.path().join("derived").exists());

    let key = DataDirStore::at(dir.path()).derived_store_key().unwrap();

    let file = dir.path().join("derived").join("master.key");
    let names: BTreeSet<_> = std::fs::read_dir(dir.path().join("derived"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, BTreeSet::from(["master.key".into()]));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&dir.path().join("derived")), 0o700);
    }
    let written = fields(&file);
    let store = fields(&dir.path().join("store").join("store.key"));
    assert_eq!(written["version"], store["version"]);
    assert_eq!(written["custody_kind"], store["custody_kind"]);
    assert_eq!(written["root_id"], store["root_id"]);
    assert_eq!(written["namespace"], "derived");
    let custody = custody_for(&CustodyLocator {
        kind: serde_json::from_value::<CustodyKind>(store["custody_kind"].clone()).unwrap(),
        root_id: RootId::parse(store["root_id"].as_str().unwrap()).unwrap(),
        roots: dir.path().join("roots"),
    })
    .unwrap();
    let wrapped = WrappedKey(
        BASE64
            .decode(written["wrapped_key"].as_str().unwrap())
            .unwrap(),
    );
    assert_eq!(
        custody
            .unwrap_store_key(&wrapped, derived())
            .unwrap()
            .expose(),
        key.expose()
    );
    assert!(custody.unwrap_store_key(&wrapped, KeyLabel::STORE).is_err());
}
