#[path = "items_support.rs"]
mod support;

use halvo_coord::guard::{guard_step, EffectRecord, EffectState, Gives, Table};
use halvo_coord::items::drive::{
    answer, flush, issue, lapse, lapse_bind, on_screen, raise_bind, raise_run_item, tick, time_out,
    Held, Ledger,
};
use halvo_coord::items::{ConfirmAnswer, ConfirmState};
use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{Clock, Owner, PortError, Reaped, Rng, Runs};
use halvo_coord::types::{
    Agent, AgentId, Answer, BindAsk, BindResult, Body, Category, Class, Decision, DecisionEntry,
    Event, FullWorkflow, HandbackResult, LapseCause, Lapsed, Limits, Name, Origin, OtherLapse,
    Presenter, Reply, RunId, RunItemKind, RunSpec, What, Workflow, WorkflowState, WorkflowTrigger,
};
use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use support::{agent_id, ask, ask_of, asked, run_id, workflow_id};

const NOW: u64 = 1_793_793_600_000;
const TODAY: u32 = 20_760;
const LIMITS: Limits = Limits::PRODUCT;
const PAGE: &[Class] = &[Class::Send, Class::Delete];
const STOPS: [LapseCause; 2] = [LapseCause::WaitLimit, LapseCause::Cancelled];

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

struct Panicky<'a> {
    inner: &'a Ports<Config>,
    panics: Cell<Option<&'static str>>,
}

impl<'a> Panicky<'a> {
    fn new(inner: &'a Ports<Config>) -> Panicky<'a> {
        Panicky {
            inner,
            panics: Cell::new(None),
        }
    }

    fn at(&self, name: &str) {
        if self.panics.get() == Some(name) {
            panic!("the port {name} panics");
        }
    }
}

impl Owner for Panicky<'_> {
    type Config = Config;
    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        if events
            .iter()
            .any(|e| matches!(e.what, What::ItemLapsed { .. }))
        {
            self.at("log_lapsed");
        }
        self.inner.log(events)
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

impl Runs for Panicky<'_> {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.inner.start(spec)
    }
    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        self.at("answer_confirm");
        self.inner.answer_confirm(run, answers, record)
    }
    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        self.at("answer_handback");
        self.inner.answer_handback(run, result)
    }
    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        self.at("answer_bind");
        self.inner.answer_bind(run, result)
    }
    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        self.at("cancel");
        self.inner.cancel(run)
    }
    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.inner.kill(run)
    }
    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        self.inner.reap()
    }
}

impl Clock for Panicky<'_> {
    fn now(&self) -> Result<u64, PortError> {
        self.inner.now()
    }
    fn today(&self) -> Result<u32, PortError> {
        self.inner.today()
    }
    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.inner.day(at)
    }
}

impl Rng for Panicky<'_> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.inner.bytes16()
    }
}

fn held<P: Owner + Runs + Clock + Rng>(
    p: &P,
    inner: &Ports<Config>,
    l: &mut Ledger,
    t: &mut Table,
    armed: bool,
) -> Held {
    let mut held = issue(p, l, t, &LIMITS, &ask(PAGE)).expect("an item");
    if armed {
        on_screen(&mut held, NOW);
        inner.set_clock(NOW + LIMITS.arm_ms, TODAY);
        tick(p, l, t, &mut held, &LIMITS).expect("a tick");
        assert_eq!(held.confirmation().state(), &ConfirmState::Armed);
    } else {
        assert_eq!(held.confirmation().state(), &ConfirmState::Shown);
    }
    held
}

fn methods(p: &Ports<Config>, from: usize) -> Vec<&'static str> {
    p.tried.borrow()[from..]
        .iter()
        .map(|t| t.call.method())
        .collect()
}

fn lapsed_rows(p: &Ports<Config>) -> usize {
    p.events
        .borrow()
        .iter()
        .filter(|e| matches!(e.what, What::ItemLapsed { .. }))
        .count()
}

fn waiting_lapses(l: &Ledger) -> usize {
    l.waiting()
        .iter()
        .filter(|e| matches!(e.what, What::ItemLapsed { .. }))
        .count()
}

fn record_of(entry: &DecisionEntry, state: EffectState) -> EffectRecord {
    EffectRecord {
        run: run_id(1),
        agent: agent_id(1),
        effect: entry.effect.clone(),
        state,
        class: Class::Send,
        digest: entry.digest.clone(),
        approval: Some(entry.approval.clone()),
    }
}

#[test]
fn rd83_a_stopping_lapse_cancels_then_decides_then_sends_its_row() {
    for cause in STOPS {
        for armed in [false, true] {
            let case = format!("{cause} armed={armed}");
            let p = ports();
            let (mut l, mut t) = (ledger(), Table::new());
            let mut h = held(&p, &p, &mut l, &mut t, armed);
            let from = p.tried.borrow().len();
            lapse(&p, &mut l, &mut t, &mut h, cause, &LIMITS).unwrap();
            assert_eq!(
                methods(&p, from),
                ["cancel", "answer_confirm", "log"],
                "{case}"
            );
            assert_eq!(
                h.confirmation().state(),
                &ConfirmState::Lapsed(cause),
                "{case}"
            );
            assert!(t.is_stopped(&run_id(1)), "{case}");
            let decided = p.sent_decisions();
            let last = decided.last().expect("a decision");
            assert!(
                last.approvals()
                    .iter()
                    .all(|e| e.answer == halvo_coord::types::EffectAnswer::RejectOnce),
                "{case}"
            );
            assert_eq!(lapsed_rows(&p), 1, "{case}");
        }
    }
}

