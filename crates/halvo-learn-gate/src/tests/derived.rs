use std::collections::hash_map::RandomState;
use std::collections::BTreeSet;
use std::path::PathBuf;

use halvo_schema::executable::names::LocatorBy;
use halvo_schema::executable::step::Target;
use halvo_schema::executable::{ExecutableStep, RunnerReply};
use halvo_schema::WorkflowId;
use serde_json::json;

use crate::delete::{seed_case, MARKERS, SEED_DAY};
use crate::derived::{key_bytes, process_key, DerivedHost, AGENT_OPS, LEARN_OPS};
use crate::host::{GateHost, PlayHost};
use crate::store::MemoryStore;
use crate::stub::{id, play_all, Arm, Play};

fn scratch(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let root = exe
        .ancestors()
        .nth(3)
        .expect("the target directory")
        .join("tmp")
        .join("r5-derived-gate");
    halvo_test_dir::make(&root, name).expect("a test directory")
}

fn first_role_name(steps: &[ExecutableStep]) -> Option<String> {
    match &steps.first()?.target {
        Some(Target::Web(target)) => target
            .locators
            .first()
            .filter(|locator| locator.by == LocatorBy::Role)
            .and_then(|locator| locator.name.as_ref())
            .map(|name| name.as_str().to_owned()),
        _ => None,
    }
}

fn last_day() -> u32 {
    let case = seed_case(MARKERS[0], SEED_DAY);
    case.occurrences[case.occurrences.len() - 1].day
}

fn served_to_a_run<H: PlayHost>(play: &mut Play<'_, H>, run: u32, restart: bool) -> Option<String> {
    let (workflow, agent, run) = (play.workflow().to_owned(), id('4', 1), id('5', run));
    let body = |value: serde_json::Value| value.to_string().into_bytes();
    play.host
        .coordinator(&body(json!({"op": "start_run", "workflow": workflow, "agent": agent, "run": run, "mode": "replay"})))
        .expect("a start");
    if restart {
        play.restart().expect("a restart of the host and the store");
    }
    let RunnerReply::Load(load) = play
        .host
        .runner(&body(json!({"op": "load", "agent": agent, "run": run})))
        .expect("a load")
    else {
        panic!("a load answered with another reply");
    };
    let number = load.playbook.expect("a playbook").number.get();
    let RunnerReply::Steps(steps) = play
        .host
        .runner(&body(
            json!({"op": "load_steps", "agent": agent, "run": run, "number": number, "from": 1}),
        ))
        .expect("a load_steps")
    else {
        panic!("a load_steps answered with another reply");
    };
    first_role_name(&steps.steps)
}

fn a_run_started_before_a_restart_is_served_no_learned_step<H: PlayHost>(host: H) {
    let case = seed_case(MARKERS[0], SEED_DAY);
    let mut play = Play::with_host(&case, Arm::Admit, 1, host);
    play_all(&mut play, false).expect("the seeded case plays");
    assert_eq!(
        served_to_a_run(&mut play, 900, false).as_deref(),
        Some(MARKERS[0])
    );
    assert_ne!(
        served_to_a_run(&mut play, 901, true).as_deref(),
        Some(MARKERS[0]),
        "the run that the old handlers started is a lost run after the restart (contracts.md L4.2)"
    );
}

#[test]
fn derived_the_handlers_in_front_take_every_op_of_the_agent_store_and_list_refinements() {
    let mut want: Vec<&str> = AGENT_OPS.to_vec();
    want.push("list_refinements");
    assert_eq!(LEARN_OPS, want.as_slice());
    let mut ops: Vec<&str> = AGENT_OPS.to_vec();
    ops.sort_unstable();
    ops.dedup();
    assert_eq!(ops.len(), 8);
}

#[test]
fn derived_the_key_holds_four_words_of_its_own_and_another_state_gives_another_key() {
    let state = RandomState::new();
    let bytes = key_bytes(&state);
    let words: BTreeSet<&[u8]> = bytes.chunks(8).collect();
    assert_eq!(words.len(), 4);
    assert_eq!(key_bytes(&state), bytes);
    assert_ne!(key_bytes(&RandomState::new()), bytes);
}

