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
