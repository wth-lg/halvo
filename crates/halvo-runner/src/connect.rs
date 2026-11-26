//! The runner's link to the connect adapters of halvo-connect (A11.2): the reads of mail and calendar and the drafts
//! of replies, through one `Graph` for the origin of the agent's bindings; the rows of answered messages (A7.4
//! `agent_reply`), read for each page of the mail list by `load_replies` and written by `reply_row`; and the keys
//! of the run's list for the record of `finish`. The send of a reply (`send`) and the delete of an event (`delete`)
//! are linked in a build with the feature `loopback` alone, which only the runner's own tests turn on; in every other
//! build their steps fail with the cause `adapter`.

#[cfg(feature = "loopback")]
mod delete;
#[cfg(feature = "loopback")]
mod send;

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::time::Duration;

use halvo_connect::{
    reply_senders, Answered, Calendar, CallError, DraftStop, Drafting, EventCounts, Graph,
    ListStop, Mail, MailCounts, Message, Messages, ReplyRule, Route, RunDay, Token,
};
use halvo_schema::executable::body::{LoadReplies, LoadReply, LoadReplyBody, PutReplyRow, Stored};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::names::{ApiOp, LogEvent, Source};
use halvo_schema::executable::text::{OwnValue, SecretText, V1};
use halvo_schema::executable::{
    AgentReply, AgentRun, Cause, EffectId, ExecutableStep, Outcome, ProviderId, ReplyState,
    RunnerCall, RunnerReply, WebOrigin,
};
use halvo_schema::{RequestId, RunnerRequest};

use crate::clock::{now, random16};
use crate::link::Broken;
use crate::model::write::{Body, Writer};
use crate::run::{cause_of, Failure, Runner, Stop};

/// The one time limit of a request of the adapter, from its start to the end of its answer's body.
const CALL_LIMIT: Duration = Duration::from_secs(30);

/// The const text of a reply (A11.4): the fallback line without its line end.
pub(crate) fn const_body() -> &'static str {
    FALLBACK.strip_suffix('\n').unwrap_or(FALLBACK)
}

/// The body of a reply that no writer model wrote (A11.4, crate::model::write): the one line of the fallback file,
/// without its line end, with the source `const`.
const FALLBACK: &str = include_str!("../playbooks/reply-fallback.txt");

/// The draft of an earlier run that waits for the message of `row`, the agent's own newest row of it: the draft that a
/// `drafted` row holds, which the list gives to the PATCH (`Answered::Draft`) and for which the writer asks no second
/// body (crate::model::write).
fn waiting_draft(row: &AgentReply) -> Option<&str> {
    if row.state != ReplyState::Drafted {
        return None;
    }
    row.draft.as_ref().map(|draft| draft.as_str())
}

/// The message and the draft of `row` where its draft waits (`waiting_draft`), for the read of its kept text
/// (r3 U7.9).
fn waiting(row: &AgentReply) -> Option<(ProviderId, ProviderId)> {
    waiting_draft(row)?;
    Some((row.message.clone(), row.draft.clone()?))
}

/// The text that the store keeps beside the first `drafted` row of the draft of the message `id` (r3 U7.9): the body
/// that the draft step wrote for it where its source is `model`, the writer model's text as cut; none for the const
/// text, which is the build's own.
fn kept_text<'b>(bodies: &'b HashMap<String, Body>, id: &str) -> Option<&'b str> {
    bodies
        .get(id)
        .filter(|body| body.source == Source::Model)
        .map(|body| body.text.as_str())
}

/// How many times a row of a reply is sent when the store answers it with a fault (`store` or `internal`), each
/// time with the same request id, as a row of an effect is sent (A7.5, CP-131 (e)).
const ROW_SENDS: u32 = 3;

