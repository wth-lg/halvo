mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    domain, episode, open_host, position, raw_request, store_id, summary_id, token, Fixed,
    Grouping, Scratch, Seq, TestClock,
};
use halvo_derived::{Error, Handler, HandlerCx, Host, Store, Takes};
use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{RequestId, SummaryId, WorkflowId};
use halvo_schema_core::record::{
    Confidence, Episode, Evidence, EvidencePage, FloorLayer, Instant, ObservedStep, Proposal,
    Recurrence, Tagged, Text, Trigger, Workflow, WorkflowItem,
};
use halvo_schema_core::request::{
    self, ErrorName, InterfaceDecision, InterfaceRead, InterfaceRole, RawRequest, ReplyResult,
    REPLY_MAX,
};
use halvo_schema_core::site::{AppId, DomainSource, SiteDomain, SiteValue};
use serde_json::value::RawValue;

const DAY: u32 = 20_767;

fn rid() -> RequestId {
    RequestId::from_random([6; 16])
}

fn workflows(store: &Store, after: Option<WorkflowId>) -> Result<ReplyResult, Error> {
    store.interface_read(&InterfaceRead::workflows(
        &InterfaceRole::claim(),
        rid(),
        after,
    ))
}

fn evidence(
    store: &Store,
    workflow: WorkflowId,
    after: Option<SummaryId>,
) -> Result<ReplyResult, Error> {
    store.interface_read(&InterfaceRead::evidence(
        &InterfaceRole::claim(),
        rid(),
        workflow,
        after,
    ))
}

fn reply_len(result: &ReplyResult) -> usize {
    request::write_reply(Some(&rid()), Ok(result))
        .unwrap()
        .len()
}

fn site(name: &str) -> Option<SiteValue> {
    Some(SiteValue::Domain {
        key: SiteDomain::parse(name).unwrap(),
        source: DomainSource::AddressField,
    })
}

fn proposal(site: Option<SiteValue>, app: &str, ids: &[u64]) -> Proposal {
    Proposal {
        category: SeedCategory::UtilityBillPayment,
        site,
        app: AppId::parse(app).unwrap(),
        trigger: Trigger::Schedule,
        recurrence: Recurrence {
            count: 1,
            period: None,
            first: DAY,
            last: DAY,
        },
        confidence: Confidence(60),
        summaries: ids.iter().map(|n| summary_id(*n)).collect(),
    }
}

fn page(result: Result<ReplyResult, Error>) -> (Vec<WorkflowItem>, Option<WorkflowId>) {
    match result {
        Ok(ReplyResult::Workflows(page)) => (page.workflows, page.next),
        other => panic!("the read answered {other:?}"),
    }
}

#[test]
fn the_read_of_workflows_returns_each_stored_workflow_with_its_support_set_and_its_hint() {
    let scratch = Scratch::new("read-workflows");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    let mut late = episode(2, DAY, token("northwind-power"));
    late.end = Instant(late.end.0 + 10_000);
    let mut early = episode(1, DAY, token("northwind-power"));
    early.start = Instant(early.start.0 - 5_000);
    early.end = Instant(early.end.0 - 5_000);
    store
        .commit_batch(
            &store_id(1),
            &[
                early,
                late,
                episode(3, DAY, domain("shop.test")),
                episode(4, DAY, None),
            ],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    assert!(page(workflows(&store, None)).0.is_empty());
    drop(store);
    let title_site: Option<SiteValue> = common::bodies(&scratch, "summary")
        .iter()
        .map(|b| serde_json::from_str::<halvo_schema_core::record::Summary>(b).unwrap())
        .find(|s| s.id == summary_id(1))
        .unwrap()
        .site;
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    *fixed.borrow_mut() = vec![
        proposal(title_site.clone(), "com.example.browser", &[2, 1]),
        proposal(site("shop.test"), "com.example.browser", &[3]),
        proposal(None, "com.example.browser", &[4]),
    ];
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(500))
        .unwrap();
    let (items, next) = page(workflows(&store, None));
    assert_eq!(next, None);
    assert_eq!(items.len(), 3);
    assert!(items
        .windows(2)
        .all(|w| w[0].workflow.id() < w[1].workflow.id()));
    let by_site = |want: &Option<SiteValue>| {
        items
            .iter()
            .find(|i| matches!(&i.workflow, Workflow::Full(f) if &f.site == want))
            .unwrap()
            .clone()
    };
    let titled = by_site(&title_site);
    assert_eq!(titled.summaries, vec![summary_id(1), summary_id(2)]);
    assert_eq!(titled.hint.as_ref().unwrap().as_str(), "northwind-power");
    let shop = by_site(&site("shop.test"));
    assert_eq!((shop.summaries.len(), shop.hint.is_none()), (1, true));
    let plain = by_site(&None);
    store
        .decide(&InterfaceDecision::dismiss(
            &InterfaceRole::claim(),
            rid(),
            plain.workflow.id().clone(),
        ))
        .unwrap();
    let (items, _) = page(workflows(&store, None));
    let dismissed = items
        .iter()
        .find(|i| matches!(i.workflow, Workflow::Dismissed(_)))
        .unwrap();
    assert!(dismissed.summaries.is_empty() && dismissed.hint.is_none());
    let first = items[0].workflow.id().clone();
    let (rest, _) = page(workflows(&store, Some(first)));
    assert_eq!(rest.len(), 2);
}

