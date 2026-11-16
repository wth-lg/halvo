//! The tests of ruling ST-45: the adversary's cases of its pass 2 at
//! 79f75b1b (prefix `p2_`), the tests that the ruling names for H1 to H5, and
//! the table test of the tail, one row for each arm of `delete_data_dir`.
//! Every store and every root lives in a directory that the test made. They
//! print answers, readings, counts and name shapes, never a key, a root, a
//! root id or a path.

use super::*;
use crate::custody::KeyLabel;

fn report_line(answer: &StoreResult<crate::store::DeleteReport>) -> String {
    match answer {
        Ok(_) | Err(StoreError::DeleteIncomplete(_)) => {
            let report = delete_report(answer);
            format!(
                "{} root={} roots_left={} names_left={} listed={} key_file_gone={} database_left={} files_left={} not_followed={}",
                if answer.is_ok() { "Ok" } else { "DeleteIncomplete" },
                report.root.as_text(),
                report.roots.left.len(),
                report.roots.names.len(),
                report.roots.listed,
                report.key_file_gone,
                report.database_left,
                report.files_left,
                report.not_followed
            )
        }
        Err(e) => format!("{e:?} (no report)"),
    }
}

fn names(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
        Err(e) => vec![format!("<{:?}>", e.kind())],
    }
}

/// The names with every root id cut to its length, so that no id is printed.
fn shapes(dir: &Path) -> Vec<String> {
    names(dir)
        .into_iter()
        .map(|name| match name.split_once('.') {
            Some((stem, ext)) if stem.len() == 32 => format!("<32 hex>.{ext}"),
            _ => name,
        })
        .collect()
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

fn wrapped(store_dir: &Path) -> WrappedKey {
    KeyFile::read(store_dir)
        .unwrap()
        .wrapped_store_key()
        .unwrap()
}

fn unwraps(roots_dir: &Path, root_id: RootId, wrapped: &WrappedKey) -> String {
    let custody = file_root_custody(roots_dir, TestProtector::new(0x5a).0, root_id);
    match custody.unwrap_store_key(wrapped, KeyLabel::STORE) {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

fn copy_reads(copy: &Path, roots_dir: &Path, root_id: RootId, segment: Uuid) -> String {
    match open_over_file_root(copy, roots_dir, root_id) {
        Ok(store) => {
            let mut out = Vec::new();
            match store.read_segment(segment, &mut out) {
                Ok(_) => format!("open=Ok segment_plaintext_equal={}", out == b"typed errors"),
                Err(e) => format!("open=Ok read={e:?}"),
            }
        }
        Err(e) => format!("open={e:?}"),
    }
}

fn public_reads(store_dir: &Path, roots: &Path, segment: Uuid) -> String {
    match EncryptedStore::open(store_dir, roots, DEFAULT_RETENTION) {
        Ok(store) => {
            let mut out = Vec::new();
            match store.read_segment(segment, &mut out) {
                Ok(_) => format!("open=Ok segment_plaintext_equal={}", out == b"typed errors"),
                Err(e) => format!("open=Ok read={e:?}"),
            }
        }
        Err(e) => format!("open={e:?}"),
    }
}

fn store_with_its_roots_inside(dir: &Path) -> PathBuf {
    let store_dir = dir.join("store");
    let root_id = RootId::fresh().unwrap();
    let custody = DevFallbackCustody::new(root_id, store_dir.join("roots"));
    let mut store =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
    store
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(store);
    store_dir
}

fn put_a_root(roots_dir: &Path) -> RootId {
    let id = RootId::fresh().unwrap();
    file_root_custody(roots_dir, TestProtector::new(0x5a).0, id)
        .create_root()
        .unwrap();
    id
}

fn zero_the_key_file(store_dir: &Path) {
    let len = std::fs::metadata(KeyFile::path(store_dir)).unwrap().len() as usize;
    std::fs::write(KeyFile::path(store_dir), vec![0u8; len]).unwrap();
}

fn write_mark(store_dir: &Path, root_id: RootId, reading: &str) {
    std::fs::write(
        store_dir.join("create.pending"),
        format!("{{\"custody_kind\":\"dev_fallback\",\"root_id\":\"{root_id}\",\"reading\":\"{reading}\"}}"),
    )
    .unwrap();
}

/// The adversary's p2_a. A first delete-all removes the root and is stopped by
/// something else (segments/ at 0500). The store directory is then moved and
/// a second store is made at its old name, in the scratch layout (roots
/// inside the store directory). The first handle's delete-all is called
/// again: it must not open the roots path again.
#[test]
fn p2_a_resumed_delete_all_keeps_the_root_of_a_store_made_at_its_old_name() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = store_with_its_roots_inside(dir.path());
    let roots = store_dir.join("roots");
    let mut first = EncryptedStore::open(&store_dir, &roots, DEFAULT_RETENTION).unwrap();
    set_mode(&store_dir.join("segments"), 0o500);
    let stopped = first.delete_all();
    println!("ST45 P2-A first call {}", report_line(&stopped));
    let moved = dir.path().join("moved-away");
    std::fs::rename(&store_dir, &moved).unwrap();
    let second_id = RootId::fresh().unwrap();
    let mut second = EncryptedStore::create(
        &store_dir,
        Box::new(DevFallbackCustody::new(second_id, roots.clone())),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (segment, _) = second
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(second);
    set_mode(&moved.join("segments"), 0o700);
    let resumed = first.delete_all();
    drop(first);
    let reads = public_reads(&store_dir, &roots, segment.segment_id);
    println!("ST45 P2-A resumed call {}", report_line(&resumed));
    println!(
        "ST45 P2-A the_directory_at_the_old_name={:?} its_roots={:?} the_store_made_there: {reads}",
        shapes(&store_dir),
        shapes(&roots)
    );
    assert!(
        reads.starts_with("open=Ok"),
        "the resumed delete-all destroyed the root of the store made at the old name: {reads}"
    );
    assert!(
        resumed.is_ok() && delete_report(&resumed).root_destroyed(),
        "the resumed call did not read its own root as destroyed from the first call and finish: {}",
        report_line(&resumed)
    );
}

/// H3: once a call on a handle read its root destroyed, a later call on that
/// handle reads its root and the other roots from the listing that the earlier
/// call kept. Here a roots directory that cannot be listed stands at the
/// roots path by the later call; the later call does not open it.
#[test]
fn a_later_delete_all_after_the_root_read_destroyed_does_not_open_the_roots_path_again() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let other = put_a_root(&roots_dir);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    set_mode(&store_dir.join("segments"), 0o500);
    let first = store.delete_all();
    set_mode(&store_dir.join("segments"), 0o700);
    let aside = dir.path().join("roots-aside");
    std::fs::rename(&roots_dir, &aside).unwrap();
    std::fs::create_dir(&roots_dir).unwrap();
    let third = put_a_root(&roots_dir);
    set_mode(&roots_dir, 0o000);
    let later = store.delete_all();
    set_mode(&roots_dir, 0o700);
    drop(store);
    println!("ST45 H3 first call {}", report_line(&first));
    println!("ST45 H3 later call {}", report_line(&later));
    println!(
        "ST45 H3 roots_put_at_the_path={:?} the_first_roots={:?}",
        shapes(&roots_dir),
        shapes(&aside)
    );
    let _ = (other, third);
    assert!(
        delete_report(&first).root_destroyed() && first.is_err(),
        "the first call: {}",
        report_line(&first)
    );
    assert!(
        later.is_ok() && delete_report(&later).roots.listed,
        "the later call opened the roots path again: {}",
        report_line(&later)
    );
    assert_eq!(
        root_files_in(&roots_dir).len(),
        1,
        "the later call destroyed a root in the directory put at the roots path"
    );
}

/// The adversary's p2_d: the tail and a second name at store.key.tmp of a
/// file of another party.
#[test]
fn p2_d_the_tail_writes_nothing_through_a_second_name_at_store_key_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    let theirs = dir.path().join("a-file-of-another-party");
    std::fs::write(&theirs, vec![7u8; 300]).unwrap();
    std::fs::hard_link(&theirs, store_dir.join("store.key.tmp")).unwrap();
    let answer = tail(&store_dir, &roots_dir);
    let bytes = std::fs::read(&theirs).unwrap();
    let kept = bytes == vec![7u8; 300];
    println!(
        "ST45 P2-D {} the_other_partys_file: len={} bytes_kept={kept} all_zero={}",
        report_line(&answer),
        bytes.len(),
        bytes.iter().all(|&b| b == 0)
    );
    assert!(
        kept,
        "the tail wrote zeros through a second name at store.key.tmp"
    );
    assert!(
        !store_dir.join("store.key.tmp").exists(),
        "the tail left the second name"
    );
}

/// H5 on the handle: delete-all runs the same removal.
#[test]
fn delete_all_writes_nothing_through_a_second_name_at_store_key_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let theirs = dir.path().join("a-file-of-another-party");
    std::fs::write(&theirs, vec![7u8; 300]).unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    std::fs::hard_link(&theirs, store_dir.join("store.key.tmp")).unwrap();
    let answer = store.delete_all();
    drop(store);
    let bytes = std::fs::read(&theirs).unwrap();
    println!(
        "ST45 H5 handle {} the_other_partys_file_kept={}",
        report_line(&answer),
        bytes == vec![7u8; 300]
    );
    assert_eq!(
        bytes,
        vec![7u8; 300],
        "delete-all wrote zeros through a second name at store.key.tmp"
    );
}

