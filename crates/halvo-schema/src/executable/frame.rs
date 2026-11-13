use std::fmt;
use std::num::NonZeroU32;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::value::RawValue;

use crate::ids::{AgentId, RequestId};
use crate::request::{self, Decoded, ErrorName, RunnerRequest};

use super::body::{RunnerCall, RunnerReply};
use super::bound::Bound;
use super::json::{self, each_once, from_json, list, object_type, opt, FromJson};
use super::names::{
    ActionClass, Answer, HandbackOutcome, HandbackReason, Outcome, SecretAnswer, Unreadable, YesNo,
};
use super::text::{
    ApprovalId, CreationTime, Digest, EffectId, Endpoint, Name, RunId, SecretText, WebOrigin, V1,
};
use super::{Refusal, ERROR_NAMES};

/// The most bytes of the JSON of one frame.
pub const FRAME_JSON_MAX: usize = 65_536;

object_type! {
    /// One effect of a confirmation request: a confirm class, its digest, its bound object of the same class, and
    /// the origin that a supervised first run would add to the bindings, which only a request of the browser has.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ConfirmEffect {
        pub effect: EffectId = "effect" => from_json,
        pub class: ActionClass = "class" => from_json,
        pub digest: Digest = "digest" => from_json,
        pub bound: Bound = "bound" => Bound::from_raw,
        pub new_origin: Option<WebOrigin> = "new_origin" => opt(from_json),
    }
    rules(e) {
        let fail = |key, rule| Err(Refusal::malformed("ConfirmEffect", rule).at(key));
        if !e.class.is_confirm() {
            return fail("class", "not_a_confirm_class");
        }
        if e.bound.class() != e.class {
            return fail("bound", "class_of_bound");
        }
        if e.new_origin.is_some() && !e.class.is_browser_effect() {
            return fail("new_origin", "new_origin_of_adapter");
        }
        Ok(())
    }
}

object_type! {
    /// A confirmation request: one or more effects, each once; an effect of a request of the browser stands alone.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ConfirmRequest {
        pub v: V1 = "v" => from_json,
        pub run: RunId = "run" => from_json,
        pub agent: AgentId = "agent" => json::agent_id,
        pub effects: Vec<ConfirmEffect> = "effects" => list(ConfirmEffect::from_raw),
    }
    rules(r) {
        let fail = |rule| Err(Refusal::malformed("ConfirmRequest", rule).at("effects"));
        if r.effects.is_empty() {
            return fail("no_effect");
        }
        let ids: Vec<&EffectId> = r.effects.iter().map(|e| &e.effect).collect();
        each_once("ConfirmRequest", &ids).map_err(|e| e.at("effects"))?;
        if r.effects.len() > 1 && r.effects.iter().any(|e| e.class.is_browser_effect()) {
            return fail("browser_effect_not_alone");
        }
        Ok(())
    }
}

object_type! {
    /// The answer for one effect, under an approval id that the supervisor minted.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Approval {
        pub approval: ApprovalId = "approval" => from_json,
        pub effect: EffectId = "effect" => from_json,
        pub digest: Digest = "digest" => from_json,
        pub answer: Answer = "answer" => from_json,
    }
    rules(_a) { Ok(()) }
}

object_type! {
    /// A confirmation decision: one or more approvals, each effect and each approval id once.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ConfirmDecision {
        pub v: V1 = "v" => from_json,
        pub approvals: Vec<Approval> = "approvals" => list(Approval::from_raw),
    }
    rules(d) {
        if d.approvals.is_empty() {
            return Err(Refusal::malformed("ConfirmDecision", "no_approval").at("approvals"));
        }
        let effects: Vec<&EffectId> = d.approvals.iter().map(|a| &a.effect).collect();
        let ids: Vec<&ApprovalId> = d.approvals.iter().map(|a| &a.approval).collect();
        each_once("ConfirmDecision", &effects).map_err(|e| e.at("approvals"))?;
        each_once("ConfirmDecision", &ids).map_err(|e| e.at("approvals"))
    }
}

impl ConfirmDecision {
    /// `Ok` when the decision holds one approval per effect of the request and no other, each with the digest
    /// of its effect in the request.
    pub fn answers(&self, request: &ConfirmRequest) -> Result<(), Refusal> {
        let fail = Err(Refusal::malformed("ConfirmDecision", "not_the_request").at("approvals"));
        if self.approvals.len() != request.effects.len() {
            return fail;
        }
        for approval in &self.approvals {
            match request.effects.iter().find(|e| e.effect == approval.effect) {
                Some(effect) if effect.digest == approval.digest => {}
                _ => return fail,
            }
        }
        Ok(())
    }
}

