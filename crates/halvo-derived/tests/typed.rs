mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, count, episode, open_entries, position, raw_request, store_id, Grouping, Scratch, Seq,
    TestClock, FRONT, NOTE, NOTE_UNKEYED, OWN, TRACE,
};
use halvo_derived::{
    Cause, Committed, DayFault, Error, Handler, HandlerCx, Host, OpenError, Refusal, Store,
    StoredFault, Takes,
};
use halvo_schema_core::kinds::{FieldType, KindEntry, Origin, RetentionClass};
use halvo_schema_core::record::{
    Ordinal, RecurrenceStep, RowRead, SummaryRead, TypedRecord, WriterStep,
};
use halvo_schema_core::request::{ErrorName, RawRequest};
use serde_json::value::RawValue;

const DAY: u32 = 20_767;
const FAR: i64 = 9_000_000_000_000_000_000;

type Records = Rc<RefCell<Vec<TypedRecord>>>;

struct Returning {
    records: Records,
    seen: Rc<RefCell<Vec<String>>>,
    read: Vec<&'static str>,
}

impl WriterStep for Returning {
    fn after_commit(&self, _summaries: &dyn SummaryRead, rows: &dyn RowRead) -> Vec<TypedRecord> {
        for kind in &self.read {
            rows.for_each(kind, &mut |row| {
                self.seen.borrow_mut().push(format!("{kind} {row}"));
            });
        }
        self.records.borrow().clone()
    }
}

struct FnHandler<F>(F);

impl<F> Handler for FnHandler<F>
where
    F: Fn(&RawRequest, &mut HandlerCx<'_>) -> Result<Box<RawValue>, ErrorName>,
{
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        (self.0)(request, cx)
    }
}

fn reply(text: &str) -> Box<RawValue> {
    RawValue::from_string(text.to_owned()).unwrap()
}

fn note(workflow: &str, day: u32, n: u32) -> TypedRecord {
    TypedRecord {
        kind: "test_note",
        json: format!(r#"{{"v":1,"workflow":"{workflow}","day":{day},"n":{n}}}"#),
    }
}

fn own(workflow: &str, text: &str) -> TypedRecord {
    TypedRecord {
        kind: "test_own",
        json: format!(
            r#"{{"v":1,"workflow":"{workflow}","note":"{text}","at":1794339000000000000}}"#
        ),
    }
}

fn trace(workflow: &str, segments: &[u64]) -> TypedRecord {
    let list: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
    TypedRecord {
        kind: "test_trace",
        json: format!(
            r#"{{"v":1,"workflow":"{workflow}","segments":[{}],"expires":{FAR}}}"#,
            list.join(",")
        ),
    }
}

fn base(clock: &TestClock, step: impl RecurrenceStep + 'static) -> Host {
    Host::new(Box::new(clock.clone()), Box::new(step))
}

fn kind_rows(scratch: &Scratch, kind: &str) -> Vec<String> {
    bodies(scratch, kind)
}

/// Stores one proposed workflow through the test proposer and returns its id.
fn workflow(scratch: &Scratch, clock: &TestClock, entries: &[&'static KindEntry]) -> String {
    let mut store = open_entries(scratch, base(clock, Grouping { min: 1 }), entries).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    drop(store);
    let body = &bodies(scratch, "workflow")[0];
    let stored: halvo_schema_core::record::Workflow = serde_json::from_str(body).unwrap();
    stored.id().as_str().to_owned()
}

fn with_step(clock: &TestClock, records: &Records, read: Vec<&'static str>) -> Host {
    base(clock, Grouping { min: 1 }).step(
        "test",
        Box::new(Returning {
            records: Rc::clone(records),
            seen: Rc::new(RefCell::new(Vec::new())),
            read,
        }),
    )
}

#[test]
fn a_step_after_the_recurrence_step_has_its_records_stored_by_the_typed_commit() {
    let scratch = Scratch::new("step-commit");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&NOTE]);
    let records: Records = Rc::new(RefCell::new(vec![note(&w, DAY, 1), note(&w, DAY, 2)]));
    let mut store = open_entries(&scratch, with_step(&clock, &records, vec![]), &[&NOTE]).unwrap();
    let outcome = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    assert_eq!(
        outcome.steps,
        vec![Ok(Committed {
            stored: 2,
            dropped: 0
        })]
    );
    drop(store);
    assert_eq!(kind_rows(&scratch, "test_note").len(), 2);
    assert_eq!(
        count(&scratch, "SELECT n FROM revisions WHERE family = 'test'"),
        1
    );
}

