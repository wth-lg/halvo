use crate::items::params;
use crate::types::{
    Category, Cause, Cleared, ConfigView, Doing, Facts, HoldSetCause, OwnItemKind, Raised, Runtime,
    What, WorkflowState,
};

/// The holds of I5.4, in their order: each stops every start of its agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hold {
    /// `not_yet`.
    NotYet,
    /// `off`.
    Off,
    /// `failures`.
    Failures,
    /// `check`.
    Check,
    /// `setup`.
    Setup,
}

impl Hold {
    /// Every hold, in the order of I5.4.
    pub const ALL: &'static [Hold] = &[
        Hold::NotYet,
        Hold::Off,
        Hold::Failures,
        Hold::Check,
        Hold::Setup,
    ];

    /// The hold's name.
    pub const fn name(self) -> &'static str {
        match self {
            Hold::NotYet => "not_yet",
            Hold::Off => "off",
            Hold::Failures => "failures",
            Hold::Check => "check",
            Hold::Setup => "setup",
        }
    }

    /// Reads a name of the list; any other string is `malformed` (I0.6).
    pub fn parse(text: &str) -> Result<Hold, Cause> {
        Hold::ALL
            .iter()
            .copied()
            .find(|h| h.name() == text)
            .ok_or(Cause::Malformed)
    }
}

fn stands<C>(facts: &Facts<C>, hold: HoldSetCause) -> bool {
    facts.events.iter().fold(false, |on, e| match &e.what {
        What::HoldSet { cause } if *cause == hold => true,
        What::HoldCleared {
            cause: Cleared::Person,
        } if hold == HoldSetCause::Person => false,
        What::HoldCleared {
            cause: Cleared::Failures,
        } if hold == HoldSetCause::Failures => false,
        _ => on,
    })
}

/// Whether the hold `off` or the hold `failures` stands by the stored events (I5.4): from its
/// `hold_set` event until a later `hold_cleared` event of the same cause.
pub fn held_by_events<C>(facts: &Facts<C>) -> bool {
    stands(facts, HoldSetCause::Person) || stands(facts, HoldSetCause::Failures)
}

/// The derived hold `off` of ruling RD-67 1.2 (I5.4): the owner's list holds the agent's workflow
/// in the full shape with the state `paused` while neither the hold `off` nor the hold `failures`
/// stands by the stored events, because only this coordinator's `set_state` writes `paused`; the
/// stand-in of a dismissed workflow (I5.8) is no entry of that list and gives none. Only the
/// control `switch_on` ends it, through an acknowledged state, and nothing is stored for it.
pub fn kept_off<C>(facts: &Facts<C>) -> bool {
    !facts.stand_in && facts.workflow.state == WorkflowState::Paused && !held_by_events(facts)
}

fn check<C>(facts: &Facts<C>) -> bool {
    let Some(newest) = facts.runs.first().filter(|_| facts.store_ended) else {
        return false;
    };
    let raised = facts
        .events
        .iter()
        .skip_while(|e| !matches!(&e.what, What::RunStarted { run, .. } if *run == newest.run))
        .any(|e| {
            matches!(
                e.what,
                What::ItemRaised {
                    raised: Raised::Check(None),
                    ..
                }
            )
        });
    let not_known = newest.unknown > 0
        || facts.unknown.iter().any(|u| u.run == newest.run)
        || facts.replies.not_known()
        || (facts.workflow.category == Category::Email && raised)
        || matches!(&facts.doing, Doing::Idle(Some(own)) if own.kind == OwnItemKind::Check);
    not_known
        && !facts.events.iter().any(|e| {
            matches!(&e.what, What::HoldCleared { cause: Cleared::Check { run, .. } } if *run == newest.run)
        })
}

/// Each hold that stands for the agent, in the order of I5.4, read from the facts alone:
/// `not_yet` while its runtime is `none`; `off` from a `hold_set` event with the cause `person`
/// until a later `hold_cleared` event with the cause `person`, or by the derived hold of
/// `kept_off`, the one test that reads the state of the workflow (ruling RD-67 1.2, which
/// replaces ruling H-1 in that part); `failures` from a `hold_set` event with the cause `failures`
/// until a later
/// `hold_cleared` event with the cause `failures`; `check` while the agent store has ended its
/// newest run and no `hold_cleared` event of the cause `check` names that run, and that run's
/// record has `unknown` above 0, the roster's key `unknown` of the same read names an effect of
/// that run, the move's read of its reply rows answered a row that is not known, the agent holds
/// its own item `check` open on the board, or, for an agent of the category `email`, an
/// `item_raised` of the kind `check` with no run follows that run's `run_started`; so a card once
/// raised keeps its hold until the person's answer, whatever a later read of the record, the key
/// or the rows gives or a roster read that did not answer, in its life, and for an `email` agent
/// in the next life too (rulings XB-2, RD-67 3.3, RD-81 8.1, 8.2, 8.4, RD-130, RD-134). Named limit: for an agent of another category whose
/// key entry left the roster, after Halvo is opened again the card of the earlier life has lapsed
/// and the agent may start or show its ready card without that answer. `setup` while a parameter
/// of its category (I3.5) has no slot in its newest bindings. Both `off` and `failures` are
/// returned where both stand.
pub fn holds<C: ConfigView>(facts: &Facts<C>) -> Vec<Hold> {
    let mut out = Vec::new();
    if facts.config.runtime() == Runtime::None {
        out.push(Hold::NotYet);
    }
    if stands(facts, HoldSetCause::Person) || kept_off(facts) {
        out.push(Hold::Off);
    }
    if stands(facts, HoldSetCause::Failures) {
        out.push(Hold::Failures);
    }
    if check(facts) {
        out.push(Hold::Check);
    }
    let unbound = params(facts.workflow.category)
        .iter()
        .any(|p| !facts.slots.iter().any(|s| s.as_str() == p.name()));
    if unbound {
        out.push(Hold::Setup);
    }
    out
}
