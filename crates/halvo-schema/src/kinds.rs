use serde_json::value::RawValue;

use crate::ids::{hex32, hex64, AgentId, RequestId, SummaryId, WorkflowId, MAX_ORDINAL};
use crate::record::{Confidence, KeyCounts, PointerCounts, Recurrence};
use crate::request::Fields;
use crate::site::{AppId, SiteDomain, SiteHash, SiteToken, SiteValue};

/// The names of the kind registry of this run: the four kind names, the family `workflow` and the three
/// retention-class names. The names of `FieldType` and of `Origin` and the key `days` are none.
pub const NAMES: &[&str] = &[
    "workflow", "summary", "support", "evidence", "window", "bounded", "kept",
];

/// The type of a field of a record, one variant per row of the closed set of field types: the integer 1, a random
/// id, a name of a closed list, an app id, a stored domain (`SiteDomain`), a site hash, a site value, a count, a
/// day, an instant, a segment ordinal, a site token, a text after masking, and a text that capture did not supply.
/// It has no serde form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Version,
    Id,
    Name,
    App,
    Domain,
    Hash,
    Site,
    Count,
    Day,
    Instant,
    Ordinal,
    Token,
    Text,
    OwnText,
}

/// The retention class of a kind: window, bounded with its bound in days, or kept. It has no serde form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionClass {
    Window,
    Bounded { days: u16 },
    Kept,
}

impl RetentionClass {
    /// `window`, `bounded` or `kept`, the retention-class names of the names file.
    pub fn name(self) -> &'static str {
        match self {
            RetentionClass::Window => "window",
            RetentionClass::Bounded { .. } => "bounded",
            RetentionClass::Kept => "kept",
        }
    }
}

/// The origin of a kind, fixed when its entry is added and never changed: capture, or own, a bounded or kept kind
/// that may hold an instant and a text that capture did not supply, names a workflow and is written only for a
/// `runner` or a `coordinator_write` request. It has no serde form and no name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Capture,
    Own,
}

/// The entry of one record kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindEntry {
    /// The kind name: 1 to 32 bytes of `a` to `z`, `0` to `9` and `_`, new in the chain of the registry.
    pub name: &'static str,
    /// The retention class; a bounded class carries its bound in days.
    pub class: RetentionClass,
    /// Capture or own.
    pub origin: Origin,
    /// The record family, the unit of the derived revision: a token of the form of a kind name.
    pub family: &'static str,
    /// The top-level key that holds a workflow id, or none.
    pub workflow_key: Option<&'static str>,
    /// The top-level keys of the fullest shape of the record in their order, each with the type of its innermost
    /// values; a key whose innermost values have more than one type stands once for each, its pairs together.
    pub fields: &'static [(&'static str, FieldType)],
    /// One serialized record of the kind.
    pub example: &'static str,
}

