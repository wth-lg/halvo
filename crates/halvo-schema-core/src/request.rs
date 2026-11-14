use std::fmt;
use std::marker::PhantomData;

use serde::de::{DeserializeOwned, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::ids::{
    name_enum, object, strict_struct, Position, RequestId, StoreId, Strict, SummaryId, WorkflowId,
    MAX_ORDINAL, VERSION,
};
use crate::record::{Episode, EvidencePage, WorkflowsPage};

pub(crate) struct Key(());

/// The bound of a serialized analysis write in bytes, so that it fits the 65,536 payload bytes of the seam's
/// frame with the 35 bytes of its wrapper.
pub const ANALYSIS_WRITE_MAX: usize = 65_500;

/// The bound of a serialized interface request in bytes.
pub const INTERFACE_REQUEST_MAX: usize = 4_096;

/// The bound of a serialized reply in bytes, its envelope included, which every counter of a reply uses. The
/// envelope takes 53 bytes beside a request id and 23 beside a null id, so a page holds at most 261,067 bytes
/// beside a request id.
pub const REPLY_MAX: usize = 261_120;

/// The names that this module writes in a request or a reply: the keys of the envelope, of the bodies and of a
/// reply, the five request names, the two names of a read, the three decisions and the four roles. No error name
/// is among them.
pub const NAMES: &[&str] = &[
    "v",
    "id",
    "request",
    "body",
    "analysis_write",
    "interface_read",
    "interface_decision",
    "runner",
    "coordinator_write",
    "store",
    "episodes",
    "commit",
    "read",
    "after",
    "workflow",
    "workflows",
    "evidence",
    "decision",
    "approve",
    "dismiss",
    "delete",
    "analysis",
    "interface",
    "coordinator",
    "ok",
    "error",
];

const REQUEST_NAMES: [&str; 5] = [
    "analysis_write",
    "interface_read",
    "interface_decision",
    "runner",
    "coordinator_write",
];

name_enum! {
    /// The four roles of this run. A channel admits some of them, and the owner answers `role` to any other.
    pub enum Role {
        Analysis = "analysis",
        Interface = "interface",
        Runner = "runner",
        Coordinator = "coordinator",
    }
}

/// The error name of a reply: one of the nine of the reply form, or a name that a file of a later run declares in
/// its list `ERROR_NAMES` and that run's handlers answer. This type holds an error name; no error name is a key or
/// a stored token of a record or a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorName {
    Malformed,
    Version,
    Role,
    TooLarge,
    Store,
    NoWorkflow,
    State,
    Unsupported,
    Internal,
    /// A name that a file of a later run declares and that is none of the nine.
    Declared(DeclaredName),
}

/// A name that a file of a later run declares in its list `ERROR_NAMES` and that is none of the nine names of the
/// reply form. It holds the string of the list that declares it, and only `ErrorName::read` makes one, so no value
/// holds a name that no file declares.
///
/// ```compile_fail
/// let name = halvo_schema_core::request::DeclaredName("nobody_declares_this");
/// ```
///
/// ```
/// let name = halvo_schema_core::request::ErrorName::read("nobody_declares_this");
/// assert!(name.is_err());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclaredName(&'static str);

impl DeclaredName {
    /// The name: the string of the list `ERROR_NAMES` that declares it.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl ErrorName {
    /// The nine names of the reply form, in the order of the contract.
    pub const ALL: &'static [ErrorName] = &[
        ErrorName::Malformed,
        ErrorName::Version,
        ErrorName::Role,
        ErrorName::TooLarge,
        ErrorName::Store,
        ErrorName::NoWorkflow,
        ErrorName::State,
        ErrorName::Unsupported,
        ErrorName::Internal,
    ];

    /// The name as it is serialized.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorName::Malformed => "malformed",
            ErrorName::Version => "version",
            ErrorName::Role => "role",
            ErrorName::TooLarge => "too_large",
            ErrorName::Store => "store",
            ErrorName::NoWorkflow => "no_workflow",
            ErrorName::State => "state",
            ErrorName::Unsupported => "unsupported",
            ErrorName::Internal => "internal",
            ErrorName::Declared(name) => name.as_str(),
        }
    }

    /// Returns the one of the nine whose name is `s`, else the error with the rule `unknown_name`. It takes no
    /// declared name: `read` does.
    pub fn parse(s: &str) -> Result<Self, crate::ids::ParseError> {
        ErrorName::ALL
            .iter()
            .copied()
            .find(|name| name.as_str() == s)
            .ok_or_else(|| crate::ids::ParseError::new(Key(()), "ErrorName", "unknown_name"))
    }

    /// Returns the error name of a reply whose text is `s`: one of the nine, else a name that a file of a later run
    /// declares, which holds the string of the list that declares it; else the error with the rule `unknown_name`.
    /// A name of the nine that a later file lists again is that one of the nine.
    pub fn read(s: &str) -> Result<Self, crate::ids::ParseError> {
        ErrorName::parse(s).or_else(|refused| {
            crate::kinds::declared_error_names()
                .find(|name| *name == s)
                .map(|name| ErrorName::Declared(DeclaredName(name)))
                .ok_or(refused)
        })
    }
}

