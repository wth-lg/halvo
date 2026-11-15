mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    episode, own_note_body, position, raw_request, registry, store_id, summary_id, Fixed, OwnNote,
    Scratch, Seq, TestClock, OWN,
};
use halvo_derived::test_host::open_keyless;
use halvo_derived::{Host, Store, Takes};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{RequestId, SummaryId, WorkflowId};
use halvo_schema_core::kinds::KindEntry;
use halvo_schema_core::record::{
    Confidence, Episode, FloorLayer, Proposal, Recurrence, Tagged, Text, Trigger,
};
use halvo_schema_core::request::{InterfaceDecision, InterfaceRole};
use halvo_schema_core::site::{AppId, Domain, DomainSource, SiteDomain, SiteValue};

const DAY: u32 = 20_767;

/// A drawn sequence of numbers, the same for the same seed (xorshift).
struct Draw(u64);

impl Draw {
    fn new(seed: u64) -> Draw {
        Draw(0x9e37_79b9_7f4a_7c15 ^ seed.wrapping_mul(0x1_0001_0001))
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A length of made-up text from 20 to 950 bytes, short, middling and long in a drawn mix.
    fn length(&mut self) -> usize {
        (match self.below(6) {
            0 => 20 + self.below(60),
            1 => 700 + self.below(250),
            _ => 80 + self.below(500),
        }) as usize
    }

    /// Made-up text of `len` lower-case letters.
    fn text(&mut self, len: usize) -> String {
        (0..len)
            .map(|_| char::from(b'a' + self.below(26) as u8))
            .collect()
    }

    /// The numbers `0..n` in a drawn order.
    fn order(&mut self, n: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            order.swap(i, self.below(i as u64 + 1) as usize);
        }
        order
    }
}

/// The forms in which a marker can stand in a file: UTF-8, UTF-16LE and UTF-16BE.
fn forms(marker: &str) -> [Vec<u8>; 3] {
    [
        marker.as_bytes().to_vec(),
        marker.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        marker.encode_utf16().flat_map(u16::to_be_bytes).collect(),
    ]
}

/// The name and the bytes of every entry of `derived/` under `scratch`, in the order of their names, from a listing
/// of the directory with no list of names of its own. An entry that is no regular file fails the test with its name.
fn entries_of(scratch: &Scratch) -> Vec<(String, Vec<u8>)> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(scratch.path().join("derived"))
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    entries
        .into_iter()
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(
                entry.metadata().unwrap().is_file(),
                "{name} is no regular file"
            );
            (name, std::fs::read(entry.path()).unwrap())
        })
        .collect()
}

/// The names of the entries of `files` whose bytes hold `marker` whole in one of its forms. Its limit: a marker that
/// crosses the end of a page is not found whole, which is why each test first asserts that each marker is found while
/// its value is stored.
fn holding(files: &[(String, Vec<u8>)], marker: &str) -> Vec<String> {
    let forms = forms(marker);
    files
        .iter()
        .filter(|(_, bytes)| forms.iter().any(|form| contains(bytes, form)))
        .map(|(name, _)| name.clone())
        .collect()
}

/// Whether `pattern`, of two bytes or more, stands whole in `bytes`.
fn contains(bytes: &[u8], pattern: &[u8]) -> bool {
    let (a, b) = (pattern[0], pattern[1]);
    bytes
        .windows(pattern.len())
        .any(|w| w[0] == a && w[1] == b && w == pattern)
}

/// A marker that a search found in the files where none may stand: the step, the marker and the entries.
type Left = (String, String, Vec<String>);

/// Searches every entry of `derived/` for each of `markers` and adds each one found to `left` under `step`.
fn left_after(scratch: &Scratch, step: &str, markers: &[String], left: &mut Vec<Left>) {
    let files = entries_of(scratch);
    for marker in markers {
        let found = holding(&files, marker);
        if !found.is_empty() {
            left.push((step.to_owned(), marker.clone(), found));
        }
    }
}

