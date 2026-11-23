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

/// The deletion's promise when a pass fails behind its shred: a real store of two segments, the second captured an
/// hour after the first. The first pass of the expiry, with the removal of a segment file failing, answers an error
/// although it has cleared and committed the first segment's key, and the cell's list names that ordinal. The
/// second, with that file still owed and its removal failing once more, shreds the second segment and answers the
/// owed error, and the list names that ordinal too. The cell is this test thread's own (`cursor`, in the crate's
/// tests), so no other test feeds it or empties it.
#[cfg(feature = "r2")]
#[test]
fn a_pass_that_answers_an_error_after_its_shred_hands_its_ordinals_on() {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::daemon::maintenance::{Job, Schedule, SHRED_CHUNK};
    use crate::daemon::port::tests::write_at;
    use crate::daemon::r2::daemon_caller::DaemonCaller;
    use crate::daemon::r2::owner_slot::Caller;

    const SECOND: i64 = 1_000_000_000;
    let dir = tempfile::tempdir().unwrap();
    let Opened::ReadWrite { mut store, .. } = a_store(dir.path()) else {
        unreachable!("a_store answers a store open for writing")
    };
    let noon = (20_000 * 86_400 + 43_200) * SECOND;
    let first = write_at(&mut store, noon, b"{\"n\":1}\n");
    let second = write_at(&mut store, noon + 3_600 * SECOND, b"{\"n\":2}\n");
    let after_the_window = |seconds: i64| {
        let at = noon / SECOND + 14 * 86_400 + seconds;
        UNIX_EPOCH + Duration::from_secs(u64::try_from(at).unwrap())
    };
    let schedule = Schedule::new(SHRED_CHUNK, None);
    let caller = DaemonCaller::new(dir.path(), Box::new(std::io::sink()));
    assert!(caller.shredded().is_empty());

    super::failpoint::arm("segment_file_remove");
    let now = after_the_window(1_800);
    let first_pass = schedule.run(&mut store, Job::Expire { now }).is_err();
    let first_list = caller.shredded();
    super::failpoint::arm("segment_file_remove");
    let now = after_the_window(5_400);
    let second_pass = schedule.run(&mut store, Job::Expire { now }).is_err();
    let second_list = caller.shredded();
    assert_eq!(
        (first_pass, first_list, second_pass, second_list),
        (
            true,
            vec![first.ordinal.get()],
            true,
            vec![second.ordinal.get()]
        ),
        "(whether each pass answered an error, and the ordinals that the cell's list then named)"
    );
}
