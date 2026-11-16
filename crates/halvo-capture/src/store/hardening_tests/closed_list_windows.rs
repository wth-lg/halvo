//! The Windows tests of ruling ST-44: the nine of the verify step's Windows
//! list, its test 4 with one more case where such a name is the roots path,
//! and the Locked answer of ruling ST-41 (3). Every store and every root lives
//! in a directory that the test made. They print answers, counts and raw
//! error numbers, never a key, a root or a root id.

use std::os::windows::fs::OpenOptionsExt;

use super::*;
use crate::custody::KeyLabel;

const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_SHARE_READ: u32 = 0x1;
const ERROR_SHARING_VIOLATION: i32 = 32;

fn hold_file(path: &Path, share: u32) -> std::fs::File {
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(share)
        .open(path)
        .unwrap()
}

fn hold_dir(path: &Path, share: u32) -> std::fs::File {
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(share)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .unwrap()
}

fn report_line(answer: &StoreResult<crate::store::DeleteReport>) -> String {
    match answer {
        Ok(_) | Err(StoreError::DeleteIncomplete(_)) => {
            let report = delete_report(answer);
            format!(
                "{} root_destroyed={} roots_left={} listed={} key_file_gone={} database_left={} files_left={} not_followed={}",
                if answer.is_ok() { "Ok" } else { "DeleteIncomplete" },
                report.root_destroyed(),
                report.roots.left.len(),
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

fn unwraps(roots_dir: &Path, root_id: RootId, wrapped: &WrappedKey) -> bool {
    file_root_custody(roots_dir, TestProtector::new(0x5a).0, root_id)
        .unwrap_store_key(wrapped, KeyLabel::STORE)
        .is_ok()
}

fn wrapped_of(store_dir: &Path) -> WrappedKey {
    KeyFile::read(store_dir)
        .unwrap()
        .wrapped_store_key()
        .unwrap()
}

/// Point 1: the roots directory renamed while the store is open.
#[test]
fn delete_all_names_the_root_as_left_when_the_roots_directory_was_renamed_while_the_store_is_open()
{
    for tail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        let wrapped = wrapped_of(&store_dir);
        let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
        let moved = dir.path().join("roots-moved");
        let renamed = std::fs::rename(&roots_dir, &moved);
        let answer = if tail {
            drop(store);
            super::tail(&store_dir, &roots_dir)
        } else {
            let mut store = store;
            store.delete_all()
        };
        let alive = renamed.is_ok() && unwraps(&moved, root_id, &wrapped);
        println!(
            "WIN-ROOTS-RENAMED tail={tail} renamed={} {} moved_root_unwraps={alive}",
            renamed.is_ok(),
            report_line(&answer)
        );
        assert!(
            !(alive && answer.is_ok()),
            "tail={tail}: the delete answered Ok while the root in the renamed roots directory unwraps"
        );
        assert!(
            !(alive && delete_report(&answer).root_destroyed()),
            "tail={tail}: the report reads the root as destroyed while it unwraps"
        );
    }
}

/// Point 2: the tail on a store directory that another handle holds without
/// sharing (Locked, nothing done), and on one whose writer.lock is a directory
/// (the root step runs without the lock and the answer names what stays).
#[test]
fn the_tail_destroys_the_root_or_reports_it_when_the_store_directory_or_its_lock_file_cannot_be_opened(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = wrapped_of(&store_dir);
    let held = hold_dir(&store_dir, 0);
    let answer = tail(&store_dir, &roots_dir);
    drop(held);
    println!(
        "WIN-TAIL-HELD-DIR {} key_file_left={} root_unwraps={}",
        report_line(&answer),
        store_dir.join("store.key").exists(),
        unwraps(&roots_dir, root_id, &wrapped)
    );
    assert!(
        matches!(answer, Err(StoreError::Locked)),
        "a store directory held without sharing must answer Locked: {}",
        report_line(&answer)
    );
    assert!(
        store_dir.join("store.key").exists(),
        "Locked removed the key file"
    );

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = wrapped_of(&store_dir);
    let _ = std::fs::remove_file(store_dir.join("writer.lock"));
    std::fs::create_dir(store_dir.join("writer.lock")).unwrap();
    let answer = tail(&store_dir, &roots_dir);
    let alive = unwraps(&roots_dir, root_id, &wrapped);
    println!(
        "WIN-TAIL-LOCK-DIR {} root_unwraps={alive}",
        report_line(&answer)
    );
    assert!(
        !alive || matches!(&answer, Err(StoreError::DeleteIncomplete(r)) if !r.root_destroyed()),
        "the tail with a directory at writer.lock answered {} and the root still unwraps",
        report_line(&answer)
    );
}

/// Point 3, a reading: a create in a directory whose name is not Unicode.
#[test]
fn a_create_in_a_directory_whose_name_is_not_unicode_leaves_only_the_lock_file_and_makes_no_root() {
    use std::os::windows::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let mut wide: Vec<u16> = "not-unicode-".encode_utf16().collect();
    wide.push(0xD800);
    let odd = dir.path().join(std::ffi::OsString::from_wide(&wide));
    let roots = dir.path().join("roots");
    let custody = file_root_custody(&roots, TestProtector::new(0x5a).0, RootId::fresh().unwrap());
    let created = answer_text(&EncryptedStore::create(
        &odd,
        Box::new(custody),
        DEFAULT_RETENTION,
    ));
    let left: Vec<String> = std::fs::read_dir(&odd)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let opened = answer_text(&EncryptedStore::open(&odd, &roots, DEFAULT_RETENTION));
    println!(
        "WIN-NON-UNICODE create={created} open={opened} store_dir_now={left:?} roots_path_exists={} \
         steps={:?}",
        roots.exists(),
        crate::store::db::database_open_steps(&odd)
    );
    assert_eq!(left, vec!["writer.lock".to_string()]);
    assert!(
        !roots.exists(),
        "the create in a directory whose name is not Unicode made a roots directory"
    );
}

/// Test 4 of the verify step: the tail given a verbatim name that ends in a
/// dot, with a file, nothing or an empty directory at the trimmed name, and
/// one case with such a name as the roots path.
#[test]
fn the_tail_given_a_verbatim_name_that_ends_in_a_dot_removes_no_sibling_and_reports_what_stays() {
    for case in [
        "a file at the trimmed name",
        "nothing at the trimmed name",
        "a directory at the trimmed name",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        let verbatim = PathBuf::from(format!(r"\\?\{}\store.", base.display()));
        std::fs::create_dir(&verbatim).unwrap();
        std::fs::write(verbatim.join("held-file"), b"inside").unwrap();
        let sibling = base.join("store");
        match case {
            "a file at the trimmed name" => std::fs::write(&sibling, b"sibling").unwrap(),
            "a directory at the trimmed name" => std::fs::create_dir(&sibling).unwrap(),
            _ => {}
        }
        let answer = tail(&verbatim, &base.join("roots"));
        let sibling_now = if sibling.is_dir() {
            Some(b"<dir>".to_vec())
        } else {
            std::fs::read(&sibling).ok()
        };
        let inside_stays = verbatim.join("held-file").exists();
        println!(
            "WIN-TRAILING-DOT case={case} {} sibling_now={:?} store_dot_holds_its_file={inside_stays}",
            report_line(&answer),
            sibling_now.as_ref().map(|b| b.len())
        );
        match case {
            "a file at the trimmed name" => {
                assert_eq!(sibling_now.as_deref(), Some(&b"sibling"[..]))
            }
            "a directory at the trimmed name" => {
                assert!(sibling.is_dir(), "the sibling directory went")
            }
            _ => {}
        }
        assert!(
            !(answer.is_ok() && inside_stays),
            "{case}: the tail answered Ok while store. holds a file"
        );
        assert!(inside_stays, "{case}: the tail removed a file in store.");
        if case == "nothing at the trimmed name" {
            assert!(
                !sibling.exists(),
                "{case}: an entry came at the trimmed name"
            );
        }
        let report = delete_report(&answer);
        assert!(
            matches!(answer, Err(StoreError::DeleteIncomplete(_))) && report.not_followed == 1,
            "{case}: the tail did not answer DeleteIncomplete with not_followed 1"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let (made_at, roots_dir, root_id, _) = file_root_store(&base);
    let wrapped = wrapped_of(&made_at);
    let verbatim = PathBuf::from(format!(r"\\?\{}\store.", base.display()));
    std::fs::rename(&made_at, &verbatim).unwrap();
    let answer = tail(&verbatim, &roots_dir);
    println!(
        "WIN-TRAILING-DOT case=a store under such a name {} store_dot_holds_its_key_file={} trimmed_name_exists={}",
        report_line(&answer),
        verbatim.join("store.key").exists(),
        base.join("store").exists()
    );
    let report = delete_report(&answer);
    assert!(
        report.root_destroyed() && !unwraps(&roots_dir, root_id, &wrapped),
        "the tail at a name that ends in a dot did not run the root step"
    );
    assert!(
        verbatim.join("store.key").exists() && !base.join("store").exists(),
        "the tail removed a file at or beside the name that ends in a dot"
    );
    assert!(
        matches!(answer, Err(StoreError::DeleteIncomplete(_))) && report.not_followed == 1,
        "the tail at a store under such a name did not answer DeleteIncomplete with not_followed 1"
    );
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let (store_dir, _, _, _) = file_root_store(&base);
    let roots_dot = PathBuf::from(format!(r"\\?\{}\roots.", base.display()));
    std::fs::create_dir(&roots_dot).unwrap();
    std::fs::write(roots_dot.join("held-file"), b"inside").unwrap();
    let answer = tail(&store_dir, &roots_dot);
    println!(
        "WIN-TRAILING-DOT case=such a name as the roots path {} roots_dot_holds_its_file={} sibling_roots_exists={}",
        report_line(&answer),
        roots_dot.join("held-file").exists(),
        base.join("roots").exists()
    );
    assert!(
        roots_dot.join("held-file").exists(),
        "the tail removed a file under the roots path that ends in a dot"
    );
    assert!(
        matches!(&answer, Err(StoreError::DeleteIncomplete(r)) if !r.root_destroyed()),
        "the tail read the root as destroyed under a roots path that ends in a dot"
    );
}

/// Point 5: a read-only root file with a second name.
#[test]
fn delete_all_zeroes_a_read_only_root_file_under_a_second_name_or_names_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let second = dir.path().join("second-name");
    std::fs::hard_link(&root_file, &second).unwrap();
    let mut permissions = std::fs::metadata(&root_file).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&root_file, permissions).unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let answer = store.delete_all();
    drop(store);
    let bytes = std::fs::read(&second).unwrap();
    let all_zero = bytes.iter().all(|&b| b == 0);
    println!(
        "WIN-READ-ONLY-SECOND-NAME {} second_name_all_zero={all_zero} root_name_stays={}",
        report_line(&answer),
        root_file.exists()
    );
    assert!(
        all_zero || answer.is_err(),
        "delete-all answered Ok and the second name keeps the root"
    );
}

/// Point 6: a shard directory that another handle keeps from removal.
#[test]
fn delete_all_counts_a_shard_directory_that_another_handle_keeps_from_removal() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    let shard = segment_path(&store_dir, segment)
        .parent()
        .unwrap()
        .to_path_buf();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let held = hold_dir(&shard, FILE_SHARE_READ);
    let answer = store.delete_all();
    drop(held);
    drop(store);
    let report = delete_report(&answer);
    println!(
        "WIN-HELD-SHARD {} shard_stays={}",
        report_line(&answer),
        shard.exists()
    );
    assert!(
        !shard.exists() || (report.files_left >= 1 && answer.is_err()),
        "delete-all answered {} while the shard directory stays",
        report_line(&answer)
    );
}

/// Point 7: the tail on a root file whose name was put in upper case.
#[test]
fn the_tail_destroys_a_root_file_whose_name_was_renamed_to_upper_case() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = wrapped_of(&store_dir);
    let lower = roots_dir.join(format!("{root_id}.root"));
    let upper = roots_dir.join(format!("{root_id}.root").to_uppercase());
    std::fs::rename(&lower, &upper).unwrap();
    let answer = tail(&store_dir, &roots_dir);
    let alive = unwraps(&roots_dir, root_id, &wrapped);
    println!(
        "WIN-UPPER-CASE {} root_unwraps={alive}",
        report_line(&answer)
    );
    assert!(
        !(answer.is_ok() && alive),
        "the tail answered Ok and the root still unwraps"
    );
}

/// Point 8: a partial root file and store.key.tmp that a foreign handle keeps.
#[test]
fn delete_all_reports_a_partial_root_file_and_a_key_temp_file_that_a_foreign_handle_keeps() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_bytes = std::fs::read(roots_dir.join(format!("{root_id}.root"))).unwrap();
    let partial = roots_dir.join(format!("{root_id}.0123456789abcdef.root.tmp"));
    std::fs::write(&partial, &root_bytes).unwrap();
    let temp = store_dir.join("store.key.tmp");
    std::fs::copy(store_dir.join("store.key"), &temp).unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let held_partial = hold_file(&partial, FILE_SHARE_READ);
    let held_temp = hold_file(&temp, FILE_SHARE_READ);
    let answer = store.delete_all();
    drop(held_partial);
    drop(held_temp);
    drop(store);
    println!(
        "WIN-HELD-PARTIAL-AND-TEMP {} partial_stays={} temp_stays={}",
        report_line(&answer),
        partial.exists(),
        temp.exists()
    );
    assert!(
        !(answer.is_ok() && (partial.exists() || temp.exists())),
        "delete-all answered Ok while a partial root file or store.key.tmp stays"
    );
}

