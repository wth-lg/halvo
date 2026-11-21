use halvo_schema::executable::{RunId, RunnerReply};
use serde_json::{json, Value};

use super::coordinator::edit_row;
use super::host::{active_world, agent, compiled, locator, run, wf, with_locators, World, DAY};
use crate::LearnHost;

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

fn pay() -> Value {
    locator("role", "button", Some("Pay"))
}

fn laid(list: Vec<Value>) -> Value {
    let mut locators = list;
    locators.push(pay());
    with_locators(compiled()[1].clone(), "button Pay now", locators)
}

fn store(w: &mut World, rows: Vec<halvo_schema::TypedRecord>) {
    w.host.commit(rows).expect("the rows are stored");
}

#[test]
fn load_steps_lays_the_provisional_edit_else_the_newest_active_one_on_its_step_alone() {
    let mut w = active_world();
    assert_eq!(w.replay(2), compiled());
    store(
        &mut w,
        vec![edit_row(
            1,
            1,
            1,
            2,
            "active",
            None,
            &[11, 12, 13],
            (DAY - 9, DAY - 2),
            2,
            vec![loc("A")],
        )],
    );
    let served = w.replay(3);
    assert_eq!(
        served,
        vec![
            compiled()[0].clone(),
            laid(vec![loc("A")]),
            compiled()[2].clone()
        ]
    );
    store(
        &mut w,
        vec![edit_row(
            2,
            1,
            1,
            2,
            "active",
            Some(1),
            &[14, 15, 16],
            (DAY - 1, DAY - 1),
            2,
            vec![loc("B"), loc("A")],
        )],
    );
    assert_eq!(w.replay(4)[1], laid(vec![loc("B"), loc("A")]));
    store(
        &mut w,
        vec![edit_row(
            3,
            1,
            1,
            2,
            "provisional",
            Some(2),
            &[17],
            (DAY, DAY),
            0,
            vec![loc("C"), loc("B"), loc("A")],
        )],
    );
    assert_eq!(w.replay(5)[1], laid(vec![loc("C"), loc("B"), loc("A")]));
    store(
        &mut w,
        vec![edit_row(
            3,
            2,
            1,
            2,
            "reverted",
            Some(2),
            &[17],
            (DAY, DAY),
            0,
            vec![loc("C"), loc("B"), loc("A")],
        )],
    );
    let served = w.replay(6);
    assert_eq!(served[1], laid(vec![loc("B"), loc("A")]));
    assert_eq!(served[0], compiled()[0]);
    assert_eq!(served[2], compiled()[2]);
}

#[test]
fn load_steps_lays_no_edit_of_a_lower_compile_number_and_none_on_a_run_whose_number_is_not_the_newest(
) {
    let mut w = active_world();
    store(
        &mut w,
        vec![edit_row(
            1,
            1,
            1,
            2,
            "provisional",
            None,
            &[11],
            (DAY, DAY),
            0,
            vec![loc("A")],
        )],
    );
    w.start(2, "replay");
    w.load(2);
    assert_eq!(w.load_steps(2, 1, 1)[1], laid(vec![loc("A")]));
    w.direct_compile(30, 2);
    assert_eq!(w.load_steps(2, 1, 1), compiled());
    assert_eq!(w.load_steps(2, 1, 2), compiled()[1..].to_vec());
    w.start(3, "replay");
    let RunnerReply::Load(load) = w.load(3) else {
        panic!("a load reply");
    };
    assert_eq!(load.playbook.map(|p| p.number.get()), Some(2));
    assert_eq!(w.load_steps(3, 2, 1), compiled());
}

#[test]
fn load_steps_of_a_run_that_the_handlers_did_not_see_load_lays_no_edit() {
    let mut w = active_world();
    store(
        &mut w,
        vec![edit_row(
            1,
            1,
            1,
            2,
            "provisional",
            None,
            &[11],
            (DAY, DAY),
            0,
            vec![loc("A")],
        )],
    );
    w.start(2, "replay");
    let now = w.host.agent_now();
    let body = json!({"op": "load", "agent": agent(1), "run": run(2)});
    let reply = halvo_agent_store::runner(&mut w.host.agent, now, body.to_string().as_bytes());
    assert!(reply.is_ok());
    assert_eq!(w.load_steps(2, 1, 1), compiled());
}

/// R4 (L4.2, LM-90's form): a run whose facts go at a refused `learn_run` row of its workflow keeps its start
/// remembered with no row, so after that row leaves by its 93-day bound a new `load` of the run makes no facts and
/// its steps carry no learned target.
#[test]
fn a_dropped_run_is_served_no_learned_target_after_its_refused_row_leaves() {
    let mut w = active_world();
    w.learn(2, 2, "button Pay now", vec![loc("A"), pay()]);
    w.start(3, "replay");
    w.load(3);
    assert_eq!(w.load_steps(3, 1, 1)[1], laid(vec![loc("A")]));
    let refused = format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":3,"day":{},"mode":"replay","state":"started","cause":null,"base":null,"assisted":0,"kept":0,"failed_step":null}}"#,
        wf(1),
        agent(1),
        DAY - 92
    );
    w.host.put_raw("learn_run", &refused);
    assert_eq!(w.load_steps(3, 1, 1), compiled());
    w.host.day = DAY + 1;
    w.load(3);
    assert_eq!(w.load_steps(3, 1, 1), compiled());
    let id = RunId::parse(&run(3)).expect("a run id");
    assert!(!w.handlers.facts.contains_key(&id));
    assert!(w.handlers.starts.get(&id).is_some_and(|start| !start.row));
}
