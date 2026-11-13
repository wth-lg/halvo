use std::collections::VecDeque;

use halvo_policy::{admit, decide, digest, Checkpoint, Decision, EffectMove, Facts, LIMITS};
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::{Confirm, ConfirmEffect, ConfirmReply, ConfirmRequest};
use halvo_schema::executable::names::{Answer, Autonomy, FieldName, LogEvent};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::V1;
use halvo_schema::executable::WebOrigin;
use halvo_schema::executable::{
    ActionClass, AgentConfig, Bound, Cause, EffectState, FromRunner, Outcome, ToRunner,
};

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
}

/// The facts of A9.3 that a request maker gives: no maker can name an origin of the bindings or a new origin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MakerFacts {
    pub(crate) request_origin: Option<WebOrigin>,
    pub(crate) ids: Vec<String>,
    pub(crate) senders: Vec<String>,
}

/// A request of an effect that left.
pub(crate) trait Left {
    /// Waits for the request's reply, within the maker's own bound: whether it came, and whether the maker reads it
    /// as made.
    fn reply(self: Box<Self>) -> Replied;
}

/// What came of a request that left: an answer that the maker reads as made, from the one success status that its
/// request has; an answer that it does not read as made, which may be a refusal, an error or a redirect; or no
/// answer (CP-159 G4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Replied {
    Made,
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
    /// for them in one confirmation request (CP-143 (4) (iv)); one that gives `run` changes nothing for an
    /// `approved` effect. The bound values are read again while the request is paused, and when their digest is not
    /// the row's an `approved` effect takes a `rebind` after `admit`, its request is failed and made again, and the
    /// person is asked again with the new values, at most `LIMITS.rebinds` times, then the effect is ended and the
    /// run ends `failed` with `changed`; a `cleared` one is ended and proposed again. The times that one step
    /// proposes effects again are at most `LIMITS.rebinds`, a batch of case (iv) counting once, after which every
    /// open effect of the step is ended and the run ends `failed` with `changed`. `begin` carries the digest of
    /// the values read again; the request is let through once under the lock of the link, where a held `cancel`
    /// lets nothing through, and `commit` is stored only after its reply. A request with no reply leaves its effect
    /// `releasing`, and the run ends `effect_unknown`.
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
        while let Some(one) = open.pop_front() {
            if self.effects[one.at].state == EffectState::Rejected {
                declined = true;
                continue;
            }
            let (first, again) = match self.let_through(one, &mut renewed)? {
                Through::Left => continue,
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
            for mut o in batch {
                self.advance(o.at, EffectMove::End)?;
                let class = o.request.class();
                o.request = o
                    .request
                    .renew(&class_slots(&again, class))
                    .map_err(failed)?;
                o.bound = o
                    .request
                    .bound(&class_slots(&again, class))
                    .map_err(failed)?;
                admit(class, &o.bound, &facts_of(o.request.as_ref(), &again, None))
                    .map_err(|refusal| failed(refusal.cause()))?;
                self.check()?;
                self.within_wall()?;
                o.at = self.propose(decide(&again.config, class, Checkpoint::No), &o.bound)?;
                match self.effects[o.at].state {
                    EffectState::Refused => return Err(refused(&again.config)),
                    EffectState::Held | EffectState::Cleared => asked.push(o),
                    _ => return Err(failed(Cause::Effect)),
                }
            }
            self.ask_held(asked.iter())?;
            open = asked.into_iter().chain(rest).collect();
        }
        if declined {
            return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
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
    /// changed is ended and a new effect is proposed from the load's values; `renewed` counts the times that the
    /// step proposes effects again (A5.2, CP-157 E4). A `reject_once` on a later card for the effect, after a
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
                self.advance(one.at, EffectMove::Begin { digest: read })?;
                let request = one.request;
                let Some(left) = self.link.settle(|| request.release())? else {
                    return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
                };
                return match left.reply() {
                    Replied::Made => self
                        .advance(one.at, EffectMove::Commit)
                        .map(|_| Through::Left),
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
                self.advance(one.at, EffectMove::End)?;
                *renewed += 1;
                one.request = one
                    .request
                    .renew(&class_slots(&again, class))
                    .map_err(failed)?;
                self.check()?;
                self.within_wall()?;
                one.at = self.propose(decide(&again.config, class, Checkpoint::No), &now)?;
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