/// Asserts that each of `markers` is found in the files: the control of a search.
fn found_now(scratch: &Scratch, step: &str, markers: &[String]) {
    let files = entries_of(scratch);
    for marker in markers {
        assert!(
            !holding(&files, marker).is_empty(),
            "{step}, the control: {marker} is in the files while its value is stored or before the next checkpointed call"
        );
    }
}

fn site(name: &str) -> SiteValue {
    SiteValue::Domain {
        key: SiteDomain::from(Domain::parse(name).unwrap()),
        source: DomainSource::AddressField,
    }
}

/// An episode of summary number `n` on `day` under the key of `site` with the site in the address field, or of the app
/// `app` with no site, whose title is `len` bytes of made-up text.
fn drawn_episode(
    draw: &mut Draw,
    n: u64,
    day: u32,
    site: Option<&str>,
    app: &str,
    len: usize,
) -> Episode {
    let mut e = episode(n, day, site.and_then(common::domain));
    e.app = AppId::parse(app).unwrap();
    e.steps[0].title = Some(Tagged {
        value: Text::parse(&draw.text(len)).unwrap(),
        layer: FloorLayer::Floor,
    });
    e
}

/// A proposal of the key of `site`, or of `app` where `site` is none, with the app `app` over the summaries `ids`.
fn proposal(site: Option<&str>, app: &str, ids: &[SummaryId]) -> Proposal {
    Proposal {
        category: SeedCategory::UtilityBillPayment,
        site: site.map(self::site),
        app: AppId::parse(app).unwrap(),
        trigger: Trigger::Manual,
        recurrence: Recurrence {
            count: ids.len() as u32,
            period: None,
            first: DAY,
            last: DAY,
        },
        confidence: Confidence(60),
        summaries: ids.to_vec(),
    }
}

fn keyless(scratch: &Scratch, host: Host) -> Store {
    let mut entries: Vec<&'static KindEntry> = registry();
    entries.push(&OWN);
    open_keyless(scratch.path(), &entries, host, &mut Seq(1 << 40)).unwrap()
}

fn delete(store: &mut Store, id: &WorkflowId) {
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([9; 16]),
            id.clone(),
        ))
        .unwrap();
}

/// The id of the stored workflow whose app is `app`.
fn workflow_of(store: &Store, app: &str) -> WorkflowId {
    common::listed(store)
        .into_iter()
        .find(|item| {
            matches!(&item.workflow, halvo_schema_core::record::Workflow::Full(full) if full.app.as_str() == app)
        })
        .map(|item| item.workflow.id().clone())
        .unwrap()
}

/// The number of the summary `j` of the key `k` of a drawn store of the seed `seed`: a mix of the three below 2^48, so
/// that the ids of a store, 32 hex digits each, stand in a drawn order in the indexes that hold them, among each other
/// and among those of other keys, and each is a marker that no other value of a test holds.
fn marked(seed: u64, k: u64, j: u64) -> u64 {
    let mut z = seed
        .wrapping_mul(1_000_003)
        .wrapping_add(k.wrapping_mul(10_007))
        .wrapping_add(j)
        .wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)) & 0x0000_ffff_ffff_ffff
}

