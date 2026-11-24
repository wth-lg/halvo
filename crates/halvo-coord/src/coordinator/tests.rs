use super::*;
use crate::mem::{Call, Config, Method, Ports};
use crate::types::{
    Agent, AgentState, Body, Category, Control, Item, ItemKind, Name, Param, SpendLimit, System,
    Tier, Workflow, WorkflowState, WorkflowTrigger,
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
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"calendar_cleanup\",\"tier\":\"{}\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"web\",\"playbook\":1}}",
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

fn sheet_agent() -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa2)).unwrap()
}

fn sheet_flow() -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xf2)).unwrap()
}

/// A coordinator after its first tick over the stand-in owner alone, with one manual spreadsheet
/// agent of the runtime file whose newest bindings hold the slots `slots`, and whose next slot is
/// not due today; where `ran`, its one run ended `completed` yesterday, so that no item `ready`
/// waits for its first run.
fn sheet_world(slots: &[&str], ran: bool) -> Coordinator<Ports<Config>> {
    sheet_world_holding(slots, ran, None)
}

/// `sheet_world` whose stored events end, after that run where `ran`, with a `hold_set` of
/// `hold` where given.
fn sheet_world_holding(
    slots: &[&str],
    ran: bool,
    hold: Option<crate::types::HoldSetCause>,
) -> Coordinator<Ports<Config>> {
    let mut stored = Vec::new();
    if ran {
        let run = RunId::parse(&format!("{:032x}", 0x71)).unwrap();
        stored.push(crate::types::What::RunStarted {
            run: run.clone(),
            trigger: crate::types::Trigger::Person,
            slot: None,
        });
        stored.push(crate::types::What::RunEnded {
            run,
            cause: RunEnd::Completed,
            defers: None,
        });
    }
    if let Some(cause) = hold {
        stored.push(crate::types::What::HoldSet { cause });
    }
    sheet_world_from(slots, stored, None)
}

/// The world of `sheet_world`, whose stored events are `stored`, of yesterday, and whose roster
/// entry holds `last_cause`.
fn sheet_world_from(
    slots: &[&str],
    stored: Vec<crate::types::What>,
    last_cause: Option<crate::types::LastCause>,
) -> Coordinator<Ports<Config>> {
    let ports: Ports<Config> = Ports::with_limits(T0, DAY, Limits::PRODUCT);
    let config = Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"spreadsheet_fill\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[\"user_files\"],\"runtime\":\"file\",\"playbook\":1}}",
        sheet_flow(),
        sheet_agent(),
    ))
    .unwrap();
    ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(FullWorkflow {
            id: sheet_flow(),
            state: WorkflowState::Active,
            category: Category::SpreadsheetFill,
            site: None,
            trigger: WorkflowTrigger::Manual,
            period: NonZeroU32::new(2),
            last: DAY,
            agent: Some(sheet_agent()),
        }));
    ports.agents.borrow_mut().push(Agent {
        workflow: sheet_flow(),
        agent: sheet_agent(),
        config,
        state: AgentState::Active,
        slots: slots.iter().map(|s| Name::parse(s).unwrap()).collect(),
        session: false,
        window: false,
        last: None,
        unknown: Vec::new(),
        refused_run: None,
        last_cause,
    });
    {
        for (n, what) in (1..).zip(stored) {
            ports.events.borrow_mut().push(crate::types::Event {
                workflow: sheet_flow(),
                agent: sheet_agent(),
                seq: NonZeroU32::new(n).unwrap(),
                day: DAY - 1,
                at: i64::from(DAY - 1) * 86_400_000_000_000,
                what,
            });
        }
    }
    let c = Coordinator::new(ports, Limits::PRODUCT);
    c.tick();
    c
}

/// The open set-up item of the spreadsheet agent.
fn sheet_setup(c: &Coordinator<Ports<Config>>) -> Option<Item> {
    c.snapshot()
        .items
        .into_iter()
        .find(|i| i.kind() == ItemKind::Setup && i.agent() == &sheet_agent())
}

/// The bodies `set_bindings` that the owner was sent since `from`.
fn set_bindings(c: &Coordinator<Ports<Config>>, from: usize) -> Vec<Body> {
    c.ports().calls.borrow()[from..]
        .iter()
        .filter_map(|t| match &t.call {
            Call::Send(body @ Body::SetBindings { .. }) => Some(body.clone()),
            _ => None,
        })
        .collect()
}

/// The value of the slot `param` in the body `set_bindings`.
fn slot_of(body: &Body, param: Param) -> Option<Vec<String>> {
    let Body::SetBindings { slots, .. } = body else {
        return None;
    };
    slots
        .iter()
        .find(|s| s.name.as_str() == param.name())
        .map(|s| s.value.clone())
}

/// The sentence that the entry of `param` of the open set-up item carries for a line that was
/// not taken.
fn not_taken_of(c: &Coordinator<Ports<Config>>, param: Param) -> Option<String> {
    let item = sheet_setup(c).expect("the set-up item stays open");
    let entry = item.params().iter().find(|p| p.name == param)?;
    entry.not_taken.clone()
}

/// A full path of this build's system in the form of the tables of `tests/typed`.
const HOST_FORM: &str = if cfg!(windows) { "windows" } else { "posix" };

fn system_of(form: &str) -> System {
    match form {
        "posix" => System::Mac,
        "windows" => System::Windows,
        _ => panic!("no form {form}"),
    }
}