/// H5: a second name at store.key.tmp that is the store's own key file. The
/// temporary name goes first without a zero-fill, and the zero-fill of
/// store.key reaches every name of the file: a third name outside the store
/// ends all zero. It passes at the commit with the tests and no change
/// (hosted run 43578145799): it guards against a removal of store.key.tmp
/// that would leave the key file's bytes, and is not a failing-first test.
#[test]
fn the_store_key_file_under_a_second_name_at_store_key_tmp_ends_all_zero_under_every_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    let third = dir.path().join("a-third-name-of-the-key-file");
    std::fs::hard_link(KeyFile::path(&store_dir), &third).unwrap();
    std::fs::hard_link(KeyFile::path(&store_dir), store_dir.join("store.key.tmp")).unwrap();
    let answer = tail(&store_dir, &roots_dir);
    let bytes = std::fs::read(&third).unwrap();
    println!(
        "ST45 H5 own key file {} third_name_len={} all_zero={}",
        report_line(&answer),
        bytes.len(),
        bytes.iter().all(|&b| b == 0)
    );
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(
        !bytes.is_empty() && bytes.iter().all(|&b| b == 0),
        "a name of the store's own key file keeps its bytes"
    );
}

/// H5: an entry at store.key.tmp that is no regular file and that the call
/// cannot remove (a directory that holds a file) is counted in not_followed,
/// so that the report names a cause, and the file in it is kept.
#[test]
fn an_entry_at_store_key_tmp_that_no_call_can_remove_is_counted_as_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    std::fs::create_dir(store_dir.join("store.key.tmp")).unwrap();
    std::fs::write(
        store_dir.join("store.key.tmp").join("theirs"),
        vec![7u8; 10],
    )
    .unwrap();
    let first = tail(&store_dir, &roots_dir);
    let second = tail(&store_dir, &roots_dir);
    let theirs = std::fs::read(store_dir.join("store.key.tmp").join("theirs"));
    println!("ST45 H5 count first tail {}", report_line(&first));
    println!("ST45 H5 count second tail {}", report_line(&second));
    assert_eq!(
        theirs.ok(),
        Some(vec![7u8; 10]),
        "a call removed the file of another party"
    );
    for answer in [&first, &second] {
        let report = delete_report(answer);
        assert!(
            answer.is_err() && !report.key_file_gone && report.not_followed >= 1,
            "the entry at store.key.tmp is not counted: {}",
            report_line(answer)
        );
    }
}

/// The adversary's p2_b: a root file that the root step could not open
/// (mode 000, the unit's own case; mode 0200), and the tail three times.
#[test]
fn p2_b_a_tail_after_a_tail_that_left_the_root_file_names_it_or_destroys_it() {
    let mut problems = Vec::new();
    for mode in [0o000u32, 0o200] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
        let wrapped = wrapped(&store_dir);
        let copy = dir.path().join("copy-of-the-store");
        copy_tree(&store_dir, &copy);
        let root_file =
            FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0).root_path(root_id);
        set_mode(&root_file, mode);
        let first = tail(&store_dir, &roots_dir);
        let second = tail(&store_dir, &roots_dir);
        let third = tail(&store_dir, &roots_dir);
        let stands = root_file.exists();
        println!("ST45 P2-B mode={mode:o} first tail {}", report_line(&first));
        println!(
            "ST45 P2-B mode={mode:o} second tail {}",
            report_line(&second)
        );
        println!("ST45 P2-B mode={mode:o} third tail {}", report_line(&third));
        if stands {
            set_mode(&root_file, 0o600);
        }
        let bytes_kept = stands && std::fs::read(&root_file).unwrap().iter().any(|&b| b != 0);
        println!(
            "ST45 P2-B mode={mode:o} roots_dir_now={:?} root_file_stands={stands} its_bytes_are_kept={bytes_kept} after_chmod_it_unwraps={} copy_of_the_store: {}",
            shapes(&roots_dir),
            unwraps(&roots_dir, root_id, &wrapped),
            copy_reads(&copy, &roots_dir, root_id, segment)
        );
        if second.is_ok() && bytes_kept {
            problems.push(format!(
                "mode {mode:o}: the second tail answered Ok and names no root, and the root file stands with its bytes"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("; "));
}

/// The adversary's p2_b2: the same on a handle whose root file is made
/// unreadable after the open, then the tail.
#[test]
fn p2_b2_a_handle_whose_root_file_cannot_be_opened_then_the_tail() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file =
        FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0).root_path(root_id);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    set_mode(&root_file, 0o000);
    let first = store.delete_all();
    let again = store.delete_all();
    drop(store);
    let tail = tail(&store_dir, &roots_dir);
    let stands = root_file.exists();
    println!("ST45 P2-B2 handle first {}", report_line(&first));
    println!("ST45 P2-B2 handle again {}", report_line(&again));
    println!(
        "ST45 P2-B2 tail after the handle {} root_file_stands={stands}",
        report_line(&tail)
    );
    if stands {
        set_mode(&root_file, 0o600);
    }
    assert!(
        !(tail.is_ok() && stands),
        "the tail after the handle answered Ok and names no root, and the root file stands"
    );
}

