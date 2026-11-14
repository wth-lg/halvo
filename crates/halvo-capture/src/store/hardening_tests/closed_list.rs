//! The tests of ruling ST-44, the closed list of store-3 after its stress step:
//! the cases of the three adversaries and of the verify step, taken into the
//! unit's tests with the assertions that the ruling gives them. Every store
//! and every root lives in a directory that the test made. No test prints a
//! key, a root or the bytes of a segment: lengths, flags and answers only.

use super::*;
use crate::custody::KeyLabel;
use std::os::unix::fs::PermissionsExt;

fn adv1_copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type().unwrap().is_dir() {
            adv1_copy_tree(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

fn adv1_names(dir: &Path) -> Vec<String> {
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
fn adv1_shapes(dir: &Path) -> Vec<String> {
    adv1_names(dir)
        .into_iter()
        .map(|name| match name.split_once('.') {
            Some((stem, ext)) if stem.len() == 32 => format!("<32 hex>.{ext}"),
            _ => name,
        })
        .collect()
}

fn adv1_wrapped(store_dir: &Path) -> WrappedKey {
    KeyFile::read(store_dir)
        .unwrap()
        .wrapped_store_key()
        .unwrap()
}

fn adv1_unwraps(roots_dir: &Path, root_id: RootId, wrapped: &WrappedKey) -> String {
    let custody = file_root_custody(roots_dir, TestProtector::new(0x5a).0, root_id);
    match custody.unwrap_store_key(wrapped, KeyLabel::STORE) {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

fn adv1_copy_reads(copy: &Path, roots_dir: &Path, root_id: RootId, segment: Uuid) -> String {
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

fn adv1_report_line(answer: &StoreResult<crate::store::DeleteReport>) -> String {
    let report = delete_report(answer);
    format!(
        "delete_all={} complete={} root_destroyed={} roots_left={} listed={} key_file_gone={} database_left={} files_left={} not_followed={}",
        if answer.is_ok() { "Ok" } else { "DeleteIncomplete" },
        report.complete(),
        report.root_destroyed(),
        report.roots.left.len(),
        report.roots.listed,
        report.key_file_gone,
        report.database_left,
        report.files_left,
        report.not_followed
    )
}

/// What moves before the delete.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Adv1Move {
    Nothing,
    TheDataDirectory,
    TheRootsDirectory,
    TheRootsDirectoryWithAnEmptyOneAtItsName,
}

/// One store in <tmp>/data/store with its roots in <tmp>/data/roots, a copy
/// of the store's files taken while it is closed (what a backup holds), the
/// move, then the delete (on the handle, or the tail), then: does the root
/// still unwrap the key that store.key held, and does the copy still open.
fn adv1_delete_after(what: Adv1Move, tail: bool) -> (bool, String) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(&data);
    let wrapped = adv1_wrapped(&store_dir);
    let copy = dir.path().join("copy-of-the-store");
    adv1_copy_tree(&store_dir, &copy);
    let handle = if tail {
        None
    } else {
        Some(open_over_file_root(&store_dir, &roots_dir, root_id).unwrap())
    };
    let moved_data = dir.path().join("data-moved");
    let roots_now = match what {
        Adv1Move::Nothing => roots_dir.clone(),
        Adv1Move::TheDataDirectory => {
            std::fs::rename(&data, &moved_data).unwrap();
            moved_data.join("roots")
        }
        Adv1Move::TheRootsDirectory | Adv1Move::TheRootsDirectoryWithAnEmptyOneAtItsName => {
            let moved = dir.path().join("roots-moved");
            std::fs::rename(&roots_dir, &moved).unwrap();
            if what == Adv1Move::TheRootsDirectoryWithAnEmptyOneAtItsName {
                std::fs::create_dir(&roots_dir).unwrap();
            }
            moved
        }
    };
    let store_now = match what {
        Adv1Move::TheDataDirectory => moved_data.join("store"),
        _ => store_dir.clone(),
    };
    let answer = match handle {
        Some(mut store) => {
            let answer = store.delete_all();
            let write_after = answer_text(&store.write_segment(&mut Cursor::new(b"after")));
            println!("ADV1 {what:?} tail={tail} write_after_the_delete={write_after}");
            answer
        }
        None => EncryptedStore::delete_store_dir(&store_dir, &roots_dir),
    };
    let line = adv1_report_line(&answer);
    let unwraps = adv1_unwraps(&roots_now, root_id, &wrapped);
    let copy_reads = adv1_copy_reads(&copy, &roots_now, root_id, segment);
    println!("ADV1 {what:?} tail={tail} {line}");
    println!(
        "ADV1 {what:?} tail={tail} store_dir_now={:?} roots_dir_now={:?}",
        adv1_shapes(&store_now),
        adv1_shapes(&roots_now)
    );
    println!("ADV1 {what:?} tail={tail} root_unwraps_the_key_of_store_key={unwraps} copy_of_the_store: {copy_reads}");
    (answer.is_ok() && unwraps == "Ok", line)
}

#[test]
fn control_nothing_moved_the_delete_answers_ok_and_no_root_unwraps() {
    for tail in [false, true] {
        let (ok_with_a_live_root, line) = adv1_delete_after(Adv1Move::Nothing, tail);
        assert!(!ok_with_a_live_root, "{line}");
        assert!(line.starts_with("delete_all=Ok"), "{line}");
    }
}

#[test]
fn delete_all_answers_ok_only_when_no_root_unwraps_after_the_data_directory_moved() {
    let (ok_with_a_live_root, line) = adv1_delete_after(Adv1Move::TheDataDirectory, false);
    assert!(
        !ok_with_a_live_root,
        "delete-all answered Ok and the root still unwraps the store key: {line}"
    );
}

#[test]
fn delete_all_answers_ok_only_when_no_root_unwraps_after_the_roots_directory_moved() {
    let (ok_with_a_live_root, line) = adv1_delete_after(Adv1Move::TheRootsDirectory, false);
    assert!(
        !ok_with_a_live_root,
        "delete-all answered Ok and the root still unwraps the store key: {line}"
    );
}

#[test]
fn delete_all_answers_ok_only_when_no_root_unwraps_after_an_empty_directory_took_the_roots_name() {
    let (ok_with_a_live_root, line) =
        adv1_delete_after(Adv1Move::TheRootsDirectoryWithAnEmptyOneAtItsName, false);
    assert!(
        !ok_with_a_live_root,
        "delete-all answered Ok and the root still unwraps the store key: {line}"
    );
}

#[test]
fn the_tail_answers_ok_only_when_no_root_unwraps_after_the_roots_directory_moved() {
    let (ok_with_a_live_root, line) = adv1_delete_after(Adv1Move::TheRootsDirectory, true);
    assert!(
        !ok_with_a_live_root,
        "the tail answered Ok and the root still unwraps the store key: {line}"
    );
}

/// A root file of mode 0400 (the unit's own case) that has a second name (the
/// doc's own case of a backup tool that restores with shared names): is the
/// second name zeroed, as the doc says of every name, before delete-all
/// answers Ok?
fn adv1_second_name_after_delete(mode: u32) -> (bool, String) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    let wrapped = adv1_wrapped(&store_dir);
    let copy = dir.path().join("copy-of-the-store");
    adv1_copy_tree(&store_dir, &copy);
    let roots = FileRoots::new(roots_dir.clone(), TestProtector::new(0x5a).0);
    let root_file = roots.root_path(root_id);
    let second = dir.path().join("second-name-of-the-root-file");
    std::fs::hard_link(&root_file, &second).unwrap();
    set_mode(&root_file, mode);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let answer = store.delete_all();
    drop(store);
    let line = adv1_report_line(&answer);
    let bytes = std::fs::read(&second).unwrap();
    let all_zero = bytes.iter().all(|&b| b == 0);
    let put_back = dir.path().join("roots-put-back");
    std::fs::create_dir(&put_back).unwrap();
    std::fs::set_permissions(&put_back, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::copy(&second, put_back.join(format!("{root_id}.root"))).unwrap();
    let unwraps = adv1_unwraps(&put_back, root_id, &wrapped);
    let copy_reads = adv1_copy_reads(&copy, &put_back, root_id, segment);
    println!("ADV1 second-name mode={mode:o} {line}");
    println!(
        "ADV1 second-name mode={mode:o} roots_dir_now={:?} second_name_len={} second_name_all_zero={all_zero}",
        adv1_shapes(&roots_dir),
        bytes.len()
    );
    println!(
        "ADV1 second-name mode={mode:o} put_back_unwraps={unwraps} copy_of_the_store: {copy_reads}"
    );
    (answer.is_ok() && !all_zero, line)
}

#[test]
fn control_a_second_name_of_a_root_file_of_mode_0600_is_zeroed() {
    let (ok_with_a_live_second_name, line) = adv1_second_name_after_delete(0o600);
    assert!(!ok_with_a_live_second_name, "{line}");
}

#[test]
fn a_second_name_of_a_root_file_of_mode_0400_is_zeroed_before_delete_all_answers_ok() {
    let (ok_with_a_live_second_name, line) = adv1_second_name_after_delete(0o400);
    assert!(
        !ok_with_a_live_second_name,
        "delete-all answered Ok and a second name of the root file keeps the root: {line}"
    );
}

/// Question 7: calls on a handle that delete-all sealed.
#[test]
fn calls_on_a_sealed_handle_answer_sealed_and_touch_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let first = store.delete_all();
    println!("ADV1 sealed first {}", adv1_report_line(&first));
    assert!(first.is_ok());

    // A second store under the same roots directory, made after the delete.
    let other_id = RootId::fresh().unwrap();
    let other_dir = dir.path().join("other-store");
    let other = EncryptedStore::create(
        &other_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            other_id,
        )),
        DEFAULT_RETENTION,
    )
    .unwrap();
    drop(other);
    // Files that the sealed store did not make, put into its directory.
    std::fs::write(store_dir.join("store.key"), vec![7u8; 100]).unwrap();
    std::fs::create_dir_all(store_dir.join("segments").join("ab")).unwrap();
    std::fs::write(
        store_dir.join("segments").join("ab").join("x"),
        vec![7u8; 1000],
    )
    .unwrap();

    let mut out = Vec::new();
    println!(
        "ADV1 sealed read_segment={}",
        answer_text(&store.read_segment(segment, &mut out))
    );
    println!(
        "ADV1 sealed write_segment={}",
        answer_text(&store.write_segment(&mut Cursor::new(b"after")))
    );
    println!(
        "ADV1 sealed crypto_shred_expired={}",
        answer_text(&store.crypto_shred_expired())
    );
    println!(
        "ADV1 sealed remove_keyless_files={}",
        answer_text(&store.remove_keyless_files())
    );
    let size = store.size_bytes();
    println!("ADV1 sealed size_bytes={size:?}");
    let before = (adv1_shapes(&store_dir), adv1_shapes(&roots_dir));
    let second = store.delete_all();
    let after = (adv1_shapes(&store_dir), adv1_shapes(&roots_dir));
    println!("ADV1 sealed second {}", adv1_report_line(&second));
    println!(
        "ADV1 sealed before_second_call store_dir={:?} roots_dir={:?}",
        before.0, before.1
    );
    println!(
        "ADV1 sealed after_second_call store_dir={:?} roots_dir={:?}",
        after.0, after.1
    );
    drop(store);
    let other_opens = answer_text(&open_over_file_root(&other_dir, &roots_dir, other_id));
    println!("ADV1 sealed other_store_under_the_same_roots_directory_opens={other_opens}");
    assert!(
        matches!(size, Err(StoreError::Sealed)),
        "size_bytes on a sealed handle listed the directory: {size:?}"
    );
    assert_eq!(
        before, after,
        "the second delete-all on a sealed handle removed entries"
    );
    assert_eq!(other_opens, "Ok");
}

/// The layout of the head's one caller outside the tests
/// (store/verify.rs, scratch_store): the roots directory is `roots` inside
/// the store directory, under the dev-fallback custody, through the public
/// create and open.
fn adv1_store_with_its_roots_inside(dir: &Path) -> (PathBuf, RootId, Uuid, WrappedKey) {
    let store_dir = dir.join("store");
    let root_id = RootId::fresh().unwrap();
    let custody = DevFallbackCustody::new(root_id, store_dir.join("roots"));
    let mut store =
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).unwrap();
    let (segment, _) = store
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(store);
    let wrapped = adv1_wrapped(&store_dir);
    (store_dir, root_id, segment.segment_id, wrapped)
}

