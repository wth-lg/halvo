mod common;

use std::path::Path;

use common::*;
use halvo_coord_store::{CoordHandler, FAMILY, OPS};
use halvo_derived::{
    Clock as StoreClock, DbKey, Error, Handler, HandlerCx, Host as StoreHost, IdSource, MoveTo,
    Store, Takes,
};
use halvo_schema::kinds::registry;
use halvo_schema::{
    AgentId, AppId, Confidence, Episode, ErrorName, FloorLayer, Instant, InterfaceDecision,
    InterfaceRole, KindEntry, ObservedStep, Ordinal, Part, Position, Proposal, RawRequest,
    Recurrence, RecurrenceStep, RequestId, SeedCategory, StoreId, Summary, SummaryId, SummaryRead,
    Tagged, Text, Trigger, WorkflowId,
};
use serde_json::value::RawValue;

struct Noon;

impl StoreClock for Noon {
    fn now(&self) -> Instant {
        Instant(noon(TODAY))
    }

    fn day(&self, at: Instant) -> u32 {
        u32::try_from(at.0.div_euclid(DAY_S * NS)).unwrap_or(0)
    }

    fn day_start(&self, day: u32) -> Instant {
        Instant(i64::from(day) * DAY_S * NS)
    }
}

struct ProposesOne;

impl RecurrenceStep for ProposesOne {
    fn propose(&self, summaries: &dyn SummaryRead) -> Vec<Proposal> {
        let mut all: Vec<Summary> = Vec::new();
        summaries.for_each(&mut |s| all.push(s.clone()));
        let (Some(first), Some(last)) = (all.first(), all.last()) else {
            return Vec::new();
        };
        vec![Proposal {
            category: first.category,
            site: first.site.clone(),
            app: first.app.clone(),
            trigger: Trigger::Schedule,
            recurrence: Recurrence {
                count: u32::try_from(all.len()).unwrap_or(u32::MAX),
                period: None,
                first: first.day,
                last: last.day,
            },
            confidence: Confidence(80),
            summaries: all.iter().map(|s| s.id.clone()).collect(),
        }]
    }
}

struct GivesAgentOne(WorkflowId);

impl Handler for GivesAgentOne {
    fn handle(&self, _: &RawRequest, cx: &mut HandlerCx<'_>) -> Result<Box<RawValue>, ErrorName> {
        cx.state(
            &self.0,
            MoveTo::Approved,
            Some(AgentId::parse(&agent(1)).unwrap()),
        )
        .map_err(|_| ErrorName::State)?;
        RawValue::from_string("null".to_owned()).map_err(|_| ErrorName::Internal)
    }
}

struct Ones;

impl IdSource for Ones {
    fn fill(&mut self, bytes: &mut [u8]) {
        bytes.fill(1);
    }
}

fn one_episode() -> Episode {
    let start = noon(TODAY) - 3_600 * NS;
    Episode {
        id: SummaryId::parse(&format!("{:032x}", 1)).unwrap(),
        category: SeedCategory::UtilityBillPayment,
        site: None,
        app: AppId::parse("com.example.browser").unwrap(),
        day: TODAY,
        part: Part::Morning,
        minutes: 5,
        keys: None,
        pointer: None,
        segments: vec![Ordinal::new(1).unwrap()],
        expires: Instant(9_000_000_000_000_000_000),
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

fn adapter_row(workflow: &str, seq: u32) -> String {
    format!(
        r#"{{"v":1,"workflow":"{workflow}","agent":"{}","seq":{seq},"day":{TODAY},"at":{},"event":"hold_set","run":null,"slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"person","trigger":null,"origin":null,"defer_launch":null,"defer_ticks_failed":null}}"#,
        agent(1),
        noon(TODAY)
    )
}

/// The host of the derived store: the coordinator's handler, and a handler that gives `workflow` agent 1.
fn host(workflow: &WorkflowId) -> StoreHost {
    StoreHost::new(Box::new(Noon), Box::new(ProposesOne))
        .handler(
            FAMILY,
            Takes::Coordinator(&OPS),
            Box::new(CoordHandler::new(Box::new(Noon))),
        )
        .handler(
            "coordtest",
            Takes::Coordinator(&["test_agent"]),
            Box::new(GivesAgentOne(workflow.clone())),
        )
}

/// The names of the entries under `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the data directory")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .into_string()
                .expect("a name")
        })
        .collect();
    names.sort();
    names
}

/// After delete-all no row of this run stands (contracts.md I12.4, the case `coord_delete_all`). The rows that
/// `coord_log` stores lie under the entry `derived/` of the data directory and nowhere else, so the removal of that
/// entry with every file under it, the step that the product's delete-all makes for the row `derived` of the
/// registry, leaves no entry under the data directory, and a store opened there anew with the same key holds no row
/// of the workflow.
#[test]
fn coord_delete_all() {
    let data_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("halvo-coord-store-delete-all");
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir).expect("an empty data directory");
    let key = DbKey::new([3; 32]);
    let entries: Vec<&'static KindEntry> = registry().collect();
    let workflow = WorkflowId::from_random([1; 16]);
    let mut store =
        Store::open(&data_dir, &key, &entries, host(&workflow), &mut Ones).expect("a store");
    store
        .commit_batch(
            &StoreId::parse(&format!("{:032x}", 1)).unwrap(),
            &[one_episode()],
            Position { seg: 1, idx: 0 },
            &mut Ones,
        )
        .expect("the batch");
    store
        .decide(&InterfaceDecision::approve(
            &InterfaceRole::claim(),
            RequestId::from_random([9; 16]),
            workflow.clone(),
        ))
        .expect("the workflow approved");
    store
        .handle(&request("coordinator_write", r#"{"op":"test_agent"}"#))
        .expect("the workflow's agent set");

    let w = workflow.as_str();
    let rows: Vec<String> = (1..=3).map(|seq| adapter_row(w, seq)).collect();
    let log = format!(
        r#"{{"op":"coord_log","workflow":"{w}","agent":"{}","records":[{}]}}"#,
        agent(1),
        rows.join(",")
    );
    let stored = store
        .handle(&request("coordinator_write", &log))
        .expect("the log");
    assert_eq!(stored.get(), r#"{"stored":3}"#);
    let history = request(
        "coordinator_write",
        &format!(
            r#"{{"op":"coord_history","workflow":"{w}","agent":"{}","after":null}}"#,
            agent(1)
        ),
    );
    let canonical_rows: Vec<String> = rows.iter().map(String::as_str).map(canonical).collect();
    assert_eq!(
        store.handle(&history).map(|reply| reply.get().to_owned()),
        Ok(format!(
            r#"{{"records":[{}],"last":3}}"#,
            canonical_rows.join(",")
        )),
        "the rows before the delete"
    );
    assert_eq!(
        names(&data_dir),
        ["derived"],
        "the entries before the delete"
    );

    drop(store);
    std::fs::remove_dir_all(data_dir.join("derived")).expect("the entry derived removed");
    assert_eq!(
        names(&data_dir),
        Vec::<String>::new(),
        "the entries after the delete"
    );

    let mut store = Store::open(&data_dir, &key, &entries, host(&workflow), &mut Ones)
        .expect("a store opened anew");
    assert_eq!(
        store.handle(&history).map(|reply| reply.get().to_owned()),
        Err(Error::Refused(ErrorName::NoWorkflow)),
        "the history after the delete"
    );
}
