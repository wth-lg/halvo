//! store-3b: the roots directory that a store holds for its life, the
//! identity of its root file, and the tail on a data directory with a closed
//! layout.

use super::*;
use crate::custody::KeyLabel;
use crate::store::RootReading;

fn answer_line(answer: &StoreResult<crate::store::DeleteReport>) -> String {
    match answer {
        Ok(report) => format!(
            "Ok root={} names_left={} listed={} at_its_path={}",
            report.root.as_text(),
            report.roots.names.len(),
            report.roots.listed,
            report.roots.at_its_path
        ),
        Err(StoreError::DeleteIncomplete(report)) => format!(
            "DeleteIncomplete root={} names_left={} listed={} at_its_path={}",
            report.root.as_text(),
            report.roots.names.len(),
            report.roots.listed,
            report.roots.at_its_path
        ),
        Err(e) => format!("{e:?}"),
    }
}

fn wrapped_of(store_dir: &Path) -> WrappedKey {
    KeyFile::read(store_dir)
        .unwrap()
        .wrapped_store_key()
        .unwrap()
}

fn unwraps_under(roots_dir: &Path, root_id: RootId, wrapped: &WrappedKey) -> bool {
    file_root_custody(roots_dir, TestProtector::new(0x5a).0, root_id)
        .unwrap_store_key(wrapped, KeyLabel::STORE)
        .is_ok()
}

fn no_root_bytes_at(path: &Path) -> bool {
    match std::fs::read(path) {
        Ok(bytes) => bytes.iter().all(|&b| b == 0),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

#[test]
fn st46_k6_the_handles_delete_destroys_its_root_in_the_moved_roots_directory_and_leaves_the_directory_at_the_old_name(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = wrapped_of(&store_dir);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let moved = dir.path().join("roots-moved");
    std::fs::rename(&roots_dir, &moved).unwrap();
    let other_dir = dir.path().join("other-store");
    let other_id = RootId::fresh().unwrap();
    drop(
        EncryptedStore::create(
            &other_dir,
            Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                other_id,
            )),
            DEFAULT_RETENTION,
        )
        .unwrap(),
    );
    let planted = roots_dir.join(format!("{root_id}.root"));
    std::fs::write(&planted, [0x33u8; 32]).unwrap();

    let answer = store.delete_all();
    drop(store);

    let moved_root_unwraps = unwraps_under(&moved, root_id, &wrapped);
    let planted_untouched = std::fs::read(&planted).ok().as_deref() == Some(&[0x33u8; 32][..]);
    let other_opens = open_over_file_root(&other_dir, &roots_dir, other_id).map(drop);
    println!(
        "ST46 K6 {} moved_root_unwraps={moved_root_unwraps} planted_untouched={planted_untouched} \
         other_store_open={other_opens:?}",
        answer_line(&answer)
    );
    assert!(
        !moved_root_unwraps,
        "K6: the root in the moved roots directory still unwraps after the handle's delete: {}",
        answer_line(&answer)
    );
    assert!(
        planted_untouched,
        "K6: the delete touched the file at the root's name in the directory put at the old name"
    );
    assert!(
        other_opens.is_ok(),
        "K6: the store made at the old roots name does not open after the delete: {other_opens:?}"
    );
    assert!(
        answer.is_err(),
        "K6: Ok while the held roots directory no longer stands at its path: {}",
        answer_line(&answer)
    );
    let report = delete_report(&answer);
    assert_eq!(
        report.root,
        RootReading::Destroyed,
        "{}",
        answer_line(&answer)
    );
    assert!(
        !report.roots.at_its_path && report.roots.listed,
        "{}",
        answer_line(&answer)
    );
}

#[test]
fn a_roots_directory_moved_while_a_store_holds_it_loses_the_root_where_it_lies_and_the_delete_says_so(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = wrapped_of(&store_dir);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let moved = dir.path().join("roots-moved");
    std::fs::rename(&roots_dir, &moved).unwrap();

    let answer = store.delete_all();
    let again = store.delete_all();
    drop(store);

    let moved_root_unwraps = unwraps_under(&moved, root_id, &wrapped);
    println!(
        "MOVED-ROOTS {} again={} moved_root_unwraps={moved_root_unwraps} nothing_at_the_old_name={}",
        answer_line(&answer),
        answer_line(&again),
        !roots_dir.exists()
    );
    assert!(
        !moved_root_unwraps,
        "the root in the moved roots directory still unwraps: {}",
        answer_line(&answer)
    );
    assert!(
        answer.is_err() && again.is_err(),
        "Ok while the held roots directory no longer stands at its path: {} then {}",
        answer_line(&answer),
        answer_line(&again)
    );
    assert!(
        !roots_dir.exists(),
        "the delete made a roots directory at the old name"
    );
    let report = delete_report(&answer);
    assert_eq!(
        report.root,
        RootReading::Destroyed,
        "{}",
        answer_line(&answer)
    );
    assert!(!report.roots.at_its_path, "{}", answer_line(&answer));
    assert!(report.key_file_gone && !report.database_left && report.files_left == 0);
}

#[test]
fn a_root_file_renamed_inside_the_held_roots_directory_is_destroyed_where_it_lies() {
    for aside in [
        "kept-aside".to_string(),
        format!("{}.root", RootId::fresh().unwrap()),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, segment) = file_root_store(dir.path());
        let copy = dir.path().join("copy-of-the-store");
        std::fs::create_dir(&copy).unwrap();
        for name in ["store.key", "store.db"] {
            std::fs::copy(store_dir.join(name), copy.join(name)).unwrap();
        }
        let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
        let root_file = roots_dir.join(format!("{root_id}.root"));
        let aside_path = roots_dir.join(&aside);
        std::fs::rename(&root_file, &aside_path).unwrap();

        let answer = store.delete_all();
        drop(store);

        let destroyed_where_it_lies = no_root_bytes_at(&aside_path);
        if aside_path.exists() {
            std::fs::rename(&aside_path, &root_file).unwrap();
        }
        let copy_reads = open_over_file_root(&copy, &roots_dir, root_id)
            .map(|store| store.read_segment(segment, &mut Vec::new()).is_ok());
        println!(
            "RENAMED-INSIDE aside_is_a_root_name={} {} destroyed_where_it_lies={destroyed_where_it_lies} \
             copy_reads={copy_reads:?}",
            aside.ends_with(".root"),
            answer_line(&answer)
        );
        assert!(
            destroyed_where_it_lies,
            "the root file renamed inside the held roots directory still holds its bytes: {}",
            answer_line(&answer)
        );
        assert!(
            !matches!(copy_reads, Ok(true)),
            "a copy of the store still reads with the renamed root file"
        );
        assert!(answer.is_ok(), "{}", answer_line(&answer));
        assert_eq!(delete_report(&answer).root, RootReading::Destroyed);
    }
}

fn identity_json(path: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(path).unwrap();
    format!("{{\"volume\":{},\"file\":{}}}", meta.dev(), meta.ino())
}

fn write_made_mark(store_dir: &Path, root_id: RootId, root_file: Option<&Path>) {
    let identity = root_file.map_or_else(String::new, |path| {
        format!(",\"root_file\":{}", identity_json(path))
    });
    std::fs::write(
        store_dir.join("create.pending"),
        format!(
            "{{\"custody_kind\":\"dpapi\",\"root_id\":\"{root_id}\",\"reading\":\"made\"{identity}}}"
        ),
    )
    .unwrap();
}

/// A create in `dir` stopped after the second write of its mark and before
/// its key file, as a process that stops there.
fn a_create_stopped_after_its_made_mark(dir: &Path) -> (PathBuf, PathBuf, RootId) {
    let (store_dir, roots_dir) = (dir.join("store"), dir.join("roots"));
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _seam = crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT, || {
            panic!("STOP the create stops after its made mark, before its key file")
        });
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION).map(drop)
    }));
    assert!(stopped.is_err(), "the create did not stop at its seam");
    assert!(!store_dir.join("store.key").exists());
    assert!(roots_dir.join(format!("{root_id}.root")).exists());
    (store_dir, roots_dir, root_id)
}

/// The root file of a stopped create moved out of the roots directory, and a
/// file of another identity, of the root file's length, put at its name.
fn replace_the_root_file(dir: &Path, roots_dir: &Path, root_id: RootId) -> PathBuf {
    let root_file = roots_dir.join(format!("{root_id}.root"));
    std::fs::rename(&root_file, dir.join("moved-out-of-the-roots")).unwrap();
    std::fs::write(&root_file, [0x44u8; 32]).unwrap();
    root_file
}

#[test]
fn an_opens_cleanup_leaves_the_root_that_store_key_names_under_the_name_of_the_marks_root() {
    for mark in ["no identity", "the identity of the live root file"] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
        let root_id = RootId::fresh().unwrap();
        let mut store = EncryptedStore::create(
            &store_dir,
            Box::new(DevFallbackCustody::new(root_id, roots_dir.clone())),
            DEFAULT_RETENTION,
        )
        .unwrap();
        let (segment, _) = store
            .write_segment(&mut Cursor::new(b"typed errors"))
            .unwrap();
        drop(store);
        let marked = RootId::fresh().unwrap();
        let live = roots_dir.join(format!("{root_id}.root"));
        std::fs::hard_link(&live, roots_dir.join(format!("{marked}.root"))).unwrap();
        write_made_mark(
            &store_dir,
            marked,
            (mark != "no identity").then_some(live.as_path()),
        );

        let opened = EncryptedStore::open(&store_dir, &roots_dir, DEFAULT_RETENTION).map(drop);
        let reads = EncryptedStore::open(&store_dir, &roots_dir, DEFAULT_RETENTION).map(|store| {
            store
                .read_segment(segment.segment_id, &mut Vec::new())
                .is_ok()
        });
        println!("PERSON14 mark={mark:?} first_open={opened:?} reopen_reads={reads:?}");
        assert!(opened.is_ok(), "{mark}: {opened:?}");
        assert!(
            matches!(reads, Ok(true)),
            "{mark}: the open's cleanup destroyed the root that store.key names, which stood \
             under the name of the mark's root: {reads:?}"
        );
    }
}

