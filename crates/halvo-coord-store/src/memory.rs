use std::collections::BTreeMap;

use halvo_schema::coordinator::record::CoordEvent;
use halvo_schema::coordinator::KINDS;
use halvo_schema::kinds::{check_record, registry};
use halvo_schema::{
    KindEntry, Origin, RetentionClass, RowRead, Summary, SummaryRead, TypedRecord, Workflow,
    WorkflowId, WorkflowItem, WriterStep,
};
use serde_json::{Map, Value};

use crate::host::{Clock, CommitRefused, DayFault, HeldRefused, Host, StoreFault};

const AHEAD_DAYS: i64 = 2;
const FAMILY: &str = "coord";
const COORDINATOR_WRITE: &str = "coordinator_write";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Caller {
    Handler,
    Step,
}

struct NoSummaries;

impl SummaryRead for NoSummaries {
    fn for_each(&self, _visit: &mut dyn FnMut(&Summary)) {}
}

struct Row {
    kind: &'static str,
    workflow: Option<String>,
    day: Option<i64>,
    json: String,
}

impl Row {
    fn stored(kind: &'static str, json: String) -> Row {
        let top = serde_json::from_str::<Map<String, Value>>(&json).ok();
        let workflow = entry(kind)
            .and_then(|e| e.workflow_key)
            .and_then(|key| top.as_ref()?.get(key)?.as_str().map(str::to_owned));
        let day = top
            .as_ref()
            .and_then(|t| t.get("day")?.as_u64())
            .and_then(|d| i64::try_from(d).ok());
        Row {
            kind,
            workflow,
            day,
            json,
        }
    }
}

/// The store in memory behind the handler, as the derived store will serve it: the workflows that a host puts, the
/// rows of the family `coord`, the host's clock and its steps. Its typed commit is the commit of a handler and holds
/// the rules of W-C25.3 for the kinds of `halvo_schema::coordinator::KINDS`: the kind, `check_record`, the typed
/// reader, a stored workflow, and a bounded day that is neither after the clock's day nor outside its bound.
/// `after_batch` runs the steps as the derived store does after a committed batch and refuses a record of origin own
/// that a step returns, which every kind of the family is. `expire` removes the rows past their bound by W-C2.5.
///
/// Beside each row the commit keeps, from the committed text and without the typed reader, the workflow id under the
/// kind's workflow key and the row's top-level `day`, as the derived store keeps its workflow column and its day of
/// going (RD-64 1.3). The keyed read of `Host::rows_under` hands out the rows held under one workflow id by that key
/// alone. Its reads hand out exactly the rows that an expiry at the read's clock would leave (the workflow run's
/// ruling DS-4): the reads and `expire` rest on one condition, so that a row past its bound, or more than two days
/// ahead of the clock, is never handed out, called `expire` or not. A row is dated by its top-level day alone, so a
/// row that the typed reader refuses is handed out while it lies inside its bound and goes at its own day, and is
/// never due because the typed reader refuses it; a row of a bounded kind whose day cannot be read is not handed out
/// and is due (DS-4, DS-2 E3).
///
/// Its fifth call answers as the derived store's `HandlerCx::held_ahead` does, the refusals included: it holds the
/// entries of `halvo_schema::kinds::registry` and those that `with_kind` adds, it answers `HeldRefused::Role` while it
/// serves a request that is no `coordinator_write` by `serve_as`, and it dates a row by the day kept beside it, so a
/// row that the typed reader refuses makes a yes while it lies more than two days ahead. Its fifth call has no fault.
/// What this stand-in lacks against the derived store: one reading of the clock for a whole request, which is the
/// adapter's; here each read, each fifth call and each commit reads the clock anew. This store holds no delete of a
/// workflow: the cascade of W-C3.10 is the derived store's.
pub struct MemoryStore {
    clock: Box<dyn Clock>,
    workflows: BTreeMap<WorkflowId, Workflow>,
    rows: Vec<Row>,
    steps: Vec<Box<dyn WriterStep>>,
    kinds: Vec<&'static KindEntry>,
    request: &'static str,
}

impl MemoryStore {
    /// A store on the clock, with no workflow, no row and no step, serving a `coordinator_write`.
    pub fn new(clock: Box<dyn Clock>) -> MemoryStore {
        MemoryStore {
            clock,
            workflows: BTreeMap::new(),
            rows: Vec::new(),
            steps: Vec::new(),
            kinds: Vec::new(),
            request: COORDINATOR_WRITE,
        }
    }

