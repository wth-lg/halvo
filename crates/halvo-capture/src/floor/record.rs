//! The stored record (contracts.md C13.1): one JSON object on a line of the
//! segment payload, `{"v":1,"id":..,"at":..,"seq":..,"kind":..,"app":..,"p":{..}}`.
//! The encoder takes a received floor value alone and writes every captured
//! string through the text filter point that it is given; the strict decoder
//! returns a separate decoded value, which is data.

use std::fmt::{self, Write as _};
use std::str::FromStr;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use uuid::{Uuid, Variant};
use zeroize::Zeroizing;

use super::payload::{is_recorded, Button, FoldClass, KeyClass, Observation, TitleWhy};
use super::site::SITE_MAX_BYTES;
use super::text::{push_json_string, write_through, TextField, TextFilterPoint, TEXT_MAX_BYTES};
use super::{KEY_TIME_RESOLUTION, TIME_RESOLUTION};
use crate::event::{coarsened, FloorEvent, Modality};
use crate::sink::FloorReceived;

/// The shape version `v` of every record.
pub const SHAPE_VERSION: u64 = 1;

/// The kind text of the reserved kind `run_mark`, which the decoder compares
/// in every build and the codec offers to the owner's port.
pub(crate) const RUN_MARK_KIND: &str = "run_mark";

const LARGEST_SKELETON: &str = concat!(
    r#"{"v":1,"id":"","at":,"seq":,"kind":"window_title","app":,"#,
    r#""p":{"text":,"cut":false,"why":"title_change","#,
    r#""site":{"domain":,"source":"address_field"}}}"#
);
const UUID_TEXT_BYTES: usize = 36;
const LONGEST_I64_TEXT: usize = 20;
const LONGEST_U64_TEXT: usize = 20;
const LONGEST_ESCAPE: usize = 6;

/// The longest record that the encoder writes: a title record whose app id and
/// title hold [`TEXT_MAX_BYTES`] bytes that each take the longest JSON escape,
/// with a site of [`SITE_MAX_BYTES`], the longest `at` and the longest `seq`.
pub const LARGEST_RECORD_BYTES: usize = LARGEST_SKELETON.len()
    + UUID_TEXT_BYTES
    + LONGEST_I64_TEXT
    + LONGEST_U64_TEXT
    + 2 * (LONGEST_ESCAPE * TEXT_MAX_BYTES + 2)
    + (SITE_MAX_BYTES + 2);

/// A record would be longer than [`LARGEST_RECORD_BYTES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the record is {len} bytes, longer than the largest record of {max} bytes")]
pub struct RecordTooLong {
    pub len: usize,
    pub max: usize,
}

/// One encoded record with no line end. Its buffer is wiped when it drops, and
/// its `Debug` prints a placeholder.
pub struct EncodedRecord(Zeroizing<String>);

impl EncodedRecord {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for EncodedRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EncodedRecord(..)")
    }
}

/// The record of a received floor value. Each captured string goes to `point`
/// once, with its field, and the record holds what the point returned; a
/// title's `cut` is true when the text entry cut the title or the point
/// changed it.
pub fn encode<P: TextFilterPoint>(
    received: &FloorReceived,
    point: &P,
) -> Result<EncodedRecord, RecordTooLong> {
    let event = received.event();
    let mut out = Zeroizing::new(String::with_capacity(LARGEST_RECORD_BYTES));
    let shape = match event.observation() {
        Observation::ForegroundApp | Observation::ScreenFrame => Shape::Empty,
        Observation::WindowTitle(title) => Shape::Title {
            why: title.why,
            site: title.site.is_some(),
        },
        Observation::Keystroke(class) => Shape::Key(*class),
        Observation::Click(button) => Shape::Click(*button),
        Observation::Fold(fold) => Shape::Fold(fold.class(), fold.count().get()),
    };
    let head = Head {
        id: event.id().uuid(),
        at: event.at(),
        seq: received.number().get(),
        kind: event.kind().as_str(),
    };
    write_record(&mut out, &head, &shape, &mut ThroughPoint { event, point });
    if out.len() > LARGEST_RECORD_BYTES {
        return Err(RecordTooLong {
            len: out.len(),
            max: LARGEST_RECORD_BYTES,
        });
    }
    Ok(EncodedRecord(out))
}

struct Head<'a> {
    id: Uuid,
    at: i64,
    seq: u64,
    kind: &'a str,
}

/// What `p` holds, apart from its captured strings.
enum Shape {
    Empty,
    Title {
        why: TitleWhy,
        site: bool,
    },
    Key(KeyClass),
    Click(Button),
    Fold(FoldClass, u64),
    #[cfg(feature = "r2")]
    Free(Map<String, Value>),
}

/// The writer of the captured strings of one record.
trait Texts {
    /// Writes the app as a JSON string, or `null`.
    fn app(&mut self, out: &mut String);
    /// Writes the title text as a JSON string and gives the value of `cut`.
    fn title(&mut self, out: &mut String) -> bool;
    /// Writes the site's domain as a JSON string.
    fn site(&mut self, out: &mut String);
}

struct ThroughPoint<'a, P> {
    event: &'a FloorEvent,
    point: &'a P,
}

impl<P: TextFilterPoint> Texts for ThroughPoint<'_, P> {
    fn app(&mut self, out: &mut String) {
        match self.event.app().text() {
            Some((text, field)) => {
                write_through(text, field, self.point, out);
            }
            None => out.push_str("null"),
        }
    }

    fn title(&mut self, out: &mut String) -> bool {
        match self.event.observation() {
            Observation::WindowTitle(title) => {
                let changed = write_through(&title.text, TextField::Title, self.point, out);
                title.text.was_cut() || changed
            }
            _ => false,
        }
    }

    fn site(&mut self, out: &mut String) {
        if let Observation::WindowTitle(title) = self.event.observation() {
            if let Some(site) = &title.site {
                write_through(site.domain(), TextField::Site, self.point, out);
            }
        }
    }
}

