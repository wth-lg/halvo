use std::cell::Cell;
use std::collections::BTreeMap;

use halvo_schema_core::category::SeedCategory;
use halvo_schema_core::ids::{SummaryId, WorkflowId};
use halvo_schema_core::record::{
    FullWorkflow, Proposal, Summary, SummaryRead, Support, Workflow, WorkflowState,
};
use halvo_sqlcipher::rusqlite::Connection;

use crate::batch::{bound_days, key_of, this_run, ProposalOutcome};
use crate::db::{one, reading, rows, NewRow, Now, Tx};
use crate::error::{db_fault, Fault};
use crate::host::IdSource;
use crate::sql;

/// The read of the bounded summaries over the store's rows, in ascending order of `day` and then of summary id: of
/// the stored summaries it hands out those that an expiry at `now` would leave (DS-4; its lower side is the stored
/// day of going, whatever the entry's days say now) and of these, by a sentence of its own, none of a day after the
/// clock's day. `now` is the one reading of the host's clock that `commit_batch` took at its start, for the
/// recurrence step and every later step. A stored row that its reader refuses is visited not and kept as the fault of
/// the read.
pub(crate) struct SummaryReader<'c> {
    conn: &'c Connection,
    now: Now,
    fault: Cell<Option<Fault>>,
}

impl<'c> SummaryReader<'c> {
    pub(crate) fn new(conn: &'c Connection, now: Now) -> SummaryReader<'c> {
        SummaryReader {
            conn,
            now,
            fault: Cell::new(None),
        }
    }

    pub(crate) fn fault(&self) -> Option<Fault> {
        self.fault.get()
    }

    fn read(&self, visit: &mut dyn FnMut(&Summary)) -> Result<(), Fault> {
        let mut statement = self
            .conn
            .prepare_cached(sql::SUMMARY_READ.text())
            .map_err(db_fault)?;
        let mut found = statement
            .query(reading!(self.now, ":last" => self.now.today))
            .map_err(db_fault)?;
        while let Some(row) = found.next().map_err(db_fault)? {
            let body: String = row.get(0).map_err(db_fault)?;
            let summary: Summary = serde_json::from_str(&body).map_err(|_| Fault::Row)?;
            visit(&summary);
        }
        Ok(())
    }
}

impl SummaryRead for SummaryReader<'_> {
    fn for_each(&self, visit: &mut dyn FnMut(&Summary)) {
        if let Err(fault) = self.read(visit) {
            self.fault.set(Some(fault));
        }
    }
}

struct Checked {
    proposal: Proposal,
    summaries: BTreeMap<SummaryId, u32>,
    key: String,
}

fn check(tx: &Tx<'_>, now: Now, proposal: Proposal) -> Result<Option<Checked>, Fault> {
    let days = bound_days(this_run("summary")?).ok_or(Fault::Row)?;
    let probe = FullWorkflow {
        id: WorkflowId::from_random([0; 16]),
        state: WorkflowState::Proposed,
        category: proposal.category,
        site: proposal.site.clone(),
        app: proposal.app.clone(),
        trigger: proposal.trigger,
        recurrence: proposal.recurrence,
        confidence: proposal.confidence,
        agent: None,
    };
    if serde_json::to_string(&probe).is_err() || proposal.summaries.is_empty() {
        return Ok(None);
    }
    let key = key_of(proposal.category, &proposal.site, &proposal.app);
    let mut summaries = BTreeMap::new();
    for id in &proposal.summaries {
        let stored = one(tx.conn, sql::SUMMARY_DAY_KEY, [id.as_str()], |r| {
            Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?))
        })?;
        match stored {
            Some((day, summary_key)) if now.keeps_day(days, day) && summary_key == key => {
                summaries.insert(id.clone(), day);
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(Checked {
        proposal,
        summaries,
        key,
    }))
}

fn mint(tx: &Tx<'_>, ids: &mut dyn IdSource) -> Result<WorkflowId, Fault> {
    loop {
        let mut bytes = [0u8; 16];
        ids.fill(&mut bytes);
        let id = WorkflowId::from_random(bytes);
        if one(tx.conn, sql::WORKFLOW_ROW, [id.as_str()], |r| {
            r.get::<_, i64>(0)
        })?
        .is_none()
        {
            return Ok(id);
        }
    }
}

