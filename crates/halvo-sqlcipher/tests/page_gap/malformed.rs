use rusqlite::{params, Connection};

use super::{connect, Mode, Scratch, PAGE};

fn checkpoint(conn: &Connection) {
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
        .unwrap();
    assert_eq!(busy, 0);
}

fn read2(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]))
}

pub(super) fn varint(bytes: &[u8]) -> (u64, usize) {
    let mut value = 0u64;
    for (at, byte) in bytes.iter().take(9).enumerate() {
        if at == 8 {
            return ((value << 8) | u64::from(*byte), 9);
        }
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return (value, at + 1);
        }
    }
    unreachable!("a varint of a cell runs past the page")
}

fn integrity(conn: &Connection) -> Vec<String> {
    let mut statement = conn.prepare("PRAGMA integrity_check").unwrap();
    let lines = statement
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    lines
}

fn body(id: i64) -> String {
    format!("row{id:05}-{}", "q".repeat(30))
}

#[test]
fn on_a_leaf_whose_content_start_lies_above_live_cells_the_integrity_check_refuses_it_and_deletes_lose_those_cells(
) {
    crate::initialize().unwrap();
    let scratch = Scratch::new(41, Mode::Plain);
    let conn = connect(&scratch.db(), Mode::Plain);
    conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY, body TEXT); BEGIN;")
        .unwrap();
    for id in 1..=600i64 {
        conn.execute(
            "INSERT INTO t(id, body) VALUES (?1, ?2)",
            params![id, body(id)],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    checkpoint(&conn);
    drop(conn);
    let mut bytes = std::fs::read(scratch.db()).unwrap();
    let leaves: Vec<usize> = (1..bytes.len() / PAGE)
        .filter(|page| bytes[page * PAGE] == 13)
        .collect();
    let base = leaves[2] * PAGE;
    let raised = read2(&bytes, base + 5) + 200;
    bytes[base + 5..base + 7].copy_from_slice(&(raised as u16).to_be_bytes());
    std::fs::write(scratch.db(), &bytes).unwrap();
    let (mut low, mut high) = (Vec::new(), Vec::new());
    for slot in 0..read2(&bytes, base + 3) {
        let offset = read2(&bytes, base + 8 + 2 * slot);
        let (_, size) = varint(&bytes[base + offset..]);
        let (rowid, _) = varint(&bytes[base + offset + size..]);
        if offset < raised { &mut low } else { &mut high }.push(rowid as i64);
    }
    assert!(
        !low.is_empty(),
        "no live cell lies below the raised content start {raised}"
    );
    let conn = connect(&scratch.db(), Mode::Plain);
    let before = integrity(&conn);
    let named = format!(" out of range {raised}..{}", PAGE - 4);
    assert!(
        before.iter().any(|line| line.contains(&named)),
        "before the first delete the integrity check does not name a cell below the content start \
         ({named}): {before:?}"
    );
    for id in high.iter().take(60) {
        assert_eq!(
            conn.execute("DELETE FROM t WHERE id = ?1", [id]).unwrap(),
            1
        );
    }
    let kept: Vec<i64> = low
        .iter()
        .copied()
        .filter(|id| {
            conn.query_row("SELECT body FROM t WHERE id = ?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .map(|text| text == body(*id))
            .unwrap_or(false)
        })
        .collect();
    assert!(
        kept.is_empty(),
        "the head's build changed: of the cells {low:?} below the content start, {kept:?} read back \
         after the deletes; read the editPage statement and the bounded sentence of ADR 0013 again"
    );
}

#[test]
fn under_a_key_one_byte_changed_on_disk_in_a_b_tree_page_fails_the_read() {
    crate::initialize().unwrap();
    let scratch = Scratch::new(42, Mode::Keyed);
    let conn = connect(&scratch.db(), Mode::Keyed);
    conn.execute_batch("CREATE TABLE t(id INTEGER PRIMARY KEY, body TEXT); BEGIN;")
        .unwrap();
    for id in 1..=600i64 {
        conn.execute(
            "INSERT INTO t(id, body) VALUES (?1, ?2)",
            params![id, body(id)],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    checkpoint(&conn);
    drop(conn);
    let scan = |conn: &Connection| -> rusqlite::Result<(i64, i64)> {
        conn.query_row("SELECT count(*), sum(length(body)) FROM t", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
    };
    let conn = connect(&scratch.db(), Mode::Keyed);
    assert_eq!(scan(&conn).unwrap(), (600, 600 * 39));
    let root: i64 = conn
        .query_row(
            "SELECT rootpage FROM sqlite_schema WHERE name = 't'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(conn);
    let mut bytes = std::fs::read(scratch.db()).unwrap();
    let at = (root as usize - 1) * PAGE + 1000;
    bytes[at] ^= 0x01;
    std::fs::write(scratch.db(), &bytes).unwrap();
    let conn = connect(&scratch.db(), Mode::Keyed);
    let read = scan(&conn);
    assert!(
        read.is_err(),
        "with byte {at} of page {root} changed on disk the keyed read gave {read:?}"
    );
}