/// Point 9: a create that loses the key file's name to a hard link.
#[test]
fn a_create_that_loses_the_key_file_name_leaves_the_entry_that_took_it() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let outside = dir.path().join("outside-file");
    let original = b"bytes that are not the store's".to_vec();
    std::fs::write(&outside, &original).unwrap();
    let act = {
        let (key, outside) = (store_dir.join("store.key"), outside.clone());
        move || std::fs::hard_link(&outside, &key).unwrap()
    };
    let seam = crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT, nth_call(1, act));
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        RootId::fresh().unwrap(),
    );
    let created = answer_text(&EncryptedStore::create(
        &store_dir,
        Box::new(custody),
        DEFAULT_RETENTION,
    ));
    drop(seam);
    let after = std::fs::read(&outside).unwrap();
    println!(
        "WIN-KEY-NAME-LOST answer={created} outside_unchanged={} store_key_stays={}",
        after == original,
        store_dir.join("store.key").exists()
    );
    assert_eq!(created, "AlreadyExists");
    assert_eq!(
        after, original,
        "the create wrote over a file that it did not make"
    );
    assert!(
        store_dir.join("store.key").exists(),
        "the create removed the entry that took the name"
    );
}

/// Ruling ST-41 (3): a contender's open while another handle holds
/// writer.lock without sharing answers Locked.
#[test]
fn an_open_while_another_handle_holds_the_lock_file_without_sharing_answers_locked() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let held = hold_file(&store_dir.join("writer.lock"), 0);
    let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
    let raw = std::fs::OpenOptions::new()
        .read(true)
        .open(store_dir.join("writer.lock"))
        .err()
        .and_then(|e| e.raw_os_error());
    drop(held);
    println!("WIN-LOCK-SHARING open={opened} raw_error_of_a_second_open={raw:?}");
    assert_eq!(raw, Some(ERROR_SHARING_VIOLATION));
    assert_eq!(opened, "Locked");
}