#[test]
fn the_typed_commit_refuses_the_whole_list_for_one_record_that_breaks_a_rule() {
    let scratch = Scratch::new("refusals");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&NOTE, &FRONT]);
    let missing = "3".repeat(32);
    let cases: Vec<(TypedRecord, Cause)> = vec![
        (note(&missing, DAY, 1), Cause::NoWorkflow),
        (
            TypedRecord {
                kind: "test_unknown",
                json: "{}".to_owned(),
            },
            Cause::UnknownKind,
        ),
        (
            TypedRecord {
                kind: "summary",
                json: "{}".to_owned(),
            },
            Cause::WorkflowRun,
        ),
        (
            TypedRecord {
                kind: "front_mark",
                json: "{}".to_owned(),
            },
            Cause::OtherFamily,
        ),
        (
            TypedRecord {
                kind: "test_note",
                json: format!(r#"{{"v":1,"workflow":"{w}","day":{DAY}}}"#),
            },
            Cause::Record("test_note: n: missing_key".to_owned()),
        ),
    ];
    for (bad, cause) in cases {
        let records: Records = Rc::new(RefCell::new(vec![note(&w, DAY, 1), bad]));
        let mut store = open_entries(
            &scratch,
            with_step(&clock, &records, vec![]),
            &[&NOTE, &FRONT],
        )
        .unwrap();
        let outcome = store
            .commit_batch(&store_id(1), &[], position(9, 0), &mut Seq(1))
            .unwrap();
        drop(store);
        assert_eq!(outcome.steps, vec![Err(Refusal { index: 1, cause })]);
        assert_eq!(kind_rows(&scratch, "test_note").len(), 0);
        common::raw(&scratch)
            .execute("UPDATE meta SET seg = 1, idx = 0", [])
            .unwrap();
    }
}

#[test]
fn the_typed_commit_refuses_a_record_of_origin_own_from_a_step_and_stores_it_for_a_runner_and_a_coordinator_write(
) {
    let scratch = Scratch::new("own-origin");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&OWN]);
    let records: Records = Rc::new(RefCell::new(vec![own(&w, "From a step")]));
    let committing = |text: &'static str| {
        let w = w.clone();
        Box::new(FnHandler(
            move |_: &RawRequest, cx: &mut HandlerCx<'_>| match cx.commit(&[own(&w, text)]) {
                Ok(_) => Ok(reply("null")),
                Err(_) => Err(ErrorName::Internal),
            },
        ))
    };
    let host = with_step(&clock, &records, vec![])
        .handler("test", Takes::Runner, committing("From a runner"))
        .handler(
            "test",
            Takes::Coordinator(&["test_put"]),
            committing("From a coordinator"),
        )
        .handler(
            "test",
            Takes::Role("test_role"),
            committing("From a further role"),
        );
    let mut store = open_entries(&scratch, host, &[&OWN]).unwrap();
    let outcome = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    assert!(matches!(
        &outcome.steps[..],
        [Err(Refusal {
            index: 0,
            cause: Cause::OwnOrigin
        })]
    ));
    assert_eq!(
        store
            .handle(&raw_request("runner", "{}"))
            .map(|r| r.get().to_owned()),
        Ok("null".to_owned())
    );
    assert_eq!(
        store
            .handle(&raw_request("coordinator_write", r#"{"op":"test_put"}"#))
            .map(|r| r.get().to_owned()),
        Ok("null".to_owned())
    );
    assert_eq!(
        store.handle(&raw_request("test_role", "{}")).err(),
        Some(Error::Refused(ErrorName::Internal))
    );
    drop(store);
    let stored = kind_rows(&scratch, "test_own");
    assert_eq!(stored.len(), 2);
    assert!(stored.iter().any(|r| r.contains("From a runner")));
    assert!(stored.iter().any(|r| r.contains("From a coordinator")));
    let mut store = open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[&OWN]).unwrap();
    store
        .decide(&halvo_schema_core::request::InterfaceDecision::delete(
            &halvo_schema_core::request::InterfaceRole::claim(),
            halvo_schema_core::ids::RequestId::from_random([1; 16]),
            halvo_schema_core::ids::WorkflowId::parse(&w).unwrap(),
        ))
        .unwrap();
    drop(store);
    assert_eq!(kind_rows(&scratch, "test_own").len(), 0);
}

fn delete_leaves(entry: &'static KindEntry, name: &str) -> usize {
    let scratch = Scratch::new(name);
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[entry]);
    let records: Records = Rc::new(RefCell::new(vec![note(&w, DAY, 1)]));
    let mut store = open_entries(&scratch, with_step(&clock, &records, vec![]), &[entry]).unwrap();
    let outcome = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    assert_eq!(
        outcome.steps,
        vec![Ok(Committed {
            stored: 1,
            dropped: 0
        })]
    );
    store
        .decide(&halvo_schema_core::request::InterfaceDecision::delete(
            &halvo_schema_core::request::InterfaceRole::claim(),
            halvo_schema_core::ids::RequestId::from_random([1; 16]),
            halvo_schema_core::ids::WorkflowId::parse(&w).unwrap(),
        ))
        .unwrap();
    drop(store);
    assert_eq!(bodies(&scratch, "workflow").len(), 0);
    kind_rows(&scratch, "test_note")
        .iter()
        .filter(|row| row.contains(&w))
        .count()
}

#[test]
fn deleting_a_workflow_leaves_no_row_of_a_registered_test_kind_that_names_it() {
    assert_eq!(delete_leaves(&NOTE, "cascade-test-kind"), 0);
}

#[test]
fn without_the_workflow_key_in_its_entry_the_rows_of_a_test_kind_outlive_the_delete() {
    assert_eq!(delete_leaves(&NOTE_UNKEYED, "cascade-control"), 1);
}