/// Why a message of the list was left alone by the rows of answered messages, for the counts of the run's record.
#[derive(Clone, Copy)]
enum Alone {
    /// The agent's own newest row is `begun`, `committed` or `unknown`: in no count.
    Own,
    /// Another agent holds `begun`, `committed` or `unknown`: `other_task`.
    OtherTask,
    /// The agent's own newest row, or another agent's, is `withheld`: `held_earlier`.
    HeldEarlier,
}

/// What the run's list step and draft step gave: the kept messages until the draft step takes them, the agent's
/// own newest row of each message read, and the facts that the record of `finish` carries (A7.4).
#[derive(Default)]
pub(crate) struct Lists {
    messages: Option<Messages>,
    rows: HashMap<String, AgentReply>,
    alone: HashMap<String, Alone>,
    mail: Option<MailCounts>,
    events: Option<EventCounts>,
    answered: Option<(u32, u32)>,
    held_recipient: Option<u32>,
    /// The drafts with a send handle that the draft step kept, until the send step takes them.
    #[cfg(feature = "loopback")]
    drafts: Option<halvo_connect::Drafts>,
    /// The id and the sender of each message that the list kept, in its order, as `mail_list` gave them.
    #[cfg(feature = "loopback")]
    senders: Vec<(String, String)>,
    /// The body that the draft step wrote for each message, by the message's id, until the send step takes them.
    #[cfg(feature = "loopback")]
    pub(crate) bodies: HashMap<String, Body>,
    /// The events that the list step kept, until the delete step takes them.
    #[cfg(feature = "loopback")]
    calendar: Option<halvo_connect::Events>,
}

impl Lists {
    /// Sets the keys of the run's list of `record` from the answers that the run's steps had: each as the adapter
    /// answered it, `other_task` and `held_earlier` as the runner counted them, and null where no answer came or
    /// where the run's kind of list gives no such key.
    pub(crate) fn fill(&self, record: &mut AgentRun) {
        if let Some(c) = self.mail {
            record.before_start = Some(c.before_start);
            record.cut = Some(c.cut);
            record.no_key = Some(c.no_key);
            record.not_to_account = Some(c.not_to_account);
            record.incomplete = Some(c.incomplete);
            record.reply_to_other = Some(c.reply_to_other);
            record.more_pages = Some(c.more_pages);
            record.earlier_page = Some(c.earlier_page);
            record.held_recipient = self.held_recipient;
            record.other_task = self.answered.map(|(other_task, _)| other_task);
            record.held_earlier = self.answered.map(|(_, held_earlier)| held_earlier);
        }
        if let Some(c) = self.events {
            record.cut = Some(c.cut);
            record.no_key = Some(c.no_key);
            record.organized = Some(c.organized);
            record.incomplete = Some(c.incomplete);
            record.more_pages = Some(c.more_pages);
            record.earlier_page = Some(c.earlier_page);
        }
    }
}

/// The route of the adapter's client: the loopback of A11.2 for an `http` origin in a build with the feature
/// `loopback`, which only the runner's own tests turn on; else the system's resolver, which refuses an `http`
/// origin.
fn route(origin: &WebOrigin) -> Route {
    #[cfg(feature = "loopback")]
    {
        if origin.as_str().starts_with("http://") {
            return Route::Loopback;
        }
    }
    let _ = origin;
    Route::System
}

/// The body that the draft step wrote for the message `id`: its entry of `bodies`, else the const text.
pub(crate) fn body_of(bodies: &HashMap<String, Body>, id: &str) -> Body {
    bodies.get(id).cloned().unwrap_or_else(Body::constant)
}

/// The text of the slot `name` of the load's bindings, its pieces joined with no trim.
pub(crate) fn slot(loaded: &LoadReply, name: &str) -> Option<String> {
    let bindings = loaded.bindings.as_ref()?;
    let slot = bindings.slots.iter().find(|s| s.name.as_str() == name)?;
    Some(slot.value.joined())
}

