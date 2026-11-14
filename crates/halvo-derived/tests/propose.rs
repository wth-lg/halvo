mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, count, domain, episode, open_host, position, raw_request, store_id, summary_id, token,
    Fixed, Grouping, Recorder, Scratch, Seq, SignInGroup, TestClock,
};
use halvo_derived::{
    Error, Handler, HandlerCx, Host, IdSource, MoveTo, ProposalOutcome, Store, Takes,
};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{AgentId, RequestId, WorkflowId};
use halvo_schema_core::record::{
    Confidence, Episode, Proposal, Recurrence, RecurrenceStep, SummaryRead, Support, Trigger,
    Workflow, WorkflowState,
};
use halvo_schema_core::request::{
    ErrorName, InterfaceDecision, InterfaceRead, InterfaceRole, RawRequest, ReplyResult,
};
use halvo_schema_core::site::{AppId, DomainSource, ObservedSite, SiteDomain, SiteValue};
use serde_json::value::RawValue;

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
    let mut valid = proposal(SeedCategory::Email, &[2]);
    valid.summaries = vec![summary_id(2), summary_id(2)];
    let mut email = episode(2, DAY, None);
    email.category = SeedCategory::Email;
    *fixed.borrow_mut() = vec![confident, backwards, empty, unknown, long, none, valid];
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None), email],
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

fn keyed(category: SeedCategory, site: Option<SiteValue>, app: &str, ids: &[u64]) -> Proposal {
    Proposal {
        category,
        site,
        app: AppId::parse(app).unwrap(),
        trigger: Trigger::Manual,
        recurrence: Recurrence {
            count: ids.len() as u32,
            period: None,
            first: DAY,
            last: DAY,
        },
        confidence: Confidence(50),
        summaries: ids.iter().map(|n| summary_id(*n)).collect(),
    }
}

fn address(name: &str) -> Option<SiteValue> {
    Some(SiteValue::Domain {
        key: SiteDomain::parse(name).unwrap(),
        source: DomainSource::AddressField,
    })
}

/// Commits `episodes` in one batch under `step` and returns what the store did with the step's proposals, with the
/// number of workflow rows and of support rows that stand after it.
fn proposed_over(
    name: &str,
    episodes: Vec<Episode>,
    step: impl RecurrenceStep + 'static,
) -> (ProposalOutcome, usize, usize) {
    let scratch = Scratch::new(name);
    let clock = TestClock::at_day(DAY);
    let mut store = open_host(&scratch, Host::new(Box::new(clock), Box::new(step))).unwrap();
    let outcome = store
        .commit_batch(&store_id(1), &episodes, position(1, 0), &mut Seq(100))
        .unwrap();
    drop(store);
    (
        outcome.proposals,
        workflows(&scratch).len(),
        supports(&scratch).len(),
    )
}

fn fixed(proposal: Proposal) -> Fixed {
    Fixed(Rc::new(RefCell::new(vec![proposal])))
}

