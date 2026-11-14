use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection};

use super::{connect, rules, Mode, Rng, Scratch};

fn checkpoint(conn: &Connection) {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(busy, 0);
}

fn one(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn body(rng: &mut Rng, marker: &str) -> String {
    let length = match rng.below(4) {
        0 => 8180 + rng.below(400) as usize,
        _ => 20 + rng.below(900) as usize,
    };
    let mut text = marker.to_owned();
    while text.len() < length {
        text.push(char::from(b'b' + rng.below(5) as u8));
    }
    text
}

fn markers(scratch: &Scratch) -> BTreeMap<String, usize> {
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(&scratch.0).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for at in memchr::memchr_iter(b'h', &bytes) {
            if let Some(marker) = bytes.get(at..at + 8) {
                if marker[1..7].iter().all(u8::is_ascii_digit) && marker[7] == b'q' {
                    *found
                        .entry(String::from_utf8_lossy(marker).into_owned())
                        .or_insert(0) += 1;
                }
            }
        }
    }
    found
}

fn search(
    conn: &Connection,
    scratch: &Scratch,
    mode: Mode,
    places: &BTreeMap<String, usize>,
    when: &str,
) -> Vec<String> {
    let mut found = Vec::new();
    let log = scratch.0.join("page-gap.db-wal");
    if std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0) > 0 {
        found.push(format!("{when}: the log is not empty"));
    }
    for (marker, count) in markers(scratch) {
        let allowed = places.get(&marker).copied().unwrap_or(0);
        if count > allowed {
            found.push(format!(
                "{when}: the marker {marker} stands {count} times, where its rows allow {allowed}"
            ));
        }
    }
    if mode == Mode::Plain {
        let db = std::fs::read(scratch.db()).unwrap();
        for line in rules::check(conn, &db).problems {
            found.push(format!("{when}: structural check: {line}"));
        }
    }
    found
}

fn copied_rows(mode: Mode) -> Vec<String> {
    let scratch = Scratch::new(31, mode);
    let conn = connect(&scratch.db(), mode);
    conn.execute_batch(
        "CREATE TABLE a(id INTEGER PRIMARY KEY, body TEXT NOT NULL);
         CREATE TABLE b(id INTEGER PRIMARY KEY, body TEXT NOT NULL);",
    )
    .unwrap();
    let mut rng = Rng::new(31);
    let mut places: BTreeMap<String, usize> = BTreeMap::new();
    conn.execute_batch("BEGIN").unwrap();
    for id in 1..=60i64 {
        let marker = format!("h{id:06}q");
        conn.execute(
            "INSERT INTO a(id, body) VALUES (?1, ?2)",
            params![id, body(&mut rng, &marker)],
        )
        .unwrap();
        places.insert(marker, 2);
    }
    conn.execute_batch("COMMIT").unwrap();
    let opcodes: Vec<String> = {
        let mut statement = conn
            .prepare("EXPLAIN INSERT INTO b SELECT * FROM a")
            .unwrap();
        let rows = statement
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        rows
    };
    assert!(
        opcodes.iter().any(|op| op == "RowCell"),
        "the copy does not take the transfer path: {opcodes:?}"
    );
    conn.execute_batch("INSERT INTO b SELECT * FROM a;")
        .unwrap();
    checkpoint(&conn);
    assert!(
        one(
            &conn,
            "SELECT count(*) FROM dbstat WHERE name = 'b' AND pagetype = 'overflow'"
        ) > 0,
        "the copy holds no overflow page"
    );
    assert!(
        one(&conn, "SELECT count(*) FROM b WHERE length(body) < 1000") > 0,
        "the copy holds no row that is wholly local"
    );
    let mut found = search(&conn, &scratch, mode, &places, "after the copy");
    conn.execute_batch("DELETE FROM a;").unwrap();
    checkpoint(&conn);
    for allowed in places.values_mut() {
        *allowed = 1;
    }
    found.extend(search(
        &conn,
        &scratch,
        mode,
        &places,
        "after a was emptied",
    ));
    let ids: Vec<i64> = (1..=60).collect();
    for id in ids {
        conn.execute("DELETE FROM b WHERE id = ?1", [id]).unwrap();
        checkpoint(&conn);
        places.insert(format!("h{id:06}q"), 0);
        found.extend(search(
            &conn,
            &scratch,
            mode,
            &places,
            &format!("after the removal of copied row {id}"),
        ));
        if !found.is_empty() {
            break;
        }
    }
    found
}

