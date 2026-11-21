#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use halvo_coord_store::{
    Clock, CommitRefused, CoordReply, HeldRefused, Host, MemoryStore, StoreFault,
};
use halvo_schema::coordinator::body::Stored;
use halvo_schema::coordinator::record::CoordEvent;
use halvo_schema::request::decode;
use halvo_schema::{
    Decoded, ErrorName, Instant, RawRequest, RowRead, TypedRecord, Workflow, WorkflowId,
    WorkflowItem,
};
use serde_json::json;

pub const DAY_S: i64 = 86_400;
pub const NS: i64 = 1_000_000_000;
pub const TODAY: u32 = 20_771;
pub const WEST: i64 = -5 * 3_600;

pub struct TestClock {
    now: Rc<Cell<i64>>,
    offset_s: i64,
}

impl Clock for TestClock {
    fn now(&self) -> Instant {
        Instant(self.now.get())
    }

    fn day(&self, at: Instant) -> u32 {
        let local = at.0.div_euclid(NS) + self.offset_s;
        u32::try_from(local.div_euclid(DAY_S)).unwrap_or(0)
    }
}

pub fn at(day: u32, seconds: i64, offset_s: i64) -> i64 {
    (i64::from(day) * DAY_S + seconds - offset_s) * NS
}

pub fn noon(day: u32) -> i64 {
    at(day, 12 * 3_600, 0)
}

pub fn id(prefix: char, n: u32) -> String {
    format!("{prefix}{n:031x}")
}

pub fn wf(n: u32) -> String {
    id('3', n)
}

pub fn agent(n: u32) -> String {
    id('4', n)
}

pub fn run(n: u32) -> String {
    id('5', n)
}

pub fn item(n: u32) -> String {
    id('7', n)
}

pub fn workflow(n: u32, agent_n: Option<u32>) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": wf(n),
        "state": if agent_n.is_some() { "active" } else { "approved" },
        "category": "email",
        "site": null,
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": agent_n.map(agent),
    }))
    .expect("a workflow record")
}

pub fn dismissed(n: u32) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": wf(n),
        "state": "dismissed",
        "category": "email",
        "site": null,
        "app": "com.example.browser",
    }))
    .expect("a dismissed workflow")
}

/// A store whose clock reads `now` in a zone `offset_s` seconds east of UTC, with workflow 1 of agent 1 and
/// workflow 2 of agent 2, and the cell that sets the clock.
pub fn store_at(now: i64, offset_s: i64) -> (MemoryStore, Rc<Cell<i64>>) {
    let cell = Rc::new(Cell::new(now));
    let mut store = MemoryStore::new(Box::new(TestClock {
        now: Rc::clone(&cell),
        offset_s,
    }));
    store.put_workflow(workflow(1, Some(1)));
    store.put_workflow(workflow(2, Some(2)));
    (store, cell)
}

/// A store whose clock reads noon of `TODAY` in UTC.
pub fn store() -> MemoryStore {
    store_at(noon(TODAY), 0).0
}

/// A host over a store whose keyed read hands out, after the store's own rows, each extra row held under the
/// workflow of its number, as rows that another build stored, whose commit answers the refusal where one is given,
/// whose read of a workflow faults where `workflow_fault` is set, and whose fifth call answers `held` where it is
/// given; it keeps the kind and the workflow of each fifth call.
pub struct StandIn {
    pub store: MemoryStore,
    pub extra: Vec<(u32, String)>,
    pub refusal: Option<CommitRefused>,
    pub workflow_fault: bool,
    pub held: Option<Result<bool, HeldRefused>>,
    pub asks: RefCell<Vec<(String, String)>>,
}

impl Host for StandIn {
    fn commit(&mut self, records: Vec<TypedRecord>) -> Result<(), CommitRefused> {
        match self.refusal {
            Some(refused) => Err(refused),
            None => Host::commit(&mut self.store, records),
        }
    }

    fn rows_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        self.store.rows_under(kind, workflow, visit);
        for (n, row) in &self.extra {
            if kind == "coord_event" && wf(*n) == workflow.as_str() {
                visit(row);
            }
        }
    }

    fn held_ahead(&self, kind: &str, workflow: &WorkflowId) -> Result<bool, HeldRefused> {
        self.asks
            .borrow_mut()
            .push((kind.to_owned(), workflow.as_str().to_owned()));
        match self.held {
            Some(held) => held,
            None => self.store.held_ahead(kind, workflow),
        }
    }

    fn workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowItem>, StoreFault> {
        if self.workflow_fault {
            return Err(StoreFault);
        }
        Host::workflow(&self.store, id)
    }

    fn clock(&self) -> &dyn Clock {
        Host::clock(&self.store)
    }
}

/// The stand-in over `store()` with the extra rows, each under the workflow of its number, and the refusal.
pub fn stand_in(extra: Vec<(u32, String)>, refusal: Option<CommitRefused>) -> StandIn {
    StandIn {
        store: store(),
        extra,
        refusal,
        workflow_fault: false,
        held: None,
        asks: RefCell::default(),
    }
}

/// A row `run_ended` of workflow 1 and agent 1 at noon of `TODAY` for run 1, with the cause, and with the keys
/// `defer_launch` and `defer_ticks_failed` after `origin`, the pair of their values where it is given and both null
/// where it is not (ruling RD-120).
pub fn run_ended_row(seq: u32, cause: &str, defers: Option<(&str, &str)>) -> String {
    let (launch, ticks) = defers.unwrap_or(("null", "null"));
    let tail = format!(r#","defer_launch":{launch},"defer_ticks_failed":{ticks}"#);
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":{seq},"day":{TODAY},"at":{},"event":"run_ended","run":"{}","slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"{cause}","trigger":null,"origin":null{tail}}}"#,
        wf(1),
        agent(1),
        noon(TODAY),
        run(1)
    )
}

