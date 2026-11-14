//! Temporary probes of verifier 133 of unit sqlcipher-1. Not part of the unit:
//! a step of the scratch workflow copies this file into the test tree after the
//! unit's own tests have run. The log and journal cases are adversary 1's
//! (adv1_snapshot, adv1_journal), with a keyed case added.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use rusqlite::{params, Connection};

use super::KEY;

#[derive(Clone, Copy, Debug)]
struct Cfg {
    secure: &'static str,
    keyed: bool,
    exclusive: bool,
}

const BASE: Cfg = Cfg {
    secure: "ON",
    keyed: false,
    exclusive: true,
};

fn work(case: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let root = std::env::var("V133_WORK").expect("V133_WORK names the work directory");
    let dir = Path::new(&root).join(format!(
        "{case}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open(path: &Path, cfg: &Cfg) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.busy_timeout(Duration::from_millis(50)).unwrap();
    if cfg.keyed {
        conn.execute_batch(KEY).unwrap();
    }
    if cfg.exclusive {
        conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE;").unwrap();
    }
    conn.execute_batch("PRAGMA temp_store = MEMORY;").unwrap();
    conn.execute_batch(&format!("PRAGMA secure_delete = {};", cfg.secure))
        .unwrap();
    let journal: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
    conn
}

fn text(serial: u32, len: usize) -> String {
    let unit = format!("h{serial:06}q");
    unit.repeat(len / 8 + 1)[..len.max(8)].to_owned()
}

fn ckpt(conn: &Connection, mode: &str) -> String {
    match conn.query_row(&format!("PRAGMA wal_checkpoint({mode})"), [], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    }) {
        Ok((busy, log, done)) => {
            format!("wal_checkpoint({mode}) = busy {busy}, log {log}, checkpointed {done}")
        }
        Err(e) => format!("wal_checkpoint({mode}) failed: {e}"),
    }
}

fn report(tag: &str, dir: &Path, needles: &[(&str, String)]) {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    println!("V133 {tag}: files {}", names.join(" "));
    for name in names {
        let bytes = match std::fs::read(dir.join(&name)) {
            Ok(bytes) => bytes,
            Err(e) => {
                println!("V133 {tag}: file {name} not read: {e}");
                continue;
            }
        };
        let mut line = format!("V133 {tag}: file {name} {} bytes", bytes.len());
        for (label, needle) in needles {
            let at: Vec<usize> = memchr::memmem::find_iter(&bytes, needle.as_bytes()).collect();
            line.push_str(&format!("; {label} {needle} {} times", at.len()));
            if let Some(first) = at.first() {
                line.push_str(&format!(" (first at byte {first})"));
            }
        }
        if name.ends_with("-journal") && bytes.len() >= 28 {
            line.push_str(&format!(
                "; first 28 bytes all zero: {}",
                bytes[..28].iter().all(|b| *b == 0)
            ));
        }
        println!("{line}");
    }
}

fn fill(conn: &Connection, from: u32, to: u32) {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS x(id INTEGER PRIMARY KEY, body TEXT NOT NULL);")
        .unwrap();
    conn.execute_batch("BEGIN").unwrap();
    for id in from..to {
        conn.execute(
            "INSERT INTO x(id, body) VALUES (?1, ?2)",
            params![id, text(id, 24 + (id as usize % 40))],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
}

fn needles() -> [(&'static str, String); 2] {
    [("removed", text(150, 8)), ("kept", text(151, 8))]
}

#[test]
fn v133_which_library() {
    crate::initialize().unwrap();
    let c = Connection::open_in_memory().unwrap();
    let cache: i64 = c.query_row("PRAGMA cache_size", [], |r| r.get(0)).unwrap();
    let foreign: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    let secure: i64 = c.query_row("PRAGMA secure_delete", [], |r| r.get(0)).unwrap();
    let cipher: String = c
        .query_row("PRAGMA cipher_version", [], |r| r.get(0))
        .unwrap_or_else(|e| format!("no answer: {e}"));
    let mut options: Vec<String> = Vec::new();
    let mut read = c.prepare("PRAGMA compile_options").unwrap();
    let mut rows = read.query([]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        options.push(row.get(0).unwrap());
    }
    let marked: Vec<&String> = options
        .iter()
        .filter(|o| o.starts_with("DEFAULT_CACHE_SIZE") || o.starts_with("DEFAULT_FOREIGN_KEYS"))
        .collect();
    println!(
        "V133 which: page_gap_mend_linked={} cache_size={cache} foreign_keys={foreign} secure_delete={secure} cipher_version={cipher} compile_options={} of them {marked:?}",
        crate::page_gap_mend_linked(),
        options.len()
    );
}

#[test]
fn v133_log_at_the_last_close() {
    crate::initialize().unwrap();
    let shared = Cfg {
        exclusive: false,
        ..BASE
    };
    let keyed = Cfg { keyed: true, ..BASE };
    let needles = needles();

    let dir = work("snap2");
    let a = open(&dir.join("case.db"), &shared);
    fill(&a, 1, 301);
    let b = open(&dir.join("case.db"), &shared);
    b.execute_batch("BEGIN").unwrap();
    let rows: i64 = b.query_row("SELECT count(*) FROM x", [], |r| r.get(0)).unwrap();
    println!("V133 snap2 reader holds a snapshot of {rows} rows inside the log");
    a.execute("DELETE FROM x WHERE id = 150", []).unwrap();
    println!("V133 snap2 after the delete: {}", ckpt(&a, "TRUNCATE"));
    report("snap2 delete, reader open", &dir, &needles);
    drop(a);
    report("snap2 writer closed, reader open", &dir, &needles);
    b.execute_batch("COMMIT").unwrap();
    drop(b);
    report("snap2 both closed", &dir, &needles);

    for (case, cfg) in [
        ("kinds-shared", shared),
        ("kinds-exclusive", BASE),
        ("kinds-exclusive-keyed", keyed),
    ] {
        let dir = work(case);
        let a = open(&dir.join("case.db"), &cfg);
        fill(&a, 1, 301);
        report(&format!("{case} after the insert"), &dir, &needles);
        a.execute("DELETE FROM x WHERE id = 150", []).unwrap();
        report(&format!("{case} after the delete"), &dir, &needles);
        drop(a);
        report(
            &format!("{case} closed right after the delete, no checkpoint call"),
            &dir,
            &needles,
        );
        let a = open(&dir.join("case.db"), &cfg);
        for mode in ["PASSIVE", "FULL", "RESTART"] {
            a.execute("DELETE FROM x WHERE id = ?1", [200 + mode.len() as i64])
                .unwrap();
            println!("V133 {case}: {}", ckpt(&a, mode));
            report(&format!("{case} delete then {mode}"), &dir, &needles);
        }
        fill(&a, 1000, 1002);
        println!("V133 {case}: {}", ckpt(&a, "TRUNCATE"));
        report(&format!("{case} after TRUNCATE"), &dir, &needles);
        fill(&a, 2000, 2300);
        a.execute("DELETE FROM x WHERE id = 151", []).unwrap();
        report(&format!("{case} 151 removed too, before the close"), &dir, &needles);
        drop(a);
        report(
            &format!("{case} closed with no checkpoint call, 151 removed too"),
            &dir,
            &needles,
        );
    }
}

#[test]
fn v133_journal_at_the_close() {
    crate::initialize().unwrap();
    let needles = needles();
    for locking in ["NORMAL", "EXCLUSIVE"] {
        for mode in ["DELETE", "TRUNCATE", "PERSIST"] {
            let case = format!("journal-{locking}-{mode}");
            let dir = work(&case);
            let conn = Connection::open(dir.join("case.db")).unwrap();
            conn.execute_batch(&format!(
                "PRAGMA locking_mode = {locking}; PRAGMA temp_store = MEMORY; PRAGMA secure_delete = ON;"
            ))
            .unwrap();
            let read: String = conn
                .query_row(&format!("PRAGMA journal_mode = {mode}"), [], |r| r.get(0))
                .unwrap();
            fill(&conn, 1, 301);
            conn.execute("DELETE FROM x WHERE id = 150", []).unwrap();
            println!("V133 {case}: journal_mode reads {read}");
            report(&format!("{case} after the delete"), &dir, &needles);
            drop(conn);
            report(&format!("{case} closed"), &dir, &needles);
        }
    }
}
