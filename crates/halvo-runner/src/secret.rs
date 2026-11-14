use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::{Secret, SecretReply};
use halvo_schema::executable::names::{HandbackOutcome, HandbackReason};
use halvo_schema::executable::step::ApiTarget;
use halvo_schema::executable::text::{Name, SecretText};
use halvo_schema::executable::{Cause, FromRunner, Outcome, ToRunner};

use crate::link::Broken;
use crate::run::{Failure, Runner, Stop};

/// The end of a run whose token store could not be read (A8.6, CP-80).
fn unreadable() -> Stop {
    Stop::End(Outcome::Failed, Some(Cause::Secret))
}

impl Runner {
    /// An `api` `call`. Before it the runner reads the adapter's token and acts on the three answers of
    /// `secret_reply` (A8.6): with the secret it makes the call; with `none` it asks the handback `account_link`
    /// once, with the base address of the bindings as its origin, and reads the secret that the result names,
    /// whose `none` ends the run `handed_back`;
    /// with `unreadable` it makes no call, asks nothing and ends the run `failed` with the cause `secret`. This
    /// build links no adapter, so the call itself fails with the cause `adapter`.
    pub(crate) fn api_call(
        &mut self,
        target: &ApiTarget,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let token = self.token(target, loaded)?;
        drop(token);
        Err(Failure::Step(Cause::Adapter))
    }

    fn token(&mut self, target: &ApiTarget, loaded: &LoadReply) -> Result<SecretText, Stop> {
        let name = Name::parse(target.adapter.as_str()).map_err(|_| unreadable())?;
        match self.secret(&name)? {
            SecretReply::Stored(value) => return Ok(value),
            SecretReply::Unreadable(_) => return Err(unreadable()),
            SecretReply::None => {}
        }
        let origin = loaded
            .bindings
            .as_ref()
            .and_then(|b| b.origins.first().cloned());
        let result = self.handback(HandbackReason::AccountLink, origin)?;
        match result.outcome {
            HandbackOutcome::Declined => Err(Stop::End(Outcome::Declined, Some(Cause::Declined))),
            HandbackOutcome::Timeout => Err(Stop::End(Outcome::HandedBack, Some(Cause::Timeout))),
            HandbackOutcome::Done => match self.secret(result.secret.as_ref().unwrap_or(&name))? {
                SecretReply::Stored(value) => Ok(value),
                SecretReply::Unreadable(_) => Err(unreadable()),
                SecretReply::None => Err(Stop::End(Outcome::HandedBack, None)),
            },
        }
    }

    /// Writes the frame `secret` for `name` and waits for its `secret_reply`.
    fn secret(&mut self, name: &Name) -> Result<SecretReply, Stop> {
        match self.link.ask(&FromRunner::Secret(Secret {
            secret: name.clone(),
        }))? {
            ToRunner::SecretReply(reply) => Ok(reply),
            _ => Err(Broken::Frame("not_a_secret_reply").into()),
        }
    }
}
