use std::io::{Seek, SeekFrom, Write};

use rusqlite::{params, Connection};

use super::layout::small_store;
use super::{connect, rules, Mode, Run, Scratch, MARKER_LEN, PAGE};

pub(super) fn read2(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]))
}

pub(super) fn read4(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
}

pub(super) fn checkpoint(conn: &Connection) {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(busy, 0);
}

fn drawn(seed: u64) -> Run {
    let mut run = Run::new(seed, Mode::Plain);
    for _ in 0..3 {
        assert_eq!(run.round(false), None);
    }
    assert!(
        !run.removed.is_empty(),
        "seed {seed}: three rounds removed no row"
    );
    run
}

/// A leaf of `db` with 8 bytes of gap past its run of stale slots: its
/// number and the offset, inside the page, of the first byte past the zero
/// pair that ends the run.
pub(super) fn gap_past_the_stale_run(db: &[u8]) -> (usize, usize) {
    (2..=db.len() / PAGE)
        .find_map(|number| {
            let page = &db[(number - 1) * PAGE..number * PAGE];
            let content = read2(page, 5);
            if !matches!(page[0], 10 | 13) || content > PAGE {
                return None;
            }
            let mut run = 8 + 2 * read2(page, 3);
            while run + 1 < content && (page[run] != 0 || page[run + 1] != 0) {
                run += 2;
            }
            (content >= run + 8).then_some((number, run + 2))
        })
        .expect("a leaf with 8 bytes of gap past its stale run")
}

pub(super) fn write_at(path: &std::path::Path, at: usize, bytes: &[u8]) {
    let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(at as u64)).unwrap();
    file.write_all(bytes).unwrap();
}

#[test]
fn a_checkpoint_that_does_not_truncate_leaves_a_log_that_the_search_names() {
    crate::initialize().unwrap();
    let mut run = Run::new(1, Mode::Plain);
    run.checkpoint_word = "PASSIVE";
    let seen = run.checked_rounds();
    assert!(
        seen.as_deref()
            .is_some_and(|p| p.contains("the log page-gap.db-wal holds ")),
        "with a checkpoint that does not truncate the search does not name the log: {seen:?}"
    );
}

#[test]
fn with_the_store_closed_text_planted_past_the_stale_run_of_a_gap_is_named_by_the_search() {
    crate::initialize().unwrap();
    let mut run = drawn(5);
    drop(run.conn.take());
    let db = std::fs::read(run.scratch.db()).unwrap();
    let (number, at) = gap_past_the_stale_run(&db);
    write_at(&run.scratch.db(), (number - 1) * PAGE + at, b"text");
    run.conn = Some(connect(&run.scratch.db(), Mode::Plain));
    let found = run.search("after a plant");
    let named = format!("structural check: page {number}: byte {at} of the gap");
    assert!(
        found.left.iter().any(|line| line.contains(&named)),
        "the search does not name 4 bytes of text planted at byte {at} of page {number} ({named}): {:?}",
        found.left
    );
}

#[test]
fn text_planted_in_a_gap_with_the_store_open_is_named_by_the_look_with_the_store_open() {
    crate::initialize().unwrap();
    let mut run = drawn(6);
    let db = std::fs::read(run.scratch.db()).unwrap();
    let (number, at) = gap_past_the_stale_run(&db);
    write_at(&run.scratch.db(), (number - 1) * PAGE + at, b"text");
    let seen = run.checks();
    let named =
        format!("with the store open: structural check: page {number}: byte {at} of the gap");
    assert!(
        seen.as_deref().is_some_and(|p| p.contains(&named)),
        "the checks do not name text planted with the store open ({named}): {seen:?}"
    );
}

#[test]
fn a_removed_marker_written_after_the_close_is_named_by_the_look_after_a_close() {
    crate::initialize().unwrap();
    let mut run = drawn(8);
    let marker = run.removed.iter().next().unwrap().clone();
    run.after_close = Some(|run: &Run| {
        let marker = run.removed.iter().next().unwrap();
        std::fs::write(run.scratch.0.join("written-after-the-close"), marker).unwrap();
    });
    let seen = run.checks();
    let named = format!(
        "after a close: the removed marker {marker} stands at written-after-the-close page 1 offset 0"
    );
    assert!(
        seen.as_deref().is_some_and(|p| p.contains(&named)),
        "the look after a close does not name a removed marker written then ({named}): {seen:?}"
    );
}

#[test]
fn a_stored_marker_missing_from_the_files_is_named_by_the_search() {
    crate::initialize().unwrap();
    let mut run = drawn(9);
    run.t
        .insert(1 << 41, (0, format!("h999999q{}", "b".repeat(32))));
    let found = run.search("after a model entry with no row");
    assert!(
        found
            .left
            .iter()
            .any(|line| line.ends_with("the search does not see the stored marker h999999q")),
        "the search does not name a stored marker that no file holds: {:?}",
        found.left
    );
}

#[test]
fn a_marker_that_the_test_never_wrote_is_named_by_the_search() {
    crate::initialize().unwrap();
    let run = drawn(10);
    run.conn()
        .execute(
            "INSERT INTO t(id, kind, body) VALUES (?1, 0, ?2)",
            params![1i64 << 41, format!("h888888q{}", "b".repeat(32))],
        )
        .unwrap();
    run.checkpoint();
    let found = run.search("after a row of no model");
    assert!(
        found
            .left
            .iter()
            .any(|line| line.contains("the marker h888888q, which the test never wrote, stands at")),
        "the search does not name a marker that the test never wrote: {:?}",
        found.left
    );
}

