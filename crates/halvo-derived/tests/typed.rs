mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, count, episode, open_entries, position, raw_request, store_id, Grouping, Scratch, Seq,
    TestClock, FRONT, NOTE, NOTE_UNKEYED, OWN, TRACE,
};
use halvo_derived::{
    Cause, Committed, DayFault, Error, Handler, HandlerCx, HeldRefusal, Host, OpenError, Refusal,
    Store, StoredFault, Takes,
};
use halvo_schema_core::ids::WorkflowId;
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

/// The kind `test_note` under another name with no workflow key in its entry: its records name a workflow under the
/// key `workflow`, and the store keeps no workflow column for its rows.
static PLAIN: KindEntry = KindEntry {
    name: "test_plain",
    workflow_key: None,
    ..NOTE
};

/// What a handler of the family `test` reads in one request: the kind, and the workflow of a keyed read or none for
/// the read of the whole kind.
type Asks = Rc<RefCell<Vec<(&'static str, Option<WorkflowId>)>>>;

/// A runner handler that passes the request on first where `pass` is set, then commits the records that the test
/// put in `records`, then makes the reads that the test put in `asks` and keeps what each visited in `seen`.
fn keyed_handler(pass: bool, records: &Records, asks: &Asks, seen: &Seen) -> Box<dyn Handler> {
    let records = Rc::clone(records);
    let asks = Rc::clone(asks);
    let seen = Rc::clone(seen);
    Box::new(FnHandler(move |_: &RawRequest, cx: &mut HandlerCx<'_>| {
        if pass {
            cx.pass_on()?;
        }
        let list: Vec<TypedRecord> = records.borrow_mut().drain(..).collect();
        if !list.is_empty() {
            assert!(matches!(cx.commit(&list), Ok(c) if c.dropped == 0));
        }
        for (kind, workflow) in asks.borrow().iter() {
            let mut rows = Vec::new();
            let read: halvo_derived::Rows<'_> = cx.rows();
            match workflow {
                Some(w) => read.for_each_under(kind, w, &mut |row| rows.push(row.to_owned())),
                None => read.for_each(kind, &mut |row| rows.push(row.to_owned())),
            }
            seen.borrow_mut().push(rows);
        }
        Ok(reply("null"))
    }))
}

type Seen = Rc<RefCell<Vec<Vec<String>>>>;

/// A store with two workflows and, for each, rows of a bounded, a window and a kept kind of the family `test`, a
/// row of the family `front`, and rows of `test_plain` that name the first workflow in their text alone. The
/// bounded rows of the first workflow lie on DAY, DAY - 92 (going on DAY + 1), DAY + 2 and DAY + 3, the last two
/// committed with the clock on their own day; its window rows expire at noon of DAY plus an hour and far later. The
/// clock is then set to `at`.
struct Keyed {
    store: Store,
    first: WorkflowId,
    second: WorkflowId,
    asks: Asks,
    seen: Seen,
    scratch: Scratch,
}

const NOON: i64 = 43_200_000_000_000;
const HOUR: i64 = 3_600_000_000_000;

