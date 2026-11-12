#![cfg(all(target_os = "macos", debug_assertions))]

use halvo_secrets::{Delete, Directory, Fault, Locked, Name, Read, Secret, Store, Write};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const ENOENT: i64 = 2;
const EACCES: i64 = 13;
const ELOOP: i64 = 62;
const LINK_BOUND: usize = 32;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn scratch(label: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "halvo-secrets-walk-{}-{label}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("a scratch directory");
    path
}

fn store_in(dir: &Path) -> Store {
    Store::in_directory(
        Directory::new(dir.to_path_buf()).expect("an absolute directory"),
        Name::new("credential").expect("a name"),
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

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("a mode");
}

fn chmod(args: &[&str], path: &Path) {
    let status = Command::new("/bin/chmod")
        .args(args)
        .arg(path)
        .status()
        .expect("chmod");
    assert!(status.success(), "chmod {args:?}");
}

fn person() -> String {
    let out = Command::new("/usr/bin/id")
        .arg("-un")
        .output()
        .expect("id -un");
    String::from_utf8(out.stdout)
        .expect("a name")
        .trim()
        .to_string()
}

/// Gives each directory that a case narrowed mode 0755 and no access list again when the
/// case ends, also when it fails, so that the test directory can be removed.
struct Restore(Vec<PathBuf>);

impl Drop for Restore {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
            let _ = Command::new("/bin/chmod").arg("-N").arg(path).status();
        }
    }
}

fn cycle(dir: &Path) {
    let store = store_in(dir);
    assert_eq!(store.write(&secret(b"the credential")), Write::Stored);
    assert_eq!(found(store.read()), b"the credential");
    assert_eq!(store.delete(), Delete::Deleted);
    assert!(matches!(store.read(), Read::NotStored));
}

fn fault(code: i64) -> Fault {
    Fault { code }
}

fn read_fault(read: Read) -> i64 {
    match read {
        Read::Fault(fault) => fault.code,
        other => panic!("read answered {other:?}"),
    }
}

fn within_five_seconds<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let (sent, received) = mpsc::channel();
    thread::spawn(move || {
        let _ = sent.send(work());
    });
    received
        .recv_timeout(Duration::from_secs(5))
        .expect("the call answered within 5 seconds")
}

#[test]
fn a_link_in_a_directory_that_others_may_write_on_the_callers_path_is_refused_at_the_first_write() {
    let base = scratch("open-link");
    let open = base.join("open");
    let real = base.join("real");
    fs::create_dir_all(&open).expect("open");
    fs::create_dir_all(&real).expect("real");
    symlink(&real, open.join("l")).expect("a link");
    set_mode(&open, 0o777);
    let _restore = Restore(vec![open.clone()]);
    let store = store_in(&open.join("l").join("store"));
    assert_eq!(
        store.write(&secret(b"credential-one")),
        Write::Locked(Locked::Ancestor)
    );
    assert!(!real.join("store").exists());
    assert!(matches!(store.read(), Read::Locked(Locked::Ancestor)));
    assert_eq!(store.delete(), Delete::Locked(Locked::Ancestor));
}

#[test]
fn a_link_that_a_party_who_may_write_its_directory_points_elsewhere_never_reads_not_stored() {
    let base = scratch("repointed");
    let open = base.join("open");
    let real = base.join("real");
    let other = base.join("other");
    for dir in [&open, &real, &other] {
        fs::create_dir_all(dir).expect("a directory");
    }
    symlink(&real, open.join("l")).expect("a link");
    let store = store_in(&open.join("l").join("store"));
    assert_eq!(store.write(&secret(b"credential-one")), Write::Stored);
    assert!(real.join("store").join("credential").exists());
    set_mode(&open, 0o777);
    let _restore = Restore(vec![open.clone()]);
    symlink(&other, open.join("l2")).expect("a second link");
    fs::rename(open.join("l2"), open.join("l")).expect("the link points elsewhere");
    let read = store.read();
    let second = store.write(&secret(b"credential-two"));
    assert!(matches!(read, Read::Locked(Locked::Ancestor)), "{read:?}");
    assert_eq!(second, Write::Locked(Locked::Ancestor));
    assert!(!other.join("store").exists());
    assert_eq!(
        fs::read(real.join("store").join("credential")).expect("the credential"),
        b"credential-one"
    );
}

#[test]
fn a_link_of_the_person_in_a_sticky_directory_is_followed() {
    let base = scratch("sticky-link");
    let sticky = base.join("sticky");
    let real = base.join("real");
    fs::create_dir_all(&sticky).expect("sticky");
    fs::create_dir_all(&real).expect("real");
    set_mode(&sticky, 0o1777);
    let _restore = Restore(vec![sticky.clone()]);
    symlink(&real, sticky.join("l")).expect("a link");
    cycle(&sticky.join("l").join("store"));
}

