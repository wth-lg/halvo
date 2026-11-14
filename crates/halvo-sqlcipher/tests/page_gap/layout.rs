//! The structural check: every page of a keyless file read by the
//! database's own layout. Every byte that holds no cell, no header and no
//! pointer must be zero: the rest of a freeblock after its 4 header bytes,
//! the content area that no cell and no freeblock covers, a leaf of the
//! freelist, and bytes 52 to 55 of the file header. The allowances, each a
//! limit of the check: in the gap between the pointer array and the content
//! area, a run of whole 2-byte slots from the end of the pointer array, in
//! step with it, whose values are offsets inside the page (stale pointers);
//! in the tail of an overflow page and in a freelist trunk past its count,
//! 4-byte values no larger than the page count. A leftover made only of such
//! values is not seen.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection};

use super::{connect, Mode, Rng, Scratch};

fn read2(page: &[u8], at: usize) -> usize {
    usize::from(u16::from_be_bytes([page[at], page[at + 1]]))
}

fn read4(page: &[u8], at: usize) -> usize {
    u32::from_be_bytes([page[at], page[at + 1], page[at + 2], page[at + 3]]) as usize
}

fn varint(bytes: &[u8]) -> (u64, usize) {
    let mut value = 0u64;
    for (at, byte) in bytes.iter().take(8).enumerate() {
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return (value, at + 1);
        }
    }
    ((value << 8) | u64::from(bytes[8]), 9)
}

fn local_size(payload: usize, usable: usize, table_leaf: bool) -> (usize, bool) {
    let max = if table_leaf {
        usable - 35
    } else {
        (usable - 12) * 64 / 255 - 23
    };
    if payload <= max {
        return (payload, false);
    }
    let min = (usable - 12) * 32 / 255 - 23;
    let k = min + (payload - min) % (usable - 4);
    (if k <= max { k } else { min }, true)
}

fn cell_size(page: &[u8], at: usize, kind: u8, usable: usize) -> (usize, Option<(usize, usize)>) {
    let child = if kind == 2 || kind == 5 { 4 } else { 0 };
    if kind == 5 {
        return (4 + varint(&page[at + 4..]).1, None);
    }
    let (payload, head) = varint(&page[at + child..]);
    let mut size = child + head;
    if kind == 13 {
        size += varint(&page[at + size..]).1;
    }
    let (local, spills) = local_size(payload as usize, usable, kind == 13);
    size += local;
    if !spills {
        return (size.max(4), None);
    }
    let first = read4(page, at + size);
    (size + 4, Some((first, payload as usize - local)))
}

fn btree_page(
    page: &[u8],
    number: usize,
    usable: usize,
    chains: &mut Vec<(usize, usize)>,
    problems: &mut Vec<String>,
) {
    let header = if number == 1 { 100 } else { 0 };
    let kind = page[header];
    let header_size = match kind {
        2 | 5 => 12,
        10 | 13 => 8,
        other => {
            problems.push(format!(
                "page {number}: type byte {other} is no b-tree page"
            ));
            return;
        }
    };
    let cells = read2(page, header + 3);
    let content = match read2(page, header + 5) {
        0 => 65536,
        start => start,
    };
    let pointers_end = header + header_size + 2 * cells;
    let mut covered = vec![false; usable];
    for at in 0..cells {
        let offset = read2(page, header + header_size + 2 * at);
        let (size, chain) = cell_size(page, offset, kind, usable);
        chains.extend(chain);
        for flag in covered.iter_mut().skip(offset).take(size) {
            *flag = true;
        }
    }
    let mut free = read2(page, header + 1);
    let mut blocks = 0;
    while free != 0 && blocks < usable {
        let size = read2(page, free + 2);
        if let Some(at) = (free + 4..free + size).find(|&at| page[at] != 0) {
            problems.push(format!(
                "page {number}: byte {at} of the freeblock at {free} (size {size}) is {:#04x}",
                page[at]
            ));
        }
        for flag in covered.iter_mut().skip(free).take(size) {
            *flag = true;
        }
        free = read2(page, free);
        blocks += 1;
    }
    if let Some(at) = (content..usable).find(|&at| !covered[at] && page[at] != 0) {
        problems.push(format!(
            "page {number}: byte {at} of the content area, in no cell and no freeblock, is {:#04x}",
            page[at]
        ));
    }
    let mut at = pointers_end;
    while at + 1 < content && (page[at] != 0 || page[at + 1] != 0) {
        let stale = read2(page, at);
        if stale >= page.len() {
            problems.push(format!(
                "page {number}: the gap's slot at {at} holds {stale:#06x}, no offset inside the page"
            ));
        }
        at += 2;
    }
    if let Some(left) = (at..content).find(|&left| page[left] != 0) {
        problems.push(format!(
            "page {number}: byte {left} of the gap {pointers_end}..{content}, past its run of stale \
             pointers that ends at {at}, is {:#04x}",
            page[left]
        ));
    }
}

