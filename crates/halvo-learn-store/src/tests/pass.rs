use halvo_agent_store::MemoryStore;
use halvo_schema::RowRead;
use serde_json::{json, Value};

use super::host::{agent, compiled, run, wf, workflow, TestHost};
use crate::{CoordinatorAnswer, LearnHandlers, LearnHost, Refused};

const KINDS: [&str; 8] = [
    "agent_config",
    "agent_run",
    "agent_playbook",
    "agent_bindings",
    "agent_effect",
    "agent_log",
    "agent_session",
    "agent_checkpoint",
];

fn agent_rows(store: &MemoryStore) -> Vec<String> {
    let mut out = Vec::new();
    for kind in KINDS {
        store.for_each(kind, &mut |row| out.push(format!("{kind} {row}")));
    }
    out
}

fn bodies() -> Vec<(&'static str, Value)> {
    let record = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(1), "day": 20_771,
        "mode": "compile", "outcome": "completed", "playbook": null, "steps": 3, "prompts": 0,
        "handbacks": 0, "confirmations": 0, "effects": 0, "unknown": 0, "model_calls": 0,
        "tokens_in": 0, "tokens_out": 0, "wall_s": 9, "model": null,
    });
    let line = json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(1), "seq": 1, "day": 20_771,
        "at": 1_794_500_000_000_000_000_i64, "event": "step_ok", "step": 1, "cause": null,
        "model": null, "ms": null, "tokens_in": null, "tokens_out": null,
    });
    vec![
        (
            "c",
            json!({"op": "create_agent", "workflow": wf(1), "agent": agent(1)}),
        ),
        (
            "c",
            json!({"op": "set_bindings", "workflow": wf(1), "agent": agent(1), "slots": []}),
        ),
        (
            "c",
            json!({"op": "start_run", "workflow": wf(1), "agent": agent(1), "run": run(1), "mode": "compile"}),
        ),
        (
            "c",
            json!({"op": "add_origin", "workflow": wf(1), "agent": agent(1), "run": run(1), "origin": "https://northwind-power.test:443", "way": "navigate"}),
        ),
        ("c", json!({"op": "no_such_op", "workflow": wf(1)})),
        ("c", json!([1, 2])),
        ("r", json!({"op": "load", "agent": agent(1), "run": run(1)})),
        (
            "r",
            json!({"op": "put_playbook", "agent": agent(1), "run": run(1), "number": 1, "steps": compiled()}),
        ),
        (
            "r",
            json!({"op": "load_steps", "agent": agent(1), "run": run(1), "number": 1, "from": 1}),
        ),
        (
            "r",
            json!({"op": "load_session", "agent": agent(1), "run": run(1), "origin": "https://northwind-power.test:443", "rev": 1, "part": 1}),
        ),
        (
            "r",
            json!({"op": "log", "agent": agent(1), "run": run(1), "records": [line]}),
        ),
        (
            "r",
            json!({"op": "finish", "agent": agent(1), "run": run(1), "record": record}),
        ),
        (
            "r",
            json!({"op": "finish", "agent": agent(1), "run": run(1), "record": record}),
        ),
        (
            "c",
            json!({"op": "set_state", "workflow": wf(1), "agent": agent(1), "state": "active"}),
        ),
        (
            "c",
            json!({"op": "end_run", "workflow": wf(1), "agent": agent(1), "run": run(2)}),
        ),
        ("r", json!({"op": "load", "agent": agent(1), "run": run(2)})),
        ("r", json!({"op": "no_such_op"})),
        ("r", json!("text")),
    ]
}

#[test]
fn every_request_of_the_agent_store_gets_its_reply_and_its_rows() {
    let mut plain = MemoryStore::new();
    plain.put_workflow(workflow(&wf(1), "utility_bill_payment", "approved", None));
    let mut host = TestHost::new();
    host.put_workflow(workflow(&wf(1), "utility_bill_payment", "approved", None));
    let mut handlers = LearnHandlers::new();
    for (i, (role, body)) in bodies().into_iter().enumerate() {
        let bytes = body.to_string();
        let now = host.agent_now();
        if role == "c" {
            let want = halvo_agent_store::coordinator(&mut plain, now, bytes.as_bytes());
            let got = handlers.coordinator(&mut host, bytes.as_bytes());
            let got = got.map(|answer| match answer {
                CoordinatorAnswer::Agent(reply) => reply,
                other => panic!("body {i}: the answer of the learning read {other:?}"),
            });
            assert_eq!(got, want.map_err(Refused::Error), "body {i}");
        } else {
            let want = halvo_agent_store::runner(&mut plain, now, bytes.as_bytes());
            let got = handlers.runner(&mut host, bytes.as_bytes());
            assert_eq!(got, want.map_err(Refused::Error), "body {i}");
        }
        assert_eq!(
            agent_rows(&host.agent.store),
            agent_rows(&plain),
            "body {i}"
        );
    }
}
