//! The host's step list (contracts C25.5 and C25.7): the steps, the handlers and the delete guards of the owner's host,
//! in their order. The owner builds its host from `list` alone, so a later run links its step, its handlers and its
//! delete guards by adding lines to `list`, and to no other file of this crate.

use halvo_derived::{Clock, DeleteGuard, Handler, Host, Takes};
use halvo_identify::Proposer;
use halvo_schema_core::record::{RecurrenceStep, WriterStep};

/// One line of the list after its recurrence step, with the family that it names.
enum Line {
    Step(&'static str, Box<dyn WriterStep>),
    Handler(&'static str, Takes, Box<dyn Handler>),
    Guard(&'static str, DeleteGuard),
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

    /// Adds a delete guard behind the guards added before it (`Host::delete_guard`).
    pub fn delete_guard(mut self, family: &'static str, guard: DeleteGuard) -> StepList {
        self.lines.push(Line::Guard(family, guard));
        self
    }

    /// The host of this list with `clock`: its recurrence step first, then each line in the order of the list.
    pub fn host(self, clock: Box<dyn Clock>) -> Host {
        let mut host = Host::new(clock, self.recurrence);
        for line in self.lines {
            host = match line {
                Line::Step(family, step) => host.step(family, step),
                Line::Handler(family, takes, handler) => host.handler(family, takes, handler),
                Line::Guard(family, guard) => host.delete_guard(family, guard),
            };
        }
        host
    }
}

/// The step list of the owner. It names halvo-identify's recurrence step `Proposer`, which every host of the workflow
/// run puts first, and no line after it. A later run adds its lines here, in their order, a handler in front before
/// the handler behind it:
///
/// ```text
/// StepList { recurrence: Box::new(Proposer), lines: Vec::new() }
///     .step(family, writer_step)
///     .handler(family, takes, handler)
///     .delete_guard(family, guard)
/// ```
pub fn list() -> StepList {
    StepList {
        recurrence: Box::new(Proposer),
        lines: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::rc::Rc;

    use halvo_derived::{
        Clock, DbKey, Error, GuardAnswer, Handler, HandlerCx, IdSource, Store, Takes,
    };
    use halvo_schema_core::category::SeedCategory;
    use halvo_schema_core::ids::{Position, RequestId, StoreId, SummaryId};
    use halvo_schema_core::kinds;
    use halvo_schema_core::record::{
        Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Tagged, Text,
    };
    use halvo_schema_core::request::{
        self, Decoded, ErrorName, InterfaceDecision, InterfaceRead, InterfaceRole, RawRequest,
        ReplyResult,
    };
    use halvo_schema_core::site::AppId;
    use serde_json::value::RawValue;

    use super::list;

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
    #[test]
    fn the_host_is_built_in_the_order_of_the_list() {
        let scratch = Scratch::new("owner-steps");
        let seen = Rc::new(RefCell::new(Vec::new()));
        let asked = Rc::new(Cell::new(0));
        let guard_asked = Rc::clone(&asked);
        let host = list()
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
}