fn words_no_larger_than(page: &[u8], from: usize, to: usize, count: usize) -> Option<usize> {
    (from..to).step_by(4).find(|&at| {
        let mut word = [0u8; 4];
        for (slot, byte) in word.iter_mut().zip(&page[at..to.min(at + 4)]) {
            *slot = *byte;
        }
        u32::from_be_bytes(word) as usize > count
    })
}

/// The leftovers of the keyless file `db`, whose pages `conn` names through
/// the database's page table, one line for each page that breaks a rule.
pub(super) fn leftovers(conn: &Connection, db: &[u8]) -> Vec<String> {
    read_pages(conn, db).0
}

fn read_pages(conn: &Connection, db: &[u8]) -> (Vec<String>, Vec<(usize, usize)>) {
    let size = match read2(db, 16) {
        1 => 65536,
        size => size,
    };
    let usable = size - usize::from(db[20]);
    let count = db.len() / size;
    let mut problems = Vec::new();
    let mut chains = Vec::new();
    if let Some(at) = (52..56).find(|&at| db[at] != 0) {
        problems.push(format!("page 1: header byte {at} is {:#04x}", db[at]));
    }
    let mut statement = conn
        .prepare("SELECT pageno, pagetype, unused FROM dbstat")
        .unwrap();
    let pages: BTreeMap<usize, (String, usize)> = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)? as usize,
                (r.get::<_, String>(1)?, r.get::<_, i64>(2)? as usize),
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut freelist: BTreeSet<usize> = BTreeSet::new();
    let mut trunk = read4(db, 32);
    while trunk != 0 && trunk <= count && freelist.insert(trunk) {
        let page = &db[(trunk - 1) * size..trunk * size];
        let leaves = read4(page, 4);
        for at in 0..leaves {
            let leaf = read4(page, 8 + 4 * at);
            freelist.insert(leaf);
            if leaf == 0 || leaf > count {
                problems.push(format!("page {trunk}: freelist trunk names page {leaf}"));
                continue;
            }
            let bytes = &db[(leaf - 1) * size..leaf * size];
            if let Some(at) = bytes.iter().position(|byte| *byte != 0) {
                problems.push(format!(
                    "page {leaf}: byte {at} of a freelist leaf is {:#04x}",
                    bytes[at]
                ));
            }
        }
        if let Some(at) = words_no_larger_than(page, 8 + 4 * leaves, usable, count) {
            problems.push(format!(
                "page {trunk}: the freelist trunk past its count holds at {at} a value above the page count"
            ));
        }
        trunk = read4(page, 0);
    }
    for number in 1..=count {
        let page = &db[(number - 1) * size..number * size];
        match pages.get(&number) {
            Some((kind, _)) if kind == "internal" || kind == "leaf" => {
                btree_page(page, number, usable, &mut chains, &mut problems)
            }
            Some((kind, _)) if kind == "overflow" => {}
            Some((kind, _)) => problems.push(format!("page {number}: page type {kind}")),
            None if freelist.contains(&number) => {}
            None => problems.push(format!("page {number}: in no tree and not on the freelist")),
        }
    }
    let mut tails = Vec::new();
    for (first, mut left) in chains {
        let mut number = first;
        while number != 0 && number <= count && left > 0 {
            let page = &db[(number - 1) * size..number * size];
            let used = left.min(usable - 4);
            left -= used;
            if left == 0 {
                tails.push((number, 4 + used));
                if let Some(at) = words_no_larger_than(page, 4 + used, usable, count) {
                    problems.push(format!(
                        "page {number}: the tail of an overflow page holds at {at} a value above the page count"
                    ));
                }
            }
            number = read4(page, 0);
        }
    }
    (problems, tails)
}

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

