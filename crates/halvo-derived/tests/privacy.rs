mod common;

use common::{
    episode, listed, own_note_body, position, raw_request, registry, store_id, Grouping, OwnNote,
    Scratch, Seq, TestClock, DAY_NS, FAR, OWN,
};
use halvo_derived::test_host::open_keyless;
use halvo_derived::{Host, Store, Takes};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::RequestId;
use halvo_schema_core::kinds::KindEntry;
use halvo_schema_core::record::{Episode, FloorLayer, Instant, Ordinal, Tagged, Text};
use halvo_schema_core::request::{InterfaceDecision, InterfaceRole};
use halvo_schema_core::site::AppId;

const DAY: u32 = 20_767;

fn ord(n: u64) -> Ordinal {
    Ordinal::new(n).unwrap()
}

/// The episode `n` of `day` in the app `app`, whose one observed step holds `title`.
fn titled(n: u64, day: u32, app: &str, title: &str) -> Episode {
    let mut episode = episode(n, day, None);
    episode.app = AppId::parse(app).unwrap();
    episode.steps[0].title = Some(Tagged {
        value: Text::parse(title).unwrap(),
        layer: FloorLayer::Floor,
    });
    episode
}

/// The forms in which a marker can stand in a file: UTF-8, UTF-16LE and UTF-16BE.
fn forms(marker: &str) -> [Vec<u8>; 3] {
    [
        marker.as_bytes().to_vec(),
        marker.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        marker.encode_utf16().flat_map(u16::to_be_bytes).collect(),
    ]
}

/// The files of the store under `scratch`, of the database, its write-ahead log and its shared-memory file, whose
/// bytes hold `marker` in one of its forms.
fn holding(scratch: &Scratch, marker: &str) -> Vec<&'static str> {
    let dir = scratch.path().join("derived");
    let mut found = Vec::new();
    for name in ["derived.db", "derived.db-wal", "derived.db-shm"] {
        let bytes = match std::fs::read(dir.join(name)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("{name} cannot be read: {e}"),
        };
        let held = forms(marker)
            .iter()
            .any(|form| bytes.windows(form.len()).any(|w| w == form.as_slice()));
        if held {
            found.push(name);
        }
    }
    found
}

fn with(extra: &[&'static KindEntry]) -> Vec<&'static KindEntry> {
    let mut entries = registry();
    entries.extend_from_slice(extra);
    entries
}

fn keyless(scratch: &Scratch, entries: &[&'static KindEntry], host: Host) -> Store {
    open_keyless(scratch.path(), entries, host, &mut Seq(1 << 40)).unwrap()
}

const SCRUB_EXPIRY: &str = "qzscrube5501";
const SCRUB_PURGE: &str = "qzscrubp5502";
const SCRUB_DELETE: &str = "qzscrubd5503";
const SCRUB_OWN: &str = "qzscrubo5504";

/// privacy_scrub: in the keyless test build, which keeps every setting of the product and leaves the key out, a
/// marker that the test planted in a row that an expiry, a purge or the delete of a workflow removes is in no byte of
/// the database, its write-ahead log or its shared-memory file after them, while every marker stood in those files
/// before them. The rows are an evidence row past its expiry, an evidence row whose segment is purged, the evidence of
/// a deleted workflow's summaries and a row of a kind of origin own that names it.
#[test]
fn privacy_scrub() {
    let scratch = Scratch::new("privacy-scrub");
    let clock = TestClock::at_day(DAY);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 2 })).handler(
        "test",
        Takes::Runner,
        Box::new(OwnNote),
    );
    let mut store = keyless(&scratch, &with(&[&OWN]), host);
    let mut expiring = titled(1, DAY, "com.example.expiry", &format!("Pay {SCRUB_EXPIRY}"));
    expiring.expires = Instant(i64::from(DAY + 1) * DAY_NS);
    let purged = titled(2, DAY, "com.example.purge", &format!("Pay {SCRUB_PURGE}"));
    let mut deleted: Vec<Episode> = (3..=4)
        .map(|n| titled(n, DAY, "com.example.delete", &format!("Pay {SCRUB_DELETE}")))
        .collect();
    for episode in &mut deleted {
        episode.category = SeedCategory::ShopOrder;
        episode.expires = Instant(FAR);
    }
    let mut batch = vec![expiring, purged];
    batch.extend(deleted);
    let outcome = store
        .commit_batch(&store_id(1), &batch, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(
        (outcome.summaries, outcome.evidence, outcome.proposals.stored),
        (4, 4, 1)
    );
    let items = listed(&store);
    assert_eq!(items.len(), 1, "one workflow, of the two deleted episodes");
    let workflow = items[0].workflow.id().clone();
    store
        .handle(&raw_request(
            "runner",
            &own_note_body(workflow.as_str(), &format!("Note {SCRUB_OWN}")),
        ))
        .unwrap();
    let markers = [SCRUB_EXPIRY, SCRUB_PURGE, SCRUB_DELETE, SCRUB_OWN];
    for marker in markers {
        assert!(
            !holding(&scratch, marker).is_empty(),
            "the search finds {marker} while its row is stored"
        );
    }
    clock.set_day(DAY + 2);
    assert_eq!(store.expire().unwrap(), 1, "the expiry removes one row");
    assert_eq!(store.purge(&[ord(2)]).unwrap(), 1, "the purge removes one row");
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            workflow,
        ))
        .unwrap();
    for marker in markers {
        assert_eq!(
            holding(&scratch, marker),
            Vec::<&str>::new(),
            "after the expiry, the purge and the delete, the files that hold {marker}"
        );
    }
    drop(store);
    for marker in markers {
        assert_eq!(
            holding(&scratch, marker),
            Vec::<&str>::new(),
            "after the store is closed, the files that hold {marker}"
        );
    }
}
