#[path = "items_support.rs"]
mod support;

use halvo_coord::guard::Table;
use halvo_coord::items::drive::{
    answer_bind, bind_due, lapse_bind, raise_bind, BindState, HeldBind, Ledger,
};
use halvo_coord::items::{compose_bind, compose_plain, raise_step, Plain, Raise, Source};
use halvo_coord::mem::{Call, Config, Method, Ports, Tried};
use halvo_coord::ports::{PortError, Runs};
use halvo_coord::types::fixtures::{self, State};
use halvo_coord::types::{
    Answered, BindAnswer, BindAsk, BindResult, Category, Cause, Choice, Doing, Facts, FullWorkflow,
    ItemKind, Lapsed, Limits, Live, Name, Origin, OtherLapse, Raised, RunEnd, RunEvent, RunItem,
    RunItemKind, SignInAnswer, What, Workflow, WorkflowState, WorkflowTrigger,
};
use halvo_coord::words;
use std::num::NonZeroU32;
use support::{agent_id, item_id, run_id, workflow_id};

const NOW: u64 = 1_793_793_600_000;
const TODAY: u32 = 20_760;
const LIMITS: Limits = Limits::PRODUCT;
const ORIGIN: &str = "https://pay.site1.example:8443";
const SITE: &str = "pay.site1.example:8443";
const NAME: &str = "Paying a utility bill on Site1";

fn ports() -> Ports<Config> {
    let p = Ports::new(NOW, TODAY);
    p.workflows.borrow_mut().push(Workflow::Full(FullWorkflow {
        id: workflow_id(1),
        state: WorkflowState::Active,
        category: Category::UtilityBillPayment,
        site: None,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: TODAY,
        agent: Some(agent_id(1)),
    }));
    p.runner_of(&run_id(1));
    p
}

fn ledger() -> Ledger {
    Ledger::new(workflow_id(1), agent_id(1), 0)
}

fn origin() -> Origin {
    Origin::parse(ORIGIN).unwrap()
}

fn ask() -> BindAsk {
    BindAsk {
        run: run_id(1),
        agent: agent_id(1),
        origin: origin(),
    }
}

fn tried(p: &Ports<Config>) -> Vec<Tried> {
    p.tried.borrow().clone()
}

fn stored(p: &Ports<Config>) -> Vec<What> {
    p.events.borrow().iter().map(|e| e.what.clone()).collect()
}

fn row(held: &HeldBind, answer: BindResult) -> What {
    What::ItemAnswered {
        item: held.item().clone(),
        answer: Answered::SignIn(SignInAnswer::Second(BindAnswer {
            answer,
            origin: origin(),
        })),
    }
}

fn working() -> Facts<Config> {
    let f = fixtures::facts(State::Working)
        .into_iter()
        .find(|f| f.workflow.category == Category::UtilityBillPayment)
        .unwrap();
    assert_eq!(f.doing, Doing::Live(Live::Working));
    f
}

/// The page item of a held second step, with the choices as (choice, safe).
fn choices_of(held: &HeldBind) -> Vec<(Choice, &'static str)> {
    held.page_item(NAME)
        .unwrap()
        .choices()
        .iter()
        .map(|c| (c.choice, c.safe.name()))
        .collect()
}

