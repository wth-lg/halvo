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

/// An item, as the page gets it (I3.4), made by the one constructor of its kind: a
/// confirmation has its presenter and an empty `text`, every other kind a null presenter;
/// `fields` stand only in a confirmation and a check, and `params` only in a setup.
///
/// ```
/// use halvo_coord::types::{AgentId, Item, ItemId, ItemKind, YesNo};
/// let ready = Item::ready(
///     ItemId::parse("00000000000000000000000000000003").unwrap(),
///     AgentId::parse("00000000000000000000000000000002").unwrap(),
///     YesNo::No,
///     String::new(),
///     String::new(),
///     vec![],
/// );
/// assert_eq!((ready.kind(), ready.presenter()), (ItemKind::Ready, None));
/// ```
///
/// A `ready` item with a presenter cannot be written:
///
/// ```compile_fail,E0451
/// use halvo_coord::types::{AgentId, Item, ItemId, ItemKind, Presenter, YesNo};
/// let ready = Item {
///     item: ItemId::parse("00000000000000000000000000000003").unwrap(),
///     agent: AgentId::parse("00000000000000000000000000000002").unwrap(),
///     kind: ItemKind::Ready,
///     presenter: Some(Presenter::Page),
///     armed: YesNo::No,
///     title: String::new(),
///     text: String::new(),
///     fields: vec![],
///     params: vec![],
///     choices: vec![],
/// };
/// ```
///
/// Nor can a confirmation without its presenter:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{AgentId, Item, ItemId, YesNo};
/// let confirm = Item::confirm(
///     ItemId::parse("00000000000000000000000000000003").unwrap(),
///     AgentId::parse("00000000000000000000000000000002").unwrap(),
///     None,
///     YesNo::Yes,
///     String::new(),
///     vec![],
///     vec![],
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    item: ItemId,
    agent: AgentId,
    kind: ItemKind,
    presenter: Option<Presenter>,
    armed: YesNo,
    title: String,
    text: String,
    fields: Vec<Field>,
    params: Vec<ParamEntry>,
    choices: Vec<ChoiceText>,
}

impl Item {
    fn of_kind(
        kind: ItemKind,
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
    ) -> Item {
        Item {
            item,
            agent,
            kind,
            presenter: None,
            armed,
            title,
            text,
            fields: vec![],
            params: vec![],
            choices: vec![],
        }
    }

    /// A `confirm` item: its presenter, its title, the bound fields of its effects and its
    /// choices (I4.3), with an empty `text`.
    pub fn confirm(
        item: ItemId,
        agent: AgentId,
        presenter: Presenter,
        armed: YesNo,
        title: String,
        fields: Vec<Field>,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            presenter: Some(presenter),
            fields,
            choices,
            ..Item::of_kind(ItemKind::Confirm, item, agent, armed, title, String::new())
        }
    }

    /// A `sign_in` item.
    pub fn sign_in(
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::SignIn, item, agent, armed, title, text)
        }
    }

    /// A `link_account` item.
    pub fn link_account(
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::LinkAccount, item, agent, armed, title, text)
        }
    }

    /// A `stuck` item.
    pub fn stuck(
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::Stuck, item, agent, armed, title, text)
        }
    }

    /// A `check` item with the kept fields of its effect.
    pub fn check(
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
        fields: Vec<Field>,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            fields,
            choices,
            ..Item::of_kind(ItemKind::Check, item, agent, armed, title, text)
        }
    }

    /// A `setup` item with its parameters (I3.5).
    pub fn setup(
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
        params: Vec<ParamEntry>,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            params,
            choices,
            ..Item::of_kind(ItemKind::Setup, item, agent, armed, title, text)
        }
    }

    /// A `ready` item.
    pub fn ready(
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::Ready, item, agent, armed, title, text)
        }
    }

    /// A `show` item.
    pub fn show(
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::Show, item, agent, armed, title, text)
        }
    }

    /// `item`.
    pub fn item(&self) -> &ItemId {
        &self.item
    }

    /// `agent`.
    pub fn agent(&self) -> &AgentId {
        &self.agent
    }

    /// `kind`.
    pub fn kind(&self) -> ItemKind {
        self.kind
    }

    /// `presenter`: none for every kind but `confirm`.
    pub fn presenter(&self) -> Option<Presenter> {
        self.presenter
    }

    /// `armed`.
    pub fn armed(&self) -> YesNo {
        self.armed
    }

    /// `title`.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// `text`: empty for a confirmation.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// `fields`: the bound fields of a confirmation or the kept fields of a check.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// `params`: empty but for `setup`.
    pub fn params(&self) -> &[ParamEntry] {
        &self.params
    }

    /// `choices`: the safe one first.
    pub fn choices(&self) -> &[ChoiceText] {
        &self.choices
    }
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
