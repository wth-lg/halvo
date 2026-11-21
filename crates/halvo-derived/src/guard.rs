use std::cell::Cell;

use halvo_schema_core::ids::WorkflowId;
use halvo_schema_core::record::RowRead;
use halvo_schema_core::request::ErrorName;

use crate::db::{Db, Now};
use crate::error::Error;
use crate::state::{held_ahead, HeldRefusal};
use crate::typed::Rows;

/// What a delete guard answers to the delete of one workflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardAnswer {
    /// Nothing of the guard's family holds the workflow: the delete goes on to the next guard and then to its rows.
    Go,
    /// Something of the guard's family is still at work under the workflow: the store removes nothing and answers the
    /// decision with `state`.
    Wait,
}

/// The held-ahead call that the store hands a delete guard, by kind name: `held_ahead("a_kind")` answers yes or no,
/// whether at least one stored row of that kind of the guard's own family under the workflow id of the delete has a
/// day more than two days after the clock's day, by the one reading of the host's clock that the delete took. It is
/// the call of `HandlerCx::held_ahead` without the check of the role: under a delete it never answers
/// `HeldRefusal::Role`, and it answers a refusal of `HeldRefusal`, never no, for a kind that the store was not opened
/// with, a kind of the family `workflow`, a kind of another family and a kind whose entry gives no workflow key, in
/// that order. It reads no row's text and writes nothing.
pub type HeldAhead<'a> = dyn Fn(&str) -> Result<bool, HeldRefusal> + 'a;

/// A delete guard: the code of a run whose rows can show that an agent still works under a workflow. The store calls
/// it inside the transaction of the delete of a stored workflow, before the delete removes a row, with the workflow
/// id of the delete, a `RowRead` whose `for_each` visits the rows of one kind of the guard's own family that are held
/// under that workflow id and no other, under the rule of the reads (DS-4), and the held-ahead call of the guard's
/// own family under that workflow (`HeldAhead`). Both read by the one reading of the host's clock that the delete
/// took. A fault of the database in that read or in that call is the store's and not the guard's to answer: the store
/// answers the decision with the fault and removes nothing, whatever the guard answered.
pub type DeleteGuard = Box<dyn Fn(&WorkflowId, &dyn RowRead, &HeldAhead<'_>) -> GuardAnswer>;

pub(crate) struct GuardEntry {
    pub(crate) family: &'static str,
    pub(crate) guard: DeleteGuard,
}

impl crate::Host {
    /// Adds a delete guard behind the guards added before it. `family` is the guard's own family: it reads the rows
    /// of that family alone and asks the held-ahead call about its kinds alone. A delete calls the guards in the order
    /// in which they were added and stops at the first that answers `GuardAnswer::Wait`.
    pub fn delete_guard(mut self, family: &'static str, guard: DeleteGuard) -> crate::Host {
        self.guards.push(GuardEntry { family, guard });
        self
    }
}

/// The rows of one family that are held under one workflow id, as a `RowRead`: `for_each` is the keyed read of
/// `Rows::for_each_under` with that id.
struct RowsUnder<'a> {
    rows: Rows<'a>,
    workflow: &'a WorkflowId,
}

impl RowRead for RowsUnder<'_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        self.rows.for_each_under(kind, self.workflow, visit);
    }
}

/// The delete guards of the host with the store's database, from which one delete hands them their inputs.
pub(crate) struct Guards<'a> {
    pub(crate) db: &'a Db,
    pub(crate) entries: &'a [GuardEntry],
}

impl Guards<'_> {
    /// Calls each guard in the order given with the workflow id and its two inputs, on the connection that holds the
    /// delete's transaction open. It reads the host's clock once, before the first guard, for the inputs of every
    /// guard, and not at all where the host gave no guard. It answers the fault of the first guard whose read or whose
    /// held-ahead call met a fault of the database, whatever that guard answered, and else `state` at the first guard
    /// that answers `GuardAnswer::Wait`; a guard behind that one is not called.
    pub(crate) fn ask(&self, workflow: &WorkflowId) -> Result<(), Error> {
        if self.entries.is_empty() {
            return Ok(());
        }
        let now: Now = self.db.now();
        for entry in self.entries {
            let fault = Cell::new(None);
            let rows = RowsUnder {
                rows: Rows {
                    db: self.db,
                    family: entry.family,
                    fault: &fault,
                    now,
                },
                workflow,
            };
            let held = |kind: &str| {
                let answer = held_ahead(self.db, entry.family, now, kind, workflow);
                if let Err(HeldRefusal::Fault(met)) = answer {
                    fault.set(Some(met));
                }
                answer
            };
            let answer = (entry.guard)(workflow, &rows, &held);
            if let Some(fault) = fault.get() {
                return Err(Error::Fault(fault));
            }
            if answer == GuardAnswer::Wait {
                return Err(Error::Refused(ErrorName::State));
            }
        }
        Ok(())
    }
}