#[test]
fn a_made_mark_records_the_identity_of_the_root_file_that_the_create_made() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_create_stopped_after_its_made_mark(dir.path());
    let mark: serde_json::Value =
        serde_json::from_slice(&std::fs::read(store_dir.join("create.pending")).unwrap()).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&identity_json(&roots_dir.join(format!("{root_id}.root")))).unwrap();
    println!(
        "MARK reading={} records_the_root_file={}",
        mark["reading"],
        mark["root_file"] == expected
    );
    assert_eq!(mark["reading"], "made");
    assert_eq!(
        mark["root_file"], expected,
        "the made mark records no identity of its root file"
    );
}

#[test]
fn a_tail_without_the_lock_leaves_a_file_of_another_identity_at_the_marks_root_name_and_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_create_stopped_after_its_made_mark(dir.path());
    let root_file = replace_the_root_file(dir.path(), &roots_dir, root_id);
    crate::store::failpoint::arm("tail_lock");

    let answer = tail(&store_dir, &roots_dir);

    let stands = std::fs::read(&root_file).ok().as_deref() == Some(&[0x44u8; 32][..]);
    let report = delete_report(&answer);
    let named = report.roots.names.contains(&format!("{root_id}.root"));
    println!(
        "MARK-TAIL-NO-LOCK {} other_identity_stands={stands} named={named}",
        answer_line(&answer)
    );
    assert!(
        stands,
        "the mark destroyed a file of another identity at its root's name: {}",
        answer_line(&answer)
    );
    assert!(answer.is_err() && named, "{}", answer_line(&answer));
    assert_eq!(report.root, RootReading::Left(Some(root_id)));
}

#[test]
fn a_tail_reads_the_marks_root_left_when_only_a_file_of_another_identity_stands_at_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_create_stopped_after_its_made_mark(dir.path());
    replace_the_root_file(dir.path(), &roots_dir, root_id);

    let answer = tail(&store_dir, &roots_dir);
    let next = tail(&store_dir, &roots_dir);

    println!(
        "MARK-TAIL-SWEEP {} next={}",
        answer_line(&answer),
        answer_line(&next)
    );
    assert!(
        answer.is_err(),
        "Ok over the mark's root file, which was moved out of the roots directory: {}",
        answer_line(&answer)
    );
    assert_eq!(
        delete_report(&answer).root,
        RootReading::Left(Some(root_id))
    );
    assert!(
        root_files_in(&roots_dir).is_empty(),
        "the sweep left a root file"
    );
    assert!(next.is_ok(), "{}", answer_line(&next));
    assert_eq!(delete_report(&next).root, RootReading::Unnamed);
}

#[test]
fn a_creates_cleanup_by_a_made_mark_leaves_a_file_of_another_identity_at_the_marks_root_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_create_stopped_after_its_made_mark(dir.path());
    let root_file = replace_the_root_file(dir.path(), &roots_dir, root_id);
    let next_id = RootId::fresh().unwrap();

    let created = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            next_id,
        )),
        DEFAULT_RETENTION,
    )
    .map(drop);

    let stands = std::fs::read(&root_file).ok().as_deref() == Some(&[0x44u8; 32][..]);
    println!("MARK-CREATE created={created:?} other_identity_stands={stands}");
    assert!(created.is_ok(), "{created:?}");
    assert!(
        stands,
        "the create's cleanup by the mark destroyed a file of another identity at its root's name"
    );
}

/// A scratch store in `<parent>/store`, its roots inside it, with one segment.
fn a_scratch_store(parent: &Path) -> (PathBuf, RootId, uuid::Uuid) {
    std::fs::create_dir_all(parent).unwrap();
    let store_dir = parent.join("store");
    let root_id = RootId::fresh().unwrap();
    let mut store = EncryptedStore::create(
        &store_dir,
        Box::new(DevFallbackCustody::new(root_id, store_dir.join("roots"))),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (segment, _) = store
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    (store_dir, root_id, segment.segment_id)
}

/// ST-46 K10, an unrun race: the store directory moved away and a link to
/// another store put at its name between the tail's look at the store and
/// its sweep. The tail opens the roots from the store directory's handle,
/// so it sweeps the roots of the directory that it looked at, and the other
/// store opens and reads after it.
#[test]
fn a_link_put_at_the_store_name_after_the_tails_look_leads_its_sweep_nowhere_else() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, root_id, _) = a_scratch_store(&dir.path().join("first"));
    let (other_dir, other_id, other_segment) = a_scratch_store(&dir.path().join("second"));
    let moved = dir.path().join("first").join("moved");
    let (from, to, link_to) = (store_dir.clone(), moved.clone(), other_dir.clone());
    let _seam = crate::store::failpoint::at(
        crate::store::db::SEAM_BEFORE_THE_TAILS_ROOT_STEP,
        move || {
            std::fs::rename(&from, &to).unwrap();
            std::os::unix::fs::symlink(&link_to, &from).unwrap();
        },
    );

    let answer = tail(&store_dir, &store_dir.join("roots"));

    let other_reads = EncryptedStore::open(&other_dir, &other_dir.join("roots"), DEFAULT_RETENTION)
        .map(|store| store.read_segment(other_segment, &mut Vec::new()).is_ok());
    let own_root_left = moved.join("roots").join(format!("{root_id}.root")).exists();
    println!(
        "K10 LINK-AT-THE-STORE-NAME {} other_store_reads={other_reads:?} own_root_left={own_root_left}",
        answer_line(&answer)
    );
    assert!(
        matches!(other_reads, Ok(true)),
        "the tail swept the roots of another store through a link put at its store name: \
         {other_reads:?}; {}",
        answer_line(&answer)
    );
    assert!(
        !own_root_left,
        "the tail left the root of the store that it looked at"
    );
    assert!(
        root_files_in(&other_dir.join("roots")).contains(&format!("{other_id}.root")),
        "the other store's root file is gone"
    );
}

