#[path = "items_support.rs"]
mod support;

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
use std::num::NonZeroU32;
use support::{ask_of, asked, item_id, name, run_id};

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

fn handback(run: u32, reason: HandbackReason) -> HandbackAsk {
    HandbackAsk {
        run: run_id(run),
        agent: support::agent_id(1),
        reason,
        origin: None,
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