/// Ruling ST-45 H6 (a): verbatim_form builds its form from the path's parts.
/// A real directory named `store.` holds a store, and the tail is given its
/// path written with forward slashes, with a dot part and with a doubled
/// separator: the root step runs (the root no longer unwraps), the other root
/// stays (H1: the named root alone in this arm), and `store.` keeps its files.
#[test]
fn the_tail_given_a_dotted_store_path_in_another_spelling_destroys_the_named_root_alone() {
    let mut problems = Vec::new();
    for spelling in ["forward slashes", "a dot part", "a doubled separator"] {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        let (made_at, roots_dir, root_id, _) = file_root_store(&base);
        let wrapped = wrapped_of(&made_at);
        let other = RootId::fresh().unwrap();
        file_root_custody(&roots_dir, TestProtector::new(0x5a).0, other)
            .create_root()
            .unwrap();
        let verbatim = PathBuf::from(format!(r"\\?\{}\store.", base.display()));
        std::fs::rename(&made_at, &verbatim).unwrap();
        let text = base.display().to_string();
        let given = PathBuf::from(match spelling {
            "forward slashes" => format!("{}/store.", text.replace('\\', "/")),
            "a dot part" => format!(r"{text}\.\store."),
            _ => format!(r"{text}\\store."),
        });
        let answer = tail(&given, &roots_dir);
        let destroyed = !unwraps(&roots_dir, root_id, &wrapped);
        let other_stays = roots_dir.join(format!("{other}.root")).exists();
        let key_file_stays = verbatim.join("store.key").exists();
        println!(
            "WIN-ST45-H6A spelling={spelling} {} root_no_longer_unwraps={destroyed} other_root_stays={other_stays} store_dot_keeps_its_key_file={key_file_stays} trimmed_name_exists={}",
            report_line(&answer),
            base.join("store").exists()
        );
        let incomplete =
            matches!(&answer, Err(StoreError::DeleteIncomplete(r)) if r.not_followed == 1);
        if !(destroyed && other_stays && key_file_stays && incomplete) {
            problems.push(spelling);
        }
    }
    assert!(problems.is_empty(), "spellings that fail: {problems:?}");
}

