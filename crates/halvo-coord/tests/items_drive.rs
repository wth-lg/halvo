#[path = "items_support.rs"]
mod support;

use halvo_coord::guard::{guard_step, EffectRecord, EffectState, Gives, Refusal, Table};
use halvo_coord::items::drive::{
    answer, answer_bind, answer_handback, due, flush, issue, lapse, log_bytes, log_text, on_screen,
    raise_bind, store, tick, BindState, Held, Ledger,
};
use halvo_coord::items::{ConfirmAnswer, ConfirmMove, ConfirmState};
use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{Clock, Owner, PortError, Rng, Runs};
use halvo_coord::types::{
    Agent, AgentId, Answer, Answered, BindAnswer, BindAsk, BindResult, Body, Category, Cause,
    CheckAnswer, Choice, Class, Cleared, ConfirmAsk, Decision, DecisionEntry, DoneAnswer,
    EffectAnswer, Event, FullWorkflow, HandbackAsk, HandbackReason, HandbackResult, HoldSetCause,
    LapseCause, Lapsed, Limits, LinkAnswer, Name, Origin, OtherLapse, Presenter, Raised,
    ReadyAnswer, Reply, RunEnd, RunId, RunSpec, SetupAnswer, SignInAnswer, Trigger, What, Workflow,
    WorkflowState, WorkflowTrigger,
};
use halvo_schema::coordinator::body::CoordLog;
use std::cell::{Cell, RefCell};
use support::{agent_id, ask, ask_of, asked, field, run_id, workflow_id};

const NOW: u64 = 1_793_793_600_000;
const TODAY: u32 = 20_760;
const LIMITS: Limits = Limits::PRODUCT;
const PAGE: &[Class] = &[Class::Send, Class::Delete];
const NATIVE: &[Class] = &[Class::Payment];

fn ports() -> Ports<Config> {
    let p = Ports::new(NOW, TODAY);
    p.workflows.borrow_mut().push(Workflow::Full(FullWorkflow {
        id: workflow_id(1),
        state: WorkflowState::Active,
        category: Category::Email,
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

fn presenter_of(classes: &[Class]) -> Presenter {
    if classes == NATIVE {
        Presenter::Native
    } else {
        Presenter::Page
    }
}

/// Issues the request, reports it on screen and arms it `arm_ms` later.
fn armed<P: Owner + Runs + Clock + Rng>(
    p: &P,
    set_clock: &dyn Fn(u64),
    l: &mut Ledger,
    t: &mut Table,
    a: &ConfirmAsk,
) -> Held {
    let mut held = issue(p, l, t, &LIMITS, a).expect("an item");
    on_screen(&mut held, NOW);
    set_clock(NOW + LIMITS.arm_ms);
    tick(p, l, t, &mut held, &LIMITS).expect("a tick");
    assert_eq!(held.confirmation().state(), &ConfirmState::Armed);
    held
}

fn whats(events: &[Event]) -> Vec<&What> {
    events.iter().map(|e| &e.what).collect()
}

fn released(p: &Ports<Config>) -> Vec<DecisionEntry> {
    p.events
        .borrow()
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemReleased { entry, .. } => Some(entry.clone()),
            _ => None,
        })
        .collect()
}

fn answered_rows(events: &[Event]) -> Vec<DecisionEntry> {
    events
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemAnswered {
                answer: Answered::Confirm(entry),
                ..
            } => Some(entry.clone()),
            _ => None,
        })
        .collect()
}

fn record_of(entry: &DecisionEntry, state: EffectState, class: Class) -> EffectRecord {
    EffectRecord {
        run: run_id(1),
        agent: agent_id(1),
        effect: entry.effect.clone(),
        state,
        class,
        digest: entry.digest.clone(),
        approval: Some(entry.approval.clone()),
    }
}

/// Whether the guard lets through any record `approved` or `releasing` that names an approval
/// id of `entries`, with the effect and the digest of its entry.
fn any_passes(t: &Table, entries: &[DecisionEntry]) -> bool {
    entries.iter().any(|e| {
        [EffectState::Approved, EffectState::Releasing]
            .iter()
            .any(|s| guard_step(t, &record_of(e, *s, Class::Send), Gives::Confirm).is_ok())
    })
}

#[test]
fn refusal_no_record() {
    for classes in [PAGE, NATIVE] {
        let from = presenter_of(classes);
        let a = ask(classes);
        let p = ports();
        let (mut l, mut t) = (ledger(), Table::new());
        let mut held = armed(&p, &|n| p.set_clock(n, TODAY), &mut l, &mut t, &a);
        p.refuse(Method::Log, PortError::NoService);
        assert_eq!(
            answer(
                &p,
                &mut l,
                &mut t,
                &mut held,
                ConfirmAnswer::Allow,
                from,
                &LIMITS
            ),
            Err(Cause::NoService)
        );
        assert_eq!(
            held.confirmation().state(),
            &ConfirmState::Lapsed(LapseCause::NoRecord)
        );
        let sent = p.sent_decisions();
        assert_eq!(sent.len(), 1, "one decision went to the runner");
        assert!(
            sent[0]
                .approvals()
                .iter()
                .all(|e| e.answer == EffectAnswer::RejectOnce),
            "and it holds reject_once only"
        );
        assert!(released(&p).is_empty(), "no item_released event");
        assert!(!any_passes(&t, sent[0].approvals()));
        p.clear_refusals();
        flush(&p, &mut l, &LIMITS).expect("the waiting event goes");
        let stored = p.events.borrow().clone();
        assert!(matches!(
            whats(&stored)[..],
            [
                What::ItemRaised { .. },
                What::ItemLapsed {
                    lapsed: Lapsed::Confirm(LapseCause::NoRecord),
                    ..
                }
            ]
        ));
        assert!(released(&p).is_empty());

        let q = ports();
        let (mut l, mut t) = (ledger(), Table::new());
        let mut held = armed(&q, &|n| q.set_clock(n, TODAY), &mut l, &mut t, &a);
        answer(
            &q,
            &mut l,
            &mut t,
            &mut held,
            ConfirmAnswer::Allow,
            from,
            &LIMITS,
        )
        .expect("the same answer with the acknowledgement");
        let sent = q.sent_decisions();
        assert_eq!(sent.len(), 1);
        assert!(sent[0]
            .approvals()
            .iter()
            .all(|e| e.answer == EffectAnswer::AllowOnce));
        assert_eq!(
            released(&q),
            sent[0].approvals(),
            "item_released holds the decision"
        );
        let methods: Vec<&str> = q.calls.borrow().iter().map(|c| c.call.method()).collect();
        assert_eq!(
            methods,
            ["log", "log", "answer_confirm", "log"],
            "item_raised, the rows, the decision, then item_released"
        );
        assert!(matches!(
            held.confirmation().state(),
            ConfirmState::Released(_)
        ));
    }
}

#[test]
fn refusal_timer_allow() {
    let causes: Vec<Option<LapseCause>> = std::iter::once(None)
        .chain(LapseCause::ALL.iter().map(|c| Some(*c)))
        .collect();
    let mut runs = 0;
    for classes in [PAGE, NATIVE] {
        let a = ask(classes);
        for arm in [false, true] {
            for first in &causes {
                for second in &causes {
                    for wait in [false, true] {
                        runs += 1;
                        let p = ports();
                        let (mut l, mut t) = (ledger(), Table::new());
                        let mut held = issue(&p, &mut l, &Table::new(), &LIMITS, &a).unwrap();
                        let mut now = NOW;
                        if arm {
                            on_screen(&mut held, now);
                            now += LIMITS.arm_ms;
                            p.set_clock(now, TODAY);
                            tick(&p, &mut l, &mut t, &mut held, &LIMITS).unwrap();
                        }
                        for cause in [first, second].into_iter().flatten() {
                            let _ = lapse(&p, &mut l, &mut t, &mut held, *cause, &LIMITS);
                        }
                        if wait {
                            now += LIMITS.wait_limit_s * 1000;
                            p.set_clock(now, TODAY);
                            let _ = tick(&p, &mut l, &mut t, &mut held, &LIMITS);
                        }
                        for d in p.sent_decisions() {
                            assert!(
                                d.approvals()
                                    .iter()
                                    .all(|e| e.answer == EffectAnswer::RejectOnce),
                                "arm {arm}, {first:?}, {second:?}, wait {wait}"
                            );
                        }
                        assert!(released(&p).is_empty());
                        assert!(t.approvals().is_empty(), "no row in the guard's table");
                    }
                }
            }
        }
    }
    assert_eq!(runs, 2 * 2 * 7 * 7 * 2);
}