/// The four kinds of this run, each of origin capture and of the family `workflow`.
pub const KINDS: &[KindEntry] = &[
    KindEntry {
        name: "workflow",
        class: RetentionClass::Kept,
        origin: Origin::Capture,
        family: "workflow",
        workflow_key: Some("id"),
        fields: &[
            ("v", FieldType::Version),
            ("id", FieldType::Id),
            ("state", FieldType::Name),
            ("category", FieldType::Name),
            ("site", FieldType::Site),
            ("app", FieldType::App),
            ("trigger", FieldType::Name),
            ("recurrence", FieldType::Count),
            ("recurrence", FieldType::Day),
            ("confidence", FieldType::Count),
            ("agent", FieldType::Id),
        ],
        example: r#"{"v":1,"id":"10000000000000000000000000000001","state":"proposed","category":"utility_bill_payment","site":{"key":"abababababababababababababababababababababababababababababababab","source":"title"},"app":"com.example.browser","trigger":"schedule","recurrence":{"count":3,"period":31,"first":20705,"last":20767},"confidence":87,"agent":null}"#,
    },
    KindEntry {
        name: "summary",
        class: RetentionClass::Bounded { days: 93 },
        origin: Origin::Capture,
        family: "workflow",
        workflow_key: None,
        fields: &[
            ("v", FieldType::Version),
            ("id", FieldType::Id),
            ("category", FieldType::Name),
            ("site", FieldType::Site),
            ("app", FieldType::App),
            ("day", FieldType::Day),
            ("part", FieldType::Name),
            ("minutes", FieldType::Count),
            ("keys", FieldType::Count),
            ("pointer", FieldType::Count),
        ],
        example: r#"{"v":1,"id":"20000000000000000000000000000001","category":"utility_bill_payment","site":{"key":"abababababababababababababababababababababababababababababababab","source":"title"},"app":"com.example.browser","day":20767,"part":"evening","minutes":5,"keys":{"printable":42,"editing":3,"navigation":0,"enter":2,"tab":4,"shortcut":0,"other":0},"pointer":{"click":7,"scroll":12,"drag":0,"move":30}}"#,
    },
    KindEntry {
        name: "support",
        class: RetentionClass::Bounded { days: 93 },
        origin: Origin::Capture,
        family: "workflow",
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("summary", FieldType::Id),
            ("day", FieldType::Day),
        ],
        example: r#"{"v":1,"workflow":"10000000000000000000000000000001","summary":"20000000000000000000000000000001","day":20767}"#,
    },
    KindEntry {
        name: "evidence",
        class: RetentionClass::Window,
        origin: Origin::Capture,
        family: "workflow",
        workflow_key: None,
        fields: &[
            ("v", FieldType::Version),
            ("summary", FieldType::Id),
            ("segments", FieldType::Ordinal),
            ("expires", FieldType::Instant),
            ("start", FieldType::Instant),
            ("end", FieldType::Instant),
            ("hint", FieldType::Token),
            ("steps", FieldType::App),
            ("steps", FieldType::Name),
            ("steps", FieldType::Text),
            ("steps", FieldType::Domain),
        ],
        example: r#"{"v":1,"summary":"20000000000000000000000000000001","segments":[4410,4411],"expires":1795478400000000000,"start":1794339000000000000,"end":1794339135000000000,"hint":"shop","steps":[{"app":{"value":"com.example.browser","layer":"floor"},"title":{"value":"Pay bill - Example Power","layer":"floor"},"input":{"value":"click","layer":"floor"},"site":{"value":"shop.test","layer":"address_field"},"text":{"value":"Amount due","layer":"accessibility_text"}}]}"#,
    },
];

/// Every registered kind: the kinds of this run, then the kind lists of the files of the agent-setup, interface,
/// learning and routing runs, in that order.
pub fn registry() -> impl Iterator<Item = &'static KindEntry> {
    KINDS
        .iter()
        .chain(crate::executable::KINDS)
        .chain(crate::coordinator::KINDS)
        .chain(crate::learning::KINDS)
        .chain(crate::routing::KINDS)
}

fn name_form(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn has(entry: &KindEntry, key: &str, field_type: FieldType) -> bool {
    entry.fields.contains(&(key, field_type))
}

fn keys_in_order(entry: &KindEntry) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = Vec::new();
    for (key, _) in entry.fields {
        if !keys.contains(key) {
            keys.push(key);
        }
    }
    keys
}

