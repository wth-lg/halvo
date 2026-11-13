use std::collections::BTreeMap;

use halvo_agent_store::{Host, AGENT_KINDS};
use halvo_schema::executable::names::AgentState;
use halvo_schema::kinds::{check_record, registry};
use halvo_schema::{
    AgentId, ErrorName, KindEntry, RetentionClass, RowRead, TypedRecord, Workflow, WorkflowId,
    WorkflowItem, WorkflowState,
};

/// A record family that the memory store keeps: `agent` (the agent store's eight kinds) or `learn` (the
/// learning run's three kinds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Agent,
    Learn,
}

impl Family {
    fn holds(self, kind: &str) -> Option<&'static str> {
        match self {
            Family::Agent => AGENT_KINDS.iter().map(|(k, _)| *k).find(|k| *k == kind),
            Family::Learn => halvo_schema::learning::KINDS
                .iter()
                .map(|e| e.name)
                .find(|k| *k == kind),
        }
    }
}

/// The rows of one family, each kind's rows as serialized records. A read visits a kind's rows in ascending
/// byte order and nothing for a kind of another family (W-C2.11).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rows {
    by_kind: BTreeMap<&'static str, Vec<String>>,
}

impl Rows {
    /// Every row of the family, as (kind, record).
    pub fn all(&self) -> impl Iterator<Item = (&'static str, &str)> {
        self.by_kind
            .iter()
            .flat_map(|(k, rows)| rows.iter().map(move |r| (*k, r.as_str())))
    }

    pub fn count(&self, kind: &str) -> usize {
        self.by_kind.get(kind).map_or(0, Vec::len)
    }
}

impl RowRead for Rows {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        let Some(rows) = self.by_kind.get(kind) else {
            return;
        };
        let mut sorted: Vec<&String> = rows.iter().collect();
        sorted.sort();
        for row in sorted {
            visit(row);
        }
    }
}

fn entry(kind: &str) -> Option<&'static KindEntry> {
    registry().find(|e| e.name == kind)
}

fn key_of<'a>(json: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    json.as_object()?.get(key)
}

/// The workflow that a record names under its kind's `workflow_key`, `workflow` where the registry holds no
/// entry of the kind (the agent store's table says so of its eight kinds).
fn workflow_of(kind: &str, json: &str) -> Option<WorkflowId> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let key = entry(kind).map_or(Some("workflow"), |e| e.workflow_key)?;
    WorkflowId::parse(key_of(&value, key)?.as_str()?).ok()
}

/// Whether a row of a kind stays at an open on day `today`: a row of a bounded kind with day `d` stays while
/// `today - d` is from 0 to the bound less 1, or while `d` is at most two days after `today`; a bounded row with no
/// day goes; a row of any other class stays (the workflow run's bound rule, W-C2.5 and its rulings DS-2, DS-4).
pub fn stays(kind: &str, json: &str, today: u32) -> bool {
    let Some(RetentionClass::Bounded { days }) = entry(kind).map(|e| e.class) else {
        return true;
    };
    let day = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| key_of(&v, "day").and_then(serde_json::Value::as_u64));
    match day {
        None => false,
        Some(d) if d > u64::from(today) => d - u64::from(today) <= 2,
        Some(d) => u64::from(today) - d < u64::from(days),
    }
}

/// The store in memory of the gate host (contracts.md L10.1): the workflows, the rows of the family `agent` that
/// the agent store's handler reads and writes, and the rows of the family `learn` that `LearnHandlers` reads and
/// writes, each family behind its own calls. It expires bounded rows by `day` at an open and deletes a workflow
/// with every row that names it.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    workflows: BTreeMap<WorkflowId, Workflow>,
    agent: Rows,
    learn: Rows,
}

/// What an open removed, by kind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expired {
    pub by_kind: BTreeMap<&'static str, usize>,
}

impl Expired {
    pub fn of(&self, kind: &str) -> usize {
        self.by_kind.get(kind).copied().unwrap_or(0)
    }
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    /// Stores a workflow, or replaces the stored workflow of its id.
    pub fn put_workflow(&mut self, workflow: Workflow) {
        self.workflows.insert(workflow.id().clone(), workflow);
    }

