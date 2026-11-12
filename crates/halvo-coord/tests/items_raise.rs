#[path = "items_support.rs"]
mod support;

use halvo_coord::guard::{EffectRecord, EffectState, Table};
use halvo_coord::items::raise_confirm;
use halvo_coord::items::{
    choices, handback_kind, handback_result, params, raise_step, Raise, Source, CONFIRM_CLASSES,
};
use halvo_coord::mem::Config;
use halvo_coord::types::fixtures::{self, State};
use halvo_coord::types::{
    Answered, Category, Cause, CheckAnswer, Choice, Class, Cleared, Doing, Event, Facts,
    HandbackAsk, HandbackReason, HandbackResult, ItemKind, Limits, Live, Outcome, OwnItemKind,
    Param, PastRun, Raised, RunEnd, RunEvent, RunItem, RunItemKind, What,
};
use halvo_coord::types::{Decision, DecisionEntry, EffectAnswer};
use std::num::NonZeroU32;
use support::{ask_of, asked, item_id, name, run_id};

/// A record of effect `effect` of run `run` in `state`.
fn record(run: u32, effect: u32, state: EffectState) -> EffectRecord {
    EffectRecord {
        run: run_id(run),
        agent: support::agent_id(1),
        effect: support::effect_id(effect),
        state,
        class: Class::Payment,
        digest: support::digest(effect),
        approval: None,
    }
}

/// The table after the release of a decision of item `item` of run `run` with `answer` for
/// effect `effect`.
fn released(t: &Table, item: u32, run: u32, effect: u32, answer: EffectAnswer) -> Table {
    let decision = Decision::new(vec![DecisionEntry {
        approval: support::approval(item),
        effect: support::effect_id(effect),
        digest: support::digest(effect),
        answer,
    }])
    .unwrap();
    t.record(&item_id(item), &run_id(run), &decision)
        .release(&item_id(item))
}

fn agent_in(state: State, category: Category) -> Facts<Config> {
    fixtures::facts(state)
        .into_iter()
        .find(|f| f.workflow.category == category)
        .expect("the agent of the category in the state")
}

/// An agent whose run 1 is `working`.
fn working() -> Facts<Config> {
    let f = agent_in(State::Working, Category::UtilityBillPayment);
    assert_eq!(f.doing, Doing::Live(Live::Working));
    f
}

fn waiting() -> Facts<Config> {
    let mut f = working();
    f.doing = Doing::Live(Live::Waiting(RunItem {
        item: item_id(1),
        kind: RunItemKind::Confirm,
    }));
    f
}

fn event(f: &Facts<Config>, what: What) -> Event {
    let seq = f.events.last().map_or(1, |e| e.seq.get() + 1);
    Event {
        workflow: f.workflow.id.clone(),
        agent: f.agent.clone(),
        seq: NonZeroU32::new(seq).unwrap(),
        day: f.today,
        at: 0,
        what,
    }
}

fn push(f: &mut Facts<Config>, what: What) {
    let e = event(f, what);
    f.events.push(e);
}

/// A handback request of `run` with `reason`; a request `account_link` holds the base address of
/// its adapter as its origin, as the runner fills it (A-A6.4).
fn handback(run: u32, reason: HandbackReason) -> HandbackAsk {
    HandbackAsk {
        run: run_id(run),
        agent: support::agent_id(1),
        reason,
        origin: (reason == HandbackReason::AccountLink)
            .then(|| halvo_coord::types::Origin::parse("https://mail.site5.example:443").unwrap()),
    }
}

/// A handback `account_link` with no origin cannot show the site that its item must show, so it
/// raises no item and ends the run with `kind`, as a request outside the runner's contract does
/// (I3.1, I3.4; A-A6.4 fills the origin of every `account_link`); with its origin it raises the
/// item `link_account`.
#[test]
fn a_link_request_with_no_origin_ends_the_run_with_kind() {
    let f = working();
    let mut h = handback(1, HandbackReason::AccountLink);
    assert_eq!(
        raise_step(&f, Source::Handback(&h), &LIMITS),
        Raise::Run(RunItemKind::LinkAccount)
    );
    h.origin = None;
    assert_eq!(
        raise_step(&f, Source::Handback(&h), &LIMITS),
        Raise::End(RunEnd::Kind)
    );
    for reason in HandbackReason::ALL
        .iter()
        .filter(|r| **r != HandbackReason::AccountLink)
    {
        let mut other = handback(1, *reason);
        other.origin = None;
        assert_eq!(
            raise_step(&f, Source::Handback(&other), &LIMITS),
            Raise::Run(handback_kind(*reason)),
            "{reason} needs no origin"
        );
    }
}