/// The case bind_stored of I5.11 (the agent-setup run's CP-37): the run's event `Bind` for an
/// origin raises an item of the kind `sign_in` in its second step with the choices `decline` and
/// `allow`; answered `allow`, and answered `decline`, each first with the owner not acknowledging
/// its `item_answered` row, `Runs::answer_bind` is called only after the row is acknowledged, with
/// the answer of the row, and the row holds the whole origin.
#[test]
fn refusal_bind_stored() {
    let event = RunEvent::Bind(ask());
    let source = Source::of(&event).expect("a request of the run");
    assert_eq!(
        raise_step(&working(), source, &LIMITS),
        Raise::Run(RunItemKind::SignIn)
    );
    for (choice, result) in [
        (Choice::Allow, BindResult::Allow),
        (Choice::Decline, BindResult::Decline),
    ] {
        let p = ports();
        let mut l = ledger();
        let t = Table::new();
        let mut held = raise_bind(&p, &mut l, &t, &LIMITS, &ask()).unwrap();
        assert_eq!(
            stored(&p),
            vec![What::ItemRaised {
                item: held.item().clone(),
                raised: Raised::SignIn(run_id(1)),
            }]
        );
        let page = held.page_item(NAME).unwrap();
        assert_eq!(page.kind(), ItemKind::SignIn);
        assert_eq!(
            choices_of(&held),
            vec![(Choice::Decline, "yes"), (Choice::Allow, "no")]
        );
        p.refuse(Method::Log, PortError::NoService);
        assert_eq!(
            answer_bind(&p, &mut l, &t, &LIMITS, &mut held, choice),
            Err(Cause::NoService),
            "{choice:?} with no acknowledgement"
        );
        assert!(
            !tried(&p)
                .iter()
                .any(|c| matches!(c.call, Call::AnswerBind(..))),
            "the run gets no answer before its row is stored"
        );
        assert_eq!(held.state(), &BindState::Open);
        p.clear_refusals();
        answer_bind(&p, &mut l, &t, &LIMITS, &mut held, choice).unwrap();
        let calls: Vec<Call> = p.calls.borrow().iter().map(|c| c.call.clone()).collect();
        let n = calls.len();
        assert!(
            matches!(
                &calls[n - 2..],
                [Call::Log(rows), Call::AnswerBind(r, got)]
                    if rows.len() == 1 && rows[0].what == row(&held, result) && r == &run_id(1) && *got == result
            ),
            "{calls:?}"
        );
        assert_eq!(held.state(), &BindState::Closed);
        assert_eq!(
            answer_bind(&p, &mut l, &t, &LIMITS, &mut held, choice),
            Err(Cause::Stale)
        );
        assert_eq!(stored(&p).len(), 2, "one item_raised and one row");
    }
}

/// The second step is a new item of the kind `sign_in` of its run, raised only while the run is
/// `working`, ending the run with `kind` while it waits for an open item, and counted by
/// `items_per_run` (I3.1, I3.3, ruling IG-16).
#[test]
fn a_bind_raises_a_new_sign_in_item_that_items_per_run_counts() {
    let a = ask();
    let mut f = working();
    assert_eq!(
        raise_step(&f, Source::Bind(&a), &LIMITS),
        Raise::Run(RunItemKind::SignIn)
    );
    let mut waiting = f.clone();
    waiting.doing = Doing::Live(Live::Waiting(RunItem {
        item: item_id(1),
        kind: RunItemKind::SignIn,
    }));
    assert_eq!(
        raise_step(&waiting, Source::Bind(&a), &LIMITS),
        Raise::End(RunEnd::Kind),
        "a second request before the first is answered"
    );
    let mut idle = f.clone();
    idle.doing = Doing::Idle(None);
    assert_eq!(
        raise_step(&idle, Source::Bind(&a), &LIMITS),
        Raise::Refused(Cause::State)
    );
    for n in 1..=LIMITS.items_per_run {
        let seq = NonZeroU32::new(f.events.len() as u32 + 1).unwrap();
        f.events.push(halvo_coord::types::Event {
            workflow: f.workflow.id.clone(),
            agent: f.agent.clone(),
            seq,
            day: TODAY,
            at: 0,
            what: What::ItemRaised {
                item: item_id(n),
                raised: Raised::SignIn(run_id(1)),
            },
        });
    }
    assert_eq!(
        raise_step(&f, Source::Bind(&a), &LIMITS),
        Raise::End(RunEnd::Budget)
    );
}

