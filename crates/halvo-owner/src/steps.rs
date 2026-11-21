//! The host's step list (contracts C25.5 and C25.7): the steps, the handlers and the delete guards of the owner's host,
//! in their order. The owner builds its host from `list` alone, so a later run links its step, its handlers and its
//! delete guards by adding lines to `list`; the adapter of the agent-setup run's handlers is the module `agent`.

use std::rc::Rc;

use halvo_derived::{Clock, DeleteGuard, Handler, Host, Takes};
use halvo_identify::Proposer;
use halvo_schema_core::record::{Instant, RecurrenceStep, WriterStep};

use crate::agent;

/// One line of the list after its recurrence step, with the family that it names.
enum Line {
    Step(&'static str, Box<dyn WriterStep>),
    Handler(&'static str, Takes, Box<dyn Handler>),
    /// A handler that reads the host's clock, made by `StepList::host` with the clock that the store reads.
    Clocked(&'static str, Takes, fn(Rc<dyn Clock>) -> Box<dyn Handler>),
    Guard(&'static str, DeleteGuard),
}

/// The host's one clock, read by the store and by the handlers of the list.
struct Shared(Rc<dyn Clock>);

impl Clock for Shared {
    fn now(&self) -> Instant {
        self.0.now()
    }

    fn day(&self, at: Instant) -> u32 {
        self.0.day(at)
    }

    fn day_start(&self, day: u32) -> Instant {
        self.0.day_start(day)
    }
}

/// The recurrence step of a host and, after it, its lines in the order in which they were added.
pub struct StepList {
    recurrence: Box<dyn RecurrenceStep>,
    lines: Vec<Line>,
}

impl StepList {
    /// Adds a step after the recurrence step and after the steps added before it (`Host::step`).
    pub fn step(mut self, family: &'static str, step: Box<dyn WriterStep>) -> StepList {
        self.lines.push(Line::Step(family, step));
        self
    }

    /// Adds a handler of the requests that `takes` names, behind the handlers of those requests added before it
    /// (`Host::handler`): the handler in front is added before the handler behind it, and the handler of the owning
    /// run last.
    pub fn handler(
        mut self,
        family: &'static str,
        takes: Takes,
        handler: Box<dyn Handler>,
    ) -> StepList {
        self.lines.push(Line::Handler(family, takes, handler));
        self
    }

    /// Adds a handler as `handler` does, made by `make` with the host's clock when the host is built.
    fn clocked(
        mut self,
        family: &'static str,
        takes: Takes,
        make: fn(Rc<dyn Clock>) -> Box<dyn Handler>,
    ) -> StepList {
        self.lines.push(Line::Clocked(family, takes, make));
        self
    }

    /// Adds a delete guard behind the guards added before it (`Host::delete_guard`).
    pub fn delete_guard(mut self, family: &'static str, guard: DeleteGuard) -> StepList {
        self.lines.push(Line::Guard(family, guard));
        self
    }

    /// The host of this list with `clock`: its recurrence step first, then each line in the order of the list. The
    /// store and every handler made with the clock read this one clock.
    pub fn host(self, clock: Box<dyn Clock>) -> Host {
        let clock: Rc<dyn Clock> = Rc::from(clock);
        let mut host = Host::new(Box::new(Shared(Rc::clone(&clock))), self.recurrence);
        for line in self.lines {
            host = match line {
                Line::Step(family, step) => host.step(family, step),
                Line::Handler(family, takes, handler) => host.handler(family, takes, handler),
                Line::Clocked(family, takes, make) => {
                    host.handler(family, takes, make(Rc::clone(&clock)))
                }
                Line::Guard(family, guard) => host.delete_guard(family, guard),
            };
        }
        host
    }
}

/// The list of the workflow run: the recurrence step `Proposer` of halvo-identify, which every host of that run puts
/// first, and no line after it.
fn proposer_only() -> StepList {
    StepList {
        recurrence: Box::new(Proposer),
        lines: Vec::new(),
    }
}

/// The step list of the owner. It names halvo-identify's recurrence step `Proposer`, and after it the two handlers
/// of the agent-setup run's family `agent`: the handler of every `runner` request and the handler of the
/// `coordinator_write` bodies of the nine `op` names of `CoordinatorCall`. A later run adds its lines here, in their
/// order, a handler in front before the handler behind it:
///
/// ```text
/// proposer_only()
///     .step(family, writer_step)
///     .handler(family, takes, handler)
///     .delete_guard(family, guard)
/// ```
pub fn list() -> StepList {
    proposer_only()
        .clocked(agent::FAMILY, Takes::Runner, agent::runner)
        .clocked(
            agent::FAMILY,
            Takes::Coordinator(agent::OPS),
            agent::coordinator,
        )
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;

    use halvo_derived::{
        Clock, DbKey, DeleteGuard, Error, GuardAnswer, Handler, HandlerCx, IdSource, Store, Takes,
    };
    use halvo_schema::executable::RunnerReply;
    use halvo_schema_core::category::SeedCategory;
    use halvo_schema_core::ids::{Position, RequestId, StoreId, SummaryId, WorkflowId};
    use halvo_schema_core::kinds;
    use halvo_schema_core::record::{
        Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Tagged, Text, Workflow,
        WorkflowState,
    };
    use halvo_schema_core::request::{
        self, Decoded, ErrorName, InterfaceDecision, InterfaceRead, InterfaceRole, RawRequest,
        ReplyResult,
    };
    use halvo_schema_core::site::AppId;
    use serde_json::value::RawValue;
    use serde_json::Value;

    use super::{list, proposer_only};

    const DAY_NS: i64 = 86_400_000_000_000;
    const DAY: u32 = 20_767;

    /// A clock that stands at noon of `DAY`, with UTC as local time.
    struct Noon;

    impl Clock for Noon {
        fn now(&self) -> Instant {
            Instant(i64::from(DAY) * DAY_NS + DAY_NS / 2)
        }

        fn day(&self, at: Instant) -> u32 {
            u32::try_from(at.0.div_euclid(DAY_NS)).unwrap_or(0)
        }

        fn day_start(&self, day: u32) -> Instant {
            Instant(i64::from(day) * DAY_NS)
        }
    }

    struct Seq(u8);

    impl IdSource for Seq {
        fn fill(&mut self, bytes: &mut [u8]) {
            self.0 = self.0.wrapping_add(1);
            bytes.fill(self.0);
        }
    }

    /// A runner handler that writes its name down and, where `pass` holds, passes the request on.
    struct Named {
        name: &'static str,
        pass: bool,
        seen: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Handler for Named {
        fn handle(
            &self,
            _request: &RawRequest,
            cx: &mut HandlerCx<'_>,
        ) -> Result<Box<RawValue>, ErrorName> {
            self.seen.borrow_mut().push(self.name);
            if self.pass {
                cx.pass_on()
            } else {
                RawValue::from_string("null".to_owned()).map_err(|_| ErrorName::Internal)
            }
        }
    }

    /// A new directory under the target's tmp directory, `<target>/<profile>/deps/<test binary>` three steps up,
    /// removed when the value is dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let exe = std::env::current_exe().unwrap();
            let target = exe.ancestors().nth(3).unwrap().join("tmp");
            Scratch(halvo_test_dir::make(&target, name).unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Err(e) = std::fs::remove_dir_all(&self.0) {
                eprintln!(
                    "the scratch directory {} was not removed: {e}",
                    self.0.display()
                );
            }
        }
    }

    fn episode(n: u8, day: u32) -> Episode {
        let start = i64::from(day) * DAY_NS + 64_800_000_000_000 + i64::from(n);
        Episode {
            id: SummaryId::from_random([n; 16]),
            category: SeedCategory::UtilityBillPayment,
            site: None,
            app: AppId::parse("com.example.browser").unwrap(),
            day,
            part: Part::Evening,
            minutes: 5,
            keys: None,
            pointer: None,
            segments: vec![Ordinal::new(u64::from(n)).unwrap()],
            expires: Instant(i64::MAX),
            start: Instant(start),
            end: Instant(start + 1_000),
            steps: vec![ObservedStep {
                app: None,
                title: Some(Tagged {
                    value: Text::parse("Pay bill").unwrap(),
                    layer: FloorLayer::Floor,
                }),
                input: None,
                site: None,
                text: None,
            }],
        }
    }

    /// C25.5: the list's builder puts each handler in the host in the order of the list, so the handler named first
    /// gets a request first and reaches the next by `pass_on`, and a delete guard of the list is asked by a delete.
    /// It builds on the list without the lines of `list`, whose own runner handler would stand in front.
    #[test]
    fn the_host_is_built_in_the_order_of_the_list() {
        let scratch = Scratch::new("owner-steps");
        let seen = Rc::new(RefCell::new(Vec::new()));
        let asked = Rc::new(Cell::new(0));
        let guard_asked = Rc::clone(&asked);
        let host = proposer_only()
            .handler(
                "test",
                Takes::Runner,
                Box::new(Named {
                    name: "first",
                    pass: true,
                    seen: Rc::clone(&seen),
                }),
            )
            .handler(
                "test",
                Takes::Runner,
                Box::new(Named {
                    name: "second",
                    pass: false,
                    seen: Rc::clone(&seen),
                }),
            )
            .delete_guard(
                "test",
                Box::new(move |_, _, _| {
                    guard_asked.set(guard_asked.get() + 1);
                    GuardAnswer::Wait
                }),
            )
            .host(Box::new(Noon));
        let entries: Vec<_> = kinds::registry().collect();
        let mut store = Store::open(
            &scratch.0,
            &DbKey::new([9; 32]),
            &entries,
            host,
            &mut Seq(0),
        )
        .unwrap();

        let outcome = store
            .commit_batch(
                &StoreId::parse(&"1".repeat(32)).unwrap(),
                &[episode(1, DAY - 2), episode(2, DAY - 1), episode(3, DAY)],
                Position { seg: 4, idx: 0 },
                &mut Seq(100),
            )
            .unwrap();
        assert_eq!(
            outcome.proposals.stored, 1,
            "Proposer proposes for three summaries of one key"
        );

        let raw = format!(
            r#"{{"v":1,"id":"{}","request":"runner","body":{{"probe":1}}}}"#,
            "5".repeat(32)
        );
        let Ok(Decoded::Raw(raw)) = request::decode(raw.as_bytes()) else {
            panic!("the runner request does not decode as a raw request");
        };
        store.handle(&raw).unwrap();
        assert_eq!(*seen.borrow(), ["first", "second"]);

        let role = InterfaceRole::claim();
        let read = InterfaceRead::workflows(&role, RequestId::from_random([6; 16]), None);
        let Ok(ReplyResult::Workflows(page)) = store.interface_read(&read) else {
            panic!("the read of the workflows fails");
        };
        assert_eq!(page.workflows.len(), 1);
        let id = page.workflows[0].workflow.id().clone();
        let delete = InterfaceDecision::delete(&role, RequestId::from_random([7; 16]), id);
        assert_eq!(store.decide(&delete), Err(Error::Refused(ErrorName::State)));
        assert_eq!(asked.get(), 1);
        let Ok(ReplyResult::Workflows(page)) = store.interface_read(&read) else {
            panic!("the read of the workflows fails");
        };
        assert_eq!(page.workflows.len(), 1, "the guard held the delete");
        drop(store);
    }

    const AGENT: &str = "40000000000000000000000000000001";
    const RUN: &str = "50000000000000000000000000000001";

    type Held = Rc<RefCell<Vec<(&'static str, Value)>>>;

    /// A delete guard of the family `agent` that writes the rows of `agent_config` and `agent_start` under the
    /// workflow into `held` and holds the delete.
    fn reader(held: &Held) -> DeleteGuard {
        let held = Rc::clone(held);
        Box::new(move |_, rows, _| {
            for kind in ["agent_config", "agent_start"] {
                rows.for_each(kind, &mut |row| {
                    held.borrow_mut()
                        .push((kind, serde_json::from_str(row).unwrap()));
                });
            }
            GuardAnswer::Wait
        })
    }

    /// The owner's store of `list()` with the clock `Noon`, with `reader` behind its lines, and its one workflow, the
    /// one that `Proposer` proposes for three summaries of one key, approved by the person.
    struct Owner {
        store: Store,
        workflow: WorkflowId,
        held: Held,
        sent: u8,
    }

    impl Owner {
        fn open(scratch: &Scratch) -> Owner {
            let held = Held::default();
            let host = list()
                .delete_guard("agent", reader(&held))
                .host(Box::new(Noon));
            let entries: Vec<_> = kinds::registry().collect();
            let mut store = Store::open(
                &scratch.0,
                &DbKey::new([9; 32]),
                &entries,
                host,
                &mut Seq(0),
            )
            .unwrap();
            store
                .commit_batch(
                    &StoreId::parse(&"1".repeat(32)).unwrap(),
                    &[episode(1, DAY - 2), episode(2, DAY - 1), episode(3, DAY)],
                    Position { seg: 4, idx: 0 },
                    &mut Seq(100),
                )
                .unwrap();
            let workflow = Owner::stored(&store).id().clone();
            let role = InterfaceRole::claim();
            let approve = InterfaceDecision::approve(
                &role,
                RequestId::from_random([7; 16]),
                workflow.clone(),
            );
            store.decide(&approve).unwrap();
            Owner {
                store,
                workflow,
                held,
                sent: 0,
            }
        }

        /// The store's one workflow, as the read `workflows` gives it.
        fn stored(store: &Store) -> Workflow {
            let role = InterfaceRole::claim();
            let read = InterfaceRead::workflows(&role, RequestId::from_random([6; 16]), None);
            let Ok(ReplyResult::Workflows(page)) = store.interface_read(&read) else {
                panic!("the read of the workflows fails");
            };
            assert_eq!(page.workflows.len(), 1);
            page.workflows[0].workflow.clone()
        }

        /// The reply of the store to one request of the name with the body, as the owner's reader decodes it.
        fn send(&mut self, name: &str, body: &str) -> Result<String, Error> {
            self.sent += 1;
            let text = format!(
                r#"{{"v":1,"id":"{}","request":"{name}","body":{body}}}"#,
                format!("{:02x}", self.sent).repeat(16)
            );
            let Ok(Decoded::Raw(raw)) = request::decode(text.as_bytes()) else {
                panic!("the request does not decode as a raw request");
            };
            self.store.handle(&raw).map(|reply| reply.get().to_owned())
        }

        /// The `coordinator_write` body of the op with the workflow, the agent and `more`.
        fn coordinator(&mut self, op: &str, more: &str) -> Result<String, Error> {
            let body = format!(
                r#"{{"op":"{op}","workflow":"{}","agent":"{AGENT}"{more}}}"#,
                self.workflow.as_str()
            );
            self.send("coordinator_write", &body)
        }

        /// The rows that `reader` reads under the workflow, at a delete that it holds.
        fn rows(&mut self) -> Vec<(&'static str, Value)> {
            let role = InterfaceRole::claim();
            let delete = InterfaceDecision::delete(
                &role,
                RequestId::from_random([8; 16]),
                self.workflow.clone(),
            );
            assert_eq!(
                self.store.decide(&delete),
                Err(Error::Refused(ErrorName::State))
            );
            self.held.take()
        }
    }

    /// U9.1 (A7.5, W-C25.7): a `coordinator_write` of `create_agent` reaches `halvo_agent_store::coordinator`, which
    /// stores the agent's configuration and start row and names the agent in the approved workflow.
    #[test]
    fn a_coordinator_write_creates_an_agent_in_the_owners_store() {
        let scratch = Scratch::new("owner-agent-create");
        let mut owner = Owner::open(&scratch);

        assert_eq!(
            owner.coordinator("create_agent", "").as_deref(),
            Ok(r#"{"rev":1}"#)
        );

        let Workflow::Full(workflow) = Owner::stored(&owner.store) else {
            panic!("the workflow is not a full workflow");
        };
        assert_eq!(workflow.state, WorkflowState::Approved);
        assert_eq!(
            workflow.agent.as_ref().map(|agent| agent.as_str()),
            Some(AGENT)
        );
        let rows = owner.rows();
        let names: Vec<&str> = rows.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(names, ["agent_config", "agent_start"]);
        for (kind, row) in &rows {
            assert_eq!(row["agent"], AGENT, "{kind}");
            assert_eq!(row["workflow"], owner.workflow.as_str(), "{kind}");
        }
        assert_eq!(rows[0].1["rev"], 1);
    }

    /// U9.1 (A7.5): a `runner` request reaches `halvo_agent_store::runner` and reads the rows that the coordinator's
    /// bodies stored: `load` of the run that `start_run` accepted answers with the agent's configuration.
    #[test]
    fn a_runner_request_loads_what_the_coordinator_stored() {
        let scratch = Scratch::new("owner-agent-load");
        let mut owner = Owner::open(&scratch);
        assert_eq!(
            owner.coordinator("create_agent", "").as_deref(),
            Ok(r#"{"rev":1}"#)
        );
        let start = format!(r#","run":"{RUN}","mode":"compile""#);
        assert_eq!(
            owner.coordinator("start_run", &start).as_deref(),
            Ok(r#"{"ok":"yes"}"#)
        );

        let load = format!(r#"{{"op":"load","agent":"{AGENT}","run":"{RUN}"}}"#);
        let reply = owner.send("runner", &load).unwrap();
        let Ok(RunnerReply::Load(loaded)) = RunnerReply::parse(reply.as_bytes()) else {
            panic!("the reply is not the reply of load: {reply}");
        };
        assert_eq!(loaded.config.agent.as_str(), AGENT);
        assert_eq!(&loaded.config.workflow, &owner.workflow);
        assert_eq!(loaded.config.rev.get(), 1);
        assert_eq!(
            loaded.start.map(|start| start.agent.as_str().to_owned()),
            Some(AGENT.to_owned())
        );
    }

    /// U9.1: the handler takes the request's instant and day from the clock that `StepList::host` received, the clock
    /// of the store: the start row that `create_agent` builds holds `Noon`'s instant and its day.
    #[test]
    fn a_stored_row_takes_the_day_of_the_hosts_clock() {
        let scratch = Scratch::new("owner-agent-day");
        let mut owner = Owner::open(&scratch);
        assert_eq!(
            owner.coordinator("create_agent", "").as_deref(),
            Ok(r#"{"rev":1}"#)
        );

        let rows = owner.rows();
        let Some((_, start)) = rows.iter().find(|(kind, _)| *kind == "agent_start") else {
            panic!("no start row is stored");
        };
        assert_eq!(start["day"], DAY);
        assert_eq!(start["at"], Noon.now().0);
    }
}
