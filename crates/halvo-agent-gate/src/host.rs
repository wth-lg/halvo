use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_agent_store::{MemoryStore, Now};
use halvo_schema::executable::{
    AgentConfig, CoordinatorCall, CoordinatorReply, RunnerCall, RunnerReply,
};
use halvo_schema::{
    ErrorName, Instant, RowRead, SeedCategory, Workflow, WorkflowId, WorkflowState,
};

const NANOS_PER_DAY: i128 = 86_400 * 1_000_000_000;

/// One coordinator body that the gate host sent to the handler, by its `op`, with the handler's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    pub op: &'static str,
    pub body: CoordinatorCall,
    pub answer: Result<CoordinatorReply, ErrorName>,
}

/// The gate host on the memory store: the workflows that it puts, the rows that the handlers store, and every
/// coordinator body that it sent with its answer, in order.
pub struct GateHost {
    store: MemoryStore,
    sent: Vec<Sent>,
}

impl Default for GateHost {
    fn default() -> GateHost {
        GateHost::new()
    }
}

impl GateHost {
    /// A host with an empty store.
    pub fn new() -> GateHost {
        GateHost {
            store: MemoryStore::new(),
            sent: Vec::new(),
        }
    }

    /// Puts one workflow of the seed category as a proposal and the person's `approve` leave it: `approved`, with
    /// no agent, and a fresh id. Returns its id.
    pub fn put_seed_workflow(&mut self, category: SeedCategory) -> WorkflowId {
        let id = WorkflowId::from_random(minted());
        self.put_record(seed_workflow(&id, category));
        id
    }

    /// Stores a workflow record as the recurrence step and the person's decisions leave it, or replaces the stored
    /// workflow of its id.
    pub fn put_record(&mut self, workflow: Workflow) {
        self.store.put_workflow(workflow);
    }

    /// Sends one coordinator body to the handler, as the bytes of its compact JSON, and records it with the answer.
    pub fn send(&mut self, body: &CoordinatorCall) -> Result<CoordinatorReply, ErrorName> {
        let answer = match body.to_json() {
            Ok(json) => halvo_agent_store::coordinator(&mut self.store, now(), json.as_bytes()),
            Err(_) => Err(ErrorName::Malformed),
        };
        self.sent.push(Sent {
            op: body.op(),
            body: body.clone(),
            answer: answer.clone(),
        });
        answer
    }

    /// Relays one runner body to the handler, as the bytes of its compact JSON, and returns the answer.
    pub fn runner(&mut self, body: &RunnerCall) -> Result<RunnerReply, ErrorName> {
        let json = body.to_json().map_err(|_| ErrorName::Malformed)?;
        halvo_agent_store::runner(&mut self.store, now(), json.as_bytes())
    }

    /// The coordinator bodies sent so far, in order.
    pub fn sent(&self) -> &[Sent] {
        &self.sent
    }

    /// The stored workflow of the id, as the read `workflows` gives it.
    pub fn workflow(&self, id: &WorkflowId) -> Option<Workflow> {
        halvo_agent_store::Host::workflow(&self.store, id).map(|item| item.workflow)
    }

    /// The rows of the family `agent` as they stand.
    pub fn rows(&self) -> &dyn RowRead {
        &self.store
    }

    /// Every stored row of `agent_config` that the typed reader takes, in the store's order.
    pub fn configs(&self) -> Vec<AgentConfig> {
        let mut out = Vec::new();
        self.store.for_each("agent_config", &mut |row| {
            if let Ok(config) = AgentConfig::parse(row.as_bytes()) {
                out.push(config);
            }
        });
        out
    }
}

/// 16 bytes for an id of a body that the gate host sends first, as A0.6 has the sender mint it. The bytes come from
/// the keys of std's `RandomState`, which the system seeds per process.
pub(crate) fn minted() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    for half in bytes.chunks_mut(8) {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u8(0);
        half.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    bytes
}

fn now() -> Now {
    let nanos = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i128::try_from(d.as_nanos()).unwrap_or(i128::MAX),
        Err(e) => -i128::try_from(e.duration().as_nanos()).unwrap_or(i128::MAX),
    };
    let at = i64::try_from(nanos).unwrap_or(if nanos < 0 { i64::MIN } else { i64::MAX });
    let day = u32::try_from(nanos.div_euclid(NANOS_PER_DAY)).unwrap_or(0);
    Now {
        at: Instant(at),
        day,
    }
}

fn seed_workflow(id: &WorkflowId, category: SeedCategory) -> Workflow {
    serde_json::from_value(serde_json::json!({
        "v": 1,
        "id": id.as_str(),
        "state": WorkflowState::Approved.as_str(),
        "category": category.as_str(),
        "site": null,
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": null,
    }))
    .expect("a seed workflow is a workflow record")
}
