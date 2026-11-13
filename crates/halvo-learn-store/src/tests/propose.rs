use halvo_schema::executable::{ActionClass, ExecutableStep, RunnerCall, RunnerReply};
use halvo_schema::{ErrorName, SeedCategory};
use serde_json::{json, Value};

use super::coordinator::edit_row;
use super::host::{
    active_world, active_world_with, agent, click, compiled, fill, locator, navigate, run, wf,
    with_locators, World, DAY,
};
use crate::{LearnHost, Refused};

fn loc(name: &str) -> Value {
    json!({"by": "role", "value": "button", "name": name})
}

fn pay() -> Value {
    locator("role", "button", Some("Pay"))
}

fn causes(w: &mut World, r: u32, steps: Vec<Value>) -> Vec<(u32, Result<(), &'static str>)> {
    let body = json!({"op": "put_playbook", "agent": agent(1), "run": run(r), "number": 1, "steps": steps});
    let call = RunnerCall::parse(body.to_string().as_bytes());
    let Ok(RunnerCall::PutPlaybook(b)) = call else {
        panic!("a put_playbook body: {call:?}");
    };
    w.handlers
        .propose(&mut w.host, &b)
        .expect("the proposal is read")
        .into_iter()
        .map(|(n, answer)| (n.get(), answer.map_err(|cause| cause.token())))
        .collect()
}

