use super::*;
use crate::ports::{Analysis, Clock, Owner, Page, PortError, Rng, Runs, Stamp, Tray};
use crate::types::*;

fn id<T>(parse: fn(&str) -> Result<T, Cause>, n: u8) -> T {
    let mut b = [0u8; 16];
    b[15] = n;
    parse(&hex(b)).unwrap()
}

const BILL: &str = "{\"v\":1,\"workflow\":\"00000000000000000000000000000001\",\"agent\":\"00000000000000000000000000000002\",\"rev\":1,\"category\":\"utility_bill_payment\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"yes\"},\"limits\":{\"steps\":20,\"wall_s\":120,\"model_tokens\":0},\"budget\":null,\"permissions\":[],\"runtime\":\"web\",\"playbook\":null}";

#[test]
fn config_reads_the_record_of_a_a2_1_behind_config_view() {
    let c = Config::parse(BILL).unwrap();
    assert_eq!(c.tier(), Tier::OnDevice);
    assert_eq!(c.reasoning(), Reasoning::Medium);
    assert_eq!(c.autonomy(), Autonomy::DraftAndConfirm);
    assert!(c.allow_is_empty());
    assert!(c.caps_external());
    assert_eq!(c.wall_s(), 120);
    assert_eq!(c.model_tokens(), 0);
    assert_eq!(c.budget(), None);
    assert!(c.permissions().is_empty());
    assert_eq!(c.runtime(), Runtime::Web);
    assert_eq!(c.playbook(), None);
    assert_eq!(c.rev(), 1);
    assert_eq!(c.category, Category::UtilityBillPayment);
    assert_eq!(c.workflow, id(WorkflowId::parse, 1));
    assert_eq!(c.agent, id(AgentId::parse, 2));

    let fill = BILL
        .replace("\"external\":\"yes\"", "\"external\":\"no\"")
        .replace("\"permissions\":[]", "\"permissions\":[\"user_files\"]")
        .replace(
            "\"budget\":null",
            "\"budget\":{\"amount\":500,\"period\":\"month\"}",
        )
        .replace("\"allow\":[]", "\"allow\":[\"send\"]")
        .replace("\"playbook\":null", "\"playbook\":3");
    let c = Config::parse(&fill).unwrap();
    assert!(!c.caps_external());
    assert!(!c.allow_is_empty());
    assert_eq!(c.permissions(), vec![Permission::UserFiles]);
    assert_eq!(
        c.budget(),
        Some(Budget {
            amount: 500,
            period: Period::Month
        })
    );
    assert_eq!(c.playbook(), Some(3));
}

#[test]
fn config_refuses_every_other_shape() {
    let bad = [
        String::new(),
        "[]".to_owned(),
        BILL.replace("\"v\":1", "\"v\":2"),
        BILL.replace("\"tier\":\"on_device\"", "\"tier\":\"gpt\""),
        BILL.replace(",\"playbook\":null", ""),
        BILL.replace("}", ",\"extra\":1}"),
        BILL.replace("\"allow\":[]", "\"allow\":[\"payment\"]"),
        BILL.replace("\"caps\":{", "\"caps\":{\"other\":\"yes\","),
        BILL.replace("\"external\":\"yes\"", "\"external\":true"),
        BILL.replace("\"wall_s\":120", "\"wall_s\":-1"),
        BILL.replace("\"budget\":null", "\"budget\":{\"amount\":500}"),
        BILL.replace("\"permissions\":[]", "\"permissions\":[\"camera\"]"),
        BILL.replace(
            "\"agent\":\"00000000000000000000000000000002\"",
            "\"agent\":\"2\"",
        ),
    ];
    for text in bad {
        assert_eq!(Config::parse(&text).err(), Some(Cause::Malformed), "{text}");
    }
}

const BUDGET: &str = "\"budget\":{\"amount\":500,\"period\":\"month\"}";

fn with_budget() -> String {
    BILL.replace("\"budget\":null", BUDGET)
}

#[test]
fn config_refuses_a_doubled_key_at_every_level() {
    let bad = [
        with_budget().replace("\"rev\":1", "\"rev\":1,\"rev\":1"),
        with_budget().replace("\"rev\":1", "\"rev\":1,\"r\\u0065v\":2"),
        with_budget().replace(
            "\"external\":\"yes\"",
            "\"external\":\"yes\",\"external\":\"no\"",
        ),
        with_budget().replace("\"wall_s\":120", "\"wall_s\":120,\"wall_s\":120"),
        with_budget().replace("\"amount\":500", "\"amount\":500,\"amount\":7"),
    ];
    assert!(Config::parse(&with_budget()).is_ok());
    for text in bad {
        assert_eq!(Config::parse(&text).err(), Some(Cause::Malformed), "{text}");
    }
}