impl fmt::Display for ErrorName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ErrorName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::ids::parse_str(deserializer, "a name of ErrorName", ErrorName::read)
    }
}

macro_rules! role_token {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug)]
        pub struct $name {
            _private: (),
        }

        impl $name {
            /// Claims this role. A binary claims its one role in `main`. A token is no proof to the owner.
            pub fn claim() -> Self {
                $name { _private: () }
            }
        }
    };
}

role_token! {
    /// The token of the role `analysis`, which the analysis worker claims. It builds the analysis write alone.
    AnalysisRole
}

role_token! {
    /// The token of the role `interface`, which the app's command module claims. It builds the interface read
    /// and the interface decision.
    InterfaceRole
}

role_token! {
    /// The token of the role `runner`, which a runner of the agent-setup run claims. It builds runner requests.
    RunnerRole
}

role_token! {
    /// The token of the role `coordinator`, which the host-side handler of the agent-setup run claims. It builds
    /// coordinator writes, under which every state change but a decision and every configuration change arrive.
    CoordinatorRole
}

/// A body of a runner request: a type of the agent-setup run, with the type of its reply.
pub trait RunnerBody: Serialize + DeserializeOwned {
    type Reply: Serialize + DeserializeOwned;
}

/// A body of a coordinator write: a type of the run that owns its handler, a JSON object whose key `op` names
/// the call, with the type of its reply.
pub trait CoordinatorBody: Serialize + DeserializeOwned {
    type Reply: Serialize + DeserializeOwned;
}

/// A role that a later run declares in its own file, with a token type that has a private field and one
/// `claim()`. `NAME` is 1 to 32 bytes of `a` to `z`, `0` to `9` and `_`, and none of the five request names
/// and none of the four role names of this run.
pub trait ExtRole {
    const NAME: &'static str;
}

/// A body of a request of the further role `R`, with the type of its reply.
pub trait ExtBody<R: ExtRole>: Serialize + DeserializeOwned {
    type Reply: Serialize + DeserializeOwned;
}

fn envelope<B: Serialize + ?Sized>(
    id: &RequestId,
    request: &str,
    body: &B,
) -> Result<Vec<u8>, ErrorName> {
    #[derive(Serialize)]
    struct Envelope<'a, B: ?Sized> {
        v: u32,
        id: &'a RequestId,
        request: &'a str,
        body: &'a B,
    }
    serde_json::to_vec(&Envelope {
        v: VERSION,
        id,
        request,
        body,
    })
    .map_err(|_| ErrorName::Malformed)
}

fn bounded(bytes: Vec<u8>, max: usize) -> Result<Vec<u8>, ErrorName> {
    if bytes.len() > max {
        return Err(ErrorName::TooLarge);
    }
    Ok(bytes)
}

strict_struct! {
    #[derive(Debug)]
    struct AnalysisWriteBody {
        pub store: StoreId,
        pub episodes: Vec<Episode>,
        pub commit: Option<Position>,
    }
    rules(_b) {
        Ok(())
    }
}

const LONGEST_POSITION: Position = Position {
    seg: MAX_ORDINAL,
    idx: MAX_ORDINAL,
};

struct Unchecked<'a, T>(&'a T);

impl<T: Strict> Serialize for Unchecked<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.write_fields(serializer, Key(()))
    }
}

fn episode_len(episode: &Episode) -> usize {
    serde_json::to_vec(&Unchecked(episode)).map_or(usize::MAX, |b| b.len())
}

fn counted(id: &RequestId, body: &AnalysisWriteBody) -> usize {
    let empty = AnalysisWriteBody {
        store: body.store.clone(),
        episodes: Vec::new(),
        commit: Some(LONGEST_POSITION),
    };
    let base = envelope(id, "analysis_write", &empty).map_or(usize::MAX, |b| b.len());
    let episodes = body
        .episodes
        .iter()
        .fold(0usize, |sum, e| sum.saturating_add(episode_len(e)));
    let commas = body.episodes.len().saturating_sub(1);
    base.saturating_add(episodes).saturating_add(commas)
}

/// The answer of `push` that refuses an episode, with the episode; the write is unchanged.
#[derive(Debug, PartialEq, Eq)]
pub enum PushRefused {
    /// The episode holds a value that its reader would refuse. No cut of its steps mends it.
    Malformed(Episode),
    /// The episode would take the write over `ANALYSIS_WRITE_MAX` bytes.
    TooLarge(Episode),
}

impl PushRefused {
    /// The episode that `push` handed back.
    pub fn into_episode(self) -> Episode {
        match self {
            PushRefused::Malformed(episode) | PushRefused::TooLarge(episode) => episode,
        }
    }

    /// The error name: `malformed` or `too_large`.
    pub fn name(&self) -> ErrorName {
        match self {
            PushRefused::Malformed(_) => ErrorName::Malformed,
            PushRefused::TooLarge(_) => ErrorName::TooLarge,
        }
    }
}

/// The analysis write, the one request of the role `analysis`: a store id, episodes and an optional commit
/// position. It has no key for a state, a decision or a configuration.
#[derive(Debug)]
pub struct AnalysisWrite {
    id: RequestId,
    body: AnalysisWriteBody,
    counted: usize,
}

