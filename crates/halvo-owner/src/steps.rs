//! The host's step list (contracts C25.5 and C25.7): the steps, the handlers and the delete guards of the owner's host,
//! in their order. The owner builds its host from `list` alone, so a later run links its step, its handlers and its
//! delete guards by adding lines to `list`; the adapter of the agent-setup run's handlers is the module `agent`, and
//! that of the learning run's handlers and step the module `learn`.

use std::rc::Rc;

use halvo_derived::{Clock, DeleteGuard, Handler, Host, Takes};
use halvo_identify::Proposer;
use halvo_schema_core::record::{Instant, RecurrenceStep, WriterStep};

use crate::{agent, learn};

/// One line of the list after its recurrence step, with the family that it names.
pub(crate) enum Line {
    Step(&'static str, Box<dyn WriterStep>),
    Handler(&'static str, Takes, Box<dyn Handler>),
    /// A handler that reads the host's clock, made by `StepList::host` with the clock that the store reads.
    Clocked(&'static str, Takes, fn(Rc<dyn Clock>) -> Box<dyn Handler>),
    /// Lines that share what one call makes for them with the host's clock, made by `StepList::host` and added in
    /// the order in which the call returns them.
    Group(fn(Rc<dyn Clock>) -> Vec<Line>),
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

    /// Adds the lines that `make` returns with the host's clock when the host is built, in their order.
    fn group(mut self, make: fn(Rc<dyn Clock>) -> Vec<Line>) -> StepList {
        self.lines.push(Line::Group(make));
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
        let host = Host::new(Box::new(Shared(Rc::clone(&clock))), self.recurrence);
        add(host, self.lines, &clock)
    }
}

/// The host with each of `lines` added after what it holds, in their order, a group's lines in its place.
fn add(mut host: Host, lines: Vec<Line>, clock: &Rc<dyn Clock>) -> Host {
    for line in lines {
        host = match line {
            Line::Step(family, step) => host.step(family, step),
            Line::Handler(family, takes, handler) => host.handler(family, takes, handler),
            Line::Clocked(family, takes, make) => {
                host.handler(family, takes, make(Rc::clone(clock)))
            }
            Line::Group(make) => add(host, make(Rc::clone(clock)), clock),
            Line::Guard(family, guard) => host.delete_guard(family, guard),
        };
    }
    host
}

/// The list of the workflow run: the recurrence step `Proposer` of halvo-identify, which every host of that run puts
/// first, and no line after it.
fn proposer_only() -> StepList {
    StepList {
        recurrence: Box::new(Proposer),
        lines: Vec::new(),
    }
}

/// The step list of the owner. It names halvo-identify's recurrence step `Proposer`; after it the learning run's
/// lines (`learn::lines`): its step `LearnStep`, and `LearnHandlers` in front of the handlers of every `runner` request
/// and of the `coordinator_write` bodies of `learn::OPS`; then the two handlers of the agent-setup run's family
/// `agent`: the handler of every `runner` request and the handler of the `coordinator_write` bodies of the twenty-two
/// `op` names of `CoordinatorCall`; and then the interface run's family `coord`: the handler of the bodies `coord_log` and
/// `coord_history` on the host's clock, and the guard that holds a workflow's delete while a run of it is live. A later
/// run adds its lines here, in their order, a handler in front before the handler behind it:
///
/// ```text
/// proposer_only()
///     .step(family, writer_step)
///     .handler(family, takes, handler)
///     .delete_guard(family, guard)
/// ```
pub fn list() -> StepList {
    proposer_only()
        .group(learn::lines)
        .clocked(agent::FAMILY, Takes::Runner, agent::runner)
        .clocked(
            agent::FAMILY,
            Takes::Coordinator(agent::OPS),
            agent::coordinator,
        )
        .clocked(
            halvo_coord_store::FAMILY,
            Takes::Coordinator(&halvo_coord_store::OPS),
            coord,
        )
        .delete_guard(halvo_coord_store::FAMILY, halvo_coord_store::family_guard())
}

/// The interface run's handler of `coord_log` and `coord_history`, which reads the local day of each record from the
/// host's clock.
fn coord(clock: Rc<dyn Clock>) -> Box<dyn Handler> {
    Box::new(halvo_coord_store::CoordHandler::new(Box::new(Shared(
        clock,
    ))))
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

    /// The kinds of the family `agent` that `Owner::open` reads at a delete.
    const AGENT_KINDS: &[&str] = &["agent_config", "agent_start"];

    /// A delete guard that writes the rows of `kinds`, of its own family, under the workflow into `held` and holds
    /// the delete.
    fn reader(held: &Held, kinds: &'static [&'static str]) -> DeleteGuard {
        let held = Rc::clone(held);
        Box::new(move |_, rows, _| {
            for &kind in kinds {
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
            Owner::open_reading(scratch, "agent", AGENT_KINDS)
        }

        /// The owner's store as `open` makes it, whose `reader` reads the rows of `kinds` of the family `family`.
        fn open_reading(
            scratch: &Scratch,
            family: &'static str,
            kinds: &'static [&'static str],
        ) -> Owner {
            let held = Held::default();
            let host = list()
                .delete_guard(family, reader(&held, kinds))
                .host(Box::new(Noon));
            Owner::with_host(scratch, host, held)
        }

        /// The owner's store as `open` makes it, on `host`.
        fn with_host(scratch: &Scratch, host: halvo_derived::Host, held: Held) -> Owner {
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

    const RUN_2: &str = "50000000000000000000000000000002";

    /// A web step of the compiled playbook: step 1 navigates to the slot `start`, every later step clicks the button
    /// named `name` by its role.
    fn step(n: u32, name: &str, locators: &[&str]) -> Value {
        if n == 1 {
            return serde_json::json!({
                "v": 1, "n": 1, "kind": "web", "verb": "navigate", "target": null,
                "args": [{"name": "url", "from": "slot", "value": null, "key": "start"}],
                "pre": [], "post": [], "effect": "none", "class": "read",
            });
        }
        let locators: Vec<Value> = locators
            .iter()
            .map(|name| serde_json::json!({"by": "role", "value": "button", "name": name}))
            .collect();
        serde_json::json!({
            "v": 1, "n": n, "kind": "web", "verb": "click",
            "target": {"desc": format!("button {name}"), "frame": [], "credential": "no", "locators": locators},
            "args": [], "pre": [], "post": [], "effect": "none", "class": "prepare",
        })
    }

    fn compiled() -> Value {
        Value::Array(vec![
            step(1, "", &[]),
            step(2, "Pay", &["Pay"]),
            step(3, "Next", &["Next"]),
        ])
    }

    impl Owner {
        /// The runner body of `op` for the run, with the agent, the run and `more`.
        fn runner(&mut self, op: &str, run: &str, more: &str) -> String {
            let body = format!(r#"{{"op":"{op}","agent":"{AGENT}","run":"{run}"{more}}}"#);
            let reply = self.send("runner", &body);
            assert!(reply.is_ok(), "{op}: {reply:?}");
            reply.unwrap()
        }

        /// `finish` of the run in the mode with the outcome `completed`, its record naming `playbook`.
        fn finish(&mut self, run: &str, mode: &str, playbook: &str) -> String {
            let record = serde_json::json!({
                "v": 1, "workflow": self.workflow.as_str(), "agent": AGENT, "run": run, "day": DAY,
                "mode": mode, "outcome": "completed", "playbook": serde_json::from_str::<Value>(playbook).unwrap(),
                "steps": 3, "prompts": 0, "handbacks": 0, "confirmations": 0, "effects": 0, "unknown": 0,
                "model_calls": 0, "tokens_in": 0, "tokens_out": 0, "wall_s": 9, "model": null,
                "before_start": null, "cut": null, "no_key": null, "not_to_account": null,
                "incomplete": null, "reply_to_other": null, "more_pages": null, "held_recipient": null,
                "organized": null, "other_task": null, "held_earlier": null, "earlier_page": null,
            });
            self.runner("finish", run, &format!(r#","record":{record}"#))
        }

        /// The agent, then a compile run that stores the playbook under number 1 and finishes, and the agent active.
        fn compile(&mut self) {
            assert_eq!(
                self.coordinator("create_agent", "").as_deref(),
                Ok(r#"{"rev":1}"#)
            );
            let start = format!(r#","run":"{RUN}","mode":"compile""#);
            assert_eq!(
                self.coordinator("start_run", &start).as_deref(),
                Ok(r#"{"ok":"yes"}"#)
            );
            self.runner("load", RUN, "");
            let put = format!(r#","number":1,"steps":{}"#, compiled());
            assert_eq!(self.runner("put_playbook", RUN, &put), r#"{"stored":3}"#);
            self.finish(RUN, "compile", "1");
            let active = self.coordinator("set_state", r#","state":"active""#);
            assert!(active.is_ok(), "{active:?}");
        }

        /// The reply of `list_refinements` for the workflow and the agent.
        fn refinements(&mut self) -> Result<String, Error> {
            self.coordinator("list_refinements", "")
        }
    }

    /// U8.1 sentence 3 (L11.5): a run through the owner's step list keeps its run record, the `learn_run` rows of
    /// rev 1 at its start and rev 2 at its finish, and the agent's routine row with its mark in the owner's store.
    #[test]
    fn a_run_through_the_owner_keeps_its_run_record_and_routine_mark() {
        let scratch = Scratch::new("owner-learn-run");
        let mut owner = Owner::open_reading(&scratch, "learn", &["learn_run", "learn_routine"]);
        owner.compile();

        let rows = owner.rows();
        let runs: Vec<&Value> = rows
            .iter()
            .filter(|(kind, row)| *kind == "learn_run" && row["run"] == RUN)
            .map(|(_, row)| row)
            .collect();
        let revs: Vec<&Value> = runs.iter().map(|row| &row["rev"]).collect();
        assert_eq!(revs, [1, 2], "{runs:?}");
        assert_eq!(runs[1]["state"], "verified");
        assert_eq!(runs[1]["day"], DAY);
        let routines: Vec<&Value> = rows
            .iter()
            .filter(|(kind, _)| *kind == "learn_routine")
            .map(|(_, row)| row)
            .collect();
        assert_eq!(routines.len(), 1, "{routines:?}");
        assert_eq!(routines[0]["agent"], AGENT);
        assert_eq!(routines[0]["workflow"], owner.workflow.as_str());
        assert_eq!(routines[0]["mark"], "none");
    }

    /// pr-review-25 F2: a fault of the store at the agent store's read of the workflow, made behind `LearnHandlers`,
    /// is answered `store` through the owner, as the agent store's handler of the list answers it, and never as the
    /// refusal that the empty read would give.
    #[test]
    fn a_store_fault_at_the_workflow_read_is_answered_store_through_the_owner() {
        let scratch = Scratch::new("owner-learn-fault");
        let mut owner = Owner::open(&scratch);
        crate::learn::tests::FAULTED_WORKFLOW_READS.with(|on| on.set(true));
        let answer = owner.coordinator("create_agent", "");
        crate::learn::tests::FAULTED_WORKFLOW_READS.with(|on| on.set(false));
        assert_eq!(answer, Err(Error::Refused(ErrorName::Store)));
    }

    impl Owner {
        /// A replay run's `start_run` and its first `load`.
        fn start_replay(&mut self, run: &str) {
            let start = format!(r#","run":"{run}","mode":"replay""#);
            assert_eq!(
                self.coordinator("start_run", &start).as_deref(),
                Ok(r#"{"ok":"yes"}"#)
            );
            self.runner("load", run, "");
        }

        /// The run's `put_playbook` under number 1 that proposes a new target for step 2.
        fn propose(&mut self, run: &str) -> Result<String, Error> {
            let proposed = Value::Array(vec![
                step(1, "", &[]),
                step(2, "Pay now", &["A", "Pay"]),
                step(3, "Next", &["Next"]),
            ]);
            let body = format!(
                r#"{{"op":"put_playbook","agent":"{AGENT}","run":"{run}","number":1,"steps":{proposed}}}"#
            );
            self.send("runner", &body)
        }

        /// The request with every read of a workflow behind answered as a fault of the store.
        fn faulted(&mut self, name: &str, body: &str) -> Result<String, Error> {
            crate::learn::tests::FAULTED_WORKFLOW_READS.with(|on| on.set(true));
            let answer = self.send(name, body);
            crate::learn::tests::FAULTED_WORKFLOW_READS.with(|on| on.set(false));
            answer
        }
    }

    /// verifier-32's case y2 (sentence 1 through the owner): a replay `put_playbook` whose own load meets a fault of
    /// the store at the workflow read answers its count, not `store`.
    #[test]
    fn a_replay_put_at_a_faulted_workflow_read_answers_its_count_through_the_owner() {
        let scratch = Scratch::new("owner-learn-fault-put");
        let mut owner = Owner::open(&scratch);
        owner.compile();
        owner.start_replay(RUN_2);
        owner.runner("load_steps", RUN_2, r#","number":1,"from":1"#);
        crate::learn::tests::FAULTED_WORKFLOW_READS.with(|on| on.set(true));
        let stored = owner.propose(RUN_2);
        crate::learn::tests::FAULTED_WORKFLOW_READS.with(|on| on.set(false));
        assert_eq!(stored.as_deref(), Ok(r#"{"stored":0}"#));
    }

    /// verifier-32's case y1 (L4.2, the row of `load`): one `load` of a replay run that meets a fault of the store at
    /// the workflow read is answered `store` and keeps the run: after a good `load` its proposal is taken and the
    /// learned list holds the edit after its finish.
    #[test]
    fn one_faulted_load_does_not_lose_a_replay_run_through_the_owner() {
        let scratch = Scratch::new("owner-learn-fault-load");
        let mut owner = Owner::open(&scratch);
        owner.compile();
        owner.start_replay(RUN_2);
        let load = format!(r#"{{"op":"load","agent":"{AGENT}","run":"{RUN_2}"}}"#);
        assert_eq!(
            owner.faulted("runner", &load),
            Err(Error::Refused(ErrorName::Store))
        );
        owner.runner("load", RUN_2, "");
        let served = owner.runner("load_steps", RUN_2, r#","number":1,"from":1"#);
        let served: Value = serde_json::from_str(&served).unwrap();
        assert_eq!(served["steps"], compiled());
        assert_eq!(owner.propose(RUN_2).as_deref(), Ok(r#"{"stored":1}"#));
        let line = |seq: u32, event: &str| {
            serde_json::json!({
                "v": 1, "workflow": owner.workflow.as_str(), "agent": AGENT, "run": RUN_2, "seq": seq,
                "day": DAY, "at": Noon.now().0, "event": event, "step": 2, "cause": null, "model": null,
                "ms": null, "tokens_in": null, "tokens_out": null, "page_amount": null,
            })
        };
        let records = Value::Array(vec![line(2, "recovered"), line(3, "step_ok")]);
        owner.runner("log", RUN_2, &format!(r#","records":{records}"#));
        owner.finish(RUN_2, "replay", "null");
        assert_eq!(
            owner.refinements().as_deref(),
            Ok(format!(r#"{{"refinements":[{{"day":{DAY},"what":"target_learned"}}]}}"#).as_str())
        );
    }

    /// U5.4a sentence 3 (ruling RD-64 1.6 (a); the deletion of the store): through the host of `list()` alone, a
    /// workflow's delete waits while a run of it is live and goes after the run's end. With a `run_started` row stored
    /// through `coord_log` and no `run_ended`, the decision answers `state` and removes nothing; once the `run_ended`
    /// is stored, the same decision answers ok.
    #[test]
    fn a_workflows_delete_waits_while_its_run_is_live_and_goes_after_its_end() {
        let scratch = Scratch::new("owner-coord-guard");
        let mut owner = Owner::with_host(&scratch, list().host(Box::new(Noon)), Held::default());
        assert_eq!(
            owner.coordinator("create_agent", "").as_deref(),
            Ok(r#"{"rev":1}"#)
        );
        let workflow = owner.workflow.as_str().to_owned();
        let row = |seq: u32, event: &str, cause: &str, trigger: &str| {
            format!(
                r#"{{"v":1,"workflow":"{workflow}","agent":"{AGENT}","seq":{seq},"day":{DAY},"at":{},"event":"{event}","run":"{RUN}","slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":{cause},"trigger":{trigger},"origin":null,"defer_launch":null,"defer_ticks_failed":null}}"#,
                Noon.now().0
            )
        };
        let log = |owner: &mut Owner, record: String| {
            let records = format!(r#","records":[{record}]"#);
            assert_eq!(
                owner.coordinator("coord_log", &records).as_deref(),
                Ok(r#"{"stored":1}"#)
            );
        };
        log(&mut owner, row(1, "run_started", "null", r#""person""#));
        let role = InterfaceRole::claim();
        let delete = InterfaceDecision::delete(
            &role,
            RequestId::from_random([8; 16]),
            owner.workflow.clone(),
        );
        assert_eq!(
            owner.store.decide(&delete),
            Err(Error::Refused(ErrorName::State)),
            "the delete of a workflow whose run is live"
        );
        Owner::stored(&owner.store);

        log(&mut owner, row(2, "run_ended", r#""completed""#, "null"));
        let after = owner.store.decide(&delete);
        assert!(after.is_ok(), "the delete after the run's end: {after:?}");
    }

    const AGENT_2: &str = "40000000000000000000000000000002";

    impl Owner {
        /// A second workflow, which `Proposer` proposes for three summaries of another app, approved by the person.
        fn second_workflow(&mut self) -> WorkflowId {
            let episodes: Vec<Episode> = (4u8..=6)
                .map(|n| {
                    let mut episode = episode(n, DAY + u32::from(n) - 6);
                    episode.app = AppId::parse("com.example.mail").unwrap();
                    episode
                })
                .collect();
            self.store
                .commit_batch(
                    &StoreId::parse(&"1".repeat(32)).unwrap(),
                    &episodes,
                    Position { seg: 8, idx: 0 },
                    &mut Seq(200),
                )
                .unwrap();
            let role = InterfaceRole::claim();
            let read = InterfaceRead::workflows(&role, RequestId::from_random([9; 16]), None);
            let Ok(ReplyResult::Workflows(page)) = self.store.interface_read(&read) else {
                panic!("the read of the workflows fails");
            };
            let other = page
                .workflows
                .iter()
                .map(|item| item.workflow.id().clone())
                .find(|id| *id != self.workflow)
                .expect("a second workflow");
            let approve =
                InterfaceDecision::approve(&role, RequestId::from_random([10; 16]), other.clone());
            self.store.decide(&approve).unwrap();
            other
        }
    }

    /// The condition of round 2: `list_overlaps` reads the workflow of every other agent with bindings and takes one
    /// that it cannot read as not live, so the agent store's code answers `Ok` from a read that met a fault. Through
    /// the owner, no door of the handlers takes that fault, and the answer is `store`, though the handlers answered
    /// `Ok`.
    #[test]
    fn an_overlaps_read_at_a_faulted_workflow_read_is_answered_store_through_the_owner() {
        let scratch = Scratch::new("owner-learn-fault-overlaps");
        let mut owner = Owner::open(&scratch);
        let other = owner.second_workflow();
        assert_eq!(
            owner.coordinator("create_agent", "").as_deref(),
            Ok(r#"{"rev":1}"#)
        );
        let slots = r#"[{"name":"account_address","value":["me@mail.test"]},{"name":"reply_senders","value":["ann@people.test"]}]"#;
        for (op, more) in [
            ("create_agent", String::new()),
            ("set_bindings", format!(r#","slots":{slots}"#)),
        ] {
            let body = format!(
                r#"{{"op":"{op}","workflow":"{}","agent":"{AGENT_2}"{more}}}"#,
                other.as_str()
            );
            assert_eq!(
                owner.send("coordinator_write", &body).as_deref(),
                Ok(r#"{"rev":1}"#),
                "{op}"
            );
        }
        let more = format!(r#","slots":{slots}"#);
        let listed: Value =
            serde_json::from_str(&owner.coordinator("list_overlaps", &more).unwrap()).unwrap();
        assert_eq!(
            listed,
            serde_json::json!({
                "overlaps": [{"workflow": other.as_str(), "agent": AGENT_2, "senders": ["ann@people.test"]}],
                "more": "no",
            })
        );
        crate::learn::tests::FAULTED_WORKFLOW.with(|one| *one.borrow_mut() = Some(other.clone()));
        let answer = owner.coordinator("list_overlaps", &more);
        crate::learn::tests::FAULTED_WORKFLOW.with(|one| *one.borrow_mut() = None);
        assert_eq!(answer, Err(Error::Refused(ErrorName::Store)));
    }

    /// U8.1 sentence 4 (L2.2): `list_refinements` through the owner answers the learned list: empty while nothing
    /// proposes, and after a replay run whose proposal for step 2 was taken, the one edit that it learned.
    #[test]
    fn list_refinements_through_the_owner_answers_the_learned_list() {
        let scratch = Scratch::new("owner-learn-list");
        let mut owner = Owner::open(&scratch);
        owner.compile();
        assert_eq!(owner.refinements().as_deref(), Ok(r#"{"refinements":[]}"#));

        let start = format!(r#","run":"{RUN_2}","mode":"replay""#);
        assert_eq!(
            owner.coordinator("start_run", &start).as_deref(),
            Ok(r#"{"ok":"yes"}"#)
        );
        owner.runner("load", RUN_2, "");
        let served = owner.runner("load_steps", RUN_2, r#","number":1,"from":1"#);
        let served: Value = serde_json::from_str(&served).unwrap();
        assert_eq!(served["steps"], compiled());
        let proposed = Value::Array(vec![
            step(1, "", &[]),
            step(2, "Pay now", &["A", "Pay"]),
            step(3, "Next", &["Next"]),
        ]);
        let put = format!(r#","number":1,"steps":{proposed}"#);
        assert_eq!(owner.runner("put_playbook", RUN_2, &put), r#"{"stored":1}"#);
        let line = |seq: u32, event: &str| {
            serde_json::json!({
                "v": 1, "workflow": owner.workflow.as_str(), "agent": AGENT, "run": RUN_2, "seq": seq,
                "day": DAY, "at": Noon.now().0, "event": event, "step": 2, "cause": null, "model": null,
                "ms": null, "tokens_in": null, "tokens_out": null, "page_amount": null,
            })
        };
        let records = Value::Array(vec![line(2, "recovered"), line(3, "step_ok")]);
        owner.runner("log", RUN_2, &format!(r#","records":{records}"#));
        owner.finish(RUN_2, "replay", "null");

        assert_eq!(
            owner.refinements().as_deref(),
            Ok(format!(r#"{{"refinements":[{{"day":{DAY},"what":"target_learned"}}]}}"#).as_str())
        );
    }
}