    /// The store with one more step after the steps that it has.
    pub fn with_step(mut self, step: Box<dyn WriterStep>) -> MemoryStore {
        self.steps.push(step);
        self
    }

    /// The store with one more entry beside the registry's, as the derived store is opened with the entries that its
    /// host gives. The fifth call reads it; the commit takes the kinds of `halvo_schema::coordinator::KINDS` alone.
    pub fn with_kind(mut self, entry: &'static KindEntry) -> MemoryStore {
        self.kinds.push(entry);
        self
    }

    /// The store answers the handler's calls from now on as under a request of the name `request`; the fifth call
    /// answers `HeldRefused::Role` under every name but `coordinator_write`.
    pub fn serve_as(&mut self, request: &'static str) {
        self.request = request;
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

    /// The expiry, on the clock as it reads now: it removes every row of a bounded kind whose top-level day lies the
    /// kind's days or more before the clock's day, or more than two days after it, and every row of a bounded kind
    /// whose day cannot be read. A row whose day lies one or two days after the clock's day stays. It returns the
    /// number of rows removed.
    pub fn expire(&mut self) -> usize {
        let today = self.today();
        let before = self.rows.len();
        self.rows.retain(|row| handed_out(today, row));
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
            checked.push(Row::stored(kind, record.json));
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

fn handed_out(today: i64, row: &Row) -> bool {
    let Some(RetentionClass::Bounded { days }) = entry(row.kind).map(|e| e.class) else {
        return true;
    };
    let Some(day) = row.day else {
        return false;
    };
    let age = today - day;
    age < i64::from(days) && age >= -AHEAD_DAYS
}

impl RowRead for MemoryStore {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        let today = self.today();
        for row in &self.rows {
            if row.kind == kind && handed_out(today, row) {
                visit(&row.json);
            }
        }
    }
}

impl Host for MemoryStore {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), CommitRefused> {
        self.commit_from(Caller::Handler, records)
    }

    fn rows_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        let today = self.today();
        for row in &self.rows {
            if row.kind == kind
                && row.workflow.as_deref() == Some(workflow.as_str())
                && handed_out(today, row)
            {
                visit(&row.json);
            }
        }
    }

    fn workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowItem>, StoreFault> {
        Ok(self.workflows.get(id).map(|w| WorkflowItem {
            workflow: w.clone(),
            summaries: Vec::new(),
            hint: None,
        }))
    }

    fn clock(&self) -> &dyn Clock {
        &*self.clock
    }