#[test]
fn a_removed_marker_in_a_file_of_another_name_is_named_by_the_search() {
    crate::initialize().unwrap();
    let run = drawn(11);
    let marker = run.removed.iter().next().unwrap().clone();
    assert_eq!(marker.len(), MARKER_LEN);
    std::fs::write(
        run.scratch.0.join("another-name.bin"),
        format!("xx{marker}"),
    )
    .unwrap();
    let found = run.search("after a file of another name");
    let named = format!("the removed marker {marker} stands at another-name.bin page 1 offset 2");
    assert!(
        found.left.iter().any(|line| line.contains(&named)),
        "the search does not name a removed marker in a file of another name ({named}): {:?}",
        found.left
    );
}

pub(super) fn plant(
    conn: &Connection,
    db: &[u8],
    at: usize,
    bytes: &[u8],
    named: &str,
) -> Option<String> {
    let mut copy = db.to_vec();
    copy[at..at + bytes.len()].copy_from_slice(bytes);
    let found = rules::check(conn, &copy).problems;
    (!found.iter().any(|line| line.contains(named))).then(|| {
        format!("{bytes:?} planted at byte {at} of the file: no line names {named:?}: {found:?}")
    })
}

#[test]
fn the_structural_check_sees_a_byte_planted_at_each_end_of_the_places_that_it_reads() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(rules::check(&conn, &db).problems, Vec::<String>::new());
    let page = |number: usize| (number - 1) * PAGE;
    let mut misses = Vec::new();
    let trunk = read4(&db, 32);
    let leaves = read4(&db, page(trunk) + 4);
    assert!(
        trunk != 0 && leaves > 0 && 12 + 4 * leaves <= PAGE,
        "the image has no freelist trunk with a leaf and 4 bytes past its count"
    );
    let first = 8 + 4 * leaves;
    misses.extend(plant(
        &conn,
        &db,
        page(trunk) + first,
        &[0xff; 4],
        &format!("page {trunk}: the freelist trunk past its count holds at {first} "),
    ));
    let mut statement = conn
        .prepare("SELECT pageno, payload FROM dbstat WHERE pagetype = 'overflow'")
        .unwrap();
    let overflow: Vec<(usize, usize)> = statement
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)? as usize, r.get::<_, i64>(1)? as usize))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let (tail, used) = *overflow
        .iter()
        .find(|(_, used)| *used + 8 <= PAGE)
        .expect("an overflow tail with 4 bytes past its payload");
    misses.extend(plant(
        &conn,
        &db,
        page(tail) + 4 + used,
        &[0xff; 4],
        &format!(
            "page {tail}: the tail of an overflow page holds at {} ",
            4 + used
        ),
    ));
    let leaf = read4(&db, page(trunk) + 8);
    misses.extend(plant(
        &conn,
        &db,
        page(leaf),
        &[1],
        &format!("page {leaf}: byte 0 of a freelist leaf"),
    ));
    misses.extend(plant(&conn, &db, 55, &[1], "page 1: header byte 55 "));
    let (number, free, size) = (2..=db.len() / PAGE)
        .find_map(|number| {
            let free = read2(&db, page(number) + 1);
            (matches!(db[page(number)], 2 | 5 | 10 | 13) && free != 0)
                .then(|| (number, free, read2(&db, page(number) + free + 2)))
                .filter(|(_, _, size)| *size > 4)
        })
        .expect("a freeblock longer than its header");
    misses.extend(plant(
        &conn,
        &db,
        page(number) + free + size - 1,
        &[1],
        &format!(
            "page {number}: byte {} of the freeblock at {free} (size {size})",
            free + size - 1
        ),
    ));
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

/// One row of a blob of `length` bytes in a table with an index on the blob:
/// what dbstat reads for the index's leaf (its local payload and the largest
/// payload of a cell) and the structural check's lines.
fn one_index_record(seed: u64, length: usize) -> ((i64, i64), Vec<String>) {
    let scratch = Scratch::new(seed, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch("CREATE TABLE x(id INTEGER PRIMARY KEY, b BLOB); CREATE INDEX x_b ON x(b);")
        .unwrap();
    let blob: Vec<u8> = (0..length).map(|at| b'b' + (at % 7) as u8).collect();
    conn.execute("INSERT INTO x(id, b) VALUES (1, ?1)", [blob])
        .unwrap();
    checkpoint(&conn);
    let read: (i64, i64) = conn
        .query_row(
            "SELECT payload, mx_payload FROM dbstat WHERE name = 'x_b' AND pagetype = 'leaf'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let db = std::fs::read(scratch.db()).unwrap();
    (read, rules::check(&conn, &db).problems)
}

#[test]
fn an_index_record_whose_local_part_is_the_largest_passes_the_check() {
    crate::initialize().unwrap();
    let max = (PAGE - 12) * 64 / 255 - 23;
    let (read, problems) = one_index_record(43, 5090);
    assert_eq!(
        read,
        (max as i64, 5094),
        "dbstat does not read a local part of {max} bytes of a record of 5,094: {read:?}"
    );
    assert_eq!(problems, Vec::<String>::new());
}

#[test]
fn an_index_record_one_byte_above_the_largest_local_payload_passes_the_check() {
    crate::initialize().unwrap();
    let (max, min) = ((PAGE - 12) * 64 / 255 - 23, (PAGE - 12) * 32 / 255 - 23);
    let (read, problems) = one_index_record(44, 999);
    assert_eq!(
        read,
        (min as i64, max as i64 + 1),
        "dbstat does not read a record of {} bytes that spills with a local part of {min}: {read:?}",
        max + 1
    );
    assert_eq!(problems, Vec::<String>::new());
}
