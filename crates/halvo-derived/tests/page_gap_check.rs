mod common;

#[path = "../../halvo-sqlcipher/tests/page_gap/rules.rs"]
mod rules;

use common::{episode, position, registry, store_id, Grouping, Scratch, Seq, TestClock};
use halvo_derived::test_host::open_keyless;
use halvo_derived::{Host, Store};
use halvo_schema_core::record::{Episode, FloorLayer, Ordinal, Tagged, Text};
use halvo_sqlcipher::rusqlite::Connection;

const DAY: u32 = 20_767;

/// The evidence rows of the draw, each naming six segments, so that `sources` holds six small cells for each.
const ROWS: u64 = 120;

/// The single removals of the draw.
const STEPS: usize = 100;

/// A drawn sequence of numbers, the same for the same seed (xorshift).
struct Draw(u64);

impl Draw {
    fn order(&mut self, n: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            order.swap(i, (self.0 % (i as u64 + 1)) as usize);
        }
        order
    }
}

fn keyless(scratch: &Scratch) -> Store {
    let host = Host::new(
        Box::new(TestClock::at_day(DAY)),
        Box::new(Grouping { min: 1_000 }),
    );
    open_keyless(scratch.path(), &registry(), host, &mut Seq(1 << 40)).unwrap()
}

/// The marker of the title of evidence row `i` of a store of `seed`.
fn marker(seed: u64, i: u64) -> String {
    format!("qzgap{seed}x{i:04}q")
}

fn titled(mut e: Episode, title: String) -> Episode {
    e.steps[0].title = Some(Tagged {
        value: Text::parse(&title).unwrap(),
        layer: FloorLayer::Floor,
    });
    e
}

/// A keyless store of `seed`: `ROWS` evidence rows of six segments each, so that the table `sources`, the smallest of
/// the store, holds many small cells, and twelve evidence rows among them of about 3,500 to 4,400 bytes, of stepped
/// sizes, whose cells spill onto overflow pages, so that some of them keep the least local part and leave room in the
/// tail of their last overflow page, which `plants` needs; all written in one batch; the store is closed.
fn drawn_store(seed: u64) -> Scratch {
    let scratch = Scratch::new(&format!("page-gap-check-{seed}"));
    let mut store = keyless(&scratch);
    let mut episodes: Vec<Episode> = (0..ROWS)
        .map(|i| {
            let mut e = episode(1_000 + i, DAY, None);
            e.segments = (0..6)
                .map(|k| Ordinal::new(10_000 + i * 10 + k).unwrap())
                .collect();
            titled(e, marker(seed, i))
        })
        .collect();
    for b in 0..12u64 {
        let mut e = episode(3_000 + b, DAY, None);
        let step = e.steps[0].clone();
        let each = (3_500 + b as usize * 80) / 5;
        e.steps = (0..5)
            .map(|k| {
                let mut s = step.clone();
                s.title = Some(Tagged {
                    value: Text::parse(&format!("big{b:02}{k}{}", "z".repeat(each))).unwrap(),
                    layer: FloorLayer::Floor,
                });
                s
            })
            .collect();
        episodes.insert(10 + 9 * b as usize, e);
    }
    store
        .commit_batch(&store_id(1), &episodes, position(1, 0), &mut Seq(100))
        .unwrap();
    drop(store);
    scratch
}

/// A keyless connection of the test's own on the database of the closed store, through which dbstat reads, and the
/// bytes of the main file.
fn image(scratch: &Scratch) -> (Connection, Vec<u8>) {
    halvo_derived::test_host::start_sqlite();
    let conn = Connection::open(scratch.db()).unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    let bytes = std::fs::read(scratch.db()).unwrap();
    (conn, bytes)
}

/// The file's own page count: the header's count at byte 28.
fn header_pages(bytes: &[u8]) -> usize {
    u32::from_be_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]) as usize
}

