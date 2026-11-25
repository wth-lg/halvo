//! One development persona of the generated sample, fed through the episode step with stage A, one scripted day
//! per call, and then through the recurrence step: each of its planted routines is proposed and no other task,
//! the work between its run marks is not, and no step holds the card number of its card page.
//!
//! Beside it stands `hit_rates`, a counting tool that runs by hand and in no CI job. It feeds every stream of the
//! script sets seen and unseen through the same path and prints, for each kind of task and each set, how many
//! planted routines the recurrence step finds and how much of what it proposes is planted. `counting_a_tiny_case`
//! checks its count on a stream made by hand.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use halvo_identify::{episode_step, Call, Proposer, StageA};
use halvo_sample::build::render_stream;
use halvo_sample::labels::{rows_of, Row};
use halvo_sample::script::{Script, TaskKind};
use halvo_schema::{
    Category, DomainSource, Episode, Instant, ObservedSite, Position, RecurrenceStep, SeedCategory,
    SiteDomain, SiteHash, SiteValue, Summary, SummaryId, SummaryRead,
};
use halvo_seam::message::{Entry, Expiry};
use halvo_seam::record::{CaptureRecord, Payload, Title};

const DAY: i64 = 86_400 * 1_000_000_000;

/// The app of the persona's documents, which hold no site.
const OFFICE: &str = "org.libreoffice.script";

/// The persona's 12 planted routines on a site: the category, the site and the times that the script plants it.
const ON_A_SITE: [(SeedCategory, &str, u32); 12] = [
    (
        SeedCategory::UtilityBillPayment,
        "netherby-broadband.test",
        3,
    ),
    (SeedCategory::TenantRent, "larkspur-lettings.test", 4),
    (
        SeedCategory::TenantApplicationReview,
        "holloway-tenant-check.test",
        5,
    ),
    (SeedCategory::Email, "corvid-post.test", 6),
    (SeedCategory::Email, "lanternbox-mail.test", 5),
    (SeedCategory::SpreadsheetFill, "gridwell-sheets.test", 4),
    (SeedCategory::ShopOrder, "bramble-goods.test", 6),
    (SeedCategory::ShopOrder, "pinecart.test", 6),
    (SeedCategory::CalendarCleanup, "ledgerline-calendar.test", 6),
    (SeedCategory::CalendarCleanup, "tickmark-planner.test", 6),
    (SeedCategory::RideOrder, "swiftfare-rides.test", 4),
    (SeedCategory::DailyChartCheck, "steadypulse-health.test", 6),
];

/// Its two planted routines in documents, proposed by the category and the app, each 3 times or more.
const IN_A_DOCUMENT: [SeedCategory; 2] = [
    SeedCategory::LeaseRenewalOrChange,
    SeedCategory::SpreadsheetFill,
];

/// The sites of the tasks that are no routine: opened with the task not done (3, 3 and 4 times), no seed task
/// (6 times), rent paid twice, a bill paid once, and the work between the run marks.
const NO_ROUTINE: [&str; 7] = [
    "drysdale-leasing-desk.test",
    "hailcomet.test",
    "cinder-markets.test",
    "mossbank-weather.test",
    "cobblestone-rentals.test",
    "brightkiln-gas.test",
    "oakhollow-leasing.test",
];

struct Stored(Vec<Summary>);

impl SummaryRead for Stored {
    fn for_each(&self, visit: &mut dyn FnMut(&Summary)) {
        for s in &self.0 {
            visit(s);
        }
    }
}

/// The summary of an episode as halvo-derived builds it at commit. A title-derived token would be hashed there
/// under the store's key; this stand-in writes its bytes in hex instead, and the persona's episodes hold none.
fn summary_of(episode: &Episode) -> Summary {
    let site = episode.site.as_ref().map(|site| match site {
        ObservedSite::AddressField { domain } => SiteValue::Domain {
            key: SiteDomain::from(domain.clone()),
            source: DomainSource::AddressField,
        },
        ObservedSite::Title { token } => {
            let hex: String = token
                .as_str()
                .bytes()
                .chain(std::iter::repeat(0))
                .take(32)
                .map(|b| format!("{b:02x}"))
                .collect();
            SiteValue::Title {
                key: SiteHash::parse(&hex).unwrap(),
            }
        }
    });
    Summary {
        id: episode.id.clone(),
        category: episode.category,
        site,
        app: episode.app.clone(),
        day: episode.day,
        part: episode.part,
        minutes: episode.minutes,
        keys: episode.keys,
        pointer: episode.pointer,
    }
}

