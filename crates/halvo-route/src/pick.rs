//! The pick of contract T3: the one function from a policy table, a cell and an agent's ceiling to
//! a tier, a hand back with its cause, or no model.

use crate::policy::{Basis, PickCell, Policy, Row};
use crate::tier::{CallKind, CategoryId, Level, Tier};

/// Why a pick hands the step back to the person.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cause {
    LineMissed,
    Untested,
    NoRuntime,
    NotSeed,
    Ceiling,
}

impl Cause {
    pub const ALL: [Cause; 5] = [
        Cause::LineMissed,
        Cause::Untested,
        Cause::NoRuntime,
        Cause::NotSeed,
        Cause::Ceiling,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Cause::LineMissed => "line_missed",
            Cause::Untested => "untested",
            Cause::NoRuntime => "no_runtime",
            Cause::NotSeed => "not_seed",
            Cause::Ceiling => "ceiling",
        }
    }
}

/// What serves a call: no model, a tier at a level, or the person.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pick {
    NoModel,
    Tier { tier: Tier, level: Level },
    HandBack(Cause),
}

impl Pick {
    /// The word of the pick: `none`, the tier, or `hand_back`.
    pub const fn cell(&self) -> PickCell {
        match self {
            Pick::NoModel => PickCell::None,
            Pick::Tier { tier, .. } => PickCell::Tier(*tier),
            Pick::HandBack(_) => PickCell::HandBack,
        }
    }
}

/// The first rule that fits: the kind `replay` gives `NoModel`; a row whose pick is `hand_back`
/// gives `HandBack` with the cause of its basis; a row whose tier is above `ceiling` gives
/// `HandBack(Ceiling)`; else the row's tier at the row's level. Reads nothing but its arguments.
pub const fn pick(policy: &Policy, category: CategoryId, kind: CallKind, ceiling: Tier) -> Pick {
    if matches!(kind, CallKind::Replay) {
        return Pick::NoModel;
    }
    let row = policy.row(category, kind);
    match row.pick {
        PickCell::HandBack => Pick::HandBack(cause(row.basis)),
        PickCell::Tier(tier) if tier as u8 > ceiling as u8 => Pick::HandBack(Cause::Ceiling),
        PickCell::Tier(tier) => match row.level {
            Some(level) => Pick::Tier { tier, level },
            None => Pick::HandBack(cause(row.basis)),
        },
        PickCell::None => Pick::HandBack(cause(row.basis)),
    }
}

/// The row behind the pick of a cell, whose `basis`, `k` and `n` a caller logs beside the pick.
pub const fn explain(policy: &Policy, category: CategoryId, kind: CallKind) -> &Row {
    policy.row(category, kind)
}

const fn cause(basis: Basis) -> Cause {
    match basis {
        Basis::LineMissed => Cause::LineMissed,
        Basis::NoRuntime => Cause::NoRuntime,
        Basis::NotSeed => Cause::NotSeed,
        Basis::Untested | Basis::NoModel | Basis::LineMet => Cause::Untested,
    }
}
