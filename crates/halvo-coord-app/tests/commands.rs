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
    Core::tick(&*hub);
    hub
}

fn args_of(command: &str) -> Value {
    let item = format!("{:032x}", 0x77);
    match command {
        "agents_snapshot" => json!({}),
        "agents_next" => json!({"after": 0}),
        "agents_card" => json!({"agent": agent(0).to_string()}),
        "agents_answer" => json!({"item": item, "step": 0, "choice": "allow"}),
        "agents_setup_typed" => {
            json!({"item": item, "step": 0, "param": "payee", "text": "x", "more": "no"})
        }
        "agents_choose_file" => json!({"item": item, "step": 0, "param": "workbook"}),
        "agents_present" => json!({"item": item}),
        "agents_control" => json!({"agent": agent(0).to_string(), "action": "run_now"}),
        "agents_shown" => json!({"item": item, "step": 0}),
        "agents_delete" => json!({"agent": agent(0).to_string()}),
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
            json!({"item": unknown, "step": 0, "choice": "allow"}),
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
    let card: serde_json::Value = serde_json::from_str(
        &invoke(&main, "agents_card", json!({"agent": agent(0).to_string()})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (card["history"].clone(), card["runs"].clone()),
        (json!("busy"), json!([])),
        "a busy history read gives the card with the key history busy and no run"
    );
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.refuse(Method::Agents, PortError::Busy));
    assert_eq!(
        invoke(
            &main,
            "agents_control",
            json!({"agent": agent(0).to_string(), "action": "run_now"})
        ),
        Ok(fixture("reply-error-busy")),
        "a start whose roster read answers busy is written as the fixture of busy"
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
        ("agents_answer", json!({"item": item, "choice": "allow"})),
        (
            "agents_setup_typed",
            json!({"item": item, "param": "payee", "text": "x", "more": "no"}),
        ),
        (
            "agents_answer",
            json!({"item": item, "step": 0, "choice": "present"}),
        ),
        (
            "agents_answer",
            json!({"item": item, "step": 0, "choice": "choose", "more": "no"}),
        ),
        (
            "agents_answer",
            json!({"item": item, "step": 0, "choice": "choose", "param": "payee", "more": "no"}),
        ),
        (
            "agents_answer",
            json!({"item": item, "step": 0, "choice": "choose", "param": "payee", "n": 1}),
        ),
        (
            "agents_answer",
            json!({"item": item, "step": 0, "choice": "allow", "param": "payee", "n": 1}),
        ),
        (
            "agents_answer",
            json!({"item": item, "step": 0, "choice": "choose", "param": "colour", "n": 1, "more": "no"}),
        ),
        (
            "agents_setup_typed",
            json!({"item": item, "step": 0, "param": "payee", "more": "no"}),
        ),
        (
            "agents_setup_typed",
            json!({"item": item, "step": 0, "param": "payee", "text": 3, "more": "no"}),
        ),
        (
            "agents_setup_typed",
            json!({"item": item, "step": 0, "param": "payee", "text": "x"}),
        ),
        ("agents_control", json!({"agent": a, "action": "delete"})),
        ("agents_control", json!({"agent": a})),
        ("agents_shown", json!({"step": 0})),
        ("agents_shown", json!({"item": item})),
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
fn agents_delete_from_main_reaches_the_core_and_the_agents_row_leaves() {
    let hub = hub(2);
    let (_app, main, other) = app(hub.clone());
    let delete = json!({"agent": agent(0).to_string()});
    assert_eq!(
        invoke(&other, "agents_delete", delete.clone()),
        Err(json!(REFUSED))
    );
    assert!(hub
        .coordinator()
        .snapshot()
        .rows
        .iter()
        .any(|r| r.agent == agent(0)));
    assert_eq!(
        invoke(&main, "agents_delete", delete.clone()),
        Ok(fixture("reply-ok"))
    );
    let rows = hub.coordinator().snapshot().rows;
    assert!(!rows.iter().any(|r| r.agent == agent(0)), "{rows:?}");
    assert!(rows.iter().any(|r| r.agent == agent(1)));
    assert_eq!(
        Core::call(&*hub, "agents_delete", Some(&delete)),
        Reply::Error(Cause::UnknownAgent)
    );
    assert_eq!(
        Core::call(&*hub, "agents_delete", Some(&json!({"agent": 7}))),
        Reply::Error(Cause::Malformed)
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
            json!({"item": item, "step": 0, "choice": "choose", "param": "payee", "n": 0, "more": "no"}),
        ),
        (
            "agents_answer",
            json!({"item": item, "step": 0, "choice": "choose", "param": "payee", "n": 0, "more": "yes"}),
        ),
        (
            "agents_answer",
            json!({"item": item, "step": 0, "choice": "skip"}),
        ),
        (
            "agents_setup_typed",
            json!({"item": item, "step": 0, "param": "payee", "text": "City Power", "more": "no"}),
        ),
        (
            "agents_setup_typed",
            json!({"item": item, "step": 0, "param": "payee", "text": "City Power", "more": "yes"}),
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

#[test]
fn the_card_is_the_coordinators_card_byte_for_byte() {
    let hub = hub(2);
    let (_app, main, _other) = app(hub.clone());
    for n in 0..2 {
        let card = hub.coordinator().card(&agent(n)).unwrap();
        assert_eq!(
            invoke(&main, "agents_card", json!({"agent": agent(n).to_string()})),
            Ok(serde_json::to_string(&card).unwrap())
        );
    }
}

#[test]
fn a_name_outside_the_list_answers_malformed_in_the_core() {
    let hub = hub(1);
    for name in ["agents_ping", "agents_delete", "", "AGENTS_SNAPSHOT"] {
        assert_eq!(
            Core::call(&*hub, name, Some(&json!({}))),
            Reply::Error(Cause::Malformed),
            "{name:?}"
        );
    }
}

#[test]
fn every_name_of_the_list_with_arguments_that_fit_is_answered_other_than_malformed() {
    let hub = hub(1);
    let (tx, rx) = std::sync::mpsc::channel();
    let core = hub.clone();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        for command in LIST.iter() {
            let choice = command
                .args
                .iter()
                .find(|a| a.name == "choice")
                .and_then(|a| a.values)
                .and_then(|v| v.first().copied());
            let mut map = serde_json::Map::new();
            for arg in command.args {
                if arg.when.is_some_and(|c| choice != Some(c.name())) {
                    continue;
                }
                let value = match arg.kind {
                    halvo_coord::types::ArgType::Count => json!(0),
                    halvo_coord::types::ArgType::Id if arg.name == "agent" => {
                        json!(agent(0).to_string())
                    }
                    halvo_coord::types::ArgType::Id => json!(format!("{:032x}", 0x77)),
                    halvo_coord::types::ArgType::Text => json!("x"),
                    halvo_coord::types::ArgType::Name => {
                        json!(arg.values.and_then(|v| v.first().copied()).unwrap_or("x"))
                    }
                };
                map.insert(arg.name.to_string(), value);
            }
            let args = Value::Object(map);
            let reply = Core::call(&*core, command.name, Some(&args));
            out.push((command.name, reply));
        }
        let _ = tx.send(out);
    });
    let out = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("the calls answered inside 20 s");
    assert_eq!(out.len(), LIST.len());
    let malformed: Vec<&str> = out
        .iter()
        .filter(|(_, reply)| *reply == Reply::Error(Cause::Malformed))
        .map(|(name, _)| *name)
        .collect();
    assert!(
        malformed.is_empty(),
        "a name of LIST with arguments that fit falls to malformed: {malformed:?}"
    );
}

fn stand_in() -> String {
    format!(
        r#"{{"seq":0,"rows":[],"items":[],"note":{}}}"#,
        serde_json::to_string(&halvo_coord::words::text("note.no_service", &[]).unwrap()).unwrap()
    )
}

fn app_managing_no_agents(
    next: u64,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri::WebviewWindow<tauri::test::MockRuntime>,
    tauri::WebviewWindow<tauri::test::MockRuntime>,
) {
    let app = tauri::test::mock_builder()
        .invoke_handler(halvo_coord_app::handler_with(halvo_coord_app::Waits {
            next: std::time::Duration::from_millis(next),
            coalesce: std::time::Duration::from_millis(100),
        }))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let other = tauri::WebviewWindowBuilder::new(&app, "other", Default::default())
        .build()
        .unwrap();
    (app, main, other)
}

#[test]
fn the_seq_of_a_new_coordinator_is_at_least_1() {
    let fresh = Hub::new(
        world(1, Limits::PRODUCT),
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    assert!(fresh.coordinator().snapshot().seq >= 1);
    Core::tick(&*fresh);
    assert!(fresh.coordinator().snapshot().seq >= 1);
}

#[test]
fn before_agents_is_managed_another_window_is_refused_and_main_gets_the_stand_in_or_a_cause() {
    let (_app, main, other) = app_managing_no_agents(1_000);
    for command in LIST.iter().map(|c| c.name) {
        assert_eq!(
            invoke(&other, command, args_of(command)),
            Err(json!(REFUSED)),
            "{command} from the window other"
        );
    }
    assert_eq!(invoke(&main, "agents_snapshot", json!({})), Ok(stand_in()));
    let causes = [
        ("agents_card", "no_service"),
        ("agents_answer", "no_service"),
        ("agents_setup_typed", "no_service"),
        ("agents_present", "no_service"),
        ("agents_control", "no_service"),
        ("agents_shown", "no_service"),
    ];
    for (command, cause) in causes {
        assert_eq!(
            invoke(&main, command, args_of(command)),
            Ok(fixture(&format!("reply-error-{cause}"))),
            "{command}"
        );
    }
}

#[test]
fn before_agents_is_managed_a_next_is_held_and_answers_the_stand_in_at_its_wait() {
    let (_app, main, _other) = app_managing_no_agents(2_000);
    let start = std::time::Instant::now();
    let waiting = std::thread::spawn(move || invoke(&main, "agents_next", json!({"after": 1})));
    std::thread::sleep(std::time::Duration::from_millis(1_000));
    assert!(!waiting.is_finished(), "no answer inside a second");
    assert_eq!(waiting.join().unwrap(), Ok(stand_in()));
    let took = start.elapsed();
    assert!(
        took >= std::time::Duration::from_millis(1_900)
            && took < std::time::Duration::from_millis(3_000),
        "the stand-in after the handler's wait of 2 s: {took:?}"
    );
}

#[test]
fn a_held_next_is_handed_to_the_hub_once_the_app_manages_agents() {
    let (app, main, _other) = app_managing_no_agents(10_000);
    let start = std::time::Instant::now();
    let waiting = std::thread::spawn(move || invoke(&main, "agents_next", json!({"after": 0})));
    std::thread::sleep(std::time::Duration::from_millis(300));
    let hub = hub(1);
    tauri::Manager::manage(&app, halvo_coord_app::Agents(hub.clone()));
    let reply = waiting.join().unwrap();
    assert_eq!(
        reply,
        Ok(serde_json::to_string(&hub.coordinator().snapshot()).unwrap())
    );
    assert!(start.elapsed() < std::time::Duration::from_millis(5_000));
}

#[test]
fn before_agents_is_managed_each_command_answers_a_cause_of_its_own_row() {
    let (_app, main, _other) = app_managing_no_agents(1_000);
    for command in LIST
        .iter()
        .filter(|c| c.name != "agents_snapshot" && c.name != "agents_next")
    {
        let reply = invoke(&main, command.name, args_of(command.name)).unwrap();
        let cause: Value = serde_json::from_str(&reply).unwrap();
        assert!(
            command
                .errors
                .iter()
                .any(|e| cause["error"] == json!(e.name())),
            "{}: {reply} is no cause of its row",
            command.name
        );
    }
}

#[test]
fn before_its_first_tick_the_hub_hands_every_other_command_to_the_coordinator() {
    let fresh = Hub::new(
        world(1, Limits::PRODUCT),
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    let c = fresh.coordinator();
    let snapshot =
        serde_json::to_string(&Core::call(&*fresh, "agents_snapshot", Some(&json!({})))).unwrap();
    assert_eq!(snapshot, stand_in());
    let item = halvo_coord::types::ItemId::parse(&format!("{:032x}", 0x77)).unwrap();
    let own: [(&str, Reply); 6] = [
        (
            "agents_card",
            c.card(&agent(0)).map_or_else(Reply::Error, Reply::Card),
        ),
        (
            "agents_control",
            c.control(&agent(0), halvo_coord::types::Action::RunNow)
                .into(),
        ),
        (
            "agents_answer",
            c.answer(&item, halvo_coord::types::Choice::Allow, None, 0)
                .into(),
        ),
        ("agents_present", c.present(&item).into()),
        ("agents_shown", c.shown_on_page(&item, 0).into()),
        (
            "agents_setup_typed",
            c.setup_typed(&item, halvo_coord::types::Param::Payee, "x", 0)
                .into(),
        ),
    ];
    for (command, reply) in own {
        assert_eq!(
            Core::call(&*fresh, command, Some(&args_of(command))),
            reply,
            "{command} goes to the coordinator before the first tick"
        );
    }
    assert_eq!(
        serde_json::to_string(&Core::call(&*fresh, "agents_snapshot", Some(&json!({})))).unwrap(),
        stand_in()
    );
}

#[test]
fn the_day_is_the_day_of_one_read_of_now_and_follows_the_memory_clock() {
    use halvo_coord::ports::Clock;
    let hub = hub(1);
    let ports = hub.coordinator().ports();
    assert_ne!(
        u64::from(support::DAY),
        support::T0 / 86_400_000,
        "the world's day is not the UTC day of its instant"
    );
    assert_eq!(ports.day(support::T0), Ok(support::DAY));
    assert_eq!(ports.today(), Ok(support::DAY));
    ports.inner().with(|p| p.set_now(support::T0 + 86_400_000));
    assert_eq!(
        ports.today(),
        Ok(support::DAY + 1),
        "the day follows the instant"
    );
}

#[test]
fn the_day_of_an_instant_on_another_day_than_the_clocks_is_the_day_of_that_instant() {
    use halvo_coord::ports::Clock;
    const ONE_DAY: u64 = 86_400_000;
    let hub = hub(1);
    let wired = hub.coordinator().ports();
    let shared = wired.inner();
    let (t0, day) = (support::T0, support::DAY);
    for (who, clock) in [
        ("Wired", wired as &dyn Clock),
        ("Shared", shared as &dyn Clock),
    ] {
        assert_eq!(
            clock.day(t0 + ONE_DAY),
            Ok(day + 1),
            "{who}: the day after now"
        );
        assert_eq!(
            clock.day(t0 + 3 * ONE_DAY),
            Ok(day + 3),
            "{who}: three days after now"
        );
    }
    shared.with(|p| p.set_now(t0 + 3 * ONE_DAY));
    for (who, clock) in [
        ("Wired", wired as &dyn Clock),
        ("Shared", shared as &dyn Clock),
    ] {
        assert_eq!(clock.day(t0), Ok(day), "{who}: a day before now");
        assert_eq!(clock.today(), Ok(day + 3), "{who}: today follows now");
    }
}

/// The sentence that names `other` with the shared sender, as the core holds it for the page.
fn names_other() -> Option<halvo_coord::ports::Shared> {
    Some(halvo_coord::ports::Shared::Named(
        halvo_coord::ports::Overlaps {
            entries: vec![halvo_coord::ports::Overlap {
                workflow: support::other_flow(),
                agent: support::other(),
                senders: vec![support::ANN.to_owned()],
            }],
            more: false,
        },
    ))
}

fn overlap_reads(hub: &Hub<support::Shared>) -> usize {
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.overlap_reads.borrow().len())
}

/// The coordinator's row of 2026-11-21T07:03:55Z (round RD-160 of U2.4g), through the app's
/// real wiring, `Hub` over `Wired`: a set-up answer that the owner refuses with `overlap` asks
/// the owner's read `list_overlaps`, and the item holds the sentence that names the other agent
/// with the shared sender (I3.5 (a) to (c)).
#[test]
fn through_the_app_a_set_up_answer_refused_with_overlap_names_the_agent_that_shares_the_sender() {
    let hub = Hub::new(
        support::sharing_set_up(),
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    Core::tick(&*hub);
    let (_app, main, _other) = app(hub.clone());
    let setup = hub
        .coordinator()
        .snapshot()
        .items
        .iter()
        .find(|i| i.kind() == halvo_coord::types::ItemKind::Setup)
        .map(|i| (i.item().clone(), i.step()))
        .expect("the set-up item of the asking agent");
    let reads = overlap_reads(&hub);
    let reply = invoke(
        &main,
        "agents_setup_typed",
        json!({"item": setup.0.to_string(), "step": setup.1, "param": "reply_senders", "text": support::ANN, "more": "no"}),
    );
    println!("setup_typed answered: {reply:?}");
    assert_eq!(overlap_reads(&hub), reads + 1, "the read reached the owner");
    assert_eq!(hub.coordinator().setup_overlap(&setup.0), names_other());
}

/// The same row: through the app's real wiring, a Switch on and then a Start now of an agent
/// held by `failures`, whose `active` the owner refuses with `overlap`, each ask the owner's read
/// once, and the core holds the sentence that names the other agent with the shared sender
/// (I3.5 (f), RD-160 (3)).
#[test]
fn through_the_app_a_switch_on_and_a_start_refused_with_overlap_name_the_agent_that_shares_the_sender(
) {
    let hub = Hub::new(
        support::sharing_held(),
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    Core::tick(&*hub);
    let (_app, main, _other) = app(hub.clone());
    for action in ["switch_on", "run_now"] {
        let reads = overlap_reads(&hub);
        let reply = invoke(
            &main,
            "agents_control",
            json!({"agent": support::asking().to_string(), "action": action}),
        );
        println!("{action} answered: {reply:?}");
        assert_eq!(
            overlap_reads(&hub),
            reads + 1,
            "{action}: the read reached the owner"
        );
        assert_eq!(
            hub.coordinator().switch_on_overlap(&support::asking()),
            names_other(),
            "{action}"
        );
    }
}

/// The same row: the line `off.kept` that the unit adds (I5.4, RD-67 1.2), as the page reads it
/// through the app's real wiring: an agent listed `paused` with no hold by its events shows the
/// status `off`, the words of `line.off.kept` and the controls of the hold `off`.
#[test]
fn through_the_app_an_agent_listed_paused_with_no_hold_by_its_events_reads_the_line_off_kept() {
    let shared = world(1, Limits::PRODUCT);
    shared.with(|p| {
        for w in p.workflows.borrow_mut().iter_mut() {
            if let halvo_coord::types::Workflow::Full(f) = w {
                f.state = halvo_coord::types::WorkflowState::Paused;
            }
        }
    });
    let hub = Hub::new(
        shared,
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    Core::tick(&*hub);
    let (_app, main, _other) = app(hub.clone());
    let state: Value =
        serde_json::from_str(&invoke(&main, "agents_snapshot", json!({})).unwrap()).unwrap();
    let words: Value = serde_json::from_str(&fixture("words")).unwrap();
    let row = &state["rows"][0];
    assert_eq!(row["status"], "off");
    assert_eq!(row["line"], words["line.off.kept"]);
    assert_eq!(
        row["controls"]
            .as_array()
            .map(|c| c.iter().map(|x| x["control"].clone()).collect::<Vec<_>>()),
        Some(vec![json!("switch_on"), json!("delete")])
    );
}

fn hold_set(e: &halvo_coord::types::Event) -> bool {
    matches!(e.what, halvo_coord::types::What::HoldSet { .. })
}

/// U3.12 (A), term (5) of RD-161 R1 with RD-155 (1): a Switch off that stops the live run while
/// the owner refuses to store its hold under class B answers, through the hub, its cause beside
/// the sentence that says both halves, and the row's line no longer says it.
#[test]
fn u312_a_a_switch_off_that_could_not_be_saved_answers_both_halves_beside_its_cause() {
    let hub = hub(1);
    let (_app, main, _other) = app(hub.clone());
    let control = |action: &str| {
        invoke(
            &main,
            "agents_control",
            json!({"agent": agent(0).to_string(), "action": action}),
        )
    };
    assert_eq!(control("run_now"), Ok(fixture("reply-ok")));
    let run = started(&hub).last().cloned().unwrap();
    assert_eq!(hub.coordinator().on_run(&run, RunEvent::Hello), Ok(()));
    let class_b = PortError::Refused(halvo_coord::types::Name::parse("no_workflow").unwrap());
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.refuse_record(hold_set, class_b));
    let sentence = halvo_coord::words::text("refused.switch_not_saved", &[]).unwrap();
    let reply: Value = serde_json::from_str(&control("switch_off").unwrap()).unwrap();
    assert_eq!(
        reply,
        json!({"error": "no_service", "text": sentence}),
        "the answer at the press"
    );
    let row = hub
        .coordinator()
        .snapshot()
        .rows
        .into_iter()
        .find(|r| r.agent == agent(0))
        .unwrap();
    assert!(
        row.controls
            .iter()
            .any(|c| c.control == halvo_coord::types::Control::SwitchOff),
        "the row shows the agent on"
    );
    assert_ne!(row.line, sentence, "one fact, one place");
}

/// r4 U3.13, the second sentence (RD-161 R4, WAY A; I3.5 (f)): through the app's real wiring, a
/// Switch on whose `active` the owner refuses with `overlap` answers the thirteenth cause,
/// `overlap`, and not `no_service`, the cause of a lost call, with the sentence beside the
/// control that names the other agent and the senders that its read gave; where the read did not
/// answer, the sentence of `refused.overlap`, which names nobody.
#[test]
fn u313_a_switch_on_refused_with_overlap_answers_overlap_with_the_sentence_of_its_read() {
    let hub = Hub::new(
        support::sharing_held(),
        Limits::PRODUCT,
        Counts::default().sink(),
        Dialogs::default().boxed(),
    );
    Core::tick(&*hub);
    let (_app, main, _other) = app(hub.clone());
    let switch_on = || -> Value {
        let reply = invoke(
            &main,
            "agents_control",
            json!({"agent": support::asking().to_string(), "action": "switch_on"}),
        );
        serde_json::from_str(&reply.unwrap()).unwrap()
    };
    let name = hub
        .coordinator()
        .snapshot()
        .rows
        .iter()
        .find(|r| r.agent == support::other())
        .map(|r| r.name.clone())
        .expect("the row of the other agent");
    let reply = switch_on();
    assert_eq!(reply["error"], "overlap", "{reply}");
    let text = reply["text"].as_str().unwrap_or_default().to_owned();
    assert!(
        text.contains(&name) && text.contains(support::ANN),
        "{text}"
    );
    assert_eq!(
        Ok(text),
        halvo_coord::words::text(
            "refused.overlap_named",
            &[("name", &name), ("senders", support::ANN)]
        )
    );
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.refuse(Method::Overlaps, PortError::Busy));
    assert_eq!(
        switch_on(),
        json!({
            "error": "overlap",
            "text": halvo_coord::words::text("refused.overlap", &[]).unwrap()
        }),
        "the read did not answer"
    );
}