#[test]
fn a_relative_link_is_walked_from_the_directory_that_holds_it() {
    let base = scratch("relative");
    let real = base.join("real");
    let holder = base.join("holder");
    fs::create_dir_all(&real).expect("real");
    fs::create_dir_all(&holder).expect("holder");
    symlink("../real", holder.join("l")).expect("a relative link");
    let store = store_in(&holder.join("l").join("store"));
    assert_eq!(store.write(&secret(b"relative")), Write::Stored);
    assert_eq!(
        fs::read(real.join("store").join("credential")).expect("the credential"),
        b"relative"
    );
    assert_eq!(found(store.read()), b"relative");
}

#[test]
fn a_relative_link_whose_target_passes_a_directory_that_others_may_write_is_refused() {
    let base = scratch("relative-open");
    let open = base.join("open");
    let inner = open.join("inner");
    let holder = base.join("holder");
    fs::create_dir_all(&inner).expect("inner");
    fs::create_dir_all(&holder).expect("holder");
    symlink("../open/inner", holder.join("l")).expect("a relative link");
    set_mode(&open, 0o777);
    let _restore = Restore(vec![open.clone()]);
    let store = store_in(&holder.join("l").join("store"));
    assert_eq!(store.write(&secret(b"x")), Write::Locked(Locked::Ancestor));
    assert!(!inner.join("store").exists());
}

#[test]
fn a_link_on_the_path_to_a_file_is_refused() {
    let base = scratch("link-to-file");
    fs::write(base.join("file"), b"").expect("a file");
    symlink(base.join("file"), base.join("l")).expect("a link");
    let store = store_in(&base.join("l").join("store"));
    assert!(matches!(store.read(), Read::Locked(Locked::Ancestor)));
    assert_eq!(store.write(&secret(b"x")), Write::Locked(Locked::Ancestor));
}

#[test]
fn a_dangling_link_on_the_path_answers_a_fault_and_makes_nothing() {
    let base = scratch("dangling-step");
    symlink(base.join("gone"), base.join("l")).expect("a link");
    let store = store_in(&base.join("l").join("store"));
    assert_eq!(read_fault(store.read()), ENOENT);
    assert_eq!(store.write(&secret(b"x")), Write::Fault(fault(ENOENT)));
    assert_eq!(store.delete(), Delete::Fault(fault(ENOENT)));
    assert!(!base.join("gone").exists());
}

#[test]
fn a_loop_of_links_answers_a_fault_without_waiting() {
    let base = scratch("loop");
    symlink("b", base.join("a")).expect("a");
    symlink("a", base.join("b")).expect("b");
    symlink("c", base.join("c")).expect("c");
    for start in ["a", "c"] {
        let dir = base.join(start).join("store");
        let (read, write) = within_five_seconds(move || {
            let store = store_in(&dir);
            (store.read(), store.write(&secret(b"x")))
        });
        assert_eq!(read_fault(read), ELOOP, "{start}");
        assert_eq!(write, Write::Fault(fault(ELOOP)), "{start}");
    }
}

#[test]
fn a_chain_of_links_is_followed_up_to_the_bound_and_no_further() {
    let base = scratch("chain");
    let real = base.join("real");
    fs::create_dir_all(&real).expect("real");
    symlink(&real, base.join(format!("l{LINK_BOUND}"))).expect("the last link");
    for n in (0..LINK_BOUND).rev() {
        symlink(format!("l{}", n + 1), base.join(format!("l{n}"))).expect("a link");
    }
    cycle(&base.join("l1").join("store"));
    let store = store_in(&base.join("l0").join("store"));
    assert_eq!(read_fault(store.read()), ELOOP);
}

#[test]
fn reads_through_the_links_of_the_root_directory_still_pass() {
    let unique = format!("halvo-secrets-walk-{}-absent", std::process::id());
    let mut places = vec![PathBuf::from("/tmp"), PathBuf::from("/private/tmp")];
    let out = Command::new("/usr/bin/getconf")
        .arg("DARWIN_USER_TEMP_DIR")
        .output()
        .expect("getconf");
    let temp = PathBuf::from(String::from_utf8(out.stdout).expect("a path").trim());
    assert!(temp.starts_with("/var/folders"), "{}", temp.display());
    places.push(temp);
    for place in places {
        let dir = place.join(&unique);
        let read = store_in(&dir).read();
        assert!(
            matches!(read, Read::NotStored),
            "{}: {read:?}",
            place.display()
        );
        assert!(!dir.exists());
    }
}