/// The one writer of a record, for the encoder and for the decoder's check of
/// the form.
fn write_record(out: &mut String, head: &Head<'_>, shape: &Shape, texts: &mut impl Texts) {
    let _ = write!(
        out,
        "{{\"v\":{SHAPE_VERSION},\"id\":\"{}\",\"at\":{},\"seq\":{},\"kind\":\"{}\",\"app\":",
        head.id.hyphenated(),
        head.at,
        head.seq,
        head.kind
    );
    texts.app(out);
    out.push_str(",\"p\":");
    match shape {
        Shape::Empty => out.push_str("{}"),
        Shape::Title { why, site } => {
            out.push_str("{\"text\":");
            let cut = texts.title(out);
            let _ = write!(out, ",\"cut\":{cut},\"why\":\"{}\"", why.as_str());
            if *site {
                out.push_str(",\"site\":{\"domain\":");
                texts.site(out);
                out.push_str(",\"source\":\"address_field\"}");
            }
            out.push('}');
        }
        Shape::Key(class) => {
            let _ = write!(out, "{{\"class\":\"{}\"}}", class.as_str());
        }
        Shape::Click(button) => {
            let _ = write!(
                out,
                "{{\"class\":\"click\",\"button\":\"{}\"}}",
                button.as_str()
            );
        }
        Shape::Fold(class, count) => {
            let _ = write!(
                out,
                "{{\"class\":\"{}\",\"count\":{count}}}",
                class.as_str()
            );
        }
        #[cfg(feature = "r2")]
        Shape::Free(p) => out.push_str(&Value::Object(p.clone()).to_string()),
    }
    out.push('}');
}

/// A text of a decoded record: an app id or a title of at most
/// [`TEXT_MAX_BYTES`] bytes. Only the decoder makes one.
#[derive(Clone, PartialEq, Eq)]
pub struct DecodedText(String);

impl DecodedText {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The domain of a decoded site, of the shape that the cut keeps: 1 to
/// [`SITE_MAX_BYTES`] bytes of two or more labels of 1 to 63 bytes of `a` to
/// `z`, `0` to `9` and the hyphen, no hyphen at either end of a label, and a
/// last label that is not all digits. Only the decoder makes one.
#[derive(Clone, PartialEq, Eq)]
pub struct DecodedSite(String);

impl DecodedSite {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A record as the strict decoder read it. It is data: no route leads from it
/// into the sink. Its `Debug` prints a placeholder.
#[derive(Clone, PartialEq, Eq)]
pub struct DecodedRecord {
    id: Uuid,
    at: i64,
    seq: u64,
    app: Option<DecodedText>,
    payload: DecodedPayload,
}

/// The payload `p` of a decoded record, by kind.
#[derive(Clone, PartialEq, Eq)]
pub enum DecodedPayload {
    ForegroundApp,
    WindowTitle {
        text: DecodedText,
        cut: bool,
        why: TitleWhy,
        site: Option<DecodedSite>,
    },
    Keystroke(KeyClass),
    Click(Button),
    Fold {
        class: FoldClass,
        count: u64,
    },
    ScreenFrame,
    #[cfg(feature = "r2")]
    RunMark(Map<String, Value>),
}

impl DecodedRecord {
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The instant in signed unix nanoseconds.
    pub fn at(&self) -> i64 {
        self.at
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn app(&self) -> Option<&str> {
        self.app.as_ref().map(DecodedText::as_str)
    }

    pub fn payload(&self) -> &DecodedPayload {
        &self.payload
    }

    /// The kind text of the record.
    pub fn kind(&self) -> &'static str {
        match &self.payload {
            DecodedPayload::ForegroundApp => Modality::ForegroundApp.as_str(),
            DecodedPayload::WindowTitle { .. } => Modality::WindowTitle.as_str(),
            DecodedPayload::Keystroke(_) => Modality::Keystroke.as_str(),
            DecodedPayload::Click(_) | DecodedPayload::Fold { .. } => Modality::Pointer.as_str(),
            DecodedPayload::ScreenFrame => Modality::ScreenFrame.as_str(),
            #[cfg(feature = "r2")]
            DecodedPayload::RunMark(_) => RUN_MARK_KIND,
        }
    }
}

impl fmt::Debug for DecodedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DecodedText(..)")
    }
}

impl fmt::Debug for DecodedSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DecodedSite(..)")
    }
}

impl fmt::Debug for DecodedPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let class = match self {
            DecodedPayload::Keystroke(class) => class.as_str(),
            DecodedPayload::Click(button) => button.as_str(),
            DecodedPayload::Fold { class, .. } => class.as_str(),
            _ => "..",
        };
        write!(f, "DecodedPayload({class})")
    }
}

impl fmt::Debug for DecodedRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DecodedRecord({})", self.kind())
    }
}

/// The object of a record that held a refused key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Object {
    /// The record itself.
    Record,
    /// The payload `p`.
    Payload,
    /// The `site` of a title.
    Site,
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Object::Record => "the record",
            Object::Payload => "p",
            Object::Site => "site",
        })
    }
}

