#[allow(unused_imports)]
#[path = "coordinator_support.rs"]
mod support;

use halvo_coord::mem::{Call, Config, Method, Ports};
use halvo_coord::ports::{Overlap, Overlaps, Owner, PortError, Shared};
use halvo_coord::types::{
    Action, Agent, AgentId, AgentState, Body, Category, Cause, FullWorkflow, ItemId, ItemKind,
    Limits, Name, Param, Slot, What, Workflow, WorkflowId, WorkflowState, WorkflowTrigger,
};
use halvo_coord::Coordinator;
use support::Both;

const DAY: u32 = 20_000;
const T0: u64 = 1_700_000_000_000;
const ACCOUNT: &str = "me@home.test";
const ANN: &str = "ann@people.test";

/// The agent that the creation move makes for the approved workflow: the first id that the
/// memory random bytes give.
fn asking() -> AgentId {
    AgentId::parse(&format!("{:032x}", 1)).unwrap()
}

fn asking_flow() -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xfa)).unwrap()
}

fn other(n: usize) -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xb0 + n)).unwrap()
}

fn other_flow(n: usize) -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xe0 + n)).unwrap()
}

fn name(text: &str) -> Name {
    Name::parse(text).unwrap()
}

fn config(flow: &WorkflowId, agent: &AgentId) -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{flow}\",\"agent\":\"{agent}\",\"rev\":1,\"category\":\"email\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_mail\",\"playbook\":1}}"
    ))
    .unwrap()
}

fn workflow(id: WorkflowId, state: WorkflowState, agent: Option<AgentId>) -> Workflow {
    Workflow::Full(FullWorkflow {
        id,
        state,
        category: Category::Email,
        site: None,
        trigger: WorkflowTrigger::Manual,
        period: None,
        last: DAY - 2,
        agent,
    })
}

fn slot(n: &str, value: &str) -> Slot {
    Slot {
        name: name(n),
        value: vec![value.to_owned()],
    }
}

fn entry(flow: WorkflowId, agent: AgentId, slots: &[&str]) -> Agent<Config> {
    Agent {
        config: config(&flow, &agent),
        workflow: flow,
        agent,
        state: AgentState::Active,
        slots: slots.iter().map(|s| name(s)).collect(),
        session: true,
        last: None,
        unknown: vec![],
    }
}

/// Memory ports with an approved email workflow and no agent, whose agent the first tick makes
/// with a stored `account_address` and no `reply_senders`, so that its hold `setup` raises the
/// item, and `others` live email agents on the same mailbox, each listening to the one sender
/// given.
fn ports(others: &[&str]) -> Ports<Config> {
    let p = Ports::new(T0, DAY);
    p.workflows
        .borrow_mut()
        .push(workflow(asking_flow(), WorkflowState::Approved, None));
    p.agents
        .borrow_mut()
        .push(entry(asking_flow(), asking(), &["account_address"]));
    p.send(&Body::SetBindings {
        workflow: asking_flow(),
        agent: asking(),
        slots: vec![slot("account_address", ACCOUNT)],
    })
    .unwrap();
    for (n, sender) in others.iter().enumerate() {
        p.workflows.borrow_mut().push(workflow(
            other_flow(n),
            WorkflowState::Paused,
            Some(other(n)),
        ));
        p.agents.borrow_mut().push(entry(
            other_flow(n),
            other(n),
            &["account_address", "reply_senders"],
        ));
        p.send(&Body::SetBindings {
            workflow: other_flow(n),
            agent: other(n),
            slots: vec![
                slot("account_address", ACCOUNT),
                slot("reply_senders", sender),
            ],
        })
        .unwrap();
    }
    for n in 0..others.len() {
        set_listed(&p, &other_flow(n), WorkflowState::Active);
    }
    p
}

/// The owner of the coordinator tests over the memory ports `p`.
fn both(p: Ports<Config>) -> Both {
    let mut b = support::owner(&[], Limits::PRODUCT);
    b.ports = p;
    b
}

fn coordinator(p: Ports<Config>) -> (Coordinator<Both>, ItemId) {
    let c = Coordinator::new(both(p), Limits::PRODUCT);
    c.tick();
    let item = setup_item(&c).expect("the setup item of the new agent");
    (c, item)
}

fn setup_item(c: &Coordinator<Both>) -> Option<ItemId> {
    c.snapshot()
        .items
        .iter()
        .find(|i| i.kind() == ItemKind::Setup)
        .map(|i| i.item().clone())
}

fn step(c: &Coordinator<Both>, item: &ItemId) -> u32 {
    c.snapshot()
        .items
        .iter()
        .find(|i| i.item() == item)
        .map_or(0, |i| i.step())
}

