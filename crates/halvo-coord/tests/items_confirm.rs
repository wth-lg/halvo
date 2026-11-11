#[path = "items_support.rs"]
mod support;

use halvo_coord::items::{confirm_step, ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation};
use halvo_coord::types::{
    Cause, Class, ConfirmAsk, Decision, DecisionEntry, EffectAnswer, LapseCause, Presenter,
};
use support::{approval, ask, ask_of, asked, decision_for, digest, effect_id};

const PAGE: &[Class] = &[Class::Send, Class::Delete];
const NATIVE: &[Class] = &[Class::Send, Class::Payment];

const STATES: [&str; 6] = [
    "shown", "armed", "answered", "recorded", "released", "lapsed",
];

fn presenter_of(classes: &[Class]) -> Presenter {
    if classes == NATIVE {
        Presenter::Native
    } else {
        Presenter::Page
    }
}

fn step(item: Option<&Confirmation>, mv: ConfirmMove<'_>) -> Confirmation {
    confirm_step(item, mv).expect("a move of the table")
}

/// An item of the request `a` driven to `state` through the moves of the table; an answered
/// item was allowed, and a lapsed one lapsed from `shown` with `run_ended`.
fn at(a: &ConfirmAsk, state: &str, from: Presenter) -> Confirmation {
    let allow = decision_for(a, EffectAnswer::AllowOnce, 1);
    let shown = step(None, ConfirmMove::Issue(a));
    if state == "shown" {
        return shown;
    }
    if state == "lapsed" {
        return step(Some(&shown), ConfirmMove::Lapse(LapseCause::RunEnded));
    }
    let armed = step(Some(&shown), ConfirmMove::Arm);
    if state == "armed" {
        return armed;
    }
    let answered = step(
        Some(&armed),
        ConfirmMove::Answer {
            answer: ConfirmAnswer::Allow,
            from,
        },
    );
    if state == "answered" {
        return answered;
    }
    let recorded = step(Some(&answered), ConfirmMove::Record(&allow));
    if state == "recorded" {
        return recorded;
    }
    assert_eq!(state, "released");
    step(Some(&recorded), ConfirmMove::Release)
}

fn state_name(item: &Confirmation) -> &'static str {
    match item.state() {
        ConfirmState::Shown => "shown",
        ConfirmState::Armed => "armed",
        ConfirmState::Answered(_) => "answered",
        ConfirmState::Recorded(_) => "recorded",
        ConfirmState::Released(_) => "released",
        ConfirmState::Lapsed(_) => "lapsed",
    }
}

#[test]
fn row_issue_with_digests_that_hold_shows_the_item() {
    for classes in [PAGE, NATIVE] {
        let a = ask(classes);
        let item = step(None, ConfirmMove::Issue(&a));
        assert_eq!(item.state(), &ConfirmState::Shown);
        assert_eq!(item.presenter(), presenter_of(classes));
        let asked: Vec<_> = item.asked().to_vec();
        assert_eq!(
            asked,
            vec![(effect_id(1), digest(1)), (effect_id(2), digest(2))],
            "the effects and digests of the request, in its order"
        );
    }
}

#[test]
fn row_issue_with_a_digest_that_does_not_hold_lapses_with_digest() {
    for bad in 1..=3 {
        let effects = (1..=3)
            .map(|n| asked(n, Class::Send, n, n != bad))
            .collect();
        let a = ask_of(1, 1, effects);
        let item = step(None, ConfirmMove::Issue(&a));
        assert_eq!(
            item.state(),
            &ConfirmState::Lapsed(LapseCause::Digest),
            "effect {bad} of 3 does not hold"
        );
    }
}

#[test]
fn row_arm_arms_a_shown_item() {
    for classes in [PAGE, NATIVE] {
        let a = ask(classes);
        let item = step(
            Some(&at(&a, "shown", presenter_of(classes))),
            ConfirmMove::Arm,
        );
        assert_eq!(item.state(), &ConfirmState::Armed);
        assert_eq!(item.presenter(), presenter_of(classes));
    }
}