#[test]
fn rd83_the_guard_at_the_lapses_decision_refuses_the_records_of_an_earlier_allow() {
    for cause in STOPS {
        let inner = ports();
        let p = Panicky::new(&inner);
        let (mut l, mut t) = (ledger(), Table::new());
        let mut one = held(&p, &inner, &mut l, &mut t, true);
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
        let allowed: Vec<DecisionEntry> = inner.sent_decisions()[0].approvals().to_vec();
        let passes = |t: &Table| {
            allowed.iter().any(|e| {
                [EffectState::Approved, EffectState::Releasing]
                    .iter()
                    .any(|s| guard_step(t, &record_of(e, *s), Gives::Confirm).is_ok())
            })
        };
        assert!(passes(&t), "{cause}: the first card's rows count");
        let second = ask_of(1, 1, vec![asked(3, Class::Send, 3, true)]);
        let mut two = issue(&p, &mut l, &t, &LIMITS, &second).unwrap();
        p.panics.set(Some("answer_confirm"));
        let out = catch_unwind(AssertUnwindSafe(|| {
            lapse(&p, &mut l, &mut t, &mut two, cause, &LIMITS)
        }));
        p.panics.set(None);
        assert!(out.is_err(), "{cause}: the decision's call panicked");
        assert!(!passes(&t), "{cause}: approval for both records");
    }
}

#[test]
fn rd83_a_cancel_that_panics_leaves_the_stop_the_lapse_and_the_row_owed() {
    for cause in STOPS {
        let inner = ports();
        let p = Panicky::new(&inner);
        let (mut l, mut t) = (ledger(), Table::new());
        let mut h = held(&p, &inner, &mut l, &mut t, false);
        let from = inner.tried.borrow().len();
        p.panics.set(Some("cancel"));
        let out = catch_unwind(AssertUnwindSafe(|| {
            lapse(&p, &mut l, &mut t, &mut h, cause, &LIMITS)
        }));
        p.panics.set(None);
        assert!(out.is_err(), "{cause}");
        assert!(
            t.is_stopped(&run_id(1)),
            "{cause}: the table holds the stop"
        );
        assert_eq!(h.confirmation().state(), &ConfirmState::Lapsed(cause));
        assert_eq!(methods(&inner, from), Vec::<&str>::new(), "{cause}");
        assert_eq!(waiting_lapses(&l), 1, "{cause}: the row is owed");
        assert_eq!(lapsed_rows(&inner), 0, "{cause}");
        flush(&p, &mut l, &LIMITS).unwrap();
        assert_eq!(lapsed_rows(&inner), 1, "{cause}: sent by the next flush");
    }
}

#[test]
fn rd83_a_decision_that_panics_after_the_cancel_leaves_the_same() {
    for cause in STOPS {
        let inner = ports();
        let p = Panicky::new(&inner);
        let (mut l, mut t) = (ledger(), Table::new());
        let mut h = held(&p, &inner, &mut l, &mut t, true);
        let from = inner.tried.borrow().len();
        p.panics.set(Some("answer_confirm"));
        let out = catch_unwind(AssertUnwindSafe(|| {
            lapse(&p, &mut l, &mut t, &mut h, cause, &LIMITS)
        }));
        p.panics.set(None);
        assert!(out.is_err(), "{cause}");
        assert_eq!(methods(&inner, from), ["cancel"], "{cause}");
        assert!(t.is_stopped(&run_id(1)), "{cause}");
        assert_eq!(h.confirmation().state(), &ConfirmState::Lapsed(cause));
        assert_eq!(waiting_lapses(&l), 1, "{cause}: the row is owed");
        flush(&p, &mut l, &LIMITS).unwrap();
        assert_eq!(lapsed_rows(&inner), 1, "{cause}");
    }
}

#[test]
fn rd83_a_row_store_that_panics_leaves_the_cancel_sent() {
    for cause in STOPS {
        let inner = ports();
        let p = Panicky::new(&inner);
        let (mut l, mut t) = (ledger(), Table::new());
        let mut h = held(&p, &inner, &mut l, &mut t, false);
        let from = inner.tried.borrow().len();
        p.panics.set(Some("log_lapsed"));
        let out = catch_unwind(AssertUnwindSafe(|| {
            lapse(&p, &mut l, &mut t, &mut h, cause, &LIMITS)
        }));
        p.panics.set(None);
        assert!(out.is_err(), "{cause}");
        assert_eq!(
            methods(&inner, from),
            ["cancel", "answer_confirm"],
            "{cause}"
        );
        assert!(t.is_stopped(&run_id(1)), "{cause}");
    }
}