/// A row `hold_set` with the cause `person`.
pub fn hold_set(w: u32, a: u32, seq: u32, day: u32, at: i64) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":{seq},"day":{day},"at":{at},"event":"hold_set","run":null,"slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"person","trigger":null,"origin":null,"defer_launch":null,"defer_ticks_failed":null}}"#,
        wf(w),
        agent(a)
    )
}

/// A row `run_started` of workflow `w` and agent `a` at noon of `TODAY` for run `run_n`, started by the person.
pub fn run_started(w: u32, a: u32, seq: u32, run_n: u32) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":{seq},"day":{TODAY},"at":{},"event":"run_started","run":"{}","slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":null,"trigger":"person","origin":null,"defer_launch":null,"defer_ticks_failed":null}}"#,
        wf(w),
        agent(a),
        noon(TODAY),
        run(run_n)
    )
}

/// A row `run_ended` of workflow `w` and agent `a` at noon of `TODAY` for run `run_n`, with the cause `completed`
/// and no defer values.
pub fn run_ended(w: u32, a: u32, seq: u32, run_n: u32) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":{seq},"day":{TODAY},"at":{},"event":"run_ended","run":"{}","slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"completed","trigger":null,"origin":null,"defer_launch":null,"defer_ticks_failed":null}}"#,
        wf(w),
        agent(a),
        noon(TODAY),
        run(run_n)
    )
}

/// The id of workflow `n`.
pub fn wid(n: u32) -> WorkflowId {
    WorkflowId::parse(&wf(n)).expect("a workflow id")
}

/// The read of the family `coord` that the guard of a delete gets: the keyed read of the host under one workflow.
pub struct Under<'a>(pub &'a dyn Host, pub WorkflowId);

impl RowRead for Under<'_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        self.0.rows_under(kind, &self.1, visit);
    }
}

/// A row of workflow 1 and agent 1 at noon of its day.
pub fn row(seq: u32, day: u32) -> String {
    hold_set(1, 1, seq, day, noon(day))
}

/// A row `hold_set` or `hold_cleared` of workflow 1 and agent 1 at noon of `TODAY` with the cause and the run.
pub fn hold_row(seq: u32, event: &str, cause: &str, run_n: Option<u32>) -> String {
    let run = run_n.map_or("null".to_owned(), |n| format!("\"{}\"", run(n)));
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":{seq},"day":{TODAY},"at":{},"event":"{event}","run":{run},"slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"{cause}","trigger":null,"origin":null,"defer_launch":null,"defer_ticks_failed":null}}"#,
        wf(1),
        agent(1),
        noon(TODAY)
    )
}

/// A row `item_answered` of workflow 1 and agent 1 at noon of its day: the answer `reject_once` to effect `n` of the
/// confirmation item 1.
pub fn confirm_answered(seq: u32, day: u32, n: u32) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":{seq},"day":{day},"at":{},"event":"item_answered","run":null,"slot":null,"item":"{}","kind":"confirm","effect":"{}","approval":"{}","digest":"{}","answer":"reject_once","cause":null,"trigger":null,"origin":null,"defer_launch":null,"defer_ticks_failed":null}}"#,
        wf(1),
        agent(1),
        noon(day),
        item(1),
        id('6', n),
        id('8', n),
        "ab".repeat(32)
    )
}

pub fn typed(json: String) -> TypedRecord {
    TypedRecord {
        kind: "coord_event",
        json,
    }
}

pub fn rows(store: &MemoryStore) -> Vec<String> {
    let mut out = Vec::new();
    store.for_each("coord_event", &mut |row| out.push(row.to_owned()));
    out
}

pub fn declared(name: &str) -> ErrorName {
    ErrorName::read(name).expect("a declared error name")
}

pub fn stored(n: u32) -> Result<CoordReply, ErrorName> {
    Ok(CoordReply::Stored(Stored { stored: n }))
}

/// What the typed writer writes for the row.
pub fn canonical(json: &str) -> String {
    CoordEvent::parse(json.as_bytes())
        .expect("a coord_event")
        .to_json()
        .expect("written")
}

pub const REQUEST_ID: &str = "7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d";

/// The raw request of the name with the body text as it is.
pub fn request(name: &str, body: &str) -> RawRequest {
    let bytes = format!(r#"{{"v":1,"id":"{REQUEST_ID}","request":"{name}","body":{body}}}"#);
    match decode(bytes.as_bytes()) {
        Ok(Decoded::Raw(raw)) => raw,
        other => panic!("not a raw request: {other:?}"),
    }
}

/// The body `coord_log` of workflow `w` and agent `a` with the rows.
pub fn log_body(w: u32, a: u32, records: &[String]) -> String {
    format!(
        r#"{{"op":"coord_log","workflow":"{}","agent":"{}","records":[{}]}}"#,
        wf(w),
        agent(a),
        records.join(",")
    )
}

/// The coordinator write `coord_log` of workflow 1 and agent 1 with the rows.
pub fn log(records: &[String]) -> RawRequest {
    request("coordinator_write", &log_body(1, 1, records))
}

/// The body `coord_history` of workflow `w` and agent `a` after the count.
pub fn history_body(w: u32, a: u32, after: Option<u32>) -> String {
    let after = after.map_or("null".to_owned(), |n| n.to_string());
    format!(
        r#"{{"op":"coord_history","workflow":"{}","agent":"{}","after":{after}}}"#,
        wf(w),
        agent(a)
    )
}

/// The coordinator write `coord_history` of workflow 1 and agent 1 after the count.
pub fn history(after: Option<u32>) -> RawRequest {
    request("coordinator_write", &history_body(1, 1, after))
}
