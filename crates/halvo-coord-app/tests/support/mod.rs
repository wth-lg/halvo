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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::ThreadId;
use std::time::{Duration, Instant};
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
    clock_back_at_cancel: Mutex<Option<u64>>,
    panic_at_cancel: AtomicBool,
    kills_in_turn: AtomicBool,
    hook_clock_gated: AtomicBool,
    hook_at_clock: AtomicBool,
    hook_clock_go: AtomicBool,
    cancels: AtomicUsize,
    start_gate: Mutex<Option<StartGate>>,
    start_at_gate: AtomicBool,
    start_gate_go: AtomicBool,
    recorder: Recorder,
}

/// The name of a thread on which a test calls `Core::quit`, which `Shared::gate_the_hooks_clock_read`
/// reads.
pub const QUIT_HOOK_THREAD: &str = "quit-hook";

/// The port call of a start at which `Shared::hold_the_next_start_at` holds it, behind the
/// start's first reading of the quit's mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartGate {
    /// The store of the events that hold `run_started` (`Owner::log`).
    StoreOfRunStarted,
    /// The send of the body `start_run` (`Owner::send`).
    SendOfStartRun,
    /// `Runs::start`, before the memory ports record it.
    RunsStart,
}

/// How long the recorder holds a kill before it answers `no_service`.
#[derive(Clone, Copy, Debug)]
pub enum Hold {
    /// For this time.
    For(Duration),
    /// Until `Shared::release_kills`.
    Released,
    /// Until `Shared::release_kills`, and then the kill of the memory ports answers, Ok for a
    /// runner that this life started.
    ReleasedThenOk,
    /// No time: the recorder panics in place of an answer.
    Panics,
}

#[derive(Default)]
struct Recorder {
    hold: Mutex<Option<Hold>>,
    calls: Mutex<Vec<(RunId, Instant)>>,
    holding: AtomicUsize,
    released: Mutex<bool>,
    free: Condvar,
}