#[test]
fn config_refuses_an_unknown_key_at_every_level() {
    let bad = [
        with_budget().replace("\"rev\":1", "\"rev\":1,\"name\":\"x\""),
        with_budget().replace(
            "\"external\":\"yes\"",
            "\"external\":\"yes\",\"other\":\"no\"",
        ),
        with_budget().replace("\"wall_s\":120", "\"wall_s\":120,\"cpu_s\":1"),
        with_budget().replace("\"amount\":500", "\"amount\":500,\"currency\":\"usd\""),
    ];
    for text in bad {
        assert_eq!(Config::parse(&text).err(), Some(Cause::Malformed), "{text}");
    }
}

#[test]
fn config_refuses_a_missing_key_at_every_level() {
    let bad = [
        with_budget().replace("\"rev\":1,", ""),
        with_budget().replace(",\"external\":\"yes\"", ""),
        with_budget().replace(",\"wall_s\":120", ""),
        with_budget().replace(",\"period\":\"month\"", ""),
    ];
    for text in bad {
        assert_eq!(Config::parse(&text).err(), Some(Cause::Malformed), "{text}");
    }
}

#[test]
fn config_refuses_a_budget_amount_outside_1_to_100000000() {
    for amount in ["0", "100000001", "9223372036854775808"] {
        let text = with_budget().replace("\"amount\":500", &format!("\"amount\":{amount}"));
        assert_eq!(
            Config::parse(&text).err(),
            Some(Cause::Malformed),
            "{amount}"
        );
    }
    for amount in [1, 100_000_000] {
        let text = with_budget().replace("\"amount\":500", &format!("\"amount\":{amount}"));
        assert_eq!(
            Config::parse(&text).unwrap().budget().map(|b| b.amount),
            Some(amount)
        );
    }
}

#[test]
fn rng_counts_up_from_one() {
    let p: Ports<Config> = Ports::new(0, 0);
    let first = p.bytes16().unwrap();
    assert_eq!(hex(first), "00000000000000000000000000000001");
    assert_eq!(
        hex(p.bytes16().unwrap()),
        "00000000000000000000000000000002"
    );
}

#[test]
fn clock_gives_what_the_test_set() {
    let p: Ports<Config> = Ports::new(1_793_707_200_000, 20_760);
    assert_eq!(p.now(), Ok(1_793_707_200_000));
    assert_eq!(p.today(), Ok(20_760));
    p.set_clock(1_793_707_205_000, 20_760);
    assert_eq!(p.now(), Ok(1_793_707_205_000));
}

const DAY_20760: u64 = 1_793_664_000_000;
const HOUR: u64 = 3_600_000;

#[test]
fn clock_gives_the_local_day_of_an_instant_in_its_zone() {
    let p: Ports<Config> = Ports::new(DAY_20760 + 12 * HOUR, 20_760);
    assert_eq!(p.day(DAY_20760 + 24 * HOUR - 1), Ok(20_760));
    assert_eq!(p.day(DAY_20760 + 24 * HOUR), Ok(20_761));
    assert_eq!(p.day(DAY_20760), Ok(20_760));
    assert_eq!(p.day(DAY_20760 - 1), Ok(20_759));
    p.set_zone(5 * HOUR as i64);
    assert_eq!(p.day(DAY_20760 + 19 * HOUR - 1), Ok(20_760));
    assert_eq!(p.day(DAY_20760 + 19 * HOUR), Ok(20_761));
    p.set_zone(-(7 * HOUR as i64));
    assert_eq!(p.day(DAY_20760 + 7 * HOUR - 1), Ok(20_759));
    assert_eq!(p.day(DAY_20760 + 7 * HOUR), Ok(20_760));
    assert_eq!(p.today(), Ok(20_760));
    p.set_now(DAY_20760 + 31 * HOUR);
    assert_eq!(p.today(), Ok(20_761));
    p.set_clock(DAY_20760 + 2 * HOUR, 20_700);
    assert_eq!(p.today(), Ok(20_700));
    assert_eq!(p.day(DAY_20760 + 24 * HOUR), Ok(20_701));
}

