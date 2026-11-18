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
    /// plain text. Takes `messages` whole, so that a list is drafted once, and removes no draft.
    ///
    /// Before the PATCH the call fails, and no request names the answered id, when createReply does not answer
    /// 201, when its answer does not carry `isDraft` true (`Answer(Draft)`), or when its id is the id of any
    /// message of the list's answers, kept by the rule or not, or of an earlier draft of the call
    /// (`Answer(Repeated)`). After the PATCH the call fails when the answer's id is not the draft's
    /// (`Answer(Item)`), or when it lacks `isDraft`, `toRecipients`, `ccRecipients` or `bccRecipients`, or an
    /// entry of them has no address (`Answer(Form)`). Otherwise a draft whose answer does not carry `isDraft` true
    /// is returned unsent as `NotDraft`, and then a draft is held back, unsent, for the first reason of
    /// [`Unsent`] that its answer meets, each compare byte for byte; the other drafts are kept in `drafts`, each
    /// with its one recipient and its subject as the PATCH answered them.
    ///
    /// A failed call returns, beside its error, the drafts that it finished, the drafts that it returned unsent,
    /// the draft whose PATCH gave no answer that passed, and the messages that it did not reach.
    fn mail_draft_reply(
        &self,
        token: &Token,
        messages: Messages,
        body: &str,
    ) -> Result<Drafted, Box<DraftFailure>>;

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
    fn mail_send(&self, token: &Token, draft: DraftToSend) -> Result<Sent, CallError>;

    /// The mark of CP-109 (f), a sixth row of A11.2's table and no part of the send's effect: after the commit of
    /// the send that gave `sent` is stored, one PATCH of the key `isRead` to true on the message that the draft
    /// answered, whose answer must name that message and carry `isRead` true (`Answer(Item)`, `Answer(Form)`).
    /// It sends no request for a message that asks for a read receipt and answers `ReceiptAsked`. It takes the
    /// handle that one answered send gives, so a message is marked at most once for a send:
    ///
    /// ```compile_fail
    /// use halvo_connect::{Graph, Mail, Sent, Token};
    /// fn twice(graph: &Graph, token: &Token, sent: Sent) {
    ///     let _ = graph.mail_mark_read(token, sent);
    ///     let _ = graph.mail_mark_read(token, sent);
    /// }
    /// ```
    ///
    /// ```compile_fail
    /// fn make() -> halvo_connect::Sent {
    ///     halvo_connect::Sent { message: "m1".to_owned(), receipt: false }
    /// }
    /// ```
    ///
    /// ```
    /// use halvo_connect::{Graph, Mail, Sent, Token};
    /// fn once(graph: &Graph, token: &Token, sent: Sent) {
    ///     let _ = graph.mail_mark_read(token, sent);
    /// }
    /// ```
    fn mail_mark_read(&self, token: &Token, sent: Sent) -> Result<Marked, CallError>;
}

/// What a send that the provider answered with its success status leaves for the mark: the id of the message that
/// the draft answered and whether that message asks for a read receipt. Only `mail_send` makes one, and it has no
/// `Clone`. `Debug` prints none of its text.
pub struct Sent {
    message: String,
    receipt: bool,
}

impl Sent {
    /// The provider's id of the message that the sent draft answered.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// What `mail_mark_read` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marked {
    /// The provider answered the PATCH: the message is read.
    Read,
    /// The message asks for a read receipt, so no request was sent and it stays unread.
    ReceiptAsked,
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
    receipt: bool,
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

/// Why `mail_draft_reply` gives no send handle for a draft that its PATCH answered: `NotDraft`, or one of the
/// closed list of CP-160 (4) (b), for which the draft is held back and counts in `held_recipient`. Each compare
/// holds the texts as the provider's JSON gave them, byte for byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsent {
    /// The answer does not carry `isDraft` true: the draft stays drafted, and a later run sends its PATCH again.
    NotDraft,
    /// `toRecipients` holds no address or more than one.
    Recipients,
    /// The one address of `toRecipients` is not the sender of the message that the draft answers.
    Recipient,
    /// `ccRecipients` or `bccRecipients` holds an address.
    Copy,
    /// `conversationId` is present and is not the conversation of the message that the draft answers.
    Conversation,
}

impl Unsent {
    /// Whether the draft is held back for a reason of the closed list, so that it counts in `held_recipient`.
    pub fn is_held(self) -> bool {
        self != Unsent::NotDraft
    }
}

