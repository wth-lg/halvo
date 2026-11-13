mod support;

use halvo_coord::mem::{Call, Method};
use halvo_coord::ports::PortError;
use halvo_coord::types::commands::LIST;
use halvo_coord::types::{Cause, Limits, RunEvent};
use halvo_coord_app::{Core, Hub, Reply, REFUSED};
use serde_json::{json, Value};
use support::{
    agent, app, calls_since, events, fixture, invoke, mark, started, world, Counts, Dialogs,
};

fn hub(agents: usize) -> std::sync::Arc<Hub<support::Shared>> {
    let hub = Hub::new(
        world(agents, Limits::PRODUCT),
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    hub.coordinator().tick();
    hub
}

fn args_of(command: &str) -> Value {
    let item = format!("{:032x}", 0x77);
    match command {
        "agents_snapshot" => json!({}),
        "agents_next" => json!({"after": 0}),
        "agents_card" => json!({"agent": agent(0).to_string()}),
        "agents_answer" => json!({"item": item, "choice": "allow"}),
        "agents_setup_typed" => json!({"item": item, "param": "payee", "text": "x"}),
        "agents_present" => json!({"item": item}),
        "agents_control" => json!({"agent": agent(0).to_string(), "action": "run_now"}),
        other => panic!("no arguments for {other}"),
    }
}

fn changes(calls: &[Call]) -> Vec<Call> {
    calls
        .iter()
        .filter(|c| {
            !matches!(
                c,
                Call::Workflows
                    | Call::Agents
                    | Call::History(..)
                    | Call::Attention(_)
                    | Call::Changed
            )
        })
        .cloned()
        .collect()
}

#[test]
fn every_command_of_the_list_answers_the_window_main_and_no_other() {
    let hub = hub(1);
    let (_app, main, other) = app(hub.clone());
    let seq = hub.coordinator().snapshot().seq;
    let m = mark(&hub);
    for command in LIST.iter().map(|c| c.name) {
        assert_eq!(
            invoke(&other, command, args_of(command)),
            Err(json!(REFUSED)),
            "{command} from the window other"
        );
    }
    assert!(
        calls_since(&hub, m).is_empty(),
        "the window other reached no port"
    );
    assert_eq!(hub.coordinator().snapshot().seq, seq);
    for command in LIST.iter().map(|c| c.name) {
        let reply = invoke(&main, command, args_of(command));
        assert!(reply.is_ok(), "{command} from the window main: {reply:?}");
    }
    assert_eq!(started(&hub).len(), 1, "run_now from main started its run");
}

#[test]
fn a_name_outside_the_list_is_left_to_the_next_handler() {
    let (_app, main, _other) = app(hub(1));
    let reply = invoke(&main, "agents_ping", json!({}));
    assert_ne!(reply, Err(json!(REFUSED)));
    assert!(reply.is_err(), "{reply:?}");
}

#[test]
fn each_refusal_is_written_as_its_fixture_and_never_rejects() {
    for cause in Cause::ALL {
        assert_eq!(
            serde_json::to_string(&Reply::Error(*cause)).unwrap(),
            fixture(&format!("reply-error-{}", cause.name())),
            "{cause}"
        );
    }
    let hub = hub(1);
    let (_app, main, _other) = app(hub.clone());
    let unknown = format!("{:032x}", 0x77);
    let cases = [
        (
            "agents_answer",
            json!({"item": unknown, "choice": "allow"}),
            Cause::UnknownItem,
        ),
        (
            "agents_present",
            json!({"item": unknown}),
            Cause::UnknownItem,
        ),
        (
            "agents_card",
            json!({"agent": unknown}),
            Cause::UnknownAgent,
        ),
        (
            "agents_control",
            json!({"agent": unknown, "action": "stop"}),
            Cause::UnknownAgent,
        ),
        (
            "agents_card",
            json!({"agent": "not an id"}),
            Cause::Malformed,
        ),
    ];
    for (command, args, cause) in cases {
        assert_eq!(
            invoke(&main, command, args.clone()),
            Ok(fixture(&format!("reply-error-{}", cause.name()))),
            "{command} {args}"
        );
    }
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.refuse(Method::AgentHistory, PortError::Busy));
    assert_eq!(
        invoke(&main, "agents_card", json!({"agent": agent(0).to_string()})),
        Ok(fixture("reply-error-busy"))
    );
    let fresh = Hub::new(
        world(1, Limits::PRODUCT),
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    let (_app, main, _other) = app(fresh);
    assert_eq!(
        invoke(&main, "agents_card", json!({"agent": agent(0).to_string()})),
        Ok(fixture("reply-error-no_service")),
        "before the first tick the coordinator has no board"
    );
}

#[test]
fn malformed_arguments_answer_malformed() {
    let hub = hub(1);
    let (_app, main, _other) = app(hub.clone());
    let item = format!("{:032x}", 0x77);
    let a = agent(0).to_string();
    let cases = [
        ("agents_card", json!({})),
        ("agents_card", json!({"agent": a, "extra": 1})),
        ("agents_card", json!({"agent": 7})),
        ("agents_card", json!([a])),
        ("agents_answer", json!({"item": item, "choice": "present"})),
        ("agents_answer", json!({"item": item, "choice": "choose"})),
        (
            "agents_answer",
            json!({"item": item, "choice": "choose", "param": "payee"}),
        ),
        (
            "agents_answer",
            json!({"item": item, "choice": "allow", "param": "payee", "n": 1}),
        ),
        (
            "agents_answer",
            json!({"item": item, "choice": "choose", "param": "colour", "n": 1}),
        ),
        (
            "agents_setup_typed",
            json!({"item": item, "param": "payee"}),
        ),
        (
            "agents_setup_typed",
            json!({"item": item, "param": "payee", "text": 3}),
        ),
        ("agents_control", json!({"agent": a, "action": "delete"})),
        ("agents_control", json!({"agent": a})),
    ];
    let m = mark(&hub);
    for (command, args) in cases {
        assert_eq!(
            invoke(&main, command, args.clone()),
            Ok(fixture("reply-error-malformed")),
            "{command} {args}"
        );
    }
    assert!(changes(&calls_since(&hub, m)).is_empty());
    assert_eq!(
        serde_json::to_string(&Core::call(&*hub, "agents_card", None)).unwrap(),
        fixture("reply-error-malformed"),
        "a body that is no JSON"
    );
}

#[test]
fn the_snapshot_of_an_empty_world_is_the_fixture_of_the_state_empty() {
    let (_app, main, _other) = app(hub(0));
    let reply = invoke(&main, "agents_snapshot", json!({})).unwrap();
    assert_eq!(
        fixture("state-empty"),
        format!("{{\"snapshot\":{reply},\"card\":null}}")
    );
    let reply = invoke(&main, "agents_snapshot", Value::Null).unwrap();
    assert_eq!(
        fixture("state-empty"),
        format!("{{\"snapshot\":{reply},\"card\":null}}")
    );
}

fn shape(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), shape(v))).collect())
        }
        Value::Array(list) => Value::Array(list.first().map(shape).into_iter().collect()),
        _ => Value::Null,
    }
}