#[test]
fn the_structural_check_sees_what_a_delete_leaves_with_secure_delete_off() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Unsecured);
    let conn = connect(&scratch.db(), Mode::Unsecured);
    let found = leftovers(&conn, &std::fs::read(scratch.db()).unwrap());
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
    assert_eq!(leftovers(&conn, &db), Vec::<String>::new());
    let size = read2(&db, 16);
    let (number, at) = (2..=db.len() / size)
        .find_map(|number| {
            let page = &db[(number - 1) * size..number * size];
            let content = read2(page, 5);
            let pointers_end = 8 + 2 * read2(page, 3);
            (matches!(page[0], 10 | 13) && content >= pointers_end + 8)
                .then(|| (number, content - 4))
        })
        .expect("a leaf with a gap of at least 4 bytes");
    db[(number - 1) * size + at..(number - 1) * size + at + 4].copy_from_slice(b"text");
    let found = leftovers(&conn, &db);
    assert!(
        found
            .iter()
            .any(|line| line.starts_with(&format!("page {number}: ")) && line.contains("gap")),
        "the structural check does not see 4 bytes planted in the gap of page {number}: {found:?}"
    );
}

fn planted(db: &[u8], conn: &Connection, at: usize, bytes: &[u8], named: &str) -> Option<String> {
    let mut copy = db.to_vec();
    copy[at..at + bytes.len()].copy_from_slice(bytes);
    let found = leftovers(conn, &copy);
    if found.iter().any(|line| line.contains(named)) {
        None
    } else {
        Some(format!(
            "{bytes:?} planted at byte {at}: no line names {named:?}: {found:?}"
        ))
    }
}

#[test]
fn the_structural_check_sees_a_byte_planted_in_each_place_that_it_reads() {
    crate::initialize().unwrap();
    let scratch = small_store(Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    let db = std::fs::read(scratch.db()).unwrap();
    assert_eq!(leftovers(&conn, &db), Vec::<String>::new());
    let size = read2(&db, 16);
    let page = |number: usize| (number - 1) * size;
    let mut misses = Vec::new();
    misses.extend(planted(&db, &conn, 52, &[1], "header byte 52"));
    let trunk = read4(&db, 32);
    assert!(trunk != 0, "the store has no freelist");
    let trunk_page = &db[page(trunk)..page(trunk) + size];
    let leaves = read4(trunk_page, 4);
    assert!(leaves > 0, "the freelist trunk names no leaf");
    let leaf = read4(trunk_page, 8);
    misses.extend(planted(
        &db,
        &conn,
        page(leaf) + 100,
        &[1],
        "of a freelist leaf",
    ));
    misses.extend(planted(
        &db,
        &conn,
        page(trunk) + size - 4,
        &[0xff; 4],
        "the freelist trunk past its count",
    ));
    let (number, _) = read_pages(&conn, &db)
        .1
        .into_iter()
        .find(|(_, tail)| *tail + 4 <= size)
        .expect("an overflow page with a tail of at least 4 bytes");
    misses.extend(planted(
        &db,
        &conn,
        page(number) + size - 4,
        &[0xff; 4],
        "the tail of an overflow page",
    ));
    let (number, free) = (2..=db.len() / size)
        .find_map(|number| {
            let at = page(number);
            let free = read2(&db, at + 1);
            (matches!(db[at], 2 | 5 | 10 | 13) && free != 0 && read2(&db, at + free + 2) > 4)
                .then_some((number, free))
        })
        .expect("a page with a freeblock longer than its header");
    misses.extend(planted(
        &db,
        &conn,
        page(number) + free + 4,
        &[1],
        "of the freeblock at",
    ));
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}
