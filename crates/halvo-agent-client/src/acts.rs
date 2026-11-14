//! The acts with the person's answer: a confirmation, a handback, and the second step of a sign-in, with the
//! `add_origin` that each of the three ways sends, again until the owner acknowledges it or the run ends.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use halvo_schema::executable::body::{AddOrigin, CoordinatorCall};
use halvo_schema::executable::frame::{
    Approval, BindReply, ConfirmDecision, ConfirmReply, ConfirmRequest, HandbackReply,
    HandbackRequest, HandbackResult, ToRunner,
};
use halvo_schema::executable::names::{Answer, HandbackOutcome, HandbackReason, Way, YesNo};
use halvo_schema::executable::text::{ApprovalId, WebOrigin, V1};
use halvo_schema::RequestId;

use crate::api::{CallError, OriginAnswer};
use crate::owner::{self, Ack};
use crate::supervisor::{Flow, Running};
use crate::sys;

/// How long the library waits before it sends an `add_origin` that was not acknowledged again.
pub(crate) const ADD_AGAIN: Duration = Duration::from_millis(500);

/// How many sends of one `add_origin` in all, the first and two repeats, may meet a reply that `decode_reply`
/// refuses before the library gives the body up (ruling CP-193 item 4).
pub(crate) const ADD_REFUSED_MAX: u8 = 3;

/// What the runner waits for while the library holds its reply.
#[derive(Default)]
pub(crate) enum Waiting {
    #[default]
    Nothing,
    Handback(HandbackRequest),
    Bind,
    Link,
    /// The library wrote `cancel` while the runner waited for a `bind_reply`: it writes none, and takes the store of
    /// the run's `finish` and its done (ruling CP-193 item 3).
    CancelledBind,
}

enum Then {
    BindReply,
    HandbackReply(HandbackResult),
    Nothing,
}

struct Pending {
    id: RequestId,
    origin: WebOrigin,
    way: Way,
    then: Then,
    refused: u8,
}

/// The state of a run's acts: the origins that the bindings hold, what the runner waits for, whether a `bind` is in
/// turn, and the `add_origin` bodies that wait for their acknowledgement.
#[derive(Default)]
pub(crate) struct Acts {
    pub(crate) bound: HashSet<WebOrigin>,
    waiting: Waiting,
    bind_turn: bool,
    declined: bool,
    adds: Vec<Pending>,
    next_try: Option<Instant>,
}

impl Acts {
    /// Whether the runner waits for a reply that the library holds.
    pub(crate) fn waiting(&self) -> bool {
        !matches!(self.waiting, Waiting::Nothing)
    }

    /// Whether the library dropped an open bind because it wrote `cancel`.
    pub(crate) fn cancelled_bind(&self) -> bool {
        matches!(self.waiting, Waiting::CancelledBind)
    }

    /// Whether the library wrote `bound=no` to the runner: from then on it takes progress, done and the store of the
    /// run's `finish` alone (ruling CP-193 item 2).
    pub(crate) fn declined(&self) -> bool {
        self.declined
    }
}

fn mint() -> Option<ApprovalId> {
    sys::random16().ok().map(ApprovalId::from_random)
}

