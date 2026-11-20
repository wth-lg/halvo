use halvo_agent_store::{Host, Now};
use halvo_learn_store::{Clock, LearnHandlers, LearnHost};
use halvo_schema::executable::{CoordinatorReply, RunnerReply};
use halvo_schema::{ErrorName, Instant, RowRead, TypedRecord, WorkflowId, WorkflowItem};

use crate::store::{Family, MemoryStore};
use crate::stub::{workflow_record, Arm};

const NANOS_PER_DAY: i64 = 86_400_000_000_000;

/// The instant of a request on `day`: noon UTC of the day, or the last instant that an `i64` of nanoseconds holds
/// for a day past it (a case's day reaches 4,294,967,295, L9.2; the handlers read the day).
pub fn noon(day: u32) -> Instant {
    Instant(
        i64::from(day)
            .saturating_mul(NANOS_PER_DAY)
            .saturating_add(NANOS_PER_DAY / 2),
    )
}

/// What a play of the stub runner needs from the host of its store (contracts.md L10.1 to L10.3): the clock's day,
/// the workflow that a test proposer and an approval make, the requests of the runner and the coordinator through
/// the handlers of the arm, the rows of the family `learn`, the restart of the host and the store, and, for the arm
/// `none`, the store as it stood after the compile. The memory store (`GateHost`) and the derived store
/// (`crate::derived::DerivedHost`) are its two hosts.
pub trait PlayHost {
    /// Links the handlers of the arm: `LearnHandlers` in front of the agent store's handler in the arm `admit`, the
    /// agent store's handler alone in the arms `none` and `plain`.
    fn link(&mut self, arm: Arm);

    /// Sets the clock to `day`.
    fn set_day(&mut self, day: u32);

    /// The instant of a request on the clock's day.
    fn now(&self) -> Now;

    /// The day that the agent store's handler takes from `now`.
    fn request_day(&self) -> u32;

    /// Stores the workflow of the play numbered `n`, approved and with no agent, and answers its id: the memory
    /// store keeps `workflow` as its id, the derived store gives the id that it mints.
    fn store_workflow(&mut self, n: u32, workflow: &str) -> Result<String, String>;

    /// Sends one `coordinator_write` body; the error is the reply's error, as text.
    fn coordinator(&mut self, body: &[u8]) -> Result<(), String>;

    /// Sends one `runner` body and answers its reply.
    fn runner(&mut self, body: &[u8]) -> Result<RunnerReply, String>;

    /// The rows of one kind of the family `learn` as the store hands them out, in its order.
    fn learn_rows(&mut self, kind: &str) -> Result<Vec<String>, String>;

    /// After the finish of a run: the derived store notes the `learn_run` rows that its read hands out, which a later
    /// open counts as expired when its read no longer hands them out.
    fn ended(&mut self) -> Result<(), String>;

    /// Closes and opens the host and the store on the clock's day: the handlers lose the facts of every run, and the
    /// open expires bounded rows. Answers the `learn_run` rows that passed their bound since the last open.
    fn restart(&mut self) -> Result<u32, String>;

    /// Keeps the store as it stands after the compile.
    fn keep_after_compile(&mut self) -> Result<(), String>;

    /// Puts the store back to what `keep_after_compile` kept, where it kept one.
    fn back_to_after_compile(&mut self) -> Result<(), String>;
}

/// The gate host on the memory store (contracts.md L10.1, L10.2): the memory store, the day of its clock, which the
/// command sets for each occurrence, and the handlers of the arm. It gives `LearnHandlers` the calls of the family
/// `learn` and the agent store's handler the calls of the family `agent`.
pub struct GateHost {
    pub store: MemoryStore,
    pub day: u32,
    arm: Arm,
    handlers: LearnHandlers,
    after_compile: Option<MemoryStore>,
}

impl GateHost {
    pub fn new(store: MemoryStore, day: u32) -> GateHost {
        GateHost {
            store,
            day,
            arm: Arm::Admit,
            handlers: LearnHandlers::new(),
            after_compile: None,
        }
    }

    /// The instant and the day of a request: noon UTC of the clock's day (`noon`).
    pub fn now(&self) -> Now {
        Now { at: noon(self.day) }
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

    fn for_each_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        self.store
            .rows(Family::Learn)
            .for_each_under(kind, workflow, visit);
    }

    fn workflow(&self, id: &WorkflowId) -> Option<WorkflowItem> {
        self.store.workflow_item(id)
    }
}

impl PlayHost for GateHost {
    fn link(&mut self, arm: Arm) {
        self.arm = arm;
    }

    fn set_day(&mut self, day: u32) {
        self.day = day;
    }

    fn now(&self) -> Now {
        GateHost::now(self)
    }

    fn request_day(&self) -> u32 {
        self.store.day(self.now().at)
    }

    fn store_workflow(&mut self, _n: u32, workflow: &str) -> Result<String, String> {
        self.store.put_workflow(workflow_record(workflow));
        Ok(workflow.to_owned())
    }

    fn coordinator(&mut self, body: &[u8]) -> Result<(), String> {
        match self.arm {
            Arm::Admit => {
                let mut handlers = std::mem::take(&mut self.handlers);
                let answer = handlers
                    .coordinator(self, body)
                    .map(|_| ())
                    .map_err(|e| format!("{e:?}"));
                self.handlers = handlers;
                answer
            }
            Arm::None | Arm::Plain => {
                let now = GateHost::now(self);
                halvo_agent_store::coordinator(&mut self.store, now, body)
                    .map(|_: CoordinatorReply| ())
                    .map_err(|e| format!("{e:?}"))
            }
        }
    }

    fn runner(&mut self, body: &[u8]) -> Result<RunnerReply, String> {
        match self.arm {
            Arm::Admit => {
                let mut handlers = std::mem::take(&mut self.handlers);
                let answer = handlers.runner(self, body).map_err(|e| format!("{e:?}"));
                self.handlers = handlers;
                answer
            }
            Arm::None | Arm::Plain => {
                let now = GateHost::now(self);
                halvo_agent_store::runner(&mut self.store, now, body).map_err(|e| format!("{e:?}"))
            }
        }
    }

    fn learn_rows(&mut self, kind: &str) -> Result<Vec<String>, String> {
        let mut rows = Vec::new();
        self.store
            .rows(Family::Learn)
            .for_each(kind, &mut |row| rows.push(row.to_owned()));
        Ok(rows)
    }

    fn ended(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn restart(&mut self) -> Result<u32, String> {
        self.handlers = LearnHandlers::new();
        let expired = self.store.open(self.day);
        Ok(u32::try_from(expired.of("learn_run")).unwrap_or(u32::MAX))
    }

    fn keep_after_compile(&mut self) -> Result<(), String> {
        self.after_compile = Some(self.store.clone());
        Ok(())
    }

    fn back_to_after_compile(&mut self) -> Result<(), String> {
        if let Some(store) = &self.after_compile {
            self.store = store.clone();
        }
        Ok(())
    }
}