#[test]
fn today_is_the_day_of_one_read_of_now() {
    let p: Ports<Config> = Ports::new(DAY_20760 + 24 * HOUR - 1, 20_760);
    p.step_reads(1);
    assert_eq!(p.today(), Ok(20_760));
    assert_eq!(p.now(), Ok(DAY_20760 + 24 * HOUR));
    assert_eq!(p.today(), Ok(20_761));
}

#[test]
fn a_stamp_takes_the_day_of_the_instant_that_it_read() {
    let p: Ports<Config> = Ports::new(DAY_20760 + 24 * HOUR - 1, 20_760);
    p.step_reads(1);
    let stamp = Stamp::read(&p).unwrap();
    assert_eq!(stamp.now(), DAY_20760 + 24 * HOUR - 1);
    assert_eq!(stamp.day(), 20_760);
    assert_eq!(stamp.at(), (DAY_20760 + 24 * HOUR - 1) as i64 * 1_000_000);
    assert_eq!(p.today(), Ok(20_761));
    let largest = i64::MAX as u64 / 1_000_000;
    let p: Ports<Config> = Ports::new(largest, 106_751);
    assert_eq!(
        Stamp::read(&p).map(Stamp::at),
        Ok(largest as i64 * 1_000_000)
    );
    p.set_now(largest + 1);
    assert_eq!(Stamp::read(&p), Err(PortError::NoService));
    let p: Ports<Config> = Ports::new(u64::MAX, 0);
    assert_eq!(Stamp::read(&p), Err(PortError::NoService));
}

/// A clock whose day answers for every instant.
struct Far;

impl Clock for Far {
    fn now(&self) -> Result<u64, PortError> {
        Ok(u64::MAX)
    }

    fn day(&self, _at: u64) -> Result<u32, PortError> {
        Ok(0)
    }
}

/// No stamp holds an instant whose nanoseconds pass `i64::MAX`, also from a clock that gives a
/// day for it: the `at` of a row would not be its instant (review case review_day_1).
#[test]
fn no_stamp_holds_an_instant_past_the_largest_at() {
    assert_eq!(Stamp::read(&Far), Err(PortError::NoService));
}

fn spec() -> RunSpec {
    RunSpec {
        run: id(RunId::parse, 14),
        agent: id(AgentId::parse, 2),
        workflow: id(WorkflowId::parse, 1),
        mode: Mode::Compile,
        headed: true,
    }
}

fn answers() -> Vec<Answer> {
    let digest = Digest::parse(&"d".repeat(64)).unwrap();
    vec![
        Answer {
            effect: id(EffectId::parse, 5),
            digest: digest.clone(),
            answer: EffectAnswer::AllowOnce,
        },
        Answer {
            effect: id(EffectId::parse, 6),
            digest,
            answer: EffectAnswer::AllowOnce,
        },
    ]
}

#[test]
fn runs_records_each_call_with_the_clock_time() {
    let p: Ports<Config> = Ports::new(1000, 1);
    p.start(&spec()).unwrap();
    p.set_clock(6000, 1);
    p.cancel(&spec().run).unwrap();
    p.kill(&spec().run).unwrap();
    assert_eq!(p.reap(), Ok(vec![]));
    p.start(&RunSpec {
        run: id(RunId::parse, 48),
        ..spec()
    })
    .unwrap();
    p.answer_handback(&id(RunId::parse, 48), HandbackResult::Done)
        .unwrap();
    let calls = p.calls.borrow();
    let at: Vec<u64> = calls.iter().map(|c| c.at).collect();
    assert_eq!(at, vec![1000, 6000, 6000, 6000, 6000, 6000]);
    assert!(matches!(calls[0].call, Call::Start(ref s) if s == &spec()));
    assert!(matches!(calls[1].call, Call::Cancel(_)));
    assert!(matches!(calls[2].call, Call::Kill(_)));
    assert!(matches!(calls[3].call, Call::Reap));
    let calls = &calls[1..];
    assert!(matches!(
        calls[4].call,
        Call::AnswerHandback(_, HandbackResult::Done)
    ));
}

