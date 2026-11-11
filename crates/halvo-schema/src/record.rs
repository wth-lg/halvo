use std::fmt;

use serde::de::{self, DeserializeOwned, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::category::SeedCategory;
use crate::ids::{
    name_enum, object, parse_str, strict_struct, AgentId, ParseError, SummaryId, Version,
    WorkflowId, MAX_ORDINAL, VERSION,
};
use crate::site::{AppId, Domain, ObservedSite, SiteDomain, SiteToken, SiteValue};

/// An instant: a signed count of nanoseconds since 1970-01-01T00:00:00Z, a JSON integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Instant(pub i64);

/// A segment ordinal of the capture store, from 0 to 2^63 - 1, the range of a position's `seg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ordinal(u64);

impl Ordinal {
    /// Returns the ordinal when `n` is at most 2^63 - 1, else the error with the rule `range`.
    pub fn new(n: u64) -> Result<Ordinal, ParseError> {
        if n > MAX_ORDINAL {
            return Err(ParseError::new("Ordinal", "range"));
        }
        Ok(Ordinal(n))
    }

    /// The number.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl Serialize for Ordinal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for Ordinal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ordinal::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A text field: a string of at most 1,024 bytes. A sender masks with `halvo_redact::mask` first and then cuts.
/// `Debug` prints the type name alone.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Text(String);

impl Text {
    /// Returns the text when it has at most 1,024 bytes, else the error with the rule `length`.
    pub fn parse(s: &str) -> Result<Text, ParseError> {
        if s.len() > 1024 {
            return Err(ParseError::new("Text", "length"));
        }
        Ok(Text(s.to_owned()))
    }

    /// Keeps the longest prefix of whole characters of `s` within 1,024 bytes.
    pub fn cut(s: &str) -> Text {
        let mut end = s.len().min(1024);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        Text(s[..end].to_owned())
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Text")
    }
}

impl Serialize for Text {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        parse_str(deserializer, "a string of at most 1,024 bytes", Text::parse)
    }
}

/// The confidence of a workflow, from 0 to 100. Its reader and its `Serialize` refuse a value above 100.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Confidence(pub u32);

impl Serialize for Confidence {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0 > 100 {
            return Err(serde::ser::Error::custom(ParseError::new(
                "Confidence",
                "range",
            )));
        }
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for Confidence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let n = u32::deserialize(deserializer)?;
        if n > 100 {
            return Err(serde::de::Error::custom(ParseError::new(
                "Confidence",
                "range",
            )));
        }
        Ok(Confidence(n))
    }
}

name_enum! {
    /// How a routine starts: in the app or on the site, on a schedule with a period, or by the user.
    pub enum Trigger {
        AppContext = "app_context",
        Schedule = "schedule",
        Manual = "manual",
    }
}

name_enum! {
    /// The stored states of the full shape of a workflow. `dismissed` has a shape of its own, and a deleted
    /// workflow leaves no row.
    pub enum WorkflowState {
        Proposed = "proposed",
        Approved = "approved",
        Active = "active",
        Paused = "paused",
    }
}

name_enum! {
    /// The part of the local day in which an episode began: from 00:00, 06:00, 12:00 and 18:00.
    pub enum Part {
        Night = "night",
        Morning = "morning",
        Afternoon = "afternoon",
        Evening = "evening",
    }
}

name_enum! {
    /// The input classes of an observed step: the key classes, then the pointer classes.
    pub enum InputClass {
        Printable = "printable",
        Editing = "editing",
        Navigation = "navigation",
        Enter = "enter",
        Tab = "tab",
        Shortcut = "shortcut",
        Other = "other",
        Click = "click",
        Scroll = "scroll",
        Drag = "drag",
        Move = "move",
    }
}

name_enum! {
    /// The content floor, the one layer that fills the app, the title and the input of an observed step.
    pub enum FloorLayer {
        Floor = "floor",
    }
}

name_enum! {
    /// The address field, the one layer that fills the site of an observed step.
    pub enum AddressFieldLayer {
        AddressField = "address_field",
    }
}

name_enum! {
    /// The two layers that fill the text of an observed step.
    pub enum TextLayer {
        AccessibilityText = "accessibility_text",
        ExtractedText = "extracted_text",
    }
}