/// A draft that `mail_draft_reply` made and gives no send handle for: the id of the message that it answers, its
/// own id and why. `Debug` prints none of its text.
#[derive(Clone, PartialEq, Eq)]
pub struct UnsentDraft {
    message: String,
    draft: String,
    why: Unsent,
}

impl UnsentDraft {
    /// The provider's id of the message that the draft answers.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The provider's id of the draft.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Why it has no send handle.
    pub fn why(&self) -> Unsent {
        self.why
    }
}

/// The answer of `mail_draft_reply`: the kept list `drafts`, and the drafts that it made and returned unsent, in
/// the order of the messages.
pub struct Drafted {
    drafts: Drafts,
    unsent: Vec<UnsentDraft>,
}

impl Drafted {
    /// The drafts that it returned unsent, with their reasons.
    pub fn unsent(&self) -> &[UnsentDraft] {
        &self.unsent
    }

    /// The count `held_recipient` of A7.4: the drafts held back for a reason of the closed list.
    pub fn held_recipient(&self) -> u32 {
        let held = self
            .unsent
            .iter()
            .filter(|unsent| unsent.why.is_held())
            .count();
        u32::try_from(held).unwrap_or(u32::MAX)
    }

    /// The kept list `drafts`, each with a send handle.
    pub fn drafts(&self) -> &Drafts {
        &self.drafts
    }

    /// The kept list `drafts`, for its send handles.
    pub fn into_drafts(self) -> Drafts {
        self.drafts
    }
}

/// A draft that createReply made and whose PATCH gave no answer that passed: the ids of its message and of the
/// draft.
#[derive(Clone, PartialEq, Eq)]
pub struct Unfinished {
    message: String,
    draft: String,
}

impl Unfinished {
    /// The provider's id of the message that the draft answers.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The provider's id of the draft.
    pub fn draft(&self) -> &str {
        &self.draft
    }
}

/// Why `mail_draft_reply` stopped, with what it made before (CP-109 (g)). No request of the adapter removes a
/// draft, so each draft that it names lies in the person's Drafts folder. None of them has a send handle.
pub struct DraftFailure {
    error: CallError,
    finished: Vec<Draft>,
    unsent: Vec<UnsentDraft>,
    unfinished: Option<Unfinished>,
    unreached: Vec<String>,
}

impl DraftFailure {
    /// The error that stopped the call.
    pub fn error(&self) -> CallError {
        self.error
    }

    /// The drafts that it finished before the error, each of which would have had a send handle.
    pub fn finished(&self) -> &[Draft] {
        &self.finished
    }

    /// The drafts that it returned unsent before the error.
    pub fn unsent(&self) -> &[UnsentDraft] {
        &self.unsent
    }

    /// The draft whose PATCH gave no answer that passed, where the error came after createReply had answered.
    pub fn unfinished(&self) -> Option<&Unfinished> {
        self.unfinished.as_ref()
    }

    /// The ids of the messages for which it made no draft that it knows of, in order: the message whose
    /// createReply failed, and every message after the one where it stopped.
    pub fn unreached(&self) -> &[String] {
        &self.unreached
    }
}

impl fmt::Display for DraftFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for DraftFailure {}

enum Made {
    Draft(Draft),
    Unsent(UnsentDraft),
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
    ) -> Result<Drafted, Box<DraftFailure>> {
        let content = json!({"body": {"contentType": "text", "content": body}});
        let mut ids = Ids::of(messages.listed.iter().map(String::as_str));
        let mut drafts = Vec::with_capacity(messages.len());
        let mut unsent = Vec::new();
        let mut queue = messages.kept.into_iter();
        while let Some(message) = queue.next() {
            match self.draft_one(token, &content, &mut ids, &message) {
                Ok(Made::Draft(draft)) => drafts.push(draft),
                Ok(Made::Unsent(draft)) => unsent.push(draft),
                Err((error, unfinished)) => {
                    let mut unreached = Vec::new();
                    if unfinished.is_none() {
                        unreached.push(message.id);
                    }
                    unreached.extend(queue.map(|message| message.id));
                    return Err(Box::new(DraftFailure {
                        error,
                        finished: drafts,
                        unsent,
                        unfinished,
                        unreached,
                    }));
                }
            }
        }
        Ok(Drafted {
            drafts: Drafts(drafts),
            unsent,
        })
    }

    fn mail_send(&self, token: &Token, draft: DraftToSend) -> Result<Sent, CallError> {
        self.effect(
            token,
            Method::POST,
            &format!("/v1.0/me/messages/{}/send", segment(&draft.0.id)),
            202,
        )?;
        Ok(Sent {
            message: draft.0.message,
            receipt: draft.0.receipt,
        })
    }

    fn mail_mark_read(&self, token: &Token, sent: Sent) -> Result<Marked, CallError> {
        if sent.receipt {
            return Ok(Marked::ReceiptAsked);
        }
        let answer = self.json(
            token,
            Method::PATCH,
            &format!("/v1.0/me/messages/{}", segment(&sent.message)),
            Some(&json!({"isRead": true})),
            200,
        )?;
        if answer::id(&answer)? != sent.message {
            return Err(CallError::Answer(AnswerFault::Item));
        }
        if answer.get("isRead").and_then(Value::as_bool) != Some(true) {
            return Err(FORM);
        }
        Ok(Marked::Read)
    }
}