impl Runner {
    /// Carries out the operation `op` with the adapter's token `secret`. The mail list, the drafts and the calendar
    /// list are linked; a send and a delete are linked in a build with the feature `loopback` alone, and fail with
    /// the cause `adapter` in every other build.
    pub(crate) fn connect(
        &mut self,
        step: &ExecutableStep,
        op: ApiOp,
        secret: SecretText,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let token = Token::new(secret.expose().to_owned());
        drop(secret);
        let token = token.map_err(|_| Failure::Step(Cause::Adapter))?;
        match op {
            ApiOp::MailList => self.mail_list(step, &token, loaded),
            ApiOp::MailDraftReply => self.draft_replies(step, &token, loaded),
            ApiOp::CalendarList => self.calendar_list(step, &token, loaded),
            #[cfg(feature = "loopback")]
            ApiOp::MailSend => self.send_replies(step, token, loaded),
            #[cfg(not(feature = "loopback"))]
            ApiOp::MailSend => Err(Failure::Step(Cause::Adapter)),
            #[cfg(feature = "loopback")]
            ApiOp::CalendarDelete => self.delete_events(step, token, loaded),
            #[cfg(not(feature = "loopback"))]
            ApiOp::CalendarDelete => Err(Failure::Step(Cause::Adapter)),
        }
    }

    /// The adapter for the first origin of the load's bindings.
    fn graph(&self, loaded: &LoadReply) -> Result<Graph, Failure> {
        let origin = loaded
            .bindings
            .as_ref()
            .and_then(|b| b.origins.first())
            .ok_or(Failure::Step(Cause::Adapter))?;
        Graph::new(origin, route(origin), CALL_LIMIT).map_err(|_| Failure::Step(Cause::Adapter))
    }

    /// The end after an answer 401 (A8.6, CP-130 (b), CP-135 (2)): the row `step_failed` of the step with the cause
    /// `unauthorized`, no handback, and the run ends `failed` with the cause `adapter`.
    fn unauthorized(&mut self, step: &ExecutableStep) -> Failure {
        match self.log(
            LogEvent::StepFailed,
            Some(step.n),
            Some(Cause::Unauthorized),
        ) {
            Err(stop) => Failure::Stop(stop),
            Ok(()) => Failure::Stop(Stop::End(Outcome::Failed, Some(Cause::Adapter))),
        }
    }

    /// The end of a list step that makes no request: the row `stopped` of the step with `cause`, and the run ends
    /// `failed`.
    fn stopped(&mut self, step: &ExecutableStep, cause: Cause) -> Failure {
        match self.log(LogEvent::Stopped, Some(step.n), Some(cause)) {
            Err(stop) => Failure::Stop(stop),
            Ok(()) => Failure::Stop(Stop::End(Outcome::Failed, None)),
        }
    }

    /// The failure of a step whose call answered `error`.
    fn call_failed(&mut self, step: &ExecutableStep, error: CallError) -> Failure {
        match error {
            CallError::Unauthorized => self.unauthorized(step),
            _ => Failure::Step(Cause::Adapter),
        }
    }

    /// `mail_list` under the reply rule of the load: the account as the slot `account_address` holds it, the senders
    /// that the reader of `reply_senders` read, and the instant of the load's `start`. A load with no start row, or
    /// whose slot `reply_senders` the reader refuses, makes no request. Before the rule selects from a page, the rows
    /// of the page's messages are read by `load_replies`.
    fn mail_list(
        &mut self,
        step: &ExecutableStep,
        token: &Token,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let Some(start) = loaded.start.as_ref().map(|s| s.at) else {
            return Err(self.stopped(step, Cause::NoStart));
        };
        let Some(Ok(senders)) = slot(loaded, "reply_senders").map(|text| reply_senders(&text))
        else {
            return Err(self.stopped(step, Cause::SendersUnread));
        };
        let account = slot(loaded, "account_address").ok_or(Failure::Step(Cause::Adapter))?;
        let rule = ReplyRule::new(account, senders, start);
        let graph = self.graph(loaded)?;
        let listed = graph.mail_list_answered(token, &rule, |ids| self.answered(ids));
        let messages = match listed {
            Ok(messages) => messages,
            Err(ListStop::Answered(stop)) => return Err(Failure::Stop(stop)),
            Err(ListStop::Call(error)) => return Err(self.call_failed(step, error)),
        };
        let (mut other_task, mut held_earlier) = (0u32, 0u32);
        for id in messages.left() {
            match self.lists.alone.get(id) {
                Some(Alone::OtherTask) => other_task += 1,
                Some(Alone::HeldEarlier) => held_earlier += 1,
                Some(Alone::Own) | None => {}
            }
        }
        self.lists.mail = Some(messages.counts());
        self.lists.answered = Some((other_task, held_earlier));
        self.lists.messages = Some(messages);
        Ok(())
    }

