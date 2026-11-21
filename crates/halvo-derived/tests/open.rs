mod common;

use common::{open, open_with, raw, registry, Scratch, Seq, TestClock};
use halvo_derived::{DbKey, Host, OpenError, Store};
use halvo_schema_core::kinds::{FieldType, KindEntry, Origin, RetentionClass};

static NOTE: KindEntry = KindEntry {
    name: "test_note",
    class: RetentionClass::Bounded { days: 94 },
    origin: Origin::Capture,
    family: "test",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("day", FieldType::Day),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":20767}"#,
};

fn names(scratch: &Scratch) -> Vec<(String, String)> {
    let conn = raw(scratch);
    let mut statement = conn
        .prepare("SELECT type, name FROM sqlite_master ORDER BY name")
        .unwrap();
    statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn open_creates_the_database_and_opens_it_again() {
    let scratch = Scratch::new("reopen");
    let clock = TestClock::at_day(20_000);
    drop(open(&scratch, &clock).unwrap());
    assert!(scratch.db().is_file());
    assert!(scratch.path().join("derived").join("writer.lock").is_file());
    drop(open(&scratch, &clock).unwrap());
}

#[test]
fn open_refuses_a_key_that_does_not_open_the_database() {
    let scratch = Scratch::new("wrong-key");
    let clock = TestClock::at_day(20_000);
    drop(open(&scratch, &clock).unwrap());
    let refused = Store::open(
        scratch.path(),
        &DbKey::new([8; 32]),
        &registry(),
        Host::new(Box::new(clock.clone()), Box::new(common::NoProposals)),
        &mut Seq(1),
    );
    assert_eq!(refused.err(), Some(OpenError::Sealed));
}

#[test]
fn open_refuses_while_another_store_holds_the_writer_lock() {
    let scratch = Scratch::new("second-open");
    let clock = TestClock::at_day(20_000);
    let first = open(&scratch, &clock).unwrap();
    assert_eq!(open(&scratch, &clock).err(), Some(OpenError::Locked));
    drop(first);
    drop(open(&scratch, &clock).unwrap());
}

#[test]
fn the_database_refuses_another_connection_while_the_store_is_open() {
    let scratch = Scratch::new("exclusive");
    let clock = TestClock::at_day(20_000);
    let store = open(&scratch, &clock).unwrap();
    let conn = raw(&scratch);
    let read = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    });
    assert!(read.is_err(), "a second connection read the database");
    drop(store);
}

