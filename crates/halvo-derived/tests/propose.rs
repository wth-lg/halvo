mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, count, domain, episode, open_host, position, store_id, summary_id, Fixed, Grouping,
    Recorder, Scratch, Seq, TestClock,
};
use halvo_derived::{Host, IdSource};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::WorkflowId;
use halvo_schema_core::record::{
    Confidence, Proposal, Recurrence, Support, Trigger, Workflow, WorkflowState,
};
use halvo_schema_core::site::AppId;

const DAY: u32 = 20_767;

fn workflows(scratch: &Scratch) -> Vec<Workflow> {
    bodies(scratch, "workflow")
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect()
}

fn supports(scratch: &Scratch) -> Vec<Support> {
    bodies(scratch, "support")
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect()
}

fn revision(scratch: &Scratch) -> i64 {
    count(scratch, "SELECT n FROM revisions WHERE family = 'workflow'")
}

fn minted(seq: &mut Seq) -> WorkflowId {
    let mut bytes = [0u8; 16];
    seq.fill(&mut bytes);
    WorkflowId::from_random(bytes)
}

fn proposal(category: SeedCategory, ids: &[u64]) -> Proposal {
    Proposal {
        category,
        site: None,
        app: AppId::parse("com.example.browser").unwrap(),
        trigger: Trigger::Manual,
        recurrence: Recurrence {
            count: 1,
            period: Some(31),
            first: DAY,
            last: DAY,
        },
        confidence: Confidence(50),
        summaries: ids.iter().map(|n| summary_id(*n)).collect(),
    }
}

#[test]
fn a_proposal_of_the_recurrence_step_is_stored_as_a_proposed_workflow_with_its_support_rows() {
    let scratch = Scratch::new("propose");
    let clock = TestClock::at_day(DAY);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 3 }));
    let mut store = open_host(&scratch, host).unwrap();
    for (n, day) in [(1, DAY - 62), (2, DAY - 31)] {
        let outcome = store
            .commit_batch(
                &store_id(1),
                &[episode(n, day, None)],
                position(n, 0),
                &mut Seq(100),
            )
            .unwrap();
        assert_eq!(outcome.proposals.stored, 0);
    }
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[episode(3, DAY, None)],
            position(3, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert_eq!(outcome.proposals.stored, 1);
    drop(store);
    let stored = workflows(&scratch);
    assert_eq!(stored.len(), 1);
    let Workflow::Full(workflow) = &stored[0] else {
        panic!("the workflow is dismissed");
    };
    assert_eq!(workflow.state, WorkflowState::Proposed);
    assert_eq!(workflow.id, minted(&mut Seq(100)));
    assert_eq!(workflow.agent, None);
    assert_eq!(workflow.recurrence.count, 3);
    let support = supports(&scratch);
    assert_eq!(
        support
            .iter()
            .map(|s| (s.summary.clone(), s.day, s.workflow == workflow.id))
            .collect::<Vec<_>>(),
        vec![
            (summary_id(1), DAY - 62, true),
            (summary_id(2), DAY - 31, true),
            (summary_id(3), DAY, true)
        ]
    );
}

#[test]
fn a_proposal_with_the_key_of_a_stored_workflow_updates_that_row_and_adds_none() {
    let scratch = Scratch::new("update");
    let clock = TestClock::at_day(DAY);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 3 }));
    let mut store = open_host(&scratch, host).unwrap();
    let three: Vec<_> = (1..=3)
        .map(|n| episode(n, DAY - 3 + n as u32, None))
        .collect();
    store
        .commit_batch(&store_id(1), &three, position(1, 0), &mut Seq(100))
        .unwrap();
    let mut other_app = episode(4, DAY, None);
    other_app.app = AppId::parse("com.example.other").unwrap();
    other_app.site = domain("northwind-power.test");
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[episode(5, DAY, None), other_app],
            position(2, 0),
            &mut Seq(200),
        )
        .unwrap();
    assert_eq!(
        (outcome.proposals.stored, outcome.proposals.updated),
        (0, 1)
    );
    drop(store);
    let stored = workflows(&scratch);
    assert_eq!(stored.len(), 1);
    let Workflow::Full(workflow) = &stored[0] else {
        panic!("the workflow is dismissed");
    };
    assert_eq!(workflow.id, minted(&mut Seq(100)));
    assert_eq!(workflow.recurrence.count, 4);
    assert_eq!(supports(&scratch).len(), 4);
    let before = revision(&scratch);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 3 }));
    let mut store = open_host(&scratch, host).unwrap();
    let outcome = store
        .commit_batch(&store_id(1), &[], position(3, 0), &mut Seq(300))
        .unwrap();
    assert_eq!(
        (outcome.proposals.updated, outcome.proposals.unchanged),
        (0, 1)
    );
    drop(store);
    assert_eq!(revision(&scratch), before);
}

