use std::cell::Cell;
use std::fmt;
use std::io::{self, BufRead};

use halvo_schema::{AppId, Domain, Instant, Text};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

/// The longest line of a batch file, its LF included (C4.8).
pub const LINE_MAX: usize = 16_384;

/// The longest `app` string of a record, in bytes (C4.2).
pub const APP_MAX: usize = 1_024;

const NUMBER_MAX: u64 = i64::MAX as u64;

macro_rules! names {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)+
        }

        impl $name {
            /// Every value, in the order of the contract.
            pub const ALL: &'static [$name] = &[$($name::$variant,)+];

            /// The name as a record or a message writes it.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text,)+
                }
            }

            /// The value whose written name is `s`, or none.
            pub fn parse(s: &str) -> Option<$name> {
                match s {
                    $($text => Some($name::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

pub(crate) use names;

names! {
    /// The six kinds that the reader knows (C4.3), `screen_frame` among them, which no server serves.
    Kind {
        ForegroundApp = "foreground_app",
        WindowTitle = "window_title",
        Keystroke = "keystroke",
        Pointer = "pointer",
        RunMark = "run_mark",
        ScreenFrame = "screen_frame",
    }
}

names! {
    /// The five kinds that a server serves, a closed list in the order of C4.4.
    ServedKind {
        ForegroundApp = "foreground_app",
        WindowTitle = "window_title",
        Keystroke = "keystroke",
        Pointer = "pointer",
        RunMark = "run_mark",
    }
}

impl Kind {
    /// The served kind of the same name, or none for `screen_frame`.
    pub fn served(self) -> Option<ServedKind> {
        ServedKind::parse(self.as_str())
    }
}

impl ServedKind {
    /// The kind of the same name.
    pub fn kind(self) -> Kind {
        match self {
            ServedKind::ForegroundApp => Kind::ForegroundApp,
            ServedKind::WindowTitle => Kind::WindowTitle,
            ServedKind::Keystroke => Kind::Keystroke,
            ServedKind::Pointer => Kind::Pointer,
            ServedKind::RunMark => Kind::RunMark,
        }
    }
}

names! {
    /// The `why` of a title record.
    Why {
        AppChange = "app_change",
        TitleChange = "title_change",
    }
}

names! {
    /// The `class` of a keystroke record.
    KeyClass {
        Printable = "printable",
        Editing = "editing",
        Navigation = "navigation",
        Enter = "enter",
        Tab = "tab",
        Shortcut = "shortcut",
        Other = "other",
    }
}

names! {
    /// The `button` of a click record.
    Button {
        Primary = "primary",
        Secondary = "secondary",
        Other = "other",
    }
}

names! {
    /// The pointer classes that carry a `count`.
    Motion {
        Scroll = "scroll",
        Drag = "drag",
        Move = "move",
    }
}

names! {
    /// The `edge` of a run mark: `start` opens an agent run and the next `end` closes it.
    Edge {
        Start = "start",
        End = "end",
    }
}

/// The id of a record: 36 characters, lower-case hex digits in groups of 8, 4, 4, 4 and 12 joined by
/// hyphens, whose third group starts with `7`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RecordId(String);

impl RecordId {
    /// The id when `s` has the form, or none.
    pub fn parse(s: &str) -> Option<RecordId> {
        let b = s.as_bytes();
        if b.len() != 36 || b[14] != b'7' {
            return None;
        }
        let form = b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(c),
        });
        form.then(|| RecordId(s.to_owned()))
    }

    /// The id as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The capture run's count inside one process, from 0 to 2^63 - 1. No reader takes order, identity or a
/// position from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Seq(u64);

impl Seq {
    /// The count when `n` is at most 2^63 - 1, or none.
    pub fn new(n: u64) -> Option<Seq> {
        (n <= NUMBER_MAX).then_some(Seq(n))
    }

    /// The number.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// The `count` of a scroll, drag or move record: the OS events folded into it, from 0 to 2^63 - 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Count(u64);

impl Count {
    /// The count when `n` is at most 2^63 - 1, or none.
    pub fn new(n: u64) -> Option<Count> {
        (n <= NUMBER_MAX).then_some(Count(n))
    }

    /// The number.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// The payload of a title record. `text` is always there; each other key is none where the record does not
/// hold it, which a reduced record (C6.5) may do. `site` is written `{"domain":..,"source":"address_field"}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Title {
    pub text: Text,
    pub cut: Option<bool>,
    pub why: Option<Why>,
    pub site: Option<Domain>,
}

/// The payload of a pointer record. A click holds a button, a scroll, drag or move a count, each unless a
/// reduction took it out; a record whose class was taken out holds at most one of the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Click(Option<Button>),
    Motion(Motion, Option<Count>),
    Unclassed(Option<PointerDetail>),
}

/// The key of a pointer record that stands beside its class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerDetail {
    Button(Button),
    Count(Count),
}

/// The payload `p` of a record, by kind (C4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    ForegroundApp,
    WindowTitle(Title),
    Keystroke(Option<KeyClass>),
    Pointer(Pointer),
    RunMark(Edge),
    ScreenFrame,
}

impl Payload {
    /// The kind that this payload belongs to.
    pub fn kind(&self) -> Kind {
        match self {
            Payload::ForegroundApp => Kind::ForegroundApp,
            Payload::WindowTitle(_) => Kind::WindowTitle,
            Payload::Keystroke(_) => Kind::Keystroke,
            Payload::Pointer(_) => Kind::Pointer,
            Payload::RunMark(_) => Kind::RunMark,
            Payload::ScreenFrame => Kind::ScreenFrame,
        }
    }
}

/// Why the reader refused a record: the key at fault and a rule token, never a value of the record. An
/// unknown key is named by the object that holds it (`record`, `p` or `site`); a defect of a whole line of a
/// batch file has the key `line`; a defect of JSON syntax that no key holds has the key `record` and the rule
/// `json`. `Display` prints `<key>: <rule>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordError {
    key: &'static str,
    rule: &'static str,
}