fn minutes(type_name: &'static str, m: u32) -> Result<(), ParseError> {
    if !m.is_multiple_of(5) || !(5..=1440).contains(&m) {
        return Err(ParseError::new(type_name, "minutes"));
    }
    Ok(())
}

fn segments(type_name: &'static str, s: &[Ordinal]) -> Result<(), ParseError> {
    if s.is_empty() || !s.windows(2).all(|w| w[0] < w[1]) {
        return Err(ParseError::new(type_name, "segments"));
    }
    Ok(())
}

fn range(type_name: &'static str, start: Instant, end: Instant) -> Result<(), ParseError> {
    if end <= start {
        return Err(ParseError::new(type_name, "range"));
    }
    Ok(())
}

strict_struct! {
    /// How often a routine recurred: `count` occurrences, 1 or more; `period`, the days between them, 1 to 93 or
    /// none; and the days of the first and the last occurrence, `first` not after `last`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Recurrence {
        pub count: u32,
        pub period: Option<u32>,
        pub first: u32,
        pub last: u32,
    }
    rules(r) {
        if r.count == 0 {
            return Err(ParseError::new("Recurrence", "count"));
        }
        if r.period.is_some_and(|p| !(1..=93).contains(&p)) {
            return Err(ParseError::new("Recurrence", "period"));
        }
        if r.first > r.last {
            return Err(ParseError::new("Recurrence", "first_last"));
        }
        Ok(())
    }
}

strict_struct! {
    /// The kept record of a workflow in a state other than `dismissed`, the full shape of the kind `workflow`:
    /// `{"v":1,"id":..,"state":..,"category":..,"site":..,"app":..,"trigger":..,"recurrence":..,"confidence":..,"agent":..}`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct FullWorkflow [v] {
        pub id: WorkflowId,
        pub state: WorkflowState,
        pub category: SeedCategory,
        pub site: Option<SiteValue>,
        pub app: AppId,
        pub trigger: Trigger,
        pub recurrence: Recurrence,
        pub confidence: Confidence,
        pub agent: Option<AgentId>,
    }
    rules(_w) {
        Ok(())
    }
}

/// The kept record of a dismissed workflow: `{"v":1,"id":..,"state":"dismissed","category":..,"site":..,"app":..}`
/// and nothing else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DismissedWorkflow {
    pub id: WorkflowId,
    pub category: SeedCategory,
    pub site: Option<SiteValue>,
    pub app: AppId,
}

impl Serialize for DismissedWorkflow {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Out<'a> {
            v: u32,
            id: &'a WorkflowId,
            state: &'static str,
            category: SeedCategory,
            site: &'a Option<SiteValue>,
            app: &'a AppId,
        }
        Out {
            v: VERSION,
            id: &self.id,
            state: "dismissed",
            category: self.category,
            site: &self.site,
            app: &self.app,
        }
        .serialize(serializer)
    }
}

struct DismissedState;

impl<'de> Deserialize<'de> for DismissedState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        parse_str(deserializer, "the state dismissed", |s| {
            if s == "dismissed" {
                Ok(DismissedState)
            } else {
                Err(ParseError::new("DismissedWorkflow", "state"))
            }
        })
    }
}

impl<'de> Deserialize<'de> for DismissedWorkflow {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        #[allow(dead_code)]
        struct In {
            v: Version,
            id: WorkflowId,
            state: DismissedState,
            category: SeedCategory,
            #[serde(deserialize_with = "crate::ids::required")]
            site: Option<SiteValue>,
            app: AppId,
        }
        let input: In = object(deserializer)?;
        Ok(DismissedWorkflow {
            id: input.id,
            category: input.category,
            site: input.site,
            app: input.app,
        })
    }
}

/// The kept record of the kind `workflow`, in one of its two shapes, told apart by `state`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Workflow {
    /// The states `proposed`, `approved`, `active` and `paused`.
    Full(FullWorkflow),
    /// The state `dismissed`.
    Dismissed(DismissedWorkflow),
}

impl Workflow {
    /// The workflow id of either shape.
    pub fn id(&self) -> &WorkflowId {
        match self {
            Workflow::Full(w) => &w.id,
            Workflow::Dismissed(w) => &w.id,
        }
    }
}

impl Serialize for Workflow {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Workflow::Full(w) => w.serialize(serializer),
            Workflow::Dismissed(w) => w.serialize(serializer),
        }
    }
}

