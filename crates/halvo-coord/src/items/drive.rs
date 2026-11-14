//! The drive of the items (I5.10): it carries out through the ports what `confirm_step`,
//! `handback_result` and `setup_step` decided, records an answer before it releases it (I4.5),
//! keeps the guard's table in step with the items, and sends the setup's body until the owner
//! acknowledges it (I3.5).

use super::{choices, compose_bind, compose_confirm, confirm_step, handback_kind, handback_result};
use super::{setup_step, ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation};
use super::{Setup, SetupBody, SetupInput, SetupMove, SetupState};
use crate::guard::Table;
use crate::ports::{Clock, Owner, PortError, Rng, Runs, Stamp};
use crate::schedule::wants_active;
use crate::types::{
    hex, AgentId, AgentState, Answered, BindAnswer, BindAsk, BindResult, Body, Cause, CheckAnswer,
    Choice, Cleared, ConfigView, ConfirmAsk, DecisionEntry, DoneAnswer, Event, Facts, HandbackAsk,
    HandbackResult, Item, ItemId, ItemKind, LapseCause, Lapsed, Limits, LinkAnswer, OtherLapse,
    OwnItem, OwnItemKind, Presenter, Raised, RunId, RunItem, RunItemKind, SetupAnswer,
    SignInAnswer, What, WorkflowId,
};
use serde::Serialize;
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
    closing: Vec<Event>,
    unsure: Vec<ItemId>,
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
            closing: Vec::new(),
            unsure: Vec::new(),
        }
    }

    /// Takes the owner's events after the last acknowledged one that a read outside this ledger
    /// brought, the coordinator's read right after a call that answered no acknowledgement, as
    /// the read after a refusal `seq` takes them (ruling RD-40).
    pub fn take(&mut self, events: &[Event]) {
        let read: Vec<Event> = events
            .iter()
            .filter(|e| e.seq.get() > self.last)
            .cloned()
            .collect();
        if !read.is_empty() {
            self.absorb(read);
        }
    }

    /// The newest row by `seq` of the owner's events, as this ledger stored them or its reads
    /// gave them, that closes `item`: an answer whose acknowledgement was lost, or a lapse (ruling
    /// RD-40), or the decline that a `link_account` item stored after its `done` (ruling RD-46
    /// (4) B4). One decision for both orders in which the coordinator learns of such a row.
    fn closing(&self, item: &ItemId) -> Option<&Event> {
        self.closing
            .iter()
            .filter(|e| closed_item(&e.what) == Some(item) && closes(std::slice::from_ref(e), item))
            .max_by_key(|e| e.seq)
    }

    /// Keeps the rows that close an item among `events`, which the owner acknowledged.
    fn kept(&mut self, events: &[Event]) {
        for e in events {
            if closed_item(&e.what).is_some_and(|item| closes(std::slice::from_ref(e), item)) {
                self.closing.push(e.clone());
            }
        }
    }

    /// The events of a read: the waiting events that the owner stored with no acknowledgement
    /// leave the list, every waiting row that would close an item that the owner's events close
    /// leaves it, the rows that close an item are kept for `closing` and end the doubt of a press
    /// of that item whose row got no acknowledgement, and the rest is numbered after the owner's
    /// last.
    fn absorb(&mut self, read: Vec<Event>) {
        for e in &read {
            if let Some(k) = self.waiting.iter().position(|w| same_but_seq(w, e)) {
                self.waiting.remove(k);
            }
            self.last = self.last.max(e.seq.get());
        }
        for e in read {
            if let Some(item) = closed_item(&e.what).filter(|i| closes(std::slice::from_ref(&e), i))
            {
                self.unsure.retain(|i| i != item);
                self.closing.push(e);
            }
        }
        let closing = std::mem::take(&mut self.closing);
        self.waiting
            .retain(|w| closed_item(&w.what).is_none_or(|item| !closes(&closing, item)));
        self.closing = closing;
        self.renumber();
    }

    /// The `seq` of the last acknowledged event.
    pub fn last(&self) -> u32 {
        self.last
    }

    /// The events that wait for an acknowledgement, in order.
    pub fn waiting(&self) -> &[Event] {
        &self.waiting
    }

    fn number(&self, whats: Vec<What>, stamp: Stamp) -> Vec<Event> {
        let first = self.last.saturating_add(self.waiting.len() as u32);
        (0..)
            .zip(whats)
            .map(|(i, what)| Event {
                workflow: self.workflow.clone(),
                agent: self.agent.clone(),
                seq: NonZeroU32::MIN.saturating_add(first.saturating_add(i)),
                day: stamp.day(),
                at: stamp.at(),
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

/// Reads the owner's events after the last acknowledged one and takes them as `Ledger::absorb`
/// does: the waiting events that the owner stored with no acknowledgement leave the list, and so
/// does every waiting row that would close an item that those events close, whatever row closed
/// it (I7.2, rulings RD-34 (b) (1), RD-40); the rest is numbered after the owner's last. Gives up
/// after a fixed count of calls.
fn resync<P: Owner>(ports: &P, ledger: &mut Ledger) -> Result<(), PortError> {
    let mut after = ledger.last;
    let mut read = Vec::new();
    for _ in 0..HISTORY_CALLS {
        let page = ports.history(&ledger.agent, after)?;
        let full = page.len() >= HISTORY_PAGE;
        for e in page {
            after = after.max(e.seq.get());
            read.push(e);
        }
        if !full {
            break;
        }
    }
    ledger.absorb(read);
    Ok(())
}

/// The item that a row closes: an `item_answered` or an `item_lapsed`.
fn closed_item(what: &What) -> Option<&ItemId> {
    match what {
        What::ItemAnswered { item, .. } | What::ItemLapsed { item, .. } => Some(item),
        _ => None,
    }
}

/// Whether the rows close the item: its `item_released` or its `item_lapsed`, or its
/// `item_answered`, but for a confirmation, whose rows without their acknowledgement count as
/// not stored (I4.5, ruling RD-34 (b) (3)), and for the second step of a sign-in, whose answer
/// row that its run did not take is followed by the row of the person's next press or by its
/// lapse (ruling RD-46 (2) b, d, e). The kind is the row's own, since the read need not hold the
/// item's `item_raised`.
fn closes(rows: &[Event], item: &ItemId) -> bool {
    rows.iter().any(|e| match &e.what {
        What::ItemReleased { item: i, .. } | What::ItemLapsed { item: i, .. } => i == item,
        What::ItemAnswered { item: i, answer } => {
            i == item
                && !matches!(
                    answer,
                    Answered::Confirm(_) | Answered::SignIn(SignInAnswer::Second(_))
                )
        }
        _ => false,
    })
}

/// The compact text of a `coord_log` body of `events` for `workflow` and `agent` (I7.2): `op`
/// first, then `workflow`, `agent` and `records`, each record with the eighteen keys of I7.1 in
/// their order and null for a key that its event does not set, as halvo-schema's
/// `CoordLog::to_json` writes it.
pub fn log_text(workflow: &WorkflowId, agent: &AgentId, events: &[Event]) -> String {
    let body = LogBody {
        op: "coord_log",
        workflow: workflow.as_str(),
        agent: agent.as_str(),
        records: events.iter().map(Record::of).collect(),
    };
    serde_json::to_string(&body).unwrap_or_default()
}

/// The bytes of `log_text` for the workflow and the agent of the first of `events`, which a body
/// holds with every record (I7.2); 0 for none.
pub fn log_bytes(events: &[Event]) -> usize {
    events
        .first()
        .map_or(0, |e| log_text(&e.workflow, &e.agent, events).len())
}

/// A `coord_log` body, its keys in the order of I7.2.
#[derive(Serialize)]
struct LogBody<'a> {
    op: &'static str,
    workflow: &'a str,
    agent: &'a str,
    records: Vec<Record<'a>>,
}

/// A record of I7.1, its eighteen keys in their order, each set by the event or null.
#[derive(Serialize, Default)]
struct Record<'a> {
    v: u8,
    workflow: &'a str,
    agent: &'a str,
    seq: u32,
    day: u32,
    at: i64,
    event: &'static str,
    run: Option<&'a str>,
    slot: Option<u32>,
    item: Option<&'a str>,
    kind: Option<&'static str>,
    effect: Option<&'a str>,
    approval: Option<&'a str>,
    digest: Option<&'a str>,
    answer: Option<&'static str>,
    cause: Option<&'static str>,
    trigger: Option<&'static str>,
    origin: Option<&'a str>,
}

impl<'a> Record<'a> {
    fn of(e: &'a Event) -> Record<'a> {
        let base = Record {
            v: 1,
            workflow: e.workflow.as_str(),
            agent: e.agent.as_str(),
            seq: e.seq.get(),
            day: e.day,
            at: e.at,
            ..Record::default()
        };
        match &e.what {
            What::RunStarted { run, trigger, slot } => Record {
                event: "run_started",
                run: Some(run.as_str()),
                slot: *slot,
                trigger: Some(trigger.name()),
                ..base
            },
            What::RunEnded { run, cause } => Record {
                event: "run_ended",
                run: Some(run.as_str()),
                cause: Some(cause.name()),
                ..base
            },
            What::ItemRaised { item, raised } => Record {
                event: "item_raised",
                item: Some(item.as_str()),
                kind: Some(raised.kind().name()),
                run: raised.run().map(RunId::as_str),
                ..base
            },
            What::ItemAnswered {
                item,
                answer: Answered::Confirm(entry),
            } => Record {
                event: "item_answered",
                ..Record::entry(item, entry, base)
            },
            What::ItemAnswered { item, answer } => Record {
                event: "item_answered",
                item: Some(item.as_str()),
                kind: Some(answer.kind().name()),
                answer: answer.choice().map(Choice::name),
                origin: match answer {
                    Answered::SignIn(SignInAnswer::Second(b)) => Some(b.origin.as_str()),
                    Answered::LinkAccount(l) => Some(l.origin.as_str()),
                    _ => None,
                },
                ..base
            },
            What::ItemLapsed { item, lapsed } => Record {
                event: "item_lapsed",
                item: Some(item.as_str()),
                kind: Some(lapsed.kind().name()),
                cause: Some(lapsed.cause().name()),
                ..base
            },
            What::ItemReleased { item, entry } => Record {
                event: "item_released",
                ..Record::entry(item, entry, base)
            },
            What::HoldSet { cause } => Record {
                event: "hold_set",
                cause: Some(cause.name()),
                ..base
            },
            What::HoldCleared { cause } => {
                let (run, answer, cause) = match cause {
                    Cleared::Person => (None, None, "person"),
                    Cleared::Failures => (None, None, "failures"),
                    Cleared::Check { run, answer } => {
                        (Some(run.as_str()), Some(answer.name()), "check")
                    }
                };
                Record {
                    event: "hold_cleared",
                    run,
                    answer,
                    cause: Some(cause),
                    ..base
                }
            }
            What::SlotSkipped { slot } => Record {
                event: "slot_skipped",
                slot: *slot,
                ..base
            },
        }
    }

    fn entry(item: &'a ItemId, entry: &'a DecisionEntry, base: Record<'a>) -> Record<'a> {
        Record {
            item: Some(item.as_str()),
            kind: Some(ItemKind::Confirm.name()),
            effect: Some(entry.effect.as_str()),
            approval: Some(entry.approval.as_str()),
            digest: Some(entry.digest.as_str()),
            answer: Some(entry.answer.name()),
            ..base
        }
    }
}

/// How many of the first `events` one body takes: as many as `request_records` and
/// `request_bytes` allow, and at least one (I7.2; ruling RD-37 (3)).
fn body_len(events: &[Event], limits: &Limits) -> usize {
    let records = events.len().min(limits.request_records.max(1) as usize);
    let mut n = 1;
    while n < records && log_bytes(&events[..=n]) <= limits.request_bytes as usize {
        n += 1;
    }
    n
}

/// Sends the waiting events in order, in bodies of at most `request_records` records and
/// `request_bytes` bytes, a row that alone passes `request_bytes` going as a body of its own, and
/// takes each acknowledged body off the list. A refusal `seq` reads the owner's events once and
/// sends again (I7.2); any other failure leaves the rest waiting.
pub fn flush<P: Owner>(ports: &P, ledger: &mut Ledger, limits: &Limits) -> Result<(), PortError> {
    let mut resynced = false;
    while !ledger.waiting.is_empty() {
        let n = body_len(&ledger.waiting, limits);
        match ports.log(&ledger.waiting[..n]) {
            Ok(()) => {
                ledger.last = ledger.waiting[n - 1].seq.get();
                let sent: Vec<Event> = ledger.waiting.drain(..n).collect();
                ledger.kept(&sent);
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

/// `flush` whose one read after a refusal `seq` is `read`: false where that read showed the
/// agent switched off beside its run, after which nothing more is sent (ruling RD-52 (b) rule 3);
/// a read that failed is the refusal.
fn flush_reading<P: Owner>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    read: &mut dyn FnMut(&mut Ledger) -> Read,
) -> Result<bool, PortError> {
    let mut resynced = false;
    while !ledger.waiting.is_empty() {
        let n = body_len(&ledger.waiting, limits);
        match ports.log(&ledger.waiting[..n]) {
            Ok(()) => {
                ledger.last = ledger.waiting[n - 1].seq.get();
                let sent: Vec<Event> = ledger.waiting.drain(..n).collect();
                ledger.kept(&sent);
            }
            Err(e) if refused_with(&e, "seq") && !resynced => {
                resynced = true;
                match read(ledger) {
                    Read::Failed => return Err(e),
                    Read::Off => return Ok(false),
                    Read::Taken => {}
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// The ledger's own read, for the functions that the coordinator does not hand its read.
fn own_read<P: Owner>(ports: &P, ledger: &mut Ledger) -> Read {
    resync(ports, ledger).map_or(Read::Failed, |()| Read::Taken)
}

/// Stores the raise `row` as `store` does, with `read` as the read after a refusal `seq`. Where
/// that read shows the agent switched off beside its run, the row leaves the ledger unsent and
/// the answer is `state`, so that the item is not raised (ruling RD-52 (b) rule 3); any other
/// failure leaves the row waiting, as `store` does.
fn store_raise<P: Owner + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    row: What,
    read: &mut dyn FnMut(&mut Ledger) -> Read,
) -> Result<(), Cause> {
    let Ok(stamp) = Stamp::read(ports) else {
        return Ok(());
    };
    let events = ledger.number(vec![row.clone()], stamp);
    ledger.waiting.extend(events);
    if let Ok(false) = flush_reading(ports, ledger, limits, read) {
        ledger.waiting.retain(|e| e.what != row);
        ledger.renumber();
        return Err(Cause::State);
    }
    Ok(())
}

/// Stores the events after the waiting ones, at the instant and on the day of one read of the
/// clock; an event that gets no acknowledgement waits and goes again with the next write or
/// `flush`.
pub fn store<P: Owner + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    whats: Vec<What>,
) -> Result<(), PortError> {
    let stamp = Stamp::read(ports)?;
    store_at(ports, ledger, limits, whats, stamp)
}

/// Stores the events after the waiting ones at the instant and on the day of `at`, read before
/// the move that makes them, so that no failure of the clock after the move can lose them; an event
/// that gets no acknowledgement waits and goes again with the next write or `flush`.
fn store_at<P: Owner>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    whats: Vec<What>,
    at: Stamp,
) -> Result<(), PortError> {
    let whats: Vec<What> = whats
        .into_iter()
        .filter(|w| closed_item(w).is_none_or(|item| ledger.closing(item).is_none()))
        .collect();
    let events = ledger.number(whats, at);
    ledger.waiting.extend(events);
    flush(ports, ledger, limits)
}

/// One read of the clock, or the cause of its failure.
fn clock_at<P: Clock>(ports: &P) -> Result<Stamp, Cause> {
    Stamp::read(ports).map_err(|e| cause_of(&e))
}

/// Stores the events in one body once the waiting ones are stored, and keeps none of them
/// waiting: rows that get no acknowledgement count as not stored (I4.5).
fn store_once<P: Owner>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    whats: Vec<What>,
    stamp: Stamp,
) -> Result<(), PortError> {
    flush(ports, ledger, limits)?;
    let mut events = ledger.number(whats.clone(), stamp);
    let mut resynced = false;
    loop {
        match ports.log(&events) {
            Ok(()) => {
                ledger.last = events.last().map_or(ledger.last, |e| e.seq.get());
                ledger.kept(&events);
                return Ok(());
            }
            Err(e) if refused_with(&e, "seq") && !resynced => {
                resync(ports, ledger)?;
                events = ledger.number(whats.clone(), stamp);
                resynced = true;
            }
            Err(e) => return Err(e),
        }
    }
}

/// The press of a choice on an item that a handback raised, its `item_answered` row and the
/// result that the row gives (ruling RD-61 (c)).
struct Press<'a> {
    kind: RunItemKind,
    item: &'a ItemId,
    answer: &'a Answered,
    result: HandbackResult,
}

/// What a press does, decided by the newest closing row of its item that the ledger holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decided {
    /// The row is the press's own: its result goes to the run, and the move answers as for an
    /// open item.
    Own(HandbackResult),
    /// Another answer decides: its result goes to the run, and the move answers `unknown_item`.
    Other(HandbackResult),
    /// The item lapsed: nothing goes to the run, and the move answers `unknown_item`.
    Lapsed,
    /// The press's own row goes to the owner.
    Log,
    /// The read showed the agent switched off beside the live run: nothing goes to the run and
    /// nothing more is stored, and the move answers `unknown_item` (ruling RD-52 (b) rule 3).
    Stopped,
}

/// What a read of the owner's events in a press gave (rulings RD-52 (b) rule 3, (c) rule 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read {
    /// The read failed.
    Failed,
    /// The ledger took the events.
    Taken,
    /// The ledger took the events, which show the agent's hold `off` standing beside its live
    /// run where the coordinator did not know it: the run's stop comes first.
    Off,
}

impl Press<'_> {
    /// The one decision of a press by the newest closing row `newest` (rulings RD-40, RD-61
    /// (c), (d)): none, or the older `done` that a declining press follows, logs the press's
    /// row; the press's own answer counts as stored; another answer decides with its own
    /// result; a lapse, or an answer whose result does not fit the kind, closes the item.
    fn decide(&self, newest: Option<&What>) -> Decided {
        match newest {
            None => Decided::Log,
            Some(What::ItemAnswered { answer, .. }) if answer == self.answer => {
                Decided::Own(self.result)
            }
            Some(What::ItemAnswered { answer, .. }) if self.declines_after(answer) => Decided::Log,
            Some(What::ItemAnswered { answer, .. }) => answer
                .choice()
                .and_then(|c| handback_result(self.kind, c).ok())
                .map_or(Decided::Lapsed, Decided::Other),
            Some(_) => Decided::Lapsed,
        }
    }

    /// Whether the press is the declining choice after a stored `done`, for a kind whose two
    /// choices are decline and done (rulings RD-46 B4, RD-61 (d)): the declining choice is the
    /// first of the kind's own choices, never one taken from a result, and a kind with other
    /// choices never declines after.
    fn declines_after(&self, stored: &Answered) -> bool {
        let [declining, done] = choices(self.kind.kind());
        [declining, done] == [Choice::Decline, Choice::Done]
            && stored.choice() == Some(done)
            && self.answer.choice() == Some(declining)
    }
}

/// Logs the row of a press (rulings RD-61 (c), RD-52 (c)): one log, and after its failure one
/// read by `read`, which takes the owner's events into the ledger; a read that shows the agent
/// switched off beside its run stops the press (RD-52 (b) rule 3).
/// `Press::decide` then decides by the newest closing row that the read shows: the press's own
/// row counts as its store, and another row decides. Where it gives `Log`, the row goes once more
/// after a refusal `seq`, and after any other failure only where the read showed the older `done`
/// that a declining press follows (RD-61 (f)); where the read showed no closing row of the item
/// the press's row is not known stored. At an acknowledgement the row is the item's newest and
/// the press's own. A read that failed, and a failure after the read, is the error.
fn log_press<P: Owner>(
    ports: &P,
    ledger: &mut Ledger,
    press: &Press<'_>,
    row: &What,
    stamp: Stamp,
    read: &mut dyn FnMut(&mut Ledger) -> Read,
) -> Result<Decided, PortError> {
    let mut events = ledger.number(vec![row.clone()], stamp);
    let mut once = false;
    loop {
        match ports.log(&events) {
            Ok(()) => {
                ledger.last = events.last().map_or(ledger.last, |e| e.seq.get());
                ledger.kept(&events);
                return Ok(Decided::Own(press.result));
            }
            Err(e) => {
                if once {
                    return Err(e);
                }
                match read(ledger) {
                    Read::Failed => return Err(e),
                    Read::Off => return Ok(Decided::Stopped),
                    Read::Taken => {}
                }
                once = true;
                let newest = ledger.closing(press.item).map(|e| &e.what);
                match press.decide(newest) {
                    Decided::Log if refused_with(&e, "seq") || newest.is_some() => {
                        events = ledger.number(vec![row.clone()], stamp);
                    }
                    Decided::Log => return Err(e),
                    decided => return Ok(decided),
                }
            }
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
    let mut read = |l: &mut Ledger| own_read(ports, l);
    issue_reading(ports, ledger, table, limits, ask, &mut read)
}

/// `issue` with `read` as the read after a refusal `seq` of its `item_raised` row, which the
/// coordinator hands: a read that shows the agent switched off beside its run issues nothing and
/// answers `state` (ruling RD-52 (b) rule 3).
pub fn issue_reading<P: Owner + Runs + Clock + Rng>(
    ports: &P,
    ledger: &mut Ledger,
    table: &Table,
    limits: &Limits,
    ask: &ConfirmAsk,
    read: &mut dyn FnMut(&mut Ledger) -> Read,
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
    store_raise(ports, ledger, limits, raised, read)?;
    Ok(held)
}

/// Notes the presenter's first report that the item is on screen, at `now`.
pub fn on_screen(held: &mut Held, now: u64) {
    if held.on_screen_at.is_none() {
        held.on_screen_at = Some(now);
    }
}

/// Arms a `shown` item whose first report is `arm_ms` or more before `now`, the clock that its
/// answer read before the coordinator's lock, so that the answer is taken in its own move
/// (ruling RD-80 5.3); nothing for any other item.
pub fn arm_at(held: &mut Held, now: u64, limits: &Limits) {
    if let Some(ConfirmMove::Arm) = due(held, now, limits) {
        if let Ok(armed) = confirm_step(Some(&held.confirmation), ConfirmMove::Arm) {
            held.confirmation = armed;
        }
    }
}

/// Forgets the report of an item that is still `shown`, so that its next show counts anew; an
/// armed item stays armed (ruling RD-80 5.6).
pub fn forget_report(held: &mut Held) {
    if *held.confirmation.state() == ConfirmState::Shown {
        held.on_screen_at = None;
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
    let stamp = clock_at(ports)?;
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
        store_once(ports, ledger, limits, rows, stamp)?;
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
        *table = table.stop(run);
        let _ = ports.cancel(run);
    }
    Ok(())
}

/// The lapse of the open item of `run`, of a kind other than `confirm`, with `cause` (I7.1):
/// stores its `item_lapsed`, and for `wait_limit` and `cancelled`, which stop the run, stops the
/// run in the guard's table and sends `Runs::cancel` once, the fact that every kind of item sets
/// at its lapse (ruling RD-6); the caller holds the run's stop in the guard's table and on the
/// board before it (ruling RD-48 (a)). `malformed` for a confirmation, whose lapse is `lapse`.
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
    if matches!(cause, OtherLapse::WaitLimit | OtherLapse::Cancelled) {
        *table = table.stop(run);
        let _ = ports.cancel(run);
    }
    Ok(())
}

/// The wait limit of an open item that a live run waits on, of a kind other than `confirm`
/// (I4.2, I5.7, ruling RD-34 (a)): answers the run's handback with `timeout`, as every such kind
/// but `show` comes from a handback, and lapses the item with `wait_limit`, which stops the run
/// in the guard's table and sends `Runs::cancel`.
pub fn time_out<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &mut Table,
    run: &RunId,
    open: &RunItem,
    limits: &Limits,
) -> Result<(), Cause> {
    if open.kind == RunItemKind::Confirm {
        return Err(Cause::Malformed);
    }
    if open.kind != RunItemKind::Show {
        let _ = ports.answer_handback(run, HandbackResult::Timeout);
    }
    lapse_item(
        ports,
        ledger,
        table,
        run,
        open,
        OtherLapse::WaitLimit,
        limits,
    )
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
    let mut read = |l: &mut Ledger| own_read(ports, l);
    raise_run_item_reading(ports, ledger, limits, run, kind, &mut read)
}

/// `raise_run_item` with `read` as the read after a refusal `seq` of its row, which the
/// coordinator hands: a read that shows the agent switched off beside its run raises nothing and
/// answers `state` (ruling RD-52 (b) rule 3).
pub fn raise_run_item_reading<P: Owner + Clock + Rng>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    run: &RunId,
    kind: RunItemKind,
    read: &mut dyn FnMut(&mut Ledger) -> Read,
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
    store_raise(ports, ledger, limits, row, read)?;
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

/// The person's choice on the item that the handback request `ask` raised (I3.1, I4.5). A choice
/// that is not one of the kind's, and a `link_account` request with no origin, are `malformed`.
/// The press is decided by `Press::decide` on the newest closing row of the item that the ledger
/// holds, but after a press whose row got no acknowledgement no result is handed from the kept
/// rows, and the press logs its own row first (the clause of ruling RD-61 (c)). The row is
/// stored by one log, and after a refusal `seq` one read decides again: the press's own answer
/// counts as stored and is not logged again; another answer decides, whose result the run gets,
/// and the move answers `unknown_item`, as for an item that is no longer open (rulings RD-34
/// (b) (2), RD-40); a lapse gives `unknown_item` and nothing; where the newest row is the older
/// `done` that a declining press of an account link follows, the decline goes as a row of its
/// own (ruling RD-46 B4). A row that for a `link_account` item holds the request's whole origin
/// (I7.1, ruling IG-14) goes to the run only once it is the newest row by the owner's
/// acknowledgement or by that read. A log that fails with no acknowledgement reads the owner's
/// events once too, and the read decides as after a refusal `seq`, so that a read that shows the
/// press's own row counts as its store and the run gets the answer in this move (ruling RD-52
/// (c)). Where no read decides, the answer is the store's cause, the run gets nothing and the
/// item's next press logs its own row first; a run that ended (`ended`) takes nothing back. The
/// read is the ledger's own; `answer_handback_reading` takes the coordinator's.
pub fn answer_handback<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    item: &ItemId,
    ask: &HandbackAsk,
    choice: Choice,
) -> Result<(), Cause> {
    let mut read = |l: &mut Ledger| own_read(ports, l);
    answer_handback_reading(ports, ledger, limits, item, ask, choice, &mut read)
}

/// `answer_handback` with `read` as the one read of the owner's events after a failed log of the
/// press's row: it takes the events into the ledger and says what it showed.
pub fn answer_handback_reading<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    limits: &Limits,
    item: &ItemId,
    ask: &HandbackAsk,
    choice: Choice,
    read: &mut dyn FnMut(&mut Ledger) -> Read,
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
    let stamp = clock_at(ports)?;
    let row = What::ItemAnswered {
        item: item.clone(),
        answer: answer.clone(),
    };
    let press = Press {
        kind,
        item,
        answer: &answer,
        result,
    };
    flush(ports, ledger, limits).map_err(|e| cause_of(&e))?;
    let decided = if ledger.unsure.contains(item) {
        Decided::Log
    } else {
        press.decide(ledger.closing(item).map(|e| &e.what))
    };
    let decided = match decided {
        Decided::Log => match log_press(ports, ledger, &press, &row, stamp, read) {
            Ok(decided) => decided,
            Err(e) => {
                if !ledger.unsure.contains(item) {
                    ledger.unsure.push(item.clone());
                }
                return Err(cause_of(&e));
            }
        },
        other => other,
    };
    ledger.unsure.retain(|i| i != item);
    let hand = |result| match ports.answer_handback(run, result) {
        Ok(()) => Ok(()),
        Err(e) if refused_with(&e, "ended") => Ok(()),
        Err(e) => Err(cause_of(&e)),
    };
    match decided {
        Decided::Own(result) => hand(result),
        Decided::Other(result) => hand(result).and(Err(Cause::UnknownItem)),
        Decided::Lapsed | Decided::Log | Decided::Stopped => Err(Cause::UnknownItem),
    }
}

/// The state of a held second step of a `sign_in` item: open for the person's answer, also after
/// a press whose row got no acknowledgement; its `item_answered` row stored and acknowledged
/// with this answer, which the run has not taken yet; or closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindState {
    /// Waits for the person.
    Open,
    /// The row of this answer is stored and acknowledged, and `Runs::answer_bind` has not
    /// taken it.
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
    let mut read = |l: &mut Ledger| own_read(ports, l);
    raise_bind_reading(ports, ledger, table, limits, ask, &mut read)
}