impl RecordError {
    const fn new(key: &'static str, rule: &'static str) -> RecordError {
        RecordError { key, rule }
    }

    /// The key at fault.
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// The rule: `missing`, `twice`, `unknown_key`, `form`, `kind_key`, `pairing`, `run_mark`, `json`, or for
    /// the key `line` one of `empty`, `no_lf`, `lf`, `cr`, `bom` and `length`.
    pub fn rule(&self) -> &'static str {
        self.rule
    }
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.rule)
    }
}

impl std::error::Error for RecordError {}

/// A capture record as a reader of the seam sees it (C4.1): the envelope and the payload. The type holds the
/// app id that the reader cut from the stored string, and a record written from it holds no directory. Its
/// serde form answers every input that is no record, its object or any other value, with the fixed error
/// `malformed record`, which holds no value of the input (C4.6 item 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureRecord {
    id: RecordId,
    at: Instant,
    seq: Seq,
    app: Option<AppId>,
    payload: Payload,
}

impl CaptureRecord {
    /// A record from its parts; refused with the key `app` when a run mark has an app.
    pub fn new(
        id: RecordId,
        at: Instant,
        seq: Seq,
        app: Option<AppId>,
        payload: Payload,
    ) -> Result<CaptureRecord, RecordError> {
        if app.is_some() && payload.kind() == Kind::RunMark {
            return Err(RecordError::new("app", "run_mark"));
        }
        Ok(CaptureRecord {
            id,
            at,
            seq,
            app,
            payload,
        })
    }

    /// The key `id`.
    pub fn id(&self) -> &RecordId {
        &self.id
    }

    /// The key `at`.
    pub fn at(&self) -> Instant {
        self.at
    }

    /// The key `seq`.
    pub fn seq(&self) -> Seq {
        self.seq
    }

    /// The app id, or none where the record holds null or a string whose cut name `AppId::from_capture`
    /// refuses.
    pub fn app(&self) -> Option<&AppId> {
        self.app.as_ref()
    }

    /// The payload.
    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    /// The key `kind`.
    pub fn kind(&self) -> Kind {
        self.payload.kind()
    }

    pub(crate) fn with_payload(&self, payload: Payload) -> CaptureRecord {
        CaptureRecord {
            id: self.id.clone(),
            at: self.at,
            seq: self.seq,
            app: self.app.clone(),
            payload,
        }
    }

    /// Reads one JSON object by the rules of C4.6: keys in any order, JSON white space between tokens, no key
    /// twice in the record, in `p` or in `site`, no unknown key, integers in decimal with no fraction and no
    /// exponent, and an array or any other value in place of an object refused.
    pub fn read(json: &[u8]) -> Result<CaptureRecord, RecordError> {
        let slot = Cell::new(None);
        let mut de = serde_json::Deserializer::from_slice(json);
        let result = RecordSeed(&slot)
            .deserialize(&mut de)
            .and_then(|record| de.end().map(|()| record));
        result.map_err(|_| slot.get().unwrap_or(RecordError::new("record", "json")))
    }

