use std::fmt;

use reqwest::Method;
use serde_json::{json, Value};

use crate::address::same_address;
use crate::answer::{self, Ids, FORM};
use crate::error::{AnswerFault, CallError};
use crate::graph::{segment, Graph};
use crate::token::Token;

const INBOX: &str = "/v1.0/me/mailFolders/inbox/messages";

/// The mail operations of A11.2, each of the class of its row: `mail_list` is `read`, `mail_draft_reply` is
/// `prepare` and `mail_send` is `send`. Each takes the token for its call alone.
pub trait Mail {
    /// GET of the messages of the inbox, once, then the reply rule: the unread messages that name the rule's account
    /// in `toRecipients` and whose sender is one of the rule's senders, in the order of the answer. Keeps
    /// `messages`. Refuses an answer that goes on in another page, that holds an id twice, or whose items lack a
    /// key of the face, with `Answer`.
    fn mail_list(&self, token: &Token, rule: &ReplyRule) -> Result<Messages, CallError>;

    /// For each message of `messages`, once and in order: createReply, then PATCH of the draft's body to `body` as
    /// plain text. Keeps `drafts`, each with its `to` and `subject` as the PATCH answered them. Takes `messages`
    /// whole, so that a list is drafted once. Refuses a createReply answer whose id is the id of any message of the
    /// list answer, kept by the rule or not, or of an earlier draft with `Answer(Repeated)`, before its PATCH, and
    /// a PATCH answer whose id is not the draft's with `Answer(Item)`, so that a handle exists only for a draft
    /// that createReply made for its own message. Stops at the first error; drafts made before it stay with the
    /// provider, unsent.
    fn mail_draft_reply(
        &self,
        token: &Token,
        messages: Messages,
        body: &str,
    ) -> Result<Drafts, CallError>;

    /// POST send of the one draft that `draft` holds: one effect. `Drafts::into_sends` gives one handle per draft
    /// and this call takes the handle, so a draft is sent at most once:
    ///
    /// ```compile_fail
    /// use halvo_connect::{DraftToSend, Graph, Mail, Token};
    /// fn twice(graph: &Graph, token: &Token, draft: DraftToSend) {
    ///     let _ = graph.mail_send(token, draft);
    ///     let _ = graph.mail_send(token, draft);
    /// }
    /// ```
    ///
    /// ```
    /// use halvo_connect::{DraftToSend, Graph, Mail, Token};
    /// fn once(graph: &Graph, token: &Token, draft: DraftToSend) {
    ///     let _ = graph.mail_send(token, draft);
    /// }
    /// ```
    fn mail_send(&self, token: &Token, draft: DraftToSend) -> Result<(), CallError>;
}

/// The reply rule of A1.2: the account's address and the senders of the slot `reply_senders`. Two addresses are
/// one by [`same_address`](crate::same_address).
pub struct ReplyRule {
    account: String,
    senders: Vec<String>,
}

impl ReplyRule {
    /// The rule for the account's address `account` and the senders of the slot `reply_senders`.
    pub fn new(account: String, senders: Vec<String>) -> ReplyRule {
        ReplyRule { account, senders }
    }

    fn selects(&self, is_read: bool, sender: &str, to: &[String]) -> bool {
        !is_read
            && to
                .iter()
                .any(|address| same_address(address, &self.account))
            && self
                .senders
                .iter()
                .any(|address| same_address(address, sender))
    }
}

/// A message that `mail_list` kept, as the provider gave it. `Debug` prints none of its text.
#[derive(Clone)]
pub struct Message {
    id: String,
    conversation: String,
    subject: String,
    sender: String,
    body: String,
}

impl Message {
    /// The provider's id of the message.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The provider's id of its conversation.
    pub fn conversation(&self) -> &str {
        &self.conversation
    }

    /// Its subject.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The address of its sender, a fact of A9.3 for the bound field `to` of `send`.
    pub fn sender(&self) -> &str {
        &self.sender
    }

    /// The text of its body, which is untrusted input (A9).
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// The kept list `messages` of A11.2, with the id of every message that the list answer held. Only `mail_list`
/// makes one, and `mail_draft_reply` takes it whole. It has no
/// `Clone`, so that one list is drafted once:
///
/// ```compile_fail
/// fn copy(messages: &halvo_connect::Messages) -> halvo_connect::Messages {
///     messages.clone()
/// }
/// ```
pub struct Messages {
    kept: Vec<Message>,
    listed: Vec<String>,
}

impl Messages {
    /// The messages, in the order of the provider's answer.
    pub fn iter(&self) -> std::slice::Iter<'_, Message> {
        self.kept.iter()
    }

    /// The count of messages.
    pub fn len(&self) -> usize {
        self.kept.len()
    }

    /// Whether the rule selected no message.
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

/// A draft that `mail_draft_reply` kept: its id, the id of the message that it answers, and its `to` and
/// `subject`, the bound fields of `send` (A9.4). `Debug` prints none of its text.
#[derive(Clone)]
pub struct Draft {
    id: String,
    message: String,
    to: Vec<String>,
    subject: String,
}

impl Draft {
    /// The provider's id of the draft.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The provider's id of the message that the draft answers.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The addresses of `toRecipients`.
    pub fn to(&self) -> &[String] {
        &self.to
    }

