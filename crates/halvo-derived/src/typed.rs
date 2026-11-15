use std::cell::Cell;
use std::collections::BTreeMap;

use halvo_schema_core::ids::WorkflowId;
use halvo_schema_core::kinds::{self, KindEntry, Origin, RetentionClass, KINDS};
use halvo_schema_core::record::{RowRead, TypedRecord};
use serde_json::value::RawValue;

use crate::db::{one, reading, Db, NewRow, Now};
use crate::error::{db_fault, Fault};
use crate::handler::{Cause, Committed, Refusal};
use crate::sql;

/// For which call the store runs a typed commit. The store sets it itself and takes it from no argument of a step
/// or a handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Writer {
    /// A step after a committed batch, the one way by which an analysis write leads to a typed commit.
    Step,
    /// A handler answering a `runner` request.
    Runner,
    /// A handler answering a `coordinator_write`.
    Coordinator,
    /// A handler answering a request of a further role.
    Role,
}

impl Writer {
    pub(crate) fn may_write_own(self) -> bool {
        matches!(self, Writer::Runner | Writer::Coordinator)
    }

    /// A handler's commit refuses a bounded record whose day the store will not keep; a step's drops and counts it,
    /// as `commit_batch` does an episode, so that one late record does not hold back the rest.
    pub(crate) fn refuses_day(self) -> bool {
        !matches!(self, Writer::Step)
    }
}

/// The values under the rule keys of a record, which the store reads for itself.
struct RuleKeys {
    workflow: Option<WorkflowId>,
    day: Option<u32>,
    expires: Option<i64>,
    segments: Vec<u64>,
}

fn rule_keys(entry: &KindEntry, json: &str) -> Option<RuleKeys> {
    let fields: BTreeMap<String, Box<RawValue>> = serde_json::from_str(json).ok()?;
    let value = |key: &str| fields.get(key).map(|raw| raw.get());
    let workflow = match entry.workflow_key {
        Some(key) => {
            let text: String = serde_json::from_str(value(key)?).ok()?;
            Some(WorkflowId::parse(&text).ok()?)
        }
        None => None,
    };
    let (day, expires, segments) = match entry.class {
        RetentionClass::Bounded { .. } => (
            Some(serde_json::from_str(value("day")?).ok()?),
            None,
            Vec::new(),
        ),
        RetentionClass::Window => (
            None,
            Some(serde_json::from_str(value("expires")?).ok()?),
            serde_json::from_str(value("segments")?).ok()?,
        ),
        RetentionClass::Kept => (None, None, Vec::new()),
    };
    Some(RuleKeys {
        workflow,
        day,
        expires,
        segments,
    })
}