    /// Reads one line of a batch file, its LF included, by the rules of C4.8: at most `LINE_MAX` bytes, an LF
    /// at the end and nowhere else, no CR byte, no byte-order mark, and spaces and tabs only beside the object.
    pub fn read_line(line: &[u8]) -> Result<CaptureRecord, RecordError> {
        if line.len() > LINE_MAX {
            return Err(RecordError::new("line", "length"));
        }
        let Some(body) = line.strip_suffix(b"\n") else {
            return Err(RecordError::new("line", "no_lf"));
        };
        if body.is_empty() {
            return Err(RecordError::new("line", "empty"));
        }
        if body.contains(&b'\n') {
            return Err(RecordError::new("line", "lf"));
        }
        if body.contains(&b'\r') {
            return Err(RecordError::new("line", "cr"));
        }
        if body.starts_with(&[0xEF, 0xBB, 0xBF]) {
            return Err(RecordError::new("line", "bom"));
        }
        CaptureRecord::read(body)
    }

    /// The record as one compact JSON object, with the keys in the order of C4.1 and C4.3 and the escapes of
    /// C4.8 item 3.
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// The record as one line of a batch file: `to_json` and an LF.
    pub fn to_line(&self) -> Vec<u8> {
        let mut out = self.to_json();
        out.push(b'\n');
        out
    }
}

impl Serialize for CaptureRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(7))?;
        map.serialize_entry("v", &1u32)?;
        map.serialize_entry("id", self.id.as_str())?;
        map.serialize_entry("at", &self.at.0)?;
        map.serialize_entry("seq", &self.seq.0)?;
        map.serialize_entry("kind", self.kind().as_str())?;
        map.serialize_entry("app", &self.app.as_ref().map(AppId::as_str))?;
        map.serialize_entry("p", &PayloadOut(&self.payload))?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for CaptureRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let slot = Cell::new(None);
        RecordSeed(&slot)
            .deserialize(deserializer)
            .map_err(|_| de::Error::custom("malformed record"))
    }
}

struct PayloadOut<'a>(&'a Payload);

impl Serialize for PayloadOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self.0 {
            Payload::ForegroundApp | Payload::ScreenFrame => {}
            Payload::WindowTitle(t) => {
                map.serialize_entry("text", t.text.as_str())?;
                if let Some(cut) = t.cut {
                    map.serialize_entry("cut", &cut)?;
                }
                if let Some(why) = t.why {
                    map.serialize_entry("why", why.as_str())?;
                }
                if let Some(domain) = &t.site {
                    map.serialize_entry("site", &SiteOut(domain))?;
                }
            }
            Payload::Keystroke(class) => {
                if let Some(class) = class {
                    map.serialize_entry("class", class.as_str())?;
                }
            }
            Payload::Pointer(pointer) => match pointer {
                Pointer::Click(button) => {
                    map.serialize_entry("class", "click")?;
                    if let Some(button) = button {
                        map.serialize_entry("button", button.as_str())?;
                    }
                }
                Pointer::Motion(motion, count) => {
                    map.serialize_entry("class", motion.as_str())?;
                    if let Some(count) = count {
                        map.serialize_entry("count", &count.0)?;
                    }
                }
                Pointer::Unclassed(detail) => match detail {
                    Some(PointerDetail::Button(button)) => {
                        map.serialize_entry("button", button.as_str())?;
                    }
                    Some(PointerDetail::Count(count)) => {
                        map.serialize_entry("count", &count.0)?;
                    }
                    None => {}
                },
            },
            Payload::RunMark(edge) => {
                map.serialize_entry("edge", edge.as_str())?;
            }
        }
        map.end()
    }
}

struct SiteOut<'a>(&'a Domain);

impl Serialize for SiteOut<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("domain", self.0.as_str())?;
        map.serialize_entry("source", "address_field")?;
        map.end()
    }
}

type Slot<'a> = &'a Cell<Option<RecordError>>;

fn fail<E: de::Error>(slot: Slot<'_>, key: &'static str, rule: &'static str) -> E {
    if slot.get().is_none() {
        slot.set(Some(RecordError::new(key, rule)));
    }
    E::custom("malformed record")
}

struct KeyName(&'static [&'static str]);

