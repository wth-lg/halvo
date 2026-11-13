use std::time::Duration;

use halvo_sqlcipher::rusqlite::{self, Connection, ErrorCode};

use crate::db::{due_now, require, Now, Progress, Tx};
use crate::error::OpenError;
use crate::sql;

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