/// The verifier's copy L30 (ruling ST-46 K4 (g)): an abandoned create whose
/// root file cannot be destroyed (here the user's immutable flag, set after
/// the root was made) answers an error and keeps its made mark, so that the
/// next cleanup finds the root that stays.
#[cfg(target_os = "macos")]
#[test]
fn an_abandoned_create_whose_root_cannot_be_destroyed_keeps_its_made_mark() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let immutable = move |path: &Path, on: bool| {
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let flags = if on { libc::UF_IMMUTABLE } else { 0 };
        assert_eq!(unsafe { libc::chflags(c.as_ptr(), flags) }, 0);
    };
    let at_the_seam = root_file.clone();
    let _seam = crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT, move || {
        immutable(&at_the_seam, true)
    });
    crate::store::failpoint::arm("create_after_the_key_file");
    let created = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )),
        DEFAULT_RETENTION,
    )
    .map(drop);
    immutable(&root_file, false);
    let mark: Option<serde_json::Value> = std::fs::read(store_dir.join("create.pending"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    println!(
        "L30 create={created:?} root_stands={} mark={}",
        root_file.exists(),
        mark.as_ref()
            .map_or("none".to_string(), |m| m["reading"].to_string())
    );
    assert!(created.is_err(), "the create answered Ok");
    assert!(root_file.exists(), "the root file did not stay");
    assert_eq!(
        mark.as_ref().map(|m| m["reading"].clone()),
        Some(serde_json::Value::from("made")),
        "the abandoned create removed its made mark beside a root that stays"
    );
}

#[test]
fn delete_all_removes_a_short_file_at_the_roots_name_unwritten() {
    use std::io::{Read, Seek, SeekFrom};
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let own_name = roots_dir.join(format!("{root_id}.root"));
    let moved = roots_dir.join("moved-inside-the-roots");
    std::fs::rename(&own_name, &moved).unwrap();
    std::fs::write(&own_name, [0x77u8; 100]).unwrap();
    let mut planted = std::fs::File::open(&own_name).unwrap();

    let answer = store.delete_all();
    drop(store);

    let mut bytes = Vec::new();
    planted.seek(SeekFrom::Start(0)).unwrap();
    planted.read_to_end(&mut bytes).unwrap();
    let planted_unwritten = bytes.len() == 100 && bytes.iter().all(|&b| b == 0x77);
    let planted_zeroed = bytes.len() == 100 && bytes.iter().all(|&b| b == 0);
    let moved_root_gone = no_root_bytes_at(&moved);
    println!(
        "REVIEW-H1H5-01 {} moved_root_gone={moved_root_gone} planted_unwritten={planted_unwritten} \
         planted_zeroed={planted_zeroed}",
        answer_line(&answer)
    );
    assert!(
        moved_root_gone,
        "the held root file, moved inside the roots directory, kept its bytes: {}",
        answer_line(&answer)
    );
    assert!(
        planted_unwritten,
        "H4: a 100-byte file at the root's own name, which cannot be the root's file, was \
         written through (zero-filled) by delete_all once the identity was known, instead of \
         removed as the entry that it is: {}",
        answer_line(&answer)
    );
}

#[test]
fn the_tail_removes_a_short_file_at_the_marks_root_name_unwritten() {
    use std::io::{Read, Seek, SeekFrom};
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_create_stopped_after_its_made_mark(dir.path());
    let own_name = roots_dir.join(format!("{root_id}.root"));
    let moved = roots_dir.join("moved-inside-the-roots");
    std::fs::rename(&own_name, &moved).unwrap();
    std::fs::write(&own_name, [0x77u8; 100]).unwrap();
    let mut planted = std::fs::File::open(&own_name).unwrap();

    let answer = tail(&store_dir, &roots_dir);

    let mut bytes = Vec::new();
    planted.seek(SeekFrom::Start(0)).unwrap();
    planted.read_to_end(&mut bytes).unwrap();
    let planted_unwritten = bytes.len() == 100 && bytes.iter().all(|&b| b == 0x77);
    let planted_zeroed = bytes.len() == 100 && bytes.iter().all(|&b| b == 0);
    let moved_root_gone = no_root_bytes_at(&moved);
    println!(
        "REVIEW-H1H5-02 {} moved_root_gone={moved_root_gone} planted_unwritten={planted_unwritten} \
         planted_zeroed={planted_zeroed}",
        answer_line(&answer)
    );
    assert!(
        moved_root_gone,
        "the mark's root file, moved inside the roots directory, kept its bytes: {}",
        answer_line(&answer)
    );
    assert!(
        planted_unwritten,
        "H4: the tail's sweep by a made mark zero-filled a 100-byte file at the mark's root \
         name, which cannot be the root's file, instead of removing it unwritten: {}",
        answer_line(&answer)
    );
}

#[test]
fn an_abandoned_create_beside_unlistable_roots_keeps_what_names_its_root() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let at_the_seam = roots_dir.clone();
    let created = {
        let _seam = crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT, move || {
            std::fs::set_permissions(&at_the_seam, std::fs::Permissions::from_mode(0o300)).unwrap()
        });
        crate::store::failpoint::arm("create_after_the_key_file");
        EncryptedStore::create(
            &store_dir,
            Box::new(file_root_custody(
                &roots_dir,
                TestProtector::new(0x5a).0,
                root_id,
            )),
            DEFAULT_RETENTION,
        )
        .map(drop)
    };
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    let root_bytes_stay = !no_root_bytes_at(&root_file);
    let mark: Option<serde_json::Value> = std::fs::read(store_dir.join("create.pending"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let mark_reads_made =
        mark.as_ref().map(|m| m["reading"].clone()) == Some(serde_json::Value::from("made"));
    let key_file_stays = store_dir.join("store.key").exists();
    println!(
        "REVIEW-H1H5-03 create_is_err={} root_bytes_stay={root_bytes_stay} \
         mark_reads_made={mark_reads_made} key_file_stays={key_file_stays}",
        created.is_err()
    );
    assert!(
        created.is_err(),
        "the create answered Ok past its failpoint"
    );
    assert!(
        !root_bytes_stay || mark_reads_made || key_file_stays,
        "the abandoned create's cleanup by identity (destroy_root_through) answered Ok on a \
         roots directory that it could not list, so the made mark was removed while the root \
         file keeps its bytes and nothing names it"
    );
}

#[test]
fn no_mode_change_on_a_file_of_another_identity() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_create_stopped_after_its_made_mark(dir.path());
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let moved_inside = roots_dir.join("moved-inside");
    let other = dir.path().join("another-partys-file");
    std::fs::write(&other, [0x44u8; 32]).unwrap();
    std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o400)).unwrap();
    let ran = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let seam = {
        let (ran, root_file, moved_inside, other) = (
            ran.clone(),
            root_file.clone(),
            moved_inside.clone(),
            other.clone(),
        );
        crate::store::failpoint::at(
            crate::custody::file_root::SEAM_AFTER_THE_LISTING,
            move || {
                ran.set(ran.get() + 1);
                if ran.get() == 1 {
                    std::fs::rename(&root_file, &moved_inside).unwrap();
                    std::fs::hard_link(&other, &root_file).unwrap();
                }
            },
        )
    };
    crate::store::failpoint::arm("tail_lock");

    let answer = tail(&store_dir, &roots_dir);
    drop(seam);

    let mode = std::fs::metadata(&other).unwrap().permissions().mode() & 0o777;
    let bytes_kept = std::fs::read(&other).ok().as_deref() == Some(&[0x44u8; 32][..]);
    let own_destroyed = no_root_bytes_at(&moved_inside);
    println!(
        "IDENT01 seam_runs={} {} other_mode={mode:o} other_bytes_kept={bytes_kept} \
         own_root_destroyed={own_destroyed}",
        ran.get(),
        answer_line(&answer)
    );
    assert_eq!(ran.get(), 1, "the root step's seam did not run once");
    assert!(
        own_destroyed,
        "the store's own root file, moved inside, kept its bytes"
    );
    assert!(
        bytes_kept,
        "the root step wrote into a file of another identity"
    );
    assert_eq!(
        mode, 0o400,
        "the root step set the mode of a file of another identity, which it met at the root's \
         name after its listing, before it compared the identity"
    );
}

#[test]
fn an_opens_cleanup_beside_an_unlistable_held_directory_keeps_its_mark() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let marked = RootId::fresh().unwrap();
    let marked_file = roots_dir.join(format!("{marked}.root"));
    std::fs::write(&marked_file, [0x77u8; 32]).unwrap();
    write_made_mark(&store_dir, marked, Some(marked_file.as_path()));
    let ran = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let seam = {
        let (ran, roots) = (ran.clone(), roots_dir.clone());
        crate::store::failpoint::at(crate::store::db::SEAM_BEFORE_THE_FIRST_READ, move || {
            ran.set(ran.get() + 1);
            std::fs::set_permissions(&roots, std::fs::Permissions::from_mode(0o300)).unwrap();
        })
    };

    let opened = open_over_file_root(&store_dir, &roots_dir, root_id).map(drop);
    drop(seam);
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mark_stays = store_dir.join("create.pending").exists();
    let marked_bytes_stay = std::fs::read(&marked_file).ok().as_deref() == Some(&[0x77u8; 32][..]);
    println!(
        "IDENT02 seam_runs={} open={opened:?} mark_stays={mark_stays} \
         marked_root_keeps_its_bytes={marked_bytes_stay}",
        ran.get()
    );
    assert!(ran.get() >= 1, "the database open's seam did not run");
    assert!(
        mark_stays || !marked_bytes_stay,
        "the open's cleanup removed the made mark while the file of the mark's identity keeps its \
         bytes: its listing of the held roots directory failed and the cleanup answered Ok"
    );
}

#[test]
fn an_abandoned_create_with_an_unlistable_held_directory_keeps_its_mark() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let ran = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let seam = {
        let (ran, roots) = (ran.clone(), roots_dir.clone());
        crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT, move || {
            ran.set(ran.get() + 1);
            std::fs::set_permissions(&roots, std::fs::Permissions::from_mode(0o300)).unwrap();
        })
    };
    crate::store::failpoint::arm("create_after_the_key_file");

    let created = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )),
        DEFAULT_RETENTION,
    )
    .map(drop);
    drop(seam);
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    let mark: Option<serde_json::Value> = std::fs::read(store_dir.join("create.pending"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let root_keeps_its_bytes = !no_root_bytes_at(&root_file);
    println!(
        "IDENT03 seam_runs={} create={created:?} root_keeps_its_bytes={root_keeps_its_bytes} mark={}",
        ran.get(),
        mark.as_ref()
            .map_or("none".to_string(), |m| m["reading"].to_string())
    );
    assert_eq!(
        ran.get(),
        1,
        "the create's seam after its root did not run once"
    );
    assert!(created.is_err(), "the create answered Ok");
    assert!(
        !root_keeps_its_bytes
            || mark.as_ref().map(|m| m["reading"].clone()) == Some(serde_json::Value::from("made")),
        "the abandoned create removed its made mark while its root file keeps its bytes: the \
         listing of the held roots directory failed and the cleanup by identity answered Ok"
    );
}

/// The review's F1: a create whose look met an entry at the root's name of
/// its id, an entry that is gone before the create picks its answer, answers
/// RootExists and publishes no root that nothing names.
#[test]
fn a_create_refused_at_a_root_that_leaves_before_its_answer_publishes_no_root() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
    std::fs::create_dir_all(&roots_dir).unwrap();
    let root_file = roots_dir.join(format!("{root_id}.root"));
    std::fs::write(&root_file, [0x21u8; 32]).unwrap();
    let gone = root_file.clone();
    let _seam = crate::store::failpoint::at(crate::store::db::SEAM_IN_THE_REFUSAL_ARM, move || {
        std::fs::rename(&gone, gone.with_extension("taken-away")).unwrap();
    });
    let created = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )),
        DEFAULT_RETENTION,
    )
    .map(drop);
    let published = root_files_in(&roots_dir);
    println!("F1 create={created:?} root_files_after={}", published.len());
    assert!(
        matches!(
            created,
            Err(StoreError::Custody(
                crate::custody::CustodyError::RootExists
            ))
        ),
        "{created:?}"
    );
    assert_eq!(
        published,
        Vec::<String>::new(),
        "the refused create published a root that nothing names"
    );
}

/// store-3b: a data directory that is a link is not followed: the tail
/// removes nothing, counts it as not followed and answers DeleteIncomplete,
/// and the store behind the link opens and reads after it.
#[test]
fn a_data_directory_that_is_a_link_is_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    let (store_dir, roots_dir, root_id, segment) = file_root_store(&real);
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let answer = EncryptedStore::delete_data_dir(&link, crate::store::Layout::DataDirectory);

    let reads = open_over_file_root(&store_dir, &roots_dir, root_id)
        .map(|store| store.read_segment(segment, &mut Vec::new()).is_ok());
    let link_stands =
        std::fs::symlink_metadata(&link).is_ok_and(|meta| meta.file_type().is_symlink());
    println!(
        "DATA-DIR-LINK {} store_behind_reads={reads:?} link_stands={link_stands}",
        answer_line(&answer)
    );
    assert!(matches!(reads, Ok(true)), "{reads:?}");
    assert!(
        link_stands,
        "the tail removed the link at the data directory"
    );
    assert!(answer.is_err(), "{}", answer_line(&answer));
    assert_eq!(delete_report(&answer).not_followed, 1);
}