impl<'de> DeserializeSeed<'de> for KeyName {
    type Value = Option<usize>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Option<usize>, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for KeyName {
    type Value = Option<usize>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a key")
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<Option<usize>, E> {
        Ok(self.0.iter().position(|k| *k == s))
    }
}

fn once<T, E: de::Error>(slot: Slot<'_>, seen: &Option<T>, key: &'static str) -> Result<(), E> {
    if seen.is_some() {
        return Err(fail(slot, key, "twice"));
    }
    Ok(())
}

const ENVELOPE: [&str; 7] = ["v", "id", "at", "seq", "kind", "app", "p"];

struct RecordSeed<'a>(Slot<'a>);

impl<'de> DeserializeSeed<'de> for RecordSeed<'_> {
    type Value = CaptureRecord;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<CaptureRecord, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for RecordSeed<'_> {
    type Value = CaptureRecord;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a capture record object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<CaptureRecord, A::Error> {
        let slot = self.0;
        let mut v: Option<u64> = None;
        let mut id: Option<String> = None;
        let mut at: Option<i64> = None;
        let mut seq: Option<u64> = None;
        let mut kind: Option<String> = None;
        let mut app: Option<Option<String>> = None;
        let mut p: Option<Fields> = None;
        while let Some(key) = map.next_key_seed(KeyName(&ENVELOPE))? {
            match key {
                Some(0) => {
                    once(slot, &v, "v")?;
                    v = Some(
                        map.next_value()
                            .map_err(|_: A::Error| fail(slot, "v", "form"))?,
                    );
                }
                Some(1) => {
                    once(slot, &id, "id")?;
                    id = Some(
                        map.next_value()
                            .map_err(|_: A::Error| fail(slot, "id", "form"))?,
                    );
                }
                Some(2) => {
                    once(slot, &at, "at")?;
                    at = Some(
                        map.next_value()
                            .map_err(|_: A::Error| fail(slot, "at", "form"))?,
                    );
                }
                Some(3) => {
                    once(slot, &seq, "seq")?;
                    seq = Some(
                        map.next_value()
                            .map_err(|_: A::Error| fail(slot, "seq", "form"))?,
                    );
                }
                Some(4) => {
                    once(slot, &kind, "kind")?;
                    kind = Some(
                        map.next_value()
                            .map_err(|_: A::Error| fail(slot, "kind", "form"))?,
                    );
                }
                Some(5) => {
                    once(slot, &app, "app")?;
                    app = Some(
                        map.next_value()
                            .map_err(|_: A::Error| fail(slot, "app", "form"))?,
                    );
                }
                Some(6) => {
                    once(slot, &p, "p")?;
                    p = Some(
                        map.next_value_seed(FieldsSeed(slot))
                            .map_err(|_: A::Error| fail(slot, "p", "form"))?,
                    );
                }
                _ => return Err(fail(slot, "record", "unknown_key")),
            }
        }
        let missing = |key| fail::<A::Error>(slot, key, "missing");
        let v = v.ok_or_else(|| missing("v"))?;
        let id = id.ok_or_else(|| missing("id"))?;
        let at = at.ok_or_else(|| missing("at"))?;
        let seq = seq.ok_or_else(|| missing("seq"))?;
        let kind = kind.ok_or_else(|| missing("kind"))?;
        let app = app.ok_or_else(|| missing("app"))?;
        let p = p.ok_or_else(|| missing("p"))?;
        envelope(v, &id, at, seq, &kind, app, p).map_err(|e| fail(slot, e.key, e.rule))
    }
}

fn envelope(
    v: u64,
    id: &str,
    at: i64,
    seq: u64,
    kind: &str,
    app: Option<String>,
    p: Fields,
) -> Result<CaptureRecord, RecordError> {
    if v != 1 {
        return Err(RecordError::new("v", "form"));
    }
    let id = RecordId::parse(id).ok_or(RecordError::new("id", "form"))?;
    let seq = Seq::new(seq).ok_or(RecordError::new("seq", "form"))?;
    let kind = Kind::parse(kind).ok_or(RecordError::new("kind", "form"))?;
    let app = match app {
        None => None,
        Some(s) => {
            if s.is_empty() || s.len() > APP_MAX {
                return Err(RecordError::new("app", "form"));
            }
            if kind == Kind::RunMark {
                return Err(RecordError::new("app", "run_mark"));
            }
            AppId::from_capture(&s).ok()
        }
    };
    let payload = payload(kind, p)?;
    CaptureRecord::new(id, Instant(at), seq, app, payload)
}

