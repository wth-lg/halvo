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
        last_cause: None,
    });
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
/// line leaves no `set_bindings`, the item stays open and that question's entry carries S5, though
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
            assert!(set_bindings(&c, m).is_empty(), "{typed:?}: no body");
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
/// line leaves no `set_bindings`, the item stays open and the entry carries S6, though it was the
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
        assert!(set_bindings(&c, m).is_empty(), "{text:?}: no body");
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