/// The adversary's p2_b3: the root file that two tails left, and the delete
/// of the next store under that roots directory.
#[test]
fn p2_b3_the_next_store_under_a_roots_directory_with_a_root_file_that_no_call_names() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file =
        FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0).root_path(root_id);
    set_mode(&root_file, 0o000);
    let first = tail(&store_dir, &roots_dir);
    let second = tail(&store_dir, &roots_dir);
    println!("ST45 P2-B3 first tail {}", report_line(&first));
    println!("ST45 P2-B3 second tail {}", report_line(&second));
    let next_id = RootId::fresh().unwrap();
    let mut next = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            next_id,
        )),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let deleted = next.delete_all();
    drop(next);
    println!(
        "ST45 P2-B3 delete_all of the next store {}",
        report_line(&deleted)
    );
    println!(
        "ST45 P2-B3 roots_dir_now={:?} the_first_root_file_stands={}",
        shapes(&roots_dir),
        root_file.exists()
    );
    if root_file.exists() {
        set_mode(&root_file, 0o600);
    }
    assert!(
        second.is_err() || !root_file.exists(),
        "the second tail answered Ok while the root file that the first tail named stands"
    );
}

#[cfg(target_os = "macos")]
fn set_immutable(path: &Path, on: bool) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let flags = if on { libc::UF_IMMUTABLE } else { 0 };
    // SAFETY: c is NUL-terminated and outlives the call, which reads only it.
    assert_eq!(unsafe { libc::chflags(c.as_ptr(), flags) }, 0);
}

/// H2: a root file that no open reaches and whose mode the step cannot change
/// (the user's immutable flag) keeps its name; every tail tries it again and
/// names it under its real name, and none answers Ok.
#[cfg(target_os = "macos")]
#[test]
fn a_root_file_that_no_open_reaches_is_named_by_every_tail_and_none_answers_ok() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file =
        FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0).root_path(root_id);
    set_mode(&root_file, 0o000);
    set_immutable(&root_file, true);
    let first = tail(&store_dir, &roots_dir);
    let second = tail(&store_dir, &roots_dir);
    set_immutable(&root_file, false);
    set_mode(&root_file, 0o600);
    println!("ST45 H2 first tail {}", report_line(&first));
    println!("ST45 H2 second tail {}", report_line(&second));
    let expected = vec![format!("{root_id}.root")];
    for answer in [&first, &second] {
        let report = delete_report(answer);
        assert!(
            answer.is_err(),
            "a tail answered Ok: {}",
            report_line(answer)
        );
        assert_eq!(report.roots.names, expected, "{}", report_line(answer));
    }
}

/// The adversary's p2_c: the root file renamed aside inside the roots
/// directory, an empty file at its name, the handle's delete-all.
#[test]
fn p2_c_delete_all_reads_destroyed_only_when_it_removed_the_root_file_itself() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    let wrapped = wrapped(&store_dir);
    let copy = dir.path().join("copy-of-the-store");
    copy_tree(&store_dir, &copy);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let aside = roots_dir.join("kept-aside");
    std::fs::rename(&root_file, &aside).unwrap();
    std::fs::write(&root_file, b"").unwrap();
    let answer = store.delete_all();
    drop(store);
    let report = delete_report(&answer);
    println!("ST45 P2-C {}", report_line(&answer));
    println!("ST45 P2-C roots_dir_now={:?}", shapes(&roots_dir));
    if aside.exists() {
        std::fs::rename(&aside, &root_file).unwrap();
    }
    let unwraps = unwraps(&roots_dir, root_id, &wrapped);
    println!(
        "ST45 P2-C the_file_put_back_under_its_name_unwraps={unwraps} copy_of_the_store: {}",
        copy_reads(&copy, &roots_dir, root_id, segment)
    );
    assert!(
        !(report.root_destroyed() && unwraps == "Ok"),
        "the root reads destroyed and the root file still unwraps the store key: {}",
        report_line(&answer)
    );
}

/// The adversary's p2_c2: the root file moved to another directory of the
/// test and a link to it at its name, then the tail.
#[test]
fn p2_c2_the_tail_reads_destroyed_only_when_it_removed_the_root_file_and_not_a_link_to_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    let wrapped = wrapped(&store_dir);
    let copy = dir.path().join("copy-of-the-store");
    copy_tree(&store_dir, &copy);
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    set_mode(&elsewhere, 0o700);
    let moved = elsewhere.join(format!("{root_id}.root"));
    std::fs::rename(&root_file, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &root_file).unwrap();
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    let answer = tail(&store_dir, &roots_dir);
    let report = delete_report(&answer);
    let unwraps = unwraps(&elsewhere, root_id, &wrapped);
    println!("ST45 P2-C2 open_with_the_link_at_the_root_name={opened}");
    println!("ST45 P2-C2 tail {}", report_line(&answer));
    println!(
        "ST45 P2-C2 roots_dir_now={:?} the_file_behind_the_link_unwraps={unwraps} copy_of_the_store: {}",
        shapes(&roots_dir),
        copy_reads(&copy, &elsewhere, root_id, segment)
    );
    assert!(
        !(report.root_destroyed() && unwraps == "Ok"),
        "the root reads destroyed, and the root file behind the link still unwraps the store key: {}",
        report_line(&answer)
    );
}

/// H4: a file of another length than the root file at the root's name, a
/// second name of a file of another party, is removed as the entry that it
/// is, with no zero-fill and no write through it, and the root reads left.
#[test]
fn a_file_of_another_length_at_the_root_name_is_removed_unwritten_and_the_root_reads_left() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let aside = dir.path().join("the-root-file-kept-aside");
    std::fs::rename(&root_file, &aside).unwrap();
    let theirs = dir.path().join("a-file-of-another-party");
    std::fs::write(&theirs, vec![7u8; 48]).unwrap();
    std::fs::hard_link(&theirs, &root_file).unwrap();
    let answer = tail(&store_dir, &roots_dir);
    let bytes = std::fs::read(&theirs).unwrap();
    println!(
        "ST45 H4 another length {} the_other_partys_file_kept={} root_name_stands={}",
        report_line(&answer),
        bytes == vec![7u8; 48],
        root_file.exists()
    );
    assert_eq!(
        bytes,
        vec![7u8; 48],
        "the tail wrote through the entry at the root name"
    );
    assert!(!root_file.exists(), "the entry at the root name stays");
    assert_eq!(
        delete_report(&answer).root,
        crate::store::RootReading::Left(Some(root_id)),
        "{}",
        report_line(&answer)
    );
    assert!(answer.is_err());
}

/// H4: an all-zero file of the root file's length at the root's name (a
/// delete stopped after its zeros) counts: the next tail reads the root as
/// destroyed and answers Ok.
#[test]
fn an_all_zero_root_file_of_the_root_length_reads_destroyed_when_the_tail_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let len = std::fs::metadata(&root_file).unwrap().len() as usize;
    std::fs::write(&root_file, vec![0u8; len]).unwrap();
    let answer = tail(&store_dir, &roots_dir);
    println!("ST45 H4 all zero len={len} {}", report_line(&answer));
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(
        delete_report(&answer).root_destroyed(),
        "{}",
        report_line(&answer)
    );
    assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
}