/// Why the decoder refused a line. No variant holds a text or a number of the
/// line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("the line is longer than the largest record")]
    LineTooLong,
    #[error("the line is not one JSON object")]
    NotAnObject,
    #[error("a key stands twice in one object of the record")]
    DuplicateField,
    #[error("{0} holds a field that is not in its shape")]
    UnknownField(Object),
    #[error("the record lacks the field {0}")]
    MissingField(&'static str),
    #[error("the field {0} has a value of the wrong form")]
    BadValue(&'static str),
    #[error("the record has a shape version other than 1")]
    Version,
    #[error("the kind run_mark is reserved")]
    ReservedKind,
    #[error("the kind {0} has no record")]
    UnrecordedKind(&'static str),
    #[error("the kind is not one of the kinds")]
    UnknownKind,
    #[error("the {0} is empty")]
    EmptyText(&'static str),
    #[error("the {0} is longer than its cap")]
    TextTooLong(&'static str),
    #[error("the site is not of the shape that the cut keeps")]
    BadSite,
    #[error("the instant is finer than the stored resolution of its kind")]
    FinerInstant,
    #[error("the line is not the form that the encoder writes")]
    NotEncoderForm,
}

const RECORD_FIELDS: [&str; 7] = ["v", "id", "at", "seq", "kind", "app", "p"];

/// The strict decoder. It refuses a line longer than [`LARGEST_RECORD_BYTES`],
/// a field more or less in any object, an id that is not an RFC 4122 version 7
/// UUID, an empty app, an app or a title above [`TEXT_MAX_BYTES`], a site that
/// the cut could not have kept, an instant finer than its kind's resolution,
/// and any line that is not byte for byte what the encoder writes for the
/// decoded value. It cannot see a short title that no filter saw, or a domain
/// of the right shape that is not the page's.
pub fn decode(line: &str) -> Result<DecodedRecord, DecodeError> {
    if line.len() > LARGEST_RECORD_BYTES {
        return Err(DecodeError::LineTooLong);
    }
    let mut record = match serde_json::from_str::<Unique>(line) {
        Ok(Unique(Value::Object(record))) => record,
        Ok(_) => return Err(DecodeError::NotAnObject),
        Err(error) if error.to_string().starts_with(DUPLICATE) => {
            return Err(DecodeError::DuplicateField)
        }
        Err(_) => return Err(DecodeError::NotAnObject),
    };
    exact_fields(&record, Object::Record, &RECORD_FIELDS, &[])?;
    if take_u64(&mut record, "v")? != SHAPE_VERSION {
        return Err(DecodeError::Version);
    }
    let id = take_string(&mut record, "id")?;
    let id = Uuid::parse_str(&id).map_err(|_| DecodeError::BadValue("id"))?;
    if id.get_version_num() != 7 || id.get_variant() != Variant::RFC4122 {
        return Err(DecodeError::BadValue("id"));
    }
    let at = match record.remove("at") {
        Some(Value::Number(number)) => number.as_i64().ok_or(DecodeError::BadValue("at"))?,
        _ => return Err(DecodeError::BadValue("at")),
    };
    let seq = take_u64(&mut record, "seq")?;
    let kind = take_string(&mut record, "kind")?;
    let app = match record.remove("app") {
        Some(Value::Null) => None,
        Some(Value::String(app)) if app.is_empty() => return Err(DecodeError::EmptyText("app")),
        Some(Value::String(app)) => Some(capped(app, "app")?),
        _ => return Err(DecodeError::BadValue("app")),
    };
    let Some(Value::Object(p)) = record.remove("p") else {
        return Err(DecodeError::BadValue("p"));
    };
    let payload = decoded_payload(&kind, app.is_none(), p)?;
    let resolution = match payload {
        DecodedPayload::Keystroke(_) | DecodedPayload::Click(_) => KEY_TIME_RESOLUTION,
        _ => TIME_RESOLUTION,
    };
    if coarsened(at, resolution) != at {
        return Err(DecodeError::FinerInstant);
    }
    let decoded = DecodedRecord {
        id,
        at,
        seq,
        app,
        payload,
    };
    if encoder_form(&decoded).as_str() != line {
        return Err(DecodeError::NotEncoderForm);
    }
    Ok(decoded)
}

fn capped(text: String, field: &'static str) -> Result<DecodedText, DecodeError> {
    if text.len() > TEXT_MAX_BYTES {
        return Err(DecodeError::TextTooLong(field));
    }
    Ok(DecodedText(text))
}

fn site_of_the_cut(domain: String) -> Result<DecodedSite, DecodeError> {
    let label = |label: &str| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    let labels: Vec<&str> = domain.split('.').collect();
    let shaped = (1..=SITE_MAX_BYTES).contains(&domain.len())
        && labels.len() >= 2
        && labels.iter().all(|l| label(l))
        && !labels
            .last()
            .is_some_and(|last| last.bytes().all(|b| b.is_ascii_digit()));
    if shaped {
        Ok(DecodedSite(domain))
    } else {
        Err(DecodeError::BadSite)
    }
}

/// What the encoder writes for `record`, in a buffer that is wiped when it
/// drops.
fn encoder_form(record: &DecodedRecord) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(LARGEST_RECORD_BYTES));
    let shape = match &record.payload {
        DecodedPayload::ForegroundApp | DecodedPayload::ScreenFrame => Shape::Empty,
        DecodedPayload::WindowTitle { why, site, .. } => Shape::Title {
            why: *why,
            site: site.is_some(),
        },
        DecodedPayload::Keystroke(class) => Shape::Key(*class),
        DecodedPayload::Click(button) => Shape::Click(*button),
        DecodedPayload::Fold { class, count } => Shape::Fold(*class, *count),
        #[cfg(feature = "r2")]
        DecodedPayload::RunMark(p) => Shape::Free(p.clone()),
    };
    let head = Head {
        id: record.id,
        at: record.at,
        seq: record.seq,
        kind: record.kind(),
    };
    write_record(&mut out, &head, &shape, &mut AsDecoded(record));
    out
}

struct AsDecoded<'a>(&'a DecodedRecord);

impl Texts for AsDecoded<'_> {
    fn app(&mut self, out: &mut String) {
        match &self.0.app {
            Some(app) => push_json_string(out, app.as_str()),
            None => out.push_str("null"),
        }
    }

    fn title(&mut self, out: &mut String) -> bool {
        match &self.0.payload {
            DecodedPayload::WindowTitle { text, cut, .. } => {
                push_json_string(out, text.as_str());
                *cut
            }
            _ => false,
        }
    }

    fn site(&mut self, out: &mut String) {
        if let DecodedPayload::WindowTitle {
            site: Some(site), ..
        } = &self.0.payload
        {
            push_json_string(out, site.as_str());
        }
    }
}

const DUPLICATE: &str = "a key stands twice";

/// A JSON value read with every key of each object at most once.
struct Unique(Value);

impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Unique, D::Error> {
        deserializer.deserialize_any(UniqueVisitor).map(Unique)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value with no key twice in one object")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Ok(Value::from(value))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(Unique(item)) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom(DUPLICATE));
            }
            let Unique(value) = map.next_value()?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}

fn decoded_payload(
    kind: &str,
    app_is_null: bool,
    mut p: Map<String, Value>,
) -> Result<DecodedPayload, DecodeError> {
    if kind == RUN_MARK_KIND {
        return run_mark_payload(app_is_null, p);
    }
    let modality = Modality::from_str(kind).map_err(|_| DecodeError::UnknownKind)?;
    if !is_recorded(modality) {
        return Err(DecodeError::UnrecordedKind(modality.as_str()));
    }
    match modality {
        Modality::ForegroundApp | Modality::ScreenFrame => {
            exact_fields(&p, Object::Payload, &[], &[])?;
            Ok(if modality == Modality::ForegroundApp {
                DecodedPayload::ForegroundApp
            } else {
                DecodedPayload::ScreenFrame
            })
        }
        Modality::WindowTitle => {
            exact_fields(&p, Object::Payload, &["text", "cut", "why"], &["site"])?;
            let text = capped(take_string(&mut p, "text")?, "text")?;
            let cut = match p.remove("cut") {
                Some(Value::Bool(cut)) => cut,
                _ => return Err(DecodeError::BadValue("cut")),
            };
            let why = take_string(&mut p, "why")?;
            let why = [TitleWhy::AppChange, TitleWhy::TitleChange]
                .into_iter()
                .find(|known| known.as_str() == why)
                .ok_or(DecodeError::BadValue("why"))?;
            let site = match p.remove("site") {
                None => None,
                Some(Value::Object(mut site)) => {
                    exact_fields(&site, Object::Site, &["domain", "source"], &[])?;
                    if take_string(&mut site, "source")? != "address_field" {
                        return Err(DecodeError::BadValue("source"));
                    }
                    Some(site_of_the_cut(take_string(&mut site, "domain")?)?)
                }
                Some(_) => return Err(DecodeError::BadValue("site")),
            };
            Ok(DecodedPayload::WindowTitle {
                text,
                cut,
                why,
                site,
            })
        }
        Modality::Keystroke => {
            exact_fields(&p, Object::Payload, &["class"], &[])?;
            let class = take_string(&mut p, "class")?;
            KeyClass::ALL
                .into_iter()
                .find(|known| known.as_str() == class)
                .map(DecodedPayload::Keystroke)
                .ok_or(DecodeError::BadValue("class"))
        }
        Modality::Pointer => {
            let class = match p.get("class") {
                Some(Value::String(class)) => class.clone(),
                _ => return Err(DecodeError::BadValue("class")),
            };
            if class == "click" {
                exact_fields(&p, Object::Payload, &["class", "button"], &[])?;
                let button = take_string(&mut p, "button")?;
                return Button::ALL
                    .into_iter()
                    .find(|known| known.as_str() == button)
                    .map(DecodedPayload::Click)
                    .ok_or(DecodeError::BadValue("button"));
            }
            exact_fields(&p, Object::Payload, &["class", "count"], &[])?;
            let class = FoldClass::ALL
                .into_iter()
                .find(|known| known.as_str() == class)
                .ok_or(DecodeError::BadValue("class"))?;
            let count = take_u64(&mut p, "count")?;
            if count == 0 {
                return Err(DecodeError::BadValue("count"));
            }
            Ok(DecodedPayload::Fold { class, count })
        }
        Modality::SystemAudio | Modality::Microphone | Modality::Camera => {
            Err(DecodeError::UnrecordedKind(modality.as_str()))
        }
    }
}