#[derive(Default)]
struct Fields {
    text: Option<String>,
    cut: Option<bool>,
    why: Option<String>,
    site: Option<Domain>,
    class: Option<String>,
    button: Option<String>,
    count: Option<u64>,
    edge: Option<String>,
}

impl Fields {
    fn present(&self) -> [(&'static str, bool); 8] {
        [
            ("text", self.text.is_some()),
            ("cut", self.cut.is_some()),
            ("why", self.why.is_some()),
            ("site", self.site.is_some()),
            ("class", self.class.is_some()),
            ("button", self.button.is_some()),
            ("count", self.count.is_some()),
            ("edge", self.edge.is_some()),
        ]
    }
}

const PAYLOAD_KEYS: [&str; 8] = [
    "text", "cut", "why", "site", "class", "button", "count", "edge",
];

struct FieldsSeed<'a>(Slot<'a>);

impl<'de> DeserializeSeed<'de> for FieldsSeed<'_> {
    type Value = Fields;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Fields, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for FieldsSeed<'_> {
    type Value = Fields;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a payload object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Fields, A::Error> {
        let slot = self.0;
        let mut f = Fields::default();
        while let Some(key) = map.next_key_seed(KeyName(&PAYLOAD_KEYS))? {
            let Some(index) = key else {
                return Err(fail(slot, "p", "unknown_key"));
            };
            let name = PAYLOAD_KEYS[index];
            if f.present()[index].1 {
                return Err(fail(slot, name, "twice"));
            }
            let form = |_: A::Error| fail::<A::Error>(slot, name, "form");
            match index {
                0 => f.text = Some(map.next_value().map_err(form)?),
                1 => f.cut = Some(map.next_value().map_err(form)?),
                2 => f.why = Some(map.next_value().map_err(form)?),
                3 => f.site = Some(map.next_value_seed(SiteSeed(slot)).map_err(form)?),
                4 => f.class = Some(map.next_value().map_err(form)?),
                5 => f.button = Some(map.next_value().map_err(form)?),
                6 => f.count = Some(map.next_value().map_err(form)?),
                _ => f.edge = Some(map.next_value().map_err(form)?),
            }
        }
        Ok(f)
    }
}

struct SiteSeed<'a>(Slot<'a>);

impl<'de> DeserializeSeed<'de> for SiteSeed<'_> {
    type Value = Domain;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Domain, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for SiteSeed<'_> {
    type Value = Domain;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a site object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Domain, A::Error> {
        let slot = self.0;
        let mut domain: Option<String> = None;
        let mut source: Option<String> = None;
        while let Some(key) = map.next_key_seed(KeyName(&["domain", "source"]))? {
            match key {
                Some(0) => {
                    once(slot, &domain, "domain")?;
                    domain = Some(
                        map.next_value()
                            .map_err(|_: A::Error| fail(slot, "domain", "form"))?,
                    );
                }
                Some(1) => {
                    once(slot, &source, "source")?;
                    source = Some(
                        map.next_value()
                            .map_err(|_: A::Error| fail(slot, "source", "form"))?,
                    );
                }
                _ => return Err(fail(slot, "site", "unknown_key")),
            }
        }
        let domain = domain.ok_or_else(|| fail::<A::Error>(slot, "domain", "missing"))?;
        let source = source.ok_or_else(|| fail::<A::Error>(slot, "source", "missing"))?;
        if source != "address_field" {
            return Err(fail(slot, "source", "form"));
        }
        Domain::parse(&domain).map_err(|_| fail(slot, "domain", "form"))
    }
}

fn allowed(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::ForegroundApp | Kind::ScreenFrame => &[],
        Kind::WindowTitle => &["text", "cut", "why", "site"],
        Kind::Keystroke => &["class"],
        Kind::Pointer => &["class", "button", "count"],
        Kind::RunMark => &["edge"],
    }
}

fn parsed<T>(
    value: Option<String>,
    parse: fn(&str) -> Option<T>,
    key: &'static str,
) -> Result<Option<T>, RecordError> {
    value
        .map(|s| parse(&s).ok_or(RecordError::new(key, "form")))
        .transpose()
}