/// Memory ports whose owner can store a body and then send no acknowledgement, and whose
/// runner can take the rows and then fail to send the decision.
struct Faulty<'a> {
    inner: &'a Ports<Config>,
    silent: Cell<bool>,
    send_fails: Cell<bool>,
    clock_fails: Cell<bool>,
    minted: Cell<u32>,
    sent: RefCell<Vec<Decision>>,
}

impl<'a> Faulty<'a> {
    fn new(inner: &'a Ports<Config>) -> Faulty<'a> {
        Faulty {
            inner,
            silent: Cell::new(false),
            send_fails: Cell::new(false),
            clock_fails: Cell::new(false),
            minted: Cell::new(100),
            sent: RefCell::new(Vec::new()),
        }
    }

    fn mint(&self, answers: &[Answer], reject: bool) -> Decision {
        Decision::new(
            answers
                .iter()
                .map(|a| {
                    self.minted.set(self.minted.get() + 1);
                    DecisionEntry {
                        approval: support::approval(self.minted.get()),
                        effect: a.effect.clone(),
                        digest: a.digest.clone(),
                        answer: if reject {
                            EffectAnswer::RejectOnce
                        } else {
                            a.answer
                        },
                    }
                })
                .collect(),
        )
        .unwrap()
    }
}

impl Owner for Faulty<'_> {
    type Config = Config;
    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.inner.log(events)?;
        if self.silent.get() {
            Err(PortError::NoService)
        } else {
            Ok(())
        }
    }
    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        self.inner.history(agent, after)
    }
    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.inner.workflows()
    }
    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        self.inner.agents()
    }
    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        self.inner.send(body)
    }
}

impl Runs for Faulty<'_> {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.inner.start(spec)
    }
    fn answer_confirm(
        &self,
        _run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        let decision = self.mint(answers, false);
        match record(&decision) {
            Ok(()) if self.send_fails.get() => {
                Err(PortError::Refused(Name::parse("ended").unwrap()))
            }
            Ok(()) => {
                self.sent.borrow_mut().push(decision);
                Ok(())
            }
            Err(e) => {
                let fresh = self.mint(answers, true);
                self.sent.borrow_mut().push(fresh);
                Err(e)
            }
        }
    }
    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        self.inner.answer_handback(run, result)
    }
    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        self.inner.answer_bind(run, result)
    }
    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        self.inner.cancel(run)
    }
    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.inner.kill(run)
    }
    fn reap(&self) -> Result<Vec<(RunId, halvo_coord::ports::Reaped)>, PortError> {
        self.inner.reap()
    }
}

impl Clock for Faulty<'_> {
    fn now(&self) -> Result<u64, PortError> {
        if self.clock_fails.get() {
            return Err(PortError::NoService);
        }
        self.inner.now()
    }
    fn today(&self) -> Result<u32, PortError> {
        self.inner.today()
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.inner.day(at)
    }
}

impl Rng for Faulty<'_> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.inner.bytes16()
    }
}

/// The owner stores the rows and stays silent: the answer is not released, the item lapses
/// with `no_record`, the decision that the runner gets holds reject_once under other ids, and
/// neither this life's guard nor the empty table of a new life lets a record through, although
/// the store holds the rows.
#[test]
fn an_answer_whose_rows_get_no_acknowledgement_is_not_released() {
    for classes in [PAGE, NATIVE] {
        let from = presenter_of(classes);
        let inner = ports();
        let p = Faulty::new(&inner);
        let (mut l, mut t) = (ledger(), Table::new());
        let a = ask(classes);
        let mut held = armed(&p, &|n| inner.set_clock(n, TODAY), &mut l, &mut t, &a);
        p.silent.set(true);
        assert_eq!(
            answer(
                &p,
                &mut l,
                &mut t,
                &mut held,
                ConfirmAnswer::Allow,
                from,
                &LIMITS
            ),
            Err(Cause::NoService)
        );
        assert_eq!(
            held.confirmation().state(),
            &ConfirmState::Lapsed(LapseCause::NoRecord)
        );
        let stored = answered_rows(&inner.events.borrow());
        assert_eq!(stored.len(), classes.len(), "the store wrote the rows");
        assert!(stored.iter().all(|e| e.answer == EffectAnswer::AllowOnce));
        let sent = p.sent.borrow().clone();
        assert_eq!(sent.len(), 1);
        assert!(sent[0]
            .approvals()
            .iter()
            .all(|e| e.answer == EffectAnswer::RejectOnce
                && stored.iter().all(|s| s.approval != e.approval)));
        assert!(released(&inner).is_empty());
        assert!(!any_passes(&t, &stored), "this life's guard");
        assert!(!any_passes(&Table::new(), &stored), "a new life's guard");
        p.silent.set(false);
        flush(&p, &mut l, &LIMITS).expect("the lapse goes after a resync");
        let all = inner.events.borrow().clone();
        let lapses = all
            .iter()
            .filter(|e| matches!(e.what, What::ItemLapsed { .. }))
            .count();
        assert_eq!(lapses, 1, "item_lapsed is stored once");
        let seqs: Vec<u32> = all.iter().map(|e| e.seq.get()).collect();
        assert_eq!(seqs, (1..=all.len() as u32).collect::<Vec<_>>());
    }
}

/// The owner acknowledges the rows and the runner's library then fails to send: the item
/// stays `recorded`, its rows stand in the guard's table but count for no record, a restart
/// lapses it with `app_ended`, and nothing passes in either life.
#[test]
fn rows_acknowledged_before_a_release_that_failed_pass_nothing() {
    let inner = ports();
    let p = Faulty::new(&inner);
    let (mut l, mut t) = (ledger(), Table::new());
    let a = ask(PAGE);
    let mut held = armed(&p, &|n| inner.set_clock(n, TODAY), &mut l, &mut t, &a);
    p.send_fails.set(true);
    assert!(answer(
        &p,
        &mut l,
        &mut t,
        &mut held,
        ConfirmAnswer::Allow,
        Presenter::Page,
        &LIMITS
    )
    .is_err());
    let ConfirmState::Recorded(decision) = held.confirmation().state().clone() else {
        panic!("{:?}", held.confirmation().state());
    };
    assert_eq!(t.approvals().len(), 2, "the rows stand in the table");
    assert!(t.approvals().iter().all(|(_, counts, _)| !counts));
    assert!(!any_passes(&t, decision.approvals()), "before the release");
    assert!(p.sent.borrow().is_empty(), "no decision went to the runner");
    assert!(released(&inner).is_empty());
    lapse(&p, &mut l, &mut t, &mut held, LapseCause::AppEnded, &LIMITS).unwrap();
    assert!(t.approvals().is_empty(), "the rows of a lapsed item leave");
    assert!(!any_passes(&t, decision.approvals()));
    assert!(!any_passes(&Table::new(), decision.approvals()));
    assert_eq!(
        lapse(
            &p,
            &mut l,
            &mut t,
            &mut held,
            LapseCause::WaitLimit,
            &LIMITS
        ),
        Err(Cause::State)
    );
}

#[test]
fn an_answer_with_no_clock_leaves_the_item_armed_and_calls_no_port() {
    let inner = ports();
    let p = Faulty::new(&inner);
    let (mut l, mut t) = (ledger(), Table::new());
    let a = ask(PAGE);
    let mut held = armed(&p, &|n| inner.set_clock(n, TODAY), &mut l, &mut t, &a);
    let before = inner.calls.borrow().len();
    p.clock_fails.set(true);
    assert_eq!(
        answer(
            &p,
            &mut l,
            &mut t,
            &mut held,
            ConfirmAnswer::Allow,
            Presenter::Page,
            &LIMITS
        ),
        Err(Cause::NoService)
    );
    assert_eq!(held.confirmation().state(), &ConfirmState::Armed);
    assert_eq!(inner.calls.borrow().len(), before);
    assert!(p.sent.borrow().is_empty());
    p.clock_fails.set(false);
    answer(
        &p,
        &mut l,
        &mut t,
        &mut held,
        ConfirmAnswer::Allow,
        Presenter::Page,
        &LIMITS,
    )
    .expect("the answer once the clock is back");
}

