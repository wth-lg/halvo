use std::num::NonZeroU32;
use std::path::Path;

use halvo_schema::coordinator::body::{CoordHistory, CoordLog, History, Stored};
use halvo_schema::coordinator::names::Trigger;
use halvo_schema::coordinator::record::{BindAnswer, CoordEvent, ItemId, SignInAnswer, What};
use halvo_schema::coordinator::{names::BindResult, record::Answered};
use halvo_schema::coordinator::{HISTORY_RECORDS, REQUEST_BYTES, REQUEST_RECORDS};
use halvo_schema::executable::{RunId, WebOrigin};
use halvo_schema::record::Instant;
use halvo_schema::request::{
    decode, decode_reply, write_reply, CoordinatorBody, CoordinatorRole, CoordinatorWrite, Decoded,
    ReplyResult,
};
use halvo_schema::{AgentId, RequestId, WorkflowId};
use serde_json::value::RawValue;

const WORKFLOW: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
const AGENT: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f809";
const ID: &str = "2c3d4e5f60718293a4b5c6d7e8f90a1b";

fn workflow() -> WorkflowId {
    WorkflowId::parse(WORKFLOW).unwrap()
}

fn agent() -> AgentId {
    AgentId::parse(AGENT).unwrap()
}

fn skipped(seq: u32) -> CoordEvent {
    CoordEvent {
        workflow: workflow(),
        agent: agent(),
        seq: NonZeroU32::new(seq).unwrap(),
        day: 20767,
        at: Instant(1_794_339_000_000_000_000),
        what: What::SlotSkipped { slot: Some(20766) },
    }
}

fn started(seq: u32) -> CoordEvent {
    CoordEvent {
        what: What::RunStarted {
            run: RunId::parse("9f8e7d6c5b4a39281706f5e4d3c2b1a0").unwrap(),
            trigger: Trigger::Person,
            slot: None,
        },
        ..skipped(seq)
    }
}

fn long_origin(seq: u32) -> CoordEvent {
    let host = format!("{}.test", vec!["a".repeat(60); 4].join("."));
    CoordEvent {
        what: What::ItemAnswered {
            item: ItemId::from_random([7; 16]),
            answered: Answered::SignIn(SignInAnswer::Second(BindAnswer {
                answer: BindResult::Allow,
                origin: WebOrigin::parse(&format!("https://{host}:443")).unwrap(),
            })),
        },
        ..skipped(seq)
    }
}

fn log(records: Vec<CoordEvent>) -> CoordLog {
    CoordLog {
        workflow: workflow(),
        agent: agent(),
        records,
    }
}

fn reply_of<B: CoordinatorBody<Reply = R>, R>() {}

#[test]
fn r4_bodies_implement_coordinator_body_with_their_replies() {
    reply_of::<CoordLog, Stored>();
    reply_of::<CoordHistory, History>();
    assert_eq!(CoordLog::OP, "coord_log");
    assert_eq!(CoordHistory::OP, "coord_history");
}

#[test]
fn r4_bodies_coord_log_writes_op_workflow_agent_then_records_and_reads_them_back() {
    let body = log(vec![started(5), skipped(6)]);
    let text = body.to_json().unwrap();
    let records = format!(
        "[{},{}]",
        started(5).to_json().unwrap(),
        skipped(6).to_json().unwrap()
    );
    assert_eq!(
        text,
        format!(
            r#"{{"op":"coord_log","workflow":"{WORKFLOW}","agent":"{AGENT}","records":{records}}}"#
        )
    );
    assert_eq!(CoordLog::parse(text.as_bytes()).unwrap(), body);
    assert_eq!(serde_json::to_string(&body).unwrap(), text);
    assert_eq!(serde_json::from_str::<CoordLog>(&text).unwrap(), body);

    let id = RequestId::parse(ID).unwrap();
    let bytes = CoordinatorWrite::new(&CoordinatorRole::claim(), id, body.clone())
        .to_bytes()
        .unwrap();
    let Ok(Decoded::Raw(request)) = decode(&bytes) else {
        panic!("the request is no coordinator_write that decode returns undecoded");
    };
    assert_eq!(request.name(), "coordinator_write");
    assert_eq!(request.role(), Some(halvo_schema::Role::Coordinator));
    assert_eq!(
        CoordLog::parse(request.body().get().as_bytes()).unwrap(),
        body
    );
}