#[test]
fn a_proposal_that_names_a_summary_of_another_key_is_refused_and_stores_nothing() {
    let other_app = |n: u64, site: Option<ObservedSite>| {
        let mut e = episode(n, DAY, site);
        e.app = AppId::parse("com.example.other").unwrap();
        e
    };
    let browser = "com.example.browser";
    let cases: Vec<(&str, Vec<Episode>, Box<dyn RecurrenceStep>)> = vec![
        (
            "the-other-source-of-one-domain",
            vec![
                episode(1, DAY, domain("northwind-power.test")),
                episode(2, DAY, domain("northwind-power.test")),
            ],
            Box::new(SignInGroup { min: 2 }),
        ),
        (
            "another-domain",
            vec![episode(1, DAY, domain("a-power.test"))],
            Box::new(fixed(keyed(
                SeedCategory::UtilityBillPayment,
                address("b-power.test"),
                browser,
                &[1],
            ))),
        ),
        (
            "another-category",
            vec![episode(1, DAY, None)],
            Box::new(fixed(keyed(SeedCategory::ShopOrder, None, browser, &[1]))),
        ),
        (
            "a-title-key-against-a-domain-key",
            vec![episode(1, DAY, token("northwind-power"))],
            Box::new(fixed(keyed(
                SeedCategory::UtilityBillPayment,
                address("northwind-power.test"),
                browser,
                &[1],
            ))),
        ),
        (
            "an-app-key-against-a-site-key",
            vec![episode(1, DAY, domain("northwind-power.test"))],
            Box::new(fixed(keyed(
                SeedCategory::UtilityBillPayment,
                None,
                browser,
                &[1],
            ))),
        ),
        (
            "another-app-under-an-app-key",
            vec![other_app(1, None)],
            Box::new(fixed(keyed(
                SeedCategory::UtilityBillPayment,
                None,
                browser,
                &[1],
            ))),
        ),
    ];
    let mut seen = Vec::new();
    for (name, episodes, step) in cases {
        let (outcome, workflows, supports) = proposed_over(name, episodes, BoxedStep(step));
        seen.push((name, (outcome.refused, outcome.stored, workflows, supports)));
    }
    let want: Vec<_> = seen.iter().map(|(name, _)| (*name, (1, 0, 0, 0))).collect();
    assert_eq!(
        seen, want,
        "(refused, stored, workflow rows, support rows) per case"
    );
    let (outcome, workflows, supports) = proposed_over(
        "one-site-key-from-two-apps",
        vec![
            episode(1, DAY, domain("shop.test")),
            other_app(2, domain("shop.test")),
        ],
        fixed(keyed(
            SeedCategory::UtilityBillPayment,
            address("shop.test"),
            "com.example.other",
            &[1, 2],
        )),
    );
    assert_eq!(
        (outcome.refused, outcome.stored, workflows, supports),
        (0, 1, 1, 2),
        "{outcome:?}"
    );
}

struct BoxedStep(Box<dyn RecurrenceStep>);

impl RecurrenceStep for BoxedStep {
    fn propose(&self, summaries: &dyn SummaryRead) -> Vec<Proposal> {
        self.0.propose(summaries)
    }
}

/// A proposal of the category of `episode` and its key (no site, the browser app) over `ids`.
fn over(ids: &[u64]) -> Proposal {
    let mut p = proposal(SeedCategory::UtilityBillPayment, ids);
    p.recurrence.count = ids.len() as u32;
    p
}

/// A handler that makes the state call that its body names:
/// `{"op":"set_state","workflow":<id>,"state":"approved"|"active"|"paused","agent":<id>|null}`.
struct SetState;

impl Handler for SetState {
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let body: serde_json::Value = serde_json::from_str(request.body().get()).unwrap();
        let workflow = WorkflowId::parse(body["workflow"].as_str().unwrap()).unwrap();
        let to = match body["state"].as_str().unwrap() {
            "approved" => MoveTo::Approved,
            "active" => MoveTo::Active,
            _ => MoveTo::Paused,
        };
        let agent = body["agent"].as_str().map(|a| AgentId::parse(a).unwrap());
        match cx.state(&workflow, to, agent) {
            Ok(()) => Ok(RawValue::from_string("null".to_owned()).unwrap()),
            Err(Error::Refused(name)) => Err(name),
            Err(Error::Fault(_)) => Err(ErrorName::Internal),
        }
    }
}

fn set_state(store: &mut Store, workflow: &str, state: &str, agent: Option<&str>) {
    let agent = agent.map_or("null".to_owned(), |a| format!("\"{a}\""));
    let body = format!(
        r#"{{"op":"set_state","workflow":"{workflow}","state":"{state}","agent":{agent}}}"#
    );
    store
        .handle(&raw_request("coordinator_write", &body))
        .unwrap();
}

/// The bodies of the stored workflows and of the support rows and the revision of the family `workflow`, read through
/// the test's own connection.
fn held(scratch: &Scratch) -> (Vec<String>, Vec<String>, i64) {
    (
        bodies(scratch, "workflow"),
        bodies(scratch, "support"),
        revision(scratch),
    )
}