/// The arm of `delete_data_dir` that a row reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Arm {
    /// The store directory opens and the tail takes its lock.
    LockHeld,
    /// Nothing stands at the store name.
    NothingAtTheName,
    /// A link to another directory stands at the store name.
    LinkAtTheName,
    /// A regular file stands at the store name.
    FileAtTheName,
    /// The store directory stands and cannot be opened (mode 000).
    CannotOpen,
    /// The store directory opens and its lock fails for a reason other than
    /// a holder (a test failpoint).
    LockFails,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Naming {
    /// store.key names the store's own root.
    Named,
    /// Nothing names a root (store.key zeroed).
    Unnamed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RootsAt {
    /// The roots directory holds the store's root and one other root.
    Present,
    /// Nothing stands at the roots path.
    Absent,
    /// A roots directory of mode 000 holds both roots.
    NotListable,
}

/// What stands at the roots path after the call.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RootsAfter {
    /// No root file is left.
    Empty,
    /// Both root files stay.
    Both,
    /// The store's own root is gone and the other one stays.
    TheOtherOne,
    /// The store's own root stays and the other one is gone; no row expects
    /// it.
    OnlyOwn,
    /// Nothing stands at the roots path.
    Nothing,
}

struct Row {
    arm: Arm,
    naming: Naming,
    roots: RootsAt,
    ok: bool,
    reading: &'static str,
    listed: bool,
    after: RootsAfter,
}

const fn row(
    arm: Arm,
    naming: Naming,
    roots: RootsAt,
    ok: bool,
    reading: &'static str,
    listed: bool,
    after: RootsAfter,
) -> Row {
    Row {
        arm,
        naming,
        roots,
        ok,
        reading,
        listed,
        after,
    }
}

use Arm::*;
use Naming::*;
use RootsAfter::{Both, Empty, Nothing, OnlyOwn, TheOtherOne};
use RootsAt::*;

/// One row for each arm of `delete_data_dir` by named or unnamed and by
/// roots present, absent or not listable. A named root with nothing at the
/// roots path reads left with the roots location not listed (ruling ST-44 F1
/// (d)); with nothing named, nothing at the roots path is an answer, listed. An arm in which nothing can name a
/// root (nothing, a link or a file at the store name) has the unnamed rows
/// alone. The witness binds the two arms without the lock that run a root
/// step (the lock that fails), with the named root alone; the store directory
/// that cannot be opened gets no root step.
const TAIL_ROWS: &[Row] = &[
    row(LockHeld, Named, Present, true, "yes", true, Empty),
    row(LockHeld, Named, Absent, false, "no", false, Nothing),
    row(LockHeld, Named, NotListable, false, "no", false, Both),
    row(LockHeld, Unnamed, Present, true, "unknown", true, Empty),
    row(LockHeld, Unnamed, Absent, true, "unknown", true, Nothing),
    row(
        LockHeld,
        Unnamed,
        NotListable,
        false,
        "unknown",
        false,
        Both,
    ),
    row(
        NothingAtTheName,
        Unnamed,
        Present,
        true,
        "unknown",
        true,
        Empty,
    ),
    row(
        NothingAtTheName,
        Unnamed,
        Absent,
        true,
        "unknown",
        true,
        Nothing,
    ),
    row(
        NothingAtTheName,
        Unnamed,
        NotListable,
        false,
        "unknown",
        false,
        Both,
    ),
    row(
        LinkAtTheName,
        Unnamed,
        Present,
        true,
        "unknown",
        true,
        Empty,
    ),
    row(
        LinkAtTheName,
        Unnamed,
        Absent,
        true,
        "unknown",
        true,
        Nothing,
    ),
    row(
        LinkAtTheName,
        Unnamed,
        NotListable,
        false,
        "unknown",
        false,
        Both,
    ),
    row(
        FileAtTheName,
        Unnamed,
        Present,
        true,
        "unknown",
        true,
        Empty,
    ),
    row(
        FileAtTheName,
        Unnamed,
        Absent,
        true,
        "unknown",
        true,
        Nothing,
    ),
    row(
        FileAtTheName,
        Unnamed,
        NotListable,
        false,
        "unknown",
        false,
        Both,
    ),
    row(CannotOpen, Named, Present, false, "no", false, Both),
    row(CannotOpen, Named, Absent, false, "no", false, Nothing),
    row(CannotOpen, Named, NotListable, false, "no", false, Both),
    row(CannotOpen, Unnamed, Present, false, "no", false, Both),
    row(CannotOpen, Unnamed, Absent, false, "no", false, Nothing),
    row(CannotOpen, Unnamed, NotListable, false, "no", false, Both),
    row(LockFails, Named, Present, false, "yes", true, TheOtherOne),
    row(LockFails, Named, Absent, false, "no", false, Nothing),
    row(LockFails, Named, NotListable, false, "no", false, Both),
    row(LockFails, Unnamed, Present, false, "unknown", false, Both),
    row(LockFails, Unnamed, Absent, false, "unknown", false, Nothing),
    row(
        LockFails,
        Unnamed,
        NotListable,
        false,
        "unknown",
        false,
        Both,
    ),
];

/// Runs one row in a directory of its own and answers the mismatches.
fn run_row(row: &Row) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let roots_dir = data.join("roots");
    let store_at = data.join("store");
    let made_at = match row.arm {
        LockHeld | CannotOpen | LockFails => store_at.clone(),
        NothingAtTheName | LinkAtTheName | FileAtTheName => dir.path().join("elsewhere"),
    };
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let mut store = EncryptedStore::create(&made_at, Box::new(custody), DEFAULT_RETENTION).unwrap();
    store
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(store);
    let other = put_a_root(&roots_dir);
    if row.naming == Unnamed {
        zero_the_key_file(&made_at);
    }
    match row.arm {
        NothingAtTheName => {}
        LinkAtTheName => std::os::unix::fs::symlink(&made_at, &store_at).unwrap(),
        FileAtTheName => std::fs::write(&store_at, b"a file of another party").unwrap(),
        LockHeld | CannotOpen | LockFails => {}
    }
    match row.roots {
        Present => {}
        Absent => std::fs::rename(&roots_dir, dir.path().join("roots-elsewhere")).unwrap(),
        NotListable => set_mode(&roots_dir, 0o000),
    }
    if row.arm == CannotOpen {
        set_mode(&store_at, 0o000);
    }
    if row.arm == LockFails {
        crate::store::failpoint::arm("tail_lock");
    }
    let answer = tail(&store_at, &roots_dir);
    crate::store::failpoint::arm("");
    if row.arm == CannotOpen {
        set_mode(&store_at, 0o700);
    }
    if row.roots == NotListable {
        set_mode(&roots_dir, 0o700);
    }
    let own = format!("{root_id}.root");
    let theirs = format!("{other}.root");
    let after = if !roots_dir.exists() {
        Nothing
    } else {
        let files = root_files_in(&roots_dir);
        match (files.contains(&own), files.contains(&theirs)) {
            (false, false) => Empty,
            (true, true) => Both,
            (false, true) => TheOtherOne,
            (true, false) => OnlyOwn,
        }
    };
    let line = report_line(&answer);
    println!(
        "ST45 TABLE arm={:?} naming={:?} roots={:?} {line} roots_after={after:?}",
        row.arm, row.naming, row.roots
    );
    let mut wrong = Vec::new();
    let (ok, reading, listed) = match &answer {
        Ok(_) | Err(StoreError::DeleteIncomplete(_)) => {
            let report = delete_report(&answer);
            (answer.is_ok(), report.root.as_text(), report.roots.listed)
        }
        Err(e) => {
            wrong.push(format!(
                "{:?}/{:?}/{:?}: {e:?}",
                row.arm, row.naming, row.roots
            ));
            return wrong;
        }
    };
    if (ok, reading, listed, after) != (row.ok, row.reading, row.listed, row.after) {
        wrong.push(format!(
            "{:?}/{:?}/{:?}: answered ok={ok} reading={reading} listed={listed} roots_after={after:?}, the row says ok={} reading={} listed={} roots_after={:?}",
            row.arm, row.naming, row.roots, row.ok, row.reading, row.listed, row.after
        ));
    }
    wrong
}

