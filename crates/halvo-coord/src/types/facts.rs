use super::names::names;
use super::{AgentId, Event, FullWorkflow, ItemId, ItemKind, LivePhase, Name, PastRun};

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
    /// Its ledger events in ascending `seq` (I7.1).
    pub events: Vec<Event>,
    /// Whether the agent store has ended the agent's newest run: no run of the agent was
    /// started, or the store's newest run record names the newest run that was started. The
    /// coordinator reads it from the owner for each start, through `Owner::agents` (I5.1, I5.5
    /// step 3, ruling XB-2).
    pub store_ended: bool,
    /// The day of the coordinator's `Clock` (I5.9).
    pub today: u32,
    /// Its run state and its open item.
    pub doing: Doing,
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

/// An open item of an agent with its kind, as `Doing::item` gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenItem {
    /// The item.
    pub item: ItemId,
    /// Its kind.
    pub kind: ItemKind,
}
