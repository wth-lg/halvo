use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection};

use super::{connect, Mode, Rng, Scratch, PAGE};

fn checkpoint(conn: &Connection) {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(busy, 0);
}

fn fts5_table(conn: &Connection, secure: bool) {
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE f USING fts5(app);
         INSERT INTO f(f, rank) VALUES ('secure-delete', {});",
        i64::from(secure)
    ))
    .unwrap();
}

fn letters(rng: &mut Rng, count: usize) -> String {
    (0..count)
        .map(|_| char::from(b'a' + rng.below(26) as u8))
        .collect()
}

fn page_names(conn: &Connection) -> BTreeMap<usize, String> {
    let mut statement = conn.prepare("SELECT pageno, name FROM dbstat").unwrap();
    let names = statement
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)? as usize, r.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    names
}

fn standing(scratch: &Scratch, needle: &[u8]) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&scratch.0).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        for at in memchr::memmem::find_iter(&bytes, needle) {
            found.push((name.clone(), at));
        }
    }
    found
}

fn outside_idx(
    conn: &Connection,
    scratch: &Scratch,
    removed: &BTreeSet<String>,
    when: &str,
) -> Vec<String> {
    let names = page_names(conn);
    let mut found = Vec::new();
    for token in removed {
        let bytes = token.as_bytes();
        for (needle, how) in [
            (bytes, "whole"),
            (&bytes[6..], "by its tail from byte 6"),
            (&bytes[bytes.len() - 6..], "by its last 6 bytes"),
        ] {
            for (file, at) in standing(scratch, needle) {
                let page = at / PAGE + 1;
                let owner = names.get(&page).map(String::as_str).unwrap_or("no tree");
                if !(file.ends_with(".db") && owner == "f_idx") {
                    found.push(format!(
                        "{when}: the removed token {token}, searched {how}, stands in {file} page {page} offset {} of {owner}",
                        at % PAGE
                    ));
                }
            }
        }
    }
    found
}

