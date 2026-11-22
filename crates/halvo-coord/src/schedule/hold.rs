use crate::items::params;
use crate::types::{
    Category, Cause, Cleared, ConfigView, Doing, Facts, HoldSetCause, OwnItemKind, Raised, RunId,
    Runtime, What, WorkflowState,
};

/// The holds of I5.4, in their order: each stops every start of its agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hold {
    /// `not_yet`.
    NotYet,
    /// `off`.
    Off,
    /// `sign_in_refused`: the hold after a refused sign-in (ruling RD-73 4.2).
    SignInRefused,
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
        Hold::SignInRefused,
        Hold::Failures,
        Hold::Check,
        Hold::Setup,
    ];

    /// The hold's name.
    pub const fn name(self) -> &'static str {
        match self {
            Hold::NotYet => "not_yet",
            Hold::Off => "off",
            Hold::SignInRefused => "sign_in_refused",
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

fn cleared_from<C>(facts: &Facts<C>, from: usize, run: &RunId) -> bool {
    facts.events[from.min(facts.events.len())..].iter().any(|e| {
        matches!(&e.what, What::HoldCleared { cause: Cleared::SignInRefused { run: r } } if r == run)
    })
}

/// The runs whose refused sign-in holds the agent by the stored events (I5.4; ruling RD-73
/// 4.2): each `hold_set` of the cause `sign_in_refused` with no later `hold_cleared` of that
/// cause that names its run.
fn refused_by_events<C>(facts: &Facts<C>) -> Vec<RunId> {
    let mut out: Vec<RunId> = Vec::new();
    for (k, e) in facts.events.iter().enumerate() {
        if let What::HoldSet {
            cause: HoldSetCause::SignInRefused { run },
        } = &e.what
        {
            if !cleared_from(facts, k + 1, run) && !out.contains(run) {
                out.push(run.clone());
            }
        }
    }
    out
}

/// The runs whose refused sign-in holds the agent by `sign_in_refused` (I5.4; ruling RD-73 4.2):
/// by the stored events, from its `hold_set` until a `hold_cleared` of that cause names the run,
/// whatever the roster then shows; and by the facts, the run that the roster's key `refused_run`
/// names while no `hold_cleared` of that cause names it. Time ends none.
pub fn refused_runs<C>(facts: &Facts<C>) -> Vec<RunId> {
    let mut out = refused_by_events(facts);
    if let Some(run) = &facts.refused {
        if !cleared_from(facts, 0, run) && !out.contains(run) {
            out.push(run.clone());
        }
    }
    out
}

/// The run whose refused sign-in the roster shows while no event of the cause `sign_in_refused`
/// names it: the move that reads it stores its one `hold_set` (I5.4; ruling RD-73 4.2).
pub fn refusal_unstored<C>(facts: &Facts<C>) -> Option<RunId> {
    let run = facts.refused.as_ref()?;
    let named = facts.events.iter().any(|e| match &e.what {
        What::HoldSet {
            cause: HoldSetCause::SignInRefused { run: r },
        }
        | What::HoldCleared {
            cause: Cleared::SignInRefused { run: r },
        } => r == run,
        _ => false,
    });
    (!named).then(|| run.clone())
}

/// Whether the hold `off`, the hold `sign_in_refused` or the hold `failures` stands by the stored
/// events (I5.4): from its `hold_set` event until a later `hold_cleared` event of the same cause,
/// for `sign_in_refused` one that names the same run.
pub fn held_by_events<C>(facts: &Facts<C>) -> bool {
    stands(facts, HoldSetCause::Person)
        || !refused_by_events(facts).is_empty()
        || stands(facts, HoldSetCause::Failures)
}

/// The derived hold `off` of ruling RD-67 1.2 (I5.4): the owner's list holds the agent's workflow
/// in the full shape with the state `paused` while none of the holds `off`, `sign_in_refused` and
/// `failures` stands by the stored events (`held_by_events`; ruling RD-73 4.3), because only this
/// coordinator's `set_state` writes `paused`; the
/// stand-in of a dismissed workflow (I5.8) is no entry of that list and gives none. It stands as
/// well while this life's mark of an `active` refused with `overlap` stands
/// (`Facts::active_refused`), whatever the list shows, so that no start passes while the
/// `paused` that follows has no acknowledgement (ruling RD-160 (1)). Only the control
/// `switch_on` ends it, through an acknowledged state, and nothing is stored for it.
pub fn kept_off<C>(facts: &Facts<C>) -> bool {
    let listed_paused = facts.workflow.state == WorkflowState::Paused;
    !facts.stand_in && (listed_paused || facts.active_refused) && !held_by_events(facts)
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
/// replaces ruling H-1 in that part); `sign_in_refused` while `refused_runs` names a run (ruling
/// RD-73 4.2); `failures` from a `hold_set` event with the cause `failures` until a later
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
    if !refused_runs(facts).is_empty() {
        out.push(Hold::SignInRefused);
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
