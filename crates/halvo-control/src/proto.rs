use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::model::{
    one_spelling, CaptureState, Inventory, InventoryError, InventoryRow, Modality, StorageCap,
    StoreId, UnknownSpelling, UtcSecond,
};
use crate::onboarding::{GrantState, OnboardingState, Permission, RequestOutcome};

pub const PROTOCOL_VERSION: u64 = 1;

/// The ops of the request table of protocol version 1, in the order of contracts.md C2.1.
pub const V1_OPS: [&str; 10] = [
    "status",
    "inventory",
    "pause",
    "resume",
    "stop",
    "delete_all",
    "set_storage_cap",
    "request_permission",
    "permission_asked",
    "acknowledge_lost_permission",
];

/// The run marks, request names that a later version may use. Protocol version 1 answers
/// each with `unsupported_op`, as it answers every op that [`V1_OPS`] does not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReservedOp {
    RunStart,
    RunEnd,
}

one_spelling!(ReservedOp, "reserved op", [RunStart => "run_start", RunEnd => "run_end"]);

pub const RESERVED_OPS: [&str; 2] = [ReservedOp::RunStart.as_str(), ReservedOp::RunEnd.as_str()];

fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Status {
        indicator: bool,
    },
    Inventory,
    Pause,
    Resume,
    Stop,
    DeleteAll {
        #[serde(deserialize_with = "required")]
        store: Option<StoreId>,
    },
    SetStorageCap {
        bytes: StorageCap,
    },
    RequestPermission {
        permission: Permission,
    },
    PermissionAsked {
        permission: Permission,
    },
    AcknowledgeLostPermission {
        permission: Permission,
    },
}

impl Request {
    pub const fn op(&self) -> &'static str {
        match self {
            Request::Status { .. } => "status",
            Request::Inventory => "inventory",
            Request::Pause => "pause",
            Request::Resume => "resume",
            Request::Stop => "stop",
            Request::DeleteAll { .. } => "delete_all",
            Request::SetStorageCap { .. } => "set_storage_cap",
            Request::RequestPermission { .. } => "request_permission",
            Request::PermissionAsked { .. } => "permission_asked",
            Request::AcknowledgeLostPermission { .. } => "acknowledge_lost_permission",
        }
    }

    /// Whether the request changes the daemon or its files; one mutation runs at a time.
    pub const fn is_mutation(&self) -> bool {
        !matches!(self, Request::Status { .. } | Request::Inventory)
    }

    pub const fn claims_the_indicator(&self) -> bool {
        matches!(self, Request::Status { indicator: true })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    Known(Request),
    Unknown(UnknownRequest),
}

/// A request that the v1 table does not hold: a reserved name, an op that no version
/// knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRequest {
    version: u64,
    body: serde_json::Map<String, serde_json::Value>,
}

impl UnknownRequest {
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn op(&self) -> Option<&str> {
        self.body.get("op").and_then(serde_json::Value::as_str)
    }

    pub fn body(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.body
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    #[error("the request is not an envelope with a version and a body")]
    NotAnEnvelope,
    #[error("protocol version {version} is not served")]
    UnsupportedVersion { version: u64 },
    #[error("the request does not fit its op")]
    BadBody,
}

impl RequestError {
    pub fn code(&self) -> ErrorCode {
        match self {
            RequestError::NotAnEnvelope | RequestError::BadBody => ErrorCode::InvalidArgument,
            RequestError::UnsupportedVersion { .. } => ErrorCode::UnsupportedVersion,
        }
    }

    pub fn version(&self) -> Option<u64> {
        match self {
            RequestError::UnsupportedVersion { version } => Some(*version),
            _ => None,
        }
    }

    /// The reply that the server sends for the refused request.
    pub fn answer(&self) -> Reply {
        Reply::Error(ErrorReply {
            code: self.code(),
            message: self.to_string(),
        })
    }
}

/// The reply to a request that the v1 table does not hold.
pub fn unknown_answer(unknown: &UnknownRequest) -> Reply {
    if unknown.version == PROTOCOL_VERSION {
        Reply::Error(ErrorReply {
            code: ErrorCode::UnsupportedOp,
            message: "the op is not in the request table of protocol version 1".into(),
        })
    } else {
        RequestError::UnsupportedVersion {
            version: unknown.version,
        }
        .answer()
    }
}

#[derive(Serialize)]
struct Outgoing<'a, T> {
    v: u64,
    body: &'a T,
}

fn envelope<T: Serialize>(body: &T) -> Vec<u8> {
    serde_json::to_vec(&Outgoing {
        v: PROTOCOL_VERSION,
        body,
    })
    .expect("a value of these types always serializes")
}

pub fn encode_request(request: &Request) -> Vec<u8> {
    envelope(request)
}

pub fn encode_reply(reply: &Reply) -> Vec<u8> {
    envelope(reply)
}

type Object = serde_json::Map<String, serde_json::Value>;

struct Unique(serde_json::Value);

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visit;

        impl<'de> serde::de::Visitor<'de> for Visit {
            type Value = Unique;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value whose objects hold each key once")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }

            fn visit_str<E>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }

            fn visit_unit<E>(self) -> Result<Unique, E> {
                Ok(Unique(serde_json::Value::Null))
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Unique, A::Error> {
                let mut items = Vec::new();
                while let Some(Unique(item)) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Unique(serde_json::Value::Array(items)))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Unique, A::Error> {
                let mut object = Object::new();
                while let Some((key, Unique(value))) = map.next_entry::<String, Unique>()? {
                    if object.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("a key occurs twice"));
                    }
                }
                Ok(Unique(serde_json::Value::Object(object)))
            }
        }

        deserializer.deserialize_any(Visit)
    }
}

/// The most bytes that one call of [`decode_request`] holds live at once for a frame of at most
/// [`crate::wire::REQUEST_MAX_BYTES`]: 128 for each byte. The call parses the whole frame into
/// a tree before it reads the shape; the worst frame that the test of wire.rs measures, a pause
/// with one field too many that holds objects of the empty key nested 101 deep, 8 side by side,
/// holds 518,676.
pub const REQUEST_DECODE_PEAK_BYTES: usize = 524_288;

/// The most bytes that one call of [`decode_reply`] holds live at once for a frame of at most
/// [`crate::wire::REPLY_MAX_BYTES`]: 128 for each byte. The call keeps one tree of the body.
/// The worst frame that the test of wire.rs measures, an error reply with one field that the
/// build does not know that holds objects of the empty key nested 100 deep, 522 side by side,
/// holds 33,047,177.
pub const REPLY_DECODE_PEAK_BYTES: usize = 33_554_432;

/// The version of a frame that is one JSON object whose key `v` holds an integer from 0 to
/// 18446744073709551615, and its body when the object holds `v` and an object `body` alone.
fn read_envelope(frame: &[u8]) -> Option<(u64, Option<Object>)> {
    if frame.first() != Some(&b'{') || frame.last() != Some(&b'}') {
        return None;
    }
    let Unique(serde_json::Value::Object(mut object)) = serde_json::from_slice(frame).ok()? else {
        return None;
    };
    let version = object.get("v").and_then(serde_json::Value::as_u64)?;
    let body = match (object.len(), object.remove("body")) {
        (2, Some(serde_json::Value::Object(body))) => Some(body),
        _ => None,
    };
    Some((version, body))
}

