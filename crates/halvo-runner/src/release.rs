use std::collections::VecDeque;
use std::num::NonZeroU32;

use halvo_connect::{reply_senders, same_address, NotMade};
use halvo_policy::{admit, decide, digest, Checkpoint, Decision, EffectMove, Facts, LIMITS};
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::{Confirm, ConfirmEffect, ConfirmReply, ConfirmRequest};
use halvo_schema::executable::names::{Answer, Autonomy, FieldName, LogEvent};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::V1;
use halvo_schema::executable::WebOrigin;
use halvo_schema::executable::{
    ActionClass, AgentConfig, Bound, Cause, EffectState, FromRunner, Outcome, ReplyState, ToRunner,
};

use crate::connect::slot;
use crate::link::Broken;
use crate::run::{Runner, Stop};

#[cfg(test)]
mod fake;
#[cfg(test)]
mod standin;
#[cfg(test)]
pub(crate) mod suite;

/// A state-changing request of one effect, paused before it leaves: the one port through which the runner lets
/// the request of an effect leave, whoever makes it (the coordinator's ruling CP-142). Dropping a paused request
/// fails it: no byte of it leaves.
pub(crate) trait Paused {
    /// The class of the effect, a confirm class of A9.3.
    fn class(&self) -> ActionClass;
    /// The bound object built again from `slots`, the slots of the request's class in the bindings of the load
    /// that the runner has just made, and from the request's own source read again while the request is paused
    /// (A5.1, A9.3, A6.3); the cause `changed` where a slot of the class is no longer in `slots`.
    fn bound(&mut self, slots: &[Slot]) -> Result<Bound, Cause>;
    /// What the maker knows from this run and not from a page's text (A9.3): the origin of the paused request, the
    /// ids and the senders that its own list calls returned, and nothing else. The origins of the bindings and the
    /// `new_origin` of an allowed confirmation are the runner's own facts (CP-166 E1).
    fn facts(&self) -> MakerFacts;
    /// Lets the paused request leave, once, and gives the request that left.
    fn release(self: Box<Self>) -> Box<dyn Left>;
    /// Fails the paused request, so that no byte of it leaves, and then either makes the request again, paused, so
    /// that the new request carries the value in `slots` of every bound slot of its class that a request of its
    /// kind carries, or answers `changed` when a bound slot of the class changed whose value its request does not
    /// carry (the coordinator's correction of CP-157 E1).
    fn renew(self: Box<Self>, slots: &[Slot]) -> Result<Box<dyn Paused>, Cause>;
    /// The reply that the request sends, for the request of `mail_send` alone: the runner tests its message against
    /// the reply rule before each `begin` and writes its rows of A7.4 `agent_reply`.
    fn reply(&self) -> Option<&Reply> {
        None
    }
    /// The step that makes the request: the step of the adapter's call, whose row `step_failed` with the cause
    /// `unauthorized` the runner writes after an answer 401 (A8.6), the send step of a reply and the delete step of an
    /// event; and the web step of an effect class, whose `stopped` row holds the cause of the read after the reply
    /// (crate::web::maker).
    fn step(&self) -> Option<NonZeroU32> {
        self.reply().map(|reply| reply.step)
    }
    /// Whether the request is one of `calendar_delete`, which always asks in this release (`always_asks`); the
    /// maker of that op alone answers yes.
    fn deletes_an_event(&self) -> bool {
        false
    }
    /// Whether the request is the one held request of a web step of an effect class, which always asks in this
    /// release (`always_asks`); the browser's maker (crate::web::maker) alone answers yes.
    fn of_a_web_step(&self) -> bool {
        false
    }
}

/// The reply that a request of `mail_send` sends (A5.1, A7.4 `agent_reply`): the send step's number, the provider's
/// ids of the message and of its draft, the message's sender as the run's own `mail_list` gave it, and the account
/// address under which that list selected the message, the joined text of the slot `account_address`.
#[derive(Clone)]
pub(crate) struct Reply {
    pub(crate) step: NonZeroU32,
    pub(crate) message: String,
    pub(crate) draft: String,
    pub(crate) sender: String,
    pub(crate) account: String,
}

