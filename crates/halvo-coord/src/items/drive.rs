//! The drive of the items (I5.10): it carries out through the ports what `confirm_step`,
//! `handback_result` and `setup_step` decided, records an answer before it releases it (I4.5),
//! keeps the guard's table in step with the items, and sends the setup's body until the owner
//! acknowledges it (I3.5).

use super::{compose_bind, compose_confirm, confirm_step, handback_kind, handback_result};
use super::{setup_step, ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation};
use super::{Setup, SetupBody, SetupInput, SetupMove, SetupState};
use crate::guard::Table;
use crate::ports::{Clock, Owner, PortError, Rng, Runs};
use crate::schedule::wants_active;
use crate::types::{
    hex, AgentId, AgentState, Answered, BindAnswer, BindAsk, BindResult, Body, Cause, CheckAnswer,
    Choice, ConfigView, ConfirmAsk, DoneAnswer, Event, Facts, HandbackAsk, Item, ItemId,
    LapseCause, Lapsed, Limits, LinkAnswer, OtherLapse, OwnItem, OwnItemKind, Presenter, Raised,
    RunId, RunItem, RunItemKind, SetupAnswer, SignInAnswer, What, WorkflowId,
};
use std::num::NonZeroU32;

const HISTORY_PAGE: usize = 64;
const HISTORY_CALLS: usize = 64;

fn cause_of(error: &PortError) -> Cause {
    match error {
        PortError::Busy => Cause::Busy,
        PortError::NoService | PortError::Refused(_) => Cause::NoService,
    }
}

fn refused_with(error: &PortError, name: &str) -> bool {
    matches!(error, PortError::Refused(n) if n.as_str() == name)
}

/// One agent's ledger as the drive writes it (I7.1): its workflow and agent, the `seq` of the
/// last event that the owner acknowledged, and the events that wait for an acknowledgement,
/// which every later write sends first. The coordinator holds one per agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    workflow: WorkflowId,
    agent: AgentId,
    last: u32,
    waiting: Vec<Event>,
}

impl Ledger {
    /// The ledger of `agent` of `workflow`, whose last stored event has the `seq` `last`, 0 for
    /// none.
    pub fn new(workflow: WorkflowId, agent: AgentId, last: u32) -> Ledger {
        Ledger {
            workflow,
            agent,
            last,
            waiting: Vec::new(),
        }
    }

    /// The `seq` of the last acknowledged event.
    pub fn last(&self) -> u32 {
        self.last
    }

    /// The events that wait for an acknowledgement, in order.
    pub fn waiting(&self) -> &[Event] {
        &self.waiting
    }

    fn number(&self, whats: Vec<What>, now: u64, today: u32) -> Vec<Event> {
        let at = i64::try_from(now).map_or(i64::MAX, |n| n.saturating_mul(1_000_000));
        let first = self.last.saturating_add(self.waiting.len() as u32);
        (0..)
            .zip(whats)
            .map(|(i, what)| Event {
                workflow: self.workflow.clone(),
                agent: self.agent.clone(),
                seq: NonZeroU32::MIN.saturating_add(first.saturating_add(i)),
                day: today,
                at,
                what,
            })
            .collect()
    }

    fn renumber(&mut self) {
        let last = self.last;
        for (i, e) in (0u32..).zip(self.waiting.iter_mut()) {
            e.seq = NonZeroU32::MIN.saturating_add(last.saturating_add(i));
        }
    }
}

fn same_but_seq(a: &Event, b: &Event) -> bool {
    a.workflow == b.workflow
        && a.agent == b.agent
        && a.day == b.day
        && a.at == b.at
        && a.what == b.what
}

/// Reads the owner's events after the last acknowledged one, takes the waiting events that the
/// owner stored with no acknowledgement off the list, and numbers the rest after the owner's
/// last; gives up after a fixed count of calls.
fn resync<P: Owner>(ports: &P, ledger: &mut Ledger) -> Result<(), PortError> {
    let mut after = ledger.last;
    for _ in 0..HISTORY_CALLS {
        let page = ports.history(&ledger.agent, after)?;
        for e in &page {
            if ledger.waiting.first().is_some_and(|w| same_but_seq(w, e)) {
                ledger.waiting.remove(0);
            }
            after = after.max(e.seq.get());
        }
        if page.len() < HISTORY_PAGE {
            break;
        }
    }
    ledger.last = after;
    ledger.renumber();
    Ok(())
}

