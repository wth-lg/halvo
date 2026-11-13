use std::num::NonZeroU32;

use halvo_learn::{
    admit, build_list, is_rejected, new_locators, value_hit, AdmitCause, AdmitFacts, RunMode,
};
use halvo_policy::check_playbook;
use halvo_schema::executable::body::{PutPlaybook, Stored};
use halvo_schema::executable::names::{Mode, StepKind};
use halvo_schema::executable::step::Target;
use halvo_schema::executable::{ExecutableStep, RunnerReply};
use halvo_schema::learning::record::LearnEdit;
use halvo_schema::ErrorName;

use crate::agent;
use crate::convert;
use crate::facts::{Candidate, RunFacts};
use crate::handlers::{ClassRule, LearnHandlers, Refused};
use crate::host::LearnHost;
use crate::moves::withdrawals;
use crate::rows::{self, Edits};
use crate::serve::{fits, with_target};
use crate::sources;

fn binding() -> ErrorName {
    ErrorName::read("binding").unwrap_or(ErrorName::Internal)
}

fn web(step: &ExecutableStep) -> Option<&halvo_schema::executable::step::WebTarget> {
    match &step.target {
        Some(Target::Web(target)) if step.kind == StepKind::Web => Some(target),
        _ => None,
    }
}

/// The proposed step equals the served step in every key but `target.desc` and `target.locators`.
fn same_but_target(proposed: &ExecutableStep, served: &ExecutableStep) -> bool {
    let mut copy = proposed.clone();
    if let (Some(Target::Web(p)), Some(Target::Web(s))) = (&mut copy.target, &served.target) {
        p.desc = s.desc.clone();
        p.locators = s.locators.clone();
    }
    &copy == served
}

/// The answer of `admit` for each step of a body that differs from its served step.
pub(crate) type Answers = Vec<(NonZeroU32, Result<(), AdmitCause>)>;

struct Facts<'a> {
    mode: RunMode,
    facts: &'a RunFacts,
    edits: &'a Edits,
    current: bool,
    compiled: &'a [ExecutableStep],
    class_rule: ClassRule,
}

impl Facts<'_> {
    fn admit(
        &self,
        proposed: &ExecutableStep,
        number: NonZeroU32,
    ) -> Result<Candidate, AdmitCause> {
        let n = proposed.n;
        let served = self.facts.served.get(&n).map(|s| &s.step);
        let compiled = self.compiled.iter().find(|s| s.n == n);
        let reference = served.or(compiled).unwrap_or(proposed);
        let proposed_target = web(proposed);
        let served_locators = web(reference)
            .map(|t| t.locators.clone())
            .unwrap_or_default();
        let proposed_locators = proposed_target
            .map(|t| t.locators.clone())
            .unwrap_or_default();
        let new = new_locators(
            &convert::learn_list(&proposed_locators),
            &convert::learn_list(&served_locators),
        );
        let base_list = self
            .edits
            .newest_active(&self.facts.agent, number, n)
            .map(|e| e.target.locators.clone())
            .unwrap_or_default();
        let list = convert::schema_list(&build_list(&new, &convert::learn_list(&base_list)));
        let fits = match (compiled, proposed_target) {
            (Some(step), Some(target)) => {
                with_target(step, &target.desc, &list).is_some_and(|s| fits(&s))
            }
            _ => false,
        };
        let mut playbook: Vec<ExecutableStep> = self.compiled.to_vec();
        for step in &mut playbook {
            if step.n == n {
                *step = proposed.clone();
            }
        }
        let value_target = halvo_learn::Target {
            desc: proposed_target
                .map(|t| t.desc.as_str().to_owned())
                .unwrap_or_default(),
            locators: convert::learn_list(&proposed_locators),
        };
        let barred = self.edits.barred(&self.facts.agent, number, n);
        let facts = AdmitFacts {
            mode: self.mode,
            has_base: self.current && served.is_some() && compiled.is_some(),
            same_but_target: same_but_target(proposed, reference),
            kind_web: proposed_target.is_some() && web(reference).is_some(),
            class_same: (self.class_rule)(self.facts.category, proposed) == reference.class,
            playbook_ok: !playbook.is_empty()
                && check_playbook(self.facts.category, &playbook).is_ok(),
            new_count: new.len(),
            value_hit: value_hit(&value_target, &self.facts.slots),
            fits,
            rejected: is_rejected(&new, &convert::learn_list(&barred)),
            edits_for_step: self.edits.count(&self.facts.agent, number, n),
        };
        admit(&facts)?;
        let desc = proposed_target
            .map(|t| t.desc.clone())
            .ok_or(AdmitCause::Scope)?;
        Ok(Candidate {
            desc,
            locators: convert::schema_list(&new),
        })
    }
}