/// The rows of the ONE table of typed paths (tests/typed/paths.txt), which a test of
/// halvo-agent-store reads too: the form, whether the core takes the line, the line as typed and
/// the line that it checks and sends.
fn path_rows() -> Vec<(String, bool, String, String)> {
    include_str!("../../tests/typed/paths.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 4, "{l}");
            (
                f[0].to_owned(),
                f[1] == "taken",
                f[2].to_owned(),
                f[3].to_owned(),
            )
        })
        .collect()
}

/// S5 of the ruling of r4 U3.22, the form of this build's system, to the letter.
const NOT_A_FULL_PATH: &str = if cfg!(windows) {
    "That is not a full path. A full path starts with the drive, such as C:\\, and names every folder down to the file."
} else {
    "That is not a full path. A full path starts with a slash (/) and names every folder down to the file."
};

/// A typed answer of each question of the spreadsheet agent that the core takes on this build's
/// system.
fn good(param: Param) -> &'static str {
    match (param, cfg!(windows)) {
        (Param::Workbook, false) => "/Users/ana/Documents/Budget.xlsx",
        (Param::Workbook, true) => "C:\\Users\\ana\\Documents\\Budget.xlsx",
        (Param::Source, false) => "/Users/ana/Documents/receipts.csv",
        (Param::Source, true) => "C:\\Users\\ana\\Documents\\receipts.csv",
        (Param::Sheet, _) => "Sheet1",
        _ => "A2:C11",
    }
}

const SHEET_PARAMS: [Param; 4] = [Param::Workbook, Param::Sheet, Param::Range, Param::Source];

/// A spreadsheet agent with no slot whose set-up item holds a taken answer of every question
/// but `left`.
fn all_answered_but(left: Param) -> (Coordinator<Ports<Config>>, Item) {
    let c = sheet_world(&[], false);
    let item = sheet_setup(&c).expect("the set-up item of an agent with no slot");
    for p in SHEET_PARAMS.into_iter().filter(|p| *p != left) {
        assert_eq!(
            c.setup_typed(item.item(), p, good(p), item.step()),
            Ok(()),
            "{}",
            p.name()
        );
    }
    assert!(set_bindings(&c, 0).is_empty(), "one question is left");
    (c, item)
}

/// r4 U3.22a sentence 3, OF WEIGHT: for Which spreadsheet file and Where the data comes from the
/// core takes away the spaces around a typed line and then ONE pair of quotation marks around the
/// whole of it, and takes the line only where what is left is a full path of the system in one
/// spelling, by the agent store's own rule; over ONE table, which the store's test reads too:
/// the core's rule gives each row's verdict on both systems; on this build's system each refused
/// line sends no body and stores no row, the item stays open and that question's entry carries S5, though
/// it was the last answer that the item lacked; the quoted path goes, its slot holding the path
/// alone.
#[test]
fn u322a_a_typed_path_that_is_no_full_path_is_not_taken_at_its_line_and_a_quoted_one_goes_alone() {
    let rows = path_rows();
    for (form, taken, typed, sent) in &rows {
        let line = crate::items::typed_line(Param::Workbook, typed, system_of(form));
        assert_eq!(line.is_ok(), *taken, "{form} {typed:?}");
        if *taken {
            assert_eq!(line.as_deref(), Ok(sent.as_str()), "{form} {typed:?}");
        }
    }
    let host: Vec<_> = rows.iter().filter(|r| r.0 == HOST_FORM).collect();
    let quoted = host
        .iter()
        .find(|r| r.1 && r.2.starts_with('"'))
        .expect("a quoted path that is taken");
    for param in [Param::Workbook, Param::Source] {
        let (c, item) = all_answered_but(param);
        for (_, _, typed, _) in host.iter().filter(|r| !r.1) {
            let m = mark(&c);
            assert_eq!(
                c.setup_typed(item.item(), param, typed, item.step()),
                Ok(()),
                "{typed:?}"
            );
            assert!(
                sent_or_stored(&c, m).is_empty(),
                "{typed:?}: no body sent, no row stored"
            );
            assert_eq!(
                not_taken_of(&c, param).as_deref(),
                Some(NOT_A_FULL_PATH),
                "{typed:?}"
            );
        }
        let m = mark(&c);
        assert_eq!(
            c.setup_typed(item.item(), param, &quoted.2, item.step()),
            Ok(())
        );
        let sent = set_bindings(&c, m);
        assert_eq!(sent.len(), 1, "{}", param.name());
        assert_eq!(slot_of(&sent[0], param), Some(vec![quoted.3.clone()]));
    }
}

/// S6 of the ruling of r4 U3.22, to the letter.
const CELLS_NOT_READ: &str = "Halvo cannot read these cells. Type them as A2:C11.";