/// Sends the waiting events in order, at most `request_records` in one body, and takes each
/// acknowledged body off the list. A refusal `seq` reads the owner's events once and sends
/// again (I7.2); any other failure leaves the rest waiting.
pub fn flush<P: Owner>(ports: &P, ledger: &mut Ledger, limits: &Limits) -> Result<(), PortError> {
    let mut resynced = false;
    while !ledger.waiting.is_empty() {
        let n = ledger
            .waiting
            .len()
            .min(limits.request_records.max(1) as usize);
        match ports.log(&ledger.waiting[..n]) {
            Ok(()) => {
                ledger.last = ledger.waiting[n - 1].seq.get();
                ledger.waiting.drain(..n);
            }
            Err(e) if refused_with(&e, "seq") && !resynced => {
                resync(ports, ledger)?;
                resynced = true;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Stores the events after the waiting ones, at the clock's time; an event that gets no
/// acknowledgement waits and goes again with the next write or `flush`.
pub fn store<P: Owner + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    whats: Vec<What>,
) -> Result<(), PortError> {
    let (now, today) = (ports.now()?, ports.today()?);
    store_at(ports, ledger, limits, whats, (now, today))
}

/// Stores the events after the waiting ones at the time and on the day `at`, read before the
/// move that makes them, so that no failure of the clock after the move can lose them; an event
/// that gets no acknowledgement waits and goes again with the next write or `flush`.
fn store_at<P: Owner>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    whats: Vec<What>,
    at: (u64, u32),
) -> Result<(), PortError> {
    let events = ledger.number(whats, at.0, at.1);
    ledger.waiting.extend(events);
    flush(ports, ledger, limits)
}

/// The clock's time and day, or the cause of its failure.
fn clock_at<P: Clock>(ports: &P) -> Result<(u64, u32), Cause> {
    match (ports.now(), ports.today()) {
        (Ok(now), Ok(today)) => Ok((now, today)),
        (Err(e), _) | (_, Err(e)) => Err(cause_of(&e)),
    }
}

/// Stores the events in one body once the waiting ones are stored, and keeps none of them
/// waiting: rows that get no acknowledgement count as not stored (I4.5).
fn store_once<P: Owner>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    whats: Vec<What>,
    now: u64,
    today: u32,
) -> Result<(), PortError> {
    flush(ports, ledger, limits)?;
    let mut events = ledger.number(whats.clone(), now, today);
    let mut resynced = false;
    loop {
        match ports.log(&events) {
            Ok(()) => {
                ledger.last = events.last().map_or(ledger.last, |e| e.seq.get());
                return Ok(());
            }
            Err(e) if refused_with(&e, "seq") && !resynced => {
                resync(ports, ledger)?;
                events = ledger.number(whats.clone(), now, today);
                resynced = true;
            }
            Err(e) => return Err(e),
        }
    }
}

/// A confirmation that the coordinator holds: its item id, its request, its state, when it was
/// issued and when its presenter first reported it on screen, in milliseconds of the clock, and
/// the guard's table as it stood at its issue, which holds the rows of the earlier items of this
/// life and of no later one (I4.3, ruling IG-6d).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    item: ItemId,
    ask: ConfirmAsk,
    confirmation: Confirmation,
    issued_at: u64,
    on_screen_at: Option<u64>,
    earlier: Table,
}

impl Held {
    /// The item id.
    pub fn item(&self) -> &ItemId {
        &self.item
    }

    /// The confirm request.
    pub fn ask(&self) -> &ConfirmAsk {
        &self.ask
    }

    /// The item's state with its request's effects and presenter.
    pub fn confirmation(&self) -> &Confirmation {
        &self.confirmation
    }

    /// When the item was issued.
    pub fn issued_at(&self) -> u64 {
        self.issued_at
    }

    /// When its presenter first reported it on screen, or none.
    pub fn on_screen_at(&self) -> Option<u64> {
        self.on_screen_at
    }

    /// The item as the page gets it (I3.4), with its lines `confirm.changed` from the guard's
    /// table as it stood at the item's issue, so that only an earlier item gives the line, and
    /// neither the item's own release nor a later item's (I4.3, ruling IG-6d).
    pub fn page_item(&self) -> Result<Item, Cause> {
        compose_confirm(&self.item, &self.ask, &self.confirmation, &self.earlier)
    }
}