fn answer(c: &Coordinator<Both>, item: &ItemId, senders: &str) -> Result<(), Cause> {
    c.setup_typed(item, Param::ReplySenders, senders, step(c, item))
}

fn set_listed(p: &Ports<Config>, flow: &WorkflowId, state: WorkflowState) {
    for w in p.workflows.borrow_mut().iter_mut() {
        if let Workflow::Full(f) = w {
            if &f.id == flow {
                f.state = state;
            }
        }
    }
}

fn mark(c: &Coordinator<Both>) -> usize {
    c.ports().ports.tried.borrow().len()
}

/// The `set_bindings` and `set_state` calls as tried since `from`, each with `refused` where
/// the owner refused it.
fn writes(c: &Coordinator<Both>, from: usize) -> Vec<String> {
    c.ports().ports.tried.borrow()[from..]
        .iter()
        .filter_map(|t| {
            let text = match &t.call {
                Call::Send(Body::SetBindings { slots, .. }) => format!(
                    "set_bindings {}",
                    slots
                        .iter()
                        .map(|s| s.value.join(","))
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
                Call::Send(Body::SetState { agent, state, .. }) => {
                    format!("set_state {} {}", &agent.as_str()[30..], state.name())
                }
                _ => return None,
            };
            Some(match &t.refused {
                Some(_) => format!("{text} refused"),
                None => text,
            })
        })
        .collect()
}

fn answered_rows(c: &Coordinator<Both>) -> usize {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| e.agent == asking() && matches!(e.what, What::ItemAnswered { .. }))
        .count()
}

fn named(entries: &[(AgentId, WorkflowId, &str)], more: bool) -> Shared {
    Shared::Named(Overlaps {
        entries: entries
            .iter()
            .map(|(agent, workflow, sender)| Overlap {
                workflow: workflow.clone(),
                agent: agent.clone(),
                senders: vec![(*sender).to_owned()],
            })
            .collect(),
        more,
    })
}

#[test]
fn a_refused_body_with_overlap_sends_the_owed_paused_and_the_body_once_more_and_names_the_other_agent(
) {
    let p = ports(&[ANN]);
    p.workflows.borrow_mut().push(workflow(
        other_flow(5),
        WorkflowState::Active,
        Some(other(5)),
    ));
    p.agents
        .borrow_mut()
        .push(entry(other_flow(5), other(5), &["reply_senders"]));
    let (c, item) = coordinator(p);
    c.ports().ports.refuse(Method::Send, PortError::Busy);
    assert_eq!(c.control(&other(5), Action::SwitchOff), Ok(()));
    c.ports().ports.clear_refusals();
    let before = step(&c, &item);
    let m = mark(&c);
    assert_eq!(answer(&c, &item, ANN), Ok(()));
    assert_eq!(
        writes(&c, m),
        [
            "set_bindings ann@people.test refused",
            "set_state b5 paused",
            "set_bindings ann@people.test refused"
        ],
        "the owed paused and the body once more, before the sentence"
    );
    assert_eq!(
        c.setup_overlap(&item),
        Some(named(&[(other(0), other_flow(0), ANN)], false))
    );
    assert_eq!(
        setup_item(&c),
        Some(item.clone()),
        "the item takes answers again"
    );
    assert_eq!(step(&c, &item), before + 1, "it asks again");
    assert_eq!(answered_rows(&c), 0, "nothing stored");
}

#[test]
fn the_kept_body_goes_once_more_after_a_switch_off_of_the_named_agent_and_the_item_closes() {
    let (c, item) = coordinator(ports(&[ANN]));
    answer(&c, &item, ANN).unwrap();
    assert!(c.setup_overlap(&item).is_some());
    let m = mark(&c);
    assert_eq!(c.control(&other(0), Action::SwitchOff), Ok(()));
    assert_eq!(
        writes(&c, m),
        [
            "set_state b0 paused",
            "set_bindings ann@people.test",
            "set_state 01 active"
        ],
        "the kept body, not typed again, then the rule of active"
    );
    assert_eq!(
        setup_item(&c),
        None,
        "the item closed at the acknowledgement"
    );
    assert_eq!(answered_rows(&c), 1);
    assert_eq!(
        c.ports().ports.slots(&asking_flow(), &asking()),
        vec![slot("account_address", ACCOUNT), slot("reply_senders", ANN)]
    );
}

