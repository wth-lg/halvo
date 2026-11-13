#![allow(dead_code)]

use halvo_coord::guard::Table;
use halvo_coord::mem::{Call, Config, Ports};
use halvo_coord::run::{drive, Act, Done};
use halvo_coord::schedule::{Board, Member};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Category, Doing, Event, Facts, FullWorkflow, Limits, PastRun,
    RunEnd, RunId, Trigger, What, Workflow, WorkflowId, WorkflowState, WorkflowTrigger,
};
use std::num::NonZeroU32;

pub const DAY: u32 = 20_000;
pub const T0: u64 = 1_700_000_000_000;

pub fn agent(n: usize) -> AgentId {
    AgentId::parse(&format!("{:032x}", 0xa0 + n)).unwrap()
}

pub fn flow(n: usize) -> WorkflowId {
    WorkflowId::parse(&format!("{:032x}", 0xf0 + n)).unwrap()
}

/// An agent of the world: whether it needs the person, and whether its workflow has a
/// schedule of period 2 whose first slot is today.
#[derive(Clone, Copy)]
pub struct A {
    pub person: bool,
    pub slotted: bool,
}

pub const MANUAL: A = A {
    person: false,
    slotted: false,
};

pub fn config_text(n: usize, person: bool) -> String {
    format!(
        "{{\"v\":1,\"workflow\":\"{}\",\"agent\":\"{}\",\"rev\":1,\"category\":\"calendar_cleanup\",\"tier\":\"on_device\",\"reasoning\":\"medium\",\"autonomy\":\"draft_and_confirm\",\"allow\":[],\"caps\":{{\"untrusted\":\"yes\",\"private\":\"yes\",\"external\":\"{}\"}},\"limits\":{{\"steps\":20,\"wall_s\":120,\"model_tokens\":0}},\"budget\":null,\"permissions\":[],\"runtime\":\"graph_calendar\",\"playbook\":1}}",
        flow(n),
        agent(n),
        if person { "yes" } else { "no" },
    )
}

pub fn config(n: usize, a: A) -> Config {
    Config::parse(&config_text(n, a.person)).unwrap()
}

pub fn workflow(n: usize, a: A) -> FullWorkflow {
    FullWorkflow {
        id: flow(n),
        state: WorkflowState::Active,
        category: Category::CalendarCleanup,
        site: None,
        trigger: if a.slotted {
            WorkflowTrigger::Schedule
        } else {
            WorkflowTrigger::Manual
        },
        period: NonZeroU32::new(2),
        last: DAY - 2,
        agent: Some(agent(n)),
    }
}

/// The world of a test: the memory ports, the board and the guard's table of one life.
pub struct World {
    pub ports: Ports<Config>,
    pub board: Board<Config>,
    pub table: Table,
    pub limits: Limits,
    pub spec: Vec<A>,
    seen: usize,
}

/// A run that completed on the day before the first slot, as its record and two events.
pub fn finished_run(n: usize) -> (PastRun, Vec<Event>) {
    let run = RunId::parse(&format!("{:032x}", 0x0100 + n)).unwrap();
    let event = |seq: u32, what: What| Event {
        workflow: flow(n),
        agent: agent(n),
        seq: NonZeroU32::new(seq).unwrap(),
        day: DAY - 2,
        at: 0,
        what,
    };
    let events = vec![
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
                cause: RunEnd::Completed,
            },
        ),
    ];
    let record = PastRun {
        run,
        day: DAY - 2,
        outcome: halvo_coord::types::Outcome::Completed,
        unknown: 0,
    };
    (record, events)
}

pub fn world_with(spec: &[A], limits: Limits) -> World {
    let ports: Ports<Config> = Ports::with_limits(T0, DAY, limits);
    let mut members = Vec::new();
    for (n, a) in spec.iter().enumerate() {
        let (record, events) = finished_run(n);
        ports.events.borrow_mut().extend(events.iter().cloned());
        ports
            .workflows
            .borrow_mut()
            .push(Workflow::Full(workflow(n, *a)));
        ports.agents.borrow_mut().push(Agent {
            workflow: flow(n),
            agent: agent(n),
            config: config(n, *a),
            state: AgentState::Active,
            slots: Vec::new(),
            session: true,
            last: Some(record.clone()),
            unknown: Vec::new(),
        });
        members.push(Member {
            facts: Facts {
                agent: agent(n),
                workflow: workflow(n, *a),
                config: config(n, *a),
                slots: Vec::new(),
                runs: vec![record],
                events,
                store_ended: true,
                today: DAY,
                doing: Doing::Idle(None),
            },
            session: true,
        });
    }
    let seen = ports.tried.borrow().len();
    World {
        ports,
        board: Board::new(DAY, members),
        table: Table::new(),
        limits,
        spec: spec.to_vec(),
        seen,
    }
}

