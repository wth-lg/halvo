use std::io::Cursor;
use std::path::Path;

use halvo_sqlcipher::rusqlite::Connection;
use uuid::Uuid;

use super::super::db::test_support::key_statement;
use super::super::db::{random_segment_id, EncryptedStore, SEAM_AT_THE_LOG_CHECK};
use super::super::failpoint;
use super::super::pages::{places_of, store_pages};
use super::super::segment::segment_path;
use super::super::{
    KindTotal, Met, Reconciled, SegmentExpiry, SegmentStore, SegmentWrite, ShredReport, StoreError,
    StoreName, UtcDay, WriteFault, Written, DEFAULT_RETENTION,
};
use super::{file_root_store, nth_call, open_over_file_root};

const NANOS_PER_DAY: i64 = 86_400 * 1_000_000_000;
const PAYLOAD: &[u8] = b"a batch whose first commit meets a log that left its name";
const TOTALS: [KindTotal<'static>; 1] = [KindTotal {
    kind: "keystroke",
    events: 5,
    bytes: 40,
}];

fn noon_today() -> i64 {
    let now = crate::event::unix_nanos(std::time::SystemTime::now());
    now.div_euclid(NANOS_PER_DAY) * NANOS_PER_DAY + NANOS_PER_DAY / 2
}

fn write(store: &mut EncryptedStore, id: &Uuid, first: i64) -> Result<Written, WriteFault> {
    store.write_batch(SegmentWrite {
        segment_id: id,
        day: UtcDay::of_instant(first),
        first_capture: first,
        last_capture: first,
        kinds: &TOTALS,
        payload: PAYLOAD,
    })
}

fn said(answer: &Result<Written, WriteFault>) -> String {
    match answer {
        Ok(w) => format!("Ok(ordinal {})", w.ordinal.get()),
        Err(e) => format!("Err({e:?})"),
    }
}

fn held(answer: &Result<Written, WriteFault>) -> bool {
    matches!(answer, Ok(_) | Err(WriteFault::AlreadyWritten { .. }))
}