fn shared_prefixes(seed: u64, secure: bool) -> (Vec<String>, BTreeSet<String>) {
    let scratch = Scratch::new(seed, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    fts5_table(&conn, secure);
    let mut rng = Rng::new(seed);
    let mut live: BTreeMap<i64, String> = BTreeMap::new();
    let mut removed: BTreeSet<String> = BTreeSet::new();
    let mut next = 1i64;
    let token = |rng: &mut Rng| format!("appname{:02}{}", rng.below(4), letters(rng, 7));
    for round in 0..6 {
        let mut this_round: BTreeSet<String> = BTreeSet::new();
        conn.execute_batch("BEGIN").unwrap();
        for _ in 0..120 {
            let text = token(&mut rng);
            conn.execute(
                "INSERT INTO f(rowid, app) VALUES (?1, ?2)",
                params![next, format!("{text} app{}", rng.below(7))],
            )
            .unwrap();
            live.insert(next, text);
            next += 1;
        }
        conn.execute_batch("COMMIT").unwrap();
        for step in 0..60 {
            let at = rng.below(live.len() as u64) as usize;
            let rowid = *live.keys().nth(at).unwrap();
            if step % 3 == 0 {
                let text = token(&mut rng);
                conn.execute(
                    "UPDATE f SET app = ?2 WHERE rowid = ?1",
                    params![rowid, format!("{text} app{}", rng.below(7))],
                )
                .unwrap();
                let old = live.insert(rowid, text).unwrap();
                this_round.insert(old.clone());
                removed.insert(old);
            } else {
                conn.execute("DELETE FROM f WHERE rowid = ?1", [rowid])
                    .unwrap();
                let old = live.remove(&rowid).unwrap();
                this_round.insert(old.clone());
                removed.insert(old);
            }
        }
        checkpoint(&conn);
        let found = outside_idx(
            &conn,
            &scratch,
            &removed,
            &format!("seed {seed} round {round}"),
        );
        if !found.is_empty() {
            return (found, this_round);
        }
    }
    (Vec::new(), BTreeSet::new())
}

#[test]
fn removed_fts5_tokens_with_a_shared_prefix_stand_in_no_page_but_the_idx_tree() {
    crate::initialize().unwrap();
    let found: Vec<String> = (24..=27)
        .flat_map(|seed| shared_prefixes(seed, true).0)
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn with_fts5_secure_delete_off_every_removed_token_that_a_needle_finds_is_found_by_its_last_6_bytes(
) {
    crate::initialize().unwrap();
    let mut misses = Vec::new();
    for seed in 24..=27 {
        let (found, removed) = shared_prefixes(seed, false);
        let by = |how: &str| -> BTreeSet<&String> {
            removed
                .iter()
                .filter(|token| {
                    let named = format!("the removed token {token}, searched {how}, stands in ");
                    found.iter().any(|line| line.contains(&named))
                })
                .collect()
        };
        let (whole, tail, last) = (
            by("whole"),
            by("by its tail from byte 6"),
            by("by its last 6 bytes"),
        );
        println!(
            "FTS5 seed {seed}: {} tokens removed in the round of the first find; found whole {}, by the tail from byte 6 {}, by the last 6 bytes {}",
            removed.len(),
            whole.len(),
            tail.len(),
            last.len()
        );
        if last.is_empty() {
            misses.push(format!(
                "seed {seed}: no removed token found by its last 6 bytes"
            ));
        }
        for token in whole.union(&tail) {
            if !last.contains(token) {
                misses.push(format!(
                    "seed {seed}: {token} is found by another needle and not by its last 6 bytes"
                ));
            }
        }
    }
    assert!(
        misses.is_empty(),
        "with FTS5's secure-delete off: {}",
        misses.join("; ")
    );
}

#[test]
fn the_fts5_idx_key_of_a_removed_row_stays_until_a_rebuild() {
    crate::initialize().unwrap();
    let scratch = Scratch::new(40, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    fts5_table(&conn, true);
    let mut rng = Rng::new(40);
    let mut rows: BTreeMap<String, i64> = BTreeMap::new();
    conn.execute_batch("BEGIN").unwrap();
    for pair in 0..1500i64 {
        let head = format!("app{}", letters(&mut rng, 12));
        for (at, last) in ["a", "b"].iter().enumerate() {
            let text = format!("{head}{last}");
            let rowid = pair * 2 + at as i64 + 1;
            conn.execute(
                "INSERT INTO f(rowid, app) VALUES (?1, ?2)",
                params![rowid, text],
            )
            .unwrap();
            rows.insert(text, rowid);
        }
    }
    conn.execute_batch("COMMIT").unwrap();
    checkpoint(&conn);
    let keys: Vec<Vec<u8>> = {
        let mut statement = conn.prepare("SELECT term FROM f_idx").unwrap();
        let keys = statement
            .query_map([], |r| r.get::<_, Vec<u8>>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        keys
    };
    let Some(token) = keys.iter().find_map(|key| {
        let text = String::from_utf8(key.get(1..)?.to_vec()).ok()?;
        (key[0] == b'0' && rows.contains_key(&text)).then_some(text)
    }) else {
        panic!(
            "no key of f_idx is a whole live token of a row of its own ({} keys): draw more rows, or \
             tokens whose neighbours differ in the last byte alone",
            keys.len()
        );
    };
    conn.execute("DELETE FROM f WHERE rowid = ?1", [rows[&token]])
        .unwrap();
    checkpoint(&conn);
    let names = page_names(&conn);
    let in_idx = standing(&scratch, token.as_bytes())
        .into_iter()
        .filter(|(file, at)| {
            file.ends_with(".db")
                && names.get(&(at / PAGE + 1)).map(String::as_str) == Some("f_idx")
        })
        .count();
    assert!(
        in_idx > 0,
        "FTS5 changed: the token {token} of the removed row no longer stands as a key of f_idx; read \
         fts5DoSecureDelete of the pinned source again and take the limit of the _idx key out of ADR 0013"
    );
    conn.execute_batch("INSERT INTO f(f) VALUES ('rebuild');")
        .unwrap();
    checkpoint(&conn);
    let after = standing(&scratch, token.as_bytes());
    assert!(
        after.is_empty(),
        "after 'rebuild' the token {token} of the removed row still stands at {after:?}"
    );
}
