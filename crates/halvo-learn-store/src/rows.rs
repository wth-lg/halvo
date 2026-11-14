use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_schema::executable::step::Locator;

use halvo_schema::learning::record::{EditId, EditState, LearnEdit, LearnRoutine, LearnRun};
use halvo_schema::learning::Refusal;
use halvo_schema::{AgentId, ErrorName, RowRead, TypedRecord, WorkflowId};

use crate::convert;

pub(crate) const LEARN_RUN: &str = "learn_run";
pub(crate) const LEARN_EDIT: &str = "learn_edit";
pub(crate) const LEARN_ROUTINE: &str = "learn_routine";

fn parsed<T>(
    rows: &dyn RowRead,
    kind: &str,
    parse: fn(&[u8]) -> Result<T, Refusal>,
) -> Result<Vec<T>, ErrorName> {
    let mut out = Vec::new();
    let mut unread = false;
    rows.for_each(kind, &mut |row| match parse(row.as_bytes()) {
        Ok(value) => out.push(value),
        Err(_) => unread = true,
    });
    if unread {
        return Err(ErrorName::Store);
    }
    Ok(out)
}

pub(crate) fn runs(rows: &dyn RowRead) -> Result<Vec<LearnRun>, ErrorName> {
    parsed(rows, LEARN_RUN, LearnRun::parse)
}

pub(crate) fn routines(rows: &dyn RowRead) -> Result<Vec<LearnRoutine>, ErrorName> {
    parsed(rows, LEARN_ROUTINE, LearnRoutine::parse)
}

pub(crate) fn run_record(row: &LearnRun) -> Result<TypedRecord, ErrorName> {
    Ok(TypedRecord {
        kind: LEARN_RUN,
        json: row.to_json().map_err(|_| ErrorName::Internal)?,
    })
}

pub(crate) fn edit_record(row: &LearnEdit) -> Result<TypedRecord, ErrorName> {
    Ok(TypedRecord {
        kind: LEARN_EDIT,
        json: row.to_json().map_err(|_| ErrorName::Internal)?,
    })
}

pub(crate) fn routine_record(row: &LearnRoutine) -> Result<TypedRecord, ErrorName> {
    Ok(TypedRecord {
        kind: LEARN_ROUTINE,
        json: row.to_json().map_err(|_| ErrorName::Internal)?,
    })
}

/// The newest row of the run, the one with the higher `rev`.
pub(crate) fn newest_run<'a>(runs: &'a [LearnRun], run: &str) -> Option<&'a LearnRun> {
    runs.iter()
        .filter(|r| r.run.as_str() == run)
        .max_by_key(|r| r.rev())
}

/// The newest `learn_routine` row of the workflow and agent (contracts.md L1.1, the coordinator's ruling LM-56 (f)):
/// the row of the highest `rev`, of two rows of one `rev` the one of the later `day`, and of two rows of one `rev`
/// and one `day` the one that stands later in `routines`, which holds the rows in the order in which the read
/// visited them. The step, `list_refinements` and the writes at a `start_run` take the newest row from it alone.
pub(crate) fn newest_routine<'a>(
    routines: &'a [LearnRoutine],
    workflow: &WorkflowId,
    agent: &AgentId,
) -> Option<&'a LearnRoutine> {
    routines
        .iter()
        .filter(|r| &r.workflow == workflow && &r.agent == agent)
        .max_by_key(|r| (r.rev, r.day))
}

/// The workflow that a row of `kind` names, read from its text by the registry's entry of the kind and never by the
/// kind's typed parser, for a row that the typed parser refused (contracts.md L4.5, L4.2; the coordinator's ruling
/// LM-58 (d), (e)): the value under the entry's `workflow_key` at the top level of the one JSON object that the text
/// holds. It gives none where the kind has no entry or no workflow key, the text is not one JSON object, the key
/// does not stand exactly once at its top level, or the key's value is not a string of the form of a workflow id;
/// a caller stops every workflow then.
pub(crate) fn refused_workflow(kind: &str, json: &str) -> Option<WorkflowId> {
    let key = halvo_schema::kinds::registry()
        .find(|entry| entry.name == kind)?
        .workflow_key?;
    let mut reader = serde_json::Deserializer::from_str(json);
    let text = serde::de::DeserializeSeed::deserialize(TopLevelKey(key), &mut reader).ok()??;
    reader.end().ok()?;
    WorkflowId::parse(&text).ok()
}

