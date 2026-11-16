use std::path::{Path, PathBuf};
use std::time::Duration;

use halvo_schema_core::kinds;
use halvo_schema_core::record::{Instant, Proposal, RecurrenceStep, SummaryRead};
use halvo_sqlcipher::rusqlite::{self, Connection, ErrorCode};

use crate::db::{due_now, require, require_mend, Checkpointed, Now, Progress, Removes, Tx};
use crate::error::OpenError;
use crate::{sql, Clock, DbKey, Host, IdSource, Store};

/// An in-memory connection of a test, opened after SQLite's start-up has run to its end under the store's guard.
fn memory() -> Connection {
    crate::db::initialize_once();
    Connection::open_in_memory().unwrap()
}

#[test]
fn no_catalogued_statement_plans_a_temporary_b_tree() {
    let conn = memory();
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
    let mut conn = memory();
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

/// S-85, item 5: the open refuses by name a process whose linked database does not answer that it is Halvo's mended
/// build, and this build is that one. No test build links another library, so the call in `Db::open` is held by the
/// text of src/db.rs: the call stands once on a line of code, a line that is no comment, and that line lies in the
/// function `open` before the first line of `open` that makes or opens a file: the first line of code there that
/// names `std::fs::`, `WriterLock::take(` or `Connection::open(`, which at this head is the line that makes the
/// directory `derived/` by `std::fs::create_dir_all` (ruling S-100, item 5). Its limit: it reads text, so a call that
/// stands on a line of code and never runs, as one behind a
/// condition that is false, passes it; a test by behaviour would need a test-only switch in src/db.rs (ruling S-98,
/// item 2).
#[test]
fn the_open_refuses_a_database_library_without_the_mend_of_the_page_gap() {
    assert_eq!(
        require_mend(false),
        Err(OpenError::Setting("page_gap_mend_linked"))
    );
    assert_eq!(require_mend(true), Ok(()));
    assert!(halvo_sqlcipher::page_gap_mend_linked());
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("db.rs"),
    )
    .unwrap()
    .replace("\r\n", "\n");
    let call = "require_mend(halvo_sqlcipher::page_gap_mend_linked())?;";
    let code: Vec<(usize, &str)> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .collect();
    let calls: Vec<usize> = code
        .iter()
        .filter(|(_, line)| line.contains(call))
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(calls.len(), 1, "the call on a line of code of src/db.rs");
    let lines: Vec<&str> = source.lines().collect();
    let open = lines
        .iter()
        .position(|line| line.contains("pub(crate) fn open("))
        .unwrap();
    let end = open
        + lines[open..]
            .iter()
            .position(|line| *line == "    }")
            .unwrap();
    assert!(
        (open..end).contains(&calls[0]),
        "the call stands in Db::open"
    );
    let first_file = code
        .iter()
        .filter(|(n, _)| (open..end).contains(n))
        .find(|(_, line)| {
            ["std::fs::", "WriterLock::take(", "Connection::open("]
                .iter()
                .any(|maker| line.contains(maker))
        })
        .map(|(n, _)| *n)
        .unwrap();
    assert!(
        lines[first_file].contains("std::fs::create_dir_all("),
        "the first line of Db::open that makes or opens a file"
    );
    assert!(
        calls[0] < first_file,
        "the call stands before the first line of Db::open that makes or opens a file"
    );
}

#[test]
fn a_setting_that_is_not_in_effect_is_refused() {
    let conn = memory();
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
    let conn = memory();
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
    let plain = Tx::new(&conn);
    let tx = Checkpointed::for_test(&plain);
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
    assert_eq!(
        Checkpointed::for_test(&Tx::new(&conn)).expire(NOW).unwrap(),
        1
    );
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

/// A data directory of a unit test, removed when the guard is dropped.
struct UnitDir(PathBuf);

impl UnitDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for UnitDir {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            eprintln!(
                "the unit directory {} was not removed: {e}",
                self.0.display()
            );
        }
    }
}

/// A data directory of its own, one per call, under `tmp/` of the target directory beside the integration tests'
/// scratch: 12-test fails a test run that leaves an entry in its temporary directory. The test binary stands at
/// `<target>/<profile>/deps/<binary>`. The directory `derived-unit-<name>-<process id>-<n>` is made by
/// `halvo_test_dir::make`, which passes over a name that stands.
fn data_dir(name: &str) -> UnitDir {
    let exe = std::env::current_exe().unwrap();
    let parent = exe.ancestors().nth(3).unwrap().join("tmp");
    match halvo_test_dir::make(&parent, &format!("derived-unit-{name}")) {
        Ok(path) => UnitDir(path),
        Err(e) => panic!(
            "the unit directory derived-unit-{name} under {} cannot be made: {e}",
            parent.display()
        ),
    }
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
    let store = open_unit(dir.path());
    assert_eq!(secure_delete(&store), 1, "after the first open");
    drop(store);
    let store = open_unit(dir.path());
    assert_eq!(
        secure_delete(&store),
        1,
        "after a second open of the same files"
    );
}