fn decision(request: &ConfirmRequest, answers: &[Answer]) -> Option<ConfirmDecision> {
    let approvals = request
        .effects
        .iter()
        .zip(answers)
        .map(|(effect, answer)| {
            Some(Approval {
                approval: mint()?,
                effect: effect.effect.clone(),
                digest: effect.digest.clone(),
                answer: *answer,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ConfirmDecision { v: V1, approvals })
}

impl Running<'_> {
    pub(crate) fn confirm(&mut self, request: &ConfirmRequest) -> Flow {
        let spec = &self.slot.spec;
        if request.run != spec.run || request.agent != spec.agent {
            return Flow::Lost;
        }
        let confirmer = &self.inner.parts.confirmer;
        let rejected = vec![Answer::RejectOnce; request.effects.len()];
        let answers = confirmer.answers(&spec.run, request);
        let answers = if answers.len() == request.effects.len() {
            answers
        } else {
            rejected.clone()
        };
        let Some(first) = decision(request, &answers) else {
            return Flow::Lost;
        };
        let released = match confirmer.issued(&spec.run, &first) {
            Ok(()) => first,
            Err(_) => match decision(request, &rejected) {
                Some(d) => d,
                None => return Flow::Lost,
            },
        };
        let reply = ToRunner::ConfirmReply(ConfirmReply {
            decision: released.clone(),
        });
        if !self.slot.write(&reply) {
            return Flow::Lost;
        }
        for (effect, approval) in request.effects.iter().zip(&released.approvals) {
            if let (Some(origin), Answer::AllowOnce) = (&effect.new_origin, approval.answer) {
                if !self.queue_add(origin.clone(), Way::NewOrigin, Then::Nothing) {
                    return Flow::Lost;
                }
            }
        }
        self.try_adds()
    }

    pub(crate) fn handback(&mut self, request: HandbackRequest) -> Flow {
        let spec = &self.slot.spec;
        let link_without_origin =
            request.reason == HandbackReason::AccountLink && request.origin.is_none();
        if request.run != spec.run || request.agent != spec.agent || link_without_origin {
            return Flow::Lost;
        }
        self.acts.bind_turn = false;
        self.inner.parts.human.handback(&spec.run, &request);
        self.acts.waiting = Waiting::Handback(request);
        Flow::Go
    }

    /// Applies the person's result to the open handback: the result reaches the runner at once, or for a done
    /// `account_link` once the `add_origin` of the request's origin is acknowledged. `ended` when no handback is
    /// open, since no answer is then applied.
    pub(crate) fn answer(&mut self, result: HandbackResult) -> Result<(), CallError> {
        if !matches!(self.acts.waiting, Waiting::Handback(_)) {
            return Err(CallError::Ended);
        }
        let Waiting::Handback(request) = std::mem::take(&mut self.acts.waiting) else {
            return Err(CallError::Ended);
        };
        let done = result.outcome == HandbackOutcome::Done;
        let link = request.reason == HandbackReason::AccountLink && done;
        let signed_in = done
            && matches!(
                request.reason,
                HandbackReason::SignIn | HandbackReason::SecondFactor | HandbackReason::Captcha
            );
        match (link, request.origin) {
            (true, Some(origin)) => {
                self.acts.waiting = Waiting::Link;
                if !self.queue_add(origin, Way::AccountLink, Then::HandbackReply(result)) {
                    return Err(CallError::Ended);
                }
                let _ = self.try_adds();
            }
            _ => {
                if !self
                    .slot
                    .write(&ToRunner::HandbackReply(HandbackReply { result }))
                {
                    return Err(CallError::Ended);
                }
                self.acts.bind_turn = signed_in;
            }
        }
        Ok(())
    }

    /// Takes a `bind` once after a result `done` of a handback of the reason `sign_in`, `second_factor` or
    /// `captcha` of the run; any other `bind` is a frame out of turn, for which the library asks nobody, sends no
    /// `add_origin` and ends the run as a lost flow (contracts.md A6.6, ruling CP-135 (7)).
    pub(crate) fn bind(&mut self, origin: WebOrigin) -> Flow {
        if !std::mem::take(&mut self.acts.bind_turn) {
            return Flow::Lost;
        }
        if self.acts.bound.contains(&origin) {
            return self.bind_reply(YesNo::Yes);
        }
        let answer = self
            .inner
            .parts
            .human
            .allow_origin(&self.slot.spec.run, &origin);
        if self.slot.cancelled() {
            self.acts.waiting = Waiting::CancelledBind;
            return Flow::Go;
        }
        match answer {
            OriginAnswer::Decline => self.bind_reply(YesNo::No),
            OriginAnswer::Allow => {
                self.acts.waiting = Waiting::Bind;
                if !self.queue_add(origin, Way::SignIn, Then::BindReply) {
                    return Flow::Lost;
                }
                self.try_adds()
            }
        }
    }

    fn bind_reply(&mut self, bound: YesNo) -> Flow {
        match self
            .slot
            .write_unless_cancelled(&ToRunner::BindReply(BindReply { bound }))
        {
            Some(true) => {
                self.acts.declined |= bound == YesNo::No;
                Flow::Go
            }
            Some(false) => Flow::Lost,
            None => {
                self.acts.waiting = Waiting::CancelledBind;
                Flow::Go
            }
        }
    }

    /// After the library wrote `cancel` while the runner waits for a `bind_reply`, drops the waiting `add_origin` of
    /// the way `sign_in` and writes no `bind_reply` (ruling CP-193 item 3).
    pub(crate) fn drop_cancelled_bind(&mut self) {
        if matches!(self.acts.waiting, Waiting::Bind) && self.slot.cancelled() {
            self.acts
                .adds
                .retain(|p| !matches!(p.then, Then::BindReply));
            self.acts.waiting = Waiting::CancelledBind;
        }
    }

    fn queue_add(&mut self, origin: WebOrigin, way: Way, then: Then) -> bool {
        let Some(id) = owner::request_id() else {
            return false;
        };
        self.acts.adds.push(Pending {
            id,
            origin,
            way,
            then,
            refused: 0,
        });
        true
    }

    /// Sends again each `add_origin` that waits, once `ADD_AGAIN` has passed since the last try. A link error, a reply
    /// with another id, an error reply or a reply of another form is sent again without a count; at the
    /// `ADD_REFUSED_MAX`th reply that `decode_reply` refuses the body is given up: for the way `new_origin` the run
    /// goes on, for `sign_in` and `account_link`, where the runner waits for the reply, the run is lost.
    pub(crate) fn retry_adds(&mut self) -> Flow {
        self.drop_cancelled_bind();
        match self.acts.next_try {
            Some(at) if Instant::now() >= at => self.try_adds(),
            _ => Flow::Go,
        }
    }

    fn try_adds(&mut self) -> Flow {
        let spec = &self.slot.spec;
        let mut left = Vec::new();
        let mut flow = Flow::Go;
        for mut p in std::mem::take(&mut self.acts.adds) {
            let call = CoordinatorCall::AddOrigin(AddOrigin {
                workflow: spec.workflow.clone(),
                agent: spec.agent.clone(),
                run: spec.run.clone(),
                origin: p.origin.clone(),
                way: p.way,
            });
            match owner::acknowledged(self.inner, &p.id, call) {
                Ack::Acknowledged => {}
                Ack::Unanswered => {
                    left.push(p);
                    continue;
                }
                Ack::Refused => {
                    p.refused += 1;
                    if p.refused < ADD_REFUSED_MAX {
                        left.push(p);
                    } else if p.way != Way::NewOrigin {
                        flow = Flow::Lost;
                    }
                    continue;
                }
            }
            self.acts.bound.insert(p.origin);
            match p.then {
                Then::BindReply => {
                    self.acts.waiting = Waiting::Nothing;
                    if let Flow::Lost = self.bind_reply(YesNo::Yes) {
                        flow = Flow::Lost;
                    }
                }
                Then::HandbackReply(result) => {
                    self.acts.waiting = Waiting::Nothing;
                    if !self
                        .slot
                        .write(&ToRunner::HandbackReply(HandbackReply { result }))
                    {
                        flow = Flow::Lost;
                    }
                }
                Then::Nothing => {}
            }
        }
        self.acts.next_try = (!left.is_empty()).then(|| Instant::now() + ADD_AGAIN);
        self.acts.adds = left;
        flow
    }
}
