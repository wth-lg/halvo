mod common;

use std::path::Path;

use common::*;
use halvo_coord_store::{coordinator, CommitRefused, CoordHandler, FAMILY, OPS};
use halvo_derived::{Clock as StoreClock, DbKey, Error, Host as StoreHost, IdSource, Store, Takes};
use halvo_schema::coordinator::body::{CoordHistory, CoordLog};
use halvo_schema::kinds::registry;
use halvo_schema::{
    ErrorName, Instant, KindEntry, Proposal, RecurrenceStep, RowRead, SummaryRead, TypedRecord,
    WriterStep,
};

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
