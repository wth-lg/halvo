use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{params, Connection};

use super::{connect, log_lines, rows, rules, Mode, Rng, Scratch, SCHEMA};

const WORKLOAD_SEEDS: [u64; 3] = [211, 212, 213];
const ROUNDS: u32 = 4;
const INSERTS_T: u32 = 150;
const INSERTS_W: u32 = 50;
const SINGLE_DELETES: u32 = 40;
const REPLACES: u32 = 20;

fn word(rng: &mut Rng, len: u64) -> String {
    (0..len)
        .map(|_| char::from(b'a' + rng.below(26) as u8))
        .collect()
}

fn body(rng: &mut Rng) -> String {
    let len = 16 + rng.below(160);
    word(rng, len)
}

fn truncate(conn: &Connection) {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(busy, 0);
}

fn lines_of_the_search(conn: &Connection, dir: &Path, removed: &BTreeSet<String>) -> usize {
    let mut lines = log_lines(dir).len();
    lines += rules::check(conn, &std::fs::read(dir.join("page-gap.db")).unwrap())
        .problems
        .len();
    for entry in std::fs::read_dir(dir).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for text in removed {
            lines += memchr::memmem::find_iter(&bytes, text.as_bytes()).count();
        }
    }
    lines
}

fn digest(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn print_rows(seed: u64, conn: &Connection) {
    let t: Vec<(i64, i64, String)> = rows(conn, "SELECT id, kind, body FROM t ORDER BY id");
    let t_index: Vec<(i64, String, i64)> = rows(
        conn,
        "SELECT kind, body, id FROM t INDEXED BY t_kind_body ORDER BY kind, body, id",
    );
    let w: Vec<(String, i64, String)> = rows(conn, "SELECT k, kind, body FROM w ORDER BY k");
    let w_index: Vec<(i64, String, String)> = rows(
        conn,
        "SELECT kind, body, k FROM w INDEXED BY w_kind_body ORDER BY kind, body, k",
    );
    for (what, count, text) in [
        ("table=t", t.len(), format!("{t:?}")),
        ("index=t_kind_body", t_index.len(), format!("{t_index:?}")),
        ("table=w", w.len(), format!("{w:?}")),
        ("index=w_kind_body", w_index.len(), format!("{w_index:?}")),
    ] {
        println!(
            "UNMENDED-PROBE seed={seed} {what} rows={count} digest={:016x}",
            digest(&text)
        );
    }
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    println!("UNMENDED-PROBE seed={seed} integrity={integrity}");
}

#[test]
fn the_drawn_workloads_print_their_rows_their_integrity_and_the_lines_of_the_search() {
    crate::initialize().unwrap();
    for seed in WORKLOAD_SEEDS {
        let scratch = Scratch::new(seed, Mode::Plain);
        let conn = connect(&scratch.db(), Mode::Plain);
        conn.execute_batch(SCHEMA).unwrap();
        let mut rng = Rng::new(seed);
        let mut t: BTreeMap<i64, String> = BTreeMap::new();
        let mut w: BTreeMap<String, String> = BTreeMap::new();
        let mut removed: BTreeSet<String> = BTreeSet::new();
        let mut lines = 0usize;
        let mut next = 1i64;
        for _ in 0..ROUNDS {
            conn.execute_batch("BEGIN").unwrap();
            for _ in 0..INSERTS_T {
                let text = body(&mut rng);
                conn.execute(
                    "INSERT INTO t(id, kind, body) VALUES (?1, ?2, ?3)",
                    params![next, rng.below(5) as i64, text],
                )
                .unwrap();
                t.insert(next, text);
                next += 1;
            }
            for _ in 0..INSERTS_W {
                let (k, text) = (word(&mut rng, 24), body(&mut rng));
                conn.execute(
                    "INSERT INTO w(k, kind, body) VALUES (?1, ?2, ?3)",
                    params![k, rng.below(5) as i64, text],
                )
                .unwrap();
                w.insert(k, text);
            }
            conn.execute_batch("COMMIT").unwrap();
            truncate(&conn);
            for _ in 0..SINGLE_DELETES {
                if w.is_empty() || (!t.is_empty() && rng.below(3) != 0) {
                    let id = *t.keys().nth(rng.below(t.len() as u64) as usize).unwrap();
                    conn.execute("DELETE FROM t WHERE id = ?1", [id]).unwrap();
                    removed.insert(t.remove(&id).unwrap());
                } else {
                    let k = w
                        .keys()
                        .nth(rng.below(w.len() as u64) as usize)
                        .unwrap()
                        .clone();
                    conn.execute("DELETE FROM w WHERE k = ?1", [&k]).unwrap();
                    removed.insert(w.remove(&k).unwrap());
                }
                truncate(&conn);
                lines += lines_of_the_search(&conn, &scratch.0, &removed);
            }
            conn.execute_batch("BEGIN").unwrap();
            for _ in 0..REPLACES {
                let id = *t.keys().nth(rng.below(t.len() as u64) as usize).unwrap();
                let text = body(&mut rng);
                conn.execute("UPDATE t SET body = ?1 WHERE id = ?2", params![text, id])
                    .unwrap();
                removed.insert(t.insert(id, text).unwrap());
            }
            conn.execute_batch("COMMIT").unwrap();
            truncate(&conn);
            lines += lines_of_the_search(&conn, &scratch.0, &removed);
        }
        print_rows(seed, &conn);
        println!("UNMENDED-PROBE seed={seed} search_lines={lines}");
    }
}
