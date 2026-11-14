use halvo_learn as learn;
use halvo_schema::executable::names::{Cause, LocatorBy, Mode, Outcome};
use halvo_schema::executable::step::Locator;
use halvo_schema::executable::OwnText;
use halvo_schema::learning::names::{EditCause, RunCause};
use halvo_schema::learning::record::EditState;

pub(crate) fn learn_locator(l: &Locator) -> learn::Locator {
    learn::Locator {
        by: l.by.as_str().to_owned(),
        value: l.value.as_str().to_owned(),
        name: l.name.as_ref().map(|n| n.as_str().to_owned()),
    }
}

pub(crate) fn learn_list(list: &[Locator]) -> Vec<learn::Locator> {
    list.iter().map(learn_locator).collect()
}

fn schema_locator(l: &learn::Locator) -> Option<Locator> {
    Some(Locator {
        by: LocatorBy::parse(&l.by).ok()?,
        value: OwnText::parse(&l.value).ok()?,
        name: match &l.name {
            Some(name) => Some(OwnText::parse(name).ok()?),
            None => None,
        },
    })
}

/// The locators back in the schema's type; each came from it, so none is lost.
pub(crate) fn schema_list(list: &[learn::Locator]) -> Vec<Locator> {
    list.iter().filter_map(schema_locator).collect()
}

pub(crate) fn run_mode(mode: Option<Mode>) -> learn::RunMode {
    match mode {
        None => learn::RunMode::None,
        Some(Mode::Compile) => learn::RunMode::Compile,
        Some(Mode::Replay) => learn::RunMode::Replay,
    }
}

pub(crate) fn outcome(outcome: Outcome) -> learn::Outcome {
    match outcome {
        Outcome::Completed => learn::Outcome::Completed,
        Outcome::NothingToDo => learn::Outcome::NothingToDo,
        Outcome::Declined => learn::Outcome::Declined,
        Outcome::HandedBack => learn::Outcome::HandedBack,
        Outcome::ClassMismatch => learn::Outcome::ClassMismatch,
        Outcome::Refused => learn::Outcome::Refused,
        Outcome::Limit => learn::Outcome::Limit,
        Outcome::EffectUnknown => learn::Outcome::EffectUnknown,
        Outcome::Failed => learn::Outcome::Failed,
    }
}

/// The cause of a log line as halvo-learn names it. The four causes that the schema's list holds and halvo-learn's
/// does not, `secret`, `unauthorized`, `not_selected` and `no_start`, are no counted cause (contracts.md L3.5 counts
/// `step_failed`, `class_mismatch` and `origin` alone) and map to none (CP-188).
pub(crate) fn log_cause(cause: Cause) -> Option<learn::LogCause> {
    Some(match cause {
        Cause::Approval => learn::LogCause::Approval,
        Cause::Digest => learn::LogCause::Digest,
        Cause::Used => learn::LogCause::Used,
        Cause::Unknown => learn::LogCause::Unknown,
        Cause::Autonomy => learn::LogCause::Autonomy,
        Cause::Checkpoint => learn::LogCause::Checkpoint,
        Cause::Class => learn::LogCause::Class,
        Cause::ClassMismatch => learn::LogCause::ClassMismatch,
        Cause::Origin => learn::LogCause::Origin,
        Cause::Provenance => learn::LogCause::Provenance,
        Cause::Declined => learn::LogCause::Declined,
        Cause::Timeout => learn::LogCause::Timeout,
        Cause::SessionSize => learn::LogCause::SessionSize,
        Cause::StepFailed => learn::LogCause::StepFailed,
        Cause::Steps => learn::LogCause::Steps,
        Cause::Wall => learn::LogCause::Wall,
        Cause::Tokens => learn::LogCause::Tokens,
        Cause::Sheet => learn::LogCause::Sheet,
        Cause::Workbook => learn::LogCause::Workbook,
        Cause::Adapter => learn::LogCause::Adapter,
        Cause::Effect => learn::LogCause::Effect,
        Cause::Ended => learn::LogCause::Ended,
        Cause::Changed => learn::LogCause::Changed,
        Cause::Secret | Cause::Unauthorized | Cause::NotSelected | Cause::NoStart => return None,
    })
}

pub(crate) fn run_cause(cause: learn::RunCause) -> RunCause {
    match cause {
        learn::RunCause::NothingToDo => RunCause::NothingToDo,
        learn::RunCause::Declined => RunCause::Declined,
        learn::RunCause::HandedBack => RunCause::HandedBack,
        learn::RunCause::ClassMismatch => RunCause::ClassMismatch,
        learn::RunCause::Refused => RunCause::Refused,
        learn::RunCause::Limit => RunCause::Limit,
        learn::RunCause::EffectUnknown => RunCause::EffectUnknown,
        learn::RunCause::Failed => RunCause::Failed,
        learn::RunCause::Unknown => RunCause::Unknown,
    }
}

pub(crate) fn edit_cause(cause: learn::EditCause) -> EditCause {
    match cause {
        learn::EditCause::StepFailed => EditCause::StepFailed,
        learn::EditCause::ClassMismatch => EditCause::ClassMismatch,
        learn::EditCause::Origin => EditCause::Origin,
    }
}

pub(crate) fn learn_state(state: EditState) -> learn::EditState {
    match state {
        EditState::Provisional => learn::EditState::Provisional,
        EditState::Active => learn::EditState::Active,
        EditState::Reverted(_) => learn::EditState::Reverted,
        EditState::Withdrawn => learn::EditState::Withdrawn,
    }
}

/// The stored state for a state of halvo-learn, with the counted cause of a revert; none for `none`,
/// `candidate` and `refused`, which are never stored, and for `reverted` with no cause.
pub(crate) fn stored_state(
    state: learn::EditState,
    cause: Option<learn::EditCause>,
) -> Option<EditState> {
    match state {
        learn::EditState::Provisional => Some(EditState::Provisional),
        learn::EditState::Active => Some(EditState::Active),
        learn::EditState::Reverted => cause.map(|c| EditState::Reverted(edit_cause(c))),
        learn::EditState::Withdrawn => Some(EditState::Withdrawn),
        learn::EditState::None | learn::EditState::Candidate | learn::EditState::Refused => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CP-188: `unauthorized`, `not_selected` and `no_start` map to none, as `secret` does, and every other cause of
    /// the schema's list maps to a name of halvo-learn, so that a new cause of the schema fails this test and not only
    /// the build.
    #[test]
    fn the_causes_that_halvo_learn_does_not_hold_map_to_none_and_every_other_cause_to_a_name() {
        let none = [
            Cause::Secret,
            Cause::Unauthorized,
            Cause::NotSelected,
            Cause::NoStart,
        ];
        for cause in [Cause::Unauthorized, Cause::NotSelected, Cause::NoStart] {
            assert_eq!(log_cause(cause), None, "{} maps to none", cause.as_str());
        }
        for cause in Cause::ALL.iter().filter(|c| !none.contains(c)) {
            assert!(
                log_cause(*cause).is_some(),
                "{} maps to a name of halvo-learn",
                cause.as_str()
            );
        }
        assert_eq!(
            Cause::ALL
                .iter()
                .filter(|c| log_cause(**c).is_none())
                .count(),
            none.len(),
            "no other cause maps to none"
        );
    }
}
