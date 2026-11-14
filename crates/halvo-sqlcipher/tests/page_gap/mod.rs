use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use rusqlite::{params, Connection};

mod fts5;
mod layout;
mod malformed;
mod pins;
mod rewrites;
mod rules;
mod second_place;
mod table;

const SEEDS: [u64; 4] = [1, 2, 3, 4];
const CYCLES: u32 = 2;
const ROUND_CAP: u32 = 120;
const INSERTS: u32 = 40;
const PAGE: usize = 4096;
const MARKER_HEAD: &[u8] = b"h";
const MARKER_LEN: usize = 8;
const FILL: &[u8] = b"bcdefjkmnrstvwxyz";
const MARK: &[u8] = b"int halvo_sqlcipher_page_gap_mend(void){ return 2; }";
const STOPS: [&str; 24] = [
    "#if defined(SQLITE_ENABLE_STAT4)",
    "#error \"halvo-sqlcipher: the build stops: SQLITE_ENABLE_STAT4 is defined\"",
    "#endif",
    "#if !defined(SQLITE_TEMP_STORE) || SQLITE_TEMP_STORE!=2",
    "#error \"halvo-sqlcipher: the build stops: SQLITE_TEMP_STORE is not 2\"",
    "#endif",
    "#if !defined(SQLITE_DEFAULT_AUTOVACUUM) || SQLITE_DEFAULT_AUTOVACUUM!=0",
    "#error \"halvo-sqlcipher: the build stops: SQLITE_DEFAULT_AUTOVACUUM is not 0\"",
    "#endif",
    "#if !defined(BTS_FAST_SECURE)",
    "#error \"halvo-sqlcipher: the build stops: BTS_FAST_SECURE is not defined, so the database's code is not compiled in this file\"",
    "#endif",
    "#if !defined(SQLITE_HAS_CODEC)",
    "#error \"halvo-sqlcipher: the build stops: SQLITE_HAS_CODEC is not defined\"",
    "#endif",
    "#if !defined(SQLITE_DEFAULT_SYNCHRONOUS) || SQLITE_DEFAULT_SYNCHRONOUS!=2 || !defined(SQLITE_DEFAULT_WAL_SYNCHRONOUS) || SQLITE_DEFAULT_WAL_SYNCHRONOUS!=2",
    "#error \"halvo-sqlcipher: the build stops: SQLITE_DEFAULT_SYNCHRONOUS or SQLITE_DEFAULT_WAL_SYNCHRONOUS is not 2\"",
    "#endif",
    "#if defined(SQLITE_DEBUG)",
    "#error \"halvo-sqlcipher: the build stops: SQLITE_DEBUG is defined\"",
    "#endif",
    "#if !defined(SQLITE_DEFAULT_PAGE_SIZE) || SQLITE_DEFAULT_PAGE_SIZE!=4096",
    "#error \"halvo-sqlcipher: the build stops: SQLITE_DEFAULT_PAGE_SIZE is not 4096\"",
    "#endif",
];
const MENDS: [(usize, &str); 2] = [
    (81262, " if( pPg->pBt->btsFlags & BTS_FAST_SECURE ){ memset(pCellptr, 0, (size_t)(pData - pCellptr)); }"),
    (81517, " if( (pPg->pBt->btsFlags & BTS_FAST_SECURE) && pData>pBegin ){ memset(pBegin, 0, (size_t)(pData - pBegin)); }"),
];
const KEY: &str =
    "PRAGMA key = \"x'000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'\";";