/// r4 U3.22a sentence 4: for Which cells the core takes only a text that the sheet crate's
/// `Range::parse` takes, over ONE table, which the sheet crate's test reads too: the core's rule
/// gives each row's verdict on both systems and sends a taken line as it is typed; each refused
/// line sends no body and stores no row, the item stays open and the entry carries S6, though it was the
/// last answer that the item lacked; a taken line goes.
#[test]
fn u322a_typed_cells_that_are_no_range_are_not_taken_at_their_line() {
    let rows: Vec<(bool, &str)> = include_str!("../../tests/typed/cells.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let (verdict, text) = l.split_once('\t').expect("two fields");
            (verdict == "taken", text)
        })
        .collect();
    for (taken, text) in &rows {
        for system in System::ALL {
            let line = crate::items::typed_line(Param::Range, text, system);
            assert_eq!(line.is_ok(), *taken, "{text:?}");
            if *taken {
                assert_eq!(line.as_deref(), Ok(*text), "{text:?}");
            }
        }
    }
    let (c, item) = all_answered_but(Param::Range);
    for (_, text) in rows.iter().filter(|r| !r.0) {
        let m = mark(&c);
        assert_eq!(
            c.setup_typed(item.item(), Param::Range, text, item.step()),
            Ok(()),
            "{text:?}"
        );
        assert!(
            sent_or_stored(&c, m).is_empty(),
            "{text:?}: no body sent, no row stored"
        );
        assert_eq!(
            not_taken_of(&c, Param::Range).as_deref(),
            Some(CELLS_NOT_READ),
            "{text:?}"
        );
    }
    let (_, taken) = rows.iter().find(|r| r.0).expect("a taken line");
    let m = mark(&c);
    assert_eq!(
        c.setup_typed(item.item(), Param::Range, taken, item.step()),
        Ok(())
    );
    let sent = set_bindings(&c, m);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        slot_of(&sent[0], Param::Range),
        Some(vec![(*taken).to_owned()])
    );
}

const SHEET_SLOTS: [&str; 4] = ["workbook", "sheet", "range", "source"];

fn sheet_row(c: &Coordinator<Ports<Config>>) -> Row {
    c.snapshot()
        .rows
        .into_iter()
        .find(|r| r.agent == sheet_agent())
        .expect("the spreadsheet agent's row")
}

fn sheet_offers(c: &Coordinator<Ports<Config>>, control: Control) -> bool {
    sheet_row(c).controls.iter().any(|x| x.control == control)
}

/// The bodies that the owner was sent and the rows that it was given to store since `from`.
fn sent_or_stored(c: &Coordinator<Ports<Config>>, from: usize) -> Vec<Call> {
    c.ports().calls.borrow()[from..]
        .iter()
        .map(|t| t.call.clone())
        .filter(|call| matches!(call, Call::Send(_) | Call::Log(_)))
        .collect()
}

/// r4 U3.22a sentence 6, OF WEIGHT: the row of a spreadsheet agent whose four questions all have a
/// stored answer offers Change its answers while the agent has no run under way, no queued start
/// and no open item. Its press, `agents_control` with `change_answers`, sends nothing to the owner
/// and stores nothing, and puts the set-up item up with every line empty; while it is open the
/// row reads the line of a set-up and a start is refused. Not now closes it, the stored slots
/// stand and the row offers the control again; Use these answers sends ONE body `set_bindings`
/// with the four slots. An agent of another category, and a spreadsheet agent with a run under
/// way, get no such control.
#[test]
fn u322a_change_its_answers_asks_again_with_nothing_sent_and_stores_all_four_in_one_body() {
    use crate::types::{Choice, YesNo};
    let c = sheet_world(&SHEET_SLOTS, true);
    assert!(sheet_setup(&c).is_none(), "every question has an answer");
    assert!(
        sheet_offers(&c, Control::ChangeAnswers),
        "{:?}",
        sheet_row(&c)
    );
    let m = mark(&c);
    assert_eq!(c.control(&sheet_agent(), Action::ChangeAnswers), Ok(()));
    assert!(
        sent_or_stored(&c, m).is_empty(),
        "the press: no body, no row"
    );
    let item = sheet_setup(&c).expect("the set-up item is up again");
    let names: Vec<Param> = item.params().iter().map(|p| p.name).collect();
    assert_eq!(names, SHEET_PARAMS);
    assert!(item
        .params()
        .iter()
        .all(|p| p.typed == YesNo::Yes && p.candidates.is_empty() && p.not_taken.is_none()));
    assert_eq!(
        sheet_row(&c).line,
        "It needs a few answers from you before it can start."
    );
    assert!(!sheet_offers(&c, Control::ChangeAnswers));
    assert_eq!(
        c.control(&sheet_agent(), Action::RunNow),
        Err(Cause::State),
        "no start while the item is open"
    );
    assert!(c.ports().calls.borrow()[m..]
        .iter()
        .all(|t| !matches!(t.call, Call::Start(_))));
    assert_eq!(
        c.answer(item.item(), Choice::None, None, item.step()),
        Ok(())
    );
    assert!(sheet_setup(&c).is_none(), "Not now closes the item");
    assert!(
        sent_or_stored(&c, m).is_empty(),
        "Not now sends and stores nothing"
    );
    let stored: Vec<String> = c
        .ports()
        .agents
        .borrow()
        .iter()
        .find(|a| a.agent == sheet_agent())
        .map(|a| a.slots.iter().map(|s| s.as_str().to_owned()).collect())
        .unwrap();
    assert_eq!(stored, SHEET_SLOTS, "the stored slots stand");
    assert!(sheet_offers(&c, Control::ChangeAnswers), "offered again");
    assert_eq!(c.control(&sheet_agent(), Action::ChangeAnswers), Ok(()));
    let item = sheet_setup(&c).expect("the set-up item is up again");
    let m = mark(&c);
    for p in SHEET_PARAMS {
        assert_eq!(
            c.setup_typed(item.item(), p, good(p), item.step()),
            Ok(()),
            "{}",
            p.name()
        );
    }
    let sent = set_bindings(&c, m);
    assert_eq!(sent.len(), 1, "ONE body");
    for p in SHEET_PARAMS {
        assert_eq!(slot_of(&sent[0], p), Some(vec![good(p).to_owned()]));
    }
    assert!(
        sheet_setup(&c).is_none(),
        "the item closes at the acknowledgement"
    );
    let other = world(Tier::OnDevice, ready(false));
    assert!(!offers(&other, Control::ChangeAnswers), "another category");
    let running = sheet_world(&SHEET_SLOTS, true);
    assert_eq!(running.control(&sheet_agent(), Action::RunNow), Ok(()));
    assert!(
        !sheet_offers(&running, Control::ChangeAnswers),
        "a run under way"
    );
}