fn site_of(episode: &Episode) -> Option<&str> {
    match &episode.site {
        Some(ObservedSite::AddressField { domain }) => Some(domain.as_str()),
        _ => None,
    }
}

#[test]
fn persona_s001() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/r2/scripts/seen");
    let paths: Vec<PathBuf> = ["s001.jsonl", "s001.marked.jsonl"]
        .iter()
        .map(|name| dir.join(name))
        .collect();
    let script = Script::read(&paths).expect("the persona's script reads");
    let days = render_stream(&script, "s001", 20261113);
    assert!(!days.is_empty());

    let mut n = 0u128;
    let mut ids = || {
        n += 1;
        SummaryId::from_random(n.to_be_bytes())
    };
    let mut episodes: Vec<Episode> = Vec::new();
    for (day, bytes) in &days {
        let seg = u64::from(*day);
        let entries: Vec<Entry> = bytes
            .split_inclusive(|b| *b == b'\n')
            .enumerate()
            .map(|(idx, line)| Entry {
                pos: Position {
                    seg,
                    idx: idx as u64,
                },
                record: CaptureRecord::read_line(line).expect("a rendered line reads"),
            })
            .collect();
        let first = entries[0].record.at().0;
        let expiry = [Expiry {
            seg,
            at: Instant(first + 93 * DAY),
        }];
        let call = Call {
            entries: &entries,
            expiry: &expiry,
            utc_offset_s: 0,
            carry: None,
        };
        let outcome = episode_step(&call, &StageA, &mut ids);
        assert!(!outcome.run_open, "day {day} ends inside a run");
        episodes.extend(outcome.episodes);
    }

    assert!(
        episodes
            .iter()
            .all(|e| site_of(e) != Some("oakhollow-leasing.test")),
        "an episode between the run marks of day 75 was kept"
    );
    let titles: Vec<&str> = episodes
        .iter()
        .flat_map(|e| &e.steps)
        .filter_map(|s| s.title.as_ref().map(|t| t.value.as_str()))
        .collect();
    assert!(titles.iter().any(|t| t.contains("[card]")));
    assert!(titles.iter().all(|t| !t.contains("4242")));

    let mut summaries: Vec<Summary> = episodes.iter().map(summary_of).collect();
    summaries.sort_by(|a, b| (a.day, &a.id).cmp(&(b.day, &b.id)));
    let proposals = Proposer.propose(&Stored(summaries));
    let mut on_a_site: Vec<(SeedCategory, &str, u32)> = Vec::new();
    let mut in_a_document: Vec<SeedCategory> = Vec::new();
    for p in &proposals {
        match &p.site {
            Some(SiteValue::Domain {
                key,
                source: DomainSource::AddressField,
            }) => on_a_site.push((p.category, key.as_str(), p.recurrence.count)),
            None => {
                assert_eq!(p.app.as_str(), OFFICE, "proposal: {p:?}");
                assert!(p.recurrence.count >= 3, "proposal: {p:?}");
                in_a_document.push(p.category);
            }
            other => panic!("a proposal with the site {other:?}"),
        }
    }
    for site in NO_ROUTINE {
        assert!(
            on_a_site.iter().all(|(_, proposed, _)| *proposed != site),
            "a task at {site} that is no routine was proposed: {on_a_site:?}"
        );
    }
    on_a_site.sort_unstable();
    in_a_document.sort_unstable();
    assert_eq!(on_a_site, ON_A_SITE);
    assert_eq!(in_a_document, IN_A_DOCUMENT);
}

/// The seed of the build draws with which persona_s001 renders its stream, and the tool every stream.
const SEED: u64 = 20261113;

