mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, count, episode, open_entries, position, raw_request, store_id, Grouping, Scratch, Seq,
    TestClock, FRONT, NOTE, NOTE_UNKEYED, OWN, TRACE,
};
use halvo_derived::{
    Cause, Committed, Error, Handler, HandlerCx, Host, OpenError, Refusal, Store, Takes,
};
use halvo_schema_core::kinds::KindEntry;
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
    let w = workflow(&scratch, &clock, &[&NOTE]);
    let missing = "3".repeat(32);
    let cases: Vec<(TypedRecord, fn(&Cause) -> bool)> = vec![
        (note(&missing, DAY, 1), |c| *c == Cause::NoWorkflow),
        (
            TypedRecord {
                kind: "test_unknown",
                json: "{}".to_owned(),
            },
            |c| *c == Cause::UnknownKind,
        ),
        (
            TypedRecord {
                kind: "summary",
                json: "{}".to_owned(),
            },
            |c| *c == Cause::WorkflowRun,
        ),
        (
            TypedRecord {
                kind: "agent_run",
                json: "{}".to_owned(),
            },
            |c| *c == Cause::OtherFamily,
        ),
        (
            TypedRecord {
                kind: "test_note",
                json: format!(r#"{{"v":1,"workflow":"{w}","day":{DAY}}}"#),
            },
            |c| *c == Cause::Record("test_note: n: missing_key".to_owned()),
        ),
    ];
    for (bad, cause) in cases {
        let records: Records = Rc::new(RefCell::new(vec![note(&w, DAY, 1), bad]));
        let mut store =
            open_entries(&scratch, with_step(&clock, &records, vec![]), &[&NOTE]).unwrap();
        let outcome = store
            .commit_batch(&store_id(1), &[], position(9, 0), &mut Seq(1))
            .unwrap();
        drop(store);
        match &outcome.steps[..] {
            [Err(Refusal { index: 1, cause: c })] if cause(c) => {}
            other => panic!("the step's list was answered {other:?}"),
        }
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
    let w = workflow(&scratch, &clock, &[&NOTE]);
    let records: Records = Rc::new(RefCell::new(vec![note(&w, DAY, 7), note(&w, DAY, 3)]));
    let mut store = open_entries(&scratch, with_step(&clock, &records, vec![]), &[&NOTE]).unwrap();
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
                "agent_run",
                "test_missing",
            ],
        }),
    );
    let mut store = open_entries(&scratch, host, &[&NOTE]).unwrap();
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