#[test]
fn a_window_record_that_names_a_purged_ordinal_is_dropped_and_the_rest_of_the_list_applied() {
    let scratch = Scratch::new("typed-purged");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&TRACE, &NOTE]);
    let records: Records = Rc::new(RefCell::new(vec![
        trace(&w, &[40, 41]),
        trace(&w, &[50]),
        note(&w, DAY + 1, 1),
        note(&w, DAY - 93, 2),
        note(&w, DAY, 3),
    ]));
    let mut store = open_entries(
        &scratch,
        with_step(&clock, &records, vec![]),
        &[&TRACE, &NOTE],
    )
    .unwrap();
    store.purge(&[Ordinal::new(41).unwrap()]).unwrap();
    let outcome = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    assert_eq!(
        outcome.steps,
        vec![Ok(Committed {
            stored: 2,
            dropped: 3
        })]
    );
    assert_eq!(store.purge(&[Ordinal::new(50).unwrap()]).unwrap(), 1);
    drop(store);
    assert_eq!(kind_rows(&scratch, "test_trace").len(), 0);
    assert_eq!(kind_rows(&scratch, "test_note").len(), 1);
}

#[test]
fn row_read_visits_the_rows_of_its_own_family_alone_in_byte_order() {
    let scratch = Scratch::new("row-read");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&NOTE, &FRONT]);
    let records: Records = Rc::new(RefCell::new(vec![note(&w, DAY, 7), note(&w, DAY, 3)]));
    let mut store = open_entries(
        &scratch,
        with_step(&clock, &records, vec![]),
        &[&NOTE, &FRONT],
    )
    .unwrap();
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let host = base(&clock, Grouping { min: 1 }).step(
        "test",
        Box::new(Returning {
            records: Rc::new(RefCell::new(Vec::new())),
            seen: Rc::clone(&seen),
            read: vec![
                "test_note",
                "workflow",
                "summary",
                "front_mark",
                "test_missing",
            ],
        }),
    );
    let mut store = open_entries(&scratch, host, &[&NOTE, &FRONT]).unwrap();
    store
        .commit_batch(&store_id(1), &[], position(3, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    assert_eq!(
        *seen.borrow(),
        vec![
            format!(r#"test_note {{"v":1,"workflow":"{w}","day":{DAY},"n":3}}"#),
            format!(r#"test_note {{"v":1,"workflow":"{w}","day":{DAY},"n":7}}"#),
        ]
    );
}

#[test]
fn the_owner_answers_unsupported_or_malformed_where_no_handler_takes_the_request() {
    let scratch = Scratch::new("dispatch");
    let clock = TestClock::at_day(DAY);
    let mut bare = open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[]).unwrap();
    for (name, body) in [
        ("runner", "{}"),
        ("coordinator_write", "[1]"),
        ("coordinator_write", r#"{"op":"test_put"}"#),
        ("test_role", "{}"),
    ] {
        assert_eq!(
            bare.handle(&raw_request(name, body)).err(),
            Some(Error::Refused(ErrorName::Unsupported)),
            "{name} {body}"
        );
    }
    drop(bare);
    let answer = || {
        Box::new(FnHandler(|_: &RawRequest, _: &mut HandlerCx<'_>| {
            Ok(reply("1"))
        }))
    };
    let host = base(&clock, Grouping { min: 1 })
        .handler(
            "test",
            Takes::Coordinator(&["test_put", "test_get"]),
            answer(),
        )
        .handler("test", Takes::Role("test_role"), answer());
    let mut store = open_entries(&scratch, host, &[]).unwrap();
    for (name, body, want) in [
        (
            "coordinator_write",
            r#"{"op":"test_get","x":[1]}"#,
            Ok("1".to_owned()),
        ),
        (
            "coordinator_write",
            r#"{"workflow":null,"agent":null,"op":"test_put"}"#,
            Ok("1".to_owned()),
        ),
        (
            "coordinator_write",
            r#"{"op":"other_op"}"#,
            Err(Error::Refused(ErrorName::Unsupported)),
        ),
        (
            "coordinator_write",
            r#"{"op":7}"#,
            Err(Error::Refused(ErrorName::Malformed)),
        ),
        (
            "coordinator_write",
            r#"{"no_op":"test_put"}"#,
            Err(Error::Refused(ErrorName::Malformed)),
        ),
        (
            "coordinator_write",
            r#"["test_put"]"#,
            Err(Error::Refused(ErrorName::Malformed)),
        ),
        (
            "coordinator_write",
            r#""test_put""#,
            Err(Error::Refused(ErrorName::Malformed)),
        ),
        (
            "coordinator_write",
            r#"{"op":"test_put","op":"test_get"}"#,
            Err(Error::Refused(ErrorName::Malformed)),
        ),
        ("runner", "{}", Err(Error::Refused(ErrorName::Unsupported))),
        ("test_role", "{}", Ok("1".to_owned())),
        (
            "other_role",
            "{}",
            Err(Error::Refused(ErrorName::Unsupported)),
        ),
    ] {
        assert_eq!(
            store
                .handle(&raw_request(name, body))
                .map(|r| r.get().to_owned()),
            want,
            "{name} {body}"
        );
    }
}

#[test]
fn a_handler_in_front_passes_the_request_on_and_commits_after_the_owning_handler() {
    let scratch = Scratch::new("front");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&NOTE, &FRONT]);
    let order = Rc::new(RefCell::new(Vec::new()));
    let (front_order, owning_order) = (Rc::clone(&order), Rc::clone(&order));
    let (front_w, owning_w) = (w.clone(), w.clone());
    let front = FnHandler(move |_: &RawRequest, cx: &mut HandlerCx<'_>| {
        front_order.borrow_mut().push("front");
        let behind = cx.pass_on()?;
        let mut own_rows = 0;
        cx.rows().for_each("test_note", &mut |_| own_rows += 1);
        cx.rows().for_each("front_mark", &mut |_| own_rows += 100);
        let mark = TypedRecord {
            kind: "front_mark",
            json: format!(r#"{{"v":1,"workflow":"{front_w}","seen":1}}"#),
        };
        cx.commit(&[mark]).map_err(|_| ErrorName::Internal)?;
        let note_from_front = cx.commit(&[note(&front_w, DAY, 9)]);
        assert!(matches!(
            note_from_front,
            Err(Refusal {
                cause: Cause::OtherFamily,
                ..
            })
        ));
        Ok(reply(&format!(
            r#"{{"behind":{},"rows":{own_rows}}}"#,
            behind.get()
        )))
    });
    let owning = FnHandler(move |_: &RawRequest, cx: &mut HandlerCx<'_>| {
        owning_order.borrow_mut().push("owning");
        cx.commit(&[note(&owning_w, DAY, 1)])
            .map_err(|_| ErrorName::Internal)?;
        assert_eq!(cx.pass_on().err(), Some(ErrorName::Unsupported));
        Ok(reply("2"))
    });
    let host = base(&clock, Grouping { min: 1 })
        .handler("front", Takes::Runner, Box::new(front))
        .handler("test", Takes::Runner, Box::new(owning));
    let mut store = open_entries(&scratch, host, &[&NOTE, &FRONT]).unwrap();
    let answered = store.handle(&raw_request("runner", "{}")).unwrap();
    assert_eq!(answered.get(), r#"{"behind":2,"rows":0}"#);
    assert_eq!(*order.borrow(), vec!["front", "owning"]);
    drop(store);
    assert_eq!(kind_rows(&scratch, "test_note").len(), 1);
    assert_eq!(kind_rows(&scratch, "front_mark").len(), 1);
}

