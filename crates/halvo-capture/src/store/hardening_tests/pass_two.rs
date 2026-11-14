//! The tests of ruling ST-45: the adversary's cases of its pass 2 at
//! 79f75b1b (prefix `p2_`), the tests that the ruling names for H1 to H5, and
//! the table test of the tail, one row for each arm of `delete_store_dir`.
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

// ---------------------------------------------------------------------------
// H3: a later call on a handle whose root reads destroyed
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// H5: an entry at store.key.tmp in the delete
// ---------------------------------------------------------------------------

/// The adversary's p2_d: the tail and a second name at store.key.tmp of a
/// file of another party.
#[test]
fn p2_d_the_tail_writes_nothing_through_a_second_name_at_store_key_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    let theirs = dir.path().join("a-file-of-another-party");
    std::fs::write(&theirs, vec![7u8; 300]).unwrap();
    std::fs::hard_link(&theirs, store_dir.join("store.key.tmp")).unwrap();
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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
/// ends all zero.
#[test]
fn the_store_key_file_under_a_second_name_at_store_key_tmp_ends_all_zero_under_every_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    let third = dir.path().join("a-third-name-of-the-key-file");
    std::fs::hard_link(KeyFile::path(&store_dir), &third).unwrap();
    std::fs::hard_link(KeyFile::path(&store_dir), store_dir.join("store.key.tmp")).unwrap();
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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
    let first = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let second = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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

// ---------------------------------------------------------------------------
// H1 with H2: the tail's sweep, the root file that cannot be opened
// ---------------------------------------------------------------------------

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
        let first = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
        let second = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
        let third = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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
    let tail = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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
    let first = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let second = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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
    let first = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let second = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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

// ---------------------------------------------------------------------------
// H4: what counts as the root's own file at its name
// ---------------------------------------------------------------------------

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
    std::fs::rename(&aside, &root_file).unwrap();
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
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    println!("ST45 H4 all zero len={len} {}", report_line(&answer));
    assert!(answer.is_ok(), "{}", report_line(&answer));
    assert!(
        delete_report(&answer).root_destroyed(),
        "{}",
        report_line(&answer)
    );
    assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// H1: the table test of the tail
// ---------------------------------------------------------------------------

/// The arm of `delete_store_dir` that a row reaches.
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
use RootsAfter::{Both, Empty, Nothing, TheOtherOne};
use RootsAt::*;

/// One row for each arm of `delete_store_dir` by named or unnamed and by
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
    let answer = EncryptedStore::delete_store_dir(&store_at, &roots_dir);
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
            (true, false) => Both,
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
    let answer =
        EncryptedStore::delete_store_dir(&data.join("store"), &data.join("store").join("roots"));
    println!("ST45 H1 roots through a link {}", report_line(&answer));
    assert_eq!(root_files_in(&roots_behind), roots_before);
    assert!(std::fs::symlink_metadata(data.join("store")).is_err());
    assert!(answer.is_ok(), "{}", report_line(&answer));
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
    let first = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let second = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
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
/// root, which reads destroyed.
#[test]
fn the_tail_sweeps_the_root_of_an_owed_mark_and_reads_the_root_of_a_made_mark_destroyed() {
    for (reading, expected) in [("owed", "unknown"), ("made", "yes")] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
        std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
        let marked = put_a_root(&roots_dir);
        write_mark(&store_dir, marked, reading);
        let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
        println!("ST45 H1 mark={reading} {}", report_line(&answer));
        assert!(answer.is_ok(), "{reading}: {}", report_line(&answer));
        assert_eq!(delete_report(&answer).root.as_text(), expected);
        assert_eq!(root_files_in(&roots_dir), Vec::<String>::new());
    }
}
