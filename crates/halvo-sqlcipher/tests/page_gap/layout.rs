use rusqlite::params;

use super::{connect, rules, Mode, Rng, Scratch};

pub(super) fn small_store(mode: Mode) -> Scratch {
    let scratch = Scratch::new(7, mode);
    let conn = connect(&scratch.db(), mode);
    conn.execute_batch(
        "CREATE TABLE t(id INTEGER PRIMARY KEY, body TEXT NOT NULL);
         CREATE INDEX t_body ON t(body);",
    )
    .unwrap();
    let mut rng = Rng::new(7);
    for round in 0..4 {
        conn.execute_batch("BEGIN").unwrap();
        for at in 0..200 {
            let len = if at % 50 == 0 {
                8180
            } else {
                8 + rng.below(300) as usize
            };
            let body: String = (0..len)
                .map(|_| char::from(b'b' + rng.below(5) as u8))
                .collect();
            conn.execute(
                "INSERT INTO t(id, body) VALUES (?1, ?2)",
                params![round * 1000 + at, body],
            )
            .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();
        conn.execute(
            "DELETE FROM t WHERE id % 3 = ?1",
            [i64::try_from(rng.below(3)).unwrap()],
        )
        .unwrap();
        let busy: i64 = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, 0);
    }
    scratch
}

fn read2(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]))
}

#[test]
fn the_structural_check_sees_what_a_delete_leaves_with_secure_delete_off() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Unsecured);
    let conn = connect(&scratch.db(), Mode::Unsecured);
    let found = rules::check(&conn, &std::fs::read(scratch.db()).unwrap()).problems;
    assert!(
        found
            .iter()
            .any(|line| line.contains("of the freeblock at")),
        "with secure delete off the structural check sees no freeblock that holds bytes: {found:?}"
    );
}

#[test]
fn the_structural_check_sees_four_bytes_of_text_planted_in_a_gap() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let mut db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(rules::check(&conn, &db).problems, Vec::<String>::new());
    let size = read2(&db, 16);
    let (number, pointers_end, run, content) = (2..=db.len() / size)
        .find_map(|number| {
            let page = &db[(number - 1) * size..number * size];
            let content = read2(page, 5);
            let pointers_end = 8 + 2 * read2(page, 3);
            let mut run = pointers_end;
            while run + 1 < content && (page[run] != 0 || page[run + 1] != 0) {
                run += 2;
            }
            (matches!(page[0], 10 | 13) && content >= run + 8).then_some((
                number,
                pointers_end,
                run,
                content,
            ))
        })
        .expect("a leaf with 8 bytes of gap past its stale run");
    let at = content - 4;
    db[(number - 1) * size + at..(number - 1) * size + at + 4].copy_from_slice(b"text");
    let found = rules::check(&conn, &db).problems;
    let named = format!(
        "page {number}: byte {at} of the gap {pointers_end}..{content}, past its run of stale pointers that ends at {run}, is 0x74"
    );
    assert!(
        found.iter().any(|line| line == &named),
        "the structural check does not name 4 bytes planted in the gap of page {number} ({named}): {found:?}"
    );
}

#[test]
fn a_stale_offset_passes_only_between_its_slot_plus_2_and_the_usable_size_less_4() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(rules::check(&conn, &db).problems, Vec::<String>::new());
    let size = read2(&db, 16);
    let (number, slot) = (2..=db.len() / size)
        .find_map(|number| {
            let page = &db[(number - 1) * size..number * size];
            let pointers_end = 8 + 2 * read2(page, 3);
            (matches!(page[0], 10 | 13) && read2(page, 5) >= pointers_end + 8)
                .then_some((number, pointers_end))
        })
        .expect("a leaf with a gap of at least 8 bytes");
    for (value, passes) in [
        (slot + 1, false),
        (slot + 2, true),
        (size - 4, true),
        (size - 3, false),
    ] {
        let mut planted = db.clone();
        let at = (number - 1) * size + slot;
        planted[at..at + 2].copy_from_slice(&(value as u16).to_be_bytes());
        let found = rules::check(&conn, &planted).problems;
        let named = found.iter().any(|line| {
            line.starts_with(&format!("page {number}: the gap's slot at {slot} holds"))
        });
        assert_eq!(
            (found.is_empty(), named),
            (passes, !passes),
            "the stale value {value} in the slot at {slot} of page {number}: {found:?}"
        );
    }
}

#[test]
fn the_structural_check_sees_a_byte_planted_in_each_place_that_it_reads() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    let read = rules::check(&conn, &db);
    assert_eq!(read.problems, Vec::<String>::new());
    assert_eq!(
        read.btree + read.overflow + read.trunks + read.leaves,
        read.pages,
        "the check did not read every page"
    );
    assert!(read.overflow_tails > 0 && read.trunks > 0 && read.leaves > 0);
    let misses = rules::plants(&conn, &db);
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

fn checkpoint(conn: &rusqlite::Connection) {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(busy, 0);
}

#[test]
fn a_record_at_the_largest_local_payload_and_one_byte_above_passes_the_check() {
    crate::initialize().unwrap();
    let scratch = Scratch::new(17, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY, body TEXT NOT NULL);")
        .unwrap();
    for (id, length, overflow) in [(1, 4057, 0), (2, 4058, 1)] {
        conn.execute(
            "INSERT INTO t(id, body) VALUES (?1, ?2)",
            params![id, "b".repeat(length)],
        )
        .unwrap();
        checkpoint(&conn);
        let pages: i64 = conn
            .query_row(
                "SELECT count(*) FROM dbstat WHERE name = 't' AND pagetype = 'overflow'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            pages, overflow,
            "overflow pages after a body of {length} bytes"
        );
        let read = rules::check(&conn, &std::fs::read(scratch.db()).unwrap());
        assert_eq!(
            read.problems,
            Vec::<String>::new(),
            "a body of {length} bytes"
        );
    }
}

#[test]
fn cells_of_three_bytes_in_a_one_column_table_without_rowid_pass_the_check() {
    crate::initialize().unwrap();
    let scratch = Scratch::new(16, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch(
        "CREATE TABLE big(id INTEGER PRIMARY KEY, body TEXT NOT NULL);
         CREATE TABLE k(key PRIMARY KEY) WITHOUT ROWID;",
    )
    .unwrap();
    for id in 0..20 {
        conn.execute(
            "INSERT INTO big(id, body) VALUES (?1, ?2)",
            params![id, "x".repeat(3000)],
        )
        .unwrap();
    }
    conn.execute_batch("INSERT INTO k(key) VALUES (0), (1), (''), (X'');")
        .unwrap();
    checkpoint(&conn);
    let (cells, payload): (i64, i64) = conn
        .query_row(
            "SELECT sum(ncell), sum(payload) FROM dbstat WHERE name = 'k'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((cells, payload), (4, 8), "four cells of 2 payload bytes");
    let mut db = std::fs::read(scratch.db()).unwrap();
    let read = rules::check(&conn, &db);
    assert_eq!(read.problems, Vec::<String>::new());
    let page: i64 = conn
        .query_row("SELECT pageno FROM dbstat WHERE name = 'k'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let size = read2(&db, 16);
    let at = (page as usize - 1) * size;
    let cell = read2(&db, at + 8);
    db[at + cell + 3] = 1;
    let found = rules::check(&conn, &db).problems;
    assert!(
        found
            .iter()
            .any(|line| line.contains(&format!("byte {} of the content area", cell + 3))),
        "a byte planted after a cell of 3 bytes is not named: {found:?}"
    );
}