/// A stopped run gets no further item and no decision that allows (I4.2, L6): an answer for
/// an item of a run that the table holds as stopped, and a request of a run whose item lapsed
/// with wait_limit or cancelled, are refused with state and reach no port.
/// `issue` raises no item for a request that names an effect that has begun or whose newest
/// released decision is `reject_once`: it is refused with `state`, nothing is stored and nothing
/// sent, so that no card shows it even where the caller did not ask `raise_confirm` first (I4.7,
/// ruling IG-32). The record whose acknowledgement sets the fact is one that the guard passed.
#[test]
fn no_item_is_issued_for_an_effect_that_has_begun_or_whose_newest_decision_rejects_it() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let mut held = armed(&p, &|n| p.set_clock(n, TODAY), &mut l, &mut t, &ask(PAGE));
    answer(
        &p,
        &mut l,
        &mut t,
        &mut held,
        ConfirmAnswer::Allow,
        Presenter::Page,
        &LIMITS,
    )
    .unwrap();
    let entry = released(&p)[0].clone();
    let rec = record_of(&entry, EffectState::Releasing, PAGE[0]);
    t = guard_step(&t, &rec, Gives::Confirm).unwrap();
    let again = ask_of(1, 1, vec![asked(1, Class::Send, 7, true)]);
    let before = p.calls.borrow().len();
    assert!(
        issue(&p, &mut l, &t, &LIMITS, &again).is_ok(),
        "passed, not stored"
    );
    t = t.stored(&rec);
    let before_refused = p.calls.borrow().len();
    assert_eq!(
        issue(&p, &mut l, &t, &LIMITS, &again).err(),
        Some(Cause::State)
    );
    assert_eq!(
        p.calls.borrow().len(),
        before_refused,
        "nothing stored or sent"
    );
    assert!(before_refused > before);
    let with_fresh = ask_of(
        1,
        1,
        vec![
            asked(9, Class::Delete, 9, true),
            asked(1, Class::Send, 8, true),
        ],
    );
    assert_eq!(
        issue(&p, &mut l, &t, &LIMITS, &with_fresh).err(),
        Some(Cause::State),
        "one begun effect among others"
    );
    assert_eq!(p.calls.borrow().len(), before_refused);

    let q = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let mut held = armed(&q, &|n| q.set_clock(n, TODAY), &mut l, &mut t, &ask(PAGE));
    answer(
        &q,
        &mut l,
        &mut t,
        &mut held,
        ConfirmAnswer::Reject,
        Presenter::Page,
        &LIMITS,
    )
    .unwrap();
    let before = q.calls.borrow().len();
    assert_eq!(
        issue(&q, &mut l, &t, &LIMITS, &again).err(),
        Some(Cause::State),
        "the newest released decision of effect 1 is reject_once"
    );
    assert_eq!(q.calls.borrow().len(), before);
    let rejected_among_fresh = ask_of(
        1,
        1,
        vec![
            asked(9, Class::Send, 9, true),
            asked(1, Class::Send, 7, true),
        ],
    );
    assert_eq!(
        issue(&q, &mut l, &t, &LIMITS, &rejected_among_fresh).err(),
        Some(Cause::State),
        "one rejected effect among others"
    );
    assert_eq!(q.calls.borrow().len(), before);
    let other = ask_of(1, 1, vec![asked(9, Class::Send, 9, true)]);
    assert!(
        issue(&q, &mut l, &t, &LIMITS, &other).is_ok(),
        "another effect"
    );
}

#[test]
fn a_stopped_run_gets_no_item_and_no_allow_decision() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let mut held = armed(&p, &|n| p.set_clock(n, TODAY), &mut l, &mut t, &ask(PAGE));
    t = t.stop(&run_id(1));
    let before = p.calls.borrow().len();
    assert_eq!(
        answer(
            &p,
            &mut l,
            &mut t,
            &mut held,
            ConfirmAnswer::Allow,
            Presenter::Page,
            &LIMITS
        ),
        Err(Cause::State)
    );
    assert_eq!(held.confirmation().state(), &ConfirmState::Armed);
    assert_eq!(p.calls.borrow().len(), before, "no port was called");
    for cause in [LapseCause::WaitLimit, LapseCause::Cancelled] {
        let q = ports();
        let (mut l, mut t) = (ledger(), Table::new());
        let mut first = issue(&q, &mut l, &t, &LIMITS, &ask(PAGE)).unwrap();
        lapse(&q, &mut l, &mut t, &mut first, cause, &LIMITS).unwrap();
        let before = q.calls.borrow().len();
        let second = ask_of(1, 1, vec![asked(3, Class::Send, 3, true)]);
        assert_eq!(
            issue(&q, &mut l, &t, &LIMITS, &second).err(),
            Some(Cause::State),
            "a request after a lapse with {cause}"
        );
        assert_eq!(
            q.calls.borrow().len(),
            before,
            "{cause}: nothing stored or sent"
        );
        assert!(q.sent_decisions().iter().all(|d| d
            .approvals()
            .iter()
            .all(|e| e.answer == EffectAnswer::RejectOnce)));
    }
}

#[test]
fn a_released_answer_lets_its_own_records_through() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let a = ask(NATIVE);
    let mut held = armed(&p, &|n| p.set_clock(n, TODAY), &mut l, &mut t, &a);
    answer(
        &p,
        &mut l,
        &mut t,
        &mut held,
        ConfirmAnswer::Allow,
        Presenter::Native,
        &LIMITS,
    )
    .unwrap();
    let entry = released(&p)[0].clone();
    let releasing = record_of(&entry, EffectState::Releasing, Class::Payment);
    assert_eq!(
        guard_step(&t, &releasing, Gives::Run).err(),
        Some(Refusal::Approval)
    );
    let approved = record_of(&entry, EffectState::Approved, Class::Payment);
    let t = guard_step(&t, &approved, Gives::Confirm).expect("the approved record");
    assert!(guard_step(&t, &releasing, Gives::Confirm).is_ok());
}

#[test]
fn a_rejection_is_recorded_and_released_and_passes_nothing() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let a = ask(PAGE);
    let mut held = armed(&p, &|n| p.set_clock(n, TODAY), &mut l, &mut t, &a);
    answer(
        &p,
        &mut l,
        &mut t,
        &mut held,
        ConfirmAnswer::Reject,
        Presenter::Page,
        &LIMITS,
    )
    .unwrap();
    let entries = released(&p);
    assert!(entries.iter().all(|e| e.answer == EffectAnswer::RejectOnce));
    assert!(!any_passes(&t, &entries));
}

#[test]
fn an_answer_that_confirm_step_refuses_reaches_no_port() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let a = ask(NATIVE);
    let mut held = issue(&p, &mut l, &Table::new(), &LIMITS, &a).unwrap();
    let before = p.calls.borrow().len();
    assert_eq!(
        answer(
            &p,
            &mut l,
            &mut t,
            &mut held,
            ConfirmAnswer::Allow,
            Presenter::Native,
            &LIMITS
        ),
        Err(Cause::Early)
    );
    on_screen(&mut held, NOW);
    p.set_clock(NOW + LIMITS.arm_ms, TODAY);
    tick(&p, &mut l, &mut t, &mut held, &LIMITS).unwrap();
    assert_eq!(
        answer(
            &p,
            &mut l,
            &mut t,
            &mut held,
            ConfirmAnswer::Allow,
            Presenter::Page,
            &LIMITS
        ),
        Err(Cause::Presenter)
    );
    answer(
        &p,
        &mut l,
        &mut t,
        &mut held,
        ConfirmAnswer::Allow,
        Presenter::Native,
        &LIMITS,
    )
    .unwrap();
    assert_eq!(
        answer(
            &p,
            &mut l,
            &mut t,
            &mut held,
            ConfirmAnswer::Allow,
            Presenter::Native,
            &LIMITS
        ),
        Err(Cause::Stale)
    );
    let decisions = p.sent_decisions();
    assert_eq!(decisions.len(), 1, "one decision for one item");
    assert!(p.calls.borrow().len() > before);
}

#[test]
fn an_item_arms_only_arm_ms_after_its_first_report_on_screen() {
    let p = ports();
    let mut held = issue(&p, &mut ledger(), &Table::new(), &LIMITS, &ask(PAGE)).unwrap();
    assert!(
        due(&held, NOW + 10 * LIMITS.arm_ms, &LIMITS).is_none(),
        "never reported"
    );
    on_screen(&mut held, NOW + 7);
    on_screen(&mut held, NOW + 400);
    assert_eq!(
        held.on_screen_at(),
        Some(NOW + 7),
        "the first report counts"
    );
    assert!(due(&held, NOW + 7 + LIMITS.arm_ms - 1, &LIMITS).is_none());
    assert!(matches!(
        due(&held, NOW + 7 + LIMITS.arm_ms, &LIMITS),
        Some(ConfirmMove::Arm)
    ));
    let late = NOW + LIMITS.wait_limit_s * 1000;
    assert!(matches!(
        due(&held, late, &LIMITS),
        Some(ConfirmMove::Lapse(LapseCause::WaitLimit))
    ));
    assert!(due(&held, late - 1, &LIMITS).is_some_and(|m| matches!(m, ConfirmMove::Arm)));
}

