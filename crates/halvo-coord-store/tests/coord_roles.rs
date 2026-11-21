mod common;

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use common::*;
use halvo_coord_store::{coordinator, CommitRefused, CoordHandler, FAMILY, OPS};
use halvo_derived::{
    Clock as StoreClock, DbKey, Error, Handler, HandlerCx, Host as StoreHost, IdSource, MoveTo,
    Store, Takes,
};
use halvo_schema::coordinator::body::{CoordHistory, CoordLog, History};
use halvo_schema::kinds::registry;
use halvo_schema::{
    AgentId, AppId, Confidence, Episode, ErrorName, FloorLayer, Instant, InterfaceDecision,
    InterfaceRole, KindEntry, ObservedStep, Ordinal, Part, Position, Proposal, RawRequest,
    Recurrence, RecurrenceStep, RequestId, RowRead, SeedCategory, StoreId, Summary, SummaryId,
    SummaryRead, Tagged, Text, Trigger, TypedRecord, WorkflowId, WriterStep,
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

struct NoProposals;

impl RecurrenceStep for NoProposals {
    fn propose(&self, _: &dyn SummaryRead) -> Vec<Proposal> {
        Vec::new()
    }
}

struct Ones;

impl IdSource for Ones {
    fn fill(&mut self, bytes: &mut [u8]) {
        bytes.fill(1);
    }
}

#[test]
fn the_derived_store_hands_the_handler_its_two_bodies_and_no_other_request() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("halvo-coord-store-roles");
    let entries: Vec<&'static KindEntry> = registry().collect();
    let host = StoreHost::new(Box::new(Noon), Box::new(NoProposals)).handler(
        FAMILY,
        Takes::Coordinator(&OPS),
        Box::new(CoordHandler::new(Box::new(Noon))),
    );
    let mut store =
        Store::open(&dir, &DbKey::new([7; 32]), &entries, host, &mut Ones).expect("a store");
    let answer =
        |store: &mut Store, name: &str, body: &str| store.handle(&request(name, body)).err();
    for body in [
        history_body(9, 1, None),
        log_body(9, 1, &[hold_set(9, 1, 1, TODAY, noon(TODAY))]),
    ] {
        assert_eq!(
            answer(&mut store, "coordinator_write", &body),
            Some(Error::Refused(ErrorName::NoWorkflow)),
            "{body}"
        );
        for name in ["runner", "probe_role"] {
            assert_eq!(
                answer(&mut store, name, &body),
                Some(Error::Refused(ErrorName::Unsupported)),
                "{name}"
            );
        }
    }
    assert_eq!(
        answer(
            &mut store,
            "coordinator_write",
            &history_body(1, 1, None).replacen("coord_history", "coord_histories", 1)
        ),
        Some(Error::Refused(ErrorName::Unsupported))
    );
    assert_eq!(
        answer(
            &mut store,
            "coordinator_write",
            &history_body(9, 1, None).replacen(r#""after":null"#, r#""after":-1"#, 1)
        ),
        Some(Error::Refused(ErrorName::Malformed))
    );
}

#[derive(Clone)]
struct Set(Rc<Cell<i64>>);

impl StoreClock for Set {
    fn now(&self) -> Instant {
        Instant(self.0.get())
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

#[test]
fn the_adapter_stores_and_reads_back_a_row_through_the_derived_store_and_refuses_while_one_is_held_ahead(
) {
    let now = Rc::new(Cell::new(noon(TODAY)));
    let workflow = WorkflowId::from_random([1; 16]);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("halvo-coord-store-adapter");
    let entries: Vec<&'static KindEntry> = registry().collect();
    let host = StoreHost::new(Box::new(Set(Rc::clone(&now))), Box::new(ProposesOne))
        .handler(
            FAMILY,
            Takes::Coordinator(&OPS),
            Box::new(CoordHandler::new(Box::new(Set(Rc::clone(&now))))),
        )
        .handler(
            "coordtest",
            Takes::Coordinator(&["test_agent"]),
            Box::new(GivesAgentOne(workflow.clone())),
        );
    let mut store =
        Store::open(&dir, &DbKey::new([5; 32]), &entries, host, &mut Ones).expect("a store");
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
    let history_of = |after: &str| {
        format!(
            r#"{{"op":"coord_history","workflow":"{w}","agent":"{}","after":{after}}}"#,
            agent(1)
        )
    };
    let read = |store: &mut Store, after: &str| {
        store
            .handle(&request("coordinator_write", &history_of(after)))
            .map(|reply| reply.get().to_owned())
    };
    let before = History::parse(
        read(&mut store, "null")
            .expect("the history before")
            .as_bytes(),
    )
    .expect("a history reply");
    let last = before.last;
    let row = adapter_row(w, last + 1);
    let log = format!(
        r#"{{"op":"coord_log","workflow":"{w}","agent":"{}","records":[{row}]}}"#,
        agent(1)
    );
    let stored = store
        .handle(&request("coordinator_write", &log))
        .expect("the log");
    assert_eq!(stored.get(), r#"{"stored":1}"#);
    let after = read(&mut store, &last.to_string()).expect("the history after");
    let canonical_row = canonical(&row);
    assert_eq!(
        after,
        format!(r#"{{"records":[{canonical_row}],"last":{}}}"#, last + 1)
    );

    now.set(noon(TODAY - 3));
    assert_eq!(
        read(&mut store, "null"),
        Err(Error::Refused(ErrorName::Store))
    );
    now.set(noon(TODAY));
    assert!(read(&mut store, "null").is_ok());
}

#[test]
fn no_source_of_the_crate_writes_a_log_or_names_push_refused_or_decode_error() {
    let mut names: Vec<String> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
            .expect("the source directory")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .into_string()
                    .expect("a name")
            })
            .collect();
    names.sort();
    assert_eq!(names, ["handler.rs", "host.rs", "lib.rs", "memory.rs"]);
    for (name, text) in [
        ("handler.rs", include_str!("../src/handler.rs")),
        ("host.rs", include_str!("../src/host.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
        ("memory.rs", include_str!("../src/memory.rs")),
    ] {
        for word in [
            "print!",
            "println!",
            "dbg!",
            "log::",
            "tracing",
            "stderr",
            "stdout",
            ":?}",
            "{:#",
            "PushRefused",
            "DecodeError",
        ] {
            assert!(!text.contains(word), "{name} holds {word}");
        }
    }
}

#[test]
fn ops_names_the_two_bodies() {
    assert_eq!(OPS, ["coord_log", "coord_history"]);
    assert!(OPS.contains(&CoordLog::OP));
    assert!(OPS.contains(&CoordHistory::OP));
}

#[test]
fn a_further_role_with_a_history_body_is_role() {
    let mut store = store();
    assert_eq!(
        coordinator(
            &mut store,
            &request("probe_role", &history_body(1, 1, None))
        ),
        Err(ErrorName::Role)
    );
}

struct Returns(Vec<TypedRecord>);

impl WriterStep for Returns {
    fn after_commit(&self, _: &dyn SummaryRead, _: &dyn RowRead) -> Vec<TypedRecord> {
        self.0.clone()
    }
}

#[test]
fn coord_roles() {
    let mut analysis = store().with_step(Box::new(Returns(vec![typed(row(1, TODAY))])));
    assert_eq!(analysis.after_batch(), Err(CommitRefused::OwnFromStep));
    assert!(rows(&analysis).is_empty());

    let body = log_body(1, 1, &[row(1, TODAY)]);
    let mut store = store();
    for name in ["runner", "probe_role"] {
        assert_eq!(
            coordinator(&mut store, &request(name, &body)),
            Err(ErrorName::Role),
            "{name}"
        );
        assert!(rows(&store).is_empty(), "{name}");
    }
    assert_eq!(
        coordinator(&mut store, &request("coordinator_write", &body)),
        stored(1)
    );
    assert_eq!(rows(&store), [row(1, TODAY)]);
}
