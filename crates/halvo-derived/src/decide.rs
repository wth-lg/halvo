use halvo_schema_core::ids::WorkflowId;
use halvo_schema_core::record::{DismissedWorkflow, Workflow, WorkflowState};
use halvo_schema_core::request::{Decision, ErrorName, InterfaceDecision};

use crate::batch::this_run;
use crate::db::{one, rows, Tx};
use crate::error::{Error, Fault};
use crate::sql;
use crate::Store;

/// The stored workflow of an id: its row id and its record.
pub(crate) fn workflow_by_id(
    tx: &Tx<'_>,
    id: &WorkflowId,
) -> Result<Option<(i64, Workflow)>, Fault> {
    let found = one(tx.conn, sql::WORKFLOW_BY_ID, [id.as_str()], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    match found {
        Some((row, body)) => Ok(Some((
            row,
            serde_json::from_str(&body).map_err(|_| Fault::Row)?,
        ))),
        None => Ok(None),
    }
}

pub(crate) fn set_body(tx: &Tx<'_>, row: i64, workflow: &Workflow) -> Result<(), Fault> {
    let body = serde_json::to_string(workflow).map_err(|_| Fault::Row)?;
    tx.exec(sql::ROW_BODY_SET, (row, body))?;
    tx.changed(this_run("workflow")?.family);
    Ok(())
}

fn approve(tx: &Tx<'_>, id: &WorkflowId) -> Result<(), Error> {
    match workflow_by_id(tx, id)? {
        None => Err(Error::Refused(ErrorName::NoWorkflow)),
        Some((row, Workflow::Full(mut full))) if full.state == WorkflowState::Proposed => {
            full.state = WorkflowState::Approved;
            Ok(set_body(tx, row, &Workflow::Full(full))?)
        }
        Some((_, Workflow::Full(full))) if full.state == WorkflowState::Approved => Ok(()),
        Some(_) => Err(Error::Refused(ErrorName::State)),
    }
}

fn dismiss(tx: &Tx<'_>, id: &WorkflowId) -> Result<(), Error> {
    match workflow_by_id(tx, id)? {
        None => Err(Error::Refused(ErrorName::NoWorkflow)),
        Some((row, Workflow::Full(full))) if full.state == WorkflowState::Proposed => {
            let dismissed = Workflow::Dismissed(DismissedWorkflow {
                id: full.id,
                category: full.category,
                site: full.site,
                app: full.app,
            });
            set_body(tx, row, &dismissed)?;
            tx.remove(sql::SUPPORT_REMOVE, [id.as_str()])?;
            Ok(())
        }
        Some((_, Workflow::Dismissed(_))) => Ok(()),
        Some(_) => Err(Error::Refused(ErrorName::State)),
    }
}

/// The delete cascade: every row of every kind that names the workflow under its workflow key, every stored
/// summary whose key is the workflow's key, and every row that names one of those summaries (their evidence and
/// their support rows). Every summary that supports a workflow, or once did, holds its key, because a proposal that
/// names a summary of another key is refused.
pub(crate) fn delete(tx: &Tx<'_>, id: &WorkflowId) -> Result<u64, Fault> {
    let Some(key) = one(tx.conn, sql::WORKFLOW_KEY, [id.as_str()], |r| {
        r.get::<_, String>(0)
    })?
    else {
        return Ok(0);
    };
    let mut removed = tx.remove(sql::REMOVE_NAMING, [id.as_str()])?;
    let summaries: Vec<String> = rows(tx.conn, sql::KEY_SUMMARIES, [&key], |r| r.get(0))?;
    for summary in &summaries {
        removed += tx.remove(sql::REMOVE_OF_SUMMARY, [summary])?;
    }
    Ok(removed)
}

impl Store {
    /// Applies an interface decision to the workflow that it names, in one transaction.
    ///
    /// `approve` moves a `proposed` workflow to `approved`. `dismiss` cuts a `proposed` workflow to its dismissed
    /// shape and removes its support rows. Each answers `no_workflow` for a workflow that is not stored, `state` for
    /// one that is neither `proposed` nor already in the state asked for, and changes nothing for a workflow that is
    /// in that state already. `delete` removes the workflow by the delete cascade: every row of every kind that names
    /// it under its workflow key, the kinds of later runs included, every stored summary whose key is its key, in
    /// its support set or not, the evidence of those summaries, and every support row that names one of them; that
    /// is all that ever supported it, because the store refuses a proposal that names a summary of another key; a
    /// workflow that is not stored changes nothing. Each raises the revision of every family that gained, changed or
    /// lost a row. The truncating checkpoint runs after a dismissal and after a deletion.
    pub fn decide(&mut self, decision: &InterfaceDecision) -> Result<(), Error> {
        let db = &self.db;
        db.arm();
        let id = decision.workflow();
        match decision.decision() {
            Decision::Approve => db.write(|tx| approve(tx, id)),
            Decision::Dismiss => db.write_checkpointed(|tx| dismiss(tx, id)),
            Decision::Delete => {
                db.write_checkpointed(|tx| delete(tx, id))?;
                Ok(())
            }
        }
    }
}
