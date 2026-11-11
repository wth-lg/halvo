//! The drive of the items (I5.10): it carries out through the ports what `confirm_step` and
//! `handback_result` decided, records an answer before it releases it (I4.5), and keeps the
//! guard's table in step with the items.

use super::{compose_confirm, confirm_step, handback_result};
use super::{ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation};
use crate::guard::Table;
use crate::ports::{Clock, Owner, PortError, Rng, Runs};
use crate::types::{
    hex, AgentId, Answered, Cause, CheckAnswer, Choice, ConfirmAsk, DoneAnswer, Event, Item,
    ItemId, LapseCause, Lapsed, Limits, Presenter, Raised, RunId, RunItemKind, What, WorkflowId,
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

    /// The item as the page gets it (I3.4).
    pub fn page_item(&self) -> Result<Item, Cause> {
        compose_confirm(&self.item, &self.ask, &self.confirmation)
    }
}

/// Issues a confirmation for a confirm request that `raise_step` admitted (I4.2, I4.4): mints
/// the item id and stores `item_raised` with the run. A request whose item does not compose is
/// `malformed`: no item, nothing stored and nothing sent. A request whose digests do not hold is
/// not shown: the port answers `reject` for every effect with a record that writes no row, and
/// `item_lapsed` with the cause `digest` is stored, with no `item_raised`.
pub fn issue<P: Owner + Runs + Clock + Rng>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    ask: &ConfirmAsk,
) -> Result<Held, Cause> {
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

/// The person's answer from `from` (I4.5): `confirm_step` refuses it with `stale`, `presenter`
/// or `early` and no port is called. Else the answers go to `Runs::answer_confirm`, whose call
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
    let answered = confirm_step(
        Some(&held.confirmation),
        ConfirmMove::Answer { answer, from },
    )?;
    held.confirmation = answered.clone();
    let (now, today) = match (ports.now(), ports.today()) {
        (Ok(now), Ok(today)) => (now, today),
        (Err(e), _) | (_, Err(e)) => return Err(cause_of(&e)),
    };
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
            *table = table.record(&item, &decision).release(&item);
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
                *table = table.record(&item, decision);
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
