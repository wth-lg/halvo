use std::cell::Cell;
use std::fmt;

use halvo_schema_core::record::{TypedRecord, WriterStep};
use halvo_schema_core::request::{ErrorName, RawRequest};
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::value::RawValue;

use crate::db::{Db, Now};
use crate::error::{Error, Fault};
use crate::typed::{typed_commit, Rows, Writer};
use crate::Store;

/// The handler of a request type that the library does not know: a `runner` request, a `coordinator_write` or a
/// request of a further role. It is code of the run that owns the body; it decodes the body with that run's types
/// and answers with the serialized reply of the body's own reply type, which the owner writes with the reply writer
/// of the schema, or with an error name.
pub trait Handler {
    /// Answers one request. `cx` offers the five calls of the store and, to a handler in front, `pass_on`.
    fn handle(
        &self,
        request: &RawRequest,
        cx: &mut HandlerCx<'_>,
    ) -> Result<Box<RawValue>, ErrorName>;
}

/// Which requests a handler takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Takes {
    /// Every `runner` request.
    Runner,
    /// Every `coordinator_write` whose body's `op` is one of these names.
    Coordinator(&'static [&'static str]),
    /// Every request of the further role of this name.
    Role(&'static str),
}

/// What a typed commit stored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Committed {
    /// Records stored.
    pub stored: u64,
    /// Records dropped and not refused: a window record whose expiry has passed or that names a purged ordinal, and,
    /// in a step's commit alone, a bounded record whose day lies after the clock's day or outside its bound. A
    /// handler's commit refuses the whole list for such a bounded record instead (`Cause::Day`), so in a handler's
    /// commit `dropped` counts window records alone.
    pub dropped: u64,
}

/// Why a typed commit refused its list. Its text names the rule and never a value of a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    /// The kind is not among the entries that the store was opened with.
    UnknownKind,
    /// The kind is one of the four kinds of the workflow run, which enter only through `commit_batch`, the recurrence
    /// step and the decisions.
    WorkflowRun,
    /// The kind belongs to another family than the step's or the handler's own.
    OtherFamily,
    /// The kind is of origin own, and the commit is a step's or that of the handler of a request of a further role.
    OwnOrigin,
    /// `kinds::check_record` refused the record; its error names the kind, the key and the rule.
    Record(String),
    /// The record's workflow key names a workflow that is not stored.
    NoWorkflow,
    /// The record is of a bounded kind and the store will not keep its day: the day lies after the clock's day, or
    /// already outside its kind's bound. Only a handler's typed commit refuses for it.
    Day(DayFault),
    /// A fault of the store.
    Fault(Fault),
}

/// Which day of a bounded record a handler's typed commit refuses. It holds no day and no other value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayFault {
    /// The day lies after the clock's day.
    AfterToday,
    /// The day lies already outside the bound of its kind: the clock's day less the kind's days, or earlier.
    OutsideBound,
}

/// The refusal of a typed commit: nothing of the list was stored. `index` is the place of the refused record in the
/// list, or `usize::MAX` for a fault of the store that is no record's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub index: usize,
    pub cause: Cause,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cause {
            Cause::UnknownKind => write!(f, "record {}: the kind is not registered", self.index),
            Cause::WorkflowRun => write!(f, "record {}: a kind of the workflow run", self.index),
            Cause::OtherFamily => write!(f, "record {}: a kind of another family", self.index),
            Cause::OwnOrigin => write!(f, "record {}: a kind of origin own", self.index),
            Cause::Record(error) => write!(f, "record {}: {error}", self.index),
            Cause::NoWorkflow => write!(f, "record {}: no stored workflow", self.index),
            Cause::Day(DayFault::AfterToday) => {
                write!(
                    f,
                    "record {}: a bounded day after the clock's day",
                    self.index
                )
            }
            Cause::Day(DayFault::OutsideBound) => {
                write!(f, "record {}: a bounded day outside its bound", self.index)
            }
            Cause::Fault(fault) => write!(f, "record {}: {fault}", self.index),
        }
    }
}

impl std::error::Error for Refusal {}

pub(crate) struct StepEntry {
    pub(crate) family: &'static str,
    pub(crate) step: Box<dyn WriterStep>,
}

pub(crate) struct HandlerEntry {
    pub(crate) family: &'static str,
    pub(crate) takes: Takes,
    pub(crate) handler: Box<dyn Handler>,
}

impl crate::Host {
    /// Adds a step after the recurrence step and after the steps added before it. `family` is the step's own family:
    /// it reads the rows of that family alone and commits records of its kinds alone.
    pub fn step(mut self, family: &'static str, step: Box<dyn WriterStep>) -> crate::Host {
        self.steps.push(StepEntry { family, step });
        self
    }

    /// Adds a handler of the requests that `takes` names, behind the handlers of those requests added before it.
    /// `family` is the handler's own family. The handler added first for a request is the one in front: it gets the
    /// request first and reaches the next one by `HandlerCx::pass_on`; the handler of the owning run is added last.
    pub fn handler(
        mut self,
        family: &'static str,
        takes: Takes,
        handler: Box<dyn Handler>,
    ) -> crate::Host {
        self.handlers.push(HandlerEntry {
            family,
            takes,
            handler,
        });
        self
    }
}

