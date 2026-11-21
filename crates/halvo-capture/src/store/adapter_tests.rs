use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::custody::{custody_for, CustodyKind, CustodyLocator, KeyLabel, RootId, WrappedKey};
use crate::daemon::custody_policy::{Custody, System};
use crate::daemon::port::{DataDirStore, DerivedKeyFault, Opened, StoreHost};

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

fn names_under(dir: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut open = vec![dir.to_path_buf()];
    while let Some(at) = open.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            found.insert(path.strip_prefix(dir).unwrap().to_path_buf());
            if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                open.push(path);
            }
        }
    }
    found
}

fn with_field(file: &Path, name: &str, value: &str) -> Vec<u8> {
    let mut changed = fields(file);
    changed[name] = value.into();
    let bytes = serde_json::to_vec_pretty(&changed).unwrap();
    std::fs::write(file, &bytes).unwrap();
    bytes
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

#[test]
fn a_later_ask_answers_the_same_key_and_refuses_another_namespace() {
    let dir = tempfile::tempdir().unwrap();
    let _held = a_store(dir.path());
    let first = DataDirStore::at(dir.path()).derived_store_key().unwrap();
    let file = dir.path().join("derived").join("master.key");
    let written = std::fs::read(&file).unwrap();

    let again = DataDirStore::at(dir.path()).derived_store_key().unwrap();

    assert_eq!(again.expose(), first.expose());
    assert_eq!(std::fs::read(&file).unwrap(), written);

    let relabelled = with_field(&file, "namespace", "store");

    let fault = DataDirStore::at(dir.path()).derived_store_key().err();

    assert_eq!(fault, Some(DerivedKeyFault::DerivedKeyFile));
    assert_eq!(std::fs::read(&file).unwrap(), relabelled);
    let wrapped_text = fields(&file)["wrapped_key"].as_str().unwrap().to_owned();
    let fault = fault.unwrap();
    for shown in [fault.to_string(), format!("{fault:?}")] {
        for run in first.expose().windows(4) {
            let lower: String = run.iter().map(|byte| format!("{byte:02x}")).collect();
            let listed = format!("{}, {}, {}, {}", run[0], run[1], run[2], run[3]);
            assert!(!shown.to_lowercase().contains(&lower), "{shown}");
            assert!(!shown.contains(&listed), "{shown}");
        }
        for run in wrapped_text.as_bytes().windows(8) {
            assert!(
                !shown.as_bytes().windows(8).any(|seen| seen == run),
                "{shown}"
            );
        }
    }
}

fn nothing_made(dir: &Path) -> DerivedKeyFault {
    let before = names_under(dir);
    let fault = DataDirStore::at(dir).derived_store_key().err();
    assert_eq!(names_under(dir), before);
    fault.expect("an ask without the store's root answers no key")
}

#[test]
fn no_key_and_nothing_is_made_without_the_stores_root() {
    let no_store = tempfile::tempdir().unwrap();
    let garbled = tempfile::tempdir().unwrap();
    drop(a_store(garbled.path()));
    std::fs::write(
        garbled.path().join("store").join("store.key"),
        b"not a key file",
    )
    .unwrap();
    let rootless = tempfile::tempdir().unwrap();
    drop(a_store(rootless.path()));
    let taken = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(rootless.path().join("roots")).unwrap() {
        let entry = entry.unwrap();
        std::fs::rename(entry.path(), taken.path().join(entry.file_name())).unwrap();
    }
    let enclave = tempfile::tempdir().unwrap();
    drop(a_store(enclave.path()));
    with_field(
        &enclave.path().join("store").join("store.key"),
        "custody_kind",
        "secure_enclave",
    );

    let faults = [
        nothing_made(no_store.path()),
        nothing_made(garbled.path()),
        nothing_made(rootless.path()),
        nothing_made(enclave.path()),
    ];

    assert_eq!(
        faults,
        [
            DerivedKeyFault::NoStore,
            DerivedKeyFault::StoreKeyFile,
            DerivedKeyFault::RootMissing,
            DerivedKeyFault::CustodyKind,
        ]
    );
}

#[cfg(unix)]
#[test]
fn an_ask_through_a_linked_data_directory_makes_the_key() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("data");
    std::fs::create_dir(&target).unwrap();
    let linked = root.path().join("linked");
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    let _held = a_store(&linked);

    let first = DataDirStore::at(&linked).derived_store_key().unwrap();

    let file = target.join("derived").join("master.key");
    assert!(std::fs::symlink_metadata(&file).unwrap().is_file());
    assert_eq!(fields(&file)["namespace"], "derived");
    let again = DataDirStore::at(&linked).derived_store_key().unwrap();
    assert_eq!(again.expose(), first.expose());
}
