//! The rule of admission and rollback of a learned edit (contracts.md L3) and the routine
//! step (L6.2, L6.3): pure functions over small types of this crate, with no I/O, no clock,
//! no store and no dependency. A caller turns its own types into these with exhaustive
//! `match` functions, so a name added on one side fails the build of the other.

macro_rules! tokens {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($variant:ident = $token:literal,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $(
                #[doc = concat!("`", $token, "`")]
                $variant,
            )+
        }

        impl $name {
            /// Every value, in the order of the contract's list.
            pub const ALL: &'static [$name] = &[$($name::$variant,)+];

            /// The token of the value, as the contract writes it.
            pub fn token(self) -> &'static str {
                match self {
                    $($name::$variant => $token,)+
                }
            }

            /// The value whose token is `token`, or `None` for any other text.
            pub fn from_token(token: &str) -> Option<$name> {
                $name::ALL.iter().copied().find(|value| value.token() == token)
            }
        }
    };
}

mod admit;
mod edit;
mod routine;
mod run;
mod sources;

pub use admit::{
    admit, brought, build_list, is_rejected, new_locators, value_hit, AdmitCause, AdmitFacts,
    Locator, RunMode, Target,
};
pub use edit::{edit_step, use_move, EditAnswer, EditMove, EditState};
pub use routine::{routine_step, Mark, RoutineState};
pub use run::{
    candidate_move, counted, verify_run, CandidateFacts, CandidateMove, EditCause, LogCause,
    Outcome, RunCause, RunFacts,
};
pub use sources::{check_sources, EditSources, RunSource, SourceCause, SourceFacts};

/// The clean uses that make a `provisional` edit `active` (L3.6).
pub const K_CLEAN: u32 = 2;

/// The most learned locators of one edit's list, which stand before the compiled ones (L3.6).
pub const LOCATORS_MAX: usize = 4;

/// The most edits of one step per compile number (L3.6).
pub const EDITS_MAX: u32 = 8;

/// The fewest bytes of a slot value that `value_hit` searches for (L3.6).
pub const VALUE_MIN: usize = 4;

/// The missed cycles from which a routine is marked `stale`; 0 sets no mark (L6.3).
pub const MISS_CYCLES: u32 = 3;

/// The fewest days since the last occurrence for the mark `stale` (L6.3).
pub const STALE_FLOOR: u32 = 60;

/// The days by which each cycle may be late before it counts as missed (L6.2).
pub const TOLERANCE: u32 = 5;