impl AnalysisWrite {
    /// An empty write with `commit` null.
    pub fn new(_role: &AnalysisRole, id: RequestId, store: StoreId) -> Self {
        let body = AnalysisWriteBody {
            store,
            episodes: Vec::new(),
            commit: None,
        };
        let counted = counted(&id, &body);
        AnalysisWrite { id, body, counted }
    }

    /// Adds the episode, or hands it back and leaves the write unchanged: as `PushRefused::Malformed` when it
    /// holds a value that its reader would refuse, else as `PushRefused::TooLarge` when it would take the write
    /// over `ANALYSIS_WRITE_MAX` bytes, counted as if `commit` held the longest position (53 bytes).
    #[allow(clippy::result_large_err)]
    pub fn push(&mut self, episode: Episode) -> Result<(), PushRefused> {
        let Ok(bytes) = serde_json::to_vec(&episode) else {
            return Err(PushRefused::Malformed(episode));
        };
        let comma = usize::from(!self.body.episodes.is_empty());
        let after = self
            .counted
            .saturating_add(bytes.len())
            .saturating_add(comma);
        if after > ANALYSIS_WRITE_MAX {
            return Err(PushRefused::TooLarge(episode));
        }
        self.counted = after;
        self.body.episodes.push(episode);
        Ok(())
    }

    /// Sets the commit position, the next position of the batch. It never takes the write over its bound.
    pub fn commit(&mut self, position: Position) {
        self.body.commit = Some(position);
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// The key `store`: the store id of the batch the worker read.
    pub fn store(&self) -> &StoreId {
        &self.body.store
    }

    /// The key `episodes`.
    pub fn episodes(&self) -> &[Episode] {
        &self.body.episodes
    }

    /// The key `commit`.
    pub fn commit_position(&self) -> Option<Position> {
        self.body.commit
    }

    /// The serialized request, or `malformed` when it holds a value that its reader refuses, or `too_large`.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ErrorName> {
        bounded(
            envelope(&self.id, "analysis_write", &self.body)?,
            ANALYSIS_WRITE_MAX,
        )
    }
}

/// The body of an interface read: `{"read":"workflows","after":..}` or
/// `{"read":"evidence","workflow":..,"after":..}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "read", rename_all = "snake_case")]
pub enum Read {
    /// Every stored workflow after the workflow id `after`, or from the first.
    Workflows { after: Option<WorkflowId> },
    /// The evidence of one workflow's support set after the summary id `after`, or from the first.
    Evidence {
        workflow: WorkflowId,
        after: Option<SummaryId>,
    },
}

impl<'de> Deserialize<'de> for Read {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "read", rename_all = "snake_case", deny_unknown_fields)]
        enum In {
            Workflows {
                #[serde(deserialize_with = "crate::ids::required")]
                after: Option<WorkflowId>,
            },
            Evidence {
                workflow: WorkflowId,
                #[serde(deserialize_with = "crate::ids::required")]
                after: Option<SummaryId>,
            },
        }
        Ok(match object(deserializer)? {
            In::Workflows { after } => Read::Workflows { after },
            In::Evidence { workflow, after } => Read::Evidence { workflow, after },
        })
    }
}

/// The interface read, a request of the role `interface`.
#[derive(Debug)]
pub struct InterfaceRead {
    id: RequestId,
    read: Read,
}

impl InterfaceRead {
    /// The read `workflows`.
    pub fn workflows(_role: &InterfaceRole, id: RequestId, after: Option<WorkflowId>) -> Self {
        InterfaceRead {
            id,
            read: Read::Workflows { after },
        }
    }

    /// The read `evidence` of one workflow.
    pub fn evidence(
        _role: &InterfaceRole,
        id: RequestId,
        workflow: WorkflowId,
        after: Option<SummaryId>,
    ) -> Self {
        InterfaceRead {
            id,
            read: Read::Evidence { workflow, after },
        }
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// The body: the keys `read`, `workflow` and `after`.
    pub fn read(&self) -> &Read {
        &self.read
    }

    /// The serialized request, or `too_large` above `INTERFACE_REQUEST_MAX` bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ErrorName> {
        bounded(
            envelope(&self.id, "interface_read", &self.read)?,
            INTERFACE_REQUEST_MAX,
        )
    }
}

name_enum! {
    /// The three decisions of the interface.
    pub enum Decision {
        Approve = "approve",
        Dismiss = "dismiss",
        Delete = "delete",
    }
}

strict_struct! {
    struct DecisionBody {
        pub decision: Decision,
        pub workflow: WorkflowId,
    }
    rules(_b) {
        Ok(())
    }
}

/// The interface decision, a request of the role `interface` and the one request that builds an approval, a
/// dismissal or a deletion. It takes a workflow id and nothing else.
#[derive(Debug)]
pub struct InterfaceDecision {
    id: RequestId,
    decision: Decision,
    workflow: WorkflowId,
}

