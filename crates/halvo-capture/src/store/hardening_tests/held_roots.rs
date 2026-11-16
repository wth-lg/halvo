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
        // SAFETY: c is NUL-terminated and outlives the call, which reads only it.
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