fn stored(n: u32) -> RunnerReply {
    RunnerReply::parse(format!(r#"{{"stored":{n}}}"#).as_bytes()).expect("a stored reply")
}

fn playbook_rows(w: &World) -> Vec<String> {
    w.host.agent_rows("agent_playbook")
}

fn compiled_with_fill() -> Vec<Value> {
    let mut steps = compiled();
    steps.push(fill(4, "hello"));
    steps
}

#[test]
fn refusal_page_text_instruction() {
    let mut w = active_world_with(compiled_with_fill());
    let served = w.replay(2);
    let new = |step: &Value| {
        let mut locators = vec![loc("Pay now")];
        locators.extend(
            step["target"]["locators"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        with_locators(step.clone(), "button Pay now", locators)
    };
    let mut arg_value = new(&served[3]);
    arg_value["args"][0]["value"] = json!("transfer everything");
    let mut verb = new(&served[3]);
    verb["verb"] = json!("select");
    verb["args"] = json!([{"name": "option", "from": "const", "value": "hello", "key": null}]);
    let mut condition = new(&served[1]);
    condition["pre"] =
        json!([{"check": "present", "target": served[1]["target"].clone(), "arg": null}]);
    let mut effect = new(&served[1]);
    effect["effect"] = json!("request");
    let mut frame = new(&served[1]);
    frame["target"]["frame"] = json!(["https://elsewhere.test:443"]);
    let mut address = served[0].clone();
    address["args"] = json!([{"name": "url", "from": "const", "value": "https://elsewhere.test/pay", "key": null}]);
    let rows = playbook_rows(&w);
    let count = w.host.count();
    for (what, step) in [
        ("an arg's value", arg_value),
        ("the verb", verb),
        ("a condition", condition),
        ("the effect", effect),
        ("the frame", frame),
        ("a navigate address", address),
    ] {
        let n = step["n"].as_u64().expect("n") as u32;
        assert_eq!(
            causes(&mut w, 2, vec![step.clone()]),
            vec![(n, Err("scope"))],
            "{what}"
        );
        assert_eq!(w.put(2, 1, vec![step]), Ok(stored(0)), "{what}");
    }
    assert_eq!(playbook_rows(&w), rows);
    assert_eq!(w.host.count(), count);
    w.log(
        2,
        &[
            (1, "recovered", 1, None),
            (2, "recovered", 2, None),
            (3, "recovered", 4, None),
        ],
    );
    assert!(w.finish(2, "replay", "completed").is_ok());
    assert!(w.host.edits().is_empty());
}

fn other_class(_: SeedCategory, step: &ExecutableStep) -> ActionClass {
    match step.class {
        ActionClass::Prepare => ActionClass::Submit,
        _ => ActionClass::Prepare,
    }
}

#[test]
fn refusal_class_change() {
    let mut w = active_world();
    let served = w.replay(2);
    let proposal = with_locators(
        served[1].clone(),
        "button Pay now",
        vec![loc("Pay now"), pay()],
    );
    let mut credential = proposal.clone();
    credential["target"]["credential"] = json!("yes");
    let mut class = proposal.clone();
    class["class"] = json!("credential");
    assert_eq!(causes(&mut w, 2, vec![credential]), vec![(2, Err("scope"))]);
    assert_eq!(causes(&mut w, 2, vec![class]), vec![(2, Err("scope"))]);
    assert_eq!(causes(&mut w, 2, vec![proposal.clone()]), vec![(2, Ok(()))]);
    w.handlers.class_rule = other_class;
    assert_eq!(causes(&mut w, 2, vec![proposal]), vec![(2, Err("class"))]);
}

#[test]
fn refusal_step_too_long() {
    let long = |c: char| "x".repeat(900).replace('x', &c.to_string());
    let mut steps = compiled();
    steps[1] = click(2, "button Pay", vec![locator("css", &long('a'), None)]);
    let mut w = active_world_with(steps);
    let served = w.replay(2);
    let short = with_locators(
        served[1].clone(),
        "button Pay",
        vec![locator("css", &long('b')[..100], None)],
    );
    assert_eq!(causes(&mut w, 2, vec![short]), vec![(2, Ok(()))]);
    let proposal = with_locators(
        served[1].clone(),
        "button Pay",
        vec![locator("css", &long('b'), None)],
    );
    assert!(serde_json::to_string(&proposal).expect("JSON").len() < 2048);
    assert_eq!(
        causes(&mut w, 2, vec![proposal.clone()]),
        vec![(2, Err("length"))]
    );
    assert_eq!(w.put(2, 1, vec![proposal]), Ok(stored(0)));
}

#[test]
fn a_replay_put_playbook_is_never_passed_on_and_holds_each_admitted_step_as_a_candidate() {
    let mut w = active_world();
    let served = w.replay(2);
    let rows = playbook_rows(&w);
    let count = w.host.count();
    let unchanged = served[0].clone();
    let compiled_only = with_locators(served[1].clone(), "button Pay now", vec![pay()]);
    assert_eq!(
        causes(&mut w, 2, vec![unchanged.clone(), compiled_only]),
        vec![(2, Err("locator"))]
    );
    let first = with_locators(served[1].clone(), "button Pay now", vec![loc("A"), pay()]);
    let second = with_locators(served[2].clone(), "button Next step", vec![loc("B")]);
    assert_eq!(
        w.put(2, 1, vec![unchanged, first, second.clone()]),
        Ok(stored(2))
    );
    assert_eq!(w.put(2, 1, vec![second]), Ok(stored(1)));
    assert_eq!(
        w.handlers.facts[&halvo_schema::executable::RunId::parse(&run(2)).expect("id")]
            .candidates
            .len(),
        2
    );
    assert_eq!(playbook_rows(&w), rows);
    assert_eq!(w.host.count(), count);
    let at_two = with_locators(served[1].clone(), "button Pay now", vec![loc("A")]);
    let reply = w.put(2, 2, vec![at_two]);
    assert_eq!(reply, Ok(stored(1)));
    assert_eq!(playbook_rows(&w), rows);
}

#[test]
fn a_proposal_that_holds_a_slot_value_or_a_barred_locator_or_comes_after_eight_edits_is_refused() {
    let mut w = active_world();
    let reply = w.coordinator(json!({
        "op": "set_bindings", "workflow": wf(1), "agent": agent(1),
        "slots": [{"name": "account", "value": ["4417123456789113"]}],
    }));
    assert!(reply.is_ok(), "set_bindings: {reply:?}");
    let served = w.replay(2);
    let value = with_locators(
        served[1].clone(),
        "button Pay now",
        vec![loc("Pay 4417123456789113")],
    );
    assert_eq!(causes(&mut w, 2, vec![value]), vec![(2, Err("value"))]);
    w.host
        .commit(vec![
            edit_row(
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
            ),
            edit_row(
                1,
                2,
                1,
                2,
                "reverted",
                None,
                &[11],
                (DAY, DAY),
                0,
                vec![loc("A")],
            ),
        ])
        .expect("the rows are stored");
    let barred = with_locators(served[1].clone(), "button Pay now", vec![loc("A")]);
    assert_eq!(causes(&mut w, 2, vec![barred]), vec![(2, Err("rejected"))]);
    let rows: Vec<_> = (2..=8)
        .map(|e| {
            edit_row(
                e,
                1,
                1,
                2,
                "withdrawn",
                None,
                &[11 + e],
                (DAY, DAY),
                0,
                vec![loc(&format!("W{e}"))],
            )
        })
        .collect();
    w.host.commit(rows).expect("the rows are stored");
    let ninth = with_locators(served[1].clone(), "button Pay now", vec![loc("N")]);
    assert_eq!(causes(&mut w, 2, vec![ninth]), vec![(2, Err("full"))]);
}

#[test]
fn put_playbook_of_a_run_with_no_learn_run_row_is_refused_with_binding() {
    let mut w = active_world();
    let now = w.host.agent_now();
    let load = json!({"op": "load", "agent": agent(1), "run": run(2)});
    assert!(halvo_agent_store::runner(&mut w.host.agent, now, load.to_string().as_bytes()).is_ok());
    let binding = ErrorName::read("binding").expect("a declared name");
    let step = with_locators(compiled()[1].clone(), "button Pay now", vec![loc("A")]);
    assert_eq!(w.put(2, 1, vec![step]), Err(Refused::Error(binding)));
    assert_eq!(w.put(2, 2, compiled()), Err(Refused::Error(binding)));
    assert_eq!(playbook_rows(&w).len(), 3);
}

#[test]
fn a_compile_put_playbook_withdraws_every_edit_of_a_lower_compile_number_in_one_commit() {
    let mut w = active_world();
    w.host
        .commit(vec![
            edit_row(
                1,
                1,
                1,
                2,
                "provisional",
                None,
                &[11],
                (DAY - 3, DAY - 3),
                0,
                vec![loc("A")],
            ),
            edit_row(
                1,
                2,
                1,
                2,
                "reverted",
                None,
                &[11],
                (DAY - 3, DAY - 3),
                0,
                vec![loc("A")],
            ),
            edit_row(
                2,
                1,
                1,
                2,
                "active",
                None,
                &[12, 13, 14],
                (DAY - 9, DAY - 4),
                2,
                vec![loc("B")],
            ),
            edit_row(
                3,
                1,
                1,
                3,
                "provisional",
                Some(2),
                &[15],
                (DAY - 1, DAY - 1),
                0,
                vec![loc("C")],
            ),
            edit_row(
                4,
                1,
                1,
                3,
                "withdrawn",
                None,
                &[16],
                (DAY - 1, DAY - 1),
                0,
                vec![loc("D")],
            ),
        ])
        .expect("the rows are stored");
    let commits = w.host.commits.get();
    let count = w.host.count();
    w.start(20, "compile");
    w.load(20);
    let commits = commits + 1;
    assert_eq!(w.put(20, 2, compiled()), Ok(stored(3)));
    assert_eq!(w.host.commits.get(), commits + 1);
    assert_eq!(w.host.count(), count + 1 + 3);
    let edits = w.host.edits();
    for (e, rev) in [(1, 3), (2, 2), (3, 2)] {
        let newest = edits
            .iter()
            .filter(|row| row.edit.as_str() == super::coordinator::edit_id(e))
            .max_by_key(|row| row.rev)
            .expect("rows of the edit");
        assert_eq!(
            (newest.rev.get(), newest.state.name()),
            (rev, "withdrawn"),
            "edit {e}"
        );
    }
    let before: Vec<_> = edits
        .iter()
        .filter(|r| r.state.name() != "withdrawn")
        .cloned()
        .collect();
    assert_eq!(before.len(), 4);
    assert_eq!(w.put(20, 2, compiled()), Ok(stored(3)));
    assert_eq!(w.host.count(), count + 1 + 3);
    assert_eq!(navigate(1), compiled()[0]);
}