/// `PRAGMA temp_store` is 2, memory, on the store's one connection after a first and after a second open of the same
/// files, so that no temporary table or index of a statement is written to a file outside `derived/`.
#[test]
fn the_store_s_own_connection_keeps_temporary_storage_in_memory_after_a_first_and_a_second_open() {
    let dir = data_dir("temp-store");
    let temp_store = |store: &Store| -> i64 {
        store
            .db
            .conn
            .query_row("PRAGMA temp_store", [], |r| r.get(0))
            .unwrap()
    };
    let store = open_unit(dir.path());
    assert_eq!(temp_store(&store), 2, "after the first open");
    drop(store);
    let store = open_unit(dir.path());
    assert_eq!(
        temp_store(&store),
        2,
        "after a second open of the same files"
    );
}

/// The settings of the store's connection that the open sets and reads back since U5.1d, part 6 (S-74 (e), S-78, item
/// 1), each with the value that it reads back as and another value that the read-back must refuse.
const READ_BACK: [(&str, &str, &str); 4] = [
    ("temp_store", "2", "FILE"),
    ("synchronous", "2", "NORMAL"),
    ("fullfsync", "1", "OFF"),
    ("checkpoint_fullfsync", "1", "OFF"),
];

/// A setting as the store's own connection reads it, as text.
fn setting(store: &Store, name: &str) -> String {
    store
        .db
        .conn
        .query_row(&format!("PRAGMA {name}"), [], |r| {
            r.get::<_, rusqlite::types::Value>(0)
        })
        .map(|value| match value {
            rusqlite::types::Value::Integer(n) => n.to_string(),
            rusqlite::types::Value::Text(s) => s,
            _ => String::new(),
        })
        .unwrap()
}

/// S-78, item 1 and S-74 (e): `temp_store` reads 2, memory, `synchronous` 2, FULL, and `fullfsync` and
/// `checkpoint_fullfsync` 1 on the store's own connection after a first and after a second open of the same files.
#[test]
fn the_store_s_own_connection_reads_each_setting_of_the_open_after_a_first_and_a_second_open() {
    let dir = data_dir("settings");
    for open in ["first", "second"] {
        let store = open_unit(dir.path());
        for (name, want, _) in READ_BACK {
            assert_eq!(setting(&store, name), want, "{name} after the {open} open");
        }
    }
}

/// S-78, item 1 and S-74 (e): the open's read-back refuses by name, with `OpenError::Setting`, a store whose
/// `temp_store`, `synchronous`, `fullfsync` or `checkpoint_fullfsync` reads otherwise than the open set it, each
/// changed alone on the store's own connection, and passes once it is set back.
#[test]
fn the_open_s_read_back_refuses_by_name_each_setting_that_reads_otherwise() {
    let dir = data_dir("settings-refused");
    let store = open_unit(dir.path());
    assert_eq!(crate::db::read_back(&store.db.conn), Ok(()));
    for (name, want, other) in READ_BACK {
        let set = |value: &str| {
            store
                .db
                .conn
                .execute_batch(&format!("PRAGMA {name} = {value};"))
                .unwrap()
        };
        set(other);
        assert_eq!(
            crate::db::read_back(&store.db.conn),
            Err(OpenError::Setting(name)),
            "{name} set to {other}"
        );
        set(want);
        assert_eq!(crate::db::read_back(&store.db.conn), Ok(()), "{name}");
    }
}