/// The first shape of S-80, item 3 (U5.1d, part 9), one drawn keyless store: four workflows whose keys hold a site, each
/// with an app that is a marker and three summaries, one of them close to its bound, each made between two runs of 15
/// rows of origin own, of drawn lengths, of a filler workflow of its own, so that its rows stand on pages among the
/// filler's. Then for each marked workflow in turn: its filler is deleted, so that the pages that hold the marked rows
/// are left nearly empty and the database merges them and builds them again; a batch gives the marked workflow another
/// app of another length and drops the support row of its summary close to its bound; both markers are found in the
/// files (the control); the expiry call takes that summary past its bound; and every entry of `derived/` is searched
/// for both markers. After the delete of the marked workflows and after the close every marker of the store is
/// searched again. It returns each marker found where none may stand.
fn large_store(seed: u64) -> Vec<Left> {
    let scratch = Scratch::new(&format!("replaced-large-{seed}"));
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed)))).handler(
        "test",
        Takes::Runner,
        Box::new(OwnNote),
    );
    let mut store = keyless(&scratch, host);
    let mut draw = Draw::new(seed);
    let sites: Vec<String> = (0..4).map(|k| format!("qzm{k}s{seed}.test")).collect();
    let old_apps: Vec<String> = (0..4)
        .map(|k| {
            format!(
                "com.example.qzold{k}s{seed}{}",
                "o".repeat(draw.below(9) as usize)
            )
        })
        .collect();
    let new_apps: Vec<String> = (0..4)
        .map(|k| {
            format!(
                "com.example.qznew{k}s{seed}{}",
                "n".repeat(10 + draw.below(9) as usize)
            )
        })
        .collect();
    let mut at = 0;
    let mut make =
        |store: &mut Store, draw: &mut Draw, proposal: Proposal, episodes: &[Episode]| {
            *fixed.borrow_mut() = vec![proposal.clone()];
            at += 1;
            let out = store
                .commit_batch(&store_id(1), episodes, position(at, 0), &mut Seq(100 * at))
                .unwrap();
            assert_eq!(out.proposals.stored, 1, "seed {seed}");
            *fixed.borrow_mut() = Vec::new();
            let _ = draw;
            workflow_of(store, proposal.app.as_str())
        };
    let notes = |store: &mut Store, draw: &mut Draw, owner: &WorkflowId| {
        for _ in 0..15 {
            let len = draw.length();
            let note = draw.text(len);
            store
                .handle(&raw_request(
                    "runner",
                    &own_note_body(owner.as_str(), &note),
                ))
                .unwrap();
        }
    };
    let (mut marked_ids, mut fill_ids) = (Vec::new(), Vec::new());
    for k in 0..4u64 {
        let app = format!("com.example.fill{k}");
        let number = marked(seed, 40 + k, 0);
        let len = draw.length();
        let fill_episode = drawn_episode(&mut draw, number, DAY, None, &app, len);
        let fill = make(
            &mut store,
            &mut draw,
            proposal(None, &app, &[summary_id(number)]),
            &[fill_episode],
        );
        notes(&mut store, &mut draw, &fill);
        let numbers: Vec<u64> = (0..3).map(|j| marked(seed, k, j)).collect();
        let mut episodes = Vec::new();
        for (j, &n) in numbers.iter().enumerate() {
            let day = if j == 0 { DAY - 90 + 3 * k as u32 } else { DAY };
            let len = draw.length();
            episodes.push(drawn_episode(
                &mut draw,
                n,
                day,
                Some(&sites[k as usize]),
                "com.example.browser",
                len,
            ));
        }
        let summaries: Vec<SummaryId> = numbers.iter().map(|n| summary_id(*n)).collect();
        let id = make(
            &mut store,
            &mut draw,
            proposal(Some(&sites[k as usize]), &old_apps[k as usize], &summaries),
            &episodes,
        );
        notes(&mut store, &mut draw, &fill);
        marked_ids.push(id);
        fill_ids.push(fill);
    }
    store.expire().unwrap();
    let pages = std::fs::metadata(scratch.db()).unwrap().len() / 4096;
    assert!(pages >= 30, "seed {seed}: the database holds {pages} pages");
    let mut left = Vec::new();
    for k in 0..4usize {
        delete(&mut store, &fill_ids[k]);
        let kept: Vec<SummaryId> = (1..3)
            .map(|j| summary_id(marked(seed, k as u64, j)))
            .collect();
        *fixed.borrow_mut() = vec![proposal(Some(&sites[k]), &new_apps[k], &kept)];
        at += 1;
        let out = store
            .commit_batch(&store_id(1), &[], position(at, 0), &mut Seq(100 * at))
            .unwrap();
        assert_eq!(out.proposals.updated, 1, "seed {seed}, workflow {k}");
        let markers = [
            old_apps[k].clone(),
            summary_id(marked(seed, k as u64, 0)).as_str().to_owned(),
        ];
        found_now(&scratch, &format!("seed {seed}, workflow {k}"), &markers);
        clock.set_day(DAY + 3 + 3 * k as u32);
        assert!(store.expire().unwrap() >= 2, "seed {seed}, workflow {k}");
        left_after(
            &scratch,
            &format!("workflow {k}, after the expiry call"),
            &markers,
            &mut left,
        );
    }
    let every: Vec<String> = old_apps
        .iter()
        .cloned()
        .chain((0..4).map(|k| summary_id(marked(seed, k, 0)).as_str().to_owned()))
        .collect();
    for id in &marked_ids {
        delete(&mut store, id);
    }
    left_after(
        &scratch,
        "after the delete of the marked workflows",
        &every,
        &mut left,
    );
    drop(store);
    left_after(&scratch, "after the close", &every, &mut left);
    left
}