/// Ruling ST-45 H1 on Windows: in the two arms without the lock that run a
/// root step (a path part that Win32 trims, and a lock that fails for a
/// reason other than a holder: here a directory at writer.lock), the tail
/// destroys the named root alone, with the witness, and another root stays.
#[test]
fn the_tail_without_the_lock_destroys_the_named_root_alone() {
    let mut problems = Vec::new();
    for arm in ["a dotted name", "a directory at writer.lock"] {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        let (made_at, roots_dir, root_id, _) = file_root_store(&base);
        let wrapped = wrapped_of(&made_at);
        let other = RootId::fresh().unwrap();
        file_root_custody(&roots_dir, TestProtector::new(0x5a).0, other)
            .create_root()
            .unwrap();
        let store_at = if arm == "a dotted name" {
            let verbatim = PathBuf::from(format!(r"\\?\{}\store.", base.display()));
            std::fs::rename(&made_at, &verbatim).unwrap();
            verbatim
        } else {
            std::fs::remove_file(made_at.join("writer.lock")).unwrap();
            std::fs::create_dir(made_at.join("writer.lock")).unwrap();
            made_at.clone()
        };
        let answer = tail(&store_at, &roots_dir);
        let destroyed = !unwraps(&roots_dir, root_id, &wrapped);
        let other_stays = roots_dir.join(format!("{other}.root")).exists();
        println!(
            "WIN-ST45-H1 arm={arm} {} root_no_longer_unwraps={destroyed} other_root_stays={other_stays}",
            report_line(&answer)
        );
        if !(destroyed && other_stays && answer.is_err()) {
            problems.push(arm);
        }
    }
    assert!(problems.is_empty(), "arms that fail: {problems:?}");
}