/// `raise_bind` with `read` as the read after a refusal `seq` of its `item_raised` row, which
/// the coordinator hands: a read that shows the agent switched off beside its run raises nothing
/// and answers `state` (ruling RD-52 (b) rule 3).
pub fn raise_bind_reading<P: Owner + Clock + Rng>(
    ports: &P,
    ledger: &mut Ledger,
    table: &Table,
    limits: &Limits,
    ask: &BindAsk,
    read: &mut dyn FnMut(&mut Ledger) -> Read,
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
    store_raise(ports, ledger, limits, raised, read)?;
    Ok(HeldBind {
        item,
        ask: ask.clone(),
        raised_at: now,
        state: BindState::Open,
    })
}

/// Whether a second step that is still held, open or with an acknowledged answer that the run
/// did not take, has waited `wait_limit_s` since it was raised, at `now`, so that it lapses with
/// `wait_limit` (I3.1, ruling RD-46 (2) e): the limit counts from the raise, and no press of the
/// person moves it.
pub fn bind_due(held: &HeldBind, now: u64, limits: &Limits) -> bool {
    let limit = held
        .raised_at
        .saturating_add(limits.wait_limit_s.saturating_mul(1000));
    held.state != BindState::Closed && now >= limit
}

/// The person's choice on a second step (I3.1): `stale` for a closed step, `malformed` for a
/// choice other than `decline` and `allow`, and `state` for a run that the guard's table holds as
/// stopped, after which no port is called. The press stores its `item_answered` row, which holds
/// the answer and the whole origin (I7.1, ruling IG-14), and only once the owner acknowledged it
/// calls `Runs::answer_bind` with the row's answer; with no acknowledgement the answer is the
/// store's cause, the run gets nothing and the step is open, so that the next press stores a row
/// of its own (ruling RD-46 (2) a, b). A step whose acknowledged answer the run did not take
/// sends that answer again for a press of the same choice and stores no second row; after a
/// stored `allow` the choice `decline` stores its row and then sends `decline`; after a stored
/// `decline` the choice `allow` is refused with `state`, and nothing is stored or sent (ruling
/// RD-46 (2) c, d). The step closes after a success of `Runs::answer_bind` and after its refusal
/// `ended`, which is dropped (ruling IG-18), and stays after any other error, which the port
/// returns only when the waiting call did not get the answer (ruling RD-46 (2) g).
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
        BindState::Recorded(stored) if stored == result => {}
        BindState::Recorded(BindResult::Decline) => return Err(Cause::State),
        _ => {
            let stamp = clock_at(ports)?;
            let row = What::ItemAnswered {
                item: held.item.clone(),
                answer: Answered::SignIn(SignInAnswer::Second(BindAnswer {
                    answer: result,
                    origin: held.ask.origin.clone(),
                })),
            };
            if let Err(e) = store_once(ports, ledger, limits, vec![row], stamp) {
                held.state = BindState::Open;
                return Err(cause_of(&e));
            }
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

/// A lapse of a second step with `cause` (I3.1, ruling IG-18), as a confirmation lapses, of an
/// open step and of one whose acknowledged answer the run did not take alike (ruling RD-46 (2)
/// e): `stale` for a closed step. For `wait_limit` and for `cancelled`, the person's stop of the
/// run (ruling RD-6), the run lives: the waiting call is answered `decline` through
/// `Runs::answer_bind`, whose refusal is dropped, so that a stored `allow` is never sent; then
/// `item_lapsed` is stored; then the run is stopped in the guard's table and through
/// `Runs::cancel`, the fact that `Act::Stop` reads. For `run_ended` and `app_ended` only
/// `item_lapsed` is stored. The step closes in each case.
pub fn lapse_bind<P: Owner + Runs + Clock>(
    ports: &P,
    ledger: &mut Ledger,
    table: &mut Table,
    held: &mut HeldBind,
    cause: OtherLapse,
    limits: &Limits,
) -> Result<(), Cause> {
    if held.state == BindState::Closed {
        return Err(Cause::Stale);
    }
    let run = held.ask.run.clone();
    let lives = match cause {
        OtherLapse::WaitLimit | OtherLapse::Cancelled => true,
        OtherLapse::RunEnded | OtherLapse::AppEnded => false,
    };
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
fn closed_row<P: Owner>(ports: &P, ledger: &mut Ledger, limits: &Limits, setup: &Setup, at: Stamp) {
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
    at: Stamp,
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