    fn held_ahead(&self, kind: &str, workflow: &WorkflowId) -> Result<bool, HeldRefused> {
        if self.request != COORDINATOR_WRITE {
            return Err(HeldRefused::Role);
        }
        let entry = registry()
            .chain(self.kinds.iter().copied())
            .find(|e| e.name == kind)
            .ok_or(HeldRefused::UnknownKind)?;
        if entry.family == "workflow" {
            return Err(HeldRefused::WorkflowRun);
        }
        if entry.family != FAMILY {
            return Err(HeldRefused::OtherFamily);
        }
        if entry.workflow_key.is_none() {
            return Err(HeldRefused::NoWorkflowKey);
        }
        let today = self.today();
        Ok(self.rows.iter().any(|row| {
            row.kind == kind
                && row.workflow.as_deref() == Some(workflow.as_str())
                && row.day.is_some_and(|day| day - today > AHEAD_DAYS)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halvo_schema::Instant;

    const DAY_NS: i64 = 86_400_000_000_000;
    const TODAY: u32 = 20_771;

    struct At(std::rc::Rc<std::cell::Cell<i64>>);

    impl Clock for At {
        fn now(&self) -> Instant {
            Instant(self.0.get())
        }

        fn day(&self, at: Instant) -> u32 {
            u32::try_from(at.0.div_euclid(DAY_NS)).unwrap_or(0)
        }
    }

    fn store() -> (MemoryStore, std::rc::Rc<std::cell::Cell<i64>>) {
        let now = std::rc::Rc::new(std::cell::Cell::new(i64::from(TODAY) * DAY_NS));
        (
            MemoryStore::new(Box::new(At(std::rc::Rc::clone(&now)))),
            now,
        )
    }

    fn wf(n: u32) -> String {
        format!("3{n:031x}")
    }

    fn id(n: u32) -> WorkflowId {
        WorkflowId::parse(&wf(n)).unwrap()
    }

    fn text(v: u32, workflow: u32, day: u32) -> String {
        format!(
            r#"{{"v":{v},"workflow":"{}","agent":"4{:031x}","seq":1,"day":{day}}}"#,
            wf(workflow),
            1
        )
    }

    fn under(store: &MemoryStore, n: u32) -> Vec<String> {
        let mut seen = Vec::new();
        store.rows_under("coord_event", &id(n), &mut |row| seen.push(row.to_owned()));
        seen
    }

    fn whole(store: &MemoryStore) -> Vec<String> {
        let mut seen = Vec::new();
        store.for_each("coord_event", &mut |row| seen.push(row.to_owned()));
        seen
    }

    #[test]
    fn a_bounded_row_whose_day_cannot_be_read_is_not_handed_out_and_is_due() {
        let (mut store, _) = store();
        for json in [
            r#"{"v":2}"#.to_owned(),
            format!(r#"{{"v":2,"workflow":"{}","day":"x"}}"#, wf(1)),
            format!(r#"{{"v":2,"workflow":"{}","day":-1}}"#, wf(1)),
            "not json".to_owned(),
        ] {
            store.rows.push(Row::stored("coord_event", json));
        }
        assert!(whole(&store).is_empty());
        assert!(under(&store, 1).is_empty());
        assert_eq!(store.expire(), 4);
    }

    #[test]
    fn a_row_that_the_typed_reader_refuses_is_handed_out_inside_its_bound_and_goes_at_its_day() {
        let (mut store, now) = store();
        let unread = text(2, 1, TODAY);
        store.rows.push(Row::stored("coord_event", unread.clone()));
        assert_eq!(under(&store, 1), std::slice::from_ref(&unread));
        assert_eq!(whole(&store), std::slice::from_ref(&unread));
        assert_eq!(store.expire(), 0);
        now.set(i64::from(TODAY + 92) * DAY_NS);
        assert_eq!(under(&store, 1), std::slice::from_ref(&unread));
        assert_eq!(store.expire(), 0);
        now.set(i64::from(TODAY + 93) * DAY_NS);
        assert!(under(&store, 1).is_empty());
        assert_eq!(store.expire(), 1);
    }

    #[test]
    fn the_keyed_read_hands_out_the_rows_held_under_one_workflow_id_alone() {
        let (mut store, _) = store();
        let first = text(2, 1, TODAY);
        let second = text(2, 2, TODAY);
        store.rows.push(Row::stored("coord_event", first.clone()));
        store.rows.push(Row::stored("coord_event", second.clone()));
        assert_eq!(under(&store, 1), std::slice::from_ref(&first));
        assert_eq!(under(&store, 2), std::slice::from_ref(&second));
        assert!(under(&store, 3).is_empty());
        assert_eq!(whole(&store), [first, second]);
        let mut seen = 0;
        store.rows_under("agent_run", &id(1), &mut |_| seen += 1);
        assert_eq!(seen, 0);
    }

    #[test]
    fn a_row_that_the_typed_reader_refuses_makes_a_yes_by_the_day_kept_beside_it() {
        let (mut store, now) = store();
        store
            .rows
            .push(Row::stored("coord_event", text(2, 1, TODAY + 3)));
        store.rows.push(Row::stored(
            "coord_event",
            format!(r#"{{"v":2,"workflow":"{}"}}"#, wf(2)),
        ));
        assert_eq!(Host::held_ahead(&store, "coord_event", &id(1)), Ok(true));
        assert_eq!(Host::held_ahead(&store, "coord_event", &id(2)), Ok(false));
        assert!(under(&store, 1).is_empty());
        now.set(i64::from(TODAY + 1) * DAY_NS);
        assert_eq!(Host::held_ahead(&store, "coord_event", &id(1)), Ok(false));
        assert_eq!(under(&store, 1), [text(2, 1, TODAY + 3)]);
    }

    #[test]
    fn the_key_beside_a_row_is_the_one_that_its_text_gave_at_the_commit() {
        let (mut store, _) = store();
        let theirs = text(1, 2, TODAY);
        store.rows.push(Row {
            kind: "coord_event",
            workflow: Some(wf(1)),
            day: Some(i64::from(TODAY)),
            json: theirs.clone(),
        });
        assert_eq!(under(&store, 1), [theirs]);
        assert!(under(&store, 2).is_empty());
    }
}
