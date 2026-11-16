use crate::bell::Bell;
use halvo_coord::guard::Policy;
use halvo_coord::ports::{Analysis, Clock, Owner, Page, PortError, Reaped, Rng, Runs, Tray};
use halvo_coord::types::{
    Agent, AgentId, Answer, BindResult, Body, Class, Decision, Event, HandbackResult, Reply,
    RunHistory, RunId, RunSpec, Workflow, WorkflowId,
};
use std::sync::Arc;

/// The ports of the app: every port of `P` as it is, and the ports `Page` and `Tray` answered
/// by the bell, so that a change of the coordinator's state wakes the waiting call and its count
/// of agents under `needs_you` reaches the tray (I8.2, I10.3).
pub struct Wired<P> {
    ports: P,
    bell: Arc<Bell>,
}

impl<P> Wired<P> {
    /// `ports` with the bell.
    pub fn new(ports: P, bell: Arc<Bell>) -> Wired<P> {
        Wired { ports, bell }
    }

    /// The ports of `P`.
    pub fn inner(&self) -> &P {
        &self.ports
    }
}

impl<P: Owner> Owner for Wired<P> {
    type Config = P::Config;

    fn log(&self, events: &[Event]) -> Result<(), PortError> {
        self.ports.log(events)
    }

    fn history(&self, agent: &AgentId, after: u32) -> Result<Vec<Event>, PortError> {
        self.ports.history(agent, after)
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        self.ports.workflows()
    }

    fn agents(&self) -> Result<Vec<Agent<Self::Config>>, PortError> {
        self.ports.agents()
    }

    fn send(&self, body: &Body) -> Result<Reply, PortError> {
        self.ports.send(body)
    }

    fn agent_history(
        &self,
        workflow: &WorkflowId,
        agent: &AgentId,
        runs: u32,
    ) -> Result<Vec<RunHistory>, PortError> {
        self.ports.agent_history(workflow, agent, runs)
    }
}

impl<P: Runs> Runs for Wired<P> {
    fn start(&self, spec: &RunSpec) -> Result<(), PortError> {
        self.ports.start(spec)
    }

    fn answer_confirm(
        &self,
        run: &RunId,
        answers: &[Answer],
        record: &mut dyn FnMut(&Decision) -> Result<(), PortError>,
    ) -> Result<(), PortError> {
        self.ports.answer_confirm(run, answers, record)
    }

    fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), PortError> {
        self.ports.answer_handback(run, result)
    }

    fn answer_bind(&self, run: &RunId, result: BindResult) -> Result<(), PortError> {
        self.ports.answer_bind(run, result)
    }

    fn cancel(&self, run: &RunId) -> Result<(), PortError> {
        self.ports.cancel(run)
    }

    fn kill(&self, run: &RunId) -> Result<(), PortError> {
        self.ports.kill(run)
    }

    fn reap(&self) -> Result<Vec<(RunId, Reaped)>, PortError> {
        self.ports.reap()
    }
}

impl<P: Analysis> Analysis for Wired<P> {
    fn defer(&self, until: u64) -> Result<(), PortError> {
        self.ports.defer(until)
    }
}

impl<P: Clock> Clock for Wired<P> {
    fn now(&self) -> Result<u64, PortError> {
        self.ports.now()
    }

    fn day(&self, at: u64) -> Result<u32, PortError> {
        self.ports.day(at)
    }
}

impl<P: Rng> Rng for Wired<P> {
    fn bytes16(&self) -> Result<[u8; 16], PortError> {
        self.ports.bytes16()
    }
}

impl<P: Policy> Policy for Wired<P> {
    type Config = P::Config;

    fn gives_run(&self, config: &Self::Config, class: Class) -> Result<bool, PortError> {
        self.ports.gives_run(config, class)
    }
}

impl<P> Page for Wired<P> {
    fn changed(&self) -> Result<(), PortError> {
        self.bell.ring();
        Ok(())
    }
}

impl<P> Tray for Wired<P> {
    fn attention(&self, count: u32) -> Result<(), PortError> {
        self.bell.attention(count);
        Ok(())
    }
}