/// DS-4: the reads' condition at the level of the statement, for the rows that no call of the store can make: a
/// bounded row with no day is not handed out, and a kept row and a window row with no day are.
#[test]
fn rows_read_hands_out_a_kept_and_a_window_row_with_no_day_and_no_bounded_row_without_one() {
    let conn = memory();
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
    assert_eq!(
        Checkpointed::for_test(&Tx::new(&conn)).expire(NOW).unwrap(),
        3
    );
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
    let conn = memory();
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

/// S-67, part 2: the keyed row read searches the index `rows_read_workflow` by its kind and its workflow and sorts
/// nothing, so it walks the rows of one workflow of one kind and no row of another workflow; the read of the whole kind
/// keeps its search of `rows_read` by the kind.
#[test]
fn the_keyed_row_read_searches_its_index_by_kind_and_workflow_and_the_whole_read_searches_rows_read_by_kind(
) {
    assert_eq!(
        plan_of(sql::ROWS_READ),
        ["SEARCH rows USING INDEX rows_read (kind=?)"]
    );
    assert_eq!(
        plan_of(sql::ROWS_READ_UNDER),
        ["SEARCH rows USING INDEX rows_read_workflow (kind=? AND workflow=?)"]
    );
}

/// S-67, part 1: the index of the keyed read holds the rows of every family but `workflow`, as `rows_read` does, so no
/// body of a workflow stands in it: a statement that must use it for rows of the family `workflow` has no plan, and the
/// same statement for the rows of another family has one.
#[test]
fn the_index_of_the_keyed_read_holds_no_row_of_the_family_workflow() {
    let conn = memory();
    conn.execute_batch(sql::SCHEMA).unwrap();
    let forced = |family: &str| {
        conn.prepare(&format!(
            "SELECT body FROM rows INDEXED BY rows_read_workflow \
             WHERE kind = ?1 AND workflow = ?2 AND family {family}"
        ))
        .map(drop)
        .map_err(|e| e.to_string())
    };
    assert_eq!(forced("<> 'workflow'"), Ok(()));
    assert_eq!(forced("= 'workflow'"), Err("no query solution".to_owned()));
}

/// The steps of the database's own machine that the keyed row read of the workflow `w1`, with its three rows of the
/// kind `k`, takes on the store's schema in memory when `others` rows of the same kind and family name other
/// workflows, read to its end.
fn keyed_read_steps(others: usize) -> i32 {
    let conn = memory();
    conn.execute_batch(sql::SCHEMA).unwrap();
    let insert =
        "INSERT INTO rows (kind, family, class, workflow, body) VALUES ('k', 'test', 2, ?1, ?2)";
    for n in 0..3 {
        conn.execute(insert, rusqlite::params!["w1", format!("w1 row {n}")])
            .unwrap();
    }
    for n in 0..others {
        let workflow = format!("w{}", 2 + n % 50);
        conn.execute(
            insert,
            rusqlite::params![workflow, format!("other row {n:06}")],
        )
        .unwrap();
    }
    let mut statement = conn.prepare(sql::ROWS_READ_UNDER.text()).unwrap();
    let read: Vec<String> = statement
        .query_map(
            crate::db::reading!(NOW, ":kind" => "k", ":workflow" => "w1"),
            |r| r.get(0),
        )
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(read, ["w1 row 0", "w1 row 1", "w1 row 2"]);
    statement.get_status(rusqlite::StatementStatus::VmStep)
}

/// S-67, part 3, the cost without a clock: the keyed row read of one workflow takes the same number of the database's
/// own steps with 10 and with 2,000 rows of its kind under other workflows.
#[test]
fn the_keyed_row_read_takes_the_same_steps_however_many_rows_of_its_kind_other_workflows_hold() {
    let few = keyed_read_steps(10);
    let many = keyed_read_steps(2_000);
    assert!(few > 0);
    assert_eq!(
        few, many,
        "steps with 10 and with 2,000 rows of other workflows"
    );
}

/// RD-64 (a) and DS-4 at the level of the statement: the keyed row read hands out the rows of its kind whose workflow
/// column names its workflow and that the read of the whole kind hands out, and no row of another workflow, no row
/// whose column is empty while its text names the workflow, and no row of the family `workflow`.
#[test]
fn rows_read_under_hands_out_by_the_reads_condition_and_the_workflow_column_alone() {
    let conn = memory();
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
    assert_eq!(
        Checkpointed::for_test(&Tx::new(&conn)).expire(NOW).unwrap(),
        4
    );
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
    let conn = memory();
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
    let dir = data_dir("held-fault");
    let mut store = Store::open(
        dir.path(),
        &DbKey::new([7; 32]),
        &entries,
        host,
        &mut Counting(0),
    )
    .unwrap();
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
    drop(dir);
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

/// Two unit directories of one name in one process get two paths, and each is made new.
#[test]
fn two_unit_directories_of_one_name_in_one_process_get_two_paths() {
    let first = data_dir("twice");
    let second = data_dir("twice");
    assert_ne!(first.path(), second.path());
    assert!(first.path().is_dir() && second.path().is_dir());
}

/// What the text check of the crate's sources found: each line that names ANALYZE or the pragma optimize, each
/// declaration of `fn remove` with the trait or impl that holds it, each construction of the checkpointed writer with
/// the function that holds it, and the order of the commit and the truncating checkpoint in the checkpointed write.
#[derive(Debug, PartialEq, Eq)]
struct SourceText {
    statistics: Vec<String>,
    removes: Vec<(String, String)>,
    writers: Vec<(String, String)>,
    commit_then_checkpoint: bool,
    checkpoint_truncates: bool,
}

/// Reads every source file of the crate under `src/` but this file of tests, line by line, comment lines left out.
fn source_text() -> SourceText {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".rs") && name != "tests.rs")
        .collect();
    files.sort();
    assert!(files.len() >= 15, "the crate's source files: {files:?}");
    let mut found = SourceText {
        statistics: Vec::new(),
        removes: Vec::new(),
        writers: Vec::new(),
        commit_then_checkpoint: false,
        checkpoint_truncates: false,
    };
    for file in files {
        let text = std::fs::read_to_string(root.join(&file)).unwrap();
        let (mut block, mut function) = (String::new(), String::new());
        let (mut committed, mut checkpointed) = (false, false);
        for line in text.lines() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            let lower = code.to_ascii_lowercase();
            if lower.contains("analyze") || lower.contains("optimize") {
                found.statistics.push(format!("src/{file}: {code}"));
            }
            if code.starts_with("impl") || code.starts_with("pub(crate) trait") {
                block = code.to_owned();
            }
            if let Some(rest) = code.split("fn ").nth(1).filter(|_| code.contains("fn ")) {
                function = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                if function == "remove" {
                    found.removes.push((format!("src/{file}"), block.clone()));
                }
            }
            for (at, _) in code.match_indices("Checkpointed(") {
                let before = &code[..at];
                if !before.ends_with("struct ") && !before.ends_with('_') {
                    found
                        .writers
                        .push((format!("src/{file}"), function.clone()));
                }
            }
            if function == "write_checkpointed" {
                committed |= code.contains("self.commit(");
                checkpointed |= committed && code.contains("self.checkpoint()?");
            }
            if function == "checkpoint" && code.contains("wal_checkpoint(TRUNCATE)") {
                found.checkpoint_truncates = true;
            }
        }
        found.commit_then_checkpoint |= checkpointed;
    }
    found
}