fn adv1_dev_fallback_unwraps(roots: &Path, root_id: RootId, wrapped: &WrappedKey) -> String {
    match DevFallbackCustody::new(root_id, roots.to_path_buf())
        .unwrap_store_key(wrapped, KeyLabel::STORE)
    {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("{e:?}"),
    }
}

/// P1 of the unit (the store directory moved, another directory at its
/// name), with the roots where verify_store puts them.
#[test]
fn p1_with_the_roots_inside_the_store_directory_leaves_no_root_that_unwraps() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, root_id, _, wrapped) = adv1_store_with_its_roots_inside(dir.path());
    let roots = store_dir.join("roots");
    let mut store = EncryptedStore::open(&store_dir, &roots, DEFAULT_RETENTION).unwrap();
    let moved = dir.path().join("moved-away");
    std::fs::rename(&store_dir, &moved).unwrap();
    std::fs::create_dir(&store_dir).unwrap();
    let answer = store.delete_all();
    drop(store);
    let line = adv1_report_line(&answer);
    let unwraps = adv1_dev_fallback_unwraps(&moved.join("roots"), root_id, &wrapped);
    println!("ADV1 p1-roots-inside {line}");
    println!(
        "ADV1 p1-roots-inside moved_store_dir={:?} moved_roots={:?} root_unwraps_the_key_of_store_key={unwraps}",
        adv1_shapes(&moved),
        adv1_shapes(&moved.join("roots"))
    );
    assert!(
        !(answer.is_ok() && unwraps == "Ok"),
        "delete-all answered Ok and the root still unwraps the store key: {line}"
    );
}