/// Turned by ruling RD-83 from `a_lapse_with_wait_limit_answers_reject_stores_the_lapse_and_stops_the_run`:
/// the stop and the cancel come before the decision, and the row last.
#[test]
fn a_lapse_with_wait_limit_stops_the_run_then_answers_reject_then_stores_the_lapse() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let first = ask(PAGE);
    let mut one = armed(&p, &|n| p.set_clock(n, TODAY), &mut l, &mut t, &first);
    answer(
        &p,
        &mut l,
        &mut t,
        &mut one,
        ConfirmAnswer::Allow,
        Presenter::Page,
        &LIMITS,
    )
    .unwrap();
    let allowed = released(&p);
    assert!(any_passes(&t, &allowed), "the first item's rows count");
    let second = ask_of(1, 1, vec![asked(3, Class::Send, 3, true)]);
    let mut two = issue(&p, &mut l, &Table::new(), &LIMITS, &second).unwrap();
    p.set_clock(two.issued_at() + LIMITS.wait_limit_s * 1000, TODAY);
    tick(&p, &mut l, &mut t, &mut two, &LIMITS).unwrap();
    assert_eq!(
        two.confirmation().state(),
        &ConfirmState::Lapsed(LapseCause::WaitLimit)
    );
    let tail: Vec<&str> = p
        .calls
        .borrow()
        .iter()
        .rev()
        .take(3)
        .map(|c| c.call.method())
        .collect();
    assert_eq!(
        tail,
        ["log", "answer_confirm", "cancel"],
        "the stop, the decision, the lapse"
    );
    assert!(t.is_stopped(&run_id(1)));
    assert!(
        !any_passes(&t, &allowed),
        "no record of a stopped run passes"
    );
}

#[test]
fn a_lapse_of_a_run_that_ended_sends_nothing() {
    for cause in [LapseCause::RunEnded, LapseCause::AppEnded] {
        let p = ports();
        let (mut l, mut t) = (ledger(), Table::new());
        let mut held = issue(&p, &mut l, &Table::new(), &LIMITS, &ask(PAGE)).unwrap();
        lapse(&p, &mut l, &mut t, &mut held, cause, &LIMITS).unwrap();
        assert!(p.sent_decisions().is_empty(), "{cause}");
        let methods: Vec<&str> = p.calls.borrow().iter().map(|c| c.call.method()).collect();
        assert_eq!(methods, ["log", "log"], "{cause}: item_raised, item_lapsed");
        assert!(!t.is_stopped(&run_id(1)));
    }
}

/// Turned by ruling RD-83 from `a_lapse_with_cancelled_answers_reject_and_stops_the_run`: the
/// cancel comes before the decision, and the row last.
#[test]
fn a_lapse_with_cancelled_stops_the_run_then_answers_reject_then_stores_the_lapse() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let mut held = issue(&p, &mut l, &Table::new(), &LIMITS, &ask(PAGE)).unwrap();
    lapse(
        &p,
        &mut l,
        &mut t,
        &mut held,
        LapseCause::Cancelled,
        &LIMITS,
    )
    .unwrap();
    let methods: Vec<&str> = p.calls.borrow().iter().map(|c| c.call.method()).collect();
    assert_eq!(methods, ["log", "cancel", "answer_confirm", "log"]);
    assert!(t.is_stopped(&run_id(1)));
}

#[test]
fn a_request_whose_digest_does_not_hold_is_not_shown() {
    let p = ports();
    let (mut l, _t) = (ledger(), Table::new());
    let a = ask_of(
        1,
        1,
        vec![
            asked(1, Class::Send, 1, true),
            asked(2, Class::Send, 2, false),
        ],
    );
    let held = issue(&p, &mut l, &Table::new(), &LIMITS, &a).unwrap();
    assert_eq!(
        held.confirmation().state(),
        &ConfirmState::Lapsed(LapseCause::Digest)
    );
    let sent = p.sent_decisions();
    assert_eq!(sent.len(), 1);
    assert!(sent[0]
        .approvals()
        .iter()
        .all(|e| e.answer == EffectAnswer::RejectOnce));
    let stored = p.events.borrow().clone();
    assert!(matches!(
        whats(&stored)[..],
        [What::ItemLapsed {
            lapsed: Lapsed::Confirm(LapseCause::Digest),
            ..
        }]
    ));
}

#[test]
fn a_request_with_a_field_that_no_key_labels_raises_no_item() {
    let p = ports();
    let mut l = ledger();
    let mut bad = asked(1, Class::Send, 1, true);
    bad.fields
        .push(field("note", halvo_coord::types::BoundFrom::Model, "x"));
    let a = ask_of(1, 1, vec![bad]);
    assert_eq!(
        issue(&p, &mut l, &Table::new(), &LIMITS, &a).err(),
        Some(Cause::Malformed)
    );
    assert!(p.calls.borrow().is_empty(), "nothing stored, nothing sent");
}

#[test]
fn the_item_raised_names_its_run_and_its_page_item_composes() {
    let p = ports();
    let mut l = ledger();
    let held = issue(&p, &mut l, &Table::new(), &LIMITS, &ask(PAGE)).unwrap();
    let stored = p.events.borrow().clone();
    assert!(matches!(
        &stored[0].what,
        What::ItemRaised { item, raised: Raised::Confirm(run) } if item == held.item() && run == &run_id(1)
    ));
    assert_eq!(stored[0].at, (NOW * 1_000_000) as i64);
    assert_eq!(stored[0].day, TODAY);
    let page = held.page_item().unwrap();
    assert_eq!(page.item(), held.item());
    assert_eq!(page.armed().name(), "no");
}

#[test]
fn waiting_events_go_first_and_keep_their_order() {
    let p = ports();
    let mut l = ledger();
    p.refuse(Method::Log, PortError::Busy);
    let item = support::item_id(1);
    store(
        &p,
        &mut l,
        &LIMITS,
        vec![What::ItemRaised {
            item: item.clone(),
            raised: Raised::Ready,
        }],
    )
    .unwrap_err();
    assert_eq!(l.waiting().len(), 1);
    p.clear_refusals();
    store(
        &p,
        &mut l,
        &LIMITS,
        vec![What::SlotSkipped { slot: Some(TODAY) }],
    )
    .unwrap();
    let stored = p.events.borrow().clone();
    assert!(matches!(
        whats(&stored)[..],
        [What::ItemRaised { .. }, What::SlotSkipped { .. }]
    ));
    assert_eq!(l.last(), 2);
    assert!(l.waiting().is_empty());
}

/// A handback request of run 1 with its reason and origin.
fn handback(reason: HandbackReason, origin: Option<&str>) -> HandbackAsk {
    HandbackAsk {
        run: run_id(1),
        agent: agent_id(1),
        reason,
        origin: origin.map(|o| Origin::parse(o).unwrap()),
    }
}

#[test]
fn a_handback_answer_is_stored_before_the_runner_gets_it() {
    let p = ports();
    let mut l = ledger();
    let item = support::item_id(1);
    let run = run_id(1);
    answer_handback(
        &p,
        &mut l,
        &LIMITS,
        &item,
        &handback(HandbackReason::SignIn, None),
        Choice::Done,
    )
    .unwrap();
    let calls: Vec<Call> = p.calls.borrow().iter().map(|c| c.call.clone()).collect();
    assert!(matches!(
        &calls[..],
        [Call::Log(rows), Call::AnswerHandback(r, HandbackResult::Done)]
            if r == &run && matches!(rows[0].what, What::ItemAnswered { .. })
    ));
    let q = ports();
    let mut l = ledger();
    q.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        answer_handback(
            &q,
            &mut l,
            &LIMITS,
            &item,
            &handback(HandbackReason::EffectUnknown, None),
            Choice::WentThrough
        ),
        Err(Cause::NoService)
    );
    let calls: Vec<Call> = q.calls.borrow().iter().map(|c| c.call.clone()).collect();
    assert!(
        matches!(&calls[..], [Call::History(..)]),
        "no acknowledgement, one read and no answer to the runner (ruling RD-52 (c)): {calls:?}"
    );
    let e = ports();
    let mut l = ledger();
    e.refuse(
        Method::AnswerHandback,
        PortError::Refused(Name::parse("ended").unwrap()),
    );
    assert_eq!(
        answer_handback(
            &e,
            &mut l,
            &LIMITS,
            &item,
            &handback(HandbackReason::StepFailed, None),
            Choice::Decline
        ),
        Ok(()),
        "a run that ended takes nothing back"
    );
    assert_eq!(
        answer_handback(
            &e,
            &mut l,
            &LIMITS,
            &item,
            &handback(HandbackReason::StepFailed, None),
            Choice::Allow
        ),
        Err(Cause::Malformed)
    );
}