impl InterfaceDecision {
    /// The decision `approve` of the workflow.
    pub fn approve(_role: &InterfaceRole, id: RequestId, workflow: WorkflowId) -> Self {
        InterfaceDecision {
            id,
            decision: Decision::Approve,
            workflow,
        }
    }

    /// The decision `dismiss` of the workflow.
    pub fn dismiss(_role: &InterfaceRole, id: RequestId, workflow: WorkflowId) -> Self {
        InterfaceDecision {
            id,
            decision: Decision::Dismiss,
            workflow,
        }
    }

    /// The decision `delete` of the workflow.
    pub fn delete(_role: &InterfaceRole, id: RequestId, workflow: WorkflowId) -> Self {
        InterfaceDecision {
            id,
            decision: Decision::Delete,
            workflow,
        }
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// The key `decision`.
    pub fn decision(&self) -> Decision {
        self.decision
    }

    /// The key `workflow`.
    pub fn workflow(&self) -> &WorkflowId {
        &self.workflow
    }

    /// The serialized request, or `too_large` above `INTERFACE_REQUEST_MAX` bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ErrorName> {
        let body = DecisionBody {
            decision: self.decision,
            workflow: self.workflow.clone(),
        };
        bounded(
            envelope(&self.id, "interface_decision", &body)?,
            INTERFACE_REQUEST_MAX,
        )
    }
}

/// A runner request, the request of the role `runner`, with a body of the agent-setup run. It has no bound in
/// this run.
#[derive(Debug)]
pub struct RunnerRequest<B: RunnerBody> {
    id: RequestId,
    body: B,
}

impl<B: RunnerBody> RunnerRequest<B> {
    /// The request with its body.
    pub fn new(_role: &RunnerRole, id: RequestId, body: B) -> Self {
        RunnerRequest { id, body }
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// The body.
    pub fn body(&self) -> &B {
        &self.body
    }

    /// The serialized request, or `malformed` when the body cannot be serialized.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ErrorName> {
        envelope(&self.id, "runner", &self.body)
    }
}

/// A coordinator write, the request of the role `coordinator`: every state change but a decision and every
/// configuration change of an agent exists only as one of its bodies. It has no bound in this run.
#[derive(Debug)]
pub struct CoordinatorWrite<B: CoordinatorBody> {
    id: RequestId,
    body: B,
}

impl<B: CoordinatorBody> CoordinatorWrite<B> {
    /// The request with its body.
    pub fn new(_role: &CoordinatorRole, id: RequestId, body: B) -> Self {
        CoordinatorWrite { id, body }
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// The body.
    pub fn body(&self) -> &B {
        &self.body
    }

    /// The serialized request, or `malformed` when the body cannot be serialized.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ErrorName> {
        envelope(&self.id, "coordinator_write", &self.body)
    }
}

fn further_role_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !REQUEST_NAMES.contains(&name)
        && Role::parse(name).is_err()
}

/// A request of a further role `R` that a later run declares, written
/// `{"v":1,"id":..,"request":"<NAME>","body":..}`. It has no bound in this run.
#[derive(Debug)]
pub struct ExtRequest<R: ExtRole, B: ExtBody<R>> {
    id: RequestId,
    body: B,
    role: PhantomData<fn() -> R>,
}

impl<R: ExtRole, B: ExtBody<R>> ExtRequest<R, B> {
    /// The request with its body.
    pub fn new(_role: &R, id: RequestId, body: B) -> Self {
        ExtRequest {
            id,
            body,
            role: PhantomData,
        }
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// The body.
    pub fn body(&self) -> &B {
        &self.body
    }

    /// The serialized request, or `malformed` for a `NAME` outside its form or equal to a request name or a
    /// role name of this run, or a body that cannot be serialized.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ErrorName> {
        if !further_role_name(R::NAME) {
            return Err(ErrorName::Malformed);
        }
        envelope(&self.id, R::NAME, &self.body)
    }
}

/// A request that `decode` returns undecoded: a runner request, a coordinator write or a request of a further
/// role, with its name, its id and its body kept as the bytes it had in the request.
#[derive(Debug)]
pub struct RawRequest {
    name: String,
    id: RequestId,
    body: Box<RawValue>,
}

impl RawRequest {
    /// The request name: `runner`, `coordinator_write` or the name of a further role.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// The body as the sender wrote it.
    pub fn body(&self) -> &RawValue {
        &self.body
    }

    /// The role of the request, or none for a request of a further role.
    pub fn role(&self) -> Option<Role> {
        match self.name.as_str() {
            "runner" => Some(Role::Runner),
            "coordinator_write" => Some(Role::Coordinator),
            _ => None,
        }
    }
}

/// A request that `decode` read.
#[derive(Debug)]
pub enum Decoded {
    AnalysisWrite(AnalysisWrite),
    InterfaceRead(InterfaceRead),
    InterfaceDecision(InterfaceDecision),
    Raw(RawRequest),
}

impl Decoded {
    /// The role that the table of requests gives the request, or none for a request of a further role.
    pub fn role(&self) -> Option<Role> {
        match self {
            Decoded::AnalysisWrite(_) => Some(Role::Analysis),
            Decoded::InterfaceRead(_) | Decoded::InterfaceDecision(_) => Some(Role::Interface),
            Decoded::Raw(raw) => raw.role(),
        }
    }

