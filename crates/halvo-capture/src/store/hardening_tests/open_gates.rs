use std::path::Path;

use halvo_sqlcipher::rusqlite::Connection;

use super::super::db::test_support::key_statement;
use super::super::db::{read_back_seam, EncryptedStore};
use super::super::{StoreError, DEFAULT_RETENTION};
use super::{
    answer_text, file_root_custody, file_root_store, open_over_file_root, RootId, TestProtector,
};

pub(super) fn names(listed: &[(String, Option<Vec<u8>>)]) -> Vec<(String, Option<usize>)> {
    listed
        .iter()
        .map(|(name, bytes)| (name.clone(), bytes.as_ref().map(Vec::len)))
        .collect()
}

pub(super) fn tree(dir: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    let mut listed = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let entry = entry.unwrap();
            let name = entry
                .path()
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if entry.file_type().unwrap().is_dir() {
                listed.push((name, None));
                stack.push(entry.path());
            } else {
                listed.push((name, Some(std::fs::read(entry.path()).unwrap())));
            }
        }
    }
    listed.sort();
    listed
}

#[test]
fn an_open_of_a_store_whose_auto_vacuum_is_not_0_is_refused_by_its_name_and_leaves_the_directory_as_it_was(
) {
    for mode in [1i64, 2] {
        let dir = tempfile::tempdir().unwrap();
        let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
        let key = {
            let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
            *store.database_key().unwrap()
        };
        let set: i64 = {
            let conn = Connection::open(store_dir.join("store.db")).unwrap();
            conn.execute_batch(&key_statement(&key)).unwrap();
            conn.execute_batch(&format!(
                "PRAGMA auto_vacuum = {mode}; VACUUM; PRAGMA wal_checkpoint(TRUNCATE);"
            ))
            .unwrap();
            conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
                .unwrap()
        };
        let before = tree(&store_dir);
        let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
        let answer = match &opened {
            Ok(_) => "Ok".to_string(),
            Err(e) => format!("Err({e:?})"),
        };
        let refused = matches!(&opened, Err(StoreError::SettingNotInEffect("auto_vacuum")));
        drop(opened);
        let after = tree(&store_dir);
        println!(
            "T2-auto-vacuum set={set} answer={answer} before={:?} after={:?}",
            names(&before),
            names(&after)
        );
        assert_eq!(set, mode, "the database's auto_vacuum was not set");
        assert!(
            refused,
            "an open of a store with auto_vacuum {mode} answered {answer}"
        );
        assert_eq!(
            names(&after),
            names(&before),
            "the names and lengths after the refused open"
        );
        assert!(
            after == before,
            "the bytes of an entry changed at the refused open"
        );
    }
}

fn a_create_that_meets(
    statement: &'static str,
) -> (String, Vec<(String, Option<usize>)>, bool, String) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir) = (dir.path().join("store"), dir.path().join("roots"));
    let root_id = RootId::fresh().unwrap();
    let custody = file_root_custody(&roots_dir, TestProtector::new(0x5a).0, root_id);
    let roots_before = tree_or_none(&roots_dir);
    let created = {
        let _seam = read_back_seam::before_the_read_backs(move |conn| {
            conn.execute_batch(statement).unwrap();
        });
        EncryptedStore::create(&store_dir, Box::new(custody), DEFAULT_RETENTION)
    };
    let answer = answer_text(&created);
    drop(created);
    let left = tree_or_none(&store_dir).map_or_else(Vec::new, |listed| names(&listed));
    let roots_same = tree_or_none(&roots_dir) == roots_before;
    let again_id = RootId::fresh().unwrap();
    let again = EncryptedStore::create(
        &store_dir,
        Box::new(file_root_custody(
            &roots_dir,
            TestProtector::new(0x5a).0,
            again_id,
        )),
        DEFAULT_RETENTION,
    );
    (answer, left, roots_same, answer_text(&again))
}

fn tree_or_none(dir: &Path) -> Option<Vec<(String, Option<Vec<u8>>)>> {
    dir.exists().then(|| tree(dir))
}

fn database_or_key(left: &[(String, Option<usize>)]) -> Vec<String> {
    left.iter()
        .map(|(name, _)| name.clone())
        .filter(|name| name.starts_with("store.db") || name == "store.key")
        .collect()
}

#[test]
fn a_create_that_reads_auto_vacuum_1_back_is_refused_by_its_name_and_its_cleanup_leaves_no_database_file_no_key_file_and_the_roots_as_they_were(
) {
    let (answer, left, roots_same, again) = a_create_that_meets("PRAGMA auto_vacuum = 1;");
    println!(
        "S3-auto-vacuum answer={answer} left={left:?} roots_same={roots_same} next_create={again}"
    );
    assert_eq!(answer, "SettingNotInEffect(\"auto_vacuum\")");
    assert!(
        database_or_key(&left).is_empty(),
        "the refused create left {left:?}"
    );
    assert!(roots_same, "the refused create changed the roots directory");
    assert_eq!(again, "Ok", "a create after the refused one");
}

#[test]
fn a_create_that_reads_page_size_8192_back_is_refused_by_its_name_and_its_cleanup_leaves_no_database_file_no_key_file_and_the_roots_as_they_were(
) {
    let (answer, left, roots_same, again) = a_create_that_meets("PRAGMA page_size = 8192;");
    println!(
        "S3-page-size answer={answer} left={left:?} roots_same={roots_same} next_create={again}"
    );
    assert_eq!(answer, "SettingNotInEffect(\"page_size\")");
    assert!(
        database_or_key(&left).is_empty(),
        "the refused create left {left:?}"
    );
    assert!(roots_same, "the refused create changed the roots directory");
    assert_eq!(again, "Ok", "a create after the refused one");
}