#[test]
fn open_refuses_a_step_or_a_handler_of_the_family_workflow() {
    let scratch = Scratch::new("workflow-family");
    let clock = TestClock::at_day(DAY);
    let records: Records = Rc::new(RefCell::new(Vec::new()));
    let step = base(&clock, Grouping { min: 1 }).step(
        "workflow",
        Box::new(Returning {
            records: Rc::clone(&records),
            seen: Rc::new(RefCell::new(Vec::new())),
            read: vec![],
        }),
    );
    assert_eq!(
        open_entries(&scratch, step, &[]).err(),
        Some(OpenError::Family("workflow"))
    );
    let handler = base(&clock, Grouping { min: 1 }).handler(
        "Test",
        Takes::Runner,
        Box::new(FnHandler(|_: &RawRequest, _: &mut HandlerCx<'_>| {
            Ok(reply("1"))
        })),
    );
    assert_eq!(
        open_entries(&scratch, handler, &[]).err(),
        Some(OpenError::Family("Test"))
    );
    let _unused: Option<Store> = None;
}

#[test]
fn a_row_stored_before_its_entry_gave_a_workflow_key_goes_with_its_workflow() {
    let scratch = Scratch::new("cascade-late-key");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&NOTE_UNKEYED]);
    let records: Records = Rc::new(RefCell::new(vec![note(&w, DAY, 1)]));
    let mut store = open_entries(
        &scratch,
        with_step(&clock, &records, vec![]),
        &[&NOTE_UNKEYED],
    )
    .unwrap();
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    assert_eq!(kind_rows(&scratch, "test_note").len(), 1);
    let mut store = open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[&NOTE]).unwrap();
    store
        .decide(&halvo_schema_core::request::InterfaceDecision::delete(
            &halvo_schema_core::request::InterfaceRole::claim(),
            halvo_schema_core::ids::RequestId::from_random([1; 16]),
            halvo_schema_core::ids::WorkflowId::parse(&w).unwrap(),
        ))
        .unwrap();
    drop(store);
    assert_eq!(kind_rows(&scratch, "test_note").len(), 0);
}

static CLS_WINDOW: KindEntry = KindEntry {
    name: "test_cls",
    class: RetentionClass::Window,
    origin: Origin::Capture,
    family: "test",
    workflow_key: None,
    fields: &[
        ("v", FieldType::Version),
        ("segments", FieldType::Ordinal),
        ("expires", FieldType::Instant),
    ],
    example: r#"{"v":1,"segments":[4410],"expires":1795478400000000000}"#,
};

static CLS_BOUNDED: KindEntry = KindEntry {
    class: RetentionClass::Bounded { days: 93 },
    fields: &[("v", FieldType::Version), ("day", FieldType::Day)],
    example: r#"{"v":1,"day":20767}"#,
    ..CLS_WINDOW
};

static CLS_KEPT: KindEntry = KindEntry {
    class: RetentionClass::Kept,
    fields: &[("v", FieldType::Version), ("n", FieldType::Count)],
    example: r#"{"v":1,"n":1}"#,
    ..CLS_WINDOW
};