/// RD-61 (c): after a press whose row got no acknowledgement, a read of another move that brings
/// that row into the ledger ends the press's doubt, so that the next press of the same choice
/// takes the stored row as its own and goes to the run with no log of its own.
#[test]
fn rd61c_a_read_of_another_move_that_brings_the_lost_row_ends_the_doubt_of_its_press() {
    let p = ports();
    let mut l = ledger();
    let item = support::item_id(1);
    let ask = handback(HandbackReason::StepFailed, None);
    p.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        answer_handback(&p, &mut l, &LIMITS, &item, &ask, Choice::Done),
        Err(Cause::NoService)
    );
    p.clear_refusals();
    let row: Vec<Event> = p
        .tried
        .borrow()
        .iter()
        .rev()
        .find_map(|t| match (&t.call, &t.refused) {
            (Call::Log(events), Some(_)) => Some(events.clone()),
            _ => None,
        })
        .unwrap();
    p.events.borrow_mut().extend(row);
    let read: Vec<Event> = p.events.borrow().clone();
    l.take(&read);
    let tried = p.tried.borrow().len();
    assert_eq!(
        answer_handback(&p, &mut l, &LIMITS, &item, &ask, Choice::Done),
        Ok(())
    );
    let calls: Vec<Call> = p.tried.borrow()[tried..]
        .iter()
        .map(|t| t.call.clone())
        .collect();
    assert!(
        matches!(&calls[..], [Call::AnswerHandback(_, HandbackResult::Done)]),
        "the stored answer goes with no log: {calls:?}"
    );
}

/// The row of an answer of a `link_account` item holds the whole origin of its request, whose
/// site's name the item showed, before the runner gets the result; a request with no origin gets
/// no row and no result, because its item cannot show a site (I7.1, I3.4, ruling IG-14).
#[test]
fn a_link_account_answer_stores_the_whole_origin_of_its_request() {
    let item = support::item_id(1);
    let origin = "https://mail.site2.example:8443";
    for (choice, done, result) in [
        (Choice::Done, DoneAnswer::Done, HandbackResult::Done),
        (
            Choice::Decline,
            DoneAnswer::Decline,
            HandbackResult::Declined,
        ),
    ] {
        let p = ports();
        let mut l = ledger();
        let ask = handback(HandbackReason::AccountLink, Some(origin));
        answer_handback(&p, &mut l, &LIMITS, &item, &ask, choice).unwrap();
        let calls: Vec<Call> = p.calls.borrow().iter().map(|c| c.call.clone()).collect();
        let row = What::ItemAnswered {
            item: item.clone(),
            answer: Answered::LinkAccount(LinkAnswer {
                answer: done,
                origin: Origin::parse(origin).unwrap(),
            }),
        };
        assert!(
            matches!(
                &calls[..],
                [Call::Log(rows), Call::AnswerHandback(r, got)]
                    if rows.len() == 1 && rows[0].what == row && r == &run_id(1) && *got == result
            ),
            "{calls:?}"
        );
    }
    let p = ports();
    let mut l = ledger();
    assert_eq!(
        answer_handback(
            &p,
            &mut l,
            &LIMITS,
            &item,
            &handback(HandbackReason::AccountLink, None),
            Choice::Done
        ),
        Err(Cause::Malformed)
    );
    assert!(p.calls.borrow().is_empty());
}

/// The page item of a held confirmation.
fn page(held: &Held) -> halvo_coord::types::Item {
    held.page_item().expect("the item composes")
}

/// The fields of an item whose label is the sentence of `confirm.changed`, each with its index.
fn changed_lines(item: &halvo_coord::types::Item) -> Vec<usize> {
    let line = halvo_coord::words::text("confirm.changed", &[]).unwrap();
    item.fields()
        .iter()
        .enumerate()
        .filter(|(_, f)| f.label == line)
        .map(|(i, f)| {
            assert!(f.value.is_empty() && f.from.name() == "halvo");
            i
        })
        .collect()
}

/// Issues the request of `a`, arms it and answers it from its presenter.
fn answered<P: Owner + Runs + Clock + Rng>(
    p: &P,
    set_clock: &dyn Fn(u64),
    l: &mut Ledger,
    t: &mut Table,
    a: &ConfirmAsk,
    choice: ConfirmAnswer,
) -> Result<(), Cause> {
    let mut held = armed(p, set_clock, l, t, a);
    answer(p, l, t, &mut held, choice, Presenter::Page, &LIMITS)
}

/// The case `changed_line` of I4.8 as ruling IG-25 gives it: a later item of the run shows the line
/// when the newest decision released for its effect id in this run and this life is
/// `allow_once`, whether or not a record used that decision's row. Effect 1 is allowed with no
/// record; effect 2 is the flow of ruling XA-4: allowed, its record `approved` marks the row used,
/// a `begin` refused with `digest` and a `rebind` (no record of either crosses the link), then the
/// second request; effect 3 is rejected, so that a later request for it gets no item at all
/// (ruling IG-32); effect 4 has only an item that lapsed; effect 5 is allowed, then an item for it
/// lapsed, then the third request comes.
#[test]
fn refusal_changed_line() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let set = |n| p.set_clock(n, TODAY);
    let earlier = |n: u32| ask_of(1, 1, vec![asked(n, Class::Send, n, true)]);
    answered(&p, &set, &mut l, &mut t, &earlier(1), ConfirmAnswer::Allow).unwrap();
    answered(&p, &set, &mut l, &mut t, &earlier(2), ConfirmAnswer::Allow).unwrap();
    let used = released(&p)
        .into_iter()
        .find(|e| e.effect == support::effect_id(2))
        .unwrap();
    t = guard_step(
        &t,
        &record_of(&used, EffectState::Approved, Class::Send),
        Gives::Confirm,
    )
    .expect("the record approved of effect 2 uses its row");
    assert!(t
        .approvals()
        .iter()
        .any(|(a, _, u)| *a == used.approval && *u));
    answered(&p, &set, &mut l, &mut t, &earlier(3), ConfirmAnswer::Reject).unwrap();
    p.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        answered(&p, &set, &mut l, &mut t, &earlier(4), ConfirmAnswer::Allow),
        Err(Cause::NoService),
        "the rows of effect 4 get no acknowledgement and its item lapses"
    );
    p.clear_refusals();
    answered(&p, &set, &mut l, &mut t, &earlier(5), ConfirmAnswer::Allow).unwrap();
    p.refuse(Method::Log, PortError::NoService);
    let second_of_5 = ask_of(1, 1, vec![asked(5, Class::Send, 50, true)]);
    assert_eq!(
        answered(&p, &set, &mut l, &mut t, &second_of_5, ConfirmAnswer::Allow),
        Err(Cause::NoService),
        "the second item of effect 5 lapses"
    );
    p.clear_refusals();
    let later = |run: u32, n: u32| ask_of(run, 1, vec![asked(n, Class::Send, n + 100, true)]);
    assert_eq!(
        issue(&p, &mut l, &t, &LIMITS, &later(1, 3)).err(),
        Some(Cause::State),
        "effect 3, whose newest released decision is reject_once, gets no item (ruling IG-32)"
    );
    for (n, line) in [(1, true), (2, true), (4, false), (5, true)] {
        let held = issue(&p, &mut l, &t, &LIMITS, &later(1, n)).unwrap();
        let item = page(&held);
        let want = if line { vec![0] } else { vec![] };
        assert_eq!(changed_lines(&item), want, "effect {n}");
        assert_eq!(
            item.fields().len(),
            3 + usize::from(line),
            "the line stands above the values of effect {n}"
        );
    }
    let other_run = issue(&p, &mut l, &t, &LIMITS, &later(2, 1)).unwrap();
    assert!(changed_lines(&page(&other_run)).is_empty(), "another run");
    let new_life = issue(&p, &mut l, &Table::new(), &LIMITS, &later(1, 1)).unwrap();
    assert!(changed_lines(&page(&new_life)).is_empty(), "a new life");
}

