use std::path::{Path, PathBuf};

use halvo_sqlcipher::rusqlite::config::DbConfig;
use halvo_sqlcipher::rusqlite::Connection;

use super::super::db::test_support::key_statement;
use super::super::StoreError;
use super::open_gates::{names, tree};
use super::{answer_text, file_root_store, open_over_file_root, RootId};

type Listed = Vec<(String, Option<Vec<u8>>)>;

fn log_length(listed: &Listed) -> usize {
    listed
        .iter()
        .find(|(name, _)| name == "store.db-wal")
        .and_then(|(_, bytes)| bytes.as_ref().map(Vec::len))
        .unwrap_or(0)
}

/// Runs `statements` on store.db with the database key, under exclusive
/// locking so that no -shm is made, and closes without a checkpoint, so that
/// the log keeps their frames at its name.
fn write_frames(store_dir: &Path, key: &[u8; 32], statements: &str) {
    let conn = Connection::open(store_dir.join("store.db")).unwrap();
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
        .unwrap();
    conn.execute_batch(&key_statement(key)).unwrap();
    conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE;")
        .unwrap();
    conn.execute_batch(statements).unwrap();
}

fn a_store_with_frames(dir: &Path, statements: &str) -> (PathBuf, PathBuf, RootId) {
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir);
    let key = {
        let store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
        *store.database_key().unwrap()
    };
    write_frames(&store_dir, &key, statements);
    (store_dir, roots_dir, root_id)
}

fn refused_over_frames(tag: &str, statements: &str, wanted: impl Fn(&StoreError) -> bool) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_store_with_frames(dir.path(), statements);
    let before = tree(&store_dir);
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
    let answer = answer_text(&opened);
    let refused = matches!(&opened, Err(e) if wanted(e));
    drop(opened);
    let after = tree(&store_dir);
    println!(
        "{tag} answer={answer} before={:?} after={:?}",
        names(&before),
        names(&after)
    );
    assert!(
        log_length(&before) > 0,
        "the log holds no frames before the open"
    );
    assert!(refused, "the open over frames answered {answer}");
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

#[test]
fn an_open_refused_by_the_auto_vacuum_gate_while_the_log_holds_frames_leaves_names_lengths_and_bytes_as_they_were(
) {
    refused_over_frames("S2-auto-vacuum", "PRAGMA auto_vacuum = 1; VACUUM;", |e| {
        matches!(e, StoreError::SettingNotInEffect("auto_vacuum"))
    });
}

#[test]
fn an_open_refused_by_the_schema_look_while_the_log_holds_frames_leaves_names_lengths_and_bytes_as_they_were(
) {
    refused_over_frames("S2-schema", "DROP TABLE daily_totals;", |e| {
        matches!(e, StoreError::DatabaseUnreadable)
    });
}

#[cfg(unix)]
#[test]
fn an_open_whose_store_db_is_exchanged_before_the_drain_while_the_log_holds_frames_is_refused_and_writes_no_byte(
) {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::super::db::SEAM_BEFORE_THE_DRAIN;
    use super::super::failpoint;
    use super::nth_call;

    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id) = a_store_with_frames(
        dir.path(),
        "CREATE TABLE frames_alone(x); DROP TABLE frames_alone;",
    );
    let aside = dir.path().join("store-db-aside");
    let seen: Rc<RefCell<Option<(Listed, Vec<u8>)>>> = Rc::new(RefCell::new(None));
    let act = {
        let (store_dir, aside, seen) = (store_dir.clone(), aside.clone(), seen.clone());
        move || {
            std::fs::rename(store_dir.join("store.db"), &aside).unwrap();
            std::fs::copy(&aside, store_dir.join("store.db")).unwrap();
            *seen.borrow_mut() = Some((tree(&store_dir), std::fs::read(&aside).unwrap()));
        }
    };
    let seam = failpoint::at(SEAM_BEFORE_THE_DRAIN, nth_call(1, act));
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
    drop(seam);
    let answer = answer_text(&opened);
    drop(opened);
    let (before, aside_before) = seen.borrow_mut().take().expect("the seam did not run");
    let after = tree(&store_dir);
    let aside_after = std::fs::read(&aside).unwrap();
    println!(
        "S2-exchanged answer={answer} before={:?} after={:?} held_file_before={} held_file_after={} held_file_same={}",
        names(&before),
        names(&after),
        aside_before.len(),
        aside_after.len(),
        aside_before == aside_after
    );
    assert!(
        log_length(&before) > 0,
        "the log holds no frames before the open"
    );
    assert_eq!(
        answer,
        "Obstructed { name: Database, met: DatabaseOutsideTheDirectory }"
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
    assert!(
        aside_after == aside_before,
        "the open wrote into the file that it held"
    );
}