object_type! {
    /// A handback request: the reason, the origin of the page where there is one, and the debugging address of the
    /// stand-in's browser, which a product supervisor never gets.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct HandbackRequest {
        pub v: V1 = "v" => from_json,
        pub run: RunId = "run" => from_json,
        pub agent: AgentId = "agent" => json::agent_id,
        pub reason: HandbackReason = "reason" => from_json,
        pub origin: Option<WebOrigin> = "origin" => opt(from_json),
        pub endpoint: Option<Endpoint> = "endpoint" => opt(from_json),
    }
    rules(_r) { Ok(()) }
}

object_type! {
    /// A handback result: the outcome, and the name of an entry of the supervisor's secret store, never its value,
    /// which only the outcome `done` names.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct HandbackResult {
        pub v: V1 = "v" => from_json,
        pub outcome: HandbackOutcome = "outcome" => from_json,
        pub secret: Option<Name> = "secret" => opt(from_json),
    }
    rules(r) {
        if r.secret.is_some() && r.outcome != HandbackOutcome::Done {
            return Err(Refusal::malformed("HandbackResult", "secret_without_done").at("secret"));
        }
        Ok(())
    }
}

/// The envelope of a `store` frame: a runner request of the workflow run's form whose body is a runner body of this
/// run, kept as the bytes the runner wrote, so that the supervisor relays them unchanged. `Debug` prints the request
/// id and the body, whose texts print their lengths alone.
#[derive(Clone)]
pub struct StoreRequest {
    envelope: Box<RawValue>,
    id: RequestId,
    body: RunnerCall,
}

impl StoreRequest {
    /// The envelope of a runner request, or the refusal when its body breaks a rule.
    pub fn new(request: &RunnerRequest<RunnerCall>) -> Result<StoreRequest, Refusal> {
        let bytes = request
            .to_bytes()
            .map_err(|_| Refusal::malformed("StoreRequest", "envelope"))?;
        StoreRequest::from_raw(json::whole("StoreRequest", &bytes)?)
    }

    fn from_raw(raw: &RawValue) -> Result<StoreRequest, Refusal> {
        if !raw.get().starts_with('{') {
            return Err(Refusal::malformed("StoreRequest", "not_an_object"));
        }
        match request::decode(raw.get().as_bytes()) {
            Ok(Decoded::Raw(r)) if r.name() == "runner" => Ok(StoreRequest {
                envelope: raw.to_owned(),
                id: r.id().clone(),
                body: RunnerCall::from_raw(r.body()).map_err(|e| e.at("body"))?,
            }),
            _ => Err(Refusal::malformed("StoreRequest", "not_a_runner_request")),
        }
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// The body.
    pub fn body(&self) -> &RunnerCall {
        &self.body
    }

    /// The envelope as the runner wrote it.
    pub fn envelope(&self) -> &str {
        self.envelope.get()
    }
}

impl fmt::Debug for StoreRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoreRequest")
            .field("id", &self.id)
            .field("body", &self.body)
            .finish()
    }
}

impl PartialEq for StoreRequest {
    fn eq(&self, other: &Self) -> bool {
        self.envelope.get() == other.envelope.get()
    }
}

impl Eq for StoreRequest {}

impl Serialize for StoreRequest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.envelope.serialize(serializer)
    }
}

impl FromJson for StoreRequest {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        StoreRequest::from_raw(raw)
    }
}

/// An error name of this run's handlers, one of `ERROR_NAMES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RunError(&'static str);

impl RunError {
    /// The name when it is one of `ERROR_NAMES`, else the refusal with `malformed`.
    pub fn parse(s: &str) -> Result<RunError, Refusal> {
        ERROR_NAMES
            .iter()
            .find(|name| **name == s)
            .map(|name| RunError(name))
            .ok_or_else(|| Refusal::malformed("RunError", "unknown_name"))
    }

    /// The name.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

/// The error of a reply to a runner request: one of the error names of the workflow run's reply form, or one of
/// this run's handlers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReplyError {
    Workflow(ErrorName),
    Run(RunError),
}

impl ReplyError {
    /// The error name.
    pub fn as_str(self) -> &'static str {
        match self {
            ReplyError::Workflow(name) => name.as_str(),
            ReplyError::Run(name) => name.as_str(),
        }
    }
}

