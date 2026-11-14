use std::num::NonZeroU32;

use halvo_policy::check_playbook;
use halvo_schema::executable::body::{LoadReply, LoadSteps, LoadedPlaybook};
use halvo_schema::executable::names::Runtime;
use halvo_schema::executable::{Cause, ExecutableStep, Outcome, Playbook, RunnerCall, RunnerReply};

use crate::link::Broken;
use crate::run::{cause_of, Runner, Stop};

const FILL: &[u8] = include_bytes!("../playbooks/fill.jsonl");
const REPLY: &[u8] = include_bytes!("../playbooks/reply.jsonl");
const CLEANUP: &[u8] = include_bytes!("../playbooks/cleanup.jsonl");

/// The built-in playbook of a runtime that is not `web` (A11.4): `fill` for `file`, `reply` for `graph_mail` and
/// `cleanup` for `graph_calendar`; none for `web` and `none`.
fn built_in(runtime: Runtime) -> Option<&'static [u8]> {
    match runtime {
        Runtime::File => Some(FILL),
        Runtime::GraphMail => Some(REPLY),
        Runtime::GraphCalendar => Some(CLEANUP),
        Runtime::Web | Runtime::None => None,
    }
}

impl Runner {
    /// The steps that the run replays: the built-in playbook of the agent's runtime, or the stored playbook of a
    /// `web` agent; each checked by `check_playbook` for the agent's category. An agent whose runtime is `none`
    /// has nothing to do.
    pub(crate) fn playbook(&mut self, loaded: &LoadReply) -> Result<Vec<ExecutableStep>, Stop> {
        let config = &loaded.config;
        let steps = match (built_in(config.runtime), config.runtime, &loaded.playbook) {
            (Some(file), _, _) => Playbook::parse_file(file)
                .map_err(|_| Stop::End(Outcome::Failed, Some(Cause::StepFailed)))?
                .steps()
                .to_vec(),
            (None, Runtime::None, _) => return Err(Stop::End(Outcome::NothingToDo, None)),
            (None, _, None) => return Err(Stop::End(Outcome::Failed, Some(Cause::StepFailed))),
            (None, _, Some(stored)) => {
                self.playbook = Some(stored.number);
                self.stored_steps(stored)?
            }
        };
        check_playbook(config.category, &steps)
            .map_err(|differs| Stop::End(Outcome::Failed, Some(differs.cause())))?;
        Ok(steps)
    }

    /// The steps of the stored playbook, in pages of at most 16 from the store.
    fn stored_steps(&mut self, stored: &LoadedPlaybook) -> Result<Vec<ExecutableStep>, Stop> {
        let count = stored.steps.get() as usize;
        let mut steps: Vec<ExecutableStep> = Vec::with_capacity(count);
        while steps.len() < count {
            let from = NonZeroU32::new(steps.len() as u32 + 1).unwrap_or(NonZeroU32::MIN);
            let call = RunnerCall::LoadSteps(LoadSteps {
                agent: self.args.agent.clone(),
                run: self.args.run.clone(),
                number: stored.number,
                from,
            });
            match self.store(call)? {
                Ok(RunnerReply::Steps(page)) => {
                    if page.steps.first().map(|s| s.n) != Some(from) {
                        return Err(Broken::Frame("steps_not_from").into());
                    }
                    steps.extend(page.steps);
                }
                Ok(_) => return Err(Broken::Frame("not_the_answer").into()),
                Err(error) => return Err(Stop::End(Outcome::Failed, Some(cause_of(error)))),
            }
        }
        if steps.len() != count {
            return Err(Broken::Frame("steps_count").into());
        }
        Playbook::new(steps)
            .map(|playbook| playbook.steps().to_vec())
            .map_err(|_| Stop::End(Outcome::Failed, Some(Cause::StepFailed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_runtime_but_web_and_none_has_its_built_in_file() {
        assert_eq!(built_in(Runtime::File), Some(FILL));
        assert_eq!(built_in(Runtime::GraphMail), Some(REPLY));
        assert_eq!(built_in(Runtime::GraphCalendar), Some(CLEANUP));
        assert_eq!(built_in(Runtime::Web), None);
        assert_eq!(built_in(Runtime::None), None);
        for file in [FILL, REPLY, CLEANUP] {
            assert!(Playbook::parse_file(file).is_ok());
        }
    }
}
