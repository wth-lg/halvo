use std::num::NonZeroU32;

use halvo_learn::{Mark as RuleMark, RoutineState};
use halvo_schema::learning::names::Mark;
use halvo_schema::learning::record::{Day, LearnRoutine, LearnRun};
use halvo_schema::{RowRead, Summary, SummaryRead, TypedRecord, WriterStep};
use serde_json::{json, Value};

use crate::routine::LearnStep;
use crate::Clock;

pub struct Fixed(pub u32);

impl Clock for Fixed {
    fn today(&self) -> u32 {
        self.0
    }
}

#[derive(Default)]
pub struct Store {
    pub rows: Vec<(&'static str, String)>,
    pub summaries: Vec<Summary>,
}

impl Store {
    pub fn put_routine(&mut self, row: &LearnRoutine) {
        self.rows
            .push(("learn_routine", row.to_json().expect("a writable row")));
    }

    pub fn put_run(&mut self, row: &LearnRun) {
        self.rows
            .push(("learn_run", row.to_json().expect("a writable row")));
    }

    pub fn routines(&self) -> Vec<LearnRoutine> {
        self.rows
            .iter()
            .filter(|(kind, _)| *kind == "learn_routine")
            .map(|(_, json)| LearnRoutine::parse(json.as_bytes()).expect("a stored row"))
            .collect()
    }

    pub fn newest(&self, w: u32) -> LearnRoutine {
        self.routines()
            .into_iter()
            .filter(|r| r.workflow.as_str() == wf(w))
            .max_by_key(|r| r.rev)
            .expect("a routine row of the workflow")
    }

    pub fn step(&self, today: u32) -> Vec<TypedRecord> {
        LearnStep::new(Fixed(today)).after_commit(self, self)
    }

    pub fn commit(&mut self, records: &[TypedRecord]) {
        for record in records {
            self.rows.push((record.kind, record.json.clone()));
        }
    }
}

impl RowRead for Store {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        let mut rows: Vec<&String> = self
            .rows
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, json)| json)
            .collect();
        rows.sort();
        for row in rows {
            visit(row);
        }
    }
}

impl SummaryRead for Store {
    fn for_each(&self, visit: &mut dyn FnMut(&Summary)) {
        let mut summaries: Vec<&Summary> = self.summaries.iter().collect();
        summaries.sort_by(|a, b| (a.day, a.id.as_str()).cmp(&(b.day, b.id.as_str())));
        for summary in summaries {
            visit(summary);
        }
    }
}

pub fn figures(row: &LearnRoutine) -> RoutineState {
    RoutineState {
        period: row.period.map(NonZeroU32::get),
        seen: row.seen,
        first: row.first.map(Day::get),
        last: row.last.map(Day::get),
        next: row.next.map(Day::get),
        missed: row.missed,
        mark: match row.mark {
            Mark::None => RuleMark::None,
            Mark::Stale => RuleMark::Stale,
        },
        since: row.since.map(Day::get),
    }
}

pub fn with_figures(row: &LearnRoutine, rev: u32, day: u32, s: &RoutineState) -> LearnRoutine {
    let day_of = |d: Option<u32>| d.map(|d| Day::new(d).expect("a day above 0"));
    LearnRoutine {
        rev: NonZeroU32::new(rev).expect("a rev above 0"),
        day: Day::new(day).expect("a day above 0"),
        period: s
            .period
            .map(|p| NonZeroU32::new(p).expect("a period above 0")),
        seen: s.seen,
        first: day_of(s.first),
        last: day_of(s.last),
        next: day_of(s.next),
        missed: s.missed,
        mark: match s.mark {
            RuleMark::None => Mark::None,
            RuleMark::Stale => Mark::Stale,
        },
        since: day_of(s.since),
        ..row.clone()
    }
}

pub const EDIT_ROW: &str = r#"{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":1,"base":1,"n":4,"state":"provisional","cause":null,"on":null,"runs":["50000000000000000000000000000001"],"first":20767,"last":20767,"clean":0,"target":{"desc":"button \"Submit payment\"","locators":[{"by":"role","value":"button","name":"Submit payment"}]}}"#;

pub const DAY: u32 = 20_771;
pub const BILL: &str = "utility_bill_payment";
pub const ORDER: &str = "shop_order";
pub const HOME: &str = "northwind-power.test";
pub const OTHER: &str = "contoso-water.test";
pub const BROWSER: &str = "com.example.browser";
pub const MAIL: &str = "com.example.mail";
pub const HASH: &str = "9f2c4b7a1d3e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5061728394a5b6c7d8e";
pub const HASH_TWO: &str = "0f2c4b7a1d3e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5061728394a5b6c7d8e";

pub fn id(prefix: char, n: u32) -> String {
    format!("{prefix}{n:031x}")
}

pub fn wf(n: u32) -> String {
    id('3', n)
}

pub fn agent(n: u32) -> String {
    id('4', n)
}

pub fn domain(key: &str, source: &str) -> Value {
    json!({"key": key, "source": source})
}

pub fn title(hash: &str) -> Value {
    json!({"key": hash, "source": "title"})
}

pub fn summary(n: u32, category: &str, site: Value, app: &str, day: u32) -> Summary {
    serde_json::from_value(json!({
        "v": 1,
        "id": id('6', n),
        "category": category,
        "site": site,
        "app": app,
        "day": day,
        "part": "evening",
        "minutes": 5,
        "keys": null,
        "pointer": null,
    }))
    .expect("a summary record")
}

pub fn fresh(w: u32, day: u32) -> LearnRoutine {
    fresh_at(w, day, domain(HOME, "address_field"), BROWSER)
}

pub fn fresh_at(w: u32, day: u32, site: Value, app: &str) -> LearnRoutine {
    let row = json!({
        "v": 1,
        "workflow": wf(w),
        "agent": agent(w),
        "rev": 1,
        "day": day,
        "category": BILL,
        "site": site,
        "app": app,
        "period": null,
        "seen": 0,
        "first": null,
        "last": null,
        "next": null,
        "missed": 0,
        "mark": "none",
        "since": null,
    });
    LearnRoutine::parse(row.to_string().as_bytes()).expect("a learn_routine row")
}

pub fn run_of(w: u32, a: u32, n: u32, day: u32, mode: &str, state: &str) -> LearnRun {
    let ended = state != "started";
    let row = json!({
        "v": 1,
        "workflow": wf(w),
        "agent": agent(a),
        "run": id('5', n),
        "rev": if ended { 2 } else { 1 },
        "day": day,
        "mode": mode,
        "state": state,
        "cause": if state == "unverified" { json!("failed") } else { Value::Null },
        "base": if ended && mode == "replay" { json!(1) } else { Value::Null },
        "assisted": 0,
        "kept": 0,
        "failed_step": null,
    });
    LearnRun::parse(row.to_string().as_bytes()).expect("a learn_run row")
}

pub fn run(w: u32, n: u32, day: u32, mode: &str, state: &str) -> LearnRun {
    run_of(w, w, n, day, mode, state)
}
