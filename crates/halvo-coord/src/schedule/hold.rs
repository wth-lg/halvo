use crate::types::{
    Category, Cause, Cleared, ConfigView, Facts, HoldSetCause, Param, Runtime, What,
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

/// The parameters of a category that a setup binds in a slot, the rows of A-A1.5 with the
/// source `activation` (I3.5); none for a category that I3.5 does not list.
pub fn params(category: Category) -> &'static [Param] {
    match category {
        Category::UtilityBillPayment => &[Param::Start, Param::Payee, Param::Instrument],
        Category::Email => &[Param::ReplySenders],
        Category::SpreadsheetFill => &[Param::Workbook, Param::Sheet, Param::Range, Param::Source],
        Category::ShopOrder => &[Param::Start, Param::Seller, Param::Instrument],
        Category::CalendarCleanup
        | Category::TenantRent
        | Category::LeaseRenewalOrChange
        | Category::TenantApplicationReview
        | Category::RideOrder
        | Category::DailyChartCheck => &[],
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

fn check<C>(facts: &Facts<C>) -> bool {
    facts.runs.first().is_some_and(|newest| {
        newest.unknown > 0
            && !facts.events.iter().any(|e| {
                matches!(&e.what, What::HoldCleared { cause: Cleared::Check { run, .. } } if *run == newest.run)
            })
    })
}

/// Each hold that stands for the agent, in the order of I5.4, read from the facts alone and
/// never from the state of its workflow: `not_yet` while its runtime is `none`; `off` from a
/// `hold_set` event with the cause `person` until a later `hold_cleared` event with the cause
/// `person`; `failures` from a `hold_set` event with the cause `failures` until a later
/// `hold_cleared` event with the cause `failures`; `check` while its newest run record has an
/// effect of an unknown outcome, which the agent store also counts for an effect that a dead
/// runner left in `releasing` (A-A7.5, I5.8), and no `hold_cleared` event of the cause `check`
/// names that run; `setup` while a parameter of its category (I3.5) has no slot in its newest
/// bindings. Both `off` and `failures` are returned where both stand.
pub fn holds<C: ConfigView>(facts: &Facts<C>) -> Vec<Hold> {
    let mut out = Vec::new();
    if facts.config.runtime() == Runtime::None {
        out.push(Hold::NotYet);
    }
    if stands(facts, HoldSetCause::Person) {
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
