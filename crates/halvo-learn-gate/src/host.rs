use halvo_agent_store::{Host, Now};
use halvo_learn_store::{Clock, LearnHost};
use halvo_schema::{ErrorName, Instant, RowRead, TypedRecord, WorkflowId, WorkflowItem};

use crate::store::{Family, MemoryStore};

const NANOS_PER_DAY: i64 = 86_400_000_000_000;

/// The gate host (contracts.md L10.1, L10.2): the memory store and the day of its clock, which the command sets
/// for each occurrence. It gives `LearnHandlers` the calls of the family `learn` and the agent store's handler
/// the calls of the family `agent`.
pub struct GateHost {
    pub store: MemoryStore,
    pub day: u32,
}

impl GateHost {
    pub fn new(store: MemoryStore, day: u32) -> GateHost {
        GateHost { store, day }
    }

    /// The instant and the day of a request: noon UTC of the clock's day.
    pub fn now(&self) -> Now {
        Now {
            at: Instant(i64::from(self.day) * NANOS_PER_DAY + NANOS_PER_DAY / 2),
            day: self.day,
        }
    }
}

impl Clock for GateHost {
    fn today(&self) -> u32 {
        self.day
    }
}

impl LearnHost for GateHost {
    fn agent_host(&mut self) -> &mut dyn Host {
        &mut self.store
    }

    fn agent_now(&self) -> Now {
        self.now()
    }

    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), ErrorName> {
        self.store.commit(Family::Learn, records)
    }

    fn rows(&self) -> &dyn RowRead {
        self.store.rows(Family::Learn)
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.store.workflow_item(id)
    }
}