fn payload(kind: Kind, f: Fields) -> Result<Payload, RecordError> {
    let keys = allowed(kind);
    if let Some((name, _)) = f
        .present()
        .into_iter()
        .find(|(name, there)| *there && !keys.contains(name))
    {
        return Err(RecordError::new(name, "kind_key"));
    }
    Ok(match kind {
        Kind::ForegroundApp => Payload::ForegroundApp,
        Kind::ScreenFrame => Payload::ScreenFrame,
        Kind::WindowTitle => {
            let text = f.text.ok_or(RecordError::new("text", "missing"))?;
            let text = Text::parse(&text).map_err(|_| RecordError::new("text", "form"))?;
            Payload::WindowTitle(Title {
                text,
                cut: f.cut,
                why: parsed(f.why, Why::parse, "why")?,
                site: f.site,
            })
        }
        Kind::Keystroke => Payload::Keystroke(parsed(f.class, KeyClass::parse, "class")?),
        Kind::Pointer => {
            let button = parsed(f.button, Button::parse, "button")?;
            let count = f
                .count
                .map(|n| Count::new(n).ok_or(RecordError::new("count", "form")))
                .transpose()?;
            let pointer = match f.class.as_deref() {
                None => match (button, count) {
                    (Some(_), Some(_)) => return Err(RecordError::new("count", "pairing")),
                    (Some(b), None) => Pointer::Unclassed(Some(PointerDetail::Button(b))),
                    (None, Some(n)) => Pointer::Unclassed(Some(PointerDetail::Count(n))),
                    (None, None) => Pointer::Unclassed(None),
                },
                Some("click") => {
                    if count.is_some() {
                        return Err(RecordError::new("count", "pairing"));
                    }
                    Pointer::Click(button)
                }
                Some(class) => {
                    let motion = Motion::parse(class).ok_or(RecordError::new("class", "form"))?;
                    if button.is_some() {
                        return Err(RecordError::new("button", "pairing"));
                    }
                    Pointer::Motion(motion, count)
                }
            };
            Payload::Pointer(pointer)
        }
        Kind::RunMark => {
            let edge = f.edge.ok_or(RecordError::new("edge", "missing"))?;
            Payload::RunMark(Edge::parse(&edge).ok_or(RecordError::new("edge", "form"))?)
        }
    })
}

/// The length of one line that `Lines::next_line` read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// The line is in the caller's buffer: its bytes up to and including the LF, or the last bytes of the
    /// input that no LF ends.
    Bytes,
    /// The line has more than `LINE_MAX` bytes, its LF included; it was passed over and not kept, and the
    /// number is its length.
    Long(usize),
}

/// Reads a batch file one line at a time and never holds more than one line of at most `LINE_MAX` bytes.
pub struct Lines<R> {
    inner: R,
}

impl<R: BufRead> Lines<R> {
    /// A reader of the lines of `inner`.
    pub fn new(inner: R) -> Lines<R> {
        Lines { inner }
    }

    /// Reads the next line into `out`, which it clears first; none at the end of the input.
    pub fn next_line(&mut self, out: &mut Vec<u8>) -> io::Result<Option<Line>> {
        out.clear();
        let mut long = 0usize;
        loop {
            let avail = match self.inner.fill_buf() {
                Ok(avail) => avail,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if avail.is_empty() {
                return Ok(if long > 0 {
                    Some(Line::Long(long))
                } else if out.is_empty() {
                    None
                } else {
                    Some(Line::Bytes)
                });
            }
            let (take, ends) = match avail.iter().position(|b| *b == b'\n') {
                Some(i) => (i + 1, true),
                None => (avail.len(), false),
            };
            if long > 0 {
                long += take;
            } else if out.len() + take > LINE_MAX {
                long = out.len() + take;
                out.clear();
            } else {
                out.extend_from_slice(&avail[..take]);
            }
            self.inner.consume(take);
            if ends {
                return Ok(Some(if long > 0 {
                    Line::Long(long)
                } else {
                    Line::Bytes
                }));
            }
        }
    }

    /// Passes over the next line without keeping it; false at the end of the input.
    pub fn skip_line(&mut self) -> io::Result<bool> {
        let mut any = false;
        loop {
            let avail = match self.inner.fill_buf() {
                Ok(avail) => avail,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if avail.is_empty() {
                return Ok(any);
            }
            any = true;
            match avail.iter().position(|b| *b == b'\n') {
                Some(i) => {
                    self.inner.consume(i + 1);
                    return Ok(true);
                }
                None => {
                    let n = avail.len();
                    self.inner.consume(n);
                }
            }
        }
    }

    /// True when no byte is left.
    pub fn at_end(&mut self) -> io::Result<bool> {
        loop {
            match self.inner.fill_buf() {
                Ok(avail) => return Ok(avail.is_empty()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}