fn confirm_of(classes: &[Class]) -> halvo_coord::types::ConfirmAsk {
    ask_of(
        1,
        1,
        classes
            .iter()
            .zip(1..)
            .map(|(c, i)| asked(i, *c, i, true))
            .collect(),
    )
}

const LIMITS: Limits = Limits::PRODUCT;

#[test]
fn refusal_kind_reason() {
    assert_eq!(
        HandbackReason::parse("other"),
        Err(Cause::Malformed),
        "a handback with the reason other makes no typed request"
    );
    let f = working();
    let other = RunEvent::Other;
    let source = Source::of(&other).expect("a request of a type that A-A6.2 does not list");
    assert_eq!(raise_step(&f, source, &LIMITS), Raise::End(RunEnd::Kind));
    for reason in HandbackReason::ALL {
        let h = handback(1, *reason);
        assert_eq!(
            raise_step(&f, Source::Handback(&h), &LIMITS),
            Raise::Run(handback_kind(*reason)),
            "{reason}"
        );
    }
}

#[test]
fn refusal_kind_class() {
    let f = working();
    for class in Class::ALL {
        let ask = confirm_of(&[Class::Send, *class]);
        let six = [
            Class::Send,
            Class::Delete,
            Class::Submit,
            Class::Purchase,
            Class::Payment,
            Class::Sign,
        ];
        let want = if six.contains(class) {
            Raise::Run(RunItemKind::Confirm)
        } else {
            Raise::End(RunEnd::Kind)
        };
        assert_eq!(
            raise_step(&f, Source::Confirm(&ask), &LIMITS),
            want,
            "{class}"
        );
    }
    let read = confirm_of(&[Class::Read]);
    assert_eq!(
        raise_step(&f, Source::Confirm(&read), &LIMITS),
        Raise::End(RunEnd::Kind),
        "a confirm request with an effect of the class read"
    );
}

#[test]
fn refusal_kind_frame() {
    assert_eq!(
        raise_step(&working(), Source::Other, &LIMITS),
        Raise::End(RunEnd::Kind),
        "a request of a type that A-A6.2 does not list"
    );
    let f = waiting();
    let ask = confirm_of(&[Class::Send]);
    let h = handback(1, HandbackReason::SignIn);
    let run = run_id(1);
    for (label, source) in [
        ("a confirm request", Source::Confirm(&ask)),
        ("a handback request", Source::Handback(&h)),
        ("a show", Source::Show(&run)),
        ("an unlisted request", Source::Other),
    ] {
        assert_eq!(
            raise_step(&f, source, &LIMITS),
            Raise::End(RunEnd::Kind),
            "{label} before the first is answered"
        );
    }
}

#[test]
fn the_six_confirm_classes_of_a_a4_1() {
    assert_eq!(
        CONFIRM_CLASSES,
        [
            Class::Send,
            Class::Delete,
            Class::Submit,
            Class::Purchase,
            Class::Payment,
            Class::Sign
        ]
    );
}

#[test]
fn refusal_second_item() {
    let ready = agent_in(State::NeedsReady, Category::Email);
    let first = ready.doing.item().expect("an open ready item");
    for source in [Source::Check, Source::Setup, Source::Ready] {
        assert_eq!(
            raise_step(&ready, source, &LIMITS),
            Raise::Refused(Cause::State),
            "{source:?} beside an open item"
        );
    }
    assert_eq!(ready.doing.item(), Some(first), "the first stays");
    for f in [waiting(), working()] {
        for source in [Source::Check, Source::Setup, Source::Ready] {
            assert_eq!(
                raise_step(&f, source, &LIMITS),
                Raise::Refused(Cause::State),
                "{source:?} beside a live run"
            );
        }
    }
}