struct TopLevelKey(&'static str);

impl<'de> serde::de::DeserializeSeed<'de> for TopLevelKey {
    type Value = Option<String>;

    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Option<String>, D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> serde::de::Visitor<'de> for TopLevelKey {
    type Value = Option<String>;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("one JSON object")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut map: A,
    ) -> Result<Option<String>, A::Error> {
        let mut found: Vec<serde_json::Value> = Vec::new();
        while let Some(name) = map.next_key::<String>()? {
            if name == self.0 {
                found.push(map.next_value()?);
            } else {
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
        Ok(match found.as_slice() {
            [serde_json::Value::String(text)] => Some(text.clone()),
            _ => None,
        })
    }
}

/// Every row of the family's kind `learn_edit`, by edit, each edit's rows in the order of `rev`.
#[derive(Clone)]
pub(crate) struct Edits {
    by_id: BTreeMap<EditId, Vec<LearnEdit>>,
}

impl Edits {
    pub(crate) fn read(rows: &dyn RowRead) -> Result<Edits, ErrorName> {
        let mut by_id: BTreeMap<EditId, Vec<LearnEdit>> = BTreeMap::new();
        for row in parsed(rows, LEARN_EDIT, LearnEdit::parse)? {
            by_id.entry(row.edit.clone()).or_default().push(row);
        }
        for revisions in by_id.values_mut() {
            revisions.sort_by_key(|r| r.rev);
        }
        Ok(Edits { by_id })
    }

    pub(crate) fn contains(&self, id: &EditId) -> bool {
        self.by_id.contains_key(id)
    }

    /// Adds a revision that the commit being built stores, so that the reads after it see it.
    pub(crate) fn apply(&mut self, row: LearnEdit) {
        let revisions = self.by_id.entry(row.edit.clone()).or_default();
        revisions.push(row);
        revisions.sort_by_key(|r| r.rev);
    }

    pub(crate) fn newest(&self, id: &EditId) -> Option<&LearnEdit> {
        self.by_id.get(id).and_then(|revisions| revisions.last())
    }

    /// The newest row of every edit.
    pub(crate) fn all(&self) -> impl Iterator<Item = &LearnEdit> {
        self.by_id.values().filter_map(|revisions| revisions.last())
    }

    pub(crate) fn of_agent(&self, agent: &AgentId) -> impl Iterator<Item = &LearnEdit> + '_ {
        let agent = agent.clone();
        self.all().filter(move |e| e.agent == agent)
    }

    /// The newest row of every edit of the agent for this compile number and step.
    pub(crate) fn of_step(
        &self,
        agent: &AgentId,
        base: NonZeroU32,
        n: NonZeroU32,
    ) -> impl Iterator<Item = &LearnEdit> + '_ {
        self.of_agent(agent)
            .filter(move |e| e.base == base && e.n == n)
    }

    /// The edits of the step in any state (contracts.md L3.2, the cause `full`).
    pub(crate) fn count(&self, agent: &AgentId, base: NonZeroU32, n: NonZeroU32) -> u32 {
        u32::try_from(self.of_step(agent, base, n).count()).unwrap_or(u32::MAX)
    }

    pub(crate) fn provisional(
        &self,
        agent: &AgentId,
        base: NonZeroU32,
        n: NonZeroU32,
    ) -> Option<&LearnEdit> {
        self.of_step(agent, base, n)
            .find(|e| e.state == EditState::Provisional)
    }

    /// The newest `active` edit of the step: the one that no other `active` edit of the step names as `on`,
    /// since an edit is built on the `active` edit in force when it is stored and only one edit of a step is
    /// `provisional` at a time.
    pub(crate) fn newest_active(
        &self,
        agent: &AgentId,
        base: NonZeroU32,
        n: NonZeroU32,
    ) -> Option<&LearnEdit> {
        let active: Vec<&LearnEdit> = self
            .of_step(agent, base, n)
            .filter(|e| e.state == EditState::Active)
            .collect();
        active
            .iter()
            .copied()
            .filter(|e| {
                !active
                    .iter()
                    .any(|other| other.on.as_ref() == Some(&e.edit))
            })
            .max_by(|a, b| (a.last, &a.edit).cmp(&(b.last, &b.edit)))
    }

    /// The edit in force on the step (contracts.md L3.3): the `provisional` one, else the newest `active` one.
    pub(crate) fn in_force(
        &self,
        agent: &AgentId,
        base: NonZeroU32,
        n: NonZeroU32,
    ) -> Option<&LearnEdit> {
        self.provisional(agent, base, n)
            .or_else(|| self.newest_active(agent, base, n))
    }

    /// The locators of the edit's list.
    pub(crate) fn list(&self, id: &EditId) -> Vec<Locator> {
        self.newest(id)
            .map(|e| e.target.locators.clone())
            .unwrap_or_default()
    }

    /// What the edit brought: its list less the list of the edit that it names as `on`.
    pub(crate) fn brought(&self, edit: &LearnEdit) -> Vec<Locator> {
        let on = edit.on.as_ref().map(|id| self.list(id)).unwrap_or_default();
        convert::schema_list(&halvo_learn::brought(
            &convert::learn_list(&edit.target.locators),
            &convert::learn_list(&on),
        ))
    }

    /// The barred set of the step: the union of what its `reverted` edits brought.
    pub(crate) fn barred(&self, agent: &AgentId, base: NonZeroU32, n: NonZeroU32) -> Vec<Locator> {
        let mut out: Vec<Locator> = Vec::new();
        for edit in self.of_step(agent, base, n) {
            if matches!(edit.state, EditState::Reverted(_)) {
                for locator in self.brought(edit) {
                    if !out.contains(&locator) {
                        out.push(locator);
                    }
                }
            }
        }
        out
    }
}