fn cls_record(entry: &KindEntry) -> TypedRecord {
    let json = match entry.class {
        RetentionClass::Window => format!(r#"{{"v":1,"segments":[5],"expires":{FAR}}}"#),
        RetentionClass::Bounded { .. } => format!(r#"{{"v":1,"day":{DAY}}}"#),
        RetentionClass::Kept => r#"{"v":1,"n":1}"#.to_owned(),
    };
    TypedRecord {
        kind: "test_cls",
        json,
    }
}

/// Stores one row of `first` through a step and returns the scratch directory, the clock and what an open with
/// `later` answers.
fn stored_then_opened(
    name: &str,
    first: &'static KindEntry,
    later: &'static KindEntry,
    record: TypedRecord,
) -> (Scratch, TestClock, Result<(), OpenError>) {
    let scratch = Scratch::new(name);
    let clock = TestClock::at_day(DAY);
    let records: Records = Rc::new(RefCell::new(vec![record]));
    let mut store = open_entries(&scratch, with_step(&clock, &records, vec![]), &[first]).unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert!(
        matches!(&outcome.steps[..], [Ok(c)] if c.stored == 1),
        "{name}: {:?}",
        outcome.steps
    );
    drop(store);
    let later_open = open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[later]).map(drop);
    (scratch, clock, later_open)
}

#[test]
fn an_open_refuses_an_entry_that_gives_a_stored_kind_another_class() {
    let classes: [(&str, &'static KindEntry); 3] = [
        ("window", &CLS_WINDOW),
        ("bounded", &CLS_BOUNDED),
        ("kept", &CLS_KEPT),
    ];
    let mut seen = Vec::new();
    for (from, first) in classes {
        for (to, later) in classes {
            if from == to {
                continue;
            }
            let name = format!("class-{from}-{to}");
            let (scratch, clock, opened) =
                stored_then_opened(&name, first, later, cls_record(first));
            seen.push((name.clone(), opened.clone()));
            if from == "kept" && to == "bounded" {
                assert_eq!(kind_rows(&scratch, "test_cls").len(), 1);
                let rows = Rc::new(RefCell::new(Vec::new()));
                let host = base(&clock, Grouping { min: 1 }).step(
                    "test",
                    Box::new(Returning {
                        records: Rc::new(RefCell::new(Vec::new())),
                        seen: Rc::clone(&rows),
                        read: vec!["test_cls"],
                    }),
                );
                let mut store = open_entries(&scratch, host, &[&CLS_KEPT]).unwrap();
                store
                    .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
                    .unwrap();
                drop(store);
                assert_eq!(rows.borrow().len(), 1, "the kept entry reads the row");
            }
        }
    }
    let want: Vec<_> = seen
        .iter()
        .map(|(name, _)| {
            (
                name.clone(),
                Err(OpenError::Stored {
                    kind: "test_cls",
                    fault: StoredFault::Class,
                }),
            )
        })
        .collect();
    assert_eq!(seen, want);
}

static PAIR_A: KindEntry = KindEntry {
    name: "test_pair",
    class: RetentionClass::Kept,
    origin: Origin::Capture,
    family: "test",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("target", FieldType::Id),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","target":"10000000000000000000000000000002"}"#,
};

static PAIR_B: KindEntry = KindEntry {
    workflow_key: Some("target"),
    ..PAIR_A
};

/// Two workflows of two keys, and their ids.
fn two_workflows(scratch: &Scratch, clock: &TestClock) -> (String, String) {
    let mut store = open_entries(scratch, base(clock, Grouping { min: 1 }), &[]).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, DAY, None),
                episode(2, DAY, common::domain("northwind-power.test")),
            ],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    drop(store);
    let ids: Vec<String> = bodies(scratch, "workflow")
        .iter()
        .map(|b| {
            serde_json::from_str::<halvo_schema_core::record::Workflow>(b)
                .unwrap()
                .id()
                .as_str()
                .to_owned()
        })
        .collect();
    (ids[0].clone(), ids[1].clone())
}

fn pair(scratch: &Scratch, clock: &TestClock, workflow: &str, target: &str) {
    let records: Records = Rc::new(RefCell::new(vec![TypedRecord {
        kind: "test_pair",
        json: format!(r#"{{"v":1,"workflow":"{workflow}","target":"{target}"}}"#),
    }]));
    let mut store = open_entries(scratch, with_step(clock, &records, vec![]), &[&PAIR_A]).unwrap();
    let outcome = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    assert!(
        matches!(&outcome.steps[..], [Ok(c)] if c.stored == 1),
        "{:?}",
        outcome.steps
    );
}

fn delete_workflow(store: &mut Store, id: &str) {
    store
        .decide(&halvo_schema_core::request::InterfaceDecision::delete(
            &halvo_schema_core::request::InterfaceRole::claim(),
            halvo_schema_core::ids::RequestId::from_random([1; 16]),
            halvo_schema_core::ids::WorkflowId::parse(id).unwrap(),
        ))
        .unwrap();
}

#[test]
fn an_open_refuses_an_entry_whose_workflow_key_names_another_id_than_a_stored_row() {
    let scratch = Scratch::new("pair-moved-key");
    let clock = TestClock::at_day(DAY);
    let (first, second) = two_workflows(&scratch, &clock);
    pair(&scratch, &clock, &first, &second);
    let opened = open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[&PAIR_B]).map(drop);
    assert_eq!(
        opened,
        Err(OpenError::Stored {
            kind: "test_pair",
            fault: StoredFault::WorkflowKey,
        })
    );
    let scratch = Scratch::new("pair-no-id-under-key");
    let (first, _) = two_workflows(&scratch, &clock);
    let records: Records = Rc::new(RefCell::new(vec![TypedRecord {
        kind: "test_pair",
        json: format!(r#"{{"v":1,"workflow":"{first}","target":null}}"#),
    }]));
    let mut store =
        open_entries(&scratch, with_step(&clock, &records, vec![]), &[&PAIR_A]).unwrap();
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    drop(store);
    assert_eq!(kind_rows(&scratch, "test_pair").len(), 1);
    assert_eq!(
        open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[&PAIR_B]).map(drop),
        Err(OpenError::Stored {
            kind: "test_pair",
            fault: StoredFault::WorkflowKey,
        }),
        "the record holds no id under the key of the entry"
    );
    for (entry, name) in [(&PAIR_A, "pair-same-id-a"), (&PAIR_B, "pair-same-id-b")] {
        let scratch = Scratch::new(name);
        let (first, _) = two_workflows(&scratch, &clock);
        pair(&scratch, &clock, &first, &first);
        let mut store =
            open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[entry]).unwrap();
        delete_workflow(&mut store, &first);
        drop(store);
        assert_eq!(kind_rows(&scratch, "test_pair").len(), 0, "{name}");
    }
}