    /// Reads the rows of the messages `ids` of one page by `load_replies` and says what they make of each (A7.4,
    /// CP-160 (3) (d)): the agent's own newest row decides first, `begun`, `committed`, `unknown` and `withheld`
    /// keeping the message out; then an id in `blocked` or `blocked_withheld` keeps it out; then an own `drafted`
    /// row gives its draft to the PATCH. An id that no row can name is left alone.
    fn answered(&mut self, ids: &[String]) -> Result<HashMap<String, Answered>, Stop> {
        let mut words = HashMap::new();
        let mut asked = Vec::with_capacity(ids.len());
        for id in ids {
            match ProviderId::parse(id) {
                Ok(id) => asked.push(id),
                Err(_) => {
                    words.insert(id.clone(), Answered::Leave);
                }
            }
        }
        if asked.is_empty() {
            return Ok(words);
        }
        let call = RunnerCall::LoadReplies(LoadReplies {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            message_ids: asked,
        });
        let reply = match self.store(call)? {
            Ok(RunnerReply::Replies(reply)) => reply,
            Ok(_) => return Err(Broken::Frame("not_the_answer").into()),
            Err(error) => return Err(Stop::End(Outcome::Failed, Some(cause_of(error)))),
        };
        for row in reply.replies {
            self.lists.rows.insert(row.message.as_str().to_owned(), row);
        }
        let listed = |list: &[ProviderId], id: &str| list.iter().any(|l| l.as_str() == id);
        for id in ids {
            let own = self.lists.rows.get(id);
            let alone = match own.map(|row| row.state) {
                Some(ReplyState::Begun | ReplyState::Committed | ReplyState::Unknown) => {
                    Some(Alone::Own)
                }
                Some(ReplyState::Withheld) => Some(Alone::HeldEarlier),
                _ if listed(&reply.blocked, id) => Some(Alone::OtherTask),
                _ if listed(&reply.blocked_withheld, id) => Some(Alone::HeldEarlier),
                _ => None,
            };
            if let Some(alone) = alone {
                self.lists.alone.insert(id.clone(), alone);
                words.insert(id.clone(), Answered::Leave);
            } else if let Some(draft) = own.and_then(waiting_draft) {
                words.insert(id.clone(), Answered::Draft(draft.to_owned()));
            }
        }
        Ok(words)
    }