/// Issues a confirmation for a confirm request that `raise_confirm` admitted (I4.2, I4.4): mints
/// the item id and stores `item_raised` with the run. A request of a run that the guard's table
/// holds as stopped, and one that names an effect that has begun or whose newest released
/// decision is `reject_once` (`Table::asks_too_late`, ruling IG-32), are refused with `state`:
/// nothing is stored and nothing sent (I4.2). A request whose item does not compose is
/// `malformed`: no item, nothing stored and nothing sent. A request whose digests do not hold is
/// not shown: the port answers `reject` for every effect with a record that writes no row, and
/// `item_lapsed` with the cause `digest` is stored, with no `item_raised`.
pub fn issue<P: Owner + Runs + Clock + Rng>(
    ports: &P,
    ledger: &mut Ledger,
    table: &Table,
    limits: &Limits,
    ask: &ConfirmAsk,
) -> Result<Held, Cause> {
    let late = ask
        .effects()
        .iter()
        .any(|e| table.asks_too_late(ask.run(), &e.effect));
    if table.is_stopped(ask.run()) || late {
        return Err(Cause::State);
    }
    let confirmation = confirm_step(None, ConfirmMove::Issue(ask))?;
    let now = ports.now().map_err(|e| cause_of(&e))?;
    let item = ItemId::parse(&hex(ports.bytes16().map_err(|e| cause_of(&e))?))?;
    let held = Held {
        item: item.clone(),
        ask: ask.clone(),
        confirmation,
        issued_at: now,
        on_screen_at: None,
        earlier: table.clone(),
    };
    if *held.confirmation.state() == ConfirmState::Lapsed(LapseCause::Digest) {
        let answers = held.confirmation.answers(ConfirmAnswer::Reject);
        let _ = ports.answer_confirm(ask.run(), &answers, &mut |_| Ok(()));
        let lapsed = What::ItemLapsed {
            item,
            lapsed: Lapsed::Confirm(LapseCause::Digest),
        };
        let _ = store(ports, ledger, limits, vec![lapsed]);
        return Ok(held);
    }
    held.page_item()?;
    let raised = What::ItemRaised {
        item,
        raised: Raised::Confirm(ask.run().clone()),
    };
    let _ = store(ports, ledger, limits, vec![raised]);
    Ok(held)
}

/// Notes the presenter's first report that the item is on screen, at `now`.
pub fn on_screen(held: &mut Held, now: u64) {
    if held.on_screen_at.is_none() {
        held.on_screen_at = Some(now);
    }
}

/// The move that time makes on a held confirmation at `now`: a lapse with `wait_limit` once
/// `wait_limit_s` have passed since `issue` while it waits for the person, else `arm` once
/// `arm_ms` have passed since its first report on screen while it is `shown`; none otherwise.
pub fn due(held: &Held, now: u64, limits: &Limits) -> Option<ConfirmMove<'static>> {
    let state = held.confirmation.state();
    let limit = held
        .issued_at
        .saturating_add(limits.wait_limit_s.saturating_mul(1000));
    if state.waits_for_person() && now >= limit {
        return Some(ConfirmMove::Lapse(LapseCause::WaitLimit));
    }
    match (state, held.on_screen_at) {
        (ConfirmState::Shown, Some(at)) if now >= at.saturating_add(limits.arm_ms) => {
            Some(ConfirmMove::Arm)
        }
        _ => None,
    }
}

/// One tick for a held confirmation: sends the waiting events and makes the move that `due`
/// gives at the clock's time.
pub fn tick<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &mut Table,
    held: &mut Held,
    limits: &Limits,
) -> Result<(), Cause> {
    let _ = flush(ports, ledger, limits);
    let now = ports.now().map_err(|e| cause_of(&e))?;
    match due(held, now, limits) {
        Some(ConfirmMove::Lapse(cause)) => lapse(ports, ledger, table, held, cause, limits),
        Some(mv) => {
            held.confirmation = confirm_step(Some(&held.confirmation), mv)?;
            Ok(())
        }
        None => Ok(()),
    }
}

/// The person's answer from `from` (I4.5): an answer for a run that the guard's table holds as
/// stopped is refused with `state`, and `confirm_step` refuses it with `stale`, `presenter` or
/// `early`; then no port is called. Else the answers go to `Runs::answer_confirm`, whose call
/// back stores one `item_answered` row per entry of the port's decision and takes the move
/// `record` only when the owner acknowledged them. When the call returns success the item is
/// `released`, its rows count in the guard's table, and `item_released` is stored with the
/// entries of the decision. When the rows get no acknowledgement the item lapses with
/// `no_record`, `item_lapsed` is stored, and the answer is `no_service`: the port sent
/// `reject_once` under new ids. When the rows were acknowledged and the decision was not sent,
/// the item stays `recorded` and its rows count for no record.
pub fn answer<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &mut Table,
    held: &mut Held,
    answer: ConfirmAnswer,
    from: Presenter,
    limits: &Limits,
) -> Result<(), Cause> {
    if table.is_stopped(held.ask.run()) {
        return Err(Cause::State);
    }
    let answered = confirm_step(
        Some(&held.confirmation),
        ConfirmMove::Answer { answer, from },
    )?;
    let (now, today) = match (ports.now(), ports.today()) {
        (Ok(now), Ok(today)) => (now, today),
        (Err(e), _) | (_, Err(e)) => return Err(cause_of(&e)),
    };
    held.confirmation = answered.clone();
    let item = held.item.clone();
    let mut recorded: Option<Confirmation> = None;
    let sent = ports.answer_confirm(held.ask.run(), &answered.answers(answer), &mut |decision| {
        let next = confirm_step(Some(&answered), ConfirmMove::Record(decision))
            .map_err(|_| PortError::NoService)?;
        let rows = decision
            .approvals()
            .iter()
            .map(|entry| What::ItemAnswered {
                item: item.clone(),
                answer: Answered::Confirm(entry.clone()),
            })
            .collect();
        store_once(ports, ledger, limits, rows, now, today)?;
        recorded = Some(next);
        Ok(())
    });
    match (sent, recorded) {
        (Ok(()), Some(rec)) => {
            let ConfirmState::Recorded(decision) = rec.state().clone() else {
                return Err(Cause::State);
            };
            held.confirmation = confirm_step(Some(&rec), ConfirmMove::Release)?;
            *table = table
                .record(&item, held.ask.run(), &decision)
                .release(&item);
            let released = decision
                .approvals()
                .iter()
                .map(|entry| What::ItemReleased {
                    item: item.clone(),
                    entry: entry.clone(),
                })
                .collect();
            let _ = store(ports, ledger, limits, released);
            Ok(())
        }
        (Err(e), Some(rec)) => {
            if let ConfirmState::Recorded(decision) = rec.state() {
                *table = table.record(&item, held.ask.run(), decision);
            }
            held.confirmation = rec;
            Err(cause_of(&e))
        }
        (_, None) => {
            held.confirmation = confirm_step(
                Some(&held.confirmation),
                ConfirmMove::Lapse(LapseCause::NoRecord),
            )?;
            let lapsed = What::ItemLapsed {
                item,
                lapsed: Lapsed::Confirm(LapseCause::NoRecord),
            };
            let _ = store(ports, ledger, limits, vec![lapsed]);
            Err(Cause::NoService)
        }
    }
}