/// store-3b: the tail on a data directory at whose name nothing stands, in
/// both layouts, answers Ok with nothing named and the roots read as listed
/// and empty, and makes nothing at that name.
#[test]
fn a_tail_on_a_data_directory_that_does_not_exist_answers_ok_with_nothing_named() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("never-made");
    for layout in [
        crate::store::Layout::DataDirectory,
        crate::store::Layout::ScratchStore,
    ] {
        let answer = EncryptedStore::delete_data_dir(&absent, layout);
        println!("ABSENT-DATA-DIR layout={layout:?} {}", answer_line(&answer));
        assert!(answer.is_ok(), "{layout:?}: {}", answer_line(&answer));
        let report = delete_report(&answer);
        assert_eq!(report.root, RootReading::Unnamed);
        assert!(report.roots.listed && report.roots.names.is_empty());
        assert_eq!(report.not_followed, 0);
        assert!(
            !absent.exists(),
            "{layout:?}: the tail made something at the name"
        );
    }
}

/// store-3b: the open holds its roots directory following no link at its
/// last part: a link at the roots path is refused with RootCorrupt, and the
/// root behind it is not read.
#[test]
fn an_open_whose_roots_path_is_a_link_is_refused_and_reads_no_root_behind_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let behind = dir.path().join("roots-behind");
    std::fs::rename(&roots_dir, &behind).unwrap();
    std::os::unix::fs::symlink(&behind, &roots_dir).unwrap();
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id).map(drop);
    println!("ROOTS-LINK open={opened:?}");
    assert!(
        matches!(
            opened,
            Err(StoreError::Custody(
                crate::custody::CustodyError::RootCorrupt
            ))
        ),
        "an open held a roots directory through a link: {opened:?}"
    );
}

/// store-3b: abandon_create destroys its root through the roots directory
/// that the create holds: with that directory moved after the root was made,
/// the root in the moved directory is destroyed and nothing is made at the
/// old path.
#[test]
fn an_abandoned_create_destroys_its_root_in_the_held_roots_directory_after_it_moved() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
    let moved = dir.path().join("roots-moved");
    let (from, to) = (roots_dir.clone(), moved.clone());
    let _seam = crate::store::failpoint::at(crate::store::db::SEAM_AFTER_THE_ROOT, move || {
        std::fs::rename(&from, &to).unwrap()
    });
    crate::store::failpoint::arm("create_after_the_key_file");
    let created = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            root_id,
        )),
        DEFAULT_RETENTION,
    )
    .map(drop);
    let destroyed = no_root_bytes_at(&moved.join(format!("{root_id}.root")));
    println!(
        "ABANDON-HELD create={created:?} moved_root_destroyed={destroyed} old_path_exists={}",
        roots_dir.exists()
    );
    assert!(created.is_err(), "the create answered Ok");
    assert!(
        destroyed,
        "the abandoned create left its root in the roots directory that it held"
    );
    assert!(
        !roots_dir.exists(),
        "the abandoned create made a roots directory at the old path"
    );
}

/// store-3b: the tail without the lock, on a stopped create whose made mark
/// names the root by its identity, with that root file renamed inside the
/// roots directory and a file of another identity put at its name: the step
/// destroys the mark's root where it lies and leaves the other file, which it
/// names.
#[test]
fn a_tail_without_the_lock_destroys_the_marks_root_where_it_lies_and_leaves_another_identity_at_its_name(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_create_stopped_after_its_made_mark(dir.path());
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let aside = roots_dir.join("kept-aside");
    std::fs::rename(&root_file, &aside).unwrap();
    std::fs::write(&root_file, [0x44u8; 32]).unwrap();
    crate::store::failpoint::arm("tail_lock");

    let answer = tail(&store_dir, &roots_dir);

    let other_stands = std::fs::read(&root_file).ok().as_deref() == Some(&[0x44u8; 32][..]);
    let destroyed_where_it_lies = no_root_bytes_at(&aside);
    println!(
        "MARK-TAIL-NO-LOCK-WITNESS {} other_identity_stands={other_stands} \
         destroyed_where_it_lies={destroyed_where_it_lies}",
        answer_line(&answer)
    );
    assert!(destroyed_where_it_lies, "{}", answer_line(&answer));
    assert!(
        other_stands,
        "the tail without the lock destroyed a file of another identity at the mark's root name"
    );
    assert!(answer.is_err(), "{}", answer_line(&answer));
    assert!(delete_report(&answer)
        .roots
        .names
        .contains(&format!("{root_id}.root")));
}

/// Whether the bytes now at `file`, put as the root file of `root_id` in a
/// fresh directory, unwrap `wrapped`.
fn st47_unwraps(
    scratch: &Path,
    tag: &str,
    file: &Path,
    root_id: RootId,
    wrapped: &WrappedKey,
) -> bool {
    let Ok(bytes) = std::fs::read(file) else {
        return false;
    };
    let check = scratch.join(format!("check-{tag}"));
    std::fs::create_dir_all(&check).unwrap();
    std::fs::write(check.join(format!("{root_id}.root")), bytes).unwrap();
    unwraps_under(&check, root_id, wrapped)
}

#[derive(Clone, Copy, Debug)]
enum St47When {
    HeldAndThatHandleDeletes,
    HeldThenDropped,
    NoStoreOpen,
}

/// One of the verify step's v21 cases (ruling ST-47 T1): the root file renamed
/// inside the roots folder, with or without a copy put at its name, with the
/// store open during the rename or not, then two deletes of the handle that
/// the case has and two tails. No answer is Ok while the renamed file
/// unwraps, after an Ok it does not unwrap, and while it stands the report
/// names it.
fn st47_renamed_inside(round: usize, when: St47When, copy_at_the_name: bool) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = wrapped_of(&store_dir);
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let aside = roots_dir.join("kept-aside");
    let tag = format!("r{round} {when:?} copy={copy_at_the_name}");
    let mut problems = Vec::new();
    let mut n = 0;
    let mut look = |what: &str, answer: &StoreResult<crate::store::DeleteReport>| {
        n += 1;
        let unwraps = st47_unwraps(dir.path(), &format!("{n}"), &aside, root_id, &wrapped);
        let stands = aside.exists();
        println!(
            "ST47-T1 {tag} {what}: {} aside_stands={stands} aside_unwraps={unwraps}",
            answer_line(answer)
        );
        if answer.is_ok() && unwraps {
            problems.push(format!("{tag} {what}: Ok while the renamed root unwraps"));
        }
        let counted = match answer {
            Ok(report) => report.roots.others_left.root_shaped > 0,
            Err(StoreError::DeleteIncomplete(report)) => report.roots.others_left.root_shaped > 0,
            Err(_) => true,
        };
        if stands && !counted {
            problems.push(format!(
                "{tag} {what}: the renamed file stands and the report does not count it (ruling ST-48 U3)"
            ));
        }
    };
    let rename = || {
        std::fs::rename(&root_file, &aside).unwrap();
        if copy_at_the_name {
            std::fs::copy(&aside, &root_file).unwrap();
        }
    };
    match when {
        St47When::HeldAndThatHandleDeletes => {
            let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
            rename();
            let first = store.delete_all();
            look("held delete_all 1", &first);
            let second = store.delete_all();
            look("held delete_all 2", &second);
            drop(store);
        }
        St47When::HeldThenDropped | St47When::NoStoreOpen => {
            if let St47When::HeldThenDropped = when {
                let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
                rename();
                drop(store);
            } else {
                rename();
            }
            if let Ok(mut store) = open_over_file_root(&store_dir, &roots_dir, root_id) {
                let first = store.delete_all();
                look("new open's delete_all 1", &first);
                let second = store.delete_all();
                look("new open's delete_all 2", &second);
                drop(store);
            }
        }
    }
    let first = tail(&store_dir, &roots_dir);
    look("tail 1", &first);
    let second = tail(&store_dir, &roots_dir);
    look("tail 2", &second);
    problems
}

