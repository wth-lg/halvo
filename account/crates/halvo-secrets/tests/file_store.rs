#![cfg(all(target_os = "macos", debug_assertions))]

use halvo_secrets::{Delete, Directory, Locked, Name, Read, Secret, Store, StoreKind, Write};
use std::ffi::CString;
use std::fs;
use std::os::raw::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

extern "C" {
    fn mkfifo(path: *const c_char, mode: u16) -> c_int;
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn scratch(label: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("halvo-secrets-{}-{label}-{n}", std::process::id()));
    fs::create_dir_all(&path).expect("a scratch directory");
    path
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

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o7777
}

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("a mode");
}

fn private_dir(path: &Path) {
    fs::create_dir_all(path).expect("a directory");
    set_mode(path, 0o700);
}

fn private_file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("a file");
    set_mode(path, 0o600);
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("a listing")
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

#[test]
fn the_file_store_writes_reads_and_deletes_with_modes_0600_and_0700() {
    let dir = scratch("cycle").join("store");
    let store = store_in(&dir);
    assert_eq!(store.kind(), StoreKind::File);
    assert_eq!(store.kind().as_str(), "file");
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    assert_eq!(store.write(&secret(b"0123456789abcdef")), Write::Stored);
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join("account")), 0o600);
    assert_eq!(
        fs::read(dir.join("account")).expect("the file"),
        b"0123456789abcdef"
    );
    assert_eq!(names_in(&dir), ["account"]);
    assert_eq!(found(store.read()), b"0123456789abcdef");
    assert_eq!(store.write(&secret(b"another")), Write::Occupied);
    assert_eq!(found(store.read()), b"0123456789abcdef");
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(!dir.join("account").exists());
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    assert!(names_in(&dir).is_empty());
}

#[test]
fn a_store_makes_the_missing_parents_of_its_directory() {
    let base = scratch("parents");
    let dir = base.join("a").join("b").join("store");
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::NotStored));
    assert!(!base.join("a").exists());
    assert_eq!(store.write(&secret(b"secret")), Write::Stored);
    assert_eq!(found(store.read()), b"secret");
    assert_eq!(mode(&base.join("a")) & 0o077, 0);
}

#[test]
fn a_file_of_a_wider_mode_is_refused_and_kept() {
    for wider in [0o644, 0o640, 0o604, 0o700, 0o4600] {
        let dir = scratch("wide-file");
        private_dir(&dir);
        let path = dir.join("account");
        private_file(&path, b"kept");
        set_mode(&path, wider);
        let store = store_in(&dir);
        assert!(
            matches!(store.read(), Read::Locked(Locked::Mode)),
            "{wider:o}"
        );
        assert_eq!(store.delete(), Delete::Locked(Locked::Mode));
        assert_eq!(store.write(&secret(b"new")), Write::Occupied);
        assert_eq!(fs::read(&path).expect("the file"), b"kept");
        assert_eq!(mode(&path), wider);
    }
}

#[test]
fn a_directory_of_a_wider_mode_is_refused() {
    for wider in [0o755, 0o750, 0o701] {
        let dir = scratch("wide-dir");
        private_dir(&dir);
        private_file(&dir.join("account"), b"kept");
        set_mode(&dir, wider);
        let store = store_in(&dir);
        assert!(
            matches!(store.read(), Read::Locked(Locked::Mode)),
            "{wider:o}"
        );
        assert_eq!(store.write(&secret(b"new")), Write::Locked(Locked::Mode));
        assert_eq!(store.delete(), Delete::Locked(Locked::Mode));
        assert_eq!(fs::read(dir.join("account")).expect("the file"), b"kept");
        assert_eq!(names_in(&dir), ["account"]);
    }
}

#[test]
fn a_link_in_the_place_of_the_file_is_refused_and_kept() {
    let base = scratch("link");
    let elsewhere = base.join("elsewhere");
    private_dir(&elsewhere);
    private_file(&elsewhere.join("target"), b"a file elsewhere");
    let dir = base.join("store");
    private_dir(&dir);
    symlink(elsewhere.join("target"), dir.join("account")).expect("a link");
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::Locked(Locked::NotAFile)));
    assert_eq!(store.delete(), Delete::Locked(Locked::NotAFile));
    assert_eq!(store.write(&secret(b"new")), Write::Occupied);
    assert!(fs::symlink_metadata(dir.join("account"))
        .expect("the link")
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read(elsewhere.join("target")).expect("the target"),
        b"a file elsewhere"
    );
}