fn vacuumed_rows(mode: Mode) -> Vec<String> {
    let scratch = Scratch::new(32, mode);
    let conn = connect(&scratch.db(), mode);
    conn.execute_batch(
        "CREATE TABLE t(id INTEGER PRIMARY KEY, kind INTEGER NOT NULL, body TEXT NOT NULL);
         CREATE INDEX t_kind_body ON t(kind, body);",
    )
    .unwrap();
    let mut rng = Rng::new(32);
    let mut places: BTreeMap<String, usize> = BTreeMap::new();
    conn.execute_batch("BEGIN").unwrap();
    for id in 1..=120i64 {
        let marker = format!("h{id:06}q");
        conn.execute(
            "INSERT INTO t(id, kind, body) VALUES (?1, ?2, ?3)",
            params![id, rng.below(3) as i64, body(&mut rng, &marker)],
        )
        .unwrap();
        places.insert(marker, 2);
    }
    conn.execute_batch("COMMIT").unwrap();
    let removed: BTreeSet<i64> = (1..=120).filter(|id| id % 3 != 0).collect();
    conn.execute_batch("BEGIN").unwrap();
    for id in &removed {
        conn.execute("DELETE FROM t WHERE id = ?1", [id]).unwrap();
        places.insert(format!("h{id:06}q"), 0);
    }
    conn.execute_batch("COMMIT").unwrap();
    checkpoint(&conn);
    assert!(
        one(&conn, "PRAGMA freelist_count") > 0,
        "the removals freed no page before the vacuum"
    );
    let mut found = search(&conn, &scratch, mode, &places, "before the vacuum");
    conn.execute_batch("VACUUM;").unwrap();
    checkpoint(&conn);
    assert_eq!(
        one(&conn, "PRAGMA freelist_count"),
        0,
        "the vacuum left free pages"
    );
    found.extend(search(&conn, &scratch, mode, &places, "after the vacuum"));
    let kept: Vec<i64> = (1..=120).filter(|id| id % 3 == 0).collect();
    for id in kept {
        conn.execute("DELETE FROM t WHERE id = ?1", [id]).unwrap();
        checkpoint(&conn);
        places.insert(format!("h{id:06}q"), 0);
        found.extend(search(
            &conn,
            &scratch,
            mode,
            &places,
            &format!("after the vacuum and the removal of row {id}"),
        ));
        if !found.is_empty() {
            break;
        }
    }
    found
}

#[test]
fn rows_copied_by_insert_select_leave_no_text_after_their_removal() {
    crate::initialize().unwrap();
    let found = copied_rows(Mode::Plain);
    assert!(found.is_empty(), "{}", found.join("\n"));
    let control = copied_rows(Mode::Unsecured);
    assert!(
        control
            .iter()
            .any(|line| line.contains("where its rows allow")),
        "with secure delete off the search finds no removed copied row"
    );
}

#[test]
fn rows_removed_before_a_vacuum_and_after_it_leave_no_text() {
    crate::initialize().unwrap();
    let found = vacuumed_rows(Mode::Plain);
    assert!(found.is_empty(), "{}", found.join("\n"));
    let control = vacuumed_rows(Mode::Unsecured);
    assert!(
        control
            .iter()
            .any(|line| line.contains("where its rows allow")),
        "with secure delete off the search finds no removed row"
    );
}