/// The typed commit: a list of records of registered kinds, applied in one transaction or not at all.
///
/// It refuses the whole list when a record names a kind that the store was not opened with (`UnknownKind`), one of
/// the four kinds of the workflow run (`WorkflowRun`), a kind of another family than the caller's own
/// (`OtherFamily`), or a kind of origin own while the call is a step's or that of a handler of a further role
/// (`OwnOrigin`); when `kinds::check_record` refuses a record (`Record`, with that function's error, which names the
/// kind, the key and the rule and never a value); and when a record's workflow key names a workflow that is not
/// stored (`NoWorkflow`). In the commit of a handler (a `runner` request, a `coordinator_write` or a request of a
/// further role) it also refuses the whole list for a record of a bounded kind whose day lies after the clock's day
/// or already outside its kind's bound (`Day`, with `DayFault::AfterToday` or `DayFault::OutsideBound`); the store
/// sets and mends no day. In the commit of a step such a record is dropped and counted, and the rest of the list is
/// applied, as is a record of a window kind whose expiry has passed or whose segments name a purged ordinal in the
/// commit of every writer.
pub(crate) fn typed_commit(
    db: &Db,
    family: &str,
    writer: Writer,
    records: &[TypedRecord],
) -> Result<Committed, Refusal> {
    let now = db.now();
    db.write(|tx| {
        let mark = tx.mark()?;
        let mut out = Committed::default();
        for (index, record) in records.iter().enumerate() {
            let refuse = |cause: Cause| Refusal { index, cause };
            let entry: &'static KindEntry = db
                .entries
                .iter()
                .copied()
                .find(|entry| entry.name == record.kind)
                .ok_or_else(|| refuse(Cause::UnknownKind))?;
            if KINDS.contains(entry) || entry.family == "workflow" {
                return Err(refuse(Cause::WorkflowRun));
            }
            if entry.family != family {
                return Err(refuse(Cause::OtherFamily));
            }
            if entry.origin == Origin::Own && !writer.may_write_own() {
                return Err(refuse(Cause::OwnOrigin));
            }
            kinds::check_record(entry, &record.json).map_err(|e| refuse(Cause::Record(e)))?;
            let keys = rule_keys(entry, &record.json).ok_or_else(|| {
                refuse(Cause::Record(format!(
                    "{}: the rule keys: unreadable",
                    entry.name
                )))
            })?;
            if let Some(workflow) = &keys.workflow {
                let stored = one(tx.conn, sql::WORKFLOW_ROW, [workflow.as_str()], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(|fault| refuse(Cause::Fault(fault)))?;
                if stored.is_none() {
                    return Err(refuse(Cause::NoWorkflow));
                }
            }
            let keeps = match entry.class {
                RetentionClass::Bounded { days } => {
                    let day = keys.day.ok_or_else(|| {
                        refuse(Cause::Record(format!(
                            "{}: the rule keys: unreadable",
                            entry.name
                        )))
                    })?;
                    match now.day_fault(days, day) {
                        None => true,
                        Some(fault) if writer.refuses_day() => {
                            return Err(refuse(Cause::Day(fault)))
                        }
                        Some(_) => false,
                    }
                }
                RetentionClass::Window => {
                    keys.expires
                        .is_some_and(|expires| now.keeps_expiry(expires))
                        && !tx
                            .names_purged(mark, &keys.segments)
                            .map_err(|fault| refuse(Cause::Fault(fault)))?
                }
                RetentionClass::Kept => true,
            };
            if !keeps {
                out.dropped += 1;
                continue;
            }
            tx.insert(&NewRow {
                entry,
                day: keys.day,
                expires: keys.expires,
                workflow: keys.workflow.as_ref().map(|w| w.as_str()),
                summary: None,
                wkey: None,
                body: &record.json,
                segments: &keys.segments,
            })
            .map_err(|fault| refuse(Cause::Fault(fault)))?;
            out.stored += 1;
        }
        Ok(out)
    })
}

impl From<Fault> for Refusal {
    fn from(fault: Fault) -> Self {
        Refusal {
            index: usize::MAX,
            cause: Cause::Fault(fault),
        }
    }
}

/// The read of the rows of one family, as they stand: of them it hands out exactly those that an expiry at the clock
/// would leave (DS-4), a bounded row while its day of going lies after the clock's day and its day at most two days
/// after it, a window row while its expiry lies after the clock's instant, a kept row always. The clock is the one
/// reading of the host's clock that the call of the host took at its start, `commit_batch` for a step and
/// `Store::handle` for a handler; no read takes a day or an instant from its caller. It reads one registered kind of
/// the family whole, by `RowRead::for_each`, or under one workflow id, by `Rows::for_each_under`, and visits nothing
/// for a kind of the family `workflow`. A fault of the database is kept for the call that handed out the read, which
/// answers it.
pub struct Rows<'a> {
    pub(crate) db: &'a Db,
    pub(crate) family: &'a str,
    pub(crate) fault: &'a Cell<Option<Fault>>,
    pub(crate) now: Now,
}

impl Rows<'_> {
    fn read(
        &self,
        kind: &str,
        workflow: Option<&WorkflowId>,
        visit: &mut dyn FnMut(&str),
    ) -> Result<(), Fault> {
        let own = self.db.entries.iter().any(|entry| {
            entry.name == kind && entry.family == self.family && entry.family != "workflow"
        });
        if !own {
            return Ok(());
        }
        let statement = match workflow {
            None => sql::ROWS_READ,
            Some(_) => sql::ROWS_READ_UNDER,
        };
        let mut statement = self
            .db
            .conn
            .prepare_cached(statement.text())
            .map_err(db_fault)?;
        let mut found = match workflow {
            None => statement.query(reading!(self.now, ":kind" => kind)),
            Some(id) => {
                statement.query(reading!(self.now, ":kind" => kind, ":workflow" => id.as_str()))
            }
        }
        .map_err(db_fault)?;
        while let Some(row) = found.next().map_err(db_fault)? {
            let body: String = row.get(0).map_err(db_fault)?;
            visit(&body);
        }
        Ok(())
    }

    /// Visits the serialized rows of one registered kind of the holder's own family that name `workflow`, each once,
    /// in ascending byte order of the serialized rows and under the rule of `RowRead::for_each`, so that it hands out
    /// exactly the rows of the read of the whole kind that name `workflow` (C2.11; the interface run's ruling RD-64,
    /// part 1.3 (a)). A row names a workflow by the workflow column that the commit set from the committed text under
    /// the workflow key of the kind's entry, and the read parses no row: a row whose column is empty is not visited,
    /// as a row committed while its entry gave no workflow key, until an open with an entry that gives one fills the
    /// column. It visits nothing for a kind that is not registered, that belongs to another family or that belongs
    /// to the family `workflow`.
    pub fn for_each_under(&self, kind: &str, workflow: &WorkflowId, visit: &mut dyn FnMut(&str)) {
        if let Err(fault) = self.read(kind, Some(workflow), visit) {
            self.fault.set(Some(fault));
        }
    }
}

impl RowRead for Rows<'_> {
    fn for_each(&self, kind: &str, visit: &mut dyn FnMut(&str)) {
        if let Err(fault) = self.read(kind, None, visit) {
            self.fault.set(Some(fault));
        }
    }
}
