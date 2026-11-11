use std::fmt;

use halvo_schema::{AnalysisWrite, ErrorName, Instant, Position, StoreId};
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::record::{CaptureRecord, Kind, ServedKind};

/// The most bytes of the `detail` of an error (C5.10).
pub const DETAIL_MAX: usize = 256;

const NUMBER_MAX: u64 = i64::MAX as u64;

/// The zero position, `{"seg":0,"idx":0}`, which lies before every record.
pub const ZERO: Position = Position { seg: 0, idx: 0 };

struct ObjectOnly<D>(D);

impl<'de, D: Deserializer<'de>> Deserializer<'de> for ObjectOnly<D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.0.deserialize_map(MapOnly(visitor))
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

struct MapOnly<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for MapOnly<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.0.visit_map(map)
    }
}

fn object<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> Result<T, D::Error> {
    T::deserialize(ObjectOnly(deserializer))
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}

fn is_object(raw: &RawValue) -> bool {
    raw.get().starts_with('{')
}

fn in_range(p: Position) -> bool {
    p.seg <= NUMBER_MAX && p.idx <= NUMBER_MAX
}

/// The `kinds` filter of an `open_cursor`: 1 to 5 served kinds with no kind twice, in the order given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindFilter(Vec<ServedKind>);

impl KindFilter {
    /// The filter when `kinds` holds 1 to 5 kinds and none twice, or none.
    pub fn new(kinds: &[ServedKind]) -> Option<KindFilter> {
        let twice = kinds
            .iter()
            .enumerate()
            .any(|(i, k)| kinds[..i].contains(k));
        (!kinds.is_empty() && !twice).then(|| KindFilter(kinds.to_vec()))
    }

    /// The kinds, in the order given.
    pub fn kinds(&self) -> &[ServedKind] {
        &self.0
    }

    /// True when a record of `kind` passes the filter.
    pub fn admits(&self, kind: Kind) -> bool {
        kind.served().is_some_and(|k| self.0.contains(&k))
    }
}

/// The `range` of an `open_cursor`: a half-open range of instants whose `end` is after its `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeRange {
    start: Instant,
    end: Instant,
}

impl TimeRange {
    /// The range when `end` is after `start`, or none.
    pub fn new(start: Instant, end: Instant) -> Option<TimeRange> {
        (end > start).then_some(TimeRange { start, end })
    }

    /// The key `start`, inside the range.
    pub fn start(&self) -> Instant {
        self.start
    }

    /// The key `end`, outside the range.
    pub fn end(&self) -> Instant {
        self.end
    }

    /// True when `at` is at or after `start` and before `end`.
    pub fn contains(&self, at: Instant) -> bool {
        at >= self.start && at < self.end
    }
}

/// An `open_cursor` (C5.4). A null store goes only with the zero position, and a position is at most 2^63 - 1
/// in each part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenCursor {
    store: Option<StoreId>,
    position: Position,
    kinds: Option<KindFilter>,
    range: Option<TimeRange>,
}

impl OpenCursor {
    /// An open with a null store at the zero position, which asks for no check of the store.
    pub fn fresh(kinds: Option<KindFilter>, range: Option<TimeRange>) -> OpenCursor {
        OpenCursor {
            store: None,
            position: ZERO,
            kinds,
            range,
        }
    }

    /// An open at a position of the store `store`; none when a part of the position is above 2^63 - 1.
    pub fn at(
        store: StoreId,
        position: Position,
        kinds: Option<KindFilter>,
        range: Option<TimeRange>,
    ) -> Option<OpenCursor> {
        in_range(position).then_some(OpenCursor {
            store: Some(store),
            position,
            kinds,
            range,
        })
    }

    /// The key `store`.
    pub fn store(&self) -> Option<&StoreId> {
        self.store.as_ref()
    }

    /// The key `position`.
    pub fn position(&self) -> Position {
        self.position
    }

    /// The key `kinds`.
    pub fn kinds(&self) -> Option<&KindFilter> {
        self.kinds.as_ref()
    }

    /// The key `range`.
    pub fn range(&self) -> Option<TimeRange> {
        self.range
    }
}

/// A message of the client (C5.4). The request of an `analysis_write` is the bytes that `to_bytes()` of the
/// schema crate returned, held as a raw JSON object.
#[derive(Clone, Debug)]
pub enum ClientMessage {
    OpenCursor(OpenCursor),
    ReadBatch,
    Close,
    AnalysisWrite(Box<RawValue>),
}

#[derive(Serialize)]
struct RangeOut {
    start: i64,
    end: i64,
}

