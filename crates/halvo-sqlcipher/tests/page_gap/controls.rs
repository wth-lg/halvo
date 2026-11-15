//! The controls of S-87 G1 that need this crate's own images: each plant is
//! made so that only its rule names it, and each test requires that rule's own
//! line, with the page number where two rules print the same words.

use std::collections::BTreeMap;

use rusqlite::{params, Connection};

use super::holes::{checkpoint, plant, read2, read4, write_at};
use super::layout::small_store;
use super::malformed::varint;
use super::{connect, rules, Mode, Scratch, PAGE};

fn page(number: usize) -> usize {
    (number - 1) * PAGE
}

/// A b-tree page after page 1: its kind, the end of its pointer array, the
/// end of its run of stale slots and its content start.
struct Tree {
    number: usize,
    kind: u8,
    pointers_end: usize,
    run: usize,
    content: usize,
}

fn tree(db: &[u8], number: usize) -> Option<Tree> {
    let at = page(number);
    let kind = db[at];
    let header = match kind {
        2 | 5 => 12,
        10 | 13 => 8,
        _ => return None,
    };
    let content = read2(db, at + 5);
    let pointers_end = header + 2 * read2(db, at + 3);
    let mut run = pointers_end;
    while run + 1 < content && (db[at + run] != 0 || db[at + run + 1] != 0) {
        run += 2;
    }
    Some(Tree {
        number,
        kind,
        pointers_end,
        run,
        content,
    })
}

/// The first page of `kind` with at least `room` bytes of gap past its stale
/// run.
fn tree_of_kind(db: &[u8], kind: u8, room: usize) -> Tree {
    (2..=db.len() / PAGE)
        .filter_map(|number| tree(db, number))
        .find(|tree| tree.kind == kind && tree.content >= tree.run + room)
        .unwrap_or_else(|| {
            panic!(
                "the image has no page of kind {kind} with {room} bytes of gap past its stale run"
            )
        })
}

fn lines_of(conn: &Connection, db: &[u8]) -> Vec<String> {
    rules::check(conn, db).problems
}

/// The four rules of btree_page on one page of `kind`, each by a plant of
/// `value` (the slot rule by a value past the usable size), each line required
/// by its own text.
fn four_rules(conn: &Connection, db: &[u8], kind: u8, value: u8) -> Vec<String> {
    let t = tree_of_kind(db, kind, 8);
    let (n, base, c) = (t.number, page(t.number), t.content);
    let mut misses = Vec::new();
    for at in [c - 4, c - 1] {
        misses.extend(plant(
            conn,
            db,
            base + at,
            &[value],
            &format!(
                "page {n}: byte {at} of the gap {}..{c}, past its run of stale pointers that ends at {}, is {value:#04x}",
                t.pointers_end, t.run
            ),
        ));
    }
    let mut lowered = db.to_vec();
    lowered[base + 5..base + 7].copy_from_slice(&((c - 4) as u16).to_be_bytes());
    misses.extend(plant(
        conn,
        &lowered,
        base + c - 4,
        &[value],
        &format!(
            "page {n}: byte {} of the content area, in no cell and no freeblock, is {value:#04x}",
            c - 4
        ),
    ));
    let mut freed = db.to_vec();
    let first = read2(db, base + 1);
    freed[base + 1..base + 3].copy_from_slice(&((c - 8) as u16).to_be_bytes());
    freed[base + 5..base + 7].copy_from_slice(&((c - 8) as u16).to_be_bytes());
    freed[base + c - 8..base + c - 6].copy_from_slice(&(first as u16).to_be_bytes());
    freed[base + c - 6..base + c - 4].copy_from_slice(&8u16.to_be_bytes());
    misses.extend(plant(
        conn,
        &freed,
        base + c - 1,
        &[value],
        &format!(
            "page {n}: byte {} of the freeblock at {} (size 8) is {value:#04x}",
            c - 1,
            c - 8
        ),
    ));
    misses.extend(plant(
        conn,
        db,
        base + t.pointers_end,
        &(PAGE as u16).to_be_bytes(),
        &format!(
            "page {n}: the gap's slot at {} holds {:#06x}, no offset of a cell past the slot",
            t.pointers_end, PAGE
        ),
    ));
    misses
        .into_iter()
        .map(|miss| format!("kind {kind}, page {n}: {miss}"))
        .collect()
}