/// The tail on the same layout after the store directory was moved: the
/// name is gone, and so is the roots directory below it.
#[test]
fn the_tail_with_the_roots_inside_a_moved_store_directory_answers_ok_only_when_the_store_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, root_id, segment, wrapped) = adv1_store_with_its_roots_inside(dir.path());
    let roots = store_dir.join("roots");
    let moved = dir.path().join("moved-away");
    std::fs::rename(&store_dir, &moved).unwrap();
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots);
    let line = adv1_report_line(&answer);
    let unwraps = adv1_dev_fallback_unwraps(&moved.join("roots"), root_id, &wrapped);
    let reads = match EncryptedStore::open(&moved, &moved.join("roots"), DEFAULT_RETENTION) {
        Ok(store) => {
            let mut out = Vec::new();
            match store.read_segment(segment, &mut out) {
                Ok(_) => format!("open=Ok segment_plaintext_equal={}", out == b"typed errors"),
                Err(e) => format!("open=Ok read={e:?}"),
            }
        }
        Err(e) => format!("open={e:?}"),
    };
    println!("ADV1 tail-roots-inside {line}");
    println!(
        "ADV1 tail-roots-inside moved_store_dir={:?} root_unwraps_the_key_of_store_key={unwraps} the_moved_store: {reads}",
        adv1_shapes(&moved)
    );
    assert!(
        answer.is_ok() && !delete_report(&answer).root_destroyed(),
        "the tail on a name where nothing stands must answer Ok with the root unnamed, not destroyed: {line}"
    );
}

/// The tail on a store directory that it cannot open (mode 000): does it
/// try its root step, as every other answer of that open does?
#[test]
fn the_tail_on_a_store_directory_of_mode_000_destroys_the_root_or_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    set_mode(&store_dir, 0o000);
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    set_mode(&store_dir, 0o700);
    let text = match &answer {
        Ok(_) | Err(StoreError::DeleteIncomplete(_)) => adv1_report_line(&answer),
        Err(e) => format!("{e:?} (no report)"),
    };
    let reads = adv1_copy_reads(&store_dir, &roots_dir, root_id, segment);
    println!("ADV1 tail-mode-000 answer={text}");
    println!(
        "ADV1 tail-mode-000 roots_dir_now={:?} store_dir_now={:?} the_store: {reads}",
        adv1_shapes(&roots_dir),
        adv1_shapes(&store_dir)
    );
    assert!(
        !reads.starts_with("open=Ok")
            || matches!(&answer, Err(StoreError::DeleteIncomplete(report)) if !report.root_destroyed()),
        "the tail answered {text}, tried no root step, and the store still opens and reads"
    );
}

/// delete-all with the segments directory at mode 0500: the segment file
/// goes, the shard directory and segments/ cannot be removed.
#[test]
fn delete_all_with_segments_at_0500_answers_ok_only_when_nothing_of_the_store_is_left() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
    let shard = segment_path(&store_dir, segment)
        .parent()
        .unwrap()
        .to_path_buf();
    let shard_name = shard.file_name().unwrap().to_string_lossy().into_owned();
    let id_text = segment.simple().to_string();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    set_mode(&store_dir.join("segments"), 0o500);
    let answer = store.delete_all();
    let again = store.delete_all();
    drop(store);
    set_mode(&store_dir.join("segments"), 0o700);
    let line = adv1_report_line(&answer);
    println!("ADV1 segments-0500 first {line}");
    println!("ADV1 segments-0500 second {}", adv1_report_line(&again));
    println!(
        "ADV1 segments-0500 store_dir_now={:?} entries_below_segments={} shard_name_len={} shard_name_is_a_cut_of_the_segment_id={}",
        adv1_names(&store_dir),
        adv1_names(&store_dir.join("segments")).len(),
        shard_name.len(),
        id_text.contains(&shard_name)
    );
    assert!(
        !(answer.is_ok() && store_dir.join("segments").exists()),
        "delete-all answered Ok with files_left 0 and segments/ with its shard directory stays: {line}"
    );
}

