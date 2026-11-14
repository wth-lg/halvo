//! The structural check: every page of a keyless image of a database read by
//! the database's own layout. Every byte up to the usable size that holds no
//! cell, no header and no pointer must be zero: the rest of a freeblock after
//! its 4 header bytes, the content area that no cell and no freeblock covers,
//! the gap past its run of stale slots, a leaf of the freelist, and bytes 52
//! to 55 and 72 to 91 of the file header. The reserved tail of a page past the
//! usable size is the codec's and is not read. The allowances, each a limit
//! of the check: in the gap between the pointer array and the content area, a
//! run of whole 2-byte slots from the end of the pointer array, in step with
//! it, whose values lie between the slot's own offset plus 2 and the usable
//! size less 4 (stale pointers); in the tail of an overflow page and in a
//! freelist trunk past its count, 4-byte values no larger than the page
//! count. A leftover made only of such values is not seen. Every page that
//! dbstat names overflow must be walked by exactly one chain of the cells that
//! the check reads, every page must be read as a b-tree page, an overflow
//! page, a freelist trunk or a freelist leaf, and the four must add up to the
//! page count; a page that dbstat cannot read as a tree page ("corrupted") or
//! whose type byte is no b-tree page is a line. [`check`] also names a log of
//! the connection's file that holds frames. This file uses `std` and
//! `rusqlite` alone, so that another crate's tests can include it by a path
//! attribute; such a caller runs [`plants`] on its own images and reads every
//! field of [`Checked`]. The controls of the rules that need pages a caller's
//! image may lack stand in the tests page_gap of halvo-sqlcipher.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

