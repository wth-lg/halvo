use halvo_agent_store::{CommitRefused, Host, MemoryStore, AGENT_KINDS};
use halvo_schema::executable::names::AgentState;
use halvo_schema::{
    AgentId, ErrorName, Origin, RowRead, SummaryRead, TypedRecord, Workflow, WorkflowId,
    WorkflowState, WriterStep,
};
use serde_json::json;

fn wf(n: u32) -> String {
    format!("3{n:031x}")
}

fn agent(n: u32) -> String {
    format!("4{n:031x}")
}

fn workflow_id(n: u32) -> WorkflowId {
    WorkflowId::parse(&wf(n)).expect("a workflow id")
}

fn agent_id(n: u32) -> AgentId {
    AgentId::parse(&agent(n)).expect("an agent id")
}

fn workflow(n: u32, state: &str, agent_n: Option<u32>) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": wf(n),
        "state": state,
        "category": "email",
        "site": null,
        "app": "com.example.browser",
        "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87,
        "agent": agent_n.map(agent),
    }))
    .expect("a workflow record")
}

fn dismissed(n: u32) -> Workflow {
    serde_json::from_value(json!({
        "v": 1,
        "id": wf(n),
        "state": "dismissed",
        "category": "email",
        "site": null,
        "app": "com.example.browser",
    }))
    .expect("a dismissed workflow")
}

fn log_line(w: u32, seq: u32) -> TypedRecord {
    TypedRecord {
        kind: "agent_log",
        json: format!(
            r#"{{"v":1,"workflow":"{}","agent":"{}","run":"5{:031x}","seq":{seq},"day":20770,"at":1794500000000000000,"event":"step_ok","step":1,"cause":null,"model":null,"ms":null,"tokens_in":null,"tokens_out":null}}"#,
            wf(w),
            agent(1),
            1
        ),
    }
}

fn run_line(w: u32, run: u32) -> TypedRecord {
    TypedRecord {
        kind: "agent_run",
        json: format!(
            r#"{{"v":1,"workflow":"{}","agent":"{}","run":"5{run:031x}","day":20770,"mode":"replay","outcome":"completed","playbook":1,"steps":9,"prompts":1,"handbacks":0,"confirmations":1,"effects":1,"unknown":0,"model_calls":0,"tokens_in":0,"tokens_out":0,"wall_s":14,"model":null,"before_start":null,"cut":null,"no_key":null,"not_to_account":null,"incomplete":null,"reply_to_other":null,"more_pages":null,"held_recipient":null,"organized":null,"other_task":null,"held_earlier":null,"earlier_page":null}}"#,
            wf(w),
            agent(1)
        ),
    }
}

fn rows(store: &MemoryStore, kind: &str) -> Vec<String> {
    let mut out = Vec::new();
    store.for_each(kind, &mut |row| out.push(row.to_owned()));
    out
}

struct Returns(Vec<TypedRecord>);

impl WriterStep for Returns {
    fn after_commit(&self, _: &dyn SummaryRead, _: &dyn RowRead) -> Vec<TypedRecord> {
        self.0.clone()
    }
}

fn store() -> MemoryStore {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(1, "approved", Some(1)));
    store
}

#[test]
fn own_kind_roles() {
    let mut refused = store().with_step(Box::new(Returns(vec![log_line(1, 1)])));
    assert_eq!(refused.after_batch(), Err(CommitRefused::OwnFromStep));
    assert!(rows(&refused, "agent_log").is_empty());

    let mut capture = store().with_step(Box::new(Returns(vec![run_line(1, 1)])));
    assert_eq!(capture.after_batch(), Ok(()));
    assert_eq!(rows(&capture, "agent_run").len(), 1);

    let mut both = store().with_step(Box::new(Returns(vec![run_line(1, 2), log_line(1, 2)])));
    assert_eq!(both.after_batch(), Err(CommitRefused::OwnFromStep));
    assert!(rows(&both, "agent_run").is_empty());

    let mut handler = store();
    assert_eq!(Host::commit(&mut handler, vec![log_line(1, 1)]), Ok(()));
    assert_eq!(rows(&handler, "agent_log").len(), 1);
}

#[test]
fn steps_run_in_order_and_read_the_rows() {
    struct Counts;
    impl WriterStep for Counts {
        fn after_commit(&self, _: &dyn SummaryRead, rows: &dyn RowRead) -> Vec<TypedRecord> {
            let mut n = 0;
            rows.for_each("agent_run", &mut |_| n += 1);
            vec![run_line(1, 10 + n)]
        }
    }
    let mut store = store()
        .with_step(Box::new(Returns(vec![run_line(1, 1)])))
        .with_step(Box::new(Counts));
    assert_eq!(store.after_batch(), Ok(()));
    let runs = rows(&store, "agent_run");
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().any(|r| r.contains(&format!("5{:031x}", 11))));
}

#[test]
fn the_kind_table_is_the_family_agent() {
    let names: Vec<&str> = AGENT_KINDS.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        [
            "agent_config",
            "agent_run",
            "agent_playbook",
            "agent_bindings",
            "agent_effect",
            "agent_log",
            "agent_session",
            "agent_checkpoint",
        ]
    );
    let capture: Vec<&str> = AGENT_KINDS
        .iter()
        .filter(|(_, origin)| *origin == Origin::Capture)
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(capture, ["agent_config", "agent_run"]);
}