/// The second step reaches the page as an item of the kind `sign_in` whose text is the entry
/// `item.sign_in.bind` with the site's name of the origin, and whose two choices `decline` and
/// `allow` have words of their own; the first step keeps `decline` and `done` with theirs
/// (I3.4, rulings IG-14, IG-15).
#[test]
fn the_second_step_names_the_site_with_its_own_two_choices() {
    let item = compose_bind(&item_id(1), &agent_id(1), NAME, &origin()).unwrap();
    assert_eq!(item.kind(), ItemKind::SignIn);
    assert_eq!(item.presenter(), None);
    assert_eq!(
        item.title(),
        words::text("item.sign_in.title", &[("name", NAME)]).unwrap()
    );
    assert_eq!(
        item.text(),
        words::text("item.sign_in.bind", &[("origin", SITE)]).unwrap()
    );
    assert!(item.text().contains(SITE) && !item.text().contains("https"));
    assert!(item.fields().is_empty() && item.params().is_empty());
    let got: Vec<(Choice, String, &str)> = item
        .choices()
        .iter()
        .map(|c| (c.choice, c.text.clone(), c.safe.name()))
        .collect();
    let t = |k: &str| words::text(k, &[]).unwrap();
    assert_eq!(
        got,
        vec![
            (Choice::Decline, t("choice.sign_in.bind_decline"), "yes"),
            (Choice::Allow, t("choice.sign_in.bind_allow"), "no"),
        ]
    );
    let first = compose_plain(&item_id(1), &agent_id(1), Plain::SignIn, NAME).unwrap();
    let first: Vec<(Choice, String)> = first
        .choices()
        .iter()
        .map(|c| (c.choice, c.text.clone()))
        .collect();
    assert_eq!(
        first,
        vec![
            (Choice::Decline, t("choice.sign_in.decline")),
            (Choice::Done, t("choice.sign_in.done")),
        ]
    );
    assert_ne!(
        t("choice.sign_in.decline"),
        t("choice.sign_in.bind_decline")
    );
    let http = Origin::parse("http://pay.site1.example:80").unwrap();
    let item = compose_bind(&item_id(1), &agent_id(1), NAME, &http).unwrap();
    assert_eq!(
        item.text(),
        words::text("item.sign_in.bind", &[("origin", "pay.site1.example")]).unwrap()
    );
}

/// A second step that the person does not answer in time lapses as a confirmation does: the
/// waiting call is answered `decline` through `Runs::answer_bind`, then `item_lapsed` is stored,
/// then the run is stopped in the guard's table and through `Runs::cancel` (I3.1, ruling IG-18).
#[test]
fn a_lapse_of_the_second_step_answers_decline_before_it_stops_the_run() {
    let p = ports();
    let mut l = ledger();
    let mut t = Table::new();
    let mut held = raise_bind(&p, &mut l, &t, &LIMITS, &ask()).unwrap();
    let limit = NOW + LIMITS.wait_limit_s * 1000;
    assert!(!bind_due(&held, limit - 1, &LIMITS));
    assert!(bind_due(&held, limit, &LIMITS));
    let before = p.calls.borrow().len();
    lapse_bind(
        &p,
        &mut l,
        &mut t,
        &mut held,
        OtherLapse::WaitLimit,
        &LIMITS,
    )
    .unwrap();
    let calls: Vec<Call> = p.calls.borrow()[before..]
        .iter()
        .map(|c| c.call.clone())
        .collect();
    let lapsed = What::ItemLapsed {
        item: held.item().clone(),
        lapsed: Lapsed::SignIn(OtherLapse::WaitLimit),
    };
    assert!(
        matches!(
            &calls[..],
            [Call::AnswerBind(r, BindResult::Decline), Call::Log(rows), Call::Cancel(c)]
                if r == &run_id(1) && rows.len() == 1 && rows[0].what == lapsed && c == &run_id(1)
        ),
        "{calls:?}"
    );
    assert!(t.is_stopped(&run_id(1)));
    assert_eq!(held.state(), &BindState::Closed);
    assert!(
        !bind_due(&held, limit, &LIMITS),
        "a closed step is due no more"
    );
    assert_eq!(
        answer_bind(&p, &mut l, &t, &LIMITS, &mut held, Choice::Allow),
        Err(Cause::Stale)
    );
    assert_eq!(
        lapse_bind(
            &p,
            &mut l,
            &mut t,
            &mut held,
            OtherLapse::WaitLimit,
            &LIMITS
        ),
        Err(Cause::Stale)
    );
    for cause in [OtherLapse::RunEnded, OtherLapse::AppEnded] {
        let q = ports();
        let mut l = ledger();
        let mut t = Table::new();
        let mut held = raise_bind(&q, &mut l, &t, &LIMITS, &ask()).unwrap();
        let before = q.calls.borrow().len();
        lapse_bind(&q, &mut l, &mut t, &mut held, cause, &LIMITS).unwrap();
        let calls: Vec<Call> = q.calls.borrow()[before..]
            .iter()
            .map(|c| c.call.clone())
            .collect();
        assert!(
            matches!(&calls[..], [Call::Log(rows)] if matches!(rows[0].what, What::ItemLapsed { lapsed: Lapsed::SignIn(c), .. } if c == cause)),
            "{cause}: a run that ended or an earlier life gets no answer and no stop: {calls:?}"
        );
        assert!(!t.is_stopped(&run_id(1)), "{cause}");
    }
}