struct Adv1StopsAtItsRoot<C>(C);

impl<C: KeyCustody> KeyCustody for Adv1StopsAtItsRoot<C> {
    fn kind(&self) -> CustodyKind {
        self.0.kind()
    }
    fn root_id(&self) -> RootId {
        self.0.root_id()
    }
    fn roots(&self) -> Option<&Path> {
        self.0.roots()
    }
    fn wrap_store_key(&self, key: &StoreKey, label: KeyLabel) -> Result<WrappedKey, CustodyError> {
        self.0.wrap_store_key(key, label)
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        panic!("ADV1 the create stops here, before its root step, as a killed process does");
    }
    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        label: KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        self.0.unwrap_store_key(wrapped, label)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.0.destroy_root()
    }
}

/// A create that stopped after its pending mark and before its root, a second
/// directory that makes a live store under the same root id, and then a
/// create in the first directory: does it destroy the root of the live store?
#[test]
fn a_create_over_a_stale_pending_mark_destroys_no_root_of_a_live_store() {
    let dir = tempfile::tempdir().unwrap();
    let roots_dir = dir.path().join("roots");
    let (first, second) = (dir.path().join("first"), dir.path().join("second"));
    let shared = RootId::fresh().unwrap();
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        EncryptedStore::create(
            &first,
            Box::new(Adv1StopsAtItsRoot(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                shared,
            ))),
            DEFAULT_RETENTION,
        )
        .map(drop)
    }));
    println!(
        "ADV1 stale-pending the_first_create_stopped={} first_dir_now={:?} roots_dir_now={:?}",
        stopped.is_err(),
        adv1_names(&first),
        adv1_shapes(&roots_dir)
    );
    let mut live = EncryptedStore::create(
        &second,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            shared,
        )),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (segment, _) = live
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(live);
    println!(
        "ADV1 stale-pending the_second_directory_made_a_store_under_the_same_root_id roots_dir_now={:?} it: {}",
        adv1_shapes(&roots_dir),
        adv1_copy_reads(&second, &roots_dir, shared, segment.segment_id)
    );
    let third = RootId::fresh().unwrap();
    let again = EncryptedStore::create(
        &first,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            third,
        )),
        DEFAULT_RETENTION,
    );
    let again_text = answer_text(&again);
    drop(again);
    let reads = adv1_copy_reads(&second, &roots_dir, shared, segment.segment_id);
    println!(
        "ADV1 stale-pending create_in_the_first_directory_again={again_text} root_files_now={} the_live_store_in_the_second_directory: {reads}",
        root_files_in(&roots_dir).len()
    );
    assert!(
        reads.starts_with("open=Ok"),
        "a create over a stale pending mark destroyed the root of a live store: {reads}"
    );
}

struct Adv1AnotherPartyWritesStoreKey<C> {
    inner: C,
    at: PathBuf,
}

impl<C: KeyCustody> KeyCustody for Adv1AnotherPartyWritesStoreKey<C> {
    fn kind(&self) -> CustodyKind {
        self.inner.kind()
    }
    fn root_id(&self) -> RootId {
        self.inner.root_id()
    }
    fn roots(&self) -> Option<&Path> {
        self.inner.roots()
    }
    fn wrap_store_key(&self, key: &StoreKey, label: KeyLabel) -> Result<WrappedKey, CustodyError> {
        // The instant between the create's look at store.key and its publish:
        // another party, which holds no lock, puts its own file at the name.
        std::fs::write(&self.at, vec![7u8; 200]).unwrap();
        self.inner.wrap_store_key(key, label)
    }
    fn create_root(&self) -> Result<(), CustodyError> {
        self.inner.create_root()
    }
    fn unwrap_store_key(
        &self,
        wrapped: &WrappedKey,
        label: KeyLabel,
    ) -> Result<StoreKey, CustodyError> {
        self.inner.unwrap_store_key(wrapped, label)
    }
    fn destroy_root(&self) -> Result<(), CustodyError> {
        self.inner.destroy_root()
    }
}

/// The create-only publish refuses an entry that appeared at store.key after
/// the create's look. Does the create then leave that entry, which it did
/// not make?
#[test]
fn a_create_whose_publish_meets_an_entry_at_store_key_leaves_that_entry() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    std::fs::create_dir(&store_dir).unwrap();
    let root_id = RootId::fresh().unwrap();
    let custody = Adv1AnotherPartyWritesStoreKey {
        inner: file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id),
        at: store_dir.join("store.key"),
    };
    let answer = EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION);
    let text = answer_text(&answer);
    drop(answer);
    let theirs = std::fs::read(store_dir.join("store.key"));
    let kept = matches!(&theirs, Ok(bytes) if bytes == &vec![7u8; 200]);
    println!(
        "ADV1 publish-meets-an-entry create={text} store_dir_now={:?} roots_dir_now={:?} the_other_partys_file_kept={kept} read={:?}",
        adv1_names(&store_dir),
        adv1_shapes(&roots_dir),
        theirs.as_ref().map(|b| b.len()).map_err(|e| e.kind())
    );
    assert!(
        kept,
        "the create answered {text} and removed the entry at store.key that it did not make"
    );
}

/// The tail twice on a key file of version 1: the first call reports the
/// root as not handled by this build and removes the key file, the one
/// record that names that root. What does the second call answer?
#[test]
fn a_second_tail_on_a_key_file_of_version_1_does_not_answer_ok_for_a_root_that_no_call_destroyed() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
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
    println!(
        "ADV1 version-1 first {} root_not_handled_is_named={}",
        adv1_report_line(&first),
        delete_report(&first).root_not_handled == Some(older)
    );
    println!(
        "ADV1 version-1 second {} root_not_handled_is_named={}",
        adv1_report_line(&second),
        delete_report(&second).root_not_handled.is_some()
    );
    assert!(
        first.is_err() && delete_report(&first).root_not_handled == Some(older),
        "the first tail must answer DeleteIncomplete and name the root as not handled"
    );
    assert!(
        !delete_report(&second).root_destroyed(),
        "the second tail reads the root of a key file of version 1 as destroyed, and no call destroyed it"
    );
}