/// C2.4, the row of a kept workflow whose `agent` is not null (the coordinator's ruling S-64, point 1, item 2): a
/// proposal with the key of a workflow that is `active` with an agent, or `paused` with an agent, leaves the row, its
/// support rows and the revision as they are, and is counted in `unchanged`; the same proposal for the same workflow
/// with `agent` null, `approved` with no agent or `approved` after its agent was cleared, changes the row and its
/// support rows and raises the revision by one, as before. In each case the five counts sum to the two proposals of
/// the call, the second of which names no summary and is refused.
#[test]
fn a_proposal_with_the_key_of_a_workflow_that_an_agent_does_leaves_its_row_support_rows_and_revision(
) {
    let agent = "4".repeat(32);
    let cases: [(&str, &[(&str, bool)], bool); 4] = [
        ("guard-active", &[("active", true)], true),
        ("guard-paused", &[("active", true), ("paused", true)], true),
        ("guard-approved", &[], false),
        (
            "guard-cleared",
            &[("active", true), ("approved", false)],
            false,
        ),
    ];
    for (name, moves, guarded) in cases {
        let scratch = Scratch::new(name);
        let clock = TestClock::at_day(DAY);
        let fixed = Rc::new(RefCell::new(vec![over(&[1])]));
        let host = || {
            Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed)))).handler(
                "test",
                Takes::Coordinator(&["set_state"]),
                Box::new(SetState),
            )
        };
        let mut store = open_host(&scratch, host()).unwrap();
        let first = store
            .commit_batch(
                &store_id(1),
                &[episode(1, DAY, None), episode(2, DAY, None)],
                position(1, 0),
                &mut Seq(100),
            )
            .unwrap();
        assert_eq!(first.proposals.stored, 1, "{name}");
        let id = minted(&mut Seq(100));
        store
            .decide(&InterfaceDecision::approve(
                &InterfaceRole::claim(),
                RequestId::from_random([2; 16]),
                id.clone(),
            ))
            .unwrap();
        for (state, with_agent) in moves {
            set_state(
                &mut store,
                id.as_str(),
                state,
                with_agent.then_some(&agent[..]),
            );
        }
        drop(store);
        let before = held(&scratch);
        let Workflow::Full(set) = serde_json::from_str(&before.0[0]).unwrap() else {
            panic!("{name}: the workflow is dismissed");
        };
        let mut changed = over(&[1, 2]);
        changed.trigger = Trigger::Schedule;
        changed.confidence = Confidence(90);
        let mut names_none = over(&[]);
        names_none.app = AppId::parse("com.example.other").unwrap();
        *fixed.borrow_mut() = vec![changed, names_none];
        let mut store = open_host(&scratch, host()).unwrap();
        let outcome = store
            .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
            .unwrap()
            .proposals;
        drop(store);
        let after = held(&scratch);
        let sum = outcome.stored
            + outcome.updated
            + outcome.unchanged
            + outcome.dismissed
            + outcome.refused;
        assert_eq!(sum, 2, "{name}: the counts sum to the proposals");
        let Workflow::Full(stored) = serde_json::from_str(&after.0[0]).unwrap() else {
            panic!("{name}: the workflow is dismissed");
        };
        assert_eq!(
            (stored.id.clone(), stored.state, stored.agent.clone()),
            (set.id.clone(), set.state, set.agent.clone()),
            "{name}: the id, the state and the agent stay"
        );
        if guarded {
            assert!(set.agent.is_some(), "{name}");
            assert_eq!(
                outcome,
                ProposalOutcome {
                    unchanged: 1,
                    refused: 1,
                    ..ProposalOutcome::default()
                },
                "{name}"
            );
            assert_eq!(
                after, before,
                "{name}: (the workflow, the support rows, the revision) stay as they were"
            );
        } else {
            assert_eq!(set.agent, None, "{name}");
            assert_eq!(
                outcome,
                ProposalOutcome {
                    updated: 1,
                    refused: 1,
                    ..ProposalOutcome::default()
                },
                "{name}"
            );
            assert_eq!(
                (
                    stored.trigger,
                    stored.confidence,
                    stored.recurrence.count,
                    after.1.len(),
                    after.2
                ),
                (Trigger::Schedule, Confidence(90), 2, 2, before.2 + 1),
                "{name}: (trigger, confidence, count, support rows, revision) as the proposal gives them"
            );
        }
    }
}

fn support_ids(scratch: &Scratch) -> Vec<String> {
    let mut ids: Vec<String> = supports(scratch)
        .iter()
        .map(|s| s.summary.as_str().to_owned())
        .collect();
    ids.sort();
    ids
}

