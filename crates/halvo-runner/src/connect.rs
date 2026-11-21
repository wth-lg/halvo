//! The runner's link to the connect adapters of halvo-connect (A11.2): the reads of mail and calendar and the drafts
//! of replies, through one `Graph` for the origin of the agent's bindings; the rows of answered messages (A7.4
//! `agent_reply`), read for each page of the mail list by `load_replies` and written by `reply_row`; and the keys
//! of the run's list for the record of `finish`. A send and a delete are not linked: their steps fail with the
//! cause `adapter`.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::time::Duration;

use halvo_connect::{
    reply_senders, Answered, Calendar, CallError, Drafted, EventCounts, Graph, ListStop, Mail,
    MailCounts, Messages, ReplyRule, Route, RunDay, Token,
};
use halvo_schema::executable::body::{LoadReplies, LoadReply, PutReplyRow, Stored};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::names::{ApiOp, LogEvent};
use halvo_schema::executable::text::{SecretText, V1};
use halvo_schema::executable::{
    AgentReply, AgentRun, Cause, ExecutableStep, Outcome, ProviderId, ReplyState, RunnerCall,
    RunnerReply, WebOrigin,
};
use halvo_schema::{RequestId, RunnerRequest};

use crate::clock::{now, random16};
use crate::link::Broken;
use crate::run::{cause_of, Failure, Runner, Stop};

/// The one time limit of a request of the adapter, from its start to the end of its answer's body.
const CALL_LIMIT: Duration = Duration::from_secs(30);

/// The body of a reply when no model is given (A11.4): the one line of the fallback file, without its line end.
const FALLBACK: &str = include_str!("../playbooks/reply-fallback.txt");

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

/// The text of the slot `name` of the load's bindings, its pieces joined with no trim.
fn slot(loaded: &LoadReply, name: &str) -> Option<String> {
    let bindings = loaded.bindings.as_ref()?;
    let slot = bindings.slots.iter().find(|s| s.name.as_str() == name)?;
    Some(slot.value.joined())
}

impl Runner {
    /// Carries out the operation `op` with the adapter's token `secret`. The mail list, the drafts and the calendar
    /// list are linked; a send and a delete fail with the cause `adapter`.
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
            ApiOp::MailSend | ApiOp::CalendarDelete => Err(Failure::Step(Cause::Adapter)),
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
            } else if let Some(draft) = own
                .filter(|row| row.state == ReplyState::Drafted)
                .and_then(|row| row.draft.as_ref())
            {
                words.insert(id.clone(), Answered::Draft(draft.as_str().to_owned()));
            }
        }
        Ok(words)
    }

    /// `mail_draft_reply` of the messages that the list kept, with the fallback body, and a row of each: `drafted`
    /// for a new draft, then `withheld` for one that the adapter held back, and `lost` for a draft of an earlier run
    /// that the adapter found lost; a draft of an earlier run that passed gets no row.
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
        let body = FALLBACK.strip_suffix('\n').unwrap_or(FALLBACK);
        match graph.mail_draft_reply(token, messages, body) {
            Ok(drafted) => {
                self.store_drafted(&drafted)?;
                self.lists.held_recipient = Some(drafted.held_recipient());
                Ok(())
            }
            Err(failure) => {
                for draft in failure.finished() {
                    self.new_draft(draft.message(), draft.id())?;
                }
                for unsent in failure.unsent() {
                    self.new_draft(unsent.message(), unsent.draft())?;
                    if unsent.why().is_held() {
                        self.reply_row(
                            unsent.message(),
                            ReplyState::Withheld,
                            Some(unsent.draft()),
                        )?;
                    }
                }
                if let Some(unfinished) = failure.unfinished() {
                    self.new_draft(unfinished.message(), unfinished.draft())?;
                }
                for message in failure.lost() {
                    self.reply_row(message, ReplyState::Lost, None)?;
                }
                Err(self.call_failed(step, failure.error()))
            }
        }
    }

    /// The rows of an answer of `mail_draft_reply`.
    fn store_drafted(&mut self, drafted: &Drafted) -> Result<(), Stop> {
        for draft in drafted.drafts().iter() {
            self.new_draft(draft.message(), draft.id())?;
        }
        for unsent in drafted.unsent() {
            self.new_draft(unsent.message(), unsent.draft())?;
            if unsent.why().is_held() {
                self.reply_row(unsent.message(), ReplyState::Withheld, Some(unsent.draft()))?;
            }
        }
        for message in drafted.lost() {
            self.reply_row(message, ReplyState::Lost, None)?;
        }
        Ok(())
    }

    /// The row `drafted` of the draft `draft` of `message`, unless the agent's own newest row of the message is
    /// already `drafted` with that draft, which the PATCH reused.
    fn new_draft(&mut self, message: &str, draft: &str) -> Result<(), Stop> {
        let reused = self.lists.rows.get(message).is_some_and(|row| {
            row.state == ReplyState::Drafted
                && row.draft.as_ref().is_some_and(|d| d.as_str() == draft)
        });
        if reused {
            return Ok(());
        }
        self.reply_row(message, ReplyState::Drafted, Some(draft))
    }

    /// Stores one row of the reply to `message` by `reply_row`, numbered after the agent's own newest row of it, and
    /// sends it as a row of an effect is sent: at most three times, on `store` and `internal` alone, after which the
    /// run ends `failed` with the cause `step_failed`; any other refusal ends it with its cause.
    fn reply_row(
        &mut self,
        message: &str,
        state: ReplyState,
        draft: Option<&str>,
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
            effect: None,
            state,
        };
        let call = RunnerCall::ReplyRow(PutReplyRow {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            record: record.clone(),
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

    /// `calendar_list` for the run's day, which `day_of` gives at the run's offset.
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
                Ok(())
            }
            Err(error) => Err(self.call_failed(step, error)),
        }
    }
}