    /// The request id.
    pub fn id(&self) -> &RequestId {
        match self {
            Decoded::AnalysisWrite(r) => r.id(),
            Decoded::InterfaceRead(r) => r.id(),
            Decoded::InterfaceDecision(r) => r.id(),
            Decoded::Raw(r) => r.id(),
        }
    }
}

/// The failure of a decoder: the error name, with the request id where the decoder read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError {
    pub id: Option<RequestId>,
    pub error: ErrorName,
}

struct Fields<'a>(Vec<(String, &'a RawValue)>);

impl<'de: 'a, 'a> Deserialize<'de> for Fields<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<'a>(PhantomData<&'a ()>);
        impl<'de: 'a, 'a> Visitor<'de> for V<'a> {
            type Value = Fields<'a>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Fields<'a>, A::Error> {
                let mut fields = Vec::new();
                while let Some(entry) = map.next_entry::<String, &'de RawValue>()? {
                    fields.push(entry);
                }
                Ok(Fields(fields))
            }
        }
        deserializer.deserialize_map(V(PhantomData))
    }
}

impl<'a> Fields<'a> {
    fn read(bytes: &'a [u8]) -> Option<Fields<'a>> {
        serde_json::from_slice(bytes).ok()
    }

    fn unique(&self, key: &str) -> Option<&'a RawValue> {
        let mut found = self.0.iter().filter(|(k, _)| k == key);
        match (found.next(), found.next()) {
            (Some((_, value)), None) => Some(value),
            _ => None,
        }
    }

    fn has_exactly(&self, keys: &[&str]) -> bool {
        self.0.len() == keys.len() && keys.iter().all(|k| self.unique(k).is_some())
    }

    fn keys_are(&self, keys: &[&str]) -> bool {
        self.0.iter().all(|(k, _)| keys.contains(&k.as_str()))
            && keys.iter().all(|k| self.0.iter().any(|(key, _)| key == k))
    }
}

fn version(fields: &Fields<'_>) -> Result<(), ErrorName> {
    let v = fields.unique("v").ok_or(ErrorName::Malformed)?.get();
    let digits = v.strip_prefix('-').unwrap_or(v);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ErrorName::Malformed);
    }
    if v != "1" {
        return Err(ErrorName::Version);
    }
    Ok(())
}

fn read_id(fields: &Fields<'_>) -> Option<RequestId> {
    fields
        .unique("id")
        .and_then(|raw| serde_json::from_str::<RequestId>(raw.get()).ok())
}

/// Reads a request, the owner's side and the one path that builds a request without a role token. Checks in
/// this order, and the first that fails names the error: the bytes are a JSON object whose `v` is an integer,
/// else `malformed`; `v` is 1, else `version`; `request` is a string, else `malformed`; the length of the bytes
/// is inside the bound of the request named, else `too_large`; every other rule of the request holds, else
/// `malformed`. A runner request, a coordinator write and a request of a further role are returned undecoded,
/// with no bound. A `request` that is a role name and no request name is `malformed`.
pub fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    let fields = Fields::read(bytes).ok_or(DecodeError {
        id: None,
        error: ErrorName::Malformed,
    })?;
    let id = read_id(&fields);
    let fail = |error| DecodeError {
        id: id.clone(),
        error,
    };
    version(&fields).map_err(fail)?;
    let request = fields
        .unique("request")
        .and_then(|raw| serde_json::from_str::<String>(raw.get()).ok())
        .ok_or_else(|| fail(ErrorName::Malformed))?;
    let bound = match request.as_str() {
        "analysis_write" => Some(ANALYSIS_WRITE_MAX),
        "interface_read" | "interface_decision" => Some(INTERFACE_REQUEST_MAX),
        _ => None,
    };
    if bound.is_some_and(|max| bytes.len() > max) {
        return Err(fail(ErrorName::TooLarge));
    }
    if !fields.has_exactly(&["v", "id", "request", "body"]) {
        return Err(fail(ErrorName::Malformed));
    }
    let (Some(id), Some(body)) = (id.clone(), fields.unique("body")) else {
        return Err(fail(ErrorName::Malformed));
    };
    let malformed = |_| fail(ErrorName::Malformed);
    Ok(match request.as_str() {
        "analysis_write" => {
            let body: AnalysisWriteBody = serde_json::from_str(body.get()).map_err(malformed)?;
            let counted = counted(&id, &body);
            Decoded::AnalysisWrite(AnalysisWrite { id, body, counted })
        }
        "interface_read" => Decoded::InterfaceRead(InterfaceRead {
            id,
            read: serde_json::from_str(body.get()).map_err(malformed)?,
        }),
        "interface_decision" => {
            let body: DecisionBody = serde_json::from_str(body.get()).map_err(malformed)?;
            Decoded::InterfaceDecision(InterfaceDecision {
                id,
                decision: body.decision,
                workflow: body.workflow,
            })
        }
        name if name == "runner" || name == "coordinator_write" || further_role_name(name) => {
            Decoded::Raw(RawRequest {
                name: request,
                id,
                body: body.to_owned(),
            })
        }
        _ => return Err(fail(ErrorName::Malformed)),
    })
}