/// The reply that a `store_reply` frame carries, in the reply form of the workflow run
/// (`{"v":1,"id":..,"ok":..}` or `{"v":1,"id":..,"error":..}`): the request id, or none where the request's id
/// could not be read, and a runner reply or an error name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreReply {
    pub id: Option<RequestId>,
    pub outcome: Result<RunnerReply, ReplyError>,
}

impl StoreReply {
    fn from_raw(raw: &RawValue) -> Result<StoreReply, Refusal> {
        let obj = json::object("StoreReply", raw)?;
        let ok = obj.has("ok");
        obj.exactly(if ok {
            &["v", "id", "ok"]
        } else {
            &["v", "id", "error"]
        })?;
        obj.read("v", V1::from_json)?;
        let id = obj.read("id", opt(json::request_id))?;
        let outcome = if ok {
            Ok(obj.read("ok", RunnerReply::from_raw)?)
        } else {
            let name = obj.read("error", |raw| json::string("StoreReply", raw))?;
            Err(match ErrorName::parse(&name) {
                Ok(name) => ReplyError::Workflow(name),
                Err(_) => ReplyError::Run(RunError::parse(&name).map_err(|e| e.at("error"))?),
            })
        };
        Ok(StoreReply { id, outcome })
    }
}

impl Serialize for StoreReply {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("v", &V1)?;
        map.serialize_entry("id", &self.id)?;
        match &self.outcome {
            Ok(reply) => map.serialize_entry("ok", reply)?,
            Err(error) => map.serialize_entry("error", error.as_str())?,
        }
        map.end()
    }
}

impl FromJson for StoreReply {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        StoreReply::from_raw(raw)
    }
}

object_type! {
    /// `{"t":"hello","run":..,"agent":..,"v":1}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Hello [t = "hello"] {
        pub run: RunId = "run" => from_json,
        pub agent: AgentId = "agent" => json::agent_id,
        pub v: V1 = "v" => from_json,
    }
    rules(_f) { Ok(()) }
}

object_type! {
    /// `{"t":"store","request":<envelope>}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Store [t = "store"] {
        pub request: StoreRequest = "request" => from_json,
    }
    rules(_f) { Ok(()) }
}

object_type! {
    /// `{"t":"confirm","request":<confirmation request>}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Confirm [t = "confirm"] {
        pub request: ConfirmRequest = "request" => ConfirmRequest::from_raw,
    }
    rules(_f) { Ok(()) }
}

object_type! {
    /// `{"t":"handback","request":<handback request>}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Handback [t = "handback"] {
        pub request: HandbackRequest = "request" => HandbackRequest::from_raw,
    }
    rules(_f) { Ok(()) }
}

object_type! {
    /// `{"t":"secret","ref":"<secret name>"}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Secret [t = "secret"] {
        pub secret: Name = "ref" => from_json,
    }
    rules(_f) { Ok(()) }
}

object_type! {
    /// `{"t":"progress","step":..,"of":..}`, the step at most `of`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Progress [t = "progress"] {
        pub step: NonZeroU32 = "step" => from_json,
        pub of: NonZeroU32 = "of" => from_json,
    }
    rules(f) {
        if f.step > f.of {
            return Err(Refusal::malformed("Progress", "step_above_of").at("step"));
        }
        Ok(())
    }
}

object_type! {
    /// `{"t":"done","outcome":..}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Done [t = "done"] {
        pub outcome: Outcome = "outcome" => from_json,
    }
    rules(_f) { Ok(()) }
}

object_type! {
    /// `{"t":"store_reply","reply":<reply>}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StoreReplyFrame [t = "store_reply"] {
        pub reply: StoreReply = "reply" => from_json,
    }
    rules(_f) { Ok(()) }
    arrives(FRAME_JSON_MAX, "frame_length")
}

object_type! {
    /// `{"t":"confirm_reply","decision":<confirmation decision>}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ConfirmReply [t = "confirm_reply"] {
        pub decision: ConfirmDecision = "decision" => ConfirmDecision::from_raw,
    }
    rules(_f) { Ok(()) }
    arrives(FRAME_JSON_MAX, "frame_length")
}

object_type! {
    /// `{"t":"handback_reply","result":<handback result>}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct HandbackReply [t = "handback_reply"] {
        pub result: HandbackResult = "result" => HandbackResult::from_raw,
    }
    rules(_f) { Ok(()) }
    arrives(FRAME_JSON_MAX, "frame_length")
}