#[test]
fn answer_confirm_sends_the_decision_only_after_record_returned_success() {
    let p: Ports<Config> = Ports::new(0, 0);
    let mut seen = Vec::new();
    p.answer_confirm(&spec().run, &answers(), &mut |d: &Decision| {
        seen.push(d.clone());
        p.log(&[]).map(|_| ())
    })
    .unwrap();
    assert_eq!(seen.len(), 1);
    let sent = p.sent_decisions();
    assert_eq!(sent, seen);
    let ids: Vec<&str> = sent[0]
        .approvals()
        .iter()
        .map(|e| e.approval.as_str())
        .collect();
    assert_eq!(
        ids,
        vec![
            "00000000000000000000000000000001",
            "00000000000000000000000000000002"
        ]
    );
    assert!(sent[0]
        .approvals()
        .iter()
        .all(|e| e.answer == EffectAnswer::AllowOnce));
    let order: Vec<&str> = p.calls.borrow().iter().map(|c| c.call.method()).collect();
    assert_eq!(order, vec!["log", "answer_confirm"]);
}

#[test]
fn answer_confirm_sends_reject_once_under_new_ids_when_record_fails() {
    let p: Ports<Config> = Ports::new(0, 0);
    let mut handed = Vec::new();
    let answered = p.answer_confirm(&spec().run, &answers(), &mut |d: &Decision| {
        handed.push(d.clone());
        Err(PortError::NoService)
    });
    assert_eq!(
        answered,
        Err(PortError::NoService),
        "a success return means that the answer's decision was sent (I4.5)"
    );
    let sent = p.sent_decisions();
    assert_eq!(sent.len(), 1);
    assert!(sent[0]
        .approvals()
        .iter()
        .all(|e| e.answer == EffectAnswer::RejectOnce));
    for e in sent[0].approvals() {
        assert!(handed[0]
            .approvals()
            .iter()
            .all(|h| h.approval != e.approval));
    }
    let effects: Vec<EffectId> = sent[0]
        .approvals()
        .iter()
        .map(|e| e.effect.clone())
        .collect();
    let asked: Vec<EffectId> = answers().into_iter().map(|a| a.effect).collect();
    assert_eq!(effects, asked);
}

#[test]
fn answer_confirm_refuses_answers_that_make_no_decision() {
    let p: Ports<Config> = Ports::new(0, 0);
    let mut called = 0;
    let mut record = |_: &Decision| {
        called += 1;
        Ok(())
    };
    let doubled = vec![answers()[0].clone(), answers()[0].clone()];
    assert_eq!(
        p.answer_confirm(&spec().run, &[], &mut record),
        refused("malformed")
    );
    assert_eq!(
        p.answer_confirm(&spec().run, &doubled, &mut record),
        refused("malformed")
    );
    assert_eq!(called, 0);
    assert!(p.sent_decisions().is_empty());
    p.answer_confirm(&spec().run, &answers(), &mut |_| Ok(()))
        .unwrap();
    assert_eq!(
        p.sent_decisions()[0].approvals()[0].approval.as_str(),
        "00000000000000000000000000000001",
        "a refusal mints no id"
    );
}

#[test]
fn a_refusal_set_by_the_test_answers_that_method() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 1, Some(2));
    p.refuse(Method::Log, PortError::NoService);
    p.refuse(
        Method::Send,
        PortError::Refused(Name::parse("playbook").unwrap()),
    );
    assert_eq!(p.log(&[]), Err(PortError::NoService));
    let body = Body::SetState {
        workflow: id(WorkflowId::parse, 1),
        agent: id(AgentId::parse, 2),
        state: AgentState::Active,
    };
    assert_eq!(
        p.send(&body),
        Err(PortError::Refused(Name::parse("playbook").unwrap()))
    );
    p.clear_refusals();
    assert!(p.log(&[]).is_ok());
    assert_eq!(
        p.send(&body).unwrap(),
        serde_json::json!({"state": "active"})
    );
}

#[test]
fn owner_answers_each_body_as_a_a7_2_says() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 1, None);
    let (w, a) = (id(WorkflowId::parse, 1), id(AgentId::parse, 2));
    let create = Body::CreateAgent {
        workflow: w.clone(),
        agent: a.clone(),
    };
    let bind = Body::SetBindings {
        workflow: w.clone(),
        agent: a.clone(),
        slots: vec![Slot {
            name: Name::parse("payee").unwrap(),
            value: vec!["City Power".to_owned()],
        }],
    };
    let start = Body::StartRun {
        workflow: w,
        agent: a,
        run: id(RunId::parse, 14),
        mode: Mode::Replay,
    };
    assert_eq!(p.send(&create).unwrap(), serde_json::json!({"rev": 1}));
    assert_eq!(p.send(&bind).unwrap(), serde_json::json!({"rev": 1}));
    assert_eq!(p.send(&bind).unwrap(), serde_json::json!({"rev": 2}));
    assert_eq!(p.send(&start).unwrap(), serde_json::json!({"ok": "yes"}));
}

