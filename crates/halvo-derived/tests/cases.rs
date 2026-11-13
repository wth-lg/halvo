mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, domain, episode, key, listed, open_host, own_note_body, position, raw, raw_request,
    registry, store_id, summary_id, Fixed, Grouping, OwnNote, Scratch, Seq, TestClock, DAY_NS, OWN,
};
use halvo_derived::{Cause, Error, Host, Refusal, Store, Takes};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::RequestId;
use halvo_schema_core::record::{
    Confidence, Instant, Proposal, Recurrence, RowRead, SummaryRead, Trigger, TypedRecord,
    Workflow, WorkflowState, WriterStep,
};
use halvo_schema_core::request::{ErrorName, InterfaceDecision, InterfaceRole};
use halvo_schema_core::site::{AppId, DomainSource, SiteDomain, SiteValue};

const DAY: u32 = 20_767;

fn count(scratch: &Scratch, sql: &str) -> i64 {
    raw(scratch).query_row(sql, [], |r| r.get(0)).unwrap()
}

/// The second test step of own_origin_refused: after a commit it returns one record of the kind of origin own
/// `test_own` for the workflow that the test put in its cell, and nothing while the cell is empty.
struct OwnStep(Rc<RefCell<Option<String>>>);

impl WriterStep for OwnStep {
    fn after_commit(&self, _summaries: &dyn SummaryRead, _rows: &dyn RowRead) -> Vec<TypedRecord> {
        match &*self.0.borrow() {
            None => Vec::new(),
            Some(workflow) => vec![TypedRecord {
                kind: "test_own",
                json: format!(
                    r#"{{"v":1,"workflow":"{workflow}","note":"Call the landlord","at":1794339000000000000}}"#
                ),
            }],
        }
    }
}

fn own_host(clock: &TestClock, step: &Rc<RefCell<Option<String>>>) -> Host {
    Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 }))
        .step("test", Box::new(OwnStep(Rc::clone(step))))
        .handler("test", Takes::Runner, Box::new(OwnNote))
        .handler("test", Takes::Coordinator(&["own_note"]), Box::new(OwnNote))
        .handler("test", Takes::Role("test_role"), Box::new(OwnNote))
}

fn open_own(scratch: &Scratch, clock: &TestClock, step: &Rc<RefCell<Option<String>>>) -> Store {
    let mut entries = registry();
    entries.push(&OWN);
    Store::open(
        scratch.path(),
        &key(),
        &entries,
        own_host(clock, step),
        &mut Seq(1 << 40),
    )
    .unwrap()
}

const OWN_ROWS: &str = "SELECT count(*) FROM rows WHERE kind = 'test_own'";

/// own_origin_refused: with a second test-only kind, of origin own, with a workflow key and a field of the type
/// own_text, the store refuses one record of that kind, and stores no row of it, when the second test step returns it
/// after a commit and when a test handler commits it for a request of a further role; it stores the record when a test
/// handler commits it for a `runner` request and for a `coordinator_write`; and after the delete of the workflow that
/// the records name no row of that kind is left (contracts.md C25.8).
#[test]
fn own_origin_refused() {
    let scratch = Scratch::new("own-origin");
    let clock = TestClock::at_day(DAY);
    let step = Rc::new(RefCell::new(None));
    let mut store = open_own(&scratch, &clock, &step);
    let first = store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert_eq!(first.proposals.stored, 1);
    let workflow = listed(&store)[0].workflow.id().clone();
    *step.borrow_mut() = Some(workflow.as_str().to_owned());
    let by_step = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    assert_eq!(
        by_step.steps,
        vec![Err(Refusal {
            index: 0,
            cause: Cause::OwnOrigin
        })],
        "the record of origin own that the second step returns"
    );
    *step.borrow_mut() = None;
    let body = own_note_body(workflow.as_str(), "Call the landlord");
    assert_eq!(
        store.handle(&raw_request("test_role", &body)).err(),
        Some(Error::Refused(ErrorName::Role)),
        "the record of origin own that a handler of a further role commits"
    );
    drop(store);
    assert_eq!(
        count(&scratch, OWN_ROWS),
        0,
        "rows of origin own after the two refusals"
    );
    let mut store = open_own(&scratch, &clock, &step);
    store.handle(&raw_request("runner", &body)).unwrap();
    store
        .handle(&raw_request("coordinator_write", &body))
        .unwrap();
    drop(store);
    assert_eq!(
        count(&scratch, OWN_ROWS),
        2,
        "rows of origin own for a runner request and a coordinator_write"
    );
    let mut store = open_own(&scratch, &clock, &step);
    store
        .decide(&InterfaceDecision::delete(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            workflow,
        ))
        .unwrap();
    drop(store);
    assert_eq!(
        count(&scratch, OWN_ROWS),
        0,
        "rows of origin own after the delete of their workflow"
    );
}

fn northwind() -> SiteValue {
    SiteValue::Domain {
        key: SiteDomain::parse("northwind-power.test").unwrap(),
        source: DomainSource::AddressField,
    }
}

fn proposal(app: &str, trigger: Trigger, confidence: u32, summaries: &[u64]) -> Proposal {
    Proposal {
        category: SeedCategory::UtilityBillPayment,
        site: Some(northwind()),
        app: AppId::parse(app).unwrap(),
        trigger,
        recurrence: Recurrence {
            count: summaries.len() as u32,
            period: None,
            first: DAY - 1,
            last: DAY,
        },
        confidence: Confidence(confidence),
        summaries: summaries.iter().map(|n| summary_id(*n)).collect(),
    }
}