    /// `mail_draft_reply_each` of the messages that the list kept, each with its body, all of them before the first
    /// draft: for a message whose draft of an earlier run waits, the text that the store keeps for that draft
    /// (`load_reply_body`, r3 U7.9) with the source `model`, else the const text, and no request to a model; for
    /// every other message the body that the writer gives it (crate::model::write): the writer model's text cut at
    /// `reply_body`, or the const text. The rows of each draft are stored as the adapter hands it over, before it
    /// drafts the next message, so that a run cut in this step leaves at most one draft with no row: `drafted` for a
    /// new draft, with the writer model's text beside it where the body is one (r3 U7.9), then `withheld` for one
    /// that the adapter held back, and `lost` for a draft of an earlier run that the adapter found lost; a draft of an
    /// earlier run that passed gets no row. A failed call leaves the row of the draft whose PATCH gave no answer that
    /// passed.
    fn draft_replies(
        &mut self,
        step: &ExecutableStep,
        token: &Token,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let messages = self
            .lists
            .messages
            .take()
            .ok_or(Failure::Step(Cause::Adapter))?;
        let graph = self.graph(loaded)?;
        #[cfg(feature = "loopback")]
        {
            self.lists.senders = messages
                .iter()
                .map(|m| (m.id().to_owned(), m.sender().to_owned()))
                .collect();
        }
        let mut writer = Writer::default();
        let mut bodies = HashMap::new();
        for message in messages.iter() {
            let body = match self.lists.rows.get(message.id()).and_then(waiting) {
                Some((id, draft)) => self.kept_body(id, draft)?.unwrap_or_else(Body::constant),
                None => self.reply_body(&mut writer, loaded, message)?,
            };
            bodies.insert(message.id().to_owned(), body);
        }
        let text_of = |message: &Message| body_of(&bodies, message.id()).text;
        let rows_of = |made: Drafting<'_>| self.rows_of(made, &bodies);
        match graph.mail_draft_reply_each(token, messages, text_of, rows_of) {
            Ok(drafted) => {
                self.lists.held_recipient = Some(drafted.held_recipient());
                #[cfg(feature = "loopback")]
                {
                    self.lists.drafts = Some(drafted.into_drafts());
                    self.lists.bodies = bodies;
                }
                Ok(())
            }
            Err(DraftStop::Each(stop)) => Err(Failure::Stop(stop)),
            Err(DraftStop::Call(failure)) => {
                if let Some(unfinished) = failure.unfinished() {
                    let kept = kept_text(&bodies, unfinished.message());
                    self.new_draft(unfinished.message(), unfinished.draft(), kept)?;
                }
                Err(self.call_failed(step, failure.error()))
            }
        }
    }

    /// The rows of one draft that `mail_draft_reply_each` handed over, with the kept text of its body (`kept_text`)
    /// beside a new `drafted` row, but for a draft that the adapter held back (r3 U7.9).
    fn rows_of(&mut self, made: Drafting<'_>, bodies: &HashMap<String, Body>) -> Result<(), Stop> {
        match made {
            Drafting::Kept(draft) => {
                let kept = kept_text(bodies, draft.message());
                self.new_draft(draft.message(), draft.id(), kept)
            }
            Drafting::Unsent(unsent) => {
                let held = unsent.why().is_held();
                let kept = kept_text(bodies, unsent.message()).filter(|_| !held);
                self.new_draft(unsent.message(), unsent.draft(), kept)?;
                if held {
                    let draft = Some(unsent.draft());
                    self.reply_row(unsent.message(), ReplyState::Withheld, draft, None)?;
                }
                Ok(())
            }
            Drafting::Lost(message) => self.reply_row(message, ReplyState::Lost, None, None),
        }
    }

    /// The row `drafted` of the draft `draft` of `message`, with `kept`, the writer model's text, for the store to keep
    /// beside it (r3 U7.9), unless the agent's own newest row of the message is already `drafted` with that draft,
    /// which the PATCH reused.
    fn new_draft(&mut self, message: &str, draft: &str, kept: Option<&str>) -> Result<(), Stop> {
        let reused = self.lists.rows.get(message).is_some_and(|row| {
            row.state == ReplyState::Drafted
                && row.draft.as_ref().is_some_and(|d| d.as_str() == draft)
        });
        if reused {
            return Ok(());
        }
        self.row(message, ReplyState::Drafted, Some(draft), None, kept)
    }

