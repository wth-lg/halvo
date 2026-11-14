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
use crate::owner;
use crate::supervisor::{Flow, Running};
use crate::sys;

/// How long the library waits before it sends an `add_origin` that was not acknowledged again.
pub(crate) const ADD_AGAIN: Duration = Duration::from_millis(500);

/// What the runner waits for while the library holds its reply.
#[derive(Default)]
pub(crate) enum Waiting {
    #[default]
    Nothing,
    Handback(HandbackRequest),
    Bind,
    Link,
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
}

/// The state of a run's acts: the origins that the bindings hold, what the runner waits for, and the
/// `add_origin` bodies that wait for their acknowledgement.
#[derive(Default)]
pub(crate) struct Acts {
    pub(crate) bound: HashSet<WebOrigin>,
    waiting: Waiting,
    adds: Vec<Pending>,
    next_try: Option<Instant>,
}

impl Acts {
    /// Whether the runner waits for a reply that the library holds.
    pub(crate) fn waiting(&self) -> bool {
        !matches!(self.waiting, Waiting::Nothing)
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
        let link = request.reason == HandbackReason::AccountLink
            && result.outcome == HandbackOutcome::Done;
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
            }
        }
        Ok(())
    }

    pub(crate) fn bind(&mut self, origin: WebOrigin) -> Flow {
        if self.acts.bound.contains(&origin) {
            return self.bind_reply(YesNo::Yes);
        }
        match self
            .inner
            .parts
            .human
            .allow_origin(&self.slot.spec.run, &origin)
        {
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
        if self.slot.write(&ToRunner::BindReply(BindReply { bound })) {
            Flow::Go
        } else {
            Flow::Lost
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
        });
        true
    }

    /// Sends again each `add_origin` that waits, once `ADD_AGAIN` has passed since the last try.
    pub(crate) fn retry_adds(&mut self) -> Flow {
        match self.acts.next_try {
            Some(at) if Instant::now() >= at => self.try_adds(),
            _ => Flow::Go,
        }
    }

    fn try_adds(&mut self) -> Flow {
        let spec = &self.slot.spec;
        let mut left = Vec::new();
        let mut flow = Flow::Go;
        for p in std::mem::take(&mut self.acts.adds) {
            let call = CoordinatorCall::AddOrigin(AddOrigin {
                workflow: spec.workflow.clone(),
                agent: spec.agent.clone(),
                run: spec.run.clone(),
                origin: p.origin.clone(),
                way: p.way,
            });
            if !owner::acknowledged(self.inner, &p.id, call) {
                left.push(p);
                continue;
            }
            self.acts.bound.insert(p.origin);
            let reply = match p.then {
                Then::BindReply => Some(ToRunner::BindReply(BindReply { bound: YesNo::Yes })),
                Then::HandbackReply(result) => {
                    Some(ToRunner::HandbackReply(HandbackReply { result }))
                }
                Then::Nothing => None,
            };
            if let Some(reply) = reply {
                self.acts.waiting = Waiting::Nothing;
                if !self.slot.write(&reply) {
                    flow = Flow::Lost;
                }
            }
        }
        self.acts.next_try = (!left.is_empty()).then(|| Instant::now() + ADD_AGAIN);
        self.acts.adds = left;
        flow
    }
}