/// A version other than [`PROTOCOL_VERSION`] is
/// `UnsupportedVersion`; an op outside [`V1_OPS`] is [`Decoded::Unknown`]; a body that does
/// not fit its op, also one with a field the op does not have, is `BadBody`.
pub fn decode_request(frame: &[u8]) -> Result<Decoded, RequestError> {
    let (version, body) = read_envelope(frame).ok_or(RequestError::NotAnEnvelope)?;
    #[cfg(feature = "r2")]
    {
        if crate::r2::takes_version(version) {
            let body = body.ok_or(RequestError::NotAnEnvelope)?;
            return Ok(Decoded::Unknown(UnknownRequest { version, body }));
        }
    }
    if version != PROTOCOL_VERSION {
        return Err(RequestError::UnsupportedVersion { version });
    }
    let body = body.ok_or(RequestError::NotAnEnvelope)?;
    let op = body
        .get("op")
        .and_then(serde_json::Value::as_str)
        .ok_or(RequestError::BadBody)?;
    if !V1_OPS.contains(&op) {
        return Ok(Decoded::Unknown(UnknownRequest { version, body }));
    }
    let keys: std::collections::BTreeSet<String> = body.keys().cloned().collect();
    let request: Request = serde_json::from_value(serde_json::Value::Object(body))
        .map_err(|_| RequestError::BadBody)?;
    match serde_json::to_value(&request) {
        Ok(serde_json::Value::Object(written))
            if written
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                == keys =>
        {
            Ok(Decoded::Known(request))
        }
        _ => Err(RequestError::BadBody),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplyError {
    #[error("the reply is not an envelope with a version and a body")]
    NotAnEnvelope,
    #[error("the reply is of protocol version {0}, not 1")]
    Version(u64),
    #[error("the reply does not fit protocol version 1: {0}")]
    Body(String),
    /// The reply carries a status whose fields read as a status and break a rule of
    /// [`DaemonStatus`].
    #[error("{0}")]
    Status(StatusError),
}

/// The rule that the status of a reply body breaks, when the body is a status, a
/// capture_changed or a cap_set whose status has every field of a status. It reads the body
/// in place and copies no part of it.
fn broken_status_rule(body: &serde_json::Value) -> Option<StatusError> {
    let status = match body.get("kind").and_then(serde_json::Value::as_str)? {
        "status" | "capture_changed" => body,
        "cap_set" => body.get("status")?,
        _ => return None,
    };
    let fields = StatusFields::deserialize(status).ok()?;
    DaemonStatus::new(fields).err()
}

/// A field that this build does not know is skipped.
pub fn decode_reply(frame: &[u8]) -> Result<Reply, ReplyError> {
    let (version, body) = read_envelope(frame).ok_or(ReplyError::NotAnEnvelope)?;
    if version != PROTOCOL_VERSION {
        return Err(ReplyError::Version(version));
    }
    let body = serde_json::Value::Object(body.ok_or(ReplyError::NotAnEnvelope)?);
    let broken = broken_status_rule(&body);
    serde_json::from_value(body).map_err(|e| match broken {
        Some(broken) => ReplyError::Status(broken),
        None => ReplyError::Body(e.to_string()),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Status(DaemonStatus),
    Inventory(InventoryReply),
    CaptureChanged(DaemonStatus),
    Stopped(Stopped),
    Deleted(Deleted),
    AlreadyDeleted(AlreadyDeleted),
    /// The delete removed nothing; the reply carries no store id (ruling PL-8 part (4)(c)).
    DeleteRefused,
    CapSet(CapSet),
    PermissionRequested(PermissionRequested),
    Error(ErrorReply),
}

impl Reply {
    pub const fn kind(&self) -> &'static str {
        match self {
            Reply::Status(_) => "status",
            Reply::Inventory(_) => "inventory",
            Reply::CaptureChanged(_) => "capture_changed",
            Reply::Stopped(_) => "stopped",
            Reply::Deleted(_) => "deleted",
            Reply::AlreadyDeleted(_) => "already_deleted",
            Reply::DeleteRefused => "delete_refused",
            Reply::CapSet(_) => "cap_set",
            Reply::PermissionRequested(_) => "permission_requested",
            Reply::Error(_) => "error",
        }
    }

    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            Reply::Error(error) => Some(error.code),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    UnsupportedVersion,
    UnsupportedOp,
    Busy,
    StoreUnavailable,
    InvalidArgument,
    PeerRejected,
    StateFile,
    Internal,
}

one_spelling!(ErrorCode, "error code", [
    UnsupportedVersion => "unsupported_version",
    UnsupportedOp => "unsupported_op",
    Busy => "busy",
    StoreUnavailable => "store_unavailable",
    InvalidArgument => "invalid_argument",
    PeerRejected => "peer_rejected",
    StateFile => "state_file",
    Internal => "internal",
]);

/// The message is a fixed text of the server and holds no captured content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReply {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stopped {
    pub flushed: bool,
    pub events_lost: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deleted {
    pub report: DeleteReport,
    #[serde(deserialize_with = "required")]
    pub store: Option<StoreId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlreadyDeleted {
    pub current: StoreId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapSet {
    pub status: DaemonStatus,
    pub evicting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequested {
    pub outcome: RequestOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RootDestroyed {
    Yes,
    No,
    Unknown,
}

one_spelling!(RootDestroyed, "root result", [Yes => "yes", No => "no", Unknown => "unknown"]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntentState {
    Written,
    Failed,
}

one_spelling!(IntentState, "intent result", [Written => "written", Failed => "failed"]);

/// The fields of a delete report, as the wire and the `deleted` line carry them; `None`
/// is `unknown` for the two counts and `-` for the root id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteFields {
    #[serde(deserialize_with = "required")]
    pub segments_shredded: Option<u64>,
    #[serde(deserialize_with = "required")]
    pub keys_destroyed: Option<u64>,
    pub root_destroyed: RootDestroyed,
    #[serde(deserialize_with = "required")]
    pub root_id: Option<StoreId>,
    pub files_left: u64,
    pub database_left: bool,
    /// The store names, the store directory's own among them, at which the delete met a link,
    /// an entry of another kind, or a directory that was not its own (ruling ST-30 (2)(e)).
    pub not_followed: u64,
    pub entries_removed: u64,
    pub entries_left: u64,
    pub unknown_entries: u64,
    pub intent: IntentState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeleteReportError {
    #[error("keys_destroyed is segments_shredded plus 1, and both are known or both unknown")]
    Counts,
    #[error("root_id names the surviving root when root_destroyed is no, and only then")]
    Root,
}

/// A delete report whose counts agree: `keys_destroyed` is `segments_shredded` plus 1, and
/// `root_id` is present exactly when the root survived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DeleteFields", into = "DeleteFields")]
pub struct DeleteReport(DeleteFields);

impl DeleteReport {
    pub fn new(fields: DeleteFields) -> Result<Self, DeleteReportError> {
        match (fields.segments_shredded, fields.keys_destroyed) {
            (None, None) => {}
            (Some(segments), Some(keys)) if segments.checked_add(1) == Some(keys) => {}
            _ => return Err(DeleteReportError::Counts),
        }
        if (fields.root_destroyed == RootDestroyed::No) != fields.root_id.is_some() {
            return Err(DeleteReportError::Root);
        }
        Ok(Self(fields))
    }

    pub fn fields(&self) -> &DeleteFields {
        &self.0
    }
}

impl TryFrom<DeleteFields> for DeleteReport {
    type Error = DeleteReportError;

    fn try_from(fields: DeleteFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<DeleteReport> for DeleteFields {
    fn from(report: DeleteReport) -> Self {
        report.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub events: u64,
    pub bytes: u64,
    pub disk_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InventoryWire {
    store: StoreId,
    as_of: UtcSecond,
    rows: Vec<InventoryRow>,
    totals: Totals,
    ext: Vec<Section>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InventoryReplyError {
    #[error(transparent)]
    Rows(#[from] InventoryError),
    #[error("the totals of events and bytes are not the sums of the rows")]
    Totals,
}

/// The inventory reply: per-day rows of the rollup, in day and kind order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "InventoryWire", into = "InventoryWire")]
pub struct InventoryReply {
    store: StoreId,
    as_of: UtcSecond,
    rows: Inventory,
    totals: Totals,
    ext: Vec<Section>,
}

impl InventoryReply {
    pub fn new(
        store: StoreId,
        as_of: UtcSecond,
        rows: Vec<InventoryRow>,
        totals: Totals,
        ext: Vec<Section>,
    ) -> Result<Self, InventoryReplyError> {
        let rows = Inventory::from_rows(rows)?;
        if rows.total_events() != totals.events || rows.total_bytes() != totals.bytes {
            return Err(InventoryReplyError::Totals);
        }
        Ok(Self {
            store,
            as_of,
            rows,
            totals,
            ext,
        })
    }

    pub fn store(&self) -> StoreId {
        self.store
    }

    pub fn as_of(&self) -> UtcSecond {
        self.as_of
    }

    pub fn rows(&self) -> &Inventory {
        &self.rows
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    pub fn ext(&self) -> &[Section] {
        &self.ext
    }
}

impl TryFrom<InventoryWire> for InventoryReply {
    type Error = InventoryReplyError;

    fn try_from(wire: InventoryWire) -> Result<Self, Self::Error> {
        Self::new(wire.store, wire.as_of, wire.rows, wire.totals, wire.ext)
    }
}

impl From<InventoryReply> for InventoryWire {
    fn from(reply: InventoryReply) -> Self {
        Self {
            store: reply.store,
            as_of: reply.as_of,
            rows: reply.rows.rows().to_vec(),
            totals: reply.totals,
            ext: reply.ext,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a snake_case name")]
pub struct NameError;

fn is_token(text: &str, max: usize) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

macro_rules! token {
    ($name:ident, $max:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl FromStr for $name {
            type Err = NameError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                if is_token(text, $max) {
                    Ok(Self(text.to_string()))
                } else {
                    Err(NameError)
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                crate::model::from_text(deserializer)
            }
        }
    };
}

token!(SectionName, 64);
token!(Token, 32);

/// One row of an extension section: a label and a text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionRow {
    pub k: String,
    pub v: String,
}

/// An extension section of a status or an inventory reply. R1 sends none; a client keeps
/// every section it does not know as its name and rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub name: SectionName,
    pub rows: Vec<SectionRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DaemonVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl fmt::Display for DaemonVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for DaemonVersion {
    type Err = UnknownSpelling;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = UnknownSpelling { what: "version" };
        let mut parts = text.split('.').map(|part| {
            if !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
            {
                part.parse::<u32>().map_err(|_| error)
            } else {
                Err(error)
            }
        });
        let version = DaemonVersion {
            major: parts.next().ok_or(error)??,
            minor: parts.next().ok_or(error)??,
            patch: parts.next().ok_or(error)??,
        };
        if parts.next().is_some() {
            return Err(error);
        }
        Ok(version)
    }
}

impl Serialize for DaemonVersion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DaemonVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::model::from_text(deserializer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Macos,
    Windows,
}

one_spelling!(Platform, "platform", [Macos => "macos", Windows => "windows"]);

impl Platform {
    pub const fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Macos
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Parent {
    Watched,
    None,
}

one_spelling!(Parent, "parent watch", [Watched => "watched", None => "none"]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capture {
    Starting,
    Running,
    Partial,
    Pausing,
    Paused,
    NoIndicator,
    AwaitingResume,
    Faulted,
}

one_spelling!(Capture, "capture state", [
    Starting => "starting",
    Running => "running",
    Partial => "partial",
    Pausing => "pausing",
    Paused => "paused",
    NoIndicator => "no_indicator",
    AwaitingResume => "awaiting_resume",
    Faulted => "faulted",
]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureReason {
    Store,
    Drain,
    NoSource,
    Permission,
}

one_spelling!(CaptureReason, "capture reason", [
    Store => "store",
    Drain => "drain",
    NoSource => "no_source",
    Permission => "permission",
]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lease {
    Fresh,
    Lapsed,
    None,
}

one_spelling!(Lease, "lease", [Fresh => "fresh", Lapsed => "lapsed", None => "none"]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreState {
    Opening,
    Open,
    Deleting,
    Sealed,
    Unavailable,
    NoReleaseCustody,
    UnsupportedVersion,
}

one_spelling!(StoreState, "store state", [
    Opening => "opening",
    Open => "open",
    Deleting => "deleting",
    Sealed => "sealed",
    Unavailable => "unavailable",
    NoReleaseCustody => "no_release_custody",
    UnsupportedVersion => "unsupported_version",
]);

/// The causes of a store that is not open, one closed list (contracts.md C11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreErrorKind {
    Locked,
    Io,
    NoSpace,
    CustodyLocked,
    RootDenied,
    /// At one of the store's names stands something that is not the store's own entry, or the
    /// store directory does not stand at its name (ruling ST-30 part (2)(a)).
    Obstructed,
    RootMissing,
    RootCorrupt,
    WrapAuth,
    KeyFile,
    CustodyKind,
    Database,
    Older,
    Newer,
}

one_spelling!(StoreErrorKind, "store error", [
    Locked => "locked",
    Io => "io",
    NoSpace => "no_space",
    CustodyLocked => "custody_locked",
    RootDenied => "root_denied",
    Obstructed => "obstructed",
    RootMissing => "root_missing",
    RootCorrupt => "root_corrupt",
    WrapAuth => "wrap_auth",
    KeyFile => "key_file",
    CustodyKind => "custody_kind",
    Database => "database",
    Older => "older",
    Newer => "newer",
]);

impl StoreState {
    /// Whether the table of contracts.md C11.3 pairs this state with `cause`.
    pub fn takes(self, cause: Option<StoreErrorKind>) -> bool {
        use StoreErrorKind::*;
        match self {
            StoreState::Opening
            | StoreState::Open
            | StoreState::Deleting
            | StoreState::NoReleaseCustody => cause.is_none(),
            StoreState::Unavailable => matches!(
                cause,
                Some(Locked | Io | NoSpace | CustodyLocked | RootDenied | Obstructed)
            ),
            StoreState::Sealed => matches!(
                cause,
                Some(RootMissing | RootCorrupt | WrapAuth | KeyFile | CustodyKind | Database)
            ),
            StoreState::UnsupportedVersion => matches!(cause, Some(Older | Newer)),
        }
    }
}

/// The storage cap as Status reports it: known bytes, or unknown when the stored cap is
/// out of range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Known(StorageCap),
    Unknown,
}

impl Serialize for Cap {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Cap::Known(cap) => serializer.serialize_u64(cap.bytes()),
            Cap::Unknown => serializer.serialize_str("unknown"),
        }
    }
}

impl<'de> Deserialize<'de> for Cap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Bytes(u64),
            Text(String),
        }
        match Either::deserialize(deserializer)? {
            Either::Bytes(bytes) => StorageCap::from_bytes(bytes)
                .map(Cap::Known)
                .map_err(serde::de::Error::custom),
            Either::Text(text) if text == "unknown" => Ok(Cap::Unknown),
            Either::Text(_) => Err(serde::de::Error::custom("a cap is bytes or unknown")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Health {
    Healthy,
    Failing,
}

one_spelling!(Health, "health", [Healthy => "healthy", Failing => "failing"]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DrainError {
    NoSpace,
    Io,
    Db,
    Rejected,
}

one_spelling!(DrainError, "drain error", [
    NoSpace => "no_space",
    Io => "io",
    Db => "db",
    Rejected => "rejected",
]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpiryError {
    Io,
    NoSpace,
    Db,
}

one_spelling!(ExpiryError, "expiry error", [Io => "io", NoSpace => "no_space", Db => "db"]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LastStop {
    None,
    Clean,
    Unclean,
    SourceNotStopped,
}

one_spelling!(LastStop, "last stop", [
    None => "none",
    Clean => "clean",
    Unclean => "unclean",
    SourceNotStopped => "source_not_stopped",
]);

/// The trigger of the stop that ended the last life (contracts.md C2.3, last_stop_kind).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StopKind {
    Pause,
    Lapse,
    DeleteAll,
    GrantLost,
    Signal,
    Stop,
    Lifeline,
    SessionEnd,
    GrantAppeared,
}

one_spelling!(StopKind, "stop kind", [
    Pause => "pause",
    Lapse => "lapse",
    DeleteAll => "delete_all",
    GrantLost => "grant_lost",
    Signal => "signal",
    Stop => "stop",
    Lifeline => "lifeline",
    SessionEnd => "session_end",
    GrantAppeared => "grant_appeared",
]);

/// What the last life received and did not store. With `final_figures` false the count is an
/// upper bound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LostRecord {
    pub events: u64,
    #[serde(deserialize_with = "required")]
    pub first: Option<UtcSecond>,
    #[serde(deserialize_with = "required")]
    pub last: Option<UtcSecond>,
    #[serde(deserialize_with = "required")]
    pub error: Option<Token>,
    #[serde(rename = "final")]
    pub final_figures: bool,
}

impl LostRecord {
    /// The line of a marker that held no record.
    pub const NONE: LostRecord = LostRecord {
        events: 0,
        first: None,
        last: None,
        error: None,
        final_figures: true,
    };
}

/// A source whose shutdown answered neither Ok nor ThreadEnded in the stop that ended the last
/// life.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopSourceLine {
    pub name: SourceName,
    pub error: SourceError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateFile {
    Ok,
    Corrupt,
    NewerVersion,
    WriteFailed,
}

one_spelling!(StateFile, "state file", [
    Ok => "ok",
    Corrupt => "corrupt",
    NewerVersion => "newer_version",
    WriteFailed => "write_failed",
]);

/// The capture sources of both systems; [`SourceName::of`] gives each system's own list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceName {
    Foreground,
    Screen,
    Input,
    SystemAudio,
    Microphone,
    Keyboard,
    Pointer,
    WindowTitle,
}

one_spelling!(SourceName, "source name", [
    Foreground => "foreground",
    Screen => "screen",
    Input => "input",
    SystemAudio => "system_audio",
    Microphone => "microphone",
    Keyboard => "keyboard",
    Pointer => "pointer",
    WindowTitle => "window_title",
]);

impl SourceName {
    pub const MACOS: [SourceName; 5] = [
        SourceName::Foreground,
        SourceName::Screen,
        SourceName::Input,
        SourceName::SystemAudio,
        SourceName::Microphone,
    ];

    pub const WINDOWS: [SourceName; 6] = [
        SourceName::Foreground,
        SourceName::Screen,
        SourceName::Keyboard,
        SourceName::Pointer,
        SourceName::WindowTitle,
        SourceName::SystemAudio,
    ];

    pub const fn of(platform: Platform) -> &'static [SourceName] {
        match platform {
            Platform::Macos => &Self::MACOS,
            Platform::Windows => &Self::WINDOWS,
        }
    }
}

/// The source that emits `kind` on `platform`; `None` for a kind that no source of the system
/// emits.
pub const fn source_of(kind: Modality, platform: Platform) -> Option<SourceName> {
    match (platform, kind) {
        (_, Modality::ScreenFrame) => Some(SourceName::Screen),
        (_, Modality::ForegroundApp) => Some(SourceName::Foreground),
        (_, Modality::SystemAudio) => Some(SourceName::SystemAudio),
        (_, Modality::Camera) => None,
        (Platform::Macos, Modality::Keystroke | Modality::Pointer) => Some(SourceName::Input),
        (Platform::Macos, Modality::WindowTitle) => Some(SourceName::Foreground),
        (Platform::Macos, Modality::Microphone) => Some(SourceName::Microphone),
        (Platform::Windows, Modality::Keystroke) => Some(SourceName::Keyboard),
        (Platform::Windows, Modality::Pointer) => Some(SourceName::Pointer),
        (Platform::Windows, Modality::WindowTitle) => Some(SourceName::WindowTitle),
        (Platform::Windows, Modality::Microphone) => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceStateName {
    Running,
    Impaired,
    Failed,
    Stopped,
    Off,
}

one_spelling!(SourceStateName, "source state", [
    Running => "running",
    Impaired => "impaired",
    Failed => "failed",
    Stopped => "stopped",
    Off => "off",
]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    Permission,
    NoKeyEvents,
    SecureInputElsewhere,
    TitleReadErrors,
    StreamBlank,
    StreamSuspended,
    ThreadEnded,
    WriterGone,
    ItemClosed,
    Timeout,
    OsError,
    NotImplemented,
    RunLoopStalled,
    User,
}

one_spelling!(Reason, "source reason", [
    Permission => "permission",
    NoKeyEvents => "no_key_events",
    SecureInputElsewhere => "secure_input_elsewhere",
    TitleReadErrors => "title_read_errors",
    StreamBlank => "stream_blank",
    StreamSuspended => "stream_suspended",
    ThreadEnded => "thread_ended",
    WriterGone => "writer_gone",
    ItemClosed => "item_closed",
    Timeout => "timeout",
    OsError => "os_error",
    NotImplemented => "not_implemented",
    RunLoopStalled => "run_loop_stalled",
    User => "user",
]);

/// A source's error as it crosses the wire: an error kind, a stage and an OS code only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceError {
    pub kind: Token,
    pub stage: Token,
    pub os_domain: Token,
    pub os_code: i64,
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} during {}: {} {}",
            self.kind, self.stage, self.os_domain, self.os_code
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceState {
    Running,
    Impaired {
        reason: Reason,
        error: Option<SourceError>,
    },
    Failed {
        reason: Reason,
        error: Option<SourceError>,
    },
    Stopped {
        by_user: bool,
    },
    Off,
}

impl SourceState {
    pub fn name(&self) -> SourceStateName {
        match self {
            SourceState::Running => SourceStateName::Running,
            SourceState::Impaired { .. } => SourceStateName::Impaired,
            SourceState::Failed { .. } => SourceStateName::Failed,
            SourceState::Stopped { .. } => SourceStateName::Stopped,
            SourceState::Off => SourceStateName::Off,
        }
    }

    pub fn reason(&self) -> Option<Reason> {
        match self {
            SourceState::Impaired { reason, .. } | SourceState::Failed { reason, .. } => {
                Some(*reason)
            }
            SourceState::Stopped { by_user: true } => Some(Reason::User),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&SourceError> {
        match self {
            SourceState::Impaired { error, .. } | SourceState::Failed { error, .. } => {
                error.as_ref()
            }
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct SourceLineWire {
    name: SourceName,
    state: SourceStateName,
    #[serde(deserialize_with = "required")]
    reason: Option<Reason>,
    #[serde(deserialize_with = "required")]
    error: Option<SourceError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SourceLineWire", into = "SourceLineWire")]
pub struct SourceLine {
    pub name: SourceName,
    pub state: SourceState,
}

impl TryFrom<SourceLineWire> for SourceLine {
    type Error = &'static str;

    fn try_from(wire: SourceLineWire) -> Result<Self, Self::Error> {
        let state = match (wire.state, wire.reason, wire.error) {
            (SourceStateName::Running, None, None) => SourceState::Running,
            (SourceStateName::Off, None, None) => SourceState::Off,
            (SourceStateName::Stopped, None, None) => SourceState::Stopped { by_user: false },
            (SourceStateName::Stopped, Some(Reason::User), None) => {
                SourceState::Stopped { by_user: true }
            }
            (SourceStateName::Impaired, Some(reason), error) if reason != Reason::User => {
                SourceState::Impaired { reason, error }
            }
            (SourceStateName::Failed, Some(reason), error) if reason != Reason::User => {
                SourceState::Failed { reason, error }
            }
            _ => return Err("the reason or the error does not fit the source's state"),
        };
        Ok(SourceLine {
            name: wire.name,
            state,
        })
    }
}

impl From<SourceLine> for SourceLineWire {
    fn from(line: SourceLine) -> Self {
        SourceLineWire {
            name: line.name,
            state: line.state.name(),
            reason: line.state.reason(),
            error: line.state.error().cloned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SwitchName {
    TitleText,
    KeyClass,
    PointerClass,
    ScreenActivity,
    SiteRead,
}

one_spelling!(SwitchName, "switch", [
    TitleText => "title_text",
    KeyClass => "key_class",
    PointerClass => "pointer_class",
    ScreenActivity => "screen_activity",
    SiteRead => "site_read",
]);

impl SwitchName {
    pub const fn default_on(self) -> bool {
        matches!(
            self,
            SwitchName::TitleText | SwitchName::KeyClass | SwitchName::PointerClass
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OnOff {
    On,
    Off,
}

one_spelling!(OnOff, "switch value", [On => "on", Off => "off"]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchLine {
    pub name: SwitchName,
    pub value: OnOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionLine {
    pub name: Permission,
    pub grant: GrantState,
    pub at_start: GrantState,
    pub asked: bool,
    pub once_granted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindLine {
    pub name: Modality,
    pub dropped: u64,
    pub refused: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PasswordGuard {
    Os,
    Halvo,
    None,
}

one_spelling!(PasswordGuard, "password guard", [Os => "os", Halvo => "halvo", None => "none"]);

/// The key custody of the running build (contracts.md C20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Custody {
    RootFile,
    Dpapi,
    SecureEnclave,
}

one_spelling!(Custody, "custody", [
    RootFile => "root_file",
    Dpapi => "dpapi",
    SecureEnclave => "secure_enclave",
]);

/// Every field of the status, in the order of the text of contracts.md C2.3, with the wire
/// fields for the app after them. [`DaemonStatus`] holds a value whose fields agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusFields {
    pub daemon_version: DaemonVersion,
    pub pid: u32,
    pub started_at: UtcSecond,
    pub rss_peak_bytes: u64,
    pub parent: Parent,
    pub desired: CaptureState,
    pub capture: Capture,
    #[serde(deserialize_with = "required")]
    pub capture_reason: Option<CaptureReason>,
    pub lease: Lease,
    pub store: StoreState,
    #[serde(deserialize_with = "required")]
    pub store_id: Option<StoreId>,
    #[serde(deserialize_with = "required")]
    pub store_error: Option<StoreErrorKind>,
    #[serde(deserialize_with = "required")]
    pub store_retry_in_s: Option<u64>,
    #[serde(deserialize_with = "required")]
    pub store_bytes: Option<u64>,
    #[serde(deserialize_with = "required")]
    pub store_logical_bytes: Option<u64>,
    pub cap_bytes: Cap,
    pub drain: Health,
    #[serde(deserialize_with = "required")]
    pub drain_error: Option<DrainError>,
    #[serde(deserialize_with = "required")]
    pub drain_since: Option<UtcSecond>,
    pub events_pending: u64,
    pub events_lost: u64,
    pub dropped_frames: u64,
    pub pointer_coalesced: u64,
    pub tap_disabled_timeout: u64,
    pub tap_disabled_user_input: u64,
    pub late_events: u64,
    pub max_lag_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secure_input_keystrokes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys_withheld: Option<bool>,
    pub title_read_errors: u64,
    #[serde(deserialize_with = "required")]
    pub last_flush_at: Option<UtcSecond>,
    pub expiry: Health,
    #[serde(deserialize_with = "required")]
    pub expiry_error: Option<ExpiryError>,
    #[serde(deserialize_with = "required")]
    pub last_expiry_at: Option<UtcSecond>,
    #[serde(deserialize_with = "required")]
    pub last_eviction_at: Option<UtcSecond>,
    pub last_stop: LastStop,
    #[serde(deserialize_with = "required")]
    pub last_stop_newest_capture: Option<UtcSecond>,
    #[serde(deserialize_with = "required")]
    pub last_stop_kind: Option<StopKind>,
    pub last_stop_lost: LostRecord,
    pub last_stop_sources: Vec<StopSourceLine>,
    pub state_file: StateFile,
    pub platform: Platform,
    pub enabled_sources: Vec<SourceName>,
    pub switches: Vec<SwitchLine>,
    pub permissions: Vec<PermissionLine>,
    pub sources: Vec<SourceLine>,
    pub kinds: Vec<KindLine>,
    pub password_guard: PasswordGuard,
    #[serde(deserialize_with = "required")]
    pub screen_filter_excludes_app: Option<bool>,
    pub custody: Custody,
    pub onboarding: OnboardingState,
    pub ext: Vec<Section>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the status does not hold together: {0}")]
pub struct StatusError(&'static str);

impl StatusError {
    /// The fixed text of the rule that the status broke.
    pub const fn rule(&self) -> &'static str {
        self.0
    }
}

/// The daemon's status. Its fields agree with each other and with its platform: the
/// platform's own lines, its sources in order, the five switches and the eight kinds in
/// order, the enabled sources against the source lines, the store state against its
/// cause, and the sums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "StatusFields", into = "StatusFields")]
pub struct DaemonStatus(StatusFields);

impl DaemonStatus {
    pub fn new(fields: StatusFields) -> Result<Self, StatusError> {
        check(&fields)?;
        Ok(Self(fields))
    }

    pub fn fields(&self) -> &StatusFields {
        &self.0
    }

    pub fn into_fields(self) -> StatusFields {
        self.0
    }
}

impl TryFrom<StatusFields> for DaemonStatus {
    type Error = StatusError;

    fn try_from(fields: StatusFields) -> Result<Self, Self::Error> {
        Self::new(fields)
    }
}

impl From<DaemonStatus> for StatusFields {
    fn from(status: DaemonStatus) -> Self {
        status.0
    }
}

fn check(f: &StatusFields) -> Result<(), StatusError> {
    let fail = |why| Err(StatusError(why));
    match f.platform {
        Platform::Macos => {
            if f.secure_input_keystrokes.is_none() || f.keys_withheld.is_some() {
                return fail("macOS reports secure_input_keystrokes and never keys_withheld");
            }
            let names: Vec<Permission> = f.permissions.iter().map(|p| p.name).collect();
            if names != Permission::R1_MACOS {
                return fail("macOS reports the three permissions in their order");
            }
        }
        Platform::Windows => {
            if f.keys_withheld.is_none() || f.secure_input_keystrokes.is_some() {
                return fail("Windows reports keys_withheld and never a keystroke count");
            }
            if !f.permissions.is_empty() {
                return fail("Windows reports no permission");
            }
        }
    }
    let sources: Vec<SourceName> = f.sources.iter().map(|s| s.name).collect();
    if sources != SourceName::of(f.platform) {
        return fail("the source lines are the platform's sources in their order");
    }
    let switches: Vec<SwitchName> = f.switches.iter().map(|s| s.name).collect();
    if switches != SwitchName::ALL {
        return fail("the switch lines are the five switches in their order");
    }
    let kinds: Vec<Modality> = f.kinds.iter().map(|k| k.name).collect();
    if kinds != Modality::ALL {
        return fail("the kind lines are the eight kinds in their order");
    }
    let dropped = f
        .kinds
        .iter()
        .try_fold(0u64, |sum, kind| sum.checked_add(kind.dropped));
    if dropped != Some(f.dropped_frames) {
        return fail("dropped_frames is the sum of the dropped counts");
    }
    let mut enabled = f.enabled_sources.iter();
    let mut next = enabled.next();
    for line in &f.sources {
        let listed = next == Some(&line.name);
        if listed {
            next = enabled.next();
        }
        if listed == (line.state == SourceState::Off) {
            return fail("a source is in enabled_sources exactly when its state is not off");
        }
    }
    if next.is_some() {
        return fail("enabled_sources names the platform's sources in their order, once each");
    }
    if !f.store.takes(f.store_error) {
        return fail("the store state and its cause are a row of the store table");
    }
    if f.store_error == Some(StoreErrorKind::KeyFile) && f.store_id.is_some() {
        return fail("a store whose key file does not parse has no id");
    }
    let order = |name: &SourceName| SourceName::of(f.platform).iter().position(|s| s == name);
    let mut at = None;
    for line in &f.last_stop_sources {
        let here = order(&line.name);
        if here.is_none() || here <= at {
            return fail(
                "the last_stop_source lines name the platform's sources in their order, once each",
            );
        }
        at = here;
    }
    if f.last_stop_kind.is_none()
        && (f.last_stop_lost != LostRecord::NONE || !f.last_stop_sources.is_empty())
    {
        return fail("a marker with no record has no stop kind, no loss and no source line");
    }
    if f.last_stop == LastStop::SourceNotStopped
        && (f.last_stop_kind.is_none() || f.last_stop_sources.is_empty())
    {
        return fail(
            "a life that ended for a source that did not stop names its kind and the source",
        );
    }
    let never = match f.platform {
        Platform::Macos => &[SourceName::SystemAudio, SourceName::Microphone][..],
        Platform::Windows => &[SourceName::SystemAudio][..],
    };
    if f.sources
        .iter()
        .any(|s| never.contains(&s.name) && s.state != SourceState::Off)
    {
        return fail("a source that never captures in R1 reads off");
    }
    let runs =
        |s: &&SourceLine| matches!(s.state, SourceState::Running | SourceState::Impaired { .. });
    let required = f.sources.iter().filter(|s| s.state != SourceState::Off);
    let running = required.clone().filter(runs).count();
    let all = required.count();
    let fits = match f.capture {
        Capture::Running => running == all,
        Capture::Partial => running > 0 && running < all,
        Capture::Faulted | Capture::Paused | Capture::NoIndicator | Capture::AwaitingResume => {
            running == 0
        }
        Capture::Starting | Capture::Pausing => true,
    };
    if !fits {
        return fail("the capture value agrees with the states of the required sources");
    }
    if f.capture == Capture::NoIndicator
        && (f.lease == Lease::Fresh || f.desired != CaptureState::Running)
    {
        return fail("no_indicator has desired running and a lease that is not fresh");
    }
    if f.capture == Capture::Faulted
        && (f.lease != Lease::Fresh || f.desired != CaptureState::Running)
    {
        return fail("faulted has desired running and a fresh lease");
    }
    if f.capture == Capture::Paused && f.desired != CaptureState::Paused {
        return fail("paused has desired paused");
    }
    if f.capture == Capture::AwaitingResume && f.desired != CaptureState::Running {
        return fail("a capture that awaits a resume has desired running");
    }
    if (f.capture == Capture::Faulted) != f.capture_reason.is_some() {
        return fail("a faulted capture has a reason and no other state has one");
    }
    if (f.drain == Health::Failing) != f.drain_error.is_some() {
        return fail("a failing drain has an error and a healthy one has none");
    }
    if (f.expiry == Health::Failing) != f.expiry_error.is_some() {
        return fail("a failing expiry has an error and a healthy one has none");
    }
    Ok(())
}

/// The two requests that change capture, as [`reaches_the_end`] names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureChange {
    Pause,
    Resume,
}

/// Whether `status` shows the end that `change` asks for (ruling PL-8 part (5)(a)): for a pause
/// desired paused and capture paused; for a resume desired running and capture starting,
/// running, partial, faulted or no_indicator. ctl's exit status and the app's
/// `set_capture_state` both read it.
pub fn reaches_the_end(change: CaptureChange, status: &DaemonStatus) -> bool {
    let f = status.fields();
    match change {
        CaptureChange::Pause => f.desired == CaptureState::Paused && f.capture == Capture::Paused,
        CaptureChange::Resume => {
            f.desired == CaptureState::Running
                && matches!(
                    f.capture,
                    Capture::Starting
                        | Capture::Running
                        | Capture::Partial
                        | Capture::Faulted
                        | Capture::NoIndicator
                )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteDecision {
    Accept,
    AlreadyDeleted { current: StoreId },
    Busy,
    Refused,
}

/// The delete rule of ruling PL-8 part (4)(a), in its order. A deleting store or a locked one
/// is busy, before any id is read. An armed id that is the shown id is accepted; two ids that
/// differ are already deleted, with the shown id; an armed id with no shown id, or no armed id
/// with a shown id, is refused. With neither, an open store or one with no release custody is
/// refused, and any other store is accepted. The rule reads the store's state, its cause and
/// the two ids, and no other field of a status.
pub fn delete_decision(
    store: StoreState,
    cause: Option<StoreErrorKind>,
    shown: Option<StoreId>,
    armed: Option<StoreId>,
) -> DeleteDecision {
    if store == StoreState::Deleting || cause == Some(StoreErrorKind::Locked) {
        return DeleteDecision::Busy;
    }
    match (armed, shown) {
        (Some(armed), Some(shown)) if armed == shown => DeleteDecision::Accept,
        (Some(_), Some(shown)) => DeleteDecision::AlreadyDeleted { current: shown },
        (Some(_), None) | (None, Some(_)) => DeleteDecision::Refused,
        (None, None) if matches!(store, StoreState::Open | StoreState::NoReleaseCustody) => {
            DeleteDecision::Refused
        }
        (None, None) => DeleteDecision::Accept,
    }
}

/// Who starts the permission request child. Form A: the daemon, which serves
/// `request_permission`; form B: the app, which sends `permission_asked`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionForm {
    A,
    B,
}

pub const PERMISSION_FORM: PermissionForm = PermissionForm::A;

impl PermissionForm {
    /// Whether a daemon of this form serves `request`; it answers `unsupported_op` to the
    /// permission request of the other form.
    pub const fn serves(self, request: &Request) -> bool {
        match request {
            Request::RequestPermission { .. } => matches!(self, PermissionForm::A),
            Request::PermissionAsked { .. } => matches!(self, PermissionForm::B),
            _ => true,
        }
    }
}

/// Who may send a request that changes daemon state. R1 can build only `SameUser`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerPolicy {
    SameUser,
    SignedBy(SigningIdentity),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningIdentity {}

impl PeerPolicy {
    pub const R1: PeerPolicy = PeerPolicy::SameUser;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{InventoryRow, Modality, StorageCap, StoreId, UtcDay};
    use crate::onboarding::{GrantState, Permission, RequestOutcome};
    use crate::testing::{sample_status, store_id, FIXTURE_STORE_ID};
    use crate::wire::{read_one_frame, write_frame, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};

    fn every_request() -> Vec<Request> {
        vec![
            Request::Status { indicator: false },
            Request::Status { indicator: true },
            Request::Inventory,
            Request::Pause,
            Request::Resume,
            Request::Stop,
            Request::DeleteAll {
                store: Some(store_id(1)),
            },
            Request::DeleteAll { store: None },
            Request::SetStorageCap {
                bytes: StorageCap::MIN,
            },
            Request::RequestPermission {
                permission: Permission::Accessibility,
            },
            Request::PermissionAsked {
                permission: Permission::InputMonitoring,
            },
            Request::AcknowledgeLostPermission {
                permission: Permission::ScreenAndSystemAudioRecording,
            },
        ]
    }

    fn deleted(store: Option<StoreId>) -> Reply {
        Reply::Deleted(Deleted {
            report: DeleteReport::new(DeleteFields {
                segments_shredded: Some(2),
                keys_destroyed: Some(3),
                root_destroyed: RootDestroyed::Yes,
                root_id: None,
                files_left: 0,
                database_left: false,
                not_followed: 0,
                entries_removed: 4,
                entries_left: 0,
                unknown_entries: 0,
                intent: IntentState::Written,
            })
            .unwrap(),
            store,
        })
    }

    fn inventory_reply(ext: Vec<Section>) -> InventoryReply {
        let day = UtcDay::from_year_month_day(2026, 11, 11).unwrap();
        let rows = vec![
            InventoryRow {
                day,
                modality: Modality::Keystroke,
                events: 5,
                bytes: 160,
            },
            InventoryRow {
                day,
                modality: Modality::Pointer,
                events: 7,
                bytes: 266,
            },
        ];
        InventoryReply::new(
            store_id(1),
            "2026-11-12T09:00:00Z".parse().unwrap(),
            rows,
            Totals {
                events: 12,
                bytes: 426,
                disk_bytes: 1_048_576,
            },
            ext,
        )
        .unwrap()
    }

    fn section_x() -> Section {
        Section {
            name: "x".parse().unwrap(),
            rows: vec![
                SectionRow {
                    k: "first".into(),
                    v: "1".into(),
                },
                SectionRow {
                    k: "second".into(),
                    v: "two words".into(),
                },
            ],
        }
    }

    fn every_reply() -> Vec<Reply> {
        let mut replies = vec![
            Reply::Status(sample_status(Platform::Macos)),
            Reply::Status(sample_status(Platform::Windows)),
            Reply::Inventory(inventory_reply(Vec::new())),
            Reply::Inventory(inventory_reply(vec![section_x()])),
            Reply::CaptureChanged(sample_status(Platform::Macos)),
            Reply::Stopped(Stopped {
                flushed: false,
                events_lost: 3,
            }),
            deleted(Some(store_id(2))),
            deleted(None),
            Reply::AlreadyDeleted(AlreadyDeleted {
                current: store_id(2),
            }),
            Reply::DeleteRefused,
            Reply::CapSet(CapSet {
                status: sample_status(Platform::Windows),
                evicting: true,
            }),
        ];
        for outcome in [RequestOutcome::AlreadyGranted, RequestOutcome::Requested] {
            replies.push(Reply::PermissionRequested(PermissionRequested { outcome }));
        }
        for &code in ErrorCode::ALL {
            replies.push(Reply::Error(ErrorReply {
                code,
                message: "a fixed text".into(),
            }));
        }
        replies
    }

    fn through_the_wire_request(request: &Request) -> Decoded {
        let mut frame = Vec::new();
        write_frame(&mut frame, &encode_request(request), REQUEST_MAX_BYTES).unwrap();
        decode_request(&read_one_frame(&mut &frame[..], REQUEST_MAX_BYTES).unwrap()).unwrap()
    }

    fn through_the_wire_reply(reply: &Reply) -> Reply {
        let mut frame = Vec::new();
        write_frame(&mut frame, &encode_reply(reply), REPLY_MAX_BYTES).unwrap();
        decode_reply(&read_one_frame(&mut &frame[..], REPLY_MAX_BYTES).unwrap()).unwrap()
    }

    fn body(json: &str) -> Vec<u8> {
        format!("{{\"v\":1,\"body\":{json}}}").into_bytes()
    }

    #[test]
    fn every_v1_request_round_trips_through_serde_and_the_frame_codec() {
        let requests = every_request();
        let ops: std::collections::BTreeSet<&str> = requests.iter().map(Request::op).collect();
        assert_eq!(
            ops.len(),
            V1_OPS.len(),
            "a request of the table has no sample"
        );
        for request in requests {
            assert_eq!(through_the_wire_request(&request), Decoded::Known(request));
        }
    }

    #[test]
    fn every_v1_reply_round_trips_through_serde_and_the_frame_codec() {
        let replies = every_reply();
        let kinds: std::collections::BTreeSet<&str> = replies.iter().map(Reply::kind).collect();
        assert_eq!(kinds.len(), 10, "a reply kind has no sample");
        for reply in replies {
            assert_eq!(through_the_wire_reply(&reply), reply);
        }
    }

    #[test]
    fn the_requests_have_these_wire_texts() {
        let golden = [
            r#"{"v":1,"body":{"op":"status","indicator":false}}"#,
            r#"{"v":1,"body":{"op":"status","indicator":true}}"#,
            r#"{"v":1,"body":{"op":"inventory"}}"#,
            r#"{"v":1,"body":{"op":"pause"}}"#,
            r#"{"v":1,"body":{"op":"resume"}}"#,
            r#"{"v":1,"body":{"op":"stop"}}"#,
            r#"{"v":1,"body":{"op":"delete_all","store":"00000000000000000000000000000001"}}"#,
            r#"{"v":1,"body":{"op":"delete_all","store":null}}"#,
            r#"{"v":1,"body":{"op":"set_storage_cap","bytes":1073741824}}"#,
            r#"{"v":1,"body":{"op":"request_permission","permission":"accessibility"}}"#,
            r#"{"v":1,"body":{"op":"permission_asked","permission":"input_monitoring"}}"#,
            r#"{"v":1,"body":{"op":"acknowledge_lost_permission","permission":"screen_and_system_audio_recording"}}"#,
        ];
        for (request, text) in every_request().iter().zip(golden) {
            assert_eq!(String::from_utf8(encode_request(request)).unwrap(), text);
            assert_eq!(
                decode_request(text.as_bytes()).unwrap(),
                Decoded::Known(request.clone())
            );
        }
    }

    #[test]
    fn the_small_replies_have_these_wire_texts() {
        let cases = [
            (
                Reply::Stopped(Stopped {
                    flushed: true,
                    events_lost: 0,
                }),
                r#"{"v":1,"body":{"kind":"stopped","flushed":true,"events_lost":0}}"#,
            ),
            (
                Reply::AlreadyDeleted(AlreadyDeleted {
                    current: store_id(2),
                }),
                r#"{"v":1,"body":{"kind":"already_deleted","current":"00000000000000000000000000000002"}}"#,
            ),
            (
                Reply::DeleteRefused,
                r#"{"v":1,"body":{"kind":"delete_refused"}}"#,
            ),
            (
                deleted(None),
                r#"{"v":1,"body":{"kind":"deleted","report":{"segments_shredded":2,"keys_destroyed":3,"root_destroyed":"yes","root_id":null,"files_left":0,"database_left":false,"not_followed":0,"entries_removed":4,"entries_left":0,"unknown_entries":0,"intent":"written"},"store":null}}"#,
            ),
            (
                Reply::PermissionRequested(PermissionRequested {
                    outcome: RequestOutcome::AlreadyGranted,
                }),
                r#"{"v":1,"body":{"kind":"permission_requested","outcome":"already_granted"}}"#,
            ),
            (
                Reply::PermissionRequested(PermissionRequested {
                    outcome: RequestOutcome::Requested,
                }),
                r#"{"v":1,"body":{"kind":"permission_requested","outcome":"requested"}}"#,
            ),
            (
                Reply::Error(ErrorReply {
                    code: ErrorCode::Busy,
                    message: "another mutation is in flight".into(),
                }),
                r#"{"v":1,"body":{"kind":"error","code":"busy","message":"another mutation is in flight"}}"#,
            ),
        ];
        for (reply, text) in cases {
            assert_eq!(String::from_utf8(encode_reply(&reply)).unwrap(), text);
            assert_eq!(decode_reply(text.as_bytes()).unwrap(), reply);
        }
    }

    #[test]
    fn each_error_code_has_one_snake_case_spelling() {
        let spellings: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            spellings,
            [
                "unsupported_version",
                "unsupported_op",
                "busy",
                "store_unavailable",
                "invalid_argument",
                "peer_rejected",
                "state_file",
                "internal"
            ]
        );
    }

    #[test]
    fn the_reserved_names_and_an_op_the_table_does_not_know_are_unknown_and_not_invalid() {
        for op in [
            "run_start",
            "run_end",
            "open_cursor",
            "no_such_op",
            "Status",
            "status ",
        ] {
            let text = body(&format!(r#"{{"op":"{op}","x":1}}"#));
            match decode_request(&text) {
                Ok(Decoded::Unknown(unknown)) => {
                    assert_eq!(unknown.version(), PROTOCOL_VERSION);
                    assert_eq!(unknown.op(), Some(op));
                    assert_eq!(
                        unknown_answer(&unknown).code(),
                        Some(ErrorCode::UnsupportedOp)
                    );
                }
                other => panic!("{op}: {other:?}"),
            }
        }
    }

    #[test]
    fn no_live_request_uses_a_reserved_name() {
        assert_eq!(RESERVED_OPS, ["run_start", "run_end"]);
        assert_eq!(ReservedOp::ALL, [ReservedOp::RunStart, ReservedOp::RunEnd]);
        let names: Vec<&str> = ReservedOp::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(names, RESERVED_OPS);
        for request in every_request() {
            assert!(V1_OPS.contains(&request.op()), "{}", request.op());
            assert!(!RESERVED_OPS.contains(&request.op()), "{}", request.op());
        }
        for reserved in RESERVED_OPS {
            assert!(!V1_OPS.contains(&reserved));
        }
    }

    #[test]
    fn a_version_other_than_1_is_unsupported_version_and_2_is_among_them() {
        for v in ["0", "2", "3", "18446744073709551615"] {
            let text = format!(r#"{{"v":{v},"body":{{"op":"status","indicator":false}}}}"#);
            let error = decode_request(text.as_bytes()).unwrap_err();
            assert_eq!(error.code(), ErrorCode::UnsupportedVersion, "v={v}");
            assert_eq!(error.version(), v.parse::<u64>().ok());
        }
        for v in ["-1", "1.0", "\"1\"", "null", "true", "1e0"] {
            let text = format!(r#"{{"v":{v},"body":{{"op":"pause"}}}}"#);
            assert_eq!(
                decode_request(text.as_bytes()).unwrap_err().code(),
                ErrorCode::InvalidArgument,
                "v={v}"
            );
        }
    }

    /// A frame of version 2 whose body holds in `x` arrays nested so that the frame nests
    /// `levels` deep, each object and each array counted, the frame's own object among them.
    fn version_2_nested(levels: usize) -> Vec<u8> {
        let arrays = levels - 2;
        let mut frame = br#"{"v":2,"body":{"x":"#.to_vec();
        frame.extend(std::iter::repeat_n(b'[', arrays));
        frame.extend(std::iter::repeat_n(b']', arrays));
        frame.extend_from_slice(b"}}");
        frame
    }

    #[test]
    fn a_frame_of_another_version_is_unsupported_version_only_when_the_decoder_reads_all_of_it() {
        let code = |frame: &[u8]| decode_request(frame).unwrap_err().code();
        assert_eq!(code(&version_2_nested(127)), ErrorCode::UnsupportedVersion);
        assert_eq!(
            code(&version_2_nested(128)),
            ErrorCode::InvalidArgument,
            "one level deeper than the 127 that the decoder reads"
        );
        assert_eq!(
            code(br#"{"v":2,"body":{"n":1e300}}"#),
            ErrorCode::UnsupportedVersion
        );
        assert_eq!(
            code(br#"{"v":2,"body":{"n":1e400}}"#),
            ErrorCode::InvalidArgument,
            "a number outside a 64-bit float"
        );
        assert_eq!(
            code(br#"{"v":2,"body":{"s":"\ud800\udc00"}}"#),
            ErrorCode::UnsupportedVersion
        );
        assert_eq!(
            code(br#"{"v":2,"body":{"s":"\ud800"}}"#),
            ErrorCode::InvalidArgument,
            "a lone surrogate escape"
        );
    }

    /// The sample status as JSON, with `store`, `store_error` and `store_retry_in_s` set.
    fn status_json_with_store(store: &str, error: &str, retry: u64) -> serde_json::Value {
        let mut json = serde_json::to_value(sample_status(Platform::Macos).into_fields()).unwrap();
        json["store"] = serde_json::Value::from(store);
        json["store_error"] = serde_json::Value::from(error);
        json["store_retry_in_s"] = serde_json::Value::from(retry);
        json
    }

    /// The store lines of C2.8 that this test holds, `store=unavailable` beside
    /// `store_error=obstructed`, stand in for fixture lines that gate-3 writes, until gate-3 adds
    /// them to a fixture.
    #[test]
    fn e14_obstructed_is_a_cause_of_store_unavailable_and_a_value_outside_the_list_is_refused() {
        let status: DaemonStatus =
            serde_json::from_value(status_json_with_store("unavailable", "obstructed", 5))
                .expect("store=unavailable store_error=obstructed is taken");
        let text = crate::ctl_text::status_text(&status);
        assert!(
            text.contains(&format!(
                "\nstore=unavailable\nstore_id={FIXTURE_STORE_ID}\nstore_error=obstructed\nstore_retry_in_s=5\n"
            )),
            "{text}"
        );
        assert_eq!(
            serde_json::to_value(&status).unwrap()["store_error"],
            "obstructed"
        );
        for (store, error) in [
            ("unavailable", "obstructe"),
            ("unavailable", "Obstructed"),
            ("open", "obstructed"),
            ("sealed", "obstructed"),
        ] {
            assert!(
                serde_json::from_value::<DaemonStatus>(status_json_with_store(store, error, 5))
                    .is_err(),
                "store={store} store_error={error}"
            );
        }
    }

    #[test]
    fn e15_a_deleted_report_carries_not_followed_after_database_left_and_one_without_it_does_not_decode(
    ) {
        let with = br#"{"v":1,"body":{"kind":"deleted","report":{"segments_shredded":2,"keys_destroyed":3,"root_destroyed":"yes","root_id":null,"files_left":0,"database_left":false,"not_followed":1,"entries_removed":4,"entries_left":0,"unknown_entries":0,"intent":"written"},"store":null}}"#;
        let Reply::Deleted(deleted) = decode_reply(with).expect("a report with not_followed")
        else {
            panic!("not a deleted reply");
        };
        assert!(
            crate::ctl_text::deleted_text(&deleted)
                .contains(" files_left=0 database_left=no not_followed=1 entries_removed=4 "),
            "{}",
            crate::ctl_text::deleted_text(&deleted)
        );
        let without = br#"{"v":1,"body":{"kind":"deleted","report":{"segments_shredded":2,"keys_destroyed":3,"root_destroyed":"yes","root_id":null,"files_left":0,"database_left":false,"entries_removed":4,"entries_left":0,"unknown_entries":0,"intent":"written"},"store":null}}"#;
        assert!(decode_reply(without).is_err());
    }

    #[test]
    fn a1_capture_paused_beside_a_desired_state_that_is_not_paused_is_refused() {
        for platform in [Platform::Macos, Platform::Windows] {
            let mut f = sample_status(platform).into_fields();
            f.capture = Capture::Paused;
            for line in &mut f.sources {
                if line.state != SourceState::Off {
                    line.state = SourceState::Stopped { by_user: false };
                }
            }
            assert_eq!(f.desired, CaptureState::Running);
            let refused = DaemonStatus::new(f.clone()).unwrap_err();
            assert_eq!(
                refused.to_string(),
                "the status does not hold together: paused has desired paused"
            );
            f.desired = CaptureState::Paused;
            assert!(DaemonStatus::new(f).is_ok(), "{platform:?}");
        }
    }

    #[test]
    fn a1_a_version_is_an_integer_from_0_to_18446744073709551615_and_one_other_than_1_is_unsupported_whatever_the_frame_holds(
    ) {
        for text in [
            r#"{"v":2,"body":{"op":"status","indicator":false}}"#,
            r#"{"v":2,"body":{"op":"status"},"id":7}"#,
            r#"{"v":2,"body":["status"]}"#,
            r#"{"v":2}"#,
            r#"{"v":18446744073709551615,"id":7}"#,
            r#"{"v":0,"body":7}"#,
        ] {
            let error = decode_request(text.as_bytes()).unwrap_err();
            assert_eq!(error.code(), ErrorCode::UnsupportedVersion, "{text}");
            assert!(
                matches!(decode_reply(text.as_bytes()), Err(ReplyError::Version(_))),
                "{text}"
            );
        }
        for text in [
            r#"{"v":"2","body":{"op":"status","indicator":false}}"#,
            r#"{"v":1.5,"body":{"op":"status","indicator":false}}"#,
            r#"{"v":-1,"body":{"op":"status","indicator":false}}"#,
            r#"{"v":18446744073709551616,"body":{"op":"status","indicator":false}}"#,
            r#"{"v":null,"body":{"op":"status","indicator":false}}"#,
        ] {
            assert_eq!(
                decode_request(text.as_bytes()).unwrap_err().code(),
                ErrorCode::InvalidArgument,
                "{text}"
            );
            assert!(
                matches!(
                    decode_reply(text.as_bytes()),
                    Err(ReplyError::NotAnEnvelope)
                ),
                "{text}"
            );
        }
        assert!(THREAT_MODEL.contains("a version is an integer from 0 to 18446744073709551615"));
    }

    #[test]
    fn a_frame_with_a_key_twice_is_invalid_argument_at_any_depth() {
        for text in [
            r#"{"v":1,"body":{"op":"run_start","op":"pause"}}"#,
            r#"{"v":1,"body":{"op":"status","indicator":false,"indicator":true}}"#,
            r#"{"v":2,"body":{"op":"pause"},"v":1}"#,
            r#"{"v":1,"body":{"op":"pause"},"body":{"op":"stop"}}"#,
            r#"{"v":1,"body":{"op":"delete_all","store":null,"store":"00000000000000000000000000000001"}}"#,
        ] {
            assert_eq!(
                decode_request(text.as_bytes()).unwrap_err().code(),
                ErrorCode::InvalidArgument,
                "{text}"
            );
        }
        let nested = r#"{"v":1,"body":{"op":"no_such_op","x":{"a":1,"a":2}}}"#;
        assert_eq!(
            decode_request(nested.as_bytes()).unwrap_err().code(),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_frame_that_is_no_envelope_is_invalid_argument() {
        for text in [
            "",
            "{",
            "[]",
            "null",
            r#"{"body":{"op":"pause"}}"#,
            r#"{"v":1}"#,
            r#"{"v":1,"body":{"op":"pause"},"extra":0}"#,
            r#"{"v":1,"body":[]}"#,
            r#"{"v":1,"body":"pause"}"#,
            r#"{"v":1,"body":{}}"#,
            r#"{"v":1,"body":{"op":7}}"#,
            r#"{"v":1,"body":{"op":"pause"}} "#,
            "\u{feff}{\"v\":1,\"body\":{\"op\":\"pause\"}}",
        ] {
            assert_eq!(
                decode_request(text.as_bytes()).unwrap_err().code(),
                ErrorCode::InvalidArgument,
                "{text:?}"
            );
        }
        assert_eq!(
            decode_request(&[0xff, 0xfe]).unwrap_err().code(),
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn a_request_with_an_unknown_field_is_invalid_argument() {
        for request in every_request() {
            let mut value: serde_json::Value =
                serde_json::from_slice(&encode_request(&request)).unwrap();
            value["body"]["later"] = serde_json::json!(1);
            let text = serde_json::to_vec(&value).unwrap();
            assert_eq!(
                decode_request(&text).unwrap_err().code(),
                ErrorCode::InvalidArgument,
                "{}",
                request.op()
            );
        }
    }

    #[test]
    fn a_request_without_a_field_or_with_a_bad_value_is_invalid_argument() {
        for json in [
            r#"{"op":"status"}"#,
            r#"{"op":"status","indicator":"yes"}"#,
            r#"{"op":"status","indicator":null}"#,
            r#"{"op":"delete_all"}"#,
            r#"{"op":"delete_all","store":"not hex"}"#,
            r#"{"op":"delete_all","store":"0000000000000000000000000000000A"}"#,
            r#"{"op":"set_storage_cap"}"#,
            r#"{"op":"set_storage_cap","bytes":1073741823}"#,
            r#"{"op":"set_storage_cap","bytes":1099511627777}"#,
            r#"{"op":"set_storage_cap","bytes":-1}"#,
            r#"{"op":"request_permission"}"#,
            r#"{"op":"request_permission","permission":"camera"}"#,
            r#"{"op":"permission_asked","permission":"Accessibility"}"#,
            r#"{"op":"acknowledge_lost_permission","permission":null}"#,
        ] {
            assert_eq!(
                decode_request(&body(json)).unwrap_err().code(),
                ErrorCode::InvalidArgument,
                "{json}"
            );
        }
    }

    #[test]
    fn the_cap_bounds_of_a_request_are_one_gib_and_one_tib() {
        for (bytes, ok) in [
            (1_073_741_824u64, true),
            (1_099_511_627_776, true),
            (1_073_741_823, false),
            (1_099_511_627_777, false),
        ] {
            let json = format!(r#"{{"op":"set_storage_cap","bytes":{bytes}}}"#);
            assert_eq!(decode_request(&body(&json)).is_ok(), ok, "{bytes}");
        }
    }

    #[test]
    fn a_reply_with_an_unknown_field_parses() {
        for reply in every_reply() {
            let mut value: serde_json::Value =
                serde_json::from_slice(&encode_reply(&reply)).unwrap();
            value["body"]["later"] = serde_json::json!({"a": [1, 2]});
            if let Some(status) = value["body"].get_mut("status") {
                status["later"] = serde_json::json!(true);
            }
            let text = serde_json::to_vec(&value).unwrap();
            assert_eq!(decode_reply(&text).unwrap(), reply, "{}", reply.kind());
        }
    }

    #[test]
    fn a_reply_of_another_version_or_an_unknown_kind_is_an_error() {
        let stopped = r#"{"kind":"stopped","flushed":true,"events_lost":0}"#;
        for text in [
            format!(r#"{{"v":2,"body":{stopped}}}"#),
            format!(r#"{{"v":0,"body":{stopped}}}"#),
            r#"{"v":1,"body":{"kind":"later_kind"}}"#.to_string(),
            r#"{"v":1,"body":{"kind":"error","code":"later_code","message":""}}"#.to_string(),
            r#"{"v":1,"body":{"flushed":true,"events_lost":0}}"#.to_string(),
            r#"{"v":1,"body":{"kind":"stopped","flushed":true}}"#.to_string(),
        ] {
            assert!(decode_reply(text.as_bytes()).is_err(), "{text}");
        }
    }

    #[test]
    fn a_reply_section_that_no_one_knows_parses_and_keeps_its_name_and_rows() {
        let reply = Reply::Inventory(inventory_reply(vec![section_x()]));
        let Reply::Inventory(read) = through_the_wire_reply(&reply) else {
            panic!("not an inventory");
        };
        assert_eq!(read.ext(), [section_x()]);
        let text = String::from_utf8(encode_reply(&reply)).unwrap();
        assert!(
            text.contains(r#""ext":[{"name":"x","rows":[{"k":"first","v":"1"},{"k":"second","v":"two words"}]}]"#),
            "{text}"
        );
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.ext = vec![section_x()];
        let status = DaemonStatus::new(fields).unwrap();
        let Reply::Status(read) = through_the_wire_reply(&Reply::Status(status)) else {
            panic!("not a status");
        };
        assert_eq!(read.fields().ext, [section_x()]);
    }

    #[test]
    fn a_section_name_is_snake_case() {
        for good in ["x", "derived", "cursor_lag", "a1"] {
            assert!(good.parse::<SectionName>().is_ok(), "{good}");
        }
        for bad in ["", "X", "two words", "a-b", "1a", "_a", "a.b"] {
            assert!(bad.parse::<SectionName>().is_err(), "{bad}");
        }
    }

    #[test]
    fn r1_sends_no_section() {
        assert!(sample_status(Platform::Macos).fields().ext.is_empty());
        assert!(sample_status(Platform::Windows).fields().ext.is_empty());
    }

    fn status_json(platform: Platform) -> serde_json::Value {
        serde_json::to_value(sample_status(platform)).unwrap()
    }

    #[test]
    fn a_status_without_one_of_its_keys_is_refused() {
        for platform in [Platform::Macos, Platform::Windows] {
            crate::model::every_key_is_read::<DaemonStatus>(&status_json(platform));
        }
    }

    #[test]
    fn the_status_names_its_platform_and_carries_the_platform_s_own_lines() {
        let mac = status_json(Platform::Macos);
        assert_eq!(mac["platform"], "macos");
        assert_eq!(mac["secure_input_keystrokes"], 0);
        assert!(mac.get("keys_withheld").is_none());
        assert_eq!(mac["permissions"].as_array().unwrap().len(), 3);
        let win = status_json(Platform::Windows);
        assert_eq!(win["platform"], "windows");
        assert_eq!(win["keys_withheld"], false);
        assert!(win.get("secure_input_keystrokes").is_none());
        assert_eq!(win["permissions"], serde_json::json!([]));
        assert_eq!(win["password_guard"], "none");
        assert_eq!(mac["password_guard"], "os");
        assert_eq!(mac["custody"], "root_file");
        assert_eq!(win["custody"], "dpapi");
        assert_eq!(mac["screen_filter_excludes_app"], serde_json::Value::Null);
    }

    fn refused(edit: impl Fn(&mut StatusFields)) {
        for platform in [Platform::Macos, Platform::Windows] {
            let mut fields = sample_status(platform).into_fields();
            edit(&mut fields);
            let json = serde_json::to_value(&fields).unwrap();
            assert!(DaemonStatus::new(fields).is_err(), "{platform:?}: built");
            assert!(
                serde_json::from_value::<DaemonStatus>(json).is_err(),
                "{platform:?}: read"
            );
        }
    }

    #[test]
    fn a_status_whose_platform_lines_do_not_fit_its_platform_is_refused() {
        refused(|f| {
            f.platform = match f.platform {
                Platform::Macos => Platform::Windows,
                Platform::Windows => Platform::Macos,
            }
        });
        refused(|f| {
            f.secure_input_keystrokes = Some(1);
            f.keys_withheld = Some(true);
        });
        refused(|f| {
            f.secure_input_keystrokes = None;
            f.keys_withheld = None;
        });
        refused(|f| {
            if f.permissions.is_empty() {
                f.permissions = sample_status(Platform::Macos).into_fields().permissions;
            } else {
                f.permissions.reverse();
            }
        });
        refused(|f| {
            if f.permissions.is_empty() {
                f.permissions = sample_status(Platform::Macos).into_fields().permissions;
            } else {
                f.permissions.pop();
            }
        });
    }

    #[test]
    fn a_status_with_its_sources_switches_or_kinds_out_of_order_or_missing_is_refused() {
        refused(|f| f.sources.swap(0, 1));
        refused(|f| {
            f.sources.pop();
        });
        refused(|f| f.switches.swap(3, 4));
        refused(|f| {
            f.switches.pop();
        });
        refused(|f| f.kinds.swap(0, 1));
        refused(|f| {
            f.kinds.pop();
        });
    }

    #[test]
    fn a_status_whose_enabled_sources_disagree_with_the_source_lines_is_refused() {
        refused(|f| {
            f.enabled_sources.pop();
        });
        refused(|f| f.enabled_sources.push(SourceName::Screen));
        refused(|f| f.enabled_sources.reverse());
        refused(|f| {
            let first = f.enabled_sources[0];
            f.enabled_sources.push(first);
        });
        refused(|f| f.sources[0].state = SourceState::Off);
        refused(|f| f.sources[1].state = SourceState::Running);
    }

    #[test]
    fn the_dropped_frames_figure_is_the_sum_of_the_dropped_counts() {
        refused(|f| f.dropped_frames = 1);
        refused(|f| f.kinds[1].dropped = 1);
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.kinds[0].dropped = 2;
        fields.kinds[2].dropped = 5;
        fields.dropped_frames = 7;
        assert!(DaemonStatus::new(fields).is_ok());
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.kinds[0].dropped = u64::MAX;
        fields.kinds[1].dropped = 1;
        fields.dropped_frames = 0;
        assert!(DaemonStatus::new(fields).is_err());
    }

    #[test]
    fn a_store_state_takes_only_the_causes_of_its_row() {
        use StoreErrorKind::*;
        use StoreState::*;
        let rows: [(StoreState, &[StoreErrorKind]); 7] = [
            (Opening, &[]),
            (Open, &[]),
            (Deleting, &[]),
            (NoReleaseCustody, &[]),
            (
                Unavailable,
                &[Locked, Io, NoSpace, CustodyLocked, RootDenied, Obstructed],
            ),
            (
                Sealed,
                &[
                    RootMissing,
                    RootCorrupt,
                    WrapAuth,
                    KeyFile,
                    CustodyKind,
                    Database,
                ],
            ),
            (UnsupportedVersion, &[Older, Newer]),
        ];
        let mut seen = Vec::new();
        for (state, causes) in rows {
            let none_ok = causes.is_empty();
            let mut fields = sample_status(Platform::Macos).into_fields();
            fields.store = state;
            fields.store_error = None;
            assert_eq!(DaemonStatus::new(fields).is_ok(), none_ok, "{state:?} -");
            for &error in StoreErrorKind::ALL {
                let mut fields = sample_status(Platform::Macos).into_fields();
                fields.store = state;
                fields.store_error = Some(error);
                if error == KeyFile {
                    fields.store_id = None;
                }
                assert_eq!(
                    DaemonStatus::new(fields).is_ok(),
                    causes.contains(&error),
                    "{state:?} {error:?}"
                );
            }
            seen.extend_from_slice(causes);
        }
        assert_eq!(seen.len(), StoreErrorKind::ALL.len());
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.store = Sealed;
        fields.store_error = Some(KeyFile);
        assert!(
            DaemonStatus::new(fields).is_err(),
            "a key_file store with an id"
        );
    }

    #[test]
    fn the_store_id_is_carried_in_every_state_whose_key_file_parses() {
        for &state in StoreState::ALL {
            let mut fields = sample_status(Platform::Macos).into_fields();
            fields.store = state;
            fields.store_error = match state {
                StoreState::Unavailable => Some(StoreErrorKind::RootDenied),
                StoreState::Sealed => Some(StoreErrorKind::WrapAuth),
                StoreState::UnsupportedVersion => Some(StoreErrorKind::Older),
                _ => None,
            };
            let status = DaemonStatus::new(fields).unwrap();
            let read: DaemonStatus =
                serde_json::from_value(serde_json::to_value(&status).unwrap()).unwrap();
            assert_eq!(read.fields().store_id, Some(FIXTURE_STORE_ID), "{state:?}");
        }
    }

    #[test]
    fn faulted_capture_and_a_reason_go_together() {
        refused(|f| f.capture_reason = Some(CaptureReason::Store));
        refused(|f| f.capture = Capture::Faulted);
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.capture = Capture::Faulted;
        fields.capture_reason = Some(CaptureReason::NoSource);
        for line in &mut fields.sources {
            if line.state == SourceState::Running {
                line.state = SourceState::Failed {
                    reason: Reason::Permission,
                    error: None,
                };
            }
        }
        assert!(DaemonStatus::new(fields).is_ok());
    }

    fn with_sources(
        platform: Platform,
        capture: Capture,
        state: impl Fn(SourceName) -> Option<SourceState>,
    ) -> StatusFields {
        let mut fields = sample_status(platform).into_fields();
        fields.capture = capture;
        fields.capture_reason = (capture == Capture::Faulted).then_some(CaptureReason::NoSource);
        for line in &mut fields.sources {
            if let Some(new) = state(line.name) {
                line.state = new;
            }
        }
        fields
    }

    #[test]
    fn a_source_that_never_captures_in_r1_reads_off() {
        for (platform, name) in [
            (Platform::Macos, SourceName::SystemAudio),
            (Platform::Macos, SourceName::Microphone),
            (Platform::Windows, SourceName::SystemAudio),
        ] {
            let mut fields = with_sources(platform, Capture::Running, |n| {
                (n == name).then_some(SourceState::Running)
            });
            let at = SourceName::of(platform)
                .iter()
                .position(|n| *n == name)
                .unwrap();
            fields.enabled_sources = SourceName::of(platform)
                .iter()
                .enumerate()
                .filter(|(i, n)| *i == at || fields.enabled_sources.contains(n))
                .map(|(_, n)| *n)
                .collect();
            assert!(DaemonStatus::new(fields).is_err(), "{platform:?} {name:?}");
        }
    }

    #[test]
    fn the_capture_value_agrees_with_the_states_of_the_required_sources() {
        let failed = || SourceState::Failed {
            reason: Reason::OsError,
            error: None,
        };
        let stopped = || SourceState::Stopped { by_user: false };
        let input_failed = |n| (n == SourceName::Input).then(failed);
        let all_stopped = |n| {
            (n != SourceName::Screen && n != SourceName::SystemAudio && n != SourceName::Microphone)
                .then(stopped)
        };
        let all_failed = |n| {
            (n != SourceName::Screen && n != SourceName::SystemAudio && n != SourceName::Microphone)
                .then(failed)
        };
        let impaired = |n| {
            (n == SourceName::Input).then_some(SourceState::Impaired {
                reason: Reason::NoKeyEvents,
                error: None,
            })
        };
        type States<'a> = Box<dyn Fn(SourceName) -> Option<SourceState> + 'a>;
        let cases: Vec<(Capture, States<'_>, bool)> = vec![
            (Capture::Running, Box::new(|_| None), true),
            (Capture::Running, Box::new(impaired), true),
            (Capture::Running, Box::new(input_failed), false),
            (Capture::Partial, Box::new(input_failed), true),
            (Capture::Partial, Box::new(|_| None), false),
            (Capture::Partial, Box::new(all_failed), false),
            (Capture::Faulted, Box::new(all_failed), true),
            (Capture::Faulted, Box::new(input_failed), false),
            (Capture::Paused, Box::new(all_stopped), true),
            (Capture::Paused, Box::new(|_| None), false),
            (Capture::NoIndicator, Box::new(|_| None), false),
            (Capture::AwaitingResume, Box::new(|_| None), false),
        ];
        for (capture, state, ok) in cases {
            let mut fields = with_sources(Platform::Macos, capture, state);
            if capture == Capture::Paused {
                fields.desired = crate::model::CaptureState::Paused;
            }
            if capture == Capture::NoIndicator {
                fields.lease = Lease::Lapsed;
            }
            let built = DaemonStatus::new(fields.clone());
            assert_eq!(built.is_ok(), ok, "{capture:?}: {:?}", fields.sources);
        }
    }

    #[test]
    fn no_indicator_has_a_lease_that_is_not_fresh_and_faulted_one_that_is() {
        let stopped = |n| {
            (n == SourceName::Foreground || n == SourceName::Input)
                .then_some(SourceState::Stopped { by_user: false })
        };
        for (lease, ok) in [
            (Lease::Fresh, false),
            (Lease::Lapsed, true),
            (Lease::None, true),
        ] {
            let mut fields = with_sources(Platform::Macos, Capture::NoIndicator, stopped);
            fields.lease = lease;
            assert_eq!(
                DaemonStatus::new(fields).is_ok(),
                ok,
                "no_indicator {lease:?}"
            );
        }
        let mut paused = with_sources(Platform::Macos, Capture::NoIndicator, stopped);
        paused.lease = Lease::Lapsed;
        paused.desired = crate::model::CaptureState::Paused;
        assert!(
            DaemonStatus::new(paused).is_err(),
            "no_indicator with desired paused"
        );
        let failed = |n| {
            (n == SourceName::Foreground || n == SourceName::Input).then_some(SourceState::Failed {
                reason: Reason::Permission,
                error: None,
            })
        };
        for (lease, ok) in [
            (Lease::Fresh, true),
            (Lease::Lapsed, false),
            (Lease::None, false),
        ] {
            let mut fields = with_sources(Platform::Macos, Capture::Faulted, failed);
            fields.lease = lease;
            assert_eq!(DaemonStatus::new(fields).is_ok(), ok, "faulted {lease:?}");
        }
        let mut faulted_paused = with_sources(Platform::Macos, Capture::Faulted, failed);
        faulted_paused.desired = crate::model::CaptureState::Paused;
        assert!(
            DaemonStatus::new(faulted_paused).is_err(),
            "faulted with a fresh lease and desired paused"
        );
    }

    #[test]
    fn a_failing_drain_or_expiry_and_its_error_go_together() {
        refused(|f| f.drain_error = Some(DrainError::Io));
        refused(|f| f.drain = Health::Failing);
        refused(|f| f.expiry_error = Some(ExpiryError::Db));
        refused(|f| f.expiry = Health::Failing);
    }

    #[test]
    fn a_source_line_takes_only_the_reasons_of_its_state() {
        let error = SourceError {
            kind: "os_error".parse().unwrap(),
            stage: "start".parse().unwrap(),
            os_domain: "osstatus".parse().unwrap(),
            os_code: -3801,
        };
        let good = [
            SourceState::Running,
            SourceState::Off,
            SourceState::Stopped { by_user: false },
            SourceState::Stopped { by_user: true },
            SourceState::Impaired {
                reason: Reason::TitleReadErrors,
                error: None,
            },
            SourceState::Failed {
                reason: Reason::OsError,
                error: Some(error.clone()),
            },
        ];
        for state in good {
            let line = SourceLine {
                name: SourceName::Foreground,
                state: state.clone(),
            };
            let json = serde_json::to_value(&line).unwrap();
            assert_eq!(serde_json::from_value::<SourceLine>(json).unwrap(), line);
        }
        for json in [
            r#"{"name":"foreground","state":"running","reason":"permission","error":null}"#,
            r#"{"name":"foreground","state":"off","reason":null,"error":{"kind":"a","stage":"b","os_domain":"c","os_code":1}}"#,
            r#"{"name":"foreground","state":"stopped","reason":"permission","error":null}"#,
            r#"{"name":"foreground","state":"failed","reason":null,"error":null}"#,
            r#"{"name":"foreground","state":"impaired","reason":"user","error":null}"#,
            r#"{"name":"foreground","state":"running","reason":null}"#,
            r#"{"name":"foreground","state":"running","error":null}"#,
        ] {
            assert!(serde_json::from_str::<SourceLine>(json).is_err(), "{json}");
        }
    }

    #[test]
    fn a_source_error_crosses_typed_and_holds_tokens_and_a_code_only() {
        let json = r#"{"kind":"os_error","stage":"start","os_domain":"osstatus","os_code":-3801}"#;
        let error: SourceError = serde_json::from_str(json).unwrap();
        assert_eq!(serde_json::to_string(&error).unwrap(), json);
        assert_eq!(error.to_string(), "os_error during start: osstatus -3801");
        for bad in [
            r#"{"kind":"Os Error","stage":"start","os_domain":"osstatus","os_code":1}"#,
            r#"{"kind":"os_error","stage":"a=b","os_domain":"osstatus","os_code":1}"#,
            r#"{"kind":"os_error","stage":"start","os_domain":"/Users/a","os_code":1}"#,
            r#"{"kind":"os_error","stage":"start","os_domain":"osstatus","os_code":"1"}"#,
        ] {
            assert!(serde_json::from_str::<SourceError>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn each_system_has_its_source_names_in_order_and_one_closed_type_holds_both() {
        let mac: Vec<&str> = SourceName::of(Platform::Macos)
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(
            mac,
            [
                "foreground",
                "screen",
                "input",
                "system_audio",
                "microphone"
            ]
        );
        let win: Vec<&str> = SourceName::of(Platform::Windows)
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(
            win,
            [
                "foreground",
                "screen",
                "keyboard",
                "pointer",
                "window_title",
                "system_audio"
            ]
        );
        for &name in SourceName::ALL {
            assert!(
                SourceName::of(Platform::Macos).contains(&name)
                    || SourceName::of(Platform::Windows).contains(&name),
                "{name:?}"
            );
        }
    }

    #[test]
    fn each_kind_is_placed_on_the_source_that_emits_it_on_each_system() {
        use Modality::*;
        let table = [
            (ScreenFrame, Some("screen"), Some("screen")),
            (Keystroke, Some("input"), Some("keyboard")),
            (Pointer, Some("input"), Some("pointer")),
            (ForegroundApp, Some("foreground"), Some("foreground")),
            (WindowTitle, Some("foreground"), Some("window_title")),
            (SystemAudio, Some("system_audio"), Some("system_audio")),
            (Microphone, Some("microphone"), None),
            (Camera, None, None),
        ];
        assert_eq!(table.len(), Modality::ALL.len());
        for (kind, mac, win) in table {
            assert_eq!(
                source_of(kind, Platform::Macos).map(SourceName::as_str),
                mac,
                "{kind:?}"
            );
            assert_eq!(
                source_of(kind, Platform::Windows).map(SourceName::as_str),
                win,
                "{kind:?}"
            );
            for platform in [Platform::Macos, Platform::Windows] {
                if let Some(source) = source_of(kind, platform) {
                    assert!(SourceName::of(platform).contains(&source));
                }
            }
        }
    }

    #[test]
    fn the_five_switches_are_in_the_order_of_c13_5_with_their_defaults() {
        let names: Vec<&str> = SwitchName::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            names,
            [
                "title_text",
                "key_class",
                "pointer_class",
                "screen_activity",
                "site_read"
            ]
        );
        let defaults: Vec<bool> = SwitchName::ALL.iter().map(|s| s.default_on()).collect();
        assert_eq!(defaults, [true, true, true, false, false]);
        let json = serde_json::to_value(&sample_status(Platform::Macos).fields().switches).unwrap();
        assert_eq!(
            json,
            serde_json::json!([
                {"name": "title_text", "value": "on"},
                {"name": "key_class", "value": "on"},
                {"name": "pointer_class", "value": "on"},
                {"name": "screen_activity", "value": "off"},
                {"name": "site_read", "value": "off"},
            ])
        );
    }

    #[test]
    fn the_custody_the_guard_and_the_closed_values_have_these_spellings() {
        let custody: Vec<&str> = Custody::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(custody, ["root_file", "dpapi", "secure_enclave"]);
        let guard: Vec<&str> = PasswordGuard::ALL.iter().map(|g| g.as_str()).collect();
        assert_eq!(guard, ["os", "halvo", "none"]);
        let store: Vec<&str> = StoreState::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            store,
            [
                "opening",
                "open",
                "deleting",
                "sealed",
                "unavailable",
                "no_release_custody",
                "unsupported_version"
            ]
        );
        let errors: Vec<&str> = StoreErrorKind::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            errors,
            [
                "locked",
                "io",
                "no_space",
                "custody_locked",
                "root_denied",
                "obstructed",
                "root_missing",
                "root_corrupt",
                "wrap_auth",
                "key_file",
                "custody_kind",
                "database",
                "older",
                "newer"
            ]
        );
        let capture: Vec<&str> = Capture::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            capture,
            [
                "starting",
                "running",
                "partial",
                "pausing",
                "paused",
                "no_indicator",
                "awaiting_resume",
                "faulted"
            ]
        );
        let reasons: Vec<&str> = Reason::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            reasons,
            [
                "permission",
                "no_key_events",
                "secure_input_elsewhere",
                "title_read_errors",
                "stream_blank",
                "stream_suspended",
                "thread_ended",
                "writer_gone",
                "item_closed",
                "timeout",
                "os_error",
                "not_implemented",
                "run_loop_stalled",
                "user"
            ]
        );
        let last_stop: Vec<&str> = LastStop::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            last_stop,
            ["none", "clean", "unclean", "source_not_stopped"]
        );
    }

    #[test]
    fn each_token_of_the_stop_kind_last_stop_and_capture_round_trips_and_another_is_refused() {
        let kinds: Vec<&str> = StopKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "pause",
                "lapse",
                "delete_all",
                "grant_lost",
                "signal",
                "stop",
                "lifeline",
                "session_end",
                "grant_appeared"
            ]
        );
        fn round_trip<T>(all: &[T])
        where
            T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
        {
            for value in all {
                let text = serde_json::to_string(value).unwrap();
                assert_eq!(&serde_json::from_str::<T>(&text).unwrap(), value);
            }
            for other in ["\"-\"", "\"\"", "\"Pause\"", "\"later\"", "null"] {
                assert!(serde_json::from_str::<T>(other).is_err(), "{other}");
            }
        }
        round_trip(StopKind::ALL);
        round_trip(LastStop::ALL);
        round_trip(Capture::ALL);
    }

    #[test]
    fn the_kept_record_crosses_the_wire_and_has_its_rules() {
        let mut fields = sample_status(Platform::Macos).into_fields();
        assert_eq!(fields.last_stop_kind, None);
        assert_eq!(fields.last_stop_lost, LostRecord::NONE);
        let json = serde_json::to_value(sample_status(Platform::Macos)).unwrap();
        assert_eq!(json["last_stop_kind"], serde_json::Value::Null);
        assert_eq!(
            json["last_stop_lost"],
            serde_json::json!({"events": 0, "first": null, "last": null, "error": null, "final": true})
        );
        assert_eq!(json["last_stop_sources"], serde_json::json!([]));
        crate::model::every_key_is_read::<LostRecord>(&json["last_stop_lost"]);
        fields.last_stop = LastStop::SourceNotStopped;
        fields.last_stop_kind = Some(StopKind::Pause);
        fields.last_stop_lost = LostRecord {
            events: 3,
            first: Some("2026-11-12T08:59:58Z".parse().unwrap()),
            last: Some("2026-11-12T08:59:59Z".parse().unwrap()),
            error: Some("io".parse().unwrap()),
            final_figures: false,
        };
        let line = |name| StopSourceLine {
            name,
            error: SourceError {
                kind: "timeout".parse().unwrap(),
                stage: "shutdown".parse().unwrap(),
                os_domain: "osstatus".parse().unwrap(),
                os_code: 0,
            },
        };
        fields.last_stop_sources = vec![line(SourceName::Foreground), line(SourceName::Input)];
        let reply = Reply::Status(DaemonStatus::new(fields.clone()).unwrap());
        assert_eq!(through_the_wire_reply(&reply), reply);
        for bad in [
            {
                let mut f = fields.clone();
                f.last_stop_sources.reverse();
                f
            },
            {
                let mut f = fields.clone();
                f.last_stop_sources.push(line(SourceName::Input));
                f
            },
            {
                let mut f = fields.clone();
                f.last_stop_sources = vec![line(SourceName::Keyboard)];
                f
            },
            {
                let mut f = fields.clone();
                f.last_stop_sources.clear();
                f
            },
            {
                let mut f = fields.clone();
                f.last_stop_kind = None;
                f
            },
            {
                let mut f = sample_status(Platform::Macos).into_fields();
                f.last_stop_lost.events = 1;
                f
            },
            {
                let mut f = sample_status(Platform::Macos).into_fields();
                f.last_stop_sources = vec![line(SourceName::Input)];
                f
            },
        ] {
            assert!(DaemonStatus::new(bad).is_err());
        }
    }

    #[test]
    fn a_status_awaiting_resume_after_a_source_that_did_not_stop_round_trips() {
        for platform in [Platform::Macos, Platform::Windows] {
            let mut fields = sample_status(platform).into_fields();
            fields.capture = Capture::AwaitingResume;
            fields.last_stop = LastStop::SourceNotStopped;
            fields.last_stop_kind = Some(StopKind::Lapse);
            fields.last_stop_sources = vec![StopSourceLine {
                name: SourceName::Screen,
                error: SourceError {
                    kind: "timeout".parse().unwrap(),
                    stage: "shutdown".parse().unwrap(),
                    os_domain: "osstatus".parse().unwrap(),
                    os_code: 0,
                },
            }];
            for line in &mut fields.sources {
                if line.state == SourceState::Running {
                    line.state = SourceState::Stopped { by_user: false };
                }
            }
            let status = DaemonStatus::new(fields).unwrap();
            let json = serde_json::to_value(&status).unwrap();
            assert_eq!(json["capture"], "awaiting_resume");
            assert_eq!(json["last_stop"], "source_not_stopped");
            let reply = Reply::Status(status);
            assert_eq!(through_the_wire_reply(&reply), reply);
        }
        let mut paused = sample_status(Platform::Macos).into_fields();
        paused.capture = Capture::AwaitingResume;
        paused.desired = crate::model::CaptureState::Paused;
        for line in &mut paused.sources {
            if line.state == SourceState::Running {
                line.state = SourceState::Stopped { by_user: false };
            }
        }
        assert!(
            DaemonStatus::new(paused).is_err(),
            "awaiting_resume says that desired is running"
        );
    }

    #[test]
    fn a_source_that_failed_with_writer_gone_or_item_closed_crosses_the_wire() {
        for reason in [Reason::WriterGone, Reason::ItemClosed] {
            let mut fields = sample_status(Platform::Windows).into_fields();
            fields.capture = Capture::Partial;
            fields.sources[3].state = SourceState::Failed {
                reason,
                error: None,
            };
            let reply = Reply::Status(DaemonStatus::new(fields).unwrap());
            assert_eq!(through_the_wire_reply(&reply), reply, "{reason:?}");
            let text = String::from_utf8(encode_reply(&reply)).unwrap();
            assert!(
                text.contains(&format!(
                    r#"{{"name":"pointer","state":"failed","reason":"{}","error":null}}"#,
                    reason.as_str()
                )),
                "{text}"
            );
        }
    }

    #[test]
    fn the_cap_is_known_bytes_or_unknown_on_the_wire() {
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.cap_bytes = Cap::Unknown;
        let status = DaemonStatus::new(fields).unwrap();
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["cap_bytes"], "unknown");
        assert_eq!(
            serde_json::from_value::<DaemonStatus>(json).unwrap(),
            status
        );
        assert_eq!(status_json(Platform::Macos)["cap_bytes"], 26_843_545_600u64);
        for bad in ["\"Unknown\"", "null", "5", "\"5\""] {
            assert!(serde_json::from_str::<Cap>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_delete_report_keeps_its_counts_together_and_names_a_surviving_root() {
        let fields = || DeleteFields {
            segments_shredded: Some(2),
            keys_destroyed: Some(3),
            root_destroyed: RootDestroyed::Yes,
            root_id: None,
            files_left: 0,
            database_left: false,
            not_followed: 0,
            entries_removed: 4,
            entries_left: 0,
            unknown_entries: 0,
            intent: IntentState::Written,
        };
        assert!(DeleteReport::new(fields()).is_ok());
        let unknown = DeleteFields {
            segments_shredded: None,
            keys_destroyed: None,
            ..fields()
        };
        assert!(DeleteReport::new(unknown).is_ok());
        for bad in [
            DeleteFields {
                keys_destroyed: Some(2),
                ..fields()
            },
            DeleteFields {
                keys_destroyed: None,
                ..fields()
            },
            DeleteFields {
                segments_shredded: Some(u64::MAX),
                keys_destroyed: Some(u64::MAX),
                ..fields()
            },
            DeleteFields {
                root_destroyed: RootDestroyed::No,
                ..fields()
            },
            DeleteFields {
                root_id: Some(store_id(9)),
                ..fields()
            },
            DeleteFields {
                root_destroyed: RootDestroyed::Unknown,
                root_id: Some(store_id(9)),
                ..fields()
            },
        ] {
            let json = serde_json::to_value(bad).unwrap();
            assert!(DeleteReport::new(bad).is_err());
            assert!(serde_json::from_value::<DeleteReport>(json).is_err());
        }
        let left = DeleteFields {
            root_destroyed: RootDestroyed::No,
            root_id: Some(store_id(9)),
            ..fields()
        };
        assert!(DeleteReport::new(left).is_ok());
    }

    #[test]
    fn a_delete_report_or_a_reply_without_one_of_its_keys_is_refused() {
        let Reply::Deleted(d) = deleted(Some(store_id(2))) else {
            unreachable!()
        };
        crate::model::every_key_is_read::<DeleteReport>(&serde_json::to_value(d.report).unwrap());
        crate::model::every_key_is_read::<Deleted>(&serde_json::to_value(&d).unwrap());
        crate::model::every_key_is_read::<AlreadyDeleted>(
            &serde_json::json!({"current": "00000000000000000000000000000002"}),
        );
        crate::model::every_key_is_read::<Stopped>(
            &serde_json::json!({"flushed": true, "events_lost": 0}),
        );
        crate::model::every_key_is_read::<InventoryReply>(
            &serde_json::to_value(inventory_reply(vec![section_x()])).unwrap(),
        );
    }

    #[test]
    fn an_inventory_reply_whose_totals_disagree_with_its_rows_is_refused() {
        let good = serde_json::to_value(inventory_reply(Vec::new())).unwrap();
        assert!(serde_json::from_value::<InventoryReply>(good.clone()).is_ok());
        for (key, value) in [("events", 13u64), ("bytes", 425)] {
            let mut bad = good.clone();
            bad["totals"][key] = serde_json::json!(value);
            assert!(
                serde_json::from_value::<InventoryReply>(bad).is_err(),
                "{key}"
            );
        }
        let mut twice = good.clone();
        let first = twice["rows"][0].clone();
        twice["rows"].as_array_mut().unwrap().push(first);
        twice["totals"]["events"] = serde_json::json!(17);
        twice["totals"]["bytes"] = serde_json::json!(586);
        assert!(serde_json::from_value::<InventoryReply>(twice).is_err());
    }

    #[test]
    fn the_delete_rule_accepts_every_state_but_deleting_and_locked() {
        let current = FIXTURE_STORE_ID;
        for &state in StoreState::ALL {
            let causes: &[Option<StoreErrorKind>] = match state {
                StoreState::Unavailable => &[
                    Some(StoreErrorKind::Locked),
                    Some(StoreErrorKind::Io),
                    Some(StoreErrorKind::NoSpace),
                    Some(StoreErrorKind::CustodyLocked),
                    Some(StoreErrorKind::RootDenied),
                    Some(StoreErrorKind::Obstructed),
                ],
                StoreState::Sealed => &[
                    Some(StoreErrorKind::RootMissing),
                    Some(StoreErrorKind::RootCorrupt),
                    Some(StoreErrorKind::WrapAuth),
                    Some(StoreErrorKind::CustodyKind),
                    Some(StoreErrorKind::Database),
                ],
                StoreState::UnsupportedVersion => {
                    &[Some(StoreErrorKind::Older), Some(StoreErrorKind::Newer)]
                }
                _ => &[None],
            };
            for &cause in causes {
                let mut fields = sample_status(Platform::Macos).into_fields();
                fields.store = state;
                fields.store_error = cause;
                let status = DaemonStatus::new(fields).unwrap();
                let busy = state == StoreState::Deleting || cause == Some(StoreErrorKind::Locked);
                let want = if busy {
                    DeleteDecision::Busy
                } else {
                    DeleteDecision::Accept
                };
                let f = status.fields();
                assert_eq!(
                    delete_decision(f.store, f.store_error, Some(current), Some(current)),
                    want,
                    "{state:?} {cause:?}"
                );
            }
        }
    }

    #[test]
    fn the_delete_rule_reads_the_two_ids_in_the_order_of_ruling_pl_8() {
        use StoreState::*;
        let (shown, other) = (Some(FIXTURE_STORE_ID), Some(store_id(7)));
        let key_file = Some(StoreErrorKind::KeyFile);
        let io = Some(StoreErrorKind::Io);
        for (store, cause, shown, armed, want) in [
            (Open, None, shown, shown, DeleteDecision::Accept),
            (
                Open,
                None,
                shown,
                other,
                DeleteDecision::AlreadyDeleted {
                    current: FIXTURE_STORE_ID,
                },
            ),
            (Open, None, shown, None, DeleteDecision::Refused),
            (Sealed, key_file, None, other, DeleteDecision::Refused),
            (Unavailable, io, None, other, DeleteDecision::Refused),
            (Open, None, None, None, DeleteDecision::Refused),
            (NoReleaseCustody, None, None, None, DeleteDecision::Refused),
            (Sealed, key_file, None, None, DeleteDecision::Accept),
            (Unavailable, io, None, None, DeleteDecision::Accept),
            (
                Unavailable,
                Some(StoreErrorKind::Obstructed),
                None,
                None,
                DeleteDecision::Accept,
            ),
            (UnsupportedVersion, None, None, None, DeleteDecision::Accept),
            (Deleting, None, None, None, DeleteDecision::Busy),
            (
                Unavailable,
                Some(StoreErrorKind::Locked),
                shown,
                other,
                DeleteDecision::Busy,
            ),
        ] {
            assert_eq!(
                delete_decision(store, cause, shown, armed),
                want,
                "{store:?} {cause:?} shown {shown:?} armed {armed:?}"
            );
        }
    }

    fn decide(status: &DaemonStatus, armed: Option<StoreId>) -> DeleteDecision {
        let f = status.fields();
        delete_decision(f.store, f.store_error, f.store_id, armed)
    }

    #[test]
    fn a_busy_store_answers_busy_before_the_id_is_compared() {
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.store = StoreState::Deleting;
        let deleting = DaemonStatus::new(fields).unwrap();
        for armed in [None, Some(store_id(1)), Some(store_id(7))] {
            assert_eq!(decide(&deleting, armed), DeleteDecision::Busy);
        }
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.store = StoreState::Unavailable;
        fields.store_error = Some(StoreErrorKind::Locked);
        let locked = DaemonStatus::new(fields).unwrap();
        for armed in [None, Some(FIXTURE_STORE_ID), Some(store_id(7))] {
            assert_eq!(
                decide(&locked, armed),
                DeleteDecision::Busy,
                "locked, armed {armed:?}"
            );
        }
    }

    #[test]
    fn each_permission_form_serves_one_of_the_two_requests() {
        let request = Request::RequestPermission {
            permission: Permission::Accessibility,
        };
        let asked = Request::PermissionAsked {
            permission: Permission::Accessibility,
        };
        assert_eq!(PERMISSION_FORM, PermissionForm::A);
        assert!(PermissionForm::A.serves(&request));
        assert!(!PermissionForm::A.serves(&asked));
        assert!(!PermissionForm::B.serves(&request));
        assert!(PermissionForm::B.serves(&asked));
        for other in every_request() {
            if !matches!(
                other,
                Request::RequestPermission { .. } | Request::PermissionAsked { .. }
            ) {
                assert!(PermissionForm::A.serves(&other) && PermissionForm::B.serves(&other));
            }
        }
    }

    #[test]
    fn the_mutations_are_the_requests_that_change_the_daemon_or_its_files() {
        let mutations: Vec<&str> = every_request()
            .iter()
            .filter(|r| r.is_mutation())
            .map(Request::op)
            .collect();
        assert_eq!(
            mutations,
            [
                "pause",
                "resume",
                "stop",
                "delete_all",
                "delete_all",
                "set_storage_cap",
                "request_permission",
                "permission_asked",
                "acknowledge_lost_permission"
            ]
        );
    }

    #[test]
    fn a_status_request_carries_the_claim_only_when_asked_and_inventory_never() {
        assert!(Request::Status { indicator: true }.claims_the_indicator());
        for request in every_request() {
            if request != (Request::Status { indicator: true }) {
                assert!(!request.claims_the_indicator(), "{request:?}");
            }
        }
    }

    #[test]
    fn the_unknown_answer_is_unsupported_op_with_no_echo_of_the_request() {
        let text = body(r#"{"op":"run_start","p":"a title"}"#);
        let Ok(Decoded::Unknown(unknown)) = decode_request(&text) else {
            panic!()
        };
        let Reply::Error(answer) = unknown_answer(&unknown) else {
            panic!()
        };
        assert_eq!(answer.code, ErrorCode::UnsupportedOp);
        assert!(!answer.message.contains("title"), "{}", answer.message);
        let refusal = decode_request(&body(r#"{"op":"pause","p":"a title"}"#)).unwrap_err();
        let Reply::Error(answer) = refusal.answer() else {
            panic!()
        };
        assert_eq!(answer.code, ErrorCode::InvalidArgument);
        assert!(!answer.message.contains("title"), "{}", answer.message);
    }

    #[test]
    fn a_reply_over_the_frame_limit_is_refused_by_the_writer_and_never_cut() {
        let rows: Vec<Section> = (0..2_000)
            .map(|i| Section {
                name: format!("s{i}").parse().unwrap(),
                rows: vec![SectionRow {
                    k: "k".repeat(64),
                    v: "v".repeat(64),
                }],
            })
            .collect();
        let reply = Reply::Inventory(inventory_reply(rows));
        let bytes = encode_reply(&reply);
        assert!(bytes.len() > REPLY_MAX_BYTES);
        let mut out = Vec::new();
        assert!(write_frame(&mut out, &bytes, REPLY_MAX_BYTES).is_err());
        assert!(out.is_empty());
    }

    #[test]
    fn a_full_inventory_of_14_days_and_8_kinds_fits_one_reply_frame() {
        let mut rows = Vec::new();
        let first = UtcDay::from_year_month_day(2026, 11, 1).unwrap();
        let mut day = first;
        for _ in 0..14 {
            for &modality in &Modality::ALL {
                rows.push(InventoryRow {
                    day,
                    modality,
                    events: u64::MAX / 1_000,
                    bytes: u64::MAX / 1_000,
                });
            }
            day = day.next().unwrap();
        }
        let events = rows.iter().map(|r| r.events).sum();
        let bytes = rows.iter().map(|r| r.bytes).sum();
        let reply = Reply::Inventory(
            InventoryReply::new(
                store_id(1),
                "2026-11-14T00:00:00Z".parse().unwrap(),
                rows,
                Totals {
                    events,
                    bytes,
                    disk_bytes: u64::MAX,
                },
                Vec::new(),
            )
            .unwrap(),
        );
        let encoded = encode_reply(&reply);
        assert!(encoded.len() < REPLY_MAX_BYTES / 10, "{}", encoded.len());
        assert_eq!(through_the_wire_reply(&reply), reply);
    }

    #[test]
    fn the_peer_policy_of_r1_is_the_same_user() {
        assert_eq!(PeerPolicy::R1, PeerPolicy::SameUser);
        match PeerPolicy::R1 {
            PeerPolicy::SameUser => {}
            PeerPolicy::SignedBy(identity) => match identity {},
        }
    }

    #[test]
    fn the_status_reply_carries_the_onboarding_object() {
        let status = sample_status(Platform::Macos);
        let json = serde_json::to_value(&status).unwrap();
        let onboarding = &json["onboarding"];
        assert_eq!(onboarding["all_granted"], true);
        assert_eq!(onboarding["permissions"].as_array().unwrap().len(), 3);
        let grants: Vec<&str> = json["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["grant"].as_str().unwrap())
            .collect();
        assert_eq!(grants, ["granted", "granted", "granted"]);
        assert_eq!(status.fields().permissions[0].grant, GrantState::Granted);
    }

    const THREAT_MODEL: &str = include_str!("../SECURITY.md");

    fn listed(request: &Request) -> &'static str {
        match request {
            Request::Status { .. } => "status",
            Request::Inventory => "inventory",
            Request::Pause => "pause",
            Request::Resume => "resume",
            Request::Stop => "stop",
            Request::DeleteAll { .. } => "delete_all",
            Request::SetStorageCap { .. } => "set_storage_cap",
            Request::RequestPermission { .. } => "request_permission",
            Request::PermissionAsked { .. } => "permission_asked",
            Request::AcknowledgeLostPermission { .. } => "acknowledge_lost_permission",
        }
    }

    fn table_rows() -> Vec<&'static str> {
        THREAT_MODEL
            .lines()
            .filter_map(|line| line.strip_prefix("| `"))
            .filter_map(|rest| rest.split_once("` |"))
            .map(|(op, _)| op)
            .collect()
    }

    #[test]
    fn threat_model_lists_every_request() {
        let rows = table_rows();
        for request in every_request() {
            let op = listed(&request);
            assert_eq!(op, request.op());
            assert_eq!(
                rows.iter().filter(|row| **row == op).count(),
                1,
                "crates/halvo-control/SECURITY.md has no row, or two, for {op}"
            );
        }
        for row in &rows {
            assert!(
                V1_OPS.contains(row) || RESERVED_OPS.contains(row),
                "{row} is not a request of protocol v1"
            );
        }
    }

    #[test]
    fn the_threat_model_names_the_same_user_boundary_and_the_file_a_later_run_adds() {
        for sentence in [
            "Until builds are signed, any process of the same user can read whatever a reply or a section carries and can send every request, a later run's included.",
            "crates/halvo-control/src/r2/SECURITY.md",
        ] {
            assert!(THREAT_MODEL.contains(sentence), "{sentence}");
        }
        assert!(THREAT_MODEL.ends_with('\n'));
        let last = THREAT_MODEL
            .lines()
            .rfind(|line| line.starts_with("## "))
            .unwrap();
        assert_eq!(last, "## Rows of a later run");
    }
}