/// The stored workflow of a key: its row id and its record.
pub(crate) fn workflow_of_key(tx: &Tx<'_>, key: &str) -> Result<Option<(i64, Workflow)>, Fault> {
    let found = one(tx.conn, sql::WORKFLOW_BY_KEY, [key], |r| {
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

/// Sets the support rows of a workflow to one row per summary of `summaries`, with the summary's day.
pub(crate) fn set_support(
    tx: &Tx<'_>,
    workflow: &WorkflowId,
    summaries: &BTreeMap<SummaryId, u32>,
) -> Result<bool, Fault> {
    let support = this_run("support")?;
    let stored: Vec<(i64, String)> = rows(tx.conn, sql::SUPPORT_OF, [workflow.as_str()], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    let mut changed = false;
    for (row, summary) in &stored {
        if !summaries.keys().any(|id| id.as_str() == summary) {
            tx.exec(sql::ROW_DELETE, [row])?;
            tx.changed(support.family);
            changed = true;
        }
    }
    for (id, day) in summaries {
        if stored.iter().any(|(_, summary)| summary == id.as_str()) {
            continue;
        }
        let record = Support {
            workflow: workflow.clone(),
            summary: id.clone(),
            day: *day,
        };
        let body = serde_json::to_string(&record).map_err(|_| Fault::Row)?;
        tx.insert(&NewRow {
            entry: support,
            day: Some(*day),
            expires: None,
            workflow: Some(workflow.as_str()),
            summary: Some(id.as_str()),
            wkey: None,
            body: &body,
            segments: &[],
        })?;
        changed = true;
    }
    Ok(changed)
}

/// Applies the proposals of one call of the recurrence step by the table of the workflow record: a proposal whose key
/// no stored workflow holds is stored as a new workflow in the state `proposed`; one whose key a dismissed workflow
/// holds is dropped; one whose key a workflow in another state holds changes that row, which keeps its id, its state
/// and its agent and takes the proposal's app, trigger, recurrence and confidence, and its support rows become those
/// of the proposal's summaries. A proposal that names a summary stored under another key than its own is refused, so
/// every summary that supports a workflow, or once did, holds the workflow's key. A step returns at most one proposal
/// for a key in one call: where it returns more, the first that passes its checks is the call's proposal for that key,
/// and each later one with that key is refused whole and counted in `refused`, with no workflow row, support row or
/// revision of it; a proposal that another cause refuses holds no key. The five counts sum to the proposals. The ids
/// of the new workflows are drawn from `ids` in the order of their category and then of their lowest supporting
/// summary id.
pub(crate) fn apply(
    tx: &Tx<'_>,
    now: Now,
    proposals: Vec<Proposal>,
    ids: &mut dyn IdSource,
) -> Result<ProposalOutcome, Fault> {
    let workflow_entry = this_run("workflow")?;
    let mut out = ProposalOutcome::default();
    let mut by_key: Vec<Checked> = Vec::new();
    for proposal in proposals {
        match check(tx, now, proposal)? {
            None => out.refused += 1,
            Some(checked) if by_key.iter().any(|earlier| earlier.key == checked.key) => {
                out.refused += 1;
            }
            Some(checked) => by_key.push(checked),
        }
    }
    let mut new: Vec<Checked> = Vec::new();
    for checked in by_key {
        match workflow_of_key(tx, &checked.key)? {
            None => new.push(checked),
            Some((_, Workflow::Dismissed(_))) => out.dismissed += 1,
            Some((row, Workflow::Full(stored))) => {
                let updated = FullWorkflow {
                    app: checked.proposal.app.clone(),
                    trigger: checked.proposal.trigger,
                    recurrence: checked.proposal.recurrence,
                    confidence: checked.proposal.confidence,
                    ..stored.clone()
                };
                let mut changed = false;
                if updated != stored {
                    let body =
                        serde_json::to_string(&Workflow::Full(updated)).map_err(|_| Fault::Row)?;
                    tx.exec(sql::ROW_BODY_SET, (row, body))?;
                    tx.changed(workflow_entry.family);
                    changed = true;
                }
                changed |= set_support(tx, &stored.id, &checked.summaries)?;
                if changed {
                    out.updated += 1;
                } else {
                    out.unchanged += 1;
                }
            }
        }
    }
    let order = |c: &Checked| -> (SeedCategory, Option<SummaryId>) {
        (c.proposal.category, c.summaries.keys().next().cloned())
    };
    new.sort_by_key(order);
    for checked in new {
        let id = mint(tx, ids)?;
        let workflow = Workflow::Full(FullWorkflow {
            id: id.clone(),
            state: WorkflowState::Proposed,
            category: checked.proposal.category,
            site: checked.proposal.site.clone(),
            app: checked.proposal.app.clone(),
            trigger: checked.proposal.trigger,
            recurrence: checked.proposal.recurrence,
            confidence: checked.proposal.confidence,
            agent: None,
        });
        let body = serde_json::to_string(&workflow).map_err(|_| Fault::Row)?;
        tx.insert(&NewRow {
            entry: workflow_entry,
            day: None,
            expires: None,
            workflow: Some(id.as_str()),
            summary: None,
            wkey: Some(&checked.key),
            body: &body,
            segments: &[],
        })?;
        set_support(tx, &id, &checked.summaries)?;
        out.stored += 1;
    }
    Ok(out)
}
