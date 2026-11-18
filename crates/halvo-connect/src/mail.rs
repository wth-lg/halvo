use std::fmt;

use halvo_schema::executable::names::YesNo;
use halvo_schema::record::Instant;
use reqwest::Method;
use serde_json::{json, Value};

use crate::address::{same_address, Senders};
use crate::answer::{self, Ids, FORM};
use crate::calendar::{instant_text, seconds};
use crate::error::{AnswerFault, CallError};
use crate::graph::{segment, Collection, Graph};
use crate::list::{self, PAGE_SIZE};
use crate::token::Token;

const INBOX: &str = "/v1.0/me/mailFolders/inbox/messages";
const BOUND: usize = 10;
const FIELDS: &str =
    "id,isRead,from,toRecipients,replyTo,receivedDateTime,conversationId,subject,body,isReadReceiptRequested";

/// The mail operations of A11.2, each of the class of its row: `mail_list` is `read`, `mail_draft_reply` is
/// `prepare` and `mail_send` is `send`. Each takes the token for its call alone.
pub trait Mail {
    /// One chain of GET requests for each sender of the rule, in the order of the senders: the unread messages of
    /// the inbox from that sender, with the filter `isRead eq false and from/emailAddress/address eq '<sender>' and
    /// receivedDateTime ge <the rule's start, cut to its whole second>`, the apostrophe doubled and the whole value
    /// percent-encoded, a page size of 25 and the fields that the rule and the face read; it names no order. A chain
    /// follows each next link as it stands when the link is of the request's scheme, host and port and names the
    /// inbox's messages in either spelling of the folder's key without regard to ASCII case, and ends with its pages
    /// or after 10 pages. Then the rule, applied to every item whatever the filter said, keeps the first 10
    /// messages that it selects, in the order of the answers, and reads no further once it holds them. Keeps
    /// `messages` with the counts of what it left out. Refuses with `Answer` a page that is not the list's form,
    /// that holds more than 25 items, an id twice or an id that is empty, `.` or `..`, and a link of another place,
    /// to which no request is sent.
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

/// The reply rule of A1.2: the account's address, the senders of the slot `reply_senders` and the agent's start
/// instant (A7.4 `agent_start`). A message is selected when it is unread, its sender is one of the senders, it was
/// received at the start's whole second or later, and it names the account in `toRecipients`; the account and the
/// senders are held by [`same_address`](crate::same_address).
pub struct ReplyRule {
    account: String,
    senders: Senders,
    start: i64,
}

impl ReplyRule {
    /// The rule for the account's address `account` and the senders that [`reply_senders`](crate::reply_senders)
    /// read from the slot `reply_senders`. It takes the reader's answer and no list of text, so that no rule
    /// holds a sender that the reader never read:
    ///
    /// ```compile_fail,E0308
    /// let senders = vec!["ann berg@people.test".to_owned()];
    /// let start = halvo_schema::record::Instant(0);
    /// let rule = halvo_connect::ReplyRule::new("me@home.test".to_owned(), senders, start);
    /// ```
    ///
    /// ```
    /// let senders = halvo_connect::reply_senders("ann@people.test").unwrap();
    /// let start = halvo_schema::record::Instant(0);
    /// let rule = halvo_connect::ReplyRule::new("me@home.test".to_owned(), senders, start);
    /// ```
    pub fn new(account: String, senders: Senders, start: Instant) -> ReplyRule {
        ReplyRule {
            account,
            senders,
            start: start.0.div_euclid(1_000_000_000),
        }
    }

    fn selects(&self, is_read: bool, sender: &str, to: &[String]) -> bool {
        !is_read
            && to
                .iter()
                .any(|address| same_address(address, &self.account))
            && self.senders.holds(sender)
    }