/// r4 U3.22a round 1, ruling (2) of the coordinator's log line 18909: the row of a spreadsheet
/// agent whose set-up Change its answers put up again offers no Start now while that item is open,
/// as a first set-up's row offers none, and a start by the person's call is refused with `state`
/// before any hold ends: with the hold `failures` standing and with `sign_in_refused` standing, the
/// refusal stores no row and sends no body, and behind Not now the row reads that hold again.
#[test]
fn u322a_a_set_up_put_up_again_offers_no_start_now_and_its_start_ends_no_hold() {
    use crate::types::{Choice, HoldSetCause};
    let run = RunId::parse(&format!("{:032x}", 0x71)).unwrap();
    for (hold, line) in [
        (HoldSetCause::Failures, "line.problem.failures"),
        (
            HoldSetCause::SignInRefused { run },
            "line.problem.sign_in_refused",
        ),
    ] {
        let c = sheet_world_holding(&SHEET_SLOTS, true, Some(hold.clone()));
        let said = words::text(line, &[]).unwrap();
        assert_eq!(sheet_row(&c).line, said, "{hold:?}: the hold stands");
        assert_eq!(
            c.control(&sheet_agent(), Action::ChangeAnswers),
            Ok(()),
            "{hold:?}"
        );
        let item = sheet_setup(&c).expect("the set-up item is up again");
        assert!(
            !sheet_offers(&c, Control::RunNow),
            "{hold:?}: no Start now while the item is open"
        );
        let m = mark(&c);
        assert_eq!(
            c.control(&sheet_agent(), Action::RunNow),
            Err(Cause::State),
            "{hold:?}"
        );
        assert!(
            sent_or_stored(&c, m).is_empty(),
            "{hold:?}: no row stored, no body sent"
        );
        assert_eq!(
            c.answer(item.item(), Choice::None, None, item.step()),
            Ok(())
        );
        assert_eq!(sheet_row(&c).line, said, "{hold:?}: the hold is as it was");
    }
}

/// r4 U3.22a round 1, item (4): a typed line of Which sheet, a question with no rule of a line,
/// goes to the owner as it was typed, the spaces around it and among it kept.
#[test]
fn u322a_a_line_of_which_sheet_keeps_the_spaces_around_it() {
    let (c, item) = all_answered_but(Param::Sheet);
    let typed = "  Rent  2026 ";
    let m = mark(&c);
    assert_eq!(
        c.setup_typed(item.item(), Param::Sheet, typed, item.step()),
        Ok(())
    );
    let sent = set_bindings(&c, m);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        slot_of(&sent[0], Param::Sheet),
        Some(vec![typed.to_owned()])
    );
}

/// r4 U3.22a round 1, the fifth ruling (the coordinator's log line 18922): behind a failed run
/// (after a finished one, so that no item `ready` waits) whose last cause is `no_workbook` the
/// row reads that cause's sentence; behind Use these answers
/// of a set-up that Change its answers put up again, whose `item_answered` stands behind the end
/// of that run, it reads R1 to the letter and offers Start now; behind the next run's end it reads
/// that run's line.
#[test]
fn u322a_behind_new_answers_after_a_failed_run_the_row_says_they_are_saved_until_the_next_run() {
    use crate::types::{LastCause, What};
    let r0 = RunId::parse(&format!("{:032x}", 0x71)).unwrap();
    let r1 = RunId::parse(&format!("{:032x}", 0x72)).unwrap();
    let failed = vec![
        What::RunStarted {
            run: r0.clone(),
            trigger: crate::types::Trigger::Person,
            slot: None,
        },
        What::RunEnded {
            run: r0,
            cause: RunEnd::Completed,
            defers: None,
        },
        What::RunStarted {
            run: r1.clone(),
            trigger: crate::types::Trigger::Person,
            slot: None,
        },
        What::RunEnded {
            run: r1,
            cause: RunEnd::Failed,
            defers: None,
        },
    ];
    let c = sheet_world_from(&SHEET_SLOTS, failed.clone(), Some(LastCause::NoWorkbook));
    assert_eq!(
        sheet_row(&c).line,
        words::text("line.problem.no_workbook", &[]).unwrap()
    );
    assert_eq!(c.control(&sheet_agent(), Action::ChangeAnswers), Ok(()));
    let item = sheet_setup(&c).expect("the set-up item is up again");
    let m = mark(&c);
    for p in SHEET_PARAMS {
        assert_eq!(c.setup_typed(item.item(), p, good(p), item.step()), Ok(()));
    }
    assert_eq!(set_bindings(&c, m).len(), 1, "Use these answers");
    let row = sheet_row(&c);
    assert_eq!(
        row.line,
        "Its answers are saved. Choose Start now to try again."
    );
    assert!(sheet_offers(&c, Control::RunNow), "Start now beside it");
    let answered = c
        .ports()
        .events
        .borrow()
        .iter()
        .find(|e| {
            matches!(
                &e.what,
                What::ItemAnswered {
                    answer: crate::types::Answered::Setup(_),
                    ..
                }
            )
        })
        .map(|e| e.what.clone())
        .expect("the row of Use these answers");
    let r2 = RunId::parse(&format!("{:032x}", 0x73)).unwrap();
    let mut next = failed;
    next.extend([
        answered,
        What::RunStarted {
            run: r2.clone(),
            trigger: crate::types::Trigger::Person,
            slot: None,
        },
        What::RunEnded {
            run: r2,
            cause: RunEnd::Completed,
            defers: None,
        },
    ]);
    let after = sheet_world_from(&SHEET_SLOTS, next, None);
    assert_eq!(
        sheet_row(&after).line,
        words::text("line.all_set.done", &[]).unwrap()
    );
}