#[test]
fn the_read_of_evidence_returns_the_evidence_of_a_support_set_by_summary_id() {
    let scratch = Scratch::new("read-evidence");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(vec![proposal(
        None,
        "com.example.browser",
        &[3, 1, 2],
    )]));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    let mut gone = episode(2, DAY, None);
    gone.expires = Instant(clock.now());
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None), gone, episode(3, DAY, None)],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    let id = page(workflows(&store, None)).0[0].workflow.id().clone();
    match evidence(&store, id.clone(), None) {
        Ok(ReplyResult::Evidence(page)) => {
            let ids: Vec<SummaryId> = page.evidence.iter().map(|e| e.summary.clone()).collect();
            assert_eq!(ids, vec![summary_id(1), summary_id(3)]);
            assert_eq!(page.next, None);
            assert_eq!(page.evidence[0].steps.len(), 1);
        }
        other => panic!("the read answered {other:?}"),
    }
    match evidence(&store, id, Some(summary_id(1))) {
        Ok(ReplyResult::Evidence(page)) => assert_eq!(page.evidence.len(), 1),
        other => panic!("the read answered {other:?}"),
    }
    assert_eq!(
        evidence(&store, WorkflowId::from_random([5; 16]), None).err(),
        Some(Error::Refused(ErrorName::NoWorkflow))
    );
}

fn big(n: u64, steps: usize, last: usize) -> Episode {
    let mut episode = episode(n, DAY, domain(&format!("w{n}.test")));
    let step = |len: usize| ObservedStep {
        app: None,
        title: Some(Tagged {
            value: Text::parse(&"a".repeat(len)).unwrap(),
            layer: FloorLayer::Floor,
        }),
        input: None,
        site: None,
        text: None,
    };
    episode.steps = (0..steps).map(|_| step(900)).chain([step(last)]).collect();
    episode
}

/// The length of the reply whose page holds the evidence of these episodes, as the store keeps it.
fn evidence_reply_len(episodes: &[&Episode], next: Option<SummaryId>) -> usize {
    let evidence = episodes
        .iter()
        .map(|e| Evidence {
            summary: e.id.clone(),
            segments: e.segments.clone(),
            expires: e.expires,
            start: e.start,
            end: e.end,
            hint: None,
            steps: e.steps.clone(),
        })
        .collect();
    reply_len(&ReplyResult::Evidence(EvidencePage { evidence, next }))
}

fn workflow_of(store: &Store, domain_name: &str) -> WorkflowId {
    page(workflows(store, None))
        .0
        .into_iter()
        .find(|i| matches!(&i.workflow, Workflow::Full(f) if f.site == site(domain_name)))
        .unwrap()
        .workflow
        .id()
        .clone()
}

#[test]
fn a_page_of_261120_bytes_passes_and_a_page_one_byte_longer_is_refused() {
    let scratch = Scratch::new("reply-bound");
    let clock = TestClock::at_day(DAY);
    let mut steps = 200;
    while evidence_reply_len(&[&big(1, steps + 1, 0)], None) <= REPLY_MAX {
        steps += 1;
    }
    let rest = REPLY_MAX - evidence_reply_len(&[&big(1, steps, 0)], None);
    let exact = big(1, steps, rest);
    let over = big(2, steps, rest + 1);
    assert_eq!(evidence_reply_len(&[&exact], None), 261_120);
    assert_eq!(evidence_reply_len(&[&over], None), 261_121);
    let fixed = Rc::new(RefCell::new(vec![
        proposal(site("w1.test"), "com.example.browser", &[1]),
        proposal(site("w2.test"), "com.example.browser", &[2]),
    ]));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    store
        .commit_batch(&store_id(1), &[exact, over], position(1, 0), &mut Seq(1))
        .unwrap();
    let full = evidence(&store, workflow_of(&store, "w1.test"), None).unwrap();
    assert_eq!(reply_len(&full), 261_120);
    assert!(matches!(&full, ReplyResult::Evidence(p) if p.evidence.len() == 1 && p.next.is_none()));
    assert_eq!(
        evidence(&store, workflow_of(&store, "w2.test"), None).err(),
        Some(Error::Refused(ErrorName::TooLarge))
    );
}

