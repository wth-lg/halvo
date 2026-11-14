use halvo_schema::executable::ActionClass;

/// The four causes of the move `not_made` (A5.2, A7.4), held here until U1.8 (b) gives the schema's `Cause` these
/// names and `effect_step` the move and the state; (b) removes this module (the coordinator's ruling CP-144).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotMadeCause {
    NotSent,
    NotReplaced,
    Unauthorized,
    Throttled,
}

impl NotMadeCause {
    pub(crate) const ALL: [NotMadeCause; 4] = [
        NotMadeCause::NotSent,
        NotMadeCause::NotReplaced,
        NotMadeCause::Unauthorized,
        NotMadeCause::Throttled,
    ];

    /// The name that the model and the record write.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            NotMadeCause::NotSent => "not_sent",
            NotMadeCause::NotReplaced => "not_replaced",
            NotMadeCause::Unauthorized => "unauthorized",
            NotMadeCause::Throttled => "throttled",
        }
    }

    /// The cause of a name, or none for a name that is not one of the four.
    pub(crate) fn parse(name: &str) -> Option<NotMadeCause> {
        NotMadeCause::ALL.into_iter().find(|c| c.as_str() == name)
    }

    /// Whether the plain words of the cause can be true for an effect of the class: `not_replaced` for a local
    /// edit alone, `not_sent` for every class but a local edit, `unauthorized` and `throttled` for a send and a
    /// delete alone (the coordinator's ruling CP-144 (4)).
    pub(crate) fn fits(self, class: ActionClass) -> bool {
        match self {
            NotMadeCause::NotReplaced => class == ActionClass::LocalEdit,
            NotMadeCause::NotSent => class != ActionClass::LocalEdit,
            NotMadeCause::Unauthorized | NotMadeCause::Throttled => {
                matches!(class, ActionClass::Send | ActionClass::Delete)
            }
        }
    }
}

/// Whether a test that follows the model's traces gives a move to `effect_step`: never a `not_made` move, and no
/// move of a trace after its first accepted `not_made`, because `effect_step` has neither the move nor the state
/// until U1.8 (b) (the coordinator's ruling CP-144 (1)).
pub(crate) fn followed(kind: &str, after_not_made: bool) -> bool {
    kind != "not_made" && !after_not_made
}