#[test]
fn a_row_and_a_card_have_the_form_of_their_fixtures() {
    let hub = hub(1);
    let (_app, main, _other) = app(hub.clone());
    let state: Value =
        serde_json::from_str(&invoke(&main, "agents_snapshot", json!({})).unwrap()).unwrap();
    let ten: Value = serde_json::from_str(&fixture("state-ten")).unwrap();
    assert_eq!(state["rows"].as_array().map(Vec::len), Some(1));
    assert_eq!(shape(&state["rows"][0]), shape(&ten["snapshot"]["rows"][0]));
    let card: Value = serde_json::from_str(
        &invoke(&main, "agents_card", json!({"agent": agent(0).to_string()})).unwrap(),
    )
    .unwrap();
    let fixture_card: Value = serde_json::from_str(&fixture("state-card")).unwrap();
    let keys = |v: &Value| v.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>());
    assert_eq!(keys(&card), keys(&fixture_card["card"]));
    assert_eq!(
        card["runs"],
        json!([]),
        "the memory owner holds no past run of the agent"
    );
}

#[test]
fn a_stop_for_an_agent_with_no_live_run_answers_ok_and_changes_nothing() {
    let hub = hub(1);
    let (_app, main, _other) = app(hub.clone());
    let stop = json!({"agent": agent(0).to_string(), "action": "stop"});
    let before = (hub.coordinator().snapshot(), events(&hub));
    let m = mark(&hub);
    assert_eq!(
        invoke(&main, "agents_control", stop.clone()),
        Ok(fixture("reply-ok"))
    );
    assert_eq!(
        invoke(&main, "agents_control", stop.clone()),
        Ok(fixture("reply-ok"))
    );
    assert_eq!(changes(&calls_since(&hub, m)), []);
    assert_eq!((hub.coordinator().snapshot(), events(&hub)), before);

    let run_now = json!({"agent": agent(0).to_string(), "action": "run_now"});
    assert_eq!(
        invoke(&main, "agents_control", run_now),
        Ok(fixture("reply-ok"))
    );
    let run = started(&hub).pop().unwrap();
    hub.coordinator().on_run(&run, RunEvent::Hello).unwrap();
    assert_eq!(
        invoke(&main, "agents_control", stop.clone()),
        Ok(fixture("reply-ok"))
    );
    let before = (hub.coordinator().snapshot(), events(&hub));
    let m = mark(&hub);
    assert_eq!(
        invoke(&main, "agents_control", stop),
        Ok(fixture("reply-ok")),
        "already stopping"
    );
    assert_eq!(changes(&calls_since(&hub, m)), []);
    assert_eq!((hub.coordinator().snapshot(), events(&hub)), before);
}

