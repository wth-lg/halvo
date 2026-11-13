use std::collections::BTreeMap;

use halvo_schema::coordinator::record::CoordEvent;
use halvo_schema::coordinator::KINDS;
use halvo_schema::kinds::check_record;
use halvo_schema::{
    KindEntry, Origin, RetentionClass, RowRead, Summary, SummaryRead, TypedRecord, Workflow,
    WorkflowId, WorkflowItem, WriterStep,
};

use crate::host::{Clock, CommitRefused, DayFault, Host};

const AHEAD_DAYS: i64 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Caller {
    Handler,
    Step,
}

struct NoSummaries;

impl SummaryRead for NoSummaries {
    fn for_each(&self, _visit: &mut dyn FnMut(&Summary)) {}
}

/// The store in memory behind the handler, as the derived store will serve it: the workflows that a host puts, the
/// rows of the family `coord`, the host's clock and its steps. Its typed commit is the commit of a handler and holds
/// the rules of W-C25.3 for the kinds of `halvo_schema::coordinator::KINDS`: the kind, `check_record`, the typed
/// reader, a stored workflow, and a bounded day that is neither after the clock's day nor outside its bound.
/// `after_batch` runs the steps as the derived store does after a committed batch and refuses a record of origin own
/// that a step returns, which every kind of the family is. `expire` removes the rows past their bound by W-C2.5.
/// This store holds no delete of a workflow: the cascade of W-C3.10 is the derived store's.
pub struct MemoryStore {
    clock: Box<dyn Clock>,
    workflows: BTreeMap<WorkflowId, Workflow>,
    rows: Vec<(&'static str, String)>,
    steps: Vec<Box<dyn WriterStep>>,
}

impl MemoryStore {
    /// A store on the clock, with no workflow, no row and no step.
    pub fn new(clock: Box<dyn Clock>) -> MemoryStore {
        MemoryStore {
            clock,
            workflows: BTreeMap::new(),
            rows: Vec::new(),
            steps: Vec::new(),
        }
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

    /// The expiry, on the clock as it reads now: it removes every row of a bounded kind whose day lies the kind's
    /// days or more before the clock's day, or more than two days after it, and every row of a bounded kind whose
    /// day cannot be read. A row whose day lies one or two days after the clock's day stays. It returns the number
    /// of rows removed.
    pub fn expire(&mut self) -> usize {
        let today = self.today();
        let before = self.rows.len();
        self.rows.retain(|(kind, json)| keeps(today, kind, json));
        before - self.rows.len()
    }

    fn today(&self) -> i64 {
        i64::from(self.clock.day(self.clock.now()))
    }

    fn commit_from(
        &mut self,
        caller: Caller,
        records: Vec<TypedRecord>,
    ) -> Result<(), CommitRefused> {
        let today = self.today();
        let mut checked = Vec::with_capacity(records.len());
        for record in records {
            let kind = self.check(caller, today, &record)?;
            checked.push((kind, record.json));
        }
        self.rows.extend(checked);
        Ok(())
    }

    fn check(
        &self,
        caller: Caller,
        today: i64,
        record: &TypedRecord,
    ) -> Result<&'static str, CommitRefused> {
        let entry = entry(record.kind).ok_or(CommitRefused::Kind)?;
        if caller == Caller::Step && entry.origin == Origin::Own {
            return Err(CommitRefused::OwnFromStep);
        }
        check_record(entry, &record.json).map_err(|_| CommitRefused::Record)?;
        let event = CoordEvent::parse(record.json.as_bytes()).map_err(|_| CommitRefused::Record)?;
        if !self.workflows.contains_key(&event.workflow) {
            return Err(CommitRefused::Workflow);
        }
        if let RetentionClass::Bounded { days } = entry.class {
            if let Some(fault) = day_fault(today, days, event.day) {
                return Err(CommitRefused::Day(fault));
            }
        }
        Ok(entry.name)
    }
}

fn entry(kind: &str) -> Option<&'static KindEntry> {
    KINDS.iter().find(|e| e.name == kind)
}

fn day_fault(today: i64, days: u16, day: u32) -> Option<DayFault> {
    let age = today - i64::from(day);
    if age < 0 {
        Some(DayFault::AfterToday)
    } else if age >= i64::from(days) {
        Some(DayFault::OutsideBound)
    } else {
        None
    }
}

fn keeps(today: i64, kind: &str, json: &str) -> bool {
    let Some(RetentionClass::Bounded { days }) = entry(kind).map(|e| e.class) else {
        return true;
    };
    let Ok(event) = CoordEvent::parse(json.as_bytes()) else {
        return false;
    };
    let age = today - i64::from(event.day);
    age < i64::from(days) && age >= -AHEAD_DAYS
}

impl RowRead for MemoryStore {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        for (k, row) in &self.rows {
            if *k == kind {
                visit(row);
            }
        }
    }
}

impl Host for MemoryStore {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), CommitRefused> {
        self.commit_from(Caller::Handler, records)
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

    fn clock(&self) -> &dyn Clock {
        &*self.clock
    }
}
