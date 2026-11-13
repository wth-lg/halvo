#![allow(dead_code)]

use halvo_coord::guard::Policy as PolicyPort;
use halvo_coord::mem::policy::Policy;
use halvo_coord::mem::{Call, Config, Ports};
use halvo_coord::ports::{Analysis, Clock, Owner, PortError, Reaped, Rng, Runs};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Answer, AskedEffect, BindResult, Body, BoundField, BoundFrom,
    Category, Class, ConfirmAsk, Decision, Digest, EffectId, Event, FullWorkflow, HandbackResult,
    Limits, Name, Outcome, PastRun, Reply, RunEnd, RunEvent, RunHistory, RunId, RunSpec, Trigger,
    What, Workflow, WorkflowId, WorkflowState, WorkflowTrigger,
};
use halvo_coord_app::{handler, Agents, Hub, Native, Present};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::ThreadId;
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow, WebviewWindowBuilder};

pub const DAY: u32 = 20_000;
pub const T0: u64 = 1_700_000_000_000;

/// The memory ports and the memory policy of `halvo_coord` as one value that threads share:
/// one thread at a time holds them, and the holder may enter again, since the coordinator's
/// `record` logs inside `Runs::answer_confirm`.
pub struct Shared {
    turn: Mutex<(Option<ThreadId>, u32)>,
    free: Condvar,
    ports: Ports<Config>,
    policy: Policy<Config>,
    clock_fails_at_cancel: AtomicBool,
    clock_failing: AtomicBool,
}

// SAFETY: `ports` and `policy` are read only inside `enter`, which lets one thread in at a time.
unsafe impl Sync for Shared {}

struct Turn<'a>(&'a Shared);

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut turn = self.0.turn.lock().unwrap_or_else(PoisonError::into_inner);
        turn.1 -= 1;
        if turn.1 == 0 {
            turn.0 = None;
            self.0.free.notify_all();
        }
    }
}

impl Shared {
    fn enter(&self) -> Turn<'_> {
        let me = std::thread::current().id();
        let mut turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
        while turn.0.is_some_and(|t| t != me) {
            turn = self.free.wait(turn).unwrap_or_else(PoisonError::into_inner);
        }
        turn.0 = Some(me);
        turn.1 += 1;
        Turn(self)
    }

    /// From the next `Runs::cancel` on, `Clock::now` and `Clock::day` fail with `no_service`.
    pub fn fail_clock_at_cancel(&self) {
        self.clock_fails_at_cancel.store(true, Ordering::SeqCst);
    }

    /// Runs `f` on the memory ports.
    pub fn with<T>(&self, f: impl FnOnce(&Ports<Config>) -> T) -> T {
        let _turn = self.enter();
        f(&self.ports)
    }
}

pub fn agent(n: usize) -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa0 + n)).unwrap()
}

pub fn flow(n: usize) -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xf0 + n)).unwrap()
}

fn config(n: usize) -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"calendar_cleanup\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_calendar\",\"playbook\":1}}",
        flow(n),
        agent(n),
    ))
    .unwrap()
}

/// The ports of a world of `agents` manual agents, each with one run that completed two days
/// before the clock's day.
pub fn world(agents: usize, limits: Limits) -> Shared {
    world_ending(&vec![RunEnd::Completed; agents], limits)
}