impl ClientMessage {
    /// The `analysis_write` of `write`, or the error name of `to_bytes()` when it refuses.
    pub fn analysis_write(write: &AnalysisWrite) -> Result<ClientMessage, ErrorName> {
        let bytes = write.to_bytes()?;
        let text = String::from_utf8(bytes).map_err(|_| ErrorName::Malformed)?;
        let raw = RawValue::from_string(text).map_err(|_| ErrorName::Malformed)?;
        if !is_object(&raw) {
            return Err(ErrorName::Malformed);
        }
        Ok(ClientMessage::AnalysisWrite(raw))
    }

    /// The payload of the message's frame: one compact JSON object with the keys in the order of C5.4.
    pub fn to_payload(&self) -> Vec<u8> {
        #[derive(Serialize)]
        struct Open<'a> {
            msg: &'static str,
            store: Option<&'a str>,
            position: Position,
            kinds: Option<Vec<&'static str>>,
            range: Option<RangeOut>,
        }
        #[derive(Serialize)]
        struct Write<'a> {
            msg: &'static str,
            request: &'a RawValue,
        }
        let written = match self {
            ClientMessage::OpenCursor(o) => serde_json::to_vec(&Open {
                msg: "open_cursor",
                store: o.store.as_ref().map(StoreId::as_str),
                position: o.position,
                kinds: o
                    .kinds
                    .as_ref()
                    .map(|k| k.0.iter().map(|k| k.as_str()).collect()),
                range: o.range.map(|r| RangeOut {
                    start: r.start.0,
                    end: r.end.0,
                }),
            }),
            ClientMessage::ReadBatch => Ok(br#"{"msg":"read_batch"}"#.to_vec()),
            ClientMessage::Close => Ok(br#"{"msg":"close"}"#.to_vec()),
            ClientMessage::AnalysisWrite(request) => serde_json::to_vec(&Write {
                msg: "analysis_write",
                request,
            }),
        };
        written.unwrap_or_default()
    }
}

/// Why a receiver of client messages refused a payload: `bad-frame` for one that is no message of C5.4, and
/// `bad-request` for a message of C5.4 with an illegal value (C5.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    BadFrame,
    BadRequest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeFields {
    start: i64,
    end: i64,
}

struct RawRange(RangeFields);

impl<'de> Deserialize<'de> for RawRange {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        object(deserializer).map(RawRange)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientFields {
    msg: String,
    #[serde(default, deserialize_with = "present")]
    store: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    position: Option<Option<Position>>,
    #[serde(default, deserialize_with = "present")]
    kinds: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "present")]
    range: Option<Option<RawRange>>,
    #[serde(default, deserialize_with = "present")]
    request: Option<Option<Box<RawValue>>>,
}

/// Reads the payload of a frame from the client. Accepts the keys of a message, of a `position` and of a
/// `range` in any order and JSON white space between tokens; refuses a key twice, an unknown or a missing key,
/// a key of another message, also with the value null, an array or any other value in place of an object, and an integer outside its form or its range with
/// `bad-frame`, and the illegal values of C5.10 with `bad-request`.
pub fn decode_client(payload: &[u8]) -> Result<ClientMessage, Refusal> {
    let mut de = serde_json::Deserializer::from_slice(payload);
    let fields: ClientFields = object(&mut de).map_err(|_| Refusal::BadFrame)?;
    de.end().map_err(|_| Refusal::BadFrame)?;
    let ClientFields {
        msg,
        store,
        position,
        kinds,
        range,
        request,
    } = fields;
    let only_msg = store.is_none()
        && position.is_none()
        && kinds.is_none()
        && range.is_none()
        && request.is_none();
    match msg.as_str() {
        "open_cursor" => {
            let (Some(store), Some(Some(position)), Some(kinds), Some(range), None) =
                (store, position, kinds, range, request)
            else {
                return Err(Refusal::BadFrame);
            };
            let kinds = match kinds {
                None => None,
                Some(names) => {
                    let parsed: Option<Vec<ServedKind>> =
                        names.iter().map(|n| ServedKind::parse(n)).collect();
                    Some(
                        parsed
                            .and_then(|k| KindFilter::new(&k))
                            .ok_or(Refusal::BadRequest)?,
                    )
                }
            };
            let range = match range {
                None => None,
                Some(RawRange(r)) => Some(
                    TimeRange::new(Instant(r.start), Instant(r.end)).ok_or(Refusal::BadRequest)?,
                ),
            };
            let open = match store {
                None if position == ZERO => OpenCursor::fresh(kinds, range),
                None => return Err(Refusal::BadRequest),
                Some(s) => {
                    let store = StoreId::parse(&s).map_err(|_| Refusal::BadRequest)?;
                    OpenCursor::at(store, position, kinds, range).ok_or(Refusal::BadFrame)?
                }
            };
            Ok(ClientMessage::OpenCursor(open))
        }
        "read_batch" if only_msg => Ok(ClientMessage::ReadBatch),
        "close" if only_msg => Ok(ClientMessage::Close),
        "analysis_write" => match (store, position, kinds, range, request) {
            (None, None, None, None, Some(Some(request))) if is_object(&request) => {
                Ok(ClientMessage::AnalysisWrite(request))
            }
            _ => Err(Refusal::BadFrame),
        },
        _ => Err(Refusal::BadFrame),
    }
}