#[test]
fn open_refuses_entries_that_check_entries_reports() {
    let scratch = Scratch::new("bad-entry");
    let clock = TestClock::at_day(20_000);
    let mut entries = registry();
    entries.push(&NOTE);
    match open_with(&scratch, &clock, &entries) {
        Err(OpenError::Entries(reports)) => assert_eq!(
            reports,
            vec![r#"kind "test_note": a bounded kind of origin capture with more than 93 days"#]
        ),
        other => panic!("open answered {:?}", other.err()),
    }
    assert!(!scratch.db().exists());
}

#[test]
fn open_refuses_entries_without_the_kinds_of_the_workflow_run() {
    let scratch = Scratch::new("no-summary");
    let clock = TestClock::at_day(20_000);
    let entries: Vec<&'static KindEntry> = registry()
        .into_iter()
        .filter(|entry| entry.name != "summary")
        .collect();
    assert_eq!(
        open_with(&scratch, &clock, &entries).err(),
        Some(OpenError::MissingKind("summary"))
    );
}

#[test]
fn open_refuses_a_database_with_another_schema() {
    let scratch = Scratch::new("other-schema");
    let clock = TestClock::at_day(20_000);
    std::fs::create_dir_all(scratch.path().join("derived")).unwrap();
    raw(&scratch)
        .execute_batch("CREATE TABLE other (x INTEGER);")
        .unwrap();
    assert_eq!(open(&scratch, &clock).err(), Some(OpenError::Schema));
}

#[test]
fn open_creates_no_index_in_a_database_that_holds_tables() {
    let scratch = Scratch::new("no-late-index");
    let clock = TestClock::at_day(20_000);
    drop(open(&scratch, &clock).unwrap());
    let before = names(&scratch);
    assert!(before.contains(&("index".to_owned(), "rows_day".to_owned())));
    raw(&scratch).execute_batch("DROP INDEX rows_day;").unwrap();
    assert_eq!(open(&scratch, &clock).err(), Some(OpenError::Schema));
    let after = names(&scratch);
    assert!(!after.contains(&("index".to_owned(), "rows_day".to_owned())));
    assert_eq!(after.len() + 1, before.len());
}

#[test]
fn the_database_holds_no_full_text_index_and_no_shared_memory_file() {
    let scratch = Scratch::new("no-fts");
    let clock = TestClock::at_day(20_000);
    let store = open(&scratch, &clock).unwrap();
    assert!(!scratch
        .path()
        .join("derived")
        .join("derived.db-shm")
        .exists());
    drop(store);
    let conn = raw(&scratch);
    let virtual_tables: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE sql LIKE '%VIRTUAL%' OR name LIKE '%fts%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(virtual_tables, 0);
    assert!(names(&scratch).len() > 5);
}

#[test]
fn open_removes_rows_that_name_a_summary_or_a_workflow_that_is_not_stored() {
    let scratch = Scratch::new("reconcile");
    let clock = TestClock::at_day(20_000);
    drop(open(&scratch, &clock).unwrap());
    raw(&scratch)
        .execute_batch(
            "INSERT INTO rows (kind, family, class, expires, summary, body) VALUES \
             ('evidence', 'workflow', 0, 9000000000000000000, '20000000000000000000000000000001', '{}');\
             INSERT INTO rows (kind, family, class, day, workflow, body) VALUES \
             ('agent_run', 'agent', 1, 20000, '30000000000000000000000000000001', \
             json_object('workflow', '30000000000000000000000000000001'));\
             INSERT INTO rows (kind, family, class, day, body) VALUES ('test_kept', 'test', 1, 20000, '{}');",
        )
        .unwrap();
    drop(open(&scratch, &clock).unwrap());
    let conn = raw(&scratch);
    let kinds: Vec<String> = conn
        .prepare("SELECT kind FROM rows")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(kinds, vec!["test_kept".to_owned()]);
    let families: Vec<(String, i64)> = conn
        .prepare("SELECT family, n FROM revisions ORDER BY family")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut expected: Vec<(String, i64)> = common::families(&registry())
        .into_iter()
        .map(|f| (f.to_owned(), i64::from(f == "agent" || f == "workflow")))
        .collect();
    expected.sort();
    assert_eq!(
        families, expected,
        "the open raises the revision of each family that lost a row, the agent row's and the evidence row's, by \
         one, and moves no other family of its entries; the table holds the entries' families and no more"
    );
}

/// A keyless database made by a connection of the test's own with `pragma`, holding no table (one is made and dropped,
/// so that the setting is written into the file), and what the keyless open of the test host answers for it.
fn keyless_open_of_a_file_made_with(name: &str, pragma: &str) -> Result<(), OpenError> {
    let scratch = Scratch::new(name);
    std::fs::create_dir_all(scratch.path().join("derived")).unwrap();
    halvo_derived::test_host::start_sqlite();
    let conn = halvo_sqlcipher::rusqlite::Connection::open(scratch.db()).unwrap();
    conn.execute_batch(&format!("{pragma} CREATE TABLE t(x); DROP TABLE t;"))
        .unwrap();
    drop(conn);
    let host = Host::new(
        Box::new(TestClock::at_day(20_767)),
        Box::new(common::NoProposals),
    );
    halvo_derived::test_host::open_keyless(scratch.path(), &registry(), host, &mut Seq(1)).map(drop)
}

/// S-85, item 5: the open refuses by name a store whose `auto_vacuum` reads otherwise than 0, here a keyless file made
/// with `auto_vacuum` FULL, and opens one made with it NONE.
#[test]
fn open_refuses_a_store_whose_auto_vacuum_is_not_0() {
    assert_eq!(
        keyless_open_of_a_file_made_with("auto-vacuum-full", "PRAGMA auto_vacuum = FULL;"),
        Err(OpenError::Setting("auto_vacuum"))
    );
    assert_eq!(
        keyless_open_of_a_file_made_with("auto-vacuum-none", "PRAGMA auto_vacuum = NONE;"),
        Ok(())
    );
}

/// S-85, item 5 and S-94 P3: the open refuses by name a store whose page size reads otherwise than 4,096, tested keyless
/// alone: under a key the database answers `PRAGMA page_size` itself, as the text of the cipher's page size, so a
/// keyed file of another page size is the cipher's setting and no test here makes one. A keyless file made with a page
/// size of 8,192 is refused and one made with 4,096 opens.
#[test]
fn open_refuses_a_keyless_store_whose_page_size_is_not_4096() {
    assert_eq!(
        keyless_open_of_a_file_made_with("page-size-8192", "PRAGMA page_size = 8192;"),
        Err(OpenError::Setting("page_size"))
    );
    assert_eq!(
        keyless_open_of_a_file_made_with("page-size-4096", "PRAGMA page_size = 4096;"),
        Ok(())
    );
}

/// Ruling S-100, item 6 and pr-review-26 N1: an open that `auto_vacuum`, the page size or the schema refuses changes no
/// byte of the database file, also of a file in rollback-journal mode. Three files, each made in that mode by a
/// connection of the test's own, as `keyless_open_of_a_file_made_with` and `open_refuses_a_database_with_another_schema`
/// make theirs: a keyless file with `auto_vacuum` FULL and one with a page size of 8,192, each opened by the keyless
/// open of the test host, and a file under the store's key that holds a table `other`, opened with that key. Each open
/// answers its refusal, and the bytes of derived.db after it are those before it.
#[test]
fn a_refused_open_of_a_rollback_journal_file_changes_no_byte() {
    let made = |name: &str, keyed: bool, sql: &str| -> Scratch {
        let scratch = Scratch::new(name);
        std::fs::create_dir_all(scratch.path().join("derived")).unwrap();
        let conn = if keyed {
            raw(&scratch)
        } else {
            halvo_derived::test_host::start_sqlite();
            halvo_sqlcipher::rusqlite::Connection::open(scratch.db()).unwrap()
        };
        conn.execute_batch(sql).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            mode, "delete",
            "{name}: the file is in rollback-journal mode"
        );
        scratch
    };
    let keyless = |scratch: &Scratch| {
        let host = Host::new(
            Box::new(TestClock::at_day(20_767)),
            Box::new(common::NoProposals),
        );
        halvo_derived::test_host::open_keyless(scratch.path(), &registry(), host, &mut Seq(1)).err()
    };
    let cases = [
        (
            made(
                "refused-auto-vacuum",
                false,
                "PRAGMA auto_vacuum = FULL; CREATE TABLE t(x); DROP TABLE t;",
            ),
            false,
            OpenError::Setting("auto_vacuum"),
        ),
        (
            made(
                "refused-page-size",
                false,
                "PRAGMA page_size = 8192; CREATE TABLE t(x); DROP TABLE t;",
            ),
            false,
            OpenError::Setting("page_size"),
        ),
        (
            made("refused-schema", true, "CREATE TABLE other (x INTEGER);"),
            true,
            OpenError::Schema,
        ),
    ];
    for (scratch, keyed, refusal) in cases {
        let before = std::fs::read(scratch.db()).unwrap();
        let answer = if keyed {
            open(&scratch, &TestClock::at_day(20_767)).err()
        } else {
            keyless(&scratch)
        };
        assert_eq!(answer, Some(refusal.clone()), "the open's answer");
        assert!(
            std::fs::read(scratch.db()).unwrap() == before,
            "the bytes of derived.db after the open that {refusal:?} refused"
        );
    }
}

