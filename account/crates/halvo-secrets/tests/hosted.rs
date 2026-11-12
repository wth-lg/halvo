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
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!(
            "halvo-secrets-hosted-{}-{label}",
            std::process::id()
        ))
        .join("store")
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

#[cfg(windows)]
mod dpapi {
    use super::*;
    use halvo_secrets::Locked;
    use std::fs;
    use std::sync::Arc;

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
