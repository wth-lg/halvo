use rusqlite::params;

use super::{connect, rules, Mode, Rng, Scratch};

fn small_store(mode: Mode) -> Scratch {
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
    let (number, at) = (2..=db.len() / size)
        .find_map(|number| {
            let page = &db[(number - 1) * size..number * size];
            let content = read2(page, 5);
            let pointers_end = 8 + 2 * read2(page, 3);
            (matches!(page[0], 10 | 13) && content >= pointers_end + 8)
                .then_some((number, content - 4))
        })
        .expect("a leaf with a gap of at least 8 bytes");
    db[(number - 1) * size + at..(number - 1) * size + at + 4].copy_from_slice(b"text");
    let found = rules::check(&conn, &db).problems;
    assert!(
        found
            .iter()
            .any(|line| line.starts_with(&format!("page {number}: ")) && line.contains("gap")),
        "the structural check does not see 4 bytes planted in the gap of page {number}: {found:?}"
    );
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
