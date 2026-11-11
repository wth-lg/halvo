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
    /// The phase of its live run, or none.
    pub live: Option<LivePhase>,
    /// Whether a start of it stands in the queue.
    pub queued: bool,
    /// Its open item, or none.
    pub item: Option<OpenItem>,
}

/// An open item of an agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenItem {
    /// The item.
    pub item: ItemId,
    /// Its kind.
    pub kind: ItemKind,
}
