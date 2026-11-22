use halvo_schema::executable::names::AgentState;
use halvo_schema::{AgentId, ErrorName, Instant, RowRead, TypedRecord, WorkflowId, WorkflowItem};

/// The calls that a store offers a handler while the handler answers one request (W-C25.5), and no other: the six
/// of the store and the day of the host's clock. The host calls a handler to its end before it passes it the next
/// request, so no other request changes the rows between a handler's reads and its commits. Each commit is a
/// transaction of its own: a handler that makes a commit and then a state call (`create_agent`) leaves the commit
/// stored when the state call is refused.
pub trait Host {
    /// The typed commit (W-C25.3): every record of the list, each a kind of the family `agent` and its compact
    /// JSON, in one transaction, or none of them with the error of the refusal.
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName>;

    /// The removal call (`HandlerCx::remove_under` of the derived store): in one transaction with the typed commit of
    /// `records`, it removes every stored row of the kind `kind` of the family `agent` that names the workflow
    /// `workflow`, the rows that a read no longer hands out among them, stores the records after the removal, and
    /// answers how many rows it removed. A refusal, or a fault before the commit, leaves every row and stores no
    /// record; a fault of the checkpoint or of the report after the commit is answered `store` with the rows removed
    /// and the records stored. The derived store removes each row under its secure delete with the truncating
    /// checkpoint behind the commit, makes the call under a `coordinator_write` alone and answers `role` under a
    /// runner's request. A host that does not hand the call on keeps this body: it answers `unsupported` and removes
    /// nothing.
    fn remove_under(
        &mut self,
        _kind: &str,
        _workflow: &WorkflowId,
        _records: Vec<TypedRecord>,
    ) -> Result<u64, ErrorName> {
        Err(ErrorName::Unsupported)
    }

    /// The removal call of a person's kind (`HandlerCx::remove_person` of the derived store, r3 U7.6): in one
    /// transaction with the typed commit of `records`, it removes every stored row of the kind `kind` of the family
    /// `agent`, a kind of origin person whose rows name no workflow, the rows that a read no longer hands out among
    /// them, stores the records after the removal, and answers how many rows it removed. Its refusals and faults are
    /// those of `remove_under`, and it refuses a kind that has a workflow key. The derived store removes each row under
    /// its secure delete with the truncating checkpoint behind the commit, makes the call under a `coordinator_write`
    /// alone and answers `role` under a runner's request. A host that does not hand the call on keeps this body: it
    /// answers `unsupported` and removes nothing.
    fn remove_person(&mut self, _kind: &str, _records: Vec<TypedRecord>) -> Result<u64, ErrorName> {
        Err(ErrorName::Unsupported)
    }

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

    /// The local calendar day that holds the instant, counted in days from 1970-01-01 (W-C2.5): by the machine's
    /// time zone in the product, with UTC as local time in a replay and in the gate. It is the one source of every
    /// day that a handler builds, checks or bounds (CP-122).
    fn day(&self, at: Instant) -> u32;
}

/// The instant of a request, as the host reads it from its clock once at the request's start, and nothing else: the
/// day of the request is `Host::day` of it. The rows that a handler builds itself take this instant and its day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub at: Instant,
}