fn table(scratch: &Scratch, sql: &str) -> Vec<String> {
    let conn = common::raw(scratch);
    let mut statement = conn.prepare(sql).unwrap();
    statement
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn tables(scratch: &Scratch) -> Vec<Vec<String>> {
    vec![
        table(
            scratch,
            "SELECT id || '|' || kind || '|' || class || '|' || ifnull(day, '') || '|' || ifnull(gone, '') || '|' \
             || ifnull(expires, '') || '|' || ifnull(workflow, '') || '|' || body FROM rows ORDER BY id",
        ),
        table(scratch, "SELECT family || '|' || n FROM revisions ORDER BY family"),
        table(
            scratch,
            "SELECT ifnull(store, '') || '|' || ifnull(seg, '') || '|' || ifnull(idx, '') || '|' || mark FROM meta",
        ),
    ]
}

#[test]
fn a_refused_open_changes_no_row_no_revision_and_no_value_of_meta() {
    let scratch = Scratch::new("refused-open-writes-nothing");
    let clock = TestClock::at_day(DAY);
    let (first, second) = two_workflows(&scratch, &clock);
    pair(&scratch, &clock, &first, &second);
    let before = tables(&scratch);
    clock.set_day(DAY + 400);
    let refused = open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[&PAIR_B]).map(drop);
    assert!(refused.is_err());
    assert_eq!(tables(&scratch), before);
    clock.set_day(DAY);
    drop(open_entries(&scratch, base(&clock, Grouping { min: 1 }), &[&PAIR_A]).unwrap());
    assert_eq!(kind_rows(&scratch, "test_pair").len(), 1);
}

#[test]
fn a_window_record_that_names_one_segment_twice_is_stored_once_and_goes_with_its_purge() {
    let scratch = Scratch::new("segment-twice");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&TRACE]);
    let records: Records = Rc::new(RefCell::new(Vec::new()));
    let mut store = open_entries(&scratch, with_step(&clock, &records, vec![]), &[&TRACE]).unwrap();
    let mut at = 2;
    let mut commit = |store: &mut Store, segments: &[u64]| {
        *records.borrow_mut() = vec![trace(&w, segments)];
        let outcome = store
            .commit_batch(&store_id(1), &[], position(at, 0), &mut Seq(at * 100))
            .unwrap();
        at += 1;
        outcome.steps
    };
    assert_eq!(
        commit(&mut store, &[5, 6]),
        vec![Ok(Committed {
            stored: 1,
            dropped: 0
        })]
    );
    assert_eq!(
        commit(&mut store, &[7, 7]),
        vec![Ok(Committed {
            stored: 1,
            dropped: 0
        })]
    );
    assert_eq!(store.purge(&[Ordinal::new(7).unwrap()]).unwrap(), 1);
    store.purge(&[Ordinal::new(8).unwrap()]).unwrap();
    assert_eq!(
        commit(&mut store, &[8, 8]),
        vec![Ok(Committed {
            stored: 0,
            dropped: 1
        })]
    );
    drop(store);
    let left = kind_rows(&scratch, "test_trace");
    assert_eq!(left.len(), 1);
    assert!(left[0].contains("[5,6]"));
    assert_eq!(
        count(
            &scratch,
            "SELECT count(*) FROM sources JOIN rows ON rows.id = sources.row WHERE rows.kind = 'test_trace'"
        ),
        2
    );
}

/// A bounded test kind of 10 days, beside `NOTE` of 93.
static NOTE_TEN: KindEntry = KindEntry {
    name: "test_note_ten",
    class: RetentionClass::Bounded { days: 10 },
    ..NOTE
};

fn note_ten(workflow: &str, day: u32) -> TypedRecord {
    TypedRecord {
        kind: "test_note_ten",
        json: format!(r#"{{"v":1,"workflow":"{workflow}","day":{day},"n":1}}"#),
    }
}

type Answers = Rc<RefCell<Vec<Result<Committed, Refusal>>>>;

/// A handler that commits the list that the test put in `records` and keeps the typed commit's answer.
fn keeping(records: &Records, answers: &Answers) -> Box<dyn Handler> {
    let records = Rc::clone(records);
    let answers = Rc::clone(answers);
    Box::new(FnHandler(move |_: &RawRequest, cx: &mut HandlerCx<'_>| {
        let answer = cx.commit(&records.borrow());
        answers.borrow_mut().push(answer);
        Ok(reply("null"))
    }))
}

