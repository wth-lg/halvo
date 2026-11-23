use super::*;
use crate::mem::{Call, Config, Method, Ports};
use crate::types::{
    Agent, AgentState, Body, Category, Control, Name, SpendLimit, Tier, Workflow, WorkflowState,
    WorkflowTrigger,
};
use crate::types::{ModelUse, Refusal as Refused};
use std::num::NonZeroU32;

const DAY: u32 = 20_000;
const T0: u64 = 1_700_000_000_000;
const USE_NOT_FINISHED: &str =
    "Halvo could not finish this. If this agent still shows Use an AI model, choose it again.";

fn agent() -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa0)).unwrap()
}

fn flow() -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xf0)).unwrap()
}

/// What the app hands of a host with one model, where a key is saved and a limit stands, with
/// `stands`.
fn ready(stands: bool) -> ModelUse {
    ModelUse {
        host: "hosted-model.test".to_owned(),
        models: "test-model-small".to_owned(),
        stands,
        key: Some(true),
        limit: SpendLimit::Stands {
            amount: "20.00 USD".to_owned(),
            period: "month".to_owned(),
        },
    }
}

/// A coordinator after its first tick over the stand-in owner alone, with one manual agent of
/// `tier` whose configuration hands `model`.
fn world(tier: Tier, model: ModelUse) -> Coordinator<Ports<Config>> {
    let ports: Ports<Config> = Ports::with_limits(T0, DAY, Limits::PRODUCT);
    let mut config = Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"calendar_cleanup\",\"tier\":\"{}\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_calendar\",\"playbook\":1}}",
        flow(),
        agent(),
        tier.name()
    ))
    .unwrap();
    config.set_model(Some(model));
    ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(FullWorkflow {
            id: flow(),
            state: WorkflowState::Active,
            category: Category::CalendarCleanup,
            site: None,
            trigger: WorkflowTrigger::Manual,
            period: NonZeroU32::new(2),
            last: DAY - 2,
            agent: Some(agent()),
        }));
    ports.agents.borrow_mut().push(Agent {
        workflow: flow(),
        agent: agent(),
        config,
        state: AgentState::Active,
        slots: Vec::new(),
        session: false,
        window: false,
        last: None,
        unknown: Vec::new(),
        refused_run: None,
        last_cause: None,
    });
    let c = Coordinator::new(ports, Limits::PRODUCT);
    c.tick();
    c
}

fn row(c: &Coordinator<Ports<Config>>) -> Row {
    c.snapshot()
        .rows
        .into_iter()
        .find(|r| r.agent == agent())
        .expect("the agent's row")
}

fn offers(c: &Coordinator<Ports<Config>>, control: Control) -> bool {
    row(c).controls.iter().any(|x| x.control == control)
}

fn mark(c: &Coordinator<Ports<Config>>) -> usize {
    c.ports().calls.borrow().len()
}

/// The bodies of the yes and of the way back that the owner stored since `from`.
fn sets(c: &Coordinator<Ports<Config>>, from: usize) -> Vec<Call> {
    c.ports().calls.borrow()[from..]
        .iter()
        .map(|t| t.call.clone())
        .filter(|call| {
            matches!(
                call,
                Call::Send(Body::AgreeHosted { .. } | Body::WithdrawHosted { .. })
            )
        })
        .collect()
}

fn store() -> PortError {
    PortError::Refused(Name::parse("store").unwrap())
}

/// r4 U3.17b sentence 4, the verifier's first claim: the row that offers Use an AI model
/// carries its question, and neither the row nor its question sends anything, so that Not now
/// and a closed question send none; the answer Allow for this agent, `agents_control` with
/// `use_model`, sends ONE body, the yes `Body::AgreeHosted` of the agent. Where the owner
/// refuses that body with `store` or no reply comes, the reply says that Halvo could not
/// finish this, nothing is stored, and the row still offers Use an AI model.
#[test]
fn the_yes_of_use_model_sends_one_body_and_a_refusal_says_that_halvo_could_not_finish_this() {
    let c = world(Tier::OnDevice, ready(false));
    assert!(row(&c).ask_use_model.is_some(), "the question");
    assert!(offers(&c, Control::UseModel));
    assert!(
        sets(&c, 0).is_empty(),
        "the row and its question send nothing"
    );
    let m = mark(&c);
    assert_eq!(c.control(&agent(), Action::UseModel), Ok(()));
    assert_eq!(
        sets(&c, m),
        [Call::Send(Body::AgreeHosted {
            workflow: flow(),
            agent: agent()
        })]
    );
    for error in [store(), PortError::NoService] {
        let c = world(Tier::OnDevice, ready(false));
        c.ports().refuse(Method::Send, error.clone());
        let m = mark(&c);
        let refusal: Refused = c
            .control_reply(&agent(), Action::UseModel)
            .expect_err("refused");
        assert_eq!(refusal.cause(), Cause::NoService, "{error:?}");
        assert_eq!(refusal.text(), Ok(USE_NOT_FINISHED.to_owned()), "{error:?}");
        assert!(sets(&c, m).is_empty(), "{error:?}: nothing stored");
        assert!(
            offers(&c, Control::UseModel),
            "{error:?}: the row keeps the control"
        );
    }
}

