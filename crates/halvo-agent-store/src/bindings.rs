use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_schema::executable::body::{
    AddOrigin, ListOverlaps, Overlap, OverlapsReply, RevReply, SetBindings, OVERLAPS_MAX,
};
use halvo_schema::executable::names::{AgentState, YesNo};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::{OwnText, V1};
use halvo_schema::executable::{
    reply_senders, same_address, AgentBindings, CoordinatorReply, Senders, WebOrigin,
};
use halvo_schema::{AgentId, ErrorName, TypedRecord, Workflow, WorkflowId, WorkflowState};

use crate::bind;
use crate::errors::{refused, Own};
use crate::host::Host;
use crate::rows;
use crate::workbook::{workbook_path, PathForm};

const WORKBOOK: &str = "workbook";
const ACCOUNT_ADDRESS: &str = "account_address";
const REPLY_SENDERS: &str = "reply_senders";
const OVERLAP: &str = "overlap";

fn overlap() -> ErrorName {
    ErrorName::read(OVERLAP).unwrap_or(ErrorName::Internal)
}

pub(crate) fn set_bindings(
    host: &mut dyn Host,
    b: &SetBindings,
) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(&*host, &b.workflow, &b.agent)?;
    for slot in &b.slots {
        if slot.name.as_str() == WORKBOOK {
            workbook_path(&slot.value.joined(), PathForm::NATIVE).map_err(|_| Own::SlotForm)?;
        }
    }
    let (rev, origins, slots) = next(newest(&*host, &b.workflow, &b.agent)?)?;
    let slots = merged(slots, &b.slots);
    if !overlaps(&*host, &b.agent, &slots)?.is_empty() {
        return Err(overlap());
    }
    store(host, &b.workflow, &b.agent, (rev, origins, slots))
}

pub(crate) fn add_origin(
    host: &mut dyn Host,
    b: &AddOrigin,
) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(&*host, &b.workflow, &b.agent)?;
    let newest = newest(&*host, &b.workflow, &b.agent)?;
    if let Some(n) = newest.as_ref().filter(|n| n.origins.contains(&b.origin)) {
        return Ok(CoordinatorReply::Rev(RevReply { rev: n.rev }));
    }
    let (rev, mut origins, slots) = next(newest)?;
    origins.push(b.origin.clone());
    store(host, &b.workflow, &b.agent, (rev, origins, slots))
}

pub(crate) fn state_call(
    host: &mut dyn Host,
    workflow: &WorkflowId,
    state: AgentState,
    agent: &AgentId,
) -> Result<(), ErrorName> {
    if state != AgentState::Paused {
        let slots = newest(&*host, workflow, agent)?.map_or_else(Vec::new, |n| n.slots);
        if !overlaps(&*host, agent, &slots)?.is_empty() {
            return Err(overlap());
        }
    }
    host.set_state(workflow, state, Some(agent))
}

pub(crate) fn list_overlaps(
    host: &dyn Host,
    b: &ListOverlaps,
) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(host, &b.workflow, &b.agent)?;
    let stored = newest(host, &b.workflow, &b.agent)?.map_or_else(Vec::new, |n| n.slots);
    let slots = merged(stored, b.slots.as_deref().unwrap_or_default());
    let mut overlaps = overlaps(host, &b.agent, &slots)?;
    let more = if overlaps.len() > OVERLAPS_MAX {
        YesNo::Yes
    } else {
        YesNo::No
    };
    overlaps.truncate(OVERLAPS_MAX);
    Ok(CoordinatorReply::Overlaps(OverlapsReply { overlaps, more }))
}

fn overlaps(host: &dyn Host, agent: &AgentId, slots: &[Slot]) -> Result<Vec<Overlap>, ErrorName> {
    let Some((account, senders)) = reply_list(slots) else {
        return Ok(Vec::new());
    };
    let mut others: BTreeMap<(WorkflowId, AgentId), AgentBindings> = BTreeMap::new();
    for row in rows::bindings(host.rows())? {
        if &row.agent == agent {
            continue;
        }
        let key = (row.workflow.clone(), row.agent.clone());
        if others.get(&key).is_none_or(|newer| newer.rev < row.rev) {
            others.insert(key, row);
        }
    }
    let mut found = Vec::new();
    for ((workflow, other), row) in others {
        if !live(host, &workflow, &other) {
            continue;
        }
        let Some((their_account, theirs)) = reply_list(&row.slots) else {
            continue;
        };
        if !same_address(&account, &their_account) {
            continue;
        }
        let shared = senders
            .iter()
            .filter(|sender| theirs.holds(sender))
            .map(|sender| OwnText::parse(sender).map_err(refused))
            .collect::<Result<Vec<OwnText>, ErrorName>>()?;
        if !shared.is_empty() {
            found.push(Overlap {
                workflow,
                agent: other,
                senders: shared,
            });
        }
    }
    Ok(found)
}

fn reply_list(slots: &[Slot]) -> Option<(String, Senders)> {
    let value = |name: &str| {
        slots
            .iter()
            .find(|slot| slot.name.as_str() == name)
            .map(|slot| slot.value.joined())
    };
    let senders = reply_senders(&value(REPLY_SENDERS)?).ok()?;
    Some((value(ACCOUNT_ADDRESS)?, senders))
}

fn live(host: &dyn Host, workflow: &WorkflowId, agent: &AgentId) -> bool {
    match host.workflow(workflow).map(|item| item.workflow) {
        Some(Workflow::Full(w)) => {
            w.agent.as_ref() == Some(agent)
                && matches!(w.state, WorkflowState::Approved | WorkflowState::Active)
        }
        _ => false,
    }
}

fn merged(mut slots: Vec<Slot>, body: &[Slot]) -> Vec<Slot> {
    for slot in body {
        match slots.iter_mut().find(|s| s.name == slot.name) {
            Some(stored) => stored.value = slot.value.clone(),
            None => slots.push(slot.clone()),
        }
    }
    slots
}

fn newest(
    host: &dyn Host,
    workflow: &WorkflowId,
    agent: &AgentId,
) -> Result<Option<AgentBindings>, ErrorName> {
    Ok(rows::bindings(host.rows())?
        .into_iter()
        .filter(|b| &b.workflow == workflow && &b.agent == agent)
        .max_by_key(|b| b.rev))
}

type Revision = (NonZeroU32, Vec<WebOrigin>, Vec<Slot>);

fn next(newest: Option<AgentBindings>) -> Result<Revision, ErrorName> {
    match newest {
        None => Ok((NonZeroU32::MIN, Vec::new(), Vec::new())),
        Some(n) => Ok((n.rev.checked_add(1).ok_or(Own::Length)?, n.origins, n.slots)),
    }
}

fn store(
    host: &mut dyn Host,
    workflow: &WorkflowId,
    agent: &AgentId,
    (rev, origins, slots): Revision,
) -> Result<CoordinatorReply, ErrorName> {
    let record = AgentBindings {
        v: V1,
        workflow: workflow.clone(),
        agent: agent.clone(),
        rev,
        origins,
        slots,
    };
    let json = record.to_json().map_err(refused)?;
    host.commit(vec![TypedRecord {
        kind: rows::AGENT_BINDINGS,
        json,
    }])?;
    Ok(CoordinatorReply::Rev(RevReply { rev }))
}