#[test]
fn commit_refuses_a_kind_outside_the_family() {
    let mut store = store();
    let other = TypedRecord {
        kind: "learn_run",
        json: log_line(1, 1).json,
    };
    assert_eq!(
        store.typed_commit(vec![other.clone()]),
        Err(CommitRefused::Kind)
    );
    assert_eq!(Host::commit(&mut store, vec![other]), Err(ErrorName::Store));
}

#[test]
fn commit_is_all_or_none() {
    let mut store = store();
    let bad = TypedRecord {
        kind: "agent_log",
        json: log_line(2, 1).json,
    };
    assert_eq!(
        store.typed_commit(vec![log_line(1, 1), run_line(1, 1), bad]),
        Err(CommitRefused::Workflow)
    );
    assert!(rows(&store, "agent_log").is_empty());
    assert!(rows(&store, "agent_run").is_empty());
}

#[test]
fn commit_checks_a_record_against_its_entry() {
    let mut store = store();
    let mut extra = run_line(1, 1);
    extra.json = extra
        .json
        .replace(r#""earlier_page":null}"#, r#""earlier_page":null,"x":1}"#);
    assert_eq!(store.typed_commit(vec![extra]), Err(CommitRefused::Record));
    let mut not_json = log_line(1, 1);
    not_json.json = "[1]".to_owned();
    assert_eq!(
        store.typed_commit(vec![not_json]),
        Err(CommitRefused::Record)
    );
    let mut no_workflow = log_line(1, 1);
    no_workflow.json = no_workflow.json.replace(r#""workflow""#, r#""flow""#);
    assert_eq!(
        store.typed_commit(vec![no_workflow]),
        Err(CommitRefused::Record)
    );
    assert_eq!(store.typed_commit(vec![run_line(1, 1)]), Ok(()));
}

#[test]
fn commit_needs_a_stored_workflow() {
    let mut store = store();
    assert_eq!(
        store.typed_commit(vec![run_line(2, 1)]),
        Err(CommitRefused::Workflow)
    );
    assert_eq!(
        store.typed_commit(vec![log_line(2, 1)]),
        Err(CommitRefused::Workflow)
    );
    store.put_workflow(dismissed(2));
    assert_eq!(store.typed_commit(vec![log_line(2, 1)]), Ok(()));
}

#[test]
fn rows_are_read_in_byte_order() {
    let mut store = store();
    for seq in [3, 1, 2] {
        assert_eq!(store.typed_commit(vec![log_line(1, seq)]), Ok(()));
    }
    let read = rows(&store, "agent_log");
    let mut sorted = read.clone();
    sorted.sort();
    assert_eq!(read.len(), 3);
    assert_eq!(read, sorted);
    assert!(rows(&store, "agent_effect").is_empty());
    assert!(rows(&store, "workflow").is_empty());
}

#[test]
fn the_state_call() {
    let mut store = MemoryStore::new();
    store.put_workflow(workflow(1, "approved", None));
    store.put_workflow(workflow(2, "proposed", None));
    store.put_workflow(dismissed(3));
    let a = agent_id(1);
    assert_eq!(
        store.set_state(&workflow_id(9), AgentState::Approved, Some(&a)),
        Err(ErrorName::NoWorkflow)
    );
    assert_eq!(
        store.set_state(&workflow_id(2), AgentState::Approved, Some(&a)),
        Err(ErrorName::State)
    );
    assert_eq!(
        store.set_state(&workflow_id(3), AgentState::Approved, Some(&a)),
        Err(ErrorName::State)
    );
    assert_eq!(
        store.set_state(&workflow_id(1), AgentState::Active, None),
        Err(ErrorName::State)
    );
    assert_eq!(
        store.set_state(&workflow_id(1), AgentState::Paused, None),
        Err(ErrorName::State)
    );
    assert_eq!(
        store.set_state(&workflow_id(1), AgentState::Approved, None),
        Ok(())
    );
    assert_eq!(
        store.set_state(&workflow_id(1), AgentState::Active, Some(&a)),
        Ok(())
    );
    assert_eq!(
        store.set_state(&workflow_id(1), AgentState::Active, Some(&a)),
        Ok(())
    );
    let Some(item) = store.workflow(&workflow_id(1)) else {
        panic!("the workflow is stored")
    };
    let Workflow::Full(w) = item.workflow else {
        panic!("a full workflow")
    };
    assert_eq!(w.state, WorkflowState::Active);
    assert_eq!(w.agent, Some(a.clone()));
    assert!(item.summaries.is_empty());
    assert_eq!(
        store.set_state(&workflow_id(1), AgentState::Paused, Some(&a)),
        Ok(())
    );
    let Some(Workflow::Full(w)) = store.workflow(&workflow_id(1)).map(|i| i.workflow) else {
        panic!("a full workflow")
    };
    assert_eq!(w.state, WorkflowState::Paused);
    assert!(store.workflow(&workflow_id(9)).is_none());
}