/// A lapse of a held confirmation with `cause` (I4.5): `confirm_step` refuses it with `state`.
/// For a lapse from `shown` or `armed` of a run that lives, every cause but `run_ended` and
/// `app_ended`, the port answers `reject` for every effect with a record that writes no row;
/// then `item_lapsed` is stored and the item's rows leave the guard's table; a lapse with
/// `wait_limit` or `cancelled` then stops the run in the guard's table and through
/// `Runs::cancel`.
pub fn lapse<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &mut Table,
    held: &mut Held,
    cause: LapseCause,
    limits: &Limits,
) -> Result<(), Cause> {
    let next = confirm_step(Some(&held.confirmation), ConfirmMove::Lapse(cause))?;
    let run = held.ask.run().clone();
    let lives = !matches!(cause, LapseCause::RunEnded | LapseCause::AppEnded);
    if lives && held.confirmation.state().waits_for_person() {
        let answers = held.confirmation.answers(ConfirmAnswer::Reject);
        let _ = ports.answer_confirm(&run, &answers, &mut |_| Ok(()));
    }
    held.confirmation = next;
    *table = table.lapse(&held.item);
    let lapsed = What::ItemLapsed {
        item: held.item.clone(),
        lapsed: Lapsed::Confirm(cause),
    };
    let _ = store(ports, ledger, limits, vec![lapsed]);
    if matches!(cause, LapseCause::WaitLimit | LapseCause::Cancelled) {
        *table = table.stop(&run);
        let _ = ports.cancel(&run);
    }
    Ok(())
}

/// The person's stop of the run of a held confirmation (I5.7, ruling RD-6): the lapse with
/// `cancelled` where the machine takes it; else, for an item that is `recorded`, the run is
/// stopped in the guard's table and `Runs::cancel` is sent, the fact that `Act::Stop` reads,
/// and the item stays to the end of its run.
pub fn stop_confirm<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &mut Table,
    held: &mut Held,
    limits: &Limits,
) -> Result<(), Cause> {
    if lapse(ports, ledger, table, held, LapseCause::Cancelled, limits).is_err() {
        let run = held.ask.run();
        if !table.is_stopped(run) {
            *table = table.stop(run);
            let _ = ports.cancel(run);
        }
    }
    Ok(())
}

/// The lapse of the open item of `run`, of a kind other than `confirm`, with `cause` (I7.1):
/// stores its `item_lapsed`, and for `wait_limit` and `cancelled`, which stop the run, stops the
/// run in the guard's table and sends `Runs::cancel` once, the fact that every kind of item sets
/// at its lapse and that `Act::Stop` reads (ruling RD-6); `malformed` for a confirmation, whose
/// lapse is `lapse`.
pub fn lapse_item<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &mut Table,
    run: &RunId,
    open: &RunItem,
    cause: OtherLapse,
    limits: &Limits,
) -> Result<(), Cause> {
    let lapsed = match open.kind {
        RunItemKind::SignIn => Lapsed::SignIn(cause),
        RunItemKind::LinkAccount => Lapsed::LinkAccount(cause),
        RunItemKind::Stuck => Lapsed::Stuck(cause),
        RunItemKind::Check => Lapsed::Check(cause),
        RunItemKind::Show => Lapsed::Show(cause),
        RunItemKind::Confirm => return Err(Cause::Malformed),
    };
    let row = What::ItemLapsed {
        item: open.item.clone(),
        lapsed,
    };
    let _ = store(ports, ledger, limits, vec![row]);
    if matches!(cause, OtherLapse::WaitLimit | OtherLapse::Cancelled) && !table.is_stopped(run) {
        *table = table.stop(run);
        let _ = ports.cancel(run);
    }
    Ok(())
}

