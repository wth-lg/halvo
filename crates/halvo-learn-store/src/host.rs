use halvo_agent_store::{Host, Now};
use halvo_schema::{ErrorName, RowRead, TypedRecord, WorkflowId, WorkflowItem};

/// The day of the host (contracts.md L0.6): the product's host reads the local date, a test host returns the day
/// that its test set. No other code of the learning run reads a clock.
pub trait Clock {
    /// The local calendar day counted in days from 1970-01-01.
    fn today(&self) -> u32;
}

/// What `LearnHandlers` needs from its host while it answers one request (contracts.md L4.4), and no other call.
/// The calls of the family `learn` serve the handlers; the agent store's handler behind them gets the calls of
/// the family `agent` alone, through `agent_host`, so each handler keeps the calls of its own family. The host
/// calls the handlers to their end before it passes them the next request.
pub trait LearnHost: Clock {
    /// The calls that the agent store's handler gets (A7.5): its typed commit, its state call, its rows of the
    /// family `agent` and the read of one stored workflow.
    fn agent_host(&mut self) -> &mut dyn Host;

    /// The instant and the day that the agent store's handler takes for the request. `Clock::today` gives the day
    /// of the rows of the family `learn` and this day that of the agent store's rows, so the host keeps the two on
    /// one day.
    fn agent_now(&self) -> Now;

    /// The typed commit (W-C25.3) of rows of the family `learn`: every record of the list in one transaction, or
    /// none of them with the error of the refusal.
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName>;

    /// The rows of the family `learn` as they stand at the call (W-C2.11).
    fn rows(&self) -> &dyn RowRead;

    /// The item of the read `workflows` for one stored workflow, or none where it is not stored (W-C2.10).
    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem>;
}
