use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use halvo_sqlcipher::rusqlite::config::DbConfig;
use halvo_sqlcipher::rusqlite::Connection;

use super::super::db::test_support::key_statement;
use super::super::db::EncryptedStore;
use super::super::roots::{self, RootGuard};
use super::super::{StoreError, DEFAULT_RETENTION};

fn made(dir: &Path, root: &RootGuard) -> ([u8; 32], [u8; 32]) {
    let store = EncryptedStore::create(dir, Box::new(roots::custody(root.id())), DEFAULT_RETENTION)
        .unwrap();
    let store_key = *store.store_key().unwrap().expose();
    let database = *store.database_key().unwrap();
    (store_key, database)
}

fn fresh_database(dir: &Path, key: &[u8; 32]) -> Connection {
    for name in ["store.db", "store.db-wal", "store.db-shm"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let conn = Connection::open(dir.join("store.db")).unwrap();
    conn.execute_batch(&key_statement(key)).unwrap();
    conn
}

fn older_at(dir: &Path, version: i64, key: &[u8; 32], frames: bool) {
    let conn = fresh_database(dir, key);
    conn.execute_batch(&format!(
        "PRAGMA journal_mode = WAL;\n\
         CREATE TABLE segment_keys (id INTEGER PRIMARY KEY, segment_id TEXT NOT NULL UNIQUE, \
           wrapped_key BLOB, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL);\n\
         CREATE TABLE events (event_id TEXT PRIMARY KEY, segment_id TEXT NOT NULL);\n\
         PRAGMA user_version = {version};\n\
         PRAGMA wal_checkpoint(TRUNCATE);"
    ))
    .unwrap();
    if frames {
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .unwrap();
        conn.execute_batch("INSERT INTO segment_keys VALUES (1, 'a', NULL, 0, 0);")
            .unwrap();
    }
}

fn of_another_build(dir: &Path, key: &[u8; 32], version: i64, tables: &str) {
    let conn = fresh_database(dir, key);
    conn.execute_batch(&format!(
        "PRAGMA journal_mode = WAL;\n\
         {tables}\n\
         PRAGMA user_version = {version};\n\
         PRAGMA wal_checkpoint(TRUNCATE);"
    ))
    .unwrap();
}

fn cut_behind_the_first_page(dir: &Path) {
    let path = dir.join("store.db");
    let before = std::fs::metadata(&path).unwrap().len();
    assert!(before > 4096, "the database to cut has {before} bytes");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(4096)
        .unwrap();
}

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

fn refused(
    name: &str,
    shape: &dyn Fn(&Path, &[u8; 32], &[u8; 32]),
    wanted: &dyn Fn(&StoreError) -> bool,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = RootGuard::fresh();
    let (store_key, database) = made(dir.path(), &root);
    shape(dir.path(), &store_key, &database);
    let before = tree(dir.path());
    let opened = roots::open(dir.path(), DEFAULT_RETENTION);
    let answer = match &opened {
        Ok(_) => "Ok".to_string(),
        Err(e) => format!("Err({e:?})"),
    };
    let right = matches!(&opened, Err(e) if wanted(e));
    drop(opened);
    let after = tree(dir.path());
    let names = |listed: &[(String, Option<Vec<u8>>)]| {
        listed
            .iter()
            .map(|(name, bytes)| (name.clone(), bytes.as_ref().map(Vec::len)))
            .collect::<Vec<_>>()
    };
    println!(
        "T4-{name} answer={answer} before={:?} after={:?}",
        names(&before),
        names(&after)
    );
    assert!(right, "{name}: the open answered {answer}");
    assert_eq!(
        names(&after),
        names(&before),
        "{name}: the names and lengths after the refused open"
    );
    assert!(
        after == before,
        "{name}: the bytes of an entry changed at the refused open"
    );
}

fn older(e: &StoreError) -> bool {
    matches!(
        e,
        StoreError::OlderStore {
            found: 1,
            supported: 2
        }
    )
}

fn newer(e: &StoreError) -> bool {
    matches!(
        e,
        StoreError::NewerStore {
            found: 3,
            supported: 2
        }
    )
}

fn unreadable(e: &StoreError) -> bool {
    matches!(e, StoreError::DatabaseUnreadable)
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_store_of_version_1_answers_older_store_and_its_directory_stays_as_it_was() {
    for mode in [0o644, 0o400] {
        refused(
            &format!("v1-{mode:o}"),
            &|dir, store_key, _| {
                older_at(dir, 1, store_key, false);
                set_mode(&dir.join("store.db"), mode);
            },
            &older,
        );
    }
    refused(
        "v1-with-an-empty-log",
        &|dir, store_key, _| {
            older_at(dir, 1, store_key, false);
            std::fs::write(dir.join("store.db-wal"), b"").unwrap();
        },
        &older,
    );
}

#[test]
fn a_store_db_of_one_byte_answers_database_unreadable_and_its_directory_stays_as_it_was() {
    for byte in [0xffu8, 0x00] {
        refused(
            &format!("one-byte-{byte:02x}"),
            &|dir, _, _| {
                for name in ["store.db-wal", "store.db-shm"] {
                    let _ = std::fs::remove_file(dir.join(name));
                }
                std::fs::write(dir.join("store.db"), [byte]).unwrap();
            },
            &unreadable,
        );
    }
}

#[test]
fn a_store_of_version_3_answers_newer_store_also_with_a_statement_of_a_later_build() {
    refused(
        "v3",
        &|dir, _, database| of_another_build(dir, database, 3, "CREATE TABLE later (x);"),
        &newer,
    );
    refused(
        "v3-statement-of-a-later-build",
        &|dir, _, database| {
            of_another_build(dir, database, 3, "CREATE TABLE later (x);");
            let conn = Connection::open(dir.join("store.db")).unwrap();
            conn.execute_batch(&key_statement(database)).unwrap();
            conn.execute_batch(
                "PRAGMA writable_schema = ON;\n\
                 INSERT INTO sqlite_master (type, name, tbl_name, rootpage, sql) VALUES \
                 ('table', 'latest', 'latest', 0, 'CREATE TABLE latest (x) FROM A LATER BUILD');",
            )
            .unwrap();
        },
        &newer,
    );
}

#[test]
fn a_store_cut_behind_its_first_page_answers_database_unreadable_whatever_its_version() {
    refused(
        "v3-cut",
        &|dir, _, database| {
            of_another_build(
                dir,
                database,
                3,
                "CREATE TABLE later (x); INSERT INTO later VALUES (randomblob(20000));",
            );
            cut_behind_the_first_page(dir);
        },
        &unreadable,
    );
    refused(
        "v1-cut",
        &|dir, store_key, _| {
            older_at(dir, 1, store_key, false);
            let conn = Connection::open(dir.join("store.db")).unwrap();
            conn.execute_batch(&key_statement(store_key)).unwrap();
            conn.execute_batch(
                "INSERT INTO segment_keys VALUES (1, 'a', randomblob(20000), 0, 0);\n\
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
            drop(conn);
            cut_behind_the_first_page(dir);
        },
        &unreadable,
    );
    refused(
        "v1-a-byte-more",
        &|dir, store_key, _| {
            older_at(dir, 1, store_key, false);
            let mut bytes = std::fs::read(dir.join("store.db")).unwrap();
            bytes.push(0);
            std::fs::write(dir.join("store.db"), bytes).unwrap();
        },
        &unreadable,
    );
}

#[test]
fn a_database_without_the_store_tables_answers_database_unreadable_also_at_version_0() {
    refused(
        "no-store-tables-v0",
        &|dir, _, database| of_another_build(dir, database, 0, "CREATE TABLE other (x);"),
        &unreadable,
    );
}

#[test]
fn a_store_db_that_neither_key_opens_or_of_another_page_size_answers_database_unreadable_and_stays_as_it_was(
) {
    refused(
        "another-key",
        &|dir, _, _| older_at(dir, 1, &[0x5au8; 32], false),
        &unreadable,
    );
    refused(
        "page-size-8192",
        &|dir, _, database| {
            let conn = fresh_database(dir, database);
            conn.execute_batch(
                "PRAGMA cipher_page_size = 8192;\n\
                 PRAGMA journal_mode = WAL;\n\
                 CREATE TABLE segment_keys (x);\n\
                 PRAGMA user_version = 2;\n\
                 PRAGMA wal_checkpoint(TRUNCATE);",
            )
            .unwrap();
        },
        &unreadable,
    );
}

#[test]
fn a_store_of_version_1_with_frames_in_its_log_answers_database_unreadable_and_stays_as_it_was() {
    refused(
        "v1-with-frames",
        &|dir, store_key, _| older_at(dir, 1, store_key, true),
        &unreadable,
    );
}