fn event(seq: u32, agent: u8) -> Event {
    Event {
        workflow: id(WorkflowId::parse, agent - 1),
        agent: id(AgentId::parse, agent),
        seq: std::num::NonZeroU32::new(seq).unwrap(),
        day: 1,
        at: 1_000_000,
        what: What::SlotSkipped { slot: None },
    }
}

fn listed(p: &Ports<Config>, workflow: u8, agent: Option<u8>) {
    p.workflows.borrow_mut().push(Workflow::Full(FullWorkflow {
        id: id(WorkflowId::parse, workflow),
        state: WorkflowState::Active,
        category: Category::UtilityBillPayment,
        site: None,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: 20_700,
        agent: agent.map(|a| id(AgentId::parse, a)),
    }));
}

fn refused<T>(name: &str) -> Result<T, PortError> {
    Err(PortError::Refused(Name::parse(name).unwrap()))
}

#[test]
fn log_refuses_a_seq_that_is_not_the_stored_last_plus_one() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 1, Some(2));
    assert_eq!(p.log(&[event(2, 2)]), refused("seq"), "the first seq is 1");
    p.log(&[event(1, 2)]).unwrap();
    assert_eq!(p.log(&[event(1, 2)]), refused("seq"), "a repeat");
    assert_eq!(p.log(&[event(5, 2)]), refused("seq"), "a gap");
    assert_eq!(
        p.log(&[event(2, 2), event(4, 2)]),
        refused("seq"),
        "a gap inside one body"
    );
    assert_eq!(p.events.borrow().len(), 1, "a refusal stores nothing");
    assert_eq!(p.calls.borrow().len(), 1, "a refusal records no call");
    p.log(&[event(2, 2), event(3, 2)]).unwrap();
    assert_eq!(p.events.borrow().len(), 3);
}

#[test]
fn log_refuses_a_workflow_that_is_not_in_the_owner_s_list() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 3, Some(4));
    assert_eq!(p.log(&[event(1, 2)]), refused("no_workflow"));
    assert_eq!(
        p.log(&[event(1, 4), event(1, 2)]),
        refused("no_workflow"),
        "one record of an unlisted workflow refuses the body"
    );
    assert!(p.events.borrow().is_empty());
    assert!(p.calls.borrow().is_empty());
}

#[test]
fn log_refuses_an_agent_that_is_not_the_workflow_s() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 1, Some(6));
    listed(&p, 3, None);
    assert_eq!(p.log(&[event(1, 2)]), refused("binding"));
    assert_eq!(
        p.log(&[event(1, 4)]),
        refused("binding"),
        "a workflow with no agent"
    );
    assert!(p.events.borrow().is_empty());
    p.agents.borrow_mut().push(Agent {
        workflow: id(WorkflowId::parse, 1),
        agent: id(AgentId::parse, 2),
        config: Config::parse(BILL).unwrap(),
        state: AgentState::Approved,
        slots: vec![],
        session: false,
        last: None,
        unknown: vec![],
    });
    p.log(&[event(1, 2)]).unwrap();
    p.send(&Body::CreateAgent {
        workflow: id(WorkflowId::parse, 3),
        agent: id(AgentId::parse, 4),
    })
    .unwrap();
    p.log(&[event(1, 4)]).unwrap();
    assert_eq!(p.events.borrow().len(), 2);
}

#[test]
fn log_refuses_more_than_request_records_records_in_one_body() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 1, Some(2));
    let nine: Vec<Event> = (1..=9).map(|s| event(s, 2)).collect();
    assert_eq!(p.log(&nine), refused("malformed"));
    assert!(p.events.borrow().is_empty());
    p.log(&nine[..8]).unwrap();
    assert_eq!(p.events.borrow().len(), 8);
}

#[test]
fn ports_keep_the_request_records_of_the_limits_that_they_are_given() {
    let p: Ports<Config> = Ports::with_limits(
        0,
        0,
        Limits {
            request_records: 2,
            ..Limits::PRODUCT
        },
    );
    listed(&p, 1, Some(2));
    assert_eq!(
        p.log(&[event(1, 2), event(2, 2), event(3, 2)]),
        refused("malformed")
    );
    p.log(&[event(1, 2), event(2, 2)]).unwrap();
    assert_eq!(p.events.borrow().len(), 2);
}