#[test]
fn refusal_items_budget() {
    let ask = confirm_of(&[Class::Send]);
    let h = handback(1, HandbackReason::StepFailed);
    let run = run_id(1);
    let mut f = working();
    for n in 1..=LIMITS.items_per_run {
        assert_eq!(
            raise_step(&f, Source::Confirm(&ask), &LIMITS),
            Raise::Run(RunItemKind::Confirm),
            "item {n} of the run"
        );
        push(
            &mut f,
            What::ItemRaised {
                item: item_id(n),
                raised: Raised::Confirm(run.clone()),
            },
        );
    }
    for (label, source) in [
        ("a confirm request", Source::Confirm(&ask)),
        ("a handback request", Source::Handback(&h)),
        ("a show", Source::Show(&run)),
    ] {
        assert_eq!(
            raise_step(&f, source, &LIMITS),
            Raise::End(RunEnd::Budget),
            "{label} after {} items",
            LIMITS.items_per_run
        );
    }
    let mut other = working();
    for n in 1..=LIMITS.items_per_run {
        push(
            &mut other,
            What::ItemRaised {
                item: item_id(n),
                raised: Raised::Confirm(run_id(9)),
            },
        );
    }
    assert_eq!(
        raise_step(&other, Source::Confirm(&ask), &LIMITS),
        Raise::Run(RunItemKind::Confirm),
        "the items of another run do not count"
    );
    let small = Limits {
        items_per_run: 1,
        ..LIMITS
    };
    let mut one = working();
    push(
        &mut one,
        What::ItemRaised {
            item: item_id(1),
            raised: Raised::SignIn(run.clone()),
        },
    );
    assert_eq!(
        raise_step(&one, Source::Confirm(&ask), &small),
        Raise::End(RunEnd::Budget),
        "the limit is read from the limits given"
    );
}

#[test]
fn refusal_ask_again() {
    let mut f = agent_in(State::NeedsSetup, Category::UtilityBillPayment);
    f.doing = Doing::Idle(None);
    assert_eq!(
        raise_step(&f, Source::Setup, &LIMITS),
        Raise::Own(OwnItemKind::Setup),
        "payee has no slot"
    );
    f.slots.push(name("payee"));
    assert_eq!(
        raise_step(&f, Source::Setup, &LIMITS),
        Raise::Nothing,
        "the newest bindings hold every parameter"
    );
    let mut calendar = agent_in(State::Ten, Category::CalendarCleanup);
    calendar.doing = Doing::Idle(None);
    assert_eq!(
        raise_step(&calendar, Source::Setup, &LIMITS),
        Raise::Nothing
    );
}

fn with_past_runs(mut f: Facts<Config>) -> Facts<Config> {
    f.runs = vec![
        PastRun {
            run: run_id(5),
            day: f.today - 1,
            outcome: Outcome::EffectUnknown,
            unknown: 1,
        },
        PastRun {
            run: run_id(4),
            day: f.today - 2,
            outcome: Outcome::EffectUnknown,
            unknown: 1,
        },
    ];
    f
}

#[test]
fn refusal_check_again() {
    let h = handback(1, HandbackReason::EffectUnknown);
    for (answer, result) in [
        (CheckAnswer::TryAgain, HandbackResult::Done),
        (CheckAnswer::WentThrough, HandbackResult::Declined),
    ] {
        let mut f = with_past_runs(working());
        push(
            &mut f,
            What::HoldCleared {
                cause: Cleared::Check {
                    run: run_id(5),
                    answer,
                },
            },
        );
        assert_eq!(
            raise_step(&f, Source::Handback(&h), &LIMITS),
            Raise::Answer(result),
            "the check of the last run was answered {answer}"
        );
    }
    let fresh = with_past_runs(working());
    assert_eq!(
        raise_step(&fresh, Source::Handback(&h), &LIMITS),
        Raise::Run(RunItemKind::Check),
        "no check answered"
    );
    let mut older = with_past_runs(working());
    push(
        &mut older,
        What::HoldCleared {
            cause: Cleared::Check {
                run: run_id(4),
                answer: CheckAnswer::WentThrough,
            },
        },
    );
    assert_eq!(
        raise_step(&older, Source::Handback(&h), &LIMITS),
        Raise::Run(RunItemKind::Check),
        "the answered check is of an earlier run than the last"
    );
}