/// r4 U3.22b sentence 2: a press of Choose a file, `agents_choose_file`, asks the port `Files` for
/// ONE file and answers what the stand-in answers: the picked path, or that the person closed the
/// chooser; the chooser of Where the data comes from opens in the folder of the workbook whose
/// line the item holds. The core takes the call only for Which spreadsheet file or Where the data
/// comes from of an open set-up item, and refuses every other call as it refuses a typed line
/// there: another question, an item of another kind, an item that is closed. No body is sent and
/// no row is stored in any of them, and neither a pick nor a close changes the item.
#[test]
fn u322b_choose_a_file_answers_the_pick_or_the_close_and_is_refused_as_a_typed_line_there() {
    use crate::ports::Chosen;
    let c = sheet_world(&[], false);
    let item = sheet_setup(&c).expect("the set-up item of an agent with no slot");
    let picked = good(Param::Workbook).to_owned();
    c.ports()
        .chosen
        .borrow_mut()
        .push_back(Chosen::Path(picked.clone()));
    let m = mark(&c);
    assert_eq!(
        c.setup_choose(item.item(), Param::Workbook, item.step()),
        Ok(Chosen::Path(picked))
    );
    assert_eq!(
        c.setup_choose(item.item(), Param::Source, item.step()),
        Ok(Chosen::Closed),
        "a close, which the stand-in answers once it holds no pick"
    );
    assert_eq!(sheet_setup(&c), Some(item.clone()), "the item as it was");
    assert!(
        sent_or_stored(&c, m).is_empty(),
        "no body sent, no row stored"
    );
    assert_eq!(
        c.setup_typed(
            item.item(),
            Param::Workbook,
            good(Param::Workbook),
            item.step()
        ),
        Ok(())
    );
    assert_eq!(
        c.setup_choose(item.item(), Param::Source, item.step()),
        Ok(Chosen::Closed)
    );
    let folder = good(Param::Workbook)
        .rsplit_once(['/', '\\'])
        .map(|(folder, _)| folder.to_owned());
    assert_eq!(
        *c.ports().choose_reads.borrow(),
        [None, None, folder],
        "the data file's chooser opens in the folder of the workbook's line"
    );

    let refused = |c: &Coordinator<Ports<Config>>, item: &Item, param: Param, cause: Cause| {
        let m = mark(c);
        let reads = c.ports().choose_reads.borrow().len();
        let before = c.snapshot().items;
        assert_eq!(
            c.setup_choose(item.item(), param, item.step()),
            Err(cause),
            "{}",
            param.name()
        );
        assert!(
            sent_or_stored(c, m).is_empty(),
            "no body sent, no row stored"
        );
        assert_eq!(c.ports().choose_reads.borrow().len(), reads, "no chooser");
        assert_eq!(c.snapshot().items, before, "the items as they were");
    };
    for param in [Param::Sheet, Param::Range] {
        refused(&c, &item, param, Cause::Malformed);
    }
    let ready = sheet_world(&SHEET_SLOTS, false);
    let other = ready
        .snapshot()
        .items
        .into_iter()
        .find(|i| i.agent() == &sheet_agent())
        .expect("the item ready of an agent whose four answers are stored");
    assert_ne!(other.kind(), ItemKind::Setup);
    let typed = ready.setup_typed(other.item(), Param::Workbook, good(Param::Workbook), 0);
    assert_eq!(typed, Err(Cause::Stale), "a typed line there");
    refused(&ready, &other, Param::Workbook, Cause::Stale);
    let (closed, open) = all_answered_but(Param::Source);
    assert_eq!(
        closed.setup_typed(open.item(), Param::Source, good(Param::Source), open.step()),
        Ok(())
    );
    assert!(
        sheet_setup(&closed).is_none(),
        "the last answer closed the item"
    );
    let typed = closed.setup_typed(open.item(), Param::Workbook, good(Param::Workbook), 0);
    assert_eq!(typed, Err(Cause::Stale), "a typed line there");
    refused(&closed, &open, Param::Workbook, Cause::Stale);
}

/// The rows of the ONE table of where a data file may lie against its workbook
/// (tests/typed/folders.txt), which a test of halvo-sheet reads too: the form, whether the data
/// file's place is taken, the workbook's line and the data file's line.
fn folder_rows() -> Vec<(String, bool, String, String)> {
    include_str!("../../tests/typed/folders.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert_eq!(f.len(), 4, "{l}");
            (
                f[0].to_owned(),
                f[1] == "taken",
                f[2].to_owned(),
                f[3].to_owned(),
            )
        })
        .collect()
}

/// S7 of the ruling of r4 U3.22b, to the letter.
const NOT_IN_FOLDER: &str = "The data file must be in the same folder as the spreadsheet file.";

