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
