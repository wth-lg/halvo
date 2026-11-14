use halvo_coord::types::commands::LIST;
use halvo_coord::types::{Action, Choice, Param};
use serde_json::{json, Value};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ui/tests/fixtures/coord/commands.json"
);

const STATES: [&str; 21] = [
    "empty",
    "ten",
    "needs_confirm_page",
    "needs_confirm_native",
    "needs_sign_in",
    "needs_link_account",
    "needs_stuck",
    "needs_check",
    "needs_setup",
    "needs_ready",
    "needs_show",
    "setup_three",
    "card",
    "problem",
    "off",
    "working",
    "queue",
    "no_service",
    "needs_sign_in_bind",
    "needs_confirm_mixed",
    "needs_confirm_changed",
];

fn arg(name: &str, kind: &str, when: Option<&str>, values: Option<Vec<&str>>) -> Value {
    json!({"name": name, "type": kind, "when": when, "values": values})
}

fn i8_1() -> Value {
    let states: Vec<String> = STATES.iter().map(|s| format!("state-{s}")).collect();
    let mut next = states.clone();
    next.push("reply-next-same".to_owned());
    next.push("reply-closed".to_owned());
    let choices = vec![
        "reject",
        "allow",
        "decline",
        "done",
        "try_again",
        "went_through",
        "none",
        "choose",
        "skip",
        "start",
    ];
    let params = vec![
        "start",
        "payee",
        "instrument",
        "reply_senders",
        "workbook",
        "sheet",
        "range",
        "source",
        "seller",
    ];
    json!([
        {"command": "agents_snapshot", "args": [], "replies": states, "errors": []},
        {"command": "agents_next", "args": [arg("after", "count", None, None)], "replies": next, "errors": []},
        {"command": "agents_card", "args": [arg("agent", "id", None, None)], "replies": ["state-card"],
         "errors": ["unknown_agent", "no_service", "busy", "malformed"]},
        {"command": "agents_answer", "args": [
            arg("item", "id", None, None),
            arg("step", "count", None, None),
            arg("choice", "name", None, Some(choices)),
            arg("param", "name", Some("choose"), Some(params.clone())),
            arg("n", "count", Some("choose"), None)],
         "replies": ["reply-ok"],
         "errors": ["stale", "presenter", "early", "hold", "state", "budget", "queue_full", "unknown_item", "no_service", "busy", "malformed"]},
        {"command": "agents_setup_typed", "args": [
            arg("item", "id", None, None),
            arg("step", "count", None, None),
            arg("param", "name", None, Some(params)),
            arg("text", "text", None, None)],
         "replies": ["reply-ok"], "errors": ["stale", "state", "unknown_item", "malformed", "no_service"]},
        {"command": "agents_present", "args": [arg("item", "id", None, None)], "replies": ["reply-ok"],
         "errors": ["stale", "presenter", "unknown_item", "no_service", "malformed"]},
        {"command": "agents_control", "args": [
            arg("agent", "id", None, None),
            arg("action", "name", None, Some(vec!["run_now", "stop", "switch_off", "switch_on"]))],
         "replies": ["reply-ok"],
         "errors": ["hold", "state", "budget", "queue_full", "unknown_agent", "no_service", "busy", "malformed"]},
        {"command": "agents_shown", "args": [
            arg("item", "id", None, None),
            arg("step", "count", None, None)],
         "replies": ["reply-ok"],
         "errors": ["unknown_item", "presenter", "stale", "state", "no_service", "malformed"]}
    ])
}

#[test]
fn commands_same() {
    let tracked = std::fs::read_to_string(FIXTURE).expect("commands.json is tracked");
    let written = halvo_coord::types::commands::json();
    assert!(
        tracked == written,
        "ui/tests/fixtures/coord/commands.json differs from LIST; write the fixtures again with `cargo run -p halvo-coord --bin coord-words -- fixtures ui/tests/fixtures/coord`"
    );
    let file: Value = serde_json::from_str(&tracked).expect("JSON");
    assert_eq!(file, i8_1(), "commands.json is not the list of I8.1");
    let list: Value = serde_json::to_value(LIST).unwrap();
    assert_eq!(list, i8_1(), "LIST is not the list of I8.1");
}

#[test]
fn commands_json_keeps_the_key_order_of_i8_3_on_one_line() {
    let text = halvo_coord::types::commands::json();
    assert!(text.starts_with("[{\"command\":\"agents_snapshot\",\"args\":[],\"replies\":["));
    assert!(text.contains("{\"name\":\"after\",\"type\":\"count\",\"when\":null,\"values\":null}"));
    assert!(text.ends_with("]\n") && text.matches('\n').count() == 1);
}

#[test]
fn the_answer_commands_require_the_step_and_answer_one_without_it_malformed() {
    use halvo_coord::types::{ArgType, Cause};
    for name in ["agents_answer", "agents_setup_typed"] {
        let row = LIST.iter().find(|c| c.name == name).unwrap();
        let step = row.args.iter().find(|a| a.name == "step");
        assert!(
            step.is_some_and(|a| a.kind == ArgType::Count && a.when.is_none()),
            "{name}: {step:?}"
        );
        assert!(row.errors.contains(&Cause::Malformed), "{name}");
    }
}

#[test]
fn the_closed_lists_of_the_arguments_are_the_types_lists() {
    let answer = &LIST[3];
    let choice = answer.args.iter().find(|a| a.name == "choice").unwrap();
    let answerable: Vec<&str> = Choice::ALL
        .iter()
        .filter(|c| **c != Choice::Present)
        .map(|c| c.name())
        .collect();
    assert_eq!(choice.values.unwrap().to_vec(), answerable);
    let param = answer.args.iter().find(|a| a.name == "param").unwrap();
    let params: Vec<&str> = Param::ALL.iter().map(|p| p.name()).collect();
    assert_eq!(param.values.unwrap().to_vec(), params);
    let action = LIST[6].args.iter().find(|a| a.name == "action").unwrap();
    let actions: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
    assert_eq!(action.values.unwrap().to_vec(), actions);
}
