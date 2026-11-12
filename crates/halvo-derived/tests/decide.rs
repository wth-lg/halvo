mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, count, domain, episode, open_host, position, raw, store_id, summary_id, Fixed,
    Grouping, Scratch, Seq, TestClock,
};
use halvo_derived::{Error, Host, Store};
use halvo_schema_core::ids::{RequestId, WorkflowId};
use halvo_schema_core::record::{Proposal, RecurrenceStep, Summary, Workflow, WorkflowState};
use halvo_schema_core::request::{ErrorName, InterfaceDecision, InterfaceRole};
use halvo_schema_core::site::AppId;

const DAY: u32 = 20_767;

fn workflows(scratch: &Scratch) -> Vec<Workflow> {
    bodies(scratch, "workflow")
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect()
}

fn revision(scratch: &Scratch, family: &str) -> i64 {
    count(
        scratch,
        &format!("SELECT n FROM revisions WHERE family = '{family}'"),
    )
}

fn request() -> RequestId {
    RequestId::from_random([9; 16])
}

fn approve(store: &mut Store, id: &WorkflowId) -> Result<(), Error> {
    store.decide(&InterfaceDecision::approve(
        &InterfaceRole::claim(),
        request(),
        id.clone(),
    ))
}

fn dismiss(store: &mut Store, id: &WorkflowId) -> Result<(), Error> {
    store.decide(&InterfaceDecision::dismiss(
        &InterfaceRole::claim(),
        request(),
        id.clone(),
    ))
}

fn delete(store: &mut Store, id: &WorkflowId) -> Result<(), Error> {
    store.decide(&InterfaceDecision::delete(
        &InterfaceRole::claim(),
        request(),
        id.clone(),
    ))
}

fn host(clock: &TestClock, step: impl RecurrenceStep + 'static) -> Host {
    Host::new(Box::new(clock.clone()), Box::new(step))
}

/// A store with one proposed workflow of three summaries, the key utility_bill_payment and the app
/// com.example.browser, and its id.
fn proposed(scratch: &Scratch, clock: &TestClock) -> (Store, WorkflowId) {
    let mut store = open_host(scratch, host(clock, Grouping { min: 3 })).unwrap();
    let three: Vec<_> = (1..=3).map(|n| episode(n, DAY, None)).collect();
    store
        .commit_batch(&store_id(1), &three, position(1, 0), &mut Seq(100))
        .unwrap();
    drop(store);
    let id = workflows(scratch)[0].id().clone();
    (
        open_host(scratch, host(clock, Grouping { min: 3 })).unwrap(),
        id,
    )
}

#[test]
fn approve_moves_a_proposed_workflow_to_approved_and_a_repeat_changes_nothing() {
    let scratch = Scratch::new("approve");
    let clock = TestClock::at_day(DAY);
    let (mut store, id) = proposed(&scratch, &clock);
    drop(store);
    let before = revision(&scratch, "workflow");
    store = open_host(&scratch, host(&clock, Grouping { min: 3 })).unwrap();
    assert_eq!(approve(&mut store, &id), Ok(()));
    drop(store);
    assert_eq!(revision(&scratch, "workflow"), before + 1);
    store = open_host(&scratch, host(&clock, Grouping { min: 3 })).unwrap();
    assert_eq!(approve(&mut store, &id), Ok(()));
    assert_eq!(
        dismiss(&mut store, &id),
        Err(Error::Refused(ErrorName::State))
    );
    let missing = WorkflowId::from_random([3; 16]);
    assert_eq!(
        approve(&mut store, &missing),
        Err(Error::Refused(ErrorName::NoWorkflow))
    );
    assert_eq!(
        dismiss(&mut store, &missing),
        Err(Error::Refused(ErrorName::NoWorkflow))
    );
    drop(store);
    assert_eq!(revision(&scratch, "workflow"), before + 1);
    let Workflow::Full(workflow) = &workflows(&scratch)[0] else {
        panic!("dismissed");
    };
    assert_eq!(workflow.state, WorkflowState::Approved);
    assert_eq!(bodies(&scratch, "support").len(), 3);
}