#[test]
fn a_step_that_the_person_may_pass_but_not_list_is_taken() {
    let base = scratch("search-only");
    let step = base.join("step");
    let parent = step.join("parent");
    fs::create_dir_all(&parent).expect("parent");
    set_mode(&step, 0o711);
    chmod(&["+a", &format!("user:{} deny list", person())], &step);
    let _restore = Restore(vec![step.clone()]);
    assert!(fs::read_dir(&step).is_err(), "the step may not be listed");
    cycle(&parent.join("store"));
}

#[test]
fn a_parent_that_the_person_may_write_and_pass_but_not_list_is_taken() {
    let base = scratch("write-search");
    let parent = base.join("parent");
    fs::create_dir_all(&parent).expect("parent");
    set_mode(&parent, 0o311);
    let _restore = Restore(vec![parent.clone()]);
    cycle(&parent.join("store"));
}

#[test]
fn an_allow_entry_that_names_the_person_alone_or_root_does_not_refuse_a_step() {
    let me = person();
    for entry in [
        format!("user:{me} allow add_file,add_subdirectory,delete_child,delete"),
        format!("user:{me} allow writesecurity,chown"),
        "user:root allow add_file,add_subdirectory,delete_child".to_string(),
    ] {
        let base = scratch("named");
        let parent = base.join("parent");
        fs::create_dir_all(&parent).expect("parent");
        chmod(&["+a", &entry], &parent);
        let _restore = Restore(vec![parent.clone()]);
        cycle(&parent.join("store"));
    }
}

#[test]
fn an_inherit_only_entry_does_not_refuse_a_step() {
    let base = scratch("inherit-only");
    let step = base.join("step");
    let parent = step.join("parent");
    fs::create_dir_all(&parent).expect("parent");
    chmod(
        &[
            "+a",
            "everyone allow add_file,add_subdirectory,delete_child,file_inherit,directory_inherit,only_inherit",
        ],
        &step,
    );
    let _restore = Restore(vec![step.clone()]);
    cycle(&parent.join("store"));
}

#[test]
fn an_allow_entry_for_another_user_or_a_group_still_refuses_a_step() {
    for entry in [
        "user:nobody allow add_file",
        "group:staff allow add_subdirectory",
        "group:wheel allow delete_child",
        "everyone allow add_file,file_inherit,directory_inherit",
    ] {
        let base = scratch("others");
        let step = base.join("step");
        let parent = step.join("parent");
        fs::create_dir_all(&parent).expect("parent");
        chmod(&["+a", entry], &step);
        let _restore = Restore(vec![step.clone()]);
        let store = store_in(&parent.join("store"));
        let read = store.read();
        assert!(
            matches!(read, Read::Locked(Locked::Ancestor)),
            "{entry}: {read:?}"
        );
        assert_eq!(
            store.write(&secret(b"x")),
            Write::Locked(Locked::Ancestor),
            "{entry}"
        );
    }
}

#[test]
fn an_access_list_that_cannot_be_read_answers_a_fault() {
    let base = scratch("unreadable-list");
    let step = base.join("step");
    let parent = step.join("parent");
    fs::create_dir_all(&parent).expect("parent");
    let store = store_in(&parent.join("store"));
    assert_eq!(store.write(&secret(b"kept")), Write::Stored);
    chmod(&["+a", "everyone deny readsecurity"], &step);
    let _restore = Restore(vec![step.clone()]);
    assert_eq!(read_fault(store.read()), EACCES);
    assert_eq!(store.write(&secret(b"x")), Write::Fault(fault(EACCES)));
    assert_eq!(store.delete(), Delete::Fault(fault(EACCES)));
    let fresh = store_in(&parent.join("fresh"));
    assert_eq!(read_fault(fresh.read()), EACCES);
    assert_eq!(fresh.write(&secret(b"x")), Write::Fault(fault(EACCES)));
    assert!(!parent.join("fresh").exists());
}

#[test]
fn a_missing_parent_answers_a_fault_and_makes_nothing() {
    let base = scratch("missing-parent");
    let store = store_in(&base.join("a").join("b").join("store"));
    assert_eq!(read_fault(store.read()), ENOENT);
    assert_eq!(store.write(&secret(b"x")), Write::Fault(fault(ENOENT)));
    assert_eq!(store.delete(), Delete::Fault(fault(ENOENT)));
    assert!(!base.join("a").exists());
}

#[test]
fn a_parent_that_exists_without_the_store_directory_reads_not_stored_and_then_stores() {
    let base = scratch("parent-exists");
    let dir = base.join("store");
    let store = store_in(&dir);
    assert!(matches!(store.read(), Read::NotStored));
    assert_eq!(store.delete(), Delete::NotStored);
    assert!(!dir.exists());
    assert_eq!(store.write(&secret(b"made")), Write::Stored);
    assert_eq!(
        fs::metadata(&dir)
            .expect("the store directory")
            .permissions()
            .mode()
            & 0o7777,
        0o700
    );
    assert_eq!(found(store.read()), b"made");
}
