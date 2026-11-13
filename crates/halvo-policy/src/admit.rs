use halvo_schema::executable::names::{Cause, FieldName, Source};
use halvo_schema::executable::{ActionClass, Bound, BoundField, WebOrigin};

/// What the runner knows from this run and not from a page's text (A9.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The origins of the agent's bindings.
    pub origins: Vec<WebOrigin>,
    /// The origin of the paused request as the executor reports it; none for an adapter call and a file edit.
    pub request_origin: Option<WebOrigin>,
    /// The `new_origin` of the confirmation; none in the mode `replay`.
    pub new_origin: Option<WebOrigin>,
    /// The sender address of each message that the run's own `mail_list` returned.
    pub senders: Vec<String>,
    /// The ids that the run's own list calls returned.
    pub ids: Vec<String>,
}

/// The refusal of `admit`, the cause `provenance`: the bound object is of another class than the one passed, or
/// the first field whose source, origin, value or size its row of A9.3 does not admit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    Class,
    Field(FieldName),
}

impl Provenance {
    /// `provenance`.
    pub fn cause(&self) -> Cause {
        Cause::Provenance
    }
}

/// `Ok` when every field of `bound` comes from where its row of A9.3 admits, within its size, else the refusal
/// with the first field that does not. The runner calls it before every `propose` and every `rebind`.
pub fn admit(class: ActionClass, bound: &Bound, facts: &Facts) -> Result<(), Provenance> {
    if bound.class() != class {
        return Err(Provenance::Class);
    }
    for field in bound.fields() {
        let admitted = rule(class, field.name).is_some_and(|rule| rule.admits(field, facts));
        if !admitted {
            return Err(Provenance::Field(field.name));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Rule {
    Slot,
    Extract,
    ExtractOfBindings { max: Option<usize> },
    Sender { max: usize },
    OneId,
    ModelOrConst { max: usize },
    ConstOrSlot,
    RequestOrigin,
}

const AMOUNT_MAX: usize = 32;
const ADDRESS_MAX: usize = 320;
const BODY_MAX: usize = 4096;

fn rule(class: ActionClass, name: FieldName) -> Option<Rule> {
    use ActionClass as C;
    use FieldName as F;
    let rule = match (class, name) {
        (C::Payment, F::Payee) => Rule::Slot,
        (C::Payment, F::Amount) | (C::Purchase, F::Amount) => Rule::ExtractOfBindings {
            max: Some(AMOUNT_MAX),
        },
        (C::Payment, F::Instrument) | (C::Purchase, F::Instrument) => Rule::Slot,
        (C::Payment, F::Origin) | (C::Purchase, F::Origin) | (C::Submit, F::Origin) => {
            Rule::RequestOrigin
        }
        (C::Purchase, F::Seller) => Rule::Slot,
        (C::Purchase, F::Items) => Rule::ExtractOfBindings { max: None },
        (C::Send, F::To) => Rule::Sender { max: ADDRESS_MAX },
        (C::Send, F::Subject) => Rule::Extract,
        (C::Send, F::Body) => Rule::ModelOrConst { max: BODY_MAX },
        (C::Delete, F::Items) => Rule::OneId,
        (C::Submit, F::Fields) => Rule::ConstOrSlot,
        (C::Sign, F::Document | F::Party) => Rule::Slot,
        (C::LocalEdit, F::File | F::Range) => Rule::Slot,
        _ => return None,
    };
    Some(rule)
}

impl Rule {
    fn admits(self, field: &BoundField, facts: &Facts) -> bool {
        let value = field.value.joined();
        match self {
            Rule::Slot => field.from == Source::Slot,
            Rule::Extract => field.from == Source::Extract,
            Rule::ExtractOfBindings { max } => {
                field.from == Source::Extract
                    && field
                        .origin
                        .as_ref()
                        .is_some_and(|o| facts.origins.contains(o))
                    && max.is_none_or(|max| value.len() <= max)
            }
            Rule::Sender { max } => {
                field.from == Source::Extract
                    && value.len() <= max
                    && facts.senders.contains(&value)
            }
            Rule::OneId => field.from == Source::Extract && facts.ids.contains(&value),
            Rule::ModelOrConst { max } => {
                matches!(field.from, Source::Model | Source::Const) && value.len() <= max
            }
            Rule::ConstOrSlot => matches!(field.from, Source::Const | Source::Slot),
            Rule::RequestOrigin => {
                field.from == Source::Const
                    && facts.request_origin.as_ref().is_some_and(|o| {
                        value == o.as_str()
                            && (facts.origins.contains(o) || facts.new_origin.as_ref() == Some(o))
                    })
            }
        }
    }
}