/// One served record of a Batch with its position.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub pos: Position,
    pub record: CaptureRecord,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryFields {
    pos: Position,
    record: CaptureRecord,
}

impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let f: EntryFields = object(deserializer)?;
        Ok(Entry {
            pos: f.pos,
            record: f.record,
        })
    }
}

/// One entry of `expiry`: a segment ordinal and the instant at which the store destroys that segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Expiry {
    pub seg: u64,
    pub at: Instant,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpiryFields {
    seg: u64,
    at: i64,
}

impl<'de> Deserialize<'de> for Expiry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let f: ExpiryFields = object(deserializer)?;
        if f.seg > NUMBER_MAX {
            return Err(de::Error::custom("seg out of range"));
        }
        Ok(Expiry {
            seg: f.seg,
            at: Instant(f.at),
        })
    }
}

/// A Batch (C5.5, C5.7): the store read, the watermark, the served records in rising position order, one
/// expiry entry for each segment that held a record before any reduction, and the position of the next read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub store: StoreId,
    pub watermark: Position,
    pub records: Vec<Entry>,
    pub expiry: Vec<Expiry>,
    pub next: Position,
}

impl Batch {
    /// True when the watermark has the form of C5.3, `{"seg":H+1,"idx":0}` with H at least 0, and every
    /// record and every expiry entry names a segment ordinal of at least 1, since no segment has the ordinal 0.
    pub fn has_form(&self) -> bool {
        self.watermark.seg >= 1
            && self.watermark.idx == 0
            && self.records.iter().all(|e| e.pos.seg >= 1)
            && self.expiry.iter().all(|e| e.seg >= 1)
    }
}

/// The cause of an error (C5.10), a closed list in the order in which a server picks the first that fits.
/// Only `store-changed` holds a store id, the id of the server's current store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    BadFrame,
    BadRequest,
    StoreChanged(StoreId),
    BadPosition,
    StoreError,
}

impl Cause {
    /// The cause as an error writes it.
    pub fn name(&self) -> &'static str {
        match self {
            Cause::BadFrame => "bad-frame",
            Cause::BadRequest => "bad-request",
            Cause::StoreChanged(_) => "store-changed",
            Cause::BadPosition => "bad-position",
            Cause::StoreError => "store-error",
        }
    }
}

/// An `error` message: its cause and a fixed `detail` of at most `DETAIL_MAX` bytes that holds no value of
/// a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerError {
    cause: Cause,
    detail: String,
}

impl ServerError {
    /// The error, or none when `detail` has more than `DETAIL_MAX` bytes.
    pub fn new(cause: Cause, detail: &str) -> Option<ServerError> {
        (detail.len() <= DETAIL_MAX).then(|| ServerError {
            cause,
            detail: detail.to_owned(),
        })
    }

    pub(crate) fn bounded(cause: Cause, detail: &str) -> ServerError {
        let mut end = detail.len().min(DETAIL_MAX);
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        ServerError {
            cause,
            detail: detail[..end].to_owned(),
        }
    }

    /// The cause.
    pub fn cause(&self) -> &Cause {
        &self.cause
    }

    /// The key `detail`, which no client compares.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// A message of the server (C5.5). The reply of a `write_ack` is the bytes received, held as a raw JSON
/// object, for `request::decode_reply` of the schema crate.
#[derive(Clone, Debug)]
pub enum ServerMessage {
    Batch(Batch),
    Closed,
    WriteAck(Box<RawValue>),
    Error(ServerError),
}

/// A payload that is no message of C5.5, or a message that cannot be written as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageError;

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("no message of the seam protocol")
    }
}

impl std::error::Error for MessageError {}

