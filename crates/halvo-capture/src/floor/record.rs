//! The stored record (contracts.md C13.1): one JSON object on a line of the
//! segment payload, `{"v":1,"id":..,"at":..,"seq":..,"kind":..,"app":..,"p":{..}}`.
//! The encoder takes a received floor value alone and writes every captured
//! string through the text filter point that it is given; the strict decoder
//! returns a separate decoded value, which is data.

use std::fmt::{self, Write as _};
use std::str::FromStr;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::payload::{is_recorded, Button, FoldClass, KeyClass, Observation, TitleWhy};
use super::site::SITE_MAX_BYTES;
use super::text::{write_through, TextField, TextFilterPoint, TEXT_MAX_BYTES};
use crate::event::Modality;
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
    head(
        &mut out,
        event.id().uuid(),
        event.at(),
        received.number().get(),
        event.kind().as_str(),
    );
    match event.app().text() {
        Some((text, field)) => {
            write_through(text, field, point, &mut out);
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"p\":");
    payload(&mut out, event.observation(), point);
    out.push('}');
    if out.len() > LARGEST_RECORD_BYTES {
        return Err(RecordTooLong {
            len: out.len(),
            max: LARGEST_RECORD_BYTES,
        });
    }
    Ok(EncodedRecord(out))
}

fn head(out: &mut String, id: Uuid, at: i64, seq: u64, kind: &str) {
    let _ = write!(
        out,
        "{{\"v\":{SHAPE_VERSION},\"id\":\"{}\",\"at\":{at},\"seq\":{seq},\"kind\":\"{kind}\",\"app\":",
        id.hyphenated()
    );
}

fn payload<P: TextFilterPoint>(out: &mut String, observation: &Observation, point: &P) {
    match observation {
        Observation::ForegroundApp | Observation::ScreenFrame => out.push_str("{}"),
        Observation::WindowTitle(title) => {
            out.push_str("{\"text\":");
            let changed = write_through(&title.text, TextField::Title, point, out);
            let cut = title.text.was_cut() || changed;
            let _ = write!(out, ",\"cut\":{cut},\"why\":\"{}\"", title.why.as_str());
            if let Some(site) = &title.site {
                out.push_str(",\"site\":{\"domain\":");
                write_through(site.domain(), TextField::Site, point, out);
                out.push_str(",\"source\":\"address_field\"}");
            }
            out.push('}');
        }
        Observation::Keystroke(class) => {
            let _ = write!(out, "{{\"class\":\"{}\"}}", class.as_str());
        }
        Observation::Click(button) => {
            let _ = write!(
                out,
                "{{\"class\":\"click\",\"button\":\"{}\"}}",
                button.as_str()
            );
        }
        Observation::Fold(fold) => {
            let _ = write!(
                out,
                "{{\"class\":\"{}\",\"count\":{}}}",
                fold.class().as_str(),
                fold.count().get()
            );
        }
    }
}

/// A record as the strict decoder read it. It is data: no route leads from it
/// into the sink. Its `Debug` prints a placeholder.
#[derive(Clone, PartialEq, Eq)]
pub struct DecodedRecord {
    pub id: Uuid,
    pub at: i64,
    pub seq: u64,
    pub app: Option<String>,
    pub payload: DecodedPayload,
}

