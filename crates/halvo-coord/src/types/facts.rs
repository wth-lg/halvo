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
    /// The day of the coordinator's `Clock` (I5.9).
    pub today: u32,
    /// Whether it is idle, queued or live in a phase, with its open item.
    pub doing: Doing,
}

/// What an agent does, of the coordinator's own state (I5.1), in one value: idle, with the
/// item that a hold or the scheduler raised or none; a start in the queue; or a live run in a
/// phase of I5.6, the phase `waiting` holding the run's open item.
///
/// ```
/// use halvo_coord::types::{Doing, ItemId, ItemKind, Live, LivePhase, OpenItem};
/// let item = OpenItem {
///     item: ItemId::parse("00000000000000000000000000000003").unwrap(),
///     kind: ItemKind::Confirm,
/// };
/// let doing = Doing::Live(Live::Waiting(item));
/// assert_eq!(doing.phase(), Some(LivePhase::Waiting));
/// ```
///
/// A waiting run without its item cannot be written:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{Doing, Live};
/// let doing = Doing::Live(Live::Waiting);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Doing {
    /// No run of it is live and no start of it is queued; the item that a hold or the
    /// scheduler raised, or none.
    Idle(Option<OpenItem>),
    /// A start of it stands in the queue.
    Queued,
    /// A run of it is live.
    Live(Live),
}

/// The phase of a live run (I5.6); `waiting` holds the open item of the run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Live {
    /// `starting`.
    Starting,
    /// `working`.
    Working,
    /// `waiting`, with the item that the run opened.
    Waiting(OpenItem),
    /// `stopping`.
    Stopping,
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
            Doing::Idle(_) | Doing::Queued => None,
        }
    }

    /// Its open item, or none.
    pub const fn item(&self) -> Option<&OpenItem> {
        match self {
            Doing::Idle(item) => item.as_ref(),
            Doing::Live(Live::Waiting(item)) => Some(item),
            Doing::Queued | Doing::Live(_) => None,
        }
    }
}

/// An open item of an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenItem {
    /// The item.
    pub item: ItemId,
    /// Its kind.
    pub kind: ItemKind,
}
