#![allow(dead_code)]

use std::cell::Cell;
use std::rc::Rc;

use halvo_coord_store::{Clock, CoordReply, MemoryStore};
use halvo_schema::coordinator::body::Stored;
use halvo_schema::coordinator::record::CoordEvent;
use halvo_schema::request::decode;
use halvo_schema::{Decoded, ErrorName, Instant, RawRequest, RowRead, TypedRecord, Workflow};
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

/// A row `hold_set` with the cause `person`.
pub fn hold_set(w: u32, a: u32, seq: u32, day: u32, at: i64) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":{seq},"day":{day},"at":{at},"event":"hold_set","run":null,"slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"person","trigger":null,"origin":null}}"#,
        wf(w),
        agent(a)
    )
}

/// A row of workflow 1 and agent 1 at noon of its day.
pub fn row(seq: u32, day: u32) -> String {
    hold_set(1, 1, seq, day, noon(day))
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