#[test]
fn reap_ends_and_counts_the_trees_of_an_earlier_life() {
    let p: Ports<Config> = Ports::new(0, 0);
    assert_eq!(p.reap(), Ok(vec![]));
    let gone = (id(RunId::parse, 4), crate::ports::Reaped::Gone);
    p.leave_marks(std::slice::from_ref(&gone));
    assert_eq!(p.reap(), Ok(vec![gone]));
    assert_eq!(p.reap(), Ok(vec![]), "a run answered gone has no mark left");
}

#[test]
fn send_refuses_set_state_active_for_a_web_agent_with_no_playbook() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 1, Some(2));
    listed(&p, 3, Some(4));
    listed(&p, 5, Some(6));
    let agent = |a: u8, record: String| Agent {
        workflow: id(WorkflowId::parse, a - 1),
        agent: id(AgentId::parse, a),
        config: Config::parse(&record).unwrap(),
        state: AgentState::Approved,
        slots: vec![],
        session: false,
        last: None,
        unknown: vec![],
    };
    let record = |a: u8| {
        BILL.replace(
            "\"workflow\":\"00000000000000000000000000000001\",\"agent\":\"00000000000000000000000000000002\"",
            &format!(
                "\"workflow\":\"{:032x}\",\"agent\":\"{:032x}\"",
                a - 1,
                a
            ),
        )
    };
    p.agents.borrow_mut().push(agent(2, record(2)));
    p.agents.borrow_mut().push(agent(
        4,
        record(4).replace("\"playbook\":null", "\"playbook\":3"),
    ));
    p.agents.borrow_mut().push(agent(
        6,
        record(6).replace("\"runtime\":\"web\"", "\"runtime\":\"graph_mail\""),
    ));
    let set = |a: u8, state: AgentState| Body::SetState {
        workflow: id(WorkflowId::parse, a - 1),
        agent: id(AgentId::parse, a),
        state,
    };
    assert_eq!(p.send(&set(2, AgentState::Active)), refused("playbook"));
    assert!(p.calls.borrow().is_empty(), "a refusal records no call");
    assert_eq!(
        p.send(&set(2, AgentState::Approved)).unwrap(),
        serde_json::json!({"state": "approved"})
    );
    assert_eq!(
        p.send(&set(4, AgentState::Active)).unwrap(),
        serde_json::json!({"state": "active"})
    );
    assert_eq!(
        p.send(&set(6, AgentState::Active)).unwrap(),
        serde_json::json!({"state": "active"})
    );
}

fn workflow_in(p: &Ports<Config>, workflow: u8, category: Category, state: WorkflowState) {
    p.workflows.borrow_mut().push(Workflow::Full(FullWorkflow {
        id: id(WorkflowId::parse, workflow),
        state,
        category,
        site: None,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: 20_700,
        agent: None,
    }));
    p.send(&Body::CreateAgent {
        workflow: id(WorkflowId::parse, workflow),
        agent: id(AgentId::parse, workflow + 1),
    })
    .unwrap();
}

#[test]
fn send_refuses_set_state_active_for_a_created_web_agent_with_no_playbook() {
    let p: Ports<Config> = Ports::new(0, 0);
    workflow_in(&p, 1, Category::UtilityBillPayment, WorkflowState::Approved);
    workflow_in(&p, 3, Category::ShopOrder, WorkflowState::Approved);
    workflow_in(&p, 5, Category::Email, WorkflowState::Approved);
    let set = |w: u8, state: AgentState| Body::SetState {
        workflow: id(WorkflowId::parse, w),
        agent: id(AgentId::parse, w + 1),
        state,
    };
    let calls = p.calls.borrow().len();
    assert_eq!(p.send(&set(1, AgentState::Active)), refused("playbook"));
    assert_eq!(p.send(&set(3, AgentState::Active)), refused("playbook"));
    assert_eq!(p.calls.borrow().len(), calls, "a refusal records no call");
    assert_eq!(
        p.send(&set(1, AgentState::Approved)).unwrap(),
        serde_json::json!({"state": "approved"})
    );
    assert_eq!(
        p.send(&set(5, AgentState::Active)).unwrap(),
        serde_json::json!({"state": "active"}),
        "the preset of email has the runtime graph_mail (A-A1.3)"
    );
}