#[test]
fn a_page_holds_whole_items_while_the_reply_stays_inside_its_bound() {
    let scratch = Scratch::new("reply-pages");
    let clock = TestClock::at_day(DAY);
    let mut first = big(3, 140, 0);
    first.site = domain("w3.test");
    let mut second = big(4, 140, 0);
    second.site = domain("w3.test");
    assert!(evidence_reply_len(&[&first], Some(first.id.clone())) <= REPLY_MAX);
    assert!(evidence_reply_len(&[&first, &second], None) > REPLY_MAX);
    let fixed = Rc::new(RefCell::new(vec![proposal(
        site("w3.test"),
        "com.example.browser",
        &[3, 4],
    )]));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    store
        .commit_batch(&store_id(1), &[first, second], position(1, 0), &mut Seq(1))
        .unwrap();
    let id = workflow_of(&store, "w3.test");
    let one = evidence(&store, id.clone(), None).unwrap();
    assert!(reply_len(&one) <= REPLY_MAX);
    let ReplyResult::Evidence(one) = one else {
        panic!("no evidence page");
    };
    assert_eq!(one.evidence.len(), 1);
    assert_eq!(one.next, Some(summary_id(3)));
    match evidence(&store, id, one.next) {
        Ok(ReplyResult::Evidence(two)) => {
            assert_eq!(two.evidence.len(), 1);
            assert_eq!(
                (two.evidence[0].summary.clone(), two.next),
                (summary_id(4), None)
            );
        }
        other => panic!("the read answered {other:?}"),
    }
}

/// A first item and a small second one under one workflow of the site `w<n>.test`, the first tuned so that the reply
/// that holds it with the id of its summary as `next` has `bytes` bytes.
fn pair(n: u64, bytes: usize) -> (Episode, Episode) {
    let next = Some(summary_id(n));
    let mut steps = 200;
    while evidence_reply_len(&[&big(n, steps + 1, 0)], next.clone()) <= bytes {
        steps += 1;
    }
    let rest = bytes - evidence_reply_len(&[&big(n, steps, 0)], next.clone());
    let first = big(n, steps, rest);
    let mut second = episode(n + 1, DAY, domain(&format!("w{n}.test")));
    second.steps.clear();
    assert_eq!(evidence_reply_len(&[&first], next), bytes);
    (first, second)
}

#[test]
fn a_page_counts_the_id_under_next_against_the_bound() {
    let scratch = Scratch::new("reply-next");
    let clock = TestClock::at_day(DAY);
    let (fits, after_fits) = pair(10, REPLY_MAX);
    let (over, after_over) = pair(20, REPLY_MAX + 1);
    let fixed = Rc::new(RefCell::new(vec![
        proposal(site("w10.test"), "com.example.browser", &[10, 11]),
        proposal(site("w20.test"), "com.example.browser", &[20, 21]),
    ]));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[fits, after_fits, over, after_over],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    let page = evidence(&store, workflow_of(&store, "w10.test"), None).unwrap();
    assert_eq!(reply_len(&page), 261_120);
    assert!(
        matches!(&page, ReplyResult::Evidence(p) if p.evidence.len() == 1 && p.next == Some(summary_id(10)))
    );
    assert_eq!(
        evidence(&store, workflow_of(&store, "w20.test"), None).err(),
        Some(Error::Refused(ErrorName::TooLarge))
    );
}

struct ReadsOne {
    workflow: WorkflowId,
    found: Rc<RefCell<Vec<String>>>,
}

impl Handler for ReadsOne {
    fn handle(
        &self,
        _request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let item = cx
            .workflow(&self.workflow)
            .map_err(|_| ErrorName::Internal)?;
        let missing = cx
            .workflow(&WorkflowId::from_random([5; 16]))
            .map_err(|_| ErrorName::Internal)?;
        let page = cx
            .evidence(&self.workflow, None)
            .map_err(|_| ErrorName::Internal)?;
        self.found.borrow_mut().push(format!(
            "{} {} {}",
            item.map_or(0, |i| i.summaries.len()),
            missing.is_none(),
            page.evidence.len()
        ));
        Ok(RawValue::from_string("null".to_owned()).unwrap())
    }
}