#[test]
fn a_switch_off_of_an_agent_that_the_sentence_does_not_name_sends_nothing_more() {
    let (c, item) = coordinator(ports(&[ANN, "zed@people.test"]));
    answer(&c, &item, ANN).unwrap();
    let m = mark(&c);
    assert_eq!(c.control(&other(1), Action::SwitchOff), Ok(()));
    assert_eq!(writes(&c, m), ["set_state b1 paused"]);
    assert_eq!(setup_item(&c), Some(item));
}

#[test]
fn a_read_that_fails_keeps_the_refusal_shown_and_a_tick_asks_it_again_while_waiting_can_change_it()
{
    let (c, item) = coordinator(ports(&[ANN]));
    c.ports().ports.refuse(Method::Overlaps, PortError::Busy);
    answer(&c, &item, ANN).unwrap();
    assert_eq!(c.setup_overlap(&item), Some(Shared::Unread { again: true }));
    assert_eq!(
        setup_item(&c),
        Some(item.clone()),
        "the refusal stays shown"
    );
    let reads = c.ports().ports.overlap_reads.borrow().len();
    c.tick();
    assert_eq!(
        c.ports().ports.overlap_reads.borrow().len(),
        reads + 1,
        "asked again once at a tick"
    );
    assert_eq!(c.setup_overlap(&item), Some(Shared::Unread { again: true }));
    c.ports().ports.clear_refusals();
    c.tick();
    assert_eq!(
        c.setup_overlap(&item),
        Some(named(&[(other(0), other_flow(0), ANN)], false))
    );
    let reads = c.ports().ports.overlap_reads.borrow().len();
    c.tick();
    assert_eq!(
        c.ports().ports.overlap_reads.borrow().len(),
        reads,
        "no read once one answered"
    );
}

#[test]
fn a_read_refused_by_a_name_that_waiting_cannot_change_is_not_asked_again_at_a_tick() {
    let (c, item) = coordinator(ports(&[ANN]));
    c.ports()
        .ports
        .refuse(Method::Overlaps, PortError::Refused(name("malformed")));
    answer(&c, &item, ANN).unwrap();
    assert_eq!(
        c.setup_overlap(&item),
        Some(Shared::Unread { again: false })
    );
    let reads = c.ports().ports.overlap_reads.borrow().len();
    c.tick();
    assert_eq!(c.ports().ports.overlap_reads.borrow().len(), reads);
}

#[test]
fn a_read_at_a_tick_that_answers_with_no_entry_takes_the_sentence_away_and_sends_the_kept_body_once(
) {
    let (c, item) = coordinator(ports(&[ANN]));
    c.ports()
        .ports
        .refuse(Method::Overlaps, PortError::NoService);
    answer(&c, &item, ANN).unwrap();
    c.ports().ports.clear_refusals();
    set_listed(&c.ports().ports, &other_flow(0), WorkflowState::Paused);
    let m = mark(&c);
    c.tick();
    assert_eq!(
        writes(&c, m)
            .iter()
            .filter(|w| w.starts_with("set_bindings"))
            .collect::<Vec<_>>(),
        ["set_bindings ann@people.test"]
    );
    assert_eq!(setup_item(&c), None);
    assert_eq!(answered_rows(&c), 1);
}

