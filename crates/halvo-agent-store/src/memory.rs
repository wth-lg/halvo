use std::collections::BTreeMap;

use halvo_schema::executable::names::AgentState;
use halvo_schema::kinds::{check_record, registry};
use halvo_schema::{
    AgentId, ErrorName, Origin, RowRead, Summary, SummaryRead, TypedRecord, Workflow, WorkflowId,
    WorkflowItem, WorkflowState, WriterStep,
};

use crate::host::Host;

/// The eight kinds of the family `agent` with their origin (A7.3), each with the workflow id under the key
/// `workflow`. The memory store takes rows of these kinds alone. Where the registry holds the entry of a kind, the
/// store checks a record against it with `check_record` and takes the kind's origin from it; the six kinds of
/// origin own have no entry until U3.3 adds them, and until then this table gives their origin.
pub const AGENT_KINDS: &[(&str, Origin)] = &[
    ("agent_config", Origin::Capture),
    ("agent_run", Origin::Capture),
    ("agent_playbook", Origin::Own),
    ("agent_bindings", Origin::Own),
    ("agent_effect", Origin::Own),
    ("agent_log", Origin::Own),
    ("agent_session", Origin::Own),
    ("agent_checkpoint", Origin::Own),
];

/// Why the typed commit of the memory store refused a whole list (W-C25.3): the first record that breaks a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitRefused {
    /// The kind is not one of `AGENT_KINDS`.
    Kind,
    /// The record fails `check_record` against the kind's entry, or it is no JSON object whose key `workflow`
    /// holds a workflow id.
    Record,
    /// The record names a workflow that is not stored.
    Workflow,
    /// A step returned a record of a kind of origin own, which only a handler of a request stores.
    OwnFromStep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Caller {
    Handler,
    Step,
}

struct NoSummaries;

impl SummaryRead for NoSummaries {
    fn for_each(&self, _visit: &mut dyn FnMut(&Summary)) {}
}

/// The store in memory behind the handler calls (A7.6's `memory-store`): the workflows that a test host puts, the
/// rows of the family `agent`, and the host's steps. Its typed commit is the commit of a handler; `after_batch`
/// runs the steps as the derived store does after a committed batch, and refuses a record of origin own that a step
/// returns. It removes no row.
#[derive(Default)]
pub struct MemoryStore {
    workflows: BTreeMap<WorkflowId, Workflow>,
    rows: BTreeMap<&'static str, Vec<String>>,
    steps: Vec<Box<dyn WriterStep>>,
}

impl MemoryStore {
    /// A store with no workflow, no row and no step.
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }

    /// The store with one more step after the steps that it has.
    pub fn with_step(mut self, step: Box<dyn WriterStep>) -> MemoryStore {
        self.steps.push(step);
        self
    }

    /// Stores a workflow as the recurrence step and the person's decisions leave it, or replaces the stored
    /// workflow of its id.
    pub fn put_workflow(&mut self, workflow: Workflow) {
        self.workflows.insert(workflow.id().clone(), workflow);
    }

    /// Calls each step in order, after a batch of the analysis writer, with no summaries and the rows as they
    /// stand, and commits what it returns in one transaction per step; stops at the first list that it refuses.
    pub fn after_batch(&mut self) -> Result<(), CommitRefused> {
        for i in 0..self.steps.len() {
            let records = self.steps[i].after_commit(&NoSummaries, &*self);
            self.commit_from(Caller::Step, records)?;
        }
        Ok(())
    }

    /// The typed commit of a handler: every record or none, with the first refusal.
    pub fn typed_commit(&mut self, records: Vec<TypedRecord>) -> Result<(), CommitRefused> {
        self.commit_from(Caller::Handler, records)
    }

    fn commit_from(
        &mut self,
        caller: Caller,
        records: Vec<TypedRecord>,
    ) -> Result<(), CommitRefused> {
        let mut checked = Vec::with_capacity(records.len());
        for record in records {
            let kind = self.check(caller, &record)?;
            checked.push((kind, record.json));
        }
        for (kind, json) in checked {
            self.rows.entry(kind).or_default().push(json);
        }
        Ok(())
    }

    fn check(&self, caller: Caller, record: &TypedRecord) -> Result<&'static str, CommitRefused> {
        let &(kind, table_origin) = AGENT_KINDS
            .iter()
            .find(|(name, _)| *name == record.kind)
            .ok_or(CommitRefused::Kind)?;
        let entry = registry().find(|e| e.name == kind);
        let origin = entry.map_or(table_origin, |e| e.origin);
        if caller == Caller::Step && origin == Origin::Own {
            return Err(CommitRefused::OwnFromStep);
        }
        if let Some(entry) = entry {
            check_record(entry, &record.json).map_err(|_| CommitRefused::Record)?;
        }
        let workflow = workflow_of(&record.json).ok_or(CommitRefused::Record)?;
        if !self.workflows.contains_key(&workflow) {
            return Err(CommitRefused::Workflow);
        }
        Ok(kind)
    }
}

fn workflow_of(json: &str) -> Option<WorkflowId> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    WorkflowId::parse(value.as_object()?.get("workflow")?.as_str()?).ok()
}

impl RowRead for MemoryStore {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        let Some(rows) = self.rows.get(kind) else {
            return;
        };
        let mut sorted: Vec<&String> = rows.iter().collect();
        sorted.sort();
        for row in sorted {
            visit(row);
        }
    }
}

impl Host for MemoryStore {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.typed_commit(records).map_err(|_| ErrorName::Store)
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
        self
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.workflows.get(id).map(|w| WorkflowItem {
            workflow: w.clone(),
            summaries: Vec::new(),
            hint: None,
        })
    }
}