/// The same with the end of each agent's one run given.
pub fn world_ending(ends: &[RunEnd], limits: Limits) -> Shared {
    let ports: Ports<Config> = Ports::with_limits(T0, DAY, limits);
    for (n, end) in ends.iter().copied().enumerate() {
        let run = RunId::parse(&format!("{:032x}", 0x0100 + n)).unwrap();
        let event = |seq: u32, what: What| Event {
            workflow: flow(n),
            agent: agent(n),
            seq: NonZeroU32::new(seq).unwrap(),
            day: DAY - 2,
            at: 0,
            what,
        };
        ports.events.borrow_mut().extend([
            event(
                1,
                What::RunStarted {
                    run: run.clone(),
                    trigger: Trigger::Person,
                    slot: None,
                },
            ),
            event(
                2,
                What::RunEnded {
                    run: run.clone(),
                    cause: end,
                },
            ),
        ]);
        ports
            .workflows
            .borrow_mut()
            .push(Workflow::Full(FullWorkflow {
                id: flow(n),
                state: WorkflowState::Active,
                category: Category::CalendarCleanup,
                site: None,
                trigger: WorkflowTrigger::Manual,
                period: NonZeroU32::new(2),
                last: DAY - 2,
                agent: Some(agent(n)),
            }));
        ports.agents.borrow_mut().push(Agent {
            workflow: flow(n),
            agent: agent(n),
            config: config(n),
            state: AgentState::Active,
            slots: Vec::new(),
            session: true,
            last: Some(PastRun {
                run,
                day: DAY - 2,
                outcome: Outcome::parse(end.name()).unwrap_or(Outcome::Completed),
                unknown: 0,
            }),
            unknown: Vec::new(),
        });
    }
    Shared {
        turn: Mutex::new((None, 0)),
        free: Condvar::new(),
        ports,
        policy: Policy::new(&[]),
        clock_fails_at_cancel: AtomicBool::new(false),
        clock_failing: AtomicBool::new(false),
    }
}

/// The counts that the hub's tray received, in order.
#[derive(Clone, Default)]
pub struct Counts(pub Arc<Mutex<Vec<u32>>>);

impl Counts {
    pub fn sink(&self) -> Box<dyn Fn(u32) + Send + Sync> {
        let counts = self.0.clone();
        Box::new(move |n| counts.lock().unwrap().push(n))
    }

    pub fn all(&self) -> Vec<u32> {
        self.0.lock().unwrap().clone()
    }
}

/// A stand-in presenter whose dialogs stay open: it keeps every `Native` that it gets until the
/// test takes it.
#[derive(Clone, Default)]
pub struct Dialogs(pub Arc<Mutex<Vec<Native>>>);

impl Present for Dialogs {
    fn present(&self, dialog: Native) {
        self.0.lock().unwrap().push(dialog);
    }
}

impl Dialogs {
    pub fn boxed(&self) -> Box<dyn Present> {
        Box::new(self.clone())
    }

    pub fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    pub fn take(&self) -> Vec<Native> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

/// The calls of the memory ports since `from`.
pub fn calls_since(hub: &Hub<Shared>, from: usize) -> Vec<Call> {
    hub.coordinator().ports().inner().with(|p| {
        p.calls.borrow()[from..]
            .iter()
            .map(|t| t.call.clone())
            .collect()
    })
}

pub fn mark(hub: &Hub<Shared>) -> usize {
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.calls.borrow().len())
}

/// The events that the memory owner acknowledged.
pub fn events(hub: &Hub<Shared>) -> Vec<Event> {
    hub.coordinator()
        .ports()
        .inner()
        .with(|p| p.events.borrow().clone())
}

/// The runs that the coordinator started, in order.
pub fn started(hub: &Hub<Shared>) -> Vec<RunId> {
    hub.coordinator().ports().inner().with(|p| {
        p.calls
            .borrow()
            .iter()
            .filter_map(|t| match &t.call {
                Call::Start(spec) => Some(spec.run.clone()),
                _ => None,
            })
            .collect()
    })
}

/// A confirm request of `run` of agent `n` with one effect of `class` and the fields of that
/// class in the order of A-A9.3.
pub fn ask(run: &RunId, n: usize, class: Class) -> RunEvent {
    let names: &[(&str, BoundFrom)] = match class {
        Class::Payment => &[
            ("payee", BoundFrom::Slot),
            ("amount", BoundFrom::Extract),
            ("instrument", BoundFrom::Slot),
            ("origin", BoundFrom::Const),
        ],
        _ => &[
            ("to", BoundFrom::Extract),
            ("subject", BoundFrom::Extract),
            ("body", BoundFrom::Model),
        ],
    };
    RunEvent::Confirm(
        ConfirmAsk::new(
            run.clone(),
            agent(n),
            vec![AskedEffect {
                effect: EffectId::parse(&format!("{:032x}", 0xe1)).unwrap(),
                class,
                digest: Digest::parse(&"ab".repeat(32)).unwrap(),
                digest_ok: true,
                fields: names
                    .iter()
                    .map(|(name, from)| BoundField {
                        name: Name::parse(name).unwrap(),
                        from: *from,
                        value: vec!["City Power".into()],
                    })
                    .collect(),
                new_origin: None,
            }],
        )
        .unwrap(),
    )
}

