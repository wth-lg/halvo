use std::time::Duration;

use halvo_sqlcipher::rusqlite::{Connection, ErrorCode};

use crate::db::{require, Progress};
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