/// The line names an earlier item of the run: the item's own release of `allow_once` gives no
/// line on its own card, whenever the page composes it (review finding F1).
#[test]
fn an_item_shows_no_line_for_its_own_release() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let a = ask_of(1, 1, vec![asked(1, Class::Send, 1, true)]);
    let mut held = armed(&p, &|n| p.set_clock(n, TODAY), &mut l, &mut t, &a);
    answer(
        &p,
        &mut l,
        &mut t,
        &mut held,
        ConfirmAnswer::Allow,
        Presenter::Page,
        &LIMITS,
    )
    .unwrap();
    assert!(t.newest_allows(&run_id(1), &support::effect_id(1)));
    assert!(changed_lines(&page(&held)).is_empty());
}

/// An item issued before another item of the run released `allow_once` for the same effect
/// shows no line for that later release (review finding F2).
#[test]
fn an_item_shows_no_line_for_a_later_items_release() {
    let p = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let first = ask_of(1, 1, vec![asked(1, Class::Send, 1, true)]);
    let second = ask_of(1, 1, vec![asked(1, Class::Send, 2, true)]);
    let a = issue(&p, &mut l, &t, &LIMITS, &first).unwrap();
    assert!(changed_lines(&page(&a)).is_empty());
    answered(
        &p,
        &|n| p.set_clock(n, TODAY),
        &mut l,
        &mut t,
        &second,
        ConfirmAnswer::Allow,
    )
    .unwrap();
    assert!(t.newest_allows(&run_id(1), &support::effect_id(1)));
    assert!(changed_lines(&page(&a)).is_empty());
}

/// The end of the local day `TODAY` in the zone of `ports()`.
const MIDNIGHT: u64 = NOW + 12 * 3_600_000;

/// For each start from 8 ms before local midnight to 3 ms after it: fresh ports, `before` at
/// `NOW`, then the clock at the start, moving on 1 ms at each read of `now`, and `make`. Every
/// row that the owner stored carries the local day of its `at` (I7.1), and rows of both days
/// were stored.
fn across_midnight<S>(before: &dyn Fn(&Ports<Config>) -> S, make: &dyn Fn(&Ports<Config>, S)) {
    let mut days = Vec::new();
    for start in MIDNIGHT - 8..MIDNIGHT + 4 {
        let p = ports();
        assert_eq!(p.day(MIDNIGHT - 1), Ok(TODAY));
        assert_eq!(p.day(MIDNIGHT), Ok(TODAY + 1));
        let state = before(&p);
        p.set_now(start);
        p.step_reads(1);
        make(&p, state);
        let events = p.events.borrow();
        assert!(!events.is_empty(), "no row from {start}");
        for e in events.iter() {
            let at = u64::try_from(e.at / 1_000_000).unwrap();
            assert_eq!(p.day(at), Ok(e.day), "{:?} from {start}", e.what);
            days.push(e.day);
        }
    }
    assert!(days.contains(&TODAY) && days.contains(&(TODAY + 1)));
}

#[test]
fn a_row_of_store_carries_the_day_of_its_at_across_midnight() {
    across_midnight(&|_| ledger(), &|p, mut l| {
        store(p, &mut l, &LIMITS, vec![What::SlotSkipped { slot: None }]).unwrap();
    });
}

#[test]
fn the_rows_of_a_confirmation_answer_carry_the_day_of_their_at_across_midnight() {
    across_midnight(
        &|p| {
            let (mut l, mut t) = (ledger(), Table::new());
            let held = armed(p, &|n| p.set_clock(n, TODAY), &mut l, &mut t, &ask(PAGE));
            (l, t, held)
        },
        &|p, (mut l, mut t, mut held)| {
            let answered = ConfirmAnswer::Allow;
            answer(
                p,
                &mut l,
                &mut t,
                &mut held,
                answered,
                Presenter::Page,
                &LIMITS,
            )
            .unwrap();
            assert_eq!(released(p).len(), ask(PAGE).effects().len());
        },
    );
}

#[test]
fn a_handback_answer_row_carries_the_day_of_its_at_across_midnight() {
    across_midnight(&|_| ledger(), &|p, mut l| {
        let ask = handback(HandbackReason::SignIn, None);
        let item = support::item_id(1);
        answer_handback(p, &mut l, &LIMITS, &item, &ask, Choice::Done).unwrap();
    });
}

#[test]
fn a_second_step_answer_row_carries_the_day_of_its_at_across_midnight() {
    let bind = BindAsk {
        run: run_id(1),
        agent: agent_id(1),
        origin: Origin::parse("https://pay.site1.example:8443").unwrap(),
    };
    across_midnight(
        &|p| {
            let mut l = ledger();
            let held = raise_bind(p, &mut l, &Table::new(), &LIMITS, &bind).unwrap();
            (l, held)
        },
        &|p, (mut l, mut held)| {
            let t = Table::new();
            answer_bind(p, &mut l, &t, &LIMITS, &mut held, Choice::Allow).unwrap();
            assert_eq!(held.state(), &BindState::Closed);
        },
    );
}

/// An origin of the longest form that `Origin::parse` takes: a host of 253 bytes and a port of
/// five digits, 267 bytes in all.
fn longest_origin() -> Origin {
    let host = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    assert_eq!(host.len(), 253);
    Origin::parse(&format!("https://{host}:65535")).unwrap()
}

fn long_link(n: u32) -> What {
    What::ItemAnswered {
        item: support::item_id(n),
        answer: Answered::LinkAccount(LinkAnswer {
            answer: DoneAnswer::Done,
            origin: longest_origin(),
        }),
    }
}