/// The checks of the rules for kind entries over any list, one line per breach; empty when the list is sound.
/// It reports a name that stands twice; a name or a family outside the form of a kind name; the family `workflow`
/// in an entry that is none of the four kinds of this run; a bounded entry with 0 days or without the key `day`
/// of the type `Day`; a window entry without the key `expires` of the type `Instant` or the key `segments` of the
/// type `Ordinal`; a `workflow_key` that is no key of the fields with the type `Id`; an entry of origin own of the
/// class window or without a `workflow_key`; a bounded or kept entry of origin capture with a field of the type
/// `Instant`, `Ordinal`, `Token`, `Text` or `OwnText`; an entry of origin own with a field of the type `Ordinal`,
/// `Token` or `Text`; a window entry with a field of the type `OwnText`; an example that `check_record` refuses;
/// and an example whose top-level keys are not the keys of the fields in their order.
pub fn check_entries(entries: &[&KindEntry]) -> Vec<String> {
    let mut reports = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for entry in entries {
        let count = entries.iter().filter(|e| e.name == entry.name).count();
        if count > 1 && !seen.contains(&entry.name) {
            seen.push(entry.name);
            reports.push(format!(
                "kind {:?}: the name stands {count} times in the list",
                entry.name
            ));
        }
    }
    for entry in entries {
        let kind = format!("kind {:?}", entry.name);
        if !name_form(entry.name) {
            reports.push(format!(
                "{kind}: the name is not 1 to 32 bytes of a-z, 0-9 and _"
            ));
        }
        if !name_form(entry.family) {
            reports.push(format!(
                "{kind}: the family is not 1 to 32 bytes of a-z, 0-9 and _"
            ));
        }
        if entry.family == "workflow" && !KINDS.contains(entry) {
            reports.push(format!(
                "{kind}: the family workflow belongs to the four kinds of this run"
            ));
        }
        match entry.class {
            RetentionClass::Bounded { days } => {
                if days == 0 {
                    reports.push(format!("{kind}: a bounded kind with 0 days"));
                }
                if !has(entry, "day", FieldType::Day) {
                    reports.push(format!(
                        "{kind}: a bounded kind without the key day of the type Day"
                    ));
                }
            }
            RetentionClass::Window => {
                if !has(entry, "expires", FieldType::Instant) {
                    reports.push(format!(
                        "{kind}: a window kind without the key expires of the type Instant"
                    ));
                }
                if !has(entry, "segments", FieldType::Ordinal) {
                    reports.push(format!(
                        "{kind}: a window kind without the key segments of the type Ordinal"
                    ));
                }
            }
            RetentionClass::Kept => {}
        }
        if let Some(key) = entry.workflow_key {
            if !has(entry, key, FieldType::Id) {
                reports.push(format!(
                    "{kind}: the workflow_key {key} is no key of the fields with the type Id"
                ));
            }
        }
        if entry.origin == Origin::Own {
            if entry.class == RetentionClass::Window {
                reports.push(format!(
                    "{kind}: a kind of origin own with the class window"
                ));
            }
            if entry.workflow_key.is_none() {
                reports.push(format!(
                    "{kind}: a kind of origin own without a workflow_key"
                ));
            }
        }
        let capture_stored =
            entry.origin == Origin::Capture && entry.class != RetentionClass::Window;
        for (key, field_type) in entry.fields {
            let rules = [
                (
                    capture_stored,
                    "a bounded or kept kind of origin capture",
                    &[
                        FieldType::Instant,
                        FieldType::Ordinal,
                        FieldType::Token,
                        FieldType::Text,
                        FieldType::OwnText,
                    ][..],
                ),
                (
                    entry.origin == Origin::Own,
                    "a kind of origin own",
                    &[FieldType::Ordinal, FieldType::Token, FieldType::Text][..],
                ),
                (
                    entry.class == RetentionClass::Window,
                    "a window kind",
                    &[FieldType::OwnText][..],
                ),
            ];
            for (applies, what, barred) in rules {
                if applies && barred.contains(field_type) {
                    reports.push(format!(
                        "{kind}: {what} with the key {key} of the type {field_type:?}"
                    ));
                }
            }
        }
        match check_record(entry, entry.example) {
            Err(error) => reports.push(format!("{kind}: the example is refused: {error}")),
            Ok(()) => {
                let example: Vec<String> = Fields::read(entry.example.as_bytes())
                    .map(|fields| fields.0.into_iter().map(|(key, _)| key).collect())
                    .unwrap_or_default();
                if example != keys_in_order(entry) {
                    reports.push(format!(
                        "{kind}: the top-level keys of the example are not the keys of the fields in their order"
                    ));
                }
            }
        }
    }
    reports
}

/// `check_entries` over `registry()`.
pub fn check_registry() -> Vec<String> {
    let entries: Vec<&KindEntry> = registry().collect();
    check_entries(&entries)
}

fn string_form(field_type: FieldType, s: &str) -> bool {
    match field_type {
        FieldType::Id => hex32(s).is_ok(),
        FieldType::Name => name_form(s),
        FieldType::App => AppId::parse(s).is_ok(),
        FieldType::Domain => SiteDomain::parse(s).is_ok(),
        FieldType::Hash => hex64(s).is_ok(),
        FieldType::Token => SiteToken::parse(s).is_ok(),
        FieldType::Text | FieldType::OwnText => s.len() <= 1024,
        FieldType::Version
        | FieldType::Site
        | FieldType::Count
        | FieldType::Day
        | FieldType::Instant
        | FieldType::Ordinal => false,
    }
}

fn integer_form(field_type: FieldType, n: i128) -> bool {
    match field_type {
        FieldType::Version => n == 1,
        FieldType::Count | FieldType::Day => (0..=i128::from(u32::MAX)).contains(&n),
        FieldType::Instant => (i128::from(i64::MIN)..=i128::from(i64::MAX)).contains(&n),
        FieldType::Ordinal => (0..=i128::from(MAX_ORDINAL)).contains(&n),
        FieldType::Id
        | FieldType::Name
        | FieldType::App
        | FieldType::Domain
        | FieldType::Hash
        | FieldType::Site
        | FieldType::Token
        | FieldType::Text
        | FieldType::OwnText => false,
    }
}