#[test]
fn a_handler_reads_one_stored_workflow_and_the_pages_of_its_evidence() {
    let scratch = Scratch::new("read-one");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(vec![proposal(
        None,
        "com.example.browser",
        &[1, 2],
    )]));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    store
        .commit_batch(
            &store_id(1),
            &[episode(1, DAY, None), episode(2, DAY, None)],
            position(1, 0),
            &mut Seq(1),
        )
        .unwrap();
    let id = page(workflows(&store, None)).0[0].workflow.id().clone();
    drop(store);
    let found = Rc::new(RefCell::new(Vec::new()));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed)))).handler(
        "test",
        Takes::Runner,
        Box::new(ReadsOne {
            workflow: id,
            found: Rc::clone(&found),
        }),
    );
    let mut store = open_host(&scratch, host).unwrap();
    store.handle(&raw_request("runner", "{}")).unwrap();
    assert_eq!(*found.borrow(), vec!["2 true 2".to_owned()]);
}

/// DS-4: the evidence read hands out a window row while its `expires` lies after the clock's instant, and from that
/// instant leaves it out with no expiry run; the expiry at that clock still removes it.
#[test]
fn the_evidence_read_leaves_out_a_window_row_from_its_expiry_instant() {
    let scratch = Scratch::new("evidence-past-expiry");
    let clock = TestClock::at_day(DAY);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 }));
    let mut store = open_host(&scratch, host).unwrap();
    let expires = clock.now() + 5_000;
    let mut soon = episode(1, DAY, None);
    soon.expires = Instant(expires);
    store
        .commit_batch(&store_id(1), &[soon], position(1, 0), &mut Seq(1))
        .unwrap();
    let id = page(workflows(&store, None)).0[0].workflow.id().clone();
    let handed_out = |store: &Store| match evidence(store, id.clone(), None) {
        Ok(ReplyResult::Evidence(page)) => page.evidence.len(),
        other => panic!("the read answered {other:?}"),
    };
    let before = handed_out(&store);
    clock.set(expires - 1);
    let edge = handed_out(&store);
    clock.set(expires);
    let at = handed_out(&store);
    let removed = store.expire().unwrap();
    let after = handed_out(&store);
    drop(store);
    assert_eq!(
        (before, edge, at, removed, after),
        (1, 1, 0, 1, 0),
        "(evidence handed out before its expiry, one nanosecond before it, at it with no expiry run, rows that the \
         expiry removes, evidence handed out after it)"
    );
}

/// DS-4, H5 (h): the hint is that of the evidence row with the latest end among the rows that the read hands out: it
/// falls to the next evidence row when the latest passes its expiry, and it is null when none is left in its window.
#[test]
fn the_hint_falls_to_the_next_evidence_row_inside_its_window_and_is_null_when_none_is_left() {
    let scratch = Scratch::new("hint-past-expiry");
    let clock = TestClock::at_day(DAY);
    let host = Host::new(Box::new(clock.clone()), Box::new(Grouping { min: 1 }));
    let mut store = open_host(&scratch, host).unwrap();
    let first = clock.now() + 5_000;
    let second = clock.now() + 9_000;
    let mut late = episode(2, DAY, token("northwind-power"));
    late.end = Instant(late.end.0 + 10_000);
    late.expires = Instant(first);
    let mut early = episode(1, DAY, token("northwind-power"));
    early.expires = Instant(second);
    store
        .commit_batch(&store_id(1), &[early, late], position(1, 0), &mut Seq(1))
        .unwrap();
    let mut seen = Vec::new();
    for at in [clock.now(), first, second] {
        clock.set(at);
        let item = page(workflows(&store, None)).0.remove(0);
        seen.push((item.hint.is_some(), item.summaries.len()));
    }
    drop(store);
    assert_eq!(
        seen,
        vec![(true, 2), (true, 2), (false, 2)],
        "(a hint, support ids) before the first expiry, at it, and at the second"
    );
}

