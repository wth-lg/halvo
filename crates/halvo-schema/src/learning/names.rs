use serde::{Serialize, Serializer};
use serde_json::value::RawValue;

use super::json::{self, Refusal};

macro_rules! names {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $variant:ident = $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $variant, )+
        }

        impl $name {
            /// Every name of the closed list, in the order of the contract.
            pub const ALL: &'static [$name] = &[ $( $name::$variant, )+ ];

            /// The name as it is written.
            pub fn as_str(self) -> &'static str {
                match self {
                    $( $name::$variant => $text, )+
                }
            }

            /// The value whose name is `s`; any other string is refused with the rule `unknown_name`.
            pub fn parse(s: &str) -> Result<Self, Refusal> {
                match s {
                    $( $text => Ok($name::$variant), )+
                    _ => Err(Refusal::new(stringify!($name), "unknown_name")),
                }
            }

            pub(in crate::learning) fn from_raw(raw: &RawValue) -> Result<Self, Refusal> {
                $name::parse(&json::string(stringify!($name), raw)?)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    };
}

names! {
    /// The cause of an unverified run, the closed list of `learn_run`'s `cause` (contracts.md L3.4): the outcome
    /// names of the agent-setup run other than `completed`, and `unknown` for a completed run whose `unknown`
    /// count is above 0.
    pub enum RunCause {
        NothingToDo = "nothing_to_do",
        Declined = "declined",
        HandedBack = "handed_back",
        ClassMismatch = "class_mismatch",
        Refused = "refused",
        Limit = "limit",
        EffectUnknown = "effect_unknown",
        Failed = "failed",
        Unknown = "unknown",
    }
}

names! {
    /// The counted cause of a revert, the closed list of `learn_edit`'s `cause` (contracts.md L3.5).
    pub enum EditCause {
        StepFailed = "step_failed",
        ClassMismatch = "class_mismatch",
        Origin = "origin",
    }
}

names! {
    /// The mark of a routine (contracts.md L6.3).
    pub enum Mark {
        None = "none",
        Stale = "stale",
    }
}

names! {
    /// What an entry of the reply of `list_refinements` says (contracts.md L2.2): an edit of the newest compile
    /// number by the state of its newest row, or a routine marked `stale`.
    pub enum RefinementKind {
        TargetLearned = "target_learned",
        TargetKept = "target_kept",
        TargetRevoked = "target_revoked",
        RoutineStale = "routine_stale",
    }
}

/// The names of `learn_run`'s `state`, in the order of the contract.
pub const RUN_STATES: [&str; 3] = ["started", "verified", "unverified"];

/// The names of `learn_edit`'s `state`, in the order of the contract.
pub const EDIT_STATES: [&str; 4] = ["provisional", "active", "reverted", "withdrawn"];

pub(in crate::learning) fn state_name(
    type_name: &'static str,
    names: &[&'static str],
    raw: &RawValue,
) -> Result<&'static str, Refusal> {
    let s = json::string(type_name, raw)?;
    names
        .iter()
        .find(|name| **name == s)
        .copied()
        .ok_or(Refusal::new(type_name, "unknown_name"))
}