/// The tail on a volume that folds case, after the root file's name was put
/// in upper case: the tail destroys roots by its listing alone.
#[test]
fn the_tail_destroys_a_root_file_whose_name_stands_in_upper_case_on_a_volume_that_folds_case() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = adv1_wrapped(&store_dir);
    let lower = roots_dir.join(format!("{root_id}.root"));
    let upper = roots_dir.join(format!("{root_id}.root").to_uppercase());
    std::fs::rename(&lower, &upper).unwrap();
    let folds = lower.exists();
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let line = adv1_report_line(&answer);
    let unwraps = adv1_unwraps(&roots_dir, root_id, &wrapped);
    println!("ADV1 upper-case volume_folds_case={folds} {line}");
    println!(
        "ADV1 upper-case root_files_now={} root_unwraps_the_key_of_store_key={unwraps}",
        adv1_names(&roots_dir).len()
    );
    assert!(
        !(answer.is_ok() && unwraps == "Ok"),
        "the tail answered Ok and the root still unwraps the store key: {line}"
    );
}

/// P1 of the unit once more, in the layout of the contract's scratch stores
/// (roots inside the store directory): the store directory is moved, and a
/// second store is made at its old name before the first one is deleted.
/// Delete-all of the first acts on its files through its handle. On which
/// roots directory does its root step act?
#[test]
fn delete_all_of_a_moved_store_keeps_the_root_of_the_store_made_at_its_old_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, first_id, _, first_wrapped) = adv1_store_with_its_roots_inside(dir.path());
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
    let answer = first.delete_all();
    drop(first);
    let line = adv1_report_line(&answer);
    let first_root = adv1_dev_fallback_unwraps(&moved.join("roots"), first_id, &first_wrapped);
    let second_reads = match EncryptedStore::open(&store_dir, &roots, DEFAULT_RETENTION) {
        Ok(store) => {
            let mut out = Vec::new();
            match store.read_segment(segment.segment_id, &mut out) {
                Ok(_) => format!("open=Ok segment_plaintext_equal={}", out == b"typed errors"),
                Err(e) => format!("open=Ok read={e:?}"),
            }
        }
        Err(e) => format!("open={e:?}"),
    };
    println!("ADV1 moved-and-replaced {line}");
    println!(
        "ADV1 moved-and-replaced the_moved_directory={:?} its_roots={:?} the_root_of_the_deleted_store_unwraps_its_key={first_root}",
        adv1_shapes(&moved),
        adv1_shapes(&moved.join("roots"))
    );
    println!(
        "ADV1 moved-and-replaced the_directory_at_the_old_name={:?} its_roots={:?} the_store_made_there: {second_reads}",
        adv1_shapes(&store_dir),
        adv1_shapes(&roots)
    );
    assert!(
        second_reads.starts_with("open=Ok"),
        "delete-all of the moved store destroyed the root of the store made at its old name: {line}; the store at its old name: {second_reads}"
    );
    assert!(
        answer.is_err() && !delete_report(&answer).root_destroyed(),
        "delete-all of the moved store must answer DeleteIncomplete with its root not destroyed, while that root stays in the moved directory (unwraps: {first_root}): {line}"
    );
}

