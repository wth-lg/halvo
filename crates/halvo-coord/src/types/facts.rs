use super::names::names;
use super::{
    AgentId, Event, FullWorkflow, ItemId, ItemKind, LivePhase, Name, PastRun, RunId, Unknown,
};

/// What the coordinator knows of one agent (I5.1), which `holds`, `start_step`, `status` and
/// `fixtures::facts` share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts<C> {
    /// The agent.
    pub agent: AgentId,
    /// Its workflow: state, trigger and recurrence of W-C2.4.
    pub workflow: FullWorkflow,
    /// Its configuration (I2.1).
    pub config: C,
    /// The slot names of its newest bindings.
    pub slots: Vec<Name>,
    /// Its last `history_runs` run records, newest first (A-A7.4).
    pub runs: Vec<PastRun>,
    /// The entries of the roster's key `unknown` that name the newest run of `runs`: the effects
    /// of that run whose outcome the agent store does not know, taken from the entry of
    /// `Owner::agents` where its `last` is taken (I5.1; ruling RD-81 8.1).
    pub unknown: Vec<Unknown>,
    /// The roster's key `refused_run` of the agent: its newest started run whose step the
    /// service refused with `unauthorized`, or none, taken from the entry of `Owner::agents`
    /// where its `last` is taken (I5.4; ruling RD-73 4.2).
    pub refused: Option<RunId>,
    /// Its ledger events in ascending `seq` (I7.1).
    pub events: Vec<Event>,
    /// Whether the agent store has ended the agent's newest run: no run of the agent was
    /// started, or the store's newest run record names the newest run that was started. The
    /// coordinator reads it from the owner for each start, through `Owner::agents` (I5.1, I5.5
    /// step 3, ruling XB-2).
    pub store_ended: bool,
    /// The move's read of the `agent_reply` rows of the newest run, asked for an agent of the
    /// category `email` alone (I5.1, I5.4; rulings RD-67 3.3, RD-81 8.4).
    pub replies: Replies,
    /// The day of the coordinator's `Clock` (I5.9).
    pub today: u32,
    /// Its run state and its open item.
    pub doing: Doing,
    /// Whether `workflow` is the stand-in that a new life assumes for a workflow that the owner's
    /// list holds in the dismissed shape, with the state `paused` (I5.8), which is no entry of
    /// that list and gives no derived hold `off` (I5.4; ruling RD-67 1.2).
    pub stand_in: bool,
    /// Whether this life marked the agent because the owner refused with `overlap` a
    /// `set_state` `active` of the tick's rule or of `activate` after a set-up, and no
    /// acknowledged switch-on came since: the agent then stands as off by the derived hold, also
    /// while the `paused` that follows has no acknowledgement (rulings RD-67 4.3, RD-160 (1)). A
    /// new life holds no mark; the state of `workflow` stays the owner's acknowledged answer.
    pub active_refused: bool,
}

impl<C> Facts<C> {
    /// Takes, of the entries of the roster's key `unknown` of one read, those that name the
    /// newest run of `runs`, in place of the ones held (I5.1; ruling RD-81 8.1).
    pub fn take_unknown(&mut self, entry: &[Unknown]) {
        let newest = self.runs.first().map(|r| &r.run);
        self.unknown = entry
            .iter()
            .filter(|u| Some(&u.run) == newest)
            .cloned()
            .collect();
    }
}

/// What an agent does, of the coordinator's own state (I5.1): one run state, idle, queued or
/// live in a phase of I5.6, and at most one open item. The phase `waiting` holds the item that
/// its run raised; an idle or queued agent may hold an item of a kind that no run raises; no
/// other pair can be written.
///
/// ```
/// use halvo_coord::types::{Doing, ItemId, OwnItem, OwnItemKind};
/// let ready = OwnItem {
///     item: ItemId::parse("00000000000000000000000000000003").unwrap(),
///     kind: OwnItemKind::Ready,
/// };
/// let doing = Doing::Queued(Some(ready));
/// assert_eq!(doing.phase(), None);
/// ```
///
/// An idle agent with an item that a run raised cannot be written:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{Doing, ItemId, RunItem, RunItemKind};
/// let confirm = RunItem {
///     item: ItemId::parse("00000000000000000000000000000003").unwrap(),
///     kind: RunItemKind::Confirm,
/// };
/// let doing = Doing::Idle(Some(confirm));
/// ```
///
/// Nor a waiting run with an item that no run raises:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{Doing, ItemId, Live, OwnItem, OwnItemKind};
/// let ready = OwnItem {
///     item: ItemId::parse("00000000000000000000000000000003").unwrap(),
///     kind: OwnItemKind::Ready,
/// };
/// let doing = Doing::Live(Live::Waiting(ready));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Doing {
    /// No run of it is live and no start of it is queued; an item that no run raised, or none.
    Idle(Option<OwnItem>),
    /// A start of it stands in the queue; an item that no run raised, or none.
    Queued(Option<OwnItem>),
    /// A run of it is live.
    Live(Live),
}

/// The phase of a live run (I5.6); `waiting` holds the item that the run raised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Live {
    /// `starting`.
    Starting,
    /// `working`.
    Working,
    /// `waiting`, with the item that the run raised.
    Waiting(RunItem),
    /// `stopping`.
    Stopping,
}