/// The least denominator of a printed share: under it a row prints its counts and `thin` (plan.md of the workflow
/// run, line 156: the per-category line that 20 routines can carry).
const THIN: u32 = 20;

/// A labelled episode of one stream, as its label row gives it: its task, whether that task is a planted routine,
/// and its category.
struct Labelled {
    task: String,
    planted: bool,
    category: Category,
}

/// An episode of the identifier as the count reads it: for each record that it names among its segments (its own
/// records and the title records of their windows), the place among the stream's labelled episodes of the scripted
/// episode that the record came from, or none for a record of no scripted episode.
struct Found {
    records: Vec<Option<usize>>,
}

/// A proposal as the count reads it: its category and, for each of its summaries, the place of that summary's
/// episode among the stream's `Found`.
struct Proposed {
    category: SeedCategory,
    support: Vec<usize>,
}

/// The counts of one kind of task.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tally {
    planted: u32,
    proposed: u32,
    paired: u32,
}

/// The counts of each kind of task.
type Tallies = BTreeMap<SeedCategory, Tally>;

/// How many episodes of the identifier there are, how many hold the records of two scripted episodes or more, and
/// how many hold the records of none.
#[derive(Clone, Copy, Default)]
struct Held {
    episodes: u32,
    mixed: u32,
    none: u32,
}

impl Held {
    fn add(&mut self, found: &Found) {
        let scripted: BTreeSet<usize> = found.records.iter().flatten().copied().collect();
        self.episodes += 1;
        self.mixed += u32::from(scripted.len() > 1);
        self.none += u32::from(scripted.is_empty());
    }
}

/// What one form of the tool counts over a set: the tallies of each kind, and how the episodes of the identifier,
/// all of them and those that the proposals hold, hold the records of scripted episodes.
#[derive(Default)]
struct Form {
    tallies: Tallies,
    all: Held,
    in_proposals: Held,
}

/// The labelled episode that an episode of the identifier is: the scripted episode that most of its records came
/// from, the first in the order of the label rows of equals; none where no record of it came from a scripted episode.
fn labelled_of(found: &Found) -> Option<usize> {
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    for &l in found.records.iter().flatten() {
        *counts.entry(l).or_default() += 1;
    }
    let most = counts.values().copied().max()?;
    counts.into_iter().find(|&(_, n)| n == most).map(|(l, _)| l)
}

/// Adds one stream to the tallies: each planted routine to its kind's `planted`, each proposal to its category's
/// `proposed`, and each pair to its kind's `paired`. A proposal and a planted routine pair when the category agrees
/// and the labelled episodes that the proposal's episodes are, with one more for each of its episodes that is none,
/// meet the routine's episodes in at least half of their union (plan.md of the workflow run, line 28, D12). The
/// pairs are one to one, taken by the highest share first, then by the routine's task, then by the proposal's place.
fn count_stream(
    labelled: &[Labelled],
    found: &[Found],
    proposed: &[Proposed],
    tallies: &mut Tallies,
) {
    let mut routines: BTreeMap<&str, (SeedCategory, Vec<usize>)> = BTreeMap::new();
    for (i, l) in labelled.iter().enumerate() {
        if let (true, Some(kind)) = (l.planted, l.category.seed()) {
            routines
                .entry(l.task.as_str())
                .or_insert((kind, Vec::new()))
                .1
                .push(i);
        }
    }
    for (kind, _) in routines.values() {
        tallies.entry(*kind).or_default().planted += 1;
    }
    let mut candidates: Vec<(usize, usize, usize, usize)> = Vec::new();
    for (p, proposal) in proposed.iter().enumerate() {
        tallies.entry(proposal.category).or_default().proposed += 1;
        let mut support = BTreeSet::new();
        let mut none = 0;
        for &f in &proposal.support {
            match labelled_of(&found[f]) {
                Some(l) => {
                    support.insert(l);
                }
                None => none += 1,
            }
        }
        for (r, (kind, episodes)) in routines.values().enumerate() {
            if *kind != proposal.category {
                continue;
            }
            let meet = episodes.iter().filter(|e| support.contains(e)).count();
            let union = support.len() + none + episodes.len() - meet;
            if meet > 0 && 2 * meet >= union {
                candidates.push((p, r, meet, union));
            }
        }
    }
    candidates.sort_by(|a, b| {
        (b.2 * a.3)
            .cmp(&(a.2 * b.3))
            .then(a.1.cmp(&b.1))
            .then(a.0.cmp(&b.0))
    });
    let mut proposal_taken = vec![false; proposed.len()];
    let mut routine_taken = vec![false; routines.len()];
    for (p, r, _, _) in candidates {
        if !proposal_taken[p] && !routine_taken[r] {
            proposal_taken[p] = true;
            routine_taken[r] = true;
            tallies.entry(proposed[p].category).or_default().paired += 1;
        }
    }
}