/// An `answer_bind` refused with `ended` is dropped: the row stays stored, nothing more is stored
/// or taken back, and the item closes; a lapse whose answer is refused with `ended` still stores
/// the lapse and stops the run (I5.10, ruling IG-18). Any other refusal keeps the row's answer:
/// a later answer of the same choice sends it again with no second row; `decline` after a stored
/// `allow` stores a row of its own and then sends `decline`; `allow` after a stored `decline` is
/// refused with `state`, and nothing is stored or sent (ruling RD-46 (2) c, d).
#[test]
fn an_answer_bind_refused_with_ended_is_dropped_and_after_another_refusal_dont_allow_still_decides()
{
    let p = ports();
    let mut l = ledger();
    let mut t = Table::new();
    let mut held = raise_bind(&p, &mut l, &t, &LIMITS, &ask()).unwrap();
    p.end_runner(&run_id(1));
    assert_eq!(
        answer_bind(&p, &mut l, &t, &LIMITS, &mut held, Choice::Allow),
        Ok(())
    );
    assert_eq!(held.state(), &BindState::Closed);
    assert_eq!(stored(&p).len(), 2);
    assert_eq!(stored(&p)[1], row(&held, BindResult::Allow));
    let last = tried(&p).last().cloned().unwrap();
    assert!(
        matches!(&last.call, Call::AnswerBind(_, BindResult::Allow))
            && last.refused == Some(PortError::Refused(Name::parse("ended").unwrap()))
    );
    let mut lapsing = raise_bind(&p, &mut l, &t, &LIMITS, &ask()).unwrap();
    lapse_bind(
        &p,
        &mut l,
        &mut t,
        &mut lapsing,
        OtherLapse::WaitLimit,
        &LIMITS,
    )
    .unwrap();
    assert!(matches!(
        stored(&p).last(),
        Some(What::ItemLapsed {
            lapsed: Lapsed::SignIn(OtherLapse::WaitLimit),
            ..
        })
    ));
    assert!(t.is_stopped(&run_id(1)));

    let q = ports();
    let mut l = ledger();
    let t = Table::new();
    let mut held = raise_bind(&q, &mut l, &t, &LIMITS, &ask()).unwrap();
    q.refuse(Method::AnswerBind, PortError::Busy);
    assert_eq!(
        answer_bind(&q, &mut l, &t, &LIMITS, &mut held, Choice::Allow),
        Err(Cause::Busy)
    );
    assert_eq!(held.state(), &BindState::Recorded(BindResult::Allow));
    q.clear_refusals();
    answer_bind(&q, &mut l, &t, &LIMITS, &mut held, Choice::Allow).unwrap();
    assert_eq!(held.state(), &BindState::Closed);
    assert_eq!(answers(&q), [BindResult::Allow], "one row for one answer");
    assert_eq!(sent(&q), [BindResult::Allow]);

    let q = ports();
    let mut l = ledger();
    let mut held = raise_bind(&q, &mut l, &t, &LIMITS, &ask()).unwrap();
    q.refuse(Method::AnswerBind, PortError::Busy);
    assert_eq!(
        answer_bind(&q, &mut l, &t, &LIMITS, &mut held, Choice::Allow),
        Err(Cause::Busy)
    );
    q.clear_refusals();
    assert_eq!(
        answer_bind(&q, &mut l, &t, &LIMITS, &mut held, Choice::Decline),
        Ok(()),
        "the stored row says allow, and the person's press of decline decides"
    );
    assert_eq!(held.state(), &BindState::Closed);
    assert_eq!(answers(&q), [BindResult::Allow, BindResult::Decline]);
    assert_eq!(sent(&q), [BindResult::Decline]);

    let q = ports();
    let mut l = ledger();
    let mut held = raise_bind(&q, &mut l, &t, &LIMITS, &ask()).unwrap();
    q.refuse(Method::AnswerBind, PortError::Busy);
    assert_eq!(
        answer_bind(&q, &mut l, &t, &LIMITS, &mut held, Choice::Decline),
        Err(Cause::Busy)
    );
    q.clear_refusals();
    let before = q.tried.borrow().len();
    assert_eq!(
        answer_bind(&q, &mut l, &t, &LIMITS, &mut held, Choice::Allow),
        Err(Cause::State),
        "the stored row says decline"
    );
    assert_eq!(
        q.tried.borrow().len(),
        before,
        "nothing stored and nothing sent"
    );
    assert_eq!(held.state(), &BindState::Recorded(BindResult::Decline));
    assert_eq!(answers(&q), [BindResult::Decline]);
}