#[test]
fn the_tail_answers_each_arm_as_its_row_says_and_leaves_the_roots_directory_as_its_row_says() {
    let wrong: Vec<String> = TAIL_ROWS.iter().flat_map(run_row).collect();
    println!("ST45 TABLE rows={} wrong={}", TAIL_ROWS.len(), wrong.len());
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// H1: the tail opens no roots path that leads through a store name at which
/// it did not open a directory itself. A link at the store name leads to a
/// store whose roots lie inside it; the tail removes the link and leaves
/// that store and its root whole.
#[test]
fn the_tail_opens_no_roots_path_through_a_link_at_the_store_name() {
    let dir = tempfile::tempdir().unwrap();
    let behind = store_with_its_roots_inside(&dir.path().join("behind"));
    let roots_behind = behind.join("roots");
    let roots_before = root_files_in(&roots_behind);
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    std::os::unix::fs::symlink(&behind, data.join("store")).unwrap();
    let answer = tail(&data.join("store"), &data.join("store").join("roots"));
    println!("ST45 H1 roots through a link {}", report_line(&answer));
    assert_eq!(root_files_in(&roots_behind), roots_before);
    assert!(std::fs::symlink_metadata(data.join("store")).is_err());
    assert!(answer.is_ok(), "{}", report_line(&answer));
}

/// H1: in the arm whose lock fails for a reason other than a holder (a test
/// failpoint), beside a key file that this build does not handle, the tail
/// opens no roots path, so its report reads the roots as not listed and the
/// root files stay.
#[test]
fn the_tail_whose_lock_fails_beside_a_key_file_of_version_1_reads_the_roots_as_not_listed() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    put_a_root(&roots_dir);
    let older = RootId::fresh().unwrap();
    std::fs::write(
        KeyFile::path(&store_dir),
        format!(
            "{{\"version\":1,\"custody_kind\":\"dev_fallback\",\"root_id\":\"{}\",\"wrapped_store_key_b64\":\"AAAA\"}}",
            uuid::Uuid::from_bytes(*older.as_bytes()).hyphenated()
        ),
    )
    .unwrap();
    let roots_before = root_files_in(&roots_dir);
    crate::store::failpoint::arm("tail_lock");
    let answer = tail(&store_dir, &roots_dir);
    crate::store::failpoint::arm("");
    println!(
        "ST45 H1 lock fails beside version 1 {}",
        report_line(&answer)
    );
    let report = delete_report(&answer);
    assert!(answer.is_err(), "{}", report_line(&answer));
    assert_eq!(root_files_in(&roots_dir), roots_before);
    assert!(
        !report.roots.listed,
        "the report reads the roots as listed, and no call opened them: {}",
        report_line(&answer)
    );
}

/// H1: beside a key file that this build does not handle the sweep runs; the
/// reading stays not handled on that call and reads unnamed on the next.
#[test]
fn the_tail_beside_a_key_file_of_version_1_sweeps_the_roots_and_the_next_tail_reads_unnamed() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    put_a_root(&roots_dir);
    let older = RootId::fresh().unwrap();
    std::fs::write(
        KeyFile::path(&store_dir),
        format!(
            "{{\"version\":1,\"custody_kind\":\"dev_fallback\",\"root_id\":\"{}\",\"wrapped_store_key_b64\":\"AAAA\"}}",
            uuid::Uuid::from_bytes(*older.as_bytes()).hyphenated()
        ),
    )
    .unwrap();
    let first = tail(&store_dir, &roots_dir);
    let second = tail(&store_dir, &roots_dir);
    println!("ST45 H1 version 1 first {}", report_line(&first));
    println!("ST45 H1 version 1 second {}", report_line(&second));
    assert_eq!(delete_report(&first).root_not_handled, Some(older));
    assert!(first.is_err());
    assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
    assert!(second.is_ok(), "{}", report_line(&second));
    assert_eq!(
        delete_report(&second).root,
        crate::store::RootReading::Unnamed
    );
}

