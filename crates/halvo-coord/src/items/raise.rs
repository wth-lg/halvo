use crate::types::{
    Answered, BindAsk, Category, CheckAnswer, Choice, Class, Cleared, ConfirmAsk, Doing, Facts,
    HandbackAsk, HandbackReason, HandbackResult, ItemId, ItemKind, Limits, Live, OwnItemKind,
    Param, Raised, RunEnd, RunEvent, RunId, RunItemKind, What,
};

/// The six classes that a confirm request may name (A-A4.1, I3.1).
pub const CONFIRM_CLASSES: [Class; 6] = [
    Class::Send,
    Class::Delete,
    Class::Submit,
    Class::Purchase,
    Class::Payment,
    Class::Sign,
];

/// Where an item would come from: the only sources of I3.1, and a request of a type that
/// A-A6.2 does not list.
#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    /// A confirm request of the live run.
    Confirm(&'a ConfirmAsk),
    /// A handback request of the live run.
    Handback(&'a HandbackAsk),
    /// The second step of a `sign_in` item of the live run, the run's event `Bind`.
    Bind(&'a BindAsk),
    /// A request of a type that A-A6.2 does not list.
    Other,
    /// The first run of an agent in the mode `compile`, in path B of the agent-setup run.
    Show(&'a RunId),
    /// The hold `check`.
    Check,
    /// The hold `setup`.
    Setup,
    /// The scheduler, for a due agent that needs the person or a first run.
    Ready,
}

impl<'a> Source<'a> {
    /// The source of a run event that is a request, or none for `hello`, `progress` and the
    /// runner's end.
    pub fn of(event: &'a RunEvent) -> Option<Source<'a>> {
        match event {
            RunEvent::Confirm(ask) => Some(Source::Confirm(ask)),
            RunEvent::Handback(ask) => Some(Source::Handback(ask)),
            RunEvent::Bind(ask) => Some(Source::Bind(ask)),
            RunEvent::Other => Some(Source::Other),
            RunEvent::Hello | RunEvent::Progress(_) | RunEvent::Ended(_) => None,
        }
    }
}

/// What a source does to an agent (I3.1 to I3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raise {
    /// Raises an item of this kind for the request of the live run.
    Run(RunItemKind),
    /// Raises an item of this kind, which no run raises.
    Own(OwnItemKind),
    /// Raises no item: the handback is answered from the record of an answered check (I5.4).
    Answer(HandbackResult),
    /// Raises no item: what it would ask is bound (I3.2).
    Nothing,
    /// Refused with the cause; nothing changes and the open item stays.
    Refused(crate::types::Cause),
    /// The run ends with this cause and nothing is raised (I3.1, I3.3).
    End(RunEnd),
}

/// The parameters of a category, the rows of A-A1.5 with the source `activation` that are bound
/// in a slot (I3.5): the one table, which the hold `setup`, the item `setup` and the decision of
/// the setup flow read (ruling IG-11).
pub const fn params(category: Category) -> &'static [Param] {
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

/// The kind that a handback request raises by its reason (I3.1).
pub const fn handback_kind(reason: HandbackReason) -> RunItemKind {
    match reason {
        HandbackReason::SignIn | HandbackReason::SecondFactor | HandbackReason::Captcha => {
            RunItemKind::SignIn
        }
        HandbackReason::AccountLink => RunItemKind::LinkAccount,
        HandbackReason::StepFailed => RunItemKind::Stuck,
        HandbackReason::EffectUnknown => RunItemKind::Check,
    }
}

/// The two choices of a kind, the safe one first (I3.1).
pub const fn choices(kind: ItemKind) -> [Choice; 2] {
    match kind {
        ItemKind::Confirm => [Choice::Reject, Choice::Allow],
        ItemKind::SignIn | ItemKind::LinkAccount | ItemKind::Stuck | ItemKind::Show => {
            [Choice::Decline, Choice::Done]
        }
        ItemKind::Check => [Choice::TryAgain, Choice::WentThrough],
        ItemKind::Setup => [Choice::None, Choice::Choose],
        ItemKind::Ready => [Choice::Skip, Choice::Start],
    }
}

/// The result that a handback gets for a choice of the item that it raised (I3.1): `done`
/// gives `done` and `decline` gives `declined`, and for a `check`, `try_again` gives `done`
/// and `went_through` gives `declined`; `malformed` for any other pair.
pub fn handback_result(
    kind: RunItemKind,
    choice: Choice,
) -> Result<HandbackResult, crate::types::Cause> {
    match (kind, choice) {
        (RunItemKind::Confirm, _) => Err(crate::types::Cause::Malformed),
        (RunItemKind::Check, Choice::TryAgain) => Ok(HandbackResult::Done),
        (RunItemKind::Check, Choice::WentThrough) => Ok(HandbackResult::Declined),
        (RunItemKind::Check, _) => Err(crate::types::Cause::Malformed),
        (_, Choice::Done) => Ok(HandbackResult::Done),
        (_, Choice::Decline) => Ok(HandbackResult::Declined),
        _ => Err(crate::types::Cause::Malformed),
    }
}

fn run_of(source: &Source<'_>) -> Option<RunId> {
    match source {
        Source::Confirm(ask) => Some(ask.run().clone()),
        Source::Handback(ask) => Some(ask.run.clone()),
        Source::Bind(ask) => Some(ask.run.clone()),
        Source::Show(run) => Some((*run).clone()),
        Source::Other | Source::Check | Source::Setup | Source::Ready => None,
    }
}

fn raised_by<C>(facts: &Facts<C>, run: &RunId) -> Vec<ItemId> {
    facts
        .events
        .iter()
        .filter_map(|e| match &e.what {
            What::ItemRaised { item, raised } if raised.run() == Some(run) => Some(item.clone()),
            _ => None,
        })
        .collect()
}

/// The answer that the person gave to a check of `run` or of the agent's last run, the newest
/// first: the `hold_cleared` event of a hold `check`, or the `item_answered` row of a check
/// that a handback of that run raised (I3.2, I5.4).
fn answered_check<C>(facts: &Facts<C>, run: &RunId) -> Option<CheckAnswer> {
    let last = facts.runs.first().map(|r| &r.run);
    let named = |r: &RunId| r == run || Some(r) == last;
    facts.events.iter().rev().find_map(|e| match &e.what {
        What::HoldCleared {
            cause: Cleared::Check { run: r, answer },
        } if named(r) => Some(*answer),
        What::ItemAnswered {
            item,
            answer: Answered::Check(answer),
        } => facts
            .events
            .iter()
            .any(|f| {
                matches!(&f.what, What::ItemRaised { item: i, raised: Raised::Check(Some(r)) }
                    if i == item && named(r))
            })
            .then_some(*answer),
        _ => None,
    })
}

/// What a source does to the agent of `facts` (I3.1 to I3.3), first that fits. A request of a
/// run is refused with `state` unless the run is `working`; the run ends with `kind` for a
/// request of a type that A-A6.2 does not list, a confirm request that names a class outside
/// the six of A-A4.1, and a request while its run waits for an open item; a handback
/// `effect_unknown` after the person answered the check of that run or of the agent's last run
/// is answered from that answer; a request after the run raised `items_per_run` items ends the
/// run with `budget`. A source of the coordinator's own is refused with `state` while the agent
/// has an open item or a live run, and a `setup` raises nothing when the newest bindings hold
/// every parameter of the category.
pub fn raise_step<C>(facts: &Facts<C>, source: Source<'_>, limits: &Limits) -> Raise {
    use crate::types::Cause;
    let own = match source {
        Source::Check => Some(OwnItemKind::Check),
        Source::Setup => Some(OwnItemKind::Setup),
        Source::Ready => Some(OwnItemKind::Ready),
        Source::Confirm(_)
        | Source::Handback(_)
        | Source::Bind(_)
        | Source::Other
        | Source::Show(_) => None,
    };
    if let Some(kind) = own {
        if facts.doing.item().is_some() || matches!(facts.doing, Doing::Live(_)) {
            return Raise::Refused(Cause::State);
        }
        if kind == OwnItemKind::Setup
            && params(facts.workflow.category)
                .iter()
                .all(|p| facts.slots.iter().any(|s| s.as_str() == p.name()))
        {
            return Raise::Nothing;
        }
        return Raise::Own(kind);
    }
    match &facts.doing {
        Doing::Live(Live::Working) => {}
        Doing::Live(Live::Waiting(_)) => return Raise::End(RunEnd::Kind),
        Doing::Live(Live::Starting | Live::Stopping) | Doing::Idle(_) | Doing::Queued(_) => {
            return Raise::Refused(Cause::State)
        }
    }
    let kind = match source {
        Source::Confirm(ask) => {
            if ask
                .effects()
                .iter()
                .any(|e| !CONFIRM_CLASSES.contains(&e.class))
            {
                return Raise::End(RunEnd::Kind);
            }
            RunItemKind::Confirm
        }
        Source::Handback(ask) => handback_kind(ask.reason),
        Source::Bind(_) => RunItemKind::SignIn,
        Source::Show(_) => RunItemKind::Show,
        Source::Other | Source::Check | Source::Setup | Source::Ready => {
            return Raise::End(RunEnd::Kind)
        }
    };
    let Some(run) = run_of(&source) else {
        return Raise::End(RunEnd::Kind);
    };
    if let Source::Handback(ask) = source {
        if ask.reason == HandbackReason::EffectUnknown {
            if let Some(answer) = answered_check(facts, &run) {
                return Raise::Answer(match answer {
                    CheckAnswer::TryAgain => HandbackResult::Done,
                    CheckAnswer::WentThrough => HandbackResult::Declined,
                });
            }
        }
    }
    if raised_by(facts, &run).len() >= limits.items_per_run as usize {
        return Raise::End(RunEnd::Budget);
    }
    Raise::Run(kind)
}
