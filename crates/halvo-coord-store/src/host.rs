use halvo_schema::{Instant, RowRead, TypedRecord, WorkflowId, WorkflowItem};

/// The clock that the host supplies to the store (W-C25.5): the machine's clock and time zone in the owner, a clock
/// that the host sets, with UTC as local time, in a replay and in the gate.
pub trait Clock {
    /// The instant now.
    fn now(&self) -> Instant;
    /// The local calendar day that holds the instant, counted in days from 1970-01-01 (W-C2.5).
    fn day(&self, at: Instant) -> u32;
}

/// Which day of a bounded record the typed commit of a handler refuses (W-C25.3, the derived store's ruling DS-2,
/// point E1). It holds no day and no other value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayFault {
    /// The day lies after the day of the clock.
    AfterToday,
    /// The day lies already outside the bound of its kind: the clock's day less the kind's days, or earlier.
    OutsideBound,
}

/// Why a typed commit refused its whole list: the first record that breaks a rule. Nothing of the list is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitRefused {
    /// The kind is no kind of the family `coord`.
    Kind,
    /// A step returned a record of a kind of origin own, which only the handler of a request stores.
    OwnFromStep,
    /// The record fails `check_record` against its kind's entry, or its typed reader refuses it.
    Record,
    /// The record names a workflow that is not stored.
    Workflow,
    /// The record is of a bounded kind and the store will not keep its day.
    Day(DayFault),
}

/// The four calls that a store offers the handler while it answers one request, and no other. The host calls the
/// handler to its end before it passes it the next request, so no other request changes the rows between the
/// handler's reads and its commit.
pub trait Host {
    /// The typed commit of a handler (W-C25.3): every record of the list, each a kind of the family `coord` and its
    /// compact JSON, in one transaction, or none of them with the refusal. It refuses a bounded record whose day
    /// lies after the day of the clock or already outside its bound.
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), CommitRefused>;

    /// The rows of the family `coord` as they stand at the call (W-C2.11).
    fn rows(&self) -> &dyn RowRead;

    /// The item of the read `workflows` for one stored workflow, or none where it is not stored (W-C2.10).
    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem>;

    /// The store's clock.
    fn clock(&self) -> &dyn Clock;
}