#[test]
fn row_answer_from_the_items_presenter_answers_an_armed_item() {
    for classes in [PAGE, NATIVE] {
        let from = presenter_of(classes);
        let a = ask(classes);
        for answer in [ConfirmAnswer::Reject, ConfirmAnswer::Allow] {
            let item = step(
                Some(&at(&a, "armed", from)),
                ConfirmMove::Answer { answer, from },
            );
            assert_eq!(item.state(), &ConfirmState::Answered(answer));
        }
    }
}

#[test]
fn row_record_takes_the_decision_of_the_answer() {
    let a = ask(PAGE);
    for (answer, once) in [
        (ConfirmAnswer::Reject, EffectAnswer::RejectOnce),
        (ConfirmAnswer::Allow, EffectAnswer::AllowOnce),
    ] {
        let armed = at(&a, "armed", Presenter::Page);
        let answered = step(
            Some(&armed),
            ConfirmMove::Answer {
                answer,
                from: Presenter::Page,
            },
        );
        let d = decision_for(&a, once, 7);
        let item = step(Some(&answered), ConfirmMove::Record(&d));
        assert_eq!(item.state(), &ConfirmState::Recorded(d.clone()));
    }
}

#[test]
fn row_record_takes_the_entries_in_any_order() {
    let a = ask(PAGE);
    let answered = at(&a, "answered", Presenter::Page);
    let mut entries = decision_for(&a, EffectAnswer::AllowOnce, 1)
        .approvals()
        .to_vec();
    entries.reverse();
    let d = Decision::new(entries).unwrap();
    let item = step(Some(&answered), ConfirmMove::Record(&d));
    assert_eq!(item.state(), &ConfirmState::Recorded(d));
}

#[test]
fn row_release_releases_a_recorded_item_with_its_decision() {
    let a = ask(NATIVE);
    let recorded = at(&a, "recorded", Presenter::Native);
    let item = step(Some(&recorded), ConfirmMove::Release);
    assert_eq!(
        item.state(),
        &ConfirmState::Released(decision_for(&a, EffectAnswer::AllowOnce, 1))
    );
}

#[test]
fn row_lapse_takes_each_of_the_six_causes_from_shown_or_armed() {
    let a = ask(PAGE);
    for state in ["shown", "armed"] {
        for cause in LapseCause::ALL {
            let item = step(
                Some(&at(&a, state, Presenter::Page)),
                ConfirmMove::Lapse(*cause),
            );
            assert_eq!(
                item.state(),
                &ConfirmState::Lapsed(*cause),
                "{state} {cause}"
            );
        }
    }
}

#[test]
fn row_lapse_takes_four_causes_from_answered() {
    let a = ask(PAGE);
    for cause in [
        LapseCause::RunEnded,
        LapseCause::Cancelled,
        LapseCause::NoRecord,
        LapseCause::AppEnded,
    ] {
        let item = step(
            Some(&at(&a, "answered", Presenter::Page)),
            ConfirmMove::Lapse(cause),
        );
        assert_eq!(item.state(), &ConfirmState::Lapsed(cause), "{cause}");
    }
}

#[test]
fn row_lapse_takes_run_ended_and_app_ended_from_recorded() {
    let a = ask(PAGE);
    for cause in [LapseCause::RunEnded, LapseCause::AppEnded] {
        let item = step(
            Some(&at(&a, "recorded", Presenter::Page)),
            ConfirmMove::Lapse(cause),
        );
        assert_eq!(item.state(), &ConfirmState::Lapsed(cause), "{cause}");
    }
}

/// Every move that the tests try on an item of the request `a`, each with a label.
fn moves<'a>(
    a: &'a ConfirmAsk,
    allow: &'a Decision,
    reject: &'a Decision,
) -> Vec<(String, ConfirmMove<'a>)> {
    let mut all = vec![
        ("issue".to_owned(), ConfirmMove::Issue(a)),
        ("arm".to_owned(), ConfirmMove::Arm),
        ("record allow".to_owned(), ConfirmMove::Record(allow)),
        ("record reject".to_owned(), ConfirmMove::Record(reject)),
        ("release".to_owned(), ConfirmMove::Release),
    ];
    for answer in [ConfirmAnswer::Reject, ConfirmAnswer::Allow] {
        for from in [Presenter::Page, Presenter::Native] {
            all.push((
                format!("answer {answer:?} from {from}"),
                ConfirmMove::Answer { answer, from },
            ));
        }
    }
    for cause in LapseCause::ALL {
        all.push((format!("lapse {cause}"), ConfirmMove::Lapse(*cause)));
    }
    all
}