    pub fn rows(&self, family: Family) -> &Rows {
        match family {
            Family::Agent => &self.agent,
            Family::Learn => &self.learn,
        }
    }

    /// The typed commit of one family (W-C25.3): every record or none. A record is refused when its kind is not
    /// of the family, when it fails `check_record` against the registry's entry of its kind, or when it names a
    /// workflow that is not stored.
    pub fn commit(&mut self, family: Family, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        let mut checked = Vec::with_capacity(records.len());
        for record in records {
            let kind = family.holds(record.kind).ok_or(ErrorName::Store)?;
            if let Some(entry) = entry(kind) {
                check_record(entry, &record.json).map_err(|_| ErrorName::Store)?;
            }
            let workflow = workflow_of(kind, &record.json).ok_or(ErrorName::Store)?;
            if !self.workflows.contains_key(&workflow) {
                return Err(ErrorName::Store);
            }
            checked.push((kind, record.json));
        }
        let rows = match family {
            Family::Agent => &mut self.agent,
            Family::Learn => &mut self.learn,
        };
        for (kind, json) in checked {
            rows.by_kind.entry(kind).or_default().push(json);
        }
        Ok(())
    }

    /// Opens the store on day `today`: removes every row that does not stay by `stays`.
    pub fn open(&mut self, today: u32) -> Expired {
        let mut expired = Expired::default();
        for rows in [&mut self.agent, &mut self.learn] {
            for (kind, list) in rows.by_kind.iter_mut() {
                let before = list.len();
                list.retain(|json| stays(kind, json, today));
                if list.len() < before {
                    *expired.by_kind.entry(kind).or_default() += before - list.len();
                }
            }
        }
        expired
    }

    /// Deletes a workflow and every row of both families that names it under its kind's workflow key; answers
    /// whether the workflow was stored.
    pub fn delete_workflow(&mut self, id: &WorkflowId) -> bool {
        let stored = self.workflows.remove(id).is_some();
        for rows in [&mut self.agent, &mut self.learn] {
            for (kind, list) in rows.by_kind.iter_mut() {
                list.retain(|json| workflow_of(kind, json).as_ref() != Some(id));
            }
        }
        stored
    }

    /// The rows of the family `learn` by kind, for a test that stands in for a wrong cascade.
    #[cfg(test)]
    pub fn learn_rows_mut(&mut self) -> &mut BTreeMap<&'static str, Vec<String>> {
        &mut self.learn.by_kind
    }

    pub fn workflow_item(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.workflows.get(id).map(|w| WorkflowItem {
            workflow: w.clone(),
            summaries: Vec::new(),
            hint: None,
        })
    }
}

/// The calls that the agent store's handler gets (W-C25.5): its typed commit and rows of the family `agent`, the
/// state call, and the read of one stored workflow.
impl Host for MemoryStore {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        MemoryStore::commit(self, Family::Agent, records)
    }

    fn set_state(
        &mut self,
        workflow: &WorkflowId,
        state: AgentState,
        agent: Option<&AgentId>,
    ) -> Result<(), ErrorName> {
        let stored = self
            .workflows
            .get_mut(workflow)
            .ok_or(ErrorName::NoWorkflow)?;
        let Workflow::Full(w) = stored else {
            return Err(ErrorName::State);
        };
        if w.state == WorkflowState::Proposed {
            return Err(ErrorName::State);
        }
        let state = match state {
            AgentState::Approved => WorkflowState::Approved,
            AgentState::Active => WorkflowState::Active,
            AgentState::Paused => WorkflowState::Paused,
        };
        if state != WorkflowState::Approved && agent.is_none() {
            return Err(ErrorName::State);
        }
        w.state = state;
        w.agent = agent.cloned();
        Ok(())
    }

    fn rows(&self) -> &dyn RowRead {
        &self.agent
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.workflow_item(id)
    }
}