impl Reply {
    /// Whether the reply rule still selects the message with the two slots of `loaded` (A5.1, CP-143 (3)): its
    /// sender stands in `reply_senders` as that load's reader reads it, and the account address under which it was
    /// selected is `account_address`, each held by the one function `same_address`. A slot that the reader refuses,
    /// a slot that the load does not hold and null bindings list nobody.
    fn selected_by(&self, loaded: &LoadReply) -> bool {
        let senders = slot(loaded, "reply_senders").and_then(|text| reply_senders(&text).ok());
        let account = slot(loaded, "account_address");
        senders.is_some_and(|senders| senders.holds(&self.sender))
            && account.is_some_and(|account| same_address(&account, &self.account))
    }
}

/// Whether `request` always asks in this release: a request of the class `send`, which `mail_send` alone makes, a
/// request of `calendar_delete` (A5.1, the coordinator's ruling of 2026-11-21T20:16:11Z, log line 11974, point
/// (3)), and the held request of a web step of an effect class (the coordinator's ruling of log line 14717, point
/// (7)). The runner lets no such request through without an `approved` row whose digest it has read again, whatever
/// the policy clears. The delete and the web step are named by their makers and not by a class, which the stand-in
/// makers of the port's tests take for effects that the policy clears and lets through.
fn always_asks(request: &dyn Paused) -> bool {
    request.class() == ActionClass::Send || request.deletes_an_event() || request.of_a_web_step()
}

/// The facts of A9.3 that a request maker gives: no maker can name an origin of the bindings or a new origin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MakerFacts {
    pub(crate) request_origin: Option<WebOrigin>,
    pub(crate) ids: Vec<String>,
    pub(crate) senders: Vec<String>,
}

/// What the runner reads of the page after a reply that the maker reads as made, all of it after the `commit` row
/// (r3 U5.10-B): every amount that the page names, in minor units, where the runner asks for them (a `payment` or a
/// `purchase`) and none where it asks for none, and `Err` where the page could not be read; whether the rule's record
/// holds a request of the page that the rule failed; and whether every condition `post` of the step holds on the page
/// after the reply, true for a step with none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PageAfter {
    pub(crate) amounts: Result<Vec<u64>, ()>,
    pub(crate) refused: bool,
    pub(crate) post_held: bool,
}

impl PageAfter {
    /// The read of a maker that reads no page: a page that names no amount, no failed request and no condition.
    fn of_no_page() -> PageAfter {
        PageAfter {
            amounts: Ok(Vec::new()),
            refused: false,
            post_held: true,
        }
    }
}

/// The read of the page after a reply that the maker reads as made, which the runner calls once after its `commit`
/// row, with whether it asks for the page's amounts.
pub(crate) type ReadPage = Box<dyn FnOnce(bool) -> PageAfter>;

/// A request of an effect that left.
pub(crate) trait Left {
    /// Waits for the request's reply, within the maker's own bound: whether it came, and whether the maker reads it
    /// as made.
    fn reply(self: Box<Self>) -> Replied;
    /// `reply`, and the read of the page after it where the maker reads one: none for a maker that reads no page,
    /// which the runner takes as a page that names no amount, holds no failed request and no condition.
    fn reply_and_page(self: Box<Self>) -> (Replied, Option<ReadPage>) {
        (self.reply(), None)
    }
}

/// The cause of the read of the page after the reply of a committed `payment` or `purchase` (contracts.md, the
/// pull request that first links a payment or purchase maker; the coordinator's rulings of log lines 14717, points (1)
/// and (2), and 15717), with the amount that the step's `stopped` row holds: `page_unread` where the page could not
/// be read; `amount_unread` where it names no amount; `amount_differs` where it names an amount that is not the kept
/// `amount` of `bound`, each amount read in minor units by `extract_amount`, with the first such amount that the page
/// names, none where it does not fit the row's count; none where every amount that it names is the kept one.
fn amount_cause(bound: &Bound, named: &Result<Vec<u64>, ()>) -> (Option<Cause>, Option<u32>) {
    let Ok(named) = named else {
        return (Some(Cause::PageUnread), None);
    };
    let kept = bound
        .fields()
        .iter()
        .find(|f| f.name == FieldName::Amount)
        .and_then(|f| halvo_web::extract_amount(&f.value.joined()));
    if named.is_empty() {
        return (Some(Cause::AmountUnread), None);
    }
    match named.iter().find(|n| Some(**n) != kept) {
        Some(other) => (Some(Cause::AmountDiffers), u32::try_from(*other).ok()),
        None => (None, None),
    }
}

