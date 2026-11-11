use super::{AgentId, CandidateFrom, Cause, Choice, Control, FieldFrom, ItemId, ItemKind};
use super::{LineKey, Param, Presenter, Status, WorkflowId, YesNo};
use serde::Serialize;

/// The reply of `agents_snapshot` and of `agents_next` with a higher `seq` (I8.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    /// `seq`: this run's own count of states, from 1.
    pub seq: u32,
    /// `rows`, in the order of I2.4.
    pub rows: Vec<Row>,
    /// `items`.
    pub items: Vec<Item>,
    /// `note`, or null.
    pub note: Option<String>,
}

/// A row of the list of agents (I2.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    /// `agent`.
    pub agent: AgentId,
    /// `workflow`.
    pub workflow: WorkflowId,
    /// `name`: the composed name of I2.3.
    pub name: String,
    /// `status`.
    pub status: Status,
    /// `word`: the text of `status.<status>`.
    pub word: String,
    /// `line`: the text of the status line of I1.3.
    pub line: String,
    /// `say`: the text of `say.<status>` with `{name}`.
    pub say: String,
    /// `level`: the text of `line.level.<reasoning>`.
    pub level: String,
    /// `next`: the day of the first slot after today, or null.
    pub next: Option<String>,
    /// `controls`.
    pub controls: Vec<ControlText>,
}

/// A control of a row with its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ControlText {
    /// `control`.
    pub control: Control,
    /// `text`: the text of `control.<control>`.
    pub text: String,
}

/// The card of an agent (I2.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Card {
    /// `agent`.
    pub agent: AgentId,
    /// `name`.
    pub name: String,
    /// `lines`: the settings lines of I2.2.
    pub lines: Vec<CardLine>,
    /// `runs`: at most `history_runs`, newest first.
    pub runs: Vec<CardRun>,
}

/// A settings line of a card (I2.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CardLine {
    /// `key`.
    pub key: LineKey,
    /// `text`.
    pub text: String,
}

/// A run of a card (I2.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CardRun {
    /// `day`: the day of its `run_started` event as text.
    pub day: String,
    /// `text`: the sentence of `line.run.<end>`.
    pub text: String,
}

/// An item, as the page gets it (I3.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    /// `item`.
    pub item: ItemId,
    /// `agent`.
    pub agent: AgentId,
    /// `kind`.
    pub kind: ItemKind,
    /// `presenter`: null for every kind but `confirm`.
    pub presenter: Option<Presenter>,
    /// `armed`.
    pub armed: YesNo,
    /// `title`.
    pub title: String,
    /// `text`: empty for a confirmation.
    pub text: String,
    /// `fields`: the bound fields of a confirmation or the kept fields of a check.
    pub fields: Vec<Field>,
    /// `params`: empty but for `setup`.
    pub params: Vec<ParamEntry>,
    /// `choices`: the safe one first.
    pub choices: Vec<ChoiceText>,
}

/// A field of an item (I3.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Field {
    /// `label`.
    pub label: String,
    /// `value`: pieces of at most 1,024 bytes, as bound or as the person gave them.
    pub value: Vec<String>,
    /// `from`.
    pub from: FieldFrom,
    /// `from_text`: the text of `from.<source>`.
    pub from_text: String,
}

/// A parameter of a setup item (I3.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ParamEntry {
    /// `name`.
    pub name: Param,
    /// `label`: the text of `param.<name>`.
    pub label: String,
    /// `candidates`: at most `setup_candidates`.
    pub candidates: Vec<Candidate>,
    /// `typed`: `yes` only for a parameter with no candidate.
    pub typed: YesNo,
}

/// A candidate of a parameter (I3.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// `n`: from 1.
    pub n: u32,
    /// `value`.
    pub value: Vec<String>,
    /// `from`.
    pub from: CandidateFrom,
}

/// A choice of an item with its text (I3.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChoiceText {
    /// `choice`.
    pub choice: Choice,
    /// `text`.
    pub text: String,
    /// `safe`.
    pub safe: YesNo,
}

/// The reply `{"ok":"yes"}` (I8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct OkReply {
    /// `ok`.
    pub ok: YesNo,
}

/// The reply `{"closed":"yes"}` of `agents_next` (I8.1, I8.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ClosedReply {
    /// `closed`.
    pub closed: YesNo,
}

/// The reply of a refused command (I8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ErrorReply {
    /// `error`.
    pub error: Cause,
}
