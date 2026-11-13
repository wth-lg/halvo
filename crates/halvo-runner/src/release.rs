use std::time::Instant as Clock;

use halvo_policy::{admit, decide, digest, Checkpoint, Decision, EffectMove, Facts, LIMITS};
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::{
    Approval, Confirm, ConfirmEffect, ConfirmReply, ConfirmRequest,
};
use halvo_schema::executable::names::{Answer, Autonomy, LogEvent};
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
    /// The bound object read again from the request's source while the request is paused (A9.3, A6.3).
    fn bound(&mut self) -> Result<Bound, Cause>;
    /// The facts of A9.3 that the maker knows from this run and not from a page's text: the origin of the paused
    /// request, the ids and the senders that its own list calls returned. The runner adds the origins of the
    /// bindings of its newest load.
    fn facts(&self) -> Facts;
    /// Lets the paused request leave, once, and gives the request that left.
    fn release(self: Box<Self>) -> Box<dyn Left>;
    /// Fails the paused request, so that no byte of it leaves, and makes the same request again, paused, for an
    /// effect that is asked again with new values.
    fn renew(self: Box<Self>) -> Result<Box<dyn Paused>, Cause>;
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
    /// request. For each effect in turn a load precedes the `begin`: a `decide` on it that gives `refuse` ends the
    /// effect; the bound values are read again while the request is paused, and when their digest is not the row's
    /// the effect takes a `rebind` after `admit`, its request is failed and made again, and the person is asked
    /// again with the new values, at most `LIMITS.rebinds` times, then the effect is ended and the run ends `failed`
    /// with `changed`. `begin` carries the digest of the values read again; the request is let through once under
    /// the lock of the link, where a held `cancel` lets nothing through, and `commit` is stored only after its
    /// reply. A request with no reply leaves its effect `releasing`, and the run ends `effect_unknown`.
    pub(crate) fn release(&mut self, requests: Vec<Box<dyn Paused>>) -> Result<(), Stop> {
        let loaded = self.load()?;
        let mut open = Vec::new();
        for mut request in requests {
            self.check()?;
            self.within_wall()?;
            let bound = request.bound().map_err(failed)?;
            let class = request.class();
            admit(class, &bound, &facts_of(request.as_ref(), &loaded))
                .map_err(|refusal| failed(refusal.cause()))?;
            let at = self.propose(decide(&loaded.config, class, Checkpoint::No), &bound)?;
            match self.effects[at].state {
                EffectState::Refused => return Err(refused(&loaded.config)),
                EffectState::Held | EffectState::Cleared => open.push(Open { at, request, bound }),
                _ => return Err(failed(Cause::Effect)),
            }
        }
        let held: Vec<usize> = (0..open.len())
            .filter(|&i| self.effects[open[i].at].state == EffectState::Held)
            .collect();
        if !held.is_empty() {
            let asked: Vec<(usize, &Bound)> =
                held.iter().map(|&i| (open[i].at, &open[i].bound)).collect();
            self.confirm(&asked)?;
        }
        let mut declined = false;
        for one in open {
            if self.effects[one.at].state == EffectState::Rejected {
                declined = true;
                continue;
            }
            self.let_through(one)?;
        }
        if declined {
            return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
        }
        Ok(())
    }

    /// Lets the request of one `cleared` or `approved` effect leave, or ends the effect. A `cleared` effect whose
    /// values changed is ended and a new effect is proposed from the load's values, at most `LIMITS.rebinds` times
    /// (A5.2).
    fn let_through(&mut self, mut one: Open) -> Result<(), Stop> {
        let mut renewed = 0;
        loop {
            let again = self.load()?;
            self.check()?;
            self.within_wall()?;
            let class = one.request.class();
            if decide(&again.config, class, Checkpoint::No).decision() == Decision::Refuse {
                self.advance(one.at, EffectMove::End)?;
                return Err(refused(&again.config));
            }
            let now = match one.request.bound() {
                Ok(now) => now,
                Err(cause) => {
                    self.advance(one.at, EffectMove::End)?;
                    return Err(failed(cause));
                }
            };
            let read = digest(&now).map_err(|_| failed(Cause::StepFailed))?;
            if &read == self.effects[one.at].digest() {
                self.advance(one.at, EffectMove::Begin { digest: read })?;
                let request = one.request;
                let Some(left) = self.link.settle(|| request.release())? else {
                    return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
                };
                return match left.reply() {
                    Replied::Reply => self.advance(one.at, EffectMove::Commit).map(|_| ()),
                    Replied::NoReply => {
                        Err(Stop::End(Outcome::EffectUnknown, Some(Cause::Unknown)))
                    }
                };
            }
            let cleared = self.effects[one.at].state == EffectState::Cleared;
            let count = if cleared {
                renewed
            } else {
                self.effects[one.at].rebinds()
            };
            if count >= LIMITS.rebinds {
                self.advance(one.at, EffectMove::End)?;
                return Err(failed(Cause::Changed));
            }
            if admit(class, &now, &facts_of(one.request.as_ref(), &again)).is_err() {
                self.advance(one.at, EffectMove::End)?;
                return Err(failed(Cause::Provenance));
            }
            if cleared {
                self.advance(one.at, EffectMove::End)?;
                renewed += 1;
                one.request = one.request.renew().map_err(failed)?;
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
            self.advance(
                one.at,
                EffectMove::Rebind {
                    digest: read,
                    class,
                },
            )?;
            one.request = one.request.renew().map_err(failed)?;
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
        let since = Clock::now();
        let reply = self.link.ask(&frame);
        self.waited += since.elapsed();
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