#[test]
fn a_handler_s_typed_commit_refuses_the_list_for_a_bounded_day_after_the_clock_or_outside_its_bound(
) {
    let scratch = Scratch::new("day-refused");
    let clock = TestClock::at_day(DAY);
    let entries: &[&'static KindEntry] = &[&NOTE, &NOTE_TEN];
    let w = workflow(&scratch, &clock, entries);
    let records: Records = Rc::new(RefCell::new(Vec::new()));
    let answers: Answers = Rc::new(RefCell::new(Vec::new()));
    let step: Records = Rc::new(RefCell::new(Vec::new()));
    let host = || {
        with_step(&clock, &step, vec![])
            .handler("test", Takes::Runner, keeping(&records, &answers))
            .handler(
                "test",
                Takes::Coordinator(&["test_put"]),
                keeping(&records, &answers),
            )
            .handler(
                "test",
                Takes::Role("test_role"),
                keeping(&records, &answers),
            )
    };
    let requests = [
        raw_request("runner", "{}"),
        raw_request("coordinator_write", r#"{"op":"test_put"}"#),
        raw_request("test_role", "{}"),
    ];
    let cases = [
        (
            "runner, after the clock",
            note(&w, DAY + 1, 2),
            DayFault::AfterToday,
        ),
        (
            "coordinator, 93 days",
            note(&w, DAY - 93, 2),
            DayFault::OutsideBound,
        ),
        (
            "further role, 10 days",
            note_ten(&w, DAY - 10),
            DayFault::OutsideBound,
        ),
    ];
    for ((name, middle, fault), request) in cases.into_iter().zip(&requests) {
        drop(open_entries(&scratch, host(), entries).unwrap());
        let before = tables(&scratch);
        *records.borrow_mut() = vec![note(&w, DAY, 1), middle, note(&w, DAY, 3)];
        let mut store = open_entries(&scratch, host(), entries).unwrap();
        store.handle(request).unwrap();
        drop(store);
        assert_eq!(
            answers.borrow_mut().pop(),
            Some(Err(Refusal {
                index: 1,
                cause: Cause::Day(fault)
            })),
            "{name}"
        );
        assert_eq!(tables(&scratch), before, "{name}");
    }
    let refused = Refusal {
        index: 1,
        cause: Cause::Day(DayFault::AfterToday),
    };
    assert_eq!(
        refused.to_string(),
        "record 1: a bounded day after the clock's day"
    );
    *records.borrow_mut() = vec![
        note(&w, DAY, 4),
        note(&w, DAY - 92, 5),
        note_ten(&w, DAY - 9),
    ];
    let mut store = open_entries(&scratch, host(), entries).unwrap();
    store.handle(&requests[0]).unwrap();
    drop(store);
    assert_eq!(
        answers.borrow_mut().pop(),
        Some(Ok(Committed {
            stored: 3,
            dropped: 0
        })),
        "the clock's day and the last day inside each bound are stored"
    );
    assert_eq!(kind_rows(&scratch, "test_note").len(), 2);
    assert_eq!(kind_rows(&scratch, "test_note_ten").len(), 1);
    *step.borrow_mut() = vec![
        note(&w, DAY + 1, 6),
        note(&w, DAY - 93, 7),
        note_ten(&w, DAY - 10),
    ];
    let mut store = open_entries(&scratch, host(), entries).unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[episode(9, DAY + 1, None), episode(8, DAY - 93, None)],
            position(2, 0),
            &mut Seq(1),
        )
        .unwrap();
    drop(store);
    assert_eq!(
        outcome.steps,
        vec![Ok(Committed {
            stored: 0,
            dropped: 3
        })],
        "a step's typed commit drops and counts"
    );
    assert_eq!(
        (outcome.summaries, outcome.outside),
        (0, 2),
        "commit_batch counts the episodes as outside"
    );
    assert_eq!(kind_rows(&scratch, "test_note").len(), 2);
}

/// A handler that passes the request on and answers what the handler behind answers.
fn passing() -> Box<dyn Handler> {
    Box::new(FnHandler(|_: &RawRequest, cx: &mut HandlerCx<'_>| {
        cx.pass_on()
    }))
}

/// A handler that commits one record of origin own with `text` and keeps the typed commit's answer.
fn committing_own(workflow: &str, text: &'static str, answers: &Answers) -> Box<dyn Handler> {
    let workflow = workflow.to_owned();
    let answers = Rc::clone(answers);
    Box::new(FnHandler(move |_: &RawRequest, cx: &mut HandlerCx<'_>| {
        let answer = cx.commit(&[own(&workflow, text)]);
        answers.borrow_mut().push(answer);
        Ok(reply("null"))
    }))
}

