use halvo_schema_core::ids::{RequestId, SummaryId, WorkflowId};
use halvo_schema_core::record::{Evidence, EvidencePage, Workflow, WorkflowItem, WorkflowsPage};
use halvo_schema_core::request::{
    self, ErrorName, InterfaceRead, InterfaceRole, Read, ReplyResult, REPLY_MAX,
};
use halvo_schema_core::site::SiteValue;
use serde::Serialize;

use crate::db::{one, reading, rows, Db, Now};
use crate::error::{Error, Fault};
use crate::handler::HandlerCx;
use crate::sql;
use crate::Store;

/// The length of a reply that holds `page`, with the request id `id`, as the reply writer writes it.
fn reply_len(id: Option<&RequestId>, page: &ReplyResult) -> Result<usize, Error> {
    request::write_reply(id, Ok(page))
        .map(|bytes| bytes.len())
        .map_err(|_| Error::Refused(ErrorName::Internal))
}

fn json_len<T: Serialize>(value: &T) -> Result<usize, Fault> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|_| Fault::Row)
}

/// Builds a page from `candidates` in order: whole items, while the reply that holds the page, its envelope with
/// `id` included, stays inside 261,120 bytes; `next` is the key of the last item when more remain. An item that
/// passes the bound alone is refused with `too_large`. `empty` is the length of the reply with no item and a null
/// `next`; each further item adds its own length and a comma, and a `next` that is an id adds 30 bytes to `null`.
fn fill<K, T: Serialize>(
    candidates: &[K],
    empty: usize,
    mut item: impl FnMut(&K) -> Result<Option<T>, Fault>,
) -> Result<(Vec<T>, bool), Error> {
    let mut items = Vec::new();
    let mut length = empty;
    for (at, key) in candidates.iter().enumerate() {
        let Some(value) = item(key)? else {
            continue;
        };
        let added = json_len(&value)? + usize::from(!items.is_empty());
        let more = at + 1 < candidates.len();
        let total = length + added + if more { 30 } else { 0 };
        if total > REPLY_MAX {
            if items.is_empty() {
                return Err(Error::Refused(ErrorName::TooLarge));
            }
            return Ok((items, true));
        }
        length += added;
        items.push(value);
    }
    Ok((items, false))
}

fn workflow_item(db: &Db, now: Now, id: &WorkflowId) -> Result<Option<WorkflowItem>, Fault> {
    let Some(body) = one(&db.conn, sql::WORKFLOW_BODY, [id.as_str()], |r| {
        r.get::<_, String>(0)
    })?
    else {
        return Ok(None);
    };
    let workflow: Workflow = serde_json::from_str(&body).map_err(|_| Fault::Row)?;
    let summaries: Vec<SummaryId> = rows(
        &db.conn,
        sql::SUPPORT_HANDED_OUT,
        reading!(now, ":workflow" => id.as_str()),
        |r| r.get::<_, String>(0),
    )?
    .iter()
    .map(|s| SummaryId::parse(s).map_err(|_| Fault::Row))
    .collect::<Result<_, _>>()?;
    let titled = match &workflow {
        Workflow::Full(full) => matches!(full.site, Some(SiteValue::Title { .. })),
        Workflow::Dismissed(_) => false,
    };
    let mut hint = None;
    if titled {
        let mut latest = None;
        for summary in &summaries {
            if let Some(evidence) = evidence_of(db, now, summary)? {
                if evidence.hint.is_some() && latest.is_none_or(|end| evidence.end >= end) {
                    latest = Some(evidence.end);
                    hint = evidence.hint;
                }
            }
        }
    }
    Ok(Some(WorkflowItem {
        workflow,
        summaries,
        hint,
    }))
}

fn evidence_of(db: &Db, now: Now, summary: &SummaryId) -> Result<Option<Evidence>, Fault> {
    let body = one(
        &db.conn,
        sql::EVIDENCE_OF,
        reading!(now, ":summary" => summary.as_str()),
        |r| r.get::<_, String>(0),
    )?;
    body.map(|b| serde_json::from_str(&b).map_err(|_| Fault::Row))
        .transpose()
}

fn workflows_page(
    db: &Db,
    now: Now,
    id: Option<&RequestId>,
    after: Option<&WorkflowId>,
) -> Result<WorkflowsPage, Error> {
    let after = after.map_or("", |a| a.as_str());
    let candidates: Vec<WorkflowId> = rows(&db.conn, sql::WORKFLOWS_AFTER, [after], |r| {
        r.get::<_, String>(0)
    })?
    .iter()
    .map(|w| WorkflowId::parse(w).map_err(|_| Fault::Row))
    .collect::<Result<_, _>>()?;
    let empty = reply_len(
        id,
        &ReplyResult::Workflows(WorkflowsPage {
            workflows: Vec::new(),
            next: None,
        }),
    )?;
    let (workflows, more) = fill(&candidates, empty, |w| workflow_item(db, now, w))?;
    let next = if more {
        workflows.last().map(|item| item.workflow.id().clone())
    } else {
        None
    };
    Ok(WorkflowsPage { workflows, next })
}