/// The record with no site in its title, as a capture source records a title at main: none reads a site yet.
fn without_site(record: CaptureRecord) -> CaptureRecord {
    let Payload::WindowTitle(title) = record.payload() else {
        return record;
    };
    let title = Title {
        site: None,
        ..title.clone()
    };
    CaptureRecord::new(
        record.id().clone(),
        record.at(),
        record.seq(),
        record.app().cloned(),
        Payload::WindowTitle(title),
    )
    .expect("a title record without its site")
}

/// The place among the labelled episodes of the scripted episode whose range holds the instant `at`, or none:
/// `ranges` holds the start, the end and the episode's place of each range of a scripted episode, by start, and no
/// two of them overlap.
fn scripted_at(ranges: &[(i64, i64, usize)], at: i64) -> Option<usize> {
    let after = ranges.partition_point(|&(start, _, _)| start <= at);
    let &(_, end, l) = ranges.get(after.checked_sub(1)?)?;
    (at < end).then_some(l)
}

/// The episodes of the identifier over the rendered days of one stream: one call of the episode step with stage A
/// for each scripted day, as persona_s001 makes it. With `sites` false every title record loses its site first.
/// Each record goes in at a segment of its own, its place in the stream from 1, so that an episode's segments name
/// the records that it holds; every segment of a day expires at the same instant, so the episodes are those of
/// persona_s001's call. The second value gives, for each segment, the scripted episode that its record came from.
fn episodes_of(
    days: &BTreeMap<u32, Vec<u8>>,
    sites: bool,
    ranges: &[(i64, i64, usize)],
) -> (Vec<Episode>, Vec<Option<usize>>) {
    let mut n = 0u128;
    let mut ids = || {
        n += 1;
        SummaryId::from_random(n.to_be_bytes())
    };
    let mut episodes = Vec::new();
    let mut origin: Vec<Option<usize>> = Vec::new();
    for bytes in days.values() {
        let entries: Vec<Entry> = bytes
            .split_inclusive(|b| *b == b'\n')
            .enumerate()
            .map(|(idx, line)| {
                let record = CaptureRecord::read_line(line).expect("a rendered line reads");
                origin.push(scripted_at(ranges, record.at().0));
                Entry {
                    pos: Position {
                        seg: origin.len() as u64,
                        idx: idx as u64,
                    },
                    record: if sites { record } else { without_site(record) },
                }
            })
            .collect();
        let at = Instant(entries[0].record.at().0 + 93 * DAY);
        let expiry: Vec<Expiry> = entries
            .iter()
            .map(|entry| Expiry {
                seg: entry.pos.seg,
                at,
            })
            .collect();
        let call = Call {
            entries: &entries,
            expiry: &expiry,
            utc_offset_s: 0,
            carry: None,
        };
        episodes.extend(episode_step(&call, &StageA, &mut ids).episodes);
    }
    (episodes, origin)
}

