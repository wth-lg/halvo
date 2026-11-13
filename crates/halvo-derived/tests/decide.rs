mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{
    bodies, count, domain, episode, open_entries, open_host, position, raw, raw_request, store_id,
    summary_id, Fixed, Grouping, GroupingExcept, Scratch, Seq, SignInGroup, TestClock, NOTE,
};
use halvo_derived::{Error, Handler, HandlerCx, Host, MoveTo, Store, Takes};
use halvo_schema_core::ids::{AgentId, RequestId, WorkflowId};
use halvo_schema_core::record::{
    Proposal, RecurrenceStep, RowRead, Summary, SummaryRead, TypedRecord, Workflow, WorkflowState,
    WriterStep,
};
use halvo_schema_core::request::{ErrorName, InterfaceDecision, InterfaceRole, RawRequest};
use halvo_schema_core::site::{AppId, DomainSource, SiteDomain, SiteValue};
use serde_json::value::RawValue;

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
    let mut x = common_proposal(&[9]);
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
            "INSERT INTO rows (kind, family, class, day, workflow, body) VALUES ('agent_run', 'agent', 1, ?1, ?2, \
             json_object('workflow', ?2))",
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

fn summary_ids(scratch: &Scratch) -> Vec<String> {
    let mut ids: Vec<String> = bodies(scratch, "summary")
        .iter()
        .map(|b| {
            serde_json::from_str::<Summary>(b)
                .unwrap()
                .id
                .as_str()
                .to_owned()
        })
        .collect();
    ids.sort();
    ids
}

fn sid(n: u64) -> String {
    summary_id(n).as_str().to_owned()
}

/// A proposal under the sign-in key of northwind-power.test that names the given summaries.
fn signed(ids: &[u64]) -> Proposal {
    let mut proposal = common_proposal(ids);
    proposal.app = AppId::parse("com.example.browser").unwrap();
    proposal.site = Some(SiteValue::Domain {
        key: SiteDomain::parse("northwind-power.test").unwrap(),
        source: DomainSource::SignIn,
    });
    proposal
}

#[test]
fn the_summaries_of_a_proposal_under_another_key_never_support_a_workflow_to_dismiss_and_delete() {
    let scratch = Scratch::new("refused-other-key");
    let clock = TestClock::at_day(DAY);
    let fixed = Rc::new(RefCell::new(vec![signed(&[1, 2])]));
    let mut store = open_host(&scratch, host(&clock, Fixed(Rc::clone(&fixed)))).unwrap();
    let first = store
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
    assert_eq!((first.proposals.refused, first.proposals.stored), (1, 0));
    let later = store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    assert_eq!(later.proposals.stored, 0);
    drop(store);
    assert_eq!(workflows(&scratch).len(), 0);
    assert_eq!(summary_ids(&scratch), vec![sid(1), sid(2), sid(3)]);
    assert_eq!(bodies(&scratch, "evidence").len(), 3);
    assert_eq!(bodies(&scratch, "support").len(), 0);
}

#[test]
fn after_a_refused_proposal_under_another_key_a_batch_with_no_episode_stores_no_workflow() {
    let scratch = Scratch::new("refused-sign-in");
    let clock = TestClock::at_day(DAY);
    let mut store = open_host(&scratch, host(&clock, SignInGroup { min: 2 })).unwrap();
    let first = store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, DAY, domain("northwind-power.test")),
                episode(2, DAY, domain("northwind-power.test")),
            ],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    assert_eq!((first.proposals.refused, first.proposals.stored), (1, 0));
    for at in 2..4 {
        let later = store
            .commit_batch(&store_id(1), &[], position(at, 0), &mut Seq(100 * at))
            .unwrap();
        assert_eq!(later.proposals.stored, 0);
    }
    drop(store);
    assert_eq!(workflows(&scratch).len(), 0);
    assert_eq!(summary_ids(&scratch), vec![sid(1), sid(2)]);
    assert_eq!(bodies(&scratch, "evidence").len(), 2);
    assert_eq!(bodies(&scratch, "support").len(), 0);
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
        cx.state(&workflow, to, agent).map_err(|e| match e {
            Error::Refused(name) => name,
            Error::Fault(_) => ErrorName::Internal,
        })?;
        Ok(RawValue::from_string("null".to_owned()).unwrap())
    }
}