/// A mock app that manages `hub` and registers the handler, with the windows `main` and
/// `other`.
pub fn app(
    hub: Arc<Hub<Shared>>,
) -> (
    App<MockRuntime>,
    WebviewWindow<MockRuntime>,
    WebviewWindow<MockRuntime>,
) {
    let app = mock_builder()
        .invoke_handler(handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    app.manage(Agents(hub));
    let main = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let other = WebviewWindowBuilder::new(&app, "other", Default::default())
        .build()
        .unwrap();
    (app, main, other)
}

/// The reply of `command` with `args` from the page of `window`: the reply's JSON text, or the
/// rejection.
pub fn invoke(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
    args: serde_json::Value,
) -> Result<String, serde_json::Value> {
    invoke_body(window, command, InvokeBody::Json(args))
}

/// The reply of `command` with the call's body `body` from the page of `window`.
pub fn invoke_body(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
    body: InvokeBody,
) -> Result<String, serde_json::Value> {
    let url = if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    tauri::test::get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: url.parse().unwrap(),
            body,
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| match body {
        tauri::ipc::InvokeResponseBody::Json(text) => text,
        tauri::ipc::InvokeResponseBody::Raw(bytes) => String::from_utf8(bytes).unwrap(),
    })
}

/// The text of the page's fixture `<stem>.json` without its line end.
pub fn fixture(stem: &str) -> String {
    let path = format!(
        "{}/../../ui/tests/fixtures/coord/{stem}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path)
        .unwrap()
        .trim_end_matches('\n')
        .to_owned()
}

impl Owner for Shared {
    type Config = Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        let _turn = self.enter();
        self.ports.log(events)
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        let _turn = self.enter();
        self.ports.history(agent, after)
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        let _turn = self.enter();
        self.ports.workflows()
    }

    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        let _turn = self.enter();
        self.ports.agents()
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        let _turn = self.enter();
        self.ports.send(body)
    }

    fn agent_history(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        runs: u32,
    ) -> Result<Vec<RunHistory>, PortError> {
        let _turn = self.enter();
        self.ports.agent_history(workflow, agent, runs)
    }
}

impl Runs for Shared {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        let _turn = self.enter();
        self.ports.start(spec)
    }

    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        let _turn = self.enter();
        self.ports.answer_confirm(run, answers, record)
    }

    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        let _turn = self.enter();
        self.ports.answer_handback(run, result)
    }

    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        let _turn = self.enter();
        self.ports.answer_bind(run, result)
    }

    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        let _turn = self.enter();
        if self.clock_fails_at_cancel.load(Ordering::SeqCst) {
            self.clock_failing.store(true, Ordering::SeqCst);
        }
        self.ports.cancel(run)
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        let _turn = self.enter();
        self.ports.kill(run)
    }

    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        let _turn = self.enter();
        self.ports.reap()
    }
}

impl Analysis for Shared {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        let _turn = self.enter();
        self.ports.defer(until)
    }
}

impl Clock for Shared {
    fn now(&self) -> Result<u64, PortError> {
        let _turn = self.enter();
        if self.clock_failing.load(Ordering::SeqCst) {
            return Err(PortError::NoService);
        }
        self.ports.now()
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        let _turn = self.enter();
        if self.clock_failing.load(Ordering::SeqCst) {
            return Err(PortError::NoService);
        }
        self.ports.day(at)
    }
}

impl Rng for Shared {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        let _turn = self.enter();
        self.ports.bytes16()
    }
}

impl PolicyPort for Shared {
    type Config = Config;

    fn gives_run(&self, config: &Config, class: Class) -> Result<bool, PortError> {
        let _turn = self.enter();
        self.policy.gives_run(config, class)
    }
}
