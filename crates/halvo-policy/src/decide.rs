use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{ActionClass, AllowedClass, Autonomy, Caps};

/// The decision of one row of the table (A5.1): `run`, `confirm`, `handback` or `refuse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Decision {
    Run,
    Confirm,
    Handback,
    Refuse,
}

impl Decision {
    /// The four decisions in the order of the contract.
    pub const ALL: [Decision; 4] = [
        Decision::Run,
        Decision::Confirm,
        Decision::Handback,
        Decision::Refuse,
    ];

    /// The name as the table writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Run => "run",
            Decision::Confirm => "confirm",
            Decision::Handback => "handback",
            Decision::Refuse => "refuse",
        }
    }
}

impl serde::Serialize for Decision {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Whether a restorable copy of the file exists (A11.3): `yes` or `no`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Checkpoint {
    Yes,
    No,
}

/// The row of the table for these arguments (A5.1); the first condition that fits gives the decision.
/// `caps.external` changes no row.
pub fn decide_row(
    autonomy: Autonomy,
    class: ActionClass,
    allow: &[AllowedClass],
    caps: &Caps,
    checkpoint: Checkpoint,
) -> Decision {
    if class == ActionClass::Credential {
        return Decision::Handback;
    }
    if autonomy == Autonomy::Observe && class != ActionClass::Read {
        return Decision::Refuse;
    }
    if matches!(class, ActionClass::Read | ActionClass::Prepare) {
        return Decision::Run;
    }
    if class == ActionClass::LocalEdit {
        return match checkpoint {
            Checkpoint::Yes => Decision::Run,
            Checkpoint::No => Decision::Refuse,
        };
    }
    let allowed = allow.iter().any(|a| ActionClass::from(*a) == class);
    let split = caps.untrusted == YesNo::No || caps.private == YesNo::No;
    if allowed && autonomy == Autonomy::Unattended && split {
        return Decision::Run;
    }
    Decision::Confirm
}