/// The lapse of an open item that no run raised, `ready`, `check` or `setup`, with `cause`
/// (I7.1), as the app's quit lapses every open item with `app_ended` (I4.5): stores its
/// `item_lapsed`, which waits in the ledger when the owner does not acknowledge it.
pub fn lapse_own<P: Owner + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    open: &OwnItem,
    cause: OtherLapse,
) {
    let lapsed = match open.kind {
        OwnItemKind::Ready => Lapsed::Ready(cause),
        OwnItemKind::Check => Lapsed::Check(cause),
        OwnItemKind::Setup => Lapsed::Setup(cause),
    };
    let row = What::ItemLapsed {
        item: open.item.clone(),
        lapsed,
    };
    let _ = store(ports, ledger, limits, vec![row]);
}

/// Raises the item of a request of `run` that `raise_step` gave as `kind`, a kind other than
/// `confirm`, whose item `issue` raises (I3.1): mints its id and stores `item_raised` with the
/// run, which waits in the ledger when the owner does not acknowledge it; the caller moves the
/// run to `waiting` with the item.
pub fn raise_run_item<P: Owner + Clock + Rng>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    run: &RunId,
    kind: RunItemKind,
) -> Result<RunItem, Cause> {
    let raised = match kind {
        RunItemKind::SignIn => Raised::SignIn(run.clone()),
        RunItemKind::LinkAccount => Raised::LinkAccount(run.clone()),
        RunItemKind::Stuck => Raised::Stuck(run.clone()),
        RunItemKind::Check => Raised::Check(Some(run.clone())),
        RunItemKind::Show => Raised::Show(run.clone()),
        RunItemKind::Confirm => return Err(Cause::Malformed),
    };
    let item = ItemId::parse(&hex(ports.bytes16().map_err(|e| cause_of(&e))?))?;
    let row = What::ItemRaised {
        item: item.clone(),
        raised,
    };
    let _ = store(ports, ledger, limits, vec![row]);
    Ok(RunItem { item, kind })
}

/// Answers a handback of `run` that `raise_step` answered from the record of an answered check
/// (I5.4): sends the result and stores nothing; a run that ended (`ended`) takes nothing.
pub fn answer_from_check<P: Runs>(
    ports: &P,
    run: &RunId,
    result: crate::types::HandbackResult,
) -> Result<(), Cause> {
    match ports.answer_handback(run, result) {
        Ok(()) => Ok(()),
        Err(e) if refused_with(&e, "ended") => Ok(()),
        Err(e) => Err(cause_of(&e)),
    }
}

/// The person's choice on the item that the handback request `ask` raised (I3.1): stores its
/// `item_answered` row, which for a `link_account` item holds the request's whole origin (I7.1,
/// ruling IG-14), and only once the owner acknowledged it hands the run its result. A choice that
/// is not one of the kind's, and a `link_account` request with no origin, are `malformed`; with
/// no acknowledgement the answer is `no_service` and the run gets nothing; a run that ended
/// (`ended`) takes nothing back.
pub fn answer_handback<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    item: &ItemId,
    ask: &HandbackAsk,
    choice: Choice,
) -> Result<(), Cause> {
    let run = &ask.run;
    let kind = handback_kind(ask.reason);
    let result = handback_result(kind, choice)?;
    let done = || DoneAnswer::parse(choice.name());
    let answer = match kind {
        RunItemKind::SignIn => Answered::SignIn(SignInAnswer::First(done()?)),
        RunItemKind::LinkAccount => Answered::LinkAccount(LinkAnswer {
            answer: done()?,
            origin: ask.origin.clone().ok_or(Cause::Malformed)?,
        }),
        RunItemKind::Stuck => Answered::Stuck(done()?),
        RunItemKind::Show => Answered::Show(done()?),
        RunItemKind::Check => Answered::Check(CheckAnswer::parse(choice.name())?),
        RunItemKind::Confirm => return Err(Cause::Malformed),
    };
    let (now, today) = match (ports.now(), ports.today()) {
        (Ok(now), Ok(today)) => (now, today),
        (Err(e), _) | (_, Err(e)) => return Err(cause_of(&e)),
    };
    let row = What::ItemAnswered {
        item: item.clone(),
        answer,
    };
    store_once(ports, ledger, limits, vec![row], now, today).map_err(|e| cause_of(&e))?;
    match ports.answer_handback(run, result) {
        Ok(()) => Ok(()),
        Err(e) if refused_with(&e, "ended") => Ok(()),
        Err(e) => Err(cause_of(&e)),
    }
}

