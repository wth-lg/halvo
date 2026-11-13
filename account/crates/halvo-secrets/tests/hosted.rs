use halvo_secrets::{Delete, Directory, Name, Read, Secret, Store, StoreKind, Write};
use std::path::{Path, PathBuf};

const SERVICE: &str = "dev.halvo.secrets.hosted-test";

fn on_a_hosted_runner() {
    let actions = std::env::var("GITHUB_ACTIONS").unwrap_or_default();
    let environment = std::env::var("RUNNER_ENVIRONMENT").unwrap_or_default();
    assert!(
        actions == "true" && environment == "github-hosted",
        "the hosted store tests run only where GITHUB_ACTIONS=true and RUNNER_ENVIRONMENT=github-hosted"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(
        std::env::var("HOME").unwrap_or_default(),
        "/Users/runner",
        "the hosted store tests run only as the user of the hosted macOS runner"
    );
}

fn dir(label: &str) -> PathBuf {
    let parent = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "halvo-secrets-hosted-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir_all(&parent).expect("the parent of the store directory");
    parent.join("store")
}

fn name(label: &str) -> Name {
    Name::new(&format!("hosted-{label}-{}", std::process::id())).expect("a name")
}

fn store(label: &str) -> Store {
    Store::new(
        Directory::new(dir(label)).expect("an absolute directory"),
        Name::new(SERVICE).expect("a service"),
        name(label),
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

fn the_kind_of_this_build() -> StoreKind {
    if cfg!(windows) {
        StoreKind::Dpapi
    } else if cfg!(feature = "keychain") {
        StoreKind::Keychain
    } else {
        StoreKind::File
    }
}

#[test]
fn the_store_of_this_build_stores_reads_and_deletes_one_item() {
    on_a_hosted_runner();
    let store = store("cycle");
    let kind = store.kind();
    assert_eq!(kind, the_kind_of_this_build());
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    assert_eq!(store.write(&secret(b"hosted secret one")), Write::Stored);
    assert_eq!(store.write(&secret(b"hosted secret two")), Write::Occupied);
    assert_eq!(found(store.read()), b"hosted secret one");
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    println!("\nstore={}", kind.as_str());
}

#[cfg(target_os = "macos")]
mod keychain_items {
    use super::*;
    use security_framework::item::{ItemClass, ItemSearchOptions};
    use security_framework::os::macos::keychain::SecKeychain;

    pub(crate) fn add(account: &Name, bytes: &[u8]) {
        SecKeychain::default()
            .expect("the default keychain")
            .add_generic_password(SERVICE, account.as_str(), bytes)
            .expect("an item is added");
    }

    pub(crate) fn count(account: &Name) -> usize {
        let mut options = ItemSearchOptions::new();
        options
            .class(ItemClass::generic_password())
            .service(SERVICE)
            .account(account.as_str())
            .load_attributes(true)
            .limit(security_framework::item::Limit::All);
        match options.search() {
            Ok(found) => found.len(),
            Err(error) if error.code() == -25300 => 0,
            Err(error) => panic!("the search failed with {}", error.code()),
        }
    }

    #[cfg(not(feature = "keychain"))]
    pub(crate) fn remove(account: &Name) {
        let mut options = ItemSearchOptions::new();
        options
            .class(ItemClass::generic_password())
            .service(SERVICE)
            .account(account.as_str());
        options.delete().expect("the item is deleted");
    }
}

#[cfg(all(target_os = "macos", debug_assertions))]
fn file_store(label: &str) -> Store {
    Store::in_directory(
        Directory::new(dir(label)).expect("an absolute directory"),
        name(label),
    )
}

#[cfg(all(target_os = "macos", debug_assertions, not(feature = "keychain")))]
mod beside_the_keychain {
    use super::*;
    use halvo_secrets::Locked;

    #[test]
    fn a_keychain_item_read_by_a_build_without_the_feature_answers_locked() {
        on_a_hosted_runner();
        let label = "beside";
        let store = store(label);
        assert_eq!(store.kind(), StoreKind::File);
        assert!(matches!(store.read(), Read::NotStored));
        keychain_items::add(&name(label), b"kept by a build with the feature");
        let read = store.read();
        assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
        assert_eq!(
            store.write(&secret(b"a new sign-in")),
            Write::Locked(Locked::OtherStore)
        );
        let file = file_store(label);
        assert!(matches!(file.read(), Read::NotStored));
        assert_eq!(file.write(&secret(b"in the file")), Write::Stored);
        let both = store.read();
        assert!(matches!(both, Read::Locked(Locked::OtherStore)), "{both:?}");
        assert_eq!(found(file.read()), b"in the file");
        assert_eq!(store.delete(), Delete::Locked(Locked::OtherStore));
        assert_eq!(keychain_items::count(&name(label)), 1);
        assert_eq!(found(file.read()), b"in the file");
        assert_eq!(file.delete(), Delete::Deleted);
        keychain_items::remove(&name(label));
        assert!(matches!(store.read(), Read::NotStored));
        println!("\na keychain item read by a build without the feature answers {read:?}, and {both:?} beside a file");
    }

    #[test]
    fn the_store_of_a_build_without_the_keychain_is_the_file_store() {
        on_a_hosted_runner();
        let label = "file-kind";
        let place = dir(label);
        let store = store(label);
        assert_eq!(store.kind(), StoreKind::File);
        assert_eq!(store.write(&secret(b"secret")), Write::Stored);
        assert_eq!(
            std::fs::read(place.join(name(label).as_str())).expect("the file"),
            b"secret"
        );
        assert_eq!(keychain_items::count(&name(label)), 0);
        assert_eq!(found(store.read()), b"secret");
        assert_eq!(store.delete(), Delete::Deleted);
        assert_eq!(std::fs::read_dir(&place).expect("the directory").count(), 0);
    }
}

#[cfg(all(target_os = "macos", debug_assertions, feature = "keychain"))]
mod moves {
    use super::*;
    use halvo_secrets::{Fault, Locked, MoveFault};
    use std::os::unix::fs::PermissionsExt;

    const EARLIER: &[u8] = b"kept by an unsigned build";

    fn set_mode(place: &Path, mode: u32) {
        std::fs::set_permissions(place, std::fs::Permissions::from_mode(mode))
            .expect("the mode of the store directory");
    }

    #[test]
    fn a_delete_beside_a_file_that_cannot_be_read_answers_a_fault_and_keeps_the_file() {
        on_a_hosted_runner();
        let label = "unreadable";
        let store = store(label);
        let file = file_store(label);
        assert_eq!(file.write(&secret(EARLIER)), Write::Stored);
        keychain_items::add(&name(label), EARLIER);
        let place = dir(label);
        set_mode(&place, 0o000);
        let delete = store.delete();
        set_mode(&place, 0o700);
        assert_eq!(delete, Delete::Fault(Fault { code: 13 }));
        assert_eq!(keychain_items::count(&name(label)), 0);
        assert_eq!(found(file.read()), EARLIER);
        assert_eq!(found(store.read()), EARLIER);
        assert!(matches!(file.read(), Read::NotStored));
        assert_eq!(store.delete(), Delete::Deleted);
        println!(
            "\na delete beside a file that cannot be read answered {delete:?} and kept the file"
        );
    }

    fn failing(label: &str, fault: MoveFault) -> Store {
        Store::with_move_fault(
            Directory::new(dir(label)).expect("an absolute directory"),
            Name::new(SERVICE).expect("a service"),
            name(label),
            fault,
        )
    }

    #[test]
    fn a_file_of_an_earlier_build_moves_once_into_the_keychain_and_reads_found_after() {
        on_a_hosted_runner();
        let label = "move";
        let store = store(label);
        let file = file_store(label);
        assert_eq!(store.kind(), StoreKind::Keychain);
        assert_eq!(file.write(&secret(EARLIER)), Write::Stored);
        assert_eq!(keychain_items::count(&name(label)), 0);
        assert_eq!(found(store.read()), EARLIER);
        assert!(matches!(file.read(), Read::NotStored));
        assert_eq!(keychain_items::count(&name(label)), 1);
        assert_eq!(found(store.read()), EARLIER);
        assert_eq!(keychain_items::count(&name(label)), 1);
        assert_eq!(store.delete(), Delete::Deleted);
        assert!(matches!(store.read(), Read::NotStored));
        println!("\na file of an earlier build moved once into the keychain and reads Found after");
    }

    #[test]
    fn a_failed_add_leaves_the_file_and_answers_locked() {
        on_a_hosted_runner();
        let label = "failed-add";
        let file = file_store(label);
        assert_eq!(file.write(&secret(EARLIER)), Write::Stored);
        let read = failing(label, MoveFault::Add).read();
        assert!(
            matches!(read, Read::Locked(Locked::Move { code: -61 })),
            "{read:?}"
        );
        assert_eq!(found(file.read()), EARLIER);
        assert_eq!(keychain_items::count(&name(label)), 0);
        assert_eq!(found(store(label).read()), EARLIER);
        assert!(matches!(file.read(), Read::NotStored));
        assert_eq!(store(label).delete(), Delete::Deleted);
        println!("\na failed add left the file and answered {read:?}");
    }

    #[test]
    fn a_read_back_that_differs_leaves_the_file_and_answers_locked() {
        on_a_hosted_runner();
        let label = "read-back";
        let file = file_store(label);
        assert_eq!(file.write(&secret(EARLIER)), Write::Stored);
        let read = failing(label, MoveFault::ReadBack).read();
        assert!(
            matches!(read, Read::Locked(Locked::Move { code: -1 })),
            "{read:?}"
        );
        assert_eq!(found(file.read()), EARLIER);
        assert_eq!(keychain_items::count(&name(label)), 1);
        assert_eq!(found(store(label).read()), EARLIER);
        assert!(matches!(file.read(), Read::NotStored));
        assert_eq!(keychain_items::count(&name(label)), 1);
        assert_eq!(store(label).delete(), Delete::Deleted);
        println!("\na read back that differs left the file and answered {read:?}; the next read of equal bytes finished the move");
    }

    #[test]
    fn with_both_places_full_the_read_answers_locked_never_not_stored() {
        on_a_hosted_runner();
        let label = "both";
        let store = store(label);
        let file = file_store(label);
        let signed = b"0123456789abcdef".repeat(4);
        let mut earlier = signed.clone();
        earlier[63] ^= 1;
        assert_eq!((signed.len(), earlier.len()), (64, 64));
        assert_eq!(store.write(&secret(&signed)), Write::Stored);
        assert_eq!(file.write(&secret(&earlier)), Write::Stored);
        for _ in 0..2 {
            let read = store.read();
            assert!(matches!(read, Read::Locked(Locked::OtherStore)), "{read:?}");
        }
        assert_eq!(found(file.read()), earlier);
        assert_eq!(keychain_items::count(&name(label)), 1);
        assert_eq!(store.delete(), Delete::Locked(Locked::OtherStore));
        assert_eq!(file.delete(), Delete::Deleted);
        assert_eq!(found(store.read()), signed);
        assert_eq!(store.delete(), Delete::Deleted);
        println!("\nwith both places full of two credentials of 64 bytes that differ in one byte, the read answered Locked(OtherStore) twice, and so did a delete");
    }
}

#[cfg(windows)]
mod dpapi {
    use super::*;
    use halvo_secrets::{Fault, Locked};
    use std::fs;
    use std::process::Command;
    use std::sync::Arc;

    fn store_at(place: PathBuf, label: &str) -> Store {
        Store::new(
            Directory::new(place).expect("an absolute directory"),
            Name::new(SERVICE).expect("a service"),
            name(label),
        )
    }

    fn icacls(path: &Path, args: &[&str]) -> bool {
        Command::new("icacls")
            .arg(path)
            .args(args)
            .arg("/Q")
            .status()
            .expect("icacls")
            .success()
    }

    fn cycle(store: &Store, label: &str) {
        assert_eq!(
            store.write(&secret(b"hosted secret")),
            Write::Stored,
            "{label}"
        );
        assert_eq!(found(store.read()), b"hosted secret", "{label}");
        assert_eq!(store.delete(), Delete::Deleted, "{label}");
    }

    #[test]
    fn the_store_directory_that_the_crate_makes_is_protected_and_names_three_accounts() {
        on_a_hosted_runner();
        let place = dir("descriptor");
        let store = store_at(place.clone(), "descriptor");
        assert_eq!(store.write(&secret(b"hosted secret")), Write::Stored);
        let lines = halvo_secrets::access_list_lines(&place).expect("an access list");
        for line in &lines {
            println!("\ndescriptor {line}");
        }
        assert!(lines.contains(&"protected yes".to_string()), "{lines:?}");
        let allowed: Vec<&String> = lines
            .iter()
            .filter(|line| line.starts_with("allow "))
            .collect();
        assert_eq!(allowed.len(), 3, "{lines:?}");
        let sids: Vec<&str> = allowed
            .iter()
            .map(|line| line.split(' ').nth(1).expect("a SID"))
            .collect();
        assert!(
            sids.contains(&"S-1-5-18") && sids.contains(&"S-1-5-32-544"),
            "{sids:?}"
        );
        let person: Vec<&&str> = sids
            .iter()
            .filter(|sid| sid.starts_with("S-1-5-21-"))
            .collect();
        assert_eq!(person.len(), 1, "{sids:?}");
        for line in &allowed {
            assert!(line.ends_with("mask 0x001f01ff flags 0x03"), "{line}");
        }
        let owner = lines[0].strip_prefix("owner ").expect("an owner line");
        assert!(sids.contains(&owner), "{owner} {sids:?}");
        assert_eq!(store.delete(), Delete::Deleted);
    }

    #[test]
    fn a_store_under_temp_and_under_the_work_directory_both_work() {
        on_a_hosted_runner();
        let temp =
            std::env::temp_dir().join(format!("halvo-secrets-hosted-{}-temp", std::process::id()));
        cycle(&store_at(temp, "temp"), "temp");
        cycle(&store_at(dir("work"), "work"), "work");
    }

    #[test]
    fn a_store_directory_that_grants_everyone_full_control_still_works() {
        on_a_hosted_runner();
        let place = dir("everyone");
        let store = store_at(place.clone(), "everyone");
        assert_eq!(store.write(&secret(b"hosted secret")), Write::Stored);
        assert!(icacls(&place, &["/grant", "*S-1-1-0:(OI)(CI)F"]));
        assert_eq!(found(store.read()), b"hosted secret");
        assert_eq!(store.delete(), Delete::Deleted);
        cycle(&store, "everyone");
    }

    #[test]
    fn a_store_directory_of_another_owner_is_locked() {
        on_a_hosted_runner();
        let place = dir("owner");
        fs::create_dir_all(&place).expect("a store directory");
        if !icacls(&place, &["/setowner", "*S-1-5-32-545"]) {
            println!("\nowner case not run: icacls could not set the owner");
            return;
        }
        let store = store_at(place.clone(), "owner");
        assert_eq!(
            store.write(&secret(b"hosted secret")),
            Write::Locked(Locked::Owner)
        );
        assert!(matches!(store.read(), Read::Locked(Locked::Owner)));
        assert_eq!(store.delete(), Delete::Locked(Locked::Owner));
        println!("\nowner case run: a store directory owned by BUILTIN\\Users is locked");
    }

    #[test]
    fn a_store_on_a_drive_that_does_not_exist_answers_a_fault() {
        on_a_hosted_runner();
        let letter = ('E'..='Z')
            .rev()
            .find(|letter| fs::metadata(format!("{letter}:\\")).is_err())
            .expect("a free drive letter");
        let place = PathBuf::from(format!("{letter}:\\halvo\\store"));
        let store = store_at(place, "drive");
        let read = store.read();
        assert!(matches!(read, Read::Fault(_)), "{read:?}");
        assert!(matches!(
            store.write(&secret(b"hosted secret")),
            Write::Fault(_)
        ));
        assert!(matches!(store.delete(), Delete::Fault(_)));
        println!("\na store on the drive that does not exist answers {read:?}");
    }

    #[test]
    fn a_store_under_a_parent_that_exists_reads_not_stored_and_then_stores() {
        on_a_hosted_runner();
        let place = dir("parent-exists");
        let store = store_at(place.clone(), "parent-exists");
        assert!(matches!(store.read(), Read::NotStored));
        assert_eq!(store.delete(), Delete::NotStored);
        assert!(!place.exists());
        cycle(&store, "parent-exists");
    }

    #[test]
    fn a_store_under_a_missing_parent_answers_a_fault_and_makes_nothing() {
        on_a_hosted_runner();
        let base = dir("missing-parent")
            .parent()
            .expect("a parent")
            .to_path_buf();
        let place = base.join("absent").join("store");
        let store = store_at(place, "missing-parent");
        let read = store.read();
        assert!(matches!(read, Read::Fault(Fault { code: 3 })), "{read:?}");
        assert_eq!(
            store.write(&secret(b"hosted secret")),
            Write::Fault(Fault { code: 3 })
        );
        assert_eq!(store.delete(), Delete::Fault(Fault { code: 3 }));
        assert!(!base.join("absent").exists());
        println!("\na store under a missing parent answers {read:?} and makes nothing");
    }

    #[test]
    fn a_dangling_link_at_the_store_directory_never_answers_not_stored() {
        on_a_hosted_runner();
        let place = dir("dangling");
        let target = place.with_file_name("gone");
        let made = std::os::windows::fs::symlink_dir(&target, &place).is_ok()
            || Command::new("cmd")
                .arg("/C")
                .arg("mklink")
                .arg("/J")
                .arg(&place)
                .arg(&target)
                .status()
                .is_ok_and(|status| status.success());
        assert!(made, "the runner made neither a link nor a junction");
        assert!(fs::symlink_metadata(&place).is_ok() && !target.exists());
        let store = store_at(place, "dangling");
        let read = store.read();
        assert!(matches!(read, Read::Fault(_) | Read::Locked(_)), "{read:?}");
        assert!(!matches!(
            store.write(&secret(b"hosted secret")),
            Write::Stored
        ));
        assert!(!target.exists());
        println!("\na dangling link at the store directory answers {read:?}");
    }

    #[test]
    fn the_access_lists_of_the_runners_directories_are_read() {
        on_a_hosted_runner();
        let places = [
            ("temp", std::env::temp_dir()),
            ("work", PathBuf::from(env!("CARGO_TARGET_TMPDIR"))),
        ];
        for (which, place) in places {
            let real = fs::canonicalize(&place).expect("a real path");
            let mut steps: Vec<&Path> = real.ancestors().collect();
            steps.reverse();
            for (depth, step) in steps.iter().enumerate() {
                let lines = halvo_secrets::access_list_lines(step).expect("an access list");
                assert!(!lines.is_empty());
                let shown = step
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                println!(
                    "
acl {which} {depth} {shown}"
                );
                for line in lines {
                    println!("acl {which} {depth}   {line}");
                }
            }
        }
    }

    #[test]
    fn dpapi_keeps_the_secret_sealed_and_locks_a_blob_it_cannot_open() {
        on_a_hosted_runner();
        let store = store("sealed");
        let path = dir("sealed").join(name("sealed").as_str());
        let plain = b"hosted secret in the clear";
        assert_eq!(store.write(&secret(plain)), Write::Stored);
        let on_disk = fs::read(&path).expect("the blob");
        assert!(on_disk.len() > plain.len());
        assert!(!on_disk.windows(plain.len()).any(|window| window == plain));
        assert_eq!(found(store.read()), plain);
        assert_eq!(store.delete(), Delete::Deleted);

        fs::write(&path, b"not a blob of DPAPI").expect("a file");
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(Locked::Refused { .. })),
            "{read:?}"
        );
        assert_eq!(store.write(&secret(b"new")), Write::Occupied);
        assert_eq!(fs::read(&path).expect("the file"), b"not a blob of DPAPI");
    }

    #[test]
    fn dpapi_refuses_a_directory_or_a_link_in_the_place_of_its_file() {
        on_a_hosted_runner();
        let in_place = store("directory");
        let path = dir("directory").join(name("directory").as_str());
        fs::create_dir_all(&path).expect("a directory");
        assert!(matches!(in_place.read(), Read::Locked(Locked::NotAFile)));
        assert_eq!(in_place.write(&secret(b"new")), Write::Occupied);
        assert_eq!(in_place.delete(), Delete::Locked(Locked::NotAFile));
        assert!(path.is_dir());

        let linked = store("link");
        let path = dir("link").join(name("link").as_str());
        let target = dir("link").join("target");
        fs::create_dir_all(dir("link")).expect("a directory");
        fs::write(&target, b"a file elsewhere").expect("a file");
        match std::os::windows::fs::symlink_file(&target, &path) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(1314) => {
                println!("\nlink case not run: this user may not make a link");
                return;
            }
            Err(error) => panic!("no link was made: {error}"),
        }
        assert!(matches!(linked.read(), Read::Locked(Locked::NotAFile)));
        assert_eq!(linked.write(&secret(b"new")), Write::Occupied);
        assert_eq!(linked.delete(), Delete::Locked(Locked::NotAFile));
        assert!(fs::symlink_metadata(&path)
            .expect("the link")
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&target).expect("the target"), b"a file elsewhere");
    }

    #[test]
    fn writers_that_race_for_one_dpapi_name_store_one_secret() {
        on_a_hosted_runner();
        let shared = Arc::new(store("race"));
        let threads: Vec<_> = (0..8u8)
            .map(|n| {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || (n, shared.write(&secret(&[b'a' + n; 16]))))
            })
            .collect();
        let answers: Vec<(u8, Write)> = threads
            .into_iter()
            .map(|thread| thread.join().expect("a writer"))
            .collect();
        let stored: Vec<u8> = answers
            .iter()
            .filter(|(_, answer)| *answer == Write::Stored)
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(stored.len(), 1, "{answers:?}");
        assert_eq!(found(shared.read()), [b'a' + stored[0]; 16]);
        assert_eq!(shared.delete(), Delete::Deleted);
    }
}
