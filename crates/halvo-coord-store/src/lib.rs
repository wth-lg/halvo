#![forbid(unsafe_code)]
//! The store side of the interface run (contracts.md I7.3): the handler of the coordinator bodies `coord_log` and
//! `coord_history`, the guard of a workflow's delete for the family `coord`, the adapter that hands the handler to
//! the derived store's host, and the store in memory that stands in for the derived store in tests. The handler reads
//! a body with the typed parser of `halvo_schema::coordinator`, checks it against the stored rows of the family
//! `coord` under the body's workflow key and stores what the typed writer writes by the typed commit; it reads the
//! clock of its host and no clock, file or network of its own.

mod handler;
mod host;
mod memory;

pub use handler::{coordinator, delete_guard, CoordReply, DeleteGuard, HISTORY_PAGE, OPS};
pub use host::{Clock, CommitRefused, DayFault, HeldRefused, Host, StoreFault};
pub use memory::MemoryStore;

use halvo_derived::{Cause, Committed, HandlerCx, HeldRefusal, Refusal};
use halvo_schema::{ErrorName, Instant, RawRequest, TypedRecord, WorkflowId, WorkflowItem};
use serde_json::value::RawValue;

/// The family of the kind `coord_event`, which the handler's commits, reads and fifth call keep to.
pub const FAMILY: &str = "coord";

/// The handler of the bodies `coord_log` and `coord_history` for the host of the derived store (W-C25.5; the
/// review's F-2). U5.4's owner is to add it with `halvo_derived::Host::handler(FAMILY, Takes::Coordinator(&OPS),
/// ..)`; nothing adds it to a host at this head. For each request it answers with `coordinator` over the calls of `HandlerCx`, and writes the reply's compact JSON, or
/// `internal` where the reply cannot be written. A refusal of the typed commit answers by `CommitRefused`: an unknown
/// kind, a kind of the family `workflow` and a kind of another family are `Kind`, and a fault of the store and a
/// commit that stored fewer records than its list held are `Fault`, so each answers `store`; a fault of the read of a
/// workflow and every refusal of the fifth call, its fault included, answer `store` as well. A fault of the rows'
/// read is not the handler's: halvo-derived keeps it for the request and answers the request with it.
pub struct CoordHandler {
    clock: HostClock,
}

impl CoordHandler {
    /// The handler on the clock that the host gives the store. `HandlerCx` offers a handler no clock (W-C25.5: five
    /// calls and no other); the interface run's request WR3, a clock read once for a handled request and reached from
    /// `HandlerCx`, would replace this argument. The handler reads from it the local day of each record's `at` and
    /// nothing else.
    pub fn new(clock: Box<dyn halvo_derived::Clock>) -> CoordHandler {
        CoordHandler {
            clock: HostClock(clock),
        }
    }
}

impl halvo_derived::Handler for CoordHandler {
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName> {
        let mut host = Cx {
            cx,
            clock: &self.clock,
        };
        let reply = coordinator(&mut host, request)?;
        let json = reply.to_json().map_err(|_| ErrorName::Internal)?;
        RawValue::from_string(json).map_err(|_| ErrorName::Internal)
    }
}

struct HostClock(Box<dyn halvo_derived::Clock>);

impl Clock for HostClock {
    fn now(&self) -> Instant {
        self.0.now()
    }

    fn day(&self, at: Instant) -> u32 {
        self.0.day(at)
    }
}

struct Cx<'c, 'x> {
    cx: &'c mut HandlerCx<'x>,
    clock: &'c HostClock,
}

impl Host for Cx<'_, '_> {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), CommitRefused> {
        committed(records.len(), self.cx.commit(&records))
    }

    fn rows_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        self.cx.rows().for_each_under(kind, workflow, visit);
    }

    fn workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowItem>, StoreFault> {
        self.cx.workflow(id).map_err(|_| StoreFault)
    }

    fn clock(&self) -> &dyn Clock {
        self.clock
    }

    fn held_ahead(&self, kind: &str, workflow: &WorkflowId) -> Result<bool, HeldRefused> {
        self.cx.held_ahead(kind, workflow).map_err(held)
    }
}

fn committed(sent: usize, outcome: Result<Committed, Refusal>) -> Result<(), CommitRefused> {
    match outcome {
        Ok(c) if u64::try_from(sent) == Ok(c.stored) => Ok(()),
        Ok(_) => Err(CommitRefused::Fault),
        Err(refusal) => Err(refused(&refusal.cause)),
    }
}

