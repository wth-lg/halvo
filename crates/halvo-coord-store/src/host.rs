use halvo_schema::{Instant, TypedRecord, WorkflowId, WorkflowItem};

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

/// Why the fifth call of `Host` gave neither yes nor no: the names of `HeldRefusal` of halvo-derived, which the store
/// gives and its stand-in gives alike (the workflow run's ruling S-59 (a) 3 and its request of 2026-11-13 17:42Z,
/// item 2). It holds no value. The handler answers `store` for each of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldRefused {
    /// The store serves a request that is no `coordinator_write`.
    Role,
    /// The kind is not among the entries that the store holds.
    UnknownKind,
    /// The kind belongs to the family `workflow`.
    WorkflowRun,
    /// The kind belongs to another family than `coord`.
    OtherFamily,
    /// The kind's entry gives no workflow key, so no row of it is held under a workflow id.
    NoWorkflowKey,
    /// A fault of the store, which the store does not keep for the request: a handler that dropped it would answer
    /// as if the call had said no.
    Fault,
}

/// The calls that a store offers the handler while it answers one request, and no other. The host calls the
/// handler to its end before it passes it the next request, so no other request changes the rows between the
/// handler's reads and its commit.
pub trait Host {
    /// The typed commit of a handler (W-C25.3): every record of the list, each a kind of the family `coord` and its
    /// compact JSON, in one transaction, or none of them with the refusal. It refuses a bounded record whose day
    /// lies after the day of the clock or already outside its bound.
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), CommitRefused>;

    /// The keyed read (W-C2.11, `Rows::for_each_under` of halvo-derived): visits each stored row of the kind `kind`
    /// of the family `coord` that the store holds under the workflow id `workflow`, the id that the commit took from
    /// the committed text under the kind's workflow key, and no row held under another id. It hands out exactly the
    /// rows under that id that an expiry at the store's clock would leave, and it parses no row, so a row that the
    /// typed reader refuses is handed out while it lies inside its bound. A fault of this read is the store's to
    /// answer: halvo-derived keeps it for the request and answers the request with it.
    fn rows_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str));

    /// The item of the read `workflows` for one stored workflow, or none where it is not stored (W-C2.10).
    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem>;

    /// The store's clock.
    fn clock(&self) -> &dyn Clock;

    /// The fifth call (contracts.md I7.2; W-C25.5, `HandlerCx::held_ahead` of halvo-derived): yes or no, whether at
    /// least one stored row of the kind `kind` of the family `coord` held under the workflow id `workflow` has a day
    /// more than two days after the clock's day. It hands out no row and no number, reads no row's text and writes
    /// nothing; a row past its day of going, a row one or two days ahead and a bounded row with no day make no yes.
    /// It answers `HeldRefused::Role`, before anything else, while the store serves a request that is no
    /// `coordinator_write`, and a refusal, never no, for a kind that the store does not hold, a kind of the family
    /// `workflow`, a kind of another family and a kind whose entry gives no workflow key, in that order.
    fn held_ahead(&self, kind: &str, workflow: &WorkflowId) -> Result<bool, HeldRefused>;
}