#[test]
fn send_refuses_start_run_as_a_a7_2_checks_it() {
    let p: Ports<Config> = Ports::new(0, 0);
    workflow_in(&p, 1, Category::Email, WorkflowState::Approved);
    workflow_in(&p, 3, Category::Email, WorkflowState::Active);
    workflow_in(&p, 5, Category::Email, WorkflowState::Paused);
    p.workflows.borrow_mut().push(Workflow::Full(FullWorkflow {
        id: id(WorkflowId::parse, 7),
        state: WorkflowState::Proposed,
        category: Category::Email,
        site: None,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: 20_700,
        agent: Some(id(AgentId::parse, 8)),
    }));
    let start = |w: u8, a: u8, mode: Mode| Body::StartRun {
        workflow: id(WorkflowId::parse, w),
        agent: id(AgentId::parse, a),
        run: id(RunId::parse, 99),
        mode,
    };
    let ok = serde_json::json!({"ok": "yes"});
    let calls = p.calls.borrow().len();
    assert_eq!(p.send(&start(9, 10, Mode::Compile)), refused("no_workflow"));
    assert_eq!(p.send(&start(1, 4, Mode::Compile)), refused("binding"));
    assert_eq!(p.send(&start(1, 2, Mode::Replay)), refused("state"));
    assert_eq!(p.send(&start(5, 6, Mode::Compile)), refused("state"));
    assert_eq!(p.send(&start(7, 8, Mode::Compile)), refused("state"));
    assert_eq!(p.calls.borrow().len(), calls, "a refusal records no call");
    assert_eq!(p.send(&start(1, 2, Mode::Compile)).unwrap(), ok);
    assert_eq!(p.send(&start(3, 4, Mode::Compile)).unwrap(), ok);
    assert_eq!(p.send(&start(3, 4, Mode::Replay)).unwrap(), ok);
}

fn held(p: &Ports<Config>, workflow: u8, state: Option<WorkflowState>) {
    let w = id(WorkflowId::parse, workflow);
    p.workflows.borrow_mut().push(match state {
        Some(state) => Workflow::Full(FullWorkflow {
            id: w,
            state,
            category: Category::Email,
            site: None,
            trigger: WorkflowTrigger::Manual,
            period: None,
            last: 20_700,
            agent: None,
        }),
        None => Workflow::Dismissed(DismissedWorkflow {
            id: w,
            category: Category::Email,
            site: None,
        }),
    });
}

fn create(w: u8) -> Body {
    Body::CreateAgent {
        workflow: id(WorkflowId::parse, w),
        agent: id(AgentId::parse, w + 1),
    }
}

fn set(w: u8, state: AgentState) -> Body {
    Body::SetState {
        workflow: id(WorkflowId::parse, w),
        agent: id(AgentId::parse, w + 1),
        state,
    }
}

#[test]
fn send_refuses_create_agent_for_a_workflow_that_the_owner_does_not_hold() {
    let p: Ports<Config> = Ports::new(0, 0);
    held(&p, 3, Some(WorkflowState::Approved));
    assert_eq!(p.send(&create(1)), refused("no_workflow"));
    assert!(p.calls.borrow().is_empty(), "a refusal records no call");
    assert_eq!(p.send(&create(3)).unwrap(), serde_json::json!({"rev": 1}));
}

#[test]
fn send_refuses_create_agent_for_a_proposed_or_dismissed_workflow() {
    let p: Ports<Config> = Ports::new(0, 0);
    held(&p, 1, Some(WorkflowState::Proposed));
    held(&p, 3, None);
    assert_eq!(p.send(&create(1)), refused("state"));
    assert_eq!(p.send(&create(3)), refused("state"));
    assert!(p.calls.borrow().is_empty(), "a refusal records no call");
    let event = Event {
        workflow: id(WorkflowId::parse, 1),
        agent: id(AgentId::parse, 2),
        seq: std::num::NonZeroU32::MIN,
        day: 1,
        at: 1_000_000,
        what: What::SlotSkipped { slot: None },
    };
    assert_eq!(
        p.log(&[event]),
        refused("binding"),
        "a refused create_agent binds no agent"
    );
}