/// An open item that a request of its run raised (I3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunItem {
    /// The item.
    pub item: ItemId,
    /// Its kind.
    pub kind: RunItemKind,
}

names! {
    /// The kinds that a request of a run raises (I3.1): `confirm` from a confirm request,
    /// `sign_in`, `link_account` and `stuck` from a handback request, `check` from a handback
    /// request with the reason `effect_unknown`, and `show` from the first run in the mode
    /// `compile`.
    RunItemKind {
        Confirm = "confirm",
        SignIn = "sign_in",
        LinkAccount = "link_account",
        Stuck = "stuck",
        Check = "check",
        Show = "show",
    }
}

/// An open item that no run raised: the coordinator's own, from a hold or the scheduler (I3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnItem {
    /// The item.
    pub item: ItemId,
    /// Its kind.
    pub kind: OwnItemKind,
}

names! {
    /// The kinds that no run raises (I3.1): `check` from the hold `check`, `setup` from the hold
    /// `setup`, and `ready` from the scheduler.
    OwnItemKind {
        Check = "check",
        Setup = "setup",
        Ready = "ready",
    }
}

impl RunItemKind {
    /// The kind's name in `ItemKind`.
    pub const fn kind(self) -> ItemKind {
        match self {
            RunItemKind::Confirm => ItemKind::Confirm,
            RunItemKind::SignIn => ItemKind::SignIn,
            RunItemKind::LinkAccount => ItemKind::LinkAccount,
            RunItemKind::Stuck => ItemKind::Stuck,
            RunItemKind::Check => ItemKind::Check,
            RunItemKind::Show => ItemKind::Show,
        }
    }
}

impl OwnItemKind {
    /// The kind's name in `ItemKind`.
    pub const fn kind(self) -> ItemKind {
        match self {
            OwnItemKind::Check => ItemKind::Check,
            OwnItemKind::Setup => ItemKind::Setup,
            OwnItemKind::Ready => ItemKind::Ready,
        }
    }
}

impl Live {
    /// The phase's name.
    pub const fn phase(&self) -> LivePhase {
        match self {
            Live::Starting => LivePhase::Starting,
            Live::Working => LivePhase::Working,
            Live::Waiting(_) => LivePhase::Waiting,
            Live::Stopping => LivePhase::Stopping,
        }
    }
}

impl Doing {
    /// The phase of its live run, or none.
    pub const fn phase(&self) -> Option<LivePhase> {
        match self {
            Doing::Live(live) => Some(live.phase()),
            Doing::Idle(_) | Doing::Queued(_) => None,
        }
    }

    /// Its open item with its kind, or none.
    pub fn item(&self) -> Option<OpenItem> {
        match self {
            Doing::Idle(Some(own)) | Doing::Queued(Some(own)) => Some(OpenItem {
                item: own.item.clone(),
                kind: own.kind.kind(),
            }),
            Doing::Live(Live::Waiting(run)) => Some(OpenItem {
                item: run.item.clone(),
                kind: run.kind.kind(),
            }),
            Doing::Idle(None) | Doing::Queued(None) | Doing::Live(_) => None,
        }
    }
}

names! {
    /// The state of a row of `agent_reply`, the agent store's record of one state of the reply
    /// to one message (the agent-setup run's A7.4).
    ReplyState {
        Drafted = "drafted",
        Begun = "begun",
        Committed = "committed",
        Unknown = "unknown",
        Lost = "lost",
        Withheld = "withheld",
    }
}

/// One row of `agent_reply` of a run, as `Owner::replies` gives it (ruling RD-67 3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyRow {
    /// A row in a state that the port's reader knows.
    Read(ReplyState),
    /// A row that the port's reader could not read, such as one in a state that it does not
    /// know; the read keeps it and answers the other rows (ruling RD-81 8.4).
    Unreadable,
}

/// What one move learned of the `agent_reply` rows of an agent's newest run (I5.1; ruling RD-81
/// 8.4). No answer is kept from one move to the next (ruling XB-9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Replies {
    /// Not asked in the move: the agent is not of the category `email`, the agent store has not
    /// ended its newest run, or a `hold_cleared` of the cause `check` names that run.
    NotAsked,
    /// Asked, and the read did not answer.
    NoAnswer,
    /// The rows that the read answered.
    Rows(Vec<ReplyRow>),
}

impl Replies {
    /// Whether the read answered with a row that leaves the fate of its reply not known.
    pub fn not_known(&self) -> bool {
        matches!(self, Replies::Rows(rows) if rows.iter().any(|r| r.not_known()))
    }
}

impl ReplyRow {
    /// Whether the row leaves the fate of its reply not known: a row in the state `unknown`, or
    /// one that the reader could not read (ruling RD-81 8.4).
    pub const fn not_known(self) -> bool {
        matches!(
            self,
            ReplyRow::Read(ReplyState::Unknown) | ReplyRow::Unreadable
        )
    }
}

/// An open item of an agent with its kind, as `Doing::item` gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenItem {
    /// The item.
    pub item: ItemId,
    /// Its kind.
    pub kind: ItemKind,
}