/// DS-4, H2: the evidence page's candidates hold the evidence row's own expiry inside their EXISTS, so `next` is
/// reserved only for a candidate that the page could hand out: a page of exactly 261,120 bytes stays whole, with a
/// null `next`, when the other evidence row of its workflow has passed its expiry and no expiry has run.
#[test]
fn a_page_of_the_bound_stays_whole_when_the_other_evidence_row_has_passed_its_expiry() {
    let scratch = Scratch::new("reply-bound-expired");
    let clock = TestClock::at_day(DAY);
    let mut steps = 200;
    while evidence_reply_len(&[&big(1, steps + 1, 0)], None) <= REPLY_MAX {
        steps += 1;
    }
    let rest = REPLY_MAX - evidence_reply_len(&[&big(1, steps, 0)], None);
    let exact = big(1, steps, rest);
    let mut soon = episode(2, DAY, domain("w1.test"));
    soon.expires = Instant(clock.now() + 5_000);
    let fixed = Rc::new(RefCell::new(vec![proposal(
        site("w1.test"),
        "com.example.browser",
        &[1, 2],
    )]));
    let host = Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
    let mut store = open_host(&scratch, host).unwrap();
    store
        .commit_batch(&store_id(1), &[exact, soon], position(1, 0), &mut Seq(1))
        .unwrap();
    clock.set(clock.now() + 5_000);
    let page = evidence(&store, workflow_of(&store, "w1.test"), None).unwrap();
    assert_eq!(reply_len(&page), 261_120);
    assert!(
        matches!(&page, ReplyResult::Evidence(p) if p.evidence.len() == 1 && p.next.is_none()),
        "one whole item and a null next"
    );
}

/// The answer of the evidence read of the workflow of `w1.test` as a test reads it: the error, or the reply's length,
/// the number of evidence rows and whether `next` names one.
fn page_shape(store: &Store) -> Result<(usize, usize, bool), Error> {
    let page = evidence(store, workflow_of(store, "w1.test"), None)?;
    match &page {
        ReplyResult::Evidence(p) => Ok((reply_len(&page), p.evidence.len(), p.next.is_some())),
        other => panic!("the read answered {other:?}"),
    }
}

/// DS-4 H2, the conditions of a support row among the candidates of an evidence page: beside the support row of a
/// page of exactly 261,120 bytes stands the support row of a second summary of its workflow, whose evidence row is
/// inside its window. While that row is handed out the first item cannot leave room for a `next` and the read is
/// refused as too large. The test's own connection then gives the second support row a day of going of its own:
/// one later than its summary's, or one on the next day while its summary stands, states that no call of the store
/// makes, so that each condition is the only one that holds the row back. Once the row is due, by its summary's day
/// of going or by its own, it is no candidate and the page stays whole, with a null `next`, before any expiry has
/// run.
#[test]
fn a_page_of_the_bound_stays_whole_beside_a_support_row_that_is_due_by_its_summary_or_by_its_own_day(
) {
    let mut steps = 200;
    while evidence_reply_len(&[&big(1, steps + 1, 0)], None) <= REPLY_MAX {
        steps += 1;
    }
    let rest = REPLY_MAX - evidence_reply_len(&[&big(1, steps, 0)], None);
    let mut seen = Vec::new();
    for by_its_own_day in [false, true] {
        let scratch = Scratch::new("reply-bound-due-support");
        let clock = TestClock::at_day(DAY);
        let second_day = if by_its_own_day { DAY } else { DAY - 92 };
        let fixed = Rc::new(RefCell::new(vec![proposal(
            site("w1.test"),
            "com.example.browser",
            &[1, 2],
        )]));
        let host = || Host::new(Box::new(clock.clone()), Box::new(Fixed(Rc::clone(&fixed))));
        let mut store = open_host(&scratch, host()).unwrap();
        store
            .commit_batch(
                &store_id(1),
                &[
                    big(1, steps, rest),
                    episode(2, second_day, domain("w1.test")),
                ],
                position(1, 0),
                &mut Seq(1),
            )
            .unwrap();
        fixed.borrow_mut().clear();
        drop(store);
        let (day, gone) = if by_its_own_day {
            (DAY, DAY + 1)
        } else {
            (DAY - 50, DAY + 43)
        };
        common::raw(&scratch)
            .execute(
                "UPDATE rows SET day = ?1, gone = ?2 WHERE kind = 'support' AND summary = ?3",
                (day, gone, summary_id(2).as_str()),
            )
            .unwrap();
        let store = open_host(&scratch, host()).unwrap();
        let live = page_shape(&store).err();
        clock.set(i64::from(DAY + 1) * common::DAY_NS);
        seen.push((by_its_own_day, live, page_shape(&store).ok()));
    }
    assert_eq!(
        seen,
        vec![
            (
                false,
                Some(Error::Refused(ErrorName::TooLarge)),
                Some((261_120, 1, false))
            ),
            (
                true,
                Some(Error::Refused(ErrorName::TooLarge)),
                Some((261_120, 1, false))
            ),
        ],
        "(due by its own day, the read while the second support row is handed out, the page once it is due)"
    );
}