#[test]
fn the_support_rows_of_an_updated_workflow_are_those_of_its_latest_proposal_alone() {
    let scratch = Scratch::new("support-replaced");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(vec![over(&[1, 2])]));
    let host = || Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host()).unwrap();
    let outcome = store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, DAY, None),
                episode(2, DAY, None),
                episode(3, DAY, None),
            ],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert_eq!(outcome.proposals.stored, 1);
    drop(store);
    let id = |n: u64| summary_id(n).as_str().to_owned();
    assert_eq!(support_ids(&scratch), vec![id(1), id(2)]);
    let before = revision(&scratch);
    *fixed.borrow_mut() = vec![over(&[2, 3])];
    let mut store = open_host(&scratch, host()).unwrap();
    let outcome = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    assert_eq!(outcome.proposals.updated, 1);
    let report = store.report().unwrap();
    let role = InterfaceRole::claim();
    let page = store
        .interface_read(&InterfaceRead::workflows(
            &role,
            RequestId::from_random([1; 16]),
            None,
        ))
        .unwrap();
    drop(store);
    let ReplyResult::Workflows(page) = page else {
        panic!("no workflows page");
    };
    let read: Vec<String> = page.workflows[0]
        .summaries
        .iter()
        .map(|s| s.as_str().to_owned())
        .collect();
    assert_eq!(
        (support_ids(&scratch), read),
        (vec![id(2), id(3)], vec![id(2), id(3)]),
        "(the support rows, the read's summaries)"
    );
    assert_eq!(
        report.counts.bounded, 5,
        "three summaries and the two support rows of the latest proposal"
    );
    assert_eq!(revision(&scratch), before + 1);
}

/// A proposal that names a stored summary outside its bound for the proposal check, one day past it or one of a day
/// after the clock's day, is refused and counted in `refused`, and no workflow row and no support row comes of it,
/// while the same proposal inside the bound is stored: the control, with the clock moved to the day of the summary
/// that was one day ahead, stores the proposal that names that summary alone as one workflow. For this check a summary is inside its bound when its day is not
/// after the clock's day and the clock's day less its day is less than the days of the entry in force, which every
/// summary that the summary read of C2.11 visits meets (C2.4; the coordinator's ruling DS-5 (a), reading A).
#[test]
fn a_proposal_that_names_a_summary_after_the_clock_s_day_or_past_its_bound_is_refused() {
    let scratch = Scratch::new("proposal-outside");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY - 92, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    clock.set_day(DAY + 1);
    *fixed.borrow_mut() = vec![over(&[1])];
    let past = store
        .commit_batch(
            &store_id(1),
            &[episode(3, DAY + 1, None)],
            position(2, 0),
            &mut Seq(200),
        )
        .unwrap();
    clock.set_day(DAY);
    *fixed.borrow_mut() = vec![over(&[3])];
    let ahead = store
        .commit_batch(&store_id(1), &[], position(3, 0), &mut Seq(300))
        .unwrap();
    drop(store);
    assert_eq!(
        (
            (past.summaries, past.proposals.refused, past.proposals.stored),
            (ahead.proposals.refused, ahead.proposals.stored)
        ),
        ((1, 1, 0), (1, 0)),
        "((summaries stored, refused, stored) one day past the bound, (refused, stored) one day ahead)"
    );
    assert_eq!(workflows(&scratch).len(), 0);
    assert_eq!(supports(&scratch).len(), 0);
    let clock = TestClock::at_day(DAY + 1);
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    let control = store
        .commit_batch(&store_id(1), &[], position(4, 0), &mut Seq(400))
        .unwrap();
    drop(store);
    assert_eq!(
        control.proposals.stored, 1,
        "the control: the same proposal inside the bound is stored"
    );
}

/// DS-4, H5 (l): the summary read takes its lower side from the stored day of going: a summary whose day of going
/// was brought forward, as an earlier and shorter bound leaves it, is not visited on that day with no expiry run.
#[test]
fn the_summary_read_leaves_out_a_summary_whose_stored_day_of_going_has_come() {
    let scratch = Scratch::new("summary-gone");
    let clock = TestClock::at_day(DAY);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let host = || {
        Host::new(
            Box::new(clock.clone()),
            Box::new(Recorder(Rc::clone(&seen))),
        )
    };
    let mut store = open_host(&scratch, host()).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY - 50, None), episode(2, DAY - 50, None)],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    drop(store);
    common::raw(&scratch)
        .execute(
            "UPDATE rows SET gone = ?1 WHERE kind = 'summary' AND summary = ?2",
            (DAY + 5, summary_id(1).as_str()),
        )
        .unwrap();
    let mut store = open_host(&scratch, host()).unwrap();
    clock.set_day(DAY + 5);
    seen.borrow_mut().clear();
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    drop(store);
    assert_eq!(
        *seen.borrow(),
        vec![(DAY - 50, summary_id(2).as_str().to_owned())],
        "the proposer's summary read on the day of going of summary 1, with no expiry run"
    );
}

