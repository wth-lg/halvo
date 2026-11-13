#![cfg(all(target_os = "macos", debug_assertions))]
#![deny(clippy::undocumented_unsafe_blocks)]

use halvo_secrets::{Delete, Directory, Fault, Locked, Name, Read, Secret, Store, Write};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::os::raw::{c_char, c_int, c_uint};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

extern "C" {
    fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
}

const RENAME_SWAP: c_uint = 0x0002;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn scratch(label: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "halvo-secrets-ancestors-{}-{label}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("a scratch directory");
    path
}

fn store_named(dir: &Path, name: &str) -> Store {
    Store::in_directory(
        Directory::new(dir.to_path_buf()).expect("an absolute directory"),
        Name::new(name).expect("a name"),
    )
}

fn store_in(dir: &Path) -> Store {
    store_named(dir, "credential")
}

fn secret(bytes: &[u8]) -> Secret {
    Secret::new(bytes.to_vec()).expect("a secret")
}

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("a mode");
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

fn found(read: Read) -> Vec<u8> {
    match read {
        Read::Found(secret) => secret.expose().to_vec(),
        other => panic!("read answered {other:?}"),
    }
}

fn cycle(dir: &Path) {
    let store = store_in(dir);
    assert_eq!(store.write(&secret(b"the credential")), Write::Stored);
    assert_eq!(found(store.read()), b"the credential");
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(matches!(store.read(), Read::NotStored));
}

#[test]
fn a_parent_that_others_may_write_without_the_sticky_bit_is_refused() {
    for mode in [0o777, 0o775, 0o757, 0o2775] {
        let parent = scratch("open-parent");
        let dir = parent.join("store");
        assert_eq!(
            store_in(&dir).write(&secret(b"the credential")),
            Write::Stored
        );
        set_mode(&parent, mode);
        let store = store_in(&dir);
        let read = store.read();
        let delete = store.delete();
        let write = store.write(&secret(b"a second credential"));
        set_mode(&parent, 0o755);
        assert!(
            matches!(read, Read::Locked(Locked::Ancestor)),
            "{mode:o}: {read:?}"
        );
        assert_eq!(delete, Delete::Locked(Locked::Ancestor), "{mode:o}");
        assert_eq!(write, Write::Locked(Locked::Ancestor), "{mode:o}");
        assert_eq!(
            fs::read(dir.join("credential")).expect("the item"),
            b"the credential"
        );
    }
}

#[test]
fn a_parent_with_an_access_list_that_lets_others_change_its_entries_is_refused() {
    for entry in [
        "everyone allow add_file",
        "everyone allow add_subdirectory",
        "everyone allow delete_child",
        "everyone allow delete",
        "everyone allow writesecurity",
        "everyone allow chown",
    ] {
        let parent = scratch("acl-parent");
        let dir = parent.join("store");
        add_acl(&parent, entry);
        let store = store_in(&dir);
        assert_eq!(
            store.write(&secret(b"the credential")),
            Write::Locked(Locked::Ancestor),
            "{entry}"
        );
        assert!(!dir.exists(), "{entry}");
        assert!(
            matches!(store.read(), Read::Locked(Locked::Ancestor)),
            "{entry}"
        );
    }
}

#[test]
fn a_parent_whose_access_list_only_denies_or_only_lets_others_read_still_works() {
    for entry in ["everyone deny delete", "everyone allow read,list,search"] {
        let parent = scratch("acl-read-parent");
        add_acl(&parent, entry);
        cycle(&parent.join("store"));
    }
}

#[test]
fn a_store_under_a_sticky_parent_that_everyone_may_write_still_works() {
    let parent = scratch("sticky");
    set_mode(&parent, 0o1777);
    cycle(&parent.join("store"));
    set_mode(&parent, 0o755);
}

#[test]
fn a_store_in_the_test_directory_of_the_build_still_works() {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    println!(
        "the test directory of the build is under the home directory: {}",
        home.is_some_and(|home| base.starts_with(home))
    );
    cycle(&scratch("home").join("store"));
}