#[test]
fn derived_the_delete_reads_as_stored_the_workflow_that_it_names_alone() {
    let case = seed_case(MARKERS[0], SEED_DAY);
    let host = DerivedHost::new(scratch("stored"), process_key(), SEED_DAY);
    let mut play = Play::with_host(&case, Arm::Admit, 1, host);
    play_all(&mut play, false).expect("the seeded case plays on the derived store");
    let absent = WorkflowId::from_random([9; 16]);
    assert!(!play
        .host
        .delete_workflow(&absent)
        .expect("the decision delete of a workflow that is not stored"));
    let stored = WorkflowId::parse(play.workflow()).expect("the play's workflow id");
    assert!(play
        .host
        .delete_workflow(&stored)
        .expect("the decision delete of the stored workflow"));
    assert!(play
        .host
        .learn_rows("learn_edit")
        .expect("a read")
        .is_empty());
}

#[test]
fn derived_a_run_started_before_a_restart_is_served_the_compiled_steps_after_it() {
    a_run_started_before_a_restart_is_served_no_learned_step(DerivedHost::new(
        scratch("lost-run"),
        process_key(),
        SEED_DAY,
    ));
}

#[test]
fn memory_a_run_started_before_a_restart_is_served_the_compiled_steps_after_it() {
    a_run_started_before_a_restart_is_served_no_learned_step(GateHost::new(
        MemoryStore::new(),
        SEED_DAY,
    ));
}

#[test]
fn derived_a_learned_edit_is_read_back_and_served_after_the_host_and_the_store_restart() {
    let case = seed_case(MARKERS[0], SEED_DAY);
    let host = DerivedHost::new(scratch("restart"), process_key(), SEED_DAY);
    let mut play = Play::with_host(&case, Arm::Admit, 1, host);
    play_all(&mut play, false).expect("the seeded case plays on the derived store");
    let edits = play.host.learn_rows("learn_edit").expect("a read");
    assert!(
        edits.iter().any(|row| row.contains("\"active\"")),
        "{edits:?}"
    );
    play.restart().expect("a restart of the host and the store");
    assert_eq!(
        play.host
            .learn_rows("learn_edit")
            .expect("a read after the restart"),
        edits
    );
    let served = play.served_now().expect("a run after the restart");
    assert_eq!(first_role_name(&served).as_deref(), Some(MARKERS[0]));
}