/// S-57 (a): a step returns at most one proposal for a key in one call. Where it returns more, the first that passes
/// its checks is the call's proposal for that key, and each later one with that key is refused whole and counted in
/// `refused`; a proposal that another cause refuses holds no key. The five counts sum to the proposals returned.
#[test]
fn of_proposals_of_one_key_in_one_call_the_first_that_passes_stands_and_each_later_one_is_refused()
{
    let run = |name: &str, proposals: Vec<Proposal>| {
        let scratch = Scratch::new(name);
        let clock = TestClock::at_day(DAY);
        let fixed = Rc::new(RefCell::new(proposals));
        let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
        let mut store = open_host(&scratch, host).unwrap();
        let outcome = store
            .commit_batch(
                &store_id(1),
                &[
                    episode(1, DAY, None),
                    episode(2, DAY, None),
                    episode(3, DAY, domain("x-power.test")),
                ],
                position(1, 0),
                &mut Seq(100),
            )
            .unwrap()
            .proposals;
        drop(store);
        let sum = outcome.stored
            + outcome.updated
            + outcome.unchanged
            + outcome.dismissed
            + outcome.refused;
        let mut ids: Vec<String> = supports(&scratch)
            .iter()
            .map(|s| s.summary.as_str().to_owned())
            .collect();
        ids.sort();
        (
            outcome.stored,
            outcome.refused,
            sum,
            ids,
            revision(&scratch),
        )
    };
    let id = |n: u64| summary_id(n).as_str().to_owned();
    assert_eq!(
        run("one-key-three", vec![over(&[1]), over(&[2]), over(&[1, 2])]),
        (1, 2, 3, vec![id(1)], 2),
        "(stored, refused, the five counts summed, support ids, revision after the batch and the proposals): the \
         first stands"
    );
    assert_eq!(
        run(
            "one-key-first-refused",
            vec![over(&[9]), over(&[2]), over(&[1])]
        ),
        (1, 2, 3, vec![id(2)], 2),
        "where another cause refuses the first, the next one of the key stands"
    );
    let other = keyed(
        SeedCategory::UtilityBillPayment,
        address("x-power.test"),
        "com.example.browser",
        &[3],
    );
    assert_eq!(
        run("two-keys", vec![over(&[1]), other]),
        (2, 0, 2, vec![id(1), id(3)], 2),
        "two proposals of two keys are both stored"
    );
}

/// DS-5 (a), C2.11: the summary read keeps its upper side. With the clock set back below two stored summaries, one
/// and two days after the clock's day, which the expiry keeps, a commit's summary read visits neither; each is
/// visited from the first commit after the clock has reached its day, and no expiry on the way removes a summary.
#[test]
fn the_summary_read_visits_a_summary_of_a_day_after_the_clock_s_day_only_once_the_clock_reaches_it()
{
    let scratch = Scratch::new("summary-read-upper");
    let clock = TestClock::at_day(DAY + 2);
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
                episode(1, DAY, None),
                episode(2, DAY + 1, None),
                episode(3, DAY + 2, None),
            ],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    let mut walked = Vec::new();
    for (at, day) in [(2, DAY), (3, DAY + 1), (4, DAY + 2)] {
        clock.set_day(day);
        let removed = store.expire().unwrap();
        seen.borrow_mut().clear();
        store
            .commit_batch(&store_id(1), &[], position(at, 0), &mut Seq(1))
            .unwrap();
        let ids: Vec<String> = seen.borrow().iter().map(|(_, id)| id.clone()).collect();
        walked.push((ids, removed));
    }
    drop(store);
    let id = |n: u64| summary_id(n).as_str().to_owned();
    assert_eq!(
        walked,
        vec![
            (vec![id(1)], 0),
            (vec![id(1), id(2)], 0),
            (vec![id(1), id(2), id(3)], 0),
        ],
        "(the summaries visited, the rows that the expiry removed) with the clock on DAY, DAY + 1 and DAY + 2"
    );
    assert_eq!(
        count(&scratch, "SELECT count(*) FROM rows WHERE kind = 'summary'"),
        3
    );
}
