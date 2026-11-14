use halvo_coord::items::{compose_plain, Plain};
use halvo_coord::roster::{names, order, plain_site, row, status, Placed};
use halvo_coord::sync::{Next, Sync};
use halvo_coord::types::fixtures::{facts, State};
use halvo_coord::types::{Item, ItemKind, Limits, Row, Snapshot};

/// The rows and the items of a state of the fixtures, built here from the roster's functions
/// and not by `Coordinator::compose`, so that the tests of this file hold for `Sync` alone;
/// `coordinator_moves.rs` runs a page against the coordinator's own snapshots.
fn state(s: State) -> (Vec<Row>, Vec<Item>, Option<String>) {
    let all = facts(s);
    let refs: Vec<_> = all.iter().collect();
    let names = names(&refs, |w| w.site.as_deref().and_then(plain_site)).unwrap();
    let mut placed: Vec<Placed> = all
        .iter()
        .zip(&names)
        .map(|(f, n)| Placed {
            row: row(f, status(f, &Limits::PRODUCT, None), n).unwrap(),
            raised: None,
        })
        .collect();
    order(&mut placed);
    let items = all
        .iter()
        .zip(&names)
        .filter_map(|(f, n)| {
            let open = f.doing.item()?;
            (open.kind == ItemKind::Ready)
                .then(|| compose_plain(&open.item, &f.agent, Plain::Ready, n).unwrap())
        })
        .collect();
    let note = (s == State::NoService).then(|| "note".to_owned());
    (placed.into_iter().map(|p| p.row).collect(), items, note)
}

/// A page that holds the last whole state that it received and asks for the next one with its
/// `seq`, as I8.2 has it.
struct Page {
    held: Snapshot,
}

impl Page {
    fn shown(sync: &Sync) -> Page {
        Page {
            held: sync.snapshot(),
        }
    }

    fn next(&mut self, sync: &Sync) -> bool {
        match sync.next(self.held.seq) {
            Next::Changed(s) => {
                self.held = s;
                true
            }
            Next::Same => false,
        }
    }
}

#[test]
fn sync_alone_a_snapshot_and_the_changes_that_follow_it_give_the_state_of_a_new_snapshot() {
    let mut sync = Sync::new();
    let mut page = Page::shown(&sync);
    let mut late = Page::shown(&sync);
    let order = [
        State::Ten,
        State::Ten,
        State::Working,
        State::Queue,
        State::Queue,
        State::NeedsReady,
        State::NeedsReady,
        State::NoService,
        State::Empty,
        State::Ten,
    ];
    for (k, s) in order.iter().enumerate() {
        let (rows, items, note) = state(*s);
        let before = sync.snapshot();
        let changed = sync.publish(rows.clone(), items.clone(), note.clone());
        let after = sync.snapshot();
        let same = before.rows == rows && before.items == items && before.note == note;
        assert_eq!(changed, !same, "step {k}");
        assert_eq!(after.seq, before.seq + u32::from(changed), "step {k}");
        assert_eq!((after.rows, after.items, after.note), (rows, items, note));
        assert_eq!(page.next(&sync), changed, "step {k}");
        assert_eq!(page.held, sync.snapshot(), "step {k}");
        if k % 3 == 2 {
            late.next(&sync);
            assert_eq!(late.held, sync.snapshot(), "step {k}");
        }
    }
    assert!(!page.next(&sync), "no change, no reply but the same one");
}

#[test]
fn sync_alone_the_reply_with_no_change_holds_the_old_seq_and_empty_lists() {
    let mut sync = Sync::new();
    let (rows, items, note) = state(State::Ten);
    sync.publish(rows, items, note);
    let same = sync.same();
    assert_eq!(same.seq, sync.snapshot().seq);
    assert!(same.rows.is_empty() && same.items.is_empty() && same.note.is_none());
    assert!(matches!(sync.next(sync.snapshot().seq), Next::Same));
}

#[test]
fn sync_alone_a_page_whose_seq_is_not_the_syncs_gets_the_whole_state() {
    let mut sync = Sync::new();
    assert_eq!(sync.snapshot().seq, 1);
    let (rows, items, note) = state(State::Ten);
    sync.publish(rows, items, note);
    let seq = sync.snapshot().seq;
    for after in [0, seq - 1, seq + 5] {
        match sync.next(after) {
            Next::Changed(s) => assert_eq!(s, sync.snapshot()),
            Next::Same => panic!("after {after}"),
        }
    }
}