    /// Its subject.
    pub fn subject(&self) -> &str {
        &self.subject
    }
}

/// The kept list `drafts` of A11.2. Only `mail_draft_reply` makes one. It has no `Clone`, so that one list gives
/// its handles once:
///
/// ```compile_fail
/// fn copy(drafts: &halvo_connect::Drafts) -> halvo_connect::Drafts {
///     drafts.clone()
/// }
/// ```
pub struct Drafts(Vec<Draft>);

impl Drafts {
    /// The drafts, in the order of the messages.
    pub fn iter(&self) -> std::slice::Iter<'_, Draft> {
        self.0.iter()
    }

    /// The count of drafts.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there is no draft.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// One handle per draft, in order, each for one `mail_send`.
    pub fn into_sends(self) -> impl Iterator<Item = DraftToSend> {
        self.0.into_iter().map(DraftToSend)
    }
}

/// The handle of one draft of `drafts` for one `mail_send`. It has no `Clone` and no constructor outside this
/// crate:
///
/// ```compile_fail
/// fn copy(handle: &halvo_connect::DraftToSend) -> halvo_connect::DraftToSend {
///     handle.clone()
/// }
/// ```
///
/// ```compile_fail
/// fn make(item: &halvo_connect::Draft) -> halvo_connect::DraftToSend {
///     halvo_connect::DraftToSend(item.clone())
/// }
/// ```
///
/// ```
/// fn look(handle: &halvo_connect::DraftToSend) -> &halvo_connect::Draft {
///     handle.draft()
/// }
/// ```
pub struct DraftToSend(Draft);

impl DraftToSend {
    /// The draft that the handle sends.
    pub fn draft(&self) -> &Draft {
        &self.0
    }
}

impl Mail for Graph {
    fn mail_list(&self, token: &Token, rule: &ReplyRule) -> Result<Messages, CallError> {
        let answer = self.json(token, Method::GET, INBOX, None, 200)?;
        kept(&answer, rule)
    }

    fn mail_draft_reply(
        &self,
        token: &Token,
        messages: Messages,
        body: &str,
    ) -> Result<Drafts, CallError> {
        let content = json!({"body": {"contentType": "text", "content": body}});
        let mut ids = Ids::of(messages.listed.iter().map(String::as_str));
        let mut drafts = Vec::with_capacity(messages.len());
        for message in messages.kept {
            let created = self.json(
                token,
                Method::POST,
                &format!("/v1.0/me/messages/{}/createReply", segment(&message.id)),
                Some(&json!({})),
                201,
            )?;
            let id = answer::id(&created)?;
            ids.first(&id)?;
            let patched = self.json(
                token,
                Method::PATCH,
                &format!("/v1.0/me/messages/{}", segment(&id)),
                Some(&content),
                200,
            )?;
            if answer::id(&patched)? != id {
                return Err(CallError::Answer(AnswerFault::Item));
            }
            drafts.push(Draft {
                to: recipients(&patched)?,
                subject: answer::text(&patched, "subject")?,
                id,
                message: message.id,
            });
        }
        Ok(Drafts(drafts))
    }

    fn mail_send(&self, token: &Token, draft: DraftToSend) -> Result<(), CallError> {
        self.effect(
            token,
            Method::POST,
            &format!("/v1.0/me/messages/{}/send", segment(&draft.0.id)),
            202,
        )
    }
}

fn kept(answer: &Value, rule: &ReplyRule) -> Result<Messages, CallError> {
    let mut ids = Ids::default();
    let mut kept = Vec::new();
    let mut listed = Vec::new();
    for item in answer::page(answer)? {
        let id = answer::id(item)?;
        ids.first(&id)?;
        listed.push(id.clone());
        let is_read = item.get("isRead").and_then(Value::as_bool).ok_or(FORM)?;
        let sender = address(item.get("from"))?;
        let to = recipients(item)?;
        let conversation = answer::text(item, "conversationId")?;
        let subject = answer::text(item, "subject")?;
        let body = item
            .get("body")
            .and_then(|body| body.get("content"))
            .and_then(Value::as_str)
            .ok_or(FORM)?;
        if rule.selects(is_read, &sender, &to) {
            kept.push(Message {
                id,
                conversation,
                subject,
                sender,
                body: body.to_owned(),
            });
        }
    }
    Ok(Messages { kept, listed })
}

fn recipients(item: &Value) -> Result<Vec<String>, CallError> {
    item.get("toRecipients")
        .and_then(Value::as_array)
        .ok_or(FORM)?
        .iter()
        .map(|recipient| address(Some(recipient)))
        .collect()
}

fn address(value: Option<&Value>) -> Result<String, CallError> {
    value
        .and_then(|value| value.get("emailAddress"))
        .and_then(|email| email.get("address"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(FORM)
}

impl fmt::Debug for ReplyRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplyRule")
            .field("senders", &self.senders.len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message").finish_non_exhaustive()
    }
}

impl fmt::Debug for Messages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Messages")
            .field("len", &self.kept.len())
            .finish()
    }
}

impl fmt::Debug for Draft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Draft").finish_non_exhaustive()
    }
}

impl fmt::Debug for Drafts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Drafts")
            .field("len", &self.0.len())
            .finish()
    }
}

impl fmt::Debug for DraftToSend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DraftToSend").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