#[test]
fn a_dangling_link_among_the_ancestors_never_answers_not_stored() {
    let base = scratch("dangling");
    let real = base.join("real");
    fs::create_dir_all(&real).expect("the parent");
    let dir = real.join("store");
    assert_eq!(
        store_in(&dir).write(&secret(b"the credential")),
        Write::Stored
    );
    let linked = base.join("linked");
    symlink(&real, &linked).expect("a link");
    let through = linked.join("store");
    assert_eq!(found(store_in(&through).read()), b"the credential");
    let mut aside = real.file_name().expect("a name").to_os_string();
    aside.push("-aside");
    fs::rename(&real, real.with_file_name(aside)).expect("the target moved");
    let read = store_in(&through).read();
    assert!(
        matches!(read, Read::Fault(Fault { code: 2 })),
        "a dangling link answers the fault of the missing step: {read:?}"
    );
    assert_eq!(
        store_in(&through).write(&secret(b"second")),
        Write::Fault(Fault { code: 2 })
    );
}

#[test]
fn a_store_directory_renamed_away_under_a_parent_that_others_may_write_reads_locked() {
    let parent = scratch("rename").join("parent");
    fs::create_dir_all(&parent).expect("the parent");
    let dir = parent.join("store");
    let store = store_in(&dir);
    assert_eq!(store.write(&secret(b"credential-one")), Write::Stored);
    set_mode(&parent, 0o777);
    fs::rename(&dir, parent.join("moved")).expect("the parent's writer renames the store");
    let read = store.read();
    let second = store.write(&secret(b"credential-two"));
    set_mode(&parent, 0o755);
    assert!(matches!(read, Read::Locked(Locked::Ancestor)), "{read:?}");
    assert_eq!(second, Write::Locked(Locked::Ancestor));
    assert!(!dir.join("credential").exists());
    assert_eq!(
        fs::read(parent.join("moved").join("credential")).expect("the credential"),
        b"credential-one"
    );
}

fn holds_a_secret(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).expect("a listing") {
        let entry = entry.expect("an entry");
        if let Ok(bytes) = fs::read(entry.path()) {
            if bytes.starts_with(b"secret-") {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    names
}

fn inode(path: &Path) -> u64 {
    fs::symlink_metadata(path).expect("metadata").ino()
}

#[test]
fn writes_against_a_thread_that_swaps_the_store_directory_never_reach_the_other_directory() {
    let parent = scratch("swap").join("parent");
    let store_dir = parent.join("store");
    let other = parent.join("other");
    fs::create_dir_all(&store_dir).expect("the store directory");
    fs::create_dir_all(&other).expect("the other directory");
    set_mode(&store_dir, 0o700);
    set_mode(&other, 0o700);
    add_acl(&other, "everyone allow read,file_inherit");
    let other_inode = inode(&other);
    assert_eq!(
        store_named(&other, "probe").write(&secret(b"probe")),
        Write::Locked(Locked::Acl)
    );

    let start = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let swaps = Arc::new(AtomicU64::new(0));
    let swapper = {
        let stop = Arc::clone(&stop);
        let swaps = Arc::clone(&swaps);
        let a = CString::new(store_dir.as_os_str().as_bytes()).expect("a path");
        let b = CString::new(other.as_os_str().as_bytes()).expect("a path");
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(60) {
                // SAFETY: both names are NUL-terminated strings that the thread owns for its
                // whole life.
                if unsafe { renamex_np(a.as_ptr(), b.as_ptr(), RENAME_SWAP) } == 0 {
                    swaps.fetch_add(1, Ordering::Relaxed);
                }
                thread::sleep(Duration::from_micros(20));
            }
        })
    };
    let mut answers: BTreeMap<String, u64> = BTreeMap::new();
    for n in 0..2000u32 {
        let answer = store_named(&store_dir, &format!("item{n}"))
            .write(&secret(format!("secret-{n}").as_bytes()));
        *answers.entry(format!("{answer:?}")).or_insert(0) += 1;
    }
    stop.store(true, Ordering::Relaxed);
    swapper.join().expect("the swapper");

    let other_now = if inode(&other) == other_inode {
        other.clone()
    } else {
        store_dir.clone()
    };
    assert_eq!(inode(&other_now), other_inode);
    let leaked = holds_a_secret(&other_now);
    println!(
        "2000 writes, {} swaps, answers {answers:?}, files of a secret in the directory with the access list: {}",
        swaps.load(Ordering::Relaxed),
        leaked.len()
    );
    assert!(
        swaps.load(Ordering::Relaxed) > 0,
        "the swapper never swapped"
    );
    assert!(
        answers.get("Stored").copied().unwrap_or(0) > 0,
        "no write was stored"
    );
    assert_eq!(leaked, Vec::<String>::new());
}