fn kind(mv: &ConfirmMove<'_>) -> &'static str {
    match mv {
        ConfirmMove::Issue(_) => "issue",
        ConfirmMove::Arm => "arm",
        ConfirmMove::Answer { .. } => "answer",
        ConfirmMove::Record(_) => "record",
        ConfirmMove::Release => "release",
        ConfirmMove::Lapse(_) => "lapse",
    }
}

/// The answer that I4.2 gives a move on an item in `state` that was allowed from the page.
fn contract(
    state: &str,
    presenter: Presenter,
    mv: &ConfirmMove<'_>,
) -> Result<&'static str, Cause> {
    use LapseCause::*;
    match (state, mv) {
        ("none", ConfirmMove::Issue(_)) => Ok("shown"),
        ("none", _) => Err(Cause::State),
        ("shown", ConfirmMove::Arm) => Ok("armed"),
        (s, ConfirmMove::Answer { from, .. }) => match s {
            "answered" | "recorded" | "released" | "lapsed" => Err(Cause::Stale),
            _ if *from != presenter => Err(Cause::Presenter),
            "shown" => Err(Cause::Early),
            _ => Ok("answered"),
        },
        ("answered", ConfirmMove::Record(d)) => {
            if d.approvals()[0].answer == EffectAnswer::AllowOnce {
                Ok("recorded")
            } else {
                Err(Cause::State)
            }
        }
        ("recorded", ConfirmMove::Release) => Ok("released"),
        ("shown" | "armed", ConfirmMove::Lapse(_)) => Ok("lapsed"),
        ("answered", ConfirmMove::Lapse(RunEnded | Cancelled | NoRecord | AppEnded)) => {
            Ok("lapsed")
        }
        ("recorded", ConfirmMove::Lapse(RunEnded | AppEnded)) => Ok("lapsed"),
        _ => Err(Cause::State),
    }
}

/// Checks every move of the kind `move_kind` on an item in `state`, for a page request and a
/// native one, against the contract.
fn pair(state: &str, move_kind: &str) {
    for classes in [PAGE, NATIVE] {
        let presenter = presenter_of(classes);
        let a = ask(classes);
        let allow = decision_for(&a, EffectAnswer::AllowOnce, 1);
        let reject = decision_for(&a, EffectAnswer::RejectOnce, 1);
        let item = (state != "none").then(|| at(&a, state, presenter));
        let mut tried = 0;
        for (label, mv) in moves(&a, &allow, &reject) {
            if kind(&mv) != move_kind {
                continue;
            }
            tried += 1;
            let want = contract(state, presenter, &mv);
            let got = confirm_step(item.as_ref(), mv).map(|i| state_name(&i));
            assert_eq!(got, want, "{state} + {label} for a {presenter} item");
        }
        assert!(tried > 0, "no move of the kind {move_kind}");
    }
}

macro_rules! pairs {
    ($($test:ident: $state:literal $move_kind:literal;)+) => {
        $(
            #[test]
            fn $test() {
                pair($state, $move_kind);
            }
        )+
    };
}