impl Recorder {
    fn take(&self, run: &RunId) -> Option<Result<(), PortError>> {
        let hold = (*self.hold.lock().unwrap_or_else(PoisonError::into_inner))?;
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((run.clone(), Instant::now()));
        self.holding.fetch_add(1, Ordering::SeqCst);
        match hold {
            Hold::Panics => {
                self.holding.fetch_sub(1, Ordering::SeqCst);
                panic!("the recorder's kill panics");
            }
            Hold::For(time) => std::thread::sleep(time),
            Hold::Released | Hold::ReleasedThenOk => {
                let mut released = self.released.lock().unwrap_or_else(PoisonError::into_inner);
                while !*released {
                    released = self
                        .free
                        .wait(released)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
        self.holding.fetch_sub(1, Ordering::SeqCst);
        match hold {
            Hold::ReleasedThenOk => None,
            _ => Some(Err(PortError::NoService)),
        }
    }
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

    /// From now on, `Clock::now` and `Clock::day` fail with `no_service`.
    pub fn fail_the_clock_now(&self) {
        self.clock_failing.store(true, Ordering::SeqCst);
    }

    /// At the next `Runs::cancel` the clock is set back to `to`, once.
    pub fn step_clock_at_cancel(&self, to: u64) {
        *self
            .clock_back_at_cancel
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(to);
    }

    /// Runs `f` on the memory ports.
    pub fn with<T>(&self, f: impl FnOnce(&Ports<Config>) -> T) -> T {
        let _turn = self.enter();
        f(&self.ports)
    }

    /// From the next `Runs::kill` on, the kill is replaced by a recorder (the never-here rule of
    /// a line that ends a process): it writes the run that it would have ended and the time of
    /// the call, ends nothing, holds by `hold` and answers `no_service`, but for
    /// `Hold::ReleasedThenOk`, after which the kill of the memory ports answers. It holds
    /// outside the turn of `Shared`, so that the other ports answer while it holds, unless
    /// `hold_kills_inside_the_turn` was called: then it takes the turn first, as the kill of the
    /// memory ports does, and every other port call waits while it holds (the test of a tick's
    /// kill that shares a lock with the clock).
    pub fn hold_kills(&self, hold: Hold) {
        *self
            .recorder
            .hold
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hold);
    }

    /// From the next `Runs::kill` on, the recorder holds inside the turn of `Shared`.
    pub fn hold_kills_inside_the_turn(&self) {
        self.kills_in_turn.store(true, Ordering::SeqCst);
    }

    /// From now on, a `Clock::now` on a thread named `QUIT_HOOK_THREAD` says so
    /// (`hook_at_its_clock_read`) and waits, before it takes the turn, until
    /// `let_the_hooks_clock_read_go`.
    pub fn gate_the_hooks_clock_read(&self) {
        self.hook_clock_gated.store(true, Ordering::SeqCst);
    }

    /// Whether a thread named `QUIT_HOOK_THREAD` waits at its read of the clock.
    pub fn hook_at_its_clock_read(&self) -> bool {
        self.hook_at_clock.load(Ordering::SeqCst)
    }

    /// Lets a read of the clock that waits at the gate go on, now and later.
    pub fn let_the_hooks_clock_read_go(&self) {
        self.hook_clock_go.store(true, Ordering::SeqCst);
    }

    /// The next `Runs::cancel` panics, once.
    pub fn panic_at_the_next_cancel(&self) {
        self.panic_at_cancel.store(true, Ordering::SeqCst);
    }

    /// How many `Runs::cancel` calls have returned, read with no turn.
    pub fn cancels_made(&self) -> usize {
        self.cancels.load(Ordering::SeqCst)
    }

    /// Ends every hold `Hold::Released`, now and later.
    pub fn release_kills(&self) {
        *self
            .recorder
            .released
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = true;
        self.recorder.free.notify_all();
    }

    /// The kills that the recorder took: the run that each would have ended, with the time of
    /// its call.
    pub fn recorded_kills(&self) -> Vec<(RunId, Instant)> {
        self.recorder
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The kills that the recorder holds now.
    pub fn kills_holding(&self) -> usize {
        self.recorder.holding.load(Ordering::SeqCst)
    }

    /// The next call at `gate` says so (`start_at_its_gate`) and waits, before it takes the turn,
    /// until `let_the_start_go`; one call holds, and the later ones answer at once.
    pub fn hold_the_next_start_at(&self, gate: StartGate) {
        *self
            .start_gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(gate);
    }

    /// Whether a call waits at the gate of `hold_the_next_start_at`, read with no turn.
    pub fn start_at_its_gate(&self) -> bool {
        self.start_at_gate.load(Ordering::SeqCst)
    }

    /// Lets the call that waits at the gate go on.
    pub fn let_the_start_go(&self) {
        self.start_gate_go.store(true, Ordering::SeqCst);
    }

    fn start_gate(&self, here: StartGate) {
        let held = {
            let mut gate = self
                .start_gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let held = *gate == Some(here);
            if held {
                *gate = None;
            }
            held
        };
        if held {
            self.start_at_gate.store(true, Ordering::SeqCst);
            while !self.start_gate_go.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
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
                    defers: None,
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
        clock_back_at_cancel: Mutex::new(None),
        panic_at_cancel: AtomicBool::new(false),
        kills_in_turn: AtomicBool::new(false),
        hook_clock_gated: AtomicBool::new(false),
        hook_at_clock: AtomicBool::new(false),
        hook_clock_go: AtomicBool::new(false),
        cancels: AtomicUsize::new(0),
        start_gate: Mutex::new(None),
        start_at_gate: AtomicBool::new(false),
        start_gate_go: AtomicBool::new(false),
        recorder: Recorder::default(),
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
        if events
            .iter()
            .any(|e| matches!(e.what, What::RunStarted { .. }))
        {
            self.start_gate(StartGate::StoreOfRunStarted);
        }
        let _turn = self.enter();
        self.ports.log(events)
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<(Vec<Event>, u32), PortError> {
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
        if matches!(body, Body::StartRun { .. }) {
            self.start_gate(StartGate::SendOfStartRun);
        }
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

    fn replies(
        &self,
        workflow: &halvo_coord::types::WorkflowId,
        agent: &halvo_coord::types::AgentId,
        run: &halvo_coord::types::RunId,
    ) -> Result<Vec<halvo_coord::types::ReplyRow>, PortError> {
        let _turn = self.enter();
        self.ports.replies(workflow, agent, run)
    }

    fn overlaps(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        slots: Option<&[halvo_coord::types::Slot]>,
    ) -> Result<halvo_coord::ports::Overlaps, PortError> {
        let _turn = self.enter();
        self.ports.overlaps(workflow, agent, slots)
    }

    fn delete(&self, workflow: &WorkflowId) -> Result<(), PortError> {
        let _turn = self.enter();
        self.ports.delete(workflow)
    }
}

impl Runs for Shared {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.start_gate(StartGate::RunsStart);
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
        if self.panic_at_cancel.swap(false, Ordering::SeqCst) {
            panic!("the port cancel panics once");
        }
        if self.clock_fails_at_cancel.load(Ordering::SeqCst) {
            self.clock_failing.store(true, Ordering::SeqCst);
        }
        if let Some(to) = self
            .clock_back_at_cancel
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            self.ports.set_now(to);
        }
        let answer = self.ports.cancel(run);
        self.cancels.fetch_add(1, Ordering::SeqCst);
        answer
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        let _inside = self
            .kills_in_turn
            .load(Ordering::SeqCst)
            .then(|| self.enter());
        if let Some(answer) = self.recorder.take(run) {
            return answer;
        }
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
        if self.hook_clock_gated.load(Ordering::SeqCst)
            && std::thread::current().name() == Some(QUIT_HOOK_THREAD)
        {
            self.hook_at_clock.store(true, Ordering::SeqCst);
            while !self.hook_clock_go.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
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

/// The memory ports `ports` as the value that threads share, with the memory policy and every
/// other field as `world_ending` starts it.
pub fn shared_of(ports: Ports<Config>) -> Shared {
    Shared {
        ports,
        ..world_ending(&[], Limits::PRODUCT)
    }
}

/// The mailbox of the email agents of `sharing_set_up` and `sharing_held`.
pub const ACCOUNT: &str = "me@home.test";
/// The sender that the two agents of those worlds share.
pub const ANN: &str = "ann@people.test";

/// The asking email agent: in `sharing_set_up` the agent that the first tick makes for the
/// approved workflow, the first id that the memory random bytes give.
pub fn asking() -> AgentId {
    AgentId::parse(&format!("{:032x}", 1)).unwrap()
}

pub fn asking_flow() -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xfa)).unwrap()
}

/// The live email agent on the same mailbox that listens to `ANN`.
pub fn other() -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xb0)).unwrap()
}

pub fn other_flow() -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xe0)).unwrap()
}

