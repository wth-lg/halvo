use halvo_schema::executable::names::AgentState;
use halvo_schema::{AgentId, ErrorName, Instant, RowRead, TypedRecord, WorkflowId, WorkflowItem};

/// The four calls that a store offers a handler while the handler answers one request (W-C25.5), and no other.
/// The host calls a handler to its end before it passes it the next request, so no other request changes the rows
/// between a handler's reads and its commits. Each commit is a transaction of its own: a handler that makes a commit
/// and then a state call (`create_agent`) leaves the commit stored when the state call is refused.
pub trait Host {
    /// The typed commit (W-C25.3): every record of the list, each a kind of the family `agent` and its compact
    /// JSON, in one transaction, or none of them with the error of the refusal.
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName>;

    /// The state call (W-C25.7): sets the stored workflow's state and its agent, or answers `no_workflow` for a
    /// workflow that is not stored and `state` for one that is `proposed` or `dismissed` and for a call that would
    /// leave an `active` or a `paused` workflow with no agent. A call that asks for what is stored succeeds.
    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: AgentState,
        agent: Option<&AgentId>,
    ) -> Result<(), ErrorName>;

    /// The rows of the family `agent` as they stand at the call (W-C2.11).
    fn rows(&self) -> &dyn RowRead;

    /// The item of the read `workflows` for one stored workflow, or none where it is not stored (W-C2.10).
    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem>;
}

/// The instant and the local day number of a request, as the host reads them from its clock. The rows that a
/// handler writes beyond the body's own record (the moves of the run's end, the record of `end_run`, a session
/// part) take them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub at: Instant,
    pub day: u32,
}