/// The second shape of S-83 (f) for S-80, item 3 (U5.1d, part 9), one drawn keyless store: one workflow whose key holds
/// a site, supported by 80 summaries whose ids are markers, with titles of 20 to 59 bytes, so that its rows and their
/// index entries stand as many small cells on a page, beside the summaries and evidence of 80 more summaries of six
/// other keys, all in a drawn order. Step by step, in a drawn order, one batch gives the workflow another app, a marker
/// of a drawn length, and drops one support row, whose summary the next checkpointed call (the expiry call) takes past
/// its bound: one removal at a time, each with its truncating checkpoint. Right after each batch both markers of that
/// step are found in the files (the control), and right after the checkpointed call every entry of `derived/` is
/// searched for both. After the delete of the workflow and after the close every marker of the store is searched
/// again. It returns each marker found where none may stand.
fn small_cells(seed: u64) -> Vec<Left> {
    const N: usize = 80;
    let scratch = Scratch::new(&format!("replaced-small-{seed}"));
    let start = DAY - 85;
    let clock = TestClock::at_day(start + N as u32);
    let fixed = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = keyless(&scratch, host);
    let mut draw = Draw::new(seed);
    let site = format!("qzsmall{seed}.test");
    let drop_order = draw.order(N);
    let mut day_of = vec![0u32; N];
    for (step, &i) in drop_order.iter().enumerate() {
        day_of[i] = start + step as u32;
    }
    let ids: Vec<SummaryId> = (0..N as u64)
        .map(|i| summary_id(marked(seed, 50, i)))
        .collect();
    let apps: Vec<String> = (0..N)
        .map(|s| {
            format!(
                "com.example.qzapp{s:03}s{seed}{}",
                "y".repeat(draw.below(12) as usize)
            )
        })
        .collect();
    let mut episodes = Vec::new();
    for (i, &day) in day_of.iter().enumerate() {
        let len = 20 + draw.below(40) as usize;
        let n = marked(seed, 50, i as u64);
        episodes.push(drawn_episode(
            &mut draw,
            n,
            day,
            Some(&site),
            "com.example.browser",
            len,
        ));
    }
    for n in 0..N as u64 {
        let len = 20 + draw.below(40) as usize;
        let app = format!("com.example.other{}", n % 6);
        episodes.push(drawn_episode(
            &mut draw,
            marked(seed, 60, n),
            start + N as u32,
            None,
            &app,
            len,
        ));
    }
    let episodes: Vec<Episode> = draw
        .order(episodes.len())
        .into_iter()
        .map(|i| episodes[i].clone())
        .collect();
    *fixed.borrow_mut() = vec![proposal(Some(&site), &apps[0], &ids)];
    store
        .commit_batch(&store_id(1), &episodes, position(1, 0), &mut Seq(100))
        .unwrap();
    let workflow = workflow_of(&store, &apps[0]);
    store.expire().unwrap();
    let mut left = Vec::new();
    for (step, &i) in drop_order.iter().enumerate().take(N - 1) {
        clock.set_day(start + step as u32 + 93);
        let mut kept: Vec<SummaryId> = drop_order[step + 1..]
            .iter()
            .map(|&j| ids[j].clone())
            .collect();
        kept.sort();
        *fixed.borrow_mut() = vec![proposal(Some(&site), &apps[step + 1], &kept)];
        let out = store
            .commit_batch(
                &store_id(1),
                &[],
                position(2 + step as u64, 0),
                &mut Seq(200),
            )
            .unwrap();
        assert_eq!(out.proposals.updated, 1, "seed {seed}, step {step}");
        let markers = [apps[step].clone(), ids[i].as_str().to_owned()];
        found_now(&scratch, &format!("seed {seed}, step {step}"), &markers);
        assert!(store.expire().unwrap() >= 2, "seed {seed}, step {step}");
        left_after(
            &scratch,
            &format!("step {step}, after the expiry call"),
            &markers,
            &mut left,
        );
    }
    delete(&mut store, &workflow);
    let every: Vec<String> = apps
        .iter()
        .cloned()
        .chain(ids.iter().map(|id| id.as_str().to_owned()))
        .collect();
    left_after(
        &scratch,
        "after the delete of the workflow",
        &every,
        &mut left,
    );
    drop(store);
    left_after(&scratch, "after the close", &every, &mut left);
    left
}

