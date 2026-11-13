use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use halvo_schema_core::kinds;
use halvo_schema_core::record::{Instant, Proposal, RecurrenceStep, SummaryRead};
use halvo_sqlcipher::rusqlite::{self, Connection, ErrorCode};

use crate::db::{due_now, require, Now, Progress, Tx};
use crate::error::OpenError;
use crate::{sql, Clock, DbKey, Host, IdSource, Store};

#[test]
fn no_catalogued_statement_plans_a_temporary_b_tree() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(sql::SCHEMA).unwrap();
    assert!(sql::CATALOGUE.len() >= 7);
    for (name, statement) in sql::CATALOGUE {
        let mut plan = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {}", statement.text()))
            .unwrap_or_else(|e| panic!("{name} does not prepare: {e}"));
        let mut found = plan.raw_query();
        let mut details: Vec<String> = Vec::new();
        while let Some(row) = found.next().unwrap() {
            details.push(row.get(3).unwrap());
        }
        for detail in &details {
            assert!(
                !detail.contains("TEMP B-TREE") && !detail.contains("AUTOMATIC"),
                "{name} plans {detail}"
            );
        }
    }
}

#[test]
fn the_progress_handler_interrupts_a_statement_past_its_budget() {
    let mut conn = Connection::open_in_memory().unwrap();
    let progress = Progress::install(&mut conn, Duration::from_secs(30)).unwrap();
    let count = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 200000) \
                 SELECT count(*) FROM n";
    let within: i64 = conn.query_row(count, [], |r| r.get(0)).unwrap();
    assert_eq!(within, 200_000);
    progress.arm(Duration::ZERO);
    std::thread::sleep(Duration::from_millis(2));
    let past = conn.query_row(count, [], |r| r.get::<_, i64>(0));
    assert_eq!(
        past.unwrap_err().sqlite_error_code(),
        Some(ErrorCode::OperationInterrupted)
    );
}

#[test]
fn a_setting_that_is_not_in_effect_is_refused() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA secure_delete = OFF;").unwrap();
    assert_eq!(
        require(&conn, "secure_delete", "1"),
        Err(OpenError::Setting("secure_delete"))
    );
    conn.execute_batch("PRAGMA secure_delete = ON;").unwrap();
    assert_eq!(require(&conn, "secure_delete", "1"), Ok(()));
}

const TODAY: i64 = 20_767;
const NOW: Now = Now {
    now: TODAY * 86_400_000_000_000 + 43_200_000_000_000,
    today: TODAY,
};

/// The store's tables in memory, with rows written by hand: kind, class, day and expiry instant.
fn hand_rows(rows: &[(&str, i64, Option<i64>, Option<i64>)]) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(sql::SCHEMA).unwrap();
    for (kind, class, day, expires) in rows {
        conn.execute(
            "INSERT INTO rows (kind, family, class, day, expires, body) VALUES (?1, 'test', ?2, ?3, ?4, '{}')",
            rusqlite::params![kind, class, day, expires],
        )
        .unwrap();
    }
    conn
}

fn kinds(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT kind FROM rows ORDER BY kind")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn expire_removes_and_counts_a_bounded_row_with_no_day_which_the_report_reads_as_due_now() {
    let conn = hand_rows(&[
        ("bounded", 1, None, None),
        ("kept", 2, None, None),
        ("window", 0, None, Some(i64::MAX)),
    ]);
    assert!(due_now(&conn, NOW).unwrap());
    let tx = Tx::new(&conn);
    assert_eq!(tx.expire(NOW).unwrap(), 1);
    assert_eq!(kinds(&conn), ["kept", "window"]);
    assert!(!due_now(&conn, NOW).unwrap());
    assert_eq!(tx.expire(NOW).unwrap(), 0);
}

#[test]
fn a_row_more_than_two_days_after_the_clock_is_due_now_and_one_two_days_after_it_is_not() {
    let conn = hand_rows(&[("two", 1, Some(TODAY + 2), None)]);
    assert!(!due_now(&conn, NOW).unwrap());
    let conn = hand_rows(&[("three", 1, Some(TODAY + 3), None)]);
    assert!(due_now(&conn, NOW).unwrap());
    assert_eq!(Tx::new(&conn).expire(NOW).unwrap(), 1);
    assert!(!due_now(&conn, NOW).unwrap());
}

const DAY_NS: i64 = 86_400_000_000_000;

/// The clock of the unit tests: noon of `TODAY`, standing still.
struct UnitClock;

impl Clock for UnitClock {
    fn now(&self) -> Instant {
        Instant(NOW.now)
    }

    fn day(&self, at: Instant) -> u32 {
        u32::try_from(at.0.div_euclid(DAY_NS)).unwrap_or(0)
    }

    fn day_start(&self, day: u32) -> Instant {
        Instant(i64::from(day) * DAY_NS)
    }
}