/// S-83, item 4, S-82 6 (e) and C2.4 (U5.1d, part 8), by the crate's text: no source of the store runs ANALYZE or
/// PRAGMA optimize; `remove` is declared in the trait `Removes` and in its one impl, for the writer that the
/// checkpointed write hands out; that writer is made in the checkpointed write alone (and by the constructor of the
/// unit tests); and the checkpointed write commits and then runs the truncating checkpoint. Its limit: it reads text,
/// so a statement built from parts or a removal through `exec` of a statement that begins with `DELETE` is not seen
/// here; the second is the test of S-81, F2 in tests/privacy.rs.
#[test]
fn no_source_runs_analyze_or_optimize_and_every_removal_stands_under_the_truncating_checkpoint() {
    let found = source_text();
    assert_eq!(
        found,
        SourceText {
            statistics: Vec::new(),
            removes: vec![
                (
                    "src/db.rs".to_owned(),
                    "pub(crate) trait Removes {".to_owned()
                ),
                (
                    "src/db.rs".to_owned(),
                    "impl Removes for Checkpointed<'_, '_> {".to_owned()
                ),
            ],
            writers: vec![
                ("src/db.rs".to_owned(), "for_test".to_owned()),
                ("src/db.rs".to_owned(), "write_checkpointed".to_owned()),
            ],
            commit_then_checkpoint: true,
            checkpoint_truncates: true,
        }
    );
}

/// Whether a writer type has the removal of rows: the inherent constant where the type implements `Removes`, the
/// trait's constant of `false` where it does not.
struct Probe<T>(std::marker::PhantomData<T>);

trait NoRemoval {
    const REMOVES: bool = false;
}

impl<T> NoRemoval for Probe<T> {}

impl<T: Removes> Probe<T> {
    const REMOVES: bool = true;
}

/// S-82 6 (g) and S-83, item 4 (U5.1d, part 8): the writer of a plain write has no removal of rows, so that a call of
/// `remove` under `Db::write` does not compile, and the writer that the checkpointed write hands out has it.
#[test]
fn a_removal_compiles_under_the_checkpointed_write_alone() {
    let removes = [
        Probe::<Tx<'static>>::REMOVES,
        Probe::<Checkpointed<'static, 'static>>::REMOVES,
    ];
    assert_eq!(
        removes,
        [false, true],
        "(the plain writer removes, the checkpointed writer removes)"
    );
}