/// H1: for the tail a mark that reads owed names no root, and the sweep
/// destroys that root file with the rest; a mark that reads made names its
/// root, which reads destroyed; from store-3b a made mark without the
/// identity of its root file reads owed.
#[test]
fn the_tail_sweeps_the_root_of_an_owed_mark_and_reads_the_root_of_a_made_mark_destroyed() {
    for (reading, expected) in [
        ("owed", "unknown"),
        ("made", "yes"),
        ("made without the identity of its root file", "unknown"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
        std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
        let marked = put_a_root(&roots_dir);
        write_mark(&store_dir, marked, reading.split(' ').next().unwrap());
        if reading == "made" {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::metadata(roots_dir.join(format!("{marked}.root"))).unwrap();
            std::fs::write(
                store_dir.join("create.pending"),
                format!(
                    "{{\"custody_kind\":\"dev_fallback\",\"root_id\":\"{marked}\",\"reading\":\"made\",\"root_file\":{{\"volume\":{},\"file\":{}}}}}",
                    meta.dev(),
                    meta.ino()
                ),
            )
            .unwrap();
        }
        let answer = tail(&store_dir, &roots_dir);
        println!("ST45 H1 mark={reading} {}", report_line(&answer));
        assert!(answer.is_ok(), "{reading}: {}", report_line(&answer));
        assert_eq!(delete_report(&answer).root.as_text(), expected);
        assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
    }
}

/// The adversary's p3_f. Case 29 again (the store moved, a second store made
/// at its old name, roots inside), with one entry more in the second store's
/// roots: a name of the first store's root id that cannot be its root file
/// (an empty file at the root's name; a partial name). It is no witness: the
/// first handle's delete-all destroys nothing there, and the second store
/// opens and reads (ruling ST-45 Addition 2 (a)).
#[test]
fn p3_f_the_witness_is_no_entry_that_cannot_be_the_roots_own_file() {
    let mut problems = Vec::new();
    for what in [
        "an empty file at the root's name",
        "a partial name of the root id",
        "a partial name of the root id that holds 32 bytes",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = store_with_its_roots_inside(dir.path());
        let first_id = KeyFile::read(&store_dir).unwrap().root_id;
        let roots = store_dir.join("roots");
        let mut first = EncryptedStore::open(&store_dir, &roots, DEFAULT_RETENTION).unwrap();
        let moved = dir.path().join("moved-away");
        std::fs::rename(&store_dir, &moved).unwrap();
        let second_id = RootId::fresh().unwrap();
        let mut second = EncryptedStore::create(
            &store_dir,
            Box::new(DevFallbackCustody::new(second_id, roots.clone())),
            DEFAULT_RETENTION,
        )
        .unwrap();
        let (segment, _) = second
            .write_segment(&mut Cursor::new(b"typed errors"))
            .unwrap();
        drop(second);
        let entry = if what.starts_with("an empty") {
            let entry = roots.join(format!("{first_id}.root"));
            std::fs::write(&entry, b"").unwrap();
            entry
        } else {
            let entry = roots.join(format!("{first_id}.0123456789abcdef.root.tmp"));
            let len = if what.ends_with("32 bytes") { 32 } else { 10 };
            std::fs::write(&entry, vec![7u8; len]).unwrap();
            entry
        };
        let answer = first.delete_all();
        drop(first);
        let reads = public_reads(&store_dir, &roots, segment.segment_id);
        println!("ST45 P3-F {what:?} {}", report_line(&answer));
        println!(
            "ST45 P3-F {what:?} the_roots_at_the_old_name={:?} the_store_made_there: {reads}",
            shapes(&roots)
        );
        if !reads.starts_with("open=Ok") {
            problems.push(format!("{what}: {reads}"));
        }
        if !entry.exists() {
            problems.push(format!("{what}: the step removed the entry that it met"));
        }
        if answer.is_ok() || !delete_report(&answer).root_destroyed() {
            problems.push(format!(
                "{what}: the held handle's delete must destroy the root in the moved directory and answer DeleteIncomplete: {}",
                report_line(&answer)
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "an entry that cannot be the root's file served as the witness: {}",
        problems.join("; ")
    );
}

/// Addition 2 (a) in a tail without the lock (a test failpoint at the lock):
/// an empty file at the root's name is no witness, so the step destroys
/// nothing in that directory, also not that entry, and the root reads left.
#[test]
fn the_tail_whose_lock_fails_takes_no_empty_file_at_the_root_name_as_its_witness() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let aside = dir.path().join("the-root-file-kept-aside");
    std::fs::rename(&root_file, &aside).unwrap();
    std::fs::write(&root_file, b"").unwrap();
    let other = put_a_root(&roots_dir);
    crate::store::failpoint::arm("tail_lock");
    let answer = tail(&store_dir, &roots_dir);
    crate::store::failpoint::arm("");
    println!(
        "ST45 A2 lock fails {} roots_dir_now={:?}",
        report_line(&answer),
        shapes(&roots_dir)
    );
    assert!(root_file.exists(), "the step removed the entry that it met");
    assert!(
        roots_dir.join(format!("{other}.root")).exists(),
        "the step destroyed another root without its witness"
    );
    assert_eq!(
        delete_report(&answer).root,
        crate::store::RootReading::Left(Some(root_id)),
        "{}",
        report_line(&answer)
    );
}

/// The adversary's p3_e, restated to Addition 2 (b): the unnamed sweep meets a
/// file of another length at a root name, a second name of a file of another
/// party. It removes that name with no write through it.
#[test]
fn p3_e_the_unnamed_sweep_removes_a_second_name_at_a_root_name_without_writing_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let roots_dir = dir.path().join("roots");
    std::fs::create_dir(&roots_dir).unwrap();
    set_mode(&roots_dir, 0o700);
    let theirs = dir.path().join("a-file-of-another-party");
    std::fs::write(&theirs, vec![7u8; 300]).unwrap();
    let any_id = RootId::fresh().unwrap();
    let at_the_root_name = roots_dir.join(format!("{any_id}.root"));
    std::fs::hard_link(&theirs, &at_the_root_name).unwrap();
    let answer = tail(&dir.path().join("store"), &roots_dir);
    let bytes = std::fs::read(&theirs).unwrap();
    println!(
        "ST45 P3-E {} roots_dir_now={:?} the_other_partys_file: len={} bytes_kept={}",
        report_line(&answer),
        shapes(&roots_dir),
        bytes.len(),
        bytes == vec![7u8; 300]
    );
    assert_eq!(
        bytes,
        vec![7u8; 300],
        "the sweep wrote through the second name"
    );
    assert!(!at_the_root_name.exists(), "the sweep left the second name");
    assert!(answer.is_ok(), "{}", report_line(&answer));
}

/// Addition 2 (b) in the handle's form: beside the store's own root, a second
/// name of a file of another party at another root id's name is removed with
/// no write through it.
#[test]
fn delete_all_removes_a_second_name_at_another_root_name_without_writing_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let theirs = dir.path().join("a-file-of-another-party");
    std::fs::write(&theirs, vec![7u8; 300]).unwrap();
    let any_id = RootId::fresh().unwrap();
    let at_the_root_name = roots_dir.join(format!("{any_id}.root"));
    std::fs::hard_link(&theirs, &at_the_root_name).unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let answer = store.delete_all();
    drop(store);
    let bytes = std::fs::read(&theirs).unwrap();
    println!(
        "ST45 A2 handle {} the_other_partys_file_kept={}",
        report_line(&answer),
        bytes == vec![7u8; 300]
    );
    assert_eq!(
        bytes,
        vec![7u8; 300],
        "delete-all wrote through the second name"
    );
    assert!(
        !at_the_root_name.exists(),
        "delete-all left the second name"
    );
    assert!(answer.is_ok(), "{}", report_line(&answer));
}

/// What a test does at the seam after the root step's listing (ruling ST-46
/// K1): the root file, or a partial name of the root, renamed aside within the
/// roots directory, and what then stands at its name.
#[derive(Clone, Copy, Debug, PartialEq)]
enum AfterTheListing {
    Nothing,
    RenameAlone,
    RenameAndLinkAtTheName,
    RenameAndEmptyFileAtTheName,
    RenamePartialAside,
}

/// One delete-all on a handle with `what` done after its listing: the answer,
/// the bytes of what was put aside (empty for Nothing), and whether a copy of
/// the store taken before the delete opens and reads with the aside file put
/// back at its name.
fn delete_all_with_a_move_after_the_listing(
    what: AfterTheListing,
) -> (
    StoreResult<crate::store::DeleteReport>,
    RootId,
    Vec<u8>,
    bool,
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    let copy = dir.path().join("copy-of-the-store");
    copy_tree(&store_dir, &copy);
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let partial = roots_dir.join(format!("{root_id}.0123456789abcdef.root.tmp"));
    if what == AfterTheListing::RenamePartialAside {
        std::fs::copy(&root_file, &partial).unwrap();
    }
    let aside = roots_dir.join("put-aside");
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let (from, to, name) = (
        if what == AfterTheListing::RenamePartialAside {
            partial.clone()
        } else {
            root_file.clone()
        },
        aside.clone(),
        root_file.clone(),
    );
    let _seam = crate::store::failpoint::at(
        crate::custody::file_root::SEAM_AFTER_THE_LISTING,
        move || match what {
            AfterTheListing::Nothing => {}
            AfterTheListing::RenameAlone | AfterTheListing::RenamePartialAside => {
                std::fs::rename(&from, &to).unwrap()
            }
            AfterTheListing::RenameAndLinkAtTheName => {
                std::fs::rename(&from, &to).unwrap();
                std::os::unix::fs::symlink(&to, &name).unwrap();
            }
            AfterTheListing::RenameAndEmptyFileAtTheName => {
                std::fs::rename(&from, &to).unwrap();
                std::fs::write(&name, b"").unwrap();
            }
        },
    );
    let answer = store.delete_all();
    drop(store);
    let aside_bytes = std::fs::read(&aside).unwrap_or_default();
    let mut copy_opens = false;
    if what != AfterTheListing::Nothing
        && what != AfterTheListing::RenamePartialAside
        && aside.exists()
    {
        let _ = std::fs::remove_file(&root_file);
        std::fs::rename(&aside, &root_file).unwrap();
        copy_opens = copy_reads(&copy, &roots_dir, root_id, segment)
            .starts_with("open=Ok segment_plaintext_equal=true");
    }
    println!(
        "ST46 K1 {what:?} {} aside_len={} aside_nonzero={} copy_opens_and_reads={copy_opens}",
        report_line(&answer),
        aside_bytes.len(),
        aside_bytes.iter().any(|&b| b != 0)
    );
    (answer, root_id, aside_bytes, copy_opens)
}

/// ST-46 K1, the review's case B1: the root file renamed aside after the
/// listing and before the step's look, in a roots directory of usual size.
/// With store-3b's held handle the step finds the file of the identity that
/// the store read by one more listing and destroys it where it lies, so no
/// copy of the store opens.
#[test]
fn a_root_file_renamed_aside_after_the_listing_is_destroyed_where_it_lies_and_no_copy_opens() {
    let (answer, _, aside, copy_opens) =
        delete_all_with_a_move_after_the_listing(AfterTheListing::RenameAlone);
    assert!(
        !copy_opens,
        "B1: a copy of the store opens with the root file renamed aside: {}",
        report_line(&answer)
    );
    assert!(
        aside.iter().all(|&b| b == 0),
        "B1: the root file renamed aside inside the held roots directory keeps its bytes: {}",
        report_line(&answer)
    );
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(delete_report(&answer).root_destroyed());
}

/// ST-46 K1 with store-3b's identity: the root file renamed aside after the
/// listing with a link to it put at its name. The link is removed as the
/// entry that it is and nothing is written through it; the renamed file, of
/// the identity that the store read, is destroyed where it lies.
#[test]
fn a_root_file_renamed_aside_after_the_listing_with_a_link_at_its_name_is_destroyed_where_it_lies()
{
    let (answer, _, aside, copy_opens) =
        delete_all_with_a_move_after_the_listing(AfterTheListing::RenameAndLinkAtTheName);
    assert!(!copy_opens, "{}", report_line(&answer));
    assert!(
        aside.iter().all(|&b| b == 0),
        "the renamed root file keeps its bytes: {}",
        report_line(&answer)
    );
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(delete_report(&answer).root_destroyed());
}

/// ST-46 K1 with store-3b's identity: the root file renamed aside after the
/// listing with an empty file put at its name. The empty file is no root
/// file of the store's identity; the renamed file is destroyed where it lies.
#[test]
fn a_root_file_renamed_aside_after_the_listing_with_an_empty_file_at_its_name_is_destroyed_where_it_lies(
) {
    let (answer, _, aside, copy_opens) =
        delete_all_with_a_move_after_the_listing(AfterTheListing::RenameAndEmptyFileAtTheName);
    assert!(!copy_opens, "{}", report_line(&answer));
    assert!(aside.iter().all(|&b| b == 0), "{}", report_line(&answer));
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(delete_report(&answer).root_destroyed());
}

/// ST-46 K1: a partial name of the named root that holds the root's bytes,
/// renamed aside after the listing. An absent entry of the named id at any of
/// its names reads the root left.
#[test]
fn a_partial_root_file_renamed_aside_after_the_listing_reads_the_root_left() {
    let (answer, root_id, aside, _) =
        delete_all_with_a_move_after_the_listing(AfterTheListing::RenamePartialAside);
    assert_eq!(
        delete_report(&answer).root,
        crate::store::RootReading::Left(Some(root_id)),
        "{}",
        report_line(&answer)
    );
    assert!(answer.is_err() && aside.iter().any(|&b| b != 0));
}

/// ST-46 K1, the control: nothing moves after the listing, and the delete
/// answers Ok with the root destroyed.
#[test]
fn with_nothing_moved_after_the_listing_the_root_reads_destroyed() {
    let (answer, _, _, _) = delete_all_with_a_move_after_the_listing(AfterTheListing::Nothing);
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(delete_report(&answer).root_destroyed());
}

/// ST-46 K2 with store-3b's closed layout: the tail opens the roots
/// directory from the data directory's handle by its fixed name and follows
/// no link there, so a link at the roots name to another store's roots is
/// not swept, also beside a link at the store name; the roots read not
/// listed. The control: the data directory's own roots are swept.
#[test]
fn the_tail_follows_no_link_at_the_roots_name_of_its_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let behind = dir.path().join("behind");
    let behind_id = RootId::fresh().unwrap();
    let mut other = EncryptedStore::create(
        &behind,
        Box::new(DevFallbackCustody::new(behind_id, behind.join("roots"))),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (segment, _) = other
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(other);
    for at_the_store_name in ["nothing", "a link to the other store"] {
        if at_the_store_name != "nothing" {
            std::os::unix::fs::symlink(&behind, data.join("store")).unwrap();
        }
        std::os::unix::fs::symlink(behind.join("roots"), data.join("roots")).unwrap_or(());
        let answer = tail(&data.join("store"), &data.join("roots"));
        let reads = public_reads(&behind, &behind.join("roots"), segment.segment_id);
        println!(
            "ST46 K2 a link at the roots name, {at_the_store_name} at the store name: {} \
             the_other_store: {reads}",
            report_line(&answer)
        );
        assert!(
            reads.starts_with("open=Ok segment_plaintext_equal=true"),
            "the tail swept another store's roots through a link at its roots name: {reads}"
        );
        assert!(
            answer.is_err() && !delete_report(&answer).roots.listed,
            "{}",
            report_line(&answer)
        );
    }

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    put_a_root(&roots_dir);
    zero_the_key_file(&store_dir);
    let control = tail(&store_dir, &roots_dir);
    println!(
        "ST46 K2 the data directory's own roots {}",
        report_line(&control)
    );
    assert!(control.is_ok(), "{}", report_line(&control));
    assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());

    let dir = tempfile::tempdir().unwrap();
    let behind = dir.path().join("behind");
    let behind_id = RootId::fresh().unwrap();
    let mut other = EncryptedStore::create(
        &behind,
        Box::new(DevFallbackCustody::new(behind_id, behind.join("roots"))),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (segment, _) = other
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(other);
    let (store_dir, roots_dir, _, _) = file_root_store(&dir.path().join("data"));
    std::fs::rename(&roots_dir, dir.path().join("data").join("roots-aside")).unwrap();
    std::os::unix::fs::symlink(behind.join("roots"), &roots_dir).unwrap();
    let answer = tail(&store_dir, &roots_dir);
    let reads = public_reads(&behind, &behind.join("roots"), segment.segment_id);
    println!(
        "ST46 K2 a link at the roots name beside a store of its own: {} the_other_store: {reads}",
        report_line(&answer)
    );
    assert!(
        reads.starts_with("open=Ok segment_plaintext_equal=true"),
        "the tail's root step swept another store's roots through a link at its roots name: {reads}"
    );
    assert!(
        answer.is_err() && !delete_report(&answer).roots.listed,
        "{}",
        report_line(&answer)
    );
}

/// The adversary's p3_c (ruling ST-46 K3, the verifier's copy L44): the tail
/// on a store that a live handle of this process holds answers Locked and
/// changes nothing: the root file's bytes, store.key's bytes and the listing
/// of the roots directory are equal before and after. After the handle is
/// dropped an open answers Ok and reads a segment.
#[test]
fn p3_c_the_tail_on_a_store_that_a_handle_holds_answers_locked_and_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    put_a_root(&roots_dir);
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let before = (
        std::fs::read(&root_file).unwrap(),
        std::fs::read(KeyFile::path(&store_dir)).unwrap(),
        names(&roots_dir),
    );
    let answer = tail(&store_dir, &roots_dir);
    let after = (
        std::fs::read(&root_file).unwrap_or_default(),
        std::fs::read(KeyFile::path(&store_dir)).unwrap_or_default(),
        names(&roots_dir),
    );
    drop(store);
    let reads = copy_reads(&store_dir, &roots_dir, root_id, segment);
    println!(
        "ST46 K3 tail={} unchanged={} after_the_handle: {reads}",
        answer_text(&answer),
        before == after
    );
    assert_eq!(
        answer_text(&answer),
        "Locked",
        "the tail went on beside a live handle"
    );
    assert!(
        before == after,
        "the tail beside a live handle changed the root, store.key or the roots"
    );
    assert!(
        reads.starts_with("open=Ok segment_plaintext_equal=true"),
        "{reads}"
    );
}