#[test]
fn r4_bodies_coord_log_holds_one_to_request_records_records_of_its_own_agent() {
    assert_eq!(REQUEST_RECORDS, 8);
    let full: Vec<CoordEvent> = (1..=8).map(skipped).collect();
    let text = log(full.clone()).to_json().unwrap();
    assert!(text.len() <= REQUEST_BYTES);
    assert_eq!(CoordLog::parse(text.as_bytes()).unwrap().records, full);

    let over: Vec<CoordEvent> = (1..=9).map(skipped).collect();
    let refusal = log(over.clone()).to_json().unwrap_err();
    assert_eq!(
        (refusal.key(), refusal.rule()),
        (Some("records"), "records")
    );
    assert!(serde_json::to_string(&log(over.clone())).is_err());
    let nine = format!(
        r#"{{"op":"coord_log","workflow":"{WORKFLOW}","agent":"{AGENT}","records":[{}]}}"#,
        over.iter()
            .map(|e| e.to_json().unwrap())
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(nine.len() <= REQUEST_BYTES);
    let refusal = CoordLog::parse(nine.as_bytes()).unwrap_err();
    assert_eq!(
        (refusal.key(), refusal.rule()),
        (Some("records"), "records")
    );

    let refusal = log(Vec::new()).to_json().unwrap_err();
    assert_eq!((refusal.key(), refusal.rule()), (Some("records"), "empty"));
    let empty =
        format!(r#"{{"op":"coord_log","workflow":"{WORKFLOW}","agent":"{AGENT}","records":[]}}"#);
    assert_eq!(
        CoordLog::parse(empty.as_bytes()).unwrap_err().rule(),
        "empty"
    );

    for other in [
        CoordEvent {
            agent: AgentId::parse("2a2b3c4d5e6f708192a3b4c5d6e7f809").unwrap(),
            ..skipped(2)
        },
        CoordEvent {
            workflow: WorkflowId::parse("1f1e2d3c4b5a69788796a5b4c3d2e1f0").unwrap(),
            ..skipped(2)
        },
    ] {
        let mixed = log(vec![skipped(1), other.clone()]);
        let refusal = mixed.to_json().unwrap_err();
        assert_eq!(
            (refusal.key(), refusal.rule()),
            (Some("records"), "other_agent")
        );
        let text = format!(
            r#"{{"op":"coord_log","workflow":"{WORKFLOW}","agent":"{AGENT}","records":[{},{}]}}"#,
            skipped(1).to_json().unwrap(),
            other.to_json().unwrap()
        );
        assert_eq!(
            CoordLog::parse(text.as_bytes()).unwrap_err().rule(),
            "other_agent"
        );
    }
}

#[test]
fn r4_bodies_coord_log_holds_at_most_request_bytes_bytes() {
    assert_eq!(REQUEST_BYTES, 4096);
    let big: Vec<CoordEvent> = (1..=8).map(long_origin).collect();
    let body = log(big.clone());
    let refusal = body.to_json().unwrap_err();
    assert_eq!((refusal.key(), refusal.rule()), (None, "bytes"));
    assert!(serde_json::to_string(&body).is_err());
    let id = RequestId::parse(ID).unwrap();
    assert!(CoordinatorWrite::new(&CoordinatorRole::claim(), id, body)
        .to_bytes()
        .is_err());

    let one = log(vec![skipped(1)]).to_json().unwrap();
    let pad = |to: usize| {
        let mut text = one.clone();
        text.insert_str(1, &" ".repeat(to - one.len()));
        text
    };
    let at_bound = pad(REQUEST_BYTES);
    assert_eq!(at_bound.len(), REQUEST_BYTES);
    assert!(CoordLog::parse(at_bound.as_bytes()).is_ok());
    let above = pad(REQUEST_BYTES + 1);
    assert_eq!(
        CoordLog::parse(above.as_bytes()).unwrap_err().rule(),
        "bytes"
    );
    assert!(serde_json::from_str::<CoordLog>(&above).is_err());
}

#[test]
fn r4_bodies_coord_log_refuses_another_op_and_another_key() {
    let good = log(vec![skipped(1)]).to_json().unwrap();
    let cases = [
        (
            r#""op":"coord_log""#,
            r#""op":"coord_logs""#,
            Some("op"),
            "unknown_op",
        ),
        (
            r#""op":"coord_log""#,
            r#""op":"coord_history""#,
            Some("op"),
            "unknown_op",
        ),
        (r#""op":"coord_log","#, "", Some("op"), "missing_key"),
        (
            r#""records":["#,
            r#""after":null,"records":["#,
            None,
            "unknown_key",
        ),
        (
            r#""agent":"1a"#,
            r#""agent":"Xa"#,
            Some("agent"),
            "hex_digit",
        ),
    ];
    for (from, to, key, rule) in cases {
        let text = good.replacen(from, to, 1);
        assert_ne!(text, good, "{from}");
        let refusal = CoordLog::parse(text.as_bytes()).unwrap_err();
        assert_eq!((refusal.key(), refusal.rule()), (key, rule), "{text}");
    }
    let refusal =
        CoordLog::parse(good.replacen(r#""seq":1"#, r#""seq":0"#, 1).as_bytes()).unwrap_err();
    assert_eq!((refusal.key(), refusal.rule()), (Some("seq"), "zero"));
}

#[test]
fn r4_bodies_stored_is_the_count_of_stored_records() {
    let stored = Stored { stored: 2 };
    assert_eq!(stored.to_json().unwrap(), r#"{"stored":2}"#);
    assert_eq!(Stored::parse(br#"{"stored":2}"#).unwrap(), stored);
    assert_eq!(
        serde_json::from_str::<Stored>(r#"{"stored":0}"#)
            .unwrap()
            .stored,
        0
    );
    for bad in [
        r#"{"stored":-1}"#,
        r#"{"stored":null}"#,
        r#"{"stored":2,"last":2}"#,
        "{}",
        "2",
    ] {
        assert!(Stored::parse(bad.as_bytes()).is_err(), "{bad}");
    }
    let raw = RawValue::from_string(stored.to_json().unwrap()).unwrap();
    let id = RequestId::parse(ID).unwrap();
    let bytes = write_reply(Some(&id), Ok(&ReplyResult::Raw(raw))).unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(r#"{{"v":1,"id":"{ID}","ok":{{"stored":2}}}}"#)
    );
    let Ok(ReplyResult::Raw(result)) = decode_reply(&bytes).unwrap().outcome else {
        panic!("no raw result");
    };
    assert_eq!(Stored::parse(result.get().as_bytes()).unwrap(), stored);
}

#[test]
fn r4_bodies_coord_history_reads_after_a_count_or_from_the_first() {
    for (after, text) in [
        (
            None,
            format!(
                r#"{{"op":"coord_history","workflow":"{WORKFLOW}","agent":"{AGENT}","after":null}}"#
            ),
        ),
        (
            Some(0),
            format!(
                r#"{{"op":"coord_history","workflow":"{WORKFLOW}","agent":"{AGENT}","after":0}}"#
            ),
        ),
        (
            Some(6),
            format!(
                r#"{{"op":"coord_history","workflow":"{WORKFLOW}","agent":"{AGENT}","after":6}}"#
            ),
        ),
    ] {
        let body = CoordHistory {
            workflow: workflow(),
            agent: agent(),
            after,
        };
        assert_eq!(body.to_json().unwrap(), text);
        assert_eq!(CoordHistory::parse(text.as_bytes()).unwrap(), body);
        assert_eq!(serde_json::from_str::<CoordHistory>(&text).unwrap(), body);
    }
    let good =
        format!(r#"{{"op":"coord_history","workflow":"{WORKFLOW}","agent":"{AGENT}","after":6}}"#);
    for (from, to, rule) in [
        (r#""after":6"#, r#""after":-1"#, "not_a_count"),
        (r#""after":6"#, r#""after":"6""#, "not_a_count"),
        (r#","after":6"#, "", "missing_key"),
        (
            r#""op":"coord_history""#,
            r#""op":"coord_log""#,
            "unknown_op",
        ),
        (r#""after":6"#, r#""after":6,"records":[]"#, "unknown_key"),
    ] {
        let text = good.replacen(from, to, 1);
        assert_ne!(text, good);
        assert_eq!(
            CoordHistory::parse(text.as_bytes()).unwrap_err().rule(),
            rule,
            "{text}"
        );
    }
}

#[test]
fn r4_bodies_history_holds_at_most_64_records_in_ascending_seq_up_to_last() {
    assert_eq!(HISTORY_RECORDS, 64);
    let page = History {
        records: (5..=68).map(skipped).collect(),
        last: 70,
    };
    let text = page.to_json().unwrap();
    assert_eq!(History::parse(text.as_bytes()).unwrap(), page);
    assert_eq!(serde_json::from_str::<History>(&text).unwrap(), page);
    assert!(text.starts_with(r#"{"records":["#) && text.ends_with(r#"],"last":70}"#));

    let empty = History {
        records: Vec::new(),
        last: 0,
    };
    assert_eq!(empty.to_json().unwrap(), r#"{"records":[],"last":0}"#);
    assert_eq!(
        History::parse(br#"{"records":[],"last":0}"#).unwrap(),
        empty
    );

    let refused = |page: History, rule: &str| {
        let refusal = page.to_json().unwrap_err();
        assert_eq!((refusal.key(), refusal.rule()), (Some("records"), rule));
        let records: Vec<String> = page.records.iter().map(|e| e.to_json().unwrap()).collect();
        let text = format!(
            r#"{{"records":[{}],"last":{}}}"#,
            records.join(","),
            page.last
        );
        assert_eq!(History::parse(text.as_bytes()).unwrap_err().rule(), rule);
        assert!(serde_json::to_string(&page).is_err());
    };
    refused(
        History {
            records: (5..=69).map(skipped).collect(),
            last: 70,
        },
        "records",
    );
    refused(
        History {
            records: vec![skipped(6), skipped(5)],
            last: 6,
        },
        "order",
    );
    refused(
        History {
            records: vec![skipped(5), skipped(5)],
            last: 6,
        },
        "order",
    );
    refused(
        History {
            records: vec![skipped(5), skipped(7)],
            last: 6,
        },
        "last",
    );
    refused(
        History {
            records: vec![
                skipped(5),
                CoordEvent {
                    agent: AgentId::parse("2a2b3c4d5e6f708192a3b4c5d6e7f809").unwrap(),
                    ..skipped(6)
                },
            ],
            last: 6,
        },
        "other_agent",
    );
    for bad in [
        r#"{"records":[],"last":null}"#,
        r#"{"records":null,"last":0}"#,
        r#"{"last":0}"#,
    ] {
        assert!(History::parse(bad.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn r4_bodies_a_record_of_another_workflow_or_agent_is_refused_at_any_place() {
    let others = [
        CoordEvent {
            workflow: WorkflowId::parse("1f1e2d3c4b5a69788796a5b4c3d2e1f0").unwrap(),
            ..skipped(1)
        },
        CoordEvent {
            agent: AgentId::parse("2a2b3c4d5e6f708192a3b4c5d6e7f809").unwrap(),
            ..skipped(1)
        },
    ];
    for other in others {
        let one = log(vec![other.clone()]);
        let refusal = one.to_json().unwrap_err();
        assert_eq!(
            (refusal.key(), refusal.rule()),
            (Some("records"), "other_agent"),
            "I7.2: a coord_log record names the body's workflow and agent"
        );
        let text = format!(
            r#"{{"op":"coord_log","workflow":"{WORKFLOW}","agent":"{AGENT}","records":[{}]}}"#,
            other.to_json().unwrap()
        );
        assert_eq!(
            CoordLog::parse(text.as_bytes()).unwrap_err().rule(),
            "other_agent"
        );

        for records in [
            vec![
                skipped(1),
                CoordEvent {
                    seq: NonZeroU32::new(2).unwrap(),
                    ..other.clone()
                },
            ],
            vec![
                other.clone(),
                CoordEvent {
                    seq: NonZeroU32::new(2).unwrap(),
                    ..skipped(1)
                },
            ],
        ] {
            let page = History { records, last: 2 };
            let refusal = page.to_json().unwrap_err();
            assert_eq!(
                (refusal.key(), refusal.rule()),
                (Some("records"), "other_agent"),
                "I7.2: a coord_history reply holds the records of one workflow and agent"
            );
            let lines: Vec<String> = page.records.iter().map(|e| e.to_json().unwrap()).collect();
            let text = format!(r#"{{"records":[{}],"last":2}}"#, lines.join(","));
            assert_eq!(
                History::parse(text.as_bytes()).unwrap_err().rule(),
                "other_agent"
            );
        }
    }
}

fn product_value(limits: &str, field: &str) -> u64 {
    let start = limits.find("pub const PRODUCT").expect("Limits::PRODUCT");
    let line = limits[start..]
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(&format!("{field}:")))
        .unwrap_or_else(|| panic!("{field}"));
    line[field.len() + 1..]
        .trim()
        .trim_end_matches(',')
        .replace('_', "")
        .parse()
        .unwrap()
}

#[test]
fn r4_bodies_the_bounds_equal_the_product_limits_of_halvo_coord() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../halvo-coord/src/types/limits.rs");
    let limits = std::fs::read_to_string(file).unwrap();
    assert_eq!(
        product_value(&limits, "request_records"),
        REQUEST_RECORDS as u64
    );
    assert_eq!(
        product_value(&limits, "request_bytes"),
        REQUEST_BYTES as u64
    );
}