#[test]
fn dismiss_cuts_a_proposed_workflow_to_its_dismissed_shape_and_removes_its_support_rows() {
    let scratch = Scratch::new("dismiss");
    let clock = TestClock::at_day(DAY);
    let (mut store, id) = proposed(&scratch, &clock);
    assert_eq!(dismiss(&mut store, &id), Ok(()));
    assert_eq!(dismiss(&mut store, &id), Ok(()));
    assert_eq!(
        approve(&mut store, &id),
        Err(Error::Refused(ErrorName::State))
    );
    drop(store);
    let body = &bodies(&scratch, "workflow")[0];
    assert_eq!(
        body,
        &format!(
            r#"{{"v":1,"id":"{}","state":"dismissed","category":"utility_bill_payment","site":null,"app":"com.example.browser"}}"#,
            id.as_str()
        )
    );
    assert_eq!(bodies(&scratch, "support").len(), 0);
    assert_eq!(bodies(&scratch, "summary").len(), 3);
}

#[test]
fn while_a_dismissed_workflow_is_stored_no_proposal_with_its_key_is_stored() {
    let scratch = Scratch::new("dismissal-key");
    let clock = TestClock::at_day(DAY);
    let mut store = open_host(&scratch, host(&clock, Grouping { min: 2 })).unwrap();
    let mut episodes = Vec::new();
    for n in 1..=2 {
        episodes.push(episode(n, DAY, None));
        episodes.push(episode(10 + n, DAY, domain("northwind-power.test")));
    }
    store
        .commit_batch(&store_id(1), &episodes, position(1, 0), &mut Seq(100))
        .unwrap();
    drop(store);
    let ids: Vec<WorkflowId> = workflows(&scratch).iter().map(|w| w.id().clone()).collect();
    assert_eq!(ids.len(), 2);
    let mut store = open_host(&scratch, host(&clock, Grouping { min: 2 })).unwrap();
    for id in &ids {
        dismiss(&mut store, id).unwrap();
    }
    let mut other_app = episode(21, DAY, None);
    other_app.app = AppId::parse("com.example.other").unwrap();
    let mut other_app_on_site = episode(22, DAY, domain("northwind-power.test"));
    other_app_on_site.app = AppId::parse("com.example.other").unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[
                episode(3, DAY, None),
                episode(13, DAY, domain("northwind-power.test")),
                other_app,
                other_app_on_site,
                episode(23, DAY, None),
            ],
            position(2, 0),
            &mut Seq(200),
        )
        .unwrap();
    assert_eq!(
        (outcome.proposals.dismissed, outcome.proposals.stored),
        (2, 0)
    );
    let mut third_app = episode(31, DAY, None);
    third_app.app = AppId::parse("com.example.third").unwrap();
    let mut third_again = episode(32, DAY, None);
    third_again.app = AppId::parse("com.example.third").unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[third_app, third_again],
            position(3, 0),
            &mut Seq(300),
        )
        .unwrap();
    assert_eq!(
        (outcome.proposals.dismissed, outcome.proposals.stored),
        (2, 1)
    );
    drop(store);
    let stored = workflows(&scratch);
    assert_eq!(stored.len(), 3);
    assert_eq!(
        stored
            .iter()
            .filter(|w| matches!(w, Workflow::Dismissed(_)))
            .count(),
        2
    );
}

#[test]
fn delete_removes_every_row_that_names_the_workflow_and_every_summary_of_its_key() {
    let scratch = Scratch::new("delete");
    let clock = TestClock::at_day(DAY);
    let fixed: Rc<RefCell<Vec<Proposal>>> = Rc::new(RefCell::new(Vec::new()));
    let mut store = open_host(&scratch, host(&clock, Fixed(Rc::clone(&fixed)))).unwrap();
    let mut other = episode(9, DAY, domain("northwind-power.test"));
    other.app = AppId::parse("com.example.other").unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, DAY, None),
                episode(2, DAY, None),
                episode(3, DAY, None),
                other,
            ],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    let mut w = common_proposal(&[1, 2]);
    w.app = AppId::parse("com.example.browser").unwrap();
    let mut x = common_proposal(&[9, 3]);
    x.site = Some(halvo_schema_core::site::SiteValue::Domain {
        key: halvo_schema_core::site::SiteDomain::parse("northwind-power.test").unwrap(),
        source: halvo_schema_core::site::DomainSource::AddressField,
    });
    *fixed.borrow_mut() = vec![w, x];
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(100))
        .unwrap();
    fixed.borrow_mut().clear();
    drop(store);
    let stored = workflows(&scratch);
    let w_id = stored
        .iter()
        .find(|w| matches!(w, Workflow::Full(f) if f.site.is_none()))
        .unwrap()
        .id()
        .clone();
    let x_id = stored
        .iter()
        .find(|w| w.id() != &w_id)
        .unwrap()
        .id()
        .clone();
    raw(&scratch)
        .execute(
            "INSERT INTO rows (kind, family, class, day, workflow, body) VALUES ('agent_run', 'agent', 1, ?1, ?2, '{}')",
            (DAY, w_id.as_str()),
        )
        .unwrap();
    let before = (revision(&scratch, "workflow"), revision(&scratch, "agent"));
    let mut store = open_host(&scratch, host(&clock, Grouping { min: 1 })).unwrap();
    assert_eq!(delete(&mut store, &w_id), Ok(()));
    assert_eq!(delete(&mut store, &w_id), Ok(()));
    let outcome = store
        .commit_batch(&store_id(1), &[], position(3, 0), &mut Seq(300))
        .unwrap();
    assert_eq!(outcome.proposals.stored, 0);
    drop(store);
    let naming = count(
        &scratch,
        &format!(
            "SELECT count(*) FROM rows WHERE workflow = '{0}' OR instr(body, '{0}') > 0",
            w_id.as_str()
        ),
    );
    assert_eq!(naming, 0);
    let left: Vec<String> = bodies(&scratch, "summary")
        .iter()
        .map(|b| {
            serde_json::from_str::<Summary>(b)
                .unwrap()
                .id
                .as_str()
                .to_owned()
        })
        .collect();
    assert_eq!(left, vec![summary_id(9).as_str().to_owned()]);
    assert_eq!(bodies(&scratch, "evidence").len(), 1);
    let x_support: Vec<String> = bodies(&scratch, "support");
    assert_eq!(x_support.len(), 1);
    assert!(x_support[0].contains(x_id.as_str()) && x_support[0].contains(summary_id(9).as_str()));
    assert_eq!(
        (revision(&scratch, "workflow"), revision(&scratch, "agent")),
        (before.0 + 2, before.1 + 1)
    );
}

