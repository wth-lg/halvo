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

            pub(in crate::coordinator) fn from_raw(raw: &RawValue) -> Result<Self, Refusal> {
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
    /// The nine events of `coord_event` (contracts.md I7.1).
    pub enum Event {
        RunStarted = "run_started",
        RunEnded = "run_ended",
        ItemRaised = "item_raised",
        ItemAnswered = "item_answered",
        ItemLapsed = "item_lapsed",
        ItemReleased = "item_released",
        HoldSet = "hold_set",
        HoldCleared = "hold_cleared",
        SlotSkipped = "slot_skipped",
    }
}

names! {
    /// The eight kinds of item (contracts.md I3.1).
    pub enum ItemKind {
        Confirm = "confirm",
        SignIn = "sign_in",
        LinkAccount = "link_account",
        Stuck = "stuck",
        Check = "check",
        Setup = "setup",
        Ready = "ready",
        Show = "show",
    }
}

names! {
    /// The seven kinds of item other than `confirm`, whose lapse takes a cause of `OtherLapse`.
    pub enum OtherKind {
        SignIn = "sign_in",
        LinkAccount = "link_account",
        Stuck = "stuck",
        Check = "check",
        Setup = "setup",
        Ready = "ready",
        Show = "show",
    }
}

impl OtherKind {
    /// The kind of item of the same name.
    pub fn kind(self) -> ItemKind {
        match self {
            OtherKind::SignIn => ItemKind::SignIn,
            OtherKind::LinkAccount => ItemKind::LinkAccount,
            OtherKind::Stuck => ItemKind::Stuck,
            OtherKind::Check => ItemKind::Check,
            OtherKind::Setup => ItemKind::Setup,
            OtherKind::Ready => ItemKind::Ready,
            OtherKind::Show => ItemKind::Show,
        }
    }
}

names! {
    /// Who asked for a start, the `trigger` of `run_started` (contracts.md I5.5, I7.1).
    pub enum Trigger {
        Person = "person",
        Schedule = "schedule",
    }
}

names! {
    /// The fourteen ends of a run that a stored `run_ended` row may hold under `cause`: the nine outcomes of the
    /// agent-setup run and five of the coordinator's own six (contracts.md I5.6). The sixth, `deleted`, is the end
    /// of a run whose workflow is gone, "the cause of no stored row" (I5.6), and I7.1 stores no `run_ended` for
    /// such a run, so no value of this list holds it and the reader refuses it as an unknown name.
    pub enum RunEnd {
        Completed = "completed",
        NothingToDo = "nothing_to_do",
        Declined = "declined",
        HandedBack = "handed_back",
        ClassMismatch = "class_mismatch",
        Refused = "refused",
        Limit = "limit",
        EffectUnknown = "effect_unknown",
        Failed = "failed",
        Cancelled = "cancelled",
        Lapsed = "lapsed",
        Interrupted = "interrupted",
        Budget = "budget",
        Kind = "kind",
    }
}

names! {
    /// The six causes of the lapse of a confirmation (contracts.md I4.2).
    pub enum ConfirmLapse {
        Digest = "digest",
        RunEnded = "run_ended",
        WaitLimit = "wait_limit",
        Cancelled = "cancelled",
        NoRecord = "no_record",
        AppEnded = "app_ended",
    }
}

names! {
    /// The causes of the lapse of an item of a kind other than `confirm` (contracts.md I7.1): `check`, an open
    /// `ready` item that the item `check` replaces in the same body (ruling RD-5), for a `ready` item alone, and
    /// `cancelled`, the person's stop of the run whose item it is (ruling RD-6).
    pub enum OtherLapse {
        RunEnded = "run_ended",
        AppEnded = "app_ended",
        WaitLimit = "wait_limit",
        Check = "check",
        Cancelled = "cancelled",
    }
}

names! {
    /// The `cause` of `hold_set`: the hold that was set, `person` for the hold `off` (contracts.md I7.1).
    pub enum HoldSetCause {
        Person = "person",
        Failures = "failures",
    }
}

names! {
    /// The `cause` of `hold_cleared`: the hold that ended (contracts.md I7.1).
    pub enum HoldCause {
        Person = "person",
        Failures = "failures",
        Check = "check",
    }
}

names! {
    /// The answer for one effect of a confirmation (the agent-setup run's A6.3).
    pub enum EffectAnswer {
        AllowOnce = "allow_once",
        RejectOnce = "reject_once",
    }
}

names! {
    /// The two choices of `stuck`, `show`, the first step of `sign_in` and `link_account` (contracts.md I3.1).
    pub enum DoneAnswer {
        Decline = "decline",
        Done = "done",
    }
}

names! {
    /// The two choices of the second step of a `sign_in` item (contracts.md I3.1, I7.1).
    pub enum BindResult {
        Decline = "decline",
        Allow = "allow",
    }
}

names! {
    /// The two choices of `check` (contracts.md I3.1).
    pub enum CheckAnswer {
        TryAgain = "try_again",
        WentThrough = "went_through",
    }
}

names! {
    /// The one choice of `setup` that stores a row: `none` stores none (contracts.md I7.1).
    pub enum SetupAnswer {
        Choose = "choose",
    }
}

names! {
    /// The two choices of `ready` (contracts.md I3.1).
    pub enum ReadyAnswer {
        Skip = "skip",
        Start = "start",
    }
}
