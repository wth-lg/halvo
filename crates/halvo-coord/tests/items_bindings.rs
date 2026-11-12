use halvo_coord::mem::{Config, Ports};
use halvo_coord::ports::Owner;
use halvo_coord::types::{Agent, AgentId, AgentState, Body, Name, Slot, WorkflowId};

const AGENT: &str = "000000000000000000000000000000a1";
const FLOW: &str = "000000000000000000000000000000f1";

fn slot(name: &str, value: &str) -> Slot {
    Slot {
        name: Name::parse(name).unwrap(),
        value: vec![value.to_owned()],
    }
}

/// Two bodies `set_bindings`, the second naming one slot of the first with a new value and one
/// new slot: the memory owner merges them by name, as the agent-setup run's handler does (its
/// CP-36), so that a setup asked again for one parameter keeps the others.
#[test]
fn the_memory_owner_merges_the_slots_of_set_bindings_by_name() {
    let p: Ports<Config> = Ports::new(0, 0);
    let config = Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{FLOW}\",\"agent\":\"{AGENT}\",\"rev\":1,\"category\":\"utility_bill_payment\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"web\",\"playbook\":1}}"
    ))
    .unwrap();
    let (w, a) = (
        WorkflowId::parse(FLOW).unwrap(),
        AgentId::parse(AGENT).unwrap(),
    );
    p.agents.borrow_mut().push(Agent {
        workflow: w.clone(),
        agent: a.clone(),
        config,
        state: AgentState::Approved,
        slots: vec![],
        session: false,
        last: None,
        unknown: vec![],
    });
    let first = Body::SetBindings {
        workflow: w.clone(),
        agent: a.clone(),
        slots: vec![slot("start", "Site1"), slot("payee", "City Power")],
    };
    let second = Body::SetBindings {
        workflow: w.clone(),
        agent: a.clone(),
        slots: vec![
            slot("payee", "City Water"),
            slot("instrument", "Card ending 4242"),
        ],
    };
    assert_eq!(p.send(&first).unwrap(), serde_json::json!({"rev": 2}));
    assert_eq!(p.send(&second).unwrap(), serde_json::json!({"rev": 3}));
    let names: Vec<String> = p.agents().unwrap()[0]
        .slots
        .iter()
        .map(|n| n.as_str().to_owned())
        .collect();
    assert_eq!(names, ["start", "payee", "instrument"]);
    assert_eq!(
        p.slots(&w, &a),
        vec![
            slot("start", "Site1"),
            slot("payee", "City Water"),
            slot("instrument", "Card ending 4242"),
        ]
    );
    let other = AgentId::parse("000000000000000000000000000000a2").unwrap();
    assert!(p.slots(&w, &other).is_empty(), "another agent's slots");
}