fn plain(workflow: &str, day: u32, n: u32) -> TypedRecord {
    TypedRecord {
        kind: "test_plain",
        json: format!(r#"{{"v":1,"workflow":"{workflow}","day":{day},"n":{n}}}"#),
    }
}

impl Keyed {
    fn new(at: i64) -> Keyed {
        let scratch = Scratch::new("keyed");
        let clock = TestClock::at_day(DAY);
        let (w1, w2) = two_workflows(&scratch, &clock);
        let records: Records = Rc::new(RefCell::new(Vec::new()));
        let front: Records = Rc::new(RefCell::new(Vec::new()));
        let asks: Asks = Rc::new(RefCell::new(Vec::new()));
        let seen: Seen = Rc::new(RefCell::new(Vec::new()));
        let none: Asks = Rc::new(RefCell::new(Vec::new()));
        let host = base(&clock, Grouping { min: 1 })
            .handler(
                "front",
                Takes::Runner,
                keyed_handler(true, &front, &none, &Rc::new(RefCell::new(Vec::new()))),
            )
            .handler(
                "test",
                Takes::Runner,
                keyed_handler(false, &records, &asks, &seen),
            );
        let mut store =
            open_entries(&scratch, host, &[&NOTE, &TRACE, &OWN, &FRONT, &PLAIN]).unwrap();
        let soon = i64::from(DAY) * common::DAY_NS + NOON + HOUR;
        *records.borrow_mut() = vec![
            note(&w1, DAY, 2),
            note(&w1, DAY, 1),
            note(&w1, DAY - 92, 3),
            note(&w2, DAY, 4),
            trace_until(&w1, 11, soon),
            trace_until(&w1, 12, FAR),
            trace_until(&w2, 13, FAR),
            own(&w2, "Second"),
            own(&w1, "First"),
            plain(&w1, DAY, 5),
        ];
        *front.borrow_mut() = vec![TypedRecord {
            kind: "front_mark",
            json: format!(r#"{{"v":1,"workflow":"{w1}","seen":1}}"#),
        }];
        let request = raw_request("runner", "{}");
        store.handle(&request).unwrap();
        for (day, n) in [(DAY + 2, 6), (DAY + 3, 7)] {
            clock.set_day(day);
            *records.borrow_mut() = vec![note(&w1, day, n)];
            store.handle(&request).unwrap();
        }
        assert!(records.borrow().is_empty() && front.borrow().is_empty());
        clock.set(at);
        Keyed {
            store,
            first: WorkflowId::parse(&w1).unwrap(),
            second: WorkflowId::parse(&w2).unwrap(),
            asks,
            seen,
            scratch,
        }
    }

    /// The rows that each of `asks` visits, in one handled request.
    fn read(&mut self, asks: Vec<(&'static str, Option<WorkflowId>)>) -> Vec<Vec<String>> {
        *self.asks.borrow_mut() = asks;
        self.seen.borrow_mut().clear();
        self.store.handle(&raw_request("runner", "{}")).unwrap();
        self.seen.borrow().clone()
    }
}

/// The workflow id that a row of a test kind names under the key `workflow`.
fn named(row: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(row).unwrap();
    value["workflow"].as_str().unwrap().to_owned()
}

/// RD-64 (a), C2.11: at each clock, a handler's keyed read of a kind of its own family hands out, for each
/// workflow, exactly the rows of the read of the whole kind that name that workflow, in the same order; before an
/// expiry and after it at the same clock alike, so by DS-4's rule; and nothing for a kind whose rows hold no workflow
/// column, a kind of another family, a kind of the workflow run, a kind that is not registered or a workflow that is
/// not stored.
#[test]
fn a_handler_s_keyed_row_read_hands_out_the_rows_of_the_whole_read_that_name_its_workflow_and_no_other(
) {
    let day = |d: u32| i64::from(d) * common::DAY_NS;
    let soon = day(DAY) + NOON + HOUR;
    let clocks = [
        ("noon of DAY", day(DAY) + NOON),
        ("expiry - 1", soon - 1),
        ("expiry", soon),
        ("going - 1", day(DAY + 1) - 1),
        ("going", day(DAY + 1)),
        ("noon of DAY + 1", day(DAY + 1) + NOON),
        ("one day back", day(DAY - 1) + NOON),
        ("DAY + 93", day(DAY + 93)),
    ];
    let keyed_kinds = ["test_note", "test_trace", "test_own"];
    let empty_kinds = [
        "test_plain",
        "front_mark",
        "support",
        "workflow",
        "summary",
        "evidence",
        "test_missing",
    ];
    let mut walked = Vec::new();
    for (label, at) in clocks {
        let mut fx = Keyed::new(at);
        let (w1, w2) = (fx.first.clone(), fx.second.clone());
        let stray = WorkflowId::from_random([9; 16]);
        let mut asks = Vec::new();
        for kind in keyed_kinds {
            asks.push((kind, None));
            asks.push((kind, Some(w1.clone())));
            asks.push((kind, Some(w2.clone())));
            asks.push((kind, Some(stray.clone())));
        }
        for kind in empty_kinds {
            asks.push((kind, Some(w1.clone())));
        }
        asks.push(("test_plain", None));
        let before = fx.read(asks.clone());
        for (k, kind) in keyed_kinds.iter().enumerate() {
            let whole = &before[4 * k];
            for (j, w) in [&w1, &w2, &stray].iter().enumerate() {
                let want: Vec<String> = whole
                    .iter()
                    .filter(|row| named(row) == w.as_str())
                    .cloned()
                    .collect();
                assert_eq!(
                    before[4 * k + 1 + j],
                    want,
                    "{label}: {kind} under workflow {j}"
                );
            }
        }
        for (k, kind) in empty_kinds.iter().enumerate() {
            assert!(
                before[12 + k].is_empty(),
                "{label}: {kind} under the first workflow"
            );
        }
        let plain_rows = &before[12 + empty_kinds.len()];
        assert!(
            plain_rows.iter().all(|row| named(row) == w1.as_str()),
            "{label}: the rows of test_plain name the first workflow"
        );
        let removed = fx.store.expire().unwrap();
        let after = fx.read(asks);
        assert_eq!(
            before, after,
            "{label}: the reads before and after the expiry"
        );
        walked.push((
            label,
            before[1].len(),
            before[5].len(),
            before[9].len(),
            removed > 0,
        ));
    }
    assert_eq!(
        walked,
        vec![
            ("noon of DAY", 4, 2, 1, true),
            ("expiry - 1", 4, 2, 1, true),
            ("expiry", 4, 1, 1, true),
            ("going - 1", 4, 1, 1, true),
            ("going", 4, 1, 1, true),
            ("noon of DAY + 1", 4, 1, 1, true),
            ("one day back", 3, 2, 1, true),
            ("DAY + 93", 2, 1, 1, true),
        ],
        "(clock, the first workflow's rows of test_note, of test_trace and of test_own, the expiry removed rows)"
    );
    let mut fx = Keyed::new(day(DAY) + NOON);
    let w1 = fx.first.clone();
    let rows = fx.read(vec![("test_note", Some(w1.clone()))]).remove(0);
    let w = w1.as_str();
    assert_eq!(
        rows,
        vec![
            format!(r#"{{"v":1,"workflow":"{w}","day":{},"n":3}}"#, DAY - 92),
            format!(r#"{{"v":1,"workflow":"{w}","day":{DAY},"n":1}}"#),
            format!(r#"{{"v":1,"workflow":"{w}","day":{DAY},"n":2}}"#),
            format!(r#"{{"v":1,"workflow":"{w}","day":{},"n":6}}"#, DAY + 2),
        ],
        "the first workflow's rows of test_note in ascending byte order"
    );
    drop(fx.store);
    assert_eq!(
        (
            count(&fx.scratch, "SELECT count(*) FROM rows WHERE kind = 'front_mark'"),
            count(&fx.scratch, "SELECT count(*) FROM rows WHERE kind = 'test_plain'"),
            count(
                &fx.scratch,
                "SELECT count(*) FROM rows WHERE kind = 'support' AND workflow IS NOT NULL"
            ),
        ),
        (1, 1, 2),
        "(rows of front_mark, of test_plain, support rows with a workflow column) stand while the keyed read hands \
         none of them out"
    );
}

/// The kind `test_note` as a later entry of its family gives it: one more key, `more`, so that the check of a record
/// under this entry refuses the text of a row stored under `NOTE`.
static NOTE_LATER: KindEntry = KindEntry {
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("day", FieldType::Day),
        ("n", FieldType::Count),
        ("more", FieldType::Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":20767,"n":1,"more":1}"#,
    ..NOTE
};

/// The typed reader of `test_note` that a later run of the family `test` would hold: it requires `more` and takes
/// no key that it does not name.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct LaterNote {
    v: u32,
    workflow: String,
    day: u32,
    n: u32,
    more: u32,
}

/// C25.2, RD-64 (b): a stored row whose text a later typed reader of its family refuses keeps its workflow column and
/// its day of going: after an open with the later entry the keyed read of its workflow hands it out while it is
/// inside its bound, one nanosecond before its day of going as well, an expiry then removes nothing, and from the
/// first instant of its day of going the read hands it out no more and the next expiry removes it.
#[test]
fn a_stored_row_that_a_later_typed_reader_refuses_keeps_its_workflow_and_its_day_and_goes_at_that_day(
) {
    let scratch = Scratch::new("later-reader");
    let clock = TestClock::at_day(DAY);
    let (w1, w2) = two_workflows(&scratch, &clock);
    let records: Records = Rc::new(RefCell::new(vec![note(&w1, DAY, 1), note(&w2, DAY, 2)]));
    let none: Asks = Rc::new(RefCell::new(Vec::new()));
    let unseen: Seen = Rc::new(RefCell::new(Vec::new()));
    let host = base(&clock, Grouping { min: 1 }).handler(
        "test",
        Takes::Runner,
        keyed_handler(false, &records, &none, &unseen),
    );
    let mut store = open_entries(&scratch, host, &[&NOTE]).unwrap();
    store.handle(&raw_request("runner", "{}")).unwrap();
    drop(store);
    let stored = format!(r#"{{"v":1,"workflow":"{w1}","day":{DAY},"n":1}}"#);
    assert!(halvo_schema_core::kinds::check_record(&NOTE, &stored).is_ok());
    assert!(halvo_schema_core::kinds::check_record(&NOTE_LATER, &stored).is_err());
    assert!(serde_json::from_str::<LaterNote>(&stored).is_err());
    let columns = || {
        common::raw(&scratch)
            .query_row(
                "SELECT workflow, gone FROM rows WHERE kind = 'test_note' AND body = ?1",
                [&stored],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .unwrap()
    };
    assert_eq!(columns(), (w1.clone(), i64::from(DAY + 93)));
    let asks: Asks = Rc::new(RefCell::new(vec![(
        "test_note",
        Some(WorkflowId::parse(&w1).unwrap()),
    )]));
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let host = base(&clock, Grouping { min: 1 }).handler(
        "test",
        Takes::Runner,
        keyed_handler(false, &Rc::new(RefCell::new(Vec::new())), &asks, &seen),
    );
    let mut store = open_entries(&scratch, host, &[&NOTE_LATER]).unwrap();
    let going = i64::from(DAY + 93) * common::DAY_NS;
    let mut walked = Vec::new();
    for at in [
        i64::from(DAY + 92) * common::DAY_NS + NOON,
        going - 1,
        going,
    ] {
        clock.set(at);
        seen.borrow_mut().clear();
        store.handle(&raw_request("runner", "{}")).unwrap();
        let visited = seen.borrow()[0].clone();
        let removed = store.expire().unwrap();
        walked.push((visited == [stored.clone()], visited.is_empty(), removed > 0));
    }
    drop(store);
    assert_eq!(
        walked,
        vec![(true, false, false), (true, false, false), (false, true, true)],
        "(the keyed read hands out the stored row, it hands out nothing, the expiry removed a row) on DAY + 92, one \
         nanosecond before the day of going and at its first instant"
    );
    assert_eq!(
        count(
            &scratch,
            "SELECT count(*) FROM rows WHERE kind = 'test_note'"
        ),
        0
    );
}

/// A test-only kept kind of the family `test` whose rows count the requests that read it before they wrote it.
static SEQ: KindEntry = KindEntry {
    name: "test_seq",
    class: RetentionClass::Kept,
    origin: Origin::Capture,
    family: "test",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("seq", FieldType::Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","seq":0}"#,
};

/// The type of `Store::commit_batch` when it takes the store by `&mut`.
type CommitBatch = fn(
    &mut Store,
    &halvo_schema_core::ids::StoreId,
    &[halvo_schema_core::record::Episode],
    halvo_schema_core::ids::Position,
    &mut dyn halvo_derived::IdSource,
) -> Result<halvo_derived::CommitOutcome, Error>;

/// C25.5 and the agent-setup run's ruling CP-132 (d): two requests of one family never interleave at the test host.
/// The store takes every call that writes by `&mut Store`, `Store::handle` among them, so a host holds the store
/// itself, which holds the writer lock of every family, from a request's first read to its last commit: the test
/// host holds it in a `RefCell`, and the handler, which reads the count of its rows, tries between that read and its
/// commit to have the host handle a second request of its family. That attempt finds the store held, no write of a
/// second request falls between the read and the commit of the first, and each request writes the next count once.
#[test]
fn two_requests_of_one_family_never_interleave_at_the_test_host() {
    let handle: fn(&mut Store, &RawRequest) -> Result<Box<RawValue>, Error> = Store::handle;
    let _: CommitBatch = Store::commit_batch;
    let _: fn(&mut Store, &halvo_schema_core::request::InterfaceDecision) -> Result<(), Error> =
        Store::decide;
    let _: fn(&mut Store, &[Ordinal]) -> Result<u64, halvo_derived::Fault> = Store::purge;
    let _: fn(&mut Store, Ordinal) -> Result<u64, halvo_derived::Fault> = Store::purge_below;
    let _: fn(&mut Store) -> Result<u64, halvo_derived::Fault> = Store::expire;
    let scratch = Scratch::new("one-request-at-a-time");
    let clock = TestClock::at_day(DAY);
    let (w1, _) = two_workflows(&scratch, &clock);
    let workflow = WorkflowId::parse(&w1).unwrap();
    let slot: Rc<RefCell<Option<Store>>> = Rc::new(RefCell::new(None));
    let events: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let handler = {
        let slot = Rc::downgrade(&slot);
        let events = Rc::clone(&events);
        Box::new(FnHandler(
            move |request: &RawRequest, cx: &mut HandlerCx<'_>| {
                let label = request.body().get().trim_matches('"').to_owned();
                let mut next = 0;
                cx.rows()
                    .for_each_under("test_seq", &workflow, &mut |_| next += 1);
                events.borrow_mut().push(format!("read {label} {next}"));
                let slot = slot.upgrade().unwrap();
                let attempt = slot.try_borrow_mut();
                match attempt {
                    Ok(mut store) => {
                        let inner = raw_request("runner", r#""inner""#);
                        let answer = store.as_mut().map(|s| s.handle(&inner).is_ok());
                        events
                            .borrow_mut()
                            .push(format!("a second request ran: {answer:?}"));
                    }
                    Err(_) => events.borrow_mut().push(format!("held during {label}")),
                }
                let committed = cx.commit(&[TypedRecord {
                    kind: "test_seq",
                    json: format!(
                        r#"{{"v":1,"workflow":"{}","seq":{next}}}"#,
                        workflow.as_str()
                    ),
                }]);
                events
                    .borrow_mut()
                    .push(format!("commit {label} {}", committed.is_ok()));
                Ok(reply("null"))
            },
        ))
    };
    let reports = Rc::clone(&events);
    let host = base(&clock, Grouping { min: 1 })
        .handler("test", Takes::Runner, handler)
        .on_report(Box::new(move |_| {
            reports.borrow_mut().push("report".to_owned())
        }));
    *slot.borrow_mut() = Some(open_entries(&scratch, host, &[&SEQ]).unwrap());
    events.borrow_mut().clear();
    {
        let mut held = slot.borrow_mut();
        let store = held.as_mut().unwrap();
        for label in ["a", "b"] {
            handle(store, &raw_request("runner", &format!(r#""{label}""#))).unwrap();
        }
    }
    drop(slot.borrow_mut().take());
    assert_eq!(
        *events.borrow(),
        [
            "read a 0",
            "held during a",
            "report",
            "commit a true",
            "read b 1",
            "held during b",
            "report",
            "commit b true",
        ]
    );
    assert_eq!(
        kind_rows(&scratch, "test_seq"),
        [
            format!(r#"{{"v":1,"workflow":"{w1}","seq":0}}"#),
            format!(r#"{{"v":1,"workflow":"{w1}","seq":1}}"#),
        ]
    );
}

/// C25.5, `Store::handle`: a fault of the store while a handler's keyed read reads a row is answered as the fault of
/// the request. A row of the first workflow whose stored text the store's own reader cannot take (bytes that are no
/// UTF-8, written by the test's own connection) makes the keyed read of that workflow and the read of the whole kind
/// answer the fault, while the keyed read of the second workflow, which reaches no such row, answers as before.
#[test]
fn a_fault_of_a_handler_s_keyed_read_is_answered_as_the_fault_of_its_request_and_stays_with_its_workflow(
) {
    let scratch = Scratch::new("keyed-fault");
    let clock = TestClock::at_day(DAY);
    let (w1, w2) = two_workflows(&scratch, &clock);
    let records: Records = Rc::new(RefCell::new(vec![note(&w1, DAY, 1), note(&w2, DAY, 2)]));
    let asks: Asks = Rc::new(RefCell::new(Vec::new()));
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let host = || {
        base(&clock, Grouping { min: 1 }).handler(
            "test",
            Takes::Runner,
            keyed_handler(false, &records, &asks, &seen),
        )
    };
    let mut store = open_entries(&scratch, host(), &[&NOTE]).unwrap();
    store.handle(&raw_request("runner", "{}")).unwrap();
    drop(store);
    let mut bytes = format!(r#"{{"v":1,"workflow":"{w1}","day":{DAY},"n":1,"x":""#).into_bytes();
    bytes.extend_from_slice(&[0xff, b'"', b'}']);
    let changed = common::raw(&scratch)
        .execute(
            "UPDATE rows SET body = CAST(?1 AS TEXT) WHERE kind = 'test_note' AND workflow = ?2",
            (bytes, &w1),
        )
        .unwrap();
    assert_eq!(changed, 1);
    let mut store = open_entries(&scratch, host(), &[&NOTE]).unwrap();
    let mut answers = Vec::new();
    for ask in [
        ("test_note", Some(WorkflowId::parse(&w1).unwrap())),
        ("test_note", None),
        ("test_note", Some(WorkflowId::parse(&w2).unwrap())),
    ] {
        *asks.borrow_mut() = vec![ask];
        seen.borrow_mut().clear();
        let answer = store.handle(&raw_request("runner", "{}"));
        answers.push((
            matches!(answer, Err(Error::Fault(halvo_derived::Fault::Row))),
            seen.borrow().iter().map(Vec::len).sum::<usize>(),
        ));
    }
    drop(store);
    assert_eq!(
        answers,
        vec![(true, 0), (true, 0), (false, 1)],
        "(the request answered the fault, rows visited) for the keyed read of the first workflow, the read of the \
         whole kind and the keyed read of the second workflow"
    );
}

/// What one handler asks with `HandlerCx::held_ahead` in one request: the kind and the workflow id.
type HeldAsks = Rc<RefCell<Vec<(&'static str, String)>>>;
type HeldAnswers = Rc<RefCell<Vec<Result<bool, HeldRefusal>>>>;

/// A handler that commits the records that the test put in `records`, asks each of `asks` and keeps the answers in
/// `answers`, and, where `pass` is set, passes the request on after its asks.
fn held_handler(
    pass: bool,
    records: &Records,
    asks: &HeldAsks,
    answers: &HeldAnswers,
) -> Box<dyn Handler> {
    let records = Rc::clone(records);
    let asks = Rc::clone(asks);
    let answers = Rc::clone(answers);
    Box::new(FnHandler(move |_: &RawRequest, cx: &mut HandlerCx<'_>| {
        let list: Vec<TypedRecord> = records.borrow_mut().drain(..).collect();
        if !list.is_empty() {
            assert!(matches!(cx.commit(&list), Ok(c) if c.stored == list.len() as u64));
        }
        for (kind, workflow) in asks.borrow().iter() {
            let id = WorkflowId::parse(workflow).unwrap();
            answers.borrow_mut().push(cx.held_ahead(kind, &id));
        }
        if pass {
            cx.pass_on()?;
        }
        Ok(reply("null"))
    }))
}

/// A store with two workflows, the kinds of the family `test` and `front_mark`, and one handler of the family `test`
/// for `coordinator_write` bodies of the op `held`, for `runner` requests and for the further role `test_role`.
struct Held {
    store: Store,
    clock: TestClock,
    first: String,
    second: String,
    records: Records,
    asks: HeldAsks,
    answers: HeldAnswers,
    reports: Rc<RefCell<usize>>,
    scratch: Scratch,
}

const HELD: &str = r#"{"op":"held"}"#;

impl Held {
    fn new(name: &str) -> Held {
        let scratch = Scratch::new(name);
        let clock = TestClock::at_day(DAY);
        let (first, second) = two_workflows(&scratch, &clock);
        let records: Records = Rc::new(RefCell::new(Vec::new()));
        let asks: HeldAsks = Rc::new(RefCell::new(Vec::new()));
        let answers: HeldAnswers = Rc::new(RefCell::new(Vec::new()));
        let reports = Rc::new(RefCell::new(0));
        let sink = Rc::clone(&reports);
        let host = base(&clock, Grouping { min: 1 })
            .handler(
                "test",
                Takes::Coordinator(&["held"]),
                held_handler(false, &records, &asks, &answers),
            )
            .handler(
                "test",
                Takes::Runner,
                held_handler(false, &records, &asks, &answers),
            )
            .handler(
                "test",
                Takes::Role("test_role"),
                held_handler(false, &records, &asks, &answers),
            )
            .on_report(Box::new(move |_| *sink.borrow_mut() += 1));
        let store = open_entries(&scratch, host, &[&NOTE, &TRACE, &OWN, &FRONT, &PLAIN]).unwrap();
        Held {
            store,
            clock,
            first,
            second,
            records,
            asks,
            answers,
            reports,
            scratch,
        }
    }

    /// Commits `records` under a `coordinator_write` with the clock on `day`, then sets the clock to `at`.
    fn store_on(&mut self, day: u32, records: Vec<TypedRecord>, at: i64) {
        self.clock.set_day(day);
        *self.records.borrow_mut() = records;
        self.store
            .handle(&raw_request("coordinator_write", HELD))
            .unwrap();
        self.clock.set(at);
    }

    /// The answers of `asks` in one request of the role `name` with the body `body`.
    fn ask(
        &mut self,
        name: &str,
        body: &str,
        asks: Vec<(&'static str, String)>,
    ) -> Vec<Result<bool, HeldRefusal>> {
        *self.asks.borrow_mut() = asks;
        self.answers.borrow_mut().clear();
        self.store.handle(&raw_request(name, body)).unwrap();
        self.answers.borrow().clone()
    }

    fn held(&mut self, kind: &'static str, workflow: &str) -> Result<bool, HeldRefusal> {
        let w = workflow.to_owned();
        self.ask("coordinator_write", HELD, vec![(kind, w)])
            .remove(0)
    }
}

fn noon(day: u32) -> i64 {
    i64::from(day) * common::DAY_NS + NOON
}

/// S-59 (a) 1 and 2: no while the only row of the kind and workflow lies two days after the clock's day, yes once one
/// lies three days after it, and the expiry at that clock removes exactly the row that made the yes.
#[test]
fn held_ahead_is_no_at_two_days_ahead_and_yes_at_three_and_the_expiry_removes_the_row_of_the_yes() {
    let mut fx = Held::new("held-two-three");
    let w1 = fx.first.clone();
    fx.store_on(DAY + 2, vec![note(&w1, DAY + 2, 6)], noon(DAY));
    let two = fx.held("test_note", &w1);
    fx.store_on(DAY + 3, vec![note(&w1, DAY + 3, 7)], noon(DAY));
    let three = fx.held("test_note", &w1);
    let removed = fx.store.expire().unwrap();
    let after = fx.held("test_note", &w1);
    drop(fx.store);
    assert_eq!(
        (two, three, removed, after),
        (Ok(false), Ok(true), 1, Ok(false))
    );
    assert_eq!(
        kind_rows(&fx.scratch, "test_note"),
        [format!(
            r#"{{"v":1,"workflow":"{w1}","day":{},"n":6}}"#,
            DAY + 2
        )]
    );
}

/// S-59 (a) 6: the call is no read: at one clock it answers yes before `Store::expire` and no after it.
#[test]
fn held_ahead_is_yes_before_the_expiry_and_no_after_it_at_one_clock() {
    let mut fx = Held::new("held-expiry");
    let w1 = fx.first.clone();
    fx.store_on(DAY + 2, vec![note(&w1, DAY + 2, 1)], noon(DAY - 1));
    let before = fx.held("test_note", &w1);
    assert!(fx.store.expire().unwrap() >= 1);
    let after = fx.held("test_note", &w1);
    drop(fx.store);
    assert_eq!((before, after), (Ok(true), Ok(false)));
}

/// S-59 (a) 2: a row past its day of going, at a clock set forward with no expiry run, is no row ahead of the clock.
#[test]
fn held_ahead_is_no_for_a_row_past_its_day_of_going_at_a_clock_set_forward() {
    let mut fx = Held::new("held-past");
    let w1 = fx.first.clone();
    fx.store_on(DAY, vec![note(&w1, DAY - 92, 1)], noon(DAY + 5));
    let answer = fx.held("test_note", &w1);
    drop(fx.store);
    assert_eq!(answer, Ok(false));
    assert_eq!(kind_rows(&fx.scratch, "test_note").len(), 1);
}

/// S-59 (a) 1: after the delete of the workflow no row of it is ahead of the clock.
#[test]
fn held_ahead_is_no_after_the_delete_of_the_workflow() {
    let mut fx = Held::new("held-delete");
    let w1 = fx.first.clone();
    fx.store_on(DAY + 3, vec![note(&w1, DAY + 3, 1)], noon(DAY));
    let before = fx.held("test_note", &w1);
    delete_workflow(&mut fx.store, &w1);
    let after = fx.held("test_note", &w1);
    drop(fx.store);
    assert_eq!((before, after), (Ok(true), Ok(false)));
}

/// S-59 (a) 1 and 2: a row three days ahead answers for its kind and its workflow alone: no for the other workflow and
/// no for the other kinds of the family that have a workflow key.
#[test]
fn held_ahead_is_no_for_another_workflow_and_for_another_kind_of_the_family() {
    let mut fx = Held::new("held-other");
    let (w1, w2) = (fx.first.clone(), fx.second.clone());
    fx.store_on(DAY + 3, vec![note(&w1, DAY + 3, 1)], noon(DAY));
    let answers = fx.ask(
        "coordinator_write",
        HELD,
        vec![
            ("test_note", w1.clone()),
            ("test_note", w2),
            ("test_own", w1.clone()),
            ("test_trace", w1),
        ],
    );
    drop(fx.store);
    assert_eq!(answers, [Ok(true), Ok(false), Ok(false), Ok(false)]);
}

/// S-59 (a) 3 and 4: under a `runner` request and under a request of a further role the call answers `role`, while
/// under a `coordinator_write` the same handler code gets the yes.
#[test]
fn held_ahead_answers_role_under_a_runner_request_and_under_a_request_of_a_further_role() {
    let mut fx = Held::new("held-role");
    let w1 = fx.first.clone();
    fx.store_on(DAY + 3, vec![note(&w1, DAY + 3, 1)], noon(DAY));
    let ask = vec![("test_note", w1)];
    let answers = [
        fx.ask("runner", "{}", ask.clone()),
        fx.ask("test_role", "{}", ask.clone()),
        fx.ask("coordinator_write", HELD, ask),
    ];
    drop(fx.store);
    assert_eq!(
        answers,
        [
            vec![Err(HeldRefusal::Role)],
            vec![Err(HeldRefusal::Role)],
            vec![Ok(true)]
        ]
    );
}

/// S-59 (a) 3 and 4: a refusal, and never no, for a kind that the store was not opened with, a kind of another
/// family, a kind of the family `workflow` and a kind whose entry gives no workflow key; and a handler in front, of
/// the family `front`, that asks about a kind of the handler behind gets the refusal while the handler behind gets
/// its answer.
#[test]
fn held_ahead_refuses_the_kinds_that_it_cannot_answer_for_and_a_kind_of_the_handler_behind() {
    let mut fx = Held::new("held-refusals");
    let w1 = fx.first.clone();
    fx.store_on(
        DAY + 3,
        vec![note(&w1, DAY + 3, 1), plain(&w1, DAY + 3, 2)],
        noon(DAY),
    );
    let answers = fx.ask(
        "coordinator_write",
        HELD,
        vec![
            ("test_missing", w1.clone()),
            ("front_mark", w1.clone()),
            ("support", w1.clone()),
            ("test_plain", w1.clone()),
        ],
    );
    drop(fx.store);
    assert_eq!(
        answers,
        [
            Err(HeldRefusal::UnknownKind),
            Err(HeldRefusal::OtherFamily),
            Err(HeldRefusal::WorkflowRun),
            Err(HeldRefusal::NoWorkflowKey),
        ]
    );
    let scratch = Scratch::new("held-front");
    let clock = TestClock::at_day(DAY + 3);
    let (w1, _) = two_workflows(&scratch, &clock);
    let none: Records = Rc::new(RefCell::new(Vec::new()));
    let behind_records: Records = Rc::new(RefCell::new(vec![note(&w1, DAY + 3, 1)]));
    let asks: HeldAsks = Rc::new(RefCell::new(vec![("test_note", w1.clone())]));
    let front: HeldAnswers = Rc::new(RefCell::new(Vec::new()));
    let behind: HeldAnswers = Rc::new(RefCell::new(Vec::new()));
    let host = base(&clock, Grouping { min: 1 })
        .handler(
            "front",
            Takes::Coordinator(&["held"]),
            held_handler(true, &none, &asks, &front),
        )
        .handler(
            "test",
            Takes::Coordinator(&["held"]),
            held_handler(false, &behind_records, &asks, &behind),
        );
    let mut store = open_entries(&scratch, host, &[&NOTE, &FRONT]).unwrap();
    store
        .handle(&raw_request("coordinator_write", HELD))
        .unwrap();
    front.borrow_mut().clear();
    behind.borrow_mut().clear();
    clock.set(noon(DAY));
    store
        .handle(&raw_request("coordinator_write", HELD))
        .unwrap();
    drop(store);
    assert_eq!(
        (front.borrow().clone(), behind.borrow().clone()),
        (vec![Err(HeldRefusal::OtherFamily)], vec![Ok(true)]),
        "(the handler in front, the handler behind)"
    );
}

/// S-59 (a) 1 and 6: three calls change no byte of the database's files, raise no revision and give the host no
/// report.
#[test]
fn three_held_ahead_calls_change_no_file_no_revision_and_give_no_report() {
    let mut fx = Held::new("held-no-write");
    let w1 = fx.first.clone();
    fx.store_on(DAY + 3, vec![note(&w1, DAY + 3, 1)], noon(DAY));
    let files = |fx: &Held| -> Vec<Vec<u8>> {
        let db = fx.scratch.db();
        [db.clone(), db.with_extension("db-wal")]
            .iter()
            .map(|p| std::fs::read(p).unwrap_or_default())
            .collect()
    };
    let before = (
        files(&fx),
        fx.store.report().unwrap().revisions,
        *fx.reports.borrow(),
    );
    let answers = fx.ask(
        "coordinator_write",
        HELD,
        vec![
            ("test_note", w1.clone()),
            ("test_note", w1.clone()),
            ("test_note", w1),
        ],
    );
    let after = (
        files(&fx),
        fx.store.report().unwrap().revisions,
        *fx.reports.borrow(),
    );
    drop(fx.store);
    assert_eq!(answers, [Ok(true), Ok(true), Ok(true)]);
    assert!(
        before == after,
        "the files, the revisions and the count of reports"
    );
}

/// A host clock that moves on by one nanosecond after each reading and counts its readings.
#[derive(Clone)]
struct MovingClock {
    at: Rc<std::cell::Cell<i64>>,
    readings: Rc<std::cell::Cell<u32>>,
}

impl halvo_derived::Clock for MovingClock {
    fn now(&self) -> halvo_schema_core::record::Instant {
        self.readings.set(self.readings.get() + 1);
        let at = self.at.get();
        self.at.set(at + 1);
        halvo_schema_core::record::Instant(at)
    }

    fn day(&self, at: halvo_schema_core::record::Instant) -> u32 {
        u32::try_from(at.0.div_euclid(common::DAY_NS)).unwrap()
    }

    fn day_start(&self, day: u32) -> halvo_schema_core::record::Instant {
        halvo_schema_core::record::Instant(i64::from(day) * common::DAY_NS)
    }
}

/// S-59 (a) 1 and 8: the call answers by the one reading of the host's clock that the handled request took at its
/// start. A request that starts in the last nanosecond of DAY, while a row lies on DAY + 3, gets the yes although
/// the clock has passed into DAY + 1, where the row lies two days ahead, before the call is made; the request reads
/// the clock once.
#[test]
fn held_ahead_answers_by_the_one_reading_of_its_request() {
    let scratch = Scratch::new("held-one-reading");
    let (w1, _) = two_workflows(&scratch, &TestClock::at_day(DAY));
    let clock = MovingClock {
        at: Rc::new(std::cell::Cell::new(noon(DAY + 3))),
        readings: Rc::new(std::cell::Cell::new(0)),
    };
    let records: Records = Rc::new(RefCell::new(vec![note(&w1, DAY + 3, 1)]));
    let asks: HeldAsks = Rc::new(RefCell::new(Vec::new()));
    let answers: HeldAnswers = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 })).handler(
        "test",
        Takes::Coordinator(&["held"]),
        held_handler(false, &records, &asks, &answers),
    );
    let mut store = open_entries(&scratch, host, &[&NOTE]).unwrap();
    store
        .handle(&raw_request("coordinator_write", HELD))
        .unwrap();
    *asks.borrow_mut() = vec![("test_note", w1)];
    clock.at.set(i64::from(DAY + 1) * common::DAY_NS - 1);
    clock.readings.set(0);
    store
        .handle(&raw_request("coordinator_write", HELD))
        .unwrap();
    let readings = clock.readings.get();
    drop(store);
    assert_eq!(
        (answers.borrow().clone(), readings),
        (vec![Ok(true)], 1),
        "(the answer, the readings of the clock in the request)"
    );
}

/// S-66 E1, S-59 (a) 3: `Role` comes before every refusal of a kind. Under a `runner` request and under a request of a
/// further role the call asked about a kind that the store was not opened with, a kind of another family, a kind of
/// the family `workflow` and a kind whose entry gives no workflow key answers `Role` each time.
#[test]
fn held_ahead_answers_role_before_every_refusal_of_a_kind() {
    let mut fx = Held::new("held-role-first");
    let w1 = fx.first.clone();
    let ask = vec![
        ("test_missing", w1.clone()),
        ("front_mark", w1.clone()),
        ("support", w1.clone()),
        ("test_plain", w1),
    ];
    let answers = [
        fx.ask("runner", "{}", ask.clone()),
        fx.ask("test_role", "{}", ask),
    ];
    drop(fx.store);
    let four = vec![Err(HeldRefusal::Role); 4];
    assert_eq!(answers, [four.clone(), four]);
}

/// S-66 E2: the kinds of the family `workflow`, those that give no workflow key (`summary`, `evidence`) as well as
/// those that give one (`workflow`, `support`), answer `WorkflowRun`, which the doc of `held_ahead` puts before the
/// check of a missing key.
#[test]
fn held_ahead_answers_workflow_run_for_every_kind_of_the_workflow_run_with_a_key_or_without() {
    let mut fx = Held::new("held-workflow-kinds");
    let w1 = fx.first.clone();
    let answers = fx.ask(
        "coordinator_write",
        HELD,
        vec![
            ("summary", w1.clone()),
            ("evidence", w1.clone()),
            ("workflow", w1.clone()),
            ("support", w1),
        ],
    );
    drop(fx.store);
    assert_eq!(answers, vec![Err(HeldRefusal::WorkflowRun); 4]);
}

/// A kind of the family `front` whose entry gives no workflow key.
static FRONT_PLAIN: KindEntry = KindEntry {
    name: "front_plain",
    workflow_key: None,
    ..FRONT
};

/// S-66 E2: a kind of another family answers `OtherFamily` whether its entry gives a workflow key or not, since the
/// check of the family comes before the check of a missing key.
#[test]
fn held_ahead_answers_other_family_for_a_kind_of_another_family_with_a_key_or_without() {
    let scratch = Scratch::new("held-front-plain");
    let clock = TestClock::at_day(DAY);
    let (w1, _) = two_workflows(&scratch, &clock);
    let none: Records = Rc::new(RefCell::new(Vec::new()));
    let asks: HeldAsks = Rc::new(RefCell::new(vec![
        ("front_plain", w1.clone()),
        ("front_mark", w1),
    ]));
    let answers: HeldAnswers = Rc::new(RefCell::new(Vec::new()));
    let host = base(&clock, Grouping { min: 1 }).handler(
        "test",
        Takes::Coordinator(&["held"]),
        held_handler(false, &none, &asks, &answers),
    );
    let mut store = open_entries(&scratch, host, &[&NOTE, &FRONT, &FRONT_PLAIN]).unwrap();
    store
        .handle(&raw_request("coordinator_write", HELD))
        .unwrap();
    drop(store);
    assert_eq!(
        answers.borrow().clone(),
        vec![Err(HeldRefusal::OtherFamily), Err(HeldRefusal::OtherFamily)]
    );
}