fn evidence_page(
    db: &Db,
    now: Now,
    id: Option<&RequestId>,
    workflow: &WorkflowId,
    after: Option<&SummaryId>,
) -> Result<EvidencePage, Error> {
    if one(&db.conn, sql::WORKFLOW_ROW, [workflow.as_str()], |r| {
        r.get::<_, i64>(0)
    })?
    .is_none()
    {
        return Err(Error::Refused(ErrorName::NoWorkflow));
    }
    let after = after.map_or("", |a| a.as_str());
    let candidates: Vec<SummaryId> = rows(
        &db.conn,
        sql::SUPPORT_WITH_EVIDENCE,
        reading!(now, ":workflow" => workflow.as_str(), ":after" => after),
        |r| r.get::<_, String>(0),
    )?
    .iter()
    .map(|s| SummaryId::parse(s).map_err(|_| Fault::Row))
    .collect::<Result<_, _>>()?;
    let empty = reply_len(
        id,
        &ReplyResult::Evidence(EvidencePage {
            evidence: Vec::new(),
            next: None,
        }),
    )?;
    let (evidence, more) = fill(&candidates, empty, |s| evidence_of(db, now, s))?;
    let next = if more {
        evidence.last().map(|e| e.summary.clone())
    } else {
        None
    };
    Ok(EvidencePage { evidence, next })
}

/// A handle of the interface-read role on one store: it serves the interface read and nothing else.
pub struct ReadHandle<'s> {
    store: &'s Store,
    role: &'s InterfaceRole,
}

impl ReadHandle<'_> {
    /// The role of the handle, with which its holder builds the requests that it sends.
    pub fn role(&self) -> &InterfaceRole {
        self.role
    }

    /// Serves one interface read, as `Store::interface_read` does.
    pub fn read(&self, request: &InterfaceRead) -> Result<ReplyResult, Error> {
        self.store.interface_read(request)
    }
}

impl Store {
    /// Serves an interface read. `workflows` returns every stored workflow in any state after the workflow id
    /// `after`, in ascending order of workflow id, each with the ids of its support set in ascending order and its
    /// hint: the hint of the evidence row with the latest end in the support set that has one, where the
    /// workflow's site is a title hash, and else none. `evidence` returns the evidence rows of one stored workflow's
    /// support set after the summary id `after`, in ascending order of summary id, and answers `no_workflow` for a
    /// workflow that is not stored. A page holds whole items while the reply that holds it, its envelope with the
    /// request's id included, stays inside 261,120 bytes, and its `next` is the key of its last item when more
    /// remain; an item that passes the bound alone is refused with `too_large`.
    ///
    /// Of the rows it hands out exactly those that an expiry at the clock would leave (DS-4): an item's `summaries`
    /// are the ids of its support rows that are handed out, its hint is that of the latest evidence row handed out
    /// inside its window, and an evidence row or a support row is handed out only while its summary is. The read
    /// takes one reading of the host's clock at its start, so the statements of one page agree; it takes no day or
    /// instant from its caller, and it writes nothing.
    pub fn interface_read(&self, request: &InterfaceRead) -> Result<ReplyResult, Error> {
        self.db.arm();
        let now = self.db.now();
        let id = Some(request.id());
        let page = match request.read() {
            Read::Workflows { after } => {
                ReplyResult::Workflows(workflows_page(&self.db, now, id, after.as_ref())?)
            }
            Read::Evidence { workflow, after } => {
                ReplyResult::Evidence(evidence_page(&self.db, now, id, workflow, after.as_ref())?)
            }
        };
        if reply_len(id, &page)? > REPLY_MAX {
            return Err(Error::Refused(ErrorName::Internal));
        }
        Ok(page)
    }

    /// A handle of the interface-read role on this store.
    pub fn read_handle<'s>(&'s self, role: &'s InterfaceRole) -> ReadHandle<'s> {
        ReadHandle { store: self, role }
    }
}

impl HandlerCx<'_> {
    /// The item of one stored workflow as the read `workflows` gives it, or none where the workflow is not stored,
    /// by the one reading of the host's clock that `Store::handle` took at the start of this request.
    pub fn workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowItem>, Fault> {
        self.db.arm();
        workflow_item(self.db, self.now, id)
    }

    /// A page of the read `evidence` of one stored workflow, counted against the bound of a reply with the id of
    /// the request that the handler answers, by the one reading of the host's clock that `Store::handle` took at the
    /// start of this request.
    pub fn evidence(
        &self,
        id: &WorkflowId,
        after: Option<&SummaryId>,
    ) -> Result<EvidencePage, Error> {
        self.db.arm();
        evidence_page(self.db, self.now, Some(self.request.id()), id, after)
    }
}
