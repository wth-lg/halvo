use std::num::NonZeroU32;

use halvo_schema::coordinator::body::{CoordHistory, CoordLog};
use halvo_schema::coordinator::names::BindResult;
use halvo_schema::coordinator::record::{
    Answered, BindAnswer, CoordEvent, ItemId, SignInAnswer, What,
};
use halvo_schema::coordinator::REQUEST_BYTES;
use halvo_schema::executable::WebOrigin;
use halvo_schema::record::Instant;
use halvo_schema::{AgentId, WorkflowId};

const WORKFLOW: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
const AGENT: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f809";

fn skipped(seq: u32) -> CoordEvent {
    CoordEvent {
        workflow: WorkflowId::parse(WORKFLOW).unwrap(),
        agent: AgentId::parse(AGENT).unwrap(),
        seq: NonZeroU32::new(seq).unwrap(),
        day: 20767,
        at: Instant(1_794_339_000_000_000_000),
        what: What::SlotSkipped { slot: Some(20766) },
    }
}

fn host(len: usize) -> String {
    (0..len)
        .map(|i| {
            if (i + 1) % 63 == 0 && i + 1 < len {
                '.'
            } else {
                'a'
            }
        })
        .collect()
}

fn bound(seq: u32, host_len: usize) -> CoordEvent {
    CoordEvent {
        what: What::ItemAnswered {
            item: ItemId::from_random([7; 16]),
            answered: Answered::SignIn(SignInAnswer::Second(BindAnswer {
                answer: BindResult::Allow,
                origin: WebOrigin::parse(&format!("https://{}:443", host(host_len))).unwrap(),
            })),
        },
        ..skipped(seq)
    }
}

fn log_text() -> String {
    CoordLog {
        workflow: WorkflowId::parse(WORKFLOW).unwrap(),
        agent: AgentId::parse(AGENT).unwrap(),
        records: vec![skipped(1)],
    }
    .to_json()
    .unwrap()
}

fn history_text() -> String {
    format!(r#"{{"op":"coord_history","workflow":"{WORKFLOW}","agent":"{AGENT}","after":6}}"#)
}

fn inside(text: &str, len: usize) -> String {
    let mut out = text.to_owned();
    out.insert_str(1, &" ".repeat(len - text.len()));
    out
}

fn before(text: &str, len: usize) -> String {
    format!("{}{text}", " ".repeat(len - text.len()))
}

fn after(text: &str, len: usize) -> String {
    format!("{text}{}", "\n".repeat(len - text.len()))
}

fn around(text: &str, len: usize) -> String {
    let pad = len - text.len();
    format!(
        "{}{text}{}",
        " ".repeat(pad / 2),
        "\t".repeat(pad - pad / 2)
    )
}

fn no_json_first(text: &str, len: usize) -> String {
    let mut out = after(text, len);
    out.replace_range(0..1, "x");
    out
}

fn broken_last(text: &str, len: usize) -> String {
    let mut out = after(text, len - 1);
    out.push('{');
    out
}

type Parse = fn(&[u8]) -> Result<(), &'static str>;

fn log_parse(bytes: &[u8]) -> Result<(), &'static str> {
    CoordLog::parse(bytes).map(|_| ()).map_err(|r| r.rule())
}

fn history_parse(bytes: &[u8]) -> Result<(), &'static str> {
    CoordHistory::parse(bytes).map(|_| ()).map_err(|r| r.rule())
}

fn the_bound_on_the_input(name: &str, text: &str, parse: Parse) {
    assert!(text.len() < REQUEST_BYTES, "{name}");
    for (how, input) in [
        ("white space inside the value", inside(text, REQUEST_BYTES)),
        ("white space outside the value", around(text, REQUEST_BYTES)),
    ] {
        assert_eq!(input.len(), REQUEST_BYTES);
        assert_eq!(parse(input.as_bytes()), Ok(()), "{name}: 4096 bytes, {how}");
    }
    let one_over = REQUEST_BYTES + 1;
    for (how, input) in [
        ("white space before the value", before(text, one_over)),
        ("white space after the value", after(text, one_over)),
        ("white space inside the value", inside(text, one_over)),
        ("no JSON at the first byte", no_json_first(text, 5000)),
        ("broken at the last byte", broken_last(text, 5000)),
        ("1 MiB", after(text, 1 << 20)),
    ] {
        assert_eq!(
            parse(input.as_bytes()),
            Err("bytes"),
            "I7.2 and the limits table: a body of this run holds at most request_bytes bytes as it arrives, \
             counted before it is parsed; {name}: {} bytes, {how}",
            input.len()
        );
    }
}

#[test]
fn r4_bounds_coord_log_counts_the_bytes_of_the_input_before_it_parses_them() {
    the_bound_on_the_input("coord_log", &log_text(), log_parse);
}

#[test]
fn r4_bounds_coord_history_counts_the_bytes_of_the_input_before_it_parses_them() {
    the_bound_on_the_input("coord_history", &history_text(), history_parse);
}

#[test]
fn r4_bounds_the_compact_coord_history_has_at_most_130_bytes() {
    let longest = CoordHistory {
        workflow: WorkflowId::parse(WORKFLOW).unwrap(),
        agent: AgentId::parse(AGENT).unwrap(),
        after: Some(u32::MAX),
    };
    assert_eq!(longest.to_json().unwrap().len(), 130);
    let first = CoordHistory {
        after: None,
        ..longest
    };
    assert!(first.to_json().unwrap().len() < 130);
}

fn body_of(host_len: usize, last_host: usize) -> CoordLog {
    let mut records: Vec<CoordEvent> = (1..=7).map(|seq| bound(seq, host_len)).collect();
    records.push(bound(8, last_host));
    CoordLog {
        workflow: WorkflowId::parse(WORKFLOW).unwrap(),
        agent: AgentId::parse(AGENT).unwrap(),
        records,
    }
}

#[test]
fn r4_bounds_the_writer_writes_a_compact_body_of_4096_bytes_and_refuses_4097() {
    let mut found = None;
    for host_len in (1..=253).rev() {
        let Ok(text) = body_of(host_len, 1).to_json() else {
            continue;
        };
        let need = REQUEST_BYTES - text.len() + 1;
        if need < 253 {
            found = Some((host_len, need));
            break;
        }
    }
    let (host_len, need) = found.expect("a body that reaches the bound");
    let at = body_of(host_len, need);
    let text = at.to_json().unwrap();
    assert_eq!(text.len(), REQUEST_BYTES);
    assert_eq!(serde_json::to_string(&at).unwrap(), text);
    assert!(CoordLog::parse(text.as_bytes()).is_ok());

    let over = body_of(host_len, need + 1);
    let refusal = over.to_json().unwrap_err();
    assert_eq!(
        (refusal.key(), refusal.rule()),
        (None, "bytes"),
        "I7.2: the writer refuses a body whose compact text has 4097 bytes"
    );
    assert!(serde_json::to_string(&over).is_err());
}