/// r4 U3.22b sentence 3: a line of Where the data comes from that is a full path outside the
/// folder of the workbook whose line the item holds is not taken, by the sheet crate's own rule,
/// over ONE table, which the sheet crate's test reads too: the core's rule gives each row's
/// verdict on both systems; on this build's system each refused row sends no body and stores no
/// row, the item stays open and the data file's entry carries S7, whether the data file's line
/// came before the workbook's line, checked when the workbook's line is taken, or after it; a
/// taken row's body goes with the data file's line.
#[test]
fn u322b_a_data_file_outside_the_workbooks_folder_is_not_taken_at_its_line() {
    let rows = folder_rows();
    for (form, taken, workbook, data) in &rows {
        assert_eq!(
            crate::items::in_folder(workbook, data, system_of(form)),
            *taken,
            "{form} {workbook} {data}"
        );
    }
    for (_, taken, workbook, data) in rows.iter().filter(|r| r.0 == HOST_FORM) {
        for data_first in [false, true] {
            let c = sheet_world(&[], false);
            let item = sheet_setup(&c).expect("the set-up item of an agent with no slot");
            let mut lines = vec![
                (Param::Workbook, workbook.as_str()),
                (Param::Sheet, good(Param::Sheet)),
                (Param::Range, good(Param::Range)),
                (Param::Source, data.as_str()),
            ];
            if data_first {
                lines.reverse();
            }
            let m = mark(&c);
            for (param, text) in lines {
                assert_eq!(c.setup_typed(item.item(), param, text, item.step()), Ok(()));
            }
            if *taken {
                let sent = set_bindings(&c, m);
                assert_eq!(sent.len(), 1, "{workbook} {data}");
                assert_eq!(slot_of(&sent[0], Param::Source), Some(vec![data.clone()]));
            } else {
                assert!(
                    sent_or_stored(&c, m).is_empty(),
                    "{workbook} {data}: no body sent, no row stored"
                );
                assert_eq!(
                    not_taken_of(&c, Param::Source).as_deref(),
                    Some(NOT_IN_FOLDER),
                    "{workbook} {data}, the data file's line first: {data_first}"
                );
            }
        }
    }
}

/// The entry of Which sheet of the open set-up item of the spreadsheet agent.
fn sheet_entry(c: &Coordinator<Ports<Config>>) -> crate::types::ParamEntry {
    let item = sheet_setup(c).expect("the set-up item stays open");
    let entry = item.params().iter().find(|p| p.name == Param::Sheet);
    entry.expect("the item asks Which sheet").clone()
}

/// The list of Which sheet: each entry's number, its value and where it comes from.
fn sheet_list(c: &Coordinator<Ports<Config>>) -> Vec<(u32, Vec<String>, CandidateFrom)> {
    let entry = sheet_entry(c);
    let list = entry.candidates.iter();
    list.map(|e| (e.n, e.value.clone(), e.from)).collect()
}

/// `names` as a list of Which sheet, numbered from 1, each from `files`.
fn listed(names: &[&str]) -> Vec<(u32, Vec<String>, CandidateFrom)> {
    (1..)
        .zip(names)
        .map(|(n, name)| (n, vec![name.to_string()], CandidateFrom::Files))
        .collect()
}

/// r4 U3.22c sentence 4: over the stand-in port of files, a taken line of Which spreadsheet file
/// whose read answers three names makes Which sheet a list of the three, in the workbook's order,
/// with no typed line and no hint; the pick of the second, which reaches the core by its name,
/// sends ONE set_bindings whose sheet slot holds that name. A line whose names were not read leaves the typed line with its hint. In
/// an item that asks its answers again (Change its answers) a taken workbook line shows its
/// workbook's list, and a second workbook line with other names replaces the list and drops the
/// older choice, so that the other answers make no body.
#[test]
fn u322c_which_sheet_offers_the_names_that_the_read_of_the_workbook_answers_as_its_list() {
    use crate::types::YesNo;
    let path = |name: &str| {
        if cfg!(windows) {
            format!("C:\\Users\\ana\\Documents\\{name}")
        } else {
            format!("/Users/ana/Documents/{name}")
        }
    };
    let three = ["January", "February", "Summary"];
    let (book, other, unread) = (path("Budget.xlsx"), path("Plan.xlsx"), path("Old.xlsx"));
    let give = |c: &Coordinator<Ports<Config>>| {
        let mut sheets = c.ports().sheets.borrow_mut();
        sheets.push((book.clone(), three.iter().map(|n| n.to_string()).collect()));
        sheets.push((other.clone(), vec!["Plan".to_owned(), "Actual".to_owned()]));
    };
    let c = sheet_world(&[], false);
    give(&c);
    let item = sheet_setup(&c).expect("the set-up item of an agent with no slot");
    assert_eq!(
        c.setup_typed(item.item(), Param::Workbook, &unread, item.step()),
        Ok(())
    );
    assert_eq!(*c.ports().sheet_reads.borrow(), vec![unread.clone()]);
    let entry = sheet_entry(&c);
    assert!(entry.candidates.is_empty() && entry.typed == YesNo::Yes);
    assert_eq!(
        entry.hint,
        "Type the sheet's name as its tab shows it, for example Sheet1."
    );
    assert_eq!(
        c.setup_typed(item.item(), Param::Workbook, &book, item.step()),
        Ok(())
    );
    assert_eq!(sheet_list(&c), listed(&three));
    let entry = sheet_entry(&c);
    assert_eq!((entry.typed, entry.hint.as_str()), (YesNo::No, ""));
    for p in [Param::Range, Param::Source] {
        assert_eq!(c.setup_typed(item.item(), p, good(p), item.step()), Ok(()));
    }
    let m = mark(&c);
    assert_eq!(
        c.setup_typed(item.item(), Param::Sheet, "February", item.step()),
        Ok(())
    );
    let sent = set_bindings(&c, m);
    assert_eq!(sent.len(), 1, "ONE body");
    assert_eq!(
        slot_of(&sent[0], Param::Sheet),
        Some(vec!["February".into()])
    );
    assert_eq!(slot_of(&sent[0], Param::Workbook), Some(vec![book.clone()]));

    let again = sheet_world(&SHEET_SLOTS, true);
    give(&again);
    assert_eq!(again.control(&sheet_agent(), Action::ChangeAnswers), Ok(()));
    let item = sheet_setup(&again).expect("the set-up item is up again");
    assert_eq!(
        again.setup_typed(item.item(), Param::Workbook, &book, item.step()),
        Ok(())
    );
    assert_eq!(sheet_list(&again), listed(&three), "its workbook's list");
    assert_eq!(
        again.setup_typed(item.item(), Param::Sheet, "January", item.step()),
        Ok(())
    );
    assert_eq!(
        again.setup_typed(item.item(), Param::Workbook, &other, item.step()),
        Ok(())
    );
    assert_eq!(sheet_list(&again), listed(&["Plan", "Actual"]));
    let m = mark(&again);
    for p in [Param::Range, Param::Source] {
        let typed = again.setup_typed(item.item(), p, good(p), item.step());
        assert_eq!(typed, Ok(()));
    }
    assert!(
        set_bindings(&again, m).is_empty(),
        "the older choice went with its list"
    );
}