pub fn world(spec: &[A]) -> World {
    world_with(spec, Limits::PRODUCT)
}

pub fn act_name(call: &Call) -> Option<String> {
    Some(match call {
        Call::Log(events) => format!(
            "log {}",
            events
                .iter()
                .map(|e| what_name(&e.what))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Call::Send(halvo_coord::types::Body::StartRun { .. }) => "start_run".into(),
        Call::Send(halvo_coord::types::Body::SetState { state, .. }) => {
            format!("set_state {}", state.name())
        }
        Call::Send(halvo_coord::types::Body::EndRun { .. }) => "end_run".into(),
        Call::Send(_) => "send".into(),
        Call::Start(_) => "start".into(),
        Call::Cancel(_) => "cancel".into(),
        Call::Kill(_) => "kill".into(),
        Call::Defer(_) => "defer".into(),
        Call::Reap => "reap".into(),
        Call::AnswerConfirm(..) | Call::AnswerHandback(..) | Call::AnswerBind(..) => {
            "answer".into()
        }
        Call::Workflows | Call::Agents | Call::History(..) | Call::Attention(_) | Call::Changed => {
            return None
        }
    })
}

pub fn what_name(what: &What) -> String {
    match what {
        What::RunStarted { .. } => "run_started".into(),
        What::RunEnded { cause, .. } => format!("run_ended {}", cause.name()),
        What::ItemRaised { raised, .. } => format!("item_raised {}", raised.kind().name()),
        What::ItemAnswered { .. } => "item_answered".into(),
        What::ItemLapsed { lapsed, .. } => format!("item_lapsed {}", lapsed.cause().name()),
        What::ItemReleased { .. } => "item_released".into(),
        What::HoldSet { cause } => format!("hold_set {}", cause.name()),
        What::HoldCleared { .. } => "hold_cleared".into(),
        What::SlotSkipped { .. } => "slot_skipped".into(),
    }
}

impl World {
    pub fn act(&mut self, act: Act) -> Done {
        drive(
            &self.ports,
            &mut self.board,
            &mut self.table,
            &self.limits,
            act,
        )
    }

    /// The calls made since the last read, refused ones marked, reads left out.
    pub fn calls(&mut self) -> Vec<String> {
        let tried = self.ports.tried.borrow();
        let out = tried[self.seen..]
            .iter()
            .filter_map(|t| {
                act_name(&t.call).map(|n| match &t.refused {
                    None => n,
                    Some(_) => format!("{n} refused"),
                })
            })
            .collect();
        self.seen = tried.len();
        out
    }

    pub fn forget(&mut self) {
        self.seen = self.ports.tried.borrow().len();
    }

    pub fn facts(&self, n: usize) -> &Facts<Config> {
        let id = agent(n);
        &self
            .board
            .members
            .iter()
            .find(|m| m.facts.agent == id)
            .unwrap()
            .facts
    }

    /// The store ends the run as its handler of the run's end does.
    pub fn store_end(
        &self,
        n: usize,
        run: &RunId,
        outcome: halvo_coord::types::Outcome,
        unknown: u32,
    ) {
        self.ports.store_end(
            &agent(n),
            PastRun {
                run: run.clone(),
                day: self.board.day,
                outcome,
                unknown,
            },
        );
    }

    pub fn set_clock(&self, now: u64, day: u32) {
        self.ports.set_clock(now, day);
    }

    pub fn events_of(&self, n: usize) -> Vec<String> {
        self.ports
            .events
            .borrow()
            .iter()
            .filter(|e| e.agent == agent(n))
            .map(|e| what_name(&e.what))
            .collect()
    }
}