/// The state of a held second step of a `sign_in` item: open for the person's answer; its
/// `item_answered` row stored with this answer, which the run has not taken yet; or closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindState {
    /// Waits for the person.
    Open,
    /// The row of this answer is stored and `Runs::answer_bind` has not taken it.
    Recorded(BindResult),
    /// Answered or lapsed.
    Closed,
}

/// The second step of a `sign_in` item that the coordinator holds (I3.1): its item id, its
/// request, when it was raised, in milliseconds of the clock, and its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldBind {
    item: ItemId,
    ask: BindAsk,
    raised_at: u64,
    state: BindState,
}

impl HeldBind {
    /// The item id.
    pub fn item(&self) -> &ItemId {
        &self.item
    }

    /// The request.
    pub fn ask(&self) -> &BindAsk {
        &self.ask
    }

    /// When the item was raised.
    pub fn raised_at(&self) -> u64 {
        self.raised_at
    }

    /// The state.
    pub fn state(&self) -> &BindState {
        &self.state
    }

    /// The item as the page gets it (I3.4), for the agent of the name `name` (I2.3).
    pub fn page_item(&self, name: &str) -> Result<Item, Cause> {
        compose_bind(&self.item, &self.ask.agent, name, &self.ask.origin)
    }
}

/// Raises the second step of a `sign_in` item for a request `Bind` that `raise_step` admitted
/// (I3.1): mints the item id and stores `item_raised` with the kind `sign_in` and the run. A
/// request of a run that the guard's table holds as stopped is refused with `state`: nothing is
/// stored and nothing sent.
pub fn raise_bind<P: Owner + Clock + Rng>(
    ports: &P,
    ledger: &mut Ledger,
    table: &Table,
    limits: &Limits,
    ask: &BindAsk,
) -> Result<HeldBind, Cause> {
    if table.is_stopped(&ask.run) {
        return Err(Cause::State);
    }
    let now = ports.now().map_err(|e| cause_of(&e))?;
    let item = ItemId::parse(&hex(ports.bytes16().map_err(|e| cause_of(&e))?))?;
    let raised = What::ItemRaised {
        item: item.clone(),
        raised: Raised::SignIn(ask.run.clone()),
    };
    let _ = store(ports, ledger, limits, vec![raised]);
    Ok(HeldBind {
        item,
        ask: ask.clone(),
        raised_at: now,
        state: BindState::Open,
    })
}

/// Whether an open second step has waited `wait_limit_s` since it was raised at `now`, so that
/// it lapses with `wait_limit` (I3.1, ruling IG-18).
pub fn bind_due(held: &HeldBind, now: u64, limits: &Limits) -> bool {
    let limit = held
        .raised_at
        .saturating_add(limits.wait_limit_s.saturating_mul(1000));
    held.state == BindState::Open && now >= limit
}

/// The person's choice on a second step (I3.1): `stale` for a closed step, `malformed` for a
/// choice other than `decline` and `allow`, and `state` for a run that the guard's table holds as
/// stopped, after which no port is called. An open step stores its `item_answered` row, which
/// holds the answer and the whole origin (I7.1, ruling IG-14), and only once the owner
/// acknowledged it calls `Runs::answer_bind` with the row's answer; with no acknowledgement the
/// answer is `no_service`, nothing is stored and the run gets nothing. A step whose row is stored
/// and whose answer the run did not take sends the row's answer again, stores no second row, and
/// refuses the other choice with `state`. An `answer_bind` refused with `ended` is dropped and
/// the step closes (ruling IG-18).
pub fn answer_bind<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &Table,
    limits: &Limits,
    held: &mut HeldBind,
    choice: Choice,
) -> Result<(), Cause> {
    if held.state == BindState::Closed {
        return Err(Cause::Stale);
    }
    let result = BindResult::parse(choice.name())?;
    if table.is_stopped(&held.ask.run) {
        return Err(Cause::State);
    }
    match held.state {
        BindState::Recorded(stored) if stored != result => return Err(Cause::State),
        BindState::Recorded(_) | BindState::Closed => {}
        BindState::Open => {
            let (now, today) = match (ports.now(), ports.today()) {
                (Ok(now), Ok(today)) => (now, today),
                (Err(e), _) | (_, Err(e)) => return Err(cause_of(&e)),
            };
            let row = What::ItemAnswered {
                item: held.item.clone(),
                answer: Answered::SignIn(SignInAnswer::Second(BindAnswer {
                    answer: result,
                    origin: held.ask.origin.clone(),
                })),
            };
            store_once(ports, ledger, limits, vec![row], now, today).map_err(|e| cause_of(&e))?;
            held.state = BindState::Recorded(result);
        }
    }
    match ports.answer_bind(&held.ask.run, result) {
        Ok(()) => {
            held.state = BindState::Closed;
            Ok(())
        }
        Err(e) if refused_with(&e, "ended") => {
            held.state = BindState::Closed;
            Ok(())
        }
        Err(e) => Err(cause_of(&e)),
    }
}

