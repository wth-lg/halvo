use halvo_schema::learning::record::{LearnRoutine, LearnRun};
use halvo_schema::Summary;
use serde_json::{json, Value};

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