#[test]
fn the_four_rules_of_a_tree_page_name_their_plants_on_each_kind_of_page_with_a_low_and_a_high_byte()
{
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(lines_of(&conn, &db), Vec::<String>::new());
    let mut misses = Vec::new();
    let mut taken = Vec::new();
    for kind in [2u8, 5, 10, 13] {
        taken.push(format!(
            "kind {kind}: page {}",
            tree_of_kind(&db, kind, 8).number
        ));
        for value in [0x01, 0x80] {
            misses.extend(four_rules(&conn, &db, kind, value));
        }
    }
    println!("CONTROLS pages taken: {}", taken.join(", "));
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

#[test]
fn a_low_and_a_high_byte_in_a_freelist_leaf_and_in_the_header_are_named() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(lines_of(&conn, &db), Vec::<String>::new());
    let trunk = read4(&db, 32);
    let leaf = read4(&db, page(trunk) + 8);
    let mut misses = Vec::new();
    for value in [0x01u8, 0x80] {
        for at in [0, PAGE - 1] {
            misses.extend(plant(
                &conn,
                &db,
                page(leaf) + at,
                &[value],
                &format!("page {leaf}: byte {at} of a freelist leaf is {value:#04x}"),
            ));
        }
        for at in [52, 55] {
            misses.extend(plant(
                &conn,
                &db,
                at,
                &[value],
                &format!("page 1: header byte {at} is {value:#04x}"),
            ));
        }
        for at in [72, 91] {
            misses.extend(plant(
                &conn,
                &db,
                at,
                &[value],
                &format!("page 1: header byte {at}, reserved for expansion, is {value:#04x}"),
            ));
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

#[test]
fn the_page_count_passes_in_a_trunk_and_a_tail_and_the_count_plus_1_is_named() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(lines_of(&conn, &db), Vec::<String>::new());
    let count = db.len() / PAGE;
    let trunk = read4(&db, 32);
    let past = 8 + 4 * read4(&db, page(trunk) + 4);
    assert!(
        past + 4 <= PAGE,
        "the first trunk has no room past its count"
    );
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
    let mut misses = Vec::new();
    for (at, named) in [
        (
            page(trunk) + past,
            format!("page {trunk}: the freelist trunk past its count holds at {past} a value above the page count"),
        ),
        (
            page(tail) + 4 + used,
            format!("page {tail}: the tail of an overflow page holds at {} a value above the page count", 4 + used),
        ),
    ] {
        let mut copy = db.to_vec();
        copy[at..at + 4].copy_from_slice(&(count as u32).to_be_bytes());
        let found = lines_of(&conn, &copy);
        if !found.is_empty() {
            misses.push(format!("the page count {count} at byte {at} of the file is named: {found:?}"));
        }
        misses.extend(plant(&conn, &db, at, &((count + 1) as u32).to_be_bytes(), &named));
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

#[test]
fn a_cell_that_takes_the_overflow_chain_of_another_names_a_page_of_2_chains_and_a_page_of_0() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(lines_of(&conn, &db), Vec::<String>::new());
    let (min, max) = ((PAGE - 12) * 32 / 255 - 23, PAGE - 35);
    let mut by_payload: BTreeMap<u64, Vec<(usize, usize)>> = BTreeMap::new();
    for number in 2..=db.len() / PAGE {
        let at = page(number);
        if db[at] != 13 {
            continue;
        }
        for slot in 0..read2(&db, at + 3) {
            let offset = read2(&db, at + 8 + 2 * slot);
            let (payload, head) = varint(&db[at + offset..]);
            let (_, rowid) = varint(&db[at + offset + head..]);
            let payload_size = payload as usize;
            if payload_size <= max {
                continue;
            }
            let k = min + (payload_size - min) % (PAGE - 4);
            let local = if k <= max { k } else { min };
            let pointer = at + offset + head + rowid + local;
            by_payload
                .entry(payload)
                .or_default()
                .push((pointer, read4(&db, pointer)));
        }
    }
    let (payload, cells) = by_payload
        .iter()
        .find(|(_, cells)| cells.len() >= 2)
        .expect("two table cells of equal payload that spill");
    let ((pointer, left), (_, shared)) = (cells[0], cells[1]);
    let mut copy = db.clone();
    copy[pointer..pointer + 4].copy_from_slice(&(shared as u32).to_be_bytes());
    let found = lines_of(&conn, &copy);
    for named in [
        format!("page {shared}: an overflow page that 2 chains of cells walk, not one"),
        format!("page {left}: an overflow page that 0 chains of cells walk, not one"),
    ] {
        assert!(
            found.iter().any(|line| line == &named),
            "cells of payload {payload}: the first page {left} set to {shared}: no line {named:?}: {found:?}"
        );
    }
}

#[test]
fn a_leaf_whose_type_byte_is_0_on_disk_is_named_as_a_page_that_dbstat_reads_corrupted() {
    crate::initialize().unwrap();
    let scratch = Scratch::new(46, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch("CREATE TABLE s(id INTEGER PRIMARY KEY, body TEXT NOT NULL); BEGIN;")
        .unwrap();
    for id in 1..=400i64 {
        conn.execute(
            "INSERT INTO s(id, body) VALUES (?1, ?2)",
            params![id, format!("s{id:05}{}", "b".repeat(60))],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    checkpoint(&conn);
    drop(conn);
    let db = std::fs::read(scratch.db()).unwrap();
    let leaf = (3..=db.len() / PAGE)
        .find(|&number| db[page(number)] == 13)
        .expect("a table leaf after page 2");
    write_at(&scratch.db(), page(leaf), &[0]);
    let conn = connect(&scratch.db(), Mode::Plain);
    let found = lines_of(&conn, &std::fs::read(scratch.db()).unwrap());
    let named = format!("page {leaf}: page type corrupted");
    assert!(
        found.iter().any(|line| line == &named),
        "the type byte of page {leaf} written to 0 on disk: no line {named:?}: {found:?}"
    );
}

#[test]
fn the_check_names_a_log_of_the_connections_file_that_holds_frames() {
    crate::initialize().unwrap();
    let scratch = Scratch::new(47, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch("CREATE TABLE s(id INTEGER PRIMARY KEY, body TEXT NOT NULL);")
        .unwrap();
    checkpoint(&conn);
    assert_eq!(
        lines_of(&conn, &std::fs::read(scratch.db()).unwrap()),
        Vec::<String>::new()
    );
    conn.execute("INSERT INTO s(id, body) VALUES (1, 'a row in the log')", [])
        .unwrap();
    let found = lines_of(&conn, &std::fs::read(scratch.db()).unwrap());
    assert!(
        found
            .iter()
            .any(|line| line.starts_with("the log of the connection's file holds ")),
        "a log with frames: no line names it: {found:?}"
    );
}

#[test]
fn a_draw_that_frees_a_second_trunk_leaves_no_text_and_its_plants_are_named() {
    crate::initialize().unwrap();
    let scratch = Scratch::new(48, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch("CREATE TABLE big(id INTEGER PRIMARY KEY, body TEXT NOT NULL); BEGIN;")
        .unwrap();
    let marker = |id: i64| format!("r{id:06}x");
    let ids = 1100i64;
    assert!(
        ids < 10_000,
        "the search below looks for the markers' prefix r00, which holds for ids below 10,000 alone: {ids} ids"
    );
    for id in 1..=ids {
        conn.execute(
            "INSERT INTO big(id, body) VALUES (?1, ?2)",
            params![id, format!("{}{}", marker(id), "b".repeat(3992))],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    checkpoint(&conn);
    conn.execute("DELETE FROM big", []).unwrap();
    checkpoint(&conn);
    let mut left = Vec::new();
    for entry in std::fs::read_dir(&scratch.0).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        for at in memchr::memmem::find_iter(&bytes, b"r00") {
            let Some(text) = bytes.get(at..at + 8) else {
                continue;
            };
            if text[1..7].iter().all(u8::is_ascii_digit) && text[7] == b'x' {
                left.push(format!(
                    "the removed text {} stands in {} page {} offset {}",
                    String::from_utf8_lossy(text),
                    path.file_name().unwrap().to_string_lossy(),
                    at / PAGE + 1,
                    at % PAGE
                ));
            }
        }
    }
    assert!(left.is_empty(), "a third place: {}", left.join("\n"));
    let db = std::fs::read(scratch.db()).unwrap();
    let read = rules::check(&conn, &db);
    assert_eq!(read.problems, Vec::<String>::new());
    assert!(read.trunks >= 2, "the freelist has {} trunks", read.trunks);
    let count = db.len() / PAGE;
    let second = read4(&db, page(read4(&db, 32)));
    let leaves = read4(&db, page(second) + 4);
    assert!(leaves > 0 && 12 + 4 * leaves <= PAGE);
    let last = read4(&db, page(second) + 4 + 4 * leaves);
    let mut misses = Vec::new();
    for at in [0, PAGE - 1] {
        misses.extend(plant(
            &conn,
            &db,
            page(last) + at,
            &[0x80],
            &format!("page {last}: byte {at} of a freelist leaf is 0x80"),
        ));
    }
    misses.extend(plant(
        &conn,
        &db,
        page(second) + 8 + 4 * leaves,
        &[0xff; 4],
        &format!(
            "page {second}: the freelist trunk past its count holds at {} a value above the page count",
            8 + 4 * leaves
        ),
    ));
    for (named, value) in [(0, 0), (count + 1, count + 1), (second, second)] {
        misses.extend(plant(
            &conn,
            &db,
            page(second) + 8,
            &(value as u32).to_be_bytes(),
            &format!("page {second}: freelist trunk names page {named}"),
        ));
    }
    println!(
        "CONTROLS second trunk: page {second} with {leaves} leaves, the last page {last}; {} pages, {} trunks, {} leaves",
        read.pages, read.trunks, read.leaves
    );
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

#[test]
fn plants_refuses_a_log_of_the_connections_file_that_holds_frames() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(rules::plants(&conn, &db), Vec::<String>::new());
    conn.execute(
        "INSERT INTO t(id, body) VALUES (999999, 'a row in the log')",
        [],
    )
    .unwrap();
    let answer = rules::plants(&conn, &std::fs::read(scratch.db()).unwrap());
    assert!(
        answer.len() == 1 && answer[0].starts_with("the log of the connection's file holds "),
        "plants() on a connection whose log holds frames does not answer with the log's line alone: {answer:?}"
    );
}

/// S-94 P1: on an image shaped like halvo-derived's, rows of about 5 KB among
/// small rows, the plant under 32 reserved bytes gives a cell with an overflow
/// chain a larger local size, and when that cell is the last of a leaf's
/// content area its overflow pointer lies past the usable size. The check
/// names that read as a line of its page, never a panic, and plants() still
/// finds each of its plants on the image. A page size of 0 and a file cut
/// inside its header are lines too.
#[test]
fn a_cell_read_past_the_usable_size_is_a_line_and_plants_name_every_plant_on_an_image_shaped_like_derived(
) {
    crate::initialize().unwrap();
    let scratch = store_shaped_like_derived();
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(rules::check(&conn, &db).problems, Vec::<String>::new());
    let misses = rules::plants(&conn, &db);
    assert!(
        misses.is_empty(),
        "plants() on an image shaped like derived's: {}",
        misses.join("\n")
    );
    let mut reserved = db.clone();
    reserved[20] = 32;
    let found = rules::check(&conn, &reserved).problems;
    assert!(
        found
            .iter()
            .any(|line| line.contains("the cell at byte") && line.contains("runs past byte 4064")),
        "under 32 reserved bytes no line names a cell that runs past the usable size: {found:?}"
    );
    let mut no_size = db.clone();
    no_size[16..18].copy_from_slice(&[0, 0]);
    let found = rules::check(&conn, &no_size).problems;
    assert!(
        found
            .iter()
            .any(|line| line.contains("page 1: page size 0 is no power of two from 512 up")),
        "a page size of 0: {found:?}"
    );
    let found = rules::check(&conn, &db[..37]).problems;
    assert!(
        found
            .iter()
            .any(|line| line.contains("page 1: the file header at byte 0 runs past byte 37")),
        "a file of 37 bytes: {found:?}"
    );
}

/// The draw of small_store with rows of 4,600 to 5,400 bytes in place of its
/// rows of 8,180, as halvo-derived's evidence rows are: one of them is the
/// last cell of a leaf's content area, and its local size grows with a smaller
/// usable size.
fn store_shaped_like_derived() -> Scratch {
    let scratch = Scratch::new(94, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch(
        "CREATE TABLE t(id INTEGER PRIMARY KEY, body TEXT NOT NULL);
         CREATE INDEX t_body ON t(body);",
    )
    .unwrap();
    let mut rng = super::Rng::new(94);
    for round in 0..4 {
        conn.execute_batch("BEGIN").unwrap();
        for at in 0..200 {
            let len = if at % 50 == 0 {
                4600 + rng.below(800) as usize
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
        checkpoint(&conn);
    }
    scratch
}

/// S-96 1: a table leaf whose one cell has a payload length of 2^64 less 1
/// and an overflow chain that starts at a page naming itself as its next. The
/// check answers within its own time limit, with the line of the page that
/// the chain meets a second time.
#[test]
fn an_overflow_chain_that_meets_a_page_a_second_time_is_a_line_and_the_check_answers() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let mut db = std::fs::read(scratch.db()).unwrap();
    let count = db.len() / PAGE;
    let leaf = (2..=count).find(|&n| db[page(n)] == 13).unwrap();
    let other = (2..=count).rev().find(|&n| n != leaf).unwrap();
    let payload = u64::MAX;
    let (max, min) = (PAGE as u64 - 35, (PAGE as u64 - 12) * 32 / 255 - 23);
    let k = min + (payload - min) % (PAGE as u64 - 4);
    let local = if k <= max { k } else { min } as usize;
    let at = PAGE - (9 + 1 + local + 4);
    let base = page(leaf);
    db[base..base + PAGE].fill(0);
    db[base] = 13;
    db[base + 3..base + 5].copy_from_slice(&1u16.to_be_bytes());
    db[base + 5..base + 7].copy_from_slice(&(at as u16).to_be_bytes());
    db[base + 8..base + 10].copy_from_slice(&(at as u16).to_be_bytes());
    db[base + at..base + at + 9].fill(0xff);
    db[base + at + 9] = 1;
    db[base + PAGE - 4..base + PAGE].copy_from_slice(&(other as u32).to_be_bytes());
    db[page(other)..page(other) + 4].copy_from_slice(&(other as u32).to_be_bytes());
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(rules::check(&conn, &db).problems);
    });
    let found = receiver
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap_or_else(|_| {
            panic!(
                "check() gave no answer in 30 s on an image whose overflow chain starts at page \
                 {other} and names page {other} as its next"
            )
        });
    let named = format!(
        "page {other}: one overflow chain meets this page a second time, so the walk of that chain stops there"
    );
    assert!(
        found.iter().any(|line| line == &named),
        "no line names the page that one chain meets twice: {found:?}"
    );
}