/// What came of a request that left: an answer that the maker reads as made, from the one success status that its
/// request has; an answer that the maker reads as not made, with the case of A11.2 that `effect()` of halvo-connect
/// gives for a send or a delete, from which the runner takes the cause and reads no status; an answer that it reads
/// as neither, which may be a refusal, an error or a redirect; or no answer (CP-159 G4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Replied {
    Made,
    NotMade(NotMade),
    Answered,
    NoReply,
}

/// The end of a run whose effect `decide` refuses: `refused`, with the cause `autonomy` under `observe`.
fn refused(config: &AgentConfig) -> Stop {
    let cause = if config.autonomy == Autonomy::Observe {
        Cause::Autonomy
    } else {
        Cause::Checkpoint
    };
    Stop::End(Outcome::Refused, Some(cause))
}

fn failed(cause: Cause) -> Stop {
    Stop::End(Outcome::Failed, Some(cause))
}

/// The cause of `not_made` that a case of A11.2 gives (A5.2): the runner maps the case and reads no status.
fn not_made_cause(case: NotMade) -> Cause {
    match case {
        NotMade::NotSent => Cause::NotSent,
        NotMade::Unauthorized => Cause::Unauthorized,
        NotMade::Throttled => Cause::Throttled,
    }
}

/// The bound fields that a slot of the same name gives, the slots of a class (A5.1, A9.3, A9.4).
const SLOT_FIELDS: [FieldName; 6] = [
    FieldName::Payee,
    FieldName::Instrument,
    FieldName::Seller,
    FieldName::Document,
    FieldName::Party,
    FieldName::Fields,
];