/// Ruling ST-45 H6 (b): a create and then an open at a store path or with a
/// roots path of which a part ends in a dot or a space, not verbatim and
/// verbatim. It prints what stands at the trimmed name and fails when a call
/// acted on the trimmed sibling, when one made anything at the name given,
/// or when one answered other than Io(InvalidInput). The first run of this
/// test, at the commit with the tests and no change, found that every create
/// made its directory at the trimmed sibling.
#[test]
fn a_create_and_an_open_at_a_dotted_store_or_roots_path_make_nothing_at_the_trimmed_name() {
    let mut acted = Vec::new();
    for (case, store_name, roots_name, verbatim) in [
        ("a store path that ends in a dot", "store.", "roots", false),
        (
            "a store path that ends in a space",
            "store ",
            "roots",
            false,
        ),
        ("a roots path that ends in a dot", "store", "roots.", false),
        (
            "a roots path that ends in a space",
            "store",
            "roots ",
            false,
        ),
        (
            "a verbatim store path that ends in a dot",
            "store.",
            "roots",
            true,
        ),
        (
            "a verbatim roots path that ends in a dot",
            "store",
            "roots.",
            true,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        let text = base.display().to_string();
        let lead = if verbatim { r"\\?\" } else { "" };
        let store_dir = PathBuf::from(format!(r"{lead}{text}\{store_name}"));
        let roots_dir = PathBuf::from(format!(r"{lead}{text}\{roots_name}"));
        let root_id = RootId::fresh().unwrap();
        let created = answer_text(&EncryptedStore::create(
            &store_dir,
            Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )),
            DEFAULT_RETENTION,
        ));
        let opened = answer_text(&EncryptedStore::open(
            &store_dir,
            &roots_dir,
            DEFAULT_RETENTION,
        ));
        let trimmed_store = base.join(store_name.trim_end_matches(['.', ' ']));
        let trimmed_roots = base.join(roots_name.trim_end_matches(['.', ' ']));
        let verbatim_store = PathBuf::from(format!(r"\\?\{text}\{store_name}")).exists();
        let verbatim_roots = PathBuf::from(format!(r"\\?\{text}\{roots_name}")).exists();
        let store_trims = store_name != store_name.trim_end_matches(['.', ' ']);
        let roots_trims = roots_name != roots_name.trim_end_matches(['.', ' ']);
        let at_trimmed_store = store_trims && trimmed_store.exists();
        let at_trimmed_roots = roots_trims && trimmed_roots.exists();
        println!(
            "WIN-ST45-H6B case={case} create={created} open={opened} trimmed_store_name_exists={at_trimmed_store} trimmed_roots_name_exists={at_trimmed_roots} verbatim_store_name_exists={verbatim_store} verbatim_roots_name_exists={verbatim_roots}"
        );
        let refused = "Io(InvalidInput)";
        if at_trimmed_store
            || at_trimmed_roots
            || (store_trims && verbatim_store)
            || (roots_trims && verbatim_roots)
            || created != refused
            || opened != refused
        {
            acted.push(case);
        }
    }
    assert!(
        acted.is_empty(),
        "calls acted on the trimmed sibling, made something at the name given or answered other than Io(InvalidInput): {acted:?}"
    );
}