#[test]
fn derived_list_refinements_goes_to_the_handlers_in_front_and_lists_the_learned_edit() {
    let case = seed_case(MARKERS[0], SEED_DAY);
    let host = DerivedHost::new(scratch("refinements"), process_key(), SEED_DAY);
    let mut play = Play::with_host(&case, Arm::Admit, 1, host);
    play_all(&mut play, false).expect("the seeded case plays on the derived store");
    let body = serde_json::json!({
        "op": "list_refinements",
        "workflow": play.workflow(),
        "agent": crate::stub::id('4', 1),
    })
    .to_string();
    let reply = play
        .host
        .coordinator_reply(&body)
        .expect("the handlers in front answer list_refinements");
    assert!(reply.starts_with(r#"{"refinements":[{"day":"#), "{reply}");
}

#[test]
fn derived_the_store_s_files_hold_neither_the_sqlite_header_nor_a_learned_label() {
    let root = scratch("cipher");
    let case = seed_case(MARKERS[0], SEED_DAY);
    {
        let host = DerivedHost::new(root.clone(), process_key(), SEED_DAY);
        let mut play = Play::with_host(&case, Arm::Admit, 1, host);
        play_all(&mut play, false).expect("the seeded case plays on the derived store");
        let edits = play.host.learn_rows("learn_edit").expect("a read");
        assert!(
            edits.iter().any(|row| row.contains(MARKERS[0])),
            "{edits:?}"
        );
    }
    let database =
        std::fs::read(root.join("derived").join("derived.db")).expect("the database file");
    assert!(database.len() >= 4_096, "{}", database.len());
    for entry in std::fs::read_dir(root.join("derived")).expect("the store's directory") {
        let bytes = std::fs::read(entry.expect("an entry").path()).expect("a file of the store");
        assert!(!bytes.starts_with(b"SQLite format 3\0"));
        assert!(!bytes
            .windows(MARKERS[0].len())
            .any(|window| window == MARKERS[0].as_bytes()));
    }
}

#[test]
fn derived_the_store_opens_again_with_its_key_and_refuses_another_key() {
    let root = scratch("key");
    let key = process_key();
    let case = seed_case(MARKERS[0], SEED_DAY);
    let edits = {
        let mut play = Play::with_host(
            &case,
            Arm::Admit,
            1,
            DerivedHost::new(root.clone(), key.clone(), SEED_DAY),
        );
        play_all(&mut play, false).expect("the seeded case plays on the derived store");
        play.host.learn_rows("learn_edit").expect("a read")
    };
    let mut again = DerivedHost::new(root.clone(), key, last_day());
    assert_eq!(
        again
            .learn_rows("learn_edit")
            .expect("an open with the store's key"),
        edits
    );
    drop(again);
    let mut other = DerivedHost::new(root, process_key(), last_day());
    let refused = other
        .learn_rows("learn_edit")
        .expect_err("an open with another key");
    assert_eq!(
        refused,
        "the derived store does not open: the key does not open the derived database"
    );
}

/// L4.4, failing_first line 5: a row of the family learn is read by the keyed read alone. A learn_edit row that the
/// store keeps (check_record takes it) and LearnEdit::parse refuses (first > last), committed into workflow 1, stops
/// workflow 1's list_refinements and reaches no read of workflow 2: its list answers and its run is served its learned
/// locator.
#[test]
fn derived_a_learn_edit_row_that_one_workflow_refuses_reaches_no_read_of_another_workflow() {
    use halvo_derived::{Handler, HandlerCx, Host, IdSource, Store, Takes};
    use halvo_schema::kinds::registry;
    use halvo_schema::request::decode;
    use halvo_schema::{Decoded, ErrorName, RawRequest, RecurrenceStep, SummaryRead, TypedRecord};
    use serde_json::value::RawValue;

    struct NoProposal;
    impl RecurrenceStep for NoProposal {
        fn propose(&self, _: &dyn SummaryRead) -> Vec<halvo_schema::Proposal> {
            Vec::new()
        }
    }
    struct OnesIds;
    impl IdSource for OnesIds {
        fn fill(&mut self, bytes: &mut [u8]) {
            bytes.fill(1);
        }
    }
    struct PlantLearnEdit;
    impl Handler for PlantLearnEdit {
        fn handle(
            &self,
            request: &RawRequest,
            cx: &mut HandlerCx<'_>,
        ) -> Result<Box<RawValue>, ErrorName> {
            let body: serde_json::Value =
                serde_json::from_str(request.body().get()).map_err(|_| ErrorName::Malformed)?;
            cx.commit(&[TypedRecord {
                kind: "learn_edit",
                json: body["row"].to_string(),
            }])
            .map_err(|_| ErrorName::Store)?;
            RawValue::from_string("{\"ok\":\"yes\"}".to_owned()).map_err(|_| ErrorName::Internal)
        }
    }

    let root = scratch("refused-row");
    let key = process_key();
    let cases = crate::delete::seed_cases();
    let day = cases[1].occurrences[cases[1].occurrences.len() - 1].day;
    let (first_workflow, second_workflow) = {
        let host = DerivedHost::new(root.clone(), key.clone(), cases[0].occurrences[0].day);
        let mut first = Play::with_host(&cases[0], Arm::Admit, 1, host);
        play_all(&mut first, false).expect("workflow 1 plays on the derived store");
        let first_workflow = first.workflow().to_owned();
        let mut second = Play::with_host(&cases[1], Arm::Admit, 2, first.host);
        play_all(&mut second, false).expect("workflow 2 plays on the derived store");
        (first_workflow, second.workflow().to_owned())
    };
    {
        let clock = crate::derived::Day::default();
        clock.set(day);
        let entries: Vec<&'static halvo_schema::KindEntry> = registry().collect();
        let host = Host::new(Box::new(clock), Box::new(NoProposal)).handler(
            "learn",
            Takes::Coordinator(&["plant"]),
            Box::new(PlantLearnEdit),
        );
        let mut store = Store::open(&root, &key, &entries, host, &mut OnesIds)
            .expect("the store opens again with its key");
        let refused_row = json!({
            "v": 1, "workflow": first_workflow, "agent": id('4', 1),
            "edit": "5c4b3a29180f7e6d5c4b3a29180f7e6d", "rev": 1, "base": 1, "n": 1,
            "state": "provisional", "cause": null, "on": null, "runs": [id('5', 99)],
            "first": day + 1, "last": day, "clean": 0,
            "target": {"desc": "button \"bad\"", "locators": [{"by": "role", "value": "button", "name": "bad"}]}
        });
        let request = json!({
            "v": 1, "id": "11111111111111111111111111111111", "request": "coordinator_write",
            "body": {"op": "plant", "row": refused_row}
        })
        .to_string();
        let Decoded::Raw(raw) = decode(request.as_bytes()).expect("the request decodes") else {
            panic!("the request is not a raw request");
        };
        store
            .handle(&raw)
            .expect("the store keeps the row that LearnEdit::parse refuses");
    }
    let mut host = DerivedHost::new(root, key, day);
    host.link(Arm::Admit);
    let list = |workflow: &str, agent: u32| {
        json!({"op": "list_refinements", "workflow": workflow, "agent": id('4', agent)}).to_string()
    };
    let first_list = host.coordinator_reply(&list(&first_workflow, 1));
    assert!(
        first_list.is_err(),
        "workflow 1 reads its refused row: {first_list:?}"
    );
    let second_list = host
        .coordinator_reply(&list(&second_workflow, 2))
        .expect("workflow 2's list_refinements does not read workflow 1's refused row");
    assert!(second_list.contains("target_kept"), "{second_list}");
    let (agent, run) = (id('4', 2), id('5', 8802));
    let body = |value: serde_json::Value| value.to_string().into_bytes();
    host.coordinator(&body(json!({"op": "start_run", "workflow": second_workflow, "agent": agent, "run": run, "mode": "replay"})))
        .expect("workflow 2's start_run");
    let RunnerReply::Load(load) = host
        .runner(&body(json!({"op": "load", "agent": agent, "run": run})))
        .expect("workflow 2's load")
    else {
        panic!("a load answered with another reply");
    };
    let number = load.playbook.expect("a playbook").number.get();
    let RunnerReply::Steps(steps) = host
        .runner(&body(
            json!({"op": "load_steps", "agent": agent, "run": run, "number": number, "from": 1}),
        ))
        .expect("workflow 2's load_steps")
    else {
        panic!("a load_steps answered with another reply");
    };
    assert_eq!(
        first_role_name(&steps.steps).as_deref(),
        Some(MARKERS[1]),
        "workflow 2's run is served its learned locator"
    );
}

/// failing_first line 3: a restart drops the derived store and opens it again from its files, so that a read after it
/// comes from the files and not from rows that the old connection still holds.
#[test]
fn derived_a_restart_closes_the_store_and_opens_it_again() {
    let mut host = DerivedHost::new(scratch("reopen"), process_key(), SEED_DAY);
    host.learn_rows("learn_run")
        .expect("a read, which opens the store");
    let before = host.opens();
    host.restart().expect("a restart");
    assert_eq!(
        host.opens(),
        before + 1,
        "a restart closes the store and opens it again"
    );
}
