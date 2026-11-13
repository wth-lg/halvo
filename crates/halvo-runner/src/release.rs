use std::collections::VecDeque;

use halvo_policy::{admit, decide, digest, Checkpoint, Decision, EffectMove, Facts, LIMITS};
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::{
    Approval, Confirm, ConfirmEffect, ConfirmReply, ConfirmRequest,
};
use halvo_schema::executable::names::{Answer, Autonomy, FieldName, LogEvent};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::V1;
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
    /// The facts of A9.3 that the maker knows from this run and not from a page's text: the origin of the paused
    /// request, the ids and the senders that its own list calls returned. The runner adds the origins of the
    /// bindings of its newest load.
    fn facts(&self) -> Facts;
    /// Lets the paused request leave, once, and gives the request that left.
    fn release(self: Box<Self>) -> Box<dyn Left>;
    /// Fails the paused request, so that no byte of it leaves, and then either makes the request again, paused, so
    /// that the new request carries the value in `slots` of every bound slot of its class that a request of its
    /// kind carries, or answers `changed` when a bound slot of the class changed whose value its request does not
    /// carry (the coordinator's correction of CP-157 E1).
    fn renew(self: Box<Self>, slots: &[Slot]) -> Result<Box<dyn Paused>, Cause>;
}

/// A request of an effect that left.
pub(crate) trait Left {
    /// Waits for the request's reply, within the maker's own bound; whether it came.
    fn reply(self: Box<Self>) -> Replied;
}

/// Whether the reply to a request that left came.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Replied {
    Reply,
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

/// The facts of a paused request with the origins of the bindings of `loaded`.
fn facts_of(request: &dyn Paused, loaded: &LoadReply) -> Facts {
    let mut facts = request.facts();
    facts.origins = loaded
        .bindings
        .as_ref()
        .map(|b| b.origins.clone())
        .unwrap_or_default();
    facts
}

/// One effect of the confirm path: its index among the run's effects, its paused request and the bound object
/// that its last row was made for.
struct Open {
    at: usize,
    request: Box<dyn Paused>,
    bound: Bound,
}

impl Runner {
    /// The confirm path of A5.2 and A6.3 for the paused requests of one step: the adapter effects of a run
    /// together, or a browser effect alone. The load before the `propose` gives `decide`, the origins of `admit` and
    /// the limits; each effect is proposed after `admit`, and the effects that are `held` stand in one confirmation
    /// request, whose answers are applied before any later load is acted on. For each effect in turn a load precedes
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
            admit(class, &bound, &facts_of(request.as_ref(), &loaded))
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
            let Some((first, again)) = self.let_through(one, &mut renewed)? else {
                continue;
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
                admit(class, &o.bound, &facts_of(o.request.as_ref(), &again))
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
        self.confirm(&asked)
    }

    /// Lets the request of one `cleared` or `approved` effect leave, or ends the effect, or gives the effect back
    /// with the later load when that load gives `confirm` for a `cleared` effect. Before each `begin`, and before the
    /// digests are compared, the origin of the paused request must be an origin of the later load's bindings or the
    /// allowed `new_origin` (none: the runner's confirmations name none in the mode `replay`), else the effect is
    /// ended and the run ends `refused` with `origin` (A9.2); then `admit` is asked with that load's origins, and its
    /// refusal ends the run `failed` with `provenance` (CP-159 G1). A `cleared` effect whose values
    /// changed is ended and a new effect is proposed from the load's values; `renewed` counts the times that the
    /// step proposes effects again (A5.2, CP-157 E4).
    fn let_through(
        &mut self,
        mut one: Open,
        renewed: &mut u32,
    ) -> Result<Option<(Open, LoadReply)>, Stop> {
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
                return Ok(Some((one, again)));
            }
            let now = match one.request.bound(&class_slots(&again, class)) {
                Ok(now) => now,
                Err(cause) => {
                    self.advance(one.at, EffectMove::End)?;
                    return Err(failed(cause));
                }
            };
            let facts = facts_of(one.request.as_ref(), &again);
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
                    Replied::Reply => self.advance(one.at, EffectMove::Commit).map(|_| None),
                    Replied::NoReply => {
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
                        self.confirm(&[(one.at, &one.bound)])?;
                        if self.effects[one.at].state == EffectState::Rejected {
                            return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
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
            self.confirm(&[(one.at, &one.bound)])?;
            if self.effects[one.at].state == EffectState::Rejected {
                return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
            }
        }
    }

    /// Asks the person about the `held` effects of `asked`, each with the bound object of its last row, in one
    /// confirmation request (A6.3), and moves each by its answer: `approve` for `allow_once`, `reject` for
    /// `reject_once`. The wait is no time of the run's `wall_s`.
    fn confirm(&mut self, asked: &[(usize, &Bound)]) -> Result<(), Stop> {
        let request = ConfirmRequest {
            v: V1,
            run: self.args.run.clone(),
            agent: self.args.agent.clone(),
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
        self.check()?;
        for (at, _) in asked {
            let id = self.effects[*at].id().clone();
            let Some(approval) = decision.approvals.iter().find(|a| a.effect == id) else {
                return Err(Broken::Frame("not_the_decision").into());
            };
            self.answer(*at, approval)?;
        }
        Ok(())
    }

    /// The move that the person's answer for the effect at `at` makes.
    fn answer(&mut self, at: usize, approval: &Approval) -> Result<(), Stop> {
        match approval.answer {
            Answer::AllowOnce => self.approve(at, approval),
            Answer::RejectOnce => self.advance(at, EffectMove::Reject).map(|_| ()),
        }
    }
}