#[test]
fn a_dangling_link_is_not_read_as_no_item() {
    let dir = scratch("dangling");
    private_dir(&dir);
    symlink(dir.join("nowhere"), dir.join("account")).expect("a link");
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::Locked(Locked::NotAFile)));
    assert_eq!(store.write(&secret(b"new")), Write::Occupied);
    assert_eq!(store.delete(), Delete::Locked(Locked::NotAFile));
    assert!(!dir.join("nowhere").exists());
}

#[test]
fn a_directory_in_the_place_of_the_file_is_refused() {
    let dir = scratch("dir-in-place");
    private_dir(&dir);
    private_dir(&dir.join("account"));
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::Locked(Locked::NotAFile)));
    assert_eq!(store.delete(), Delete::Locked(Locked::NotAFile));
    assert_eq!(store.write(&secret(b"new")), Write::Occupied);
    assert!(dir.join("account").is_dir());
}

#[test]
fn a_fifo_in_the_place_of_the_file_is_refused_without_waiting() {
    let dir = scratch("fifo");
    private_dir(&dir);
    let path = CString::new(dir.join("account").as_os_str().as_bytes()).expect("a path");
    assert_eq!(unsafe { mkfifo(path.as_ptr(), 0o600) }, 0);
    let store = store_in(&dir);
    let start = Instant::now();
    assert!(matches!(store.read(), Read::Locked(Locked::NotAFile)));
    assert_eq!(store.delete(), Delete::Locked(Locked::NotAFile));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_link_or_a_file_in_the_place_of_the_directory_is_refused() {
    let base = scratch("dir-link");
    let real = base.join("real");
    private_dir(&real);
    private_file(&real.join("account"), b"kept");
    let linked = base.join("linked");
    symlink(&real, &linked).expect("a link");
    let store = store_in(&linked);
    assert!(matches!(store.read(), Read::Locked(Locked::NotADirectory)));
    assert_eq!(
        store.write(&secret(b"new")),
        Write::Locked(Locked::NotADirectory)
    );
    assert_eq!(store.delete(), Delete::Locked(Locked::NotADirectory));
    assert_eq!(fs::read(real.join("account")).expect("the file"), b"kept");

    let file = base.join("file");
    private_file(&file, b"a file");
    let store = store_in(&file);
    assert!(matches!(store.read(), Read::Locked(Locked::NotADirectory)));
    assert_eq!(
        store.write(&secret(b"new")),
        Write::Locked(Locked::NotADirectory)
    );
    assert_eq!(fs::read(&file).expect("the file"), b"a file");
}

#[test]
fn a_relative_directory_is_refused() {
    assert!(Directory::new(PathBuf::from("relative/store")).is_none());
    assert!(Directory::new(PathBuf::from("")).is_none());
    let absolute = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    assert_eq!(
        Directory::new(absolute.clone()).map(|dir| dir.as_path().to_path_buf()),
        Some(absolute)
    );
}

#[test]
fn writers_that_race_for_one_name_store_one_secret() {
    let dir = scratch("race").join("store");
    let dir = Arc::new(dir);
    let threads: Vec<_> = (0..8u8)
        .map(|n| {
            let dir = Arc::clone(&dir);
            std::thread::spawn(move || (n, store_in(&dir).write(&secret(&[b'a' + n; 16]))))
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
    assert!(answers
        .iter()
        .all(|(_, answer)| matches!(answer, Write::Stored | Write::Occupied)));
    assert_eq!(found(store_in(&dir).read()), [b'a' + stored[0]; 16]);
    assert_eq!(names_in(&dir), ["account"]);
}

#[cfg(not(feature = "keychain"))]
#[test]
fn the_store_of_a_build_without_the_keychain_is_the_file_store() {
    let dir = scratch("build").join("store");
    let store = Store::new(
        Directory::new(dir.clone()).expect("an absolute directory"),
        Name::new("dev.halvo.test").expect("a service"),
        Name::new("credential").expect("a name"),
    );
    assert_eq!(store.kind(), StoreKind::File);
    assert_eq!(store.write(&secret(b"secret")), Write::Stored);
    assert_eq!(
        fs::read(dir.join("credential")).expect("the file"),
        b"secret"
    );
    assert_eq!(found(store.read()), b"secret");
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(names_in(&dir).is_empty());
}