/// The review's keyro case (ruling ST-46 K5 N1): a store.key of mode 0400
/// with a second name outside the store. The key-file step sets the mode to
/// 0600 by its name before its zero-fill, so the zero-fill reaches every name
/// of the file and the second name ends all zero.
#[test]
fn delete_all_zeroes_a_store_key_of_mode_0400_through_every_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let key_path = KeyFile::path(&store_dir);
    let second_name = dir.path().join("a-second-name-of-store-key");
    std::fs::hard_link(&key_path, &second_name).unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    set_mode(&key_path, 0o400);
    let answer = store.delete_all();
    drop(store);
    let after = std::fs::read(&second_name).unwrap();
    println!(
        "ST46 N1 {} second_name_len={} second_name_nonzero_bytes={}",
        report_line(&answer),
        after.len(),
        after.iter().filter(|&&b| b != 0).count()
    );
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(
        !after.is_empty() && after.iter().all(|&b| b == 0),
        "a second name of a store.key of mode 0400 keeps the wrapped key"
    );
}

/// The verifier's copy L48 (ruling ST-46 K4 (g)): a delete beside a
/// create.pending that cannot be removed (a directory that holds a file)
/// counts it in files_left and answers DeleteIncomplete.
#[test]
fn a_create_pending_that_cannot_be_removed_is_counted_by_the_delete() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    std::fs::create_dir(store_dir.join("create.pending")).unwrap();
    std::fs::write(store_dir.join("create.pending").join("theirs"), b"kept").unwrap();
    let answer = store.delete_all();
    drop(store);
    println!("ST46 L48 {}", report_line(&answer));
    assert!(
        answer.is_err() && delete_report(&answer).files_left >= 1,
        "a create.pending that stays is not counted: {}",
        report_line(&answer)
    );
}

