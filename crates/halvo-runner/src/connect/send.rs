//! The request maker of `mail_send` (A11.2), linked in a build with the feature `loopback` alone, which only the
//! runner's own tests turn on, so that no build that a person can use sends a mail (the coordinator's ruling of
//! 2026-11-21T20:16:11Z, log line 11974, point (2)). Each draft with a send handle that the run's draft step kept is
//! one paused request of the port `Paused`, and it leaves by `Runner::release` alone (A5.2, A6.3).

use std::rc::Rc;

use halvo_connect::{CallError, DraftToSend, Effect, Graph, Mail, Token};
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::{FieldName, Source};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{ActionClass, Bound, BoundField, Cause, ExecutableStep};

use super::{body_of, slot};
use crate::model::write::Body;
use crate::release::{Left, MakerFacts, Paused, Replied, Reply};
use crate::run::{Failure, Runner};

/// The send of one draft: the adapter and the token of the send step, the draft's handle, the reply that it sends,
/// its subject as the PATCH answered it, its body as the draft step wrote it, and the facts of the run's own list.
struct SendReply {
    graph: Rc<Graph>,
    token: Rc<Token>,
    draft: DraftToSend,
    reply: Reply,
    subject: String,
    body: Body,
    facts: MakerFacts,
}

/// The answer of a send that left.
struct Answer(Result<(), CallError>);

fn field(name: FieldName, from: Source, value: &str) -> BoundField {
    BoundField {
        name,
        from,
        origin: None,
        value: OwnValue::new(value),
    }
}

impl Paused for SendReply {
    fn class(&self) -> ActionClass {
        ActionClass::Send
    }

    /// `to`, the sender of the message as the run's own `mail_list` gave it (A5.1, CP-146 (1)); `subject`, the
    /// draft's as the PATCH answered it; and `body`, the body that the draft step wrote, with the source `model` for a
    /// writer's text and `const` for the const text. The object holds no slot, so the
    /// slots of a later load change nothing.
    fn bound(&mut self, _slots: &[Slot]) -> Result<Bound, Cause> {
        Bound::new(
            ActionClass::Send,
            vec![
                field(FieldName::To, Source::Extract, &self.reply.sender),
                field(FieldName::Subject, Source::Extract, &self.subject),
                field(FieldName::Body, self.body.source, &self.body.text),
            ],
        )
        .map_err(|_| Cause::StepFailed)
    }

    fn facts(&self) -> MakerFacts {
        self.facts.clone()
    }

    /// POST send of the draft, once: `mail_send` takes the draft's handle.
    fn release(self: Box<Self>) -> Box<dyn Left> {
        let sent = self.graph.mail_send(&self.token, self.draft).map(|_| ());
        Box::new(Answer(sent))
    }

    fn renew(self: Box<Self>, _slots: &[Slot]) -> Result<Box<dyn Paused>, Cause> {
        Ok(self)
    }

    fn reply(&self) -> Option<&Reply> {
        Some(&self.reply)
    }
}

impl Left for Answer {
    /// Made for the send's success status alone; not made for a case of A11.2 that `effect()` of halvo-connect
    /// gives; no reply when no whole answer came; any other answer is read as neither (CP-159 part 5).
    fn reply(self: Box<Self>) -> Replied {
        match self.0 {
            Ok(()) => Replied::Made,
            Err(error) => match error.effect() {
                Effect::NotMade(case) => Replied::NotMade(case),
                Effect::Unknown if error == CallError::NoAnswer => Replied::NoReply,
                Effect::Unknown => Replied::Answered,
            },
        }
    }
}

impl Runner {
    /// `mail_send` of each draft with a send handle that the draft step kept, in the order of the messages: one
    /// paused request each, handed to `Runner::release` together, under the adapter for the first origin of the
    /// load's bindings and the account of its slot `account_address`. A step with no such draft sends nothing.
    pub(super) fn send_replies(
        &mut self,
        step: &ExecutableStep,
        token: Token,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let drafts = self
            .lists
            .drafts
            .take()
            .ok_or(Failure::Step(Cause::Adapter))?;
        let account = slot(loaded, "account_address").ok_or(Failure::Step(Cause::Adapter))?;
        let origin = loaded
            .bindings
            .as_ref()
            .and_then(|b| b.origins.first().cloned())
            .ok_or(Failure::Step(Cause::Adapter))?;
        let graph = Rc::new(self.graph(loaded)?);
        let token = Rc::new(token);
        let facts = MakerFacts {
            request_origin: Some(origin),
            ids: self
                .lists
                .senders
                .iter()
                .map(|(id, _)| id.clone())
                .collect(),
            senders: self.lists.senders.iter().map(|(_, s)| s.clone()).collect(),
        };
        let mut requests: Vec<Box<dyn Paused>> = Vec::new();
        for draft in drafts.into_sends() {
            let message = draft.draft().message().to_owned();
            let Some((_, sender)) = self.lists.senders.iter().find(|(id, _)| *id == message) else {
                return Err(Failure::Step(Cause::Adapter));
            };
            let body = body_of(&self.lists.bodies, &message);
            let reply = Reply {
                step: step.n,
                draft: draft.draft().id().to_owned(),
                sender: sender.clone(),
                account: account.clone(),
                message,
            };
            requests.push(Box::new(SendReply {
                graph: Rc::clone(&graph),
                token: Rc::clone(&token),
                subject: draft.draft().subject().to_owned(),
                body,
                draft,
                reply,
                facts: facts.clone(),
            }));
        }
        if requests.is_empty() {
            return Ok(());
        }
        self.release(requests).map_err(Failure::Stop)
    }
}