/// The case of a proposal for a stored workflow: an approved workflow, whose agent is null, takes a later proposal of
/// its key and keeps its id, its state `approved` and its agent, and takes the proposal's app, trigger, recurrence and
/// confidence, and its support rows become those of the proposal's summaries; no second workflow is stored. The row of
/// C2.4 for a workflow whose agent is not null is U5.1d's.
#[test]
fn a_proposal_for_a_stored_workflow_keeps_its_id_state_and_agent_and_takes_the_rest() {
    let scratch = Scratch::new("stored-proposal");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(vec![proposal(
        "com.example.browser",
        Trigger::Schedule,
        60,
        &[1, 2],
    )]));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    let batch = [
        episode(1, DAY - 1, domain("northwind-power.test")),
        episode(2, DAY, domain("northwind-power.test")),
    ];
    let first = store
        .commit_batch(&store_id(1), &batch, position(1, 0), &mut Seq(100))
        .unwrap();
    assert_eq!(first.proposals.stored, 1);
    let workflow = listed(&store)[0].workflow.id().clone();
    store
        .decide(&InterfaceDecision::approve(
            &InterfaceRole::claim(),
            RequestId::from_random([2; 16]),
            workflow.clone(),
        ))
        .unwrap();
    *fixed.borrow_mut() = vec![proposal(
        "com.example.other",
        Trigger::AppContext,
        90,
        &[1, 2, 3],
    )];
    let mut other_app = episode(3, DAY, domain("northwind-power.test"));
    other_app.app = AppId::parse("com.example.other").unwrap();
    let second = store
        .commit_batch(&store_id(1), &[other_app], position(2, 0), &mut Seq(200))
        .unwrap();
    assert_eq!(
        (second.proposals.stored, second.proposals.updated),
        (0, 1),
        "(stored, updated) of the later proposal"
    );
    drop(store);
    let bodies = bodies(&scratch, "workflow");
    assert_eq!(bodies.len(), 1, "stored workflows");
    let Workflow::Full(stored) = serde_json::from_str(&bodies[0]).unwrap() else {
        panic!("the workflow is dismissed");
    };
    assert_eq!(
        (&stored.id, stored.state, &stored.agent),
        (&workflow, WorkflowState::Approved, &None),
        "the id, the state and the agent that the row keeps"
    );
    assert_eq!(
        (
            stored.app.as_str(),
            stored.trigger,
            stored.recurrence,
            stored.confidence
        ),
        (
            "com.example.other",
            Trigger::AppContext,
            Recurrence {
                count: 3,
                period: None,
                first: DAY - 1,
                last: DAY
            },
            Confidence(90)
        ),
        "the app, the trigger, the recurrence and the confidence that the row takes"
    );
    assert_eq!(
        (stored.category, stored.site),
        (SeedCategory::UtilityBillPayment, Some(northwind()))
    );
    let support: Vec<String> = bodies_of_support(&scratch);
    let want: Vec<String> = [1, 2, 3]
        .iter()
        .map(|n| summary_id(*n).as_str().to_owned())
        .collect();
    assert_eq!(support, want, "the summaries of the support rows");
}

fn bodies_of_support(scratch: &Scratch) -> Vec<String> {
    let mut summaries: Vec<String> = bodies(scratch, "support")
        .iter()
        .map(|body| {
            let record: serde_json::Value = serde_json::from_str(body).unwrap();
            record["summary"].as_str().unwrap().to_owned()
        })
        .collect();
    summaries.sort();
    summaries
}

/// The cross-window case: a routine planted on days 1, 31 and 61, one episode per batch with an observation window of
/// 14 days, with the expiry run on the injected clock after each batch, yields no proposal on days 1 and 31 and its
/// proposal on day 61 from the test proposer, supported by the three summaries, while the expiry has removed the
/// evidence of days 1 and 31.
#[test]
fn a_routine_on_days_1_31_and_61_yields_its_proposal_on_day_61_across_the_window() {
    let scratch = Scratch::new("cross-window");
    let clock = TestClock::at_day(DAY);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 3 }));
    let mut store = open_host(&scratch, host).unwrap();
    let mut seen = Vec::new();
    for (at, n) in [1u32, 31, 61].into_iter().zip(1u64..) {
        clock.set_day(DAY + at);
        let mut planted = episode(n, DAY + at, None);
        planted.expires = Instant(i64::from(DAY + at + 14) * DAY_NS);
        let outcome = store
            .commit_batch(&store_id(1), &[planted], position(n, 0), &mut Seq(100 * n))
            .unwrap();
        let expired = store.expire().unwrap();
        seen.push((outcome.proposals.stored, expired));
    }
    assert_eq!(
        seen,
        vec![(0, 0), (0, 1), (1, 1)],
        "(proposals stored, rows expired) on days 1, 31 and 61"
    );
    drop(store);
    assert_eq!(
        count(
            &scratch,
            "SELECT count(*) FROM rows WHERE kind = 'evidence'"
        ),
        1
    );
    assert_eq!(
        count(&scratch, "SELECT count(*) FROM rows WHERE kind = 'summary'"),
        3
    );
    let bodies = bodies(&scratch, "workflow");
    assert_eq!(bodies.len(), 1);
    let Workflow::Full(stored) = serde_json::from_str(&bodies[0]).unwrap() else {
        panic!("the workflow is dismissed");
    };
    assert_eq!(
        stored.recurrence,
        Recurrence {
            count: 3,
            period: None,
            first: DAY + 1,
            last: DAY + 61
        }
    );
    let want: Vec<String> = [1, 2, 3]
        .iter()
        .map(|n| summary_id(*n).as_str().to_owned())
        .collect();
    assert_eq!(
        bodies_of_support(&scratch),
        want,
        "the summaries of the support rows"
    );
}