fn refused(cause: &Cause) -> CommitRefused {
    match cause {
        Cause::UnknownKind | Cause::WorkflowRun | Cause::OtherFamily => CommitRefused::Kind,
        Cause::OwnOrigin => CommitRefused::OwnFromStep,
        Cause::Record(_) => CommitRefused::Record,
        Cause::NoWorkflow => CommitRefused::Workflow,
        Cause::Day(halvo_derived::DayFault::AfterToday) => CommitRefused::Day(DayFault::AfterToday),
        Cause::Day(halvo_derived::DayFault::OutsideBound) => {
            CommitRefused::Day(DayFault::OutsideBound)
        }
        Cause::Fault(_) => CommitRefused::Fault,
    }
}

fn held(refusal: HeldRefusal) -> HeldRefused {
    match refusal {
        HeldRefusal::Role => HeldRefused::Role,
        HeldRefusal::UnknownKind => HeldRefused::UnknownKind,
        HeldRefusal::OtherFamily => HeldRefused::OtherFamily,
        HeldRefusal::WorkflowRun => HeldRefused::WorkflowRun,
        HeldRefusal::NoWorkflowKey => HeldRefused::NoWorkflowKey,
        HeldRefusal::Fault(_) => HeldRefused::Fault,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::commit_error;
    use halvo_derived::Fault;

    #[test]
    fn each_cause_of_the_derived_commit_has_its_refusal_and_its_error_name() {
        let cases = [
            (Cause::UnknownKind, CommitRefused::Kind, ErrorName::Store),
            (Cause::WorkflowRun, CommitRefused::Kind, ErrorName::Store),
            (Cause::OtherFamily, CommitRefused::Kind, ErrorName::Store),
            (
                Cause::OwnOrigin,
                CommitRefused::OwnFromStep,
                ErrorName::Store,
            ),
            (
                Cause::Record(String::new()),
                CommitRefused::Record,
                ErrorName::Malformed,
            ),
            (
                Cause::NoWorkflow,
                CommitRefused::Workflow,
                ErrorName::NoWorkflow,
            ),
            (
                Cause::Day(halvo_derived::DayFault::AfterToday),
                CommitRefused::Day(DayFault::AfterToday),
                ErrorName::Malformed,
            ),
            (
                Cause::Day(halvo_derived::DayFault::OutsideBound),
                CommitRefused::Day(DayFault::OutsideBound),
                ErrorName::Malformed,
            ),
            (
                Cause::Fault(Fault::Row),
                CommitRefused::Fault,
                ErrorName::Store,
            ),
            (
                Cause::Fault(Fault::Db(5)),
                CommitRefused::Fault,
                ErrorName::Store,
            ),
        ];
        for (i, (cause, refusal, name)) in cases.into_iter().enumerate() {
            let answer = committed(1, Err(Refusal { index: 0, cause }));
            assert_eq!(answer, Err(refusal), "case {i}");
            assert_eq!(commit_error(refusal), name, "case {i}");
        }
    }

    #[test]
    fn a_commit_counts_as_done_when_it_stored_every_record_of_its_list() {
        let done = |stored, dropped| Ok(Committed { stored, dropped });
        assert_eq!(committed(2, done(2, 0)), Ok(()));
        assert_eq!(committed(0, done(0, 0)), Ok(()));
        assert_eq!(committed(2, done(1, 1)), Err(CommitRefused::Fault));
        assert_eq!(committed(2, done(3, 0)), Err(CommitRefused::Fault));
        assert_eq!(commit_error(CommitRefused::Fault), ErrorName::Store);
    }

    #[test]
    fn each_refusal_of_the_derived_fifth_call_has_its_name() {
        let cases = [
            (HeldRefusal::Role, HeldRefused::Role),
            (HeldRefusal::UnknownKind, HeldRefused::UnknownKind),
            (HeldRefusal::OtherFamily, HeldRefused::OtherFamily),
            (HeldRefusal::WorkflowRun, HeldRefused::WorkflowRun),
            (HeldRefusal::NoWorkflowKey, HeldRefused::NoWorkflowKey),
            (HeldRefusal::Fault(Fault::Row), HeldRefused::Fault),
            (HeldRefusal::Fault(Fault::Checkpoint), HeldRefused::Fault),
        ];
        for (i, (refusal, name)) in cases.into_iter().enumerate() {
            assert_eq!(held(refusal), name, "case {i}");
        }
    }

    #[test]
    fn the_host_clock_gives_the_day_and_the_instant_of_the_store_clock() {
        struct Fixed;
        impl halvo_derived::Clock for Fixed {
            fn now(&self) -> Instant {
                Instant(7)
            }
            fn day(&self, at: Instant) -> u32 {
                u32::try_from(at.0).unwrap_or(0) + 3
            }
            fn day_start(&self, _: u32) -> Instant {
                Instant(0)
            }
        }
        let handler = CoordHandler::new(Box::new(Fixed));
        assert_eq!(handler.clock.now(), Instant(7));
        assert_eq!(handler.clock.day(Instant(4)), 7);
    }
}