/// `secret_reply`, one of the three answers of `Secrets::read`: `{"t":"secret_reply","answer":"stored","value":"<text>"}`,
/// `{"t":"secret_reply","answer":"none"}` or `{"t":"secret_reply","answer":"unreadable","member":"locked"|"fault"}`.
/// Only `None` says that no secret is stored; a store that could not be read is `Unreadable`. A value stands only
/// with the answer `stored` and a member only with the answer `unreadable`, so the type has no form of the other
/// pairs, and the parser refuses them. `Debug` prints no value.
///
/// What must not compile, a value with the answer `none` and a member with the answer `stored`:
///
/// ```compile_fail
/// use halvo_schema::executable::frame::SecretReply;
/// use halvo_schema::executable::text::SecretText;
/// let _ = SecretReply::None(SecretText::parse("tok").unwrap());
/// ```
///
/// ```compile_fail
/// use halvo_schema::executable::frame::SecretReply;
/// use halvo_schema::executable::names::Unreadable;
/// let _ = SecretReply::Stored(Unreadable::Locked);
/// ```
///
/// Their twins, which differ in the one name and compile:
///
/// ```
/// use halvo_schema::executable::frame::SecretReply;
/// use halvo_schema::executable::text::SecretText;
/// let _ = SecretReply::Stored(SecretText::parse("tok").unwrap());
/// ```
///
/// ```
/// use halvo_schema::executable::frame::SecretReply;
/// use halvo_schema::executable::names::Unreadable;
/// let _ = SecretReply::Unreadable(Unreadable::Locked);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretReply {
    Stored(SecretText),
    None,
    Unreadable(Unreadable),
}

impl json::Arrives for SecretReply {
    const MAX: usize = FRAME_JSON_MAX;
}

impl SecretReply {
    const KEYS: &'static [&'static str] = &["t", "answer", "value", "member"];

    /// The answer.
    pub fn answer(&self) -> SecretAnswer {
        match self {
            SecretReply::Stored(_) => SecretAnswer::Stored,
            SecretReply::None => SecretAnswer::None,
            SecretReply::Unreadable(_) => SecretAnswer::Unreadable,
        }
    }

    fn from_raw(raw: &RawValue) -> Result<SecretReply, Refusal> {
        let fail = |key, rule| Err(Refusal::malformed("SecretReply", rule).at(key));
        let obj = json::object("SecretReply", raw)?;
        obj.known(Self::KEYS)?;
        let tag = obj.read("t", |raw| json::string("SecretReply", raw))?;
        if tag != "secret_reply" {
            return fail("t", "tag");
        }
        let answer = obj.read("answer", SecretAnswer::from_json)?;
        if obj.has("value") && answer != SecretAnswer::Stored {
            return fail("value", "value_without_stored");
        }
        if obj.has("member") && answer != SecretAnswer::Unreadable {
            return fail("member", "member_without_unreadable");
        }
        match answer {
            SecretAnswer::Stored if !obj.has("value") => fail("value", "stored_without_value"),
            SecretAnswer::Stored => obj
                .read("value", SecretText::from_json)
                .map(SecretReply::Stored),
            SecretAnswer::None => Ok(SecretReply::None),
            SecretAnswer::Unreadable if !obj.has("member") => {
                fail("member", "unreadable_without_member")
            }
            SecretAnswer::Unreadable => obj
                .read("member", Unreadable::from_json)
                .map(SecretReply::Unreadable),
        }
    }

    /// Reads one JSON value of this type and refuses every other input. An input of more than `FRAME_JSON_MAX`
    /// bytes, counted on the bytes given before anything is read, is refused with the cause `length` and the rule
    /// `frame_length`.
    pub fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
        if bytes.len() > FRAME_JSON_MAX {
            return Err(Refusal::length("SecretReply", "frame_length"));
        }
        Self::from_raw(json::whole("SecretReply", bytes)?)
    }

    /// The compact JSON of the frame.
    pub fn to_json(&self) -> Result<String, Refusal> {
        json::write_json("SecretReply", self)
    }
}

impl Serialize for SecretReply {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("t", "secret_reply")?;
        map.serialize_entry("answer", &self.answer())?;
        match self {
            SecretReply::Stored(value) => map.serialize_entry("value", value)?,
            SecretReply::None => {}
            SecretReply::Unreadable(member) => map.serialize_entry("member", member)?,
        }
        map.end()
    }
}

object_type! {
    /// `{"t":"cancel"}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Cancel [t = "cancel"] {}
    rules(_f) { Ok(()) }
    arrives(FRAME_JSON_MAX, "frame_length")
}

