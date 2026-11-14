#![cfg(all(target_os = "macos", debug_assertions))]
#![deny(clippy::undocumented_unsafe_blocks)]

use halvo_secrets::{
    Delete, Directory, Fault, Locked, Name, Read, Secret, Store, StoreKind, Write,
};
use std::ffi::CString;
use std::fs;
use std::os::raw::{c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

fn add_acl(path: &Path, entry: &str) {
    let status = Command::new("/bin/chmod")
        .arg("+a")
        .arg(entry)
        .arg(path)
        .status()
        .expect("chmod");
    assert!(status.success(), "chmod +a {entry}");
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
fn a_store_makes_its_directory_and_no_missing_step_above_it() {
    let base = scratch("parents");
    let dir = base.join("a").join("b").join("store");
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::Fault(Fault { code: 2 })));
    assert_eq!(
        store.write(&secret(b"secret")),
        Write::Fault(Fault { code: 2 })
    );
    assert!(!base.join("a").exists());
    fs::create_dir_all(base.join("a").join("b")).expect("the parent");
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.write(&secret(b"secret")), Write::Stored);
    assert_eq!(found(store.read()), b"secret");
    assert_eq!(mode(&dir) & 0o7777, 0o700);
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

fn make_fifo(path: &Path) {
    let path = CString::new(path.as_os_str().as_bytes()).expect("a path");
    // SAFETY: `path` is a NUL-terminated string that lives to the end of this function.
    let made = unsafe { mkfifo(path.as_ptr(), 0o600) };
    assert_eq!(made, 0);
}

fn within_five_seconds(store: Store) -> (String, Delete) {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let read = format!("{:?}", store.read());
        let _ = sender.send((read, store.delete()));
    });
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("a read and a delete that end within five seconds")
}

#[test]
fn a_fifo_in_the_place_of_the_file_is_refused_without_waiting() {
    let dir = scratch("fifo");
    private_dir(&dir);
    make_fifo(&dir.join("account"));
    let (read, delete) = within_five_seconds(store_in(&dir));
    assert_eq!(read, "Locked(NotAFile)");
    assert_eq!(delete, Delete::Locked(Locked::NotAFile));
}

#[test]
fn a_fifo_in_the_place_of_the_directory_is_refused_without_waiting() {
    let base = scratch("fifo-dir");
    let place = base.join("store");
    make_fifo(&place);
    let (read, delete) = within_five_seconds(store_in(&place));
    assert_eq!(read, "Locked(NotADirectory)");
    assert_eq!(delete, Delete::Locked(Locked::NotADirectory));
}

#[test]
fn a_write_leaves_a_temporary_file_of_another_writer_as_it_was() {
    let dir = scratch("leftover");
    private_dir(&dir);
    let leftover = dir.join(format!(".account.{}.0.tmp", std::process::id()));
    private_file(&leftover, b"another writer");
    assert_eq!(
        store_in(&dir).write(&secret(b"the credential")),
        Write::Stored
    );
    assert_eq!(
        fs::read(&leftover).expect("the leftover"),
        b"another writer"
    );
    assert_eq!(found(store_in(&dir).read()), b"the credential");
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
fn a_file_or_a_directory_with_an_acl_is_refused_and_kept() {
    let dir = scratch("acl-file");
    private_dir(&dir);
    let path = dir.join("account");
    private_file(&path, b"kept");
    add_acl(&path, "everyone allow read");
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::Locked(Locked::Acl)));
    assert_eq!(store.delete(), Delete::Locked(Locked::Acl));
    assert_eq!(store.write(&secret(b"new")), Write::Occupied);
    assert_eq!(fs::read(&path).expect("the file"), b"kept");

    let dir = scratch("acl-dir");
    private_dir(&dir);
    private_file(&dir.join("account"), b"kept");
    add_acl(&dir, "everyone allow list,search,add_file,delete_child");
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::Locked(Locked::Acl)));
    assert_eq!(store.write(&secret(b"new")), Write::Locked(Locked::Acl));
    assert_eq!(store.delete(), Delete::Locked(Locked::Acl));
    assert_eq!(fs::read(dir.join("account")).expect("the file"), b"kept");
}

#[test]
fn a_write_under_an_inheritable_acl_stores_nothing() {
    let base = scratch("acl-inherit");
    set_mode(&base, 0o700);
    add_acl(
        &base,
        "everyone allow list,search,read,file_inherit,directory_inherit",
    );
    let dir = base.join("store");
    let store = store_in(&dir);
    assert_eq!(
        store.write(&secret(b"the credential")),
        Write::Locked(Locked::Acl)
    );
    assert!(!dir.join("account").exists());
    assert!(matches!(store.read(), Read::Locked(Locked::Acl)));
}

#[test]
fn a_hard_link_in_the_place_of_the_file_is_refused_and_kept() {
    let dir = scratch("hard-link");
    private_dir(&dir);
    let elsewhere = scratch("hard-link-elsewhere");
    private_dir(&elsewhere);
    let other = elsewhere.join("other");
    private_file(&other, b"another file");
    fs::hard_link(&other, dir.join("account")).expect("a hard link");
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::Locked(Locked::HardLink)));
    assert_eq!(store.delete(), Delete::Locked(Locked::HardLink));
    assert_eq!(store.write(&secret(b"new")), Write::Occupied);
    assert_eq!(
        fs::read(dir.join("account")).expect("the file"),
        b"another file"
    );
    assert_eq!(fs::read(&other).expect("the other name"), b"another file");
}

#[test]
fn a_trailing_separator_or_dot_does_not_let_a_linked_directory_through() {
    let base = scratch("trailing");
    let real = base.join("real");
    private_dir(&real);
    private_file(&real.join("account"), b"kept");
    let linked = base.join("linked");
    symlink(&real, &linked).expect("a link");
    for suffix in ["/", "/.", "/./"] {
        let given = PathBuf::from(format!("{}{suffix}", linked.display()));
        let store = store_in(&given);
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(Locked::NotADirectory)),
            "{suffix}: {read:?}"
        );
        assert_eq!(
            store.write(&secret(b"new")),
            Write::Locked(Locked::NotADirectory)
        );
        assert_eq!(store.delete(), Delete::Locked(Locked::NotADirectory));
    }
    assert_eq!(fs::read(real.join("account")).expect("the file"), b"kept");
}

#[test]
fn a_relative_directory_or_one_with_a_parent_step_is_refused() {
    assert!(Directory::new(PathBuf::from("relative/store")).is_none());
    assert!(Directory::new(PathBuf::from("")).is_none());
    assert!(Directory::new(PathBuf::from("/a/../b")).is_none());
    assert!(Directory::new(PathBuf::from("/a/b/..")).is_none());
    for (given, kept) in [
        ("/a/b/", "/a/b"),
        ("/a/./b/.", "/a/b"),
        ("/a//b", "/a/b"),
        ("/a/b/./", "/a/b"),
    ] {
        let dir = Directory::new(PathBuf::from(given)).expect("an absolute directory");
        assert_eq!(
            dir.as_path().as_os_str(),
            std::ffi::OsStr::new(kept),
            "{given}: the text of the path, not its components, which compare equal either way"
        );
    }
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