/// The result of a reply that is no error.
#[derive(Clone, Debug)]
pub enum ReplyResult {
    /// `null`: the result of an analysis write and of a decision.
    Null,
    /// The page of the read `workflows`.
    Workflows(WorkflowsPage),
    /// The page of the read `evidence`.
    Evidence(EvidencePage),
    /// Any other value: the serialized reply of a body's own reply type.
    Raw(Box<RawValue>),
}

impl Serialize for ReplyResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ReplyResult::Null => serializer.serialize_unit(),
            ReplyResult::Workflows(page) => page.serialize(serializer),
            ReplyResult::Evidence(page) => page.serialize(serializer),
            ReplyResult::Raw(raw) => raw.serialize(serializer),
        }
    }
}

fn classify(raw: &RawValue) -> Result<ReplyResult, ErrorName> {
    let text = raw.get();
    if text == "null" {
        return Ok(ReplyResult::Null);
    }
    if text.starts_with('{') {
        let fields = Fields::read(text.as_bytes()).ok_or(ErrorName::Malformed)?;
        if fields.keys_are(&["workflows", "next"]) {
            return serde_json::from_str(text)
                .map(ReplyResult::Workflows)
                .map_err(|_| ErrorName::Malformed);
        }
        if fields.keys_are(&["evidence", "next"]) {
            return serde_json::from_str(text)
                .map(ReplyResult::Evidence)
                .map_err(|_| ErrorName::Malformed);
        }
    }
    Ok(ReplyResult::Raw(raw.to_owned()))
}

/// A reply that `decode_reply` read: the request id, or none where the request's id could not be read, and the
/// result or the error name.
#[derive(Clone, Debug)]
pub struct Reply {
    pub id: Option<RequestId>,
    pub outcome: Result<ReplyResult, ErrorName>,
}

/// Reads a reply: checks `v` as `decode` does, then that the keys are `v`, `id` and one of `ok` and `error`,
/// each once; the id is null or a request id; an error name is one of the nine or a name that a file of a later
/// run declares (`ErrorName::read`), and any other name is `malformed`. A result is the null
/// result when it is `null`, a workflows page when it is an object whose keys are exactly `workflows` and
/// `next`, an evidence page when they are exactly `evidence` and `next`, and else the raw value; a page that
/// breaks a rule of its type is `malformed`. Before it reads anything it refuses an input longer than
/// `REPLY_MAX` bytes as given, white space included, with `too_large` and a null id.
pub fn decode_reply(bytes: &[u8]) -> Result<Reply, DecodeError> {
    if bytes.len() > REPLY_MAX {
        return Err(DecodeError {
            id: None,
            error: ErrorName::TooLarge,
        });
    }
    let fields = Fields::read(bytes).ok_or(DecodeError {
        id: None,
        error: ErrorName::Malformed,
    })?;
    let id = read_id(&fields);
    let fail = |error| DecodeError {
        id: id.clone(),
        error,
    };
    version(&fields).map_err(fail)?;
    let id_is_null = fields.unique("id").is_some_and(|raw| raw.get() == "null");
    if id.is_none() && !id_is_null {
        return Err(fail(ErrorName::Malformed));
    }
    let outcome = if fields.has_exactly(&["v", "id", "ok"]) {
        let ok = fields
            .unique("ok")
            .ok_or_else(|| fail(ErrorName::Malformed))?;
        Ok(classify(ok).map_err(fail)?)
    } else if fields.has_exactly(&["v", "id", "error"]) {
        let error = fields
            .unique("error")
            .and_then(|raw| serde_json::from_str::<String>(raw.get()).ok())
            .and_then(|name| ErrorName::read(&name).ok())
            .ok_or_else(|| fail(ErrorName::Malformed))?;
        Err(error)
    } else {
        return Err(fail(ErrorName::Malformed));
    };
    Ok(Reply { id, outcome })
}

/// The one writer of a reply. It takes the request id, or none when the request's id could not be read, and
/// either a result or an error name, and returns `{"v":1,"id":..,"ok":..}` or `{"v":1,"id":..,"error":..}`.
/// It writes an error name of the nine and a declared name alike, and `ErrorName` holds no other name.
/// It refuses with `malformed` a result that breaks a rule of `decode_reply` on the result, and applies no length
/// bound: it writes a reply longer than `REPLY_MAX`, which `decode_reply` refuses with `too_large`, so the owner
/// keeps a page inside 261,120 bytes.
pub fn write_reply(
    id: Option<&RequestId>,
    outcome: Result<&ReplyResult, ErrorName>,
) -> Result<Vec<u8>, ErrorName> {
    #[derive(Serialize)]
    struct OkReply<'a> {
        v: u32,
        id: Option<&'a RequestId>,
        ok: &'a ReplyResult,
    }
    #[derive(Serialize)]
    struct ErrorReply<'a> {
        v: u32,
        id: Option<&'a RequestId>,
        error: ErrorName,
    }
    match outcome {
        Ok(result) => {
            if let ReplyResult::Raw(raw) = result {
                classify(raw)?;
            }
            serde_json::to_vec(&OkReply {
                v: VERSION,
                id,
                ok: result,
            })
            .map_err(|_| ErrorName::Malformed)
        }
        Err(error) => serde_json::to_vec(&ErrorReply {
            v: VERSION,
            id,
            error,
        })
        .map_err(|_| ErrorName::Malformed),
    }
}

