use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{params, Connection};

use super::{connect, layout, Mode, Rng, Scratch, PAGE};

fn token(rng: &mut Rng) -> String {
    (0..16)
        .map(|_| char::from(b'a' + rng.below(26) as u8))
        .collect()
}

fn checkpoint(conn: &Connection) {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(busy, 0);
}

fn places(conn: &Connection, dir: &Path, removed: &BTreeSet<String>, when: &str) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut found: Vec<String> =
        layout::leftovers(conn, &std::fs::read(dir.join("page-gap.db")).unwrap())
            .into_iter()
            .map(|line| format!("{when}: structural check: {line}"))
            .collect();
    for name in names {
        let bytes = std::fs::read(dir.join(&name)).unwrap();
        for text in removed {
            for at in memchr::memmem::find_iter(&bytes, text.as_bytes()) {
                found.push(format!(
                    "{when}: the removed text {text} stands in {name} page {} offset {}",
                    at / PAGE + 1,
                    at % PAGE
                ));
            }
        }
    }
    found
}

fn small_rows_with_single_deletes(seed: u64) -> Vec<String> {
    let scratch = Scratch::new(seed, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch("CREATE TABLE c(id INTEGER PRIMARY KEY, c0 TEXT NOT NULL);")
        .unwrap();
    let mut rng = Rng::new(seed);
    let mut live: BTreeMap<i64, String> = BTreeMap::new();
    let mut removed: BTreeSet<String> = BTreeSet::new();
    let mut next = 1i64;
    for round in 0..3 {
        conn.execute_batch("BEGIN").unwrap();
        for _ in 0..120 {
            let text = token(&mut rng);
            let row = format!("{text} app{}", rng.below(7));
            conn.execute("INSERT INTO c(id, c0) VALUES (?1, ?2)", params![next, row])
                .unwrap();
            live.insert(next, text);
            next += 1;
        }
        conn.execute_batch("COMMIT").unwrap();
        for step in 0..60 {
            let at = rng.below(live.len() as u64) as usize;
            let id = *live.keys().nth(at).unwrap();
            conn.execute("DELETE FROM c WHERE id = ?1", [id]).unwrap();
            removed.insert(live.remove(&id).unwrap());
            checkpoint(&conn);
            let found = places(
                &conn,
                &scratch.0,
                &removed,
                &format!("seed {seed} round {round} delete {step}"),
            );
            if !found.is_empty() {
                return found;
            }
        }
    }
    Vec::new()
}

fn fts5_form(seed: u64) -> Vec<String> {
    let scratch = Scratch::new(seed, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch(
        "CREATE VIRTUAL TABLE f USING fts5(app);
         INSERT INTO f(f, rank) VALUES ('secure-delete', 1);",
    )
    .unwrap();
    let mut rng = Rng::new(seed);
    let mut live: BTreeMap<i64, String> = BTreeMap::new();
    let mut removed: BTreeSet<String> = BTreeSet::new();
    let mut next = 1i64;
    for round in 0..6 {
        conn.execute_batch("BEGIN").unwrap();
        for _ in 0..120 {
            let text = token(&mut rng);
            let app = format!("{text} app{}", rng.below(7));
            conn.execute(
                "INSERT INTO f(rowid, app) VALUES (?1, ?2)",
                params![next, app],
            )
            .unwrap();
            live.insert(next, text);
            next += 1;
        }
        conn.execute_batch("COMMIT").unwrap();
        conn.execute_batch("BEGIN").unwrap();
        for step in 0..60 {
            let at = rng.below(live.len() as u64) as usize;
            let rowid = *live.keys().nth(at).unwrap();
            if step % 3 == 0 {
                let text = token(&mut rng);
                let app = format!("{text} app{}", rng.below(7));
                conn.execute(
                    "UPDATE f SET app = ?2 WHERE rowid = ?1",
                    params![rowid, app],
                )
                .unwrap();
                removed.insert(live.insert(rowid, text).unwrap());
            } else {
                conn.execute("DELETE FROM f WHERE rowid = ?1", [rowid])
                    .unwrap();
                removed.insert(live.remove(&rowid).unwrap());
            }
        }
        conn.execute_batch("COMMIT").unwrap();
        checkpoint(&conn);
        let stored: i64 = conn
            .query_row("SELECT count(*) FROM f", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored as usize, live.len());
        let tails: BTreeSet<String> = removed.iter().map(|t| t[6..].to_owned()).collect();
        let found = places(
            &conn,
            &scratch.0,
            &tails,
            &format!("seed {seed} round {round}"),
        );
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

#[test]
fn small_rows_deleted_one_at_a_time_leave_no_text_in_a_page_edited_in_place() {
    crate::initialize().unwrap();
    let found: Vec<String> = [1, 8]
        .into_iter()
        .flat_map(small_rows_with_single_deletes)
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn a_table_of_the_fts5_form_with_its_secure_delete_leaves_no_removed_token() {
    crate::initialize().unwrap();
    let found = fts5_form(24);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn rows_that_analyze_or_optimize_read_leave_no_text_after_their_removal() {
    crate::initialize().unwrap();
    for (seed, analyze) in [(11, "ANALYZE;"), (12, "PRAGMA optimize = 0x10002;")] {
        let scratch = Scratch::new(seed, Mode::Plain);
        let conn = connect(&scratch.db(), Mode::Plain);
        conn.execute_batch(
            "CREATE TABLE t(id INTEGER PRIMARY KEY, kind INTEGER NOT NULL, body TEXT NOT NULL);
             CREATE INDEX t_kind_body ON t(kind, body);",
        )
        .unwrap();
        let mut rng = Rng::new(seed);
        let mut removed = BTreeSet::new();
        conn.execute_batch("BEGIN").unwrap();
        for id in 1..=400u64 {
            let marker = format!("h{id:06}q");
            let body = format!("{marker}{}", "v".repeat(20 + rng.below(200) as usize));
            conn.execute(
                "INSERT INTO t(id, kind, body) VALUES (?1, ?2, ?3)",
                params![id as i64, rng.below(5) as i64, body],
            )
            .unwrap();
            removed.insert(marker);
        }
        conn.execute_batch("COMMIT").unwrap();
        conn.execute_batch(analyze).unwrap();
        let samples: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'sqlite_stat4'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute_batch("DELETE FROM t;").unwrap();
        checkpoint(&conn);
        let mut found = places(&conn, &scratch.0, &removed, analyze);
        if samples != 0 {
            found.push(format!("{analyze}: the build made sqlite_stat4"));
        }
        assert!(found.is_empty(), "{}", found.join("\n"));
    }
}