struct Entries(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Entries;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Entries, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, Box<RawValue>>()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_map(V)
    }
}

fn shape<T: DeserializeOwned, E: de::Error>(text: &str) -> Result<T, E> {
    serde_json::from_str(text).map_err(|e| {
        let message = e.to_string();
        let position = format!(" at line {} column {}", e.line(), e.column());
        E::custom(message.strip_suffix(&position).unwrap_or(&message))
    })
}

impl<'de> Deserialize<'de> for Workflow {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let Entries(entries) = Entries::deserialize(deserializer)?;
        let mut states = entries.iter().filter(|(k, _)| k == "state");
        let dismissed = match (states.next(), states.next()) {
            (Some((_, state)), None) => {
                serde_json::from_str::<String>(state.get()).is_ok_and(|s| s == "dismissed")
            }
            _ => false,
        };
        let fields: Vec<String> = entries
            .iter()
            .map(|(k, v)| format!("{}:{}", serde_json::Value::from(k.as_str()), v.get()))
            .collect();
        let text = format!("{{{}}}", fields.join(","));
        if dismissed {
            shape(&text).map(Workflow::Dismissed)
        } else {
            shape(&text).map(Workflow::Full)
        }
    }
}

strict_struct! {
    /// Key presses per class of the capture record's `keystroke` kind.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct KeyCounts {
        pub printable: u32,
        pub editing: u32,
        pub navigation: u32,
        pub enter: u32,
        pub tab: u32,
        pub shortcut: u32,
        pub other: u32,
    }
    rules(_k) {
        Ok(())
    }
}

strict_struct! {
    /// Pointer counts: `click` counts click records; `scroll`, `drag` and `move` sum the `count` of their records,
    /// a sum above 4,294,967,295 written as 4,294,967,295.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct PointerCounts {
        pub click: u32,
        pub scroll: u32,
        pub drag: u32,
        pub r#move: u32,
    }
    rules(_p) {
        Ok(())
    }
}

strict_struct! {
    /// The bounded record of the kind `summary`, one per episode in a seed category. `minutes` is a multiple of 5
    /// from 5 to 1,440; its reader and its `Serialize` refuse any other value.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Summary [v] {
        pub id: SummaryId,
        pub category: SeedCategory,
        pub site: Option<SiteValue>,
        pub app: AppId,
        pub day: u32,
        pub part: Part,
        pub minutes: u32,
        pub keys: Option<KeyCounts>,
        pub pointer: Option<PointerCounts>,
    }
    rules(s) {
        minutes("Summary", s.minutes)
    }
}

strict_struct! {
    /// The bounded record of the kind `support`: one pair of a workflow and a summary that supports it, with the
    /// day of the summary.
    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    pub struct Support [v] {
        pub workflow: WorkflowId,
        pub summary: SummaryId,
        pub day: u32,
    }
    rules(_s) {
        Ok(())
    }
}

/// A value of an observed step with the capture layer that filled it: `{"value":<v>,"layer":<layer>}`.
/// The layer type of each field of `ObservedStep` holds only the layers that may fill that field.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Tagged<T, L> {
    pub value: T,
    pub layer: L,
}

impl<'de, T: Deserialize<'de>, L: Deserialize<'de>> Deserialize<'de> for Tagged<T, L> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct In<T, L> {
            value: T,
            layer: L,
        }
        let input: In<T, L> = object(deserializer)?;
        Ok(Tagged {
            value: input.value,
            layer: input.layer,
        })
    }
}

strict_struct! {
    /// An observed step: every field optional and tagged with the capture layer that filled it. The app, the
    /// masked title and the input class come from the floor, the site from the address field, and the text from
    /// the accessibility text or the extracted text. A step holds no locator and no AppUserModelID.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ObservedStep {
        pub app: Option<Tagged<AppId, FloorLayer>>,
        pub title: Option<Tagged<Text, FloorLayer>>,
        pub input: Option<Tagged<InputClass, FloorLayer>>,
        pub site: Option<Tagged<SiteDomain, AddressFieldLayer>>,
        pub text: Option<Tagged<Text, TextLayer>>,
    }
    rules(_s) {
        Ok(())
    }
}