struct Emit(Rc<RefCell<Vec<TypedRecord>>>);

impl WriterStep for Emit {
    fn after_commit(&self, _summaries: &dyn SummaryRead, _rows: &dyn RowRead) -> Vec<TypedRecord> {
        self.0.borrow_mut().drain(..).collect()
    }
}

#[derive(Clone, Copy, Debug)]
enum StateAt {
    Proposed,
    Approved,
    Active,
    Paused,
    Dismissed,
}

#[derive(Clone, Copy, Debug)]
enum Before {
    AsProposed,
    OneLeftTheSet,
    OneExpired,
}

fn set_state(store: &mut Store, id: &WorkflowId, state: &str) {
    let body = format!(
        r#"{{"op":"set_state","workflow":"{}","state":"{state}","agent":"{}"}}"#,
        id.as_str(),
        "4".repeat(32)
    );
    store
        .handle(&raw_request("coordinator_write", &body))
        .unwrap();
}

fn families(scratch: &Scratch) -> Vec<(String, i64)> {
    let conn = raw(scratch);
    let mut statement = conn
        .prepare("SELECT family, n FROM revisions ORDER BY family")
        .unwrap();
    statement
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn strings(scratch: &Scratch, sql: &str, param: &str) -> Vec<String> {
    let conn = raw(scratch);
    let mut statement = conn.prepare(sql).unwrap();
    statement
        .query_map([param], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// One case of the sweep: W, of the site key northwind-power.test, with three summaries of its key, a row of the
/// test kind that names it, and one summary of another key; its support set first changed as `before` says and W
/// moved to `state`; then W is deleted.
fn sweep_case(state: StateAt, before: Before) {
    let scratch = Scratch::new(&format!("sweep-{state:?}-{before:?}").to_lowercase());
    let clock = TestClock::at_day(DAY);
    let except: Rc<RefCell<Vec<halvo_schema_core::ids::SummaryId>>> =
        Rc::new(RefCell::new(Vec::new()));
    let emitted: Rc<RefCell<Vec<TypedRecord>>> = Rc::new(RefCell::new(Vec::new()));
    let host = || {
        Host::new(
            Box::new(clock.clone()),
            Box::new(GroupingExcept {
                min: 2,
                except: Rc::clone(&except),
            }),
        )
        .step("test", Box::new(Emit(Rc::clone(&emitted))))
        .handler(
            "test",
            Takes::Coordinator(&["set_state"]),
            Box::new(SetState),
        )
    };
    let mut store = open_entries(&scratch, host(), &[&NOTE]).unwrap();
    let first_day = match before {
        Before::OneExpired => DAY - 92,
        Before::AsProposed | Before::OneLeftTheSet => DAY,
    };
    store
        .commit_batch(
            &store_id(1),
            &[
                episode(1, first_day, domain("northwind-power.test")),
                episode(2, DAY, domain("northwind-power.test")),
                episode(3, DAY, domain("northwind-power.test")),
                episode(9, DAY, domain("other.test")),
            ],
            position(1, 0),
            &mut Seq(100),
        )
        .unwrap();
    drop(store);
    let stored = workflows(&scratch);
    assert_eq!(stored.len(), 1, "{state:?} {before:?}");
    let w = stored[0].id().clone();
    emitted.borrow_mut().push(TypedRecord {
        kind: "test_note",
        json: format!(r#"{{"v":1,"workflow":"{}","day":{DAY},"n":1}}"#, w.as_str()),
    });
    let mut store = open_entries(&scratch, host(), &[&NOTE]).unwrap();
    store
        .commit_batch(&store_id(1), &[], position(2, 0), &mut Seq(200))
        .unwrap();
    match before {
        Before::AsProposed => {}
        Before::OneLeftTheSet => {
            except.borrow_mut().push(summary_id(1));
            let outcome = store
                .commit_batch(&store_id(1), &[], position(3, 0), &mut Seq(300))
                .unwrap();
            assert_eq!(outcome.proposals.updated, 1);
        }
        Before::OneExpired => {
            clock.set_day(DAY + 1);
            assert!(store.expire().unwrap() >= 3);
        }
    }
    match state {
        StateAt::Proposed => {}
        StateAt::Approved => approve(&mut store, &w).unwrap(),
        StateAt::Active => {
            approve(&mut store, &w).unwrap();
            set_state(&mut store, &w, "active");
        }
        StateAt::Paused => {
            approve(&mut store, &w).unwrap();
            set_state(&mut store, &w, "active");
            set_state(&mut store, &w, "paused");
        }
        StateAt::Dismissed => dismiss(&mut store, &w).unwrap(),
    }
    drop(store);
    let key = strings(
        &scratch,
        "SELECT wkey FROM rows WHERE workflow = ?1 AND kind = 'workflow'",
        w.as_str(),
    )
    .remove(0);
    let removed = strings(
        &scratch,
        "SELECT summary FROM rows WHERE wkey = ?1 AND kind = 'summary'",
        &key,
    );
    let expect_removed = match before {
        Before::OneExpired => 2,
        Before::AsProposed | Before::OneLeftTheSet => 3,
    };
    assert_eq!(removed.len(), expect_removed, "{state:?} {before:?}");
    let revisions_before = families(&scratch);
    let mut store = open_entries(&scratch, host(), &[&NOTE]).unwrap();
    assert_eq!(delete(&mut store, &w), Ok(()));
    except.borrow_mut().clear();
    let after = store
        .commit_batch(&store_id(1), &[], position(9, 0), &mut Seq(900))
        .unwrap();
    assert_eq!(after.proposals.stored, 0, "{state:?} {before:?}");
    drop(store);
    let case = format!("{state:?} {before:?}");
    assert_eq!(
        count(
            &scratch,
            &format!(
                "SELECT count(*) FROM rows WHERE workflow = '{}'",
                w.as_str()
            )
        ),
        0,
        "{case}: a row names W"
    );
    assert_eq!(
        strings(&scratch, "SELECT kind FROM rows WHERE wkey = ?1", &key).len(),
        0,
        "{case}: a summary or a workflow holds W's key"
    );
    for summary in &removed {
        assert_eq!(
            strings(
                &scratch,
                "SELECT kind FROM rows WHERE summary = ?1 AND kind IN ('evidence', 'support', 'summary')",
                summary
            )
            .len(),
            0,
            "{case}: a row names a removed summary"
        );
    }
    assert_eq!(summary_ids(&scratch), vec![sid(9)], "{case}");
    assert_eq!(
        strings(
            &scratch,
            "SELECT kind FROM rows WHERE summary = ?1 AND kind = 'evidence'",
            &sid(9)
        )
        .len(),
        1,
        "{case}"
    );
    let revisions_after = families(&scratch);
    for ((family, was), (_, now)) in revisions_before.iter().zip(&revisions_after) {
        let lost = family == "workflow" || family == "test";
        assert_eq!(
            now > was,
            lost,
            "{case}: the revision of {family} went from {was} to {now}"
        );
    }
}

#[test]
fn the_delete_removes_all_that_supported_a_workflow_in_every_state_and_every_history_of_its_support_set(
) {
    for state in [
        StateAt::Proposed,
        StateAt::Approved,
        StateAt::Active,
        StateAt::Paused,
        StateAt::Dismissed,
    ] {
        for before in [
            Before::AsProposed,
            Before::OneLeftTheSet,
            Before::OneExpired,
        ] {
            sweep_case(state, before);
        }
    }
}