/// The seeds of the small-cell test: chosen so that, at a head whose build of the database leaves the gaps of the pages
/// that it builds or edits as they were (9087f2d7 on this Mac, a drawn search over seeds 1 to 6), each store leaves a
/// marker (seed 2 nine, seed 4 three), and on the same head with the mended build laid over it none.
const SMALL_SEEDS: [u64; 2] = [2, 4];

/// The seeds of the test on pages built again. No seed of 1 to 12 left a marker at 9087f2d7 on this Mac, so this
/// test fails first only under a changed copy that takes the checkpoint off the next checkpointed call.
const LARGE_SEEDS: [u64; 2] = [1, 2];

/// Runs `store` for each of `seeds` on a thread of its own and returns, by seed, the markers found where none may
/// stand.
fn drawn(seeds: [u64; 2], store: fn(u64) -> Vec<Left>) -> Vec<(u64, Vec<Left>)> {
    std::thread::scope(|scope| {
        let runs: Vec<_> = seeds
            .iter()
            .map(|&seed| scope.spawn(move || (seed, store(seed))))
            .collect();
        runs.into_iter()
            .map(|run| run.join().unwrap())
            .filter(|(_, left)| !left.is_empty())
            .collect()
    })
}

/// S-80, item 3, on a store large enough that the database builds pages again (U5.1d, part 9): for each seed, a store
/// as `large_store` draws it; a replaced app and a dropped support row are in no byte of `derived/` after the next
/// checkpointed call, after the delete of the workflow and after the close.
#[test]
fn a_replaced_app_and_a_dropped_support_row_leave_no_marker_on_pages_built_again() {
    assert_eq!(
        drawn(LARGE_SEEDS, large_store),
        vec![],
        "(seed, markers found where none may stand)"
    );
}

/// S-80, item 3, in the second shape of S-83 (f) (U5.1d, part 9): for each seed, a store as `small_cells` draws it;
/// with one support row dropped at a time and a truncating checkpoint after each, the replaced app and the dropped
/// row are in no byte of `derived/` after the checkpointed call, after the delete of the workflow and after the close.
/// It fails on a build of the database that leaves the gaps of the pages that it builds or edits as they were.
#[test]
fn single_drops_of_small_support_rows_each_with_its_checkpoint_leave_no_marker() {
    assert_eq!(
        drawn(SMALL_SEEDS, small_cells),
        vec![],
        "(seed, markers found where none may stand)"
    );
}