/// A lapse of a second step with `cause` (I3.1, ruling IG-18), as a confirmation lapses: `stale`
/// for a closed step. For `wait_limit`, a lapse of a run that lives, of an open step, the waiting
/// call is answered `decline` through `Runs::answer_bind`, whose refusal is dropped; then
/// `item_lapsed` is stored; then the run is stopped in the guard's table and through
/// `Runs::cancel`; a step whose row is stored is refused `wait_limit` with `state`, because the
/// person answered it. For `cancelled`, the person's stop of the run (ruling RD-6), an open step
/// lapses as for `wait_limit`, and a step whose row is stored keeps its row and stays to the end of
/// its run, which is stopped in the guard's table and through `Runs::cancel`, the fact that
/// `Act::Stop` reads. For `run_ended` and `app_ended`, of an open step or of one whose row is
/// stored and whose answer the run did not take, only `item_lapsed` is stored, and the step
/// closes.
pub fn lapse_bind<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &mut Table,
    held: &mut HeldBind,
    cause: OtherLapse,
    limits: &Limits,
) -> Result<(), Cause> {
    let run = held.ask.run.clone();
    let lives = match cause {
        OtherLapse::WaitLimit | OtherLapse::Cancelled => true,
        OtherLapse::RunEnded | OtherLapse::AppEnded => false,
    };
    match held.state {
        BindState::Closed => return Err(Cause::Stale),
        BindState::Recorded(_) if cause == OtherLapse::Cancelled => {
            if !table.is_stopped(&run) {
                *table = table.stop(&run);
                let _ = ports.cancel(&run);
            }
            return Ok(());
        }
        BindState::Recorded(_) if lives => return Err(Cause::State),
        BindState::Recorded(_) | BindState::Open => {}
    }
    if lives {
        let _ = ports.answer_bind(&run, BindResult::Decline);
    }
    held.state = BindState::Closed;
    let lapsed = What::ItemLapsed {
        item: held.item.clone(),
        lapsed: Lapsed::SignIn(cause),
    };
    let _ = store(ports, ledger, limits, vec![lapsed]);
    if lives {
        *table = table.stop(&run);
        let _ = ports.cancel(&run);
    }
    Ok(())
}

/// What a move of the setup flow did (I3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupDone {
    /// The item stays open and takes more answers.
    Open,
    /// The body was sent and the owner did not acknowledge it: it is owed and goes again at the
    /// next tick, and the item stays open and takes no answer.
    Owed,
    /// The owner acknowledged the body and the item closed with its `item_answered` row of the
    /// choice `choose`; `active` says whether the rule of `active` of I5.5 (`wants_active`) gave
    /// `set_state` `active` for the agent, which was then sent once. The caller takes the item
    /// off the agent's facts.
    Closed {
        /// Whether `set_state` `active` was sent.
        active: bool,
    },
    /// The owner refused the body by a name after which another value can pass: nothing more is
    /// sent until the person answers again, and the same item asks every parameter again with
    /// its answers cleared and the line `item.setup.not_taken`; nothing is stored (I3.5, ruling
    /// IG-34).
    NotTaken,
    /// The owner refused the body with `no_workflow`: the item ends with its workflow, nothing
    /// is stored and nothing more is sent (I5.7).
    Gone,
    /// The owner refused the body with `binding`: the agent is not the workflow's, the item
    /// ends, nothing is stored and nothing more is sent, and the caller treats the agent as one
    /// that the owner's list no longer holds (I3.5, ruling IG-36).
    AgentGone,
}

/// What the owner's refusal of a setup's body decides, in the three groups of names of I3.5
/// (rulings IG-28, IG-34, IG-36) and with `no_workflow` (I5.7): still owed after a failure with
/// no name, `busy`, `store_unavailable`, `internal` and `state`; gone after `no_workflow` and
/// `binding`; asked again after `config`, `malformed`, `too_large`, `length` and every other
/// name, so that the item always takes the person's answers again.
fn refused_body(error: &PortError) -> SetupDone {
    let PortError::Refused(name) = error else {
        return SetupDone::Owed;
    };
    match name.as_str() {
        "busy" | "store_unavailable" | "internal" | "state" => SetupDone::Owed,
        "no_workflow" => SetupDone::Gone,
        "binding" => SetupDone::AgentGone,
        _ => SetupDone::NotTaken,
    }
}

fn same_agent<C>(setup: &Setup, facts: &Facts<C>, ledger: &Ledger) -> Result<(), Cause> {
    let ours = |workflow: &WorkflowId, agent: &AgentId| {
        setup.agent() == agent && setup.workflow() == workflow
    };
    if ours(&facts.workflow.id, &facts.agent) && ours(&ledger.workflow, &ledger.agent) {
        Ok(())
    } else {
        Err(Cause::State)
    }
}