#[test]
fn only_an_effect_unknown_handback_is_answered_from_a_check() {
    let mut f = with_past_runs(working());
    push(
        &mut f,
        What::HoldCleared {
            cause: Cleared::Check {
                run: run_id(5),
                answer: CheckAnswer::TryAgain,
            },
        },
    );
    for reason in HandbackReason::ALL {
        let h = handback(1, *reason);
        let want = if *reason == HandbackReason::EffectUnknown {
            Raise::Answer(HandbackResult::Done)
        } else {
            Raise::Run(handback_kind(*reason))
        };
        assert_eq!(
            raise_step(&f, Source::Handback(&h), &LIMITS),
            want,
            "{reason}"
        );
    }
}

/// Of two answers to the check of the last run, the newest answers the handback.
#[test]
fn the_newest_check_answer_answers_the_handback() {
    let h = handback(1, HandbackReason::EffectUnknown);
    let mut f = with_past_runs(working());
    for answer in [CheckAnswer::TryAgain, CheckAnswer::WentThrough] {
        push(
            &mut f,
            What::HoldCleared {
                cause: Cleared::Check {
                    run: run_id(5),
                    answer,
                },
            },
        );
    }
    assert_eq!(
        raise_step(&f, Source::Handback(&h), &LIMITS),
        Raise::Answer(HandbackResult::Declined)
    );
}

#[test]
fn a_check_of_the_same_run_is_answered_from_its_row() {
    let h = handback(1, HandbackReason::EffectUnknown);
    let mut f = with_past_runs(working());
    push(
        &mut f,
        What::ItemRaised {
            item: item_id(3),
            raised: Raised::Check(Some(run_id(1))),
        },
    );
    push(
        &mut f,
        What::ItemAnswered {
            item: item_id(3),
            answer: Answered::Check(CheckAnswer::WentThrough),
        },
    );
    assert_eq!(
        raise_step(&f, Source::Handback(&h), &LIMITS),
        Raise::Answer(HandbackResult::Declined)
    );
    let mut other = with_past_runs(working());
    push(
        &mut other,
        What::ItemRaised {
            item: item_id(3),
            raised: Raised::Check(Some(run_id(8))),
        },
    );
    push(
        &mut other,
        What::ItemAnswered {
            item: item_id(3),
            answer: Answered::Check(CheckAnswer::WentThrough),
        },
    );
    assert_eq!(
        raise_step(&other, Source::Handback(&h), &LIMITS),
        Raise::Run(RunItemKind::Check),
        "a check of another run"
    );
}

#[test]
fn a_request_of_a_run_that_is_not_working_is_refused() {
    let ask = confirm_of(&[Class::Send]);
    let idle = agent_in(State::Ten, Category::Email);
    let mut starting = working();
    starting.doing = Doing::Live(Live::Starting);
    let mut stopping = working();
    stopping.doing = Doing::Live(Live::Stopping);
    for (label, f) in [
        ("idle", idle),
        ("starting", starting),
        ("stopping", stopping),
    ] {
        assert_eq!(
            raise_step(&f, Source::Confirm(&ask), &LIMITS),
            Raise::Refused(Cause::State),
            "{label}"
        );
    }
}

#[test]
fn the_sources_of_the_coordinator_raise_their_kind() {
    let mut f = agent_in(State::Ten, Category::Email);
    f.doing = Doing::Idle(None);
    assert_eq!(
        raise_step(&f, Source::Check, &LIMITS),
        Raise::Own(OwnItemKind::Check)
    );
    assert_eq!(
        raise_step(&f, Source::Ready, &LIMITS),
        Raise::Own(OwnItemKind::Ready)
    );
    f.doing = Doing::Queued(None);
    assert_eq!(
        raise_step(&f, Source::Ready, &LIMITS),
        Raise::Own(OwnItemKind::Ready)
    );
    let run = run_id(1);
    assert_eq!(
        raise_step(&working(), Source::Show(&run), &LIMITS),
        Raise::Run(RunItemKind::Show)
    );
}

#[test]
fn the_run_events_that_are_requests_are_sources() {
    let ask = confirm_of(&[Class::Send]);
    let h = handback(1, HandbackReason::Captcha);
    assert!(matches!(
        Source::of(&RunEvent::Confirm(ask)),
        Some(Source::Confirm(_))
    ));
    assert!(matches!(
        Source::of(&RunEvent::Handback(h)),
        Some(Source::Handback(_))
    ));
    assert!(matches!(Source::of(&RunEvent::Other), Some(Source::Other)));
    assert!(Source::of(&RunEvent::Hello).is_none());
    assert!(Source::of(&RunEvent::Ended(None)).is_none());
}

