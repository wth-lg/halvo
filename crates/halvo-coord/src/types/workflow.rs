use super::{
    AgentId, AgentState, Category, Class, Digest, EffectId, Name, Outcome, RunId, WorkflowId,
};
use super::{LastCause, Param, WorkflowState, WorkflowTrigger};
use std::fmt;
use std::num::NonZeroU32;

/// A workflow of the owner's list, in the two shapes of W-C2.4: a dismissed workflow has no
/// state value of `WorkflowState` (I5.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Workflow {
    /// The full shape, with one of the four states.
    Full(FullWorkflow),
    /// The dismissed shape.
    Dismissed(DismissedWorkflow),
}

impl Workflow {
    /// The workflow's id, in either shape.
    pub fn id(&self) -> &WorkflowId {
        match self {
            Workflow::Full(w) => &w.id,
            Workflow::Dismissed(w) => &w.id,
        }
    }
}

/// The full shape of a workflow, with what the coordinator reads of it (W-C2.4, I5.10).
///
/// ```
/// use halvo_coord::types::{Category, FullWorkflow, WorkflowId, WorkflowState, WorkflowTrigger};
/// use std::num::NonZeroU32;
/// let w = FullWorkflow {
///     id: WorkflowId::parse("00000000000000000000000000000001").unwrap(),
///     state: WorkflowState::Active,
///     category: Category::CalendarCleanup,
///     site: None,
///     trigger: WorkflowTrigger::Schedule,
///     period: NonZeroU32::new(7),
///     last: 20_750,
///     agent: None,
/// };
/// ```
///
/// A period of 0 days cannot be written:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{Category, FullWorkflow, WorkflowId, WorkflowState, WorkflowTrigger};
/// let w = FullWorkflow {
///     id: WorkflowId::parse("00000000000000000000000000000001").unwrap(),
///     state: WorkflowState::Active,
///     category: Category::CalendarCleanup,
///     site: None,
///     trigger: WorkflowTrigger::Schedule,
///     period: Some(0),
///     last: 20_750,
///     agent: None,
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FullWorkflow {
    /// `id`.
    pub id: WorkflowId,
    /// `state`.
    pub state: WorkflowState,
    /// `category`, a seed category.
    pub category: Category,
    /// The registrable domain of the domain form of `site` (W-C2.3); none for null and for the
    /// title form, which holds a hash and no host.
    pub site: Option<String>,
    /// `trigger`.
    pub trigger: WorkflowTrigger,
    /// `recurrence.period`: days between occurrences, 1 to 93, or none; never 0.
    pub period: Option<NonZeroU32>,
    /// `recurrence.last`: the day of the last occurrence counted, as days since 1970-01-01.
    pub last: u32,
    /// `agent`, or none.
    pub agent: Option<AgentId>,
}

/// The dismissed shape of a workflow (W-C2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DismissedWorkflow {
    /// `id`.
    pub id: WorkflowId,
    /// `category`.
    pub category: Category,
    /// As `FullWorkflow::site`.
    pub site: Option<String>,
}

/// One entry of the reply of `list_agents` (I6.1). `workflow` and `agent` are the ids of its
/// configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent<C> {
    /// The configuration's `workflow`.
    pub workflow: WorkflowId,
    /// The configuration's `agent`.
    pub agent: AgentId,
    /// `config`, the record of A-A2.1.
    pub config: C,
    /// `state`.
    pub state: AgentState,
    /// `slots`: the slot names of the newest bindings.
    pub slots: Vec<Name>,
    /// `session`: whether a session is stored.
    pub session: bool,
    /// `window`: whether the agent's next run needs a browser window, which the agent store reads
    /// from the agent's own rows (r3 U5.15).
    pub window: bool,
    /// `last`: the last run record, or none.
    pub last: Option<PastRun>,
    /// `unknown`: the effects of an unknown outcome.
    pub unknown: Vec<Unknown>,
    /// `refused_run`: the agent's newest started run whose log holds a `step_failed` row with
    /// the cause `unauthorized`, or none (the agent-setup run's contracts line 305).
    pub refused_run: Option<RunId>,
    /// `last_cause`: the cause of the end of the agent's newest started run where the roster
    /// has a sentence for it, or none: also for a cause of the store's list that `LastCause`
    /// lacks.
    pub last_cause: Option<LastCause>,
}

/// The stored answers of the four questions of the spreadsheet agent, as `Owner::sheet_answers`
/// gives them (r3 U11.7): for each of the slots `workbook`, `sheet`, `range` and `source` of the
/// agent's newest bindings, the stored value as one text of 1 to 1,024 bytes, byte for byte the
/// value that `set_bindings` stored, or none where the store holds no value of it. The four fields
/// are the whole list: no other slot has a place here, and `of` answers none for every other
/// parameter. `Debug` names which fields hold a value and prints no value.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SheetAnswers {
    /// `workbook`: Which spreadsheet file.
    pub workbook: Option<String>,
    /// `sheet`: Which sheet.
    pub sheet: Option<String>,
    /// `range`: Which cells.
    pub range: Option<String>,
    /// `source`: Where the data comes from.
    pub source: Option<String>,
}

impl SheetAnswers {
    /// The stored answer of the parameter: the field of its name for the four of the category
    /// `spreadsheet_fill`, and none for every other parameter.
    pub fn of(&self, param: Param) -> Option<&str> {
        match param {
            Param::Workbook => self.workbook.as_deref(),
            Param::Sheet => self.sheet.as_deref(),
            Param::Range => self.range.as_deref(),
            Param::Source => self.source.as_deref(),
            Param::Start
            | Param::Payee
            | Param::Instrument
            | Param::ReplySenders
            | Param::Seller => None,
        }
    }
}

impl fmt::Debug for SheetAnswers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SheetAnswers")
            .field("workbook", &self.workbook.is_some())
            .field("sheet", &self.sheet.is_some())
            .field("range", &self.range.is_some())
            .field("source", &self.source.is_some())
            .finish()
    }
}

/// What the coordinator reads of a run record `agent_run` (A-A7.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastRun {
    /// `run`.
    pub run: RunId,
    /// `day`.
    pub day: u32,
    /// `outcome`.
    pub outcome: Outcome,
    /// `unknown`: the count of effects of an unknown outcome.
    pub unknown: u32,
}

/// An effect of an unknown outcome in the reply of I6.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unknown {
    /// `run`.
    pub run: RunId,
    /// `effect`.
    pub effect: EffectId,
    /// `day`.
    pub day: u32,
    /// `class`.
    pub class: Class,
    /// `kept`: the kept fields of A-A9.3.
    pub kept: Vec<Kept>,
}

/// A kept field of an effect: its name and its value in pieces (A-A7.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    /// `name`.
    pub name: Name,
    /// `value`.
    pub value: Vec<String>,
}

/// One run of the reply of the body `agent_history` (I6.1): the run's record of A-A7.4 and its
/// effects in the order of the reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunHistory {
    /// The run's record.
    pub run: PastRun,
    /// Its effects.
    pub effects: Vec<PastEffect>,
}

/// One effect of a run of the reply of `agent_history` (I6.1): its effect id and the digest of
/// its last row (ruling RD-85 item 4), its class, its state of A-A5.2, the cause of its last row
/// or none, and its kept fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastEffect {
    /// `effect`.
    pub effect: EffectId,
    /// `digest`: of the effect's last row.
    pub digest: Digest,
    /// `class`.
    pub class: Class,
    /// `state`.
    pub state: crate::guard::EffectState,
    /// `cause`, or none.
    pub cause: Option<Name>,
    /// `kept`.
    pub kept: Vec<Kept>,
}