impl ServerMessage {
    /// The payload of the message's frame: one compact JSON object with the keys in the order of C5.5. Fails
    /// when a position of a Batch is above 2^63 - 1 in a part.
    pub fn to_payload(&self) -> Result<Vec<u8>, MessageError> {
        #[derive(Serialize)]
        struct BatchOut<'a> {
            msg: &'static str,
            store: &'a str,
            watermark: Position,
            records: &'a [Entry],
            expiry: &'a [Expiry],
            next: Position,
        }
        #[derive(Serialize)]
        struct AckOut<'a> {
            msg: &'static str,
            reply: &'a RawValue,
        }
        #[derive(Serialize)]
        struct ErrorOut<'a> {
            msg: &'static str,
            cause: &'static str,
            store: Option<&'a str>,
            detail: &'a str,
        }
        let written = match self {
            ServerMessage::Batch(b) => serde_json::to_vec(&BatchOut {
                msg: "batch",
                store: b.store.as_str(),
                watermark: b.watermark,
                records: &b.records,
                expiry: &b.expiry,
                next: b.next,
            }),
            ServerMessage::Closed => Ok(br#"{"msg":"closed"}"#.to_vec()),
            ServerMessage::WriteAck(reply) => serde_json::to_vec(&AckOut {
                msg: "write_ack",
                reply,
            }),
            ServerMessage::Error(e) => serde_json::to_vec(&ErrorOut {
                msg: "error",
                cause: e.cause.name(),
                store: match &e.cause {
                    Cause::StoreChanged(s) => Some(s.as_str()),
                    _ => None,
                },
                detail: &e.detail,
            }),
        };
        written.map_err(|_| MessageError)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerFields {
    msg: String,
    #[serde(default, deserialize_with = "present")]
    store: Option<Option<StoreId>>,
    #[serde(default, deserialize_with = "present")]
    watermark: Option<Option<Position>>,
    #[serde(default, deserialize_with = "present")]
    records: Option<Option<Vec<Entry>>>,
    #[serde(default, deserialize_with = "present")]
    expiry: Option<Option<Vec<Expiry>>>,
    #[serde(default, deserialize_with = "present")]
    next: Option<Option<Position>>,
    #[serde(default, deserialize_with = "present")]
    reply: Option<Option<Box<RawValue>>>,
    #[serde(default, deserialize_with = "present")]
    cause: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    detail: Option<Option<String>>,
}

/// Reads the payload of a frame from the server by the rules of C5.5 and C5.10: keys in any order, no key
/// twice, no unknown or missing key and no key of another message, also with the value null, no array in place
/// of an object, each served record through the reader of C4.6, a Batch of the form of `Batch::has_form`, a
/// store id only on `store-changed`, and a detail of at most `DETAIL_MAX` bytes.
pub fn decode_server(payload: &[u8]) -> Result<ServerMessage, MessageError> {
    let mut de = serde_json::Deserializer::from_slice(payload);
    let f: ServerFields = object(&mut de).map_err(|_| MessageError)?;
    de.end().map_err(|_| MessageError)?;
    let ServerFields {
        msg,
        store,
        watermark,
        records,
        expiry,
        next,
        reply,
        cause,
        detail,
    } = f;
    match msg.as_str() {
        "batch" => match (
            store, watermark, records, expiry, next, reply, cause, detail,
        ) {
            (
                Some(Some(store)),
                Some(Some(watermark)),
                Some(Some(records)),
                Some(Some(expiry)),
                Some(Some(next)),
                None,
                None,
                None,
            ) => {
                let batch = Batch {
                    store,
                    watermark,
                    records,
                    expiry,
                    next,
                };
                if !batch.has_form() {
                    return Err(MessageError);
                }
                Ok(ServerMessage::Batch(batch))
            }
            _ => Err(MessageError),
        },
        "closed" => match (
            store, watermark, records, expiry, next, reply, cause, detail,
        ) {
            (None, None, None, None, None, None, None, None) => Ok(ServerMessage::Closed),
            _ => Err(MessageError),
        },
        "write_ack" => match (
            store, watermark, records, expiry, next, reply, cause, detail,
        ) {
            (None, None, None, None, None, Some(Some(reply)), None, None) if is_object(&reply) => {
                Ok(ServerMessage::WriteAck(reply))
            }
            _ => Err(MessageError),
        },
        "error" => match (
            store, watermark, records, expiry, next, reply, cause, detail,
        ) {
            (Some(store), None, None, None, None, None, Some(Some(cause)), Some(Some(detail))) => {
                let cause = match (cause.as_str(), store) {
                    ("bad-frame", None) => Cause::BadFrame,
                    ("bad-request", None) => Cause::BadRequest,
                    ("store-changed", Some(id)) => Cause::StoreChanged(id),
                    ("bad-position", None) => Cause::BadPosition,
                    ("store-error", None) => Cause::StoreError,
                    _ => return Err(MessageError),
                };
                ServerError::new(cause, &detail)
                    .map(ServerMessage::Error)
                    .ok_or(MessageError)
            }
            _ => Err(MessageError),
        },
        _ => Err(MessageError),
    }
}