fn mail_config(flow: &WorkflowId, agent: &AgentId) -> Config {
    Config::parse(&format!(
        "{{\"v\":1,\"workflow\":\"{flow}\",\"agent\":\"{agent}\",\"rev\":1,\"category\":\"email\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"no\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_mail\",\"playbook\":1}}"
    ))
    .unwrap()
}

fn mail_workflow(id: WorkflowId, state: WorkflowState, agent: Option<AgentId>) -> Workflow {
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

fn mail_entry(flow: WorkflowId, agent: AgentId, slots: &[&str]) -> Agent<Config> {
    Agent {
        config: mail_config(&flow, &agent),
        workflow: flow,
        agent,
        state: AgentState::Active,
        slots: slots.iter().map(|s| Name::parse(s).unwrap()).collect(),
        session: true,
        last: None,
        unknown: vec![],
    }
}

fn mail_slot(name: &str, value: &str) -> halvo_coord::types::Slot {
    halvo_coord::types::Slot {
        name: Name::parse(name).unwrap(),
        value: vec![value.to_owned()],
    }
}

fn mail_bind(p: &Ports<Config>, flow: WorkflowId, agent: AgentId, senders: Option<&str>) {
    let mut slots = vec![mail_slot("account_address", ACCOUNT)];
    slots.extend(senders.map(|s| mail_slot("reply_senders", s)));
    p.send(&Body::SetBindings {
        workflow: flow,
        agent,
        slots,
    })
    .unwrap();
}

fn mail_listed(p: &Ports<Config>, flow: &WorkflowId, state: WorkflowState) {
    for w in p.workflows.borrow_mut().iter_mut() {
        if let Workflow::Full(f) = w {
            if &f.id == flow {
                f.state = state;
            }
        }
    }
}

/// A world for the sentence of the refusal `overlap` at a set-up item (I3.5): an approved email
/// workflow with no agent, whose agent the first tick makes with a stored `account_address` and
/// no `reply_senders`, so that its hold `setup` raises the item, beside the email agent `other`,
/// live on the same mailbox and listening to `ANN`.
pub fn sharing_set_up() -> Shared {
    let p: Ports<Config> = Ports::new(T0, DAY);
    p.workflows
        .borrow_mut()
        .push(mail_workflow(asking_flow(), WorkflowState::Approved, None));
    p.agents
        .borrow_mut()
        .push(mail_entry(asking_flow(), asking(), &["account_address"]));
    mail_bind(&p, asking_flow(), asking(), None);
    p.workflows.borrow_mut().push(mail_workflow(
        other_flow(),
        WorkflowState::Paused,
        Some(other()),
    ));
    p.agents.borrow_mut().push(mail_entry(
        other_flow(),
        other(),
        &["account_address", "reply_senders"],
    ));
    mail_bind(&p, other_flow(), other(), Some(ANN));
    mail_listed(&p, &other_flow(), WorkflowState::Active);
    shared_of(p)
}

/// A world for the sentence of a refused Switch on and Start now (I3.5 (f), RD-160 (3)): the
/// asking email agent holds `ANN` in its stored bindings and is listed `paused` with the hold
/// `failures`, beside `other`, listed `active` with `ANN` on the same mailbox, so that each move
/// sends `active`, which the owner refuses with `overlap`.
pub fn sharing_held() -> Shared {
    let p: Ports<Config> = Ports::new(T0, DAY);
    for (flow, agent) in [(asking_flow(), asking()), (other_flow(), other())] {
        p.workflows.borrow_mut().push(mail_workflow(
            flow.clone(),
            WorkflowState::Paused,
            Some(agent.clone()),
        ));
        p.agents.borrow_mut().push(mail_entry(
            flow.clone(),
            agent.clone(),
            &["account_address", "reply_senders"],
        ));
        mail_bind(&p, flow, agent, Some(ANN));
    }
    mail_listed(&p, &other_flow(), WorkflowState::Active);
    p.log(&[Event {
        workflow: asking_flow(),
        agent: asking(),
        seq: NonZeroU32::new(1).unwrap(),
        day: DAY - 1,
        at: 0,
        what: What::HoldSet {
            cause: halvo_coord::types::HoldSetCause::Failures,
        },
    }])
    .unwrap();
    shared_of(p)
}