/// What the identifier finds in one stream and what the recurrence step proposes over it.
fn found_in(
    days: &BTreeMap<u32, Vec<u8>>,
    sites: bool,
    ranges: &[(i64, i64, usize)],
) -> (Vec<Found>, Vec<Proposed>) {
    let (episodes, origin) = episodes_of(days, sites, ranges);
    let place: BTreeMap<SummaryId, usize> = episodes
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.clone(), i))
        .collect();
    let mut summaries: Vec<Summary> = episodes.iter().map(summary_of).collect();
    summaries.sort_by(|a, b| (a.day, &a.id).cmp(&(b.day, &b.id)));
    let proposed = Proposer
        .propose(&Stored(summaries))
        .iter()
        .map(|p| Proposed {
            category: p.category,
            support: p.summaries.iter().map(|id| place[id]).collect(),
        })
        .collect();
    let found = episodes
        .iter()
        .map(|e| Found {
            records: e
                .segments
                .iter()
                .map(|seg| origin[seg.get() as usize - 1])
                .collect(),
        })
        .collect();
    (found, proposed)
}

/// The labelled episodes of one stream, in the order of its label rows as halvo-sample's `rows_of` gives them, and
/// the ranges of those episodes by start, each with its episode's place.
fn labelled_in(script: &Script, stream: &str) -> (Vec<Labelled>, Vec<(i64, i64, usize)>) {
    let mut labelled = Vec::new();
    let mut ranges = Vec::new();
    for row in rows_of(script, stream) {
        match row {
            Row::Episode {
                task,
                category,
                kind,
                ..
            } => labelled.push(Labelled {
                task,
                planted: kind == TaskKind::Planted,
                category,
            }),
            Row::Range { start, end, .. } => ranges.push((start, end, labelled.len() - 1)),
        }
    }
    ranges.sort_unstable();
    (labelled, ranges)
}

/// What the two forms count over one set of scripts, the first with every site taken out and the second with the
/// scripts' sites, over every stream that its files hold, and the number of those streams.
fn tally_set(dir: &Path) -> (usize, [Form; 2]) {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("the set's directory reads")
        .map(|entry| {
            let name = entry.expect("an entry of the set").file_name();
            name.into_string().expect("a name of UTF-8")
        })
        .filter(|name| name.ends_with(".jsonl") && !name.ends_with(".marked.jsonl"))
        .collect();
    names.sort();
    let mut streams = 0;
    let mut forms = [Form::default(), Form::default()];
    for name in names {
        let marked = name.replace(".jsonl", ".marked.jsonl");
        let script = Script::read(&[dir.join(&name), dir.join(marked)]).expect("a script reads");
        for stream in script.streams.keys() {
            streams += 1;
            let days = render_stream(&script, stream, SEED);
            let (labelled, ranges) = labelled_in(&script, stream);
            for (sites, form) in [false, true].into_iter().zip(forms.iter_mut()) {
                let (found, proposed) = found_in(&days, sites, &ranges);
                for f in &found {
                    form.all.add(f);
                }
                for p in &proposed {
                    for &f in &p.support {
                        form.in_proposals.add(&found[f]);
                    }
                }
                count_stream(&labelled, &found, &proposed, &mut form.tallies);
            }
        }
    }
    (streams, forms)
}

/// The share `n/d = s` to two decimals, or the counts and `thin` where `d` is under `THIN`.
fn share(n: u32, d: u32) -> String {
    if d < THIN {
        format!("{n}/{d} thin")
    } else {
        format!("{n}/{d} = {:.2}", f64::from(n) / f64::from(d))
    }
}

/// The rows of one set: a line with its name and the number of its streams, a heading, one row for each kind in
/// the order of `SeedCategory::ALL`, and a line on how the episodes of the identifier hold the records of scripted
/// episodes.
fn rows(set: &str, streams: usize, form: &Form) -> String {
    let mut out = format!("set {set}: {streams} streams\n");
    out.push_str(&format!(
        "{:<26} {:>7} {:>8} {:>6}  {:<21} {}\n",
        "kind", "planted", "proposed", "paired", "precision (plan 0.90)", "recall (plan 0.70)"
    ));
    for kind in SeedCategory::ALL {
        let t = form.tallies.get(kind).copied().unwrap_or_default();
        out.push_str(&format!(
            "{:<26} {:>7} {:>8} {:>6}  {:<21} {}\n",
            kind.as_str(),
            t.planted,
            t.proposed,
            t.paired,
            share(t.paired, t.proposed),
            share(t.paired, t.planted)
        ));
    }
    let (a, p) = (form.all, form.in_proposals);
    out.push_str(&format!(
        "episodes of the identifier {}: with records of two scripted episodes or more {}, of none {}; \
         in proposals {}: {} and {}\n",
        a.episodes, a.mixed, a.none, p.episodes, p.mixed, p.none
    ));
    out
}

