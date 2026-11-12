use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{ActionClass, AgentConfig, AllowedClass, Autonomy, Caps};

/// The decision that the table gives for an agent's configuration and a class (A5.1), as `decide` returns it and
/// `propose` takes it. Only `decide` makes one: the fields are private and the type has no public constructor, so
/// a plain `Decision` cannot become a `Decided`. It is used once: it is neither `Clone` nor `Copy`, and
/// `effect_step` takes the move that holds it by value, so one call of `decide` gives at most one proposed effect.
/// What must not compile, a `propose` built from a decision of its own:
///
/// ```compile_fail
/// use halvo_policy::{Decided, Decision, EffectMove};
/// use halvo_schema::executable::ActionClass;
/// let own = Decided { class: ActionClass::Payment, decision: Decision::Run };
/// let _ = EffectMove::Propose(own);
/// ```
///
/// Nor through the constructor of the crate's own test build:
///
/// ```compile_fail
/// use halvo_policy::{Decided, Decision, EffectMove};
/// use halvo_schema::executable::ActionClass;
/// let own = Decided::free(ActionClass::Payment, Decision::Run);
/// let _ = EffectMove::Propose(own);
/// ```
///
/// Nor by a default value:
///
/// ```compile_fail
/// use halvo_policy::{Decided, EffectMove};
/// let own = Decided::default();
/// let _ = EffectMove::Propose(own);
/// ```
///
/// Nor by changing the decision of a `Decided` that `decide` returned:
///
/// ```compile_fail
/// use halvo_policy::{decide, preset, Checkpoint, Decision, EffectMove};
/// use halvo_schema::executable::ActionClass;
/// use halvo_schema::{AgentId, Category, WorkflowId};
/// let config = preset(Category::Email, WorkflowId::from_random([1; 16]), AgentId::from_random([2; 16])).unwrap();
/// let mut own = decide(&config, ActionClass::Send, Checkpoint::No);
/// own.decision = Decision::Run;
/// let _ = EffectMove::Propose(own);
/// ```
///
/// Nor by changing its class:
///
/// ```compile_fail
/// use halvo_policy::{decide, preset, Checkpoint, EffectMove};
/// use halvo_schema::executable::ActionClass;
/// use halvo_schema::{AgentId, Category, WorkflowId};
/// let config = preset(Category::Email, WorkflowId::from_random([1; 16]), AgentId::from_random([2; 16])).unwrap();
/// let mut own = decide(&config, ActionClass::Read, Checkpoint::No);
/// own.class = ActionClass::Send;
/// let _ = EffectMove::Propose(own);
/// ```
///
/// Nor two proposes from one `Decided` by a copy:
///
/// ```compile_fail
/// use halvo_policy::{decide, preset, Checkpoint, EffectMove};
/// use halvo_schema::executable::ActionClass;
/// use halvo_schema::{AgentId, Category, WorkflowId};
/// let config = preset(Category::Email, WorkflowId::from_random([1; 16]), AgentId::from_random([2; 16])).unwrap();
/// let own = decide(&config, ActionClass::Send, Checkpoint::No);
/// let first = EffectMove::Propose(own);
/// let second = EffectMove::Propose(own);
/// ```
///
/// Nor by a clone:
///
/// ```compile_fail
/// use halvo_policy::{decide, preset, Checkpoint, EffectMove};
/// use halvo_schema::executable::ActionClass;
/// use halvo_schema::{AgentId, Category, WorkflowId};
/// let config = preset(Category::Email, WorkflowId::from_random([1; 16]), AgentId::from_random([2; 16])).unwrap();
/// let own = decide(&config, ActionClass::Send, Checkpoint::No);
/// let second = EffectMove::Propose(own.clone());
/// let first = EffectMove::Propose(own);
/// ```
///
/// Nor by lending one move to `effect_step` twice, since it takes the move by value:
///
/// ```compile_fail
/// use halvo_policy::{decide, effect_step, preset, Checkpoint, EffectMove};
/// use halvo_schema::executable::ActionClass;
/// use halvo_schema::{AgentId, Category, WorkflowId};
/// let config = preset(Category::Email, WorkflowId::from_random([1; 16]), AgentId::from_random([2; 16])).unwrap();
/// let first = EffectMove::Propose(decide(&config, ActionClass::Send, Checkpoint::No));
/// let _ = effect_step(None, &first);
/// let _ = effect_step(None, &first);
/// ```
///
/// Their twin, which takes the `Decided` from `decide`, reads its class and decision, and proposes once:
///
/// ```
/// use halvo_policy::{decide, effect_step, preset, Checkpoint, Decided, Decision, EffectMove};
/// use halvo_schema::executable::{ActionClass, EffectState};
/// use halvo_schema::{AgentId, Category, WorkflowId};
/// let config = preset(Category::Email, WorkflowId::from_random([1; 16]), AgentId::from_random([2; 16])).unwrap();
/// let own: Decided = decide(&config, ActionClass::Send, Checkpoint::No);
/// assert_eq!(own.decision(), Decision::Confirm);
/// assert_eq!(own.class(), ActionClass::Send);
/// assert_eq!(effect_step(None, EffectMove::Propose(own)), Ok(EffectState::Held));
/// ```
#[derive(Debug, PartialEq, Eq)]
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
