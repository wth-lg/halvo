use halvo_account::environment::{backend, store, Environment, ITEM, VARIABLES};
use halvo_account::outcome::EnvironmentFault;
use halvo_secrets::{Read, StoreKind};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

fn scratch(label: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "halvo-account-environment-{}-{label}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("a scratch directory");
    path
}

fn environment(backend: Option<&str>, store_dir: Option<OsString>) -> Environment {
    Environment {
        backend: backend.map(OsString::from),
        store_dir,
    }
}

fn sources() -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut pending = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).expect("a source directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let name = path
                    .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                    .expect("under the crate")
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((name, fs::read_to_string(&path).expect("a source")));
            }
        }
    }
    found.sort();
    found
}

#[cfg(debug_assertions)]
#[test]
fn a_debug_build_reads_the_two_variables_of_the_committed_list() {
    assert_eq!(
        VARIABLES,
        ["HALVO_ACCOUNT_BACKEND", "HALVO_ACCOUNT_STORE_DIR"]
    );
}

#[test]
fn with_the_store_directory_unset_a_debug_build_has_no_store() {
    let none = environment(Some("http://127.0.0.1:8787"), None);
    assert_eq!(store(&none).err(), Some(EnvironmentFault::NoStoreDirectory));
}

#[test]
fn a_store_directory_that_is_relative_or_climbs_is_refused() {
    for dir in ["relative/dir", ".", "", "/tmp/../tmp/x"] {
        let given = environment(None, Some(OsString::from(dir)));
        assert_eq!(
            store(&given).err(),
            Some(EnvironmentFault::BadStoreDirectory),
            "{dir:?}"
        );
    }
}

#[test]
fn the_store_of_a_debug_build_is_the_file_in_the_given_directory_whatever_the_features() {
    let dir = scratch("store").join("account-store");
    let given = environment(None, Some(dir.clone().into_os_string()));
    let kept = store(&given).unwrap_or_else(|fault| panic!("no store: {fault:?}"));
    let expected = if cfg!(windows) {
        StoreKind::Dpapi
    } else {
        StoreKind::File
    };
    assert_eq!(kept.kind(), expected);
    assert!(matches!(kept.read(), Read::NotStored));
    assert!(!dir.exists());
    assert_eq!(ITEM, "account");
}

#[test]
fn the_backend_of_a_debug_build_is_its_variable_read_as_an_address() {
    assert_eq!(
        backend(&environment(None, None)).err(),
        Some(EnvironmentFault::NoBackend)
    );
    for text in ["http://localhost:8787", "http://127.0.0.1:8787/", ""] {
        assert_eq!(
            backend(&environment(Some(text), None)).err(),
            Some(EnvironmentFault::BadBackend),
            "{text:?}"
        );
    }
    let read = backend(&environment(Some("http://127.0.0.1:8787"), None)).expect("a backend");
    assert_eq!(read.url_of("/v1/cap"), "http://127.0.0.1:8787/v1/cap");
}

#[cfg(unix)]
#[test]
fn a_variable_that_is_not_unicode_is_refused() {
    use std::os::unix::ffi::OsStringExt;
    let bad = OsString::from_vec(b"http://127.0.0.1:87\xff".to_vec());
    let given = Environment {
        backend: Some(bad),
        store_dir: None,
    };
    assert_eq!(backend(&given).err(), Some(EnvironmentFault::BadBackend));
}

#[test]
fn the_sources_read_the_environment_only_through_the_committed_list() {
    let readers = [
        "var_os(",
        "env::var(",
        "vars(",
        "vars_os(",
        "env!(",
        "option_env!(",
        "set_var(",
        "remove_var(",
        "stdin()",
        "Stdin",
        "io::stdin",
    ];
    let mut uses = Vec::new();
    for (name, text) in sources() {
        if name.ends_with("/tests.rs") {
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            for reader in readers {
                for _ in line.matches(reader) {
                    uses.push(format!("{name}:{} {reader}", number + 1));
                }
            }
        }
    }
    let allowed: Vec<&str> = uses
        .iter()
        .map(|found| found.split_once(' ').map_or("", |(_, reader)| reader))
        .collect();
    assert_eq!(
        allowed,
        ["var_os(", "var_os(", "env!(", "env!("],
        "the readers are {uses:?}"
    );
    let environment_source = sources()
        .into_iter()
        .find(|(name, _)| name == "src/environment.rs")
        .expect("src/environment.rs")
        .1;
    assert!(environment_source.contains("var_os(VARIABLES[0])"));
    assert!(environment_source.contains("var_os(VARIABLES[1])"));
}

#[test]
fn no_source_calls_store_new() {
    for (name, text) in sources() {
        assert!(!text.contains("Store::new"), "{name} calls Store::new");
        assert!(!text.contains("Store :: new"), "{name} calls Store::new");
    }
}
