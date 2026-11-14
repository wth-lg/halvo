use halvo_schema::executable::RunnerReply;
use serde_json::{json, Value};

use super::coordinator::edit_row;
use super::host::{active_world, agent, compiled, locator, run, with_locators, World, DAY};
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

fn direct_compile(w: &mut World, r: u32, number: u32) {
    let now = w.host.agent_now();
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": number, "steps": compiled()});
    let reply = halvo_agent_store::runner(&mut w.host.agent, now, body.to_string().as_bytes());
    assert!(
        reply.is_ok(),
        "the agent store stores number {number}: {reply:?}"
    );
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
    direct_compile(&mut w, 30, 2);
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