#[test]
fn the_sentence_says_more_where_more_than_sixteen_agents_share_people() {
    let senders = [ANN; 17];
    let (c, item) = coordinator(ports(&senders));
    answer(&c, &item, ANN).unwrap();
    match c.setup_overlap(&item) {
        Some(Shared::Named(o)) => {
            assert_eq!(o.entries.len(), 16);
            assert!(o.more);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_new_answer_of_the_person_sends_a_new_body_and_ends_the_kept_one() {
    let (c, item) = coordinator(ports(&[ANN]));
    answer(&c, &item, ANN).unwrap();
    let m = mark(&c);
    assert_eq!(answer(&c, &item, "carol@people.test"), Ok(()));
    assert_eq!(
        writes(&c, m),
        ["set_bindings carol@people.test", "set_state 01 active"]
    );
    assert_eq!(setup_item(&c), None);
    assert_eq!(c.setup_overlap(&item), None);
}

/// Memory ports in which the agent of the asking workflow holds the sender in its stored
/// bindings and stands switched off by the list alone (the derived hold `off`), beside a live
/// agent on the same mailbox that shares the sender.
fn ports_on() -> Ports<Config> {
    let p = Ports::new(T0, DAY);
    p.workflows.borrow_mut().push(workflow(
        asking_flow(),
        WorkflowState::Paused,
        Some(asking()),
    ));
    p.agents.borrow_mut().push(entry(
        asking_flow(),
        asking(),
        &["account_address", "reply_senders"],
    ));
    p.workflows.borrow_mut().push(workflow(
        other_flow(0),
        WorkflowState::Paused,
        Some(other(0)),
    ));
    p.agents.borrow_mut().push(entry(
        other_flow(0),
        other(0),
        &["account_address", "reply_senders"],
    ));
    for (flow, agent) in [(asking_flow(), asking()), (other_flow(0), other(0))] {
        p.send(&Body::SetBindings {
            workflow: flow,
            agent,
            slots: vec![slot("account_address", ACCOUNT), slot("reply_senders", ANN)],
        })
        .unwrap();
    }
    set_listed(&p, &other_flow(0), WorkflowState::Active);
    p
}

#[test]
fn a_switch_on_refused_with_overlap_holds_the_names_that_its_read_gave() {
    let c = Coordinator::new(both(ports_on()), Limits::PRODUCT);
    c.tick();
    assert_eq!(
        c.control(&asking(), Action::SwitchOn),
        Err(Cause::NoService)
    );
    assert_eq!(
        c.switch_on_overlap(&asking()),
        Some(named(&[(other(0), other_flow(0), ANN)], false))
    );
    set_listed(&c.ports().ports, &other_flow(0), WorkflowState::Paused);
    assert_eq!(c.control(&asking(), Action::SwitchOn), Ok(()));
    assert_eq!(c.switch_on_overlap(&asking()), None);
}

#[test]
fn a_switch_on_refused_with_overlap_whose_read_did_not_answer_holds_the_half_that_is_known() {
    let c = Coordinator::new(both(ports_on()), Limits::PRODUCT);
    c.tick();
    c.ports().ports.refuse(Method::Overlaps, PortError::Busy);
    assert_eq!(
        c.control(&asking(), Action::SwitchOn),
        Err(Cause::NoService)
    );
    assert_eq!(
        c.switch_on_overlap(&asking()),
        Some(Shared::Unread { again: true })
    );
}

#[test]
fn a_switch_off_of_any_agent_while_the_sentence_names_nobody_sends_the_kept_body_once_more() {
    let (c, item) = coordinator(ports(&[ANN, "zed@people.test"]));
    c.ports().ports.refuse(Method::Overlaps, PortError::Busy);
    answer(&c, &item, ANN).unwrap();
    c.ports().ports.clear_refusals();
    assert_eq!(c.setup_overlap(&item), Some(Shared::Unread { again: true }));
    let m = mark(&c);
    assert_eq!(c.control(&other(1), Action::SwitchOff), Ok(()));
    assert_eq!(
        writes(&c, m),
        [
            "set_state b1 paused",
            "set_bindings ann@people.test refused"
        ],
        "the kept body once more, refused again while b0 still shares ann"
    );
    assert_eq!(
        c.setup_overlap(&item),
        Some(named(&[(other(0), other_flow(0), ANN)], false)),
        "the names of the read that followed the newest refusal"
    );
}

#[test]
fn a_switch_off_of_the_asking_agent_sends_no_kept_body() {
    let (c, item) = coordinator(ports(&[ANN]));
    c.ports().ports.refuse(Method::Overlaps, PortError::Busy);
    answer(&c, &item, ANN).unwrap();
    c.ports().ports.clear_refusals();
    let m = mark(&c);
    let _ = c.control(&asking(), Action::SwitchOff);
    assert!(
        !writes(&c, m).iter().any(|w| w.starts_with("set_bindings")),
        "{:?}",
        writes(&c, m)
    );
}

fn listed_state(c: &Coordinator<Both>, flow: &WorkflowId) -> Option<WorkflowState> {
    c.ports()
        .ports
        .workflows
        .borrow()
        .iter()
        .find_map(|w| match w {
            Workflow::Full(f) if &f.id == flow => Some(f.state),
            _ => None,
        })
}

/// Ruling RD-160 (2), part 1, from lens 3's case `adv3_1`: the person switches off b0, which the
/// sentence names, and the owner answers that switch-off's `paused` with `busy` once. Every
/// `paused` that the board owes goes before the kept body, as before the once-more send of a
/// refusal (I3.5 (b)): here b0's `paused` again, acknowledged, and then the kept body, which the
/// owner takes, so the item closes in the control's move.
#[test]
fn a_switch_off_of_the_named_agent_whose_paused_is_busy_sends_the_owed_paused_before_the_kept_body()
{
    let (c, item) = coordinator(ports(&[ANN]));
    answer(&c, &item, ANN).unwrap();
    let m = mark(&c);
    c.ports().paused_busy.set(1);
    assert_eq!(c.control(&other(0), Action::SwitchOff), Ok(()));
    assert_eq!(
        c.ports().refused_paused.borrow().clone(),
        ["set_state b0 paused busy"]
    );
    assert_eq!(
        writes(&c, m),
        [
            "set_state b0 paused",
            "set_bindings ann@people.test",
            "set_state 01 active"
        ]
    );
    assert_eq!(
        listed_state(&c, &other_flow(0)),
        Some(WorkflowState::Paused)
    );
    assert_eq!(setup_item(&c), None);
    assert_eq!(c.setup_overlap(&item), None);
}

/// Ruling RD-160 (2), part 2, the verify step's case with two `busy` answers to `paused`: the
/// freed body goes only once the `paused` of the task that the person switched off is
/// acknowledged. Until then the item keeps its body and its sentence and sends nothing; the
/// first tick that brings b0 `paused` sends the body, and the item closes.
#[test]
fn freed_body_goes_again_once_the_named_agent_is_listed_paused() {
    let (c, item) = coordinator(ports(&[ANN]));
    answer(&c, &item, ANN).unwrap();
    let named_b0 = c.setup_overlap(&item);
    let m = mark(&c);
    c.ports().paused_busy.set(2);
    let out = c.control(&other(0), Action::SwitchOff);
    println!("VFY-5 switch-off answered: {out:?}");
    println!(
        "VFY-5 refused by the stand-in: {:?}",
        c.ports().refused_paused.borrow()
    );
    println!("VFY-5 writes of the switch-off move: {:?}", writes(&c, m));
    println!(
        "VFY-5 b0 in the owner's list after the move: {:?}",
        listed_state(&c, &other_flow(0))
    );
    println!(
        "VFY-5 item open after the move: {}",
        setup_item(&c).is_some()
    );
    println!("VFY-5 held after the move: {:?}", c.setup_overlap(&item));
    assert_eq!(out, Ok(()));
    assert_eq!(
        writes(&c, m),
        Vec::<String>::new(),
        "no body while b0 is listed active"
    );
    assert_eq!(
        setup_item(&c),
        Some(item.clone()),
        "the item waits with its body"
    );
    assert_eq!(
        c.setup_overlap(&item),
        named_b0,
        "and with the sentence of the newest read"
    );
    let m2 = mark(&c);
    for _ in 0..4 {
        c.tick();
    }
    println!("VFY-5 writes of four ticks: {:?}", writes(&c, m2));
    println!(
        "VFY-5 b0 in the owner's list after them: {:?}",
        listed_state(&c, &other_flow(0))
    );
    println!("VFY-5 item open after them: {}", setup_item(&c).is_some());
    println!(
        "VFY-5 held for the page after them: {:?}",
        c.setup_overlap(&item)
    );
    assert_eq!(
        writes(&c, m2),
        [
            "set_state b0 paused",
            "set_bindings ann@people.test",
            "set_state 01 active"
        ]
    );
    assert_eq!(
        listed_state(&c, &other_flow(0)),
        Some(WorkflowState::Paused)
    );
    assert_eq!(setup_item(&c), None, "the item closes once b0 is off");
    assert_eq!(c.setup_overlap(&item), None);
}

/// A body that waits for the `paused` of the task that the person switched off goes at the first
/// tick at which that task's workflow is no longer in the owner's list, since a task off the list
/// is live no more (I3.5 (e): a delete of a named task frees the body as a switch-off does).
#[test]
fn a_waiting_body_goes_when_the_switched_off_task_leaves_the_list() {
    let (c, item) = coordinator(ports(&[ANN]));
    answer(&c, &item, ANN).unwrap();
    c.ports().paused_busy.set(2);
    assert_eq!(c.control(&other(0), Action::SwitchOff), Ok(()));
    assert_eq!(setup_item(&c), Some(item.clone()));
    c.ports().paused_busy.set(u32::MAX);
    c.ports()
        .ports
        .workflows
        .borrow_mut()
        .retain(|w| w.id() != &other_flow(0));
    let m = mark(&c);
    c.tick();
    assert_eq!(
        writes(&c, m)
            .into_iter()
            .filter(|w| w.starts_with("set_bindings"))
            .collect::<Vec<_>>(),
        ["set_bindings ann@people.test"]
    );
    assert_eq!(setup_item(&c), None);
}

/// The same wait where the sentence names nobody: the person switches off b1, which the read did
/// not name, and the owner answers `busy` to the first two `paused` of b1. The move sends no body;
/// the tick that brings b1 `paused` sends it once, and the owner refuses it again while b0 still
/// shares ann, so the item names b0 from the read that followed.
#[test]
fn a_switch_off_while_the_sentence_names_nobody_waits_for_the_paused_of_the_task_switched_off() {
    let (c, item) = coordinator(ports(&[ANN, "zed@people.test"]));
    c.ports().ports.refuse(Method::Overlaps, PortError::Busy);
    answer(&c, &item, ANN).unwrap();
    c.ports().ports.clear_refusals();
    assert_eq!(c.setup_overlap(&item), Some(Shared::Unread { again: true }));
    let m = mark(&c);
    c.ports().paused_busy.set(2);
    assert_eq!(c.control(&other(1), Action::SwitchOff), Ok(()));
    assert_eq!(
        c.ports().refused_paused.borrow().clone(),
        ["set_state b1 paused busy", "set_state b1 paused busy"]
    );
    assert_eq!(
        writes(&c, m),
        Vec::<String>::new(),
        "no body while b1 is listed active"
    );
    assert_eq!(c.setup_overlap(&item), Some(Shared::Unread { again: true }));
    let m2 = mark(&c);
    c.tick();
    assert_eq!(
        writes(&c, m2),
        [
            "set_state b1 paused",
            "set_bindings ann@people.test refused"
        ]
    );
    assert_eq!(
        c.setup_overlap(&item),
        Some(named(&[(other(0), other_flow(0), ANN)], false))
    );
    let m3 = mark(&c);
    c.tick();
    assert_eq!(writes(&c, m3), Vec::<String>::new(), "the body went once");
}

/// Lens 3's case `adv3_6`, which reads after RD-160 (2) as it read before: a switch-off of the
/// named agent under a failing clock sends nothing in its move, and the ticks that follow send
/// b0's `paused` and then the kept body, which the owner takes.
#[test]
fn a_switch_off_under_a_failing_clock_sends_the_kept_body_after_the_paused_at_a_tick() {
    let (c, item) = coordinator(ports(&[ANN]));
    answer(&c, &item, ANN).unwrap();
    let m = mark(&c);
    c.ports().clock_fails.set(true);
    let out = c.control(&other(0), Action::SwitchOff);
    c.ports().clock_fails.set(false);
    assert_eq!(out, Ok(()));
    assert_eq!(writes(&c, m), Vec::<String>::new());
    assert!(c.setup_overlap(&item).is_some());
    let m2 = mark(&c);
    for _ in 0..3 {
        c.tick();
    }
    assert_eq!(
        writes(&c, m2),
        [
            "set_state b0 paused",
            "set_bindings ann@people.test",
            "set_state 01 active"
        ]
    );
    assert_eq!(setup_item(&c), None);
    assert_eq!(c.setup_overlap(&item), None);
}

/// `ports_on` with the asking agent held by the hold `failures` instead of `off` (I5.4): listed
/// `paused` with a stored `hold_set` of the cause `failures`, so that a Start now ends that hold
/// and sends `active` first, beside b0 listed `active` with the same sender.
fn ports_failures() -> Ports<Config> {
    let p = ports_on();
    p.log(&[halvo_coord::types::Event {
        workflow: asking_flow(),
        agent: asking(),
        seq: std::num::NonZeroU32::new(1).unwrap(),
        day: DAY - 1,
        at: 0,
        what: What::HoldSet {
            cause: halvo_coord::types::HoldSetCause::Failures,
        },
    }])
    .unwrap();
    p
}

/// Ruling RD-160 (3), from the verify step: a Start now that the owner refuses with `overlap`
/// holds the read as a Switch on does, and a move of the person on that agent that the owner
/// does not refuse so takes it away; a move on another agent leaves it.
#[test]
fn a_start_refused_with_overlap_holds_the_read_and_a_start_that_goes_clears_it() {
    let c = Coordinator::new(both(ports_failures()), Limits::PRODUCT);
    c.tick();
    let reads = c.ports().ports.overlap_reads.borrow().len();
    let m = mark(&c);
    assert_eq!(c.control(&asking(), Action::RunNow), Err(Cause::NoService));
    assert_eq!(writes(&c, m), ["set_state 01 active refused"]);
    assert_eq!(
        c.ports().ports.overlap_reads.borrow().len(),
        reads + 1,
        "one read"
    );
    let b0 = Some(named(&[(other(0), other_flow(0), ANN)], false));
    assert_eq!(c.switch_on_overlap(&asking()), b0);
    assert_eq!(c.control(&other(0), Action::SwitchOff), Ok(()));
    assert_eq!(
        listed_state(&c, &other_flow(0)),
        Some(WorkflowState::Paused)
    );
    assert_eq!(c.switch_on_overlap(&asking()), b0, "a move on b0 leaves it");
    assert_eq!(c.control(&asking(), Action::RunNow), Ok(()));
    assert_eq!(
        listed_state(&c, &asking_flow()),
        Some(WorkflowState::Active)
    );
    assert_eq!(c.switch_on_overlap(&asking()), None);
}

/// Ruling RD-160 (3), a move of the person on the agent that sends no state: a Stop of the agent
/// with no live run, which the owner does not refuse, takes away the read that a refused Start
/// now held.
#[test]
fn a_stop_that_sends_no_state_takes_the_read_of_a_refused_start_away() {
    let c = Coordinator::new(both(ports_failures()), Limits::PRODUCT);
    c.tick();
    assert_eq!(c.control(&asking(), Action::RunNow), Err(Cause::NoService));
    assert!(c.switch_on_overlap(&asking()).is_some());
    let m = mark(&c);
    assert_eq!(c.control(&asking(), Action::Stop), Ok(()));
    assert_eq!(
        writes(&c, m),
        Vec::<String>::new(),
        "the stop sends no state"
    );
    assert_eq!(c.switch_on_overlap(&asking()), None);
}

fn bindings_sent(c: &Coordinator<Both>, from: usize) -> Vec<String> {
    writes(c, from)
        .into_iter()
        .filter(|w| w.starts_with("set_bindings"))
        .collect()
}

/// The verify step's row V6, I3.5 (d): "a read that answers with no entry takes the sentence
/// away and sends the kept body once". The tick's read answers with no entry and takes the
/// sentence away before the kept body goes, whatever the owner answers to that body: here
/// `busy`, so the item owes its body and holds nothing to show; the next tick sends the owed body
/// and the item closes.
#[test]
fn a_tick_read_with_no_entry_takes_the_sentence_away_also_when_the_body_gets_busy() {
    let (c, item) = coordinator(ports(&[ANN]));
    c.ports()
        .ports
        .refuse(Method::Overlaps, PortError::NoService);
    answer(&c, &item, ANN).unwrap();
    c.ports().ports.clear_refusals();
    assert_eq!(c.setup_overlap(&item), Some(Shared::Unread { again: true }));
    set_listed(&c.ports().ports, &other_flow(0), WorkflowState::Paused);
    let reads = c.ports().ports.overlap_reads.borrow().len();
    c.ports().ports.refuse(Method::Send, PortError::Busy);
    let m = mark(&c);
    c.tick();
    c.ports().ports.clear_refusals();
    assert_eq!(
        c.ports().ports.overlap_reads.borrow().len(),
        reads + 1,
        "one read at the tick"
    );
    assert_eq!(
        bindings_sent(&c, m),
        ["set_bindings ann@people.test refused"]
    );
    assert_eq!(setup_item(&c), Some(item.clone()), "the item owes its body");
    assert_eq!(c.setup_overlap(&item), None);
    let m2 = mark(&c);
    c.tick();
    assert_eq!(bindings_sent(&c, m2), ["set_bindings ann@people.test"]);
    assert_eq!(setup_item(&c), None);
    assert_eq!(c.setup_overlap(&item), None);
}

/// Ruling RD-160 (2) owes the `paused` before a freed body alone: a switch-off of the person
/// while no set-up keeps a body sends its own `paused` and no `paused` that another agent owes,
/// which the next tick sends, as before the round.
#[test]
fn a_switch_off_while_no_set_up_keeps_a_body_sends_no_paused_of_another_agent_in_its_move() {
    let (c, _item) = coordinator(ports(&[ANN, "zed@people.test"]));
    c.ports().paused_busy.set(1);
    assert_eq!(c.control(&other(1), Action::SwitchOff), Ok(()));
    assert_eq!(
        c.ports().refused_paused.borrow().clone(),
        ["set_state b1 paused busy"]
    );
    assert_eq!(
        listed_state(&c, &other_flow(1)),
        Some(WorkflowState::Active)
    );
    let m = mark(&c);
    assert_eq!(c.control(&other(0), Action::SwitchOff), Ok(()));
    assert_eq!(writes(&c, m), ["set_state b0 paused"]);
    let m2 = mark(&c);
    c.tick();
    assert_eq!(writes(&c, m2), ["set_state b1 paused"]);
}

/// Ruling RD-160 (2): a waiting body stays while the tick's `paused` of the task switched off
/// gets `busy` too, and goes at the tick that brings that `paused`.
#[test]
fn a_waiting_body_stays_while_the_ticks_paused_gets_busy_too() {
    let (c, item) = coordinator(ports(&[ANN]));
    answer(&c, &item, ANN).unwrap();
    c.ports().paused_busy.set(3);
    assert_eq!(c.control(&other(0), Action::SwitchOff), Ok(()));
    let m = mark(&c);
    c.tick();
    assert_eq!(
        writes(&c, m),
        Vec::<String>::new(),
        "no body while b0 is listed active"
    );
    assert_eq!(setup_item(&c), Some(item.clone()));
    let m2 = mark(&c);
    c.tick();
    assert_eq!(
        writes(&c, m2),
        [
            "set_state b0 paused",
            "set_bindings ann@people.test",
            "set_state 01 active"
        ]
    );
    assert_eq!(setup_item(&c), None);
}

/// Ruling RD-160 (2): a waiting body goes at the first tick at which the workflow of the task
/// switched off stands dismissed in the owner's list, whose stand-in is live no more.
#[test]
fn a_waiting_body_goes_when_the_switched_off_task_is_dismissed() {
    let (c, item) = coordinator(ports(&[ANN]));
    answer(&c, &item, ANN).unwrap();
    c.ports().paused_busy.set(u32::MAX);
    assert_eq!(c.control(&other(0), Action::SwitchOff), Ok(()));
    assert_eq!(setup_item(&c), Some(item.clone()));
    for w in c.ports().ports.workflows.borrow_mut().iter_mut() {
        if w.id() == &other_flow(0) {
            *w = Workflow::Dismissed(halvo_coord::types::DismissedWorkflow {
                id: other_flow(0),
                category: Category::Email,
                site: None,
            });
        }
    }
    let m = mark(&c);
    c.tick();
    assert_eq!(bindings_sent(&c, m), ["set_bindings ann@people.test"]);
    assert_eq!(setup_item(&c), None);
}

/// r4 U3.13, the first sentence (I3.5 (c)): where the owner refuses a set-up's answers with
/// `overlap`, the item of the published state says why in the place of the line
/// `item.setup.not_taken`, in the form that the read after the refusal gives: the other agent by
/// the name of its row with the senders that both would read; that further agents share senders
/// where the read says more; and nobody where the read did not answer.
#[test]
fn u313_a_set_up_refused_with_overlap_says_why_in_the_form_that_its_read_gives() {
    const BOB: &str = "bob@people.test";
    let line = |c: &Coordinator<Both>, item: &ItemId| -> Vec<Result<String, Cause>> {
        let s = c.snapshot();
        let item = s.items.iter().find(|i| i.item() == item).expect("the item");
        item.fields()
            .iter()
            .filter(|f| f.from == halvo_coord::types::FieldFrom::Halvo)
            .map(|f| Ok(f.label.clone()))
            .collect()
    };
    let name_of = |c: &Coordinator<Both>, agent: &AgentId| -> String {
        let s = c.snapshot();
        s.rows.iter().find(|r| &r.agent == agent).unwrap().name.clone()
    };
    let say = |key: &str, pairs: &[(&str, &str)]| halvo_coord::words::text(key, pairs);

    let (c, item) = coordinator(ports(&[ANN]));
    *c.ports().overlaps_answer.borrow_mut() = Some(Overlaps {
        entries: vec![Overlap {
            workflow: other_flow(0),
            agent: other(0),
            senders: vec![ANN.to_owned(), BOB.to_owned()],
        }],
        more: false,
    });
    answer(&c, &item, ANN).unwrap();
    let name = name_of(&c, &other(0));
    let senders = format!("{ANN}, {BOB}");
    let named = say(
        "item.setup.overlap",
        &[("name", &name), ("senders", &senders)],
    );
    assert_eq!(line(&c, &item), [named.clone()], "the read names one agent");
    let named = named.unwrap();
    assert!(
        named.contains(&name) && named.contains(ANN) && named.contains(BOB),
        "{named}"
    );

    let (c, item) = coordinator(ports(&[ANN; 17]));
    answer(&c, &item, ANN).unwrap();
    let name = name_of(&c, &other(0));
    assert_eq!(
        line(&c, &item),
        [say(
            "item.setup.overlap_more",
            &[("name", &name), ("senders", ANN)]
        )],
        "the read says more"
    );

    let (c, item) = coordinator(ports(&[ANN]));
    c.ports().ports.refuse(Method::Overlaps, PortError::Busy);
    answer(&c, &item, ANN).unwrap();
    let name = name_of(&c, &other(0));
    let unread = say("item.setup.overlap_unread", &[]);
    assert_eq!(
        line(&c, &item),
        [unread.clone()],
        "the read did not answer"
    );
    assert!(!unread.unwrap().contains(&name), "it names nobody");
}