/// The coordinator's rulings of 2026-11-21: an open with a key that does not open a stored database answers `Sealed`,
/// not `Locked`, while another holds the lock of derived/writer.lock, changes no byte of derived.db and leaves the
/// entries of derived/ as they were: the read-only test of the key stands in front of the writer lock. The store is
/// made and closed with the test's key, and the test holds the writer lock as
/// `an_open_is_refused_while_another_holds_the_writer_lock_and_the_database_is_free` of tests/privacy.rs does. The
/// control: an open with the test's key under the same lock answers `Locked` and changes no byte either.
#[test]
fn a_wrong_key_under_a_held_writer_lock_is_sealed_and_changes_no_byte() {
    let scratch = Scratch::new("wrong-key-held-lock");
    let clock = TestClock::at_day(20_000);
    drop(open(&scratch, &clock).unwrap());
    let entries = || -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(scratch.path().join("derived"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let before = std::fs::read(scratch.db()).unwrap();
    let listed = entries();
    let holder = std::fs::File::options()
        .write(true)
        .open(scratch.path().join("derived").join("writer.lock"))
        .unwrap();
    holder.try_lock().unwrap();
    let refused = Store::open(
        scratch.path(),
        &DbKey::new([8; 32]),
        &registry(),
        Host::new(Box::new(clock.clone()), Box::new(common::NoProposals)),
        &mut Seq(1),
    );
    assert_eq!(refused.err(), Some(OpenError::Sealed), "another key");
    assert!(
        std::fs::read(scratch.db()).unwrap() == before,
        "the bytes of derived.db after the open with another key"
    );
    assert_eq!(
        entries(),
        listed,
        "the entries of derived/ after the open with another key"
    );
    assert_eq!(
        open(&scratch, &clock).err(),
        Some(OpenError::Locked),
        "the control: the test's key under the held lock"
    );
    assert!(
        std::fs::read(scratch.db()).unwrap() == before,
        "the bytes of derived.db after the control"
    );
    assert_eq!(
        entries(),
        listed,
        "the entries of derived/ after the control"
    );
    holder.unlock().unwrap();
}