/// The slots of the class `class` in the bindings of `loaded`, and no other slot; null bindings give none.
fn class_slots(loaded: &LoadReply, class: ActionClass) -> Vec<Slot> {
    let names: Vec<&str> = class
        .bound_fields()
        .iter()
        .filter(|f| SLOT_FIELDS.contains(f))
        .map(|f| f.as_str())
        .collect();
    loaded
        .bindings
        .as_ref()
        .map(|b| {
            b.slots
                .iter()
                .filter(|s| names.contains(&s.name.as_str()))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// The facts of `admit` and of the origin test before a `begin`, the runner's own: the maker's facts of its paused
/// request, the origins of the bindings of `loaded`, and `allowed`, the `new_origin` of the confirmation whose
/// `allow_once` approved the effect, none before an approval and none where that card named none, which is every
/// card of this runner (CP-159 part 2, CP-166 E1).
fn facts_of(request: &dyn Paused, loaded: &LoadReply, allowed: Option<&WebOrigin>) -> Facts {
    let maker = request.facts();
    Facts {
        origins: loaded
            .bindings
            .as_ref()
            .map(|b| b.origins.clone())
            .unwrap_or_default(),
        request_origin: maker.request_origin,
        new_origin: allowed.cloned(),
        senders: maker.senders,
        ids: maker.ids,
    }
}

/// The paused request of an effect of a batch of case (iv) made again from the slots of `loaded`, with the bound
/// object read from it after `admit` passed for it with that load's origins: the runner's own part, before the
/// new `propose`, of an effect that the batch proposes again.
fn made_again(
    request: Box<dyn Paused>,
    loaded: &LoadReply,
) -> Result<(Box<dyn Paused>, Bound), Stop> {
    let class = request.class();
    let mut request = request.renew(&class_slots(loaded, class)).map_err(failed)?;
    let bound = request.bound(&class_slots(loaded, class)).map_err(failed)?;
    admit(class, &bound, &facts_of(request.as_ref(), loaded, None))
        .map_err(|refusal| failed(refusal.cause()))?;
    Ok((request, bound))
}

/// One effect of the confirm path: its index among the run's effects, its paused request and the bound object
/// that its last row was made for.
struct Open {
    at: usize,
    request: Box<dyn Paused>,
    bound: Bound,
}

/// What `let_through` did with one effect, when the run goes on.
enum Through {
    /// The request left and its reply was read as made, so the effect is `committed`.
    Left,
    /// The request left and its reply was read as not made, with the cause `not_sent` or `throttled`: the effect is
    /// `not_made`, and the next effect of the call is let through (A6.5; the coordinator's ruling CP-226 item 8).
    NotMade,
    /// The reply rule no longer selects the message of a send at the load before its `begin`: the effect is ended,
    /// and the run goes on with its other effects (A5.1, A6.5).
    NotSelected,
    /// A `reject_once` on a later card for the effect ended it, and that effect alone.
    Rejected,
    /// A later load gives `confirm` for the `cleared` effect: the effect and that load, boxed for its size, for the
    /// batch of case (iv).
    Batch(Open, Box<LoadReply>),
}

impl Runner {
    /// The confirm path of A5.2 and A6.3 for the paused requests of one step: the adapter effects of a run
    /// together, or a browser effect alone. The load before the `propose` gives `decide`, the origins of `admit` and
    /// the limits; each effect is proposed after `admit`, and the effects that are `held` stand in one confirmation
    /// request, whose answers are applied before any later load is acted on. At the first request that fails `bound`
    /// or `admit` it returns, and the earlier effects of the call stay open until the store's handler of the run's end
    /// ends them. For each effect in turn a load precedes
    /// the `begin`: a `decide` on it that gives `refuse` ends the effect; one that gives `confirm` for a `cleared`
    /// effect ends it and every other effect that is still `cleared`, proposes them again from that load and asks
    /// for them in one confirmation request (CP-143 (4) (iv)), and an effect of that batch for which the load gives
    /// `refuse` is ended as above, with no new proposal, and the run ends `refused` (the coordinator's ruling of
    /// 2026-11-21T22:03:07Z, log line 12316 (1)); the real table gives no such batch today, because in one load it
    /// gives `refuse` beside a class that asks only for `local_edit`, which `Checkpoint::No` refuses at its first
    /// `propose`, so the arm keeps the count true for a later table; one that gives `run` changes nothing for an
    /// `approved` effect. The bound values are read again while the request is paused, and when their digest is not
    /// the row's an `approved` effect takes a `rebind` after `admit`, its request is failed and made again, and the
    /// person is asked again with the new values, at most `LIMITS.rebinds` times, then the effect is ended and the
    /// run ends `failed` with `changed`; a `cleared` one is ended and proposed again. The times that one step
    /// proposes effects again are at most `LIMITS.rebinds`, a batch of case (iv) counting once, after which every
    /// open effect of the step is ended and the run ends `failed` with `changed`. The end of a `cleared` effect that
    /// the runner proposes again holds the cause `reproposed`, and it is stored after the renewal of the request, the
    /// bound object, `admit`, the read of a `cancel` and the wall bound, right before the new `propose`; an exit
    /// before that `propose` and every end at `LIMITS.rebinds` hold `ended` (CP-181). `begin` carries the digest of
    /// the values read again; the request is let through once under the lock of the link, where a held `cancel`
    /// lets nothing through, makes the move `not_made` with the cause `not_sent` and ends the run `declined`, and
    /// `commit` is stored only after its reply. A reply that the maker reads as not made makes the move `not_made`
    /// with the cause of its case; after `not_sent` or `throttled` the next effect of the call is let through, each
    /// behind its own rows, and the run ends `failed` with the cause `adapter` after the call; `unauthorized` ends
    /// it so at once (CP-226 item 8). A request with no reply, or with an answer read as neither made nor not made,
    /// leaves its effect `releasing`, and the run ends `effect_unknown`.
    ///
    /// A request that always asks in this release (`always_asks`: a send or a delete of an event) goes to its `begin`
    /// only from an `approved` row whose digest the runner has read again; a `cleared` one is ended and the run ends
    /// `refused` with the cause `approval`. After an answer 401 the step of the request's adapter call gets the row
    /// `step_failed` with the cause `unauthorized`. For a request of `mail_send` the reply rule is tested again on
    /// the load before each `begin`, and a message that it no longer selects ends its effect, with the send step's
    /// `stopped` row and the cause `not_selected`, while the run goes on (A5.1); its reply row `begun` is stored and
    /// answered before the `begin` row, and after the reply the row `committed`, or `drafted` with the same draft
    /// after `not_made`.
    ///
    /// A `reject_once` ends its effect `rejected`, and that effect alone, whatever a later load gives, on the first
    /// card, on the card of a batch of case (iv) and on a later card for one effect; its paused request is failed and
    /// never let through. The other effects of the call that are `approved` or `cleared` go to their `begin` in their
    /// order, each after its own load (A5.1 case (i), CP-174 Part 1). When no effect of the call is open, one or more
    /// of them was ended by a `reject_once` and nothing else ended the run, the run makes no further step and ends
    /// `declined` with the cause `declined`, also when an effect of the run is `committed`; an end that the path of
    /// another effect gives after the reject is the run's end. After a reject this outcome says that one effect or
    /// more of the run was ended by a `reject_once`, the person's or the one that a lapse of its card sent, and says
    /// nothing of the other effects, which the effect rows show; such a run is never `completed` (A6.5).
    ///
    /// After the `commit` row of a reply that the maker reads as made, the runner reads the page after it
    /// (`after_the_reply`, r3 U5.10-B): for a `payment` or a `purchase`, a page that could not be read writes
    /// `page_unread` and one that names another amount than the kept `amount` writes `amount_differs` with that amount,
    /// each in the step's `stopped` row, the effect staying `committed`, and the run makes no further step and ends
    /// `failed` with that cause; one that names no amount writes `amount_unread` and the run goes on (the coordinator's
    /// rulings of log lines 14717, points (1) and (2), and 15717). Then a request of the page that the rule failed
    /// ends the run `refused` with `origin` (A9.2), and a condition `post` of the step that does not hold writes
    /// `step_failed` for the step and ends the run `failed`.
    pub(crate) fn release(&mut self, requests: Vec<Box<dyn Paused>>) -> Result<(), Stop> {
        let loaded = self.load()?;
        let mut open = VecDeque::new();
        for mut request in requests {
            self.check()?;
            self.within_wall()?;
            let bound = request
                .bound(&class_slots(&loaded, request.class()))
                .map_err(failed)?;
            let class = request.class();
            admit(class, &bound, &facts_of(request.as_ref(), &loaded, None))
                .map_err(|refusal| failed(refusal.cause()))?;
            let at = self.propose(decide(&loaded.config, class, Checkpoint::No), &bound)?;
            match self.effects[at].state {
                EffectState::Refused => return Err(refused(&loaded.config)),
                EffectState::Held | EffectState::Cleared => {
                    open.push_back(Open { at, request, bound })
                }
                _ => return Err(failed(Cause::Effect)),
            }
        }
        self.ask_held(open.iter())?;
        let mut renewed = 0;
        let mut declined = false;
        let mut not_made = false;
        while let Some(one) = open.pop_front() {
            if self.effects[one.at].state == EffectState::Rejected {
                declined = true;
                continue;
            }
            let (first, again) = match self.let_through(one, &mut renewed)? {
                Through::Left | Through::NotSelected => continue,
                Through::NotMade => {
                    not_made = true;
                    continue;
                }
                Through::Rejected => {
                    declined = true;
                    continue;
                }
                Through::Batch(first, again) => (first, *again),
            };
            let (cleared, rest): (Vec<Open>, Vec<Open>) = open
                .drain(..)
                .partition(|o| self.effects[o.at].state == EffectState::Cleared);
            let batch: Vec<Open> = std::iter::once(first).chain(cleared).collect();
            if renewed >= LIMITS.rebinds {
                for o in &batch {
                    self.advance(o.at, EffectMove::End)?;
                }
                return Err(failed(Cause::Changed));
            }
            renewed += 1;
            let mut asked = Vec::new();
            for o in batch {
                let class = o.request.class();
                if decide(&again.config, class, Checkpoint::No).decision() == Decision::Refuse {
                    self.advance(o.at, EffectMove::End)?;
                    return Err(refused(&again.config));
                }
                let made = made_again(o.request, &again);
                let (request, bound) = self.or_ended(o.at, made)?;
                let checked = self.check().and_then(|()| self.within_wall());
                self.or_ended(o.at, checked)?;
                let decided = decide(&again.config, request.class(), Checkpoint::No);
                let at = self.propose_again(o.at, decided, &bound)?;
                match self.effects[at].state {
                    EffectState::Refused => return Err(refused(&again.config)),
                    EffectState::Held | EffectState::Cleared => {
                        asked.push(Open { at, request, bound })
                    }
                    _ => return Err(failed(Cause::Effect)),
                }
            }
            self.ask_held(asked.iter())?;
            open = asked.into_iter().chain(rest).collect();
        }
        if declined {
            return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
        }
        if not_made {
            return Err(failed(Cause::Adapter));
        }
        Ok(())
    }

    /// The row `state` of A7.4 `agent_reply` for the reply that the request of the effect at `at` sends, with its
    /// draft and that effect, stored and answered before this returns; nothing for a request that sends no reply.
    fn reply_moved(
        &mut self,
        at: usize,
        reply: Option<&Reply>,
        state: ReplyState,
    ) -> Result<(), Stop> {
        let Some(reply) = reply else {
            return Ok(());
        };
        let effect = self.effects[at].id().clone();
        self.reply_row(&reply.message, state, Some(&reply.draft), Some(&effect))
    }

    /// The ends that the read of the page after a made reply gives, in this order (r3 U5.10-B): where `amounts`, the
    /// cause of `amount_cause` in the `stopped` row of `step` with its amount, after which `page_unread` and
    /// `amount_differs` end the run `failed`; a request of the page that the rule failed ends it `refused` with
    /// `origin` (A9.2); a condition `post` that does not hold writes `step_failed` for `step` and ends it `failed`.
    fn after_the_reply(
        &mut self,
        step: Option<NonZeroU32>,
        bound: &Bound,
        amounts: bool,
        after: &PageAfter,
    ) -> Result<(), Stop> {
        if amounts {
            if let (Some(cause), page_amount) = amount_cause(bound, &after.amounts) {
                self.log_amount(LogEvent::Stopped, step, Some(cause), page_amount)?;
                if cause != Cause::AmountUnread {
                    return Err(failed(cause));
                }
            }
        }
        if after.refused {
            return Err(Stop::End(Outcome::Refused, Some(Cause::Origin)));
        }
        if !after.post_held {
            self.log(LogEvent::StepFailed, step, Some(Cause::StepFailed))?;
            return Err(failed(Cause::StepFailed));
        }
        Ok(())
    }

    /// Asks the person about the effects of `open` that are `held`, together in one confirmation request.
    fn ask_held<'a>(&mut self, open: impl Iterator<Item = &'a Open>) -> Result<(), Stop> {
        let asked: Vec<(usize, Bound)> = open
            .filter(|o| self.effects[o.at].state == EffectState::Held)
            .map(|o| (o.at, o.bound.clone()))
            .collect();
        if asked.is_empty() {
            return Ok(());
        }
        let asked: Vec<(usize, &Bound)> = asked.iter().map(|(at, b)| (*at, b)).collect();
        self.confirm(&asked).map(|_| ())
    }

    /// Lets the request of one `cleared` or `approved` effect leave, or ends the effect, or gives the effect back
    /// with the later load when that load gives `confirm` for a `cleared` effect. Before each `begin`, and before the
    /// digests are compared, the origin of the paused request must be an origin of the later load's bindings or the
    /// `new_origin` of the confirmation whose `allow_once` approved the effect, the runner's own fact and never the
    /// maker's (none: the runner's confirmations name none in the mode `replay`), else the effect is
    /// ended and the run ends `refused` with `origin` (A9.2); then `admit` is asked with that load's origins, and its
    /// refusal ends the run `failed` with `provenance` (CP-159 G1). A `cleared` effect whose values
    /// changed is ended with the cause `reproposed`, after the renewal of its request, the read of a `cancel` and the
    /// wall bound, and a new effect is proposed from the load's values; `renewed` counts the times that the
    /// step proposes effects again (A5.2, CP-157 E4, CP-181). A `reject_once` on a later card for the effect, after a
    /// `rebind` or for a `cleared` effect proposed again as `held`, ends that effect alone: it gives back
    /// `Through::Rejected`, so that the loop goes on with the queue and reads the reject from that answer and never
    /// from the state `rejected`, which the runner's own `end` leaves too (CP-174 Part 1). The arm for a `cleared`
    /// effect proposed again as `held` is not reached at this head, because the `propose` there takes the decision of
    /// the load, the class and the checkpoint that the read above it gave `run` (Addition 1 of CP-174).
    fn let_through(&mut self, mut one: Open, renewed: &mut u32) -> Result<Through, Stop> {
        loop {
            let again = self.load()?;
            self.check()?;
            self.within_wall()?;
            let class = one.request.class();
            let decision = decide(&again.config, class, Checkpoint::No).decision();
            if decision == Decision::Refuse {
                self.advance(one.at, EffectMove::End)?;
                return Err(refused(&again.config));
            }
            if let Some(reply) = one.request.reply().filter(|r| !r.selected_by(&again)) {
                let step = reply.step;
                self.advance(one.at, EffectMove::End)?;
                self.log(LogEvent::Stopped, Some(step), Some(Cause::NotSelected))?;
                return Ok(Through::NotSelected);
            }
            if decision == Decision::Confirm && self.effects[one.at].state == EffectState::Cleared {
                return Ok(Through::Batch(one, Box::new(again)));
            }
            let now = match one.request.bound(&class_slots(&again, class)) {
                Ok(now) => now,
                Err(cause) => {
                    self.advance(one.at, EffectMove::End)?;
                    return Err(failed(cause));
                }
            };
            let facts = facts_of(
                one.request.as_ref(),
                &again,
                self.effects[one.at].allowed_origin(),
            );
            if facts
                .request_origin
                .as_ref()
                .is_some_and(|o| !facts.origins.contains(o) && facts.new_origin.as_ref() != Some(o))
            {
                self.advance(one.at, EffectMove::End)?;
                return Err(Stop::End(Outcome::Refused, Some(Cause::Origin)));
            }
            if admit(class, &now, &facts).is_err() {
                self.advance(one.at, EffectMove::End)?;
                return Err(failed(Cause::Provenance));
            }
            let read = digest(&now).map_err(|_| failed(Cause::StepFailed))?;
            if &read == self.effects[one.at].digest() {
                if always_asks(one.request.as_ref())
                    && self.effects[one.at].state != EffectState::Approved
                {
                    self.advance(one.at, EffectMove::End)?;
                    return Err(Stop::End(Outcome::Refused, Some(Cause::Approval)));
                }
                let reply = one.request.reply().cloned();
                let step = one.request.step();
                self.reply_moved(one.at, reply.as_ref(), ReplyState::Begun)?;
                self.advance(one.at, EffectMove::Begin { digest: read })?;
                let request = one.request;
                let Some(left) = self.link.settle(|| request.release())? else {
                    let not_sent = EffectMove::NotMade {
                        cause: Cause::NotSent,
                    };
                    self.advance(one.at, not_sent)?;
                    self.reply_moved(one.at, reply.as_ref(), ReplyState::Drafted)?;
                    return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
                };
                let (replied, page) = left.reply_and_page();
                return match replied {
                    Replied::Made => {
                        self.advance(one.at, EffectMove::Commit)?;
                        self.reply_moved(one.at, reply.as_ref(), ReplyState::Committed)?;
                        let amounts = matches!(class, ActionClass::Payment | ActionClass::Purchase);
                        let after = page.map_or_else(PageAfter::of_no_page, |read| read(amounts));
                        self.after_the_reply(step, &now, amounts, &after)?;
                        Ok(Through::Left)
                    }
                    Replied::NotMade(case) => {
                        let cause = not_made_cause(case);
                        self.advance(one.at, EffectMove::NotMade { cause })?;
                        self.reply_moved(one.at, reply.as_ref(), ReplyState::Drafted)?;
                        if case != NotMade::Unauthorized {
                            return Ok(Through::NotMade);
                        }
                        if let Some(step) = step {
                            self.log(LogEvent::StepFailed, Some(step), Some(cause))?;
                        }
                        Err(failed(Cause::Adapter))
                    }
                    Replied::Answered | Replied::NoReply => {
                        Err(Stop::End(Outcome::EffectUnknown, Some(Cause::Unknown)))
                    }
                };
            }
            let cleared = self.effects[one.at].state == EffectState::Cleared;
            let count = if cleared {
                *renewed
            } else {
                self.effects[one.at].rebinds()
            };
            if count >= LIMITS.rebinds {
                self.advance(one.at, EffectMove::End)?;
                return Err(failed(Cause::Changed));
            }
            if cleared {
                *renewed += 1;
                let request = one
                    .request
                    .renew(&class_slots(&again, class))
                    .map_err(failed);
                one.request = self.or_ended(one.at, request)?;
                let checked = self.check().and_then(|()| self.within_wall());
                self.or_ended(one.at, checked)?;
                let decided = decide(&again.config, class, Checkpoint::No);
                one.at = self.propose_again(one.at, decided, &now)?;
                one.bound = now;
                match self.effects[one.at].state {
                    EffectState::Cleared => continue,
                    EffectState::Held => {
                        if self.confirm(&[(one.at, &one.bound)])?.contains(&one.at) {
                            return Ok(Through::Rejected);
                        }
                        continue;
                    }
                    EffectState::Refused => return Err(refused(&again.config)),
                    _ => return Err(failed(Cause::Effect)),
                }
            }
            one.request = match one.request.renew(&class_slots(&again, class)) {
                Ok(request) => request,
                Err(cause) => {
                    self.advance(one.at, EffectMove::End)?;
                    return Err(failed(cause));
                }
            };
            self.rebind(one.at, &now)?;
            one.bound = now;
            if self.confirm(&[(one.at, &one.bound)])?.contains(&one.at) {
                return Ok(Through::Rejected);
            }
        }
    }

    /// Asks the person about the `held` effects of `asked`, each with the bound object of its last row, in one
    /// confirmation request (A6.3), and moves each by its answer; the wait is no time of the run's `wall_s`. The card
    /// gives one answer for every effect of its request, the decision carries it as one entry for each effect, and
    /// the runner reads each entry for its effect alone. When the decision has come, each effect's approval is found
    /// first, else `not_the_decision` before any row; then the `rejected` row of each `reject_once` is stored, in the
    /// request's order; then a held `cancel` is read, at once, at this place, and it ends the run `declined` with no
    /// further load and no `approved` row; only then is the `approved` row of each `allow_once` stored, in the
    /// request's order (A5.1 case (i), A6.2, CP-174 Part 2). An effect whose `allow_once` was not applied stays
    /// `held` until the handler of the run's end ends it (A7.5). The effects that a `reject_once` ended come back.
    fn confirm(&mut self, asked: &[(usize, &Bound)]) -> Result<Vec<usize>, Stop> {
        let request = ConfirmRequest {
            v: V1,
            run: self.args.run.clone(),
            agent: self.args.agent.clone(),
            cut: None,
            effects: asked
                .iter()
                .map(|(at, bound)| ConfirmEffect {
                    effect: self.effects[*at].id().clone(),
                    class: bound.class(),
                    digest: self.effects[*at].digest().clone(),
                    bound: (*bound).clone(),
                    new_origin: None,
                })
                .collect(),
        };
        if let Err(Stop::Link(broken)) = self.log(LogEvent::Confirm, None, None) {
            return Err(Stop::Link(broken));
        }
        self.confirmations += 1;
        let frame = FromRunner::Confirm(Confirm {
            request: request.clone(),
        });
        let since = self.elapsed();
        let reply = self.link.ask(&frame);
        self.waited += self.elapsed().saturating_sub(since);
        let decision = match reply? {
            ToRunner::ConfirmReply(ConfirmReply { decision }) => decision,
            _ => return Err(Broken::Frame("not_a_confirm_reply").into()),
        };
        if decision.answers(&request).is_err() {
            return Err(Broken::Frame("not_the_decision").into());
        }
        let mut answers = Vec::with_capacity(asked.len());
        for (at, _) in asked {
            let id = self.effects[*at].id().clone();
            let Some(approval) = decision.approvals.iter().find(|a| a.effect == id) else {
                return Err(Broken::Frame("not_the_decision").into());
            };
            let shown = request
                .effects
                .iter()
                .find(|e| e.effect == id)
                .and_then(|e| e.new_origin.clone());
            answers.push((*at, approval, shown));
        }
        let mut rejected = Vec::new();
        for (at, approval, _) in &answers {
            match approval.answer {
                Answer::RejectOnce => {
                    self.advance(*at, EffectMove::Reject)?;
                    rejected.push(*at);
                }
                Answer::AllowOnce => {}
            }
        }
        self.check()?;
        for (at, approval, shown) in answers {
            match approval.answer {
                Answer::AllowOnce => self.approve(at, approval, shown)?,
                Answer::RejectOnce => {}
            }
        }
        Ok(rejected)
    }
}
