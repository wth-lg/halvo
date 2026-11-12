#![cfg(all(target_os = "macos", debug_assertions))]

use halvo_secrets::{Delete, Directory, Name, Read, Secret, Store, Write};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const EPERM_CHILD: &str = "HALVO_SECRETS_EPERM_CHILD";

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn scratch(label: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "halvo-secrets-failed-{}-{label}-{n}",
        std::process::id()
    ));
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

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("a mode");
}

fn stored(label: &str) -> PathBuf {
    let dir = scratch(label).join("store");
    assert_eq!(
        store_in(&dir).write(&secret(b"the credential")),
        Write::Stored
    );
    dir
}

fn never_absent(read: &Read, delete: Delete, case: &str) {
    assert!(
        matches!(read, Read::Locked(_) | Read::Fault(_)),
        "{case}: the read answered {read:?}"
    );
    assert!(
        matches!(delete, Delete::Locked(_) | Delete::Fault(_)),
        "{case}: the delete answered {delete:?}"
    );
}

#[test]
fn a_read_of_an_item_that_the_user_may_not_open_answers_locked() {
    for mode in [0o000, 0o200] {
        let dir = stored("item-mode");
        let item = dir.join("account");
        set_mode(&item, mode);
        let store = store_in(&dir);
        let read = store.read();
        let delete = store.delete();
        let write = store.write(&secret(b"a second credential"));
        set_mode(&item, 0o600);
        let case = format!("item of mode {mode:o}");
        assert!(
            matches!(read, Read::Locked(_)),
            "{case}: the read answered {read:?}"
        );
        never_absent(&read, delete, &case);
        assert!(
            write != Write::Stored,
            "{case}: a second write answered Stored"
        );
        assert_eq!(fs::read(&item).expect("the item"), b"the credential");
    }
}

#[test]
fn a_read_in_a_directory_that_the_user_may_not_search_never_answers_not_stored() {
    for mode in [0o000, 0o400, 0o600] {
        let dir = stored("dir-mode");
        set_mode(&dir, mode);
        let store = store_in(&dir);
        let read = store.read();
        let delete = store.delete();
        let write = store.write(&secret(b"a second credential"));
        set_mode(&dir, 0o700);
        let case = format!("store directory of mode {mode:o}");
        never_absent(&read, delete, &case);
        assert!(
            write != Write::Stored,
            "{case}: a second write answered Stored"
        );
        assert_eq!(
            fs::read(dir.join("account")).expect("the item"),
            b"the credential"
        );
    }
}

#[test]
fn a_read_under_a_parent_that_the_user_may_not_search_never_answers_not_stored() {
    let dir = stored("parent-mode");
    let parent = dir.parent().expect("a parent").to_path_buf();
    set_mode(&parent, 0o000);
    let store = store_in(&dir);
    let read = store.read();
    let delete = store.delete();
    let write = store.write(&secret(b"a second credential"));
    set_mode(&parent, 0o755);
    never_absent(&read, delete, "parent of mode 0");
    assert!(
        write != Write::Stored,
        "parent of mode 0: a second write answered Stored"
    );
    assert_eq!(
        fs::read(dir.join("account")).expect("the item"),
        b"the credential"
    );
}

#[test]
fn a_read_past_an_ancestor_that_is_a_file_never_answers_not_stored() {
    let dir = stored("ancestor-file");
    let parent = dir.parent().expect("a parent").to_path_buf();
    let aside = parent.with_file_name("parent-aside");
    fs::rename(&parent, &aside).expect("the parent moved aside");
    fs::write(&parent, b"a file in the place of an ancestor").expect("a file");
    let store = store_in(&dir);
    let read = store.read();
    let delete = store.delete();
    let write = store.write(&secret(b"a second credential"));
    never_absent(&read, delete, "an ancestor that is a file");
    assert!(write != Write::Stored, "a second write answered Stored");
    assert_eq!(
        fs::read(aside.join("store").join("account")).expect("the item"),
        b"the credential"
    );
}

#[test]
fn child_reads_the_store_that_its_parent_names() {
    let Some(dir) = std::env::var_os(EPERM_CHILD) else {
        return;
    };
    let store = store_in(Path::new(&dir));
    println!("child read {:?}", store.read());
    println!("child delete {:?}", store.delete());
}

#[test]
fn a_read_that_the_system_does_not_permit_never_answers_not_stored() {
    let dir = stored("eperm");
    let item = fs::canonicalize(dir.join("account")).expect("the item");
    let profile = format!(
        "(version 1)(allow default)(deny file-read* (literal \"{}\"))",
        item.display()
    );
    let output = Command::new("/usr/bin/sandbox-exec")
        .arg("-p")
        .arg(&profile)
        .arg(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "child_reads_the_store_that_its_parent_names",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(EPERM_CHILD, &dir)
        .output()
        .expect("a sandboxed child");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "the child failed: {text}");
    assert!(
        text.contains("child read Locked(Refused { code: 1 })"),
        "{text}"
    );
    assert!(
        text.contains("child delete Locked(Refused { code: 1 })"),
        "{text}"
    );
    assert_eq!(fs::read(&item).expect("the item"), b"the credential");
}