fn walk(value: &RawValue, types: &[FieldType]) -> Result<(), &'static str> {
    let text = value.get();
    match text.as_bytes().first() {
        Some(b'n') => Ok(()),
        Some(b't' | b'f') => Err("boolean"),
        Some(b'{') => {
            if types.contains(&FieldType::Site) && serde_json::from_str::<SiteValue>(text).is_ok() {
                return Ok(());
            }
            let fields = Fields::read(text.as_bytes()).ok_or("not_an_object")?;
            fields.0.iter().try_for_each(|(_, v)| walk(v, types))
        }
        Some(b'[') => {
            let items: Vec<&RawValue> = serde_json::from_str(text).map_err(|_| "not_a_list")?;
            items.iter().try_for_each(|v| walk(v, types))
        }
        Some(b'"') => {
            let s: String = serde_json::from_str(text).map_err(|_| "string_form")?;
            if types.iter().any(|t| string_form(*t, &s)) {
                Ok(())
            } else {
                Err("string_form")
            }
        }
        _ => {
            if text.contains(['.', 'e', 'E']) || text == "-0" {
                return Err("not_an_integer");
            }
            let n: i128 = text.parse().map_err(|_| "integer_form")?;
            if types.iter().any(|t| integer_form(*t, n)) {
                Ok(())
            } else {
                Err("integer_form")
            }
        }
    }
}

/// The one parser of a record against an entry: `Ok` when the text is one JSON object whose top-level keys are
/// the keys of the entry's fields, each once, in any order, and no other, and whose value under each key passes
/// the walk by the types of that key: `null` passes; an object passes whole when `Site` is among the types and it
/// reads as a `SiteValue`, else when each of its values passes; a list passes when each of its items passes; a
/// string or an integer passes when it has the JSON form of one of the types (`App`, `Domain` and `Token` through
/// their parsers, `Text` and `OwnText` by their length alone, `Name` as 1 to 32 bytes of `a` to `z`, `0` to `9`
/// and `_`); a boolean, a number with a fraction or an exponent, and `-0`, which serde reads as a float, never
/// pass. The fullest shape alone passes: a dismissed workflow does not. The error is
/// `<kind>: <key>: <rule>` and names a key only where the entry lists it, so it holds no value of the record.
pub fn check_record(entry: &KindEntry, json: &str) -> Result<(), String> {
    let fail = |key: &str, rule: &str| Err(format!("{}: {key}: {rule}", entry.name));
    let Some(fields) = Fields::read(json.as_bytes()) else {
        return fail("the record", "not_an_object");
    };
    let keys = keys_in_order(entry);
    for (key, _) in &fields.0 {
        if !keys.contains(&key.as_str()) {
            return fail("a key that the fields do not list", "unknown_key");
        }
    }
    for key in &keys {
        match fields.0.iter().filter(|(k, _)| k == key).count() {
            0 => return fail(key, "missing_key"),
            1 => {}
            _ => return fail(key, "repeated_key"),
        }
    }
    for key in &keys {
        let types: Vec<FieldType> = entry
            .fields
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, t)| *t)
            .collect();
        if let Some((_, value)) = fields.0.iter().find(|(k, _)| k == key) {
            if let Err(rule) = walk(value, &types) {
                return fail(key, rule);
            }
        }
    }
    Ok(())
}

/// The marker of the field types that a bounded or kept record of every origin may hold: a random id, a name of a
/// closed list (every enum of names), an app id, a stored domain, a site hash, a site value, a count or a day
/// (`u32`), the objects of counts and days of this run's records, and `Option` and `Vec` of them. No type of free
/// text, token, instant or ordinal is `Coarse`. A later run's kinds are covered by `check_registry` alone.
pub trait Coarse {}

/// Compiles only for a `Coarse` type. The assertion of `src/record.rs` calls it for every field type of the
/// summary, the support row and both shapes of the workflow.
pub const fn assert_coarse<T: Coarse + ?Sized>() {}

impl Coarse for u32 {}
impl<T: Coarse> Coarse for Option<T> {}
impl<T: Coarse> Coarse for Vec<T> {}
impl Coarse for WorkflowId {}
impl Coarse for SummaryId {}
impl Coarse for AgentId {}
impl Coarse for RequestId {}
impl Coarse for AppId {}
impl Coarse for SiteDomain {}
impl Coarse for SiteHash {}
impl Coarse for SiteValue {}
impl Coarse for Confidence {}
impl Coarse for Recurrence {}
impl Coarse for KeyCounts {}
impl Coarse for PointerCounts {}