fn answers(p: &Ports<Config>) -> Vec<BindResult> {
    stored(p)
        .iter()
        .filter_map(|w| match w {
            What::ItemAnswered {
                answer: Answered::SignIn(SignInAnswer::Second(b)),
                ..
            } => Some(b.answer),
            _ => None,
        })
        .collect()
}

fn sent(p: &Ports<Config>) -> Vec<BindResult> {
    p.calls
        .borrow()
        .iter()
        .filter_map(|c| match c.call {
            Call::AnswerBind(_, r) => Some(r),
            _ => None,
        })
        .collect()
}

/// A second step whose row of `allow` is stored and whose answer the run did not take: once the
/// run is stopped in the guard's table, a later answer sends nothing; at `wait_limit` and at the
/// person's stop it lapses as an open step does, with `decline` to the waiting call first, then
/// `item_lapsed`, then the stop; when its run ends, or at the quit, it lapses with `item_lapsed`
/// alone; each lapse closes it (I3.1, I5.7, ruling RD-46 (2) e).
#[test]
fn a_stored_answer_that_the_run_did_not_take_is_never_sent_to_a_stopped_run_and_lapses_as_an_open_step(
) {
    let recorded = |p: &Ports<Config>, l: &mut Ledger| {
        let mut held = raise_bind(p, l, &Table::new(), &LIMITS, &ask()).unwrap();
        p.refuse(Method::AnswerBind, PortError::Busy);
        assert_eq!(
            answer_bind(p, l, &Table::new(), &LIMITS, &mut held, Choice::Allow),
            Err(Cause::Busy)
        );
        p.clear_refusals();
        assert_eq!(held.state(), &BindState::Recorded(BindResult::Allow));
        held
    };
    let p = ports();
    let mut l = ledger();
    let mut held = recorded(&p, &mut l);
    let stopped = Table::new().stop(&run_id(1));
    let before = p.tried.borrow().len();
    assert_eq!(
        answer_bind(&p, &mut l, &stopped, &LIMITS, &mut held, Choice::Allow),
        Err(Cause::State)
    );
    assert_eq!(p.tried.borrow().len(), before, "no port was called");
    assert_eq!(held.state(), &BindState::Recorded(BindResult::Allow));

    for cause in [OtherLapse::WaitLimit, OtherLapse::Cancelled] {
        let q = ports();
        let mut l = ledger();
        let mut t = Table::new();
        let mut held = recorded(&q, &mut l);
        let limit = held.raised_at() + LIMITS.wait_limit_s * 1000;
        assert!(!bind_due(&held, limit - 1, &LIMITS), "{cause}");
        assert!(
            bind_due(&held, limit, &LIMITS),
            "{cause}: due as an open step"
        );
        let before = q.calls.borrow().len();
        lapse_bind(&q, &mut l, &mut t, &mut held, cause, &LIMITS).unwrap();
        let calls: Vec<Call> = q.calls.borrow()[before..]
            .iter()
            .map(|c| c.call.clone())
            .collect();
        assert!(
            matches!(
                &calls[..],
                [Call::AnswerBind(r, BindResult::Decline), Call::Log(rows), Call::Cancel(c)]
                    if r == &run_id(1) && c == &run_id(1) && matches!(rows[0].what, What::ItemLapsed { lapsed: Lapsed::SignIn(k), .. } if k == cause)
            ),
            "{cause}: {calls:?}"
        );
        assert!(t.is_stopped(&run_id(1)), "{cause}");
        assert_eq!(held.state(), &BindState::Closed, "{cause}");
    }
    for cause in [OtherLapse::RunEnded, OtherLapse::AppEnded] {
        let q = ports();
        let mut l = ledger();
        let mut t = Table::new();
        let mut held = recorded(&q, &mut l);
        let before = q.calls.borrow().len();
        lapse_bind(&q, &mut l, &mut t, &mut held, cause, &LIMITS).unwrap();
        let calls: Vec<Call> = q.calls.borrow()[before..]
            .iter()
            .map(|c| c.call.clone())
            .collect();
        assert!(
            matches!(&calls[..], [Call::Log(rows)] if matches!(rows[0].what, What::ItemLapsed { lapsed: Lapsed::SignIn(c), .. } if c == cause)),
            "{cause}: {calls:?}"
        );
        assert_eq!(held.state(), &BindState::Closed, "{cause}");
        assert!(!t.is_stopped(&run_id(1)), "{cause}");
        assert_eq!(
            answer_bind(&q, &mut l, &t, &LIMITS, &mut held, Choice::Allow),
            Err(Cause::Stale),
            "{cause}: the closed step sends nothing more"
        );
    }
}