impl Graph {
    fn draft_one(
        &self,
        token: &Token,
        content: &Value,
        ids: &mut Ids,
        message: &Message,
    ) -> Result<Made, (CallError, Option<Unfinished>)> {
        let created = self
            .json(
                token,
                Method::POST,
                &format!("/v1.0/me/messages/{}/createReply", segment(&message.id)),
                Some(&json!({})),
                201,
            )
            .map_err(|error| (error, None))?;
        let id = answer::id(&created).map_err(|error| (error, None))?;
        if created.get("isDraft").and_then(Value::as_bool) != Some(true) {
            return Err((CallError::Answer(AnswerFault::Draft), None));
        }
        ids.first(&id).map_err(|error| (error, None))?;
        let unfinished = || {
            Some(Unfinished {
                message: message.id.clone(),
                draft: id.clone(),
            })
        };
        let patched = self
            .json(
                token,
                Method::PATCH,
                &format!("/v1.0/me/messages/{}", segment(&id)),
                Some(content),
                200,
            )
            .map_err(|error| (error, unfinished()))?;
        patched_draft(&patched, id.clone(), message).map_err(|error| (error, unfinished()))
    }
}

fn patched_draft(patched: &Value, id: String, message: &Message) -> Result<Made, CallError> {
    if answer::id(patched)? != id {
        return Err(CallError::Answer(AnswerFault::Item));
    }
    let is_draft = patched
        .get("isDraft")
        .and_then(Value::as_bool)
        .ok_or(FORM)?;
    let to = recipients(patched, "toRecipients")?;
    let copies =
        recipients(patched, "ccRecipients")?.len() + recipients(patched, "bccRecipients")?.len();
    let subject = answer::subject(patched).ok_or(FORM)?;
    let conversation = match patched.get("conversationId") {
        None => None,
        Some(conversation) => Some(conversation.as_str().ok_or(FORM)?),
    };
    let why = if !is_draft {
        Some(Unsent::NotDraft)
    } else if to.len() != 1 {
        Some(Unsent::Recipients)
    } else if to[0] != message.sender {
        Some(Unsent::Recipient)
    } else if copies > 0 {
        Some(Unsent::Copy)
    } else if conversation.is_some_and(|conversation| conversation != message.conversation) {
        Some(Unsent::Conversation)
    } else {
        None
    };
    Ok(match why {
        Some(why) => Made::Unsent(UnsentDraft {
            message: message.id.clone(),
            draft: id,
            why,
        }),
        None => Made::Draft(Draft {
            id,
            message: message.id.clone(),
            to,
            subject,
            receipt: message.receipt,
        }),
    })
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

fn recipients(item: &Value, key: &str) -> Result<Vec<String>, CallError> {
    item.get(key)
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

impl fmt::Debug for UnsentDraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnsentDraft")
            .field("why", &self.why)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Sent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sent").finish_non_exhaustive()
    }
}

impl fmt::Debug for Unfinished {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Unfinished").finish_non_exhaustive()
    }
}

impl fmt::Debug for Drafted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Drafted")
            .field("drafts", &self.drafts.len())
            .field("unsent", &self.unsent.len())
            .finish()
    }
}

impl fmt::Debug for DraftFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DraftFailure")
            .field("error", &self.error)
            .field("finished", &self.finished.len())
            .field("unsent", &self.unsent.len())
            .field("unfinished", &self.unfinished.is_some())
            .field("unreached", &self.unreached.len())
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