    /// The kept text of the draft `draft` of the message `message`, a draft of an earlier run that waits (r3 U7.9):
    /// the writer model's text that the store keeps beside that draft's first `drafted` row, asked by
    /// `load_reply_body`, with the source `model`; none where the store hands none out. A refusal of the store ends
    /// the run as one of `load_replies` does.
    fn kept_body(&mut self, message: ProviderId, draft: ProviderId) -> Result<Option<Body>, Stop> {
        let call = RunnerCall::LoadReplyBody(LoadReplyBody {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            message,
            draft,
        });
        let reply = match self.store(call)? {
            Ok(RunnerReply::Kept(reply)) => reply,
            Ok(_) => return Err(Broken::Frame("not_the_answer").into()),
            Err(error) => return Err(Stop::End(Outcome::Failed, Some(cause_of(error)))),
        };
        Ok(reply.kept.map(|text| Body {
            text: text.joined(),
            source: Source::Model,
        }))
    }

    /// Stores one row of the reply to `message` by `reply_row`, numbered after the agent's own newest row of it, with
    /// `effect`, the effect of the send of the reply, and sends it as a row of an effect is sent: at most three
    /// times, on `store` and `internal` alone, after which the run ends `failed` with the cause `step_failed`; any
    /// other refusal ends it with its cause. It returns once the store has answered the row with a stored count of 1.
    pub(crate) fn reply_row(
        &mut self,
        message: &str,
        state: ReplyState,
        draft: Option<&str>,
        effect: Option<&EffectId>,
    ) -> Result<(), Stop> {
        self.row(message, state, draft, effect, None)
    }

    /// `reply_row`, with `kept`, a writer model's text, under the body's key `body` beside a `drafted` row that names
    /// no effect (r3 U7.9).
    fn row(
        &mut self,
        message: &str,
        state: ReplyState,
        draft: Option<&str>,
        effect: Option<&EffectId>,
        kept: Option<&str>,
    ) -> Result<(), Stop> {
        let failed = |cause| Stop::End(Outcome::Failed, Some(cause));
        let seq = self
            .lists
            .rows
            .get(message)
            .map_or(1, |row| row.seq.get().saturating_add(1));
        let message_id = ProviderId::parse(message).map_err(|_| failed(Cause::StepFailed))?;
        let draft = draft
            .map(ProviderId::parse)
            .transpose()
            .map_err(|_| failed(Cause::StepFailed))?;
        let at = now();
        let record = AgentReply {
            v: V1,
            workflow: self.args.workflow.clone(),
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            seq: NonZeroU32::new(seq).unwrap_or(NonZeroU32::MAX),
            day: day_of(at, self.args.utc_offset_s),
            at,
            message: message_id,
            draft,
            effect: effect.cloned(),
            state,
        };
        let call = RunnerCall::ReplyRow(PutReplyRow {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            record: record.clone(),
            body: kept.map(OwnValue::new),
        });
        let request = RunnerRequest::new(self.role(), RequestId::from_random(random16()), call);
        let mut sends = 0;
        loop {
            sends += 1;
            match self.send_store(&request)? {
                Ok(RunnerReply::Stored(Stored { stored: 1 })) => break,
                Ok(_) => return Err(failed(Cause::StepFailed)),
                Err(error) => {
                    if !matches!(error.as_str(), "store" | "internal") {
                        return Err(failed(cause_of(error)));
                    }
                    if sends >= ROW_SENDS {
                        return Err(failed(Cause::StepFailed));
                    }
                }
            }
        }
        self.lists.rows.insert(message.to_owned(), record);
        Ok(())
    }

    /// `calendar_list` for the run's day, which `day_of` gives at the run's offset. In a build with the feature
    /// `loopback` the kept events wait for the delete step.
    fn calendar_list(
        &mut self,
        step: &ExecutableStep,
        token: &Token,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let graph = self.graph(loaded)?;
        let day = day_of(now(), self.args.utc_offset_s);
        let day = RunDay::from_unix_seconds(u64::from(day) * 86_400);
        match graph.calendar_list(token, day) {
            Ok(events) => {
                self.lists.events = Some(events.counts());
                #[cfg(feature = "loopback")]
                {
                    self.lists.calendar = Some(events);
                }
                Ok(())
            }
            Err(error) => Err(self.call_failed(step, error)),
        }
    }
}