/// A run that the guard's table holds as stopped gets no second step and no answer: nothing is
/// stored and no port is called (I4.2, I5.7). A choice that is not one of the second step's is
/// `malformed`.
#[test]
fn a_stopped_run_gets_no_second_step_and_no_answer() {
    let p = ports();
    let mut l = ledger();
    let stopped = Table::new().stop(&run_id(1));
    assert_eq!(
        raise_bind(&p, &mut l, &stopped, &LIMITS, &ask()).map(|_| ()),
        Err(Cause::State)
    );
    assert!(p.calls.borrow().is_empty());
    let t = Table::new();
    let mut held = raise_bind(&p, &mut l, &t, &LIMITS, &ask()).unwrap();
    let before = p.tried.borrow().len();
    for choice in [Choice::Done, Choice::Reject, Choice::Present] {
        assert_eq!(
            answer_bind(&p, &mut l, &t, &LIMITS, &mut held, choice),
            Err(Cause::Malformed)
        );
    }
    assert_eq!(
        answer_bind(&p, &mut l, &stopped, &LIMITS, &mut held, Choice::Allow),
        Err(Cause::State)
    );
    assert_eq!(p.tried.borrow().len(), before);
    assert_eq!(held.state(), &BindState::Open);
}

/// An answer `done` on a stopped run is `malformed`, as the doc of `answer_bind` orders its
/// refusals: a choice that is not the second step's is refused before the stop is read, and no
/// port is called (the review's N1, ruling RD-23 (c)).
#[test]
fn an_answer_done_on_a_stopped_run_is_malformed() {
    let p = ports();
    let mut l = ledger();
    let mut held = raise_bind(&p, &mut l, &Table::new(), &LIMITS, &ask()).unwrap();
    let stopped = Table::new().stop(&run_id(1));
    let before = p.tried.borrow().len();
    assert_eq!(
        answer_bind(&p, &mut l, &stopped, &LIMITS, &mut held, Choice::Done),
        Err(Cause::Malformed)
    );
    assert_eq!(p.tried.borrow().len(), before, "no port was called");
    assert_eq!(held.state(), &BindState::Open);
}

/// The memory `Runs::answer_bind` answers as the supervisor library does (I5.10): `unknown_run`
/// for a run that no start of this life made, `ended` for a run whose runner ended, and success
/// otherwise; `tried` holds every call, a refused one with its refusal.
#[test]
fn the_memory_answer_bind_answers_as_the_library_does() {
    let p: Ports<Config> = Ports::new(NOW, TODAY);
    let unknown = PortError::Refused(Name::parse("unknown_run").unwrap());
    let ended = PortError::Refused(Name::parse("ended").unwrap());
    assert_eq!(
        p.answer_bind(&run_id(1), BindResult::Allow),
        Err(unknown.clone())
    );
    p.runner_of(&run_id(1));
    assert_eq!(p.answer_bind(&run_id(1), BindResult::Decline), Ok(()));
    p.end_runner(&run_id(1));
    assert_eq!(
        p.answer_bind(&run_id(1), BindResult::Allow),
        Err(ended.clone())
    );
    let got: Vec<(Call, Option<PortError>)> =
        tried(&p).into_iter().map(|t| (t.call, t.refused)).collect();
    assert_eq!(
        got,
        vec![
            (
                Call::AnswerBind(run_id(1), BindResult::Allow),
                Some(unknown)
            ),
            (Call::AnswerBind(run_id(1), BindResult::Decline), None),
            (Call::AnswerBind(run_id(1), BindResult::Allow), Some(ended)),
        ]
    );
    assert_eq!(p.calls.borrow().len(), 1);
    assert_eq!(p.calls.borrow()[0].call.method(), "answer_bind");
}