/// Below this median, in milliseconds, the time to a destroyed root at 2,000
/// segments passes whatever its ratio to 250 segments (ruling ST-46 K5 N5).
const N5_FLOOR_MS: f64 = 50.0;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// A store of `segments` small segments, opened, with the wrapped key of
/// store.key: the directory, the handle, the roots, the root id and the key.
fn store_of_segments(
    segments: usize,
) -> (
    tempfile::TempDir,
    EncryptedStore,
    PathBuf,
    RootId,
    WrappedKey,
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let mut store =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
    for i in 0..segments {
        store
            .write_segment(&mut Cursor::new((i as u64).to_le_bytes()))
            .unwrap();
    }
    let wrapped = wrapped(&store_dir);
    (dir, store, roots_dir, root_id, wrapped)
}

/// Milliseconds from the delete-all request to the end of its root step, after
/// which the old wrapped key no longer unwraps (checked), and the whole
/// delete-all's milliseconds.
fn delete_times(segments: usize) -> (f64, f64) {
    let (_dir, mut store, roots_dir, root_id, wrapped) = store_of_segments(segments);
    let stamp = Arc::new(std::sync::Mutex::new(None));
    let seen = Arc::clone(&stamp);
    let _seam =
        crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT_STEP, move || {
            *seen.lock().unwrap() = Some(Instant::now());
        });
    let started = Instant::now();
    let answer = store.delete_all();
    let whole = started.elapsed();
    let to_the_root = stamp.lock().unwrap().expect("the root step's seam") - started;
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(
        unwraps(&roots_dir, root_id, &wrapped) != "Ok",
        "the old wrapped key still unwraps after the root step"
    );
    (
        to_the_root.as_secs_f64() * 1000.0,
        whole.as_secs_f64() * 1000.0,
    )
}

/// ST-46 K5 N5, port (b) of X-store-01 and the Bounds figures: the time from
/// the request to "the old wrapped key no longer unwraps" differs by less
/// than a factor of 2 between 250 and 2,000 segments, on the medians of 5
/// runs at each size, or the larger median is under N5_FLOOR_MS. It prints
/// the delete times of 250, 500, 1,000 and 2,000 segments, median of 3.
#[test]
#[ignore = "no time on this Mac is a measurement (ruling ST-46 K5 N5): scripts/r1/ci.d/31-store-full-volume.sh runs it in CI on the macos job"]
fn the_time_to_a_destroyed_root_differs_by_less_than_twice_between_250_and_2000_segments() {
    let at = |segments: usize| median((0..5).map(|_| delete_times(segments).0).collect());
    let (small, large) = (at(250), at(2_000));
    println!();
    println!("N5 to_the_root_ms_median_of_5 segments=250 ms={small:.2}");
    println!("N5 to_the_root_ms_median_of_5 segments=2000 ms={large:.2}");
    println!(
        "N5 ratio_2000_to_250={:.2} floor_ms={N5_FLOOR_MS}",
        large / small
    );
    for segments in [250, 500, 1_000, 2_000] {
        let whole = median((0..3).map(|_| delete_times(segments).1).collect());
        println!("N5 delete_all_ms median_of_3: segments={segments} ms={whole:.2}");
    }
    assert!(
        large < 2.0 * small || large < N5_FLOOR_MS,
        "the time to a destroyed root grows with the store: 250={small:.2} ms, 2000={large:.2} ms"
    );
}

/// The verifier's point 1 (ruling ST-46 Addition 1 S3, copy L47): a create in
/// a directory that holds a store whose store.key cannot be read for a
/// moment, beside a pending mark that reads made and names that store's root.
/// The create answers AlreadyExists, destroys no root file and leaves the
/// mark, and the store opens once its key file can be read again.
#[test]
fn a_create_beside_a_made_mark_and_an_unreadable_key_file_keeps_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let before = root_files_in(&roots_dir);
    write_mark(&store_dir, root_id, "made");
    let key = KeyFile::path(&store_dir);
    set_mode(&key, 0o000);
    let answer = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            RootId::fresh().unwrap(),
        )),
        DEFAULT_RETENTION,
    );
    set_mode(&key, 0o600);
    let created = answer_text(&answer);
    drop(answer);
    let after = root_files_in(&roots_dir);
    let mark_stays = store_dir.join("create.pending").exists();
    let opens = open_over_file_root(&store_dir, &roots_dir, root_id).is_ok();
    println!(
        "ST46A1 L47 create={created} root_files_unchanged={} mark_stays={mark_stays} the_store_opens_after={opens}",
        after == before
    );
    assert_eq!(created, "AlreadyExists");
    assert_eq!(
        after, before,
        "a create beside an unreadable key file destroyed the root that the made mark names"
    );
    assert!(
        mark_stays,
        "the create removed the mark beside a key file that it cannot read"
    );
    assert!(
        opens,
        "the store no longer opens once its key file can be read again"
    );
}