#[cfg(not(feature = "r2"))]
fn run_mark_payload(
    _app_is_null: bool,
    _p: Map<String, Value>,
) -> Result<DecodedPayload, DecodeError> {
    Err(DecodeError::ReservedKind)
}

#[cfg(feature = "r2")]
fn run_mark_payload(
    app_is_null: bool,
    p: Map<String, Value>,
) -> Result<DecodedPayload, DecodeError> {
    if !app_is_null {
        return Err(DecodeError::BadValue("app"));
    }
    Ok(DecodedPayload::RunMark(p))
}

fn exact_fields(
    object: &Map<String, Value>,
    which: Object,
    required: &[&'static str],
    optional: &[&'static str],
) -> Result<(), DecodeError> {
    if object
        .keys()
        .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(DecodeError::UnknownField(which));
    }
    match required.iter().find(|key| !object.contains_key(**key)) {
        Some(missing) => Err(DecodeError::MissingField(missing)),
        None => Ok(()),
    }
}

fn take_string(object: &mut Map<String, Value>, key: &'static str) -> Result<String, DecodeError> {
    match object.remove(key) {
        Some(Value::String(text)) => Ok(text),
        _ => Err(DecodeError::BadValue(key)),
    }
}

fn take_u64(object: &mut Map<String, Value>, key: &'static str) -> Result<u64, DecodeError> {
    match object.remove(key) {
        Some(Value::Number(number)) => number.as_u64().ok_or(DecodeError::BadValue(key)),
        _ => Err(DecodeError::BadValue(key)),
    }
}

/// A record of the reserved kind `run_mark`: the record seam of contracts.md
/// C18 item 7. R1 gives the kind no payload and no meaning.
#[cfg(feature = "r2")]
pub(crate) struct RunMark {
    id: crate::event::EventId,
    at: i64,
    p: Map<String, Value>,
}

#[cfg(feature = "r2")]
impl RunMark {
    /// A mark at the instant `at` of the request with the object `p`, both from
    /// the caller. Its id is made from `at` as the id of an event is, and its
    /// app is null. A mark whose record would be longer than
    /// [`LARGEST_RECORD_BYTES`] with the longest `seq` is refused.
    pub(crate) fn new(
        at: std::time::SystemTime,
        p: Map<String, Value>,
    ) -> Result<RunMark, RecordTooLong> {
        let resolution = super::TIME_RESOLUTION;
        let at = crate::event::coarsened(crate::event::unix_nanos(at), resolution);
        let id = crate::event::EventClock::system().id_at(at, resolution);
        let mark = RunMark { id, at, p };
        let len = mark.line(u64::MAX).len();
        if len > LARGEST_RECORD_BYTES {
            return Err(RecordTooLong {
                len,
                max: LARGEST_RECORD_BYTES,
            });
        }
        Ok(mark)
    }

    /// The record of the mark with the number that the receiver gave it.
    pub(crate) fn encode(&self, number: crate::sink::ArrivalNumber) -> EncodedRecord {
        EncodedRecord(self.line(number.get()))
    }

    fn line(&self, seq: u64) -> Zeroizing<String> {
        let mut out = Zeroizing::new(String::new());
        let head = Head {
            id: self.id.uuid(),
            at: self.at,
            seq,
            kind: RUN_MARK_KIND,
        };
        write_record(&mut out, &head, &Shape::Free(self.p.clone()), &mut NoTexts);
        out
    }
}

/// The captured strings of a record that holds none: a run mark.
#[cfg(feature = "r2")]
struct NoTexts;

#[cfg(feature = "r2")]
impl Texts for NoTexts {
    fn app(&mut self, out: &mut String) {
        out.push_str("null");
    }

    fn title(&mut self, _out: &mut String) -> bool {
        false
    }

    fn site(&mut self, _out: &mut String) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AppIdentity, EventClock, FloorEvent, FoldEvent};
    use crate::floor::cases::{keep, IdentityPoint, LongestPoint, RecordingPoint};
    use crate::floor::payload::{FoldCount, PointerFold, Title};
    use crate::floor::site::Site;
    use crate::secure_input::{check, FixedProbe, SecureInputReading};
    use crate::sink::{ArrivalNumber, FloorReceived};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const ID: &str = "0190a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2b";

    fn line(kind: &str, app: &str, p: &str) -> String {
        format!(
            r#"{{"v":1,"id":"{ID}","at":-5000000000,"seq":3,"kind":"{kind}","app":{app},"p":{p}}}"#
        )
    }

    fn title(text: &str, site: &str) -> String {
        let site = if site.is_empty() {
            String::new()
        } else {
            format!(r#","site":{{"domain":{site},"source":"address_field"}}"#)
        };
        line(
            "window_title",
            r#""com.apple.Safari""#,
            &format!(r#"{{"text":{text},"cut":false,"why":"app_change"{site}}}"#),
        )
    }

    #[test]
    fn the_decoder_refuses_every_form_that_the_encoder_never_writes() {
        const ID: &str = "0190a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2b";
        let base = |kind: &str, at: &str, app: &str, p: &str| {
            format!(
                r#"{{"v":1,"id":"{ID}","at":{at},"seq":3,"kind":"{kind}","app":{app},"p":{p}}}"#
            )
        };
        let title = |text: String, site: Option<String>| {
            let site = site.map_or(String::new(), |domain| {
                format!(r#","site":{{"domain":"{domain}","source":"address_field"}}"#)
            });
            base(
                "window_title",
                "-5000000000",
                r#""com.apple.Safari""#,
                &format!(r#"{{"text":"{text}","cut":false,"why":"app_change"{site}}}"#),
            )
        };
        let app = |app: String| {
            base(
                "foreground_app",
                "-5000000000",
                &format!(r#""{app}""#),
                "{}",
            )
        };
        let site = |domain: &str| title("Inbox".to_string(), Some(domain.to_string()));
        let good = base("keystroke", "-5000000000", "null", r#"{"class":"tab"}"#);
        let with_id = |id: &str| good.replacen(ID, id, 1);
        let mut forms: Vec<(&str, String)> = vec![
            ("title of 1,025 bytes", title("a".repeat(1025), None)),
            (
                "title of 1,025 bytes in two-byte letters",
                title(format!("{}a", "\u{e9}".repeat(512)), None),
            ),
            ("title of 100,000 bytes", title("a".repeat(100_000), None)),
            (
                "title of four times the largest record",
                title("a".repeat(4 * 12_771), None),
            ),
            ("app of 1,025 bytes", app("a".repeat(1025))),
            ("app of 5,000 bytes", app("a".repeat(5000))),
            ("an empty app", app(String::new())),
            ("site with a path", site("example.com/inbox")),
            ("site with a port", site("example.com:8080")),
            ("site with a user", site("jane@example.com")),
            (
                "site with a user and a password",
                site("jane:pw@example.com"),
            ),
            ("site with a query", site("example.com?q=1")),
            ("site with a fragment", site("example.com#top")),
            (
                "site that is a full url",
                site("https://jane:pw@www.example.com:8080/a?b=c#d"),
            ),
            ("site in upper case", site("Example.com")),
            ("site of one label", site("example")),
            ("site that is empty", site("")),
            ("site with a space", site("exa mple.com")),
            ("site with a nul", site("exa\\u0000mple.com")),
            ("site with a byte above ascii", site("b\u{fc}cher.de")),
            (
                "site of 254 bytes",
                site(&format!(
                    "{}.{}.{}.{}.com",
                    "a".repeat(63),
                    "b".repeat(63),
                    "c".repeat(63),
                    "d".repeat(58)
                )),
            ),
            ("site with a trailing dot", site("example.com.")),
            ("site in brackets", site("[::1]")),
            ("site that is localhost", site("localhost")),
            ("site that is a dotted number", site("192.168.1.10")),
            ("site with a backslash", site("example.com\\\\x")),
            (
                "site with a label of 64 bytes",
                site(&format!("{}.com", "a".repeat(64))),
            ),
            ("site with a hyphen at a label's end", site("example-.com")),
            (
                "line longer than the largest record by padding",
                format!("{good}{}", " ".repeat(12_771)),
            ),
            (
                "line longer than the largest record by escapes",
                title("\\u0061".repeat(2200), None),
            ),
            ("id in upper case", with_id(&ID.to_uppercase())),
            ("id in braces", with_id(&format!("{{{ID}}}"))),
            ("id as a urn", with_id(&format!("urn:uuid:{ID}"))),
            ("id without hyphens", with_id(&ID.replace('-', ""))),
            (
                "id of another variant",
                with_id("0190a1b2-c3d4-7e5f-ca6b-7c8d9e0f1a2b"),
            ),
            (
                "keystroke instant finer than 1,000 ms",
                base("keystroke", "-5000000001", "null", r#"{"class":"tab"}"#),
            ),
            (
                "click instant finer than 1,000 ms",
                base(
                    "pointer",
                    "1800000000500000000",
                    "null",
                    r#"{"class":"click","button":"primary"}"#,
                ),
            ),
            (
                "title instant finer than 1 ms",
                base(
                    "window_title",
                    "1800000000000000001",
                    "null",
                    r#"{"text":"t","cut":false,"why":"app_change"}"#,
                ),
            ),
            ("a space after the object", format!("{good} ")),
            ("a tab after the object", format!("{good}\t")),
            ("a carriage return after the object", format!("{good}\r")),
            ("a line feed after the object", format!("{good}\n")),
            ("a space before the object", format!(" {good}")),
            (
                "a space between tokens",
                good.replacen(r#""v":1"#, r#""v": 1"#, 1),
            ),
            (
                "a line feed between tokens",
                good.replacen(r#","seq""#, "\n,\"seq\"", 1),
            ),
            (
                "a carriage return between tokens",
                good.replacen(r#","seq""#, "\r,\"seq\"", 1),
            ),
            (
                "keys in another order",
                good.replacen(r#""v":1,"id":"#, r#""id":"#, 1).replacen(
                    r#""seq":3,"#,
                    r#""seq":3,"v":1,"#,
                    1,
                ),
            ),
            ("an escaped slash", title("a\\/b".to_string(), None)),
            (
                "a plain letter as an escape",
                title("\\u0061b".to_string(), None),
            ),
        ];
        forms.push(("an upper-case escape", title("\\u001F".to_string(), None)));
        let taken: Vec<&str> = forms
            .iter()
            .filter(|(_, line)| decode(line).is_ok())
            .map(|(name, _)| *name)
            .collect();
        assert!(decode(&good).is_ok());
        assert_eq!(taken, Vec::<&str>::new(), "forms that the decoder took");
    }

    #[test]
    fn each_refused_form_names_its_rule() {
        let refused = |line: &str| decode(line).expect_err("the line was taken");
        let good = line("keystroke", "null", r#"{"class":"tab"}"#);
        assert!(decode(&good).is_ok());
        let padded = format!("{good}{}", " ".repeat(LARGEST_RECORD_BYTES));
        assert_eq!(refused(&padded), DecodeError::LineTooLong);
        let long_title = title(&format!("\"{}\"", "a".repeat(TEXT_MAX_BYTES + 1)), "");
        assert_eq!(refused(&long_title), DecodeError::TextTooLong("text"));
        let at_cap = title(&format!("\"{}\"", "a".repeat(TEXT_MAX_BYTES)), "");
        assert!(decode(&at_cap).is_ok());
        let long_app = line(
            "foreground_app",
            &format!("\"{}\"", "a".repeat(TEXT_MAX_BYTES + 1)),
            "{}",
        );
        assert_eq!(refused(&long_app), DecodeError::TextTooLong("app"));
        assert_eq!(
            refused(&line("foreground_app", r#""""#, "{}")),
            DecodeError::EmptyText("app")
        );
        assert!(decode(&title(r#""""#, "")).is_ok());
        for domain in [
            "example.com/x",
            "Example.com",
            "example",
            "192.168.1.10",
            "a-.com",
            "-a.com",
            "example..com",
        ] {
            let site = title(r#""Inbox""#, &format!("\"{domain}\""));
            assert_eq!(refused(&site), DecodeError::BadSite, "{domain}");
        }
        let longest = format!(
            "{}.{}.{}.{}.com",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(57)
        );
        assert_eq!(longest.len(), SITE_MAX_BYTES);
        assert!(decode(&title(r#""Inbox""#, &format!("\"{longest}\""))).is_ok());
        assert!(decode(&title(r#""Inbox""#, r#""x-1.co.uk""#)).is_ok());
        let fine = good.replacen("-5000000000", "-5000000001", 1);
        assert_eq!(refused(&fine), DecodeError::FinerInstant);
        let frame = line("screen_frame", "null", "{}").replacen("-5000000000", "-5001000000", 1);
        assert!(decode(&frame).is_ok());
        let frame_fine = frame.replacen("-5001000000", "-5001000001", 1);
        assert_eq!(refused(&frame_fine), DecodeError::FinerInstant);
        let variant = good.replacen("-8a6b-", "-ca6b-", 1);
        assert_eq!(refused(&variant), DecodeError::BadValue("id"));
        for form in [
            good.replacen(ID, &ID.to_uppercase(), 1),
            good.replacen(ID, &format!("urn:uuid:{ID}"), 1),
            format!("{good} "),
            good.replacen(r#""v":1"#, r#""v": 1"#, 1),
            title(r#""a\/b""#, ""),
            title(r#""\u0061""#, ""),
            title(r#""\u001F""#, ""),
        ] {
            assert_eq!(refused(&form), DecodeError::NotEncoderForm, "{form}");
        }
    }

    #[test]
    fn the_payload_of_each_kind_refuses_a_field_more() {
        let unknown = Err(DecodeError::UnknownField(Object::Payload));
        let cases = [
            ("keystroke", r#"{"class":"tab","key_code":48}"#),
            ("screen_frame", r#"{"pixels":"AAAA"}"#),
            ("foreground_app", r#"{"pixels":"AAAA"}"#),
            ("pointer", r#"{"class":"click","button":"primary","x":10}"#),
            ("pointer", r#"{"class":"move","count":3,"dx":4}"#),
            (
                "window_title",
                r#"{"text":"t","cut":false,"why":"app_change","len":1}"#,
            ),
        ];
        for (kind, p) in cases {
            assert_eq!(decode(&line(kind, "null", p)), unknown, "{kind} {p}");
        }
        assert_eq!(
            decode(&line("keystroke", "null", r#"{"class":"tab"}"#).replacen(
                r#""v":1,"#,
                r#""v":1,"len":4,"#,
                1
            )),
            Err(DecodeError::UnknownField(Object::Record))
        );
        let site = r#"{"text":"t","cut":false,"why":"app_change","site":{"domain":"example.com","source":"address_field","path":"/x"}}"#;
        assert_eq!(
            decode(&line("window_title", "null", site)),
            Err(DecodeError::UnknownField(Object::Site))
        );
    }

    #[test]
    fn a_site_of_another_source_is_refused() {
        let site = |source: &str| {
            line(
                "window_title",
                "null",
                &format!(
                    r#"{{"text":"t","cut":false,"why":"app_change","site":{{"domain":"example.com","source":"{source}"}}}}"#
                ),
            )
        };
        assert!(decode(&site("address_field")).is_ok());
        for source in ["page_title", "tab_url", "", "Address_field"] {
            assert_eq!(
                decode(&site(source)),
                Err(DecodeError::BadValue("source")),
                "{source}"
            );
        }
    }

    #[test]
    fn the_decoder_rejects_an_unknown_field() {
        let good = line("keystroke", "null", r#"{"class":"tab"}"#);
        let decoded = decode(&good).unwrap();
        assert_eq!(decoded.payload(), &DecodedPayload::Keystroke(KeyClass::Tab));
        assert_eq!(
            (decoded.at(), decoded.seq(), decoded.app()),
            (-5_000_000_000, 3, None)
        );
        let missing = good.replacen(r#""seq":3,"#, "", 1);
        assert_eq!(decode(&missing), Err(DecodeError::MissingField("seq")));
        let version = good.replacen(r#""v":1"#, r#""v":2"#, 1);
        assert_eq!(decode(&version), Err(DecodeError::Version));
        let random = uuid::Builder::from_random_bytes([7; 16]).into_uuid();
        let v4 = good.replacen(ID, &random.hyphenated().to_string(), 1);
        assert_eq!(decode(&v4), Err(DecodeError::BadValue("id")));
        assert_eq!(
            decode(&line("camera", "null", "{}")),
            Err(DecodeError::UnrecordedKind("camera"))
        );
        assert_eq!(
            decode(&line("clipboard", "null", "{}")),
            Err(DecodeError::UnknownKind)
        );
        let fold = r#"{"class":"move","count":0}"#;
        assert_eq!(
            decode(&line("pointer", "null", fold)),
            Err(DecodeError::BadValue("count"))
        );
        assert_eq!(
            decode(&line("keystroke", "null", r#"{"class":"a"}"#)),
            Err(DecodeError::BadValue("class"))
        );
        let float = good.replacen(r#""at":-5000000000"#, r#""at":-5.5"#, 1);
        assert_eq!(decode(&float), Err(DecodeError::BadValue("at")));
        assert_eq!(decode("[1]"), Err(DecodeError::NotAnObject));
        let twice_v = good.replacen(r#""v":1,"#, r#""v":1,"v":1,"#, 1);
        assert_eq!(decode(&twice_v), Err(DecodeError::DuplicateField));
        let twice_app = good.replacen(r#""app":null"#, r#""app":"a","app":null"#, 1);
        assert_eq!(decode(&twice_app), Err(DecodeError::DuplicateField));
        let twice_text = r#"{"text":"raw","text":"t","cut":false,"why":"app_change"}"#;
        assert_eq!(
            decode(&line("window_title", "null", twice_text)),
            Err(DecodeError::DuplicateField)
        );
        let twice_domain = r#"{"text":"t","cut":false,"why":"app_change","site":{"domain":"a.com","domain":"example.com","source":"address_field"}}"#;
        assert_eq!(
            decode(&line("window_title", "null", twice_domain)),
            Err(DecodeError::DuplicateField)
        );
        assert_eq!(decode("{\"v\":"), Err(DecodeError::NotAnObject));
        assert_eq!(format!("{decoded:?}"), "DecodedRecord(keystroke)");
    }

    #[test]
    fn no_refusal_shows_a_planted_marker() {
        let marker = "PlantedMarkerZq7Xw9Kd";
        let digits = "8273645190827364519";
        let line = |p: &str| {
            format!(
                r#"{{"v":1,"id":"0190a1b2-c3d4-7e5f-8a6b-7c8d9e0f1a2b","at":-5000000000,"seq":3,"kind":"keystroke","app":null,"p":{p}}}"#
            )
        };
        let refused = [
            line(r#"{"class":"tab"}"#).replacen(r#""v":1,"#, &format!(r#""v":1,"{marker}":4,"#), 1),
            line(&format!(r#"{{"class":"tab","{marker}":1}}"#)),
            line(r#"{"class":"tab"}"#).replacen(r#""v":1"#, &format!(r#""v":{digits}"#), 1),
        ];
        let mut shown = Vec::new();
        for form in &refused {
            let error = decode(form).expect_err("refused");
            for text in [
                error.to_string(),
                format!("{error:?}"),
                format!("{error:#?}"),
            ] {
                if text.contains(marker) || text.contains(digits) {
                    shown.push(text);
                }
            }
        }
        assert_eq!(shown, Vec::<String>::new());
    }

    #[test]
    fn no_error_holds_a_text_or_a_number_of_the_line() {
        let marker = "PlantedMarkerZq7Xw9Kd";
        assert!(marker.len() >= 20);
        let digits = "8273645190827364519";
        let unknown_key = [
            line("keystroke", "null", r#"{"class":"tab"}"#).replacen(
                r#""v":1,"#,
                &format!(r#""v":1,"{marker}":4,"#),
                1,
            ),
            line(
                "keystroke",
                "null",
                &format!(r#"{{"class":"tab","{marker}":1}}"#),
            ),
            line(
                "window_title",
                "null",
                &format!(
                    r#"{{"text":"t","cut":false,"why":"app_change","site":{{"domain":"example.com","source":"address_field","{marker}":1}}}}"#
                ),
            ),
        ];
        let version = line("keystroke", "null", r#"{"class":"tab"}"#).replacen(
            r#""v":1"#,
            &format!(r#""v":{digits}"#),
            1,
        );
        let mut errors = Vec::new();
        for refused in unknown_key.iter().chain([&version]) {
            errors.push(decode(refused).expect_err("refused"));
        }
        assert_eq!(errors[0], DecodeError::UnknownField(Object::Record));
        assert_eq!(errors[1], DecodeError::UnknownField(Object::Payload));
        assert_eq!(errors[2], DecodeError::UnknownField(Object::Site));
        assert_eq!(errors[3], DecodeError::Version);
        for error in &errors {
            for shown in [
                error.to_string(),
                format!("{error:?}"),
                format!("{error:#?}"),
            ] {
                assert!(!shown.contains(marker), "{shown}");
                assert!(!shown.contains(digits), "{shown}");
            }
        }
        assert_eq!(
            errors[0].to_string(),
            "the record holds a field that is not in its shape"
        );
        assert_eq!(
            errors[1].to_string(),
            "p holds a field that is not in its shape"
        );
        assert_eq!(
            errors[2].to_string(),
            "site holds a field that is not in its shape"
        );
    }

    fn smallest() -> SystemTime {
        UNIX_EPOCH - Duration::from_nanos(i64::MIN.unsigned_abs())
    }

    fn longest_title(point_title_bytes: usize) -> (usize, Result<EncodedRecord, RecordTooLong>) {
        let clock = EventClock::new(smallest);
        let control = "\u{1}".repeat(2 * TEXT_MAX_BYTES);
        let app = AppIdentity::executable_path(control.clone());
        let site = Site::cut("https://example.com/".to_string());
        let title = Title::new(
            control[..TEXT_MAX_BYTES].to_string(),
            TitleWhy::TitleChange,
            site,
        );
        let event = FloorEvent::window_title(keep(app, 7), title, None, &clock);
        let at = event.at();
        let received = FloorReceived::for_test(event, ArrivalNumber::for_test(u64::MAX));
        let record = if point_title_bytes == TEXT_MAX_BYTES {
            encode(&received, &LongestPoint)
        } else {
            encode(
                &received,
                &crate::floor::cases::Lengthening(point_title_bytes),
            )
        };
        (at.to_string().len(), record)
    }

    #[test]
    fn no_record_is_longer_than_the_named_constant() {
        let (at_digits, record) = longest_title(TEXT_MAX_BYTES);
        assert_eq!(at_digits, LONGEST_I64_TEXT);
        let record = record.unwrap();
        assert_eq!(record.len(), LARGEST_RECORD_BYTES);
        assert_eq!(LARGEST_RECORD_BYTES, 12_771);
        assert!(decode(record.as_str()).is_ok());
        let (_, longer) = longest_title(TEXT_MAX_BYTES + 1);
        let refused = longer.expect_err("a record over the constant is refused");
        assert_eq!(
            refused,
            RecordTooLong {
                len: LARGEST_RECORD_BYTES + 6 - 1,
                max: LARGEST_RECORD_BYTES
            }
        );
        let point = RecordingPoint::new();
        let app = AppIdentity::bundle_id("\u{1}".repeat(TEXT_MAX_BYTES));
        let event = FloorEvent::foreground_app(keep(app, 7), None, &EventClock::new(smallest));
        let received = FloorReceived::for_test(event, ArrivalNumber::for_test(u64::MAX));
        assert!(encode(&received, &point).unwrap().len() < LARGEST_RECORD_BYTES);
    }

    /// A small generator with a fixed seed, so that a failure repeats.
    struct Draw(u64);

    impl Draw {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn text(&mut self, max: usize) -> String {
            let pieces = [
                "a",
                "Z",
                "7",
                " ",
                "\"",
                "\\",
                "/",
                "\u{e9}",
                "\u{1F600}",
                "\u{2028}",
                "\u{7f}",
            ];
            let len = self.below(max as u64 + 1) as usize;
            let mut text = String::new();
            while text.len() < len {
                let piece = if self.below(4) == 0 {
                    char::from_u32(self.below(0x20) as u32).unwrap().to_string()
                } else {
                    pieces[self.below(pieces.len() as u64) as usize].to_string()
                };
                if text.len() + piece.len() > len {
                    break;
                }
                text.push_str(&piece);
            }
            text
        }
    }

    #[test]
    fn a_round_trip_of_drawn_records_decodes_to_what_went_in() {
        fn fixed() -> SystemTime {
            UNIX_EPOCH + Duration::new(1_800_000_000, 123_456_789)
        }
        let clock = EventClock::new(fixed);
        let mut draw = Draw(0x5eed_c0de);
        let domains = [
            "example.com",
            "mail.example.co.uk",
            "a-b.example.org",
            "x.y.z.example.net",
        ];
        let mut kinds = [0usize; 6];
        for number in 0..600u64 {
            let app_text = draw.text(TEXT_MAX_BYTES);
            let app = match draw.below(3) {
                0 => AppIdentity::bundle_id(app_text.clone()),
                1 => AppIdentity::executable_path(app_text.clone()),
                _ => AppIdentity::unknown(),
            };
            let expected_app = if app.is_unknown() {
                None
            } else {
                Some(app_text.clone())
            };
            let kind = draw.below(6) as usize;
            kinds[kind] += 1;
            let mut expected_title = None;
            let event = match kind {
                0 => FloorEvent::foreground_app(keep(app, 7), None, &clock),
                1 => {
                    let words = if draw.below(5) == 0 {
                        "\u{1f}".repeat(TEXT_MAX_BYTES)
                    } else {
                        draw.text(TEXT_MAX_BYTES)
                    };
                    let site = if draw.below(2) == 0 {
                        let domain = domains[draw.below(domains.len() as u64) as usize];
                        Site::cut(format!("https://www.{domain}/path?q=1"))
                    } else {
                        None
                    };
                    expected_title = Some((words.clone(), site.is_some()));
                    let title = Title::new(words, TitleWhy::TitleChange, site);
                    FloorEvent::window_title(keep(app, 7), title, None, &clock)
                }
                2 => {
                    let cleared = check(&FixedProbe(SecureInputReading::Inactive), None).unwrap();
                    let class = KeyClass::ALL[draw.below(7) as usize];
                    FloorEvent::keystroke(keep(app, 7), cleared, class, &clock)
                }
                3 => {
                    let button = Button::ALL[draw.below(3) as usize];
                    FloorEvent::click(keep(app, 7), button, None, &clock)
                }
                4 => {
                    let mut count = FoldCount::one();
                    for _ in 0..draw.below(50) {
                        count.add_one();
                    }
                    let class = FoldClass::ALL[draw.below(3) as usize];
                    FoldEvent::new(keep(app, 7), PointerFold::new(class, count), None, &clock)
                        .into_event()
                }
                _ => FloorEvent::screen_frame(keep(app, 7), None, &clock),
            };
            let (id, at) = (event.id().uuid(), event.at());
            let received = FloorReceived::for_test(event, ArrivalNumber::for_test(number));
            let line = encode(&received, &IdentityPoint).unwrap();
            let decoded = decode(line.as_str()).unwrap_or_else(|e| panic!("record {number}: {e}"));
            assert_eq!(
                (decoded.id(), decoded.at(), decoded.seq()),
                (id, at, number)
            );
            assert_eq!(decoded.app(), expected_app.as_deref(), "record {number}");
            if let Some((words, has_site)) = expected_title {
                let DecodedPayload::WindowTitle {
                    text, cut, site, ..
                } = decoded.payload()
                else {
                    panic!("record {number}: not a title");
                };
                assert_eq!(text.as_str(), words, "record {number}");
                assert!(!cut);
                assert_eq!(site.is_some(), has_site);
            }
        }
        assert!(kinds.iter().all(|n| *n > 50), "{kinds:?}");
    }

    #[cfg(not(feature = "r2"))]
    #[test]
    fn the_reserved_run_mark_kind_is_a_typed_error_and_has_no_constructor() {
        assert_eq!(RUN_MARK_KIND, "run_mark");
        assert!(Modality::from_str(RUN_MARK_KIND).is_err());
        let mark = line(RUN_MARK_KIND, "null", r#"{"phase":"start"}"#);
        assert_eq!(decode(&mark), Err(DecodeError::ReservedKind));
        assert_eq!(
            decode(&line(RUN_MARK_KIND, "null", "{}")),
            Err(DecodeError::ReservedKind)
        );
    }

    #[cfg(all(test, feature = "r2"))]
    fn instant() -> SystemTime {
        UNIX_EPOCH + Duration::new(1_800_000_000, 123_000_000)
    }

    #[cfg(all(test, feature = "r2"))]
    #[test]
    fn a_run_mark_record_round_trips_under_the_feature() {
        let mut p = Map::new();
        p.insert("phase".to_string(), Value::from("start"));
        p.insert("step".to_string(), Value::from(3));
        let mark = RunMark::new(instant(), p.clone()).unwrap();
        let (_handle, writer) = crate::sink::bounded(std::num::NonZeroUsize::MIN);
        let number = writer.number_next();
        assert_eq!(writer.number_next().get(), number.get() + 1);
        let record = mark.encode(number);
        let decoded = decode(record.as_str()).unwrap();
        assert_eq!(decoded.kind(), RUN_MARK_KIND);
        assert_eq!(decoded.app(), None);
        assert_eq!(decoded.seq(), number.get());
        assert_eq!(decoded.at(), crate::event::unix_nanos(instant()));
        assert_eq!(decoded.id(), mark.id.uuid());
        assert_eq!(decoded.id().get_version_num(), 7);
        let (seconds, nanos) = decoded.id().get_timestamp().unwrap().to_unix();
        assert!(seconds * 1_000 + u64::from(nanos) / 1_000_000 >= 1_800_000_000_123);
        assert_eq!(decoded.payload(), &DecodedPayload::RunMark(p));
        let with_app = record.as_str().replacen(r#""app":null"#, r#""app":"x""#, 1);
        assert_eq!(decode(&with_app), Err(DecodeError::BadValue("app")));
        let spaced = record.as_str().replacen(r#""step":3"#, r#""step": 3"#, 1);
        assert_eq!(decode(&spaced), Err(DecodeError::NotEncoderForm));
    }

    #[cfg(all(test, feature = "r2"))]
    #[test]
    fn a_run_mark_record_longer_than_the_largest_record_is_a_typed_error() {
        let note = |bytes: usize| {
            let mut p = Map::new();
            p.insert("note".to_string(), Value::from("x".repeat(bytes)));
            p
        };
        let empty = RunMark::new(instant(), note(0)).unwrap();
        let fits = LARGEST_RECORD_BYTES - empty.line(u64::MAX).len();
        let exact =
            RunMark::new(instant(), note(fits)).expect("a mark exactly as long as the constant");
        assert_eq!(exact.line(u64::MAX).len(), LARGEST_RECORD_BYTES);
        let refused = RunMark::new(instant(), note(fits + 1)).err();
        assert_eq!(
            refused,
            Some(RecordTooLong {
                len: LARGEST_RECORD_BYTES + 1,
                max: LARGEST_RECORD_BYTES
            })
        );
    }
}