pairs! {
    pair_none_arm: "none" "arm";
    pair_none_answer: "none" "answer";
    pair_none_record: "none" "record";
    pair_none_release: "none" "release";
    pair_none_lapse: "none" "lapse";
    pair_shown_issue: "shown" "issue";
    pair_shown_answer: "shown" "answer";
    pair_shown_record: "shown" "record";
    pair_shown_release: "shown" "release";
    pair_armed_issue: "armed" "issue";
    pair_armed_arm: "armed" "arm";
    pair_armed_answer_from_another_presenter: "armed" "answer";
    pair_armed_record: "armed" "record";
    pair_armed_release: "armed" "release";
    pair_answered_issue: "answered" "issue";
    pair_answered_arm: "answered" "arm";
    pair_answered_answer: "answered" "answer";
    pair_answered_record_of_another_answer: "answered" "record";
    pair_answered_release: "answered" "release";
    pair_answered_lapse_with_wait_limit_or_digest: "answered" "lapse";
    pair_recorded_issue: "recorded" "issue";
    pair_recorded_arm: "recorded" "arm";
    pair_recorded_answer: "recorded" "answer";
    pair_recorded_record: "recorded" "record";
    pair_recorded_lapse_with_another_cause: "recorded" "lapse";
    pair_released_issue: "released" "issue";
    pair_released_arm: "released" "arm";
    pair_released_answer: "released" "answer";
    pair_released_record: "released" "record";
    pair_released_release: "released" "release";
    pair_released_lapse: "released" "lapse";
    pair_lapsed_issue: "lapsed" "issue";
    pair_lapsed_arm: "lapsed" "arm";
    pair_lapsed_answer: "lapsed" "answer";
    pair_lapsed_record: "lapsed" "record";
    pair_lapsed_release: "lapsed" "release";
    pair_lapsed_lapse: "lapsed" "lapse";
}

#[test]
fn every_state_and_move_kind_is_checked() {
    let mut named = 0;
    for state in std::iter::once("none").chain(STATES) {
        for move_kind in ["issue", "arm", "answer", "record", "release", "lapse"] {
            pair(state, move_kind);
            named += 1;
        }
    }
    assert_eq!(named, 42);
}

#[test]
fn a_record_whose_rows_are_not_those_of_the_answer_is_refused() {
    let a = ask(PAGE);
    let answered = at(&a, "answered", Presenter::Page);
    let good = decision_for(&a, EffectAnswer::AllowOnce, 1)
        .approvals()
        .to_vec();
    let mut other_effect = good.clone();
    other_effect[1].effect = effect_id(9);
    let mut other_digest = good.clone();
    other_digest[0].digest = digest(9);
    let mut other_answer = good.clone();
    other_answer[1].answer = EffectAnswer::RejectOnce;
    let missing = good[..1].to_vec();
    let mut extra = good.clone();
    extra.push(DecisionEntry {
        approval: approval(3),
        effect: effect_id(3),
        digest: digest(3),
        answer: EffectAnswer::AllowOnce,
    });
    for (label, entries) in [
        ("another effect", other_effect),
        ("another digest", other_digest),
        ("another answer", other_answer),
        ("an entry missing", missing),
        ("an entry more", extra),
    ] {
        let d = Decision::new(entries).unwrap();
        assert_eq!(
            confirm_step(Some(&answered), ConfirmMove::Record(&d)).map(|i| state_name(&i)),
            Err(Cause::State),
            "{label}"
        );
    }
}

#[test]
fn refusal_answer_twice() {
    for classes in [PAGE, NATIVE] {
        let from = presenter_of(classes);
        let a = ask(classes);
        for state in ["answered", "recorded", "released"] {
            let item = at(&a, state, from);
            for answer in [ConfirmAnswer::Allow, ConfirmAnswer::Reject] {
                assert_eq!(
                    confirm_step(Some(&item), ConfirmMove::Answer { answer, from })
                        .map(|i| state_name(&i)),
                    Err(Cause::Stale),
                    "{answer:?} on a {from} item that is {state}"
                );
            }
        }
    }
}

#[test]
fn refusal_answer_early() {
    for classes in [PAGE, NATIVE] {
        let from = presenter_of(classes);
        let a = ask(classes);
        let shown = at(&a, "shown", from);
        for answer in [ConfirmAnswer::Allow, ConfirmAnswer::Reject] {
            assert_eq!(
                confirm_step(Some(&shown), ConfirmMove::Answer { answer, from })
                    .map(|i| state_name(&i)),
                Err(Cause::Early)
            );
        }
        assert_eq!(shown.state(), &ConfirmState::Shown, "the item stays shown");
        let armed = step(Some(&shown), ConfirmMove::Arm);
        assert_eq!(
            armed.state(),
            &ConfirmState::Armed,
            "and can still be armed"
        );
    }
}

