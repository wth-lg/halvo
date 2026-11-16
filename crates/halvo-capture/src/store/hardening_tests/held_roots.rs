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
