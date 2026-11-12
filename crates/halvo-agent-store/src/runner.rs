use halvo_schema::executable::{RunnerCall, RunnerReply};
use halvo_schema::ErrorName;

use crate::errors::refused;
use crate::host::{Host, Now};

/// Answers one runner body (A7.1).
pub fn runner(_host: &mut dyn Host, _now: Now, body: &[u8]) -> Result<RunnerReply, ErrorName> {
    match RunnerCall::parse(body).map_err(refused)? {
        RunnerCall::Load(_)
        | RunnerCall::LoadSteps(_)
        | RunnerCall::LoadSession(_)
        | RunnerCall::PutPlaybook(_)
        | RunnerCall::PutSession(_)
        | RunnerCall::Effect(_)
        | RunnerCall::Log(_)
        | RunnerCall::Finish(_) => Err(ErrorName::Unsupported),
    }
}
