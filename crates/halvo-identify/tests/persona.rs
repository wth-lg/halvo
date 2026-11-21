//! One development persona of the generated sample, fed through the episode step with stage A, one scripted day
//! per call, and then through the recurrence step: each of its planted routines is proposed and no other task,
//! the work between its run marks is not, and no step holds the card number of its card page.

use std::path::{Path, PathBuf};

use halvo_identify::{episode_step, Call, Proposer, StageA};
use halvo_sample::build::render_stream;
use halvo_sample::script::Script;
use halvo_schema::{
    DomainSource, Episode, Instant, ObservedSite, Position, RecurrenceStep, SeedCategory,
    SiteDomain, SiteHash, SiteValue, Summary, SummaryId, SummaryRead,
};
use halvo_seam::message::{Entry, Expiry};
use halvo_seam::record::CaptureRecord;

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
