//! The hosted model of a run: a compile's (r3 U7.2a) and the writer's of a reply's body (r3 U7.4, crate::model::write)
//! (contracts A6.1, A8.6, A12.3, A12.6). Before a compile of an agent whose tier is a hosted tier does anything, the runner checks that the person allowed it: the start's `--hosted`, which
//! the supervisor library writes from the app's one function of the agreement, names the agent's tier; where it does
//! not, the run ends `handed_back` with `no_agreement`, asks the supervisor nothing, starts no door and sends
//! nothing. The writer of a reply's body makes the same check by the same function (`agreed`) in a run of the mode
//! replay, and there a missing agreement gives the const text of a reply and no hand-back (crate::model::write); a run
//! of an agent at `on_device` makes no check.
//!
//! The one way to the model is `ask` (crate::model::ask): a hold of the request's cost in the person's count
//! (`model_hold`), refused at the person's cap with `over_cap` before the door is started or a line is written to it;
//! at the first call the door's start, which asks for the person's key (the frame `secret` for the reserved name
//! `model_key`) in the one place of the runner that asks for it and then starts the door with the key as the first
//! line of its standard input; one request line out and one line in; and the settle of the hold (`model_settle`).
//! The door is the program `halvo-model` beside the runner's own executable, and the run ends it before the run's
//! end is written (`end_door`). The writer of a reply's body (crate::model::write, r3 U7.4) and the compile's drive
//! (crate::compile, r3 U7.2b) call `ask` with the router's pick for their kind of call (crate::route's `picked`),
//! only where that pick names the run's tier; the writer never calls it for a message whose draft of an earlier run
//! waits. While every `compile` and every `write` row of the router's table (its file policy.tsv) hands back, no
//! shipped run reaches `ask`: no shipped run asks for the key or starts the door; the unit tests do.

pub(crate) mod ask;
pub(crate) mod snapshot;
#[cfg(test)]
mod tests;
pub(crate) mod texts;
pub(crate) mod write;

use halvo_schema::executable::config::AgentConfig;
use halvo_schema::executable::names::ModelTier;
use halvo_schema::executable::{Cause, Outcome};

use crate::run::{Runner, Stop};

/// What a run that the person allowed to ask a hosted model holds, a compile or the writer of a reply: its tier,
/// the door once `ask` has started it, and the id of its next request line.
pub(crate) struct Hosted {
    tier: ModelTier,
    door: Option<ask::Door>,
    next_line: u64,
}

/// The end of a run that may not ask its hosted model: `handed_back` with `cause`.
fn handed_back(cause: Cause) -> Stop {
    Stop::End(Outcome::HandedBack, Some(cause))
}

impl Runner {
    /// The check before a compile of an agent of `config` (r3 U7.2a): nothing for `on_device`; for a hosted tier, the
    /// start's `--hosted` equal to that tier, else the run ends `handed_back` with `no_agreement`, before any frame to
    /// the supervisor about the model. With the agreement the run holds its tier for `ask`; it asks for no key and
    /// starts no door here.
    pub(crate) fn hosted_model(&mut self, config: &AgentConfig) -> Result<(), Stop> {
        if config.tier == ModelTier::OnDevice {
            return Ok(());
        }
        if !self.agreed(config.tier) {
            return Err(handed_back(Cause::NoAgreement));
        }
        self.hosted = Some(Hosted {
            tier: config.tier,
            door: None,
            next_line: 1,
        });
        Ok(())
    }

    /// THE ONE RULE of the person's agreement to a hosted model (r3 U7.2a, r3 U7.4), which the check of a compile and
    /// the writer of a reply's body both call: `tier` is a hosted tier and the start's `--hosted` names it.
    pub(crate) fn agreed(&self, tier: ModelTier) -> bool {
        tier != ModelTier::OnDevice && self.args.hosted == Some(tier)
    }

    /// Ends the run's door where one was started: its standard input closed, then its exit awaited up to
    /// `ask::DOOR_END`, then the system's hard kill; it is reaped before this returns. One line on standard error
    /// says how it ended.
    pub(crate) fn end_door(&mut self) {
        if let Some(door) = self.hosted.as_mut().and_then(|hosted| hosted.door.take()) {
            let ended = door.end();
            crate::line(&format!(
                "run door=ended by={}",
                if ended { "input_end" } else { "kill" }
            ));
        }
    }
}