const SCHEMA: &str =
    "CREATE TABLE t(id INTEGER PRIMARY KEY, kind INTEGER NOT NULL, body TEXT NOT NULL);
     CREATE INDEX t_kind_body ON t(kind, body);
     CREATE TABLE w(k TEXT PRIMARY KEY, kind INTEGER NOT NULL, body TEXT NOT NULL) WITHOUT ROWID;
     CREATE INDEX w_kind_body ON w(kind, body);";

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(0x9e37_79b9_7f4a_7c15 ^ seed.wrapping_mul(0x1_0000_0001))
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Plain,
    Unsecured,
    Keyed,
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(seed: u64, mode: Mode) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let target = std::env::current_exe()
            .unwrap()
            .ancestors()
            .nth(3)
            .unwrap()
            .to_path_buf();
        let dir = target.join("tmp").join(format!(
            "halvo-sqlcipher-page-gap-{}-{}-{seed}-{mode:?}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn db(&self) -> PathBuf {
        self.0.join("page-gap.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn connect(path: &Path, mode: Mode) -> Connection {
    let conn = Connection::open(path).unwrap();
    if mode == Mode::Keyed {
        conn.execute_batch(KEY).unwrap();
    }
    conn.execute_batch("PRAGMA locking_mode = EXCLUSIVE; PRAGMA temp_store = MEMORY;")
        .unwrap();
    let secure = i64::from(mode != Mode::Unsecured);
    conn.execute_batch(&format!("PRAGMA secure_delete = {secure};"))
        .unwrap();
    let read: i64 = conn
        .query_row("PRAGMA secure_delete", [], |r| r.get(0))
        .unwrap();
    assert_eq!(read, secure, "secure_delete reads {read}");
    let journal: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
    conn
}

struct Run {
    seed: u64,
    mode: Mode,
    rng: Rng,
    conn: Option<Connection>,
    scratch: Scratch,
    t: BTreeMap<i64, (i64, String)>,
    w: BTreeMap<String, (i64, String)>,
    removed: BTreeSet<String>,
    serial: u64,
    round: u32,
    deepest: i64,
    shallow_after_deep: bool,
    cycles: u32,
    first_copy: Option<String>,
}

struct Found {
    copies: Vec<String>,
    left: Vec<String>,
}

impl Run {
    fn new(seed: u64, mode: Mode) -> Self {
        let scratch = Scratch::new(seed, mode);
        let conn = connect(&scratch.db(), mode);
        conn.execute_batch(SCHEMA).unwrap();
        Run {
            seed,
            mode,
            rng: Rng::new(seed),
            scratch,
            conn: Some(conn),
            t: BTreeMap::new(),
            w: BTreeMap::new(),
            removed: BTreeSet::new(),
            serial: 0,
            round: 0,
            deepest: 0,
            shallow_after_deep: false,
            cycles: 0,
            first_copy: None,
        }
    }

    fn conn(&self) -> &Connection {
        self.conn.as_ref().unwrap()
    }

    fn body(&mut self) -> (i64, String) {
        self.serial += 1;
        let len = match self.rng.below(24) {
            0..=5 => 8 + self.rng.below(33),
            6..=9 => 20 + self.rng.below(60),
            10..=13 => 700 + self.rng.below(251),
            14 => 4200 + self.rng.below(1800),
            _ => 80 + self.rng.below(500),
        } as usize;
        let pattern: Vec<u8> = (0..8)
            .map(|_| FILL[self.rng.below(FILL.len() as u64) as usize])
            .collect();
        let mut body = format!("h{:06}q", self.serial).into_bytes();
        while body.len() < len {
            body.push(pattern[body.len() % pattern.len()]);
        }
        (self.rng.below(5) as i64, String::from_utf8(body).unwrap())
    }

    fn depth(&self) -> i64 {
        self.conn()
            .query_row(
                "SELECT max(length(path) - length(replace(path, '/', ''))) FROM dbstat \
                 WHERE name = 't_kind_body' AND pagetype <> 'overflow'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn note_depth(&mut self) {
        let depth = self.depth();
        if self.deepest >= 3 && depth == 1 {
            self.shallow_after_deep = true;
        }
        self.deepest = self.deepest.max(depth);
    }

    fn drawn_t(&mut self) -> i64 {
        let at = self.rng.below(self.t.len() as u64) as usize;
        *self.t.keys().nth(at).unwrap()
    }

    fn drawn_w(&mut self) -> String {
        let at = self.rng.below(self.w.len() as u64) as usize;
        self.w.keys().nth(at).unwrap().clone()
    }

    fn remove_t(&mut self, id: i64) {
        let (_, body) = self.t.remove(&id).unwrap();
        self.removed.insert(body[..MARKER_LEN].to_owned());
        self.conn()
            .execute("DELETE FROM t WHERE id = ?1", [id])
            .unwrap();
    }

    fn remove_w(&mut self, k: String) {
        let (_, body) = self.w.remove(&k).unwrap();
        self.removed.insert(body[..MARKER_LEN].to_owned());
        self.conn()
            .execute("DELETE FROM w WHERE k = ?1", [&k])
            .unwrap();
    }

    fn replace_t(&mut self, id: i64) {
        let (kind, body) = self.body();
        let (_, old) = self.t.insert(id, (kind, body.clone())).unwrap();
        self.removed.insert(old[..MARKER_LEN].to_owned());
        self.conn()
            .execute(
                "UPDATE t SET kind = ?2, body = ?3 WHERE id = ?1",
                params![id, kind, body],
            )
            .unwrap();
    }

    fn replace_w(&mut self, k: String) {
        let (kind, body) = self.body();
        let (_, old) = self.w.insert(k.clone(), (kind, body.clone())).unwrap();
        self.removed.insert(old[..MARKER_LEN].to_owned());
        self.conn()
            .execute(
                "UPDATE w SET kind = ?2, body = ?3 WHERE k = ?1",
                params![k, kind, body],
            )
            .unwrap();
    }

    fn insert(&mut self) {
        let (kind, body) = self.body();
        if self.rng.below(2) == 0 {
            let id = loop {
                let id = self.rng.below(1 << 40) as i64;
                if !self.t.contains_key(&id) {
                    break id;
                }
            };
            self.t.insert(id, (kind, body.clone()));
            self.conn()
                .execute(
                    "INSERT INTO t(id, kind, body) VALUES (?1, ?2, ?3)",
                    params![id, kind, body],
                )
                .unwrap();
        } else {
            let k = loop {
                let k = format!("k{:012}", self.rng.below(1_000_000_000_000));
                if !self.w.contains_key(&k) {
                    break k;
                }
            };
            self.w.insert(k.clone(), (kind, body.clone()));
            self.conn()
                .execute(
                    "INSERT INTO w(k, kind, body) VALUES (?1, ?2, ?3)",
                    params![k, kind, body],
                )
                .unwrap();
        }
    }

    fn removal(&mut self, shrink: bool) -> Option<String> {
        let replace = !shrink && self.rng.below(3) == 0;
        if !shrink && !replace {
            for _ in 0..1 + self.rng.below(4) {
                if self.w.is_empty() || (!self.t.is_empty() && self.rng.below(2) == 0) {
                    let id = self.drawn_t();
                    self.remove_t(id);
                } else {
                    let k = self.drawn_w();
                    self.remove_w(k);
                }
                self.checkpoint();
                if let Some(problem) = self.look("after a single delete") {
                    return Some(problem);
                }
            }
            return None;
        }
        self.conn().execute_batch("BEGIN").unwrap();
        if shrink {
            let keep_t = if self.t.len() > 60 {
                self.t.len() / 3
            } else {
                2
            };
            while self.t.len() > keep_t {
                let id = self.drawn_t();
                self.remove_t(id);
            }
            let keep_w = if self.w.len() > 60 {
                self.w.len() / 3
            } else {
                2
            };
            while self.w.len() > keep_w {
                let k = self.drawn_w();
                self.remove_w(k);
            }
        } else {
            for _ in 0..1 + self.rng.below(4) {
                if self.w.is_empty() || (!self.t.is_empty() && self.rng.below(2) == 0) {
                    let id = self.drawn_t();
                    self.replace_t(id);
                } else {
                    let k = self.drawn_w();
                    self.replace_w(k);
                }
            }
        }
        self.conn().execute_batch("COMMIT").unwrap();
        None
    }

    fn checkpoint(&self) {
        let busy: i64 = self
            .conn()
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, 0, "seed {}: the checkpoint was blocked", self.seed);
    }

    fn round(&mut self, shrink: bool) -> Option<String> {
        self.round += 1;
        assert!(
            self.round <= ROUND_CAP,
            "seed {} {:?}: {ROUND_CAP} rounds did not take the index to depth 3 and back to 1",
            self.seed,
            self.mode
        );
        if !self.t.is_empty() || !self.w.is_empty() {
            if let Some(problem) = self.removal(shrink) {
                return Some(problem);
            }
        }
        self.note_depth();
        self.conn().execute_batch("BEGIN").unwrap();
        for _ in 0..INSERTS {
            self.insert();
        }
        self.conn().execute_batch("COMMIT").unwrap();
        self.checkpoint();
        self.note_depth();
        None
    }

    fn search(&self, when: &str) -> Found {
        let mut copies = Vec::new();
        if self.mode == Mode::Keyed {
            return Found {
                copies,
                left: Vec::new(),
            };
        }
        let mut places: BTreeMap<Vec<u8>, Vec<(String, usize)>> = BTreeMap::new();
        let mut names: Vec<_> = std::fs::read_dir(&self.scratch.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let mut problems = Vec::new();
        for name in names {
            let bytes = std::fs::read(self.scratch.0.join(&name)).unwrap();
            if name.ends_with("-wal") && !bytes.is_empty() {
                problems.push(format!("the log {name} holds {} bytes", bytes.len()));
            }
            for at in memchr::memmem::find_iter(&bytes, MARKER_HEAD) {
                let Some(marker) = bytes.get(at..at + MARKER_LEN) else {
                    continue;
                };
                if marker[1..7].iter().all(u8::is_ascii_digit) && marker[7] == b'q' {
                    places
                        .entry(marker.to_vec())
                        .or_default()
                        .push((name.clone(), at));
                }
            }
        }
        if let (Mode::Plain, Some(conn)) = (self.mode, self.conn.as_ref()) {
            let db = std::fs::read(self.scratch.db()).unwrap();
            for line in rules::check(conn, &db).problems {
                problems.push(format!("structural check: {line}"));
            }
        }
        let stored: BTreeSet<&[u8]> = self
            .t
            .values()
            .chain(self.w.values())
            .map(|(_, body)| &body.as_bytes()[..MARKER_LEN])
            .collect();
        for (marker, at) in &places {
            let text = String::from_utf8_lossy(marker);
            let where_ = self.places(at);
            if stored.contains(marker.as_slice()) {
                if at.len() > 2 && self.mode == Mode::Plain {
                    copies.push(format!(
                        "the stored marker {text} stands {} times: {where_}",
                        at.len()
                    ));
                }
            } else if self.removed.contains(text.as_ref()) {
                problems.push(format!("the removed marker {text} stands at {where_}"));
            } else {
                problems.push(format!(
                    "the marker {text}, which the test never wrote, stands at {where_}"
                ));
            }
        }
        if self.mode == Mode::Plain {
            for marker in &stored {
                if !places.contains_key(*marker) {
                    problems.push(format!(
                        "the search does not see the stored marker {}",
                        String::from_utf8_lossy(marker)
                    ));
                }
            }
        }
        let context = format!("seed {} round {} {when}", self.seed, self.round);
        let with_context = |all: Vec<String>| {
            all.into_iter()
                .map(|problem| format!("{context}: {problem}"))
                .collect()
        };
        Found {
            copies: with_context(copies),
            left: with_context(problems),
        }
    }

    fn look(&mut self, when: &str) -> Option<String> {
        let found = self.search(when);
        if self.first_copy.is_none() {
            self.first_copy = found.copies.into_iter().next();
        }
        if found.left.is_empty() {
            None
        } else {
            Some(found.left.join("; "))
        }
    }

    fn places(&self, at: &[(String, usize)]) -> String {
        let db = std::fs::read(self.scratch.db()).unwrap_or_default();
        at.iter()
            .map(|(name, offset)| {
                let page = offset / PAGE;
                let gap = if name.ends_with(".db") {
                    page_gap(&db, page)
                        .map(|(from, to)| {
                            let inside = (from..to).contains(&(offset % PAGE));
                            format!(", in the page's gap {from}..{to}: {inside}")
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                format!("{name} page {} offset {}{gap}", page + 1, offset % PAGE)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn model_and_integrity(&self) {
        let conn = self.conn();
        let context = format!("seed {} {:?} round {}", self.seed, self.mode, self.round);
        let t: Vec<(i64, i64, String)> = rows(conn, "SELECT id, kind, body FROM t ORDER BY id");
        let want_t: Vec<(i64, i64, String)> = self
            .t
            .iter()
            .map(|(id, (kind, body))| (*id, *kind, body.clone()))
            .collect();
        assert!(
            t == want_t,
            "{context}: the table t reads otherwise than the model"
        );
        let mut want_t_index: Vec<(i64, String, i64)> = want_t
            .into_iter()
            .map(|(id, kind, body)| (kind, body, id))
            .collect();
        want_t_index.sort();
        let t_index: Vec<(i64, String, i64)> = rows(
            conn,
            "SELECT kind, body, id FROM t INDEXED BY t_kind_body ORDER BY kind, body, id",
        );
        assert!(
            t_index == want_t_index,
            "{context}: the index t_kind_body reads otherwise than the model"
        );
        let w: Vec<(String, i64, String)> = rows(conn, "SELECT k, kind, body FROM w ORDER BY k");
        let want_w: Vec<(String, i64, String)> = self
            .w
            .iter()
            .map(|(k, (kind, body))| (k.clone(), *kind, body.clone()))
            .collect();
        assert!(
            w == want_w,
            "{context}: the table w reads otherwise than the model"
        );
        let mut want_w_index: Vec<(i64, String, String)> = want_w
            .into_iter()
            .map(|(k, kind, body)| (kind, body, k))
            .collect();
        want_w_index.sort();
        let w_index: Vec<(i64, String, String)> = rows(
            conn,
            "SELECT kind, body, k FROM w INDEXED BY w_kind_body ORDER BY kind, body, k",
        );
        assert!(
            w_index == want_w_index,
            "{context}: the index w_kind_body reads otherwise than the model"
        );
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(integrity, "ok", "{context}: the integrity check");
    }

    fn checked_rounds(&mut self) -> Option<String> {
        for _ in 0..CYCLES {
            let mut deep_rounds = 0;
            while deep_rounds < 3 {
                if let Some(problem) = self.round(false) {
                    return Some(self.with_first_copy(problem));
                }
                if self.depth() >= 3 {
                    deep_rounds += 1;
                }
                if let Some(problem) = self.checks() {
                    return Some(self.with_first_copy(problem));
                }
            }
            self.shallow_after_deep = false;
            while !self.shallow_after_deep {
                if let Some(problem) = self.round(true) {
                    return Some(self.with_first_copy(problem));
                }
                if let Some(problem) = self.checks() {
                    return Some(self.with_first_copy(problem));
                }
            }
            self.cycles += 1;
        }
        self.first_copy.clone()
    }

    fn with_first_copy(&self, problem: String) -> String {
        match &self.first_copy {
            Some(copy) => format!("{problem}\n{copy}"),
            None => problem,
        }
    }

    fn checks(&mut self) -> Option<String> {
        if let Some(problem) = self.look("with the store open") {
            return Some(problem);
        }
        if self.mode != Mode::Unsecured {
            self.model_and_integrity();
        }
        drop(self.conn.take());
        let closed = self.look("after a close");
        self.conn = Some(connect(&self.scratch.db(), self.mode));
        if closed.is_some() {
            return closed;
        }
        if self.mode != Mode::Unsecured {
            self.model_and_integrity();
        }
        None
    }
}

fn rows<T: rusqlite::types::FromSql, U: rusqlite::types::FromSql, V: rusqlite::types::FromSql>(
    conn: &Connection,
    sql: &str,
) -> Vec<(T, U, V)> {
    let mut statement = conn.prepare(sql).unwrap();
    let read = statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    read
}

fn page_gap(db: &[u8], page: usize) -> Option<(usize, usize)> {
    let data = db.get(page * PAGE..(page + 1) * PAGE)?;
    let header = if page == 0 { 100 } else { 0 };
    let kind = data[header];
    let size = match kind {
        2 | 5 => 12,
        10 | 13 => 8,
        _ => return None,
    };
    let cells = usize::from(u16::from_be_bytes([data[header + 3], data[header + 4]]));
    let content = usize::from(u16::from_be_bytes([data[header + 5], data[header + 6]]));
    Some((header + size + 2 * cells, content))
}

#[test]
fn a_removed_or_replaced_row_leaves_no_text_in_a_page_built_again_or_edited_in_place() {
    crate::initialize().unwrap();
    let mut failed = Vec::new();
    for seed in SEEDS {
        let started = Instant::now();
        let mut plain = Run::new(seed, Mode::Plain);
        match plain.checked_rounds() {
            Some(problem) => failed.push(problem),
            None => assert!(
                plain.cycles == CYCLES && plain.deepest >= 3,
                "seed {seed}: the index did not go to depth 3 and back to 1 {CYCLES} times"
            ),
        }
        println!(
            "page gap seed {seed}: plain rounds {}, rows written {}, seconds {:.2}",
            plain.round,
            plain.serial,
            started.elapsed().as_secs_f64()
        );
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn with_secure_delete_off_the_search_finds_a_removed_row_of_each_seed() {
    crate::initialize().unwrap();
    for seed in SEEDS {
        let mut control = Run::new(seed, Mode::Unsecured);
        let seen = control.checked_rounds();
        assert!(
            seen.as_deref()
                .is_some_and(|p| p.contains("the removed marker")),
            "seed {seed}: with secure delete off the search found no removed marker: {seen:?}"
        );
    }
}

#[test]
fn under_a_key_every_row_of_each_seed_reads_back_and_the_integrity_check_holds() {
    crate::initialize().unwrap();
    for seed in SEEDS {
        let mut keyed = Run::new(seed, Mode::Keyed);
        assert_eq!(keyed.checked_rounds(), None);
    }
}

#[test]
fn the_compiled_copy_is_the_pinned_file_with_two_statements_then_eight_compile_stops_and_one_added_function(
) {
    let pinned = std::fs::read(env!("HALVO_SQLCIPHER_PINNED_SOURCE")).unwrap();
    let copy = std::fs::read(concat!(env!("OUT_DIR"), "/sqlcipher.c"))
        .expect("the build script wrote its copy of the source");
    let pinned: Vec<&[u8]> = pinned.split(|byte| *byte == b'\n').collect();
    let copy: Vec<&[u8]> = copy.split(|byte| *byte == b'\n').collect();
    assert_eq!(copy.len(), pinned.len() + STOPS.len() + 1);
    let last = pinned.len() - 1;
    assert_eq!(pinned[last], b"");
    let tail: Vec<&[u8]> = STOPS
        .iter()
        .map(|line| line.as_bytes())
        .chain([MARK, b"".as_slice()])
        .collect();
    assert_eq!(copy[last..], tail[..]);
    let differ: Vec<usize> = (0..last)
        .filter(|&at| copy[at] != pinned[at])
        .map(|at| at + 1)
        .collect();
    assert_eq!(differ, MENDS.map(|(line, _)| line));
    for (line, statement) in MENDS {
        assert_eq!(
            copy[line - 1],
            [pinned[line - 1], statement.as_bytes()].concat()
        );
    }
}
