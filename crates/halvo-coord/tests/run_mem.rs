use halvo_coord::guard::Policy as _;
use halvo_coord::mem::policy::Policy;
use halvo_coord::mem::{Call, Config, Method, Ports, Tried};
use halvo_coord::ports::{Owner, PortError, Reaped, Runs};
use halvo_coord::types::{
    AgentId, Autonomy, Body, Class, Event, HandbackResult, Mode, Name, RunId, RunSpec, Trigger,
    What, Workflow, WorkflowId,
};
use std::num::NonZeroU32;

fn id(n: u8) -> String {
    format!("{:032x}", n)
}

fn run(n: u8) -> RunId {
    RunId::parse(&id(n)).unwrap()
}

fn spec(n: u8) -> RunSpec {
    RunSpec {
        run: run(n),
        agent: AgentId::parse(&id(2)).unwrap(),
        workflow: WorkflowId::parse(&id(1)).unwrap(),
        mode: Mode::Replay,
        headed: false,
    }
}

fn refused(name: &str) -> PortError {
    PortError::Refused(Name::parse(name).unwrap())
}

fn config(autonomy: &str) -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"calendar_cleanup\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"{autonomy}\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_calendar\",\"playbook\":1}}",
        id(1),
        id(2)
    ))
    .unwrap()
}

fn refusals(p: &Ports<Config>) -> Vec<(String, Option<PortError>)> {
    p.tried
        .borrow()
        .iter()
        .map(|t: &Tried| (t.call.method().to_string(), t.refused.clone()))
        .collect()
}

#[test]
fn reap_answers_one_mark_per_run_and_a_gone_mark_once() {
    let p: Ports<Config> = Ports::new(0, 0);
    assert_eq!(p.reap(), Ok(vec![]));
    p.leave_marks(&[(run(1), Reaped::Gone), (run(2), Reaped::Alive)]);
    assert_eq!(
        p.reap(),
        Ok(vec![(run(1), Reaped::Gone), (run(2), Reaped::Alive)])
    );
    assert_eq!(
        p.reap(),
        Ok(vec![(run(2), Reaped::Alive)]),
        "the library removes the mark of a run that it answers gone"
    );
}

#[test]
fn a_second_start_of_a_run_id_is_refused_with_started_and_makes_no_second_runner() {
    let p: Ports<Config> = Ports::new(0, 0);
    p.start(&spec(1)).unwrap();
    assert_eq!(p.start(&spec(1)), Err(refused("started")));
    let starts = p
        .calls
        .borrow()
        .iter()
        .filter(|t| matches!(t.call, Call::Start(_)))
        .count();
    assert_eq!(starts, 1);
}

#[test]
fn cancel_kill_and_answer_handback_of_a_run_that_this_life_never_started_are_unknown_run() {
    let p: Ports<Config> = Ports::new(0, 0);
    assert_eq!(p.cancel(&run(1)), Err(refused("unknown_run")));
    assert_eq!(p.kill(&run(1)), Err(refused("unknown_run")));
    assert_eq!(
        p.answer_handback(&run(1), HandbackResult::Done),
        Err(refused("unknown_run"))
    );
    p.start(&spec(1)).unwrap();
    p.new_life();
    assert_eq!(
        p.kill(&run(1)),
        Err(refused("unknown_run")),
        "a new life of the library knows no runner of the earlier one"
    );
}

#[test]
fn cancel_and_answer_handback_of_an_ended_runner_are_ended_and_its_kill_succeeds() {
    let p: Ports<Config> = Ports::new(0, 0);
    p.start(&spec(1)).unwrap();
    p.cancel(&run(1)).unwrap();
    p.end_runner(&run(1));
    assert_eq!(p.cancel(&run(1)), Err(refused("ended")));
    assert_eq!(
        p.answer_handback(&run(1), HandbackResult::Done),
        Err(refused("ended"))
    );
    assert_eq!(p.kill(&run(1)), Ok(()));
    p.start(&spec(2)).unwrap();
    p.kill(&run(2)).unwrap();
    assert_eq!(
        p.cancel(&run(2)),
        Err(refused("ended")),
        "a killed runner has ended"
    );
}

#[test]
fn a_refused_start_stands_in_the_port_column_and_in_no_call() {
    let p: Ports<Config> = Ports::new(0, 0);
    p.refuse(Method::Start, refused("not_started"));
    assert_eq!(p.start(&spec(1)), Err(refused("not_started")));
    assert!(p.calls.borrow().is_empty());
    assert_eq!(
        refusals(&p),
        vec![("start".to_string(), Some(refused("not_started")))]
    );
    p.clear_refusals();
    p.start(&spec(1)).unwrap();
    assert_eq!(refusals(&p)[1], ("start".to_string(), None));
}

fn listed(p: &Ports<Config>) {
    let w = halvo_coord::types::FullWorkflow {
        id: WorkflowId::parse(&id(1)).unwrap(),
        state: halvo_coord::types::WorkflowState::Active,
        category: halvo_coord::types::Category::CalendarCleanup,
        site: None,
        trigger: halvo_coord::types::WorkflowTrigger::Manual,
        period: None,
        last: 0,
        agent: Some(AgentId::parse(&id(2)).unwrap()),
    };
    p.workflows.borrow_mut().push(Workflow::Full(w));
}

fn event(seq: u32, what: What) -> Event {
    Event {
        workflow: WorkflowId::parse(&id(1)).unwrap(),
        agent: AgentId::parse(&id(2)).unwrap(),
        seq: NonZeroU32::new(seq).unwrap(),
        day: 0,
        at: 0,
        what,
    }
}

#[test]
fn the_owner_can_refuse_the_bodies_that_hold_run_started_and_no_other() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p);
    p.refuse(Method::LogRunStarted, PortError::NoService);
    let started = What::RunStarted {
        run: run(1),
        trigger: Trigger::Person,
        slot: None,
    };
    let skipped = What::SlotSkipped { slot: None };
    assert_eq!(
        p.log(&[event(1, skipped.clone()), event(2, started)]),
        Err(PortError::NoService)
    );
    assert!(p.events.borrow().is_empty());
    assert_eq!(p.log(&[event(1, skipped)]), Ok(()));
    assert_eq!(
        refusals(&p),
        vec![
            ("log".to_string(), Some(PortError::NoService)),
            ("log".to_string(), None)
        ]
    );
}

#[test]
fn the_owner_can_refuse_the_body_start_run_alone_and_notes_the_refused_send() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p);
    p.refuse(Method::StartRun, PortError::NoService);
    let start = Body::StartRun {
        workflow: WorkflowId::parse(&id(1)).unwrap(),
        agent: AgentId::parse(&id(2)).unwrap(),
        run: run(1),
        mode: Mode::Replay,
    };
    assert_eq!(p.send(&start), Err(PortError::NoService));
    assert!(p.send(&Body::EndRun { run: run(1) }).is_ok());
    assert_eq!(
        refusals(&p),
        vec![
            ("send".to_string(), Some(PortError::NoService)),
            ("send".to_string(), None)
        ]
    );
}

#[test]
fn the_memory_policy_answers_for_the_class_of_the_record_alone() {
    let policy: Policy<Config> = Policy::new(&[(Autonomy::DraftAndConfirm, Class::Send, true)]);
    let c = config("draft_and_confirm");
    assert_eq!(policy.gives_run(&c, Class::Send), Ok(true));
    assert_eq!(policy.gives_run(&c, Class::LocalEdit), Ok(false));
    assert_eq!(
        policy.gives_run(&config("unattended"), Class::Send),
        Ok(false)
    );
}
