#![cfg(debug_assertions)]

use halvo_secrets::{
    Delete, DeleteIfHolds, Directory, Name, Read, Secret, Store, StoreKind, Write,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const HELD: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn scratch(label: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(base).expect("the target's temporary directory");
    loop {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = base.join(format!(
            "halvo-secrets-ops-{}-{label}-{n}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return path.join("store"),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("a scratch directory: {error}"),
        }
    }
}

fn store_in(dir: &Path) -> Store {
    Store::in_directory(
        Directory::new(dir.to_path_buf()).expect("an absolute directory"),
        Name::new("account").expect("a name"),
    )
}

fn secret(bytes: &[u8]) -> Secret {
    Secret::new(bytes.to_vec()).expect("a secret")
}

fn found(read: Read) -> Vec<u8> {
    match read {
        Read::Found(secret) => secret.expose().to_vec(),
        other => panic!("read answered {other:?}"),
    }
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("the store directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn one_byte_off(at: usize) -> Vec<u8> {
    let mut other = HELD.to_vec();
    other[at] ^= 1;
    other
}

fn kind_of_this_store() -> StoreKind {
    if cfg!(windows) {
        StoreKind::Dpapi
    } else {
        StoreKind::File
    }
}

#[test]
fn a_delete_if_holds_removes_only_the_bytes_that_its_caller_read() {
    let dir = scratch("holds");
    let store = store_in(&dir);
    assert!(matches!(
        store.delete_if_holds(&secret(HELD)),
        DeleteIfHolds::NotStored
    ));
    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    for at in [0, HELD.len() / 2, HELD.len() - 1] {
        match store.delete_if_holds(&secret(&one_byte_off(at))) {
            DeleteIfHolds::Other(kept) => assert_eq!(kept.expose(), HELD),
            answer => panic!("the delete answered {answer:?}"),
        }
        assert_eq!(found(store.read()), HELD);
        assert_eq!(names_in(&dir), ["account"]);
    }
    assert!(matches!(
        store.delete_if_holds(&secret(HELD)),
        DeleteIfHolds::Deleted
    ));
    assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
    assert!(matches!(store.read(), Read::NotStored));
}

fn left_claim(dir: &Path) {
    fs::rename(
        dir.join("account"),
        dir.join(format!(".account.{}.0.claim", std::process::id() + 1)),
    )
    .expect("the claim of a stopped delete");
}

#[test]
fn a_claim_that_a_stopped_delete_left_is_put_back_by_the_next_operation() {
    for op in ["read", "write", "delete", "delete-if-holds"] {
        let dir = scratch("left-claim");
        let store = store_in(&dir);
        assert_eq!(store.write(&secret(HELD)), Write::Stored);
        left_claim(&dir);
        let answer = match op {
            "read" => format!("{:?}", store.read()),
            "write" => format!("{:?}", store.write(&secret(b"a new sign-in"))),
            "delete" => format!("{:?}", store.delete()),
            _ => format!("{:?}", store.delete_if_holds(&secret(&one_byte_off(1)))),
        };
        let want = match op {
            "read" => "Found(..)",
            "write" => "Occupied",
            "delete" => "Deleted",
            _ => "Other(..)",
        };
        assert_eq!(answer, want, "{op}");
        let names = names_in(&dir);
        assert!(
            !names.iter().any(|name| name.ends_with(".claim")),
            "{op}: {names:?}"
        );
        if matches!(op, "read" | "write" | "delete-if-holds") {
            assert_eq!(found(store.read()), HELD, "{op}");
        } else {
            assert!(names.is_empty(), "{op}: {names:?}");
        }
    }
}

#[test]
fn a_removal_names_the_store_of_this_place_alone_and_then_reads() {
    let dir = scratch("removal");
    let store = store_in(&dir);
    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    for kind in [StoreKind::Keychain, StoreKind::File, StoreKind::Dpapi] {
        if kind != kind_of_this_store() {
            assert!(store.remove(kind).is_none(), "{kind:?}");
            assert_eq!(found(store.read()), HELD, "{kind:?}");
        }
    }
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::Deleted);
    assert!(
        matches!(removal.then, Read::NotStored),
        "{:?}",
        removal.then
    );
    assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::NotStored);
    assert!(matches!(removal.then, Read::NotStored));

    assert_eq!(store.write(&secret(HELD)), Write::Stored);
    left_claim(&dir);
    let removal = store
        .remove(kind_of_this_store())
        .expect("the store of this place");
    assert_eq!(removal.removed, Delete::Deleted);
    assert!(matches!(removal.then, Read::NotStored));
    assert!(names_in(&dir).is_empty(), "{:?}", names_in(&dir));
}
