use halvo_schema::coordinator::body::{CoordHistory, CoordLog, History, Stored};
use halvo_schema::coordinator::record::CoordEvent;
use halvo_schema::coordinator::Refusal;
use halvo_schema::{AgentId, ErrorName, RawRequest, Role, TypedRecord, Workflow, WorkflowId};

use crate::host::{CommitRefused, Host};

const KIND: &str = "coord_event";

/// The `op` names of the bodies that the handler takes.
pub const OPS: [&str; 2] = [CoordLog::OP, CoordHistory::OP];

/// The most records of one reply of `coord_history`, which the coordinator reads as the end of the records where a
/// reply holds fewer; it equals `HISTORY_RECORDS` of halvo-schema.
pub const HISTORY_PAGE: usize = 64;

/// The reply of a body that the handler took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoordReply {
    Stored(Stored),
    History(History),
}

impl CoordReply {
    /// The compact JSON of the reply.
    pub fn to_json(&self) -> Result<String, Refusal> {
        match self {
            CoordReply::Stored(s) => s.to_json(),
            CoordReply::History(h) => h.to_json(),
        }
    }
}

fn declared(name: &str) -> ErrorName {
    ErrorName::read(name).unwrap_or(ErrorName::Internal)
}

/// Answers one request of the owner whose body is a coordinator body of the interface run (contracts.md I7.2,
/// I7.3): the reply, or the error name of the first check that refuses it. A refused request stores nothing.
///
/// - Any request that is not a `coordinator_write` is `role`, whatever its body.
/// - A body that `CoordLog::parse` and `CoordHistory::parse` both refuse, as read from the bytes of the body as they
///   arrived, is `malformed`.
/// - Both bodies: `no_workflow` for a workflow that is not stored; `binding` unless the stored workflow's agent is
///   the body's agent.
/// - `coord_log`: `malformed` for a record whose `day` is not the local day of its `at` by the store's clock;
///   `seq` unless the records' `seq` are the stored last `seq` of the workflow and agent plus 1, 2 and on, the last
///   being 0 where none is stored. It commits, in one typed commit, the text that `CoordEvent::to_json` writes for
///   each record, never the bytes that arrived, and answers `malformed` where the commit refuses a day after the
///   clock's day or outside the bound of 93 days, or a record that `check_record` refuses, `no_workflow` where it
///   finds no stored workflow, and `store` for any other refusal. It answers the number of records stored.
/// - `coord_history`: the stored records of the workflow and agent whose `seq` is above `after`, or every one where
///   it is null, at most `HISTORY_PAGE` in ascending `seq`, gaps allowed, with `last` the largest stored `seq` of
///   the workflow and agent, or 0 where none is stored. It stores nothing.
/// - A stored row of the family that the typed reader refuses, read for either body, is `store`.
pub fn coordinator(host: &mut dyn Host, request: &RawRequest) -> Result<CoordReply, ErrorName> {
    if request.role() != Some(Role::Coordinator) {
        return Err(ErrorName::Role);
    }
    let body = request.body().get().as_bytes();
    if let Ok(log) = CoordLog::parse(body) {
        return coord_log(host, &log).map(CoordReply::Stored);
    }
    match CoordHistory::parse(body) {
        Ok(history) => coord_history(&*host, &history).map(CoordReply::History),
        Err(_) => Err(ErrorName::Malformed),
    }
}

fn bound(host: &dyn Host, workflow: &WorkflowId, agent: &AgentId) -> Result<(), ErrorName> {
    let item = host.workflow(workflow).ok_or(ErrorName::NoWorkflow)?;
    match item.workflow {
        Workflow::Full(w) if w.agent.as_ref() == Some(agent) => Ok(()),
        _ => Err(declared("binding")),
    }
}

fn stored_of(
    host: &dyn Host,
    workflow: &WorkflowId,
    agent: &AgentId,
) -> Result<Vec<CoordEvent>, ErrorName> {
    let mut out = Vec::new();
    let mut unread = false;
    host.rows()
        .for_each(KIND, &mut |row| match CoordEvent::parse(row.as_bytes()) {
            Ok(e) if &e.workflow == workflow && &e.agent == agent => out.push(e),
            Ok(_) => {}
            Err(_) => unread = true,
        });
    if unread {
        return Err(ErrorName::Store);
    }
    out.sort_by_key(|e| e.seq);
    Ok(out)
}

fn commit_error(refused: CommitRefused) -> ErrorName {
    match refused {
        CommitRefused::Day(_) | CommitRefused::Record => ErrorName::Malformed,
        CommitRefused::Workflow => ErrorName::NoWorkflow,
        CommitRefused::Kind | CommitRefused::OwnFromStep => ErrorName::Store,
    }
}

fn coord_log(host: &mut dyn Host, b: &CoordLog) -> Result<Stored, ErrorName> {
    bound(&*host, &b.workflow, &b.agent)?;
    let clock = host.clock();
    if b.records.iter().any(|r| clock.day(r.at) != r.day) {
        return Err(ErrorName::Malformed);
    }
    let stored = stored_of(&*host, &b.workflow, &b.agent)?;
    let mut last = stored.last().map_or(0, |e| e.seq.get());
    for r in &b.records {
        last = last.checked_add(1).ok_or_else(|| declared("seq"))?;
        if r.seq.get() != last {
            return Err(declared("seq"));
        }
    }
    let records = b
        .records
        .iter()
        .map(|r| {
            r.to_json()
                .map(|json| TypedRecord { kind: KIND, json })
                .map_err(|_| ErrorName::Malformed)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let count = u32::try_from(records.len()).map_err(|_| ErrorName::Malformed)?;
    host.commit(records).map_err(commit_error)?;
    Ok(Stored { stored: count })
}

fn coord_history(host: &dyn Host, b: &CoordHistory) -> Result<History, ErrorName> {
    bound(host, &b.workflow, &b.agent)?;
    let stored = stored_of(host, &b.workflow, &b.agent)?;
    let last = stored.last().map_or(0, |e| e.seq.get());
    let after = b.after.unwrap_or(0);
    let records = stored
        .into_iter()
        .filter(|e| e.seq.get() > after)
        .take(HISTORY_PAGE)
        .collect();
    let reply = History { records, last };
    reply.rules().map_err(|_| ErrorName::Store)?;
    Ok(reply)
}
