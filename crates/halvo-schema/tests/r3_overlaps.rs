use halvo_schema::executable::body::CoordinatorCall;
use halvo_schema::executable::{Refusal, RefusalCause};
use halvo_schema::request::{self, CoordinatorRole, CoordinatorWrite, Decoded};
use halvo_schema::RequestId;

const W: &str = "30303030303030303030303030303030";
const A: &str = "40404040404040404040404040404040";
const ACCOUNT: &str = r#"{"name":"account_address","value":["me@mail.test"]}"#;
const SENDERS: &str = r#"{"name":"reply_senders","value":["ann@people.test, bob@people.test"]}"#;

fn body(slots: &str) -> String {
    format!(r#"{{"op":"list_overlaps","workflow":"{W}","agent":"{A}","slots":{slots}}}"#)
}

fn expect(result: Result<impl std::fmt::Debug, Refusal>, type_name: &str, rule: &str, key: &str) {
    let refusal = result.expect_err("refused");
    assert_eq!(refusal.cause(), RefusalCause::Malformed, "{refusal}");
    assert_eq!(
        (
            refusal.type_name(),
            refusal.rule(),
            refusal.key().unwrap_or("-")
        ),
        (type_name, rule, key)
    );
}

fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from} in {text}");
    text.replacen(from, to, 1)
}

#[test]
fn the_body_list_overlaps_reads_writes_back_and_travels_in_a_coordinator_write() {
    let slots = [
        "null".to_owned(),
        "[]".to_owned(),
        format!("[{SENDERS}]"),
        format!("[{ACCOUNT},{SENDERS}]"),
    ];
    for slots in slots {
        let text = body(&slots);
        let call =
            CoordinatorCall::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(call.op(), "list_overlaps");
        assert_eq!(call.to_json().unwrap(), text);
        let write = CoordinatorWrite::new(
            &CoordinatorRole::claim(),
            RequestId::from_random([9; 16]),
            call.clone(),
        );
        let Ok(Decoded::Raw(raw)) = request::decode(&write.to_bytes().unwrap()) else {
            panic!("{text}")
        };
        assert_eq!(
            CoordinatorCall::parse(raw.body().get().as_bytes()).unwrap(),
            call
        );
    }
}

#[test]
fn the_body_list_overlaps_names_each_slot_once_and_holds_exactly_its_four_keys() {
    expect(
        CoordinatorCall::parse(body(&format!("[{SENDERS},{ACCOUNT},{SENDERS}]")).as_bytes()),
        "ListOverlaps",
        "repeated_name",
        "slots",
    );
    for key in ["workflow", "agent", "slots"] {
        let value = match key {
            "workflow" => format!(r#","workflow":"{W}""#),
            "agent" => format!(r#","agent":"{A}""#),
            _ => r#","slots":null"#.to_owned(),
        };
        expect(
            CoordinatorCall::parse(swap(&body("null"), &value, "").as_bytes()),
            "ListOverlaps",
            "missing_key",
            key,
        );
    }
    expect(
        CoordinatorCall::parse(
            swap(
                &body("null"),
                r#""slots":null"#,
                r#""slots":null,"origins":[]"#,
            )
            .as_bytes(),
        ),
        "ListOverlaps",
        "unknown_key",
        "-",
    );
    expect(
        CoordinatorCall::parse(
            swap(
                &body("null"),
                &format!(r#""workflow":"{W}""#),
                r#""workflow":null"#,
            )
            .as_bytes(),
        ),
        "WorkflowId",
        "not_a_string",
        "workflow",
    );
    expect(
        CoordinatorCall::parse(
            swap(
                &body("null"),
                &format!(r#""agent":"{A}""#),
                r#""agent":null"#,
            )
            .as_bytes(),
        ),
        "AgentId",
        "not_a_string",
        "agent",
    );
    for slots in ["{}", "0", r#""reply_senders""#] {
        expect(
            CoordinatorCall::parse(body(slots).as_bytes()),
            "list",
            "not_a_list",
            "slots",
        );
    }
    expect(
        CoordinatorCall::parse(body(r#"[{"name":"Reply","value":[]}]"#).as_bytes()),
        "Name",
        "name_form",
        "name",
    );
}
