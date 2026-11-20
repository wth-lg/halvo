use std::path::Path;

use halvo_sqlcipher::rusqlite::Connection;

use super::super::db::test_support::key_statement;
use super::super::StoreError;
use super::{file_root_store, open_over_file_root};

fn tree(dir: &Path) -> Vec<(String, Option<Vec<u8>>)> {
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
        let names = |listed: &[(String, Option<Vec<u8>>)]| {
            listed
                .iter()
                .map(|(name, bytes)| (name.clone(), bytes.as_ref().map(Vec::len)))
                .collect::<Vec<_>>()
        };
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