object_type! {
    /// `{"t":"bind","origin":<origin>}`: the origin of the address that the runner read once after the result `done`
    /// of a handback of the reasons `sign_in`, `second_factor` or `captcha`, made by `WebOrigin::of_address`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Bind [t = "bind"] {
        pub origin: WebOrigin = "origin" => from_json,
    }
    rules(_f) { Ok(()) }
}

object_type! {
    /// `{"t":"browser","pid":<count>,"created":"<creation time>","group":<count>|null}`: the process of the browser
    /// that the runner started, its creation time as the process table records it, and its process group on macOS
    /// (null on Windows), sent once the browser runs and before it loads any page.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Browser [t = "browser"] {
        pub pid: NonZeroU32 = "pid" => from_json,
        pub created: CreationTime = "created" => from_json,
        pub group: Option<NonZeroU32> = "group" => opt(from_json),
    }
    rules(_f) { Ok(()) }
}

object_type! {
    /// `{"t":"bind_reply","bound":"yes"|"no"}`: `yes` only once the origin is bound, `no` only after the person's
    /// `decline`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct BindReply [t = "bind_reply"] {
        pub bound: YesNo = "bound" => from_json,
    }
    rules(_f) { Ok(()) }
    arrives(FRAME_JSON_MAX, "frame_length")
}

object_type! {
    /// `{"t":"browser_reply"}`, written once the mark of the run holds the browser's line.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct BrowserReply [t = "browser_reply"] {}
    rules(_f) { Ok(()) }
    arrives(FRAME_JSON_MAX, "frame_length")
}

macro_rules! frame_enum {
    (
        bounded($max:expr)
        $(#[$meta:meta])*
        pub enum $name:ident { $( $variant:ident($ty:ident) = $t:literal ),+ $(,)? }
    ) => {
        frame_enum! {
            $(#[$meta])*
            pub enum $name { $( $variant($ty) = $t ),+ }
        }

        const _: () = {
            $( assert!(<$ty as json::Arrives>::MAX == $max); )+
        };
    };
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $( $variant:ident($ty:ident) = $t:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq)]
        #[allow(clippy::large_enum_variant)]
        pub enum $name {
            $( $variant($ty), )+
        }

        impl $name {
            /// Reads the JSON of one frame, at most 65,536 bytes (else the cause `length`), and refuses every other
            /// input.
            pub fn parse(bytes: &[u8]) -> Result<Self, Refusal> {
                if bytes.len() > FRAME_JSON_MAX {
                    return Err(Refusal::length(stringify!($name), "frame_length"));
                }
                let raw = json::whole(stringify!($name), bytes)?;
                let obj = json::object(stringify!($name), raw)?;
                let t = obj.read("t", |raw| json::string(stringify!($name), raw))?;
                match t.as_str() {
                    $( $t => $ty::from_raw(raw).map($name::$variant), )+
                    _ => Err(Refusal::malformed(stringify!($name), "unknown_frame").at("t")),
                }
            }

            /// The JSON of the frame, or the refusal of a rule it breaks or of more than 65,536 bytes.
            pub fn to_json(&self) -> Result<String, Refusal> {
                let text = match self {
                    $( $name::$variant(f) => f.to_json()?, )+
                };
                if text.len() > FRAME_JSON_MAX {
                    return Err(Refusal::length(stringify!($name), "frame_length"));
                }
                Ok(text)
            }
        }
    };
}

frame_enum! {
    /// A frame from the runner to its supervisor.
    pub enum FromRunner {
        Hello(Hello) = "hello",
        Store(Store) = "store",
        Confirm(Confirm) = "confirm",
        Handback(Handback) = "handback",
        Secret(Secret) = "secret",
        Progress(Progress) = "progress",
        Done(Done) = "done",
        Bind(Bind) = "bind",
        Browser(Browser) = "browser",
    }
}

frame_enum! {
    bounded(FRAME_JSON_MAX)
    /// A frame from the supervisor to the runner. Each variant's type has a bound of `FRAME_JSON_MAX` bytes as they
    /// arrive in its own `parse`, or the crate does not compile.
    pub enum ToRunner {
        StoreReply(StoreReplyFrame) = "store_reply",
        ConfirmReply(ConfirmReply) = "confirm_reply",
        HandbackReply(HandbackReply) = "handback_reply",
        SecretReply(SecretReply) = "secret_reply",
        Cancel(Cancel) = "cancel",
        BindReply(BindReply) = "bind_reply",
        BrowserReply(BrowserReply) = "browser_reply",
    }
}