fn the_log_obstructed(e: &StoreError) -> bool {
    matches!(
        e,
        StoreError::Obstructed {
            name: StoreName::Log,
            met: Met::DatabaseOutsideTheDirectory,
        }
    )
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
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
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

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn live_rows_in_the_files(dir: &Path, key: &[u8; 32], id: &Uuid) -> i64 {
    let conn = Connection::open(dir.join("store.db")).unwrap();
    conn.execute_batch(&key_statement(key)).unwrap();
    conn.query_row(
        "SELECT count(*) FROM segment_keys WHERE segment_id = ?1 AND wrapped_key IS NOT NULL",
        [id.simple().to_string()],
        |r| r.get(0),
    )
    .unwrap()
}

fn live_rows_of(store: &EncryptedStore, id: &Uuid) -> Option<i64> {
    store.db().ok().map(|db| {
        db.connection()
            .query_row(
                "SELECT count(*) FROM segment_keys WHERE segment_id = ?1 AND wrapped_key IS NOT NULL",
                [id.simple().to_string()],
                |r| r.get(0),
            )
            .unwrap()
    })
}

fn live_rows_of_all(store: &EncryptedStore) -> Option<i64> {
    store.db().ok().map(|db| {
        db.connection()
            .query_row(
                "SELECT count(*) FROM segment_keys WHERE wrapped_key IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap()
    })
}

fn segment_files(store_dir: &Path) -> i64 {
    tree(&store_dir.join("segments"))
        .iter()
        .filter(|(_, bytes)| bytes.is_some())
        .count() as i64
}

fn assert_held(store: &EncryptedStore, store_dir: &Path, id: &Uuid, what: &str) {
    assert_eq!(live_rows_of(store, id), Some(1), "{what}: the live key row");
    assert!(segment_path(store_dir, *id).exists(), "{what}: the file");
    let mut plain = Vec::new();
    store.read_segment(*id, &mut plain).unwrap();
    assert_eq!(plain, PAYLOAD, "{what}: the read");
    let inventory = store.inventory().unwrap();
    assert!(
        inventory
            .iter()
            .any(|row| row.day == UtcDay::of_instant(noon_today())
                && row.kind == "keystroke"
                && row.events == 5
                && row.bytes == 40),
        "{what}: the inventory is {inventory:?}"
    );
}

fn greatest_ordinal(store: &EncryptedStore) -> i64 {
    store
        .db()
        .unwrap()
        .connection()
        .query_row("SELECT max(id) FROM segment_keys", [], |r| r.get(0))
        .unwrap()
}

fn assert_ordinal_above(answer: &Result<Written, WriteFault>, named: i64, what: &str) {
    let ordinal = answer.as_ref().map(|w| w.ordinal.get() as i64).unwrap();
    println!("RC1 {what}: the retry's ordinal {ordinal}, the greatest ordinal that an answer named {named}");
    assert!(
        ordinal > named,
        "{what}: the retry's ordinal {ordinal} is not above {named}, the greatest ordinal that an answer named"
    );
}

enum Taken {
    Unlinked,
    MovedOut,
}

fn batch_after_the_log_left(name: &str, how: Taken) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let key = *store.database_key().unwrap();
    let named = greatest_ordinal(&store);
    let log = store_dir.join("store.db-wal");
    match how {
        Taken::Unlinked => std::fs::remove_file(&log).unwrap(),
        Taken::MovedOut => {
            std::fs::rename(&log, dir.path().join("log-out-of-the-directory")).unwrap()
        }
    }
    let id = random_segment_id().unwrap();
    let first = write(&mut store, &id, noon_today());
    let handle_live = live_rows_of(&store, &id);
    let file = segment_path(&store_dir, id).exists();
    let read = store.read_segment(id, &mut Vec::new()).map(|_| ());
    let retry = write(&mut store, &id, noon_today());
    let copy = dir.path().join("as-a-kill-leaves-it");
    copy_tree(&store_dir, &copy);
    let copy_live = live_rows_in_the_files(&copy, &key, &id);
    let copy_file = segment_path(&copy, id).exists();
    println!(
        "RC1-{name} first={} handle_live={handle_live:?} file={file} read={read:?} retry={} copy_live={copy_live} copy_file={copy_file}",
        said(&first),
        said(&retry)
    );
    assert!(
        matches!(first, Err(WriteFault::Io(std::io::ErrorKind::Other))),
        "the first answer: {}",
        said(&first)
    );
    assert_eq!(
        handle_live.map_or(file, |live| live == 1),
        file,
        "the handle shows a live key row {handle_live:?} and a file {file} that disagree"
    );
    assert!(
        !held(&retry),
        "a retry on the same handle answered {}",
        said(&retry)
    );
    assert!(read.is_err(), "a read on the same handle answered {read:?}");
    assert_eq!(
        copy_live == 1,
        copy_file,
        "the files as a kill leaves them hold a live key row {copy_live} and a file {copy_file}"
    );
    {
        let mut left = open_over_file_root(&copy, &roots_dir, root_id).unwrap();
        let reconciled = left.reconciled();
        let live = live_rows_of(&left, &id).unwrap();
        let file = segment_path(&copy, id).exists();
        println!("RC1-{name} after_a_kill reconciled={reconciled:?} live={live} file={file}");
        assert_eq!(
            reconciled.rows_shredded, 0,
            "the open after a kill shredded a row"
        );
        assert_eq!(
            live == 1,
            file,
            "after a kill: a live key row {live} and a file {file}"
        );
        let again = write(&mut left, &id, noon_today());
        assert!(
            again.is_ok(),
            "after a kill the retry answered {}",
            said(&again)
        );
        assert_ordinal_above(&again, named, "after a kill");
        assert_held(&left, &copy, &id, "after a kill and a retry");
    }
    drop(store);
    let mut reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let reconciled = reopened.reconciled();
    let live = live_rows_of(&reopened, &id).unwrap();
    let file = segment_path(&store_dir, id).exists();
    println!("RC1-{name} after_a_clean_close reconciled={reconciled:?} live={live} file={file}");
    assert_eq!(
        reconciled.rows_shredded, 0,
        "the open after a clean close shredded a row"
    );
    assert_eq!(
        live == 1,
        file,
        "after a clean close: a live key row {live} and a file {file}"
    );
    let again = write(&mut reopened, &id, noon_today());
    assert!(
        again.is_ok(),
        "after a clean close the retry answered {}",
        said(&again)
    );
    assert_ordinal_above(&again, named, "after a clean close");
    assert_held(&reopened, &store_dir, &id, "the retry after a clean close");
    let at_the_answer = dir.path().join("after-the-retry-as-a-kill-leaves-it");
    copy_tree(&store_dir, &at_the_answer);
    assert_eq!(
        (
            live_rows_in_the_files(&at_the_answer, &key, &id),
            segment_path(&at_the_answer, id).exists()
        ),
        (1, true),
        "the files as a kill after the retry's Ok leaves them"
    );
    drop(reopened);
    let again_reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    assert_held(
        &again_reopened,
        &store_dir,
        &id,
        "a close and a reopen after the retry",
    );
}

#[test]
fn a_batch_whose_first_commit_meets_an_unlinked_log_is_answered_not_held_and_its_row_and_file_agree(
) {
    batch_after_the_log_left("unlinked", Taken::Unlinked);
}

#[test]
fn a_batch_whose_first_commit_meets_a_log_moved_out_is_answered_not_held_and_its_row_and_file_agree(
) {
    batch_after_the_log_left("moved-out", Taken::MovedOut);
}

#[test]
fn a_legacy_write_whose_first_commit_meets_an_unlinked_log_leaves_rows_and_files_in_agreement() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let key = *store.database_key().unwrap();
    std::fs::remove_file(store_dir.join("store.db-wal")).unwrap();
    let (live_before, files_before) =
        (live_rows_of_all(&store).unwrap(), segment_files(&store_dir));
    let first = store
        .write_segment(&mut Cursor::new(b"a legacy write"))
        .map(|_| ());
    let (live_after, files_after) = (live_rows_of_all(&store), segment_files(&store_dir));
    let second = store
        .write_segment(&mut Cursor::new(b"a second legacy write"))
        .map(|_| ());
    let copy = dir.path().join("as-a-kill-leaves-it");
    copy_tree(&store_dir, &copy);
    let copy_live: i64 = {
        let conn = Connection::open(copy.join("store.db")).unwrap();
        conn.execute_batch(&key_statement(&key)).unwrap();
        conn.query_row(
            "SELECT count(*) FROM segment_keys WHERE wrapped_key IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    println!(
        "RC1-legacy first={first:?} live_before={live_before} live_after={live_after:?} files_before={files_before} files_after={files_after} second={second:?} copy_live={copy_live} copy_files={}",
        segment_files(&copy)
    );
    assert!(
        matches!(&first, Err(e) if the_log_obstructed(e)),
        "the first answer: {first:?}"
    );
    assert_eq!(
        live_after.map_or(files_after - files_before, |live| live - live_before),
        files_after - files_before,
        "the handle shows live key rows and files that disagree"
    );
    assert!(
        second.is_err(),
        "a second write on the same handle answered {second:?}"
    );
    assert_eq!(
        copy_live,
        segment_files(&copy),
        "the files as a kill leaves them"
    );
    drop(store);
    let mut reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    assert_eq!(
        reopened.reconciled().rows_shredded,
        0,
        "the open after a clean close"
    );
    assert_eq!(
        live_rows_of_all(&reopened).unwrap(),
        segment_files(&store_dir),
        "after a clean close"
    );
    assert!(reopened
        .write_segment(&mut Cursor::new(b"a later legacy write"))
        .is_ok());
}

fn the_kept_key(
    store_dir: &Path,
    roots_dir: &Path,
    root_id: super::RootId,
    kept: Uuid,
) -> ([u8; 32], Vec<u8>) {
    let store = open_over_file_root(store_dir, roots_dir, root_id).unwrap();
    let key = *store.database_key().unwrap();
    let wrapped: Vec<u8> = store
        .db()
        .unwrap()
        .connection()
        .query_row(
            "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
            [kept.simple().to_string()],
            |r| r.get(0),
        )
        .unwrap();
    (key, wrapped)
}

fn in_the_pages(store_dir: &Path, key: &[u8; 32], wrapped: &[u8]) -> Vec<(&'static str, u32)> {
    places_of(&store_pages(store_dir, key), &[wrapped]).remove(0)
}

#[test]
fn an_open_whose_reconcile_shred_meets_a_log_that_left_its_name_answers_obstructed_and_leaves_the_directory_as_it_was(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, kept) = file_root_store(dir.path());
    let (key, wrapped) = the_kept_key(&store_dir, &roots_dir, root_id, kept);
    std::fs::remove_file(segment_path(&store_dir, kept)).unwrap();
    let before = tree(&store_dir);
    let unlink = {
        let store_dir = store_dir.clone();
        move || {
            let log = store_dir.join("store.db-wal");
            if log.exists() {
                std::fs::remove_file(&log).unwrap();
            }
        }
    };
    let seam = failpoint::at(SEAM_AT_THE_LOG_CHECK, nth_call(1, unlink));
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id);
    drop(seam);
    let answer = match &opened {
        Ok(store) => format!("Ok({:?})", store.reconciled()),
        Err(e) => format!("Err({e:?})"),
    };
    let obstructed = matches!(&opened, Err(e) if the_log_obstructed(e));
    drop(opened);
    let after = tree(&store_dir);
    let places = in_the_pages(&store_dir, &key, &wrapped);
    let names = |listed: &[(String, Option<Vec<u8>>)]| {
        listed
            .iter()
            .map(|(name, bytes)| (name.clone(), bytes.as_ref().map(Vec::len)))
            .collect::<Vec<_>>()
    };
    println!(
        "RC1-reconcile answer={answer} before={:?} after={:?} places_of_the_wrapped_key_after={places:?}",
        names(&before),
        names(&after)
    );
    assert!(obstructed, "the open answered {answer}");
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
        !places.is_empty(),
        "the refused open's row is gone from the pages"
    );
    let reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let places = in_the_pages(&store_dir, &key, &wrapped);
    println!(
        "RC1-reconcile next_open reconciled={:?} places_of_the_wrapped_key={places:?}",
        reopened.reconciled()
    );
    assert_eq!(
        reopened.reconciled(),
        Reconciled {
            files_removed: 0,
            rows_shredded: 1,
            owed: false
        }
    );
    assert!(
        places.is_empty(),
        "the next open answered Ok with the wrapped key in {places:?}"
    );
}

