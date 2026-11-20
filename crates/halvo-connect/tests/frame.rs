use halvo_connect::reply_senders;
use halvo_schema::executable::frame::FRAME_JSON_MAX;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::FromRunner;
use serde_json::{json, Value};

const REPLIES: usize = 10;
const ADDRESS: usize = 320;
const BODY: usize = 4_096;

fn address() -> String {
    let domain = "people.test";
    format!("{}@{domain}", "a".repeat(ADDRESS - 1 - domain.len()))
}

fn field(name: &str, from: &str, value: &str) -> Value {
    json!({"name": name, "from": from, "origin": null, "value": OwnValue::new(value)})
}

fn confirmation(subject: &str) -> String {
    let to = address();
    let body = "b".repeat(BODY);
    let effects: Vec<Value> = (0..REPLIES)
        .map(|n| {
            json!({
                "effect": format!("{n:032x}"),
                "class": "send",
                "digest": "d".repeat(64),
                "bound": {"class": "send", "fields": [
                    field("to", "extract", &to),
                    field("subject", "extract", subject),
                    field("body", "model", &body),
                ]},
                "new_origin": null,
            })
        })
        .collect();
    json!({"t": "confirm", "request": {
        "v": 1,
        "run": "5".repeat(32),
        "agent": "4".repeat(32),
        "cut": "yes",
        "effects": effects,
    }})
    .to_string()
}

fn fits(subject: &str) -> bool {
    let frame = confirmation(subject);
    match FromRunner::parse(frame.as_bytes()) {
        Ok(FromRunner::Confirm(confirm)) => {
            assert_eq!(confirm.request.effects.len(), REPLIES);
            assert!(frame.len() <= FRAME_JSON_MAX);
            true
        }
        Ok(_) => panic!("another frame"),
        Err(_) => {
            assert!(frame.len() > FRAME_JSON_MAX, "refused for its length alone");
            false
        }
    }
}

#[test]
fn ten_replies_with_every_field_at_its_bound_fit_one_confirmation_frame() {
    assert_eq!(address().len(), ADDRESS);
    assert!(
        reply_senders(&address()).is_ok(),
        "a sender that the rule takes"
    );
    assert!(
        fits(&"s".repeat(1_024)),
        "a subject of one whole own_text piece"
    );
    let room = (1_700..FRAME_JSON_MAX)
        .take_while(|n| fits(&"s".repeat(*n)))
        .last()
        .expect("room");
    assert!(!fits(&"s".repeat(room + 1)));
    assert_eq!(
        room, 1_741,
        "the longest subject of plain bytes that ten replies hold"
    );
}
