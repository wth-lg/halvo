//! The drive of the items (I5.10): it carries out through the ports what `confirm_step`,
//! `handback_result` and `setup_step` decided, records an answer before it releases it (I4.5),
//! keeps the guard's table in step with the items, and sends the setup's body until the owner
//! acknowledges it (I3.5).

use super::{compose_confirm, confirm_step, handback_result, setup_step};
use super::{ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation};
use super::{Setup, SetupBody, SetupInput, SetupMove, SetupState};
use crate::guard::Table;
use crate::ports::{Clock, Owner, PortError, Rng, Runs};
use crate::schedule::wants_active;
use crate::types::{
    hex, AgentId, AgentState, Answered, Body, Cause, CheckAnswer, Choice, ConfigView, ConfirmAsk,
    DoneAnswer, Event, Facts, Item, ItemId, LapseCause, Lapsed, Limits, Presenter, Raised, RunId,
    RunItemKind, What, WorkflowId,
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
    let events = ledger.number(whats, now, today);
    ledger.waiting.extend(events);
    flush(ports, ledger, limits)
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
/// issued and when its presenter first reported it on screen, in milliseconds of the clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    item: ItemId,
    ask: ConfirmAsk,
    confirmation: Confirmation,
    issued_at: u64,
    on_screen_at: Option<u64>,
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

    /// The item as the page gets it (I3.4), with its lines `confirm.changed` from `table`, the
    /// guard's table of this life (I4.3).
    pub fn page_item(&self, table: &Table) -> Result<Item, Cause> {
        compose_confirm(&self.item, &self.ask, &self.confirmation, table)
    }
}

/// Issues a confirmation for a confirm request that `raise_step` admitted (I4.2, I4.4): mints
/// the item id and stores `item_raised` with the run. A request of a run that the guard's table
/// holds as stopped is refused with `state`: nothing is stored and nothing sent (I4.2). A request whose item does not compose is
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
    if table.is_stopped(ask.run()) {
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
    held.page_item(table)?;
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

/// The person's choice on an item that a handback of `run` raised (I3.1): stores its
/// `item_answered` row, and only once the owner acknowledged it hands the run its result. A
/// choice that is not one of the kind's is `malformed`; with no acknowledgement the answer is
/// `no_service` and the run gets nothing; a run that ended (`ended`) takes nothing back.
pub fn answer_handback<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    item: &ItemId,
    run: &RunId,
    kind: RunItemKind,
    choice: Choice,
) -> Result<(), Cause> {
    let result = handback_result(kind, choice)?;
    let done = || DoneAnswer::parse(choice.name());
    let answer = match kind {
        RunItemKind::SignIn => Answered::SignIn(done()?),
        RunItemKind::LinkAccount => Answered::LinkAccount(done()?),
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

/// What a move of the setup flow did (I3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupDone {
    /// The item stays open and takes more answers.
    Open,
    /// The body was sent and the owner did not acknowledge it: it is owed and goes again at the
    /// next tick, and the item stays open and takes no answer.
    Owed,
    /// The owner acknowledged the body and the item closed; `active` says whether the rule of
    /// `active` of I5.5 (`wants_active`) gave `set_state` `active` for the agent, which was then
    /// sent once. The caller takes the item off the agent's facts.
    Closed {
        /// Whether `set_state` `active` was sent.
        active: bool,
    },
    /// The owner refused the body with `no_workflow`: the item ends with its workflow, nothing
    /// is stored and nothing more is sent (I5.7).
    Gone,
}

fn same_agent<C>(setup: &Setup, facts: &Facts<C>) -> Result<(), Cause> {
    if setup.agent() == &facts.agent && setup.workflow() == &facts.workflow.id {
        Ok(())
    } else {
        Err(Cause::State)
    }
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

/// Sends the owed body: at the acknowledgement the item closes and the rule of `active` runs
/// once; a refusal `no_workflow` ends the item; any other failure leaves the body owed.
fn send_owed<P: Owner, C: ConfigView + Clone>(
    ports: &P,
    setup: &mut Setup,
    facts: &Facts<C>,
) -> Result<SetupDone, Cause> {
    let SetupState::Owed(body) = setup.state().clone() else {
        return Ok(SetupDone::Open);
    };
    match ports.send(&body.body()) {
        Ok(_) => {
            *setup = setup_step(setup, SetupMove::Acknowledged)?;
            Ok(SetupDone::Closed {
                active: activate(ports, facts, &body),
            })
        }
        Err(e) if refused_with(&e, "no_workflow") => {
            *setup = setup_step(setup, SetupMove::Gone)?;
            Ok(SetupDone::Gone)
        }
        Err(_) => Ok(SetupDone::Owed),
    }
}

/// The person's answer to a setup item (I3.5), with the facts of its agent: `state` for the facts
/// of another agent, and the refusal of `setup_step`, `stale` for an item that closed, `state`
/// while its body is owed (ruling IG-10) and `malformed` for an answer that does not fit it; then
/// nothing is sent. When the answer gives the last parameter, the one body `set_bindings` goes
/// to the owner in this move; at its acknowledgement the item closes and the rule of `active` of
/// I5.5 runs once for the agent, and without one the body is owed.
pub fn answer_setup<P: Owner, C: ConfigView + Clone>(
    ports: &P,
    setup: &mut Setup,
    facts: &Facts<C>,
    answer: SetupInput<'_>,
) -> Result<SetupDone, Cause> {
    same_agent(setup, facts)?;
    *setup = setup_step(setup, SetupMove::Input(answer))?;
    send_owed(ports, setup, facts)
}

/// One tick for a setup item (I3.5, I5.9), which the coordinator's tick calls: sends its owed
/// body again, with what `answer_setup` does at the owner's answer; an open item sends nothing.
/// `state` for the facts of another agent, and `stale` for an item that closed or whose workflow
/// is gone.
pub fn tick_setup<P: Owner, C: ConfigView + Clone>(
    ports: &P,
    setup: &mut Setup,
    facts: &Facts<C>,
) -> Result<SetupDone, Cause> {
    same_agent(setup, facts)?;
    match setup.state() {
        SetupState::Closed | SetupState::Gone => Err(Cause::Stale),
        SetupState::Open | SetupState::Owed(_) => send_owed(ports, setup, facts),
    }
}
