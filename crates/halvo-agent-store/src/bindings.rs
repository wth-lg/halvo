use std::num::NonZeroU32;

use halvo_schema::executable::body::{AddOrigin, RevReply, SetBindings};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{AgentBindings, CoordinatorReply, WebOrigin};
use halvo_schema::{AgentId, ErrorName, TypedRecord, WorkflowId};

use crate::bind;
use crate::errors::{refused, Own};
use crate::host::Host;
use crate::rows;
use crate::workbook::{workbook_path, PathForm};

const WORKBOOK: &str = "workbook";

pub(crate) fn set_bindings(
    host: &mut dyn Host,
    b: &SetBindings,
) -> Result<CoordinatorReply, ErrorName> {
    bind::of_agent(&*host, &b.workflow, &b.agent)?;
    for slot in &b.slots {
        if slot.name.as_str() == WORKBOOK {
            workbook_path(&slot.value.joined(), PathForm::NATIVE).map_err(|_| Own::Binding)?;
        }
    }
    let (rev, origins, mut slots) = next(newest(&*host, &b.workflow, &b.agent)?)?;
    for slot in &b.slots {
        match slots.iter_mut().find(|s| s.name == slot.name) {
            Some(stored) => stored.value = slot.value.clone(),
            None => slots.push(slot.clone()),
        }
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