/// The stored answer of each question of the spreadsheet agent in the tests of r4 U3.22g: a line
/// that the core takes on this build's system, and a text that no other call of these tests holds.
fn kept_answer(param: Param) -> &'static str {
    match (param, cfg!(windows)) {
        (Param::Workbook, false) => "/Qx7mnt/Zqv8 Kxjw",
        (Param::Workbook, true) => "D:\\Qx7mnt\\Zqv8 Kxjw",
        (Param::Source, false) => "/Qx7mnt/zqv8 Bxyl",
        (Param::Source, true) => "D:\\Qx7mnt\\zqv8 Bxyl",
        (Param::Sheet, _) => "Zqv8 Hjk",
        _ => "B3:D9",
    }
}

/// The four stored answers of `kept_answer`.
fn kept_answers() -> crate::types::SheetAnswers {
    let kept = |p| Some(kept_answer(p).to_owned());
    crate::types::SheetAnswers {
        workbook: kept(Param::Workbook),
        sheet: kept(Param::Sheet),
        range: kept(Param::Range),
        source: kept(Param::Source),
    }
}

/// The key `stored` of the question `param` of `item` as the page gets it.
fn stored_of(item: &Item, param: Param) -> Option<String> {
    let json = serde_json::to_value(item).expect("the item as the page gets it");
    let entry = json["params"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == param.name())?;
    entry["stored"].as_str().map(str::to_owned)
}

/// The world of `sheet_world` with the four stored slots, whose read of the stored answers answers
/// `answers`, and the open set-up item that the press of Change its answers put up there.
fn put_up_with(answers: crate::types::SheetAnswers) -> (Coordinator<Ports<Config>>, Item) {
    let c = sheet_world(&SHEET_SLOTS, true);
    c.ports()
        .answers
        .borrow_mut()
        .push((sheet_agent(), answers));
    assert_eq!(c.control(&sheet_agent(), Action::ChangeAnswers), Ok(()));
    let item = sheet_setup(&c).expect("the set-up item is up again");
    (c, item)
}

/// r4 U3.22g sentence 1, OF WEIGHT: the core asks the owner for the stored answers at the press of
/// Change its answers alone, ONCE, for that agent (`Owner::sheet_answers`, r3 U11.7), and the
/// set-up item that it puts up holds each of the four as its question's stored line. A read that
/// answers one value gone leaves that line empty; a value of Which spreadsheet file that is no full
/// path is dropped; a read that fails leaves all four lines empty and the item open. No row that
/// the presses store and no event that they publish holds a value: of every call that Change its
/// answers and the four lines of the press of Use these answers make, the one body `set_bindings`
/// alone holds one. The plain and the pretty Debug form of the item that holds the stored lines
/// hold no four bytes in a row of a stored value (the coordinator's log line 19851), so that no
/// later log line of an item can carry a person's paths.
#[test]
fn u322g_change_its_answers_puts_up_the_stored_answers_and_no_row_or_event_holds_one() {
    let c = sheet_world(&SHEET_SLOTS, true);
    c.ports()
        .answers
        .borrow_mut()
        .push((sheet_agent(), kept_answers()));
    assert!(
        c.ports().answer_reads.borrow().is_empty(),
        "no read at a tick"
    );
    let m = mark(&c);
    assert_eq!(c.control(&sheet_agent(), Action::ChangeAnswers), Ok(()));
    assert_eq!(*c.ports().answer_reads.borrow(), vec![sheet_agent()]);
    let item = sheet_setup(&c).expect("the set-up item is up again");
    for p in SHEET_PARAMS {
        let stored = stored_of(&item, p);
        assert_eq!(stored.as_deref(), Some(kept_answer(p)), "{}", p.name());
    }
    for form in [format!("{item:?}"), format!("{item:#?}")] {
        for p in SHEET_PARAMS {
            let value = kept_answer(p).as_bytes();
            let held = |w: &[u8]| form.as_bytes().windows(4).any(|f| f == w);
            let four = value.windows(4).find(|w| held(w));
            let four = four.map(String::from_utf8_lossy);
            assert_eq!(four, None, "{}: the item's Debug form: {form}", p.name());
        }
    }
    for p in SHEET_PARAMS {
        let typed = c.setup_typed(item.item(), p, kept_answer(p), item.step());
        assert_eq!(typed, Ok(()), "{}", p.name());
    }
    assert_eq!(
        set_bindings(&c, m).len(),
        1,
        "the press of Use these answers"
    );
    assert!(sheet_setup(&c).is_none(), "the item closed");
    assert_eq!(c.ports().answer_reads.borrow().len(), 1, "asked ONCE");
    let calls = c.ports().calls.borrow();
    assert!(calls[m..].iter().any(|t| matches!(t.call, Call::Log(_))));
    for t in &calls[m..] {
        if matches!(t.call, Call::Send(Body::SetBindings { .. })) {
            continue;
        }
        let call = format!("{:?}", t.call);
        for p in SHEET_PARAMS {
            assert!(!call.contains(kept_answer(p)), "{}: {call}", p.name());
        }
    }
    drop(calls);

    let mut some = kept_answers();
    some.range = None;
    some.workbook = Some("Stored budget.xlsx".to_owned());
    let (_, item) = put_up_with(some);
    let lines: Vec<Option<String>> = SHEET_PARAMS.iter().map(|p| stored_of(&item, *p)).collect();
    let kept = |p| Some(kept_answer(p).to_owned());
    assert_eq!(
        lines,
        [None, kept(Param::Sheet), None, kept(Param::Source)],
        "a value gone and a workbook that is no full path: those two lines empty"
    );

    let failed = sheet_world(&SHEET_SLOTS, true);
    failed
        .ports()
        .answers
        .borrow_mut()
        .push((sheet_agent(), kept_answers()));
    failed
        .ports()
        .refuse(Method::SheetAnswers, PortError::NoService);
    assert_eq!(
        failed.control(&sheet_agent(), Action::ChangeAnswers),
        Ok(())
    );
    let item = sheet_setup(&failed).expect("the item is up with a read that failed");
    assert!(SHEET_PARAMS.iter().all(|p| stored_of(&item, *p).is_none()));
}

