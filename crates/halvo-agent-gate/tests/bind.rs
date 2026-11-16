use std::path::PathBuf;

use halvo_agent_gate::seed::{
    bind, join_senders, prepare, seed_binding, Binding, Prepared, SeedRefused, SeedTask,
    SendersRefused, StopCause, Stopped,
};
use halvo_agent_gate::GateHost;
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::Way;
use halvo_schema::executable::{
    AgentBindings, AgentRun, CoordinatorCall, RunId, RunnerCall, RunnerReply, WebOrigin,
};
use halvo_schema::{AgentId, ErrorName, SeedCategory};
use serde_json::{json, Value};

const PORT: u16 = 18090;

fn seed_file(n: u32) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/fixture-server/variants/data")
        .join(format!("seed-{n}.json"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn seed_json(n: u32) -> Value {
    serde_json::from_slice(&seed_file(n)).expect("a seed file is JSON")
}

fn with_binding(task: &str, binding: Value) -> Vec<u8> {
    let mut seed = seed_json(1);
    seed["bindings"][task] = binding;
    serde_json::to_vec(&seed).expect("JSON")
}

fn declared(name: &str) -> ErrorName {
    ErrorName::read(name).expect("a declared error name")
}

fn run_id(n: u8) -> RunId {
    RunId::from_random([n; 16])
}

fn bindings_rows(host: &GateHost) -> Vec<AgentBindings> {
    let mut out = Vec::new();
    host.rows().for_each("agent_bindings", &mut |row| {
        out.push(AgentBindings::parse(row.as_bytes()).expect("a stored bindings row"));
    });
    out
}

fn ops(host: &GateHost) -> Vec<&'static str> {
    host.sent().iter().map(|s| s.op).collect()
}

fn expected_slots(task: &str, seed: &Value) -> Vec<(String, String)> {
    seed["bindings"][task]["slots"]
        .as_array()
        .expect("slots")
        .iter()
        .map(|slot| {
            let name = slot["name"].as_str().expect("a name").to_owned();
            let values: Vec<&str> = slot["value"]
                .as_array()
                .expect("a value list")
                .iter()
                .map(|v| v.as_str().expect("a string"))
                .collect();
            let text = match name.as_str() {
                "start" => format!("{}:{PORT}", values[0]),
                "reply_senders" => values.join(", "),
                _ => values[0].to_owned(),
            };
            (name, text)
        })
        .collect()
}

fn expected_origins(task: &str, seed: &Value) -> Vec<String> {
    seed["bindings"][task]["origins"]
        .as_array()
        .expect("origins")
        .iter()
        .map(|o| format!("{}:{PORT}", o.as_str().expect("a string")))
        .collect()
}

fn load(host: &mut GateHost, agent: &AgentId, run: &RunId) -> Result<RunnerReply, ErrorName> {
    let body = json!({"op": "load", "agent": agent.as_str(), "run": run.as_str()});
    host.runner(&RunnerCall::parse(body.to_string().as_bytes()).expect("a load body"))
}

#[test]
fn every_seed_binds_through_set_bindings_and_add_origin_with_the_port() {
    for n in 1..=3 {
        let seed = seed_json(n);
        for task in SeedTask::ALL {
            let name = task.as_str();
            let binding = seed_binding(&seed_file(n), *task, PORT).expect("the seed's binding");
            let mut host = GateHost::new();
            let run = run_id(0x50);
            let prepared = prepare(&mut host, *task, &binding, &run).expect("the gate run binds");
            let slots = expected_slots(name, &seed);
            let origins = expected_origins(name, &seed);

            let mut want_ops = vec!["create_agent"];
            if !slots.is_empty() {
                want_ops.push("set_bindings");
            }
            want_ops.extend(std::iter::repeat_n("add_origin", origins.len()));
            assert_eq!(ops(&host), want_ops, "seed {n} {name}");

            let rows = bindings_rows(&host);
            let newest = rows.iter().max_by_key(|b| b.rev).expect("a bindings row");
            let revisions = usize::from(!slots.is_empty()) + origins.len();
            assert_eq!(rows.len(), revisions, "seed {n} {name}");
            assert_eq!(newest.rev.get() as usize, revisions, "seed {n} {name}");
            assert_eq!(prepared.rev.map(|r| r.get() as usize), Some(revisions));
            assert_eq!(newest.workflow, prepared.workflow);
            assert_eq!(newest.agent, prepared.agent);
            let got_origins: Vec<&str> = newest.origins.iter().map(WebOrigin::as_str).collect();
            assert_eq!(got_origins, origins, "seed {n} {name}");
            let got_slots: Vec<(String, String)> = newest
                .slots
                .iter()
                .map(|s| (s.name.as_str().to_owned(), s.value.joined()))
                .collect();
            assert_eq!(got_slots, slots, "seed {n} {name}");

            let reply = load(&mut host, &prepared.agent, &run).expect("the runner's load");
            let RunnerReply::Load(LoadReply {
                bindings: Some(loaded),
                ..
            }) = reply
            else {
                panic!("a load reply with bindings, seed {n} {name}");
            };
            assert_eq!(loaded.origins, newest.origins);
            assert_eq!(loaded.slots, newest.slots);
        }
    }
}

#[test]
fn a_seed_workflow_is_the_task_s_category_and_approved_with_the_agent() {
    for task in SeedTask::ALL {
        let binding = seed_binding(&seed_file(2), *task, PORT).expect("a binding");
        let mut host = GateHost::new();
        let Prepared {
            workflow, agent, ..
        } = prepare(&mut host, *task, &binding, &run_id(1)).expect("binds");
        let config = host
            .configs()
            .into_iter()
            .find(|c| c.agent == agent)
            .expect("a stored configuration");
        assert_eq!(config.category, task.category());
        assert_eq!(config.workflow, workflow);
    }
    assert_eq!(SeedTask::Bill.category(), SeedCategory::UtilityBillPayment);
    assert_eq!(SeedTask::Order.category(), SeedCategory::ShopOrder);
    assert_eq!(SeedTask::Reply.category(), SeedCategory::Email);
    assert_eq!(SeedTask::Cleanup.category(), SeedCategory::CalendarCleanup);
}

#[test]
fn reply_senders_is_one_text_joined_by_a_comma_and_a_space() {
    let senders = vec!["ada@x.test".to_owned(), "bo@y.test".to_owned()];
    assert_eq!(
        join_senders(&senders),
        Ok("ada@x.test, bo@y.test".to_owned())
    );
    assert_eq!(
        join_senders(&["solo@x.test".to_owned()]),
        Ok("solo@x.test".to_owned())
    );
}

#[test]
fn join_senders_refuses_what_a_reader_of_the_slot_refuses() {
    let list = |items: &[&str]| -> Vec<String> { items.iter().map(|s| (*s).to_owned()).collect() };
    assert_eq!(join_senders(&[]), Err(SendersRefused::Empty));
    assert_eq!(
        join_senders(&list(&["a@x.test", "b,c@y.test"])),
        Err(SendersRefused::Comma)
    );
    assert_eq!(
        join_senders(&list(&["a@x.test", ""])),
        Err(SendersRefused::EmptyAddress)
    );
    assert_eq!(
        join_senders(&list(&["  ", "a@x.test"])),
        Err(SendersRefused::EmptyAddress)
    );
    assert_eq!(
        join_senders(&list(&["A@X.test", "a@x.TEST"])),
        Err(SendersRefused::Twice)
    );
    assert_eq!(
        join_senders(&list(&["a@x.test", " a@x.test "])),
        Err(SendersRefused::Twice)
    );
}

#[test]
fn seed_binding_refuses_a_seed_that_does_not_bind() {
    let cases: Vec<(Vec<u8>, SeedTask, SeedRefused)> = vec![
        (b"{".to_vec(), SeedTask::Bill, SeedRefused::Json),
        (
            with_binding("bill", Value::Null),
            SeedTask::Bill,
            SeedRefused::Json,
        ),
        (
            with_binding(
                "reply",
                json!({"origins": ["http://mail.agent.test"], "slots": [{"name": "reply_senders", "value": []}]}),
            ),
            SeedTask::Reply,
            SeedRefused::Senders(SendersRefused::Empty),
        ),
        (
            with_binding(
                "reply",
                json!({"origins": ["http://mail.agent.test"], "slots": [{"name": "reply_senders", "value": ["a,b@x.test"]}]}),
            ),
            SeedTask::Reply,
            SeedRefused::Senders(SendersRefused::Comma),
        ),
        (
            with_binding(
                "bill",
                json!({"origins": ["http://utility.agent.test:80"], "slots": []}),
            ),
            SeedTask::Bill,
            SeedRefused::Origin,
        ),
        (
            with_binding(
                "bill",
                json!({"origins": ["http://utility.agent.test/pay"], "slots": []}),
            ),
            SeedTask::Bill,
            SeedRefused::Origin,
        ),
        (
            with_binding(
                "bill",
                json!({"origins": ["utility.agent.test"], "slots": []}),
            ),
            SeedTask::Bill,
            SeedRefused::Origin,
        ),
        (
            with_binding(
                "bill",
                json!({"origins": [], "slots": [{"name": "start", "value": ["http://utility.agent.test/bill"]}]}),
            ),
            SeedTask::Bill,
            SeedRefused::Start,
        ),
        (
            with_binding(
                "bill",
                json!({"origins": [], "slots": [{"name": "payee", "value": ["A", "B"]}]}),
            ),
            SeedTask::Bill,
            SeedRefused::Value,
        ),
        (
            with_binding(
                "bill",
                json!({"origins": [], "slots": [{"name": "payee", "value": [""]}]}),
            ),
            SeedTask::Bill,
            SeedRefused::Value,
        ),
        (
            with_binding(
                "bill",
                json!({"origins": [], "slots": [{"name": "Payee", "value": ["A"]}]}),
            ),
            SeedTask::Bill,
            SeedRefused::Name,
        ),
        (
            with_binding("bill", json!({"origins": [], "slots": [], "rev": 1})),
            SeedTask::Bill,
            SeedRefused::Json,
        ),
        (
            with_binding(
                "bill",
                json!({"origins": [], "slots": [{"name": "payee", "value": ["A"], "note": "x"}]}),
            ),
            SeedTask::Bill,
            SeedRefused::Json,
        ),
        (
            with_binding(
                "bill",
                json!({"origins": [], "slots": [{"name": "start", "value": ["http://utility.agent.test", "http://shop.agent.test"]}]}),
            ),
            SeedTask::Bill,
            SeedRefused::Start,
        ),
    ];
    for (i, (seed, task, want)) in cases.into_iter().enumerate() {
        assert_eq!(
            seed_binding(&seed, task, PORT).map(|_| ()),
            Err(want),
            "case {i}"
        );
    }
    assert_eq!(
        seed_binding(&seed_file(1), SeedTask::Bill, 0).map(|_| ()),
        Err(SeedRefused::Origin)
    );
}

#[test]
fn a_gate_run_whose_set_bindings_is_refused_stops_before_any_origin() {
    let seed = with_binding(
        "bill",
        json!({"origins": ["http://utility.agent.test", "http://pay.agent.test"], "slots": [{"name": "workbook", "value": ["relative/book.xlsx"]}]}),
    );
    let binding = seed_binding(&seed, SeedTask::Bill, PORT).expect("the seed reads");
    let mut host = GateHost::new();
    let stopped = prepare(&mut host, SeedTask::Bill, &binding, &run_id(2)).map(|_| ());
    assert_eq!(
        stopped,
        Err(Stopped {
            op: "set_bindings",
            cause: StopCause::Refused(declared("binding")),
        })
    );
    assert_eq!(ops(&host), vec!["create_agent", "set_bindings"]);
    assert!(bindings_rows(&host).is_empty());
}

#[test]
fn a_gate_run_whose_slot_names_repeat_stops_at_set_bindings() {
    let seed = with_binding(
        "reply",
        json!({"origins": ["http://mail.agent.test"], "slots": [{"name": "reply_senders", "value": ["a@x.test"]}, {"name": "reply_senders", "value": ["b@x.test"]}]}),
    );
    let binding = seed_binding(&seed, SeedTask::Reply, PORT).expect("the seed reads");
    let mut host = GateHost::new();
    let stopped = prepare(&mut host, SeedTask::Reply, &binding, &run_id(3)).map(|_| ());
    assert_eq!(
        stopped,
        Err(Stopped {
            op: "set_bindings",
            cause: StopCause::Refused(declared("malformed")),
        })
    );
    assert_eq!(ops(&host), vec!["create_agent", "set_bindings"]);
    assert!(bindings_rows(&host).is_empty());
}

#[test]
fn a_gate_run_whose_first_add_origin_is_refused_sends_no_second() {
    let mut host = GateHost::new();
    let workflow = host.put_seed_workflow(SeedCategory::CalendarCleanup);
    let agent = AgentId::from_random([0x45; 16]);
    let created = host.send(
        &CoordinatorCall::parse(
            json!({"op": "create_agent", "workflow": workflow.as_str(), "agent": agent.as_str()})
                .to_string()
                .as_bytes(),
        )
        .expect("a body"),
    );
    assert!(created.is_ok());
    let stranger = AgentId::from_random([0x46; 16]);
    let binding = Binding {
        origins: vec![
            WebOrigin::parse("http://calendar.agent.test:18090").expect("an origin"),
            WebOrigin::parse("http://mail.agent.test:18090").expect("an origin"),
        ],
        slots: Vec::new(),
    };
    let stopped = bind(&mut host, &workflow, &stranger, &run_id(4), &binding);
    assert_eq!(
        stopped,
        Err(Stopped {
            op: "add_origin",
            cause: StopCause::Refused(declared("binding")),
        })
    );
    assert_eq!(ops(&host), vec!["create_agent", "add_origin"]);
    assert!(bindings_rows(&host).is_empty());
}

#[test]
fn a_bound_origin_bound_again_stores_nothing_and_answers_the_newest() {
    let binding = seed_binding(&seed_file(1), SeedTask::Order, PORT).expect("a binding");
    let mut host = GateHost::new();
    let prepared = prepare(&mut host, SeedTask::Order, &binding, &run_id(5)).expect("binds");
    let again = bind(
        &mut host,
        &prepared.workflow,
        &prepared.agent,
        &run_id(6),
        &Binding {
            origins: binding.origins.clone(),
            slots: Vec::new(),
        },
    );
    assert_eq!(again, Ok(prepared.rev));
    assert_eq!(bindings_rows(&host).len(), 3);
}

#[test]
fn every_runner_body_of_an_ended_run_is_refused() {
    let binding = seed_binding(&seed_file(1), SeedTask::Bill, PORT).expect("a binding");
    let mut host = GateHost::new();
    let run = run_id(7);
    let p = prepare(&mut host, SeedTask::Bill, &binding, &run).expect("binds");
    let (agent, workflow) = (p.agent.as_str().to_owned(), p.workflow.as_str().to_owned());
    let origin = format!("http://utility.agent.test:{PORT}");
    let session = |r: &RunId| json!({"op": "put_session", "agent": agent, "run": r.as_str(), "origin": origin, "rev": 1, "part": 1, "of": 1, "data": ["QUJD"]});
    let log = |r: &RunId| {
        json!({"op": "log", "agent": agent, "run": r.as_str(), "records": [{
            "v": 1, "workflow": workflow, "agent": agent, "run": r.as_str(), "seq": 1, "day": 20_770,
            "at": 1_794_400_000_000_000_000_i64, "event": "step_ok", "step": 1, "cause": null,
            "model": null, "ms": null, "tokens_in": null, "tokens_out": null,
        }]})
    };
    let playbook = |r: &RunId| {
        json!({"op": "put_playbook", "agent": agent, "run": r.as_str(), "number": 1, "steps": [{
            "v": 1, "n": 1, "kind": "web", "verb": "navigate", "target": null,
            "args": [{"name": "url", "from": "slot", "value": null, "key": "start"}],
            "pre": [], "post": [], "effect": "none", "class": "read",
        }]})
    };
    let load_session = |r: &RunId| json!({"op": "load_session", "agent": agent, "run": r.as_str(), "origin": origin, "rev": 1, "part": 1});
    let body = |v: Value| RunnerCall::parse(v.to_string().as_bytes()).expect("a runner body");

    assert!(host.runner(&body(session(&run))).is_ok());
    assert!(host.runner(&body(log(&run))).is_ok());
    assert!(load(&mut host, &p.agent, &run).is_ok());

    let end = json!({"op": "end_run", "workflow": workflow, "agent": agent, "run": run.as_str()});
    let end = CoordinatorCall::parse(end.to_string().as_bytes()).expect("an end_run body");
    assert!(host.send(&end).is_ok());

    let effect = Err(declared("effect"));
    assert_eq!(load(&mut host, &p.agent, &run).map(|_| ()), effect);
    assert_eq!(host.runner(&body(log(&run))).map(|_| ()), effect);
    assert_eq!(host.runner(&body(session(&run))).map(|_| ()), effect);
    assert_eq!(host.runner(&body(playbook(&run))).map(|_| ()), effect);
    assert_eq!(host.runner(&body(load_session(&run))).map(|_| ()), effect);

    let next = run_id(8);
    assert!(load(&mut host, &p.agent, &next).is_ok());
    assert!(host.runner(&body(playbook(&next))).is_ok());
}

#[test]
fn a_stopped_run_names_the_body_and_the_refusal() {
    let stopped = Stopped {
        op: "set_bindings",
        cause: StopCause::Refused(declared("binding")),
    };
    assert_eq!(
        stopped.to_string(),
        "the seed's body set_bindings was refused with binding"
    );
}

#[test]
fn every_add_origin_names_the_way_sign_in_and_the_run_given() {
    let binding = seed_binding(&seed_file(3), SeedTask::Bill, PORT).expect("a binding");
    let mut host = GateHost::new();
    let run = run_id(9);
    let p = prepare(&mut host, SeedTask::Bill, &binding, &run).expect("binds");
    let mut origins = Vec::new();
    for sent in host.sent() {
        match &sent.body {
            CoordinatorCall::AddOrigin(b) => {
                assert_eq!(b.way, Way::SignIn);
                assert_eq!(b.run, run);
                assert_eq!(b.workflow, p.workflow);
                assert_eq!(b.agent, p.agent);
                origins.push(b.origin.clone());
            }
            CoordinatorCall::SetBindings(b) => assert_eq!(b.slots, binding.slots),
            CoordinatorCall::CreateAgent(b) => assert_eq!(b.agent, p.agent),
            other => panic!("a body that the gate run does not send: {}", other.op()),
        }
    }
    assert_eq!(origins, binding.origins);
}

#[test]
fn the_record_of_end_run_holds_the_day_of_the_request() {
    let binding = seed_binding(&seed_file(1), SeedTask::Cleanup, PORT).expect("a binding");
    let mut host = GateHost::new();
    let run = run_id(10);
    let p = prepare(&mut host, SeedTask::Cleanup, &binding, &run).expect("binds");
    let day = |d: std::time::Duration| (d.as_secs() / 86_400) as u32;
    let epoch = std::time::UNIX_EPOCH;
    let before = day(std::time::SystemTime::now()
        .duration_since(epoch)
        .expect("after 1970"));
    let end = json!({"op": "end_run", "workflow": p.workflow.as_str(), "agent": p.agent.as_str(), "run": run.as_str()});
    let end = CoordinatorCall::parse(end.to_string().as_bytes()).expect("an end_run body");
    assert!(host.send(&end).is_ok());
    let after = day(std::time::SystemTime::now()
        .duration_since(epoch)
        .expect("after 1970"));
    let mut days = Vec::new();
    host.rows().for_each("agent_run", &mut |row| {
        days.push(AgentRun::parse(row.as_bytes()).expect("a run record").day);
    });
    assert_eq!(days.len(), 1);
    assert!(
        (before..=after).contains(&days[0]),
        "{days:?} {before} {after}"
    );
}

#[test]
fn a_seed_origin_outside_the_seed_form_is_refused_before_any_body() {
    let hosts = [
        "https://a.test",
        "http://127.1",
        "http://0x7f000001",
        "http://2130706433",
        "http://1.2.3.04",
        "http://256.1.1.1",
        "http://a1.agent.test",
        "https://a.agent.test",
    ];
    let mut cases: Vec<Value> = Vec::new();
    for host in hosts {
        cases.push(json!({"origins": [host], "slots": []}));
        cases.push(json!({"origins": ["http://pay.agent.test", host], "slots": [{"name": "payee", "value": ["P"]}]}));
    }
    cases.push(json!({"origins": ["http://utility.agent.test", "http://127.0.0.1", "http://256.1.1.1"], "slots": [{"name": "range", "value": ["B2:D6"]}]}));
    assert_eq!(cases.len(), 17);
    for case in cases {
        let seed = with_binding("bill", case.clone());
        assert_eq!(
            seed_binding(&seed, SeedTask::Bill, PORT).map(|_| ()),
            Err(SeedRefused::Origin),
            "{case}"
        );
    }
}

#[test]
fn a_start_slot_with_another_scheme_than_http_is_refused() {
    let seed = with_binding(
        "bill",
        json!({"origins": ["http://utility.agent.test"], "slots": [{"name": "start", "value": ["https://utility.agent.test"]}]}),
    );
    assert_eq!(
        seed_binding(&seed, SeedTask::Bill, PORT).map(|_| ()),
        Err(SeedRefused::Start)
    );
}

#[test]
fn join_senders_trims_ascii_spaces_alone_and_folds_ascii_case_alone() {
    let list = |items: &[&str]| -> Vec<String> { items.iter().map(|s| (*s).to_owned()).collect() };
    assert_eq!(
        join_senders(&list(&["\ta@x.test", "a@x.test"])),
        Ok("\ta@x.test, a@x.test".to_owned())
    );
    assert_eq!(
        join_senders(&list(&["\u{e4}@x.test", "\u{c4}@x.test"])),
        Ok("\u{e4}@x.test, \u{c4}@x.test".to_owned())
    );
}