/// Ruling ST-46 K2 on Windows with store-3b's closed layout: a directory
/// link at the roots name of a data directory, to another store's roots. The
/// tail opens the roots from the data directory's handle and follows no
/// link there: it runs no sweep, and the other store's root unwraps after
/// it. When the runner cannot make a directory link the test prints so and
/// asserts nothing.
#[test]
fn the_tail_follows_no_directory_link_at_the_roots_name_of_its_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let (behind, behind_roots, behind_id, _) = file_root_store(&base.join("behind"));
    let wrapped = wrapped_of(&behind);
    let data = base.join("data");
    std::fs::create_dir(&data).unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(&data);
    std::fs::rename(&roots_dir, data.join("roots-aside")).unwrap();
    let made = std::os::windows::fs::symlink_dir(&behind_roots, &roots_dir);
    println!("WIN-ST46-K2 directory_link_made={}", made.is_ok());
    if made.is_err() {
        return;
    }
    let answer = tail(&store_dir, &roots_dir);
    let other_unwraps = unwraps(&behind_roots, behind_id, &wrapped);
    println!(
        "WIN-ST46-K2 {} the_other_root_unwraps={other_unwraps}",
        report_line(&answer)
    );
    assert!(
        other_unwraps,
        "the tail swept another store's roots through a directory link at its roots name"
    );
    assert!(
        answer.is_err() && !delete_report(&answer).roots.listed,
        "{}",
        report_line(&answer)
    );
}