/// r4 U3.22g sentence 2: behind Change its answers with four stored answers, the press of Use these
/// answers with ONE line turned sends what the page's press sends, the three stored lines and the
/// new line of Where the data comes from, and the owner gets ONE body `set_bindings` whose four
/// slots hold the three stored values and the new one. Not now sends nothing. The names of the
/// workbook's sheets are not read at the put-up; the first line of Which spreadsheet file that the
/// press sends reads them ONCE, though it did not change, and the sheet's line is taken where the
/// workbook has a sheet of that name. Where it has none (the coordinator's log line 19815), the
/// list stands at that press and nothing is stored.
#[test]
fn u322g_one_line_turned_behind_the_stored_answers_sends_one_body_with_the_three_stored_values() {
    use crate::types::Choice;
    let (c, item) = put_up_with(kept_answers());
    let m = mark(&c);
    assert_eq!(
        c.answer(item.item(), Choice::None, None, item.step()),
        Ok(())
    );
    assert!(sheet_setup(&c).is_none(), "Not now closes the item");
    assert!(sent_or_stored(&c, m).is_empty(), "Not now sends nothing");

    assert_eq!(c.control(&sheet_agent(), Action::ChangeAnswers), Ok(()));
    let item = sheet_setup(&c).expect("the set-up item is up again");
    assert!(
        c.ports().sheet_reads.borrow().is_empty(),
        "no names at the put-up"
    );
    let new = if cfg!(windows) {
        "D:\\Qx7mnt\\December receipts.csv"
    } else {
        "/Qx7mnt/December receipts.csv"
    };
    let book = kept_answer(Param::Workbook).to_owned();
    let names = |n: &[&str]| n.iter().map(|s| (*s).to_owned()).collect::<Vec<String>>();
    let has = names(&["Notes", kept_answer(Param::Sheet)]);
    c.ports().sheets.borrow_mut().push((book.clone(), has));
    let m = mark(&c);
    for p in SHEET_PARAMS {
        let line = match p {
            Param::Source => new.to_owned(),
            _ => stored_of(&item, p).expect("the stored line"),
        };
        let typed = c.setup_typed(item.item(), p, &line, item.step());
        assert_eq!(typed, Ok(()), "{}", p.name());
    }
    let sent = set_bindings(&c, m);
    assert_eq!(sent.len(), 1, "ONE body");
    for p in [Param::Workbook, Param::Sheet, Param::Range] {
        let slot = slot_of(&sent[0], p);
        assert_eq!(slot, Some(vec![kept_answer(p).to_owned()]), "{}", p.name());
    }
    assert_eq!(slot_of(&sent[0], Param::Source), Some(vec![new.to_owned()]));
    let reads = c.ports().sheet_reads.borrow().clone();
    assert_eq!(
        reads,
        vec![book.clone()],
        "the names read once, at the press"
    );

    let (lacks, item) = put_up_with(kept_answers());
    let other = names(&["Plan", "Actual"]);
    lacks.ports().sheets.borrow_mut().push((book, other));
    let m = mark(&lacks);
    for p in SHEET_PARAMS {
        let line = stored_of(&item, p).expect("the stored line");
        let typed = lacks.setup_typed(item.item(), p, &line, item.step());
        assert_eq!(typed, Ok(()), "{}", p.name());
    }
    assert_eq!(sheet_list(&lacks), listed(&["Plan", "Actual"]), "the list");
    assert!(sent_or_stored(&lacks, m).is_empty(), "nothing is stored");
}
