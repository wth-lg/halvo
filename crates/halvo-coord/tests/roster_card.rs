use halvo_coord::guard::EffectState;
use halvo_coord::mem::{Config, Method, Ports};
use halvo_coord::ports::{Owner, PortError};
use halvo_coord::roster::{card, names, plain_site};
use halvo_coord::types::fixtures::{facts, State};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Body, Class, Event, Kept, Limits, Name, Outcome, PastEffect,
    PastRun, Reply, RunHistory, RunId, Workflow, WorkflowId,
};
use serde_json::Value;

const TRACKED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/tests/fixtures/coord");

/// A committed payment with the four kept fields of its class (A-A9.3), as the fixture of the
/// state `card` holds it.
fn payment() -> PastEffect {
    let kept = |name: &str, value: &str| Kept {
        name: Name::parse(name).unwrap(),
        value: vec![value.into()],
    };
    PastEffect {
        class: Class::Payment,
        state: EffectState::Committed,
        cause: None,
        kept: vec![
            kept("payee", "City Power"),
            kept("amount", "42.10 USD"),
            kept("instrument", "Card ending 4242"),
            kept("origin", "https://site1.example:443"),
        ],
    }
}

fn history(n: u32, outcome: Outcome) -> RunHistory {
    RunHistory {
        run: PastRun {
            run: RunId::parse(&format!("{:032x}", 0x300 + n)).unwrap(),
            day: 20_700 + n,
            outcome,
            unknown: 0,
        },
        effects: vec![payment()],
    }
}

fn owner() -> (Ports<Config>, WorkflowId, AgentId) {
    let f = facts(State::Card).remove(0);
    let ports: Ports<Config> = Ports::new(0, 20_760);
    ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(f.workflow.clone()));
    ports.agents.borrow_mut().push(Agent {
        workflow: f.workflow.id.clone(),
        agent: f.agent.clone(),
        config: f.config.clone(),
        state: AgentState::Active,
        slots: f.slots.clone(),
        session: false,
        last: f.runs.first().cloned(),
        unknown: vec![],
    });
    (ports, f.workflow.id.clone(), f.agent.clone())
}

#[test]
fn the_card_of_the_fixture_is_its_lines_and_the_runs_that_agent_history_gives() {
    let file: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{TRACKED}/state-card.json")).unwrap(),
    )
    .unwrap();
    let f = facts(State::Card).remove(0);
    let (ports, workflow, agent) = owner();
    for past in &f.runs {
        ports.histories.borrow_mut().push((
            agent.clone(),
            RunHistory {
                run: past.clone(),
                effects: if past.outcome == Outcome::Completed {
                    vec![payment()]
                } else {
                    vec![]
                },
            },
        ));
    }
    let read = ports.agent_history(&workflow, &agent, 5).unwrap();
    let all = facts(State::Card);
    let refs: Vec<_> = all.iter().collect();
    let name = &names(&refs, |w| w.site.as_deref().and_then(plain_site)).unwrap()[0];
    let got = card(&agent, &f.config, name, &read, &Limits::PRODUCT).unwrap();
    assert_eq!(serde_json::to_value(got).unwrap(), file["card"]);
}

#[test]
fn the_memory_owner_gives_the_newest_runs_first_and_at_most_the_count_asked() {
    let (ports, workflow, agent) = owner();
    for n in (0..7).rev() {
        ports
            .histories
            .borrow_mut()
            .push((agent.clone(), history(n, Outcome::Completed)));
    }
    let read = ports.agent_history(&workflow, &agent, 5).unwrap();
    assert_eq!(read.len(), 5);
    assert_eq!(read[0], history(6, Outcome::Completed));
    let f = facts(State::Card).remove(0);
    let limits = Limits {
        history_runs: 3,
        ..Limits::PRODUCT
    };
    let all = ports.agent_history(&workflow, &agent, 7).unwrap();
    let c = card(&agent, &f.config, "N", &all, &limits).unwrap();
    assert_eq!(c.runs.len(), 3);
    assert_eq!(c.runs[0].day, halvo_coord::words::day(20_706));
}

#[test]
fn the_memory_owner_refuses_a_workflow_that_is_gone_another_agent_and_a_refusal_set() {
    let (ports, workflow, agent) = owner();
    let other = AgentId::parse(&format!("{:032x}", 0x999)).unwrap();
    let gone = WorkflowId::parse(&format!("{:032x}", 0x998)).unwrap();
    let name = |e: PortError| match e {
        PortError::Refused(n) => n.as_str().to_owned(),
        other => format!("{other:?}"),
    };
    assert_eq!(
        name(ports.agent_history(&gone, &agent, 5).unwrap_err()),
        "no_workflow"
    );
    assert_eq!(
        name(ports.agent_history(&workflow, &other, 5).unwrap_err()),
        "binding"
    );
    ports.refuse(Method::AgentHistory, PortError::Busy);
    assert_eq!(
        ports.agent_history(&workflow, &agent, 5),
        Err(PortError::Busy)
    );
}

struct Silent;

impl Owner for Silent {
    type Config = Config;

    fn log(&self, _: &[Event]) -> Result<(), PortError> {
        Ok(())
    }

    fn history(&self, _: &AgentId, _: u32) -> Result<Vec<Event>, PortError> {
        Ok(vec![])
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        Ok(vec![])
    }

    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        Ok(vec![])
    }

    fn send(&self, _: &Body) -> Result<Reply, PortError> {
        Ok(Reply::Null)
    }
}

#[test]
fn an_owner_that_does_not_read_agent_history_answers_no_service() {
    let (_, workflow, agent) = owner();
    assert_eq!(
        Silent.agent_history(&workflow, &agent, 5),
        Err(PortError::NoService)
    );
}

#[test]
fn a_card_run_shows_its_committed_effects_alone_in_the_order_of_the_reply() {
    let f = facts(State::Card).remove(0);
    let mut h = history(1, Outcome::Completed);
    let mut refused = payment();
    refused.state = EffectState::Refused;
    h.effects = vec![refused, payment(), payment()];
    let c = card(&f.agent, &f.config, "N", &[h], &Limits::PRODUCT).unwrap();
    let effects = serde_json::to_value(&c.runs[0].effects).unwrap();
    assert_eq!(effects.as_array().map(Vec::len), Some(2), "{effects}");
    assert_eq!(effects[0]["key"], "run.effect.payment");
    let mut bad = payment();
    bad.kept.pop();
    let mut h = history(2, Outcome::Completed);
    h.effects = vec![bad];
    assert_eq!(
        card(&f.agent, &f.config, "N", &[h], &Limits::PRODUCT),
        Err(halvo_coord::types::Cause::Malformed)
    );
}