#[test]
fn equal_calls_of_a_double_press_each_reach_the_core() {
    let hub = hub(1);
    let (_app, main, _other) = app(hub.clone());
    let run_now = json!({"agent": agent(0).to_string(), "action": "run_now"});
    assert_eq!(
        invoke(&main, "agents_control", run_now.clone()),
        Ok(fixture("reply-ok"))
    );
    assert_eq!(
        invoke(&main, "agents_control", run_now),
        Ok(fixture("reply-error-state")),
        "the second press meets a live run"
    );
    assert_eq!(started(&hub).len(), 1);
    let run = started(&hub).pop().unwrap();
    hub.coordinator().on_run(&run, RunEvent::Hello).unwrap();
    let stop = json!({"agent": agent(0).to_string(), "action": "stop"});
    let m = mark(&hub);
    assert_eq!(
        invoke(&main, "agents_control", stop.clone()),
        Ok(fixture("reply-ok"))
    );
    assert_eq!(
        invoke(&main, "agents_control", stop),
        Ok(fixture("reply-ok"))
    );
    let cancels = calls_since(&hub, m)
        .iter()
        .filter(|c| matches!(c, Call::Cancel(_)))
        .count();
    assert_eq!(cancels, 1, "one cancel for two presses");
}

#[test]
fn a_call_whose_arguments_have_the_shape_of_the_list_reaches_the_core() {
    let hub = hub(1);
    let (_app, main, _other) = app(hub.clone());
    let item = format!("{:032x}", 0x77);
    let cases = [
        (
            "agents_answer",
            json!({"item": item, "choice": "choose", "param": "payee", "n": 0}),
        ),
        ("agents_answer", json!({"item": item, "choice": "skip"})),
        (
            "agents_setup_typed",
            json!({"item": item, "param": "payee", "text": "City Power"}),
        ),
    ];
    for (command, args) in cases {
        let reply = invoke(&main, command, args.clone()).unwrap();
        assert_ne!(reply, fixture("reply-error-malformed"), "{command} {args}");
        let cause: Value = serde_json::from_str(&reply).unwrap();
        let errors = LIST.iter().find(|c| c.name == command).unwrap().errors;
        assert!(
            errors.iter().any(|e| cause["error"] == json!(e.name())),
            "{command} {args}: {reply}"
        );
    }
}

#[test]
fn a_body_that_is_no_json_answers_malformed_through_the_handler() {
    let (_app, main, _other) = app(hub(1));
    assert_eq!(
        support::invoke_body(
            &main,
            "agents_card",
            tauri::ipc::InvokeBody::Raw(b"{}".to_vec())
        ),
        Ok(fixture("reply-error-malformed"))
    );
}

#[test]
fn agents_snapshot_and_agents_next_are_never_refused_and_answer_the_whole_state() {
    let hub = hub(1);
    let (_app, main, _other) = app(hub.clone());
    let state = serde_json::to_string(&hub.coordinator().snapshot()).unwrap();
    let cases = [
        ("agents_snapshot", json!({"after": 1})),
        ("agents_snapshot", json!([1])),
        ("agents_next", json!({})),
        ("agents_next", json!({"after": -1})),
        ("agents_next", json!({"after": 4_294_967_296_u64})),
        ("agents_next", json!({"after": "1"})),
        ("agents_next", json!({"after": 1, "extra": 1})),
    ];
    for (command, args) in cases {
        assert_eq!(
            invoke(&main, command, args.clone()),
            Ok(state.clone()),
            "{command} {args}"
        );
    }
    for command in ["agents_snapshot", "agents_next"] {
        assert_eq!(
            support::invoke_body(&main, command, tauri::ipc::InvokeBody::Raw(b"{}".to_vec())),
            Ok(state.clone()),
            "{command} with a body that is no JSON"
        );
    }
}