/// r4 U3.17b sentence 3: where no key is saved, where no limit is set, and where the key's state
/// or the count's state could not be read, the row that offers Use an AI model carries ONE
/// sentence in place of the question and no question, and the action of the yes is refused with
/// `state` and sends nothing.
#[test]
fn with_no_key_no_limit_or_a_state_that_does_not_read_the_row_says_one_sentence_and_sends_no_yes() {
    let first = "Save a key and set a spending limit under AI model first.";
    let stands = ready(false).limit;
    let rows = [
        ("no key saved", Some(false), stands.clone(), first),
        ("no limit set", Some(true), SpendLimit::NotSet, first),
        ("the key's state", None, stands, USE_NOT_FINISHED),
        (
            "the count's state",
            Some(true),
            SpendLimit::Unread,
            USE_NOT_FINISHED,
        ),
    ];
    for (case, key, limit, sentence) in rows {
        let c = world(
            Tier::OnDevice,
            ModelUse {
                key,
                limit,
                ..ready(false)
            },
        );
        let r = row(&c);
        assert!(offers(&c, Control::UseModel), "{case}");
        assert_eq!(r.ask_use_model, None, "{case}: no question");
        assert_eq!(r.note_use_model.as_deref(), Some(sentence), "{case}");
        let m = mark(&c);
        assert_eq!(
            c.control(&agent(), Action::UseModel),
            Err(Cause::State),
            "{case}"
        );
        assert!(sets(&c, m).is_empty(), "{case}: nothing sent");
    }
}

/// r4 U3.17b sentence 5: Use this computer only asks nothing: the row of an agent whose yes
/// stands for its hosted tier offers it with no question, and `agents_control` with
/// `this_computer_only` sends ONE body, the way back `Body::WithdrawHosted` of the agent. Where the
/// owner refuses that body with `store` or no reply comes, the reply says that Halvo could not
/// finish this, nothing is stored, and the row still offers Use this computer only.
#[test]
fn use_this_computer_only_asks_nothing_and_sends_one_body() {
    let c = world(Tier::Frontier, ready(true));
    let r = row(&c);
    assert!(offers(&c, Control::ThisComputerOnly));
    assert!(!offers(&c, Control::UseModel));
    assert_eq!(
        (r.ask_use_model, r.note_use_model),
        (None, None),
        "no question"
    );
    let m = mark(&c);
    assert_eq!(c.control(&agent(), Action::ThisComputerOnly), Ok(()));
    assert_eq!(
        sets(&c, m),
        [Call::Send(Body::WithdrawHosted {
            workflow: flow(),
            agent: agent()
        })]
    );
    for error in [store(), PortError::NoService] {
        let c = world(Tier::Frontier, ready(true));
        c.ports().refuse(Method::Send, error.clone());
        let m = mark(&c);
        let refusal: Refused = c
            .control_reply(&agent(), Action::ThisComputerOnly)
            .expect_err("refused");
        assert_eq!(refusal.cause(), Cause::NoService, "{error:?}");
        assert_eq!(
            refusal.text(),
            Ok("Halvo could not finish this. If this agent still shows Use this computer only, choose it again.".to_owned()),
            "{error:?}"
        );
        assert!(sets(&c, m).is_empty(), "{error:?}: nothing stored");
        assert!(
            offers(&c, Control::ThisComputerOnly),
            "{error:?}: the row keeps the control"
        );
    }
}