    fn chain(&self, sender: &str) -> String {
        let filter = format!(
            "isRead eq false and from/emailAddress/address eq '{}' and receivedDateTime ge {}",
            sender.replace('\'', "''"),
            instant_text(self.start)
        );
        format!(
            "{INBOX}?$filter={}&$top={PAGE_SIZE}&$select={}",
            segment(&filter),
            segment(FIELDS)
        )
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
    receipt: bool,
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

    /// Whether it asks for a read receipt (`isReadReceiptRequested`): the adapter marks no such message read.
    pub fn asks_receipt(&self) -> bool {
        self.receipt
    }
}

/// The kept list `messages` of A11.2, with the id of every message that the list's answers held and the counts of
/// what the rule left out. Only `mail_list` makes one, and `mail_draft_reply` takes it whole. It has no `Clone`, so
/// that one list is drafted once:
///
/// ```compile_fail
/// fn copy(messages: &halvo_connect::Messages) -> halvo_connect::Messages {
///     messages.clone()
/// }
/// ```
pub struct Messages {
    kept: Vec<Message>,
    listed: Vec<String>,
    counts: MailCounts,
}

/// What `mail_list` left out and where it stopped, on the pages that it read, in the order of A11.2 and A7.4. Each
/// message that it left out stands in one count, the first that fits in the order `no_key`, `before_start`,
/// `not_to_account`, `incomplete`, `reply_to_other`; a message that is read or whose sender is none of the senders
/// is not selected and stands in none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailCounts {
    /// Messages received before the start's whole second, which the filter should have kept out.
    pub before_start: u32,
    /// `yes` when the call stopped at its bound of 10 messages with an item of a page, a link to a next page or a
    /// chain of a later sender not yet read.
    pub cut: YesNo,
    /// Items without an id or without a key that the rule reads: `isRead`, the sender's address, `toRecipients`
    /// with an address in each entry, `replyTo` with an address in each entry, and a `receivedDateTime` of the
    /// form `YYYY-MM-DDTHH:MM:SS`, up to seven digits of a fraction and `Z`.
    pub no_key: u32,
    /// Messages of a sender that name the account's address nowhere in `toRecipients`.
    pub not_to_account: u32,
    /// Messages that the rule selects and that lack another key of the face: `conversationId`, the body's
    /// `content`, `isReadReceiptRequested`, or a subject that is neither text nor null.
    pub incomplete: u32,
    /// Messages whose `replyTo` holds an address that is not, byte for byte, their sender's.
    pub reply_to_other: u32,
    /// `yes` only when a chain read 10 pages without meeting the bound and its tenth page carried a link.
    pub more_pages: YesNo,
    /// Entries left out because an earlier page of the call gave their id; they stand outside the sum of the
    /// counts above, and no value of it says that the list was read whole.
    pub earlier_page: u32,
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

    /// The counts of what the call left out and where it stopped.
    pub fn counts(&self) -> MailCounts {
        self.counts
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
        let firsts: Vec<String> = rule
            .senders
            .iter()
            .map(|sender| rule.chain(sender))
            .collect();
        let mut counts = MailCounts {
            before_start: 0,
            cut: YesNo::No,
            no_key: 0,
            not_to_account: 0,
            incomplete: 0,
            reply_to_other: 0,
            more_pages: YesNo::No,
            earlier_page: 0,
        };
        let chains = list::read(self, token, Collection::Inbox, &firsts, BOUND, |item| {
            let count = match verdict(item, rule) {
                Verdict::Kept(message) => return Some(message),
                Verdict::Other => return None,
                Verdict::NoKey => &mut counts.no_key,
                Verdict::BeforeStart => &mut counts.before_start,
                Verdict::NotToAccount => &mut counts.not_to_account,
                Verdict::Incomplete => &mut counts.incomplete,
                Verdict::ReplyToOther => &mut counts.reply_to_other,
            };
            *count = count.saturating_add(1);
            None
        })?;
        counts.cut = yes_no(chains.cut);
        counts.more_pages = yes_no(chains.more_pages);
        counts.earlier_page = chains.earlier_page;
        Ok(Messages {
            kept: chains.kept,
            listed: chains.listed,
            counts,
        })
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

pub(crate) fn yes_no(fact: bool) -> YesNo {
    if fact {
        YesNo::Yes
    } else {
        YesNo::No
    }
}

enum Verdict {
    Kept(Message),
    Other,
    NoKey,
    BeforeStart,
    NotToAccount,
    Incomplete,
    ReplyToOther,
}

fn verdict(item: &Value, rule: &ReplyRule) -> Verdict {
    let read = (
        item.get("id").and_then(Value::as_str),
        item.get("isRead").and_then(Value::as_bool),
        address(item.get("from")).ok(),
        addresses(item.get("toRecipients")),
        addresses(item.get("replyTo")),
        item.get("receivedDateTime")
            .and_then(Value::as_str)
            .and_then(|text| text.strip_suffix('Z'))
            .and_then(seconds),
    );
    let (Some(id), Some(is_read), Some(sender), Some(to), Some(reply_to), Some(received)) = read
    else {
        return Verdict::NoKey;
    };
    if is_read || !rule.senders.holds(&sender) {
        return Verdict::Other;
    }
    if received < rule.start {
        return Verdict::BeforeStart;
    }
    if !rule.selects(is_read, &sender, &to) {
        return Verdict::NotToAccount;
    }
    let face = (
        answer::text(item, "conversationId").ok(),
        item.get("body")
            .and_then(|body| body.get("content"))
            .and_then(Value::as_str),
        answer::subject(item),
        item.get("isReadReceiptRequested").and_then(Value::as_bool),
    );
    let (Some(conversation), Some(body), Some(subject), Some(receipt)) = face else {
        return Verdict::Incomplete;
    };
    if reply_to.iter().any(|address| *address != sender) {
        return Verdict::ReplyToOther;
    }
    Verdict::Kept(Message {
        id: id.to_owned(),
        conversation,
        subject,
        sender,
        body: body.to_owned(),
        receipt,
    })
}

fn addresses(value: Option<&Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|entry| address(Some(entry)).ok())
        .collect()
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
            .field("senders", &self.senders.iter().count())
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