// Adversary 3: F1, F2 and F3 of its report.
#[cfg(target_os = "macos")]
fn adv3_set_immutable(path: &Path, on: bool) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let flags = if on { libc::UF_IMMUTABLE } else { 0 };
    // SAFETY: c is NUL-terminated and outlives the call.
    assert_eq!(
        unsafe { libc::chflags(c.as_ptr(), flags as _) },
        0,
        "chflags"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_partial_root_file_with_the_root_bytes_that_stays_leaves_delete_all_ok() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let root_bytes = std::fs::read(&root_file).unwrap();
    let partial = roots_dir.join(format!("{root_id}.0123456789abcdef.root.tmp"));
    std::fs::write(&partial, &root_bytes).unwrap();
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    adv3_set_immutable(&partial, true);
    let answer = store.delete_all();
    adv3_set_immutable(&partial, false);
    let stays = partial.exists();
    let same = std::fs::read(&partial).ok().as_deref() == Some(&root_bytes[..]);
    let named = match &answer {
        Ok(r) => r.roots.left.len(),
        Err(StoreError::DeleteIncomplete(r)) => r.roots.left.len(),
        Err(_) => 99,
    };
    println!(
        "ADV3-PARTIAL answer_ok={} root_file_left={} partial_left={stays} partial_holds_the_root_bytes={same} roots_named_in_report={named}",
        answer.is_ok(),
        root_file.exists()
    );
    assert!(
        !(answer.is_ok() && stays && same),
        "delete_all answered Ok while a partial root file with the root's bytes stays"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_store_key_temp_with_the_wrapped_key_that_stays_leaves_delete_all_ok() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let temp = store_dir.join("store.key.tmp");
    std::fs::copy(store_dir.join("store.key"), &temp).unwrap();
    let key_bytes = std::fs::read(store_dir.join("store.key")).unwrap();
    adv3_set_immutable(&temp, true);
    let answer = store.delete_all();
    adv3_set_immutable(&temp, false);
    let stays = temp.exists();
    let same = std::fs::read(&temp).ok().as_deref() == Some(&key_bytes[..]);
    let files_left = match &answer {
        Ok(r) => r.files_left,
        Err(StoreError::DeleteIncomplete(r)) => r.files_left,
        Err(_) => 99,
    };
    println!(
        "ADV3-KEYTEMP answer_ok={} store_key_left={} temp_left={stays} temp_holds_the_key_file_bytes={same} files_left_in_report={files_left}",
        answer.is_ok(),
        store_dir.join("store.key").exists()
    );
    assert!(
        !(answer.is_ok() && stays),
        "delete_all answered Ok while store.key.tmp stays"
    );
}

#[cfg(unix)]
#[test]
fn a_create_whose_key_name_is_taken_after_its_root_leaves_what_stands_there() {
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
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(
        &dir.path().join("roots"),
        TestProtector::new(0x5a).0,
        root_id,
    );
    let created = answer_text(&EncryptedStore::create(
        &store_dir,
        Box::new(custody),
        DEFAULT_RETENTION,
    ));
    drop(seam);
    let after = std::fs::read(&outside).unwrap();
    println!(
        "ADV3-KEYNAME answer={created} outside_unchanged={} outside_all_zero={} store_key_left={}",
        after == original,
        after.iter().all(|b| *b == 0),
        store_dir.join("store.key").exists()
    );
    assert_ne!(created, "Ok");
    assert_eq!(
        after, original,
        "the failed create wrote over a file that it did not make"
    );
    assert!(
        store_dir.join("store.key").exists(),
        "the failed create removed a store.key that it did not make"
    );
}

#[test]
fn a_key_file_version_past_u32_or_below_zero_answers_newer_or_older_store() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let key = store_dir.join("store.key");
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&key).unwrap()).unwrap();
    let mut wrong = Vec::new();
    for (version, want) in [
        (serde_json::json!(3), "NewerStore"),
        (serde_json::json!(4294967296u64), "NewerStore"),
        (serde_json::json!(-1), "OlderStore"),
    ] {
        json["version"] = version.clone();
        std::fs::remove_file(&key).unwrap();
        std::fs::write(&key, serde_json::to_vec(&json).unwrap()).unwrap();
        let opened = answer_text(&open_over_file_root(&store_dir, &roots_dir, root_id));
        println!("ADV3-VERSION version={version} answer={opened}");
        if !opened.starts_with(want) {
            wrong.push(format!("{version}: {opened}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

#[cfg(unix)]
#[test]
fn a_create_whose_key_name_is_taken_by_a_plain_file_after_its_root_removes_that_file() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let planted = b"{\"version\":2,\"written\":\"by another writer\"}".to_vec();
    let act = {
        let (key, planted) = (store_dir.join("store.key"), planted.clone());
        move || std::fs::write(&key, &planted).unwrap()
    };
    let seam = crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT, nth_call(1, act));
    let root_id = RootId::fresh().unwrap();
    let roots = dir.path().join("roots");
    let custody = file_root_custody(&roots, TestProtector::new(0x5a).0, root_id);
    let created = answer_text(&EncryptedStore::create(
        &store_dir,
        Box::new(custody),
        DEFAULT_RETENTION,
    ));
    drop(seam);
    let left = std::fs::read(store_dir.join("store.key")).ok();
    println!(
        "ADV3-KEYNAME-PLAIN answer={created} store_key_left={} store_key_unchanged={} own_root_left={}",
        left.is_some(),
        left.as_deref() == Some(&planted[..]),
        roots.join(format!("{root_id}.root")).exists()
    );
    assert_ne!(created, "Ok");
    assert_eq!(
        left.as_deref(),
        Some(&planted[..]),
        "the failed create removed a store.key that it did not make"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_tail_answers_ok_while_a_partial_root_and_a_store_key_temp_stay() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let root_bytes = std::fs::read(roots_dir.join(format!("{root_id}.root"))).unwrap();
    let partial = roots_dir.join(format!("{root_id}.0123456789abcdef.root.tmp"));
    std::fs::write(&partial, &root_bytes).unwrap();
    let temp = store_dir.join("store.key.tmp");
    std::fs::copy(store_dir.join("store.key"), &temp).unwrap();
    adv3_set_immutable(&partial, true);
    adv3_set_immutable(&temp, true);
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    adv3_set_immutable(&partial, false);
    adv3_set_immutable(&temp, false);
    println!(
        "ADV3-TAIL answer_ok={} partial_left={} temp_left={} report={}",
        answer.is_ok(),
        partial.exists(),
        temp.exists(),
        match &answer {
            Ok(r) => format!(
                "roots_left={} files_left={}",
                r.roots.left.len(),
                r.files_left
            ),
            Err(e) => format!("{e}"),
        }
    );
    let elsewhere = dir.path().join("roots-elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::fs::copy(&partial, elsewhere.join(format!("{root_id}.root"))).unwrap();
    let custody = file_root_custody(&elsewhere, TestProtector::new(0x5a).0, root_id);
    let wrapped = KeyFile::at(&temp)
        .and_then(|place| place.read(crate::custody::KeyLabel::STORE))
        .and_then(|key_file| key_file.wrapped_store_key());
    let unwraps = wrapped
        .map(|w| {
            custody
                .unwrap_store_key(&w, crate::custody::KeyLabel::STORE)
                .is_ok()
        })
        .unwrap_or(false);
    println!("ADV3-TAIL the leftover partial root unwraps the wrapped key of the leftover store.key.tmp: {unwraps}");
    assert!(
        !(answer.is_ok() && (partial.exists() || temp.exists())),
        "the tail answered Ok while files stay"
    );
}

// Adversary 2: F1 of its report, the kill at the seam.
#[cfg(unix)]
fn adv2_open_at(
    store_dir: &Path,
    roots_dir: &Path,
    root_id: RootId,
) -> StoreResult<EncryptedStore> {
    let roots = roots_dir.to_path_buf();
    EncryptedStore::open_with(store_dir, roots_dir, DEFAULT_RETENTION, move |_| {
        default_file_root_custody(&roots, root_id)
    })
}

/// Child: create B in `b` under root id X, and inside B's database step
/// (after B wrote create.pending naming X) let create A in `a` under the same
/// root id X run to its Ok; then the process stops (SIGKILL by its own pid).
#[cfg(unix)]
#[test]
fn child_two_creates_one_root_id_second_stopped_after_its_pending() {
    let Ok(dir) = std::env::var("ADV2_TWO_CREATES_DIR") else {
        println!("adv2 child: ADV2_TWO_CREATES_DIR is not set");
        return;
    };
    let dir = PathBuf::from(dir);
    let root_id = RootId::parse(&std::fs::read_to_string(dir.join("root-id")).unwrap()).unwrap();
    let roots = dir.join("roots");
    let a_dir = dir.join("a");
    let marks = dir.clone();
    let roots_for_a = roots.clone();
    let _seam =
        crate::store::failpoint::at(crate::store::db::SEAM_BEFORE_THE_PAGE_LOOK, move || {
            let b_pending = std::fs::read(marks.join("b").join("create.pending")).is_ok();
            std::fs::write(marks.join("b-pending-seen"), format!("{b_pending}")).unwrap();
            let a = EncryptedStore::create(
                &a_dir,
                Box::new(file_root_custody(
                    &roots_for_a,
                    TestProtector::new(0x5a).0,
                    root_id,
                )),
                DEFAULT_RETENTION,
            );
            std::fs::write(
                marks.join("a-answer"),
                format!("{:?}", a.as_ref().map(|_| ())),
            )
            .unwrap();
            drop(a);
            // SAFETY: SIGKILL to the process's own id ends this child, which the
            // parent started for this case alone.
            unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
        });
    let _ = EncryptedStore::create(
        &dir.join("b"),
        Box::new(file_root_custody(
            &roots,
            TestProtector::new(0x5a).0,
            root_id,
        )),
        DEFAULT_RETENTION,
    );
}

#[cfg(unix)]
#[test]
fn a_stopped_create_under_a_root_id_that_another_create_then_made_keeps_that_live_root() {
    let work = tempfile::tempdir().unwrap();
    let root_id = RootId::fresh().unwrap();
    std::fs::write(work.path().join("root-id"), root_id.to_string()).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::hardening_tests::closed_list::child_two_creates_one_root_id_second_stopped_after_its_pending",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ADV2_TWO_CREATES_DIR", work.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    let (roots, a_dir, b_dir) = (
        work.path().join("roots"),
        work.path().join("a"),
        work.path().join("b"),
    );
    let a_answer = std::fs::read_to_string(work.path().join("a-answer")).unwrap_or_default();
    let b_pending_seen =
        std::fs::read_to_string(work.path().join("b-pending-seen")).unwrap_or_default();
    println!(
        "adv2 child_status={status:?} a_answer={a_answer} b_pending_at_the_seam={b_pending_seen} roots={:?} b_pending_now={} a_key_file={}",
        root_files_in(&roots).len(),
        b_dir.join("create.pending").exists(),
        a_dir.join("store.key").exists(),
    );
    let first_open = adv2_open_at(&a_dir, &roots, root_id).map(drop);
    println!("adv2 open of A before anything else: {first_open:?}");
    let y = RootId::fresh().unwrap();
    let next_create_in_b = EncryptedStore::create(
        &b_dir,
        Box::new(file_root_custody(&roots, TestProtector::new(0x5a).0, y)),
        DEFAULT_RETENTION,
    )
    .map(drop);
    println!("adv2 next create in B under a fresh root: {next_create_in_b:?}");
    let a_root_left = roots.join(format!("{root_id}.root")).exists();
    let second_open = adv2_open_at(&a_dir, &roots, root_id).map(drop);
    println!(
        "adv2 A's root file after B's next create: {a_root_left}; open of A now: {second_open:?}"
    );
    assert!(
        first_open.is_ok(),
        "A did not open after the child: {first_open:?}"
    );
    assert!(
        second_open.is_ok(),
        "store A answered Ok to its create and opened, and a create in another directory destroyed its root: {second_open:?}"
    );
}

// The verify step: the roots beside the store.
fn ver12_shapes(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .map(|name| match name.split_once('.') {
                    Some((stem, ext)) if stem.len() == 32 => format!("<32 hex>.{ext}"),
                    _ => name,
                })
                .collect();
            names.sort();
            names
        }
        Err(e) => vec![format!("<{:?}>", e.kind())],
    }
}

fn ver12_report_line(answer: &StoreResult<crate::store::DeleteReport>) -> String {
    match answer {
        Ok(_) | Err(StoreError::DeleteIncomplete(_)) => {
            let report = delete_report(answer);
            format!(
                "{} complete={} root_destroyed={} roots_left={} listed={} key_file_gone={} database_left={} files_left={} not_followed={}",
                if answer.is_ok() { "Ok" } else { "DeleteIncomplete" },
                report.complete(),
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

fn ver12_reads(store_dir: &Path, roots_dir: &Path, root_id: RootId, segment: Uuid) -> String {
    match open_over_file_root(store_dir, roots_dir, root_id) {
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

/// Question (b). Adversary 1's case 29 with the roots BESIDE the store
/// (<data>/store and <data>/roots): the data directory is moved while the
/// first store is open, a second data directory with a second store is made
/// at the old name, and the first store is deleted on its handle.
#[test]
fn delete_all_after_the_data_directory_moved_keeps_the_root_of_a_store_made_at_the_old_names() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let (store_dir, roots_dir, first_id, _) = file_root_store(&data);
    let first_wrapped = KeyFile::read(&store_dir)
        .unwrap()
        .wrapped_store_key()
        .unwrap();
    let mut first = open_over_file_root(&store_dir, &roots_dir, first_id).unwrap();
    let moved = dir.path().join("data-moved");
    std::fs::rename(&data, &moved).unwrap();
    std::fs::create_dir(&data).unwrap();
    let (second_dir, second_roots, second_id, segment) = file_root_store(&data);
    assert_eq!(second_roots, roots_dir);
    let answer = first.delete_all();
    drop(first);
    let first_root =
        match file_root_custody(&moved.join("roots"), TestProtector::new(0x5a).0, first_id)
            .unwrap_store_key(&first_wrapped, KeyLabel::STORE)
        {
            Ok(_) => "Ok".to_string(),
            Err(e) => format!("{e:?}"),
        };
    let second_reads = ver12_reads(&second_dir, &second_roots, second_id, segment);
    println!(
        "VER12 roots-beside delete_all={}",
        ver12_report_line(&answer)
    );
    println!(
        "VER12 roots-beside the_moved_data_directory: store={:?} roots={:?} the_root_of_the_deleted_store_unwraps_its_key={first_root}",
        ver12_shapes(&moved.join("store")),
        ver12_shapes(&moved.join("roots"))
    );
    println!(
        "VER12 roots-beside the_data_directory_at_the_old_name: store={:?} roots={:?} the_store_made_there: {second_reads}",
        ver12_shapes(&second_dir),
        ver12_shapes(&second_roots)
    );
    assert!(
        second_reads.starts_with("open=Ok"),
        "delete-all of the moved store destroyed the root of the store at the old names: {}; the store at the old names: {second_reads}",
        ver12_report_line(&answer)
    );
    assert!(
        answer.is_err() && !delete_report(&answer).root_destroyed(),
        "delete-all of the moved store must answer DeleteIncomplete with its root not destroyed (unwraps: {first_root}): {}",
        ver12_report_line(&answer)
    );
}

// Tests of ruling ST-44 that no adversary wrote.

/// F4: an entry at store.key.tmp that the create did not make and that has a
/// second name is removed without a zero-fill, so the file behind the second
/// name keeps its bytes.
#[test]
fn a_create_removes_a_hard_link_at_store_key_tmp_without_writing_through_it() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    std::fs::create_dir(&store_dir).unwrap();
    let outside = dir.path().join("outside-file");
    let original = b"bytes that are not the store's".to_vec();
    std::fs::write(&outside, &original).unwrap();
    std::fs::hard_link(&outside, store_dir.join("store.key.tmp")).unwrap();
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
    let after = std::fs::read(&outside).unwrap();
    println!(
        "CL-KEYTEMP-LINK answer={created} outside_unchanged={} store_key_tmp_left={}",
        after == original,
        store_dir.join("store.key.tmp").exists()
    );
    assert_eq!(
        after, original,
        "the create wrote zeros through a second name at store.key.tmp"
    );
}

/// F1 (d), case 06 of adversary 1 with a second call: the tail after the roots
/// directory moved names the root as left, and the second tail, which finds no
/// record of a root, never reads it as destroyed.
#[test]
fn the_second_tail_after_the_roots_directory_moved_does_not_read_the_root_as_destroyed() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = adv1_wrapped(&store_dir);
    let moved = dir.path().join("roots-moved");
    std::fs::rename(&roots_dir, &moved).unwrap();
    let first = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let second = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let unwraps = adv1_unwraps(&moved, root_id, &wrapped);
    println!("CL-TAIL-TWICE first {}", adv1_report_line(&first));
    println!("CL-TAIL-TWICE second {}", adv1_report_line(&second));
    println!("CL-TAIL-TWICE the_moved_root_unwraps={unwraps}");
    assert!(
        first.is_err() && !delete_report(&first).root_destroyed(),
        "the first tail must name the root as left: {}",
        adv1_report_line(&first)
    );
    assert!(
        !delete_report(&second).root_destroyed(),
        "the second tail read a root as destroyed that no call destroyed: {}",
        adv1_report_line(&second)
    );
}

/// F5: the same for a key file of a newer version whose version field parses.
#[test]
fn a_second_tail_on_a_key_file_of_version_3_does_not_read_its_root_as_destroyed() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    edit_key_file(&store_dir, |v| v["version"] = 3.into());
    let first = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let second = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    println!(
        "CL-V3 first {} root_not_handled_is_named={}",
        adv1_report_line(&first),
        delete_report(&first).root_not_handled == Some(root_id)
    );
    println!("CL-V3 second {}", adv1_report_line(&second));
    assert!(
        first.is_err() && delete_report(&first).root_not_handled == Some(root_id),
        "the first tail on a key file of version 3 must name its root as not handled"
    );
    assert!(
        !delete_report(&second).root_destroyed(),
        "the second tail read the root of a key file of version 3 as destroyed"
    );
}

/// F1 (d): a delete-all stopped between its root step and its file step. The
/// first tail cannot tell that from a root file moved out, so it answers
/// DeleteIncomplete with the root not found; the next answers Ok, unnamed.
#[test]
fn the_tail_after_a_delete_stopped_after_its_root_step_answers_not_found_then_unnamed() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let stop = crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT_STEP, || {
        panic!("CL the delete stops after its root step, as a killed process does");
    });
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.delete_all().map(drop)
    }));
    drop(stop);
    assert!(stopped.is_err(), "the seam did not stop the delete");
    drop(store);
    assert!(
        store_dir.join("store.key").exists(),
        "the stopped delete left no key file"
    );
    let first = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let second = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    println!("CL-STOPPED first {}", adv1_report_line(&first));
    println!("CL-STOPPED second {}", adv1_report_line(&second));
    assert!(
        first.is_err() && !delete_report(&first).root_destroyed(),
        "the first tail after a stopped delete must answer DeleteIncomplete with the root not found: {}",
        adv1_report_line(&first)
    );
    assert!(
        second.is_ok() && !delete_report(&second).root_destroyed(),
        "the second tail must answer Ok with the root unnamed: {}",
        adv1_report_line(&second)
    );
}

/// F1 (e): the tail on a key file whose custody kind keeps no root file under
/// the roots directory names that root as not handled and never destroyed.
#[test]
fn the_tail_on_a_key_file_of_a_custody_with_no_root_file_names_the_root_as_not_handled() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    edit_key_file(&store_dir, |v| v["custody_kind"] = "secure_enclave".into());
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let report = delete_report(&answer);
    println!(
        "CL-NO-ROOT-FILE {} root_not_handled_is_named={}",
        adv1_report_line(&answer),
        report.root_not_handled == Some(root_id)
    );
    assert!(
        answer.is_err() && report.root_not_handled == Some(root_id) && !report.root_destroyed(),
        "the tail must name the root of a custody that keeps no root file as not handled: {}",
        adv1_report_line(&answer)
    );
}