struct NoProposals;

impl RecurrenceStep for NoProposals {
    fn propose(&self, _summaries: &dyn SummaryRead) -> Vec<Proposal> {
        Vec::new()
    }
}

struct Counting(u8);

impl IdSource for Counting {
    fn fill(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            self.0 = self.0.wrapping_add(1);
            *byte = self.0;
        }
    }
}

/// A data directory of its own, one per call, under `tmp/` of the target directory beside the integration tests'
/// scratch: 12-test fails a test run that leaves an entry in its temporary directory. The test binary stands at
/// `<target>/<profile>/deps/<binary>`.
fn data_dir(name: &str) -> PathBuf {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed);
    let exe = std::env::current_exe().unwrap();
    let target = exe.ancestors().nth(3).unwrap();
    let path = target
        .join("tmp")
        .join(format!("derived-unit-{name}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn open_unit(dir: &Path) -> Store {
    let entries: Vec<_> = kinds::registry().collect();
    Store::open(
        dir,
        &DbKey::new([7; 32]),
        &entries,
        Host::new(Box::new(UnitClock), Box::new(NoProposals)),
        &mut Counting(0),
    )
    .unwrap()
}

/// `PRAGMA secure_delete` as the store's one connection, the one of its writes and its reads, gives it.
fn secure_delete(store: &Store) -> i64 {
    store
        .db
        .conn
        .query_row("PRAGMA secure_delete", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn the_store_s_own_connection_has_secure_delete_on_after_a_first_and_a_second_open() {
    let dir = data_dir("secure-delete");
    let store = open_unit(&dir);
    assert_eq!(secure_delete(&store), 1, "after the first open");
    drop(store);
    let store = open_unit(&dir);
    assert_eq!(
        secure_delete(&store),
        1,
        "after a second open of the same files"
    );
}

/// DS-4: the reads' condition at the level of the statement, for the rows that no call of the store can make: a
/// bounded row with no day is not handed out, and a kept row and a window row with no day are.
#[test]
fn rows_read_hands_out_a_kept_and_a_window_row_with_no_day_and_no_bounded_row_without_one() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(sql::SCHEMA).unwrap();
    for (body, class, day, gone, expires) in [
        ("bounded without a day", 1, None, None, None),
        ("bounded inside", 1, Some(TODAY), Some(TODAY + 1), None),
        ("bounded gone today", 1, Some(TODAY - 9), Some(TODAY), None),
        ("kept", 2, None, None, None),
        ("window inside", 0, None, None, Some(NOW.now + 1)),
        ("window at now", 0, None, None, Some(NOW.now)),
    ] {
        conn.execute(
            "INSERT INTO rows (kind, family, class, day, gone, expires, body) VALUES ('k', 'test', ?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![class, day, gone, expires, body],
        )
        .unwrap();
    }
    let read: Vec<String> = conn
        .prepare(sql::ROWS_READ.text())
        .unwrap()
        .query_map(crate::db::reading!(NOW, ":kind" => "k"), |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(read, ["bounded inside", "kept", "window inside"]);
    assert_eq!(Tx::new(&conn).expire(NOW).unwrap(), 3);
    let left: Vec<String> = conn
        .prepare("SELECT body FROM rows ORDER BY body")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        left, read,
        "the expiry leaves exactly what the read handed out"
    );
}

/// The plan of a statement as `EXPLAIN QUERY PLAN` gives it on the store's schema, one detail per line.
fn plan_of(statement: sql::Sql) -> Vec<String> {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(sql::SCHEMA).unwrap();
    let mut plan = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {}", statement.text()))
        .unwrap();
    let mut found = plan.raw_query();
    let mut details = Vec::new();
    while let Some(row) = found.next().unwrap() {
        details.push(row.get(3).unwrap());
    }
    details
}

/// DS-4 H2: the keyed row read searches the index of the read of the whole kind by its kind, as that read does, so it
/// walks no more than that read and sorts nothing.
#[test]
fn the_keyed_row_read_plans_the_search_of_the_whole_read_and_sorts_nothing() {
    let whole = plan_of(sql::ROWS_READ);
    assert_eq!(whole, ["SEARCH rows USING INDEX rows_read (kind=?)"]);
    assert_eq!(plan_of(sql::ROWS_READ_UNDER), whole);
}

/// RD-64 (a) and DS-4 at the level of the statement: the keyed row read hands out the rows of its kind whose workflow
/// column names its workflow and that the read of the whole kind hands out, and no row of another workflow, no row
/// whose column is empty while its text names the workflow, and no row of the family `workflow`.
#[test]
fn rows_read_under_hands_out_by_the_reads_condition_and_the_workflow_column_alone() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(sql::SCHEMA).unwrap();
    let w1 = "10000000000000000000000000000001";
    let w2 = "20000000000000000000000000000002";
    for (kind, family, body, class, day, gone, expires, workflow) in [
        (
            "k",
            "test",
            "bounded without a day",
            1,
            None,
            None,
            None,
            Some(w1),
        ),
        (
            "k",
            "test",
            "bounded inside",
            1,
            Some(TODAY),
            Some(TODAY + 1),
            None,
            Some(w1),
        ),
        (
            "k",
            "test",
            "bounded gone today",
            1,
            Some(TODAY - 9),
            Some(TODAY),
            None,
            Some(w1),
        ),
        (
            "k",
            "test",
            "bounded three ahead",
            1,
            Some(TODAY + 3),
            Some(TODAY + 96),
            None,
            Some(w1),
        ),
        ("k", "test", "kept", 2, None, None, None, Some(w1)),
        ("k", "test", "kept of w2", 2, None, None, None, Some(w2)),
        (
            "k",
            "test",
            "kept, w1 in its text alone",
            2,
            None,
            None,
            None,
            None,
        ),
        (
            "k",
            "test",
            "window inside",
            0,
            None,
            None,
            Some(NOW.now + 1),
            Some(w1),
        ),
        (
            "k",
            "test",
            "window at now",
            0,
            None,
            None,
            Some(NOW.now),
            Some(w1),
        ),
        (
            "k",
            "workflow",
            "of the family workflow",
            2,
            None,
            None,
            None,
            Some(w1),
        ),
        (
            "workflow",
            "workflow",
            "workflow w1",
            2,
            None,
            None,
            None,
            Some(w1),
        ),
        (
            "workflow",
            "workflow",
            "workflow w2",
            2,
            None,
            None,
            None,
            Some(w2),
        ),
    ] {
        conn.execute(
            "INSERT INTO rows (kind, family, class, day, gone, expires, workflow, body) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![kind, family, class, day, gone, expires, workflow, body],
        )
        .unwrap();
    }
    let read = |statement: sql::Sql, workflow: &str| -> Vec<String> {
        conn.prepare(statement.text())
            .unwrap()
            .query_map(
                crate::db::reading!(NOW, ":kind" => "k", ":workflow" => workflow),
                |r| r.get(0),
            )
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(
        read(sql::ROWS_READ_UNDER, w1),
        ["bounded inside", "kept", "window inside"]
    );
    assert_eq!(read(sql::ROWS_READ_UNDER, w2), ["kept of w2"]);
    let whole: Vec<String> = conn
        .prepare(sql::ROWS_READ.text())
        .unwrap()
        .query_map(crate::db::reading!(NOW, ":kind" => "k"), |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        whole,
        [
            "bounded inside",
            "kept",
            "kept of w2",
            "kept, w1 in its text alone",
            "window inside"
        ]
    );
    assert_eq!(Tx::new(&conn).expire(NOW).unwrap(), 4);
    assert_eq!(
        read(sql::ROWS_READ_UNDER, w1),
        ["bounded inside", "kept", "window inside"],
        "the expiry leaves what the keyed read handed out"
    );
}

/// S-59 (a) 2 and DS-4 H2: the statement of the held-ahead call searches the rows of its kind by their day and sorts
/// nothing.
#[test]
fn the_held_ahead_statement_searches_its_kind_by_day_and_sorts_nothing() {
    assert_eq!(
        plan_of(sql::HELD_AHEAD),
        ["SEARCH rows USING INDEX rows_kind_day (kind=? AND day>?)"]
    );
}

/// S-59 (a) 2 at the level of the statement: it finds a row of its kind and workflow whose day lies more than two days
/// after the clock's day, and no row one or two days ahead, no row past its day of going, no bounded row with no day,
/// no row of another workflow or kind, and no row whose workflow column is empty while its text names the workflow.
#[test]
fn held_ahead_finds_only_a_row_of_its_kind_and_workflow_more_than_two_days_ahead() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(sql::SCHEMA).unwrap();
    let w1 = "10000000000000000000000000000001";
    let w2 = "20000000000000000000000000000002";
    let insert = |kind: &str, day: Option<i64>, gone: Option<i64>, workflow: Option<&str>| {
        conn.execute(
            "INSERT INTO rows (kind, family, class, day, gone, workflow, body) VALUES (?1, 'test', 1, ?2, ?3, ?4, ?5)",
            rusqlite::params![kind, day, gone, workflow, format!("{kind} {day:?} {workflow:?}")],
        )
        .unwrap();
    };
    let held = |kind: &str, workflow: &str| -> bool {
        let ahead = NOW.ahead_limit();
        conn.query_row(
            sql::HELD_AHEAD.text(),
            rusqlite::named_params! {":kind": kind, ":workflow": workflow, ":ahead": ahead},
            |_| Ok(()),
        )
        .is_ok()
    };
    insert("k", Some(TODAY + 1), Some(TODAY + 94), Some(w1));
    insert("k", Some(TODAY + 2), Some(TODAY + 95), Some(w1));
    insert("k", Some(TODAY - 93), Some(TODAY), Some(w1));
    insert("k", None, None, Some(w1));
    insert("k", Some(TODAY + 3), Some(TODAY + 96), None);
    insert("other", Some(TODAY + 3), Some(TODAY + 96), Some(w1));
    let before = (held("k", w1), held("k", w2), held("other", w2));
    insert("k", Some(TODAY + 3), Some(TODAY + 96), Some(w2));
    let after = (held("k", w1), held("k", w2), held("other", w2));
    assert_eq!(
        (before, after),
        ((false, false, false), (false, true, false)),
        "((k under w1, k under w2, other under w2) before and after a row of k three days ahead under w2)"
    );
}

/// A kind of the family `test` with a workflow key, for the held-ahead call on a store whose table is renamed away.
static HELD_NOTE: kinds::KindEntry = kinds::KindEntry {
    name: "test_note",
    class: kinds::RetentionClass::Bounded { days: 93 },
    origin: kinds::Origin::Capture,
    family: "test",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", kinds::FieldType::Version),
        ("workflow", kinds::FieldType::Id),
        ("day", kinds::FieldType::Day),
        ("n", kinds::FieldType::Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":20767,"n":1}"#,
};

/// A handler that asks the held-ahead call about `test_note` and keeps the answer.
struct HeldAsker(std::rc::Rc<std::cell::RefCell<Vec<Result<bool, crate::HeldRefusal>>>>);

impl crate::Handler for HeldAsker {
    fn handle(
        &self,
        _request: &halvo_schema_core::request::RawRequest,
        cx: &mut crate::HandlerCx<'_>,
    ) -> Result<Box<serde_json::value::RawValue>, halvo_schema_core::request::ErrorName> {
        let id =
            halvo_schema_core::ids::WorkflowId::parse("10000000000000000000000000000001").unwrap();
        self.0.borrow_mut().push(cx.held_ahead("test_note", &id));
        Ok(serde_json::value::RawValue::from_string("null".to_owned()).unwrap())
    }
}

fn request(name: &str, body: &str) -> halvo_schema_core::request::RawRequest {
    let text = format!(
        r#"{{"v":1,"id":"{}","request":"{name}","body":{body}}}"#,
        "5".repeat(32)
    );
    match halvo_schema_core::request::decode(text.as_bytes()) {
        Ok(halvo_schema_core::request::Decoded::Raw(raw)) => raw,
        _ => panic!("the request does not decode as a raw request"),
    }
}

/// S-66 E3 and E4. With the table of the rows renamed away through the store's own connection, so that every
/// statement on it fails: under a `runner` request and under a request of a further role the held-ahead call answers
/// `Role`, which comes before any statement (E3); under a `coordinator_write`, about a kind of the handler's own family
/// that has a workflow key, it answers the store's fault and never no (E4). The rename rewrites the table's stored
/// text, which a later open refuses, so the test takes a directory named by its start and removes it at its end.
#[test]
fn held_ahead_answers_role_before_any_statement_and_a_fault_of_the_database_never_as_no() {
    let answers = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let asker = || Box::new(HeldAsker(std::rc::Rc::clone(&answers)));
    let mut entries: Vec<_> = kinds::registry().collect();
    entries.push(&HELD_NOTE);
    let host = Host::new(Box::new(UnitClock), Box::new(NoProposals))
        .handler("test", crate::Takes::Runner, asker())
        .handler("test", crate::Takes::Role("test_role"), asker())
        .handler("test", crate::Takes::Coordinator(&["held"]), asker());
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = data_dir(&format!("held-fault-{started}"));
    let mut store =
        Store::open(&dir, &DbKey::new([7; 32]), &entries, host, &mut Counting(0)).unwrap();
    store
        .db
        .conn
        .execute_batch("ALTER TABLE rows RENAME TO rows_away")
        .unwrap();
    for (name, body) in [
        ("runner", "{}"),
        ("test_role", "{}"),
        ("coordinator_write", r#"{"op":"held"}"#),
    ] {
        store.handle(&request(name, body)).unwrap();
    }
    store
        .db
        .conn
        .execute_batch("ALTER TABLE rows_away RENAME TO rows")
        .unwrap();
    drop(store);
    std::fs::remove_dir_all(&dir).unwrap();
    let answers = answers.borrow().clone();
    assert_eq!(
        answers[..2],
        [Err(crate::HeldRefusal::Role), Err(crate::HeldRefusal::Role)],
        "E3: the runner request and the request of a further role"
    );
    assert!(
        matches!(answers[2], Err(crate::HeldRefusal::Fault(_))),
        "E4: the coordinator_write answered {:?}",
        answers[2]
    );
}