#[test]
fn the_kinds_of_the_six_reasons() {
    use HandbackReason::*;
    let table = [
        (SignIn, RunItemKind::SignIn),
        (SecondFactor, RunItemKind::SignIn),
        (Captcha, RunItemKind::SignIn),
        (AccountLink, RunItemKind::LinkAccount),
        (StepFailed, RunItemKind::Stuck),
        (EffectUnknown, RunItemKind::Check),
    ];
    assert_eq!(table.len(), HandbackReason::ALL.len());
    for (reason, kind) in table {
        assert_eq!(handback_kind(reason), kind, "{reason}");
    }
}

#[test]
fn the_choices_of_the_eight_kinds_stand_safe_one_first() {
    use Choice::*;
    let table = [
        (ItemKind::Confirm, [Reject, Allow]),
        (ItemKind::SignIn, [Decline, Done]),
        (ItemKind::LinkAccount, [Decline, Done]),
        (ItemKind::Stuck, [Decline, Done]),
        (ItemKind::Check, [TryAgain, WentThrough]),
        (ItemKind::Setup, [None, Choose]),
        (ItemKind::Ready, [Skip, Start]),
        (ItemKind::Show, [Decline, Done]),
    ];
    let kinds: Vec<_> = table.iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, ItemKind::ALL);
    for (kind, two) in table {
        assert_eq!(choices(kind), two, "{kind}");
    }
}

#[test]
fn the_results_of_a_handback_by_its_items_choice() {
    use RunItemKind::*;
    for kind in [SignIn, LinkAccount, Stuck, Show] {
        assert_eq!(
            handback_result(kind, Choice::Done),
            Ok(HandbackResult::Done)
        );
        assert_eq!(
            handback_result(kind, Choice::Decline),
            Ok(HandbackResult::Declined)
        );
        assert_eq!(
            handback_result(kind, Choice::TryAgain),
            Err(Cause::Malformed)
        );
    }
    assert_eq!(
        handback_result(Check, Choice::TryAgain),
        Ok(HandbackResult::Done)
    );
    assert_eq!(
        handback_result(Check, Choice::WentThrough),
        Ok(HandbackResult::Declined)
    );
    assert_eq!(handback_result(Check, Choice::Done), Err(Cause::Malformed));
    assert_eq!(
        handback_result(Confirm, Choice::Allow),
        Err(Cause::Malformed)
    );
}

#[test]
fn the_parameters_of_each_category() {
    use Param::*;
    assert_eq!(
        params(Category::UtilityBillPayment),
        [Start, Payee, Instrument]
    );
    assert_eq!(params(Category::Email), [ReplySenders]);
    assert_eq!(
        params(Category::SpreadsheetFill),
        [Workbook, Sheet, Range, Source]
    );
    assert_eq!(params(Category::ShopOrder), [Start, Seller, Instrument]);
    assert!(params(Category::CalendarCleanup).is_empty());
    let named = [
        Category::UtilityBillPayment,
        Category::Email,
        Category::SpreadsheetFill,
        Category::ShopOrder,
        Category::CalendarCleanup,
    ];
    for c in Category::ALL {
        if !named.contains(c) {
            assert!(params(*c).is_empty(), "{c}");
        }
    }
}