#[test]
fn workflow_ids_are_minted_in_the_order_of_category_and_then_lowest_summary_id() {
    let scratch = Scratch::new("mint-order");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    let mut shop = episode(5, DAY, None);
    shop.category = SeedCategory::ShopOrder;
    let mut powered = episode(9, DAY, domain("a-power.test"));
    powered.category = SeedCategory::UtilityBillPayment;
    let mut a = proposal(SeedCategory::ShopOrder, &[5]);
    a.summaries = vec![summary_id(5)];
    let mut b = proposal(SeedCategory::UtilityBillPayment, &[9]);
    b.site = Some(halvo_schema_core::site::SiteValue::Domain {
        key: halvo_schema_core::site::SiteDomain::parse("a-power.test").unwrap(),
        source: halvo_schema_core::site::DomainSource::AddressField,
    });
    let c = proposal(SeedCategory::UtilityBillPayment, &[2]);
    *fixed.borrow_mut() = vec![a, b, c];
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[shop, powered, episode(2, DAY, None)],
            position(1, 0),
            &mut Seq(500),
        )
        .unwrap();
    assert_eq!(outcome.proposals.stored, 3);
    drop(store);
    let mut seq = Seq(500);
    let (first, second, third) = (minted(&mut seq), minted(&mut seq), minted(&mut seq));
    let by_summary = |n: u64| {
        supports(&scratch)
            .into_iter()
            .find(|s| s.summary == summary_id(n))
            .unwrap()
            .workflow
    };
    assert_eq!(by_summary(2), first);
    assert_eq!(by_summary(9), second);
    assert_eq!(by_summary(5), third);
}

#[test]
fn a_proposal_that_its_reader_would_refuse_or_that_names_no_stored_summary_is_not_stored() {
    let scratch = Scratch::new("refused-proposals");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    let base = proposal(SeedCategory::UtilityBillPayment, &[1]);
    let mut confident = base.clone();
    confident.confidence = Confidence(101);
    let mut backwards = base.clone();
    backwards.recurrence.first = DAY + 1;
    let mut empty = base.clone();
    empty.summaries.clear();
    let mut unknown = base.clone();
    unknown.summaries = vec![summary_id(1), summary_id(77)];
    let mut long = base.clone();
    long.recurrence.period = Some(94);
    let mut none = base.clone();
    none.recurrence.count = 0;
    let mut valid = proposal(SeedCategory::Email, &[1]);
    valid.summaries = vec![summary_id(1), summary_id(1)];
    *fixed.borrow_mut() = vec![confident, backwards, empty, unknown, long, none, valid];
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None)],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    assert_eq!(
        (outcome.proposals.refused, outcome.proposals.stored),
        (6, 1)
    );
    drop(store);
    assert_eq!(workflows(&scratch).len(), 1);
    assert_eq!(supports(&scratch).len(), 1);
}

#[test]
fn the_recurrence_step_reads_the_summaries_inside_their_bound_by_day_and_then_id() {
    let scratch = Scratch::new("summary-read");
    let clock = TestClock::at_day(DAY);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(
        Box::new(clock.clone()),
        Box::new(Recorder(Rc::clone(&seen))),
    );
    let mut store = open_host(&scratch, host).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[
                episode(3, DAY, None),
                episode(7, DAY - 1, None),
                episode(1, DAY, None),
                episode(5, DAY - 92, None),
            ],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    seen.borrow_mut().clear();
    clock.set_day(DAY + 1);
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(1))
        .unwrap();
    let id = |n: u64| summary_id(n).as_str().to_owned();
    assert_eq!(
        *seen.borrow(),
        vec![(DAY - 1, id(7)), (DAY, id(1)), (DAY, id(3))]
    );
}
