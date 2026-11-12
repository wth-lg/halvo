mod common;

use common::{
    bodies, count, episode, open, position, store_id, summary_id, Scratch, Seq, TestClock,
};
use halvo_schema_core::record::{Episode, Evidence, Ordinal};

fn ord(n: u64) -> Ordinal {
    Ordinal::new(n).unwrap()
}

fn spanning(n: u64, segments: &[u64]) -> Episode {
    let mut e = episode(n, 20_767, None);
    e.segments = segments.iter().map(|s| ord(*s)).collect();
    e
}

fn evidence_summaries(scratch: &Scratch) -> Vec<String> {
    bodies(scratch, "evidence")
        .iter()
        .map(|b| {
            serde_json::from_str::<Evidence>(b)
                .unwrap()
                .summary
                .as_str()
                .to_owned()
        })
        .collect()
}

fn revision(scratch: &Scratch) -> i64 {
    count(scratch, "SELECT n FROM revisions WHERE family = 'workflow'")
}

#[test]
fn purge_removes_every_window_row_that_names_a_shredded_ordinal() {
    let scratch = Scratch::new("purge");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[
                spanning(1, &[10, 11]),
                spanning(2, &[12]),
                spanning(3, &[11, 13]),
            ],
            position(13, 0),
            &mut Seq(1),
        )
        .unwrap();
    drop(store);
    let before = revision(&scratch);
    let mut store = open(&scratch, &clock).unwrap();
    assert_eq!(store.purge(&[ord(11), ord(11), ord(40)]).unwrap(), 2);
    assert_eq!(store.purge(&[]).unwrap(), 0);
    drop(store);
    assert_eq!(evidence_summaries(&scratch), vec![summary_id(2).as_str()]);
    assert_eq!(bodies(&scratch, "summary").len(), 3);
    assert_eq!(count(&scratch, "SELECT count(*) FROM sources"), 1);
    assert_eq!(revision(&scratch), before + 1);
}

#[test]
fn purge_below_removes_every_window_row_below_the_first_live_ordinal() {
    let scratch = Scratch::new("purge-below");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[spanning(5, &[5]), spanning(6, &[6, 9]), spanning(7, &[7])],
            position(9, 0),
            &mut Seq(1),
        )
        .unwrap();
    assert_eq!(store.purge_below(ord(7)).unwrap(), 2);
    assert_eq!(store.purge_below(ord(3)).unwrap(), 0);
    drop(store);
    assert_eq!(evidence_summaries(&scratch), vec![summary_id(7).as_str()]);
    assert_eq!(count(&scratch, "SELECT mark FROM meta"), 7);
}

#[test]
fn a_commit_stores_no_window_row_that_names_a_purged_ordinal() {
    let scratch = Scratch::new("purge-then-commit");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    assert_eq!(store.purge(&[ord(20)]).unwrap(), 0);
    assert_eq!(store.purge_below(ord(10)).unwrap(), 0);
    drop(store);
    let mut store = open(&scratch, &clock).unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[
                spanning(1, &[20, 21]),
                spanning(2, &[9]),
                spanning(3, &[10, 21]),
            ],
            position(21, 0),
            &mut Seq(1),
        )
        .unwrap();
    assert_eq!(
        (outcome.summaries, outcome.evidence, outcome.dropped),
        (3, 1, 2)
    );
    drop(store);
    assert_eq!(evidence_summaries(&scratch), vec![summary_id(3).as_str()]);
}

#[test]
fn the_high_water_mark_rises_over_purged_ordinals_that_follow_it() {
    let scratch = Scratch::new("mark");
    let clock = TestClock::at_day(20_767);
    let mut store = open(&scratch, &clock).unwrap();
    store.purge(&[ord(1), ord(3)]).unwrap();
    store.purge(&[ord(0)]).unwrap();
    store.purge(&[ord(2)]).unwrap();
    drop(store);
    assert_eq!(count(&scratch, "SELECT mark FROM meta"), 4);
    assert_eq!(count(&scratch, "SELECT count(*) FROM purged"), 0);
    let mut store = open(&scratch, &clock).unwrap();
    store.purge(&[ord(9)]).unwrap();
    store.purge_below(ord(6)).unwrap();
    drop(store);
    assert_eq!(count(&scratch, "SELECT mark FROM meta"), 6);
    assert_eq!(count(&scratch, "SELECT ordinal FROM purged"), 9);
}
