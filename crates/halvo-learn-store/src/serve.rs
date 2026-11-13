use halvo_schema::executable::body::{Load, LoadSteps, StepsReply};
use halvo_schema::executable::step::Target;
use halvo_schema::executable::step::{Locator, STEP_JSON_MAX};
use halvo_schema::executable::{ExecutableStep, OwnText, RunnerReply};
use halvo_schema::learning::record::LearnEdit;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::agent;
use crate::facts::{slot_values, RunFacts, Served};
use crate::handlers::{LearnHandlers, Refused};
use crate::host::LearnHost;
use crate::rows::Edits;

/// The step with its target's description and locators replaced: the learned list, then the step's own locators
/// that the list does not hold. None for a step with no web target.
pub(crate) fn with_target(
    step: &ExecutableStep,
    desc: &OwnText,
    learned: &[Locator],
) -> Option<ExecutableStep> {
    let Some(Target::Web(target)) = &step.target else {
        return None;
    };
    let mut target = target.clone();
    let compiled = std::mem::take(&mut target.locators);
    target.desc = desc.clone();
    target.locators = learned.to_vec();
    for locator in compiled {
        if !target.locators.contains(&locator) {
            target.locators.push(locator);
        }
    }
    let mut out = step.clone();
    out.target = Some(Target::Web(target));
    Some(out)
}

/// The keys of a step in the order in which its writer writes them, with their values, so that the bytes are
/// counted without the bound that the writer itself holds.
struct StepForm<'a>(&'a ExecutableStep);

impl Serialize for StepForm<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let step = self.0;
        let mut map = serializer.serialize_map(Some(10))?;
        map.serialize_entry("v", &step.v)?;
        map.serialize_entry("n", &step.n)?;
        map.serialize_entry("kind", &step.kind)?;
        map.serialize_entry("verb", &step.verb)?;
        map.serialize_entry("target", &step.target)?;
        map.serialize_entry("args", &step.args)?;
        map.serialize_entry("pre", &step.pre)?;
        map.serialize_entry("post", &step.post)?;
        map.serialize_entry("effect", &step.effect)?;
        map.serialize_entry("class", &step.class)?;
        map.end()
    }
}

/// The bytes of the step's compact JSON as its writer writes them, or none where a part of it breaks a rule.
pub(crate) fn written(step: &ExecutableStep) -> Option<usize> {
    serde_json::to_vec(&StepForm(step))
        .ok()
        .map(|json| json.len())
}

/// The compact JSON of the step has at most 2,048 bytes (AG-A3.4).
pub(crate) fn fits(step: &ExecutableStep) -> bool {
    written(step).is_some_and(|len| len <= STEP_JSON_MAX)
}

fn lay(step: &ExecutableStep, edit: &LearnEdit) -> Option<ExecutableStep> {
    with_target(step, &edit.target.desc, &edit.target.locators).filter(fits)
}

impl LearnHandlers {
    pub(crate) fn load(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
        b: &Load,
    ) -> Result<RunnerReply, Refused> {
        let reply = agent::runner(host, body)?;
        let RunnerReply::Load(loaded) = &reply else {
            return Ok(reply);
        };
        let Some(mode) = self
            .starts
            .get(&b.run)
            .filter(|start| start.agent == b.agent)
            .map(|start| start.mode)
        else {
            return Ok(reply);
        };
        match self.facts.get_mut(&b.run) {
            Some(facts) => {
                for value in slot_values(loaded, mode) {
                    if !facts.slots.contains(&value) {
                        facts.slots.push(value);
                    }
                }
            }
            None => {
                let facts = RunFacts::new(&b.agent, loaded, mode);
                self.facts.insert(b.run.clone(), facts);
            }
        }
        Ok(reply)
    }

    pub(crate) fn load_steps(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
        b: &LoadSteps,
    ) -> Result<RunnerReply, Refused> {
        let reply = agent::runner(host, body)?;
        let RunnerReply::Steps(StepsReply { mut steps }) = reply else {
            return Ok(reply);
        };
        let seen = self
            .facts
            .get(&b.run)
            .is_some_and(|f| f.agent == b.agent && f.loaded == Some(b.number));
        if !seen {
            return Ok(RunnerReply::Steps(StepsReply { steps }));
        }
        let newest = self.newest_of(host, &b.agent, &b.run)?;
        let edits = Edits::read(host.rows())?;
        let Some(facts) = self.facts.get_mut(&b.run) else {
            return Ok(RunnerReply::Steps(StepsReply { steps }));
        };
        for step in &mut steps {
            let mut carried = None;
            if newest == Some(b.number) {
                if let Some(edit) = edits.in_force(&b.agent, b.number, step.n) {
                    if let Some(laid) = lay(step, edit) {
                        *step = laid;
                        carried = Some(edit.edit.clone());
                    }
                }
            }
            let served = Served {
                step: step.clone(),
                edit: carried,
            };
            facts.served.insert(step.n, served);
        }
        Ok(RunnerReply::Steps(StepsReply { steps }))
    }
}