#[test]
fn send_refuses_set_state_for_a_workflow_that_the_owner_does_not_hold() {
    let p: Ports<Config> = Ports::new(0, 0);
    held(&p, 3, Some(WorkflowState::Approved));
    for state in AgentState::ALL {
        assert_eq!(p.send(&set(1, *state)), refused("no_workflow"), "{state}");
    }
    assert!(p.calls.borrow().is_empty(), "a refusal records no call");
    assert_eq!(
        p.send(&set(3, AgentState::Paused)).unwrap(),
        serde_json::json!({"state": "paused"})
    );
}

#[test]
fn send_refuses_set_state_for_a_proposed_or_dismissed_workflow() {
    let p: Ports<Config> = Ports::new(0, 0);
    held(&p, 1, Some(WorkflowState::Proposed));
    held(&p, 3, None);
    for state in AgentState::ALL {
        assert_eq!(p.send(&set(1, *state)), refused("state"), "{state}");
        assert_eq!(p.send(&set(3, *state)), refused("state"), "{state}");
    }
    assert!(p.calls.borrow().is_empty(), "a refusal records no call");
}

#[test]
fn history_answers_at_most_64_events_a_call() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 1, Some(2));
    let all: Vec<Event> = (1..=100).map(|s| event(s, 2)).collect();
    for body in all.chunks(8) {
        p.log(body).unwrap();
    }
    let first = p.history(&id(AgentId::parse, 2), 0).unwrap();
    assert_eq!(first.len(), 64);
    assert_eq!((first[0].seq.get(), first[63].seq.get()), (1, 64));
    let rest = p.history(&id(AgentId::parse, 2), 64).unwrap();
    assert_eq!(rest.len(), 36);
    assert_eq!((rest[0].seq.get(), rest[35].seq.get()), (65, 100));
}

#[test]
fn history_gives_the_logged_events_of_an_agent_above_a_seq() {
    let p: Ports<Config> = Ports::new(0, 0);
    listed(&p, 1, Some(2));
    listed(&p, 3, Some(4));
    p.log(&[event(1, 2), event(1, 4), event(2, 2), event(3, 2)])
        .unwrap();
    let h = p.history(&id(AgentId::parse, 2), 1).unwrap();
    assert_eq!(
        h.iter().map(|e| e.seq.get()).collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(p.history(&id(AgentId::parse, 4), 0).unwrap().len(), 1);
}

#[test]
fn owner_lists_what_the_test_put_in() {
    let p: Ports<Config> = Ports::new(0, 0);
    let w = FullWorkflow {
        id: id(WorkflowId::parse, 1),
        state: WorkflowState::Approved,
        category: Category::Email,
        site: Some("site5.example".to_owned()),
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: 20_700,
        agent: None,
    };
    p.workflows.borrow_mut().push(Workflow::Full(w.clone()));
    p.workflows
        .borrow_mut()
        .push(Workflow::Dismissed(DismissedWorkflow {
            id: id(WorkflowId::parse, 3),
            category: Category::Email,
            site: None,
        }));
    let listed = p.workflows().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0], Workflow::Full(w));
    assert_eq!(listed[1].id(), &id(WorkflowId::parse, 3));
    p.agents.borrow_mut().push(Agent {
        workflow: id(WorkflowId::parse, 1),
        agent: id(AgentId::parse, 2),
        config: Config::parse(BILL).unwrap(),
        state: AgentState::Approved,
        slots: vec![],
        session: false,
        last: None,
        unknown: vec![],
    });
    assert_eq!(p.agents().unwrap().len(), 1);
}

#[test]
fn analysis_tray_and_page_are_recorded() {
    let p: Ports<Config> = Ports::new(7, 0);
    p.defer(1234).unwrap();
    p.attention(2).unwrap();
    p.changed().unwrap();
    let order: Vec<&str> = p.calls.borrow().iter().map(|c| c.call.method()).collect();
    assert_eq!(order, vec!["defer", "attention", "changed"]);
    assert!(matches!(p.calls.borrow()[0].call, Call::Defer(1234)));
    assert!(matches!(p.calls.borrow()[1].call, Call::Attention(2)));
}

#[test]
fn run_events_hold_an_outcome_or_none() {
    let ended = RunEvent::Ended(Some(Outcome::Completed));
    let gone = RunEvent::Ended(None);
    assert_ne!(ended, gone);
    let progress = RunEvent::Progress(Progress::new(1, 3).unwrap());
    assert!(matches!(progress, RunEvent::Progress(p) if (p.step(), p.of()) == (1, 3)));
}