/// Stores the one `item_answered` row of a setup item, which closes at the owner's
/// acknowledgement with the choice `choose` and holds no value (I7.1, rulings IG-30, IG-35,
/// RD-23 (b)), at the time `at` that the clock gave before the move; a row that gets no
/// acknowledgement waits in the ledger and goes with the next write.
fn closed_row<P: Owner>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    setup: &Setup,
    at: (u64, u32),
) {
    let row = What::ItemAnswered {
        item: setup.item().clone(),
        answer: Answered::Setup(SetupAnswer::Choose),
    };
    let _ = store_at(ports, ledger, limits, vec![row], at);
}

/// The rule of `active` of I5.5, once, for the agent's facts with the slots of the acknowledged
/// body among its newest bindings: sends `set_state` `active` where `wants_active` gives it and
/// drops the answer, which the tick's own rule sends again until the owner's list shows the
/// agent `active` (ruling IG-6a).
fn activate<P: Owner, C: ConfigView + Clone>(
    ports: &P,
    facts: &Facts<C>,
    body: &SetupBody,
) -> bool {
    let mut after = facts.clone();
    for slot in body.slots() {
        if !after.slots.contains(&slot.name) {
            after.slots.push(slot.name.clone());
        }
    }
    let wants = wants_active(&after);
    if wants {
        let _ = ports.send(&Body::SetState {
            workflow: facts.workflow.id.clone(),
            agent: facts.agent.clone(),
            state: AgentState::Active,
        });
    }
    wants
}

/// Sends the owed body: at the acknowledgement the item closes, its row is stored and the rule
/// of `active` runs once; a refusal decides by `refused_body`.
fn send_owed<P: Owner, C: ConfigView + Clone>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    setup: &mut Setup,
    facts: &Facts<C>,
    at: (u64, u32),
) -> Result<SetupDone, Cause> {
    let SetupState::Owed(body) = setup.state().clone() else {
        return Ok(SetupDone::Open);
    };
    match ports.send(&body.body()) {
        Ok(_) => {
            *setup = setup_step(setup, SetupMove::Acknowledged)?;
            closed_row(ports, ledger, limits, setup, at);
            Ok(SetupDone::Closed {
                active: activate(ports, facts, &body),
            })
        }
        Err(e) => {
            let done = refused_body(&e);
            match done {
                SetupDone::Gone => *setup = setup_step(setup, SetupMove::Gone)?,
                SetupDone::AgentGone => *setup = setup_step(setup, SetupMove::Binding)?,
                SetupDone::NotTaken => *setup = setup_step(setup, SetupMove::NotTaken)?,
                SetupDone::Open | SetupDone::Owed | SetupDone::Closed { .. } => {}
            }
            Ok(done)
        }
    }
}

/// The person's answer to a setup item (I3.5), with the facts and the ledger of its agent:
/// `state` for the facts or the ledger of another agent, and the refusal of `setup_step`, `stale`
/// for an item that closed, `state` while its body is owed (ruling IG-10) and `malformed` for an
/// answer that does not fit it; then nothing is sent and nothing stored. `none` keeps the item
/// open with its answers and stores and sends nothing (I3.5, ruling RD-23 (b)). When the answer
/// gives the last parameter, the one body
/// `set_bindings` goes to the owner in this move; at its acknowledgement the item closes, its row
/// is stored and the rule of `active` of I5.5 runs once for the agent, and a refusal decides as
/// `refused_body` gives.
pub fn answer_setup<P: Owner + Clock, C: ConfigView + Clone>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    setup: &mut Setup,
    facts: &Facts<C>,
    answer: SetupInput<'_>,
) -> Result<SetupDone, Cause> {
    same_agent(setup, facts, ledger)?;
    let next = setup_step(setup, SetupMove::Input(answer))?;
    let at = match next.state() {
        SetupState::Owed(_) => clock_at(ports)?,
        SetupState::Open | SetupState::Closed | SetupState::Gone | SetupState::AgentGone => {
            *setup = next;
            return Ok(SetupDone::Open);
        }
    };
    *setup = next;
    send_owed(ports, ledger, limits, setup, facts, at)
}

/// One tick for a setup item (I3.5, I5.9), which the coordinator's tick calls: sends its owed
/// body again, with what `answer_setup` does at the owner's answer; an open item sends nothing.
/// `state` for the facts or the ledger of another agent, and `stale` for an item that closed or
/// is gone.
pub fn tick_setup<P: Owner + Clock, C: ConfigView + Clone>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    setup: &mut Setup,
    facts: &Facts<C>,
) -> Result<SetupDone, Cause> {
    same_agent(setup, facts, ledger)?;
    match setup.state() {
        SetupState::Closed | SetupState::Gone | SetupState::AgentGone => Err(Cause::Stale),
        SetupState::Open => Ok(SetupDone::Open),
        SetupState::Owed(_) => {
            let at = clock_at(ports)?;
            send_owed(ports, ledger, limits, setup, facts, at)
        }
    }
}