/// What the store offers a handler while it answers one request: the typed commit, the state call, the held-ahead
/// call, the read of the rows of the handler's own family and the read of one stored workflow, and to a handler in
/// front the call that passes the request on. Each call keeps the handler's own family, the handler behind as well.
pub struct HandlerCx<'a> {
    pub(crate) db: &'a Db,
    pub(crate) request: &'a RawRequest,
    pub(crate) writer: Writer,
    chain: &'a [&'a HandlerEntry],
    at: usize,
    fault: &'a Cell<Option<Fault>>,
    /// The one reading of the host's clock that `Store::handle` took at its start, by which every read of this
    /// request reads, for the handler in front and for the handlers behind it.
    pub(crate) now: Now,
}

impl HandlerCx<'_> {
    pub(crate) fn family(&self) -> &'static str {
        self.chain[self.at].family
    }

    /// The typed commit for this request, of records of kinds of the handler's own family. A record of a kind of
    /// origin own is stored for a `runner` request and a `coordinator_write`, and refused for a request of a further
    /// role. See `Cause` for every refusal.
    pub fn commit(&mut self, records: &[TypedRecord]) -> Result<Committed, Refusal> {
        self.db.arm();
        let committed = typed_commit(self.db, self.family(), self.writer, records)?;
        Ok(committed)
    }

    /// The read of the rows of the handler's own family, as they stand: of one kind whole by `RowRead::for_each`, or
    /// of one kind under one workflow id by `Rows::for_each_under`. It hands out exactly the rows that an expiry at
    /// the clock would leave (DS-4), by the one reading of the host's clock that `Store::handle` took at the start of
    /// this request, so that every read of one request reads by one clock. No read writes.
    pub fn rows(&self) -> Rows<'_> {
        Rows {
            db: self.db,
            family: self.family(),
            fault: self.fault,
            now: self.now,
        }
    }

    /// Passes the request, unchanged, to the next handler that takes it and returns that handler's answer, after
    /// which the handler in front may change the reply where its own contract says so and makes its own typed
    /// commits. Where no handler stands behind, it answers `unsupported`.
    pub fn pass_on(&mut self) -> Result<Box<RawValue>, ErrorName> {
        let next = self.at + 1;
        let Some(entry) = self.chain.get(next) else {
            return Err(ErrorName::Unsupported);
        };
        let mut behind = HandlerCx {
            db: self.db,
            request: self.request,
            writer: self.writer,
            chain: self.chain,
            at: next,
            fault: self.fault,
            now: self.now,
        };
        entry.handler.handle(self.request, &mut behind)
    }
}

struct Op(Option<String>);

impl<'de> de::Deserialize<'de> for Op {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Op;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Op, A::Error> {
                let mut op = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "op" {
                        if op.is_some() {
                            return Err(de::Error::custom("repeated_key"));
                        }
                        op = Some(map.next_value::<String>()?);
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(Op(op))
            }
        }
        deserializer.deserialize_map(V)
    }
}

/// The `op` of a coordinator body: the string under the key `op` of a JSON object, and nothing else of the body.
fn read_op(body: &RawValue) -> Option<String> {
    serde_json::from_str::<Op>(body.get()).ok()?.0
}

impl Store {
    /// Answers a request that `request::decode` returned undecoded, after the owner has checked its role, by the
    /// handlers of the host: a `runner` request goes to the handlers that take `runner`; a `coordinator_write`, where
    /// the host supplies a handler of `coordinator_write`, goes to the handlers whose names hold the `op` of its body,
    /// which the store reads from the raw body and nothing else of it, answering `malformed` to a body that is no JSON
    /// object with a string under the key `op`; a request of a further role goes to the handlers of that role. With no
    /// handler it answers `unsupported`. The first handler in the host's order gets the request; a fault of the store
    /// while it reads rows is answered as the fault.
    pub fn handle(&mut self, request: &RawRequest) -> Result<Box<RawValue>, Error> {
        self.db.arm();
        let now = self.db.now();
        let handlers = &self.handlers;
        let (writer, chain): (Writer, Vec<&HandlerEntry>) = match request.name() {
            "runner" => (
                Writer::Runner,
                handlers
                    .iter()
                    .filter(|h| h.takes == Takes::Runner)
                    .collect(),
            ),
            "coordinator_write" => {
                if !handlers
                    .iter()
                    .any(|h| matches!(h.takes, Takes::Coordinator(_)))
                {
                    return Err(Error::Refused(ErrorName::Unsupported));
                }
                let op = read_op(request.body()).ok_or(Error::Refused(ErrorName::Malformed))?;
                (
                    Writer::Coordinator,
                    handlers
                        .iter()
                        .filter(|h| matches!(h.takes, Takes::Coordinator(ops) if ops.contains(&op.as_str())))
                        .collect(),
                )
            }
            name => (
                Writer::Role,
                handlers
                    .iter()
                    .filter(|h| matches!(h.takes, Takes::Role(role) if role == name))
                    .collect(),
            ),
        };
        let Some(first) = chain.first() else {
            return Err(Error::Refused(ErrorName::Unsupported));
        };
        let fault = Cell::new(None);
        let mut cx = HandlerCx {
            db: &self.db,
            request,
            writer,
            chain: &chain,
            at: 0,
            fault: &fault,
            now,
        };
        let answer = first.handler.handle(request, &mut cx);
        if let Some(fault) = fault.get() {
            return Err(Error::Fault(fault));
        }
        answer.map_err(Error::Refused)
    }
}