#[test]
fn delete_of_a_dismissed_workflow_removes_the_summaries_of_its_key() {
    let scratch = Scratch::new("delete-dismissed");
    let clock = TestClock::at_day(DAY);
    let (mut store, id) = proposed(&scratch, &clock);
    dismiss(&mut store, &id).unwrap();
    assert_eq!(delete(&mut store, &id), Ok(()));
    drop(store);
    assert_eq!(workflows(&scratch).len(), 0);
    assert_eq!(bodies(&scratch, "summary").len(), 0);
    assert_eq!(bodies(&scratch, "evidence").len(), 0);
}

fn common_proposal(ids: &[u64]) -> Proposal {
    Proposal {
        category: halvo_schema_core::category::SeedCategory::UtilityBillPayment,
        site: None,
        app: AppId::parse("com.example.other").unwrap(),
        trigger: halvo_schema_core::record::Trigger::Manual,
        recurrence: halvo_schema_core::record::Recurrence {
            count: ids.len() as u32,
            period: None,
            first: DAY,
            last: DAY,
        },
        confidence: halvo_schema_core::record::Confidence(70),
        summaries: ids.iter().map(|n| summary_id(*n)).collect(),
    }
}

#[test]
fn delete_removes_each_summary_of_the_support_set_whatever_its_key() {
    let scratch = Scratch::new("delete-support-set");
    let clock = TestClock::at_day(DAY);
    let mut signed = common_proposal(&[1, 2]);
    signed.site = Some(halvo_schema_core::site::SiteValue::Domain {
        key: halvo_schema_core::site::SiteDomain::parse("northwind-power.test").unwrap(),
        source: halvo_schema_core::site::DomainSource::SignIn,
    });
    let fixed: Rc<RefCell<Vec<Proposal>>> = Rc::new(RefCell::new(vec![signed]));
    let mut store = open_host(&scratch, host(&clock, Fixed(Rc::clone(&fixed)))).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, DAY, domain("northwind-power.test")),
                episode(2, DAY, domain("northwind-power.test")),
                episode(3, DAY, None),
            ],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    fixed.borrow_mut().clear();
    let id = {
        drop(store);
        workflows(&scratch)[0].id().clone()
    };
    let mut store = open_host(&scratch, host(&clock, Fixed(Rc::clone(&fixed)))).unwrap();
    assert_eq!(delete(&mut store, &id), Ok(()));
    drop(store);
    let left: Vec<String> = bodies(&scratch, "summary")
        .iter()
        .map(|b| {
            serde_json::from_str::<Summary>(b)
                .unwrap()
                .id
                .as_str()
                .to_owned()
        })
        .collect();
    assert_eq!(left, vec![summary_id(3).as_str().to_owned()]);
    assert_eq!(bodies(&scratch, "evidence").len(), 1);
    assert_eq!(bodies(&scratch, "support").len(), 0);
}