/// The payload `p` of a decoded record, by kind.
#[derive(Clone, PartialEq, Eq)]
pub enum DecodedPayload {
    ForegroundApp,
    WindowTitle {
        text: String,
        cut: bool,
        why: TitleWhy,
        site: Option<String>,
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

/// Why the decoder refused a line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("the line is not one JSON object")]
    NotAnObject,
    #[error("a key stands twice in one object of the record")]
    DuplicateField,
    #[error("the record holds a field that is not in its shape: {0}")]
    UnknownField(String),
    #[error("the record lacks the field {0}")]
    MissingField(&'static str),
    #[error("the field {0} has a value of the wrong form")]
    BadValue(&'static str),
    #[error("the record has shape version {0}, not 1")]
    Version(u64),
    #[error("the kind run_mark is reserved")]
    ReservedKind,
    #[error("the kind {0} has no record")]
    UnrecordedKind(&'static str),
    #[error("the kind is not one of the kinds")]
    UnknownKind,
}

const RECORD_FIELDS: [&str; 7] = ["v", "id", "at", "seq", "kind", "app", "p"];

/// The strict decoder: one record of the shape above, with no field more and
/// none less, in `p` as in the record.
pub fn decode(line: &str) -> Result<DecodedRecord, DecodeError> {
    let mut record = match serde_json::from_str::<Unique>(line) {
        Ok(Unique(Value::Object(record))) => record,
        Ok(_) => return Err(DecodeError::NotAnObject),
        Err(error) if error.to_string().starts_with(DUPLICATE) => {
            return Err(DecodeError::DuplicateField)
        }
        Err(_) => return Err(DecodeError::NotAnObject),
    };
    exact_fields(&record, &RECORD_FIELDS, &[])?;
    let version = take_u64(&mut record, "v")?;
    if version != SHAPE_VERSION {
        return Err(DecodeError::Version(version));
    }
    let id = take_string(&mut record, "id")?;
    let id = Uuid::parse_str(&id).map_err(|_| DecodeError::BadValue("id"))?;
    if id.get_version_num() != 7 {
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
        Some(Value::String(app)) => Some(app),
        _ => return Err(DecodeError::BadValue("app")),
    };
    let Some(Value::Object(p)) = record.remove("p") else {
        return Err(DecodeError::BadValue("p"));
    };
    let payload = decoded_payload(&kind, app.is_none(), p)?;
    Ok(DecodedRecord {
        id,
        at,
        seq,
        app,
        payload,
    })
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
            exact_fields(&p, &[], &[])?;
            Ok(if modality == Modality::ForegroundApp {
                DecodedPayload::ForegroundApp
            } else {
                DecodedPayload::ScreenFrame
            })
        }
        Modality::WindowTitle => {
            exact_fields(&p, &["text", "cut", "why"], &["site"])?;
            let text = take_string(&mut p, "text")?;
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
                    exact_fields(&site, &["domain", "source"], &[])?;
                    if take_string(&mut site, "source")? != "address_field" {
                        return Err(DecodeError::BadValue("source"));
                    }
                    Some(take_string(&mut site, "domain")?)
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
            exact_fields(&p, &["class"], &[])?;
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
                exact_fields(&p, &["class", "button"], &[])?;
                let button = take_string(&mut p, "button")?;
                return Button::ALL
                    .into_iter()
                    .find(|known| known.as_str() == button)
                    .map(DecodedPayload::Click)
                    .ok_or(DecodeError::BadValue("button"));
            }
            exact_fields(&p, &["class", "count"], &[])?;
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
    required: &[&'static str],
    optional: &[&'static str],
) -> Result<(), DecodeError> {
    if let Some(extra) = object
        .keys()
        .find(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(DecodeError::UnknownField(extra.clone()));
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
        head(&mut out, self.id.uuid(), self.at, seq, RUN_MARK_KIND);
        out.push_str("null,\"p\":");
        out.push_str(&Value::Object(self.p.clone()).to_string());
        out.push('}');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AppIdentity, EventClock, FloorEvent};
    use crate::floor::cases::{keep, LongestPoint, RecordingPoint};
    use crate::floor::payload::Title;
    use crate::sink::{ArrivalNumber, FloorReceived};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn line(kind: &str, app: &str, p: &str) -> String {
        let id = Uuid::now_v7().hyphenated().to_string();
        format!(r#"{{"v":1,"id":"{id}","at":-5,"seq":3,"kind":"{kind}","app":{app},"p":{p}}}"#)
    }

    #[test]
    fn the_decoder_rejects_an_unknown_field() {
        let good = line("keystroke", "null", r#"{"class":"tab"}"#);
        let decoded = decode(&good).unwrap();
        assert_eq!(decoded.payload, DecodedPayload::Keystroke(KeyClass::Tab));
        assert_eq!(
            (decoded.at, decoded.seq, decoded.app.as_deref()),
            (-5, 3, None)
        );
        let unknown = |field: &str| Err(DecodeError::UnknownField(field.to_string()));
        let extra = good.replacen(r#""v":1,"#, r#""v":1,"len":4,"#, 1);
        assert_eq!(decode(&extra), unknown("len"));
        let in_p = line("keystroke", "null", r#"{"class":"tab","code":48}"#);
        assert_eq!(decode(&in_p), unknown("code"));
        let title = r#"{"text":"t","cut":false,"why":"app_change","site":{"domain":"example.com","source":"address_field","path":"/x"}}"#;
        assert_eq!(
            decode(&line("window_title", r#""a""#, title)),
            unknown("path")
        );
        let title = r#"{"text":"t","cut":false,"why":"app_change","url":"https://example.com/"}"#;
        assert_eq!(
            decode(&line("window_title", r#""a""#, title)),
            unknown("url")
        );
        let click = r#"{"class":"click","button":"primary","x":10}"#;
        assert_eq!(decode(&line("pointer", "null", click)), unknown("x"));
        let fold = r#"{"class":"move","count":3,"dx":4}"#;
        assert_eq!(decode(&line("pointer", "null", fold)), unknown("dx"));
        assert_eq!(
            decode(&line("screen_frame", "null", r#"{"width":10}"#)),
            unknown("width")
        );
        let missing = good.replacen(r#""seq":3,"#, "", 1);
        assert_eq!(decode(&missing), Err(DecodeError::MissingField("seq")));
        let version = good.replacen(r#""v":1"#, r#""v":2"#, 1);
        assert_eq!(decode(&version), Err(DecodeError::Version(2)));
        let v4 = line("keystroke", "null", r#"{"class":"tab"}"#);
        let random = uuid::Builder::from_random_bytes([7; 16]).into_uuid();
        let v4 = v4.replacen(&v4[13..49], &random.hyphenated().to_string(), 1);
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
        let key = r#"{"class":"a"}"#;
        assert_eq!(
            decode(&line("keystroke", "null", key)),
            Err(DecodeError::BadValue("class"))
        );
        let float = good.replacen(r#""at":-5"#, r#""at":-5.5"#, 1);
        assert_eq!(decode(&float), Err(DecodeError::BadValue("at")));
        assert_eq!(decode("[1]"), Err(DecodeError::NotAnObject));
        let twice_v = good.replacen(r#""v":1,"#, r#""v":1,"v":1,"#, 1);
        assert_eq!(decode(&twice_v), Err(DecodeError::DuplicateField));
        let twice_app = good.replacen(r#""app":null"#, r#""app":"a","app":null"#, 1);
        assert_eq!(decode(&twice_app), Err(DecodeError::DuplicateField));
        let title = r#"{"text":"raw","text":"t","cut":false,"why":"app_change"}"#;
        assert_eq!(
            decode(&line("window_title", "null", title)),
            Err(DecodeError::DuplicateField)
        );
        let site = r#"{"text":"t","cut":false,"why":"app_change","site":{"domain":"a.com","domain":"example.com","source":"address_field"}}"#;
        assert_eq!(
            decode(&line("window_title", "null", site)),
            Err(DecodeError::DuplicateField)
        );
        assert_eq!(decode("{\"v\":"), Err(DecodeError::NotAnObject));
        assert_eq!(format!("{decoded:?}"), "DecodedRecord(keystroke)");
    }

    fn smallest() -> SystemTime {
        UNIX_EPOCH - Duration::from_nanos(i64::MIN.unsigned_abs())
    }

    fn longest_title(point_title_bytes: usize) -> (usize, Result<EncodedRecord, RecordTooLong>) {
        let clock = EventClock::new(smallest);
        let control = "\u{1}".repeat(2 * TEXT_MAX_BYTES);
        let app = AppIdentity::executable_path(control.clone());
        let site = crate::floor::site::Site::cut("https://example.com/".to_string());
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
        assert_eq!(decoded.app, None);
        assert_eq!(decoded.seq, number.get());
        assert_eq!(decoded.at, crate::event::unix_nanos(instant()));
        assert_eq!(decoded.id, mark.id.uuid());
        assert_eq!(decoded.id.get_version_num(), 7);
        let (seconds, nanos) = decoded.id.get_timestamp().unwrap().to_unix();
        assert!(seconds * 1_000 + u64::from(nanos) / 1_000_000 >= 1_800_000_000_123);
        assert_eq!(decoded.payload, DecodedPayload::RunMark(p));
        let with_app = record.as_str().replacen(r#""app":null"#, r#""app":"x""#, 1);
        assert_eq!(decode(&with_app), Err(DecodeError::BadValue("app")));
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
