// Temporary probes of verifier-128 for its reread at eeb6a4c7 (ruling ST-46). They ship nowhere.
// Every store and every root lives in a tempfile::tempdir of the test (the root-file custody).
// They print names, lengths and answers only.

use super::*;

fn v128r_tree(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            out.push("<unreadable>".to_string());
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let kind = if meta.file_type().is_symlink() {
                "link"
            } else if meta.is_dir() {
                "dir"
            } else {
                "file"
            };
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let len = if meta.is_file() { meta.len() } else { 0 };
            out.push(format!("{rel}:{kind}:{len}"));
            if meta.is_dir() {
                walk(base, &path, out);
            }
        }
    }
    if std::fs::symlink_metadata(dir).is_err() {
        return vec!["<nothing at the name>".to_string()];
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn v128r_reads(store_dir: &Path, roots: &Path, segment: Uuid) -> String {
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

fn v128r_line(answer: &StoreResult<crate::store::DeleteReport>) -> String {
    match answer {
        Ok(report) => format!("Ok {report:?}"),
        Err(StoreError::DeleteIncomplete(report)) => format!("DeleteIncomplete {report:?}"),
        Err(e) => format!("{e:?}"),
    }
}

/// Issue 1 of verifier-128: a roots path that reaches another store's roots through a link in a part
/// below the directory that it shares with the store path. Three arms of the tail: a link at the store
/// name, nothing at the store name, and a store of the tail's own at the store name.
#[test]
fn v128r_issue_1_the_tail_runs_no_sweep_through_a_link_in_its_roots_path() {
    let mut problems = Vec::new();
    for arm in ["a link at the store name", "nothing at the store name", "a store at the store name"] {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("data");
        std::fs::create_dir(&base).unwrap();
        let store_dir = base.join("store");
        let behind = dir.path().join("behind");
        let second_id = RootId::fresh().unwrap();
        let mut second = EncryptedStore::create(
            &behind,
            Box::new(DevFallbackCustody::new(second_id, behind.join("roots"))),
            DEFAULT_RETENTION,
        )
        .unwrap();
        let (segment, _) = second
            .write_segment(&mut Cursor::new(b"typed errors"))
            .unwrap();
        drop(second);
        match arm {
            "a link at the store name" => std::os::unix::fs::symlink(&behind, &store_dir).unwrap(),
            "a store at the store name" => {
                let own = EncryptedStore::create(
                    &store_dir,
                    Box::new(DevFallbackCustody::new(
                        RootId::fresh().unwrap(),
                        base.join("own-roots"),
                    )),
                    DEFAULT_RETENTION,
                )
                .unwrap();
                drop(own);
            }
            _ => {}
        }
        std::os::unix::fs::symlink(&behind, base.join("alias")).unwrap();
        let roots = base.join("alias").join("roots");
        let before = (files_with_bytes(&behind), v128r_tree(&behind));
        let answer = EncryptedStore::delete_store_dir(&store_dir, &roots);
        let unchanged = (files_with_bytes(&behind), v128r_tree(&behind)) == before;
        let reads = v128r_reads(&behind, &behind.join("roots"), segment.segment_id);
        println!("V128R issue-1 {arm:?} answer={}", v128r_line(&answer));
        println!(
            "V128R issue-1 {arm:?} behind_unchanged={unchanged} roots_behind={:?} the_other_store: {reads}",
            v128r_tree(&behind.join("roots")).len()
        );
        if !reads.starts_with("open=Ok segment_plaintext_equal=true") || !unchanged {
            problems.push(format!("{arm}: {reads}, behind_unchanged={unchanged}"));
        }
        if answer.is_ok() {
            problems.push(format!("{arm}: the tail answered Ok on a roots path with a link"));
        }
    }
    assert!(
        problems.is_empty(),
        "the tail reached another store's roots through a link in its roots path: {}",
        problems.join("; ")
    );
}

/// Issue 2 of verifier-128 (copy L44): the tail on a store that a live handle holds.
#[test]
fn v128r_issue_2_the_tail_on_a_held_store_answers_locked_and_changes_nothing() {
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
    let before = (
        files_with_bytes(&roots_dir),
        std::fs::read(store_dir.join("store.key")).unwrap(),
        v128r_tree(&roots_dir),
    );
    let answer = EncryptedStore::delete_store_dir(&store_dir, &roots_dir);
    let after = (
        files_with_bytes(&roots_dir),
        std::fs::read(store_dir.join("store.key")).unwrap_or_default(),
        v128r_tree(&roots_dir),
    );
    drop(store);
    let reads = v128r_reads(&store_dir, &roots_dir, segment.segment_id);
    println!(
        "V128R issue-2 tail={} roots_and_key_file_unchanged={} after the drop: {reads}",
        v128r_line(&answer),
        before == after
    );
    assert!(
        matches!(answer, Err(StoreError::Locked)),
        "the tail on a held store did not answer Locked: {}",
        v128r_line(&answer)
    );
    assert!(before == after, "the tail on a held store changed the root file or store.key");
    assert!(reads.starts_with("open=Ok segment_plaintext_equal=true"), "{reads}");
}

/// Issue 3 of verifier-128, the named limit of ruling ST-46 K6: a directory put at the old roots name
/// that holds a file of the root file's length at the root's name. It prints and asserts nothing.
#[test]
fn v128r_issue_3_what_the_named_limit_prints() {
    let dir = tempfile::tempdir().unwrap();
    let store_dir = dir.path().join("store");
    let roots = store_dir.join("roots");
    let first_id = RootId::fresh().unwrap();
    let mut first = EncryptedStore::create(
        &store_dir,
        Box::new(DevFallbackCustody::new(first_id, roots.clone())),
        DEFAULT_RETENTION,
    )
    .unwrap();
    first
        .write_segment(&mut Cursor::new(b"typed errors"))
        .unwrap();
    drop(first);
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
    std::fs::write(roots.join(format!("{first_id}.root")), [7u8; 32]).unwrap();
    let answer = first.delete_all();
    drop(first);
    println!("V128R issue-3 answer={}", v128r_line(&answer));
    println!(
        "V128R issue-3 the_moved_roots={:?} the_roots_at_the_old_name={:?} the_store_made_there: {}",
        v128r_tree(&moved.join("roots")).len(),
        v128r_tree(&roots).len(),
        v128r_reads(&store_dir, &roots, segment.segment_id)
    );
}