/// What [`check`] found and what it read: the lines of the rules that a page
/// breaks, the pages of the image, and how many of them it read as a b-tree
/// page, an overflow page, a freelist trunk and a freelist leaf, and how many
/// overflow tails it read. A caller requires `problems` to be empty and the
/// four kinds to add up to `pages`.
pub struct Checked {
    pub problems: Vec<String>,
    pub pages: usize,
    pub btree: usize,
    pub overflow: usize,
    pub trunks: usize,
    pub leaves: usize,
    pub overflow_tails: usize,
}

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
        return (size, None);
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
        if stale < at + 2 || stale > usable - 4 {
            problems.push(format!(
                "page {number}: the gap's slot at {at} holds {stale:#06x}, no offset of a cell past \
                 the slot ({}..={})",
                at + 2,
                usable - 4
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

fn read_pages(conn: &Connection, db: &[u8]) -> (Checked, Vec<(usize, usize)>) {
    let size = match read2(db, 16) {
        1 => 65536,
        size => size,
    };
    let usable = size - usize::from(db[20]);
    let count = db.len() / size;
    let (mut btree, mut overflow, mut trunks, mut leaves_read) = (0, 0, 0, 0);
    let mut problems = Vec::new();
    let mut chains = Vec::new();
    if size > 4096 {
        problems.push(format!(
            "page 1: page size {size} above 4,096: the check reads no page of such a file"
        ));
        let checked = Checked {
            problems,
            pages: count,
            btree,
            overflow,
            trunks,
            leaves: leaves_read,
            overflow_tails: 0,
        };
        return (checked, Vec::new());
    }
    if let Some(at) = (52..56).find(|&at| db[at] != 0) {
        problems.push(format!("page 1: header byte {at} is {:#04x}", db[at]));
    }
    if let Some(at) = (72..92).find(|&at| db[at] != 0) {
        problems.push(format!(
            "page 1: header byte {at}, reserved for expansion, is {:#04x}",
            db[at]
        ));
    }
    let mut statement = conn.prepare("SELECT pageno, pagetype FROM dbstat").unwrap();
    let kinds: BTreeMap<usize, String> = statement
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)? as usize, r.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut freelist: BTreeSet<usize> = BTreeSet::new();
    let mut trunk = read4(db, 32);
    while trunk != 0 && trunk <= count && freelist.insert(trunk) {
        trunks += 1;
        let page = &db[(trunk - 1) * size..trunk * size];
        let leaves = read4(page, 4);
        for at in 0..leaves {
            let leaf = read4(page, 8 + 4 * at);
            if leaf == 0 || leaf > count || !freelist.insert(leaf) {
                problems.push(format!("page {trunk}: freelist trunk names page {leaf}"));
                continue;
            }
            leaves_read += 1;
            let bytes = &db[(leaf - 1) * size..(leaf - 1) * size + usable];
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
        match kinds.get(&number).map(String::as_str) {
            Some("internal" | "leaf") => {
                btree += 1;
                btree_page(page, number, usable, &mut chains, &mut problems)
            }
            Some("overflow") => overflow += 1,
            Some(kind) => problems.push(format!("page {number}: page type {kind}")),
            None if freelist.contains(&number) => {}
            None => problems.push(format!("page {number}: in no tree and not on the freelist")),
        }
    }
    let read = btree + overflow + trunks + leaves_read;
    if read != count {
        problems.push(format!(
            "page 1: the check read {read} of the {count} pages of the file as a b-tree page, an \
             overflow page, a freelist trunk or a freelist leaf"
        ));
    }
    let mut tails = Vec::new();
    let mut walks: BTreeMap<usize, usize> = BTreeMap::new();
    for (first, mut left) in chains {
        let mut number = first;
        while number != 0 && number <= count && left > 0 {
            *walks.entry(number).or_default() += 1;
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
    for (number, kind) in &kinds {
        let walked = walks.get(number).copied().unwrap_or(0);
        if kind == "overflow" && walked != 1 {
            problems.push(format!(
                "page {number}: an overflow page that {walked} chains of cells walk, not one"
            ));
        }
    }
    let checked = Checked {
        problems,
        pages: count,
        btree,
        overflow,
        trunks,
        leaves: leaves_read,
        overflow_tails: tails.len(),
    };
    (checked, tails)
}

/// Reads every page of the keyless image `db`, whose pages `conn` names
/// through the database's page table (dbstat), by the rules of this file.
pub fn check(conn: &Connection, db: &[u8]) -> Checked {
    let mut checked = read_pages(conn, db).0;
    if let Some(path) = conn.path().filter(|path| !path.is_empty()) {
        let log = format!("{path}-wal");
        let length = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
        if length > 0 {
            checked.problems.push(format!(
                "the log of the connection's file holds {length} bytes: the check reads the main \
                 file alone, so checkpoint the log with TRUNCATE first"
            ));
        }
    }
    checked
}

fn planted(db: &[u8], conn: &Connection, at: usize, bytes: &[u8], named: &str) -> Option<String> {
    let mut copy = db.to_vec();
    copy[at..at + bytes.len()].copy_from_slice(bytes);
    let found = check(conn, &copy).problems;
    if found.iter().any(|line| line.contains(named)) {
        None
    } else {
        Some(format!(
            "{bytes:?} planted at byte {at}: no line names {named:?}: {found:?}"
        ))
    }
}

/// Plants in copies of the caller's own image `db`, which must pass the
/// check, and requires the line that names each: header byte 52; the first
/// leaf of the first freelist trunk at byte 100 and at its last usable byte;
/// that trunk's last 4 bytes, its first entry set to 0, to the page count
/// plus 1 and to the trunk itself, and a b-tree page added to its entries; a
/// page appended to the file; the type byte of the first leaf after page 1
/// set to 0; the last 4 bytes of an overflow tail with room; a byte past the
/// header of the first freeblock longer than 4 bytes; text in a freeblock of
/// 8 bytes or more shortened by 4; in the first leaf with 8 bytes of gap past
/// its stale run, the page size in the first slot, a valid offset past the run
/// and, under 32 reserved bytes, an offset inside the page past the usable
/// size; and a page size of 8,192. Returns one line for each plant that no
/// line names, and for each such page that the image lacks; an empty answer
/// is the pass. It shows that the check sees these plants on the caller's
/// image, not that the caller's draw would leave anything.
pub fn plants(conn: &Connection, db: &[u8]) -> Vec<String> {
    let (clean, tails) = read_pages(conn, db);
    if !clean.problems.is_empty() {
        return clean.problems;
    }
    let size = read2(db, 16);
    let page = |number: usize| (number - 1) * size;
    let mut misses = Vec::new();
    misses.extend(planted(db, conn, 52, &[1], "header byte 52"));
    let trunk = read4(db, 32);
    if trunk == 0 || read4(db, page(trunk) + 4) == 0 {
        misses.push("the image has no freelist trunk with a leaf".to_owned());
    } else {
        let leaf = read4(db, page(trunk) + 8);
        misses.extend(planted(
            db,
            conn,
            page(leaf) + 100,
            &[1],
            "of a freelist leaf",
        ));
        misses.extend(planted(
            db,
            conn,
            page(trunk) + size - 4,
            &[0xff; 4],
            "the freelist trunk past its count",
        ));
        let count = db.len() / size;
        for (named, value) in [(0, 0), (count + 1, count + 1), (trunk, trunk)] {
            misses.extend(planted(
                db,
                conn,
                page(trunk) + 8,
                &(value as u32).to_be_bytes(),
                &format!("page {trunk}: freelist trunk names page {named}"),
            ));
        }
        let leaves = read4(db, page(trunk) + 4);
        match (2..=count).find(|&number| matches!(db[page(number)], 2 | 5 | 10 | 13)) {
            Some(tree) if 12 + 4 * leaves <= size => {
                let mut copy = db.to_vec();
                copy[page(trunk) + 4..page(trunk) + 8]
                    .copy_from_slice(&((leaves + 1) as u32).to_be_bytes());
                misses.extend(planted(
                    &copy,
                    conn,
                    page(trunk) + 8 + 4 * leaves,
                    &(tree as u32).to_be_bytes(),
                    &format!("page 1: the check read {} of the {count} pages", count + 1),
                ));
            }
            _ => misses.push("the image has no b-tree page and room in its first trunk".to_owned()),
        }
    }
    let count = db.len() / size;
    let mut longer = db.to_vec();
    longer.resize(db.len() + size, 0);
    let found = check(conn, &longer).problems;
    let named = format!("page {}: in no tree and not on the freelist", count + 1);
    if !found.iter().any(|line| line.contains(&named)) {
        misses.push(format!(
            "a page appended to the file: no line names {named:?}: {found:?}"
        ));
    }
    match (2..=count).find(|&number| matches!(db[page(number)], 10 | 13)) {
        Some(number) => misses.extend(planted(
            db,
            conn,
            page(number),
            &[0],
            &format!("page {number}: type byte 0 is no b-tree page"),
        )),
        None => misses.push("the image has no leaf after page 1".to_owned()),
    }
    match tails.iter().find(|(_, tail)| *tail + 4 <= size) {
        Some((number, _)) => misses.extend(planted(
            db,
            conn,
            page(*number) + size - 4,
            &[0xff; 4],
            "the tail of an overflow page",
        )),
        None => misses.push("the image has no overflow tail of 4 bytes".to_owned()),
    }
    let leaf_kinds = |at: usize| matches!(db[at], 2 | 5 | 10 | 13);
    match (2..=db.len() / size).find_map(|number| {
        let at = page(number);
        let free = read2(db, at + 1);
        (leaf_kinds(at) && free != 0 && read2(db, at + free + 2) > 4).then_some((number, free))
    }) {
        Some((number, free)) => misses.extend(planted(
            db,
            conn,
            page(number) + free + 4,
            &[1],
            "of the freeblock at",
        )),
        None => misses.push("the image has no freeblock longer than its header".to_owned()),
    }
    let usable = size - usize::from(db[20]);
    if trunk != 0 && read4(db, page(trunk) + 4) != 0 {
        let leaf = read4(db, page(trunk) + 8);
        misses.extend(planted(
            db,
            conn,
            page(leaf) + usable - 1,
            &[1],
            "of a freelist leaf",
        ));
    }
    match (2..=db.len() / size).find_map(|number| {
        let at = page(number);
        let free = read2(db, at + 1);
        (leaf_kinds(at) && free != 0 && read2(db, at + free + 2) >= 8).then_some((at, free))
    }) {
        Some((at, free)) => {
            let mut copy = db.to_vec();
            let length = read2(db, at + free + 2);
            copy[at + free + 2..at + free + 4]
                .copy_from_slice(&((length - 4) as u16).to_be_bytes());
            misses.extend(planted(
                &copy,
                conn,
                at + free + length - 4,
                b"text",
                "of the content area, in no cell and no freeblock",
            ));
        }
        None => misses.push("the image has no freeblock of 8 bytes".to_owned()),
    }
    let gap = (2..=db.len() / size).find_map(|number| {
        let at = page(number);
        let content = read2(db, at + 5);
        let mut run = 8 + 2 * read2(db, at + 3);
        while run + 1 < content && (db[at + run] != 0 || db[at + run + 1] != 0) {
            run += 2;
        }
        (matches!(db[at], 10 | 13) && content >= run + 8).then_some((
            at,
            8 + 2 * read2(db, at + 3),
            run,
            content,
        ))
    });
    match gap {
        Some((at, pointers_end, run, content)) => {
            misses.extend(planted(
                db,
                conn,
                at + pointers_end,
                &(size as u16).to_be_bytes(),
                &format!("the gap's slot at {pointers_end} holds {size:#06x}"),
            ));
            let offset = (content as u16).to_be_bytes();
            misses.extend(planted(
                db,
                conn,
                at + run,
                &[0, 0, offset[0], offset[1]],
                "past its run of stale pointers",
            ));
            let mut reserved = db.to_vec();
            reserved[20] = 32;
            let inside_the_page = (size - 16) as u16;
            misses.extend(planted(
                &reserved,
                conn,
                at + pointers_end,
                &inside_the_page.to_be_bytes(),
                &format!("the gap's slot at {pointers_end} holds {inside_the_page:#06x}"),
            ));
        }
        None => {
            misses.push("the image has no leaf with 8 bytes of gap past its stale run".to_owned())
        }
    }
    misses.extend(planted(
        db,
        conn,
        16,
        &[0x20, 0x00],
        "page size 8192 above 4,096",
    ));
    misses
}