#[test]
fn a_request_of_a_further_role_that_a_handler_passes_on_stores_no_record_of_origin_own() {
    let scratch = Scratch::new("pass-on-own");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&OWN]);
    let answers: Answers = Rc::new(RefCell::new(Vec::new()));
    let role = || Takes::Role("test_role");
    let cases: [(&str, Host, RawRequest, bool); 3] = [
        (
            "a further role, one handler in front",
            base(&clock, Grouping { min: 1 })
                .handler("test", role(), passing())
                .handler("test", role(), committing_own(&w, "Behind two", &answers)),
            raw_request("test_role", "{}"),
            false,
        ),
        (
            "a further role, three handlers deep",
            base(&clock, Grouping { min: 1 })
                .handler("test", role(), passing())
                .handler("test", role(), passing())
                .handler("test", role(), committing_own(&w, "Behind three", &answers)),
            raw_request("test_role", "{}"),
            false,
        ),
        (
            "a runner request passed on, the control",
            base(&clock, Grouping { min: 1 })
                .handler("test", Takes::Runner, passing())
                .handler(
                    "test",
                    Takes::Runner,
                    committing_own(&w, "Behind a runner", &answers),
                ),
            raw_request("runner", "{}"),
            true,
        ),
    ];
    let mut seen = Vec::new();
    for (name, host, request, stores) in cases {
        let mut store = open_entries(&scratch, host, &[&OWN]).unwrap();
        store.handle(&request).unwrap();
        drop(store);
        let answer = answers.borrow_mut().pop().unwrap();
        let rows = kind_rows(&scratch, "test_own").len();
        seen.push((name, answer.is_ok(), rows));
        if !stores {
            assert_eq!(
                answer,
                Err(Refusal {
                    index: 0,
                    cause: Cause::OwnOrigin
                }),
                "{name}"
            );
        }
    }
    assert_eq!(
        seen,
        vec![
            ("a further role, one handler in front", false, 0),
            ("a further role, three handlers deep", false, 0),
            ("a runner request passed on, the control", true, 1),
        ]
    );
    assert!(kind_rows(&scratch, "test_own")[0].contains("Behind a runner"));
}

fn trace_until(workflow: &str, segment: u64, expires: i64) -> TypedRecord {
    TypedRecord {
        kind: "test_trace",
        json: format!(
            r#"{{"v":1,"workflow":"{workflow}","segments":[{segment}],"expires":{expires}}}"#
        ),
    }
}

#[test]
fn a_window_record_whose_expiry_has_passed_is_dropped_and_counted_by_a_handler_and_by_a_step() {
    let scratch = Scratch::new("window-expired");
    let clock = TestClock::at_day(DAY);
    let entries: &[&'static KindEntry] = &[&NOTE, &TRACE];
    let w = workflow(&scratch, &clock, entries);
    let records: Records = Rc::new(RefCell::new(Vec::new()));
    let answers: Answers = Rc::new(RefCell::new(Vec::new()));
    let step: Records = Rc::new(RefCell::new(Vec::new()));
    let host = || {
        with_step(&clock, &step, vec![]).handler("test", Takes::Runner, keeping(&records, &answers))
    };
    let now = clock.now();
    *records.borrow_mut() = vec![
        note(&w, DAY, 1),
        trace_until(&w, 40, now),
        trace_until(&w, 41, FAR),
    ];
    let mut store = open_entries(&scratch, host(), entries).unwrap();
    store.handle(&raw_request("runner", "{}")).unwrap();
    *step.borrow_mut() = vec![
        trace_until(&w, 42, now - 1),
        note(&w, DAY, 2),
        trace_until(&w, 43, FAR),
    ];
    let outcome = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    drop(store);
    let committed = Ok(Committed {
        stored: 2,
        dropped: 1,
    });
    assert_eq!(
        answers.borrow_mut().pop(),
        Some(committed.clone()),
        "a handler's commit, expiry at the clock's instant"
    );
    assert_eq!(
        outcome.steps,
        vec![committed],
        "a step's commit, expiry one nanosecond before it"
    );
    assert_eq!(kind_rows(&scratch, "test_note").len(), 2);
    let traces = kind_rows(&scratch, "test_trace");
    assert_eq!(traces.len(), 2);
    assert!(traces.iter().all(|t| t.contains(&FAR.to_string())));
    assert_eq!(
        count(
            &scratch,
            &format!("SELECT count(*) FROM rows WHERE kind = 'test_trace' AND expires <= {now}")
        ),
        0
    );
}

/// DS-4: a handler's row read hands out a bounded row while its day of going lies after the clock's day, and from
/// the first instant of that day leaves it out with no expiry run; the expiry at that clock still removes it.
#[test]
fn a_handler_s_row_read_leaves_out_a_bounded_row_from_the_first_instant_of_its_day_of_going() {
    let scratch = Scratch::new("row-read-past-bound");
    let clock = TestClock::at_day(DAY);
    let w = workflow(&scratch, &clock, &[&NOTE]);
    let step: Records = Rc::new(RefCell::new(vec![note(&w, DAY, 1)]));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let reading = {
        let seen = Rc::clone(&seen);
        Box::new(FnHandler(move |_: &RawRequest, cx: &mut HandlerCx<'_>| {
            let mut visited = 0;
            cx.rows().for_each("test_note", &mut |_| visited += 1);
            seen.borrow_mut().push(visited);
            Ok(reply("null"))
        }))
    };
    let host = with_step(&clock, &step, vec![]).handler("test", Takes::Runner, reading);
    let mut store = open_entries(&scratch, host, &[&NOTE]).unwrap();
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    let read = |store: &mut Store| store.handle(&raw_request("runner", "{}")).unwrap();
    read(&mut store);
    let going = i64::from(DAY + 93) * common::DAY_NS;
    clock.set(going - 1);
    read(&mut store);
    clock.set(going);
    read(&mut store);
    let removed = store.expire().unwrap();
    read(&mut store);
    drop(store);
    assert_eq!(
        (seen.borrow().clone(), removed > 0),
        (vec![1, 1, 0, 0], true),
        "(rows visited on the row's day, one nanosecond before its day of going, at its first instant with no \
         expiry run, after the expiry; the expiry removed rows)"
    );
}