macro_rules! decision_case {
    ($fence:literal, $role:literal, $verb:literal) => {
        concat!(
            "```",
            $fence,
            "\n",
            "use halvo_schema_core::request::{InterfaceDecision, ",
            $role,
            "};\n",
            "use halvo_schema_core::{RequestId, WorkflowId};\n",
            "let role = ",
            $role,
            "::claim();\n",
            "let id = RequestId::from_random([1; 16]);\n",
            "let workflow = WorkflowId::from_random([2; 16]);\n",
            "let decision = InterfaceDecision::",
            $verb,
            "(&role, id, workflow);\n",
            "assert!(decision.to_bytes().is_ok());\n",
            "```"
        )
    };
}

macro_rules! coordinator_case {
    ($fence:literal, $role:literal) => {
        concat!(
            "```",
            $fence,
            "\n",
            "use halvo_schema_core::request::{CoordinatorBody, CoordinatorWrite, ",
            $role,
            "};\n",
            "use halvo_schema_core::RequestId;\n",
            "#[derive(serde::Serialize, serde::Deserialize)]\n",
            "struct Probe {\n",
            "    probe: u32,\n",
            "}\n",
            "impl CoordinatorBody for Probe {\n",
            "    type Reply = ();\n",
            "}\n",
            "let role = ",
            $role,
            "::claim();\n",
            "let write = CoordinatorWrite::new(&role, RequestId::from_random([1; 16]), Probe { probe: 1 });\n",
            "assert!(write.to_bytes().is_ok());\n",
            "```"
        )
    };
}

macro_rules! analysis_conversion_case {
    ($fence:literal, $convert:literal) => {
        concat!(
            "```",
            $fence,
            "\n",
            "use halvo_schema_core::request::{AnalysisRole, AnalysisWrite, InterfaceDecision};\n",
            "use halvo_schema_core::{RequestId, StoreId};\n",
            "let role = AnalysisRole::claim();\n",
            "let store = StoreId::parse(\"0123456789abcdef0123456789abcdef\").unwrap();\n",
            "let write = AnalysisWrite::new(&role, RequestId::from_random([1; 16]), store);\n",
            "let built = ",
            $convert,
            "(write);\n",
            "assert!(built.to_bytes().is_ok());\n",
            "```"
        )
    };
}

macro_rules! runner_conversion_case {
    ($fence:literal, $convert:literal) => {
        concat!(
            "```",
            $fence,
            "\n",
            "use halvo_schema_core::request::{InterfaceDecision, RunnerBody, RunnerRequest, RunnerRole};\n",
            "use halvo_schema_core::RequestId;\n",
            "#[derive(serde::Serialize, serde::Deserialize)]\n",
            "struct Probe {\n",
            "    probe: u32,\n",
            "}\n",
            "impl RunnerBody for Probe {\n",
            "    type Reply = ();\n",
            "}\n",
            "let role = RunnerRole::claim();\n",
            "let request = RunnerRequest::new(&role, RequestId::from_random([1; 16]), Probe { probe: 1 });\n",
            "let built = ",
            $convert,
            "(request);\n",
            "assert!(built.to_bytes().is_ok());\n",
            "```"
        )
    };
}

macro_rules! interface_role_case {
    ($fence:literal, $role:literal) => {
        concat!(
            "```",
            $fence,
            "\n",
            "use halvo_schema_core::request::{AnalysisRole, InterfaceDecision, InterfaceRole, RunnerRole};\n",
            "use halvo_schema_core::{RequestId, WorkflowId};\n",
            "let role: InterfaceRole = ",
            $role,
            ";\n",
            "let id = RequestId::from_random([1; 16]);\n",
            "let workflow = WorkflowId::from_random([2; 16]);\n",
            "let decision = InterfaceDecision::approve(&role, id, workflow);\n",
            "assert!(decision.to_bytes().is_ok());\n",
            "```"
        )
    };
}

macro_rules! coordinator_role_case {
    ($fence:literal, $role:literal) => {
        concat!(
            "```",
            $fence,
            "\n",
            "use halvo_schema_core::request::{AnalysisRole, CoordinatorBody, CoordinatorRole, CoordinatorWrite, RunnerRole};\n",
            "use halvo_schema_core::RequestId;\n",
            "#[derive(serde::Serialize, serde::Deserialize)]\n",
            "struct Probe {\n",
            "    probe: u32,\n",
            "}\n",
            "impl CoordinatorBody for Probe {\n",
            "    type Reply = ();\n",
            "}\n",
            "let role: CoordinatorRole = ",
            $role,
            ";\n",
            "let write = CoordinatorWrite::new(&role, RequestId::from_random([1; 16]), Probe { probe: 1 });\n",
            "assert!(write.to_bytes().is_ok());\n",
            "```"
        )
    };
}