/// The counting of the tool on a stream made by hand, with two kinds. The email routine has three labelled episodes,
/// and in the order of the label rows an episode of a task that is no routine stands before its third. The email
/// proposal holds an episode of the identifier whose records came from the first, one whose records came from the
/// third, and one with no record of a scripted episode, so it meets the routine in 2 of a union of 4, the plan's half,
/// and pairs: the second counts as the third episode, whose records it holds, and not as the episode before it in the
/// order of the label rows. Nothing finds the shop routine, and the shop proposal holds two episodes of a task that
/// is no planted routine, so it fits nothing. By hand: email planted 1, proposed 1, paired 1, precision 1/1 and
/// recall 1/1; shop_order planted 1, proposed 1, paired 0, precision 0/1 and recall 0/1; each share is thin.
#[test]
fn counting_a_tiny_case() {
    let l = |task: &str, planted: bool, category: Category| Labelled {
        task: task.to_owned(),
        planted,
        category,
    };
    let labelled = [
        l("t1", true, Category::Email),
        l("t1", true, Category::Email),
        l("t4", false, Category::Email),
        l("t1", true, Category::Email),
        l("t2", true, Category::ShopOrder),
        l("t2", true, Category::ShopOrder),
        l("t2", true, Category::ShopOrder),
        l("t3", false, Category::ShopOrder),
        l("t3", false, Category::ShopOrder),
    ];
    let f = |records: &[usize]| Found {
        records: records.iter().map(|&r| Some(r)).collect(),
    };
    let found = [f(&[0, 0]), f(&[3, 3]), f(&[]), f(&[7]), f(&[8])];
    let proposed = [
        Proposed {
            category: SeedCategory::Email,
            support: vec![0, 1, 2],
        },
        Proposed {
            category: SeedCategory::ShopOrder,
            support: vec![3, 4],
        },
    ];
    let mut tallies = BTreeMap::new();
    count_stream(&labelled, &found, &proposed, &mut tallies);
    let t = |planted, proposed, paired| Tally {
        planted,
        proposed,
        paired,
    };
    assert_eq!(
        tallies,
        BTreeMap::from([
            (SeedCategory::Email, t(1, 1, 1)),
            (SeedCategory::ShopOrder, t(1, 1, 0)),
        ])
    );
    assert_eq!(share(1, 1), "1/1 thin");
    assert_eq!(share(14, 20), "14/20 = 0.70");
}

/// The counting tool. It runs by hand and in no CI job, by the one command
/// `cargo test -p halvo-identify --test persona --locked -- --ignored --exact hit_rates --nocapture`, and prints two
/// tables, each over the sets seen and unseen: the first with every site taken out of the title records before the
/// episode step, the second with the scripts' sites. In both an episode of the identifier is the scripted episode
/// whose records it holds. It prints counts and shares of each kind alone: no title, no site and no stream.
#[test]
#[ignore = "a counting tool that runs by hand alone, by --ignored --exact hit_rates --nocapture"]
fn hit_rates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/r2/scripts");
    let sets: Vec<(&str, usize, [Form; 2])> = ["seen", "unseen"]
        .into_iter()
        .map(|set| {
            let (streams, forms) = tally_set(&root.join(set));
            (set, streams, forms)
        })
        .collect();
    let heads = [
        "TABLE 1, the product as it runs today: every site taken out of every title record before the episode step",
        "TABLE 2, what a site reader would add: the scripts' sites kept",
    ];
    for (form, head) in heads.into_iter().enumerate() {
        println!("{head}");
        for (set, streams, forms) in &sets {
            print!("{}", rows(set, *streams, &forms[form]));
        }
        println!();
    }
}