impl LearnHandlers {
    pub(crate) fn put_playbook(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
        b: &PutPlaybook,
    ) -> Result<RunnerReply, Refused> {
        let runs = rows::runs(host.rows())?;
        let row = rows::newest_run(&runs, b.run.as_str())
            .filter(|r| r.agent == b.agent)
            .ok_or_else(binding)?;
        match row.mode {
            Mode::Compile => {
                let reply = agent::runner(host, body)?;
                if let RunnerReply::Stored(_) = reply {
                    let mut edits = Edits::read(host.rows())?;
                    let rows = withdrawals(&mut edits, &b.agent, b.number)?;
                    commit_edits(host, &rows)?;
                }
                Ok(reply)
            }
            Mode::Replay => {
                let taken = self
                    .propose(host, b)?
                    .iter()
                    .filter(|(_, answer)| answer.is_ok())
                    .count();
                Ok(RunnerReply::Stored(Stored {
                    stored: u32::try_from(taken).unwrap_or(u32::MAX),
                }))
            }
        }
    }

    /// `admit` for each step of the body that differs from the step served to the run on its `n` (contracts.md
    /// L3.2, L4.2); an admitted step becomes the run's candidate on its `n`, in place of an earlier one.
    pub(crate) fn propose(
        &mut self,
        host: &mut dyn LearnHost,
        b: &PutPlaybook,
    ) -> Result<Answers, ErrorName> {
        let changed: Vec<&ExecutableStep> = b
            .steps
            .iter()
            .filter(|step| {
                self.facts
                    .get(&b.run)
                    .and_then(|f| f.served.get(&step.n))
                    .is_none_or(|served| &served.step != *step)
            })
            .collect();
        let refused = |changed: &[&ExecutableStep]| -> Answers {
            changed
                .iter()
                .map(|s| (s.n, Err(AdmitCause::Base)))
                .collect()
        };
        let Some(loaded) = self
            .facts
            .get(&b.run)
            .filter(|f| f.agent == b.agent)
            .map(|f| f.loaded)
        else {
            return Ok(refused(&changed));
        };
        let newest = self.newest_of(host, &b.agent, &b.run)?;
        let Some(facts) = self.facts.get(&b.run) else {
            return Ok(refused(&changed));
        };
        let runs = rows::runs(host.rows())?;
        let mode = convert::run_mode(rows::newest_run(&runs, b.run.as_str()).map(|r| r.mode));
        let named = loaded.is_some_and(|l| b.number == l || l.checked_add(1) == Some(b.number));
        let current = newest.is_some() && loaded == newest && named;
        let (true, Some(number)) = (current, loaded) else {
            return Ok(refused(&changed));
        };
        let compiled = agent::compiled(host, &b.agent, &b.run, number)?;
        let edits = Edits::read(host.rows())?;
        let reader = Facts {
            mode,
            facts,
            edits: &edits,
            current,
            compiled: &compiled,
            class_rule: self.class_rule,
        };
        let mut answers = Vec::new();
        let mut taken = Vec::new();
        for step in changed {
            match reader.admit(step, number) {
                Ok(candidate) => {
                    taken.push((step.n, candidate));
                    answers.push((step.n, Ok(())));
                }
                Err(cause) => answers.push((step.n, Err(cause))),
            }
        }
        if let Some(facts) = self.facts.get_mut(&b.run) {
            facts.candidates.extend(taken);
        }
        Ok(answers)
    }
}

/// One typed commit of `learn_edit` rows that add no run, each checked by `check_sources`; none for no row.
pub(crate) fn commit_edits(host: &mut dyn LearnHost, rows: &[LearnEdit]) -> Result<(), Refused> {
    if rows.is_empty() {
        return Ok(());
    }
    let stored = Edits::read(host.rows())?;
    let mut records = Vec::new();
    for row in rows {
        if !sources::kept(row, stored.newest(&row.edit), None) {
            return Err(Refused::Source);
        }
        records.push(rows::edit_record(row)?);
    }
    Ok(host.commit(records)?)
}