strict_struct! {
    /// The window record of the kind `evidence`: the evidence of one episode. `segments` is ascending, each once,
    /// at least one; the range `start`..`end` is half-open and `end` is after `start`.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Evidence [v] {
        pub summary: SummaryId,
        pub segments: Vec<Ordinal>,
        pub expires: Instant,
        pub start: Instant,
        pub end: Instant,
        pub hint: Option<SiteToken>,
        pub steps: Vec<ObservedStep>,
    }
    rules(e) {
        segments("Evidence", &e.segments)?;
        range("Evidence", e.start, e.end)
    }
}

strict_struct! {
    /// An episode of the analysis write: the fields of its summary with the site in its observed form, and the
    /// fields of its evidence without `summary` and `hint`. The rules of both hold, and the site of each step is
    /// a `Domain`, rule 5 included.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Episode {
        pub id: SummaryId,
        pub category: SeedCategory,
        pub site: Option<ObservedSite>,
        pub app: AppId,
        pub day: u32,
        pub part: Part,
        pub minutes: u32,
        pub keys: Option<KeyCounts>,
        pub pointer: Option<PointerCounts>,
        pub segments: Vec<Ordinal>,
        pub expires: Instant,
        pub start: Instant,
        pub end: Instant,
        pub steps: Vec<ObservedStep>,
    }
    rules(e) {
        minutes("Episode", e.minutes)?;
        segments("Episode", &e.segments)?;
        range("Episode", e.start, e.end)?;
        for step in &e.steps {
            if let Some(site) = &step.site {
                Domain::parse(site.value.as_str())?;
            }
        }
        Ok(())
    }
}

fn next_rule<T: PartialEq>(
    type_name: &'static str,
    last: Option<&T>,
    next: &Option<T>,
) -> Result<(), ParseError> {
    match next {
        Some(n) if last != Some(n) => Err(ParseError::new(type_name, "next")),
        _ => Ok(()),
    }
}

strict_struct! {
    /// One workflow of a workflows page, with the ids of its support set in ascending order and its hint.
    /// A dismissed workflow has no summaries, a workflow with no summaries has no hint, and a workflow whose site
    /// is null or a domain has no hint.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct WorkflowItem {
        pub workflow: Workflow,
        pub summaries: Vec<SummaryId>,
        pub hint: Option<SiteToken>,
    }
    rules(i) {
        if !i.summaries.windows(2).all(|w| w[0] < w[1]) {
            return Err(ParseError::new("WorkflowItem", "order"));
        }
        if matches!(i.workflow, Workflow::Dismissed(_)) && !i.summaries.is_empty() {
            return Err(ParseError::new("WorkflowItem", "dismissed"));
        }
        if i.summaries.is_empty() && i.hint.is_some() {
            return Err(ParseError::new("WorkflowItem", "hint"));
        }
        let site = match &i.workflow {
            Workflow::Full(w) => &w.site,
            Workflow::Dismissed(w) => &w.site,
        };
        if i.hint.is_some() && !matches!(site, Some(SiteValue::Title { .. })) {
            return Err(ParseError::new("WorkflowItem", "hint_site"));
        }
        Ok(())
    }
}

strict_struct! {
    /// The page of the read `workflows`: workflows in ascending order of workflow id; `next` is the id of the last
    /// item when more remain, and none at the end.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct WorkflowsPage {
        pub workflows: Vec<WorkflowItem>,
        pub next: Option<WorkflowId>,
    }
    rules(p) {
        if !p.workflows.windows(2).all(|w| w[0].workflow.id() < w[1].workflow.id()) {
            return Err(ParseError::new("WorkflowsPage", "order"));
        }
        next_rule("WorkflowsPage", p.workflows.last().map(|i| i.workflow.id()), &p.next)
    }
}

strict_struct! {
    /// The page of the read `evidence`: evidence records in ascending order of summary id; `next` is the summary id
    /// of the last item when more remain, and none at the end.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct EvidencePage {
        pub evidence: Vec<Evidence>,
        pub next: Option<SummaryId>,
    }
    rules(p) {
        if !p.evidence.windows(2).all(|w| w[0].summary < w[1].summary) {
            return Err(ParseError::new("EvidencePage", "order"));
        }
        next_rule("EvidencePage", p.evidence.last().map(|e| &e.summary), &p.next)
    }
}
