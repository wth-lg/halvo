//! One development persona of the generated sample, fed through the episode step one scripted day per call and
//! then through the recurrence step: its repeated task is proposed, the work between its run marks is not, and
//! no step holds the card number of its card page.

use std::path::{Path, PathBuf};

use halvo_identify::{episode_step, Call, Proposer, Stage};
use halvo_sample::build::render_stream;
use halvo_sample::script::Script;
use halvo_schema::{
    Category, DomainSource, Episode, Instant, ObservedSite, ObservedStep, Position, RecurrenceStep,
    SeedCategory, SiteDomain, SiteHash, SiteValue, Summary, SummaryId, SummaryRead,
};
use halvo_seam::message::{Entry, Expiry};
use halvo_seam::record::CaptureRecord;

const DAY: i64 = 86_400 * 1_000_000_000;

/// The persona's stage: a category by the site of the episode's first step that holds one.
struct BySite;

impl Stage for BySite {
    fn classify(&self, steps: &[ObservedStep]) -> Category {
        match steps
            .iter()
            .find_map(|s| s.site.as_ref())
            .map(|s| s.value.as_str())
        {
            Some("pinecart.test") => Category::ShopOrder,
            Some("cobblestone-rentals.test") => Category::TenantRent,
            Some("oakhollow-leasing.test") => Category::LeaseRenewalOrChange,
            _ => Category::Other,
        }
    }
}

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
        };
        let outcome = episode_step(&call, &BySite, &mut ids);
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
    assert!(
        proposals
            .iter()
            .all(|p| p.category != SeedCategory::TenantRent),
        "the rent task seen twice was proposed: {proposals:?}"
    );
    assert_eq!(proposals.len(), 1, "proposals: {proposals:?}");
    let p = &proposals[0];
    assert_eq!(p.category, SeedCategory::ShopOrder);
    assert_eq!(
        p.site,
        Some(SiteValue::Domain {
            key: SiteDomain::parse("pinecart.test").unwrap(),
            source: DomainSource::AddressField,
        })
    );
    assert_eq!(p.recurrence.count, 6);
}