#[test]
fn an_open_whose_reconcile_shred_meets_its_log_answers_the_true_count_with_clean_pages() {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, kept) = file_root_store(dir.path());
    let (key, wrapped) = the_kept_key(&store_dir, &roots_dir, root_id, kept);
    std::fs::remove_file(segment_path(&store_dir, kept)).unwrap();
    let opened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let places = in_the_pages(&store_dir, &key, &wrapped);
    println!(
        "RC1-reconcile-log-in-place reconciled={:?} places_of_the_wrapped_key={places:?}",
        opened.reconciled()
    );
    assert_eq!(
        opened.reconciled(),
        Reconciled {
            files_removed: 0,
            rows_shredded: 1,
            owed: false
        }
    );
    assert!(
        places.is_empty(),
        "the open answered Ok with the wrapped key in {places:?}"
    );
}

#[test]
fn a_pass_whose_first_commit_meets_an_unlinked_log_answers_obstructed_and_its_close_leaves_store_db_as_it_was(
) {
    let dir = tempfile::tempdir().unwrap();
    let (store_dir, roots_dir, root_id, _) = file_root_store(dir.path());
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let key = *store.database_key().unwrap();
    let due = noon_today() - DEFAULT_RETENTION.as_nanos() as i64 - 2 * NANOS_PER_DAY;
    let id = random_segment_id().unwrap();
    write(&mut store, &id, due).unwrap();
    let wrapped: Vec<u8> = store
        .db()
        .unwrap()
        .connection()
        .query_row(
            "SELECT wrapped_key FROM segment_keys WHERE segment_id = ?1",
            [id.simple().to_string()],
            |r| r.get(0),
        )
        .unwrap();
    drop(store);
    let mut store = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    std::fs::remove_file(store_dir.join("store.db-wal")).unwrap();
    let database_before = std::fs::read(store_dir.join("store.db")).unwrap();
    let pass = store.crypto_shred_expired();
    let other = random_segment_id().unwrap();
    let write_after = write(&mut store, &other, noon_today());
    let read_after = store.read_segment(id, &mut Vec::new()).map(|_| ());
    let pass_after = store.crypto_shred_expired();
    drop(store);
    let database_after = std::fs::read(store_dir.join("store.db")).unwrap();
    println!(
        "RC1-pass pass={pass:?} write_after={} read_after={read_after:?} pass_after={pass_after:?} store_db_same={}",
        said(&write_after),
        database_after == database_before
    );
    assert!(
        matches!(&pass, Err(e) if the_log_obstructed(e)),
        "the pass answered {pass:?}"
    );
    assert!(
        !held(&write_after),
        "a later write on that handle answered {}",
        said(&write_after)
    );
    assert!(
        read_after.is_err(),
        "a later read on that handle answered {read_after:?}"
    );
    assert!(
        pass_after.is_err(),
        "a later pass on that handle answered {pass_after:?}"
    );
    assert!(
        database_after == database_before,
        "the close of the handle changed store.db"
    );
    let mut reopened = open_over_file_root(&store_dir, &roots_dir, root_id).unwrap();
    let places = in_the_pages(&store_dir, &key, &wrapped);
    println!(
        "RC1-pass next_open reconciled={:?} places_of_the_wrapped_key={places:?}",
        reopened.reconciled()
    );
    assert_eq!(reopened.reconciled(), Reconciled::default());
    assert_eq!(
        live_rows_of(&reopened, &id),
        Some(1),
        "the row after the refused pass"
    );
    let pass = reopened.crypto_shred_expired().unwrap();
    let places = in_the_pages(&store_dir, &key, &wrapped);
    println!("RC1-pass next_pass={pass:?} places_of_the_wrapped_key={places:?}");
    assert_eq!(
        pass,
        ShredReport {
            segments_shredded: 1,
            keys_destroyed: 1
        }
    );
    assert!(
        places.is_empty(),
        "the pass answered Ok with the wrapped key in {places:?}"
    );
}