/// The entries of `derived/` under `scratch` whose bytes hold `marker` whole.
fn holding(scratch: &Scratch, marker: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(scratch.path().join("derived")).unwrap() {
        let entry = entry.unwrap();
        let bytes = std::fs::read(entry.path()).unwrap();
        if bytes.windows(marker.len()).any(|w| w == marker.as_bytes()) {
            found.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    found
}

/// The check's reading of the image of the closed store under `scratch`: the problems, and whether the four kinds of
/// pages add up to the pages read, which equal the file's own page count.
fn checked(scratch: &Scratch) -> (rules::Checked, bool) {
    let (conn, bytes) = image(scratch);
    let read = rules::check(&conn, &bytes);
    let sums = read.btree + read.overflow + read.trunks + read.leaves == read.pages
        && read.pages == header_pages(&bytes);
    (read, sums)
}

/// S-83 7 (b), S-85 item 4 and S-94 P2: the structural check of halvo-sqlcipher's tests, included here by a path
/// attribute, on the files of a keyless derived store. For each seed, a store as `drawn_store` writes it; then
/// `STEPS` single removals in a drawn order, each the purge of one ordinal of one evidence row, a checkpointed call
/// that ends with the truncating checkpoint, the store closed after each. After each removal the removed row's marker
/// is in no entry of `derived/`, the check names no problem, and its four kinds of pages add up to the pages that it
/// read, which equal the file's own page count. The last image holds a freelist trunk with a leaf and an overflow
/// tail, and `plants` names every byte that it plants in it: the control that the check sees what it is to see on
/// this store's own pages. On a build of the database without the statement of editPage (the row
/// edited-page-gap-not-zeroed of crates/halvo-sqlcipher/mutants.rows) this test fails by the check's own lines.
#[test]
fn small_rows_removed_one_at_a_time_leave_no_line_of_the_structural_check() {
    let mut lines = Vec::new();
    for seed in [1u64, 2, 3] {
        let scratch = drawn_store(seed);
        let order = Draw(0x9e37_79b9_7f4a_7c15 ^ seed).order(ROWS as usize);
        for (step, &i) in order.iter().take(STEPS).enumerate() {
            let mut store = keyless(&scratch);
            let removed = store
                .purge(&[Ordinal::new(10_000 + i as u64 * 10).unwrap()])
                .unwrap();
            assert_eq!(removed, 1, "seed {seed}, step {step}");
            drop(store);
            let marker = marker(seed, i as u64);
            assert_eq!(
                holding(&scratch, &marker),
                Vec::<String>::new(),
                "seed {seed}, step {step}: {marker}"
            );
            let (read, sums) = checked(&scratch);
            assert!(
                sums,
                "seed {seed}, step {step}: the kinds of pages do not add up"
            );
            lines.extend(
                read.problems
                    .iter()
                    .map(|p| format!("seed {seed}, step {step}: {p}")),
            );
        }
        let (conn, bytes) = image(&scratch);
        let read = rules::check(&conn, &bytes);
        assert!(
            read.trunks >= 1 && read.leaves >= 1 && read.overflow_tails >= 1,
            "seed {seed}: the last image lacks a page kind that plants needs"
        );
        assert_eq!(
            rules::plants(&conn, &bytes),
            Vec::<String>::new(),
            "seed {seed}: the plants that the check does not name"
        );
    }
    assert_eq!(
        lines,
        Vec::<String>::new(),
        "the lines of the structural check"
    );
}

/// The draw of the test above, with secure delete off: the includer's own control (rules.rs, the caller's duties),
/// which must produce lines. The store of a seed is written and closed as `drawn_store` writes it; then a connection
/// of the test's own with secure delete off deletes the same evidence rows one at a time, each with its cells of
/// `sources`, and checkpoints the log with TRUNCATE after each; the check names a line after some of the steps.
#[test]
fn the_same_draw_with_secure_delete_off_leaves_lines_of_the_structural_check() {
    let scratch = drawn_store(1);
    let order = Draw(0x9e37_79b9_7f4a_7c15 ^ 1).order(ROWS as usize);
    let (conn, _) = image(&scratch);
    conn.execute_batch("PRAGMA secure_delete = OFF; PRAGMA foreign_keys = ON;")
        .unwrap();
    let mut with_lines = 0;
    for &i in order.iter().take(STEPS) {
        let row: i64 = conn
            .query_row(
                "SELECT row FROM sources WHERE ordinal = ?1",
                [10_000 + i as i64 * 10],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            conn.execute("DELETE FROM rows WHERE id = ?1", [row])
                .unwrap(),
            1
        );
        let busy: i64 = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, 0);
        let bytes = std::fs::read(scratch.db()).unwrap();
        if !rules::check(&conn, &bytes).problems.is_empty() {
            with_lines += 1;
        }
    }
    assert!(with_lines > 0, "no step with secure delete off left a line");
    println!("steps with lines of the check, secure delete off: {with_lines} of {STEPS}");
}