/// What must not compile: an analysis or a runner value builds no approval, dismissal or deletion, no coordinator
/// write (which stands for every other state change and every configuration change), no decision by conversion
/// and no interface or coordinator token by conversion. Each case differs from its twin in `compiles` in the role value alone or
/// in the one conversion call.
pub mod compile_fail {
    /// `InterfaceDecision::approve` does not take `&AnalysisRole`. Twin: `compiles::interface_approve`.
    #[doc = decision_case!("compile_fail", "AnalysisRole", "approve")]
    pub mod analysis_approve {}

    /// `InterfaceDecision::dismiss` does not take `&AnalysisRole`. Twin: `compiles::interface_dismiss`.
    #[doc = decision_case!("compile_fail", "AnalysisRole", "dismiss")]
    pub mod analysis_dismiss {}

    /// `InterfaceDecision::delete` does not take `&AnalysisRole`. Twin: `compiles::interface_delete`.
    #[doc = decision_case!("compile_fail", "AnalysisRole", "delete")]
    pub mod analysis_delete {}

    /// `InterfaceDecision::approve` does not take `&RunnerRole`. Twin: `compiles::interface_approve`.
    #[doc = decision_case!("compile_fail", "RunnerRole", "approve")]
    pub mod runner_approve {}

    /// `InterfaceDecision::dismiss` does not take `&RunnerRole`. Twin: `compiles::interface_dismiss`.
    #[doc = decision_case!("compile_fail", "RunnerRole", "dismiss")]
    pub mod runner_dismiss {}

    /// `InterfaceDecision::delete` does not take `&RunnerRole`. Twin: `compiles::interface_delete`.
    #[doc = decision_case!("compile_fail", "RunnerRole", "delete")]
    pub mod runner_delete {}

    /// `CoordinatorWrite::new` does not take `&AnalysisRole`. Twin: `compiles::coordinator_write`.
    #[doc = coordinator_case!("compile_fail", "AnalysisRole")]
    pub mod analysis_coordinator_write {}

    /// `CoordinatorWrite::new` does not take `&RunnerRole`. Twin: `compiles::coordinator_write`.
    #[doc = coordinator_case!("compile_fail", "RunnerRole")]
    pub mod runner_coordinator_write {}

    /// No `InterfaceDecision::from` of an `AnalysisWrite`. Twin: `compiles::analysis_write_builds`.
    #[doc = analysis_conversion_case!("compile_fail", "InterfaceDecision::from")]
    pub mod analysis_into_decision {}

    /// No `InterfaceDecision::from` of a `RunnerRequest`. Twin: `compiles::runner_request_builds`.
    #[doc = runner_conversion_case!("compile_fail", "InterfaceDecision::from")]
    pub mod runner_into_decision {}

    /// No `InterfaceRole` converted from an `AnalysisRole`. Twin: `compiles::interface_role_claims`.
    #[doc = interface_role_case!("compile_fail", "AnalysisRole::claim().into()")]
    pub mod analysis_role_into_interface {}

    /// No `InterfaceRole` converted from a `RunnerRole`. Twin: `compiles::interface_role_claims`.
    #[doc = interface_role_case!("compile_fail", "RunnerRole::claim().into()")]
    pub mod runner_role_into_interface {}

    /// No `CoordinatorRole` converted from an `AnalysisRole`. Twin: `compiles::coordinator_role_claims`.
    #[doc = coordinator_role_case!("compile_fail", "AnalysisRole::claim().into()")]
    pub mod analysis_role_into_coordinator {}

    /// No `CoordinatorRole` converted from a `RunnerRole`. Twin: `compiles::coordinator_role_claims`.
    #[doc = coordinator_role_case!("compile_fail", "RunnerRole::claim().into()")]
    pub mod runner_role_into_coordinator {}
}

/// The twins of the cases of `compile_fail`, which compile and run.
pub mod compiles {
    /// An interface value builds an approval.
    #[doc = decision_case!("", "InterfaceRole", "approve")]
    pub mod interface_approve {}

    /// An interface value builds a dismissal.
    #[doc = decision_case!("", "InterfaceRole", "dismiss")]
    pub mod interface_dismiss {}

    /// An interface value builds a deletion.
    #[doc = decision_case!("", "InterfaceRole", "delete")]
    pub mod interface_delete {}

    /// A coordinator value builds a coordinator write.
    #[doc = coordinator_case!("", "CoordinatorRole")]
    pub mod coordinator_write {}

    /// An analysis value builds an analysis write.
    #[doc = analysis_conversion_case!("", "std::convert::identity")]
    pub mod analysis_write_builds {}

    /// A runner value builds a runner request.
    #[doc = runner_conversion_case!("", "std::convert::identity")]
    pub mod runner_request_builds {}

    /// An interface value claimed as such builds an approval.
    #[doc = interface_role_case!("", "InterfaceRole::claim()")]
    pub mod interface_role_claims {}

    /// A coordinator value claimed as such builds a coordinator write.
    #[doc = coordinator_role_case!("", "CoordinatorRole::claim()")]
    pub mod coordinator_role_claims {}
}
