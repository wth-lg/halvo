#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use halvo_derived::{Clock, DbKey, Host, IdSource, OpenError, Store};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{Position, StoreId, SummaryId};
use halvo_schema_core::kinds::{self, FieldType, KindEntry, Origin, RetentionClass};
use halvo_schema_core::record::{
    Confidence, Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Proposal, Recurrence,
    RecurrenceStep, Summary, SummaryRead, Tagged, Text, Trigger,
};
use halvo_schema_core::request::{self, Decoded, RawRequest};
use halvo_schema_core::site::{
    AppId, Domain, DomainSource, ObservedSite, SiteDomain, SiteToken, SiteValue,
};
use halvo_sqlcipher::rusqlite::Connection;

pub const DAY_NS: i64 = 86_400_000_000_000;

pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("derived-{name}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn db(&self) -> PathBuf {
        self.path.join("derived").join("derived.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Clone)]
pub struct TestClock(Rc<Cell<i64>>);

impl TestClock {
    pub fn at_day(day: u32) -> TestClock {
        TestClock(Rc::new(Cell::new(
            i64::from(day) * DAY_NS + 12 * 3_600_000_000_000,
        )))
    }

    pub fn set_day(&self, day: u32) {
        self.0.set(i64::from(day) * DAY_NS + 12 * 3_600_000_000_000);
    }

    pub fn set(&self, now: i64) {
        self.0.set(now);
    }

    pub fn now(&self) -> i64 {
        self.0.get()
    }
}

impl Clock for TestClock {
    fn now(&self) -> Instant {
        Instant(self.0.get())
    }

    fn day(&self, at: Instant) -> u32 {
        u32::try_from(at.0.div_euclid(DAY_NS)).unwrap_or(0)
    }

    fn day_start(&self, day: u32) -> Instant {
        Instant(i64::from(day) * DAY_NS)
    }
}

pub struct Seq(pub u64);

impl IdSource for Seq {
    fn fill(&mut self, bytes: &mut [u8]) {
        for chunk in bytes.chunks_mut(8) {
            self.0 += 1;
            let word = self.0.to_be_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }
}

pub fn key() -> DbKey {
    DbKey::new([7; 32])
}

pub fn registry() -> Vec<&'static KindEntry> {
    kinds::registry().collect()
}

/// The distinct families of `entries`, in the order of their first entry: the families to which an open with these
/// entries gives a revision, in the report's order.
pub fn families(entries: &[&'static KindEntry]) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for entry in entries {
        if !found.contains(&entry.family) {
            found.push(entry.family);
        }
    }
    found
}

pub fn open(scratch: &Scratch, clock: &TestClock) -> Result<Store, OpenError> {
    open_with(scratch, clock, &registry())
}

pub fn open_with(
    scratch: &Scratch,
    clock: &TestClock,
    entries: &[&'static KindEntry],
) -> Result<Store, OpenError> {
    Store::open(
        scratch.path(),
        &key(),
        entries,
        Host::new(Box::new(clock.clone()), Box::new(NoProposals)),
        &mut Seq(1 << 40),
    )
}

pub fn open_host(scratch: &Scratch, host: Host) -> Result<Store, OpenError> {
    Store::open(scratch.path(), &key(), &registry(), host, &mut Seq(1 << 40))
}

/// A recurrence step that proposes nothing.
pub struct NoProposals;

impl RecurrenceStep for NoProposals {
    fn propose(&self, _summaries: &dyn SummaryRead) -> Vec<Proposal> {
        Vec::new()
    }
}

/// The test proposer: one proposal for each key whose summaries inside their bound number `min` or more, with
/// those summaries as its support, its days as its recurrence and a confidence of 80.
pub struct Grouping {
    pub min: usize,
}

impl RecurrenceStep for Grouping {
    fn propose(&self, summaries: &dyn SummaryRead) -> Vec<Proposal> {
        group_by_key(summaries, self.min, &[])
    }
}

/// The test proposer that leaves out the summaries whose ids the test put in its list.
pub struct GroupingExcept {
    pub min: usize,
    pub except: Rc<RefCell<Vec<SummaryId>>>,
}

impl RecurrenceStep for GroupingExcept {
    fn propose(&self, summaries: &dyn SummaryRead) -> Vec<Proposal> {
        group_by_key(summaries, self.min, &self.except.borrow())
    }
}

/// A recurrence step that proposes, for each domain seen in the address field on `min` or more summaries of one
/// category, one workflow under that domain with the source `sign_in`: a proposal whose key its summaries do not
/// hold.
pub struct SignInGroup {
    pub min: usize,
}

impl RecurrenceStep for SignInGroup {
    fn propose(&self, summaries: &dyn SummaryRead) -> Vec<Proposal> {
        let mut groups: BTreeMap<String, Vec<Summary>> = BTreeMap::new();
        summaries.for_each(&mut |s| {
            if let Some(SiteValue::Domain {
                key,
                source: DomainSource::AddressField,
            }) = &s.site
            {
                groups
                    .entry(format!("{} {}", s.category, key.as_str()))
                    .or_default()
                    .push(s.clone());
            }
        });
        groups
            .into_values()
            .filter(|g| g.len() >= self.min)
            .map(|g| {
                let key = match &g[0].site {
                    Some(SiteValue::Domain { key, .. }) => key.clone(),
                    _ => SiteDomain::parse("unreachable.test").unwrap(),
                };
                Proposal {
                    category: g[0].category,
                    site: Some(SiteValue::Domain {
                        key,
                        source: DomainSource::SignIn,
                    }),
                    app: g[0].app.clone(),
                    trigger: Trigger::Schedule,
                    recurrence: Recurrence {
                        count: g.len() as u32,
                        period: None,
                        first: g[0].day,
                        last: g[g.len() - 1].day,
                    },
                    confidence: Confidence(80),
                    summaries: g.iter().map(|s| s.id.clone()).collect(),
                }
            })
            .collect()
    }
}

fn group_by_key(summaries: &dyn SummaryRead, min: usize, except: &[SummaryId]) -> Vec<Proposal> {
    let mut groups: BTreeMap<String, Vec<Summary>> = BTreeMap::new();
    summaries.for_each(&mut |s| {
        if except.contains(&s.id) {
            return;
        }
        let key = match &s.site {
            Some(site) => format!("{} {}", s.category, serde_json::to_string(site).unwrap()),
            None => format!("{} app {}", s.category, s.app),
        };
        groups.entry(key).or_default().push(s.clone());
    });
    groups
        .into_values()
        .filter(|group| group.len() >= min)
        .map(|group| Proposal {
            category: group[0].category,
            site: group[0].site.clone(),
            app: group[group.len() - 1].app.clone(),
            trigger: Trigger::Schedule,
            recurrence: Recurrence {
                count: group.len() as u32,
                period: None,
                first: group[0].day,
                last: group[group.len() - 1].day,
            },
            confidence: Confidence(80),
            summaries: group.iter().map(|s| s.id.clone()).collect(),
        })
        .collect()
}

/// A recurrence step that returns the proposals that the test put in its list.
pub struct Fixed(pub Rc<RefCell<Vec<Proposal>>>);

impl RecurrenceStep for Fixed {
    fn propose(&self, _summaries: &dyn SummaryRead) -> Vec<Proposal> {
        self.0.borrow().clone()
    }
}

/// A recurrence step that records the day and the id of each summary that it reads, in the order read.
pub struct Recorder(pub Rc<RefCell<Vec<(u32, String)>>>);

impl RecurrenceStep for Recorder {
    fn propose(&self, summaries: &dyn SummaryRead) -> Vec<Proposal> {
        summaries.for_each(&mut |s| self.0.borrow_mut().push((s.day, s.id.as_str().to_owned())));
        Vec::new()
    }
}

pub fn open_with_ids(scratch: &Scratch, clock: &TestClock, seed: u64) -> Result<Store, OpenError> {
    Store::open(
        scratch.path(),
        &key(),
        &registry(),
        Host::new(Box::new(clock.clone()), Box::new(NoProposals)),
        &mut Seq(seed),
    )
}

pub fn raw(scratch: &Scratch) -> Connection {
    let conn = Connection::open(scratch.db()).unwrap();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", "07".repeat(32)))
        .unwrap();
    conn
}

pub fn bodies(scratch: &Scratch, kind: &str) -> Vec<String> {
    let conn = raw(scratch);
    let mut statement = conn
        .prepare("SELECT body FROM rows WHERE kind = ?1 ORDER BY id")
        .unwrap();
    statement
        .query_map([kind], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

pub fn count(scratch: &Scratch, sql: &str) -> i64 {
    raw(scratch).query_row(sql, [], |r| r.get(0)).unwrap()
}

pub fn store_id(n: u8) -> StoreId {
    StoreId::parse(&format!("{n:032x}")).unwrap()
}

pub fn summary_id(n: u64) -> SummaryId {
    SummaryId::parse(&format!("{n:032x}")).unwrap()
}

pub fn position(seg: u64, idx: u64) -> Position {
    Position { seg, idx }
}

pub fn domain(name: &str) -> Option<ObservedSite> {
    Some(ObservedSite::AddressField {
        domain: Domain::parse(name).unwrap(),
    })
}

pub fn token(name: &str) -> Option<ObservedSite> {
    Some(ObservedSite::Title {
        token: SiteToken::parse(name).unwrap(),
    })
}

pub const FAR: i64 = 9_000_000_000_000_000_000;

/// An episode of the seed category `utility_bill_payment` in the app `com.example.browser`, begun on `day`, with
/// one segment `n`, an expiry far ahead and one observed step.
pub fn episode(n: u64, day: u32, site: Option<ObservedSite>) -> Episode {
    let start = i64::from(day) * DAY_NS + 64_800_000_000_000 + i64::try_from(n).unwrap();
    Episode {
        id: summary_id(n),
        category: SeedCategory::UtilityBillPayment,
        site,
        app: AppId::parse("com.example.browser").unwrap(),
        day,
        part: Part::Evening,
        minutes: 5,
        keys: None,
        pointer: None,
        segments: vec![Ordinal::new(n).unwrap()],
        expires: Instant(FAR),
        start: Instant(start),
        end: Instant(start + 1_000),
        steps: vec![ObservedStep {
            app: None,
            title: Some(Tagged {
                value: Text::parse("Pay bill").unwrap(),
                layer: FloorLayer::Floor,
            }),
            input: None,
            site: None,
            text: None,
        }],
    }
}

/// A test-only kind of origin capture, bounded at 93 days, of the family `test`, with the workflow key `workflow`.
pub static NOTE: KindEntry = KindEntry {
    name: "test_note",
    class: RetentionClass::Bounded { days: 93 },
    origin: Origin::Capture,
    family: "test",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("day", FieldType::Day),
        ("n", FieldType::Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":20767,"n":1}"#,
};

/// The kind `test_note` with the workflow key left out of its entry.
pub static NOTE_UNKEYED: KindEntry = KindEntry {
    workflow_key: None,
    ..NOTE
};

/// A test-only kind of origin own, kept, of the family `test`, with a text of its own and an instant.
pub static OWN: KindEntry = KindEntry {
    name: "test_own",
    class: RetentionClass::Kept,
    origin: Origin::Own,
    family: "test",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("note", FieldType::OwnText),
        ("at", FieldType::Instant),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","note":"Call the landlord","at":1794339000000000000}"#,
};

/// A test-only window kind of the family `test`.
pub static TRACE: KindEntry = KindEntry {
    name: "test_trace",
    class: RetentionClass::Window,
    origin: Origin::Capture,
    family: "test",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("segments", FieldType::Ordinal),
        ("expires", FieldType::Instant),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","segments":[4410],"expires":1795478400000000000}"#,
};

/// A test-only kept kind of the family `front`, which a handler in front writes.
pub static FRONT: KindEntry = KindEntry {
    name: "front_mark",
    class: RetentionClass::Kept,
    origin: Origin::Capture,
    family: "front",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("seen", FieldType::Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","seen":1}"#,
};

pub fn open_entries(
    scratch: &Scratch,
    host: Host,
    extra: &[&'static KindEntry],
) -> Result<Store, OpenError> {
    let mut entries = registry();
    entries.extend_from_slice(extra);
    Store::open(scratch.path(), &key(), &entries, host, &mut Seq(1 << 40))
}

/// The request that `request::decode` returns undecoded for `{"v":1,"id":..,"request":<name>,"body":<body>}`.
pub fn raw_request(name: &str, body: &str) -> RawRequest {
    let text = format!(
        r#"{{"v":1,"id":"{}","request":"{name}","body":{body}}}"#,
        "5".repeat(32)
    );
    match request::decode(text.as_bytes()) {
        Ok(Decoded::Raw(raw)) => raw,
        other => panic!("the request decodes as {:?}", other.map(|d| d.role())),
    }
}
