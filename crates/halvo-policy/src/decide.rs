use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{ActionClass, AgentConfig, AllowedClass, Autonomy, Caps};

/// The decision that the table gives for an agent's configuration and a class (A5.1), as `decide` returns it and
/// `propose` takes it. Only `decide` makes one: the fields are private and the type has no public constructor, so
/// a plain `Decision` cannot become a `Decided`. What must not compile, a `propose` built from a decision of its
/// own:
///
/// ```compile_fail
/// use halvo_policy::{Decided, Decision, EffectMove};
/// use halvo_schema::executable::ActionClass;
/// let own = Decided { class: ActionClass::Payment, decision: Decision::Run };
/// let _ = EffectMove::Propose(own);
/// ```
///
/// Its twin, which takes the `Decided` from `decide` and compiles:
///
/// ```
/// use halvo_policy::{decide, Checkpoint, Decided, Decision, EffectMove};
/// use halvo_schema::executable::{ActionClass, AgentConfig};
/// let config = AgentConfig::parse(br#"{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{"untrusted":"yes","private":"yes","external":"yes"},"limits":{"steps":20,"wall_s":120,"model_tokens":0},"budget":null,"permissions":[],"runtime":"web","playbook":null}"#).unwrap();
/// let own: Decided = decide(&config, ActionClass::Payment, Checkpoint::No);
/// assert_eq!(own.decision(), Decision::Confirm);
/// let _ = EffectMove::Propose(own);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decided {
    class: ActionClass,
    decision: Decision,
}

impl Decided {
    /// The class that was decided.
    pub fn class(&self) -> ActionClass {
        self.class
    }

    /// The decision that `decide_row` gave for the configuration and the class.
    pub fn decision(&self) -> Decision {
        self.decision
    }

    /// A `Decided` with a free decision, for the tests that follow the model's traces, whose moves take the
    /// decision as a free value. It exists in the crate's own test build alone.
    #[cfg(test)]
    pub(crate) fn free(class: ActionClass, decision: Decision) -> Decided {
        Decided { class, decision }
    }
}

/// The `Decided` for `config`, `class` and `checkpoint`: the class and the decision that `decide_row` gives for
/// the configuration's `autonomy`, `allow` and `caps`.
pub fn decide(config: &AgentConfig, class: ActionClass, checkpoint: Checkpoint) -> Decided {
    Decided {
        class,
        decision: decide_row(
            config.autonomy,
            class,
            &config.allow,
            &config.caps,
            checkpoint,
        ),
    }
}

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
