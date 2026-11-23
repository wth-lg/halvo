use std::cell::Cell;
use std::fmt;

use halvo_schema_core::ids::WorkflowId;
use halvo_schema_core::kinds::{Origin, KINDS};
use halvo_schema_core::record::{TypedRecord, WriterStep};
use halvo_schema_core::request::{ErrorName, RawRequest};
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::value::RawValue;

use crate::db::{Db, Now, Removes};
use crate::error::{Error, Fault};
use crate::sql;
use crate::typed::{store_records, typed_commit, Rows, Writer};
use crate::Store;

/// The handler of a request type that the library does not know: a `runner` request, a `coordinator_write` or a
/// request of a further role. It is code of the run that owns the body; it decodes the body with that run's types
/// and answers with the serialized reply of the body's own reply type, which the owner writes with the reply writer
/// of the schema, or with an error name.
pub trait Handler {
    /// Answers one request. `cx` offers the six calls of the store and, to a handler in front, `pass_on`.
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
    /// The kind is of origin own, and the commit is a step's or that of the handler of a request of a further role; or
    /// the kind is of origin person, and the commit is no `coordinator_write`'s.
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
            Cause::OwnOrigin => write!(f, "record {}: a kind of origin own or person", self.index),
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

/// What the removal call (`HandlerCx::remove_under`) did in its one write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Removed {
    /// The rows of the kind under the workflow id that it removed.
    pub removed: u64,
    /// What the typed commit of its list stored and dropped in the same write.
    pub committed: Committed,
}

/// Why a removal call (`HandlerCx::remove_under` or `HandlerCx::remove_person`) answered no `Removed`. Every refusal
/// but `Commit` comes before any statement, and `Commit` for a refused list or a fault before the commit: the call
/// removed no row and stored no record. `Commit` with a fault of the checkpoint or of the host's report after the
/// commit comes with the rows removed and the records stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoveRefusal {
    /// The request is no `coordinator_write`. The call refuses it before any statement.
    Role,
    /// The kind is not among the entries that the store was opened with.
    UnknownKind,
    /// The kind is one of the four kinds of the workflow run.
    WorkflowRun,
    /// The kind belongs to another family than the handler's own.
    OtherFamily,
    /// The kind's entry gives no workflow key, so no row of it is held under a workflow id (`remove_under`).
    NoWorkflowKey,
    /// The kind is not of origin person: its entry gives a workflow key, or it is of another origin, and
    /// `remove_person` never takes its rows, so that it never takes a workflow's kind across workflows.
    WorkflowKey,
    /// The typed commit of the list refused it, or the store met a fault in the write: the refusal of the commit,
    /// whose `index` is `usize::MAX` for a fault that is no record's.
    Commit(Refusal),
}

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
/// call, the removal call, the read of the rows of the handler's own family and the read of one stored workflow, and
/// to a handler in front the call that passes the request on. Each call keeps the handler's own family, the handler
/// behind as well.
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

    /// The removal call: in one write it removes every stored row of the kind `kind` of the handler's own family that
    /// names the workflow id `workflow`, the rows that no read hands out any more among them, and then stores
    /// `records` by the rules of the typed commit (`commit`), which takes a reading of the clock of its own, as the
    /// typed commit does; the rows that it stores are not removed by it. It answers how many rows it removed and what
    /// the commit stored. The removal goes the way of the delete cascade: each row by the store's secure delete, in a
    /// transaction that the truncating checkpoint follows after its commit, so that no byte of a removed row stays in
    /// the files of the store. A refusal of the list or a fault before the commit leaves every row and stores no
    /// record; a fault of the checkpoint or of the host's report after the commit answers the fault with the rows
    /// removed and the records stored, as it does after a delete.
    ///
    /// It answers `RemoveRefusal::Role`, before any statement, while the handler answers a request that is no
    /// `coordinator_write`, so that no runner removes a row, and then a refusal of `RemoveRefusal` for a kind that the
    /// store was not opened with, a kind of the workflow run, a kind of another family and a kind whose entry gives no
    /// workflow key, in that order. It removes no single row and no row of another kind.
    pub fn remove_under(
        &mut self,
        kind: &str,
        workflow: &WorkflowId,
        records: &[TypedRecord],
    ) -> Result<Removed, RemoveRefusal> {
        self.db.arm();
        if self.writer != Writer::Coordinator {
            return Err(RemoveRefusal::Role);
        }
        let entry = self
            .db
            .entries
            .iter()
            .copied()
            .find(|entry| entry.name == kind)
            .ok_or(RemoveRefusal::UnknownKind)?;
        if KINDS.contains(entry) || entry.family == "workflow" {
            return Err(RemoveRefusal::WorkflowRun);
        }
        let family = self.family();
        if entry.family != family {
            return Err(RemoveRefusal::OtherFamily);
        }
        if entry.workflow_key.is_none() {
            return Err(RemoveRefusal::NoWorkflowKey);
        }
        let (db, writer, now) = (self.db, self.writer, self.db.now());
        db.write_checkpointed(|tx| {
            let removed = tx.remove(sql::REMOVE_OF_KIND_UNDER, [kind, workflow.as_str()])?;
            let committed = store_records(db, tx, now, family, writer, records)?;
            Ok(Removed { removed, committed })
        })
        .map_err(RemoveRefusal::Commit)
    }

    /// The removal call of a person's kind (r3 U7.6), the sibling of `remove_under` for a kind of origin person, whose
    /// rows name no workflow (`kinds::PERSON_KINDS`): in one write it removes every stored row of the kind `kind` of
    /// the handler's own family, the rows that no read hands out any more among them, and then stores `records` by the
    /// rules of the typed commit; the rows that it stores are not removed by it. It answers how many rows it removed
    /// and what the commit stored. The write is that of `remove_under`: each row by the store's secure delete, in a
    /// transaction that the truncating checkpoint follows after its commit, so that no byte of a removed row stays in
    /// the files of the store; a refusal of the list or a fault before the commit leaves every row and stores no
    /// record; a fault of the checkpoint or of the host's report after the commit answers the fault with the rows
    /// removed and the records stored.
    ///
    /// It answers `RemoveRefusal::Role`, before any statement, while the handler answers a request that is no
    /// `coordinator_write`; then a refusal of `RemoveRefusal` for a kind that the store was not opened with, a kind of
    /// the workflow run, a kind of another family and a kind that is not of origin person (`WorkflowKey`), in that
    /// order. It removes no row of another kind.
    pub fn remove_person(
        &mut self,
        kind: &str,
        records: &[TypedRecord],
    ) -> Result<Removed, RemoveRefusal> {
        self.db.arm();
        if self.writer != Writer::Coordinator {
            return Err(RemoveRefusal::Role);
        }
        let entry = self
            .db
            .entries
            .iter()
            .copied()
            .find(|entry| entry.name == kind)
            .ok_or(RemoveRefusal::UnknownKind)?;
        if KINDS.contains(entry) || entry.family == "workflow" {
            return Err(RemoveRefusal::WorkflowRun);
        }
        let family = self.family();
        if entry.family != family {
            return Err(RemoveRefusal::OtherFamily);
        }
        if entry.origin != Origin::Person || entry.workflow_key.is_some() {
            return Err(RemoveRefusal::WorkflowKey);
        }
        let (db, writer, now) = (self.db, self.writer, self.db.now());
        db.write_checkpointed(|tx| {
            let removed = tx.remove(sql::REMOVE_OF_KIND, [kind])?;
            let committed = store_records(db, tx, now, family, writer, records)?;
            Ok(Removed { removed, committed })
        })
        .map_err(RemoveRefusal::Commit)
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