/// A confirm request that names an effect that has begun, by the guard's table of this life,
/// shows no card and ends the run with `kind`, also where `raise_step` would end it with
/// `budget`; a request whose effects have not begun, an effect that began in another run, and a
/// record that did not begin the effect raise the item (I3.1, I4.7, ruling IG-32).
#[test]
fn a_confirm_request_for_an_effect_that_has_begun_ends_the_run_with_kind() {
    let ask = confirm_of(&[Class::Send, Class::Payment]);
    let f = working();
    let t = Table::new();
    assert_eq!(
        raise_confirm(&f, &t, &ask, &LIMITS),
        Raise::Run(RunItemKind::Confirm)
    );
    for state in [
        EffectState::Releasing,
        EffectState::Committed,
        EffectState::Unknown,
    ] {
        let begun = t.stored(&record(1, 2, state));
        assert_eq!(
            raise_confirm(&f, &begun, &ask, &LIMITS),
            Raise::End(RunEnd::Kind),
            "{state}"
        );
        let mut full = working();
        for n in 1..=LIMITS.items_per_run {
            push(
                &mut full,
                What::ItemRaised {
                    item: item_id(n),
                    raised: Raised::Confirm(run_id(1)),
                },
            );
        }
        assert_eq!(
            raise_confirm(&full, &begun, &ask, &LIMITS),
            Raise::End(RunEnd::Kind),
            "{state}: the rule of a request outside the closed list comes before the budget"
        );
        let elsewhere = t.stored(&record(2, 2, state));
        assert_eq!(
            raise_confirm(&f, &elsewhere, &ask, &LIMITS),
            Raise::Run(RunItemKind::Confirm),
            "{state}: the effect id began in another run"
        );
    }
    for state in [
        EffectState::Refused,
        EffectState::Cleared,
        EffectState::Held,
        EffectState::Approved,
        EffectState::Rejected,
    ] {
        assert_eq!(
            raise_confirm(&f, &t.stored(&record(1, 2, state)), &ask, &LIMITS),
            Raise::Run(RunItemKind::Confirm),
            "{state} begins nothing"
        );
    }
    assert_eq!(
        raise_confirm(
            &waiting(),
            &t.stored(&record(1, 2, EffectState::Committed)),
            &ask,
            &LIMITS
        ),
        Raise::End(RunEnd::Kind)
    );
    assert_eq!(
        raise_confirm(
            &fixtures::facts(State::Ten).remove(0),
            &t.stored(&record(1, 2, EffectState::Committed)),
            &ask,
            &LIMITS
        ),
        Raise::Refused(Cause::State),
        "a run that is not live is refused as raise_step refuses it"
    );
}

/// A confirm request that names an effect whose newest released decision in this run and life is
/// `reject_once` shows no card and ends the run with `kind`; where the newest is `allow_once`,
/// where the rejection was recorded and not released, where its item lapsed, and where it was
/// another run's, the request raises the item (I3.1, I4.3, I4.7, ruling IG-32).
#[test]
fn a_confirm_request_for_an_effect_whose_newest_released_decision_is_reject_once_ends_the_run_with_kind(
) {
    let ask = confirm_of(&[Class::Send]);
    let f = working();
    let t = Table::new();
    let rejected = released(&t, 1, 1, 1, EffectAnswer::RejectOnce);
    assert_eq!(
        raise_confirm(&f, &rejected, &ask, &LIMITS),
        Raise::End(RunEnd::Kind)
    );
    let allowed_then_rejected = released(
        &released(&t, 1, 1, 1, EffectAnswer::AllowOnce),
        2,
        1,
        1,
        EffectAnswer::RejectOnce,
    );
    assert_eq!(
        raise_confirm(&f, &allowed_then_rejected, &ask, &LIMITS),
        Raise::End(RunEnd::Kind)
    );
    let rejected_then_allowed = released(&rejected, 2, 1, 1, EffectAnswer::AllowOnce);
    assert_eq!(
        raise_confirm(&f, &rejected_then_allowed, &ask, &LIMITS),
        Raise::Run(RunItemKind::Confirm),
        "the newest released decision is allow_once"
    );
    let recorded = t.record(
        &item_id(1),
        &run_id(1),
        &Decision::new(vec![DecisionEntry {
            approval: support::approval(1),
            effect: support::effect_id(1),
            digest: support::digest(1),
            answer: EffectAnswer::RejectOnce,
        }])
        .unwrap(),
    );
    assert_eq!(
        raise_confirm(&f, &recorded, &ask, &LIMITS),
        Raise::Run(RunItemKind::Confirm),
        "a decision that was not released"
    );
    assert_eq!(
        raise_confirm(&f, &rejected.lapse(&item_id(1)), &ask, &LIMITS),
        Raise::Run(RunItemKind::Confirm),
        "the rows of an item that lapsed are not in the table"
    );
    assert_eq!(
        raise_confirm(
            &f,
            &released(&t, 1, 2, 1, EffectAnswer::RejectOnce),
            &ask,
            &LIMITS
        ),
        Raise::Run(RunItemKind::Confirm),
        "another run's rejection"
    );
}
