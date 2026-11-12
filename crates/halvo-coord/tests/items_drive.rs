#[path = "items_support.rs"]
mod support;

use halvo_coord::guard::{guard_step, EffectRecord, EffectState, Gives, Refusal, Table};
use halvo_coord::items::drive::{
    answer, answer_handback, due, flush, issue, lapse, on_screen, store, tick, Held, Ledger,
};
use halvo_coord::items::{ConfirmAnswer, ConfirmMove, ConfirmState};
use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{Clock, Owner, PortError, Rng, Runs};
use halvo_coord::types::{
    Agent, AgentId, Answer, Answered, Body, Category, Cause, Choice, Class, ConfirmAsk, Decision,
    DecisionEntry, EffectAnswer, Event, FullWorkflow, HandbackResult, LapseCause, Lapsed, Limits,
    Name, Presenter, Raised, Reply, RunId, RunItemKind, RunSpec, What, Workflow, WorkflowState,
    WorkflowTrigger,
};
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
    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        self.inner.cancel(run)
    }
    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.inner.kill(run)
    }
    fn reap(&self) -> Result<u32, PortError> {
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

#[test]
fn a_lapse_with_wait_limit_answers_reject_stores_the_lapse_and_stops_the_run() {
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
        ["cancel", "log", "answer_confirm"],
        "the decision, the lapse, the stop"
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

#[test]
fn a_lapse_with_cancelled_answers_reject_and_stops_the_run() {
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
    assert_eq!(methods, ["log", "answer_confirm", "log", "cancel"]);
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
    let page = held.page_item(&Table::new()).unwrap();
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
        &run,
        RunItemKind::SignIn,
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
            &run,
            RunItemKind::Check,
            Choice::WentThrough
        ),
        Err(Cause::NoService)
    );
    assert!(
        q.calls.borrow().is_empty(),
        "no acknowledgement, no answer to the runner"
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
            &run,
            RunItemKind::Stuck,
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
            &run,
            RunItemKind::Stuck,
            Choice::Allow
        ),
        Err(Cause::Malformed)
    );
}

/// The page item of a held confirmation, with the guard's table of this life.
fn page(held: &Held, table: &Table) -> halvo_coord::types::Item {
    held.page_item(table).expect("the item composes")
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
    .expect("a record uses the row of effect 2");
    answered(&p, &set, &mut l, &mut t, &earlier(3), ConfirmAnswer::Reject).unwrap();
    p.refuse(Method::Log, PortError::NoService);
    assert_eq!(
        answered(&p, &set, &mut l, &mut t, &earlier(4), ConfirmAnswer::Allow),
        Err(Cause::NoService),
        "the rows of effect 4 get no acknowledgement and its item lapses"
    );
    p.clear_refusals();
    let later = |run: u32, n: u32| ask_of(run, 1, vec![asked(n, Class::Send, n + 100, true)]);
    for (n, line) in [(1, true), (2, false), (3, false), (4, false)] {
        let held = issue(&p, &mut l, &t, &LIMITS, &later(1, n)).unwrap();
        let item = page(&held, &t);
        let want = if line { vec![0] } else { vec![] };
        assert_eq!(changed_lines(&item), want, "effect {n}");
        assert_eq!(
            item.fields().len(),
            3 + usize::from(line),
            "the line stands above the values of effect {n}"
        );
    }
    let other_run = issue(&p, &mut l, &t, &LIMITS, &later(2, 1)).unwrap();
    assert!(
        changed_lines(&page(&other_run, &t)).is_empty(),
        "another run"
    );
}