#[test]
fn st47_t1_no_delete_answers_ok_while_a_root_renamed_inside_the_roots_folder_unwraps() {
    let mut problems = Vec::new();
    for round in 1..=2 {
        for when in [
            St47When::HeldAndThatHandleDeletes,
            St47When::HeldThenDropped,
            St47When::NoStoreOpen,
        ] {
            for copy_at_the_name in [false, true] {
                problems.extend(st47_renamed_inside(round, when, copy_at_the_name));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn st47_t1_the_sweep_removes_a_hard_link_of_an_outside_file_at_a_plain_name_without_writing_through_it(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    let outside = dir.path().join("a-file-of-another-party");
    std::fs::write(&outside, [0x5bu8; 32]).unwrap();
    let mode_before = st48_mode(&outside);
    let plain = roots_dir.join("plain");
    std::fs::hard_link(&outside, &plain).unwrap();

    let answer = tail(&store_dir, &roots_dir);

    let outside_kept = std::fs::read(&outside).ok().as_deref() == Some(&[0x5bu8; 32][..]);
    let mode_kept = st48_mode(&outside) == mode_before;
    println!(
        "ST47-T1-LINK {} plain_stands={} outside_kept={outside_kept} outside_mode_kept={mode_kept}",
        answer_line(&answer),
        plain.exists()
    );
    assert!(
        outside_kept,
        "the sweep wrote through a second name of another party's file"
    );
    assert!(
        mode_kept,
        "the sweep changed the mode of another party's file (ruling ST-48 U1, the review's N4)"
    );
    assert!(
        !plain.exists(),
        "the sweep left a file at a plain name of the roots folder"
    );
    assert!(answer.is_ok(), "{}", answer_line(&answer));
}

#[test]
fn st47_t1_a_root_moved_into_a_directory_of_the_roots_folder_keeps_every_delete_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let wrapped = wrapped_of(&store_dir);
    let sub = roots_dir.join("sub");
    std::fs::create_dir(&sub).unwrap();
    let moved = sub.join(format!("{root_id}.root"));
    std::fs::rename(roots_dir.join(format!("{root_id}.root")), &moved).unwrap();

    let first = tail(&store_dir, &roots_dir);
    let second = tail(&store_dir, &roots_dir);

    let unwraps = st47_unwraps(dir.path(), "sub", &moved, root_id, &wrapped);
    let named = |answer: &StoreResult<crate::store::DeleteReport>| match answer {
        Err(StoreError::DeleteIncomplete(report)) => report.roots.others_left.directories == 1,
        _ => false,
    };
    println!(
        "ST47-T1-DIR first={} second={} moved_root_unwraps={unwraps} sub_stands={}",
        answer_line(&first),
        answer_line(&second),
        sub.exists()
    );
    assert!(
        sub.exists(),
        "the sweep entered or removed a directory of the roots folder"
    );
    assert!(
        named(&first) && named(&second),
        "a tail answered other than DeleteIncomplete naming the directory while it stands"
    );
}

/// A scratch store at `<parent>/real` made over a file-root custody whose
/// roots lie inside it, with one segment, and a link at `<parent>/link` to it.
fn st47_a_scratch_store_behind_a_link(
    parent: &Path,
) -> (PathBuf, PathBuf, PathBuf, RootId, uuid::Uuid) {
    let real = parent.join("real");
    let roots = real.join("roots");
    let root_id = RootId::fresh().unwrap();
    let mut store = EncryptedStore::create(
        &real,
        Box::new(file_root_custody(
            &roots,
            TestProtector::new(0x5a).0,
            root_id,
        )),
        DEFAULT_RETENTION,
    )
    .unwrap();
    let (segment, _) = store
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(store);
    let link = parent.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    (real, roots, link, root_id, segment.segment_id)
}

/// Ruling ST-47 T2 (adversary 1's T14): a store path that is a link, under
/// both closed layouts. Each tail answers DeleteIncomplete with not_followed
/// 1, leaves the link standing, and the store behind the link opens and
/// reads after it.
#[test]
fn st47_t2_a_store_path_that_is_a_link_is_refused_in_both_layouts() {
    let mut problems = Vec::new();
    for layout in [
        crate::store::Layout::ScratchStore,
        crate::store::Layout::DataDirectory,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (real, roots, link, root_id, segment) = st47_a_scratch_store_behind_a_link(dir.path());

        let answer = EncryptedStore::delete_data_dir(&link, layout);

        let reads = open_over_file_root(&real, &roots, root_id)
            .map(|store| store.read_segment(segment, &mut Vec::new()).is_ok());
        let link_stands =
            std::fs::symlink_metadata(&link).is_ok_and(|meta| meta.file_type().is_symlink());
        let (incomplete, not_followed) = match &answer {
            Ok(report) => (false, report.not_followed),
            Err(StoreError::DeleteIncomplete(report)) => (true, report.not_followed),
            Err(_) => (false, u64::MAX),
        };
        println!(
            "ST47-T2-T14 layout={layout:?} {} not_followed={not_followed} link_stands={link_stands} store_behind_reads={reads:?}",
            answer_line(&answer)
        );
        if !matches!(reads, Ok(true)) {
            problems.push(format!(
                "{layout:?}: the store behind the link no longer reads: {reads:?}"
            ));
        }
        if !link_stands {
            problems.push(format!("{layout:?}: the tail removed the link"));
        }
        if !incomplete || not_followed != 1 {
            problems.push(format!(
                "{layout:?}: {} with not_followed={not_followed}, not DeleteIncomplete with 1",
                answer_line(&answer)
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Ruling ST-47 T2 (adversary 1's H1): the scratch tail on a store path that
/// is a link answers no Ok while the root behind the link unwraps the key,
/// and removes nothing: the link, the key file, the database and the root
/// file behind it stand.
#[test]
fn st47_t2_a_scratch_store_path_that_is_a_link_removes_nothing_and_answers_no_ok() {
    let dir = tempfile::tempdir().unwrap();
    let (real, roots, link, root_id, _) = st47_a_scratch_store_behind_a_link(dir.path());
    let wrapped = wrapped_of(&real);
    let root_file = roots.join(format!("{root_id}.root"));

    let answer = EncryptedStore::delete_data_dir(&link, crate::store::Layout::ScratchStore);

    let unwraps = st47_unwraps(dir.path(), "h1", &root_file, root_id, &wrapped);
    let link_stands = std::fs::symlink_metadata(&link).is_ok();
    let behind_stands = [
        real.join("store.key"),
        real.join("store.db"),
        root_file.clone(),
    ]
    .iter()
    .all(|path| path.exists());
    println!(
        "ST47-T2-H1 {} root_unwraps={unwraps} link_stands={link_stands} behind_stands={behind_stands}",
        answer_line(&answer)
    );
    assert!(
        !(answer.is_ok() && unwraps),
        "the tail answered Ok while the root behind the scratch link unwraps the key"
    );
    assert!(
        link_stands,
        "the scratch tail removed the link at the store path"
    );
    assert!(
        behind_stands,
        "the scratch tail removed a file behind the link"
    );
    assert_eq!(delete_report(&answer).not_followed, 1);
}

/// Ruling ST-47 T2, the control in the data layout: a link at `DIR/store`
/// is removed as the entry that it is, counted as not followed, the roots
/// directory of `DIR` is swept, and the tail answers Ok; the directory that
/// the link named keeps its files.
#[test]
fn st47_t2_control_a_link_at_the_store_name_of_a_data_directory_is_removed_and_the_roots_swept() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let (store_dir, roots_dir, root_id, _) = file_root_store(&data);
    let aside = dir.path().join("aside");
    std::fs::rename(&store_dir, &aside).unwrap();
    std::os::unix::fs::symlink(&aside, &store_dir).unwrap();

    let answer = EncryptedStore::delete_data_dir(&data, crate::store::Layout::DataDirectory);

    let link_stands = std::fs::symlink_metadata(&store_dir).is_ok();
    let root_stands = roots_dir.join(format!("{root_id}.root")).exists();
    let aside_kept = aside.join("store.key").exists() && aside.join("store.db").exists();
    println!(
        "ST47-T2-CONTROL {} link_stands={link_stands} root_stands={root_stands} aside_kept={aside_kept}",
        answer_line(&answer)
    );
    assert!(
        !link_stands,
        "the data tail left the link at the store name"
    );
    assert!(
        !root_stands,
        "the data tail left the root file in the roots directory"
    );
    assert!(
        aside_kept,
        "the data tail followed the link at the store name"
    );
    assert!(answer.is_ok(), "{}", answer_line(&answer));
    assert_eq!(delete_report(&answer).not_followed, 1);
}

/// The spellings of ruling ST-47 T3 that end a path in a separator: `p/`,
/// `p/.` and `p//`.
fn st47_spellings(path: &Path) -> Vec<(&'static str, PathBuf)> {
    ["/", "/.", "//"]
        .into_iter()
        .map(|tail| {
            let mut spelled = path.as_os_str().to_os_string();
            spelled.push(tail);
            (tail, PathBuf::from(spelled))
        })
        .collect()
}

/// The store's open with the roots path `roots` given to it and to the
/// file-root custody that it makes.
fn st47_open_at(store_dir: &Path, roots: &Path, root_id: RootId) -> StoreResult<EncryptedStore> {
    let custody_roots = roots.to_path_buf();
    EncryptedStore::open_with(
        store_dir,
        roots,
        DEFAULT_RETENTION,
        move |_: &crate::custody::CustodyLocator| {
            Ok(Box::new(file_root_custody(
                &custody_roots,
                TestProtector::new(0x5a).0,
                root_id,
            )))
        },
    )
}

/// Ruling ST-47 T3: the tail of either layout given a link spelled `link/`,
/// `link/.` or `link//` refuses it as it refuses `link`: DeleteIncomplete
/// with not_followed 1, the link left standing, and the store behind it
/// opens and reads after the call.
#[test]
fn st47_t3_the_tail_refuses_a_link_spelled_with_a_trailing_separator_in_both_layouts() {
    let mut problems = Vec::new();
    for layout in [
        crate::store::Layout::ScratchStore,
        crate::store::Layout::DataDirectory,
    ] {
        for spelling in ["/", "/.", "//"] {
            let dir = tempfile::tempdir().unwrap();
            let real = dir.path().join("real");
            let (store_dir, roots_dir, root_id, segment) = match layout {
                crate::store::Layout::ScratchStore => {
                    let (real, roots, _, root_id, segment) =
                        st47_a_scratch_store_behind_a_link(dir.path());
                    (real, roots, root_id, segment)
                }
                crate::store::Layout::DataDirectory => {
                    let made = file_root_store(&real);
                    std::os::unix::fs::symlink(&real, dir.path().join("link")).unwrap();
                    made
                }
            };
            let link = dir.path().join("link");
            let spelled = st47_spellings(&link)
                .into_iter()
                .find(|(tail, _)| *tail == spelling)
                .unwrap()
                .1;

            let answer = EncryptedStore::delete_data_dir(&spelled, layout);

            let reads = open_over_file_root(&store_dir, &roots_dir, root_id)
                .map(|store| store.read_segment(segment, &mut Vec::new()).is_ok());
            let link_stands =
                std::fs::symlink_metadata(&link).is_ok_and(|meta| meta.file_type().is_symlink());
            let (incomplete, not_followed) = match &answer {
                Ok(report) => (false, report.not_followed),
                Err(StoreError::DeleteIncomplete(report)) => (true, report.not_followed),
                Err(_) => (false, u64::MAX),
            };
            println!(
                "ST47-T3-TAIL layout={layout:?} spelling=link{spelling} {} not_followed={not_followed} link_stands={link_stands} store_behind_reads={reads:?}",
                answer_line(&answer)
            );
            if !(incomplete && not_followed == 1 && link_stands && matches!(reads, Ok(true))) {
                problems.push(format!(
                    "{layout:?} link{spelling}: {} not_followed={not_followed} link_stands={link_stands} store_behind_reads={reads:?}",
                    answer_line(&answer)
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Ruling ST-47 T3: the roots handle's open and the store's open given a
/// roots path that is a link spelled `link/`, `link/.` or `link//` refuse it
/// as they refuse `link`, and hold nothing behind it.
#[test]
fn st47_t3_the_roots_handles_open_refuses_a_link_spelled_with_a_trailing_separator() {
    let mut problems = Vec::new();
    for spelling in ["/", "/.", "//"] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        let behind = dir.path().join("roots-behind");
        std::fs::rename(&roots_dir, &behind).unwrap();
        let link = dir.path().join("roots-link");
        std::os::unix::fs::symlink(&behind, &link).unwrap();
        let spelled = st47_spellings(&link)
            .into_iter()
            .find(|(tail, _)| *tail == spelling)
            .unwrap()
            .1;

        let held = crate::custody::file_root::hold(&spelled).map(drop);
        let opened = st47_open_at(&store_dir, &spelled, root_id).map(drop);

        println!("ST47-T3-ROOTS spelling=link{spelling} hold={held:?} open={opened:?}");
        if !matches!(held, Err(crate::custody::CustodyError::RootCorrupt)) {
            problems.push(format!(
                "link{spelling}: the roots handle's open answered {held:?}"
            ));
        }
        if opened.is_ok() {
            problems.push(format!(
                "link{spelling}: the store opened over roots through a link"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Ruling ST-47 T3: the store's create and open given a store path that is a
/// link spelled `link/`, `link/.` or `link//` refuse it as they refuse
/// `link`: the create makes nothing behind the link and the open holds
/// nothing behind it.
#[test]
fn st47_t3_the_stores_create_and_open_refuse_a_link_spelled_with_a_trailing_separator() {
    let mut problems = Vec::new();
    for spelling in ["", "/", "/.", "//"] {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        let link_to_empty = dir.path().join("link-to-empty");
        std::os::unix::fs::symlink(&empty, &link_to_empty).unwrap();
        let mut spelled_create = link_to_empty.as_os_str().to_os_string();
        spelled_create.push(spelling);
        let created = EncryptedStore::create(
            Path::new(&spelled_create),
            Box::new(file_root_custody(
                &dir.path().join("roots-of-create"),
                TestProtector::new(0x5a).0,
                RootId::fresh().unwrap(),
            )),
            DEFAULT_RETENTION,
        )
        .map(drop);
        let made_behind = std::fs::read_dir(&empty).unwrap().count();

        let (real, roots, link, root_id, _) = st47_a_scratch_store_behind_a_link(dir.path());
        let mut spelled_open = link.as_os_str().to_os_string();
        spelled_open.push(spelling);
        let opened = st47_open_at(Path::new(&spelled_open), &roots, root_id).map(drop);
        let real_opens = st47_open_at(&real, &roots, root_id).map(drop);

        println!(
            "ST47-T3-STORE spelling=link{spelling} create={created:?} entries_made_behind={made_behind} open={opened:?} real_opens_after={real_opens:?}"
        );
        if created.is_ok() || made_behind != 0 {
            problems.push(format!(
                "link{spelling}: the create answered {created:?} and made {made_behind} entries behind the link"
            ));
        }
        if opened.is_ok() {
            problems.push(format!(
                "link{spelling}: the open held the store behind the link"
            ));
        }
        if real_opens.is_err() {
            problems.push(format!(
                "link{spelling}: the store behind the link no longer opens: {real_opens:?}"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Ruling ST-47 T3, the controls: a real folder spelled `realdir/` or
/// `realdir/.` answers as `realdir` does, for the tail of both layouts, the
/// roots handle's open and the store's create and open.
#[test]
fn st47_t3_control_a_real_folder_spelled_with_a_trailing_separator_answers_as_the_folder() {
    let mut problems = Vec::new();
    for spelling in ["", "/", "/."] {
        let dir = tempfile::tempdir().unwrap();
        let spell = |path: &Path| {
            let mut spelled = path.as_os_str().to_os_string();
            spelled.push(spelling);
            PathBuf::from(spelled)
        };

        let created_at = dir.path().join("created");
        std::fs::create_dir(&created_at).unwrap();
        let root_id = RootId::fresh().unwrap();
        let roots = dir.path().join("created-roots");
        let created = EncryptedStore::create(
            &spell(&created_at),
            Box::new(file_root_custody(
                &roots,
                TestProtector::new(0x5a).0,
                root_id,
            )),
            DEFAULT_RETENTION,
        )
        .map(drop);
        let held = crate::custody::file_root::hold(&spell(&roots)).map(drop);
        let opened = st47_open_at(&spell(&created_at), &spell(&roots), root_id).map(drop);

        let scratch_parent = dir.path().join("scratch");
        std::fs::create_dir(&scratch_parent).unwrap();
        let (scratch, scratch_roots, _, scratch_id, _) =
            st47_a_scratch_store_behind_a_link(&scratch_parent);
        let scratch_tail =
            EncryptedStore::delete_data_dir(&spell(&scratch), crate::store::Layout::ScratchStore);
        let scratch_root_left = scratch_roots.join(format!("{scratch_id}.root")).exists();

        let data = dir.path().join("data");
        let (_, data_roots, data_id, _) = file_root_store(&data);
        let data_tail =
            EncryptedStore::delete_data_dir(&spell(&data), crate::store::Layout::DataDirectory);
        let data_root_left = data_roots.join(format!("{data_id}.root")).exists();

        println!(
            "ST47-T3-CONTROL spelling=realdir{spelling} create={created:?} hold={held:?} open={opened:?} scratch_tail={} scratch_root_left={scratch_root_left} data_tail={} data_root_left={data_root_left}",
            answer_line(&scratch_tail),
            answer_line(&data_tail)
        );
        if created.is_err() || held.is_err() || opened.is_err() {
            problems.push(format!(
                "realdir{spelling}: create={created:?} hold={held:?} open={opened:?}"
            ));
        }
        if scratch_tail.is_err() || scratch_root_left || data_tail.is_err() || data_root_left {
            problems.push(format!(
                "realdir{spelling}: scratch_tail={} data_tail={}",
                answer_line(&scratch_tail),
                answer_line(&data_tail)
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Ruling ST-47 T4 (adversary 1's M01): a create refused over a live store
/// removes nothing. A create.pending written by hand, whose made mark records
/// the identity of the live store's root file, with a hard link of that file
/// at the mark's id: the create answers AlreadyExists, the live root still
/// unwraps the store key, and the store opens after it.
#[test]
fn st47_t4_a_create_over_a_live_store_answers_already_exists_before_any_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let live = roots_dir.join(format!("{root_id}.root"));
    let wrapped = wrapped_of(&store_dir);
    let marked = RootId::fresh().unwrap();
    std::fs::hard_link(&live, roots_dir.join(format!("{marked}.root"))).unwrap();
    write_made_mark(&store_dir, marked, Some(&live));

    let created = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            RootId::fresh().unwrap(),
        )),
        DEFAULT_RETENTION,
    )
    .map(drop);

    let live_unwraps = st47_unwraps(dir.path(), "live", &live, root_id, &wrapped);
    let mark_stands = store_dir.join("create.pending").exists();
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id).map(drop);
    println!(
        "ST47-T4-M01 create={created:?} live_unwraps={live_unwraps} mark_stands_after_the_create={mark_stands} open={opened:?}"
    );
    assert!(
        matches!(created, Err(StoreError::AlreadyExists)),
        "{created:?}"
    );
    assert!(
        live_unwraps,
        "a create refused over a live store destroyed the root that store.key names"
    );
    assert!(
        mark_stands,
        "a create refused over a live store removed create.pending"
    );
    assert!(opened.is_ok(), "{opened:?}");
}

/// The mode bits of the file at `path`, read without following a link.
fn st48_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777
}

/// A file of another party at `dir/name`: `len` bytes that are not zero, of
/// mode `mode`, with a read handle kept open from before the mode was set, so
/// that its bytes can be read through it whatever the mode becomes.
fn st48_foreign_file(
    dir: &Path,
    name: &str,
    len: usize,
    mode: u32,
) -> (PathBuf, Vec<u8>, std::fs::File) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    let bytes: Vec<u8> = (0..len).map(|i| 0x41 + (i % 26) as u8).collect();
    std::fs::write(&path, &bytes).unwrap();
    let reader = std::fs::File::open(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    (path, bytes, reader)
}

/// The bytes of the file that `reader` holds, read from its start.
fn st48_bytes_of(reader: &std::fs::File) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let mut reader = reader;
    let mut bytes = Vec::new();
    reader.seek(SeekFrom::Start(0)).unwrap();
    reader.read_to_end(&mut bytes).unwrap();
    bytes
}

/// A made mark of another root that lies in the roots folder, so that the
/// next open of the store runs its cleanup by that mark.
fn st48_mark_of_another_root(store_dir: &Path, roots_dir: &Path) {
    let marked = RootId::fresh().unwrap();
    file_root_custody(roots_dir, TestProtector::new(0x5a).0, marked)
        .create_root()
        .unwrap();
    write_made_mark(
        store_dir,
        marked,
        Some(&roots_dir.join(format!("{marked}.root"))),
    );
}

/// Ruling ST-48 U1 (the review's B1, the verifier's D-1): a file of another
/// party with a second name at a plain name of the roots folder, of a root
/// file's length and mode 0444, and of 100 bytes and mode 0644, keeps its
/// mode and its bytes, read through its outside name, after the tail, after
/// the handle's delete-all and after the open's cleanup; so does a two-name
/// file of mode 0000. A single-name file of a root file's length and mode
/// 0000 at a plain name is zeroed and removed.
#[test]
fn st48_u1_no_mode_change_at_a_plain_name_before_the_count_and_the_length_pass() {
    let mut problems = Vec::new();
    for (len, mode) in [(32usize, 0o444u32), (100, 0o644), (32, 0o000)] {
        for caller in ["tail", "delete_all", "the open's cleanup"] {
            let dir = tempfile::tempdir().unwrap();
            let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
            let (outside, bytes, reader) = st48_foreign_file(dir.path(), "outside", len, mode);
            std::fs::hard_link(&outside, roots_dir.join("plain")).unwrap();
            let answer = match caller {
                "tail" => tail(&store_dir, &roots_dir).map(drop),
                "delete_all" => {
                    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
                    store.delete_all().map(drop)
                }
                _ => {
                    st48_mark_of_another_root(&store_dir, &roots_dir);
                    open_over_file_root(&store_dir, &roots_dir, root_id).map(drop)
                }
            };
            let mode_after = st48_mode(&outside);
            let bytes_kept = st48_bytes_of(&reader) == bytes;
            println!(
                "ST48-U1 len={len} mode={mode:o} caller={caller} answer={answer:?} outside_mode={mode_after:o} outside_bytes_kept={bytes_kept}"
            );
            if mode_after != mode || !bytes_kept {
                problems.push(format!(
                    "{len} bytes, mode {mode:o}, {caller}: outside mode {mode_after:o}, bytes kept {bytes_kept}"
                ));
            }
        }
    }
    for caller in ["tail", "delete_all"] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        let (single, _, reader) = st48_foreign_file(&roots_dir, "single", 32, 0o000);
        let answer = match caller {
            "tail" => tail(&store_dir, &roots_dir).map(drop),
            _ => {
                let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
                store.delete_all().map(drop)
            }
        };
        let zeroed = st48_bytes_of(&reader).iter().all(|&b| b == 0);
        let stands = std::fs::symlink_metadata(&single).is_ok();
        println!(
            "ST48-U1-SINGLE caller={caller} answer={answer:?} zeroed={zeroed} stands={stands}"
        );
        if !zeroed || stands {
            problems.push(format!(
                "a single-name file of mode 0000, {caller}: zeroed {zeroed}, stands {stands}"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Ruling ST-48 U2 (the review's B2, the verifier's D-2): a create.pending
/// written by hand whose made mark records the identity of another party's
/// file of 100 bytes and mode 0644, which has a second name in the roots
/// folder (i) at a plain name, (ii) at the mark's own root name, (iii) at
/// another id's root name. After the next open, after the tail and after the
/// next create, each in its own case, that file's bytes and mode are
/// unchanged, read through its outside name, and no tail reads the root
/// destroyed for it.
#[test]
fn st48_u2_a_hand_written_mark_never_writes_through_another_partys_file() {
    let mut problems = Vec::new();
    for place in [
        "a plain name",
        "the mark's own root name",
        "another id's root name",
    ] {
        for reader in ["the next open", "the tail", "the next create"] {
            let dir = tempfile::tempdir().unwrap();
            let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
            let marked = RootId::fresh().unwrap();
            let other = RootId::fresh().unwrap();
            let (outside, bytes, kept) = st48_foreign_file(dir.path(), "outside", 100, 0o644);
            let inside = match place {
                "a plain name" => roots_dir.join("plain"),
                "the mark's own root name" => roots_dir.join(format!("{marked}.root")),
                _ => roots_dir.join(format!("{other}.root")),
            };
            std::fs::hard_link(&outside, &inside).unwrap();
            if reader != "the next open" {
                std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
            }
            write_made_mark(&store_dir, marked, Some(&outside));
            let (answer, destroyed) = match reader {
                "the next open" => (
                    format!(
                        "{:?}",
                        open_over_file_root(&store_dir, &roots_dir, root_id).map(drop)
                    ),
                    false,
                ),
                "the tail" => {
                    let answer = tail(&store_dir, &roots_dir);
                    let destroyed = match &answer {
                        Ok(report) => report.root_destroyed(),
                        Err(StoreError::DeleteIncomplete(report)) => report.root_destroyed(),
                        Err(_) => false,
                    };
                    (answer_line(&answer), destroyed)
                }
                _ => (
                    format!(
                        "{:?}",
                        EncryptedStore::create(
                            &store_dir,
                            Box::new(file_root_custody(
                                &roots_dir,
                                TestProtector::new(0x5a).0,
                                RootId::fresh().unwrap(),
                            )),
                            DEFAULT_RETENTION,
                        )
                        .map(drop)
                    ),
                    false,
                ),
            };
            let mode_after = st48_mode(&outside);
            let bytes_kept = st48_bytes_of(&kept) == bytes;
            println!(
                "ST48-U2 place={place} reader={reader} answer={answer} outside_mode={mode_after:o} outside_bytes_kept={bytes_kept} root_read_destroyed={destroyed}"
            );
            if mode_after != 0o644 || !bytes_kept || destroyed {
                problems.push(format!(
                    "{place}, {reader}: outside mode {mode_after:o}, bytes kept {bytes_kept}, root read destroyed {destroyed}"
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Ruling ST-48 U2, the named cost: the store's own root renamed inside the
/// roots folder to a plain name while it has a second name outside the folder
/// is removed there with no write through it. The handle's delete answers
/// DeleteIncomplete with the root left, the outside name keeps the root's
/// bytes, and the next tail answers by what then stands.
#[test]
fn st48_u2_the_stores_root_at_a_plain_name_with_a_second_name_outside_is_removed_unwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let root_file = roots_dir.join(format!("{root_id}.root"));
    let outside = dir.path().join("the-root-under-another-name");
    std::fs::hard_link(&root_file, &outside).unwrap();
    std::fs::rename(&root_file, roots_dir.join("plain")).unwrap();
    let bytes = std::fs::read(&outside).unwrap();

    let first = store.delete_all();
    drop(store);
    let first_bytes_kept = std::fs::read(&outside).ok() == Some(bytes.clone());
    let next = tail(&store_dir, &roots_dir);
    let next_bytes_kept = std::fs::read(&outside).ok() == Some(bytes.clone());
    println!(
        "ST48-U2-COST delete_all={} outside_bytes_kept={first_bytes_kept} plain_stands={} tail={} outside_bytes_kept_after_the_tail={next_bytes_kept}",
        answer_line(&first),
        roots_dir.join("plain").exists(),
        answer_line(&next)
    );
    assert!(
        first_bytes_kept,
        "the handle's delete wrote through a name of its root outside the roots folder"
    );
    assert!(
        matches!(&first, Err(StoreError::DeleteIncomplete(report)) if report.root == RootReading::Left(Some(root_id))),
        "a removal with no write read as the root's destruction: {}",
        answer_line(&first)
    );
}

/// Ruling ST-48 U3 (the verifier's row 26): a regular file of a root file's
/// length that stays at a plain name of the roots folder (here the folder is
/// at mode 0500, so that nothing in it can be removed) keeps the delete
/// unfinished, and no form of the answer holds its name.
#[test]
fn st48_u3_a_root_shaped_file_that_stays_at_a_plain_name_is_not_named() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    let plain = "st48-a-plain-name-of-the-person";
    std::fs::write(roots_dir.join(plain), [0x77u8; 32]).unwrap();
    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o500)).unwrap();

    let answer = tail(&store_dir, &roots_dir);

    std::fs::set_permissions(&roots_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let stands = roots_dir.join(plain).exists();
    let debug = format!("{answer:?}");
    println!(
        "ST48-U3-ROOT-SHAPED {} stands={stands} debug_holds_the_name={}",
        answer_line(&answer),
        debug.contains(plain)
    );
    assert!(stands, "the file at the plain name did not stay");
    assert_eq!(
        delete_report(&answer).roots.others_left.root_shaped,
        1,
        "the file that stays at a plain name is not counted as one that can be a root file"
    );
    assert!(
        matches!(answer, Err(StoreError::DeleteIncomplete(_))),
        "{}",
        answer_line(&answer)
    );
    assert!(
        !debug.contains(plain),
        "the answer names a file at a plain name of the roots folder"
    );
}

/// Ruling ST-48 U5 (b), the store directory opened from the parent's handle
/// in the tail: the data directory is moved away right after the tail opened
/// it, and another store's data directory is put at its old path. The tail
/// destroys the store of the directory that it opened, where it now lies,
/// and the other store keeps its key file and reads.
#[test]
fn st48_u5_the_tail_opens_the_store_directory_from_the_data_directorys_handle() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let (_, _, root_id, _) = file_root_store(&data);
    let other_parent = dir.path().join("other");
    let (_, _, other_id, other_segment) = file_root_store(&other_parent);
    let moved = dir.path().join("moved");
    let (from, to, other_from) = (data.clone(), moved.clone(), other_parent.clone());
    let _seam = crate::store::failpoint::at(
        crate::store::db::SEAM_AFTER_THE_DATA_DIRECTORY_OPEN,
        move || {
            std::fs::rename(&from, &to).unwrap();
            std::fs::rename(&other_from, &from).unwrap();
        },
    );

    let answer = EncryptedStore::delete_data_dir(&data, crate::store::Layout::DataDirectory);

    let other_reads = open_over_file_root(&data.join("store"), &data.join("roots"), other_id)
        .map(|store| store.read_segment(other_segment, &mut Vec::new()).is_ok());
    let own_key_left = moved.join("store").join("store.key").exists();
    let own_root_left = moved.join("roots").join(format!("{root_id}.root")).exists();
    println!(
        "ST48-U5-STORE-FROM-THE-PARENT {} other_store_reads={other_reads:?} own_key_left={own_key_left} own_root_left={own_root_left}",
        answer_line(&answer)
    );
    assert!(
        matches!(other_reads, Ok(true)),
        "the tail reached the store directory of another data directory put at the old path: {other_reads:?}"
    );
    assert!(
        !own_key_left && !own_root_left,
        "the tail left the store of the data directory that it opened"
    );
}

/// Where the root-step tests of store-3c put a file of another party in the
/// roots folder: at a root name of another id, or at the root name that the
/// call names (the store's own for the tail and the delete-all, the mark's own
/// for a cleanup by a made mark).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    AnotherIdsRootName,
    TheNamedRootName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caller {
    Tail,
    DeleteAll,
    TheOpensCleanup,
}

/// Runs `caller` on a fresh store of `dir` with the file at `outside` given a
/// second name at `place` in the roots folder; for the open's cleanup the
/// made mark records the identity of that file. Answers how the call ended.
fn st3c_call_with_a_foreign_file(
    dir: &Path,
    outside: &Path,
    place: Place,
    caller: Caller,
) -> String {
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir);
    let marked = RootId::fresh().unwrap();
    let named = match caller {
        Caller::TheOpensCleanup => marked,
        _ => root_id,
    };
    let mut store = (caller == Caller::DeleteAll)
        .then(|| open_over_file_root(&store_dir, &roots_dir, root_id).unwrap());
    let inside = match place {
        Place::AnotherIdsRootName => roots_dir.join(format!("{}.root", RootId::fresh().unwrap())),
        Place::TheNamedRootName => {
            let own = roots_dir.join(format!("{named}.root"));
            if own.exists() {
                std::fs::rename(&own, roots_dir.join("the-root-aside")).unwrap();
            }
            own
        }
    };
    std::fs::hard_link(outside, &inside).unwrap();
    match caller {
        Caller::Tail => answer_line(&tail(&store_dir, &roots_dir)),
        Caller::DeleteAll => answer_line(&store.as_mut().unwrap().delete_all()),
        Caller::TheOpensCleanup => {
            write_made_mark(&store_dir, marked, Some(outside));
            format!(
                "{:?}",
                open_over_file_root(&store_dir, &roots_dir, root_id).map(drop)
            )
        }
    }
}

/// The inbox entry of 2026-11-17T02:59:55Z (1), row 20 of
/// reports/verifier-162.md: a file of another party of 100 bytes and mode 0444
/// with a second name outside the roots folder, at a root name of another id
/// and at the root name that the call names, keeps its mode and its bytes,
/// read through its outside name, after the tail, after the handle's
/// delete-all and after the open's cleanup by a made mark that records its
/// identity: the step reads the count and the length by the name before it
/// sets any mode. A file with one name, of a root file's length and of mode
/// 0000, at a root name of another id is still zeroed and removed.
#[test]
fn row20_a_file_of_another_length_at_a_root_name_keeps_its_mode_and_its_bytes() {
    let mut problems = Vec::new();
    for place in [Place::AnotherIdsRootName, Place::TheNamedRootName] {
        for caller in [Caller::Tail, Caller::DeleteAll, Caller::TheOpensCleanup] {
            let dir = tempfile::tempdir().unwrap();
            let (outside, bytes, reader) = st48_foreign_file(dir.path(), "outside", 100, 0o444);
            let answer = st3c_call_with_a_foreign_file(dir.path(), &outside, place, caller);
            let mode_after = st48_mode(&outside);
            let bytes_kept = st48_bytes_of(&reader) == bytes;
            println!(
                "ROW20 place={place:?} caller={caller:?} answer={answer} outside_mode={mode_after:o} outside_bytes_kept={bytes_kept}"
            );
            if mode_after != 0o444 || !bytes_kept {
                problems.push(format!(
                    "{place:?}, {caller:?}: outside mode {mode_after:o}, bytes kept {bytes_kept}"
                ));
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, _, _) = file_root_store(dir.path());
    let single = roots_dir.join(format!("{}.root", RootId::fresh().unwrap()));
    let (_, _, reader) = st48_foreign_file(
        &roots_dir,
        single.file_name().unwrap().to_str().unwrap(),
        32,
        0o000,
    );
    let answer = tail(&store_dir, &roots_dir);
    let zeroed = st48_bytes_of(&reader).iter().all(|&b| b == 0);
    println!(
        "ROW20-SINGLE answer={} zeroed={zeroed} stands={}",
        answer_line(&answer),
        single.exists()
    );
    if !zeroed || single.exists() {
        problems.push(format!(
            "a single-name file of a root file's length and mode 0000 at another id's root name: zeroed {zeroed}, stands {}",
            single.exists()
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Where the test of ruling ST-49 gives a file of another party a second name
/// in the roots folder, and whether the mark's path writes through it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkedAt {
    OthersPartial,
    MarksOwnPartial,
    OthersRootName,
}

/// Ruling ST-49 (part (3) of the inbox entry of 2026-11-17T02:59:55Z, the
/// verifier's case N3): a create.pending written by hand whose made mark
/// records the identity of another party's file of 100 bytes and mode 0444,
/// with a second name at a partial name of another id than the mark's. After
/// the next open, after the tail and after the next create, each in its own
/// case, that file keeps its mode and its bytes through its outside name, the
/// partial name is gone and the tail reads no root destroyed. Controls: at a
/// partial name of the mark's own id the file is still zero-filled through
/// every name, at another id's root name a file of a root file's length still
/// is (ruling ST-48), and on the handle's path the store's own root renamed
/// to another id's partial name is still destroyed.
#[test]
fn st49_a_mark_writes_through_at_a_partial_name_of_its_own_id_alone() {
    let mut problems = Vec::new();
    for at in [
        MarkedAt::OthersPartial,
        MarkedAt::MarksOwnPartial,
        MarkedAt::OthersRootName,
    ] {
        for reader in ["the next open", "the tail", "the next create"] {
            let dir = tempfile::tempdir().unwrap();
            let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
            let marked = RootId::fresh().unwrap();
            let other = RootId::fresh().unwrap();
            let (len, mode) = match at {
                MarkedAt::OthersRootName => (32, 0o644),
                _ => (100, 0o444),
            };
            let (outside, bytes, kept) = st48_foreign_file(dir.path(), "outside", len, mode);
            let inside = roots_dir.join(match at {
                MarkedAt::OthersPartial => format!("{other}.0123456789abcdef.root.tmp"),
                MarkedAt::MarksOwnPartial => {
                    format!("{marked}.0123456789abcdef.root.tmp")
                }
                MarkedAt::OthersRootName => format!("{other}.root"),
            });
            std::fs::hard_link(&outside, &inside).unwrap();
            if reader != "the next open" {
                std::fs::remove_file(KeyFile::path(&store_dir)).unwrap();
            }
            write_made_mark(&store_dir, marked, Some(&outside));
            let (answer, destroyed) = match reader {
                "the next open" => (
                    format!(
                        "{:?}",
                        open_over_file_root(&store_dir, &roots_dir, root_id).map(drop)
                    ),
                    false,
                ),
                "the tail" => {
                    let answer = tail(&store_dir, &roots_dir);
                    let destroyed = match &answer {
                        Ok(report) => report.root_destroyed(),
                        Err(StoreError::DeleteIncomplete(report)) => report.root_destroyed(),
                        Err(_) => false,
                    };
                    (answer_line(&answer), destroyed)
                }
                _ => (
                    format!(
                        "{:?}",
                        EncryptedStore::create(
                            &store_dir,
                            Box::new(file_root_custody(
                                &roots_dir,
                                TestProtector::new(0x5a).0,
                                RootId::fresh().unwrap(),
                            )),
                            DEFAULT_RETENTION,
                        )
                        .map(drop)
                    ),
                    false,
                ),
            };
            let mode_after = st48_mode(&outside);
            let now = st48_bytes_of(&kept);
            let inside_stands = std::fs::symlink_metadata(&inside).is_ok();
            println!(
                "ST49 at={at:?} reader={reader} answer={answer} outside_mode={mode_after:o} outside_bytes_kept={} outside_zeroed={} inside_stands={inside_stands} root_read_destroyed={destroyed}",
                now == bytes,
                now.iter().all(|&b| b == 0)
            );
            let wrong = match at {
                MarkedAt::OthersPartial => {
                    mode_after != mode || now != bytes || inside_stands || destroyed
                }
                _ => !now.iter().all(|&b| b == 0) || inside_stands,
            };
            if wrong {
                problems.push(format!(
                    "{at:?}, {reader}: outside mode {mode_after:o}, bytes kept {}, zeroed {}, the name in the roots folder stands {inside_stands}, root read destroyed {destroyed}",
                    now == bytes,
                    now.iter().all(|&b| b == 0)
                ));
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let partial = roots_dir.join(format!(
        "{}.0123456789abcdef.root.tmp",
        RootId::fresh().unwrap()
    ));
    std::fs::rename(roots_dir.join(format!("{root_id}.root")), &partial).unwrap();
    let answer = store.delete_all();
    println!(
        "ST49-HANDLE answer={} partial_stands={}",
        answer_line(&answer),
        partial.exists()
    );
    if !matches!(&answer, Ok(report) if report.root == RootReading::Destroyed) || partial.exists() {
        problems.push(format!(
            "the handle's delete-all of its root renamed to another id's partial name answered {}",
            answer_line(&answer)
        ));
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