#[test]
fn rd83_a_cancel_that_fails_sends_no_decision_and_one_that_answers_ended_does() {
    let ended = PortError::Refused(Name::parse("ended").unwrap());
    for cause in STOPS {
        for (error, decides) in [
            (PortError::Busy, false),
            (PortError::NoService, false),
            (ended.clone(), true),
        ] {
            let case = format!("{cause} {error:?}");
            let p = ports();
            let (mut l, mut t) = (ledger(), Table::new());
            let mut h = held(&p, &p, &mut l, &mut t, true);
            let from = p.tried.borrow().len();
            p.refuse(Method::Cancel, error);
            lapse(&p, &mut l, &mut t, &mut h, cause, &LIMITS).unwrap();
            let want: &[&str] = if decides {
                &["cancel", "answer_confirm", "log"]
            } else {
                &["cancel", "log"]
            };
            assert_eq!(methods(&p, from), want, "{case}");
            assert!(t.is_stopped(&run_id(1)), "{case}");
            assert_eq!(lapsed_rows(&p), 1, "{case}");
        }
    }
}

#[test]
fn rd83_time_out_holds_the_stop_before_its_result_and_keeps_its_order() {
    let inner = ports();
    let p = Panicky::new(&inner);
    let (mut l, mut t) = (ledger(), Table::new());
    let open = raise_run_item(&p, &mut l, &LIMITS, &run_id(1), RunItemKind::Stuck).unwrap();
    p.panics.set(Some("answer_handback"));
    let out = catch_unwind(AssertUnwindSafe(|| {
        time_out(&p, &mut l, &mut t, &run_id(1), &open, &LIMITS)
    }));
    p.panics.set(None);
    assert!(out.is_err());
    assert!(t.is_stopped(&run_id(1)), "the table holds the stop first");
    let inner = ports();
    let (mut l, mut t) = (ledger(), Table::new());
    let open = raise_run_item(&inner, &mut l, &LIMITS, &run_id(1), RunItemKind::Stuck).unwrap();
    let from = inner.tried.borrow().len();
    time_out(&inner, &mut l, &mut t, &run_id(1), &open, &LIMITS).unwrap();
    assert_eq!(methods(&inner, from), ["answer_handback", "log", "cancel"]);
    assert!(matches!(
        inner.calls.borrow()[from].call,
        Call::AnswerHandback(_, HandbackResult::Timeout)
    ));
}

#[test]
fn rd83_lapse_bind_holds_the_stop_before_its_decline_and_keeps_its_order() {
    let bind = BindAsk {
        run: run_id(1),
        agent: agent_id(1),
        origin: Origin::parse("https://pay.site1.example:8443").unwrap(),
    };
    for cause in [OtherLapse::WaitLimit, OtherLapse::Cancelled] {
        let inner = ports();
        let p = Panicky::new(&inner);
        let (mut l, mut t) = (ledger(), Table::new());
        let mut held = raise_bind(&p, &mut l, &Table::new(), &LIMITS, &bind).unwrap();
        p.panics.set(Some("answer_bind"));
        let out = catch_unwind(AssertUnwindSafe(|| {
            lapse_bind(&p, &mut l, &mut t, &mut held, cause, &LIMITS)
        }));
        p.panics.set(None);
        assert!(out.is_err(), "{cause}");
        assert!(
            t.is_stopped(&run_id(1)),
            "{cause}: the table holds the stop first"
        );
        let inner = ports();
        let (mut l, mut t) = (ledger(), Table::new());
        let mut held = raise_bind(&inner, &mut l, &Table::new(), &LIMITS, &bind).unwrap();
        let from = inner.tried.borrow().len();
        lapse_bind(&inner, &mut l, &mut t, &mut held, cause, &LIMITS).unwrap();
        assert_eq!(
            methods(&inner, from),
            ["answer_bind", "log", "cancel"],
            "{cause}"
        );
        assert!(matches!(
            inner.calls.borrow()[from].call,
            Call::AnswerBind(_, BindResult::Decline)
        ));
    }
}

#[test]
fn rd83_a_lapse_that_stops_no_run_sends_no_cancel() {
    for cause in [
        LapseCause::NoRecord,
        LapseCause::Digest,
        LapseCause::RunEnded,
        LapseCause::AppEnded,
    ] {
        let p = ports();
        let (mut l, mut t) = (ledger(), Table::new());
        let mut h = held(&p, &p, &mut l, &mut t, true);
        let from = p.tried.borrow().len();
        lapse(&p, &mut l, &mut t, &mut h, cause, &LIMITS).unwrap();
        assert!(!methods(&p, from).contains(&"cancel"), "{cause}");
        assert!(!t.is_stopped(&run_id(1)), "{cause}");
        assert_eq!(
            h.confirmation().state(),
            &ConfirmState::Lapsed(cause),
            "{cause}"
        );
        let rows: Vec<Lapsed> = p
            .events
            .borrow()
            .iter()
            .filter_map(|e| match &e.what {
                What::ItemLapsed { lapsed, .. } => Some(*lapsed),
                _ => None,
            })
            .collect();
        assert_eq!(rows, [Lapsed::Confirm(cause)], "{cause}");
    }
}