fn logged_bodies(p: &Ports<Config>) -> Vec<Vec<Event>> {
    p.calls
        .borrow()
        .iter()
        .filter_map(|c| match &c.call {
            Call::Log(rows) => Some(rows.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn eight_rows_with_long_origins_are_cut_into_bodies_under_request_bytes() {
    let p = ports();
    let mut l = ledger();
    p.refuse(Method::Log, PortError::NoService);
    for n in 1..=8 {
        assert!(store(&p, &mut l, &LIMITS, vec![long_link(n)]).is_err());
    }
    assert_eq!(l.waiting().len(), 8);
    let bytes = LIMITS.request_bytes as usize;
    assert!(
        log_bytes(l.waiting()) > bytes,
        "eight such rows break the bound"
    );
    p.clear_refusals();
    flush(&p, &mut l, &LIMITS).unwrap();
    assert!(l.waiting().is_empty());
    assert_eq!(l.last(), 8);
    let bodies = logged_bodies(&p);
    assert!(bodies.len() >= 2, "{} bodies", bodies.len());
    let seqs: Vec<u32> = bodies.iter().flatten().map(|e| e.seq.get()).collect();
    assert_eq!(seqs, (1..=8).collect::<Vec<u32>>());
    let stored: Vec<u32> = p.events.borrow().iter().map(|e| e.seq.get()).collect();
    assert_eq!(stored, seqs);
    for (i, body) in bodies.iter().enumerate() {
        assert!(body.len() <= LIMITS.request_records as usize);
        assert!(
            log_bytes(body) <= bytes,
            "body {i}: {} bytes",
            log_bytes(body)
        );
        if let Some(next) = bodies.get(i + 1) {
            let mut more = body.clone();
            more.push(next[0].clone());
            assert!(log_bytes(&more) > bytes, "body {i} could take one more row");
        }
    }
}

#[test]
fn a_body_is_cut_at_request_records_and_at_request_bytes_whichever_comes_first() {
    let limits = Limits {
        request_records: 3,
        ..LIMITS
    };
    let p = Ports::with_limits(NOW, TODAY, limits);
    p.workflows.borrow_mut().extend(ports().workflows.take());
    let mut l = ledger();
    p.refuse(Method::Log, PortError::NoService);
    for _ in 0..7 {
        let _ = store(&p, &mut l, &limits, vec![What::SlotSkipped { slot: None }]);
    }
    p.clear_refusals();
    flush(&p, &mut l, &limits).unwrap();
    let sizes: Vec<usize> = logged_bodies(&p).iter().map(Vec::len).collect();
    assert_eq!(sizes, [3, 3, 1]);
    let p = ports();
    let mut l = ledger();
    p.refuse(Method::Log, PortError::NoService);
    let tight = Limits {
        request_bytes: u32::try_from(log_bytes(&[row(1, long_link(1))])).unwrap(),
        ..LIMITS
    };
    for n in 1..=3 {
        let _ = store(&p, &mut l, &tight, vec![long_link(n)]);
    }
    p.clear_refusals();
    flush(&p, &mut l, &tight).unwrap();
    let sizes: Vec<usize> = logged_bodies(&p).iter().map(Vec::len).collect();
    assert_eq!(
        sizes,
        [1, 1, 1],
        "one row per body under a bound that one row fills"
    );
}

#[test]
fn the_memory_owner_refuses_a_body_over_request_bytes_and_stores_none_of_it() {
    let p = ports();
    let rows: Vec<Event> = (1..=8).map(|n| row(n, long_link(n))).collect();
    assert!(log_bytes(&rows) > LIMITS.request_bytes as usize);
    assert_eq!(
        p.log(&rows),
        Err(PortError::Refused(Name::parse("malformed").unwrap()))
    );
    assert!(p.events.borrow().is_empty());
    let fit = (1..=rows.len())
        .take_while(|&k| log_bytes(&rows[..k]) <= LIMITS.request_bytes as usize)
        .last()
        .unwrap();
    assert!(fit < 8);
    assert_eq!(p.log(&rows[..fit]), Ok(()));
    assert_eq!(p.events.borrow().len(), fit);
}

/// The rows of one confirmation go in one body (I4.5): `request_records` such rows, with the
/// largest `seq`, `day` and `at`, stay under `request_bytes`.
#[test]
fn request_records_rows_of_a_confirmation_fit_one_body() {
    let rows: Vec<Event> = (0..LIMITS.request_records)
        .map(|n| Event {
            seq: std::num::NonZeroU32::MAX,
            day: u32::MAX,
            at: i64::MAX,
            ..row(
                1,
                What::ItemAnswered {
                    item: support::item_id(1),
                    answer: Answered::Confirm(DecisionEntry {
                        approval: support::approval(n),
                        effect: support::effect_id(n),
                        digest: support::digest(n),
                        answer: EffectAnswer::RejectOnce,
                    }),
                },
            )
        })
        .collect();
    let bytes = log_bytes(&rows);
    assert!(bytes <= LIMITS.request_bytes as usize, "{bytes} bytes");
}

fn row(seq: u32, what: What) -> Event {
    Event {
        workflow: workflow_id(1),
        agent: agent_id(1),
        seq: std::num::NonZeroU32::new(seq).unwrap(),
        day: TODAY,
        at: i64::try_from(NOW).unwrap() * 1_000_000,
        what,
    }
}

/// One event of every form of I7.1 that the coordinator writes, with the longest origin.
fn every_form() -> Vec<What> {
    let run = run_id(1);
    let entry = |answer| DecisionEntry {
        approval: support::approval(1),
        effect: support::effect_id(1),
        digest: support::digest(1),
        answer,
    };
    let item = support::item_id;
    vec![
        What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Schedule,
            slot: Some(TODAY),
        },
        What::RunStarted {
            run: run.clone(),
            trigger: Trigger::Person,
            slot: None,
        },
        What::RunEnded {
            run: run.clone(),
            cause: RunEnd::Completed,
        },
        What::ItemRaised {
            item: item(1),
            raised: Raised::Confirm(run.clone()),
        },
        What::ItemRaised {
            item: item(2),
            raised: Raised::Check(None),
        },
        What::ItemRaised {
            item: item(3),
            raised: Raised::Ready,
        },
        What::ItemAnswered {
            item: item(1),
            answer: Answered::Confirm(entry(EffectAnswer::AllowOnce)),
        },
        What::ItemAnswered {
            item: item(4),
            answer: Answered::SignIn(SignInAnswer::Second(BindAnswer {
                answer: BindResult::Allow,
                origin: longest_origin(),
            })),
        },
        What::ItemAnswered {
            item: item(5),
            answer: Answered::SignIn(SignInAnswer::First(DoneAnswer::Done)),
        },
        long_link(6),
        What::ItemAnswered {
            item: item(2),
            answer: Answered::Check(CheckAnswer::WentThrough),
        },
        What::ItemAnswered {
            item: item(7),
            answer: Answered::Setup(SetupAnswer::Choose),
        },
        What::ItemAnswered {
            item: item(3),
            answer: Answered::Ready(ReadyAnswer::Start),
        },
        What::ItemAnswered {
            item: item(8),
            answer: Answered::Stuck(DoneAnswer::Decline),
        },
        What::ItemAnswered {
            item: item(9),
            answer: Answered::Show(DoneAnswer::Done),
        },
        What::ItemLapsed {
            item: item(10),
            lapsed: Lapsed::Confirm(LapseCause::Digest),
        },
        What::ItemLapsed {
            item: item(11),
            lapsed: Lapsed::Ready(OtherLapse::WaitLimit),
        },
        What::ItemReleased {
            item: item(1),
            entry: entry(EffectAnswer::RejectOnce),
        },
        What::HoldSet {
            cause: HoldSetCause::Failures,
        },
        What::HoldCleared {
            cause: Cleared::Person,
        },
        What::HoldCleared {
            cause: Cleared::Failures,
        },
        What::HoldCleared {
            cause: Cleared::Check {
                run,
                answer: CheckAnswer::TryAgain,
            },
        },
        What::SlotSkipped { slot: Some(TODAY) },
        What::SlotSkipped { slot: None },
    ]
}

#[test]
fn log_text_is_the_text_that_halvo_schema_reads_and_writes_for_every_form() {
    let rows: Vec<Event> = every_form()
        .into_iter()
        .zip(1..)
        .map(|(what, seq)| row(seq, what))
        .collect();
    let mut bodies: Vec<Vec<Event>> = rows.iter().map(|e| vec![e.clone()]).collect();
    bodies.extend(rows.chunks(4).map(<[Event]>::to_vec));
    for body in &bodies {
        let text = log_text(&workflow_id(1), &agent_id(1), body);
        assert_eq!(log_bytes(body), text.len());
        let read = CoordLog::parse(text.as_bytes()).unwrap_or_else(|r| panic!("{r:?}: {text}"));
        assert_eq!(read.to_json().unwrap(), text);
        assert_eq!(read.records.len(), body.len());
    }
    let long = row(1, long_link(1));
    let text = log_text(&workflow_id(1), &agent_id(1), std::slice::from_ref(&long));
    assert!(text.contains(longest_origin().as_str()));
}

/// The longest record of every form, with the largest `seq`, `day`, `at` and `slot`, fits a body
/// alone with room to spare, so that a body of one record is never refused for its bytes.
#[test]
fn the_longest_record_of_every_form_fits_a_body_alone() {
    let longest = every_form()
        .into_iter()
        .map(|what| {
            let what = match what {
                What::RunStarted { run, trigger, .. } => What::RunStarted {
                    run,
                    trigger,
                    slot: Some(u32::MAX),
                },
                What::SlotSkipped { .. } => What::SlotSkipped {
                    slot: Some(u32::MAX),
                },
                other => other,
            };
            let e = Event {
                seq: std::num::NonZeroU32::MAX,
                day: u32::MAX,
                at: i64::MAX,
                ..row(1, what)
            };
            log_bytes(&[e])
        })
        .max()
        .unwrap();
    assert!(
        longest <= LIMITS.request_bytes as usize / 4,
        "{longest} bytes"
    );
}

/// The arm that ruling RD-64 keeps for its class D (2.1), pinned since RD-62 (b): a body that
/// the owner refuses with `store` stays whole and first in the ledger with every row behind it,
/// nothing behind it is sent in that flush, and the next flush sends the same body first again.
#[test]
fn a_body_refused_with_store_stays_whole_and_first_with_nothing_behind_it_sent() {
    let p = ports();
    let mut l = ledger();
    p.refuse(Method::Log, PortError::NoService);
    for n in 1..=10 {
        let _ = store(&p, &mut l, &LIMITS, vec![long_link(n)]);
    }
    let waiting: Vec<Event> = l.waiting().to_vec();
    assert_eq!(waiting.len(), 10);
    p.clear_refusals();
    let refusal = PortError::Refused(Name::parse("store").unwrap());
    p.refuse(Method::Log, refusal.clone());
    let tried_before = p.tried.borrow().len();
    assert_eq!(flush(&p, &mut l, &LIMITS), Err(refusal));
    assert_eq!(l.waiting(), &waiting[..], "the rows stay, in order");
    assert_eq!(l.last(), 0);
    let tried: Vec<Vec<Event>> = p.tried.borrow()[tried_before..]
        .iter()
        .filter_map(|t| match &t.call {
            Call::Log(rows) => Some(rows.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(tried.len(), 1, "one body tried");
    assert_eq!(tried[0][..], waiting[..tried[0].len()], "the first body");
    assert!(p.events.borrow().is_empty());
    p.clear_refusals();
    flush(&p, &mut l, &LIMITS).unwrap();
    assert_eq!(logged_bodies(&p)[0], tried[0], "the same body goes first");
    assert_eq!(p.events.borrow().len(), 10);
}

/// The memory owner takes a body of exactly `request_bytes` bytes and refuses one of a byte more
/// with `malformed`, storing nothing (I7.2).
#[test]
fn the_memory_owner_takes_a_body_of_exactly_request_bytes_and_refuses_one_byte_more() {
    let rows: Vec<Event> = (1..=3).map(|n| row(n, long_link(n))).collect();
    let exact = u32::try_from(log_bytes(&rows)).unwrap();
    for (bound, taken) in [(exact, true), (exact - 1, false)] {
        let limits = Limits {
            request_bytes: bound,
            ..LIMITS
        };
        let p = Ports::<Config>::with_limits(NOW, TODAY, limits);
        p.workflows.borrow_mut().extend(ports().workflows.take());
        let answer = p.log(&rows);
        if taken {
            assert_eq!(answer, Ok(()), "{bound} bytes");
            assert_eq!(p.events.borrow().len(), 3);
        } else {
            let malformed = PortError::Refused(Name::parse("malformed").unwrap());
            assert_eq!(answer, Err(malformed), "{bound} bytes");
            assert!(p.events.borrow().is_empty());
        }
    }
}

/// The flush puts into one body every row that fits up to the last byte of `request_bytes`.
#[test]
fn the_flush_fills_a_body_up_to_exactly_request_bytes() {
    let rows: Vec<What> = (1..=5).map(long_link).collect();
    let numbered: Vec<Event> = (1..=3).map(|n| row(n, long_link(n))).collect();
    let limits = Limits {
        request_bytes: u32::try_from(log_bytes(&numbered)).unwrap(),
        ..LIMITS
    };
    let p = ports();
    let mut l = ledger();
    p.refuse(Method::Log, PortError::NoService);
    for what in rows {
        let _ = store(&p, &mut l, &limits, vec![what]);
    }
    p.clear_refusals();
    flush(&p, &mut l, &limits).unwrap();
    let sizes: Vec<usize> = logged_bodies(&p).iter().map(Vec::len).collect();
    assert_eq!(sizes, [3, 2]);
}

/// A body holds at least one row: under a bound that one row alone breaks, the flush still sends
/// the rows one to a body and never an empty body.
#[test]
fn a_body_holds_one_row_even_under_a_bound_that_one_row_breaks() {
    let tight = Limits {
        request_bytes: 100,
        ..LIMITS
    };
    assert!(log_bytes(&[row(1, long_link(1))]) > 100);
    let p = ports();
    let mut l = ledger();
    p.refuse(Method::Log, PortError::NoService);
    for n in 1..=2 {
        let _ = store(&p, &mut l, &tight, vec![long_link(n)]);
    }
    p.clear_refusals();
    flush(&p, &mut l, &tight).unwrap();
    let sizes: Vec<usize> = logged_bodies(&p).iter().map(Vec::len).collect();
    assert_eq!(sizes, [1, 1]);
}

const GOLDEN_REQUESTS: &str = include_str!("../../halvo-schema/tests/golden/r4/requests.jsonl");

/// `log_text` of the three events of halvo-schema's golden `coord_log` request is that request's
/// body, byte for byte.
#[test]
fn log_text_writes_the_body_of_halvo_schemas_golden_coord_log_request() {
    let id = |text: &str| text.to_owned();
    let workflow =
        halvo_coord::types::WorkflowId::parse(&id("0f1e2d3c4b5a69788796a5b4c3d2e1f0")).unwrap();
    let agent = AgentId::parse(&id("1a2b3c4d5e6f708192a3b4c5d6e7f809")).unwrap();
    let run = RunId::parse(&id("9f8e7d6c5b4a39281706f5e4d3c2b1a0")).unwrap();
    let item = halvo_coord::types::ItemId::parse(&id("7e6d5c4b3a2918070f1e2d3c4b5a6971")).unwrap();
    let entry = DecisionEntry {
        approval: halvo_coord::types::ApprovalId::parse(&id("6d5c4b3a29180f7e6d5c4b3a29180f7e"))
            .unwrap(),
        effect: halvo_coord::types::EffectId::parse(&id("5c4b3a29180f7e6d5c4b3a29180f7e6d"))
            .unwrap(),
        digest: halvo_coord::types::Digest::parse(&"ab".repeat(32)).unwrap(),
        answer: EffectAnswer::AllowOnce,
    };
    let at = |s: i64| 1_794_339_000_000_000_000 + s * 1_000_000_000;
    let event = |seq: u32, at: i64, what: What| Event {
        workflow: workflow.clone(),
        agent: agent.clone(),
        seq: std::num::NonZeroU32::new(seq).unwrap(),
        day: 20_767,
        at,
        what,
    };
    let events = [
        event(
            1,
            at(60),
            What::RunStarted {
                run: run.clone(),
                trigger: Trigger::Schedule,
                slot: Some(20_767),
            },
        ),
        event(
            2,
            at(120),
            What::ItemRaised {
                item: item.clone(),
                raised: Raised::Confirm(run.clone()),
            },
        ),
        event(
            3,
            at(180),
            What::ItemAnswered {
                item,
                answer: Answered::Confirm(entry),
            },
        ),
    ];
    let text = log_text(&workflow, &agent, &events);
    let line = GOLDEN_REQUESTS
        .lines()
        .find(|l| l.contains("\"op\":\"coord_log\""))
        .unwrap();
    assert!(line.contains(&format!("\"body\":{text}}}")), "{text}");
}

/// A clock past the largest instant that a row's `at` holds stores no row: `store` answers the
/// clock's failure and nothing waits (review case review_day_1).
#[test]
fn store_at_an_instant_past_the_largest_at_stores_nothing() {
    let p = ports();
    p.set_now(i64::MAX as u64 / 1_000_000 + 1);
    let mut l = ledger();
    let stored = store(&p, &mut l, &LIMITS, vec![What::SlotSkipped { slot: None }]);
    assert_eq!(stored, Err(PortError::NoService));
    assert!(l.waiting().is_empty());
    assert!(p.events.borrow().is_empty());
}

/// A clock whose `now` answers and whose `day` fails (a zone that puts the local instant before
/// 1970) stamps no row: `store` answers the failure, stores nothing and leaves nothing waiting
/// (I7.1, I5.10; ruling RD-69 E2).
#[test]
fn a_clock_whose_day_fails_stamps_no_row() {
    let p = ports();
    let mut l = ledger();
    p.set_zone(-(NOW as i64) - 86_400_000);
    assert!(p.now().is_ok());
    assert!(p.day(NOW).is_err());
    let stored = store(&p, &mut l, &LIMITS, vec![What::SlotSkipped { slot: None }]);
    assert_eq!(stored, Err(PortError::NoService));
    assert!(p.events.borrow().is_empty());
    assert!(l.waiting().is_empty());
}

/// The measured text of a body with a `slot_skipped` row whose slot is set, against the text
/// written out here: the slot is the number of its day, not null (I7.1; ruling RD-69 E3).
#[test]
fn log_text_writes_the_slot_of_a_slot_skipped_row() {
    let e = row(1, What::SlotSkipped { slot: Some(TODAY) });
    let text = log_text(&workflow_id(1), &agent_id(1), std::slice::from_ref(&e));
    let ids = r#""workflow":"00000000000000000000000000007701","agent":"00000000000000000000000000006101""#;
    let written = format!(
        concat!(
            r#"{{"op":"coord_log",{ids},"records":[{{"v":1,{ids},"seq":1,"day":20760,"#,
            r#""at":1793793600000000000,"event":"slot_skipped","run":null,"slot":20760,"#,
            r#""item":null,"kind":null,"effect":null,"approval":null,"digest":null,"#,
            r#""answer":null,"cause":null,"trigger":null,"origin":null}}]}}"#
        ),
        ids = ids
    );
    assert_eq!(text, written);
    assert_eq!(log_bytes(std::slice::from_ref(&e)), written.len());
}
