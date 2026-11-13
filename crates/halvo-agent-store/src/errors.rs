use halvo_schema::executable::{Refusal, RefusalCause};
use halvo_schema::ErrorName;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Own {
    Binding,
    Playbook,
    Config,
    Class,
    Length,
    Effect,
}

impl Own {
    fn as_str(self) -> &'static str {
        match self {
            Own::Binding => "binding",
            Own::Playbook => "playbook",
            Own::Config => "config",
            Own::Class => "class",
            Own::Length => "length",
            Own::Effect => "effect",
        }
    }
}

impl From<Own> for ErrorName {
    fn from(own: Own) -> ErrorName {
        ErrorName::read(own.as_str()).unwrap_or(ErrorName::Internal)
    }
}

pub(crate) fn refused(refusal: Refusal) -> ErrorName {
    match refusal.cause() {
        RefusalCause::Malformed => ErrorName::Malformed,
        RefusalCause::Length => Own::Length.into(),
    }
}