#[test]
fn refusal_answer_page_native() {
    let a = ask(NATIVE);
    for state in ["shown", "armed"] {
        let item = at(&a, state, Presenter::Native);
        for answer in [ConfirmAnswer::Allow, ConfirmAnswer::Reject] {
            assert_eq!(
                confirm_step(
                    Some(&item),
                    ConfirmMove::Answer {
                        answer,
                        from: Presenter::Page
                    }
                )
                .map(|i| state_name(&i)),
                Err(Cause::Presenter),
                "{answer:?} from the page on a native item that is {state}"
            );
        }
    }
    let page = at(&ask(PAGE), "armed", Presenter::Page);
    assert_eq!(
        confirm_step(
            Some(&page),
            ConfirmMove::Answer {
                answer: ConfirmAnswer::Allow,
                from: Presenter::Native
            }
        )
        .map(|i| state_name(&i)),
        Err(Cause::Presenter),
        "and a dialog's answer on a page item"
    );
}

#[test]
fn no_sequence_of_arm_and_lapse_moves_reaches_an_answer() {
    let a = ask(PAGE);
    let mut seen = 0;
    let mut frontier = vec![step(None, ConfirmMove::Issue(&a))];
    for _ in 0..4 {
        let mut next = Vec::new();
        for item in &frontier {
            let mut tries = vec![ConfirmMove::Arm];
            tries.extend(LapseCause::ALL.iter().map(|c| ConfirmMove::Lapse(*c)));
            for mv in tries {
                if let Ok(after) = confirm_step(Some(item), mv) {
                    seen += 1;
                    assert!(
                        matches!(
                            after.state(),
                            ConfirmState::Shown | ConfirmState::Armed | ConfirmState::Lapsed(_)
                        ),
                        "an arm or a lapse reached {:?}",
                        after.state()
                    );
                    next.push(after);
                }
            }
        }
        frontier = next;
    }
    assert!(seen > 0);
}

/// `needs_you` counts a confirmation while it is `shown` or `armed`; an answered item is still
/// open and no longer counts (I4.2, ruling H-5).
#[test]
fn an_item_waits_for_the_person_only_while_shown_or_armed() {
    let a = ask(PAGE);
    for (state, waits, open) in [
        ("shown", true, true),
        ("armed", true, true),
        ("answered", false, true),
        ("recorded", false, true),
        ("released", false, false),
        ("lapsed", false, false),
    ] {
        let item = at(&a, state, Presenter::Page);
        assert_eq!(item.state().waits_for_person(), waits, "{state} waits");
        assert_eq!(item.state().is_open(), open, "{state} is open");
    }
}

#[test]
fn the_answers_of_an_item_cover_each_effect_with_its_digest() {
    let a = ask(NATIVE);
    let item = at(&a, "shown", Presenter::Native);
    for (answer, once) in [
        (ConfirmAnswer::Reject, EffectAnswer::RejectOnce),
        (ConfirmAnswer::Allow, EffectAnswer::AllowOnce),
    ] {
        let got: Vec<_> = item
            .answers(answer)
            .into_iter()
            .map(|x| (x.effect, x.digest, x.answer))
            .collect();
        assert_eq!(
            got,
            vec![
                (effect_id(1), digest(1), once),
                (effect_id(2), digest(2), once)
            ]
        );
    }
}

#[test]
fn a_confirm_answer_is_a_choice_of_the_kind_confirm() {
    use halvo_coord::types::Choice;
    assert_eq!(
        ConfirmAnswer::from_choice(Choice::Allow),
        Ok(ConfirmAnswer::Allow)
    );
    assert_eq!(
        ConfirmAnswer::from_choice(Choice::Reject),
        Ok(ConfirmAnswer::Reject)
    );
    for c in Choice::ALL {
        if !matches!(c, Choice::Allow | Choice::Reject) {
            assert_eq!(ConfirmAnswer::from_choice(*c), Err(Cause::Malformed), "{c}");
        }
    }
    assert_eq!(ConfirmAnswer::Allow.once(), EffectAnswer::AllowOnce);
    assert_eq!(ConfirmAnswer::Reject.once(), EffectAnswer::RejectOnce);
}