/// store-3b, the share mode (ruling ST-44 F10): a store holds its roots
/// directory without delete sharing, so the directory cannot be renamed
/// while a handle of the store, from a create or from an open, is open;
/// after the handle is dropped the rename goes through.
#[test]
fn a_roots_directory_that_a_store_holds_cannot_be_renamed_until_the_store_is_dropped() {
    let mut problems = Vec::new();
    for handle in ["the create's", "the open's"] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
        let root_id = RootId::fresh().unwrap();
        let store = if handle == "the create's" {
            EncryptedStore::create(
                &store_dir,
                Box::new(file_root_custody(
                    &roots_dir,
                    TestProtector::new(0x5a).0,
                    root_id,
                )),
                DEFAULT_RETENTION,
            )
            .unwrap()
        } else {
            drop(
                EncryptedStore::create(
                    &store_dir,
                    Box::new(file_root_custody(
                        &roots_dir,
                        TestProtector::new(0x5a).0,
                        root_id,
                    )),
                    DEFAULT_RETENTION,
                )
                .unwrap(),
            );
            open_over_file_root(&store_dir, &roots_dir, root_id).unwrap()
        };
        let moved = dir.path().join("roots-moved");
        let while_held = std::fs::rename(&roots_dir, &moved);
        let code = while_held.as_ref().err().and_then(|e| e.raw_os_error());
        drop(store);
        let after = std::fs::rename(&roots_dir, &moved);
        println!(
            "WIN-ROOTS-SHARE handle={handle} renamed_while_held={} code={code:?} renamed_after_drop={}",
            while_held.is_ok(),
            after.is_ok()
        );
        if while_held.is_ok() || after.is_err() {
            problems.push(handle);
        }
    }
    assert!(
        problems.is_empty(),
        "the roots directory was renamed while the store held it, or not after: {problems:?}"
    );
}

/// store-3b (ruling ST-45 Addition 2 (c)): a real directory whose name ends
/// in a space, made through its verbatim name, with a store below it. The
/// create, the open and the tail refuse such a path, although Win32 would
/// open it as written, which fails closed; nothing under the directory is
/// written or destroyed, and nothing stands at the trimmed name.
#[test]
fn a_store_below_a_real_directory_whose_name_ends_in_a_space_is_refused_and_nothing_under_it_changes(
) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let spaced = base.join("data ");
    let verbatim = crate::store::durable::verbatim_form(&spaced)
        .expect("the verbatim form of an absolute path");
    std::fs::create_dir(&verbatim).unwrap();
    std::fs::write(verbatim.join("left-as-it-was"), b"bytes").unwrap();
    let (store_dir, roots_dir) = (spaced.join("store"), spaced.join("roots"));
    let created = answer_text(&EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            RootId::fresh().unwrap(),
        )),
        DEFAULT_RETENTION,
    ));
    let opened = answer_text(&EncryptedStore::open(
        &store_dir,
        &roots_dir,
        DEFAULT_RETENTION,
    ));
    let tail = EncryptedStore::delete_data_dir(&spaced, crate::store::Layout::DataDirectory);
    let listing: Vec<String> = std::fs::read_dir(&verbatim)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let kept = std::fs::read(verbatim.join("left-as-it-was"))
        .ok()
        .as_deref()
        == Some(&b"bytes"[..]);
    let at_the_trimmed_name = base.join("data").exists();
    println!(
        "WIN-SPACE create={created} open={opened} tail={} listing={listing:?} kept={kept} \
         trimmed_name_exists={at_the_trimmed_name}",
        report_line(&tail)
    );
    assert_eq!(created, "Io(InvalidInput)");
    assert_eq!(opened, "Io(InvalidInput)");
    assert_eq!(listing, vec!["left-as-it-was".to_string()]);
    assert!(kept, "a call wrote into the file under the directory");
    assert!(
        !at_the_trimmed_name,
        "a call made something at the trimmed name"
    );
    assert!(
        tail.is_err() && delete_report(&tail).not_followed == 1,
        "{}",
        report_line(&tail)
    );
}
